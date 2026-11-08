//! omareel in Rune, over omareel_core and omareel_record: the whole app, as the `omareel` binary
//! is (same commands, same processes). Run with no arguments it opens the launcher; recording
//! starts the recorder daemon, the recording bar and, when the recording is saved, the editor,
//! each by running this binary again (`/proc/self/exe`) with the command below.
//!
//!   omareel-rune-spike                        the launcher
//!   omareel-rune-spike edit <bundle>          the editor
//!   omareel-rune-spike export <bundle> -o <out.mp4|out.gif> [--fps n] [--width px] [--quality q]
//!                                       [--gif-fps n] [--gif-width px] [--timing]  as `omareel export`
//!   omareel-rune-spike probe <bundle>         as `omareel probe`
//!   omareel-rune-spike record [...]           as `omareel record`
//!   omareel-rune-spike __record-daemon|__record-bar|__record-gsr|__selfview-ipc|__record-bar-ipc ...
//!                                             the recording's own processes (started by the above)
//!
//! The spike's measurement commands stay:
//!
//!   omareel-rune-spike setup <bundle>
//!   omareel-rune-spike preview <bundle> [--upload surface|pixels] [--decode vaapi|software] [--canvas-height <pt>]
//!                                       [--decode-height <px>] [--measure <s>] [--stats <json>]
//!   omareel-rune-spike export <bundle> ... [--gpu-timing] [--png <frame,...>]
//!   omareel-rune-spike editor <bundle> [preview flags] [-- <Rune flags: --snap out.png, --script s.json --video out.mp4, --tree t.json>]
//!   omareel-rune-spike bar

mod bar;
mod camera;
mod comp;
mod editor;
mod export;
mod icons;
mod kit;
mod launcher;
mod preview;
mod recording_bar;
mod setup;
mod theme;
mod video;

fn value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// The recording's processes, which only run omareel_record's code (no window of their own,
/// except `__record-bar`). Returns their exit status.
fn recording_process(args: &[String]) -> Option<i32> {
    let rest = &args[2.min(args.len())..];
    Some(match args.get(1).map(String::as_str)? {
        // Omarchy's REC indicator looks for an argv[0] starting with "gpu-screen-recorder".
        "__record-gsr" => {
            use std::os::unix::process::CommandExt;
            let error = std::process::Command::new("gpu-screen-recorder").arg0("gpu-screen-recorder").args(rest).exec();
            eprintln!("omareel: could not exec gpu-screen-recorder: {error}");
            127
        }
        "__record-daemon" => omareel_record::recorder::daemon::daemon_main(rest, &|message| eprintln!("{message}")),
        "record" => omareel_record::cli::record_command(rest, &mut std::io::stdout().lock(), &mut std::io::stderr().lock()),
        "__selfview-ipc" => omareel_record::cli::self_view_ipc(rest),
        "__record-bar-ipc" => omareel_record::cli::record_bar_ipc(rest, &mut std::io::stderr().lock()),
        _ => return None,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(status) = recording_process(&args) {
        std::process::exit(status);
    }
    let result = match args.get(1).map(String::as_str) {
        None => launcher::run(),
        Some("__record-bar") => recording_bar::run(&args[2..]),
        Some("edit") if args.len() > 2 => preview::run(preview::Options::editor(&args[2])),
        Some("setup") if args.len() > 2 => setup::run(&args[2]),
        // As `omareel export` (src/main.cpp's exportCommand) and `omareel probe`.
        Some("export") => std::process::exit(export::command(&args[2..])),
        Some("probe") => std::process::exit(export::probe_command(&args[2..])),
        Some(mode @ ("preview" | "editor")) if args.len() > 2 => preview::run(preview::Options {
            editor: mode == "editor",
            // Rune's own flags follow `--`: `--snap out.png`, `--script s.json --video out.mp4`, ...
            rune_args: args.iter().position(|a| a == "--").map(|i| args[i + 1..].to_vec()).unwrap_or_default(),
            bundle: args[2].clone(),
            upload: match value(&args, "--upload") {
                Some("pixels") => video::Upload::Pixels,
                _ => video::Upload::Surface,
            },
            decode: match value(&args, "--decode") {
                Some("software") => video::Decode::Software,
                Some("vaapi") => omareel_core::export::hardware::vaapi_device().map_or(video::Decode::Software, video::Decode::Vaapi),
                _ => video::Decode::auto(),
            },
            canvas_height: value(&args, "--canvas-height").and_then(|v| v.parse().ok()).unwrap_or(620.0),
            decode_height: value(&args, "--decode-height").and_then(|v| v.parse().ok()).unwrap_or(0),
            measure: value(&args, "--measure").and_then(|v| v.parse().ok()),
            stats: value(&args, "--stats").map(str::to_owned),
        }),
        Some("bar") => bar::run(),
        _ => Err("usage: omareel-rune-spike setup|preview|export|bar ...".into()),
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
