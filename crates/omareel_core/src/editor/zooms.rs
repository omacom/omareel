//! Zoom edits (`addZoomAt`, `moveZoom`, `resizeZoom`, `setZoomLevel`, `setZoomTarget`,
//! `setZoomTargetFromPreview`, `removeZoom`, `selectZoomsInOutputRange`, `moveSelectedZooms`,
//! `removeSelectedZooms`, `regenerateZooms`).

use super::{Document, Effect};
use crate::compat::cpp;
use crate::compat::geometry::PointF;
use crate::compat::qdir;
use crate::compat::qround::q_fuzzy_is_null;
use crate::project::ZoomSegment;
use crate::zoom_timeline;
use std::collections::HashSet;

/// What `setZoomTarget` makes of its `QVariant`: `Auto` when `target.toString()` is "auto" in
/// any case ([`ZoomTarget::is_auto`]), otherwise a point from `target.toMap()`, with
/// `value("x", 0.5).toDouble()` and the same for y (so a missing key is 0.5 and anything that
/// is not a number 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoomTarget {
    Auto,
    Point(f64, f64),
}

impl ZoomTarget {
    /// `text.compare("auto", Qt::CaseInsensitive) == 0`. Qt folds case per character, and no
    /// character but the ASCII letters folds to a, u, t or o.
    pub fn is_auto(text: &str) -> bool {
        text.eq_ignore_ascii_case("auto")
    }
}

impl Document {
    /// `addZoomAt(outputTime, length)` (QML's default length is 2): zooms never overlap. A new
    /// one starts where asked inside the open stretch around that point, ends early at the next
    /// zoom, and backs up when less than a second remains; it is selected. Inserted clips have
    /// no recording to zoom into. Returns the new id, or "".
    pub fn add_zoom_at(&mut self, output_time: f64, length: f64) -> String {
        let (clip, _) = self.clip_for_output(cpp::clamp(output_time, 0.0, self.duration()));
        if self.clip_is_media(clip) {
            return String::new();
        }
        let at = cpp::clamp(self.output_to_source(output_time), 0.0, self.recording.source_duration);
        let mut lower = 0.0;
        let mut upper = self.recording.source_duration;
        for zoom in &self.project.zooms {
            if zoom.start <= at && at < zoom.end {
                return String::new();
            }
            if zoom.end <= at {
                lower = cpp::max(lower, zoom.end);
            } else {
                upper = cpp::min(upper, zoom.start);
            }
        }
        let end = cpp::min(upper, at + cpp::max(1.0, length));
        let start = if end - at < 1.0 { cpp::max(lower, end - 1.0) } else { at };
        if end - start < 1.0 {
            return String::new();
        }
        self.snapshot("");
        let zoom = ZoomSegment {
            id: self.next_id("z"),
            start,
            end,
            level: 2.0,
            automatic_target: true,
            target: PointF::new(0.5, 0.5),
        };
        let id = zoom.id.clone();
        self.project.zooms.push(zoom);
        qdir::std_sort(&mut self.project.zooms, &|a: &ZoomSegment, b: &ZoomSegment| a.start < b.start);
        self.selected_zoom_id = id.clone();
        self.selected_zoom_ids = vec![id.clone()];
        self.changed(true);
        self.emit(Effect::SelectionChanged);
        id
    }

    /// `moveZoom(id, sourceStart)`: slides a zoom between its neighbours, keeping its length.
    /// Always an edit, even when it does not move.
    pub fn move_zoom(&mut self, id: &str, source_start: f64) -> bool {
        let index = self.zoom_index(id);
        if index < 0 {
            return false;
        }
        let i = index as usize;
        let zooms = &self.project.zooms;
        let length = zooms[i].end - zooms[i].start;
        let lower = if i > 0 { zooms[i - 1].end } else { 0.0 };
        let upper =
            if i + 1 < zooms.len() { zooms[i + 1].start - length } else { self.recording.source_duration - length };
        let source_start = cpp::clamp(source_start, lower, cpp::max(lower, upper));
        self.snapshot(&format!("move-{id}"));
        self.project.zooms[i].start = source_start;
        self.project.zooms[i].end = source_start + length;
        self.changed(true);
        true
    }

    /// `resizeZoom(id, sourceStart, sourceEnd)`: both ends between the neighbours; under a
    /// second is refused.
    pub fn resize_zoom(&mut self, id: &str, source_start: f64, source_end: f64) -> bool {
        let index = self.zoom_index(id);
        if index < 0 {
            return false;
        }
        let i = index as usize;
        let zooms = &self.project.zooms;
        let lower = if i > 0 { zooms[i - 1].end } else { 0.0 };
        let upper = if i + 1 < zooms.len() { zooms[i + 1].start } else { self.recording.source_duration };
        let source_start = cpp::clamp(source_start, lower, upper);
        let source_end = cpp::clamp(source_end, lower, upper);
        if source_end - source_start < 1.0 {
            return false;
        }
        self.snapshot(&format!("resize-{id}"));
        self.project.zooms[i].start = source_start;
        self.project.zooms[i].end = source_end;
        self.changed(true);
        true
    }

    /// `setZoomLevel(id, level, coalesce)`: 1 to 4.
    pub fn set_zoom_level(&mut self, id: &str, level: f64, coalesce: bool) -> bool {
        let index = self.zoom_index(id);
        if index < 0 {
            return false;
        }
        self.snapshot(&if coalesce { format!("level-{id}") } else { String::new() });
        self.project.zooms[index as usize].level = cpp::clamp(level, 1.0, 4.0);
        self.changed(true);
        true
    }

    /// `setZoomTarget(id, target)`: automatic, or a point clamped into the frame.
    pub fn set_zoom_target(&mut self, id: &str, target: ZoomTarget) -> bool {
        let index = self.zoom_index(id);
        if index < 0 {
            return false;
        }
        self.snapshot("");
        let zoom = &mut self.project.zooms[index as usize];
        match target {
            ZoomTarget::Auto => zoom.automatic_target = true,
            ZoomTarget::Point(x, y) => {
                zoom.automatic_target = false;
                zoom.target = PointF::new(cpp::clamp(x, 0.0, 1.0), cpp::clamp(y, 0.0, 1.0));
            }
        }
        self.changed(true);
        true
    }

    /// `setZoomTargetFromPreview(x, y)`: the selected zoom's target, then picking ends.
    pub fn set_zoom_target_from_preview(&mut self, x: f64, y: f64) -> bool {
        if self.selected_zoom_id.is_empty() {
            return false;
        }
        let id = self.selected_zoom_id.clone();
        let result = self.set_zoom_target(&id, ZoomTarget::Point(x, y));
        self.set_picking_zoom_target(false);
        result
    }

    /// `removeZoom(id)`: the zoom now in its place (or the last) becomes the selection.
    pub fn remove_zoom(&mut self, id: &str) -> bool {
        let index = self.zoom_index(id);
        if index < 0 {
            return false;
        }
        let i = index as usize;
        self.snapshot("");
        self.project.zooms.remove(i);
        self.selected_zoom_ids.retain(|selected| selected != id);
        self.selected_zoom_id = if self.project.zooms.is_empty() {
            String::new()
        } else {
            self.project.zooms[i.min(self.project.zooms.len() - 1)].id.clone()
        };
        if !self.selected_zoom_id.is_empty() {
            self.selected_zoom_ids = vec![self.selected_zoom_id.clone()];
        }
        self.changed(true);
        self.emit(Effect::SelectionChanged);
        true
    }

    /// `regenerateZooms()`: the automatic zooms around the recording's clicks again, the first
    /// selected.
    pub fn regenerate_zooms(&mut self) {
        self.snapshot("");
        let clicks: Vec<f64> = self.input.click_downs(true).iter().map(|click| click.time).collect();
        self.project.zooms = zoom_timeline::generate(&clicks, self.recording.source_duration);
        self.selected_zoom_id = self.project.zooms.first().map(|zoom| zoom.id.clone()).unwrap_or_default();
        self.selected_zoom_ids =
            if self.selected_zoom_id.is_empty() { Vec::new() } else { vec![self.selected_zoom_id.clone()] };
        self.changed(true);
        self.emit(Effect::SelectionChanged);
    }

    /// `selectZoomsInOutputRange(start, end)`: every zoom whose output span (both ends still
    /// shown) touches the range.
    pub fn select_zooms_in_output_range(&mut self, output_start: f64, output_end: f64) {
        let (output_start, output_end) =
            if output_start > output_end { (output_end, output_start) } else { (output_start, output_end) };
        let ids: Vec<String> = self
            .project
            .zooms
            .iter()
            .filter(|zoom| {
                let start = self.source_to_output(zoom.start, -1);
                let end = self.source_to_output(zoom.end, -1);
                start >= 0.0 && end >= 0.0 && end >= output_start && start <= output_end
            })
            .map(|zoom| zoom.id.clone())
            .collect();
        self.set_selected_zoom_ids(&ids);
    }

    /// `moveSelectedZooms(anchorId, sourceStart)`: moves the selection (made the anchor alone if
    /// it was not in it) so the anchor starts at `source_start`, as far as the recording and the
    /// unselected zooms allow. No movement is success without an undo step.
    pub fn move_selected_zooms(&mut self, anchor_id: &str, source_start: f64) -> bool {
        let anchor = self.zoom_index(anchor_id);
        if anchor < 0 {
            return false;
        }
        if !self.selected_zoom_ids.iter().any(|id| id == anchor_id) {
            self.set_selected_zoom_id(anchor_id);
        }
        let selected: HashSet<&str> = self.selected_zoom_ids.iter().map(String::as_str).collect();
        let duration = self.recording.source_duration;
        let requested = source_start - self.project.zooms[anchor as usize].start;
        let mut minimum = -duration;
        let mut maximum = duration;
        for zoom in &self.project.zooms {
            if !selected.contains(zoom.id.as_str()) {
                continue;
            }
            minimum = cpp::max(minimum, -zoom.start);
            maximum = cpp::min(maximum, duration - zoom.end);
            for other in &self.project.zooms {
                if selected.contains(other.id.as_str()) {
                    continue;
                }
                if other.end <= zoom.start {
                    minimum = cpp::max(minimum, other.end - zoom.start);
                } else if other.start >= zoom.end {
                    maximum = cpp::min(maximum, other.start - zoom.end);
                }
            }
        }
        let delta = cpp::clamp(requested, minimum, maximum);
        if q_fuzzy_is_null(delta) {
            return true;
        }
        let moving: HashSet<String> = selected.iter().map(|id| (*id).to_owned()).collect();
        let key = self.history.key_or("move-selection");
        self.snapshot(&key);
        for zoom in &mut self.project.zooms {
            if moving.contains(&zoom.id) {
                zoom.start += delta;
                zoom.end += delta;
            }
        }
        self.changed(true);
        true
    }

    /// `removeSelectedZooms()`: how many went. Any selection is an edit, even if none of its
    /// ids match a zoom any more.
    pub fn remove_selected_zooms(&mut self) -> i32 {
        if self.selected_zoom_ids.is_empty() {
            return 0;
        }
        let selected: HashSet<String> = self.selected_zoom_ids.iter().cloned().collect();
        let before = self.project.zooms.len();
        self.snapshot("");
        self.project.zooms.retain(|zoom| !selected.contains(&zoom.id));
        self.selected_zoom_id.clear();
        self.selected_zoom_ids.clear();
        self.changed(true);
        self.emit(Effect::SelectionChanged);
        (before - self.project.zooms.len()) as i32
    }
}
