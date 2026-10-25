//! The recording side for C++ (`omareel_record`): the recording commands `main.cpp` dispatches
//! (`omareel record`, the recorder daemon, `__selfview-ipc` and `__record-bar-ipc`), and the
//! recorder's state files, the cursor query and the encoder probe cache for the C++ that still
//! reads them (the launcher, the recording bar and its windows, the camera).
//!
//! The state files' objects cross as JSON text: `QJsonDocument` parses what Rust wrote as it
//! would have parsed the file, and Rust parses what `QJsonDocument` wrote as Qt would have kept
//! it, so the values stay what they were.

use cxx_qt_lib::{QByteArray, QList, QString, QStringList};
use omareel_core::compat::qjson::{self, Format, Value};
use omareel_record::recorder::{daemon, state};
use omareel_record::{cli, cursor_sampler, screen_capture};

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        // QtShims.h first: it brings in QString whole before cxx-qt-lib's qstring.h (see
        // export.rs).
        include!("QtShims.h");
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qbytearray.h");
        type QByteArray = cxx_qt_lib::QByteArray;

        /// `qWarning().noquote() << message`.
        #[namespace = "Omareel::Shims"]
        fn warning(message: &QString);
    }

    #[namespace = "omareel::record"]
    extern "Rust" {
        /// `omareel record <arguments>`: prints what it did and returns the exit status.
        #[cxx_name = "recordCommand"]
        fn record_command(arguments: &QStringList) -> i32;
        /// `omareel __record-daemon <arguments>`: the recorder daemon; its exit status.
        #[cxx_name = "daemonMain"]
        fn daemon_main(arguments: &QStringList) -> i32;
        /// `omareel __selfview-ipc <arguments>`.
        #[cxx_name = "selfViewIpc"]
        fn self_view_ipc(arguments: &QStringList) -> i32;
        /// `omareel __record-bar-ipc <arguments>`.
        #[cxx_name = "recordBarIpc"]
        fn record_bar_ipc(arguments: &QStringList) -> i32;

        /// `Recorder::stateFilePath()`.
        #[cxx_name = "stateFilePath"]
        fn state_file_path() -> QString;
        /// `Recorder::selfViewHostPath()`.
        #[cxx_name = "selfViewHostPath"]
        fn self_view_host_path() -> QString;
        /// `Recorder::readStateFile(path)`, as compact JSON.
        #[cxx_name = "readStateFile"]
        fn read_state_file(path: &QString) -> QByteArray;
        /// `Recorder::updateStateFile(path, values, &error)` with `values` as JSON: `error` is
        /// cleared on success and says why otherwise.
        #[cxx_name = "updateStateFile"]
        fn update_state_file(path: &QString, values: &QByteArray, error: &mut QString) -> bool;
        /// `Recorder::sendHostCommand(command)`.
        #[cxx_name = "sendHostCommand"]
        fn send_host_command(command: &QString) -> bool;
        /// `Recorder::isRecording()`.
        #[cxx_name = "isRecording"]
        fn is_recording() -> bool;
        /// `Recorder::recordingStartedUs()`.
        #[cxx_name = "recordingStartedUs"]
        fn recording_started_us() -> i64;
        /// `Recorder::recordedMonitor()`.
        #[cxx_name = "recordedMonitor"]
        fn recorded_monitor() -> QString;
        /// `Recorder::recordingHasWebcam()`.
        #[cxx_name = "recordingHasWebcam"]
        fn recording_has_webcam() -> bool;
        /// `Recorder::signalExisting(cancel, &error)`: `error` is only set on failure.
        #[cxx_name = "signalExisting"]
        fn signal_existing(cancel: bool, error: &mut QString) -> bool;
        /// `CursorSampler::cursorPosition(&point)`: `x` and `y` are only set when Hyprland
        /// answered.
        #[cxx_name = "cursorPosition"]
        fn cursor_position(x: &mut f64, y: &mut f64) -> bool;
        /// `cachedPreferredEncoderAvailable()`: whether the encoder probe cache says NVENC works.
        #[cxx_name = "cachedPreferredEncoderAvailable"]
        fn cached_preferred_encoder_available() -> bool;
    }
}

/// A QString as Rust text, through `toUtf8` (which never fails, unlike a UTF-16 conversion of
/// a string holding a lone surrogate).
fn text(string: &QString) -> String {
    String::from_utf8_lossy(string.to_utf8().as_slice()).into_owned()
}

/// The arguments as Rust strings.
fn strings(arguments: &QStringList) -> Vec<String> {
    QList::<QString>::from(arguments).iter().map(text).collect()
}

fn record_command(arguments: &QStringList) -> i32 {
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    cli::record_command(&strings(arguments), &mut out, &mut err)
}

fn daemon_main(arguments: &QStringList) -> i32 {
    daemon::daemon_main(&strings(arguments), &|message| ffi::warning(&QString::from(message)))
}

fn self_view_ipc(arguments: &QStringList) -> i32 {
    cli::self_view_ipc(&strings(arguments))
}

fn record_bar_ipc(arguments: &QStringList) -> i32 {
    cli::record_bar_ipc(&strings(arguments), &mut std::io::stderr().lock())
}

fn state_file_path() -> QString {
    QString::from(&state::state_file_path())
}

fn self_view_host_path() -> QString {
    QString::from(&state::self_view_host_path())
}

fn read_state_file(path: &QString) -> QByteArray {
    let object = state::read_state_file(&text(path));
    QByteArray::from(Value::Object(object).to_json(Format::Compact).as_str())
}

fn update_state_file(path: &QString, values: &QByteArray, error: &mut QString) -> bool {
    let values = qjson::object_from_json(values.as_slice()).unwrap_or_default();
    match state::update_state_file(&text(path), &values) {
        Ok(()) => {
            *error = QString::default();
            true
        }
        Err(message) => {
            *error = QString::from(&message);
            false
        }
    }
}

fn send_host_command(command: &QString) -> bool {
    state::send_host_command(&text(command))
}

fn is_recording() -> bool {
    state::is_recording()
}

fn recording_started_us() -> i64 {
    state::recording_started_us()
}

fn recorded_monitor() -> QString {
    QString::from(&state::recorded_monitor())
}

fn recording_has_webcam() -> bool {
    state::recording_has_webcam()
}

fn signal_existing(cancel: bool, error: &mut QString) -> bool {
    match state::signal_existing(cancel) {
        Ok(()) => true,
        Err(message) => {
            *error = QString::from(&message);
            false
        }
    }
}

fn cursor_position(x: &mut f64, y: &mut f64) -> bool {
    match cursor_sampler::cursor_position() {
        Some(point) => {
            *x = point.x;
            *y = point.y;
            true
        }
        None => false,
    }
}

fn cached_preferred_encoder_available() -> bool {
    screen_capture::cached_preferred_encoder_available()
}
