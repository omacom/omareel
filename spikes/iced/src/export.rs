//! The offscreen export: omareel_core's `ExportJob` (decoding, the per-frame composition state,
//! the ffmpeg encoder with `omareel export`'s arguments) around the scene renderer on a headless
//! wgpu device, with two readback buffers in flight as the Qt exporter has.

use crate::scene::composition::RenderTheme;
use crate::scene::{Scene, Video, VideoFrame, load_wallpaper};
use iced::wgpu;
use omareel_core::export::{ExportJob, ExportOptions, Image};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub struct Options {
    pub bundle: String,
    pub output: String,
    pub quality: String,
    pub timing: bool,
    /// Output times whose frames are also written as PNGs next to the output.
    pub png_at: Vec<f64>,
}

/// A headless device on the default adapter.
pub fn headless_device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::PRIMARY, ..Default::default() });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|error| format!("No GPU adapter: {error}"))?;
    let name = format!("{} ({:?})", adapter.get_info().name, adapter.get_info().backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("omareel export"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default().using_resolution(adapter.limits()),
        ..Default::default()
    }))
    .map_err(|error| format!("No GPU device: {error}"))?;
    Ok((device, queue, name))
}

struct Readback {
    buffer: wgpu::Buffer,
    frame: Option<(i32, wgpu::SubmissionIndex)>,
    mapped: Arc<AtomicBool>,
}

struct Source {
    last: Option<Arc<Vec<u8>>>,
    id: u64,
}

impl Source {
    /// The frame for the scene: a new id only when the decoder handed over new pixels.
    fn frame(&mut self, image: &Image) -> VideoFrame {
        if !self.last.as_ref().is_some_and(|last| Arc::ptr_eq(last, &image.pixels)) {
            self.id += 1;
            self.last = Some(Arc::clone(&image.pixels));
        }
        VideoFrame { id: self.id, width: image.width as u32, height: image.height as u32, pixels: Arc::clone(&image.pixels) }
    }
}

pub fn run(options: &Options) -> Result<(), String> {
    let total_timer = Instant::now();
    let job_options = ExportOptions {
        bundle_path: options.bundle.clone(),
        output_path: options.output.clone(),
        quality: options.quality.clone(),
        timing: options.timing,
        ..ExportOptions::default()
    };
    let mut job = ExportJob::prepare(&job_options, Arc::new(AtomicBool::new(false)))?;
    if job.is_gif() {
        return Err("The spike exports MP4 only".into());
    }
    let (width, height) = (job.width() as u32, job.height() as u32);
    let project = job.project().clone();
    let theme = RenderTheme::load();
    let (device, queue, adapter) = headless_device()?;
    eprintln!("export: {width}x{height} at {} frames on {adapter}", job.total_frames());
    let wallpaper = load_wallpaper(&job.background_image(), width, height);
    let mut scene = Scene::new(&device, wgpu::TextureFormat::Rgba8Unorm);
    scene.set_font_family(&theme.mono_family);
    if let Some(wallpaper) = &wallpaper {
        scene.set_wallpaper(&device, &queue, wallpaper);
    }
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("export target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let row = width * 4;
    let padded_row = row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let mut readbacks: Vec<Readback> = (0..2)
        .map(|_| Readback {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("export readback"),
                size: u64::from(padded_row * height),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            frame: None,
            mapped: Arc::new(AtomicBool::new(false)),
        })
        .collect();
    let png_frames: Vec<i32> = options.png_at.iter().map(|t| (t * 60.0).round() as i32).collect();
    let mut readback_ns = 0u64;
    let mut render_ns = 0u64;

    // Frames come back top-down, so the encoder needs no vflip: the "software" path's arguments.
    job.start(true)?;
    let mut screen = Source { last: None, id: 0 };
    let mut camera = Source { last: None, id: 0 };
    let total = job.total_frames();
    let mut error: Option<String> = None;

    let collect = |job: &mut ExportJob, slot: &mut Readback, readback_ns: &mut u64| -> Result<(), String> {
        let Some((frame, submission)) = slot.frame.take() else { return Ok(()) };
        let started = Instant::now();
        let range = slot.buffer.slice(..);
        while !slot.mapped.load(Ordering::Acquire) {
            device
                .poll(wgpu::PollType::Wait { submission_index: Some(submission.clone()), timeout: None })
                .map_err(|error| format!("GPU readback failed: {error}"))?;
        }
        slot.mapped.store(false, Ordering::Release);
        let bytes = {
            let mapped = range.get_mapped_range();
            if padded_row == row {
                mapped.to_vec()
            } else {
                let mut bytes = Vec::with_capacity((row * height) as usize);
                for line in mapped.chunks(padded_row as usize) {
                    bytes.extend_from_slice(&line[..row as usize]);
                }
                bytes
            }
        };
        slot.buffer.unmap();
        *readback_ns += started.elapsed().as_nanos() as u64;
        if png_frames.contains(&frame) {
            let path = format!("{}.{frame:04}.png", options.output);
            image::save_buffer(&path, &bytes, width, height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
        }
        while !job.wait_encode_space(20) {}
        job.encode(Box::new(bytes))
    };

    for frame in 0..total {
        let images = match job.next_frame(frame) {
            Ok(images) => images,
            Err(message) => {
                error = Some(message);
                break;
            }
        };
        // The slot this frame reads back into holds the frame from two frames ago.
        let slot = frame as usize % readbacks.len();
        if let Err(message) = collect(&mut job, &mut readbacks[slot], &mut readback_ns) {
            error = Some(message);
            break;
        }
        if let Some(image) = &images.screen {
            scene.set_video(&device, &queue, Video::Screen, &screen.frame(image));
        }
        if let Some(image) = &images.camera {
            scene.set_video(&device, &queue, Video::Camera, &camera.frame(image));
        }
        let started = Instant::now();
        let state = job.state().clone();
        scene.prepare(&device, &queue, &state, &project, &theme, 1.0);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("export frame") });
        scene.render(&mut encoder, &view, [0.0, 0.0, width as f32, height as f32], [0, 0, width, height], Some(wgpu::Color::BLACK));
        encoder.copy_texture_to_buffer(
            target.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readbacks[slot].buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: None },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        let submission = queue.submit([encoder.finish()]);
        let mapped = Arc::clone(&readbacks[slot].mapped);
        readbacks[slot].buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            if result.is_ok() {
                mapped.store(true, Ordering::Release);
            }
        });
        readbacks[slot].frame = Some((frame, submission));
        render_ns += started.elapsed().as_nanos() as u64;
        if frame % 60 == 59 {
            eprint!("\rframe {}/{total}", frame + 1);
        }
    }
    eprintln!();
    job.finish_decoder();
    if error.is_none() {
        // The last frames still on the GPU, oldest first.
        let mut pending: Vec<usize> = (0..readbacks.len()).filter(|&i| readbacks[i].frame.is_some()).collect();
        pending.sort_by_key(|&i| readbacks[i].frame.as_ref().map(|(frame, _)| *frame));
        for i in pending {
            if let Err(message) = collect(&mut job, &mut readbacks[i], &mut readback_ns) {
                error = Some(message);
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
    let seconds = total_timer.elapsed().as_secs_f64();
    if options.timing {
        let upload = scene.timing.upload_ns as i64;
        eprint!("{}", job.timing_report(upload, render_ns as i64, readback_ns as i64));
        eprintln!("  scene prepare: {:.3} ms/frame", scene.timing.prepare_ns as f64 / 1e6 / f64::from(total));
    }
    println!("exported {} frames in {seconds:.2} s ({:.1} frames/s) to {}", total, f64::from(total) / seconds, options.output);
    Ok(())
}
