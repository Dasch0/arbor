pub use anyhow::Result;
pub use cmd::Executable;
use derive_new::*;
use enum_dispatch::*;
use log::{debug, info, trace};
use rayon::prelude::*;
use seahash::hash;
use serde::{Deserialize, Serialize};
use serde_diff::SerdeDiff;
pub use std::collections::{HashMap, VecDeque};
use std::hash::BuildHasherDefault;
use std::io;
use std::io::Write;
pub use std::ops::Range;
use structopt::clap::AppSettings;
pub use structopt::StructOpt;
use thiserror::Error;
use tree::Tree;

// TODO: Future plans
// 1. Replace petgraph with lower level graph implementation (petgraph limits us on diffing,
//    parallel iteration, and general lack of lower level access to data structures)

// TODO: Minor Features
// 1. More tests and benchmarks, focus on rebuild_tree
// 2. Add more help messages and detail for error types

// TODO: Targets for performance improvement
// 1. SPEED: Change dialogue/choice text in cmd Structs (new/edit node/edge) to use something other than a
//    heap allocated string. Right now string slices cannot be used with structopt, and each time a
//    cmd struct is created a heap allocation happens. This isn't all that frequent, but it still
//    incurs at least two unnessecary copies
// 2. FILE SIZE: right now the dialogue tree contains a lot of data that isn't technically needed
//    for just reading through the tree. Includes hashes, node positions. This could be optimized
//    by exporting a minimal struct type of tree that doesn't use any of that stuff
// 3. MEMORY: right now the DiffKind enum is super space inefficient. This means the undo/redo
//    history deque is mostly wasted space (around 75% of the buffer). This may be improved by
//    first, minimizing the enum size for different even types where possible, and more
//    intensely by serializing the diff of the entire EditorState and pushing it to a packed buffer
//    of u8's, but that introduces some validity considerations and serialization/deserialization
//    overhead. Additionally private members in petgraph block low-level access to perform diff

pub static TREE_EXT: &str = ".tree";
pub static BACKUP_EXT: &str = ".bkp";
pub static TOKEN_SEP: &str = "::";

pub const KEY_MAX_LEN: usize = 8;
pub const NAME_MAX_LEN: usize = 32;

/// Stack allocated string with max length suitable for keys
pub type KeyString = arrayvec::ArrayString<KEY_MAX_LEN>;

/// Stack allocated string with max length suitable for keys
pub type NameString = arrayvec::ArrayString<NAME_MAX_LEN>;

/// Struct for storing the 2d position of a node. Used for graph visualization
#[derive(new, Serialize, Deserialize, SerdeDiff, Clone, Copy)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

impl Default for Position {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// Struct representing a section of text in a rope. This section contains a start and end index,
/// stored in an array. The first element should always be smaller than the second. Additionally
/// the hash of that text section is stored in order to validate that the section is valid
//TODO: Is hash necessary for actually running the dialogue tree?
#[derive(new, Serialize, Deserialize, SerdeDiff, Clone, Copy)]
pub struct Section {
    /// A start and end index to some section of text
    pub text: [usize; 2],
    /// A hash of the text this section points to
    pub hash: u64,
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

/// Typedef representing the petgraph::Graph type used in dialogue trees. The nodes are made up of
/// Sections, which define slices of a text buffer. The edges are Choice structs, which define a
/// Section as well as data regarding different action types a player may perform
//pub type Tree = petgraph::stable_graph::StableGraph<Dialogue, Choice>;

pub mod tree {
    use super::*;

    /// Error types for tree operations
    ///
    /// Uses thiserror to generate messages for common situations. This does not
    /// attempt to implement From trait on any lower level error types, but relies
    /// on anyhow for unification and printing a stack trace
    #[derive(Error, Debug)]
    pub enum Error {
        #[error("Attempted to access a node that is not present in the tree")]
        InvalidNodeIndex,
        #[error("Attempted to access an edge that is not present in the tree")]
        InvalidEdgeIndex,
    }

    #[derive(new, Serialize, Deserialize, SerdeDiff, Clone)]
    pub struct Node {
        pub dialogue: Dialogue,
        #[serde_diff(opaque)]
        pub edge_range: Range<u32>,
    }

    /// Stores linking information between nodes and edges for a given dialogue tree
    #[derive(new, Serialize, Deserialize, SerdeDiff, Clone)]
    pub struct Link {
        pub source_index: usize,
        pub target_index: usize,
        pub edge_index: usize,
    }

    #[derive(new, Serialize, Deserialize, SerdeDiff, Clone)]
    pub struct Tree {
        /// Range defines the section of 'links' that contains outgoing edges for this node
        nodes: Vec<Node>,
        edges: Vec<Choice>,
        links: Vec<Link>,
    }

    impl Tree {
        pub fn with_capacity(node_capacity: usize, edge_capacity: usize) -> Self {
            Self {
                nodes: Vec::with_capacity(node_capacity as usize),
                edges: Vec::with_capacity(edge_capacity as usize),
                links: Vec::with_capacity(edge_capacity as usize),
            }
        }

        /// Get the contents of a node
        ///
        /// # Errors
        ///
        /// Error if node index is invalid
        #[inline]
        pub fn get_node(&self, node_index: usize) -> Result<Dialogue> {
            let node = self
                .nodes
                .get(node_index)
                .ok_or(tree::Error::InvalidNodeIndex)?;

            Ok(node.dialogue)
        }

        /// Push a new node onto the tree, and return the index of the added node
        #[inline]
        pub fn add_node(&mut self, dialogue: Dialogue) -> usize {
            // default range is 0..0
            self.nodes.push(Node::new(dialogue, Range::default()));
            self.nodes.len()
        }

        /// Edit the dialogue in an existing node and return the old dialogue
        ///
        /// # Errors
        ///
        /// If the index is invalid, a corresponding error will be returned with no modification to
        /// the tree.
        #[inline]
        pub fn edit_node(&mut self, index: usize, new_dialogue: Dialogue) -> Result<Dialogue> {
            trace!("attempt to get mutable weight from node index");
            let weight = self.nodes.get_mut(index).ok_or(Error::InvalidNodeIndex)?;
            let old_dialogue = weight.dialogue;

            weight.dialogue = new_dialogue;
            Ok(old_dialogue)
        }

        /// Remove a node if no edges use it as the source or target. Returns the weight of the
        /// removed node
        ///
        /// # Errors
        ///
        /// If the index is invalid, or if an edge currently uses the node as a source or target,
        /// an error is returned with no modification to the tree
        #[inline]
        pub fn remove_node(&mut self, index: usize) -> Result<Dialogue> {
            trace!("check that node index is valid");
            self.nodes.get(index).ok_or(tree::Error::InvalidNodeIndex)?;

            trace!("check that node is not in use in any edges");
            let mut node_in_use = false;
            for link in self.links {
                node_in_use |= link.source_index == index || link.target_index == index;
            }

            let removed_node = self.nodes.swap_remove(index);
            Ok(removed_node.dialogue)
        }

        /// Get the contents of an edge
        ///
        /// # Errors
        ///
        /// Error if edge index is invalid
        #[inline]
        pub fn get_edge(&self, edge_index: usize) -> Result<Choice> {
            let choice = self
                .edges
                .get(edge_index)
                .ok_or(tree::Error::InvalidEdgeIndex)?;

            Ok(*choice)
        }

        /// Get the (source, target) node indices of an edge
        #[inline]
        pub fn edge_endpoints(&self, edge_index: usize) -> Result<(usize, usize)> {
            let link = self
                .links
                .get(edge_index)
                .ok_or(tree::Error::InvalidEdgeIndex)?;
            Ok((link.source_index, link.target_index))
        }

        /// Create a new edge from a source node to a target node, return the index of the added edge
        ///
        /// # Errors
        ///
        /// If either the source or target node is invalid, a corresponding error will be returned
        /// with no modification to the tree.
        #[inline]
        pub fn add_edge(&mut self, source: usize, target: usize, choice: Choice) -> Result<usize> {
            trace!("check validity of source and target node");
            self.nodes
                .get(source)
                .ok_or(tree::Error::InvalidNodeIndex)?;
            self.nodes
                .get(target)
                .ok_or(tree::Error::InvalidNodeIndex)?;

            self.edges.push(choice);
            self.links.push(Link::new(source, target, self.edges.len()));

            Ok(self.edges.len())
        }

        /// Edit the choice in an existing edge. The source or target node cannot be modified, the
        /// edge will have to be deleted and rebuilt
        ///
        /// # Errors
        ///
        /// If the index is invalid, a corresponding error will be returned
        /// with no modification to the tree.
        #[inline]
        pub fn edit_edge(&mut self, index: usize, new_choice: Choice) -> Result<Choice> {
            trace!("check validity of edge index");
            let choice = self
                .edges
                .get_mut(index as usize)
                .ok_or(tree::Error::InvalidEdgeIndex)?;

            let old_choice = choice.clone();
            *choice = new_choice;
            Ok(old_choice)
        }

        /// Remove an existing edge from the tree and return the removed choice.
        ///
        /// Removing edges invalidates edge indices
        ///
        /// # Errors
        ///
        /// If the index is invalid, an error will be returned without modifying the tree
        #[inline]
        pub fn remove_edge(&mut self, index: usize) -> Result<Choice> {
            trace!("check validity of edge index");
            self.edges
                .get(index as usize)
                .ok_or(tree::Error::InvalidEdgeIndex)?;

            trace!("remove from links, then edges list");
            self.links.swap_remove(index);
            Ok(self.edges.swap_remove(index))
        }

        /// Get an immutable view of the edges in the tree
        pub fn edges(&self) -> &[Choice] {
            self.edges.as_slice()
        }
    }
}

/// Typedef representing the hashmap type used to store names in dialogue trees. These may be
/// substituted into the text before displaying, or updated by choices in the tree.
pub type NameTable = HashMap<KeyString, NameString>;

/// Typedef representing the hashmap type used to store values in dialogue trees. These are used as
/// requirements or effects from player choices.
pub type ValTable = HashMap<KeyString, u32>;

/// Top level data structure for storing a dialogue tree
///
/// This struct contains the tree representing the dialogue nodes and player actions connecting
/// them, the buffer which stores all text in a tightly packed manner, and hashtables for storing
/// variables such as player names, conditionals, etc.
#[derive(new, Serialize, Deserialize, SerdeDiff, Clone)]
pub struct DialogueTreeData {
    pub uid: usize,
    pub tree: Tree,
    pub text: String,
    #[serde_diff(opaque)]
    pub name_table: NameTable,
    #[serde_diff(opaque)]
    pub val_table: ValTable,
    pub name: String,
}

impl DialogueTreeData {
    pub fn default() -> Self {
        DialogueTreeData {
            uid: cmd::util::gen_uid(),
            tree: Tree::with_capacity(512, 2048),
            text: String::with_capacity(8192),
            name_table: HashMap::default(),
            val_table: HashMap::default(),
            name: String::new(),
        }
    }
}

/// State information for an editor instance. Includes two copies of the dialogue tree (one active
/// and one backup) as well as other state information
#[derive(Serialize, Deserialize, SerdeDiff)]
pub struct EditorState {
    pub active: DialogueTreeData,
    pub backup: DialogueTreeData,
    pub scratchpad: String,
}

impl EditorState {
    /// Create a new Editor state.
    ///
    /// Editor state needs to take ownership of the data. However since
    /// a backup copy needs to be created on construction, the data is moved, and then cloned
    pub fn new(data: DialogueTreeData) -> Self {
        EditorState {
            active: data.clone(),
            backup: data,
            scratchpad: String::with_capacity(1000),
        }
    }

    /// Swap the active and backup trees without copying any of the underlying data
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.active, &mut self.backup);
    }
}

/// Struct storing the information for a player choice. Stored in the edges of a dialogue tree
#[derive(new, Serialize, Deserialize, SerdeDiff, Clone, Copy)]
pub struct Choice {
    pub section: Section,
    pub requirement: ReqKind,
    pub effect: EffectKind,
}

/// Struct for storing the information for a line of dialogue. Stored in the nodes of a dialogue
/// tree
#[derive(new, Serialize, Deserialize, SerdeDiff, Clone, Copy)]
pub struct Dialogue {
    pub section: Section,
    pub pos: Position,
}

#[derive(Debug, Serialize, Deserialize, SerdeDiff, PartialEq, Clone, Copy)]
#[serde_diff(opaque)]
pub enum ReqKind {
    /// No requirement
    None,
    /// Must be greater than num
    Greater(KeyString, u32),
    /// Must be less than num
    Less(KeyString, u32),
    /// Must be equal to num
    Equal(KeyString, u32),
    /// Must match name string
    Cmp(KeyString, NameString),
}

impl std::str::FromStr for ReqKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        info!("Parsing ReqKind from string");
        // Implementation notes:
        // The enum string format is set up to directly map to how the enum is declared in rust:
        // e.g. 'GreaterThan(my_key,10)'
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
        debug!("{}", s);

        trace!("Check that first item is ''");
        anyhow::ensure!(split.next().ok_or(cmd::Error::Generic)?.is_empty());

        trace!(
            "second item should be number or string, check for valid length, wait to check if int"
        );
        let val = match NameString::from(split.next().ok_or(cmd::Error::Generic)?) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.simplify()),
        }?;

        trace!("third item should be key, check that the key is a valid length");
        // match required due to lifetime limitations on CapacityError
        let key = match KeyString::from(split.next().ok_or(cmd::Error::Generic)?) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.simplify()),
        }?;

        trace!("fourth item should be Enum type, build it!, and also try to resolve the val");
        match split.next().ok_or(cmd::Error::Generic)? {
            "Greater" => Ok(ReqKind::Greater(key, val.parse::<u32>()?)),
            "Less" => Ok(ReqKind::Less(key, val.parse::<u32>()?)),
            "Equal" => Ok(ReqKind::Equal(key, val.parse::<u32>()?)),
            "Cmp" => {
                // check name can be pulled from value
                // match required due to lifetime limitations on CapacityError
                let name = match NameString::from(val) {
                    Ok(v) => Ok(v),
                    Err(e) => Err(e.simplify()),
                }?;
                Ok(ReqKind::Cmp(key, name))
            }
            _ => Err(cmd::Error::Generic.into()),
        }
    }
}

/// Struct storing the data required to diff two ReqKind's for undo/redo
#[derive(Clone, Copy)]
#[serde_diff(opaque)]
pub struct ReqDiff {
    old: ReqKind,
    new: ReqKind,
}

/// Represents an effect that occurs when a choice is made.
///
/// Name length strings are stored as a heap allocated String rather than a static NameString as
/// that would bloat enum size by 32 bytes, when Cmp will rarely be used compared to val based
/// requirements
#[derive(Debug, Serialize, Deserialize, SerdeDiff, PartialEq, Clone, Copy)]
pub enum EffectKind {
    /// No effect
    No,
    Add(KeyString, u32),
    Sub(KeyString, u32),
    Set(KeyString, u32),
    Assign(KeyString, NameString),
}

impl std::str::FromStr for EffectKind {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        info!("Parsing EffectKind from string");
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
        debug!("{}", s);

        trace!("First item should be ''");
        anyhow::ensure!(split.next().ok_or(cmd::Error::Generic)?.is_empty());

        trace!("Second item should be number or string, don't check for validity yet");
        let val = split.next().ok_or(cmd::Error::Generic)?;

        trace!("Third item should be key, check that the key and name are of a valid length");
        // match required due to lifetime limitations on CapacityError
        let key = match KeyString::from(split.next().ok_or(cmd::Error::Generic)?) {
            Ok(v) => Ok(v),
            Err(e) => Err(e.simplify()),
        }?;

        trace!("fourth item should be Enum type, build it!, and also try to resolve the val");
        match split.next().ok_or(cmd::Error::Generic)? {
            "Add" => Ok(EffectKind::Add(key, val.parse::<u32>()?)),
            "Sub" => Ok(EffectKind::Sub(key, val.parse::<u32>()?)),
            "Set" => Ok(EffectKind::Set(key, val.parse::<u32>()?)),
            "Assign" => {
                // match required due to lifetime limitations on CapacityError
                let name = match NameString::from(val) {
                    Ok(v) => Ok(v),
                    Err(e) => Err(e.simplify()),
                }?;
                Ok(EffectKind::Assign(key, name))
            }
            _ => Err(cmd::Error::Generic.into()),
        }
    }
}

/// Struct storing the data required to diff two EffectKind's for undo/redo
#[derive(Clone, Copy)]
pub struct EffectDiff {
    old: EffectKind,
    new: EffectKind,
}

/// Top level module for all arbor commands. These commands rely heavily on the structopt
/// derive feature to easily implement a command line interface along with command structs for
/// input through other methods (UI, test code, etc.). In any structopt derived structure or enum,
/// the doc comments are displayed to the user through the CLI.
///
/// All commands also implement the generic Executable trait. This trait uses enum_dispatch to
/// propagate through to all types contained in the Parse enums. This executable method is where
/// the core logic of any command happens.
pub mod cmd {
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
        #[error("Node parsing failed")]
        NodeParse,
        #[error("Edge parsing failed")]
        EdgeParse,
        #[error("The name already exists")]
        NameExists,
        #[error("The name does not exist")]
        NameNotExists,
        #[error("The name is in use")]
        NameInUse,
        #[error("The value already exists")]
        ValExists,
        #[error("The value does not exist")]
        ValNotExists,
        #[error("The value is in use")]
        ValInUse,
        #[error("Attempted to access an invalid section of the text")]
        InvalidSection,
        #[error("Hash does not match text section")]
        InvalidHash,
        #[error("The event history is empty, undo not possible")]
        EventHistoryEmpty,
        #[error("The event future queue is empty, redo not possible")]
        EventFuturesEmpty,
        #[error("The undo operation failed")]
        UndoFailed,
        #[error("The redo operation failed")]
        RedoFailed,
    }

    /// Trait to allow structopt generated
    #[enum_dispatch]
    pub trait Executable {
        fn execute(&self, state: &mut EditorState) -> Result<usize>;
    }

    /// A tree based dialogue editor
    // NoBinaryName is set so that the first arg is not parsed as binary name when using
    // StructOpt::from_iter_safe
    // name is set as "" to prevent usage help from recommending to start commands with "arbor"
    #[enum_dispatch(Executable)]
    #[derive(StructOpt)]
    #[structopt(name="", setting = AppSettings::NoBinaryName)]
    pub enum Parse {
        New(new::Parse),
        Edit(edit::Parse),
        Remove(remove::Parse),
        Save(Save),
        Load(Load),
        Rebuild(Rebuild),
        Swap(Swap),
        List(List),
    }

    pub mod new {
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
            Val(new::Val),
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
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                let new_project = DialogueTreeData::new(
                    util::gen_uid(),
                    Tree::with_capacity(512, 2048),
                    String::with_capacity(8192),
                    HashMap::default(),
                    HashMap::default(),
                    self.name.clone(),
                );

                let encoded = bincode::serialize(&new_project)?;
                std::fs::write(self.name.clone() + TREE_EXT, encoded)?;

                if self.set_active {
                    *state = EditorState::new(new_project);
                }
                Ok(state.active.uid)
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
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Creating new node");

                trace!("verify the speaker name is valid");
                state
                    .active
                    .name_table
                    .get(self.speaker.as_str())
                    .ok_or(cmd::Error::NameNotExists)?;

                trace!("push dialogue to text buffer");
                let start = state.active.text.len();
                state.active.text.push_str(&format!(
                    "{}{}{}{}",
                    TOKEN_SEP, self.speaker, TOKEN_SEP, self.dialogue
                ));
                let end = state.active.text.len();
                debug!("start: {}, end: {}", start, end);

                trace!("compute hash from text section");
                let hash = hash(&state.active.text[start..end].as_bytes());
                debug!("hash {}", hash);

                let dialogue =
                    Dialogue::new(Section::new([start, end], hash), Position::new(0.0, 0.0));

                trace!("add new node to tree");
                let index = state.active.tree.add_node(dialogue);

                Ok(index)
            }
        }

        /// Create a new edge in the dialogue tree
        ///
        /// An edge represents an action from the player that connects two nodes
        #[derive(new, StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Edge {
            /// dialogue node index that this action originates from
            source: usize,
            /// dialogue node index that this action will lead to
            target: usize,
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
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Creating new edge");

                trace!("push choice text buffer");
                let start = state.active.text.len();
                state.active.text.push_str(&self.text);
                let end = state.active.text.len();
                debug!("start: {}, end: {}", start, end);

                trace!("Compute hash from text section");
                let hash = hash(&state.active.text[start..end].as_bytes());
                debug!("hash {}", hash);

                trace!("Validate that any requirements/effects reference valid hashmap keys");
                if self.requirement.is_some() {
                    util::validate_requirement(
                        self.requirement.as_ref().ok_or(cmd::Error::Generic)?,
                        &state.active.name_table,
                        &state.active.val_table,
                    )?;
                }
                if self.effect.is_some() {
                    util::validate_effect(
                        self.effect.as_ref().ok_or(cmd::Error::Generic)?,
                        &state.active.name_table,
                        &state.active.val_table,
                    )?;
                }

                let choice = Choice::new(
                    Section::new([start, end], hash),
                    self.requirement.clone().unwrap_or(ReqKind::None),
                    self.effect.clone().unwrap_or(EffectKind::No),
                );

                trace!("Adding new edge to tree");
                let edge_index =
                    state
                        .active
                        .tree
                        .add_edge(self.source, self.target, choice.clone())?;
                Ok(edge_index)
            }
        }

        /// Create a new name for use in dialogue nodes and actions
        ///
        /// A name represents some variable that may be substituted into the text. Examples
        /// include player names, pronouns, and character traits
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Name {
            /// The keyword to reference the name with in the text. Maximum length of 8 characters
            key: KeyString,
            /// The name to store, able be updated by player actions. Maximum length of 32
            /// characters
            name: NameString,
        }
        impl Executable for Name {
            /// New Name
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Create new name");

                trace!("check that key does not already exist");
                if state.active.name_table.get(self.key.as_str()).is_none() {
                    trace!("add key and name to table");
                    state.active.name_table.insert(self.key, self.name);
                    Ok(0)
                } else {
                    Err(cmd::Error::NameExists.into())
                }
            }
        }

        /// Create a new value for use in dialogue nodes and actions
        ///
        /// A value represents some variable number that is used as requirements and effects for
        /// choices. Examples include player skill levels, relationship stats, and presence of an item.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Val {
            /// The keyword to reference the value with in the dialogue tree. Max length of 8
            /// characters
            key: KeyString,
            /// Value to store, able be updated by player actions
            value: u32,
        }
        impl Executable for Val {
            /// New Val
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Create new val");

                trace!("check that key does not already exist");
                if state.active.val_table.get(self.key.as_str()).is_none() {
                    trace!("add key and val to table");
                    state.active.val_table.insert(self.key, self.value);
                    Ok(self.value as usize)
                } else {
                    Err(cmd::Error::ValExists.into())
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
            Name(edit::Name),
            Val(edit::Val),
        }

        /// Edit the contents of a node in the dialogue tree
        ///
        /// A node represents a text a segment of dialogue from a character.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Node {
            /// Index of the node to edit
            node_index: usize,
            /// The speaker for this node
            speaker: KeyString,
            /// The text or action for this node
            dialogue: String,
        }
        impl Executable for Node {
            /// Edit Node
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Edit node {}", self.node_index);

                trace!("push new dialogue to text buffer");
                let start = state.active.text.len();
                state.active.text.push_str(&format!(
                    "{}{}{}{}",
                    TOKEN_SEP, self.speaker, TOKEN_SEP, self.dialogue
                ));
                let end = state.active.text.len();

                trace!("get node weight from tree");
                let old_node = state.active.tree.get_node(self.node_index)?;

                trace!("recalculate hash");
                let hash = hash(state.active.text[start..end].as_bytes());
                debug!("hash {}", hash);

                let new_node = Dialogue::new(Section::new([start, end], hash), old_node.pos);

                trace!("update node weight in tree");
                state.active.tree.edit_node(self.node_index, new_node)?;

                Ok(self.node_index)
            }
        }

        /// Edit the contents of an edge in the dialogue tree
        ///
        /// The source and target node of an edge may not be edited, you must remove the edge and
        /// then create a new one to do this.
        #[derive(new, StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Edge {
            /// Id of the edge to edit
            edge_index: usize,
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
            /// Edit Edge
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Edit edge {}", self.edge_index);

                trace!("push choice to text buffer");
                let start = state.active.text.len();
                state.active.text.push_str(&self.text);
                let end = state.active.text.len();

                trace!("recalculate hash");
                let hash = hash(state.active.text[start..end].as_bytes());
                debug!("hash {}", hash);

                trace!("validate that any requirements/effects reference valid hashmap keys");
                if self.requirement.is_some() {
                    util::validate_requirement(
                        self.requirement.as_ref().ok_or(cmd::Error::Generic)?,
                        &state.active.name_table,
                        &state.active.val_table,
                    )?;
                }
                if self.effect.is_some() {
                    util::validate_effect(
                        self.effect.as_ref().ok_or(cmd::Error::Generic)?,
                        &state.active.name_table,
                        &state.active.val_table,
                    )?;
                }

                trace!("get edge weight from tree");
                let old_weight = state.active.tree.get_edge(self.edge_index)?;

                let new_weight = Choice::new(
                    Section::new([start, end], hash),
                    self.requirement.clone().unwrap_or(ReqKind::None),
                    self.effect.clone().unwrap_or(EffectKind::No),
                );

                trace!("update edge weight in tree");
                state.active.tree.edit_edge(self.edge_index, new_weight);

                Ok(self.edge_index)
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
            key: KeyString,
            /// Value of the name to store
            name: NameString,
        }

        impl Executable for Name {
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Edit name {}", self.key);

                trace!("check that key exists before editing");
                if state.active.name_table.get(&self.key).is_some() {
                    let name = state
                        .active
                        .name_table
                        .get_mut(&self.key)
                        .ok_or(cmd::Error::Generic)?;
                    debug!("old name: {}, new name: {}", name, self.name);

                    trace!("update key-value in name table");
                    *name = self.name;
                    Ok(0)
                } else {
                    Err(cmd::Error::NameNotExists.into())
                }
            }
        }

        /// Edit an existing value
        ///
        /// A value represents some variable number that is used as requirements and effects for
        /// choices. Examples include player skill levels, relationship stats, and presence of an item.
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Val {
            /// The keyword to reference the name with in the text
            key: KeyString,
            /// Value to store to the name
            value: u32,
        }

        impl Executable for Val {
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Edit val {}", self.key);

                trace!("check that key exists before editing");
                if state.active.name_table.get(&self.key).is_some() {
                    let old_value = state
                        .active
                        .val_table
                        .get_mut(&self.key)
                        .ok_or(cmd::Error::Generic)?;
                    debug!("old val: {}, new val: {}", old_value, self.value);

                    trace!("update key-value in value table");
                    *old_value = self.value;

                    Ok(self.value as usize)
                } else {
                    Err(cmd::Error::ValNotExists.into())
                }
            }
        }
    }

    mod remove {
        use super::*;

        /// Remove existing things
        #[enum_dispatch(Executable)]
        #[derive(StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub enum Parse {
            Node(remove::Node),
            Edge(remove::Edge),
            Name(remove::Name),
            Val(remove::Val),
        }

        /// Remove the contents of a node in the dialogue tree and return the hash of the removed
        /// node's text section
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Node {
            /// Index of the node to remove
            node_index: usize,
        }
        impl Executable for Node {
            /// Remove Node
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Remove node {}", self.node_index);

                let removed_weight = state.active.tree.remove_node(self.node_index)?;
                let hash = removed_weight.section.hash;
                Ok(hash as usize)
            }
        }

        /// Remove an edge from the dialogue tree and return the hash of the removed edge's text
        /// section
        #[derive(new, StructOpt)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Edge {
            /// Id of the edge to remove
            edge_index: usize,
        }

        impl Executable for Edge {
            /// Remove Edge
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Remove Edge {}", self.edge_index);

                trace!("fetch source and target not for event history");
                let (source, target) = state.active.tree.edge_endpoints(self.edge_index)?;

                trace!("remove edge from tree");
                let removed_weight = state.active.tree.remove_edge(self.edge_index)?;
                let hash = removed_weight.section.hash;

                Ok(hash as usize)
            }
        }

        /// Remove a name, only allowed if the name is not used anywhere
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Name {
            /// The keyword to reference the name with in the text
            key: KeyString,
        }

        impl Executable for Name {
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                info!("Remove Name {}", self.key);

                trace!("check if the key is referenced anywhere in the text");
                if let Some(_found) = state
                    .active
                    .text
                    .find(format!("{}{}{}", TOKEN_SEP, self.key, TOKEN_SEP).as_str())
                {
                    return Err(cmd::Error::NameInUse.into());
                }

                trace!("check if the key is referenced in any requirements or effects");
                for choice in state.active.tree.edges() {
                    // this match will stop compiling any time a new reqKind is added
                    match &choice.requirement {
                        ReqKind::None => Ok(()),
                        ReqKind::Greater(_, _) => Ok(()),
                        ReqKind::Less(_, _) => Ok(()),
                        ReqKind::Equal(_, _) => Ok(()),
                        ReqKind::Cmp(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                    }?;
                    match &choice.effect {
                        EffectKind::No => Ok(()),
                        EffectKind::Add(_, _) => Ok(()),
                        EffectKind::Sub(_, _) => Ok(()),
                        EffectKind::Set(_, _) => Ok(()),
                        EffectKind::Assign(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                    }?;
                }

                trace!("remove key-value pair from name table");
                let removed_name = state
                    .active
                    .name_table
                    .remove(self.key.as_str())
                    .ok_or(cmd::Error::NameNotExists)?;

                Ok(0)
            }
        }

        /// Remove a value, only allowed if the value is not used anywhere
        #[derive(new, StructOpt, Debug)]
        #[structopt(setting = AppSettings::NoBinaryName)]
        pub struct Val {
            /// The keyword to reference the name with in the text
            key: KeyString,
        }

        impl Executable for Val {
            fn execute(&self, state: &mut EditorState) -> Result<usize> {
                trace!("check if the key is referenced in any requirements or effects");
                for choice in state.active.tree.edges() {
                    // this match will stop compiling any time a new reqKind is added
                    match &choice.requirement {
                        ReqKind::None => Ok(()),
                        ReqKind::Greater(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        ReqKind::Less(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        ReqKind::Equal(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        ReqKind::Cmp(_, _) => Ok(()),
                    }?;
                    match &choice.effect {
                        EffectKind::No => Ok(()),
                        EffectKind::Add(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        EffectKind::Sub(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        EffectKind::Set(key, _) => {
                            if key.eq(self.key.as_str()) {
                                Err(cmd::Error::NameInUse)
                            } else {
                                Ok(())
                            }
                        }
                        EffectKind::Assign(_, _) => Ok(()),
                    }?;
                }

                trace!("remove key-value pair from value table");
                let removed_value = state
                    .active
                    .val_table
                    .remove(self.key.as_str())
                    .ok_or(cmd::Error::NameNotExists)?;

                Ok(0)
            }
        }
    }

    /// Undo the last event that modified the dialogue tree
    ///
    /// Rebuilding the tree removes the entire undo/redo history. Undo does not interact with file
    /// level operations such as saving or loading projects
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Undo {}

    impl Executable for Undo {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            info!("Undo");

            Ok(0)
        }
    }

    /// Redo the last undo event that modified the dialogue tree
    ///
    /// Rebuilding the tree removes the entire undo/redo history. Redo does not interact with file
    /// level operations such as saving or loading projects
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Redo {}

    impl Executable for Redo {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            info!("Redo");

            Ok(0)
        }
    }
    /// Save the current project
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Save {}

    impl Executable for Save {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            info!("Save project");
            let encoded = bincode::serialize(&state.active)?;
            std::fs::write(state.active.name.clone() + TREE_EXT, encoded)?;

            trace!("save successful, sync backup with active copy");
            state.backup = state.active.clone();

            Ok(state.active.uid)
        }
    }

    /// Rebuild the tree and text buffer for efficient access and memory use. Rebuilding the tree
    /// erases the undo/redo history.
    ///
    /// Rebuilding the tree is used to remove unused sections of text from the buffer. It performs
    /// a DFS search through the tree, and creates a new tree and text buffer where the text sections
    /// of a node and its outgoing edges are next to each other. This rebuilding process has a risk
    /// of corrupting the tree, so a backup copy is is saved before hand. The backup is stored both
    /// in memory and copied to disk as project_name.tree.bkp. To use the backup copy, either call
    /// the swap subcommand to load from memory, or remove the .bkp tag from the end of the file
    /// and then load it.
    ///
    /// Since the rebuild tree cleans out any artifacts from edits/removals, the undo/redo
    ///
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Rebuild {}

    impl Executable for Rebuild {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            // save states to backup buffer
            state.backup = state.active.clone();

            // save backup to filesystem
            let encoded = bincode::serialize(&state.active)?;
            std::fs::write(state.active.name.clone() + TREE_EXT + BACKUP_EXT, encoded)?;

            // attempt rebuild tree on active buffer, backup buffer is used as source
            util::rebuild_tree(
                &state.backup.text,
                &state.backup.tree,
                &mut state.active.text,
                &mut state.active.tree,
            )?;

            // Confirm that that rebuilt tree is valid
            util::validate_tree(&state.active)?;

            Ok(state.active.uid)
        }
    }

    /// Load a project from disk, will overwrite unsaved changes
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Load {
        name: String,
    }

    impl Executable for Load {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            let new_state = EditorState::new(bincode::deserialize_from(std::io::BufReader::new(
                std::fs::File::open(self.name.clone() + TREE_EXT)?,
            ))?);
            // check that the loaded tree is valid before loading into main state
            util::validate_tree(&state.active)?;
            *state = new_state;
            Ok(state.active.uid)
        }
    }

    /// Swap the backup and active trees.
    ///
    /// The backup tree stores the state from the last new, load, save, or just before a rebuild
    /// is attempted. This is mainly useful as a recovery option if the active tree gets corrupted.
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct Swap {}

    impl Executable for Swap {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            std::mem::swap(&mut state.active, &mut state.backup);
            Ok(state.active.uid)
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
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            let mut name_buf = String::with_capacity(64);
            let mut text_buf = String::with_capacity(256);
            let node_iter = state.active.tree.node_references();

            for (idx, n) in node_iter {
                let text = &state.active.text[n.section[0]..n.section[1]];
                util::parse_node(text, &state.active.name_table, &mut name_buf, &mut text_buf)?;
                state.scratchpad.push_str(&format!(
                    "node {}: {} says \"{}\"\r\n",
                    idx.index(),
                    name_buf,
                    text_buf
                ));
                for e in state
                    .active
                    .tree
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                {
                    let choice = e.weight();
                    util::parse_edge(
                        &state.active.text[choice.section[0]..choice.section[1]],
                        &state.active.name_table,
                        &mut text_buf,
                    )?;
                    state.scratchpad.push_str(&format!(
                        "--> edge {} to node {}: \"{}\"\r\n    requirements: {:?}, effects: {:?}\r\n",
                        e.id().index(),
                        e.target().index(),
                        text_buf,
                        choice.requirement,
                        choice.effect,
                    ));
                }
            }
            println!("{}", state.scratchpad);
            Ok(state.active.uid)
        }
    }

    /// Utility methods used internally for various useful tasks. These cannot be called directly
    /// from the command line, but are useful for working with dialogue_trees in other programs
    pub mod util {
        use super::*;

        /// Push even to the event history, and clear the event futures queue.
        ///
        /// When a new event is pushed to the undo history through this method, it indicates that
        /// new edits are being made on top of the existing history, and this means any redo
        /// attempts are no longer valid
        #[inline]
        pub fn push_event(event: Event, state: &mut EditorState) {
            trace!("push event to history queue");
            state.future.clear();
            state.history.push_front(event);
        }

        /// Generate UID.
        ///
        /// UID is a 64 bit unique identifier for the project. This is stored in the dialogue
        /// tree, and is useful for associating other metadata or resources with the correct tree
        /// in the case that multiple files exist with the same name (likely if multiple users are
        /// sharing files)
        pub fn gen_uid() -> usize {
            rand::random::<usize>()
        }

        /// Helper method to parse a dialogue node's section of the text and fill in any name
        /// variables.
        ///
        /// The input text rope section should have the following format
        ///     ::name::text ::name:: more text
        ///
        /// The first name is the speaker. This name must be a valid key to the name_table
        /// Inside the text, additional names may be inserted inside a pair of :: symbols. The
        /// entire area inside the :: symbols must be a valid key to the name_table.
        ///
        /// Both the name and text buf are cleared at the beginning of this method.
        pub fn parse_node(
            text: &str,
            name_table: &NameTable,
            name_buf: &mut String,
            text_buf: &mut String,
        ) -> Result<()> {
            // Implementation notes:
            //  0. The first iterator element should always be '', if not something is wrong
            //  1. The second iterator element is always the speaker name and should be the only
            //     thing written to the name buffer
            //  2. Since only a simple flow of ::speaker_name::text::name:::text ... etc is
            //     allowed, only every 'other' token (indices 1,3,5...) need to be looked up in the
            //     hashtable
            //  3. The above is only true because split() will return an empty strings on sides of
            //     the separator with no text. For instance name::::name:: would split to ['name,
            //     '', name, '']
            name_buf.clear();
            text_buf.clear();
            let mut text_iter = text.split(TOKEN_SEP).enumerate();
            let _ = text_iter.next(); // skip first token, it is '' for any correct string
            let speaker_key = text_iter.next().ok_or(cmd::Error::Generic)?.1;
            let speaker_name = name_table.get(speaker_key).ok_or(cmd::Error::NodeParse)?;
            name_buf.push_str(speaker_name);
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 1 {
                    println!("{}:{}", i, n);
                    // token is a name (index 1, 3, 5 ...)
                    let value = name_table.get(n).ok_or(cmd::Error::NodeParse)?;
                    text_buf.push_str(value);
                    Ok(())
                } else {
                    // token cannot be a name
                    text_buf.push_str(n);
                    Ok(())
                }
            })?;

            Ok(())
        }

        /// Same routine as parse node, except the results are not actually written to a
        /// thread. This is used for validating that the section of text is valid
        pub fn validate_node(text: &str, name_table: &NameTable) -> Result<()> {
            let mut text_iter = text.split(TOKEN_SEP).enumerate();
            text_iter.next(); // discard first empty string
            let speaker_key = text_iter.next().ok_or(cmd::Error::EdgeParse)?.1;
            name_table.get(speaker_key).ok_or(cmd::Error::EdgeParse)?;
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 1 {
                    // token is a name (index 1, 3, 5 ...)
                    name_table.get(n).ok_or(cmd::Error::EdgeParse)?;
                    Ok(())
                } else {
                    // token cannot be a name
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
        pub fn parse_edge(text: &str, name_table: &NameTable, text_buf: &mut String) -> Result<()> {
            // Implementation notes
            //  1. Due to the format, only even iterator elements are names that need to be looked
            //     up in the name table. This is true because split() will return an empty strings
            //     on sides of the separator with no text. For instance name::::name:: would split
            //     to ['name', '', 'name', '']
            text_buf.clear();
            let mut text_iter = text.split(TOKEN_SEP).enumerate();
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                println!("{}:{}", i, n);
                if (i & 0x1) == 0 {
                    // token cannot be a name
                    text_buf.push_str(n);
                    Ok(())
                } else {
                    // token is a name
                    println!("{}", n);
                    let value = name_table.get(n).ok_or(cmd::Error::EdgeParse)?;
                    text_buf.push_str(value);
                    Ok(())
                }
            })?;
            Ok(())
        }

        /// Same routine as parse_edge, but does not write to an output string buffer. Useful for
        /// validating a section of text in an edge
        pub fn validate_edge(text: &str, name_table: &NameTable) -> Result<()> {
            let mut text_iter = text.split(TOKEN_SEP).enumerate();
            text_iter.try_for_each(|(i, n)| -> std::result::Result<(), cmd::Error> {
                if (i & 0x1) == 0 {
                    Ok(())
                } else {
                    name_table.get(n).ok_or(cmd::Error::Generic)?;
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

            let root_index = stable_graph::node_index(0);
            let mut dfs = Dfs::new(&tree, root_index);
            while let Some(node_index) = dfs.next(&tree) {
                // Rebuild node
                let node = tree[node_index];
                let slice: &str = &text[node.section[0]..node.section[1]];
                let start = new_text.len();
                new_text.push_str(slice);
                let end = new_text.len();
                let new_node = new_tree
                    .node_weight_mut(node_index)
                    .ok_or(cmd::Error::InvalidNodeIndex)?;
                // verify new and old hash match
                let new_hash = hash(new_text[start..end].as_bytes());
                assert!(node.section.hash == new_hash);
                *new_node = Dialogue::new(Section::new([start, end], new_hash), node.pos);

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

        /// Validate that the contents of a requirement enum are valid
        ///
        /// This is mainly used when taking a requirement from CLI and checking that the key
        /// is present in the val_table for u32 types, and the name_table for String types
        pub fn validate_requirement(
            req: &ReqKind,
            name_table: &NameTable,
            val_table: &ValTable,
        ) -> Result<()> {
            // this match will stop compiling any time a new reqKind is added
            match req {
                ReqKind::None => {}
                ReqKind::Greater(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                ReqKind::Less(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                ReqKind::Equal(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                ReqKind::Cmp(key, _val) => {
                    name_table.get(key).ok_or(cmd::Error::NameNotExists)?;
                }
            }
            Ok(())
        }

        /// Validate that the contents of a effect enum are valid
        ///
        /// This is mainly used when taking a effect from CLI and checking that the key
        /// is present in the val_table for u32 types, and the name_table for String types
        pub fn validate_effect(
            effect: &EffectKind,
            name_table: &NameTable,
            val_table: &ValTable,
        ) -> Result<()> {
            // this match will stop compiling any time a new EffectKind is added
            // NOTE: remember, if val is a u32, check the val_table, if val is a String, check the
            // name table
            match effect {
                EffectKind::No => {}
                EffectKind::Add(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                EffectKind::Sub(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                EffectKind::Set(key, _val) => {
                    val_table.get(key).ok_or(cmd::Error::ValNotExists)?;
                }
                EffectKind::Assign(key, _val) => {
                    name_table.get(key).ok_or(cmd::Error::NameNotExists)?;
                }
            }
            Ok(())
        }

        /// Validate that a given dialogue tree data structure contains all valid sections of text
        /// that all edges point to valid nodes in the tree, all have valid action enums, and have
        /// have correct hashes for all nodes and edges
        ///
        /// Returns a result with the error type if the tree was invalid, returns Ok(()) if valid
        pub fn validate_tree(data: &DialogueTreeData) -> Result<()> {
            // check nodes first, use parallel iterator in case of very large graph
            let nodes_iter = data.tree.node_references().par_bridge();
            nodes_iter.try_for_each(|(_index, node)| -> Result<()> {
                // try to grab the text section as a slice, and return an error if the get() failed
                let slice = data.text[..]
                    .get(node.section[0]..node.section[1])
                    .ok_or(cmd::Error::InvalidSection)?;
                // if the slice was successful, check its hash
                anyhow::ensure!(
                    seahash::hash(slice.as_bytes()) == node.section.hash,
                    cmd::Error::InvalidHash
                );
                // Check that the section of text parses successfully (all names present in the
                // name_table)
                validate_node(slice, &data.name_table)?;
                Ok(())
            })?;

            // check edges, will check that they point to nodes that exist, and validate the actionenums
            let mut edges_iter = data.tree.edges(NodeIndex::new(0));
            edges_iter.try_for_each(|edge| -> Result<()> {
                // try to grab the text section as a slice, and return an error if the get() failed
                let slice = data.text[..]
                    .get(edge.weight().section[0]..edge.weight().section[1])
                    .ok_or(cmd::Error::InvalidSection)?;
                // if the slice was successful, check its hash
                anyhow::ensure!(
                    seahash::hash(slice.as_bytes()) == edge.weight().section.hash,
                    cmd::Error::InvalidHash
                );
                // Check that the section of text parses successfully (all names present in the
                // name_table)
                validate_edge(slice, &data.name_table)?;
                validate_requirement(
                    &edge.weight().requirement,
                    &data.name_table,
                    &data.val_table,
                )?;
                validate_effect(&edge.weight().effect, &data.name_table, &data.val_table)?;
                Ok(())
            })?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn setup_logger() {
        simple_logger::SimpleLogger::new().init().unwrap();
    }

    /// helper function to parse cmd_bufs in the same way the editor does
    #[inline(always)]
    #[allow(dead_code)]
    fn run_cmd(cmd_buf: &str, state: &mut EditorState) -> Result<usize> {
        let cmds = shellwords::split(&cmd_buf).unwrap();
        let res = cmd::Parse::from_iter_safe(cmds);
        let v = res.unwrap();
        v.execute(state)
    }

    #[test]
    /// Test basic use case of the editor, new project, add a few nodes and names, list the output,
    /// save the project, reload, list the output again
    fn simple() {
        setup_logger();
        let mut cmd_buf = String::with_capacity(1000);
        let mut state = EditorState::new(DialogueTreeData::default());
        cmd_buf.push_str("new project \"simple_test\" -s");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        assert_eq!(state.active.name, "simple_test");

        cmd_buf.push_str("new name cat Behemoth");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        assert_eq!(state.active.name_table.get("cat").unwrap(), "Behemoth");

        cmd_buf.push_str("new val rus_lit 50");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        assert_eq!(*state.active.val_table.get("rus_lit").unwrap(), 50);

        cmd_buf.push_str("new node cat \"Well, who knows, who knows\"");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        cmd_buf.push_str(
            "new node cat \"'I protest!' ::cat:: exclaimed hotly. 'Dostoevsky is immortal'\"",
        );
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();
        cmd_buf
            .push_str("new edge -r Less(rus_lit,51) -e Sub(rus_lit,1) 0 1 \"Dostoevsky's dead\"");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("list");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        let expected_list = concat!(
            "node 0: Behemoth says \"Well, who knows, who knows\"\r\n",
            "--> edge 0 to node 1: \"Dostoevsky's dead\"\r\n",
            "    requirements: Less(\"rus_lit\", 51), effects: Sub(\"rus_lit\", 1)\r\n",
            "node 1: Behemoth says \"'I protest!' Behemoth exclaimed hotly. 'Dostoevsky is immortal'\"\r\n",
        );
        assert_eq!(state.scratchpad, expected_list);
        state.scratchpad.clear();

        cmd_buf.push_str("save");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("load simple_test");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("rebuild");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        cmd_buf.push_str("list");
        run_cmd(&cmd_buf, &mut state).unwrap();
        cmd_buf.clear();

        assert_eq!(state.scratchpad, expected_list);
        state.scratchpad.clear();

        std::fs::remove_file("simple_test.tree").unwrap();
        std::fs::remove_file("simple_test.tree.bkp").unwrap();
    }

    #[test]
    // TODO: move to criterion
    /// Benchmark node parsing worst case, many substitutions and improperly sized buffer
    fn stress_parse_node() {
        let mut name_table = NameTable::default();
        name_table.insert(
            KeyString::from("Elle").unwrap(),
            NameString::from("Amberson").unwrap(),
        );
        name_table.insert(
            KeyString::from("Patrick").unwrap(),
            NameString::from("Breakforest").unwrap(),
        );
        name_table.insert(
            KeyString::from("Anna").unwrap(),
            NameString::from("Catmire").unwrap(),
        );
        name_table.insert(
            KeyString::from("Laura").unwrap(),
            NameString::from("Dagson").unwrap(),
        );
        name_table.insert(
            KeyString::from("John").unwrap(),
            NameString::from("Elliot").unwrap(),
        );

        let text = "::Elle::xzunz::Anna::lxn ::Elle::cn::Patrick::o::Laura::sokxt::Patrick::eowln
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

        // bench part
        let mut name_buf = String::with_capacity(1);
        let mut buf = String::with_capacity(1);
        cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
    }

    #[test]
    /// Benchmark standard node parsing case, few substitutions and pre-allocated buffer
    fn quick_parse_node() {
        let mut name_table = NameTable::default();
        name_table.insert(
            KeyString::from("vamp").unwrap(),
            NameString::from("Dracula").unwrap(),
        );
        name_table.insert(
            KeyString::from("king").unwrap(),
            NameString::from("King Laugh").unwrap(),
        );

        let text = "::vamp::It is a strange world, a sad world, a world full of miseries, and woes, and 
        troubles. And yet when ::king:: come, he make them all dance to the tune he play. Bleeding hearts, 
        and dry bones of the churchyard, and tears that burn as they fall, all dance together to the music
        that he make with that smileless mouth of him. Ah, we men and women are like ropes drawn tight with
        strain that pull us different ways. Then tears come, and like the rain on the ropes, they brace us 
        up, until perhaps the strain become too great, and we break. But ::king:: he come like the
        sunshine, and he ease off the strain again, and we bear to go on with our labor, what it may be.";

        let mut name_buf = String::with_capacity(20);
        let mut buf = String::with_capacity(text.len() + 50);

        // bench part
        cmd::util::parse_node(text, &name_table, &mut name_buf, &mut buf).unwrap();
    }

    /// Sanity check that the index on stablegraph nodes and indices are preserved across removes
    /// and adds. This is not guaranteed by stable_graph docs
    #[test]
    fn stable_graph_remove_add() {
        let dia = Dialogue::new(Section::new([0, 10], 0), Position::new(0.0, 0.0));

        // create dummy graph with a bunch of nodes
        let mut tree = Tree::new();
        for i in 0..100 {
            let mut node = dia.clone();
            node.section.hash = i;
            tree.add_node(node);
        }

        // confirm that all indices equal the hash for added nodes before doing any removals
        for index in tree.node_indices() {
            assert_eq!(
                index.index() as u64,
                tree.node_weight(index).unwrap().section.hash
            );
        }

        // remove some random nodes
        let weight_0 = tree.remove_node(NodeIndex::new(0)).unwrap();
        let weight_57 = tree.remove_node(NodeIndex::new(57)).unwrap();
        let weight_26 = tree.remove_node(NodeIndex::new(26)).unwrap();

        // re-add some nodes in reverse order that they were removed
        // check that the indices are as expected
        assert_eq!(tree.add_node(weight_26), NodeIndex::new(26));
        assert_eq!(tree.add_node(weight_57), NodeIndex::new(57));
        assert_eq!(tree.add_node(weight_0), NodeIndex::new(0));
    }

    #[test]
    fn undo_redo() {
        setup_logger();
        let mut state = EditorState::new(DialogueTreeData::default());
        // create dummy project and speaker name
        cmd::new::Project::new("undo_redo".to_string(), true)
            .execute(&mut state)
            .unwrap();
        let speaker = KeyString::from("test").unwrap();
        cmd::new::Name::new(speaker, NameString::from("undo_redo").unwrap())
            .execute(&mut state)
            .unwrap();

        // create dummy nodes and edges
        for i in 0..100 {
            cmd::new::Node::new(speaker.to_string(), format!("test dialogue {}", i));
        }
        for i in 0..99 {
            cmd::new::Edge::new(i, i + 1, format!("test choice {}", i), None, None);
        }

        // undo every other node
    }
}
