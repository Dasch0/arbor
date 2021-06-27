mod gfx;
mod ui;
mod window;

use anyhow::Result;
use std::io::Write;
use wgpu;
use wgpu_glyph::GlyphBrush;
use wgpu_glyph::{ab_glyph, GlyphBrushBuilder, Section, Text};
use winit::event_loop::{self, ControlFlow};

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

pub fn init_test_text(
    device: &wgpu::Device,
) -> Result<wgpu_glyph::GlyphBrush<wgpu::DepthStencilState>> {
    // Prepare glyph_brush
    let font_data =
        ab_glyph::FontArc::try_from_slice(include_bytes!("../data/fonts/Lora-Regular.ttf"))?;

    let glyph_brush = GlyphBrushBuilder::using_font(font_data)
        .depth_stencil_state(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        })
        .build(&device, gfx::OUTPUT_FORMAT);

    Ok(glyph_brush)
}

fn draw_test_text(
    context: &mut gfx::Context,
    encoder: &mut wgpu::CommandEncoder,
    frame: &gfx::Frame,
    glyph_brush: &mut GlyphBrush<wgpu::DepthStencilState>,
    size: winit::dpi::PhysicalSize<u32>,
) {
    // Queue text on top, it will be drawn first.
    // Depth buffer will make it appear on top.
    glyph_brush.queue(Section {
        screen_position: (400.0, 400.0),
        text: vec![Text::default()
            .with_text("Enter freely & of your own will!")
            .with_scale(95.0)
            .with_color([0.8, 0.8, 0.8, 1.0])
            .with_z(0.9)],
        ..Section::default()
    });

    // Draw all the text!
    glyph_brush
        .draw_queued(
            &context.device,
            &mut context.staging_belt,
            encoder,
            frame.view(),
            wgpu::RenderPassDepthStencilAttachment {
                view: &frame.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(-1.0),
                    store: true,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: true,
                }),
            },
            size.width,
            size.height,
        )
        .expect("Draw queued");
}

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

    // text
    let mut glyph_brush = init_test_text(&gfx_context.device).unwrap();

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
            gfx_context.resize(window_state.size)
        }

        if window_state.rescale {
            std::unimplemented!();
        }

        if window_state.input.cursor_pressed() {
            println!("click!");
        }

        // after this point, window_state is now ready to be inspected
        let (mut encoder, frame) = gfx::begin_frame(&gfx_context).unwrap();
        let mut renderpass = gfx::begin_renderpass(&mut encoder, &frame);

        gfx::draw_sprite(&mut renderpass, &sprite_brush, &test_texture, &test_quad);
        gfx::end_renderpass(renderpass);

        draw_test_text(
            &mut gfx_context,
            &mut encoder,
            &frame,
            &mut glyph_brush,
            window_state.size,
        );

        let frame_duration = gfx::end_frame(&mut gfx_context, encoder, frame);

        print!("\rframe_time: {:?}", frame_duration);
        stdout.flush().unwrap();
    });
}
