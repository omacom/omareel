//! Playback decoding for the preview: one thread per video running omareel_core's
//! `FfmpegDecoder` (ffmpeg to raw RGBA over a pipe) a few frames ahead of the playhead, and
//! starting again wherever the playhead jumps.

use crate::scene::VideoFrame;
use omareel_core::export::decoder::FfmpegDecoder;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

const AHEAD: usize = 6;
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
            .spawn(move || decode(&worker, &path, fps, width, height, duration))
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

fn decode(shared: &Shared, path: &str, fps: f64, width: i32, height: i32, duration: f64) {
    let mut decoder = FfmpegDecoder::new();
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
        let mut error = String::new();
        if !decoder.start(path, start_time, (duration - start_time).max(1.0 / fps), fps, width, height, false, &mut error) {
            eprintln!("decoder: {error}");
            shared.state.lock().unwrap().ended = true;
            continue;
        }
        let mut index = start;
        loop {
            let Some(pixels) = decoder.read_frame(&mut error) else {
                if !error.is_empty() {
                    eprintln!("decoder: {error}");
                }
                shared.state.lock().unwrap().ended = true;
                break;
            };
            let frame = VideoFrame {
                id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                width: width as u32,
                height: height as u32,
                pixels: Arc::new(pixels),
            };
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
        decoder.cancel();
    }
}
