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
\x20 export <bundle> -o <file.mp4|file.gif> [options]\n\
\x20     Export with --fps N, --width W, --quality LEVEL, --gif-fps N,\n\
\x20     --gif-width W, or --timing.\n\
\x20 probe <bundle>\n\
\x20     Print a JSON summary of a recording bundle.\n\
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
