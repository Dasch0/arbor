use std::path;

use super::util::lorem_ipsum;
use arbor_core::editor::Editor;
use arbor_core::{self, tree, EffectKind, ReqKind};
use eframe::egui;
use eframe::epi;
use egui::emath::{Pos2, Rect, RectTransform};
use egui::util::History;

// constants for maximum width to show for text throughout UI
const MAX_NAME_WIDTH: f32 = 128.0;
const MAX_TEXT_WIDTH: f32 = 512.0;

// constants for maximum length to expect for text string buffers throughout UI
// so far, longer texts are allowed but will require some extra allocations
const MAX_NAME_LEN: usize = 32;
const MAX_TEXT_LEN: usize = 256;

pub enum Selection {
    None,
    Node(tree::NodeIndex),
    Edge(tree::EdgeIndex),
}

pub struct ArborUi {
    painting: TreePainting,
    new_window: NewProjectWindow,
    load_window: LoadWindow,
    rebuild_window: RebuildWindow,
    backend_panel: BackendPanel,
    name_editor: NameEditor,
    value_editor: ValueEditor,
    node_editor: NodeEditor,
    edge_editor: EdgeEditor,
    active_selection: Selection,
    position_table: Vec<Pos2>,
    arbor_editor: arbor_core::editor::Editor,
}

impl Default for ArborUi {
    /// Default method for creating an arborUI at startup
    ///
    /// # Panic
    /// If core arbor initialization fails
    fn default() -> Self {
        Self {
            painting: Default::default(),
            new_window: Default::default(),
            load_window: Default::default(),
            rebuild_window: Default::default(),
            backend_panel: Default::default(),
            name_editor: Default::default(),
            value_editor: Default::default(),
            node_editor: Default::default(),
            edge_editor: Default::default(),
            active_selection: Selection::None,
            position_table: Vec::new(),
            arbor_editor: Editor::new("template", None).unwrap(),
        }
    }
}

impl epi::App for ArborUi {
    fn name(&self) -> &str {
        "arbor"
    }

    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        // UI elements for loading/saving/new projects. Declare these first so that the project
        // status is known early in the frame

        // Implementation notes:
        //  Window structs take a &mut bool to allow them to close themselves if the close button
        //  is clicked. However since that prevents us from borrowing mut elsewhere, right now a
        //  local bool variable is being created and passed to the window on each frame
        let mut new_window_open = self.new_window.open;
        egui::Window::new("New Project")
            .open(&mut new_window_open)
            .show(ctx, |ui| {
                self.new_window.ui_content(&mut self.arbor_editor, ui);
            });
        self.new_window.open &= new_window_open;

        let mut load_window_open = self.load_window.open;
        egui::Window::new("Load Project")
            .open(&mut load_window_open)
            .show(ctx, |ui| {
                self.load_window
                    .ui_content(&mut self.arbor_editor, &mut self.position_table, ui);
            });
        self.load_window.open &= load_window_open;

        let mut rebuild_window_open = self.rebuild_window.open;
        egui::Window::new("Rebuild Project")
            .open(&mut rebuild_window_open)
            .show(ctx, |ui| {
                self.rebuild_window.ui_content(&mut self.arbor_editor, ui);
            });
        self.rebuild_window.open &= rebuild_window_open;

        let mut backend_panel_open = self.backend_panel.open;
        egui::Window::new("BackendPanel")
            .open(&mut backend_panel_open)
            .show(ctx, |ui| {
                self.backend_panel.update(ctx, frame);
                self.backend_panel.ui(ui, frame);
            });
        self.backend_panel.open = backend_panel_open;

        // Draw rest of UI now that project status is sorted out
        egui::TopPanel::top("Menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    ui.separator();
                    if ui.button("new").clicked() {
                        self.new_window.open = true;
                    }
                    if ui.button("load").clicked() {
                        self.load_window.open = true;
                    }
                    if ui.button("save").clicked() {
                        let res = self.arbor_editor.save(None);
                        match res {
                            Ok(_) => {}
                            Err(e) => println!("{}", e),
                        }
                    }
                    if ui.button("rebuild").clicked() {
                        self.rebuild_window.open = true;
                    }
                    if ui.button("perf").clicked() {
                        self.backend_panel.open = true;
                    }
                    if ui.button("quit").clicked() {
                        frame.quit();
                    }
                });

                egui::menu::menu(ui, "Edit", |ui| {
                    ui.separator();
                    if ui.button("undo").clicked() {
                        let res = self.arbor_editor.undo();
                        match res {
                            Ok(_) => {}
                            Err(e) => println!("{}", e),
                        }
                    }
                    if ui.button("redo").clicked() {
                        let res = self.arbor_editor.redo();
                        match res {
                            Ok(_) => {}
                            Err(e) => println!("{}", e),
                        }
                    }
                });

                egui::menu::menu(ui, "Test", |ui| {
                    ui.separator();
                    if ui.button("lorem ipsum").clicked() {
                        let res = lorem_ipsum(&mut self.position_table, 100);
                        match res {
                            Ok(r) => self.arbor_editor = r,
                            Err(e) => println!("{}", e),
                        }
                    }
                });
            });
        });

        egui::Window::new("Editor Tools").show(ctx, |ui| {
            egui::CollapsingHeader::new("Name Editor").show(ui, |ui| {
                // left panel for editing tools on selected node
                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    self.name_editor.ui_content(&mut self.arbor_editor, ui);
                });
            });

            egui::CollapsingHeader::new("Value Editor").show(ui, |ui| {
                // left panel for editing tools on selected node
                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    self.value_editor.ui_content(&mut self.arbor_editor, ui);
                });
            });

            egui::CollapsingHeader::new("Node Editor").show(ui, |ui| {
                // left panel for editing tools on selected node
                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    self.node_editor.ui_content(&mut self.arbor_editor, ui);
                });
            });

            egui::CollapsingHeader::new("Edge Editor").show(ui, |ui| {
                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    self.edge_editor.ui_content(&mut self.arbor_editor, ui);
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading(&self.arbor_editor.arbor.name);
            self.painting.ui_control(ui);
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                self.painting.ui_content(
                    &mut self.arbor_editor.arbor,
                    &mut self.position_table,
                    &mut self.arbor_editor.history,
                    &mut self.active_selection,
                    ui,
                );
            });
        });
    }
}

/// Window for loading a project
pub struct LoadWindow {
    name_buf: String,
    open: bool,
    was_none: bool,
}

impl Default for LoadWindow {
    fn default() -> Self {
        Self {
            name_buf: String::with_capacity(MAX_NAME_LEN),
            open: false,
            was_none: false,
        }
    }
}

impl LoadWindow {
    /// Content for Load Project window. Returns flag if a new project has been loaded into the
    /// editor state by arbor_core
    pub fn ui_content(
        &mut self,
        editor: &mut Editor,
        position_table: &mut Vec<Pos2>,
        ui: &mut egui::Ui,
    ) {
        ui.label("project name");
        ui.add(
            egui::TextEdit::singleline(&mut self.name_buf)
                .text_style(egui::TextStyle::Monospace)
                .desired_width(MAX_NAME_WIDTH),
        );
        ui.separator();
        if ui.button("load project").clicked() {
            let res = Editor::load(path::Path::new(&self.name_buf));
            match res {
                Ok(_) => {
                    // if ok, close the load project window
                    self.open = false;

                    // TODO: actually store the position list in the filesystem and load it too
                    // displaying warning about placement as a result
                    *position_table = vec![Pos2::default(); editor.arbor.tree.nodes().len()];
                    let mut temp_bool = self.was_none;
                    egui::Window::new("Warning")
                        .open(&mut temp_bool)
                        .show(ui.ctx(), |ui| {
                            ui.label(
                                concat!(
            "The loaded tree did not contain preset positions for visualizing the dialogue tree.\r\n",
            "This is likely because the dialogue tree was created outside the UI editor.\r\n",
            "All nodes with no position information have been set to a default position and will\r\n",
            "need to be manually arranged."
                                ));
                            });
                    self.was_none = temp_bool;
                }
                Err(e) => {
                    println!("{}", e);
                }
            };
        }
    }
}

/// Window for creating a new project
pub struct NewProjectWindow {
    name_buf: String,
    open: bool,
    set_active: bool,
}

impl Default for NewProjectWindow {
    fn default() -> Self {
        Self {
            name_buf: String::with_capacity(MAX_NAME_LEN),
            open: false,
            set_active: true,
        }
    }
}

impl NewProjectWindow {
    /// Content for New Project Window. Returns if flag if a new project has been loaded into the
    /// editor state by arbor_core
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) {
        ui.label("new project name");
        ui.add(
            egui::TextEdit::singleline(&mut self.name_buf)
                .text_style(egui::TextStyle::Monospace)
                .desired_width(MAX_NAME_WIDTH),
        );
        ui.separator();
        ui.checkbox(
            &mut self.set_active,
            "Set new project as active after creating",
        );
        ui.separator();
        if ui.button("create new project").clicked() {
            let res = Editor::new(&self.name_buf, None);
            match res {
                // if result, new project was created and we can close the window
                Ok(r) => {
                    self.open = false;
                    *editor = r;
                }
                // if error, a new project isn't present yet, don't close yet
                Err(e) => {
                    println!("{}", e);
                }
            }
        }
    }
}

/// Window for Rebuilding the current tree
///
/// Cleans up unused text, reorders nodes and edges for optimal access
pub struct RebuildWindow {
    open: bool,
}

impl Default for RebuildWindow {
    fn default() -> Self {
        Self { open: false }
    }
}

impl RebuildWindow {
    /// Content for New Project Window. Returns if flag if a new project has been loaded into the
    /// editor state by arbor_core
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) {
        ui.label(concat!(
            "This recreates the current dialogue tree, removing unused text and reordering nodes",
            "and edges for optimal access. Rebuilding the tree clears the entire undo/redo history"
        ));
        ui.separator();
        if ui.button("rebuild current project").clicked() {
            let res = editor.rebuild();
            match res {
                Ok(_) => self.open = false,
                Err(e) => {
                    println!("{}", e);
                }
            }
        }
    }
}

/// Struct for editing names
pub struct NameEditor {
    key_buf: String,
    text_buf: String,
}

impl Default for NameEditor {
    fn default() -> Self {
        Self {
            key_buf: String::with_capacity(MAX_NAME_LEN),
            text_buf: String::with_capacity(MAX_NAME_LEN),
        }
    }
}

impl NameEditor {
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("key");
            ui.add(
                egui::TextEdit::singleline(&mut self.key_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(MAX_NAME_WIDTH),
            );
            ui.separator();
            ui.label("name");
            ui.add(
                egui::TextEdit::singleline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(MAX_NAME_WIDTH),
            );
            ui.separator();

            if ui.button("new name").clicked() {
                let res = editor.new_name(&self.key_buf, &self.text_buf);
                match res {
                    Ok(_) => {
                        // clear buffers if everything worked ok
                        self.key_buf.clear();
                        self.text_buf.clear();
                    }
                    Err(e) => println!("{}", e),
                }
            }
        })
        .response
    }
}

pub struct ValueEditor {
    key_buf: String,
    value: u32,
}

impl Default for ValueEditor {
    fn default() -> Self {
        Self {
            key_buf: String::with_capacity(MAX_NAME_LEN),
            value: 0,
        }
    }
}

impl ValueEditor {
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("key");
            ui.add(
                egui::TextEdit::singleline(&mut self.key_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(MAX_NAME_WIDTH),
            );
            ui.separator();
            ui.label("initial value");
            ui.add(egui::DragValue::new(&mut self.value));
            ui.separator();

            if ui.button("new value").clicked() {
                let res = editor.new_val(&self.key_buf, self.value);
                match res {
                    Ok(_) => {
                        // clear buffers if everything worked ok
                        self.key_buf.clear();
                        self.value = 0;
                    }
                    Err(e) => println!("{}", e),
                }
            }
        })
        .response
    }
}

pub struct NodeEditor {
    name_buf: String,
    text_buf: String,
}

impl Default for NodeEditor {
    fn default() -> Self {
        Self {
            name_buf: String::with_capacity(MAX_NAME_LEN),
            text_buf: String::with_capacity(MAX_TEXT_LEN),
        }
    }
}

impl NodeEditor {
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("name");
            egui::ComboBox::from_label(
                editor // display the selected key's name value
                    .arbor
                    .name_table
                    .get(&self.name_buf)
                    .unwrap_or(&String::default())
                    .to_string(),
            )
            .selected_text(self.name_buf.clone())
            .show_ui(ui, |ui| {
                // Name must be in key form when selecting,
                for name in editor.arbor.name_table.keys() {
                    ui.selectable_value(&mut self.name_buf, name.to_string(), name.as_str());
                }
            });
            ui.separator();
            ui.label("text");
            ui.add(
                egui::TextEdit::multiline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(10),
            );
            ui.separator();

            if ui.button("new node").clicked() {
                let res = editor.new_node(&self.name_buf, &self.text_buf);
                match res {
                    Ok(node_index) => {
                        // clear buffers if successful
                        self.name_buf.clear();
                        self.text_buf.clear();
                    }
                    Err(e) => println!("{}", e),
                }
            }
        })
        .response
    }
}

pub struct EdgeEditor {
    source_node: usize,
    target_node: usize,
    text_buf: String,
}

impl Default for EdgeEditor {
    fn default() -> Self {
        Self {
            source_node: 0,
            target_node: 0,
            text_buf: String::with_capacity(MAX_TEXT_LEN),
        }
    }
}

impl EdgeEditor {
    pub fn ui_content(&mut self, editor: &mut Editor, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("source node");
            ui.add(egui::DragValue::new(&mut self.source_node));

            ui.label("target Node");
            ui.add(egui::DragValue::new(&mut self.target_node));

            ui.separator();
            ui.add(
                egui::TextEdit::multiline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(10),
            );
            ui.separator();

            if ui.button("new edge").clicked() {
                let res = editor.new_edge(
                    &self.text_buf,
                    self.source_node,
                    self.target_node,
                    ReqKind::No,
                    EffectKind::No,
                );
                match res {
                    Ok(_) => println!("successfully added edge"),
                    Err(e) => println!("{}", e),
                }
            }
        })
        .response
    }
}

pub struct TreePainting {
    pub stroke: egui::Color32,
    pub fill: egui::Color32,
    pub select_color: egui::Color32,
    pub hover_name_buf: String,
    pub hover_text_buf: String,
    pub node_size: f32,
    pub zoom: f32,
    /// the pan amount, equivalent to translation matrix
    pub pan: egui::Pos2,
    /// the start coordinates of a new pan action, driven by the pointer position
    pub pan_start: egui::Pos2,
    /// the origin, moved around by pans but only updated after pan completed
    pub origin: egui::Pos2,
    /// temporary storage of the position of a node being dragged. Used to add a click-and-drag
    /// event to the undo/redo history
    node_drag_pos: Pos2,
}

impl Default for TreePainting {
    fn default() -> Self {
        Self {
            stroke: egui::Color32::LIGHT_BLUE,
            fill: egui::Color32::LIGHT_GRAY,
            select_color: egui::Color32::RED,
            hover_name_buf: String::with_capacity(MAX_NAME_LEN),
            hover_text_buf: String::with_capacity(MAX_TEXT_LEN),
            node_size: 20.0,
            zoom: 1.0,
            pan: egui::pos2(0.0, 0.0),
            pan_start: egui::pos2(0.0, 0.0),
            origin: egui::pos2(0.0, 0.0),
            node_drag_pos: Pos2::default(),
        }
    }
}

impl TreePainting {
    #[inline]
    fn transform(&self, p: egui::Pos2) -> egui::Pos2 {
        egui::pos2(p.x * self.zoom + self.pan.x, p.y * self.zoom + self.pan.y)
    }

    #[inline]
    fn reform(&self, p: egui::Pos2) -> egui::Pos2 {
        egui::pos2(
            (p.x - self.pan.x) / self.zoom,
            (p.y - self.pan.y) / self.zoom,
        )
    }

    pub fn ui_control(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            ui.label("stroke");
            ui.color_edit_button_srgba(&mut self.stroke);
            ui.separator();
            ui.label("fill");
            ui.color_edit_button_srgba(&mut self.fill);
            ui.label("zoom");
            ui.add(egui::Slider::new(&mut self.zoom, 0.0001..=2.0));
            ui.separator();
            ui.label("x position");
            ui.add(egui::DragValue::new(&mut self.pan.x));
            ui.separator();
            ui.label("y position");
            ui.add(egui::DragValue::new(&mut self.pan.y));
            ui.separator();
        })
        .response
    }

    /// Draw the painting of the dialogue tree, return an optional value describing any in-progress
    /// or completed events
    pub fn ui_content(
        &mut self,
        arbor: &arbor_core::Arbor,
        position_table: &mut Vec<Pos2>,
        _history: &mut arbor_core::History,
        active_selection: &mut Selection,
        ui: &mut egui::Ui,
    ) {
        // get an area to paint to
        let (response, painter) =
            ui.allocate_painter(ui.available_size_before_wrap_finite(), egui::Sense::hover());

        // get conversion from paint area space to screen space (which is how egui computes
        // collision boxes)
        let to_screen = RectTransform::from_to(
            Rect::from_min_size(Pos2::ZERO, response.rect.square_proportions()),
            response.rect,
        );
        let from_screen = to_screen.inverse();

        // draw edges first, since they need to be behind nodes
        for (edge_index, choice) in arbor.tree.edges().iter().enumerate() {
            let slice = &arbor.text[choice.section[0]..choice.section[1]];
            let _res =
                arbor_core::util::parse_edge(slice, &arbor.name_table, &mut self.hover_text_buf);

            let source_node_index = arbor.tree.source_of(edge_index).unwrap();
            let target_node_index = arbor.tree.target_of(edge_index).unwrap();

            let source_pos = position_table.get(source_node_index).unwrap();
            let target_pos = position_table.get(source_node_index).unwrap();

            let source_coord = to_screen * self.transform(egui::pos2(source_pos.x, source_pos.y));
            let target_coord = to_screen * self.transform(egui::pos2(target_pos.x, target_pos.y));

            // compute midpoint of line to place edge popup
            let midpoint = egui::pos2(
                (source_coord.x + target_coord.x) / 2.0,
                (source_coord.y + target_coord.y) / 2.0,
            );

            // create clickable hitbox at midpoint
            let rect = Rect::from_center_size(midpoint, egui::vec2(self.node_size, self.node_size));
            let resp = ui.interact(
                rect,
                egui::Id::new(edge_index).with("__edge_id"),
                egui::Sense::click_and_drag(),
            );
            // select edge if the text pop-up is clicked
            if resp.interact_pointer_pos().is_some() {
                *active_selection = Selection::Edge(edge_index);
            }

            // paint popup with edge text if conditions are met
            if response.rect.contains(midpoint) && self.zoom > 0.3 {
                Self::edge_text_popup(&response.ctx, edge_index, midpoint, |ui| {
                    ui.vertical(|ui| {
                        ui.label(self.hover_text_buf.as_str());
                    });
                });
            }

            // change edge color if this edge is the actively selected thing
            let edge_color = match active_selection {
                Selection::Edge(e) => {
                    if *e == edge_index {
                        self.select_color
                    } else {
                        self.stroke
                    }
                }
                _ => self.stroke,
            };

            // draw circle around edge hitbox
            painter.add(egui::Shape::circle_filled(
                midpoint,
                self.node_size * 0.5 * self.zoom, // FIXME: edge vs node scaling is hardcoded
                edge_color,
            ));

            // Finally, paint arrow along edge, stop at edge of target node to show arrow tip
            Self::arrow(
                &painter,
                source_coord,
                target_coord,
                self.node_size,
                self.zoom,
                (self.zoom, edge_color).into(),
            );
        }

        // loop over the nodes, draw them, and update their location if being dragged
        for (i, n) in arbor.tree.nodes().iter().enumerate() {
            let pos = position_table.get_mut(i).unwrap();

            let p = egui::pos2(pos.x, pos.y);
            let coord = to_screen * self.transform(p);
            let rect =
                Rect::from_center_size(coord, egui::vec2(self.node_size * 2., self.node_size * 2.));
            let resp = ui.interact(
                rect,
                egui::Id::new(i).with("__node_index"),
                egui::Sense::click_and_drag(),
            );
            let node_slice = &arbor.text[n.section[0]..n.section[1]];
            let _res = arbor_core::util::parse_node(
                node_slice,
                &arbor.name_table,
                &mut self.hover_name_buf,
                &mut self.hover_text_buf,
            );

            // move node with mouse drag
            if let Some(pointer_pos) = resp.interact_pointer_pos() {
                let new_pos = self.reform(from_screen * pointer_pos);
                // bypass normal cmd interface here to avoid spamming event history during a drag
                *pos = Pos2::new(new_pos.x, new_pos.y);
                *active_selection = Selection::Node(i);
            }

            // save initial position of a node when starting the drag, used below when qualifying
            // the node movement in the undo/redo history
            if resp.drag_started() {
                self.node_drag_pos = *pos;
            }

            // qualify node movement in event history after drag release
            if resp.drag_released() {
                let mut old_pos_node = *n;
                *pos = self.node_drag_pos;
                // TODO: undo/redo not supported for node movements
            }

            // get custom fill color for active selection
            let fill_color = match active_selection {
                Selection::Node(n) => {
                    if *n == i {
                        self.select_color
                    } else {
                        self.fill
                    }
                }
                _ => self.fill,
            };

            // only draw text if the user is zoomed in enough for it to make sense
            if response.rect.contains(coord) && self.zoom > 0.3 {
                Self::node_text_popup(&resp.ctx, n.section.hash, coord, |ui| {
                    ui.vertical(|ui| {
                        ui.label(self.hover_name_buf.as_str());
                        ui.label("------");
                        ui.label(self.hover_text_buf.as_str());
                    });
                });
            }

            // draw nodes
            if response.rect.contains(coord) {
                painter.add(egui::Shape::circle_filled(
                    coord,
                    self.node_size * self.zoom,
                    fill_color,
                ));
            }
        }

        // handle dragging to pan screen after drawing nodes so that clicking/dragging nodes
        // has priority
        let pan_response = response.interact(egui::Sense::drag());
        if let Some(pointer_pos) = pan_response.interact_pointer_pos() {
            if pan_response.drag_started() {
                self.pan_start = from_screen * pointer_pos;
                self.origin = self.pan;
            }
            let pan_vec = (from_screen * pointer_pos) - self.pan_start;
            self.pan = self.origin + pan_vec;
        }

        // clear selection if a click occured that didn't hit a node/edge up above
        if pan_response.clicked() {
            *active_selection = Selection::None;
        }
    }

    fn edge_text_popup(
        ctx: &egui::CtxRef,
        edge_index: usize,
        window_pos: Pos2,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) -> egui::Response {
        egui::Area::new(egui::Id::new(edge_index).with("__edge_tooltip"))
            .order(egui::Order::Middle) // middle allows other windows to get on top of the popups
            .fixed_pos(window_pos)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.style()).show(ui, |ui| {
                    ui.set_max_width(MAX_TEXT_WIDTH);
                    add_contents(ui);
                });
            })
    }

    fn node_text_popup(
        ctx: &egui::CtxRef,
        node_hash: u64,
        window_pos: Pos2,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) -> egui::Response {
        egui::Area::new(egui::Id::new(node_hash).with("__node_tooltip"))
            .order(egui::Order::Middle) // middle allows other windows to get on top of the popups
            .fixed_pos(window_pos)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.style()).show(ui, |ui| {
                    ui.set_max_width(MAX_TEXT_WIDTH);
                    add_contents(ui);
                });
            })
    }

    fn arrow(
        painter: &egui::Painter,
        source: Pos2,
        target: egui::Pos2,
        standoff: f32,
        zoom: f32,
        stroke: egui::Stroke,
    ) {
        let rot = egui::emath::Rot2::from_angle(std::f32::consts::TAU / 10.0);
        let tip_length = 8.0 * zoom;
        let dir = (target.to_vec2() - source.to_vec2()).normalized();
        let tip = target - dir * (standoff * zoom);
        painter.line_segment([source, tip], stroke);
        painter.line_segment([tip, tip - tip_length * (rot * dir)], stroke);
        painter.line_segment([tip, tip - tip_length * (rot.inverse() * dir)], stroke);
    }
}

pub struct FrameHistory {
    frame_times: History<f32>,
}

impl Default for FrameHistory {
    fn default() -> Self {
        let max_age: f64 = 1.0;
        Self {
            frame_times: History::from_max_len_age((max_age * 300.0).round() as usize, max_age),
        }
    }
}

impl FrameHistory {
    // Called first
    pub fn on_new_frame(&mut self, now: f64, previus_frame_time: Option<f32>) {
        let previus_frame_time = previus_frame_time.unwrap_or_default();
        if let Some(latest) = self.frame_times.latest_mut() {
            *latest = previus_frame_time; // rewrite history now that we know
        }
        self.frame_times.add(now, previus_frame_time); // projected
    }

    pub fn mean_frame_time(&self) -> f32 {
        self.frame_times.average().unwrap_or_default()
    }

    pub fn fps(&self) -> f32 {
        1.0 / self.frame_times.mean_time_interval().unwrap_or_default()
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "Total frames painted: {}",
            self.frame_times.total_count()
        ))
        .on_hover_text("Includes this frame.");

        ui.label(format!(
            "Mean CPU usage: {:.2} ms / frame",
            1e3 * self.mean_frame_time()
        ))
        .on_hover_text(
            "Includes egui layout and tessellation time.\n\
            Does not include GPU usage, nor overhead for sending data to GPU.",
        );
        egui::warn_if_debug_build(ui);

        egui::CollapsingHeader::new("📊 CPU usage history")
            .default_open(false)
            .show(ui, |ui| {
                self.graph(ui);
            });
    }

    fn graph(&mut self, ui: &mut egui::Ui) -> egui::Response {
        use egui::*;

        ui.label("egui CPU usage history");

        let history = &self.frame_times;

        // TODO: we should not use `slider_width` as default graph width.
        let height = ui.spacing().slider_width;
        let size = vec2(ui.available_size_before_wrap_finite().x, height);
        let (rect, response) = ui.allocate_at_least(size, Sense::hover());
        let style = ui.style().noninteractive();

        let graph_top_cpu_usage = 0.010;
        let graph_rect = Rect::from_x_y_ranges(history.max_age()..=0.0, graph_top_cpu_usage..=0.0);
        let to_screen = emath::RectTransform::from_to(graph_rect, rect);

        let mut shapes = Vec::with_capacity(3 + 2 * history.len());
        shapes.push(Shape::Rect {
            rect,
            corner_radius: style.corner_radius,
            fill: ui.visuals().extreme_bg_color,
            stroke: ui.style().noninteractive().bg_stroke,
        });

        let rect = rect.shrink(4.0);
        let color = ui.visuals().text_color();
        let line_stroke = Stroke::new(1.0, color);

        if let Some(pointer_pos) = ui.input().pointer.hover_pos() {
            if rect.contains(pointer_pos) {
                let y = pointer_pos.y;
                shapes.push(Shape::line_segment(
                    [pos2(rect.left(), y), pos2(rect.right(), y)],
                    line_stroke,
                ));
                let cpu_usage = to_screen.inverse().transform_pos(pointer_pos).y;
                let text = format!("{:.1} ms", 1e3 * cpu_usage);
                shapes.push(Shape::text(
                    ui.fonts(),
                    pos2(rect.left(), y),
                    egui::Align2::LEFT_BOTTOM,
                    text,
                    TextStyle::Monospace,
                    color,
                ));
            }
        }

        let circle_color = color;
        let radius = 2.0;
        let right_side_time = ui.input().time; // Time at right side of screen

        for (time, cpu_usage) in history.iter() {
            let age = (right_side_time - time) as f32;
            let pos = to_screen.transform_pos_clamped(Pos2::new(age, cpu_usage));

            shapes.push(Shape::line_segment(
                [pos2(pos.x, rect.bottom()), pos],
                line_stroke,
            ));

            if cpu_usage < graph_top_cpu_usage {
                shapes.push(Shape::circle_filled(pos, radius, circle_color));
            }
        }

        ui.painter().extend(shapes);

        response
    }
}

/// How often we repaint the demo app by default
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RunMode {
    /// This is the default for the demo.
    ///
    /// If this is selected, egui is only updated if are input events
    /// (like mouse movements) or there are some animations in the GUI.
    ///
    /// Reactive mode saves CPU.
    Reactive,

    /// This will call `egui::Context::request_repaint()` at the end of each frame
    /// to request the backend to repaint as soon as possible.
    Continuous,
}

/// Default is Reactive since
/// 1) We want to use minimal CPU
/// 2) There are no external events that could invalidate the UI
///    so there are no events to miss.
impl Default for RunMode {
    fn default() -> Self {
        RunMode::Reactive
    }
}

struct BackendPanel {
    pub open: bool,

    // go back to `Reactive` mode each time we start
    run_mode: RunMode,

    /// current slider value for current gui scale
    pixels_per_point: Option<f32>,

    frame_history: FrameHistory,
}

impl Default for BackendPanel {
    fn default() -> Self {
        Self {
            open: false,
            run_mode: Default::default(),
            pixels_per_point: Default::default(),
            frame_history: Default::default(),
        }
    }
}

impl BackendPanel {
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        self.frame_history
            .on_new_frame(ctx.input().time, frame.info().cpu_usage);

        if self.run_mode == RunMode::Continuous {
            // Tell the backend to repaint as soon as possible
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut epi::Frame<'_>) {
        ui.heading("💻 Backend");

        self.run_mode_ui(ui);

        ui.separator();

        self.frame_history.ui(ui);

        // For instance: `egui_web` sets `pixels_per_point` every frame to force
        // egui to use the same scale as the web zoom factor.
        let integration_controls_pixels_per_point = ui.input().raw.pixels_per_point.is_some();
        if !integration_controls_pixels_per_point {
            ui.separator();
            if let Some(new_pixels_per_point) = self.pixels_per_point_ui(ui, frame.info()) {
                ui.ctx().set_pixels_per_point(new_pixels_per_point);
            }
        }
        ui.separator();
    }

    fn pixels_per_point_ui(
        &mut self,
        ui: &mut egui::Ui,
        info: &epi::IntegrationInfo,
    ) -> Option<f32> {
        #![allow(clippy::float_cmp)]

        self.pixels_per_point = self
            .pixels_per_point
            .or(info.native_pixels_per_point)
            .or_else(|| Some(ui.ctx().pixels_per_point()));

        let pixels_per_point = self.pixels_per_point.as_mut()?;

        ui.horizontal(|ui| {
            ui.spacing_mut().slider_width = 90.0;
            ui.add(
                egui::Slider::new(pixels_per_point, 0.5..=5.0)
                    .logarithmic(true)
                    .text("Scale"),
            )
            .on_hover_text("Physical pixels per point.");
            if let Some(native_pixels_per_point) = info.native_pixels_per_point {
                let button = egui::Button::new("Reset")
                    .enabled(*pixels_per_point != native_pixels_per_point);
                if ui
                    .add(button)
                    .on_hover_text(format!(
                        "Reset scale to native value ({:.1})",
                        native_pixels_per_point
                    ))
                    .clicked()
                {
                    *pixels_per_point = native_pixels_per_point;
                }
            }
        });

        // We wait until mouse release to activate:
        if ui.ctx().is_using_pointer() {
            None
        } else {
            Some(*pixels_per_point)
        }
    }

    fn run_mode_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let run_mode = &mut self.run_mode;
            ui.label("Mode:");
            ui.radio_value(run_mode, RunMode::Continuous, "Continuous")
                .on_hover_text("Repaint everything each frame");
            ui.radio_value(run_mode, RunMode::Reactive, "Reactive")
                .on_hover_text("Repaint when there are animations or input (e.g. mouse movement)");
        });

        if self.run_mode == RunMode::Continuous {
            ui.label(format!(
                "Repainting the UI each frame. FPS: {:.1}",
                self.frame_history.fps()
            ));
        } else {
            ui.label("Only running UI code when there are animations or input");
        }
    }
}
