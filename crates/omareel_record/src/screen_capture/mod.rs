//! The parts of `ScreenCapture.cpp` that need no compositor: the capture ring's bookkeeping, the
//! crop's row copies, the Matroska stream that carries captured BGRA frames to ffmpeg, the crop
//! trim and the encoder arguments `ScreenCapture::start` decides on, the encoder probe and its
//! cache ([`probe`]), and the writer thread's loop over an abstract encoder input ([`writer`]).
//!
//! [`session`] is the capture itself: the Wayland session (`ext-image-copy-capture-v1`) on a
//! connection of its own, the shared-memory slots it captures into, the encoder process
//! ([`encoder`]) and the writer thread. It acquires a slot from the ring, lets the compositor
//! fill it, and queues it for the writer with its presentation time.

pub mod encoder;
pub mod probe;
pub mod session;
pub mod writer;

pub use probe::{
    EncoderProbeResults, cached_preferred_encoder_available, encoder_probe_cache_path, encoder_probe_key,
    encoder_probe_results,
};
pub use session::{ScreenCapture, ScreenCaptureConfig, is_supported};
pub use writer::{EncoderSink, FdSink, FrameQueue, RawSlotMemory, SlotMemory, WriterConfig, writer_loop};

/// `QSize`, as far as the capture uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub const fn new(width: i32, height: i32) -> Size {
        Size { width, height }
    }

    /// `QSize()`: -1 by -1, invalid.
    pub const fn invalid() -> Size {
        Size { width: -1, height: -1 }
    }

    /// `QSize::isEmpty`: a side below 1.
    pub fn is_empty(&self) -> bool {
        self.width < 1 || self.height < 1
    }
}

impl Default for Size {
    fn default() -> Size {
        Size::invalid()
    }
}

/// `QRect`, kept as Qt keeps it: the top-left and the bottom-right pixel, so an empty rectangle
/// has `x2 = x1 - 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl Default for Rect {
    /// `QRect()`: null, at 0,0 with a size of 0 by 0.
    fn default() -> Rect {
        Rect { x1: 0, y1: 0, x2: -1, y2: -1 }
    }
}

impl Rect {
    /// `QRect(x, y, width, height)`.
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect { x1: x, y1: y, x2: x.wrapping_add(width).wrapping_sub(1), y2: y.wrapping_add(height).wrapping_sub(1) }
    }

    /// `QRect(QPoint(), size)`.
    pub const fn from_size(size: Size) -> Rect {
        Rect::new(0, 0, size.width, size.height)
    }

    pub const fn x(&self) -> i32 {
        self.x1
    }

    pub const fn y(&self) -> i32 {
        self.y1
    }

    pub const fn width(&self) -> i32 {
        self.x2.wrapping_sub(self.x1).wrapping_add(1)
    }

    pub const fn height(&self) -> i32 {
        self.y2.wrapping_sub(self.y1).wrapping_add(1)
    }

    pub const fn size(&self) -> Size {
        Size::new(self.width(), self.height())
    }

    /// `QRect::isNull`: both sides 0.
    pub const fn is_null(&self) -> bool {
        self.x2 == self.x1.wrapping_sub(1) && self.y2 == self.y1.wrapping_sub(1)
    }

    /// `QRect::isValid`: both sides above 0.
    pub const fn is_valid(&self) -> bool {
        self.x1 <= self.x2 && self.y1 <= self.y2
    }

    /// `QRect::setWidth`: keeps the left edge.
    pub fn set_width(&mut self, width: i32) {
        self.x2 = self.x1.wrapping_add(width).wrapping_sub(1);
    }

    /// `QRect::setHeight`: keeps the top edge.
    pub fn set_height(&mut self, height: i32) {
        self.y2 = self.y1.wrapping_add(height).wrapping_sub(1);
    }

    /// `QRect::intersected` (`operator&`): `QRect()` when either is null or they do not
    /// overlap; a rectangle with negative sides is normalised first.
    pub fn intersected(&self, other: &Rect) -> Rect {
        if self.is_null() || other.is_null() {
            return Rect::default();
        }
        let span = |a1: i32, a2: i32| {
            if a2 < a1.wrapping_sub(1) { (a2.wrapping_add(1), a1.wrapping_sub(1)) } else { (a1, a2) }
        };
        let (l1, r1) = span(self.x1, self.x2);
        let (l2, r2) = span(other.x1, other.x2);
        if l1 > r2 || l2 > r1 {
            return Rect::default();
        }
        let (t1, b1) = span(self.y1, self.y2);
        let (t2, b2) = span(other.y1, other.y2);
        if t1 > b2 || t2 > b1 {
            return Rect::default();
        }
        Rect { x1: l1.max(l2), x2: r1.min(r2), y1: t1.max(t2), y2: b1.min(b2) }
    }
}

// ---------------------------------------------------------------------------------------------
// CaptureRingBookkeeping

/// `CaptureRingBookkeeping::State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotState {
    Available,
    Capturing,
    Queued,
    Writing,
}

/// `CaptureRingBookkeeping`: which capture slot is free, being captured into, queued for the
/// writer or being written, the queue's order, and how many captures found no free slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureRingBookkeeping {
    states: Vec<SlotState>,
    queue: Vec<i32>,
    drops: i32,
}

impl Default for CaptureRingBookkeeping {
    fn default() -> CaptureRingBookkeeping {
        CaptureRingBookkeeping::new(4)
    }
}

impl CaptureRingBookkeeping {
    /// At least one slot.
    pub fn new(slot_count: i32) -> CaptureRingBookkeeping {
        CaptureRingBookkeeping {
            states: vec![SlotState::Available; slot_count.max(1) as usize],
            queue: Vec::new(),
            drops: 0,
        }
    }

    fn slot_index(&self, slot: i32) -> Option<usize> {
        usize::try_from(slot).ok().filter(|&index| index < self.states.len())
    }

    /// The first available slot, now capturing; -1 (and a drop) when there is none.
    pub fn acquire(&mut self) -> i32 {
        if let Some(index) = self.states.iter().position(|&state| state == SlotState::Available) {
            self.states[index] = SlotState::Capturing;
            return index as i32;
        }
        self.drops += 1;
        -1
    }

    /// A capturing slot joins the end of the queue.
    pub fn mark_queued(&mut self, slot: i32) -> bool {
        match self.slot_index(slot) {
            Some(index) if self.states[index] == SlotState::Capturing => {
                self.states[index] = SlotState::Queued;
                self.queue.push(slot);
                true
            }
            _ => false,
        }
    }

    /// The oldest queued slot, now being written; -1 when the queue is empty or its head is not
    /// queued any more (the head is taken off either way).
    pub fn take_queued(&mut self) -> i32 {
        if self.queue.is_empty() {
            return -1;
        }
        let slot = self.queue.remove(0);
        match self.slot_index(slot) {
            Some(index) if self.states[index] == SlotState::Queued => {
                self.states[index] = SlotState::Writing;
                slot
            }
            _ => -1,
        }
    }

    /// Any slot that is not available becomes available.
    pub fn release(&mut self, slot: i32) -> bool {
        match self.slot_index(slot) {
            Some(index) if self.states[index] != SlotState::Available => {
                self.states[index] = SlotState::Available;
                true
            }
            _ => false,
        }
    }

    /// A slot's state; `Available` for one outside the ring.
    pub fn state(&self, slot: i32) -> SlotState {
        self.slot_index(slot).map_or(SlotState::Available, |index| self.states[index])
    }

    pub fn count(&self, state: SlotState) -> i32 {
        self.states.iter().filter(|&&s| s == state).count() as i32
    }

    pub fn drops(&self) -> i32 {
        self.drops
    }
}

// ---------------------------------------------------------------------------------------------
// captureCropRows

/// `CaptureRowCopy`: one copy from a captured frame into the encoded frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureRowCopy {
    pub source_offset: i64,
    pub destination_offset: i64,
    pub bytes: i64,
}

/// `captureCropRows`: the copies that cut `requested_crop` (the whole frame when it is not
/// valid, else its part inside the frame) out of a BGRA frame of `source_size` with
/// `source_stride` bytes per row. One copy when the crop spans whole rows, else one per row;
/// none when the frame is empty, the stride is too short or the crop misses the frame.
pub fn capture_crop_rows(source_size: Size, source_stride: i32, requested_crop: Rect) -> Vec<CaptureRowCopy> {
    let mut result = Vec::new();
    if source_size.is_empty() || source_stride < source_size.width.wrapping_mul(4) {
        return result;
    }
    let frame = Rect::from_size(source_size);
    let crop = if requested_crop.is_valid() { requested_crop.intersected(&frame) } else { frame };
    if !crop.is_valid() {
        return result;
    }
    let stride = i64::from(source_stride);
    let row_bytes = i64::from(crop.width()) * 4;
    if crop.x() == 0 && row_bytes == stride {
        result.push(CaptureRowCopy {
            source_offset: i64::from(crop.y()) * stride,
            destination_offset: 0,
            bytes: row_bytes * i64::from(crop.height()),
        });
        return result;
    }
    result.reserve(crop.height() as usize);
    for row in 0..crop.height() {
        result.push(CaptureRowCopy {
            source_offset: i64::from(crop.y() + row) * stride + i64::from(crop.x()) * 4,
            destination_offset: i64::from(row) * row_bytes,
            bytes: row_bytes,
        });
    }
    result
}

// ---------------------------------------------------------------------------------------------
// The Matroska stream

/// Just enough EBML for one uncompressed video track. IDs keep their marker bits; every size is
/// an eight-byte vint so element lengths never change how a header is laid out.
fn append_ebml_id(out: &mut Vec<u8>, id: u32) {
    for shift in [24, 16, 8, 0] {
        if id >> shift != 0 {
            out.push(((id >> shift) & 0xff) as u8);
        }
    }
}

fn append_ebml_size(out: &mut Vec<u8>, size: u64) {
    out.push(0x01);
    for shift in (0..=48).rev().step_by(8) {
        out.push(((size >> shift) & 0xff) as u8);
    }
}

fn ebml_element(id: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + payload.len());
    append_ebml_id(&mut out, id);
    append_ebml_size(&mut out, payload.len() as u64);
    out.extend_from_slice(payload);
    out
}

fn ebml_unsigned(id: u32, value: u64) -> Vec<u8> {
    ebml_element(id, &value.to_be_bytes())
}

/// `captureStreamHeader`: the EBML header, an unknown-size segment, the segment info (a
/// microsecond timestamp scale) and one `V_UNCOMPRESSED` BGRA track of `size` whose default
/// duration is one frame at `fps` (at least 1). Captured frames reach ffmpeg as this
/// uncompressed Matroska so each keeps its capture time; raw video would carry none.
pub fn capture_stream_header(size: Size, fps: i32) -> Vec<u8> {
    let ebml = ebml_element(
        0x1A45DFA3,
        &[
            ebml_unsigned(0x4286, 1),
            ebml_unsigned(0x42F7, 1),
            ebml_unsigned(0x42F2, 4),
            ebml_unsigned(0x42F3, 8),
            ebml_element(0x4282, b"matroska"),
            ebml_unsigned(0x4287, 4),
            ebml_unsigned(0x4285, 2),
        ]
        .concat(),
    );
    // Timestamps count microseconds; DefaultDuration gives the last frame one interval.
    let info = ebml_element(
        0x1549A966,
        &[ebml_unsigned(0x2AD7B1, 1000), ebml_element(0x4D80, b"omareel"), ebml_element(0x5741, b"omareel")].concat(),
    );
    let video = ebml_element(
        0xE0,
        &[
            ebml_unsigned(0xB0, size.width as u64),
            ebml_unsigned(0xBA, size.height as u64),
            ebml_element(0x2EB524, b"BGRA"),
        ]
        .concat(),
    );
    let default_duration = (1e9 / f64::from(fps.max(1))).round() as i64 as u64;
    let track = ebml_element(
        0xAE,
        &[
            ebml_unsigned(0xD7, 1),
            ebml_unsigned(0x73C5, 1),
            ebml_unsigned(0x83, 1),
            ebml_element(0x86, b"V_UNCOMPRESSED"),
            ebml_unsigned(0x9C, 0),
            ebml_unsigned(0x23E383, default_duration),
            video,
        ]
        .concat(),
    );
    let mut segment = Vec::new();
    append_ebml_id(&mut segment, 0x18538067);
    // Unknown size: the stream ends with the pipe.
    segment.extend_from_slice(&[0x01, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    [ebml, segment, info, ebml_element(0x1654AE6B, &track)].concat()
}

/// `captureFrameHeader`: a cluster at `timestamp_us` (0 for a negative one) holding one
/// SimpleBlock of track 1, a keyframe at no offset, whose `frame_bytes` follow the header.
pub fn capture_frame_header(timestamp_us: i64, frame_bytes: i64) -> Vec<u8> {
    // Track 1, no offset from the cluster timestamp, keyframe.
    let block_header = [0x81u8, 0x00, 0x00, 0x80];
    let mut block = Vec::new();
    append_ebml_id(&mut block, 0xA3);
    append_ebml_size(&mut block, (block_header.len() as i64).wrapping_add(frame_bytes) as u64);
    block.extend_from_slice(&block_header);
    let timestamp = ebml_unsigned(0xE7, timestamp_us.max(0) as u64);
    let mut cluster = Vec::new();
    append_ebml_id(&mut cluster, 0x1F43B675);
    append_ebml_size(&mut cluster, (timestamp.len() as i64 + block.len() as i64).wrapping_add(frame_bytes) as u64);
    [cluster, timestamp, block].concat()
}

// ---------------------------------------------------------------------------------------------
// ScreenCapture::start's decisions

/// What `ScreenCapture::start` works out about the crop once the session reported the output's
/// size (`ScreenCapture::Private`'s `crop`, `encodedSize` and `cropRows`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureCrop {
    pub crop: Rect,
    /// `QSize()` (-1 by -1) until the crop is known to be valid.
    pub encoded_size: Size,
    pub crop_rows: Vec<CaptureRowCopy>,
}

/// The crop trim of `ScreenCapture::start` (lines 855–870 at the tag) for an output of `width` by
/// `height` with `stride` bytes per row: the requested crop inside the output (the whole output
/// when it is not valid), trimmed to even sides because the encoders reject odd frame sizes, and
/// the row copies. `Err` holds `start`'s message; `crop` holds what it left in the fields either
/// way (the encoded size and the rows are only set once the crop is valid).
pub fn trim_capture_crop(
    width: i32,
    height: i32,
    stride: i32,
    requested: Rect,
    crop: &mut CaptureCrop,
) -> Result<(), String> {
    let output = Rect::new(0, 0, width, height);
    crop.crop = if requested.is_valid() { requested.intersected(&output) } else { output };
    // The encoders reject odd frame sizes; a crop that lost a pixel to the output edge (or a
    // whole output of odd size) is trimmed rather than failing at the first frame.
    let (crop_width, crop_height) = (crop.crop.width(), crop.crop.height());
    crop.crop.set_width(crop_width - crop_width % 2);
    crop.crop.set_height(crop_height - crop_height % 2);
    if !crop.crop.is_valid() {
        return Err("The capture region is outside the monitor".into());
    }
    crop.encoded_size = crop.crop.size();
    crop.crop_rows = capture_crop_rows(Size::new(width, height), stride, crop.crop);
    if crop.crop_rows.is_empty() {
        return Err("The capture crop could not be prepared".into());
    }
    Ok(())
}

/// The encoder `ScreenCapture::start` runs and how it converts frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncoderArguments {
    /// `conversionMode()`: "gpu" when CUDA converts the frames, else "cpu".
    pub conversion: String,
    /// ffmpeg's arguments, without the program name.
    pub arguments: Vec<String>,
}

/// The encoder's arguments as `ScreenCapture::start` builds them (lines 811–812, 898 and 903–962
/// at the tag) once the probes have answered: the frame size that is encoded, the requested
/// rate (at least 1 is used), the probe results, `OMAREEL_NATIVE_CONVERSION` and
/// `OMAREEL_NATIVE_ENCODER` (`None` when unset) and the output path. NVENC when it works, losslessly
/// below 1.5 megapixels and as high-quality VBR above, with CUDA converting the frames unless
/// the conversion is asked to be "cpu"; libx264 without it; libx264rgb losslessly as a debug aid.
pub fn encoder_arguments(
    encoded_size: Size,
    fps: i32,
    probe_results: &EncoderProbeResults,
    native_conversion: Option<&str>,
    native_encoder: Option<&str>,
    output_path: &str,
) -> EncoderArguments {
    let fps = fps.max(1);
    let preferred = probe_results.preferred;
    let requested_conversion = omareel_core::compat::qstring::to_lower(native_conversion.unwrap_or(""));
    let use_gpu_conversion = preferred && requested_conversion != "cpu" && probe_results.gpu_conversion;
    let conversion = if use_gpu_conversion { "gpu" } else { "cpu" };
    let mut arguments: Vec<String> =
        ["-y", "-loglevel", "error", "-f", "matroska", "-i", "pipe:0", "-an"].map(String::from).to_vec();
    let push = |arguments: &mut Vec<String>, items: &[&str]| arguments.extend(items.iter().map(|s| (*s).to_owned()));
    if use_gpu_conversion {
        push(&mut arguments, &["-vf", "hwupload_cuda,scale_cuda=format=nv12"]);
    }
    push(&mut arguments, &["-c:v"]);
    // Quality: the capture is an intermediate that gets re-encoded on export, so keep it near
    // visually lossless. A plain -cq without a rate mode made NVENC starve small regions, so pin
    // VBR with a generous ceiling scaled to the frame size, plus a 1-second GOP for clean seeking.
    let megapixels = f64::from(encoded_size.width) * f64::from(encoded_size.height) / 1e6;
    let maxrate_kbps = (megapixels * 12000.0 * (f64::from(fps) / 60.0)).clamp(8000.0, 120000.0) as i32;
    // Debug aid: OMAREEL_NATIVE_ENCODER=lossless stores the raw capture losslessly so capture
    // problems can be told apart from encoder artefacts.
    let lossless = native_encoder == Some("lossless");
    // Small captures (regions, windows) are cheap enough to store losslessly (after the 4:2:0
    // conversion, which still softens coloured edges), and NVENC's lossy modes smear
    // high-contrast edges into visible macroblocks at these sizes.
    let small_frame = megapixels < 1.5;
    let gop = fps.to_string();
    if lossless {
        push(&mut arguments, &["libx264rgb", "-preset", "ultrafast", "-qp", "0"]);
    } else if preferred && small_frame {
        push(
            &mut arguments,
            &["h264_nvenc", "-preset", "p4", "-tune", "lossless", "-zerolatency", "1", "-g", &gop, "-bf", "0"],
        );
    } else if preferred {
        let maxrate = format!("{maxrate_kbps}k");
        let bufsize = format!("{}k", maxrate_kbps.wrapping_mul(2));
        push(
            &mut arguments,
            &[
                "h264_nvenc",
                "-preset",
                "p4",
                "-tune",
                "hq",
                "-zerolatency",
                "1",
                "-rc",
                "vbr",
                "-cq",
                "16",
                "-b:v",
                "0",
                "-maxrate",
                &maxrate,
                "-bufsize",
                &bufsize,
                "-g",
                &gop,
                "-bf",
                "0",
            ],
        );
    } else {
        // -g is one second. Without zerolatency the encoder may hold that GOP and a stop never
        // writes it, so the raw recording ends about a second early.
        let crf = if small_frame { "10" } else { "16" };
        push(
            &mut arguments,
            &["libx264", "-preset", "veryfast", "-tune", "zerolatency", "-crf", crf, "-g", &gop, "-bf", "0"],
        );
    }
    if !use_gpu_conversion && !lossless {
        // swscale converts RGB with the BT.601 matrix unless told otherwise, while players read
        // untagged HD video as BT.709, so reds and greens drifted. Convert with BT.709 and say so.
        push(&mut arguments, &["-vf", "scale=out_color_matrix=bt709:out_range=tv", "-pix_fmt", "yuv420p"]);
        push(&mut arguments, &["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709"]);
        push(&mut arguments, &["-color_range", "tv"]);
    }
    // Frames keep the times they were captured at, on a 1/fps time base. Gaps stay gaps.
    let time_base = format!("1/{fps}");
    push(&mut arguments, &["-fps_mode", "vfr", "-enc_time_base", &time_base]);
    // Fragmented, so a recording whose encoder dies (a crash, an OOM kill, a power cut) is still
    // playable up to its last fragment; a plain MP4 is unreadable without the index written at
    // the end. No +faststart: it rewrites the whole file after encoding, which can outlast the
    // wait in `stop_encoder` on a long recording, and the editor reads the file locally anyway.
    // A still screen makes fragments small enough to sit in ffmpeg's output buffer, so each
    // packet is flushed as it is written.
    push(&mut arguments, &["-flush_packets", "1", "-movflags", "+frag_keyframe+empty_moov+default_base_moof"]);
    push(&mut arguments, &[output_path]);
    EncoderArguments { conversion: conversion.to_owned(), arguments }
}

/// `qEnvironmentVariable(name)` as an `Option`.
fn environment(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

/// [`encoder_arguments`] with `OMAREEL_NATIVE_CONVERSION` and `OMAREEL_NATIVE_ENCODER` read from
/// the environment, as `start` reads them.
pub fn encoder_arguments_from_environment(
    encoded_size: Size,
    fps: i32,
    probe_results: &EncoderProbeResults,
    output_path: &str,
) -> EncoderArguments {
    let conversion = environment("OMAREEL_NATIVE_CONVERSION");
    let encoder = environment("OMAREEL_NATIVE_ENCODER");
    encoder_arguments(encoded_size, fps, probe_results, conversion.as_deref(), encoder.as_deref(), output_path)
}

/// `ScreenCapture::videoDurationUs`: from the first frame to the end of the last one written
/// (`last_frame_index` on the 1/fps grid, -1 for none).
pub fn video_duration_us(last_frame_index: i64, fps: i32) -> i64 {
    if last_frame_index < 0 { 0 } else { ((last_frame_index + 1) as f64 * 1e6 / f64::from(fps)).round() as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rects_follow_qrect() {
        let frame = Rect::new(0, 0, 4, 3);
        assert_eq!(Rect::new(-1, -1, 3, 3).intersected(&frame), Rect::new(0, 0, 2, 2));
        assert_eq!(Rect::new(4, 0, 4, 4).intersected(&frame), Rect::default());
        assert!(Rect::default().is_null() && !Rect::default().is_valid());
        assert!(!Rect::new(0, 0, -2, 2).is_valid());
        let mut odd = Rect::new(1, 1, 3, 5);
        odd.set_width(2);
        assert_eq!((odd.x(), odd.width(), odd.height()), (1, 2, 5));
    }

    #[test]
    fn duration_rounds_half_away() {
        assert_eq!(video_duration_us(-1, 60), 0);
        assert_eq!(video_duration_us(0, 60), 16667);
        assert_eq!(video_duration_us(60, 30), 2033333);
    }
}
