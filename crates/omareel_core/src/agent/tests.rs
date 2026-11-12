//! `omareel apply` and `describe` over a bundle on disk, and the editor taking in a project.json
//! written by someone else.

use super::apply::{apply_ops, parse_ops};
use super::describe::describe;
use crate::compat::qdir::tests::TempDir;
use crate::compat::qjson::{Object, Value};
use crate::editor::{DiskChange, Document, Effect, Locations, MediaInfo, Probes, VideoInfo};
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

fn word(start: f64, end: f64, text: &str) -> crate::transcript::Word {
    crate::transcript::Word { start, end, text: text.into() }
}

fn transcript(timing: crate::transcript::WordTiming) -> crate::transcript::Transcript {
    let words = vec![word(1.0, 1.4, "Hello"), word(1.45, 1.7, "um"), word(1.75, 2.2, "world.")];
    crate::transcript::Transcript {
        backend: "whisper-cpp".into(),
        word_timing: timing,
        segments: vec![crate::transcript::Segment { start: 1.0, end: 2.2, text: "Hello um world.".into(), words }],
        ..Default::default()
    }
}

#[test]
fn speech_operations_cut_by_the_transcript() {
    use crate::transcript::WordTiming;
    let dir = TempDir::new("agent-speech");
    let bundle = bundle(&dir);
    let mut document = open(&bundle, &dir);
    let error = apply_ops(&mut document, &ops(r#"{"op": "remove-fillers"}"#), false).unwrap_err();
    assert!(error.contains("omareel transcribe"), "{error}");
    document.set_transcript(Some(transcript(WordTiming::Estimated)));
    let error = apply_ops(&mut document, &ops(r#"{"op": "remove-fillers"}"#), false).unwrap_err();
    assert!(error.contains("estimated"), "{error}");

    document.set_transcript(Some(transcript(WordTiming::Measured)));
    // describe maps what was said onto the edited timeline.
    apply_ops(&mut document, &ops(r#"{"op": "delete", "from": 0, "to": 0.5}"#), false).unwrap();
    let summary = Value::Object(describe(&document));
    let segment = &summary.get("transcript").get("segments").to_array()[0];
    assert_eq!(segment.get("text").to_string_or(""), "Hello um world.");
    assert_eq!(segment.get("output").to_array()[0].get("start").to_double(-1.0), 0.5);
    assert_eq!(summary.get("transcript").get("word_timing").to_string_or(""), "measured");

    let (results, _) = apply_ops(&mut document, &ops(r#"{"op": "remove-fillers"}"#), false).unwrap();
    assert_eq!(results[0].get("fillers").to_int(0), 1);
    let removed = results[0].get("removed_seconds").to_double(0.0);
    assert!(removed > 0.2 && removed < 0.5, "{removed}");

    // Several source ranges are one cut; output ranges are cut from the last.
    let mut document = open(&bundle, &dir);
    apply_ops(&mut document, &ops(r#"{"op": "delete", "ranges": [[6, 7], [2, 3]], "timeline": "source"}"#), false).unwrap();
    assert_eq!(spans(&document), vec![(0.0, 2.0, 1.0), (3.0, 6.0, 1.0), (7.0, 10.0, 1.0)]);
    document.undo();
    assert_eq!(spans(&document), vec![(0.0, 10.0, 1.0)], "one undo step");
    apply_ops(&mut document, &ops(r#"{"op": "delete", "ranges": [[2, 3], [6, 7]]}"#), false).unwrap();
    assert_eq!(spans(&document), vec![(0.0, 2.0, 1.0), (3.0, 6.0, 1.0), (7.0, 10.0, 1.0)]);
    assert!(apply_ops(&mut document, &ops(r#"{"op": "delete", "ranges": [[3, 2]]}"#), false).is_err());
}

#[test]
fn pauses_are_trimmed_where_the_screen_is_still() {
    let dir = TempDir::new("agent-pauses");
    let bundle = bundle(&dir);
    // No pointer or keys: activity would keep the silence it falls in.
    std::fs::write(format!("{bundle}/input.jsonl"), b"").unwrap();
    let mut document = open(&bundle, &dir);
    // Speech at 0–1 and 5–10, silent 1–5: the measurements the editor would make.
    document.set_speech_regions(vec![(0.0, 1.0), (5.0, 10.0)]);
    let silences = document.silences_to_check().unwrap();
    document.set_still_windows(silences);
    let (results, _) = apply_ops(&mut document, &ops(r#"{"op": "trim-pauses", "keep": 0.5}"#), false).unwrap();
    assert_eq!(results[0].get("pauses").to_int(0), 1);
    let removed = results[0].get("removed_seconds").to_double(0.0);
    assert!((removed - 3.5).abs() < 1e-3, "4 s of silence, 0.5 s kept: {removed}");
    // Measured once: a second trim with the same setting finds nothing more.
    let (results, _) = apply_ops(&mut document, &ops(r#"{"op": "trim-pauses", "keep": 0.5}"#), false).unwrap();
    assert_eq!(results[0].get("pauses").to_int(-1), 0, "{:?} {:?}", results, spans(&document));
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

#[test]
fn chapters_are_written_at_source_times_and_follow_later_cuts() {
    let dir = TempDir::new("agent-chapters");
    let bundle = bundle(&dir);
    let chapters_file = crate::chapters::path(&bundle);
    let run = |ops: &str, dry_run: bool| {
        let (report, ok) = super::apply::run_on(|| Ok(open(&bundle, &dir)), ops.as_bytes(), dry_run, true);
        (crate::compat::qjson::object_from_json(report.as_bytes()).unwrap(), ok)
    };
    // Output times after a cut of 0–2 s are stored as source times.
    let (report, ok) = run(
        r#"[{"op": "delete", "from": 0, "to": 2},
            {"op": "set-chapters", "title": " Demo ", "description": "Two parts.",
             "chapters": [{"at": 4, "title": "Second"}, {"at": 0, "title": "First"}]}]"#,
        true,
    );
    assert!(ok, "{report:?}");
    assert!(!std::path::Path::new(&chapters_file).exists(), "a dry run writes nothing");
    assert!(report.value("describe").get("chapters").is_undefined());
    let (report, ok) = run(
        r#"[{"op": "delete", "from": 0, "to": 2},
            {"op": "set-chapters", "title": " Demo ", "description": "Two parts.",
             "chapters": [{"at": 4, "title": "Second"}, {"at": 0, "title": "First"}]}]"#,
        false,
    );
    assert!(ok, "{report:?}");
    let saved = crate::chapters::Chapters::load(&bundle).unwrap().unwrap();
    assert_eq!(saved.title, "Demo");
    let sources: Vec<(f64, &str)> = saved.chapters.iter().map(|c| (c.source, c.title.as_str())).collect();
    assert_eq!(sources, vec![(2.0, "First"), (6.0, "Second")]);
    let chapters = report.value("describe").get("chapters").clone();
    assert_eq!(chapters.get("chapters").to_array()[1].get("output").to_double(-1.0), 4.0);
    // A later cut before the second chapter moves it; only the title changes the file's text.
    let (report, ok) = run(r#"[{"op": "delete", "from": 1, "to": 2}, {"op": "set-chapters", "title": "Better"}]"#, false);
    assert!(ok, "{report:?}");
    let listed = report.value("describe").get("chapters").get("chapters").to_array();
    assert_eq!(listed[1].get("output").to_double(-1.0), 3.0);
    assert_eq!(listed[1].get("source").to_double(-1.0), 6.0);
    assert_eq!(crate::chapters::Chapters::load(&bundle).unwrap().unwrap().title, "Better");
    // Errors name the chapter; nothing is saved.
    let (report, ok) = run(r#"[{"op": "set-chapters", "chapters": [{"at": 1, "title": ""}]}]"#, false);
    assert!(!ok);
    assert_eq!(report.value("error").as_str(), Some("operation 1 (set-chapters): chapter 1 needs a \"title\""));
    let (report, _) = run(r#"[{"op": "set-chapters", "timeline": "source", "chapters": [{"at": 99, "title": "Late"}]}]"#, false);
    assert!(report.value("error").to_string_or("").contains("outside the recording (0 to 10)"), "{report:?}");
    // An empty set removes the file.
    let (_, ok) = run(r#"[{"op": "set-chapters", "title": "", "description": "", "chapters": []}]"#, false);
    assert!(ok);
    assert!(!std::path::Path::new(&chapters_file).exists());
}

/// chapters.json that can't be written saves nothing, so trying again applies the cuts once.
#[test]
fn chapters_that_cannot_be_written_save_nothing() {
    let dir = TempDir::new("agent-chapters-atomic");
    let bundle = bundle(&dir);
    let project_file = format!("{bundle}/project.json");
    let ops = r#"[{"op": "delete", "from": 0, "to": 2},
                  {"op": "set-chapters", "title": "Demo", "chapters": [{"at": 0, "title": "Start"}]}]"#;
    let run = || {
        let (report, ok) = super::apply::run_on(|| Ok(open(&bundle, &dir)), ops.as_bytes(), false, false);
        (crate::compat::qjson::object_from_json(report.as_bytes()).unwrap(), ok)
    };
    let before = std::fs::read(&project_file).ok();
    // A directory where chapters.json's temporary file goes.
    let blocker = format!("{}.{}.tmp", crate::chapters::path(&bundle), std::process::id());
    std::fs::create_dir_all(&blocker).unwrap();
    let (report, ok) = run();
    assert!(!ok);
    assert!(report.value("error").to_string_or("").starts_with("nothing was saved: cannot write"), "{report:?}");
    assert_eq!(std::fs::read(&project_file).ok(), before, "project.json was saved");
    assert!(!std::path::Path::new(&crate::chapters::path(&bundle)).exists());
    std::fs::remove_dir(&blocker).unwrap();
    let (report, ok) = run();
    assert!(ok, "{report:?}");
    assert_eq!(report.value("output_duration").to_double(0.0), 8.0, "{report:?}");
    assert_eq!(crate::chapters::Chapters::load(&bundle).unwrap().unwrap().title, "Demo");
    // No temporary file is left behind.
    let leftovers: Vec<String> = std::fs::read_dir(&bundle)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// Inside an agent's scope, operations can't pull files from outside the bundle and the
/// scratch directory into the project.
#[test]
fn scoped_operations_name_only_files_inside() {
    let dir = TempDir::new("agent-scoped-apply");
    let bundle = bundle(&dir);
    let scratch = dir.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let outside = dir.join("private.png");
    std::fs::write(&outside, b"png").unwrap();
    std::fs::write(format!("{scratch}/fine.png"), b"png").unwrap();
    let scope = super::scope::Scope::parse(&super::scope::value(&bundle, std::path::Path::new(&scratch))).unwrap();
    let run = |ops: String| {
        let (report, ok) = super::apply::run_in(|| Ok(open(&bundle, &dir)), ops.as_bytes(), false, false, Some(scope.clone()));
        (crate::compat::qjson::object_from_json(report.as_bytes()).unwrap(), ok)
    };
    for ops in [
        format!(r#"[{{"op": "insert", "file": "{outside}"}}]"#),
        format!(r#"[{{"op": "music", "file": "{outside}"}}]"#),
        format!(r#"[{{"op": "set", "path": "background.image", "value": "{outside}"}}]"#),
        format!(r#"[{{"op": "set", "path": "background.image", "value": "{scratch}/../private.png"}}]"#),
    ] {
        let (report, ok) = run(ops.clone());
        assert!(!ok, "{ops}: {report:?}");
        assert!(report.value("error").to_string_or("").contains("outside the recording"), "{ops}: {report:?}");
    }
    assert!(!std::path::Path::new(&format!("{bundle}/media/private.png")).exists());
    // A file in the scratch directory is fine, and without a scope anything is.
    let (report, ok) = run(format!(r#"[{{"op": "set", "path": "background.image", "value": "{scratch}/fine.png"}}]"#));
    assert!(ok, "{report:?}");
    let (report, ok) = super::apply::run_on(
        || Ok(open(&bundle, &dir)),
        format!(r#"[{{"op": "set", "path": "background.image", "value": "{outside}"}}]"#).as_bytes(),
        false,
        false,
    );
    assert!(ok, "{report}");
}

/// While an agent edits, the editor refuses its own edits (each undone at once, nothing to
/// save, undo and redo do nothing) but still takes in the agent's.
#[test]
fn a_read_only_editor_refuses_edits_but_takes_in_the_agents() {
    let dir = TempDir::new("agent-read-only");
    let bundle = bundle(&dir);
    let mut editor = open(&bundle, &dir);
    editor.set_project_value("background.blur", Value::from(0.05), false);
    assert!(editor.save_now());
    editor.take_effects();
    let undo_before = editor.can_undo();
    editor.set_read_only(true);
    assert!(editor.read_only());

    editor.set_project_value("background.blur", Value::from(0.2), false);
    editor.set_output_position(4.0);
    assert!(!editor.split_at_playhead() || editor.project().clips.len() == 1);
    assert_eq!(editor.project().clips.len(), 1);
    assert_eq!(editor.project().background.blur, 0.05);
    assert!(!editor.dirty(), "a refused edit leaves nothing to save");
    let effects = editor.take_effects();
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::StartAutosaveTimer)), "{effects:?}");
    assert!(effects.iter().any(|effect| matches!(effect, Effect::ProjectChanged)), "the views go back");
    editor.undo();
    assert_eq!(editor.project().background.blur, 0.05);
    assert_eq!(editor.can_undo(), undo_before);

    // The agent's apply comes in.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let mut other = open(&bundle, &dir);
    apply_ops(&mut other, &ops(r#"{"op": "split", "at": 4}"#), false).unwrap();
    assert!(other.save_now());
    assert_eq!(editor.reload_if_changed_on_disk(), DiskChange::Reloaded);
    assert_eq!(editor.project().clips.len(), 2);
    assert!(editor.read_only());

    // Once the agent is done, editing and undo work again, the agent's change one step away.
    editor.set_read_only(false);
    editor.set_project_value("background.blur", Value::from(0.3), false);
    assert_eq!(editor.project().background.blur, 0.3);
    editor.undo();
    editor.undo();
    assert_eq!(editor.project().clips.len(), 1);
}
