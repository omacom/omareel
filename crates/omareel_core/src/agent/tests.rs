//! `omareel apply` and `describe` over a bundle on disk, and the editor taking in a project.json
//! written by someone else.

use super::apply::{apply_ops, parse_ops};
use super::describe::describe;
use crate::compat::qdir::tests::TempDir;
use crate::compat::qjson::{Object, Value};
use crate::editor::{DiskChange, Document, Locations, MediaInfo, Probes, VideoInfo};
use std::sync::Arc;

fn fixture(file: &str) -> String {
    format!("{}/../../tests/fixtures/synthetic/{file}", env!("CARGO_MANIFEST_DIR"))
}

/// A 10 s, 640x360 recording (the fake screen.mp4 is just bytes) and the fixture's input log.
fn probes() -> Probes {
    Probes {
        media: Arc::new(|path: &str| {
            if std::path::Path::new(path).is_file() {
                Ok(MediaInfo { width: 640, height: 360, duration: 10.0, fps: 60.0, audio: true })
            } else {
                Err(format!("{path}: No such file or directory"))
            }
        }),
        video: Arc::new(|_: &str| Ok(VideoInfo { width: 320, height: 240, duration: 2.0, fps: 30.0, audio: false })),
    }
}

fn bundle(dir: &TempDir) -> String {
    let bundle = dir.join("Test.omareel");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::copy(fixture("capture.json"), format!("{bundle}/capture.json")).unwrap();
    std::fs::copy(fixture("input.jsonl"), format!("{bundle}/input.jsonl")).unwrap();
    std::fs::write(format!("{bundle}/screen.mp4"), b"screen").unwrap();
    bundle
}

fn open(bundle: &str, dir: &TempDir) -> Document {
    let locations = Locations { config: Some(dir.join("config")), cache: Some(dir.join("cache")) };
    let document = Document::open_with(bundle, &probes(), locations).unwrap();
    document.take_effects();
    document
}

fn ops(json: &str) -> Vec<Object> {
    parse_ops(json.as_bytes()).unwrap()
}

fn spans(document: &Document) -> Vec<(f64, f64, f64)> {
    document.project().clips.iter().map(|clip| (clip.in_, clip.out, clip.speed)).collect()
}

#[test]
fn operations_edit_like_the_editor() {
    let dir = TempDir::new("agent-apply");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    let (results, warnings) = apply_ops(&mut document, &ops(r#"[
            {"op": "clear-zooms"},
            {"op": "split", "at": 3},
            {"op": "delete", "from": 4, "to": 6},
            {"op": "speed", "at": 5, "speed": 2},
            {"op": "zoom", "from": 1, "to": 2.5, "level": 2.5, "target": {"x": 0.2, "y": 0.3}},
            {"op": "set", "path": "background.blur", "value": 0.1},
            {"op": "set", "path": "background.nonsense", "value": 1}
        ]"#), false)
    .unwrap();
    assert_eq!(spans(&document), vec![(0.0, 3.0, 1.0), (3.0, 4.0, 1.0), (6.0, 10.0, 2.0)]);
    assert_eq!(document.duration(), 6.0);
    let zoom = &document.project().zooms.iter().find(|zoom| zoom.level == 2.5).unwrap();
    assert!((zoom.start - 1.0).abs() < 1e-3 && (zoom.end - 2.5).abs() < 1e-3, "{zoom:?}");
    assert!(!zoom.automatic_target);
    assert_eq!(document.project().background.blur, 0.1);
    assert_eq!(results.len(), 7);
    let error = apply_ops(&mut document, &ops(r#"{"op": "zoom", "from": 2, "to": 3}"#), false).unwrap_err();
    assert!(error.contains("zooms can't overlap; edit or remove z-"), "{error}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].starts_with("set background.nonsense"));
}

#[test]
fn source_times_stay_put_across_cuts() {
    let dir = TempDir::new("agent-source");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    apply_ops(&mut document, &ops(r#"[
            {"op": "delete", "from": 1, "to": 2, "timeline": "source"},
            {"op": "delete", "from": 5, "to": 7, "timeline": "source"},
            {"op": "split", "at": 8, "timeline": "source"}
        ]"#), false)
    .unwrap();
    assert_eq!(spans(&document), vec![(0.0, 1.0, 1.0), (2.0, 5.0, 1.0), (7.0, 8.0, 1.0), (8.0, 10.0, 1.0)]);
}

#[test]
fn a_failing_operation_says_which_and_why() {
    let dir = TempDir::new("agent-fail");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    let error = apply_ops(&mut document, &ops(r#"[{"op": "split", "at": 5}, {"op": "speed", "at": 1, "speed": 1.5}]"#), false)
        .unwrap_err();
    assert!(error.starts_with("operation 2 (speed): speed must be one of 0.5, 0.75, 1,"), "{error}");
    let error = apply_ops(&mut document, &ops(r#"{"op": "delete", "from": 0, "to": 10}"#), false).unwrap_err();
    assert!(error.contains("remove all of the recording"), "{error}");
    let error = apply_ops(&mut document, &ops(r#"{"op": "split", "at": 99}"#), false).unwrap_err();
    assert!(error.contains("outside the video"), "{error}");
    assert!(parse_ops(b"[1]").is_err());
}

#[test]
fn run_saves_only_when_everything_applied() {
    let dir = TempDir::new("agent-run");
    let bundle = bundle(&dir);
    let project = format!("{bundle}/project.json");
    let run = |ops: &[u8], dry_run: bool| super::apply::run_on(|| Ok(open(&bundle, &dir)), ops, dry_run, true);
    let (report, ok) = run(br#"[{"op": "split", "at": 2}, {"op": "bogus"}]"#, false);
    assert!(!ok && report.contains("operation 2 (bogus): unknown op"), "{report}");
    assert!(!std::path::Path::new(&project).exists());
    let (report, ok) = run(br#"[{"op": "split", "at": 2}]"#, true);
    assert!(ok && report.contains("\"saved\": false"), "{report}");
    assert!(!std::path::Path::new(&project).exists());
    let (report, ok) = run(br#"{"ops": [{"op": "split", "at": 2}]}"#, false);
    assert!(ok && report.contains("\"saved\": true") && report.contains("\"describe\""), "{report}");
    assert!(std::path::Path::new(&project).exists());
}

#[test]
fn describe_maps_activity_onto_the_edited_timeline() {
    let dir = TempDir::new("agent-describe");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    apply_ops(&mut document, &ops(r#"{"op": "delete", "from": 0, "to": 1}"#), false).unwrap();
    let summary = Value::Object(describe(&document));
    assert_eq!(summary.get("output").get("duration").to_double(0.0), 9.0);
    let clips = summary.get("clips").to_array();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].get("source").get("start").to_double(-1.0), 1.0);
    for click in summary.get("activity").get("clicks").to_array() {
        let source = click.get("source").to_double(-1.0);
        let output = click.get("output").to_array();
        if source < 1.0 {
            assert!(output.is_empty());
        } else {
            assert!((output[0].to_double(-1.0) - (source - 1.0)).abs() < 2e-3);
        }
        let x = click.get("position").get("x").to_double(-1.0);
        assert!((0.0..=1.0).contains(&x));
    }
    assert!(summary.get("settings").get("clips").is_undefined());
    assert!(summary.get("settings").get("background").is_object());
}

#[test]
fn an_open_editor_takes_in_a_project_written_elsewhere() {
    let dir = TempDir::new("agent-reload");
    let bundle = bundle(&dir);
    let mut editor = open(&bundle, &dir);
    assert!(editor.save_now());
    editor.take_effects();
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Unchanged, "its own save is not a change");

    // An unsaved edit in the editor, then another program's edit on disk.
    editor.set_project_value("background.blur", Value::from(0.05), false);
    editor.take_effects();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut other = open(&bundle, &dir);
    apply_ops(&mut other, &ops(r#"{"op": "split", "at": 4}"#), false).unwrap();
    assert!(other.save_now());

    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Reloaded);
    assert_eq!(editor.project().clips.len(), 2);
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Unchanged, "taken in once");
    // The editor's own state before the reload is one undo away.
    editor.undo();
    assert_eq!(editor.project().clips.len(), 1);
    assert_eq!(editor.project().background.blur, 0.05);
    editor.redo();
    assert_eq!(editor.project().clips.len(), 2);

    // A half-written file is ignored until it parses.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(format!("{bundle}/project.json"), b"{\"clips\": [").unwrap();
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Unreadable);
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Unreadable, "read again until it parses");
    assert!(editor.disk_changed());

    // What opening corrects is corrected on a reload too: a timeline of inserted videos only
    // gets the recording back, and audio the recording lacks (the fixture has none) stays off.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut project = editor.project().to_json();
    let clips = Value::array(vec![Value::Object(Object::from_iter([
        ("id", Value::from("m1")),
        ("in", 0.0.into()),
        ("out", 2.0.into()),
        ("speed", 1.0.into()),
        ("media", "media/x.mp4".into()),
    ]))]);
    project.insert("clips", clips);
    let mut audio = project.value("audio").to_object();
    audio.insert("desktop", true);
    project.insert("audio", audio);
    std::fs::write(format!("{bundle}/project.json"), Value::Object(project).to_json(crate::compat::qjson::Format::Indented)).unwrap();
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Reloaded);
    let ids: Vec<&str> = editor.project().clips.iter().map(|clip| clip.id.as_str()).collect();
    assert_eq!(ids, ["c1", "m1"]);
    assert!(!editor.project().audio.desktop);
}

#[test]
fn music_is_added_set_and_removed() {
    let dir = TempDir::new("agent-music");
    let bundle = bundle(&dir);
    let song = dir.join("song.mp3");
    std::fs::write(&song, b"song").unwrap();
    let open_with_audio = || {
        let mut document = open(&bundle, &dir);
        document.set_audio_probe(Arc::new(|_: &str| Ok(4.0)));
        document
    };
    let add = format!(r#"{{"op": "music", "file": "{song}", "volume": 0.4, "loop": false}}"#);
    let mut document = open_with_audio();
    assert!(apply_ops(&mut document, &ops(r#"{"op": "music", "volume": 0.4}"#), false).unwrap_err().contains("no music yet"));
    apply_ops(&mut document, &ops(&add), true).unwrap();
    assert_eq!(document.music().unwrap().file, "media/song.mp3");
    assert!(!std::path::Path::new(&format!("{bundle}/media/song.mp3")).exists(), "a dry run copies nothing");
    let mut document = open_with_audio();
    apply_ops(&mut document, &ops(&add), false).unwrap();
    let music = document.music().unwrap();
    assert_eq!((music.volume, music.loop_), (0.4, false));
    assert!(std::path::Path::new(&format!("{bundle}/media/song.mp3")).exists());
    let summary = Value::Object(describe(&document));
    assert_eq!(summary.get("music").get("duration").to_double(0.0), 4.0);
    assert!(apply_ops(&mut document, &ops(r#"{"op": "music", "duck": "yes"}"#), false).is_err());
    apply_ops(&mut document, &ops(r#"{"op": "remove-music"}"#), false).unwrap();
    assert!(document.music().is_none());
    assert!(apply_ops(&mut document, &ops(r#"{"op": "remove-music"}"#), false).is_err());
}

#[test]
fn zoom_edges_levels_and_ranges() {
    let dir = TempDir::new("agent-zoom");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    // Cut source 4–6, so output 4 is where two clips meet.
    apply_ops(&mut document, &ops(r#"[{"op": "clear-zooms"}, {"op": "delete", "from": 4, "to": 6}, {"op": "zoom", "from": 1, "to": 3}]"#), false).unwrap();
    let id = document.project().zooms[0].id.clone();
    let edit = format!(r#"{{"op": "edit-zoom", "id": "{id}", "to": 4}}"#);
    apply_ops(&mut document, &ops(&edit), false).unwrap();
    let zoom = &document.project().zooms[0];
    assert!((zoom.end - 4.0).abs() < 1e-3, "the zoom stays before the cut: {zoom:?}");
    apply_ops(&mut document, &ops(r#"{"op": "zoom", "from": 4, "to": 5.5}"#), false).unwrap();
    for (json, expected) in [
        (r#"{"op": "zoom", "from": 6, "to": 6.5}"#, "at least a second"),
        (r#"{"op": "zoom", "from": 6, "to": 7.5, "level": 6}"#, "level"),
        (r#"{"op": "zoom", "from": 7, "to": 9}"#, "outside the video"),
        (r#"{"op": "delete", "from": 7, "to": 9}"#, "outside the video"),
    ] {
        let error = apply_ops(&mut document, &ops(json), false).unwrap_err();
        assert!(error.contains(expected), "{json}: {error}");
    }
}

#[test]
fn inserts_copy_nothing_on_a_dry_run_or_a_failed_batch() {
    let dir = TempDir::new("agent-insert");
    let bundle = bundle(&dir);
    let video = dir.join("clip.mp4");
    std::fs::write(&video, b"video").unwrap();
    let insert = format!(r#"[{{"op": "insert", "file": "{video}", "at": 2}}, {{"op": "bogus"}}]"#);
    let mut document = open(&bundle, &dir);
    apply_ops(&mut document, &ops(&insert)[..1], true).unwrap();
    assert_eq!(document.project().clips.iter().filter(|clip| clip.is_media()).count(), 1);
    assert!(!std::path::Path::new(&format!("{bundle}/media/clip.mp4")).exists());
    let mut document = open(&bundle, &dir);
    assert!(apply_ops(&mut document, &ops(&insert), false).is_err());
    assert!(!std::path::Path::new(&format!("{bundle}/media/clip.mp4")).exists());
    let mut document = open(&bundle, &dir);
    apply_ops(&mut document, &ops(&insert)[..1], false).unwrap();
    assert!(std::path::Path::new(&format!("{bundle}/media/clip.mp4")).exists());
}
