//! `ScreenCapture::Private::writerLoop` and what it shares with the capture: the ring of capture
//! slots behind a mutex, the condition the writer waits on, and the counters the capture reads.
//!
//! The capture side acquires a slot ([`FrameQueue::acquire`]), has the compositor copy a frame
//! into that slot's memory, and queues it with its presentation time
//! ([`FrameQueue::mark_queued`]). The writer thread runs [`writer_loop`]: it writes the Matroska
//! stream header, then each queued frame (cropped through the row copies) at its tick on the
//! encoder's 1/fps grid, skipping a frame that lands on the tick before it, and keeps the last
//! frame written in its slot so that at stop it can be written again at the stop time (the
//! compositor only sends frames when the screen changes). The first frame's presentation time goes
//! into the timestamp file. A failure stops the writer and is kept for `captureFrame` to report.
//!
//! Frames go to a [`FrameOutput`]: the Matroska stream to ffmpeg ([`writer_loop`]), whose input is
//! an [`EncoderSink`] ([`FdSink`] for ffmpeg's pipe) reading the slots' pixels from a
//! [`SlotMemory`], so the writer can be driven without a compositor; or the VA-API encoder, which
//! takes the slots' dmabufs by index ([`run_writer`]).

use super::{CaptureRingBookkeeping, CaptureRowCopy, Size, SlotState, capture_frame_header, capture_stream_header};
use omareel_core::compat::qsavefile::SaveFile;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The C library's message for an `errno`, as `QString::fromLocal8Bit(std::strerror(errno))`
/// gives it in the C locale.
pub(crate) fn strerror(code: i32) -> String {
    let mut buffer = [0 as libc::c_char; 256];
    // SAFETY: strerror_r writes a NUL-terminated message of at most `buffer.len()` bytes.
    let result = unsafe { libc::strerror_r(code, buffer.as_mut_ptr(), buffer.len()) };
    if result != 0 {
        return format!("Unknown error {code}");
    }
    // SAFETY: the buffer holds a NUL-terminated string.
    unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy().into_owned()
}

/// The encoder's standard input as the writer uses it: single writes and a wait until it takes
/// more.
pub trait EncoderSink {
    /// One `write(2)`: the bytes taken, or the `errno`.
    fn write(&mut self, bytes: &[u8]) -> Result<usize, i32>;
    /// `poll(POLLOUT)` for up to `timeout_ms`: the number of ready descriptors, or the `errno`.
    fn wait_writable(&mut self, timeout_ms: i32) -> Result<i32, i32>;
}

/// A file descriptor (ffmpeg's pipe, non-blocking, or a file); -1 makes every write fail with
/// EBADF. The descriptor is not closed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdSink(pub RawFd);

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

impl EncoderSink for FdSink {
    fn write(&mut self, bytes: &[u8]) -> Result<usize, i32> {
        // SAFETY: `bytes` is valid for its length; a bad descriptor only makes write fail.
        let count = unsafe { libc::write(self.0, bytes.as_ptr().cast(), bytes.len()) };
        if count < 0 { Err(errno()) } else { Ok(count as usize) }
    }

    fn wait_writable(&mut self, timeout_ms: i32) -> Result<i32, i32> {
        let mut descriptor = libc::pollfd { fd: self.0, events: libc::POLLOUT, revents: 0 };
        // SAFETY: one pollfd.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if result < 0 { Err(errno()) } else { Ok(result) }
    }
}

/// `writeAll`: every byte, retrying after EINTR and waiting (100 ms at a time) while the pipe is
/// full, until `abort` is set or ten seconds pass without progress. `Err` holds the message.
pub fn write_all<S: EncoderSink + ?Sized>(sink: &mut S, bytes: &[u8], abort: &AtomicBool) -> Result<(), String> {
    let mut written = 0;
    let mut stall = Instant::now();
    while written < bytes.len() && !abort.load(Ordering::Relaxed) {
        match sink.write(&bytes[written..]) {
            Ok(count) if count > 0 => {
                written += count;
                stall = Instant::now();
                continue;
            }
            Err(libc::EINTR) => continue,
            Err(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => {
                let result = sink.wait_writable(100);
                if result.is_ok() && stall.elapsed() < Duration::from_millis(10000) {
                    continue;
                }
                return Err(match result {
                    Err(code) => strerror(code),
                    Ok(_) => "Encoder input stalled for ten seconds".into(),
                });
            }
            // A write that took nothing leaves the errno of an earlier call, as in the C++.
            Ok(_) => return Err(strerror(errno())),
            Err(code) => return Err(strerror(code)),
        }
    }
    if written == bytes.len() {
        return Ok(());
    }
    Err("Encoder input was interrupted".into())
}

/// The capture slots' pixels, `stride * height` bytes each. A slot is only read while the ring
/// says it is being written, and only filled while it is capturing.
pub trait SlotMemory: Send + Sync {
    /// Calls `read` with slot `index`'s bytes; false when they can't be read (and `read` isn't
    /// called).
    fn with_slot(&self, index: usize, read: &mut dyn FnMut(&[u8])) -> bool;
}

/// Slots in one block of memory the caller owns, such as the shared-memory pool the compositor
/// copies frames into.
pub struct RawSlotMemory {
    base: *const u8,
    slot_bytes: usize,
    slots: usize,
}

// SAFETY: the memory is only read, and only for slots the ring hands the writer.
unsafe impl Send for RawSlotMemory {}
// SAFETY: as for Send.
unsafe impl Sync for RawSlotMemory {}

impl RawSlotMemory {
    /// # Safety
    /// `base` must point at `slot_bytes * slots` readable bytes that stay mapped for as long as
    /// this value is used.
    pub unsafe fn new(base: *const u8, slot_bytes: usize, slots: usize) -> RawSlotMemory {
        RawSlotMemory { base, slot_bytes, slots }
    }
}

impl SlotMemory for RawSlotMemory {
    fn with_slot(&self, index: usize, read: &mut dyn FnMut(&[u8])) -> bool {
        if index >= self.slots {
            return false;
        }
        // SAFETY: `new`'s contract covers every slot below `slots`.
        read(unsafe { std::slice::from_raw_parts(self.base.add(index * self.slot_bytes), self.slot_bytes) });
        true
    }
}

/// What the writer needs to know about the capture: `config.fps` (at least 1),
/// `config.timestampPath`, `encodedSize` and `cropRows`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterConfig {
    pub fps: i32,
    pub timestamp_path: String,
    pub encoded_size: Size,
    pub crop_rows: Vec<CaptureRowCopy>,
}

/// What `ringMutex` guards.
struct Shared {
    ring: CaptureRingBookkeeping,
    /// Each slot's `presentationUs`, set when it is queued.
    presentation_us: Vec<i64>,
    writer_stopping: bool,
    writer_error: String,
    /// `stopUs`: when the capture stopped, 0 until it has.
    stop_us: i64,
}

/// The state `ScreenCapture::Private` shares between the capture and the writer thread.
pub struct FrameQueue {
    shared: Mutex<Shared>,
    queued_frame: Condvar,
    writer_abort: AtomicBool,
    writer_failed: AtomicBool,
    first_us: AtomicI64,
    frames: AtomicI32,
    /// On the 1/fps grid, including the stop repeat; -1 before the first frame.
    last_frame_index: AtomicI64,
}

impl FrameQueue {
    /// A ring of `slot_count` slots (the capture uses 5: one holds the last frame written, one is
    /// being written, and the rest take captures and queue them).
    pub fn new(slot_count: i32) -> FrameQueue {
        let ring = CaptureRingBookkeeping::new(slot_count);
        let slots = slot_count.max(1) as usize;
        FrameQueue {
            shared: Mutex::new(Shared {
                ring,
                presentation_us: vec![0; slots],
                writer_stopping: false,
                writer_error: String::new(),
                stop_us: 0,
            }),
            queued_frame: Condvar::new(),
            writer_abort: AtomicBool::new(false),
            writer_failed: AtomicBool::new(false),
            first_us: AtomicI64::new(0),
            frames: AtomicI32::new(0),
            last_frame_index: AtomicI64::new(-1),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A slot to capture into (`ring.acquire()` under the lock); -1 and a drop without one.
    pub fn acquire(&self) -> i32 {
        self.lock().ring.acquire()
    }

    /// Hands a captured slot to the writer with its presentation time and wakes it; false
    /// when the slot was not capturing (`Capture slot state became inconsistent`).
    pub fn mark_queued(&self, slot: i32, presentation_us: i64) -> bool {
        let queued = {
            let mut shared = self.lock();
            if let Some(time) = usize::try_from(slot).ok().and_then(|index| shared.presentation_us.get_mut(index)) {
                *time = presentation_us;
            }
            shared.ring.mark_queued(slot)
        };
        if queued {
            self.queued_frame.notify_one();
        }
        queued
    }

    /// Gives a slot back (a failed capture, or one abandoned at stop).
    pub fn release(&self, slot: i32) -> bool {
        self.lock().ring.release(slot)
    }

    /// How many slots are in `state`.
    pub fn count(&self, state: SlotState) -> i32 {
        self.lock().ring.count(state)
    }

    /// The ring as it is now.
    pub fn ring(&self) -> CaptureRingBookkeeping {
        self.lock().ring.clone()
    }

    /// `ring.drops()`.
    pub fn drops(&self) -> i32 {
        self.lock().ring.drops()
    }

    /// `writerAbort`: the writer stops writing at once (`stopEncoder(false)` sets it).
    pub fn set_writer_abort(&self) {
        self.writer_abort.store(true, Ordering::Relaxed);
    }

    /// `stopUs`, set by `finish` before the writer is stopped.
    pub fn set_stop_us(&self, stop_us: i64) {
        self.lock().stop_us = stop_us;
    }

    /// What `stopEncoder` does to the writer before joining it: `drain` false aborts it first;
    /// then `writerStopping` is set and the writer woken.
    pub fn stop_writer(&self, drain: bool) {
        if !drain {
            self.set_writer_abort();
        }
        self.lock().writer_stopping = true;
        self.queued_frame.notify_all();
    }

    /// `writerFailed`.
    pub fn writer_failed(&self) -> bool {
        self.writer_failed.load(Ordering::Relaxed)
    }

    /// `writerError`: why the writer stopped, empty while it has not failed.
    pub fn writer_error(&self) -> String {
        self.lock().writer_error.clone()
    }

    /// `firstFrameUs()`: the first frame's presentation time, 0 before it.
    pub fn first_frame_us(&self) -> i64 {
        self.first_us.load(Ordering::Relaxed)
    }

    /// `encodedFrames()`: the frames written (not counting the stop repeat).
    pub fn encoded_frames(&self) -> i32 {
        self.frames.load(Ordering::Relaxed)
    }

    /// `lastFrameIndex`.
    pub fn last_frame_index(&self) -> i64 {
        self.last_frame_index.load(Ordering::Relaxed)
    }

    /// `ScreenCapture::videoDurationUs`.
    pub fn video_duration_us(&self, fps: i32) -> i64 {
        super::video_duration_us(self.last_frame_index(), fps)
    }

    /// Records a failure: the slot (if any) released, the message kept, the writer stopping
    /// when `stopping`.
    fn fail(&self, release: Option<i32>, error: String, stopping: bool) {
        let mut shared = self.lock();
        if let Some(slot) = release {
            shared.ring.release(slot);
        }
        shared.writer_error = error;
        self.writer_failed.store(true, Ordering::Relaxed);
        if stopping {
            shared.writer_stopping = true;
        }
    }
}

/// `writeTimestamp`: `monotonic_microsec\trealtime_microsec\n<monotonic>\t<realtime>\n`
/// through a `QSaveFile`, the realtime clock read as the line is written.
pub fn write_timestamp(path: &str, monotonic: i64) -> Result<(), String> {
    let mut file = SaveFile::new(path);
    if !file.open() {
        return Err(file.error_string());
    }
    let text = format!("monotonic_microsec\trealtime_microsec\n{monotonic}\t{}\n", realtime_us());
    file.write(text.as_bytes());
    if !file.commit() {
        return Err(file.error_string());
    }
    Ok(())
}

/// `CLOCK_REALTIME` in microseconds.
fn realtime_us() -> i64 {
    let mut value = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes a timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut value) };
    value.tv_sec * 1_000_000 + value.tv_nsec / 1000
}

/// Where the writer puts frames: the Matroska stream piped to ffmpeg ([`writer_loop`]), or the
/// GPU encoder (`super::gpu`).
pub trait FrameOutput {
    /// Before the first frame.
    fn begin(&mut self) -> Result<(), String> {
        Ok(())
    }
    /// Slot `index`'s frame at `frame_index` on the encoder's 1/fps grid.
    fn write(&mut self, index: usize, frame_index: i64) -> Result<(), String>;
    /// After the last frame of a writer that was neither aborted nor failed.
    fn end(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// The Matroska stream ffmpeg reads: the header, then each frame cropped through the row copies.
struct MatroskaOutput<'a, S: EncoderSink + ?Sized, M: SlotMemory + ?Sized> {
    fps: f64,
    header: Vec<u8>,
    rows: &'a [CaptureRowCopy],
    cropped: Vec<u8>,
    memory: &'a M,
    sink: &'a mut S,
    abort: &'a AtomicBool,
}

impl<S: EncoderSink + ?Sized, M: SlotMemory + ?Sized> FrameOutput for MatroskaOutput<'_, S, M> {
    fn begin(&mut self) -> Result<(), String> {
        write_all(self.sink, &self.header, self.abort)
    }

    fn write(&mut self, index: usize, frame_index: i64) -> Result<(), String> {
        let MatroskaOutput { fps, rows, cropped, memory, sink, abort, .. } = self;
        let mut result = Ok(());
        let read = memory.with_slot(index, &mut |bytes: &[u8]| {
            let Some(first) = rows.first() else { return };
            let output: &[u8] = if rows.len() > 1 {
                for row in rows.iter() {
                    let (from, to, count) =
                        (row.source_offset as usize, row.destination_offset as usize, row.bytes as usize);
                    cropped[to..to + count].copy_from_slice(&bytes[from..from + count]);
                }
                cropped
            } else {
                &bytes[first.source_offset as usize..(first.source_offset + first.bytes) as usize]
            };
            let frame_header = capture_frame_header((frame_index as f64 * 1e6 / *fps).round() as i64, output.len() as i64);
            result = write_all(*sink, &frame_header, abort).and_then(|()| write_all(*sink, output, abort));
        });
        if !read {
            return Err("A captured frame could not be read".into());
        }
        result
    }
}

/// `ScreenCapture::Private::writerLoop`, run on the writer thread until the queue is stopped
/// ([`FrameQueue::stop_writer`]) or a write fails: the Matroska stream to ffmpeg.
pub fn writer_loop<S: EncoderSink + ?Sized, M: SlotMemory + ?Sized>(
    queue: &FrameQueue,
    config: &WriterConfig,
    memory: &M,
    sink: &mut S,
) {
    let rows = &config.crop_rows;
    let mut cropped = Vec::new();
    if rows.len() > 1 {
        cropped.resize(
            config.encoded_size.width.wrapping_mul(config.encoded_size.height).wrapping_mul(4).max(0) as usize,
            0u8,
        );
    }
    let mut output = MatroskaOutput {
        fps: f64::from(config.fps),
        header: capture_stream_header(config.encoded_size, config.fps),
        rows,
        cropped,
        memory,
        sink,
        abort: &queue.writer_abort,
    };
    run_writer(queue, config, &mut output);
}

/// The writer's loop over any [`FrameOutput`]: each queued frame at its tick on the encoder's
/// 1/fps grid, skipping a frame that lands on the tick before it, the last frame written kept in
/// its slot so it can be written again at the stop time, the first frame's presentation time
/// in the timestamp file. A failure is kept for `capture_frame` to report.
pub fn run_writer<O: FrameOutput + ?Sized>(queue: &FrameQueue, config: &WriterConfig, output: &mut O) {
    let fps = f64::from(config.fps);
    if let Err(error) = output.begin() {
        queue.fail(None, error, true);
        return;
    }
    let mut write_frame = |index: usize, frame_index: i64| output.write(index, frame_index);
    // The compositor only sends frames when the screen changes, so the last one written stays in
    // its slot until the next replaces it. At stop it is written again at the stop time, or a
    // recording that ends on a still screen would end at its last change.
    let mut held_index: i32 = -1;
    loop {
        let (index, presentation_us) = {
            let shared = queue.lock();
            let mut shared = queue
                .queued_frame
                .wait_while(shared, |s| !(s.writer_stopping || s.ring.count(SlotState::Queued) > 0))
                .unwrap_or_else(PoisonError::into_inner);
            let index = shared.ring.take_queued();
            if index < 0 && shared.writer_stopping {
                break;
            }
            let presentation = if index >= 0 { shared.presentation_us[index as usize] } else { 0 };
            (index, presentation)
        };
        if index < 0 {
            continue;
        }
        if queue.first_us.load(Ordering::Relaxed) == 0 {
            queue.first_us.store(presentation_us, Ordering::Relaxed);
            if let Err(error) = write_timestamp(&config.timestamp_path, presentation_us) {
                queue.fail(Some(index), error, false);
                break;
            }
        }
        // Frames keep their capture time on the encoder's 1/fps grid. A second capture within
        // one interval would land on the same tick as the first, so it is skipped.
        let first_us = queue.first_us.load(Ordering::Relaxed);
        let frame_index = ((presentation_us - first_us) as f64 * fps / 1e6).round() as i64;
        if frame_index <= queue.last_frame_index.load(Ordering::Relaxed) {
            queue.lock().ring.release(index);
            continue;
        }
        let wrote = write_frame(index as usize, frame_index);
        {
            let mut shared = queue.lock();
            if held_index >= 0 {
                shared.ring.release(held_index);
            }
            held_index = if wrote.is_ok() { index } else { -1 };
            if let Err(error) = &wrote {
                shared.ring.release(index);
                shared.writer_error = error.clone();
                queue.writer_failed.store(true, Ordering::Relaxed);
                shared.writer_stopping = true;
            }
        }
        if wrote.is_err() {
            break;
        }
        queue.last_frame_index.store(frame_index, Ordering::Relaxed);
        queue.frames.fetch_add(1, Ordering::Relaxed);
    }
    if held_index >= 0 {
        let stop_us = queue.lock().stop_us;
        let first_us = queue.first_us.load(Ordering::Relaxed);
        let stop_index = if stop_us > 0 { ((stop_us - first_us) as f64 * fps / 1e6).round() as i64 } else { -1 };
        let mut padded = Ok(());
        if stop_index > queue.last_frame_index.load(Ordering::Relaxed)
            && !queue.writer_abort.load(Ordering::Relaxed)
            && !queue.writer_failed.load(Ordering::Relaxed)
        {
            padded = write_frame(held_index as usize, stop_index);
            if padded.is_ok() {
                queue.last_frame_index.store(stop_index, Ordering::Relaxed);
            }
        }
        let mut shared = queue.lock();
        shared.ring.release(held_index);
        if let Err(error) = padded {
            shared.writer_error = error;
            queue.writer_failed.store(true, Ordering::Relaxed);
        }
    }
    if queue.writer_abort.load(Ordering::Relaxed) || queue.writer_failed.load(Ordering::Relaxed) {
        return;
    }
    if let Err(error) = output.end() {
        queue.fail(None, error, true);
    }
}
