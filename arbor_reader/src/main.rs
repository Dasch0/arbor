#![allow(dead_code)]
mod gfx;
mod text;
mod ui;
mod window;

use arbor_core;
use arbor_core::editor::{self, Editor};
use log::error;
use std::{io::Write, time::Duration};
use text::input::History;
use text::styles;
use winit::event_loop;
use winit::platform::run_return::EventLoopExtRunReturn;

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
pub struct EditorUiData {
    pub current_node_id: usize,
    pub prev_node_id: usize,
    pub text_buf: String,
    pub editor: Option<Editor>,
    pub input_history: History,
}

impl Default for EditorUiData {
    /// Create a new EditState instance
    fn default() -> Self {
        Self {
            current_node_id: 0,
            prev_node_id: 0,
            text_buf: String::with_capacity(4096),
            editor: None,
            input_history: History::with_capacity(4096),
        }
    }
}

/// Draws interface for creating a new editor instance
pub fn draw_new_editor_menu(
    ui_data: &mut EditorUiData,
    ws: &mut window::State,
    txt: &mut text::Writer,
) -> States {
    // Display popup for user to enter name of new project
    let menu_pt = gfx::Point::new(ws.size.width as f64 * 0.5, ws.size.height as f64 * 0.3, 0.0);

    // parse the text, ignore separators and no undo allowed
    text::input::parse_single_no_history(&mut ui_data.text_buf, ws.input.chars());

    // FIXME: automate cursor drawing
    ui_data.text_buf.push('|');

    let title = txt.enqueue(styles::TITLE, menu_pt, ui_data.text_buf.as_str());
    let subtitle = txt.enqueue(
        styles::SUBTITLE,
        title.pt() + styles::TITLE.inc(),
        "Enter the name of the new script",
    );

    // FIXME: automate cursor drawing
    ui_data.text_buf.pop();

    // Draw buttons
    let start = txt.enqueue(styles::MENU, subtitle.pt() + styles::TITLE.inc(), "Start");
    let cancel = txt.enqueue(styles::MENU, start.pt() + styles::MENU.inc(), "Cancel");

    // FIXME: ugly
    if start.clicked(&ws.input) {
        let res = editor::Editor::new(&ui_data.text_buf, None);
        match res {
            Ok(editor) => {
                ui_data.text_buf.clear();
                ui_data.editor = Some(editor);
                States::EditorLoop
            }
            Err(e) => {
                log::error!("{}", e);
                States::EditorStart
            }
        }
    } else if cancel.clicked(&ws.input) {
        States::TitleScreen
    } else {
        States::EditorStart
    }
}

/// Draws node editor interface
pub fn draw_node_editor<'r, 'rpass>(
    ui_data: &mut EditorUiData,
    ws: &mut window::State,
    renderer: &'r gfx::Renderer,
    renderpass: &'rpass mut gfx::RenderPass<'r>,
    txt: &mut text::Writer,
    ctx: &gfx::Context,
) -> States {
    //
    // error and illegal state checks
    //
    let editor = match ui_data.editor {
        Some(ref mut e) => e,
        None => {
            error!("Somehow reached the node editor view without an initialized editor for the underlying text. Returning to new editor menu");
            return States::EditorStart;
        }
    };

    //
    // static UI elements
    //
    let dialogue_pt = gfx::Point::new(ws.size.width as f64 * 0.3, ws.size.height as f64 * 0.3, 0.0);
    let dialogue_bounds = (ws.size.width as f32 * 0.6, ws.size.height as f32 * 0.2);
    let name_pt = gfx::Point::new(ws.size.width as f64 * 0.2, ws.size.height as f64 * 0.2, 0.0);
    let menu_pt = gfx::Point::new(ws.size.width as f64 * 0.8, ws.size.height as f64 * 0.8, 0.0);

    let _name_rect = txt.enqueue(styles::MENU, name_pt, &ui_data.text_buf);
    let save_rect = txt.enqueue(styles::MENU, menu_pt, "Save");
    let _new_choice_rect = txt.enqueue(
        styles::MENU,
        save_rect.pt() + styles::MENU.inc(),
        "Add Choice",
    );
    renderer.draw_quad(renderpass, ctx);

    //
    // dynamic UI elements
    //
    // Parse input (input text will be cleared next frame)
    text::input::parse(
        &mut ui_data.text_buf,
        ws.input.chars(),
        &mut ui_data.input_history,
    );
    // FIXME: cleanup cursor drawing
    ui_data.text_buf.push('|');

    let _dialogue_rect = txt.enqueue_with_bounds(
        styles::DIALOGUE,
        dialogue_pt,
        dialogue_bounds,
        ui_data.text_buf.as_str(),
    );

    // FIXME: cleanup cursor drawing
    ui_data.text_buf.pop();

    //
    // app logic
    //
    if save_rect.clicked(&ws.input) {
        let res = editor.new_node(&ui_data.text_buf, &ui_data.text_buf);
        match res {
            Ok(node_index) => {
                ui_data.current_node_id = node_index;
                ui_data.text_buf.clear();
            }
            Err(e) => log::error!("{}", e),
        }
    }

    States::EditorLoop
}

/// Data for reader mode
pub struct ReaderData {}

fn main() {
    // logging
    env_logger::init();
    // console output
    let mut stdout = std::io::stdout();

    // Window
    let (mut event_loop, window, mut ws) =
        window::init("arbor_reader", INITIAL_WIDTH, INITIAL_HEIGHT);

    // Renderer
    let mut ctx = gfx::init(&window);
    let mut renderer = gfx::Renderer::default(&ctx);

    // text
    let mut txt = text::Writer::new();

    // App data
    // always starts at title screen
    let mut state = States::TitleScreen;
    let mut editor_ui_data = EditorUiData::default();
    let mut last_frame_duration = Duration::new(1, 0);

    let mut bquit = false;

    while !bquit {
        event_loop.run_return(|event, _, control_flow| {
            // set control flow to only update when explicitly called
            *control_flow = event_loop::ControlFlow::Wait;
            // This statement repeatedly calls update() until there are no more events
            if ws.update(event) {
                return;
            }

            if ws.quit {
                bquit = true;
                *control_flow = event_loop::ControlFlow::Exit;
            }

            if ws.resize {
                ctx.resize(ws.size);
            }

            if ws.rescale {
                std::unimplemented!();
            }

            //
            // after this point, window_state is now ready to be inspected
            //

            let (mut default_encoder, frame) = gfx::begin_frame(&ctx).unwrap();
            let mut renderpass = gfx::begin_renderpass(&mut default_encoder, &frame);

            // top level state machine
            state = match state {
                States::TitleScreen => {
                    draw_title_menu(&mut ws, &mut renderer, &mut renderpass, &mut txt, &ctx)
                }
                States::GameLoop => States::GameLoop,
                States::GameStart => States::GameStart,
                States::EditorLoop => draw_node_editor(
                    &mut editor_ui_data,
                    &mut ws,
                    &mut renderer,
                    &mut renderpass,
                    &mut txt,
                    &ctx,
                ),
                States::EditorStart => draw_new_editor_menu(&mut editor_ui_data, &mut ws, &mut txt),
                States::Load => States::Load,
                States::Options => States::Options,
                States::Quit => {
                    bquit = true;
                    quit(control_flow)
                }
            };

            draw_performance_metrics(&mut txt, (last_frame_duration, ws.input.cursor_position));

            // Finalize renderpasses and end frame
            gfx::end_renderpass(renderpass);
            let default_command_buffer = default_encoder.finish();

            let glyph_command_buffer =
                renderer.render_glyphs(&mut txt.glyph_brush, None, &frame, &ctx);

            let command_buffers = [default_command_buffer, glyph_command_buffer]; //, glyph_command_buffer];
            last_frame_duration = gfx::end_frame(command_buffers, frame, &mut ctx);

            stdout.flush().unwrap();
        });
    }
}

pub fn draw_title_menu<'r, 'rpass>(
    ws: &window::State,
    renderer: &'r gfx::Renderer,
    renderpass: &'rpass mut gfx::RenderPass<'r>,
    txt: &mut text::Writer,
    ctx: &gfx::Context,
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

    renderer.draw_quad(renderpass, ctx);
    renderer.draw_triangle(renderpass, ctx);

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
    txt: &mut text::Writer,
    metrics: (std::time::Duration, window::Position),
) {
    let info_pt = gfx::Point::new(10.0, 10.0, 0.0);
    let line_height = gfx::Point::new(0.0, 24.0, 0.0);

    txt.enqueue(
        styles::METRIC,
        info_pt,
        format!("\rframe_time: {:?}", metrics.0).as_str(),
    );
    txt.enqueue(
        styles::METRIC,
        info_pt + line_height,
        format!("\rmouse_cursor: {:?}", metrics.1).as_str(),
    );
}

/// draw a clickable button, appearance can change based on hover/clicked status
pub fn draw_button(
    position: gfx::Point,
    text: &str,
    txt: &mut text::Writer,
    _input: &window::Input,
) {
    let _rect = txt.enqueue(styles::BUTTON, position, text);
}

/// handles quit request
pub fn quit(control_flow: &mut event_loop::ControlFlow) -> States {
    *control_flow = event_loop::ControlFlow::Exit;
    States::Quit
}
