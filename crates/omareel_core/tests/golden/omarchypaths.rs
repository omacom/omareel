//! omarchypaths: `omarchy_paths` against `Omareel::OmarchyPaths` (README "omarchypaths"), in the
//! oracle's hermetic `$HOME` with the fake Omarchy tree of `buildTree()`.
//!
//! `themeBackgroundGroups()` scanned the machine's `/usr/share/omarchy/themes` too, and the
//! oracle dropped every path outside the hermetic directories (and every group left without
//! one). Here the system root is a directory that does not exist, so nothing should be
//! dropped; the dump filters as the oracle did and the test also checks that it filtered
//! nothing.
//!
//! Not compared: `generate-wallpaper-thumbnail.txt`. `generateWallpaperThumbnail` decodes and
//! scales the image with `QImage` and stays in C++.
//!
//! Nothing here involves float arithmetic, so it runs on every architecture.

use crate::hermetic::*;
use crate::support::*;
use omareel_core::compat::qdir;
use omareel_core::omarchy_paths::{self, BackgroundGroup};
use std::cell::Cell;

/// The oracle's `buildTree()`.
fn build_tree(h: &Hermetic) {
    let config = h.in_home(".config/omarchy/themes");
    let share = h.in_home(".local/share/omarchy/themes");
    let touch = |path: String| write_file(&path, "image");
    for name in [
        "b.png",
        "A.PNG",
        "c.JPEG",
        "d.webp",
        "e.Jpg",
        ".hidden.png",
        "notes.txt",
        "f.gif",
        "g.png.bak",
        "\u{fc}n\u{ef}.png",
        "space name.jpg",
    ] {
        touch(format!("{config}/alpha/backgrounds/{name}"));
    }
    std::fs::create_dir_all(format!("{config}/alpha/backgrounds/sub.png")).expect("mkpath sub.png");
    make_symlink("../../beta/backgrounds/x.jpg", &format!("{config}/alpha/backgrounds/link-to-beta.jpg"));
    make_symlink("nowhere.png", &format!("{config}/alpha/backgrounds/broken.png"));
    for name in ["x.jpg", "y.jpeg", "Z.png", "a b.png"] {
        touch(format!("{config}/beta/backgrounds/{name}"));
    }
    std::fs::create_dir_all(format!("{config}/empty-theme")).expect("mkpath empty-theme");
    touch(format!("{config}/backgrounds-file/backgrounds"));
    touch(format!("{config}/.hidden-theme/backgrounds/h.png"));
    make_symlink("beta", &format!("{config}/linked-beta"));
    touch(format!("{config}/my-multi-word-theme/backgrounds/m.jpg"));
    touch(format!("{config}/Upper/backgrounds/u.png"));
    touch(format!("{config}/plain-file"));
    touch(format!("{config}/-dash-first/backgrounds/d.png"));
    write_file(&format!("{share}/alpha/backgrounds/b.png"), "another image");
    touch(format!("{share}/alpha/backgrounds/s.png"));
    touch(format!("{share}/gamma/backgrounds/g.png"));
    touch(format!("{share}/\u{fc}n\u{ef}code-th\u{e8}me/backgrounds/u.webp"));
    make_symlink(&format!("{config}/alpha/backgrounds/b.png"), &format!("{share}/zeta/backgrounds/z.jpg"));
    touch(h.in_home("Pictures/outside.png"));

    let state = h.in_home(".local/state/omarchy/current");
    write_file(&format!("{state}/theme.name"), "beta\n");
    for name in ["current-b.png", "Current-A.jpg", ".hidden.png", "readme.txt"] {
        touch(format!("{state}/theme/backgrounds/{name}"));
    }
    make_symlink(&format!("{config}/beta/backgrounds/y.jpeg"), &format!("{state}/background"));

    let alternative = h.in_home("alt-state");
    write_file(&format!("{alternative}/theme.name"), "gamma");
    make_symlink(&format!("{share}/gamma/backgrounds/g.png"), &format!("{alternative}/background"));
}

/// [`omarchy_paths::theme_roots`] with the system root replaced by one that does not exist.
fn roots(h: &Hermetic) -> Vec<String> {
    let mut roots = omarchy_paths::theme_roots();
    assert_eq!(roots.len(), 3);
    assert_eq!(roots[2], "/usr/share/omarchy/themes");
    roots[2] = format!("{}/no-system-themes/usr/share/omarchy/themes", h.scratch());
    roots
}

/// The oracle's `dumpState`. `dropped` counts the paths the oracle's filter removes.
fn dump_state(out: &mut Text, h: &Hermetic, dropped: &Cell<usize>, label: &str) {
    let hermetic = |path: &str| {
        let inside = h.sanitize(path).starts_with('$');
        if !inside {
            dropped.set(dropped.get() + 1);
        }
        inside
    };
    out.section(label);
    out.row(["stateRoot".to_owned(), quote(&h.sanitize(&omarchy_paths::state_root()))]);
    out.row(["currentThemeName".to_owned(), quote(&omarchy_paths::current_theme_name())]);
    out.row(["currentBackground".to_owned(), quote(&h.sanitize(&omarchy_paths::current_background()))]);
    let roots = roots(h);
    for BackgroundGroup { name, label, current, paths } in omarchy_paths::theme_background_groups_in(&roots) {
        let paths: Vec<String> = paths.iter().filter(|p| hermetic(p)).map(|p| quote(&h.sanitize(p))).collect();
        if paths.is_empty() {
            continue;
        }
        // The C++ group was a QVariantMap of these four keys, `current` a bool.
        out.row([
            "group".to_owned(),
            quote(&name),
            quote(&label),
            boolean(current).to_owned(),
            "bool".to_owned(),
            quote_list(&["current", "label", "name", "paths"]),
        ]);
        for path in paths {
            out.row(["  path".to_owned(), path]);
        }
    }
    for path in omarchy_paths::theme_backgrounds_in(&roots) {
        if hermetic(&path) {
            out.row(["themeBackgrounds".to_owned(), quote(&h.sanitize(&path))]);
        }
    }
}

#[test]
fn state_txt() {
    let h = Hermetic::new("omarchypaths");
    h.unset_env("OMAREEL_OMARCHY_STATE_DIR");
    h.unset_env("OMAREEL_RECENTS_DIR");
    build_tree(&h);
    let state = h.in_home(".local/state/omarchy/current");
    let config = h.in_home(".config/omarchy/themes");
    let share = h.in_home(".local/share/omarchy/themes");
    let at = |name: &str| format!("{state}/{name}");
    let dropped = Cell::new(0);
    let mut out = Text::new();
    let mut dump = |label: &str| dump_state(&mut out, &h, &dropped, label);

    dump("default state: background links into a theme folder");
    make_symlink(&h.in_home("Pictures/outside.png"), &at("background"));
    dump("background links outside the theme folders");
    std::fs::remove_file(at("theme.name")).expect("remove theme.name");
    dump("theme.name missing");
    write_file(&at("theme.name"), "empty-theme");
    dump("theme.name names a theme without backgrounds");
    write_file(&at("theme.name"), " beta \r\n\r\n");
    dump("theme.name with whitespace and CRLF");
    write_file(&at("theme.name"), "Upper\n");
    dump("theme.name with an upper-case theme");
    write_file(&at("theme.name"), "\u{fc}n\u{ef}code-th\u{e8}me\n");
    make_symlink(&format!("{share}/\u{fc}n\u{ef}code-th\u{e8}me/backgrounds/u.webp"), &at("background"));
    dump("theme.name with non-ASCII letters");
    write_file(&at("theme.name"), " \t\n");
    dump("theme.name only whitespace");
    write_file(&at("theme.name"), "beta\n");
    make_symlink("missing.png", &at("background"));
    dump("background link is broken");
    make_symlink("theme/backgrounds/current-b.png", &at("background"));
    dump("background link is relative");
    make_symlink("theme/backgrounds", &at("background"));
    dump("background links to a directory");
    make_symlink("link-two", &at("background"));
    make_symlink(&format!("{config}/beta/backgrounds/x.jpg"), &at("link-two"));
    dump("background is a chain of links");
    std::fs::remove_file(at("background")).expect("remove background");
    write_file(&at("background"), "image");
    dump("background is a regular file");
    std::fs::remove_file(at("background")).expect("remove background");
    dump("no background link");
    std::fs::remove_dir_all(at("theme")).expect("remove theme");
    dump("no background link and no theme/backgrounds");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", h.in_home("alt-state"));
    dump("OMAREEL_OMARCHY_STATE_DIR absolute");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", h.in_home("alt-state/./"));
    dump("OMAREEL_OMARCHY_STATE_DIR with ./ and a trailing slash");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", h.in_home("Pictures/../alt-state"));
    dump("OMAREEL_OMARCHY_STATE_DIR with ..");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", "alt-state");
    dump("OMAREEL_OMARCHY_STATE_DIR relative (working directory $HOME)");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", h.in_home("nowhere"));
    dump("OMAREEL_OMARCHY_STATE_DIR missing");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", "");
    dump("OMAREEL_OMARCHY_STATE_DIR empty");
    h.unset_env("OMAREEL_OMARCHY_STATE_DIR");
    make_symlink(&h.in_home("Pictures/outside.png"), &at("background"));
    std::fs::rename(h.in_home(".config/omarchy"), h.in_home(".config/omarchy-away")).expect("rename");
    std::fs::rename(h.in_home(".local/share/omarchy"), h.in_home(".local/share/omarchy-away")).expect("rename");
    dump("no theme folders");
    std::fs::remove_file(at("theme.name")).expect("remove theme.name");
    dump("no theme folders and no theme.name");

    assert_golden("omarchypaths/state.txt", &out.into_string());
    assert_eq!(dropped.get(), 0, "paths outside the hermetic directories, with the system theme root missing");
}

#[test]
fn recordings_directory_txt() {
    let h = Hermetic::new("omarchypaths");
    let mut out = Text::new();
    let mut row = |label: &str| out.row([label.to_owned(), quote(&h.sanitize(&omarchy_paths::recordings_directory()))]);
    let xdg_config = h.root("XDG_CONFIG_HOME");
    let user_dirs = qdir::file_path(&xdg_config, "user-dirs.dirs");
    for (label, value) in [
        ("OMAREEL_RECENTS_DIR=/srv/recents", "/srv/recents".to_owned()),
        ("OMAREEL_RECENTS_DIR=/srv/x/../recents/", "/srv/x/../recents/".to_owned()),
        ("OMAREEL_RECENTS_DIR=relative/recents", "relative/recents".to_owned()),
        ("OMAREEL_RECENTS_DIR=~/recents", "~/recents".to_owned()),
        ("OMAREEL_RECENTS_DIR=' '", " ".to_owned()),
        ("OMAREEL_RECENTS_DIR=$HOME/with space/", h.in_home("with space/")),
        ("OMAREEL_RECENTS_DIR=//double//slashes", "//double//slashes".to_owned()),
    ] {
        h.set_env("OMAREEL_RECENTS_DIR", value);
        row(label);
    }
    h.set_env("OMAREEL_RECENTS_DIR", "");
    row("OMAREEL_RECENTS_DIR='' and no user-dirs.dirs");
    h.unset_env("OMAREEL_RECENTS_DIR");
    row("no user-dirs.dirs");
    for (label, content) in [
        ("user-dirs.dirs $HOME/Movies", "XDG_VIDEOS_DIR=\"$HOME/Movies\"\n"),
        ("user-dirs.dirs absolute", "XDG_VIDEOS_DIR=\"/srv/videos\"\n"),
        ("user-dirs.dirs unquoted", "XDG_VIDEOS_DIR=$HOME/Unquoted\n"),
        ("user-dirs.dirs $HOME alone", "XDG_VIDEOS_DIR=\"$HOME\"\n"),
        ("user-dirs.dirs $HOME/", "XDG_VIDEOS_DIR=\"$HOME/\"\n"),
        ("user-dirs.dirs relative", "XDG_VIDEOS_DIR=\"relative/videos\"\n"),
        ("user-dirs.dirs with spaces", "XDG_VIDEOS_DIR=\"$HOME/With Space/Vids\"\n"),
        ("user-dirs.dirs ${HOME}", "XDG_VIDEOS_DIR=\"${HOME}/Braces\"\n"),
        ("user-dirs.dirs trailing slash", "XDG_VIDEOS_DIR=\"$HOME/Movies/\"\n"),
        (
            "user-dirs.dirs comments and other keys",
            "# written by xdg-user-dirs-update\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_VIDEOS_DIR=\"$HOME/Clips\"\nXDG_MUSIC_DIR=\"$HOME/Music\"\n",
        ),
        ("user-dirs.dirs spaces around =", "XDG_VIDEOS_DIR = \"$HOME/Spaced\"\n"),
        ("user-dirs.dirs indented", "   XDG_VIDEOS_DIR=\"$HOME/Indented\"\n"),
        ("user-dirs.dirs twice", "XDG_VIDEOS_DIR=\"$HOME/First\"\nXDG_VIDEOS_DIR=\"$HOME/Second\"\n"),
        ("user-dirs.dirs CRLF", "XDG_VIDEOS_DIR=\"$HOME/Crlf\"\r\n"),
        ("user-dirs.dirs no newline", "XDG_VIDEOS_DIR=\"$HOME/NoNewline\""),
        ("user-dirs.dirs empty value", "XDG_VIDEOS_DIR=\"\"\n"),
        ("user-dirs.dirs non-ASCII", "XDG_VIDEOS_DIR=\"$HOME/Vid\u{e9}os\"\n"),
        ("user-dirs.dirs escaped quote", "XDG_VIDEOS_DIR=\"$HOME/a\\\"b\"\n"),
        ("user-dirs.dirs lower-case key", "xdg_videos_dir=\"$HOME/Lower\"\n"),
        ("user-dirs.dirs empty file", ""),
    ] {
        write_file(&user_dirs, content);
        row(label);
    }
    std::fs::remove_file(&user_dirs).expect("remove user-dirs.dirs");
    h.set_env("XDG_VIDEOS_DIR", "/env/videos");
    row("XDG_VIDEOS_DIR=/env/videos in the environment, no user-dirs.dirs");
    h.unset_env("XDG_VIDEOS_DIR");
    write_file(&h.in_home(".config/user-dirs.dirs"), "XDG_VIDEOS_DIR=\"$HOME/FromHomeConfig\"\n");
    h.unset_env("XDG_CONFIG_HOME");
    row("XDG_CONFIG_HOME unset, $HOME/.config/user-dirs.dirs");
    write_file(&h.in_home("relconfig/user-dirs.dirs"), "XDG_VIDEOS_DIR=\"$HOME/FromRelative\"\n");
    h.set_env("XDG_CONFIG_HOME", "relconfig");
    row("XDG_CONFIG_HOME=relconfig (relative)");
    assert_golden("omarchypaths/recordings-directory.txt", &out.into_string());
}

/// `absoluteFilePath()` UTF-8, NUL, `size()`, NUL, `lastModified().toMSecsSinceEpoch()`, as the
/// oracle's `identityOf` builds it.
fn identity_of(path: &str) -> Vec<u8> {
    let source = qdir::FileInfo::new(path);
    let mut identity = source.absolute_file_path().into_bytes();
    identity.push(0);
    identity.extend_from_slice(source.size().to_string().as_bytes());
    identity.push(0);
    identity.extend_from_slice(source.last_modified_msecs().to_string().as_bytes());
    identity
}

/// The oracle's `dumpThumbnailPath`.
fn dump_thumbnail_path(out: &mut Text, h: &Hermetic, label: &str, input: &str) {
    let result = omarchy_paths::wallpaper_thumbnail_path(input);
    let identity = identity_of(input);
    let hash = qdir::sha256_hex(&identity);
    let matches = result.ends_with(&format!("/{hash}.jpg"));
    let identity_text = String::from_utf8(identity).expect("UTF-8 identity");
    let mut shown = h.sanitize(&result);
    // A hash over a path inside the scratch HOME changes with the machine.
    if h.sanitize(&identity_text) != identity_text {
        shown = shown.replace(&hash, "{sha256(identity)}");
    }
    out.row([
        label.to_owned(),
        quote(&h.sanitize(input)),
        quote(&h.sanitize(&identity_text)),
        if matches { "hash=sha256(identity)" } else { "hash-differs" }.to_owned(),
        quote(&shown),
    ]);
}

#[test]
fn wallpaper_thumbnail_path_txt() {
    let h = Hermetic::new("omarchypaths");
    let xdg_cache = h.root("XDG_CACHE_HOME");
    let wall = h.in_home("walls/wall.png");
    write_file(&wall, vec![b'w'; 12345]);
    set_times(&wall, 1_700_000_000, 123_456_789);
    let wall_link = h.in_home("walls/link.png");
    make_symlink("wall.png", &wall_link);
    set_times(&wall_link, 1_600_000_000, 0);
    let empty = h.in_home("walls/empty.jpg");
    write_file(&empty, "");
    set_times(&empty, 0, 0);
    let old = h.in_home("walls/before-1970.jpg");
    write_file(&old, "x");
    set_times(&old, -86400, 500_000_000);
    let mut out = Text::new();
    for (label, input) in [
        ("missing absolute", "/nonexistent/oracle/wall.png"),
        ("missing absolute with spaces and non-ASCII", "/nonexistent/a b/\u{fc}\u{1F600}.jpg"),
        ("missing with dot segments", "/nonexistent/x/../y/./wall.png"),
        ("missing with trailing slash", "/nonexistent/dir/"),
        ("empty", ""),
        ("existing", wall.as_str()),
        ("symlink", wall_link.as_str()),
        ("empty file at the epoch", empty.as_str()),
        ("before 1970", old.as_str()),
        ("relative (working directory $HOME)", "walls/wall.png"),
    ] {
        dump_thumbnail_path(&mut out, &h, label, input);
    }
    h.unset_env("XDG_CACHE_HOME");
    dump_thumbnail_path(&mut out, &h, "missing absolute, XDG_CACHE_HOME unset", "/nonexistent/oracle/wall.png");
    h.set_env("XDG_CACHE_HOME", "relative-cache");
    dump_thumbnail_path(&mut out, &h, "missing absolute, XDG_CACHE_HOME relative", "/nonexistent/oracle/wall.png");
    h.set_env("XDG_CACHE_HOME", format!("{xdg_cache}/./sub/.."));
    dump_thumbnail_path(
        &mut out,
        &h,
        "missing absolute, XDG_CACHE_HOME=$XDG_CACHE_HOME/./sub/..",
        "/nonexistent/oracle/wall.png",
    );
    assert_golden("omarchypaths/wallpaper-thumbnail-path.txt", &out.into_string());
}

/// Not from a golden: the two path variables are decoded as `qEnvironmentVariable` decodes
/// them, with a leading byte-order mark dropped and invalid UTF-8 as U+FFFD. (Checked against
/// Qt 6.11.2.)
#[test]
fn path_variables_decode_as_qt_does() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let h = Hermetic::new("omarchypaths");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", OsStr::from_bytes(b"/s\xffx"));
    assert_eq!(omarchy_paths::state_root(), "/s\u{fffd}x");
    h.set_env("OMAREEL_OMARCHY_STATE_DIR", OsStr::from_bytes(b"\xef\xbb\xbf/s"));
    assert_eq!(omarchy_paths::state_root(), "/s");
    h.set_env("OMAREEL_RECENTS_DIR", OsStr::from_bytes(b"/r\xffx/"));
    assert_eq!(omarchy_paths::recordings_directory(), "/r\u{fffd}x");
}
