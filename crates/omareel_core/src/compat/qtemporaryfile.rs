//! `QTemporaryFile` on Linux (Qt 6.11), and the `QTemporaryFileEngine` pieces `QSaveFile` shares.
//!
//! What Qt does, reproduced here:
//! - The template is the path with its last run of six or more "X" (not crossing a "/") to be
//!   replaced by random letters; without one, ".XXXXXX" is appended. The template is cleaned
//!   (`QDir::cleanPath`) first.
//! - `open` creates the file with mode 0600: unnamed (`O_TMPFILE`) in the template's directory
//!   where the file system allows it, otherwise as a fresh name from the template (`O_EXCL`, up
//!   to 16 names). A missing or unwritable directory fails with the system's message.
//! - An unnamed file gets its name when `fileName()` is first asked for (a fresh name from the
//!   template, linked in through /proc/self/fd), so it shows up in its directory only then.
//! - `close` does not close the file, it only goes back to the start; the descriptor stays open
//!   until the object goes away, which also removes the named file.
//! - Writes are `QFSFileEngine`'s: everything, or as much as the system took; -1 and an error only
//!   when nothing was written, so a write cut short reports "Unknown error".
//!
//! Not reproduced: Qt's `QRandomGenerator` letters (the names use the same 52 letters), and the
//! 16 KiB write buffer of `QFileDevice` (each write goes straight to the file, which only changes
//! when a failure would surface for writes smaller than the buffer).

use super::qdir;
use std::ffi::CString;

/// `QTemporaryFileName`: the template path with a run of at least six "X" to replace.
pub(crate) struct TemplateName {
    pub(crate) path: Vec<u8>,
    pub(crate) pos: usize,
    pub(crate) len: usize,
}

impl TemplateName {
    pub(crate) fn new(name: &str) -> TemplateName {
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
    pub(crate) fn generate(&mut self) -> Vec<u8> {
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
    pub(crate) fn dir(&self) -> Vec<u8> {
        match self.path.iter().rposition(|&c| c == b'/') {
            None => b".".to_vec(),
            Some(0) => b"/".to_vec(),
            Some(i) => self.path[..i].to_vec(),
        }
    }
}

pub(crate) fn random_bytes(buf: &mut [u8]) -> bool {
    // SAFETY: the buffer is valid for its length.
    let n = unsafe { libc::getrandom(buf.as_mut_ptr().cast(), buf.len(), 0) };
    n == buf.len() as isize
}

pub(crate) fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `qt_haveLinuxProcfs`: an unnamed file can only be linked in through /proc/self/fd.
pub(crate) fn have_procfs() -> bool {
    static PRESENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // SAFETY: a valid NUL-terminated string.
    *PRESENT.get_or_init(|| unsafe { libc::access(c"/proc/version".as_ptr(), libc::F_OK) == 0 })
}

/// `qt_safe_open`: `open` with `O_CLOEXEC`, retried on EINTR.
pub(crate) fn open_raw(path: &[u8], flags: libc::c_int, mode: u32) -> Result<libc::c_int, i32> {
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

pub(crate) fn linkat_fd(fd: libc::c_int, target: &[u8]) -> Result<(), i32> {
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

pub(crate) fn rename_raw(from: &[u8], to: &[u8]) -> Result<(), i32> {
    let from = CString::new(from).map_err(|_| libc::EINVAL)?;
    let to = CString::new(to).map_err(|_| libc::EINVAL)?;
    // SAFETY: both are valid NUL-terminated strings.
    if unsafe { libc::rename(from.as_ptr(), to.as_ptr()) } == 0 {
        Ok(())
    } else {
        Err(errno())
    }
}


/// `QTemporaryFileEngine::open` for `template` with the given creation mode: the descriptor,
/// and the path when the file could not be unnamed.
pub(crate) fn create(template: &str, mode: u32) -> Result<(libc::c_int, Option<Vec<u8>>), String> {
    create_with_errno(template, mode).map_err(|(_, text)| text)
}

/// What [`create_with_errno`] gives: the descriptor and the path of a named file, or the
/// `errno` and message of the failure.
pub(crate) type Created = Result<(libc::c_int, Option<Vec<u8>>), (i32, String)>;

/// [`create`], with the `errno` of the failure (0 when every name was taken).
pub(crate) fn create_with_errno(template: &str, mode: u32) -> Created {
    let mut name = TemplateName::new(template);
    if have_procfs() {
        match open_raw(&name.dir(), libc::O_TMPFILE | libc::O_RDWR, mode) {
            Ok(fd) => return Ok((fd, None)),
            Err(e) if e == libc::EOPNOTSUPP || e == libc::EISDIR => {}
            Err(e) => return Err((e, qdir::error_string(e))),
        }
    }
    for _ in 0..16 {
        let path = name.generate();
        match open_raw(&path, libc::O_CREAT | libc::O_EXCL | libc::O_RDWR, mode) {
            Ok(fd) => return Ok((fd, Some(path))),
            Err(libc::EEXIST) => continue,
            Err(e) => return Err((e, qdir::error_string(e))),
        }
    }
    Err((0, "No error".into()))
}

/// `QTemporaryFile` with a template, as the exporter uses it: `open`, `fileName`, `write`,
/// `flush`, `close`, and removal when it goes away (`autoRemove`).
pub struct TemporaryFile {
    template: String,
    fd: libc::c_int,
    /// `None` while the file is unnamed (or not open), else the path it has in the file system.
    named: Option<Vec<u8>>,
    error_string: String,
}

impl TemporaryFile {
    /// `QTemporaryFile(templateName)`; nothing is created before `open`.
    pub fn new(template: &str) -> TemporaryFile {
        TemporaryFile { template: template.to_string(), fd: -1, named: None, error_string: String::new() }
    }

    /// `QTemporaryFile::open()`. Opening an open file does nothing (Qt only seeks back).
    pub fn open(&mut self) -> bool {
        if self.fd >= 0 {
            self.seek_start();
            return true;
        }
        match create(&self.template, 0o600) {
            Ok((fd, named)) => {
                self.fd = fd;
                self.named = named;
                self.error_string.clear();
                true
            }
            Err(error) => {
                self.error_string = error;
                false
            }
        }
    }

    /// `QIODevice::errorString()`: the last error, "Unknown error" when there is none.
    pub fn error_string(&self) -> String {
        if self.error_string.is_empty() { "Unknown error".into() } else { self.error_string.clone() }
    }

    /// `QTemporaryFile::fileName()`. An unnamed file is linked in under a fresh name from the
    /// template first, as `materializeUnnamedFile(templateName, NameIsTemplate)` does; "" when
    /// that fails or the file was never opened.
    pub fn file_name(&mut self) -> String {
        if self.named.is_none() && self.fd >= 0 {
            let mut name = TemplateName::new(&self.template);
            for _ in 0..16 {
                let path = name.generate();
                if linkat_fd(self.fd, &path).is_ok() {
                    self.named = Some(path);
                    break;
                }
            }
        }
        self.named.as_deref().map(qdir::decode_name).unwrap_or_default()
    }

    /// `QFile::write`: the bytes written, or -1 (with the error) when nothing could be.
    pub fn write(&mut self, data: &[u8]) -> i64 {
        if self.fd < 0 {
            self.error_string = "File not open".into();
            return -1;
        }
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
            self.error_string = qdir::error_string(err);
            return -1;
        }
        written as i64
    }

    /// `QFileDevice::flush`: nothing is buffered here.
    pub fn flush(&mut self) -> bool {
        self.fd >= 0
    }

    /// `QTemporaryFile::close()`: `QTemporaryFileEngine::close` keeps the file open and only goes
    /// back to its start.
    pub fn close(&mut self) {
        self.seek_start();
    }

    fn seek_start(&mut self) {
        if self.fd >= 0 {
            // SAFETY: `fd` is open.
            unsafe { libc::lseek(self.fd, 0, libc::SEEK_SET) };
        }
    }
}

impl Drop for TemporaryFile {
    /// `~QTemporaryFile`: closes the file and removes it (`QTemporaryFileEngine::remove`); an
    /// unnamed file just goes away with its descriptor, and a file renamed meanwhile is not
    /// there to remove.
    fn drop(&mut self) {
        if self.fd >= 0 {
            // SAFETY: `fd` is open and closed once.
            unsafe { libc::close(self.fd) };
            self.fd = -1;
        }
        if let Some(path) = self.named.take() {
            if let Ok(c) = CString::new(path) {
                // SAFETY: a valid NUL-terminated string.
                unsafe { libc::unlink(c.as_ptr()) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;
    use std::os::unix::fs::PermissionsExt;

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

    #[test]
    fn named_on_demand_private_and_removed() {
        let dir = TempDir::new("qtemporaryfile");
        let template = dir.join("omareel-export-XXXXXX.rgba");
        let mut file = TemporaryFile::new(&template);
        assert!(file.open());
        let listing = || std::fs::read_dir(&dir.0).unwrap().count();
        assert_eq!(file.write(b"abc"), 3);
        assert!(file.flush());
        let name = file.file_name();
        assert_eq!(name, file.file_name());
        let prefix = dir.join("omareel-export-");
        assert!(name.starts_with(&prefix) && name.ends_with(".rgba"), "{name}");
        let letters = &name[prefix.len()..name.len() - 5];
        assert!(letters.len() == 6 && letters.bytes().all(|c| c.is_ascii_alphabetic()), "{letters}");
        assert_eq!(std::fs::read(&name).unwrap(), b"abc");
        assert_eq!(std::fs::metadata(&name).unwrap().permissions().mode() & 0o777, 0o600);
        file.close();
        assert_eq!(listing(), 1);
        drop(file);
        assert_eq!(listing(), 0);

        // A file renamed away is not removed.
        let mut moved = TemporaryFile::new(&dir.join(".omareel-export-XXXXXX.mp4"));
        assert!(moved.open());
        let from = moved.file_name();
        std::fs::rename(&from, dir.join("out.mp4")).unwrap();
        drop(moved);
        assert!(std::path::Path::new(&dir.join("out.mp4")).exists());

        let mut missing = TemporaryFile::new(&dir.join("missing/x-XXXXXX"));
        assert!(!missing.open());
        assert_eq!(missing.error_string(), "No such file or directory");
        assert_eq!(missing.file_name(), "");
    }
}
