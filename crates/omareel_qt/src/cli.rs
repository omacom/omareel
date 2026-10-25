//! Command-line entry points C++ calls.

use std::io::Write;

#[cxx_qt::bridge]
pub mod ffi {
    #[namespace = "omareel::cli"]
    extern "Rust" {
        /// Prints the help (to stdout) or a usage error (to stderr) and returns the exit status.
        fn usage(error: &str) -> i32;
        /// `omareel probe <bundle>` for an absolute bundle path: prints the summary and returns
        /// the exit status.
        fn probe(bundle: &str) -> i32;
        /// Sends the Rust code's Qt-style warnings through Qt's message handler, as the C++'s
        /// qWarning went; main() calls it first.
        #[cxx_name = "installMessageHandler"]
        fn install_message_handler();
    }
}

fn install_message_handler() {
    omareel_core::compat::qlog::set_warning_handler(|message| {
        crate::record::ffi::warning(&cxx_qt_lib::QString::from(message));
    });
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

fn probe(bundle: &str) -> i32 {
    match omareel_core::probe::run(bundle) {
        Ok(summary) => {
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(summary.as_bytes());
            let _ = out.flush();
            0
        }
        Err(error) => usage(&error),
    }
}
