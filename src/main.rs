use petgraph::prelude::*;
use petgraph::visit::{IntoNodeReferences, IntoEdgeReferences};
use petgraph::*;
use std::io;
use std::io::Write;

static PROJECT_EXTENSION: &str = ".tree";
static UNKNOWN: &str = "unknown command, type help for more info";
static NONAME: &str = "no name provided";
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
static UNIMPLEMENTED: &str = "unimplemented command";
static SUCCESS: &str = "success\r\n";

type Section = [usize; 2];

#[derive(Debug)]
pub enum Mode {
    Project,
    Node,
    Edge,
}

pub struct State {
    tree: petgraph::graph::DiGraph<Section, Section>,
    text: ropey::Rope,
    name: String,
    mode: Mode,
}
impl State {
    fn new() -> Self {
        State {
            //TODO: parameter for initial capacity
            tree: graph::DiGraph::<Section, Section>::with_capacity(1000,1000),
            text: ropey::Rope::new(),
            name: String::new(),
            mode: Mode::Project,
        }
    }
}

mod cmd {
    use super::*;

    /// Unified result type for propogating errors in cmd methods
    type Result = std::result::Result<&'static str, Box<dyn std::error::Error + Send + Sync>>;

    pub fn help(_cmd_iter: &mut std::slice::Iter<String>, _state: &mut State) -> cmd::Result {
        Ok(HELP)
    }

    pub fn new(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
        match cmd_iter.next().ok_or(cmd::Error::default())?.as_str() {
            "project" => new::project(cmd_iter, state),
            "node" => new::node(cmd_iter, state),
            "edge" => new::edge(cmd_iter, state),
            _ => Ok(UNKNOWN),
        }
    }

    mod new {
        use super::*;

        // TODO: Serializable tree struct
        pub fn project(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
            // Create new project file on disk with user supplied name
            let project_name =
                cmd_iter.next().ok_or(cmd::Error::default())?.to_owned() + PROJECT_EXTENSION;
            // TODO: use openoptions to not overwrite
            let project_file = std::fs::File::create(&project_name)?;
            // drop project to save new file to disk
            drop(project_file);

            // re-open project file as a rope
            let project = ropey::Rope::from_reader(std::io::BufReader::new(std::fs::File::open(
                &project_name,
            )?))
            .unwrap();

            // save info to state
            state.name = project_name.to_string();
            state.text = project;
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
        pub fn node(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
            // Next and final argument passed with the new node command should be the full text
            // string.
            let text = cmd_iter.next().ok_or(cmd::Error::default())?.to_owned();

            // The iterator shouldn't have any command line parameters after the text, if extra
            // parameters are passed it probably is a mistake from the user
            check_end(cmd_iter)?;

            // add the text to the rope, get the start and end line, and add to tree
            // TODO: Text sanitization needed?
            let start = state.text.len_chars();
            state.text.append(ropey::Rope::from(text));
            let end = state.text.len_chars();
            state.tree.add_node([start, end]);
            Ok(SUCCESS)
        }

        pub fn edge(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
            let start_node_idx = cmd_iter
                .next()
                .ok_or(cmd::Error::default())?
                .parse::<i32>()? as u32;

            let end_node_idx = cmd_iter
                .next()
                .ok_or(cmd::Error::default())?
                .parse::<i32>()? as u32;

            let text = cmd_iter.next().ok_or(cmd::Error::default())?.to_owned();

            // The iterator shouldn't have any command line parameters after the text, if extra
            // parameters are passed it probably is a mistake from the user
            check_end(cmd_iter)?;

            //Add edge text to rope and edge to tree
            let start = state.text.len_chars();
            state.text.append(ropey::Rope::from(text));
            let end = state.text.len_chars();
            state.tree.add_edge(
                NodeIndex::from(start_node_idx),
                NodeIndex::from(end_node_idx),
                [start, end],
            );

            Ok(SUCCESS)
        }
    }

    /// Print all nodes, edges, and associated text
    ///
    pub fn list(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
        check_end(cmd_iter)?;
        let node_iter = state.tree.node_references();
        node_iter.for_each(|n| {
            // Print node identifier, node text, and then all edges
            println!("{:#?} : {}", n.0, state.text.slice(n.1[0]..n.1[1]));
            state
                .tree
                .edges_directed(n.0, petgraph::Direction::Outgoing)
                .for_each(|e| println!("--> {:#?} : {} ", e.1, state.text.slice(e.2[0]..e.2[1])));
        });

        Ok(SUCCESS)
    }

    pub fn save(cmd_iter: &mut std::slice::Iter<String>, state: &mut State) -> cmd::Result {
        // save text
        state.text.write_to(std::io::BufWriter::new(std::fs::File::open(state.name.clone())?))?;

        // save tree
        let tree_json = serde_json::to_string(&state.tree).unwrap();
        Ok(SUCCESS)
    }
    
    /// Error types for different commands
    // TODO: remove if not needed
    #[derive(Debug, Default)]
    struct Error {
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

    impl From<std::io::Error> for Error {
        fn from(err: std::io::Error) -> Self {
            Error::default()
        }
    }

    /// Helper method for checking that a command iterator is ended
    /// Returns Ok if iterator is empty, or a cmd::Error if any more elements remain in the iterator  
    fn check_end(cmd_iter: &mut std::slice::Iter<String>) -> cmd::Result {
        cmd_iter
            .next()
            .xor(Some(&String::new()))
            .ok_or(cmd::Error::default())?;
        // TODO: different Ok message for check_end
        Ok(SUCCESS)
    }
}

fn main() {
    let mut buf = String::with_capacity(100);
    println!("welcome to arbor!");

    // TODO: clean up state init
    let mut state = State::new();
    loop {
        // print default information
        print!("project: {}\nmode: {:?}\n>> ", state.name, state.mode);

        // get next command from the user
        io::stdout().flush().unwrap();
        io::stdin()
            .read_line(&mut buf)
            .expect("Failed to read line");

        let cmds = shellwords::split(&buf).unwrap();
        let mut cmd_iter = cmds.iter();
        let res = match cmd_iter.next().unwrap().as_str() {
            "help" => cmd::help(&mut cmd_iter, &mut state),
            "new" => cmd::new(&mut cmd_iter, &mut state),
            "list" => cmd::list(&mut cmd_iter, &mut state),
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

        // clear input buffer before starting next input loop
        buf.clear();
    }
}
