//! The `theme` object QML reads its colours, fonts and spacing from (`Theme`).
//!
//! The palette is computed by `omareel_core::theme`; this object holds it as Qt values and
//! reloads it when the Omarchy theme changes. `ThemeBase` (src/plumbing) watches the files
//! and asks for the reload, and owns the font calls that need a QGuiApplication.

use core::pin::Pin;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QColor, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use omareel_core::compat::qcolor;
use omareel_core::theme;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qcolor.h");
        type QColor = cxx_qt_lib::QColor;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;

        include!("QtShims.h");
        #[namespace = "Omareel::Shims"]
        #[cxx_name = "fromRgba64"]
        fn from_rgba64(red: u16, green: u16, blue: u16, alpha: u16) -> QColor;
    }

    #[namespace = "Omareel"]
    unsafe extern "C++Qt" {
        include!("ThemeBase.h");
        #[qobject]
        type ThemeBase;
    }

    #[namespace = "Omareel"]
    unsafe extern "C++" {
        #[cxx_name = "configuredFamily"]
        #[namespace = "Omareel::ThemeFonts"]
        fn configured_font_family() -> QString;
    }

    #[namespace = "Omareel"]
    extern "RustQt" {
        #[qobject]
        #[base = ThemeBase]
        #[qproperty(QColor, accent, cxx_name = "accent", READ, NOTIFY = changed)]
        #[qproperty(QColor, accent_foreground, cxx_name = "accentForeground", READ, NOTIFY = changed)]
        #[qproperty(QColor, background, cxx_name = "background", READ, NOTIFY = changed)]
        #[qproperty(QColor, lighter_background, cxx_name = "lighterBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, dark_background, cxx_name = "darkBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, foreground, cxx_name = "foreground", READ, NOTIFY = changed)]
        #[qproperty(QColor, surface, cxx_name = "surface", READ, NOTIFY = changed)]
        #[qproperty(QColor, surface_raised, cxx_name = "surfaceRaised", READ, NOTIFY = changed)]
        #[qproperty(QColor, hairline, cxx_name = "hairline", READ, NOTIFY = changed)]
        #[qproperty(QColor, hairline_strong, cxx_name = "hairlineStrong", READ, NOTIFY = changed)]
        #[qproperty(QColor, separator, cxx_name = "separator", READ, NOTIFY = changed)]
        #[qproperty(QColor, text_muted, cxx_name = "textMuted", READ, NOTIFY = changed)]
        #[qproperty(QColor, text_faint, cxx_name = "textFaint", READ, NOTIFY = changed)]
        #[qproperty(QColor, accent_soft, cxx_name = "accentSoft", READ, NOTIFY = changed)]
        #[qproperty(QColor, zoom_accent, cxx_name = "zoomAccent", READ, NOTIFY = changed)]
        #[qproperty(QColor, record, cxx_name = "record", READ, NOTIFY = changed)]
        #[qproperty(QColor, normal_fill, cxx_name = "normalFill", READ, NOTIFY = changed)]
        #[qproperty(QColor, hover_fill, cxx_name = "hoverFill", READ, NOTIFY = changed)]
        #[qproperty(QColor, selected_fill, cxx_name = "selectedFill", READ, NOTIFY = changed)]
        #[qproperty(QColor, pressed_fill, cxx_name = "pressedFill", READ, NOTIFY = changed)]
        #[qproperty(QColor, selection_fill, cxx_name = "selectionFill", READ, NOTIFY = changed)]
        #[qproperty(QColor, normal_border, cxx_name = "normalBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, hover_border, cxx_name = "hoverBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, selected_border, cxx_name = "selectedBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, popup_background, cxx_name = "popupBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, popup_text, cxx_name = "popupText", READ, NOTIFY = changed)]
        #[qproperty(QColor, popup_border, cxx_name = "popupBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, tooltip_background, cxx_name = "tooltipBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, tooltip_text, cxx_name = "tooltipText", READ, NOTIFY = changed)]
        #[qproperty(QColor, tooltip_border, cxx_name = "tooltipBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_background, cxx_name = "menuBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_text, cxx_name = "menuText", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_border, cxx_name = "menuBorder", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_scrim, cxx_name = "menuScrim", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_selected_background, cxx_name = "menuSelectedBackground", READ, NOTIFY = changed)]
        #[qproperty(QColor, menu_selected_text, cxx_name = "menuSelectedText", READ, NOTIFY = changed)]
        #[qproperty(QColor, canvas, cxx_name = "canvas", READ, NOTIFY = changed)]
        #[qproperty(QColor, card, cxx_name = "card", READ, NOTIFY = changed)]
        #[qproperty(QColor, raised, cxx_name = "raised", READ, NOTIFY = changed)]
        #[qproperty(QColor, stroke, cxx_name = "stroke", READ, NOTIFY = changed)]
        #[qproperty(QColor, stroke_strong, cxx_name = "strokeStrong", READ, NOTIFY = changed)]
        #[qproperty(QColor, fill, cxx_name = "fill", READ, NOTIFY = changed)]
        #[qproperty(QColor, fill_hover, cxx_name = "fillHover", READ, NOTIFY = changed)]
        #[qproperty(QColor, fill_pressed, cxx_name = "fillPressed", READ, NOTIFY = changed)]
        #[qproperty(QColor, text_strong, cxx_name = "textStrong", READ, NOTIFY = changed)]
        #[qproperty(QColor, accent_hover, cxx_name = "accentHover", READ, NOTIFY = changed)]
        #[qproperty(QColor, accent_pressed, cxx_name = "accentPressed", READ, NOTIFY = changed)]
        #[qproperty(QColor, shadow, cxx_name = "shadow", READ, NOTIFY = changed)]
        #[qproperty(QString, font_family, cxx_name = "fontFamily", READ, NOTIFY = changed)]
        #[qproperty(QString, mono_family, cxx_name = "monoFamily", READ, NOTIFY = changed)]
        #[qproperty(QString, theme_name, cxx_name = "themeName", READ, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, font, READ, NOTIFY = changed)]
        #[qproperty(QMap_QString_QVariant, space, READ, NOTIFY = changed)]
        #[qproperty(i32, radius, READ, NOTIFY = changed)]
        #[qproperty(i32, card_radius, cxx_name = "cardRadius", READ, NOTIFY = changed)]
        #[qproperty(i32, control_radius, cxx_name = "controlRadius", READ, NOTIFY = changed)]
        #[qproperty(i32, small_radius, cxx_name = "smallRadius", READ, NOTIFY = changed)]
        #[qproperty(bool, rounded, READ, NOTIFY = changed)]
        #[qproperty(i32, normal_border_width, cxx_name = "normalBorderWidth", READ, NOTIFY = changed)]
        #[qproperty(i32, hover_border_width, cxx_name = "hoverBorderWidth", READ, NOTIFY = changed)]
        #[qproperty(i32, selected_border_width, cxx_name = "selectedBorderWidth", READ, NOTIFY = changed)]
        #[qproperty(bool, dark, READ, NOTIFY = changed)]
        type Theme = super::ThemeRust;

        #[qsignal]
        fn changed(self: Pin<&mut Theme>);

        #[cxx_override]
        fn reload(self: Pin<&mut Theme>);

        #[cxx_override]
        #[cxx_name = "typeMap"]
        fn type_sizes(self: &Theme) -> QMap_QString_QVariant;
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[cxx_name = "watchPaths"]
        fn watch_paths(self: Pin<&mut Theme>, paths: &QStringList);

        #[inherit]
        #[qsignal]
        #[cxx_name = "typeChanged"]
        fn type_changed(self: Pin<&mut Theme>);
    }

    #[namespace = "Omareel"]
    unsafe extern "C++" {
        #[cxx_name = "setApplicationFont"]
        #[namespace = "Omareel::ThemeFonts"]
        fn set_application_font(family: &QString, pixel_size: i32);
        #[cxx_name = "bundledUiFamily"]
        #[namespace = "Omareel::ThemeFonts"]
        fn bundled_ui_family() -> QString;
    }

    impl cxx_qt::Initialize for Theme {}

    #[namespace = "omareel::theme"]
    extern "Rust" {
        /// The theme's background and foreground, which the composition draws keystroke pills
        /// with.
        #[cxx_name = "renderBackground"]
        fn render_background() -> QColor;
        #[cxx_name = "renderForeground"]
        fn render_foreground() -> QColor;
        /// shell.toml's `[font] base-size`, 12 without one.
        #[cxx_name = "shellFontBaseSize"]
        fn shell_font_base_size() -> i32;
    }
}

fn render_background() -> QColor {
    to_qt(&theme::render_colors().0)
}

fn render_foreground() -> QColor {
    to_qt(&theme::render_colors().1)
}

fn shell_font_base_size() -> i32 {
    theme::shell_font_base_size()
}

pub struct ThemeRust {
    accent: QColor,
    accent_foreground: QColor,
    background: QColor,
    lighter_background: QColor,
    dark_background: QColor,
    foreground: QColor,
    surface: QColor,
    surface_raised: QColor,
    hairline: QColor,
    hairline_strong: QColor,
    separator: QColor,
    text_muted: QColor,
    text_faint: QColor,
    accent_soft: QColor,
    zoom_accent: QColor,
    record: QColor,
    normal_fill: QColor,
    hover_fill: QColor,
    selected_fill: QColor,
    pressed_fill: QColor,
    selection_fill: QColor,
    normal_border: QColor,
    hover_border: QColor,
    selected_border: QColor,
    popup_background: QColor,
    popup_text: QColor,
    popup_border: QColor,
    tooltip_background: QColor,
    tooltip_text: QColor,
    tooltip_border: QColor,
    menu_background: QColor,
    menu_text: QColor,
    menu_border: QColor,
    menu_scrim: QColor,
    menu_selected_background: QColor,
    menu_selected_text: QColor,
    canvas: QColor,
    card: QColor,
    raised: QColor,
    stroke: QColor,
    stroke_strong: QColor,
    fill: QColor,
    fill_hover: QColor,
    fill_pressed: QColor,
    text_strong: QColor,
    accent_hover: QColor,
    accent_pressed: QColor,
    shadow: QColor,
    font_family: QString,
    mono_family: QString,
    theme_name: QString,
    font: QMap<QMapPair_QString_QVariant>,
    space: QMap<QMapPair_QString_QVariant>,
    type_map: QMap<QMapPair_QString_QVariant>,
    radius: i32,
    card_radius: i32,
    control_radius: i32,
    small_radius: i32,
    rounded: bool,
    normal_border_width: i32,
    hover_border_width: i32,
    selected_border_width: i32,
    dark: bool,
    /// The palette and what survives reloads; computed on the first reload.
    state: Option<theme::Theme>,
}

impl Default for ThemeRust {
    fn default() -> ThemeRust {
        ThemeRust {
            accent: QColor::default(),
            accent_foreground: QColor::default(),
            background: QColor::default(),
            lighter_background: QColor::default(),
            dark_background: QColor::default(),
            foreground: QColor::default(),
            surface: QColor::default(),
            surface_raised: QColor::default(),
            hairline: QColor::default(),
            hairline_strong: QColor::default(),
            separator: QColor::default(),
            text_muted: QColor::default(),
            text_faint: QColor::default(),
            accent_soft: QColor::default(),
            zoom_accent: QColor::default(),
            record: QColor::default(),
            normal_fill: QColor::default(),
            hover_fill: QColor::default(),
            selected_fill: QColor::default(),
            pressed_fill: QColor::default(),
            selection_fill: QColor::default(),
            normal_border: QColor::default(),
            hover_border: QColor::default(),
            selected_border: QColor::default(),
            popup_background: QColor::default(),
            popup_text: QColor::default(),
            popup_border: QColor::default(),
            tooltip_background: QColor::default(),
            tooltip_text: QColor::default(),
            tooltip_border: QColor::default(),
            menu_background: QColor::default(),
            menu_text: QColor::default(),
            menu_border: QColor::default(),
            menu_scrim: QColor::default(),
            menu_selected_background: QColor::default(),
            menu_selected_text: QColor::default(),
            canvas: QColor::default(),
            card: QColor::default(),
            raised: QColor::default(),
            stroke: QColor::default(),
            stroke_strong: QColor::default(),
            fill: QColor::default(),
            fill_hover: QColor::default(),
            fill_pressed: QColor::default(),
            text_strong: QColor::default(),
            accent_hover: QColor::default(),
            accent_pressed: QColor::default(),
            shadow: QColor::default(),
            font_family: QString::from("sans-serif"),
            mono_family: QString::from("monospace"),
            theme_name: QString::default(),
            font: QMap::default(),
            space: QMap::default(),
            type_map: QMap::default(),
            radius: 0,
            card_radius: 0,
            control_radius: 0,
            small_radius: 0,
            rounded: false,
            normal_border_width: 1,
            hover_border_width: 1,
            selected_border_width: 0,
            dark: true,
            state: None,
        }
    }
}

/// A colour with the same 16-bit channels, or an invalid one.
pub fn to_qt(color: &qcolor::QColor) -> QColor {
    if !color.is_valid() {
        return QColor::default();
    }
    let (r, g, b, a) = color.rgba64();
    qobject::from_rgba64(r, g, b, a)
}

fn int_map(values: &[(&str, i32)]) -> QMap<QMapPair_QString_QVariant> {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    for (key, value) in values {
        map.insert(QString::from(*key), QVariant::from(value));
    }
    map
}

impl cxx_qt::Initialize for qobject::Theme {
    fn initialize(mut self: Pin<&mut Self>) {
        let configured = qobject::configured_font_family();
        let family = if !configured.is_empty() { configured } else { qobject::bundled_ui_family() };
        if !family.is_empty() {
            self.as_mut().rust_mut().font_family = family;
        }
        self.reload();
    }
}

impl qobject::Theme {
    fn type_sizes(&self) -> QMap<QMapPair_QString_QVariant> {
        self.type_map.clone()
    }

    fn reload(mut self: Pin<&mut Self>) {
        // The first reload builds the state, which reads the files; later ones re-read them.
        let font_family = self.font_family.to_string();
        let state = match self.as_mut().rust_mut().state.take() {
            Some(mut state) => {
                state.reload();
                state
            }
            None => theme::Theme::new(Some(font_family)),
        };
        let palette = state.palette.clone();
        {
            let mut theme = self.as_mut().rust_mut();
            theme.accent = to_qt(&palette.accent);
            theme.accent_foreground = to_qt(&palette.accent_foreground());
            theme.background = to_qt(&palette.background);
            theme.lighter_background = to_qt(&palette.lighter_background);
            theme.dark_background = to_qt(&palette.dark_background);
            theme.foreground = to_qt(&palette.foreground);
            theme.surface = to_qt(&palette.surface());
            theme.surface_raised = to_qt(&palette.surface_raised());
            theme.hairline = to_qt(&palette.hairline());
            theme.hairline_strong = to_qt(&palette.hairline_strong());
            theme.separator = to_qt(&palette.separator());
            theme.text_muted = to_qt(&palette.text_muted());
            theme.text_faint = to_qt(&palette.text_faint());
            theme.accent_soft = to_qt(&palette.accent_soft());
            theme.zoom_accent = to_qt(&palette.zoom_accent);
            theme.record = to_qt(&palette.record);
            theme.normal_fill = to_qt(&palette.normal_fill);
            theme.hover_fill = to_qt(&palette.hover_fill);
            theme.selected_fill = to_qt(&palette.selected_fill);
            theme.pressed_fill = to_qt(&palette.pressed_fill);
            theme.selection_fill = to_qt(&palette.selection_fill);
            theme.normal_border = to_qt(&palette.normal_border);
            theme.hover_border = to_qt(&palette.hover_border);
            theme.selected_border = to_qt(&palette.selected_border);
            theme.popup_background = to_qt(&palette.popup_background);
            theme.popup_text = to_qt(&palette.popup_text);
            theme.popup_border = to_qt(&palette.popup_border);
            theme.tooltip_background = to_qt(&palette.tooltip_background);
            theme.tooltip_text = to_qt(&palette.tooltip_text);
            theme.tooltip_border = to_qt(&palette.tooltip_border);
            theme.menu_background = to_qt(&palette.menu_background);
            theme.menu_text = to_qt(&palette.menu_text);
            theme.menu_border = to_qt(&palette.menu_border);
            theme.menu_scrim = to_qt(&palette.menu_scrim);
            theme.menu_selected_background = to_qt(&palette.menu_selected_background);
            theme.menu_selected_text = to_qt(&palette.menu_selected_text);
            theme.canvas = to_qt(&palette.canvas);
            theme.card = to_qt(&palette.card);
            theme.raised = to_qt(&palette.raised);
            theme.stroke = to_qt(&palette.stroke());
            theme.stroke_strong = to_qt(&palette.stroke_strong());
            theme.fill = to_qt(&palette.fill());
            theme.fill_hover = to_qt(&palette.fill_hover());
            theme.fill_pressed = to_qt(&palette.fill_pressed());
            theme.text_strong = to_qt(&palette.text_strong);
            theme.accent_hover = to_qt(&palette.accent_hover());
            theme.accent_pressed = to_qt(&palette.accent_pressed());
            theme.shadow = to_qt(&palette.shadow());
            theme.dark = palette.dark;
            theme.normal_border_width = palette.normal_border_width;
            theme.hover_border_width = palette.hover_border_width;
            theme.selected_border_width = palette.selected_border_width;
            theme.theme_name = QString::from(&state.theme_name);
            theme.mono_family = QString::from(&state.mono_family);
            theme.font_family = QString::from(&state.font_family);
            theme.type_map = int_map(&palette.type_sizes);
            theme.font = int_map(&palette.font_sizes);
            theme.space = int_map(&palette.spaces);
            let radii = state.radii();
            theme.radius = state.radius;
            theme.card_radius = radii.card;
            theme.control_radius = radii.control;
            theme.small_radius = radii.small;
            theme.rounded = state.rounded();
            theme.state = Some(state);
        }
        let family = self.font_family.clone();
        qobject::set_application_font(&family, palette.body_size());
        let mut paths = QStringList::default();
        for path in theme::watched_paths() {
            paths.append(QString::from(&path));
        }
        self.as_mut().watch_paths(&paths);
        self.as_mut().type_changed();
        self.changed();
    }
}
