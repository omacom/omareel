//! `QFile` reading: Qt's error strings and `readLine` semantics.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

/// `QFile::errorString()` after a failed system call: Qt's `qt_error_string`, which keeps a few
/// messages in English whatever the locale and takes the rest from `strerror`.
pub fn error_string(error: &io::Error) -> String {
    match error.raw_os_error() {
        Some(code) => crate::compat::qdir::error_string(code),
        None => error.to_string(),
    }
}

/// `QFile::open(QIODevice::ReadOnly)`, with QFile's own messages for an empty name and a
/// directory.
pub fn open_read(path: impl AsRef<Path>) -> Result<File, String> {
    let path = path.as_ref();
    if path.as_os_str().is_empty() {
        return Err("No file name specified".into());
    }
    let file = File::open(path).map_err(|e| error_string(&e))?;
    match file.metadata() {
        Ok(meta) if meta.is_dir() => Err("file to open is a directory".into()),
        _ => Ok(file),
    }
}

/// `QFile::readAll` after a successful open.
pub fn read_all(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
    let mut file = open_read(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| error_string(&e))?;
    Ok(bytes)
}

/// The lines `QIODevice::readLine` returns until `atEnd()`: each ends at and includes a
/// `\n`, and the last may lack one.
pub fn lines(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split_inclusive(|&b| b == b'\n')
}

/// `QByteArray::trimmed`: strips ASCII whitespace (space, `\t`, `\n`, `\v`, `\f`, `\r`).
pub fn trimmed(bytes: &[u8]) -> &[u8] {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let start = bytes.iter().position(|b| !space(b)).unwrap_or(bytes.len());
    let end = bytes.iter().rposition(|b| !space(b)).map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn c_path(path: &str) -> Option<std::ffi::CString> {
    std::ffi::CString::new(path).ok()
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `QFile::remove(fileName)`: unlinks the file (a directory is not removed).
pub fn remove(path: &str) -> bool {
    if path.is_empty() {
        eprintln!("QFile::remove: Empty or null file name");
        return false;
    }
    std::fs::remove_file(path).is_ok()
}

/// `QFile::setPermissions(fileName, permissions)` with the permissions as mode bits: `chmod`.
pub fn set_permissions(path: &str, mode: u32) -> bool {
    let Some(c) = c_path(path) else { return false };
    // SAFETY: a valid NUL-terminated string.
    unsafe { libc::chmod(c.as_ptr(), mode as libc::mode_t) == 0 }
}

/// `QFileSystemEngine::renameFile`: `renameat2` refusing to replace, and where a file system
/// rejects that flag, a hard link and unlink, then plain `rename` for the errors link(2) gives
/// that don't rule it out.
fn rename_file(source: &str, target: &str) -> bool {
    let (Some(from), Some(to)) = (c_path(source), c_path(target)) else { return false };
    // SAFETY: both are valid NUL-terminated strings.
    unsafe {
        if libc::renameat2(libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) == 0 {
            return true;
        }
        if errno() != libc::EINVAL {
            return false;
        }
        if libc::link(from.as_ptr(), to.as_ptr()) == 0 {
            if libc::unlink(from.as_ptr()) == 0 {
                return true;
            }
            libc::unlink(to.as_ptr());
            return false;
        }
        match errno() {
            libc::EACCES | libc::EEXIST | libc::ENAMETOOLONG | libc::ENOENT | libc::ENOTDIR | libc::EROFS
            | libc::EXDEV => false,
            _ => libc::rename(from.as_ptr(), to.as_ptr()) == 0,
        }
    }
}

/// `QFile::rename(oldName, newName)`: never replaces an existing file (one that `stat` finds,
/// so through symlinks), and fails for a source that does not exist. Not reproduced: Qt's
/// fallback of copying the file when the system cannot rename it, and the two-step rename for a
/// name that only changes case on a case-insensitive file system; omareel renames within one
/// directory, where neither applies.
pub fn rename(old_name: &str, new_name: &str) -> bool {
    if old_name.is_empty() {
        eprintln!("QFile::rename: Empty or null file name");
        return false;
    }
    if old_name == new_name || !crate::compat::qdir::file_exists(old_name) {
        return false;
    }
    if std::fs::metadata(new_name).is_ok() {
        return false;
    }
    rename_file(old_name, new_name)
}

/// `QFile::copy(fileName, newName)` as Qt 6.11 does it on Linux: never over an existing file
/// (one `stat` finds, so a dangling symlink does not count), the source opened for reading (a
/// directory or an unreadable file fails), and the copy written through a `QSaveFile` with the
/// direct write fallback, so it follows a symlink at `new_name` and appears whole or not at
/// all. The copy gets the source's permissions as `QFile::permissions` reports them. Not
/// reproduced: the error strings (omareel only looks at the result).
pub fn copy(file_name: &str, new_name: &str) -> bool {
    if file_name.is_empty() {
        eprintln!("QFile::copy: Empty or null file name");
        return false;
    }
    if crate::compat::qdir::file_exists(new_name) {
        return false;
    }
    // QFSFileEngine::copy is not implemented on Linux, so QFile copies the data itself.
    let Ok(mut source) = open_read(file_name) else { return false };
    let mut out = crate::compat::qsavefile::SaveFile::new(new_name);
    out.set_direct_write_fallback(true);
    if !out.open() {
        return false;
    }
    let mut block = [0u8; 4096];
    loop {
        match source.read(&mut block) {
            Ok(0) => break,
            Ok(n) => {
                if out.write(&block[..n]) != n as i64 {
                    return false;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    out.set_permissions(crate::compat::qdir::FileInfo::new(file_name).permissions_mode());
    drop(source);
    out.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_like_qt() {
        assert_eq!(open_read("/nonexistent/omareel").unwrap_err(), "No such file or directory");
        assert_eq!(open_read("/").unwrap_err(), "file to open is a directory");
        assert_eq!(open_read("").unwrap_err(), "No file name specified");
    }

    #[test]
    fn rename_never_replaces() {
        let dir = std::env::temp_dir().join(format!("omareel-qfile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b) = (dir.join("a"), dir.join("b"));
        let (a, b) = (a.to_str().unwrap(), b.to_str().unwrap());
        std::fs::write(a, "a").unwrap();
        std::fs::write(b, "b").unwrap();
        assert!(!rename(a, b));
        assert!(remove(b) && !remove(b));
        assert!(rename(a, b) && !rename(a, b));
        assert_eq!(std::fs::read(b).unwrap(), b"a");
        assert!(set_permissions(b, 0o644));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn copy_keeps_permissions_follows_links_and_never_replaces() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("omareel-qfile-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let path = |name: &str| dir.join(name).to_str().unwrap().to_string();
        std::fs::write(path("a"), "contents").unwrap();
        std::fs::set_permissions(path("a"), std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(copy(&path("a"), &path("b")));
        assert_eq!(std::fs::read(path("b")).unwrap(), b"contents");
        assert_eq!(std::fs::metadata(path("b")).unwrap().permissions().mode() & 0o7777, 0o640);
        // Never over an existing file; a dangling symlink is followed and its target created.
        std::fs::write(path("c"), "old").unwrap();
        assert!(!copy(&path("a"), &path("c")));
        assert_eq!(std::fs::read(path("c")).unwrap(), b"old");
        std::os::unix::fs::symlink("target", path("sub/link")).unwrap();
        assert!(copy(&path("a"), &path("sub/link")));
        assert_eq!(std::fs::read(path("sub/target")).unwrap(), b"contents");
        assert!(std::fs::symlink_metadata(path("sub/link")).unwrap().file_type().is_symlink());
        // A directory or a missing file is not copied; a missing destination directory fails.
        assert!(!copy(&path("sub"), &path("d")));
        assert!(!copy(&path("missing"), &path("d")));
        assert!(!copy(&path("a"), &path("missing/d")));
        assert!(!std::path::Path::new(&path("d")).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lines_keep_their_newlines_and_trim() {
        let all: Vec<_> = lines(b"a\n\n b \r\nlast").collect();
        assert_eq!(all, [&b"a\n"[..], b"\n", b" b \r\n", b"last"]);
        assert_eq!(trimmed(b" \t b \r\n"), b"b");
        assert_eq!(trimmed(b" \n"), b"");
    }
}
