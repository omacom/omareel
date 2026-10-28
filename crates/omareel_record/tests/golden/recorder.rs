//! recorder: `omareel_record::recorder` against `Omareel::Recorder` and the file-static helpers
//! of `src/record/Recorder.cpp` (README "recorder"), every file of `expected/recorder` but
//! `state-files.txt`, which `state_files` regenerates.
//!
//! `helpers.txt` has a `modeName Mode7` row, the C++ called with a `CaptureMode` outside the
//! enum; a Rust `CaptureMode` cannot hold one, so that row is left out of the comparison (the C++
//! falls through to `region` for it, as [`mode_name`]'s match does for `Region`).
//! `self-view-placement.txt` places the bubble with qRound and is checked on x86_64 only.

mod state_files;

use crate::hermetic::*;
use crate::replay::*;
use crate::support::*;
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::{Format, Object, Value};
use omareel_core::compat::qprocess::{ChannelMode, Process, ProcessState};
use omareel_record::capture::*;
use omareel_record::recorder::audio::{AudioCaptureStart, finish_audio_capture, start_audio_capture};
use omareel_record::recorder::files::*;
use omareel_record::recorder::self_view::*;
use omareel_record::recorder::*;
use std::cell::RefCell;

fn mode_text(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Fullscreen => "Fullscreen",
        CaptureMode::Region => "Region",
        CaptureMode::Window => "Window",
    }
}

fn region_cells(region: &CaptureRegion) -> Vec<String> {
    vec![
        quote(&region.monitor_name),
        num(region.x),
        num(region.y),
        num(region.width),
        num(region.height),
        num(region.scale),
        region.physical_width.to_string(),
        region.physical_height.to_string(),
        mode_text(region.mode).to_owned(),
    ]
}

fn compact(object: &Object) -> String {
    Value::Object(object.clone()).to_json(Format::Compact)
}

fn strings(value: &Value) -> Vec<String> {
    value.to_array().iter().map(|item| item.to_string_or("")).collect()
}

fn cases() -> Value {
    read_json("recorder/cases.json")
}

#[test]
fn classify_gsr_exit_txt() {
    let mut out = Text::new();
    for exit_code in [-1, 0, 1, 2, 130, 255] {
        for requested in [false, true] {
            for size in [-1i64, 0, 1, 1048576] {
                for duration in [f64::NAN, f64::NEG_INFINITY, -1.0, -0.0, 0.0, 5e-324, 0.001, 2.5, f64::INFINITY] {
                    let name = match classify_gsr_exit(exit_code, requested, size, duration) {
                        GsrExitClassification::UserStop => "UserStop",
                        GsrExitClassification::ExternalStop => "ExternalStop",
                        GsrExitClassification::Failure => "Failure",
                    };
                    out.row([
                        exit_code.to_string(),
                        boolean(requested).into(),
                        size.to_string(),
                        num(duration),
                        name.into(),
                    ]);
                }
            }
        }
    }
    assert_golden("recorder/classify-gsr-exit.txt", &out.into_string());
}

#[test]
fn even_capture_region_txt() {
    let mut out = Text::new();
    for mode in [CaptureMode::Region, CaptureMode::Fullscreen, CaptureMode::Window] {
        for scale in [1.0, 2.0, 1.5, 1.25, 0.0, -1.0, f64::NAN] {
            for width in [-3, -2, -1, 0, 1, 2, 3, 1520, 1521] {
                for height in [-1, 0, 1, 2, 835, 836] {
                    if mode != CaptureMode::Region && (scale != 2.0 || (width != 1521 && width != 1520)) {
                        continue;
                    }
                    let region = CaptureRegion {
                        monitor_name: "DP-1".into(),
                        x: 10.0,
                        y: 20.0,
                        width: 760.5,
                        height: 417.5,
                        scale,
                        physical_width: width,
                        physical_height: height,
                        mode,
                    };
                    let mut cells = region_cells(&region);
                    cells.push("->".into());
                    cells.extend(region_cells(&even_capture_region(region)));
                    out.row(cells);
                }
            }
        }
    }
    assert_golden("recorder/even-capture-region.txt", &out.into_string());
}

#[derive(Clone)]
struct CameraCall {
    device: String,
    requested_height: i32,
    width: i32,
    height: i32,
    fps: f64,
    first_frame_us: i64,
    backend: String,
    rotation: i32,
    flip: bool,
    shape: String,
    radius: f64,
}

impl Default for CameraCall {
    fn default() -> CameraCall {
        CameraCall {
            device: "/dev/v4l/by-id/usb-Insta360_Link_2-video-index0".into(),
            requested_height: 1080,
            width: 1920,
            height: 1080,
            fps: 30.0,
            first_frame_us: 1234567,
            backend: "ffmpeg-pipe".into(),
            rotation: 90,
            flip: true,
            shape: "round".into(),
            radius: 16.0,
        }
    }
}

// The fps values are the oracle's literals, written as it writes them.
#[allow(clippy::excessive_precision)]
#[test]
fn camera_capture_block_txt() {
    let mut out = Text::new();
    let mut row = |label: &str, call: CameraCall, defaults: bool| {
        let (shape, radius) =
            if defaults { (DEFAULT_CAMERA_SHAPE, DEFAULT_CAMERA_RADIUS) } else { (call.shape.as_str(), call.radius) };
        let block = camera_capture_block(
            &call.device,
            call.requested_height,
            call.width,
            call.height,
            call.fps,
            call.first_frame_us,
            &call.backend,
            call.rotation,
            call.flip,
            shape,
            radius,
        );
        out.row([
            label.to_owned(),
            quote(&call.device),
            call.requested_height.to_string(),
            call.width.to_string(),
            call.height.to_string(),
            num(call.fps),
            call.first_frame_us.to_string(),
            quote(&call.backend),
            call.rotation.to_string(),
            boolean(call.flip).into(),
            if defaults { "<default>".into() } else { quote(&call.shape) },
            if defaults { "<default>".into() } else { num(call.radius) },
            compact(&block),
        ]);
    };
    let base = CameraCall::default;
    row("base", base(), false);
    row("default shape and radius", base(), true);
    for device in ["", "/dev/video0", "Cámara \"1\""] {
        row("device", CameraCall { device: device.into(), ..base() }, false);
    }
    for height in [720, 0, 480, 1440, -720, 721] {
        row("requestedHeight", CameraCall { requested_height: height, ..base() }, false);
    }
    for (width, height) in [(0, 0), (-1, 7), (1280, 720), (i32::MAX, i32::MIN)] {
        row("size", CameraCall { width, height, ..base() }, false);
    }
    for fps in [29.97002997002997, 0.0, -30.0, 1e-300, 60.000000000000007, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        row("fps", CameraCall { fps, ..base() }, false);
    }
    for first in [0, -1, (1i64 << 53) + 1, i64::MAX, i64::MIN] {
        row("firstFrameUs", CameraCall { first_frame_us: first, ..base() }, false);
    }
    for backend in ["", "v4l2"] {
        row("backend", CameraCall { backend: backend.into(), ..base() }, false);
    }
    for rotation in [0, 45, 180, 270, -90, 360] {
        row("rotation", CameraCall { rotation, ..base() }, false);
    }
    row("flipHorizontal", CameraCall { flip: false, ..base() }, false);
    for shape in ["square", "rectangle", "Square", "circle", "", "square ", "rect"] {
        row("shape", CameraCall { shape: shape.into(), ..base() }, false);
    }
    for radius in [0.0, -1.0, 0.5, 80.0, 1e300, f64::NAN, f64::INFINITY] {
        row("radius", CameraCall { radius, ..base() }, false);
    }
    assert_golden("recorder/camera-capture-block.txt", &out.into_string());
}

#[test]
fn microphone_missing_and_peak_db_txt() {
    let cases = cases();
    let microphone = cases.get("microphone").to_object();
    let sources = microphone.value("sources").to_object();
    let mut out = Text::new();
    for device in strings(microphone.value("devices")) {
        for (name, list) in sources.iter() {
            out.row([quote(&device), name.to_owned(), boolean(microphone_missing(&device, &strings(list))).into()]);
        }
    }
    let mut peaks = Text::new();
    let logs = corpus_path("recorder/volumedetect");
    for name in sorted_entries(&logs, false) {
        if !name.ends_with(".log") {
            continue;
        }
        // As daemonMain reads audio.mka.log.
        let text = omareel_core::compat::qstring::from_utf8(&std::fs::read(logs.join(&name)).expect("log"));
        let peak = microphone_peak_db(&text);
        peaks.row([name, num(peak), hex(peak)]);
    }
    assert_no_differences(
        [
            difference(
                "recorder/microphone-missing.txt",
                &read_expected("recorder/microphone-missing.txt"),
                out.into_string().as_bytes(),
            ),
            difference(
                "recorder/microphone-peak-db.txt",
                &read_expected("recorder/microphone-peak-db.txt"),
                peaks.into_string().as_bytes(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
}

#[test]
fn pactl_sources_txt() {
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let directory = corpus_path("pactl/list-short-sources");
    let mut run = |label: &str, answer: Answer| {
        let mut replay = Replay::new(&format!("{}/replay-pactl", h.scratch()));
        replay.answer("pactl", &["list", "short", "sources"], &answer, 0);
        replay.activate(&h);
        out.row([label.to_owned(), quote_list(&pactl_sources())]);
    };
    for name in sorted_entries(&directory, false) {
        run(&name, Answer::stdout(std::fs::read(directory.join(&name)).expect("corpus file")));
    }
    let failing =
        Answer { status: 1, ..Answer::stdout(std::fs::read(directory.join("crafted-dock.txt")).expect("corpus file")) };
    run("<crafted-dock.txt, exit status 1>", failing);
    assert_golden("recorder/pactl-sources.txt", &out.into_string());
}

#[test]
fn discard_timeline_txt() {
    let names = ["abortCapture", "killAudio", "requestCameraStop", "waitForCameraStop", "removeBundle", "removeState"];
    let mut out = Text::new();
    for mask in 0..64 {
        for removes in [true, false] {
            if mask & 16 == 0 && !removes {
                continue;
            }
            let calls = RefCell::new(Vec::<String>::new());
            let push = |name: &str| calls.borrow_mut().push(name.to_owned());
            let mut actions = DiscardActions::default();
            if mask & 1 != 0 {
                actions.abort_capture = Some(Box::new(|| push("abortCapture")));
            }
            if mask & 2 != 0 {
                actions.kill_audio = Some(Box::new(|| push("killAudio")));
            }
            if mask & 4 != 0 {
                actions.request_camera_stop = Some(Box::new(|| push("requestCameraStop")));
            }
            if mask & 8 != 0 {
                actions.wait_for_camera_stop = Some(Box::new(|timeout| push(&format!("waitForCameraStop({timeout})"))));
            }
            if mask & 16 != 0 {
                actions.remove_bundle = Some(Box::new(|| {
                    push("removeBundle");
                    removes
                }));
            }
            if mask & 32 != 0 {
                actions.remove_state = Some(Box::new(|| push("removeState")));
            }
            let present: Vec<&str> = (0..6).filter(|bit| mask & (1 << bit) != 0).map(|bit| names[bit]).collect();
            let result = discard_timeline(&mut actions);
            drop(actions);
            out.row([
                if present.is_empty() { "-".into() } else { present.join(",") },
                if mask & 16 != 0 { boolean(removes).into() } else { "-".into() },
                quote_list(&calls.borrow()),
                boolean(result).into(),
            ]);
        }
    }
    assert_golden("recorder/discard-timeline.txt", &out.into_string());
}

/// The oracle's `readFile(path, &readable)`: the bytes when the file could be opened and read.
fn readable(path: &str) -> Option<Vec<u8>> {
    if std::path::Path::new(path).is_dir() {
        return None;
    }
    std::fs::read(path).ok()
}

#[test]
fn timestamps_txt() {
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let directory = corpus_path("recorder/timestamps");
    let dir = directory.to_str().expect("UTF-8 corpus path");
    for name in sorted_entries(&directory, false) {
        out.row([
            "firstFrameTimestamp".to_owned(),
            name.clone(),
            first_frame_timestamp(&format!("{dir}/{name}")).to_string(),
        ]);
    }
    for (label, path) in [
        ("<missing file>", format!("{dir}/missing.ts")),
        ("<directory>", dir.to_owned()),
        ("<empty path>", String::new()),
    ] {
        out.row(["firstFrameTimestamp".to_owned(), label.into(), first_frame_timestamp(&path).to_string()]);
    }
    let files = format!("{}/timestamps", h.scratch());
    std::fs::create_dir_all(format!("{files}/directory.ts")).expect("mkpath");
    let mut write = |label: &str, path: &str, monotonic: i64| {
        let before = realtime_now_us();
        let ok = write_start_timestamp(path, monotonic);
        let after = realtime_now_us();
        let bytes = match readable(path) {
            Some(bytes) => quote_bytes(&mask_realtime(&bytes, before, after)),
            None => "-".into(),
        };
        out.row([
            "writeStartTimestamp".to_owned(),
            label.into(),
            monotonic.to_string(),
            boolean(ok).into(),
            bytes,
            octal_mode(path),
            first_frame_timestamp(path).to_string(),
        ]);
    };
    write("new file", &format!("{files}/audio.mka.ts"), 123456789);
    write("replaced", &format!("{files}/audio.mka.ts"), 5);
    write("zero", &format!("{files}/zero.ts"), 0);
    write("negative", &format!("{files}/negative.ts"), -5);
    write("maximum", &format!("{files}/maximum.ts"), i64::MAX);
    write("missing directory", &format!("{files}/missing/audio.mka.ts"), 1);
    write("path is a directory", &format!("{files}/directory.ts"), 1);
    assert_golden("recorder/timestamps.txt", &out.into_string());
}

fn device_kind(name: &str) -> DeviceEventKind {
    match name {
        "ButtonDown" => DeviceEventKind::ButtonDown,
        "ButtonUp" => DeviceEventKind::ButtonUp,
        "Scroll" => DeviceEventKind::Scroll,
        "KeyDown" => DeviceEventKind::KeyDown,
        "KeyUp" => DeviceEventKind::KeyUp,
        _ => panic!("unknown device event kind {name}"),
    }
}

fn region_from_json(object: &Object) -> CaptureRegion {
    CaptureRegion {
        x: object.value("x").to_double(0.0),
        y: object.value("y").to_double(0.0),
        scale: object.value("scale").to_double(0.0),
        ..CaptureRegion::default()
    }
}

fn samples_from_json(value: &Value) -> Vec<RawCursorSample> {
    value
        .to_array()
        .iter()
        .map(|item| {
            let triple = item.to_array();
            let at = |i: usize| triple.get(i).cloned().unwrap_or_default();
            RawCursorSample {
                monotonic_us: at(0).to_integer(0),
                logical_position: PointF::new(at(1).to_double(0.0), at(2).to_double(0.0)),
            }
        })
        .collect()
}

#[test]
fn input_log_txt() {
    let h = Hermetic::new("recorder");
    let cases = cases();
    let inputs = cases.get("inputLog").to_object();
    let files = format!("{}/input-log", h.scratch());
    std::fs::create_dir_all(&files).expect("mkpath");
    let mut out = Text::new();
    let dump = |out: &mut Text, path: &str, result: Result<(), String>| {
        let (ok, error) = match result {
            Ok(()) => (true, "<untouched>".to_owned()),
            Err(error) => (false, error),
        };
        out.row(["ok".to_owned(), boolean(ok).into(), quote(&h.sanitize(&error))]);
        let Some(bytes) = readable(path).filter(|_| !path.is_empty()) else {
            out.row(["file", "-"]);
            return;
        };
        out.row(["file".to_owned(), bytes.len().to_string(), octal_mode(path)]);
        let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if i + 1 == lines.len() && line.is_empty() {
                break;
            }
            out.row(["line".to_owned(), String::from_utf8_lossy(line).into_owned()]);
        }
    };
    for (name, input) in inputs.iter() {
        let input = input.to_object();
        let devices: Vec<DeviceEvent> = input
            .value("devices")
            .to_array()
            .iter()
            .map(|item| {
                let object = item.to_object();
                DeviceEvent {
                    monotonic_us: object.value("t").to_integer(0),
                    kind: device_kind(&object.value("kind").to_string_or("")),
                    button: object.value("button").to_string_or(""),
                    dx: object.value("dx").to_int(0),
                    dy: object.value("dy").to_int(0),
                    key_code: object.value("code").to_int(0),
                    key_name: object.value("name").to_string_or(""),
                    key_label: object.value("label").to_string_or(""),
                    modifiers: strings(object.value("mods")),
                }
            })
            .collect();
        let path = format!("{files}/{name}.jsonl");
        let result = write_input_log(
            &path,
            &samples_from_json(input.value("cursor")),
            &devices,
            &region_from_json(&input.value("region").to_object()),
        );
        out.section(name);
        dump(&mut out, &path, result);
    }
    let one = [RawCursorSample { monotonic_us: 1, logical_position: PointF::new(1.0, 1.0) }];
    for (label, path) in
        [("<missing directory>", format!("{files}/missing/input.jsonl")), ("<path is a directory>", files.clone())]
    {
        let result = write_input_log(&path, &one, &[], &CaptureRegion::default());
        out.section(label);
        let shown = if std::path::Path::new(&path).is_dir() { String::new() } else { path };
        dump(&mut out, &shown, result);
    }
    assert_golden("recorder/input-log.txt", &out.into_string());
}

fn object(members: Vec<(&str, Value)>) -> Object {
    let mut object = Object::new();
    for (key, value) in members {
        object.insert(key, value);
    }
    object
}

#[test]
fn write_json_txt() {
    let h = Hermetic::new("recorder");
    let files = format!("{}/write-json", h.scratch());
    std::fs::create_dir_all(&files).expect("mkpath");
    let recording = read_json("recorder/state/recording.json").to_object();
    let objects = [
        ("empty", Object::new()),
        ("recording-json", object(vec![("overlay_exclusion", "plugin".into())])),
        ("state", recording),
        (
            "special-values",
            object(vec![
                ("nan", f64::NAN.into()),
                ("inf", f64::INFINITY.into()),
                ("negative-zero", (-0.0).into()),
                ("big", 9007199254740993i64.into()),
                ("tiny", 5e-324.into()),
                ("text", "line\nbreak \"q\" \\ é\u{1F3AC}".into()),
                (
                    "nested",
                    object(vec![("a", Value::array(vec![1.into(), 2.5.into(), Value::Null, true.into()]))]).into(),
                ),
            ]),
        ),
    ];
    let mut out = Text::new();
    let mut reports = Vec::new();
    for (name, object) in &objects {
        let path = format!("{files}/{name}.json");
        let (ok, error) = match write_json(&path, object) {
            Ok(()) => (true, "<untouched>".to_owned()),
            Err(error) => (false, error),
        };
        out.row([(*name).to_owned(), boolean(ok).into(), quote(&h.sanitize(&error)), octal_mode(&path)]);
        let golden = format!("recorder/write-json/{name}.json");
        reports.extend(difference(&golden, &read_expected(&golden), &std::fs::read(&path).unwrap_or_default()));
    }
    let existing = format!("{files}/existing-0600.json");
    write_file(&existing, "old");
    chmod(&existing, 0o600);
    let version = object(vec![("version", 1.into())]);
    for (label, path) in [
        ("<existing file, mode 0600>", existing.clone()),
        ("<missing directory>", format!("{files}/missing/capture.json")),
        ("<path is a directory>", files.clone()),
        ("<empty path>", String::new()),
    ] {
        let (ok, error) = match write_json(&path, &version) {
            Ok(()) => (true, "<untouched>".to_owned()),
            Err(error) => (false, error),
        };
        let bytes = if path.is_empty() { None } else { readable(&path) };
        out.row([
            label.to_owned(),
            boolean(ok).into(),
            quote(&h.sanitize(&error)),
            if path.is_empty() { "-".into() } else { octal_mode(&path) },
            bytes.map_or("-".into(), |bytes| quote_bytes(&bytes)),
        ]);
    }
    reports.extend(difference(
        "recorder/write-json.txt",
        &read_expected("recorder/write-json.txt"),
        out.into_string().as_bytes(),
    ));
    assert_no_differences(reports);
}

#[test]
fn recorder_output_txt() {
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let directory = corpus_path("recorder/output");
    for name in sorted_entries(&directory, false) {
        let line = last_non_empty_line(&std::fs::read(directory.join(&name)).expect("corpus file"));
        out.row(["lastNonEmptyLine".to_owned(), name, quote(&line)]);
    }
    out.row(["lastNonEmptyLine".to_owned(), "<empty>".into(), quote(&last_non_empty_line(b""))]);
    // Each chunk is what one `cat <file>` printed, read as daemonMain reads gpu-screen-recorder:
    // merged channels, drained into one tail.
    let sequences: [(&str, &[&str]); 6] = [
        ("one chunk", &["gsr-error.txt"]),
        ("big chunk", &["big.txt"]),
        ("three chunks", &["gsr-error.txt", "big.txt", "no-newline.txt"]),
        ("growing past the limit", &["big.txt", "big.txt", "unicode.txt"]),
        ("empty chunks", &["empty.txt", "crlf.txt", "empty.txt"]),
        ("bytes", &["invalid-utf8.bin", "nul.bin", "carriage-return.txt"]),
    ];
    let debug_path = format!("{}/omareel.log", h.scratch());
    for (label, chunks) in sequences {
        for debug in ["open", "closed", "none"] {
            out.section(&format!("{label} debugLog={debug}"));
            let _ = std::fs::remove_file(&debug_path);
            // QFile::open(WriteOnly | Append); "closed" is a QFile that was never opened.
            let mut log = (debug == "open").then(|| {
                std::fs::OpenOptions::new().append(true).create(true).open(&debug_path).expect("open the debug log")
            });
            let mut tail = Vec::new();
            for chunk in chunks {
                let mut process = Process::new();
                process.set_process_channel_mode(ChannelMode::Merged);
                process.start("cat", &[directory.join(chunk).to_str().expect("UTF-8 path")]);
                assert!(process.wait_for_finished(10000), "cat did not finish");
                drain_recorder_output(&mut process, &mut tail, log.as_mut().map(|f| f as &mut dyn std::io::Write));
                out.row([
                    "drain".to_owned(),
                    (*chunk).to_owned(),
                    tail.len().to_string(),
                    quote_bytes(&tail),
                    quote(&last_non_empty_line(&tail)),
                ]);
            }
            drop(log);
            out.row(["debugLog".to_owned(), readable(&debug_path).map_or("-".into(), |bytes| quote_bytes(&bytes))]);
        }
    }
    let _ = std::fs::remove_file(&debug_path);
    assert_golden("recorder/recorder-output.txt", &out.into_string());
}

#[test]
fn monitor_layouts_txt() {
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let directory = corpus_path("hyprctl/monitors");
    let mut dump = |label: &str, answer: Answer, hyprctl: bool| {
        let mut replay = Replay::new(&format!("{}/replay-monitors", h.scratch()));
        replay.answer("hyprctl", &["-j", "monitors"], &answer, 0);
        replay.activate(&h);
        let mut environment = ScopedEnvironment::new(&h);
        if !hyprctl {
            environment.set("PATH", shim_directory(&h.scratch(), "empty", &[]));
        }
        out.section(label);
        for layout in monitor_layouts() {
            out.row([
                quote(&layout.name),
                num(layout.geometry.x),
                num(layout.geometry.y),
                num(layout.geometry.width),
                num(layout.geometry.height),
                num(layout.reserved.left),
                num(layout.reserved.top),
                num(layout.reserved.right),
                num(layout.reserved.bottom),
            ]);
        }
    };
    for name in sorted_entries(&directory, false) {
        dump(&name, Answer::stdout(std::fs::read(directory.join(&name)).expect("corpus file")), true);
    }
    let two = std::fs::read(directory.join("crafted-two.json")).expect("corpus file");
    dump("<crafted-two.json, exit status 1>", Answer { status: 1, ..Answer::stdout(two) }, true);
    dump("<hyprctl not on PATH>", Answer::default(), false);
    assert_golden("recorder/monitor-layouts.txt", &out.into_string());
}

#[cfg(target_arch = "x86_64")]
#[test]
fn self_view_placement_txt() {
    let h = Hermetic::new("recorder");
    let cases = cases();
    let mut out = Text::new();
    for (label, input) in cases.get("selfViewPlacement").to_object().iter() {
        let input = input.to_object();
        let mut replay = Replay::new(&format!("{}/replay-placement", h.scratch()));
        let answer = match input.value("monitors").as_str() {
            Some(file) => Answer::stdout(read_corpus(file)),
            None => Answer::status(1),
        };
        replay.answer("hyprctl", &["-j", "monitors"], &answer, 0);
        replay.activate(&h);
        let options = RecordOptions {
            webcam: input.value("webcam").to_bool(false),
            self_view: input.value("selfView").to_bool(false),
            self_view_size: input.value("size").to_string_or(""),
            ..RecordOptions::default()
        };
        let capture =
            CaptureRegion { monitor_name: input.value("monitor").to_string_or(""), ..CaptureRegion::default() };
        let (x, y) = (input.value("x").to_double(0.0), input.value("y").to_double(0.0));
        let preferences = omareel_core::recording_preferences::RecordingPreferences {
            self_view_x: x,
            self_view_y: y,
            ..Default::default()
        };
        let placement = self_view_placement(&options, &capture, &preferences);
        out.row([
            label.to_owned(),
            boolean(options.webcam).into(),
            boolean(options.self_view).into(),
            quote(&options.self_view_size),
            quote(&capture.monitor_name),
            num(x),
            num(y),
            boolean(placement.visible).into(),
            boolean(placement.safe).into(),
            quote(&placement.monitor),
            quote(&placement.size_name),
            placement.pixels.to_string(),
            placement.position.x.to_string(),
            placement.position.y.to_string(),
            replay.calls().len().to_string(),
        ]);
    }
    for size in ["S", "M", "L", "s", "XL", ""] {
        out.row(["selfViewPixels".to_owned(), quote(size), self_view_pixels(size).to_string()]);
    }
    assert_golden("recorder/self-view-placement.txt", &out.into_string());
}

#[test]
fn helpers_txt() {
    let cases = cases();
    let mut out = Text::new();
    let value_after_cases = cases.get("valueAfter").to_object();
    for arguments in value_after_cases.value("arguments").to_array() {
        let arguments = strings(&arguments);
        for name in strings(value_after_cases.value("names")) {
            out.row([
                "valueAfter".to_owned(),
                quote_list(&arguments),
                quote(&name),
                quote(&value_after(&arguments, &name)),
            ]);
        }
    }
    for mode in [CaptureMode::Fullscreen, CaptureMode::Region, CaptureMode::Window] {
        out.row(["modeName".to_owned(), mode_text(mode).into(), quote(mode_name(mode))]);
    }
    let position = cases.get("positionObject").to_object();
    for region in position.value("regions").to_array() {
        for point in position.value("points").to_array() {
            let capture = region_from_json(&region.to_object());
            let point = point.to_array();
            let logical = PointF::new(point[0].to_double(0.0), point[1].to_double(0.0));
            out.row([
                "positionObject".to_owned(),
                num(logical.x),
                num(logical.y),
                num(capture.x),
                num(capture.y),
                num(capture.scale),
                compact(&position_object(logical, &capture)),
            ]);
        }
    }
    let at = cases.get("cursorAt").to_object();
    for (name, samples) in at.value("samples").to_object().iter() {
        let samples = samples_from_json(samples);
        for time in at.value("times").to_array() {
            let time = time.to_integer(0);
            let point = cursor_at(&samples, time);
            out.row(["cursorAt".to_owned(), name.to_owned(), time.to_string(), num(point.x), num(point.y)]);
        }
    }
    // The Mode7 row (a CaptureMode outside the enum) has no Rust counterpart.
    let golden = keep_lines(&read_expected("recorder/helpers.txt"), |line| line != b"modeName\tMode7\t\"region\"");
    assert_eq!(golden.len() + "modeName\tMode7\t\"region\"\n".len(), read_expected("recorder/helpers.txt").len());
    assert_same("recorder/helpers.txt", &golden, out.into_string().as_bytes());
}

struct Start {
    label: &'static str,
    desktop: bool,
    microphone: bool,
    device: &'static str,
    /// The answer to `pactl get-default-sink`: none, or (status, stdout).
    sink: Option<(i32, &'static str)>,
    source: Option<(i32, &'static str)>,
    ffmpeg: bool,
    /// The audio path, when not `bundle/audio.mka`.
    path: Option<&'static str>,
}

const START: Start = Start {
    label: "",
    desktop: false,
    microphone: false,
    device: "default_input",
    sink: None,
    source: None,
    ffmpeg: true,
    path: None,
};

struct Finish {
    label: &'static str,
    audio: Option<&'static [u8]>,
    mux_status: i32,
    creates: bool,
    screen_started_us: i64,
    audio_started_us: i64,
    screen_duration_us: i64,
    running: bool,
}

const FINISH: Finish = Finish {
    label: "",
    audio: Some(b"audio"),
    mux_status: 0,
    creates: true,
    screen_started_us: 1000000,
    audio_started_us: 1250000,
    screen_duration_us: 2000000,
    running: false,
};

#[test]
fn audio_capture_txt() {
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let files = format!("{}/audio", h.scratch());
    let audio_path = format!("{files}/bundle/audio.mka");
    let yeti = "alsa_input.usb-Blue_Yeti-00.analog-stereo";
    let starts = [
        Start {
            label: "desktop",
            desktop: true,
            sink: Some((0, "alsa_output.pci-0000_00_1f.3.analog-stereo\n")),
            ..START
        },
        Start { label: "microphone", microphone: true, device: yeti, ..START },
        Start { label: "default microphone", microphone: true, source: Some((0, "  alsa_input.default \n")), ..START },
        Start { label: "both", desktop: true, microphone: true, device: yeti, sink: Some((0, "sink")), ..START },
        Start {
            label: "both defaults",
            desktop: true,
            microphone: true,
            sink: Some((0, "sink")),
            source: Some((0, "source")),
            ..START
        },
        Start { label: "no default sink", desktop: true, sink: Some((1, "sink")), ..START },
        Start { label: "empty default source", microphone: true, source: Some((0, "\n")), ..START },
        Start {
            label: "sink fails, microphone left",
            desktop: true,
            microphone: true,
            device: "mic",
            sink: Some((1, "")),
            ..START
        },
        Start {
            label: "source fails, desktop left",
            desktop: true,
            microphone: true,
            sink: Some((0, "sink")),
            source: Some((1, "source")),
            ..START
        },
        Start { label: "nothing requested", ..START },
        Start { label: "empty device", microphone: true, device: "", ..START },
        Start { label: "ffmpeg not on PATH", desktop: true, sink: Some((0, "sink")), ffmpeg: false, ..START },
        Start {
            label: "log file cannot be created",
            desktop: true,
            sink: Some((0, "sink")),
            path: Some("missing/audio.mka"),
            ..START
        },
    ];
    for start in &starts {
        remove_path(&files);
        std::fs::create_dir_all(format!("{files}/bundle")).expect("mkpath");
        let mut replay = Replay::new(&format!("{}/replay-audio", h.scratch()));
        if let Some((status, sink)) = start.sink {
            replay.answer("pactl", &["get-default-sink"], &Answer { status, ..Answer::stdout(sink) }, 0);
        }
        if let Some((status, source)) = start.source {
            replay.answer("pactl", &["get-default-source"], &Answer { status, ..Answer::stdout(source) }, 0);
        }
        replay.answer_any("ffmpeg", &Answer { err: b"on stderr\n".to_vec(), ..Answer::stdout("ffmpeg shim log\n") });
        replay.activate(&h);
        let mut environment = ScopedEnvironment::new(&h);
        if !start.ffmpeg {
            environment.set("PATH", shim_directory(&h.scratch(), "pactl", &["pactl"]));
        }
        let options = RecordOptions {
            desktop_audio: start.desktop,
            microphone_audio: start.microphone,
            microphone_device: start.device.into(),
            ..RecordOptions::default()
        };
        let path = start.path.map_or(audio_path.clone(), |path| format!("{files}/{path}"));
        let mut process = Process::new();
        let before = realtime_now_us();
        let started = start_audio_capture(&mut process, &options, &path);
        process.wait_for_finished(10000);
        let after = realtime_now_us();
        out.section(&format!("start {}", start.label));
        out.row([
            "result".to_owned(),
            boolean(started.started).into(),
            match started.started_us {
                None => "untouched".into(),
                Some(us) if us > 0 => "positive".into(),
                Some(us) => us.to_string(),
            },
        ]);
        replay.dump_calls(&h, &mut out);
        out.row(["log".to_owned(), readable(&format!("{path}.log")).map_or("-".into(), |log| quote_bytes(&log))]);
        // The monotonic column is startedUs, a reading of the machine's clock.
        let ts = readable(&format!("{path}.ts")).map(|ts| {
            let marker = format!("\n{}\t", started.started_us.unwrap_or(-7));
            let replaced = String::from_utf8_lossy(&ts).replace(&marker, "\n{startedUs}\t");
            quote_bytes(&mask_realtime(replaced.as_bytes(), before, after))
        });
        out.row(["ts".to_owned(), ts.unwrap_or_else(|| "-".into())]);
        let parent = std::path::Path::new(&path).parent().expect("a directory").to_str().expect("UTF-8").to_owned();
        out.row(["bundle".to_owned(), directory_listing(&parent)]);
    }

    let finishes = [
        Finish { label: "muxes", audio: Some(b"audio bytes"), ..FINISH },
        Finish { label: "audio empty", audio: Some(b""), ..FINISH },
        Finish { label: "audio missing", audio: None, ..FINISH },
        Finish { label: "mux fails", mux_status: 1, ..FINISH },
        Finish { label: "mux writes nothing", creates: false, ..FINISH },
        Finish { label: "no screen duration", screen_duration_us: 0, ..FINISH },
        Finish {
            label: "audio started first",
            screen_started_us: 2000000,
            audio_started_us: 1999999,
            screen_duration_us: 1,
            ..FINISH
        },
        Finish {
            label: "long recording",
            screen_started_us: 0,
            audio_started_us: 3600000000,
            screen_duration_us: 7200000000,
            ..FINISH
        },
        Finish {
            label: "negative duration",
            screen_started_us: 5,
            audio_started_us: 3,
            screen_duration_us: -1,
            ..FINISH
        },
        Finish {
            label: "stops a running recorder",
            audio_started_us: 1000000,
            screen_duration_us: 500000,
            running: true,
            ..FINISH
        },
    ];
    for finish in &finishes {
        remove_path(&files);
        std::fs::create_dir_all(&files).expect("mkpath");
        let screen = format!("{files}/screen-video.mp4");
        let audio = format!("{files}/audio.mka");
        let output = format!("{files}/screen.mp4");
        write_file(&screen, "screen");
        if let Some(bytes) = finish.audio {
            write_file(&audio, bytes);
        }
        let mut replay = Replay::new(&format!("{}/replay-mux", h.scratch()));
        replay.answer_any(
            "ffmpeg",
            &Answer {
                status: finish.mux_status,
                creates: finish.creates.then(|| b"muxed".to_vec()),
                ..Answer::default()
            },
        );
        replay.activate(&h);
        let mut recorder = Process::new();
        if finish.running {
            recorder.start("sleep", &["30"]);
            assert!(recorder.wait_for_started(5000), "sleep did not start");
        }
        // No log beside the audio: it is placed by when its recorder started.
        let start =
            AudioCaptureStart { started: true, started_us: Some(finish.audio_started_us), realtime_offset_us: 0 };
        let ok = finish_audio_capture(
            &mut recorder,
            &screen,
            &audio,
            &output,
            finish.screen_started_us,
            &start,
            finish.screen_duration_us,
        )
        .muxed;
        out.section(&format!("finish {}", finish.label));
        out.row([
            "arguments".to_owned(),
            finish.screen_started_us.to_string(),
            finish.audio_started_us.to_string(),
            finish.screen_duration_us.to_string(),
        ]);
        let state = if !finish.running {
            "recorder not started"
        } else if recorder.state() == ProcessState::NotRunning {
            "recorder stopped"
        } else {
            "recorder running"
        };
        out.row(["result".to_owned(), boolean(ok).into(), state.into()]);
        replay.dump_calls(&h, &mut out);
        out.row(["output".to_owned(), readable(&output).map_or("-".into(), |bytes| quote_bytes(&bytes))]);
        if recorder.state() != ProcessState::NotRunning {
            recorder.kill();
            recorder.wait_for_finished(1000);
        }
    }
    remove_path(&files);
    assert_golden("recorder/audio-capture.txt", &out.into_string());
}
