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

unsafe extern "C" {
    fn omr_probe(path: *const c_char, info: *mut RawInfo) -> c_int;
    fn omr_frame_times(path: *const c_char, times: *mut *mut c_double) -> c_long;
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
        // SAFETY: av_color_space_name gives a static string or null.
        unsafe extern "C" {
            fn av_color_space_name(space: c_int) -> *const c_char;
        }
        let name = unsafe { av_color_space_name(raw.color_space) };
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
