//! omareel's program logic, free of Qt so any front end can use it. Where a behaviour came
//! from a Qt class (number formatting, JSON, colours, paths), `compat` reproduces it exactly,
//! because the files omareel writes must stay byte-identical.

pub mod camera_timeline;
pub mod cli;
pub mod clip_timeline;
pub mod compat;
pub mod cursor_path;
pub mod easing;
pub mod input_log;
pub mod keystroke_track;
pub mod layout_timeline;
pub mod motion_track;
pub mod omarchy_paths;
pub mod probe;
pub mod project;
pub mod theme;
pub mod zoom_timeline;
