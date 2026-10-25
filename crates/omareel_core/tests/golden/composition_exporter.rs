//! composition: `export::composition::CompositionState` against the frame loop of
//! `Exporter::run` (README "composition", `exporter.txt`). Defined on x86_64 only: the state
//! carries MotionTrack's spring samples and the layout's eased transitions.

use crate::support::*;
use omareel_core::clip_timeline::ClipTimeline;
use omareel_core::export::composition::{CompositionState, Sources};
use omareel_core::input_log::InputLog;
use omareel_core::keystroke_track::KeystrokeTrack;
use omareel_core::media_probe::VideoInfo;
use omareel_core::motion_track::MotionTrack;
use omareel_core::project::Project;
use std::collections::HashMap;

/// The members of `CompositionState` the oracle printed, as the variants Qt holds them in.
fn state_cells(first: String, state: &CompositionState) -> Vec<String> {
    use Variant::*;
    let map = |entries: Vec<(&str, Variant)>| Map(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect());
    let zoom = &state.zoom;
    let cursor = &state.cursor;
    let media = &state.media;
    let ripples = List(
        state
            .ripples
            .iter()
            .map(|r| map(vec![("x", Double(r.x)), ("y", Double(r.y)), ("progress", Double(r.progress))]))
            .collect(),
    );
    let pills = List(
        state
            .keystroke_pills
            .iter()
            .map(|p| {
                map(vec![
                    ("keys", StringList(p.keys.clone())),
                    ("text", String(p.text.clone())),
                    ("opacity", Double(p.opacity)),
                ])
            })
            .collect(),
    );
    vec![
        first,
        num(state.time),
        variant(&map(vec![
            ("scale", Double(zoom.scale)),
            ("cx", Double(zoom.cx)),
            ("cy", Double(zoom.cy)),
            ("velocity", Double(zoom.velocity)),
        ])),
        variant(&map(vec![("cameraFull", Double(state.layout.camera_full)), ("bubble", Double(state.layout.bubble))])),
        variant(&map(vec![
            ("active", Bool(media.active)),
            ("ready", Bool(media.ready)),
            ("fit", String(media.fit.clone())),
            ("width", Int(media.width)),
            ("height", Int(media.height)),
        ])),
        variant(&map(vec![
            ("x", Double(cursor.x)),
            ("y", Double(cursor.y)),
            ("visible", Bool(cursor.visible)),
            ("scale", Double(cursor.scale)),
            ("opacity", Double(cursor.opacity)),
            ("rotation", Double(cursor.rotation)),
        ])),
        variant(&ripples),
        variant(&pills),
        boolean(state.camera_visible).to_owned(),
    ]
}

#[test]
fn exporter_txt() {
    let cases = read_json("composition/cases.json").to_object();
    let mut out = Text::new();
    for (name, c) in cases.iter() {
        let source_duration = c.get("sourceDuration").to_double(0.0);
        let bundle = corpus_path(&c.get("bundle").to_string_or(""));
        let input = InputLog::load_bundle(bundle.to_str().expect("UTF-8 path"), source_duration)
            .unwrap_or_else(|e| panic!("composition {name}: {e}"));
        let project = Project::from_json(&c.get("project").to_object());
        let motion = MotionTrack::build(
            source_duration,
            c.get("width").to_int(0),
            c.get("height").to_int(0),
            input.events(),
            &project,
            c.get("captureFps").to_double(0.0),
        );
        let keystrokes = KeystrokeTrack::build(input.events(), &project.keystrokes);
        let camera = c.get("camera");
        let camera_offset = camera.get("offset").to_double(0.0);
        let camera_duration = camera.get("duration").to_double(0.0);
        let mut inserts: HashMap<String, VideoInfo> = HashMap::new();
        let mut inserts_valid = true;
        for (path, value) in c.get("inserts").to_object().iter() {
            let mut info = VideoInfo::default();
            if let Some(a) = value.as_array() {
                let at = |i: usize| a.get(i).cloned().unwrap_or_default();
                info = VideoInfo {
                    width: at(0).to_int(0),
                    height: at(1).to_int(0),
                    duration: at(2).to_double(0.0),
                    fps: at(3).to_double(0.0),
                    audio: at(4).to_bool(false),
                };
            }
            inserts_valid = inserts_valid && info.is_valid();
            inserts.insert(path.to_owned(), info);
        }
        // Exporter::run refuses a project whose inserted videos don't probe, so such a case has
        // no exporter section.
        if !inserts_valid {
            continue;
        }
        let timeline = ClipTimeline::new(project.clips.clone());
        // The camera's MediaInfo is {1280, 720, duration, 30, false} with a camera, {} without.
        let (camera_width, camera_duration) = if camera.is_object() { (1280, camera_duration) } else { (0, 0.0) };
        let sources = Sources {
            timeline: &timeline,
            project: &project,
            motion: &motion,
            keystrokes: &keystrokes,
            camera_width,
            camera_duration,
            camera_offset,
            inserts: &inserts,
        };
        let mut state = CompositionState::initial(&project);
        out.section(name);
        out.row(state_cells("initial".into(), &state));
        let fps = 30;
        let total_frames = ((timeline.total_duration() * f64::from(fps)).ceil() as i32).max(1);
        for frame in 0..total_frames {
            let output_time = f64::from(frame) / f64::from(fps);
            // The decode thread picks the frame's clip with clipAt and sends its index.
            let clip = &project.clips[timeline.clip_at(output_time, None) as usize];
            state.export_frame(&sources, clip, output_time);
            out.row(state_cells(num(output_time), &state));
        }
    }
    assert_golden("composition/exporter.txt", &out.into_string());
}
