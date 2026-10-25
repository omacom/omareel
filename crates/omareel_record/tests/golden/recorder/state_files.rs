//! recorder `state-files.txt`: `recorder::state` against Recorder.cpp's state-file layer
//! (README "recorder", the state-files bullet; `recordStateFiles` in
//! `tools/oracle/cases/recorder.cpp`): the paths, `readStateFile`, the running-recording queries
//! over the corpus states, `updateStateFile` step by step on one file (its lock waits included,
//! so this test takes about four seconds), the host command and recording state shortcuts,
//! `last-error.txt`, and what a held `QLockFile` writes, through `compat::qlockfile`.
//!
//! The states record permission errors as an ordinary user; run as root the test is skipped.

use crate::hermetic::*;
use crate::replay::{ScopedEnvironment, directory_listing, octal_mode};
use crate::support::*;
use omareel_core::compat::qdir::FileInfo;
use omareel_core::compat::qjson::{self, Format, Object, Value};
use omareel_core::compat::qlockfile::{self, LockFile};
use omareel_record::recorder::state::*;

fn compact(object: &Object) -> String {
    Value::Object(object.clone()).to_json(Format::Compact)
}

/// The values of an update, written as compact JSON.
fn values(json: &str) -> Object {
    qjson::object_from_json(json.as_bytes()).expect("an object")
}

fn absolute_path(path: &str) -> String {
    FileInfo::new(path).absolute_path()
}

fn show(out: &mut Text, path: &str) {
    let bytes = std::path::Path::new(path).is_file().then(|| std::fs::read(path).ok()).flatten();
    out.row(["  file".to_owned(), bytes.map_or("-".into(), |b| quote_bytes(&b)), octal_mode(path)]);
    out.row(["  directory".to_owned(), directory_listing(&absolute_path(path))]);
}

fn error_cell(h: &Hermetic, result: &Result<(), String>) -> String {
    // The oracle set the error to "<untouched>" first; updateStateFile clears it on success.
    quote(&h.sanitize(result.as_ref().err().map_or("", String::as_str)))
}

#[test]
fn state_files_txt() {
    if running_as_root() {
        eprintln!("skipped: the recorder state files check permission errors as an ordinary user");
        return;
    }
    let h = Hermetic::new("recorder");
    let mut out = Text::new();
    let runtime = h.root("XDG_RUNTIME_DIR");
    {
        let mut environment = ScopedEnvironment::new(&h);
        let mut paths = |label: &str| {
            out.row([
                "paths".to_owned(),
                label.to_owned(),
                quote(&h.sanitize(&state_file_path())),
                quote(&h.sanitize(&self_view_host_path())),
                quote(&h.sanitize(&last_error_file_path())),
            ]);
        };
        paths("XDG_RUNTIME_DIR hermetic");
        environment.set("XDG_RUNTIME_DIR", format!("{runtime}/"));
        paths("XDG_RUNTIME_DIR with a trailing slash");
        environment.set("XDG_RUNTIME_DIR", "relative/run");
        paths("XDG_RUNTIME_DIR relative");
        environment.set("XDG_RUNTIME_DIR", "");
        paths("XDG_RUNTIME_DIR empty");
        environment.unset("XDG_RUNTIME_DIR");
        paths("XDG_RUNTIME_DIR unset");
    }

    let states = corpus_path("recorder/state");
    let state_path = |name: &str| states.join(name).to_str().expect("UTF-8 corpus path").to_owned();
    let names = sorted_entries(&states, false);
    for name in &names {
        out.row(["readStateFile".to_owned(), name.clone(), compact(&read_state_file(&state_path(name)))]);
    }
    out.row(["readStateFile".to_owned(), "<missing>".into(), compact(&read_state_file(&state_path("missing.json")))]);
    out.row([
        "readStateFile".to_owned(),
        "<directory>".into(),
        compact(&read_state_file(states.to_str().expect("UTF-8 corpus path"))),
    ]);

    // The running-recording queries over each state file as recording.json. Only states whose
    // pid is at most 1 are asked: a larger pid would be looked up in the machine's /proc.
    let state_file = state_file_path();
    let state_directory = absolute_path(&state_file);
    let exists = |path: &str| boolean(std::path::Path::new(path).exists()).to_owned();
    for name in &names {
        let state = read_state_file(&state_path(name));
        if state.value("pid").to_variant_long_long() > 1 {
            out.row(["queries", name.as_str(), "skipped (pid > 1)"]);
            continue;
        }
        let install = || {
            remove_path(&state_directory);
            write_file(&state_file, std::fs::read(state_path(name)).expect("corpus state"));
        };
        install();
        let mut cells = vec![
            "queries".to_owned(),
            name.clone(),
            recording_started_us().to_string(),
            quote(&recorded_monitor()),
            boolean(recording_has_webcam()).into(),
            compact(&recording_state()),
        ];
        cells.push(boolean(is_recording()).into());
        cells.push(exists(&state_file));
        install();
        let signalled = signal_existing(false);
        cells.push(boolean(signalled.is_ok()).into());
        cells.push(quote(signalled.as_ref().err().map_or("<untouched>", String::as_str)));
        cells.push(exists(&state_file));
        install();
        let stopped = stop_existing(true);
        cells.push(stopped.code.to_string());
        cells.push(quote(stopped.bundle_path.as_deref().unwrap_or("<untouched>")));
        cells.push(quote(&stopped.error));
        cells.push(exists(&state_file));
        out.row(cells);
    }
    remove_path(&state_directory);
    out.row([
        "queries".to_owned(),
        "<no state file>".into(),
        recording_started_us().to_string(),
        quote(&recorded_monitor()),
        boolean(recording_has_webcam()).into(),
        compact(&recording_state()),
        boolean(is_recording()).into(),
        exists(&state_file),
    ]);

    // updateStateFile, one step after another on the same files.
    let update = |out: &mut Text, label: &str, path: &str, json: &str| {
        let changes = values(json);
        let result = update_state_file(path, &changes);
        out.row([
            "update".to_owned(),
            label.to_owned(),
            quote(&h.sanitize(path)),
            compact(&changes),
            boolean(result.is_ok()).into(),
            error_cell(&h, &result),
        ]);
        show(out, path);
    };
    let scratch = format!("{}/state", h.scratch());
    let file = format!("{scratch}/deep/omareel/recording.json");
    let lock_path = format!("{file}.lock");
    update(&mut out, "creates the directories", &file, r#"{"pid":0,"monitor":"DP-1"}"#);
    update(&mut out, "first command", &file, r#"{"command":"rotate"}"#);
    update(&mut out, "second command", &file, r#"{"command":"rotate"}"#);
    update(&mut out, "explicit sequence", &file, r#"{"command":"flip","command_sequence":7}"#);
    update(&mut out, "after an explicit sequence", &file, r#"{"command":"corner"}"#);
    update(&mut out, "no command", &file, r#"{"camera_status":"ready","selfview_x":16.5}"#);
    update(&mut out, "empty command", &file, r#"{"command":""}"#);
    update(&mut out, "null command", &file, r#"{"command":null}"#);
    update(&mut out, "sequence only", &file, r#"{"command_sequence":-3}"#);
    update(&mut out, "command after a negative sequence", &file, r#"{"command":"show"}"#);
    update(&mut out, "nested values", &file, r#"{"extra":{"list":[1,"two",null]},"monitor":null}"#);
    update(&mut out, "nothing", &file, "{}");
    for name in [
        "strings.json",
        "doubles.json",
        "huge.json",
        "nulls.json",
        "nested.json",
        "array.json",
        "invalid.json",
        "empty.json",
        "bom.json",
    ] {
        write_file(&file, std::fs::read(state_path(name)).expect("corpus state"));
        update(&mut out, &format!("command over {name}"), &file, r#"{"command":"x"}"#);
    }
    write_file(&file, "{\"command_sequence\": 1}");
    chmod(&file, 0o600);
    update(&mut out, "keeps mode 0600", &file, r#"{"command":"y"}"#);
    {
        let mut held = LockFile::new(lock_path.as_str());
        assert!(held.try_lock(0), "could not take the lock");
        update(&mut out, "lock held by this process (waits a second)", &file, r#"{"command":"busy"}"#);
    }
    out.row(["after unlock".to_owned(), directory_listing(&absolute_path(&file))]);
    write_file(&lock_path, "999999999\nomareel\n\n");
    update(&mut out, "stale lock of a dead process", &file, r#"{"command":"stale"}"#);
    write_file(&lock_path, "garbage");
    update(&mut out, "garbage lock file (waits a second)", &file, r#"{"command":"garbage"}"#);
    set_times(&lock_path, 1_000_000_000, 0);
    update(&mut out, "garbage lock file, old", &file, r#"{"command":"old garbage"}"#);
    std::fs::create_dir_all(&lock_path).expect("mkpath lock");
    update(&mut out, "lock path is a directory (waits a second)", &file, r#"{"command":"directory"}"#);
    remove_path(&lock_path);
    remove_path(&file);
    std::fs::create_dir_all(&file).expect("mkpath state");
    update(&mut out, "state path is a directory", &file, r#"{"command":"x"}"#);
    remove_path(&file);
    let read_only = format!("{scratch}/read-only");
    std::fs::create_dir_all(&read_only).expect("mkpath read-only");
    chmod(&read_only, 0o500);
    update(&mut out, "read-only directory", &format!("{read_only}/recording.json"), r#"{"command":"x"}"#);
    chmod(&read_only, 0o700);
    {
        // The C++ call without an error pointer leaves the caller's text alone.
        let ok = update_state_file(&format!("{scratch}/no-error.json"), &values(r#"{"command":"x"}"#)).is_ok();
        out.row(["update".to_owned(), "without an error pointer".into(), boolean(ok).into(), quote("<untouched>")]);
    }

    // The fixed paths: sendHostCommand and updateRecordingState under XDG_RUNTIME_DIR.
    remove_path(&state_directory);
    for command in ["quit", "configure", ""] {
        let ok = send_host_command(command);
        out.row(["sendHostCommand".to_owned(), quote(command), boolean(ok).into()]);
        show(&mut out, &self_view_host_path());
    }
    let result = update_recording_state(&values(r#"{"capture_started":true,"command":"rotate"}"#));
    out.row(["updateRecordingState".to_owned(), boolean(result.is_ok()).into(), error_cell(&h, &result)]);
    show(&mut out, &state_file);

    // last-error.txt.
    out.row(["readLastError".to_owned(), "<missing>".into(), quote(&read_last_error())]);
    for detail in ["Overlays could not leave the recorded output", "two\nlines\n", "  padded  ", ""] {
        write_last_error(detail);
        out.row([
            "writeLastError".to_owned(),
            quote(detail),
            quote_bytes(&read_file(&last_error_file_path())),
            quote(&read_last_error()),
        ]);
    }
    remove_path(&state_directory);

    // What a held lock leaves in its file: Qt's format, with the facts of the run (pid, process
    // name, host name, machine and boot ids) shown as the oracle's placeholders.
    let format_lock = format!("{scratch}/format.lock");
    {
        let mut lock = LockFile::new(format_lock.as_str());
        lock.set_stale_lock_time(10_000);
        assert!(lock.try_lock(1000), "could not lock {format_lock}");
        let bytes = read_file(&format_lock);
        let mut lines: Vec<Vec<u8>> = bytes.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
        let pid = i64::from(std::process::id());
        let mask = |lines: &mut Vec<Vec<u8>>, index: usize, value: &[u8], placeholder: &str| {
            if lines[index] == value {
                lines[index] = placeholder.as_bytes().to_vec();
            }
        };
        if lines.len() >= 3 {
            mask(&mut lines, 0, pid.to_string().as_bytes(), "{pid}");
            mask(&mut lines, 1, qlockfile::process_name_by_pid(pid).as_bytes(), "{applicationName}");
            mask(&mut lines, 2, qlockfile::machine_host_name().as_bytes(), "{hostName}");
        }
        if lines.len() >= 5 {
            mask(&mut lines, 3, &qlockfile::machine_unique_id(), "{machineUniqueId}");
            mask(&mut lines, 4, &qlockfile::boot_unique_id(), "{bootUniqueId}");
        }
        out.row(["QLockFile".to_owned(), quote_bytes(&lines.join(&b'\n')), octal_mode(&format_lock)]);
    }
    out.row(["QLockFile after unlock".to_owned(), exists(&format_lock)]);

    assert_golden("recorder/state-files.txt", &out.into_string());
}
