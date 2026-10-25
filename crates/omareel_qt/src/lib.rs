//! Bridges omareel_core and omareel_record to Qt and C++. Nothing here may panic across the FFI
//! boundary.

pub mod cli;
pub mod export;
pub mod launcher;
pub mod recording_bar;
mod support;
pub mod theme;

// cxx-qt-build registers cxx-qt-lib's initializers with the binary, so the crate must be
// linked even before any bridge here uses its types.
extern crate cxx_qt_lib;
