//! The editor's colours from the current Omarchy theme, through omareel_core's `Theme` (what the
//! QML reads as `theme`).

use omareel_core::compat::qcolor::QColor;
use rune::Color;

/// Rune's `Color` is linear light (sRGB primaries, straight alpha); Qt's colours are encoded.
pub fn color(c: &QColor) -> Color {
    let d = |v: i32| rune::color::srgb_decode(v as f32 / 255.0);
    Color { r: d(c.red()), g: d(c.green()), b: d(c.blue()), a: c.alpha() as f32 / 255.0 }
}

#[derive(Clone)]
pub struct Ui {
    pub surface: Color,
    pub raised: Color,
    pub canvas: Color,
    pub card: Color,
    pub text: Color,
    pub text_strong: Color,
    pub text_muted: Color,
    pub text_faint: Color,
    pub accent: Color,
    pub accent_text: Color,
    pub zoom_accent: Color,
    pub record: Color,
    pub normal_fill: Color,
    pub hover_fill: Color,
    pub pressed_fill: Color,
    pub hairline: Color,
    pub shadow: Color,
    pub radius: f32,
}

impl Ui {
    pub fn load() -> Ui {
        let theme = omareel_core::theme::Theme::new(None);
        let p = &theme.palette;
        Ui {
            surface: color(&p.surface()),
            raised: color(&p.surface_raised()),
            canvas: color(&p.canvas),
            card: color(&p.card),
            text: color(&p.foreground),
            text_strong: color(&p.text_strong),
            text_muted: color(&p.text_muted()),
            text_faint: color(&p.text_faint()),
            accent: color(&p.accent),
            accent_text: color(&p.accent_foreground()),
            zoom_accent: color(&p.zoom_accent),
            record: color(&p.record),
            normal_fill: color(&p.normal_fill),
            hover_fill: color(&p.hover_fill),
            pressed_fill: color(&p.pressed_fill),
            hairline: color(&p.hairline()),
            shadow: color(&p.shadow()),
            radius: theme.radius as f32,
        }
    }
}
