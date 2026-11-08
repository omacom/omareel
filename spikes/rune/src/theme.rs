//! omareel's colours from the current Omarchy theme, through omareel_core's `Theme` (what the
//! QML reads as `theme`): every palette token, as Rune colours, plus the radius and the font
//! families. Field names follow omareel_core's palette (`theme.textMuted` in QML is
//! `ui.text_muted` here).

use omareel_core::compat::qcolor::QColor;
use rune::Color;

/// Rune's `Color` is linear light (sRGB primaries, straight alpha); Qt's colours are encoded.
pub fn color(c: &QColor) -> Color {
    let d = |v: i32| rune::color::srgb_decode(v as f32 / 255.0);
    Color { r: d(c.red()), g: d(c.green()), b: d(c.blue()), a: c.alpha() as f32 / 255.0 }
}

/// A colour from "#rrggbb" or "#aarrggbb" (Qt's order), as project values hold them.
pub fn hex(s: &str) -> Color {
    color(&QColor::parse(s))
}

#[derive(Clone)]
pub struct Ui {
    pub accent: Color,
    pub zoom_accent: Color,
    pub background: Color,
    pub foreground: Color,
    pub lighter_background: Color,
    pub dark_background: Color,
    pub record: Color,
    pub canvas: Color,
    pub card: Color,
    pub raised: Color,
    pub text_strong: Color,
    pub normal_fill: Color,
    pub hover_fill: Color,
    pub selected_fill: Color,
    pub pressed_fill: Color,
    pub selection_fill: Color,
    pub normal_border: Color,
    pub hover_border: Color,
    pub selected_border: Color,
    pub popup_background: Color,
    pub popup_text: Color,
    pub popup_border: Color,
    pub tooltip_background: Color,
    pub tooltip_text: Color,
    pub tooltip_border: Color,
    pub menu_background: Color,
    pub menu_text: Color,
    pub menu_border: Color,
    pub menu_scrim: Color,
    pub menu_selected_background: Color,
    pub menu_selected_text: Color,
    pub surface: Color,
    pub surface_raised: Color,
    pub hairline: Color,
    pub hairline_strong: Color,
    pub separator: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub accent_soft: Color,
    pub stroke: Color,
    pub stroke_strong: Color,
    pub fill: Color,
    pub fill_hover: Color,
    pub fill_pressed: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,
    pub shadow: Color,
    pub accent_foreground: Color,
    /// Kept for the code written before the full palette: `text` is `foreground`, `accent_text`
    /// `accent_foreground`.
    pub text: Color,
    pub accent_text: Color,
    /// The theme's corner radius (Omarchy's shell.toml), in points.
    pub radius: f32,
    pub font_family: String,
    pub mono_family: String,
    pub theme_name: String,
}

impl Ui {
    pub fn load() -> Ui {
        let theme = omareel_core::theme::Theme::new(None);
        let p = &theme.palette;
        Ui {
            accent: color(&p.accent),
            zoom_accent: color(&p.zoom_accent),
            background: color(&p.background),
            foreground: color(&p.foreground),
            lighter_background: color(&p.lighter_background),
            dark_background: color(&p.dark_background),
            record: color(&p.record),
            canvas: color(&p.canvas),
            card: color(&p.card),
            raised: color(&p.raised),
            text_strong: color(&p.text_strong),
            normal_fill: color(&p.normal_fill),
            hover_fill: color(&p.hover_fill),
            selected_fill: color(&p.selected_fill),
            pressed_fill: color(&p.pressed_fill),
            selection_fill: color(&p.selection_fill),
            normal_border: color(&p.normal_border),
            hover_border: color(&p.hover_border),
            selected_border: color(&p.selected_border),
            popup_background: color(&p.popup_background),
            popup_text: color(&p.popup_text),
            popup_border: color(&p.popup_border),
            tooltip_background: color(&p.tooltip_background),
            tooltip_text: color(&p.tooltip_text),
            tooltip_border: color(&p.tooltip_border),
            menu_background: color(&p.menu_background),
            menu_text: color(&p.menu_text),
            menu_border: color(&p.menu_border),
            menu_scrim: color(&p.menu_scrim),
            menu_selected_background: color(&p.menu_selected_background),
            menu_selected_text: color(&p.menu_selected_text),
            surface: color(&p.surface()),
            surface_raised: color(&p.surface_raised()),
            hairline: color(&p.hairline()),
            hairline_strong: color(&p.hairline_strong()),
            separator: color(&p.separator()),
            text_muted: color(&p.text_muted()),
            text_faint: color(&p.text_faint()),
            accent_soft: color(&p.accent_soft()),
            stroke: color(&p.stroke()),
            stroke_strong: color(&p.stroke_strong()),
            fill: color(&p.fill()),
            fill_hover: color(&p.fill_hover()),
            fill_pressed: color(&p.fill_pressed()),
            accent_hover: color(&p.accent_hover()),
            accent_pressed: color(&p.accent_pressed()),
            shadow: color(&p.shadow()),
            accent_foreground: color(&p.accent_foreground()),
            text: color(&p.foreground),
            accent_text: color(&p.accent_foreground()),
            radius: theme.radius as f32,
            font_family: theme.font_family.clone(),
            mono_family: theme.mono_family.clone(),
            theme_name: theme.theme_name.clone(),
        }
    }
}
