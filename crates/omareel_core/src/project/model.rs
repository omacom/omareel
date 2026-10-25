//! The project's data, with the C++ defaults (`Project.h`).

use crate::compat::geometry::PointF;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub mass: f64,
    pub stiffness: f64,
    pub damping: f64,
}

impl Default for Spring {
    fn default() -> Spring {
        Spring { mass: 1.0, stiffness: 100.0, damping: 20.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ZoomStyle {
    pub spring: Spring,
    pub snap_to_edges_ratio: f64,
    pub instant_animation: bool,
    pub motion_blur: f64,
}

impl Default for ZoomStyle {
    fn default() -> ZoomStyle {
        ZoomStyle {
            spring: Spring { mass: 2.25, stiffness: 200.0, damping: 40.0 },
            snap_to_edges_ratio: 0.25,
            instant_animation: false,
            motion_blur: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keystrokes {
    pub enabled: bool,
    pub position: String,
    pub size: f64,
    pub show_only_shortcuts: bool,
    pub hold_ms: i32,
}

impl Default for Keystrokes {
    fn default() -> Keystrokes {
        Keystrokes {
            enabled: false,
            position: "bottom-center".into(),
            size: 1.0,
            show_only_shortcuts: true,
            hold_ms: 900,
        }
    }
}

/// A piece of the recording, or of an inserted video when `media` is set.
/// `layout`: "default" shows the screen with the camera bubble, "camera" the webcam full
/// frame, "screen" the screen without the bubble. Inserted videos ignore it.
/// `media`: path of an inserted video, relative to the bundle and inside its media/ directory.
/// `fit`: how an inserted video meets the canvas: "fit" (whole, over a blurred copy), "fill"
/// (covering the canvas), "framed" (in the frame, over the background, like the screen).
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: String,
    pub in_: f64,
    pub out: f64,
    pub speed: f64,
    pub layout: String,
    pub media: String,
    pub fit: String,
}

impl Default for Clip {
    fn default() -> Clip {
        Clip {
            id: String::new(),
            in_: 0.0,
            out: 0.0,
            speed: 1.0,
            layout: "default".into(),
            media: String::new(),
            fit: "fit".into(),
        }
    }
}

impl Clip {
    pub fn is_media(&self) -> bool {
        !self.media.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ZoomSegment {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub level: f64,
    pub automatic_target: bool,
    pub target: PointF,
}

impl Default for ZoomSegment {
    fn default() -> ZoomSegment {
        ZoomSegment {
            id: String::new(),
            start: 0.0,
            end: 0.0,
            level: 2.0,
            automatic_target: true,
            target: PointF::new(0.5, 0.5),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    pub desktop: bool,
    pub mic: bool,
    pub volume: f64,
}

impl Default for Audio {
    fn default() -> Audio {
        Audio { desktop: true, mic: true, volume: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GifExport {
    pub fps: i32,
    pub height: i32,
    pub quality: String,
    pub loop_: bool,
}

impl Default for GifExport {
    fn default() -> GifExport {
        GifExport { fps: 15, height: 480, quality: "studio".into(), loop_: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportSettings {
    pub format: String,
    pub fps: i32,
    pub height: i32,
    pub quality: String,
    pub gif: GifExport,
}

impl Default for ExportSettings {
    fn default() -> ExportSettings {
        ExportSettings { format: "mp4".into(), fps: 60, height: 1080, quality: "social".into(), gif: GifExport::default() }
    }
}
