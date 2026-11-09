//! The editor's preview: a Rune window playing the composition with play/pause, a scrub bar and
//! the time.
//!
//! Rune's `poll` hook runs at the start of every frame. While playing it takes the clock, asks
//! the `Document` for the composition at that time (`preview_composition`, the editor's own
//! per-frame state), hands the decoded screen and camera frames for it to their surfaces, and asks
//! for the next frame with `wake_after(0)`. The view then rebuilds the composition from that state
//! and Rune diffs it.

use crate::comp::{self, FrameState, Media, Settings, Video};
use crate::theme::Ui;
use crate::video::{self, Decode, Stream, StreamSpec, Upload};
use omareel_core::editor::{Document, Probes};
use omareel_core::media_probe;
use rune::native_surface::ColorInfo;
use rune::tok::{Family, TextStyle};
use rune::{App, AppSpec, Cursor, Cx, El, Key, KeyEvent, NativeSurface, Pixels, Tx, col, el, row, text};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct Options {
    pub bundle: String,
    pub upload: Upload,
    pub decode: Decode,
    /// The canvas's tallest size in points (the Qt editor's 620); 0 for the whole pane.
    pub canvas_height: f32,
    /// Decode the screen at this height (scaled by ffmpeg); 0 for the recording's size.
    pub decode_height: i32,
    /// Play from the start after 1.5 s, for this many seconds, then write `stats` and quit.
    pub measure: Option<f64>,
    pub stats: Option<String>,
    /// The editor around the preview (`editor` subcommand): inspector and timeline.
    pub editor: bool,
    /// Rune's own flags (`--snap`, `--script`, `--video`, `--tree`, ...), after `--`.
    pub rune_args: Vec<String>,
}

impl Options {
    /// `edit <bundle>`: the editor with the defaults (Rune's flags may follow `--`).
    pub fn editor(bundle: &str) -> Options {
        let args: Vec<String> = std::env::args().collect();
        Options {
            bundle: bundle.to_owned(),
            upload: Upload::Surface,
            decode: Decode::auto(),
            canvas_height: 0.0,
            decode_height: 0,
            measure: None,
            stats: None,
            editor: true,
            rune_args: args.iter().position(|a| a == "--").map(|i| args[i + 1..].to_vec()).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Preview,
    Editor,
}

/// When each frame finished encoding, and whether it drew (`App::on_frame_encoded`, which has no
/// access to the state).
/// `SPIKE_EXPERIMENT=nocomp`: the window without the composition (only the chrome redraws).
static NO_COMPOSITION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static ENCODED: std::sync::Mutex<Vec<(Instant, bool)>> = std::sync::Mutex::new(Vec::new());

#[derive(Default)]
struct Stats {
    frames: Vec<Instant>,
    /// Time spent in `poll` per frame (clock, state, frame choice), ns.
    poll_ns: Vec<u64>,
    poll_started: Vec<Instant>,
    shown: u64,
    skipped: u64,
    starved: u64,
}

/// Decoders started ahead for the clip after the active one.
struct Prewarm {
    clip: i32,
    screen: Stream,
    camera: Option<Stream>,
}

pub struct Preview {
    pub(crate) mode: Mode,
    pub(crate) document: Document,
    pub(crate) settings: Settings,
    pub(crate) media: Media,
    pub(crate) frame: FrameState,
    pub(crate) kit: rune_kit::Kit,
    /// The motion track a worker is building (on open, and after an edit).
    pub(crate) motion: Option<std::sync::mpsc::Receiver<omareel_core::motion_track::MotionTrack>>,
    /// The screen's frame times, read by a worker on open.
    pub(crate) frame_times: Option<std::sync::mpsc::Receiver<Vec<f64>>>,
    /// The clip and source time the running decoders started from.
    decoders_from: Option<(i32, f64)>,
    /// The output time at the last step, and whether this step's differs.
    last_position: Option<f64>,
    jumped: bool,
    /// The editor's transient UI state.
    pub(crate) edit: crate::editor::EditState,
    /// Poll's clock: seconds since start (virtual in headless runs).
    pub(crate) now: f64,
    upload: Upload,
    decode: Decode,
    screen_color: ColorInfo,
    camera_color: ColorInfo,
    screen: Option<Stream>,
    camera: Option<Stream>,
    /// An inserted video's decoder (MediaLayer's player), the media it decodes, and whether it
    /// has shown a frame of it (`media_frame_ready`).
    insert: Option<Stream>,
    insert_media: String,
    insert_ready: bool,
    screen_size: (u32, u32),
    camera_path: String,
    canvas_limit: f32,
    pub(crate) playing: bool,
    /// The output time the clock started from, and when on poll's clock (None until the first
    /// frame is there).
    clock: Option<(f64, f64)>,
    origin: f64,
    pending_seek: Option<f64>,
    /// The clip the decoders play (`m_activeClip`).
    pub(crate) active_clip: i32,
    /// The next clip's decoders, started before a cut that jumps.
    next: Option<Prewarm>,
    /// The sound while playing, and the output time its plan ends at.
    audio: Option<video::Audio>,
    audio_end: f64,
    /// When the sound was started for a clock that waits for it.
    audio_wait: Option<f64>,
    audio_sink: video::AudioSink,
    last_restart: Instant,
    started: Instant,
    measure: Option<f64>,
    measure_started: Option<Instant>,
    stats: Stats,
    stats_path: Option<String>,
    pub(crate) ui: Ui,
    pub(crate) scrubbing: bool,
    /// Keeps the wallpaper and cursor images' decoder alive.
    pub(crate) cache: rune::ImageCache,
}

impl Preview {
    /// A screen decoder from `source` for the clip at `clip` (its speed picks how many frames a
    /// second are worth decoding: at 4× a 60 fps recording shows 15 of its frames a second, 60
    /// on screen).
    fn screen_stream(&self, clip: i32, source: f64) -> Stream {
        let (w, h) = self.screen_size;
        let ahead = if w * h > 2_500_000 { 4 } else { 8 };
        let speed = self.document.project().clips.get(clip.max(0) as usize).map_or(1.0, |c| c.speed).max(0.01);
        let fps = self.document.fps();
        Stream::start(StreamSpec {
            path: self.document.video_path(),
            start: source,
            end: self.document.source_duration(),
            fps: (60.0 / speed).min(fps).max(1.0),
            width: w,
            height: h,
            ahead,
            upload: self.upload,
            decode: self.decode.clone(),
            color: self.screen_color,
            preview: !self.playing,
        })
    }

    /// The camera's decoder for the screen at `source`, when there is a camera.
    fn camera_stream(&self, source: f64) -> Option<Stream> {
        let sync = self.document.camera_sync(source)?;
        let (cw, ch) = (self.document.camera_source_width() as u32, self.document.camera_source_height() as u32);
        let camera = self.document.recording().camera;
        Some(Stream::start(StreamSpec {
            path: &self.camera_path,
            start: sync.target_ms as f64 / 1000.0,
            end: camera.map_or(self.document.source_duration(), |c| c.duration),
            fps: camera.map_or(30.0, |c| c.fps),
            width: cw,
            height: ch,
            ahead: 6,
            upload: self.upload,
            // The camera is small; software decoding keeps the video engine for the screen.
            decode: Decode::Software,
            color: self.camera_color,
            preview: !self.playing,
        }))
    }

    fn restart_decoders(&mut self, output_time: f64) {
        self.active_clip = self.document.set_output_position(output_time);
        self.next = None;
        if self.document.clip_is_media(self.active_clip) {
            // An inserted video plays in its own player; the recording's decoders rest.
            self.decoders_from = None;
            self.screen = None;
            self.camera = None;
        } else {
            let source = self.document.source_position();
            // A click seeks on press and again on release: decoders already started there for
            // the same clip carry on rather than decoding from the keyframe a second time.
            let same = !self.playing
                && self.decoders_from == Some((self.active_clip, source))
                && self.screen.as_ref().is_some_and(|s| !s.finished && s.current_time.is_none_or(|c| c <= source + 1.0 / s.fps));
            if !same {
                self.screen = Some(self.screen_stream(self.active_clip, source));
                self.camera = self.camera_stream(source);
                self.decoders_from = Some((self.active_clip, source));
            }
        }
        self.last_restart = Instant::now();
    }

    /// `Editor::seek`: to `output_time` inside the playback range, ending a scrub. While playing
    /// the clock and the sound start again from there.
    pub(crate) fn seek(&mut self, output_time: f64) {
        if self.document.project().clips.is_empty() {
            return;
        }
        self.scrubbing = false;
        let t = self.document.clamp_playback_position(output_time);
        self.pending_seek = Some(t);
        self.active_clip = self.document.set_output_position(t);
        if self.playing {
            self.audio = None;
            self.audio_wait = None;
            self.clock = None;
            self.origin = t;
        }
        self.seek_now();
    }

    /// Paused, a seek starts its decoders in the handler that asked for it, as Qt's does, not
    /// at the next frame's poll: its picture comes a frame sooner. Scrubbing keeps the 35 ms
    /// spacing `step` gives restarts.
    fn seek_now(&mut self) {
        if let Some(target) = self.pending_seek
            && !self.playing
            && !rune::is_headless()
            && (self.last_restart.elapsed().as_secs_f64() > 0.035 || self.screen.is_none())
        {
            self.pending_seek = None;
            self.restart_decoders(target);
        }
    }

    /// `Editor::play`: from the range's start when outside it, from the start at the end.
    pub(crate) fn play(&mut self) {
        if self.playing || self.document.project().clips.is_empty() {
            return;
        }
        self.end_scrub();
        if let Some(target) = self.document.play_start_seek() {
            self.seek(target);
        }
        let position = self.document.output_position();
        self.origin = position;
        self.playing = true;
        self.clock = None;
        self.active_clip = self.document.set_output_position(position);
        // A paused decoder already holds the frames after the one on screen: play on from it
        // rather than starting ffmpeg again (which costs a few hundred ms, more with VA-API).
        let source = self.document.source_position();
        let warm = self.pending_seek.is_none()
            && !self.document.clip_is_media(self.active_clip)
            && self.screen.as_ref().is_some_and(|s| !s.finished && s.current_time.is_some_and(|c| (c - source).abs() <= 1.5 / s.fps));
        if !warm {
            self.pending_seek = None;
            self.restart_decoders(position);
        }
    }

    /// `Editor::pause`.
    pub(crate) fn pause(&mut self) {
        self.playing = false;
        self.clock = None;
        self.audio = None;
        self.audio_wait = None;
        self.next = None;
    }

    pub(crate) fn toggle(&mut self) {
        if self.playing { self.pause() } else { self.play() }
    }

    /// `Editor::beginScrub`: pauses; seeks are spaced while the pointer moves.
    pub(crate) fn begin_scrub(&mut self) {
        if self.document.project().clips.is_empty() {
            return;
        }
        self.pause();
        self.scrubbing = true;
    }

    /// `Editor::scrubTo`.
    pub(crate) fn scrub_to(&mut self, output_time: f64) {
        if self.document.project().clips.is_empty() {
            return;
        }
        if !self.scrubbing {
            self.begin_scrub();
        }
        let t = self.document.clamp_playback_position(output_time);
        self.active_clip = self.document.set_output_position(t);
        self.pending_seek = Some(t);
        self.seek_now();
    }

    /// `Editor::endScrub`.
    pub(crate) fn end_scrub(&mut self) {
        self.scrubbing = false;
    }

    /// `Editor::stepFrames`: pause, then a frame (at the recording's rate) either way.
    pub(crate) fn step_frames(&mut self, frames: i32) {
        self.pause();
        let target = self.document.step_target(frames);
        self.seek(target);
    }

    /// `Editor::seekBoundary`: the start or the end.
    pub(crate) fn seek_boundary(&mut self, direction: i32) {
        let target = self.document.boundary_target(direction);
        self.seek(target);
    }

    /// The sound from output time `from`: each clip's span at its speed, silence for inserted
    /// videos without sound, up to the playback range's end, two minutes or 32 clips at a time.
    pub(crate) fn audio_plan(&self, from: f64) -> Vec<video::AudioSegment> {
        let document = &self.document;
        let (first, clip_start) = document.clip_for_output(from);
        if first < 0 {
            return Vec::new();
        }
        let end = if document.has_playback_range() { document.playback_range().1 } else { document.duration() };
        let (_, screen_muted) = document.screen_audio();
        let screen_path = if document.has_audio() { document.video_path().to_owned() } else { String::new() };
        let mut segments = Vec::new();
        let mut output = clip_start;
        for (i, clip) in document.project().clips.iter().enumerate().skip(first as usize) {
            if output >= end - 0.0005 || segments.len() >= 32 || output - from > 120.0 {
                break;
            }
            let speed = clip.speed.max(0.01);
            let mut start = clip.in_;
            if i == first as usize {
                start = (clip.in_ + (from - clip_start) * speed).clamp(clip.in_, clip.out);
            }
            let segment_output = if i == first as usize { from } else { output };
            let mut length = clip.out - start;
            length = length.min((end - segment_output) * speed).max(0.0);
            output += (clip.out - clip.in_) / speed;
            if length <= 0.0005 {
                continue;
            }
            let (path, muted) = if clip.is_media() {
                let has = document.inserts().get(&clip.media).is_some_and(|info| info.audio);
                (if has { omareel_core::compat::qdir::file_path(document.bundle_path(), &clip.media) } else { String::new() }, false)
            } else {
                (screen_path.clone(), screen_muted)
            };
            segments.push(video::AudioSegment { path, start, length, speed, muted });
        }
        segments
    }

    /// Starts the sound at the playhead (when the clock starts).
    fn start_audio(&mut self) {
        let position = self.document.output_position();
        let (volume, _) = self.document.screen_audio();
        self.audio = video::Audio::start(self.audio_plan(position), position, volume, &self.audio_sink);
        self.audio_end = position + self.audio.as_ref().map_or(0.0, |a| a.segments.iter().map(video::AudioSegment::output_length).sum());
    }

    /// After an edit while playing: the sound starts again where what it plays changed; a
    /// volume change applies at once.
    pub(crate) fn audio_edited(&mut self) {
        if !self.playing || self.clock.is_none() {
            return;
        }
        let (volume, _) = self.document.screen_audio();
        let position = self.document.output_position();
        let plan = self.audio_plan(position);
        let same = match &self.audio {
            Some(audio) => {
                // What remains of the running plan, from the playhead.
                let mut running = self.audio_plan_from(audio, position);
                running.truncate(plan.len());
                running.len() == plan.len()
                    && running.iter().zip(&plan).all(|(a, b)| a.path == b.path && a.muted == b.muted && (a.speed - b.speed).abs() < 1e-9 && (a.start - b.start).abs() < 0.02 && (a.length - b.length).abs() < 0.02)
            }
            None => plan.is_empty() || plan.iter().all(|s| s.path.is_empty() || s.muted),
        };
        if same {
            if let Some(audio) = &self.audio {
                audio.set_volume(volume);
            }
        } else {
            self.audio = None;
            self.start_audio();
        }
    }

    /// The running plan's segments from output time `at` on (the first cut at `at`).
    fn audio_plan_from(&self, audio: &video::Audio, at: f64) -> Vec<video::AudioSegment> {
        let mut output = audio.output_start;
        let mut rest = Vec::new();
        for segment in &audio.segments {
            let length = segment.output_length();
            if output + length > at + 0.0005 {
                let mut s = segment.clone();
                if at > output {
                    let cut = (at - output) * s.speed;
                    s.start += cut;
                    s.length -= cut;
                }
                rest.push(s);
            }
            output += length;
        }
        rest
    }

    fn show(video: &Video, frame: video::Frame) {
        if std::env::var("SPIKE_EXPERIMENT").is_ok_and(|e| e.contains("novideo")) {
            return;
        }
        match (video, frame.pixels) {
            (Video::Surface(surface), video::Pixels::Nv12(bytes)) => {
                if let Err(e) = surface.push_bytes(bytes, (frame.time * 1000.0) as u64) {
                    eprintln!("push: {e}");
                }
            }
            (Video::Pixels(pixels), video::Pixels::Rgba(rgba)) => pixels.set(frame.width, frame.height, rgba),
            _ => {}
        }
    }

    /// While playing, the clip after the active one, when it doesn't carry on where this one
    /// ends: its decoders start a second before the cut, so the cut shows at once.
    fn prewarm(&mut self, output: f64) {
        let next_index = self.active_clip + 1;
        if self.next.as_ref().is_some_and(|n| n.clip == next_index) {
            return;
        }
        let clips = &self.document.project().clips;
        let (Some(current), Some(next)) = (clips.get(self.active_clip.max(0) as usize), clips.get(next_index as usize)) else { return };
        if next.is_media() || self.carries_on(self.active_clip, next_index) {
            return;
        }
        let clip_end = self.clip_output_start(next_index);
        if clip_end - output > 1.0 || clip_end - output < 0.0 {
            return;
        }
        let _ = current;
        let source = next.in_;
        let screen = self.screen_stream(next_index, source);
        let camera = self.camera_stream(source);
        self.next = Some(Prewarm { clip: next_index, screen, camera });
    }

    /// Whether clip `b` continues clip `a`'s footage, so the decoder runs on through the cut.
    fn carries_on(&self, a: i32, b: i32) -> bool {
        let clips = &self.document.project().clips;
        match (clips.get(a.max(0) as usize), clips.get(b.max(0) as usize)) {
            (Some(a), Some(b)) => !a.is_media() && !b.is_media() && (b.in_ - a.out).abs() < 0.001,
            _ => false,
        }
    }

    fn clip_output_start(&self, index: i32) -> f64 {
        self.document.project().clips.iter().take(index.max(0) as usize).map(|c| (c.out - c.in_) / c.speed.max(0.01)).sum()
    }

    /// The playhead entered clip `index` while playing: its decoders take over (prewarmed ones,
    /// the running ones when the footage carries on, else new ones while the clock waits).
    fn enter_clip(&mut self, index: i32, output: f64) {
        let previous = self.active_clip;
        self.active_clip = index;
        if self.document.clip_is_media(index) {
            self.decoders_from = None;
            self.screen = None;
            self.camera = None;
            self.next = None;
            return;
        }
        if self.carries_on(previous, index) && self.screen.is_some() {
            return;
        }
        if let Some(next) = self.next.take().filter(|n| n.clip == index) {
            self.decoders_from = None;
            self.screen = Some(next.screen);
            self.camera = next.camera;
            return;
        }
        // Nothing ready: start the decoders and hold the clock (and the sound) for them.
        self.restart_decoders(output);
        self.origin = output;
        self.clock = None;
        self.audio = None;
            self.audio_wait = None;
    }

    /// The frame about to be built: the playhead, the composition and the pictures. True when
    /// anything the view shows changed.
    fn step(&mut self) -> bool {
        let started = Instant::now();
        if let Some(target) = self.pending_seek
            && (self.last_restart.elapsed().as_secs_f64() > 0.035 || self.screen.is_none() || !self.scrubbing)
        {
            self.pending_seek = None;
            if self.playing {
                self.origin = target;
                self.clock = None;
                self.audio = None;
            self.audio_wait = None;
            }
            self.restart_decoders(target);
        }
        if let Some(seconds) = self.measure
            && !self.playing
            && self.measure_started.is_none()
            && self.started.elapsed().as_secs_f64() > 1.5
            && self.stats.frames.is_empty()
        {
            let _ = seconds;
            self.play();
        }
        let media_clip = self.document.clip_is_media(self.active_clip);
        if self.playing
            && self.clock.is_none()
            && self.pending_seek.is_none()
            && (media_clip || self.screen.as_ref().is_some_and(|s| s.current_time.is_some() || !s.starved(-1.0)))
        {
            // The sound starts with the picture: its ffmpeg needs a moment for its first
            // samples, so the clock waits for them (at most 0.3 s), then for the sink's latency.
            if self.audio_wait.is_none() {
                self.document.set_output_position(self.origin);
                self.start_audio();
                self.audio_wait = Some(self.now);
            }
            let waited = self.audio_wait.map_or(0.0, |at| self.now - at);
            if rune::is_headless() || self.audio.as_ref().is_none_or(video::Audio::ready) || waited > 0.3 {
                let latency = self.audio.as_ref().map_or(0.0, |a| a.latency);
                self.clock = Some((self.origin, self.now + latency));
                self.audio_wait = None;
                self.document.set_output_position(self.origin);
                if std::env::var_os("OMAREEL_PLAYBACK_LOG").is_some() {
                    eprintln!("playback: clock starts at output {:.3} after waiting {:.0} ms for the sound", self.origin, waited * 1000.0);
                }
                if self.measure.is_some() && self.measure_started.is_none() {
                    self.measure_started = Some(Instant::now());
                    // scripts/frames.py measures the window's commits from this line on.
                    eprintln!("spike: measured playback starts");
                }
            }
        }
        if let Some((origin, since)) = self.clock {
            let mut output = origin + (self.now - since).max(0.0);
            // The end of the playback range, or of the timeline: pause there.
            let end = if self.document.has_playback_range() { self.document.playback_range().1 } else { self.document.duration() };
            let stop = output >= end - 0.0005;
            if stop {
                output = end;
            }
            let index = self.document.set_output_position(output);
            if stop {
                self.pause();
                // Qt seeks to the range's end, which clamps a millisecond inside it.
                let t = self.document.clamp_playback_position(output);
                self.document.set_output_position(t);
                self.pending_seek = Some(t);
            } else {
                if index != self.active_clip {
                    self.enter_clip(index, output);
                }
                if self.clock.is_some() {
                    self.prewarm(output);
                    // The sound covers two minutes at a time.
                    if self.audio.is_some() && output > self.audio_end - 0.5 && self.audio_end < end - 0.01 {
                        self.audio = None;
                        self.start_audio();
                    }
                }
            }
        }
        let source = self.document.source_position();
        // The playhead moved (a seek, a scrub, a step): this frame's composition lands at once,
        // as Qt's does; only edits at the same time spring.
        let position = self.document.output_position();
        self.jumped = self.last_position.is_some_and(|last| (last - position).abs() > 1e-9);
        self.last_position = Some(position);
        let mut changed = self.step_insert();
        let composition = self.document.preview_composition(self.insert_ready, &self.insert_media);
        let mut skipped = 0;
        // Headless runs (`--snap`, `--video`) are frame-exact: wait for each frame's pictures.
        if rune::is_headless() {
            if let Some(stream) = self.screen.as_mut() {
                stream.wait_for(source);
            }
            if let (Some(stream), Some(sync)) = (self.camera.as_mut(), self.document.camera_sync(source)) {
                stream.wait_for(sync.target_ms as f64 / 1000.0);
            }
        }
        if let Some(stream) = self.screen.as_mut() {
            if let Some(frame) = stream.advance(source, &mut skipped) {
                Self::show(&self.media.screen, frame);
                self.stats.shown += 1;
                changed = true;
            } else if self.playing && self.clock.is_some() && stream.starved(source) {
                self.stats.starved += 1;
            }
        }
        if let (Some(stream), Some(sync)) = (self.camera.as_mut(), self.document.camera_sync(source)) {
            let mut camera_skipped = 0;
            if let Some(frame) = stream.advance(sync.target_ms as f64 / 1000.0, &mut camera_skipped) {
                Self::show(&self.media.camera, frame);
                changed = true;
            }
        }
        self.stats.skipped += skipped;
        if self.playing && std::env::var_os("OMAREEL_PLAYBACK_LOG").is_some() {
            let shown = self.screen.as_ref().and_then(|s| s.current_time).unwrap_or(-1.0);
            eprintln!("playback: t={:.3} output={:.3} clip={} source={:.3} shown={:.3}", self.now, self.document.output_position(), self.active_clip, source, shown);
        }
        self.frame = FrameState::preview(&composition);
        if self.measure_started.is_some() {
            self.stats.poll_started.push(started);
            self.stats.poll_ns.push(started.elapsed().as_nanos() as u64);
            self.stats.frames.push(Instant::now());
        }
        changed || self.playing
    }

    /// MediaLayer's player: while the playhead is in an inserted video, decode it from the
    /// matching time and hand its frames to `media.insert`. True when a frame was shown.
    fn step_insert(&mut self) -> bool {
        let seek = self.document.media_seek(self.document.output_position());
        let Some((media, source, speed)) = seek.and_then(|m| m.media.map(|media| (media, m.source, m.speed))) else {
            self.insert = None;
            self.insert_ready = false;
            return false;
        };
        let stale = self.insert.as_ref().is_none_or(|stream| {
            stream.finished || stream.current_time.is_some_and(|c| c > source + 0.2 || source - c > 1.0)
        });
        if media != self.insert_media || stale {
            let info = self.document.inserts().get(&media).copied().unwrap_or_default();
            let path = if std::path::Path::new(&media).is_absolute() { media.clone() } else { format!("{}/{}", self.document.bundle_path(), media) };
            let (w, h) = ((info.width.max(2) & !1) as u32, (info.height.max(2) & !1) as u32);
            let fps = (info.fps / speed.max(0.01)).clamp(1.0, 60.0);
            self.insert = Some(Stream::start(StreamSpec {
                path: &path,
                start: source,
                end: info.duration.max(source + 0.1),
                fps,
                width: w,
                height: h,
                ahead: 6,
                upload: self.upload,
                decode: Decode::Software,
                color: video::color_info(&path),
                preview: false,
            }));
            if media != self.insert_media {
                self.insert_ready = false;
            }
            self.insert_media = media;
        }
        let stream = self.insert.as_mut().expect("started above");
        if rune::is_headless() {
            stream.wait_for(source);
        }
        let mut skipped = 0;
        match stream.advance(source, &mut skipped) {
            Some(frame) => {
                Self::show(&self.media.insert, frame);
                self.insert_ready = true;
                true
            }
            None => false,
        }
    }

    pub(crate) fn busy(&self) -> bool {
        let source = self.document.source_position();
        let waiting = self
            .screen
            .as_ref()
            .is_some_and(|s| s.current_time.is_none_or(|c| (c - source).abs() > 1.0 / s.fps) && !s.finished);
        self.playing || self.pending_seek.is_some() || waiting || self.scrubbing || (self.insert.is_some() && !self.insert_ready) || (self.mode == Mode::Editor && (crate::editor::busy(self) || crate::editor::toast_showing(self)))
    }
}

fn poll(s: &mut Preview, t: f64, tx: &mut Tx) -> bool {
    s.now = t;
    let edited = crate::editor::poll(s);
    let changed = s.step() || edited;
    if std::env::var_os("SPIKE_DEBUG").is_some() {
        eprintln!("poll t={t:.3} changed={changed} busy={} playing={} pos={:.3}", s.busy(), s.playing, s.document.output_position());
    }
    if let Some(started) = s.measure_started
        && let Some(seconds) = s.measure
        && (started.elapsed().as_secs_f64() >= seconds || !s.playing)
    {
        s.measure_started = None;
        s.measure = None;
        write_stats(&s.stats, s.stats_path.as_deref(), &s.document);
        tx.quit();
    }
    if s.busy() {
        tx.wake_after(Duration::ZERO);
    } else if s.measure.is_some() {
        // Waiting to start the measured playback.
        tx.wake_after(Duration::from_millis(100));
    }
    if changed && s.playing {
        // Values come from omareel_core every frame; nothing should spring.
        tx.instant();
    }
    changed
}

fn on_key(s: &mut Preview, key: &KeyEvent, tx: &mut Tx) -> bool {
    if s.mode == Mode::Editor && crate::editor::on_key(s, key, tx) {
        return true;
    }
    match &key.key {
        Key::Text(t) if t == " " => {
            s.toggle();
            tx.wake_after(Duration::ZERO);
            true
        }
        // Main.qml: arrows step a frame, with Shift a second.
        Key::Left | Key::Right => {
            let direction = if matches!(key.key, Key::Left) { -1 } else { 1 };
            if key.mods.shift {
                s.seek(s.document.output_position() + f64::from(direction));
            } else {
                s.step_frames(direction);
            }
            tx.wake_after(Duration::ZERO);
            true
        }
        _ => false,
    }
}

/// PreviewPane.qml's `formatTenths`: mm:ss.t.
pub fn tenths(seconds: f64) -> String {
    let tenths = (seconds * 10.0).round().max(0.0) as i64;
    format!("{:02}:{:02}.{}", tenths / 600, (tenths % 600) / 10, tenths % 10)
}

fn view(s: &Preview, cx: &mut Cx<Preview>) -> El {
    if s.mode == Mode::Editor {
        return crate::editor::view(s, cx);
    }
    col().w_full().h_full().fill(s.ui.canvas).child(pane(s, cx, 64.0, 56.0)).child(transport(s, &s.ui))
}

/// The preview pane: the canvas, as large as fits with `margin_x` / `margin_y` around it (at most
/// `canvas_limit` tall), centred.
pub(crate) fn pane(s: &Preview, cx: &mut Cx<Preview>, margin_x: f32, margin_y: f32) -> El {
    let ui = &s.ui;
    let (out_w, out_h) = (s.document.output_width() as f32, s.document.output_height() as f32);
    let pane = cx.layout_size("pane").unwrap_or([0.0, 0.0]);
    let (avail_w, avail_h) = ((pane[0] - margin_x).max(0.0), (pane[1] - margin_y).max(0.0));
    let mut canvas_h = avail_h.min(avail_w * out_h / out_w.max(1.0));
    if s.canvas_limit > 0.0 {
        canvas_h = canvas_h.min(s.canvas_limit);
    }
    let canvas_w = (canvas_h * out_w / out_h.max(1.0)).floor();
    let canvas_h = canvas_h.floor();
    // The root's fill is the frame's clear colour; nothing else under the canvas fills, so every
    // pixel is drawn once (Rune blends every quad and redraws the window each frame).
    let mut pane_el = el().key("pane").w_full().grow(1.0).min_h(0.0).center();
    if canvas_w > 1.0 && canvas_h > 1.0 && !NO_COMPOSITION.load(std::sync::atomic::Ordering::Relaxed) {
        let mut canvas = el().key("canvas").size(canvas_w, canvas_h);
        // SPIKE_EXPERIMENT=nocanvasclip,nocanvasshadow: the canvas without its rounded clip or shadow.
        let experiment = std::env::var("SPIKE_EXPERIMENT").unwrap_or_default();
        if !experiment.contains("nocanvasclip") && ui.radius > 0.0 {
            canvas = canvas.radius(ui.radius).clip_rounded(ui.radius);
        }
        if !experiment.contains("nocanvasshadow") {
            canvas = canvas.shadow(14.0, 40.0, ui.shadow);
        }
        // While playing, every value comes from omareel_core; paused, an edit springs.
        canvas = canvas.child(comp::composition(&s.settings, &s.media, &s.frame, canvas_w, canvas_h, s.playing || s.scrubbing || s.jumped));
        if s.mode == Mode::Editor {
            canvas = pane_overlays(s, canvas, canvas_w, canvas_h);
        }
        pane_el = pane_el.child(canvas);
    }
    pane_el
}

/// PreviewPane.qml's `scaled` geometry in canvas points: the video's frame inside the padding
/// (without the border, as the QML has it) and the zoom's origin.
struct PaneGeometry {
    frame_x: f32,
    frame_y: f32,
    video_w: f32,
    video_h: f32,
    origin_x: f32,
    origin_y: f32,
    scale: f32,
}

fn pane_geometry(s: &Preview, w: f32, h: f32) -> PaneGeometry {
    let project = s.document.project();
    let no_background = project.background.type_ == "none";
    let pad = if no_background { 0.0 } else { w.min(h) * project.frame.padding as f32 };
    let aspect = (s.document.source_width() as f32 * project.crop.width as f32) / (s.document.source_height() as f32 * project.crop.height as f32).max(1.0);
    let video_w = if no_background { w } else { (w - 2.0 * pad).min((h - 2.0 * pad) * aspect) };
    let video_h = if no_background { h } else { video_w / aspect.max(0.0001) };
    let (frame_x, frame_y) = ((w - video_w) / 2.0, (h - video_h) / 2.0);
    let [scale, cx, cy] = s.frame.zoom;
    PaneGeometry { frame_x, frame_y, video_w, video_h, origin_x: frame_x + cx * video_w, origin_y: frame_y + cy * video_h, scale: scale.max(0.0001) }
}

/// The preview's own controls over the composition (PreviewPane.qml): picking a zoom's point,
/// the manual point's marker, the crop rectangle and its corners, and the notices on top.
fn pane_overlays(s: &Preview, mut canvas: El, w: f32, h: f32) -> El {
    let ui = &s.ui;
    let g = pane_geometry(s, w, h);
    let selected = s.document.selected_zoom_id().to_owned();
    if let Some(zoom) = s.document.project().zooms.iter().find(|z| z.id == selected && !z.automatic_target) {
        let tx = g.frame_x + zoom.target.x as f32 * g.video_w;
        let ty = g.frame_y + zoom.target.y as f32 * g.video_h;
        let x = g.origin_x + g.scale * (tx - g.origin_x);
        let y = g.origin_y + g.scale * (ty - g.origin_y);
        canvas = canvas.child(
            el().key("zoom-target-marker")
                .absolute(x - 9.0, y - 9.0)
                .size(18.0, 18.0)
                .radius(9.0)
                .fill(ui.accent_soft)
                .border(2.0, ui.accent)
                .inert()
                .child(el().absolute(7.0, 7.0).size(4.0, 4.0).radius(2.0).fill(ui.accent)),
        );
    }
    if s.edit.timeline.crop_mode {
        canvas = canvas.child(crop_rect(s, w, h));
    }
    if s.document.picking_zoom_target() {
        canvas = canvas.child(
            el().key("pick-target")
                .absolute(0.0, 0.0)
                .size(w, h)
                .cursor(Cursor::Crosshair)
                .on_drag(move |s: &mut Preview, drag: &mut rune::Drag, tx: &mut Tx| {
                    if matches!(drag.phase, rune::Phase::Up) && !drag.cancelled() {
                        let g = pane_geometry(s, drag.w, drag.h);
                        let (px, py) = (drag.start_x, drag.start_y);
                        let bx = g.origin_x + (px - g.origin_x) / g.scale;
                        let by = g.origin_y + (py - g.origin_y) / g.scale;
                        let x = f64::from((bx - g.frame_x) / g.video_w.max(1.0)).clamp(0.0, 1.0);
                        let y = f64::from((by - g.frame_y) / g.video_h.max(1.0)).clamp(0.0, 1.0);
                        if s.document.set_zoom_target_from_preview(x, y) {
                            crate::editor::edited(s, "Zoom point");
                        } else {
                            crate::editor::edited(s, "");
                        }
                    }
                    tx.wake_after(Duration::ZERO);
                }),
        );
        canvas = canvas.child(notice(s, "pick-hint", "Click where the zoom should center", ui.accent, true, false));
    }
    let importing = s.document.importing();
    let error = s.document.import_error().to_owned();
    if importing || !error.is_empty() {
        let text_ = if importing { "Adding clip…".to_owned() } else { error };
        canvas = canvas.child(notice(s, "import-notice", &text_, if importing { ui.accent } else { ui.record }, importing, !importing));
    }
    canvas
}

/// A pill at the top of the preview (OmPopupCard with a pulsing dot), with an optional dismiss.
fn notice(s: &Preview, key: &str, message: &str, dot: rune::Color, pulse: bool, dismiss: bool) -> El {
    let ui = &s.ui;
    let mut dot_el = el().size(8.0, 8.0).radius(4.0).fill(dot);
    if pulse {
        dot_el = dot_el.looping(rune::LoopSpec::opacity(1.3, 0.0, 0.3, 1.0));
    }
    let mut pill = row()
        .key(key.to_owned())
        .h(32.0)
        .px(18.0)
        .gap(8.0)
        .items_center()
        .radius(ui.radius.clamp(0.0, 8.0))
        .fill(ui.raised)
        .border(1.0, ui.stroke_strong)
        .shadow(8.0, 28.0, ui.shadow)
        .transition(rune::Transition::opacity().combined(rune::Transition::scale(0.94)))
        .child(dot_el)
        .child(text(message.to_owned()).font(TextStyle::new(13.0, 500.0)).color(ui.text_strong).truncate().max_w(520.0));
    if dismiss {
        pill = pill.child(
            el().key("import-dismiss")
                .role("button")
                .label("Dismiss")
                .size(20.0, 20.0)
                .center()
                .radius(4.0)
                .hover(ui.fill_hover)
                .cursor(Cursor::Pointer)
                .on_press(|s: &mut Preview| {
                    s.document.clear_import_error();
                    crate::editor::edited(s, "");
                })
                .child(rune::icon(&crate::icons::X, 14.0).color(ui.text_muted)),
        );
    }
    row().absolute(0.0, 14.0).w_full().justify_center().child(pill)
}

/// Crop mode: the crop rectangle over the canvas and a handle on each corner; dragging one
/// moves that corner (at least 5% each way), as one undo step.
fn crop_rect(s: &Preview, w: f32, h: f32) -> El {
    let ui = &s.ui;
    let c = &s.document.project().crop;
    let (x, y, cw, ch) = (c.x as f32 * w, c.y as f32 * h, c.width as f32 * w, c.height as f32 * h);
    let mut rect = el().key("crop-rect").absolute(x, y).size(cw, ch).border(3.0, ui.accent);
    for corner in 0..4u8 {
        let (fx, fy) = ((corner % 2) as f32, (corner / 2) as f32);
        let key = format!("crop-handle-{corner}");
        rect = rect.child(
            el().key(key)
                .absolute(fx * cw - 8.0, fy * ch - 8.0)
                .size(16.0, 16.0)
                .radius(ui.radius.clamp(0.0, 5.0))
                .fill(ui.accent)
                .border(1.0, ui.accent_foreground)
                .press_scale(1.15)
                .cursor(Cursor::Move)
                .on_drag(move |s: &mut Preview, drag: &mut rune::Drag, tx: &mut Tx| {
                    let canvas = s.edit.timeline.crop_canvas.get();
                    let (w, h) = (canvas[0].max(1.0), canvas[1].max(1.0));
                    match drag.phase {
                        rune::Phase::Down => {
                            s.document.begin_coalesced_edit("crop");
                            let c = &s.document.project().crop;
                            // Where the corner was, in canvas points.
                            drag.data = 0.0;
                            s.edit.timeline.crop_press = [(c.x + f64::from(fx) * c.width) as f32 * w, (c.y + f64::from(fy) * c.height) as f32 * h];
                        }
                        rune::Phase::Move => {
                            let [cx0, cy0] = s.edit.timeline.crop_press;
                            let px = (cx0 + drag.dx).clamp(0.0, w) / w;
                            let py = (cy0 + drag.dy).clamp(0.0, h) / h;
                            let c = s.document.project().crop.clone();
                            let left = if fx == 0.0 { f64::from(px) } else { c.x };
                            let top = if fy == 0.0 { f64::from(py) } else { c.y };
                            let right = if fx == 1.0 { f64::from(px) } else { c.x + c.width };
                            let bottom = if fy == 1.0 { f64::from(py) } else { c.y + c.height };
                            if right - left >= 0.05 && bottom - top >= 0.05 {
                                s.document.set_project_value("crop.x", left.into(), true);
                                s.document.set_project_value("crop.y", top.into(), true);
                                s.document.set_project_value("crop.w", (right - left).into(), true);
                                s.document.set_project_value("crop.h", (bottom - top).into(), true);
                                crate::editor::edited(s, "");
                            }
                        }
                        rune::Phase::Up => {
                            s.document.end_coalesced_edit();
                            crate::editor::edited(s, "");
                        }
                    }
                    tx.wake_after(Duration::ZERO);
                }),
        );
    }
    s.edit.timeline.crop_canvas.set([w, h]);
    rect
}

/// The bar under the preview: play/pause, the time, and a scrub bar.
pub(crate) fn transport(s: &Preview, ui: &Ui) -> El {
    let duration = s.document.duration().max(0.001);
    let position = s.document.output_position();
    let progress = (position / duration).clamp(0.0, 1.0) as f32;
    let play = el()
        .key("play")
        .role("button")
        .label(if s.playing { "Pause" } else { "Play" })
        .size(36.0, 36.0)
        .radius(ui.radius.max(6.0))
        .center()
        .fill(ui.normal_fill)
        .hover(ui.hover_fill)
        .press(ui.pressed_fill)
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Preview, tx: &mut Tx| {
            s.toggle();
            tx.wake_after(Duration::ZERO);
        })
        .child(play_glyph(s.playing, ui.text));
    let mono = TextStyle { family: Family::SystemMono, ..TextStyle::new(13.0, 500.0) };
    let time = text(format!("{} / {}", tenths(position), tenths(duration))).font(mono).color(ui.text_muted);
    let track = el()
        .key("scrub")
        .grow(1.0)
        .h(24.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut Preview, drag, tx: &mut Tx| {
            let width = drag.w.max(1.0);
            let x = drag.x.clamp(0.0, width);
            s.scrubbing = !matches!(drag.phase, rune::Phase::Up);
            s.seek(f64::from(x / width) * s.document.duration());
            tx.wake_after(Duration::ZERO);
        })
        .child(
            el().w_full().h(4.0).radius(2.0).fill(ui.normal_fill).child(el().h(4.0).w_frac(progress).radius(2.0).fill(ui.accent)),
        );
    row().key("transport").w_full().h(56.0).px(16.0).gap(14.0).items_center().fill(ui.surface).border(0.0, ui.hairline).child(play).child(time).child(track)
}

/// A play triangle or two pause bars.
pub(crate) fn play_glyph(playing: bool, color: rune::Color) -> El {
    if playing {
        row().gap(4.0).child(el().size(4.0, 14.0).radius(1.0).fill(color)).child(el().size(4.0, 14.0).radius(1.0).fill(color))
    } else {
        let mut shape = rune::Shape::new();
        shape = shape.fill(vec![[0.0, 0.0], [12.0, 7.0], [0.0, 14.0]], color);
        el().size(12.0, 14.0).ml(2.0).shape(shape)
    }
}

pub fn run(options: Options) -> Result<(), String> {
    // The window's GPU device (250 ms on NVIDIA), the theme's fc-match and hyprctl, and the
    // colour probes of both videos run on threads while the recording opens.
    rune::warm_gpu(wgpu::Features::empty());
    omareel_core::theme::prefetch();
    let bundle_file = |name: &str| omareel_core::compat::qdir::file_path(&omareel_core::compat::qdir::absolute_path(&options.bundle), name);
    let screen_color = {
        let path = bundle_file("screen.mp4");
        std::thread::spawn(move || video::color_info(&path))
    };
    let camera_color = {
        let path = bundle_file("camera.mp4");
        std::thread::spawn(move || video::color_info(&path))
    };
    let probes = Probes::new(Arc::new(media_probe::probe_video));
    let mut document = Document::open(&options.bundle, &probes)?;
    // The editor builds the motion track and reads the frame times (an ffprobe over every
    // packet) on workers, as the Qt editor does, and shows the default sample meanwhile; the
    // preview's measurements wait for both.
    let mut motion = None;
    let mut frame_times = None;
    if options.editor {
        if let Some(job) = document.motion_job() {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(job.run());
            });
            motion = Some(receive);
        }
        let (send, receive) = std::sync::mpsc::channel();
        let video_path = document.video_path().to_owned();
        std::thread::spawn(move || {
            let _ = send.send(media_probe::probe_frame_times(&video_path));
        });
        frame_times = Some(receive);
    } else {
        if let Some(job) = document.motion_job() {
            document.apply_motion(job.run());
        }
        document.set_screen_frame_times(media_probe::probe_frame_times(document.video_path()));
    }
    // SPIKE_EXPERIMENT=pick: the first zoom selected and its point being picked (the
    // inspector's "Pick point"), to test the preview's picking headless.
    if std::env::var("SPIKE_EXPERIMENT").is_ok_and(|e| e.contains("pick"))
        && let Some(id) = document.project().zooms.first().map(|z| z.id.clone())
    {
        document.set_selected_zoom_id(&id);
        document.set_picking_zoom_target(true);
    }
    let _ = document.take_effects();

    let mut settings = Settings::new(
        document.project(),
        (document.source_width(), document.source_height()),
        document.capture_scale(),
        document.has_camera().then(|| (document.camera_source_width(), document.camera_source_height())),
    );
    let background_path = match document.project().background.type_.as_str() {
        "image" => document.project().background.image.clone(),
        "wallpaper" if document.project().background.wallpaper == "omarchy:current" => omareel_core::omarchy_paths::current_background(),
        "wallpaper" => document.project().background.wallpaper.clone(),
        _ => String::new(),
    };
    let experiment = comp::apply_experiments(&mut settings);
    if experiment.contains("nocomp") {
        NO_COMPOSITION.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    let background_path = if experiment.contains("nobg") { String::new() } else { background_path };
    let (sw, sh) = (document.source_width(), document.source_height());
    let screen_size = if options.decode_height > 0 && options.decode_height < sh {
        let h = options.decode_height & !1;
        ((((f64::from(sw) * f64::from(h) / f64::from(sh)).round() as i32) & !1) as u32, h as u32)
    } else {
        (sw as u32, sh as u32)
    };
    let camera_path = document.camera_video_path().to_owned();
    let screen_color = screen_color.join().unwrap_or_else(|_| video::color_info(document.video_path()));
    let camera_color = camera_color.join().unwrap_or_else(|_| video::color_info(&camera_path));
    let upload = options.upload;
    let decode = options.decode.clone();
    eprintln!("preview: screen {}x{} decoded {:?} ({:?}), upload {:?}", screen_size.0, screen_size.1, decode, screen_color.matrix, upload);
    let new_video = || match upload {
        Upload::Surface => Video::Surface(NativeSurface::new()),
        Upload::Pixels => Video::Pixels(Pixels::new()),
    };
    let media_screen = new_video();
    let media_camera = new_video();
    let media_insert = new_video();
    let canvas_limit = options.canvas_height;
    let measure = options.measure;
    let stats_path = options.stats.clone();

    let mode = if options.editor { Mode::Editor } else { Mode::Preview };
    let title: &'static str = if options.editor { Box::leak(format!("{} — Omareel", document.bundle_name()).into_boxed_str()) } else { "omareel (Rune spike)" };
    let size = if options.editor { (1440.0, 900.0) } else { (1280.0, 760.0) };
    let spec = AppSpec {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        // Inter for the interface, fontconfig's monospace for times and keys.
        families: ("Inter", "ui-monospace"),
        ..AppSpec::new(title, size, || unreachable!("App::init supplies the state"), view)
    };
    // Main.qml's title, and the app id the Qt editor has (Hyprland and Omarchy match on it).
    rune::set_app_id("omareel");
    let ui = Ui::load();
    let kit = crate::editor::kit(&ui);
    App::new(spec)
        .args(options.rune_args.clone())
        .on_frame_encoded(|f| {
            if let Ok(mut encoded) = ENCODED.lock() {
                encoded.push((f.at(), matches!(f.outcome(), rune::FrameOutcome::Drawn)));
            }
        })
        .init(move || {
            let cache = rune::ImageCache::new(Arc::new(|| {}));
            let background = (!background_path.is_empty()).then(|| cache.file(&background_path, 0.0, 1.0));
            // The cursor is at most a few times its 24 pt size; rasterise once, large, and let the
            // mips cover the rest.
            let cursors = comp::cursor_images(&cache, 96.0, 2.0);
            // Headless runs draw what is decoded at each frame: wait for the images.
            if rune::is_headless() {
                for image in background.iter().chain(cursors.iter()) {
                    let _ = image.wait();
                }
            }
            Preview {
                mode,
                kit,
                motion,
                frame_times,
                decoders_from: None,
                last_position: None,
                jumped: false,
                edit: crate::editor::EditState { background_path: background_path.clone(), ..Default::default() },
                now: 0.0,
                document,
                settings,
                media: Media { screen: media_screen, camera: media_camera, insert: media_insert, background, cursors },
                frame: FrameState::default(),
                upload,
                decode,
                screen_color,
                camera_color,
                screen: None,
                camera: None,
                insert: None,
                insert_media: String::new(),
                insert_ready: false,
                screen_size,
                camera_path,
                canvas_limit,
                playing: false,
                clock: None,
                origin: 0.0,
                pending_seek: Some(0.0),
                active_clip: 0,
                next: None,
                audio: None,
                audio_end: 0.0,
                audio_wait: None,
                audio_sink: video::AudioSink::from_env(rune::is_headless()),
                last_restart: Instant::now(),
                started: Instant::now(),
                measure,
                measure_started: None,
                stats: Stats::default(),
                stats_path,
                ui,
                scrubbing: false,
                cache,
            }
        })
        .run();
    Ok(())
}

fn setup(s: &mut Preview, waker: rune::Waker) {
    // SPIKE_EXPERIMENT=nowaker: frames are pushed from poll, on the UI thread, so the surfaces'
    // waker only asks for frames poll already asked for.
    if std::env::var("SPIKE_EXPERIMENT").is_ok_and(|e| e.contains("nowaker")) {
        return;
    }
    if let Video::Surface(surface) = &s.media.screen {
        surface.set_waker(waker.clone());
    }
    if let Video::Surface(surface) = &s.media.camera {
        surface.set_waker(waker.clone());
    }
}

fn write_stats(stats: &Stats, path: Option<&str>, document: &Document) {
    let mut gaps: Vec<f64> = stats.frames.windows(2).map(|w| (w[1] - w[0]).as_secs_f64() * 1000.0).collect();
    gaps.sort_by(f64::total_cmp);
    let pick = |p: f64| gaps.get(((gaps.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0);
    let mean = |v: &[u64]| if v.is_empty() { 0.0 } else { v.iter().sum::<u64>() as f64 / v.len() as f64 / 1e6 };
    let span = match (stats.frames.first(), stats.frames.last()) {
        (Some(a), Some(b)) => (*b - *a).as_secs_f64(),
        _ => 0.0,
    };
    let recording_frames = (span * document.fps()).round();
    // Each measured poll to the first frame encoded after it: the UI thread's whole frame
    // (state, view, layout, encode, submit).
    let encoded = ENCODED.lock().map(|e| e.clone()).unwrap_or_default();
    let mut cpu: Vec<f64> = stats
        .poll_started
        .iter()
        .filter_map(|p| encoded.iter().find(|(at, _)| at >= p).map(|(at, _)| (*at - *p).as_secs_f64() * 1000.0))
        .collect();
    cpu.sort_by(f64::total_cmp);
    let cpu_pick = |q: f64| cpu.get(((cpu.len().max(1) - 1) as f64 * q).round() as usize).copied().unwrap_or(0.0);
    let drawn = encoded.iter().filter(|(at, drawn)| *drawn && stats.frames.first().is_some_and(|f| at >= f)).count();
    let json = format!(
        "{{\"seconds\": {span:.2}, \"frames\": {}, \"fps\": {:.2}, \"frame_ms_median\": {:.2}, \"frame_ms_p95\": {:.2}, \"frame_ms_max\": {:.2}, \
         \"missed_vsyncs\": {}, \"poll_ms_mean\": {:.3}, \"screen_frames_shown\": {}, \"screen_frames_skipped\": {}, \"starved_frames\": {}, \
         \"recording_frames_in_span\": {recording_frames}, \"frame_cpu_ms_median\": {:.2}, \"frame_cpu_ms_p95\": {:.2}, \"drawn\": {drawn}}}",
        stats.frames.len(),
        if span > 0.0 { (stats.frames.len().max(1) - 1) as f64 / span } else { 0.0 },
        pick(0.5),
        pick(0.95),
        gaps.last().copied().unwrap_or(0.0),
        gaps.iter().filter(|g| **g > 25.0).count(),
        mean(&stats.poll_ns),
        stats.shown,
        stats.skipped,
        stats.starved,
        cpu_pick(0.5),
        cpu_pick(0.95),
    );
    println!("{json}");
    if let Some(path) = path {
        let _ = std::fs::write(path, json + "\n");
    }
}
