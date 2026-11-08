//! The preview's decoding: one thread per video running ffmpeg from a start time, a few frames
//! ahead of the playhead.
//!
//! Two ways to the GPU, picked with `--upload`:
//! - `surface` (default): ffmpeg hands over NV12 (1.5 bytes a pixel, no colour conversion on the
//!   CPU), which goes to a `rune::NativeSurface`; Rune uploads both planes and converts to RGB in
//!   its video shader. With `--decode vaapi` ffmpeg decodes on the GPU's video engine and copies
//!   the NV12 frame back to memory, so the CPU only moves bytes.
//! - `pixels`: omareel_core's `FfmpegDecoder` (RGBA, the export's decoder) into a `rune::Pixels`,
//!   which Rune uploads into its image pages.

use omareel_core::export::decoder::FfmpegDecoder;
use rune::native_surface::{ByteFrame, ColorInfo, Format, Matrix, Transfer};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upload {
    Surface,
    Pixels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decode {
    Software,
    Vaapi(String),
}

impl Decode {
    /// VA-API where omareel's export would use it (an Intel or AMD render node), else software.
    pub fn auto() -> Decode {
        omareel_core::export::hardware::vaapi_device().map_or(Decode::Software, Decode::Vaapi)
    }
}

pub enum Pixels {
    Nv12(ByteFrame),
    Rgba(Vec<u8>),
}

pub struct Frame {
    /// Seconds in the video.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    pub pixels: Pixels,
}

/// The matrix ffmpeg would use for this file: the tagged one, or BT.601 for an untagged file (as
/// swscale assumes, and as omareel's decoder is told for old recordings).
pub fn color_info(path: &str) -> ColorInfo {
    let tagged = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=color_space", "-of", "csv=p=0", path])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let matrix = if tagged.starts_with("bt709") { Matrix::Bt709 } else { Matrix::Bt601 };
    ColorInfo { matrix, transfer: Transfer::Bt709 }
}

/// A video decoding from one point on.
pub struct Stream {
    frames: Receiver<Frame>,
    stop: Arc<AtomicBool>,
    /// The frame on screen (already handed over) and the next one.
    pub current_time: Option<f64>,
    next: Option<Frame>,
    pub fps: f64,
    pub finished: bool,
}

pub struct StreamSpec<'a> {
    pub path: &'a str,
    pub start: f64,
    pub end: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub ahead: usize,
    pub upload: Upload,
    pub decode: Decode,
    pub color: ColorInfo,
}

impl Stream {
    pub fn start(spec: StreamSpec<'_>) -> Stream {
        let (sender, frames) = sync_channel(spec.ahead);
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let fps = spec.fps;
        let job = Job {
            path: spec.path.to_owned(),
            start: spec.start,
            length: (spec.end - spec.start).max(1.0 / spec.fps),
            fps: spec.fps,
            width: spec.width,
            height: spec.height,
            upload: spec.upload,
            decode: spec.decode,
            color: spec.color,
        };
        std::thread::Builder::new().name("decode".into()).spawn(move || job.run(&sender, &thread_stop)).expect("decode thread");
        Stream { frames, stop, current_time: None, next: None, fps, finished: false }
    }

    /// The newest decoded frame due at `time`, if it is not the one on screen. Counts the frames
    /// passed over without being shown.
    pub fn advance(&mut self, time: f64, skipped: &mut u64) -> Option<Frame> {
        let half = 0.5 / self.fps;
        let mut due: Option<Frame> = None;
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
            let ready = self.next.as_ref().is_some_and(|n| n.time <= time + half) || (self.current_time.is_none() && due.is_none());
            if !ready {
                break;
            }
            if due.is_some() {
                *skipped += 1;
            }
            due = self.next.take();
        }
        if let Some(frame) = &due {
            self.current_time = Some(frame.time);
        }
        due
    }

    /// Blocks until the frame for `time` is decoded (or the stream ends), for headless runs,
    /// where the clock is virtual and every frame must show its own picture.
    pub fn wait_for(&mut self, time: f64) {
        let half = 0.5 / self.fps;
        loop {
            if self.finished || self.current_time.is_some_and(|c| c + half >= time) {
                return;
            }
            if self.next.as_ref().is_some_and(|n| n.time + half >= time) {
                return;
            }
            match self.frames.recv_timeout(Duration::from_secs(10)) {
                // An older pending frame is passed over.
                Ok(frame) => self.next = Some(frame),
                Err(_) => {
                    self.finished = true;
                    return;
                }
            }
        }
    }

    /// Whether the frame for `time` is not decoded yet (the decoder is behind).
    pub fn starved(&self, time: f64) -> bool {
        !self.finished && self.next.is_none() && self.current_time.is_none_or(|c| c + 1.5 / self.fps < time)
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

struct Job {
    path: String,
    start: f64,
    length: f64,
    fps: f64,
    width: u32,
    height: u32,
    upload: Upload,
    decode: Decode,
    color: ColorInfo,
}

impl Job {
    fn run(self, sender: &SyncSender<Frame>, stop: &AtomicBool) {
        match self.upload {
            Upload::Surface => self.run_nv12(sender, stop),
            Upload::Pixels => self.run_rgba(sender, stop),
        }
    }

    fn spawn_nv12(&self, decode: &Decode) -> std::io::Result<Child> {
        let mut command = Command::new("ffmpeg");
        command.args(["-v", "error", "-nostdin"]);
        if let Decode::Vaapi(device) = decode {
            // Decoded on the video engine; ffmpeg copies the NV12 surface back to memory.
            command.args(["-hwaccel", "vaapi", "-hwaccel_device", device]);
        }
        let filter = format!("fps={}:start_time=0,scale={}:{}:flags=bilinear,format=nv12", self.fps, self.width, self.height);
        command
            .args(["-ss", &format!("{:.6}", self.start), "-noaccurate_seek", "-i", &self.path, "-t", &format!("{:.6}", self.length)])
            .args(["-vf", &filter, "-f", "rawvideo", "-pix_fmt", "nv12", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.spawn()
    }

    fn run_nv12(&self, sender: &SyncSender<Frame>, stop: &AtomicBool) {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut attempts = vec![self.decode.clone()];
        if self.decode != Decode::Software {
            attempts.push(Decode::Software);
        }
        for decode in attempts {
            let Ok(mut child) = self.spawn_nv12(&decode) else { continue };
            let mut out = child.stdout.take().expect("piped stdout");
            let mut index = 0u64;
            loop {
                if stop.load(Ordering::SeqCst) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                let mut y = vec![0u8; w * h];
                let mut uv = vec![0u8; w * h / 2];
                if out.read_exact(&mut y).is_err() || out.read_exact(&mut uv).is_err() {
                    break;
                }
                let time = self.start + index as f64 / self.fps;
                index += 1;
                let frame = Frame {
                    time,
                    width: self.width,
                    height: self.height,
                    pixels: Pixels::Nv12(ByteFrame {
                        width: self.width,
                        height: self.height,
                        format: Format::Nv12Video,
                        color: self.color,
                        planes: [y, uv],
                    }),
                };
                if !send(sender, frame, stop) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
            let _ = child.kill();
            let status = child.wait();
            if index > 0 {
                return;
            }
            let mut message = String::new();
            if let Some(mut err) = child.stderr.take() {
                let _ = err.read_to_string(&mut message);
            }
            eprintln!("decode {} ({decode:?}) gave no frames: {status:?} {}", self.path, message.trim());
        }
    }

    fn run_rgba(&self, sender: &SyncSender<Frame>, stop: &AtomicBool) {
        let mut decoder = FfmpegDecoder::new();
        let mut error = String::new();
        let scale = true;
        if !decoder.start(&self.path, self.start, self.length, self.fps, self.width as i32, self.height as i32, scale, false, &mut error) {
            eprintln!("decode {}: {error}", self.path);
            return;
        }
        let mut index = 0u64;
        while !stop.load(Ordering::SeqCst) {
            let Some(pixels) = decoder.read_frame(&mut error) else { break };
            let time = self.start + index as f64 / self.fps;
            index += 1;
            let frame = Frame { time, width: self.width, height: self.height, pixels: Pixels::Rgba(pixels) };
            if !send(sender, frame, stop) {
                return;
            }
        }
    }
}

/// Waits for room in the queue, noticing a stop. False when the stream is gone.
fn send(sender: &SyncSender<Frame>, mut frame: Frame, stop: &AtomicBool) -> bool {
    loop {
        match sender.try_send(frame) {
            Ok(()) => return true,
            Err(TrySendError::Full(back)) => {
                if stop.load(Ordering::SeqCst) {
                    return false;
                }
                frame = back;
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sound

/// One clip's part of the sound from the playhead on, in output order.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSegment {
    /// The file whose first audio stream plays; empty for silence (no audio, or a media clip
    /// whose file has none).
    pub path: String,
    /// Where in that file it starts and how much of it plays (source seconds).
    pub start: f64,
    pub length: f64,
    pub speed: f64,
    /// The recording's sound with neither desktop nor microphone kept (`Document::screen_audio`).
    pub muted: bool,
}

impl AudioSegment {
    /// Its length in the output.
    pub fn output_length(&self) -> f64 {
        self.length / self.speed.max(0.01)
    }
}

/// Where the sound goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioSink {
    /// `pw-cat --playback`: the speakers.
    Speakers,
    /// A WAV file (`OMAREEL_AUDIO=file:/path.wav`), to check what would play.
    File(String),
    /// Decoded and dropped: headless runs, and `OMAREEL_AUDIO=off`.
    Null,
}

impl AudioSink {
    /// `OMAREEL_AUDIO` (`off`, `file:<path>`), else the speakers in a window and nothing headless.
    pub fn from_env(headless: bool) -> AudioSink {
        match std::env::var("OMAREEL_AUDIO").unwrap_or_default().as_str() {
            "off" | "null" => AudioSink::Null,
            v if v.starts_with("file:") => AudioSink::File(v["file:".len()..].to_owned()),
            "speakers" => AudioSink::Speakers,
            _ if headless => AudioSink::Null,
            _ => AudioSink::Speakers,
        }
    }
}

const AUDIO_RATE: u32 = 48_000;

/// The recording's sound from one point on: an ffmpeg child decoding the clips' audio (each
/// clip's span at its speed, as the export mixes it) to 48 kHz stereo s16, copied by a thread
/// into `pw-cat`, which applies the volume on the way so a volume change needs no restart.
/// Dropping it stops both children at once.
pub struct Audio {
    ffmpeg: Option<Child>,
    sink: Option<Child>,
    stop: Arc<AtomicBool>,
    gain: Arc<std::sync::atomic::AtomicU32>,
    /// What it plays, to notice when an edit changes it.
    pub segments: Vec<AudioSegment>,
    /// The output time it starts at.
    pub output_start: f64,
}

/// ffmpeg's arguments for `segments` (silence where a segment has no file).
pub fn audio_args(segments: &[AudioSegment]) -> Vec<String> {
    use omareel_core::export::plan;
    let mut args: Vec<String> = ["-v", "error", "-nostdin"].map(String::from).into();
    let mut chains = Vec::new();
    let mut input = 0;
    for segment in segments {
        let out = segment.output_length();
        if segment.path.is_empty() {
            chains.push(plan::silent_segment(out));
            continue;
        }
        args.extend(["-ss".into(), format!("{:.6}", segment.start), "-t".into(), format!("{:.6}", segment.length), "-i".into(), segment.path.clone()]);
        let mut chain = plan::audio_segment(&format!("[{input}:a:0]asetpts=PTS-STARTPTS"), segment.speed, out);
        if segment.muted {
            chain += ",volume=0";
        }
        chains.push(chain);
        input += 1;
    }
    let mut filter = String::new();
    for (i, chain) in chains.iter().enumerate() {
        filter += &format!("{chain}[a{i}];");
    }
    filter += &format!("{}concat=n={}:v=0:a=1[out]", (0..chains.len()).map(|i| format!("[a{i}]")).collect::<String>(), chains.len());
    args.extend(["-filter_complex".into(), filter, "-map".into(), "[out]".into()]);
    args.extend(["-f", "s16le", "-ar", "48000", "-ac", "2", "-"].map(String::from));
    args
}

impl Audio {
    /// Starts the sound of `segments` (from `output_start` on the timeline) at `volume` (0 to 1).
    /// None when there is nothing to play or ffmpeg can't start.
    pub fn start(segments: Vec<AudioSegment>, output_start: f64, volume: f64, sink: &AudioSink) -> Option<Audio> {
        if segments.is_empty() || segments.iter().all(|s| s.path.is_empty() || s.muted) {
            return None;
        }
        let args = audio_args(&segments);
        let started = std::time::Instant::now();
        let mut ffmpeg = Command::new("ffmpeg").args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().ok()?;
        let mut out = ffmpeg.stdout.take()?;
        let mut sink_child = None;
        let mut writer: Box<dyn std::io::Write + Send> = match sink {
            AudioSink::Speakers => {
                let child = Command::new("pw-cat")
                    .args(["--playback", "--raw", "--format", "s16", "--rate", "48000", "--channels", "2", "--latency", "40ms"])
                    .args(["-P", "media.name=omareel preview,application.name=omareel", "-"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
                match child {
                    Ok(mut child) => {
                        let stdin = child.stdin.take();
                        sink_child = Some(child);
                        match stdin {
                            Some(stdin) => Box::new(stdin),
                            None => Box::new(std::io::sink()),
                        }
                    }
                    Err(e) => {
                        eprintln!("audio: pw-cat: {e}");
                        Box::new(std::io::sink())
                    }
                }
            }
            AudioSink::File(path) => match std::fs::File::create(path) {
                Ok(mut file) => {
                    // A WAV header with an open length (players read to the end).
                    let _ = file.write_all(&wav_header());
                    Box::new(file)
                }
                Err(_) => Box::new(std::io::sink()),
            },
            AudioSink::Null => Box::new(std::io::sink()),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let gain = Arc::new(std::sync::atomic::AtomicU32::new((volume as f32).to_bits()));
        let (thread_stop, thread_gain) = (stop.clone(), gain.clone());
        let summary: Vec<String> = segments.iter().map(|s| format!("{}@{:.3}+{:.3}x{}{}", if s.path.is_empty() { "silence" } else { std::path::Path::new(&s.path).file_name().and_then(|n| n.to_str()).unwrap_or("?") }, s.start, s.length, s.speed, if s.muted { " muted" } else { "" })).collect();
        eprintln!("audio: start at output {output_start:.3} ({sink:?}, volume {volume:.2}): {}", summary.join(", "));
        if std::env::var_os("OMAREEL_AUDIO_LOG").is_some() {
            eprintln!("audio: ffmpeg {}", args.join(" "));
        }
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                let mut buffer = vec![0u8; 4096];
                let mut first = true;
                let mut bytes = 0u64;
                loop {
                    if thread_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let n = match out.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n & !1,
                    };
                    if first {
                        first = false;
                        eprintln!("audio: first samples {:.0} ms after start", started.elapsed().as_secs_f64() * 1000.0);
                    }
                    let gain = f32::from_bits(thread_gain.load(Ordering::Relaxed));
                    if (gain - 1.0).abs() > 1e-4 {
                        for pair in buffer[..n].chunks_exact_mut(2) {
                            let v = i16::from_le_bytes([pair[0], pair[1]]) as f32 * gain;
                            pair.copy_from_slice(&(v.clamp(-32768.0, 32767.0) as i16).to_le_bytes());
                        }
                    }
                    bytes += n as u64;
                    if writer.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
                let _ = writer.flush();
                if std::env::var_os("OMAREEL_AUDIO_LOG").is_some() {
                    eprintln!("audio: ended after {:.3} s of sound", bytes as f64 / f64::from(AUDIO_RATE * 4));
                }
            })
            .ok()?;
        Some(Audio { ffmpeg: Some(ffmpeg), sink: sink_child, stop, gain, segments, output_start })
    }

    /// The volume, at once (0 to 1).
    pub fn set_volume(&self, volume: f64) {
        self.gain.store((volume as f32).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        for child in [self.ffmpeg.take(), self.sink.take()].into_iter().flatten() {
            let mut child = child;
            let _ = child.kill();
            // Reaped off the UI thread.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        eprintln!("audio: stop");
    }
}

/// A 48 kHz stereo s16 WAV header whose lengths say "to the end of the file".
fn wav_header() -> Vec<u8> {
    let mut h = Vec::with_capacity(44);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&u32::MAX.to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes());
    h.extend_from_slice(&2u16.to_le_bytes());
    h.extend_from_slice(&AUDIO_RATE.to_le_bytes());
    h.extend_from_slice(&(AUDIO_RATE * 4).to_le_bytes());
    h.extend_from_slice(&4u16.to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&(u32::MAX - 36).to_le_bytes());
    h
}
