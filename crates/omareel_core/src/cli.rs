//! Command-line text shared by every command.

/// The help text `omareel help` prints, and the tail of every usage error.
pub const USAGE: &str = "usage: omareel [command]\n\n\
commands:\n\
\x20 record [--region|--fullscreen|--window] [options]\n\
\x20     Toggle recording (smart gesture is the default: drag an area, click a\n\
\x20     window, or click the desktop for the whole screen). Options: --fps N,\n\
\x20     --dir PATH,\n\
\x20     --with-desktop-audio, --with-microphone-audio, --no-audio,\n\
\x20     --with-webcam, --webcam-device PATH, --webcam-height 720|1080,\n\
\x20     --no-webcam, --no-selfview, --no-open, --no-bar,\n\
\x20     --stop, --cancel.\n\
\x20 edit <bundle.omareel>\n\
\x20     Open a recording bundle in the editor.\n\
\x20 export <bundle> -o <file.mp4|file.gif|file.srt|file.vtt> [options]\n\
\x20     Export with --fps N, --width W, --quality LEVEL, --gif-fps N,\n\
\x20     --gif-width W, or --timing. .srt and .vtt write the captions.\n\
\x20 frame <bundle> --at SECONDS [--at ...] -o <file.png|file.jpg> [options]\n\
\x20     Render the export's frames at output times (12.5 or 1:02.5) as images\n\
\x20     (out-1.png, out-2.png... for several, or use %d). --count N instead\n\
\x20     spreads N frames; --sheet puts them on one labelled contact sheet.\n\
\x20     Options: --width W (a sheet's whole width), --fps N.\n\
\x20 transcribe <bundle> [--backend auto|voxtype|hex|superwhisper|whisper-cpp]\n\
\x20     [--force]\n\
\x20     Write transcript.json with Omarchy's dictation engine, on this computer.\n\
\x20 probe <bundle>\n\
\x20     Print a JSON summary of a recording bundle.\n\
\x20 describe <bundle>\n\
\x20     Print the edited timeline as JSON: clips, zooms, clicks, shortcuts,\n\
\x20     idle stretches and settings, in output and source seconds.\n\
\x20 apply <bundle> <ops.json|-|'[...]'> [--dry-run] [--describe]\n\
\x20     Run edit operations (split, delete, speed, zoom, set, set-chapters,\n\
\x20     ...) as the editor would, all or nothing; an open editor takes the\n\
\x20     result in. The operations come from a file, stdin, or the argument.\n\
\x20 agent <bundle> polish|chapters|ask <request...>\n\
\x20     Run Omarchy's default coding agent (Claude Code or Codex) on the\n\
\x20     recording, printing what it does as JSON lines.\n\
\x20 skill [install]\n\
\x20     Print the skill for coding agents, or install it for them.\n\
\x20 help\n\
\x20     Show this help.\n\n\
Running omareel without a command opens the launcher.\n\
The legacy omarecord command remains available as a compatibility alias.\n";

/// What `usage` prints: the help alone, or an error line before it.
pub fn usage_text(error: &str) -> String {
    if error.is_empty() { USAGE.to_owned() } else { format!("omareel: {error}\n{USAGE}") }
}

/// Exit status for a usage message: 0 for help, 2 for a usage error.
pub fn usage_status(error: &str) -> i32 {
    if error.is_empty() { 0 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_comes_first_and_sets_the_status() {
        assert!(usage_text("").starts_with("usage: omareel [command]\n\ncommands:\n  record "));
        assert_eq!(usage_text("probe requires a bundle").lines().next(), Some("omareel: probe requires a bundle"));
        assert_eq!((usage_status(""), usage_status("x")), (0, 2));
    }
}
