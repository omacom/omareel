//! The logic `tests/test_editor.cpp` checks through QML and the players, against the document,
//! plus history, coalescing, ids, presets and `setProjectValue` with what QML sends. A small
//! stand-in for the Qt side ([`Qt`]) performs the effects that move the document: seeks and
//! imports.

use super::*;
use crate::compat::qdir::tests::TempDir;
use crate::compat::qjson::{Object, Value};
use crate::compat::qround::q_fuzzy_compare;
use crate::project::{Clip, Project};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// tests/fixtures/synthetic, which the C++ test copies into each bundle.
fn fixture(file: &str) -> String {
    format!("{}/../../tests/fixtures/synthetic/{file}", env!("CARGO_MANIFEST_DIR"))
}

/// What ffprobe says of the test's 2 s, 320x180, 30 fps screen and camera recordings (the fake
/// `screen.mp4` and `camera.mp4` written here are just bytes).
fn media_probe(path: &str) -> Result<MediaInfo, String> {
    if !std::path::Path::new(path).is_file() {
        return Err(format!("{path}: No such file or directory"));
    }
    Ok(MediaInfo { width: 320, height: 180, duration: 2.0, fps: 30.0, audio: false })
}

/// What `probeVideo` says of an inserted file: a 0.7 s video for `*.mp4`, as MediaProbe words
/// its errors otherwise.
fn video_probe(path: &str) -> Result<VideoInfo, String> {
    let name = crate::compat::qdir::FileInfo::new(path).file_name();
    if !std::path::Path::new(path).is_file() {
        return Err(format!("{path} does not exist"));
    }
    if !name.ends_with(".mp4") {
        return Err(format!("{name} is not a video"));
    }
    Ok(VideoInfo { width: 320, height: 240, duration: 0.7, fps: 30.0, audio: false })
}

fn probes() -> Probes {
    Probes { media: Arc::new(media_probe), video: Arc::new(video_probe) }
}

/// A test directory with bundles, presets and caches in it.
struct World {
    dir: TempDir,
}

impl World {
    fn new(tag: &str) -> World {
        World { dir: TempDir::new(tag) }
    }

    fn path(&self, name: &str) -> String {
        self.dir.join(name)
    }

    /// `plainBundle(name)`: the screen, the input log and capture.json.
    fn bundle(&self, name: &str) -> String {
        let bundle = self.path(name);
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::copy(fixture("capture.json"), format!("{bundle}/capture.json")).unwrap();
        std::fs::copy(fixture("input.jsonl"), format!("{bundle}/input.jsonl")).unwrap();
        std::fs::write(format!("{bundle}/screen.mp4"), b"screen").unwrap();
        bundle
    }

    /// `cameraBundle(name)`: a camera that started with the screen.
    fn camera_bundle(&self, name: &str) -> String {
        let bundle = self.bundle(name);
        std::fs::write(format!("{bundle}/camera.mp4"), b"camera").unwrap();
        std::fs::write(
            format!("{bundle}/capture.json"),
            br#"{"fps":30,"first_frame_us":1000000,"camera":{"first_frame_us":1000000}}"#,
        )
        .unwrap();
        bundle
    }

    fn open(&self, bundle: &str) -> Qt {
        let locations = Locations { config: Some(self.path("config")), cache: Some(self.path("cache")) };
        let mut document =
            Document::open_with(bundle, &probes(), locations).unwrap_or_else(|e| panic!("open {bundle}: {e}"));
        let counter = Arc::new(AtomicUsize::new(0));
        document.set_id_generator(Box::new(move || format!("id{}", counter.fetch_add(1, Ordering::Relaxed) + 1)));
        let mut qt = Qt { document, effects: Vec::new() };
        qt.drain();
        qt
    }
}

/// The parts of the Qt side the tests need: seeks move the output position, imports run at once.
struct Qt {
    document: Document,
    /// Everything drained so far.
    effects: Vec<Effect>,
}

impl Qt {
    fn drain(&mut self) -> Vec<Effect> {
        let mut all = Vec::new();
        loop {
            let effects = self.document.take_effects();
            if effects.is_empty() {
                break;
            }
            for effect in &effects {
                match effect {
                    Effect::Seek(t) => self.seek(*t),
                    Effect::StartImport(source) => {
                        let imported = self.document.import_job(source).run();
                        self.document.finish_import(imported);
                    }
                    _ => {}
                }
            }
            all.extend(effects);
        }
        self.effects.extend(all.iter().cloned());
        all
    }

    /// `Editor::seek` as far as the document goes.
    fn seek(&mut self, t: f64) {
        if self.document.project().clips.is_empty() {
            return;
        }
        let target = self.document.clamp_playback_position(t);
        self.document.set_output_position(target);
    }

    fn run<T>(&mut self, f: impl FnOnce(&mut Document) -> T) -> T {
        let result = f(&mut self.document);
        self.drain();
        result
    }

    fn clip(&self, index: usize) -> Object {
        self.document.clips()[index].to_object()
    }

    fn clip_id(&self, index: usize) -> String {
        self.clip(index).value("id").to_string_or("")
    }

    fn zoom(&self, id: &str) -> Object {
        self.document.zooms().iter().map(Value::to_object).find(|z| z.value("id").to_string_or("") == id).unwrap_or_default()
    }

    fn remove_all_zooms(&mut self) {
        while let Some(first) = self.document.zooms().first().cloned() {
            let id = first.get("id").to_string_or("");
            assert!(self.run(|d| d.remove_zoom(&id)));
        }
    }

    fn undo_steps(&self) -> usize {
        self.document.history.undo.len()
    }
}

fn number(object: &Object, key: &str) -> f64 {
    object.value(key).to_double(f64::NAN)
}

fn text(object: &Object, key: &str) -> String {
    object.value(key).to_string_or("<missing>")
}

/// `QCOMPARE` on doubles.
fn fuzzy(actual: f64, expected: f64) -> bool {
    q_fuzzy_compare(actual, expected) || (actual == 0.0 && expected == 0.0)
}

// -------------------------------------------------------------------------------------------
// tests/test_editor.cpp

#[test]
fn operations_history_autosave_and_mapping() {
    let world = World::new("editor-ops");
    let bundle = world.bundle("editor.omareel");
    let mut qt = world.open(&bundle);
    let d = &qt.document;
    assert_eq!(d.bundle_name(), "editor");
    assert_eq!(d.source_width(), 320);
    assert!(!d.has_audio() && !d.has_desktop_audio() && !d.has_microphone_audio() && !d.has_camera());
    assert!(d.duration() > 1.9);
    assert!(d.selected_clip_id().is_empty());
    // A new project is dirty and saves itself soon.
    assert!(d.dirty());
    assert!(qt.effects.contains(&Effect::StartAutosaveTimer));

    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    assert_eq!(qt.document.clips().len(), 2);
    let second = qt.clip_id(1);
    assert!(qt.run(|d| d.trim_clip(&second, 1.2, 2.0)));
    assert_eq!(number(&qt.clip(1), "in"), 1.2);
    qt.run(Document::undo);
    assert_eq!(number(&qt.clip(1), "in"), 1.0);
    qt.run(Document::redo);
    assert_eq!(number(&qt.clip(1), "in"), 1.2);
    assert!(qt.run(|d| d.set_clip_speed(&second, 2.0)));
    assert!(fuzzy(qt.document.output_to_source(1.2), 1.6));
    assert!(fuzzy(qt.document.source_to_output(1.6, 1), 1.2));

    // The autosave timer's save.
    assert!(qt.run(Document::save_now));
    assert!(std::fs::metadata(format!("{bundle}/project.json")).unwrap().len() > 0);
    assert!(!qt.document.dirty());
    assert!(qt.effects.ends_with(&[Effect::DirtyChanged, Effect::Autosaved(format!("{bundle}/project.json"))]));
}

#[test]
fn opens_legacy_bundle_extension() {
    let world = World::new("editor-legacy");
    let qt = world.open(&world.bundle("legacy.omarecord"));
    assert_eq!(qt.document.bundle_name(), "legacy");
}

#[test]
fn zoom_constraints() {
    let world = World::new("editor-zooms");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let id = qt.run(|d| d.add_zoom_at(0.2, 1.2));
    assert!(!id.is_empty());
    assert!(qt.run(|d| d.move_zoom(&id, 1.8)));
    assert!(number(&qt.zoom(&id), "end") <= qt.document.source_duration());
    assert!(!qt.run(|d| d.resize_zoom(&id, 0.5, 1.0)));
    assert!(qt.run(|d| d.resize_zoom(&id, 0.4, 1.5)));
    assert!(qt.run(|d| d.set_zoom_level(&id, 9.0, false)));
    assert_eq!(number(&qt.zoom(&id), "level"), 4.0);
    assert!(qt.run(|d| d.remove_zoom(&id)));
}

#[test]
fn gradient_preset_uses_requested_index() {
    let world = World::new("editor-gradients");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let expected_stops = |d: &Document, index: usize| -> Value {
        let colours = d.gradients()[index].to_array();
        let last = (colours.len() - 1).max(1) as f64;
        Value::array(
            colours.iter().enumerate().map(|(i, c)| Value::array(vec![c.clone(), (i as f64 / last).into()])).collect(),
        )
    };
    let stops = |qt: &Qt| qt.document.project_map().value("background").get("gradient").get("stops").clone();
    qt.run(|d| d.apply_gradient_preset(7));
    assert_eq!(qt.document.project_map().value("background").get("type").to_string_or(""), "gradient");
    assert_eq!(stops(&qt), expected_stops(&qt.document, 7));
    qt.run(|d| d.apply_gradient_preset(3));
    assert_eq!(stops(&qt), expected_stops(&qt.document, 3));
    assert_ne!(expected_stops(&qt.document, 7), expected_stops(&qt.document, 3));
    // The same preset again changes nothing; out of range is ignored.
    let steps = qt.undo_steps();
    qt.run(|d| d.apply_gradient_preset(3));
    qt.run(|d| d.apply_gradient_preset(-1));
    qt.run(|d| d.apply_gradient_preset(10_000));
    assert_eq!(qt.undo_steps(), steps);
}

#[test]
fn zoom_track_drag_moves_the_selection_as_one_edit() {
    let world = World::new("editor-zoom-drag");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.remove_all_zooms();
    let id = qt.run(|d| d.add_zoom_at(0.1, 1.0));
    assert!(!id.is_empty());
    let original = number(&qt.zoom(&id), "start");
    let steps = qt.undo_steps();
    // ZoomTrack's drag: one coalesced edit through moveSelectedZooms.
    qt.run(|d| d.begin_coalesced_edit("move-selection"));
    for offset in [0.1, 0.25, 0.4] {
        assert!(qt.run(|d| d.move_selected_zooms(&id, original + offset)));
    }
    qt.run(Document::end_coalesced_edit);
    assert!((number(&qt.zoom(&id), "start") - (original + 0.4)).abs() < 1e-9);
    assert_eq!(qt.undo_steps(), steps + 1);
    // No movement is success without an undo step.
    assert!(qt.run(|d| d.move_selected_zooms(&id, original + 0.4)));
    assert_eq!(qt.undo_steps(), steps + 1);
    assert!(!qt.run(|d| d.move_selected_zooms("nope", 0.0)));
}

#[test]
fn real_timeline_block_bodies_drag() {
    let world = World::new("editor-block-drag");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    let clip_id = qt.clip_id(0);
    assert!(qt.run(|d| d.trim_clip(&clip_id, 0.0, 0.7)));
    let original_in = number(&qt.clip(0), "in");
    qt.remove_all_zooms();
    let zoom_id = qt.run(|d| d.add_zoom_at(0.1, 1.0));
    let original_start = number(&qt.zoom(&zoom_id), "start");

    // ClipTrack's Ctrl-drag of a body slides the clip's span.
    let steps = qt.undo_steps();
    qt.run(|d| d.begin_coalesced_edit(&format!("move-clip-{clip_id}")));
    for next_in in [0.1, 0.2, 0.4] {
        assert!(qt.run(|d| d.move_clip(&clip_id, original_in + next_in)));
    }
    qt.run(Document::end_coalesced_edit);
    assert!(number(&qt.clip(0), "in") > original_in + 0.05);
    // Bounded by the next recording clip (in at 1.0): length 0.7, so in at most 0.3.
    assert!(fuzzy(number(&qt.clip(0), "in"), 0.3));
    assert_eq!(qt.undo_steps(), steps + 1);
    // Not moving is success without an edit.
    assert!(qt.run(|d| d.move_clip(&clip_id, 0.3)));
    assert_eq!(qt.undo_steps(), steps + 1);

    assert!(qt.run(|d| d.move_zoom(&zoom_id, original_start + 0.4)));
    assert!(number(&qt.zoom(&zoom_id), "start") > original_start + 0.05);
}

#[test]
fn scrubbing_and_seeking_clamp_to_the_timeline() {
    let world = World::new("editor-scrub");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    // A click past the clips' end seeks to the end.
    qt.seek(qt.document.duration() + 10.0);
    assert!((qt.document.output_position() - qt.document.duration()).abs() < 0.001);
    qt.seek(-1.0);
    assert_eq!(qt.document.output_position(), 0.0);
    qt.seek(1.2);
    assert_eq!(qt.document.media_seek(qt.document.output_position()).unwrap().target_ms, 1200);
    qt.seek(0.5);
    assert_eq!(qt.document.media_seek(0.5).unwrap().target_ms, 500);
    // play() from the end starts over.
    qt.seek(qt.document.duration());
    assert_eq!(qt.document.play_start_seek(), Some(0.0));
    qt.seek(1.0);
    assert_eq!(qt.document.play_start_seek(), None);
}

#[test]
fn shift_drag_cuts_middle_as_one_undoable_edit() {
    let world = World::new("editor-cut");
    let bundle = world.bundle("editor.omareel");
    let mut qt = world.open(&bundle);
    qt.remove_all_zooms();
    let zoom_id = qt.run(|d| d.add_zoom_at(0.9, 1.1));
    assert!(!zoom_id.is_empty());
    // Timeline's shift-drag: a playback range, the playhead at its start, then Delete.
    qt.run(|d| d.set_playback_range(0.4, 1.3));
    assert!(qt.document.has_playback_range());
    qt.seek(0.4);
    let steps = qt.undo_steps();
    assert!(qt.run(|d| d.delete_output_range(0.4, 1.3)));
    qt.run(Document::clear_playback_range);
    assert!(!qt.document.has_playback_range());
    assert_eq!(qt.undo_steps(), steps + 1);
    assert_eq!(qt.document.clips().len(), 2);
    assert!((qt.document.duration() - 1.1).abs() < 0.03);
    assert_eq!(qt.document.zooms().len(), 1);
    let zoom_start = number(&qt.document.zooms()[0].to_object(), "start");
    assert!((zoom_start - 1.3).abs() < 0.03);
    assert!(qt.document.source_to_output(zoom_start, -1) >= 0.0);
    assert!((qt.document.output_to_source(number(&qt.clip(0), "out")) - 1.3).abs() < 0.03);
    // The part after the cut is selected, under a new id (the zoom took the first).
    assert_eq!(qt.document.selected_clip_id(), qt.clip_id(1));
    assert_eq!(qt.clip_id(1), "c-id2");

    assert!(qt.document.can_undo());
    qt.run(Document::undo);
    assert_eq!(qt.document.clips().len(), 1);
    assert!((qt.document.duration() - 2.0).abs() < 0.03);
    assert!(!qt.document.selected_clip_id().is_empty());
    assert_eq!(qt.document.selected_clip_id(), qt.clip_id(0));
    assert_eq!(qt.document.media_seek(qt.document.output_position()).unwrap().target_ms, 400);
    assert!((number(&qt.document.zooms()[0].to_object(), "start") - 0.9).abs() < 0.03);
    qt.run(Document::redo);
    assert_eq!(qt.document.clips().len(), 2);
    assert!((qt.document.duration() - 1.1).abs() < 0.03);
    assert!(qt.run(Document::save_now));
    let reopened = world.open(&bundle);
    assert_eq!(reopened.document.clips().len(), 2);
    assert!((reopened.document.duration() - 1.1).abs() < 0.03);
}

#[test]
fn delete_output_range_effects_and_refusals() {
    let world = World::new("editor-cut-effects");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    // The fixture's input has no clicks, so the new project has no zooms of its own.
    assert!(qt.document.zooms().is_empty());
    qt.effects.clear();
    // Not finite, empty, or everything: refused. Past the finiteness check the remainder id is
    // drawn before the cut is tried, as the C++ drew its UUID, so the two later refusals use one
    // each.
    assert!(!qt.run(|d| d.delete_output_range(f64::NAN, 1.0)));
    assert!(!qt.run(|d| d.delete_output_range(0.5, 0.5)));
    assert!(!qt.run(|d| d.delete_output_range(-5.0, 50.0)));
    assert!(qt.effects.is_empty());
    assert!(qt.run(|d| d.delete_output_range(1.5, 0.5)));
    assert_eq!(qt.clip_id(1), "c-id3");
    let effects = std::mem::take(&mut qt.effects);
    assert_eq!(effects[..2], [Effect::Pause, Effect::HistoryChanged]);
    assert_eq!(effects[effects.len() - 2..], [Effect::Seek(0.5), Effect::SelectionChanged]);
    // No zoom changed, so the motion track stays.
    assert!(!effects.contains(&Effect::StartMotionTimer));
    qt.run(Document::undo);

    // A zoom inside the cut goes, one across its end shrinks to what still shows, one before it
    // and one across the whole cut stay.
    let zoom = |id: &str, start: f64, end: f64| -> Value {
        Object::from_iter([
            ("id", Value::from(id)),
            ("start", start.into()),
            ("end", end.into()),
            ("level", 2.into()),
            ("target", "auto".into()),
        ])
        .into()
    };
    let zooms = Value::array(vec![zoom("before", 0.0, 0.3), zoom("inside", 0.6, 1.4), zoom("across", 1.0, 1.9)]);
    qt.run(|d| d.set_project_value("zooms", zooms, false));
    qt.effects.clear();
    assert!(qt.run(|d| d.delete_output_range(0.5, 1.5)));
    let left: Vec<(String, f64, f64)> =
        qt.document.project().zooms.iter().map(|z| (z.id.clone(), z.start, z.end)).collect();
    assert_eq!(left, [("before".to_owned(), 0.0, 0.3), ("across".to_owned(), 1.5, 1.9)]);
    assert!(qt.effects.contains(&Effect::StartMotionTimer));
    assert!(qt.document.selected_zoom_ids().is_empty());
    let whole = Value::array(vec![zoom("whole", 0.2, 1.8)]);
    qt.run(|d| d.set_project_value("zooms", whole, false));
    qt.seek(0.2);
    assert!(qt.run(|d| d.delete_output_range(0.3, 0.6)));
    assert_eq!(qt.document.project().zooms[0].start, 0.2);
    assert_eq!(qt.document.project().zooms[0].end, 1.8);
}

#[test]
fn video_thumbnail_urls_follow_source_times() {
    let world = World::new("editor-thumbnails");
    let mut qt = world.open(&world.bundle("thumbnails.omareel"));
    let cache = qt.document.thumbnails.sources["screen"].cache_directory.clone();
    assert!(cache.starts_with(&world.path("cache/clip-filmstrips-v2/")));
    // Nothing built yet: the frame at 0.3 s is queued.
    assert_eq!(qt.run(|d| d.video_thumbnail_url(0.25)), "");
    assert!(qt.effects.contains(&Effect::StartNextVideoThumbnail));
    assert_eq!(qt.document.next_video_thumbnail_job(true, false), None);
    assert_eq!(qt.document.next_video_thumbnail_job(false, true), None);
    let job = qt.document.next_video_thumbnail_job(false, false).unwrap();
    assert_eq!(job.path, format!("{cache}/0000000300.jpg"));
    assert_eq!(job.milliseconds, 300);
    assert_eq!(job.video_path, qt.document.video_path());
    // One build at a time.
    assert_eq!(qt.document.next_video_thumbnail_job(false, false), None);
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(&job.path, b"jpeg").unwrap();
    qt.run(|d| d.finish_video_thumbnail(true));
    assert_eq!(qt.document.video_thumbnail_revision(), 1);
    let first = qt.run(|d| d.video_thumbnail_url(0.25));
    assert_eq!(first, crate::compat::qurl::from_local_file(&job.path));
    // 0.4 within 0..0.5 is not built; the nearest built frame stands in.
    assert_eq!(qt.run(|d| d.video_thumbnail_url_for_clip(0.4, 0.0, 0.5)), first);
    // 1.25 s rounds to 1.3 s.
    let job = qt.document.next_video_thumbnail_job(false, false).unwrap();
    assert_eq!(job.milliseconds, 400);
    qt.run(|d| d.finish_video_thumbnail(false));
    qt.run(|d| d.video_thumbnail_url(1.25));
    assert_eq!(qt.document.next_video_thumbnail_job(false, false).unwrap().milliseconds, 1300);
    qt.run(|d| d.finish_video_thumbnail(false));
    // A tight span keeps its samples inside it, short of the last 1.5 frames.
    qt.run(|d| d.video_thumbnail_url_for_clip(0.17, 0.137, 0.181));
    let tight = qt.document.next_video_thumbnail_job(false, false).unwrap().milliseconds;
    assert!((137..=180).contains(&tight));
    assert_eq!(tight, 159);
    qt.run(|d| d.finish_video_thumbnail(false));
    // A failed frame is not asked for again; spans under a millisecond and non-finite times
    // give nothing.
    assert_eq!(qt.run(|d| d.video_thumbnail_url_for_clip(0.4, 0.4, 0.4005)), "");
    assert_eq!(qt.run(|d| d.video_thumbnail_url(f64::INFINITY)), "");
    qt.run(|d| d.video_thumbnail_url_for_clip(0.4, 0.0, 0.5));
    assert_eq!(qt.document.next_video_thumbnail_job(false, false), None);
}

#[test]
fn thumbnail_queue_is_bounded() {
    let world = World::new("editor-thumb-queue");
    let mut qt = world.open(&world.bundle("q.omareel"));
    // Spans starting a hundredth apart sample 200 different frames.
    for i in 0..200 {
        let t = f64::from(i) / 150.0;
        qt.run(|d| d.video_thumbnail_url_for_clip(t, t, t + 0.5));
    }
    assert_eq!(qt.document.thumbnails.queue.len(), THUMBNAIL_QUEUE_LIMIT);
}

#[test]
fn playback_stays_within_selected_range() {
    let world = World::new("editor-range");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.run(|d| d.set_playback_range(0.4, 0.9));
    assert!(qt.document.has_playback_range());
    qt.seek(0.0);
    assert!((qt.document.output_position() - 0.4).abs() < 0.002);
    qt.seek(1.5);
    assert!((qt.document.output_position() - 0.899).abs() < 0.005);
    // Playing from the range's end starts over at its start.
    assert_eq!(qt.document.play_start_seek(), Some(0.4));
    // A player position at the range's end stops there.
    let transport = Transport { playing: true, active_clip: 0, playhead_position: 0.85, ..Transport::default() };
    let step = qt.document.player_position(&transport, 899).unwrap();
    assert_eq!(step.stop_at_range_end, Some(0.9));
    // The playhead never runs past a millisecond before the end.
    let transport = Transport { playing: true, active_clip: 0, playhead_position: 0.85, playhead_clock_position: 0.85, ..transport };
    assert_eq!(qt.document.playhead_advance(&transport, 10_000), Some(0.899));
    qt.run(Document::clear_playback_range);
    qt.seek(0.0);
    assert!(qt.document.output_position().abs() < 0.002);

    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    qt.run(|d| d.set_playback_range(0.8, 1.3));
    qt.seek(0.8);
    assert_eq!(qt.document.play_start_seek(), None);
    // Equal or reversed ends, or under a millisecond, clear the range.
    qt.run(|d| d.set_playback_range(1.3, 1.3005));
    assert!(!qt.document.has_playback_range());
    qt.run(|d| d.set_playback_range(1.3, 0.8));
    assert_eq!(qt.document.playback_range(), (0.8, 1.3));
    let before = qt.effects.len();
    qt.run(|d| d.set_playback_range(0.8, 1.3));
    qt.run(|d| d.set_playback_range(f64::NAN, 1.3));
    assert_eq!(qt.effects.len(), before);
}

#[test]
fn playhead_follows_the_clock_between_frames() {
    let world = World::new("editor-playhead");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(0.2);
    let d = &qt.document;
    assert_eq!(d.playhead_update(0.0, 0.2), Some(0.2));
    assert_eq!(d.playhead_update(0.2, 0.2004), None);
    assert_eq!(d.playhead_update(0.2, 5.0), Some(2.0));
    let transport =
        Transport { playing: true, active_clip: 0, playhead_position: 0.2, playhead_clock_position: 0.2, ..Transport::default() };
    // Until the frame times are known the playhead leads by two frames at most (0.05 s floor
    // or 2/30 s).
    assert!((d.playhead_advance(&transport, 10).unwrap() - 0.21).abs() < 1e-12);
    assert!((d.playhead_advance(&transport, 1000).unwrap() - (0.2 + 2.0 / 30.0)).abs() < 1e-12);
    // A frame near the transport position anchors the clock; one from far away does not.
    assert_eq!(d.frame_anchor(Player::Screen, &transport, 230_000, true), Some(0.23));
    assert_eq!(d.frame_anchor(Player::Screen, &transport, 1_500_000, true), None);
    assert_eq!(d.frame_anchor(Player::Screen, &transport, 230_000, false), None);
    assert_eq!(d.frame_anchor(Player::Media, &transport, 230_000, true), None);
    assert_eq!(d.frame_anchor(Player::Screen, &Transport { scrubbing: true, ..transport }, 230_000, true), None);
}

#[test]
fn playback_runs_through_a_still_screen() {
    let world = World::new("editor-still");
    let mut qt = world.open(&world.bundle("still.omareel"));
    // Frames until 0.3 s, then none until 2.5 s (past this recording's 2 s end).
    let times: Vec<f64> = (0..10).map(|i| f64::from(i) / 30.0).collect();
    qt.document.set_screen_frame_times(times);
    assert!(qt.document.screen_frame_times_ready());
    assert_eq!(qt.document.next_screen_frame_time(0.1), 0.1 + 1.0 / 30.0 * 0.0 + (4.0 / 30.0 - 0.1));
    assert_eq!(qt.document.next_screen_frame_time(0.3), 2.0);
    assert_eq!(qt.document.next_screen_frame_time(5.0), 5.0);
    qt.seek(0.31);
    let transport =
        Transport { playing: true, active_clip: 0, playhead_position: 0.31, playhead_clock_position: 0.31, ..Transport::default() };
    // The clock runs on through the still stretch, to the clip's end.
    assert_eq!(qt.document.playhead_advance(&transport, 1000), Some(1.31));
    assert_eq!(qt.document.playhead_advance(&transport, 60_000), Some(2.0));
    // The player holds its last frame's time; the position follows the playhead.
    let transport = Transport { playhead_position: 1.2, ..transport };
    let step = qt.document.player_position(&transport, 300).unwrap();
    assert_eq!(step, PlayerPosition { stop_at_range_end: None, sync_camera: Some(1.2), advance: false, position: Some(1.2) });
    // At the clip's out the next clip takes over.
    let transport = Transport { playhead_position: 1.999, ..transport };
    assert!(qt.document.player_position(&transport, 300).unwrap().advance);
    // Paused, the player's own time counts.
    let paused = Transport { playing: false, ..transport };
    assert_eq!(qt.document.player_position(&paused, 300).unwrap().position, Some(0.3));
    assert_eq!(qt.document.player_position(&Transport { internal_seek: true, ..paused }, 300), None);
}

#[test]
fn zoom_rubber_band_selects_and_deletes_two() {
    let world = World::new("editor-band");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.remove_all_zooms();
    assert!(!qt.run(|d| d.add_zoom_at(0.0, 1.0)).is_empty());
    assert!(!qt.run(|d| d.add_zoom_at(1.0, 1.0)).is_empty());
    let original = qt.document.zooms().len();
    // The band runs right to left over both.
    qt.run(|d| d.select_zooms_in_output_range(590.0 / 200.0, 2.0 / 200.0));
    assert_eq!(qt.document.selected_zoom_ids().len(), 2);
    assert_eq!(qt.run(Document::remove_selected_zooms), 2);
    assert_eq!(qt.document.zooms().len(), original - 2);
    assert_eq!(qt.run(Document::remove_selected_zooms), 0);
}

#[test]
fn double_click_on_open_zoom_track_adds_zoom() {
    let world = World::new("editor-dclick");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.remove_all_zooms();
    let existing = qt.run(|d| d.add_zoom_at(1.0, 1.0));
    assert!(!existing.is_empty());
    assert!(qt.run(|d| d.add_zoom_at(1.5, 2.0)).is_empty());
    // Two seconds from 0.5 would run into the existing zoom, so the new one backs up to fit.
    let added = qt.run(|d| d.add_zoom_at(0.5, 2.0));
    assert_eq!(qt.document.zooms().len(), 2);
    let zoom = qt.document.zooms()[0].to_object();
    assert_eq!(text(&zoom, "id"), added);
    assert_ne!(added, existing);
    assert!(number(&zoom, "start").abs() < 0.03);
    assert!((number(&zoom, "end") - 1.0).abs() < 0.001);
    assert_eq!(qt.document.selected_zoom_ids(), std::slice::from_ref(&added));
    // The refused zoom drew no id.
    assert_eq!(added, "z-id2");
    // No room for a second anywhere now.
    assert!(qt.run(|d| d.add_zoom_at(0.5, 2.0)).is_empty());
}

#[test]
fn new_project_uses_captured_camera_orientation() {
    let world = World::new("editor-camera-default");
    let bundle = world.bundle("camera-default.omareel");
    std::fs::write(format!("{bundle}/camera.mp4"), b"camera").unwrap();
    std::fs::write(
        format!("{bundle}/capture.json"),
        br#"{"first_frame_us":1000000,"stopped_us":3000000,"camera":{"first_frame_us":1000000,"rotation":270,"flipHorizontal":true}}"#,
    )
    .unwrap();
    let qt = world.open(&bundle);
    let map = qt.document.project_map();
    assert_eq!(map.value("name").to_string_or(""), "camera-default");
    let camera = map.value("camera").to_object();
    assert_eq!(camera.value("rotation").to_int(0), 270);
    assert!(camera.value("flipHorizontal").to_bool(false));
    assert!(camera.value("enabled").to_bool(false));
}

#[test]
fn export_through_editor_api() {
    let world = World::new("editor-export");
    let bundle = world.bundle("editor.omareel");
    let mut qt = world.open(&bundle);
    let output = world.path("out/editor-api.mp4");
    let settings = ExportDialogSettings { fps: 10, height: 180, quality: "web-low".into() };
    let request = qt.run(|d| d.begin_export(&output, &settings)).unwrap();
    assert_eq!(
        request,
        ExportRequest {
            bundle_path: bundle.clone(),
            output_path: output.clone(),
            fps: 10,
            width: 320,
            quality: "web-low".into(),
            gif_fps: 0,
            gif_width: 0,
        }
    );
    // The project was saved first, and the output directory made.
    assert!(std::path::Path::new(&format!("{bundle}/project.json")).is_file());
    assert!(std::path::Path::new(&world.path("out")).is_dir());
    assert!(qt.effects.ends_with(&[Effect::ExportingChanged, Effect::ExportProgressChanged, Effect::ExportErrorChanged]));
    assert!(qt.document.export_state().exporting);
    // One at a time.
    assert_eq!(qt.run(|d| d.begin_export(&output, &settings)), None);
    qt.run(|d| d.export_progressed(30, 120, 2000));
    assert_eq!((qt.document.export_state().progress, qt.document.export_state().eta_seconds), (0.25, 6));
    qt.run(|d| d.export_finished(&output));
    assert_eq!(qt.document.export_state().progress, 1.0);
    assert_eq!(qt.document.export_state().exported_path, output);
    assert_eq!(qt.effects.last(), Some(&Effect::ExportFinished(output.clone())));

    // A GIF takes the rate and width as its own.
    let gif = qt.run(|d| d.begin_export(&world.path("out/x.GIF"), &settings)).unwrap();
    assert_eq!((gif.fps, gif.width, gif.gif_fps, gif.gif_width), (0, 0, 10, 320));
    qt.run(|d| d.export_failed("ffmpeg died"));
    assert_eq!(qt.document.export_state().error, "ffmpeg died");
    assert!(!qt.document.export_state().exporting);
    // No height keeps the width 0; an empty path does nothing; a directory that can't be made
    // is an error.
    let plain = qt.run(|d| d.begin_export(&world.path("out/y.mp4"), &ExportDialogSettings::default())).unwrap();
    assert_eq!(plain.width, 0);
    qt.run(|d| d.export_failed(""));
    assert_eq!(qt.run(|d| d.begin_export("", &settings)), None);
    std::fs::write(world.path("file"), b"").unwrap();
    assert_eq!(qt.run(|d| d.begin_export(&world.path("file/z.mp4"), &settings)), None);
    assert_eq!(qt.document.export_state().error, "Could not create the output directory");
    assert_eq!(qt.effects.last(), Some(&Effect::ExportErrorChanged));
}

#[test]
fn clip_layouts_are_undoable_and_survive_splits() {
    let world = World::new("editor-layouts");
    let mut qt = world.open(&world.camera_bundle("layouts.omareel"));
    assert!(qt.document.has_camera());
    let layout_of = |qt: &Qt, index: usize| text(&qt.clip(index), "layout");
    let camera_full = |qt: &Qt| qt.document.preview_layout().camera_full;
    let first = qt.clip_id(0);
    assert_eq!(layout_of(&qt, 0), "default");
    assert_eq!(camera_full(&qt), 0.0);
    assert_eq!(qt.document.preview_layout().bubble, 1.0);
    assert!(!qt.run(|d| d.set_clip_layout(&first, "sideways")));
    assert!(!qt.document.can_undo());

    assert!(qt.run(|d| d.set_clip_layout(&first, "camera")));
    assert_eq!(qt.effects.last(), Some(&Effect::SyncCamera { screen_source_time: 0.0, force: true }));
    assert_eq!(layout_of(&qt, 0), "camera");
    assert_eq!(camera_full(&qt), 1.0);
    assert!(qt.document.camera_visible());
    // A full-frame camera keeps closer to the screen.
    assert_eq!(qt.document.camera_sync(1.0), Some(CameraSync { target_ms: 1000, tolerance_ms: 80, covered: true }));
    qt.run(Document::undo);
    assert_eq!(layout_of(&qt, 0), "default");
    assert_eq!(camera_full(&qt), 0.0);
    assert_eq!(qt.document.camera_sync(1.0), Some(CameraSync { target_ms: 1000, tolerance_ms: 250, covered: true }));
    qt.run(Document::redo);
    assert_eq!(layout_of(&qt, 0), "camera");
    // The same layout again is success without an edit.
    let steps = qt.undo_steps();
    assert!(qt.run(|d| d.set_clip_layout(&first, "camera")));
    assert_eq!(qt.undo_steps(), steps);

    qt.seek(1.0);
    assert!(qt.run(Document::split_at_playhead));
    assert_eq!(qt.document.clips().len(), 2);
    assert_eq!(layout_of(&qt, 1), "camera");
    let second = qt.clip_id(1);
    assert!(qt.run(|d| d.set_clip_layout(&second, "default")));
    // A quarter second into the second clip, the camera is halfway back to its bubble.
    qt.seek(1.25);
    assert!((camera_full(&qt) - 0.5).abs() < 1e-6);
    assert!(qt.run(|d| d.set_clip_layout(&second, "screen")));
    qt.seek(1.9);
    assert_eq!(camera_full(&qt), 0.0);
    assert_eq!(qt.document.preview_layout().bubble, 0.0);

    // The camera clip's filmstrip shows the webcam, the others the screen.
    assert_eq!(qt.document.camera_thumbnail_key(), "camera-r0");
    qt.run(|d| d.clip_thumbnail_url(&first, 0.5, 0.0, 1.0));
    let job = qt.document.next_video_thumbnail_job(false, false).unwrap();
    assert!(job.path.contains("-camera-r0/"), "{}", job.path);
    assert_eq!(job.video_path, qt.document.camera_video_path());
    assert_eq!(job.filter, "");
    qt.run(|d| d.finish_video_thumbnail(false));
    qt.run(|d| d.clip_thumbnail_url(&second, 1.5, 1.0, 2.0));
    let job = qt.document.next_video_thumbnail_job(false, false).unwrap();
    assert!(!job.path.contains("-camera-"), "{}", job.path);
    qt.run(|d| d.finish_video_thumbnail(false));

    // A turned camera's frames are turned the same way, in their own cache.
    qt.run(|d| d.set_project_value("camera.rotation", 90.into(), false));
    qt.run(|d| d.set_project_value("camera.flipHorizontal", true.into(), false));
    assert_eq!(qt.document.camera_thumbnail_key(), "camera-r90f");
    qt.run(|d| d.clip_thumbnail_url(&first, 0.5, 0.0, 1.0));
    let job = qt.document.next_video_thumbnail_job(false, false).unwrap();
    assert!(job.path.contains("-camera-r90f/"));
    assert_eq!(job.filter, "hflip,transpose=clock");
}

#[test]
fn camera_timing_and_status() {
    let world = World::new("editor-camera-status");
    let bundle = world.camera_bundle("status.omareel");
    let qt = world.open(&bundle);
    assert_eq!(qt.document.camera_status(), "Webcam: 320×180 @30, offset +0.00 s");
    assert_eq!(qt.document.camera_sync(2.5), Some(CameraSync { target_ms: 2000, tolerance_ms: 250, covered: false }));
    assert!(qt.document.camera_thumbnail_job().unwrap().cache_directory.contains("/camera-filmstrips/"));
    std::fs::write(
        format!("{bundle}/capture.json"),
        br#"{"first_frame_us":1000000,"camera":{"first_frame_us":500000}}"#,
    )
    .unwrap();
    let late = world.open(&bundle);
    assert_eq!(late.document.camera_status(), "Webcam: 320×180 @30, offset -0.50 s");
    let plain = world.open(&world.bundle("plain.omareel"));
    assert_eq!(plain.document.camera_status(), "No webcam in this bundle");
    assert_eq!(plain.document.camera_sync(1.0), None);
    assert!(plain.document.camera_thumbnail_job().is_none());
}

#[test]
fn inserted_videos_split_the_timeline_and_keep_their_file() {
    let world = World::new("editor-inserts");
    let bundle = world.bundle("inserts.omareel");
    let demo = world.path("My Demo.mp4");
    std::fs::write(&demo, b"demo video").unwrap();
    let mut qt = world.open(&bundle);

    qt.seek(1.0);
    qt.run(|d| d.insert_media(&demo));
    assert!(!qt.document.importing());
    assert!(qt.document.import_error().is_empty(), "{}", qt.document.import_error());
    assert_eq!(qt.document.clips().len(), 3);
    assert_eq!(number(&qt.clip(0), "out"), 1.0);
    assert_eq!(text(&qt.clip(1), "kind"), "media");
    assert_eq!(text(&qt.clip(1), "media"), "media/My Demo.mp4");
    assert_eq!(text(&qt.clip(1), "mediaName"), "My Demo.mp4");
    assert!((number(&qt.clip(1), "out") - 0.7).abs() < 0.05);
    assert_eq!(number(&qt.clip(2), "in"), 1.0);
    assert!(std::path::Path::new(&format!("{bundle}/media/My Demo.mp4")).is_file());
    assert_eq!(qt.document.selected_clip_id(), qt.clip_id(1));
    assert_eq!(qt.document.output_position(), 1.0);
    assert!((qt.document.duration() - 2.7).abs() < 0.05);
    assert!(qt.document.preview_media(false, "").active);
    assert!(!qt.document.camera_visible());
    assert!(qt.run(|d| d.add_zoom_at(1.2, 2.0)).is_empty());
    // The right half and the insert took the next ids, in that order.
    assert_eq!((qt.clip_id(1), qt.clip_id(2)), ("m-id2".to_owned(), "c-id1".to_owned()));

    // Recording clips stay in recording order across the insert, which has its own bounds.
    let left = qt.clip_id(0);
    let insert = qt.clip_id(1);
    let right = qt.clip_id(2);
    assert_eq!(number(&qt.clip(2), "lower"), 1.0);
    assert!(qt.run(|d| d.trim_clip(&right, 0.5, 2.0)));
    assert_eq!(number(&qt.clip(2), "in"), 1.0);
    assert!(qt.run(|d| d.trim_clip(&insert, 0.2, 5.0)));
    assert!((number(&qt.clip(1), "out") - 0.7).abs() < 0.05);
    assert!(qt.run(|d| d.reset_clip_trims(&insert)));
    assert_eq!(number(&qt.clip(1), "in"), 0.0);
    assert!(!qt.run(|d| d.reorder_clip(&left, 1)));
    assert!(!qt.run(|d| d.merge_clip(&left, 1)));
    assert!(!qt.run(|d| d.set_clip_layout(&insert, "camera")));
    assert!(qt.run(|d| d.set_clip_fit(&insert, "framed")));
    assert_eq!(text(&qt.clip(1), "fit"), "framed");
    assert!(!qt.run(|d| d.set_clip_fit(&left, "fill")));

    assert!(qt.run(|d| d.reorder_clip(&insert, -1)));
    assert_eq!(qt.clip_id(0), insert);
    assert_eq!(qt.document.output_position(), 0.0);
    assert!(!qt.run(|d| d.reorder_clip(&insert, -1)));
    assert!(qt.run(|d| d.remove_clip(&left)));
    assert!(!qt.run(|d| d.remove_clip(&right)));

    // Undo walks every step back to the recording alone; the copied video stays for redo.
    while qt.document.can_undo() {
        qt.run(Document::undo);
    }
    assert_eq!(qt.document.clips().len(), 1);
    assert!(std::path::Path::new(&format!("{bundle}/media/My Demo.mp4")).is_file());
    qt.run(Document::redo);
    assert_eq!(qt.document.clips().len(), 3);
    assert_eq!(text(&qt.clip(1), "kind"), "media");

    // The same file again gets its own name; something that is not a video is refused.
    qt.seek(2.69);
    qt.run(|d| d.insert_media(&demo));
    assert_eq!(qt.document.clips().len(), 4);
    assert_eq!(text(&qt.clip(3), "media"), "media/My Demo-2.mp4");
    let notes = world.path("notes.txt");
    std::fs::write(&notes, b"not a video").unwrap();
    qt.run(|d| d.insert_media(&notes));
    assert_eq!(qt.document.import_error(), "notes.txt is not a video");
    assert_eq!(qt.document.clips().len(), 4);
    qt.run(Document::clear_import_error);
    assert!(qt.document.import_error().is_empty());
}

// The split forms the Qt side pauses inside: their checks change nothing (but the id the cut
// draws), and the edits queue what the whole operations queue after their pause.
#[test]
fn paused_operations_check_before_they_edit() {
    let world = World::new("editor-split");
    let bundle = world.bundle("split.omareel");
    let demo = world.path("demo.mp4");
    std::fs::write(&demo, b"demo video").unwrap();
    let mut qt = world.open(&bundle);
    qt.effects.clear();

    let project = qt.document.project().to_json();
    assert!(qt.document.prepare_delete_output_range(f64::NAN, 1.0).is_none());
    assert!(qt.document.prepare_delete_output_range(-5.0, 50.0).is_none());
    let cut = qt.document.prepare_delete_output_range(1.5, 0.5).unwrap();
    assert_eq!(qt.document.project().to_json(), project);
    assert!(!qt.document.can_undo());
    assert!(qt.document.take_effects().is_empty());
    qt.document.apply_delete_output_range(cut);
    let effects = qt.drain();
    assert_eq!(effects[0], Effect::HistoryChanged);
    assert_eq!(effects[effects.len() - 2..], [Effect::Seek(0.5), Effect::SelectionChanged]);
    assert!(!effects.contains(&Effect::Pause));
    // The refused cut drew c-id1, the one that went through c-id2.
    assert_eq!(qt.clip_id(1), "c-id2");
    qt.run(Document::undo);

    assert!(qt.document.can_insert_media(&demo));
    assert!(!qt.document.can_insert_media(""));
    qt.document.start_import(&demo);
    assert!(!qt.document.can_insert_media(&demo));
    assert_eq!(qt.drain()[..2], [Effect::ImportingChanged, Effect::StartImport(demo.clone())]);
    let insert = qt.clip_id(1);
    assert_eq!(text(&qt.clip(1), "kind"), "media");

    let left = qt.clip_id(0);
    assert!(!qt.document.can_reorder_clip(&left, 1));
    assert!(!qt.document.can_reorder_clip("nothing", 1));
    assert!(qt.document.can_reorder_clip(&insert, -1));
    let steps = qt.undo_steps();
    qt.document.apply_reorder_clip(&insert, -1);
    let effects = qt.drain();
    assert_eq!(effects[0], Effect::HistoryChanged);
    assert_eq!(effects[effects.len() - 2..], [Effect::Seek(0.0), Effect::SelectionChanged]);
    assert_eq!((qt.clip_id(0), qt.undo_steps()), (insert, steps + 1));
}

#[test]
fn insert_media_queues_the_import() {
    let world = World::new("editor-insert-effects");
    let mut qt = world.open(&world.bundle("i.omareel"));
    qt.document.insert_media("/somewhere/x.mp4");
    assert_eq!(
        qt.document.take_effects(),
        [Effect::Pause, Effect::ImportingChanged, Effect::StartImport("/somewhere/x.mp4".into())]
    );
    assert!(qt.document.importing());
    // One import at a time, and an empty source is ignored.
    qt.document.insert_media("/somewhere/y.mp4");
    assert!(qt.document.take_effects().is_empty());
    qt.document.finish_import(ImportedInsert { error: "x.mp4 does not exist".into(), ..ImportedInsert::default() });
    assert_eq!(qt.document.take_effects(), [Effect::ImportingChanged]);
    qt.document.insert_media("");
    assert!(qt.document.take_effects().is_empty());
}

#[test]
fn missing_inserted_video_still_opens() {
    let world = World::new("editor-missing");
    let bundle = world.bundle("missing-insert.omareel");
    let mut project = Project::defaults("Missing", 2.0);
    project.clips.push(Clip { id: "m".into(), in_: 0.0, out: 1.0, speed: 1.0, media: "media/gone.mp4".into(), ..Clip::default() });
    project.save(&format!("{bundle}/project.json")).unwrap();
    let mut qt = world.open(&bundle);
    assert_eq!(qt.document.clips().len(), 2);
    assert!(qt.clip(1).value("missing").to_bool(false));
    assert_eq!(number(&qt.clip(1), "upper"), 1.0);
    qt.seek(2.5);
    let media = qt.document.preview_media(true, "media/gone.mp4");
    assert!(media.active && media.missing && media.ready);
    assert_eq!((media.width, media.height), (1, 1));
    // An existing project is not dirty and has its own zooms.
    assert!(!qt.document.dirty());
    assert!(qt.document.zooms().is_empty());
}

#[test]
fn playback_crosses_into_an_inserted_video_and_back() {
    let world = World::new("editor-insert-playback");
    let bundle = world.bundle("insert-playback.omareel");
    std::fs::create_dir_all(format!("{bundle}/media")).unwrap();
    std::fs::write(format!("{bundle}/media/demo.mp4"), b"demo").unwrap();
    let mut project = Project::defaults("Playback", 2.0);
    let insert = Clip { id: "m".into(), in_: 0.2, out: 0.8, speed: 1.0, media: "media/demo.mp4".into(), ..Clip::default() };
    project.clips = vec![
        Clip { id: "a".into(), in_: 0.0, out: 0.6, speed: 1.0, ..Clip::default() },
        insert,
        Clip { id: "b".into(), in_: 0.6, out: 1.4, speed: 1.0, ..Clip::default() },
    ];
    project.save(&format!("{bundle}/project.json")).unwrap();
    let mut qt = world.open(&bundle);
    qt.seek(0.3);
    let d = &qt.document;
    // The insert after the playhead loads ahead and waits on its first frame.
    assert_eq!(
        d.next_clip(0),
        Some(NextClip { index: 1, kind: NextClipKind::IntoMedia("media/demo.mp4".into()), speed: 1.0, target_ms: 200 })
    );
    assert_eq!(d.next_clip(1), Some(NextClip { index: 2, kind: NextClipKind::IntoRecording, speed: 1.0, target_ms: 600 }));
    assert_eq!(d.next_clip(2), None);
    assert_eq!(d.next_clip(-1), None);
    assert_eq!(d.advance_target(0), Some(0.6));
    assert!(fuzzy(d.advance_target(1).unwrap(), 1.2));
    assert_eq!(d.advance_target(2), None);
    assert_eq!((d.player_for(0), d.player_for(1), d.player_for(9)), (Player::Screen, Player::Media, Player::Screen));
    let transport = Transport { playing: true, active_clip: 1, ..Transport::default() };
    assert!(d.player_should_play(Player::Media, &transport) && !d.player_should_play(Player::Screen, &transport));
    assert!(d.player_should_play(Player::Camera, &transport));
    // The media player is heard only during its clip.
    assert_eq!(d.insert_audio(1, false), (1.0, false));
    assert_eq!(d.insert_audio(1, true), (1.0, true));
    assert_eq!(d.insert_audio(0, false), (1.0, true));

    // Seeking back into the insert while paused shows it again.
    qt.seek(0.9);
    let seek = qt.document.media_seek(0.9).unwrap();
    assert_eq!(seek.clip_index, 1);
    assert_eq!(seek.media.as_deref(), Some("media/demo.mp4"));
    assert!(fuzzy(seek.source, 0.5));
    assert_eq!(seek.target_ms, 500);
    assert!(qt.document.preview_media(false, "media/demo.mp4").active);
    assert!(!qt.document.preview_media(false, "media/demo.mp4").ready);
    assert!(qt.document.preview_media(true, "media/demo.mp4").ready);
    assert!(!qt.document.preview_media(true, "media/other.mp4").ready);
    // During the insert, no cursor, ripples or keys, and the zoom of the recording time.
    assert!(!qt.document.preview_cursor().visible);
    assert!(qt.document.preview_keystroke_pills().is_empty());
    assert_eq!(qt.document.source_position(), 0.6);
    // The camera never runs for an inserted clip, and the end of the media ends its clip.
    let at_end = Transport { playing: true, active_clip: 1, playhead_position: 1.2, ..Transport::default() };
    let step = qt.document.player_position(&at_end, 798).unwrap();
    assert_eq!(step, PlayerPosition { stop_at_range_end: None, sync_camera: None, advance: true, position: None });
}

#[test]
fn frame_steps_move_the_paused_player() {
    let world = World::new("editor-steps");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(0.5);
    assert_eq!(qt.document.media_seek(0.5).unwrap().target_ms, 500);
    // One frame at 30 fps is 33 ms.
    let next = qt.document.step_target(1);
    qt.seek(next);
    assert_eq!(qt.document.media_seek(qt.document.output_position()).unwrap().target_ms, 533);
    let back = qt.document.step_target(-1);
    qt.seek(back);
    assert_eq!(qt.document.media_seek(qt.document.output_position()).unwrap().target_ms, 500);
    assert_eq!(qt.document.boundary_target(-1), 0.0);
    assert_eq!(qt.document.boundary_target(1), qt.document.duration());
}

// -------------------------------------------------------------------------------------------
// History, coalescing and the dirty flag.

#[test]
fn coalesced_edits_make_one_undo_step() {
    let world = World::new("editor-coalesce");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let steps = qt.undo_steps();
    // A slider: the same path coalesces with itself.
    qt.run(|d| d.set_project_value("frame.padding", 0.2.into(), true));
    qt.run(|d| d.set_project_value("frame.padding", 0.25.into(), true));
    assert_eq!(qt.undo_steps(), steps + 1);
    // Surprising but as in the C++: the snapshot leaves its key as the open coalesced edit's,
    // so a coalescing edit of another path right after joins the same step. PanelSlider ends
    // its edit when released, which keeps sliders apart.
    qt.run(|d| d.set_project_value("frame.radius", 4.into(), true));
    assert_eq!(qt.undo_steps(), steps + 1);
    qt.run(Document::end_coalesced_edit);
    qt.run(|d| d.set_project_value("frame.radius", 5.into(), true));
    assert_eq!(qt.undo_steps(), steps + 2);
    // An open coalesced edit gathers different paths under its key.
    qt.run(|d| d.begin_coalesced_edit("crop"));
    for (path, value) in [("crop.x", 0.1), ("crop.y", 0.1), ("crop.w", 0.8), ("crop.h", 0.8)] {
        qt.run(|d| d.set_project_value(path, value.into(), true));
    }
    qt.run(Document::end_coalesced_edit);
    assert_eq!(qt.undo_steps(), steps + 3);
    // An edit that does not coalesce ends the run.
    qt.run(|d| d.set_project_value("frame.padding", 0.3.into(), false));
    qt.run(|d| d.set_project_value("frame.padding", 0.35.into(), true));
    assert_eq!(qt.undo_steps(), steps + 5);
    qt.run(Document::undo);
    assert_eq!(qt.document.project().frame.padding, 0.3);
    assert!(qt.document.can_redo());
    // A new edit drops the redo steps.
    qt.run(|d| d.set_project_value("frame.radius", 6.into(), false));
    assert!(!qt.document.can_redo());
}

#[test]
fn trims_of_one_clip_coalesce_until_something_else_happens() {
    // Surprising but as in the C++: trimClip always snapshots with "trim-<id>", and the key
    // stays after the edit, so two separate trims of the same clip with nothing in between are
    // one undo step.
    let world = World::new("editor-trim-coalesce");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let id = qt.clip_id(0);
    let steps = qt.undo_steps();
    assert!(qt.run(|d| d.trim_clip(&id, 0.1, 1.9)));
    assert!(qt.run(|d| d.trim_clip(&id, 0.2, 1.8)));
    assert_eq!(qt.undo_steps(), steps + 1);
    // endCoalescedEdit (ClipTrack calls it when a drag ends) separates them.
    qt.run(Document::end_coalesced_edit);
    assert!(qt.run(|d| d.trim_clip(&id, 0.3, 1.7)));
    assert_eq!(qt.undo_steps(), steps + 2);
    // A trim under 0.1 s is refused without a step.
    assert!(!qt.run(|d| d.trim_clip(&id, 1.0, 1.05)));
    assert_eq!(qt.undo_steps(), steps + 2);
    qt.run(Document::undo);
    assert_eq!(qt.document.project().clips[0].in_, 0.2);
}

#[test]
fn redo_leaves_a_coalesced_edit_open() {
    let world = World::new("editor-redo-key");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.run(|d| d.set_project_value("frame.padding", 0.2.into(), false));
    qt.run(Document::undo);
    qt.run(|d| d.begin_coalesced_edit("k"));
    qt.run(Document::redo);
    let steps = qt.undo_steps();
    // Undo would have ended the edit; redo does not, so both values coalesce under "k".
    qt.run(|d| d.set_project_value("frame.padding", 0.3.into(), true));
    qt.run(|d| d.set_project_value("frame.radius", 5.into(), true));
    assert_eq!(qt.undo_steps(), steps + 1);
}

#[test]
fn history_keeps_the_last_hundred_steps() {
    let world = World::new("editor-history-limit");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    for i in 0..105 {
        qt.run(|d| d.set_project_value("frame.radius", (i + 100).into(), false));
    }
    assert_eq!(qt.undo_steps(), history::UNDO_LIMIT);
    while qt.document.can_undo() {
        qt.run(Document::undo);
    }
    // The oldest five are gone.
    assert_eq!(qt.document.project().frame.radius, 104.0);
}

#[test]
fn edits_signal_as_editor_cpp_did() {
    let world = World::new("editor-effects");
    let bundle = world.bundle("editor.omareel");
    let mut first = world.open(&bundle);
    assert!(first.run(Document::save_now));
    let mut qt = world.open(&bundle);
    assert!(!qt.document.dirty());
    let revision = qt.document.project_map_revision();
    qt.run(|d| d.set_project_value("frame.padding", 0.2.into(), false));
    let changed = [
        Effect::HistoryChanged,
        Effect::SetScreenAudio { volume: 1.0, muted: true },
        Effect::UpdateInsertAudio,
        Effect::DirtyChanged,
        Effect::StartAutosaveTimer,
        Effect::ProjectChanged,
        Effect::DurationChanged,
        Effect::OutputSizeChanged,
        Effect::StartMotionTimer,
        Effect::CompositionChanged,
    ];
    assert_eq!(std::mem::take(&mut qt.effects), changed);
    assert_eq!(qt.document.project_map_revision(), revision + 1);
    // Dirty already: no DirtyChanged. Clip edits leave the motion track alone.
    let id = qt.clip_id(0);
    qt.run(|d| d.set_clip_fit(&id, "fill"));
    assert!(qt.effects.is_empty());
    qt.run(|d| d.trim_clip(&id, 0.5, 1.5));
    assert_eq!(
        std::mem::take(&mut qt.effects),
        [
            Effect::HistoryChanged,
            Effect::SetScreenAudio { volume: 1.0, muted: true },
            Effect::UpdateInsertAudio,
            Effect::StartAutosaveTimer,
            Effect::ProjectChanged,
            Effect::DurationChanged,
            Effect::OutputSizeChanged,
            Effect::CompositionChanged,
        ]
    );
    // An edit that changes nothing does nothing.
    qt.run(|d| d.set_project_value("frame.padding", 0.2.into(), false));
    qt.run(|d| d.set_project_value("", 1.into(), false));
    qt.run(|d| d.set_project_value("..", 1.into(), false));
    assert!(qt.effects.is_empty());
    // Undo restores, seeks where the playhead now falls, and reports the selection.
    qt.seek(0.9);
    qt.run(Document::undo);
    let effects = std::mem::take(&mut qt.effects);
    assert!(
        effects.ends_with(&[Effect::CompositionChanged, Effect::Seek(0.9), Effect::SelectionChanged, Effect::HistoryChanged]),
        "{effects:?}"
    );
    assert!(!effects.contains(&Effect::DirtyChanged));
    qt.run(Document::save_now);
    assert_eq!(qt.effects, [Effect::DirtyChanged, Effect::Autosaved(format!("{bundle}/project.json"))]);
}

#[test]
fn motion_builds_one_at_a_time() {
    let world = World::new("editor-motion");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let job = qt.document.motion_job().unwrap();
    assert!(qt.document.motion_job().is_none());
    assert_eq!((job.width, job.height, job.duration, job.capture_fps), (320, 180, 2.0, 30.0));
    // An edit while it builds asks for another build when it lands.
    qt.run(|d| d.set_project_value("cursor.size", 2.into(), false));
    qt.document.apply_motion(job.run());
    assert_eq!(qt.document.take_effects(), [Effect::CompositionChanged, Effect::StartMotionTimer]);
    let job = qt.document.motion_job().unwrap();
    qt.document.apply_motion(job.run());
    assert_eq!(qt.document.take_effects(), [Effect::CompositionChanged]);
    assert!(qt.document.motion().is_some());
}

#[test]
fn ids_come_from_the_generator() {
    let world = World::new("editor-ids");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(0.5);
    qt.run(Document::split_at_playhead);
    qt.remove_all_zooms();
    let zoom = qt.run(|d| d.add_zoom_at(1.0, 1.0));
    assert_eq!((qt.clip_id(1).as_str(), zoom.as_str()), ("c-id1", "z-id2"));
    // Refused splits and zooms take no id.
    qt.seek(0.55);
    assert!(!qt.run(Document::split_at_playhead));
    assert!(qt.run(|d| d.add_zoom_at(1.5, 1.0)).is_empty());
    qt.seek(1.5);
    qt.run(Document::split_at_playhead);
    assert_eq!(qt.clip_id(2), "c-id3");
    // The default generator writes QUuid's form.
    let id = random_uuid();
    assert_eq!(id.len(), 36);
}

// -------------------------------------------------------------------------------------------
// setProjectValue with what QML sends.

#[test]
fn set_project_value_takes_every_value_qml_sends() {
    let world = World::new("editor-values");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let p = |qt: &Qt| qt.document.project().clone();
    // Booleans, integers, doubles, strings.
    qt.run(|d| d.set_project_value("keystrokes.enabled", true.into(), false));
    assert!(p(&qt).keystrokes.enabled);
    qt.run(|d| d.set_project_value("camera.rotation", 90.into(), false));
    assert_eq!(p(&qt).camera.rotation, 90);
    // Project::fromJson starts from the defaults, so a rotation the editor does not offer
    // becomes the default 0 rather than staying, and that is an edit.
    let steps = qt.undo_steps();
    qt.run(|d| d.set_project_value("camera.rotation", 45.into(), false));
    assert_eq!((p(&qt).camera.rotation, qt.undo_steps()), (0, steps + 1));
    // A double with a fraction is not an int (QJsonValue::toInt), so it is no rotation either.
    qt.run(|d| d.set_project_value("camera.rotation", 180.into(), false));
    qt.run(|d| d.set_project_value("camera.rotation", Value::Double(180.5), false));
    assert_eq!(p(&qt).camera.rotation, 0);
    qt.run(|d| d.set_project_value("frame.padding", 0.15.into(), false));
    assert_eq!(p(&qt).frame.padding, 0.15);
    qt.run(|d| d.set_project_value("name", "Renamed".into(), false));
    assert_eq!(qt.document.bundle_name(), "Renamed");
    qt.run(|d| d.set_project_value("aspect", "9:16".into(), false));
    assert_eq!(qt.document.output_width(), 406);
    qt.run(|d| d.set_project_value("aspect", Value::Null, false));
    assert_eq!(p(&qt).aspect, "auto");
    assert_eq!(qt.document.output_width(), 1280);
    // null: the cursor's idle hiding off.
    qt.run(|d| d.set_project_value("cursor.hideWhenIdleMs", 1500.into(), false));
    assert_eq!(p(&qt).cursor.hide_when_idle_ms, 1500);
    qt.run(|d| d.set_project_value("cursor.hideWhenIdleMs", Value::Null, false));
    assert_eq!(p(&qt).cursor.hide_when_idle_ms, -1);
    assert!(qt.document.project_map().value("cursor").get("hideWhenIdleMs").is_null());
    // Colours arrive as QVariant(QColor).toString(): #aarrggbb when not opaque. The project
    // keeps the name without alpha.
    qt.run(|d| d.set_project_value("frame.border.color", "#80ff0000".into(), false));
    assert_eq!(qt.document.project_map().value("frame").get("border").get("color").to_string_or(""), "#ff0000");
    qt.run(|d| d.set_project_value("cursor.ringColor", "#00ff00".into(), false));
    assert_eq!(p(&qt).cursor.ring_color.name(), "#00ff00");
    // Something that is no colour falls back to the default, not to the old one.
    qt.run(|d| d.set_project_value("cursor.ringColor", "not a colour".into(), false));
    assert_eq!(p(&qt).cursor.ring_color, Project::default().cursor.ring_color);
    // Undefined (an unset variant) removes the key, so the default comes back.
    qt.run(|d| d.set_project_value("frame.padding", Value::Undefined, false));
    assert_eq!(p(&qt).frame.padding, 0.10);
    // Above the last level anything that is not an object becomes one.
    qt.run(|d| d.set_project_value("name.first", "x".into(), false));
    assert_eq!(qt.document.bundle_name(), "Untitled Recording");
    // Empty parts are skipped.
    qt.run(|d| d.set_project_value(".frame..radius.", 3.into(), false));
    assert_eq!(p(&qt).frame.radius, 3.0);
    // Whole arrays and objects go in too.
    qt.run(|d| {
        d.set_project_value(
            "background.gradient.stops",
            Value::array(vec![Value::array(vec!["#000000".into(), 0.into()]), Value::array(vec!["#ffffff".into(), 1.into()])]),
            false,
        )
    });
    assert_eq!(p(&qt).background.gradient.stops.len(), 2);
    qt.run(|d| d.set_project_value("zoomStyle.spring", Object::from_iter([("mass", 3)]).into(), false));
    assert_eq!(p(&qt).zoom_style.spring.mass, 3.0);
    // Keys left out take the zoom spring's defaults.
    assert_eq!(p(&qt).zoom_style.spring.stiffness, 200.0);
    // The URL paths: the Qt side passes local files as paths.
    assert!(takes_local_file("background.image") && takes_local_file("background.wallpaper"));
    assert!(!takes_local_file("background.type"));
    qt.run(|d| d.set_project_value("background.type", "image".into(), false));
    qt.run(|d| d.set_project_value("background.image", "/tmp/a b%.png".into(), false));
    let background = qt.document.project_map().value("background").to_object();
    assert_eq!(background.value("resolvedImage").to_string_or(""), "file:///tmp/a b%25.png");
    qt.run(|d| d.set_project_value("background.type", "color".into(), false));
    assert_eq!(qt.document.project_map().value("background").get("resolvedImage").to_string_or("?"), "");
}

#[test]
fn project_map_types_match_to_variant_map() {
    let world = World::new("editor-map");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    // The default background is the current Omarchy wallpaper, which depends on the machine.
    qt.run(|d| d.set_project_value("background.type", "color".into(), false));
    qt.seek(1.0);
    qt.run(Document::split_at_playhead);
    let map = qt.document.project_map();
    let clip = map.value("clips").to_array()[1].to_object();
    // in/out are whole numbers here, so toVariantMap gives qlonglong; lower and upper were
    // inserted as doubles.
    assert!(matches!(clip.value("in"), Value::Int(1)));
    assert!(matches!(clip.value("lower"), Value::Double(l) if *l == 1.0));
    assert!(matches!(clip.value("upper"), Value::Double(u) if *u == 2.0));
    assert_eq!(clip.value("layout").to_string_or(""), "default");
    assert_eq!(clip.value("kind").to_string_or(""), "recording");
    assert!(matches!(clip.value("missing"), Value::Bool(false)));
    assert!(!clip.contains("mediaName"));
    assert!(map.value("aspect").is_null());
    assert!(map.value("background").get("image").is_null());
    assert_eq!(map.value("background").get("resolvedImage").to_string_or("?"), "");
    // What project.json holds otherwise.
    let mut json = qt.document.project().to_json();
    let mut plain = map.clone();
    let mut background = plain.value("background").to_object();
    background.remove("resolvedImage");
    plain.insert("background", background);
    plain.insert("clips", json.take("clips"));
    let stored = qt.document.project().to_json();
    for (key, value) in stored.iter() {
        if key != "clips" {
            assert_eq!(plain.value(key), value, "{key}");
        }
    }
    assert_eq!(qt.document.zooms(), stored.value("zooms").to_array());
}

// -------------------------------------------------------------------------------------------
// Selection.

#[test]
fn selection_rules() {
    let world = World::new("editor-selection");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.remove_all_zooms();
    let a = qt.run(|d| d.add_zoom_at(0.0, 1.0));
    let b = qt.run(|d| d.add_zoom_at(1.0, 1.0));
    let clip = qt.clip_id(0);
    qt.effects.clear();
    // A zoom selected, then picking its target from the preview.
    qt.run(|d| d.set_selected_zoom_id(&a));
    qt.run(|d| d.set_picking_zoom_target(true));
    assert!(qt.document.picking_zoom_target());
    assert!(qt.run(|d| d.set_zoom_target_from_preview(1.5, -0.25)));
    assert!(!qt.document.picking_zoom_target());
    assert_eq!(qt.zoom(&a).value("target").to_object(), Object::from_iter([("x", 1.0), ("y", 0.0)]));
    assert!(qt.run(|d| d.set_zoom_target(&a, ZoomTarget::Auto)));
    assert_eq!(qt.zoom(&a).value("target").to_string_or(""), "auto");
    assert!(ZoomTarget::is_auto("AUTO") && !ZoomTarget::is_auto("auto "));
    // Selecting a clip clears the zooms and ends picking.
    qt.run(|d| d.set_picking_zoom_target(true));
    qt.run(|d| d.set_selected_clip_id(&clip));
    assert!(qt.document.selected_zoom_ids().is_empty() && !qt.document.picking_zoom_target());
    // Picking needs a selected zoom.
    qt.run(|d| d.set_picking_zoom_target(true));
    assert!(!qt.document.picking_zoom_target());
    assert!(!qt.run(|d| d.set_zoom_target_from_preview(0.5, 0.5)));
    // Several zooms: unknown and repeated ids dropped, the first primary, the clip cleared.
    qt.run(|d| d.set_selected_zoom_ids(&[b.clone(), "nope".into(), a.clone(), b.clone()]));
    assert_eq!(qt.document.selected_zoom_ids(), [b.clone(), a.clone()]);
    assert_eq!(qt.document.selected_zoom_id(), b);
    assert!(qt.document.selected_clip_id().is_empty());
    let before = qt.effects.len();
    qt.run(|d| d.set_selected_zoom_ids(&[b.clone(), a.clone()]));
    assert_eq!(qt.effects.len(), before);
    // Moving the selection keeps it clear of the others and the ends.
    assert!(qt.run(|d| d.move_selected_zooms(&b, 5.0)));
    assert_eq!(number(&qt.zoom(&b), "end"), 2.0);
    // Removing the selected zoom selects its neighbour.
    qt.run(|d| d.set_selected_zoom_id(&a));
    assert!(qt.run(|d| d.remove_zoom(&a)));
    assert_eq!(qt.document.selected_zoom_id(), b);
    assert_eq!(qt.document.selected_zoom_ids(), std::slice::from_ref(&b));
    // Undo drops selected zooms that are gone.
    qt.run(Document::undo);
    qt.run(|d| d.set_selected_zoom_ids(std::slice::from_ref(&a)));
    qt.run(Document::redo);
    assert_eq!(qt.document.selected_zoom_id(), "");
    assert!(qt.document.selected_zoom_ids().is_empty());
    // Regenerating selects the first automatic zoom, if any.
    qt.run(Document::regenerate_zooms);
    assert_eq!(qt.document.selected_zoom_id(), qt.document.project().zooms.first().map_or("", |z| z.id.as_str()));
}

#[test]
fn merge_remove_and_speed() {
    let world = World::new("editor-clip-ops");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    qt.seek(1.0);
    qt.run(Document::split_at_playhead);
    let (first, second) = (qt.clip_id(0), qt.clip_id(1));
    assert!(!qt.run(|d| d.merge_clip(&first, -1)));
    assert!(qt.run(|d| d.merge_clip(&second, -1)));
    assert_eq!(qt.document.clips().len(), 1);
    assert_eq!(qt.document.selected_clip_id(), first);
    assert_eq!(number(&qt.clip(0), "out"), 2.0);
    // The last clip stays.
    assert!(!qt.run(|d| d.remove_clip(&first)));
    qt.seek(1.0);
    qt.run(Document::split_at_playhead);
    let second = qt.clip_id(1);
    qt.seek(1.9);
    assert!(qt.run(|d| d.remove_clip(&second)));
    assert_eq!(qt.document.selected_clip_id(), first);
    assert_eq!(qt.document.output_position(), 1.0);
    // Only the offered speeds, exactly.
    assert!(!qt.run(|d| d.set_clip_speed(&first, 1.3)));
    assert!(!qt.run(|d| d.set_clip_speed(&first, f64::NAN)));
    assert!(qt.run(|d| d.set_clip_speed(&first, 0.5)));
    assert_eq!(qt.document.duration(), 2.0);
    assert!(qt.run(|d| d.reset_clip_trims(&first)));
    assert_eq!(qt.document.duration(), 4.0);
    assert!(!qt.run(|d| d.trim_clip("nope", 0.0, 1.0)));
}

// -------------------------------------------------------------------------------------------
// Presets.

#[test]
fn preset_names_are_cleaned_like_qt() {
    // Answers from Qt 6.11.2 (a scratch program running cleanPresetName's statements).
    let cases: &[(&str, &str)] = &[
        ("  My preset  ", "My preset"),
        ("a\u{e9}b", "a_b"),
        ("a😀b", "a_b"),
        ("..hidden", "hidden"),
        ("a/b\\c", "a_b_c"),
        ("\u{3000}x\u{a0}", "x"),
        ("\u{85}y\u{200b}", "y_"),
        (". .x", " .x"),
        ("", ""),
        ("...", ""),
    ];
    for &(raw, clean) in cases {
        assert_eq!(clean_preset_name(raw), clean, "{raw:?}");
    }
    assert_eq!(clean_preset_name(&"x".repeat(100)), "x".repeat(80));
}

#[test]
fn presets_save_load_list_and_delete() {
    let world = World::new("editor-presets");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let directory = format!("{}/omareel/presets", world.path("config"));
    assert_eq!(qt.document.presets_directory(), directory);
    assert!(qt.document.preset_names().is_empty());
    qt.run(|d| d.set_project_value("frame.padding", 0.2.into(), false));
    qt.run(|d| d.set_project_value("name", "Kept".into(), false));
    qt.run(|d| d.save_preset("  My preset  "));
    assert_eq!(qt.effects.last(), Some(&Effect::PresetsChanged));
    let saved = std::fs::read_to_string(format!("{directory}/My preset.json")).unwrap();
    assert_eq!(saved, qt.document.style_preset().to_json(crate::compat::qjson::Format::Indented));
    let preset = crate::compat::qjson::object_from_json(saved.as_bytes()).unwrap();
    assert_eq!(
        preset.keys().collect::<Vec<_>>(),
        ["audio", "background", "camera", "cursor", "defaultsVersion", "export", "frame", "version", "zoomStyle"]
    );
    qt.run(|d| d.save_preset("b"));
    qt.run(|d| d.save_preset("A"));
    std::fs::write(format!("{directory}/Upper.JSON"), b"{}").unwrap();
    std::fs::create_dir_all(format!("{directory}/dir.json")).unwrap();
    assert_eq!(qt.document.preset_names(), ["A", "My preset", "Upper", "b"]);
    // A name that cleans to nothing saves nothing and says nothing.
    let before = qt.effects.len();
    qt.run(|d| d.save_preset(" ... "));
    assert_eq!(qt.effects.len(), before);

    // Loading puts the look back and leaves the edits.
    qt.run(|d| d.set_project_value("frame.padding", 0.3.into(), false));
    qt.run(|d| d.set_project_value("name", "Other".into(), false));
    let steps = qt.undo_steps();
    qt.run(|d| d.load_preset("My preset"));
    assert_eq!(qt.document.project().frame.padding, 0.2);
    assert_eq!(qt.document.bundle_name(), "Other");
    assert_eq!(qt.undo_steps(), steps + 1);
    // Always an edit, even when nothing changes.
    qt.run(|d| d.load_preset("My preset"));
    assert_eq!(qt.undo_steps(), steps + 2);
    // An unreadable document applies nothing but is still an edit.
    qt.run(|d| d.load_preset("Upper"));
    assert_eq!(qt.document.project().frame.padding, 0.2);
    // A missing one is an error.
    qt.run(|d| d.load_preset("missing"));
    assert_eq!(qt.effects.last(), Some(&Effect::ErrorOccurred("No such file or directory".into())));

    qt.run(|d| d.delete_preset("My preset"));
    assert_eq!(qt.effects.last(), Some(&Effect::PresetsChanged));
    assert_eq!(qt.document.preset_names(), ["A", "Upper", "b"]);
    qt.run(|d| d.delete_preset("never there"));
    assert_eq!(qt.effects.last(), Some(&Effect::PresetsChanged));

    // A presets directory that can't be made: the save reports why.
    let mut blocked = world.open(&world.bundle("blocked.omareel"));
    std::fs::write(world.path("not-a-dir"), b"").unwrap();
    blocked.document.set_config_location(Some(world.path("not-a-dir")));
    blocked.run(|d| d.save_preset("x"));
    let effects = std::mem::take(&mut blocked.effects);
    assert!(matches!(&effects[effects.len() - 2], Effect::ErrorOccurred(e) if !e.is_empty()));
    assert_eq!(effects.last(), Some(&Effect::PresetsChanged));
}

#[test]
fn loading_an_old_preset_migrates_it() {
    let world = World::new("editor-old-preset");
    let mut qt = world.open(&world.bundle("editor.omareel"));
    let directory = qt.document.presets_directory();
    std::fs::create_dir_all(&directory).unwrap();
    // A version 1 preset with the old shadow defaults and a "square" camera.
    std::fs::write(
        format!("{directory}/old.json"),
        br#"{"version":1,"frame":{"padding":0.05,"shadow":{"enabled":true,"intensity":0.75,"blur":20,"distance":25,"angle":90}},"camera":{"shape":"square"}}"#,
    )
    .unwrap();
    qt.run(|d| d.load_preset("old"));
    let project = qt.document.project();
    assert_eq!(project.frame.padding, 0.05);
    assert_eq!(project.frame.shadow, crate::project::Shadow::default());
    assert_eq!((project.camera.shape.as_str(), project.camera.radius), ("rectangle", 0.0));
}

// -------------------------------------------------------------------------------------------
// Opening bundles.

#[test]
fn opening_reports_what_is_wrong() {
    let world = World::new("editor-open-errors");
    let missing = world.path("missing.omareel");
    assert_eq!(Document::open(&missing, &probes()).err(), Some(format!("Bundle does not exist: {missing}")));
    let bundle = world.bundle("broken.omareel");
    std::fs::remove_file(format!("{bundle}/screen.mp4")).unwrap();
    assert!(Document::open(&bundle, &probes()).is_err());
    std::fs::write(format!("{bundle}/screen.mp4"), b"x").unwrap();
    std::fs::write(format!("{bundle}/project.json"), b"[]").unwrap();
    assert_eq!(Document::open(&bundle, &probes()).err(), Some("no error occurred".into()));
    std::fs::remove_file(format!("{bundle}/project.json")).unwrap();
    std::fs::remove_file(format!("{bundle}/capture.json")).unwrap();
    assert_eq!(Document::open(&bundle, &probes()).err(), Some("No such file or directory".into()));
}

#[test]
fn a_project_without_recording_clips_gets_one() {
    let world = World::new("editor-only-inserts");
    let bundle = world.bundle("inserts-only.omareel");
    let mut project = Project::defaults("Only", 2.0);
    project.clips = vec![Clip { id: "m".into(), in_: 0.0, out: 1.0, speed: 1.0, media: "media/gone.mp4".into(), ..Clip::default() }];
    project.audio.desktop = true;
    project.save(&format!("{bundle}/project.json")).unwrap();
    let qt = world.open(&bundle);
    let clips = &qt.document.project().clips;
    assert_eq!((clips[0].id.as_str(), clips[0].in_, clips[0].out), ("c1", 0.0, 2.0));
    assert_eq!(clips[1].id, "m");
    // Audio the recording does not have is off.
    assert!(!qt.document.project().audio.desktop);
    assert!(!qt.document.inserts()["media/gone.mp4"].is_valid());
    // The screen's thumbnail source was made as the editor opened.
    assert!(qt.document.thumbnails.sources.contains_key("screen"));
}

#[test]
fn default_export_path_and_output_size() {
    let world = World::new("editor-export-path");
    let qt = world.open(&world.bundle("named.omareel"));
    let path = qt.document.default_export_path("gif");
    assert!(path.ends_with("/named.gif"), "{path}");
    assert!(qt.document.default_export_path("mp4").ends_with("/named.mp4"));
    assert!(qt.document.default_export_path("webm").ends_with("/named.mp4"));
    assert_eq!((qt.document.output_width(), qt.document.output_height()), (1280, 720));
}

// -------------------------------------------------------------------------------------------
// With the real ffmpeg and ffprobe, when they are installed (test_editor.cpp skips without them).

/// A JPEG's size from its first start-of-frame marker.
fn jpeg_size(path: &str) -> Option<(u16, u16)> {
    let bytes = std::fs::read(path).ok()?;
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xff {
            return None;
        }
        let marker = bytes[i + 1];
        let length = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if (0xc0..=0xc3).contains(&marker) {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
            return Some((width, height));
        }
        i += 2 + length;
    }
    None
}

#[test]
fn real_ffmpeg_probes_and_builds_frames() {
    use crate::compat::qstandardpaths::find_executable;
    if find_executable("ffmpeg").is_empty() || find_executable("ffprobe").is_empty() {
        eprintln!("ffmpeg/ffprobe unavailable");
        return;
    }
    let world = World::new("editor-ffmpeg");
    let bundle = world.bundle("real.omareel");
    let screen = format!("{bundle}/screen.mp4");
    let made = crate::compat::qprocess::run(
        "ffmpeg",
        &["-y", "-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30", "-t", "1", "-c:v", "libx264", &screen],
        30000,
    );
    if !made.is_some_and(|f| f.succeeded()) {
        eprintln!("ffmpeg could not encode a test video");
        return;
    }
    let info = probe(&screen).unwrap();
    assert_eq!((info.width, info.height, info.fps, info.audio), (320, 180, 30.0, false));
    assert!((info.duration - 1.0).abs() < 0.1);
    // ffprobe's own complaint, trimmed.
    let error = probe(&format!("{bundle}/input.jsonl")).unwrap_err();
    assert!(!error.is_empty() && error == error.trim(), "{error:?}");

    let frame = world.path("cache/0000000300.jpg");
    assert!(build_video_thumbnail(&screen, "", &frame, 300));
    assert_eq!(jpeg_size(&frame), Some((160, 90)));
    // The temporary file is gone.
    let names: Vec<String> = std::fs::read_dir(world.path("cache")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names, ["0000000300.jpg"]);
    // A frame already there is kept.
    let before = std::fs::read(&frame).unwrap();
    assert!(build_video_thumbnail(&screen, "hflip", &frame, 300));
    assert_eq!(std::fs::read(&frame).unwrap(), before);
    assert!(!build_video_thumbnail(&format!("{bundle}/missing.mp4"), "", &world.path("cache/0000000400.jpg"), 400));

    let strip = build_camera_thumbnails(&screen, 1.0, &world.path("camera-cache"));
    assert_eq!(strip.len(), CAMERA_THUMBNAIL_COUNT);
    assert_eq!(strip[0], crate::compat::qurl::from_local_file(&world.path("camera-cache/camera-01.jpg")));
    assert_eq!(jpeg_size(&world.path("camera-cache/camera-08.jpg")), Some((112, 72)));
    assert!(build_camera_thumbnails(&format!("{bundle}/missing.mp4"), 1.0, &world.path("other-cache")).is_empty());
}
