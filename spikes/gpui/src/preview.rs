//! `omareel-gpui preview <bundle>`: the composition playing in a window at the display's rate,
//! with play/pause, a scrub bar and a time label. `--bench <seconds>` plays from the start
//! after a moment and prints frame statistics as JSON, then quits.

use crate::media::{Stream, StreamSpec, load_wallpaper};
use crate::scene::{self, Frame, Style};
use gpui::{
    App, AppContext, Bounds, Context, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Render, RenderImage, SharedString, Styled, Window,
    WindowBounds, WindowOptions, canvas, div, prelude::*, px, rgb, size,
};
use omareel_core::camera_timeline::map_camera_time;
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Options {
    pub bundle: String,
    pub bench: Option<f64>,
    /// The editor's preview pane caps the composition at 620 px high; 0 fills the window.
    pub max_height: f64,
    /// When the process started; the benchmark starts playing 1.5 s after it.
    pub launched: Instant,
}

struct Bench {
    seconds: f64,
    started: Option<Instant>,
    intervals: Vec<f64>,
    render_costs: Vec<f64>,
    last_frame: Option<Instant>,
    screen_frames: u64,
    skipped_at_start: u64,
}

pub struct Preview {
    document: Document,
    style: Style,
    focus: FocusHandle,
    screen_spec: StreamSpec,
    camera_spec: Option<StreamSpec>,
    screen: Option<Stream>,
    camera: Option<Stream>,
    shown_screen: Option<Arc<RenderImage>>,
    shown_camera: Option<(Arc<RenderImage>, Option<Arc<RenderImage>>)>,
    wallpaper: Option<Arc<RenderImage>>,
    playing: bool,
    position: f64,
    clock: Option<(Instant, f64)>,
    seek_pending: bool,
    last_seek: Option<Instant>,
    scrub_bounds: Rc<Cell<Bounds<Pixels>>>,
    max_height: f64,
    bench: Option<Bench>,
    opened: Instant,
}

impl Focusable for Preview {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

pub fn style_for(project: &omareel_core::project::Project) -> Style {
    let (theme_background, theme_foreground) = omareel_core::theme::render_colors();
    Style {
        project: project.clone(),
        theme_background,
        theme_foreground,
        mono_family: omareel_core::theme::monospace_family().unwrap_or_else(|| "monospace".into()).into(),
        subtitle_size: (omareel_core::theme::shell_font_base_size() as f64 * 1.083).round(),
    }
}

pub fn wallpaper_path(project: &omareel_core::project::Project) -> String {
    let background = &project.background;
    match background.type_.as_str() {
        "image" => background.image.clone(),
        "wallpaper" if background.wallpaper == "omarchy:current" => omareel_core::omarchy_paths::current_background(),
        "wallpaper" => background.wallpaper.clone(),
        _ => String::new(),
    }
}

impl Preview {
    fn new(options: &Options, window: &mut Window, cx: &mut Context<Self>) -> Preview {
        let probes = Probes::new(Arc::new(media_probe::probe_video));
        let mut document = Document::open(&options.bundle, &probes).expect("open the bundle");
        // The motion track (zoom spring, cursor) is built off the GUI thread by the Qt editor;
        // here it is built before the window shows.
        if let Some(job) = document.motion_job() {
            let track = job.run();
            document.apply_motion(track);
        }
        document.take_effects();
        let project = document.project().clone();
        let screen_spec = StreamSpec {
            path: document.video_path().to_owned(),
            fps: document.fps(),
            width: document.source_width(),
            height: document.source_height(),
            flip: false,
            backdrop: None,
            ahead: 4,
        };
        let camera_spec = document.has_camera().then(|| StreamSpec {
            path: document.camera_video_path().to_owned(),
            fps: document.recording().camera.map_or(30.0, |camera| camera.fps),
            width: document.camera_source_width(),
            height: document.camera_source_height(),
            flip: project.camera.flip_horizontal,
            backdrop: Some(project.camera.shape != "rectangle"),
            ahead: 3,
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut preview = Preview {
            style: style_for(&project),
            wallpaper: load_wallpaper(&wallpaper_path(&project)),
            document,
            focus,
            screen_spec,
            camera_spec,
            screen: None,
            camera: None,
            shown_screen: None,
            shown_camera: None,
            playing: false,
            position: 0.0,
            clock: None,
            seek_pending: true,
            last_seek: None,
            scrub_bounds: Rc::new(Cell::new(Bounds::default())),
            max_height: options.max_height,
            bench: options.bench.map(|seconds| Bench {
                seconds,
                started: None,
                intervals: Vec::new(),
                render_costs: Vec::new(),
                last_frame: None,
                screen_frames: 0,
                skipped_at_start: 0,
            }),
            opened: options.launched,
        };
        preview.restart_streams(false);
        preview
    }

    fn duration(&self) -> f64 {
        self.document.duration()
    }

    fn source_and_camera_time(&mut self, position: f64) -> (f64, f64) {
        self.document.set_output_position(position);
        let source = self.document.source_position();
        let camera = self
            .document
            .recording()
            .camera
            .map_or(source, |camera| map_camera_time(source, camera.offset, camera.duration).seconds);
        (source, camera)
    }

    /// Decoders from the playhead: to the end while playing, one frame while paused.
    fn restart_streams(&mut self, playing: bool) {
        let (source, camera_time) = self.source_and_camera_time(self.position);
        let remaining = |fps: f64, total: f64, from: f64| if playing { (total - from).max(1.0 / fps) } else { 1.0 / fps };
        let total = self.document.source_duration();
        self.screen = Some(Stream::start(&self.screen_spec, source, remaining(self.screen_spec.fps, total, source)));
        let camera_duration = self.document.recording().camera.map_or(0.0, |camera| camera.duration);
        self.camera = self
            .camera_spec
            .as_ref()
            .map(|spec| Stream::start(spec, camera_time, remaining(spec.fps, camera_duration, camera_time)));
        self.seek_pending = false;
        self.last_seek = Some(Instant::now());
    }

    fn set_playing(&mut self, playing: bool) {
        if playing == self.playing {
            return;
        }
        if playing && self.position >= self.duration() - 1e-3 {
            self.position = 0.0;
        }
        self.playing = playing;
        self.clock = playing.then(|| (Instant::now(), self.position));
        self.restart_streams(playing);
    }

    fn seek(&mut self, position: f64) {
        self.position = position.clamp(0.0, self.duration());
        if self.playing {
            self.clock = Some((Instant::now(), self.position));
        }
        self.seek_pending = true;
    }

    fn scrub_to(&mut self, x: Pixels) {
        let bounds = self.scrub_bounds.get();
        if bounds.size.width > px(0.0) {
            let fraction = ((x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0) as f64;
            self.seek(fraction * self.duration());
        }
    }

    /// Advances the playhead and the decoders; drops replaced pictures from GPUI's atlas.
    fn advance(&mut self, window: &mut Window) -> Frame {
        if let Some(bench) = &mut self.bench
            && bench.started.is_none()
            && self.opened.elapsed() > Duration::from_millis(1500)
        {
            bench.started = Some(Instant::now());
            self.position = 0.0;
            self.playing = false;
            self.set_playing(true);
            if let Some(screen) = &self.screen {
                self.bench.as_mut().map(|b| b.skipped_at_start = screen.skipped);
            }
        }
        if let Some((started, from)) = self.clock {
            self.position = from + started.elapsed().as_secs_f64();
            if self.position >= self.duration() {
                self.position = self.duration();
                self.playing = false;
                self.clock = None;
            }
        }
        let throttled = self.last_seek.is_some_and(|at| at.elapsed() < Duration::from_millis(35));
        if self.seek_pending && !throttled {
            self.restart_streams(self.playing);
        }
        let (source, camera_time) = self.source_and_camera_time(self.position);
        if let Some(screen) = &mut self.screen
            && screen.advance(source)
            && let Some(frame) = &screen.current
        {
            if let Some(old) = self.shown_screen.replace(frame.image.clone())
                && old.id != frame.image.id
            {
                let _ = window.drop_image(old);
            }
            if let Some(bench) = &mut self.bench
                && bench.started.is_some()
            {
                bench.screen_frames += 1;
            }
        }
        if let Some(camera) = &mut self.camera
            && camera.advance(camera_time)
            && let Some(frame) = &camera.current
        {
            if let Some((old, old_backdrop)) = self.shown_camera.replace((frame.image.clone(), frame.backdrop.clone())) {
                let _ = window.drop_image(old);
                if let Some(old_backdrop) = old_backdrop {
                    let _ = window.drop_image(old_backdrop);
                }
            }
        }
        self.document.take_effects();
        let document = &self.document;
        let comp = document.preview_composition(true, "");
        Frame {
            output_width: document.output_width() as f64,
            output_height: document.output_height() as f64,
            source_width: document.source_width() as f64,
            source_height: document.source_height() as f64,
            capture_scale: document.capture_scale(),
            camera_available: document.has_camera(),
            camera_visible: comp.camera_visible,
            camera_source_width: document.camera_source_width() as f64,
            camera_source_height: document.camera_source_height() as f64,
            zoom_scale: comp.zoom.scale,
            zoom_cx: comp.zoom.cx,
            zoom_cy: comp.zoom.cy,
            camera_full: comp.layout.camera_full,
            bubble: comp.layout.bubble,
            cursor_x: comp.cursor.x,
            cursor_y: comp.cursor.y,
            cursor_scale: comp.cursor.scale,
            cursor_opacity: comp.cursor.opacity,
            cursor_rotation: comp.cursor.rotation,
            ripples: comp.ripples,
            pills: comp.keystroke_pills,
            screen: self.shown_screen.clone(),
            camera: self.shown_camera.as_ref().map(|(image, _)| image.clone()),
            camera_blur: self.shown_camera.as_ref().and_then(|(_, backdrop)| backdrop.clone()),
            wallpaper: self.wallpaper.clone(),
        }
    }

    fn record_bench(&mut self, cost: Duration, cx: &mut Context<Self>) {
        let Some(bench) = &mut self.bench else { return };
        let Some(started) = bench.started else { return };
        let now = Instant::now();
        if let Some(last) = bench.last_frame {
            bench.intervals.push((now - last).as_secs_f64() * 1000.0);
        }
        bench.last_frame = Some(now);
        bench.render_costs.push(cost.as_secs_f64() * 1000.0);
        if started.elapsed().as_secs_f64() >= bench.seconds {
            let skipped = self.screen.as_ref().map_or(0, |s| s.skipped) - bench.skipped_at_start;
            let expected = (bench.seconds * self.screen_spec.fps).round() as i64;
            let mut intervals = bench.intervals.clone();
            intervals.sort_by(f64::total_cmp);
            let mut costs = bench.render_costs.clone();
            costs.sort_by(f64::total_cmp);
            let pick = |v: &[f64], q: f64| v.get(((v.len() as f64 - 1.0) * q).round() as usize).copied().unwrap_or(0.0);
            println!(
                "{{\"frames\":{},\"interval_ms\":{{\"median\":{:.2},\"p95\":{:.2},\"max\":{:.2}}},\"render_ms\":{{\"median\":{:.3},\"p95\":{:.3},\"max\":{:.3}}},\"screen_frames_shown\":{},\"screen_frames_expected\":{},\"dropped\":{},\"skipped_by_decoder_queue\":{}}}",
                intervals.len() + 1,
                pick(&intervals, 0.5),
                pick(&intervals, 0.95),
                intervals.last().copied().unwrap_or(0.0),
                pick(&costs, 0.5),
                pick(&costs, 0.95),
                costs.last().copied().unwrap_or(0.0),
                bench.screen_frames,
                expected,
                (expected - bench.screen_frames as i64).max(0),
                skipped,
            );
            cx.quit();
        }
    }
}

fn format_time(seconds: f64) -> String {
    let centis = (seconds.max(0.0) * 100.0).round() as i64;
    format!("{}:{:02}.{:02}", centis / 6000, centis / 100 % 60, centis % 100)
}

impl Render for Preview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cost = Instant::now();
        let frame = self.advance(window);
        let waiting = self.screen.as_ref().is_some_and(|s| s.current.is_none() && !s.exhausted());
        if self.playing || self.seek_pending || waiting || self.bench.as_ref().is_some_and(|b| b.started.is_none()) {
            window.request_animation_frame();
        }

        let viewport = window.viewport_size();
        let controls_height = 56.0;
        let margin = 24.0;
        let aspect = frame.output_width / frame.output_height;
        let mut height = (f64::from(viewport.height) - controls_height - 2.0 * margin).max(10.0);
        if self.max_height > 0.0 {
            height = height.min(self.max_height);
        }
        let width = (height * aspect).min(f64::from(viewport.width) - 2.0 * margin).max(10.0);
        let composition = scene::composition(&self.style, &frame, width);

        let fraction = (self.position / self.duration().max(1e-6)).clamp(0.0, 1.0) as f32;
        let accent = rgb(0x7aa2f7);
        let text = rgb(0xc0caf5);
        let bounds_cell = self.scrub_bounds.clone();
        let mono = self.style.mono_family.clone();
        let label: SharedString = format!("{} / {}", format_time(self.position), format_time(self.duration())).into();
        let element = div()
            .id("preview")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "space" {
                    let playing = this.playing;
                    this.set_playing(!playing);
                    cx.notify();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x16161e))
            .text_color(text)
            .child(div().flex_1().flex().items_center().justify_center().child(composition))
            .child(
                div()
                    .h(px(controls_height as f32))
                    .px(px(16.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(14.0))
                    .bg(rgb(0x1a1b26))
                    .child(
                        div()
                            .id("play")
                            .w(px(72.0))
                            .h(px(32.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.0))
                            .bg(rgb(0x2a2e42))
                            .hover(|s| s.bg(rgb(0x343a55)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if self.playing { "Pause" } else { "Play" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                let playing = this.playing;
                                this.set_playing(!playing);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("scrub")
                            .flex_1()
                            .h(px(32.0))
                            .flex()
                            .items_center()
                            .relative()
                            .child(canvas(move |bounds, _, _| bounds_cell.set(bounds), |_, _, _, _| {}).absolute().size_full())
                            .child(div().h(px(4.0)).w_full().rounded(px(2.0)).bg(rgb(0x2a2e42)))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(0.0))
                                    .h(px(4.0))
                                    .w(gpui::relative(fraction))
                                    .rounded(px(2.0))
                                    .bg(accent),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(gpui::relative(fraction))
                                    .ml(px(-7.0))
                                    .size(px(14.0))
                                    .rounded_full()
                                    .bg(accent),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    this.scrub_to(event.position.x);
                                    cx.notify();
                                }),
                            )
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                if event.dragging() {
                                    this.scrub_to(event.position.x);
                                    cx.notify();
                                }
                            })),
                    )
                    .child(div().flex_none().whitespace_nowrap().font_family(mono).child(label)),
            );
        self.record_bench(cost.elapsed(), cx);
        element
    }
}

pub fn run(options: Options) {
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("omareel-gpui".into()),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Preview::new(&options, window, cx)),
        )
        .expect("open the preview window");
        cx.activate(true);
    });
}
