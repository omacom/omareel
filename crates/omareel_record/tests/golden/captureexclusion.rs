//! captureexclusion: `capture_exclusion` against `src/record/CaptureExclusion.cpp` (README
//! "captureexclusion"), with hyprctl answered by the replay shim. Some scenarios let hyprctl
//! hang past its 2 s timeout.

use crate::hermetic::{Hermetic, write_file};
use crate::replay::{Answer, Replay, ScopedEnvironment, shim_directory};
use crate::support::*;
use omareel_core::compat::qjson::{Format, Value};
use omareel_record::capture_exclusion::{self, HardwareCursorHold, Status};

fn strings(value: &Value) -> Vec<String> {
    value.to_array().iter().map(|item| item.to_string_or("")).collect()
}

fn cases() -> Value {
    read_json("captureexclusion/cases.json")
}

fn dump_status(h: &Hermetic, out: &mut Text, status: &Status) {
    out.row(["loaded", boolean(status.loaded)]);
    out.row(["builtHash".to_owned(), quote(&status.built_hash)]);
    out.row(["reason".to_owned(), quote(&h.sanitize(&status.reason))]);
    out.row(["details".to_owned(), Value::Object(status.details.clone()).to_json(Format::Compact)]);
}

/// Replaces `@PLUGIN@` in the answers' argv by the plugin's absolute path.
fn with_plugin(answers: &Value, plugin: &str) -> Value {
    let replaced: Vec<Value> = answers
        .to_array()
        .into_iter()
        .map(|answer| {
            let mut object = answer.to_object();
            let argv: Vec<Value> = object
                .value("argv")
                .to_array()
                .into_iter()
                .map(|argument| if argument.as_str() == Some("@PLUGIN@") { Value::from(plugin) } else { argument })
                .collect();
            object.insert("argv", Value::array(argv));
            Value::Object(object)
        })
        .collect();
    Value::array(replaced)
}

#[test]
fn compatible_hash_and_overlay_monitor() {
    let cases = cases();
    let mut out = Text::new();
    let hashes = strings(cases.get("hashes"));
    for built in &hashes {
        for running in &hashes {
            out.row([quote(built), quote(running), boolean(capture_exclusion::compatible_hash(built, running)).into()]);
        }
    }
    let hash = out.into_string();

    let mut out = Text::new();
    let input = cases.get("overlayMonitor");
    for plugin in [false, true] {
        for recorded in strings(input.get("recorded")) {
            for monitors in input.get("monitors").to_array() {
                let monitors = strings(&monitors);
                out.row([
                    boolean(plugin).to_owned(),
                    quote(&recorded),
                    quote_list(&monitors),
                    quote(&capture_exclusion::overlay_monitor(plugin, &recorded, &monitors)),
                ]);
            }
        }
    }
    let overlay = out.into_string();

    assert_no_differences(
        [("captureexclusion/compatible-hash.txt", hash), ("captureexclusion/overlay-monitor.txt", overlay)]
            .iter()
            .filter_map(|(name, text)| difference(name, &read_expected(name), text.as_bytes()))
            .collect(),
    );
}

#[test]
fn status_ensure_loaded_overlays_and_hardware_cursor() {
    let h = Hermetic::new("captureexclusion");
    let cases = cases();
    let replay_directory = format!("{}/replay", h.scratch());

    let mut out = Text::new();
    for (label, answers) in cases.get("status").to_object().iter() {
        let mut replay = Replay::new(&replay_directory);
        replay.answer_json(answers);
        replay.activate(&h);
        let mut environment = ScopedEnvironment::new(&h);
        if answers.is_null() {
            environment.set("PATH", shim_directory(&h.scratch(), "empty", &[]));
        }
        out.section(label);
        dump_status(&h, &mut out, &capture_exclusion::status());
        replay.dump_calls(&h, &mut out);
    }
    let status = out.into_string();

    let mut out = Text::new();
    let plugins = format!("{}/plugins", h.scratch());
    // Without OMAREEL_PLUGIN_PATH ensure_loaded also looks beside the program; the one such
    // scenario has the plugin loaded already, so this directory is never reached.
    let application_dir = format!("{}/application", h.scratch());
    for (label, scenario) in cases.get("ensureLoaded").to_object().iter() {
        let _ = std::fs::remove_dir_all(&plugins);
        std::fs::create_dir_all(&plugins).expect("mkpath plugins");
        let file = format!("{plugins}/omareel-capture-exclude.so");
        let mut environment = ScopedEnvironment::new(&h);
        let kind = scenario.get("plugin");
        let mut variable = String::new();
        match kind.as_str() {
            None => environment.unset("OMAREEL_PLUGIN_PATH"),
            Some("missing") => variable = format!("{plugins}/missing.so"),
            Some("directory") => {
                variable = format!("{plugins}/directory.so");
                std::fs::create_dir_all(&variable).expect("mkpath directory.so");
            }
            Some("empty-variable") => environment.set("OMAREEL_PLUGIN_PATH", ""),
            Some(kind) => {
                write_file(&file, "fixture");
                if let Some(hash) = scenario.get("hash").as_str() {
                    write_file(&format!("{file}.hash"), hash);
                }
                // Relative to the working directory, $HOME.
                variable = if kind == "relative" {
                    file.strip_prefix(&format!("{}/", h.root("HOME"))).expect("under $HOME").to_owned()
                } else {
                    file.clone()
                };
            }
        }
        if !variable.is_empty() {
            environment.set("OMAREEL_PLUGIN_PATH", &variable);
        }
        let mut replay = Replay::new(&replay_directory);
        replay.answer_json(&with_plugin(scenario.get("answers"), &file));
        replay.activate(&h);
        out.section(label);
        out.row([
            "OMAREEL_PLUGIN_PATH".to_owned(),
            if kind.is_null() { "<unset>".into() } else { quote(&h.sanitize(&variable)) },
        ]);
        dump_status(&h, &mut out, &capture_exclusion::ensure_loaded(&application_dir));
        replay.dump_calls(&h, &mut out);
    }
    let _ = std::fs::remove_dir_all(&plugins);
    let ensure = out.into_string();

    let mut out = Text::new();
    let input = cases.get("overlaysOffMonitor");
    let directory = corpus_path("hyprctl/layers");
    let monitors = strings(input.get("monitors"));
    let row = |out: &mut Text, label: &str, answer: &Answer| {
        let mut replay = Replay::new(&replay_directory);
        replay.answer("hyprctl", &["-j", "layers"], answer, 0);
        replay.activate(&h);
        let mut cells = vec![label.to_owned()];
        for monitor in &monitors {
            cells.push(format!("{}={}", quote(monitor), boolean(capture_exclusion::overlays_off_monitor(monitor))));
        }
        out.row(cells);
    };
    for name in strings(input.get("files")) {
        row(&mut out, &name, &Answer::stdout(std::fs::read(directory.join(&name)).expect("corpus")));
    }
    let failing = Answer { status: 1, ..Answer::stdout(std::fs::read(directory.join("crafted-clean-two.json")).expect("corpus")) };
    row(&mut out, "<crafted-clean-two.json, exit status 1>", &failing);
    let overlays = out.into_string();

    let mut out = Text::new();
    for (label, answers) in cases.get("hardwareCursor").to_object().iter() {
        let mut replay = Replay::new(&replay_directory);
        replay.answer_json(answers);
        replay.activate(&h);
        out.section(label);
        let held;
        {
            let _hold = HardwareCursorHold::new();
            held = replay.calls().len();
        }
        for (index, call) in replay.calls().iter().enumerate() {
            out.row([if index < held { "hold" } else { "release" }.to_owned(), quote_list(call)]);
        }
    }
    let cursor = out.into_string();

    assert_no_differences(
        [
            ("captureexclusion/status.txt", status),
            ("captureexclusion/ensure-loaded.txt", ensure),
            ("captureexclusion/overlays-off-monitor.txt", overlays),
            ("captureexclusion/hardware-cursor.txt", cursor),
        ]
        .iter()
        .filter_map(|(name, text)| difference(name, &read_expected(name), text.as_bytes()))
        .collect(),
    );
}
