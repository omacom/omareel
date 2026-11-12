//! The marks a recording session leaves for the editor: `markers.jsonl` in the bundle.
//!
//! While a recording runs, `omareel record --marker`, `--retake` and `--pause` (and the
//! recording bar's buttons) append one compact JSON object per line, each with `t`, the
//! monotonic microseconds of the moment (the clock of `input.jsonl` and `capture.json`), and `k`,
//! its kind:
//!
//! - `{"k":"marker","t":…,"label":"good take"}`: a moment worth finding again; `label` is left out
//!   when there is none.
//! - `{"from":…,"k":"retake","t":…}`: "that didn't go well": `from`..`t` is cut from the new
//!   project, and the editor puts a retake marker at `t`.
//! - `{"k":"pause","t":…}` and `{"k":"resume","t":…}`: the recorder paused and resumed. The span
//!   between them is cut; a pause without a resume runs to the end.
//!
//! Any of them may carry `"via":"bar"` when the recording bar's button made it rather than a
//! command (the recorder uses it to leave the button's click out of `input.jsonl`), and `"at"`,
//! the monotonic microseconds of the moment it was made when that differs from `t`: a marker or
//! retake made while paused belongs to the pause's start (`t`, the frame the video holds) but was
//! made later (`at`), and a pause or resume was asked for at `at`. The recorder looks for the
//! keybind or click that made a mark just before `at`; the editor ignores marks made after
//! `capture.json`'s `stopped_us` (see [`RecordingMarks::made_by`]).
//!
//! The file is its own rather than part of `input.jsonl` or `capture.json` because those are
//! written once, at stop, from what the recorder held in memory: marks are appended the moment
//! they are made, by the process that made them, so a recorder that crashes keeps them, and the
//! recorder itself only has to read them back. Bundles recorded before marks existed have no such
//! file and nothing about them changes. The editor reads it only when it makes a bundle's
//! project for the first time; from then on the markers live in `project.json`.

use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qfile;
use crate::project::Clip;

/// The marks file's name inside a bundle.
pub const MARKS_FILE: &str = "markers.jsonl";

/// How far back a retake cuts when nothing was marked since the take began.
pub const DEFAULT_RETAKE_SECONDS: f64 = 5.0;

/// Clips shorter than this are left out of a new project rather than kept as slivers.
const MIN_CLIP_SECONDS: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Marker,
    Retake,
    Pause,
    Resume,
}

impl MarkKind {
    pub fn name(self) -> &'static str {
        match self {
            MarkKind::Marker => "marker",
            MarkKind::Retake => "retake",
            MarkKind::Pause => "pause",
            MarkKind::Resume => "resume",
        }
    }

    pub fn from_name(name: &str) -> Option<MarkKind> {
        match name {
            "marker" => Some(MarkKind::Marker),
            "retake" => Some(MarkKind::Retake),
            "pause" => Some(MarkKind::Pause),
            "resume" => Some(MarkKind::Resume),
            _ => None,
        }
    }
}

/// One line of `markers.jsonl`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub monotonic_us: i64,
    pub kind: MarkKind,
    /// A marker's label; empty for none.
    pub label: String,
    /// Where a retake's cut starts (monotonic microseconds); 0 for the other kinds.
    pub from_us: i64,
    /// "bar" when the recording bar's button made it; empty for a command.
    pub via: String,
    /// When it was made (monotonic microseconds), when that differs from `monotonic_us`; 0 for
    /// the same moment. See [`Mark::made_us`].
    pub at_us: i64,
}

impl Mark {
    pub fn new(kind: MarkKind, monotonic_us: i64) -> Mark {
        Mark { monotonic_us, kind, label: String::new(), from_us: 0, via: String::new(), at_us: 0 }
    }

    /// The moment it was really made: `at` when given, else `t`. A mark's trigger (keybind or
    /// click) is just before this, whatever its place on the timeline.
    pub fn made_us(&self) -> i64 {
        if self.at_us > 0 { self.at_us } else { self.monotonic_us }
    }

    /// Sets `at` to `made_us`, or leaves it out when it is `t` (or unknown).
    pub fn set_made_us(&mut self, made_us: i64) {
        self.at_us = if made_us > 0 && made_us != self.monotonic_us { made_us } else { 0 };
    }

    pub fn from_bar(&self) -> bool {
        self.via == "bar"
    }

    /// The line's JSON, compact, without its newline.
    pub fn to_line(&self) -> String {
        let mut object = Object::new();
        object.insert("t", self.monotonic_us);
        object.insert("k", self.kind.name());
        if !self.label.is_empty() {
            object.insert("label", self.label.as_str());
        }
        if self.kind == MarkKind::Retake {
            object.insert("from", self.from_us);
        }
        if !self.via.is_empty() {
            object.insert("via", self.via.as_str());
        }
        if self.at_us > 0 {
            object.insert("at", self.at_us);
        }
        Value::Object(object).to_json(Format::Compact)
    }

    /// A line's object; `None` for an unknown kind or a missing time.
    pub fn from_json(object: &Object) -> Option<Mark> {
        let kind = MarkKind::from_name(&object.value("k").to_string_or(""))?;
        let monotonic_us = object.value("t").to_variant_long_long();
        if monotonic_us <= 0 {
            return None;
        }
        let mut mark = Mark::new(kind, monotonic_us);
        mark.label = object.value("label").to_string_or("");
        mark.via = object.value("via").to_string_or("");
        mark.set_made_us(object.value("at").to_variant_long_long());
        if kind == MarkKind::Retake {
            mark.from_us = object.value("from").to_variant_long_long().min(monotonic_us);
        }
        Some(mark)
    }
}

/// Everything `markers.jsonl` holds, in time order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordingMarks {
    pub marks: Vec<Mark>,
}

impl RecordingMarks {
    /// The file's lines. A line that does not parse is skipped rather than failing the rest: the
    /// last one can be torn when a recorder dies as it writes.
    pub fn parse(bytes: &[u8]) -> RecordingMarks {
        let mut marks: Vec<Mark> = qfile::lines(bytes)
            .filter_map(|line| match qjson::from_json(qfile::trimmed(line)) {
                Ok(Value::Object(object)) => Mark::from_json(&object),
                _ => None,
            })
            .collect();
        marks.sort_by_key(|mark| mark.monotonic_us);
        RecordingMarks { marks }
    }

    /// `markers.jsonl` of a bundle; empty when it has none.
    pub fn load_bundle(bundle_path: &str) -> RecordingMarks {
        qfile::read_all(format!("{bundle_path}/{MARKS_FILE}")).map(|bytes| RecordingMarks::parse(&bytes)).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// Only the marks made by `stopped_us` (`capture.json`'s, taken just before the recorder
    /// read the marks to clean `input.jsonl`); all of them when it is 0. A command that marks as
    /// the recorder stops cannot then cut or mark what its input was never cleaned for.
    pub fn made_by(mut self, stopped_us: i64) -> RecordingMarks {
        if stopped_us > 0 {
            self.marks.retain(|mark| mark.made_us() <= stopped_us);
        }
        self
    }

    /// The paused spans in monotonic microseconds, in order; a pause still open at the end runs
    /// to `i64::MAX`. A second pause before a resume, or a resume without a pause, is ignored.
    pub fn pauses(&self) -> Vec<(i64, i64)> {
        let mut spans = Vec::new();
        let mut open: Option<i64> = None;
        for mark in &self.marks {
            match (mark.kind, open) {
                (MarkKind::Pause, None) => open = Some(mark.monotonic_us),
                (MarkKind::Resume, Some(start)) => {
                    spans.push((start, mark.monotonic_us));
                    open = None;
                }
                _ => {}
            }
        }
        if let Some(start) = open {
            spans.push((start, i64::MAX));
        }
        spans
    }

    /// The retakes' cuts in monotonic microseconds.
    pub fn retakes(&self) -> Vec<(i64, i64)> {
        self.marks
            .iter()
            .filter(|mark| mark.kind == MarkKind::Retake && mark.from_us < mark.monotonic_us)
            .map(|mark| (mark.from_us, mark.monotonic_us))
            .collect()
    }

    /// The markers and retake markers, in time order.
    pub fn markers(&self) -> impl Iterator<Item = &Mark> {
        self.marks.iter().filter(|mark| matches!(mark.kind, MarkKind::Marker | MarkKind::Retake))
    }

    /// What the new project cuts, in seconds of `screen.mp4` (its first frame at
    /// `first_frame_us`), clamped to `0..duration`, sorted and merged.
    pub fn cut_spans(&self, first_frame_us: i64, duration: f64) -> Vec<(f64, f64)> {
        let seconds = |us: i64| {
            if us == i64::MAX { duration } else { (us.saturating_sub(first_frame_us)) as f64 / 1_000_000.0 }
        };
        let spans: Vec<(f64, f64)> = self
            .pauses()
            .into_iter()
            .chain(self.retakes())
            .map(|(start, end)| (seconds(start).clamp(0.0, duration), seconds(end).clamp(0.0, duration)))
            .filter(|(start, end)| end > start)
            .collect();
        merge_spans(spans)
    }
}

/// Sorts spans and joins those that overlap or touch.
pub fn merge_spans(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(spans.len());
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Whether `t` is inside one of the (sorted or not) half-open spans `start..end`.
pub fn in_spans(t: i64, spans: &[(i64, i64)]) -> bool {
    spans.iter().any(|&(start, end)| t >= start && t < end)
}

/// Where a retake made at `now_us` cuts back to: `seconds` of recorded time before it when given
/// (paused time does not count: the cut reaches back past a pause until it has that many seconds
/// that were recorded); otherwise the last marker, retake or resume before it (where the take
/// began), or [`DEFAULT_RETAKE_SECONDS`] of recorded time before it when there is none. Never
/// before `started_us`, the recording's start.
pub fn retake_from(now_us: i64, marks: &[Mark], started_us: i64, seconds: Option<f64>) -> i64 {
    let pauses = RecordingMarks { marks: marks.to_vec() }.pauses();
    let back = |seconds: f64| recorded_back(now_us, (seconds.max(0.0) * 1_000_000.0).round() as i64, &pauses);
    let from = match seconds {
        Some(seconds) => back(seconds),
        None => marks
            .iter()
            .filter(|mark| matches!(mark.kind, MarkKind::Marker | MarkKind::Retake | MarkKind::Resume))
            .filter(|mark| mark.monotonic_us < now_us)
            .map(|mark| mark.monotonic_us)
            .max()
            .unwrap_or_else(|| back(DEFAULT_RETAKE_SECONDS)),
    };
    from.max(started_us).min(now_us)
}

/// The moment `recorded_us` of recorded (unpaused) time before `now_us`, given the `pauses`
/// (sorted, as [`RecordingMarks::pauses`] gives them).
pub fn recorded_back(now_us: i64, recorded_us: i64, pauses: &[(i64, i64)]) -> i64 {
    let mut left = recorded_us.max(0);
    let mut cursor = now_us;
    for &(start, end) in pauses.iter().rev() {
        if start >= cursor {
            continue;
        }
        let end = end.min(cursor);
        if cursor - end >= left {
            return cursor - left;
        }
        left -= cursor - end;
        cursor = start;
    }
    cursor.saturating_sub(left)
}

/// The recorded (unpaused) microseconds in `from_us..to_us`.
pub fn recorded_between(from_us: i64, to_us: i64, pauses: &[(i64, i64)]) -> i64 {
    let paused: i64 =
        pauses.iter().map(|&(start, end)| (end.min(to_us) - start.max(from_us)).max(0)).sum();
    (to_us - from_us - paused).max(0)
}

/// The recording clips of a new project whose recording runs `0..duration` with `cuts` (sorted,
/// merged) left out: `c1`, `c2`… over what is kept, slivers under 0.1 s dropped. A cut span is
/// never shown: when every kept piece is a sliver the longest one stays, and when the cuts cover
/// everything there are none (the caller decides, see
/// [`crate::editor::apply_recording_marks`]).
pub fn kept_clips(duration: f64, cuts: &[(f64, f64)]) -> Vec<Clip> {
    let mut pieces = Vec::new();
    let mut start = 0.0;
    for &(cut_start, cut_end) in cuts {
        if cut_start > start {
            pieces.push((start, cut_start.min(duration)));
        }
        start = start.max(cut_end);
    }
    if start < duration {
        pieces.push((start, duration));
    }
    pieces.retain(|(from, to)| to > from);
    if pieces.iter().any(|(from, to)| to - from >= MIN_CLIP_SECONDS) {
        pieces.retain(|(from, to)| to - from >= MIN_CLIP_SECONDS);
    } else if let Some(longest) = pieces.iter().copied().max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0))) {
        pieces = vec![longest];
    }
    pieces
        .into_iter()
        .enumerate()
        .map(|(index, (in_, out))| Clip { id: format!("c{}", index + 1), in_, out, speed: 1.0, ..Clip::default() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(kind: MarkKind, t: i64) -> Mark {
        Mark::new(kind, t)
    }

    #[test]
    fn lines_round_trip() {
        let mut marker = mark(MarkKind::Marker, 1_500_000);
        marker.label = "good take".into();
        assert_eq!(marker.to_line(), r#"{"k":"marker","label":"good take","t":1500000}"#);
        let mut retake = mark(MarkKind::Retake, 9_000_000);
        retake.from_us = 4_000_000;
        retake.via = "bar".into();
        assert_eq!(retake.to_line(), r#"{"from":4000000,"k":"retake","t":9000000,"via":"bar"}"#);
        let pause = mark(MarkKind::Pause, 10_000_000);
        assert_eq!(pause.to_line(), r#"{"k":"pause","t":10000000}"#);
        let text = [retake.to_line(), "{\"k\":\"resume\",\"t\":12000000}".into(), marker.to_line(), pause.to_line(), "{\"k\":\"mar".into()]
            .join("\n");
        let parsed = RecordingMarks::parse(text.as_bytes());
        assert_eq!(parsed.marks.len(), 4, "the torn last line is skipped");
        assert_eq!(parsed.marks[0], marker);
        assert_eq!(parsed.marks[1], retake);
        assert!(parsed.marks[1].from_bar());
        assert_eq!(parsed.marks[2], pause);
        assert_eq!(parsed.marks[3].kind, MarkKind::Resume);
        assert_eq!(RecordingMarks::parse(b"{\"k\":\"zoom\",\"t\":5}\n{\"k\":\"marker\"}\n"), RecordingMarks::default());
    }

    #[test]
    fn pauses_pair_up_and_an_open_one_runs_to_the_end() {
        let marks = RecordingMarks {
            marks: vec![
                mark(MarkKind::Resume, 1),
                mark(MarkKind::Pause, 10),
                mark(MarkKind::Pause, 12),
                mark(MarkKind::Resume, 20),
                mark(MarkKind::Pause, 30),
            ],
        };
        assert_eq!(marks.pauses(), [(10, 20), (30, i64::MAX)]);
    }

    #[test]
    fn retakes_cut_back_to_where_the_take_began() {
        let started = 1_000_000;
        let marks = [mark(MarkKind::Marker, 3_000_000), mark(MarkKind::Resume, 6_000_000), mark(MarkKind::Pause, 7_000_000)];
        assert_eq!(retake_from(10_000_000, &marks, started, None), 6_000_000, "the resume began the take");
        assert_eq!(retake_from(5_000_000, &marks, started, None), 3_000_000, "the marker before it");
        assert_eq!(retake_from(2_500_000, &marks, started, None), started, "five seconds, but not before the start");
        assert_eq!(retake_from(20_000_000, &[], started, None), 15_000_000);
        assert_eq!(retake_from(10_000_000, &marks[..2], started, Some(2.0)), 8_000_000, "seconds given win");
    }

    #[test]
    fn retakes_and_pauses_become_cuts_and_clips() {
        let first = 1_000_000;
        let mut retake = mark(MarkKind::Retake, first + 8_000_000);
        retake.from_us = first + 5_000_000;
        let marks = RecordingMarks {
            marks: vec![
                retake,
                mark(MarkKind::Pause, first + 7_500_000),
                mark(MarkKind::Resume, first + 9_000_000),
                mark(MarkKind::Pause, first + 15_000_000),
            ],
        };
        let cuts = marks.cut_spans(first, 20.0);
        assert_eq!(cuts, [(5.0, 9.0), (15.0, 20.0)], "overlapping cuts merge; the open pause runs to the end");
        let clips = kept_clips(20.0, &cuts);
        let spans: Vec<(&str, f64, f64)> = clips.iter().map(|clip| (clip.id.as_str(), clip.in_, clip.out)).collect();
        assert_eq!(spans, [("c1", 0.0, 5.0), ("c2", 9.0, 15.0)]);
        assert!(clips.iter().all(|clip| clip.speed == 1.0 && !clip.is_media()));
        let sliver: Vec<(f64, f64)> = kept_clips(20.0, &[(0.0, 19.95)]).iter().map(|c| (c.in_, c.out)).collect();
        assert_eq!(sliver, [(19.95, 20.0)], "only slivers: the longest stays, never the cut");
        let slivers: Vec<(f64, f64)> =
            kept_clips(20.0, &[(0.02, 10.0), (10.05, 20.0)]).iter().map(|c| (c.in_, c.out)).collect();
        assert_eq!(slivers, [(10.0, 10.05)]);
        assert!(kept_clips(20.0, &[(0.0, 20.0)]).is_empty(), "everything cut: no clip");
        assert_eq!(kept_clips(20.0, &[]).len(), 1);
    }

    #[test]
    fn retake_seconds_count_recorded_time_only() {
        let started = 1_000_000;
        // Paused 10..70 s; a five-second retake two seconds after the resume reaches back three
        // seconds before the pause.
        let marks = [mark(MarkKind::Pause, 10_000_000), mark(MarkKind::Resume, 70_000_000)];
        assert_eq!(retake_from(72_000_000, &marks, started, Some(5.0)), 7_000_000);
        let pauses = RecordingMarks { marks: marks.to_vec() }.pauses();
        assert_eq!(recorded_between(7_000_000, 72_000_000, &pauses), 5_000_000);
        let mut retake = mark(MarkKind::Retake, 72_000_000);
        retake.from_us = 7_000_000;
        let mut all = marks.to_vec();
        all.push(retake);
        assert_eq!(RecordingMarks { marks: all }.cut_spans(started, 80.0), [(6.0, 71.0)]);
        // Made while paused (at the pause's start), it counts back from there.
        assert_eq!(retake_from(10_000_000, &marks[..1], started, Some(2.0)), 8_000_000);
        // Not before the start, nor past two pauses' worth of nothing.
        assert_eq!(retake_from(72_000_000, &marks, started, Some(100.0)), started);
        let two = [
            mark(MarkKind::Pause, 4_000_000),
            mark(MarkKind::Resume, 5_000_000),
            mark(MarkKind::Pause, 6_000_000),
            mark(MarkKind::Resume, 8_000_000),
        ];
        let pauses = RecordingMarks { marks: two.to_vec() }.pauses();
        assert_eq!(recorded_back(9_000_000, 2_500_000, &pauses), 3_500_000);
        assert_eq!(recorded_back(9_000_000, 1_000_000, &pauses), 8_000_000, "exactly up to the resume");
        assert_eq!(recorded_back(9_000_000, 0, &pauses), 9_000_000);
        assert_eq!(recorded_between(3_500_000, 9_000_000, &pauses), 2_500_000);
        assert_eq!(retake_from(9_000_000, &two, started, None), 8_000_000, "the resume began the take");
        assert_eq!(retake_from(9_000_000, &[], started, None), 4_000_000);
        assert_eq!(retake_from(9_000_000, &two[..2], started, None), 5_000_000);
    }

    #[test]
    fn marks_keep_when_they_were_made() {
        let mut marker = mark(MarkKind::Marker, 10_000_000);
        marker.set_made_us(15_000_000);
        assert_eq!(marker.to_line(), r#"{"at":15000000,"k":"marker","t":10000000}"#);
        assert_eq!(marker.made_us(), 15_000_000);
        let parsed = RecordingMarks::parse(marker.to_line().as_bytes());
        assert_eq!(parsed.marks, [marker.clone()]);
        let mut same = mark(MarkKind::Marker, 10_000_000);
        same.set_made_us(10_000_000);
        assert_eq!(same.to_line(), r#"{"k":"marker","t":10000000}"#, "no at when it is t");
        assert_eq!(same.made_us(), 10_000_000);
        let marks = RecordingMarks { marks: vec![same.clone(), marker.clone()] };
        assert_eq!(marks.clone().made_by(12_000_000).marks, [same], "made after the stop: ignored");
        assert_eq!(marks.clone().made_by(0), marks);
    }

    #[test]
    fn spans_merge_and_contain() {
        assert_eq!(merge_spans(vec![(4.0, 6.0), (1.0, 2.0), (2.0, 3.0)]), [(1.0, 3.0), (4.0, 6.0)]);
        assert!(in_spans(5, &[(5, 7)]) && !in_spans(7, &[(5, 7)]) && !in_spans(4, &[(5, 7)]));
    }
}
