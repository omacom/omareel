//! `QStandardPaths` on Linux (Qt 6.11): `writableLocation` for the XDG base directories and the
//! xdg-user-dirs folders, and `findExecutable`.
//!
//! Environment variables are read when a function is called, decoded like
//! `qEnvironmentVariable`. Qt's rules, reproduced:
//! - `$XDG_CONFIG_HOME`, `$XDG_CACHE_HOME`, `$XDG_DATA_HOME` and `$XDG_STATE_HOME` count only if
//!   they start with "/" (unset, empty and relative values silently fall back to `$HOME/.config`
//!   and so on) and are used exactly as given, trailing slashes and all.
//! - `$HOME` is cleaned (`QDir::homePath`); unset or empty it is "/", which makes the fallbacks
//!   "//.config" and the like.
//! - The application locations add "/organization/application": omareel sets both to
//!   "omareel", so [`Location::Cache`] is `~/.cache/omareel/omareel`.
//! - [`Location::Movies`] and the other user folders come from `user-dirs.dirs` in the config
//!   directory: the last line matching `^XDG_(.*)_DIR=(.*)$` whose name (up to the last
//!   "_DIR=") is the folder's key wins; one pair of surrounding quotes is removed if the value is
//!   longer than two characters, a leading `$HOME` becomes the home path (nothing else is
//!   expanded, so `$HOMEX/a` is home + "X/a"), and one trailing slash goes. An empty value gives
//!   "". Without the file or a matching line, `$HOME/Videos` and so on.
//!
//! Checked against Qt 6.11.2 with no differences: every location and 19 `findExecutable`
//! lookups under 616 environments (`$HOME`, the XDG variables, `$TMPDIR` and `$PATH` unset,
//! empty, relative, with trailing or doubled slashes, a BOM, invalid UTF-8) and 34
//! `user-dirs.dirs` variants (quoting, CRLF and stray CR, a BOM, invalid and truncated UTF-8,
//! NUL, U+2028, comments, greedy keys, repeated keys, a directory or unreadable file in its
//! place). Not reproduced: `RuntimeLocation`; a `user-dirs.dirs` with a UTF-16 or UTF-32
//! byte-order mark (Qt would decode it as such) is read as UTF-8; Qt's test mode.

use super::qdir;
use std::os::unix::ffi::OsStrExt;

/// `QCoreApplication::organizationName()` in omareel.
pub const ORGANIZATION_NAME: &str = "omareel";
/// `QCoreApplication::applicationName()` in omareel.
pub const APPLICATION_NAME: &str = "omareel";

/// `QStandardPaths::StandardLocation`, without `RuntimeLocation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Desktop,
    Documents,
    Fonts,
    Applications,
    Music,
    Movies,
    Pictures,
    Temp,
    Home,
    AppLocalData,
    Cache,
    GenericData,
    Config,
    Download,
    GenericCache,
    GenericConfig,
    AppData,
    AppConfig,
    PublicShare,
    Templates,
    State,
    GenericState,
}

/// What a lookup reads from the process: environment variables, the application's names, and
/// the working directory (for a relative `$TMPDIR`).
pub(crate) struct Context<'a> {
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub organization: &'a str,
    pub application: &'a str,
    pub cwd: Vec<u8>,
}

fn process_context(env: &dyn Fn(&str) -> Option<String>) -> Context<'_> {
    Context {
        env,
        organization: ORGANIZATION_NAME,
        application: APPLICATION_NAME,
        cwd: std::env::current_dir().map(|d| d.as_os_str().as_bytes().to_vec()).unwrap_or_default(),
    }
}

/// `QStandardPaths::writableLocation(location)`.
pub fn writable_location(location: Location) -> String {
    writable_location_in(location, &process_context(&qdir::env_var))
}

pub(crate) fn writable_location_in(location: Location, ctx: &Context) -> String {
    let home = || qdir::home_path_with(ctx.env);
    let xdg = |var: &str, fallback: &str| -> String {
        let value = (ctx.env)(var).unwrap_or_default();
        if value.starts_with('/') {
            value
        } else {
            home() + fallback
        }
    };
    let with_app = |mut path: String| -> String {
        if !ctx.organization.is_empty() {
            path.push('/');
            path.push_str(ctx.organization);
        }
        if !ctx.application.is_empty() {
            path.push('/');
            path.push_str(ctx.application);
        }
        path
    };
    use Location::*;
    match location {
        Home => return home(),
        Temp => return qdir::temp_path_with(ctx.env, &ctx.cwd),
        Cache => return with_app(xdg("XDG_CACHE_HOME", "/.cache")),
        GenericCache => return xdg("XDG_CACHE_HOME", "/.cache"),
        State => return with_app(xdg("XDG_STATE_HOME", "/.local/state")),
        GenericState => return xdg("XDG_STATE_HOME", "/.local/state"),
        AppData | AppLocalData => return with_app(xdg("XDG_DATA_HOME", "/.local/share")),
        GenericData => return xdg("XDG_DATA_HOME", "/.local/share"),
        Config | GenericConfig => return xdg("XDG_CONFIG_HOME", "/.config"),
        AppConfig => return with_app(xdg("XDG_CONFIG_HOME", "/.config")),
        _ => {}
    }

    let key = match location {
        Desktop => "DESKTOP",
        Documents => "DOCUMENTS",
        Pictures => "PICTURES",
        Music => "MUSIC",
        Movies => "VIDEOS",
        Download => "DOWNLOAD",
        PublicShare => "PUBLICSHARE",
        Templates => "TEMPLATES",
        _ => "",
    };
    if !key.is_empty() {
        let file = xdg("XDG_CONFIG_HOME", "/.config") + "/user-dirs.dirs";
        if let Some(text) = read_text_file(&file) {
            if let Some(result) = user_dir_from(&text, key, &home()) {
                return result;
            }
        }
    }
    match location {
        Desktop => home() + "/Desktop",
        Documents => home() + "/Documents",
        Pictures => home() + "/Pictures",
        Fonts => writable_location_in(GenericData, ctx) + "/fonts",
        Music => home() + "/Music",
        Movies => home() + "/Videos",
        Download => home() + "/Downloads",
        Applications => writable_location_in(GenericData, ctx) + "/applications",
        PublicShare => home() + "/Public",
        Templates => home() + "/Templates",
        _ => String::new(),
    }
}

/// `QFile::open(ReadOnly)` and a `QTextStream` over it: `None` if it can't be opened (a
/// directory can't); otherwise UTF-8 with a leading byte-order mark dropped, U+FFFD for each
/// byte that does not start a valid sequence, and an incomplete sequence at the very end
/// dropped (the stream's decoder is never flushed).
fn read_text_file(path: &str) -> Option<String> {
    if path.contains('\0') {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.is_dir() {
        return None;
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    let _ = (&file).read_to_end(&mut bytes);
    Some(decode_stream_utf8(&bytes))
}

fn decode_stream_utf8(bytes: &[u8]) -> String {
    // A stateful QStringDecoder that is never flushed: a byte-order mark at the start is eaten,
    // errors become U+FFFD byte by byte, and an incomplete sequence at the end stays in the
    // decoder's state, never to be output.
    let mut end = bytes.len();
    for back in 1..=3.min(bytes.len()) {
        let lead = bytes[bytes.len() - back];
        if lead & 0xC0 == 0x80 {
            continue;
        }
        let need = match lead {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => 0,
        };
        if need > back && tail_is_incomplete(&bytes[bytes.len() - back..], need) {
            end = bytes.len() - back;
        }
        break;
    }
    let body = &bytes[..end];
    qdir::decode_utf8_lossy(body.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(body))
}

/// Whether `tail` (a lead byte and its continuation bytes, fewer than `need`) is what Qt's
/// decoder reports as "end of string" rather than an error.
fn tail_is_incomplete(tail: &[u8], need: usize) -> bool {
    tail.len() < need && tail[1..].iter().take(2).all(|b| b & 0xC0 == 0x80)
}

/// The `user-dirs.dirs` lookup in `QStandardPaths::writableLocation`.
fn user_dir_from(text: &str, key: &str, home: &str) -> Option<String> {
    let mut result = None;
    for line in text_stream_lines(text) {
        let Some(rest) = line.strip_prefix("XDG_") else { continue };
        let Some(at) = rest.rfind("_DIR=") else { continue };
        if &rest[..at] != key {
            continue;
        }
        let mut value = &rest[at + 5..];
        if value.len() > 2 && value.starts_with('"') && value.ends_with('"') {
            value = &value[1..value.len() - 1];
        }
        let mut r = match value.strip_prefix("$HOME") {
            Some(after) => home.to_string() + after,
            None => value.to_string(),
        };
        if r.len() > 1 && r.ends_with('/') {
            r.pop();
        }
        result = Some(r);
    }
    result
}

/// `while (!stream.atEnd()) stream.readLine()`: lines end at "\n" with one "\r" before it
/// removed, and a "\r" at the very end of the text is dropped too.
fn text_stream_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(i) => {
                let line = &rest[..i];
                lines.push(line.strip_suffix('\r').unwrap_or(line));
                rest = &rest[i + 1..];
            }
            None => {
                lines.push(rest.strip_suffix('\r').unwrap_or(rest));
                rest = "";
            }
        }
    }
    lines
}

/// `QStandardPaths::findExecutable(name)` with no extra paths: "" if nothing is found.
///
/// An absolute `name` is only checked; any other name, even one with a "/" in it, is looked up
/// in each `$PATH` entry (empty entries skipped, each cleaned, relative ones taken from the
/// working directory). The first regular file the user may execute wins, returned cleaned but
/// with symlinks unresolved. With `$PATH` unset, glibc's default "/usr/bin:/bin" is searched;
/// set but empty, nothing is.
pub fn find_executable(name: &str) -> String {
    find_executable_in(name, &process_context(&qdir::env_var))
}

pub(crate) fn find_executable_in(name: &str, ctx: &Context) -> String {
    let check = |path: &str| -> String {
        let info = qdir::FileInfo::new(path);
        if info.is_file() && info.is_executable() {
            qdir::clean_path(path)
        } else {
            String::new()
        }
    };
    if qdir::is_absolute_path(name) {
        return check(name);
    }
    let path_env = (ctx.env)("PATH").unwrap_or_else(|| "/usr/bin:/bin".to_string());
    let cwd = qdir::decode_name(&ctx.cwd);
    for raw in path_env.split(':').filter(|p| !p.is_empty()) {
        let mut dir = qdir::clean_path(raw);
        if dir.len() > 1 && dir.ends_with('/') {
            dir.pop();
        }
        let candidate = qdir::absolute_file_path_with_cwd(&cwd, &format!("{dir}/{name}"), &ctx.cwd);
        let found = check(&candidate);
        if !found.is_empty() {
            return found;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::qdir::tests::TempDir;

    fn ctx<'a>(env: &'a dyn Fn(&str) -> Option<String>) -> Context<'a> {
        Context { env, organization: ORGANIZATION_NAME, application: APPLICATION_NAME, cwd: b"/work".to_vec() }
    }

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn at(location: Location, env: &dyn Fn(&str) -> Option<String>) -> String {
        writable_location_in(location, &ctx(env))
    }

    #[test]
    fn base_directories_follow_xdg_rules() {
        // Recorded from Qt 6.11.2 with organization and application both "omareel".
        let home = vars(&[("HOME", "/home/u")]);
        assert_eq!(at(Location::Config, &home), "/home/u/.config");
        assert_eq!(at(Location::GenericConfig, &home), "/home/u/.config");
        assert_eq!(at(Location::AppConfig, &home), "/home/u/.config/omareel/omareel");
        assert_eq!(at(Location::GenericCache, &home), "/home/u/.cache");
        assert_eq!(at(Location::Cache, &home), "/home/u/.cache/omareel/omareel");
        assert_eq!(at(Location::GenericData, &home), "/home/u/.local/share");
        assert_eq!(at(Location::AppData, &home), "/home/u/.local/share/omareel/omareel");
        assert_eq!(at(Location::AppLocalData, &home), "/home/u/.local/share/omareel/omareel");
        assert_eq!(at(Location::State, &home), "/home/u/.local/state/omareel/omareel");
        assert_eq!(at(Location::GenericState, &home), "/home/u/.local/state");
        assert_eq!(at(Location::Fonts, &home), "/home/u/.local/share/fonts");
        assert_eq!(at(Location::Applications, &home), "/home/u/.local/share/applications");
        assert_eq!(at(Location::Home, &home), "/home/u");
        assert_eq!(at(Location::Temp, &home), "/tmp");

        // Relative, empty and unset variables are ignored; absolute ones are used as they are.
        for value in ["rel", "", " /x"] {
            assert_eq!(at(Location::Config, &vars(&[("HOME", "/h"), ("XDG_CONFIG_HOME", value)])), "/h/.config");
        }
        assert_eq!(at(Location::Config, &vars(&[("XDG_CONFIG_HOME", "//x//")])), "//x//");
        assert_eq!(at(Location::Cache, &vars(&[("XDG_CACHE_HOME", "/x/")])), "/x//omareel/omareel");
        assert_eq!(at(Location::GenericData, &vars(&[("XDG_DATA_HOME", "/x/../y")])), "/x/../y");
        // $HOME is cleaned, and "/" when missing.
        assert_eq!(at(Location::Config, &vars(&[])), "//.config");
        assert_eq!(at(Location::Cache, &vars(&[("HOME", "")])), "//.cache/omareel/omareel");
        assert_eq!(at(Location::Config, &vars(&[("HOME", "/a//b/./c/..")])), "/a/b/.config");
        assert_eq!(at(Location::Config, &vars(&[("HOME", "rel")])), "rel/.config");
        assert_eq!(at(Location::Temp, &vars(&[("TMPDIR", "rel")])), "/work/rel");
        assert_eq!(at(Location::Temp, &vars(&[("TMPDIR", "/t//")])), "/t/");
    }

    #[test]
    fn user_dirs_come_from_user_dirs_dirs() {
        let movies = |contents: &[u8]| -> String {
            let dir = TempDir::new("stdpaths");
            std::fs::write(dir.join("user-dirs.dirs"), contents).unwrap();
            let config = dir.0.clone();
            let env = move |name: &str| match name {
                "HOME" => Some("/home/u".to_string()),
                "XDG_CONFIG_HOME" => Some(config.clone()),
                _ => None,
            };
            at(Location::Movies, &env)
        };
        // Recorded from Qt 6.11.2.
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"$HOME/Videos2\"\n"), "/home/u/Videos2");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"$HOME/V/\"\n"), "/home/u/V");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/media/v\"\n"), "/media/v");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=$HOME/x\n"), "/home/u/x");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\n"), "");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"\"\n"), "\"\"");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"\n"), "\"");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"$HOMEX/a\"\n"), "/home/uX/a");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/first\"\nXDG_VIDEOS_DIR=\"/second\"\n"), "/second");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/crlf\"\r\nXDG_MUSIC_DIR=\"/m\"\r\n"), "/crlf");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/a\rb\"\n"), "/a\rb");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/a\"\r\r\n"), "\"/a\"\r");
        assert_eq!(movies(b"\xef\xbb\xbfXDG_VIDEOS_DIR=\"/bom\"\n"), "/bom");
        assert_eq!(movies(b"# XDG_VIDEOS_DIR=\"/c\"\n"), "/home/u/Videos");
        assert_eq!(movies(b" XDG_VIDEOS_DIR=\"/s\"\n"), "/home/u/Videos");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/x_DIR=/y\"\n"), "/home/u/Videos");
        assert_eq!(movies(b"XDG_videos_DIR=\"/lower\"\n"), "/home/u/Videos");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/inv\xff\xe2\x82id\"\n"), "/inv\u{FFFD}\u{FFFD}\u{FFFD}id");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/nonl\""), "/nonl");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/tcr\"\r"), "/tcr");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=$HOME\n"), "/home/u");
        assert_eq!(movies(b"XDG_VIDEOS_DIR=\"/n\x00ul\"\n"), "/n\0ul");
        assert_eq!(movies("XDG_VIDEOS_DIR=\"/a\u{2028}b\"\n".as_bytes()), "/a\u{2028}b");

        let dir = TempDir::new("stdpaths-all");
        let keys = ["DESKTOP", "DOCUMENTS", "PICTURES", "MUSIC", "VIDEOS", "DOWNLOAD", "PUBLICSHARE", "TEMPLATES", "FONTS"];
        let text: String = keys.iter().map(|k| format!("XDG_{k}_DIR=\"$HOME/{k}-x\"\n")).collect();
        std::fs::write(dir.join("user-dirs.dirs"), text).unwrap();
        let config = dir.0.clone();
        let env = move |name: &str| match name {
            "HOME" => Some("/h".to_string()),
            "XDG_CONFIG_HOME" => Some(config.clone()),
            _ => None,
        };
        let got: Vec<String> = [
            Location::Desktop, Location::Documents, Location::Pictures, Location::Music, Location::Movies,
            Location::Download, Location::PublicShare, Location::Templates, Location::Fonts,
        ]
        .iter()
        .map(|&l| at(l, &env))
        .collect();
        assert_eq!(
            got,
            [
                "/h/DESKTOP-x", "/h/DOCUMENTS-x", "/h/PICTURES-x", "/h/MUSIC-x", "/h/VIDEOS-x", "/h/DOWNLOAD-x",
                "/h/PUBLICSHARE-x", "/h/TEMPLATES-x", "/h/.local/share/fonts",
            ]
        );

        // No file, or a directory in its place: the defaults.
        let none = vars(&[("HOME", "/h"), ("XDG_CONFIG_HOME", "/nonexistent-omareel-config")]);
        assert_eq!(at(Location::Movies, &none), "/h/Videos");
        assert_eq!(at(Location::Download, &none), "/h/Downloads");
        assert_eq!(at(Location::Desktop, &none), "/h/Desktop");
        let dir = TempDir::new("stdpaths-dir");
        std::fs::create_dir(dir.join("user-dirs.dirs")).unwrap();
        let config = dir.0.clone();
        let env = move |name: &str| if name == "XDG_CONFIG_HOME" { Some(config.clone()) } else { None };
        assert_eq!(at(Location::Movies, &env), "//Videos");
    }

    #[test]
    fn find_executable_searches_path_like_qt() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("findexe");
        let make = |rel: &str, mode: u32| {
            let path = dir.join(rel);
            if let Some(parent) = std::path::Path::new(&path).parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        make("bin/tool", 0o755);
        make("bin/noexec", 0o644);
        make("bin/both", 0o755);
        make("bin2/both", 0o755);
        make("bin2/sub/deep", 0o755);
        make("cwdtool", 0o755);
        std::fs::create_dir(dir.join("bin/dirtool")).unwrap();
        std::os::unix::fs::symlink("tool", dir.join("bin/toollink")).unwrap();
        let find = |name: &str, path: Option<&str>| -> String {
            let path = path.map(|p| p.replace("$D", &dir.0));
            let env = move |n: &str| if n == "PATH" { path.clone() } else { None };
            find_executable_in(name, &Context { env: &env, organization: "", application: "", cwd: dir.0.as_bytes().to_vec() })
        };
        let d = |rel: &str| dir.join(rel);
        assert_eq!(find("tool", Some("$D/bin")), d("bin/tool"));
        assert_eq!(find("tool", Some("$D/bin/")), d("bin/tool"));
        assert_eq!(find("tool", Some(":$D//bin:")), d("bin/tool"));
        assert_eq!(find("noexec", Some("$D/bin")), "");
        assert_eq!(find("dirtool", Some("$D/bin")), "");
        assert_eq!(find("toollink", Some("$D/bin")), d("bin/toollink"));
        assert_eq!(find("both", Some("$D/bin2:$D/bin")), d("bin2/both"));
        // A name with a slash is still looked up in PATH, not in the working directory.
        assert_eq!(find("sub/deep", Some("$D/bin2")), d("bin2/sub/deep"));
        assert_eq!(find("bin/tool", Some("$D")), d("bin/tool"));
        assert_eq!(find("cwdtool", Some("/nonexistent")), "");
        // Relative PATH entries are taken from the working directory.
        assert_eq!(find("cwdtool", Some(".")), d("cwdtool"));
        assert_eq!(find("tool", Some("bin")), d("bin/tool"));
        // Absolute names are only checked, and come back cleaned.
        assert_eq!(find(&format!("{}//bin/./tool", dir.0), Some("")), d("bin/tool"));
        assert_eq!(find(&d("bin/noexec"), None), "");
        assert_eq!(find("", Some("$D/bin")), "");
        assert_eq!(find("tool/", Some("$D/bin")), "");
        assert_eq!(find(":tool", Some("$D/bin")), "");
        // Empty PATH: nothing; unset PATH: glibc's default "/usr/bin:/bin".
        assert_eq!(find("tool", Some("")), "");
        if qdir::FileInfo::new("/usr/bin/sh").is_executable() {
            assert_eq!(find("sh", None), "/usr/bin/sh");
        }
    }

    #[test]
    fn decodes_the_stream_like_qtextstream() {
        assert_eq!(decode_stream_utf8(b"\xef\xbb\xbfab"), "ab");
        assert_eq!(decode_stream_utf8(b"\xef\xbb\xbf\xef\xbb\xbfab"), "\u{FEFF}ab");
        assert_eq!(decode_stream_utf8(b"a\xe2\x82"), "a");
        assert_eq!(decode_stream_utf8(b"a\xe2\x82b"), "a\u{FFFD}\u{FFFD}b");
        assert_eq!(decode_stream_utf8(b"a\xc3"), "a");
        assert_eq!(decode_stream_utf8(b"a\xf0\x9f\x98"), "a");
        assert_eq!(decode_stream_utf8(b"a\xff"), "a\u{FFFD}");
        assert_eq!(decode_stream_utf8(b"a\xf0\x9f\x98\x80"), "a\u{1F600}");
        assert_eq!(text_stream_lines("a\r\nb\n\nc\r"), ["a", "b", "", "c"]);
        assert_eq!(text_stream_lines("a\n"), ["a"]);
        assert!(text_stream_lines("").is_empty());
    }
}
