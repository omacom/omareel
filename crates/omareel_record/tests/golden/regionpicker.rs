//! regionpicker: `region_picker` against `src/record/RegionPicker.cpp` (README "regionpicker"),
//! with hyprctl, gpu-screen-recorder, slurp and omarchy-capture-region answered by the replay
//! shims. The oracle has no compositor, so `ScreenCapture::isSupported()` was false; the tests
//! give `capturable_monitor` and `pick` that answer.

use crate::hermetic::Hermetic;
use crate::replay::{Answer, Replay, ScopedEnvironment, shim_directory};
use crate::support::*;
use omareel_core::compat::qjson::Value;
use omareel_record::capture::{CaptureMode, CaptureRegion};
use omareel_record::region_picker::{self, Monitor};

/// `ScreenCapture::isSupported()` without a compositor.
fn not_supported() -> bool {
    false
}

fn mode_text(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Fullscreen => "Fullscreen",
        CaptureMode::Region => "Region",
        CaptureMode::Window => "Window",
    }
}

fn mode_from(name: &str) -> CaptureMode {
    match name {
        "Fullscreen" => CaptureMode::Fullscreen,
        "Window" => CaptureMode::Window,
        _ => CaptureMode::Region,
    }
}

fn monitor_cells(monitor: &Monitor) -> Vec<String> {
    vec![
        quote(&monitor.name),
        num(monitor.x),
        num(monitor.y),
        num(monitor.logical_width),
        num(monitor.logical_height),
        num(monitor.scale),
        monitor.physical_width.to_string(),
        monitor.physical_height.to_string(),
        boolean(monitor.focused).into(),
    ]
}

fn strings(value: &Value) -> Vec<String> {
    value.to_array().iter().map(|item| item.to_string_or("")).collect()
}

fn cases() -> Value {
    read_json("regionpicker/cases.json")
}

/// One scenario's world (the oracle's `World`): replayed answers, the `PATH` it names,
/// `OMAREEL_CAPTURE` and a settings.json with its captureBackend in a config directory of its own.
struct World<'a> {
    replay: Replay,
    _environment: ScopedEnvironment<'a>,
}

impl<'a> World<'a> {
    fn new(h: &'a Hermetic, scenario: &Value) -> World<'a> {
        let mut replay = Replay::new(&format!("{}/replay", h.scratch()));
        replay.answer_json(scenario.get("answers"));
        let mut environment = ScopedEnvironment::new(h);
        let scenario_object = scenario.to_object();
        if scenario_object.contains("programs") {
            let programs = strings(scenario.get("programs"));
            let programs: Vec<&str> = programs.iter().map(String::as_str).collect();
            environment.set("PATH", shim_directory(&h.scratch(), "scenario", &programs));
        }
        environment.unset("OMAREEL_CAPTURE");
        for (key, value) in scenario.get("env").to_object().iter() {
            environment.set(key, value.to_string_or(""));
        }
        let config = format!("{}/config", h.scratch());
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("mkpath config");
        if scenario_object.contains("captureBackend") {
            crate::hermetic::write_file(
                &format!("{config}/omareel/settings.json"),
                format!("{{\"captureBackend\": \"{}\"}}\n", scenario.get("captureBackend").to_string_or("")),
            );
        }
        environment.set("XDG_CONFIG_HOME", &config);
        replay.activate(h);
        World { replay, _environment: environment }
    }
}

/// `monitors()` of a corpus file replayed as `hyprctl -j monitors`.
fn monitors_from(h: &Hermetic, file: &str) -> Vec<Monitor> {
    let mut replay = Replay::new(&format!("{}/replay-monitors", h.scratch()));
    replay.answer("hyprctl", &["-j", "monitors"], &Answer::stdout(read_corpus(file)), 0);
    replay.activate(h);
    region_picker::monitors().unwrap_or_default()
}

#[test]
// The click grid's values are written as the oracle has them.
#[allow(clippy::excessive_precision)]
fn parse_capture_options_and_clicks_and_rects() {
    let directory = corpus_path("gpu-screen-recorder/list-capture-options");
    let mut out = Text::new();
    for name in sorted_entries(&directory, false) {
        let text = omareel_core::compat::qstring::from_utf8(&std::fs::read(directory.join(&name)).expect("corpus"));
        out.row([name, quote_list(&region_picker::parse_capture_options(&text))]);
    }
    let options = out.into_string();

    let mut out = Text::new();
    let values = [
        -1.0,
        -0.0,
        0.0,
        1e-9,
        1.0,
        2.0,
        4.0,
        4.4,
        4.4721359549995796,
        4.4721359549995805,
        5.0,
        19.0,
        19.999,
        20.0,
        21.0,
        1e300,
        f64::INFINITY,
        f64::NAN,
    ];
    for width in values {
        for height in values {
            out.row([num(width), num(height), boolean(region_picker::is_click_selection(width, height)).into()]);
        }
    }
    let clicks = out.into_string();

    let mut out = Text::new();
    for text in strings(cases().get("rects")) {
        let (ok, (x, y, w, h)) = match region_picker::parse_rect(&text) {
            Some(rect) => (true, rect),
            None => (false, (-7.0, -7.0, -7.0, -7.0)),
        };
        out.row([quote(&text), boolean(ok).into(), num(x), num(y), num(w), num(h)]);
    }
    let rects = out.into_string();

    assert_no_differences(
        [
            difference(
                "regionpicker/parse-capture-options.txt",
                &read_expected("regionpicker/parse-capture-options.txt"),
                options.as_bytes(),
            ),
            difference(
                "regionpicker/is-click-selection.txt",
                &read_expected("regionpicker/is-click-selection.txt"),
                clicks.as_bytes(),
            ),
            difference("regionpicker/parse-rect.txt", &read_expected("regionpicker/parse-rect.txt"), rects.as_bytes()),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
}

#[test]
fn monitors_monitor_for_window_rectangles_and_snap() {
    let h = Hermetic::new("regionpicker");
    let cases = cases();

    let mut out = Text::new();
    let directory = corpus_path("hyprctl/monitors");
    let dump = |out: &mut Text, label: &str, answer: &Answer, on_path: bool| {
        let mut replay = Replay::new(&format!("{}/replay-monitors", h.scratch()));
        replay.answer("hyprctl", &["-j", "monitors"], answer, 0);
        replay.activate(&h);
        let mut environment = ScopedEnvironment::new(&h);
        if !on_path {
            environment.set("PATH", shim_directory(&h.scratch(), "empty", &[]));
        }
        let (values, error) = match region_picker::monitors() {
            Ok(values) => (values, "<untouched>".to_owned()),
            Err(error) => (Vec::new(), error),
        };
        out.section(label);
        out.row(["error".to_owned(), quote(&error)]);
        for monitor in &values {
            out.row(monitor_cells(monitor));
        }
    };
    for name in sorted_entries(&directory, false) {
        dump(&mut out, &name, &Answer::stdout(std::fs::read(directory.join(&name)).expect("corpus")), true);
    }
    let failing = Answer {
        out: std::fs::read(directory.join("crafted-two.json")).expect("corpus"),
        err: b"Couldn't connect to /run/user/1000/hypr/x/.socket.sock. (3)\n".to_vec(),
        status: 1,
        ..Answer::default()
    };
    dump(&mut out, "<crafted-two.json, exit status 1, stderr>", &failing, true);
    dump(&mut out, "<exit status 3, no output>", &Answer::status(3), true);
    dump(&mut out, "<hyprctl not on PATH>", &Answer::default(), false);
    let monitors = out.into_string();

    let mut out = Text::new();
    let input = cases.get("monitorFor");
    for set in strings(input.get("sets")) {
        let values = monitors_from(&h, &set);
        for rect in input.get("rects").to_array() {
            let r: Vec<f64> = rect.to_array().iter().map(|v| v.to_double(0.0)).collect();
            let (x, y, w, hh) = (r[0], r[1], r[2], r[3]);
            let found = region_picker::monitor_for(&values, x, y, w, hh);
            out.row([
                set.clone(),
                num(x),
                num(y),
                num(w),
                num(hh),
                found.map_or("-".into(), |index| index.to_string()),
                found.map_or("-".into(), |index| quote(&values[index].name)),
            ]);
        }
    }
    let monitor_for = out.into_string();

    let mut out = Text::new();
    for pair in cases.get("rectangles").to_array() {
        let mut replay = Replay::new(&format!("{}/replay-windows", h.scratch()));
        let add = |request: &str, file: &Value| {
            let answer = match file.as_str() {
                Some(file) => Answer::stdout(read_corpus(file)),
                None => Answer::status(1),
            };
            replay.answer("hyprctl", &["-j", request], &answer, 0);
        };
        add("activeworkspace", pair.get("workspace"));
        add("clients", pair.get("clients"));
        replay.activate(&h);
        let mut error = None;
        let rectangles = region_picker::window_rectangles(&mut error);
        let label = |value: &Value| value.as_str().map_or("<exit 1>".to_owned(), str::to_owned);
        out.section(&format!("workspace={} clients={}", label(pair.get("workspace")), label(pair.get("clients"))));
        out.row(["result".to_owned(), quote(&rectangles)]);
        out.row(["error".to_owned(), quote(error.as_deref().unwrap_or("<untouched>"))]);
        replay.dump_calls(&h, &mut out);
    }
    let windows = out.into_string();

    let mut out = Text::new();
    let snap = cases.get("snap");
    for rectangles in strings(snap.get("rectangles")) {
        for point in snap.get("points").to_array() {
            let point = point.to_array();
            let (px, py) = (point[0].to_double(0.0), point[1].to_double(0.0));
            let (found, (x, y, w, hh)) = match region_picker::snap_to_window(&rectangles, px, py) {
                Some(rect) => (true, rect),
                None => (false, (-7.0, -7.0, -7.0, -7.0)),
            };
            out.row([quote(&rectangles), num(px), num(py), boolean(found).into(), num(x), num(y), num(w), num(hh)]);
        }
    }
    let snaps = out.into_string();

    assert_no_differences(
        [
            ("regionpicker/monitors.txt", monitors),
            ("regionpicker/monitor-for.txt", monitor_for),
            ("regionpicker/window-rectangles.txt", windows),
            ("regionpicker/snap-to-window.txt", snaps),
        ]
        .iter()
        .filter_map(|(name, text)| difference(name, &read_expected(name), text.as_bytes()))
        .collect(),
    );
}

#[test]
fn capturable_monitor_and_pick() {
    let h = Hermetic::new("regionpicker");
    let cases = cases();

    let mut out = Text::new();
    for (label, scenario) in cases.get("capturable").to_object().iter() {
        let values = monitors_from(&h, &scenario.get("monitors").to_string_or(""));
        let requested_name = scenario.get("requested").to_string_or("");
        let requested = values
            .iter()
            .rposition(|monitor| monitor.name == requested_name)
            .unwrap_or_else(|| panic!("capturable scenario {label} names no monitor"));
        let world = World::new(&h, scenario);
        let mut note = None;
        let result = region_picker::capturable_monitor(&values, requested, &mut note, &not_supported);
        out.section(label);
        out.row([
            "result".to_owned(),
            quote(&values[requested].name),
            quote(&values[result].name),
            if result == requested { "requested".into() } else { result.to_string() },
        ]);
        out.row(["note".to_owned(), quote(note.as_deref().unwrap_or("<untouched>"))]);
        world.replay.dump_calls(&h, &mut out);
    }
    let capturable = out.into_string();

    let mut out = Text::new();
    for (label, scenario) in cases.get("pick").to_object().iter() {
        let world = World::new(&h, scenario);
        let mode = mode_from(&scenario.get("mode").to_string_or(""));
        let picked = region_picker::pick(mode, &not_supported);
        let region =
            picked.region.clone().unwrap_or(CaptureRegion { monitor_name: "<untouched>".into(), ..CaptureRegion::default() });
        out.section(label);
        out.row(["pick", mode_text(mode), boolean(picked.region.is_some())]);
        out.row(["error".to_owned(), quote(&h.sanitize(picked.message.as_deref().unwrap_or("<untouched>")))]);
        out.row([
            "region".to_owned(),
            quote(&region.monitor_name),
            num(region.x),
            num(region.y),
            num(region.width),
            num(region.height),
            num(region.scale),
            region.physical_width.to_string(),
            region.physical_height.to_string(),
            mode_text(region.mode).to_owned(),
        ]);
        world.replay.dump_calls(&h, &mut out);
    }
    let pick = out.into_string();

    assert_no_differences(
        [("regionpicker/capturable-monitor.txt", capturable), ("regionpicker/pick.txt", pick)]
            .iter()
            .filter_map(|(name, text)| difference(name, &read_expected(name), text.as_bytes()))
            .collect(),
    );
}
