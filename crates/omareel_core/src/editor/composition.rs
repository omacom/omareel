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
use crate::compat::qjson::Object;
use crate::compat::geometry::PointF;
use crate::keystroke_track::KeystrokePill;
use crate::motion_track::{MotionSample, RippleSample};
use crate::overlay_track;
use crate::reframe::{self, ReframeWindow};

/// `zoom`: `{scale, cx, cy, velocity}`, all doubles; a reframing project adds `reframe: true`
/// and the window `rx`, `ry`, `rw`, `rh` (as the exporter's [`crate::export::composition::Zoom`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewZoom {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub velocity: f64,
    pub reframe: Option<ReframeWindow>,
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
    /// `overlays`: the overlays showing, each one's project JSON with its `opacity`.
    pub overlays: Vec<Object>,
    pub camera_visible: bool,
}

impl Document {
    fn showing_insert(&self) -> bool {
        self.clip_is_media(self.clip_for_output(self.output_position).0)
    }

    pub fn preview_zoom(&self) -> PreviewZoom {
        let s = self.motion.as_ref().map_or_else(MotionSample::default, |motion| motion.sample(self.source_position()));
        let reframe = self.preview_reframe(&s);
        PreviewZoom { scale: s.zoom_scale, cx: s.zoom_cx, cy: s.zoom_cy, velocity: s.zoom_center_velocity, reframe }
    }

    /// The reframe window to draw, shaped for the project as it is now: the motion track is
    /// rebuilt off the GUI thread after an edit, so until then its window is refitted to the
    /// current canvas ([`reframe::refit`]), or made up around the cursor when reframing has just
    /// been turned on, and turning it off (or a canvas of the recording's shape) drops it at once.
    fn preview_reframe(&self, sample: &MotionSample) -> Option<ReframeWindow> {
        let (width, height) = (self.recording.source_width, self.recording.source_height);
        if self.showing_insert() || !reframe::active(&self.project, width, height) {
            return None;
        }
        let base = reframe::base_size(&self.project, width, height);
        let time = self.source_position();
        self.motion.as_ref().and_then(|motion| motion.reframe_for(time, base)).or_else(|| {
            let cursor = reframe::to_crop(PointF::new(sample.cursor_x, sample.cursor_y), &self.project.crop);
            Some(reframe::provisional(cursor, sample.zoom_scale, base))
        })
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

    /// The overlays in effect at the output position, as the exporter samples them
    /// ([`overlay_track::sample_json`]), except that the selected one shows at full opacity
    /// while it is being edited (a new one starts at the playhead, where its fade-in hides it),
    /// and a blur region shows from [`overlay_track::PREVIEW_BLUR_PAD`] of output before its
    /// span to as long after ([`Document::preview_blur_shown`]).
    pub fn preview_overlays(&self) -> Vec<Object> {
        let sampled = overlay_track::sample_by(
            &self.project.overlays,
            self.source_position(),
            self.output_position,
            !self.showing_insert(),
            &|overlay| self.preview_blur_shown(overlay),
        );
        let mut items = overlay_track::to_json(&self.project.overlays, sampled);
        for item in &mut items {
            if !self.selected_overlay_id.is_empty() && item.value("id").as_str() == Some(self.selected_overlay_id.as_str()) {
                item.insert("opacity", 1.0);
            }
        }
        items
    }

    /// Whether the preview shows a blur region now: some recording that plays within
    /// [`overlay_track::PREVIEW_BLUR_PAD`] of output either side of the playhead falls in its
    /// span. The video presents its frames ahead of the overlays that follow them, and across
    /// a cut it may still show the other side's frame, so the region covers the frames either
    /// way.
    pub fn preview_blur_shown(&self, overlay: &crate::project::Overlay) -> bool {
        let pad = overlay_track::PREVIEW_BLUR_PAD;
        let (from, to) = (self.output_position - pad, self.output_position + pad);
        let mut start = 0.0;
        for clip in &self.project.clips {
            let length = (clip.out - clip.in_) / clip.speed;
            let end = start + length;
            if !clip.is_media() && end >= from && start <= to {
                let first = clip.in_ + (cpp::max(from, start) - start) * clip.speed;
                let last = clip.in_ + (cpp::min(to, end) - start) * clip.speed;
                if last >= overlay.start && first < overlay.end {
                    return true;
                }
            }
            start = end;
        }
        false
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
            overlays: self.preview_overlays(),
            camera_visible: self.camera_visible(),
        }
    }
}
