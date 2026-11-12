//! Exporting from the editor (`exportTo` and its progress, `defaultExportPath`) and the
//! editor's other text (`formatTime`, `cameraStatus`, the trace and preview-stats lines).

use super::{Document, Effect};
use crate::compat::cpp;
use crate::compat::qdir::{self, FileInfo};
use crate::compat::qnumber;
use crate::compat::qround::{q_fuzzy_compare, q_round};
use crate::compat::qstring;
use crate::export::plan::output_aspect;
use crate::export::presets::{self, ExportPreset};
use crate::omarchy_paths;
use crate::reframe;

/// What `exportTo`'s `QVariantMap settings` hold, read as the C++ read them:
/// `settings.value("fps", 0).toInt()`, `settings.value("height", 0).toInt()` and
/// `settings.value("quality").toString()` (`QVariant::toInt` rounds a double). `aspect` and
/// `reframe` ("follow" or "off"), when set, stand in for the project's for this export only, as
/// an export preset chosen in the dialog sets them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportDialogSettings {
    pub fps: i32,
    pub height: i32,
    pub quality: String,
    pub aspect: String,
    pub reframe: String,
}

/// The `ExportOptions` `exportTo` gives the exporter (`timing` stays false).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportRequest {
    pub bundle_path: String,
    pub output_path: String,
    pub fps: i32,
    pub width: i32,
    pub quality: String,
    pub gif_fps: i32,
    pub gif_width: i32,
    pub aspect: String,
    pub reframe: String,
}

/// The export properties (`exporting`, `exportProgress`, `exportFps`, `exportEtaSeconds`,
/// `exportedPath`, `exportError`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportState {
    pub exporting: bool,
    pub progress: f64,
    pub fps: f64,
    pub eta_seconds: i32,
    pub exported_path: String,
    pub error: String,
}

/// The progress lambda in `exportTo`: the fraction done, frames per second so far, and the
/// seconds left at that rate.
pub fn export_progress(frame: i32, total: i32, elapsed_ms: i64) -> (f64, f64, i32) {
    let progress = if total > 0 { f64::from(frame) / f64::from(total) } else { 0.0 };
    let elapsed = elapsed_ms as f64 / 1000.0;
    let fps = if elapsed > 0.0 { f64::from(frame) / elapsed } else { 0.0 };
    let eta = if fps > 0.0 { cpp::max(0, q_round(f64::from(total.wrapping_sub(frame)) / fps)) } else { 0 };
    (progress, fps, eta)
}

/// `formatTime(seconds)`: `mm:ss`, rounded, never negative (minutes past 99 keep their digits).
pub fn format_time(seconds: f64) -> String {
    let total = cpp::max(0, q_round(seconds));
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// `traceInput`'s line (printed when `OMAREEL_INPUT_TRACE` is 1). The object name and phase
/// fill `%1` and `%2` together, so a `%3` in them is then filled by x as Qt fills it.
pub fn trace_input_line(object_name: &str, phase: &str, x: f64, y: f64) -> String {
    let template = "OMAREEL_INPUT object=%1 phase=%2 local=(%3,%4)\n";
    // QString::arg(a1, a2) replaces %1 and %2 in one pass; the replacements are not rescanned.
    let mut line = String::new();
    let mut rest = template;
    while let Some(at) = rest.find('%') {
        line.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(after) = tail.strip_prefix("%1") {
            line.push_str(object_name);
            rest = after;
        } else if let Some(after) = tail.strip_prefix("%2") {
            line.push_str(phase);
            rest = after;
        } else {
            line.push('%');
            rest = &tail[1..];
        }
    }
    line.push_str(rest);
    let line = qstring::arg(&line, &qnumber::number_f64(x, 'f', 1));
    qstring::arg(&line, &qnumber::number_f64(y, 'f', 1))
}

/// `handlePreviewStats`' line (with `OMAREEL_PREVIEW_STATS=1`), or `None` when no time passed.
pub fn preview_stats_line(frames: i32, cost_ns: i64, elapsed_ms: i64, recording_fps: f64) -> Option<String> {
    if elapsed_ms <= 0 {
        return None;
    }
    let fps = f64::from(frames) * 1000.0 / elapsed_ms as f64;
    let average_ms = if frames > 0 { cost_ns as f64 / 1_000_000.0 / f64::from(frames) } else { 0.0 };
    let expected = q_round(recording_fps * elapsed_ms as f64 / 1000.0);
    let drops = cpp::max(0, expected.wrapping_sub(frames));
    Some(format!(
        "preview fps={} average_frame_cost_ms={} drops={}\n",
        qnumber::number_f64(fps, 'f', 1),
        qnumber::number_f64(average_ms, 'f', 3),
        drops
    ))
}

/// The folder `openContainingFolder(path)` hands `xdg-open`: the path itself when it is a
/// directory, else the directory it is in; `None` for an empty path.
pub fn containing_folder(path: &str) -> Option<String> {
    let file = FileInfo::new(path);
    let directory = if file.is_dir() { file.absolute_file_path() } else { file.absolute_path() };
    (!directory.is_empty()).then_some(directory)
}

/// The export presets, for the dialog's Preset row.
pub fn export_presets() -> &'static [ExportPreset] {
    &presets::PRESETS
}

/// `path` as a `text/uri-list` of one `file://` URI (RFC 8089; bytes outside the unreserved set
/// and `/` percent-encoded), which file managers and chat apps paste as the file.
pub fn file_uri_list(path: &str) -> String {
    let mut uri = String::from("file://");
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri.push_str("\r\n");
    uri
}

/// Puts the file at `path` on the Wayland clipboard as `text/uri-list` with `wl-copy`, which
/// keeps offering it after the editor quits. `Err` says why not (no `wl-copy`, no file).
pub fn copy_file_to_clipboard(path: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let absolute = FileInfo::new(path).absolute_file_path();
    if !FileInfo::new(&absolute).is_file() {
        return Err(format!("{absolute} is not a file"));
    }
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/uri-list"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("wl-copy: {error}"))?;
    let written = child.stdin.take().map(|mut stdin| stdin.write_all(file_uri_list(&absolute).as_bytes()));
    // wl-copy forks to serve the clipboard and its first process exits once it has the data.
    let status = child.wait().map_err(|error| format!("wl-copy: {error}"))?;
    match written {
        Some(Ok(())) if status.success() => Ok(()),
        Some(Err(error)) => Err(format!("wl-copy: {error}")),
        _ => Err(format!("wl-copy failed ({status})")),
    }
}

impl Document {
    /// `defaultExportPath(format)`: the project's name in the recordings directory, `.gif` for
    /// "gif" and `.mp4` otherwise.
    pub fn default_export_path(&self, format: &str) -> String {
        let suffix = if format == "gif" { ".gif" } else { ".mp4" };
        qdir::file_path(&omarchy_paths::recordings_directory(), &format!("{}{suffix}", self.project.name))
    }

    /// The export dialog's path for an export preset: its file name for this project in the
    /// recordings directory; empty for an unknown id.
    pub fn preset_export_path(&self, id: &str) -> String {
        presets::find(id).map_or_else(String::new, |preset| {
            qdir::file_path(&omarchy_paths::recordings_directory(), &preset.file_name(&self.project.name))
        })
    }

    /// Saves an export preset's settings (aspect, reframing, and the format, size, rate and
    /// quality of its format) into the project, as one undo step. False for an unknown id or
    /// when the project already has them.
    pub fn apply_export_preset(&mut self, id: &str) -> bool {
        let Some(preset) = presets::find(id) else { return false };
        self.apply_project_change(|project| preset.apply(project))
    }

    /// Turns auto-reframe on ("follow") or off ("off" or ""), as one undo step. False for an
    /// unknown mode or no change.
    pub fn set_reframe(&mut self, mode: &str) -> bool {
        let mode = if mode == "off" { "" } else { mode };
        if !mode.is_empty() && !reframe::MODES.contains(&mode) {
            return false;
        }
        self.apply_project_change(|project| project.reframe = mode.into())
    }

    /// One undoable change made to a copy of the project, kept when the saved JSON differs.
    fn apply_project_change(&mut self, change: impl FnOnce(&mut crate::project::Project)) -> bool {
        let old = self.project.to_json();
        let mut next = self.project.clone();
        change(&mut next);
        let parsed = crate::project::Project::from_json(&next.to_json());
        if parsed.to_json() == old {
            return false;
        }
        self.snapshot("");
        self.project = parsed;
        self.changed(true);
        true
    }

    /// `cameraStatus`: the webcam's size, rate and offset, as the camera panel shows them.
    pub fn camera_status(&self) -> String {
        let Some(camera) = self.recording.camera else { return "No webcam in this bundle".into() };
        let fps_precision = if q_fuzzy_compare(camera.fps, f64::from(q_round(camera.fps))) { 0 } else { 2 };
        let mut text = qstring::arg("Webcam: %1×%2 @%3, offset %4%5 s", &camera.width.to_string());
        text = qstring::arg(&text, &camera.height.to_string());
        text = qstring::arg(&text, &qnumber::number_f64(camera.fps, 'f', fps_precision));
        text = qstring::arg(&text, if camera.offset >= 0.0 { "+" } else { "" });
        qstring::arg(&text, &qnumber::number_f64(camera.offset, 'f', 2))
    }

    pub fn export_state(&self) -> &ExportState {
        &self.export
    }

    /// `exportTo(path, settings)` up to creating the exporter, with `path` already turned from a
    /// URL into a path by the Qt side: saves the project, makes the output directory, and starts
    /// the export state. `None` when an export is running, the path is empty, or the directory
    /// can't be made (then `exportError` says so). A GIF takes the fps and width as its own.
    pub fn begin_export(&mut self, path: &str, settings: &ExportDialogSettings) -> Option<ExportRequest> {
        if self.export.exporting {
            return None;
        }
        self.save_now();
        self.begin_saved_export(path, settings)
    }

    /// The part of [`Document::begin_export`] after its save, for a Qt side that performs the
    /// save's effects first, where the C++ emitted them.
    pub fn begin_saved_export(&mut self, path: &str, settings: &ExportDialogSettings) -> Option<ExportRequest> {
        if path.is_empty() {
            return None;
        }
        if !qdir::mkpath(&FileInfo::new(path).absolute_path()) {
            self.export.error = "Could not create the output directory".into();
            self.emit(Effect::ExportErrorChanged);
            return None;
        }
        let mut request = ExportRequest {
            bundle_path: self.recording.bundle_path.clone(),
            output_path: FileInfo::new(path).absolute_file_path(),
            fps: settings.fps,
            quality: settings.quality.clone(),
            aspect: settings.aspect.clone(),
            reframe: settings.reframe.clone(),
            ..ExportRequest::default()
        };
        if settings.height > 0 {
            let aspect = if settings.aspect.is_empty() { &self.project.aspect } else { &settings.aspect };
            request.width = q_round(
                f64::from(settings.height)
                    * output_aspect(aspect, self.recording.source_width, self.recording.source_height, &self.project.crop),
            );
        }
        if path.len() >= 4 && path.is_char_boundary(path.len() - 4) && path[path.len() - 4..].eq_ignore_ascii_case(".gif") {
            request.gif_fps = request.fps;
            request.gif_width = request.width;
            request.fps = 0;
            request.width = 0;
        }
        self.export = ExportState { exporting: true, ..ExportState::default() };
        self.emit(Effect::ExportingChanged);
        self.emit(Effect::ExportProgressChanged);
        self.emit(Effect::ExportErrorChanged);
        Some(request)
    }

    /// `Exporter::progress(frame, total)`, with the milliseconds since [`Document::begin_export`].
    pub fn export_progressed(&mut self, frame: i32, total: i32, elapsed_ms: i64) {
        let (progress, fps, eta) = export_progress(frame, total, elapsed_ms);
        self.export.progress = progress;
        self.export.fps = fps;
        self.export.eta_seconds = eta;
        self.emit(Effect::ExportProgressChanged);
    }

    /// `Exporter::finished(path)`.
    pub fn export_finished(&mut self, path: &str) {
        self.export.exporting = false;
        self.export.progress = 1.0;
        self.export.eta_seconds = 0;
        self.export.exported_path = path.to_owned();
        self.emit(Effect::ExportingChanged);
        self.emit(Effect::ExportProgressChanged);
        self.emit(Effect::ExportFinished(path.to_owned()));
    }

    /// `Exporter::failed(error)`.
    pub fn export_failed(&mut self, error: &str) {
        self.export.exporting = false;
        self.export.error = error.to_owned();
        self.export.eta_seconds = 0;
        self.emit(Effect::ExportingChanged);
        self.emit(Effect::ExportErrorChanged);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_and_progress() {
        assert_eq!(format_time(0.0), "00:00");
        assert_eq!(format_time(59.49), "00:59");
        assert_eq!(format_time(59.5), "01:00");
        assert_eq!(format_time(-3.0), "00:00");
        assert_eq!(format_time(6000.0), "100:00");
        assert_eq!(format_time(f64::NAN), "00:00");
        assert_eq!(export_progress(0, 0, 0), (0.0, 0.0, 0));
        assert_eq!(export_progress(30, 120, 2000), (0.25, 15.0, 6));
        assert_eq!(export_progress(130, 120, 1000), (130.0 / 120.0, 130.0, 0));
    }

    #[test]
    fn file_uris() {
        assert_eq!(file_uri_list("/home/me/Videos/Demo.mp4"), "file:///home/me/Videos/Demo.mp4\r\n");
        assert_eq!(file_uri_list("/tmp/a b/ü#?.gif"), "file:///tmp/a%20b/%C3%BC%23%3F.gif\r\n");
        assert!(copy_file_to_clipboard("/nonexistent/omareel.mp4").is_err());
    }

    // Uses the session's real clipboard, so it runs only when asked (in a nested compositor):
    // OMAREEL_TEST_WL_COPY=1 cargo test -p omareel_core copies_the_file_with_wl_copy
    #[test]
    fn copies_the_file_with_wl_copy() {
        if std::env::var_os("OMAREEL_TEST_WL_COPY").is_none() {
            return;
        }
        let dir = crate::compat::qdir::tests::TempDir::new("editor-copy");
        let path = dir.join("a clip.mp4");
        std::fs::write(&path, b"x").unwrap();
        copy_file_to_clipboard(&path).unwrap();
        let types = std::process::Command::new("wl-paste").arg("--list-types").output().unwrap();
        // wl-copy offers the plain-text types too, for text fields.
        assert_eq!(String::from_utf8_lossy(&types.stdout).lines().next(), Some("text/uri-list"));
        let pasted = std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text/uri-list"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&pasted.stdout), file_uri_list(&path));
        assert!(file_uri_list(&path).contains("/a%20clip.mp4\r\n"));
    }

    #[test]
    fn containing_folders() {
        let dir = crate::compat::qdir::tests::TempDir::new("editor-folder");
        assert_eq!(containing_folder(&dir.0), Some(dir.0.clone()));
        assert_eq!(containing_folder(&dir.join("out.mp4")), Some(dir.0.clone()));
        assert_eq!(containing_folder(""), None);
    }

    #[test]
    fn trace_and_stats_lines() {
        assert_eq!(trace_input_line("clip", "press", 1.25, -0.04), "OMAREEL_INPUT object=clip phase=press local=(1.3,-0.0)\n");
        // Placeholders in the names are filled by the later args, lowest number first, as
        // QString::arg fills them: x takes the phase's %1, then y every %3.
        assert_eq!(trace_input_line("%3", "%1", 1.0, 2.0), "OMAREEL_INPUT object=2.0 phase=1.0 local=(2.0,%4)\n");
        assert_eq!(preview_stats_line(0, 0, 0, 60.0), None);
        assert_eq!(
            preview_stats_line(58, 116_000_000, 1000, 60.0).unwrap(),
            "preview fps=58.0 average_frame_cost_ms=2.000 drops=2\n"
        );
    }
}
