//! The recording side's port checked against the goldens in `tests/golden/expected` (README,
//! "Replayed programs" to "launcher"), which `tools/oracle` recorded from the C++ at
//! `rust-migration-base`. As in `omareel_core`'s golden tests, every test regenerates one golden
//! file (or a few) from `tests/golden/corpus` exactly as the README describes it and compares it
//! whole. The cell formats and the hermetic world are `omareel_core`'s (`support.rs` and
//! `hermetic.rs`, included from there so there is one copy of each); `replay` adds the
//! oracle's replay scenarios, file listings and timestamp masks.
//!
//! The programs the recording side starts (hyprctl, gpu-screen-recorder, slurp,
//! omarchy-capture-region, pactl, ffmpeg) are the golden shims, first on the hermetic `PATH`,
//! answering from a replay directory. Some scenarios wait for timeouts on purpose (hyprctl's 2 s,
//! gpu-screen-recorder's 3 s, the state lock's 1 s waits), so these tests take some seconds.
//!
//! Gated to x86_64 as `omareel_core`'s `main.rs` explains, because their values go through
//! x86_64's `qRound` (add a half and truncate, where aarch64's uses `round()`):
//! `recorder/self-view-placement.txt`, `launcher/helpers.txt` (`formatDuration` of
//! 0.49999999999999994), `recordingbar/clamped-drag.txt` and `recordingbar/standby-placement.txt`.
//! `recorder/helpers.txt` is compared without its one row for an out-of-range `CaptureMode`
//! (`Mode7`), which a Rust enum cannot hold.

#[allow(dead_code)]
#[path = "../../../omareel_core/tests/golden/hermetic.rs"]
mod hermetic;
#[allow(dead_code)]
mod replay;
#[allow(dead_code)]
#[path = "../../../omareel_core/tests/golden/support.rs"]
mod support;

mod camera;
mod captureexclusion;
mod launcher;
mod recorder;
mod recordingbar;
mod regionpicker;
mod screencapture;
