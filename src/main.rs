use petgraph::prelude::*;
use petgraph::visit::IntoNodeReferences;
use petgraph::*;
use std::io;
use std::io::Write;
use structopt::*;
use derive_new::*;
use clap::AppSettings;
use enum_dispatch::*;
use enum_from_str::ParseEnumVariantError;
use enum_from_str_derive::FromStr;
use serde::{Serialize,
            Deserialize,
            Serializer,
            Deserializer,
            de::Visitor};

use crate::cmd::Executable;

// TODO: Major Features
// 1. enums for edge function calls
// 2. Name list & name validation
// 3. Node and edge validation
// 4. Tests
// 5. Redundancy when editing/pruning/saving
// 6. Proper error/Ok propogation
// 7. Fork ropey::Rope and implement serialize/deserialize
// 8. Switch to bincode serialization format, json should only be for debugging

static ROPE_EXT: &str = ".rope";
static TREE_EXT: &str = ".tree";
static UNKNOWN: &str = "unknown command, type help for more info";
static _NONAME: &str = "no name provided";
static HELP: &str = "
    A tree based dialogue editor

    commands:
    help - display this help menu
    new - create a new project, node, or edge
    project - get the current project info
    node - get the current node info

    exit application:
    q 
    quit 
    exit";
static _UNIMPLEMENTED: &str = "unimplemented command";
static SUCCESS: &str = "success\r\n";

/// typedef representing a section of text in a rope. This section contains a start and end index,
/// stored in an array. The first element should always be smaller than the second
pub type Section = [usize; 2];

/// Struct storing the information for a player choice. Stored in the edges of a dialogue tree
#[derive(new, Serialize, Deserialize)] 
pub struct Choice {
   text: Section,
   action: action::Kind
}

/// Wrapper for ropey::Rope struct to implement the serializable trait via the Rope::write_to
/// method
#[derive(new)]
pub struct SerialRope{
    rope: ropey::Rope,
}

impl Serialize for SerialRope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> 
    where
        S: Serializer
    {
        let rope_string = self.rope.to_string();
        serializer.serialize_str(rope_string.as_str())
    }
}

impl std::str::FromStr for SerialRope {
    type Err = ();
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(SerialRope::new(ropey::Rope::from_str(s)))    
    }
}

impl<'de> Deserialize<'de> for SerialRope {
    fn deserialize<D>(deserializer: D) -> Result<SerialRope, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct RopeVisitor;

        impl<'de> Visitor<'de> for RopeVisitor {
            type Value = SerialRope;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("text rope as string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) 
            -> std::result::Result<Self::Value, E> {
                value.parse().map_err(|_| serde::de::Error::custom(""))
            }
        }
        deserializer.deserialize_str(RopeVisitor)
    }
}

#[derive(new, Serialize, Deserialize)]
pub struct EditorState {
    tree: petgraph::graph::DiGraph<Section, Choice>,
    rope: SerialRope,
    name: String,
}
impl EditorState {
    fn default() -> Self {
        EditorState {
            tree: graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
            rope: SerialRope::new(ropey::Rope::new()),
            name: String::new(),
        }
    }
}

mod cmd {
    use super::*;

    /// Unified result type for propogating errors in cmd methods
    type Result = std::result::Result<&'static str, cmd::Error>;

    /// Trait to allow structopt generated 
    #[enum_dispatch]
    pub trait Executable {
        fn execute(&self, state: &mut EditorState) -> cmd::Result;
    }

    /// A tree based dialogue editor
    // NoBinaryName is set so that the first arg is not parsed as binary name when using
    // StructOpt::from_iter_safe
    // name is set as "" to prevent usage help from recommending to start commands with "dialogue_tree"
    #[enum_dispatch(Executable)]
    #[derive(StructOpt)]
    #[structopt(name="", setting = AppSettings::NoBinaryName)]
    pub enum Parse {
        New(new::Parse),
        Save(Save),
        Load(Load),
        List(List),
    }

    mod new {
        use super::*;

        /// Create new things 
        #[enum_dispatch(Executable)]
        #[derive(StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub enum Parse{
            Project(new::Project),
            Node(new::Node),
            Edge(new::Edge),
        }
        
        /// Create a new project
        ///
        /// A project is made up of a text rope storing all dialogue text, a hashtable storing
        /// variable or user defined values, and a graph representing the narrative. Nodes of the
        /// graph represent dialogues from characters in the story, and nodes represent the 
        /// actions of the player.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Project {
            /// The name of the project
            name: String,

            /// Determine if the project should be loaded as the active project after creation. If
            /// any unsaved changes in the current project will be discarded.
            #[structopt(short, long)]
            set_active: bool,
        }
        impl Executable for Project {
            // Create a new file on disk for new project, optionally set it as active in the editor
            // state
            fn execute(&self, state: &mut EditorState) -> cmd::Result {
                let mut new_state = EditorState::new(
                    graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
                    SerialRope::new(ropey::Rope::new()),
                    self.name.clone(),
                    );

                cmd::Save::new().execute(&mut new_state)?;
                
                if self.set_active {
                    *state = new_state;
                    Ok("New project created and set as active")
                } else {
                    Ok("New project created on disk")
                }
            }
        }

        /// Create a new node in the dialogue tree
        ///
        /// A node represents a text a segment of dialogue from a character.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Node {
           /// The speaker for this node 
           speaker: String,
           /// The text or action for this node 
           dialogue: String,
        }
        impl Executable for Node {
            /// Create a new section of text on the text rope, and then make a new node on the
            /// tree pointing to the section
            fn execute(&self, state: &mut EditorState) -> cmd::Result {
                let start = state.rope.rope.len_chars();
                state.rope.rope.append(ropey::Rope::from(format!("{}::{}", self.speaker, self.dialogue)));
                let end = state.rope.rope.len_chars();
                state.tree.add_node([start, end]);
                Ok(SUCCESS)
            }
        }

        /// Create a new edge in the dialogue tree
        ///
        /// An edge represents an action from the player that connects two nodes
        #[derive(new, StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Edge {
            /// dialogue node that this action originates from
            start_node_idx: u32,
            /// dialogue node that this action will lead to 
            end_node_idx: u32,
            /// Action text or dialogue
            text: String,
            /// Special types for actions that may edit variables  
            ///
            /// An example action is if the user is prompted to input the name of their character,
            /// or if the user picks up a variable item from a table and stores it in their
            /// inventory
            action: Option<action::Kind>,
        }

        impl Executable for Edge {
            fn execute(&self, state: &mut EditorState) -> cmd::Result {
                let start = state.rope.rope.len_chars();
                state.rope.rope.append(ropey::Rope::from(self.text.clone()));
                let end = state.rope.rope.len_chars();
                state.tree.add_edge(
                    NodeIndex::from(self.start_node_idx),
                    NodeIndex::from(self.end_node_idx),
                    Choice::new([start, end], self.action.unwrap_or_default()),
                );
                Ok(SUCCESS)
            }
        }
    }

    /// Save the current project
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Save {}

    impl Executable for Save {
        fn execute(&self, state: &mut EditorState) -> cmd::Result { 
            let json = serde_json::to_string(&state).unwrap();
            std::fs::write(state.name.clone() + TREE_EXT, json)?;
            Ok(SUCCESS)
        }
    }

    /// Load a project from disk, will overwrite unsaved changes
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Load {
        name: String,
    }

    impl Executable for Load {
        fn execute(&self, state: &mut EditorState) -> cmd::Result {
            let new_state: EditorState = serde_json::from_reader(
                std::io::BufReader::new(std::fs::File::open(self.name.clone() + TREE_EXT)?),
            )?;
            *state = new_state;
            Ok(SUCCESS)
        }
    }

    /// Print all nodes, edges, and associated text
    ///
    /// Prints all nodes in index order (not necessarily the order they would appear when
    /// traversing the dialogue tree). Under each node definiton, a list of the outgoing edges from
    /// that node will be listed. This will show the path to the next dialogue option from any
    /// node, and the choice/action text associated with that edge.
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct List {}

    impl Executable for List {
        fn execute(&self, state: &mut EditorState) -> cmd::Result {
            let node_iter = state.tree.node_references();
            node_iter.for_each(|n| {
                // Print node identifier, node text, and then all edges
                println!("{:#?} : {}", n.0, state.rope.rope.slice(n.1[0]..n.1[1]));
                state
                    .tree
                    .edges_directed(n.0, petgraph::Direction::Outgoing)
                    .for_each(|e| {
                        println!(
                            "--> {:#?} : {} : {} ",
                            e.target(),
                            e.id().index(),
                            state.rope.rope.slice(e.weight().text[0]..e.weight().text[1])
                        )
                    });
            });
            Ok(SUCCESS)
        }
    }

    /// Start reading the currently loaded project from the start node
    // TODO: This is a prototype for read functionality, likely needs to be moved in the future
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Read {}

    impl Executable for Read {
        fn execute(&self, state: &mut EditorState) -> cmd::Result {
            println!("reader mode:");
            let node_idx = graph::node_index(0);
            let iter = state.tree.edges_directed(node_idx, petgraph::Direction::Outgoing);
            let _target_list = iter
                .enumerate()
                .map(|(i, e)| {
                    println!(
                        "{}. {}",
                        i,
                        state.rope.rope.slice(e.weight().text[0]..e.weight().text[1])
                    );
                    e.target()
            });
            Ok(SUCCESS)
        }
    }

    /// Error types for different commands
    // TODO: remove if not needed
    #[derive(new, Debug, Default)]
    pub struct Error {
        details: String,
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "{}", self.details)
        }
    }

    impl std::error::Error for Error {
        fn description(&self) -> &str {
            &self.details
        }
    }

    /// Placeholder from implementation for io Errors
    // TODO: use non-default error type for io errors
    impl From<std::io::Error> for Error {
        fn from(_err: std::io::Error) -> Self {
            Error::default()
        }
    }

    /// Placeholder from implementation for str->int conversion errors
    // TODO: use non-default error type for str->int conversion errors
    impl From<std::num::ParseIntError> for Error {
        fn from(_err: std::num::ParseIntError) -> Self {
            Error::default()
        }
    }

    /// Placeholder from implementation for serde serialization errors
    // TODO: use non-default error type for serde serialization errors
    impl From<serde_json::Error> for Error {
        fn from(_err: serde_json::Error) -> Self {
            println!("{}", _err);
            Error::default()
        }
    }

    pub mod util {
        use super::*;

        /// Helper method to prompt the user for input
        ///
        /// User input is stored into the provided buffer
        pub fn prompt_input(buf: &mut String) {
            // Print input prompt
            print!(">> ");

            // get next command from the user
            io::stdout().flush().unwrap();
            io::stdin().read_line(buf).expect("Failed to read line");
        }

        /// Helper method to remove text from the middle of the text rope, and adjust the node
        /// and edge weights accordingly
        ///
        /// Note that for prune to work properly, the section of text MUST not be referenced by
        /// any of the nodes or edges in the tree.
        pub fn prune(
            range: Section,
            rope: &mut ropey::Rope,
            tree: &mut graph::DiGraph<Section, Choice>,
        ) {
            // Implementation notes:
            //  1. Code is written to be branchless in case of a very large tree
            //  2. Range is non-inclusive, which means that num_removed has to be 1 larger than the
            //     difference between the ranges
            //  3. Currently it is just blindly assumed that range[1] > range[0]
            let num_removed = range[1] - range[0] + 1;
            assert!(num_removed > 0);

            rope.remove(range[0]..range[1]);

            // Iterate through each node & edge, and shift the range left by the number of removed
            // characters
            tree.node_weights_mut().for_each(|w| {
                let shift = num_removed - (w[0] >= range[1]) as usize;
                *w = [w[0] - shift, w[1] - shift]
            });

            tree.edge_weights_mut().for_each(|w| {
                let shift = num_removed - (w.text[0] >= range[1]) as usize;
                w.text = [w.text[0] - shift, w.text[1] - shift]
            });
        }
    }
}

mod action {
    use super::*; 
    /// Kind defines the types of actions available for dialogue tree choices
    #[derive(Serialize, Deserialize, FromStr, Clone, Copy)]
    pub enum Kind {
        /// No action
        Inactive,
        /// Stores a specific word or phrase to the Hashtable with a provided key, if the key is
        /// already used, the value is updated. If the key is new, a new entry in the table is
        /// created
        Store,
        /// Similar function to store, except the word or phrase is provided by the user
        StorePrompt,
    }

    impl Default for Kind {
        fn default() -> Self {
            Kind::Inactive
        }
    }
}

fn main() {
    let mut cmd_buf = String::with_capacity(1000);

    // TODO: clean up state init
    let mut state = EditorState::default();
    loop {
        // print default header
        println!("------------");
        println!("project: {}", state.name);
        println!("------------");

        cmd::util::prompt_input(&mut cmd_buf);

        let cmds = shellwords::split(&cmd_buf).unwrap();
        let cmd_result = cmd::Parse::from_iter_safe(cmds);

        // Handle results/errors
        match cmd_result {
            Ok(v) => {
                match v.execute(&mut state) {
                    Ok(r) => println!("{}", r),
                    Err(f) => println!("{}", f), 
                }
            }
            Err(e) => println!("{}", e),
        }

        // clear input buffers before starting next input loop
        cmd_buf.clear();
    }
}
