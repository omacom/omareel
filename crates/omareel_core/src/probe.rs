//! `omareel probe <bundle>`: a JSON summary of a recording (`probeCommand` in main.cpp).

use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::{qnumber, qprocess};
use crate::input_log::InputLog;
use crate::zoom_timeline;
use std::path::Path;

/// ffprobe's view of a video's first stream and duration, or an empty object.
fn video_info(path: &str) -> Object {
    let args = [
        "-v", "error", "-select_streams", "v:0", "-show_entries",
        "stream=width,height,r_frame_rate:format=duration", "-of", "json", path,
    ];
    match qprocess::run("ffprobe", &args, 10_000) {
        Some(done) if done.succeeded() => qjson::object_from_json(&done.stdout).unwrap_or_default(),
        _ => Object::new(),
    }
}

/// A rate such as "30000/1001", or a plain number.
fn fraction(text: &str) -> f64 {
    let parts: Vec<&str> = text.split('/').collect();
    let number = |s: &str| qnumber::to_double(s).unwrap_or(0.0);
    if parts.len() == 2 && number(parts[1]) != 0.0 {
        return number(parts[0]) / number(parts[1]);
    }
    number(text)
}

/// The summary `probe` prints, or the usage error it reports. `bundle` is absolute.
pub fn run(bundle: &str) -> Result<String, String> {
    if !Path::new(bundle).is_dir() {
        return Err(format!("bundle does not exist: {bundle}"));
    }
    let info = video_info(&format!("{bundle}/screen.mp4"));
    let streams = info.value("streams").to_array();
    let Some(stream) = streams.first().map(Value::to_object) else {
        return Err("screen.mp4 could not be probed".into());
    };
    let duration = qnumber::to_double(&info.value("format").get("duration").to_string_or("")).unwrap_or(0.0);
    let input = InputLog::load_bundle(bundle, duration)?;
    let click_times: Vec<f64> = input.click_downs(true).iter().map(|click| click.time).collect();
    let zooms = zoom_timeline::generate(&click_times, duration);
    let zoom_json: Vec<Value> = zooms
        .iter()
        .map(|zoom| {
            Object::from_iter([
                ("id", Value::from(zoom.id.as_str())),
                ("start", zoom.start.into()),
                ("end", zoom.end.into()),
                ("level", zoom.level.into()),
                ("target", zoom_timeline::target_to_json(zoom)),
            ])
            .into()
        })
        .collect();
    let metadata = crate::compat::qfile::read_all(format!("{bundle}/capture.json"))
        .ok()
        .and_then(|bytes| qjson::object_from_json(&bytes))
        .unwrap_or_default();
    let size = Object::from_iter([("width", stream.value("width").to_int(0)), ("height", stream.value("height").to_int(0))]);
    let mut output = Object::from_iter([
        ("overlay_exclusion", Value::from(metadata.value("overlay_exclusion").to_string_or("fallback"))),
        ("duration", duration.into()),
        ("fps", fraction(&stream.value("r_frame_rate").to_string_or("")).into()),
        ("size", size.into()),
        ("sample_count", (input.moves().len() as i64).into()),
        ("click_count", (input.click_downs(false).len() as i64).into()),
        ("key_count", (input.key_count() as i64).into()),
        ("generated_zoom_count", (zooms.len() as i64).into()),
        ("zooms", zoom_json.into()),
    ]);
    if Path::new(&format!("{bundle}/camera.mp4")).exists() {
        output.insert("camera", video_info(&format!("{bundle}/camera.mp4")));
    }
    Ok(output.to_json(Format::Indented))
}
