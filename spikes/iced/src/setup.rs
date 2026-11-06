//! Finishes a bundle from `spikes/make-bundle.sh` into the one SPEC.md describes, through
//! omareel_core's `Document` as the editor would: two zooms (auto target), the round
//! camera bubble with its border and 45° shadow (the defaults), the wallpaper, keystroke pills,
//! and a middle clip with the camera's "square + blur" full-frame layout.
//!
//! The synthetic input log has no key presses, so three shortcuts are added to the bundle's copy.

use omareel_core::compat::qjson::{Object, Value};
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use std::sync::Arc;

/// (seconds into the recording, key code, key name, modifiers)
const SHORTCUTS: [(f64, i32, &str, &[&str]); 3] = [
    (1.2, 25, "KEY_P", &["ctrl", "shift"]),
    (7.0, 31, "KEY_S", &["ctrl"]),
    (8.6, 15, "KEY_TAB", &["alt"]),
];

fn add_shortcuts(bundle: &str) -> Result<(), String> {
    let path = format!("{bundle}/input.jsonl");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    if text.contains("\"kd\"") {
        return Ok(());
    }
    let capture = std::fs::read(format!("{bundle}/capture.json")).map_err(|e| e.to_string())?;
    let first = omareel_core::compat::qjson::object_from_json(&capture)
        .unwrap_or_default()
        .value("first_frame_us")
        .to_variant_long_long();
    let mut lines: Vec<(i64, String)> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let t = omareel_core::compat::qjson::object_from_json(line.as_bytes()).unwrap_or_default().value("t").to_variant_long_long();
            (t, line.to_owned())
        })
        .collect();
    for (seconds, code, name, mods) in SHORTCUTS {
        let t = first + (seconds * 1e6) as i64;
        let mods: Vec<String> = mods.iter().map(|m| format!("\"{m}\"")).collect();
        lines.push((t, format!("{{\"t\":{t},\"k\":\"kd\",\"code\":{code},\"name\":\"{name}\",\"mods\":[{}]}}", mods.join(","))));
        lines.push((t + 90_000, format!("{{\"t\":{},\"k\":\"ku\",\"code\":{code}}}", t + 90_000)));
    }
    lines.sort_by_key(|(t, _)| *t);
    let mut out = String::new();
    for (_, line) in lines {
        out.push_str(&line);
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|e| format!("{path}: {e}"))
}

fn zoom(id: &str, start: f64, end: f64, level: f64) -> Value {
    let mut zoom = Object::new();
    zoom.insert("id", id);
    zoom.insert("start", start);
    zoom.insert("end", end);
    zoom.insert("level", level);
    zoom.insert("target", "auto");
    zoom.into()
}

fn clip(id: &str, from: f64, to: f64, layout: &str) -> Value {
    let mut clip = Object::new();
    clip.insert("id", id);
    clip.insert("in", from);
    clip.insert("out", to);
    clip.insert("speed", 1.0);
    if layout != "default" {
        clip.insert("layout", layout);
    }
    clip.into()
}

pub fn run(bundle: &str) -> Result<(), String> {
    add_shortcuts(bundle)?;
    let project = format!("{bundle}/project.json");
    let _ = std::fs::remove_file(&project);
    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(bundle, &probes)?;
    let duration = document.source_duration();
    let wallpaper = omareel_core::omarchy_paths::current_background();
    document.set_project_value("background.type", "wallpaper".into(), false);
    if !wallpaper.is_empty() {
        document.set_project_value("background.wallpaper", wallpaper.clone().into(), false);
    }
    document.set_project_value("keystrokes.enabled", true.into(), false);
    document.set_project_value("camera.enabled", true.into(), false);
    document.set_project_value("camera.shape", "round".into(), false);
    document.set_project_value("camera.border.enabled", true.into(), false);
    document.set_project_value("camera.shadow.enabled", true.into(), false);
    document.set_project_value("camera.shadow.angle", 45.0.into(), false);
    document.set_project_value("camera.fullFrame", "square".into(), false);
    // The generated zoom spans 1.2 s to 9.2 s; two that follow the cursor in and out show the
    // spring both ways, one either side of the camera clip.
    document.set_project_value("zooms", Value::array(vec![zoom("z1", 1.2, 3.7, 2.0), zoom("z2", 6.8, 9.2, 1.8)]), false);
    document.set_project_value(
        "clips",
        Value::array(vec![clip("c1", 0.0, 4.0, "default"), clip("c2", 4.0, 6.5, "camera"), clip("c3", 6.5, duration, "default")]),
        false,
    );
    if !document.save_now() {
        return Err(format!("could not save {project}"));
    }
    let p = document.project();
    println!(
        "{project}: {} zooms, {} clips, camera {} ({}, border {}, shadow {}°, full frame {}), wallpaper {}, keystrokes {}",
        p.zooms.len(),
        p.clips.len(),
        p.camera.enabled,
        p.camera.shape,
        p.camera.border.enabled,
        p.camera.shadow.angle,
        p.camera.full_frame,
        if wallpaper.is_empty() { "(none)" } else { &wallpaper },
        p.keystrokes.enabled
    );
    Ok(())
}
