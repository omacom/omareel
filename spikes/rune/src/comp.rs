//! The composition as a Rune view: a port of src/ui/qml/Composition.qml and the pieces it uses
//! (Background, RoundedFrame, CursorOverlay, CameraOverlay with BlurredBackdrop,
//! KeystrokeOverlay), with the same formulas.
//!
//! QML binds each item to `comp`; here one function builds the whole tree from the project's
//! settings ([`Settings`], once) and the frame's state ([`FrameState`], from omareel_core), and
//! Rune diffs it against the last frame. Every value is computed by omareel_core per frame, so the
//! tree is built with `instant_changes()`: Rune's own springs must not smooth it.
//!
//! QML's `opacity` multiplies into each child; Rune's `opacity` draws the subtree into a layer and
//! fades that, which is Qt's `layer.enabled`. `dim` is the QML behaviour and costs no layer, so
//! the port uses `dim` where QML fades a group.

use omareel_core::compat::qcolor::QColor;
use omareel_core::keystroke_track::KeystrokePill;
use omareel_core::motion_track::RippleSample;
use omareel_core::project::{Border, Project, Shadow as ShadowSettings};
use rune::tok::{Family, TextStyle};
use rune::{Color, El, Image, NativeSurface, Pixels, Shadow, SurfaceFit, el, row, text};

/// Rune's `Color` is linear light (sRGB primaries, straight alpha); Qt's colours are encoded.
pub fn color(c: &QColor) -> Color {
    let d = |v: i32| rune::color::srgb_decode(v as f32 / 255.0);
    Color { r: d(c.red()), g: d(c.green()), b: d(c.blue()), a: c.alpha() as f32 / 255.0 }
}

/// `SPIKE_EXPERIMENT=noblur`: the camera backdrop without its blur, to see what the blur costs.
pub static NO_BLUR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };

fn rgb(hex: u32) -> Color {
    let d = |v: u32| rune::color::srgb_decode(v as f32 / 255.0);
    Color { r: d((hex >> 16) & 0xff), g: d((hex >> 8) & 0xff), b: d(hex & 0xff), a: 1.0 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CameraShape {
    Round,
    Square,
    Rectangle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    LightArrow,
    DarkArrow,
    Hand,
    Dot,
}

#[derive(Clone)]
pub enum BackgroundFill {
    None,
    Solid(Color),
    /// CSS-style angle in degrees and up to 8 stops (Rune's limit; omareel's presets have 2–10).
    Gradient(f32, Vec<(f32, Color)>),
}

/// The project's settings the composition reads, and the recording's facts.
#[derive(Clone)]
pub struct Settings {
    pub background: BackgroundFill,
    pub frame_padding: f32,
    pub frame_radius: f32,
    pub frame_shadow: ShadowSettings,
    pub frame_border: Border,
    pub crop: [f32; 4],
    pub camera_shape: CameraShape,
    pub camera_square_full_frame: bool,
    pub camera_left: bool,
    pub camera_top: bool,
    pub camera_size: f32,
    pub camera_radius: f32,
    pub camera_rotation: i32,
    pub camera_shadow: ShadowSettings,
    pub camera_border: Border,
    pub camera_scale_during_zoom: f32,
    pub camera_offset: [f32; 2],
    pub cursor_visible: bool,
    pub cursor_size: f32,
    pub cursor_style: CursorStyle,
    pub ring_color: Color,
    pub keys_enabled: bool,
    /// 0..2 along x (left, centre, right), and top or not.
    pub keys_x: u8,
    pub keys_top: bool,
    pub keys_size: f32,
    pub theme_background: Color,
    pub theme_foreground: Color,
    pub source_width: f32,
    pub source_height: f32,
    pub capture_scale: f32,
    pub camera_available: bool,
    pub camera_source: [f32; 2],
}

impl Settings {
    pub fn new(project: &Project, source: (i32, i32), capture_scale: f64, camera_source: Option<(i32, i32)>) -> Settings {
        let bg = &project.background;
        let background = if bg.type_ == "none" {
            BackgroundFill::None
        } else if bg.type_ == "gradient" && bg.gradient.stops.len() >= 2 {
            let stops: Vec<(f32, Color)> = bg.gradient.stops.iter().take(8).map(|s| (s.position as f32, color(&s.color))).collect();
            BackgroundFill::Gradient(bg.gradient.angle as f32, stops)
        } else {
            BackgroundFill::Solid(if bg.color.is_valid() { color(&bg.color) } else { rgb(0x1a1b26) })
        };
        let camera = &project.camera;
        let cursor = &project.cursor;
        let keys = &project.keystrokes;
        let (theme_background, theme_foreground) = omareel_core::theme::render_colors();
        Settings {
            background,
            frame_padding: project.frame.padding as f32,
            frame_radius: project.frame.radius as f32,
            frame_shadow: project.frame.shadow.clone(),
            frame_border: project.frame.border.clone(),
            crop: [project.crop.x as f32, project.crop.y as f32, project.crop.width as f32, project.crop.height as f32],
            camera_shape: match camera.shape.as_str() {
                "square" => CameraShape::Square,
                "rectangle" => CameraShape::Rectangle,
                _ => CameraShape::Round,
            },
            camera_square_full_frame: camera.full_frame == "square",
            camera_left: camera.position.ends_with("left"),
            camera_top: camera.position.starts_with("top"),
            camera_size: camera.size as f32,
            camera_radius: camera.radius as f32,
            camera_rotation: camera.rotation,
            camera_shadow: camera.shadow.clone(),
            camera_border: camera.border.clone(),
            camera_scale_during_zoom: camera.scale_during_zoom as f32,
            camera_offset: [camera.offset.x as f32, camera.offset.y as f32],
            cursor_visible: cursor.visible,
            cursor_size: cursor.size as f32,
            cursor_style: match cursor.style.as_str() {
                "dark-arrow" => CursorStyle::DarkArrow,
                "hand" => CursorStyle::Hand,
                "dot" => CursorStyle::Dot,
                _ => CursorStyle::LightArrow,
            },
            ring_color: color(&cursor.ring_color),
            keys_enabled: keys.enabled,
            keys_x: if keys.position.ends_with("left") {
                0
            } else if keys.position.ends_with("right") {
                2
            } else {
                1
            },
            keys_top: keys.position.starts_with("top"),
            keys_size: keys.size as f32,
            theme_background: color(&theme_background),
            theme_foreground: color(&theme_foreground),
            source_width: source.0 as f32,
            source_height: source.1 as f32,
            capture_scale: capture_scale as f32,
            camera_available: camera_source.is_some(),
            camera_source: camera_source.map_or([640.0, 480.0], |(w, h)| [w as f32, h as f32]),
        }
    }
}

/// `SPIKE_EXPERIMENT` takes parts of the scene away, to see what a frame costs: noblur,
/// noshadow, noclip (square corners), nocamera, nokeys; nobg (no wallpaper), nocomp and novideo
/// are the callers'. Returns the experiment string.
pub fn apply_experiments(settings: &mut Settings) -> String {
    let experiment = std::env::var("SPIKE_EXPERIMENT").unwrap_or_default();
    if experiment.contains("noblur") {
        NO_BLUR.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    if experiment.contains("noshadow") {
        settings.frame_shadow.enabled = false;
        settings.camera_shadow.enabled = false;
    }
    if experiment.contains("noclip") {
        settings.frame_radius = 0.0;
        settings.camera_shape = CameraShape::Square;
        settings.camera_radius = 0.0;
    }
    if experiment.contains("nocamera") {
        settings.camera_available = false;
    }
    if experiment.contains("nokeys") {
        settings.keys_enabled = false;
    }
    experiment
}

/// Where a video's frames come from: a `NativeSurface` fed NV12 or BGRA planes (the preview), or
/// `Pixels` fed RGBA (the export, whose decoder makes RGBA).
#[derive(Clone)]
pub enum Video {
    Surface(NativeSurface),
    Pixels(Pixels),
}

impl Video {
    fn draw(&self, e: El, fit: SurfaceFit) -> El {
        match self {
            Video::Surface(s) => e.native_surface(s, fit),
            Video::Pixels(p) => e.pixels_fit(p, fit),
        }
    }
}

/// The pictures: the two videos, the wallpaper, the cursor images.
#[derive(Clone)]
pub struct Media {
    pub screen: Video,
    pub camera: Video,
    pub background: Option<Image>,
    pub cursors: [Image; 3],
}

/// The cursor SVGs, rasterised by Rune's image cache at the largest size they are drawn at.
pub fn cursor_images(cache: &rune::ImageCache, size_pt: f32, scale: f32) -> [Image; 3] {
    [
        cache.svg(&include_bytes!("../../../assets/cursors/arrow.svg")[..], size_pt, scale),
        cache.svg(&include_bytes!("../../../assets/cursors/arrow-dark.svg")[..], size_pt, scale),
        cache.svg(&include_bytes!("../../../assets/cursors/pointer.svg")[..], size_pt, scale),
    ]
}

/// One frame's state, from the editor's `PreviewComposition` or the exporter's
/// `CompositionState`.
#[derive(Clone, Default)]
pub struct FrameState {
    pub zoom: [f32; 3],
    pub camera_full: f32,
    pub bubble: f32,
    /// x, y, scale, opacity, rotation (degrees); and visible.
    pub cursor: [f32; 5],
    pub cursor_visible: bool,
    pub ripples: Vec<RippleSample>,
    pub pills: Vec<KeystrokePill>,
    pub camera_visible: bool,
}

impl FrameState {
    pub fn preview(c: &omareel_core::editor::PreviewComposition) -> FrameState {
        FrameState {
            zoom: [c.zoom.scale as f32, c.zoom.cx as f32, c.zoom.cy as f32],
            camera_full: c.layout.camera_full as f32,
            bubble: c.layout.bubble as f32,
            cursor: [c.cursor.x as f32, c.cursor.y as f32, c.cursor.scale as f32, c.cursor.opacity as f32, c.cursor.rotation as f32],
            cursor_visible: c.cursor.visible,
            ripples: c.ripples.clone(),
            pills: c.keystroke_pills.clone(),
            camera_visible: c.camera_visible,
        }
    }

    pub fn export(c: &omareel_core::export::composition::CompositionState) -> FrameState {
        FrameState {
            zoom: [c.zoom.scale as f32, c.zoom.cx as f32, c.zoom.cy as f32],
            camera_full: c.layout.camera_full as f32,
            bubble: c.layout.bubble as f32,
            cursor: [c.cursor.x as f32, c.cursor.y as f32, c.cursor.scale as f32, c.cursor.opacity as f32, c.cursor.rotation as f32],
            cursor_visible: c.cursor.visible,
            ripples: c.ripples.clone(),
            pills: c.keystroke_pills.clone(),
            camera_visible: c.camera_visible,
        }
    }
}

/// MultiEffect's shadow (`shadowBlur = min(1, blur / 64)` of `blurMax` 32 px) as a CSS box shadow.
fn shadow(s: &ShadowSettings, scale: f32, fade: f32) -> Shadow {
    let angle = (s.angle as f32).to_radians();
    Shadow {
        x: angle.cos() * s.distance as f32 * scale,
        y: angle.sin() * s.distance as f32 * scale,
        blur: (s.blur as f32 / 64.0).min(1.0) * 32.0 * scale,
        spread: 0.0,
        color: BLACK.alpha(s.intensity as f32 * fade),
        inset: false,
    }
}

fn border_color(b: &Border, fade: f32) -> Color {
    color(&b.color).alpha(b.alpha as f32 * fade)
}

/// A box at `(x, y)` in its parent, `w` x `h` points.
fn place(x: f32, y: f32, w: f32, h: f32) -> El {
    el().absolute(x, y).size(w.max(0.0), h.max(0.0))
}

/// Composition.qml at `w` x `h` points.
/// `instant`: every change lands at once (playback, export); otherwise an edit springs.
pub fn composition(s: &Settings, m: &Media, f: &FrameState, w: f32, h: f32, instant: bool) -> El {
    let r = h / 1080.0;
    let no_background = matches!(s.background, BackgroundFill::None);
    let pad = if no_background { 0.0 } else { w.min(h) * s.frame_padding };
    let (avail_w, avail_h) = (w - 2.0 * pad, h - 2.0 * pad);
    let frame_border = if !no_background && s.frame_border.enabled { s.frame_border.width as f32 * r } else { 0.0 };
    let source_aspect = (s.source_width * s.crop[2]) / (s.source_height * s.crop[3]).max(1.0);
    let video_w = if no_background { w } else { (avail_w - 2.0 * frame_border).min((avail_h - 2.0 * frame_border) * source_aspect).max(1.0) };
    let video_h = if no_background { h } else { video_w / source_aspect };
    let cursor_ref = (if s.capture_scale > 0.0 { s.capture_scale } else { 1.0 }) * video_w / (s.source_width * s.crop[2]).max(1.0);
    let camera_shown = s.camera_available && f.camera_visible;
    let camera_full = if camera_shown { f.camera_full } else { 0.0 };
    let covered = camera_full >= 1.0;
    let (fx, fy) = ((w - video_w) / 2.0, (h - video_h) / 2.0);

    // Rune blends every quad and has no opaque pass, so nothing hidden is drawn: the black
    // shows only where nothing covers it.
    let mut root = el().key("composition").size(w, h).clip_soft(0.0);
    if instant {
        root = root.instant_changes();
    }
    if covered || no_background {
        root = root.fill(BLACK);
    }
    if !covered && !no_background {
        root = root.child(background(s, m, w, h));
    }
    if !covered {
        let [scale, cx, cy] = f.zoom;
        let mut stage = place(0.0, 0.0, w, h).key("stage");
        if (scale - 1.0).abs() > 1e-4 {
            stage = stage.scale(scale).origin((fx + cx * video_w) / w, (fy + cy * video_h) / h);
        }
        stage = rounded_frame(stage, s, m, fx, fy, video_w, video_h, r, no_background);
        stage = stage.child(cursor_overlay(s, m, f, fx, fy, video_w, video_h, cursor_ref));
        root = root.child(stage);
    }
    if camera_shown && (camera_full > 0.0 || f.bubble > 0.0) {
        root = camera_overlay(root, s, m, f, w, h, camera_full);
    }
    if s.keys_enabled && !f.pills.is_empty() && !covered {
        root = root.child(keystrokes(s, &f.pills, w, h, r).dim(1.0 - camera_full));
    }
    root
}

/// Background.qml: a colour or gradient, or the wallpaper covering the canvas.
fn background(s: &Settings, m: &Media, w: f32, h: f32) -> El {
    let mut bg = place(0.0, 0.0, w, h).key("background");
    match &s.background {
        BackgroundFill::Solid(c) if m.background.is_none() => bg = bg.fill(*c),
        BackgroundFill::Solid(_) => {}
        // Background.qml's canvas gradient runs along (angle - 90°), as CSS does for `angle`.
        BackgroundFill::Gradient(angle, stops) => bg = bg.linear_gradient(angle.to_radians(), stops),
        BackgroundFill::None => {}
    }
    if let Some(image) = &m.background {
        bg = bg.child(place(0.0, 0.0, w, h).pixels_fit(image.pixels(), SurfaceFit::Fill));
    }
    bg
}

/// RoundedFrame.qml: the shadow-casting shape, the border, and the video masked to the radius.
#[allow(clippy::too_many_arguments)]
fn rounded_frame(stage: El, s: &Settings, m: &Media, x: f32, y: f32, w: f32, h: f32, r: f32, unframed: bool) -> El {
    let bw = if !unframed && s.frame_border.enabled { s.frame_border.width as f32 * r } else { 0.0 };
    let outer = if unframed { 0.0 } else { s.frame_radius * r + bw };
    let inner = if unframed { 0.0 } else { s.frame_radius * r };
    // QML's shadow-casting shape is black under the video; Rune's shadow needs no fill, and the
    // video covers the shape, so it stays transparent.
    let mut shape = place(x - bw, y - bw, w + 2.0 * bw, h + 2.0 * bw).key("frame-shadow").radius(outer);
    if s.frame_shadow.enabled && !unframed {
        shape = shape.box_shadow(shadow(&s.frame_shadow, r, 1.0));
    }
    let mut stage = stage.child(shape);
    if !unframed && s.frame_border.enabled {
        stage = stage.child(place(x - bw, y - bw, w + 2.0 * bw, h + 2.0 * bw).key("frame-border").radius(outer).fill(border_color(&s.frame_border, 1.0)));
    }
    let [cx, cy, cw, ch] = s.crop;
    let full_crop = cx == 0.0 && cy == 0.0 && cw == 1.0 && ch == 1.0;
    let video = if full_crop {
        // The surface clips itself to its corner radius.
        m.screen.draw(place(x, y, w, h).radius(inner), SurfaceFit::Fill)
    } else {
        let (vw, vh) = (w / cw.max(0.0001), h / ch.max(0.0001));
        place(x, y, w, h).clip_rounded(inner).child(m.screen.draw(place(-cx * vw, -cy * vh, vw, vh), SurfaceFit::Fill))
    };
    stage.child(video.key("frame-video"))
}

/// CursorOverlay.qml: click ripples and the pointer, over the frame.
#[allow(clippy::too_many_arguments)]
fn cursor_overlay(s: &Settings, m: &Media, f: &FrameState, x: f32, y: f32, w: f32, h: f32, r: f32) -> El {
    let mut overlay = place(x, y, w, h).key("cursor-overlay");
    for (i, ripple) in f.ripples.iter().enumerate() {
        let (rx, ry, p) = (ripple.x as f32, ripple.y as f32, ripple.progress as f32);
        if !(0.0..=1.0).contains(&rx) || !(0.0..=1.0).contains(&ry) {
            continue;
        }
        let size = 32.0 * s.cursor_size * r * (0.2 + 3.3 * (p / (0.15 / 0.45)).min(1.0));
        let opacity = 0.6 * if p < 0.05 { p / 0.05 } else if p < 0.8 { 1.0 - (p - 0.05) / 0.75 } else { 0.0 };
        overlay = overlay.child(
            place(rx * w - size / 2.0, ry * h - size / 2.0, size, size)
                .key(format!("ripple-{i}"))
                .radius(size / 2.0)
                .border(2.0 * r, s.ring_color)
                .dim(opacity),
        );
    }
    let [cx, cy, scale, opacity, rotation] = f.cursor;
    let inside = (0.0..=1.0).contains(&cx) && (0.0..=1.0).contains(&cy);
    if !s.cursor_visible || !inside || !f.cursor_visible {
        return overlay;
    }
    if s.cursor_style == CursorStyle::Dot {
        let size = 28.0 * s.cursor_size * r;
        let dot = place(cx * w - size / 2.0, cy * h - size / 2.0, size, size)
            .key("cursor")
            .radius(size / 2.0)
            .fill(s.ring_color.alpha(0.68))
            .border(1.5 * r, WHITE.alpha(0.45))
            .scale(scale)
            .dim(opacity);
        return overlay.child(dot);
    }
    let hand = s.cursor_style == CursorStyle::Hand;
    let image = match s.cursor_style {
        CursorStyle::DarkArrow => &m.cursors[1],
        CursorStyle::Hand => &m.cursors[2],
        _ => &m.cursors[0],
    };
    let size = 24.0 * s.cursor_size * r;
    let (hx, hy) = if hand { (8.0 / 24.0, 1.5 / 24.0) } else { (3.0 / 24.0, 2.0 / 24.0) };
    let mut pointer = place(cx * w - hx * size, cy * h - hy * size, size, size).key("cursor").pixels(image.pixels()).origin(0.0, 0.0);
    if (scale - 1.0).abs() > 1e-4 {
        pointer = pointer.scale(scale);
    }
    if rotation.abs() > 0.01 {
        pointer = pointer.rotate(rotation.to_radians());
    }
    overlay.child(pointer.dim(opacity))
}

/// CameraOverlay.qml: the webcam bubble, morphing into the full-frame camera (`full` 0 → 1).
fn camera_overlay(mut root: El, s: &Settings, m: &Media, f: &FrameState, out_w: f32, out_h: f32, full: f32) -> El {
    let r = out_h / 1080.0;
    let square_mode = s.camera_square_full_frame;
    let source_bw = if s.camera_border.enabled { s.camera_border.width as f32 * r } else { 0.0 };
    let bw = source_bw * (1.0 - full);
    let square_crop = s.camera_shape != CameraShape::Rectangle;
    let quarter_turn = s.camera_rotation == 90 || s.camera_rotation == 270;
    let source_aspect = s.camera_source[0] / s.camera_source[1].max(1.0);
    let aspect = if square_crop { 1.0 } else if quarter_turn { 1.0 / source_aspect } else { source_aspect };
    let zoom_progress = ((f.zoom[0] - 1.05) / 0.1).clamp(0.0, 1.0);
    let zoom_multiplier = 1.0 - (1.0 - s.camera_scale_during_zoom) * zoom_progress;
    let bubble_h = out_h * s.camera_size * zoom_multiplier;
    let bubble_w = bubble_h * aspect;
    let short = out_w.min(out_h);
    let (margin_x, margin_y) = (short * s.camera_offset[0] + source_bw, short * s.camera_offset[1] + source_bw);
    let bubble_x = if s.camera_left { margin_x } else { out_w - margin_x - bubble_w };
    let bubble_y = if s.camera_top { margin_y } else { out_h - margin_y - bubble_h };
    let bubble_radius = if s.camera_shape == CameraShape::Round { bubble_w.min(bubble_h) / 2.0 } else { s.camera_radius * r };
    let full_side = short;
    let (full_w, full_h) = if square_mode { (full_side, full_side) } else { (out_w, out_h) };
    let corner = bubble_radius * (1.0 - full);
    let outer = if corner > 0.0 { corner + bw } else { 0.0 };
    let presence = (full + f.bubble).min(1.0);
    let x = bubble_x + ((out_w - full_w) / 2.0 - bubble_x) * full;
    let y = bubble_y + ((out_h - full_h) / 2.0 - bubble_y) * full;
    let w = bubble_w + (full_w - bubble_w) * full;
    let h = bubble_h + (full_h - bubble_h) * full;
    let scale = 0.85 + 0.15 * presence;

    // BlurredBackdrop.qml, over the whole canvas behind the square. QML captures the camera into a
    // 24x24 texture and blurs that with MultiEffect (blurMax 64); Rune blurs the subtree on the
    // GPU, at its largest radius (64 device pixels).
    if square_mode && full > 0.0 {
        let fade = full * full;
        let cover_w = out_w.max(out_h * (w / h.max(1.0))) * 1.2;
        let cover_h = cover_w / (w / h.max(1.0)).max(0.01);
        let backdrop = place(0.0, 0.0, out_w, out_h)
            .key("camera-backdrop")
            .clip_soft(0.0)
            .child(place(0.0, 0.0, out_w, out_h).fill(rgb(0x0b0b0e)))
            .child(m.camera.draw(place((out_w - cover_w) / 2.0, (out_h - cover_h) / 2.0, cover_w, cover_h), SurfaceFit::Fill).blur(if NO_BLUR.load(std::sync::atomic::Ordering::Relaxed) { 0.0 } else { 64.0 * r }))
            .child(place(0.0, 0.0, out_w, out_h).fill(BLACK.alpha(0.22)))
            .dim(fade * presence);
        root = root.child(backdrop);
    }

    let mut camera = place(x, y, w, h).key("camera").scale(scale);
    let mut shape = place(-bw, -bw, w + 2.0 * bw, h + 2.0 * bw).radius(outer);
    if presence < 1.0 {
        // Shows through the fading picture, as in QML.
        shape = shape.fill(BLACK);
    }
    if s.camera_shadow.enabled {
        shape = shape.box_shadow(shadow(&s.camera_shadow, r, 1.0 - full));
    }
    camera = camera.child(shape);
    if s.camera_border.enabled {
        camera = camera.child(place(-bw, -bw, w + 2.0 * bw, h + 2.0 * bw).radius(outer).fill(border_color(&s.camera_border, 1.0 - full)));
    }
    let picture = if quarter_turn {
        // The surface is turned a quarter, so it is laid out with its sides swapped.
        place(0.0, 0.0, w, h).clip_rounded(outer).child(
            m.camera
                .draw(place((w - h) / 2.0, (h - w) / 2.0, h, w), SurfaceFit::Fill)
                .rotate((s.camera_rotation as f32).to_radians()),
        )
    } else if s.camera_rotation == 180 {
        m.camera.draw(place(0.0, 0.0, w, h).radius(outer), SurfaceFit::Fill).rotate(std::f32::consts::PI)
    } else {
        m.camera.draw(place(0.0, 0.0, w, h).radius(outer), SurfaceFit::Fill)
    };
    camera = camera.child(picture);
    root.child(camera.dim(presence))
}

/// KeystrokeOverlay.qml: a stack of pills, each a row of key caps joined by "+".
fn keystrokes(s: &Settings, pills: &[KeystrokePill], w: f32, h: f32, r: f32) -> El {
    let unit = s.keys_size * r;
    let pill_color = s.theme_background.alpha(0.85);
    let fg = s.theme_foreground;
    let margin = 40.0 * r;
    let mono = |size: f32, weight: f32| TextStyle { family: Family::SystemMono, ..TextStyle::new(size, weight) };
    let mut stack = rune::col().gap(8.0 * r);
    stack = match s.keys_x {
        0 => stack.items_start(),
        2 => stack.items_end(),
        _ => stack.items_center(),
    };
    for (i, pill) in pills.iter().enumerate() {
        let mut caps = row().h(52.0 * unit).px(12.0 * unit).gap(6.0 * unit).items_center();
        for (k, key) in pill.keys.iter().enumerate() {
            if k > 0 {
                caps = caps.child(text("+").font(mono(14.0 * unit, 400.0)).color(fg.alpha(0.65)));
            }
            caps = caps.child(
                row()
                    .h(30.0 * unit)
                    .px(7.0 * unit)
                    .items_center()
                    .radius(6.0 * unit)
                    .fill(fg.alpha(0.10))
                    .border(1.0 * unit, fg.alpha(0.22))
                    .child(text(key.as_str()).font(mono(14.0 * unit, 600.0)).color(fg)),
            );
        }
        let pill_el = caps.key(format!("pill-{i}-{}", pill.text)).radius(10.0 * unit).fill(pill_color).border(1.0 * r, fg.alpha(0.12));
        stack = stack.child(pill_el.dim(pill.opacity as f32));
    }
    // The stack sits `margin` in from the chosen edges of the canvas.
    let mut area = rune::col().absolute(0.0, 0.0).size(w, h).key("keystrokes").padding(margin);
    area = match s.keys_x {
        0 => area.items_start(),
        2 => area.items_end(),
        _ => area.items_center(),
    };
    if !s.keys_top {
        area = area.justify_end();
    }
    area.child(stack)
}
