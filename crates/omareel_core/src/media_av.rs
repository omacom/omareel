//! FFmpeg's libraries asked in process (`csrc/media.c`): what ffprobe and ffmpeg answered the
//! editor before, without a process each (30 to 70 ms of start and probing per question; a
//! filmstrip asks dozens). Each answer is `None` when the libraries could not give it, and the
//! callers then run the program as before, so errors read as they did.

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_long, c_void};

#[repr(C)]
struct RawInfo {
    has_video: c_int,
    has_audio: c_int,
    width: c_int,
    height: c_int,
    sar_num: c_int,
    sar_den: c_int,
    r_num: c_int,
    r_den: c_int,
    avg_num: c_int,
    avg_den: c_int,
    stream_duration: c_double,
    format_duration: c_double,
    color_space: c_int,
    has_rotation: c_int,
    rotation: c_int,
    has_rotate_tag: c_int,
    rotate_tag: [c_char; 32],
    error: [c_char; 512],
}

#[repr(C)]
struct RawStream {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn omr_stream_open(path: *const c_char, vaapi_device: *const c_char, width: c_int, height: c_int, threads: c_int) -> *mut RawStream;
    fn omr_stream_seek(stream: *mut RawStream, start: c_double, fps: c_double, preview: c_int) -> c_int;
    fn omr_stream_next(stream: *mut RawStream, y: *mut u8, uv: *mut u8, time: *mut c_double) -> c_int;
    fn omr_stream_close(stream: *mut RawStream);
    fn omr_probe(path: *const c_char, info: *mut RawInfo) -> c_int;
    fn omr_audio_duration(path: *const c_char, duration: *mut c_double, error: *mut c_char, error_size: usize) -> c_int;
    fn omr_frame_times(path: *const c_char, times: *mut *mut c_double) -> c_long;
    fn omr_keyframe_times(path: *const c_char, times: *mut *mut c_double) -> c_long;
    fn omr_color_space_name(space: c_int) -> *const c_char;
    fn omr_ff_preload() -> c_int;
    fn omr_free(pointer: *mut c_void);
    fn omr_thumbnail(
        path: *const c_char,
        at: c_double,
        width: c_int,
        height: c_int,
        quality: c_int,
        threads: c_int,
        output: *const c_char,
    ) -> c_int;
}

/// What ffprobe's `-show_entries stream=...:format=duration` reports of a file, for its first
/// video stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub has_video: bool,
    pub has_audio: bool,
    pub width: i32,
    pub height: i32,
    /// `sample_aspect_ratio`, 0 when unknown (ffprobe's "N/A").
    pub sample_aspect: f64,
    /// `r_frame_rate` and `avg_frame_rate`, 0 when unknown.
    pub frame_rate: f64,
    pub average_frame_rate: f64,
    /// `r_frame_rate` as ffprobe prints it ("60/1").
    pub frame_rate_text: String,
    /// The stream's and the file's durations, `None` when the file gives none.
    pub stream_duration: Option<f64>,
    pub format_duration: Option<f64>,
    /// `color_space` as ffprobe prints it ("bt709", "unknown").
    pub color_space: String,
    /// The display matrix's rotation, as ffprobe's `side_data_list` gives it.
    pub rotation: Option<i32>,
    /// The old `rotate` tag, as written.
    pub rotate_tag: Option<String>,
}

fn c_path(path: &str) -> Option<CString> {
    CString::new(path).ok()
}

fn rate(num: c_int, den: c_int) -> f64 {
    if den != 0 { f64::from(num) / f64::from(den) } else { 0.0 }
}

fn text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The file's streams, or why the libraries could not read it ("<path>: <reason>").
pub fn probe(path: &str) -> Result<Probe, String> {
    let c = c_path(path).ok_or_else(|| format!("{path}: invalid path"))?;
    // SAFETY: RawInfo is plain data the C side fills; the path outlives the call.
    let mut raw: RawInfo = unsafe { std::mem::zeroed() };
    if unsafe { omr_probe(c.as_ptr(), &mut raw) } != 0 {
        return Err(text(&raw.error));
    }
    let color_space = {
        // SAFETY: omr_color_space_name gives a static string or null.
        let name = unsafe { omr_color_space_name(raw.color_space) };
        if name.is_null() { "unknown".to_owned() } else { unsafe { CStr::from_ptr(name) }.to_string_lossy().into_owned() }
    };
    Ok(Probe {
        has_video: raw.has_video != 0,
        has_audio: raw.has_audio != 0,
        width: raw.width,
        height: raw.height,
        sample_aspect: if raw.sar_num != 0 { rate(raw.sar_num, raw.sar_den) } else { 0.0 },
        frame_rate: rate(raw.r_num, raw.r_den),
        average_frame_rate: rate(raw.avg_num, raw.avg_den),
        frame_rate_text: format!("{}/{}", raw.r_num, raw.r_den),
        stream_duration: (raw.stream_duration >= 0.0).then_some(raw.stream_duration),
        format_duration: (raw.format_duration >= 0.0).then_some(raw.format_duration),
        color_space,
        rotation: (raw.has_rotation != 0).then_some(raw.rotation),
        rotate_tag: (raw.has_rotate_tag != 0).then(|| text(&raw.rotate_tag)),
    })
}

/// The length of the first audio stream's sound (`-map 0:a:0`), its own rather than the
/// file's: `Ok(None)` when the file has no audio stream, `Ok(Some(d))` with `d` 0 or less when
/// the stream holds no sound, or why the libraries could not read it. Read from the packets
/// when the header gives no length or one guessed from the bitrate.
pub fn audio_duration(path: &str) -> Result<Option<f64>, String> {
    let c = c_path(path).ok_or_else(|| format!("{path}: invalid path"))?;
    let mut duration: c_double = -1.0;
    let mut error = [0 as c_char; 512];
    // SAFETY: the C side writes the duration and at most `error.len()` bytes of error.
    let code = unsafe { omr_audio_duration(c.as_ptr(), &mut duration, error.as_mut_ptr(), error.len()) };
    match code {
        0 => Ok(None),
        1 => Ok(Some(duration)),
        _ => Err(text(&error)),
    }
}

/// When each frame of the first video stream starts, in seconds and in order, as ffprobe's
/// `-show_entries packet=pts_time` lists them; read from an MP4's index where it can be.
pub fn frame_times(path: &str) -> Option<Vec<f64>> {
    let c = c_path(path)?;
    let mut times: *mut c_double = std::ptr::null_mut();
    // SAFETY: the C side allocates `count` doubles, freed here with its own free.
    let count = unsafe { omr_frame_times(c.as_ptr(), &mut times) };
    if count < 0 || (count > 0 && times.is_null()) {
        return None;
    }
    let result = if count == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(times, count as usize) }.to_vec() };
    unsafe { omr_free(times.cast()) };
    Some(result)
}

/// Opens FFmpeg's libraries on a thread of its own, so whatever needs them next doesn't wait for
/// the dynamic loader (about 20 ms): they are opened, not linked (csrc/ffmpeg_dl.h).
pub fn preload() {
    std::thread::spawn(|| {
        // SAFETY: no arguments; it only opens the libraries, once.
        unsafe { omr_ff_preload() };
    });
}

/// When each keyframe of the first video stream starts, in seconds from the file's start (the
/// time [`thumbnail`] takes), read from the index without decoding; `None` when the file can't
/// be read. A frame there decodes alone, where one between keyframes needs every frame before
/// it back to the last keyframe.
pub fn keyframe_times(path: &str) -> Option<Vec<f64>> {
    let c = c_path(path)?;
    let mut times: *mut c_double = std::ptr::null_mut();
    // SAFETY: as frame_times.
    let count = unsafe { omr_keyframe_times(c.as_ptr(), &mut times) };
    if count < 0 || (count > 0 && times.is_null()) {
        return None;
    }
    let result = if count == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(times, count as usize) }.to_vec() };
    unsafe { omr_free(times.cast()) };
    Some(result)
}

/// `ffmpeg -ss <at> -i <path> -frames:v 1 -vf scale=W:H:force_original_aspect_ratio=increase,
/// crop=W:H -q:v <quality> <output>`, decoding with `threads` threads. True when written.
///
/// It decodes on a thread of its own at a lower priority (nice 10, which its decoding threads
/// inherit): the filmstrip fills on whatever the editor and its players leave.
pub fn thumbnail(path: &str, at: f64, size: (i32, i32), quality: i32, threads: usize, output: &str) -> bool {
    let (Some(path), Some(output)) = (c_path(path), c_path(output)) else { return false };
    let threads = c_int::try_from(threads).unwrap_or(1);
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                // SAFETY: setpriority on this thread alone (Linux: a thread id is a process id
                // to it); both strings outlive the call.
                unsafe {
                    libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10);
                    omr_thumbnail(path.as_ptr(), at, size.0, size.1, quality, threads, output.as_ptr()) == 0
                }
            })
            .join()
            .unwrap_or(false)
    })
}

/// A video decoded for the preview and kept open between seeks: what
/// `ffmpeg [-hwaccel vaapi] -ss <start> -noaccurate_seek -i <path> -vf fps=<fps>:start_time=0,
/// scale=<w>:<h>:flags=bilinear,format=nv12 -f rawvideo -` gives, without a process per seek.
pub struct VideoStream {
    raw: *mut RawStream,
    pub width: u32,
    pub height: u32,
}

// SAFETY: the decoder is only used by one thread at a time (it moves between them whole).
unsafe impl Send for VideoStream {}

impl VideoStream {
    /// Opens `path` for NV12 frames of `width` x `height`, decoded on the VA-API device given, or
    /// in software with `threads` threads.
    /// NV12 needs an even size (its colour plane is half the width, in pairs): an odd one is
    /// rounded down, and `width` and `height` say what the frames are.
    pub fn open(path: &str, vaapi_device: Option<&str>, width: u32, height: u32, threads: usize) -> Option<VideoStream> {
        let (width, height) = ((width & !1).max(2), (height & !1).max(2));
        let path = c_path(path)?;
        let device = vaapi_device.map(c_path).unwrap_or(None);
        let threads = c_int::try_from(threads).unwrap_or(1);
        let (w, h) = (c_int::try_from(width).ok()?, c_int::try_from(height).ok()?);
        // SAFETY: the strings outlive the call; a null device means software decoding.
        let raw = unsafe { omr_stream_open(path.as_ptr(), device.as_ref().map_or(std::ptr::null(), |d| d.as_ptr()), w, h, threads) };
        (!raw.is_null()).then_some(VideoStream { raw, width, height })
    }

    /// The next frames are for `start`, `start + 1/fps`, ... It decodes on from where it is when
    /// no keyframe lies between. With `preview`, the keyframe a seek starts from comes first, at
    /// its own time, when the frame for `start` is further on: something close shows at once.
    pub fn seek(&mut self, start: f64, fps: f64, preview: bool) -> bool {
        // SAFETY: `raw` is open until drop.
        unsafe { omr_stream_seek(self.raw, start, fps, c_int::from(preview)) == 0 }
    }

    /// The next slot's frame into `y` (width*height) and `uv` (half that), and its time; `None`
    /// at the end or on an error.
    pub fn next(&mut self, y: &mut [u8], uv: &mut [u8]) -> Option<f64> {
        let size = self.width as usize * self.height as usize;
        if y.len() < size || uv.len() < size / 2 {
            return None;
        }
        let mut time = 0.0;
        // SAFETY: both buffers hold what the C side writes (checked above).
        (unsafe { omr_stream_next(self.raw, y.as_mut_ptr(), uv.as_mut_ptr(), &mut time) } == 1).then_some(time)
    }
}

impl Drop for VideoStream {
    fn drop(&mut self) {
        // SAFETY: closed once.
        unsafe { omr_stream_close(self.raw) };
    }
}
