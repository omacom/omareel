//! recordingpreferences: `recording_preferences::RecordingPreferences` against
//! `Omareel::RecordingPreferences` (README "recordingpreferences"): `path`, `load` with the legacy
//! copy-over, `save`, and a reload, over every settings.json of the corpus and the special
//! states (missing, unreadable, read-only and symlinked files and directories).
//!
//! The states record permission errors as an ordinary user; run as root the test is skipped
//! (the oracle refuses to run as root for the same reason).

use crate::hermetic::*;
use crate::support::*;
use omareel_core::compat::qjson::{self, Format, Value};
use omareel_core::recording_preferences::RecordingPreferences;
use std::os::unix::fs::{MetadataExt, symlink};

fn octal(mode: u32) -> String {
    format!("{:04o}", mode & 0o7777)
}

/// The file's bytes, when it can be read.
fn append_bytes(cells: &mut Vec<String>, path: &str) {
    if let Ok(bytes) = std::fs::read(path) {
        cells.push(quote_bytes(&bytes));
    }
}

/// README: `missing` | `directory <mode>` | `file <mode> [<bytes>]` | `symlink <target str> <state>`.
fn file_state(h: &Hermetic, path: &str) -> Vec<String> {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return vec!["missing".into()] };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path).map(|t| t.to_string_lossy().into_owned()).unwrap_or_default();
        let mut cells = vec!["symlink".to_owned(), quote(&h.sanitize(&target))];
        let Ok(meta) = std::fs::metadata(path) else {
            cells.push("missing".into());
            return cells;
        };
        if meta.is_dir() {
            cells.extend(["directory".to_owned(), octal(meta.mode())]);
            return cells;
        }
        cells.extend(["file".to_owned(), octal(meta.mode())]);
        append_bytes(&mut cells, path);
        return cells;
    }
    if meta.is_dir() {
        return vec!["directory".into(), octal(meta.mode())];
    }
    let mut cells = vec!["file".to_owned(), octal(meta.mode())];
    append_bytes(&mut cells, path);
    cells
}

fn fields(p: &RecordingPreferences) -> Vec<(&'static str, String)> {
    vec![
        ("systemAudio", boolean(p.system_audio).into()),
        ("microphone", boolean(p.microphone).into()),
        ("microphoneDevice", quote(&p.microphone_device)),
        ("webcamEnabled", boolean(p.webcam_enabled).into()),
        ("webcamDevice", quote(&p.webcam_device)),
        ("webcamChosenDevice", quote(&p.webcam_chosen_device)),
        ("webcamHeight", p.webcam_height.to_string()),
        ("webcamRotation", p.webcam_rotation.to_string()),
        ("webcamFlipHorizontal", boolean(p.webcam_flip_horizontal).into()),
        ("selfViewEnabled", boolean(p.self_view_enabled).into()),
        ("selfViewSize", quote(&p.self_view_size)),
        ("selfViewX", num(p.self_view_x)),
        ("selfViewY", num(p.self_view_y)),
        ("captureBackend", quote(&p.capture_backend)),
        ("countdownBeforeRecording", boolean(p.countdown_before_recording).into()),
        ("webcam", Value::Object(p.webcam.clone()).to_json(Format::Compact)),
    ]
}

fn dump_fields(out: &mut Text, p: &RecordingPreferences) {
    for (name, value) in fields(p) {
        out.row([name.to_owned(), value]);
    }
}

fn current_file(config: &str) -> String {
    format!("{config}/omareel/settings.json")
}

fn legacy_file(config: &str) -> String {
    format!("{config}/omarecord/settings.json")
}

/// Gives every directory and file under `root` its owner bits back (links are not followed),
/// so the scratch can be removed.
fn restore_modes(path: &str) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if meta.file_type().is_symlink() {
        return;
    }
    chmod(path, (meta.mode() & 0o7777) | 0o700);
    if meta.is_dir() {
        for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
            restore_modes(entry.path().to_str().expect("UTF-8 path"));
        }
    }
}

/// Saves, then loads again and lists the fields that did not come back as they were saved.
fn save_and_reload(h: &Hermetic, out: &mut Text, preferences: &RecordingPreferences, config: &str) {
    // The oracle set the error to "<untouched>" first; save sets or clears it.
    let (ok, error) = match preferences.save() {
        Ok(()) => (true, String::new()),
        Err(error) => (false, error),
    };
    out.row(["save".to_owned(), boolean(ok).into(), quote(&h.sanitize(&error))]);
    let mut after = vec!["after-save".to_owned(), "current".into()];
    after.extend(file_state(h, &current_file(config)));
    out.row(after);
    let before = fields(preferences);
    let reloaded = fields(&RecordingPreferences::load());
    let differing: Vec<String> =
        before.iter().zip(&reloaded).filter(|(a, b)| a.1 != b.1).map(|(a, _)| a.0.to_owned()).collect();
    let mut row = vec!["reload".to_owned()];
    if differing.is_empty() {
        row.push("same".into());
    } else {
        row.push("differs".into());
        row.extend(differing);
    }
    out.row(row);
}

#[test]
fn cases_txt() {
    if running_as_root() {
        eprintln!("skipped: the recordingpreferences states check permission errors as an ordinary user");
        return;
    }
    let h = Hermetic::new("recordingpreferences");
    let original_home = h.root("HOME");
    let original_config = h.root("XDG_CONFIG_HOME");
    let corpus = corpus_path("recordingpreferences");
    let corpus_file = |name: &str| read_corpus(&format!("recordingpreferences/{name}"));
    let mut out = Text::new();

    // A state: a fresh directory with config/ as XDG_CONFIG_HOME (setup may change that) and
    // home/ as HOME, prepared by setup; then load, dump, save, reload.
    let mut run = |label: &str, setup: &dyn Fn(&str, &mut String)| {
        let root = format!("{}/{label}", h.scratch());
        let mut config = format!("{root}/config");
        std::fs::create_dir_all(&config).expect("mkpath config");
        std::fs::create_dir_all(format!("{root}/home")).expect("mkpath home");
        h.set_env("HOME", format!("{root}/home"));
        h.set_env("XDG_CONFIG_HOME", &config);
        setup(&root, &mut config);
        out.section(label);
        out.row(["path".to_owned(), quote(&h.sanitize(&RecordingPreferences::path()))]);
        let preferences = RecordingPreferences::load();
        dump_fields(&mut out, &preferences);
        let mut current = vec!["after-load".to_owned(), "current".into()];
        current.extend(file_state(&h, &current_file(&config)));
        out.row(current);
        let mut legacy = vec!["after-load".to_owned(), "legacy".into()];
        legacy.extend(file_state(&h, &legacy_file(&config)));
        out.row(legacy);
        save_and_reload(&h, &mut out, &preferences, &config);
        restore_modes(&root);
        h.set_env("HOME", &original_home);
        h.set_env("XDG_CONFIG_HOME", &original_config);
    };

    for file in sorted_entries(&corpus, false) {
        let bytes = std::fs::read(corpus.join(&file)).expect("corpus file");
        let base = file.rsplit_once('.').map_or(file.as_str(), |(base, _)| base);
        run(&format!("current-{base}"), &|_, config| write_file(&current_file(config), &bytes));
    }
    for file in ["valid-full.json", "invalid-json.json", "webcam-legacy-inset.json"] {
        let bytes = corpus_file(file);
        let base = file.trim_end_matches(".json");
        run(&format!("legacy-{base}"), &|_, config| write_file(&legacy_file(config), &bytes));
    }
    let full = corpus_file("valid-full.json");
    let small = corpus_file("self-view-small-lowercase.json");
    run("special-no-file", &|_, _| {});
    run("special-config-home-missing", &|root, config| {
        *config = format!("{root}/missing-config");
        h.set_env("XDG_CONFIG_HOME", &*config);
    });
    run("special-legacy-and-current", &|_, config| {
        write_file(&legacy_file(config), &full);
        write_file(&current_file(config), &small);
    });
    run("special-legacy-omareel-is-file", &|_, config| {
        write_file(&legacy_file(config), &full);
        write_file(&format!("{config}/omareel"), "not a directory\n");
    });
    run("special-legacy-omareel-read-only", &|_, config| {
        write_file(&legacy_file(config), &full);
        std::fs::create_dir_all(format!("{config}/omareel")).expect("mkpath");
        chmod(&format!("{config}/omareel"), 0o500);
    });
    run("special-legacy-is-directory", &|_, config| {
        std::fs::create_dir_all(legacy_file(config)).expect("mkpath");
    });
    run("special-legacy-unreadable", &|_, config| {
        write_file(&legacy_file(config), &full);
        chmod(&legacy_file(config), 0o000);
    });
    run("special-current-is-directory", &|_, config| {
        std::fs::create_dir_all(current_file(config)).expect("mkpath");
    });
    run("special-current-unreadable", &|_, config| {
        write_file(&current_file(config), &full);
        chmod(&current_file(config), 0o000);
    });
    run("special-current-read-only", &|_, config| {
        write_file(&current_file(config), &full);
        chmod(&current_file(config), 0o444);
    });
    run("special-current-dangling-symlink", &|_, config| {
        std::fs::create_dir_all(format!("{config}/omareel")).expect("mkpath");
        write_file(&legacy_file(config), &full);
        symlink("missing-target.json", current_file(config)).expect("symlink");
    });
    run("special-current-symlink", &|_, config| {
        write_file(&format!("{config}/omareel/real.json"), &full);
        symlink("real.json", current_file(config)).expect("symlink");
    });
    run("special-config-home-read-only", &|_, config| chmod(config, 0o500));
    run("special-xdg-config-home-unset", &|root, config| {
        h.unset_env("XDG_CONFIG_HOME");
        *config = format!("{root}/home/.config");
        write_file(&current_file(config), &small);
    });
    run("special-xdg-config-home-relative", &|root, config| {
        h.set_env("XDG_CONFIG_HOME", "config");
        *config = format!("{root}/home/.config");
        write_file(&current_file(config), &small);
    });

    // Preferences built in code and saved into an empty configuration directory.
    let mut run_constructed = |label: &str, preferences: &RecordingPreferences| {
        let config = format!("{}/{label}/config", h.scratch());
        std::fs::create_dir_all(&config).expect("mkpath config");
        h.set_env("XDG_CONFIG_HOME", &config);
        out.section(label);
        out.row(["path".to_owned(), quote(&h.sanitize(&RecordingPreferences::path()))]);
        dump_fields(&mut out, preferences);
        save_and_reload(&h, &mut out, preferences, &config);
        h.set_env("XDG_CONFIG_HOME", &original_config);
    };
    run_constructed("constructed-defaults", &RecordingPreferences::default());
    let edge = RecordingPreferences {
        system_audio: false,
        microphone: false,
        microphone_device: "Mic \u{201c}\u{fc}\u{201d} \u{1f3a4}".into(),
        webcam_enabled: true,
        webcam_device: String::new(),
        webcam_chosen_device: "/dev/video9".into(),
        webcam_height: 480,
        webcam_rotation: 45,
        webcam_flip_horizontal: true,
        self_view_enabled: false,
        self_view_size: "xl".into(),
        self_view_x: -1.0,
        self_view_y: 2.5,
        capture_backend: String::new(),
        countdown_before_recording: true,
        webcam: qjson::object_from_json(
            br#"{"inset":{"width":3},"mirror":true,"captureHeight":"x","extra":{"list":[1,null,"s"]}}"#,
        )
        .expect("an object"),
    };
    run_constructed("constructed-edge-values", &edge);
    let non_finite = RecordingPreferences {
        self_view_x: f64::NAN,
        self_view_y: f64::NEG_INFINITY,
        webcam_height: 720,
        webcam: Default::default(),
        ..RecordingPreferences::default()
    };
    run_constructed("constructed-non-finite", &non_finite);

    assert_golden("recordingpreferences/cases.txt", &out.into_string());
}
