//! Opening a bundle (`Editor::loadBundle` and the helpers at the top of Editor.cpp), and
//! bringing an inserted video into it (`importInsert`, `cloneOrCopy`).

use crate::compat::cpp;
use crate::compat::qdir::{self, FileInfo};
use crate::compat::qfile;
use crate::compat::qjson::{self, Object};
use crate::compat::qnumber;
use crate::compat::qprocess;
use crate::compat::qstring;
use crate::input_log::InputLog;
use crate::project::{sanitized_media_path, Clip, Project};
use crate::zoom_timeline;
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

/// What an inserted video holds, as `probeVideo` (src/render/MediaProbe.cpp) reports it. The
/// same fields as MediaProbe's `VideoInfo`, which is being ported separately; once it lands this
/// is that type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    /// Display size: rotation and sample aspect ratio applied.
    pub width: i32,
    pub height: i32,
    /// Of the video stream; of the container when the stream has none.
    pub duration: f64,
    pub fps: f64,
    pub audio: bool,
}

impl Default for VideoInfo {
    fn default() -> VideoInfo {
        VideoInfo { width: 0, height: 0, duration: 0.0, fps: 30.0, audio: false }
    }
}

impl VideoInfo {
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.duration > 0.0
    }
}

/// `probeVideo(path, &error)`: the video's facts, or the error it set (its result is then a
/// default `VideoInfo`).
pub type ProbeVideo = Arc<dyn Fn(&str) -> Result<VideoInfo, String> + Send + Sync>;

/// Editor.cpp's own `MediaInfo`, what its `probe` reads of `screen.mp4` and `camera.mp4`. It
/// differs from MediaProbe's: the frame rate defaults to 60 and comes from `r_frame_rate`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaInfo {
    pub width: i32,
    pub height: i32,
    pub duration: f64,
    pub fps: f64,
    pub audio: bool,
}

impl Default for MediaInfo {
    fn default() -> MediaInfo {
        MediaInfo { width: 0, height: 0, duration: 0.0, fps: 60.0, audio: false }
    }
}

/// Editor.cpp's `probe(path, &error)` for the recording and the camera.
pub type ProbeMedia = Arc<dyn Fn(&str) -> Result<MediaInfo, String> + Send + Sync>;

/// How the document learns about videos: Editor.cpp's `probe` for the recording and the camera,
/// and MediaProbe's `probeVideo` for inserted videos.
#[derive(Clone)]
pub struct Probes {
    pub media: ProbeMedia,
    pub video: ProbeVideo,
}

impl Probes {
    /// Editor.cpp's ffprobe [`probe`] for the recording, and `video` for inserted videos.
    pub fn new(video: ProbeVideo) -> Probes {
        Probes { media: Arc::new(probe), video }
    }
}

/// Editor.cpp's `fraction`: `"a/b"` as a / b when b reads as non-zero, otherwise the whole text
/// as a number (0 when it isn't one).
pub fn fraction(text: &str) -> f64 {
    let to_double = |s: &str| qnumber::to_double_with_ok(s).0;
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() == 2 && to_double(parts[1]) != 0.0 {
        to_double(parts[0]) / to_double(parts[1])
    } else {
        to_double(text)
    }
}

/// The `ffprobe` arguments Editor.cpp's `probe` runs with (before the path).
pub const PROBE_ARGS: [&str; 6] =
    ["-v", "error", "-show_entries", "stream=codec_type,width,height,r_frame_rate:format=duration", "-of", "json"];

/// Editor.cpp's `probe(path, &error)`: runs ffprobe for up to 15 s. A failure gives ffprobe's
/// trimmed stderr, which is empty when it could not start or timed out.
pub fn probe(path: &str) -> Result<MediaInfo, String> {
    let mut args: Vec<&str> = PROBE_ARGS.to_vec();
    args.push(path);
    match qprocess::run("ffprobe", &args, 15000) {
        Some(finished) if finished.exit_code == 0 => parse_probe(&finished.stdout),
        Some(finished) => Err(trimmed_utf8(&finished.stderr)),
        None => Err(String::new()),
    }
}

/// `QString::fromUtf8(bytes).trimmed()` (`QChar::isSpace` is Unicode's White_Space).
fn trimmed_utf8(bytes: &[u8]) -> String {
    qdir::decode_utf8_lossy(bytes).trim_matches(char::is_whitespace).to_owned()
}

/// What `probe` makes of ffprobe's JSON: the first video stream with a width gives the size and
/// `r_frame_rate`, any audio stream sets `audio`, and the format gives the duration. An
/// unreadable document reads as empty.
pub fn parse_probe(stdout: &[u8]) -> Result<MediaInfo, String> {
    let root = qjson::object_from_json(stdout).unwrap_or_default();
    let mut info = MediaInfo {
        duration: qnumber::to_double_with_ok(&root.value("format").to_object().value("duration").to_string_or("")).0,
        ..MediaInfo::default()
    };
    for entry in root.value("streams").to_array() {
        let stream = entry.to_object();
        let codec = stream.value("codec_type");
        if codec.as_str() == Some("video") && info.width == 0 {
            info.width = stream.value("width").to_int(0);
            info.height = stream.value("height").to_int(0);
            info.fps = fraction(&stream.value("r_frame_rate").to_string_or(""));
        } else if codec.as_str() == Some("audio") {
            info.audio = true;
        }
    }
    // A NaN duration passes, as in the C++.
    if info.width <= 0 || info.height <= 0 || info.duration <= 0.0 {
        return Err("screen.mp4 has invalid metadata".into());
    }
    Ok(info)
}

/// Editor.cpp's `aspectRatio`: the project's aspect, or the cropped recording's for "auto".
pub fn aspect_ratio(project: &Project, width: i32, height: i32) -> f64 {
    match project.aspect.as_str() {
        "16:9" => 16.0 / 9.0,
        "1:1" => 1.0,
        "4:3" => 4.0 / 3.0,
        "9:16" => 9.0 / 16.0,
        "3:4" => 3.0 / 4.0,
        "4:5" => 4.0 / 5.0,
        _ => f64::from(width) * project.crop.width / cpp::max(1.0, f64::from(height) * project.crop.height),
    }
}

/// The webcam recording beside the screen, as `loadBundle` found it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraRecording {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub duration: f64,
    /// Seconds the camera's first frame came after the screen's (`capture.json`).
    pub offset: f64,
}

/// What `loadBundle` learned about the recording (the C++ members it set besides the project).
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    /// `QDir(bundlePath).absolutePath()`.
    pub bundle_path: String,
    pub source_duration: f64,
    pub source_width: i32,
    pub source_height: i32,
    pub capture_scale: f64,
    pub fps: f64,
    pub has_audio: bool,
    pub has_desktop_audio: bool,
    pub has_microphone_audio: bool,
    pub camera: Option<CameraRecording>,
}

impl Default for Recording {
    fn default() -> Recording {
        Recording {
            bundle_path: String::new(),
            source_duration: 0.0,
            source_width: 0,
            source_height: 0,
            capture_scale: 1.0,
            fps: 60.0,
            has_audio: false,
            has_desktop_audio: false,
            has_microphone_audio: false,
            camera: None,
        }
    }
}

/// Everything `loadBundle` read.
#[derive(Clone, Debug)]
pub struct Bundle {
    pub recording: Recording,
    pub project: Project,
    pub input: InputLog,
    /// What `probeVideo` said of each inserted video the project uses (`m_inserts`).
    pub inserts: HashMap<String, VideoInfo>,
    /// There was no `project.json`: the project is new, its zooms were generated, and it is
    /// dirty with the autosave timer running.
    pub new_project: bool,
}

/// `QFile::open(ReadOnly)` then `readAll()`, ignoring a read error after the open as Qt does.
fn read_if_openable(path: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = qfile::open_read(path).ok()?;
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    Some(bytes)
}

/// The name a new project takes: the bundle's directory name without `.omareel` or the older
/// `.omarecord`.
pub fn bundle_project_name(bundle_path: &str) -> String {
    let name = FileInfo::new(bundle_path).file_name();
    if let Some(stripped) = name.strip_suffix(".omareel") {
        stripped.to_owned()
    } else if let Some(stripped) = name.strip_suffix(".omarecord") {
        stripped.to_owned()
    } else {
        name
    }
}

/// The project a bundle without `project.json` starts with: [`Project::defaults`], the camera
/// on when there is one, and its rotation, flip, shape and corner radius as `capture.json`'s
/// `camera` recorded them.
pub fn new_project(name: &str, source_duration: f64, has_camera: bool, camera_capture: &Object) -> Project {
    let mut project = Project::defaults(name, source_duration);
    project.camera.enabled = has_camera;
    let rotation = camera_capture.value("rotation").to_int(0);
    if [0, 90, 180, 270].contains(&rotation) {
        project.camera.rotation = rotation;
    }
    project.camera.flip_horizontal = camera_capture.value("flipHorizontal").to_bool(false);
    let shape = camera_capture.value("shape").to_string_or("round");
    if ["round", "square", "rectangle"].contains(&shape.as_str()) {
        project.camera.shape = shape;
    }
    project.camera.radius = camera_capture.value("radius").to_double(16.0);
    project
}

/// The camera's start relative to the screen's, from `capture.json`'s `first_frame_us` and its
/// `camera.first_frame_us` (`toVariant().toLongLong()`); 0 unless both are positive.
pub fn camera_offset(capture: &Object) -> f64 {
    let screen_first = capture.value("first_frame_us").to_variant_long_long();
    let camera_first = capture.value("camera").to_object().value("first_frame_us").to_variant_long_long();
    if screen_first > 0 && camera_first > 0 {
        camera_first.wrapping_sub(screen_first) as f64 / 1_000_000.0
    } else {
        0.0
    }
}

/// `Editor::loadBundle`, with the bundle path made absolute as the constructor made it. The
/// error is `m_error` as the C++ left it (empty when ffprobe could not run).
pub fn load_bundle(bundle_path: &str, probes: &Probes) -> Result<Bundle, String> {
    let bundle_path = qdir::absolute_path(bundle_path);
    let project_path = qdir::file_path(&bundle_path, "project.json");
    let video_path = qdir::file_path(&bundle_path, "screen.mp4");
    let camera_video_path = qdir::file_path(&bundle_path, "camera.mp4");
    if !FileInfo::new(bundle_path.as_str()).is_dir() {
        return Err(qstring::arg("Bundle does not exist: %1", &bundle_path));
    }
    let info = (probes.media)(&video_path)?;
    if info.width <= 0 {
        return Err(String::new());
    }
    let mut recording = Recording {
        bundle_path: bundle_path.clone(),
        source_width: info.width,
        source_height: info.height,
        source_duration: info.duration,
        fps: if info.fps > 0.0 { info.fps } else { 60.0 },
        has_audio: info.audio,
        ..Recording::default()
    };
    let mut capture_root = Object::new();
    if let Some(bytes) = read_if_openable(&qdir::file_path(&bundle_path, "capture.json")) {
        capture_root = qjson::object_from_json(&bytes).unwrap_or_default();
        let scale = capture_root.value("scale").to_double(1.0);
        if scale > 0.0 {
            recording.capture_scale = scale;
        }
        let audio = capture_root.value("audio").to_object();
        recording.has_desktop_audio = info.audio && audio.value("desktop").to_bool(false);
        recording.has_microphone_audio = info.audio && audio.value("mic").to_bool(false);
    }
    let camera_capture = capture_root.value("camera").to_object();
    if FileInfo::new(camera_video_path.as_str()).is_file() {
        if let Ok(camera) = (probes.media)(&camera_video_path) {
            if camera.width > 0 {
                recording.camera = Some(CameraRecording {
                    width: camera.width,
                    height: camera.height,
                    fps: camera.fps,
                    duration: camera.duration,
                    offset: camera_offset(&capture_root),
                });
            }
        }
    }
    let new = !FileInfo::new(project_path.as_str()).is_file();
    let mut project = if new {
        new_project(
            &bundle_project_name(&bundle_path),
            recording.source_duration,
            recording.camera.is_some(),
            &camera_capture,
        )
    } else {
        let (project, result) = Project::load(&project_path);
        result?;
        project
    };
    if project.clips.iter().all(Clip::is_media) {
        project.clips.insert(
            0,
            Clip { id: "c1".into(), in_: 0.0, out: recording.source_duration, speed: 1.0, ..Clip::default() },
        );
    }
    // A missing inserted video keeps its clip; it shows as missing and blocks export.
    let mut inserts = HashMap::new();
    for clip in &project.clips {
        if clip.is_media() && !inserts.contains_key(&clip.media) {
            let info = (probes.video)(&qdir::file_path(&bundle_path, &clip.media)).unwrap_or_default();
            inserts.insert(clip.media.clone(), info);
        }
    }
    project.audio.desktop = project.audio.desktop && recording.has_desktop_audio;
    project.audio.mic = project.audio.mic && recording.has_microphone_audio;
    let input = InputLog::load_bundle(&bundle_path, recording.source_duration)?;
    if new {
        let clicks: Vec<f64> = input.click_downs(true).iter().map(|click| click.time).collect();
        project.zooms = zoom_timeline::generate(&clicks, recording.source_duration);
    }
    Ok(Bundle { recording, project, input, inserts, new_project: new })
}

/// What `importInsert` brought into the bundle (`Editor::ImportedInsert`): the clip's media
/// path and the video's facts, or why not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportedInsert {
    pub path: String,
    pub info: VideoInfo,
    pub error: String,
}

/// `[^A-Za-z0-9 _.-]` replaced by `_`, one per character, as Editor.cpp's QRegularExpression
/// does it.
pub(crate) fn replace_unsafe_name_characters(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, ' ' | '_' | '.' | '-') { c } else { '_' })
        .collect()
}

/// `QFileInfo(a) == QFileInfo(b)` for two paths: false if either is empty, true if they are the
/// same text or have the same canonical path.
fn same_file_info(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a == b || FileInfo::new(a).canonical_file_path() == FileInfo::new(b).canonical_file_path()
}

/// `importInsert` (off the GUI thread in the C++): probes a video and brings it into the
/// bundle's `media/` directory under a free name made from its own (unsafe characters as `_`,
/// leading dots dropped, "clip" if nothing is left, the suffix lower-cased, then `-2`, `-3`...),
/// copied through a hidden `.part` file. A video already in this bundle's `media/` is used where
/// it is.
pub fn import_insert(source: &str, bundle_path: &str, probe_video: &dyn Fn(&str) -> Result<VideoInfo, String>) -> ImportedInsert {
    let mut result = ImportedInsert::default();
    match probe_video(source) {
        Ok(info) => result.info = info,
        Err(error) => {
            result.error = error;
            return result;
        }
    }
    if !result.info.is_valid() {
        return result;
    }
    let media = qdir::file_path(bundle_path, "media");
    let file = FileInfo::new(source);
    let media_exists = qdir::dir_exists(&media);
    if media_exists
        && same_file_info(&file.canonical_path(), &FileInfo::new(media.as_str()).canonical_file_path())
        && !sanitized_media_path(&format!("media/{}", file.file_name())).is_empty()
    {
        result.path = format!("media/{}", file.file_name());
        return result;
    }
    if !media_exists && !qdir::mkpath(&media) {
        result.error = qstring::arg("Could not create %1", &media);
        return result;
    }
    let mut base = replace_unsafe_name_characters(&file.complete_base_name());
    base = base.trim_start_matches('.').to_owned();
    if base.is_empty() {
        base = "clip".into();
    }
    let suffix = file.suffix();
    let suffix = if suffix.is_empty() { String::new() } else { format!(".{}", qstring::to_lower(&suffix)) };
    let mut name = format!("{base}{suffix}");
    // `base` holds no '%', so the C++'s chained QString::arg is plain formatting.
    let mut n: i32 = 2;
    while qdir::dir_contains(&media, &name) {
        name = format!("{base}-{n}{suffix}");
        n = n.wrapping_add(1);
    }
    let partial = qdir::file_path(&media, &format!(".{name}.part"));
    if let Err(error) = clone_or_copy(source, &partial) {
        result.error = error;
        return result;
    }
    if !rename_no_replace(&partial, &qdir::file_path(&media, &name)) {
        let _ = std::fs::remove_file(&partial);
        result.error = qstring::arg("Could not add %1 to the recording", &name);
        return result;
    }
    result.path = format!("media/{name}");
    result
}

fn c_path(path: &str) -> Option<CString> {
    CString::new(path.as_bytes()).ok()
}

/// `cloneOrCopy`: a reflink (`FICLONE`) shares the original's blocks until either file changes,
/// instant and free on btrfs and xfs; elsewhere the file is copied with `QFile::copy`.
pub fn clone_or_copy(from: &str, to: &str) -> Result<(), String> {
    if let (Some(source_path), Some(target_path)) = (c_path(from), c_path(to)) {
        // SAFETY: plain system calls on paths we own; every descriptor opened here is closed.
        unsafe {
            let source = libc::open(source_path.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
            if source >= 0 {
                let target = libc::open(
                    target_path.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC | libc::O_CLOEXEC,
                    0o644 as libc::c_uint,
                );
                let cloned = target >= 0 && libc::ioctl(target, FICLONE as _, source) == 0;
                if target >= 0 {
                    libc::close(target);
                }
                libc::close(source);
                if cloned {
                    return Ok(());
                }
            }
        }
    }
    let _ = std::fs::remove_file(to);
    if qfile_copy(from, to) {
        return Ok(());
    }
    Err(qstring::arg("Could not copy %1 into the recording", &FileInfo::new(from).file_name()))
}

/// `FICLONE`, `_IOW(0x94, 9, int)`, the same on x86_64 and aarch64.
const FICLONE: libc::c_ulong = 0x4004_9409;

/// `QFile::rename(from, to)` within one directory: never replaces an existing `to` (Qt refuses
/// with "Destination file exists"), using `renameat2(RENAME_NOREPLACE)` and, where the file
/// system does not support it, a hard link and unlink, as `QFileSystemEngine::renameFile` does.
/// Qt's copy-and-delete fallback for renames across file systems is not needed here.
pub(crate) fn rename_no_replace(from: &str, to: &str) -> bool {
    let (Some(source), Some(target)) = (c_path(from), c_path(to)) else { return false };
    // "Source file does not exist" and "Destination file exists" (both follow symlinks).
    if !FileInfo::new(from).exists() || FileInfo::new(to).exists() {
        return false;
    }
    // SAFETY: system calls on NUL-terminated paths.
    unsafe {
        if libc::renameat2(libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, target.as_ptr(), libc::RENAME_NOREPLACE) == 0 {
            return true;
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINVAL) {
            return false;
        }
        if libc::link(source.as_ptr(), target.as_ptr()) == 0 {
            if libc::unlink(source.as_ptr()) == 0 {
                return true;
            }
            libc::unlink(target.as_ptr());
            return false;
        }
        let link_error = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
        if [libc::EACCES, libc::EEXIST, libc::ENAMETOOLONG, libc::ENOENT, libc::ENOTDIR, libc::EROFS, libc::EXDEV]
            .contains(&link_error)
        {
            return false;
        }
        libc::rename(source.as_ptr(), target.as_ptr()) == 0
    }
}

/// `QFile::copy(from, to)` on Linux: refuses an existing `to`, copies into a temporary file in
/// `to`'s directory (a reflink where possible), syncs it, renames it into place without
/// replacing anything, and gives `to` the source's permissions as `QFile::setPermissions` sets
/// them. Qt's further fallback to a temporary file in `QDir::tempPath()` is left out.
pub(crate) fn qfile_copy(from: &str, to: &str) -> bool {
    use std::io::{Read, Write};
    use std::os::unix::io::{AsRawFd, FromRawFd};
    if FileInfo::new(to).exists() {
        return false;
    }
    let Ok(mut source) = qfile::open_read(from) else { return false };
    let template = format!("{}/qt_temp.XXXXXX", FileInfo::new(to).path());
    let Ok(template) = CString::new(template) else { return false };
    let mut template = template.into_bytes_with_nul();
    // SAFETY: mkstemp fills in the template it is given, which is NUL-terminated.
    let fd = unsafe { libc::mkstemp(template.as_mut_ptr().cast()) };
    if fd < 0 {
        return false;
    }
    template.pop();
    let temporary = String::from_utf8_lossy(&template).into_owned();
    // SAFETY: mkstemp returned a fresh descriptor that nothing else owns.
    let mut out = unsafe { std::fs::File::from_raw_fd(fd) };
    let size = source.metadata().map_or(0, |m| m.len());
    // SAFETY: an ioctl between two open descriptors.
    let cloned = unsafe { libc::ioctl(out.as_raw_fd(), FICLONE as _, source.as_raw_fd()) } == 0;
    let mut ok = cloned;
    if !cloned {
        let mut total: u64 = 0;
        let mut block = [0u8; 4096];
        ok = true;
        loop {
            match source.read(&mut block) {
                Ok(0) => break,
                Ok(n) => {
                    if out.write_all(&block[..n]).is_err() {
                        ok = false;
                        break;
                    }
                    total += n as u64;
                }
                Err(_) => break,
            }
        }
        ok = ok && total == size;
    }
    if ok {
        let _ = out.sync_data();
        drop(out);
        ok = rename_no_replace(&temporary, to);
    } else {
        drop(out);
    }
    if !ok {
        let _ = std::fs::remove_file(&temporary);
        return false;
    }
    let mode = FileInfo::new(from).permissions_mode();
    if let Some(path) = c_path(to) {
        // SAFETY: chmod on a NUL-terminated path.
        unsafe { libc::chmod(path.as_ptr(), mode as libc::mode_t) };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn fraction_reads_rates() {
        assert_eq!(fraction("30/1"), 30.0);
        assert_eq!(fraction("30000/1001"), 30000.0 / 1001.0);
        assert_eq!(fraction("25"), 25.0);
        // A zero denominator reads the whole text, which is no number.
        assert_eq!(fraction("0/0"), 0.0);
        assert_eq!(fraction("1/2/3"), 0.0);
        assert_eq!(fraction(""), 0.0);
        assert_eq!(fraction(" 60 / 2 "), 30.0);
    }

    #[test]
    fn probe_output_is_read_like_editor_cpp() {
        let json = br#"{"streams":[{"codec_type":"audio"},{"codec_type":"video","width":0,"height":5},
            {"codec_type":"video","width":320,"height":180,"r_frame_rate":"30/1"},
            {"codec_type":"video","width":640,"height":360,"r_frame_rate":"60/1"}],
            "format":{"duration":"2.000000"}}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info, MediaInfo { width: 320, height: 180, duration: 2.0, fps: 30.0, audio: true });
        // No frame rate: 0, which loadBundle turns into 60.
        let info = parse_probe(br#"{"streams":[{"codec_type":"video","width":2,"height":2}],"format":{"duration":"1"}}"#).unwrap();
        assert_eq!(info.fps, 0.0);
        for bad in [&b"not json"[..], br#"{"streams":[{"codec_type":"video","width":2,"height":2}]}"#,
                    br#"{"streams":[],"format":{"duration":"3"}}"#,
                    br#"{"streams":[{"codec_type":"video","width":2,"height":2}],"format":{"duration":3}}"#] {
            assert_eq!(parse_probe(bad).unwrap_err(), "screen.mp4 has invalid metadata");
        }
        // Qt reads "nan" as a number, and a NaN duration is not <= 0.
        assert!(parse_probe(br#"{"streams":[{"codec_type":"video","width":2,"height":2}],"format":{"duration":"nan"}}"#)
            .unwrap()
            .duration
            .is_nan());
    }

    #[test]
    fn aspect_ratio_follows_the_project() {
        let mut project = Project::defaults("x", 1.0);
        assert_eq!(aspect_ratio(&project, 1920, 1080), 1920.0 / 1080.0);
        project.crop.width = 0.5;
        assert_eq!(aspect_ratio(&project, 1920, 1080), 960.0 / 1080.0);
        assert_eq!(aspect_ratio(&project, 100, 0), 50.0);
        for (aspect, ratio) in [("16:9", 16.0 / 9.0), ("1:1", 1.0), ("4:3", 4.0 / 3.0), ("9:16", 9.0 / 16.0), ("3:4", 0.75), ("4:5", 0.8)] {
            project.aspect = aspect.into();
            assert_eq!(aspect_ratio(&project, 1, 1), ratio);
        }
    }

    #[test]
    fn bundle_names_and_camera_capture() {
        assert_eq!(bundle_project_name("/x/editor.omareel"), "editor");
        assert_eq!(bundle_project_name("/x/legacy.omarecord"), "legacy");
        assert_eq!(bundle_project_name("/x/plain"), "plain");
        assert_eq!(bundle_project_name("/x/.omareel"), "");

        let capture = qjson::object_from_json(br#"{"rotation":270,"flipHorizontal":true,"shape":"square","radius":4}"#).unwrap();
        let project = new_project("n", 3.0, true, &capture);
        assert!(project.camera.enabled);
        assert_eq!(project.camera.rotation, 270);
        assert!(project.camera.flip_horizontal);
        assert_eq!(project.camera.shape, "square");
        assert_eq!(project.camera.radius, 4.0);
        assert_eq!(project.clips[0].out, 3.0);
        let capture = qjson::object_from_json(br#"{"rotation":45,"shape":"blob"}"#).unwrap();
        let project = new_project("n", 3.0, false, &capture);
        assert_eq!((project.camera.rotation, project.camera.shape.as_str(), project.camera.radius), (0, "round", 16.0));

        let both = qjson::object_from_json(br#"{"first_frame_us":1000000,"camera":{"first_frame_us":1250000}}"#).unwrap();
        assert_eq!(camera_offset(&both), 0.25);
        let strings = qjson::object_from_json(br#"{"first_frame_us":"2000000","camera":{"first_frame_us":1500000.4}}"#).unwrap();
        assert_eq!(camera_offset(&strings), -0.5);
        let missing = qjson::object_from_json(br#"{"first_frame_us":1000000}"#).unwrap();
        assert_eq!(camera_offset(&missing), 0.0);
    }

    fn fixed_probe(info: VideoInfo) -> impl Fn(&str) -> Result<VideoInfo, String> {
        move |_path: &str| Ok(info)
    }

    fn video() -> VideoInfo {
        VideoInfo { width: 320, height: 240, duration: 0.7, fps: 30.0, audio: false }
    }

    #[test]
    fn imports_copy_under_free_names() {
        let dir = TempDir::new("editor-import");
        let bundle = dir.join("b.omareel");
        std::fs::create_dir_all(&bundle).unwrap();
        let demo = dir.join("My Demo.MP4");
        std::fs::write(&demo, b"video bytes").unwrap();
        std::fs::set_permissions(&demo, std::fs::Permissions::from_mode(0o640)).unwrap();

        let first = import_insert(&demo, &bundle, &fixed_probe(video()));
        assert_eq!(first, ImportedInsert { path: "media/My Demo.mp4".into(), info: video(), error: String::new() });
        let copied = format!("{bundle}/media/My Demo.mp4");
        assert_eq!(std::fs::read(&copied).unwrap(), b"video bytes");
        let second = import_insert(&demo, &bundle, &fixed_probe(video()));
        assert_eq!(second.path, "media/My Demo-2.mp4");
        // Nothing is left behind but the two copies.
        let mut names: Vec<String> =
            std::fs::read_dir(format!("{bundle}/media")).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        assert_eq!(names, ["My Demo-2.mp4", "My Demo.mp4"]);

        // A video already in the bundle's media/ is used where it is.
        let again = import_insert(&copied, &bundle, &fixed_probe(video()));
        assert_eq!(again.path, "media/My Demo.mp4");

        // Unsafe characters, leading dots and an empty name.
        let odd = dir.join("..wé!rd.Name.WebM");
        std::fs::write(&odd, b"x").unwrap();
        assert_eq!(import_insert(&odd, &bundle, &fixed_probe(video())).path, "media/w__rd.Name.webm");
        let hidden = dir.join(".hidden");
        std::fs::write(&hidden, b"x").unwrap();
        assert_eq!(import_insert(&hidden, &bundle, &fixed_probe(video())).path, "media/clip.hidden");

        // Probe errors pass through; a video that probes as invalid gives no error and no path.
        let failed = import_insert(&demo, &bundle, &|_: &str| Err("notes.txt is not a video".to_owned()));
        assert_eq!(failed, ImportedInsert { error: "notes.txt is not a video".into(), ..ImportedInsert::default() });
        let invalid = import_insert(&demo, &bundle, &fixed_probe(VideoInfo::default()));
        assert_eq!(invalid, ImportedInsert::default());
    }

    #[test]
    fn import_reports_what_it_could_not_do() {
        let dir = TempDir::new("editor-import-errors");
        let bundle = dir.join("b.omareel");
        std::fs::create_dir_all(&bundle).unwrap();
        // media/ can't be made where a file stands.
        std::fs::write(format!("{bundle}/media"), b"").unwrap();
        let demo = dir.join("demo.mp4");
        std::fs::write(&demo, b"x").unwrap();
        let result = import_insert(&demo, &bundle, &fixed_probe(video()));
        assert_eq!(result.error, format!("Could not create {bundle}/media"));
        std::fs::remove_file(format!("{bundle}/media")).unwrap();
        // A source that can't be read can't be copied.
        let gone = dir.join("gone.mp4");
        let result = import_insert(&gone, &bundle, &fixed_probe(video()));
        assert_eq!(result.error, "Could not copy gone.mp4 into the recording");
    }

    #[test]
    fn copies_keep_the_source_permissions_and_never_replace() {
        let dir = TempDir::new("editor-copy");
        let from = dir.join("from");
        std::fs::write(&from, vec![7u8; 10_000]).unwrap();
        std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o640)).unwrap();
        let to = dir.join("to");
        assert!(qfile_copy(&from, &to));
        assert_eq!(std::fs::read(&to).unwrap(), vec![7u8; 10_000]);
        // The temporary file was 0600; the copy takes the source's mode.
        assert_eq!(std::fs::metadata(&to).unwrap().permissions().mode() & 0o7777, 0o640);
        assert!(!qfile_copy(&from, &to));
        assert!(clone_or_copy(&from, &to).is_ok());
        assert_eq!(std::fs::read(&to).unwrap().len(), 10_000);

        std::fs::write(dir.join("empty"), b"").unwrap();
        std::fs::write(dir.join("src"), b"x").unwrap();
        // Qt refuses to rename onto an existing file, even an empty one.
        assert!(!rename_no_replace(&dir.join("src"), &dir.join("empty")));
        assert!(rename_no_replace(&dir.join("src"), &dir.join("moved")));
        assert!(!rename_no_replace(&dir.join("src"), &dir.join("again")));
    }
}
