//! Composition.qml and the items it uses (RoundedFrame, CursorOverlay, CameraOverlay,
//! BlurredBackdrop, KeystrokeOverlay, Background) as a list of GPU items, from the state the
//! core computes for a frame. Every binding here is the QML's; nothing is decided that the QML
//! did not decide.

use super::text::TextAtlas;
use bytemuck::{Pod, Zeroable};
use omareel_core::compat::qcolor::QColor;
use omareel_core::export::composition::CompositionState;
use omareel_core::project::Project;

pub const KIND_RECT: u32 = 0;
pub const KIND_SHADOW: u32 = 1;
pub const KIND_IMAGE: u32 = 2;
pub const KIND_TEXT: u32 = 3;
pub const KIND_RING: u32 = 4;

pub const TEX_WALLPAPER: u32 = 0;
pub const TEX_SCREEN: u32 = 1;
pub const TEX_CAMERA: u32 = 2;
pub const TEX_CURSOR: u32 = 3;
pub const TEX_BACKDROP: u32 = 5;

/// MultiEffect's default `blurMax`, which `shadowBlur` scales.
const SHADOW_BLUR_MAX: f64 = 32.0;
/// The Gaussian sigma for a MultiEffect blur of `radius` pixels.
const SIGMA_PER_RADIUS: f64 = 0.5;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Item {
    pub m: [f32; 4],
    pub t: [f32; 4],
    pub quad: [f32; 4],
    pub shape: [f32; 4],
    pub color: [f32; 4],
    pub color2: [f32; 4],
    pub uv: [f32; 4],
    pub p: [f32; 4],
    pub k: [u32; 4],
}

/// A 2D affine transform: `canvas = (a x + c y + tx, b x + d y + ty)`.
#[derive(Clone, Copy, Debug)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub tx: f64,
    pub ty: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

    /// Scaling by `s` about `(ox, oy)`, as a QML `Scale` with that origin.
    pub fn scale_about(s: f64, ox: f64, oy: f64) -> Affine {
        Affine { a: s, b: 0.0, c: 0.0, d: s, tx: ox * (1.0 - s), ty: oy * (1.0 - s) }
    }

    /// `self` after `inner`: `self(inner(p))`.
    pub fn then(self, inner: Affine) -> Affine {
        Affine {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            tx: self.a * inner.tx + self.c * inner.ty + self.tx,
            ty: self.b * inner.tx + self.d * inner.ty + self.ty,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn grown(self, by: f64) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }

    fn moved(self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    fn corners(self) -> [f32; 4] {
        [self.x as f32, self.y as f32, (self.x + self.w) as f32, (self.y + self.h) as f32]
    }
}

/// A straight-alpha colour.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgba(pub [f32; 4]);

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba([0.0; 4]);
    pub const BLACK: Rgba = Rgba([0.0, 0.0, 0.0, 1.0]);

    pub fn from_qcolor(color: &QColor) -> Rgba {
        Rgba([color.red_f(), color.green_f(), color.blue_f(), color.alpha_f()])
    }

    /// `Qt.alpha(color, a)` / `Qt.rgba(r, g, b, a)` over this colour's rgb.
    pub fn with_alpha(self, alpha: f64) -> Rgba {
        Rgba([self.0[0], self.0[1], self.0[2], alpha as f32])
    }

    pub fn hex(hex: u32) -> Rgba {
        Rgba([((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0, 1.0])
    }
}

/// What the scene draws from besides the state: `project.renderTheme` and the theme's fonts.
#[derive(Clone, Debug)]
pub struct RenderTheme {
    pub background: Rgba,
    pub foreground: Rgba,
    /// `theme.font.subtitle`, the "+" between keys.
    pub subtitle_size: f64,
    pub mono_family: String,
}

impl RenderTheme {
    pub fn load() -> RenderTheme {
        let theme = omareel_core::theme::Theme::new(None);
        let (background, foreground) = omareel_core::theme::render_colors();
        let subtitle = theme.palette.font_sizes.iter().find(|(k, _)| *k == "subtitle").map_or(13, |(_, v)| *v);
        RenderTheme {
            background: Rgba::from_qcolor(&background),
            foreground: Rgba::from_qcolor(&foreground),
            subtitle_size: f64::from(subtitle),
            mono_family: theme.mono_family,
        }
    }
}

/// The textures a frame samples, and their sizes (0 when there is none).
#[derive(Clone, Copy, Debug, Default)]
pub struct TextureSizes {
    pub wallpaper: (u32, u32),
    pub cursor: (u32, u32),
}

/// The blurred camera copy's capture: cover crop and orientation of the camera content.
#[derive(Clone, Copy, Debug)]
pub struct BackdropCapture {
    pub crop: [f32; 4],
    pub quarter_turns: u32,
    pub flip: bool,
}

/// One frame of the scene.
pub struct Frame {
    pub canvas: (f64, f64),
    pub items: Vec<Item>,
    pub backdrop: Option<BackdropCapture>,
    /// The cursor image the frame wants, as (style, pixel size).
    pub cursor_image: Option<(String, u32)>,
}

struct Builder {
    items: Vec<Item>,
}

impl Builder {
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, m: Affine, kind: u32, quad: Rect, shape: Rect, color: Rgba, color2: Rgba, uv: [f32; 4], p: [f64; 4], k: [u32; 3]) {
        if p[2] <= 0.0 || quad.w <= 0.0 || quad.h <= 0.0 {
            return;
        }
        self.items.push(Item {
            m: [m.a as f32, m.b as f32, m.c as f32, m.d as f32],
            t: [m.tx as f32, m.ty as f32, 0.0, 0.0],
            quad: quad.corners(),
            shape: shape.corners(),
            color: color.0,
            color2: color2.0,
            uv,
            p: p.map(|v| v as f32),
            k: [kind, k[0], k[1], k[2]],
        });
    }

    /// A QML Rectangle: `rect` with `radius`, filled, with a border inside its edge.
    fn rect(&mut self, m: Affine, rect: Rect, radius: f64, fill: Rgba, border_width: f64, border: Rgba, opacity: f64) {
        if fill.0[3] <= 0.0 && (border_width <= 0.0 || border.0[3] <= 0.0) {
            return;
        }
        // One pixel of slack for the antialiased edge.
        self.push(m, KIND_RECT, rect.grown(1.0), rect, fill, border, [0.0; 4], [radius, border_width, opacity, 1.0], [0, 0, 0]);
    }

    /// MultiEffect's drop shadow of a rounded rectangle.
    fn shadow(&mut self, m: Affine, rect: Rect, radius: f64, blur_radius: f64, color: Rgba, opacity: f64) {
        let sigma = (blur_radius * SIGMA_PER_RADIUS).max(0.5);
        self.push(m, KIND_SHADOW, rect.grown(3.0 * sigma + 1.0), rect, color, Rgba::TRANSPARENT, [0.0; 4], [radius, sigma, opacity, 1.0], [0, 0, 0]);
    }

    #[allow(clippy::too_many_arguments)]
    fn image(&mut self, m: Affine, rect: Rect, radius: f64, texture: u32, uv: [f64; 4], quarter_turns: u32, flip: bool, opacity: f64, saturation: f64) {
        let quad = if radius > 0.0 { rect.grown(1.0) } else { rect };
        self.push(
            m,
            KIND_IMAGE,
            quad,
            rect,
            Rgba::TRANSPARENT,
            Rgba::TRANSPARENT,
            uv.map(|v| v as f32),
            [radius, 0.0, opacity, saturation],
            [texture, quarter_turns, u32::from(flip)],
        );
    }
}

/// `coverCrop`: the centred part of a `source_width` x `source_height` frame that fills an item
/// of this size.
fn cover_crop(source_width: f64, source_height: f64, item_width: f64, item_height: f64) -> [f64; 4] {
    if source_width <= 0.0 || source_height <= 0.0 || item_width <= 0.0 || item_height <= 0.0 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let source_aspect = source_width / source_height;
    let item_aspect = item_width / item_height;
    if source_aspect > item_aspect {
        let visible = item_aspect / source_aspect;
        return [(1.0 - visible) / 2.0, 0.0, visible, 1.0];
    }
    let visible = source_aspect / item_aspect;
    [0.0, (1.0 - visible) / 2.0, 1.0, visible]
}

fn rad(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// Builds the frame. `pixels_per_unit` is how many target pixels one canvas unit covers, so
/// labels are rasterised at the size they are shown at.
pub fn build(
    state: &CompositionState,
    project: &Project,
    theme: &RenderTheme,
    textures: TextureSizes,
    text: &mut TextAtlas,
    pixels_per_unit: f64,
) -> Frame {
    let mut b = Builder { items: Vec::with_capacity(64) };
    let width = f64::from(state.output_width);
    let height = f64::from(state.output_height);
    let reference = height / 1080.0;
    let crop = project.crop;
    let source_width = f64::from(state.source_width);
    let source_height = f64::from(state.source_height);
    let no_background = project.background.type_ == "none";
    let pad = if no_background { 0.0 } else { width.min(height) * project.frame.padding };
    let available_width = width - 2.0 * pad;
    let available_height = height - 2.0 * pad;
    let frame_settings = &project.frame;
    let border_width =
        if !no_background && frame_settings.border.enabled { frame_settings.border.width * reference } else { 0.0 };
    let source_aspect = (source_width * crop.width) / (source_height * crop.height).max(1.0);
    let video_width = if no_background {
        width
    } else {
        (available_width - 2.0 * border_width).min((available_height - 2.0 * border_width) * source_aspect).max(1.0)
    };
    let video_height = if no_background { height } else { video_width / source_aspect };
    let capture_scale = if state.capture_scale > 0.0 { state.capture_scale } else { 1.0 };
    let cursor_reference = capture_scale * video_width / (source_width * crop.width).max(1.0);
    let camera_shown = state.camera_available && state.camera_visible;
    let camera_full = if camera_shown { state.layout.camera_full } else { 0.0 };
    let media_active = state.media.active;
    let screen_covered = camera_full >= 1.0 || media_active;
    let background_covered = camera_full >= 1.0 || (media_active && state.media.ready && state.media.fit != "framed");

    // Background.qml.
    if !background_covered {
        let background = &project.background;
        let image_visible = (background.type_ == "image" || background.type_ == "wallpaper") && textures.wallpaper.0 > 0;
        let canvas = Rect::new(0.0, 0.0, width, height);
        if background.type_ != "none" && (background.type_ == "color" || !image_visible) {
            let color = if background.color.is_valid() { Rgba::from_qcolor(&background.color) } else { Rgba::hex(0x1a1b26) };
            b.rect(Affine::IDENTITY, canvas, 0.0, color, 0.0, Rgba::TRANSPARENT, 1.0);
        }
        if image_visible {
            // fillMode: Image.PreserveAspectCrop.
            let (iw, ih) = (f64::from(textures.wallpaper.0), f64::from(textures.wallpaper.1));
            let uv = cover_crop(iw, ih, width, height);
            b.image(Affine::IDENTITY, canvas, 0.0, TEX_WALLPAPER, uv, 0, false, 1.0, 1.0);
        }
    }

    // The stage: RoundedFrame and CursorOverlay under the zoom.
    let frame = Rect::new((width - video_width) / 2.0, (height - video_height) / 2.0, video_width, video_height);
    let mut cursor_image = None;
    if !screen_covered {
        let zoom = state.zoom;
        let stage = Affine::scale_about(zoom.scale, frame.x + zoom.cx * frame.w, frame.y + zoom.cy * frame.h);
        let unframed = no_background;
        let outer_radius = if unframed { 0.0 } else { frame_settings.radius * reference + border_width };
        let outer = frame.grown(border_width);
        let shadow = &frame_settings.shadow;
        let shadow_dx = rad(shadow.angle).cos() * shadow.distance * reference;
        let shadow_dy = rad(shadow.angle).sin() * shadow.distance * reference;
        // shadowShape, a black rectangle the MultiEffect both shadows and draws again.
        if shadow.enabled && !unframed {
            b.shadow(stage, outer.moved(shadow_dx, shadow_dy), outer_radius, (shadow.blur / 64.0).min(1.0) * SHADOW_BLUR_MAX, Rgba::BLACK, shadow.intensity);
        }
        b.rect(stage, outer, outer_radius, Rgba::BLACK, 0.0, Rgba::TRANSPARENT, 1.0);
        if !unframed && frame_settings.border.enabled {
            let color = Rgba::from_qcolor(&frame_settings.border.color).with_alpha(frame_settings.border.alpha);
            b.rect(stage, outer, outer_radius, color, 0.0, Rgba::TRANSPARENT, 1.0);
        }
        let video_radius = if unframed { 0.0 } else { frame_settings.radius * reference };
        b.image(stage, frame, video_radius, TEX_SCREEN, [crop.x, crop.y, crop.width, crop.height], 0, false, 1.0, 1.0);

        // CursorOverlay.qml, with ref = cursorRef.
        let settings = &project.cursor;
        let cursor = state.cursor;
        let size = settings.size * cursor_reference;
        for ripple in &state.ripples {
            if !(0.0..=1.0).contains(&ripple.x) || !(0.0..=1.0).contains(&ripple.y) {
                continue;
            }
            let diameter = 32.0 * size * (0.2 + 3.3 * (ripple.progress / (0.15 / 0.45)).min(1.0));
            let progress = ripple.progress;
            let opacity = 0.6
                * if progress < 0.05 {
                    progress / 0.05
                } else if progress < 0.8 {
                    1.0 - (progress - 0.05) / 0.75
                } else {
                    0.0
                };
            let stroke = 2.0 * cursor_reference;
            let cx = frame.x + ripple.x * frame.w;
            let cy = frame.y + ripple.y * frame.h;
            let r = diameter / 2.0;
            let shape = Rect::new(cx - r, cy - r, diameter, diameter);
            b.push(stage, KIND_RING, shape.grown(stroke + 1.0), shape, Rgba::from_qcolor(&settings.ring_color), Rgba::TRANSPARENT, [0.0; 4], [r, stroke, opacity, 1.0], [0, 0, 0]);
        }
        let style = if settings.style.is_empty() { "light-arrow" } else { settings.style.as_str() };
        let inside = (0.0..=1.0).contains(&cursor.x) && (0.0..=1.0).contains(&cursor.y);
        if settings.visible && inside {
            if style == "dot" {
                let side = 28.0 * size;
                let cx = frame.x + cursor.x * frame.w;
                let cy = frame.y + cursor.y * frame.h;
                // scale about the item's centre.
                let m = stage.then(Affine::scale_about(cursor.scale, cx, cy));
                let ring = Rgba::from_qcolor(&settings.ring_color);
                b.rect(m, Rect::new(cx - side / 2.0, cy - side / 2.0, side, side), side / 2.0, ring.with_alpha(0.68), 1.5 * cursor_reference, Rgba([1.0, 1.0, 1.0, 0.45]), cursor.opacity);
            } else {
                let hand = style == "hand";
                let side = 24.0 * size;
                let source_size = (24.0 * size * 4.0).round().max(1.0) as u32;
                cursor_image = Some((style.to_owned(), source_size));
                let x = frame.x + cursor.x * frame.w - if hand { 8.0 / 24.0 } else { 3.0 / 24.0 } * side;
                let y = frame.y + cursor.y * frame.h - if hand { 1.5 / 24.0 } else { 2.0 / 24.0 } * side;
                // transformOrigin: Item.TopLeft, so rotation and scale turn about (x, y).
                let (sin, cos) = rad(cursor.rotation).sin_cos();
                let s = cursor.scale;
                let local = Affine { a: cos * s, b: sin * s, c: -sin * s, d: cos * s, tx: x, ty: y };
                if textures.cursor.0 > 0 {
                    b.image(stage.then(local), Rect::new(0.0, 0.0, side, side), 0.0, TEX_CURSOR, [0.0, 0.0, 1.0, 1.0], 0, false, cursor.opacity, 1.0);
                }
            }
        }
    }

    // CameraOverlay.qml.
    let mut backdrop = None;
    let camera = &project.camera;
    let bubble_opacity = state.layout.bubble;
    if camera_shown && !media_active && (camera_full > 0.0 || bubble_opacity > 0.0) {
        let full = camera_full;
        let camera_reference = height / 1080.0;
        let full_mode_square = camera.full_frame == "square";
        let source_border_width = if camera.border.enabled { camera.border.width * camera_reference } else { 0.0 };
        let border = source_border_width * (1.0 - full);
        let square_crop = camera.shape != "rectangle";
        let quarter_turn = camera.rotation == 90 || camera.rotation == 270;
        let camera_source_width = f64::from(state.camera_source_width);
        let camera_source_height = f64::from(state.camera_source_height);
        let camera_aspect = camera_source_width / camera_source_height.max(1.0);
        let aspect = if square_crop { 1.0 } else if quarter_turn { 1.0 / camera_aspect } else { camera_aspect };
        let zoom_progress = ((state.zoom.scale - 1.05) / 0.1).clamp(0.0, 1.0);
        let zoom_multiplier = 1.0 - (1.0 - camera.scale_during_zoom) * zoom_progress;
        let bubble_height = height * camera.size * zoom_multiplier;
        let bubble_width = bubble_height * aspect;
        let margin_x = width.min(height) * camera.offset.x + source_border_width;
        let margin_y = width.min(height) * camera.offset.y + source_border_width;
        let bubble_x = if camera.position.contains("left") { margin_x } else { width - margin_x - bubble_width };
        let bubble_y = if camera.position.contains("top") { margin_y } else { height - margin_y - bubble_height };
        let bubble_radius =
            if camera.shape == "round" { bubble_width.min(bubble_height) / 2.0 } else { camera.radius * camera_reference };
        let full_side = width.min(height);
        let full_width = if full_mode_square { full_side } else { width };
        let full_height = if full_mode_square { full_side } else { height };
        let corner_radius = bubble_radius * (1.0 - full);
        let outer_radius = if corner_radius > 0.0 { corner_radius + border } else { 0.0 };
        let shadow_angle = rad(camera.shadow.angle);
        let shadow_x = shadow_angle.cos() * camera.shadow.distance * camera_reference;
        let shadow_y = shadow_angle.sin() * camera.shadow.distance * camera_reference;
        let item = Rect::new(
            bubble_x + ((width - full_width) / 2.0 - bubble_x) * full,
            bubble_y + ((height - full_height) / 2.0 - bubble_y) * full,
            bubble_width + (full_width - bubble_width) * full,
            bubble_height + (full_height - bubble_height) * full,
        );
        let presence = (full + bubble_opacity).min(1.0);
        // scale: .85 + .15 * presence, about the item's centre; opacity: presence.
        let m = Affine::scale_about(0.85 + 0.15 * presence, item.x + item.w / 2.0, item.y + item.h / 2.0);
        let opacity = presence;
        // cameraRotated is the content before the quarter turn; its cover crop is of that size.
        let (content_width, content_height) = if quarter_turn { (item.h, item.w) } else { (item.w, item.h) };
        let content_crop = cover_crop(camera_source_width, camera_source_height, content_width, content_height);
        let quarter_turns = (camera.rotation.rem_euclid(360) / 90) as u32;

        // BlurredBackdrop.qml, which spans the canvas.
        if full_mode_square && full > 0.0 {
            let canvas = Rect::new(0.0, 0.0, width, height);
            let backdrop_opacity = opacity * full * full;
            b.rect(m, canvas, 0.0, Rgba::hex(0x0b0b0e), 0.0, Rgba::TRANSPARENT, backdrop_opacity);
            let source_aspect = item.w / item.h.max(1.0);
            let blurred_width = width.max(height * source_aspect) * 1.2;
            let blurred_height = blurred_width / source_aspect.max(0.01);
            let blurred = Rect::new((width - blurred_width) / 2.0, (height - blurred_height) / 2.0, blurred_width, blurred_height);
            // MultiEffect saturation .1 (0 leaves it as it is).
            b.image(m, blurred, 0.0, TEX_BACKDROP, [0.0, 0.0, 1.0, 1.0], 0, false, backdrop_opacity, 1.1);
            b.rect(m, canvas, 0.0, Rgba::BLACK, 0.0, Rgba::TRANSPARENT, backdrop_opacity * 0.22);
            backdrop = Some(BackdropCapture {
                crop: content_crop.map(|v| v as f32),
                quarter_turns,
                flip: camera.flip_horizontal,
            });
        }
        let outer = item.grown(border);
        if camera.shadow.enabled {
            b.shadow(m, outer.moved(shadow_x, shadow_y), outer_radius, (camera.shadow.blur / 64.0).min(1.0) * SHADOW_BLUR_MAX, Rgba::BLACK, opacity * camera.shadow.intensity * (1.0 - full));
        }
        b.rect(m, outer, outer_radius, Rgba::BLACK, 0.0, Rgba::TRANSPARENT, opacity);
        if camera.border.enabled {
            let color = Rgba::from_qcolor(&camera.border.color).with_alpha(camera.border.alpha * (1.0 - full));
            b.rect(m, outer, outer_radius, color, 0.0, Rgba::TRANSPARENT, opacity);
        }
        b.image(m, item, outer_radius, TEX_CAMERA, content_crop, quarter_turns, camera.flip_horizontal, opacity, 1.0);
    }

    // KeystrokeOverlay.qml.
    let keys = &project.keystrokes;
    if keys.enabled && !state.keystroke_pills.is_empty() && !screen_covered {
        let overlay_opacity = 1.0 - camera_full;
        let s = keys.size * reference;
        let text_color = theme.foreground;
        let pill_color = theme.background.with_alpha(0.85);
        let label_size = 14.0 * s;
        let plus_size = theme.subtitle_size * s;
        let gap = 6.0 * s;
        let to_units = |pixels: f32| f64::from(pixels) / pixels_per_unit;
        // Measure each pill's row first: the column is anchored by its edges.
        struct Key {
            label: super::text::Label,
            width: f64,
        }
        let plus = text.label("+", (plus_size * pixels_per_unit) as f32, 400);
        let rows: Vec<(Vec<Key>, f64, f64)> = state
            .keystroke_pills
            .iter()
            .map(|pill| {
                let keys: Vec<Key> = pill
                    .keys
                    .iter()
                    .map(|key| {
                        let label = text.label(key, (label_size * pixels_per_unit) as f32, 600);
                        Key { width: to_units(label.width) + 14.0 * s, label }
                    })
                    .collect();
                let mut row_width = 0.0;
                for (i, key) in keys.iter().enumerate() {
                    row_width += key.width;
                    if i + 1 < keys.len() {
                        row_width += gap + to_units(plus.width) + gap;
                    }
                }
                let row_height = (30.0 * s).max(if keys.len() > 1 { to_units(plus.height) } else { 0.0 });
                (keys, row_width, row_height)
            })
            .collect();
        let pill_height = 52.0 * s;
        let spacing = 8.0 * reference;
        let column_width = rows.iter().map(|(_, w, _)| w + 24.0 * s).fold(0.0, f64::max);
        let column_height = rows.len() as f64 * pill_height + (rows.len().saturating_sub(1)) as f64 * spacing;
        let margin = 40.0 * reference;
        let position = keys.position.as_str();
        let column_x = if position.ends_with("left") {
            margin
        } else if position.ends_with("right") {
            width - margin - column_width
        } else {
            (width - column_width) / 2.0
        };
        let column_y = if position.starts_with("top") { margin } else { height - margin - column_height };
        let border_width = reference;
        for (i, ((keys, row_width, row_height), pill)) in rows.iter().zip(&state.keystroke_pills).enumerate() {
            let opacity = overlay_opacity * pill.opacity;
            let pill_rect = Rect::new(column_x, column_y + i as f64 * (pill_height + spacing), row_width + 24.0 * s, pill_height);
            b.rect(Affine::IDENTITY, pill_rect, 10.0 * s, pill_color, border_width, text_color.with_alpha(0.12), opacity);
            let mut x = pill_rect.x + (pill_rect.w - row_width) / 2.0;
            let row_y = pill_rect.y + (pill_rect.h - row_height) / 2.0;
            for (j, key) in keys.iter().enumerate() {
                let key_rect = Rect::new(x, row_y, key.width, 30.0 * s);
                b.rect(Affine::IDENTITY, key_rect, 6.0 * s, text_color.with_alpha(0.10), border_width, text_color.with_alpha(0.22), opacity);
                label_item(&mut b, key.label, key_rect, pixels_per_unit, text_color, opacity);
                x += key.width;
                if j + 1 < keys.len() {
                    x += gap;
                    let plus_rect = Rect::new(x, row_y, to_units(plus.width), *row_height);
                    label_item(&mut b, plus, plus_rect, pixels_per_unit, text_color.with_alpha(0.65), opacity);
                    x += to_units(plus.width) + gap;
                }
            }
        }
    }

    Frame { canvas: (width, height), items: b.items, backdrop, cursor_image }
}

/// A label centred in `area` (anchors.centerIn), its bitmap drawn pixel for pixel.
fn label_item(b: &mut Builder, label: super::text::Label, area: Rect, pixels_per_unit: f64, color: Rgba, opacity: f64) {
    let [ax, ay, aw, ah] = label.atlas;
    let text_width = f64::from(label.width) / pixels_per_unit;
    let text_height = f64::from(label.height) / pixels_per_unit;
    // Snap to the pixel grid so glyphs stay crisp; the bitmap starts a pixel left of the text.
    let snap = |v: f64| (v * pixels_per_unit).round() / pixels_per_unit;
    let x = snap(area.x + (area.w - text_width) / 2.0) - 1.0 / pixels_per_unit;
    let y = snap(area.y + (area.h - text_height) / 2.0);
    let rect = Rect::new(x, y, f64::from(aw) / pixels_per_unit, f64::from(ah) / pixels_per_unit);
    let size = f64::from(super::text::ATLAS_SIZE);
    let uv = [ax as f32 / size as f32, ay as f32 / size as f32, aw as f32 / size as f32, ah as f32 / size as f32];
    b.push(Affine::IDENTITY, KIND_TEXT, rect, rect, color, Rgba::TRANSPARENT, uv, [0.0, 0.0, opacity, 1.0], [0, 0, 0]);
}
