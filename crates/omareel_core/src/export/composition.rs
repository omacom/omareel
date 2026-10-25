//! What the exporter tells the QML composition at each frame (`CompositionState` in
//! `Exporter.h`, filled by `Exporter::run`), as plain data. The C++ copies it into the
//! `QVariantMap`s Composition.qml reads.
//!
//! This is the exporter's variant, which differs from the editor's preview on purpose (see
//! tests/golden/README.md, "composition"): during an inserted video the cursor keeps the
//! project's `visible` and the zoom resets, the media map has no `missing` key and keeps the
//! last insert's fit and size once a recording clip is back, and `camera_visible` does not ask
//! whether there is a camera.

use crate::camera_timeline::map_camera_time;
use crate::clip_timeline::ClipTimeline;
use crate::keystroke_track::{KeystrokePill, KeystrokeTrack};
use crate::layout_timeline::{self, sample_layout};
use crate::media_probe::VideoInfo;
use crate::motion_track::{MotionSample, MotionTrack, RippleSample};
use crate::project::{Clip, Project};
use std::collections::HashMap;

/// The `zoom` map: scale and centre of the zoom, and how fast the centre moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
    pub velocity: f64,
}

/// The `layout` map: how far the webcam has grown to full frame, how visible its bubble is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub camera_full: f64,
    pub bubble: f64,
}

/// The `media` map: the inserted video on screen, if any. `width` and `height` are ints.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub active: bool,
    pub ready: bool,
    pub fit: String,
    pub width: i32,
    pub height: i32,
}

/// The `cursor` map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    pub x: f64,
    pub y: f64,
    pub visible: bool,
    pub scale: f64,
    pub opacity: f64,
    pub rotation: f64,
}

/// `CompositionState` without the two members only Qt has: the project map, which the C++
/// builds from the project's JSON, and whether the scene graph renders in software.
#[derive(Clone, Debug, PartialEq)]
pub struct CompositionState {
    pub output_width: i32,
    pub output_height: i32,
    pub source_width: i32,
    pub source_height: i32,
    /// Physical pixels per logical pixel at capture time.
    pub capture_scale: f64,
    pub camera_available: bool,
    pub camera_visible: bool,
    pub camera_source_width: i32,
    pub camera_source_height: i32,
    pub time: f64,
    pub zoom: Zoom,
    pub layout: Layout,
    pub media: Media,
    pub cursor: Cursor,
    /// Maps of `x`, `y` and `progress`.
    pub ripples: Vec<RippleSample>,
    /// Maps of `keys` (a string list), `text` and `opacity`.
    pub keystroke_pills: Vec<KeystrokePill>,
}

impl CompositionState {
    /// The state the composition starts from, which Composition.qml reads as it loads: the
    /// C++ members' defaults and the maps `Exporter::run` sets before `ExportRenderer::init`.
    pub fn initial(project: &Project) -> CompositionState {
        CompositionState {
            output_width: 0,
            output_height: 0,
            source_width: 0,
            source_height: 0,
            capture_scale: 1.0,
            camera_available: false,
            camera_visible: true,
            camera_source_width: 0,
            camera_source_height: 0,
            time: 0.0,
            zoom: Zoom { scale: 1.0, cx: 0.5, cy: 0.5, velocity: 0.0 },
            layout: Layout { camera_full: 0.0, bubble: 1.0 },
            media: Media { active: false, ready: true, fit: "fit".into(), width: 16, height: 9 },
            cursor: Cursor {
                x: 0.5,
                y: 0.5,
                visible: project.cursor.visible,
                scale: 1.0,
                opacity: if project.cursor.visible { 1.0 } else { 0.0 },
                rotation: 0.0,
            },
            ripples: Vec::new(),
            keystroke_pills: Vec::new(),
        }
    }

    /// One frame of the export at `output_time`, showing `clip` (the clip the decoder decoded
    /// for it). Everything but `media` is replaced; a recording frame only turns `media` off.
    pub fn export_frame(&mut self, sources: &Sources, clip: &Clip, output_time: f64) {
        let insert = clip.is_media();
        let project = sources.project;
        // An inserted clip shows none of the recording's motion, cursor, keys, or camera.
        let source_time = sources.timeline.recording_source_time(output_time);
        let sample = if insert { MotionSample::default() } else { sources.motion.sample(source_time) };
        self.time = source_time;
        self.zoom =
            Zoom { scale: sample.zoom_scale, cx: sample.zoom_cx, cy: sample.zoom_cy, velocity: sample.zoom_center_velocity };
        self.cursor = Cursor {
            x: sample.cursor_x,
            y: sample.cursor_y,
            visible: project.cursor.visible,
            scale: sample.cursor_scale,
            opacity: sample.cursor_opacity,
            rotation: sample.cursor_rotation,
        };
        self.ripples.clear();
        if !insert && (project.cursor.click_effect == "ripple" || project.cursor.click_effect == "circle") {
            self.ripples.extend(sources.motion.ripples(source_time));
        }
        self.keystroke_pills = if insert { Vec::new() } else { sources.keystrokes.sample(source_time) };
        let layout = sample_layout(
            &project.clips,
            output_time,
            sources.camera_width > 0,
            project.camera.enabled,
            layout_timeline::DEFAULT_TRANSITION,
        );
        self.layout = Layout { camera_full: layout.camera_full, bubble: layout.bubble };
        let camera_time = map_camera_time(source_time, sources.camera_offset, sources.camera_duration);
        self.camera_visible = !insert && (!camera_time.before_start || layout.camera_full > 0.0);
        if insert {
            let info = sources.inserts.get(&clip.media).copied().unwrap_or_default();
            self.media =
                Media { active: true, ready: true, fit: clip.fit.clone(), width: info.width, height: info.height };
        } else {
            self.media.active = false;
        }
    }
}

/// What a frame's state is computed from.
pub struct Sources<'a> {
    pub timeline: &'a ClipTimeline,
    pub project: &'a Project,
    pub motion: &'a MotionTrack,
    pub keystrokes: &'a KeystrokeTrack,
    /// The webcam video's width as probed, 0 without one.
    pub camera_width: i32,
    pub camera_duration: f64,
    pub camera_offset: f64,
    pub inserts: &'a HashMap<String, VideoInfo>,
}
