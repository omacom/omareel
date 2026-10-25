//! cliptimeline: `clip_timeline::ClipTimeline` against `Omareel::ClipTimeline` (README
//! "cliptimeline"), and the corpus clip lists layouttimeline reads too.

use crate::support::*;
use omareel_core::clip_timeline::ClipTimeline;
use omareel_core::compat::cpp;
use omareel_core::compat::qjson::Value;
use omareel_core::project::Clip;

/// The oracle's `clipFromJson`: `{id, in, out, speed[, layout][, media][, fit]}` made into a
/// `Clip` directly, a missing key keeping `Clip`'s default (nothing validated, unlike
/// `Project::fromJson`).
fn clip_from_json(o: &Value) -> Clip {
    let o = o.to_object();
    let mut clip = Clip {
        id: o.value("id").to_string_or(""),
        in_: o.value("in").to_double(0.0),
        out: o.value("out").to_double(0.0),
        speed: o.value("speed").to_double(1.0),
        ..Clip::default()
    };
    if o.contains("layout") {
        clip.layout = o.value("layout").to_string_or("");
    }
    if o.contains("media") {
        clip.media = o.value("media").to_string_or("");
    }
    if o.contains("fit") {
        clip.fit = o.value("fit").to_string_or("");
    }
    clip
}

pub fn clips_from_json(array: &Value) -> Vec<Clip> {
    array.to_array().iter().map(clip_from_json).collect()
}

fn cases() -> Value {
    read_json("cliptimeline/cases.json")
}

/// b₀ = 0, then each clip's output length accumulated as `clipAt` does (negative for inverted
/// clips).
fn clip_boundaries(clips: &[Clip]) -> Vec<f64> {
    let mut bounds = vec![0.0];
    let mut start = 0.0;
    for clip in clips {
        start += (clip.out - clip.in_) / cpp::max(0.000001, clip.speed);
        bounds.push(start);
    }
    bounds
}

/// The grid of i/`denominator` up to ⌈`denominator`·min(T, 12)⌉ + `margin`.
fn time_grid(denominator: i32, margin: i32, total: f64) -> Vec<f64> {
    let last = (f64::from(denominator) * cpp::min(total, 12.0)).ceil() as i32 + margin;
    uniform(denominator, -margin, last)
}

/// Clip indices -1..=n, as the C++ `int`s.
fn indices(clips: &[Clip]) -> std::ops::RangeInclusive<i32> {
    -1..=i32::try_from(clips.len()).expect("a small clip list")
}

fn queries(out: &mut Text, clips: &[Clip]) {
    let timeline = ClipTimeline::new(clips.to_vec());
    let total = timeline.total_duration();
    let bounds = clip_boundaries(clips);
    out.row(["clips".into(), clips_text(timeline.clips())]);
    out.row(["totalDuration".into(), num(total)]);

    let mut grid = time_grid(20, 10, total);
    for &b in &bounds {
        grid.extend([b - 1e-9, before(b), b, after(b), b + 1e-9]);
    }
    let mut grid = sorted_unique(grid);
    grid.extend([f64::NEG_INFINITY, f64::INFINITY, f64::NAN]);
    for t in grid {
        let mut start = -7.0;
        let at = timeline.clip_at(t, Some(&mut start));
        out.row([
            "at".into(),
            num(t),
            at.to_string(),
            num(start),
            num(timeline.source_time(t)),
            num(timeline.recording_source_time(t)),
        ]);
    }

    for i in indices(clips) {
        for source_duration in [8.0, 100.0] {
            let (lower, upper) = timeline.recording_bounds(i, source_duration);
            out.row(["recordingBounds".into(), i.to_string(), num(source_duration), num(lower), num(upper)]);
        }
    }

    let mut sources = uniform(2, -2, 24);
    for clip in clips {
        sources.extend([before(clip.in_), clip.in_, after(clip.in_), before(clip.out), clip.out, after(clip.out)]);
    }
    let mut sources = sorted_unique(sources);
    sources.extend([f64::NEG_INFINITY, f64::INFINITY, f64::NAN]);
    for i in indices(clips) {
        for &source in &sources {
            out.row(["outputTime".into(), i.to_string(), num(source), num(timeline.output_time(i, source))]);
        }
    }

    let mut ends = vec![-1.0, 0.0, 100.0];
    for clip in clips {
        ends.extend([clip.in_, clip.out, (clip.in_ + clip.out) / 2.0]);
    }
    let ends = sorted_unique(ends);
    for &from in &ends {
        for &to in &ends {
            let (first, second) = timeline.output_range(from, to);
            out.row(["outputRange".into(), num(from), num(to), num(first), num(second)]);
        }
    }
}

fn edits(out: &mut Text, clips: &[Clip], split_sequence: &Value) {
    let timeline = ClipTimeline::new(clips.to_vec());
    let total = timeline.total_duration();
    let bounds = clip_boundaries(clips);

    let mut splits = time_grid(4, 2, total);
    for &b in &bounds {
        let (below, above) = (b - 1e-9, b + 1e-9);
        splits.extend([before(below), below, after(below), b, before(above), above, after(above)]);
    }
    let mut splits = sorted_unique(splits);
    splits.extend([f64::NEG_INFINITY, f64::INFINITY, f64::NAN]);
    for t in splits {
        let mut edited = timeline.clone();
        let ok = edited.split(t);
        out.row(["split".into(), num(t), boolean(ok).into(), clips_text(edited.clips())]);
    }
    // One timeline split again and again: ids count up in the object.
    let mut sequence = timeline.clone();
    for value in split_sequence.to_array() {
        let t = value.to_double(0.0);
        let ok = sequence.split(t);
        out.row(["splitSequence".into(), num(t), boolean(ok).into(), clips_text(sequence.clips())]);
    }

    let trims = [
        (0.0, 1.0),
        (0.5, 7.5),
        (-0.0, 2.0),
        (-1e-300, 2.0),
        (2.0, 2.0),
        (3.0, 2.0),
        (0.0, 1e-300),
        (5.0, f64::INFINITY),
        (f64::NAN, 1.0),
        (1.0, f64::NAN),
    ];
    for i in indices(clips) {
        for (new_in, new_out) in trims {
            let mut edited = timeline.clone();
            let ok = edited.trim(i, new_in, new_out);
            out.row([
                "trim".into(),
                i.to_string(),
                num(new_in),
                num(new_out),
                boolean(ok).into(),
                clips_text(edited.clips()),
            ]);
        }
    }

    for i in indices(clips) {
        let mut edited = timeline.clone();
        let ok = edited.remove(i);
        out.row(["remove".into(), i.to_string(), boolean(ok).into(), clips_text(edited.clips())]);
    }

    let n = clips.len();
    let mut points = vec![-1.0, 0.0, total, total + 1.0];
    for &b in &bounds[1..n.max(1)] {
        points.extend([b - 0.05, b, b + 0.05]);
    }
    for k in 1..=n {
        points.push((bounds[k - 1] + bounds[k]) / 2.0);
    }
    let points = sorted_unique(points);
    let mut ranges: Vec<(f64, f64)> = Vec::new();
    for &from in &points {
        for &to in &points {
            if from < to {
                ranges.push((from, to));
            }
        }
    }
    ranges.extend([
        (total, 0.0),
        (f64::NAN, total),
        (0.0, f64::NAN),
        (f64::NEG_INFINITY, total),
        (0.0, f64::INFINITY),
        (f64::NEG_INFINITY, f64::INFINITY),
    ]);
    let existing = clips.first().map_or("c1", |clip| clip.id.as_str());
    for (from, to) in ranges {
        let (mut with_id, mut without_id, mut with_existing) = (timeline.clone(), timeline.clone(), timeline.clone());
        let ok = with_id.delete_range(from, to, "r");
        let ok_without = without_id.delete_range(from, to, "");
        let ok_existing = with_existing.delete_range(from, to, existing);
        // The oracle checked that the other two remainder ids fail and leave the clips alone or
        // give exactly the "r" result; the port has to keep that rule too.
        for (edited, result) in [(&without_id, ok_without), (&with_existing, ok_existing)] {
            let expected = if result { with_id.clips() } else { clips };
            assert!(
                !(result && !ok) && clips_text(edited.clips()) == clips_text(expected),
                "deleteRange({}, {}) breaks the remainder-id rule",
                num(from),
                num(to)
            );
        }
        out.row([
            "deleteRange".into(),
            num(from),
            num(to),
            boolean(ok).into(),
            boolean(ok_without).into(),
            boolean(ok_existing).into(),
            clips_text(with_id.clips()),
        ]);
    }
}

#[test]
fn queries_txt() {
    let mut out = Text::new();
    for (name, clips) in cases().get("timelines").to_object().iter() {
        out.section(name);
        queries(&mut out, &clips_from_json(clips));
    }
    assert_golden("cliptimeline/queries.txt", &out.into_string());
}

#[test]
fn edits_txt() {
    let cases = cases();
    let sequences = cases.get("splitSequences").to_object();
    let mut out = Text::new();
    for (name, clips) in cases.get("timelines").to_object().iter() {
        out.section(name);
        edits(&mut out, &clips_from_json(clips), sequences.value(name));
    }
    assert_golden("cliptimeline/edits.txt", &out.into_string());
}
