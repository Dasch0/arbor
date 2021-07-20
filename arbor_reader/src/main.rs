mod gfx;
mod text;
mod ui;
mod window;

use std::{io::Write, time::Duration};
use winit::event_loop;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

fn main() {
    // console output
    let mut stdout = std::io::stdout();

    // Window
    let (event_loop, window, mut window_state) =
        window::init("arbor_reader", INITIAL_WIDTH, INITIAL_HEIGHT);

    // Renderer
    let mut gfx_context = gfx::init(&window);

    // sprites
    let sprite_brush = gfx::Brush::new_sprite_brush(&gfx_context);
    let test_texture = gfx::Texture::from_bytes(
        &gfx_context,
        &sprite_brush,
        include_bytes!("../data/images/test.png"),
    )
    .expect("failed to load texture");

    let test_quad = gfx::Quad::from_test_vertices(&gfx_context);

    //let ui_rect = ui::Rect::from_tuple((400.0, 400.0, 200.0, 200.0));
    let ui_rect = ui::Rect::from_coords(400.0, 600.0, 400.0, 600.0);
    let mut ui_quad = ui_rect.to_quad(&gfx_context, window.inner_size());

    // text
    let mut text_renderer = text::Renderer::new(&gfx_context);

    let mut last_frame_duration = Duration::new(1, 0);

    event_loop.run(move |event, _, control_flow| {
        // set control flow to only update when explicitly called
        *control_flow = event_loop::ControlFlow::Wait;
        // This statement repeatedly calls update() until there are no more events
        if window_state.update(event) {
            return;
        }

        if window_state.quit {
            *control_flow = event_loop::ControlFlow::Exit;
        }

        if window_state.resize {
            gfx_context.resize(window_state.size);
            ui_quad = ui_rect.to_quad(&gfx_context, window.inner_size());
        }

        if window_state.rescale {
            std::unimplemented!();
        }

        let input = &window_state.input;

        //
        // after this point, window_state is now ready to be inspected
        //
        //

        // RENDER
        let (mut encoder, frame) = gfx::begin_frame(&gfx_context).unwrap();

        let mut renderpass = gfx::begin_renderpass(&mut encoder, &frame);
        gfx::draw_sprite(&mut renderpass, &sprite_brush, &test_texture, &test_quad);
        gfx::draw_sprite(&mut renderpass, &sprite_brush, &test_texture, &ui_quad);
        gfx::end_renderpass(renderpass);

        text_renderer.enqueue(
            text::styles::DIALOGUE,
            (10.0, 10.0),
            0.1,
            format!("\rframe_time: {:?}", last_frame_duration).as_str(),
        );
        text_renderer.enqueue(
            text::styles::DIALOGUE,
            (10.0, 20.0),
            0.1,
            format!("\rmouse_cursor: {:?}", input.cursor_position).as_str(),
        );

        if ui_rect.clicked(input) {
            text_renderer.enqueue(
                text::styles::TITLE,
                (ui_rect.x1 as f32, ui_rect.x2 as f32),
                0.1,
                "clicked!",
            );
        }
        text_renderer.enqueue(text::styles::TITLE, (100.0, 100.0), 0.0, "Dracula");
        text_renderer.enqueue(
            text::styles::DIALOGUE,
            (400.0, 400.0),
            0.0,
            "Enter of your own free will!",
        );
        text_renderer.draw(&mut gfx_context, &mut encoder, window_state.size, &frame);

        last_frame_duration = gfx::end_frame(&mut gfx_context, encoder, frame);
        stdout.flush().unwrap();
    });
}
