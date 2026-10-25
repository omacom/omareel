//! The recording side of omareel without Qt: what the recorder daemon, the region picker, the
//! capture exclusion plugin, the camera and screen capture, the launcher and the recording bar
//! decide, ported from the C++ at `rust-migration-base` and checked against its goldens
//! (`tests/golden/README.md`, recorder to launcher), and the recording runtime itself: the
//! recorder daemon, the Wayland screen capture, the input and cursor sampling and the `omareel
//! record` command. The camera and the QObjects stay in C++ for now and call this.
//!
//! The modules follow the C++ files: [`capture`] holds the data types they share, [`recorder`]
//! is `Recorder.cpp` (its state files in [`recorder::state`], the daemon in
//! [`recorder::daemon`], starting and stopping it in [`recorder::start`]), [`cli`] the recording
//! commands of `main.cpp`, [`region_picker`] is `RegionPicker.cpp`, [`capture_exclusion`]
//! `CaptureExclusion.cpp`, [`camera`] the pure part of `CameraCapture.cpp`, [`screen_capture`]
//! `ScreenCapture.cpp`, [`evdev_listener`] `EvdevListener.cpp`, [`cursor_sampler`]
//! `CursorSampler.cpp`, and [`recording_bar`] and [`launcher`] the decisions inside
//! `RecordingBar.cpp` and `Launcher.cpp`.

pub mod camera;
pub mod capture;
pub mod capture_exclusion;
pub mod cli;
pub mod cursor_sampler;
pub mod evdev_listener;
pub mod launcher;
pub mod recorder;
pub mod recording_bar;
pub mod region_picker;
pub mod screen_capture;
