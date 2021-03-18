#![allow(dead_code)]

use bytemuck::{Pod, Zeroable};
use cgmath::SquareMatrix;
use rayon::prelude::*;
use std::sync::*;
use wgpu::util::DeviceExt;
use winit::window::Window;

#[rustfmt::skip]
#[allow(unused)]
pub const OPENGL_TO_WGPU_MATRIX: cgmath::Matrix4<f32> = cgmath::Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    0.0, 0.0, 0.5, 1.0,
);

/// Handle to core WGPU stuctures such as the device and queue
pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
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

        // TODO: support for setups without unsized_binding_array
        let required_features = wgpu::Features::default()
            | wgpu::Features::PUSH_CONSTANTS
            | wgpu::Features::UNSIZED_BINDING_ARRAY
            | wgpu::Features::SAMPLED_TEXTURE_ARRAY_NON_UNIFORM_INDEXING
            | wgpu::Features::SAMPLED_TEXTURE_BINDING_ARRAY;

        let adapter_features = adapter.features();

        let required_limits = wgpu::Limits {
            max_push_constant_size: std::mem::size_of::<PushConstant>() as u32,
            ..wgpu::Limits::default()
        };

        let trace_dir = std::env::var("WGPU_TRACE");

        log::info!("Initializing device & queue...");
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("gpu"),
                    features: (adapter_features & optional_features) | required_features,
                    limits: required_limits,
                },
                trace_dir.ok().as_ref().map(std::path::Path::new),
            )
            .await
            .unwrap();
        log::info!("Setup complete!");

        (
            Gpu {
                instance,
                adapter,
                device,
                queue,
            },
            surface,
        )
    }

    /// Wraps the async init function with blocking call
    pub fn new(window: &Window) -> (Gpu, wgpu::Surface) {
        futures::executor::block_on(Gpu::init(window))
    }

    /// Wraps the async init function with a blocking call
    /// Wraps the GPU handle in ARC
    pub fn new_with_arc(window: &Window) -> (Arc<Gpu>, wgpu::Surface) {
        let (gpu, surface) = futures::executor::block_on(Gpu::init(window));
        (Arc::new(gpu), surface)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    _pos: [f32; 4],
    _tex_coord: [f32; 2],
}
impl Vertex {
    pub fn new(pos: [i8; 3], tc: [i8; 2]) -> Vertex {
        Vertex {
            _pos: [pos[0] as f32, pos[1] as f32, pos[2] as f32, 1.0],
            _tex_coord: [tc[0] as f32, tc[1] as f32],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct PushConstant {
    pub eye_index: u32,
    pub _pad: u32,
}
impl PushConstant {
    pub fn new(eye_index: u32) -> PushConstant {
        PushConstant { eye_index, _pad: 0 }
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

/// A render target is a multisampled texture+depth buffer that may be rendered to and copied to
/// A view is created for each layer of the texture (extent.depth). This allows for rendering to
/// each layer separately
/// For display-able and copy-able targets, see ResolveTarget
// TODO: Handle invalid sample counts
#[derive(Debug)]
pub struct RenderTarget {
    pub extent: wgpu::Extent3d,
    pub format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub samples: usize,
    pub texture: wgpu::Texture,
    pub depth_buffer: wgpu::Texture,
    pub views: Vec<wgpu::TextureView>,
    pub depth_views: Vec<wgpu::TextureView>,
}

impl RenderTarget {
    pub fn new(
        device: &wgpu::Device,
        extent: wgpu::Extent3d,
        format: wgpu::TextureFormat,
        samples: usize,
    ) -> RenderTarget {
        let texture: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            size: extent,
            mip_level_count: 1,
            sample_count: samples as u32,
            dimension: match extent.depth {
                1 => wgpu::TextureDimension::D2,
                // TODO: Should multi-layer textures be D2 or D2Array?
                _ => wgpu::TextureDimension::D2,
            },
            format,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT | wgpu::TextureUsage::COPY_SRC,
            label: None,
        });

        // TODO: maybe support other depth formats?
        let depth_format = wgpu::TextureFormat::Depth32Float;
        let depth_buffer = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: samples as u32,
            dimension: match extent.depth {
                1 => wgpu::TextureDimension::D2,
                _ => wgpu::TextureDimension::D2,
            },
            format: depth_format,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        });

        // Create views for each layer of texture
        let views: Vec<wgpu::TextureView> = (0..extent.depth)
            .into_iter()
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: None,
                    format: Some(format),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    level_count: None,
                    base_array_layer: i,
                    array_layer_count: std::num::NonZeroU32::new(1),
                })
            })
            .collect();

        let depth_views: Vec<wgpu::TextureView> = (0..extent.depth)
            .into_iter()
            .map(|i| {
                depth_buffer.create_view(&wgpu::TextureViewDescriptor {
                    label: None,
                    format: Some(depth_format),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    level_count: None,
                    base_array_layer: i,
                    array_layer_count: std::num::NonZeroU32::new(1),
                })
            })
            .collect();

        RenderTarget {
            extent,
            format,
            depth_format,
            samples,
            texture,
            depth_buffer,
            views,
            depth_views,
        }
    }

    /// Returns a new RenderTarget object resized to the provided extent
    pub fn resize(&mut self, device: &wgpu::Device, extent: wgpu::Extent3d) {
        *self = RenderTarget::new(device, extent, self.format, self.samples);
    }
}

/// A single sampled image that can be displayed or captured to a buffer
// TODO: Support capture to a buffer
pub struct ResolveTarget {
    pub extent: wgpu::Extent3d,
    pub format: wgpu::TextureFormat,
    pub depth_format: wgpu::TextureFormat,
    pub texture: wgpu::Texture,
    pub depth_buffer: wgpu::Texture,
    pub views: Vec<wgpu::TextureView>,
    pub depth_views: Vec<wgpu::TextureView>,
}
impl ResolveTarget {
    pub fn from_render_target(device: &wgpu::Device, target: &RenderTarget) -> ResolveTarget {
        let texture: wgpu::Texture = device.create_texture(&wgpu::TextureDescriptor {
            size: target.extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: match target.extent.depth {
                1 => wgpu::TextureDimension::D2,
                // TODO: Should multi-layer textures always be D3?
                _ => wgpu::TextureDimension::D2,
            },
            format: target.format,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT | wgpu::TextureUsage::COPY_SRC,
            label: None,
        });

        // TODO: maybe support other depth formats?
        let depth_buffer = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: target.extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: match target.extent.depth {
                1 => wgpu::TextureDimension::D2,
                _ => wgpu::TextureDimension::D2,
            },
            format: target.depth_format,
            usage: wgpu::TextureUsage::RENDER_ATTACHMENT,
        });
        // Create views for each layer of texture
        let views: Vec<wgpu::TextureView> = (0..target.extent.depth)
            .into_iter()
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: None,
                    format: Some(target.format),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    level_count: None,
                    base_array_layer: i,
                    array_layer_count: std::num::NonZeroU32::new(1),
                })
            })
            .collect();

        let depth_views: Vec<wgpu::TextureView> = (0..target.extent.depth)
            .into_iter()
            .map(|i| {
                depth_buffer.create_view(&wgpu::TextureViewDescriptor {
                    label: None,
                    format: Some(target.depth_format),
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: 0,
                    level_count: None,
                    base_array_layer: i,
                    array_layer_count: std::num::NonZeroU32::new(1),
                })
            })
            .collect();

        ResolveTarget {
            extent: target.extent,
            format: target.format,
            depth_format: target.depth_format,
            texture,
            depth_buffer,
            views,
            depth_views,
        }
    }
}

/// Array of cameras with a shared normal
/// Since the normal is fixed, this only supports single axis rotations
#[derive(Debug)]
pub struct CameraArray {
    pub count: usize,
    pub vertical_fov: cgmath::Deg<f32>,
    pub aspect_ratio: f32,
    pub normal: cgmath::Vector3<f32>,
    pub buf: wgpu::Buffer,
    pub mat: Vec<[[f32; 4]; 4]>,
}
impl CameraArray {
    pub fn build_camera(
        aspect_ratio: f32,
        eye: cgmath::Point3<f32>,
        center: cgmath::Vector3<f32>,
        up: cgmath::Vector3<f32>,
        vertical_fov: cgmath::Deg<f32>,
    ) -> [[f32; 4]; 4] {
        let proj = cgmath::perspective(vertical_fov, aspect_ratio, 1.0, 10000.0);
        let view = cgmath::Matrix4::look_to_rh(eye, center, up);
        let correction = OPENGL_TO_WGPU_MATRIX;
        (correction * proj * view).into()
    }

    /// Create a new camera array, matrices are uninitialized
    pub fn new(
        device: &wgpu::Device,
        count: usize,
        extent: wgpu::Extent3d,
        horizontal_fov: cgmath::Deg<f32>,
        normal: cgmath::Vector3<f32>,
    ) -> CameraArray {
        let aspect_ratio = extent.width as f32 / extent.height as f32;
        let mat: Vec<[[f32; 4]; 4]> = vec![cgmath::Matrix4::identity().into(); count];
        CameraArray {
            count,
            vertical_fov: horizontal_fov / aspect_ratio,
            aspect_ratio,
            normal,
            buf: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&mat),
                usage: wgpu::BufferUsage::STORAGE | wgpu::BufferUsage::COPY_DST,
            }),
            mat,
        }
    }

    /// Update camera matrices. Eyes are the positions of each eye, and look_dirs is a vector
    /// pointing in the desired look direction. Requires a mutable camera array
    pub fn update_camera(
        &mut self,
        eyes: &[cgmath::Point3<f32>],
        look_dirs: &[cgmath::Vector3<f32>],
    ) {
        let aspect_ratio = self.aspect_ratio;
        let normal = self.normal;
        let fov = self.vertical_fov;
        self.mat.par_iter_mut().enumerate().for_each(|(i, m)| {
            *m = CameraArray::build_camera(aspect_ratio, eyes[i], look_dirs[i], normal, fov);
        });
    }

    /// Resizes camera array, maintaining horizontal fov.
    /// Note that changes will not take affect until the next update
    pub fn resize(&mut self, extent: wgpu::Extent3d) {
        // maintain the previous horizontal field of view
        let horizontal_fov = self.vertical_fov * self.aspect_ratio;
        self.aspect_ratio = extent.width as f32 / extent.height as f32;
        self.vertical_fov = horizontal_fov / self.aspect_ratio;
    }

    /// Writes the current camera matrices to the GPU
    pub fn write(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(&self.mat));
    }
}

