pub mod editor;
pub mod ensure;
pub mod stack_str;
pub mod tree;

use ensure::ensure;
use log::{debug, info, trace};
use nanoserde::{DeJson, SerJson};
use stack_str::StackStr;
use std::num::ParseIntError;
use std::{collections::HashMap, fmt};
use tree::event::{EdgeEdit, EdgeInsert, EdgeRemove, LinkMove, NodeEdit, NodeInsert, NodeRemove};
pub use tree::Tree;

// TODO: Minor Features
// 1. More tests and benchmarks, focus on rebuild_tree
// 2. Add more help messages and detail for error types
// 3. Change tree link list to be double ended, drastically improve lookup times when changing
//    placement

// TODO: Targets for performance improvement
// 1. SPEED: Change dialogue/choice text in cmd Structs (new/edit node/edge) to use something other than a
//    heap allocated string. Right now string slices cannot be used with structopt, and each time a
//    cmd struct is created a heap allocation happens. This isn't all that frequent, but it still
//    incurs at least two unnessecary copies
// 2. FILE SIZE: right now the dialogue tree contains a lot of data that isn't technically needed
//    for just reading through the tree. Includes hashes, node positions. This could be optimized
//    by exporting a minimal struct type of tree that doesn't use any of that stuff
// 3. MEMORY: right now the event enums is super space inefficient. This means the undo/redo
//    history deque is mostly wasted space (around 75% of the buffer). This may be improved by
//    first, minimizing the enum size for different even types where possible, and more
//    intensely by serializing the diff of the entire EditorState and pushing it to a packed buffer
//    of u8's, but that introduces some validity considerations and serialization/deserialization
//    overhead.

pub static TREE_EXT: &str = ".tree";
pub static BACKUP_EXT: &str = ".bkp";
pub static TOKEN_SEP: &str = "::";

pub const KEY_MAX_LEN: usize = 8;
pub const NAME_MAX_LEN: usize = 32;

/// Stack allocated string with max length suitable for keys
pub type KeyString = StackStr<KEY_MAX_LEN>;

/// Stack allocated string with max length suitable for names
pub type NameString = StackStr<NAME_MAX_LEN>;

/// Defined Tree variant used for all dialogue/choice storage
//NOTE: currently unused because the treemodule does not use generics. This is due to a lack of
//      generic support in nanoserde. Once nanoserde supports generics this will become
//      Tree<Dialogue, Choice>
pub type DialogueTree = tree::Tree;

/// Struct representing a section of text in a rope. This section contains a start and end index,
/// stored in an array. The first element should always be smaller than the second. Additionally
/// the hash of that text section is stored in order to validate that the section is valid
//TODO: Is hash necessary for actually running the dialogue tree?
#[derive(Debug, SerJson, DeJson, Clone, Copy)]
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

impl Section {
    pub fn new(text: [usize; 2], hash: u64) -> Self {
        Self { text, hash }
    }
}

/// Typedef representing the storage of names in dialogue trees. These may be
/// substituted into the text before displaying, or updated by choices in the tree.
pub type NameTable = HashMap<String, String>;

/// Information about an insertion to the NameTable such that the event can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// NameTable
#[derive(Debug)]
pub struct NameTableInsert {
    pub key: KeyString,
    pub name: NameString,
}

/// Information about a removal from the NameTable such that the event can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// NameTable
#[derive(Debug)]
pub struct NameTableRemove {
    pub key: KeyString,
    pub name: NameString,
}

/// Information about an edit to the NameTable such that the event can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// NameTable
#[derive(Debug)]
pub struct NameTableEdit {
    pub key: KeyString,
    pub from: NameString,
    pub to: NameString,
}

/// Typedef representing the storage of values in dialogue trees. These are used as
/// requirements or effects from player choices.
pub type ValTable = HashMap<String, u32>;

/// Information about an insertion (an addition or removal) to the ValTable such that the event
/// can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// ValTable
#[derive(Debug)]
pub struct ValTableInsert {
    pub key: KeyString,
    pub value: u32,
}

/// Information about a removal from the ValTable such that the event can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// ValTable
#[derive(Debug)]
pub struct ValTableRemove {
    pub key: KeyString,
    pub val: u32,
}

/// Information about an edit to the ValTable such that the event can be reconstructed later
///
/// This structure should be returned by methods that perform an equivalent transformation to a
/// ValTable
#[derive(Debug)]
pub struct ValTableEdit {
    pub key: KeyString,
    pub from: u32,
    pub to: u32,
}

/// Arbor Result Type
///
/// Generic result type that stores Arbor [Error]s
pub type Result<T> = std::result::Result<T, Error>;

/// Arbor Error types
///
/// Uses thiserror to generate messages for common situations. This does not
/// attempt to implement From trait on any lower level error types, but relies
/// on anyhow for unification and printing a stack trace
#[derive(Debug)]
pub enum Error {
    Generic,
    NodeParse,
    EdgeParse,
    NameExists,
    NameNotExists,
    NameInUse,
    ValExists,
    ValNotExists,
    ValInUse,
    InvalidSection,
    InvalidHash,
    EventHistoryEmpty,
    EventFuturesEmpty,
    UndoFailed,
    RedoFailed,
    ParseInt(ParseIntError),
    Tree(tree::Error),
    Io(std::io::Error),
    StackStr(stack_str::Error),
    NanoSerde(nanoserde::DeJsonErr),
    Path(std::path::StripPrefixError),
    Infallible(std::convert::Infallible),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Generic => write!(f, "An unspecified error occured..."),
            Error::NodeParse => write!(f, "Node parsing failed"),
            Error::EdgeParse => write!(f, "Edge parsing failed"),
            Error::NameExists => write!(f, "The name already exists"),
            Error::NameNotExists => write!(f, "The name does not exist"),
            Error::NameInUse => write!(f, "The name is in use"),
            Error::ValExists => write!(f, "The value already exists"),
            Error::ValNotExists => write!(f, "The value does not exist"),
            Error::ValInUse => write!(f, "The value is in use"),
            Error::InvalidSection => {
                write!(f, "Attempted to access an invalid section of the text")
            }
            Error::InvalidHash => write!(f, "Hash does not match text section"),
            Error::EventHistoryEmpty => write!(f, "The event history is empty, undo not possible"),
            Error::EventFuturesEmpty => {
                write!(f, "The event future queue is empty, redo not possible")
            }
            Error::UndoFailed => write!(f, "The undo operation failed"),
            Error::RedoFailed => write!(f, "The redo operation failed"),
            Error::ParseInt(e) => e.fmt(f),
            Error::Tree(e) => e.fmt(f),
            Error::Io(e) => e.fmt(f),
            Error::StackStr(e) => e.fmt(f),
            Error::NanoSerde(e) => e.fmt(f),
            Error::Path(e) => e.fmt(f),
            Error::Infallible(e) => e.fmt(f),
        }
    }
}

impl From<ParseIntError> for Error {
    fn from(err: ParseIntError) -> Error {
        Error::ParseInt(err)
    }
}

impl From<tree::Error> for Error {
    fn from(err: tree::Error) -> Error {
        Error::Tree(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Error {
        Error::Io(err)
    }
}

impl From<stack_str::Error> for Error {
    fn from(err: stack_str::Error) -> Error {
        Error::StackStr(err)
    }
}

impl From<nanoserde::DeJsonErr> for Error {
    fn from(err: nanoserde::DeJsonErr) -> Error {
        Error::NanoSerde(err)
    }
}

impl From<std::path::StripPrefixError> for Error {
    fn from(err: std::path::StripPrefixError) -> Error {
        Error::Path(err)
    }
}

impl From<std::convert::Infallible> for Error {
    fn from(err: std::convert::Infallible) -> Error {
        Error::Infallible(err)
    }
}

/// Top level data structure for Arbor
///
/// This struct contains the tree representing the dialogue nodes and player actions connecting
/// them, the buffer which stores all text in a tightly packed manner, and hashtables for storing
/// variables such as player names, conditionals, etc.
#[derive(SerJson, DeJson, Clone, Debug)]
pub struct Arbor {
    pub uid: usize,
    pub tree: DialogueTree,
    pub text: String,
    pub name_table: NameTable,
    pub val_table: ValTable,
    pub name: String,
}

impl Arbor {
    /// Create a new arbor struct with a provided project title name
    pub fn new(name: &str) -> Self {
        Arbor {
            uid: util::gen_uid() as usize,
            tree: Tree::with_capacity(512, 2048),
            text: String::with_capacity(8192),
            name_table: HashMap::default(),
            val_table: HashMap::default(),
            name: String::from(name) + TREE_EXT,
        }
    }
}

/// Struct storing a record of DialogueTreeEvent. Allows for simple linear undo/redo history
#[derive(Debug)]
pub struct History {
    /// Record of events
    pub record: Vec<ArborEvent>,
    /// Current position in the record
    pub position: usize,
}

impl Default for History {
    fn default() -> Self {
        Self {
            record: Vec::with_capacity(1000),
            position: 0,
        }
    }
}

impl History {
    /// Push a new event onto the history. This will remove record of all 'undone' changes.
    pub fn push(&mut self, event: ArborEvent) {
        // drain any undone events before pushing
        self.record.drain(self.position..);
        self.record.push(event);
        self.position += 1;
    }

    /// clear the history, this permanently deletes all events
    pub fn clear(&mut self) {
        self.record.clear();
        self.position = 0;
    }

    /// Undo the most recent event in the history.
    ///
    /// # Errors
    /// Fails and returns an error if the current position is 0, indicating there are no events to
    /// undo
    pub fn undo(&mut self, tree: &mut Arbor) -> Result<()> {
        // Cannot undo if position is 0, return an error
        ensure(self.position > 0, Error::UndoFailed)?;

        self.position -= 1;
        self.record[self.position].undo(tree)
    }

    /// Redo the most recently undone event in the history.
    ///
    /// # Errors
    /// Fails and returns an error if there are no undone events to redo
    pub fn redo(&mut self, tree: &mut Arbor) -> Result<()> {
        // Cannot undo if position is 0, return an error
        ensure(self.position < self.record.len(), Error::RedoFailed)?;

        let res = self.record[self.position].redo(tree);
        self.position += 1;
        res
    }
}

/// Trait representing an event. Types implementing event should store enough data to completely
/// undo or redo all state changes performed by the event
pub trait Event {
    fn undo(&self, target: &mut Arbor) -> Result<()>;
    fn redo(&self, target: &mut Arbor) -> Result<()>;
}

/// Enum of different types of events that modify a DialogueTree. These variants store the
/// information required to reconstruct the event, and implements the Event trait along with
/// enum_dispatch to support undoing/redoing the event, and allow a unified call to undo() or
/// redo() to propogate to the inner event type.
///
/// The Enum is flattened such that all events are granular changes to an underlying datastructure,
/// and there are no nested enum types of events. This is done to avoid extra padding/discriminant
/// words increasing the size of DialogueTreeEvent
#[derive(Debug)]
pub enum ArborEvent {
    NodeInsert(tree::event::NodeInsert),
    NodeRemove(tree::event::NodeRemove),
    NodeEdit(tree::event::NodeEdit),
    EdgeInsert(tree::event::EdgeInsert),
    EdgeRemove(tree::event::EdgeRemove),
    EdgeEdit(tree::event::EdgeEdit),
    LinkMove(tree::event::LinkMove),
    NameTableInsert(NameTableInsert),
    NameTableRemove(NameTableRemove),
    NameTableEdit(NameTableEdit),
    ValTableInsert(ValTableInsert),
    ValTableRemove(ValTableRemove),
    ValTableEdit(ValTableEdit),
}

impl Event for ArborEvent {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        match self {
            ArborEvent::NodeInsert(ev) => ev.undo(target),
            ArborEvent::NodeRemove(ev) => ev.undo(target),
            ArborEvent::NodeEdit(ev) => ev.undo(target),
            ArborEvent::EdgeInsert(ev) => ev.undo(target),
            ArborEvent::EdgeRemove(ev) => ev.undo(target),
            ArborEvent::EdgeEdit(ev) => ev.undo(target),
            ArborEvent::LinkMove(ev) => ev.undo(target),
            ArborEvent::NameTableInsert(ev) => ev.undo(target),
            ArborEvent::NameTableRemove(ev) => ev.undo(target),
            ArborEvent::NameTableEdit(ev) => ev.undo(target),
            ArborEvent::ValTableInsert(ev) => ev.undo(target),
            ArborEvent::ValTableRemove(ev) => ev.undo(target),
            ArborEvent::ValTableEdit(ev) => ev.undo(target),
        }
    }
    fn redo(&self, target: &mut Arbor) -> Result<()> {
        match self {
            ArborEvent::NodeInsert(ev) => ev.redo(target),
            ArborEvent::NodeRemove(ev) => ev.redo(target),
            ArborEvent::NodeEdit(ev) => ev.redo(target),
            ArborEvent::EdgeInsert(ev) => ev.redo(target),
            ArborEvent::EdgeRemove(ev) => ev.redo(target),
            ArborEvent::EdgeEdit(ev) => ev.redo(target),
            ArborEvent::LinkMove(ev) => ev.redo(target),
            ArborEvent::NameTableInsert(ev) => ev.redo(target),
            ArborEvent::NameTableRemove(ev) => ev.redo(target),
            ArborEvent::NameTableEdit(ev) => ev.redo(target),
            ArborEvent::ValTableInsert(ev) => ev.redo(target),
            ArborEvent::ValTableRemove(ev) => ev.redo(target),
            ArborEvent::ValTableEdit(ev) => ev.redo(target),
        }
    }
}

/// Event implementations for all ArborEvent enum types

impl Event for NodeInsert {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.remove_node(self.index)?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.insert_node(self.node, self.index)?;
        Ok(())
    }
}

impl Event for NodeRemove {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.remove_node(self.index)?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.insert_node(self.node, self.index)?;
        Ok(())
    }
}

impl Event for NodeEdit {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.edit_node(self.index, self.from)?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.edit_node(self.index, self.to)?;
        Ok(())
    }
}

impl Event for EdgeInsert {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.remove_edge(self.index)?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.insert_edge(
            self.source,
            self.target,
            self.edge.clone(),
            self.index,
            self.placement,
        )?;
        Ok(())
    }
}

impl Event for EdgeRemove {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.insert_edge(
            self.source,
            self.target,
            self.edge.clone(),
            self.index,
            self.placement,
        )?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.remove_edge(self.index)?;
        Ok(())
    }
}

impl Event for EdgeEdit {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.edit_edge(self.index, self.from.clone())?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target.tree.edit_edge(self.index, self.to.clone())?;
        Ok(())
    }
}

impl Event for LinkMove {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target
            .tree
            .edit_link_order(self.source, self.index, self.from)?;
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        let _new_event = target
            .tree
            .edit_link_order(self.source, self.index, self.to)?;
        Ok(())
    }
}

impl Event for NameTableInsert {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target.name_table.remove(&self.key.to_string());
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target
            .name_table
            .insert(self.key.to_string(), self.name.to_string());
        Ok(())
    }
}

impl Event for NameTableRemove {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target
            .name_table
            .insert(self.key.to_string(), self.name.to_string());
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target.name_table.remove(&self.key.to_string());
        Ok(())
    }
}

impl Event for NameTableEdit {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target
            .name_table
            .insert(self.key.to_string(), self.from.to_string());
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target
            .name_table
            .insert(self.key.to_string(), self.to.to_string());
        Ok(())
    }
}

impl Event for ValTableInsert {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.remove(&self.key.to_string());
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.insert(self.key.to_string(), self.value);
        Ok(())
    }
}

impl Event for ValTableRemove {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.insert(self.key.to_string(), self.val);
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.remove(&self.key.to_string());
        Ok(())
    }
}

impl Event for ValTableEdit {
    fn undo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.insert(self.key.to_string(), self.from);
        Ok(())
    }

    fn redo(&self, target: &mut Arbor) -> Result<()> {
        target.val_table.insert(self.key.to_string(), self.to);
        Ok(())
    }
}

/// State information for an editor instance. Includes two copies of the dialogue tree (one active
/// and one backup) as well as other state information
pub struct EditorState {
    pub active: Arbor,
    pub backup: Arbor,
    pub scratchpad: String,
    pub history: History,
}

impl EditorState {
    /// Create a new Editor state.
    ///
    /// Editor state needs to take ownership of the data. However since
    /// a backup copy needs to be created on construction, the data is moved, and then cloned
    pub fn new(data: Arbor) -> Self {
        EditorState {
            active: data.clone(),
            backup: data,
            scratchpad: String::with_capacity(1000),
            history: Default::default(),
        }
    }

    /// Swap the active and backup trees without copying any of the underlying data
    pub fn swap(&mut self) {
        std::mem::swap(&mut self.active, &mut self.backup);
    }
}

/// Struct storing the information for a player choice. Stored in the edges of a dialogue tree
#[derive(Debug, SerJson, DeJson, Clone)]
pub struct Choice {
    pub section: Section,
    pub requirement: ReqKind,
    pub effect: EffectKind,
}

impl Choice {
    pub fn new(section: Section, requirement: ReqKind, effect: EffectKind) -> Self {
        Self {
            section,
            requirement,
            effect,
        }
    }
}

/// Struct for storing the information for a line of dialogue. Stored in the nodes of a dialogue
/// tree
#[derive(Debug, SerJson, DeJson, Clone, Copy)]
pub struct Dialogue {
    pub section: Section,
}

impl Dialogue {
    pub fn new(section: Section) -> Self {
        Self { section }
    }
}

#[derive(Debug, SerJson, DeJson, PartialEq, Clone)]
pub enum ReqKind {
    /// No requirement
    No,
    /// Must be greater than num
    Greater(String, u32),
    /// Must be less than num
    Less(String, u32),
    /// Must be equal to num
    Equal(String, u32),
    /// Must match name string
    Cmp(String, String),
}

impl std::str::FromStr for ReqKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
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
        // TODO: create verbose error types for form_str failures
        let mut split = s.rsplit(&['(', ',', ')'][..]);
        debug!("{}", s);

        trace!("Check that first item is ''");
        ensure(
            split.next().ok_or(Error::Generic)?.is_empty(),
            Error::Generic,
        )?;

        trace!(
            "second item should be number or string, check for valid length, wait to check if int"
        );
        let val = split.next().ok_or(Error::Generic)?.to_string();

        trace!("third item should be key, check that the key is a valid length");
        // match required due to lifetime limitations on CapacityError
        let key = split.next().ok_or(Error::Generic)?.to_string();

        trace!("fourth item should be Enum type, build it!, and also try to resolve the val");
        match split.next().ok_or(Error::Generic)? {
            "Greater" => Ok(ReqKind::Greater(key, val.parse::<u32>()?)),
            "Less" => Ok(ReqKind::Less(key, val.parse::<u32>()?)),
            "Equal" => Ok(ReqKind::Equal(key, val.parse::<u32>()?)),
            "Cmp" => Ok(ReqKind::Cmp(key, val)),
            _ => Err(Error::Generic),
        }
    }
}

/// Represents an effect that occurs when a choice is made.
///
/// Name length strings are stored as a heap allocated String rather than a static NameString as
/// that would bloat enum size by 32 bytes, when Cmp will rarely be used compared to val based
/// requirements
#[derive(Debug, SerJson, DeJson, PartialEq, Clone)]
pub enum EffectKind {
    /// No effect
    No,
    Add(String, u32),
    Sub(String, u32),
    Set(String, u32),
    Assign(String, String),
}

impl std::str::FromStr for EffectKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
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
        // TODO: create custom error types for from_str failures
        let mut split = s.rsplit(&['(', ',', ')'][..]);
        debug!("{}", s);

        trace!("First item should be ''");
        ensure(
            split.next().ok_or(Error::Generic)?.is_empty(),
            Error::Generic,
        )?;

        trace!("Second item should be number or string, don't check for validity yet");
        let val = split.next().ok_or(Error::Generic)?;

        trace!("Third item should be key, check that the key and name are of a valid length");
        // match required due to lifetime limitations on CapacityError
        let key = split.next().ok_or(Error::Generic)?.to_string();

        trace!("fourth item should be Enum type, build it!, and also try to resolve the val");
        match split.next().ok_or(Error::Generic)? {
            "Add" => Ok(EffectKind::Add(key, val.parse::<u32>()?)),
            "Sub" => Ok(EffectKind::Sub(key, val.parse::<u32>()?)),
            "Set" => Ok(EffectKind::Set(key, val.parse::<u32>()?)),
            "Assign" => Ok(EffectKind::Assign(key, val.to_string())),
            _ => Err(Error::Generic),
        }
    }
}

/// Utility methods used internally for various useful tasks. These cannot be called directly
/// from the command line, but are useful for working with dialogue_trees in other programs
pub mod util {
    use super::{
        Arbor, Dialogue, DialogueTree, EffectKind, Error, NameTable, ReqKind, Result, Section,
        ValTable, TOKEN_SEP,
    };
    use crate::{ensure::ensure, tree::Dfs};
    use seahash::{hash, SeaHasher};
    use std::hash::{Hash, Hasher};
    use std::io::{self, Write};
    use std::time::Instant;

    /// Generate UID.
    ///
    /// UID is a 64 bit unique identifier. Used to associate other metadata or resources with a
    /// specific instance
    #[inline]
    pub fn gen_uid() -> u64 {
        let mut hasher = SeaHasher::new();
        Instant::now().hash(&mut hasher);
        hasher.finish()
    }

    /// Helper method to split a node into the speaker name, and the rest of the text
    /// Does not attempt to convert name keys to namestrings
    pub fn split_node(text: &str) -> Result<(&str, &str)> {
        // split only twice
        let mut text_iter = text.splitn(2, TOKEN_SEP);
        let _ = text_iter.next(); // skip first token, it is '' for any correct string
        let speaker_key = text_iter.next().ok_or(Error::NodeParse)?;
        let dialogue = text_iter.next().ok_or(Error::NodeParse)?;
        Ok((speaker_key, dialogue))
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
        let speaker_key = text_iter.next().ok_or(Error::Generic)?.1;
        let speaker_name = name_table.get(speaker_key).ok_or(Error::NodeParse)?;
        name_buf.push_str(speaker_name);
        text_iter.try_for_each(|(i, n)| -> std::result::Result<(), Error> {
            if (i & 0x1) == 1 {
                // token is a name (index 1, 3, 5 ...)
                let value = name_table.get(n).ok_or(Error::NodeParse)?;
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

    /// Same routine as parse node, except the results are not actually written.
    /// This is used for validating that the section of text is valid
    pub fn validate_node(text: &str, name_table: &NameTable) -> Result<()> {
        let mut text_iter = text.split(TOKEN_SEP).enumerate();
        text_iter.next(); // discard first empty string
        let speaker_key = text_iter.next().ok_or(Error::EdgeParse)?.1;
        name_table.get(speaker_key).ok_or(Error::EdgeParse)?;
        text_iter.try_for_each(|(i, n)| -> std::result::Result<(), Error> {
            if (i & 0x1) == 1 {
                // token is a name (index 1, 3, 5 ...)
                name_table.get(n).ok_or(Error::EdgeParse)?;
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
        text_iter.try_for_each(|(i, n)| -> std::result::Result<(), Error> {
            if (i & 0x1) == 0 {
                // token cannot be a name
                text_buf.push_str(n);
                Ok(())
            } else {
                let value = name_table.get(n).ok_or(Error::EdgeParse)?;
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
        text_iter.try_for_each(|(i, n)| -> std::result::Result<(), Error> {
            if (i & 0x1) == 0 {
                Ok(())
            } else {
                name_table.get(n).ok_or(Error::Generic)?;
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
        tree: &DialogueTree,
        new_text: &mut String,
        new_tree: &mut DialogueTree,
    ) -> Result<()> {
        new_text.clear();
        new_tree.clear();
        // Clone the old tree into the new one such that the nodes and edge indices and layout
        // are identical. This makes it much easier to rebuild as only the node weights need to
        // be updated to point to the proper sections of the next text buffer
        *new_tree = tree.clone();

        let root_index: usize = 0;
        let mut dfs = Dfs::new(tree, root_index);
        while let Some(node_index) = dfs.next(tree)? {
            // Rebuild node
            let dialogue = tree.get_node(node_index)?;
            let slice: &str = &text[dialogue.section[0]..dialogue.section[1]];
            let start = new_text.len();
            new_text.push_str(slice);
            let end = new_text.len();
            let new_dialogue = new_tree.get_node_mut(node_index)?;
            // verify new and old hash match
            let new_hash = hash(new_text[start..end].as_bytes());
            assert!(dialogue.section.hash == new_hash);
            *new_dialogue = Dialogue::new(Section::new([start, end], new_hash));

            // Rebuild all edges sourced from this node
            let edge_iter = tree.outgoing_from_index(node_index)?;
            for edge_index in edge_iter {
                let edge = tree.get_edge(edge_index)?;
                let slice: &str = &text[edge.section[0]..edge.section[1]];

                // Verify that edge and new_edge match, they should be identical since we
                // started by cloning the tree to new_tree
                assert!(tree.target_of(edge_index)? == new_tree.target_of(edge_index)?);

                let start = new_text.len();
                new_text.push_str(slice);
                let end = new_text.len();
                // verify new and old hash match
                let new_hash = hash(new_text[start..end].as_bytes());
                assert!(edge.section.hash == new_hash);
                let new_choice = new_tree.get_edge_mut(edge_index)?;
                new_choice.section = Section::new([start, end], new_hash);
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
            ReqKind::No => {}
            ReqKind::Greater(key, _val) => {
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            ReqKind::Less(key, _val) => {
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            ReqKind::Equal(key, _val) => {
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            ReqKind::Cmp(key, _val) => {
                name_table.get(key).ok_or(Error::NameNotExists)?;
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
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            EffectKind::Sub(key, _val) => {
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            EffectKind::Set(key, _val) => {
                val_table.get(key).ok_or(Error::ValNotExists)?;
            }
            EffectKind::Assign(key, _val) => {
                name_table.get(key).ok_or(Error::NameNotExists)?;
            }
        }
        Ok(())
    }

    /// Validate that a given dialogue tree data structure contains all valid sections of text
    /// that all edges point to valid nodes in the tree, all have valid action enums, and have
    /// have correct hashes for all nodes and edges
    ///
    /// Returns a result with the error type if the tree was invalid, returns Ok(()) if valid
    pub fn validate_tree(data: &Arbor) -> Result<()> {
        // check nodes first, use parallel iterator in case of very large graph
        let mut nodes_iter = data.tree.nodes().iter();
        nodes_iter.try_for_each(|node| -> Result<()> {
            // try to grab the text section as a slice, and return an error if the get() failed
            let slice = data.text[..]
                .get(node.section[0]..node.section[1])
                .ok_or(Error::InvalidSection)?;
            // if the slice was successful, check its hash
            ensure(
                hash(slice.as_bytes()) == node.section.hash,
                Error::InvalidHash,
            )?;
            // Check that the section of text parses successfully (all names present in the
            // name_table)
            validate_node(slice, &data.name_table)?;
            Ok(())
        })?;

        // check edges, will check that they point to nodes that exist, and validate the actionenums
        let mut edges_iter = data.tree.edges().iter();
        edges_iter.try_for_each(|edge| -> Result<()> {
            // try to grab the text section as a slice, and return an error if the get() failed
            let slice = data.text[..]
                .get(edge.section[0]..edge.section[1])
                .ok_or(Error::InvalidSection)?;
            // if the slice was successful, check its hash
            ensure(
                hash(slice.as_bytes()) == edge.section.hash,
                Error::InvalidHash,
            )?;
            // Check that the section of text parses successfully (all names present in the
            // name_table)
            validate_edge(slice, &data.name_table)?;
            validate_requirement(&edge.requirement, &data.name_table, &data.val_table)?;
            validate_effect(&edge.effect, &data.name_table, &data.val_table)?;
            Ok(())
        })?;
        Ok(())
    }
}
