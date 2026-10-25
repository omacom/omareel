//! Command-line entry points C++ calls.

use std::io::Write;

#[cxx_qt::bridge]
pub mod ffi {
    #[namespace = "omareel::cli"]
    extern "Rust" {
        /// Prints the help (to stdout) or a usage error (to stderr) and returns the exit status.
        fn usage(error: &str) -> i32;
    }
}

fn usage(error: &str) -> i32 {
    let text = omareel_core::cli::usage_text(error);
    if error.is_empty() {
        let _ = std::io::stdout().lock().write_all(text.as_bytes());
    } else {
        let _ = std::io::stderr().lock().write_all(text.as_bytes());
    }
    omareel_core::cli::usage_status(error)
}
