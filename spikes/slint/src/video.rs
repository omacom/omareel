//! The preview's decoding: one thread per video running omareel_core's `FfmpegDecoder` (the
//! export's ffmpeg command, RGBA on a pipe) from a start time, a few frames ahead of the playhead.
//!
//! With a GPU given, the decode thread copies each frame into a mapped wgpu staging buffer and
//! hands over the buffer, so the 33 MB copy of a 4K frame happens there; the UI thread only
//! records a buffer-to-texture copy. (`Queue::write_texture` from the decode thread does the copy
//! there too, but holds wgpu's queue lock meanwhile, which stalls the UI thread's frame submit.)
//! A buffer goes back to the pool mapped again once the GPU is done with it.

use omareel_core::export::decoder::FfmpegDecoder;
use slint::wgpu_30::wgpu;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

pub enum Pixels {
    Cpu(Vec<u8>),
    /// An unmapped staging buffer holding the frame (rows padded to 256 bytes), to copy into a
    /// texture and then give back with [`Stream::recycle`].
    Staged(wgpu::Buffer),
}

pub struct Frame {
    /// Seconds in the video.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
    /// For the camera's blurred backdrop, which is made on the CPU: a copy kept only when asked.
    pub cpu_copy: Option<Vec<u8>>,
}

/// Where a decode thread uploads.
#[derive(Clone)]
pub struct GpuSink {
    pub device: wgpu::Device,
    /// Keep a CPU copy of each frame too (the camera, for its blurred copy).
    pub keep_cpu_copy: bool,
}

/// A video decoding from one point on.
pub struct Stream {
    frames: Receiver<Frame>,
    pool: Option<Arc<Pool>>,
    stop: Arc<AtomicBool>,
    /// The frame on screen and the next one, once read off the queue.
    pub current: Option<Frame>,
    next: Option<Frame>,
    pub fps: f64,
    pub finished: bool,
}

impl Stream {
    /// Decodes `path` from `start` to `end` seconds at `fps`, as `width` x `height` RGBA (scaled
    /// by ffmpeg when that is not the video's size), keeping up to `ahead` frames queued.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        path: &str,
        start: f64,
        end: f64,
        fps: f64,
        width: i32,
        height: i32,
        scale: bool,
        ahead: usize,
        gpu: Option<GpuSink>,
    ) -> Stream {
        let (sender, frames) = sync_channel(ahead);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let path = path.to_owned();
        // Queued frames, the next one, one being copied, one being filled, and a spare.
        let pool = gpu.as_ref().map(|_| Arc::new(Pool { free: Mutex::new((Vec::new(), 0)), returned: Condvar::new(), limit: ahead + 4 }));
        let ring = gpu.zip(pool.clone());
        std::thread::Builder::new()
            .name("decode".into())
            .spawn(move || decode(&path, start, end, fps, width, height, scale, &sender, &thread_stop, ring))
            .expect("decode thread");
        Stream { frames, pool, stop, current: None, next: None, fps, finished: false }
    }

    /// Maps a staging buffer again (once the GPU has copied from it) and returns it to the pool.
    pub fn recycle(&self, frame: &Frame) {
        if let (Pixels::Staged(buffer), Some(pool)) = (&frame.pixels, &self.pool) {
            let pool = pool.clone();
            let back = buffer.clone();
            buffer.map_async(wgpu::MapMode::Write, .., move |result| {
                if result.is_ok() {
                    pool.put_back(back);
                } else {
                    pool.forget();
                }
            });
        }
    }

    fn release(&self, frame: Frame) {
        self.recycle(&frame);
    }

    /// Moves to the frame nearest `time` that has been decoded. Returns whether the frame on
    /// screen changed, and counts the decoded frames passed over without being shown.
    pub fn advance(&mut self, time: f64, skipped: &mut u64) -> bool {
        let half = 0.5 / self.fps;
        let mut changed = false;
        loop {
            if self.next.is_none() {
                match self.frames.try_recv() {
                    Ok(frame) => self.next = Some(frame),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.finished = true;
                        break;
                    }
                }
            }
            let due = self.next.as_ref().is_some_and(|n| n.time <= time + half) || self.current.is_none();
            if !due {
                break;
            }
            if changed {
                *skipped += 1;
            }
            if let Some(old) = std::mem::replace(&mut self.current, self.next.take()) {
                self.release(old);
            }
            changed = true;
        }
        changed
    }

    /// Whether the next frame for `time` is not decoded yet (the decoder is behind).
    pub fn starved(&self, time: f64) -> bool {
        !self.finished && self.next.is_none() && self.current.as_ref().is_none_or(|c| c.time + 1.5 / self.fps < time)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// A stream's staging buffers: the mapped, free ones, and how many exist.
pub struct Pool {
    free: Mutex<(Vec<wgpu::Buffer>, usize)>,
    returned: Condvar,
    limit: usize,
}

impl Pool {
    fn put_back(&self, buffer: wgpu::Buffer) {
        if let Ok(mut free) = self.free.lock() {
            free.0.push(buffer);
        }
        self.returned.notify_all();
    }

    fn forget(&self) {
        if let Ok(mut free) = self.free.lock() {
            free.1 -= 1;
        }
        self.returned.notify_all();
    }

    /// A mapped buffer, new while fewer than `limit` exist, else waited for (polling the device
    /// runs the map callbacks). None once `stop` is set.
    fn take(&self, device: &wgpu::Device, size: u64, stop: &AtomicBool) -> Option<wgpu::Buffer> {
        loop {
            {
                let mut free = self.free.lock().ok()?;
                if let Some(buffer) = free.0.pop() {
                    return Some(buffer);
                }
                if free.1 < self.limit {
                    free.1 += 1;
                    return Some(device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("decoded frame"),
                        size,
                        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: true,
                    }));
                }
            }
            if stop.load(Ordering::SeqCst) {
                return None;
            }
            let _ = device.poll(wgpu::PollType::Poll);
            let free = self.free.lock().ok()?;
            let _ = self.returned.wait_timeout(free, Duration::from_millis(2));
        }
    }
}

/// Rows of a buffer-to-texture copy are padded to 256 bytes.
pub fn padded_row(width: u32) -> u32 {
    (width * 4).div_ceil(256) * 256
}

#[allow(clippy::too_many_arguments)]
fn decode(
    path: &str,
    start: f64,
    end: f64,
    fps: f64,
    width: i32,
    height: i32,
    scale: bool,
    sender: &SyncSender<Frame>,
    stop: &AtomicBool,
    ring: Option<(GpuSink, Arc<Pool>)>,
) {
    let mut decoder = FfmpegDecoder::new();
    let mut error = String::new();
    if !decoder.start(path, start, (end - start).max(1.0 / fps), fps, width, height, scale, &mut error) {
        eprintln!("decode {path}: {error}");
        return;
    }
    let (w, h) = (width as u32, height as u32);
    let (row, padded) = (w as usize * 4, padded_row(w) as usize);
    let mut index = 0u64;
    while !stop.load(Ordering::SeqCst) {
        let Some(pixels) = decoder.read_frame(&mut error) else { break };
        let time = start + index as f64 / fps;
        index += 1;
        let mut frame = match &ring {
            Some((gpu, pool)) => {
                let Some(buffer) = pool.take(&gpu.device, (padded * h as usize) as u64, stop) else { return };
                {
                    let Ok(mut mapped) = buffer.get_mapped_range_mut(..) else { return };
                    if row == padded {
                        mapped.copy_from_slice(&pixels);
                    } else {
                        for (y, line) in pixels.chunks_exact(row).enumerate() {
                            mapped.slice(y * padded..y * padded + row).copy_from_slice(line);
                        }
                    }
                }
                buffer.unmap();
                let cpu_copy = gpu.keep_cpu_copy.then_some(pixels);
                Frame { time, width: w, height: h, pixels: Pixels::Staged(buffer), cpu_copy }
            }
            None => Frame { time, width: w, height: h, pixels: Pixels::Cpu(pixels), cpu_copy: None },
        };
        // Wait for room, but notice a stop.
        loop {
            match sender.try_send(frame) {
                Ok(()) => break,
                Err(TrySendError::Full(back)) => {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    frame = back;
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
    }
}
