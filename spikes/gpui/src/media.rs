//! Pictures for GPUI: decoded RGBA frames turned into the BGRA `RenderImage`s its image path
//! takes, the camera's blurred backdrop, the wallpaper, and the preview's decode threads.

use gpui::RenderImage;
use omareel_core::export::decoder::FfmpegDecoder;
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};

/// GPUI's images are BGRA; ffmpeg gives RGBA. Swaps red and blue in place, four bytes at a
/// time so the compiler vectorises it.
pub fn rgba_to_bgra(pixels: &mut [u8]) {
    let (head, words, tail) = unsafe { pixels.align_to_mut::<u32>() };
    debug_assert!(head.is_empty() && tail.is_empty());
    for word in words {
        let v = *word;
        *word = (v & 0xff00_ff00) | ((v >> 16) & 0xff) | ((v & 0xff) << 16);
    }
    for pixel in head.chunks_exact_mut(4).chain(tail.chunks_exact_mut(4)) {
        pixel.swap(0, 2);
    }
}

/// A BGRA copy of RGBA pixels, made on several threads (8 MB a frame at 1080p, 33 MB at 4K).
pub fn bgra_copy(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    // SAFETY: every byte is written below before anything reads it.
    unsafe { out.set_len(rgba.len()) };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    let chunk = (rgba.len() / 4).div_ceil(threads).max(1) * 4;
    std::thread::scope(|scope| {
        for (to, from) in out.chunks_mut(chunk).zip(rgba.chunks(chunk)) {
            scope.spawn(move || {
                to.copy_from_slice(from);
                rgba_to_bgra(to);
            });
        }
    });
    out
}

/// A BGRA picture as the `RenderImage` `img()` paints.
pub fn render_image(width: u32, height: u32, bgra: Vec<u8>) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(width, height, bgra).expect("frame size matches its pixels");
    Arc::new(RenderImage::new(SmallVec::from_elem(image::Frame::new(buffer), 1)))
}

/// Mirrors BGRA rows left to right (the camera's `flipHorizontal`).
pub fn flip_horizontal(width: usize, pixels: &mut [u8]) {
    for row in pixels.chunks_exact_mut(width * 4) {
        let words = unsafe { row.align_to_mut::<u32>().1 };
        words.reverse();
    }
}

/// BlurredBackdrop.qml's source: the camera as the square item shows it (a centred crop
/// unless the shape is a rectangle), captured into 24x24, blurred and slightly saturated.
/// GPUI has no blur or render-to-texture, so this runs on the CPU; at 24x24 that costs
/// nothing, and the GPU's bilinear filter does the rest as the image is drawn large.
pub fn camera_backdrop(width: usize, height: usize, bgra: &[u8], square: bool) -> Arc<RenderImage> {
    const N: usize = 24;
    let (cx, cy, cw, ch) = if square {
        let side = width.min(height);
        ((width - side) / 2, (height - side) / 2, side, side)
    } else {
        (0, 0, width, height)
    };
    // Area-average into N x N.
    let mut small = vec![[0f32; 3]; N * N];
    for ty in 0..N {
        let y0 = cy + ty * ch / N;
        let y1 = (cy + (ty + 1) * ch / N).max(y0 + 1);
        for tx in 0..N {
            let x0 = cx + tx * cw / N;
            let x1 = (cx + (tx + 1) * cw / N).max(x0 + 1);
            let mut sum = [0f32; 3];
            // Every other pixel each way is plenty for a 20x20 box average.
            for y in (y0..y1).step_by(2) {
                let row = &bgra[y * width * 4..];
                for x in (x0..x1).step_by(2) {
                    for c in 0..3 {
                        sum[c] += row[x * 4 + c] as f32;
                    }
                }
            }
            let count = (y1 - y0).div_ceil(2) * (x1 - x0).div_ceil(2);
            small[ty * N + tx] = sum.map(|v| v / count as f32);
        }
    }
    // A light separable Gaussian (sigma 0.8 texels, edges clamped): drawn 1.2 times the
    // canvas wide, a texel is about 100 output pixels, and the bilinear filter blends the rest.
    // This matches what Qt's MultiEffect makes of its 24x24 capture.
    const R: i32 = 2;
    let kernel: Vec<f32> = (-R..=R).map(|i| (-(i * i) as f32 / (2.0 * 0.8 * 0.8)).exp()).collect();
    let total: f32 = kernel.iter().sum();
    for horizontal in [true, false] {
        let source = small.clone();
        for y in 0..N {
            for x in 0..N {
                let mut sum = [0f32; 3];
                for (k, weight) in kernel.iter().enumerate() {
                    let offset = k as i32 - R;
                    let (sx, sy) = if horizontal {
                        ((x as i32 + offset).clamp(0, N as i32 - 1) as usize, y)
                    } else {
                        (x, (y as i32 + offset).clamp(0, N as i32 - 1) as usize)
                    };
                    let texel = source[sy * N + sx];
                    for c in 0..3 {
                        sum[c] += texel[c] * weight;
                    }
                }
                small[y * N + x] = sum.map(|v| v / total);
            }
        }
    }
    // MultiEffect's saturation 0.1 adds a tenth of saturation.
    let mut out = Vec::with_capacity(N * N * 4);
    for texel in small {
        let gray = 0.114 * texel[0] + 0.587 * texel[1] + 0.299 * texel[2];
        for c in texel {
            out.push((gray + (c - gray) * 1.1).clamp(0.0, 255.0) as u8);
        }
        out.push(255);
    }
    render_image(N as u32, N as u32, out)
}

/// The wallpaper, decoded once and kept at most 3840 pixels wide.
pub fn load_wallpaper(path: &str) -> Option<Arc<RenderImage>> {
    if path.is_empty() {
        return None;
    }
    let mut picture = image::open(path).ok()?.into_rgba8();
    if picture.width() > 3840 {
        let height = picture.height() * 3840 / picture.width();
        picture = image::imageops::resize(&picture, 3840, height, image::imageops::FilterType::Triangle);
    }
    let (width, height) = picture.dimensions();
    let mut pixels = picture.into_raw();
    rgba_to_bgra(&mut pixels);
    Some(render_image(width, height, pixels))
}

/// A decoded frame and when it is shown, in the stream's own seconds.
pub struct VideoFrame {
    pub time: f64,
    pub image: Arc<RenderImage>,
    pub backdrop: Option<Arc<RenderImage>>,
}

/// One video decoded ahead on its own thread by omareel_core's `FfmpegDecoder`: a stretch from
/// a start time at a fixed rate, RGBA turned BGRA (and blurred for the camera's backdrop) on
/// that thread. A bounded channel keeps it a few frames ahead.
pub struct Stream {
    receiver: Receiver<VideoFrame>,
    queued: VecDeque<VideoFrame>,
    pub current: Option<VideoFrame>,
    /// Frames thrown away because a later one was already due.
    pub skipped: u64,
    finished: bool,
}

pub struct StreamSpec {
    pub path: String,
    pub fps: f64,
    pub width: i32,
    pub height: i32,
    pub flip: bool,
    /// Have ffmpeg scale to `width` x `height`.
    pub scale: bool,
    /// Make the 24x24 backdrop for each frame (the camera).
    pub backdrop: Option<bool>,
    pub ahead: usize,
}

impl Stream {
    pub fn start(spec: &StreamSpec, start: f64, length: f64) -> Stream {
        let (sender, receiver) = sync_channel(spec.ahead);
        let path = spec.path.clone();
        let (fps, width, height, flip, scale, backdrop) =
            (spec.fps, spec.width, spec.height, spec.flip, spec.scale, spec.backdrop);
        std::thread::Builder::new()
            .name("decode".into())
            .spawn(move || decode(&path, start, length, fps, width, height, flip, scale, backdrop, &sender))
            .expect("decode thread");
        Stream { receiver, queued: VecDeque::new(), current: None, skipped: 0, finished: false }
    }

    /// Takes what has arrived, then makes the newest frame due at `time` current. Says whether
    /// the current frame changed.
    pub fn advance(&mut self, time: f64) -> bool {
        // Only take frames up to the first one still to come, so the channel's bound holds the
        // decoder back instead of decoded frames piling up here.
        while !self.queued.back().is_some_and(|frame| frame.time > time + 1e-6) {
            match self.receiver.try_recv() {
                Ok(frame) => self.queued.push_back(frame),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.finished = true;
                    break;
                }
            }
        }
        let mut changed = false;
        while self.queued.front().is_some_and(|frame| frame.time <= time + 1e-6) {
            if changed {
                self.skipped += 1;
            }
            self.current = self.queued.pop_front();
            changed = true;
        }
        changed
    }

    /// Whether the first frame has arrived.
    pub fn primed(&mut self) -> bool {
        if self.queued.is_empty()
            && let Ok(frame) = self.receiver.try_recv()
        {
            self.queued.push_back(frame);
        }
        self.current.is_some() || !self.queued.is_empty()
    }

    /// Whether a frame is still to come at all.
    pub fn exhausted(&self) -> bool {
        self.finished && self.queued.is_empty()
    }
}

#[allow(clippy::too_many_arguments)]
fn decode(
    path: &str,
    start: f64,
    length: f64,
    fps: f64,
    width: i32,
    height: i32,
    flip: bool,
    scale: bool,
    backdrop: Option<bool>,
    sender: &SyncSender<VideoFrame>,
) {
    let mut decoder = FfmpegDecoder::new();
    let mut error = String::new();
    if !decoder.start(path, start, length, fps, width, height, scale, &mut error) {
        eprintln!("decoder: {error}");
        return;
    }
    let mut index = 0u64;
    while let Some(mut pixels) = decoder.read_frame(&mut error) {
        rgba_to_bgra(&mut pixels);
        if flip {
            flip_horizontal(width as usize, &mut pixels);
        }
        let backdrop = backdrop.map(|square| camera_backdrop(width as usize, height as usize, &pixels, square));
        let frame = VideoFrame {
            time: start + index as f64 / fps,
            image: render_image(width as u32, height as u32, pixels),
            backdrop,
        };
        if sender.send(frame).is_err() {
            return;
        }
        index += 1;
    }
    if !error.is_empty() {
        eprintln!("decoder: {error}");
    }
}
