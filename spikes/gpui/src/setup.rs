//! `omareel-gpui setup <bundle>`: the project.json SPEC.md asks for, made through the editor's
//! own document so it is what the Qt editor would have saved: the zooms a new project generates
//! from the clicks, a round camera bubble with its border and 45° shadow (the defaults), the
//! current wallpaper as the background, keystroke pills, and a middle clip (6 s to 8 s) with the
//! camera's full-frame layout as a centred square over a blurred copy.
//!
//! The shared input log has no key presses, so three shortcuts are appended to the bundle's
//! copy first; otherwise no keystroke pill would ever show.

use omareel_core::compat::qjson::Value;
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use std::io::Write;
use std::sync::Arc;

const KEYS: [(f64, &str, &[&str]); 3] =
    [(1.5, "KEY_P", &["ctrl", "shift"]), (4.2, "KEY_S", &["ctrl"]), (8.6, "KEY_Z", &["ctrl"])];

pub fn run(bundle: &str) -> Result<(), String> {
    let input = format!("{bundle}/input.jsonl");
    let log = std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?;
    if !log.contains("\"kd\"") {
        let mut file = std::fs::OpenOptions::new().append(true).open(&input).map_err(|e| e.to_string())?;
        // The fixture's first frame is at 1 s of its clock.
        for (time, name, mods) in KEYS {
            let t = 1_000_000 + (time * 1e6) as i64;
            let mods: Vec<String> = mods.iter().map(|m| format!("\"{m}\"")).collect();
            for (kind, at) in [("kd", t), ("ku", t + 150_000)] {
                writeln!(file, "{{\"t\":{at},\"k\":\"{kind}\",\"code\":25,\"name\":\"{name}\",\"mods\":[{}]}}", mods.join(","))
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    let _ = std::fs::remove_file(format!("{bundle}/project.json"));
    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(bundle, &probes)?;
    let wallpaper = omareel_core::omarchy_paths::current_background();
    document.set_project_value("background.type", Value::from("wallpaper"), false);
    if !wallpaper.is_empty() {
        // Spelled out, so a nested compositor with its own config directory shows the same.
        document.set_project_value("background.wallpaper", Value::from(wallpaper.as_str()), false);
    }
    document.set_project_value("keystrokes.enabled", Value::from(true), false);
    document.set_project_value("camera.enabled", Value::from(true), false);
    document.set_project_value("camera.fullFrame", Value::from("square"), false);
    document.set_output_position(6.0);
    document.split_at_playhead();
    let middle = document.selected_clip_id().to_owned();
    document.set_output_position(8.0);
    document.split_at_playhead();
    if !document.set_clip_layout(&middle, "camera") {
        return Err("could not give the middle clip the camera layout".into());
    }
    if !document.save_now() {
        return Err("could not save project.json".into());
    }
    println!(
        "{bundle}: {} zooms, clips {:?}, wallpaper {wallpaper}",
        document.project().zooms.len(),
        document.project().clips.iter().map(|c| (c.in_, c.out, c.layout.clone())).collect::<Vec<_>>()
    );
    Ok(())
}
