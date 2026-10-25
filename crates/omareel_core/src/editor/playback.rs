//! The decisions inside `Editor`'s playback code, as functions of the transport state the Qt
//! side keeps. Each names the C++ it came from (src/ui/Editor.cpp in this tree); the player
//! calls around it stay there.

use super::Document;
use crate::camera_timeline::map_camera_time;
use crate::compat::cpp;
use crate::compat::qround::q_round64;
use crate::layout_timeline::{sample_layout, LayoutSample, DEFAULT_TRANSITION};

/// `PlaybackDriver::Player`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Player {
    Screen,
    Camera,
    Media,
}

/// The transport state `Editor` kept beside its players.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transport {
    /// `m_transportPlaying` (`playing()`).
    pub playing: bool,
    /// `m_scrubbing`.
    pub scrubbing: bool,
    /// `m_internalSeek`.
    pub internal_seek: bool,
    /// `m_activeClip`: the clip whose player runs.
    pub active_clip: i32,
    /// `m_playheadPosition`.
    pub playhead_position: f64,
    /// `m_playheadClockPosition`.
    pub playhead_clock_position: f64,
}

/// What `seekMedia(outputTime)` shows (Editor.cpp 1200-1227): the clip there, its time
/// clamped into it, and that time in milliseconds for the player. An inserted clip plays in the
/// media player (`media` is its path; the screen and camera pause), any other in the screen
/// player (the media player pauses and the camera is synced with force at `source`).
#[derive(Clone, Debug, PartialEq)]
pub struct MediaSeek {
    pub clip_index: i32,
    pub media: Option<String>,
    pub speed: f64,
    pub source: f64,
    pub target_ms: i64,
}

/// Which way `prepareNextClip` readies the next clip's player.
#[derive(Clone, Debug, PartialEq)]
pub enum NextClipKind {
    /// From the recording into an inserted video: load it (unless the media player already holds
    /// it paused with a ready frame within a millisecond of `target_ms`), set its rate, and seek
    /// it without playing.
    IntoMedia(String),
    /// From an inserted video back to the recording: unless the screen player is playing, set its
    /// rate and seek it without playing.
    IntoRecording,
}

/// `prepareNextClip()` (Editor.cpp 710-731): the cut after the active clip, when it changes
/// players.
#[derive(Clone, Debug, PartialEq)]
pub struct NextClip {
    pub index: i32,
    pub kind: NextClipKind,
    pub speed: f64,
    pub target_ms: i64,
}

/// `syncCamera(screenSourceTime, force)` (Editor.cpp 1308-1316): where the camera should be
/// (`m_playback.syncCamera(target_ms, tolerance_ms, force, covered)`). A bubble tolerates some
/// drift; a full-frame talking head needs its lips on the audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraSync {
    pub target_ms: i64,
    pub tolerance_ms: i64,
    /// The camera recorded this moment (neither before its start nor past its end).
    pub covered: bool,
}

/// What `handlePlayerPosition(milliseconds)` does with a player's position (Editor.cpp
/// 1274-1306), in order: at the playback range's end it pauses and seeks there and does nothing
/// else; otherwise it syncs the camera (without force) at `sync_camera`, then either advances to
/// the next clip or moves the position to `position` (`positionChanged`, `compositionChanged`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerPosition {
    pub stop_at_range_end: Option<f64>,
    pub sync_camera: Option<f64>,
    pub advance: bool,
    pub position: Option<f64>,
}

/// `std::max({a, b, c})`: the first of the largest, by `<`.
fn max3(a: f64, b: f64, c: f64) -> f64 {
    let mut largest = a;
    for x in [b, c] {
        if largest < x {
            largest = x;
        }
    }
    largest
}

impl Document {
    /// `playerFor(clipIndex)` (Editor.cpp 673-676).
    pub fn player_for(&self, clip_index: i32) -> Player {
        if self.clip_is_media(clip_index) { Player::Media } else { Player::Screen }
    }

    /// `m_playback.setPlayingQuery` (Editor.cpp 300-303): whether a player should be running.
    pub fn player_should_play(&self, player: Player, transport: &Transport) -> bool {
        match player {
            Player::Camera => transport.playing,
            _ => transport.playing && self.player_for(transport.active_clip) == player,
        }
    }

    /// `m_playback.setScreenRestQuery` (Editor.cpp 304): where a screen warm-up comes to rest.
    pub fn screen_rest_ms(&self) -> i64 {
        q_round64(self.source_position() * 1000.0)
    }

    /// `layoutAt(outputTime)`.
    pub fn layout_at(&self, output_time: f64) -> LayoutSample {
        sample_layout(&self.project.clips, output_time, self.has_camera(), self.project.camera.enabled, DEFAULT_TRANSITION)
    }

    /// The screen player's sound in `changed()`: the volume (0 to 1), muted when neither the
    /// desktop nor the microphone is kept.
    pub fn screen_audio(&self) -> (f64, bool) {
        (cpp::clamp(self.project.audio.volume, 0.0, 1.0), !self.project.audio.desktop && !self.project.audio.mic)
    }

    /// `updateInsertAudio()` (Editor.cpp 745-750): the media player's volume, muted while it
    /// decodes ahead of its clip or warms up on a paused frame.
    pub fn insert_audio(&self, active_clip: i32, media_warming: bool) -> (f64, bool) {
        (cpp::clamp(self.project.audio.volume, 0.0, 1.0), media_warming || !self.clip_is_media(active_clip))
    }

    /// `seekMedia(outputTime)`'s target; `None` without clips (the C++ never called it then).
    pub fn media_seek(&self, output_time: f64) -> Option<MediaSeek> {
        let (index, clip_start) = self.clip_for_output(output_time);
        let clip = self.clip(index)?;
        let source = cpp::clamp(clip.in_ + (output_time - clip_start) * clip.speed, clip.in_, clip.out);
        Some(MediaSeek {
            clip_index: index,
            media: clip.is_media().then(|| clip.media.clone()),
            speed: clip.speed,
            source,
            target_ms: q_round64(source * 1000.0),
        })
    }

    /// `prepareNextClip()`'s decision for the active clip.
    pub fn next_clip(&self, active_clip: i32) -> Option<NextClip> {
        let next = active_clip.checked_add(1)?;
        if active_clip < 0 {
            return None;
        }
        let clip = self.clip(next)?;
        let target_ms = q_round64(clip.in_ * 1000.0);
        let kind = if !self.clip_is_media(active_clip) && clip.is_media() {
            NextClipKind::IntoMedia(clip.media.clone())
        } else if self.clip_is_media(active_clip) && !clip.is_media() {
            NextClipKind::IntoRecording
        } else {
            return None;
        };
        Some(NextClip { index: next, kind, speed: clip.speed, target_ms })
    }

    /// `advanceClip()` while playing (Editor.cpp 733-743): `Some(t)` to seek to the next clip's
    /// start, `None` at the last clip, where playback pauses and the position goes to the end.
    pub fn advance_target(&self, active_clip: i32) -> Option<f64> {
        let next = active_clip.checked_add(1)?;
        let clip = self.clip(next)?;
        Some(self.timeline().output_time(next, clip.in_))
    }

    /// `play()`'s seek before it starts (Editor.cpp 1233-1237): to the range's start when the
    /// position is outside the playback range, to 0 at the end of the timeline.
    pub fn play_start_seek(&self) -> Option<f64> {
        let position = self.output_position;
        if self.has_playback_range() {
            if position < self.playback_range_start || position >= self.playback_range_end - 0.001 {
                return Some(self.playback_range_start);
            }
            None
        } else if position >= self.duration() - 0.0001 {
            Some(0.0)
        } else {
            None
        }
    }

    /// `stepFrames(frames)`'s seek target (after pausing).
    pub fn step_target(&self, frames: i32) -> f64 {
        self.output_position + f64::from(frames) / cpp::max(1.0, self.recording.fps)
    }

    /// `seekBoundary(direction)`'s seek target.
    pub fn boundary_target(&self, direction: i32) -> f64 {
        if direction < 0 { 0.0 } else { self.duration() }
    }

    /// `syncCamera`'s target for the screen's source time; `None` without a camera.
    pub fn camera_sync(&self, screen_source_time: f64) -> Option<CameraSync> {
        let camera = self.recording.camera?;
        let mapped = map_camera_time(screen_source_time, camera.offset, camera.duration);
        let tolerance_ms = if self.layout_at(self.output_position).camera_full > 0.0 { 80 } else { 250 };
        Some(CameraSync {
            target_ms: q_round64(mapped.seconds * 1000.0),
            tolerance_ms,
            covered: !mapped.before_start && !mapped.beyond_end,
        })
    }

    /// `setPlayheadPosition(outputTime)` (Editor.cpp 1142-1148): the new playhead, or `None`
    /// when it moves less than half a millisecond.
    pub fn playhead_update(&self, current: f64, output_time: f64) -> Option<f64> {
        let output_time = cpp::clamp(output_time, 0.0, self.duration());
        if (current - output_time).abs() < 0.0005 {
            return None;
        }
        Some(output_time)
    }

    /// `updatePlayheadPosition()` (Editor.cpp 1157-1180), when playing, not scrubbing and the
    /// clock is valid: what `setPlayheadPosition` gets, given the clock's elapsed milliseconds. A
    /// decoded frame supplies the clock origin; the playhead interpolates between frames but
    /// stops within two frame intervals if decoding stalls, and where the screen stood still (no
    /// frames) it runs on to the next frame. `None` without clips.
    pub fn playhead_advance(&self, transport: &Transport, elapsed_ms: i64) -> Option<f64> {
        let clips = &self.project.clips;
        if clips.is_empty() {
            return None;
        }
        let active = transport.active_clip;
        let clip = &clips[active.clamp(0, clips.len() as i32 - 1) as usize];
        let timeline = self.timeline();
        let clip_end = timeline.output_time(active, clip.out);
        let limit = cpp::min(
            clip_end,
            if self.has_playback_range() { self.playback_range_end - 0.001 } else { self.duration() },
        );
        let mut max_lead = cpp::max(0.05, 2.0 / cpp::max(1.0, self.recording.fps));
        if !clip.is_media() {
            let anchor = clip.in_ + (transport.playhead_clock_position - timeline.output_time(active, clip.in_)) * clip.speed;
            max_lead += cpp::max(0.0, self.next_screen_frame_time(anchor) - anchor) / clip.speed;
        }
        let elapsed = cpp::min(elapsed_ms as f64 / 1000.0, max_lead);
        let target = max3(transport.playhead_position, self.output_position, transport.playhead_clock_position + elapsed);
        Some(cpp::min(target, limit))
    }

    /// `handleFrameShown(player, startTimeUs, live)` (Editor.cpp 624-659): the playhead clock's
    /// new origin (`m_playheadClockPosition`, then restart the clock) when a frame the active
    /// clip's player shows while playing lies near the clip and the transport position; a seek
    /// can deliver queued frames from the previous location.
    pub fn frame_anchor(&self, player: Player, transport: &Transport, start_time_us: i64, live: bool) -> Option<f64> {
        if !transport.playing || !live || transport.scrubbing || transport.internal_seek || start_time_us < 0 {
            return None;
        }
        let active = transport.active_clip;
        let clip = match player {
            Player::Screen => self.clip(active).filter(|clip| !clip.is_media())?,
            Player::Media => self.clip(active).filter(|clip| clip.is_media())?,
            Player::Camera => return None,
        };
        let source = start_time_us as f64 / 1_000_000.0;
        let output = self.source_to_output(source, active);
        let position = self.output_position;
        if source >= clip.in_ - 0.01 && source < clip.out && output >= position - 0.1 && output <= position + 0.5 {
            Some(cpp::max(transport.playhead_position, output))
        } else {
            None
        }
    }

    /// `handlePlayerPosition(milliseconds)`'s decision; `None` while the transport seeks or
    /// scrubs, or without clips. Through a still stretch of the recording the player holds its
    /// last frame's time while the playhead runs on; the camera, the clip's end and the position
    /// follow the playhead.
    pub fn player_position(&self, transport: &Transport, milliseconds: i64) -> Option<PlayerPosition> {
        let clips = &self.project.clips;
        if transport.internal_seek || transport.scrubbing || clips.is_empty() {
            return None;
        }
        let active = transport.active_clip;
        let clip = &clips[active.clamp(0, clips.len() as i32 - 1) as usize];
        let mut source = milliseconds as f64 / 1000.0;
        if transport.playing && !clip.is_media() {
            let playhead_source =
                clip.in_ + (transport.playhead_position - self.timeline().output_time(active, clip.in_)) * clip.speed;
            if playhead_source >= clip.in_ - 0.001 && playhead_source <= clip.out + 0.001 {
                source = cpp::max(source, cpp::min(playhead_source, clip.out));
            }
        }
        let mapped = self.source_to_output(source, active);
        let mut step = PlayerPosition { stop_at_range_end: None, sync_camera: None, advance: false, position: None };
        if self.has_playback_range() && transport.playing && mapped >= self.playback_range_end - 0.001 {
            step.stop_at_range_end = Some(self.playback_range_end);
            return Some(step);
        }
        if !clip.is_media() {
            step.sync_camera = Some(source);
        }
        if transport.playing && source >= clip.out - 0.004 {
            step.advance = true;
            return Some(step);
        }
        if mapped >= 0.0 {
            let output = self.clamp_playback_position(mapped);
            if (self.output_position - output).abs() >= 0.0005 {
                step.position = Some(output);
            }
        }
        Some(step)
    }
}
