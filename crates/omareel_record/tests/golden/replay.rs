//! The recording side's test helpers, as the oracle's `replay.cpp` and `recording.cpp` have them:
//! scenarios for the replay shims (README "Replayed programs"), a `PATH` holding only some shims,
//! scoped environment changes, and the file listings, modes and masked timestamps the families
//! print.

use crate::hermetic::Hermetic;
use crate::support::{GOLDEN, Text, quote_bytes, quote_list};
use omareel_core::compat::qjson::Value;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// What one call of a replayed program answers.
#[derive(Clone, Debug, Default)]
pub struct Answer {
    pub out: Vec<u8>,
    pub err: Vec<u8>,
    pub status: i32,
    pub read_stdin: bool,
    pub hang_seconds: i32,
    /// Copied to the path the call gives as its last argument first.
    pub creates: Option<Vec<u8>>,
}

impl Answer {
    pub fn stdout(out: impl AsRef<[u8]>) -> Answer {
        Answer { out: out.as_ref().to_vec(), ..Answer::default() }
    }

    pub fn status(status: i32) -> Answer {
        Answer { status, ..Answer::default() }
    }
}

/// The oracle's `Replay`: a directory of answers for the shims, and the log of calls.
pub struct Replay {
    directory: String,
    log: String,
    active: bool,
}

fn write(path: &str, data: impl AsRef<[u8]>) {
    crate::hermetic::write_file(path, data);
}

impl Replay {
    /// Creates (or empties) the directory the answers go to.
    pub fn new(directory: &str) -> Replay {
        let _ = std::fs::remove_dir_all(directory);
        std::fs::create_dir_all(directory).expect("replay directory");
        Replay { directory: directory.to_owned(), log: format!("{directory}.log"), active: false }
    }

    /// The shim's file name for a call: the program and every argument, each after a space, with
    /// `%` written `%25` and `/` written `%2F`.
    pub fn key<S: AsRef<str>>(program: &str, arguments: &[S]) -> String {
        let mut text = program.to_owned();
        for argument in arguments {
            text.push(' ');
            text.push_str(argument.as_ref());
        }
        text.replace('%', "%25").replace('/', "%2F")
    }

    fn write_answer(base: &str, answer: &Answer, hang: bool) {
        write(&format!("{base}.status"), format!("{}\n", answer.status));
        if !answer.out.is_empty() {
            write(&format!("{base}.stdout"), &answer.out);
        }
        if !answer.err.is_empty() {
            write(&format!("{base}.stderr"), &answer.err);
        }
        if answer.read_stdin {
            write(&format!("{base}.stdin"), b"");
        }
        if hang && answer.hang_seconds > 0 {
            write(&format!("{base}.hang"), format!("{}\n", answer.hang_seconds));
        }
        if let Some(creates) = &answer.creates {
            write(&format!("{base}.creates"), creates);
        }
    }

    /// `call` > 0 answers only the call-th call with these arguments (1-based); 0 answers the rest.
    pub fn answer<S: AsRef<str>>(&self, program: &str, arguments: &[S], answer: &Answer, call: i32) {
        let mut base = format!("{}/{}", self.directory, Replay::key(program, arguments));
        if call > 0 {
            base += &format!(".{call}");
        }
        assert!(!Path::new(&format!("{base}.status")).exists(), "two answers for {}", Replay::key(program, arguments));
        Replay::write_answer(&base, answer, true);
    }

    /// The answer for every call of `program` that has no answer of its own.
    pub fn answer_any(&self, program: &str, answer: &Answer) {
        Replay::write_answer(&format!("{}/{program}.any", self.directory), answer, false);
    }

    /// Answers from corpus JSON: `{"argv", "status", "stdout" | "stdoutFile", "stderr", "stdin",
    /// "hang", "creates", "call"}`, missing members empty, 0 or false.
    pub fn answer_json(&self, answers: &Value) {
        for value in answers.to_array() {
            let object = value.to_object();
            let mut argv: Vec<String> =
                object.value("argv").to_array().iter().map(|argument| argument.to_string_or("")).collect();
            assert!(!argv.is_empty(), "a replay answer without argv");
            let out = if object.contains("stdoutFile") {
                let file = object.value("stdoutFile").to_string_or("");
                std::fs::read(crate::support::corpus_path(&file))
                    .unwrap_or_else(|_| panic!("missing replay file {file}"))
            } else {
                object.value("stdout").to_string_or("").into_bytes()
            };
            let answer = Answer {
                out,
                err: object.value("stderr").to_string_or("").into_bytes(),
                status: object.value("status").to_int(0),
                read_stdin: object.value("stdin").to_bool(false),
                hang_seconds: object.value("hang").to_int(0),
                creates: object.contains("creates").then(|| object.value("creates").to_string_or("").into_bytes()),
            };
            let program = argv.remove(0);
            self.answer(&program, &argv, &answer, object.value("call").to_int(0));
        }
    }

    /// Points `OMAREEL_ORACLE_REPLAY_DIR` and `OMAREEL_ORACLE_REPLAY_LOG` at this scenario (the
    /// log starts empty) until `deactivate` or drop.
    pub fn activate(&mut self, h: &Hermetic) {
        let _ = std::fs::remove_file(&self.log);
        let log = Path::new(&self.log);
        let (parent, name) = (log.parent().expect("log directory"), log.file_name().expect("log name"));
        let prefix = format!("{}.", name.to_string_lossy());
        for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with(&prefix) && file.ends_with(".stdin") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        h.set_env("OMAREEL_ORACLE_REPLAY_DIR", &self.directory);
        h.set_env("OMAREEL_ORACLE_REPLAY_LOG", &self.log);
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        // SAFETY: a replay is only activated by a test holding a Hermetic, which holds the
        // environment lock for the whole test.
        unsafe {
            std::env::remove_var("OMAREEL_ORACLE_REPLAY_DIR");
            std::env::remove_var("OMAREEL_ORACLE_REPLAY_LOG");
        }
        self.active = false;
    }

    /// The calls logged since `activate`, each `[program, arguments...]`.
    pub fn calls(&self) -> Vec<Vec<String>> {
        let data = std::fs::read(&self.log).unwrap_or_default();
        data.split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| String::from_utf8_lossy(line).split('\t').map(str::to_owned).collect())
            .collect()
    }

    /// What call `index` (0-based) read on its standard input, if its answer asked it to.
    pub fn standard_input(&self, index: usize) -> Option<Vec<u8>> {
        std::fs::read(format!("{}.{index}.stdin", self.log)).ok()
    }

    /// One `call <strlist>` row per logged call (arguments as **path**), plus `  stdin <bytes>`
    /// after a call that read it.
    pub fn dump_calls(&self, h: &Hermetic, out: &mut Text) {
        for (index, call) in self.calls().iter().enumerate() {
            let sanitized: Vec<String> = call.iter().map(|argument| h.sanitize(argument)).collect();
            out.row(["call".to_owned(), quote_list(&sanitized)]);
            if let Some(input) = self.standard_input(index) {
                out.row(["  stdin".to_owned(), quote_bytes(&input)]);
            }
        }
    }
}

impl Drop for Replay {
    fn drop(&mut self) {
        if self.active {
            self.deactivate();
        }
    }
}

/// The oracle's `ScopedEnvironment`: sets and unsets variables, putting every one it touched
/// back when dropped.
pub struct ScopedEnvironment<'a> {
    h: &'a Hermetic,
    saved: Vec<(String, Option<OsString>)>,
}

impl<'a> ScopedEnvironment<'a> {
    pub fn new(h: &'a Hermetic) -> ScopedEnvironment<'a> {
        ScopedEnvironment { h, saved: Vec::new() }
    }

    fn remember(&mut self, name: &str) {
        if !self.saved.iter().any(|(saved, _)| saved == name) {
            self.saved.push((name.to_owned(), std::env::var_os(name)));
        }
    }

    pub fn set(&mut self, name: &str, value: impl AsRef<std::ffi::OsStr>) {
        self.remember(name);
        self.h.set_env(name, value);
    }

    pub fn unset(&mut self, name: &str) {
        self.remember(name);
        self.h.unset_env(name);
    }
}

impl Drop for ScopedEnvironment<'_> {
    fn drop(&mut self) {
        for (name, value) in self.saved.iter().rev() {
            match value {
                Some(value) => self.h.set_env(name, value),
                None => self.h.unset_env(name),
            }
        }
    }
}

/// The oracle's `shimDirectory`: `<scratch>/bin-<name>` holding links to the named shims and to
/// the tools the shims run on, for a `PATH` without the other programs.
pub fn shim_directory(scratch: &str, name: &str, programs: &[&str]) -> String {
    let shims = std::fs::canonicalize(Path::new(GOLDEN).join("shims")).expect("tests/golden/shims");
    let directory = format!("{scratch}/bin-{name}");
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("shim directory");
    for program in programs {
        std::os::unix::fs::symlink(shims.join(program), format!("{directory}/{program}"))
            .unwrap_or_else(|e| panic!("could not link the {program} shim: {e}"));
    }
    for tool in ["sh", "cat", "cksum", "cut", "dirname", "mkdir", "readlink", "sed", "sleep", "wc"] {
        let found = ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| format!("{dir}/{tool}"))
            .find(|path| Path::new(path).exists())
            .unwrap_or_else(|| panic!("{tool} is not on PATH"));
        std::os::unix::fs::symlink(&found, format!("{directory}/{tool}"))
            .unwrap_or_else(|e| panic!("could not link {found}: {e}"));
    }
    directory
}

/// The oracle's `octalMode`: the `lstat` mode as four octal digits, `-` when there is nothing.
pub fn octal_mode(path: &str) -> String {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => format!("{:04o}", meta.mode() & 0o7777),
        Err(_) => "-".into(),
    }
}

/// The oracle's `directoryListing`: the entries in byte order as `<name>:<type><mode>` (type
/// `d`, `f` or `l`) joined by spaces; `-` when it is not a directory, `(empty)` when empty.
pub fn directory_listing(path: &str) -> String {
    if !Path::new(path).is_dir() {
        return "-".into();
    }
    let mut names: Vec<Vec<u8>> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| std::os::unix::ffi::OsStrExt::as_bytes(entry.file_name().as_os_str()).to_vec())
        .collect();
    names.sort();
    if names.is_empty() {
        return "(empty)".into();
    }
    names
        .iter()
        .map(|name| {
            let name = String::from_utf8_lossy(name).into_owned();
            let full = format!("{path}/{name}");
            let kind = match std::fs::symlink_metadata(&full) {
                Ok(meta) if meta.file_type().is_symlink() => 'l',
                Ok(meta) if meta.is_dir() => 'd',
                Ok(_) => 'f',
                Err(_) => '?',
            };
            format!("{name}:{kind}{}", octal_mode(&full))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `CLOCK_REALTIME` in microseconds, as the oracle reads it around a write.
pub fn realtime_now_us() -> i64 {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes a timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut now) };
    now.tv_sec * 1_000_000 + now.tv_nsec / 1000
}

/// The oracle's `maskRealtime`: in every line after the first with two TAB-separated cells, a
/// second cell that is a number becomes `{realtime}` when it lies in `[before, after]` and
/// `{realtime out of range}` otherwise.
pub fn mask_realtime(bytes: &[u8], before_us: i64, after_us: i64) -> Vec<u8> {
    let mut lines: Vec<Vec<u8>> = bytes.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect();
    for line in lines.iter_mut().skip(1) {
        let cells: Vec<&[u8]> = line.split(|&b| b == b'\t').collect();
        if cells.len() != 2 {
            continue;
        }
        let Some(value) = to_long_long(cells[1]) else { continue };
        let mask: &[u8] =
            if value >= before_us && value <= after_us { b"{realtime}" } else { b"{realtime out of range}" };
        let mut masked = cells[0].to_vec();
        masked.push(b'\t');
        masked.extend_from_slice(mask);
        *line = masked;
    }
    lines.join(&b'\n')
}

/// `QByteArray::toLongLong(&ok)`: base 10, surrounding ASCII whitespace allowed.
pub fn to_long_long(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(omareel_core::compat::qfile::trimmed(bytes)).ok()?;
    if text.is_empty() || text.starts_with('+') && text[1..].starts_with(['+', '-']) {
        return None;
    }
    text.parse().ok()
}
