//! composition/editor.txt: `editor::Document`'s preview composition against Editor's preview
//! getters (README "composition"). Defined on x86_64 only, as the motion track it samples is.

use crate::support::*;
use omareel_core::compat::qjson::Value;
use omareel_core::editor::{CameraRecording, Document, Recording, VideoInfo};
use omareel_core::input_log::InputLog;
use omareel_core::motion_track::MotionTrack;
use omareel_core::project::Project;
use std::collections::HashMap;
use std::sync::Arc;

/// An insert's `[width, height, duration, fps, audio]`, or a default `VideoInfo` for null, read
/// with `QJsonArray::at(i).toInt()` and friends.
fn video_info(value: &Value) -> VideoInfo {
    let Some(a) = value.as_array() else { return VideoInfo::default() };
    let at = |i: usize| a.get(i).cloned().unwrap_or_default();
    VideoInfo {
        width: at(0).to_int(0),
        height: at(1).to_int(0),
        duration: at(2).to_double(0.0),
        fps: at(3).to_double(0.0),
        audio: at(4).to_bool(false),
    }
}

fn map(entries: Vec<(&str, Variant)>) -> Variant {
    Variant::Map(entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect())
}

#[test]
fn editor_txt() {
    use Variant::*;
    let cases = read_json("composition/cases.json").to_object();
    let mut out = Text::new();
    for (name, c) in cases.iter() {
        let source_duration = c.get("sourceDuration").to_double(0.0);
        let bundle = corpus_path(&c.get("bundle").to_string_or(""));
        let input = InputLog::load_bundle(bundle.to_str().expect("UTF-8 path"), source_duration)
            .unwrap_or_else(|e| panic!("composition {name}: {e}"));
        let project = Project::from_json(&c.get("project").to_object());
        let camera = c.get("camera").as_object().map(|camera| CameraRecording {
            offset: camera.value("offset").to_double(0.0),
            duration: camera.value("duration").to_double(0.0),
            ..CameraRecording::default()
        });
        let inserts: HashMap<std::string::String, VideoInfo> =
            c.get("inserts").to_object().iter().map(|(path, info)| (path.to_owned(), video_info(info))).collect();
        let recording = Recording {
            bundle_path: bundle.to_str().expect("UTF-8 path").to_owned(),
            source_duration,
            source_width: c.get("width").to_int(0),
            source_height: c.get("height").to_int(0),
            fps: c.get("captureFps").to_double(0.0),
            camera,
            ..Recording::default()
        };
        let mut document =
            Document::new(recording, project.clone(), input.clone(), inserts, Arc::new(|_: &str| Ok(VideoInfo::default())));
        if c.get("motion").to_bool(false) {
            document.apply_motion(MotionTrack::build(
                source_duration,
                c.get("width").to_int(0),
                c.get("height").to_int(0),
                input.events(),
                &project,
                c.get("captureFps").to_double(0.0),
            ));
        }
        let media_frame_ready = c.get("mediaFrameReady").to_bool(false);
        let media_player_source = c.get("mediaPlayerSource").to_string_or("");

        let duration = document.duration();
        let mut positions = uniform(30, -3, (duration * 30.0).ceil() as i32 + 3);
        let mut start = 0.0;
        positions.extend([before(start), start, after(start)]);
        for clip in &project.clips {
            start += (clip.out - clip.in_) / clip.speed;
            positions.extend([before(start), start, after(start)]);
        }
        out.section(name);
        for position in sorted_unique(positions) {
            document.follow_output_position(position);
            let z = document.preview_zoom();
            let l = document.preview_layout();
            let m = document.preview_media(media_frame_ready, &media_player_source);
            let cursor = document.preview_cursor();
            let ripples = document
                .preview_ripples()
                .iter()
                .map(|r| map(vec![("x", Double(r.x)), ("y", Double(r.y)), ("progress", Double(r.progress))]))
                .collect();
            let pills = document
                .preview_keystroke_pills()
                .into_iter()
                .map(|p| map(vec![("keys", StringList(p.keys)), ("text", String(p.text)), ("opacity", Double(p.opacity))]))
                .collect();
            out.row([
                num(position),
                num(document.source_position()),
                variant(&map(vec![("scale", Double(z.scale)), ("cx", Double(z.cx)), ("cy", Double(z.cy)), ("velocity", Double(z.velocity))])),
                variant(&map(vec![("cameraFull", Double(l.camera_full)), ("bubble", Double(l.bubble))])),
                variant(&map(vec![
                    ("active", Bool(m.active)),
                    ("ready", Bool(m.ready)),
                    ("fit", String(m.fit)),
                    ("width", Int(m.width)),
                    ("height", Int(m.height)),
                    ("missing", Bool(m.missing)),
                ])),
                variant(&map(vec![
                    ("x", Double(cursor.x)),
                    ("y", Double(cursor.y)),
                    ("visible", Bool(cursor.visible)),
                    ("scale", Double(cursor.scale)),
                    ("opacity", Double(cursor.opacity)),
                    ("rotation", Double(cursor.rotation)),
                ])),
                variant(&List(ripples)),
                variant(&List(pills)),
                boolean(document.camera_visible()).into(),
            ]);
        }
    }
    assert_golden("composition/editor.txt", &out.into_string());
}
