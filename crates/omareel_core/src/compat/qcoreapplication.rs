//! What `QCoreApplication` says about the running program on Linux.

use super::qdir::{self, FileInfo};

/// `QCoreApplication::applicationFilePath()`: the target of `/proc/self/exe`, which the kernel
/// has already made absolute and canonical (decoded as `QFile::decodeName` does; a binary
/// replaced since it started ends in " (deleted)"). Empty when the link can't be read; Qt would
/// then look at `argv[0]`, which a program started through `/proc` never needs.
///
/// Read once and kept, as Qt keeps it, so a binary replaced while the program runs (a package
/// upgrade) is still found under its path.
pub fn application_file_path() -> String {
    use std::os::unix::ffi::OsStrExt;
    static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATH.get_or_init(|| match std::fs::read_link("/proc/self/exe") {
        Ok(target) => qdir::decode_name(target.as_os_str().as_bytes()),
        Err(_) => String::new(),
    })
    .clone()
}

/// `QCoreApplication::applicationDirPath()`: the directory the program is in.
pub fn application_dir_path() -> String {
    FileInfo::new(application_file_path()).path()
}

/// `QCoreApplication::applicationPid()`.
pub fn application_pid() -> i64 {
    i64::from(std::process::id())
}
