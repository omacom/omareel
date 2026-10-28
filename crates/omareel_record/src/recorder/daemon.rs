//! `Recorder::daemonMain`: the recorder daemon, `omareel __record-daemon ...`, which
//! [`super::start::start_detached`] starts. It makes the bundle, writes the state file the bar,
//! the launcher and `omareel record --stop` read, starts the recording bar (or adopts the
//! standby self-view host), waits for the overlays to leave the recorded output when the capture
//! exclusion plugin is not there, holds hardware cursors, captures the screen in process
//! ([`crate::screen_capture`]) or through gpu-screen-recorder (`omareel __record-gsr`, whose
//! argv[0] Omarchy's REC indicator matches), records audio beside a native capture, samples the
//! cursor and the input devices, and runs until SIGUSR1 (stop) or SIGUSR2 (discard). Then it
//! finishes the video, muxes the audio, writes `input.jsonl`, `capture.json`, `recording.json`
//! and `thumb.jpg`, says how it went and opens the editor.
//!
//! The order of everything, the files, the messages and the notifications are the C++'s. The
//! locals are declared in the C++'s order, so they go away in its order too: the capture first,
//! then the hardware cursor hold (which puts the user's setting back), then the bar's launch
//! thread, the audio recorder, gpu-screen-recorder, and the samplers last.

use super::audio::{finish_audio_capture, start_audio_capture};
use super::files::{
    drain_recorder_output, first_frame_timestamp, last_non_empty_line, write_input_log, write_json,
};
use super::self_view::{monitor_layouts, self_view_placement};
use super::state::{
    notify_stop_failure, read_state_file, run_optional_detached, self_view_host_path, state_file_path,
    update_recording_state, write_last_error,
};
use super::{
    DiscardActions, GsrExitClassification, camera_capture_block, classify_gsr_exit, debug_start_stage,
    discard_timeline, environment_variable, even_capture_region, microphone_missing, microphone_peak_db, mode_name,
    monotonic_us, pactl_sources, value_after, write_stderr,
};
use crate::capture::{CaptureMode, CaptureRegion, RecordOptions};
use crate::capture_exclusion::{self, HardwareCursorHold};
use crate::cursor_sampler::CursorSampler;
use crate::evdev_listener::EvdevListener;
use crate::screen_capture::{Rect, ScreenCapture, ScreenCaptureConfig};
use omareel_core::compat::qcoreapplication::{application_dir_path, application_file_path, application_pid};
use omareel_core::compat::qdir::{self, FileInfo};
use omareel_core::compat::qjson::{self, Object, Value};
use omareel_core::compat::qprocess::{self, ChannelMode, DetachedChannels, ExitStatus, Process, ProcessState};
use omareel_core::compat::qround::q_round;
use omareel_core::compat::{qfile, qnumber, qstring};
use omareel_core::recording_preferences::RecordingPreferences;
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static DISCARD_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn request_stop(_: libc::c_int) {
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

extern "C" fn request_discard(_: libc::c_int) {
    DISCARD_REQUESTED.store(true, Ordering::SeqCst);
    STOP_REQUESTED.store(true, Ordering::SeqCst);
}

fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::SeqCst)
}

fn msleep(milliseconds: u64) {
    std::thread::sleep(Duration::from_millis(milliseconds));
}

/// `QElapsedTimer::elapsed()` in whole milliseconds.
fn elapsed_ms(timer: &Instant) -> u128 {
    timer.elapsed().as_millis()
}

/// `readState()`.
fn read_state() -> Object {
    read_state_file(&state_file_path())
}

/// `updateRecordingState(values)` with its result ignored, as the daemon calls it.
fn update_state(values: &[(&str, Value)]) {
    let object: Object = values.iter().map(|(key, value)| (*key, value.clone())).collect();
    let _ = update_recording_state(&object);
}

/// `QString::toInt()`: 0 when the text is not a number.
fn to_int(text: &str) -> i32 {
    qnumber::to_int(text).unwrap_or(0)
}

/// `QString::toDouble()`: 0 when the text is not a number.
fn to_double(text: &str) -> f64 {
    qnumber::to_double(text).unwrap_or(0.0)
}

/// `int(std::lround(x))` as glibc rounds (half away from zero, `LONG_MIN` out of range) and GCC
/// narrows.
fn lround_int(x: f64) -> i32 {
    let rounded = x.round();
    let long = if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&rounded) {
        rounded as i64
    } else {
        i64::MIN
    };
    long as i32
}

/// `QDateTime::currentDateTime().toString("yyyy-MM-dd HH-mm-ss")` in the local time zone.
fn current_date_time_text() -> String {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: tzset reads TZ, as Qt does before converting; the rest fills locals.
    let tm = unsafe {
        libc::clock_gettime(libc::CLOCK_REALTIME, &mut now);
        unsafe extern "C" {
            fn tzset();
        }
        tzset();
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now.tv_sec, &mut tm);
        tm
    };
    format!(
        "{:04}-{:02}-{:02} {:02}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// The `std::future` of the bar's launch: its destructor waits for the launch.
struct BarLaunch(Option<JoinHandle<bool>>);

impl Drop for BarLaunch {
    fn drop(&mut self) {
        if let Some(thread) = self.0.take() {
            let _ = thread.join();
        }
    }
}

/// `notifyStartFailure(detail)`.
fn notify_start_failure(detail: &str) {
    run_optional_detached("omarchy-notification-send", &["-u", "critical", "Recording failed", detail]);
}

/// `omarchy-shell -q omarchy.indicators refresh`, so the REC indicator follows.
fn refresh_indicators() {
    run_optional_detached("omarchy-shell", &["-q", "omarchy.indicators", "refresh"]);
}

/// `failRecorderStartup`: stops sampling, removes a bundle without a video, leaves the reason in
/// `last-error.txt` for `omareel record`, removes the state, and says so.
fn fail_recorder_startup(
    reason: &str,
    bundle: &str,
    video: &str,
    cursor_sampler: &mut CursorSampler,
    evdev_listener: &mut EvdevListener,
) -> i32 {
    cursor_sampler.stop();
    evdev_listener.stop();
    if !FileInfo::new(video).is_file() || FileInfo::new(video).size() == 0 {
        qdir::remove_recursively(bundle);
    }
    write_last_error(reason);
    qfile::remove(&state_file_path());
    refresh_indicators();
    notify_start_failure(reason);
    write_stderr(&format!("{reason}\n"));
    2
}

/// The daemon's `waitForFallbackPlacement`: without the plugin, up to two seconds (100 checks
/// 10 ms apart, until a stop) for the overlays to leave the recorded output.
fn wait_for_fallback_placement(plugin_exclusion: bool, monitor: &str) -> bool {
    if plugin_exclusion {
        return true;
    }
    let timer = Instant::now();
    let mut attempt = 0;
    while attempt < 100 && elapsed_ms(&timer) < 2000 && !stop_requested() {
        if capture_exclusion::overlays_off_monitor(monitor) {
            return true;
        }
        msleep(10);
        attempt += 1;
    }
    false
}

/// The arguments the daemon gives gpu-screen-recorder (after `__record-gsr`): the whole output
/// with `-s 0x0` for a fullscreen capture that did not need trimming, else the region rounded to
/// whole logical pixels; no cursor, the first frame's time written, constant frame rate, AAC for
/// the desktop's and the microphone's audio.
pub fn gsr_arguments(region: &CaptureRegion, trimmed: bool, options: &RecordOptions, video: &str) -> Vec<String> {
    let mut gsr: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| gsr.extend(items.iter().map(|item| (*item).to_owned()));
    if region.mode == CaptureMode::Fullscreen && !trimmed {
        push(&["-w", &region.monitor_name, "-s", "0x0"]);
    } else {
        // An output with an odd size is captured as a region too: gpu-screen-recorder passes
        // the monitor size straight to the encoder with -s 0x0, and -s WxH would scale the
        // whole frame instead of trimming a pixel.
        let geometry = format!(
            "{}x{}+{}+{}",
            lround_int(region.width),
            lround_int(region.height),
            lround_int(region.x),
            lround_int(region.y)
        );
        push(&["-w", "region", "-region", &geometry]);
    }
    let fps = options.fps.to_string();
    push(&[
        "-cursor",
        "no",
        "-write-first-frame-ts",
        "yes",
        "-f",
        &fps,
        "-fm",
        "cfr",
        "-k",
        "auto",
        "-fallback-cpu-encoding",
        "yes",
    ]);
    let mut audio_sources = Vec::new();
    if options.desktop_audio {
        audio_sources.push("default_output".to_owned());
    }
    if options.microphone_audio {
        audio_sources.push(options.microphone_device.clone());
    }
    if !audio_sources.is_empty() {
        push(&["-a", &audio_sources.join("|"), "-ac", "aac"]);
    }
    push(&["-o", video]);
    gsr
}

/// The options `daemonMain` reads from its arguments, with the defaults and normalisation of
/// each, and whether the chosen microphone had to be swapped for the default one because the
/// audio server does not have it.
pub fn daemon_options(arguments: &[String], sources: impl FnOnce() -> Vec<String>) -> (RecordOptions, bool) {
    let has = |name: &str| arguments.iter().any(|argument| argument == name);
    let mut options = RecordOptions {
        fps: to_int(&value_after(arguments, "--fps")),
        output_directory: value_after(arguments, "--dir"),
        desktop_audio: has("--desktop-audio"),
        microphone_audio: has("--microphone-audio"),
        microphone_device: value_after(arguments, "--microphone-device"),
        ..RecordOptions::default()
    };
    if options.microphone_device.is_empty() {
        options.microphone_device = "default_input".into();
    }
    // A microphone on a dock or a camera comes and goes with it.
    let mut microphone_replaced = false;
    if options.microphone_audio && microphone_missing(&options.microphone_device, &sources()) {
        options.microphone_device = "default_input".into();
        microphone_replaced = true;
    }
    options.no_open = has("--no-open");
    options.no_bar = has("--no-bar");
    options.countdown = has("--countdown");
    options.webcam = has("--webcam");
    options.webcam_device = value_after(arguments, "--webcam-device");
    if options.webcam_device.is_empty() {
        options.webcam_device = "/dev/video2".into();
    }
    options.webcam_height = to_int(&value_after(arguments, "--webcam-height"));
    if options.webcam_height != 720 {
        options.webcam_height = 1080;
    }
    options.webcam_rotation = to_int(&value_after(arguments, "--webcam-rotation"));
    if !matches!(options.webcam_rotation, 90 | 180 | 270) {
        options.webcam_rotation = 0;
    }
    options.webcam_flip_horizontal = has("--webcam-flip-horizontal");
    options.self_view = !has("--no-selfview");
    options.self_view_size = qstring::to_upper(&value_after(arguments, "--selfview-size"));
    if options.self_view_size != "S" && options.self_view_size != "L" {
        options.self_view_size = "M".into();
    }
    options.capture_backend = value_after(arguments, "--capture-backend");
    if options.capture_backend.is_empty() {
        options.capture_backend = "auto".into();
    }
    (options, microphone_replaced)
}

/// The region `daemonMain` reads from its arguments (`--mode` other than fullscreen or window is
/// a region).
pub fn daemon_region(arguments: &[String]) -> CaptureRegion {
    let mode = value_after(arguments, "--mode");
    CaptureRegion {
        mode: match mode.as_str() {
            "fullscreen" => CaptureMode::Fullscreen,
            "window" => CaptureMode::Window,
            _ => CaptureMode::Region,
        },
        monitor_name: value_after(arguments, "--monitor"),
        x: to_double(&value_after(arguments, "--x")),
        y: to_double(&value_after(arguments, "--y")),
        width: to_double(&value_after(arguments, "--w")),
        height: to_double(&value_after(arguments, "--h")),
        scale: to_double(&value_after(arguments, "--scale")),
        physical_width: to_int(&value_after(arguments, "--physical-w")),
        physical_height: to_int(&value_after(arguments, "--physical-h")),
    }
}

/// The values of the state file the daemon writes before anything starts.
#[allow(clippy::too_many_arguments)]
fn initial_state(
    options: &RecordOptions,
    region: &CaptureRegion,
    daemon_pid: i64,
    bundle: &str,
    daemon_start_us: i64,
    capture_backend: &str,
    plugin_exclusion: bool,
    overlay_monitor: &str,
    self_view: &super::self_view::SelfViewPlacement,
    countdown_end_us: i64,
) -> Object {
    let mut state = Object::new();
    state.insert("pid", daemon_pid);
    state.insert("bundle", bundle);
    state.insert("started_us", daemon_start_us);
    state.insert("daemon_started_us", daemon_start_us);
    state.insert("monitor", region.monitor_name.as_str());
    state.insert("no_open", options.no_open);
    state.insert("webcam", options.webcam);
    state.insert("camera_device", options.webcam_device.as_str());
    state.insert("camera_height", options.webcam_height);
    state.insert("camera_rotation", options.webcam_rotation);
    state.insert("camera_flip_horizontal", options.webcam_flip_horizontal);
    state.insert("capture_mode", mode_name(region.mode));
    state.insert("capture_x", region.x);
    state.insert("capture_y", region.y);
    state.insert("capture_width", region.width);
    state.insert("capture_height", region.height);
    state.insert("capture_backend", capture_backend);
    state.insert("overlay_exclusion", if plugin_exclusion { "plugin" } else { "fallback" });
    state.insert("overlay_monitor", overlay_monitor);
    state.insert("capture_started", false);
    state.insert("countdown_requested", options.countdown);
    state.insert("countdown_active", false);
    state.insert("countdown_ready", false);
    state.insert("countdown_end_us", countdown_end_us);
    state.insert("countdown_width", 240);
    state.insert("countdown_height", 240);
    state.insert("bar_visible", !options.no_bar && !overlay_monitor.is_empty());
    state.insert("bar_width", if options.webcam { 448 } else { 288 });
    state.insert("bar_height", 40);
    state.insert("selfview", self_view.visible);
    state.insert("selfview_safe", self_view.safe);
    state.insert("selfview_monitor", self_view.monitor.as_str());
    state.insert("selfview_size", self_view.size_name.as_str());
    state.insert("selfview_pixels", self_view.pixels);
    state.insert("selfview_x", self_view.position.x);
    state.insert("selfview_y", self_view.position.y);
    state.insert("camera_status", if options.webcam { "starting" } else { "disabled" });
    state.insert("camera_record", false);
    state.insert("camera_stop", false);
    state
}

/// `Recorder::daemonMain(arguments)`: the exit status (0 saved or discarded, 2 failed).
/// `warn` is `qWarning()`, for the warnings the C++ logged through Qt.
pub fn daemon_main(arguments: &[String], warn: &dyn Fn(&str)) -> i32 {
    let daemon_start_us = monotonic_us();
    debug_start_stage("daemon_start", daemon_start_us, "");
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    DISCARD_REQUESTED.store(false, Ordering::SeqCst);
    // SAFETY: the handlers only store to atomics; glibc's signal() installs them with
    // SA_RESTART, as std::signal does.
    unsafe {
        libc::signal(libc::SIGUSR1, request_stop as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGUSR2, request_discard as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let (options, microphone_replaced) = daemon_options(arguments, pactl_sources);
    let mut region = daemon_region(arguments);
    let picked_region = region.clone();
    region = even_capture_region(region);
    let trimmed = region.physical_width != picked_region.physical_width
        || region.physical_height != picked_region.physical_height;
    let preferences = RecordingPreferences::load();
    let mut self_view = self_view_placement(&options, &region, &preferences);

    let display_name = format!("Recording {}", current_date_time_text());
    qdir::mkpath(&options.output_directory);
    let mut bundle = qdir::file_path(&options.output_directory, &format!("{display_name}.omareel"));
    let mut suffix = 2;
    while qdir::file_exists(&bundle) {
        bundle = qdir::file_path(&options.output_directory, &format!("{display_name} {suffix}.omareel"));
        suffix += 1;
    }
    if !qdir::mkpath(&bundle) {
        return 2;
    }

    let daemon_pid = application_pid();
    let started_us = daemon_start_us;
    let mut error = String::new();

    let mut cursor_sampler = CursorSampler::new();
    let mut evdev_listener = EvdevListener::new();

    let video = format!("{bundle}/screen.mp4");
    let camera_video = format!("{bundle}/camera.mp4");
    let gsr = gsr_arguments(&region, trimmed, &options, &video);

    let mut recorder = Process::new();
    recorder.set_process_channel_mode(ChannelMode::Merged);
    let mut recorder_output = Vec::new();
    let mut debug_log: Option<File> = if environment_variable("OMAREEL_DEBUG") == "1" {
        std::fs::OpenOptions::new().append(true).create(true).open("/tmp/omareel.log").ok()
    } else {
        None
    };
    let mut camera_available = false;
    let mut camera_backend = String::new();
    let environment_backend = qstring::to_lower(&environment_variable("OMAREEL_CAPTURE"));
    let mut native_capture = environment_backend != "gsr" && options.capture_backend != "gsr";
    let mut capture_backend = if native_capture { "ext-image-copy-capture" } else { "gsr" }.to_owned();
    let mut plugin_exclusion = capture_exclusion::ensure_loaded(&application_dir_path()).loaded && native_capture;
    let output_names: Vec<String> = monitor_layouts().into_iter().map(|layout| layout.name).collect();
    let mut overlay_monitor = capture_exclusion::overlay_monitor(plugin_exclusion, &region.monitor_name, &output_names);
    if !plugin_exclusion {
        self_view.monitor = overlay_monitor.clone();
        self_view.safe = !overlay_monitor.is_empty();
    }
    let audio_requested = options.desktop_audio || options.microphone_audio;
    let native_video = if audio_requested { format!("{bundle}/screen-video.mp4") } else { video.clone() };
    let audio_video = format!("{bundle}/audio.mka");
    let mut audio_recorder = Process::new();
    let mut audio_started_us = 0i64;
    let mut audio_active = false;
    let mut audio_muxed = !audio_requested;
    let mut microphone_peak = f64::NAN;
    let mut countdown_end_us = 0i64;
    let mut state = initial_state(
        &options,
        &region,
        daemon_pid,
        &bundle,
        daemon_start_us,
        &capture_backend,
        plugin_exclusion,
        &overlay_monitor,
        &self_view,
        countdown_end_us,
    );
    let host = read_state_file(&self_view_host_path());
    let host_pid = host.value("pid").to_variant_long_long();
    // SAFETY: kill with signal 0 only checks that the process exists.
    let adopt_host = options.webcam && host_pid > 0 && unsafe { libc::kill(host_pid as libc::pid_t, 0) } == 0;
    if adopt_host {
        state.insert("host_pid", host_pid);
    }
    if let Err(message) = write_json(&state_file_path(), &state) {
        let reason = if message.is_empty() {
            "Could not write recorder state".to_owned()
        } else {
            format!("Could not write recorder state: {message}")
        };
        return fail_recorder_startup(&reason, &bundle, &video, &mut cursor_sampler, &mut evdev_listener);
    }
    let _bar_launch = BarLaunch(if !adopt_host && (!options.no_bar || options.webcam || options.countdown) {
        let mut bar_arguments = vec!["__record-bar".to_owned()];
        if options.no_bar {
            bar_arguments.push("--hidden".into());
        }
        let application = application_file_path();
        Some(std::thread::spawn(move || {
            qprocess::start_detached(&application, &bar_arguments, DetachedChannels::default()).is_some()
        }))
    } else {
        None
    });
    if adopt_host {
        debug_start_stage("host_adopted", daemon_start_us, &format!("pid={host_pid}"));
        let hide_timer = Instant::now();
        let mut attempt = 0;
        while attempt < 250 && elapsed_ms(&hide_timer) < 250 {
            if read_state().value("host_acknowledged").to_bool(false) {
                break;
            }
            msleep(1);
            attempt += 1;
        }
    }
    if !wait_for_fallback_placement(plugin_exclusion, &region.monitor_name) {
        return fail_recorder_startup(
            "Overlays could not leave the recorded output",
            &bundle,
            &video,
            &mut cursor_sampler,
            &mut evdev_listener,
        );
    }
    // Declared before the capture so the pointer stays off the frames until it has stopped.
    let _hardware_cursor = HardwareCursorHold::new();
    let mut screen_capture = ScreenCapture::new();
    if native_capture {
        let mut crop = Rect::default();
        if region.mode != CaptureMode::Fullscreen {
            let layouts = monitor_layouts();
            if let Some(monitor) = layouts.iter().find(|layout| layout.name == region.monitor_name) {
                crop = Rect::new(
                    q_round((region.x - monitor.geometry.x) * region.scale),
                    q_round((region.y - monitor.geometry.y) * region.scale),
                    region.physical_width,
                    region.physical_height,
                );
            }
        } else {
            crop = Rect::new(0, 0, region.physical_width, region.physical_height);
        }
        let config = ScreenCaptureConfig {
            monitor: region.monitor_name.clone(),
            output_path: native_video.clone(),
            timestamp_path: format!("{video}.ts"),
            crop,
            fps: options.fps,
        };
        if !screen_capture.start(&config, &mut error) {
            native_capture = false;
            capture_backend = "gsr".into();
            plugin_exclusion = false;
            overlay_monitor = capture_exclusion::overlay_monitor(false, &region.monitor_name, &output_names);
            update_state(&[
                ("capture_backend", capture_backend.as_str().into()),
                ("overlay_exclusion", "fallback".into()),
                ("overlay_monitor", overlay_monitor.as_str().into()),
                ("bar_visible", (!options.no_bar && !overlay_monitor.is_empty()).into()),
                ("selfview_monitor", overlay_monitor.as_str().into()),
                ("selfview_safe", (!overlay_monitor.is_empty()).into()),
            ]);
            if !wait_for_fallback_placement(plugin_exclusion, &region.monitor_name) {
                return fail_recorder_startup(
                    "Overlays could not leave the recorded output",
                    &bundle,
                    &video,
                    &mut cursor_sampler,
                    &mut evdev_listener,
                );
            }
        }
    }
    let application = application_file_path();
    if !native_capture {
        let mut recorder_arguments = vec!["__record-gsr".to_owned()];
        recorder_arguments.extend(gsr.iter().cloned());
        recorder.start(&application, &recorder_arguments);
        if !recorder.wait_for_started(5000) {
            if options.webcam {
                update_state(&[("camera_stop", true.into())]);
            }
            drain_recorder_output(&mut recorder, &mut recorder_output, debug_log.as_mut().map(|log| log as &mut dyn Write));
            let mut reason = last_non_empty_line(&recorder_output);
            if reason.is_empty() {
                reason = recorder.error_string();
            }
            return fail_recorder_startup(&reason, &bundle, &video, &mut cursor_sampler, &mut evdev_listener);
        }
    }
    if native_capture && audio_requested {
        let started = start_audio_capture(&mut audio_recorder, &options, &audio_video);
        audio_active = started.started;
        if let Some(us) = started.started_us {
            audio_started_us = us;
        }
    }
    if options.countdown {
        let countdown_host_timer = Instant::now();
        while !read_state().value("countdown_ready").to_bool(false)
            && elapsed_ms(&countdown_host_timer) < 3000
            && !stop_requested()
        {
            msleep(20);
        }
        countdown_end_us = monotonic_us() + 3_000_000;
        update_state(&[("countdown_active", true.into()), ("countdown_end_us", countdown_end_us.into())]);
        debug_start_stage("countdown_begin", daemon_start_us, &format!("end_us={countdown_end_us}"));
        while monotonic_us() < countdown_end_us && !stop_requested() {
            msleep(20);
        }
        update_state(&[("countdown_active", false.into())]);
        msleep(80);
        debug_start_stage("countdown_end", daemon_start_us, "");
    }
    let recorder_started_us = monotonic_us();
    update_state(&[("started_us", recorder_started_us.into())]);
    debug_start_stage("capture_start", daemon_start_us, "");
    let mut capture_loop_failed = false;
    if native_capture {
        let first_frame_timer = Instant::now();
        while screen_capture.encoded_frames() == 0 && elapsed_ms(&first_frame_timer) < 2500 && !stop_requested() {
            if !screen_capture.capture_frame(&mut error) {
                capture_loop_failed = true;
                break;
            }
            msleep(1);
        }
        if screen_capture.encoded_frames() == 0 && !capture_loop_failed {
            error = "Capture did not produce its initial frame".into();
            capture_loop_failed = true;
        }
    }
    let timestamp_path = format!("{video}.ts");
    if !native_capture && !capture_loop_failed {
        let first_frame_timer = Instant::now();
        while first_frame_timestamp(&timestamp_path) <= 0 && elapsed_ms(&first_frame_timer) < 2500 && !stop_requested()
        {
            if recorder.state() == ProcessState::NotRunning {
                capture_loop_failed = true;
                break;
            }
            msleep(5);
        }
        if first_frame_timestamp(&timestamp_path) <= 0 {
            capture_loop_failed = true;
        }
    }
    if !capture_loop_failed {
        update_state(&[("capture_started", true.into()), ("camera_record", options.webcam.into())]);
        let first_us =
            if native_capture { screen_capture.first_frame_us() } else { first_frame_timestamp(&timestamp_path) };
        debug_start_stage("first_frame", daemon_start_us, &format!("first_frame_us={first_us}"));
    }
    if let Err(warning) = cursor_sampler.start(&region.monitor_name, options.fps) {
        warn(&warning);
    }
    if let Err(warning) = evdev_listener.start() {
        warn(&warning);
    }
    if !capture_loop_failed {
        run_optional_detached(
            "omarchy-notification-send",
            &[
                "-t",
                "3000",
                "Recording started",
                if microphone_replaced {
                    "The chosen microphone isn't plugged in, so the default one is recording"
                } else {
                    "Stop with the bar, the REC indicator, or your keybind"
                },
            ],
        );
    }
    refresh_indicators();
    while !stop_requested() && !capture_loop_failed {
        if options.webcam && !camera_available && read_state().value("camera_status").to_string_or("") == "ready" {
            camera_available = true;
            camera_backend = "ffmpeg-pipe".into();
        }
        if native_capture {
            if !screen_capture.capture_frame(&mut error) {
                capture_loop_failed = true;
                break;
            }
        } else {
            if recorder.state() == ProcessState::NotRunning {
                capture_loop_failed = true;
                break;
            }
            recorder.wait_for_finished(0);
            drain_recorder_output(&mut recorder, &mut recorder_output, debug_log.as_mut().map(|log| log as &mut dyn Write));
            msleep(40);
        }
    }
    let unexpected_exit = !stop_requested() && capture_loop_failed;
    if DISCARD_REQUESTED.load(Ordering::SeqCst) {
        cursor_sampler.stop();
        evdev_listener.stop();
        let removed = discard_timeline(&mut DiscardActions {
            abort_capture: Some(Box::new(|| {
                if native_capture {
                    screen_capture.abort();
                } else if recorder.state() != ProcessState::NotRunning {
                    recorder.terminate();
                    if !recorder.wait_for_finished(1000) {
                        recorder.kill();
                        recorder.wait_for_finished(250);
                    }
                }
            })),
            kill_audio: Some(Box::new(|| {
                if audio_recorder.state() == ProcessState::NotRunning {
                    return;
                }
                audio_recorder.kill();
                audio_recorder.wait_for_finished(250);
            })),
            request_camera_stop: Some(Box::new(|| {
                if options.webcam {
                    update_state(&[("camera_stop", true.into())]);
                }
            })),
            wait_for_camera_stop: Some(Box::new(|timeout_ms| {
                if !options.webcam {
                    return;
                }
                let camera_stop_timer = Instant::now();
                while elapsed_ms(&camera_stop_timer) < timeout_ms as u128 {
                    let status = read_state().value("camera_status").to_string_or("");
                    if status == "stopped" || status == "failed" {
                        break;
                    }
                    msleep(20);
                }
            })),
            remove_bundle: Some(Box::new(|| !qdir::file_exists(&bundle) || qdir::remove_recursively(&bundle))),
            remove_state: Some(Box::new(|| {
                qfile::remove(&state_file_path());
            })),
        });
        refresh_indicators();
        let name = FileInfo::new(bundle.clone()).complete_base_name();
        run_optional_detached("omarchy-notification-send", &["Recording discarded", name.as_str()]);
        write_stderr(&format!(
            "{}{bundle}\n",
            if removed { "Recording discarded: " } else { "Could not discard recording: " }
        ));
        return if removed { 0 } else { 2 };
    }
    let mut forced_stop = false;
    // The bar and the launcher say Saving while the capture finishes and the audio is muxed.
    let mut stopping = vec![("stopping", Value::from(true))];
    if options.webcam {
        stopping.push(("camera_stop", true.into()));
    }
    update_state(&stopping);
    if native_capture {
        if !screen_capture.finish(&mut error) {
            capture_loop_failed = true;
        }
        if audio_active {
            audio_muxed = finish_audio_capture(
                &mut audio_recorder,
                &native_video,
                &audio_video,
                &video,
                screen_capture.first_frame_us(),
                audio_started_us,
                screen_capture.video_duration_us(),
            );
        }
        // A killed ffmpeg wrote no report.
        let audio_log = format!("{audio_video}.log");
        if audio_active && options.microphone_audio && audio_recorder.exit_status() == ExitStatus::NormalExit {
            if let Ok(log) = qfile::read_all(&audio_log) {
                microphone_peak = microphone_peak_db(&qstring::from_utf8(&log));
            }
        }
        qfile::remove(&audio_log);
        if !audio_muxed {
            qfile::remove(&video);
            if native_video != video {
                qfile::rename(&native_video, &video);
            }
        }
        qfile::remove(&audio_video);
        qfile::remove(&format!("{audio_video}.ts"));
        if audio_muxed && native_video != video {
            qfile::remove(&native_video);
        }
    } else if recorder.state() != ProcessState::NotRunning {
        // SAFETY: the recorder is this process's child and still running.
        unsafe { libc::kill(recorder.process_id() as libc::pid_t, libc::SIGINT) };
        if !recorder.wait_for_finished(5000) {
            forced_stop = true;
            recorder.kill();
            recorder.wait_for_finished(1000);
        }
    }
    if options.webcam {
        let camera_stop_timer = Instant::now();
        while elapsed_ms(&camera_stop_timer) < 5000 {
            let status = read_state().value("camera_status").to_string_or("");
            if status == "stopped" || status == "failed" {
                break;
            }
            msleep(20);
        }
    }
    if FileInfo::new(camera_video.clone()).exists() && FileInfo::new(camera_video.clone()).size() == 0 {
        qfile::remove(&camera_video);
    }
    if !native_capture {
        drain_recorder_output(&mut recorder, &mut recorder_output, debug_log.as_mut().map(|log| log as &mut dyn Write));
    }
    let recorder_error_line = if native_capture { error.clone() } else { last_non_empty_line(&recorder_output) };
    let recorder_failed = if native_capture {
        capture_loop_failed
    } else {
        recorder.exit_status() != ExitStatus::NormalExit || recorder.exit_code() != 0
    };
    let stopped_us = monotonic_us();
    cursor_sampler.stop();
    evdev_listener.stop();

    let cursor = cursor_sampler.samples();
    let device_events = evdev_listener.events();
    let first_frame_us = first_frame_timestamp(&timestamp_path);

    let mut ffprobe = Process::new();
    ffprobe.start(
        "ffprobe",
        &["-v", "error", "-show_entries", "format=duration", "-of", "default=noprint_wrappers=1:nokey=1", &video],
    );
    let probe_finished = ffprobe.wait_for_finished(10000);
    let mut duration_ok = false;
    let probed_duration = if probe_finished && ffprobe.exit_code() == 0 {
        let text = qstring::from_utf8(&ffprobe.read_all_standard_output());
        let (value, ok) = qnumber::to_double_with_ok(qstring::trimmed(&text));
        duration_ok = ok;
        value
    } else {
        0.0
    };
    let effective_exit_code = if native_capture {
        if recorder_failed { -1 } else { 0 }
    } else if recorder.exit_status() == ExitStatus::NormalExit {
        recorder.exit_code()
    } else {
        -1
    };
    let video_size = FileInfo::new(video.clone()).size();
    let exit_classification = classify_gsr_exit(
        effective_exit_code,
        stop_requested(),
        video_size,
        if duration_ok { probed_duration } else { 0.0 },
    );
    if let Some(log) = debug_log.as_mut() {
        let classification = match exit_classification {
            GsrExitClassification::UserStop => "user-stop",
            GsrExitClassification::ExternalStop => "external-stop",
            GsrExitClassification::Failure => "failure",
        };
        let _ = log.write_all(
            format!(
                "omareel: capture exit classified as {classification}, exit={effective_exit_code}, bytes={video_size}, duration={}\n",
                qnumber::byte_array_number(probed_duration, 'f', 3)
            )
            .as_bytes(),
        );
        let _ = log.flush();
    }
    let mut ok = !forced_stop && exit_classification != GsrExitClassification::Failure && first_frame_us > 0;
    let mut recorded_region = region.clone();
    let recorded_fps = options.fps;
    let output_size = screen_capture.output_size();
    if native_capture && region.width > 0.0 {
        recorded_region.physical_width = output_size.width;
        recorded_region.physical_height = output_size.height;
        recorded_region.scale = f64::from(output_size.width) / region.width;
    }
    // The C++ keeps these writers' messages in `error`, which nothing reads any more.
    ok = write_input_log(&format!("{bundle}/input.jsonl"), &cursor, &device_events, &recorded_region).is_ok() && ok;
    let mut capture = Object::new();
    capture.insert("version", 1);
    capture.insert("backend", capture_backend.as_str());
    capture.insert("fps", recorded_fps);
    capture.insert("overlay_exclusion", if plugin_exclusion { "plugin" } else { "fallback" });
    capture.insert("width", if native_capture { output_size.width } else { region.physical_width });
    capture.insert("height", if native_capture { output_size.height } else { region.physical_height });
    capture.insert("mode", mode_name(region.mode));
    let mut region_object = Object::new();
    region_object.insert("x", region.x);
    region_object.insert("y", region.y);
    region_object.insert("w", region.width);
    region_object.insert("h", region.height);
    capture.insert("region", region_object);
    capture.insert("scale", recorded_region.scale);
    capture.insert("monitor", region.monitor_name.as_str());
    capture.insert("first_frame_us", first_frame_us);
    capture.insert("started_us", started_us);
    capture.insert("stopped_us", stopped_us);
    let mut audio = Object::new();
    audio.insert("desktop", options.desktop_audio && (!native_capture || audio_muxed));
    audio.insert("mic", options.microphone_audio && (!native_capture || audio_muxed));
    audio.insert("microphoneDevice", options.microphone_device.as_str());
    capture.insert("audio", audio);
    if native_capture {
        let captured_span_us = (screen_capture.last_frame_us() - screen_capture.first_frame_us()).max(1);
        let frames = screen_capture.encoded_frames();
        let measured_rate =
            if frames > 1 { f64::from(frames - 1) * 1_000_000.0 / captured_span_us as f64 } else { 0.0 };
        let mut statistics = Object::new();
        statistics.insert("frames", frames);
        statistics.insert("drops", screen_capture.dropped_frames());
        statistics.insert("measuredFps", measured_rate);
        statistics.insert("conversion", screen_capture.conversion_mode());
        capture.insert("capture", statistics);
        if audio_requested && audio_started_us > 0 {
            let mut audio = Object::new();
            audio.insert("desktop", options.desktop_audio && audio_muxed);
            audio.insert("mic", options.microphone_audio && audio_muxed);
            audio.insert("microphoneDevice", options.microphone_device.as_str());
            audio.insert("startOffsetUs", audio_started_us - screen_capture.first_frame_us());
            capture.insert("audio", audio);
        }
    }
    if options.webcam && FileInfo::new(camera_video.clone()).size() > 0 {
        let camera_timestamp = format!("{camera_video}.ts");
        let camera_first_frame_us = first_frame_timestamp(&camera_timestamp);
        let mut camera_probe = Process::new();
        camera_probe.start(
            "ffprobe",
            &[
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height,r_frame_rate",
                "-of",
                "json",
                &camera_video,
            ],
        );
        if camera_probe.wait_for_finished(10000) && camera_probe.exit_code() == 0 && camera_first_frame_us > 0 {
            let document = qjson::object_from_json(&camera_probe.read_all_standard_output()).unwrap_or_default();
            let streams = document.value("streams").to_array();
            let stream = streams.first().map(Value::to_object).unwrap_or_default();
            if !stream.is_empty() {
                let rate_text = stream.value("r_frame_rate").to_string_or("");
                let rate: Vec<&str> = rate_text.split('/').collect();
                let fps = if rate.len() == 2 && to_double(rate[1]) != 0.0 {
                    to_double(rate[0]) / to_double(rate[1])
                } else {
                    30.0
                };
                let camera_state = read_state();
                if camera_backend.is_empty() {
                    camera_backend = camera_state.value("camera_backend").to_string_or("ffmpeg-pipe");
                }
                capture.insert(
                    "camera",
                    camera_capture_block(
                        &options.webcam_device,
                        options.webcam_height,
                        stream.value("width").to_int(0),
                        stream.value("height").to_int(0),
                        fps,
                        camera_first_frame_us,
                        &camera_backend,
                        camera_state.value("camera_rotation").to_int(options.webcam_rotation),
                        camera_state.value("camera_flip_horizontal").to_bool(options.webcam_flip_horizontal),
                        &camera_state.value("camera_shape").to_string_or("round"),
                        camera_state.value("camera_radius").to_double(16.0),
                    ),
                );
            } else {
                qfile::remove(&camera_video);
                qfile::remove(&camera_timestamp);
            }
        } else {
            qfile::remove(&camera_video);
            qfile::remove(&camera_timestamp);
        }
    }
    ok = write_json(&format!("{bundle}/capture.json"), &capture).is_ok() && ok;
    let mut recording = Object::new();
    recording.insert("overlay_exclusion", if plugin_exclusion { "plugin" } else { "fallback" });
    ok = write_json(&format!("{bundle}/recording.json"), &recording).is_ok() && ok;

    let mut ffmpeg = Process::new();
    ffmpeg.start(
        "ffmpeg",
        &["-y", "-loglevel", "error", "-i", &video, "-frames:v", "1", "-q:v", "3", &format!("{bundle}/thumb.jpg")],
    );
    ffmpeg.wait_for_finished(10000);
    if !ok && (!FileInfo::new(video.clone()).is_file() || FileInfo::new(video.clone()).size() == 0) {
        qdir::remove_recursively(&bundle);
    }
    qfile::remove(&state_file_path());
    refresh_indicators();
    if ok {
        if let Some(log) = debug_log.as_mut() {
            let _ = log.write_all(format!("omareel: Recording saved: {bundle}\n").as_bytes());
            let _ = log.flush();
        }
        let audio_lost = native_capture && audio_requested && !audio_muxed;
        // A real microphone always picks up some noise; exact zeros mean it is muted or dead.
        let microphone_silent = !audio_lost && microphone_peak <= -90.0;
        let name = FileInfo::new(bundle.clone()).complete_base_name();
        let (title, body) = if audio_lost {
            ("Recording saved without audio", "No sound arrived from the desktop or microphone")
        } else if microphone_silent {
            ("Recording saved with a silent microphone", "It recorded no sound at all. Check that it isn't muted.")
        } else {
            ("Recording saved", name.as_str())
        };
        run_optional_detached(
            "omarchy-notification-send",
            &[title, body, "--exec", application.as_str(), "edit", bundle.as_str()],
        );
        if !options.no_open {
            qprocess::start_detached(&application, &["edit", bundle.as_str()], DetachedChannels::default());
        }
    } else {
        let mut detail = if forced_stop {
            "Recording process had to be force-killed. Video may be corrupted.".to_owned()
        } else {
            "The recording could not be finalized. Set OMAREEL_DEBUG=1 and check /tmp/omareel.log.".to_owned()
        };
        if (unexpected_exit || recorder_failed) && !recorder_error_line.is_empty() {
            detail += &format!("\ngpu-screen-recorder: {recorder_error_line}");
        }
        notify_stop_failure(&detail);
        write_last_error(&detail);
    }
    if let Some(log) = debug_log.as_mut() {
        let _ = log.write_all(if ok { b"omareel: daemon exit=0\n" } else { b"omareel: daemon exit=2\n" });
        let _ = log.flush();
    }
    if ok { 0 } else { 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn options_are_normalised() {
        let (options, replaced) = daemon_options(
            &arguments(&[
                "--fps",
                "30",
                "--dir",
                "/r",
                "--microphone-audio",
                "--microphone-device",
                "gone",
                "--webcam-height",
                "721",
                "--webcam-rotation",
                "45",
                "--selfview-size",
                "l",
            ]),
            || vec!["here".to_owned()],
        );
        assert!(replaced);
        assert_eq!(options.microphone_device, "default_input");
        assert_eq!((options.fps, options.output_directory.as_str()), (30, "/r"));
        assert_eq!((options.webcam_height, options.webcam_rotation), (1080, 0));
        assert_eq!(options.self_view_size, "L");
        assert_eq!((options.webcam_device.as_str(), options.capture_backend.as_str()), ("/dev/video2", "auto"));
        let (options, replaced) = daemon_options(&arguments(&["--microphone-audio"]), Vec::new);
        assert!(!replaced);
        assert_eq!((options.fps, options.self_view_size.as_str()), (0, "M"));
    }

    #[test]
    fn gsr_gets_the_region_or_the_output() {
        let region = CaptureRegion {
            monitor_name: "DP-1".into(),
            x: 10.5,
            y: -0.5,
            width: 641.4,
            height: 359.5,
            scale: 1.0,
            physical_width: 640,
            physical_height: 360,
            mode: CaptureMode::Region,
        };
        let options = RecordOptions {
            fps: 60,
            desktop_audio: true,
            microphone_audio: true,
            microphone_device: "mic".into(),
            ..RecordOptions::default()
        };
        let gsr = gsr_arguments(&region, false, &options, "/b/screen.mp4");
        assert_eq!(gsr[..4], ["-w", "region", "-region", "641x360+11+-1"]);
        assert_eq!(gsr[gsr.len() - 6..], ["-a", "default_output|mic", "-ac", "aac", "-o", "/b/screen.mp4"]);
        let whole = CaptureRegion { mode: CaptureMode::Fullscreen, ..region.clone() };
        assert_eq!(gsr_arguments(&whole, false, &RecordOptions::default(), "v")[..4], ["-w", "DP-1", "-s", "0x0"]);
        assert_eq!(gsr_arguments(&whole, true, &RecordOptions::default(), "v")[1], "region");
    }
}
