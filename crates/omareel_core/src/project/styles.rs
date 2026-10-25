//! The colour-bearing parts of the project model, with the C++ defaults (`Project.h`).

use super::model::*;
use crate::compat::geometry::{PointF, RectF};
use crate::compat::qcolor::QColor;

#[derive(Clone, Debug, PartialEq)]
pub struct GradientStop {
    pub color: QColor,
    pub position: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gradient {
    pub angle: f64,
    pub stops: Vec<GradientStop>,
}

impl Default for Gradient {
    fn default() -> Gradient {
        Gradient { angle: 135.0, stops: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Background {
    pub type_: String,
    pub wallpaper: String,
    pub gradient: Gradient,
    pub color: QColor,
    pub image: String,
    pub blur: f64,
}

impl Default for Background {
    fn default() -> Background {
        Background {
            type_: "wallpaper".into(),
            wallpaper: "omarchy:current".into(),
            gradient: Gradient::default(),
            color: QColor::parse("#1a1b26"),
            image: String::new(),
            blur: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    pub enabled: bool,
    pub intensity: f64,
    pub blur: f64,
    pub distance: f64,
    pub angle: f64,
}

impl Default for Shadow {
    fn default() -> Shadow {
        Shadow { enabled: true, intensity: 0.25, blur: 40.0, distance: 10.0, angle: 90.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Border {
    pub enabled: bool,
    pub width: f64,
    pub color: QColor,
    pub alpha: f64,
}

impl Default for Border {
    fn default() -> Border {
        Border { enabled: false, width: 7.0, color: QColor::from_rgb(0, 0, 0), alpha: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub padding: f64,
    pub radius: f64,
    pub shadow: Shadow,
    pub border: Border,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame { padding: 0.10, radius: 12.0, shadow: Shadow::default(), border: Border::default() }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    pub visible: bool,
    pub size: f64,
    pub smoothing: bool,
    pub spring: Spring,
    pub click_effect: String,
    pub click_shrink: f64,
    pub rotate_on_x_movement_ratio: f64,
    /// Negative: never hide.
    pub hide_when_idle_ms: i32,
    pub style: String,
    pub ring_color: QColor,
    pub click_sound: String,
}

impl Default for Cursor {
    fn default() -> Cursor {
        Cursor {
            visible: true,
            size: 1.5,
            smoothing: true,
            spring: Spring { mass: 3.0, stiffness: 470.0, damping: 70.0 },
            click_effect: "ripple".into(),
            click_shrink: 0.8,
            rotate_on_x_movement_ratio: 0.5,
            hide_when_idle_ms: -1,
            style: "light-arrow".into(),
            ring_color: QColor::parse("#7aa2f7"),
            click_sound: "none".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Camera {
    pub enabled: bool,
    pub position: String,
    pub size: f64,
    /// round, square, or rectangle (the webcam's own aspect). `radius` rounds the corners of
    /// a square or rectangle, in pixels at 1080p.
    pub shape: String,
    pub radius: f64,
    pub flip_horizontal: bool,
    pub rotation: i32,
    /// The shadow falls down and to the right, at 45°.
    pub shadow: Shadow,
    pub border: Border,
    pub scale_during_zoom: f64,
    /// Gap from the corner to the outside of the border, as a fraction of the canvas's
    /// shorter side, so both edges get the same gap at any aspect.
    pub offset: PointF,
    /// How a clip with the camera layout shows the webcam: "fill" covers the whole canvas,
    /// "square" keeps a centred square over a blurred copy of the camera.
    pub full_frame: String,
}

impl Default for Camera {
    fn default() -> Camera {
        Camera {
            enabled: false,
            position: "bottom-right".into(),
            size: 0.25,
            shape: "round".into(),
            radius: 16.0,
            flip_horizontal: false,
            rotation: 0,
            shadow: Shadow { enabled: true, intensity: 0.28, blur: 23.0, distance: 20.0, angle: 45.0 },
            border: Border { enabled: true, width: 6.0, color: QColor::from_rgb(255, 255, 255), alpha: 0.7 },
            scale_during_zoom: 0.7,
            offset: PointF::new(0.035, 0.035),
            full_frame: "fill".into(),
        }
    }
}

/// The whole project. `Default` is a bare project, as the C++ default constructor gives;
/// [`Project::defaults`] is the starting point for a new recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub version: i32,
    pub defaults_version: i32,
    pub name: String,
    pub aspect: String,
    pub crop: RectF,
    pub clips: Vec<Clip>,
    pub zooms: Vec<ZoomSegment>,
    pub zoom_style: ZoomStyle,
    pub background: Background,
    pub frame: Frame,
    pub cursor: Cursor,
    pub keystrokes: Keystrokes,
    pub audio: Audio,
    pub camera: Camera,
    pub export_settings: ExportSettings,
}

impl Project {
    pub const CURRENT_DEFAULTS_VERSION: i32 = 4;
}

impl Default for Project {
    fn default() -> Project {
        Project {
            version: 1,
            defaults_version: Project::CURRENT_DEFAULTS_VERSION,
            name: "Untitled Recording".into(),
            aspect: "auto".into(),
            crop: RectF::new(0.0, 0.0, 1.0, 1.0),
            clips: Vec::new(),
            zooms: Vec::new(),
            zoom_style: ZoomStyle::default(),
            background: Background::default(),
            frame: Frame::default(),
            cursor: Cursor::default(),
            keystrokes: Keystrokes::default(),
            audio: Audio::default(),
            camera: Camera::default(),
            export_settings: ExportSettings::default(),
        }
    }
}
