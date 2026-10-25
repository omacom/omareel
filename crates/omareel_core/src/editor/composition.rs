//! The preview's composition state, QML's `comp` (`previewZoom`, `previewLayout`,
//! `previewMedia`, `previewCursor`, `previewRipples`, `previewKeystrokePills`, `cameraVisible`,
//! `time`; Editor.cpp 529-603). It is the editor's variant, which differs on purpose from the
//! exporter's frame loop (tests/golden/README.md, "composition"): during an inserted video it
//! hides the cursor but keeps the recording time's zoom, its media map has `missing` and a
//! `ready` that follows the media player, and before the motion track is built it shows the
//! default sample.

use super::Document;
use crate::camera_timeline::map_camera_time;
use crate::compat::cpp;
use crate::keystroke_track::KeystrokePill;
use crate::motion_track::{MotionSample, RippleSample};

/// `zoom`: `{scale, cx, cy, velocity}`, all doubles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewZoom {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub velocity: f64,
}

/// `layout`: `{cameraFull, bubble}`, doubles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewLayout {
    pub camera_full: f64,
    pub bubble: f64,
}

/// `media`: `{active, ready, fit, width, height, missing}`; `width` and `height` are `int`s.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewMedia {
    pub active: bool,
    pub ready: bool,
    pub fit: String,
    pub width: i32,
    pub height: i32,
    pub missing: bool,
}

/// `cursor`: `{x, y, visible, scale, opacity, rotation}`; `visible` is a bool, the rest doubles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewCursor {
    pub x: f64,
    pub y: f64,
    pub visible: bool,
    pub scale: f64,
    pub opacity: f64,
    pub rotation: f64,
}

/// Everything `comp` shows at the output position, read at once.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewComposition {
    /// `time`: the source position.
    pub time: f64,
    pub zoom: PreviewZoom,
    pub layout: PreviewLayout,
    pub media: PreviewMedia,
    pub cursor: PreviewCursor,
    /// `ripples`: `{x, y, progress}` maps.
    pub ripples: Vec<RippleSample>,
    /// `keystrokePills`: `{keys (QStringList), text, opacity}` maps.
    pub keystroke_pills: Vec<KeystrokePill>,
    pub camera_visible: bool,
}

impl Document {
    fn showing_insert(&self) -> bool {
        self.clip_is_media(self.clip_for_output(self.output_position).0)
    }

    pub fn preview_zoom(&self) -> PreviewZoom {
        let s = self.motion.as_ref().map_or_else(MotionSample::default, |motion| motion.sample(self.source_position()));
        PreviewZoom { scale: s.zoom_scale, cx: s.zoom_cx, cy: s.zoom_cy, velocity: s.zoom_center_velocity }
    }

    pub fn preview_layout(&self) -> PreviewLayout {
        let layout = self.layout_at(self.output_position);
        PreviewLayout { camera_full: layout.camera_full, bubble: layout.bubble }
    }

    /// `previewMedia()`: `ready` needs the media player's frame (`m_playback.mediaFrameReady()`)
    /// and the media player to hold this clip's file (`m_mediaPlayerSource`).
    pub fn preview_media(&self, media_frame_ready: bool, media_player_source: &str) -> PreviewMedia {
        let index = self.clip_for_output(self.output_position).0;
        let Some(clip) = self.clip(index).filter(|clip| clip.is_media()) else {
            return PreviewMedia { active: false, ready: true, fit: "fit".into(), width: 16, height: 9, missing: false };
        };
        let info = self.inserts.get(&clip.media).copied().unwrap_or_default();
        PreviewMedia {
            active: true,
            ready: media_frame_ready && media_player_source == clip.media,
            fit: clip.fit.clone(),
            width: cpp::max(1, info.width),
            height: cpp::max(1, info.height),
            missing: !info.is_valid(),
        }
    }

    pub fn preview_cursor(&self) -> PreviewCursor {
        let insert = self.showing_insert();
        let s = match &self.motion {
            Some(motion) if !insert => motion.sample(self.source_position()),
            _ => MotionSample::default(),
        };
        PreviewCursor {
            x: s.cursor_x,
            y: s.cursor_y,
            visible: self.project.cursor.visible && !insert,
            scale: s.cursor_scale,
            opacity: s.cursor_opacity,
            rotation: s.cursor_rotation,
        }
    }

    /// Click ripples, for the "ripple" and "circle" click effects.
    pub fn preview_ripples(&self) -> Vec<RippleSample> {
        let effect = self.project.cursor.click_effect.as_str();
        match &self.motion {
            Some(motion) if !self.showing_insert() && (effect == "ripple" || effect == "circle") => {
                motion.ripples(self.source_position())
            }
            _ => Vec::new(),
        }
    }

    pub fn preview_keystroke_pills(&self) -> Vec<KeystrokePill> {
        if self.showing_insert() {
            return Vec::new();
        }
        self.keystroke_track.sample(self.source_position())
    }

    /// `cameraVisible()`: a camera, no inserted clip, and the camera started; a full-frame camera
    /// holds its first frame until it started instead of leaving the frame empty.
    pub fn camera_visible(&self) -> bool {
        let Some(camera) = self.recording.camera else { return false };
        if self.showing_insert() {
            return false;
        }
        let mapped = map_camera_time(self.source_position(), camera.offset, camera.duration);
        !mapped.before_start || self.layout_at(self.output_position).camera_full > 0.0
    }

    /// All of `comp` at the output position.
    pub fn preview_composition(&self, media_frame_ready: bool, media_player_source: &str) -> PreviewComposition {
        PreviewComposition {
            time: self.source_position(),
            zoom: self.preview_zoom(),
            layout: self.preview_layout(),
            media: self.preview_media(media_frame_ready, media_player_source),
            cursor: self.preview_cursor(),
            ripples: self.preview_ripples(),
            keystroke_pills: self.preview_keystroke_pills(),
            camera_visible: self.camera_visible(),
        }
    }
}
