//! The editor's preview: a winit window drawn by Slint's femtovg renderer on wgpu, playing the
//! composition at the display's rate with play/pause, a scrub bar and the time.
//!
//! The playhead moves in the window's rendering notifier: just before each frame is drawn
//! (`RenderingState::BeforeRendering`) it takes the clock, asks the `Document` for the
//! composition at that time (`preview_composition`, the editor's own per-frame state), picks the
//! decoded screen and camera frames for it, uploads them, and sets the `Comp` global; Slint then
//! draws with those values. While playing, an `animation-tick()` binding keeps Slint rendering
//! every frame.

use crate::scene::{self, FrameState, Models, Upload, VideoLayer};
use crate::video::{GpuSink, Pixels, Stream};
use crate::{Comp, PreviewWindow};
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use slint::wgpu_30::wgpu;
use slint::{ComponentHandle, GraphicsAPI, RenderingState};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

pub struct Options {
    pub bundle: String,
    pub upload: Upload,
    /// The canvas's tallest size in pixels (the Qt editor's 620); 0 for the whole pane.
    pub canvas_height: f32,
    /// Decode the screen at this height (scaled by ffmpeg); 0 for the recording's size.
    pub decode_height: i32,
    /// Play from the start after a second, for this many seconds, then write `stats` and quit.
    pub measure: Option<f64>,
    pub stats: Option<String>,
}

#[derive(Default)]
struct Stats {
    frames: Vec<Instant>,
    /// Time spent in the notifier per frame (state, frame choice, uploads), ns.
    prepare_ns: Vec<u64>,
    /// BeforeRendering to AfterRendering (Slint's render and submit), ns.
    render_ns: Vec<u64>,
    shown: u64,
    skipped: u64,
    starved: u64,
}

struct Player {
    document: Document,
    upload: Upload,
    gpu: Option<(wgpu::Device, wgpu::Queue)>,
    models: Option<Models>,
    screen_layer: VideoLayer,
    camera_layer: VideoLayer,
    screen: Option<Stream>,
    camera: Option<Stream>,
    screen_size: (i32, i32),
    camera_path: String,
    playing: bool,
    /// The output time the clock started from, and when (None until the decoders have the
    /// first frame: playback pre-rolls).
    clock: Option<(f64, Instant)>,
    origin: f64,
    pending_seek: Option<f64>,
    last_restart: Instant,
    /// Frames are wanted (playing, or a seek waiting for its frame).
    busy: bool,
    measuring: bool,
    measure_started: Option<Instant>,
    measure_seconds: f64,
    stats: Stats,
    stats_path: Option<String>,
    render_started: Option<Instant>,
}

impl Player {
    fn restart_decoders(&mut self, output_time: f64) {
        self.document.set_output_position(output_time);
        let source = self.document.source_position();
        let duration = self.document.source_duration();
        let (w, h) = self.screen_size;
        let scale = (w, h) != (self.document.source_width(), self.document.source_height());
        // Fewer 4K frames fit in memory; a few are plenty to ride out ffmpeg's jitter.
        let ahead = if w * h > 2_500_000 { 4 } else { 8 };
        let sink = |keep_cpu_copy| match (&self.gpu, self.upload) {
            (Some((device, _)), Upload::Wgpu) => Some(GpuSink { device: device.clone(), keep_cpu_copy }),
            _ => None,
        };
        self.screen =
            Some(Stream::start(self.document.video_path(), source, duration, self.document.fps(), w, h, scale, ahead, sink(false)));
        self.camera = None;
        if let Some(sync) = self.document.camera_sync(source) {
            let (cw, ch) = (self.document.camera_source_width(), self.document.camera_source_height());
            let camera_fps = self.document.recording().camera.map_or(30.0, |c| c.fps);
            let camera_duration = self.document.recording().camera.map_or(duration, |c| c.duration);
            let start = sync.target_ms as f64 / 1000.0;
            self.camera = Some(Stream::start(&self.camera_path, start, camera_duration, camera_fps, cw, ch, false, 6, sink(true)));
        }
        self.last_restart = Instant::now();
        self.busy = true;
    }

    fn seek(&mut self, output_time: f64) {
        self.pending_seek = Some(output_time);
        self.document.set_output_position(output_time);
        self.busy = true;
    }

    fn toggle(&mut self) {
        if self.playing {
            self.playing = false;
            self.clock = None;
            return;
        }
        let mut position = self.document.output_position();
        if position >= self.document.duration() - 0.01 {
            position = 0.0;
        }
        self.origin = position;
        self.playing = true;
        self.clock = None;
        self.restart_decoders(position);
    }

    /// The frame about to be drawn: the playhead, the composition and the pictures.
    fn prepare(&mut self, app: &PreviewWindow) {
        let started = Instant::now();
        if let Some(target) = self.pending_seek {
            if self.last_restart.elapsed().as_secs_f64() > 0.035 || self.screen.is_none() {
                self.pending_seek = None;
                if self.playing {
                    self.origin = target;
                    self.clock = None;
                }
                self.restart_decoders(target);
            }
        }
        // The clock starts once the first frame is there.
        if self.playing && self.clock.is_none() && self.screen.as_ref().is_some_and(|s| s.current.is_some() || !s.starved(-1.0)) {
            self.clock = Some((self.origin, Instant::now()));
            if self.measuring && self.measure_started.is_none() {
                self.measure_started = Some(Instant::now());
            }
        }
        if let Some((origin, since)) = self.clock {
            let mut output = origin + since.elapsed().as_secs_f64();
            if output >= self.document.duration() {
                output = self.document.duration();
                self.playing = false;
                self.clock = None;
            }
            self.document.set_output_position(output);
        }
        let output = self.document.output_position();
        let source = self.document.source_position();
        let composition = self.document.preview_composition(false, "");
        let comp = app.global::<Comp>();
        let gpu = self.gpu.as_ref().map(|(d, q)| (d, q));
        let mut skipped = 0;
        if let Some(stream) = self.screen.as_mut() {
            if stream.advance(source, &mut skipped) {
                if let Some(frame) = &stream.current {
                    comp.set_screen(match (&frame.pixels, gpu) {
                        (Pixels::Staged(buffer), Some((device, queue))) => {
                            self.screen_layer.staged(device, queue, buffer, frame.width, frame.height)
                        }
                        (Pixels::Cpu(pixels), _) => self.screen_layer.image(self.upload, gpu, pixels, frame.width, frame.height),
                        _ => slint::Image::default(),
                    });
                    self.stats.shown += 1;
                }
            } else if self.playing && self.clock.is_some() && stream.starved(source) {
                self.stats.starved += 1;
            }
        }
        if let (Some(stream), Some(sync)) = (self.camera.as_mut(), self.document.camera_sync(source)) {
            let mut camera_skipped = 0;
            if stream.advance(sync.target_ms as f64 / 1000.0, &mut camera_skipped) {
                if let Some(frame) = &stream.current {
                    let cpu = match (&frame.pixels, gpu) {
                        (Pixels::Staged(buffer), Some((device, queue))) => {
                            comp.set_camera_frame(self.camera_layer.staged(device, queue, buffer, frame.width, frame.height));
                            frame.cpu_copy.as_deref()
                        }
                        (Pixels::Cpu(pixels), _) => {
                            comp.set_camera_frame(self.camera_layer.image(self.upload, gpu, pixels, frame.width, frame.height));
                            Some(pixels.as_slice())
                        }
                        _ => None,
                    };
                    if let (true, Some(pixels)) = (composition.layout.camera_full > 0.0, cpu) {
                        comp.set_camera_blurred(scene::blurred_camera(pixels, frame.width as usize, frame.height as usize, 1.0));
                    }
                }
            }
        }
        self.stats.skipped += skipped;
        if let Some(models) = self.models.as_mut() {
            models.apply(&comp, &FrameState::preview(&composition));
        }
        app.set_position(output as f32);
        app.set_playing(self.playing);
        app.set_time_text(format!("{} / {}", tenths(output), tenths(self.document.duration())).into());
        // Keep rendering while playing or until a seek's frame is on screen.
        let waiting = self.screen.as_ref().is_some_and(|s| s.current.as_ref().is_none_or(|c| (c.time - source).abs() > 1.0 / s.fps));
        self.busy = self.playing || self.pending_seek.is_some() || waiting;
        app.set_animate(self.busy);
        if self.measure_started.is_some() {
            self.stats.prepare_ns.push(started.elapsed().as_nanos() as u64);
        }
    }
}

/// PreviewPane.qml's `formatTenths`: mm:ss.t.
fn tenths(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round().max(0.0) as i64;
    format!("{:02}:{:02}.{}", tenths / 600, (tenths % 600) / 10, tenths % 10)
}

pub fn run(options: Options) -> Result<(), String> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .require_wgpu_30(slint::wgpu_30::WGPUConfiguration::default())
        .select()
        .map_err(|e| format!("Slint backend: {e}"))?;
    let app = PreviewWindow::new().map_err(|e| e.to_string())?;

    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(&options.bundle, &probes)?;
    // The editor builds the motion track on a worker and shows the default sample meanwhile;
    // the spike waits for it.
    if let Some(job) = document.motion_job() {
        document.apply_motion(job.run());
    }
    document.set_screen_frame_times(media_probe::probe_frame_times(document.video_path()));
    let _ = document.take_effects();

    let comp = app.global::<Comp>();
    let recording = scene::Recording {
        source_width: document.source_width(),
        source_height: document.source_height(),
        capture_scale: document.capture_scale(),
        camera: document.has_camera().then(|| (document.camera_source_width(), document.camera_source_height())),
    };
    let background_path = match document.project().background.type_.as_str() {
        "image" => document.project().background.image.clone(),
        "wallpaper" if document.project().background.wallpaper == "omarchy:current" => omareel_core::omarchy_paths::current_background(),
        "wallpaper" => document.project().background.wallpaper.clone(),
        _ => String::new(),
    };
    let background = scene::load_background(&background_path);
    scene::apply_project(&comp, document.project(), &recording, background.as_ref());
    app.set_output_width(document.output_width());
    app.set_output_height(document.output_height());
    app.set_canvas_limit(options.canvas_height);
    app.set_duration(document.duration() as f32);

    let (sw, sh) = (document.source_width(), document.source_height());
    let screen_size = if options.decode_height > 0 && options.decode_height < sh {
        let h = options.decode_height & !1;
        (((f64::from(sw) * f64::from(h) / f64::from(sh)).round() as i32) & !1, h)
    } else {
        (sw, sh)
    };
    let camera_path = document.camera_video_path().to_owned();
    let player = Rc::new(RefCell::new(Player {
        document,
        upload: options.upload,
        gpu: None,
        models: None,
        screen_layer: VideoLayer::new("screen"),
        camera_layer: VideoLayer::new("camera"),
        screen: None,
        camera: None,
        screen_size,
        camera_path,
        playing: false,
        clock: None,
        origin: 0.0,
        pending_seek: Some(0.0),
        last_restart: Instant::now(),
        busy: true,
        measuring: options.measure.is_some(),
        measure_started: None,
        measure_seconds: options.measure.unwrap_or(0.0),
        stats: Stats::default(),
        stats_path: options.stats.clone(),
        render_started: None,
    }));
    player.borrow_mut().models = Some(Models::install(&comp));

    let weak = app.as_weak();
    let notifier_player = player.clone();
    app.window()
        .set_rendering_notifier(move |state, api| {
            let Some(app) = weak.upgrade() else { return };
            let mut player = notifier_player.borrow_mut();
            match state {
                RenderingState::RenderingSetup => {
                    if let GraphicsAPI::WGPU30 { device, queue, .. } = api {
                        player.gpu = Some((device.clone(), queue.clone()));
                    }
                }
                RenderingState::BeforeRendering => {
                    player.prepare(&app);
                    player.render_started = Some(Instant::now());
                }
                RenderingState::AfterRendering => {
                    let now = Instant::now();
                    if let Some(started) = player.measure_started {
                        if let Some(render) = player.render_started {
                            player.stats.render_ns.push((now - render).as_nanos() as u64);
                        }
                        player.stats.frames.push(now);
                        if started.elapsed().as_secs_f64() >= player.measure_seconds || !player.playing {
                            player.measure_started = None;
                            player.measuring = false;
                            write_stats(&player.stats, player.stats_path.as_deref(), &player.document);
                            let _ = slint::quit_event_loop();
                        }
                    }
                }
                _ => {}
            }
        })
        .map_err(|e| format!("rendering notifier: {e:?}"))?;

    let p = player.clone();
    app.on_toggle_play(move || p.borrow_mut().toggle());
    let p = player.clone();
    let weak = app.as_weak();
    app.on_seek(move |value| {
        p.borrow_mut().seek(f64::from(value));
        if let Some(app) = weak.upgrade() {
            app.set_animate(true);
            app.window().request_redraw();
        }
    });
    app.on_seek_finished(|| {});

    if options.measure.is_some() {
        let p = player.clone();
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
            p.borrow_mut().toggle();
            if let Some(app) = weak.upgrade() {
                app.set_animate(true);
                app.window().request_redraw();
            }
        });
    }
    app.set_animate(true);
    app.run().map_err(|e| e.to_string())
}

fn write_stats(stats: &Stats, path: Option<&str>, document: &Document) {
    let mut gaps: Vec<f64> = stats.frames.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect();
    gaps.sort_by(f64::total_cmp);
    let pick = |p: f64| gaps.get(((gaps.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0);
    let mean = |v: &[u64]| if v.is_empty() { 0.0 } else { v.iter().sum::<u64>() as f64 / v.len() as f64 / 1e6 };
    let mut sorted_prepare = stats.prepare_ns.clone();
    sorted_prepare.sort();
    let span = match (stats.frames.first(), stats.frames.last()) {
        (Some(a), Some(b)) => (*b - *a).as_secs_f64(),
        _ => 0.0,
    };
    let recording_frames = (span * document.fps()).round();
    let json = format!(
        "{{\"seconds\": {span:.2}, \"frames\": {}, \"fps\": {:.2}, \"frame_ms_median\": {:.2}, \"frame_ms_p95\": {:.2}, \"frame_ms_max\": {:.2}, \
         \"missed_vsyncs\": {}, \"prepare_ms_mean\": {:.3}, \"prepare_ms_max\": {:.3}, \"render_ms_mean\": {:.3}, \
         \"screen_frames_shown\": {}, \"screen_frames_skipped\": {}, \"starved_frames\": {}, \"recording_frames_in_span\": {recording_frames}}}",
        stats.frames.len(),
        if span > 0.0 { (stats.frames.len() - 1) as f64 / span } else { 0.0 },
        pick(0.5),
        pick(0.95),
        gaps.last().copied().unwrap_or(0.0),
        gaps.iter().filter(|g| **g > 25.0).count(),
        mean(&stats.prepare_ns),
        sorted_prepare.last().copied().unwrap_or(0) as f64 / 1e6,
        mean(&stats.render_ns),
        stats.shown,
        stats.skipped,
        stats.starved,
    );
    println!("{json}");
    if let Some(path) = path {
        let _ = std::fs::write(path, json + "\n");
    }
}
