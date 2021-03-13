use clap::AppSettings;
use derive_new::*;
use enum_dispatch::*;
use enum_from_str::ParseEnumVariantError;
use enum_from_str_derive::FromStr;
use hashbrown::HashMap;
use petgraph::prelude::*;
use petgraph::visit::IntoNodeReferences;
use petgraph::*;
use serde::{Deserialize, Serialize}; 
use std::io;
use std::io::Write;
use structopt::*;

use crate::cmd::Executable;

// TODO: Major Features
// 0. String defragmenter for prune() now that we aren't using a text rope
// 1. Actionable edge function calls, currently impossible to do anything with action::Kind enum
// 2. Node and edge validation
// 3. Tests
// 4. Redundancy when editing/pruning/saving
// 5. Proper error/Ok propogation
// 6. Fork ropey::Rope and implement serialize/deserialize, removing the need for SerialRope
// 7. Switch to bincode serialization format, json should only be for debugging

static TREE_EXT: &str = ".tree";
static _NONAME: &str = "no name provided";
static _UNIMPLEMENTED: &str = "unimplemented command";
static SUCCESS: &str = "success\r\n";
static TOKEN: &str = "::";

/// Top level data structure for storing a dialogue tree
///
/// This struct contains the tree representing the dialogue nodes and player actions connecting
/// them, the rope which stores all text in a tightly packed manner, and a hashtable for storing
/// variables such as player names, conditionals, etc.
#[derive(new, Serialize, Deserialize)]
pub struct DialogueTreeData {
    tree: petgraph::graph::DiGraph<Section, Choice>,
    text: String,
    name_table: HashMap<String, String>,
    name: String,
}

impl DialogueTreeData {
    fn default() -> Self {
        DialogueTreeData {
            tree: graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
            text: String::with_capacity(8192),
            name_table: HashMap::new(),
            name: String::new(),
        }
    }
}

/// typedef representing a section of text in a rope. This section contains a start and end index,
/// stored in an array. The first element should always be smaller than the second
pub type Section = [usize; 2];

/// Struct storing the information for a player choice. Stored in the edges of a dialogue tree
#[derive(new, Serialize, Deserialize, Clone, Copy)]
pub struct Choice {
    text: Section,
    action: action::Kind,
}

mod cmd {
    use super::*;

    /// Unified result type for propogating errors in cmd methods
    type Result = std::result::Result<&'static str, cmd::Error>;

    /// Trait to allow structopt generated
    #[enum_dispatch]
    pub trait Executable {
        fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result;
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
        Edit(edit::Parse),
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
        pub enum Parse {
            Project(new::Project),
            Node(new::Node),
            Edge(new::Edge),
            Name(new::Name),
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
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                let mut new_state = DialogueTreeData::new(
                    graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
                    String::with_capacity(8192),
                    HashMap::new(),
                    self.name.clone(),
                );

                cmd::Save::new().execute(&mut new_state)?;

                if self.set_active {
                    *data = new_state;
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
            /// The speaker for this node. The speaker name must be a key in the name table
            speaker: String,
            /// The text or action for this node
            dialogue: String,
        }

        impl Executable for Node {
            /// Create a new section of text on the text rope, and then make a new node on the
            /// tree pointing to the section
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                // verify the speaker name is valid
                data.name_table.get(&self.speaker).ok_or_else(cmd::Error::default)?;
                let start = data.text.len();
                data.text.push_str(&format!(
                    "{}::{}",
                    self.speaker, self.dialogue
                ));
                let end = data.text.len();
                data.tree.add_node([start, end]);
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
            start_index: u32,
            /// dialogue node that this action will lead to
            end_index: u32,
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
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                let start = data.text.len();
                data.text.push_str(&self.text);
                let end = data.text.len();
                data.tree.add_edge(
                    NodeIndex::from(self.start_index),
                    NodeIndex::from(self.end_index),
                    Choice::new([start, end], self.action.unwrap_or_default()),
                );
                Ok(SUCCESS)
            }
        }

        /// Create a new name for use in dialogue nodes and actions
        ///
        /// A name represents some variable that may be substituted into the text. Examples
        /// include player names, pronouns, and character traits 
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Name {
            /// The keyword to reference the value with in the text 
            key: String,
            /// Value to store, able be updated by player actions
            value: String,
        }
        impl Executable for Name {
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                // Check that the key doesn't already exist, since we want new to not overwrite
                // values. The user can use edit commands for that
                if data.name_table.get(&self.key).is_some() {
                    Ok("Key already exists")
                } else {
                    data.name_table.insert(self.key.clone(), self.value.clone());
                    Ok(SUCCESS)
                }
            }
        }
    }

    mod edit {
        use super::*;

        /// Edit existing things
        #[enum_dispatch(Executable)]
        #[derive(StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub enum Parse {
            Node(edit::Node),
            Edge(edit::Edge),
            Name(edit::Name)
        }

        /// Edit the contents of a node in the dialogue tree
        ///
        /// A node represents a text a segment of dialogue from a character.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Node {
            /// Id of the node to edit
            node_id: usize,
            /// The speaker for this node
            speaker: String,
            /// The text or action for this node
            dialogue: String,
        }
        impl Executable for Node {
            /// Edit Node
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                let node_index = NodeIndex::new(self.node_id);
                let old_weight = *data
                    .tree
                    .node_weight_mut(node_index)
                    .ok_or_else(cmd::Error::default)?;

                let start = data.text.len();
                data.text.push_str(&format!(
                    "{}::{}",
                    self.speaker, self.dialogue
                ));
                let end = data.text.len();

                // tree must be pruned before the tree is modified to preserve a valid tree
                // if prune fails
                util::prune(old_weight, &mut data.text, &mut data.tree)?;
                let node = data
                    .tree
                    .node_weight_mut(node_index)
                    .ok_or_else(cmd::Error::default)?;
                *node = [start, end];
                Ok(SUCCESS)
            }
        }

        /// Edit the contents and connections of an edge in the dialogue tree
        ///
        /// Note: Editing the source or target node will change the edge index
        #[derive(new, StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Edge {
            /// Id of the edge to edit
            edge_id: usize,
            /// Action text or dialogue
            text: String,
            /// Special types for actions that may edit variables  
            ///
            /// An example action is if the user is prompted to input the name of their character,
            /// or if the user picks up a variable item from a table and stores it in their
            /// inventory
            action: Option<action::Kind>,
            /// dialogue node that this action originates from
            #[structopt(requires("target_node_id"))]
            source_node_id: Option<usize>,
            /// dialogue node that this action will lead to
            #[structopt(requires("source_node_id"))]
            target_node_id: Option<usize>,
        }

        impl Executable for Edge {
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                let edge_index = EdgeIndex::<u32>::new(self.edge_id);
                let edge = data
                    .tree
                    .edge_weight_mut(edge_index)
                    .ok_or_else(cmd::Error::default)?;
                let old_weight = *edge;

                let start = data.text.len();
                data.text.push_str(&self.text);
                let end = data.text.len();
                let new_weight = Choice::new([start, end], self.action.unwrap_or_default());

                // Handle deletion/recreation of edge if nodes need to change
                if self.source_node_id.is_some() && self.target_node_id.is_some() {
                    // None is unexpected at this point, but double check
                    let source_node_index =
                        NodeIndex::new(self.source_node_id.ok_or_else(cmd::Error::default)?);
                    let target_node_index =
                        NodeIndex::new(self.target_node_id.ok_or_else(cmd::Error::default)?);

                    data.tree.remove_edge(edge_index);
                    data
                        .tree
                        .add_edge(source_node_index, target_node_index, new_weight);
                    util::prune(old_weight.text, &mut data.text, &mut data.tree)?;
                }

                Ok(SUCCESS)
            }
        }

        /// Edit the value of an existing name
        ///
        /// A name represents some variable that may be substituted into the text. Examples
        /// include player names, pronouns, and character traits 
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Name {
            /// The keyword to reference the name with in the text 
            key: String,
            /// Value to store to the name
            value: String,
        }

        impl Executable for Name {
            fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
                // Check that the key already exists, and make sure not to accidently add a new key
                // to the table. The user can use new commands for that
                if data.name_table.get(&self.key).is_none() {
                    Ok("Key does not exist")
                } else {
                    let name = data.name_table.get_mut(&self.key).ok_or_else(cmd::Error::default)?;
                    *name = self.value.clone();
                    Ok(SUCCESS)
                }
            }
        }
    }

    /// Save the current project
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Save {}

    impl Executable for Save {
        fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
            let json = serde_json::to_string(&data).unwrap();
            std::fs::write(data.name.clone() + TREE_EXT, json)?;
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
        fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
            let new_data: DialogueTreeData = serde_json::from_reader(std::io::BufReader::new(
                std::fs::File::open(self.name.clone() + TREE_EXT)?,
            ))?;
            *data = new_data;
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
        fn execute(&self, data: &mut DialogueTreeData) -> cmd::Result {
            let mut name_buf = String::with_capacity(64);
            let mut text_buf = String::with_capacity(256);
            let mut node_iter = data.tree.node_references();
            
            node_iter.try_for_each(|n| -> std::result::Result<(), cmd::Error> {
                let text = &data.text[n.1[0]..n.1[1]];
                util::parse_node(text, &data.name_table, &mut name_buf, &mut text_buf)?;
                println!("{} : {}", name_buf, text_buf);
                data
                    .tree
                    .edges_directed(n.0, petgraph::Direction::Outgoing)
                    .try_for_each(|e| -> std::result::Result<(), cmd::Error> {
                        let choice = e.weight();
                        util::parse_edge(&data.text[choice.text[0]..choice.text[1]], choice.action, &data.name_table, &mut text_buf)?;
                        println!(
                            "--> {:#?} : {} : {} ",
                            e.target(),
                            e.id().index(),
                            text_buf,
                        );
                        Ok(())
                    })?;
                Ok(())
            })?;
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

        /// Helper method to parse a dialogue node's section of the text and fill in any name
        /// variables. 
        ///
        /// The input text rope section should have the following format
        ///     name::text ::name:: more text
        /// 
        /// The first name is the speaker. This name must be a valid key to the name_table
        /// Inside the text, additional names may be inserted inside a :: symbol. The
        /// entire area inside the :: symbols must be a valid key to the name_table.
        ///
        /// Both the name and text buf are cleared at the beginning of this method.
        pub fn parse_node(text: &str, name_table: &HashMap<String, String>, name_buf: &mut String, text_buf: &mut String) -> cmd::Result {
            // Implementation notes:
            //  1. The first iterator element is always the speaker name and should be the only
            //     thing written to the name buffer 
            //  2. Since only a simple flow of name::text::name:::text ... etc is allowed, only
            //  odd tokens ever need to be looked up in the hashtable 
            //  3. The above is only true because split() will return an empty strings on sides of
            //     the separator with no text. For instance name::::name:: would split to ['name,
            //     '', name, '']
            name_buf.clear();
            text_buf.clear();
            let mut text_iter = text.split(TOKEN).enumerate();
            let speaker_key = text_iter.next().ok_or_else(cmd::Error::default)?.1;
            let speaker_name = name_table.get(speaker_key).ok_or_else(cmd::Error::default)?;
            name_buf.push_str(speaker_name);
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 0 { // odd token
                    let value = name_table.get(n).ok_or_else(cmd::Error::default)?;
                    text_buf.push_str(value);
                    Ok(())
                } else { // even token 
                    text_buf.push_str(n);
                    Ok(())
                }
            })?;

            Ok(SUCCESS)
        }

        /// Helper method to parse a player action (edge's) section of the text and fill in any
        /// name variables.
        ///
        /// The input text section should have the following format
        ///     action text ::name:: more action text
        ///
        /// Both the name and text buf are cleared at the beginning of this method
        // TODO: Handling of actions are not implemented yet, if this ends up being done elsewhere
        // the action arg may be removed
        pub fn parse_edge(text: &str, _action: action::Kind, name_table: &HashMap<String, String>, text_buf: &mut String) -> cmd::Result {
            // Implementation notes
            //  1. Due to the format, only even iterator elements are names that need to be looked
            //     up in the name table. This is true because split() will return an empty strings
            //     on sides of the separator with no text. For instance name::::name:: would split
            //     to ['name, '', name, '']
            //  2. This behavior is the opposite of parse_node. This is because parse_node strings
            //     start with the speaker name, where as for parse_edge strings, there is no
            //     speaker as it represents a player action

            text_buf.clear();
            let mut text_iter = text.split(TOKEN).enumerate();
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 0 { // odd token
                    text_buf.push_str(n);
                    Ok(())
                } else { // even token 
                    let value = name_table.get(n).ok_or_else(cmd::Error::default)?;
                    text_buf.push_str(value);
                    Ok(())
                }
            })?;

            Ok(SUCCESS)
        }

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
        ///
        // FIXME: redo prune now that we are no longer using text rope
        pub fn prune(
            range: Section,
            _text: &str,
            tree: &mut graph::DiGraph<Section, Choice>,
        ) -> cmd::Result {
            // Implementation notes:
            //  1. Code is written to be branchless in case of a very large tree
            //  2. Range is non-inclusive, which means that num_removed has to be 1 larger than the
            //     difference between the ranges
            
            // ensure range indices are in the proper order
            (range[1] > range[0]).then(||{}).ok_or_else(cmd::Error::default)?;

            let num_removed = range[1] - range[0] + 1;
            
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

            Ok(SUCCESS)
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
        /// Stores a specific word or phrase to the Hashtable with a provided key
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
    let mut state = DialogueTreeData::default();
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
            Ok(v) => match v.execute(&mut state) {
                Ok(r) => println!("{}", r),
                Err(f) => println!("{}", f),
            },
            Err(e) => println!("{}", e),
        }

        // clear input buffers before starting next input loop
        cmd_buf.clear();
    }
}
