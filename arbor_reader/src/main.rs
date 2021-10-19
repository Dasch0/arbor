#![allow(dead_code)]
mod gfx;
mod text;
mod ui;
mod window;

use arbor_core;
use arbor_core::{cmd, Executable};
use std::{io::Write, time::Duration};
use text::styles;
use winit::event_loop;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

pub enum States {
    EditorStart,
    EditorLoop,
    GameStart,
    GameLoop,
    Load,
    Options,
    Quit,
    TitleScreen,
}

/// Data for editor mode
pub struct Editor {
    pub current_node_id: usize,
    pub prev_node_id: usize,
    pub script: arbor_core::EditorState,
    pub name_buf: String,
    pub text_buf: String,
    pub history: text::input::History,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            current_node_id: 0,
            prev_node_id: 0,
            script: arbor_core::EditorState::new(arbor_core::DialogueTreeData::default()),
            name_buf: String::with_capacity(128),
            text_buf: String::with_capacity(4096),
            history: text::input::History::with_capacity(4096),
        }
    }
}

impl Editor {
    /// Draws interface for creating a new editor instance
    pub fn draw_new(
        &mut self,
        ws: &mut window::State,
        _renderpass: &mut gfx::RenderPass,
        txt: &mut text::Renderer,
    ) -> States {
        // Display popup for user to enter name of new project
        let menu_pt = gfx::Point::new(ws.size.width as f64 * 0.5, ws.size.height as f64 * 0.3, 0.0);

        // parse the text, ignore separators and no undo allowed
        text::input::parse_single_no_history(&mut self.text_buf, ws.input.chars());

        // FIXME: automate cursor drawing
        self.text_buf.push('|');

        let title = txt.enqueue(styles::TITLE, menu_pt, self.text_buf.as_str());
        let subtitle = txt.enqueue(
            styles::SUBTITLE,
            title.pt() + styles::TITLE.inc(),
            "Enter the name of the new script",
        );

        // FIXME: automate cursor drawing
        self.text_buf.pop();

        // Draw buttons
        let start = txt.enqueue(styles::MENU, subtitle.pt() + styles::TITLE.inc(), "Start");
        let cancel = txt.enqueue(styles::MENU, start.pt() + styles::MENU.inc(), "Cancel");

        // FIXME: ugly
        if start.clicked(&ws.input) {
            let res = arbor_core::cmd::new::Project::new(self.text_buf.drain(..).collect(), true)
                .execute(&mut self.script);
            if let Err(e) = res {
                log::error!("{}", e);
            }
            States::EditorLoop
        } else if cancel.clicked(&ws.input) {
            States::TitleScreen
        } else {
            States::EditorStart
        }
    }

    /// Draws the primary interface for the editor
    pub fn draw(
        &mut self,
        ws: &mut window::State,
        _renderpass: &mut gfx::RenderPass,
        txt: &mut text::Renderer,
    ) -> States {
        // ui anchor points and bounds
        let dialogue_pt =
            gfx::Point::new(ws.size.width as f64 * 0.3, ws.size.height as f64 * 0.3, 0.0);
        let dialogue_bounds = (ws.size.width as f32 * 0.6, ws.size.height as f32 * 0.2);
        let name_pt = gfx::Point::new(ws.size.width as f64 * 0.2, ws.size.height as f64 * 0.2, 0.0);
        let save_pt = gfx::Point::new(ws.size.width as f64 * 0.8, ws.size.height as f64 * 0.8, 0.0);

        // ui elements
        let name_rect = txt.enqueue(styles::MENU, name_pt, self.name_buf.as_str());
        let save_rect = txt.enqueue(styles::MENU, save_pt, "Save");
        let next_rect = txt.enqueue(styles::MENU, save_rect.pt() + styles::MENU.inc(), "Next");

        // send input to parser (input text will be cleared next frame)
        text::input::parse(&mut self.text_buf, ws.input.chars(), &mut self.history);
        self.text_buf.push('|');
        let dialogue_rect = txt.enqueue_with_bounds(
            styles::DIALOGUE,
            dialogue_pt,
            dialogue_bounds,
            self.text_buf.as_str(),
        );
        self.text_buf.pop();

        // app logic
        if save_rect.clicked(&ws.input) {
            let res = cmd::new::Node::new(
                self.name_buf.drain(..).collect(),
                self.text_buf.drain(..).collect(),
            )
            .execute(&mut self.script);
            match res {
                Ok(node_index) => {
                    self.current_node_id = node_index;
                }
                Err(e) => log::error!("{}", e),
            }
        }

        States::EditorLoop
    }
}

/// Data for reader mode
pub struct ReaderData {}

fn main() {
    // console output
    let mut stdout = std::io::stdout();

    // Window
    let (event_loop, window, mut ws) = window::init("arbor_reader", INITIAL_WIDTH, INITIAL_HEIGHT);

    // Renderer
    let mut gfx_context = gfx::init(&window);

    // sprites
    //let sprite_brush = gfx::Brush::new_sprite_brush(&gfx_context);
    //let test_texture = gfx::Texture::from_bytes(
    //    &gfx_context,
    //    &sprite_brush,
    //    include_bytes!("../data/images/test.png"),
    //)
    //.expect("failed to load texture");

    //let test_quad = gfx::Quad::from_test_vertices(&gfx_context);

    // UI
    //let ui_rect = ui::Rect::from_coords(400.0, 600.0, 400.0, 600.0);
    //let mut ui_quad = ui_rect.to_quad(&gfx_context, window.inner_size());

    // text
    let mut txt = text::Renderer::new(&gfx_context);
    let mut last_frame_duration = Duration::new(1, 0);

    // App data
    // always starts at title screen
    let mut state = States::TitleScreen;
    let mut editor = Editor::default();

    event_loop.run(move |event, _, control_flow| {
        // set control flow to only update when explicitly called
        *control_flow = event_loop::ControlFlow::Wait;
        // This statement repeatedly calls update() until there are no more events
        if ws.update(event) {
            return;
        }

        if ws.quit {
            *control_flow = event_loop::ControlFlow::Exit;
        }

        if ws.resize {
            gfx_context.resize(ws.size);
            //ui_quad = ui_rect.to_quad(&gfx_context, window.inner_size());
        }

        if ws.rescale {
            std::unimplemented!();
        }

        //
        // after this point, window_state is now ready to be inspected
        //

        let (mut encoder, frame) = gfx::begin_frame(&gfx_context).unwrap();
        let mut renderpass = gfx::begin_renderpass(&mut encoder, &frame);

        // top level state machine
        state = match state {
            States::TitleScreen => draw_title_menu(&ws, &mut renderpass, &mut txt),
            States::GameLoop => States::GameLoop,
            States::GameStart => States::GameStart,
            States::EditorLoop => editor.draw(&mut ws, &mut renderpass, &mut txt),
            States::EditorStart => editor.draw_new(&mut ws, &mut renderpass, &mut txt),
            States::Load => States::Load,
            States::Options => States::Options,
            States::Quit => quit(control_flow),
        };

        draw_performance_metrics(&mut txt, (last_frame_duration, ws.input.cursor_position));

        // Draw everything and end frame
        gfx::end_renderpass(renderpass);
        txt.draw(&mut gfx_context, &mut encoder, ws.size, &frame);
        last_frame_duration = gfx::end_frame(&mut gfx_context, encoder, frame);
        stdout.flush().unwrap();
    });
}

pub fn draw_title_menu(
    ws: &window::State,
    _renderpass: &mut gfx::RenderPass,
    txt: &mut text::Renderer,
) -> States {
    let title_pt = gfx::Point::new(ws.size.width as f64 * 0.5, ws.size.height as f64 * 0.1, 0.0);
    let menu_pt = gfx::Point::new(ws.size.width as f64 * 0.5, ws.size.height as f64 * 0.3, 0.0);

    txt.enqueue(styles::TITLE, title_pt, "Dracula");
    txt.enqueue(
        styles::SUBTITLE,
        title_pt + styles::TITLE.inc(),
        "Enter of your own free will!",
    );

    let new = txt.enqueue(styles::MENU, menu_pt, "New Empty Script");
    let load = txt.enqueue(styles::MENU, new.pt() + styles::MENU.inc(), "Load Script");
    let options = txt.enqueue(styles::MENU, load.pt() + styles::MENU.inc(), "Options");
    let quit = txt.enqueue(styles::MENU, options.pt() + styles::MENU.inc(), "Quit");

    // FIXME: ugly
    if new.clicked(&ws.input) {
        States::EditorStart
    } else if load.clicked(&ws.input) {
        States::Load
    } else if options.clicked(&ws.input) {
        States::Options
    } else if quit.clicked(&ws.input) {
        States::Quit
    } else {
        States::TitleScreen
    }
}

// TODO: Create metrics struct
pub fn draw_performance_metrics(
    text_renderer: &mut text::Renderer,
    metrics: (std::time::Duration, window::Position),
) {
    let info_pt = gfx::Point::new(10.0, 10.0, 0.0);
    let line_height = gfx::Point::new(0.0, 24.0, 0.0);

    text_renderer.enqueue(
        styles::METRIC,
        info_pt,
        format!("\rframe_time: {:?}", metrics.0).as_str(),
    );
    text_renderer.enqueue(
        styles::METRIC,
        info_pt + line_height,
        format!("\rmouse_cursor: {:?}", metrics.1).as_str(),
    );
}

pub fn draw_button(rect: ui::Rect, renderpass: gfx::RenderPass) {}

/// handles quit request
pub fn quit(control_flow: &mut event_loop::ControlFlow) -> States {
    *control_flow = event_loop::ControlFlow::Exit;
    States::Quit
}
