//! inputlog: `input_log::InputLog` against `Omareel::InputLog` (README "inputlog").

use crate::support::*;
use omareel_core::input_log::{InputEvent, InputKind, InputLog};

pub fn kind_name(kind: InputKind) -> &'static str {
    match kind {
        InputKind::Move => "Move",
        InputKind::ButtonDown => "ButtonDown",
        InputKind::ButtonUp => "ButtonUp",
        InputKind::Scroll => "Scroll",
        InputKind::KeyDown => "KeyDown",
        InputKind::KeyUp => "KeyUp",
    }
}

struct Case {
    label: String,
    bundle: bool,
    path: String,
    first: i64,
    duration: f64,
}

/// The oracle's `inputLogCases`.
fn cases() -> Vec<Case> {
    let root = corpus_path("inputlog");
    let in_root = |name: &str| root.join(name).to_str().expect("UTF-8 path").to_owned();
    let mut cases = Vec::new();
    for file in sorted_entries(&root, false) {
        for first in [1_000_000i64, 0, 2_000_000] {
            for duration in [-1.0, 0.0, 0.5, 2.0] {
                if first != 1_000_000 && duration != -1.0 {
                    continue;
                }
                cases.push(Case {
                    label: format!("load {file} first={first} duration={}", num(duration)),
                    bundle: false,
                    path: in_root(&file),
                    first,
                    duration,
                });
            }
        }
    }
    let load = |label: &str, path: String| Case {
        label: format!("load {label} first=1000000 duration=-1"),
        bundle: false,
        path,
        first: 1_000_000,
        duration: -1.0,
    };
    cases.push(load("missing.jsonl", in_root("missing.jsonl")));
    cases.push(load("<directory>", in_root("bundles")));
    cases.push(load("<empty path>", String::new()));
    let bundles = root.join("bundles");
    let in_bundles = |name: &str| bundles.join(name).to_str().expect("UTF-8 path").to_owned();
    for bundle in sorted_entries(&bundles, true) {
        for duration in [-1.0, 3.0, 0.0] {
            cases.push(Case {
                label: format!("loadBundle {bundle} duration={}", num(duration)),
                bundle: true,
                path: in_bundles(&bundle),
                first: 0,
                duration,
            });
        }
    }
    let load_bundle = |label: &str, path: String| Case {
        label: format!("loadBundle {label} duration=-1"),
        bundle: true,
        path,
        first: 0,
        duration: -1.0,
    };
    cases.push(load_bundle("missing-bundle", in_bundles("missing-bundle")));
    cases.push(load_bundle("synthetic/", in_bundles("synthetic") + "/"));
    cases
}

fn load_case(case: &Case) -> Result<InputLog, String> {
    if case.bundle {
        InputLog::load_bundle(&case.path, case.duration)
    } else {
        InputLog::load(&case.path, case.first, case.duration)
    }
}

/// The oracle's `indices`: where each returned event sits in the full list.
fn indices(subset: &[InputEvent], events: &[InputEvent]) -> String {
    let mut result = Vec::new();
    let mut from = 0;
    for event in subset {
        for (i, candidate) in events.iter().enumerate().skip(from) {
            if candidate.monotonic_us == event.monotonic_us
                && candidate.kind == event.kind
                && candidate.position.x == event.position.x
                && candidate.position.y == event.position.y
                && candidate.button == event.button
                && candidate.key_name == event.key_name
                && candidate.key_code == event.key_code
            {
                result.push(i.to_string());
                from = i + 1;
                break;
            }
        }
    }
    if result.is_empty() { "-".into() } else { result.join(",") }
}

pub fn event_row(event: &InputEvent) -> Vec<String> {
    vec![
        event.monotonic_us.to_string(),
        num(event.time),
        kind_name(event.kind).into(),
        num(event.position.x),
        num(event.position.y),
        quote(&event.button),
        num(event.dx),
        num(event.dy),
        event.key_code.to_string(),
        quote(&event.key_name),
        quote_list(&event.modifiers),
    ]
}

fn cases_txt() -> String {
    let mut out = Text::new();
    for case in cases() {
        let result = load_case(&case);
        out.section(&case.label);
        let (error, log) = match result {
            Ok(log) => (String::new(), log),
            Err(error) => (error, InputLog::default()),
        };
        out.row(["error".into(), quote(&sanitize(&error))]);
        let events = log.events();
        out.row(["count".into(), events.len().to_string()]);
        for event in events {
            out.row(event_row(event));
        }
        out.row(["moves".into(), indices(&log.moves(), events)]);
        out.row(["clickDowns".into(), indices(&log.click_downs(false), events)]);
        out.row(["clickDowns(leftRightOnly)".into(), indices(&log.click_downs(true), events)]);
        out.row(["keyCount".into(), log.key_count().to_string()]);
    }
    out.into_string()
}

/// Sections where `QFile::open` fails with a message of Qt's own rather than `strerror`'s.
const QT_OPEN_ERRORS: &[&str] = &[
    "load <directory> first=1000000 duration=-1",
    "load <empty path> first=1000000 duration=-1",
    "loadBundle capture-is-directory duration=-1",
    "loadBundle capture-is-directory duration=3",
    "loadBundle capture-is-directory duration=0",
];

#[test]
fn cases_txt_without_qt_open_errors() {
    let keep = |label: &str| !QT_OPEN_ERRORS.contains(&label);
    let expected = keep_sections(&read_expected("inputlog/cases.txt"), keep);
    let actual = keep_sections(cases_txt().as_bytes(), keep);
    assert_same("inputlog/cases.txt (without Qt's own open errors)", &expected, &actual);
}

#[test]
fn cases_txt_qt_open_errors() {
    let keep = |label: &str| QT_OPEN_ERRORS.contains(&label);
    let expected = keep_sections(&read_expected("inputlog/cases.txt"), keep);
    let actual = keep_sections(cases_txt().as_bytes(), keep);
    assert_same("inputlog/cases.txt (Qt's own open errors)", &expected, &actual);
}
