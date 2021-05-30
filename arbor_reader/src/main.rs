mod gfx;

use std::iter;
use std::time::Instant;

use winit::event::Event::*;
use winit::event_loop::ControlFlow;

const INITIAL_WIDTH: u32 = 1920;
const INITIAL_HEIGHT: u32 = 1080;
const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;

/// A custom event type for the winit app.
enum Event {
    RequestRedraw,
}

fn main() {
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

    // gpu
    let (gpu, surface) = gfx::Gpu::new(&window);

    // Swapchain
    let mut sc_desc = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: window.inner_size().width,
        height: window.inner_size().height,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    let mut swapchain = gpu.device.create_swap_chain(&surface, &sc_desc);

    let _sc_extent = wgpu::Extent3d {
        width: sc_desc.width,
        height: sc_desc.height,
        depth_or_array_layers: 1,
    };

    let start_time = Instant::now();
    let mut previous_frame_time = None;

    event_loop.run(move |event, _, control_flow| {
        match event {
            RedrawRequested(..) => {
                let output_frame = match swapchain.get_current_frame() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Dropped frame with error: {}", e);
                        return;
                    }
                };

                // Begin to draw the frame.
                let frame_start = Instant::now();

                // . . .

                let frame_time = (Instant::now() - frame_start).as_secs_f64() as f32;
                previous_frame_time = Some(frame_time);

                let mut encoder =
                    gpu.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("encoder"),
                        });

                // Upload all resources for the GPU.

                // Record all render passes.

                // Submit the commands.
                gpu.queue.submit(iter::once(encoder.finish()));
                *control_flow = ControlFlow::Poll;
            }

            MainEventsCleared | UserEvent(Event::RequestRedraw) => {
                window.request_redraw();
            }

            WindowEvent { event, .. } => match event {
                winit::event::WindowEvent::Resized(size) => {
                    sc_desc.width = size.width;
                    sc_desc.height = size.height;
                    swapchain = gpu.device.create_swap_chain(&surface, &sc_desc);
                }
                winit::event::WindowEvent::CloseRequested => {
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            _ => (),
        }
    });
}
