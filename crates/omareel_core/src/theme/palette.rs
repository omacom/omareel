//! Theme's colours, sizes and spacing, derived from the theme's colors.toml and shell.toml
//! (`Theme::reload` and the colour getters in Theme.cpp).

use crate::compat::qcolor::QColor;
use crate::compat::qnumber;
use crate::compat::qround::{q_bound, q_round};
use std::collections::HashMap;

pub type Values = HashMap<String, String>;

fn alpha_color(color: &QColor, alpha: f64) -> QColor {
    let mut result = *color;
    result.set_alpha_f(q_bound(0.0, alpha, 1.0) as f32);
    result
}

/// `mixColor`: the C++ adds float channels to a double product, then narrows to float again
/// for `QColor::fromRgbF`.
fn mix_color(from: &QColor, to: &QColor, amount: f64) -> QColor {
    let t = q_bound(0.0, amount, 1.0);
    let mix = |a: f32, b: f32| (a as f64 + (b - a) as f64 * t) as f32;
    QColor::from_rgb_f(
        mix(from.red_f(), to.red_f()),
        mix(from.green_f(), to.green_f()),
        mix(from.blue_f(), to.blue_f()),
        mix(from.alpha_f(), to.alpha_f()),
    )
}

fn luminance(color: &QColor) -> f64 {
    let linear = |channel: i32| {
        let channel = channel as f64 / 255.0;
        if channel <= 0.04045 { channel / 12.92 } else { ((channel + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * linear(color.red()) + 0.7152 * linear(color.green()) + 0.0722 * linear(color.blue())
}

fn black() -> QColor {
    QColor::from_rgb(0, 0, 0)
}

fn white() -> QColor {
    QColor::from_rgb(255, 255, 255)
}

/// A colour from shell.toml: a role name, or a colour (the first word of the value).
fn role_color(value: &str, foreground: &QColor, background: &QColor, accent: &QColor, attention: &QColor, fallback: &QColor) -> QColor {
    let value = value.trim();
    match value.to_lowercase().as_str() {
        "foreground" | "text" | "hyprland.active-border-foreground" => return *foreground,
        "background" => return *background,
        "accent" | "hyprland.active-border" => return *accent,
        "urgent" => return *attention,
        _ => {}
    }
    let parsed = QColor::parse(value.split(' ').next().unwrap_or_default());
    if parsed.is_valid() { parsed } else { *fallback }
}

fn get<'a>(values: &'a Values, key: &str) -> &'a str {
    values.get(key).map_or("", String::as_str)
}

/// Everything `Theme::reload` derives from the two files. Names follow Theme's properties.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub accent: QColor,
    pub zoom_accent: QColor,
    pub background: QColor,
    pub foreground: QColor,
    pub lighter_background: QColor,
    pub dark_background: QColor,
    pub record: QColor,
    pub dark: bool,
    pub canvas: QColor,
    pub card: QColor,
    pub raised: QColor,
    pub text_strong: QColor,
    pub normal_fill: QColor,
    pub hover_fill: QColor,
    pub selected_fill: QColor,
    pub pressed_fill: QColor,
    pub selection_fill: QColor,
    pub normal_border: QColor,
    pub hover_border: QColor,
    pub selected_border: QColor,
    pub normal_border_width: i32,
    pub hover_border_width: i32,
    pub selected_border_width: i32,
    pub popup_background: QColor,
    pub popup_text: QColor,
    pub popup_border: QColor,
    pub tooltip_background: QColor,
    pub tooltip_text: QColor,
    pub tooltip_border: QColor,
    pub menu_background: QColor,
    pub menu_text: QColor,
    pub menu_border: QColor,
    pub menu_scrim: QColor,
    pub menu_selected_background: QColor,
    pub menu_selected_text: QColor,
    /// `type`: text sizes for QML, in pixels.
    pub type_sizes: Vec<(&'static str, i32)>,
    /// `font`: the shell's font tokens.
    pub font_sizes: Vec<(&'static str, i32)>,
    /// `space`: spacing tokens.
    pub spaces: Vec<(&'static str, i32)>,
}

impl Palette {
    pub fn from_files(colors: &Values, shell: &Values) -> Palette {
        let color_or = |key: &str, fallback: &str| QColor::parse(values_or(colors, key, fallback));
        let accent = color_or(".accent", "#7aa2f7");
        let zoom_accent = color_or(".color4", "#bb9af7");
        let background = color_or(".background", "#1a1b26");
        let foreground = color_or(".foreground", "#a9b1d6");
        let lighter =
            if colors.contains_key(".lighter_background") { QColor::parse(get(colors, ".lighter_background")) } else { background.lighter(106) };
        let darker = if colors.contains_key(".dark_background") {
            QColor::parse(get(colors, ".dark_background"))
        } else {
            QColor::from_rgb(
                q_round(background.red() as f64 * 0.75),
                q_round(background.green() as f64 * 0.75),
                q_round(background.blue() as f64 * 0.75),
            )
        };
        let attention =
            role_color(values_or(shell, "bar.active", get(colors, ".red")), &foreground, &background, &accent, &accent, &accent);
        let number = |key: &str, fallback: f64| shell.get(key).and_then(|v| qnumber::to_double(v)).unwrap_or(fallback);
        let color = |key: &str, fallback: &QColor| role_color(get(shell, key), &foreground, &background, &accent, &attention, fallback);
        let surface = |key: &str, fallback: &QColor, fallback_alpha: f64| {
            alpha_color(&color(key, fallback), number(&format!("{key}-alpha"), fallback_alpha))
        };

        let dark = get(colors, ".mode") == "dark" || luminance(&background) < 0.35;
        // Layered surfaces: the stage sits below the chrome, cards and raised controls lift
        // above it.
        let canvas = if dark {
            if colors.contains_key(".dark_background") { darker } else { mix_color(&background, &black(), 0.22) }
        } else {
            mix_color(&background, &foreground, 0.05)
        };
        let card = mix_color(&background, &foreground, if dark { 0.035 } else { 0.025 });
        let raised = if dark { mix_color(&background, &foreground, 0.11) } else { mix_color(&background, &white(), 0.7) };
        let text_strong = if colors.contains_key(".bright_foreground") && dark {
            QColor::parse(get(colors, ".bright_foreground"))
        } else if dark {
            mix_color(&foreground, &white(), 0.25)
        } else {
            mix_color(&foreground, &black(), 0.25)
        };

        let normal = color("controls.normal-color", &foreground);
        let hover = color("controls.hover-cursor-color", &foreground);
        let selected = color("controls.selected-color", &foreground);
        let width = |key: &str, fallback: f64| q_round(number(key, fallback)).max(0);

        let base = q_round(number("font.base-size", 12.0)).max(1);
        let type_size = |multiplier: f64| q_round(base as f64 * multiplier).max(1);
        let font_token = |key: &str, multiplier: f64| match shell.get(&format!("font.{key}")).and_then(|v| qnumber::to_int(v)) {
            Some(explicit) => explicit.max(1),
            None => q_round(base as f64 * multiplier).max(1),
        };
        let scale_with_font = values_or(shell, "spacing.scale-with-font", "true") != "false";
        let spacing_scale = number("spacing.scale", 1.0) * if scale_with_font { base as f64 / 12.0 } else { 1.0 };
        let spacing = |key: &str, fallback: i32| {
            let explicit = shell.get(&format!("spacing.{key}")).and_then(|v| qnumber::to_double(v));
            q_round(explicit.unwrap_or(fallback as f64 * spacing_scale)).max(0)
        };

        Palette {
            normal_fill: alpha_color(&normal, number("controls.normal-fill-alpha", 0.04)),
            hover_fill: alpha_color(&hover, number("controls.hover-cursor-fill-alpha", 0.08)),
            selected_fill: alpha_color(&selected, number("controls.selected-fill-alpha", 0.18)),
            pressed_fill: alpha_color(&color("controls.pressed-color", &hover), number("controls.pressed-fill-alpha", 0.22)),
            selection_fill: alpha_color(&color("controls.selection-color", &foreground), number("controls.selection-fill-alpha", 0.35)),
            normal_border: alpha_color(&color("controls.normal-border", &normal), number("controls.normal-border-alpha", 0.40)),
            hover_border: alpha_color(&color("controls.hover-cursor-border", &hover), number("controls.hover-cursor-border-alpha", 0.25)),
            selected_border: alpha_color(&color("controls.selected-border", &selected), number("controls.selected-border-alpha", 1.0)),
            normal_border_width: width("controls.normal-border-width", 1.0),
            hover_border_width: width("controls.hover-cursor-border-width", 1.0),
            selected_border_width: width("controls.selected-border-width", 0.0),
            popup_background: surface("popups.background", &background, 1.0),
            popup_text: color("popups.text", &foreground),
            popup_border: surface("popups.border", &accent, 1.0),
            tooltip_background: surface("tooltip.background", &background, 0.97),
            tooltip_text: color("tooltip.text", &foreground),
            tooltip_border: surface("tooltip.border", &foreground, 1.0),
            menu_background: surface("menu.background", &background, 1.0),
            menu_text: color("menu.text", &foreground),
            menu_border: surface("menu.border", &foreground, 1.0),
            menu_scrim: surface("menu.scrim", &background, 0.5),
            menu_selected_background: surface("menu.selected-background", &foreground, 0.08),
            menu_selected_text: color("menu.selected-text", &accent),
            type_sizes: vec![
                ("caption", type_size(0.917)),
                ("small", type_size(1.0)),
                ("body", type_size(1.083)),
                ("title", type_size(1.167)),
                ("headline", type_size(1.333)),
                ("display", type_size(1.833)),
                ("hero", type_size(2.667)),
            ],
            font_sizes: vec![
                ("base", base),
                ("caption", font_token("caption", 0.833)),
                ("bodySmall", font_token("body-small", 0.917)),
                ("body", font_token("body", 1.0)),
                ("subtitle", font_token("subtitle", 1.083)),
                ("title", font_token("title", 1.167)),
                ("heading", font_token("heading", 1.333)),
                ("display", font_token("display", 2.0)),
            ],
            spaces: vec![
                ("xxs", spacing("xxs", 2)),
                ("xs", spacing("xs", 3)),
                ("sm", spacing("sm", 4)),
                ("md", spacing("md", 6)),
                ("lg", spacing("lg", 8)),
                ("xl", spacing("xl", 10)),
                ("xxl", spacing("xxl", 12)),
                ("xxxl", spacing("xxxl", 14)),
                ("huge", spacing("huge", 18)),
                ("controlGap", spacing("control-gap", 8)),
                ("controlPaddingX", spacing("control-padding-x", 10)),
                ("controlPaddingY", spacing("control-padding-y", 6)),
                ("controlHeight", spacing("control-height", 28)),
                ("popupRowHeight", spacing("popup-row-height", 28)),
                ("rowGap", spacing("row-gap", 8)),
                ("rowPaddingX", spacing("row-padding-x", 12)),
                ("panelGap", spacing("panel-gap", 14)),
                ("panelPadding", spacing("panel-padding", 18)),
                ("popupPadding", spacing("popup-padding", 14)),
            ],
            accent,
            zoom_accent,
            background,
            foreground,
            lighter_background: lighter,
            dark_background: darker,
            record: attention,
            dark,
            canvas,
            card,
            raised,
            text_strong,
        }
    }

    /// The body text size, which the application font is set to.
    pub fn body_size(&self) -> i32 {
        self.type_sizes.iter().find(|(k, _)| *k == "body").map_or(0, |(_, v)| *v)
    }

    pub fn surface(&self) -> QColor {
        self.background
    }

    pub fn surface_raised(&self) -> QColor {
        self.lighter_background
    }

    pub fn hairline(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.07 } else { 0.10 })
    }

    pub fn hairline_strong(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.12 } else { 0.16 })
    }

    pub fn separator(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.08 } else { 0.11 })
    }

    /// Muted text is mixed toward the background, so it recedes in light themes as well as
    /// dark.
    pub fn text_muted(&self) -> QColor {
        mix_color(&self.foreground, &self.background, 0.36)
    }

    pub fn text_faint(&self) -> QColor {
        mix_color(&self.foreground, &self.background, 0.56)
    }

    pub fn accent_soft(&self) -> QColor {
        alpha_color(&self.accent, 0.16)
    }

    pub fn stroke(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.08 } else { 0.12 })
    }

    pub fn stroke_strong(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.14 } else { 0.20 })
    }

    pub fn fill(&self) -> QColor {
        alpha_color(&self.foreground, 0.06)
    }

    pub fn fill_hover(&self) -> QColor {
        alpha_color(&self.foreground, 0.10)
    }

    pub fn fill_pressed(&self) -> QColor {
        alpha_color(&self.foreground, if self.dark { 0.15 } else { 0.14 })
    }

    pub fn accent_hover(&self) -> QColor {
        if self.dark { self.accent.lighter(112) } else { self.accent.darker(108) }
    }

    pub fn accent_pressed(&self) -> QColor {
        if self.dark { self.accent.darker(112) } else { self.accent.darker(118) }
    }

    pub fn shadow(&self) -> QColor {
        QColor::from_rgba(0, 0, 0, if self.dark { 140 } else { 46 })
    }

    pub fn accent_foreground(&self) -> QColor {
        if luminance(&self.accent) > 0.179 { QColor::from_rgb(16, 17, 22) } else { white() }
    }
}

/// `QHash::value(key, fallback)`.
fn values_or<'a>(values: &'a Values, key: &str, fallback: &'a str) -> &'a str {
    values.get(key).map_or(fallback, String::as_str)
}

/// The corner radius in `hyprctl getoption decoration:rounding -j` output.
pub fn rounding_from_hyprctl(output: &str) -> Option<i32> {
    // `"int"\s*:\s*(\d+)`
    let mut rest = output;
    while let Some(at) = rest.find("\"int\"") {
        let after = rest[at + 5..].trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
        if let Some(after) = after.strip_prefix(':') {
            let after = after.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            if !digits.is_empty() {
                // `captured(1).toInt()`: 0 when the digits overflow.
                return Some(qnumber::to_int(&digits).unwrap_or(0));
            }
        }
        rest = &rest[at + 5..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> Values {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    // tests/test_omarchypaths.cpp themeDerivesShadesAndMode
    #[test]
    fn derives_shades_and_mode() {
        let mut colors = values(&[(".background", "#faf4ed"), (".foreground", "#575279"), (".accent", "#907aa9")]);
        let inferred = Palette::from_files(&colors, &Values::new());
        assert_eq!(inferred.lighter_background, QColor::parse("#faf4ed").lighter(106));
        assert_eq!(inferred.dark_background, QColor::from_rgb(q_round(250.0 * 0.75), q_round(244.0 * 0.75), q_round(237.0 * 0.75)));
        assert!(!inferred.dark);
        colors.insert(".mode".into(), "dark".into());
        assert!(Palette::from_files(&colors, &Values::new()).dark);
    }

    #[test]
    fn reads_the_hyprctl_rounding() {
        assert_eq!(rounding_from_hyprctl("{\"option\": \"decoration:rounding\", \"int\": 8, \"set\": true}"), Some(8));
        assert_eq!(rounding_from_hyprctl("{\"int\" :\n 12}"), Some(12));
        assert_eq!(rounding_from_hyprctl("{\"int\": \"x\"}"), None);
        assert_eq!(rounding_from_hyprctl("{\"int\": 99999999999}"), Some(0));
    }
}
