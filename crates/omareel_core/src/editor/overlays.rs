//! Overlay edits: annotations over the recording (`project.overlays`, see
//! [`crate::project::Overlay`] for the model and its two anchors). Every edit is one undo step;
//! a patch with a coalescing key (a drag, a slider) makes one step for the whole gesture.
//!
//! Times passed in and out are in the item's own time base: SOURCE seconds for
//! screen-anchored items, OUTPUT seconds for frame-anchored ones.

use super::clips::visible_span;
use super::{Document, Effect};
use crate::compat::cpp;
use crate::compat::geometry::{PointF, RectF};
use crate::compat::qjson::{Object, Value};
use crate::project::{Clip, Overlay, MIN_OVERLAY_LENGTH};

/// How long a new overlay lasts when nothing says.
pub const DEFAULT_OVERLAY_LENGTH: f64 = 3.0;

/// How close to the end of the video a frame-anchored overlay's end must be to stay at the end
/// when the video's length changes ([`Document::fit_frame_overlays`]).
const END_TOLERANCE: f64 = 1e-3;

impl Document {
    pub fn overlays(&self) -> &[Overlay] {
        &self.project.overlays
    }

    pub fn overlay(&self, id: &str) -> Option<&Overlay> {
        self.project.overlays.iter().find(|overlay| overlay.id == id)
    }

    pub fn overlay_index(&self, id: &str) -> i32 {
        self.project.overlays.iter().position(|overlay| overlay.id == id).map_or(-1, |i| i as i32)
    }

    pub fn selected_overlay_id(&self) -> &str {
        &self.selected_overlay_id
    }

    /// Selects one overlay (or none), and no clip or zoom.
    pub fn set_selected_overlay_id(&mut self, id: &str) {
        let id = if self.overlay_index(id) >= 0 { id } else { "" };
        if id == self.selected_overlay_id && (id.is_empty() || (self.selected_clip_id.is_empty() && self.selected_zoom_id.is_empty())) {
            return;
        }
        self.selected_overlay_id = id.to_owned();
        if !id.is_empty() {
            self.set_picking_zoom_target(false);
            self.selected_clip_id.clear();
            self.selected_zoom_id.clear();
            self.selected_zoom_ids.clear();
        }
        self.emit(Effect::SelectionChanged);
        // The preview draws the selected overlay at full opacity.
        self.emit(Effect::CompositionChanged);
    }

    /// Drops the overlay selection; the preview's full-opacity view of it goes too.
    pub(crate) fn clear_overlay_selection(&mut self) {
        if !self.selected_overlay_id.is_empty() {
            self.selected_overlay_id.clear();
            self.emit(Effect::CompositionChanged);
        }
    }

    /// Adds an overlay of `kind` ("text", "box", "arrow", "badge", "blur", "card") from `start`
    /// to `end` in its time base, at `rect` (the kind's default place when `None`), with
    /// `props` (any fields as project.json writes them, `anchor` among them) over the kind's
    /// defaults. The new overlay is selected. Returns its id, or "" for an unknown kind.
    pub fn add_overlay(&mut self, kind: &str, start: f64, end: f64, rect: Option<RectF>, props: &Object) -> String {
        let Some(mut overlay) = Overlay::new_of_kind(kind) else { return String::new() };
        if let Some(rect) = rect {
            overlay.rect = rect;
        }
        overlay.start = start;
        overlay.end = end;
        if kind == "badge" && !props.contains("number") {
            overlay.number = 1 + self.project.overlays.iter().filter(|o| o.kind == "badge").map(|o| o.number).max().unwrap_or(0);
        }
        let mut overlay = overlay.patched(props);
        overlay.id = self.next_id("o");
        let id = overlay.id.clone();
        self.snapshot("");
        self.project.overlays.push(overlay);
        self.selected_overlay_id = id.clone();
        self.selected_clip_id.clear();
        self.selected_zoom_id.clear();
        self.selected_zoom_ids.clear();
        self.changed(false);
        self.emit(Effect::SelectionChanged);
        id
    }

    /// The editor's "add" (toolbar and keys): an overlay of `kind` with its defaults starting
    /// at the output position `output_time`, for `length` seconds of output. Screen-anchored
    /// kinds need the recording there (not an inserted video); they get the source span that
    /// plays then. Returns the new id, or "".
    pub fn add_overlay_at(&mut self, kind: &str, output_time: f64, length: f64) -> String {
        let Some(defaults) = Overlay::new_of_kind(kind) else { return String::new() };
        let duration = self.duration();
        let length = if length > 0.0 { length } else { DEFAULT_OVERLAY_LENGTH };
        let output_start = cpp::clamp(output_time, 0.0, cpp::max(0.0, duration - MIN_OVERLAY_LENGTH));
        let output_end = cpp::min(duration, output_start + length);
        if !defaults.is_screen_anchored() {
            return self.add_overlay(kind, output_start, cpp::max(output_end, output_start + MIN_OVERLAY_LENGTH), None, &Object::new());
        }
        let (clip, clip_start) = self.clip_for_output(output_start);
        if self.clip_is_media(clip) {
            return String::new();
        }
        let Some(c) = self.clip(clip).cloned() else { return String::new() };
        let source_start = self.output_to_source(output_start);
        // Within the clip: a screen item beyond it would cover footage cut away or elsewhere.
        let clip_end_output = clip_start + (c.out - c.in_) / c.speed;
        let source_end = cpp::min(c.out, source_start + (cpp::min(output_end, clip_end_output) - output_start) * c.speed);
        let source_end = cpp::max(source_end, source_start + MIN_OVERLAY_LENGTH);
        self.add_overlay(kind, source_start, source_end, None, &Object::new())
    }

    /// Lays `patch` (fields as project.json writes them; `rect` may give only some of x/y/w/h)
    /// over the overlay. A change of `anchor` without new times or place converts its times
    /// between source and output and its place between the recording and the canvas. With
    /// `coalesce`, the patch joins the open coalesced edit (or the overlay's own key), so a drag
    /// is one undo step. False when the id is unknown or nothing changes.
    pub fn update_overlay(&mut self, id: &str, patch: &Object, coalesce: bool) -> bool {
        let index = self.overlay_index(id);
        if index < 0 {
            return false;
        }
        let current = self.project.overlays[index as usize].clone();
        let mut patch = patch.clone();
        let anchor = patch.value("anchor").to_string_or(&current.anchor);
        if anchor != current.anchor && current.kind != "blur" && current.kind != "card" {
            self.convert_anchor(&current, &anchor, &mut patch);
        }
        let next = current.patched(&patch);
        if next == current {
            return false;
        }
        let key = if coalesce { self.history.key_or(&format!("overlay-{id}")) } else { String::new() };
        self.snapshot(&key);
        self.project.overlays[index as usize] = next;
        self.changed(false);
        true
    }

    /// Removes an overlay; the selection moves off it.
    pub fn remove_overlay(&mut self, id: &str) -> bool {
        let index = self.overlay_index(id);
        if index < 0 {
            return false;
        }
        self.snapshot("");
        self.project.overlays.remove(index as usize);
        if self.selected_overlay_id == id {
            self.selected_overlay_id.clear();
            self.emit(Effect::SelectionChanged);
        }
        self.changed(false);
        true
    }

    /// Where an overlay plays on the output timeline: its own times for a frame-anchored item,
    /// the output span of what still shows of its source span for a screen-anchored one
    /// ((-1, -1) when it was all cut).
    pub fn overlay_output_range(&self, overlay: &Overlay) -> (f64, f64) {
        if overlay.is_screen_anchored() {
            self.source_range_to_output(overlay.start, overlay.end)
        } else {
            (overlay.start, overlay.end)
        }
    }

    /// The recording's visible frame (after the crop) on the canvas, as 0–1 of the canvas,
    /// without a zoom: where Composition.qml draws it.
    pub fn screen_frame_on_canvas(&self) -> RectF {
        let width = f64::from(cpp::max(1, self.output_width()));
        let height = f64::from(cpp::max(1, self.output_height()));
        let p = &self.project;
        let no_background = p.background.type_ == "none";
        if no_background {
            return RectF::new(0.0, 0.0, 1.0, 1.0);
        }
        let pad = width.min(height) * p.frame.padding;
        let border = if p.frame.border.enabled { p.frame.border.width * height / 1080.0 } else { 0.0 };
        let aspect = (f64::from(self.source_width()) * p.crop.width) / cpp::max(1.0, f64::from(self.source_height()) * p.crop.height);
        let video_width = cpp::max(1.0, cpp::min(width - 2.0 * pad - 2.0 * border, (height - 2.0 * pad - 2.0 * border) * aspect));
        let video_height = video_width / cpp::max(0.0001, aspect);
        RectF::new(
            (width - video_width) / 2.0 / width,
            (height - video_height) / 2.0 / height,
            video_width / width,
            video_height / height,
        )
    }

    /// Fills `patch` with the times and place `current` keeps when it moves to `anchor`.
    fn convert_anchor(&self, current: &Overlay, anchor: &str, patch: &mut Object) {
        let to_frame = anchor == "frame";
        if !patch.contains("start") && !patch.contains("end") {
            let (start, end) = if to_frame {
                let (start, end) = self.source_range_to_output(current.start, current.end);
                if start < 0.0 { (self.output_position(), self.output_position() + DEFAULT_OVERLAY_LENGTH) } else { (start, end) }
            } else {
                let start = self.output_to_source(current.start);
                (start, start + (current.end - current.start))
            };
            patch.insert("start", start);
            patch.insert("end", end);
        }
        let frame = self.screen_frame_on_canvas();
        let crop = self.project.crop;
        let point = |p: PointF| {
            if to_frame {
                PointF::new(
                    frame.x + (p.x - crop.x) / cpp::max(0.0001, crop.width) * frame.width,
                    frame.y + (p.y - crop.y) / cpp::max(0.0001, crop.height) * frame.height,
                )
            } else {
                PointF::new(
                    crop.x + (p.x - frame.x) / cpp::max(0.0001, frame.width) * crop.width,
                    crop.y + (p.y - frame.y) / cpp::max(0.0001, frame.height) * crop.height,
                )
            }
        };
        let json_point = |p: PointF| Value::Object(Object::from_iter([("x", Value::from(p.x)), ("y", p.y.into())]));
        if !patch.contains("rect") && current.uses_rect() {
            let r = current.rect;
            let a = point(PointF::new(r.x, r.y));
            let b = point(PointF::new(r.x + r.width, r.y + r.height));
            patch.insert(
                "rect",
                Object::from_iter([("x", Value::from(a.x)), ("y", a.y.into()), ("w", (b.x - a.x).into()), ("h", (b.y - a.y).into())]),
            );
        }
        if current.kind == "arrow" {
            if !patch.contains("from") {
                patch.insert("from", json_point(point(current.from)));
            }
            if !patch.contains("to") {
                patch.insert("to", json_point(point(current.to)));
            }
        }
    }

    /// An output span `from`..`to` was cut: frame-anchored overlays after it move back, those
    /// across it shrink, and those inside it go. (They are then where they belong, so the
    /// edit's [`Document::fit_frame_overlays`] leaves them be.)
    pub(crate) fn cut_frame_overlays(&mut self, from: f64, to: f64) {
        self.duration_before_edit = None;
        let removed = to - from;
        if removed <= 0.0 {
            return;
        }
        let map = |t: f64| if t <= from { t } else if t < to { from } else { t - removed };
        let selected = self.selected_overlay_id.clone();
        self.project.overlays.retain_mut(|overlay| {
            if overlay.is_screen_anchored() {
                return true;
            }
            let (start, end) = (map(overlay.start), map(overlay.end));
            if end - start < MIN_OVERLAY_LENGTH {
                return false;
            }
            overlay.start = start;
            overlay.end = end;
            true
        });
        if !selected.is_empty() && self.overlay_index(&selected) < 0 {
            self.selected_overlay_id.clear();
        }
    }

    /// The clips became `clips`: screen-anchored overlays shrink to the part of their span that
    /// still shows, and those that no longer show at all go, as zooms do. Except blur regions:
    /// they keep their whole span always, so footage that comes back (a trim undone by hand,
    /// a reset or a merge) comes back hidden. One whose footage is all cut stays in the list,
    /// unseen.
    pub(crate) fn cut_screen_overlays(&mut self, clips: &[Clip]) {
        let before_clips = self.project.clips.clone();
        let selected = self.selected_overlay_id.clone();
        self.project.overlays.retain_mut(|overlay| {
            if !overlay.is_screen_anchored() || overlay.kind == "blur" {
                return true;
            }
            let before = visible_span(overlay.start, overlay.end, &before_clips);
            let after = visible_span(overlay.start, overlay.end, clips);
            if before.0.is_finite() && !after.0.is_finite() {
                return false;
            }
            if before.0.is_finite() && before != after {
                overlay.start = after.0;
                overlay.end = cpp::max(after.1, after.0 + MIN_OVERLAY_LENGTH);
            }
            true
        });
        if !selected.is_empty() && self.overlay_index(&selected) < 0 {
            self.selected_overlay_id.clear();
        }
    }

    /// After an edit that changed the video's length from `before` (any edit to the clips:
    /// trims, speed, splits and merges, inserted videos), frame-anchored overlays keep their
    /// output times, except that one that ended at the end of the video still does (it moves
    /// with the end, keeping its length: an end card), and one that now runs past the end
    /// moves back to end there (shortened only if it is longer than the video). The cuts
    /// ([`Document::cut_frame_overlays`]) place them themselves.
    pub(crate) fn fit_frame_overlays(&mut self, before: f64) {
        let after = self.duration();
        if !(after > 0.0) || !before.is_finite() {
            return;
        }
        let moved = after - before;
        for overlay in self.project.overlays.iter_mut().filter(|overlay| !overlay.is_screen_anchored()) {
            let length = overlay.end - overlay.start;
            if overlay.end >= before - END_TOLERANCE && moved.abs() > 1e-9 {
                overlay.end = after;
                overlay.start = cpp::max(0.0, after - length);
            }
            if overlay.end > after {
                overlay.end = after;
                overlay.start = cpp::max(0.0, after - length);
            }
            if overlay.end - overlay.start < MIN_OVERLAY_LENGTH {
                overlay.start = cpp::max(0.0, overlay.end - MIN_OVERLAY_LENGTH);
                overlay.end = overlay.start + MIN_OVERLAY_LENGTH;
            }
        }
    }
}
