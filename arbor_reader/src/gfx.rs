use crate::window;
use bytemuck::{Pod, Zeroable};
use std::borrow::Cow;
use std::ops::{Add, AddAssign, Mul, Sub, SubAssign};
use std::time::{Duration, Instant};
use std::{file, fmt, mem};
use wgpu;
use wgpu::util::DeviceExt;
use winit::window::Window;

use glyph_brush::ab_glyph;

pub use wgpu::CommandEncoder;
pub use wgpu::RenderPass;

/// Default output format for renderer
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
/// Default depth format for renderer
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Default present mode for renderer
pub const PRESENT_MODE: wgpu::PresentMode = wgpu::PresentMode::Mailbox;

/// Identity transform matrix
#[rustfmt::skip]
pub const IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

/// Gfx Result type wraps `gfx::Error`
pub type Result<T> = std::result::Result<T, Error>;

/// Top level errors from the gfx module
#[derive(Debug)]
pub enum Error {
    Generic,
    Surface(wgpu::SurfaceError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Generic => write!(f, "An unspecified error occured..."),
            Error::Surface(e) => e.fmt(f),
        }
    }
}

impl From<wgpu::SurfaceError> for Error {
    fn from(t: wgpu::SurfaceError) -> Self {
        Error::Surface(t)
    }
}

/// Handle to core WGPU stuctures representing physical access to the gpu, such as the device,
/// queue, and swapchain.
pub struct Context {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface: wgpu::Surface,
    pub depth_texture: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
}

impl Context {
    /// Resize the GPU to target a new swapchain size
    pub fn resize(&mut self, size: window::Size) {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        });
    }
}

/// Storage for per-frame gpu information
pub struct Frame {
    pub view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    pub start_time: Instant,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
/// Vertex primitive type for gfx
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
            step_mode: wgpu::VertexStepMode::Vertex,
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

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
/// Color primitive for gfx
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }
}

impl From<[f32; 4]> for Color {
    fn from(t: [f32; 4]) -> Color {
        Color {
            r: t[0],
            g: t[1],
            b: t[2],
            a: t[3],
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
                            visibility: wgpu::ShaderStages::FRAGMENT,
                            ty: wgpu::BindingType::Texture {
                                multisampled: false,
                                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                                view_dimension: wgpu::TextureViewDimension::D2,
                            },
                            count: None,
                        },
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::FRAGMENT,
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
                    depth_write_enabled: true,
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
                        write_mask: wgpu::ColorWrites::ALL,
                    }],
                }),
            });

        Self {
            pipeline,
            bind_group_layout,
        }
    }
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
    /// Generate a texture from the raw bytes of a `png` image. This image must be encoded with
    /// `rgba` format
    pub fn from_png(context: &Context, bytes: &[u8]) -> Result<Self> {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();

        // if color type is not rgba, fail
        if info.color_type != png::ColorType::Rgba {
            return Err(Error::Generic);
        }

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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        });

        context.queue.write_texture(
            wgpu::ImageCopyTexture {
                aspect: wgpu::TextureAspect::All,
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

/// A 2d region of the screen, generally used for scissoring the render target
#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod, Default)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub heigh: u32,
}

impl Into<[f32; 4]> for Region {
    fn into(self) -> [f32; 4] {
        [
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.heigh as f32,
        ]
    }
}

/// Stores data for a single resizable quad
// TODO: Clean up quad implementation
//      standardize format (indexed)
//      share vertex buffers between quads on GPU
//      possibly use a single actual quad with model transform?
pub struct Quad {
    pub vertices: [Vertex; 4],
    pub vertex_buffer: wgpu::Buffer,
    pub num_verts: u32,
}

impl Quad {
    /// Create a quad using hardcoded test vertices
    pub fn from_test_vertices(context: &Context) -> Self {
        let num_verts = 4;
        let vertices = [
            Vertex::new([-0.5, -0.5, 0.0], [0.0, 1.0]),
            Vertex::new([0.5, -0.5, 0.0], [1.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.0], [0.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.0], [1.0, 0.0]),
        ];

        let vertex_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        Self {
            vertices,
            vertex_buffer,
            num_verts,
        }
    }

    /// Create a quad using x and y coordinates. These should be normalized (-1 to 1) with top-left
    /// as x1y1
    // TODO: Creating a buffer for each quad is absurdly inefficient. Fix this when creating the
    // actual gfx renderer struct
    pub fn from_coords(context: &Context, x1: f32, x2: f32, y1: f32, y2: f32) -> Self {
        let num_verts = 4;
        // Implementation note: y1 and y2 are flipped to match wgpu defined coordinate system
        //  https://gpuweb.github.io/gpuweb/#coordinate-systems
        //
        // we use top-left as origin everywhere because that is how winit is set up, so this
        // inversion is only exposed here
        let y1_wgpu = -y2;
        let y2_wgpu = -y1;
        let vertices = [
            Vertex::new([x1, y1_wgpu, 0.0], [0.0, 1.0]),
            Vertex::new([x2, y1_wgpu, 0.0], [1.0, 1.0]),
            Vertex::new([x1, y2_wgpu, 0.0], [0.0, 0.0]),
            Vertex::new([x2, y2_wgpu, 0.0], [1.0, 0.0]),
        ];
        let vertex_buffer = context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        Self {
            vertices,
            vertex_buffer,
            num_verts,
        }
    }
}

/// 3 Dimensional point in winit screen coordinates
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Add for Point {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
            z: self.z + other.z,
        }
    }
}

impl Sub for Point {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
            z: self.z - other.z,
        }
    }
}

impl AddAssign for Point {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
        self.z += other.z;
    }
}

impl SubAssign for Point {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
        self.z -= other.z;
    }
}

impl Mul<f64> for Point {
    type Output = Self;
    fn mul(self, other: f64) -> Self {
        Self {
            x: self.x * other,
            y: self.y * other,
            z: self.z * other,
        }
    }
}

impl Point {
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    /// find a new point from a point on the UI such that an element with a certain width and
    /// height will be centered on that point
    pub fn centering_offset_of(&self, width: f64, height: f64) -> Self {
        Self {
            x: self.x - width / 2.0,
            y: self.y - height / 2.0,
            z: self.z,
        }
    }
}

/// Top level function for initializing the GFX module
///
/// This is required to obtain a valid gfx [Context]
pub fn init(window: &Window) -> Context {
    log::info!("Initializing gfx...");
    log::trace!("Create instance");
    let instance = wgpu::Instance::new(wgpu::Backends::all());

    log::trace!("Obtain window surface");
    let surface = unsafe { instance.create_surface(window) };

    log::trace!("Create adapter");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let optional_features = wgpu::Features::empty();
    let required_features = wgpu::Features::default();
    let adapter_features = adapter.features();
    let required_limits = wgpu::Limits::default();
    let trace_dir = std::env::var("WGPU_TRACE");

    log::trace!("Create device & queue");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            features: (adapter_features & optional_features) | required_features,
            limits: required_limits,
        },
        trace_dir.ok().as_ref().map(std::path::Path::new),
    ))
    .unwrap();

    log::trace!("Create depth texture");
    let size = window.inner_size();
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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    log::info!("gfx initialization complete!");
    Context {
        instance,
        adapter,
        device,
        queue,
        surface,
        depth_texture,
        depth_view,
    }
}

/// Data stored in Text renderer vertex buffer to draw glyphs obtained from glyph_brush
#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
struct GlyphVertex {
    left_top: [f32; 3],
    right_bottom: [f32; 2],
    tex_left_top: [f32; 2],
    tex_right_bottom: [f32; 2],
    scissor: [f32; 4],
    color: [f32; 4],
}

impl GlyphVertex {
    /// helper function to get the vertex attributes of the struct
    #[inline]
    fn vertex_attributes() -> [wgpu::VertexAttribute; 6] {
        wgpu::vertex_attr_array! [
                        0 => Float32x3,
                        1 => Float32x2,
                        2 => Float32x2,
                        3 => Float32x2,
                        4 => Float32x4,
                        5 => Float32x4,
        ]
    }
}

impl From<glyph_brush::GlyphVertex<'_>> for GlyphVertex {
    /// obtain a `gfx::GlyphVertex` from a glyph_brush::GlyphVertex`
    fn from(
        glyph_brush::GlyphVertex {
            mut tex_coords,
            pixel_coords,
            bounds,
            extra,
        }: glyph_brush::GlyphVertex,
    ) -> GlyphVertex {
        let gl_bounds = bounds;

        let mut rect = ab_glyph::Rect {
            min: ab_glyph::point(pixel_coords.min.x as f32, pixel_coords.min.y as f32),
            max: ab_glyph::point(pixel_coords.max.x as f32, pixel_coords.max.y as f32),
        };

        // handle overlapping bounds, modify uv_rect to preserve texture aspect
        if rect.max.x > gl_bounds.max.x {
            let old_width = rect.width();
            rect.max.x = gl_bounds.max.x;
            tex_coords.max.x = tex_coords.min.x + tex_coords.width() * rect.width() / old_width;
        }

        if rect.min.x < gl_bounds.min.x {
            let old_width = rect.width();
            rect.min.x = gl_bounds.min.x;
            tex_coords.min.x = tex_coords.max.x - tex_coords.width() * rect.width() / old_width;
        }

        if rect.max.y > gl_bounds.max.y {
            let old_height = rect.height();
            rect.max.y = gl_bounds.max.y;
            tex_coords.max.y = tex_coords.min.y + tex_coords.height() * rect.height() / old_height;
        }

        if rect.min.y < gl_bounds.min.y {
            let old_height = rect.height();
            rect.min.y = gl_bounds.min.y;
            tex_coords.min.y = tex_coords.max.y - tex_coords.height() * rect.height() / old_height;
        }

        GlyphVertex {
            left_top: [rect.min.x, rect.max.y, extra.z],
            right_bottom: [rect.max.x, rect.min.y],
            tex_left_top: [tex_coords.min.x, tex_coords.max.y],
            tex_right_bottom: [tex_coords.max.x, tex_coords.min.y],
            scissor: [0.0; 4],
            color: extra.color,
        }
    }
}

/// Data needed to render text. Utilized by the top level renderer struct
struct Text {
    current_glyphs: usize,
    max_glyphs: usize,
    pipeline: wgpu::RenderPipeline,
    transform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    glyph_buffer: wgpu::Buffer,
    uniforms: wgpu::BindGroup,
    cache: wgpu::Texture,
    cache_view: wgpu::TextureView,
}

impl Text {
    const INITIAL_VERTEX_BUFFER_LEN: usize = 50000;
    fn new(
        cache_width: u32,
        cache_height: u32,
        filter_mode: wgpu::FilterMode,
        render_format: wgpu::TextureFormat,
        ctx: &Context,
    ) -> Self {
        let transform = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gfx::Text transform uniform buffer"),
                contents: bytemuck::cast_slice(&IDENTITY_MATRIX),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

        let sampler = ctx.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gfx::Text texture sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: filter_mode,
            min_filter: filter_mode,
            mipmap_filter: filter_mode,
            ..Default::default()
        });

        let cache = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("gfx::Text cache texture"),
            size: wgpu::Extent3d {
                width: cache_width,
                height: cache_height,
                depth_or_array_layers: 1,
            },
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            mip_level_count: 1,
            sample_count: 1,
        });
        let cache_view = cache.create_view(&wgpu::TextureViewDescriptor::default());

        let uniform_layout = Self::uniform_layout(ctx);

        let uniforms = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gfx::Text uniforms"),
            layout: &uniform_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &transform,
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&cache_view),
                },
            ],
        });

        let glyph_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gfx::Text vertex buffer"),
            size: mem::size_of::<GlyphVertex>() as u64 * Text::INITIAL_VERTEX_BUFFER_LEN as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                push_constant_ranges: &[],
                bind_group_layouts: &[&uniform_layout],
            });

        let shader = ctx
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("gfx::Text shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../data/shaders/glyph.wgsl"
                ))),
            });

        let pipeline = ctx
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: mem::size_of::<GlyphVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &GlyphVertex::vertex_attributes(),
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    front_face: wgpu::FrontFace::Cw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[wgpu::ColorTargetState {
                        format: render_format,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::SrcAlpha,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    }],
                }),
            });

        Self {
            current_glyphs: 0,
            max_glyphs: Self::INITIAL_VERTEX_BUFFER_LEN,
            pipeline,
            transform,
            sampler,
            glyph_buffer,
            uniforms,
            cache,
            cache_view,
        }
    }

    fn upload(&mut self, ctx: &Context, glyphs: &mut [GlyphVertex], region: Option<Region>) {
        // early return if nothing to upload
        if glyphs.is_empty() {
            self.current_glyphs = 0;
            return;
        }

        // process any clipping regions
        if let Some(region) = region {
            for glyph in glyphs.iter_mut() {
                glyph.scissor = region.into();
            }
        }

        if glyphs.len() > self.max_glyphs {}
    }

    fn draw<'render>(&'render self, renderpass: &mut wgpu::RenderPass<'render>) {
        renderpass.set_pipeline(&self.pipeline);
        renderpass.set_bind_group(0, &self.uniforms, &[]);
        renderpass.set_vertex_buffer(0, self.glyph_buffer.slice(..));
        renderpass.draw(0..4, 0..self.current_glyphs as u32);
    }

    /// Helper function to generate the uniform layout for the Text renderer pipeline
    fn uniform_layout(ctx: &Context) -> wgpu::BindGroupLayout {
        ctx.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("gfx::Text uniforms layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                mem::size_of::<[f32; 16]>() as u64
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler {
                            filtering: true,
                            comparison: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                ],
            })
    }
}

struct Renderer<'ctx> {
    ctx: &'ctx Context,
    text: Text,
}

/// Called to begin a new frame to be drawn to
///
/// # Error
/// If the frame cannot be started (usually due to a failure to get the next frame from the
/// swapchain). An error will be returned. Note that this should be handled gracefully, as
/// framedrops will likely occur at some point
pub fn begin_frame(context: &Context) -> Result<(CommandEncoder, Frame)> {
    // Begin to draw the frame.
    let output = context.surface.get_current_texture()?;

    // start counting time the moment we have the frame
    let start_time = Instant::now();

    let view = output
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

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
            start_time,
            view,
            depth_view,
        },
    ))
}

/// Start recording a renderpass on a given render target. Returns a command encoder to use for
/// draw calls
pub fn begin_renderpass<'render>(
    encoder: &'render mut wgpu::CommandEncoder,
    frame: &'render Frame,
) -> RenderPass<'render> {
    // Clear frame
    let renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Render pass"),
        color_attachments: &[wgpu::RenderPassColorAttachment {
            view: &frame.view,
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

pub fn end_renderpass(renderpass: wgpu::RenderPass) {
    drop(renderpass);
}

pub fn end_frame(context: &mut Context, encoder: wgpu::CommandEncoder, frame: Frame) -> Duration {
    // Submit the commands.
    context.queue.submit(std::iter::once(encoder.finish()));

    // end frame draw
    Instant::now() - frame.start_time
}
