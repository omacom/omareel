//! The interface palette derived from the Omarchy theme (`Theme`).

mod palette;
pub mod toml;

pub use palette::{Palette, Values, rounding_from_hyprctl};

use crate::compat::qprocess;
use crate::compat::qround::{q_bound, q_round};
use crate::omarchy_paths;

/// The theme files Theme reads, from the Omarchy state directory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sources {
    pub colors: Values,
    pub shell: Values,
    pub theme_name: String,
}

impl Sources {
    pub fn read() -> Sources {
        let theme = crate::compat::qdir::file_path(&omarchy_paths::state_root(), "theme");
        Sources {
            colors: toml::read(&crate::compat::qdir::file_path(&theme, "colors.toml")),
            shell: toml::read(&crate::compat::qdir::file_path(&theme, "shell.toml")),
            theme_name: omarchy_paths::current_theme_name(),
        }
    }
}

/// Theme's state: the palette from the files, plus what it keeps between reloads when
/// fontconfig or Hyprland don't answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    /// The interface face: what main() resolved, the bundled Inter, or "sans-serif".
    pub font_family: String,
    /// The Omarchy monospace, for numbers, timecodes and keys.
    pub mono_family: String,
    pub theme_name: String,
    pub radius: i32,
}

impl Theme {
    /// A theme loaded from the current Omarchy theme. `font_family` is the interface face, if
    /// one was found.
    pub fn new(font_family: Option<String>) -> Theme {
        let mut theme = Theme {
            palette: Palette::from_files(&Values::new(), &Values::new()),
            font_family: font_family.filter(|f| !f.is_empty()).unwrap_or_else(|| "sans-serif".into()),
            mono_family: "monospace".into(),
            theme_name: String::new(),
            radius: 0,
        };
        theme.reload();
        theme
    }

    /// Reads the theme files again. The monospace family and the radius keep their values when
    /// fc-match or hyprctl fail; `OMAREEL_THEME_RADIUS` overrides the radius.
    pub fn reload(&mut self) {
        let sources = Sources::read();
        self.palette = Palette::from_files(&sources.colors, &sources.shell);
        self.theme_name = sources.theme_name;
        if let Some(mono) = monospace_family() {
            self.mono_family = mono;
        }
        if let Some(radius) = rounding() {
            self.radius = radius;
        }
        if let Some(radius) = radius_override() {
            self.radius = radius.max(0);
        }
    }

    pub fn radii(&self) -> Radii {
        Radii::from(self.radius)
    }

    pub fn rounded(&self) -> bool {
        self.radius > 0
    }
}

/// fontconfig's monospace family, which carries numbers, timecodes and keys.
pub fn monospace_family() -> Option<String> {
    let done = qprocess::run("fc-match", &["-f", "%{family[0]}", "monospace"], 1200)?;
    let family = String::from_utf8_lossy(&done.stdout).trim().to_owned();
    (done.succeeded() && !family.is_empty()).then_some(family)
}

/// Hyprland's window corner radius.
pub fn rounding() -> Option<i32> {
    let done = qprocess::run("hyprctl", &["getoption", "decoration:rounding", "-j"], 1200)?;
    if !done.succeeded() {
        return None;
    }
    rounding_from_hyprctl(&String::from_utf8_lossy(&done.stdout))
}

/// `OMAREEL_THEME_RADIUS`, read as `qEnvironmentVariableIntValue` reads it: base 10, or
/// hexadecimal after `0x`, or octal after a leading `0`.
pub fn radius_override() -> Option<i32> {
    env_int_value(&std::env::var_os("OMAREEL_THEME_RADIUS")?.to_string_lossy())
}

fn env_int_value(text: &str) -> Option<i32> {
    let text = text.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = if let Some(hex) = digits.strip_prefix("0x").or_else(|| digits.strip_prefix("0X")) {
        (16, hex)
    } else if digits.len() > 1 && digits.starts_with('0') {
        (8, &digits[1..])
    } else {
        (10, digits)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

/// The smaller radii, from the window radius.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Radii {
    pub card: i32,
    pub control: i32,
    pub small: i32,
}

impl From<i32> for Radii {
    fn from(radius: i32) -> Radii {
        if radius <= 0 {
            return Radii { card: 0, control: 0, small: 0 };
        }
        Radii {
            card: q_bound(6, radius, 12),
            control: q_bound(3, q_round(radius as f64 * 0.6), 8),
            small: q_bound(2, q_round(radius as f64 * 0.4), 5),
        }
    }
}

/// What to watch for theme changes: the state directory, its parent, the theme directory
/// and the two files.
pub fn watched_paths() -> Vec<String> {
    use crate::compat::qdir;
    let current = omarchy_paths::state_root();
    let theme = qdir::file_path(&current, "theme");
    vec![
        qdir::FileInfo::new(current.as_str()).absolute_path(),
        current.clone(),
        theme.clone(),
        qdir::file_path(&theme, "colors.toml"),
        qdir::file_path(&theme, "shell.toml"),
    ]
}

/// The background and foreground colors.toml names, with Theme's defaults; what the
/// composition draws keystroke pills with.
pub fn render_colors() -> (crate::compat::qcolor::QColor, crate::compat::qcolor::QColor) {
    use crate::compat::qcolor::QColor;
    let colors = toml::read(&crate::compat::qdir::file_path(&omarchy_paths::state_root(), "theme/colors.toml"));
    let color = |key: &str, fallback: &str| QColor::parse(colors.get(key).map_or(fallback, String::as_str));
    (color(".background", "#1a1b26"), color(".foreground", "#a9b1d6"))
}

/// shell.toml's `[font] base-size` (`^\s*base-size\s*=\s*(\d+)` inside `[font]`), at least
/// 1, or 12 when there is none.
pub fn shell_font_base_size() -> i32 {
    let path = crate::compat::qdir::file_path(&omarchy_paths::state_root(), "theme/shell.toml");
    let Ok(bytes) = crate::compat::qfile::read_all(path) else { return 12 };
    let mut in_font = false;
    for line in crate::compat::qfile::lines(&bytes) {
        let line = String::from_utf8_lossy(line);
        if let Some(name) = toml::section(&line) {
            in_font = name == "font";
            continue;
        }
        if !in_font {
            continue;
        }
        let space = |s: &str| s.trim_start_matches([' ', '\t', '\n', '\x0b', '\x0c', '\r']).to_owned();
        let rest = space(&line);
        let Some(rest) = rest.strip_prefix("base-size") else { continue };
        let rest = space(rest);
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = space(rest);
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            // `captured(1).toInt()` is 0 when the digits overflow.
            return crate::compat::qnumber::to_int(&digits).unwrap_or(0).max(1);
        }
    }
    12
}
