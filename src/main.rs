#![feature(test)]
#![feature(backtrace)]
extern crate test;
use crate::cmd::Executable;
use anyhow::Result;
use clap::AppSettings;
use derive_new::*;
use enum_dispatch::*;
use enum_from_str::ParseEnumVariantError;
use enum_from_str_derive::FromStr;
use hashbrown::HashMap;
use petgraph::prelude::*;
use petgraph::visit::IntoNodeReferences;
use petgraph::*;
use seahash::hash;
use serde::{Deserialize, Serialize};
use std::io;
use std::io::Write;
use structopt::*;
use thiserror::Error;

// TODO: Features List
// 1. More tests and benchmarks!
// 2. Switch to bincode serialization format
// 3. Add more help messages for common errors, maybe with contexts?

static TREE_EXT: &str = ".tree";
static TOKEN: &str = "::";

/// typedef representing a section of text in a rope. This section contains a start and end index,
/// stored in an array. The first element should always be smaller than the second

#[derive(new, Serialize, Deserialize, Clone, Copy)]
pub struct Section {
    text: [usize; 2],
    hash: u64,
}

impl std::ops::Index<usize> for Section {
    type Output = usize;
    fn index(&self, i: usize) -> &usize {
        &self.text[i]
    }
}

impl std::ops::IndexMut<usize> for Section {
    fn index_mut(&mut self, i: usize) -> &mut usize {
        &mut self.text[i]
    }
}

/// typedef representing the petgraph::Graph type used in dialogue trees. The nodes are made up of
/// Sections, which define slices of a text buffer. The edges are Choice structs, which define a
/// Section as well as data regarding different action types a player may perform
pub type Tree = petgraph::graph::Graph<Section, Choice>;

/// Top level data structure for storing a dialogue tree
///
/// This struct contains the tree representing the dialogue nodes and player actions connecting
/// them, the rope which stores all text in a tightly packed manner, and a hashtable for storing
/// variables such as player names, conditionals, etc.
#[derive(new, Serialize, Deserialize, Clone)]
pub struct DialogueTreeData {
    tree: Tree,
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

/// Represents a requirement to access a choice. 
#[derive(Serialize, Deserialize, Clone)]
pub enum ReqKind {
    /// Must be greater than num
    GT(String, u32),
    /// Must be less than num
    LT(String, u32),
    /// Must be equal to num
    EQ(String, u32),
    CMP(String, String),
}

impl std::str::FromStr for ReqKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Implementation notes:
        // The enum string format is set up to directly map to how the enum is declared in rust:
        // e.g. 'GreaterThan(my_key, 10)'
        // This is tokenized on the presence of '(' ',' and ')' special characters. In reverse
        // order: 
        // e.g. ['', '10', 'my_key', 'GreaterThan']
        //
        // This is done in reverse order so that the required key and val can be built up before
        // converting the enum itself, (since the key and val are required to declare the enum
        //
        // Importantly, the 'val' that is tested against can be a string or a u32. This is handled
        // by waiting to unwrap the val parameter until building the Enum
        let mut split = s.rsplit(&['(', ',', ')'][..]);
        // first item should be ''
        anyhow::ensure!(split.next().ok_or(cmd::Error::Generic)? == "");
        // second item should be number or string, wait to check validity
        let val = split
            .next()
            .ok_or(cmd::Error::Generic)?;
        // third item should be key
        let key: &str = split
            .next()
            .ok_or(cmd::Error::Generic)?;
        // fourth item should be Enum type, build it!, and also try to resolve the val
        match split.next().ok_or(cmd::Error::Generic)? {
            "GT" => Ok(ReqKind::GT(key.to_string(), val.parse::<u32>()?)),
            "LT" => Ok(ReqKind::LT(key.to_string(), val.parse::<u32>()?)),
            "EQ" => Ok(ReqKind::EQ(key.to_string(), val.parse::<u32>()?)),
            "CMP" => Ok(ReqKind::CMP(key.to_string(), val.to_string())),
            _ => Err(cmd::Error::Generic)?,
        }
    }
}

/// Represents an effect that occurs when a choice is made.
#[derive(Serialize, Deserialize, Clone)]
pub enum EffectKind {
    Add(String, u32),
    Sub(String, u32),
    Set(String, u32),
    Assign(String, String),
}

impl std::str::FromStr for EffectKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Implementation notes:
        // The enum string format is set up to directly map to how the enum is declared in rust:
        // e.g. 'Add(my_key,10)'
        // This is tokenized on the presence of '(' ',' and ')' special characters. In reverse
        // order: 
        // e.g. ['', '10', 'my_key', 'Add']
        //
        // This is done in reverse order so that the required key and val can be built up before
        // converting the enum itself, (since the key and val are required to declare the enum.
        //
        // Importantly, the 'val' that is tested against can be a string or a u32. This is handled
        // by waiting to unwrap the val parameter until building the Enum
        let mut split = s.rsplit(&['(', ',', ')'][..]);
        // first item should be ''
        anyhow::ensure!(split.next().ok_or(cmd::Error::Generic)? == "");
        // second item should be number or string, don't check for validity yet 
        let val = split
            .next()
            .ok_or(cmd::Error::Generic)?;
        // third item should be key
        let key: &str = split
            .next()
            .ok_or(cmd::Error::Generic)?;
        // fourth item should be Enum type, build it!
        match split.next().ok_or(cmd::Error::Generic)? {
            "Add" => Ok(EffectKind::Add(key.to_string(), val.parse::<u32>()?)),
            "Sub" => Ok(EffectKind::Sub(key.to_string(), val.parse::<u32>()?)),
            "Set" => Ok(EffectKind::Set(key.to_string(), val.parse::<u32>()?)),
            "Assign" => Ok(EffectKind::Assign(key.to_string(), val.to_string())),
            _ => Err(cmd::Error::Generic)?,
        }
    }
}

/// Struct storing the information for a player choice. Stored in the edges of a dialogue tree
#[derive(new, Serialize, Deserialize, Clone)]
pub struct Choice {
    section: Section,
    requirement: Option<ReqKind>,
    effect: Option<EffectKind>,
}

/// State information for an editor instance. Includes two copies of the dialogue tree (one active
/// and one backup) as well as other state information
pub struct EditorState {
    act: DialogueTreeData,
    backup: DialogueTreeData,
    scratchpad: String,
}

impl EditorState {
    /// Create a new Editor state.
    ///
    /// Editor state needs to take ownership of the data. However since
    /// a backup copy needs to be created on construction, the data is moved, and then cloned
    pub fn new(data: DialogueTreeData) -> Self {
        EditorState {
            act: data.clone(),
            backup: data,
            scratchpad: String::with_capacity(1000),
        }
    }

    /// Swap the active and backup trees without copying any of the underlying data
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.act, &mut self.backup);
    }
}

/// Top level module for all dialogue_tree commands. These commands rely heavily on the structopt
/// derive feature to easily implement a command line interface along with command structs for
/// input through other methods (UI, test code, etc.). In any structopt derived structure or enum,
/// the doc comments are displayed to the user through the CLI.
///
/// All commands also implement the generic Executable trait. This trait uses enum_dispatch to
/// propagate through to all types contained in the Parse enums. This executable method is where
/// the core logic of any command happens.
mod cmd {
    use super::*;

    /// Error types for different commands
    ///
    /// Uses thiserror to generate messages for common situations. This does not
    /// attempt to implement From trait on any lower level error types, but relies
    /// on anyhow for unification and printing a stack trace
    #[derive(Error, Debug)]
    pub enum Error {
        #[error("An unspecified error occured...")]
        Generic,
        #[error("The name already exists")]
        NameExists,
        #[error("The name does not exist")]
        NameNotExists,
        #[error("Attempted to access a node that is not present in the tree")]
        InvalidNodeIndex,
        #[error("Attempted to access an edge that is not present in the tree")]
        InvalidEdgeIndex,
    }

    /// Trait to allow structopt generated
    #[enum_dispatch]
    pub trait Executable {
        fn execute(&self, data: &mut EditorState) -> Result<()>;
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
            /// New Project
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                let new_project = DialogueTreeData::new(
                    graph::DiGraph::<Section, Choice>::with_capacity(512, 2048),
                    String::with_capacity(8192),
                    HashMap::new(),
                    self.name.clone(),
                );

                let json = serde_json::to_string(&new_project).unwrap();
                std::fs::write(self.name.clone() + TREE_EXT, json)?;

                if self.set_active {
                    *state = EditorState::new(new_project);
                }
                Ok(())
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
            /// New Node
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                // verify the speaker name is valid
                state
                    .act
                    .name_table
                    .get(&self.speaker)
                    .ok_or(cmd::Error::Generic)?;
                let start = state.act.text.len();
                state
                    .act
                    .text
                    .push_str(&format!("{}::{}", self.speaker, self.dialogue));
                let end = state.act.text.len();
                // Create hash for verifying the text section in the future
                let hash = hash(&state.act.text[start..end].as_bytes());
                state.act.tree.add_node(Section::new([start, end], hash));
                Ok(())
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
            /// Requirement for accessing this edge
            #[structopt(short = "r")]
            requirement: Option<ReqKind>,

            /// Effect caused by accessing this edge
            #[structopt(short = "e")]
            effect: Option<EffectKind>,
        }

        impl Executable for Edge {
            /// New Edge
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                let start = state.act.text.len();
                state.act.text.push_str(&self.text);
                let end = state.act.text.len();
                // Compute hash for verifying the text section later
                let hash = hash(&state.act.text[start..end].as_bytes());
                state.act.tree.add_edge(
                    NodeIndex::from(self.start_index),
                    NodeIndex::from(self.end_index),
                    Choice::new(
                        Section::new([start, end], hash),
                        self.requirement.clone(),
                        self.effect.clone(),
                    ),
                );
                Ok(())
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
            /// New Name
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                // Check that the key doesn't already exist, since we want new to not overwrite
                // values. The user can use edit commands for that
                if state.act.name_table.get(&self.key).is_none() {
                    state
                        .act
                        .name_table
                        .insert(self.key.clone(), self.value.clone());
                } else {
                    Err(cmd::Error::NameExists)?;
                }
                Ok(())
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
            Name(edit::Name),
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
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                let node_index = NodeIndex::new(self.node_id);
                let start = state.act.text.len();
                state
                    .act
                    .text
                    .push_str(&format!("{}::{}", self.speaker, self.dialogue));
                let end = state.act.text.len();

                let node = state
                    .act
                    .tree
                    .node_weight_mut(node_index)
                    .ok_or(cmd::Error::InvalidNodeIndex)?;
                // Since editing, recalculate hash
                let hash = hash(state.act.text[start..end].as_bytes());
                *node = Section::new([start, end], hash);
                Ok(())
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
            /// Requirement for accessing this edge
            #[structopt(short = "r")]
            requirement: Option<ReqKind>,
            /// Effect caused by accessing this edge
            #[structopt(short = "e")]
            effect: Option<EffectKind>,
            /// dialogue node that this action originates from
            #[structopt(requires("target_node_id"))]
            source_node_id: Option<usize>,
            /// dialogue node that this action will lead to
            #[structopt(requires("source_node_id"))]
            target_node_id: Option<usize>,

        }

        impl Executable for Edge {
            /// Edit Edge
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                let edge_index = EdgeIndex::<u32>::new(self.edge_id);
                let start = state.act.text.len();
                state.act.text.push_str(&self.text);
                let end = state.act.text.len();
                let hash = hash(state.act.text[start..end].as_bytes());
                let new_weight = Choice::new(
                    Section::new([start, end], hash),
                    self.requirement.clone(),
                    self.effect.clone(),
                );

                // Handle deletion/recreation of edge if nodes need to change
                if self.source_node_id.is_some() && self.target_node_id.is_some() {
                    // None is unexpected at this point, but double check
                    let source_node_index =
                        NodeIndex::new(self.source_node_id.ok_or(cmd::Error::Generic)?);
                    let target_node_index =
                        NodeIndex::new(self.target_node_id.ok_or(cmd::Error::Generic)?);

                    state.act.tree.remove_edge(edge_index);
                    state
                        .act
                        .tree
                        .add_edge(source_node_index, target_node_index, new_weight);
                }

                Ok(())
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
            fn execute(&self, state: &mut EditorState) -> Result<()> {
                // Check that the key already exists, and make sure not to accidently add a new key
                // to the table. The user can use new commands for that
                if state.act.name_table.get(&self.key).is_some() {
                    let name = state
                        .act
                        .name_table
                        .get_mut(&self.key)
                        .ok_or(cmd::Error::Generic)?;
                    *name = self.value.clone();
                } else {
                    Err(cmd::Error::NameNotExists)?;
                }
                Ok(())
            }
        }
    }

    /// Save the current project
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Save {}

    impl Executable for Save {
        fn execute(&self, state: &mut EditorState) -> Result<()> {
            // save states to backup buffer
            state.backup = state.act.clone();

            // attempt rebuild tree on active buffer, backup buffer is used as source
            util::rebuild_tree(
                &state.backup.text,
                &state.backup.tree,
                &mut state.act.text,
                &mut state.act.tree,
            )?;

            let json = serde_json::to_string(&state.act).unwrap();
            std::fs::write(state.act.name.clone() + TREE_EXT, json)?;
            Ok(())
        }
    }

    /// Load a project from disk, will overwrite unsaved changes
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Load {
        name: String,
    }

    impl Executable for Load {
        fn execute(&self, state: &mut EditorState) -> Result<()> {
            let new_state = EditorState::new(serde_json::from_reader(std::io::BufReader::new(
                std::fs::File::open(self.name.clone() + TREE_EXT)?,
            ))?);
            *state = new_state;
            Ok(())
        }
    }

    /// Print all nodes, edges, and associated text to the editor scratchpad
    ///
    /// Prints all nodes in index order (not necessarily the order they would appear when
    /// traversing the dialogue tree). Under each node definiton, a list of the outgoing edges from
    /// that node will be listed. This will show the path to the next dialogue option from any
    /// node, and the choice/action text associated with that edge.
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct List {}

    impl Executable for List {
        fn execute(&self, state: &mut EditorState) -> Result<()> {
            let mut name_buf = String::with_capacity(64);
            let mut text_buf = String::with_capacity(256);
            let node_iter = state.act.tree.node_references();

            for n in node_iter {
                let text = &state.act.text[n.1[0]..n.1[1]];
                util::parse_node(text, &state.act.name_table, &mut name_buf, &mut text_buf)?;
                state
                    .scratchpad
                    .push_str(&format!("{} : {}\r\n", name_buf, text_buf));
                for e in state
                    .act
                    .tree
                    .edges_directed(n.0, petgraph::Direction::Outgoing)
                {
                    let choice = e.weight();
                    util::parse_edge(
                        &state.act.text[choice.section[0]..choice.section[1]],
                        &state.act.name_table,
                        &mut text_buf,
                    )?;
                    state.scratchpad.push_str(&format!(
                        "--> {:#?} : {} : {}\r\n",
                        e.target(),
                        e.id().index(),
                        text_buf
                    ));
                }
            }
            println!("{}", state.scratchpad);
            Ok(())
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
        pub fn parse_node(
            text: &str,
            name_table: &HashMap<String, String>,
            name_buf: &mut String,
            text_buf: &mut String,
        ) -> Result<()> {
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
            let speaker_key = text_iter.next().ok_or(cmd::Error::Generic)?.1;
            let speaker_name = name_table.get(speaker_key).ok_or(cmd::Error::Generic)?;
            name_buf.push_str(speaker_name);
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 0 {
                    // odd token
                    let value = name_table.get(n).ok_or(cmd::Error::Generic)?;
                    text_buf.push_str(value);
                    Ok(())
                } else {
                    // even token
                    text_buf.push_str(n);
                    Ok(())
                }
            })?;

            Ok(())
        }

        /// Helper method to parse a player action (edge's) section of the text and fill in any
        /// name variables.
        ///
        /// The input text section should have the following format
        ///     'action text ::name:: more action text'
        ///
        /// Both the name and text buf are cleared at the beginning of this method
        pub fn parse_edge(
            text: &str,
            name_table: &HashMap<String, String>,
            text_buf: &mut String,
        ) -> Result<()> {
            // Implementation notes
            //  1. Due to the format, only even iterator elements are names that need to be looked
            //     up in the name table. This is true because split() will return an empty strings
            //     on sides of the separator with no text. For instance name::::name:: would split
            //     to ['name', '', 'name', '']
            //  2. This behavior is the opposite of parse_node. This is because parse_node strings
            //     start with the speaker name, where as for parse_edge strings, there is no
            //     speaker as it represents a player action

            text_buf.clear();
            let mut text_iter = text.split(TOKEN).enumerate();
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 0 {
                    // odd token
                    text_buf.push_str(n);
                    Ok(())
                } else {
                    // even token
                    println!("{}", n);
                    let value = name_table.get(n).ok_or(cmd::Error::Generic)?;
                    text_buf.push_str(value);
                    Ok(())
                }
            })?;

            Ok(())
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

        /// Rebuilds the text of a dialogue tree, removing unused sections and reordering text
        /// sections for improved caching of nearby nodes. The rebuilt string is then stored in
        /// the new_buf string buffer.
        ///
        /// When editing nodes/edges, currently new text is pushed to the end of the text buffer,
        /// and the indices of the node/edge are updated to point to the new text. This leaves the
        /// old section of text in the buffer, and over time many edits will bloat the string. The
        /// solution to this, without leaving gaps in the string, is to rebuild the text buffer
        /// based on the order that the text section is referenced in the tree. The order is
        /// determined by DFS order that the nodes occur, with all edges colocated immediately
        /// after their source node. This should provide good cache hitrate in most cases, as users
        /// are likely to follow DFS-like path through the tree as they make choices and advance
        /// through the dialogue.
        ///
        /// Note that the new_buf and new_tree are cleared at the beginning of this method.
        /// Make sure it is safe to do so before calling.
        pub fn rebuild_tree(
            text: &str,
            tree: &Tree,
            new_text: &mut String,
            new_tree: &mut Tree,
        ) -> Result<()> {
            new_text.clear();
            new_tree.clear();
            // Clone the old tree into the new one such that the nodes and edge indices and layout
            // are identical. This makes it much easier to rebuild as only the node weights need to
            // be updated to point to the proper sections of the next text buffer
            *new_tree = tree.clone();

            println!("cloned");

            let root_index = graph::node_index(0);
            let mut dfs = Dfs::new(&tree, root_index);
            while let Some(node_index) = dfs.next(&tree) {
                // Rebuild node
                let node = tree[node_index];
                let slice: &str = &text[node[0]..node[1]];
                let start = new_text.len();
                new_text.push_str(slice);
                let end = new_text.len();
                let new_node = new_tree
                    .node_weight_mut(node_index)
                    .ok_or(cmd::Error::InvalidNodeIndex)?;
                // verify new and old hash match
                let new_hash = hash(new_text[start..end].as_bytes());
                assert!(node.hash == new_hash);
                *new_node = Section::new([start, end], new_hash);

                // Rebuild all edges sourced from this node
                let edge_iter = tree.edges_directed(node_index, petgraph::Direction::Outgoing);
                for edge_ref in edge_iter {
                    let edge = edge_ref.weight();
                    let slice: &str = &text[edge.section[0]..edge.section[1]];

                    // Verify that edge and new_edge match, they should be identical since we
                    // started by cloning the tree to new_tree
                    assert!(
                        tree.edge_endpoints(edge_ref.id())
                            == new_tree.edge_endpoints(edge_ref.id())
                    );

                    let start = new_text.len();
                    new_text.push_str(slice);
                    let end = new_text.len();
                    let mut new_edge = new_tree
                        .edge_weight_mut(edge_ref.id())
                        .ok_or(cmd::Error::InvalidEdgeIndex)?;
                    // verify new and old hash match
                    let new_hash = hash(new_text[start..end].as_bytes());
                    assert!(edge.section.hash == new_hash);
                    new_edge.section = Section::new([start, end], new_hash);
                }
            }

            Ok(())
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

    let mut state = EditorState::new(DialogueTreeData::default());
    loop {
        // print default header
        println!("------------");
        println!("project: {}", state.act.name);
        println!("------------");

        cmd::util::prompt_input(&mut cmd_buf);

        let cmds = shellwords::split(&cmd_buf).unwrap();
        let cmd_result = cmd::Parse::from_iter_safe(cmds);

        // Handle results/errors
        match cmd_result {
            Ok(v) => match v.execute(&mut state) {
                Ok(_r) => println!("success"),
                // errors from dialogue_tree operations
                Err(f) => {
                    // pretty print top level error message
                    println!("\u{1b}[1;31merror:\u{1b}[0m {}", f);

                    // print the interesting bits of the stacktrace
                    // TODO: much to be improved here if backtrace.frames() can be
                    // pulled in
                    let s = format!("{}", f.backtrace());
                    let mut split = s.split("backtrace");
                    println!("{} . . .", split.next().unwrap());
                }
            },
            // errors from CLI interface
            Err(e) => println!("{}", e),
        }

        // clear input buffers before starting next input loop
        state.scratchpad.clear();
        cmd_buf.clear();
    }
}

/// Test code, generally these are integration level rather than unit level.
mod tests {
    use super::*;

    /// helper function to parse cmd_bufs in the same way the editor does
    #[inline(always)]
    fn run_cmd(cmd_buf: &str, state: &mut EditorState) -> Result<()> {
        let cmds = shellwords::split(&cmd_buf).unwrap();
        let res = cmd::Parse::from_iter_safe(cmds);
        let v = res.unwrap();
        v.execute(state)
    }

    #[test]
    /// Test basic use case of the editor, new project, add a few nodes and names, list the output,
    /// save the project, reload, list the output again
    fn simple_editor() {
        let mut cmd_buf = String::with_capacity(1000);
        let mut state = EditorState::new(DialogueTreeData::default());
        cmd_buf.push_str("new project \"simple_test\" -s");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        assert_eq!(state.act.name, "simple_test");

        cmd_buf.push_str("new name cat Behemoth");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        assert_eq!(state.act.name_table.get("cat").unwrap(), "Behemoth");

        cmd_buf.push_str("new node cat \"Well, who knows, who knows\"");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        cmd_buf.push_str(
            "new node cat \"'I protest!' ::cat:: exclaimed hotly. 'Dostoevsky is immortal'\"",
        );
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        cmd_buf.push_str("new edge 0 1 \"Dostoevsky's dead\"");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("list");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        let expected_list = concat!(
            "Behemoth : Well, who knows, who knows\r\n",
            "--> NodeIndex(1) : 0 : Dostoevsky's dead\r\n",
            "Behemoth : 'I protest!' Behemoth exclaimed hotly. 'Dostoevsky is immortal'\r\n"
        );
        assert_eq!(state.scratchpad, expected_list);
        state.scratchpad.clear();

        cmd_buf.push_str("save");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("load simple_test");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("list");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        assert_eq!(state.scratchpad, expected_list);
        state.scratchpad.clear();

        std::fs::remove_file("simple_test.tree").unwrap();
    }
}

/// Benchmarks are mainly created ad-hoc to help diagnose potential performance issues.
#[cfg(test)]
mod benchmarks {
    use super::*;
    use test::Bencher;

    #[bench]
    /// Benchmark node parsing worst case, many substitutions and improperly sized buffer
    fn stress_parse_node(b: &mut Bencher) {
        let mut name_table = HashMap::<String, String>::new();
        name_table.insert("Elle".to_string(), "Amberson".to_string());
        name_table.insert("Patrick".to_string(), "Breakforest".to_string());
        name_table.insert("Anna".to_string(), "Catmire".to_string());
        name_table.insert("Laura".to_string(), "Dagson".to_string());
        name_table.insert("John".to_string(), "Elliot".to_string());

        let text = "Elle::xzunz::Anna::lxn ::Elle::cn::Patrick::o::Laura::sokxt::Patrick::eowln
        ::Patrick::::John::c::Patrick::iw qyyhr.jxhccpyvchze::Anna::ox hi::Laura::nlv::John::peh
        swvnismjs::John::p::Laura::::John::slu.hlqzkei jhrskiswe::John::::John::m.rx::Patrick::pk
        te::Elle::h::John::m,z,.jwtol::Elle::h rwvnpuqw::John::::John::::Elle::tnz::Elle::.kv.
        ::Laura::xyxml jrsei::John::jlsl nysn::Patrick::mvvu.up::Laura::jh,t,,jnwheu npnxqcowev
        ::Anna::,::Elle::.emiv::John::ezoqy::Elle::cppyxtos,miqphi::Elle::.q c::Patrick::nzms
        skno::Laura:: mkzn.::Patrick::x::John::s jhl::John::ow::John::nj hsk::Elle::ihwpens rx
        ::Patrick::nn..iurtxcou::Laura::hypkqoyqyz.iihu::Elle::umcpvl::Patrick::::Anna::.cjh,cn
        phey::Patrick::hxorixcyr::Anna::u::Anna::  heuneszejtwrkewiymmq::John::ynjvh::Laura::lvvtunm
        ::Laura::i.rk::Patrick::hk::Elle::knvmml::John::j::Anna::::Anna::pslllnmtcyjzesls moj ttm
        ::Elle::jrr,mh,::Anna:: pyl::Anna::owunpjve::John::::Laura:: ::Anna::xci::Patrick::p::Laura::
        l.iwn::Elle::lnjx::Laura::oyo::Anna::eq,n::Elle::ej.::Laura::hh";

        b.iter(|| {
            let mut name_buf = String::with_capacity(1);
            let mut buf = String::with_capacity(1);
            let res = cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf);
            test::black_box(&res);
            test::black_box(&name_buf);
            test::black_box(&buf);
        });
    }

    #[bench]
    /// Benchmark standard node parsing case, few substitutions and pre-allocated buffer
    fn quick_parse_node(b: &mut Bencher) {
        let mut name_table = HashMap::<String, String>::new();
        name_table.insert("vamp".to_string(), "Dracula".to_string());
        name_table.insert("king".to_string(), "King Laugh".to_string());

        let text = "vamp::It is a strange world, a sad world, a world full of miseries, and woes, and 
        troubles. And yet when ::king:: come, he make them all dance to the tune he play. Bleeding hearts, 
        and dry bones of the churchyard, and tears that burn as they fall, all dance together to the music
        that he make with that smileless mouth of him. Ah, we men and women are like ropes drawn tight with
        strain that pull us different ways. Then tears come, and like the rain on the ropes, they brace us 
        up, until perhaps the strain become too great, and we break. But ::king:: he come like the
        sunshine, and he ease off the strain again, and we bear to go on with our labor, what it may be.";

        let mut name_buf = String::with_capacity(20);
        let mut buf = String::with_capacity(text.len() + 50);
        b.iter(|| {
            let res = cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf);
            test::black_box(&res);
            test::black_box(&name_buf);
            test::black_box(&buf);
        });
    }
}
