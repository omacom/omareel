//! Bridges omareel_core to Qt and C++. Nothing here may panic across the FFI boundary.

pub mod cli;
pub mod export;
pub mod theme;

// cxx-qt-build registers cxx-qt-lib's initializers with the binary, so the crate must be
// linked even before any bridge here uses its types.
extern crate cxx_qt_lib;
