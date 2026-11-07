//! The editor's preview: a Rune window playing the composition with play/pause, a scrub bar and
//! the time.
//!
//! Rune's `poll` hook runs at the start of every frame. While playing it takes the clock, asks
//! the `Document` for the composition at that time (`preview_composition`, the editor's own
//! per-frame state), hands the decoded screen and camera frames for it to their surfaces, and asks
//! for the next frame with `wake_after(0)`. The view then rebuilds the composition from that state
//! and Rune diffs it.

use crate::comp::{self, FrameState, Media, Settings, Video};
use crate::theme::Ui;
use crate::video::{self, Decode, Stream, StreamSpec, Upload};
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use rune::native_surface::ColorInfo;
use rune::tok::{Family, TextStyle};
use rune::{App, AppSpec, Cursor, Cx, El, Key, KeyEvent, NativeSurface, Pixels, Tx, col, el, row, text};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Options {
    pub bundle: String,
    pub upload: Upload,
    pub decode: Decode,
    /// The canvas's tallest size in points (the Qt editor's 620); 0 for the whole pane.
    pub canvas_height: f32,
    /// Decode the screen at this height (scaled by ffmpeg); 0 for the recording's size.
    pub decode_height: i32,
    /// Play from the start after 1.5 s, for this many seconds, then write `stats` and quit.
    pub measure: Option<f64>,
    pub stats: Option<String>,
    /// The editor around the preview (`editor` subcommand): inspector and timeline.
    pub editor: bool,
    /// Rune's own flags (`--snap`, `--script`, `--video`, `--tree`, ...), after `--`.
    pub rune_args: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Preview,
    Editor,
}

/// When each frame finished encoding, and whether it drew (`App::on_frame_encoded`, which has no
/// access to the state).
/// `SPIKE_EXPERIMENT=nocomp`: the window without the composition (only the chrome redraws).
static NO_COMPOSITION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static ENCODED: std::sync::Mutex<Vec<(Instant, bool)>> = std::sync::Mutex::new(Vec::new());

#[derive(Default)]
struct Stats {
    frames: Vec<Instant>,
    /// Time spent in `poll` per frame (clock, state, frame choice), ns.
    poll_ns: Vec<u64>,
    poll_started: Vec<Instant>,
    shown: u64,
    skipped: u64,
    starved: u64,
}

pub struct Preview {
    pub(crate) mode: Mode,
    pub(crate) document: Document,
    pub(crate) settings: Settings,
    pub(crate) media: Media,
    pub(crate) frame: FrameState,
    pub(crate) kit: rune_kit::Kit,
    /// The motion track a worker is rebuilding after an edit.
    pub(crate) motion: Option<std::sync::mpsc::Receiver<omareel_core::motion_track::MotionTrack>>,
    /// The editor's transient UI state.
    pub(crate) edit: crate::editor::EditState,
    /// Poll's clock: seconds since start (virtual in headless runs).
    pub(crate) now: f64,
    upload: Upload,
    decode: Decode,
    screen_color: ColorInfo,
    camera_color: ColorInfo,
    screen: Option<Stream>,
    camera: Option<Stream>,
    screen_size: (u32, u32),
    camera_path: String,
    canvas_limit: f32,
    pub(crate) playing: bool,
    /// The output time the clock started from, and when on poll's clock (None until the first
    /// frame is there).
    clock: Option<(f64, f64)>,
    origin: f64,
    pending_seek: Option<f64>,
    last_restart: Instant,
    started: Instant,
    measure: Option<f64>,
    measure_started: Option<Instant>,
    stats: Stats,
    stats_path: Option<String>,
    pub(crate) ui: Ui,
    pub(crate) scrubbing: bool,
    /// Keeps the wallpaper and cursor images' decoder alive.
    _cache: rune::ImageCache,
}

impl Preview {
    fn restart_decoders(&mut self, output_time: f64) {
        self.document.set_output_position(output_time);
        let source = self.document.source_position();
        let duration = self.document.source_duration();
        let (w, h) = self.screen_size;
        let ahead = if w * h > 2_500_000 { 4 } else { 8 };
        self.screen = Some(Stream::start(StreamSpec {
            path: self.document.video_path(),
            start: source,
            end: duration,
            fps: self.document.fps(),
            width: w,
            height: h,
            ahead,
            upload: self.upload,
            decode: self.decode.clone(),
            color: self.screen_color,
        }));
        self.camera = None;
        if let Some(sync) = self.document.camera_sync(source) {
            let (cw, ch) = (self.document.camera_source_width() as u32, self.document.camera_source_height() as u32);
            let camera = self.document.recording().camera;
            self.camera = Some(Stream::start(StreamSpec {
                path: &self.camera_path,
                start: sync.target_ms as f64 / 1000.0,
                end: camera.map_or(duration, |c| c.duration),
                fps: camera.map_or(30.0, |c| c.fps),
                width: cw,
                height: ch,
                ahead: 6,
                upload: self.upload,
                // The camera is small; software decoding keeps the video engine for the screen.
                decode: Decode::Software,
                color: self.camera_color,
            }));
        }
        self.last_restart = Instant::now();
    }

    pub(crate) fn seek(&mut self, output_time: f64) {
        let t = output_time.clamp(0.0, self.document.duration());
        self.pending_seek = Some(t);
        self.document.set_output_position(t);
    }

    pub(crate) fn toggle(&mut self) {
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
        // A paused decoder already holds the frames after the one on screen: play on from it
        // rather than starting ffmpeg again (which costs a few hundred ms, more with VA-API).
        self.document.set_output_position(position);
        let source = self.document.source_position();
        let warm = self.pending_seek.is_none()
            && self.screen.as_ref().is_some_and(|s| !s.finished && s.current_time.is_some_and(|c| (c - source).abs() <= 1.5 / s.fps));
        if !warm {
            self.restart_decoders(position);
        }
    }

    fn show(video: &Video, frame: video::Frame) {
        if std::env::var("SPIKE_EXPERIMENT").is_ok_and(|e| e.contains("novideo")) {
            return;
        }
        match (video, frame.pixels) {
            (Video::Surface(surface), video::Pixels::Nv12(bytes)) => {
                if let Err(e) = surface.push_bytes(bytes, (frame.time * 1000.0) as u64) {
                    eprintln!("push: {e}");
                }
            }
            (Video::Pixels(pixels), video::Pixels::Rgba(rgba)) => pixels.set(frame.width, frame.height, rgba),
            _ => {}
        }
    }

    /// The frame about to be built: the playhead, the composition and the pictures. True when
    /// anything the view shows changed.
    fn step(&mut self) -> bool {
        let started = Instant::now();
        if let Some(target) = self.pending_seek
            && (self.last_restart.elapsed().as_secs_f64() > 0.035 || self.screen.is_none())
        {
            self.pending_seek = None;
            if self.playing {
                self.origin = target;
                self.clock = None;
            }
            self.restart_decoders(target);
        }
        if let Some(seconds) = self.measure
            && !self.playing
            && self.measure_started.is_none()
            && self.started.elapsed().as_secs_f64() > 1.5
            && self.stats.frames.is_empty()
        {
            let _ = seconds;
            self.toggle();
        }
        if self.playing && self.clock.is_none() && self.screen.as_ref().is_some_and(|s| s.current_time.is_some() || !s.starved(-1.0)) {
            self.clock = Some((self.origin, self.now));
            if self.measure.is_some() && self.measure_started.is_none() {
                self.measure_started = Some(Instant::now());
                // scripts/frames.py measures the window's commits from this line on.
                eprintln!("spike: measured playback starts");
            }
        }
        if let Some((origin, since)) = self.clock {
            let mut output = origin + (self.now - since).max(0.0);
            if output >= self.document.duration() {
                output = self.document.duration();
                self.playing = false;
                self.clock = None;
            }
            self.document.set_output_position(output);
        }
        let source = self.document.source_position();
        let composition = self.document.preview_composition(false, "");
        let mut skipped = 0;
        let mut changed = false;
        // Headless runs (`--snap`, `--video`) are frame-exact: wait for each frame's pictures.
        if rune::is_headless() {
            if let Some(stream) = self.screen.as_mut() {
                stream.wait_for(source);
            }
            if let (Some(stream), Some(sync)) = (self.camera.as_mut(), self.document.camera_sync(source)) {
                stream.wait_for(sync.target_ms as f64 / 1000.0);
            }
        }
        if let Some(stream) = self.screen.as_mut() {
            if let Some(frame) = stream.advance(source, &mut skipped) {
                Self::show(&self.media.screen, frame);
                self.stats.shown += 1;
                changed = true;
            } else if self.playing && self.clock.is_some() && stream.starved(source) {
                self.stats.starved += 1;
            }
        }
        if let (Some(stream), Some(sync)) = (self.camera.as_mut(), self.document.camera_sync(source)) {
            let mut camera_skipped = 0;
            if let Some(frame) = stream.advance(sync.target_ms as f64 / 1000.0, &mut camera_skipped) {
                Self::show(&self.media.camera, frame);
                changed = true;
            }
        }
        self.stats.skipped += skipped;
        self.frame = FrameState::preview(&composition);
        if self.measure_started.is_some() {
            self.stats.poll_started.push(started);
            self.stats.poll_ns.push(started.elapsed().as_nanos() as u64);
            self.stats.frames.push(Instant::now());
        }
        changed || self.playing
    }

    pub(crate) fn busy(&self) -> bool {
        let source = self.document.source_position();
        let waiting = self
            .screen
            .as_ref()
            .is_some_and(|s| s.current_time.is_none_or(|c| (c - source).abs() > 1.0 / s.fps) && !s.finished);
        self.playing || self.pending_seek.is_some() || waiting || self.scrubbing
    }
}

fn poll(s: &mut Preview, t: f64, tx: &mut Tx) -> bool {
    s.now = t;
    let edited = crate::editor::poll(s);
    let changed = s.step() || edited;
    if std::env::var_os("SPIKE_DEBUG").is_some() {
        eprintln!("poll t={t:.3} changed={changed} busy={} playing={} pos={:.3}", s.busy(), s.playing, s.document.output_position());
    }
    if let Some(started) = s.measure_started
        && let Some(seconds) = s.measure
        && (started.elapsed().as_secs_f64() >= seconds || !s.playing)
    {
        s.measure_started = None;
        s.measure = None;
        write_stats(&s.stats, s.stats_path.as_deref(), &s.document);
        tx.quit();
    }
    if s.busy() {
        tx.wake_after(Duration::ZERO);
    } else if s.measure.is_some() {
        // Waiting to start the measured playback.
        tx.wake_after(Duration::from_millis(100));
    }
    if changed && s.playing {
        // Values come from omareel_core every frame; nothing should spring.
        tx.instant();
    }
    changed
}

fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    if s.mode == Mode::Editor && crate::editor::on_key(s, key, tx) {
        return true;
    }
    match &key.key {
        Key::Text(t) if t == " " => {
            s.toggle();
            tx.wake_after(Duration::ZERO);
            true
        }
        Key::Left => {
            s.seek(s.document.output_position() - 5.0);
            tx.wake_after(Duration::ZERO);
            true
        }
        Key::Right => {
            s.seek(s.document.output_position() + 5.0);
            tx.wake_after(Duration::ZERO);
            true
        }
        _ => false,
    }
}

/// PreviewPane.qml's `formatTenths`: mm:ss.t.
pub fn tenths(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round().max(0.0) as i64;
    format!("{:02}:{:02}.{}", tenths / 600, (tenths % 600) / 10, tenths % 10)
}

fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    if s.mode == Mode::Editor {
        return crate::editor::view(s, cx);
    }
    col().w_full().h_full().fill(s.ui.canvas).child(pane(s, cx, 64.0, 56.0)).child(transport(s, &s.ui))
}

/// The preview pane: the canvas, as large as fits with `margin_x` / `margin_y` around it (at most
/// `canvas_limit` tall), centred.
pub(crate) fn pane(s: &Preview, cx: &mut Cx<Preview>, margin_x: f32, margin_y: f32) -> El {
    let ui = &s.ui;
    let (out_w, out_h) = (s.document.output_width() as f32, s.document.output_height() as f32);
    let pane = cx.layout_size("pane").unwrap_or([0.0, 0.0]);
    let (avail_w, avail_h) = ((pane[0] - margin_x).max(0.0), (pane[1] - margin_y).max(0.0));
    let mut canvas_h = avail_h.min(avail_w * out_h / out_w.max(1.0));
    if s.canvas_limit > 0.0 {
        canvas_h = canvas_h.min(s.canvas_limit);
    }
    let canvas_w = (canvas_h * out_w / out_h.max(1.0)).floor();
    let canvas_h = canvas_h.floor();
    // The root's fill is the frame's clear colour; nothing else under the canvas fills, so every
    // pixel is drawn once (Rune blends every quad and redraws the window each frame).
    let mut pane_el = el().key("pane").w_full().grow(1.0).min_h(0.0).center();
    if canvas_w > 1.0 && canvas_h > 1.0 && !NO_COMPOSITION.load(std::sync::atomic::Ordering::Relaxed) {
        let mut canvas = el().key("canvas").size(canvas_w, canvas_h);
        // SPIKE_EXPERIMENT=nocanvasclip,nocanvasshadow: the canvas without its rounded clip or shadow.
        let experiment = std::env::var("SPIKE_EXPERIMENT").unwrap_or_default();
        if !experiment.contains("nocanvasclip") && ui.radius > 0.0 {
            canvas = canvas.radius(ui.radius).clip_rounded(ui.radius);
        }
        if !experiment.contains("nocanvasshadow") {
            canvas = canvas.shadow(14.0, 40.0, ui.shadow);
        }
        // While playing, every value comes from omareel_core; paused, an edit springs.
        pane_el = pane_el.child(canvas.child(comp::composition(&s.settings, &s.media, &s.frame, canvas_w, canvas_h, s.playing || s.scrubbing)));
    }
    pane_el
}

/// The bar under the preview: play/pause, the time, and a scrub bar.
pub(crate) fn transport(s: &Preview, ui: &Ui) -> El {
    let duration = s.document.duration().max(0.001);
    let position = s.document.output_position();
    let progress = (position / duration).clamp(0.0, 1.0) as f32;
    let play = el()
        .key("play")
        .role("button")
        .label(if s.playing { "Pause" } else { "Play" })
        .size(36.0, 36.0)
        .radius(ui.radius.max(6.0))
        .center()
        .fill(ui.normal_fill)
        .hover(ui.hover_fill)
        .press(ui.pressed_fill)
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
            s.toggle();
            tx.wake_after(Duration::ZERO);
        })
        .child(play_glyph(s.playing, ui.text));
    let mono = TextStyle { family: Family::SystemMono, ..TextStyle::new(13.0, 500.0) };
    let time = text(format!("{} / {}", tenths(position), tenths(duration))).font(mono).color(ui.text_muted);
    let track = el()
        .key("scrub")
        .grow(1.0)
        .h(24.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut Preview, drag, tx: &mut Tx| {
            let width = drag.w.max(1.0);
            let x = drag.x.clamp(0.0, width);
            s.scrubbing = !matches!(drag.phase, rune::Phase::Up);
            s.seek(f64::from(x / width) * s.document.duration());
            tx.wake_after(Duration::ZERO);
        })
        .child(
            el().w_full().h(4.0).radius(2.0).fill(ui.normal_fill).child(el().h(4.0).w_frac(progress).radius(2.0).fill(ui.accent)),
        );
    row().key("transport").w_full().h(56.0).px(16.0).gap(14.0).items_center().fill(ui.surface).border(0.0, ui.hairline).child(play).child(time).child(track)
}

/// A play triangle or two pause bars.
pub(crate) fn play_glyph(playing: bool, color: rune::Color) -> El {
    if playing {
        row().gap(4.0).child(el().size(4.0, 14.0).radius(1.0).fill(color)).child(el().size(4.0, 14.0).radius(1.0).fill(color))
    } else {
        let mut shape = rune::Shape::new();
        shape = shape.fill(vec![[0.0, 0.0], [12.0, 7.0], [0.0, 14.0]], color);
        el().size(12.0, 14.0).ml(2.0).shape(shape)
    }
}

pub fn run(options: Options) -> Result<(), String> {
    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(&options.bundle, &probes)?;
    // The editor builds the motion track on a worker and shows the default sample meanwhile;
    // the spike waits for it.
    if let Some(job) = document.motion_job() {
        document.apply_motion(job.run());
    }
    document.set_screen_frame_times(media_probe::probe_frame_times(document.video_path()));
    let _ = document.take_effects();

    let mut settings = Settings::new(
        document.project(),
        (document.source_width(), document.source_height()),
        document.capture_scale(),
        document.has_camera().then(|| (document.camera_source_width(), document.camera_source_height())),
    );
    let background_path = match document.project().background.type_.as_str() {
        "image" => document.project().background.image.clone(),
        "wallpaper" if document.project().background.wallpaper == "omarchy:current" => omareel_core::omarchy_paths::current_background(),
        "wallpaper" => document.project().background.wallpaper.clone(),
        _ => String::new(),
    };
    let experiment = comp::apply_experiments(&mut settings);
    if experiment.contains("nocomp") {
        NO_COMPOSITION.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let background_path = if experiment.contains("nobg") { String::new() } else { background_path };
    let (sw, sh) = (document.source_width(), document.source_height());
    let screen_size = if options.decode_height > 0 && options.decode_height < sh {
        let h = options.decode_height & !1;
        ((((f64::from(sw) * f64::from(h) / f64::from(sh)).round() as i32) & !1) as u32, h as u32)
    } else {
        (sw as u32, sh as u32)
    };
    let camera_path = document.camera_video_path().to_owned();
    let screen_color = video::color_info(document.video_path());
    let camera_color = video::color_info(&camera_path);
    let upload = options.upload;
    let decode = options.decode.clone();
    eprintln!("preview: screen {}x{} decoded {:?} ({:?}), upload {:?}", screen_size.0, screen_size.1, decode, screen_color.matrix, upload);
    let new_video = || match upload {
        Upload::Surface => Video::Surface(NativeSurface::new()),
        Upload::Pixels => Video::Pixels(Pixels::new()),
    };
    let media_screen = new_video();
    let media_camera = new_video();
    let canvas_limit = options.canvas_height;
    let measure = options.measure;
    let stats_path = options.stats.clone();

    let mode = if options.editor { Mode::Editor } else { Mode::Preview };
    let size = if options.editor { (1440.0, 900.0) } else { (1280.0, 760.0) };
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        // Inter for the interface, fontconfig's monospace for times and keys.
        families: ("Inter", "ui-monospace"),
        ..AppSpec::new("omareel (Rune spike)", size, || unreachable!("App::init supplies the state"), view)
    };
    let ui = Ui::load();
    let kit = crate::editor::kit(&ui);
    App::new(spec)
        .args(options.rune_args.clone())
        .on_frame_encoded(|f| {
            if let Ok(mut encoded) = ENCODED.lock() {
                encoded.push((f.at(), matches!(f.outcome(), rune::FrameOutcome::Drawn)));
            }
        })
        .init(move || {
            let cache = rune::ImageCache::new(Arc::new(|| {}));
            let background = (!background_path.is_empty()).then(|| cache.file(&background_path, 0.0, 1.0));
            // The cursor is at most a few times its 24 pt size; rasterise once, large, and let the
            // mips cover the rest.
            let cursors = comp::cursor_images(&cache, 96.0, 2.0);
            // Headless runs draw what is decoded at each frame: wait for the images.
            if rune::is_headless() {
                for image in background.iter().chain(cursors.iter()) {
                    let _ = image.wait();
                }
            }
            Preview {
                mode,
                kit,
                motion: None,
                edit: Default::default(),
                now: 0.0,
                document,
                settings,
                media: Media { screen: media_screen, camera: media_camera, background, cursors },
                frame: FrameState::default(),
                upload,
                decode,
                screen_color,
                camera_color,
                screen: None,
                camera: None,
                screen_size,
                camera_path,
                canvas_limit,
                playing: false,
                clock: None,
                origin: 0.0,
                pending_seek: Some(0.0),
                last_restart: Instant::now(),
                started: Instant::now(),
                measure,
                measure_started: None,
                stats: Stats::default(),
                stats_path,
                ui,
                scrubbing: false,
                _cache: cache,
            }
        })
        .run();
    Ok(())
}

fn setup(s: &mut Preview, waker: rune::Waker) {
    // SPIKE_EXPERIMENT=nowaker: frames are pushed from poll, on the UI thread, so the surfaces'
    // waker only asks for frames poll already asked for.
    if std::env::var("SPIKE_EXPERIMENT").is_ok_and(|e| e.contains("nowaker")) {
        return;
    }
    if let Video::Surface(surface) = &s.media.screen {
        surface.set_waker(waker.clone());
    }
    if let Video::Surface(surface) = &s.media.camera {
        surface.set_waker(waker.clone());
    }
}

fn write_stats(stats: &Stats, path: Option<&str>, document: &Document) {
    let mut gaps: Vec<f64> = stats.frames.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect();
    gaps.sort_by(f64::total_cmp);
    let pick = |p: f64| gaps.get(((gaps.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0);
    let mean = |v: &[u64]| if v.is_empty() { 0.0 } else { v.iter().sum::<u64>() as f64 / v.len() as f64 / 1e6 };
    let span = match (stats.frames.first(), stats.frames.last()) {
        (Some(a), Some(b)) => (*b - *a).as_secs_f64(),
        _ => 0.0,
    };
    let recording_frames = (span * document.fps()).round();
    // Each measured poll to the first frame encoded after it: the UI thread's whole frame
    // (state, view, layout, encode, submit).
    let encoded = ENCODED.lock().map(|e| e.clone()).unwrap_or_default();
    let mut cpu: Vec<f64> = stats
        .poll_started
        .iter()
        .filter_map(|p| encoded.iter().find(|(at, _)| at >= p).map(|(at, _)| (*at - *p).as_secs_f64() * 1000.0))
        .collect();
    cpu.sort_by(f64::total_cmp);
    let cpu_pick = |q: f64| cpu.get(((cpu.len().max(1) - 1) as f64 * q).round() as usize).copied().unwrap_or(0.0);
    let drawn = encoded.iter().filter(|(at, drawn)| *drawn && stats.frames.first().is_some_and(|f| at >= f)).count();
    let json = format!(
        "{{\"seconds\": {span:.2}, \"frames\": {}, \"fps\": {:.2}, \"frame_ms_median\": {:.2}, \"frame_ms_p95\": {:.2}, \"frame_ms_max\": {:.2}, \
         \"missed_vsyncs\": {}, \"poll_ms_mean\": {:.3}, \"screen_frames_shown\": {}, \"screen_frames_skipped\": {}, \"starved_frames\": {}, \
         \"recording_frames_in_span\": {recording_frames}, \"frame_cpu_ms_median\": {:.2}, \"frame_cpu_ms_p95\": {:.2}, \"drawn\": {drawn}}}",
        stats.frames.len(),
        if span > 0.0 { (stats.frames.len().max(1) - 1) as f64 / span } else { 0.0 },
        pick(0.5),
        pick(0.95),
        gaps.last().copied().unwrap_or(0.0),
        gaps.iter().filter(|g| **g > 25.0).count(),
        mean(&stats.poll_ns),
        stats.shown,
        stats.skipped,
        stats.starved,
        cpu_pick(0.5),
        cpu_pick(0.95),
    );
    println!("{json}");
    if let Some(path) = path {
        let _ = std::fs::write(path, json + "\n");
    }
}
