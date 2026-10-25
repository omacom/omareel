//! The Rust port checked against the goldens in `tests/golden/expected`, which `tools/oracle`
//! recorded from the C++ code at `rust-migration-base`. Every test regenerates one golden file
//! (or a directory of them) from `tests/golden/corpus` exactly as `tests/golden/README.md`
//! describes it and compares the whole text; once the C++ is deleted these files are its
//! specification.
//!
//! The goldens were recorded on x86_64, and results that depend on float arithmetic the C++
//! compiles differently for aarch64 are checked only on x86_64 (`#[cfg(target_arch = "x86_64")]`):
//! - `qround/round.txt`: x86_64's `qRound`/`qRound64` add a half and truncate (out of range
//!   gives `INT_MIN`); aarch64's use `round()` and saturate;
//! - the QColor files that round channels with `qRound(float)`: lighter/darker (whose HSV step
//!   aarch64 also fuses into one multiply-subtract), `setAlphaF` and `fromRgbF`;
//! - `easing/ease-in-out-cubic.txt`, which prints glibc `pow` results bit for bit, and glibc
//!   computes `pow` with FMA on aarch64;
//! - `theme/cases.txt`, whose colours are mixed in float channels, rounded with qRound, and
//!   chosen by a luminance computed with `pow`;
//! - `layouttimeline/sample.txt`, whose transitions ease through glibc `pow`;
//! - `motiontrack/sample.txt`: 240 Hz spring integration over thousands of steps, glibc's
//!   `hypot`, and zoom targets eased with `pow` (the README defines it on x86_64 only);
//! - `composition/editor.txt`, whose rows sample that motion track and ease layouts with `pow`.
//!
//! The Rust code follows each architecture's C++, so on aarch64 those files need not match.
//! Everything else is byte formatting, parsing, or IEEE arithmetic that Rust evaluates the same
//! way everywhere (cursorpath only compares `hypot` results with thresholds, never prints them;
//! cliptimeline, cameratimeline and keystroketrack only add, multiply, divide and compare), and
//! runs on every architecture.
//!
//! The project, omarchypaths and theme families read the environment and write under `$HOME`;
//! `hermetic` rebuilds the oracle's hermetic world for them (fresh `HOME` and XDG directories,
//! the golden shims first on `PATH`, `umask 022`, `$HOME` as the working directory), one test
//! at a time behind a mutex, and puts the process back afterwards.
//!
//! Rows the Rust API cannot produce (no counterpart yet, or unpaired surrogates a Rust string
//! cannot hold) are left out or normalised where each test says so; known differences from the
//! C++ are `#[ignore]`d tests whose reason states the difference.

mod hermetic;
mod support;

mod cameratimeline;
mod cliptimeline;
#[cfg(target_arch = "x86_64")]
mod composition_editor;
mod cursorpath;
mod easing;
mod inputlog;
mod keystroketrack;
#[cfg(target_arch = "x86_64")]
mod layouttimeline;
#[cfg(target_arch = "x86_64")]
mod motiontrack;
mod omarchypaths;
mod project;
mod qcolor;
mod qjson;
mod qnumber;
mod qround;
mod theme;
mod zoomtimeline;
