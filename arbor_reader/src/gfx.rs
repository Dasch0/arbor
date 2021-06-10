use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use futures::task::SpawnExt;
use std::time::{Duration, Instant};
use std::{file, io, mem, path};
use wgpu::util::DeviceExt;
use winit::window::Window;

pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Handle to core WGPU stuctures representing physical access to the gpu, such as the device and
/// queue. Within the limitations of this renderer, all constructs stored in the Gpu are
/// essentially singletons (for instance, generally there will only be a single device).
///
/// While things like multiple queues, pools, or staging belts are possible, within the limited use
/// case of the gfx renderer only a single instance of each is needed.
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub staging_belt: wgpu::util::StagingBelt,
    pub thread_pool: futures::executor::LocalPool,
    pub thread_spawner: futures::executor::LocalSpawner,
}

impl Gpu {
    //TODO:: Support configurable features/limitations
    /// Creates the GPU handle and returns the window surface used
    pub async fn init(window: &Window) -> (Gpu, wgpu::Surface) {
        log::info!("Initializing instance...");
        let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);

        log::info!("Obtaining window surface...");
        let surface = unsafe { instance.create_surface(window) };

        log::info!("Initializing adapter...");
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

        log::info!("Initializing device & queue...");
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

        log::info!("Create staging belt and thread pool utilities");
        let staging_belt = wgpu::util::StagingBelt::new(1024);
        let thread_pool = futures::executor::LocalPool::new();
        let thread_spawner = thread_pool.spawner();

        log::info!("Setup complete!");

        (
            Gpu {
                instance,
                adapter,
                device,
                queue,
                staging_belt,
                thread_pool,
                thread_spawner,
            },
            surface,
        )
    }

    /// Wraps the async init function with blocking call
    pub fn new(window: &Window) -> (Gpu, wgpu::Surface) {
        futures::executor::block_on(Gpu::init(window))
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    _pos: [f32; 4],
    _tex_coord: [f32; 2],
}
impl Vertex {
    pub fn new(pos: [f32; 3], tc: [f32; 2]) -> Vertex {
        Vertex {
            _pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32, 1.0],
            _tex_coord: [tc[0] as f32, tc[1] as f32],
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

#[derive(Debug)]
pub struct BufferDimensions {
    pub width: usize,
    pub height: usize,
    pub depth: usize,
    pub unpadded_bytes_per_row: usize,
    pub padded_bytes_per_row: usize,
    pub bytes_per_pixel: usize,
}

impl BufferDimensions {
    pub fn new(width: usize, height: usize, depth: usize) -> Self {
        let bytes_per_pixel = std::mem::size_of::<u32>();
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded_bytes_per_row_padding = (align - unpadded_bytes_per_row % align) % align;
        let padded_bytes_per_row = unpadded_bytes_per_row + padded_bytes_per_row_padding;
        Self {
            width,
            height,
            depth,
            unpadded_bytes_per_row,
            padded_bytes_per_row,
            bytes_per_pixel,
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
    pub fn new_sprite_brush(gpu: &Gpu) -> Self {
        // hardcoded parameters used for sprite_brush preset
        let texture_format = OUTPUT_FORMAT;
        let vertex_shader = wgpu::include_spirv!("../data/shaders/sprite.vert.spv");
        let fragment_shader = wgpu::include_spirv!("../data/shaders/sprite.frag.spv");

        // Create shader modules
        let vertex_shader_module = gpu.device.create_shader_module(&vertex_shader);
        let fragment_shader_module = gpu.device.create_shader_module(&fragment_shader);

        // Create the texture layout for further usage.
        let bind_group_layout =
            gpu.device
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
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("imgui-wgpu pipeline layout"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });

        // Create the render pipeline.
        let pipeline = gpu
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
    pub fn from_bytes(gpu: &Gpu, brush: &Brush, bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let (info, mut reader) = decoder.read_info().unwrap();
        let mut buf = vec![0; info.buffer_size()];
        reader.next_frame(&mut buf).unwrap();

        let size = wgpu::Extent3d {
            width: info.width,
            height: info.height,
            depth_or_array_layers: 1,
        };
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });

        gpu.queue.write_texture(
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
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
    pub fn from_test_vertices(gpu: &Gpu) -> Self {
        let num_verts = 4;
        let verts = [
            Vertex::new([-0.5, -0.5, 0.0], [0.0, 1.0]),
            Vertex::new([0.5, -0.5, 0.0], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.0], [0.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.0], [1.0, 0.0]),
        ];

        let vertex_buffer = gpu
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

/// Initializes the swapchain frames and creates a single depth texture for all frames.
pub fn create_swapchain(
    device: &wgpu::Device,
    surface: &wgpu::Surface,
    size: winit::dpi::PhysicalSize<u32>,
) -> (
    wgpu::SwapChainDescriptor,
    wgpu::SwapChain,
    wgpu::TextureView,
) {
    let (width, height) = (size.width, size.height);

    let frame_descriptor = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        format: OUTPUT_FORMAT,
        width,
        height,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    let swap_chain = device.create_swap_chain(surface, &frame_descriptor);

    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Depth buffer"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
    });

    (
        frame_descriptor,
        swap_chain,
        depth_texture.create_view(&wgpu::TextureViewDescriptor::default()),
    )
}

pub fn begin_frame(gpu: &Gpu) -> (Instant, wgpu::CommandEncoder) {
    // Begin to draw the frame.
    let frame_start = Instant::now();

    let encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("text_pass_encoder"),
        });

    (frame_start, encoder)
}

/// Start recording a renderpass on a given render target. Returns a command encoder to use for
/// draw calls
pub fn begin_renderpass<'render>(
    encoder: &'render mut wgpu::CommandEncoder,
    render_target: &'render wgpu::TextureView,
    depth_view: &'render wgpu::TextureView,
) -> wgpu::RenderPass<'render> {
    // Clear frame
    let renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Render pass"),
        color_attachments: &[wgpu::RenderPassColorAttachment {
            view: render_target,
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
            view: depth_view,
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
    gpu: &mut Gpu,
    encoder: wgpu::CommandEncoder,
    frame_start_time: Instant,
) -> Duration {
    // Submit the commands.
    gpu.staging_belt.finish();
    gpu.queue.submit(std::iter::once(encoder.finish()));

    // end frame draw
    let belt_future = gpu.staging_belt.recall();
    gpu.thread_spawner.spawn(belt_future).unwrap();

    Instant::now() - frame_start_time
}
