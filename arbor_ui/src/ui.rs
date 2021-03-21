use arbor_core::*;
use egui::emath::{Pos2, Rect, RectTransform};
use egui::util::History;

// constants for maximum width to show for text throughout UI
const MAX_NAME_WIDTH: f32 = 64.0;
const MAX_TEXT_WIDTH: f32 = 160.0;

// constants for maximum length to expect for text string buffers throughout UI
// so far, longer names are allowed but will require some extra allocations
const MAX_NAME_LEN: usize = 16;
const MAX_TEXT_LEN: usize = 256;

pub struct ArborUi {
    painting: TreePainting,
    new_project_window: NewProjectWindow,
    backend_panel: BackendPanel,
    name_editor: NameEditor,
    node_editor: NodeEditor,
    edge_editor: EdgeEditor,
    state: arbor_core::EditorState,
}

impl Default for ArborUi {
    fn default() -> Self {
        Self {
            painting: Default::default(),
            new_project_window: Default::default(),
            backend_panel: Default::default(),
            name_editor: Default::default(),
            node_editor: Default::default(),
            edge_editor: Default::default(),
            state: EditorState::new(DialogueTreeData::default()),
        }
    }
}

impl epi::App for ArborUi {
    fn name(&self) -> &str {
        "arbor"
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {

        // Implementation notes:
        //  Window structs take a &mut bool to allow them to close themselves if the close button
        //  is clicked. However since that prevents us from borrowing mut elsewhere, right now a
        //  local bool variable is being created and passed to the window on each frame
        let mut new_project_window_open = self.new_project_window.open; 
        egui::Window::new("New Project").open(&mut new_project_window_open).show(ctx, |ui| {
            self.new_project_window.ui_content(&mut self.state, ui);
        });
        self.new_project_window.open = new_project_window_open;


        let mut backend_panel_open = self.backend_panel.open;
        egui::Window::new("BackendPanel").open(&mut backend_panel_open).show(ctx, |ui| {
            self.backend_panel.update(ctx, frame);
            self.backend_panel.ui(ui, frame);
        });
        self.backend_panel.open = backend_panel_open;

        // Top panel for menu
        egui::TopPanel::top("Menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
                    if ui.button("New").clicked() {
                        self.new_project_window.open = true;
                    }
                    if ui.button("Load").clicked() {
                        let res = cmd::Load::new("test".to_string()).execute(&mut self.state);
                        println!("{:?}", res);
                    }
                    if ui.button("About").clicked() {
                        self.backend_panel.open = true;
                    }
                    if ui.button("Quit").clicked() {
                        frame.quit();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Dialogue Tree");
            self.painting.ui_control(ui);
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                self.painting.ui_content(&mut self.state.act, ui);
            });
        });

        egui::Window::new("Name Editor")
            .default_size(egui::vec2(MAX_NAME_WIDTH, f32::INFINITY))
            .show(ctx, |ui| {
                // left panel for editing tools on selected node
                egui::ScrollArea::auto_sized().show(ui, |ui| {
                    self.name_editor.ui_content(&mut self.state, ui);
                });
            });

        egui::Window::new("Node Editor").show(ctx, |ui| {
            // left panel for editing tools on selected node
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                self.node_editor
                    .ui_content(&mut self.state, &mut self.painting, ui);
            });
        });

        egui::Window::new("Edge Editor").show(ctx, |ui| {
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                self.edge_editor.ui_content(&mut self.state, ui);
            });
        });
    }
}

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
            set_active: true
        }
    }
}

impl NewProjectWindow {
    pub fn ui_content(&mut self, state: &mut EditorState, ui: &mut egui::Ui) {
        ui.label("New project name");
        ui.add(
            egui::TextEdit::singleline(&mut self.name_buf)
                .text_style(egui::TextStyle::Monospace)
                .desired_width(MAX_NAME_WIDTH),
        );
        ui.separator();
        ui.checkbox(&mut self.set_active, "Set new project as active after creating");
        if ui.button("Create New Project").clicked() {
            let res = arbor_core::cmd::new::Project::new(self.name_buf.drain(..).collect(), self.set_active).execute(state);
            // close window if new project was successful, but leave it open if there was a problem
            match res {
                Ok(_) => return,
                Err(e) => println!("{}", e),
            }
        }
    }
}

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
    pub fn ui_content(&mut self, state: &mut EditorState, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("Key");
            ui.add(
                egui::TextEdit::singleline(&mut self.key_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(MAX_NAME_WIDTH),
            );
            ui.separator();
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(MAX_NAME_WIDTH),
            );
            ui.separator();

            if ui.button("New Name").clicked() {
                let res = cmd::new::Name::new(
                    self.key_buf.drain(..).collect(),
                    self.text_buf.drain(..).collect(),
                )
                .execute(state);
                match res {
                    Ok(_) => return,
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
    pub fn ui_content(
        &mut self,
        state: &mut EditorState,
        painting: &mut TreePainting,
        ui: &mut egui::Ui,
    ) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("Name");
            egui::combo_box_with_label(
                ui,
                state // display the selected key's name value
                    .act
                    .name_table
                    .get(self.name_buf.as_str())
                    .unwrap_or(&"".to_string()),
                self.name_buf.clone(),
                |ui| {
                    // Name must be in key form when selecting,
                    for name in state.act.name_table.keys() {
                        ui.selectable_value(&mut self.name_buf, name.clone(), name);
                    }
                },
            );
            ui.separator();
            ui.label("Text");
            ui.add(
                egui::TextEdit::multiline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(20),
            );
            ui.separator();

            if ui.button("New Node").clicked() {
                let res = cmd::new::Node::new(
                    self.name_buf.drain(..).collect(),
                    self.text_buf.drain(..).collect(),
                )
                .execute(state);
                match res {
                    Ok(_) => painting.node_pos_list.push(egui::Pos2::new(0.3, 0.3)),
                    Err(e) => println!("{}", e),
                }
            }
        })
        .response
    }
}

pub struct EdgeEditor {
    source_node: u32,
    target_node: u32,
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
    pub fn ui_content(&mut self, state: &mut EditorState, ui: &mut egui::Ui) -> egui::Response {
        ui.vertical(|ui| {
            ui.label("Source Node");
            ui.add(egui::DragValue::u32(&mut self.source_node));

            ui.label("Target Node");
            ui.add(egui::DragValue::u32(&mut self.target_node));

            ui.separator();
            ui.add(
                egui::TextEdit::multiline(&mut self.text_buf)
                    .text_style(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(20),
            );
            ui.separator();

            if ui.button("New Edge").clicked() {
                let res = cmd::new::Edge::new(
                    self.source_node,
                    self.target_node,
                    self.text_buf.drain(..).collect(),
                    None,
                    None,
                )
                .execute(state);
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
    pub node_pos_list: Vec<egui::Pos2>,
    pub stroke: egui::Stroke,
    pub fill: egui::Color32,
    pub hover_name_buf: String,
    pub hover_text_buf: String,
    pub node_size: f32,
}

impl Default for TreePainting {
    fn default() -> Self {
        Self {
            node_pos_list: Default::default(),
            stroke: egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
            fill: egui::Color32::LIGHT_GRAY,
            hover_name_buf: String::with_capacity(MAX_NAME_LEN),
            hover_text_buf: String::with_capacity(MAX_TEXT_LEN),
            node_size: 20.0,
        }
    }
}

impl TreePainting {
    pub fn ui_control(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            egui::stroke_ui(ui, &mut self.stroke, "Edge");
            ui.separator();
            ui.label("Node");
            ui.color_edit_button_srgba(&mut self.fill);
        })
        .response
    }

    pub fn ui_content(
        &mut self,
        data: &arbor_core::DialogueTreeData,
        ui: &mut egui::Ui,
    ) -> egui::Response {
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

        // Implementation notes:
        // in petgraphs standard graph implementation, the nodes are stored in a backing array, and
        // per the docs, the node index is the raw position in the array. This means that the node
        // index may change from frame to frame if nodes are removed, but at the same time it means
        // that it is safe to iterate over the raw nodes array and directly map a node index to an
        // index on the positions list (that we maintain ourselves).
        //
        // When implementing removals or saving (which shuffles node id's) in the UI, the points
        // list will need to be updated accordingly
        // NOTE: for those future implementations, maybe use the hash of the node before and after the
        // removal to update the position list with

        // draw edges
        painter.extend(
            data.tree
                .edge_references()
                .map(|edge_ref| {
                    let choice = data.tree.edge_weight(edge_ref.id()).unwrap();
                    let slice = &data.text[choice.section[0]..choice.section[1]];
                    let _res =
                        cmd::util::parse_edge(slice, &data.name_table, &mut self.hover_text_buf);

                    let source_node_index = edge_ref.source().index();
                    let target_node_index = edge_ref.target().index();
                    // compute midpoint of line to place edge popup
                    let source_pos = to_screen * self.node_pos_list[source_node_index];
                    let target_pos = to_screen * self.node_pos_list[target_node_index];
                    let midpoint = egui::pos2(
                        (source_pos.x + target_pos.x) / 2.0,
                        (source_pos.y + target_pos.y) / 2.0,
                    );

                    // bias currently shifts the action text a bit up & left so it overlaps with
                    // the line.
                    // NOTE: This is has been tuned manually
                    let bias = egui::vec2(20.0, 10.0);
                    Self::edge_text_popup(
                        &response.ctx,
                        edge_ref.id().index(),
                        midpoint - bias,
                        |ui| {
                            ui.vertical(|ui| {
                                ui.label(self.hover_text_buf.as_str());
                            });
                        },
                    );
                    egui::Shape::line(vec![source_pos, target_pos], self.stroke)
                })
                .collect(),
        );

        // draw nodes
        painter.extend(
            (0..self.node_pos_list.len())
                .map(|i| {
                    let p = self.node_pos_list[i];
                    let coord = to_screen * p;
                    let rect =
                        Rect::from_center_size(coord, egui::vec2(self.node_size, self.node_size));
                    let node_index = arbor_core::NodeIndex::new(i);
                    let resp = ui.interact(rect, egui::Id::new(i), egui::Sense::click_and_drag());
                    if let Some(node_weight) = data.tree.node_weight(node_index) {
                        let node_slice = &data.text[node_weight[0]..node_weight[1]];
                        let _res = cmd::util::parse_node(
                            node_slice,
                            &data.name_table,
                            &mut self.hover_name_buf,
                            &mut self.hover_text_buf,
                        );
                        Self::node_text_popup(&resp.ctx, i, coord, |ui| {
                            ui.vertical(|ui| {
                                ui.label(self.hover_name_buf.as_str());
                                ui.label("------");
                                ui.label(self.hover_text_buf.as_str());
                            });
                        });

                        // move node
                        if let Some(pointer_pos) = resp.interact_pointer_pos() {
                            self.node_pos_list[i] = from_screen * pointer_pos;
                        }

                        egui::Shape::circle_filled(coord, self.node_size, self.fill)
                    } else {
                        egui::Shape::Noop
                    }
                })
                .collect(),
        );

        response
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
                })
            })
    }

    fn node_text_popup(
        ctx: &egui::CtxRef,
        node_index: usize,
        window_pos: Pos2,
        add_contents: impl FnOnce(&mut egui::Ui),
    ) -> egui::Response {
        egui::Area::new(egui::Id::new(node_index).with("__node_tooltip"))
            .order(egui::Order::Middle) // middle allows other windows to get on top of the popups
            .fixed_pos(window_pos)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(&ctx.style()).show(ui, |ui| {
                    ui.set_max_width(MAX_TEXT_WIDTH);
                    add_contents(ui);
                })
            })
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

        if let Some(pointer_pos) = ui.input().pointer.tooltip_pos() {
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

    #[cfg_attr(feature = "persistence", serde(skip))]
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
                egui::Slider::f32(pixels_per_point, 0.5..=5.0)
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
