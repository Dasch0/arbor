use egui::util::History;

pub struct ArborUi {
    // Example stuff:
    buffer: String,
    painting: Painting,
    backend_panel: BackendPanel,
}

impl Default for ArborUi{
    fn default() -> Self {
        Self {
            // Example stuff:
            buffer: String::with_capacity(500),
            painting: Default::default(),
            backend_panel: Default::default(),
        }
    }
}

impl epi::App for ArborUi{
    fn name(&self) -> &str {
        "arbor"
    }

    /// Called each time the UI needs repainting, which may be many times per second.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    fn update(&mut self, ctx: &egui::CtxRef, frame: &mut epi::Frame<'_>) {
        // Top panel for menu
        egui::TopPanel::top("Menu").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::menu::bar(ui, |ui| {
                egui::menu::menu(ui, "File", |ui| {
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
            egui::warn_if_debug_build(ui);

            ui.separator();

            ui.heading("Draw with your mouse to paint:");
            self.painting.ui_control(ui);
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                self.painting.ui_content(ui);
            });
        });

        egui::Window::new("editor").show(ctx, |ui| {
            // left panel for editing tools on selected node
            egui::ScrollArea::auto_sized().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::multiline(&mut self.buffer)
                        .text_style(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY).desired_rows(20));
                });
            });
        });

        // Display backend window if the user clicks the button
        self.backend_panel.update(ctx, frame);
        if self.backend_panel.open || ctx.memory().everything_is_visible() {
            egui::Window::new("backend_panel").show(ctx, |ui| {
                self.backend_panel.ui(ui, frame);
            });
        }
    }
}

/// Example code for painting on a canvas with your mouse
struct Painting {
    points: Vec<egui::Pos2>,
    stroke: egui::Stroke,
}

impl Default for Painting {
    fn default() -> Self {
        Self {
            points: Default::default(),
            stroke: egui::Stroke::new(1.0, egui::Color32::LIGHT_BLUE),
        }
    }
}

impl Painting {
    pub fn ui_control(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            egui::stroke_ui(ui, &mut self.stroke, "Stroke");
            ui.separator();
            if ui.button("Clear Painting").clicked() {
                self.points.clear();
            }
        })
        .response
    }

    pub fn ui_content(&mut self, ui: &mut egui::Ui) -> egui::Response {
        use egui::emath::{Pos2, Rect, RectTransform};

        let (mut response, painter) =
            ui.allocate_painter(ui.available_size_before_wrap_finite(), egui::Sense::drag());

        let to_screen = RectTransform::from_to(
            Rect::from_min_size(Pos2::ZERO, response.rect.square_proportions()),
            response.rect,
        );
        let from_screen = to_screen.inverse();

        if let Some(pointer_pos) = response.interact_pointer_pos(){
            let canvas_pos = from_screen * pointer_pos;
            self.points.push(canvas_pos);
            response.mark_changed();
        }

        painter.extend(self.points.iter().map(|p| {
            egui::Shape::circle_stroke(to_screen * *p, 5.0, self.stroke)
        }).collect());

        response
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

// ----------------------------------------------------------------------------
struct BackendPanel {
    open: bool,

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
