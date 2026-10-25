//! cursorpath: `cursor_path::decimate` and `position_at` against `Omareel::CursorPath`
//! (README "cursorpath"), and the synthetic recording that zoomtimeline uses too.

use crate::support::*;
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::Value;
use omareel_core::cursor_path::{self, CursorSample};
use omareel_core::input_log::InputLog;

/// The synthetic recording (640x360, 60 fps) as MotionTrack reads it.
pub fn synthetic_log() -> InputLog {
    let path = corpus_path("inputlog/bundles/synthetic");
    let log = InputLog::load_bundle(path.to_str().expect("UTF-8 path"), -1.0).expect("synthetic bundle");
    assert!(!log.events().is_empty(), "synthetic bundle has events");
    log
}

/// Its moves as samples, in pixels or divided by the capture size.
pub fn synthetic_samples(normalized: bool) -> Vec<CursorSample> {
    synthetic_log()
        .moves()
        .iter()
        .map(|event| CursorSample {
            time: event.time,
            position: if normalized {
                PointF::new(event.position.x / 640.0, event.position.y / 360.0)
            } else {
                event.position
            },
        })
        .collect()
}

pub fn synthetic_click_times() -> Vec<f64> {
    synthetic_log().click_downs(true).iter().map(|event| event.time).collect()
}

/// `[time, x, y]` triples; missing entries read as 0, like `QJsonArray::at(i).toDouble()`.
pub fn samples_from_json(array: &Value) -> Vec<CursorSample> {
    array
        .to_array()
        .iter()
        .map(|triple| {
            let triple = triple.to_array();
            let at = |i: usize| triple.get(i).map_or(0.0, |v| v.to_double(0.0));
            CursorSample { time: at(0), position: PointF::new(at(1), at(2)) }
        })
        .collect()
}

/// The oracle's `sampleIndices`: where each kept sample sits in the input.
fn sample_indices(result: &[CursorSample], raw: &[CursorSample]) -> String {
    let mut indices = Vec::new();
    let mut from = 0;
    for sample in result {
        let found = (from..raw.len()).find(|&i| {
            raw[i].time == sample.time
                && raw[i].position.x == sample.position.x
                && raw[i].position.y == sample.position.y
        });
        let Some(found) = found else { return "unmatched".into() };
        indices.push(found.to_string());
        from = found + 1;
    }
    if indices.is_empty() { "-".into() } else { indices.join(",") }
}

pub fn point(p: PointF) -> [String; 2] {
    [num(p.x), num(p.y)]
}

fn sample_sets(cases: &Value) -> Vec<(String, Vec<CursorSample>)> {
    let mut sets: Vec<(String, Vec<CursorSample>)> = cases
        .get("samples")
        .to_object()
        .iter()
        .map(|(name, samples)| (name.to_owned(), samples_from_json(samples)))
        .collect();
    sets.push(("synthetic-normalized".into(), synthetic_samples(true)));
    sets.push(("synthetic-pixels".into(), synthetic_samples(false)));
    sets
}

#[test]
fn decimate_txt() {
    let cases = read_json("cursorpath/cases.json");
    let parameters = cases.get("decimate").to_array();
    let mut out = Text::new();
    for (name, raw) in sample_sets(&cases) {
        for parameter in &parameters {
            let parameter = parameter.to_array();
            let fps = parameter.first().map_or(0.0, |v| v.to_double(0.0));
            let min_distance = parameter.get(1).map_or(0.0, |v| v.to_double(0.0));
            let result = cursor_path::decimate(&raw, fps, min_distance);
            out.row([
                name.clone(),
                num(fps),
                num(min_distance),
                result.len().to_string(),
                sample_indices(&result, &raw),
            ]);
        }
        // C++'s default argument: minDistance = 1.0.
        let result = cursor_path::decimate(&raw, 60.0, 1.0);
        out.row([name.clone(), num(60.0), "default".into(), result.len().to_string(), sample_indices(&result, &raw)]);
    }
    assert_golden("cursorpath/decimate.txt", &out.into_string());
}

#[test]
fn position_at_txt() {
    let cases = read_json("cursorpath/cases.json");
    let queries: Vec<f64> = cases.get("positionAt").to_array().iter().map(|v| v.to_double(0.0)).collect();
    let grid: Vec<f64> = (-12..=972).map(|i| f64::from(i) / 120.0).collect();
    let mut out = Text::new();
    for (name, raw) in sample_sets(&cases) {
        let times = if name.starts_with("synthetic") { &grid } else { &queries };
        for &time in times {
            let [x, y] = point(cursor_path::position_at(&raw, time));
            out.row([name.clone(), num(time), x, y]);
        }
    }
    // MotionTrack's input: decimated at the capture rate with a one-pixel threshold.
    let decimated = cursor_path::decimate(&synthetic_samples(true), 60.0, 1.0 / 640.0);
    for &time in &grid {
        let [x, y] = point(cursor_path::position_at(&decimated, time));
        out.row(["synthetic-normalized-decimated".into(), num(time), x, y]);
    }
    assert_golden("cursorpath/position-at.txt", &out.into_string());
}
