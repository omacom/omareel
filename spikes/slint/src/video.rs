//! The preview's decoding: one thread per video running omareel_core's `FfmpegDecoder` (the
//! export's ffmpeg command, RGBA on a pipe) from a start time, a few frames ahead of the playhead.

use omareel_core::export::decoder::FfmpegDecoder;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::time::Duration;

pub struct Frame {
    /// Seconds in the video.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A video decoding from one point on.
pub struct Stream {
    frames: Receiver<Frame>,
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
    pub fn start(path: &str, start: f64, end: f64, fps: f64, width: i32, height: i32, scale: bool, ahead: usize) -> Stream {
        let (sender, frames) = sync_channel(ahead);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("decode".into())
            .spawn(move || decode(&path, start, end, fps, width, height, scale, &sender, &thread_stop))
            .expect("decode thread");
        Stream { frames, stop, current: None, next: None, fps, finished: false }
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
            self.current = self.next.take();
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
) {
    let mut decoder = FfmpegDecoder::new();
    let mut error = String::new();
    if !decoder.start(path, start, (end - start).max(1.0 / fps), fps, width, height, scale, &mut error) {
        eprintln!("decode {path}: {error}");
        return;
    }
    let mut index = 0u64;
    while !stop.load(Ordering::SeqCst) {
        let Some(pixels) = decoder.read_frame(&mut error) else { break };
        let mut frame = Frame { time: start + index as f64 / fps, width: width as u32, height: height as u32, pixels };
        index += 1;
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
