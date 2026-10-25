//! The editor's preview in an iced window: the composition in a `shader` widget (the scene
//! renderer as a `shader::Pipeline`), play/pause, a scrub bar and the time, at the display's
//! frame rate. The playhead runs on the wall clock; each frame the document gives the
//! composition at the playhead, as `comp` gives it to the QML preview.

use crate::scene::composition::RenderTheme;
use crate::scene::{Scene, Video, VideoFrame, load_wallpaper};
use crate::video::Stream;
use iced::widget::{button, column, container, row, shader, slider, text};
use iced::keyboard::{self, key::Named};
use iced::{Element, Length, Rectangle, Subscription, Task, mouse, wgpu, window};
use omareel_core::camera_timeline::map_camera_time;
use omareel_core::editor::{Document, Probes};
use omareel_core::export::composition::{CompositionState, Cursor, Layout, Media, Zoom};
use omareel_core::media_probe;
use omareel_core::project::Project;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Options {
    pub bundle: String,
    /// Play from the start for this many seconds, write the stats and quit.
    pub measure: Option<f64>,
    pub stats: Option<String>,
    /// Draw the composition at most this many pixels high (the Qt editor shows it at 620 in a
    /// 1920x1080 window, beside its panels and timeline).
    pub canvas_height: Option<f32>,
}

/// What each rendered frame records while measuring.
#[derive(Debug, Default)]
pub struct Stats {
    pub recording: bool,
    /// When each frame was prepared.
    pub frames: Vec<Instant>,
    /// CPU time of each frame's prepare (upload, items, buffers).
    pub prepare_ns: Vec<u64>,
    /// The screen frame index each rendered frame showed.
    pub screen_index: Vec<i64>,
}

#[derive(Clone)]
struct Wallpaper(Arc<image::RgbaImage>);

impl std::fmt::Debug for Wallpaper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wallpaper({:?})", self.0.dimensions())
    }
}

struct Preview {
    document: Document,
    project: Arc<Project>,
    theme: Arc<RenderTheme>,
    wallpaper: Option<Wallpaper>,
    duration: f64,
    playing: bool,
    /// The wall clock and output time when playback last started.
    anchor: Option<(Instant, f64)>,
    scrubbing: bool,
    screen: Stream,
    camera: Option<(Stream, f64, f64)>,
    screen_frame: Option<(i64, VideoFrame)>,
    camera_frame: Option<(i64, VideoFrame)>,
    state: CompositionState,
    stats: Arc<Mutex<Stats>>,
    options: Options,
    measure_started: Option<Instant>,
    opened: Instant,
}

#[derive(Debug, Clone)]
enum Message {
    Frame(Instant),
    Toggle,
    Scrub(f64),
    ScrubReleased,
    Key(keyboard::Event),
}

pub fn run(options: Options) -> Result<(), String> {
    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(&options.bundle, &probes)?;
    // The motion track (zoom springs, cursor), which the editor builds on a worker thread.
    if let Some(job) = document.motion_job() {
        document.apply_motion(job.run());
    }
    let _ = document.take_effects();
    let project = Arc::new(document.project().clone());
    let theme = Arc::new(RenderTheme::load());
    let image = omareel_core::editor::Document::project_map(&document).value("background").to_object().value("resolvedImage").to_string_or("");
    let path = image.strip_prefix("file://").unwrap_or(&image).to_owned();
    let wallpaper = load_wallpaper(&path, document.output_width() as u32, document.output_height() as u32).map(|i| Wallpaper(Arc::new(i)));
    let duration = document.duration();
    let screen = Stream::new(document.video_path(), document.fps(), document.source_width(), document.source_height(), document.source_duration());
    let camera = document.recording().camera.map(|camera| {
        (Stream::new(document.camera_video_path(), camera.fps, camera.width, camera.height, camera.duration), camera.offset, camera.duration)
    });
    let title = format!("{} (iced spike)", document.bundle_name());
    let state = composition_state(&document);
    let preview = Preview {
        document,
        project,
        theme,
        wallpaper,
        duration,
        playing: false,
        anchor: None,
        scrubbing: false,
        screen,
        camera,
        screen_frame: None,
        camera_frame: None,
        state,
        stats: Arc::new(Mutex::new(Stats::default())),
        options,
        measure_started: None,
        opened: Instant::now(),
    };
    let cell = std::cell::RefCell::new(Some(preview));
    iced::application(move || cell.borrow_mut().take().expect("booted once"), Preview::update, Preview::view)
        .title(move |_: &Preview| title.clone())
        .subscription(Preview::subscription)
        .theme(|_: &Preview| iced::Theme::Dark)
        .window_size((1280.0, 820.0))
        .run()
        .map_err(|error| error.to_string())
}

/// `comp` as the QML preview reads it, in the shape the scene takes.
fn composition_state(document: &Document) -> CompositionState {
    let comp = document.preview_composition(false, "");
    let mut state = CompositionState::initial(document.project());
    state.output_width = document.output_width();
    state.output_height = document.output_height();
    state.source_width = document.source_width();
    state.source_height = document.source_height();
    state.capture_scale = document.capture_scale();
    state.camera_available = document.has_camera();
    state.camera_visible = comp.camera_visible;
    state.camera_source_width = document.camera_source_width();
    state.camera_source_height = document.camera_source_height();
    state.time = comp.time;
    state.zoom = Zoom { scale: comp.zoom.scale, cx: comp.zoom.cx, cy: comp.zoom.cy, velocity: comp.zoom.velocity };
    state.layout = Layout { camera_full: comp.layout.camera_full, bubble: comp.layout.bubble };
    state.media = Media { active: comp.media.active, ready: comp.media.ready, fit: comp.media.fit, width: comp.media.width, height: comp.media.height };
    let c = comp.cursor;
    state.cursor = Cursor { x: c.x, y: c.y, visible: c.visible, scale: c.scale, opacity: c.opacity, rotation: c.rotation };
    state.ripples = comp.ripples;
    state.keystroke_pills = comp.keystroke_pills;
    state
}

impl Preview {
    fn position(&self, now: Instant) -> f64 {
        match self.anchor {
            Some((when, from)) if self.playing => from + now.saturating_duration_since(when).as_secs_f64(),
            _ => self.document.output_position(),
        }
    }

    fn seek(&mut self, output_time: f64) {
        self.document.set_output_position(output_time);
        self.anchor = Some((Instant::now(), self.document.output_position()));
        self.refresh();
    }

    /// The composition and video frames at the document's position.
    fn refresh(&mut self) {
        self.state = composition_state(&self.document);
        let source_time = self.state.time;
        if let Some(frame) = self.screen.frame_at(source_time) {
            self.screen_frame = Some(frame);
        }
        if let Some((stream, offset, duration)) = &mut self.camera {
            let camera_time = map_camera_time(source_time, *offset, *duration);
            if let Some(frame) = stream.frame_at(camera_time.seconds) {
                self.camera_frame = Some(frame);
            }
        }
        let _ = self.document.take_effects();
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Frame(now) => {
                if let Some(seconds) = self.options.measure {
                    match self.measure_started {
                        // Start 3 s after opening (the sampler's window starts then too).
                        None if self.screen_frame.is_some() && self.opened.elapsed() > Duration::from_secs(3) => {
                            self.measure_started = Some(now);
                            self.stats.lock().unwrap().recording = true;
                            self.playing = true;
                            self.seek(0.0);
                        }
                        Some(started) if now.duration_since(started).as_secs_f64() >= seconds => {
                            self.stats.lock().unwrap().recording = false;
                            self.write_stats(seconds);
                            return iced::exit();
                        }
                        _ => {}
                    }
                }
                if self.playing && !self.scrubbing {
                    let position = self.position(now);
                    if position >= self.duration {
                        self.document.set_output_position(self.duration);
                        self.playing = false;
                    } else {
                        self.document.set_output_position(position);
                    }
                }
                self.refresh();
            }
            Message::Toggle => {
                self.playing = !self.playing;
                if self.playing && self.document.output_position() >= self.duration - 1e-3 {
                    self.document.set_output_position(0.0);
                }
                self.anchor = Some((Instant::now(), self.document.output_position()));
            }
            // Space plays and pauses, the arrows step a second, Home goes to the start.
            Message::Key(keyboard::Event::KeyPressed { key: keyboard::Key::Named(named), .. }) => {
                let position = self.document.output_position();
                match named {
                    Named::Space => return self.update(Message::Toggle),
                    Named::ArrowLeft => self.seek((position - 1.0).max(0.0)),
                    Named::ArrowRight => self.seek((position + 1.0).min(self.duration)),
                    Named::Home => self.seek(0.0),
                    _ => {}
                }
            }
            Message::Key(_) => {}
            Message::Scrub(time) => {
                self.scrubbing = true;
                self.seek(time);
            }
            Message::ScrubReleased => {
                self.scrubbing = false;
                self.anchor = Some((Instant::now(), self.document.output_position()));
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        // Frames drive playback; while paused, a slow tick lets decoded frames come in.
        let waiting = self.screen_frame.is_none() || self.options.measure.is_some();
        let ticks = if self.playing || self.scrubbing || waiting {
            window::frames().map(Message::Frame)
        } else {
            iced::time::every(Duration::from_millis(100)).map(Message::Frame)
        };
        Subscription::batch([ticks, keyboard::listen().map(Message::Key)])
    }

    fn view(&self) -> Element<'_, Message> {
        let program = SceneProgram {
            primitive: ScenePrimitive {
                state: self.state.clone(),
                project: Arc::clone(&self.project),
                theme: Arc::clone(&self.theme),
                wallpaper: self.wallpaper.clone(),
                screen: self.screen_frame.clone(),
                camera: self.camera_frame.clone(),
                stats: Arc::clone(&self.stats),
                max_height: self.options.canvas_height,
            },
        };
        let position = self.document.output_position();
        let controls = row![
            button(text(if self.playing { "Pause" } else { "Play" }).width(56).center()).on_press(Message::Toggle),
            slider(0.0..=self.duration, position, Message::Scrub).step(0.001).on_release(Message::ScrubReleased),
            text(format!("{} / {}", clock(position), clock(self.duration))).font(iced::Font::MONOSPACE),
        ]
        .spacing(14)
        .padding(12)
        .align_y(iced::Alignment::Center);
        column![
            container(shader(program).width(Length::Fill).height(Length::Fill))
                .style(|_| container::background(iced::Color::from_rgb8(0x0b, 0x0b, 0x0e))),
            controls,
        ]
        .into()
    }

    fn write_stats(&self, seconds: f64) {
        let stats = self.stats.lock().unwrap();
        let intervals: Vec<f64> = stats.frames.windows(2).map(|w| w[1].duration_since(w[0]).as_secs_f64() * 1000.0).collect();
        let mut sorted = intervals.clone();
        sorted.sort_by(f64::total_cmp);
        let percentile = |p: f64| sorted.get(((sorted.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
        let mut prepare: Vec<f64> = stats.prepare_ns.iter().map(|ns| *ns as f64 / 1e6).collect();
        prepare.sort_by(f64::total_cmp);
        let prepare_median = prepare.get(prepare.len() / 2).copied().unwrap_or(0.0);
        let prepare_p95 = prepare.get(((prepare.len() as f64 - 1.0) * 0.95).round() as usize).copied().unwrap_or(0.0);
        let mut shown: Vec<i64> = stats.screen_index.clone();
        shown.dedup();
        let first = shown.first().copied().unwrap_or(0);
        let last = shown.last().copied().unwrap_or(0);
        let expected = last - first + 1;
        let distinct = shown.len() as i64;
        let missed_vsync = intervals.iter().filter(|ms| **ms > 1.5 * 1000.0 / 60.0).count();
        let json = format!(
            "{{\"seconds\": {seconds}, \"frames\": {}, \"fps\": {:.2}, \"frame_ms_median\": {:.3}, \"frame_ms_p95\": {:.3}, \"frame_ms_max\": {:.3}, \
             \"missed_vsyncs\": {missed_vsync}, \"screen_frames_expected\": {expected}, \"screen_frames_shown\": {distinct}, \"screen_frames_dropped\": {}, \
             \"decoder_late\": {}, \"prepare_ms_median\": {prepare_median:.3}, \"prepare_ms_p95\": {prepare_p95:.3}}}",
            stats.frames.len(),
            stats.frames.len() as f64 / seconds,
            percentile(0.5),
            percentile(0.95),
            sorted.last().copied().unwrap_or(0.0),
            expected - distinct,
            self.screen.late,
        );
        println!("{json}");
        if let Some(path) = &self.options.stats {
            let _ = std::fs::write(path, json + "\n");
        }
    }
}

fn clock(seconds: f64) -> String {
    let centis = (seconds * 100.0).round() as i64;
    format!("{}:{:02}.{:02}", centis / 6000, centis / 100 % 60, centis % 100)
}

struct SceneProgram {
    primitive: ScenePrimitive,
}

#[derive(Debug, Clone)]
struct ScenePrimitive {
    state: CompositionState,
    project: Arc<Project>,
    theme: Arc<RenderTheme>,
    wallpaper: Option<Wallpaper>,
    screen: Option<(i64, VideoFrame)>,
    camera: Option<(i64, VideoFrame)>,
    stats: Arc<Mutex<Stats>>,
    max_height: Option<f32>,
}

impl<Message> shader::Program<Message> for SceneProgram {
    type State = ();
    type Primitive = ScenePrimitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> ScenePrimitive {
        self.primitive.clone()
    }
}

/// The scene renderer as iced's pipeline for the primitive, with where the canvas went.
struct ScenePipeline {
    scene: Scene,
    wallpaper_set: bool,
    font_set: bool,
    canvas: [f32; 4],
}

impl shader::Pipeline for ScenePipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> ScenePipeline {
        // The decode threads upload into textures of this device from now on.
        let _ = crate::video::GPU.set((device.clone(), queue.clone()));
        ScenePipeline { scene: Scene::new(device, format), wallpaper_set: false, font_set: false, canvas: [0.0; 4] }
    }
}

impl shader::Primitive for ScenePrimitive {
    type Pipeline = ScenePipeline;

    fn prepare(&self, pipeline: &mut ScenePipeline, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, viewport: &shader::Viewport) {
        let started = Instant::now();
        if !pipeline.font_set {
            pipeline.scene.set_font_family(&self.theme.mono_family);
            pipeline.font_set = true;
        }
        if let (false, Some(wallpaper)) = (pipeline.wallpaper_set, &self.wallpaper) {
            pipeline.scene.set_wallpaper(device, queue, &wallpaper.0);
            pipeline.wallpaper_set = true;
        }
        pipeline.scene.begin_frame();
        if let Some((_, frame)) = &self.screen {
            pipeline.scene.set_video(device, queue, Video::Screen, frame);
        }
        if let Some((_, frame)) = &self.camera {
            pipeline.scene.set_video(device, queue, Video::Camera, frame);
        }
        // Fit the canvas in the widget, centred, in physical pixels.
        let scale = viewport.scale_factor();
        let (bx, by, bw, bh) = (bounds.x * scale, bounds.y * scale, bounds.width * scale, bounds.height * scale);
        let (cw, ch) = (self.state.output_width as f32, self.state.output_height as f32);
        let mut fit = (bw / cw).min(bh / ch);
        if let Some(max) = self.max_height {
            fit = fit.min(max / ch);
        }
        let (w, h) = (cw * fit, ch * fit);
        pipeline.canvas = [(bx + (bw - w) / 2.0).round(), (by + (bh - h) / 2.0).round(), w.round(), h.round()];
        pipeline.scene.prepare(device, queue, &self.state, &self.project, &self.theme, f64::from(fit));
        let mut stats = self.stats.lock().unwrap();
        if stats.recording {
            stats.frames.push(started);
            stats.prepare_ns.push(started.elapsed().as_nanos() as u64);
            stats.screen_index.push(self.screen.as_ref().map_or(-1, |(index, _)| *index));
        }
    }

    fn render(&self, pipeline: &ScenePipeline, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, clip_bounds: &Rectangle<u32>) {
        // OMAREEL_SPIKE_NO_SCENE=1 leaves the scene undrawn (uploads still happen), to tell its
        // GPU time from the rest of the window's.
        static NO_SCENE: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("OMAREEL_SPIKE_NO_SCENE").is_some());
        if *NO_SCENE {
            pipeline.scene.record_copies(encoder);
            return;
        }
        let [x, y, w, h] = pipeline.canvas;
        let left = (x.max(0.0) as u32).max(clip_bounds.x);
        let top = (y.max(0.0) as u32).max(clip_bounds.y);
        let right = ((x + w) as u32).min(clip_bounds.x + clip_bounds.width);
        let bottom = ((y + h) as u32).min(clip_bounds.y + clip_bounds.height);
        if right <= left || bottom <= top {
            return;
        }
        pipeline.scene.render(encoder, target, pipeline.canvas, [left, top, right - left, bottom - top], None);
    }
}
