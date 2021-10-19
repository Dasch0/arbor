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
                let new_project = Arbor::new(self.name.as_str());

                let encoded = bincode::serialize(&new_project)?;
                let _res = std::fs::write(self.name.clone() + TREE_EXT, encoded);

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
                let event = state.active.tree.add_node(dialogue)?;
                let idx = event.index;
                state.history.push(event.into());

                Ok(idx)
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
                    self.requirement.clone().unwrap_or(ReqKind::No),
                    self.effect.clone().unwrap_or(EffectKind::No),
                );

                trace!("Adding new edge to tree");
                let event = state
                    .active
                    .tree
                    .add_edge(self.source, self.target, choice)?;
                let idx = event.index;

                state.history.push(event.into());
                Ok(idx)
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

                    state.history.push(
                        NameTableInsert {
                            key: self.key,
                            name: self.name,
                        }
                        .into(),
                    );

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

                    state.history.push(
                        ValTableInsert {
                            key: self.key,
                            value: self.value,
                        }
                        .into(),
                    );

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
                let event = state.active.tree.edit_node(self.node_index, new_node)?;
                state.history.push(event.into());

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

                trace!("update edge weight in tree");
                let new_weight = Choice::new(
                    Section::new([start, end], hash),
                    self.requirement.clone().unwrap_or(ReqKind::No),
                    self.effect.clone().unwrap_or(EffectKind::No),
                );
                let event = state.active.tree.edit_edge(self.edge_index, new_weight)?;

                state.history.push(event.into());
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
                    let old_name = *name;
                    debug!("old name: {}, new name: {}", old_name, self.name);

                    trace!("update key-value in name table");
                    *name = self.name;

                    state.history.push(
                        NameTableEdit {
                            key: self.key,
                            from: old_name,
                            to: self.name,
                        }
                        .into(),
                    );

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
                    let value = state
                        .active
                        .val_table
                        .get_mut(&self.key)
                        .ok_or(cmd::Error::Generic)?;
                    let old_value = *value;
                    debug!("old val: {}, new val: {}", old_value, self.value);

                    trace!("update key-value in value table");
                    *value = self.value;

                    state.history.push(
                        ValTableEdit {
                            key: self.key,
                            from: old_value,
                            to: self.value,
                        }
                        .into(),
                    );

                    Ok(self.value as usize)
                } else {
                    Err(cmd::Error::ValNotExists.into())
                }
            }
        }
    }

    pub mod remove {
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

                let event = state.active.tree.remove_node(self.node_index)?;
                let hash = event.node.section.hash;

                state.history.push(event.into());
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

                trace!("remove edge from tree");
                let event = state.active.tree.remove_edge(self.edge_index)?;
                let hash = event.edge.section.hash;

                state.history.push(event.into());
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

                let name = *state
                    .active
                    .name_table
                    .get(&self.key)
                    .ok_or(cmd::Error::NameNotExists)?;

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
                        ReqKind::No => Ok(()),
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
                state
                    .active
                    .name_table
                    .remove(self.key.as_str())
                    .ok_or(cmd::Error::NameNotExists)?;

                state.history.push(
                    NameTableRemove {
                        key: self.key,
                        name,
                    }
                    .into(),
                );

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
                info!("remove value {}", self.key);

                let value = *state
                    .active
                    .val_table
                    .get(&self.key)
                    .ok_or(cmd::Error::ValNotExists)?;

                trace!("check if the key is referenced in any requirements or effects");
                for choice in state.active.tree.edges() {
                    // this match will stop compiling any time a new reqKind is added
                    match &choice.requirement {
                        ReqKind::No => Ok(()),
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
                state
                    .active
                    .val_table
                    .remove(self.key.as_str())
                    .ok_or(cmd::Error::NameNotExists)?;

                state.history.push(
                    ValTableRemove {
                        key: self.key,
                        val: value,
                    }
                    .into(),
                );

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
            state.history.undo(&mut state.active)?;
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
            state.history.redo(&mut state.active)?;
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

            // Clear the undo/redo history
            state.history.clear();

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
    ///
    /// Note that edge and node indices will not remain stable if nodes/edges are removed from the
    /// graph.
    #[derive(new, StructOpt, Debug)]
    #[structopt(setting = AppSettings::NoBinaryName)]
    pub struct List {}

    impl Executable for List {
        fn execute(&self, state: &mut EditorState) -> Result<usize> {
            let mut name_buf = String::with_capacity(64);
            let mut text_buf = String::with_capacity(256);
            let node_iter = state.active.tree.nodes().iter().enumerate();

            for (idx, node) in node_iter {
                let text = &state.active.text[node.section[0]..node.section[1]];
                util::parse_node(text, &state.active.name_table, &mut name_buf, &mut text_buf)?;
                state.scratchpad.push_str(&format!(
                    "node {}: {} says \"{}\"\r\n",
                    idx, name_buf, text_buf
                ));
                let outgoing_edges_iter = state.active.tree.outgoing_from_index(idx)?;
                for edge_index in outgoing_edges_iter {
                    let choice = state.active.tree.get_edge(edge_index)?;
                    util::parse_edge(
                        &state.active.text[choice.section[0]..choice.section[1]],
                        &state.active.name_table,
                        &mut text_buf,
                    )?;
                    state.scratchpad.push_str(&format!(
                        "--> edge {} to node {}: \"{}\"\r\n    requirements: {:?}, effects: {:?}\r\n",
                        edge_index,
                        state.active.tree.target_of(edge_index)?,
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
                if (i & 0x1) == 0 {
                    // token cannot be a name
                    text_buf.push_str(n);
                    Ok(())
                } else {
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

            let root_index: usize = 0;
            let mut dfs = Dfs::new(&tree, root_index);
            while let Some(node_index) = dfs.next(&tree)? {
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
                *new_dialogue = Dialogue::new(Section::new([start, end], new_hash), dialogue.pos);

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
        pub fn validate_tree(data: &Arbor) -> Result<()> {
            // check nodes first, use parallel iterator in case of very large graph
            let nodes_iter = data.tree.nodes().par_iter();
            nodes_iter.try_for_each(|node| -> Result<()> {
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
            let edges_iter = data.tree.edges().par_iter();
            edges_iter.try_for_each(|edge| -> Result<()> {
                // try to grab the text section as a slice, and return an error if the get() failed
                let slice = data.text[..]
                    .get(edge.section[0]..edge.section[1])
                    .ok_or(cmd::Error::InvalidSection)?;
                // if the slice was successful, check its hash
                anyhow::ensure!(
                    seahash::hash(slice.as_bytes()) == edge.section.hash,
                    cmd::Error::InvalidHash
                );
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
}
