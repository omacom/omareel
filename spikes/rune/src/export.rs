//! The offscreen export: omareel_core's `ExportJob` (the same decode and encode threads and
//! ffmpeg arguments as `omareel export`) around a headless Rune `Runtime` and `Renderer`.
//!
//! Rune's headless path is public API: a `Runtime` builds the frame (layout, instances, text) from
//! the same view the preview uses, and a `Renderer` on our own wgpu device draws it into a
//! texture. Each frame is copied to a buffer, mapped, and handed to the encoder, with a few
//! readbacks in flight. Decoded frames reach the GPU through `rune::Pixels` (RGBA, as the
//! export's decoder makes them).

use crate::comp::{self, FrameState, Media, Settings, Video};
use omareel_core::export::{ExportJob, ExportOptions};
use rune::render::{Gpu, Renderer};
use rune::{Cx, El, Env, Pixels, Runtime};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

pub struct Options {
    pub bundle: String,
    pub output: String,
    pub quality: String,
    pub width: i32,
    pub fps: i32,
    pub timing: bool,
    pub gpu_timing: bool,
    /// Writes these frames as PNG next to the output, for looking at.
    pub png_frames: Vec<i32>,
}

struct Scene {
    settings: Settings,
    media: Media,
    frame: FrameState,
    size: (f32, f32),
}

fn view(s: &Scene, _: &mut Cx<Scene>) -> El {
    // SPIKE_VIEW=fill<n>: n full-frame opaque fills instead of the composition, to price a quad.
    if let Some(n) = std::env::var("SPIKE_VIEW").ok().and_then(|v| v.strip_prefix("fill").and_then(|n| n.parse::<usize>().ok())) {
        let (w, h) = s.size;
        let mut root = rune::el().size(w, h).fill(rune::Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 });
        for i in 1..n {
            root = root.child(rune::el().absolute(0.0, 0.0).size(w, h).fill(rune::Color { r: 0.1 * i as f32 % 1.0, g: 0.2, b: 0.3, a: 1.0 }));
        }
        return root;
    }
    let (w, h) = s.size;
    let base = || rune::el().size(w, h).fill(rune::Color::BLACK);
    match std::env::var("SPIKE_VIEW").as_deref() {
        // Pieces of the composition alone, to find what costs a layer.
        Ok("video") => base().child(rune::el().absolute(100.0, 100.0).size(1600.0, 900.0).radius(12.0).pixels_fit(match &s.media.screen { comp::Video::Pixels(p) => p, _ => unreachable!() }, rune::SurfaceFit::Fill)),
        Ok("video-square") => base().child(rune::el().absolute(100.0, 100.0).size(1600.0, 900.0).pixels_fit(match &s.media.screen { comp::Video::Pixels(p) => p, _ => unreachable!() }, rune::SurfaceFit::Fill)),
        Ok("cursor") => base().child(rune::el().absolute(100.0, 100.0).size(48.0, 48.0).pixels(s.media.cursors[0].pixels()).origin(0.0, 0.0)),
        Ok("stage") => base().child(rune::el().absolute(0.0, 0.0).size(w, h).child(rune::el().absolute(100.0, 100.0).size(400.0, 400.0).fill(rune::Color::WHITE))),
        Ok("clip") => base().clip_soft(0.0).child(rune::el().absolute(100.0, 100.0).size(400.0, 400.0).fill(rune::Color::WHITE)),
        _ => comp::composition(&s.settings, &s.media, &s.frame, w, h, true),
    }
}

struct Readback {
    buffer: wgpu::Buffer,
    submission: Option<wgpu::SubmissionIndex>,
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

    // `--gpu-timing`: a timestamp query around each frame's pass, read back at the end.
    let gpu_timing = options.gpu_timing;
    let features = if gpu_timing { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() };
    let gpu = Gpu::try_with_features(Gpu::instance(), None, features)?;
    let timestamps = gpu_timing.then(|| {
        let count = (job.total_frames().max(1) as u32) * 2;
        let set = gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("frame passes"), ty: wgpu::QueryType::Timestamp, count });
        let resolve = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps"),
            size: u64::from(count) * 8,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamps read"),
            size: u64::from(count) * 8,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        (set, resolve, read, count)
    });
    let info = gpu.adapter.get_info();
    eprintln!("export: {width}x{height}, {} frames, on {} ({:?})", job.total_frames(), info.name, info.backend);

    let state = job.state().clone();
    let mut settings = Settings::new(
        job.project(),
        (state.source_width, state.source_height),
        state.capture_scale,
        state.camera_available.then_some((state.camera_source_width, state.camera_source_height)),
    );
    let experiment = comp::apply_experiments(&mut settings);
    let cache = rune::ImageCache::new(Arc::new(|| {}));
    let background_path = if experiment.contains("nobg") { String::new() } else { job.background_image() };
    let background = (!background_path.is_empty()).then(|| cache.file(&background_path, 0.0, 1.0));
    let cursors = comp::cursor_images(&cache, 96.0, 2.0);
    // The headless run is deterministic: wait for the decodes.
    if let Some(image) = &background {
        image.wait().map_err(|e| format!("background {background_path}: {e:?}"))?;
    }
    for image in &cursors {
        image.wait().map_err(|e| format!("cursor: {e:?}"))?;
    }
    let screen = Pixels::new();
    let camera = Pixels::new();
    let media = Media { screen: Video::Pixels(screen.clone()), camera: Video::Pixels(camera.clone()), background, cursors };
    let size = (width as f32, height as f32);
    let scene = Scene { settings, media, frame: FrameState::default(), size };
    let mut rt = Runtime::new(scene, view, size, Env::new(true, 1.0));
    // Qt Quick blends in sRGB-encoded space, as Rune's windows do by default.
    let srgb = std::env::var_os("SPIKE_LINEAR").is_none();
    rt.set_srgb_blend(srgb);
    let format = if srgb { wgpu::TextureFormat::Rgba8Unorm } else { wgpu::TextureFormat::Rgba8UnormSrgb };
    let mut renderer = Renderer::new(&gpu.device, format);
    renderer.set_srgb_blend(srgb);

    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("export target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
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

    job.start(true)?;

    // The last frames handed over, by identity: a `Weak` keeps the allocation (not the pixels)
    // so the address can't be reused, and lets `Pixels::set` take the bytes without a copy
    // when the job holds no other reference.
    let mut last_screen: Option<std::sync::Weak<Vec<u8>>> = None;
    let mut last_camera: Option<std::sync::Weak<Vec<u8>>> = None;
    let own = |pixels: Arc<Vec<u8>>| Arc::try_unwrap(pixels).unwrap_or_else(|shared| shared.as_ref().clone());
    let (mut upload_ns, mut render_ns, mut readback_ns, mut build_ns) = (0u128, 0u128, 0u128, 0u128);
    let mut error: Option<String> = None;
    let dt = 1.0 / f64::from(job.total_frames().max(1)).max(1.0);
    let _ = dt;
    let frame_dt = 1.0 / 60.0;

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
    let mut pending: VecDeque<(usize, i32)> = VecDeque::new();
    for frame in 0..total {
        let images = match job.next_frame(frame) {
            Ok(images) => images,
            Err(e) => {
                error = Some(e);
                break;
            }
        };
        let started = Instant::now();
        // `Pixels::set` takes the bytes; a frame the decoder repeats keeps the picture as is.
        if let Some(image) = images.screen.filter(|i| last_screen.as_ref().is_none_or(|last| last.as_ptr() != Arc::as_ptr(&i.pixels))) {
            last_screen = Some(Arc::downgrade(&image.pixels));
            screen.set(image.width as u32, image.height as u32, own(image.pixels));
        }
        if let Some(image) = images.camera.filter(|i| last_camera.as_ref().is_none_or(|last| last.as_ptr() != Arc::as_ptr(&i.pixels))) {
            last_camera = Some(Arc::downgrade(&image.pixels));
            camera.set(image.width as u32, image.height as u32, own(image.pixels));
        }
        upload_ns += started.elapsed().as_nanos();

        let started = Instant::now();
        let state = FrameState::export(job.state());
        if std::env::var_os("SPIKE_DEBUG").is_some() && options.png_frames.contains(&frame) {
            eprintln!("frame {frame}: pills {:?}", state.pills.iter().map(|p| (p.keys.clone(), p.opacity)).collect::<Vec<_>>());
        }
        rt.update(move |s, tx| {
            s.frame = state;
            tx.instant();
        });
        rt.frame(frame_dt);
        build_ns += started.elapsed().as_nanos();

        let started = Instant::now();
        renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
        renderer.prepare_surfaces(&gpu.device, &gpu.queue, &rt.surfaces, (width, height));
        renderer.prepare(&gpu.device, &gpu.queue, &mut rt.text.atlas, &rt.instances, (width, height));
        let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("export frame") });
        match &timestamps {
            Some((set, ..)) => renderer.draw_timed(
                &mut encoder,
                &target_view,
                rt.clear_color(),
                Some(wgpu::RenderPassTimestampWrites {
                    query_set: set,
                    beginning_of_pass_write_index: Some(frame as u32 * 2),
                    end_of_pass_write_index: Some(frame as u32 * 2 + 1),
                }),
            ),
            None => renderer.draw(&mut encoder, &target_view, rt.clear_color()),
        }
        if options.gpu_timing && options.png_frames.contains(&frame) {
            let l = renderer.layer_stats();
            eprintln!("frame {frame}: {} instances, {} layer targets, {} extra passes", rt.instances.len(), l.targets, l.extra_passes);
            for (i, inst) in rt.instances.iter().enumerate() {
                eprintln!("  {i}: kind {} rect {:?} fill {:?}", inst.params[1], inst.rect, inst.fill);
            }
        }
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
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &readbacks[slot].buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded_row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        readbacks[slot].submission = Some(renderer.submit(&gpu.queue, encoder));
        pending.push_back((slot, frame));
        render_ns += started.elapsed().as_nanos();
    }
    if let Some((set, resolve, read, count)) = &timestamps {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.resolve_query_set(set, 0..*count, resolve, 0);
        encoder.copy_buffer_to_buffer(resolve, 0, read, 0, u64::from(*count) * 8);
        gpu.queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        if let Ok(data) = slice.get_mapped_range() {
            let ticks: Vec<u64> = data.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c)).collect();
            let period = f64::from(gpu.queue.get_timestamp_period());
            let mut ms: Vec<f64> = ticks.as_chunks::<2>().0.iter().filter(|p| p[1] > p[0]).map(|p| (p[1] - p[0]) as f64 * period / 1e6).collect();
            ms.sort_by(f64::total_cmp);
            let pick = |q: f64| ms.get(((ms.len().max(1) - 1) as f64 * q).round() as usize).copied().unwrap_or(0.0);
            eprintln!("GPU frame pass: median {:.2} ms, p95 {:.2} ms, max {:.2} ms over {} frames", pick(0.5), pick(0.95), pick(1.0), ms.len());
        }
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
        eprint!("{}", job.timing_report(upload_ns as i64, (build_ns + render_ns) as i64, readback_ns as i64));
        let per = |ns: u128| ns as f64 / 1e6 / f64::from(total.max(1));
        eprintln!(
            "rune per frame: pixels {:.2} ms, view+layout {:.2} ms, prepare+encode {:.2} ms, readback {:.2} ms",
            per(upload_ns),
            per(build_ns),
            per(render_ns),
            per(readback_ns)
        );
    }
    let seconds = total_timer.elapsed().as_secs_f64();
    eprintln!("exported {} frames in {seconds:.2} s ({:.1} frames/s) to {}", total, f64::from(total) / seconds, options.output);
    drop(cache);
    Ok(())
}

fn maybe_png(options: &Options, frame: i32, bytes: &[u8], width: u32, height: u32) {
    if !options.png_frames.contains(&frame) {
        return;
    }
    let path = format!("{}.frame{frame:04}.png", options.output);
    if let Some(image) = image::RgbaImage::from_raw(width, height, bytes.to_vec())
        && let Err(e) = image.save(&path)
    {
        eprintln!("{path}: {e}");
    }
}
