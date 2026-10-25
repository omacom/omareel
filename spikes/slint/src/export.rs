//! The offscreen export: omareel_core's `ExportJob` (the same decode and encode threads and
//! ffmpeg arguments as `omareel export`) around Slint rendering the composition without a
//! window. Slint has no offscreen API on its winit backend, so this installs a small custom
//! platform whose one window renders with `FemtoVGWGPURenderer` (the femtovg renderer Slint
//! uses on screen, on wgpu) into a texture on a headless wgpu device; each frame is copied to a
//! buffer, mapped, and handed to the encoder, with a few readbacks in flight.

use crate::scene::{self, FrameState, Models, Upload, VideoLayer};
use crate::{Comp, ExportScene};
use omareel_core::export::{ExportJob, ExportOptions};
use slint::ComponentHandle;
use slint::platform::femtovg_renderer::FemtoVGWGPURenderer;
use slint::platform::{Platform, PlatformError, Renderer, WindowAdapter};
use slint::wgpu_30::wgpu;
use slint::PhysicalSize;
use std::cell::Cell;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// A window that is only a size and a renderer.
struct OffscreenWindow {
    window: slint::Window,
    renderer: FemtoVGWGPURenderer,
    size: Cell<PhysicalSize>,
}

impl WindowAdapter for OffscreenWindow {
    fn window(&self) -> &slint::Window {
        &self.window
    }
    fn size(&self) -> PhysicalSize {
        self.size.get()
    }
    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }
    fn set_size(&self, size: slint::WindowSize) {
        self.size.set(size.to_physical(1.0));
    }
}

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
}

pub fn headless_gpu() -> Result<Gpu, String> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        flags: wgpu::InstanceFlags::from_build_config().with_env(),
        memory_budget_thresholds: Default::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
        display: None,
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .map_err(|e| format!("no GPU adapter: {e}"))?;
    let adapter_name = format!("{} ({:?})", adapter.get_info().name, adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("export"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .map_err(|e| format!("no GPU device: {e}"))?;
    Ok(Gpu { instance, device, queue, adapter_name })
}

struct OffscreenPlatform {
    window: Rc<OffscreenWindow>,
}

impl Platform for OffscreenPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

/// One frame's readback buffer.
struct Readback {
    buffer: wgpu::Buffer,
    submission: Option<wgpu::SubmissionIndex>,
}

pub struct Options {
    pub bundle: String,
    pub output: String,
    pub quality: String,
    pub width: i32,
    pub fps: i32,
    pub upload: Upload,
    pub timing: bool,
    /// Writes these frames as PNG next to the output, for looking at.
    pub png_frames: Vec<i32>,
}

pub fn run(options: Options) -> Result<(), String> {
    let total_timer = Instant::now();
    let cancelled = Arc::new(AtomicBool::new(false));
    let mut job = ExportJob::prepare(
        &ExportOptions {
            bundle_path: options.bundle.clone(),
            output_path: options.output.clone(),
            fps: options.fps,
            width: options.width,
            quality: options.quality.clone(),
            timing: options.timing,
            ..Default::default()
        },
        cancelled,
    )?;
    let (width, height) = (job.width() as u32, job.height() as u32);

    let gpu = headless_gpu()?;
    eprintln!("export: {width}x{height}, {} frames, on {}", job.total_frames(), gpu.adapter_name);
    let window = Rc::new_cyclic(|weak: &Weak<OffscreenWindow>| OffscreenWindow {
        window: slint::Window::new(weak.clone() as Weak<dyn WindowAdapter>),
        renderer: FemtoVGWGPURenderer::new(gpu.instance.clone(), gpu.device.clone(), gpu.queue.clone())
            .expect("femtovg on wgpu"),
        size: Cell::new(PhysicalSize::new(width, height)),
    });
    slint::platform::set_platform(Box::new(OffscreenPlatform { window: window.clone() }))
        .map_err(|e| format!("Slint platform: {e}"))?;
    let scene = ExportScene::new().map_err(|e| e.to_string())?;
    scene.window().set_size(PhysicalSize::new(width, height));
    scene.show().map_err(|e| e.to_string())?;

    let comp = scene.global::<Comp>();
    let state = job.state().clone();
    let recording = scene::Recording {
        source_width: state.source_width,
        source_height: state.source_height,
        capture_scale: state.capture_scale,
        camera: state.camera_available.then_some((state.camera_source_width, state.camera_source_height)),
    };
    let background = scene::load_background(&job.background_image());
    scene::apply_project(&comp, job.project(), &recording, background.as_ref());
    let mut models = Models::install(&comp);

    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("export target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    // Rows of a buffer copy are padded to 256 bytes.
    let row_bytes = width * 4;
    let padded_row = row_bytes.div_ceil(256) * 256;
    const IN_FLIGHT: usize = 3;
    let mut readbacks: Vec<Readback> = (0..IN_FLIGHT)
        .map(|_| Readback {
            buffer: gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("readback"),
                size: u64::from(padded_row) * u64::from(height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
            submission: None,
        })
        .collect();

    // Software-rendering frames are top-down, which wgpu's are too: no vflip.
    job.start(true)?;

    let mut screen = VideoLayer::new("screen");
    let mut camera = VideoLayer::new("camera");
    let mut last_screen: Option<Arc<Vec<u8>>> = None;
    let mut last_camera: Option<Arc<Vec<u8>>> = None;
    let (mut upload_ns, mut render_ns, mut readback_ns) = (0u128, 0u128, 0u128);
    let mut error: Option<String> = None;

    let take = |readback: &mut Readback, readback_ns: &mut u128| -> Result<Vec<u8>, String> {
        let started = Instant::now();
        let slice = readback.buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        gpu.device
            .poll(wgpu::PollType::Wait { submission_index: readback.submission.take(), timeout: None })
            .map_err(|e| format!("GPU: {e}"))?;
        receiver.recv().map_err(|e| e.to_string())?.map_err(|e| format!("readback: {e}"))?;
        let mapped = slice.get_mapped_range().map_err(|e| format!("readback: {e:?}"))?;
        let mut bytes = vec![0u8; (row_bytes * height) as usize];
        for (row, dst) in bytes.chunks_exact_mut(row_bytes as usize).enumerate() {
            let start = row * padded_row as usize;
            dst.copy_from_slice(&mapped[start..start + row_bytes as usize]);
        }
        drop(mapped);
        readback.buffer.unmap();
        *readback_ns += started.elapsed().as_nanos();
        Ok(bytes)
    };
    let write = |job: &mut ExportJob, bytes: Vec<u8>| -> Result<(), String> {
        while !job.wait_encode_space(20) {}
        job.encode(Box::new(bytes))
    };

    let total = job.total_frames();
    let mut pending: std::collections::VecDeque<(usize, i32)> = Default::default();
    for frame in 0..total {
        let images = match job.next_frame(frame) {
            Ok(images) => images,
            Err(e) => {
                error = Some(e);
                break;
            }
        };
        let started = Instant::now();
        let state = job.state();
        models.apply(&comp, &FrameState::export(state));
        let device_queue = Some((&gpu.device, &gpu.queue));
        if let Some(image) = images.screen.filter(|i| last_screen.as_ref().is_none_or(|last| !Arc::ptr_eq(last, &i.pixels))) {
            comp.set_screen(screen.image(options.upload, device_queue, &image.pixels, image.width as u32, image.height as u32));
            last_screen = Some(image.pixels);
        }
        if let Some(image) = images.camera.filter(|i| last_camera.as_ref().is_none_or(|last| !Arc::ptr_eq(last, &i.pixels))) {
            comp.set_camera_frame(camera.image(options.upload, device_queue, &image.pixels, image.width as u32, image.height as u32));
            if state.layout.camera_full > 0.0 {
                comp.set_camera_blurred(scene::blurred_camera(&image.pixels, image.width as usize, image.height as usize, 1.0));
            }
            last_camera = Some(image.pixels);
        }
        upload_ns += started.elapsed().as_nanos();

        let started = Instant::now();
        slint::platform::update_timers_and_animations();
        window.renderer.render_to_texture(&target).map_err(|e| format!("render: {e}"))?;
        let slot = frame as usize % IN_FLIGHT;
        if readbacks[slot].submission.is_some() {
            // The ring is full: the oldest frame goes to the encoder first.
            let (oldest, oldest_frame) = pending.pop_front().expect("pending readback");
            let bytes = take(&mut readbacks[oldest], &mut readback_ns)?;
            maybe_png(&options, oldest_frame, &bytes, width, height);
            if let Err(e) = write(&mut job, bytes) {
                error = Some(e);
                break;
            }
        }
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &readbacks[slot].buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        readbacks[slot].submission = Some(gpu.queue.submit([encoder.finish()]));
        pending.push_back((slot, frame));
        render_ns += started.elapsed().as_nanos();
    }
    job.finish_decoder();
    if error.is_none() {
        while let Some((slot, frame)) = pending.pop_front() {
            let bytes = take(&mut readbacks[slot], &mut readback_ns)?;
            maybe_png(&options, frame, &bytes, width, height);
            if let Err(e) = write(&mut job, bytes) {
                error = Some(e);
                break;
            }
        }
    }
    job.close_encoder(error.is_some());
    while !job.encoder_done() {
        job.wait_encoder_done(20);
    }
    job.join_encoder();
    if let Some(error) = error {
        return Err(error);
    }
    job.finish()?;
    if options.timing {
        eprint!("{}", job.timing_report(upload_ns as i64, render_ns as i64, readback_ns as i64));
    }
    let seconds = total_timer.elapsed().as_secs_f64();
    eprintln!("exported {} frames in {seconds:.2} s ({:.1} frames/s) to {}", total, f64::from(total) / seconds, options.output);
    Ok(())
}

fn maybe_png(options: &Options, frame: i32, bytes: &[u8], width: u32, height: u32) {
    if !options.png_frames.contains(&frame) {
        return;
    }
    let path = format!("{}.frame{frame:04}.png", options.output);
    if let Some(image) = image::RgbaImage::from_raw(width, height, bytes.to_vec()) {
        if let Err(e) = image.save(&path) {
            eprintln!("{path}: {e}");
        }
    }
}
