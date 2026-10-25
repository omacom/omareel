//! motiontrack: `motion_track::MotionTrack` against `Omareel::MotionTrack` (README
//! "motiontrack"). Defined on x86_64 only: thousands of 240 Hz spring steps, glibc's `hypot`,
//! and Easing's `pow` through the zoom targets.

use crate::support::*;
use crate::zoomtimeline::segments_from_json;
use omareel_core::compat::qjson::Value;
use omareel_core::input_log::InputLog;
use omareel_core::motion_track::MotionTrack;
use omareel_core::project::{Project, Spring};
use omareel_core::zoom_timeline;

/// `[mass, stiffness, damping]`; a missing entry reads as 0, like `QJsonArray::at(i).toDouble()`.
fn spring_from_array(value: &Value) -> Spring {
    let a = value.to_array();
    let at = |i: usize| a.get(i).map_or(0.0, |v| v.to_double(0.0));
    Spring { mass: at(0), stiffness: at(1), damping: at(2) }
}

/// The oracle's `applyOverride`: a `set` entry written straight into the project's field.
fn apply_override(p: &mut Project, key: &str, v: &Value) {
    match key {
        "cursor.visible" => p.cursor.visible = v.to_bool(false),
        "cursor.smoothing" => p.cursor.smoothing = v.to_bool(false),
        "cursor.spring" => p.cursor.spring = spring_from_array(v),
        "cursor.clickEffect" => p.cursor.click_effect = v.to_string_or(""),
        "cursor.clickShrink" => p.cursor.click_shrink = v.to_double(0.0),
        "cursor.rotateOnXMovementRatio" => p.cursor.rotate_on_x_movement_ratio = v.to_double(0.0),
        "cursor.hideWhenIdleMs" => p.cursor.hide_when_idle_ms = v.to_int(0),
        "zoomStyle.spring" => p.zoom_style.spring = spring_from_array(v),
        "zoomStyle.instantAnimation" => p.zoom_style.instant_animation = v.to_bool(false),
        "zoomStyle.motionBlur" => p.zoom_style.motion_blur = v.to_double(0.0),
        "zoomStyle.snapToEdgesRatio" => p.zoom_style.snap_to_edges_ratio = v.to_double(0.0),
        _ => panic!("motiontrack/cases.json: unknown setting {key}"),
    }
}

/// The oracle's `motionCells`: the `sample(t)` fields, then the ripples.
fn motion_row(track: &MotionTrack, t: f64) -> Vec<String> {
    let s = track.sample(t);
    let mut cells: Vec<String> = [
        t,
        s.zoom_scale,
        s.zoom_cx,
        s.zoom_cy,
        s.zoom_center_velocity,
        s.cursor_x,
        s.cursor_y,
        s.cursor_scale,
        s.cursor_opacity,
        s.cursor_rotation,
    ]
    .into_iter()
    .map(num)
    .collect();
    let ripples = track.ripples(t);
    cells.push(ripples.len().to_string());
    for r in &ripples {
        cells.extend([num(r.x), num(r.y), num(r.progress)]);
    }
    cells
}

#[test]
fn sample_txt() {
    let cases = read_json("motiontrack/cases.json").to_object();
    let mut out = Text::new();
    for (name, c) in cases.iter() {
        let input = corpus_path(&c.get("input").to_string_or(""));
        let duration = c.get("duration").to_double(0.0);
        // A bundle is read the way the editor reads it (with the recording's duration); a log
        // file with first frame 1000000 us and no duration cut.
        let log = if input.is_dir() {
            InputLog::load_bundle(input.to_str().expect("UTF-8 path"), duration)
        } else {
            InputLog::load(&input, 1_000_000, -1.0)
        }
        .unwrap_or_else(|e| panic!("motiontrack {name}: {e}"));

        let mut project = Project::defaults("Motion", duration);
        let zooms = c.get("zooms");
        project.zooms = if zooms.to_string_or("") == "generate" {
            let clicks: Vec<f64> = log.click_downs(true).iter().map(|click| click.time).collect();
            zoom_timeline::generate(&clicks, duration)
        } else {
            segments_from_json(zooms)
        };
        for (key, value) in c.get("set").to_object().iter() {
            apply_override(&mut project, key, value);
        }

        let track = MotionTrack::build(
            duration,
            c.get("width").to_int(0),
            c.get("height").to_int(0),
            log.events(),
            &project,
            c.get("captureFps").to_double(0.0),
        );
        out.section(name);
        out.row(["duration".into(), num(track.duration())]);
        out.row(["size".into(), track.size().to_string()]);
        let end = track.duration();
        let mut times = uniform(50, -5, (50.0 * end).ceil() as i32 + 5);
        times.extend([before(end), end, after(end)]);
        for click in log.click_downs(false) {
            for edge in [click.time, click.time + 0.45] {
                times.extend([before(edge), edge, after(edge)]);
            }
        }
        let mut times = sorted_unique(times);
        times.extend([f64::NEG_INFINITY, f64::INFINITY]);
        for t in times {
            out.row(motion_row(&track, t));
        }
    }
    out.section("default-constructed");
    let empty = MotionTrack::default();
    out.row(["duration".into(), num(empty.duration())]);
    out.row(["size".into(), empty.size().to_string()]);
    for t in [-1.0, 0.0, 0.5, f64::INFINITY] {
        out.row(motion_row(&empty, t));
    }
    assert_golden("motiontrack/sample.txt", &out.into_string());
}
