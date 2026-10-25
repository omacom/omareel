//! Where Omarchy keeps its state, and what omareel reads from there (`OmarchyPaths`).

use crate::compat::{qdir, qfile};

/// The current Omarchy state directory, or `OMAREEL_OMARCHY_STATE_DIR`.
pub fn state_root() -> String {
    match std::env::var("OMAREEL_OMARCHY_STATE_DIR") {
        Ok(path) if !path.is_empty() => qdir::absolute_path(&path),
        _ => qdir::file_path(&qdir::home_path(), ".local/state/omarchy/current"),
    }
}

/// The current theme's name, or empty.
pub fn current_theme_name() -> String {
    match qfile::read_all(qdir::file_path(&state_root(), "theme.name")) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).trim().to_owned(),
        Err(_) => String::new(),
    }
}
