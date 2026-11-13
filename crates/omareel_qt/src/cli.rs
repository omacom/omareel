//! Command-line entry points C++ calls.

use std::io::Write;

#[cxx_qt::bridge]
pub mod ffi {
    #[namespace = "omareel::cli"]
    extern "Rust" {
        /// Prints the help (to stdout) or a usage error (to stderr) and returns the exit status.
        fn usage(error: &str) -> i32;
        /// `omareel probe <bundle>` for an absolute bundle path: prints the summary and returns
        /// the exit status.
        fn probe(bundle: &str) -> i32;
        /// `omareel describe`, `apply` and `skill` with the arguments after the command; they
        /// need no Qt application, so main() runs them before making one.
        #[cxx_name = "agentCommand"]
        fn agent_command(command: &str, args: Vec<String>) -> i32;
        /// `omareel transcribe <arguments>`: writes the bundle's transcript.json; the exit status.
        fn transcribe(arguments: Vec<String>) -> i32;
        /// `omareel export <bundle> -o <file.srt|file.vtt>` for absolute paths; the exit status.
        #[cxx_name = "exportSubtitles"]
        fn export_subtitles(bundle: &str, output: &str) -> i32;
        /// Sends the Rust code's Qt-style warnings through Qt's message handler, as the C++'s
        /// qWarning went; main() calls it first.
        #[cxx_name = "installMessageHandler"]
        fn install_message_handler();
        /// Starts opening `bundle` for `omareel edit` (its ffprobe runs, the project and the
        /// input log) on a thread, before Qt starts; the Editor takes the document.
        #[cxx_name = "preloadEditor"]
        fn preload_editor(bundle: &str);
        /// Starts the theme's fc-match and hyprctl runs on threads, for a window to come.
        #[cxx_name = "prefetchTheme"]
        fn prefetch_theme();
        /// The launcher's hyprctl checks (a held hardware cursor, the capture-exclusion plugin)
        /// on a thread, for its constructor to take (launcher.rs).
        #[cxx_name = "prefetchLauncher"]
        fn prefetch_launcher();
    }
}

fn preload_editor(bundle: &str) {
    use omareel_core::editor::{Document, Probes};
    Document::preload(bundle, Probes::new(std::sync::Arc::new(omareel_core::media_probe::probe_video)));
}

fn transcribe(arguments: Vec<String>) -> i32 {
    omareel_core::transcript::command::transcribe_command(&arguments)
}

fn export_subtitles(bundle: &str, output: &str) -> i32 {
    omareel_core::transcript::command::export_subtitles_command(bundle, output)
}

fn prefetch_theme() {
    omareel_core::theme::prefetch();
}

fn prefetch_launcher() {
    crate::launcher::prefetch_capture_exclusion();
}

fn install_message_handler() {
    omareel_core::compat::qlog::set_warning_handler(|message| {
        crate::record::ffi::warning(&cxx_qt_lib::QString::from(message));
    });
}

fn usage(error: &str) -> i32 {
    let text = omareel_core::cli::usage_text(error);
    if error.is_empty() {
        let _ = std::io::stdout().lock().write_all(text.as_bytes());
    } else {
        let _ = std::io::stderr().lock().write_all(text.as_bytes());
    }
    omareel_core::cli::usage_status(error)
}

fn agent_command(command: &str, args: Vec<String>) -> i32 {
    let absolute = |path: &str| omareel_core::compat::qdir::absolute_path(path);
    if command == "skill" {
        return skill(&args);
    }
    if command == "describe" {
        return match args.as_slice() {
            [bundle] => describe(&absolute(bundle)),
            _ => usage("describe requires a bundle"),
        };
    }
    let mut positional = Vec::new();
    let (mut dry_run, mut describe_after) = (false, false);
    for arg in &args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "--describe" => describe_after = true,
            option if option.starts_with("--") => return usage(&format!("unknown apply option {option}")),
            _ => positional.push(arg.as_str()),
        }
    }
    match positional.as_slice() {
        [bundle, ops] => apply(&absolute(bundle), ops, dry_run, describe_after),
        _ => usage("apply requires a bundle and an operations file (or - for stdin)"),
    }
}

fn skill(args: &[String]) -> i32 {
    use omareel_core::agent::skill;
    match args {
        [] => {
            let _ = std::io::stdout().lock().write_all(skill::SKILL.as_bytes());
            0
        }
        [install] if install == "install" => {
            let Some(home) = std::env::var_os("HOME") else {
                let _ = writeln!(std::io::stderr(), "omareel: HOME is not set");
                return 1;
            };
            match skill::install(std::path::Path::new(&home)) {
                Ok(report) => {
                    let mut out = std::io::stdout().lock();
                    for line in report {
                        let _ = writeln!(out, "{line}");
                    }
                    0
                }
                Err(error) => {
                    let _ = writeln!(std::io::stderr(), "omareel: {error}");
                    1
                }
            }
        }
        _ => usage("skill takes no arguments, or install"),
    }
}

fn describe(bundle: &str) -> i32 {
    match omareel_core::agent::describe::run(bundle) {
        Ok(summary) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(summary.as_bytes());
            let _ = out.write_all(b"\n");
            let _ = out.flush();
            0
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "omareel: {error}");
            1
        }
    }
}

fn apply(bundle: &str, ops: &str, dry_run: bool, describe_after: bool) -> i32 {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = if ops == "-" {
        std::io::stdin().lock().read_to_end(&mut bytes).map(|_| ())
    } else {
        std::fs::read(ops).map(|contents| bytes = contents)
    };
    if let Err(error) = read {
        let _ = writeln!(std::io::stderr(), "omareel: cannot read {ops}: {error}");
        return 1;
    }
    let (report, ok) = omareel_core::agent::apply::run(bundle, &bytes, dry_run, describe_after);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(report.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
    if ok { 0 } else { 1 }
}

fn probe(bundle: &str) -> i32 {
    match omareel_core::probe::run(bundle) {
        Ok(summary) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(summary.as_bytes());
            let _ = out.flush();
            0
        }
        Err(error) => usage(&error),
    }
}
