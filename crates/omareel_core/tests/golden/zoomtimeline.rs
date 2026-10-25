//! zoomtimeline: `zoom_timeline` against `Omareel::ZoomTimeline` (README "zoomtimeline").

use crate::cursorpath::{point, samples_from_json, synthetic_click_times, synthetic_samples};
use crate::support::*;
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::Value;
use omareel_core::cursor_path::{self, CursorSample};
use omareel_core::project::{ZoomSegment, ZoomStyle};
use omareel_core::zoom_timeline;

fn segment_row(s: &ZoomSegment) -> Vec<String> {
    vec![
        quote(&s.id),
        num(s.start),
        num(s.end),
        num(s.level),
        boolean(s.automatic_target).into(),
        num(s.target.x),
        num(s.target.y),
    ]
}

/// `{id, start, end, level, auto, x, y}` objects, read with `QJsonValue`'s defaults.
fn segments_from_json(array: &Value) -> Vec<ZoomSegment> {
    array
        .to_array()
        .iter()
        .map(|o| ZoomSegment {
            id: o.get("id").to_string_or(""),
            start: o.get("start").to_double(0.0),
            end: o.get("end").to_double(0.0),
            level: o.get("level").to_double(0.0),
            automatic_target: o.get("auto").to_bool(false),
            target: PointF::new(o.get("x").to_double(0.0), o.get("y").to_double(0.0)),
        })
        .collect()
}

fn doubles(array: &Value) -> Vec<f64> {
    array.to_array().iter().map(|v| v.to_double(0.0)).collect()
}

fn cases() -> Value {
    read_json("zoomtimeline/cases.json")
}

#[test]
fn generate_txt() {
    let cases = cases();
    let mut out = Text::new();
    let emit = |out: &mut Text, name: &str, clicks: &[f64], duration: f64| {
        out.section(&format!("{name} duration={}", num(duration)));
        for segment in zoom_timeline::generate(clicks, duration) {
            out.row(segment_row(&segment));
        }
    };
    for (name, case) in cases.get("generate").to_object().iter() {
        let clicks = doubles(case.get("clicks"));
        for duration in doubles(case.get("durations")) {
            emit(&mut out, name, &clicks, duration);
        }
    }
    let clicks = synthetic_click_times();
    let mut cells = vec!["synthetic clicks".to_owned()];
    cells.extend(clicks.iter().map(|&t| num(t)));
    out.row(cells);
    for duration in [8.0, 5.0, 3.0] {
        emit(&mut out, "synthetic", &clicks, duration);
    }
    assert_golden("zoomtimeline/generate.txt", &out.into_string());
}

/// `synthetic-generated` first, then the crafted sets in key order.
fn segment_sets(cases: &Value) -> Vec<(String, Vec<ZoomSegment>)> {
    let mut sets = vec![("synthetic-generated".to_owned(), zoom_timeline::generate(&synthetic_click_times(), 8.0))];
    for (name, segments) in cases.get("segments").to_object().iter() {
        sets.push((name.to_owned(), segments_from_json(segments)));
    }
    sets
}

/// Sections that need `Project::fromJson`, which is not ported yet.
fn is_project_section(label: &str) -> bool {
    label.starts_with("segments=project:")
}

#[test]
fn target_at_txt() {
    let cases = cases();
    let sets = segment_sets(&cases);
    let synthetic = cursor_path::decimate(&synthetic_samples(true), 60.0, 1.0 / 640.0);
    let crafted = read_json("cursorpath/cases.json").get("samples").to_object();
    let sample_sets: Vec<(&str, Vec<CursorSample>)> = vec![
        ("synthetic", synthetic.clone()),
        ("random-walk-normalized", samples_from_json(crafted.value("random-walk-normalized"))),
        ("empty", Vec::new()),
        ("single", samples_from_json(crafted.value("single"))),
    ];
    let times: Vec<f64> = (-15..=255).map(|i| f64::from(i) / 30.0).collect();
    let mut out = Text::new();
    let mut emit = |label: String, segments: &[ZoomSegment], samples: &[CursorSample], style: &ZoomStyle| {
        out.section(&label);
        for &time in &times {
            let frame = zoom_timeline::target_at(segments, samples, time, style);
            let [x, y] = point(frame.center);
            out.row([num(time), num(frame.scale), x, y]);
        }
    };
    let (generated_name, generated) = &sets[0];
    for ratio in doubles(cases.get("snapRatios")) {
        let style = ZoomStyle { snap_to_edges_ratio: ratio, ..ZoomStyle::default() };
        emit(
            format!("segments={generated_name} samples=synthetic snapToEdgesRatio={}", num(ratio)),
            generated,
            &synthetic,
            &style,
        );
    }
    for (segments_name, segments) in &sets[1..] {
        for (samples_name, samples) in &sample_sets {
            emit(
                format!("segments={segments_name} samples={samples_name} snapToEdgesRatio=default"),
                segments,
                samples,
                &ZoomStyle::default(),
            );
        }
    }
    // C++'s default style argument is `ZoomStyle{}`.
    emit(
        format!("segments={generated_name} samples=synthetic style=<omitted>"),
        generated,
        &synthetic,
        &ZoomStyle::default(),
    );

    let expected = keep_sections(&read_expected("zoomtimeline/target-at.txt"), |label| !is_project_section(label));
    assert_same("zoomtimeline/target-at.txt (without the project: sections)", &expected, out.into_string().as_bytes());
}

#[test]
fn clamp_center_txt() {
    let cases = cases();
    let clamp = cases.get("clampCenter");
    let levels = doubles(clamp.get("levels"));
    let mut out = Text::new();
    for p in clamp.get("points").to_array() {
        let p = doubles(&p);
        let center = PointF::new(p.first().copied().unwrap_or(0.0), p.get(1).copied().unwrap_or(0.0));
        for &level in &levels {
            let [x, y] = point(center);
            let [rx, ry] = point(zoom_timeline::clamp_center(center, level));
            out.row([x, y, num(level), rx, ry]);
        }
    }
    assert_golden("zoomtimeline/clamp-center.txt", &out.into_string());
}

#[test]
fn target_to_json_txt() {
    let mut out = Text::new();
    for (name, segments) in segment_sets(&cases()) {
        for segment in &segments {
            out.row([name.clone(), quote(&segment.id), wrapped(&zoom_timeline::target_to_json(segment))]);
        }
    }
    let explicit = |id: &str, automatic_target: bool, x: f64, y: f64| ZoomSegment {
        id: id.into(),
        start: 0.0,
        end: 1.0,
        level: 2.0,
        automatic_target,
        target: PointF::new(x, y),
    };
    for segment in [
        explicit("auto-with-target", true, 0.1, 0.2),
        explicit("manual-negative-zero", false, -0.0, 1e-300),
        explicit("manual-big", false, 1e21, 123456789.125),
        explicit("manual-nan", false, f64::NAN, f64::INFINITY),
    ] {
        out.row(["explicit".into(), quote(&segment.id), wrapped(&zoom_timeline::target_to_json(&segment))]);
    }
    assert_golden("zoomtimeline/target-to-json.txt", &out.into_string());
}
