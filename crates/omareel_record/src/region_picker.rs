//! `RegionPicker` (`src/record/RegionPicker.cpp`): what `omareel record` captures. Hyprland's
//! monitors come from `hyprctl -j monitors`; a region or window is picked with
//! `omarchy-capture-region` or `slurp` (fed the visible windows' rectangles); and when
//! gpu-screen-recorder will do the capture, a monitor it cannot capture is swapped for one it
//! can.
//!
//! The C++ reports through a `QString *error` that it writes on failure and, on success, for
//! a note ("... is not capturable ...") or a failed window listing; [`Pick::message`] and the
//! `message` out-parameters below keep the last value written (`None` when nothing was), so a
//! caller can tell an untouched error from an empty one.
//!
//! The patterns are `QRegularExpression`s without `UseUnicodePropertiesOption`, so `\s` and
//! `\d` match ASCII whitespace (`\t \n \v \f \r` and space) and ASCII digits only; they are
//! matched by hand here.

use crate::capture::{CaptureMode, CaptureRegion};
use omareel_core::compat::qjson::{self, Value};
use omareel_core::compat::qnumber;
use omareel_core::compat::qprocess::{ExitStatus, Process};
use omareel_core::compat::qstandardpaths;
use omareel_core::compat::qstring;
use omareel_core::recording_preferences::RecordingPreferences;
use std::time::Instant;

/// A Hyprland monitor as `hyprctl -j monitors` describes it, with the physical size swapped
/// for a transform that turns it sideways and the logical size the physical one over the scale.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub logical_width: f64,
    pub logical_height: f64,
    pub scale: f64,
    pub physical_width: i32,
    pub physical_height: i32,
    pub focused: bool,
}

/// What [`pick`] decided: the region (`None` when picking failed or was cancelled) and the last
/// message it wrote to the C++'s `error` (the failure, or on success a note), `None` when it
/// wrote none.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pick {
    pub region: Option<CaptureRegion>,
    pub message: Option<String>,
}

/// PCRE2's `\s` without Unicode properties.
fn is_pattern_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r')
}

/// `QJsonDocument::fromJson(bytes).array()`.
fn json_array(bytes: &[u8]) -> Vec<Value> {
    match qjson::from_json(bytes) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `QJsonDocument::fromJson(bytes).object()`.
fn json_object(bytes: &[u8]) -> qjson::Object {
    match qjson::from_json(bytes) {
        Ok(Value::Object(object)) => object,
        _ => qjson::Object::new(),
    }
}

/// `std::lround(x)` as glibc computes it on x86_64 (and aarch64 for in-range values): half away
/// from zero; a value outside `long` (NaN included) gives `LONG_MIN`.
fn lround(x: f64) -> i64 {
    let rounded = x.round();
    if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&rounded) {
        rounded as i64
    } else {
        i64::MIN
    }
}

/// `int(std::lround(x))`: the `long` truncated to `int`, as the C++ conversion does.
fn lround_int(x: f64) -> i32 {
    lround(x) as i32
}

/// `RegionPicker::monitors`: `hyprctl -j monitors` (3 s), every entry (disabled ones too). On
/// failure the message is "hyprctl monitors failed: " and what hyprctl printed on its standard
/// error.
pub fn monitors() -> Result<Vec<Monitor>, String> {
    let mut process = Process::new();
    process.start("hyprctl", &["-j", "monitors"]);
    if !process.wait_for_finished(3000) || process.exit_code() != 0 {
        let stderr = qstring::from_utf8(&process.read_all_standard_error());
        return Err(format!("hyprctl monitors failed: {stderr}"));
    }
    let mut result = Vec::new();
    for value in json_array(&process.read_all_standard_output()) {
        let object = value.to_object();
        let scale = object.value("scale").to_double(1.0);
        let mut physical_width = object.value("width").to_int(0);
        let mut physical_height = object.value("height").to_int(0);
        let transform = object.value("transform").to_int(0);
        if transform % 2 != 0 {
            std::mem::swap(&mut physical_width, &mut physical_height);
        }
        result.push(Monitor {
            name: object.value("name").to_string_or(""),
            x: object.value("x").to_double(0.0),
            y: object.value("y").to_double(0.0),
            logical_width: f64::from(physical_width) / scale,
            logical_height: f64::from(physical_height) / scale,
            scale,
            physical_width,
            physical_height,
            focused: object.value("focused").to_bool(false),
        });
    }
    Ok(result)
}

/// One line against `^\s*([^|\r\n]+?)\s*\|\s*\d+x\d+\s*$`: the captured name, untrimmed.
fn capture_option_name(line: &str) -> Option<&str> {
    let bar = line.find('|')?;
    let (name, rest) = (&line[..bar], &line[bar + 1..]);
    // \s*\d+x\d+\s*$ after the bar ($ also matches before a final newline, which \s* takes).
    let rest = rest.trim_start_matches(is_pattern_space);
    let width = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let rest = rest[width..].strip_prefix('x')?;
    let height = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if width == 0 || height == 0 || !rest[height..].chars().all(is_pattern_space) {
        return None;
    }
    // ^\s*([^|\r\n]+?)\s* before it: the leading \s* takes what whitespace it can and the lazy
    // capture the shortest run that leaves only whitespace; neither CR nor LF may be in it.
    let lead = name.len() - name.trim_start_matches(is_pattern_space).len();
    let mut capture = name[lead..].trim_end_matches(is_pattern_space);
    if capture.is_empty() {
        // All whitespace: the leading \s* gives one character back to the capture.
        capture = name.char_indices().last().map(|(i, _)| &name[i..])?;
    }
    if capture.contains(['\r', '\n']) {
        return None;
    }
    Some(capture)
}

/// `RegionPicker::parseCaptureOptions`: the monitor names `gpu-screen-recorder
/// --list-capture-options` lists (lines `<name>|<width>x<height>`), trimmed, each once, in order.
pub fn parse_capture_options(output: &str) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();
    for line in output.split('\n') {
        if let Some(captured) = capture_option_name(line) {
            let name = qstring::trimmed(captured);
            if !name.is_empty() && !result.iter().any(|known| known == name) {
                result.push(name.to_owned());
            }
        }
    }
    result
}

/// `RegionPicker::isClickSelection`: a selection under 20 square pixels is a click.
pub fn is_click_selection(width: f64, height: f64) -> bool {
    width > 0.0 && height > 0.0 && width * height < 20.0
}

/// `captureOptions`: `gpu-screen-recorder --list-capture-options` parsed, within 3 s in all
/// (killed when it takes longer); empty when it fails.
pub fn capture_options() -> Vec<String> {
    let timer = Instant::now();
    let mut process = Process::new();
    process.start("gpu-screen-recorder", &["--list-capture-options"]);
    if !process.wait_for_started(3000) {
        return Vec::new();
    }
    let elapsed = i32::try_from(timer.elapsed().as_millis()).unwrap_or(i32::MAX);
    let remaining_ms = 0.max(3000_i32.saturating_sub(elapsed));
    if !process.wait_for_finished(remaining_ms) {
        process.kill();
        process.wait_for_finished(1000);
        return Vec::new();
    }
    if process.exit_status() != ExitStatus::NormalExit || process.exit_code() != 0 {
        return Vec::new();
    }
    parse_capture_options(&qstring::from_utf8(&process.read_all_standard_output()))
}

/// `qEnvironmentVariable(name)`.
fn environment_variable(name: &str) -> String {
    std::env::var_os(name).map(|value| String::from_utf8_lossy(value.as_encoded_bytes()).into_owned()).unwrap_or_default()
}

/// `capturableMonitor`: the index into `values` of the monitor to record instead of
/// `values[requested]`. Unless `OMAREEL_CAPTURE` (any case) or the settings' capture backend
/// asks for gpu-screen-recorder, the in-process capture is used when `is_supported` (the C++'s
/// `ScreenCapture::isSupported()`, asked last) says the compositor offers it, and any monitor
/// will do. Otherwise a monitor gpu-screen-recorder does not list is replaced by the first one
/// it lists that `values` has, and `note` says so.
pub fn capturable_monitor(
    values: &[Monitor],
    requested: usize,
    note: &mut Option<String>,
    is_supported: &dyn Fn() -> bool,
) -> usize {
    let backend = qstring::to_lower(&environment_variable("OMAREEL_CAPTURE"));
    if backend != "gsr" && RecordingPreferences::load().capture_backend != "gsr" && is_supported() {
        return requested;
    }
    let options = capture_options();
    if options.is_empty() || options.contains(&values[requested].name) {
        return requested;
    }
    for name in &options {
        for (index, monitor) in values.iter().enumerate() {
            if monitor.name == *name {
                *note = Some(format!(
                    "{} is not capturable by gpu-screen-recorder; recording {} instead",
                    values[requested].name, monitor.name
                ));
                return index;
            }
        }
    }
    requested
}

/// `-?\d+(?:\.\d+)?` at the start of `text`: the number's length, or `None`.
fn rect_number(text: &str, signed: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = usize::from(signed && bytes.first() == Some(&b'-'));
    let digits = bytes[end..].iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    end += digits;
    if bytes.get(end) == Some(&b'.') {
        let fraction = bytes[end + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
        if fraction > 0 {
            end += 1 + fraction;
        }
    }
    Some(end)
}

/// `parseRect`: `<x>,<y> <w>x<h>` as slurp prints it (`^\s*(-?\d+(?:\.\d+)?),(-?\d+(?:\.\d+)?)
/// \s+(\d+(?:\.\d+)?)x(\d+(?:\.\d+)?)\s*$`), each number read with `QString::toDouble`.
pub fn parse_rect(text: &str) -> Option<(f64, f64, f64, f64)> {
    let rest = text.trim_start_matches(is_pattern_space);
    let x_end = rect_number(rest, true)?;
    let (x, rest) = rest.split_at(x_end);
    let rest = rest.strip_prefix(',')?;
    let y_end = rect_number(rest, true)?;
    let (y, rest) = rest.split_at(y_end);
    let spaced = rest.trim_start_matches(is_pattern_space);
    if spaced.len() == rest.len() {
        return None;
    }
    let w_end = rect_number(spaced, false)?;
    let (w, rest) = spaced.split_at(w_end);
    let rest = rest.strip_prefix('x')?;
    let h_end = rect_number(rest, false)?;
    let (h, rest) = rest.split_at(h_end);
    if !rest.chars().all(is_pattern_space) {
        return None;
    }
    let value = |number: &str| qnumber::to_double_with_ok(number).0;
    Some((value(x), value(y), value(w), value(h)))
}

/// `monitorFor`: the first monitor holding the rectangle's centre, else the first monitor.
pub fn monitor_for(values: &[Monitor], x: f64, y: f64, w: f64, h: f64) -> Option<usize> {
    let (center_x, center_y) = (x + w / 2.0, y + h / 2.0);
    let found = values.iter().position(|monitor| {
        center_x >= monitor.x
            && center_x < monitor.x + monitor.logical_width
            && center_y >= monitor.y
            && center_y < monitor.y + monitor.logical_height
    });
    found.or(if values.is_empty() { None } else { Some(0) })
}

/// `windowRectangles`: the windows of the active workspace that are not hidden, as `slurp`
/// takes them on its standard input (`<x>,<y> <w>x<h>` lines). Empty when hyprctl fails; for
/// a failed client list `message` becomes "hyprctl clients failed".
pub fn window_rectangles(message: &mut Option<String>) -> String {
    let mut workspace = Process::new();
    workspace.start("hyprctl", &["-j", "activeworkspace"]);
    if !workspace.wait_for_finished(3000) || workspace.exit_code() != 0 {
        return String::new();
    }
    let active_id = json_object(&workspace.read_all_standard_output()).value("id").to_int(0);
    let mut clients = Process::new();
    clients.start("hyprctl", &["-j", "clients"]);
    if !clients.wait_for_finished(3000) || clients.exit_code() != 0 {
        *message = Some("hyprctl clients failed".into());
        return String::new();
    }
    let mut lines = String::new();
    for value in json_array(&clients.read_all_standard_output()) {
        let client = value.to_object();
        if client.value("workspace").to_object().value("id").to_int(0) != active_id
            || client.value("hidden").to_bool(false)
        {
            continue;
        }
        let at = client.value("at").to_array();
        let size = client.value("size").to_array();
        if at.len() == 2 && size.len() == 2 {
            lines += &format!("{},{} {}x{}\n", at[0].to_int(0), at[1].to_int(0), size[0].to_int(0), size[1].to_int(0));
        }
    }
    lines
}

/// `snapToWindow`: the first rectangle of `rectangles` (one per line) holding the point.
pub fn snap_to_window(rectangles: &str, point_x: f64, point_y: f64) -> Option<(f64, f64, f64, f64)> {
    for line in rectangles.split('\n').filter(|line| !line.is_empty()) {
        let Some((x, y, w, h)) = parse_rect(line) else { continue };
        if point_x < x || point_x >= x + w || point_y < y || point_y >= y + h {
            continue;
        }
        return Some((x, y, w, h));
    }
    None
}

/// The whole of a monitor as a region.
fn monitor_region(monitor: &Monitor, mode: CaptureMode) -> CaptureRegion {
    CaptureRegion {
        monitor_name: monitor.name.clone(),
        x: monitor.x,
        y: monitor.y,
        width: monitor.logical_width,
        height: monitor.logical_height,
        scale: monitor.scale,
        physical_width: monitor.physical_width,
        physical_height: monitor.physical_height,
        mode,
    }
}

/// `RegionPicker::pick`: the region to record in `mode`. Fullscreen takes the focused monitor
/// (the last focused one). A region comes from `omarchy-capture-region smart --match-monitor`
/// when it is installed, otherwise from `slurp -f "%x,%y %wx%h"` (`-r` first for a window),
/// fed the window rectangles; a picker that exits non-zero means "Selection cancelled". A
/// `monitor:<name>` answer is that monitor whole; a click in Region mode snaps to the window
/// under it (Window mode) or takes the monitor whole. Whole monitors go through
/// [`capturable_monitor`] with `is_supported`.
pub fn pick(mode: CaptureMode, is_supported: &dyn Fn() -> bool) -> Pick {
    let mut message = None;
    let values = match monitors() {
        Ok(values) => values,
        Err(error) => {
            message = Some(error);
            Vec::new()
        }
    };
    if values.is_empty() {
        return Pick { region: None, message };
    }
    if mode == CaptureMode::Fullscreen {
        let mut monitor = 0;
        for (index, candidate) in values.iter().enumerate() {
            if candidate.focused {
                monitor = index;
            }
        }
        let monitor = capturable_monitor(&values, monitor, &mut message, is_supported);
        return Pick { region: Some(monitor_region(&values[monitor], mode)), message };
    }

    let program;
    let mut arguments: Vec<String>;
    let mut standard_input = Vec::new();
    if mode == CaptureMode::Region && !qstandardpaths::find_executable("omarchy-capture-region").is_empty() {
        program = "omarchy-capture-region";
        arguments = vec!["smart".into(), "--match-monitor".into()];
    } else {
        program = "slurp";
        arguments = vec!["-f".into(), "%x,%y %wx%h".into()];
        standard_input = window_rectangles(&mut message).into_bytes();
        if mode == CaptureMode::Window {
            arguments.insert(0, "-r".into());
        }
    }
    let mut picker = Process::new();
    picker.start(program, &arguments);
    if !picker.wait_for_started(2000) {
        return Pick { region: None, message: Some(picker.error_string()) };
    }
    if !standard_input.is_empty() {
        picker.write(&standard_input);
    }
    picker.close_write_channel();
    if !picker.wait_for_finished(-1) || picker.exit_code() != 0 {
        return Pick { region: None, message: Some("Selection cancelled".into()) };
    }
    let selection = qstring::trimmed(&qstring::from_utf8(&picker.read_all_standard_output())).to_owned();
    if let Some(name) = selection.strip_prefix("monitor:") {
        if let Some(index) = values.iter().position(|monitor| monitor.name == name) {
            let selected = capturable_monitor(&values, index, &mut message, is_supported);
            return Pick { region: Some(monitor_region(&values[selected], CaptureMode::Fullscreen)), message };
        }
        return Pick { region: None, message: Some(format!("Picker returned unknown monitor {name}")) };
    }
    let Some((mut x, mut y, mut w, mut h)) = parse_rect(&selection) else {
        return Pick { region: None, message: Some(format!("Could not parse selection: {selection}")) };
    };
    // `values` is not empty, so there is always a monitor.
    let monitor = monitor_for(&values, x, y, w, h).unwrap_or(0);
    let mut selected_mode = mode;
    if mode == CaptureMode::Region && is_click_selection(w, h) {
        if let Some(window) = snap_to_window(&qstring::from_utf8(&standard_input), x, y) {
            (x, y, w, h) = window;
            selected_mode = CaptureMode::Window;
        } else {
            let whole = &values[monitor];
            (x, y, w, h) = (whole.x, whole.y, whole.logical_width, whole.logical_height);
            selected_mode = CaptureMode::Fullscreen;
        }
    }
    let region = if selected_mode == CaptureMode::Fullscreen {
        let selected = capturable_monitor(&values, monitor, &mut message, is_supported);
        monitor_region(&values[selected], selected_mode)
    } else {
        let monitor = &values[monitor];
        CaptureRegion {
            monitor_name: monitor.name.clone(),
            x,
            y,
            width: w,
            height: h,
            scale: monitor.scale,
            physical_width: lround_int(w * monitor.scale),
            physical_height: lround_int(h * monitor.scale),
            mode: selected_mode,
        }
    };
    Pick { region: Some(region), message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_option_lines_match_the_pattern() {
        assert_eq!(capture_option_name("DP-1|1920x1080"), Some("DP-1"));
        assert_eq!(capture_option_name("  DP-1 \t| 1920x1080 \r"), Some("DP-1"));
        assert_eq!(capture_option_name("a b|1x1"), Some("a b"));
        assert_eq!(capture_option_name("  |1x1"), Some(" "));
        assert_eq!(capture_option_name("a\rb|1x1"), None);
        assert_eq!(capture_option_name("DP-1|1920X1080"), None);
        assert_eq!(capture_option_name("DP-1|x1080"), None);
        assert_eq!(capture_option_name("DP-1|1920x1080 x"), None);
        assert_eq!(capture_option_name("region"), None);
        assert_eq!(parse_capture_options("DP-1|1x1\n DP-1 |2x2\n\u{a0}E\u{a0}|3x3\n"), ["DP-1", "E"]);
    }

    #[test]
    fn rectangles_parse_like_slurp_prints_them() {
        assert_eq!(parse_rect("10,20 300x200"), Some((10.0, 20.0, 300.0, 200.0)));
        assert_eq!(parse_rect("10,20\n300x200\n"), Some((10.0, 20.0, 300.0, 200.0)));
        assert_eq!(parse_rect("-1.5,-0 0.25x2"), Some((-1.5, -0.0, 0.25, 2.0)));
        for bad in ["10,20300x200", "5.,1 2x3", "10,20 -3x2", "+1,2 3x4", "1,2 3x4 x", ""] {
            assert_eq!(parse_rect(bad), None, "{bad:?}");
        }
        assert_eq!(lround_int(2.5), 3);
        assert_eq!(lround_int(-2.5), -3);
        assert_eq!(lround(f64::NAN), i64::MIN);
    }
}
