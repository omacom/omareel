//! Annotations drawn over the recording: project.json `overlays`.
//!
//! Each item has a `kind` and an `anchor`, which decides both its coordinate space and its
//! time base:
//! - `"screen"`: `rect` is 0–1 of the recording's full frame (before the project's crop), and
//!   `start`/`end` are SOURCE seconds. The item belongs to a moment and a place in the recording:
//!   it follows zooms and the crop, survives cuts the way zooms do, and hides while an inserted
//!   video plays. Blur regions are always screen-anchored.
//! - `"frame"`: `rect` is 0–1 of the output canvas, and `start`/`end` are OUTPUT seconds. Titles
//!   and cards sit on the edited timeline, not on the recording: a title at 0–3 s stays at the
//!   start whatever is trimmed under it, and an output-range cut before an end card moves it
//!   along with the end. Title/end cards are always frame-anchored.
//!
//! The list is written only when it is non-empty, so projects without overlays stay byte for
//! byte as they were.

use crate::compat::cpp;
use crate::compat::geometry::{PointF, RectF};
use crate::compat::qcolor::QColor;
use crate::compat::qjson::{Object, Value};

pub const OVERLAY_KINDS: [&str; 6] = ["text", "box", "arrow", "badge", "blur", "card"];
pub const OVERLAY_ANCHORS: [&str; 2] = ["screen", "frame"];
pub const TEXT_BACKGROUNDS: [&str; 2] = ["pill", "none"];
pub const BLUR_STYLES: [&str; 2] = ["pixelate", "blur"];
/// The shortest an overlay may last, in its time base.
pub const MIN_OVERLAY_LENGTH: f64 = 0.1;

/// One annotation. Fields that a kind doesn't use keep their defaults and aren't written.
/// Sizes in pixels (`size`, `width`, `radius`) are at a 1080-pixel-high output and scale with
/// it; screen-anchored items scale with the zoom too.
#[derive(Clone, Debug, PartialEq)]
pub struct Overlay {
    pub id: String,
    /// One of [`OVERLAY_KINDS`].
    pub kind: String,
    /// One of [`OVERLAY_ANCHORS`].
    pub anchor: String,
    pub start: f64,
    pub end: f64,
    /// Where it sits (all kinds but `arrow` and `card`).
    pub rect: RectF,
    /// Seconds of fading in and out (not for `blur`, which never shows what it hides).
    pub fade_in: f64,
    pub fade_out: f64,
    /// `text`: the label; `card`: the title (written as `title`).
    pub text: String,
    /// `card`: the line under the title.
    pub subtitle: String,
    /// `text`: font size in pixels.
    pub size: f64,
    /// `text`, `box`, `arrow`, `badge`: `#rrggbb`, or empty for the Omarchy theme's colour.
    pub color: String,
    /// `text`: "pill" (on a rounded theme-coloured background) or "none".
    pub background: String,
    /// `box`, `arrow`: stroke width in pixels.
    pub width: f64,
    /// `box`: corner radius in pixels.
    pub radius: f64,
    /// `box`: how much everything outside it is dimmed, 0 (none) to 1.
    pub spotlight: f64,
    /// `arrow`: tail and head, 0–1 like `rect`.
    pub from: PointF,
    pub to: PointF,
    /// `badge`: the number shown.
    pub number: i32,
    /// `blur`: "pixelate" or "blur".
    pub style: String,
    /// `blur`: strength, 0–1; the block or blur size is an eighth to a quarter of the region's
    /// height, and 4–8 % of the recording's height at the least (`export::redact::block_fraction`).
    pub amount: f64,
}

impl Default for Overlay {
    fn default() -> Overlay {
        Overlay {
            id: String::new(),
            kind: "text".into(),
            anchor: "frame".into(),
            start: 0.0,
            end: 3.0,
            rect: RectF::new(0.35, 0.42, 0.3, 0.16),
            fade_in: 0.2,
            fade_out: 0.2,
            text: String::new(),
            subtitle: String::new(),
            size: 48.0,
            color: String::new(),
            background: "pill".into(),
            width: 6.0,
            radius: 16.0,
            spotlight: 0.0,
            from: PointF::new(0.3, 0.3),
            to: PointF::new(0.5, 0.5),
            number: 1,
            style: "pixelate".into(),
            amount: 0.5,
        }
    }
}

impl Overlay {
    /// A new item of `kind` with that kind's defaults: its anchor, look and placement. `None`
    /// for an unknown kind.
    pub fn new_of_kind(kind: &str) -> Option<Overlay> {
        if !OVERLAY_KINDS.contains(&kind) {
            return None;
        }
        let mut overlay = Overlay { kind: kind.to_owned(), ..Overlay::default() };
        match kind {
            "text" => {
                overlay.anchor = "frame".into();
                overlay.text = "Title".into();
                overlay.rect = RectF::new(0.2, 0.08, 0.6, 0.14);
            }
            "box" => {
                overlay.anchor = "screen".into();
                overlay.rect = RectF::new(0.35, 0.35, 0.3, 0.3);
            }
            "arrow" => {
                overlay.anchor = "screen".into();
                overlay.from = PointF::new(0.3, 0.3);
                overlay.to = PointF::new(0.45, 0.45);
            }
            "badge" => {
                overlay.anchor = "screen".into();
                overlay.rect = RectF::new(0.47, 0.45, 0.06, 0.1);
            }
            "blur" => {
                overlay.anchor = "screen".into();
                overlay.rect = RectF::new(0.35, 0.4, 0.3, 0.2);
                overlay.fade_in = 0.0;
                overlay.fade_out = 0.0;
            }
            "card" => {
                overlay.anchor = "frame".into();
                overlay.text = "Title".into();
                overlay.subtitle = String::new();
                overlay.rect = RectF::new(0.0, 0.0, 1.0, 1.0);
                overlay.fade_in = 0.4;
                overlay.fade_out = 0.4;
            }
            _ => {}
        }
        Some(overlay)
    }

    pub fn is_screen_anchored(&self) -> bool {
        self.anchor == "screen"
    }

    /// Whether `rect` places it (arrows use `from`/`to`, cards fill the canvas).
    pub fn uses_rect(&self) -> bool {
        self.kind != "arrow" && self.kind != "card"
    }

    /// Clamps every field into what the editor offers, and pins what a kind requires (blur
    /// on the screen without fades, cards on the frame over the whole canvas).
    pub fn sanitize(&mut self) {
        if !OVERLAY_ANCHORS.contains(&self.anchor.as_str()) {
            self.anchor = "screen".into();
        }
        match self.kind.as_str() {
            "blur" => {
                self.anchor = "screen".into();
                self.fade_in = 0.0;
                self.fade_out = 0.0;
            }
            "card" => {
                self.anchor = "frame".into();
                self.rect = RectF::new(0.0, 0.0, 1.0, 1.0);
            }
            _ => {}
        }
        let finite = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        self.start = cpp::max(0.0, finite(self.start, 0.0));
        self.end = cpp::max(self.start + MIN_OVERLAY_LENGTH, finite(self.end, self.start + 3.0));
        let r = self.rect;
        let w = cpp::clamp(finite(r.width, 0.3), 0.01, 1.0);
        let h = cpp::clamp(finite(r.height, 0.2), 0.01, 1.0);
        self.rect = RectF::new(cpp::clamp(finite(r.x, 0.0), 0.0, 1.0 - w), cpp::clamp(finite(r.y, 0.0), 0.0, 1.0 - h), w, h);
        let point = |p: PointF| PointF::new(cpp::clamp(finite(p.x, 0.5), 0.0, 1.0), cpp::clamp(finite(p.y, 0.5), 0.0, 1.0));
        self.from = point(self.from);
        self.to = point(self.to);
        self.fade_in = cpp::clamp(finite(self.fade_in, 0.0), 0.0, 5.0);
        self.fade_out = cpp::clamp(finite(self.fade_out, 0.0), 0.0, 5.0);
        self.size = cpp::clamp(finite(self.size, 48.0), 8.0, 400.0);
        self.width = cpp::clamp(finite(self.width, 6.0), 1.0, 40.0);
        self.radius = cpp::clamp(finite(self.radius, 16.0), 0.0, 400.0);
        self.spotlight = cpp::clamp(finite(self.spotlight, 0.0), 0.0, 1.0);
        self.amount = cpp::clamp(finite(self.amount, 0.5), 0.0, 1.0);
        self.number = self.number.clamp(0, 999);
        if !self.color.is_empty() {
            let parsed = QColor::parse(&self.color);
            self.color = if parsed.is_valid() { parsed.name() } else { String::new() };
        }
        if !TEXT_BACKGROUNDS.contains(&self.background.as_str()) {
            self.background = "pill".into();
        }
        if !BLUR_STYLES.contains(&self.style.as_str()) {
            self.style = "pixelate".into();
        }
    }

    /// Reads one item; `None` for an unknown kind. Missing fields take the kind's defaults.
    pub fn from_json(o: &Object) -> Option<Overlay> {
        let kind = o.value("kind").to_string_or("");
        let d = Overlay::new_of_kind(&kind)?;
        let rect = o.value("rect").to_object();
        let point = |key: &str, fallback: PointF| {
            let p = o.value(key).to_object();
            PointF::new(p.value("x").to_double(fallback.x), p.value("y").to_double(fallback.y))
        };
        let mut overlay = Overlay {
            id: o.value("id").to_string_or(""),
            anchor: o.value("anchor").to_string_or(&d.anchor),
            start: o.value("start").to_double(d.start),
            end: o.value("end").to_double(d.end),
            rect: RectF::new(
                rect.value("x").to_double(d.rect.x),
                rect.value("y").to_double(d.rect.y),
                rect.value("w").to_double(d.rect.width),
                rect.value("h").to_double(d.rect.height),
            ),
            fade_in: o.value("fadeIn").to_double(d.fade_in),
            fade_out: o.value("fadeOut").to_double(d.fade_out),
            text: o.value(if kind == "card" { "title" } else { "text" }).to_string_or(&d.text),
            subtitle: o.value("subtitle").to_string_or(&d.subtitle),
            size: o.value("size").to_double(d.size),
            color: o.value("color").to_string_or(""),
            background: o.value("background").to_string_or(&d.background),
            width: o.value("width").to_double(d.width),
            radius: o.value("radius").to_double(d.radius),
            spotlight: o.value("spotlight").to_double(d.spotlight),
            from: point("from", d.from),
            to: point("to", d.to),
            number: o.value("number").to_int(d.number),
            style: o.value("style").to_string_or(&d.style),
            amount: o.value("amount").to_double(d.amount),
            ..d
        };
        overlay.sanitize();
        Some(overlay)
    }

    /// The item as written: the common fields and its kind's own.
    pub fn to_json(&self) -> Object {
        let mut o = Object::from_iter([
            ("id", Value::from(self.id.as_str())),
            ("kind", self.kind.as_str().into()),
            ("anchor", self.anchor.as_str().into()),
            ("start", self.start.into()),
            ("end", self.end.into()),
        ]);
        if self.kind != "blur" {
            o.insert("fadeIn", self.fade_in);
            o.insert("fadeOut", self.fade_out);
        }
        if self.uses_rect() {
            o.insert(
                "rect",
                Object::from_iter([
                    ("x", Value::from(self.rect.x)),
                    ("y", self.rect.y.into()),
                    ("w", self.rect.width.into()),
                    ("h", self.rect.height.into()),
                ]),
            );
        }
        let point = |p: PointF| Object::from_iter([("x", Value::from(p.x)), ("y", p.y.into())]);
        let colored = matches!(self.kind.as_str(), "text" | "box" | "arrow" | "badge");
        if colored && !self.color.is_empty() {
            o.insert("color", self.color.as_str());
        }
        match self.kind.as_str() {
            "text" => {
                o.insert("text", self.text.as_str());
                o.insert("size", self.size);
                o.insert("background", self.background.as_str());
            }
            "box" => {
                o.insert("width", self.width);
                o.insert("radius", self.radius);
                o.insert("spotlight", self.spotlight);
            }
            "arrow" => {
                o.insert("from", point(self.from));
                o.insert("to", point(self.to));
                o.insert("width", self.width);
            }
            "badge" => o.insert("number", self.number),
            "blur" => {
                o.insert("style", self.style.as_str());
                o.insert("amount", self.amount);
            }
            "card" => {
                o.insert("title", self.text.as_str());
                o.insert("subtitle", self.subtitle.as_str());
            }
            _ => {}
        }
        o
    }

    /// The fields of `patch` (JSON as written, any subset; `id` and `kind` are ignored) laid
    /// over this item, then sanitised. A `rect` patch may give only some of x/y/w/h.
    pub fn patched(&self, patch: &Object) -> Overlay {
        let mut json = self.to_json();
        // Fields a kind doesn't write still read back from the patch.
        for (key, value) in patch.iter() {
            if key == "id" || key == "kind" {
                continue;
            }
            if let (Some(base), Some(part)) = (json.value(key).as_object(), value.as_object()) {
                let mut merged = base.clone();
                for (k, v) in part.iter() {
                    merged.insert(k, v.clone());
                }
                json.insert(key, merged);
            } else {
                json.insert(key, value.clone());
            }
        }
        // An anchor change keeps the times as numbers; the caller converts them if it wants.
        let mut result = Overlay::from_json(&json).unwrap_or_else(|| self.clone());
        result.id = self.id.clone();
        result
    }

    /// Visibility at `time` (in the item's own time base), 0–1 with the fades; 0 outside.
    pub fn opacity_at(&self, time: f64) -> f64 {
        if !(time >= self.start && time < self.end) {
            return 0.0;
        }
        if self.kind == "blur" {
            return 1.0;
        }
        let mut opacity = 1.0;
        if self.fade_in > 0.0 {
            opacity = cpp::min(opacity, (time - self.start) / self.fade_in);
        }
        if self.fade_out > 0.0 {
            opacity = cpp::min(opacity, (self.end - time) / self.fade_out);
        }
        cpp::clamp(opacity, 0.0, 1.0)
    }
}

/// The `overlays` list read from a project's JSON: unknown kinds dropped, in the order written.
pub fn overlays_from_json(value: &Value) -> Vec<Overlay> {
    value.to_array().iter().filter_map(|v| v.as_object().and_then(Overlay::from_json)).collect()
}

pub fn overlays_to_json(overlays: &[Overlay]) -> Value {
    Value::Array(overlays.iter().map(|o| Value::Object(o.to_json())).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qjson::{self, Format};
    use crate::project::Project;

    #[test]
    fn absent_stays_absent_and_items_round_trip() {
        let mut project = Project::defaults("Demo", 10.0);
        assert!(!project.to_json().contains("overlays"));
        let json = project.to_json().to_json(Format::Indented);
        assert_eq!(Project::from_json(&qjson::object_from_json(json.as_bytes()).unwrap()).to_json().to_json(Format::Indented), json);

        for (i, kind) in OVERLAY_KINDS.iter().enumerate() {
            let mut overlay = Overlay::new_of_kind(kind).unwrap();
            overlay.id = format!("o-{i}");
            overlay.color = if *kind == "box" { "#ff0000".into() } else { String::new() };
            project.overlays.push(overlay);
        }
        let written = project.to_json().to_json(Format::Indented);
        let read = Project::from_json(&qjson::object_from_json(written.as_bytes()).unwrap());
        assert_eq!(read.overlays, project.overlays);
        assert_eq!(read.to_json().to_json(Format::Indented), written);
        // Each kind writes its own fields only.
        let items = project.to_json().value("overlays").to_array();
        let keys = |i: usize| items[i].to_object().keys().map(str::to_owned).collect::<Vec<_>>().join(",");
        assert_eq!(keys(0), "anchor,background,end,fadeIn,fadeOut,id,kind,rect,size,start,text");
        assert_eq!(keys(1), "anchor,color,end,fadeIn,fadeOut,id,kind,radius,rect,spotlight,start,width");
        assert_eq!(keys(2), "anchor,end,fadeIn,fadeOut,from,id,kind,start,to,width");
        assert_eq!(keys(3), "anchor,end,fadeIn,fadeOut,id,kind,number,rect,start");
        assert_eq!(keys(4), "amount,anchor,end,id,kind,rect,start,style");
        assert_eq!(keys(5), "anchor,end,fadeIn,fadeOut,id,kind,start,subtitle,title");
    }

    #[test]
    fn reading_drops_unknown_kinds_and_clamps() {
        let json = br#"[
            {"id":"a","kind":"sticker","start":0,"end":1},
            {"id":"b","kind":"blur","anchor":"frame","start":2,"end":1,"rect":{"x":0.9,"y":-1,"w":0.5,"h":2},"amount":7,"fadeIn":1},
            {"id":"c","kind":"text","color":"not a colour","background":"wavy","size":1}
        ]"#;
        let overlays = overlays_from_json(&qjson::from_json(json).unwrap());
        assert_eq!(overlays.len(), 2);
        let blur = &overlays[0];
        assert_eq!((blur.anchor.as_str(), blur.start, blur.end, blur.amount, blur.fade_in), ("screen", 2.0, 2.1, 1.0, 0.0));
        assert_eq!(blur.rect, RectF::new(0.5, 0.0, 0.5, 1.0));
        let text = &overlays[1];
        assert_eq!((text.color.as_str(), text.background.as_str(), text.size), ("", "pill", 8.0));
    }

    #[test]
    fn patches_merge_rect_parts() {
        let overlay = Overlay::new_of_kind("box").unwrap();
        let patched = overlay.patched(&qjson::object_from_json(br#"{"rect":{"x":0.1},"spotlight":0.5,"kind":"text","id":"x"}"#).unwrap());
        assert_eq!(patched.kind, "box");
        assert_eq!(patched.id, overlay.id);
        assert_eq!(patched.rect, RectF::new(0.1, overlay.rect.y, overlay.rect.width, overlay.rect.height));
        assert_eq!(patched.spotlight, 0.5);
    }
}
