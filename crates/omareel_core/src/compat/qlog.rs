//! `qWarning()` for the Rust code: Qt's message handler when the Qt side has installed one (so
//! a process without a terminal still logs to the journal), standard error otherwise.

use std::io::Write;
use std::sync::OnceLock;

static HANDLER: OnceLock<fn(&str)> = OnceLock::new();

/// Routes [`warning`] through Qt; the first handler installed stays.
pub fn set_warning_handler(handler: fn(&str)) {
    let _ = HANDLER.set(handler);
}

/// `qWarning().noquote() << message`. Never fails: a closed standard error is ignored.
pub fn warning(message: &str) {
    match HANDLER.get() {
        Some(handler) => handler(message),
        None => {
            let _ = writeln!(std::io::stderr(), "{message}");
        }
    }
}
