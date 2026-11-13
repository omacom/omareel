//! The agent runner: reading each agent's events (from recorded runs), the command lines, the
//! fake agent end to end, and stopping.

use super::*;
use crate::compat::qdir::tests::TempDir;
use std::sync::{Arc, Mutex};
use std::time::Instant;

fn fixture(name: &str) -> String {
    let path = format!("{}/../../tests/fixtures/agent/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn claude() -> &'static Adapter {
    adapter("claude").unwrap()
}

fn codex() -> &'static Adapter {
    adapter("codex").unwrap()
}

fn parse(adapter: &Adapter, text: &str) -> (Vec<Event>, bool) {
    let mut parser = StreamParser::new(adapter);
    let mut events = parser.feed(text.as_bytes());
    events.extend(parser.finish());
    (events, parser.finished())
}

/// A real `claude -p --output-format stream-json` run (Claude Code 2.1.322, Haiku): describe,
/// an apply with the operations inline, a contact sheet, reading it, the summary.
#[test]
fn reads_a_recorded_claude_run() {
    let (events, finished) = parse(claude(), &fixture("claude-stream.jsonl"));
    assert!(finished);
    let kinds: Vec<&str> = events.iter().map(Event::kind).collect();
    assert_eq!(kinds, ["started", "command", "apply", "command", "image", "message", "finished"], "{events:#?}");
    assert_eq!(events[0], Event::Started { agent: "Claude Code".into(), model: "claude-haiku-5-5".into() });
    assert_eq!(events[1], Event::Command("omareel describe .".into()));
    assert_eq!(
        events[2],
        Event::Applied { ok: true, summary: "Applied delete".into(), output_duration: Some(28.0), error: String::new() }
    );
    assert_eq!(events[2].text(), "Applied delete · now 28.0 s");
    assert_eq!(events[4], Event::Looked("/home/user/.cache/omareel/agent/Demo.omareel-1a2b3c4d/sheet-1.jpg".into()));
    let Event::Finished { ok, summary, cost_usd, duration_ms, turns, tokens } = &events[6] else { panic!() };
    assert!(ok);
    assert!(summary.starts_with("I cut the first two seconds"), "{summary}");
    assert_eq!((*cost_usd, *duration_ms, *turns), (Some(0.0035907100000000004), Some(7716.0), Some(5)));
    assert!(tokens.unwrap() > 10_000);
}

/// A command the permissions refuse (a real run's) shows as a problem.
#[test]
fn reads_a_refused_command() {
    let (events, finished) = parse(claude(), &fixture("claude-denied.jsonl"));
    assert!(!finished);
    assert_eq!(events.len(), 2, "{events:#?}");
    assert_eq!(events[0], Event::Command("ls /".into()));
    let Event::Problem(text) = &events[1] else { panic!("{events:#?}") };
    assert!(text.starts_with("ls /: Permission to use Bash has been denied"), "{text}");
}

/// `codex exec --json` (its documented event shapes; not a recorded run).
#[test]
fn reads_a_codex_run() {
    let (events, finished) = parse(codex(), &fixture("codex-stream.jsonl"));
    assert!(finished);
    let kinds: Vec<&str> = events.iter().map(Event::kind).collect();
    assert_eq!(kinds, ["started", "command", "message", "apply", "apply", "command", "problem", "message", "finished"], "{events:#?}");
    assert_eq!(events[1], Event::Command("omareel describe .".into()));
    assert_eq!(events[3].text(), "Applied delete · now 28.0 s");
    assert_eq!(
        events[4].text(),
        "Apply failed: operation 1 (speed): speed must be one of 0.5, 0.75, 1, 1.25, 1.5, 2, 4"
    );
    assert_eq!(events[6], Event::Problem("ls ~: ls: cannot open directory '/home/user': Permission denied".into()));
    let Event::Finished { ok, summary, cost_usd, tokens, .. } = &events[8] else { panic!() };
    assert!(ok);
    assert_eq!(summary, "Cut the first 2 s; the video is now 28 s.");
    assert_eq!((*cost_usd, *tokens), (None, Some(24885)));
}

/// Lines split anywhere between chunks read the same as whole.
#[test]
fn chunks_split_anywhere() {
    for (adapter, file) in [(claude(), "claude-stream.jsonl"), (codex(), "codex-stream.jsonl")] {
        let text = fixture(file);
        let (whole, _) = parse(adapter, &text);
        let mut parser = StreamParser::new(adapter);
        let mut events = Vec::new();
        for chunk in text.as_bytes().chunks(37) {
            events.extend(parser.feed(chunk));
        }
        events.extend(parser.finish());
        assert_eq!(events, whole, "{file}");
    }
    // A last line without its newline, and noise that isn't JSON.
    let mut parser = StreamParser::new(codex());
    assert!(parser.feed(b"warning: something\n{\"type\":\"thread.sta").is_empty());
    assert_eq!(parser.feed(b"rted\"}\n{\"type\":\"turn.failed\",\"error\":{\"message\":\"quota\"}}").len(), 1);
    assert_eq!(parser.finish(), vec![Event::Finished {
        ok: false,
        summary: "quota".into(),
        cost_usd: None,
        duration_ms: None,
        turns: None,
        tokens: None
    }]);
}

#[test]
fn recognises_applies_and_shows_commands() {
    assert!(is_apply("omareel apply . '[{\"op\": \"delete\"}]'"));
    assert!(is_apply("bash -lc 'omareel apply . -'"));
    assert!(is_apply("cd x && /usr/bin/omareel apply . ops.json"));
    assert!(!is_apply("omareel apply . ops.json --dry-run"));
    assert!(!is_apply("omareel describe . | grep apply"));
    assert_eq!(display_command("bash -lc 'omareel describe .'"), "omareel describe .");
    assert_eq!(display_command("/bin/bash -lc \"omareel apply . -\""), "omareel apply . -");
    assert_eq!(display_command("omareel apply . - <<'OPS'\n[]\nOPS"), "omareel apply . - <<'OPS' …");
}

#[test]
fn finds_rendered_frames() {
    let dir = TempDir::new("agent-frames");
    std::fs::write(dir.join("sheet.jpg"), b"x").unwrap();
    std::fs::write(dir.join("f-1.png"), b"x").unwrap();
    std::fs::write(dir.join("f-2.png"), b"x").unwrap();
    assert_eq!(rendered_images("omareel frame . --count 4 --sheet -o sheet.jpg", &dir.0), vec![dir.join("sheet.jpg")]);
    assert_eq!(rendered_images(&format!("omareel frame . --at 1 --at 2 -o {}", dir.join("f.png")), "/"), vec![
        dir.join("f-1.png"),
        dir.join("f-2.png")
    ]);
    assert!(rendered_images("omareel frame . --at 1 -o missing.png", &dir.0).is_empty());
    assert!(rendered_images("convert -o sheet.jpg", &dir.0).is_empty());
}

#[test]
fn command_lines_hold_the_agent_to_omareel() {
    let task = Task {
        bundle: "/videos/Demo.omareel".into(),
        kind: TaskKind::Ask("make it shorter".into()),
        scratch: PathBuf::from("/cache/agent/Demo"),
        omareel: None,
        model: Some("haiku".into()),
    };
    let args = arguments(claude(), &task);
    let joined = args.join(" ");
    for expected in [
        "-p --output-format stream-json --verbose --restricted --tools Bash,Read",
        "--allowedTools Bash(omareel describe:*) Bash(omareel apply:*) Bash(omareel frame:*) Bash(omareel transcribe:*) Bash(omareel help) Read --permission-mode dontAsk",
        "--strict-mcp-config --no-session-persistence --add-dir /cache/agent/Demo",
        "--model haiku --",
    ] {
        assert!(joined.contains(expected), "{expected} in {joined}");
    }
    let system = &args[args.iter().position(|a| a == "--append-system-prompt").unwrap() + 1];
    assert!(system.starts_with("---\nname: omareel"));
    assert!(system.contains("the bundle at /videos/Demo.omareel"));
    assert!(system.contains("/cache/agent/Demo/sheet-1.jpg"));
    assert!(args.last().unwrap().contains("\"make it shorter\""));

    let args = arguments(codex(), &task);
    let joined = args.join(" ");
    assert!(joined.starts_with("exec --json --skip-git-repo-check --ephemeral --sandbox workspace-write --ignore-user-config --ignore-rules"), "{joined}");
    for expected in [
        "-c sandbox_workspace_write.network_access=false",
        "-c sandbox_workspace_write.writable_roots=[]",
        "-c web_search=\"disabled\"",
    ] {
        assert!(joined.contains(expected), "{expected} in {joined}");
    }
    assert!(!joined.contains("Bash(omareel:*)"));
    assert!(joined.contains("--cd /videos/Demo.omareel --add-dir /cache/agent/Demo --model haiku --"));
    let prompt = args.last().unwrap();
    assert!(prompt.starts_with("---\nname: omareel") && prompt.contains("# Task") && prompt.contains("make it shorter"));

    for kind in [TaskKind::Polish, TaskKind::Chapters] {
        let prompt = user_prompt(&Task { kind, ..task.clone() });
        assert!(prompt.contains("omareel describe ."));
    }
    assert!(user_prompt(&Task { kind: TaskKind::Chapters, ..task.clone() }).contains("\"op\": \"set-chapters\""));
}

#[test]
fn says_why_an_agent_cannot_run() {
    assert!(Choice::NoneChosen.problem().contains("Setup › Defaults › Agent"));
    let unsupported = Choice::Unsupported { id: "opencode".into() }.problem();
    assert!(unsupported.starts_with("Omarchy's default agent is OpenCode, which omareel can't run"), "{unsupported}");
    assert!(unsupported.contains("Claude Code and Codex"));
    assert_eq!(agent_name("kiro-cli"), "Kiro CLI");
    assert_eq!(agent_name("something-new"), "something-new");
    assert!(Choice::NotInstalled { adapter: codex() }.problem().contains("`codex`"));
}

/// The fake agent (tests/tools/fake-agent.py) speaks each agent's stream and runs `omareel`
/// commands; here `omareel` is a stub that records the operations and answers as apply does.
fn fake_run(adapter: &'static Adapter, kind: TaskKind, extra_env: &[(&str, &str)]) -> (TempDir, Result<Outcome, String>, Vec<Event>, Runner) {
    fake_run_with(Runner::new(), adapter, kind, extra_env)
}

fn fake_run_with(
    runner: Runner,
    adapter: &'static Adapter,
    kind: TaskKind,
    extra_env: &[(&str, &str)],
) -> (TempDir, Result<Outcome, String>, Vec<Event>, Runner) {
    let dir = TempDir::new("agent-run");
    let bundle = dir.join("Demo.omareel");
    std::fs::create_dir_all(&bundle).unwrap();
    let stub = dir.join("omareel-stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\n\
         case \"$1\" in\n\
         describe) echo '{\"output\": {\"duration\": 30}}' ;;\n\
         apply) printf '%s' \"$3\" > \"$PWD/applied.json\"; echo '{\"ok\": true, \"saved\": true, \"results\": [{\"op\": \"delete\"}, {\"op\": \"set\"}], \"output_duration\": 22.5}' ;;\n\
         frame) while [ $# -gt 0 ]; do [ \"$1\" = -o ] && printf x > \"$2\"; shift; done ;;\n\
         esac\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let task = Task { bundle: bundle.clone(), kind, scratch: PathBuf::from(dir.join("scratch")), omareel: Some(PathBuf::from(&stub)), model: None };
    let fake = format!("{}/../../tests/tools/fake-agent.py", env!("CARGO_MANIFEST_DIR"));
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&events);
    // The fake agent reads its settings from the environment it inherits.
    let mut command_env: Vec<(String, String)> = extra_env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    command_env.push(("FAKE_AGENT_LOG".into(), dir.join("fake-agent.json")));
    let program = dir.join("fake-agent");
    let exports: String = command_env.iter().map(|(k, v)| format!("export {k}='{v}'\n")).collect();
    std::fs::write(&program, format!("#!/bin/sh\n{exports}exec python3 {fake} \"$@\"\n")).unwrap();
    std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let stopper = runner.clone();
    let pids = extra_env.iter().find(|(k, _)| *k == "FAKE_AGENT_PIDS").map(|(_, v)| v.to_string());
    let stop = extra_env.iter().any(|(k, _)| *k == "FAKE_AGENT_STOP");
    if let Some(pids) = pids.filter(|_| stop) {
        std::thread::spawn(move || {
            let started = Instant::now();
            while !Path::new(&pids).is_file() && started.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(20));
            }
            std::thread::sleep(Duration::from_millis(100));
            stopper.stop();
        });
    }
    let result = runner.run(adapter, Path::new(&program), &task, move |event| seen.lock().unwrap().push(event));
    let events = events.lock().unwrap().clone();
    (dir, result, events, runner)
}

fn have_python() -> bool {
    Command::new("python3").arg("--version").output().is_ok_and(|output| output.status.success())
}

#[test]
fn runs_the_fake_agent_end_to_end() {
    if !have_python() {
        eprintln!("skipped: no python3");
        return;
    }
    for adapter in [claude(), codex()] {
        let (dir, result, events, runner) = fake_run(adapter, TaskKind::Polish, &[]);
        let outcome = result.unwrap_or_else(|error| panic!("{}: {error}", adapter.id));
        assert!(outcome.ok);
        assert_eq!(outcome.applies, 1, "{events:#?}");
        assert!(outcome.summary.starts_with("Cut the first 1.5 s"), "{}", outcome.summary);
        assert_eq!(outcome.cost_usd, (adapter.id == "claude").then_some(0.0123));
        // The agent ran in the bundle, with the stub first on its PATH and the task's
        // restrictions on its command line.
        let log = qjson::object_from_json(&std::fs::read(dir.join("fake-agent.json")).unwrap()).unwrap();
        assert_eq!(log.value("cwd").as_str(), Some(dir.join("Demo.omareel").as_str()));
        assert!(log.value("path").to_string_or("").starts_with(&dir.join("scratch/bin:")));
        let argv: Vec<String> = log.value("argv").to_array().iter().map(|v| v.to_string_or("")).collect();
        assert!(argv.contains(&if adapter.id == "claude" { "--restricted".to_string() } else { "workspace-write".to_string() }));
        // The operations reached omareel as one argument.
        let applied = std::fs::read_to_string(dir.join("Demo.omareel/applied.json")).unwrap();
        assert!(applied.starts_with("[{\"op\": \"delete\", \"ranges\": [[0, 1.5], [9, 15]]"), "{applied}");
        let kinds: Vec<&str> = events.iter().map(Event::kind).collect();
        assert_eq!(kinds, ["started", "message", "command", "apply", "command", "image", "message", "finished"], "{}: {events:#?}", adapter.id);
        assert_eq!(events[3].text(), "Applied delete, set · now 22.5 s");
        assert_eq!(events[5], Event::Looked(dir.join("scratch/sheet.jpg")));
        // The run's output stays in the scratch directory, until the runner goes.
        assert!(std::fs::read_to_string(dir.join("scratch/stream.jsonl")).unwrap().lines().count() >= 8);
        drop(runner);
        assert!(!Path::new(&dir.join("scratch")).exists());
    }
}

#[test]
fn an_agent_that_ends_without_a_result_is_an_error() {
    let dir = TempDir::new("agent-crash");
    let bundle = dir.join("b.omareel");
    std::fs::create_dir_all(&bundle).unwrap();
    let program = dir.join("broken");
    std::fs::write(&program, "#!/bin/sh\necho '{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"m\"}'\necho 'error: unknown option --restricted' >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let task = Task { bundle, kind: TaskKind::Polish, scratch: PathBuf::from(dir.join("scratch")), omareel: None, model: None };
    let error = Runner::new().run(claude(), Path::new(&program), &task, |_| {}).unwrap_err();
    assert_eq!(error, "Claude Code ended without finishing: error: unknown option --restricted");
}

fn gone(pid: i32) -> bool {
    // Killed and reaped, or a zombie waiting for its parent.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat.rfind(')').is_some_and(|end| stat[end + 1..].trim_start().starts_with('Z')),
    }
}

/// Stop kills the agent's process group at once, and what it started in a session of its own.
#[test]
fn stop_kills_the_agents_process_group() {
    if !have_python() {
        eprintln!("skipped: no python3");
        return;
    }
    let pids_dir = TempDir::new("agent-stop");
    let pids = pids_dir.join("pids");
    let started = Instant::now();
    let (_dir, result, events, _runner) = fake_run(claude(), TaskKind::Polish, &[("FAKE_AGENT_SLEEP", "60"), ("FAKE_AGENT_PIDS", &pids), ("FAKE_AGENT_STOP", "1")]);
    assert_eq!(result.unwrap_err(), "Stopped");
    assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
    assert!(events.iter().any(|event| matches!(event, Event::Applied { ok: true, .. })));
    assert!(!events.iter().any(|event| matches!(event, Event::Finished { .. })));
    assert_all_gone(&pids);
}

/// The agent, its child, one that left its group and a daemon re-parented away: all gone.
fn assert_all_gone(pids: &str) {
    let pids: Vec<i32> = std::fs::read_to_string(pids).unwrap().split_whitespace().map(|p| p.parse().unwrap()).collect();
    assert_eq!(pids.len(), 4);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !pids.iter().all(|&pid| gone(pid)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in &pids {
        if !gone(*pid) {
            // SAFETY: cleaning up after a failed test.
            unsafe { libc::kill(*pid, libc::SIGKILL) };
            panic!("{pid} is still running");
        }
    }
}

/// A run that ends normally leaves nothing behind either: not what left the agent's process
/// group, nor a daemon re-parented away from it.
#[test]
fn a_normal_end_kills_what_the_agent_left_running() {
    if !have_python() {
        eprintln!("skipped: no python3");
        return;
    }
    let pids_dir = TempDir::new("agent-end");
    let pids = pids_dir.join("pids");
    let (_dir, result, _events, _runner) = fake_run(codex(), TaskKind::Polish, &[("FAKE_AGENT_SLEEP", "0.5"), ("FAKE_AGENT_PIDS", &pids)]);
    assert!(result.unwrap().ok);
    assert_all_gone(&pids);
}

/// And neither does one that times out.
#[test]
fn a_timeout_kills_what_the_agent_left_running() {
    if !have_python() {
        eprintln!("skipped: no python3");
        return;
    }
    let pids_dir = TempDir::new("agent-timeout");
    let pids = pids_dir.join("pids");
    let started = Instant::now();
    let runner = Runner::with_timeout(Duration::from_secs(3));
    let (_dir, result, _events, _runner) =
        fake_run_with(runner, claude(), TaskKind::Polish, &[("FAKE_AGENT_SLEEP", "60"), ("FAKE_AGENT_PIDS", &pids)]);
    assert_eq!(result.unwrap_err(), "Stopped after 3 seconds");
    assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
    assert_all_gone(&pids);
}

/// The agent's `omareel` runs with the scope set, naming the bundle and this run's own
/// scratch directory; two tasks on one bundle get different ones.
#[test]
fn each_run_has_its_own_scratch_directory() {
    let dir = TempDir::new("agent-scratch");
    let cache = dir.join("cache");
    let bundle = dir.join("Demo.omareel");
    std::fs::create_dir_all(&bundle).unwrap();
    let first = Task::in_cache(&bundle, TaskKind::Polish, &cache).unwrap();
    let second = Task::in_cache(&bundle, TaskKind::Polish, &cache).unwrap();
    assert_ne!(first.scratch, second.scratch);
    assert!(first.scratch.to_string_lossy().starts_with(&format!("{cache}/omareel/agent/Demo.omareel-")), "{}", first.scratch.display());
    assert!(first.scratch.is_dir() && second.scratch.is_dir());
    let runner = Runner::new();
    let other = Runner::new();
    let program = Path::new("/nonexistent/agent");
    assert!(runner.run(claude(), program, &first, |_| {}).is_err());
    // A second run on the same bundle doesn't touch the first's directory.
    assert!(other.run(claude(), program, &second, |_| {}).is_err());
    assert!(first.scratch.join("bin").is_dir());
    let command = command(claude(), program, &first);
    let scope = command.get_envs().find(|(key, _)| *key == "OMAREEL_AGENT_SCOPE").and_then(|(_, value)| value).unwrap();
    assert_eq!(scope.to_string_lossy(), format!("{bundle}\n{}", first.scratch.display()));
    drop(runner);
    assert!(!first.scratch.exists());
    assert!(second.scratch.exists());
}
