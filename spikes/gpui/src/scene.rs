//! The composition (src/ui/qml/Composition.qml and the items it uses) as a GPUI element tree.
//!
//! Geometry is worked out in output pixels (1920x1080 for a 1080p export) with the QML's own
//! formulas, then multiplied by `k` to draw at any size: the editor's preview lays the
//! composition out at the output size and scales it the same way. GPUI has no transforms for
//! elements, so the zoom's scale about a point and the camera's scale about its centre are
//! applied to each rectangle here; both map rectangles to rectangles, so nothing is lost.

use gpui::{
    AnyElement, BoxShadow, Div, FontWeight, Hsla, ImageSource, IntoElement, ObjectFit, ParentElement, PathBuilder,
    PathStyle, RenderImage, SharedString, StrokeOptions, Styled, StyledImage, canvas, div, img, point, px,
};
use omareel_core::compat::qcolor::QColor;
use omareel_core::keystroke_track::KeystrokePill;
use omareel_core::motion_track::RippleSample;
use omareel_core::project::Project;
use std::sync::Arc;

/// Everything one frame of the composition shows: what QML's `comp` holds, plus the pictures.
#[derive(Clone)]
pub struct Frame {
    pub output_width: f64,
    pub output_height: f64,
    pub source_width: f64,
    pub source_height: f64,
    pub capture_scale: f64,
    pub camera_available: bool,
    pub camera_visible: bool,
    pub camera_source_width: f64,
    pub camera_source_height: f64,
    pub zoom_scale: f64,
    pub zoom_cx: f64,
    pub zoom_cy: f64,
    pub camera_full: f64,
    pub bubble: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_scale: f64,
    pub cursor_opacity: f64,
    pub cursor_rotation: f64,
    pub ripples: Vec<RippleSample>,
    pub pills: Vec<KeystrokePill>,
    pub screen: Option<Arc<RenderImage>>,
    pub camera: Option<Arc<RenderImage>>,
    /// The camera shrunk to 24x24 and blurred, for the full-frame square's backdrop.
    pub camera_blur: Option<Arc<RenderImage>>,
    pub wallpaper: Option<Arc<RenderImage>>,
}

/// What does not change from frame to frame.
#[derive(Clone)]
pub struct Style {
    pub project: Project,
    /// colors.toml's background and foreground (`renderTheme`).
    pub theme_background: QColor,
    pub theme_foreground: QColor,
    pub mono_family: SharedString,
    /// The theme's subtitle font size (`theme.font.subtitle`).
    pub subtitle_size: f64,
}

fn rgba(color: &QColor, alpha: f64) -> Hsla {
    gpui::Rgba {
        r: color.red() as f32 / 255.0,
        g: color.green() as f32 / 255.0,
        b: color.blue() as f32 / 255.0,
        a: (color.alpha() as f64 / 255.0 * alpha) as f32,
    }
    .into()
}

fn black(alpha: f64) -> Hsla {
    gpui::Rgba { r: 0.0, g: 0.0, b: 0.0, a: alpha as f32 }.into()
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn adjusted(self, by: f64) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2.0 * by, self.h + 2.0 * by)
    }
}

/// A scale about a point, the only transform the composition uses.
#[derive(Clone, Copy)]
struct Scale {
    ox: f64,
    oy: f64,
    s: f64,
}

impl Scale {
    fn rect(self, r: Rect) -> Rect {
        Rect::new(self.ox + (r.x - self.ox) * self.s, self.oy + (r.y - self.oy) * self.s, r.w * self.s, r.h * self.s)
    }

    fn point(self, x: f64, y: f64) -> (f64, f64) {
        (self.ox + (x - self.ox) * self.s, self.oy + (y - self.oy) * self.s)
    }

    fn len(self, l: f64) -> f64 {
        l * self.s
    }
}

/// Draws output-space rectangles at `k` view pixels per output pixel.
struct Painter {
    k: f64,
}

impl Painter {
    fn px(&self, v: f64) -> gpui::Pixels {
        px((v * self.k) as f32)
    }

    fn at(&self, r: Rect) -> Div {
        div().absolute().left(self.px(r.x)).top(self.px(r.y)).w(self.px(r.w)).h(self.px(r.h))
    }

    /// A rounded rectangle and its MultiEffect drop shadow (`shadowBlur` is a share of
    /// `blurMax`, 32 px at the output size).
    fn shadowed(&self, r: Rect, radius: f64, fill: Hsla, shadow: Option<(f64, f64, f64, f64)>) -> Div {
        let mut d = self.at(r).rounded(self.px(radius)).bg(fill);
        if let Some((dx, dy, blur, alpha)) = shadow {
            // Qt's shadow blur of `blur` x blurMax pixels is close to a Gaussian of a third
            // of that as its standard deviation, which is what GPUI's blur radius is.
            d = d.shadow(vec![
                BoxShadow::new(self.px(dx), self.px(dy), black(alpha)).blur_radius(self.px(blur * 32.0 / 3.0)),
            ]);
        }
        d
    }
}

/// The whole composition, `width` view pixels wide.
pub fn composition(style: &Style, f: &Frame, width: f64) -> AnyElement {
    let p = &style.project;
    let (w, h) = (f.output_width, f.output_height);
    let painter = Painter { k: width / w };
    let reference = h / 1080.0;
    let no_background = p.background.type_ == "none";
    let pad = if no_background { 0.0 } else { w.min(h) * p.frame.padding };
    let border = if !no_background && p.frame.border.enabled { p.frame.border.width * reference } else { 0.0 };
    let crop = &p.crop;
    let source_aspect = (f.source_width * crop.width) / (f.source_height * crop.height).max(1.0);
    let video_width = if no_background {
        w
    } else {
        ((w - 2.0 * pad - 2.0 * border).min((h - 2.0 * pad - 2.0 * border) * source_aspect)).max(1.0)
    };
    let video_height = if no_background { h } else { video_width / source_aspect };
    let frame = Rect::new((w - video_width) / 2.0, (h - video_height) / 2.0, video_width, video_height);
    let cursor_ref = (if f.capture_scale > 0.0 { f.capture_scale } else { 1.0 }) * video_width
        / (f.source_width * crop.width).max(1.0);
    let camera_shown = f.camera_available && f.camera_visible;
    let camera_full = if camera_shown { f.camera_full } else { 0.0 };
    let screen_covered = camera_full >= 1.0;
    let background_covered = camera_full >= 1.0;

    let mut root = div()
        .relative()
        .overflow_hidden()
        .w(painter.px(w))
        .h(painter.px(h))
        .bg(rgba(&p.background.color, 1.0));
    if !background_covered {
        root = root.child(background(&painter, style, f, w, h));
    }
    if !screen_covered {
        let zoom = Scale { ox: frame.x + f.zoom_cx * frame.w, oy: frame.y + f.zoom_cy * frame.h, s: f.zoom_scale };
        root = root.children(rounded_frame(&painter, p, f, frame, reference, no_background, zoom));
        root = root.children(cursor_overlay(&painter, p, f, frame, cursor_ref, zoom));
    }
    if camera_shown && (camera_full > 0.0 || f.bubble > 0.0) {
        root = root.children(camera_overlay(&painter, p, f, camera_full, reference));
    }
    if p.keystrokes.enabled && !f.pills.is_empty() && !screen_covered {
        root = root.child(keystrokes(&painter, style, f, reference, 1.0 - camera_full, w, h));
    }
    root.into_any_element()
}

fn background(painter: &Painter, style: &Style, f: &Frame, w: f64, h: f64) -> AnyElement {
    let p = &style.project.background;
    let whole = Rect::new(0.0, 0.0, w, h);
    match (p.type_.as_str(), &f.wallpaper) {
        ("image" | "wallpaper", Some(image)) => painter
            .at(whole)
            .child(img(ImageSource::Render(image.clone())).size_full().object_fit(ObjectFit::Cover))
            .into_any_element(),
        ("gradient", _) => {
            // GPUI's gradients have two stops; omareel's presets have two or three.
            let stops = &p.gradient.stops;
            let first = stops.first().map(|s| rgba(&s.color, 1.0)).unwrap_or_default();
            let last = stops.last().map(|s| rgba(&s.color, 1.0)).unwrap_or_default();
            painter
                .at(whole)
                .bg(gpui::linear_gradient(
                    p.gradient.angle as f32,
                    gpui::linear_color_stop(first, 0.0),
                    gpui::linear_color_stop(last, 1.0),
                ))
                .into_any_element()
        }
        ("none", _) => div().into_any_element(),
        _ => painter.at(whole).bg(rgba(&p.color, 1.0)).into_any_element(),
    }
}

/// RoundedFrame.qml inside the zoomed stage: shadow, border, then the video, masked round.
fn rounded_frame(
    painter: &Painter,
    p: &Project,
    f: &Frame,
    frame: Rect,
    reference: f64,
    unframed: bool,
    zoom: Scale,
) -> Vec<AnyElement> {
    let settings = &p.frame;
    let border = if !unframed && settings.border.enabled { settings.border.width * reference } else { 0.0 };
    let outer_radius = if unframed { 0.0 } else { settings.radius * reference + border };
    let angle = settings.shadow.angle.to_radians();
    let shape = zoom.rect(frame.adjusted(border));
    let mut items = Vec::new();
    let shadow = (settings.shadow.enabled && !unframed).then(|| {
        (
            zoom.len(angle.cos() * settings.shadow.distance * reference),
            zoom.len(angle.sin() * settings.shadow.distance * reference),
            zoom.len((settings.shadow.blur / 64.0).min(1.0)),
            settings.shadow.intensity,
        )
    });
    items.push(painter.shadowed(shape, zoom.len(outer_radius), black(1.0), shadow).into_any_element());
    if !unframed && settings.border.enabled {
        items.push(
            painter
                .at(shape)
                .rounded(painter.px(zoom.len(outer_radius)))
                .bg(rgba(&settings.border.color, settings.border.alpha))
                .into_any_element(),
        );
    }
    if let Some(screen) = &f.screen {
        let video = zoom.rect(frame);
        let radius = if unframed { 0.0 } else { zoom.len(settings.radius * reference) };
        items.push(
            img(ImageSource::Render(screen.clone()))
                .absolute()
                .left(painter.px(video.x))
                .top(painter.px(video.y))
                .w(painter.px(video.w))
                .h(painter.px(video.h))
                .object_fit(ObjectFit::Fill)
                .rounded(painter.px(radius))
                .into_any_element(),
        );
    }
    items
}

/// The arrow of assets/cursors/arrow.svg (hotspot at (3, 2) of its 24x24 box).
const ARROW: [(f64, f64); 7] =
    [(3.0, 2.0), (3.0, 18.6), (7.2, 14.9), (10.1, 21.2), (13.1, 19.9), (10.2, 13.7), (15.8, 13.7)];

/// CursorOverlay.qml: click ripples and the arrow, in the zoomed stage.
fn cursor_overlay(painter: &Painter, p: &Project, f: &Frame, frame: Rect, reference: f64, zoom: Scale) -> Vec<AnyElement> {
    let settings = &p.cursor;
    let mut items = Vec::new();
    for ripple in &f.ripples {
        if !(0.0..=1.0).contains(&ripple.x) || !(0.0..=1.0).contains(&ripple.y) {
            continue;
        }
        let size = 32.0 * settings.size * reference * (0.2 + 3.3 * (ripple.progress / (0.15 / 0.45)).min(1.0));
        let progress = ripple.progress;
        let opacity = 0.6
            * if progress < 0.05 {
                progress / 0.05
            } else if progress < 0.8 {
                1.0 - (progress - 0.05) / 0.75
            } else {
                0.0
            };
        let ring = Rect::new(frame.x + ripple.x * frame.w - size / 2.0, frame.y + ripple.y * frame.h - size / 2.0, size, size);
        items.push(
            painter
                .at(zoom.rect(ring))
                .rounded_full()
                .border(painter.px(zoom.len(2.0 * reference)))
                .border_color(rgba(&settings.ring_color, 1.0))
                .opacity(opacity as f32)
                .into_any_element(),
        );
    }
    let inside = (0.0..=1.0).contains(&f.cursor_x) && (0.0..=1.0).contains(&f.cursor_y);
    if settings.visible && inside && settings.style != "dot" {
        let width = 24.0 * settings.size * reference;
        // Item.TopLeft is the transform origin, so the box's corner stays put while the arrow
        // scales and turns about it.
        let left = frame.x + f.cursor_x * frame.w - 3.0 / 24.0 * width;
        let top = frame.y + f.cursor_y * frame.h - 2.0 / 24.0 * width;
        let (sin, cos) = f.cursor_rotation.to_radians().sin_cos();
        let unit = width / 24.0 * f.cursor_scale;
        let points: Vec<(f64, f64)> = ARROW
            .iter()
            .map(|&(x, y)| {
                let (x, y) = (x * unit, y * unit);
                zoom.point(left + x * cos - y * sin, top + x * sin + y * cos)
            })
            .collect();
        let (fill, outline) = if settings.style == "dark-arrow" {
            (QColor::parse("#111111"), QColor::parse("#ffffff"))
        } else {
            (QColor::parse("#ffffff"), QColor::parse("#111111"))
        };
        let (fill, outline) = (rgba(&fill, 1.0), rgba(&outline, 1.0));
        let stroke = (1.8 * unit * zoom.s * painter.k) as f32;
        let k = painter.k;
        items.push(
            div()
                .absolute()
                .size_full()
                .opacity(f.cursor_opacity as f32)
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let at = |&(x, y): &(f64, f64)| {
                                point(bounds.origin.x + px((x * k) as f32), bounds.origin.y + px((y * k) as f32))
                            };
                            let corners: Vec<_> = points.iter().map(at).collect();
                            let mut body = PathBuilder::fill();
                            body.add_polygon(&corners, true);
                            if let Ok(path) = body.build() {
                                window.paint_path(path, fill);
                            }
                            let mut edge = PathBuilder::stroke(px(stroke)).with_style(PathStyle::Stroke(
                                StrokeOptions::default()
                                    .with_line_width(stroke)
                                    .with_line_join(lyon::tessellation::LineJoin::Round),
                            ));
                            edge.add_polygon(&corners, true);
                            if let Ok(path) = edge.build() {
                                window.paint_path(path, outline);
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element(),
        );
    }
    items
}

/// CameraOverlay.qml: the bubble, morphing into the full-frame camera, over a blurred copy of
/// itself when that is a centred square.
fn camera_overlay(painter: &Painter, p: &Project, f: &Frame, full: f64, reference: f64) -> Vec<AnyElement> {
    let s = &p.camera;
    let (w, h) = (f.output_width, f.output_height);
    let square_mode = s.full_frame == "square";
    let source_border = if s.border.enabled { s.border.width * reference } else { 0.0 };
    let border = source_border * (1.0 - full);
    let square_crop = s.shape != "rectangle";
    let quarter_turn = s.rotation == 90 || s.rotation == 270;
    let source_aspect = f.camera_source_width / f.camera_source_height.max(1.0);
    let aspect = if square_crop {
        1.0
    } else if quarter_turn {
        1.0 / source_aspect
    } else {
        source_aspect
    };
    let zoom_progress = ((f.zoom_scale - 1.05) / 0.1).clamp(0.0, 1.0);
    let zoom_multiplier = 1.0 - (1.0 - s.scale_during_zoom) * zoom_progress;
    let bubble_height = h * s.size * zoom_multiplier;
    let bubble_width = bubble_height * aspect;
    let margin_x = w.min(h) * s.offset.x + source_border;
    let margin_y = w.min(h) * s.offset.y + source_border;
    let bubble_x = if s.position.contains("left") { margin_x } else { w - margin_x - bubble_width };
    let bubble_y = if s.position.contains("top") { margin_y } else { h - margin_y - bubble_height };
    let bubble_radius =
        if s.shape == "round" { bubble_width.min(bubble_height) / 2.0 } else { s.radius * reference };
    let full_side = w.min(h);
    let (full_width, full_height) = if square_mode { (full_side, full_side) } else { (w, h) };
    let corner_radius = bubble_radius * (1.0 - full);
    let outer_radius = if corner_radius > 0.0 { corner_radius + border } else { 0.0 };
    let angle = s.shadow.angle.to_radians();
    let (shadow_x, shadow_y) = (angle.cos() * s.shadow.distance * reference, angle.sin() * s.shadow.distance * reference);
    let item = Rect::new(
        bubble_x + ((w - full_width) / 2.0 - bubble_x) * full,
        bubble_y + ((h - full_height) / 2.0 - bubble_y) * full,
        bubble_width + (full_width - bubble_width) * full,
        bubble_height + (full_height - bubble_height) * full,
    );
    let presence = (full + f.bubble).min(1.0);
    // `scale` turns about the item's centre (Item.Center).
    let scale = Scale { ox: item.x + item.w / 2.0, oy: item.y + item.h / 2.0, s: 0.85 + 0.15 * presence };
    let mut items = Vec::new();

    if square_mode && full > 0.0 {
        // BlurredBackdrop.qml, spanning the canvas behind the camera.
        let canvas = scale.rect(Rect::new(0.0, 0.0, w, h));
        let backdrop_aspect = item.w / item.h.max(1.0);
        let blur_width = w.max(h * backdrop_aspect) * 1.2;
        let blur_height = blur_width / backdrop_aspect.max(0.01);
        let blurred = scale.rect(Rect::new((w - blur_width) / 2.0, (h - blur_height) / 2.0, blur_width, blur_height));
        let mut backdrop = painter
            .at(canvas)
            .overflow_hidden()
            .opacity((full * full * presence) as f32)
            .bg(rgba(&QColor::parse("#0b0b0e"), 1.0));
        if let Some(blur) = &f.camera_blur {
            backdrop = backdrop.child(
                img(ImageSource::Render(blur.clone()))
                    .absolute()
                    .left(painter.px(blurred.x - canvas.x))
                    .top(painter.px(blurred.y - canvas.y))
                    .w(painter.px(blurred.w))
                    .h(painter.px(blurred.h))
                    .object_fit(ObjectFit::Fill),
            );
        }
        backdrop = backdrop.child(div().absolute().size_full().bg(black(0.22)));
        items.push(backdrop.into_any_element());
    }

    let shape = scale.rect(item.adjusted(border));
    let radius = scale.len(outer_radius);
    let shadow = s.shadow.enabled.then(|| {
        (
            scale.len(shadow_x),
            scale.len(shadow_y),
            scale.len((s.shadow.blur / 64.0).min(1.0)),
            s.shadow.intensity * (1.0 - full),
        )
    });
    let mut layers = vec![painter.shadowed(shape, radius, black(1.0), shadow).into_any_element()];
    if s.border.enabled {
        layers.push(
            painter.at(shape).rounded(painter.px(radius)).bg(rgba(&s.border.color, s.border.alpha * (1.0 - full))).into_any_element(),
        );
    }
    if let Some(camera) = &f.camera {
        let video = scale.rect(item);
        layers.push(
            img(ImageSource::Render(camera.clone()))
                .absolute()
                .left(painter.px(video.x))
                .top(painter.px(video.y))
                .w(painter.px(video.w))
                .h(painter.px(video.h))
                .object_fit(ObjectFit::Cover)
                .rounded(painter.px(radius))
                .into_any_element(),
        );
    }
    items.push(div().absolute().size_full().opacity(presence as f32).children(layers).into_any_element());
    items
}

/// KeystrokeOverlay.qml: a column of pills, each a row of keycaps joined by "+".
fn keystrokes(painter: &Painter, style: &Style, f: &Frame, reference: f64, opacity: f64, w: f64, h: f64) -> AnyElement {
    let settings = &style.project.keystrokes;
    let size = settings.size;
    let text = &style.theme_foreground;
    let position = settings.position.as_str();
    let mut column = painter
        .at(Rect::new(0.0, 0.0, w, h))
        .p(painter.px(40.0 * reference))
        .flex()
        .flex_col()
        .gap(painter.px(8.0 * reference))
        .opacity(opacity as f32);
    column = if position.starts_with("top") { column.justify_start() } else { column.justify_end() };
    column = if position.ends_with("left") {
        column.items_start()
    } else if position.ends_with("right") {
        column.items_end()
    } else {
        column.items_center()
    };
    for pill in &f.pills {
        let mut row = div().flex().flex_row().items_center().gap(painter.px(6.0 * size * reference));
        for (index, key) in pill.keys.iter().enumerate() {
            row = row.child(
                div()
                    .h(painter.px(30.0 * size * reference))
                    .px(painter.px(7.0 * size * reference))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(painter.px(6.0 * size * reference))
                    .bg(rgba(text, 0.10))
                    .border(painter.px(reference))
                    .border_color(rgba(text, 0.22))
                    .text_color(rgba(text, 1.0))
                    .font_family(style.mono_family.clone())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(painter.px(14.0 * size * reference))
                    .child(SharedString::from(key.clone())),
            );
            if index + 1 < pill.keys.len() {
                row = row.child(
                    div()
                        .text_color(rgba(text, 0.65))
                        .font_family(style.mono_family.clone())
                        .text_size(painter.px(style.subtitle_size * size * reference))
                        .child("+"),
                );
            }
        }
        column = column.child(
            div()
                .h(painter.px(52.0 * size * reference))
                .px(painter.px(12.0 * size * reference))
                .flex()
                .items_center()
                .rounded(painter.px(10.0 * size * reference))
                .bg(rgba(&style.theme_background, 0.85))
                .border(painter.px(reference))
                .border_color(rgba(text, 0.12))
                .opacity(pill.opacity as f32)
                .child(row),
        );
    }
    column.into_any_element()
}
