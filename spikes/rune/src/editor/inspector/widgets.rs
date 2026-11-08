//! The Om* components the side panel, the top bar and the dialogs share (`src/ui/qml/ui/` and
//! the Panel*/Editor* wrappers): OmButton, OmIconButton, OmButtonGroup, OmToggle, OmSlider with
//! PanelSlider, OmDropdown, OmColorWell, OmSettingRow, OmEmptyState, OmHero, OmSectionHeader,
//! OmSeparator, OmKeycap and OmTextField, with the theme's type scale, spacing and radii.
//!
//! Hover and press looks the QML binds to `hovered`/`pressed` are Rune's `hover`/`press`
//! overlays where a fill changes; where a size or colour of something inside changes (a slider's
//! knob, a swatch's scale) the element's key is tracked in `inspector::State::hot`.

use crate::preview::Preview;
use crate::theme::Ui;
use omareel_core::compat::qcolor::QColor;
use rune::tok::{Family, TextStyle};
use rune::{Color, Cursor, El, Icon, Phase, Tx, col, el, row, text};
use std::rc::Rc;
use std::time::Duration;

/// The theme's sizes (QML's `theme.type`, `theme.space`, the radii), read once.
pub struct Om {
    pub caption: f32,
    pub small: f32,
    pub body: f32,
    pub title: f32,
    pub headline: f32,
    pub control_h: f32,
    pub row_h: f32,
    pub pad_x: f32,
    pub md: f32,
    pub xl: f32,
    pub xxl: f32,
    pub control_gap: f32,
    pub panel_gap: f32,
    pub panel_padding: f32,
    pub popup_padding: f32,
    pub card_radius: f32,
    pub control_radius: f32,
    pub small_radius: f32,
    pub rounded: bool,
    pub dark: bool,
}

pub fn om() -> &'static Om {
    static OM: std::sync::OnceLock<Om> = std::sync::OnceLock::new();
    OM.get_or_init(|| {
        let theme = omareel_core::theme::Theme::new(None);
        let p = &theme.palette;
        let ty = |k: &str, d: f32| p.type_sizes.iter().find(|(n, _)| *n == k).map_or(d, |(_, v)| *v as f32);
        let sp = |k: &str, d: f32| p.spaces.iter().find(|(n, _)| *n == k).map_or(d, |(_, v)| *v as f32);
        let radii = theme.radii();
        Om {
            caption: ty("caption", 11.0),
            small: ty("small", 12.0),
            body: ty("body", 13.0),
            title: ty("title", 14.0),
            headline: ty("headline", 16.0),
            control_h: sp("controlHeight", 28.0),
            row_h: sp("popupRowHeight", 28.0),
            pad_x: sp("controlPaddingX", 10.0),
            md: sp("md", 6.0),
            xl: sp("xl", 10.0),
            xxl: sp("xxl", 12.0),
            control_gap: sp("controlGap", 8.0),
            panel_gap: sp("panelGap", 14.0),
            panel_padding: sp("panelPadding", 18.0),
            popup_padding: sp("popupPadding", 14.0),
            card_radius: radii.card as f32,
            control_radius: radii.control as f32,
            small_radius: radii.small as f32,
            rounded: theme.rounded(),
            dark: p.dark,
        }
    })
}

pub fn font(size: f32, weight: f32) -> TextStyle {
    TextStyle::new(size, weight)
}

pub fn mono(size: f32, weight: f32) -> TextStyle {
    TextStyle { family: Family::SystemMono, ..TextStyle::new(size, weight) }
}

/// A colour from a project's QColor.
pub fn qcolor(c: &QColor) -> Color {
    crate::theme::color(c)
}

pub const WHITE: Color = Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
pub const BLACK: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

/// A handler the widgets keep: cheap to clone into several closures.
pub type Act = Rc<dyn Fn(&mut Preview, &mut Tx)>;

pub fn act(f: impl Fn(&mut Preview, &mut Tx) + 'static) -> Act {
    Rc::new(f)
}

// ---------------------------------------------------------------------------------------------
// Hover and press tracking

pub fn is_hot(s: &Preview, key: &str) -> bool {
    s.edit.inspector.hot.as_deref() == Some(key)
}

/// Tracks the pointer over `e` as `key` in the inspector's `hot`.
pub fn track_hot(e: El, key: &str) -> El {
    let k = key.to_owned();
    e.on_hover_change(move |s: &mut Preview, on: bool, _: &mut Tx| {
        if on {
            s.edit.inspector.hot = Some(k.clone());
        } else if s.edit.inspector.hot.as_deref() == Some(k.as_str()) {
            s.edit.inspector.hot = None;
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Text

/// OmSectionHeader / PanelHeading: small, medium, muted.
pub fn section_header(ui: &Ui, title: &str) -> El {
    text(title.to_owned()).font(font(om().small, 500.0)).color(ui.text_muted)
}

/// A muted, wrapped caption (the panels' explanatory Labels, `theme.type.small`).
pub fn caption(ui: &Ui, s: &str) -> El {
    text(s.to_owned()).font(font(om().small, 400.0)).color(ui.text_muted).wrap().w_full()
}

/// PanelLabel.
pub fn panel_label(ui: &Ui, s: &str, enabled: bool) -> El {
    text(s.to_owned()).font(font(om().body, 400.0)).color(if enabled { ui.foreground } else { ui.text_faint }).truncate()
}

/// OmSeparator.
pub fn separator(ui: &Ui) -> El {
    el().w_full().h(1.0).fill(ui.separator)
}

/// OmHero: the panel's title and what it is for.
pub fn hero(ui: &Ui, title: &str, meta: &str) -> El {
    col()
        .w_full()
        .gap(3.0)
        .child(text(title.to_owned()).font(font(om().headline, 600.0)).color(ui.text_strong).truncate())
        .child(text(meta.to_owned()).font(font(om().small, 400.0)).color(ui.text_muted).truncate())
}

/// OmKeycap: a key in the monospace, with a deeper bottom edge.
pub fn keycap(ui: &Ui, key: &str, height: f32) -> El {
    let o = om();
    row()
        .h(height)
        .min_w(height)
        .px(5.0)
        .justify_center()
        .items_center()
        .radius(o.small_radius.min(4.0))
        .fill(ui.raised)
        .border(1.0, ui.stroke_strong)
        .child(text(key.to_owned()).font(mono(o.caption, 500.0)).color(ui.text_strong))
        .child(el().absolute(1.0, height - 2.0).h(1.0).w_full().mr(2.0).fill(BLACK.alpha(if o.dark { 0.35 } else { 0.08 })))
}

/// OmEmptyState: an icon in a disc, a title and a hint (with a key cap when `key` is set).
pub fn empty_state(ui: &Ui, icon: &'static Icon, title: &str, hint: &str, key: &str) -> El {
    let o = om();
    let disc = el()
        .mt(8.0)
        .size(40.0, 40.0)
        .radius(if o.rounded { 20.0 } else { 0.0 })
        .fill(ui.fill)
        .border(1.0, ui.stroke)
        .center()
        .child(rune::icon(icon, 18.0).color(ui.text_muted));
    let mut e = col()
        .w_full()
        .items_center()
        .gap(10.0)
        .child(disc)
        .child(text(title.to_owned()).font(font(o.body, 600.0)).color(ui.text_strong));
    if key.is_empty() {
        e = e.child(
            el().w_full().px(12.0).pb(8.0).child(text(hint.to_owned()).font(font(o.small, 400.0)).color(ui.text_muted).wrap().w_full().center_text()),
        );
    } else {
        e = e.child(
            row()
                .pb(8.0)
                .gap(5.0)
                .items_center()
                .child(text(hint.to_owned()).font(font(o.small, 400.0)).color(ui.text_muted))
                .child(keycap(ui, key, 20.0)),
        );
    }
    e
}

trait CenterText {
    fn center_text(self) -> El;
}

impl CenterText for El {
    fn center_text(self) -> El {
        self.align(rune::Align::Center)
    }
}

// ---------------------------------------------------------------------------------------------
// Buttons

/// OmButton's look: ghost by default; `bordered` the soft filled secondary, `primary` the accent
/// call to action, `destructive` turns either toward the record red.
#[derive(Clone, Copy, Default)]
pub struct Btn {
    pub bordered: bool,
    pub primary: bool,
    pub destructive: bool,
    pub selected: bool,
    pub disabled: bool,
    /// Layout.fillWidth.
    pub fill: bool,
    /// `font.pixelSize: theme.type.small`.
    pub small: bool,
    pub trailing: Option<&'static Icon>,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

impl Btn {
    pub fn bordered() -> Btn {
        Btn { bordered: true, ..Btn::default() }
    }
    pub fn primary() -> Btn {
        Btn { primary: true, ..Btn::default() }
    }
}

/// OmButton.
pub fn button(ui: &Ui, key: &str, label: &str, icon: Option<&'static Icon>, b: Btn, on: Act) -> El {
    let o = om();
    let enabled = !b.disabled;
    let content = if !enabled {
        ui.text_faint
    } else if b.primary {
        if b.destructive { WHITE } else { ui.accent_foreground }
    } else if b.destructive {
        ui.record
    } else if b.selected {
        ui.text_strong
    } else {
        ui.foreground
    };
    let primary_color = if b.destructive { ui.record } else { ui.accent };
    let fill = if b.primary {
        primary_color
    } else if b.selected {
        ui.fill_pressed
    } else if b.bordered {
        ui.fill
    } else {
        Color::TRANSPARENT
    };
    let mut e = row()
        .key(key.to_owned())
        .role("button")
        .label(label.to_owned())
        .h(b.height.unwrap_or(o.control_h))
        .px(o.pad_x + 2.0)
        .gap(o.md)
        .justify_center()
        .items_center()
        .radius(o.control_radius)
        .fill(fill)
        .disabled(!enabled);
    if b.bordered && !b.primary {
        e = e.border(1.0, if b.selected { ui.stroke_strong } else { ui.stroke });
    }
    if b.fill {
        e = e.w_full();
    }
    if let Some(w) = b.width {
        e = e.w(w);
    }
    if b.primary {
        // Light catches the top edge of filled buttons.
        e = e.child(
            el().absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(o.control_radius)
                .inert()
                .linear_gradient(std::f32::consts::PI, [(0.0, WHITE.alpha(0.14)), (0.5, WHITE.alpha(0.0))]),
        );
    }
    if enabled {
        let (hover, press) = if b.primary {
            (WHITE.alpha(0.08), BLACK.alpha(0.12))
        } else if b.destructive {
            (ui.record.alpha(0.14), ui.record.alpha(0.22))
        } else {
            (ui.fill_hover, ui.fill_pressed)
        };
        e = e.hover(hover).press(press).press_scale(0.97).cursor(Cursor::Pointer).on_press_tx(move |s: &mut Preview, tx: &mut Tx| on(s, tx));
    } else {
        e = e.dim(0.55);
    }
    if let Some(i) = icon {
        e = e.child(rune::icon(i, 15.0).color(content));
    }
    if !label.is_empty() {
        e = e.child(text(label.to_owned()).font(font(if b.small { o.small } else { o.body }, 500.0)).color(content).truncate());
    }
    if let Some(i) = b.trailing {
        e = e.child(rune::icon(i, 14.0).color(if b.primary { content } else { ui.text_muted }));
    }
    e
}

/// OmIconButton: a square ghost button with a 16 pt icon and a tooltip.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(s: &Preview, key: &str, icon: &'static Icon, name: &str, tip: &str, enabled: bool, selected: bool, on: Act) -> El {
    let ui = &s.ui;
    let o = om();
    let color = if !enabled {
        ui.text_faint
    } else if selected || is_hot(s, key) {
        ui.text_strong
    } else {
        ui.text_muted
    };
    let mut e = el()
        .key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .size(o.control_h, o.control_h)
        .radius(o.control_radius)
        .center()
        .fill(if selected { ui.fill_pressed } else { Color::TRANSPARENT })
        .disabled(!enabled)
        .child(rune::icon(icon, 16.0).color(color));
    if !tip.is_empty() {
        e = e.help(tip.to_owned());
    }
    if enabled {
        e = track_hot(e.hover(ui.fill_hover).press(ui.fill_pressed).press_scale(0.97).cursor(Cursor::Pointer).on_press_tx(move |s: &mut Preview, tx: &mut Tx| on(s, tx)), key);
    } else {
        e = e.dim(0.55);
    }
    e
}

// ---------------------------------------------------------------------------------------------
// Choices

/// OmButtonGroup: a recessed track with a raised thumb on the chosen option (none when
/// `selected` is None, as a custom spring shows nothing chosen).
#[allow(clippy::too_many_arguments)]
pub fn button_group(s: &Preview, key: &str, labels: &[&str], selected: Option<usize>, enabled: bool, width: Option<f32>, on: impl Fn(&mut Preview, usize) + 'static) -> El {
    let ui = &s.ui;
    let o = om();
    let on = Rc::new(on);
    let mut track = row().key(key.to_owned()).role("radiogroup").h(o.control_h).padding(2.0).radius(o.control_radius).fill(ui.fill);
    track = match width {
        Some(w) => track.w(w),
        None => track.w_full(),
    };
    for (i, label) in labels.iter().enumerate() {
        let chosen = selected == Some(i);
        let seg_key = format!("{key}/segment{i}");
        let hot = is_hot(s, &seg_key);
        let mut seg = row()
            .key(seg_key.clone())
            .role("radio")
            .label((*label).to_owned())
            .checked(chosen)
            // Layout.preferredWidth: the label and 20, then the rest shared.
            .w(label.chars().count() as f32 * o.body * 0.56 + 20.0)
            .grow(1.0)
            .shrink()
            .h_full()
            .min_w(0.0)
            .px(4.0)
            .justify_center()
            .items_center()
            .radius((o.control_radius - 1.0).max(0.0));
        if chosen {
            // The thumb.
            seg = seg.fill(ui.raised).border(1.0, ui.stroke_strong);
        } else if hot && enabled {
            seg = seg.fill(ui.fill_hover);
        }
        let color = if !enabled {
            ui.text_faint
        } else if chosen || hot {
            ui.text_strong
        } else {
            ui.text_muted
        };
        seg = seg.child(text((*label).to_owned()).font(font(o.body, if chosen { 600.0 } else { 500.0 })).color(color).truncate());
        if enabled {
            let on = on.clone();
            seg = track_hot(seg.cursor(Cursor::Pointer).on_press(move |s: &mut Preview| on(s, i)), &seg_key);
        }
        track = track.child(seg);
    }
    track.motion(rune::Spring::SNAPPY)
}

/// One row of a pop-up menu.
#[derive(Clone)]
pub struct MenuItem {
    pub label: String,
    pub enabled: bool,
    pub destructive: bool,
    pub checked: bool,
    pub separator: bool,
}

impl MenuItem {
    pub fn new(label: impl Into<String>) -> MenuItem {
        MenuItem { label: label.into(), enabled: true, destructive: false, checked: false, separator: false }
    }
    pub fn separator() -> MenuItem {
        MenuItem { separator: true, ..MenuItem::new("") }
    }
}

/// OmDropdown (EditorComboBox): the trigger; its list opens over everything as a menu
/// anchored here (`dialogs::open_menu`), the current row ticked.
#[allow(clippy::too_many_arguments)]
pub fn dropdown(s: &Preview, key: &str, items: &[String], current: Option<usize>, width: Option<f32>, enabled: bool, mono_font: bool, on: impl Fn(&mut Preview, usize) + 'static) -> El {
    let ui = &s.ui;
    let o = om();
    let open = s.edit.dialogs.menu.as_ref().is_some_and(|m| m.anchor == key);
    let shown = current.and_then(|i| items.get(i)).cloned().unwrap_or_default();
    let style = if mono_font { mono(o.body, 500.0) } else { font(o.body, 500.0) };
    let mut e = row()
        .key(key.to_owned())
        .role("combobox")
        .label(shown.clone())
        .expanded(open)
        .h(o.control_h)
        .pl(o.pad_x)
        .pr(o.pad_x - 2.0)
        .gap(o.md)
        .items_center()
        .radius(o.control_radius)
        .fill(if open { ui.fill_hover } else { ui.fill })
        .border(1.0, ui.stroke)
        .disabled(!enabled)
        .child(text(shown).font(style).color(if enabled { ui.text_strong } else { ui.text_faint }).grow(1.0).truncate())
        .child(rune::icon(&crate::icons::CHEVRON_DOWN, 14.0).color(if open { ui.foreground } else { ui.text_muted }).rotate(if open { std::f32::consts::PI } else { 0.0 }));
    e = match width {
        Some(w) => e.w(w),
        None => e.w_full(),
    };
    if enabled {
        let rows: Vec<MenuItem> = items.iter().enumerate().map(|(i, l)| MenuItem { checked: current == Some(i), ..MenuItem::new(l.clone()) }).collect();
        let key = key.to_owned();
        let on: Rc<dyn Fn(&mut Preview, usize)> = Rc::new(on);
        let menu_width = width.unwrap_or(160.0).max(160.0);
        e = e.hover(ui.fill_hover).press(ui.fill_pressed).cursor(Cursor::Pointer).on_press(move |s: &mut Preview| {
            crate::editor::dialogs::toggle_menu(s, &key, menu_width, rows.clone(), mono_font, on.clone());
        });
    } else {
        e = e.dim(0.55);
    }
    e
}

/// OmColorWell: a compact swatch that opens the colour dialog.
pub fn color_well(s: &Preview, key: &str, swatch: &QColor, on: Act) -> El {
    let ui = &s.ui;
    let o = om();
    let hot = is_hot(s, key);
    let e = el()
        .key(key.to_owned())
        .role("button")
        .label(format!("Color {}", swatch.name()))
        .size(44.0, 26.0)
        .radius(o.control_radius)
        .border(1.0, if hot { ui.stroke_strong } else { ui.stroke })
        .padding(3.0)
        .cursor(Cursor::Pointer)
        .press_scale(0.95)
        .on_press_tx(move |s: &mut Preview, tx: &mut Tx| on(s, tx))
        .child(el().w_full().h_full().radius((o.control_radius - 2.0).max(0.0)).fill(qcolor(swatch)));
    track_hot(e, key)
}

/// OmSettingRow: a label (and description) on the left, `control` on the right.
pub fn setting_row(ui: &Ui, label: &str, description: &str, enabled: bool, control: El) -> El {
    let o = om();
    let mut labels = col().grow(1.0).min_w(0.0).gap(2.0).child(panel_label(ui, label, enabled));
    if !description.is_empty() {
        labels = labels.child(text(description.to_owned()).font(font(o.caption, 400.0)).color(ui.text_muted).wrap().w_full());
    }
    row().w_full().min_h(o.control_h).gap(o.xxl).items_center().child(labels).child(control)
}

/// OmToggle (EditorSwitch): a label and description, the switch on the right. The whole row
/// toggles it.
pub fn toggle(s: &Preview, key: &str, label: &str, description: &str, checked: bool, enabled: bool, on: impl Fn(&mut Preview, bool) + 'static) -> El {
    let ui = &s.ui;
    let o = om();
    let hot = is_hot(s, key) && enabled;
    let pressed = s.edit.inspector.pressed.as_deref() == Some(key);
    let track_color = if checked {
        if hot { ui.accent_hover } else { ui.accent }
    } else if hot {
        ui.fill_pressed
    } else {
        ui.foreground.alpha(0.14)
    };
    let knob_w = if pressed { 18.0 } else { 14.0 };
    let travel = 34.0 - 14.0 - 6.0;
    let knob = el()
        .key(format!("{key}-knob"))
        .absolute(3.0 + if checked { travel - (knob_w - 14.0) } else { 0.0 }, 3.0)
        .size(knob_w, 14.0)
        .radius(if o.rounded { 7.0 } else { 0.0 })
        .fill(if checked || !o.dark { WHITE } else { ui.text_strong })
        .border(if o.dark { 0.0 } else { 1.0 }, BLACK.alpha(0.08))
        .motion(rune::Spring::SNAPPY);
    let track = el()
        .key(format!("{key}-track"))
        .size(34.0, 20.0)
        .radius(if o.rounded { 10.0 } else { 0.0 })
        .fill(track_color)
        .child(knob);
    let mut labels = col().grow(1.0).min_w(0.0).gap(2.0).child(text(label.to_owned()).font(font(o.body, 400.0)).color(if enabled { ui.foreground } else { ui.text_faint }).truncate());
    if !description.is_empty() {
        labels = labels.child(text(description.to_owned()).font(font(o.caption, 400.0)).color(ui.text_muted).wrap().w_full());
    }
    let mut e = row()
        .key(key.to_owned())
        .role("switch")
        .label(label.to_owned())
        .checked(checked)
        .disabled(!enabled)
        .w_full()
        .min_h(o.control_h)
        .py(4.0)
        .gap(o.control_gap)
        .items_center()
        .child(labels)
        .child(track);
    if enabled {
        let k = key.to_owned();
        e = track_hot(
            e.cursor(Cursor::Pointer).on_drag(move |s: &mut Preview, drag, tx: &mut Tx| match drag.phase {
                Phase::Down => {
                    s.edit.inspector.pressed = Some(k.clone());
                    tx.animated();
                }
                Phase::Up => {
                    s.edit.inspector.pressed = None;
                    tx.animated();
                    let inside = drag.x >= 0.0 && drag.x <= drag.w && drag.y >= 0.0 && drag.y <= drag.h;
                    if inside {
                        on(s, !checked);
                    }
                }
                _ => {}
            }),
            key,
        );
    }
    e
}

// ---------------------------------------------------------------------------------------------
// Sliders

/// What a PanelSlider shows and edits.
pub struct SliderSpec<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub from: f64,
    pub to: f64,
    pub value: f64,
    /// Values snap to multiples of this from `from` (0: none).
    pub step: f64,
    pub decimals: usize,
    pub suffix: &'a str,
    /// The value shown is `value * display_scale` (Volume shows 0..200 %).
    pub display_scale: f64,
    pub enabled: bool,
}

impl<'a> SliderSpec<'a> {
    pub fn new(key: &'a str, label: &'a str, from: f64, to: f64, value: f64, step: f64) -> SliderSpec<'a> {
        SliderSpec { key, label, from, to, value, step, decimals: 0, suffix: "", display_scale: 1.0, enabled: true }
    }
    pub fn decimals(mut self, d: usize) -> Self {
        self.decimals = d;
        self
    }
    pub fn suffix(mut self, s: &'a str) -> Self {
        self.suffix = s;
        self
    }
    pub fn scale(mut self, k: f64) -> Self {
        self.display_scale = k;
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
}

/// PanelSlider over a project value: the press begins a coalesced edit keyed by the path, each
/// move sets the value (coalesced), the release ends it, as PanelSlider.qml does.
pub fn project_slider(s: &Preview, spec: SliderSpec, path: &'static str) -> El {
    panel_slider(s, spec, move |s: &mut Preview, v: f64, phase: Phase| {
        if matches!(phase, Phase::Down) {
            s.document.begin_coalesced_edit(path);
        }
        if !matches!(phase, Phase::Up) {
            crate::editor::set(s, path, v.into(), true, "");
        } else {
            s.document.end_coalesced_edit();
            crate::editor::edited(s, "");
        }
    })
}

/// PanelSlider: the label and its value over an OmSlider; `apply` gets the snapped value and
/// the drag phase (Down: pressed, Move, Up: released, with the last value).
pub fn panel_slider(s: &Preview, spec: SliderSpec, apply: impl Fn(&mut Preview, f64, Phase) + 'static) -> El {
    let ui = &s.ui;
    let o = om();
    let key = spec.key.to_owned();
    let pressed = s.edit.inspector.pressed.as_deref() == Some(spec.key);
    let hot = is_hot(s, spec.key) || pressed;
    let range = (spec.to - spec.from).max(1e-9);
    let t = (((spec.value - spec.from) / range).clamp(0.0, 1.0)) as f32;
    let shown = format!("{:.*}{}", spec.decimals, spec.value * spec.display_scale, spec.suffix);
    let enabled = spec.enabled;
    let (from, to, step) = (spec.from, spec.to, spec.step);
    let track_h = if hot && enabled { 5.0 } else { 4.0 };
    let radius = if o.rounded { track_h / 2.0 } else { 0.0 };
    let knob_scale = if pressed { 1.2 } else if hot { 1.1 } else { 1.0 };
    // The track spans the panel's width; the knob's centre runs 7 points in from each end.
    let bar = el()
        .w_full()
        .h(track_h)
        .radius(radius)
        .fill(ui.foreground.alpha(0.12))
        .child(el().key(format!("{key}-fill")).h_full().w_frac(t).radius(radius).fill(if enabled { ui.accent } else { ui.text_faint }));
    let knob_color = if enabled { ui.accent } else { ui.text_faint };
    let mut knob = el()
        .key(format!("{key}-knob"))
        .size(14.0, 14.0)
        .radius(7.0)
        .fill(if enabled { ui.text_strong } else { ui.text_faint })
        .border(3.0, knob_color)
        .scale(knob_scale);
    if pressed {
        knob = knob.child(el().absolute(-6.0, -6.0).size(26.0, 26.0).radius(13.0).fill(ui.accent.alpha(0.22)).inert());
    }
    let knob_row = row()
        .absolute(0.0, 4.0)
        .w_full()
        .px(7.0)
        .child(row().grow(1.0).child(el().w_frac(t)).child(knob.ml(-7.0)));
    let mut slider = el()
        .key(format!("{key}-track"))
        .role("slider")
        .label(spec.label.to_owned())
        .range_value(spec.value, spec.from, spec.to)
        .w_full()
        .h(22.0)
        .center_y()
        .disabled(!enabled)
        .child(bar)
        .child(knob_row);
    if enabled {
        let k = key.clone();
        slider = track_hot(
            slider.cursor(Cursor::Pointer).on_drag(move |s: &mut Preview, drag, tx: &mut Tx| {
                let usable = (drag.w - 14.0).max(1.0);
                let mut v = from + f64::from(((drag.x - 7.0) / usable).clamp(0.0, 1.0)) * (to - from);
                if step > 0.0 {
                    v = from + ((v - from) / step).round() * step;
                    v = v.clamp(from.min(to), from.max(to));
                }
                match drag.phase {
                    Phase::Down => {
                        s.edit.inspector.pressed = Some(k.clone());
                        tx.animated();
                    }
                    Phase::Up => s.edit.inspector.pressed = None,
                    _ => {}
                }
                apply(s, v, drag.phase);
                tx.wake_after(Duration::ZERO);
            }),
            &key,
        );
    }
    col()
        .key(format!("slider-{key}"))
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .items_center()
                .child(panel_label(ui, spec.label, enabled).grow(1.0).min_w(0.0))
                .child(text(shown).font(mono(o.small, 400.0)).color(if pressed { ui.accent } else { ui.text_muted })),
        )
        .child(slider)
}

// ---------------------------------------------------------------------------------------------
// Text fields

/// OmTextField: a single-line field over `edit`, reached through `access`.
pub fn text_field(
    ui: &Ui,
    key: &str,
    edit: &rune::TextEdit,
    placeholder: &str,
    access: impl for<'a> Fn(&'a mut Preview) -> &'a mut rune::TextEdit + 'static,
    submit: impl Fn(&mut Preview, &mut Tx) + 'static,
) -> El {
    let o = om();
    let focused = edit.focused;
    rune::text_input(edit, font(o.body, 400.0))
        .key(key.to_owned())
        .single_line()
        .placeholder(placeholder.to_owned(), ui.text_faint)
        .color(ui.text_strong)
        .caret(ui.accent, ui.accent.alpha(0.35))
        .on_edit(access)
        .on_submit(submit)
        .w_full()
        .px(o.pad_x)
        .py(((o.control_h + 2.0 - o.body * 1.3) / 2.0).max(4.0))
        .radius(o.control_radius)
        .fill(if focused { ui.canvas } else { ui.fill })
        .hover(if focused { Color::TRANSPARENT } else { ui.fill_hover })
        .border(1.0, if focused { ui.accent } else { ui.stroke })
}
