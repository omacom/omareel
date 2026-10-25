//! `Recorder` (`src/record/Recorder.cpp`): the recorder's static decisions and the file-static
//! helpers its daemon uses, without the daemon's capture loop.
//!
//! This module holds the decisions over plain values (how a gpu-screen-recorder exit is
//! classified, the even capture size, the camera block of `capture.json`, the microphone checks,
//! the discard order, the daemon's argument lookups); [`files`] the files the daemon writes
//! (`*.ts` timestamps, `input.jsonl`, the JSON files, the recorder's output tail); [`audio`] the
//! separate ffmpeg audio capture and its mux; [`self_view`] the monitors and where the self-view
//! starts; [`state`] the state files the daemon, the bar and the launcher share.

pub mod audio;
pub mod files;
pub mod self_view;
pub mod state;

use crate::capture::{CaptureMode, CaptureRegion};
use omareel_core::compat::qjson::Object;
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::{qnumber, qstring};

/// `Recorder::GsrExitClassification`: how the screen recorder ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GsrExitClassification {
    /// Stopped because the user asked, with a usable video.
    UserStop,
    /// Stopped by something else (the recorder exited on its own), with a usable video.
    ExternalStop,
    /// No usable video.
    Failure,
}

/// `Recorder::classifyGsrExit`: a failure unless the recorder exited 0 and left a non-empty
/// file with a finite, positive probed duration; otherwise whether the stop was requested.
pub fn classify_gsr_exit(
    exit_code: i32,
    stop_requested: bool,
    file_size: i64,
    probed_duration: f64,
) -> GsrExitClassification {
    if exit_code != 0 || file_size <= 0 || !probed_duration.is_finite() || probed_duration <= 0.0 {
        return GsrExitClassification::Failure;
    }
    if stop_requested { GsrExitClassification::UserStop } else { GsrExitClassification::ExternalStop }
}

/// `Recorder::evenCaptureRegion`: yuv420p encoders refuse odd frame sizes, so the physical size
/// is trimmed to even numbers (`n - n % 2`, so a negative odd size moves towards zero), and the
/// logical size follows when the scale is positive. A region that is already even comes back
/// as it is.
pub fn even_capture_region(mut region: CaptureRegion) -> CaptureRegion {
    let width = region.physical_width - region.physical_width % 2;
    let height = region.physical_height - region.physical_height % 2;
    if width == region.physical_width && height == region.physical_height {
        return region;
    }
    region.physical_width = width;
    region.physical_height = height;
    if region.scale > 0.0 {
        region.width = f64::from(width) / region.scale;
        region.height = f64::from(height) / region.scale;
    }
    region
}

/// The `shape` argument `Recorder::cameraCaptureBlock` defaults to.
pub const DEFAULT_CAMERA_SHAPE: &str = "round";
/// The `radius` argument `Recorder::cameraCaptureBlock` defaults to.
pub const DEFAULT_CAMERA_RADIUS: f64 = 16.0;

/// `Recorder::cameraCaptureBlock`: the `camera` object of `capture.json`. A requested height
/// other than 720 is 1080, and a shape other than `square` or `rectangle` is `round`. The C++
/// defaults the last two arguments to [`DEFAULT_CAMERA_SHAPE`] and [`DEFAULT_CAMERA_RADIUS`].
#[allow(clippy::too_many_arguments)]
pub fn camera_capture_block(
    device: &str,
    requested_height: i32,
    width: i32,
    height: i32,
    fps: f64,
    first_frame_us: i64,
    backend: &str,
    rotation: i32,
    flip_horizontal: bool,
    shape: &str,
    radius: f64,
) -> Object {
    let camera_shape = if shape == "square" || shape == "rectangle" { shape } else { "round" };
    let mut block = Object::new();
    block.insert("device", device);
    block.insert("requestedHeight", if requested_height == 720 { 720 } else { 1080 });
    block.insert("width", width);
    block.insert("height", height);
    block.insert("fps", fps);
    block.insert("first_frame_us", first_frame_us);
    block.insert("backend", backend);
    block.insert("rotation", rotation);
    block.insert("flipHorizontal", flip_horizontal);
    block.insert("shape", camera_shape);
    block.insert("radius", radius);
    block
}

/// `Recorder::DiscardActions`: what discarding a recording does, each step optional.
#[derive(Default)]
pub struct DiscardActions<'a> {
    pub abort_capture: Option<Box<dyn FnMut() + 'a>>,
    pub kill_audio: Option<Box<dyn FnMut() + 'a>>,
    pub request_camera_stop: Option<Box<dyn FnMut() + 'a>>,
    /// Called with the timeout in milliseconds (1000).
    pub wait_for_camera_stop: Option<Box<dyn FnMut(i32) + 'a>>,
    /// Whether the bundle is gone afterwards.
    pub remove_bundle: Option<Box<dyn FnMut() -> bool + 'a>>,
    pub remove_state: Option<Box<dyn FnMut() + 'a>>,
}

/// `Recorder::discardTimeline`: the steps in order (abort the capture, kill the audio, ask the
/// camera to stop, wait up to a second for it, remove the bundle, remove the state file). True
/// when there was no bundle step or it removed the bundle.
pub fn discard_timeline(actions: &mut DiscardActions) -> bool {
    if let Some(abort) = actions.abort_capture.as_mut() {
        abort();
    }
    if let Some(kill) = actions.kill_audio.as_mut() {
        kill();
    }
    if let Some(request) = actions.request_camera_stop.as_mut() {
        request();
    }
    if let Some(wait) = actions.wait_for_camera_stop.as_mut() {
        wait(1000);
    }
    let removed = actions.remove_bundle.as_mut().is_none_or(|remove| remove());
    if let Some(remove) = actions.remove_state.as_mut() {
        remove();
    }
    removed
}

/// `Recorder::microphoneMissing`: the chosen microphone is not among the audio server's
/// sources, so the default one has to stand in. `default_input` is never missing, and an empty
/// list (pactl failed) says nothing either way.
pub fn microphone_missing<S: AsRef<str>>(device: &str, sources: &[S]) -> bool {
    device != "default_input" && !sources.is_empty() && !sources.iter().any(|source| source.as_ref() == device)
}

/// `QString::section(sep, start, end)` with the default flags and non-negative bounds: the
/// fields `start..=end` of the text split on `sep` (empty fields kept), joined by `sep`; empty
/// when `start` is past the last field. `end` of `None` is the last field (Qt's -1).
fn section(text: &str, sep: &str, start: usize, end: Option<usize>) -> String {
    let sections: Vec<&str> = text.split(sep).collect();
    let end = end.unwrap_or(sections.len() - 1);
    if start >= sections.len() || start > end {
        return String::new();
    }
    sections[start..=end.min(sections.len() - 1)].join(sep)
}

/// `Recorder::microphonePeakDb`: the loudest microphone sample ffmpeg's volumedetect logged as
/// it exited, in dBFS. ffmpeg configures the filter graph twice and the first volumedetect
/// reports no samples as it is torn down, so the last report counts: `n_samples: 0` makes it
/// −∞ (no sound arrived at all), any other `n_samples` NaN until a `max_volume` follows, and a
/// log without a report is NaN.
pub fn microphone_peak_db(ffmpeg_log: &str) -> f64 {
    let mut peak = f64::NAN;
    for line in ffmpeg_log.split('\n') {
        if !line.contains("Parsed_volumedetect") {
            continue;
        }
        let value = section(line, "] ", 1, None);
        if value.starts_with("n_samples:") {
            let samples = qnumber::to_long_long(&section(&value, " ", 1, Some(1))).unwrap_or(0);
            peak = if samples == 0 { f64::NEG_INFINITY } else { f64::NAN };
        } else if value.starts_with("max_volume:") {
            if let Some(decibels) = qnumber::to_double(&section(&value, " ", 1, Some(1))) {
                peak = decibels;
            }
        }
    }
    peak
}

/// The file-static `commandOutput`: runs `program` and waits up to three seconds; its trimmed
/// standard output (as UTF-8) when it exits 0, else empty.
pub fn command_output<S: AsRef<str>>(program: &str, arguments: &[S]) -> String {
    let mut process = Process::new();
    process.start(program, arguments);
    if !process.wait_for_finished(3000) || process.exit_code() != 0 {
        return String::new();
    }
    qstring::trimmed(&qstring::from_utf8(&process.read_all_standard_output())).to_owned()
}

/// The source names in what `pactl list short sources` printed (the trimmed output, as
/// `daemonMain` gets it from [`command_output`]): the second TAB-separated field of every
/// non-empty line, empty when a line has none.
pub fn pactl_source_names(output: &str) -> Vec<String> {
    output.split('\n').filter(|line| !line.is_empty()).map(|line| section(line, "\t", 1, Some(1))).collect()
}

/// `daemonMain`'s list of the audio server's sources (lines 803–807): `pactl list short sources`
/// through [`command_output`], parsed by [`pactl_source_names`]. Empty when pactl failed.
pub fn pactl_sources() -> Vec<String> {
    pactl_source_names(&command_output("pactl", &["list", "short", "sources"]))
}

/// The file-static `valueAfter`: the argument after the first `name`, or empty when there is
/// none (or `name` is last).
pub fn value_after<S: AsRef<str>>(arguments: &[S], name: &str) -> String {
    match arguments.iter().position(|argument| argument.as_ref() == name) {
        Some(index) if index + 1 < arguments.len() => arguments[index + 1].as_ref().to_owned(),
        _ => String::new(),
    }
}

/// The file-static `modeName`: the `--mode` value and the `capture_mode`/`mode` JSON value.
pub fn mode_name(mode: CaptureMode) -> &'static str {
    match mode {
        CaptureMode::Fullscreen => "fullscreen",
        CaptureMode::Window => "window",
        CaptureMode::Region => "region",
    }
}

/// `CursorSampler::monotonicUs`: `CLOCK_MONOTONIC` in microseconds, the recorder's clock.
pub fn monotonic_us() -> i64 {
    clock_us(libc::CLOCK_MONOTONIC)
}

/// `CLOCK_REALTIME` in microseconds, as the timestamp files record it.
pub fn realtime_us() -> i64 {
    clock_us(libc::CLOCK_REALTIME)
}

fn clock_us(clock: libc::clockid_t) -> i64 {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes one timespec.
    unsafe { libc::clock_gettime(clock, &mut now) };
    now.tv_sec * 1_000_000 + now.tv_nsec / 1000
}
