//! The oracle's hermetic environment, for the families that read the environment and write
//! under `$HOME` (project, omarchypaths, theme).
//!
//! `tools/oracle/generate-golden.sh` ran the oracle under `env -i` with only a fresh `HOME`,
//! `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`, `XDG_DATA_HOME`, `XDG_RUNTIME_DIR` (0700)
//! and `TMPDIR`, all siblings in one scratch directory, `TZ=UTC`, `LC_ALL=C`, `LANG=C`,
//! `QT_QPA_PLATFORM=offscreen`, `PATH=tests/golden/shims:/usr/bin:/bin`,
//! `OMAREEL_ORACLE_HERMETIC=1` and `umask 022`, its working directory `$HOME` and its scratch
//! files under `$HOME/oracle-scratch/<family>/`. [`Hermetic::new`] makes the same world in a
//! fresh directory under the system's temporary directory and puts the process in it; dropping
//! it restores the environment, working directory and umask and deletes the directory.
//!
//! The environment, the working directory and the umask belong to the whole process, so every
//! test that changes them holds [`ENVIRONMENT`] for as long as it runs (a [`Hermetic`] holds it
//! for its lifetime). The other golden tests read only absolute paths and never the
//! environment, so they run alongside unaffected.

use crate::support::{GOLDEN, corpus_root, sanitize_with};
use std::ffi::OsString;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Held by every test that touches process-wide state: the environment, the working
/// directory, the umask.
pub static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// Variables the oracle's sanitizer puts back, in the order it lists them.
const ROOT_VARIABLES: [(&str, &str); 7] = [
    ("HOME", "home"),
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_STATE_HOME", "state"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_RUNTIME_DIR", "runtime"),
    ("TMPDIR", "tmp"),
];

pub struct Hermetic {
    run: PathBuf,
    home: String,
    family: String,
    /// `(name, path)` for **path** sanitization, the longest path first.
    roots: Vec<(String, String)>,
    saved_environment: Vec<(OsString, OsString)>,
    saved_directory: PathBuf,
    saved_umask: libc::mode_t,
    // Last, so it is released after `drop` has put everything back.
    _lock: MutexGuard<'static, ()>,
}

impl Hermetic {
    /// The oracle's world for `family`, with `$HOME/oracle-scratch/<family>` created.
    pub fn new(family: &str) -> Hermetic {
        let lock = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let base = std::env::temp_dir().join(format!(
            "omareel-golden-{}-{}-{family}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("scratch directory");
        let run = std::fs::canonicalize(&base).expect("scratch directory").join("run");
        let dir = |name: &str| -> String {
            let path = run.join(name);
            std::fs::create_dir_all(&path).expect("hermetic directory");
            path.to_str().expect("UTF-8 temporary directory").to_owned()
        };
        let values: Vec<(&str, String)> =
            ROOT_VARIABLES.iter().map(|&(variable, name)| (variable, dir(name))).collect();
        std::fs::set_permissions(run.join("runtime"), std::fs::Permissions::from_mode(0o700)).expect("chmod runtime");
        let home = values[0].1.clone();

        let mut roots: Vec<(String, String)> =
            values.iter().map(|(variable, path)| (format!("${variable}"), path.clone())).collect();
        roots.push(("$CORPUS".into(), corpus_root().to_str().expect("UTF-8 corpus path").to_owned()));
        roots.sort_by_key(|(_, path)| std::cmp::Reverse(path.len()));

        let shims = std::fs::canonicalize(Path::new(GOLDEN).join("shims")).expect("tests/golden/shims");
        let saved_environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let saved_directory = std::env::current_dir().expect("working directory");
        // SAFETY: umask has no failure mode; the old mask is restored on drop.
        let saved_umask = unsafe { libc::umask(0o022) };
        let hermetic = Hermetic {
            run,
            home: home.clone(),
            family: family.to_owned(),
            roots,
            saved_environment,
            saved_directory,
            saved_umask,
            _lock: lock,
        };
        for (name, _) in &hermetic.saved_environment {
            hermetic.unset_env(name);
        }
        for (variable, value) in &values {
            hermetic.set_env(variable, value);
        }
        for (variable, value) in [("TZ", "UTC"), ("LC_ALL", "C"), ("LANG", "C"), ("QT_QPA_PLATFORM", "offscreen")] {
            hermetic.set_env(variable, value);
        }
        hermetic.set_env("PATH", format!("{}:/usr/bin:/bin", shims.to_str().expect("UTF-8 shims path")));
        hermetic.set_env("OMAREEL_ORACLE_HERMETIC", "1");
        std::env::set_current_dir(&home).expect("enter $HOME");
        std::fs::create_dir_all(hermetic.scratch()).expect("scratch directory");
        hermetic
    }

    /// `QDir(QDir::homePath()).filePath(relative)`.
    pub fn in_home(&self, relative: &str) -> String {
        format!("{}/{relative}", self.home)
    }

    /// The value a hermetic variable was given (`HOME`, `XDG_CACHE_HOME`, ...).
    pub fn root(&self, variable: &str) -> String {
        let name = format!("${variable}");
        self.roots.iter().find(|(n, _)| *n == name).map(|(_, path)| path.clone()).expect("a hermetic variable")
    }

    /// `$HOME/oracle-scratch/<family>`, the oracle's `context.scratch`.
    pub fn scratch(&self) -> String {
        self.in_home(&format!("oracle-scratch/{}", self.family))
    }

    /// **path**: the hermetic directories and the corpus written as `$HOME`, ..., `$CORPUS`.
    pub fn sanitize(&self, text: &str) -> String {
        sanitize_with(text, &self.roots)
    }

    /// `qputenv`.
    pub fn set_env(&self, name: impl AsRef<std::ffi::OsStr>, value: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: the caller holds ENVIRONMENT through `self`, and no other golden test reads
        // the environment.
        unsafe { std::env::set_var(name, value) }
    }

    /// `qunsetenv`.
    pub fn unset_env(&self, name: impl AsRef<std::ffi::OsStr>) {
        // SAFETY: as for `set_env`.
        unsafe { std::env::remove_var(name) }
    }
}

impl Drop for Hermetic {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.saved_directory);
        // SAFETY: as in `new`.
        unsafe { libc::umask(self.saved_umask) };
        for (name, _) in std::env::vars_os() {
            self.unset_env(name);
        }
        for (name, value) in &self.saved_environment {
            self.set_env(name, value);
        }
        if let Some(base) = self.run.parent() {
            make_removable(base);
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

/// Gives the owner full access to every directory under `path`, so tests that leave read-only
/// directories behind can still be cleaned up.
fn make_removable(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };
    if meta.is_dir() {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(meta.permissions().mode() | 0o700));
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                make_removable(&entry.path());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// File helpers, as the oracle's

/// The oracle's `writeFile`: creates the parent directories, then writes (mode 0666 less the
/// umask for a new file).
pub fn write_file(path: &str, data: impl AsRef<[u8]>) {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkpath {}: {e}", parent.display()));
    }
    std::fs::write(path, data).unwrap_or_else(|e| panic!("could not write {path}: {e}"));
}

/// The oracle's `readFile`: the bytes, or empty when the file can't be read.
pub fn read_file(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_default()
}

/// `QFile::setPermissions` with the mode's permission bits.
pub fn chmod(path: &str, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| panic!("chmod {path}: {e}"));
}

/// The oracle's `symlink`: creates the link's directory and replaces whatever is at `link`.
pub fn make_symlink(target: &str, link: &str) {
    if let Some(parent) = Path::new(link).parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("mkpath {}: {e}", parent.display()));
    }
    let _ = std::fs::remove_file(link);
    symlink(target, link).unwrap_or_else(|e| panic!("symlink {link} -> {target} failed: {e}"));
}

/// `utimensat(AT_FDCWD, path, {t, t}, AT_SYMLINK_NOFOLLOW)`.
pub fn set_times(path: &str, seconds: i64, nanoseconds: i64) {
    let c = std::ffi::CString::new(path).expect("path without NUL");
    let time = libc::timespec { tv_sec: seconds as libc::time_t, tv_nsec: nanoseconds as _ };
    let times = [time, time];
    // SAFETY: `c` is NUL-terminated and `times` holds the two timespecs utimensat reads.
    let result = unsafe { libc::utimensat(libc::AT_FDCWD, c.as_ptr(), times.as_ptr(), libc::AT_SYMLINK_NOFOLLOW) };
    assert_eq!(result, 0, "utimensat {path}: {}", std::io::Error::last_os_error());
}

/// The oracle's `removePath`: a file or link removed, a directory removed with its contents.
pub fn remove_path(path: &str) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

/// Whether the tests run as root, which can read and write files whatever their modes, so
/// the permission cases the oracle recorded as an ordinary user cannot be reproduced.
pub fn running_as_root() -> bool {
    // SAFETY: geteuid has no failure mode.
    unsafe { libc::geteuid() == 0 }
}

/// The process as the env-touching tests find it: environment, working directory, umask.
fn process_state() -> (Vec<(OsString, OsString)>, PathBuf, libc::mode_t) {
    let mut environment: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    environment.sort();
    // SAFETY: umask has no failure mode; the mask is put straight back.
    let umask = unsafe {
        let mask = libc::umask(0o022);
        libc::umask(mask);
        mask
    };
    (environment, std::env::current_dir().expect("working directory"), umask)
}

#[test]
fn hermetic_world_is_the_oracles_and_is_put_back() {
    let before = {
        let _lock = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
        process_state()
    };
    let scratch;
    {
        let h = Hermetic::new("hermetic");
        let (environment, directory, umask) = process_state();
        let names: Vec<&str> = environment.iter().map(|(name, _)| name.to_str().expect("UTF-8 name")).collect();
        assert_eq!(
            names,
            [
                "HOME",
                "LANG",
                "LC_ALL",
                "OMAREEL_ORACLE_HERMETIC",
                "PATH",
                "QT_QPA_PLATFORM",
                "TMPDIR",
                "TZ",
                "XDG_CACHE_HOME",
                "XDG_CONFIG_HOME",
                "XDG_DATA_HOME",
                "XDG_RUNTIME_DIR",
                "XDG_STATE_HOME",
            ]
        );
        assert_eq!(directory.to_str(), Some(h.in_home("").trim_end_matches('/')));
        assert_eq!(umask, 0o022);
        let home = h.root("HOME");
        let run = Path::new(&home).parent().expect("run directory").to_owned();
        for (variable, name) in ROOT_VARIABLES {
            assert_eq!(std::env::var(variable).ok(), run.join(name).to_str().map(str::to_owned), "{variable}");
        }
        assert_eq!(std::fs::metadata(run.join("runtime")).expect("runtime").permissions().mode() & 0o777, 0o700);
        assert!(Path::new(&h.scratch()).is_dir());
        // A path only counts when no letter, digit, ".", "_" or "-" follows it.
        assert_eq!(
            h.sanitize(&format!("{home}/a {}/b {home}", h.root("XDG_CACHE_HOME"))),
            "$HOME/a $XDG_CACHE_HOME/b $HOME"
        );
        assert_eq!(h.sanitize(&format!("{home}x {home}-y {home}.z")), format!("{home}x {home}-y {home}.z"));
        // A directory the test leaves unwritable is still cleaned up.
        std::fs::create_dir_all(h.in_home("locked/inner")).expect("mkpath");
        chmod(&h.in_home("locked"), 0o500);
        scratch = run.parent().expect("scratch directory").to_owned();
    }
    assert!(!scratch.exists(), "{} is left behind", scratch.display());
    let _lock = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
    assert!(process_state() == before, "the environment, working directory or umask was not put back");
}
