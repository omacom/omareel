//! Where Omarchy keeps its state, and what omareel reads from there (`OmarchyPaths`).
//!
//! `generateWallpaperThumbnail` stays in C++: it decodes and scales the image with `QImage`.

use crate::compat::qdir::{self, Filters, SortFlags};
use crate::compat::qfile;
use crate::compat::qstandardpaths::{self, Location};
use std::collections::{BTreeMap, HashSet};

/// The name filters the wallpaper listings use.
const IMAGE_FILTERS: [&str; 4] = ["*.png", "*.jpg", "*.jpeg", "*.webp"];

/// The current Omarchy state directory, or `OMAREEL_OMARCHY_STATE_DIR`.
pub fn state_root() -> String {
    match qdir::env_var("OMAREEL_OMARCHY_STATE_DIR") {
        Some(path) if !path.is_empty() => qdir::absolute_path(&path),
        _ => qdir::file_path(&qdir::home_path(), ".local/state/omarchy/current"),
    }
}

/// The current theme's name, or empty.
pub fn current_theme_name() -> String {
    current_theme_name_in(&state_root())
}

/// `theme.name` read as `QFile` with `QIODevice::Text` and `QString::fromUtf8(...).trimmed()`
/// read it: every `\r` dropped, a leading byte-order mark dropped, invalid UTF-8 replaced the
/// way Qt replaces it.
fn current_theme_name_in(state_root: &str) -> String {
    match qfile::read_all(qdir::file_path(state_root, "theme.name")) {
        Ok(mut bytes) => {
            bytes.retain(|&b| b != b'\r');
            qdir::decode_name(&bytes).trim().to_owned()
        }
        Err(_) => String::new(),
    }
}

/// The wallpaper Omarchy shows: the target of the state directory's `background` link when
/// that is a file, otherwise the first image (by name) in the theme's `backgrounds`; empty
/// when there is none. Canonical, so symlinks are resolved.
pub fn current_background() -> String {
    current_background_in(&state_root())
}

fn current_background_in(state_root: &str) -> String {
    let link = qdir::FileInfo::new(qdir::file_path(state_root, "background"));
    if link.is_symlink() {
        let target = qdir::FileInfo::new(link.sym_link_target());
        if target.is_file() {
            return target.canonical_file_path();
        }
    }
    let directory = qdir::file_path(state_root, "theme/backgrounds");
    let backgrounds: Vec<String> = qdir::entry_info_list(&directory, &IMAGE_FILTERS, Filters::FILES, SortFlags::NAME)
        .iter()
        .map(qdir::FileInfo::canonical_file_path)
        .collect();
    backgrounds.into_iter().next().unwrap_or_default()
}

/// One theme's wallpapers, as the background picker groups them (the C++ returned it as a
/// `QVariantMap` with these four keys).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackgroundGroup {
    /// The theme's directory name, or "Current" for the current wallpaper of an unnamed theme.
    pub name: String,
    /// The name with "-" as spaces and the first letter upper case.
    pub label: String,
    /// Whether this is the current theme.
    pub current: bool,
    /// Canonical paths of the wallpapers.
    pub paths: Vec<String>,
}

/// Where themes live, in the order they are scanned: the user's own, then the local share,
/// then Omarchy's system themes.
pub fn theme_roots() -> Vec<String> {
    let home = qdir::home_path();
    vec![
        qdir::file_path(&home, ".config/omarchy/themes"),
        qdir::file_path(&home, ".local/share/omarchy/themes"),
        "/usr/share/omarchy/themes".to_owned(),
    ]
}

/// Every theme's wallpapers in [`theme_roots`], grouped by theme.
pub fn theme_background_groups() -> Vec<BackgroundGroup> {
    theme_background_groups_in(&theme_roots())
}

/// `themeBackgroundGroups()` over the given theme roots: every `*.png`, `*.jpg`, `*.jpeg` and
/// `*.webp` file (case-insensitive) in `<root>/<theme>/backgrounds`, themes and files by name,
/// each canonical path once (the first theme to list it keeps it). The current background is
/// put first in the current theme's group (or "Current") when no theme listed it. Groups come
/// by name in `QMap` order, the current theme's first.
pub fn theme_background_groups_in(roots: &[String]) -> Vec<BackgroundGroup> {
    background_groups(roots, &state_root())
}

fn background_groups(roots: &[String], state_root: &str) -> Vec<BackgroundGroup> {
    // QMap<QString, QStringList>: keys in UTF-16 code unit order.
    let mut grouped: BTreeMap<Vec<u16>, (String, Vec<String>)> = BTreeMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let key = |name: &str| name.encode_utf16().collect::<Vec<u16>>();
    for root in roots {
        for theme in qdir::entry_info_list(root, &[], Filters::DIRS | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME) {
            let backgrounds = qdir::file_path(&theme.absolute_file_path(), "backgrounds");
            for file in qdir::entry_info_list(&backgrounds, &IMAGE_FILTERS, Filters::FILES, SortFlags::NAME) {
                let path = file.canonical_file_path();
                if path.is_empty() || seen.contains(&path) {
                    continue;
                }
                seen.insert(path.clone());
                let name = theme.file_name();
                grouped.entry(key(&name)).or_insert_with(|| (name, Vec::new())).1.push(path);
            }
        }
    }

    let current_name = current_theme_name_in(state_root);
    let current_path = current_background_in(state_root);
    if !current_path.is_empty() && !seen.contains(&current_path) {
        let name = if current_name.is_empty() { "Current".to_owned() } else { current_name.clone() };
        grouped.entry(key(&name)).or_insert_with(|| (name, Vec::new())).1.insert(0, current_path);
    }

    let mut names: Vec<(String, Vec<String>)> = grouped.into_values().collect();
    if !current_name.is_empty() {
        if let Some(index) = names.iter().position(|(name, _)| *name == current_name) {
            let current = names.remove(index);
            names.insert(0, current);
        }
    }
    names
        .into_iter()
        .map(|(name, paths)| BackgroundGroup { label: label(&name), current: name == current_name, name, paths })
        .collect()
}

/// A theme name as the picker shows it: "-" becomes a space and the first UTF-16 unit is made
/// upper case with `QChar::toUpper`.
fn label(name: &str) -> String {
    let label = name.replace('-', " ");
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => qchar_to_upper(first).to_string() + chars.as_str(),
        None => label,
    }
}

/// `QChar::toUpper`: the simple upper-case mapping, so a letter whose upper case is several
/// characters (ß, ŉ, ᾳ) stays as it is, and a character outside the BMP, which `QString`'s first
/// unit only holds half of, is not changed.
fn qchar_to_upper(c: char) -> char {
    if u32::from(c) > 0xffff {
        return c;
    }
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// Every wallpaper of [`theme_background_groups`], group by group.
pub fn theme_backgrounds() -> Vec<String> {
    theme_backgrounds_in(&theme_roots())
}

/// [`theme_background_groups_in`]'s paths, group by group.
pub fn theme_backgrounds_in(roots: &[String]) -> Vec<String> {
    theme_background_groups_in(roots).into_iter().flat_map(|group| group.paths).collect()
}

/// Where recordings go: `OMAREEL_RECENTS_DIR` made absolute, or "omareel" in the user's
/// videos folder.
pub fn recordings_directory() -> String {
    match qdir::env_var("OMAREEL_RECENTS_DIR") {
        Some(path) if !path.is_empty() => qdir::absolute_path(&path),
        _ => qdir::file_path(&qstandardpaths::writable_location(Location::Movies), "omareel"),
    }
}

/// Where the picker's thumbnail of a wallpaper is cached: the SHA-256 of the file's absolute
/// path, size and modification time (milliseconds), as `<hash>.jpg` in
/// `$XDG_CACHE_HOME/omareel/wallpapers`. A missing file counts as size 0 and time 0.
pub fn wallpaper_thumbnail_path(wallpaper_path: &str) -> String {
    let source = qdir::FileInfo::new(wallpaper_path);
    let mut identity = source.absolute_file_path().into_bytes();
    identity.push(0);
    identity.extend_from_slice(source.size().to_string().as_bytes());
    identity.push(0);
    identity.extend_from_slice(source.last_modified_msecs().to_string().as_bytes());
    let hash = qdir::sha256_hex(&identity);
    let directory = qdir::file_path(&qstandardpaths::writable_location(Location::GenericCache), "omareel/wallpapers");
    qdir::file_path(&directory, &(hash + ".jpg"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let path = std::env::temp_dir().join(format!("omareel-paths-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Scratch(std::fs::canonicalize(&path).unwrap())
        }

        fn path(&self, relative: &str) -> String {
            self.0.join(relative).to_str().unwrap().to_owned()
        }

        fn touch(&self, relative: &str) -> String {
            let path = self.path(relative);
            std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
            std::fs::write(&path, b"image").unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn labels_upper_case_the_first_unit_as_qchar_does() {
        assert_eq!(label("my-multi-word-theme"), "My multi word theme");
        assert_eq!(label("-dash-first"), " dash first");
        assert_eq!(label("ünïcode-thème"), "Ünïcode thème");
        assert_eq!(label(""), "");
        // QChar::toUpper keeps letters whose upper case is longer, and can't see past half of
        // a surrogate pair. (Checked against Qt 6.11.2.)
        assert_eq!(label("ßtheme"), "ßtheme");
        assert_eq!(label("\u{1f80}"), "\u{1f80}");
        assert_eq!(label("\u{1c6}x"), "\u{1c4}x");
        assert_eq!(label("\u{b5}"), "\u{39c}");
        assert_eq!(label("\u{10428}x"), "\u{10428}x");
    }

    #[test]
    fn theme_name_reads_as_qt_text_mode_does() {
        let scratch = Scratch::new("name");
        let state = scratch.path("state");
        std::fs::create_dir_all(&state).unwrap();
        let name = scratch.path("state/theme.name");
        // QIODevice::Text drops every \r, QString::fromUtf8 a leading BOM. (Checked against
        // Qt 6.11.2.)
        for (bytes, expected) in [
            (&b"beta\n"[..], "beta"),
            (b" a\rb \r\n\r\n", "ab"),
            (b"\xef\xbb\xbfbeta\n", "beta"),
            (b"b\xffeta\xc3\n", "b\u{fffd}eta\u{fffd}"),
            (b" \xc2\x85x\xc2\xa0\n", "x"),
        ] {
            std::fs::write(&name, bytes).unwrap();
            assert_eq!(current_theme_name_in(&state), expected, "{bytes:?}");
        }
        std::fs::remove_file(&name).unwrap();
        assert_eq!(current_theme_name_in(&state), "");
    }

    #[test]
    fn groups_backgrounds_by_theme_with_the_current_one_first() {
        let scratch = Scratch::new("groups");
        let config = scratch.path("config");
        let share = scratch.path("share");
        let b = scratch.touch("config/beta/backgrounds/b.PNG");
        let a = scratch.touch("config/alpha/backgrounds/a.jpg");
        scratch.touch("config/alpha/backgrounds/notes.txt");
        scratch.touch("config/alpha/backgrounds/.hidden.png");
        let shared = scratch.touch("share/alpha/backgrounds/s.webp");
        std::fs::create_dir_all(scratch.path("share/zeta/backgrounds")).unwrap();
        symlink(&a, scratch.path("share/zeta/backgrounds/z.jpg")).unwrap();
        let outside = scratch.touch("elsewhere/wall.png");
        let state = scratch.path("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(scratch.path("state/theme.name"), "beta\n").unwrap();
        symlink(&outside, scratch.path("state/background")).unwrap();
        let roots = [config, share, scratch.path("missing")];

        let groups = background_groups(&roots, &state);
        let group = |name: &str, label: &str, current: bool, paths: &[&String]| BackgroundGroup {
            name: name.into(),
            label: label.into(),
            current,
            paths: paths.iter().map(|p| (*p).clone()).collect(),
        };
        // z.jpg links to a.jpg, which alpha already has, so zeta has no group.
        assert_eq!(
            groups,
            [group("beta", "Beta", true, &[&outside, &b]), group("alpha", "Alpha", false, &[&a, &shared])]
        );

        std::fs::remove_file(scratch.path("state/theme.name")).unwrap();
        let groups = background_groups(&roots, &state);
        assert_eq!(groups[0], group("Current", "Current", false, &[&outside]));
        assert_eq!(groups.len(), 3);

        // A background some theme lists is not repeated.
        std::fs::remove_file(scratch.path("state/background")).unwrap();
        symlink(&b, scratch.path("state/background")).unwrap();
        assert_eq!(current_background_in(&state), b);
        assert_eq!(background_groups(&roots, &state).len(), 2);
    }

    #[test]
    fn current_background_falls_back_to_the_first_theme_background() {
        let scratch = Scratch::new("current");
        let state = scratch.path("state");
        std::fs::create_dir_all(&state).unwrap();
        assert_eq!(current_background_in(&state), "");
        let upper = scratch.touch("state/theme/backgrounds/Current-A.jpg");
        scratch.touch("state/theme/backgrounds/current-b.png");
        scratch.touch("state/theme/backgrounds/.hidden.png");
        assert_eq!(current_background_in(&state), upper);
        // A link to something that is not a file doesn't count.
        symlink(scratch.path("state/theme/backgrounds"), scratch.path("state/background")).unwrap();
        assert_eq!(current_background_in(&state), upper);
    }
}
