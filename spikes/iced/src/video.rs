//! Playback decoding for the preview: one thread per video running ffmpeg with the command
//! omareel_core's `FfmpegDecoder` uses (raw RGBA over a pipe, CUDA first and software when CUDA
//! gives no frame) a few frames ahead of the playhead, starting again wherever it jumps.
//!
//! Once the window's renderer exists ([`GPU`]), frames are read from the pipe straight into
//! mapped wgpu staging buffers of a small pool, and the UI thread only records a buffer to
//! texture copy. A 4K RGBA frame is 33 MB: going through `Queue::write_texture` cost the UI thread
//! 8 ms a frame (a memcpy into freshly faulted staging memory), and doing that on the decode
//! thread left it too slow to keep up at 60 fps.

use crate::scene::{Pixels, StagedFrame, VideoFrame};
use iced::wgpu;
use omareel_core::compat::qnumber::number_f64;
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

const AHEAD: usize = 4;
/// Staging buffers per stream: the queue, the frame being read, the frame on screen and frames
/// iced's primitives still hold.
const POOL: usize = AHEAD + 4;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The window renderer's device and queue, set by the scene pipeline when iced creates it.
pub static GPU: OnceLock<(wgpu::Device, wgpu::Queue)> = OnceLock::new();

/// A stream's staging buffers: the mapped, free ones and how many exist.
pub struct Pool {
    free: Mutex<(Vec<wgpu::Buffer>, usize)>,
    returned: Condvar,
}

impl Pool {
    pub fn put_back(&self, buffer: wgpu::Buffer) {
        self.free.lock().unwrap().0.push(buffer);
        self.returned.notify_all();
    }

    /// A mapped buffer of `size` bytes, made while fewer than [`POOL`] exist, else waited for
    /// (polling the device, which runs the map callbacks). `None` when `stop` says give up.
    fn take(&self, device: &wgpu::Device, size: u64, stop: &dyn Fn() -> bool) -> Option<wgpu::Buffer> {
        loop {
            {
                let mut free = self.free.lock().unwrap();
                if let Some(buffer) = free.0.pop() {
                    return Some(buffer);
                }
                if free.1 < POOL {
                    free.1 += 1;
                    return Some(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("decoded frame"),
                        size,
                        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: true,
                    }));
                }
            }
            if stop() {
                return None;
            }
            let _ = device.poll(wgpu::PollType::Poll);
            let free = self.free.lock().unwrap();
            let _ = self.returned.wait_timeout(free, Duration::from_millis(2)).unwrap();
        }
    }
}

/// ffmpeg decoding `length` seconds from `start`, as FfmpegDecoder runs it.
struct Ffmpeg {
    child: Child,
    hardware: bool,
    delivered: bool,
}

impl Ffmpeg {
    fn start(path: &str, start: f64, length: f64, fps: f64, hardware: bool) -> std::io::Result<Ffmpeg> {
        let mut args: Vec<String> = vec!["-v".into(), "error".into()];
        if hardware {
            args.extend(["-hwaccel".into(), "cuda".into()]);
        }
        args.extend([
            "-ss".into(),
            number_f64(start, 'f', 6),
            "-noaccurate_seek".into(),
            "-i".into(),
            path.into(),
            "-t".into(),
            number_f64(length, 'f', 6),
            "-vf".into(),
            format!("fps={}:start_time=0", number_f64(fps, 'g', 12)),
            "-f".into(),
            "rawvideo".into(),
            "-pix_fmt".into(),
            "rgba".into(),
            "-".into(),
        ]);
        let child = Command::new("ffmpeg").args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        Ok(Ffmpeg { child, hardware, delivered: false })
    }

    /// Fills `rows` (row-sized pieces of the frame); false at the end.
    fn read(&mut self, frame: &mut [u8], row: usize, stride: usize) -> bool {
        let Some(out) = self.child.stdout.as_mut() else { return false };
        let ok = if row == stride {
            out.read_exact(frame).is_ok()
        } else {
            frame.chunks_mut(stride).all(|line| out.read_exact(&mut line[..row]).is_ok())
        };
        self.delivered |= ok;
        ok
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct State {
    /// A start time the decoder should seek to.
    request: Option<f64>,
    /// Decoded frames by index (time * fps), in order.
    frames: VecDeque<(i64, VideoFrame)>,
    /// The first index of the stretch being decoded, and the next one it will hand over.
    decoding: Option<(i64, i64)>,
    ended: bool,
    quit: bool,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

pub struct Stream {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    fps: f64,
    current: Option<(i64, VideoFrame)>,
    /// Frames shown later than their time because the decoder was behind.
    pub late: u64,
}

impl Stream {
    pub fn new(path: &str, fps: f64, width: i32, height: i32, duration: f64) -> Stream {
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), changed: Condvar::new() });
        let worker = Arc::clone(&shared);
        let path = path.to_owned();
        let thread = std::thread::Builder::new()
            .name("decode".into())
            .spawn(move || decode(&worker, &path, fps, width as u32, height as u32, duration))
            .expect("decode thread");
        Stream { shared, thread: Some(thread), fps, current: None, late: 0 }
    }

    fn request(&self, time: f64) {
        let mut state = self.shared.state.lock().unwrap();
        state.request = Some(time.max(0.0));
        state.ended = false;
        drop(state);
        self.shared.changed.notify_all();
    }

    /// The frame for `time`: the newest decoded frame at or before the nearest frame. Rounding
    /// rather than flooring matters: the playhead is anchored at a vsync, so at 60 Hz every
    /// vsync lands within a millisecond of a frame time, where flooring would flip between two
    /// frames with the jitter of the frame callback. Starts the decoder again when `time` is
    /// behind it or well ahead of it.
    pub fn frame_at(&mut self, time: f64) -> Option<(i64, VideoFrame)> {
        let want = (time * self.fps).round() as i64;
        let seek = {
            let mut state = self.shared.state.lock().unwrap();
            let mut popped = false;
            while state.frames.front().is_some_and(|(index, _)| *index <= want) {
                self.current = state.frames.pop_front();
                popped = true;
            }
            if popped {
                self.shared.changed.notify_all();
            }
            if state.request.is_some() {
                false
            } else if let Some((first, next)) = state.decoding {
                let ahead_of_time = self.current.as_ref().is_some_and(|(index, _)| *index > want);
                let far_ahead = want > next + (self.fps * 0.5) as i64 && !state.ended;
                if want < first || far_ahead || ahead_of_time {
                    true
                } else {
                    if self.current.as_ref().is_some_and(|(index, _)| *index < want) && state.frames.is_empty() && !state.ended {
                        self.late += 1;
                    }
                    false
                }
            } else {
                true
            }
        };
        if seek {
            self.current = None;
            self.request(want as f64 / self.fps);
        }
        self.current.clone()
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().quit = true;
        self.shared.changed.notify_all();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn decode(shared: &Shared, path: &str, fps: f64, width: u32, height: u32, duration: f64) {
    let pool = Arc::new(Pool { free: Mutex::new((Vec::new(), 0)), returned: Condvar::new() });
    let row = width as usize * 4;
    let stride = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
    let size = stride * height as usize;
    let stop = || {
        let state = shared.state.lock().unwrap();
        state.quit || state.request.is_some()
    };
    loop {
        let start = {
            let mut state = shared.state.lock().unwrap();
            while state.request.is_none() && !state.quit {
                state = shared.changed.wait(state).unwrap();
            }
            if state.quit {
                return;
            }
            let start = state.request.take().unwrap_or(0.0);
            state.frames.clear();
            let first = (start * fps).round() as i64;
            state.decoding = Some((first, first));
            first
        };
        let start_time = start as f64 / fps;
        let length = (duration - start_time).max(1.0 / fps);
        let mut ffmpeg = match Ffmpeg::start(path, start_time, length, fps, true) {
            Ok(ffmpeg) => ffmpeg,
            Err(error) => {
                eprintln!("decoder: {error}");
                shared.state.lock().unwrap().ended = true;
                continue;
            }
        };
        let mut index = start;
        loop {
            let mut cpu = vec![];
            let staged = match GPU.get() {
                Some((device, _)) => match pool.take(device, size as u64, &stop) {
                    Some(buffer) => Some(buffer),
                    None => break,
                },
                None => {
                    cpu = vec![0u8; size];
                    None
                }
            };
            let mut read = |ffmpeg: &mut Ffmpeg| match &staged {
                Some(buffer) => ffmpeg.read(&mut buffer.slice(..).get_mapped_range_mut(), row, stride),
                None => ffmpeg.read(&mut cpu, row, stride),
            };
            let mut ok = read(&mut ffmpeg);
            if !ok && ffmpeg.hardware && !ffmpeg.delivered {
                // No CUDA: decode in software, as FfmpegDecoder falls back.
                match Ffmpeg::start(path, start_time, length, fps, false) {
                    Ok(software) => ffmpeg = software,
                    Err(_) => break,
                }
                ok = read(&mut ffmpeg);
            }
            if !ok {
                if let Some(buffer) = staged {
                    pool.put_back(buffer);
                }
                shared.state.lock().unwrap().ended = true;
                break;
            }
            let pixels = match staged {
                Some(buffer) => {
                    let pool = Arc::clone(&pool);
                    Pixels::Staged(Arc::new(StagedFrame::new(buffer, stride as u32, Arc::new(move |buffer| pool.put_back(buffer)))))
                }
                None => Pixels::Cpu(Arc::new(if stride == row { cpu } else { cpu.chunks(stride).flat_map(|line| line[..row].to_vec()).collect() })),
            };
            let frame = VideoFrame { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), width, height, pixels };
            let mut state = shared.state.lock().unwrap();
            while state.frames.len() >= AHEAD && state.request.is_none() && !state.quit {
                state = shared.changed.wait(state).unwrap();
            }
            if state.request.is_some() || state.quit {
                break;
            }
            state.frames.push_back((index, frame));
            index += 1;
            state.decoding = Some((start, index));
        }
    }
}
