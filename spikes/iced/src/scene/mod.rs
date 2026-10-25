//! The composition renderer: plain wgpu, so the preview (inside iced's `shader` widget) and the
//! export (on a headless device) draw with the same pipelines and shaders.
//!
//! A frame is the scene's items in one instanced draw, after three small passes that capture
//! and blur the camera for the full-frame square layout's backdrop.

pub mod composition;
pub mod text;

use composition::{BackdropCapture, Frame, RenderTheme, TextureSizes};
use iced::wgpu;
use omareel_core::export::composition::CompositionState;
use omareel_core::project::Project;
use std::sync::Arc;
use std::time::Instant;
use text::{ATLAS_SIZE, TextAtlas};

/// A decoded video frame: tightly packed RGBA rows. `id` changes whenever the pixels do.
#[derive(Clone)]
pub struct VideoFrame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<Vec<u8>>,
}

impl std::fmt::Debug for VideoFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VideoFrame({} {}x{})", self.id, self.width, self.height)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    canvas: [f32; 2],
    srgb_target: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BlurParams {
    crop: [f32; 4],
    direction: [f32; 2],
    sigma: f32,
    quarter_turns: u32,
    flip: u32,
    _pad: [u32; 3],
}

struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
}

impl Texture {
    fn new(device: &wgpu::Device, label: &str, size: (u32, u32), format: wgpu::TextureFormat, render: bool) -> Texture {
        let mut usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
        if render {
            usage |= wgpu::TextureUsages::RENDER_ATTACHMENT;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0.max(1), height: size.1.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Texture { texture, view, size }
    }

    fn write(&self, queue: &wgpu::Queue, pixels: &[u8], bytes_per_pixel: u32) {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size.0 * bytes_per_pixel),
                rows_per_image: Some(self.size.1),
            },
            wgpu::Extent3d { width: self.size.0, height: self.size.1, depth_or_array_layers: 1 },
        );
    }
}

/// Which video a frame is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Video {
    Screen,
    Camera,
}

/// Time spent in the renderer's CPU side, for the stats.
#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub upload_ns: u64,
    pub prepare_ns: u64,
}

pub struct Scene {
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    globals: wgpu::Buffer,
    items: wgpu::Buffer,
    item_capacity: u64,
    item_count: u32,
    sampler: wgpu::Sampler,
    wallpaper: Texture,
    screen: Texture,
    camera: Texture,
    cursor: Texture,
    text: Texture,
    backdrop_small: Texture,
    backdrop_wide: Texture,
    backdrop: Texture,
    bind_group: Option<wgpu::BindGroup>,
    capture_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    blur_layout: wgpu::BindGroupLayout,
    blur_params: [wgpu::Buffer; 3],
    blur_groups: Option<[wgpu::BindGroup; 3]>,
    atlas: TextAtlas,
    cursor_key: Option<(String, u32)>,
    screen_id: Option<u64>,
    camera_id: Option<u64>,
    backdrop_due: bool,
    pub timing: Timing,
}

const BACKDROP_SMALL: u32 = 24;
const BACKDROP_WIDE: u32 = 128;
const ITEM_SIZE: u64 = std::mem::size_of::<composition::Item>() as u64;

impl Scene {
    /// The pipelines for targets of `format`. The textures take their sizes from what is put in
    /// them: the first video frames, [`Scene::set_wallpaper`].
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Scene {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("omareel scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("omareel scene"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
                texture_entry(6),
                texture_entry(7),
                texture_entry(8),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("omareel scene"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let premultiplied = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("omareel scene"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(premultiplied), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("omareel backdrop blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("omareel backdrop blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(2),
            ],
        });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("omareel backdrop blur"),
            bind_group_layouts: &[&blur_layout],
            push_constant_ranges: &[],
        });
        let blur_pipeline_for = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("omareel backdrop blur"),
                layout: Some(&blur_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &blur_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &blur_shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let capture_pipeline = blur_pipeline_for("fs_capture");
        let blur_pipeline = blur_pipeline_for("fs_blur");
        let uniform = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let blur_size = std::mem::size_of::<BlurParams>() as u64;
        let blur_params = [uniform("capture", blur_size), uniform("blur x", blur_size), uniform("blur y", blur_size)];

        let rgba = wgpu::TextureFormat::Rgba8Unorm;
        let item_capacity = 256;
        Scene {
            format,
            pipeline,
            layout,
            globals: uniform("globals", std::mem::size_of::<Globals>() as u64),
            items: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("items"),
                size: item_capacity * ITEM_SIZE,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            item_capacity,
            item_count: 0,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("linear"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            wallpaper: Texture::new(device, "wallpaper", (0, 0), rgba, false),
            screen: Texture::new(device, "screen", (0, 0), rgba, false),
            camera: Texture::new(device, "camera", (0, 0), rgba, false),
            cursor: Texture::new(device, "cursor", (0, 0), rgba, false),
            text: Texture::new(device, "text", (ATLAS_SIZE, ATLAS_SIZE), wgpu::TextureFormat::R8Unorm, false),
            backdrop_small: Texture::new(device, "backdrop capture", (BACKDROP_SMALL, BACKDROP_SMALL), rgba, true),
            backdrop_wide: Texture::new(device, "backdrop blur x", (BACKDROP_WIDE, BACKDROP_WIDE), rgba, true),
            backdrop: Texture::new(device, "backdrop", (BACKDROP_WIDE, BACKDROP_WIDE), rgba, true),
            bind_group: None,
            capture_pipeline,
            blur_pipeline,
            blur_layout,
            blur_params,
            blur_groups: None,
            atlas: TextAtlas::new("monospace"),
            cursor_key: None,
            screen_id: None,
            camera_id: None,
            backdrop_due: false,
            timing: Timing::default(),
        }
    }

    /// The background image (already scaled, see [`load_wallpaper`]).
    pub fn set_wallpaper(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, image: &image::RgbaImage) {
        self.wallpaper = Texture::new(device, "wallpaper", image.dimensions(), wgpu::TextureFormat::Rgba8Unorm, false);
        self.wallpaper.write(queue, image.as_raw(), 4);
        self.bind_group = None;
    }

    /// The family keystroke labels are set in (the theme's monospace).
    pub fn set_font_family(&mut self, family: &str) {
        self.atlas.set_family(family);
    }

    /// Uploads a video frame unless it is the one already on the GPU.
    pub fn set_video(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, video: Video, frame: &VideoFrame) {
        let (texture, id) = match video {
            Video::Screen => (&mut self.screen, &mut self.screen_id),
            Video::Camera => (&mut self.camera, &mut self.camera_id),
        };
        if *id == Some(frame.id) {
            return;
        }
        let started = Instant::now();
        if texture.size != (frame.width, frame.height) {
            *texture = Texture::new(device, if video == Video::Screen { "screen" } else { "camera" }, (frame.width, frame.height), wgpu::TextureFormat::Rgba8Unorm, false);
            self.bind_group = None;
            self.blur_groups = None;
        }
        texture.write(queue, &frame.pixels, 4);
        *id = Some(frame.id);
        self.timing.upload_ns += started.elapsed().as_nanos() as u64;
    }

    /// Builds the frame's items and writes everything the frame draws from. `pixels_per_unit`
    /// is target pixels per canvas unit (1 for the export).
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        state: &CompositionState,
        project: &Project,
        theme: &RenderTheme,
        pixels_per_unit: f64,
    ) {
        let started = Instant::now();
        let sizes = TextureSizes { wallpaper: self.wallpaper.size, cursor: self.cursor.size };
        let mut frame: Frame = composition::build(state, project, theme, sizes, &mut self.atlas, pixels_per_unit);
        if let Some(key) = frame.cursor_image.clone().filter(|key| self.cursor_key.as_ref() != Some(key)) {
            let image = cursor_image(&key.0, key.1);
            self.cursor = Texture::new(device, "cursor", (key.1, key.1), wgpu::TextureFormat::Rgba8Unorm, false);
            self.cursor.write(queue, &image, 4);
            self.cursor_key = Some(key);
            self.bind_group = None;
            if sizes.cursor.0 == 0 {
                // The first frame with a cursor: build again now that there is a texture.
                let sizes = TextureSizes { wallpaper: self.wallpaper.size, cursor: self.cursor.size };
                frame = composition::build(state, project, theme, sizes, &mut self.atlas, pixels_per_unit);
            }
        }
        if self.atlas.dirty {
            self.text.write(queue, &self.atlas.pixels, 1);
            self.atlas.dirty = false;
        }
        let needed = frame.items.len() as u64;
        if needed > self.item_capacity {
            self.item_capacity = needed.next_power_of_two();
            self.items = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("items"),
                size: self.item_capacity * ITEM_SIZE,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.bind_group = None;
        }
        if !frame.items.is_empty() {
            queue.write_buffer(&self.items, 0, bytemuck::cast_slice(&frame.items));
        }
        self.item_count = frame.items.len() as u32;
        let globals = Globals { canvas: [frame.canvas.0 as f32, frame.canvas.1 as f32], srgb_target: u32::from(self.format.is_srgb()), _pad: 0 };
        queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        self.backdrop_due = false;
        if let Some(capture) = frame.backdrop.filter(|_| self.camera.size.0 > 0) {
            self.write_blur_params(queue, capture);
            self.backdrop_due = true;
        }
        if self.bind_group.is_none() {
            self.bind_group = Some(self.make_bind_group(device));
        }
        if self.blur_groups.is_none() {
            self.blur_groups = Some(self.make_blur_groups(device));
        }
        self.timing.prepare_ns += started.elapsed().as_nanos() as u64;
    }

    fn write_blur_params(&self, queue: &wgpu::Queue, capture: BackdropCapture) {
        let base = BlurParams { crop: capture.crop, direction: [0.0, 0.0], sigma: 1.0, quarter_turns: capture.quarter_turns, flip: u32::from(capture.flip), _pad: [0; 3] };
        let step = 1.0 / BACKDROP_WIDE as f32;
        // MultiEffect blurMax 64 at blur 1 over a canvas-sized item: about 1.8 texels here.
        let sigma = 1.8;
        queue.write_buffer(&self.blur_params[0], 0, bytemuck::bytes_of(&base));
        queue.write_buffer(&self.blur_params[1], 0, bytemuck::bytes_of(&BlurParams { direction: [step, 0.0], sigma, ..base }));
        queue.write_buffer(&self.blur_params[2], 0, bytemuck::bytes_of(&BlurParams { direction: [0.0, step], sigma, ..base }));
    }

    fn make_bind_group(&self, device: &wgpu::Device) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("omareel scene"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.globals.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.items.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.wallpaper.view) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.screen.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&self.camera.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&self.cursor.view) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&self.text.view) },
                wgpu::BindGroupEntry { binding: 8, resource: wgpu::BindingResource::TextureView(&self.backdrop.view) },
            ],
        })
    }

    fn make_blur_groups(&self, device: &wgpu::Device) -> [wgpu::BindGroup; 3] {
        let group = |params: &wgpu::Buffer, source: &Texture| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("omareel backdrop blur"),
                layout: &self.blur_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&source.view) },
                ],
            })
        };
        [
            group(&self.blur_params[0], &self.camera),
            group(&self.blur_params[1], &self.backdrop_small),
            group(&self.blur_params[2], &self.backdrop_wide),
        ]
    }

    /// Encodes the frame prepared last. `viewport` is where the canvas goes on the target, in
    /// pixels, and `scissor` what of the target may be drawn; `clear` clears the target first.
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        viewport: [f32; 4],
        scissor: [u32; 4],
        clear: Option<wgpu::Color>,
    ) {
        let (Some(bind_group), Some(blur_groups)) = (&self.bind_group, &self.blur_groups) else { return };
        if self.backdrop_due {
            let passes: [(&wgpu::RenderPipeline, &Texture, &wgpu::BindGroup); 3] = [
                (&self.capture_pipeline, &self.backdrop_small, &blur_groups[0]),
                (&self.blur_pipeline, &self.backdrop_wide, &blur_groups[1]),
                (&self.blur_pipeline, &self.backdrop, &blur_groups[2]),
            ];
            for (pipeline, target, group) in passes {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("omareel backdrop"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("omareel scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_viewport(viewport[0], viewport[1], viewport[2], viewport[3], 0.0, 1.0);
        pass.set_scissor_rect(scissor[0], scissor[1], scissor[2], scissor[3]);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..4, 0..self.item_count);
    }
}

/// The cursor SVG for a style, rasterised at `size` pixels square (QML's `sourceSize`).
fn cursor_image(style: &str, size: u32) -> Vec<u8> {
    let svg = match style {
        "dark-arrow" => include_str!("../../../../assets/cursors/arrow-dark.svg"),
        "hand" => include_str!("../../../../assets/cursors/pointer.svg"),
        _ => include_str!("../../../../assets/cursors/arrow.svg"),
    };
    let Ok(tree) = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()) else {
        return vec![0; (size * size * 4) as usize];
    };
    let Some(mut pixmap) = resvg::tiny_skia::Pixmap::new(size, size) else { return vec![0; (size * size * 4) as usize] };
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.take()
}

/// The background image of a project, scaled once to cover `width` x `height` (QML's
/// PreserveAspectCrop draws the full image scaled down by the GPU each frame).
pub fn load_wallpaper(path: &str, width: u32, height: u32) -> Option<image::RgbaImage> {
    if path.is_empty() {
        return None;
    }
    let image = image::open(path).ok()?.to_rgba8();
    let (iw, ih) = image.dimensions();
    let scale = (f64::from(width) / f64::from(iw)).max(f64::from(height) / f64::from(ih));
    if scale >= 1.0 {
        return Some(image);
    }
    let w = (f64::from(iw) * scale).round().max(1.0) as u32;
    let h = (f64::from(ih) * scale).round().max(1.0) as u32;
    Some(image::imageops::resize(&image, w, h, image::imageops::FilterType::Triangle))
}
