//! The recording side of omareel without Qt: what the recorder daemon, the region picker, the
//! capture exclusion plugin, the camera and screen capture, the launcher and the recording bar
//! decide, ported from the C++ at `rust-migration-base` and checked against its goldens
//! (`tests/golden/README.md`, recorder to launcher). The daemon's capture loop, the Wayland
//! session, the camera and the QObjects stay in C++ for now and call this.
//!
//! The modules follow the C++ files: [`capture`] holds the data types they share, [`recorder`]
//! is `Recorder.cpp` (its state files in [`recorder::state`]), [`region_picker`] is
//! `RegionPicker.cpp`, [`capture_exclusion`] `CaptureExclusion.cpp`, [`camera`] the pure part of
//! `CameraCapture.cpp`, [`screen_capture`] the pure part of `ScreenCapture.cpp`, and
//! [`recording_bar`] and [`launcher`] the decisions inside `RecordingBar.cpp` and `Launcher.cpp`.

pub mod camera;
pub mod capture;
pub mod capture_exclusion;
pub mod launcher;
pub mod recorder;
pub mod recording_bar;
pub mod region_picker;
pub mod screen_capture;
