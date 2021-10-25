use crate::{
    tree::{self, EdgeIndex, NodeIndex},
    util, Arbor, ArborEvent, Choice, Dialogue, EffectKind, Error, History, KeyString, NameString,
    NameTableEdit, NameTableInsert, NameTableRemove, ReqKind, Result, Section, ValTableEdit,
    ValTableInsert, ValTableRemove, TOKEN_SEP,
};
use log::{debug, info, trace, warn};
use nanoserde::SerJson;
use seahash::hash;

use std::fs;
use std::io::{self, Read};
use std::path;
use std::str::FromStr;

/// Top level editor struct. Implements public interface and state for modifying arbors in a safe
/// manner
#[derive(Debug)]
pub struct Editor {
    pub arbor: Arbor,
    pub backup: Arbor,
    pub history: History,
    pub path_buf: path::PathBuf,
    pub serial_buf: String,
}

impl Editor {
    /// Create a new project with a provided title. Optionally provide save path. If none provided,
    /// the current working directory is used
    ///
    /// A project is made up of a text rope storing all dialogue text, a hashtable storing
    /// variable or user defined values, and a graph representing the narrative. Nodes of the
    /// graph represent dialogues from characters in the story, and nodes represent the
    /// actions of the player.
    ///
    /// # Error
    ///
    /// If project creation on disk fails
    pub fn new(name: &str, save_dir: Option<&path::Path>) -> Result<Editor> {
        let arbor = Arbor::new(name);
        let backup = arbor.clone();
        let history = History::default();

        // use arbor.name as it has the qualified .tree extension already added
        let path_buf: path::PathBuf = match save_dir {
            Some(dir) => [dir, &path::Path::new(&arbor.name)].iter().collect(),
            None => path::PathBuf::from_str(&arbor.name)?,
        };
        // create filename in file system
        let serial_buf: String = String::new();
        let mut editor = Editor {
            arbor,
            backup,
            history,
            path_buf,
            serial_buf,
        };
        editor.save(None)?;
        Ok(editor)
    }

    /// Save the Arbor struct to a `*.tree` file on the filesystem.
    ///
    /// Optionally provide a path to save to, otherwise uses last saved path. If successful,
    /// sync the [Arbor] data with provided backup struct.
    ///
    /// # Error
    ///
    /// If file fails to save
    pub fn save(&mut self, save_dir: Option<&path::Path>) -> Result<()> {
        info!("Save project");
        self.serial_buf = self.arbor.serialize_json();

        // update path buf if user provided a new path
        if let Some(dir) = save_dir {
            self.path_buf = [dir, path::Path::new(&self.arbor.name)].iter().collect();
        }

        std::fs::write(&self.path_buf, &self.serial_buf)?;

        trace!("save successful, syncing backup with active copy");
        self.backup = self.arbor.clone();

        Ok(())
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
    /// Since the rebuild tree cleans out any artifacts from edits/removals, the undo/redo history is
    /// reset
    ///
    pub fn rebuild(&mut self) -> Result<()> {
        // save and sync beforehand
        self.save(None)?;

        // attempt rebuild tree on active buffer, backup buffer is used as source
        util::rebuild_tree(
            &self.backup.text,
            &self.backup.tree,
            &mut self.arbor.text,
            &mut self.arbor.tree,
        )?;

        // Confirm that that rebuilt tree is valid
        util::validate_tree(&self.arbor)?;

        // Clear the undo/redo history
        self.history.clear();

        Ok(())
    }

    /// Load a `*.tree` file from the filesystem into a new [Arbor] struct
    ///
    /// # Error
    /// If loading or parsing fail
    pub fn load(path: &path::Path) -> Result<Editor> {
        let mut reader = io::BufReader::new(fs::File::open(path)?);
        let mut serial_buf = String::new();
        reader.read_to_string(&mut serial_buf)?;
        info!("obtained serial buffer : {:?}", serial_buf);
        let arbor = nanoserde::DeJson::deserialize_json(&serial_buf)?;
        info!("loaded arbor {:?} with content: {:?}", path, arbor);

        // check that the loaded tree is valid before loading into main state
        util::validate_tree(&arbor)?;

        let backup = arbor.clone();
        let history = History::default();

        // use arbor.name as it has the qualified .tree extension already added
        let path_buf = path.to_path_buf();

        Ok(Editor {
            arbor,
            backup,
            history,
            path_buf,
            serial_buf,
        })
    }

    /// Undo the last editor event.
    ///
    /// # Error
    ///
    /// if the undo fails.
    pub fn undo(&mut self) -> Result<()> {
        self.history.undo(&mut self.arbor)
    }

    /// Redo the last editor event.
    ///
    /// # Error
    ///
    /// if the redo fails.
    pub fn redo(&mut self) -> Result<()> {
        self.history.redo(&mut self.arbor)
    }

    /// Create a new piece of dialogue in the dialogue tree.
    ///
    /// Dialogue is encoded as a node in the tree. If the operation was successful, returns the index
    /// of the newly added dialogue node
    pub fn new_node(&mut self, speaker: &str, dialogue: &str) -> Result<tree::NodeIndex> {
        info!("Creating new node");

        trace!("verify the speaker name is valid");
        self.arbor
            .name_table
            .get(speaker)
            .ok_or(Error::NameNotExists)?;

        trace!("push dialogue to text buffer");
        let start = self.arbor.text.len();
        self.arbor.text.push_str(&format!(
            "{}{}{}{}",
            TOKEN_SEP, speaker, TOKEN_SEP, dialogue
        ));
        let end = self.arbor.text.len();
        debug!("start: {}, end: {}", start, end);

        trace!("compute hash from text section");
        let hash = hash(self.arbor.text[start..end].as_bytes());
        debug!("hash {}", hash);

        let dialogue = Dialogue::new(Section::new([start, end], hash));

        trace!("add new node to tree");
        let event = self.arbor.tree.add_node(dialogue)?;
        let idx = event.index;
        self.history.push(ArborEvent::NodeInsert(event));
        Ok(idx)
    }

    /// Create a new user choice in the dialogue tree
    ///
    /// Choices are encoded as edges from one dialogue node to another in the tree
    pub fn new_edge(
        &mut self,
        text: &str,
        source: NodeIndex,
        target: NodeIndex,
        requirement: ReqKind,
        effect: EffectKind,
    ) -> Result<tree::EdgeIndex> {
        info!("Creating new edge");

        trace!("push choice text buffer");
        let start = self.arbor.text.len();
        self.arbor.text.push_str(text);
        let end = self.arbor.text.len();
        debug!("start: {}, end: {}", start, end);

        trace!("Compute hash from text section");
        let hash = hash(&self.arbor.text[start..end].as_bytes());
        debug!("hash {}", hash);

        trace!("Validate that any requirements/effects reference valid hashmap keys");
        util::validate_requirement(&requirement, &self.arbor.name_table, &self.arbor.val_table)?;
        util::validate_effect(&effect, &self.arbor.name_table, &self.arbor.val_table)?;

        let choice = Choice {
            section: Section::new([start, end], hash),
            requirement,
            effect,
        };

        trace!("Adding new edge to tree");
        let event = self.arbor.tree.add_edge(source, target, choice)?;
        let idx = event.index;

        self.history.push(ArborEvent::EdgeInsert(event));
        Ok(idx)
    }

    /// Create a new name for use in dialogue nodes and actions
    ///
    /// A name represents some variable that may be substituted into the text. Examples
    /// include player names, pronouns, and character traits
    pub fn new_name(&mut self, key: &str, name: &str) -> Result<()> {
        info!("Create new name");

        trace!("check that key does not already exist");
        if self.arbor.name_table.get(key).is_none() {
            trace!("add key and name to table");
            self.arbor
                .name_table
                .insert(key.to_owned(), name.to_owned());

            let event = NameTableInsert {
                key: KeyString::from_str(key)?,
                name: NameString::from_str(name)?,
            };

            self.history.push(ArborEvent::NameTableInsert(event));
            Ok(())
        } else {
            Err(Error::NameExists)
        }
    }

    /// Create a new value for use in dialogue nodes and actions
    ///
    /// A value represents some variable number that is used as requirements and effects for
    /// choices. Examples include player skill levels, relationship stats, and presence of an item.
    pub fn new_val(&mut self, key: &str, val: u32) -> Result<()> {
        info!("Create new name");

        trace!("check that key does not already exist");
        if self.arbor.val_table.get(key).is_none() {
            trace!("add key and name to table");
            self.arbor.val_table.insert(key.to_owned(), val);

            let event = ValTableInsert {
                key: KeyString::from_str(key)?,
                value: val,
            };

            self.history.push(ArborEvent::ValTableInsert(event));

            Ok(())
        } else {
            Err(Error::NameExists)
        }
    }

    /// Edit the contents of a node in the provided arbor struct. Returns the index of the edited node
    ///
    /// Can optionally edit the speaker name, and the dialogue content
    ///
    /// # Error
    /// Returns an error if the `node_index` is invalid, or if the `new_speaker` is not present in the
    /// [Arbor]'s [NameTable]
    pub fn edit_node(
        &mut self,
        node_index: NodeIndex,
        new_speaker: Option<&str>,
        new_dialogue: Option<&str>,
    ) -> Result<NodeIndex> {
        info!("Edit node {}", node_index);

        // fast path to return if noting to be done
        if new_speaker.is_none() && new_dialogue.is_none() {
            warn!("edit called with no changes requested");
            return Ok(node_index);
        }

        trace!("get old node text, speaker, and name");
        let old_node = self.arbor.tree.get_node(node_index)?;
        let old_text = self.arbor.text[old_node.section[0]..old_node.section[1]].to_owned();
        let (old_speaker, old_dialogue) = util::split_node(old_text.as_str())?;

        // re-use the old content for the speaker or dialogue if None is provided for the new_*
        let speaker = new_speaker.unwrap_or(old_speaker);
        let dialogue = new_dialogue.unwrap_or(old_dialogue);

        trace!("push new dialogue to text buffer");
        let start = self.arbor.text.len();

        self.arbor.text.push_str(&format!(
            "{}{}{}{}",
            TOKEN_SEP, speaker, TOKEN_SEP, dialogue
        ));
        let end = self.arbor.text.len();

        trace!("recalculate hash");
        let hash = hash(self.arbor.text[start..end].as_bytes());
        debug!("hash {}", hash);

        let new_node = Dialogue::new(Section::new([start, end], hash));

        trace!("update node weight in tree");
        let event = self.arbor.tree.edit_node(node_index, new_node)?;
        self.history.push(ArborEvent::NodeEdit(event));

        Ok(node_index)
    }

    pub fn edit_edge(
        &mut self,
        edge_index: usize,
        new_text: Option<&str>,
        new_requirement: Option<ReqKind>,
        new_effect: Option<EffectKind>,
    ) -> Result<EdgeIndex> {
        info!("Edit edge {}", edge_index);

        // fast path to return if noting to be done
        if new_text.is_none() && new_requirement.is_none() && new_effect.is_none() {
            warn!("edit called with no changes requested");
            return Ok(edge_index);
        }

        // start with old choice, update as needed
        let mut choice = self.arbor.tree.get_edge(edge_index)?.clone();

        if let Some(text) = new_text {
            trace!("push choice to text buffer");
            let start = self.arbor.text.len();
            self.arbor.text.push_str(text);
            let end = self.arbor.text.len();
            trace!("recalculate hash");
            let hash = hash(self.arbor.text[start..end].as_bytes());
            debug!("hash {}", hash);
            choice.section = Section::new([start, end], hash);
        }

        trace!("validate that any requirements/effects reference valid hashmap keys");
        if let Some(req) = new_requirement {
            util::validate_requirement(&req, &self.arbor.name_table, &self.arbor.val_table)?;
            choice.requirement = req;
        }
        if let Some(eff) = new_effect {
            util::validate_effect(&eff, &self.arbor.name_table, &self.arbor.val_table)?;
            choice.effect = eff;
        }

        trace!("update edge weight in tree");
        let event = self.arbor.tree.edit_edge(edge_index, choice)?;

        self.history.push(ArborEvent::EdgeEdit(event));
        Ok(edge_index)
    }

    pub fn edit_name(&mut self, key: &str, new_name: &str) -> Result<()> {
        info!("Edit name {}", key);

        trace!("check that key exists before editing");
        if self.arbor.name_table.get(key).is_some() {
            let name = self.arbor.name_table.get_mut(key).ok_or(Error::Generic)?;
            let old_name = &name.clone();
            debug!("old name: {}, new name: {}", old_name, new_name);

            trace!("update key-value in name table");
            *name = new_name.to_owned();

            let event = NameTableEdit {
                key: KeyString::from_str(key)?,
                from: NameString::from_str(&old_name)?,
                to: NameString::from_str(&name)?,
            };

            self.history.push(ArborEvent::NameTableEdit(event));

            Ok(())
        } else {
            Err(Error::NameNotExists.into())
        }
    }

    pub fn edit_val(&mut self, key: &str, new_value: u32) -> Result<()> {
        info!("Edit val {}", key);

        trace!("check that key exists before editing");
        if self.arbor.val_table.get(key).is_some() {
            let value = self.arbor.val_table.get_mut(key).ok_or(Error::Generic)?;
            let old_value = &value.clone();
            debug!("old name: {}, new name: {}", old_value, new_value);

            trace!("update key-value in name table");
            *value = new_value.to_owned();

            let event = ValTableEdit {
                key: KeyString::from_str(key)?,
                from: *old_value,
                to: *value,
            };

            self.history.push(ArborEvent::ValTableEdit(event));

            Ok(())
        } else {
            Err(Error::NameNotExists.into())
        }
    }

    /// Remove the contents of a node and return the hash of the removed node's text section
    pub fn remove_node(&mut self, node_index: NodeIndex) -> Result<usize> {
        info!("Remove node {}", node_index);

        let event = self.arbor.tree.remove_node(node_index)?;
        let hash = event.node.section.hash;

        self.history.push(ArborEvent::NodeRemove(event));
        Ok(hash as usize)
    }

    /// Remove an edge from the dialogue tree and return the hash of the removed edge's text
    /// section
    pub fn remove_edge(&mut self, edge_index: EdgeIndex) -> Result<usize> {
        info!("Remove Edge {}", edge_index);

        trace!("remove edge from tree");
        let event = self.arbor.tree.remove_edge(edge_index)?;
        let hash = event.edge.section.hash;

        self.history.push(ArborEvent::EdgeRemove(event));
        Ok(hash as usize)
    }

    pub fn remove_name(&mut self, key: &str) -> Result<()> {
        info!("Remove Name {}", key);

        let name = self
            .arbor
            .name_table
            .get(key)
            .ok_or(Error::NameNotExists)?
            .clone();

        trace!("check if the key is referenced anywhere in the text");
        if let Some(_found) = self
            .arbor
            .text
            .find(format!("{}{}{}", TOKEN_SEP, key, TOKEN_SEP).as_str())
        {
            return Err(Error::NameInUse.into());
        }

        trace!("check if the key is referenced in any requirements or effects");
        for choice in self.arbor.tree.edges() {
            // this match will stop compiling any time a new reqKind is added
            match &choice.requirement {
                ReqKind::No => Ok(()),
                ReqKind::Greater(_, _) => Ok(()),
                ReqKind::Less(_, _) => Ok(()),
                ReqKind::Equal(_, _) => Ok(()),
                ReqKind::Cmp(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
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
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
            }?;
        }

        trace!("remove key-value pair from name table");
        self.arbor
            .name_table
            .remove(key)
            .ok_or(Error::NameNotExists)?;

        let event = NameTableRemove {
            key: KeyString::from_str(key)?,
            name: NameString::from_str(&name)?,
        };

        self.history.push(ArborEvent::NameTableRemove(event));
        Ok(())
    }

    pub fn remove_val(&mut self, key: &str) -> Result<()> {
        info!("remove value {}", key);

        let value = self
            .arbor
            .val_table
            .get(key)
            .ok_or(Error::ValNotExists)?
            .clone();

        trace!("check if the key is referenced in any requirements or effects");
        for choice in self.arbor.tree.edges() {
            // this match will stop compiling any time a new reqKind is added
            match &choice.requirement {
                ReqKind::No => Ok(()),
                ReqKind::Greater(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                ReqKind::Less(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                ReqKind::Equal(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                ReqKind::Cmp(_, _) => Ok(()),
            }?;
            match &choice.effect {
                EffectKind::No => Ok(()),
                EffectKind::Add(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                EffectKind::Sub(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                EffectKind::Set(key, _) => {
                    if key.eq(key) {
                        Err(Error::NameInUse)
                    } else {
                        Ok(())
                    }
                }
                EffectKind::Assign(_, _) => Ok(()),
            }?;
        }

        trace!("remove key-value pair from value table");
        self.arbor
            .val_table
            .remove(key)
            .ok_or(Error::NameNotExists)?;

        let event = ValTableRemove {
            key: KeyString::from_str(key)?,
            val: value,
        };

        self.history.push(ArborEvent::ValTableRemove(event));

        Ok(())
    }
}

#[cfg(test)]
/// API level tests for Arbor Editor
mod test {
    use crate::{editor, util, Arbor, EffectKind, ReqKind};
    use simple_logger;
    use std::path::Path;

    #[test]
    /// Test basic use case of the editor, new project, add a few nodes and names, list the output,
    /// save the project, reload, list the output again
    fn simple() {
        // scratchpads for names and text
        let mut name_buf = String::with_capacity(32);
        let mut text_buf = String::with_capacity(1024);

        // create new project in cwd
        let mut e = editor::Editor::new("simple_test", None).unwrap();
        assert_eq!(e.arbor.name, "simple_test.tree");

        // add some new characters
        e.new_name("cat", "Behemoth").unwrap();
        assert_eq!(e.arbor.name_table.get("cat").unwrap(), "Behemoth");

        // add some vals for stats
        e.new_val("rus_lit", 50).unwrap();
        assert_eq!(*e.arbor.val_table.get("rus_lit").unwrap(), 50);

        // add some dialogue nodes and choice edges
        let first_index = e.new_node("cat", "Well, who knows, who knows").unwrap();
        let second_index = e
            .new_node(
                "cat",
                "'I protest!' ::cat:: exclaimed hotly. 'Dostoevsky is immortal'",
            )
            .unwrap();
        e.new_edge(
            "Dostoevsky's dead",
            first_index,
            second_index,
            ReqKind::Less("rus_lit".to_owned(), 51),
            EffectKind::Sub("rus_lit".to_owned(), 1),
        )
        .unwrap();

        // assert that content matches expected
        //
        // `Reader` module provides easier way to access node text, but this is manual way
        let first_node = e.arbor.tree.get_node(first_index).unwrap();
        let text = &e.arbor.text[first_node.section[0]..first_node.section[1]];
        util::parse_node(text, &e.arbor.name_table, &mut name_buf, &mut text_buf).unwrap();
        assert_eq!(*&name_buf, "Behemoth");
        assert_eq!(*&text_buf, "Well, who knows, who knows");

        let edge_index = e
            .arbor
            .tree
            .outgoing_from_index(first_index)
            .unwrap()
            .next()
            .unwrap();
        let edge = e.arbor.tree.get_edge(edge_index).unwrap();
        let text = &e.arbor.text[edge.section[0]..edge.section[1]];
        util::parse_edge(text, &e.arbor.name_table, &mut text_buf).unwrap();
        assert_eq!(*&text_buf, "Dostoevsky's dead");

        let second_node = e.arbor.tree.get_node(second_index).unwrap();
        let text = &e.arbor.text[second_node.section[0]..second_node.section[1]];
        util::parse_node(text, &e.arbor.name_table, &mut name_buf, &mut text_buf).unwrap();
        assert_eq!(*&name_buf, "Behemoth");
        assert_eq!(
            *&text_buf,
            "'I protest!' Behemoth exclaimed hotly. 'Dostoevsky is immortal'"
        );

        // test save/reload
        e.save(None).unwrap();

        // test load and deserialize from filesystem
        let loaded = editor::Editor::load(Path::new("simple_test.tree")).unwrap();
        assert_eq!(format!("{:?}", e.arbor), format!("{:?}", loaded.arbor));

        // test rebuild (simple case for rebuild is to do nothing, shouldn't affect text)
        e.rebuild().unwrap();
        // rerun content check after rebuild
        // `Reader` module provides easier way to access node text, but this is manual way
        let first_node = e.arbor.tree.get_node(first_index).unwrap();
        let text = &e.arbor.text[first_node.section[0]..first_node.section[1]];
        util::parse_node(text, &e.arbor.name_table, &mut name_buf, &mut text_buf).unwrap();
        assert_eq!(*&name_buf, "Behemoth");
        assert_eq!(*&text_buf, "Well, who knows, who knows");

        let edge_index = e
            .arbor
            .tree
            .outgoing_from_index(first_index)
            .unwrap()
            .next()
            .unwrap();
        let edge = e.arbor.tree.get_edge(edge_index).unwrap();
        let text = &e.arbor.text[edge.section[0]..edge.section[1]];
        util::parse_edge(text, &e.arbor.name_table, &mut text_buf).unwrap();
        assert_eq!(*&text_buf, "Dostoevsky's dead");

        let second_node = e.arbor.tree.get_node(second_index).unwrap();
        let text = &e.arbor.text[second_node.section[0]..second_node.section[1]];
        util::parse_node(text, &e.arbor.name_table, &mut name_buf, &mut text_buf).unwrap();
        assert_eq!(*&name_buf, "Behemoth");
        assert_eq!(
            *&text_buf,
            "'I protest!' Behemoth exclaimed hotly. 'Dostoevsky is immortal'"
        );
        // cleanup files
        std::fs::remove_file("simple_test.tree").unwrap();
    }

    /// Test top level undo-redo capability of Editor
    #[test]
    fn undo_redo() {
        // report logger during tests
        simple_logger::SimpleLogger::new().init().unwrap();

        // scratchpads for names and text
        let mut e = editor::Editor::new("undo_redo_test", None).unwrap();
        assert_eq!(e.arbor.name, "undo_redo_test.tree");

        // add some new characters
        e.new_name("cat", "Behemoth").unwrap();
        assert_eq!(e.arbor.name_table.get("cat").unwrap(), "Behemoth");

        for i in 0..10 {
            e.new_node("cat", &format!("test dialogue {}", i)).unwrap();
            e.new_edge(
                &format!("test choice {}", i),
                0,
                i,
                ReqKind::No,
                EffectKind::No,
            )
            .unwrap();
        }

        let tree_full = e.arbor.clone();

        for _ in 0..15 {
            e.undo().unwrap();
        }

        for _ in 0..15 {
            e.redo().unwrap();
        }

        assert_eq!(format!("{:?}", e.arbor), format!("{:?}", tree_full));
        // cleanup files
        std::fs::remove_file("undo_redo_test.tree").unwrap();
    }
}
