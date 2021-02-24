use petgraph::prelude::*;
use petgraph::visit::IntoNodeReferences;
use petgraph::*;
use std::io;
use std::io::Write;
use displaydoc::Display;
use serde::{Serialize, Deserialize};

// TODO: Major Features
// 1. enums for edge function calls
// 2. Name list & name validation
// 3. Node and edge validation
// 4. Tests
// 5. Redundancy when editing/pruning/saving
// 6. Help text based on /// comments
//      a. standardize /// comment style
// 7. Proper error/Ok propogation
// 8. Decouple command parsing from command execution

static ROPE_EXT: &str = ".rope";
static TREE_EXT: &str = ".tree";
static UNKNOWN: &str = "unknown command, type help for more info";
static _NONAME: &str = "no name provided";
static HELP: &str = "Arbor
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
#[derive(Serialize, Deserialize)] 
pub struct Choice {
   text: Section,
   action: action::Kind
}

impl Choice {
    fn new(text: Section, action: action::Kind) -> Self {
        Choice{
            text,
            action,
        }
    }
}

pub struct EditorState {
    tree: petgraph::graph::DiGraph<Section, Choice>,
    rope: ropey::Rope,
    name: String,
    to_be_deleted: Vec<Section>,
}
impl EditorState {
    fn new() -> Self {
        EditorState {
            //TODO: parameter for initial capacity
            tree: graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
            rope: ropey::Rope::new(),
            name: String::new(),
            to_be_deleted: Vec::<Section>::with_capacity(10),
        }
    }
}

mod cmd {
    use super::*;

    /// Unified result type for propogating errors in cmd methods

    type Result = std::result::Result<&'static str, cmd::Error>;

    pub fn help(_cmd_iter: &mut std::slice::Iter<String>, _state: &mut EditorState) -> cmd::Result {
        Ok(HELP)
    }

    pub fn new(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        match cmd_iter
            .next()
            .ok_or_else(cmd::Error::default)?
            .as_str()
        {
            "project" => new::project(cmd_iter, state),
            "node" => new::node(cmd_iter, state),
            "edge" => new::edge(cmd_iter, state),
            _ => Ok(UNKNOWN),
        }
    }

    mod new {
        use super::*;

        // TODO: Serializable tree struct
        pub fn project(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            // Create new project file on disk with user supplied name
            let project_name = cmd_iter
                .next()
                .ok_or_else(cmd::Error::default)?
                .to_owned();

            let text = ropey::Rope::new();
            // save info to state
            state.name = project_name;
            state.rope = text;
            Ok("New project created")
        }

        /// Create a new node
        /// The node represents a continuous block of text in the text rope, and a segment of
        /// dialogue from a character. Different sections of the node are delimited by '::' in the
        /// text rope. A node has 2 sections:
        /// 1. Speaker name
        /// 2. Dialogue
        ///
        /// example:
        /// new node "Algernon::You're a law student?"
        ///
        /// The node is also represented in the tree by an array of two values, the start and end
        /// line of the node's text block in the text rope.  
        ///
        pub fn node(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            // Next and final argument passed with the new node command should be the full text
            // string.
            let text = cmd_iter
                .next()
                .ok_or_else(cmd::Error::default)?
                .to_owned();

            // The iterator shouldn't have any command line parameters after the text, if extra
            // parameters are passed it probably is a mistake from the user
            util::check_end(cmd_iter)?;

            // add the text to the rope, get the start and end line, and add to tree
            // TODO: Text sanitization needed?
            let start = state.rope.len_chars();
            state.rope.append(ropey::Rope::from(text));
            let end = state.rope.len_chars();
            state.tree.add_node([start, end]);
            Ok(SUCCESS)
        }

        /// Create a new edge between nodes
        /// The edge represents a dialogue choice by the player. The edge should connect two nodes
        /// of dialogue with an action. The user will select from a list of outgoing edges on a
        /// given node in order to choose the path through the dialogue tree. Edges can loop back
        /// to the same node (eg: to retry a different option), and any number of edges may connect
        /// the same or different nodes.
        ///
        /// Note that edges are directional, so to create a loop between two dialogue options, two
        /// edges need to be defined.
        ///
        /// The format for defining a new edge is:
        ///     new edge <start_node_idx> <target_node_idx> "<user choice or action>"
        /// example:
        ///     new edge 0 1 "Yes, I am"
        ///
        // TODO: Define case for empty edge, where no action is taken and dialogue should move
        // automatically to the next node.
        pub fn edge(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            let start_node_idx = cmd_iter
                .next()
                .ok_or_else(cmd::Error::default)?
                .parse::<i32>()? as u32;

            let end_node_idx = cmd_iter
                .next()
                .ok_or_else(cmd::Error::default)?
                .parse::<i32>()? as u32;

            let text = cmd_iter
                .next()
                .ok_or_else(cmd::Error::default)?
                .to_owned();

            // The iterator shouldn't have any command line parameters after the text, if extra
            // parameters are passed it probably is a mistake from the user
            util::check_end(cmd_iter)?;

            //Add edge text to rope and edge to tree
            let start = state.rope.len_chars();
            state.rope.append(ropey::Rope::from(text));
            let end = state.rope.len_chars();
            state.tree.add_edge(
                NodeIndex::from(start_node_idx),
                NodeIndex::from(end_node_idx),
                Choice::new([start, end], action::Kind::Inactive),
            );

            Ok(SUCCESS)
        }
    }

    /// Print all nodes, edges, and associated text
    /// This prints all nodes in index order (not necessarily the order they would appear when
    /// traversing the dialogue tree). Under each node definiton, a list of the outgoing edges from
    /// that node will be listed. This will show the path to the next dialogue option from any
    /// node, and the choice/action text associated with that edge.
    ///
    /// ex:
    /// NodeIndex(0) Algernon::You're a law Student?
    /// --> NodeIndex(1) Yes
    /// --> NodeIndex(1) No
    /// NodeIndex(1) Algernon::Well...gotta run
    ///
    pub fn list(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        util::check_end(cmd_iter)?;
        let node_iter = state.tree.node_references();
        node_iter.for_each(|n| {
            // Print node identifier, node text, and then all edges
            println!("{:#?} : {}", n.0, state.rope.slice(n.1[0]..n.1[1]));
            state
                .tree
                .edges_directed(n.0, petgraph::Direction::Outgoing)
                .for_each(|e| {
                    println!(
                        "--> {:#?} : {} : {} ",
                        e.target(),
                        e.id().index(),
                        state.rope.slice(e.weight().text[0]..e.weight().text[1])
                    )
                });
        });

        Ok(SUCCESS)
    }

    /// Save the text rope and tree to the file system
    ///
    /// At the moment, the tree and text rope are saved to different files, with .rope and .tree
    /// file extensions respectively. These files are saved to the local directory

    // TODO:
    //  1. Handle overwriting, backups, etc
    //  2. Handle custom pathing to save file
    //  3. Have definable default save path (maybe save last path in state)
    pub fn save(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        util::check_end(cmd_iter)?;
        // save tree
        let tree_json = serde_json::to_string(&state.tree).unwrap();
        std::fs::write(state.name.clone() + TREE_EXT, tree_json)?;

        // save text
        state
            .rope
            .write_to(std::io::BufWriter::new(std::fs::File::create(
                state.name.clone() + ROPE_EXT,
            )?))?;
        Ok(SUCCESS)
    }

    /// Load a text rope and tree from the file system
    ///
    /// Will open a .rope and .tree file with the provided name. Currently only looks in the
    /// current working directory. Once loaded, the program state will be updated to edit the new
    /// text rope and tree, using the loaded project name
    ///
    /// Format for load command is:
    ///     load <project_name>
    /// example (with files algernon.tree and algernon.rope in ./):
    ///     load algernon
    // TODO:
    //  1. Handle custom pathing
    //  2. Consider recursive searching for files
    //  3. Have definable default path to search for file (maybe save last path in state)
    //  4. Validate files, report error after loading
    pub fn load(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        let name = cmd_iter
            .next()
            .ok_or_else(cmd::Error::default)?
            .to_owned();
        util::check_end(cmd_iter)?;

        // Attempt to load files
        let tree: petgraph::graph::DiGraph<Section, Choice> = serde_json::from_reader(
            std::io::BufReader::new(std::fs::File::open(name.clone() + TREE_EXT)?),
        )?;
        let rope = ropey::Rope::from_reader(std::io::BufReader::new(std::fs::File::open(
            name.clone() + ROPE_EXT,
        )?))?;

        // If successful, update state
        state.tree = tree;
        state.rope = rope;
        state.name = name;

        Ok(SUCCESS)
    }

    pub fn edit(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        match cmd_iter
            .next()
            .ok_or_else(cmd::Error::default)?
            .as_str()
        {
            "project" => edit::project(cmd_iter, state),
            "node" => edit::node(cmd_iter, state),
            "edge" => edit::edge(cmd_iter, state),
            _ => Ok(UNKNOWN),
        }
    }

    mod edit {
        use super::*;

        /// Edit the project info
        ///
        /// Currently only edit the project name, requires entering the current project name to
        /// confirm change
        ///
        /// format: 
        ///     edit project "<old_title>" "<new_title>"
        ///
        /// example:
        ///     edit project "My Old and Bad Title" "My New and Amazing Title" 
        pub fn project(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            // extract project names from command iter & verify matching project names and number
            // of args
            let old_name = cmd_iter.next().ok_or_else(cmd::Error::default)?;
            let new_name = cmd_iter.next().ok_or_else(cmd::Error::default)?;
            util::check_end(cmd_iter)?;
            util::check_str(&old_name, &new_name)?;

            state.name = new_name.to_string();
            Ok(SUCCESS)
        }

        /// Edit the contents of a node
        ///
        /// First, the new text section is appended to the end of the text rope. Next, the node
        /// weights are modified to point to the new text section. Finally, the old text section is
        /// removed from the rope, and all tree weights are shifted in turn.
        ///
        /// format:
        ///     edit node <NodeIndex> "<edited_node_name::edited_node_text>
        ///
        /// example:
        ///     edit node 0 "Algernon::You, of all people, are a law student!?"
        // TODO: mark unused section of the rope for deletion, implement scheme for safely
        // removing unused text and shifting node indices
        pub fn node(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            // Get node index and convert into integer
            let node_idx = NodeIndex::new(
                cmd_iter
                    .next()
                    .ok_or_else(cmd::Error::default)?
                    .parse::<i32>()? as usize,
            );

            let new_node_text = cmd_iter.next().ok_or_else(cmd::Error::default)?;
            util::check_end(cmd_iter)?;

            let node_weight = state
                .tree
                .node_weight_mut(node_idx)
                .ok_or_else(cmd::Error::default)?;

            let start = state.rope.len_chars();
            state.rope.append(ropey::Rope::from(new_node_text.as_str()));
            let end = state.rope.len_chars();

            state.to_be_deleted.push(*node_weight);
            *node_weight = [start, end];

            // TODO: Finalize pruning sequence, this may need to move 
            // Remove unused text section from the graph
            util::prune(
                state.to_be_deleted.pop().unwrap(),
                &mut state.rope,
                &mut state.tree,
            );
            Ok(SUCCESS)
        }

        /// Edit the contents of an edge
        ///
        /// First, the new text section is appended to the end of the text rope. Next, the edge 
        /// weights are modified to point to the new text section. Finally, the old text section is
        /// removed from the rope, and all tree weights are shifted in turn.
        ///
        /// format:
        ///     edit edge <EdgeIndex> "<edited_edge_text>"
        ///
        /// example:
        ///     edit edge 0 "No, I'm not" 
        pub fn edge(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
            // Get node index and convert into integer
            let node_idx = NodeIndex::new(
                cmd_iter
                    .next()
                    .ok_or_else(cmd::Error::default)?
                    .parse::<i32>()? as usize,
            );

            let new_node_text = cmd_iter.next().ok_or_else(cmd::Error::default)?;
            util::check_end(cmd_iter)?;

            let node_weight = state
                .tree
                .node_weight_mut(node_idx)
                .ok_or_else(cmd::Error::default)?;

            let start = state.rope.len_chars();
            state.rope.append(ropey::Rope::from(new_node_text.as_str()));
            let end = state.rope.len_chars();

            state.to_be_deleted.push(*node_weight);
            *node_weight = [start, end];

            // TODO: Finalize pruning sequence, this may need to move 
            // Remove unused text section from the graph
            util::prune(
                state.to_be_deleted.pop().unwrap(),
                &mut state.rope,
                &mut state.tree,
            );
            Ok(SUCCESS)
        }
    }

    /// Start reading the currently loaded project from the start node
    /// 
    /// format:
    ///     read 
    /// example:
    ///     read 
    // TODO: This is a prototype for read functionality, likely needs to be moved in the future
    pub fn read(cmd_iter: &mut std::slice::Iter<String>, state: &mut EditorState) -> cmd::Result {
        println!("reader mode:");
        util::check_end(cmd_iter)?;
        let node_idx = graph::node_index(0);
        let iter = state.tree.edges_directed(node_idx, petgraph::Direction::Outgoing);
        let _target_list = iter
            .enumerate()
            .map(|(i, e)| {
                println!(
                    "{}. {}",
                    i,
                    state.rope.slice(e.weight().text[0]..e.weight().text[1])
                );
                e.target()
        });
        Ok(SUCCESS)
    }

    /// Error types for different commands
    // TODO: remove if not needed
    #[derive(Debug, Default)]
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
            Error::default()
        }
    }

    pub mod util {
        use super::*;

        /// Helper method for checking that a command iterator is ended
        /// Returns Ok if iterator is empty, or a cmd::Error if any more elements remain in the
        /// iterator   
        pub fn check_end(cmd_iter: &mut std::slice::Iter<String>) -> cmd::Result {
            cmd_iter
                .next()
                .xor(Some(&String::new()))
                .ok_or_else(cmd::Error::default)?;
            // TODO: different Ok message for check_end
            Ok(SUCCESS)
        }

        /// Helper method to check if two string slices match, and return a cmd::Result based on
        /// the results.
        // TODO: Create non-default error type for compare mismatch
        pub fn check_str(a: &str, b: &str) -> cmd::Result {
            match a == b {
                true => Ok(""),
                false => Err(cmd::Error::default()),
            }
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
    #[derive(Serialize, Deserialize)]
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
}

fn main() {
    let mut cmd_buf = String::with_capacity(1000);

    // TODO: clean up state init
    let mut state = EditorState::new();
    loop {
        // print default header
        println!("------------");
        println!("project: {}", state.name);
        println!("------------");

        cmd::util::prompt_input(&mut cmd_buf);

        let cmds = shellwords::split(&cmd_buf).unwrap();
        let mut cmd_iter = cmds.iter();
        let res = match cmd_iter.next().unwrap().as_str() {
            "help" => cmd::help(&mut cmd_iter, &mut state),
            "new" => cmd::new(&mut cmd_iter, &mut state),
            "list" => cmd::list(&mut cmd_iter, &mut state),
            "ls" => cmd::list(&mut cmd_iter, &mut state),
            "save" => cmd::save(&mut cmd_iter, &mut state),
            "s" => cmd::save(&mut cmd_iter, &mut state),
            "load" => cmd::load(&mut cmd_iter, &mut state),
            "edit" => cmd::edit(&mut cmd_iter, &mut state),
            "read" => cmd::read(&mut cmd_iter, &mut state),
            "q" => break,
            "exit" => break,
            "quit" => break,
            _ => Ok(UNKNOWN),
        };

        // Handle results/errors
        match res {
            Ok(v) => println!("{}", v),
            Err(e) => println!("error: {}", e),
        }

        // clear input buffers before starting next input loop
        cmd_buf.clear();
    }
}
