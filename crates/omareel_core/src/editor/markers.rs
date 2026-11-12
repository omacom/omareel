//! Markers (`project.markers`) and what a recording's marks (`markers.jsonl`, see
//! [`crate::recording_marks`]) make of a new project: its first clips skip the retakes and
//! pauses, its markers are the ones dropped while recording, and its generated zooms only come
//! from clicks that are still shown.
//!
//! Each edit is one undo step. Times given to and returned by these methods are in seconds:
//! `source` is recording time (what `project.json` stores), `output` the edited timeline's.

use super::clips::visible_source_span;
use super::{Document, Effect};
use crate::compat::cpp;
use crate::project::{Clip, Marker, Project, ZoomSegment};
use crate::recording_marks::{kept_clips, MarkKind, RecordingMarks};
use crate::zoom_timeline;

/// Where a marker shows on the timeline: the marker, and its output time, `None` while every clip
/// that showed its moment is cut or trimmed away.
#[derive(Clone, Debug, PartialEq)]
pub struct MarkerRow {
    pub marker: Marker,
    pub output_time: Option<f64>,
}

/// Zooms generated from the clicks at `click_times` (recording seconds) that a recording clip
/// still shows, each shrunk to the part of it that shows. The new project and "regenerate zooms"
/// both use it, so a click inside a retake or a pause never zooms the kept recording beside it.
pub fn generate_visible_zooms(click_times: &[f64], clips: &[Clip], source_duration: f64) -> Vec<ZoomSegment> {
    let shown = |time: f64| clips.iter().any(|clip| !clip.is_media() && time >= clip.in_ && time < clip.out);
    let clicks: Vec<f64> = click_times.iter().copied().filter(|&time| shown(time)).collect();
    let mut zooms = zoom_timeline::generate(&clicks, source_duration);
    zooms.retain_mut(|zoom| {
        let (first, last) = visible_source_span(zoom, clips);
        if !first.is_finite() {
            return false;
        }
        zoom.start = first;
        zoom.end = last;
        true
    });
    for (index, zoom) in zooms.iter_mut().enumerate() {
        zoom.id = format!("z{}", index + 1);
    }
    zooms
}

/// Applies a recording's marks to its new project: clips around the cuts (see
/// [`RecordingMarks::cut_spans`]) and a marker for each marker and retake mark, `m1`, `m2`…, at
/// its recording time. `first_frame_us` is `capture.json`'s.
pub fn apply_recording_marks(project: &mut Project, marks: &RecordingMarks, first_frame_us: i64, source_duration: f64) {
    if marks.is_empty() || source_duration <= 0.0 {
        return;
    }
    let cuts = marks.cut_spans(first_frame_us, source_duration);
    if !cuts.is_empty() {
        let media: Vec<Clip> = project.clips.iter().filter(|clip| clip.is_media()).cloned().collect();
        project.clips = kept_clips(source_duration, &cuts);
        project.clips.extend(media);
    }
    project.markers = marks
        .markers()
        .enumerate()
        .map(|(index, mark)| Marker {
            id: format!("m{}", index + 1),
            time: cpp::clamp((mark.monotonic_us - first_frame_us) as f64 / 1_000_000.0, 0.0, source_duration),
            label: mark.label.clone(),
            kind: if mark.kind == MarkKind::Retake { "retake".into() } else { "marker".into() },
        })
        .collect();
}

impl Document {
    pub fn markers(&self) -> &[Marker] {
        &self.project.markers
    }

    pub fn marker_index(&self, id: &str) -> i32 {
        self.project.markers.iter().position(|marker| marker.id == id).map_or(-1, |i| i as i32)
    }

    /// Where a marker's recording time plays, or `None` when no recording clip shows it.
    pub fn marker_output_time(&self, marker: &Marker) -> Option<f64> {
        let output = self.source_to_output(marker.time, -1);
        (output >= 0.0).then_some(output)
    }

    /// Every marker in recording order with its output time.
    pub fn marker_rows(&self) -> Vec<MarkerRow> {
        self.project
            .markers
            .iter()
            .map(|marker| MarkerRow { marker: marker.clone(), output_time: self.marker_output_time(marker) })
            .collect()
    }

    /// The output time of the nearest shown marker after `output_time` (`direction` > 0) or
    /// before it (`direction` < 0), more than 10 ms away so repeated jumps move on.
    pub fn adjacent_marker_output_time(&self, output_time: f64, direction: i32) -> Option<f64> {
        let times = self.marker_rows().into_iter().filter_map(|row| row.output_time);
        if direction >= 0 {
            times.filter(|&time| time > output_time + 0.01).min_by(f64::total_cmp)
        } else {
            times.filter(|&time| time < output_time - 0.01).max_by(f64::total_cmp)
        }
    }

    /// `M` and `Shift+M`: seeks to [`Document::adjacent_marker_output_time`]. False when there is
    /// no marker that way.
    pub fn jump_to_marker(&mut self, direction: i32) -> bool {
        let Some(time) = self.adjacent_marker_output_time(self.output_position(), direction) else { return false };
        self.emit(Effect::Seek(time));
        true
    }

    /// Adds a marker at recording time `source_time` (clamped to the recording) with `label`.
    /// Returns its id, or "" for a time that is not a number.
    pub fn add_marker(&mut self, source_time: f64, label: &str) -> String {
        if !source_time.is_finite() {
            return String::new();
        }
        let time = cpp::clamp(source_time, 0.0, self.recording.source_duration);
        self.snapshot("");
        let id = self.next_id("m");
        let marker = Marker { id: id.clone(), time, label: label.trim().to_owned(), kind: "marker".into() };
        let at = self.project.markers.partition_point(|existing| existing.time <= time);
        self.project.markers.insert(at, marker);
        self.changed(false);
        id
    }

    /// [`Document::add_marker`] at the recording time playing at `output_time`; "" during an
    /// inserted video, which has no recording time of its own.
    pub fn add_marker_at_output(&mut self, output_time: f64, label: &str) -> String {
        if !output_time.is_finite() {
            return String::new();
        }
        let (clip, _) = self.clip_for_output(cpp::clamp(output_time, 0.0, self.duration()));
        if self.clip_is_media(clip) {
            return String::new();
        }
        let source_time = self.output_to_source(output_time);
        self.add_marker(source_time, label)
    }

    /// Renames a marker; an empty label leaves it numbered. False for an unknown id or the same
    /// label.
    pub fn rename_marker(&mut self, id: &str, label: &str) -> bool {
        let index = self.marker_index(id);
        let label = label.trim();
        if index < 0 || self.project.markers[index as usize].label == label {
            return false;
        }
        self.snapshot("");
        self.project.markers[index as usize].label = label.to_owned();
        self.changed(false);
        true
    }

    /// Removes a marker. False for an unknown id.
    pub fn remove_marker(&mut self, id: &str) -> bool {
        let index = self.marker_index(id);
        if index < 0 {
            return false;
        }
        self.snapshot("");
        self.project.markers.remove(index as usize);
        self.changed(false);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording_marks::Mark;

    #[test]
    fn marks_make_the_new_project() {
        let first = 2_000_000;
        let mut retake = Mark::new(MarkKind::Retake, first + 8_000_000);
        retake.from_us = first + 6_000_000;
        let mut marker = Mark::new(MarkKind::Marker, first + 3_000_000);
        marker.label = "intro".into();
        let marks = RecordingMarks {
            marks: vec![
                marker,
                retake,
                Mark::new(MarkKind::Pause, first + 12_000_000),
                Mark::new(MarkKind::Resume, first + 14_000_000),
            ],
        };
        let mut project = Project::defaults("x", 20.0);
        apply_recording_marks(&mut project, &marks, first, 20.0);
        let spans: Vec<(f64, f64)> = project.clips.iter().map(|clip| (clip.in_, clip.out)).collect();
        assert_eq!(spans, [(0.0, 6.0), (8.0, 12.0), (14.0, 20.0)]);
        let markers: Vec<(&str, f64, &str, &str)> =
            project.markers.iter().map(|m| (m.id.as_str(), m.time, m.label.as_str(), m.kind.as_str())).collect();
        assert_eq!(markers, [("m1", 3.0, "intro", "marker"), ("m2", 8.0, "", "retake")]);

        // A click in the retake would zoom the start of the kept take; one straddling a cut
        // shrinks to what still shows.
        let zooms = generate_visible_zooms(&[7.0, 11.0], &project.clips, 20.0);
        let zoom_spans: Vec<(&str, f64, f64)> = zooms.iter().map(|z| (z.id.as_str(), z.start, z.end)).collect();
        assert_eq!(zoom_spans, [("z1", 11.0 - 0.3, 12.0)]);

        let mut untouched = Project::defaults("x", 20.0);
        apply_recording_marks(&mut untouched, &RecordingMarks::default(), first, 20.0);
        assert_eq!(untouched, Project::defaults("x", 20.0));
    }
}
