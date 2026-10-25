//! `QDir` and `QFileInfo` (Qt 6.11 on Linux): path cleaning and joining, file facts, directory
//! listings with Qt's filters and sort order, `QDir::mkpath` and `QDir::removeRecursively`, and
//! the SHA-256 hex digest omareel keys its caches with (`QCryptographicHash`).
//!
//! Paths are `&str`/`String` where Qt has `QString`; they reach the system as UTF-8, as
//! `QFile::encodeName` sends them. Bytes that come back from the system (link targets,
//! `realpath`, the working directory, environment variables) are decoded the way
//! `QFile::decodeName` does it: a leading UTF-8 byte-order mark is dropped and every byte that
//! does not start a valid sequence becomes one U+FFFD. Directory listings decode names like
//! `QDirListing` instead; see [`entry_info_list`].
//!
//! [`FileInfo`] is `QFileInfo` with caching disabled: every query asks the file system again.
//! A `QFileInfo` from a listing keeps the file type `readdir` reported, so the two can only
//! differ if the file changes in between, or for the rare names a listing decodes to a
//! different path (below).
//!
//! Checked against Qt 6.11.2 with a probe program, with no differences: 5,850 path strings
//! through every string function here, from two working directories; `FileInfo` facts for 676
//! paths (symlinks to files, directories, devices, FIFOs and nowhere, loops, chains, trailing
//! slashes, unreadable directories); 71,000 listings over every filter and sort combination
//! omareel uses and most others, with mixed-case, hidden, numbered and non-ASCII names, names
//! equal but for case (more than 16 of them, so the sort's tie placement shows), invalid UTF-8
//! and sockets; 515,000 wildcard matches, each with and without case; Qt's case folding for
//! every code point and 40,000 case-insensitive comparisons; and `mkpath`/`removeRecursively`
//! over 35 scenarios, comparing the trees left behind.
//!
//! Not reproduced: paths starting with ":" name Qt resources, which don't exist here (the
//! string functions treat them as Qt does, file queries find nothing); paths containing NUL are
//! refused; `QDir::LocaleAware`; wildcard patterns with a backslash (Qt hands it to PCRE, so
//! `\.` means a backslash and any character), with a `[:alpha:]`-style class, or with a class
//! that is only `^` never match here.

use std::ffi::{CStr, CString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};

// ---------------------------------------------------------------------------------------------
// Environment and decoding

/// `qEnvironmentVariable(name)`: `None` when unset, decoded like `QFile::decodeName`.
pub(crate) fn env_var(name: &str) -> Option<String> {
    std::env::var_os(name).map(|v| decode_name(v.as_bytes()))
}

enum Utf8 {
    Char(char, usize),
    Error,
    EndOfString,
}

/// One step of Qt's UTF-8 decoder (`QUtf8Functions::fromUtf8`) at `s[i]`.
fn utf8_at(s: &[u8], i: usize) -> Utf8 {
    let b = s[i];
    if b < 0x80 {
        return Utf8::Char(b as char, 1);
    }
    if b <= 0xC1 {
        return Utf8::Error;
    }
    let (need, min, mut uc) = if b < 0xE0 {
        (2, 0x80, u32::from(b & 0x1F))
    } else if b < 0xF0 {
        (3, 0x800, u32::from(b & 0x0F))
    } else if b < 0xF5 {
        (4, 0x10000, u32::from(b & 0x07))
    } else {
        return Utf8::Error;
    };
    let continuation = |x: u8| x & 0xC0 == 0x80;
    let available = s.len() - i - 1;
    if available < need - 1 {
        if available > 0 && !continuation(s[i + 1]) {
            return Utf8::Error;
        }
        if available > 1 && !continuation(s[i + 2]) {
            return Utf8::Error;
        }
        return Utf8::EndOfString;
    }
    for &c in &s[i + 1..i + need] {
        if !continuation(c) {
            return Utf8::Error;
        }
        uc = (uc << 6) | u32::from(c & 0x3F);
    }
    if uc < min || (0xD800..=0xDFFF).contains(&uc) || uc > 0x10FFFF {
        return Utf8::Error;
    }
    match char::from_u32(uc) {
        Some(c) => Utf8::Char(c, need),
        None => Utf8::Error,
    }
}

/// `QFile::decodeName` (`QString::fromLocal8Bit`, stateless UTF-8): drops a leading BOM and
/// replaces every byte that does not start a valid sequence with U+FFFD.
pub(crate) fn decode_name(bytes: &[u8]) -> String {
    decode_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes))
}

/// Qt's UTF-8 decoding with errors replaced: one U+FFFD per byte that does not start a valid
/// sequence (a byte-order mark is kept).
pub(crate) fn decode_utf8_lossy(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match utf8_at(bytes, i) {
            Utf8::Char(c, len) => {
                out.push(c);
                i += len;
            }
            Utf8::Error | Utf8::EndOfString => {
                out.push('\u{FFFD}');
                i += 1;
            }
        }
    }
    out
}

/// How `QFileSystemIterator` decodes a directory entry's name: a fresh stateful decoder that is
/// never flushed. Any invalid sequence makes Qt skip the entry (`None`); a leading U+FEFF is
/// eaten as a byte-order mark, and an incomplete sequence at the end stays in the decoder.
fn decode_listing_name(bytes: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match utf8_at(bytes, i) {
            Utf8::Char(c, len) => {
                if !(i == 0 && c == '\u{FEFF}') {
                    out.push(c);
                }
                i += len;
            }
            Utf8::Error => return None,
            Utf8::EndOfString => break,
        }
    }
    Some(out)
}

fn c_path(path: &str) -> Option<CString> {
    CString::new(path.as_bytes()).ok()
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// `qt_error_string(errno)` (`QSystemError::stdString`): Qt's own wording for four common
/// errors, the C library's `strerror` for the rest.
pub(crate) fn error_string(code: i32) -> String {
    match code {
        0 => String::new(),
        libc::EACCES => "Permission denied".into(),
        libc::EMFILE => "Too many open files".into(),
        libc::ENOENT => "No such file or directory".into(),
        libc::ENOSPC => "No space left on device".into(),
        _ => {
            let mut buf = [0 as libc::c_char; 1024];
            // SAFETY: the buffer is valid for its length; the XSI strerror_r NUL-terminates it.
            let rc = unsafe { libc::strerror_r(code, buf.as_mut_ptr(), buf.len()) };
            if rc != 0 {
                return format!("Unknown error {code}");
            }
            // SAFETY: strerror_r succeeded, so `buf` holds a NUL-terminated string.
            let text = unsafe { CStr::from_ptr(buf.as_ptr()) };
            decode_name(text.to_bytes()).trim().to_string()
        }
    }
}

// The three ways Qt asks about a path. "" and paths with NUL fail as in Qt; resource paths
// (":...") go to Qt's resource engine, where nothing exists here.

fn access(path: &str, mode: libc::c_int) -> bool {
    if is_resource(path) {
        return false;
    }
    match c_path(path) {
        // SAFETY: `p` is a valid NUL-terminated string.
        Some(p) if !path.is_empty() => unsafe { libc::access(p.as_ptr(), mode) == 0 },
        _ => false,
    }
}

fn stat(path: &str) -> Option<std::fs::Metadata> {
    if path.is_empty() || path.contains('\0') || is_resource(path) {
        return None;
    }
    std::fs::metadata(path).ok()
}

fn lstat(path: &str) -> Option<std::fs::Metadata> {
    if path.is_empty() || path.contains('\0') || is_resource(path) {
        return None;
    }
    std::fs::symlink_metadata(path).ok()
}

// ---------------------------------------------------------------------------------------------
// Path strings

/// `qt_normalizePathSegments` for local paths: returns the path with "." and ".." resolved and
/// repeated slashes collapsed (a trailing slash survives), and `false` if an absolute path had
/// ".." left over above the root.
fn normalize_path_segments(path: &str) -> (String, bool) {
    let p = path.as_bytes();
    let n = p.len();
    let prefix = usize::from(p.first() == Some(&b'/'));
    let i = find_start_of_non_normalized(p, prefix);
    if i == n {
        return (path.to_string(), true);
    }

    let at = |k: usize| p.get(k).copied().unwrap_or(0);
    let mut out: Vec<u8> = p[..i].to_vec();
    let mut start = prefix;
    let mut input = i;
    let mut ok = true;
    loop {
        if at(input) == b'/' {
            out.push(b'/');
            input += 1;
            while input < n && p[input] == b'/' {
                input += 1;
            }
        }
        #[derive(PartialEq)]
        enum Kind {
            Nothing,
            Dot,
            DotDot,
        }
        let mut kind = Kind::Nothing;
        if at(input) == b'.' {
            if input + 1 == n || at(input + 1) == b'/' {
                kind = Kind::Dot;
            } else if at(input + 1) == b'.' && (input + 2 == n || at(input + 2) == b'/') {
                kind = Kind::DotDot;
            }
        }
        if kind == Kind::Nothing {
            while input < n && p[input] != b'/' {
                out.push(p[input]);
                input += 1;
            }
        } else {
            let mut rerooted = false;
            if kind == Kind::DotDot {
                if out.len() == start {
                    // Can't go further up: keep the ".." and re-root after it.
                    ok = false;
                    out.extend_from_slice(b"..");
                    if input + 2 != n {
                        out.push(b'/');
                        input += 1;
                    }
                    start = out.len();
                    input += 2;
                    rerooted = true;
                } else {
                    if out.len() > start {
                        out.pop();
                    }
                    while out.len() > start && out.last() != Some(&b'/') {
                        out.pop();
                    }
                    input += 2;
                }
            } else {
                input += 1;
            }
            if !rerooted {
                if input != n && out.len() > start && out.last() == Some(&b'/') {
                    out.pop();
                }
                if out.len() == start {
                    if input != n && at(input) == b'/' {
                        input += 1;
                    }
                    while prefix == 0 && input != n && at(input) == b'/' {
                        input += 1;
                    }
                }
            }
        }
        if input >= n {
            break;
        }
    }
    let mut cleaned = String::from_utf8(out).unwrap_or_default();
    if cleaned.is_empty() {
        cleaned.push('.');
    }
    (cleaned, ok || prefix == 0)
}

/// `qt_cleanPath`: `QDir::cleanPath` plus whether ".." stayed above an absolute root.
fn clean_path_ok(path: &str) -> (String, bool) {
    if path.is_empty() {
        return (String::new(), true);
    }
    let (mut cleaned, ok) = normalize_path_segments(path);
    if cleaned.len() > 1 && cleaned.ends_with('/') {
        cleaned.pop();
    }
    (cleaned, ok)
}

/// `QDir::cleanPath`: repeated slashes collapsed, "." and ".." resolved where possible, and a
/// trailing slash dropped except for "/". Symlinks are not looked at. "" stays "", and a
/// relative path that cleans to nothing becomes ".".
pub fn clean_path(path: &str) -> String {
    clean_path_ok(path).0
}

/// `qt_isPathNormalized`: nothing for `clean_path` to do (a trailing slash is allowed).
fn is_clean(path: &str) -> bool {
    let p = path.as_bytes();
    let prefix = usize::from(p.first() == Some(&b'/'));
    find_start_of_non_normalized(p, prefix) == p.len()
}

/// `findStartOfNonNormalizedPath`: where the first "." or ".." segment or repeated slash is.
fn find_start_of_non_normalized(p: &[u8], mut i: usize) -> usize {
    let n = p.len();
    let mut last_was_slash = true;
    while i < n {
        if last_was_slash && p[i] == b'.' {
            if i + 1 == n || p[i + 1] == b'/' {
                break;
            }
            if p[i + 1] == b'.' && (i + 2 == n || p[i + 2] == b'/') {
                break;
            }
        }
        if last_was_slash && p[i] == b'/' && i > 0 {
            i -= 1;
            break;
        }
        last_was_slash = p[i] == b'/';
        i += 1;
    }
    i
}

/// Paths Qt hands to its resource engine.
fn is_resource(path: &str) -> bool {
    path.starts_with(':')
}

/// `QResource::fileName()` for a resource that doesn't exist: the path as given, ":" as ":/".
fn resource_file_name(path: &str) -> &str {
    if path == ":" {
        ":/"
    } else {
        path
    }
}

/// `QDir::isAbsolutePath`: starts with "/", or with ":" (a Qt resource). "" is relative.
pub fn is_absolute_path(path: &str) -> bool {
    !is_relative_path(path)
}

/// `QDir::isRelativePath`.
pub fn is_relative_path(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    !(is_resource(path) || path.starts_with('/'))
}

/// `QDir(path)`'s stored path (`QDirPrivate::setPath`): "." for "", and one trailing slash
/// removed.
fn dir_entry_path(path: &str) -> String {
    let mut p = if path.is_empty() { ".".to_string() } else { path.to_string() };
    if p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// `QDir(path).path()`.
pub fn dir_path(path: &str) -> String {
    dir_entry_path(path)
}

/// `treatAsAbsolute` in qdir.cpp: what `QDir::filePath` passes through untouched.
fn treat_as_absolute(name: &str) -> bool {
    name.starts_with('/') || is_resource(name)
}

/// `QDir(dir).filePath(name)`: `name` itself if it is absolute, otherwise joined to the
/// directory's path with one "/". Nothing is cleaned; `QDir("")` is `QDir(".")`, so
/// `file_path("", "x")` is "./x".
pub fn file_path(dir: &str, name: &str) -> String {
    if treat_as_absolute(name) {
        return name.to_string();
    }
    let ret = dir_entry_path(dir);
    if name.is_empty() {
        return ret;
    }
    if ret.is_empty() || ret.ends_with('/') {
        ret + name
    } else {
        ret + "/" + name
    }
}

/// `QDir(dir).absoluteFilePath(name)`: like [`file_path`] but from the directory's
/// [`absolute_path`].
pub fn absolute_file_path(dir: &str, name: &str) -> String {
    absolute_file_path_in(dir, name, &current_dir_bytes())
}

fn absolute_file_path_in(dir: &str, name: &str, cwd: &[u8]) -> String {
    if treat_as_absolute(name) {
        return name.to_string();
    }
    let base = resolve_absolute_entry(&dir_entry_path(dir), cwd);
    if name.is_empty() {
        return base;
    }
    if base.ends_with('/') {
        base + name
    } else {
        base + "/" + name
    }
}

/// [`absolute_file_path`] with the working directory given (`QDir::currentPath()` and its bytes).
pub(crate) fn absolute_file_path_with_cwd(dir: &str, name: &str, cwd: &[u8]) -> String {
    absolute_file_path_in(dir, name, cwd)
}

fn current_dir_bytes() -> Vec<u8> {
    std::env::current_dir().map(|d| d.into_os_string().into_vec()).unwrap_or_default()
}

/// `QDir::currentPath()`.
pub fn current_path() -> String {
    decode_name(&current_dir_bytes())
}

/// `QFileSystemEngine::absoluteName`: an absolute, clean path stays as it is (trailing slash
/// included); anything else is joined to the working directory and cleaned, keeping a
/// trailing slash if the joined path had one.
fn absolute_name(path: &str, cwd: &[u8]) -> String {
    if path.is_empty() || path.contains('\0') {
        return path.to_string();
    }
    if path.starts_with('/') && is_clean(path) {
        return path.to_string();
    }
    let orig = path.as_bytes();
    let mut result: Vec<u8> = Vec::new();
    if !orig.starts_with(b"/") {
        result = cwd.to_vec();
    }
    if orig != b"." {
        if !result.is_empty() && !result.ends_with(b"/") {
            result.push(b'/');
        }
        result.extend_from_slice(orig);
    }
    if result == b"/" {
        return "/".into();
    }
    let is_dir = result.ends_with(b"/");
    let mut s = clean_path(&decode_name(&result));
    if is_dir {
        s.push('/');
    }
    s
}

/// `QDir(path).absolutePath()`: the directory's path made absolute against the working
/// directory and cleaned (an absolute path that is already clean is returned as it is, so
/// `QDir("/a/b//")` gives "/a/b/"). Symlinks are not resolved.
pub fn absolute_path(path: &str) -> String {
    absolute_path_in(path, &current_dir_bytes())
}

fn absolute_path_in(path: &str, cwd: &[u8]) -> String {
    let entry = dir_entry_path(path);
    if is_resource(&entry) {
        // The resource engine's absolute name, uncleaned.
        return resource_file_name(&entry).to_string();
    }
    resolve_absolute_entry(&entry, cwd)
}

/// `QDirPrivate::resolveAbsoluteEntry` for a QDir's stored path.
fn resolve_absolute_entry(entry: &str, cwd: &[u8]) -> String {
    if is_resource(entry) {
        return clean_path(resource_file_name(entry));
    }
    if entry.starts_with('/') && is_clean(entry) {
        return entry.to_string();
    }
    clean_path(&absolute_name(entry, cwd))
}

/// `QFileSystemEntry::fileName`: everything after the last "/".
fn entry_file_name(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[i + 1..],
        None => path,
    }
}

/// `QFileSystemEntry::path`: everything before the last "/", "/" for a root child, "." for none.
fn entry_path(path: &str) -> String {
    match path.rfind('/') {
        None => ".".into(),
        Some(0) => "/".into(),
        Some(i) => path[..i].to_string(),
    }
}

/// `QDir(path).dirName()`: the last component of the directory's path ("" for "/").
pub fn dir_name(path: &str) -> String {
    let entry = dir_entry_path(path);
    if is_resource(&entry) {
        return entry_file_name(resource_file_name(&entry)).to_string();
    }
    entry_file_name(&entry).to_string()
}

/// `QDir::homePath()`: `$HOME` cleaned, or "/" when it is unset or empty.
pub fn home_path() -> String {
    home_path_with(&env_var)
}

pub(crate) fn home_path_with(env: &dyn Fn(&str) -> Option<String>) -> String {
    let home = env("HOME").unwrap_or_default();
    clean_path(if home.is_empty() { "/" } else { &home })
}

/// `QDir::tempPath()`: `$TMPDIR` without one trailing slash, made absolute, or "/tmp".
pub fn temp_path() -> String {
    temp_path_with(&env_var, &current_dir_bytes())
}

pub(crate) fn temp_path_with(env: &dyn Fn(&str) -> Option<String>, cwd: &[u8]) -> String {
    let mut temp = env("TMPDIR").unwrap_or_default();
    if temp.is_empty() {
        return "/tmp".into();
    }
    if temp.len() > 1 && temp.ends_with('/') {
        temp.pop();
    }
    absolute_name(&temp, cwd)
}

/// `QDir::cd("..")` on `QDir(path)`, returning the resulting `path()` (unchanged if Qt's cd
/// fails).
fn cd_up(path: &str, cwd: &[u8]) -> String {
    let current = dir_entry_path(path);
    let mut new_path = current.clone();
    if !new_path.ends_with('/') {
        new_path.push('/');
    }
    new_path.push_str("..");
    let (mut cleaned, ok) = clean_path_ok(&new_path);
    if !ok {
        return current;
    }
    if cleaned.starts_with("..") {
        cleaned = absolute_name(&cleaned, cwd);
    }
    let candidate = dir_entry_path(&cleaned);
    if dir_exists(&candidate) {
        candidate
    } else {
        current
    }
}

// ---------------------------------------------------------------------------------------------
// QFileInfo

/// `QFileInfo` for a path, without caching: each method asks the file system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileInfo {
    path: String,
}

impl FileInfo {
    /// `QFileInfo(path)`.
    pub fn new(path: impl Into<String>) -> FileInfo {
        FileInfo { path: path.into() }
    }

    /// `filePath()`: the path as given.
    pub fn file_path(&self) -> &str {
        &self.path
    }

    /// `fileName()`: the part after the last "/" ("" for a path ending in "/").
    pub fn file_name(&self) -> String {
        entry_file_name(self.engine_name()).to_string()
    }

    /// The name `fileName()`, `baseName()` and `completeBaseName()` work from: the path, or for a
    /// resource the resource's file name.
    fn engine_name(&self) -> &str {
        if is_resource(&self.path) {
            resource_file_name(&self.path)
        } else {
            &self.path
        }
    }

    /// `path()`: the part before the last "/", "/" for "/x", "." for a bare name.
    pub fn path(&self) -> String {
        if self.path.is_empty() {
            return String::new();
        }
        entry_path(&self.path)
    }

    /// `baseName()`: the file name up to its first ".".
    pub fn base_name(&self) -> String {
        let name = entry_file_name(self.engine_name());
        name.split('.').next().unwrap_or("").to_string()
    }

    /// `completeBaseName()`: the file name up to its last ".".
    pub fn complete_base_name(&self) -> String {
        let name = entry_file_name(self.engine_name());
        match name.rfind('.') {
            Some(i) => name[..i].to_string(),
            None => name.to_string(),
        }
    }

    /// `suffix()`: the file name after its last "." ("" without one).
    pub fn suffix(&self) -> String {
        let name = entry_file_name(&self.path);
        match name.rfind('.') {
            Some(i) => name[i + 1..].to_string(),
            None => String::new(),
        }
    }

    /// `completeSuffix()`: the file name after its first ".".
    pub fn complete_suffix(&self) -> String {
        let name = entry_file_name(&self.path);
        match name.find('.') {
            Some(i) => name[i + 1..].to_string(),
            None => String::new(),
        }
    }

    /// `isRelative()`.
    pub fn is_relative(&self) -> bool {
        is_relative_path(&self.path)
    }

    /// `isAbsolute()`.
    pub fn is_absolute(&self) -> bool {
        !self.is_relative()
    }

    /// `absoluteFilePath()`: made absolute against the working directory and cleaned; a path
    /// that is already absolute and clean comes back unchanged, trailing slash and all.
    pub fn absolute_file_path(&self) -> String {
        self.absolute_file_path_in(&current_dir_bytes())
    }

    fn absolute_file_path_in(&self, cwd: &[u8]) -> String {
        if is_resource(&self.path) {
            return resource_file_name(&self.path).to_string();
        }
        absolute_name(&self.path, cwd)
    }

    /// `absolutePath()`: [`absolute_file_path`](Self::absolute_file_path) without its last
    /// component.
    pub fn absolute_path(&self) -> String {
        self.absolute_path_in(&current_dir_bytes())
    }

    fn absolute_path_in(&self, cwd: &[u8]) -> String {
        if self.path.is_empty() {
            return String::new();
        }
        if is_resource(&self.path) {
            return resource_dir(resource_file_name(&self.path));
        }
        entry_path(&absolute_name(&self.path, cwd))
    }

    /// `QFileSystemEngine::canonicalName`: `None` for "doesn't exist" (ENOENT, ENOTDIR), the
    /// path as given for any other `realpath` failure (a symlink loop, no permission).
    fn canonical(&self) -> Option<String> {
        if self.path.is_empty() || is_resource(&self.path) {
            return None;
        }
        if self.path.contains('\0') {
            return Some(self.path.clone());
        }
        match std::fs::canonicalize(&self.path) {
            Ok(p) => Some(decode_name(p.as_os_str().as_bytes())),
            Err(e) => match e.raw_os_error() {
                Some(libc::ENOENT) | Some(libc::ENOTDIR) => None,
                _ => Some(self.path.clone()),
            },
        }
    }

    /// `canonicalFilePath()`: absolute, symlinks resolved, "" if the path doesn't exist.
    pub fn canonical_file_path(&self) -> String {
        if is_resource(&self.path) {
            return resource_file_name(&self.path).to_string();
        }
        self.canonical().unwrap_or_default()
    }

    /// `canonicalPath()`: the directory of [`canonical_file_path`](Self::canonical_file_path);
    /// "." if the path doesn't exist (an empty entry's path, as Qt computes it).
    pub fn canonical_path(&self) -> String {
        if self.path.is_empty() {
            return String::new();
        }
        if is_resource(&self.path) {
            let name = resource_file_name(&self.path);
            return match name.rfind('/') {
                Some(i) => name[..i].to_string(),
                None => name.to_string(),
            };
        }
        match self.canonical() {
            Some(p) => entry_path(&p),
            None => ".".into(),
        }
    }

    /// `exists()`: `access(F_OK)`, so a broken symlink doesn't exist.
    pub fn exists(&self) -> bool {
        !is_resource(&self.path) && access(&self.path, libc::F_OK)
    }

    /// `isFile()`: a regular file, or a symlink to one.
    pub fn is_file(&self) -> bool {
        stat(&self.path).is_some_and(|m| m.file_type().is_file())
    }

    /// `isDir()`: a directory, or a symlink to one.
    pub fn is_dir(&self) -> bool {
        stat(&self.path).is_some_and(|m| m.file_type().is_dir())
    }

    /// `isSymLink()`: the path itself is a symlink, whether or not its target exists.
    pub fn is_symlink(&self) -> bool {
        lstat(&self.path).is_some_and(|m| m.file_type().is_symlink())
    }

    /// `isHidden()`: the last component (trailing slashes skipped) starts with ".".
    pub fn is_hidden(&self) -> bool {
        if self.path.is_empty() || self.path.contains('\0') || is_resource(&self.path) {
            return false;
        }
        entry_file_name(self.path.trim_end_matches('/')).starts_with('.')
    }

    /// `isReadable()`: `access(R_OK)` for the real user.
    pub fn is_readable(&self) -> bool {
        !is_resource(&self.path) && access(&self.path, libc::R_OK)
    }

    /// `isWritable()`: `access(W_OK)`.
    pub fn is_writable(&self) -> bool {
        !is_resource(&self.path) && access(&self.path, libc::W_OK)
    }

    /// `isExecutable()`: `access(X_OK)`.
    pub fn is_executable(&self) -> bool {
        !is_resource(&self.path) && access(&self.path, libc::X_OK)
    }

    /// `symLinkTarget()`: where the symlink points, made absolute and cleaned but not resolved
    /// any further; "" if the path isn't a symlink. A relative target of a symlink to a
    /// directory is resolved from `QDir(link).cdUp()`, otherwise from the link's absolute
    /// directory; both are lexical.
    pub fn sym_link_target(&self) -> String {
        self.sym_link_target_in(&current_dir_bytes())
    }

    fn sym_link_target_in(&self, cwd: &[u8]) -> String {
        if self.path.is_empty() {
            return String::new();
        }
        if is_resource(&self.path) {
            return resource_file_name(&self.path).to_string();
        }
        if self.path.contains('\0') {
            return self.path.clone();
        }
        let raw = match std::fs::read_link(&self.path) {
            Ok(t) => t.into_os_string().into_vec(),
            Err(_) => return String::new(),
        };
        if raw.is_empty() {
            return String::new();
        }
        let mut ret = String::new();
        if self.is_dir() && raw[0] != b'/' {
            ret = cd_up(&self.path, cwd);
            if !ret.is_empty() && !ret.ends_with('/') {
                ret.push('/');
            }
        }
        ret.push_str(&decode_name(&raw));
        if !ret.starts_with('/') {
            ret = entry_path(&absolute_name(&self.path, cwd)) + "/" + &ret;
        }
        let mut ret = clean_path(&ret);
        if ret.len() > 1 && ret.ends_with('/') {
            ret.pop();
        }
        ret
    }

    /// `size()`: the size of the file (or the symlink's target), 0 if it doesn't exist.
    pub fn size(&self) -> i64 {
        stat(&self.path).map_or(0, |m| m.size() as i64)
    }

    /// `lastModified().toMSecsSinceEpoch()`: the modification time in milliseconds (truncated
    /// toward zero), 0 if the file doesn't exist (Qt's invalid `QDateTime`).
    pub fn last_modified_msecs(&self) -> i64 {
        stat(&self.path).map_or(0, |m| mtime_msecs(&m))
    }

    /// `permissions()` as the mode `QFile::setPermissions` would apply (`QtPrivate::toMode_t`):
    /// the owner/group/other bits, plus an owner bit for each of read, write and execute the
    /// current user is granted.
    pub(crate) fn permissions_mode(&self) -> u32 {
        let Some(m) = stat(&self.path) else { return 0 };
        let mut mode = m.permissions().mode() & 0o777;
        if self.is_readable() {
            mode |= 0o400;
        }
        if self.is_writable() {
            mode |= 0o200;
        }
        if self.is_executable() {
            mode |= 0o100;
        }
        mode
    }
}

fn mtime_msecs(m: &std::fs::Metadata) -> i64 {
    let nanos = i128::from(m.mtime()) * 1_000_000_000 + i128::from(m.mtime_nsec());
    (nanos / 1_000_000) as i64
}

/// `QResourceFileEngine`'s directory name for a resource path that doesn't exist.
fn resource_dir(path: &str) -> String {
    match path.rfind('/') {
        None => ":".into(),
        Some(i) if i <= 1 => ":/".into(),
        Some(i) => path[..i].to_string(),
    }
}

/// `QFileInfo::exists(path)`.
pub fn file_exists(path: &str) -> bool {
    FileInfo::new(path).exists()
}

/// `QDir(path).exists()`: `stat` succeeds and finds a directory (symlinks followed).
pub fn dir_exists(path: &str) -> bool {
    let entry = dir_entry_path(path);
    !is_resource(&entry) && stat(&entry).is_some_and(|m| m.file_type().is_dir())
}

/// `QDir(dir).exists(name)`: false for an empty name, else [`file_exists`] of
/// [`file_path`]`(dir, name)`.
pub fn dir_contains(dir: &str, name: &str) -> bool {
    !name.is_empty() && file_exists(&file_path(dir, name))
}

// ---------------------------------------------------------------------------------------------
// mkpath and removeRecursively

/// `QDir().mkpath(path)`: creates the directory and any missing parents with mode 0777 (less
/// the umask). True if the directory exists afterwards, false for "". Mirrors Qt's walk: a
/// `mkdir` that fails with ENOENT creates the parent (the path before the last run of slashes,
/// "." and ".." included) and tries again, so "a/../b" also creates "a". `QDir(dir).mkpath(p)`
/// is `mkpath(&file_path(dir, p))`.
pub fn mkpath(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let full = file_path("", path);
    if full.contains('\0') {
        return false;
    }
    let mut bytes = full.into_bytes();
    while bytes.len() > 1 && bytes.ends_with(b"/") {
        bytes.pop();
    }
    create_directory_with_parents(&bytes) == 0
}

/// `createDirectoryWithParents` from qfilesystemengine_unix.cpp; returns 0 or an errno.
fn create_directory_with_parents(path: &[u8]) -> i32 {
    let try_mkdir = || -> i32 {
        let Ok(c) = CString::new(path) else { return libc::EINVAL };
        // SAFETY: `c` is a valid NUL-terminated string.
        if unsafe { libc::mkdir(c.as_ptr(), 0o777) } == 0 {
            return 0;
        }
        let err = errno();
        if err == libc::EISDIR {
            return 0;
        }
        if err == libc::EEXIST || err == libc::EROFS {
            return match std::fs::metadata(std::ffi::OsStr::from_bytes(path)) {
                Ok(m) if m.file_type().is_dir() => 0,
                Ok(_) => libc::EEXIST,
                Err(e) => e.raw_os_error().unwrap_or(libc::EIO),
            };
        }
        err
    };
    let result = try_mkdir();
    if result != libc::ENOENT {
        return result;
    }
    let Some(mut slash) = path.iter().rposition(|&b| b == b'/') else { return result };
    while slash > 0 && path[slash - 1] == b'/' {
        slash -= 1;
    }
    if slash < 1 {
        return result;
    }
    let parent = create_directory_with_parents(&path[..slash]);
    if parent != 0 {
        return parent;
    }
    try_mkdir()
}

/// `QDir(path).removeRecursively()`: true if `path` is not a directory (symlinks followed);
/// otherwise deletes everything under it, hidden files included, without following symlinks
/// inside, then the directory itself. A file that won't go and isn't writable is made owner
/// writable (`chmod`, which follows symlinks) and tried once more, as Qt does. Returns false if
/// anything is left. A symlink to a directory has the directory's contents removed and then
/// fails to `rmdir`, as in Qt.
pub fn remove_recursively(path: &str) -> bool {
    if !dir_exists(path) {
        return true;
    }
    let mut success = true;
    let mut stack: Vec<(String, bool)> = vec![(absolute_path(path), false)];
    while let Some(top) = stack.last_mut() {
        if !top.1 {
            top.1 = true;
            let dir = top.0.clone();
            for entry in read_dir_raw(&dir) {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }
                let info = FileInfo::new(entry.path.clone());
                if entry.is_dir() && !entry.is_link() {
                    stack.push((entry.path, false));
                } else {
                    let mut ok = std::fs::remove_file(&entry.path).is_ok();
                    if !ok && !info.is_writable() {
                        let mode = info.permissions_mode() | 0o200;
                        ok = std::fs::set_permissions(&entry.path, std::fs::Permissions::from_mode(mode))
                            .is_ok()
                            && std::fs::remove_file(&entry.path).is_ok();
                    }
                    if !ok {
                        success = false;
                    }
                }
            }
        } else {
            let dir = top.0.clone();
            stack.pop();
            if dir.contains('\0') || std::fs::remove_dir(&dir).is_err() {
                success = false;
            }
        }
    }
    success
}

// ---------------------------------------------------------------------------------------------
// Directory listings

/// `QDir::Filters`. Combine with `|`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filters(pub i32);

impl Filters {
    pub const DIRS: Filters = Filters(0x001);
    pub const FILES: Filters = Filters(0x002);
    pub const DRIVES: Filters = Filters(0x004);
    pub const NO_SYMLINKS: Filters = Filters(0x008);
    pub const ALL_ENTRIES: Filters = Filters(0x007);
    pub const READABLE: Filters = Filters(0x010);
    pub const WRITABLE: Filters = Filters(0x020);
    pub const EXECUTABLE: Filters = Filters(0x040);
    pub const HIDDEN: Filters = Filters(0x100);
    pub const SYSTEM: Filters = Filters(0x200);
    pub const ALL_DIRS: Filters = Filters(0x400);
    pub const CASE_SENSITIVE: Filters = Filters(0x800);
    pub const NO_DOT: Filters = Filters(0x2000);
    pub const NO_DOT_DOT: Filters = Filters(0x4000);
    pub const NO_DOT_AND_DOT_DOT: Filters = Filters(0x6000);
    /// `QDir::NoFilter`: the QDir's default, `ALL_ENTRIES`.
    pub const NO_FILTER: Filters = Filters(-1);

    fn has(self, flag: Filters) -> bool {
        self.0 & flag.0 != 0
    }
}

impl std::ops::BitOr for Filters {
    type Output = Filters;
    fn bitor(self, rhs: Filters) -> Filters {
        Filters(self.0 | rhs.0)
    }
}

/// `QDir::SortFlags`. Combine with `|`. (`QDir::LocaleAware` is not supported.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortFlags(pub i32);

impl SortFlags {
    pub const NAME: SortFlags = SortFlags(0x00);
    pub const TIME: SortFlags = SortFlags(0x01);
    pub const SIZE: SortFlags = SortFlags(0x02);
    pub const UNSORTED: SortFlags = SortFlags(0x03);
    pub const DIRS_FIRST: SortFlags = SortFlags(0x04);
    pub const REVERSED: SortFlags = SortFlags(0x08);
    pub const IGNORE_CASE: SortFlags = SortFlags(0x10);
    pub const DIRS_LAST: SortFlags = SortFlags(0x20);
    pub const TYPE: SortFlags = SortFlags(0x80);
    /// `QDir::NoSort`: the QDir's default, `NAME | IGNORE_CASE`.
    pub const NO_SORT: SortFlags = SortFlags(-1);

    fn has(self, flag: SortFlags) -> bool {
        self.0 & flag.0 != 0
    }
}

impl std::ops::BitOr for SortFlags {
    type Output = SortFlags;
    fn bitor(self, rhs: SortFlags) -> SortFlags {
        SortFlags(self.0 | rhs.0)
    }
}

// QDirListing::IteratorFlag values used below.
const EXCLUDE_DIRS: u32 = 1 << 0;
const EXCLUDE_FILES: u32 = 1 << 1;
const EXCLUDE_OTHER: u32 = 1 << 2;
const RESOLVE_SYMLINKS: u32 = 1 << 3;
const INCLUDE_BROKEN_SYMLINKS: u32 = 1 << 4;
const INCLUDE_DOT_AND_DOT_DOT: u32 = 1 << 5;
const INCLUDE_HIDDEN: u32 = 1 << 6;
const CASE_SENSITIVE: u32 = 1 << 7;
const NO_NAME_FILTERS_FOR_DIRS: u32 = 1 << 8;

/// `QDirPrivate::toDirListingFlags`.
fn listing_flags(filters: Filters) -> u32 {
    let filters = if filters == Filters::NO_FILTER { Filters::ALL_ENTRIES } else { filters };
    let mut flags = 0;
    if !filters.has(Filters::DIRS) && !filters.has(Filters::ALL_DIRS) {
        flags |= EXCLUDE_DIRS;
    }
    if !filters.has(Filters::FILES) {
        flags |= EXCLUDE_FILES;
    }
    if !filters.has(Filters::NO_SYMLINKS) {
        flags |= RESOLVE_SYMLINKS;
    }
    if filters.has(Filters::HIDDEN) {
        flags |= INCLUDE_HIDDEN;
    }
    if !filters.has(Filters::SYSTEM) {
        flags |= EXCLUDE_OTHER;
    } else {
        flags |= INCLUDE_BROKEN_SYMLINKS;
    }
    if filters.has(Filters::ALL_DIRS) {
        flags |= NO_NAME_FILTERS_FOR_DIRS;
    }
    if filters.has(Filters::CASE_SENSITIVE) {
        flags |= CASE_SENSITIVE;
    }
    let both_dots_excluded = filters.has(Filters::NO_DOT) && filters.has(Filters::NO_DOT_DOT);
    if !both_dots_excluded && flags & EXCLUDE_DIRS == 0 {
        flags |= INCLUDE_DOT_AND_DOT_DOT;
    }
    flags
}

/// A `readdir` entry with what Qt learns from `d_type` (`QFileSystemMetaData::fillFromDirEnt`);
/// anything `d_type` doesn't say is asked of the file system under the decoded path.
struct RawEntry {
    name: String,
    path: String,
    d_type: u8,
}

impl RawEntry {
    fn known_type(&self) -> bool {
        matches!(
            self.d_type,
            libc::DT_DIR | libc::DT_BLK | libc::DT_CHR | libc::DT_FIFO | libc::DT_SOCK | libc::DT_REG
        )
    }

    fn is_link(&self) -> bool {
        match self.d_type {
            libc::DT_LNK => true,
            libc::DT_UNKNOWN => lstat(&self.path).is_some_and(|m| m.file_type().is_symlink()),
            _ => false,
        }
    }

    fn exists(&self) -> bool {
        self.known_type() || access(&self.path, libc::F_OK)
    }

    fn is_dir(&self) -> bool {
        if self.known_type() {
            return self.d_type == libc::DT_DIR;
        }
        stat(&self.path).is_some_and(|m| m.file_type().is_dir())
    }

    fn is_file(&self) -> bool {
        if self.known_type() {
            return self.d_type == libc::DT_REG;
        }
        stat(&self.path).is_some_and(|m| m.file_type().is_file())
    }
}

/// `QFileSystemIterator` over `dir` (as given): `readdir` order, "." and ".." included, names
/// decoded as Qt does (entries Qt can't decode are skipped).
fn read_dir_raw(dir: &str) -> Vec<RawEntry> {
    let mut out = Vec::new();
    let Some(c) = c_path(dir) else { return out };
    if dir.is_empty() {
        return out;
    }
    // SAFETY: `c` is a valid NUL-terminated string; the stream is closed below.
    let stream = unsafe { libc::opendir(c.as_ptr()) };
    if stream.is_null() {
        return out;
    }
    let mut prefix = dir.to_string();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    loop {
        // SAFETY: `stream` is an open directory stream.
        let entry = unsafe { libc::readdir64(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: readdir64 returned a valid entry whose d_name is NUL-terminated.
        let (raw, d_type) =
            unsafe { (CStr::from_ptr((*entry).d_name.as_ptr()).to_bytes(), (*entry).d_type) };
        if let Some(name) = decode_listing_name(raw) {
            out.push(RawEntry { path: format!("{prefix}{name}"), name, d_type });
        }
    }
    // SAFETY: `stream` came from opendir and is closed once.
    unsafe { libc::closedir(stream) };
    out
}

/// `QDirListingPrivate::matchesFilters` for a native entry.
fn matches_filters(entry: &RawEntry, flags: u32, patterns: &[Glob]) -> bool {
    let name = entry.name.as_str();
    if name.is_empty() {
        return false;
    }
    let skip_name_filters = flags & NO_NAME_FILTERS_FOR_DIRS != 0 && entry.is_dir();
    if !skip_name_filters && !patterns.is_empty() && !patterns.iter().any(|p| p.matches(name)) {
        return false;
    }
    if name == "." || name == ".." {
        return flags & INCLUDE_DOT_AND_DOT_DOT != 0;
    }
    if flags & INCLUDE_HIDDEN == 0 && name.starts_with('.') {
        return false;
    }
    if flags & INCLUDE_BROKEN_SYMLINKS != 0 && entry.is_link() && !entry.exists() {
        return true;
    }
    if flags & RESOLVE_SYMLINKS != 0 {
        if entry.is_link() && !entry.exists() {
            return false;
        }
    } else if flags & (EXCLUDE_FILES | EXCLUDE_DIRS | EXCLUDE_OTHER) != 0 && entry.is_link() {
        return false;
    }
    if flags & EXCLUDE_OTHER != 0 && !entry.is_file() && !entry.is_dir() && !entry.is_link() {
        return false;
    }
    if flags & EXCLUDE_DIRS != 0 && entry.is_dir() {
        return false;
    }
    if flags & EXCLUDE_FILES != 0 && entry.is_file() {
        return false;
    }
    true
}

/// `QDirPrivate::checkNonDirListingFlags`: permission filters and NoDot/NoDotDot.
fn matches_non_listing_flags(entry: &RawEntry, filters: Filters) -> bool {
    let perms = filters.0 & 0x070;
    if perms != 0 && perms != 0x070 {
        let info = FileInfo::new(entry.path.clone());
        if filters.has(Filters::READABLE) && !info.is_readable() {
            return false;
        }
        if filters.has(Filters::WRITABLE) && !info.is_writable() {
            return false;
        }
        if filters.has(Filters::EXECUTABLE) && !info.is_executable() {
            return false;
        }
    }
    if filters.has(Filters::NO_DOT) && entry.name == "." {
        return false;
    }
    if filters.has(Filters::NO_DOT_DOT) && entry.name == ".." {
        return false;
    }
    true
}

/// `names_only` is `entryList`, which for an unsorted listing takes the names straight from the
/// iterator and so never applies the permission and NoDot/NoDotDot filters.
fn list(
    dir: &str,
    name_filters: &[&str],
    filters: Filters,
    sort: SortFlags,
    names_only: bool,
) -> Vec<RawEntry> {
    let filters = if filters == Filters::NO_FILTER { Filters::ALL_ENTRIES } else { filters };
    let sort = if sort == SortFlags::NO_SORT { SortFlags::NAME | SortFlags::IGNORE_CASE } else { sort };
    let unsorted = sort.0 & 0x03 == SortFlags::UNSORTED.0;
    let path = dir_entry_path(dir);
    if is_resource(&path) {
        return Vec::new();
    }
    let flags = listing_flags(filters);
    let case_sensitive = flags & CASE_SENSITIVE != 0;
    let patterns: Vec<Glob> = if name_filters.contains(&"*") {
        Vec::new()
    } else {
        name_filters.iter().map(|f| Glob::new(f, case_sensitive)).collect()
    };
    let entries: Vec<RawEntry> = read_dir_raw(&path)
        .into_iter()
        .filter(|e| {
            matches_filters(e, flags, &patterns)
                && ((names_only && unsorted) || matches_non_listing_flags(e, filters))
        })
        .collect();
    sort_entries(entries, sort)
}

/// `QDir(dir).entryInfoList(nameFilters, filters, sort)`.
///
/// Name filters are wildcard patterns (`*`, `?`, `[abc]`, `[!a-z]`) matched against the whole
/// name, case-insensitively unless `filters` has [`Filters::CASE_SENSITIVE`]; an empty list or
/// a "*" among them matches everything. Without [`Filters::HIDDEN`] names starting with "."
/// are left out. Symlinks are listed as what they point to (a symlink to a file is a file), a
/// broken symlink only with [`Filters::SYSTEM`], and with [`Filters::NO_SYMLINKS`] none at
/// all. As in Qt, a symlink to a device, FIFO or socket passes both [`Filters::FILES`] and
/// [`Filters::DIRS`], while the device itself needs [`Filters::SYSTEM`].
///
/// Entries come in `readdir` order and are sorted with Qt's comparison (names compared as
/// UTF-16, [`SortFlags::IGNORE_CASE`] with Qt's case folding, [`SortFlags::TIME`] newest first,
/// [`SortFlags::SIZE`] largest first, ties broken by name) using libstdc++'s `std::sort`, so
/// even names that compare equal come out in Qt's order. Each entry's path is the directory's
/// path, "/", and the name.
///
/// Names are decoded as `QDirListing` decodes them: an entry whose name is not valid UTF-8 is
/// skipped, except that a name ending in an incomplete sequence is listed without it, and a
/// leading U+FEFF is dropped. Such an entry's path then names another file, which Qt's
/// `QFileInfo` still reports with the type `readdir` gave, while [`FileInfo`] asks about the
/// path it has.
pub fn entry_info_list(
    dir: &str,
    name_filters: &[&str],
    filters: Filters,
    sort: SortFlags,
) -> Vec<FileInfo> {
    list(dir, name_filters, filters, sort, false).into_iter().map(|e| FileInfo::new(e.path)).collect()
}

/// `QDir(dir).entryList(nameFilters, filters, sort)`: the names of [`entry_info_list`], except
/// that, as in Qt, an unsorted `entryList` ignores [`Filters::READABLE`], [`Filters::WRITABLE`],
/// [`Filters::EXECUTABLE`], [`Filters::NO_DOT`] and [`Filters::NO_DOT_DOT`] (the last two only
/// matter when one is given without the other).
pub fn entry_list(dir: &str, name_filters: &[&str], filters: Filters, sort: SortFlags) -> Vec<String> {
    list(dir, name_filters, filters, sort, true).into_iter().map(|e| e.name).collect()
}

struct SortItem {
    entry: RawEntry,
    name16: Vec<u16>,
    suffix16: Vec<u16>,
    is_dir: bool,
    mtime: Option<i64>,
    size: i64,
}

fn sort_entries(entries: Vec<RawEntry>, sort: SortFlags) -> Vec<RawEntry> {
    if entries.len() <= 1 || sort.0 & 0x03 == SortFlags::UNSORTED.0 {
        return entries;
    }
    let mut items: Vec<SortItem> = entries
        .into_iter()
        .map(|entry| {
            let info = FileInfo::new(entry.path.clone());
            let meta = stat(&entry.path);
            let mtime = meta.as_ref().map(mtime_msecs).filter(|&ms| ms != 0);
            SortItem {
                name16: entry.name.encode_utf16().collect(),
                suffix16: info.suffix().encode_utf16().collect(),
                is_dir: entry.is_dir(),
                size: meta.map_or(0, |m| m.size() as i64),
                mtime,
                entry,
            }
        })
        .collect();
    let ignore_case = sort.has(SortFlags::IGNORE_CASE);
    let compare_strings = |a: &[u16], b: &[u16]| -> i64 {
        if ignore_case {
            compare_utf16_ignore_case(a, b)
        } else {
            compare_utf16(a, b)
        }
    };
    let sort_by = (sort.0 & 0x03) | (sort.0 & SortFlags::TYPE.0);
    let less = |f1: &SortItem, f2: &SortItem| -> bool {
        if sort.has(SortFlags::DIRS_FIRST) && f1.is_dir != f2.is_dir {
            return f1.is_dir;
        }
        if sort.has(SortFlags::DIRS_LAST) && f1.is_dir != f2.is_dir {
            return !f1.is_dir;
        }
        let mut r: i64 = match sort_by {
            0x01 => match (f1.mtime, f2.mtime) {
                (Some(a), Some(b)) => b - a,
                _ => 0,
            },
            0x02 => f2.size - f1.size,
            0x80 => compare_strings(&f1.suffix16, &f2.suffix16),
            _ => 0,
        };
        if r == 0 && sort_by != SortFlags::UNSORTED.0 {
            r = compare_strings(&f1.name16, &f2.name16);
        }
        if sort.has(SortFlags::REVERSED) {
            r > 0
        } else {
            r < 0
        }
    };
    std_sort(&mut items, &less);
    items.into_iter().map(|i| i.entry).collect()
}

/// `QString::compare(a, b, Qt::CaseSensitive)`: UTF-16 code units, then length.
fn compare_utf16(a: &[u16], b: &[u16]) -> i64 {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return i64::from(*x) - i64::from(*y);
        }
    }
    a.len() as i64 - b.len() as i64
}

/// `ucstricmp` in qstring.cpp: case-folded UTF-16 units, a low surrogate folded together with
/// the high surrogate before it.
fn compare_utf16_ignore_case(a: &[u16], b: &[u16]) -> i64 {
    let fold_at = |s: &[u16], i: usize| -> i64 {
        let unit = u32::from(s[i]);
        let after_high = i > 0 && (0xD800..0xDC00).contains(&u32::from(s[i - 1]));
        let ucs4 = if (0xDC00..0xE000).contains(&unit) && after_high {
            0x10000 + ((u32::from(s[i - 1]) - 0xD800) << 10) + (unit - 0xDC00)
        } else {
            unit
        };
        i64::from(fold_case(ucs4))
    };
    let l = a.len().min(b.len());
    for i in 0..l {
        let diff = fold_at(a, i) - fold_at(b, i);
        if diff != 0 {
            return diff;
        }
    }
    if l == a.len() {
        if l == b.len() {
            0
        } else {
            -1
        }
    } else {
        1
    }
}

/// libstdc++'s `std::sort` (introsort, median-of-three pivot, heapsort below the depth limit,
/// final insertion sort over runs of 16), so that elements comparing equal end up where Qt's
/// sort puts them. Index checks keep an inconsistent comparator from reading out of bounds.
fn std_sort<T>(v: &mut [T], less: &dyn Fn(&T, &T) -> bool) {
    let n = v.len();
    if n < 2 {
        return;
    }
    let depth = 2 * (usize::BITS - 1 - n.leading_zeros()) as usize;
    introsort_loop(v, 0, n, depth, less);
    final_insertion_sort(v, less);
}

const THRESHOLD: usize = 16;

fn introsort_loop<T>(
    v: &mut [T],
    first: usize,
    mut last: usize,
    mut depth: usize,
    less: &dyn Fn(&T, &T) -> bool,
) {
    while last - first > THRESHOLD {
        if depth == 0 {
            heap_sort(&mut v[first..last], less);
            return;
        }
        depth -= 1;
        let cut = partition_pivot(v, first, last, less);
        introsort_loop(v, cut, last, depth, less);
        last = cut;
    }
}

fn partition_pivot<T>(v: &mut [T], first: usize, last: usize, less: &dyn Fn(&T, &T) -> bool) -> usize {
    let mid = first + (last - first) / 2;
    let (a, b, c) = (first + 1, mid, last - 1);
    // __move_median_to_first
    let pick = if less(&v[a], &v[b]) {
        if less(&v[b], &v[c]) {
            b
        } else if less(&v[a], &v[c]) {
            c
        } else {
            a
        }
    } else if less(&v[a], &v[c]) {
        a
    } else if less(&v[b], &v[c]) {
        c
    } else {
        b
    };
    v.swap(first, pick);
    // __unguarded_partition(first + 1, last, pivot = first)
    let (mut lo, mut hi) = (first + 1, last);
    loop {
        while lo < last && less(&v[lo], &v[first]) {
            lo += 1;
        }
        hi -= 1;
        while hi > first && less(&v[first], &v[hi]) {
            hi -= 1;
        }
        if lo >= hi {
            return lo;
        }
        v.swap(lo, hi);
        lo += 1;
    }
}

fn final_insertion_sort<T>(v: &mut [T], less: &dyn Fn(&T, &T) -> bool) {
    let n = v.len();
    if n > THRESHOLD {
        insertion_sort(&mut v[..THRESHOLD], less);
        for i in THRESHOLD..n {
            unguarded_linear_insert(v, i, less);
        }
    } else {
        insertion_sort(v, less);
    }
}

fn insertion_sort<T>(v: &mut [T], less: &dyn Fn(&T, &T) -> bool) {
    for i in 1..v.len() {
        if less(&v[i], &v[0]) {
            v[..=i].rotate_right(1);
        } else {
            unguarded_linear_insert(v, i, less);
        }
    }
}

fn unguarded_linear_insert<T>(v: &mut [T], last: usize, less: &dyn Fn(&T, &T) -> bool) {
    let mut hole = last;
    while hole > 0 && less(&v[last], &v[hole - 1]) {
        hole -= 1;
    }
    v[hole..=last].rotate_right(1);
}

/// `std::__partial_sort(first, last, last)`: `__make_heap`, then `__sort_heap`.
fn heap_sort<T>(v: &mut [T], less: &dyn Fn(&T, &T) -> bool) {
    let len = v.len();
    if len < 2 {
        return;
    }
    let mut parent = (len - 2) / 2;
    loop {
        adjust_heap(v, parent, len, less);
        if parent == 0 {
            break;
        }
        parent -= 1;
    }
    let mut last = len;
    while last > 1 {
        last -= 1;
        // __pop_heap(first, last, last): the root goes to `last`, the old `v[last]` is
        // re-inserted from the root.
        v.swap(0, last);
        adjust_heap(v, 0, last, less);
    }
}

/// `std::__adjust_heap` then `std::__push_heap`, for the value at `v[top]`. libstdc++ moves the
/// value out and shifts elements into the hole; here the value rides along with the hole, which
/// makes the same comparisons and leaves every element where libstdc++ puts it.
fn adjust_heap<T>(v: &mut [T], top: usize, len: usize, less: &dyn Fn(&T, &T) -> bool) {
    let mut hole = top;
    let mut second = top;
    while second < (len - 1) / 2 {
        second = 2 * (second + 1);
        if less(&v[second], &v[second - 1]) {
            second -= 1;
        }
        v.swap(hole, second);
        hole = second;
    }
    if len & 1 == 0 && second == (len - 2) / 2 {
        second = 2 * (second + 1);
        v.swap(hole, second - 1);
        hole = second - 1;
    }
    while hole > top {
        let parent = (hole - 1) / 2;
        if !less(&v[parent], &v[hole]) {
            break;
        }
        v.swap(parent, hole);
        hole = parent;
    }
}

// ---------------------------------------------------------------------------------------------
// Wildcards and case folding

/// Simple case folding as `QChar::toCaseFolded` has it (Qt 6.11, Unicode 17), as runs of
/// `(first code point, count, step, delta)`: `first + k*step` folds to itself plus `delta`.
/// PCRE2 10.48, which Qt's `QRegularExpression` uses, matches two characters caselessly
/// exactly when they fold to the same character here.
const CASE_FOLD_RUNS: &[(u32, u32, u32, i32)] = &[
    (0x41, 26, 1, 32), (0xB5, 1, 1, 775), (0xC0, 23, 1, 32), (0xD8, 7, 1, 32), (0x100, 24, 2, 1),
    (0x132, 3, 2, 1), (0x139, 8, 2, 1), (0x14A, 23, 2, 1), (0x178, 1, 1, -121), (0x179, 3, 2, 1),
    (0x17F, 1, 1, -268), (0x181, 1, 1, 210), (0x182, 2, 2, 1), (0x186, 1, 1, 206),
    (0x187, 1, 1, 1), (0x189, 2, 1, 205), (0x18B, 1, 1, 1), (0x18E, 1, 1, 79),
    (0x18F, 1, 1, 202), (0x190, 1, 1, 203), (0x191, 1, 1, 1), (0x193, 1, 1, 205),
    (0x194, 1, 1, 207), (0x196, 1, 1, 211), (0x197, 1, 1, 209), (0x198, 1, 1, 1),
    (0x19C, 1, 1, 211), (0x19D, 1, 1, 213), (0x19F, 1, 1, 214), (0x1A0, 3, 2, 1),
    (0x1A6, 1, 1, 218), (0x1A7, 1, 1, 1), (0x1A9, 1, 1, 218), (0x1AC, 1, 1, 1),
    (0x1AE, 1, 1, 218), (0x1AF, 1, 1, 1), (0x1B1, 2, 1, 217), (0x1B3, 2, 2, 1),
    (0x1B7, 1, 1, 219), (0x1B8, 1, 1, 1), (0x1BC, 1, 1, 1), (0x1C4, 1, 1, 2), (0x1C5, 1, 1, 1),
    (0x1C7, 1, 1, 2), (0x1C8, 1, 1, 1), (0x1CA, 1, 1, 2), (0x1CB, 9, 2, 1), (0x1DE, 9, 2, 1),
    (0x1F1, 1, 1, 2), (0x1F2, 2, 2, 1), (0x1F6, 1, 1, -97), (0x1F7, 1, 1, -56),
    (0x1F8, 20, 2, 1), (0x220, 1, 1, -130), (0x222, 9, 2, 1), (0x23A, 1, 1, 10795),
    (0x23B, 1, 1, 1), (0x23D, 1, 1, -163), (0x23E, 1, 1, 10792), (0x241, 1, 1, 1),
    (0x243, 1, 1, -195), (0x244, 1, 1, 69), (0x245, 1, 1, 71), (0x246, 5, 2, 1),
    (0x345, 1, 1, 116), (0x370, 2, 2, 1), (0x376, 1, 1, 1), (0x37F, 1, 1, 116),
    (0x386, 1, 1, 38), (0x388, 3, 1, 37), (0x38C, 1, 1, 64), (0x38E, 2, 1, 63),
    (0x391, 17, 1, 32), (0x3A3, 9, 1, 32), (0x3C2, 1, 1, 1), (0x3CF, 1, 1, 8),
    (0x3D0, 1, 1, -30), (0x3D1, 1, 1, -25), (0x3D5, 1, 1, -15), (0x3D6, 1, 1, -22),
    (0x3D8, 12, 2, 1), (0x3F0, 1, 1, -54), (0x3F1, 1, 1, -48), (0x3F4, 1, 1, -60),
    (0x3F5, 1, 1, -64), (0x3F7, 1, 1, 1), (0x3F9, 1, 1, -7), (0x3FA, 1, 1, 1),
    (0x3FD, 3, 1, -130), (0x400, 16, 1, 80), (0x410, 32, 1, 32), (0x460, 17, 2, 1),
    (0x48A, 27, 2, 1), (0x4C0, 1, 1, 15), (0x4C1, 7, 2, 1), (0x4D0, 48, 2, 1),
    (0x531, 38, 1, 48), (0x10A0, 38, 1, 7264), (0x10C7, 1, 1, 7264), (0x10CD, 1, 1, 7264),
    (0x13F8, 6, 1, -8), (0x1C80, 1, 1, -6222), (0x1C81, 1, 1, -6221), (0x1C82, 1, 1, -6212),
    (0x1C83, 2, 1, -6210), (0x1C85, 1, 1, -6211), (0x1C86, 1, 1, -6204), (0x1C87, 1, 1, -6180),
    (0x1C88, 1, 1, 35267), (0x1C89, 1, 1, 1), (0x1C90, 43, 1, -3008), (0x1CBD, 3, 1, -3008),
    (0x1E00, 75, 2, 1), (0x1E9B, 1, 1, -58), (0x1E9E, 1, 1, -7615), (0x1EA0, 48, 2, 1),
    (0x1F08, 8, 1, -8), (0x1F18, 6, 1, -8), (0x1F28, 8, 1, -8), (0x1F38, 8, 1, -8),
    (0x1F48, 6, 1, -8), (0x1F59, 4, 2, -8), (0x1F68, 8, 1, -8), (0x1F88, 8, 1, -8),
    (0x1F98, 8, 1, -8), (0x1FA8, 8, 1, -8), (0x1FB8, 2, 1, -8), (0x1FBA, 2, 1, -74),
    (0x1FBC, 1, 1, -9), (0x1FBE, 1, 1, -7173), (0x1FC8, 4, 1, -86), (0x1FCC, 1, 1, -9),
    (0x1FD3, 1, 1, -7235), (0x1FD8, 2, 1, -8), (0x1FDA, 2, 1, -100), (0x1FE3, 1, 1, -7219),
    (0x1FE8, 2, 1, -8), (0x1FEA, 2, 1, -112), (0x1FEC, 1, 1, -7), (0x1FF8, 2, 1, -128),
    (0x1FFA, 2, 1, -126), (0x1FFC, 1, 1, -9), (0x2126, 1, 1, -7517), (0x212A, 1, 1, -8383),
    (0x212B, 1, 1, -8262), (0x2132, 1, 1, 28), (0x2160, 16, 1, 16), (0x2183, 1, 1, 1),
    (0x24B6, 26, 1, 26), (0x2C00, 48, 1, 48), (0x2C60, 1, 1, 1), (0x2C62, 1, 1, -10743),
    (0x2C63, 1, 1, -3814), (0x2C64, 1, 1, -10727), (0x2C67, 3, 2, 1), (0x2C6D, 1, 1, -10780),
    (0x2C6E, 1, 1, -10749), (0x2C6F, 1, 1, -10783), (0x2C70, 1, 1, -10782), (0x2C72, 1, 1, 1),
    (0x2C75, 1, 1, 1), (0x2C7E, 2, 1, -10815), (0x2C80, 50, 2, 1), (0x2CEB, 2, 2, 1),
    (0x2CF2, 1, 1, 1), (0xA640, 23, 2, 1), (0xA680, 14, 2, 1), (0xA722, 7, 2, 1),
    (0xA732, 31, 2, 1), (0xA779, 2, 2, 1), (0xA77D, 1, 1, -35332), (0xA77E, 5, 2, 1),
    (0xA78B, 1, 1, 1), (0xA78D, 1, 1, -42280), (0xA790, 2, 2, 1), (0xA796, 10, 2, 1),
    (0xA7AA, 1, 1, -42308), (0xA7AB, 1, 1, -42319), (0xA7AC, 1, 1, -42315),
    (0xA7AD, 1, 1, -42305), (0xA7AE, 1, 1, -42308), (0xA7B0, 1, 1, -42258),
    (0xA7B1, 1, 1, -42282), (0xA7B2, 1, 1, -42261), (0xA7B3, 1, 1, 928), (0xA7B4, 8, 2, 1),
    (0xA7C4, 1, 1, -48), (0xA7C5, 1, 1, -42307), (0xA7C6, 1, 1, -35384), (0xA7C7, 2, 2, 1),
    (0xA7CB, 1, 1, -42343), (0xA7CC, 8, 2, 1), (0xA7DC, 1, 1, -42561), (0xA7F5, 1, 1, 1),
    (0xAB70, 80, 1, -38864), (0xFB05, 1, 1, 1), (0xFF21, 26, 1, 32), (0x10400, 40, 1, 40),
    (0x104B0, 36, 1, 40), (0x10570, 11, 1, 39), (0x1057C, 15, 1, 39), (0x1058C, 7, 1, 39),
    (0x10594, 2, 1, 39), (0x10C80, 51, 1, 64), (0x10D50, 22, 1, 32), (0x118A0, 32, 1, 32),
    (0x16E40, 32, 1, 32), (0x16EA0, 25, 1, 27), (0x1E900, 34, 1, 34),
];

/// `QChar::toCaseFolded(c)`.
fn fold_case(c: u32) -> u32 {
    let i = CASE_FOLD_RUNS.partition_point(|&(first, _, _, _)| first <= c);
    if i == 0 {
        return c;
    }
    let (first, count, step, delta) = CASE_FOLD_RUNS[i - 1];
    let offset = c - first;
    if offset % step == 0 && offset / step < count {
        (c as i64 + i64::from(delta)) as u32
    } else {
        c
    }
}

/// Every character that folds to the same character as `c` (PCRE2's caseless set).
fn caseless_set(c: char) -> Vec<char> {
    let target = fold_case(c as u32);
    let mut set = vec![c];
    if let Some(t) = char::from_u32(target) {
        if t != c {
            set.push(t);
        }
    }
    for &(first, count, step, delta) in CASE_FOLD_RUNS {
        for k in 0..count {
            let from = first + k * step;
            if (from as i64 + i64::from(delta)) as u32 == target {
                if let Some(ch) = char::from_u32(from) {
                    if !set.contains(&ch) {
                        set.push(ch);
                    }
                }
            }
        }
    }
    set
}

#[derive(Debug)]
enum Token {
    Literal(char),
    AnyRun,
    AnyOne,
    Class { negated: bool, ranges: Vec<(char, char)> },
}

/// A name filter as `QRegularExpression::fromWildcard(pattern, cs)` compiles it, matched
/// against the whole name.
#[derive(Debug)]
struct Glob {
    tokens: Option<Vec<Token>>,
    case_sensitive: bool,
}

impl Glob {
    fn new(pattern: &str, case_sensitive: bool) -> Glob {
        Glob { tokens: parse_wildcard(pattern), case_sensitive }
    }

    fn matches(&self, name: &str) -> bool {
        let Some(tokens) = &self.tokens else { return false };
        let chars: Vec<char> = name.chars().collect();
        let one = |t: &Token, c: char| -> bool {
            match t {
                Token::Literal(l) => {
                    *l == c || (!self.case_sensitive && fold_case(*l as u32) == fold_case(c as u32))
                }
                Token::AnyOne | Token::AnyRun => c != '/',
                Token::Class { negated, ranges } => {
                    let in_class = |x: char| ranges.iter().any(|&(lo, hi)| lo <= x && x <= hi);
                    let hit = if self.case_sensitive {
                        in_class(c)
                    } else {
                        caseless_set(c).into_iter().any(in_class)
                    };
                    hit != *negated
                }
            }
        };
        // ok[s]: tokens[t..] match chars[s..]; filled from the last token backwards.
        let n = chars.len();
        let mut ok = vec![false; n + 1];
        ok[n] = true;
        for t in (0..tokens.len()).rev() {
            let mut next = vec![false; n + 1];
            for s in (0..=n).rev() {
                next[s] = match tokens[t] {
                    Token::AnyRun => ok[s] || (s < n && chars[s] != '/' && next[s + 1]),
                    ref tok => s < n && one(tok, chars[s]) && ok[s + 1],
                };
            }
            ok = next;
        }
        ok[0]
    }
}

/// `QRegularExpression::wildcardToRegularExpression` (Unix, path mode), parsed into tokens.
/// `None` when PCRE would reject the expression Qt builds (an unclosed class, "/" inside a
/// class, a range out of order, a trailing backslash) or when it uses regular-expression
/// features beyond plain characters (a backslash before a letter or digit, POSIX classes).
fn parse_wildcard(pattern: &str) -> Option<Vec<Token>> {
    let wc: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < wc.len() {
        let c = wc[i];
        i += 1;
        match c {
            '*' => {
                tokens.push(Token::AnyRun);
                while i < wc.len() && wc[i] == '*' {
                    i += 1;
                }
            }
            '?' => tokens.push(Token::AnyOne),
            // Qt passes a backslash through on Unix and escapes the next character on its own,
            // so PCRE sees regular-expression syntax ("\\." is a backslash and any character).
            '\\' => return None,
            '[' => {
                // Qt copies the class through, turning a leading "!" into "^" and a backslash
                // into an escaped backslash, and stops at the first "]" after the first char.
                let mut content: Vec<char> = Vec::new();
                let mut negated = false;
                if i < wc.len() && wc[i] == '!' {
                    negated = true;
                    i += 1;
                }
                if i < wc.len() && wc[i] == ']' {
                    content.push(']');
                    i += 1;
                }
                while i < wc.len() && wc[i] != ']' {
                    if wc[i] == '/' {
                        return None;
                    }
                    content.push(wc[i]);
                    i += 1;
                }
                if i >= wc.len() {
                    return None; // unterminated: Qt's ")" ends up inside the class
                }
                i += 1; // the "]"
                // PCRE: a "^" first (when "!" did not already negate) negates.
                let mut k = 0;
                if !negated && content.first() == Some(&'^') {
                    negated = true;
                    k = 1;
                }
                let mut ranges = Vec::new();
                let first_member = k;
                while k < content.len() {
                    let lo = content[k];
                    if lo == '[' && posix_syntax_at(&content, k) {
                        return None;
                    }
                    if k + 2 < content.len() && content[k + 1] == '-' {
                        let hi = content[k + 2];
                        if hi == '[' && posix_syntax_at(&content, k + 2) {
                            return None;
                        }
                        if hi < lo {
                            return None;
                        }
                        ranges.push((lo, hi));
                        k += 3;
                    } else {
                        ranges.push((lo, lo));
                        k += 1;
                    }
                }
                if ranges.is_empty() && first_member == content.len() {
                    // "[^]" style: PCRE reads the "]" as a member and runs past it.
                    return None;
                }
                tokens.push(Token::Class { negated, ranges });
            }
            other => tokens.push(Token::Literal(other)),
        }
    }
    Some(tokens)
}

/// PCRE2's `check_posix_syntax` for a "[" at `content[k]` inside a class Qt closes right after
/// `content`: "[:", "[." or "[=" opens a POSIX construct only if the same character comes
/// right before the closing "]" with no other "[" of that kind in between; otherwise the "["
/// is an ordinary member.
fn posix_syntax_at(content: &[char], k: usize) -> bool {
    let Some(&t) = content.get(k + 1) else { return false };
    if !matches!(t, ':' | '.' | '=') {
        return false;
    }
    let mut j = k + 2;
    while j < content.len() {
        let c = content[j];
        if c == '[' && content.get(j + 1) == Some(&t) {
            return false;
        }
        if c == t && j + 1 == content.len() {
            return true;
        }
        j += 1;
    }
    false
}

/// `QRegularExpression::fromWildcard(pattern, cs).match(name).hasMatch()`, the test
/// `QDir` name filters apply. See [`entry_info_list`] for the supported syntax.
pub fn wildcard_match(pattern: &str, name: &str, case_sensitive: bool) -> bool {
    Glob::new(pattern, case_sensitive).matches(name)
}

// ---------------------------------------------------------------------------------------------
// SHA-256

/// `QCryptographicHash::hash(data, QCryptographicHash::Sha256).toHex()`: 64 lowercase hex
/// digits.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (t, word) in block.chunks_exact(4).enumerate() {
            w[t] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for t in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[t]).wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh directory under the system temp directory, removed when dropped.
    pub(crate) struct TempDir(pub String);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> TempDir {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let path = std::env::temp_dir().join(format!(
                "omareel-{tag}-{}-{}-{nanos}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path.to_str().unwrap().to_string())
        }

        pub(crate) fn join(&self, name: &str) -> String {
            format!("{}/{name}", self.0)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::process::Command::new("chmod").args(["-R", "u+rwx", &self.0]).status();
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const CWD: &[u8] = "/tmp/paths-scratch/cwd dir/é".as_bytes();

    const STRINGS: &[[&str; 17]] = &[
        // Recorded from Qt 6.11.2 with the working directory "/tmp/paths-scratch/cwd dir/é":
        // input, cleanPath, isAbsolutePath, QDir.absolutePath, QDir.dirName, fileName, path,
        // absoluteFilePath, absolutePath, baseName, completeBaseName, suffix, completeSuffix,
        // QDir.filePath("x"), QDir.filePath("/abs"), QDir.filePath(""), QDir.absoluteFilePath("x")
        ["//a/./b/../c/", "/a/c", "1", "/a/c", "c", "", "//a/./b/../c", "/a/c/", "/a/c", "", "", "", "", "//a/./b/../c/x", "/abs", "//a/./b/../c", "/a/c/x"],
        ["../x", "../x", "0", "/tmp/paths-scratch/cwd dir/x", "x", "x", "..", "/tmp/paths-scratch/cwd dir/x", "/tmp/paths-scratch/cwd dir", "x", "x", "", "", "../x/x", "/abs", "../x", "/tmp/paths-scratch/cwd dir/x/x"],
        ["/..", "/..", "1", "/..", "..", "..", "/", "/..", "/", "", ".", "", ".", "/../x", "/abs", "/..", "/../x"],
        ["", "", "0", "/tmp/paths-scratch/cwd dir/é", ".", "", "", "", "", "", "", "", "", "./x", "/abs", ".", "/tmp/paths-scratch/cwd dir/é/x"],
        [".", ".", "0", "/tmp/paths-scratch/cwd dir/é", ".", ".", ".", "/tmp/paths-scratch/cwd dir/é", "/tmp/paths-scratch/cwd dir", "", "", "", "", "./x", "/abs", ".", "/tmp/paths-scratch/cwd dir/é/x"],
        ["a//b", "a/b", "0", "/tmp/paths-scratch/cwd dir/é/a/b", "b", "b", "a/", "/tmp/paths-scratch/cwd dir/é/a/b", "/tmp/paths-scratch/cwd dir/é/a", "b", "b", "", "", "a//b/x", "/abs", "a//b", "/tmp/paths-scratch/cwd dir/é/a/b/x"],
        ["~", "~", "0", "/tmp/paths-scratch/cwd dir/é/~", "~", "~", ".", "/tmp/paths-scratch/cwd dir/é/~", "/tmp/paths-scratch/cwd dir/é", "~", "~", "", "", "~/x", "/abs", "~", "/tmp/paths-scratch/cwd dir/é/~/x"],
        ["/", "/", "1", "/", "", "", "/", "/", "/", "", "", "", "", "/x", "/abs", "/", "/x"],
        ["//", "/", "1", "/", "", "", "/", "//", "/", "", "", "", "", "/x", "/abs", "/", "/x"],
        ["///", "/", "1", "/", "", "", "//", "//", "/", "", "", "", "", "//x", "/abs", "//", "/x"],
        ["./", ".", "0", "/tmp/paths-scratch/cwd dir/é", ".", "", ".", "/tmp/paths-scratch/cwd dir/é/", "/tmp/paths-scratch/cwd dir/é", "", "", "", "", "./x", "/abs", ".", "/tmp/paths-scratch/cwd dir/é/x"],
        ["../", "..", "0", "/tmp/paths-scratch/cwd dir", "..", "", "..", "/tmp/paths-scratch/cwd dir/", "/tmp/paths-scratch/cwd dir", "", "", "", "", "../x", "/abs", "..", "/tmp/paths-scratch/cwd dir/x"],
        ["/./", "/", "1", "/", ".", "", "/.", "//", "/", "", "", "", "", "/./x", "/abs", "/.", "/x"],
        ["/../", "/..", "1", "/..", "..", "", "/..", "/../", "/..", "", "", "", "", "/../x", "/abs", "/..", "/../x"],
        ["a/..", ".", "0", "/tmp/paths-scratch/cwd dir/é", "..", "..", "a", "/tmp/paths-scratch/cwd dir/é", "/tmp/paths-scratch/cwd dir", "", ".", "", ".", "a/../x", "/abs", "a/..", "/tmp/paths-scratch/cwd dir/é/x"],
        ["a/../..", "..", "0", "/tmp/paths-scratch/cwd dir", "..", "..", "a/..", "/tmp/paths-scratch/cwd dir", "/tmp/paths-scratch", "", ".", "", ".", "a/../../x", "/abs", "a/../..", "/tmp/paths-scratch/cwd dir/x"],
        ["/a/../..", "/..", "1", "/..", "..", "..", "/a/..", "/..", "/", "", ".", "", ".", "/a/../../x", "/abs", "/a/../..", "/../x"],
        [":/x", ":/x", "1", ":/x", "x", "x", ":", ":/x", ":/", "x", "x", "", "", ":/x/x", "/abs", ":/x", ":/x/x"],
        [":x/../y", "y", "1", ":x/../y", "y", "y", ":x/..", ":x/../y", ":x/..", "y", "y", "", "", ":x/../y/x", "/abs", ":x/../y", "y/x"],
        [":", ":", "1", ":/", "", "", ".", ":/", ":/", "", "", "", "", ":/x", "/abs", ":", ":/x"],
        ["a/b/", "a/b", "0", "/tmp/paths-scratch/cwd dir/é/a/b", "b", "", "a/b", "/tmp/paths-scratch/cwd dir/é/a/b/", "/tmp/paths-scratch/cwd dir/é/a/b", "", "", "", "", "a/b/x", "/abs", "a/b", "/tmp/paths-scratch/cwd dir/é/a/b/x"],
        ["a/b//", "a/b", "0", "/tmp/paths-scratch/cwd dir/é/a/b", "", "", "a/b/", "/tmp/paths-scratch/cwd dir/é/a/b/", "/tmp/paths-scratch/cwd dir/é/a/b", "", "", "", "", "a/b/x", "/abs", "a/b/", "/tmp/paths-scratch/cwd dir/é/a/b/x"],
        ["/a/b//", "/a/b", "1", "/a/b/", "", "", "/a/b/", "/a/b/", "/a/b", "", "", "", "", "/a/b/x", "/abs", "/a/b/", "/a/b/x"],
        ["/a/b/.", "/a/b", "1", "/a/b", ".", ".", "/a/b", "/a/b", "/a", "", "", "", "", "/a/b/./x", "/abs", "/a/b/.", "/a/b/x"],
        ["/a/b/..", "/a", "1", "/a", "..", "..", "/a/b", "/a", "/", "", ".", "", ".", "/a/b/../x", "/abs", "/a/b/..", "/a/x"],
        ["./a", "a", "0", "/tmp/paths-scratch/cwd dir/é/a", "a", "a", ".", "/tmp/paths-scratch/cwd dir/é/a", "/tmp/paths-scratch/cwd dir/é", "a", "a", "", "", "./a/x", "/abs", "./a", "/tmp/paths-scratch/cwd dir/é/a/x"],
        [".//a", "a", "0", "/tmp/paths-scratch/cwd dir/é/a", "a", "a", "./", "/tmp/paths-scratch/cwd dir/é/a", "/tmp/paths-scratch/cwd dir/é", "a", "a", "", "", ".//a/x", "/abs", ".//a", "/tmp/paths-scratch/cwd dir/é/a/x"],
        ["a/./", "a", "0", "/tmp/paths-scratch/cwd dir/é/a", ".", "", "a/.", "/tmp/paths-scratch/cwd dir/é/a/", "/tmp/paths-scratch/cwd dir/é/a", "", "", "", "", "a/./x", "/abs", "a/.", "/tmp/paths-scratch/cwd dir/é/a/x"],
        ["...", "...", "0", "/tmp/paths-scratch/cwd dir/é/...", "...", "...", ".", "/tmp/paths-scratch/cwd dir/é/...", "/tmp/paths-scratch/cwd dir/é", "", "..", "", "..", ".../x", "/abs", "...", "/tmp/paths-scratch/cwd dir/é/.../x"],
        ["x/.../y", "x/.../y", "0", "/tmp/paths-scratch/cwd dir/é/x/.../y", "y", "y", "x/...", "/tmp/paths-scratch/cwd dir/é/x/.../y", "/tmp/paths-scratch/cwd dir/é/x/...", "y", "y", "", "", "x/.../y/x", "/abs", "x/.../y", "/tmp/paths-scratch/cwd dir/é/x/.../y/x"],
        [".x", ".x", "0", "/tmp/paths-scratch/cwd dir/é/.x", ".x", ".x", ".", "/tmp/paths-scratch/cwd dir/é/.x", "/tmp/paths-scratch/cwd dir/é", "", "", "x", "x", ".x/x", "/abs", ".x", "/tmp/paths-scratch/cwd dir/é/.x/x"],
        ["x.", "x.", "0", "/tmp/paths-scratch/cwd dir/é/x.", "x.", "x.", ".", "/tmp/paths-scratch/cwd dir/é/x.", "/tmp/paths-scratch/cwd dir/é", "x", "x", "", "", "x./x", "/abs", "x.", "/tmp/paths-scratch/cwd dir/é/x./x"],
        ["a.b.c", "a.b.c", "0", "/tmp/paths-scratch/cwd dir/é/a.b.c", "a.b.c", "a.b.c", ".", "/tmp/paths-scratch/cwd dir/é/a.b.c", "/tmp/paths-scratch/cwd dir/é", "a", "a.b", "c", "b.c", "a.b.c/x", "/abs", "a.b.c", "/tmp/paths-scratch/cwd dir/é/a.b.c/x"],
        ["/.bashrc", "/.bashrc", "1", "/.bashrc", ".bashrc", ".bashrc", "/", "/.bashrc", "/", "", "", "bashrc", "bashrc", "/.bashrc/x", "/abs", "/.bashrc", "/.bashrc/x"],
        ["dir.x/file", "dir.x/file", "0", "/tmp/paths-scratch/cwd dir/é/dir.x/file", "file", "file", "dir.x", "/tmp/paths-scratch/cwd dir/é/dir.x/file", "/tmp/paths-scratch/cwd dir/é/dir.x", "file", "file", "", "", "dir.x/file/x", "/abs", "dir.x/file", "/tmp/paths-scratch/cwd dir/é/dir.x/file/x"],
        ["dir.x/file.tar.gz", "dir.x/file.tar.gz", "0", "/tmp/paths-scratch/cwd dir/é/dir.x/file.tar.gz", "file.tar.gz", "file.tar.gz", "dir.x", "/tmp/paths-scratch/cwd dir/é/dir.x/file.tar.gz", "/tmp/paths-scratch/cwd dir/é/dir.x", "file", "file.tar", "gz", "tar.gz", "dir.x/file.tar.gz/x", "/abs", "dir.x/file.tar.gz", "/tmp/paths-scratch/cwd dir/é/dir.x/file.tar.gz/x"],
        ["/tmp/", "/tmp", "1", "/tmp", "tmp", "", "/tmp", "/tmp/", "/tmp", "", "", "", "", "/tmp/x", "/abs", "/tmp", "/tmp/x"],
        ["tmp", "tmp", "0", "/tmp/paths-scratch/cwd dir/é/tmp", "tmp", "tmp", ".", "/tmp/paths-scratch/cwd dir/é/tmp", "/tmp/paths-scratch/cwd dir/é", "tmp", "tmp", "", "", "tmp/x", "/abs", "tmp", "/tmp/paths-scratch/cwd dir/é/tmp/x"],
        ["C:/x", "C:/x", "0", "/tmp/paths-scratch/cwd dir/é/C:/x", "x", "x", "C:", "/tmp/paths-scratch/cwd dir/é/C:/x", "/tmp/paths-scratch/cwd dir/é/C:", "x", "x", "", "", "C:/x/x", "/abs", "C:/x", "/tmp/paths-scratch/cwd dir/é/C:/x/x"],
        ["\\a\\b", "\\a\\b", "0", "/tmp/paths-scratch/cwd dir/é/\\a\\b", "\\a\\b", "\\a\\b", ".", "/tmp/paths-scratch/cwd dir/é/\\a\\b", "/tmp/paths-scratch/cwd dir/é", "\\a\\b", "\\a\\b", "", "", "\\a\\b/x", "/abs", "\\a\\b", "/tmp/paths-scratch/cwd dir/é/\\a\\b/x"],
        [" /x", " /x", "0", "/tmp/paths-scratch/cwd dir/é/ /x", "x", "x", " ", "/tmp/paths-scratch/cwd dir/é/ /x", "/tmp/paths-scratch/cwd dir/é/ ", "x", "x", "", "", " /x/x", "/abs", " /x", "/tmp/paths-scratch/cwd dir/é/ /x/x"],
        ["é/ü.ñ", "é/ü.ñ", "0", "/tmp/paths-scratch/cwd dir/é/é/ü.ñ", "ü.ñ", "ü.ñ", "é", "/tmp/paths-scratch/cwd dir/é/é/ü.ñ", "/tmp/paths-scratch/cwd dir/é/é", "ü", "ü", "ñ", "ñ", "é/ü.ñ/x", "/abs", "é/ü.ñ", "/tmp/paths-scratch/cwd dir/é/é/ü.ñ/x"],
        ["😀/😀.😀", "😀/😀.😀", "0", "/tmp/paths-scratch/cwd dir/é/😀/😀.😀", "😀.😀", "😀.😀", "😀", "/tmp/paths-scratch/cwd dir/é/😀/😀.😀", "/tmp/paths-scratch/cwd dir/é/😀", "😀", "😀", "😀", "😀", "😀/😀.😀/x", "/abs", "😀/😀.😀", "/tmp/paths-scratch/cwd dir/é/😀/😀.😀/x"],
        ["//a//b//", "/a/b", "1", "/a/b", "", "", "//a//b/", "/a/b/", "/a/b", "", "", "", "", "//a//b/x", "/abs", "//a//b/", "/a/b/x"],
        ["/a/./../b", "/b", "1", "/b", "b", "b", "/a/./..", "/b", "/", "b", "b", "", "", "/a/./../b/x", "/abs", "/a/./../b", "/b/x"],
        ["a/b/../../..", "..", "0", "/tmp/paths-scratch/cwd dir", "..", "..", "a/b/../..", "/tmp/paths-scratch/cwd dir", "/tmp/paths-scratch", "", ".", "", ".", "a/b/../../../x", "/abs", "a/b/../../..", "/tmp/paths-scratch/cwd dir/x"],
        ["../../a", "../../a", "0", "/tmp/paths-scratch/a", "a", "a", "../..", "/tmp/paths-scratch/a", "/tmp/paths-scratch", "a", "a", "", "", "../../a/x", "/abs", "../../a", "/tmp/paths-scratch/a/x"],
        ["/../../a", "/../../a", "1", "/../../a", "a", "a", "/../..", "/../../a", "/../..", "a", "a", "", "", "/../../a/x", "/abs", "/../../a", "/../../a/x"],
        ["/a/b/c/../../../..", "/..", "1", "/..", "..", "..", "/a/b/c/../../..", "/..", "/", "", ".", "", ".", "/a/b/c/../../../../x", "/abs", "/a/b/c/../../../..", "/../x"],
        ["./..", "..", "0", "/tmp/paths-scratch/cwd dir", "..", "..", ".", "/tmp/paths-scratch/cwd dir", "/tmp/paths-scratch", "", ".", "", ".", "./../x", "/abs", "./..", "/tmp/paths-scratch/cwd dir/x"],
        ["~/x", "~/x", "0", "/tmp/paths-scratch/cwd dir/é/~/x", "x", "x", "~", "/tmp/paths-scratch/cwd dir/é/~/x", "/tmp/paths-scratch/cwd dir/é/~", "x", "x", "", "", "~/x/x", "/abs", "~/x", "/tmp/paths-scratch/cwd dir/é/~/x/x"],
        ["a:b", "a:b", "0", "/tmp/paths-scratch/cwd dir/é/a:b", "a:b", "a:b", ".", "/tmp/paths-scratch/cwd dir/é/a:b", "/tmp/paths-scratch/cwd dir/é", "a:b", "a:b", "", "", "a:b/x", "/abs", "a:b", "/tmp/paths-scratch/cwd dir/é/a:b/x"],
    ];

    #[test]
    fn path_strings_match_qt() {
        for row in STRINGS {
            let p = row[0];
            let fi = FileInfo::new(p);
            let got = [
                p.to_string(),
                clean_path(p),
                if is_absolute_path(p) { "1" } else { "0" }.to_string(),
                absolute_path_in(p, CWD),
                dir_name(p),
                fi.file_name(),
                fi.path(),
                fi.absolute_file_path_in(CWD),
                fi.absolute_path_in(CWD),
                fi.base_name(),
                fi.complete_base_name(),
                fi.suffix(),
                fi.complete_suffix(),
                file_path(p, "x"),
                file_path(p, "/abs"),
                file_path(p, ""),
                absolute_file_path_in(p, "x", CWD),
            ];
            assert_eq!(got.map(|s| s.to_string()), row.map(|s| s.to_string()), "for {p:?}");
            assert_eq!(is_relative_path(p), !is_absolute_path(p));
        }
    }

    #[test]
    fn more_path_strings() {
        assert_eq!(file_path("/a/", "b"), "/a/b");
        assert_eq!(file_path("/a//", "b"), "/a/b");
        assert_eq!(file_path("/", "b"), "/b");
        assert_eq!(file_path("a", ":res"), ":res");
        assert_eq!(file_path("a", "a:b"), "a/a:b");
        assert_eq!(dir_path(""), ".");
        assert_eq!(dir_path("a//"), "a/");
        // An absolute, clean QDir path is returned as it is, trailing slash included.
        assert_eq!(absolute_path_in("/a/b//", CWD), "/a/b/");
        assert_eq!(absolute_path_in("/a/b/", CWD), "/a/b");
        assert_eq!(FileInfo::new("/a/b/").absolute_file_path_in(CWD), "/a/b/");
        assert_eq!(FileInfo::new("//").absolute_file_path_in(CWD), "//");
        assert_eq!(FileInfo::new("x/").absolute_file_path_in(b"/"), "/x/");
        assert_eq!(FileInfo::new(".").absolute_file_path_in(b"/w"), "/w");
        assert_eq!(FileInfo::new("").absolute_file_path_in(b"/w"), "");
        assert_eq!(FileInfo::new("").path(), "");
        assert_eq!(FileInfo::new("/x").path(), "/");
        assert_eq!(FileInfo::new(":").file_name(), "");
        assert_eq!(FileInfo::new(":").absolute_path_in(CWD), ":/");
        assert_eq!(absolute_path_in(":", CWD), ":/");
        assert_eq!(absolute_file_path_in(":", "x", CWD), ":/x");
        // A working directory that is not UTF-8 is decoded like QFile::decodeName.
        assert_eq!(FileInfo::new("a").absolute_file_path_in(b"/w\xe2\x82"), "/w\u{FFFD}\u{FFFD}/a");
    }

    #[test]
    fn home_and_temp_follow_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| v.to_string())
        };
        assert_eq!(home_path_with(&env(&[("HOME", "/home/u/")])), "/home/u");
        assert_eq!(home_path_with(&env(&[("HOME", "/home//u/./x/..")])), "/home/u");
        assert_eq!(home_path_with(&env(&[("HOME", "")])), "/");
        assert_eq!(home_path_with(&env(&[])), "/");
        assert_eq!(home_path_with(&env(&[("HOME", "rel/dir/")])), "rel/dir");
        assert_eq!(file_path(&home_path_with(&env(&[("HOME", "/")])), ".config"), "/.config");
        assert_eq!(temp_path_with(&env(&[]), b"/w"), "/tmp");
        assert_eq!(temp_path_with(&env(&[("TMPDIR", "")]), b"/w"), "/tmp");
        assert_eq!(temp_path_with(&env(&[("TMPDIR", "/t/")]), b"/w"), "/t");
        assert_eq!(temp_path_with(&env(&[("TMPDIR", "/t//")]), b"/w"), "/t/");
        assert_eq!(temp_path_with(&env(&[("TMPDIR", "rel")]), b"/w"), "/w/rel");
        assert_eq!(temp_path_with(&env(&[("TMPDIR", "/")]), b"/w"), "/");
    }

    #[test]
    fn decodes_names_like_qt() {
        // QFile::decodeName: one U+FFFD per byte that doesn't start a valid sequence, BOM dropped.
        let cases: &[(&[u8], &str)] = &[
            (b"\xe2\x82/x", "\u{FFFD}\u{FFFD}/x"),
            (b"\xff\xfe/x", "\u{FFFD}\u{FFFD}/x"),
            (b"\xc0\xaf/x", "\u{FFFD}\u{FFFD}/x"),
            (b"\xed\xa0\x80/x", "\u{FFFD}\u{FFFD}\u{FFFD}/x"),
            (b"\xf4\x90\x80\x80/x", "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}/x"),
            (b"\xe2\x82\xacA/x", "€A/x"),
            (b"\xf0\x9f\x98/x", "\u{FFFD}\u{FFFD}\u{FFFD}/x"),
            (b"\xef\xbf\xbe/x", "\u{FFFE}/x"),
            (b"\xef\xbb\xbfA/x", "A/x"),
            (b"\xf0(\x8c(/x", "\u{FFFD}(\u{FFFD}(/x"),
            (b"\xfc\x80\x80\x80\x80\x80/x", "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}/x"),
            (b"x\xe2\x82", "x\u{FFFD}\u{FFFD}"),
        ];
        for (bytes, want) in cases {
            assert_eq!(decode_name(bytes), *want, "{bytes:?}");
        }
        // QDirListing: an invalid name is skipped, an incomplete tail and a leading BOM vanish.
        assert_eq!(decode_listing_name(b"N\xe2\x82").as_deref(), Some("N"));
        assert_eq!(decode_listing_name(b"N\xc3").as_deref(), Some("N"));
        assert_eq!(decode_listing_name(b"N\xf0\x9f\x98").as_deref(), Some("N"));
        assert_eq!(decode_listing_name(b"\xef\xbb\xbfbom.png").as_deref(), Some("bom.png"));
        assert_eq!(decode_listing_name(b"N\xef\xbb\xbfA").as_deref(), Some("N\u{FEFF}A"));
        assert_eq!(decode_listing_name(b"bad\xff.png"), None);
        assert_eq!(decode_listing_name(b"N\xc3("), None);
        assert_eq!(decode_listing_name(b"N\xc0"), None);
        assert_eq!(decode_listing_name(b"ok\xef\xbf\xbe.png").as_deref(), Some("ok\u{FFFE}.png"));
    }

    #[test]
    fn folds_and_compares_case_like_qstring() {
        let folds = [
            (0x41, 0x61), (0x61, 0x61), (0xB5, 0x3BC), (0x130, 0x130), (0x131, 0x131), (0x17F, 0x73),
            (0x1E9E, 0xDF), (0x3A3, 0x3C3), (0x3C2, 0x3C3), (0x212A, 0x6B), (0x212B, 0xE5), (0x13A0, 0x13A0),
            (0xAB70, 0x13A0), (0xA7D4, 0xA7D5), (0xFB05, 0xFB06), (0xFF21, 0xFF41), (0x10400, 0x10428),
            (0x1E921, 0x1E943), (0x10FFFF, 0x10FFFF),
        ];
        for (c, f) in folds {
            assert_eq!(fold_case(c), f, "{c:X}");
        }
        let sign = |a: &str, b: &str| {
            let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
            (compare_utf16_ignore_case(&a, &b).signum(), compare_utf16(&a, &b).signum())
        };
        // (QString::compare case-insensitive, case-sensitive), recorded from Qt.
        let cases = [
            ("a", "B", -1, 1), ("B", "a", 1, -1), ("photo.png", "PHOTO.png", 0, 1), ("Photo.png", "photo.png", 0, -1),
            ("ß", "ẞ", 0, -1), ("ss", "ß", -1, -1), ("ſ", "s", 0, 1), ("\u{212A}", "k", 0, 1), ("！", "😀", 1, 1),
            ("𐐀", "𐐨", 0, -1), ("ς", "σ", 0, -1), ("İ", "i", 1, 1), ("abc", "ab", 1, 1), ("Z", "a", 1, -1),
            ("_", "a", -1, -1), ("_", "A", -1, 1),
        ];
        for (a, b, ci, cs) in cases {
            assert_eq!(sign(a, b), (ci, cs), "{a} vs {b}");
        }
    }

    #[test]
    fn wildcards_match_like_qregularexpression() {
        // (pattern, name, case-insensitive match, case-sensitive match), recorded from Qt.
        let cases = [
            ("*.json", "x.json", true, true), ("*.json", "x.JSON", true, false), ("*.json", "x.jſon", true, false),
            ("*.json", "x.jsonx", false, false), ("??????????.jpg", "0123456789.jpg", true, true),
            ("??????????.jpg", "012345678.jpg", false, false), ("??????????.jpg", "😀123456789.jpg", true, true),
            ("[!a-c]*", "A", false, true), ("[!a-c]*", "d", true, true), ("[!a-c]*", "é", true, true),
            ("[a-c]*", "B", true, false), ("[a-c]*", "\u{212A}", false, false), ("*k*", "\u{212A}", true, false),
            ("*k*", "K", true, false), ("[k]", "\u{212A}", true, false), ("[é]", "É", true, false), ("[]a]", "]", true, true),
            ("[]a]", "a", true, true), ("[!]a]", "]", false, false), ("[!]a]", "b", true, true), ("[", "a", false, false),
            ("[a-", "a", false, false), ("[z-a]", "z", false, false), ("a[/]b", "a/b", false, false), ("~", "~", true, true),
            ("~", "x", false, false), ("**", "", true, true), ("**", "x", true, true), ("*", "", true, true),
            ("?", "😀", true, true), ("[[.a]", "[", true, true), ("[[.a]", "a", true, true), ("[^ab]", "c", true, true),
            ("[^ab]", "a", false, false), ("event*", "Event2", true, false), ("", "", true, true), ("", "x", false, false),
        ];
        for (pattern, name, ci, cs) in cases {
            assert_eq!(wildcard_match(pattern, name, false), ci, "{pattern:?} ~ {name:?}");
            assert_eq!(wildcard_match(pattern, name, true), cs, "{pattern:?} = {name:?}");
        }
    }

    #[test]
    fn std_sort_places_equal_elements_like_libstdcxx() {
        // Keys from the same generator the C++ recording used; the ids are libstdc++'s order.
        let keys = |n: usize, seed: u32, modulo: u32| -> Vec<(u32, usize)> {
            let mut x = seed;
            (0..n)
                .map(|i| {
                    x = x.wrapping_mul(1103515245).wrapping_add(12345);
                    ((x >> 16) % modulo, i)
                })
                .collect()
        };
        let sorted = |mut v: Vec<(u32, usize)>| -> Vec<usize> {
            std_sort(&mut v, &|a: &(u32, usize), b: &(u32, usize)| a.0 < b.0);
            v.into_iter().map(|p| p.1).collect()
        };
        assert_eq!(sorted(keys(10, 12345, 5)), [2, 4, 5, 7, 8, 0, 1, 3, 6, 9]);
        assert_eq!(sorted(keys(17, 12345, 5)), [2, 14, 4, 5, 8, 7, 16, 0, 1, 10, 11, 13, 12, 3, 15, 6, 9]);
        assert_eq!(
            sorted(keys(40, 12345, 5)),
            [8, 17, 26, 14, 21, 7, 5, 4, 31, 2, 35, 23, 33, 22, 36, 37, 18, 16, 38, 0, 1, 13, 11, 10, 30, 15, 12, 3, 34,
             32, 39, 29, 28, 27, 25, 24, 20, 19, 9, 6]
        );
        assert_eq!(
            sorted(keys(100, 12345, 5)),
            [26, 52, 54, 58, 45, 44, 43, 64, 35, 65, 31, 75, 21, 80, 81, 17, 8, 14, 86, 2, 4, 5, 7, 16, 97,
             36, 37, 38, 41, 0, 48, 49, 1, 66, 85, 18, 22, 23, 77, 76, 92, 70, 33, 63, 99, 61, 98, 78, 68,
             90, 94, 53, 50, 47, 10, 11, 40, 13, 72, 3, 73, 74, 82, 83, 15, 87, 88, 89, 12, 96, 51, 55, 69,
             30, 56, 57, 46, 59, 42, 62, 34, 6, 9, 95, 93, 91, 60, 39, 71, 84, 32, 67, 19, 20, 79, 24, 25,
             27, 28, 29]
        );
        let v300: Vec<(u32, usize)> = (0..300).map(|i| ((i * 7919 % 13) as u32, i)).collect();
        assert_eq!(
            sorted(v300),
            [299, 52, 65, 39, 78, 91, 104, 117, 130, 143, 26, 0, 156, 169, 182, 195, 260, 208, 221, 273, 13,
             286, 234, 247, 72, 98, 85, 280, 124, 293, 59, 150, 46, 267, 111, 254, 137, 241, 228, 163, 176,
             189, 215, 202, 20, 7, 33, 261, 118, 274, 40, 66, 209, 105, 1, 14, 222, 79, 235, 92, 248, 183,
             131, 287, 157, 144, 53, 196, 170, 27, 138, 86, 151, 242, 164, 229, 34, 99, 21, 177, 216, 112,
             125, 203, 190, 60, 73, 268, 47, 281, 294, 255, 8, 158, 262, 106, 15, 210, 184, 67, 275, 145,
             119, 41, 197, 93, 2, 249, 236, 28, 171, 80, 288, 54, 132, 223, 113, 178, 126, 22, 282, 139, 165,
             100, 9, 87, 243, 35, 152, 230, 295, 256, 191, 74, 217, 269, 204, 48, 61, 159, 172, 289, 3, 185,
             198, 276, 16, 211, 263, 224, 250, 237, 133, 94, 29, 81, 55, 42, 107, 146, 120, 68, 75, 179, 127,
             62, 192, 205, 114, 270, 257, 218, 101, 244, 231, 36, 88, 10, 166, 153, 296, 49, 23, 140, 283,
             212, 238, 95, 43, 82, 225, 147, 160, 251, 108, 134, 264, 17, 69, 290, 199, 121, 56, 186, 4, 30,
             173, 277, 193, 141, 102, 63, 245, 180, 232, 24, 11, 297, 154, 89, 219, 50, 37, 76, 167, 284,
             128, 206, 258, 271, 115, 265, 252, 226, 70, 291, 278, 83, 5, 57, 239, 148, 161, 44, 174, 135,
             187, 18, 122, 200, 31, 213, 109, 96, 220, 12, 181, 285, 129, 246, 233, 51, 64, 194, 272, 168,
             142, 25, 90, 116, 207, 38, 155, 259, 298, 103, 77, 292, 162, 240, 149, 136, 253, 45, 175, 58,
             279, 188, 19, 123, 84, 227, 201, 6, 266, 97, 71, 32, 110, 214]
        );
        let heap = |mut v: Vec<(u32, usize)>| -> Vec<usize> {
            heap_sort(&mut v, &|a: &(u32, usize), b: &(u32, usize)| a.0 < b.0);
            v.into_iter().map(|p| p.1).collect()
        };
        assert_eq!(heap(keys(3, 777, 4)), [0, 2, 1]);
        assert_eq!(heap(keys(7, 777, 4)), [0, 3, 2, 1, 4, 5, 6]);
        assert_eq!(heap(keys(16, 777, 4)), [3, 7, 0, 12, 10, 2, 9, 13, 14, 15, 8, 1, 4, 5, 11, 6]);
        assert_eq!(
            heap(keys(33, 777, 4)),
            [7, 18, 10, 3, 0, 12, 30, 19, 13, 20, 9, 21, 14, 2, 27, 28, 32, 8, 1, 4, 11, 5, 26, 15, 6, 29, 31, 16, 17, 22,
             23, 24, 25]
        );
    }

    #[test]
    fn sha256_like_qcryptographichash() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let pattern: Vec<u8> = (0..1000).map(|i| (i % 256) as u8).collect();
        assert_eq!(sha256_hex(&pattern), "a8af099bf2e878609558dbf69d8f88f4a31040a8cf84b549a0cfa912f12ffc3f");
        // OmarchyPaths::wallpaperThumbnailPath's identity: path, size and mtime joined by NULs.
        assert_eq!(
            sha256_hex(b"/tmp/x.png\x001234\x001700000000000"),
            "e936cec272a0d94f07ffca129507571c9d1d725bcd5fbc3e0159eaad4ed3e274"
        );
    }

    #[test]
    fn error_strings_follow_qt_system_error() {
        assert_eq!(error_string(libc::ENOENT), "No such file or directory");
        assert_eq!(error_string(libc::EACCES), "Permission denied");
        assert_eq!(error_string(libc::ENOSPC), "No space left on device");
        assert_eq!(error_string(libc::EISDIR), "Is a directory");
        assert_eq!(error_string(0), "");
    }

    /// The tree the listing goldens were recorded over.
    fn listing_tree() -> TempDir {
        let dir = TempDir::new("qdir-list");
        let files: &[(&str, usize, i64)] = &[
            ("A.PNG", 3, 5), ("b.Jpg", 0, 2), ("c.png", 7, 9), ("C.png", 1, 9), ("img10.png", 4, 1), ("img2.png", 4, 3),
            ("img1.png", 2, 4), ("space name.png", 5, 6), ("é.png", 6, 7), ("É.png", 8, 8), (".hidden.png", 1, 1),
            ("ſ.json", 2, 2), ("s.json", 3, 3), ("noext", 0, 4), ("a.tar.gz", 9, 5), ("0123456789.jpg", 1, 6),
            ("012345678.jpg", 2, 7), ("z.webp", 3, 8),
        ];
        for &(name, size, t) in files {
            let path = dir.join(name);
            std::fs::write(&path, vec![b'x'; size]).unwrap();
            let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000 + t as u64);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        for d in ["dir", "Dir2", ".hiddendir", "zdir.png"] {
            std::fs::create_dir(dir.join(d)).unwrap();
        }
        std::os::unix::fs::symlink("c.png", dir.join("link_to_file.png")).unwrap();
        std::os::unix::fs::symlink("dir", dir.join("link_to_dir")).unwrap();
        std::os::unix::fs::symlink("nowhere.png", dir.join("broken.png")).unwrap();
        std::os::unix::fs::symlink("/dev/null", dir.join("link_to_devnull")).unwrap();
        dir
    }

    #[test]
    fn listings_match_qt() {
        let tree = listing_tree();
        let images = ["*.png", "*.jpg", "*.jpeg", "*.webp"];
        let f = |bits: i32| Filters(bits);
        let s = |bits: i32| SortFlags(bits);
        // (name filters, filters, sort, the names Qt lists, comma-separated)
        let cases: &[(&[&str], Filters, SortFlags, &str)] = &[
            (&images, Filters::FILES, SortFlags::NAME,
             "012345678.jpg,0123456789.jpg,A.PNG,C.png,b.Jpg,c.png,img1.png,img10.png,img2.png,link_to_file.png,space name.png,z.webp,É.png,é.png"),
            (&[], Filters::DIRS | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME, "Dir2,dir,link_to_devnull,link_to_dir,zdir.png"),
            (&["*.json"], Filters::FILES, SortFlags::NAME, "s.json,ſ.json"),
            (&[], Filters::FILES | Filters::DIRS | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME,
             "012345678.jpg,0123456789.jpg,A.PNG,C.png,Dir2,a.tar.gz,b.Jpg,c.png,dir,img1.png,img10.png,img2.png,link_to_devnull,link_to_dir,link_to_file.png,noext,s.json,space name.png,z.webp,zdir.png,É.png,é.png,ſ.json"),
            (&[], Filters::FILES | Filters::SYSTEM | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME,
             "012345678.jpg,0123456789.jpg,A.PNG,C.png,a.tar.gz,b.Jpg,broken.png,c.png,img1.png,img10.png,img2.png,link_to_devnull,link_to_file.png,noext,s.json,space name.png,z.webp,É.png,é.png,ſ.json"),
            (&["??????????.jpg"], Filters::FILES, SortFlags::NO_SORT, "0123456789.jpg"),
            (&[], Filters::ALL_ENTRIES | Filters::HIDDEN, SortFlags::NAME,
             ".,..,.hidden.png,.hiddendir,012345678.jpg,0123456789.jpg,A.PNG,C.png,Dir2,a.tar.gz,b.Jpg,c.png,dir,img1.png,img10.png,img2.png,link_to_devnull,link_to_dir,link_to_file.png,noext,s.json,space name.png,z.webp,zdir.png,É.png,é.png,ſ.json"),
            (&["*.png"], Filters::FILES | Filters::CASE_SENSITIVE, SortFlags::NAME,
             "C.png,c.png,img1.png,img10.png,img2.png,link_to_file.png,space name.png,É.png,é.png"),
            (&["*.*"], Filters::FILES, SortFlags::TIME,
             "C.png,c.png,link_to_file.png,z.webp,É.png,012345678.jpg,é.png,0123456789.jpg,space name.png,A.PNG,a.tar.gz,img1.png,img2.png,s.json,b.Jpg,ſ.json,img10.png"),
            (&["*.*"], Filters::FILES, SortFlags::SIZE | SortFlags::REVERSED,
             "b.Jpg,C.png,0123456789.jpg,ſ.json,img1.png,012345678.jpg,z.webp,s.json,A.PNG,img2.png,img10.png,space name.png,é.png,link_to_file.png,c.png,É.png,a.tar.gz"),
            (&["*.*"], Filters::FILES, SortFlags::TIME | SortFlags::REVERSED,
             "img10.png,ſ.json,b.Jpg,s.json,img2.png,img1.png,a.tar.gz,A.PNG,space name.png,0123456789.jpg,é.png,012345678.jpg,É.png,z.webp,link_to_file.png,c.png,C.png"),
            (&[], f(0x6007), s(0x04),
             "Dir2,dir,link_to_dir,zdir.png,012345678.jpg,0123456789.jpg,A.PNG,C.png,a.tar.gz,b.Jpg,c.png,img1.png,img10.png,img2.png,link_to_devnull,link_to_file.png,noext,s.json,space name.png,z.webp,É.png,é.png,ſ.json"),
            (&[], f(0x6007), s(0x20),
             "012345678.jpg,0123456789.jpg,A.PNG,C.png,a.tar.gz,b.Jpg,c.png,img1.png,img10.png,img2.png,link_to_devnull,link_to_file.png,noext,s.json,space name.png,z.webp,É.png,é.png,ſ.json,Dir2,dir,link_to_dir,zdir.png"),
            (&[], f(0x6003), s(0x80),
             "Dir2,dir,link_to_devnull,link_to_dir,noext,b.Jpg,A.PNG,a.tar.gz,012345678.jpg,0123456789.jpg,s.json,ſ.json,C.png,c.png,img1.png,img10.png,img2.png,link_to_file.png,space name.png,zdir.png,É.png,é.png,z.webp"),
            (&[], Filters::FILES | Filters::DIRS | Filters::NO_SYMLINKS | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME,
             "012345678.jpg,0123456789.jpg,A.PNG,C.png,Dir2,a.tar.gz,b.Jpg,c.png,dir,img1.png,img10.png,img2.png,noext,s.json,space name.png,z.webp,zdir.png,É.png,é.png,ſ.json"),
            (&["*.json"], Filters::ALL_DIRS | Filters::DIRS | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME, "Dir2,dir,link_to_dir,zdir.png"),
            (&["[!a-c]*"], Filters::FILES, SortFlags::NAME,
             "012345678.jpg,0123456789.jpg,img1.png,img10.png,img2.png,link_to_devnull,link_to_file.png,noext,s.json,space name.png,z.webp,É.png,é.png,ſ.json"),
            (&["*.PNG"], Filters::FILES, SortFlags::NAME, "A.PNG,C.png,c.png,img1.png,img10.png,img2.png,link_to_file.png,space name.png,É.png,é.png"),
        ];
        for (name_filters, filters, sort, want) in cases {
            let want: Vec<&str> = want.split(',').collect();
            assert_eq!(entry_list(&tree.0, name_filters, *filters, *sort), want, "{name_filters:?} {filters:?} {sort:?}");
            let infos = entry_info_list(&tree.0, name_filters, *filters, *sort);
            let names: Vec<String> = infos.iter().map(|i| i.file_name()).collect();
            assert_eq!(names, want);
            for info in &infos {
                assert_eq!(info.file_path(), format!("{}/{}", tree.0, info.file_name()));
            }
        }
        // The same listing through other spellings of the directory.
        let plain = entry_list(&tree.0, &images, Filters::FILES, SortFlags::NAME);
        assert_eq!(entry_list(&format!("{}/", tree.0), &images, Filters::FILES, SortFlags::NAME), plain);
        assert_eq!(entry_info_list(&format!("{}/", tree.0), &[], Filters::DIRS, SortFlags::NAME)[0].file_path(), format!("{}/.", tree.0));
        assert!(entry_list(&tree.join("nowhere"), &[], Filters::NO_FILTER, SortFlags::NO_SORT).is_empty());
        assert!(entry_list(&tree.join("c.png"), &[], Filters::NO_FILTER, SortFlags::NO_SORT).is_empty());
        assert_eq!(entry_list(&tree.join("link_to_dir"), &[], Filters::NO_FILTER, SortFlags::NO_SORT), [".", ".."]);
        // An unsorted entryList ignores the permission and NoDot filters.
        std::fs::write(tree.join("dir/locked.png"), b"").unwrap();
        std::fs::set_permissions(tree.join("dir/locked.png"), std::fs::Permissions::from_mode(0o000)).unwrap();
        let readable = Filters::FILES | Filters::READABLE | Filters::NO_DOT_AND_DOT_DOT;
        if !FileInfo::new(tree.join("dir/locked.png")).is_readable() {
            assert!(entry_list(&tree.join("dir"), &[], readable, SortFlags::NAME).is_empty());
            assert!(entry_info_list(&tree.join("dir"), &[], readable, SortFlags::UNSORTED).is_empty());
        }
        assert_eq!(entry_list(&tree.join("dir"), &[], readable, SortFlags::UNSORTED), ["locked.png"]);
        let no_dot = Filters::ALL_ENTRIES | Filters::NO_DOT;
        assert_eq!(entry_list(&tree.join("dir"), &[], no_dot, SortFlags::NAME), ["..", "locked.png"]);
        let mut unsorted = entry_list(&tree.join("dir"), &[], no_dot, SortFlags::UNSORTED);
        unsorted.sort();
        assert_eq!(unsorted, [".", "..", "locked.png"]);
    }

    #[test]
    fn listing_decodes_names_like_qdirlisting() {
        use std::os::unix::ffi::OsStrExt;
        let tree = TempDir::new("qdir-names");
        for raw in [&b"N\xe2\x82"[..], b"bad\xff.png", b"\xef\xbb\xbfbom.png", b"\xef\xbb\xbf", b"ok\xef\xbf\xbe.png", b"\xe2\x82"] {
            let path = std::path::Path::new(&tree.0).join(std::ffi::OsStr::from_bytes(raw));
            std::fs::write(path, b"x").unwrap();
        }
        assert_eq!(entry_list(&tree.0, &[], Filters::FILES, SortFlags::NAME), ["N", "bom.png", "ok\u{FFFE}.png"]);
    }

    #[test]
    fn file_info_facts() {
        let tree = listing_tree();
        let info = |name: &str| FileInfo::new(tree.join(name));
        let link = info("link_to_file.png");
        assert!(link.exists() && link.is_file() && link.is_symlink() && !link.is_dir());
        assert_eq!(link.sym_link_target(), tree.join("c.png"));
        assert_eq!(link.canonical_file_path(), FileInfo::new(tree.join("c.png")).canonical_file_path());
        assert_eq!(link.size(), 7);
        assert_eq!(link.last_modified_msecs(), 1_600_000_009_000);
        let broken = info("broken.png");
        assert!(!broken.exists() && !broken.is_file() && broken.is_symlink());
        assert_eq!(broken.sym_link_target(), tree.join("nowhere.png"));
        assert_eq!(broken.canonical_file_path(), "");
        assert_eq!(broken.canonical_path(), ".");
        assert_eq!(broken.size(), 0);
        assert_eq!(broken.last_modified_msecs(), 0);
        let dir_link = info("link_to_dir");
        assert!(dir_link.is_dir() && dir_link.is_symlink());
        assert_eq!(dir_link.sym_link_target(), tree.join("dir"));
        assert_eq!(info("c.png").sym_link_target(), "");
        assert!(info(".hidden.png").is_hidden() && info(".hiddendir/").is_hidden() && !info("c.png").is_hidden());
        assert!(info("dir/").is_dir() && !info("c.png/").exists() && !info("c.png/").is_file());
        assert_eq!(info("c.png/").canonical_file_path(), "");
        assert!(FileInfo::new("/").is_dir() && !FileInfo::new("/").is_hidden());
        assert!(!FileInfo::new("").exists() && !FileInfo::new("").is_dir() && FileInfo::new("").canonical_file_path().is_empty());
        assert!(!FileInfo::new(":/x").exists() && !FileInfo::new(":").is_dir());
        assert!(file_exists(&tree.join("dir")) && !file_exists("") && !file_exists(&tree.join("broken.png")));
        assert!(dir_exists(&tree.join("link_to_dir")) && !dir_exists(&tree.join("c.png")) && dir_exists(&tree.0));
        assert!(dir_contains(&tree.0, "c.png") && !dir_contains(&tree.0, "") && !dir_contains(&tree.0, "nope"));
        // A loop is an error other than "doesn't exist": Qt then hands back the path as given.
        std::os::unix::fs::symlink("loop2", tree.join("loop1")).unwrap();
        std::os::unix::fs::symlink("loop1", tree.join("loop2")).unwrap();
        assert_eq!(info("loop1").canonical_file_path(), tree.join("loop1"));
        assert_eq!(info("loop1").canonical_path(), tree.0);
    }

    #[test]
    fn mkpath_and_remove_recursively() {
        let tree = TempDir::new("qdir-mkpath");
        assert!(mkpath(&tree.join("a/b/c")));
        assert!(FileInfo::new(tree.join("a/b/c")).is_dir());
        assert!(mkpath(&tree.join("a/b/c/")));
        assert!(mkpath(&tree.join("x//y///")));
        // Like Qt, "p/../q" also makes "p".
        assert!(mkpath(&tree.join("p/../q")));
        assert!(dir_exists(&tree.join("p")) && dir_exists(&tree.join("q")));
        assert!(!mkpath(""));
        std::fs::write(tree.join("file"), b"").unwrap();
        assert!(!mkpath(&tree.join("file")));
        assert!(!mkpath(&tree.join("file/sub")));
        assert!(mkpath(&file_path(&tree.join("new"), ".")));

        std::fs::write(tree.join("a/b/f"), b"x").unwrap();
        std::fs::write(tree.join("a/.hidden"), b"x").unwrap();
        std::os::unix::fs::symlink(tree.join("q"), tree.join("a/link")).unwrap();
        std::os::unix::fs::symlink("nowhere", tree.join("a/broken")).unwrap();
        assert!(remove_recursively(&tree.join("a")));
        assert!(!file_exists(&tree.join("a")) && dir_exists(&tree.join("q")));
        assert!(remove_recursively(&tree.join("nonexistent")));
        assert!(remove_recursively(&tree.join("file")) && file_exists(&tree.join("file")));
        // Through a symlink to a directory: the contents go, the rmdir of the link fails.
        std::fs::write(tree.join("q/inside"), b"x").unwrap();
        std::os::unix::fs::symlink("q", tree.join("qlink")).unwrap();
        assert!(!remove_recursively(&tree.join("qlink")));
        assert!(dir_exists(&tree.join("q")) && !file_exists(&tree.join("q/inside")));
    }
}
