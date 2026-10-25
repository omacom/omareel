//! `QSaveFile` on Linux (Qt 6.11): write to a temporary file in the target's directory, then
//! put it in place, so readers see either the old file or the whole new one.
//!
//! What Qt does, reproduced here:
//! - `open` refuses an existing file the user may not write ("Existing file … is not
//!   writable") and a directory ("Filename refers to a directory"), following symlinks for both.
//! - A symlink is followed (up to 127 links, each target resolved from where its link is) and the
//!   file it ends at is the one replaced, so the link survives; a dangling symlink creates its
//!   target. A loop of links is not followed: the link itself is replaced by a regular file.
//! - The temporary file is unnamed (`O_TMPFILE` in the directory of the cleaned name) where the
//!   file system allows it, otherwise "<name>.XXXXXX" beside it (a name with its own run of six
//!   or more X's is its own template). A missing or unwritable directory makes `open` fail with
//!   the system's message ("No such file or directory", "Permission denied", "Not a directory").
//! - A new file gets mode 0666 less the umask. Replacing a file keeps its permission bits (not
//!   setuid, setgid or sticky), plus owner read, write and execute bits for whatever the current
//!   user may do with the old file (Qt's `QFileInfo::permissions` to `chmod` round trip); the owner
//!   becomes the current user.
//! - Writes are buffered in 16 KiB like `QFileDevice`; a larger write goes straight to the file.
//!   `commit` flushes, calls `fdatasync` (ignoring its result), then links the unnamed file in as
//!   the target (the name as given, not cleaned) if there is none, or links it under a temporary
//!   name and renames that over the target. A write, flush or rename error makes `commit` return
//!   false, leave the target untouched and remove the temporary file. But as in Qt, a write the
//!   system takes only part of (a full disk, a file size limit) is not an error when nothing was
//!   buffered: `write` returns the shorter count and `commit` installs the truncated file.
//! - `cancel_writing` makes a later `commit` discard everything; dropping an uncommitted
//!   `SaveFile` discards it too.
//!
//! [`SaveFile::error_string`] has Qt's messages: its own text for permission denied, no such
//! file, no space and too many open files, the C library's `strerror` for the rest, and "Unknown
//! error" where Qt has none: after a successful `commit`, after a `commit` that follows
//! `cancel_writing` or a failed large write (Qt's close clears the message first), and after a
//! partly failed flush.
//!
//! Checked against Qt 6.11.2 with no differences in results, messages, contents, modes, links or
//! leftover files over 52 scenarios: new and existing files with modes 0644, 0600, 0640, 0604,
//! 0755, 0444, 0000, 4755 and 1777 and three umasks; symlinks relative, absolute, chained,
//! dangling and looping; missing and read-only directories, directories and FIFOs as targets,
//! trailing slashes, "missing/../name", names containing "XXXXXX"; empty, small and 40 KB writes,
//! cancelled writes, drops and repeated commits; and writes cut short by a file size limit.
//! Not reproduced: `setDirectWriteFallback` (omareel doesn't use it) and the random letters of
//! a temporary name.

use super::qdir::{self, FileInfo};
use std::ffi::CString;

/// `QFILE_WRITEBUFFER_SIZE`.
const WRITE_BUFFER_CHUNK: usize = 16384;

/// `QFileDevice::FileError`, as far as the messages need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileError {
    NoError,
    Write,
    Resource,
    Open,
    Rename,
    Permissions,
    Unspecified,
}

/// `QTemporaryFileName`: the template path with a run of at least six "X" to replace.
struct TemplateName {
    path: Vec<u8>,
    pos: usize,
    len: usize,
}

impl TemplateName {
    fn new(name: &str) -> TemplateName {
        let mut q = name.to_string();
        let b = q.as_bytes();
        let (mut pos, mut len) = (b.len(), 0usize);
        while pos != 0 {
            pos -= 1;
            if b[pos] == b'X' {
                len += 1;
                continue;
            }
            if len >= 6 || b[pos] == b'/' {
                break;
            }
            len = 0;
        }
        if len < 6 {
            q.push_str(".XXXXXX");
        }
        let path = qdir::clean_path(&q).into_bytes();
        let (mut pos, mut len) = (path.len(), 0usize);
        while pos != 0 {
            pos -= 1;
            if path[pos] == b'X' {
                len += 1;
                continue;
            }
            if len >= 6 {
                pos += 1;
                break;
            }
            len = 0;
        }
        TemplateName { path, pos, len }
    }

    /// `generateNext`: the placeholder replaced by random letters.
    fn generate(&mut self) -> Vec<u8> {
        let mut random = [0u8; 64];
        let filled = random_bytes(&mut random[..self.len.min(64)]);
        for i in 0..self.len {
            let fallback = (i * 31 + std::process::id() as usize) as u8;
            let r = if filled { random[i % 64] } else { fallback };
            let v = (u32::from(r) * 52) >> 8;
            self.path[self.pos + i] = if v < 26 { b'A' + v as u8 } else { b'a' + (v - 26) as u8 };
        }
        self.path.clone()
    }

    /// The directory `O_TMPFILE` opens: up to the last "/", "/" for a root child, "." for none.
    fn dir(&self) -> Vec<u8> {
        match self.path.iter().rposition(|&c| c == b'/') {
            None => b".".to_vec(),
            Some(0) => b"/".to_vec(),
            Some(i) => self.path[..i].to_vec(),
        }
    }
}

fn random_bytes(buf: &mut [u8]) -> bool {
    // SAFETY: the buffer is valid for its length.
    let n = unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
    n == buf.len() as isize
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `qt_haveLinuxProcfs`: an unnamed file can only be linked in through /proc/self/fd.
fn have_procfs() -> bool {
    static PRESENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // SAFETY: a valid NUL-terminated string.
    *PRESENT.get_or_init(|| unsafe { libc::access(c"/proc/version".as_ptr(), libc::F_OK) == 0 })
}

/// `qt_safe_open`: `open` with `O_CLOEXEC`, retried on EINTR.
fn open_raw(path: &[u8], flags: libc::c_int, mode: u32) -> Result<libc::c_int, i32> {
    let Ok(c) = CString::new(path) else { return Err(libc::EINVAL) };
    loop {
        // SAFETY: `c` is a valid NUL-terminated string.
        let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC, mode as libc::c_uint) };
        if fd >= 0 {
            return Ok(fd);
        }
        let e = errno();
        if e != libc::EINTR {
            return Err(e);
        }
    }
}

fn linkat_fd(fd: libc::c_int, target: &[u8]) -> Result<(), i32> {
    let src = CString::new(format!("/proc/self/fd/{fd}")).map_err(|_| libc::EINVAL)?;
    let dst = CString::new(target).map_err(|_| libc::EINVAL)?;
    // SAFETY: both are valid NUL-terminated strings.
    let rc = unsafe {
        let follow = libc::AT_SYMLINK_FOLLOW;
        libc::linkat(libc::AT_FDCWD, src.as_ptr(), libc::AT_FDCWD, dst.as_ptr(), follow)
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

fn rename_raw(from: &[u8], to: &[u8]) -> Result<(), i32> {
    let from = CString::new(from).map_err(|_| libc::EINVAL)?;
    let to = CString::new(to).map_err(|_| libc::EINVAL)?;
    // SAFETY: both are valid NUL-terminated strings.
    if unsafe { libc::rename(from.as_ptr(), to.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}

/// `QTemporaryFileEngine` as `QSaveFile` drives it.
struct Engine {
    fd: libc::c_int,
    /// `None` while the file is unnamed, else the path it has in the file system.
    named: Option<Vec<u8>>,
    /// The final file name, which is also the engine's template name.
    template: String,
    error: (FileError, String),
}

impl Engine {
    /// `QTemporaryFileEngine::open` for `template` with the given creation mode.
    fn open(template: &str, mode: u32) -> Result<Engine, String> {
        let mut name = TemplateName::new(template);
        let engine = |fd, named| Engine {
            fd,
            named,
            template: template.to_string(),
            error: (FileError::Unspecified, String::new()),
        };
        if have_procfs() {
            match open_raw(&name.dir(), libc::O_TMPFILE | libc::O_RDWR, mode) {
                Ok(fd) => return Ok(engine(fd, None)),
                Err(e) if e == libc::EOPNOTSUPP || e == libc::EISDIR => {}
                Err(e) => return Err(qdir::error_string(e)),
            }
        }
        for _ in 0..16 {
            let path = name.generate();
            match open_raw(&path, libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, mode) {
                Ok(fd) => return Ok(engine(fd, Some(path))),
                Err(libc::EEXIST) => continue,
                Err(e) => return Err(qdir::error_string(e)),
            }
        }
        Err("No error".into())
    }

    /// `QFSFileEnginePrivate::writeFdFh`: everything, or as much as the system took; -1 and an
    /// error only if nothing was written.
    fn write(&mut self, data: &[u8]) -> i64 {
        let mut written = 0usize;
        let mut err = 0;
        while written < data.len() {
            let rest = &data[written..];
            // SAFETY: `rest` is valid for its length; `fd` is open.
            let r = unsafe { libc::write(self.fd, rest.as_ptr().cast(), rest.len()) };
            if r < 0 {
                err = errno();
                if err == libc::EINTR {
                    continue;
                }
                break;
            }
            if r == 0 {
                break;
            }
            written += r as usize;
        }
        if !data.is_empty() && written == 0 {
            let kind = if err == libc::ENOSPC { FileError::Resource } else { FileError::Write };
            self.error = (kind, qdir::error_string(err));
            return -1;
        }
        written as i64
    }

    fn sync(&mut self) {
        // SAFETY: `fd` is open.
        while unsafe { libc::fdatasync(self.fd) } != 0 && errno() == libc::EINTR {}
    }

    fn close(&mut self) {
        if self.fd >= 0 {
            // SAFETY: `fd` is open and closed once.
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
    }

    /// `QTemporaryFileEngine::remove`: an unnamed file just goes away with its descriptor.
    fn remove(&mut self) {
        self.close();
        if let Some(path) = self.named.take() {
            if let Ok(c) = CString::new(path) {
                // SAFETY: a valid NUL-terminated string.
                unsafe { libc::unlink(c.as_ptr()) };
            }
        }
    }

    /// `QTemporaryFileEngine::renameOverwrite(target)`.
    fn rename_overwrite(&mut self, target: &str) -> bool {
        let target = target.as_bytes();
        if self.named.is_none() {
            // materializeUnnamedFile(target, Overwrite)
            let ok = match linkat_fd(self.fd, target) {
                Ok(()) => true,
                Err(libc::EEXIST) => {
                    let mut name = TemplateName::new(&self.template);
                    let mut linked = None;
                    for _ in 0..16 {
                        let path = name.generate();
                        if linkat_fd(self.fd, &path).is_ok() {
                            linked = Some(path);
                            break;
                        }
                    }
                    match linked {
                        None => false,
                        Some(path) => {
                            self.named = Some(path.clone());
                            self.close();
                            self.rename_named(&path, target)
                        }
                    }
                }
                Err(e) => {
                    self.error = (FileError::Rename, qdir::error_string(e));
                    false
                }
            };
            self.close();
            return ok;
        }
        self.close();
        let path = self.named.clone().unwrap_or_default();
        self.rename_named(&path, target)
    }

    fn rename_named(&mut self, path: &[u8], target: &[u8]) -> bool {
        match rename_raw(path, target) {
            Ok(()) => {
                self.named = None;
                true
            }
            Err(e) => {
                self.error = (FileError::Rename, qdir::error_string(e));
                false
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.close();
    }
}

/// `QSaveFile`.
pub struct SaveFile {
    file_name: String,
    final_file_name: String,
    engine: Option<Engine>,
    open: bool,
    buffer: Vec<u8>,
    error: FileError,
    error_text: String,
    write_error: FileError,
}

impl SaveFile {
    /// `QSaveFile(name)`.
    pub fn new(file_name: impl Into<String>) -> SaveFile {
        SaveFile {
            file_name: file_name.into(),
            final_file_name: String::new(),
            engine: None,
            open: false,
            buffer: Vec::new(),
            error: FileError::NoError,
            error_text: String::new(),
            write_error: FileError::NoError,
        }
    }

    /// `fileName()`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// `isOpen()`.
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn set_error(&mut self, error: FileError, text: String) {
        self.error = error;
        self.error_text = text;
    }

    fn unset_error(&mut self) {
        self.set_error(FileError::NoError, String::new());
    }

    /// `errorString()`: the last error's message, "Unknown error" if there is none.
    pub fn error_string(&self) -> String {
        if self.error_text.is_empty() {
            "Unknown error".into()
        } else {
            self.error_text.clone()
        }
    }

    /// `open(QIODevice::WriteOnly)` (`QIODevice::Text` changes nothing on Linux).
    pub fn open(&mut self) -> bool {
        if self.open {
            return false;
        }
        self.unset_error();
        self.write_error = FileError::NoError;
        let mut existing = FileInfo::new(self.file_name.clone());
        if existing.exists() && !existing.is_writable() {
            let text = format!("Existing file {} is not writable", self.file_name);
            self.set_error(FileError::Write, text);
            self.write_error = FileError::Write;
            return false;
        }
        if existing.is_dir() {
            self.set_error(FileError::Write, "Filename refers to a directory".into());
            self.write_error = FileError::Write;
            return false;
        }
        self.final_file_name = self.file_name.clone();
        if existing.is_symlink() {
            let mut max_depth = 128;
            loop {
                max_depth -= 1;
                if max_depth == 0 || !existing.is_symlink() {
                    break;
                }
                existing = FileInfo::new(existing.sym_link_target());
            }
            if max_depth > 0 {
                self.final_file_name = existing.file_path().to_string();
            }
        }
        let exists = existing.exists();
        match Engine::open(&self.final_file_name, if exists { 0o600 } else { 0o666 }) {
            Ok(engine) => self.engine = Some(engine),
            Err(text) => {
                self.engine = None;
                self.set_error(FileError::Open, text);
                return false;
            }
        }
        self.open = true;
        self.buffer.clear();
        if exists {
            self.set_permissions(existing.permissions_mode());
        }
        true
    }

    /// `QFileDevice::setPermissions` on the open temporary file.
    fn set_permissions(&mut self, mode: u32) {
        let Some(engine) = &self.engine else { return };
        // SAFETY: `fd` is open.
        if unsafe { libc::fchmod(engine.fd, mode as libc::mode_t) } == 0 {
            self.unset_error();
        } else {
            let text = qdir::error_string(errno());
            self.set_error(FileError::Permissions, text);
        }
    }

    /// `write(data)`: the number of bytes taken, or -1 if the file isn't open or a write
    /// failed (now or earlier).
    pub fn write(&mut self, data: &[u8]) -> i64 {
        if !self.open {
            return -1;
        }
        if self.write_error != FileError::NoError {
            return -1;
        }
        let ret = self.write_data(data);
        if self.error != FileError::NoError {
            self.write_error = self.error;
        }
        ret
    }

    /// `QFileDevice::writeData`, buffered.
    fn write_data(&mut self, data: &[u8]) -> i64 {
        self.unset_error();
        if self.buffer.len() + data.len() > WRITE_BUFFER_CHUNK && !self.flush() {
            return -1;
        }
        if data.len() > WRITE_BUFFER_CHUNK {
            let Some(engine) = self.engine.as_mut() else { return -1 };
            let ret = engine.write(data);
            if ret < 0 {
                let (mut kind, text) = engine.error.clone();
                if kind == FileError::Unspecified {
                    kind = FileError::Write;
                }
                self.set_error(kind, text);
            }
            return ret;
        }
        self.buffer.extend_from_slice(data);
        data.len() as i64
    }

    /// `QFileDevice::flush`.
    fn flush(&mut self) -> bool {
        let Some(engine) = self.engine.as_mut() else { return false };
        if !self.buffer.is_empty() {
            let size = self.buffer.len();
            let written = engine.write(&self.buffer);
            if written > 0 {
                self.buffer.drain(..written as usize);
            }
            if written != size as i64 {
                let (mut kind, text) = engine.error.clone();
                if kind == FileError::Unspecified {
                    kind = FileError::Write;
                }
                self.set_error(kind, text);
                return false;
            }
        }
        true
    }

    /// `cancelWriting()`: a later `commit` discards the file.
    pub fn cancel_writing(&mut self) {
        if !self.open {
            return;
        }
        self.set_error(FileError::Write, "Writing canceled by application".into());
        self.write_error = FileError::Write;
    }

    /// `commit()`: true if everything was written and the file is now in place.
    pub fn commit(&mut self) -> bool {
        if self.engine.is_none() || !self.open {
            return false;
        }
        // QFileDevice::close(): flush, then the engine's close (which only rewinds and clears
        // its error), keeping a flush error.
        let flushed = self.flush();
        self.open = false;
        self.buffer.clear();
        let Some(mut engine) = self.engine.take() else { return false };
        engine.error = (FileError::Unspecified, String::new());
        if flushed {
            self.unset_error();
        }
        engine.sync();
        if self.error == FileError::NoError {
            self.error = self.write_error;
        }
        self.write_error = FileError::NoError;
        if self.error != FileError::NoError {
            engine.remove();
            return false;
        }
        if !engine.rename_overwrite(&self.final_file_name) {
            let (kind, text) = engine.error.clone();
            self.set_error(kind, text);
            engine.remove();
            return false;
        }
        self.error == FileError::NoError
    }
}

impl Drop for SaveFile {
    fn drop(&mut self) {
        if self.open {
            if let Some(engine) = self.engine.as_mut() {
                engine.remove();
            }
        }
    }
}

/// What `Project::save` and the other omareel savers do: open, write, commit, and on failure
/// the error string they report.
pub fn save(path: &str, data: &[u8]) -> Result<(), String> {
    let mut file = SaveFile::new(path);
    if !file.open() {
        return Err(file.error_string());
    }
    if file.write(data) < 0 || !file.commit() {
        return Err(file.error_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &str) -> u32 {
        std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
    }

    fn read(path: &str) -> String {
        String::from_utf8(std::fs::read(path).unwrap()).unwrap()
    }

    fn entries(dir: &str) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    fn write_mode(path: &str, contents: &str, mode: u32) {
        std::fs::write(path, contents).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn saves_new_and_existing_files() {
        let dir = TempDir::new("savefile");
        let f = dir.join("f.json");
        assert_eq!(save(&f, b"{}\n"), Ok(()));
        assert_eq!(read(&f), "{}\n");
        // A new file gets the mode any new file gets (0666 less the umask).
        std::fs::write(dir.join("reference"), b"").unwrap();
        assert_eq!(mode(&f), mode(&dir.join("reference")));

        // Existing files keep their permission bits, but not setuid.
        for (before, after) in [(0o640, 0o640), (0o600, 0o600), (0o755, 0o755), (0o4755, 0o755), (0o604, 0o604)] {
            write_mode(&f, "old", before);
            assert_eq!(save(&f, b"new"), Ok(()), "{before:o}");
            assert_eq!((read(&f), mode(&f)), ("new".to_string(), after), "{before:o}");
        }
        assert_eq!(save(&f, b""), Ok(()));
        assert_eq!(read(&f), "");
        let big = "x".repeat(40_000);
        assert_eq!(save(&f, big.as_bytes()), Ok(()));
        assert_eq!(read(&f), big);
        // A name with a run of X's is its own temporary-file template.
        assert_eq!(save(&dir.join("fooXXXXXXbar.json"), b"x"), Ok(()));
        assert_eq!(entries(&dir.0), ["f.json", "fooXXXXXXbar.json", "reference"]);
    }

    #[test]
    fn refuses_what_qt_refuses() {
        let dir = TempDir::new("savefile-refuse");
        let f = dir.join("ro");
        write_mode(&f, "old", 0o444);
        if !FileInfo::new(&f).is_writable() {
            assert_eq!(save(&f, b"new"), Err(format!("Existing file {f} is not writable")));
            assert_eq!(read(&f), "old");
        }
        std::fs::create_dir(dir.join("d")).unwrap();
        assert_eq!(save(&dir.join("d"), b"x"), Err("Filename refers to a directory".into()));
        std::os::unix::fs::symlink("d", dir.join("dlink")).unwrap();
        assert_eq!(save(&dir.join("dlink"), b"x"), Err("Filename refers to a directory".into()));
        assert_eq!(save(&dir.join("missing/f"), b"x"), Err("No such file or directory".into()));
        std::fs::write(dir.join("plain"), b"old").unwrap();
        assert_eq!(save(&format!("{}/", dir.join("plain")), b"x"), Err("Not a directory".into()));
        // The temporary file is made where the cleaned name points; the link goes to the name as
        // given, which fails when "missing" doesn't exist.
        assert_eq!(save(&dir.join("missing/../late"), b"x"), Err("No such file or directory".into()));
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        if !FileInfo::new(&locked).is_writable() {
            assert_eq!(save(&format!("{locked}/f"), b"x"), Err("Permission denied".into()));
        }
        assert_eq!(entries(&dir.0), ["d", "dlink", "locked", "plain", "ro"]);
    }

    #[test]
    fn follows_symlinks_to_the_file_they_name() {
        let dir = TempDir::new("savefile-links");
        std::fs::create_dir(dir.join("other")).unwrap();
        write_mode(&dir.join("other/t"), "old", 0o640);
        std::os::unix::fs::symlink("other/t", dir.join("rel")).unwrap();
        assert_eq!(save(&dir.join("rel"), b"via link"), Ok(()));
        assert!(FileInfo::new(dir.join("rel")).is_symlink());
        assert_eq!((read(&dir.join("other/t")), mode(&dir.join("other/t"))), ("via link".into(), 0o640));
        // A chain, each link resolved from where it is.
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::os::unix::fs::symlink("../other/t", dir.join("sub/l2")).unwrap();
        std::os::unix::fs::symlink("sub/l2", dir.join("chain")).unwrap();
        assert_eq!(save(&dir.join("chain"), b"via chain"), Ok(()));
        assert_eq!(read(&dir.join("other/t")), "via chain");
        // A dangling link creates its target; a loop is replaced by a regular file.
        std::os::unix::fs::symlink("other/new", dir.join("dangling")).unwrap();
        assert_eq!(save(&dir.join("dangling"), b"created"), Ok(()));
        assert_eq!(read(&dir.join("other/new")), "created");
        std::os::unix::fs::symlink("g", dir.join("loop")).unwrap();
        std::os::unix::fs::symlink("loop", dir.join("g")).unwrap();
        assert_eq!(save(&dir.join("loop"), b"loop"), Ok(()));
        assert!(!FileInfo::new(dir.join("loop")).is_symlink());
        assert_eq!(read(&dir.join("loop")), "loop");
    }

    #[test]
    fn cancel_drop_and_commit_twice() {
        let dir = TempDir::new("savefile-cancel");
        let f = dir.join("f");
        std::fs::write(&f, b"old").unwrap();
        let mut file = SaveFile::new(f.clone());
        assert!(file.open() && file.is_open());
        assert!(!file.open());
        assert_eq!(file.write(b"new"), 3);
        file.cancel_writing();
        assert_eq!(file.write(b"more"), -1);
        // Qt's close clears the message before commit notices the cancellation.
        assert!(!file.commit());
        assert_eq!(file.error_string(), "Unknown error");
        assert_eq!(read(&f), "old");

        {
            let mut dropped = SaveFile::new(dir.join("never"));
            assert!(dropped.open());
            assert_eq!(dropped.write(b"x"), 1);
        }
        let mut twice = SaveFile::new(f.clone());
        assert!(twice.open());
        assert_eq!(twice.write(b"twice"), 5);
        assert!(twice.commit());
        assert!(!twice.commit());
        assert_eq!(twice.write(b"x"), -1);
        assert_eq!(twice.error_string(), "Unknown error");
        assert_eq!(read(&f), "twice");
        assert!(!SaveFile::new(f.clone()).commit());
        assert_eq!(SaveFile::new(f.clone()).write(b"x"), -1);
        assert_eq!(entries(&dir.0), ["f"]);
    }

    #[test]
    fn temporary_name_template() {
        let t = TemplateName::new("/a/b.json");
        assert_eq!((t.path.as_slice(), t.pos, t.len), (&b"/a/b.json.XXXXXX"[..], 10, 6));
        assert_eq!(t.dir(), b"/a");
        let t = TemplateName::new("fooXXXXXXXbar");
        assert_eq!((t.path.as_slice(), t.pos, t.len), (&b"fooXXXXXXXbar"[..], 3, 7));
        assert_eq!(t.dir(), b".");
        let t = TemplateName::new("XXXXXX/x");
        assert_eq!((t.path.as_slice(), t.pos, t.len), (&b"XXXXXX/x.XXXXXX"[..], 9, 6));
        let t = TemplateName::new("/f");
        assert_eq!(t.dir(), b"/");
        let t = TemplateName::new("a/../b");
        assert_eq!(t.path, b"b.XXXXXX");
        let mut t = TemplateName::new("x");
        let name = t.generate();
        assert!(name.starts_with(b"x.") && name[2..].iter().all(|c| c.is_ascii_alphabetic()));
    }
}
