use crate::window;
use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use futures::task::SpawnExt;
use std::time::{Duration, Instant};
use std::{file, mem};
use wgpu::util::DeviceExt;
use winit::window::Window;

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const PRESENT_MODE: wgpu::PresentMode = wgpu::PresentMode::Mailbox;

/// Handle to core WGPU stuctures representing physical access to the gpu, such as the device,
/// queue, and swapchain.
pub struct Context {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface,
    pub swap_chain: wgpu::SwapChain,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub staging_belt: wgpu::util::StagingBelt,
    pub thread_pool: futures::executor::LocalPool,
    pub thread_spawner: futures::executor::LocalSpawner,
}

impl Context {
    /// Resize the GPU to target a new swapchain size
    pub fn resize(&mut self, size: window::PhysicalSize<u32>) {
        let frame_descriptor = wgpu::SwapChainDescriptor {
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
            format: OUTPUT_FORMAT,
            width: size.width,
            height: size.height,
            present_mode: PRESENT_MODE,
        };
        self.swap_chain = self
            .device
            .create_swap_chain(&self.surface, &frame_descriptor);

        log::trace!("Create swapchain depth textures");
        self.depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth buffer"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        });
    }
}

/// Storage for per-frame gpu information
pub struct Frame {
    pub data: wgpu::SwapChainFrame,
    pub depth_view: wgpu::TextureView,
    pub start_time: Instant,
}

impl Frame {
    /// Concisely return a reference to the Frame TextureView
    pub fn view(&self) -> &wgpu::TextureView {
        &self.data.output.view
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
/// Vertex primitive type for Gfx
pub struct Vertex {
    pub pos: [f32; 4],
    pub tex_coord: [f32; 2],
}
impl Vertex {
    /// Helper method for concisely creating new vertices
    pub fn new(pos: [f32; 3], tc: [f32; 2]) -> Vertex {
        Vertex {
            pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32, 1.0],
            tex_coord: [tc[0] as f32, tc[1] as f32],
        }
    }

    /// Get a description of the vertex layout
    pub fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::InputStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
            ],
        }
    }
}

/// A Brush stores data for drawing multiple entities that share the same shader and binding
/// layout. Currently the brush requires all bindings to be stored in a single binding group
pub struct Brush {
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
}

/// Default brush creation starts with a brush in the init stage
impl Brush {
    /// Creates a brush preset for drawing simple sprites
    pub fn new_sprite_brush(context: &Context) -> Self {
        // hardcoded parameters used for sprite_brush preset
        let texture_format = OUTPUT_FORMAT;
        let vertex_shader = wgpu::include_spirv!("../data/shaders/sprite.vert.spv");
        let fragment_shader = wgpu::include_spirv!("../data/shaders/sprite.frag.spv");

        // Create shader modules
        let vertex_shader_module = context.device.create_shader_module(&vertex_shader);
        let fragment_shader_module = context.device.create_shader_module(&fragment_shader);

        // Create the texture layout for further usage.
        let bind_group_layout =
            context
                .device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("imgui-wgpu bind group layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStage::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                multisampled: false,
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStage::FRAGMENT,
                            ty: wgpu::BindingType::Sampler {
                                comparison: false,
                                filtering: true,
                            },
                            count: None,
                        },
                    ],
                });

        // Create the render pipeline layout.
        let pipeline_layout =
            context
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("imgui-wgpu pipeline layout"),
                    bind_group_layouts: &[&bind_group_layout],
                    push_constant_ranges: &[],
                });

        // Create the render pipeline.
        let pipeline = context
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("imgui-wgpu pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &vertex_shader_module,
                    entry_point: "main",
                    buffers: &[Vertex::desc()],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    clamp_depth: false,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &fragment_shader_module,
                    entry_point: "main",
                    targets: &[wgpu::ColorTargetState {
                        format: texture_format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrite::ALL,
                    }],
                }),
            });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
}

/// Dummy structs used to restrict the draw methods available in different states.
mod draw_stage {
    /// Initial stage where a renderpass has not been started
    pub struct Init;
    /// A renderpass has started, multiple entities may be drawn in the same renderpass
    pub struct Draw;
    /// Drawing is completed and the renderpass is ready to be submitted to the gpu
    pub struct Finish;
}

/// A gfx::Texture stores the underlying texture as well as a quad, sampler, and bind_group to draw
/// to
pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
}

impl Texture {
    pub fn from_bytes(context: &Context, brush: &Brush, bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let (info, mut reader) = decoder.read_info().unwrap();
        let mut buf = vec![0; info.buffer_size()];
        reader.next_frame(&mut buf).unwrap();

        let size = wgpu::Extent3d {
            width: info.width,
            height: info.height,
            depth_or_array_layers: 1,
        };
        let texture = context.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });

        context.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &buf,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(4 * info.width),
                rows_per_image: std::num::NonZeroU32::new(info.height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = context.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &brush.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
                label: None,
            });

        Ok(Self {
            texture,
            view,
            bind_group,
            sampler,
        })
    }
}

/// Stores data for a single resizable quad
pub struct Quad {
    pub vertex_buffer: wgpu::Buffer,
    pub num_verts: u32,
}

impl Quad {
    /// Create a quad using hardcoded test vertices
    pub fn from_test_vertices(context: &Context) -> Self {
        let num_verts = 4;
        let verts = [
            Vertex::new([-0.5, -0.5, 0.0], [0.0, 1.0]),
            Vertex::new([0.5, -0.5, 0.0], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.0], [0.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.0], [1.0, 0.0]),
        ];

        let vertex_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsage::VERTEX,
            });

        Self {
            vertex_buffer,
            num_verts,
        }
    }
}

/// Wraps the async init function with blocking call
pub fn init(window: &Window) -> Context {
    futures::executor::block_on(initialize_gfx(window))
}

/// Creates the GPU handle and returns the window surface used
async fn initialize_gfx(window: &Window) -> Context {
    log::info!("Initializing gfx...");
    log::trace!("Create instance");
    let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);

    log::trace!("Obtain window surface");
    let surface = unsafe { instance.create_surface(window) };

    log::trace!("Create adapter");
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
        })
        .await
        .unwrap();

    let optional_features = wgpu::Features::empty();
    let required_features = wgpu::Features::default();
    let adapter_features = adapter.features();
    let required_limits = wgpu::Limits::default();
    let trace_dir = std::env::var("WGPU_TRACE");

    log::trace!("Create device & queue");
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: (adapter_features & optional_features) | required_features,
                limits: required_limits,
            },
            trace_dir.ok().as_ref().map(std::path::Path::new),
        )
        .await
        .unwrap();

    log::trace!("Create swapchain");
    let size = window.inner_size();

    let frame_descriptor = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        format: OUTPUT_FORMAT,
        width: size.width,
        height: size.height,
        present_mode: PRESENT_MODE,
    };
    let swap_chain = device.create_swap_chain(&surface, &frame_descriptor);

    log::trace!("Create swapchain depth textures");
    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth buffer"),
        size: wgpu::Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
    });

    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    log::trace!("Create staging belt and thread pool utilities");
    let staging_belt = wgpu::util::StagingBelt::new(1024);
    let thread_pool = futures::executor::LocalPool::new();
    let thread_spawner = thread_pool.spawner();

    log::info!("Gfx initialization complete!");

    Context {
        instance,
        adapter,
        device,
        queue,
        surface,
        swap_chain,
        depth_texture,
        depth_view,
        staging_belt,
        thread_pool,
        thread_spawner,
    }
}

/// Called to begin a new frame to be drawn to
///
/// # Error
/// If the frame cannot be started (usually due to a failure to get the next frame from the
/// swapchain). An error will be returned. Note that this should be handled gracefully, as
/// framedrops will likely occur at some point
pub fn begin_frame(context: &Context) -> anyhow::Result<(wgpu::CommandEncoder, Frame)> {
    // Begin to draw the frame.
    let data = context.swap_chain.get_current_frame()?;

    // start counting time the moment we have the frame
    let frame_start = Instant::now();

    let depth_view = context
        .depth_texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("text_pass_encoder"),
        });

    Ok((
        encoder,
        Frame {
            start_time: frame_start,
            data,
            depth_view,
        },
    ))
}

/// Start recording a renderpass on a given render target. Returns a command encoder to use for
/// draw calls
pub fn begin_renderpass<'render>(
    encoder: &'render mut wgpu::CommandEncoder,
    frame: &'render Frame,
) -> wgpu::RenderPass<'render> {
    // Clear frame
    let renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Render pass"),
        color_attachments: &[wgpu::RenderPassColorAttachment {
            view: frame.view(),
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: 0.4,
                    g: 0.4,
                    b: 0.4,
                    a: 1.0,
                }),
                store: true,
            },
        }],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &frame.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: true,
            }),
            stencil_ops: None,
        }),
    });

    renderpass
}

/// Draw a sprite (2d texture sampled to a quad) to an in-progress render pass using a defined
/// shader/pipeline
pub fn draw_sprite<'render>(
    renderpass: &mut wgpu::RenderPass<'render>,
    brush: &'render Brush,
    texture: &'render Texture,
    quad: &'render Quad,
) {
    renderpass.set_pipeline(&brush.pipeline);
    renderpass.set_bind_group(0, &texture.bind_group, &[]);
    renderpass.set_vertex_buffer(0, quad.vertex_buffer.slice(..));
    renderpass.draw(0..quad.num_verts, 0..1);
}

pub fn end_renderpass<'render>(renderpass: wgpu::RenderPass<'render>) {
    drop(renderpass);
}

pub fn end_frame<'render>(
    context: &mut Context,
    encoder: wgpu::CommandEncoder,
    frame: Frame,
) -> Duration {
    // Submit the commands.
    context.staging_belt.finish();
    context.queue.submit(std::iter::once(encoder.finish()));

    // end frame draw
    let belt_future = context.staging_belt.recall();
    context.thread_spawner.spawn(belt_future).unwrap();

    Instant::now() - frame.start_time
}
