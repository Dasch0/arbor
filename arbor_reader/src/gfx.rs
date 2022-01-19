use crate::window;
use bytemuck::{Pod, Zeroable};
use std::any::type_name;
use std::borrow::{self, Borrow, Cow};
use std::marker::PhantomData;
use std::ops::{self, Add, AddAssign, Mul, Sub, SubAssign};
use std::time::{Duration, Instant};
use std::{file, fmt, mem};
use wgpu;
use wgpu::util::DeviceExt;
use winit::window::Window;

use glyph_brush::ab_glyph;
use log::{debug, error, info, trace, warn};

pub use wgpu::CommandEncoder;
pub use wgpu::RenderPass;

/// Default output format for renderer
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8UnormSrgb;
/// Default Index format for renderer
pub const INDEX_FORMAT: wgpu::IndexFormat = wgpu::IndexFormat::Uint32;
/// Default depth format for renderer
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Default present mode for renderer
pub const PRESENT_MODE: wgpu::PresentMode = wgpu::PresentMode::Immediate;

/// Bind group universally used for data that changes infrequently
pub const BIND_GROUP_STATIC_INDEX: u32 = 0;
/// Bind group universally used for data that changes per frame
pub const BIND_GROUP_PER_FRAME_INDEX: u32 = 1;
/// Bind group universally used for data that changes per renderpass
pub const BIND_GROUP_PER_RENDERPASS_INDEX: u32 = 2;
/// Bind group universally used for data that changes per draw call
pub const BIND_GROUP_PER_DRAW_INDEX: u32 = 3;

/// Identity transform matrix
#[rustfmt::skip]
pub const IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

/// Helper function to generate a generate a transform matrix.
#[rustfmt::skip]
pub fn ortho_transform_builder(zoom: f32, offset_x: f32, offset_y: f32, screen_width: u32, screen_height: u32) -> [f32; 16] {
    [
        zoom / screen_width as f32, 0.0, 0.0, 0.0,
        0.0, -zoom / screen_height as f32, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0 + offset_x, 1.0 + offset_y, 0.0, 1.0,
    ]
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
/// Textured Vertex primitive type for gfx
pub struct TexturedVertex {
    pub pos: [f32; 3],
    pub tex_coord: [f32; 2],
}
impl TexturedVertex {
    /// Helper method for concisely creating new vertices
    pub fn new(pos: [f32; 3], tc: [f32; 2]) -> TexturedVertex {
        TexturedVertex {
            pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32],
            tex_coord: [tc[0] as f32, tc[1] as f32],
        }
    }

    /// Get a description of the vertex layout
    fn vertex_attributes() -> [wgpu::VertexAttribute; 2] {
        wgpu::vertex_attr_array! [
                        0 => Float32x3,
                        1 => Float32x2,
        ]
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
/// Simple vertex primitive type for gfx
pub struct Vertex {
    pub pos: [f32; 4],
}

impl Vertex {
    /// Obtain the wgpu description of the vertex layout
    fn vertex_attributes() -> [wgpu::VertexAttribute; 1] {
        wgpu::vertex_attr_array! [
                        0 => Float32x4,
        ]
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

/// Index primitive type for gfx
type Index = u32;

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

/// gfx Result type wraps `gfx::Error`
pub type Result<T> = std::result::Result<T, Error>;

/// Top level errors from the gfx module
#[derive(Debug)]
pub enum Error {
    Generic,
    InvalidOffsetBufferEntry,
    Alignment,
    Surface(wgpu::SurfaceError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Generic => write!(f, "An unspecified error occured..."),
            Error::InvalidOffsetBufferEntry => {
                write!(
                    f,
                    "Attempted to access a nonexistent entry in an OffsetBuffer"
                )
            }
            Error::Alignment => {
                write!(f, "Attempted to access gpu memory with invalid alignment")
            }
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
    /// Last checked width of the context. This may be out of step with the actual window size
    /// until a resize event is triggered
    pub width: u32,
    /// Last checked height of the context. This may be out of step with the actual window size
    /// until a resize event is triggered
    pub height: u32,
}

impl Context {
    /// Resize the GPU to target a new swapchain size
    pub fn resize(&mut self, size: window::Size) {
        trace!("recreate surface and depth textures");
        self.width = size.width;
        self.height = size.height;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: OUTPUT_FORMAT,
            width: self.width,
            height: self.height,
            present_mode: PRESENT_MODE,
        };
        self.surface.configure(&self.device, &config);
        self.depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth buffer"),
            size: wgpu::Extent3d {
                width: self.width,
                height: self.height,
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
    pub start_time: Instant,
    pub surface_texture: wgpu::SurfaceTexture,
    pub view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
}

/// A gfx::Texture stores the underlying texture and view. You can perform a shared borrow on
/// Texture to directly access the view or texture, depending on the needed access pattern
pub struct Texture {
    pub view: wgpu::TextureView,
    pub raw: wgpu::Texture,
}

impl borrow::Borrow<wgpu::TextureView> for Texture {
    fn borrow(&self) -> &wgpu::TextureView {
        &self.view
    }
}

impl borrow::Borrow<wgpu::Texture> for Texture {
    fn borrow(&self) -> &wgpu::Texture {
        &self.raw
    }
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

        Ok(Self { raw: texture, view })
    }

    /// create an empty 2d texture with a given dimension
    pub fn with_size(width: u32, height: u32, label: &str, ctx: &Context) -> Self {
        let raw = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            mip_level_count: 1,
            sample_count: 1,
        });
        let view = raw.create_view(&wgpu::TextureViewDescriptor::default());

        Self { raw, view }
    }
}

/// A GPU buffer with info on the offsets of different entries in the buffer. Provide the type of
/// data stored in the buffer as the generic param. This allows for correct offset calculation
///
/// Useful for unified vertex/index/uniform buffers
pub struct OffsetBuffer<T: Pod> {
    /// buffer size in bytes
    pub size: u64,
    buffer: wgpu::Buffer,
    offset_table: Vec<u64>,
    data_type: PhantomData<T>,
}

impl<T: Pod> OffsetBuffer<T> {
    pub fn new(size: u64, usage: wgpu::BufferUsages, ctx: &Context) -> Self {
        // Create GPU resources
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(type_name::<Self>()),
            size,
            usage,
            mapped_at_creation: false,
        });

        // Can assume max offsets unlikely to exceed buffer size in floats
        let offsets = Vec::with_capacity((size / 4) as usize);

        Self {
            size,
            buffer,
            offset_table: offsets,
            data_type: PhantomData::default(),
        }
    }

    /// Push a new element to the end of the buffer. The slice must be sized to be aligned
    /// with [wgpu::COPY_BUFFER_ALIGNMENT]
    ///
    /// returns the ID of the newly added item
    pub fn push(&mut self, data: &[T], ctx: &Context) -> Result<usize> {
        let data_bytes: &[u8] = bytemuck::cast_slice(data);
        // early return if data is not aligned properly
        if data_bytes.len() as u64 % wgpu::COPY_BUFFER_ALIGNMENT != 0 {
            return Err(Error::Alignment);
        }

        let current_offset = *self.offset_table.last().unwrap_or(&0);
        debug!(
            "pushing {} bytes to buffer {:?} with offset {}",
            data_bytes.len(),
            self.buffer,
            current_offset
        );
        ctx.queue
            .write_buffer(&self.buffer, current_offset, data_bytes);
        let new_offset = current_offset + data_bytes.len() as u64;

        self.offset_table.push(new_offset);
        Ok(self.offset_table.len())
    }

    /// get the slice of of an entry in the offset buffer
    ///
    /// # Error
    /// if the provided ID is invalid or out of range
    pub fn get_slice<'buf>(&'buf self, id: usize) -> Result<wgpu::BufferSlice<'buf>> {
        if id >= self.offset_table.len() {
            return Err(Error::InvalidOffsetBufferEntry);
        }

        let last_index = id.checked_sub(1);
        let start = match last_index {
            Some(i) => self.offset_table[i],
            None => 0,
        };
        let end = self.offset_table[id];

        let slice = self.buffer.slice(start..end);
        Ok(slice)
    }

    /// get the [std::ops::Range] of the raw bytes of an entry in the offset buffer.
    ///
    /// # Error
    /// if the provided ID is invalid or out of range
    pub fn get_byte_range<'buf>(&'buf self, id: usize) -> Result<ops::Range<u32>> {
        if id >= self.offset_table.len() {
            return Err(Error::InvalidOffsetBufferEntry);
        }

        let last_index = id.checked_sub(1);
        let start = match last_index {
            Some(i) => self.offset_table[i],
            None => 0,
        } as u32;
        let end = self.offset_table[id] as u32;

        Ok(start..end)
    }

    /// get the [std::ops::Range] of a data entry in the offset buffer. This range is based on the
    /// type supplied to the [OffsetBuffer] at creation.
    ///
    /// # Error
    /// if the provided ID is invalid or out of range
    pub fn get_range<'buf>(&'buf self, id: usize) -> Result<ops::Range<u32>> {
        if id >= self.offset_table.len() {
            return Err(Error::InvalidOffsetBufferEntry);
        }

        trace!(
            "getting range of type: {} type size reported as: {}",
            type_name::<T>(),
            mem::size_of::<T>()
        );

        let last_index = id.checked_sub(1);
        let start = (match last_index {
            Some(i) => self.offset_table[i],
            None => 0,
        } / mem::size_of::<T>() as u64) as u32;
        let end = (self.offset_table[id] / mem::size_of::<T>() as u64) as u32;

        Ok(start..end)
    }
}

/// Top level function for initializing the GFX module
///
/// This is required to obtain a valid gfx [Context]
pub fn init(window: &Window) -> Context {
    info!("Initializing gfx...");
    trace!("Create instance");
    let instance = wgpu::Instance::new(wgpu::Backends::all());

    trace!("Obtain window surface and initial size");
    let surface = unsafe { instance.create_surface(window) };
    let size = window.inner_size();
    let width = size.width;
    let height = size.height;

    trace!("Create adapter");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    // main required feature is 128 byte push constants
    let optional_features = wgpu::Features::empty();
    let required_features = wgpu::Features::default() | wgpu::Features::PUSH_CONSTANTS;
    let adapter_features = adapter.features();
    let required_limits = wgpu::Limits {
        max_push_constant_size: 128,
        ..wgpu::Limits::default()
    };
    let trace_dir = std::env::var("WGPU_TRACE");

    trace!("Create device & queue");
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            features: (adapter_features & optional_features) | required_features,
            limits: required_limits,
        },
        trace_dir.ok().as_ref().map(std::path::Path::new),
    ))
    .unwrap();

    trace!("create surface textures");
    let config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: OUTPUT_FORMAT,
        width: size.width,
        height: size.height,
        present_mode: PRESENT_MODE,
    };
    surface.configure(&device, &config);

    trace!("Create depth texture");
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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    });
    let depth_view = depth_texture.create_view(&wgpu::TextureViewDescriptor::default());

    info!("gfx initialization complete!");
    Context {
        instance,
        adapter,
        device,
        queue,
        surface,
        depth_texture,
        depth_view,
        width,
        height,
    }
}

/// Data stored in Text renderer vertex buffer to draw glyphs obtained from glyph_brush
#[repr(C)]
#[derive(Debug, Clone, Copy, Zeroable, Pod)]
pub struct GlyphVertex {
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
    /// Map internal `glyph_brush::GlyphVertex` layout to GPU compatible `gfx::GlyphVertex`
    /// layout
    fn from_glyph_brush(
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
pub struct GlyphRenderer {
    current_glyphs: usize,
    max_glyphs: usize,
    pipeline: wgpu::RenderPipeline,
    transform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    glyph_buffer: wgpu::Buffer,
    uniforms: wgpu::BindGroup,
    cache: Texture,
}

impl GlyphRenderer {
    /// Create a new glyph brush.
    ///
    /// The cache dimensions define the size of the texture used to store
    /// glyphs. Currently 2048x2048 is the max supported size.
    ///
    /// The buffer size is the initial size of the vertex buffer allocation. This will be resized
    /// as needed during processing
    ///
    /// The filter mode defines the sampler filtering and may be used for transparency. Render
    /// format should match the output format to avoid mangling or conversions
    ///
    pub fn new(
        cache_width: u32,
        cache_height: u32,
        buffer_size: usize,
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

        let cache = Texture::with_size(cache_width, cache_height, "gfx::Text cache", ctx);

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
                    resource: wgpu::BindingResource::TextureView(cache.borrow()),
                },
            ],
        });

        let glyph_buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gfx::Text vertex buffer"),
            size: mem::size_of::<GlyphVertex>() as u64 * buffer_size as u64,
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
            max_glyphs: buffer_size,
            pipeline,
            transform,
            sampler,
            glyph_buffer,
            uniforms,
            cache,
        }
    }

    /// process a `glyph_brush` and all its queued text to prepare to draw
    ///
    /// The glyph brush must be instantiated with the `gfx::GlyphVertex` vertex type to be
    /// compatible with the `gfx` text renderer
    ///
    /// # Error
    ///
    /// Error if the text fails to draw due to a glyph_brush error. Generally, can proceed by
    /// drawing next frame
    pub fn process_glyph_brush(
        &mut self,
        glyph_brush: &mut glyph_brush::GlyphBrush<GlyphVertex>,
        region: Option<Region>,
        ctx: &Context,
    ) -> Result<()> {
        // based on the current context size, create a screen transform for sizing the text
        // properly. This can happen even if the glyph brush is unchanged (window resize with no
        // other updates)
        ctx.queue.write_buffer(
            &self.transform,
            0,
            bytemuck::cast_slice(&ortho_transform_builder(
                2.0, 0.0, 0.0, ctx.width, ctx.height,
            )),
        );

        // check if any glyph changes occurred
        let brush_action = glyph_brush.process_queued(
            |rect, tex_data| {
                let offset = [rect.min[0] as u16, rect.min[1] as u16];
                let size = [rect.width() as u16, rect.height() as u16];

                self.update_cache(offset, size, tex_data, ctx);
            },
            GlyphVertex::from_glyph_brush,
        );

        match brush_action {
            Ok(glyph_brush::BrushAction::Draw(mut verts)) => {
                self.upload(&mut verts, region, ctx);
                Ok(())
            }
            Ok(glyph_brush::BrushAction::ReDraw) => Ok(()),
            Err(glyph_brush::BrushError::TextureTooSmall { suggested }) => {
                warn!(
                    "Increasing glyph texture size {old:?} -> {new:?}. \
                     Consider initializing with a larger cache_width and cache_height to avoid\
                     resizing",
                    old = glyph_brush.texture_dimensions(),
                    new = (suggested.0, suggested.1),
                );

                self.resize_cache(suggested.0, suggested.1, ctx);
                glyph_brush.resize_texture(suggested.0, suggested.1);
                Err(Error::Generic)
            }
        }
    }

    /// Draw all the text prepared by [process_glyph_brush] brush this frame
    pub fn draw<'render>(&'render self, renderpass: &mut wgpu::RenderPass<'render>) {
        info!("drawing glyphs");
        debug!("current_glyphs: {}", self.current_glyphs);
        renderpass.set_pipeline(&self.pipeline);
        renderpass.set_bind_group(BIND_GROUP_STATIC_INDEX, &self.uniforms, &[]);
        renderpass.set_vertex_buffer(0, self.glyph_buffer.slice(..));
        renderpass.draw(0..4, 0..self.current_glyphs as u32);
    }

    /// Upload vertex data to the gpu, applies clipping here in order to avoid having to modify the
    /// vertex buffer or pass in extra data during draw
    // NOTE: May be more optimal to apply the region as push constants at draw time? Reduces CPU
    // work here
    fn upload(&mut self, glyphs: &mut [GlyphVertex], region: Option<Region>, ctx: &Context) {
        info!("uploading glyph_brush content to gpu");
        // early return if nothing to upload
        if glyphs.is_empty() {
            warn!("list of glyphs obtained from glyph_brush to draw was empty");
            self.current_glyphs = 0;
            return;
        }

        self.current_glyphs = glyphs.len();

        // process any clipping regions
        if let Some(region) = region {
            for glyph in glyphs.iter_mut() {
                glyph.scissor = region.into();
            }
        }

        let glyph_bytes = bytemuck::cast_slice(glyphs);

        // handle case where buffer can fit the glyphs, or if it needs resizing
        if glyphs.len() <= self.max_glyphs {
            ctx.queue.write_buffer(&self.glyph_buffer, 0, glyph_bytes);
        } else {
            info!("resize glyph_buffer needed");
            debug!("resize from: {} to: {}", self.max_glyphs, glyphs.len());
            self.max_glyphs = glyphs.len();
            self.glyph_buffer = ctx
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("gfx::Text vertex buffer"),
                    contents: glyph_bytes,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
        }
    }

    /// Update the cache with new data from `glyph_brush`, gets called every draw of novel data
    fn update_cache(&self, offset: [u16; 2], size: [u16; 2], data: &[u8], ctx: &Context) {
        let width = size[0] as usize;
        let height = size[1] as usize;

        ctx.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: self.cache.borrow(),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: u32::from(offset[0]),
                    y: u32::from(offset[1]),
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: std::num::NonZeroU32::new(width as u32),
                rows_per_image: std::num::NonZeroU32::new(height as u32),
            },
            wgpu::Extent3d {
                width: size[0] as u32,
                height: size[1] as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Resize the cache texture. Usually only called if the `glyph_brush` throws a
    /// `TextureTooSmall` error
    fn resize_cache(&mut self, width: u32, height: u32, ctx: &Context) {
        self.cache = Texture::with_size(width, height, "gfx::Text cache", ctx);

        // recreate uniform, location of the cache has changed
        self.uniforms = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gfx::Text uniforms"),
            layout: &self.pipeline.get_bind_group_layout(0), // text renderer only ever has one bind group
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.transform,
                        offset: 0,
                        size: None,
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(self.cache.borrow()),
                },
            ],
        });
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

/// Push Constant data structure for [ShapeRenderer]
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ShapeRendererPushConstant {
    pub color: [f32; 4],
    pub transform: [f32; 16],
}

/// Data needed to render untextured shapes with simple geometries.
pub struct ShapeRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: OffsetBuffer<Vertex>,
    index_buffer: OffsetBuffer<Index>,
}

impl ShapeRenderer {
    /// shape ID for builtin shapes
    pub const QUAD_SHAPE_ID: usize = 0;
    /// shape ID for builtin shapes
    pub const TRIANGLE_SHAPE_ID: usize = 1;
    /// push constant offset for 16 bytes of color data
    pub const COLOR_PUSH_CONSTANT_OFFSET: u32 = 0;
    /// push constant offset for 64 bytes of camera transform data
    pub const TRANSFORM_PUSH_CONSTANT_OFFSET: u32 = 16;

    /// Create a new ShapeRenderer with sensible default settings
    pub fn default(ctx: &Context) -> Self {
        Self::new(ctx, 256, OUTPUT_FORMAT)
    }

    pub fn new(ctx: &Context, buffer_size: u64, render_format: wgpu::TextureFormat) -> Self {
        let mut vertex_buffer = OffsetBuffer::new(
            buffer_size,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            ctx,
        );
        let mut index_buffer = OffsetBuffer::new(
            buffer_size,
            wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            ctx,
        );
        // store default shapes in buffers. Must happen immediately and in order after buffer
        // creation to match the rect and triangle shape id constants
        Self::upload_unit_quad(&mut vertex_buffer, &mut index_buffer, ctx);
        Self::upload_unit_triangle(&mut vertex_buffer, &mut index_buffer, ctx);

        let shader = ctx
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("gfx::Shape shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../data/shaders/shape.wgsl"
                ))),
            });

        // Shader/pipeline takes a vertex buffer, an index buffer, and a push constant containing a
        // transform matrix and shape color
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                push_constant_ranges: &[wgpu::PushConstantRange {
                    stages: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    range: 0..mem::size_of::<ShapeRendererPushConstant>() as u32,
                }],

                bind_group_layouts: &[],
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
                        array_stride: mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &Vertex::vertex_attributes(),
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
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
            vertex_buffer,
            index_buffer,
            pipeline,
        }
    }

    /// Low level implementation method to draw a shape within an existing renderpass.
    ///
    /// Can draw a custom shape already loaded via [add_shape], or can reference the default
    /// shape ids, [QUAD_SHAPE_ID] or [TRIANGLE_SHAPE_ID], to draw a quad or triangle respectively.
    ///
    /// # Error
    ///
    /// If the shape_id is not valid or is not already added to the vertex or index buffers
    pub fn draw_shape<'render>(
        &'render self,
        shape_id: usize,
        color: [f32; 4],
        transform: [f32; 16],
        renderpass: &mut wgpu::RenderPass<'render>,
    ) -> Result<()> {
        let push_constant = ShapeRendererPushConstant { color, transform };
        renderpass.set_pipeline(&self.pipeline);
        renderpass.set_push_constants(
            wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            Self::COLOR_PUSH_CONSTANT_OFFSET,
            bytemuck::cast_slice(&[push_constant]),
        );
        // Here we set whole buffers, but only draw sub-slices, if multiple shape calls are
        // drawn in sequence, the repeated vertex buffer and pipeline sets do nothing
        renderpass.set_vertex_buffer(0, self.vertex_buffer.buffer.slice(..));
        renderpass.set_index_buffer(self.index_buffer.buffer.slice(..), INDEX_FORMAT);

        let vertex_range = self.vertex_buffer.get_range(shape_id)?;
        let index_range = self.index_buffer.get_range(shape_id)?;

        //TODO: logging
        info!(
            "drawing shape_id: {:?}, index range: {:?}, base_vertex: {:?}",
            shape_id, index_range, vertex_range.start
        );
        renderpass.draw_indexed(index_range, vertex_range.start as i32, 0..1);

        Ok(())
    }

    /// Helper function to generate the vertices/indices for a unit quad centered around the
    /// origin
    fn upload_unit_quad(
        vertex_buffer: &mut OffsetBuffer<Vertex>,
        index_buffer: &mut OffsetBuffer<Index>,
        ctx: &Context,
    ) {
        let vertices = [
            Vertex {
                pos: [1.0, 1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [-1.0, -1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [1.0, -1.0, 0.0, 1.0],
            },
        ];
        vertex_buffer
            .push(bytemuck::cast_slice(&vertices), ctx)
            .unwrap();

        let indices: [u32; 6] = [0, 1, 2, 2, 3, 0];
        index_buffer
            .push(bytemuck::cast_slice(&indices), ctx)
            .unwrap();
    }

    /// Helper function to generate the vertices/indices for a unit equilateral triangle centered
    /// around the origin
    fn upload_unit_triangle(
        vertex_buffer: &mut OffsetBuffer<Vertex>,
        index_buffer: &mut OffsetBuffer<Index>,
        ctx: &Context,
    ) {
        let vertices = [
            Vertex {
                pos: [0.5, 0.5, 0.0, 1.0],
            },
            Vertex {
                pos: [-0.5, 0.5, 0.0, 1.0],
            },
            Vertex {
                pos: [-0.5, -0.5, 0.0, 1.0],
            },
        ];

        vertex_buffer
            .push(bytemuck::cast_slice(&vertices), ctx)
            .unwrap();

        let indices: [u32; 3] = [0, 1, 2];

        index_buffer
            .push(bytemuck::cast_slice(&indices), ctx)
            .unwrap();
    }
}

/// Push Constant data structure for [SpriteRenderer]
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
// NOTE: Align every field by 16 bytes!!!!
struct SpriteInstance {
    pub position: [f32; 4],
    pub transform: [f32; 16],
}

/// Data needed to render untextured shapes with simple geometries.
pub struct SpriteRenderer {
    pipeline: wgpu::RenderPipeline,
    // only a single quad is ever stored here, no need for offset buffer
    vertex_buffer: wgpu::Buffer,
    instance_buffer: OffsetBuffer<SpriteInstance>,
    textures: Vec<wgpu::Texture>,
    bind_groups: Vec<wgpu::BindGroup>,
}

impl SpriteRenderer {
    /// Create a new ShapeRenderer with sensible default settings
    pub fn default(ctx: &Context) -> Self {
        Self::new(ctx, 256, OUTPUT_FORMAT)
    }

    pub fn new(ctx: &Context, buffer_size: u64, render_format: wgpu::TextureFormat) -> Self {
        // fixed vertex buffer that just stores a unit quad. All textures will be drawn to this
        // scaled quad
        let vertex_buffer = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(type_name::<Self>()),
                contents: bytemuck::cast_slice(&Self::unit_quad()),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let shader = ctx
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("gfx::Sprite shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../data/shaders/sprite.wgsl"
                ))),
            });

        let bind_group_layouts =
            ctx.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("gfx::SpriteRenderer uniform layout"),
                    entries: &[
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(
                                    mem::size_of::<SpriteInstance>() as u64,
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
                    ],
                });

        // Shader/pipeline takes a vertex buffer, an index buffer, and a push constant containing a
        // transform matrix and shape color
        let pipeline_layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                push_constant_ranges: &[],
                bind_group_layouts: &[],
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
                        array_stride: mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &Vertex::vertex_attributes(),
                    }],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
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
            vertex_buffer,
            pipeline,
        }
    }

    /// Low level implementation method to draw a sprite within an existing renderpass.
    ///
    /// # Error
    ///
    /// If the sprite id is not valid or is not already added to the vertex or index buffers
    pub fn draw_sprite<'render>(
        &'render self,
        sprite_id: usize,
        color: [f32; 4],
        transform: [f32; 16],
        renderpass: &mut wgpu::RenderPass<'render>,
    ) -> Result<()> {
        renderpass.set_bind_group(BIND_GROUP_PER_DRAW_INDEX, &self.bind_groups[sprite_id], &[]);
        renderpass.set_pipeline(&self.pipeline);
        renderpass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        // always draw the 6 vertices of the quad with the instance data
        renderpass.draw(0..6, 0..1);

        Ok(())
    }

    /// Get the vertices to draw an un-indexed quad
    fn unit_quad() -> [Vertex; 6] {
        [
            Vertex {
                pos: [1.0, 1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [-1.0, 1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [-1.0, -1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [-1.0, -1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [1.0, -1.0, 0.0, 1.0],
            },
            Vertex {
                pos: [1.0, 1.0, 0.0, 1.0],
            },
        ]
    }
}

/// Top level gfx::Renderer object. This encapsulates the different data and methods for rendering
/// different types of data as supported by gfx.
///
/// The renderer is structured with multiple sub-renderers supporting different types of drawables
/// e.g. text, sprites, and shapes. These may be accessed via the top level draw_* helper methods
/// defined for the renderer, or the lower level sub-renderers may be accessed directly. If
/// performing lower level access make sure to understand the expected call sequence, as this
/// differs in implementation depending on the sub-renderer.
///
/// For situations where only certain sub-renderers are needed, it is recommended to create those
/// directly (see [GlyphRenderer] for example), and just use them along with a gfx::Context.
// Implementation note:
//  The lifetimes on draw methods in this struct are a bit verbose. They generally follow the
//  format:
//  ```
//  def<'renderer, 'rpass> draw_fn(&'renderer self, &'rpass mut renderpass: Renderpass<'renderer>
//  ```
//  This is required to define the relationship of the renderer data and the renderpass.
//  Buffers and renderer resources are stored IN the RenderPass struct, meaning we pass in the
//  renderer lifetime via the `Renderpass<'renderer>...` syntax.
//
//  However, the renderpass itself does not live that long, and so its own lifetime is
//  encoded differently via the `&'rpass mut Renderpass` syntax
pub struct Renderer {
    pub glyph: GlyphRenderer,
    pub shape: ShapeRenderer,
    pub sprite: SpriteRenderer,
}

impl Renderer {
    /// Create a new renderer with default params.
    pub fn default(ctx: &Context) -> Self {
        // glyph renderer with sensible default config
        let glyph = GlyphRenderer::new(
            2048,
            2048,
            50000,
            wgpu::FilterMode::Linear,
            OUTPUT_FORMAT,
            ctx,
        );

        let shape = ShapeRenderer::default(ctx);
        Self { glyph, shape }
    }

    /// Create a new renderer with custom configs for the lower level renderers. This requires an
    /// initialized `gfx::Context` and requires that the context persists longer than the renderer.
    /// Generally this limits the context and renderer to the same thread
    pub fn with_renderers(glyph: GlyphRenderer, shape: ShapeRenderer) -> Self {
        Self { glyph, shape }
    }

    /// Method to prepare and renders all drawn glyphs
    ///
    /// Handled via an internal renderpass.
    ///
    /// # Error
    ///
    /// If the draw due to an internal glyph_brush error
    pub fn render_glyphs(
        &mut self,
        glyph_brush: &mut glyph_brush::GlyphBrush<GlyphVertex>,
        region: Option<Region>,
        frame: &Frame,
        ctx: &Context,
    ) -> wgpu::CommandBuffer {
        // processing the glyphs should occur prior to starting the renderpass
        let res = self.glyph.process_glyph_brush(glyph_brush, region, ctx);

        // handle error processing glyphs, we still create a command buffer and finish method normally
        // - the glyphs just won't draw correctly for that frame
        if let Err(e) = res {
            error!("{}", e)
        }

        // create a new encoder
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("text renderpass encoder"),
            });

        // create a renderpass dedicated to drawing all the glyphs
        let mut renderpass = begin_renderpass_load(&mut encoder, frame);

        self.glyph.draw(&mut renderpass);
        end_renderpass(renderpass);
        encoder.finish()
    }

    /// Draw a 2d quad shape
    pub fn draw_quad<'renderer, 'rpass>(
        &'renderer self,
        renderpass: &'rpass mut wgpu::RenderPass<'renderer>,
        ctx: &Context,
    ) {
        // default shapes always exist if initialized, safe to unwrap
        self.shape
            .draw_shape(
                ShapeRenderer::QUAD_SHAPE_ID,
                [1.0, 0.5, 0.1, 1.0],
                // FIXME: better way to get the camera transform in here?
                ortho_transform_builder(200.0, 1.0, -1.0, ctx.width, ctx.height),
                renderpass,
            )
            .unwrap();
    }

    /// Draw a 2d triangle shape
    pub fn draw_triangle<'renderer, 'rpass>(
        &'renderer self,
        renderpass: &'rpass mut wgpu::RenderPass<'renderer>,
        ctx: &Context,
    ) {
        // default shapes always exist if initialized, safe to unwrap
        self.shape
            .draw_shape(
                ShapeRenderer::TRIANGLE_SHAPE_ID,
                [1.0, 0.5, 0.3, 1.0],
                // FIXME: better way to get the camera transform in here?
                ortho_transform_builder(200.0, 1.0, 0.0, ctx.width, ctx.height),
                renderpass,
            )
            .unwrap();
    }
}

/// Top level gfx method begin a new frame to draw to
///
/// # Error
///
/// If the frame cannot be started (usually due to a failure to get the next frame from the
/// swapchain). An error will be returned. Note that this should be handled gracefully, as
/// framedrops will likely occur at some point
pub fn begin_frame(ctx: &Context) -> Result<(CommandEncoder, Frame)> {
    // Begin to draw the frame.
    let surface_texture = ctx.surface.get_current_texture()?;

    // start counting time the moment we have the frame
    let start_time = Instant::now();

    let view = surface_texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let depth_view = ctx
        .depth_texture
        .create_view(&wgpu::TextureViewDescriptor::default());

    let encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("primary renderpass encoder"),
        });

    Ok((
        encoder,
        Frame {
            start_time,
            surface_texture,
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

/// Start recording a renderpass on a given render target, loading whatever has previously been
/// rendered. Returns a command encoder to use for draw calls
pub fn begin_renderpass_load<'render>(
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
                load: wgpu::LoadOp::Load,
                store: true,
            },
        }],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &frame.depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: true,
            }),
            stencil_ops: None,
        }),
    });
    renderpass
}

pub fn end_renderpass(renderpass: wgpu::RenderPass) {
    drop(renderpass);
}

/// end the frame, submitting and drawing all recorded command buffers
pub fn end_frame<const N: usize>(
    command_buffers: [wgpu::CommandBuffer; N],
    frame: Frame,
    ctx: &mut Context,
) -> Duration {
    // Submit the commands and present image
    ctx.queue.submit(command_buffers);
    frame.surface_texture.present();
    // end frame draw
    Instant::now() - frame.start_time
}
