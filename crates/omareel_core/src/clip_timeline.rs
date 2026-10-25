//! The clip list as one output timeline: mapping between output and source time, and the cut
//! operations (`ClipTimeline`).
//!
//! Indices are `i32` like the C++ `int`s, so callers can pass -1 and the out-of-range checks
//! stay here.

use crate::compat::cpp;
use crate::compat::qstring;
use crate::project::Clip;

#[derive(Clone, Debug, PartialEq)]
pub struct ClipTimeline {
    clips: Vec<Clip>,
    next_id: i32,
}

impl Default for ClipTimeline {
    fn default() -> ClipTimeline {
        ClipTimeline::new(Vec::new())
    }
}

fn duration(clip: &Clip) -> f64 {
    (clip.out - clip.in_) / cpp::max(0.000001, clip.speed)
}

impl ClipTimeline {
    pub fn new(clips: Vec<Clip>) -> ClipTimeline {
        ClipTimeline { clips, next_id: 1 }
    }

    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// `index` as a position in `clips`, when it is one.
    fn position(&self, index: i32) -> Option<usize> {
        usize::try_from(index).ok().filter(|&i| i < self.clips.len())
    }

    pub fn total_duration(&self) -> f64 {
        let mut total = 0.0;
        for clip in &self.clips {
            total += cpp::max(0.0, duration(clip));
        }
        total
    }

    /// Index of the clip playing at an output time (the last clip past the end), -1 when empty.
    /// `clip_output_start` receives that clip's output start, and is left alone when empty.
    pub fn clip_at(&self, output_time: f64, clip_output_start: Option<&mut f64>) -> i32 {
        let mut start = 0.0;
        for (i, clip) in self.clips.iter().enumerate() {
            let length = duration(clip);
            if output_time < start + length || i == self.clips.len() - 1 {
                if let Some(clip_output_start) = clip_output_start {
                    *clip_output_start = start;
                }
                return i as i32;
            }
            start += length;
        }
        -1
    }

    /// Recording time at an output time; inside an inserted video, the recording time it
    /// interrupts (the end of the recording clip before it, or the start of the one after).
    pub fn recording_source_time(&self, output_time: f64) -> f64 {
        let Some(index) = self.position(self.clip_at(output_time, None)) else { return -1.0 };
        if !self.clips[index].is_media() {
            return self.source_time(output_time);
        }
        if let Some(clip) = self.clips[..index].iter().rev().find(|clip| !clip.is_media()) {
            return clip.out;
        }
        if let Some(clip) = self.clips[index + 1..].iter().find(|clip| !clip.is_media()) {
            return clip.in_;
        }
        0.0
    }

    /// Recording clips stay in recording order: a clip's in/out may move between the end of the
    /// nearest recording clip before it and the start of the nearest after it.
    ///
    /// The C++ reads past the clip list for an `index` below -1 or above the clip count
    /// (undefined behaviour); here those positions are skipped.
    pub fn recording_bounds(&self, index: i32, source_duration: f64) -> (f64, f64) {
        let mut lower = 0.0;
        let mut upper = source_duration;
        let before = usize::try_from(index).unwrap_or(0).min(self.clips.len());
        if let Some(clip) = self.clips[..before].iter().rev().find(|clip| !clip.is_media()) {
            lower = clip.out;
        }
        let after = usize::try_from(i64::from(index) + 1).unwrap_or(0).min(self.clips.len());
        if let Some(clip) = self.clips[after..].iter().find(|clip| !clip.is_media()) {
            upper = clip.in_;
        }
        (lower, upper)
    }

    pub fn source_time(&self, output_time: f64) -> f64 {
        let Some(last) = self.clips.last() else { return -1.0 };
        let mut cursor = 0.0;
        for (i, clip) in self.clips.iter().enumerate() {
            let length = duration(clip);
            if output_time < cursor + length || i == self.clips.len() - 1 {
                return cpp::clamp(clip.in_ + (output_time - cursor) * clip.speed, clip.in_, clip.out);
            }
            cursor += length;
        }
        last.out
    }

    pub fn output_time(&self, clip_index: i32, source_time: f64) -> f64 {
        let Some(index) = self.position(clip_index) else { return -1.0 };
        let mut output = 0.0;
        for clip in &self.clips[..index] {
            output += duration(clip);
        }
        let clip = &self.clips[index];
        output + (cpp::clamp(source_time, clip.in_, clip.out) - clip.in_) / clip.speed
    }

    /// Where a recording span plays once cuts are applied, from its first frame still in a clip
    /// to its last; (-1, -1) when all of it was cut. Inserted videos never match.
    pub fn output_range(&self, source_start: f64, source_end: f64) -> (f64, f64) {
        let mut range = (-1.0, -1.0);
        for (i, clip) in self.clips.iter().enumerate() {
            if clip.is_media() {
                continue;
            }
            let from = cpp::max(source_start, clip.in_);
            let to = cpp::min(source_end, clip.out);
            if to <= from {
                continue;
            }
            let output_from = self.output_time(i as i32, from);
            let output_to = self.output_time(i as i32, to);
            range.0 = if range.0 < 0.0 { output_from } else { cpp::min(range.0, output_from) };
            range.1 = cpp::max(range.1, output_to);
        }
        range
    }

    /// Splits the clip playing at `output_time` in two, unless that is within 1e-9 s of its
    /// edges. The right half's id is `"<id>-s<n>"` made with `QString::arg` as the C++ does, so
    /// a `%` placeholder in the id is substituted the way Qt substitutes it.
    pub fn split(&mut self, output_time: f64) -> bool {
        let mut cursor = 0.0;
        for i in 0..self.clips.len() {
            let length = duration(&self.clips[i]);
            if output_time > cursor + 1e-9 && output_time < cursor + length - 1e-9 {
                let mut left = self.clips[i].clone();
                let mut right = self.clips[i].clone();
                let source = left.in_ + (output_time - cursor) * left.speed;
                left.out = source;
                right.in_ = source;
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1);
                right.id = qstring::arg(&qstring::arg("%1-s%2", &right.id), &id.to_string());
                self.clips[i] = left;
                self.clips.insert(i + 1, right);
                return true;
            }
            cursor += length;
        }
        false
    }

    pub fn trim(&mut self, index: i32, new_in: f64, new_out: f64) -> bool {
        let Some(index) = self.position(index) else { return false };
        if new_in < 0.0 || new_out <= new_in {
            return false;
        }
        self.clips[index].in_ = new_in;
        self.clips[index].out = new_out;
        true
    }

    pub fn remove(&mut self, index: i32) -> bool {
        let Some(index) = self.position(index) else { return false };
        self.clips.remove(index);
        true
    }

    /// Cuts output time `from`..`to` (clamped to the timeline) out of the clips. A clip cut in
    /// the middle keeps its id on the left part and takes `remainder_id` on the right. Returns
    /// false and changes nothing when `from` or `to` is not finite, the range is under 1e-9 s, a
    /// part left over would be under 0.1 s of source, `remainder_id` is needed but empty or
    /// already taken, or nothing would be left.
    pub fn delete_range(&mut self, from: f64, to: f64, remainder_id: &str) -> bool {
        if !from.is_finite() || !to.is_finite() {
            return false;
        }
        let total = self.total_duration();
        let from = cpp::clamp(from, 0.0, total);
        let to = cpp::clamp(to, 0.0, total);
        if to - from < 1e-9 {
            return false;
        }

        let mut kept = Vec::new();
        let mut cursor = 0.0;
        for clip in &self.clips {
            let length = duration(clip);
            let end = cursor + length;
            if end <= from || cursor >= to {
                kept.push(clip.clone());
            } else {
                let left_length = cpp::clamp(from - cursor, 0.0, length);
                let right_length = cpp::clamp(end - to, 0.0, length);
                if left_length > 1e-9 {
                    let mut left = clip.clone();
                    left.out = clip.in_ + left_length * clip.speed;
                    if left.out - left.in_ < 0.1 {
                        return false;
                    }
                    kept.push(left);
                }
                if right_length > 1e-9 {
                    let mut right = clip.clone();
                    right.in_ = clip.out - right_length * clip.speed;
                    if right.out - right.in_ < 0.1 {
                        return false;
                    }
                    if left_length > 1e-9 {
                        if remainder_id.is_empty() || self.clips.iter().any(|existing| existing.id == remainder_id) {
                            return false;
                        }
                        right.id = remainder_id.to_owned();
                    }
                    kept.push(right);
                }
            }
            cursor = end;
        }
        if kept.is_empty() {
            return false;
        }
        self.clips = kept;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};

    /// `QCOMPARE` on doubles: fuzzy, and an expected (fuzzy) zero needs an actual one.
    fn qcompare(actual: f64, expected: f64) -> bool {
        if expected.is_nan() {
            actual.is_nan()
        } else if expected.is_infinite() {
            actual == expected
        } else if q_fuzzy_is_null(expected) {
            q_fuzzy_is_null(actual)
        } else {
            q_fuzzy_compare(actual, expected)
        }
    }

    fn clip(id: &str, in_: f64, out: f64, speed: f64) -> Clip {
        Clip { id: id.into(), in_, out, speed, ..Clip::default() }
    }

    // tests/test_cliptimeline.cpp
    #[test]
    fn frame_source_mapping_with_speed_changes() {
        let timeline = ClipTimeline::new(vec![clip("slow", 1.0, 3.0, 0.5), clip("fast", 10.0, 14.0, 2.0)]);
        let fps = 60.0;
        assert!(qcompare(timeline.source_time(0.0 / fps), 1.0));
        assert!(qcompare(timeline.source_time(120.0 / fps), 2.0));
        assert!(qcompare(timeline.source_time(240.0 / fps), 10.0));
        assert!(qcompare(timeline.source_time(270.0 / fps), 11.0));
        assert!(qcompare(timeline.source_time(359.0 / fps), 13.966666666666667));
    }

    #[test]
    fn mapping_and_operations() {
        let mut timeline = ClipTimeline::new(vec![clip("a", 2.0, 6.0, 1.0), clip("b", 10.0, 14.0, 2.0)]);
        assert!(qcompare(timeline.total_duration(), 6.0));
        assert!(qcompare(timeline.source_time(1.5), 3.5));
        assert!(qcompare(timeline.source_time(5.0), 12.0));
        assert!(qcompare(timeline.output_time(1, 12.0), 5.0));
        assert!(timeline.split(1.0));
        assert_eq!(timeline.clips().len(), 3);
        assert!(qcompare(timeline.clips()[0].out, 3.0));
        assert!(qcompare(timeline.clips()[1].in_, 3.0));
        assert!(timeline.trim(1, 3.5, 5.5));
        assert!(qcompare(timeline.total_duration(), 5.0));
        assert!(timeline.remove(0));
        assert!(qcompare(timeline.source_time(0.0), 3.5));
    }

    #[test]
    fn inserted_videos_sit_between_recording_clips() {
        let mut insert = clip("m", 2.0, 4.5, 1.0);
        insert.media = "media/demo.mp4".into();
        // Recording 0-3, a 2.5 s insert from 2 s into the video, recording 5-7 at 2x.
        let timeline = ClipTimeline::new(vec![clip("a", 0.0, 3.0, 1.0), insert.clone(), clip("b", 5.0, 7.0, 2.0)]);
        assert!(qcompare(timeline.total_duration(), 6.5));
        let mut start = -1.0;
        assert_eq!(timeline.clip_at(1.0, Some(&mut start)), 0);
        assert!(qcompare(start, 0.0));
        assert_eq!(timeline.clip_at(3.0, Some(&mut start)), 1);
        assert!(qcompare(start, 3.0));
        assert_eq!(timeline.clip_at(5.6, Some(&mut start)), 2);
        assert!(qcompare(start, 5.5));
        assert_eq!(timeline.clip_at(99.0, None), 2);
        assert_eq!(ClipTimeline::default().clip_at(0.0, None), -1);
        // Clip-local time inside the insert, and the recording time it interrupts.
        assert!(qcompare(timeline.source_time(4.0), 3.0));
        assert!(qcompare(timeline.recording_source_time(4.0), 3.0));
        assert!(qcompare(timeline.recording_source_time(1.5), 1.5));
        assert!(qcompare(timeline.recording_source_time(6.0), 6.0));
        let leading = ClipTimeline::new(vec![insert, clip("b", 5.0, 7.0, 1.0)]);
        assert!(qcompare(leading.recording_source_time(1.0), 5.0));
        // Recording neighbours bound a recording clip, across the insert.
        assert_eq!(timeline.recording_bounds(0, 9.0), (0.0, 5.0));
        assert_eq!(timeline.recording_bounds(2, 9.0), (3.0, 9.0));
        // Recording spans never map into the insert, whose in/out overlap them numerically.
        assert_eq!(timeline.output_range(2.5, 4.5), (2.5, 3.0));
        assert_eq!(timeline.output_range(3.5, 4.5), (-1.0, -1.0));
        assert_eq!(timeline.output_range(4.0, 6.0), (5.5, 6.0));
    }

    #[test]
    fn source_span_covers_what_survives_the_cuts() {
        // Source 1.438–1.903 and everything after 2.245 were cut.
        let timeline = ClipTimeline::new(vec![clip("a", 0.0, 1.438, 1.0), clip("b", 1.903, 2.245, 1.0)]);
        let across = timeline.output_range(0.376, 2.583);
        assert!(qcompare(across.0, 0.376));
        assert!((across.1 - (1.438 + 2.245 - 1.903)).abs() < 1e-9);
        let ends_in_cut = timeline.output_range(0.781, 1.781);
        assert!(qcompare(ends_in_cut.0, 0.781));
        assert!(qcompare(ends_in_cut.1, 1.438));
        let starts_in_cut = timeline.output_range(1.6, 2.0);
        assert!(qcompare(starts_in_cut.0, 1.438));
        assert!((starts_in_cut.1 - (1.438 + 2.0 - 1.903)).abs() < 1e-9);
        let all_cut = timeline.output_range(2.3, 2.583);
        assert!(qcompare(all_cut.0, -1.0));
        assert!(qcompare(all_cut.1, -1.0));

        let faster = ClipTimeline::new(vec![clip("x", 1.0, 3.0, 2.0)]);
        let halved = faster.output_range(0.0, 2.0);
        assert!(qcompare(halved.0, 0.0));
        assert!(qcompare(halved.1, 0.5));
    }

    #[test]
    fn deletes_middle_and_maps_the_new_cut() {
        let mut timeline = ClipTimeline::new(vec![clip("first", 0.0, 3.0, 1.0)]);
        assert!(timeline.delete_range(1.0, 2.0, "remainder"));
        assert_eq!(timeline.clips().len(), 2);
        assert_eq!(timeline.clips()[0].id, "first");
        assert!(qcompare(timeline.clips()[0].out, 1.0));
        assert_eq!(timeline.clips()[1].id, "remainder");
        assert!(qcompare(timeline.clips()[1].in_, 2.0));
        assert!(qcompare(timeline.total_duration(), 2.0));
        assert!(qcompare(timeline.source_time(0.99), 0.99));
        assert!(qcompare(timeline.source_time(1.0), 2.0));
    }

    #[test]
    fn deletes_across_clips_with_different_speeds() {
        let mut timeline = ClipTimeline::new(vec![clip("normal", 0.0, 2.0, 1.0), clip("fast", 4.0, 8.0, 2.0)]);
        assert!(timeline.delete_range(1.5, 2.5, "unused"));
        assert_eq!(timeline.clips().len(), 2);
        assert!(qcompare(timeline.clips()[0].out, 1.5));
        assert!(qcompare(timeline.clips()[1].in_, 5.0));
        assert!(qcompare(timeline.total_duration(), 3.0));
        assert!(qcompare(timeline.source_time(1.5), 5.0));
        assert!(qcompare(timeline.source_time(2.0), 6.0));
    }

    #[test]
    fn rejects_unrepresentable_or_complete_cuts_without_changing_clips() {
        let mut timeline = ClipTimeline::new(vec![clip("only", 0.0, 2.0, 1.0)]);
        assert!(!timeline.delete_range(0.0, 2.0, "right"));
        assert!(!timeline.delete_range(0.05, 1.0, "right"));
        assert!(!timeline.delete_range(1.0, 1.0, "right"));
        assert!(!timeline.delete_range(0.5, 1.5, "only"));
        assert_eq!(timeline.clips().len(), 1);
        assert!(qcompare(timeline.total_duration(), 2.0));
    }

    // Not in the C++ tests: split ids count up per timeline, and go through QString::arg.
    #[test]
    fn split_ids_count_up_through_qstring_arg() {
        let mut timeline = ClipTimeline::new(vec![clip("c1", 0.0, 3.0, 1.0), clip("a%1", 3.0, 6.0, 1.0)]);
        assert!(timeline.split(1.0));
        assert!(timeline.split(2.0));
        assert!(timeline.split(4.0));
        let ids: Vec<&str> = timeline.clips().iter().map(|clip| clip.id.as_str()).collect();
        assert_eq!(ids, ["c1", "c1-s1", "c1-s1-s2", "a%1", "a3-s%2"]);
        assert!(!timeline.split(4.0));
        assert!(!timeline.split(6.0));
    }

    #[test]
    fn indices_out_of_range() {
        let mut timeline = ClipTimeline::new(vec![clip("a", 0.0, 1.0, 1.0), clip("b", 2.0, 3.0, 1.0)]);
        assert_eq!(timeline.output_time(-1, 0.5), -1.0);
        assert_eq!(timeline.output_time(2, 0.5), -1.0);
        assert_eq!(timeline.recording_bounds(-1, 9.0), (0.0, 0.0));
        assert_eq!(timeline.recording_bounds(2, 9.0), (3.0, 9.0));
        assert!(!timeline.trim(-1, 0.0, 1.0));
        assert!(!timeline.trim(0, -0.1, 1.0));
        assert!(!timeline.trim(0, 1.0, 1.0));
        assert!(!timeline.remove(2));
        assert_eq!(ClipTimeline::default().source_time(1.0), -1.0);
        assert_eq!(ClipTimeline::default().recording_source_time(1.0), -1.0);
    }
}
