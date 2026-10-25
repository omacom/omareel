//! The data the recording side's pieces hand each other: what is captured (`RegionPicker.h`), the
//! recorder's options (`Recorder.h`), and the cursor samples and input device events the recorder
//! writes into `input.jsonl` (`CursorSampler.h`, `EvdevListener.h`).

use omareel_core::compat::geometry::PointF;

/// `CaptureMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureMode {
    #[default]
    Fullscreen,
    Region,
    Window,
}

/// `CaptureRegion`: the logical rectangle on a monitor, its scale and the size in physical
/// pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRegion {
    pub monitor_name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub physical_width: i32,
    pub physical_height: i32,
    pub mode: CaptureMode,
}

impl Default for CaptureRegion {
    fn default() -> CaptureRegion {
        CaptureRegion {
            monitor_name: String::new(),
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            scale: 1.0,
            physical_width: 0,
            physical_height: 0,
            mode: CaptureMode::Fullscreen,
        }
    }
}

/// `RecordOptions`: what `omareel record` asked for, as the daemon gets it.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordOptions {
    pub mode: CaptureMode,
    pub fps: i32,
    pub output_directory: String,
    pub desktop_audio: bool,
    pub microphone_audio: bool,
    pub microphone_device: String,
    pub webcam: bool,
    pub webcam_device: String,
    pub webcam_height: i32,
    pub webcam_rotation: i32,
    pub webcam_flip_horizontal: bool,
    pub self_view: bool,
    pub self_view_size: String,
    pub capture_backend: String,
    pub countdown: bool,
    pub no_open: bool,
    pub no_bar: bool,
}

impl Default for RecordOptions {
    fn default() -> RecordOptions {
        RecordOptions {
            mode: CaptureMode::Region,
            fps: 60,
            output_directory: String::new(),
            desktop_audio: false,
            microphone_audio: false,
            microphone_device: "default_input".into(),
            webcam: false,
            webcam_device: "/dev/video2".into(),
            webcam_height: 1080,
            webcam_rotation: 0,
            webcam_flip_horizontal: false,
            self_view: true,
            self_view_size: "M".into(),
            capture_backend: "auto".into(),
            countdown: false,
            no_open: false,
            no_bar: false,
        }
    }
}

/// `RawCursorSample`: where the pointer was, in logical coordinates, at a monotonic time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RawCursorSample {
    pub monotonic_us: i64,
    pub logical_position: PointF,
}

/// `DeviceEventKind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceEventKind {
    ButtonDown,
    ButtonUp,
    Scroll,
    #[default]
    KeyDown,
    KeyUp,
}

/// `DeviceEvent`: a mouse button, scroll or key event from evdev.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceEvent {
    pub monotonic_us: i64,
    pub kind: DeviceEventKind,
    pub button: String,
    pub dx: i32,
    pub dy: i32,
    pub key_code: i32,
    pub key_name: String,
    pub modifiers: Vec<String>,
}
