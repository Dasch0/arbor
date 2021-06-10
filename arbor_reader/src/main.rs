mod gfx;
mod static_init;

use anyhow::Result;
use std::io::Write;
use wgpu;
use wgpu_glyph::GlyphBrush;
use wgpu_glyph::{ab_glyph, GlyphBrushBuilder, Section, Text};
use winit::event::Event::*;
use winit::event_loop::ControlFlow;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;

/// A custom event type to force a redraw
enum Event {
    RequestRedraw,
}

fn init_test_text(
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
    gpu: &mut gfx::Gpu,
    encoder: &mut wgpu::CommandEncoder,
    glyph_brush: &mut GlyphBrush<wgpu::DepthStencilState>,
    size: (u32, u32),
    frame: &wgpu::SwapChainFrame,
    depth_view: &wgpu::TextureView,
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
            &gpu.device,
            &mut gpu.staging_belt,
            encoder,
            &frame.output.view,
            wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(-1.0),
                    store: true,
                }),
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: true,
                }),
            },
            size.0,
            size.1,
        )
        .expect("Draw queued");
}

fn main() {
    // console output
    let mut stdout = std::io::stdout();

    // Window
    let event_loop = winit::event_loop::EventLoop::with_user_event();
    let window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_transparent(false)
        .with_title("arbor")
        .with_inner_size(winit::dpi::PhysicalSize {
            width: INITIAL_WIDTH,
            height: INITIAL_HEIGHT,
        })
        .build(&event_loop)
        .unwrap();
    let size = window.inner_size();

    // gpu
    let (mut gpu, surface) = gfx::Gpu::new(&window);
    let (mut frame_descriptor, mut swapchain, mut frame_depth_view) =
        gfx::create_swapchain(&gpu.device, &surface, size);

    // sprites
    let sprite_brush = gfx::Brush::new_sprite_brush(&gpu);
    let test_texture = gfx::Texture::from_bytes(
        &gpu,
        &sprite_brush,
        include_bytes!("../data/images/vamp.png"),
    )
    .expect("failed to load texture");

    let test_quad = gfx::Quad::from_test_vertices(&gpu);

    // text
    let mut glyph_brush = init_test_text(&gpu.device).unwrap();

    event_loop.run(move |event, _, control_flow| match event {
        RedrawRequested(..) => {
            let frame = match swapchain.get_current_frame() {
                Ok(frame) => frame,
                Err(e) => {
                    eprintln!("Dropped frame with error: {}", e);
                    return;
                }
            };

            let (frame_start, mut encoder) = gfx::begin_frame(&gpu);
            let mut renderpass =
                gfx::begin_renderpass(&mut encoder, &frame.output.view, &frame_depth_view);

            gfx::draw_sprite(&mut renderpass, &sprite_brush, &test_texture, &test_quad);
            gfx::end_renderpass(renderpass);

            draw_test_text(
                &mut gpu,
                &mut encoder,
                &mut glyph_brush,
                (frame_descriptor.width, frame_descriptor.height),
                &frame,
                &frame_depth_view,
            );

            let frame_duration = gfx::end_frame(&mut gpu, encoder, frame_start);

            print!("\rframe_time: {:?}", frame_duration);
            stdout.flush().unwrap();
        }

        MainEventsCleared | UserEvent(Event::RequestRedraw) => {
            window.request_redraw();
        }

        WindowEvent { event, .. } => match event {
            winit::event::WindowEvent::Resized(size) => {
                let (new_frame_descriptor, new_swapchain, new_frame_depth_view) =
                    gfx::create_swapchain(&gpu.device, &surface, size);

                frame_descriptor = new_frame_descriptor;
                swapchain = new_swapchain;
                frame_depth_view = new_frame_depth_view;
            }
            winit::event::WindowEvent::CloseRequested => {
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        },
        _ => (),
    });
}
