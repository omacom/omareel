//! project: `project::Project` against `Omareel::Project` (README "project").
//!
//! Colours here only come from parsing and the defaults (no qRound, no float channels), so
//! these files are checked on every architecture.
//!
//! The load and save cases record permission errors as an ordinary user; run as root they are
//! skipped (the oracle refuses to run as root for the same reason).

use crate::hermetic::*;
use crate::support::*;
use omareel_core::compat::qjson::{self, Format, Value};
use omareel_core::project::*;
use std::os::unix::fs::MetadataExt;

fn field(out: &mut Text, path: &str, value: String) {
    out.row([path.to_owned(), value]);
}

fn shadow_fields(out: &mut Text, prefix: &str, s: &Shadow) {
    field(out, &format!("{prefix}.enabled"), boolean(s.enabled).into());
    field(out, &format!("{prefix}.intensity"), num(s.intensity));
    field(out, &format!("{prefix}.blur"), num(s.blur));
    field(out, &format!("{prefix}.distance"), num(s.distance));
    field(out, &format!("{prefix}.angle"), num(s.angle));
}

fn border_fields(out: &mut Text, prefix: &str, b: &Border) {
    field(out, &format!("{prefix}.enabled"), boolean(b.enabled).into());
    field(out, &format!("{prefix}.width"), num(b.width));
    field(out, &format!("{prefix}.color"), color(&b.color));
    field(out, &format!("{prefix}.alpha"), num(b.alpha));
}

fn spring_fields(out: &mut Text, prefix: &str, s: &Spring) {
    field(out, &format!("{prefix}.mass"), num(s.mass));
    field(out, &format!("{prefix}.stiffness"), num(s.stiffness));
    field(out, &format!("{prefix}.damping"), num(s.damping));
}

/// The oracle's `dumpProject`: every member in declaration order, including what `toJson()`
/// drops.
fn dump_project(out: &mut Text, p: &Project) {
    field(out, "version", p.version.to_string());
    field(out, "defaultsVersion", p.defaults_version.to_string());
    field(out, "name", quote(&p.name));
    field(out, "aspect", quote(&p.aspect));
    field(out, "crop", [p.crop.x, p.crop.y, p.crop.width, p.crop.height].map(num).join(" "));
    field(out, "clips.size", p.clips.len().to_string());
    for (i, c) in p.clips.iter().enumerate() {
        let prefix = format!("clips[{i}]");
        field(out, &format!("{prefix}.id"), quote(&c.id));
        field(out, &format!("{prefix}.in"), num(c.in_));
        field(out, &format!("{prefix}.out"), num(c.out));
        field(out, &format!("{prefix}.speed"), num(c.speed));
        field(out, &format!("{prefix}.layout"), quote(&c.layout));
        field(out, &format!("{prefix}.media"), quote(&c.media));
        field(out, &format!("{prefix}.fit"), quote(&c.fit));
        field(out, &format!("{prefix}.isMedia"), boolean(c.is_media()).into());
    }
    field(out, "zooms.size", p.zooms.len().to_string());
    for (i, z) in p.zooms.iter().enumerate() {
        let prefix = format!("zooms[{i}]");
        field(out, &format!("{prefix}.id"), quote(&z.id));
        field(out, &format!("{prefix}.start"), num(z.start));
        field(out, &format!("{prefix}.end"), num(z.end));
        field(out, &format!("{prefix}.level"), num(z.level));
        field(out, &format!("{prefix}.automaticTarget"), boolean(z.automatic_target).into());
        field(out, &format!("{prefix}.target"), format!("{} {}", num(z.target.x), num(z.target.y)));
    }
    spring_fields(out, "zoomStyle.spring", &p.zoom_style.spring);
    field(out, "zoomStyle.snapToEdgesRatio", num(p.zoom_style.snap_to_edges_ratio));
    field(out, "zoomStyle.instantAnimation", boolean(p.zoom_style.instant_animation).into());
    field(out, "zoomStyle.motionBlur", num(p.zoom_style.motion_blur));
    let bg = &p.background;
    field(out, "background.type", quote(&bg.type_));
    field(out, "background.wallpaper", quote(&bg.wallpaper));
    field(out, "background.gradient.angle", num(bg.gradient.angle));
    field(out, "background.gradient.stops.size", bg.gradient.stops.len().to_string());
    for (i, stop) in bg.gradient.stops.iter().enumerate() {
        let prefix = format!("background.gradient.stops[{i}]");
        field(out, &format!("{prefix}.color"), color(&stop.color));
        field(out, &format!("{prefix}.position"), num(stop.position));
    }
    field(out, "background.color", color(&bg.color));
    field(out, "background.image", quote(&bg.image));
    field(out, "background.blur", num(bg.blur));
    field(out, "frame.padding", num(p.frame.padding));
    field(out, "frame.radius", num(p.frame.radius));
    shadow_fields(out, "frame.shadow", &p.frame.shadow);
    border_fields(out, "frame.border", &p.frame.border);
    let c = &p.cursor;
    field(out, "cursor.visible", boolean(c.visible).into());
    field(out, "cursor.size", num(c.size));
    field(out, "cursor.smoothing", boolean(c.smoothing).into());
    spring_fields(out, "cursor.spring", &c.spring);
    field(out, "cursor.clickEffect", quote(&c.click_effect));
    field(out, "cursor.clickShrink", num(c.click_shrink));
    field(out, "cursor.rotateOnXMovementRatio", num(c.rotate_on_x_movement_ratio));
    field(out, "cursor.hideWhenIdleMs", c.hide_when_idle_ms.to_string());
    field(out, "cursor.style", quote(&c.style));
    field(out, "cursor.ringColor", color(&c.ring_color));
    field(out, "cursor.clickSound", quote(&c.click_sound));
    let k = &p.keystrokes;
    field(out, "keystrokes.enabled", boolean(k.enabled).into());
    field(out, "keystrokes.position", quote(&k.position));
    field(out, "keystrokes.size", num(k.size));
    field(out, "keystrokes.showOnlyShortcuts", boolean(k.show_only_shortcuts).into());
    field(out, "keystrokes.holdMs", k.hold_ms.to_string());
    field(out, "audio.desktop", boolean(p.audio.desktop).into());
    field(out, "audio.mic", boolean(p.audio.mic).into());
    field(out, "audio.volume", num(p.audio.volume));
    let cam = &p.camera;
    field(out, "camera.enabled", boolean(cam.enabled).into());
    field(out, "camera.position", quote(&cam.position));
    field(out, "camera.size", num(cam.size));
    field(out, "camera.shape", quote(&cam.shape));
    field(out, "camera.radius", num(cam.radius));
    field(out, "camera.flipHorizontal", boolean(cam.flip_horizontal).into());
    field(out, "camera.rotation", cam.rotation.to_string());
    shadow_fields(out, "camera.shadow", &cam.shadow);
    border_fields(out, "camera.border", &cam.border);
    field(out, "camera.scaleDuringZoom", num(cam.scale_during_zoom));
    field(out, "camera.offset", format!("{} {}", num(cam.offset.x), num(cam.offset.y)));
    field(out, "camera.fullFrame", quote(&cam.full_frame));
    let e = &p.export_settings;
    field(out, "exportSettings.format", quote(&e.format));
    field(out, "exportSettings.fps", e.fps.to_string());
    field(out, "exportSettings.height", e.height.to_string());
    field(out, "exportSettings.quality", quote(&e.quality));
    field(out, "exportSettings.gif.fps", e.gif.fps.to_string());
    field(out, "exportSettings.gif.height", e.gif.height.to_string());
    field(out, "exportSettings.gif.quality", quote(&e.gif.quality));
    field(out, "exportSettings.gif.loop", boolean(e.gif.loop_).into());
}

/// `QJsonDocument(project.toJson()).toJson(Indented)`.
fn indented(project: &Project) -> String {
    project.to_json().to_json(Format::Indented)
}

/// What `stat` says: whether the file exists, its inode and its permission bits.
#[derive(Default)]
struct FileState {
    exists: bool,
    inode: u64,
    mode: u32,
}

fn file_state(path: &str) -> FileState {
    match std::fs::metadata(path) {
        Ok(meta) => FileState { exists: true, inode: meta.ino(), mode: meta.mode() & 0o7777 },
        Err(_) => FileState::default(),
    }
}

fn octal(mode: u32) -> String {
    format!("{mode:04o}")
}

struct LoadResult {
    project: Project,
    error: String,
    rewritten: bool,
    after: Vec<u8>,
    after_state: FileState,
}

/// The oracle's `loadAt`.
fn load_at(path: &str) -> LoadResult {
    let before = file_state(path);
    let original = read_file(path);
    let (project, result) = Project::load(path);
    // The C++ clears the error on success and sets it on failure, so `<untouched>` never stays.
    let error = result.err().unwrap_or_default();
    let after_state = file_state(path);
    let after = read_file(path);
    let rewritten = before.inode != after_state.inode || original != after;
    LoadResult { project, error, rewritten, after, after_state }
}

/// What the oracle's `runProject` writes for the loads: `load.txt`, `fields.txt`, `migrate.txt`,
/// and `load/<label>`, `tojson/<label>` by label.
#[derive(Default)]
struct LoadOutputs {
    summary: Text,
    fields: Text,
    migrate: Text,
    load: Vec<(String, Vec<u8>)>,
    tojson: Vec<(String, String)>,
}

impl LoadOutputs {
    /// The oracle's `recordLoad`.
    fn record(&mut self, hermetic: &Hermetic, name: &str, result: &LoadResult) {
        let mut cells = vec![
            name.to_owned(),
            if result.rewritten { "rewritten" } else { "unchanged" }.to_owned(),
            quote(&hermetic.sanitize(&result.error)),
        ];
        if result.after_state.exists {
            cells.push(octal(result.after_state.mode));
        }
        self.summary.row(cells);
        if result.rewritten {
            self.load.push((name.to_owned(), result.after.clone()));
        }
        self.tojson.push((name.to_owned(), indented(&result.project)));
        self.fields.section(name);
        dump_project(&mut self.fields, &result.project);
    }
}

fn run_loads(hermetic: &Hermetic) -> LoadOutputs {
    let corpus = corpus_path("project");
    let mut outputs = LoadOutputs::default();
    for file in sorted_entries(&corpus, false) {
        let input = read_corpus(&format!("project/{file}"));
        let path = format!("{}/{file}/project.json", hermetic.scratch());
        write_file(&path, &input);
        chmod(&path, 0o644);
        let loaded = load_at(&path);
        outputs.record(hermetic, &file, &loaded);

        let object = match qjson::from_json(&input) {
            Ok(Value::Object(object)) => object,
            // QJsonParseError::NoError for a document that is not an object.
            Ok(_) => {
                outputs.migrate.row([file.clone(), "-".into(), quote("no error occurred")]);
                continue;
            }
            Err(error) => {
                outputs.migrate.row([file.clone(), "-".into(), quote(error.error_string())]);
                continue;
            }
        };
        let (migrated, changed) = Project::migrate_defaults(&object);
        outputs.migrate.row([file.clone(), boolean(changed).into(), json(&Value::Object(migrated.clone()))]);
        let (twice, changed_again) = Project::migrate_defaults(&migrated);
        let again = if twice == migrated { "same".to_owned() } else { json(&Value::Object(twice)) };
        outputs.migrate.row([format!("{file} (again)"), boolean(changed_again).into(), again]);
        // fromJson() migrates by itself, so it must agree with load() on the unmigrated input.
        let from_json = Project::from_json(&object).to_json();
        if from_json != loaded.project.to_json() {
            outputs.migrate.row([format!("{file} (fromJson differs from load)"), json(&Value::Object(from_json))]);
        }
    }

    // Loads that fail or cannot write the migrated file back.
    let special = format!("{}/special", hermetic.scratch());
    std::fs::create_dir_all(&special).expect("mkpath special");
    let at = |name: &str| format!("{special}/{name}");
    outputs.record(hermetic, "special-missing-file.json", &load_at(&at("missing/project.json")));
    std::fs::create_dir_all(at("directory.json")).expect("mkpath directory.json");
    outputs.record(hermetic, "special-directory.json", &load_at(&at("directory.json")));
    let legacy = read_corpus("project/history-02-2026-09-02-96d74f9.json");
    let unreadable = at("unreadable.json");
    write_file(&unreadable, &legacy);
    chmod(&unreadable, 0o000);
    outputs.record(hermetic, "special-unreadable.json", &load_at(&unreadable));
    chmod(&unreadable, 0o600);
    let read_only_directory = at("read-only");
    std::fs::create_dir_all(&read_only_directory).expect("mkpath read-only");
    let read_only = format!("{read_only_directory}/project.json");
    write_file(&read_only, &legacy);
    chmod(&read_only_directory, 0o500);
    outputs.record(hermetic, "special-legacy-in-read-only-directory.json", &load_at(&read_only));
    chmod(&read_only_directory, 0o700);
    let read_only_file = at("read-only-file.json");
    write_file(&read_only_file, &legacy);
    chmod(&read_only_file, 0o440);
    outputs.record(hermetic, "special-legacy-read-only-file.json", &load_at(&read_only_file));
    chmod(&read_only_file, 0o600);
    let executable = at("mode-0750.json");
    write_file(&executable, &legacy);
    chmod(&executable, 0o750);
    outputs.record(hermetic, "special-legacy-mode-0750.json", &load_at(&executable));
    outputs
}

/// The names of the files in an expected directory, sorted.
fn expected_names(relative: &str) -> Vec<String> {
    sorted_entries(&expected_path(relative), false)
}

#[test]
fn load_fields_migrate_and_rewritten_files() {
    if running_as_root() {
        eprintln!("skipped: root ignores the file modes the project load cases depend on");
        return;
    }
    let hermetic = Hermetic::new("project");
    let outputs = run_loads(&hermetic);
    let mut reports = Vec::new();
    for (name, text) in
        [("load.txt", outputs.summary), ("fields.txt", outputs.fields), ("migrate.txt", outputs.migrate)]
    {
        let relative = format!("project/{name}");
        reports.extend(difference(&relative, &read_expected(&relative), text.into_string().as_bytes()));
    }
    for (directory, files) in [
        ("load", outputs.load),
        ("tojson", outputs.tojson.into_iter().map(|(name, text)| (name, text.into_bytes())).collect()),
    ] {
        let mut written: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
        written.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        let expected = expected_names(&format!("project/{directory}"));
        if written != expected {
            reports.push(format!("project/{directory}: Rust wrote {written:?}, the golden has {expected:?}"));
        }
        for (name, bytes) in files {
            let relative = format!("project/{directory}/{name}");
            if expected_path(&relative).exists() {
                reports.extend(difference(&relative, &read_expected(&relative), &bytes));
            }
        }
    }
    assert_no_differences(reports);
}

#[test]
fn save_txt() {
    if running_as_root() {
        eprintln!("skipped: root ignores the file modes the project save cases depend on");
        return;
    }
    let hermetic = Hermetic::new("project");
    // What the loads leave behind and the saves use: a directory named like a file, and a
    // directory that is made read-only for one save.
    let special = format!("{}/special", hermetic.scratch());
    std::fs::create_dir_all(format!("{special}/directory.json")).expect("mkpath directory.json");
    let read_only_directory = format!("{special}/read-only");
    std::fs::create_dir_all(&read_only_directory).expect("mkpath read-only");

    let defaults = Project::defaults("Saved", 3.5);
    let mut out = Text::new();
    let mut try_save = |label: &str, path: &str| {
        let result = defaults.save(path);
        let written = read_file(path);
        let state = file_state(path);
        let content = if written == indented(&defaults).as_bytes() {
            "matches-toJson"
        } else if written.is_empty() {
            "-"
        } else {
            "differs"
        };
        out.row([
            label.to_owned(),
            boolean(result.is_ok()).to_owned(),
            quote(&hermetic.sanitize(&result.err().unwrap_or_default())),
            content.to_owned(),
            if state.exists { octal(state.mode) } else { "-".to_owned() },
        ]);
    };
    try_save("new file", &format!("{special}/saved.json"));
    try_save("missing directory", &format!("{special}/no/such/dir/saved.json"));
    try_save("path is a directory", &format!("{special}/directory.json"));
    chmod(&read_only_directory, 0o500);
    try_save("read-only directory", &format!("{read_only_directory}/saved.json"));
    chmod(&read_only_directory, 0o700);
    try_save("empty path", "");
    assert_golden("project/save.txt", &out.into_string());
}

/// The oracle's `defaults()` cases.
fn defaults_cases() -> Vec<(&'static str, Project)> {
    vec![
        // `Project::defaults()` with its default arguments.
        ("no-arguments", Project::defaults("Untitled Recording", 0.0)),
        ("demo-12.5", Project::defaults("Demo", 12.5)),
        ("empty-name-0", Project::defaults("", 0.0)),
        ("unicode-tiny", Project::defaults("R\u{e9}sum\u{e9} \u{1F3AC} \"q\"", 1e-9)),
        ("negative", Project::defaults("x", -5.0)),
        ("long", Project::defaults("Long", 86400.123456789)),
        ("nan", Project::defaults("NaN", f64::NAN)),
        ("default-constructed", Project::default()),
    ]
}

#[test]
fn defaults_and_defaults_fields() {
    let mut reports = Vec::new();
    let mut fields = Text::new();
    for (label, project) in defaults_cases() {
        let relative = format!("project/defaults/{label}.json");
        reports.extend(difference(&relative, &read_expected(&relative), indented(&project).as_bytes()));
        fields.section(label);
        dump_project(&mut fields, &project);
    }
    let names: Vec<String> = defaults_cases().iter().map(|(label, _)| format!("{label}.json")).collect();
    let mut sorted = names.clone();
    sorted.sort();
    let expected = expected_names("project/defaults");
    if sorted != expected {
        reports.push(format!("project/defaults: Rust wrote {sorted:?}, the golden has {expected:?}"));
    }
    reports.extend(difference(
        "project/defaults-fields.txt",
        &read_expected("project/defaults-fields.txt"),
        fields.into_string().as_bytes(),
    ));
    assert_no_differences(reports);
}

#[test]
fn helpers_txt() {
    let mut out = Text::new();
    let mut speeds = vec!["allowedClipSpeeds".to_owned()];
    speeds.extend(allowed_clip_speeds().into_iter().map(num));
    out.row(speeds);
    out.row(["allowedClipLayouts".to_owned(), quote_list(&allowed_clip_layouts())]);
    out.row(["allowedClipFits".to_owned(), quote_list(&allowed_clip_fits())]);
    out.row(["allowedAspects".to_owned(), quote_list(&allowed_aspects())]);
    out.row(["CurrentDefaultsVersion".to_owned(), Project::CURRENT_DEFAULTS_VERSION.to_string()]);
    for path in [
        "media/demo.mp4",
        "media/./demo.mp4",
        "media//demo.mp4",
        "media/a/../demo.mp4",
        "media/talk..final.mp4",
        "media/..hidden.mp4",
        "media/../screen.mp4",
        "media/../../etc/passwd",
        "media/a/b/../../../x.mp4",
        "./media/demo.mp4",
        "media",
        "media/",
        "media/.",
        "media/sub/",
        "media/sub/clip.mov",
        "Media/demo.mp4",
        "medias/demo.mp4",
        "mediademo.mp4",
        "demo.mp4",
        "/home/demo.mp4",
        "/media/demo.mp4",
        "media\\demo.mp4",
        "media/demo\\x.mp4",
        "~/media/x.mp4",
        "media/ spaced name.mp4",
        "media/\u{e9}\u{1F600}.mp4",
        "media/a:b.mp4",
        "C:/media/x.mp4",
        "media/...",
        "media/x/..",
        "",
        // QString(): a null string, which Rust has no separate value for.
        "",
        " media/x.mp4",
        "media/x.mp4 ",
    ] {
        out.row(["sanitizedMediaPath".to_owned(), quote(path), quote(&sanitized_media_path(path))]);
    }

    let fallback = Spring { mass: 1.5, stiffness: 250.0, damping: 25.0 };
    for text in [
        "{}",
        r#"{"mass":2}"#,
        r#"{"mass":2,"stiffness":3,"damping":4}"#,
        r#"{"mass":"2","stiffness":null,"damping":true}"#,
        r#"{"mass":0,"stiffness":-1,"damping":1e300}"#,
        r#"{"MASS":2,"extra":1}"#,
        r#"{"mass":1.0000000000000002,"stiffness":470.5,"damping":0.1}"#,
    ] {
        let object = qjson::object_from_json(text.as_bytes()).unwrap_or_default();
        let spring = spring_from_json(&object, &fallback);
        out.row([
            "springFromJson".to_owned(),
            text.to_owned(),
            num(spring.mass),
            num(spring.stiffness),
            num(spring.damping),
            json(&spring_to_json(&spring)),
        ]);
    }
    for spring in [
        Spring::default(),
        Spring { mass: 3.0, stiffness: 470.0, damping: 70.0 },
        Spring { mass: 2.25, stiffness: 200.0, damping: 40.0 },
        Spring { mass: -0.0, stiffness: 1e21, damping: 0.1 },
        Spring { mass: f64::NAN, stiffness: 1.0, damping: 2.0 },
    ] {
        out.row([
            "springToJson".to_owned(),
            num(spring.mass),
            num(spring.stiffness),
            num(spring.damping),
            json(&spring_to_json(&spring)),
        ]);
    }

    // json() and setJson() are toJson() and fromJson() under other names.
    let via_set_json = Project::from_json(&Project::defaults("Via setJson", 2.0).to_json());
    out.row([
        "setJson(json()) of defaults(\"Via setJson\", 2)".to_owned(),
        json(&Value::Object(via_set_json.to_json())),
    ]);
    assert_golden("project/helpers.txt", &out.into_string());
}
