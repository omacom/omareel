//! Exporting from the editor (`exportTo` and its progress, `defaultExportPath`) and the
//! editor's other text (`formatTime`, `cameraStatus`, the trace and preview-stats lines).

use super::bundle::aspect_ratio;
use super::{Document, Effect};
use crate::compat::cpp;
use crate::compat::qdir::{self, FileInfo};
use crate::compat::qnumber;
use crate::compat::qround::{q_fuzzy_compare, q_round};
use crate::compat::qstring;
use crate::omarchy_paths;

/// What `exportTo`'s `QVariantMap settings` hold, read as the C++ read them:
/// `settings.value("fps", 0).toInt()`, `settings.value("height", 0).toInt()` and
/// `settings.value("quality").toString()` (`QVariant::toInt` rounds a double).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportDialogSettings {
    pub fps: i32,
    pub height: i32,
    pub quality: String,
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

impl Document {
    /// `defaultExportPath(format)`: the project's name in the recordings directory, `.gif` for
    /// "gif" and `.mp4` otherwise.
    pub fn default_export_path(&self, format: &str) -> String {
        let suffix = if format == "gif" { ".gif" } else { ".mp4" };
        qdir::file_path(&omarchy_paths::recordings_directory(), &format!("{}{suffix}", self.project.name))
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
            ..ExportRequest::default()
        };
        if settings.height > 0 {
            request.width = q_round(
                f64::from(settings.height)
                    * aspect_ratio(&self.project, self.recording.source_width, self.recording.source_height),
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
