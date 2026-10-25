//! The monitors as the recorder sees them and where the self-view bubble starts
//! (`Recorder.cpp`'s `monitorLayouts`, `selfViewPixels` and `selfViewPlacement`).

use crate::capture::{CaptureRegion, RecordOptions};
use omareel_core::compat::cpp;
use omareel_core::compat::geometry::RectF;
use omareel_core::compat::qjson::{self, Value};
use omareel_core::compat::qprocess::Process;
use omareel_core::compat::qround::q_round;
use omareel_core::recording_preferences::RecordingPreferences;

/// `QMarginsF`: what bars and panels reserve along each edge of a monitor.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MarginsF {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// `MonitorLayout`: a monitor's name, its logical geometry and its reserved margins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorLayout {
    pub name: String,
    pub geometry: RectF,
    pub reserved: MarginsF,
}

/// The monitors in what `hyprctl -j monitors` printed: every one not `disabled`, its size
/// divided by its scale (at least 0.01), and its `reserved` array as left, top, right, bottom
/// (missing entries 0). Anything but a JSON array gives none.
pub fn monitor_layouts_from(json: &[u8]) -> Vec<MonitorLayout> {
    let monitors = match qjson::from_json(json) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    let mut result = Vec::new();
    for value in monitors {
        let monitor = value.to_object();
        if monitor.value("disabled").to_bool(false) {
            continue;
        }
        let scale = cpp::max(0.01, monitor.value("scale").to_double(1.0));
        let reserved = monitor.value("reserved").to_array();
        let reserved_at = |index: usize| reserved.get(index).map_or(0.0, |value| value.to_double(0.0));
        result.push(MonitorLayout {
            name: monitor.value("name").to_string_or(""),
            geometry: RectF::new(
                monitor.value("x").to_double(0.0),
                monitor.value("y").to_double(0.0),
                monitor.value("width").to_double(0.0) / scale,
                monitor.value("height").to_double(0.0) / scale,
            ),
            reserved: MarginsF {
                left: reserved_at(0),
                top: reserved_at(1),
                right: reserved_at(2),
                bottom: reserved_at(3),
            },
        });
    }
    result
}

/// The file-static `monitorLayouts`: asks `hyprctl -j monitors` (three seconds), none when it
/// fails.
pub fn monitor_layouts() -> Vec<MonitorLayout> {
    let mut process = Process::new();
    process.start("hyprctl", &["-j", "monitors"]);
    if !process.wait_for_finished(3000) || process.exit_code() != 0 {
        return Vec::new();
    }
    monitor_layouts_from(&process.read_all_standard_output())
}

/// The file-static `selfViewPixels`: the bubble's size for `S`, `M` and `L` (anything else is
/// `M`).
pub fn self_view_pixels(size_name: &str) -> i32 {
    match size_name {
        "S" => 120,
        "L" => 220,
        _ => 160,
    }
}

/// A `QPoint`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// `SelfViewPlacement`: whether the self-view shows, whether it is known to be on a monitor that
/// exists, which one, its size and where it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfViewPlacement {
    pub visible: bool,
    pub safe: bool,
    pub monitor: String,
    pub size_name: String,
    pub pixels: i32,
    pub position: Point,
}

impl Default for SelfViewPlacement {
    fn default() -> SelfViewPlacement {
        SelfViewPlacement {
            visible: false,
            safe: false,
            monitor: String::new(),
            size_name: "M".into(),
            pixels: 160,
            position: Point::default(),
        }
    }
}

/// The file-static `selfViewPlacement`, with the monitors given: visible when the webcam and the
/// self-view are on; placed (and safe) on the recorded monitor when it is among `monitors`, at
/// the saved fractions of the room left inside a 16-pixel margin.
pub fn self_view_placement_on(
    options: &RecordOptions,
    capture: &CaptureRegion,
    preferences: &RecordingPreferences,
    monitors: &[MonitorLayout],
) -> SelfViewPlacement {
    let mut placement = SelfViewPlacement {
        visible: options.webcam && options.self_view,
        size_name: options.self_view_size.clone(),
        pixels: self_view_pixels(&options.self_view_size),
        ..SelfViewPlacement::default()
    };
    if !placement.visible {
        return placement;
    }
    let Some(target) = monitors.iter().find(|monitor| monitor.name == capture.monitor_name) else {
        return placement;
    };
    placement.safe = true;
    placement.monitor = target.name.clone();
    // Signed overflow is undefined in the C++; it wraps as the x86_64 build does.
    let range = |extent: f64, pixels: i32| cpp::max(0, q_round(extent).wrapping_sub(pixels).wrapping_sub(32));
    let x_range = range(target.geometry.width, placement.pixels);
    let y_range = range(target.geometry.height, placement.pixels);
    placement.position = Point {
        x: 16i32.wrapping_add(q_round(preferences.self_view_x * f64::from(x_range))),
        y: 16i32.wrapping_add(q_round(preferences.self_view_y * f64::from(y_range))),
    };
    placement
}

/// The file-static `selfViewPlacement`: [`self_view_placement_on`] with [`monitor_layouts`],
/// which is only asked when the self-view is visible.
pub fn self_view_placement(
    options: &RecordOptions,
    capture: &CaptureRegion,
    preferences: &RecordingPreferences,
) -> SelfViewPlacement {
    if !(options.webcam && options.self_view) {
        return self_view_placement_on(options, capture, preferences, &[]);
    }
    self_view_placement_on(options, capture, preferences, &monitor_layouts())
}
