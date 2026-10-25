//! `QLockFile` on Linux (Qt 6.11.2, `qlockfile.cpp` and `qlockfile_unix.cpp`), so that omareel's
//! Rust code and the C++ binary (and any Qt program) exclude each other on the same lock files.
//!
//! What Qt does, reproduced here:
//! - The lock is the file itself, at the path given (omareel locks `recording.json.lock` and
//!   `selfview-host.json.host-lock`). `try_lock` creates it with `O_RDWR | O_CREAT | O_EXCL`
//!   (mode 0666 less the umask, close-on-exec), takes a BSD `flock(LOCK_EX | LOCK_NB)` on it so
//!   nobody else can remove it while it is held, writes its contents, `fdatasync`s it and keeps
//!   the descriptor open until `unlock`. A write that falls short (a full disk) removes the file
//!   again and fails with [`LockError::UnknownError`].
//! - The contents are five lines, each ending in `\n`: the pid, the process name (the file name
//!   of `/proc/self/exe`, without a ` (deleted)` suffix: Qt writes `processNameByPid`, not
//!   `QCoreApplication::applicationName`), the host name (`uname`'s node name), the machine id (up
//!   to 32 bytes of `/var/lib/dbus/machine-id`, else `/usr/local/var/lib/dbus/machine-id`) and the
//!   boot id (`/proc/sys/kernel/random/boot_id`, its 36 characters exactly or nothing).
//! - An existing file is a lock held by someone else ([`LockError::LockFailedError`]); a
//!   directory that may not be written (EACCES, EROFS) is [`LockError::PermissionError`] and any
//!   other failure [`LockError::UnknownError`], both returned at once without waiting.
//! - A held lock is stale when its first line is a pid above 0 and, on the same machine (the
//!   machine id when both have one, else an empty or equal host name), the boot id differs or
//!   the process is not running: `kill(pid, 0)` fails with ESRCH, or the name of the process now
//!   using the pid (`/proc/<pid>/exe`, `/ERROR/` when that can't be read) differs from the file
//!   name of the second line. It is also stale when its modification time is further than the
//!   stale lock time (30 s by default, 0 meaning never) from now, in either direction. A stale
//!   lock is removed under a second lock, `<name>.rmlock` (tried once), by opening it, taking its
//!   `flock` (which fails while its owner still holds it) and unlinking it; then locking is tried
//!   again at once.
//! - `try_lock(timeout)` tries, then sleeps 100 ms, 200 ms, 400 ms... (doubling up to at least
//!   5 s, each sleep cut to what is left) and tries again until the timeout has passed; a
//!   negative timeout waits for ever ([`LockFile::lock`]). Lock lines are read as `QFile` reads
//!   them in text mode: a `\r\n` ending becomes `\n`, and each line loses its last byte.
//! - `unlock` (and dropping a held lock) unlinks the file and closes the descriptor, which drops
//!   the `flock`.
//!
//! Not reproduced: Qt's `qWarning`/`qInfo` messages (a failed `flock`, a lock file that could not
//! be removed, a modification time in the future), `getLockInfo` beyond what staleness needs,
//! and the other platforms' code paths. The machine and boot ids and the host name are also
//! available on their own, as `QSysInfo` has them.

use super::qdir::{self, FileInfo};
use super::qtemporaryfile::{errno, have_procfs, open_raw};
use std::ffi::CString;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

/// `QLockFile::LockError`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LockError {
    #[default]
    NoError,
    /// Another process (or this one, through another `LockFile`) holds the lock.
    LockFailedError,
    /// The lock file could not be created for lack of permissions in its directory.
    PermissionError,
    /// Anything else, such as a full partition.
    UnknownError,
}

/// `QLockFile`.
#[derive(Debug)]
pub struct LockFile {
    file_name: String,
    fd: libc::c_int,
    stale_lock_time_ms: i64,
    lock_error: LockError,
    is_locked: bool,
}

/// `QSysInfo::machineHostName()`: the node name `uname` reports.
pub fn machine_host_name() -> String {
    // SAFETY: a zeroed utsname is valid for uname to fill in.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `name` is a valid utsname.
    if unsafe { libc::uname(&mut name) } != 0 {
        return String::new();
    }
    let bytes: Vec<u8> = name.nodename.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    qdir::decode_name(&bytes)
}

/// Reads up to `limit` bytes of `path`; the `errno` when it can't be opened or read.
fn read_up_to(path: &str, limit: usize) -> Result<Vec<u8>, i32> {
    let fd = open_raw(path.as_bytes(), libc::O_RDONLY, 0)?;
    let mut buffer = vec![0u8; limit];
    let read = loop {
        // SAFETY: `buffer` has `limit` bytes; `fd` is open.
        let r = unsafe { libc::read(fd, buffer.as_mut_ptr().cast(), limit) };
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        break r;
    };
    // SAFETY: `fd` is open and closed once.
    unsafe { libc::close(fd) };
    if read < 0 {
        return Err(libc::EIO);
    }
    buffer.truncate(read as usize);
    Ok(buffer)
}

/// `QSysInfo::machineUniqueId()` on Linux: up to 32 bytes of the D-Bus machine id file, empty
/// when there is none.
pub fn machine_unique_id() -> Vec<u8> {
    match read_up_to("/var/lib/dbus/machine-id", 32) {
        Ok(bytes) => bytes,
        Err(libc::ENOENT) => read_up_to("/usr/local/var/lib/dbus/machine-id", 32).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// `QSysInfo::bootUniqueId()` on Linux: the 36-character boot id, empty when it can't be read
/// whole.
pub fn boot_unique_id() -> Vec<u8> {
    match read_up_to("/proc/sys/kernel/random/boot_id", 36) {
        Ok(bytes) if bytes.len() == 36 => bytes,
        _ => Vec::new(),
    }
}

/// `QLockFilePrivate::processNameByPid`: the file name of `/proc/<pid>/exe` without a
/// ` (deleted)` suffix, `/ERROR/` when that link can't be read, empty without `/proc`.
pub fn process_name_by_pid(pid: i64) -> String {
    if !have_procfs() {
        return String::new();
    }
    let Ok(target) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return "/ERROR/".into() };
    let mut bytes = std::os::unix::ffi::OsStrExt::as_bytes(target.as_os_str()).to_vec();
    if bytes.is_empty() {
        return "/ERROR/".into();
    }
    if bytes.ends_with(b" (deleted)") {
        bytes.truncate(bytes.len() - b" (deleted)".len());
    }
    let name = bytes.rsplit(|&b| b == b'/').next().unwrap_or(&bytes);
    qdir::decode_name(name)
}

/// `QLockFilePrivate::lockFileContents()`.
pub fn lock_file_contents() -> Vec<u8> {
    let pid = i64::from(std::process::id());
    let mut data = format!("{pid}\n{}\n{}\n", process_name_by_pid(pid), machine_host_name()).into_bytes();
    data.extend_from_slice(&machine_unique_id());
    data.push(b'\n');
    data.extend_from_slice(&boot_unique_id());
    data.push(b'\n');
    data
}

/// `QLockFilePrivate::LockFileInfo`.
struct LockFileInfo {
    pid: i64,
    appname: String,
    hostname: String,
    hostid: Vec<u8>,
    bootid: Vec<u8>,
}

/// `QFile::readLine` in text mode over `data` from `*at`: up to and including a `\n`, a final
/// `\r\n` written `\n`; then `chop(1)`.
fn read_chopped_line(data: &[u8], at: &mut usize) -> Vec<u8> {
    let rest = &data[*at..];
    let end = rest.iter().position(|&b| b == b'\n').map_or(rest.len(), |i| i + 1);
    let mut line = rest[..end].to_vec();
    *at += end;
    if line.ends_with(b"\r\n") {
        line.truncate(line.len() - 2);
        line.push(b'\n');
    }
    line.pop();
    line
}

/// `QByteArray::toLongLong(&ok)`: base 10, surrounding whitespace allowed.
fn to_long_long(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(super::qfile::trimmed(bytes)).ok()?;
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `QLockFilePrivate::getLockInfo_helper`.
fn lock_info(file_name: &str) -> Option<LockFileInfo> {
    let fd = open_raw(file_name.as_bytes(), libc::O_RDONLY, 0).ok()?;
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        // SAFETY: `chunk` is valid for its length; `fd` is open.
        let r = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if r < 0 && errno() == libc::EINTR {
            continue;
        }
        if r <= 0 {
            break;
        }
        data.extend_from_slice(&chunk[..r as usize]);
    }
    // SAFETY: `fd` is open and closed once.
    unsafe { libc::close(fd) };
    let mut at = 0;
    let pid_line = read_chopped_line(&data, &mut at);
    if pid_line.is_empty() {
        return None;
    }
    let appname = qdir::decode_utf8_lossy(&read_chopped_line(&data, &mut at));
    let hostname = qdir::decode_utf8_lossy(&read_chopped_line(&data, &mut at));
    let hostid = read_chopped_line(&data, &mut at);
    let bootid = read_chopped_line(&data, &mut at);
    let pid = to_long_long(&pid_line)?;
    (pid > 0).then_some(LockFileInfo { pid, appname, hostname, hostid, bootid })
}

/// `QLockFilePrivate::isProcessRunning`.
fn is_process_running(pid: i64, appname: &str) -> bool {
    // SAFETY: kill with signal 0 only checks the pid.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == -1 && errno() == libc::ESRCH {
        return false;
    }
    let process_name = process_name_by_pid(pid);
    if !process_name.is_empty() {
        let mut info = FileInfo::new(appname);
        if info.is_symlink() {
            info = FileInfo::new(info.sym_link_target());
        }
        if process_name != info.file_name() {
            return false;
        }
    }
    true
}

/// `QDateTime::currentDateTimeUtc()` in milliseconds since the epoch.
fn now_msecs() -> i64 {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes a timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
    now.tv_sec * 1000 + now.tv_nsec / 1_000_000
}

/// `flock(fd, LOCK_EX | LOCK_NB)`.
fn set_native_locks(fd: libc::c_int) -> bool {
    // SAFETY: flock on an open descriptor.
    unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) != -1 }
}

fn unlink(path: &str) -> bool {
    let Ok(c) = CString::new(path) else { return false };
    // SAFETY: a valid NUL-terminated string.
    unsafe { libc::unlink(c.as_ptr()) == 0 }
}

impl LockFile {
    /// `QLockFile(fileName)`: unlocked, with the default stale lock time of 30 seconds.
    pub fn new(file_name: impl Into<String>) -> LockFile {
        LockFile {
            file_name: file_name.into(),
            fd: -1,
            stale_lock_time_ms: 30_000,
            lock_error: LockError::NoError,
            is_locked: false,
        }
    }

    /// `fileName()`.
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// `setStaleLockTime(ms)`: 0 means a lock is never stale because of its age.
    pub fn set_stale_lock_time(&mut self, milliseconds: i64) {
        self.stale_lock_time_ms = milliseconds;
    }

    /// `staleLockTime()`.
    pub fn stale_lock_time(&self) -> i64 {
        self.stale_lock_time_ms
    }

    /// `isLocked()`: whether this `LockFile` holds the lock.
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }

    /// `error()`: what the last `try_lock` found.
    pub fn error(&self) -> LockError {
        self.lock_error
    }

    /// `lock()`: waits as long as it takes; false only for errors other than a held lock.
    pub fn lock(&mut self) -> bool {
        self.try_lock(-1)
    }

    /// `tryLock(timeout)` in milliseconds (negative: for ever).
    pub fn try_lock(&mut self, timeout_ms: i64) -> bool {
        let deadline = (timeout_ms >= 0).then(|| Instant::now() + Duration::from_millis(timeout_ms as u64));
        let mut sleep_time = Duration::from_millis(100);
        loop {
            self.lock_error = self.try_lock_sys();
            match self.lock_error {
                LockError::NoError => {
                    self.is_locked = true;
                    return true;
                }
                LockError::PermissionError | LockError::UnknownError => return false,
                LockError::LockFailedError => {
                    if !self.is_locked && self.is_apparently_stale() {
                        // Stale lock from another thread or process; make sure two processes
                        // don't remove it at the same time.
                        let mut rmlock = LockFile::new(format!("{}.rmlock", self.file_name));
                        if rmlock.try_lock(0) && self.is_apparently_stale() && self.remove_stale_lock() {
                            continue;
                        }
                    }
                }
            }
            let remaining = match deadline {
                None => Duration::MAX,
                Some(deadline) => deadline.saturating_duration_since(Instant::now()),
            };
            // QDeadlineTimer counts whole milliseconds.
            let remaining = Duration::from_millis(remaining.as_millis().min(u128::from(u64::MAX)) as u64);
            if remaining.is_zero() {
                return false;
            }
            if sleep_time > remaining {
                sleep_time = remaining;
            }
            std::thread::sleep(sleep_time);
            if sleep_time < Duration::from_secs(5) {
                sleep_time *= 2;
            }
        }
    }

    /// `QLockFilePrivate::tryLock_sys`.
    fn try_lock_sys(&mut self) -> LockError {
        let fd = match open_raw(self.file_name.as_bytes(), libc::O_RDWR | libc::O_CREAT | libc::O_EXCL, 0o666) {
            Ok(fd) => fd,
            Err(libc::EEXIST) => return LockError::LockFailedError,
            Err(libc::EACCES | libc::EROFS) => return LockError::PermissionError,
            Err(_) => return LockError::UnknownError,
        };
        // Ensure nobody else can delete the file while it is held (Qt only warns on failure).
        set_native_locks(fd);
        let data = lock_file_contents();
        let mut written = 0;
        while written < data.len() {
            let rest = &data[written..];
            // SAFETY: `rest` is valid for its length; `fd` is open.
            let r = unsafe { libc::write(fd, rest.as_ptr().cast(), rest.len()) };
            if r < 0 && errno() == libc::EINTR {
                continue;
            }
            if r <= 0 {
                break;
            }
            written += r as usize;
        }
        if written < data.len() {
            // SAFETY: `fd` is open and closed once.
            unsafe { libc::close(fd) };
            unlink(&self.file_name);
            return LockError::UnknownError;
        }
        self.fd = fd;
        // SAFETY: `fd` is open; errors (not supported) are ignored as in Qt.
        unsafe { libc::fdatasync(fd) };
        LockError::NoError
    }

    /// `QLockFilePrivate::isApparentlyStale`.
    fn is_apparently_stale(&self) -> bool {
        if let Some(info) = lock_info(&self.file_name) {
            let mut same_host = info.hostname.is_empty() || info.hostname == machine_host_name();
            if !info.hostid.is_empty() {
                // The host id, when both have one, decides.
                let our_host_id = machine_unique_id();
                if !our_host_id.is_empty() {
                    same_host = our_host_id == info.hostid;
                }
            }
            if same_host {
                if !info.bootid.is_empty() && info.bootid != boot_unique_id() {
                    // Rebooted since, so the lock is definitely stale.
                    return true;
                }
                if !is_process_running(info.pid, &info.appname) {
                    return true;
                }
            }
        }
        // QFileInfo::lastModified: an invalid time (no file) is 0 ms away.
        let age = match std::fs::metadata(&self.file_name) {
            Ok(meta) => now_msecs() - (meta.mtime() * 1000 + meta.mtime_nsec() / 1_000_000),
            Err(_) => 0,
        };
        self.stale_lock_time_ms > 0 && age.abs() > self.stale_lock_time_ms
    }

    /// `QLockFilePrivate::removeStaleLock`: true if the file was removed, which fails while its
    /// owner holds its `flock`.
    fn remove_stale_lock(&self) -> bool {
        let Ok(fd) = open_raw(self.file_name.as_bytes(), libc::O_WRONLY, 0o666) else { return false };
        let success = set_native_locks(fd) && unlink(&self.file_name);
        // SAFETY: `fd` is open and closed once.
        unsafe { libc::close(fd) };
        success
    }

    /// `removeStaleLockFile()`: removes a lock file nobody holds (false while this one holds it).
    pub fn remove_stale_lock_file(&self) -> bool {
        if self.is_locked {
            return false;
        }
        self.remove_stale_lock()
    }

    /// `unlock()`: removes the lock file and lets go of it; nothing when not locked.
    pub fn unlock(&mut self) {
        if !self.is_locked {
            return;
        }
        unlink(&self.file_name);
        // SAFETY: `fd` is the open lock descriptor, closed once.
        unsafe { libc::close(self.fd) };
        self.fd = -1;
        self.lock_error = LockError::NoError;
        self.is_locked = false;
    }
}

impl Drop for LockFile {
    /// `~QLockFile`: unlocks.
    fn drop(&mut self) {
        self.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("omareel-qlockfile-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_owned()
    }

    fn set_mtime(path: &str, seconds: i64) {
        let c = CString::new(path).unwrap();
        let time = libc::timespec { tv_sec: seconds, tv_nsec: 0 };
        let times = [time, time];
        // SAFETY: `c` is NUL-terminated and `times` holds two timespecs.
        assert_eq!(unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), 0) }, 0);
    }

    #[test]
    fn writes_qts_five_lines_and_removes_them_on_unlock() {
        let dir = scratch("format");
        let path = format!("{dir}/a.lock");
        let mut lock = LockFile::new(path.as_str());
        assert!(lock.try_lock(0) && lock.is_locked() && lock.error() == LockError::NoError);
        let contents = std::fs::read(&path).unwrap();
        assert_eq!(contents, lock_file_contents());
        let lines: Vec<&[u8]> = contents.split(|&b| b == b'\n').collect();
        assert_eq!(lines.len(), 6, "five lines, each ending in a newline");
        assert_eq!(lines[0], std::process::id().to_string().as_bytes());
        assert_eq!(lines[1], process_name_by_pid(i64::from(std::process::id())).as_bytes());
        assert!(!lines[1].is_empty() && !lines[1].contains(&b'/'));
        assert_eq!(lines[2], machine_host_name().as_bytes());
        assert_eq!(lines[4].len(), 36);
        assert!(lines[5].is_empty());
        // flock is held: removing it as stale fails even though the file exists.
        assert!(!LockFile::new(path.as_str()).remove_stale_lock_file());
        drop(lock);
        assert!(!std::path::Path::new(&path).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_held_lock_waits_out_the_timeout() {
        let dir = scratch("held");
        let path = format!("{dir}/a.lock");
        let mut held = LockFile::new(path.as_str());
        assert!(held.try_lock(0));
        let mut other = LockFile::new(path.as_str());
        other.set_stale_lock_time(10_000);
        let started = Instant::now();
        assert!(!other.try_lock(350));
        let waited = started.elapsed();
        assert!(waited >= Duration::from_millis(340) && waited < Duration::from_millis(1500), "{waited:?}");
        assert_eq!(other.error(), LockError::LockFailedError);
        assert!(!other.try_lock(0));
        held.unlock();
        assert!(other.try_lock(0));
        assert!(!held.try_lock(0), "the other one holds it now");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stale_locks_are_removed_and_garbage_only_once_old() {
        let dir = scratch("stale");
        let path = format!("{dir}/a.lock");
        // A pid that is not running, without host ids: stale whatever its age.
        std::fs::write(&path, "999999999\nomareel\n\n").unwrap();
        let mut lock = LockFile::new(path.as_str());
        lock.set_stale_lock_time(10_000);
        assert!(lock.try_lock(0));
        assert!(!std::path::Path::new(&format!("{path}.rmlock")).exists());
        lock.unlock();
        // A running pid whose process name differs is stale too (the pid was reused).
        std::fs::write(&path, format!("{}\nnot-this-program\n\n", std::process::id())).unwrap();
        assert!(lock.try_lock(0));
        lock.unlock();
        // Another boot of this machine: stale.
        let pid = i64::from(std::process::id());
        let mut contents = format!("{pid}\n{}\n{}\n", process_name_by_pid(pid), machine_host_name()).into_bytes();
        contents.extend_from_slice(&machine_unique_id());
        contents.extend_from_slice(b"\n00000000-0000-0000-0000-000000000000\n");
        std::fs::write(&path, &contents).unwrap();
        assert!(lock.try_lock(0));
        lock.unlock();
        // Garbage is only stale once older than the stale lock time.
        std::fs::write(&path, "garbage").unwrap();
        assert!(!lock.try_lock(0));
        assert_eq!(lock.error(), LockError::LockFailedError);
        set_mtime(&path, 1_000_000_000);
        assert!(lock.try_lock(0));
        lock.unlock();
        // With a stale lock time of 0 an old lock of a live process stays.
        std::fs::write(&path, lock_file_contents()).unwrap();
        set_mtime(&path, 1_000_000_000);
        lock.set_stale_lock_time(0);
        assert!(!lock.try_lock(0));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn directories_and_permissions() {
        let dir = scratch("errors");
        // A directory at the lock's path is a lock held by someone (and never stale while new).
        std::fs::create_dir(format!("{dir}/a.lock")).unwrap();
        let mut lock = LockFile::new(format!("{dir}/a.lock"));
        assert!(!lock.try_lock(0));
        assert_eq!(lock.error(), LockError::LockFailedError);
        // A missing directory is an unknown error, returned at once.
        let mut lock = LockFile::new(format!("{dir}/missing/a.lock"));
        let started = Instant::now();
        assert!(!lock.try_lock(5000));
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(lock.error(), LockError::UnknownError);
        // SAFETY: geteuid has no failure mode.
        if unsafe { libc::geteuid() } != 0 {
            let read_only = format!("{dir}/read-only");
            std::fs::create_dir(&read_only).unwrap();
            assert!(super::super::qfile::set_permissions(&read_only, 0o500));
            let mut lock = LockFile::new(format!("{read_only}/a.lock"));
            assert!(!lock.try_lock(5000));
            assert_eq!(lock.error(), LockError::PermissionError);
            assert!(super::super::qfile::set_permissions(&read_only, 0o700));
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lines_read_as_qt_reads_them() {
        let mut at = 0;
        let data = b"12\r\napp\nhost";
        assert_eq!(read_chopped_line(data, &mut at), b"12");
        assert_eq!(read_chopped_line(data, &mut at), b"app");
        assert_eq!(read_chopped_line(data, &mut at), b"hos", "the last byte goes even without a newline");
        assert_eq!(read_chopped_line(data, &mut at), b"");
        assert_eq!((to_long_long(b" 42 "), to_long_long(b"4x"), to_long_long(b"")), (Some(42), None, None));
    }
}
