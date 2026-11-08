//! RecordingBar.qml: the bar's pill, the countdown dial and the self-view bubble, each the view
//! of a Rune overlay with its own state. The controller (`controller.rs`) feeds them models;
//! what the person does goes back to it as `Command`s. Timed motion (the bar's entrance, the
//! dial's sweep and the digit's pop) runs from each overlay's poll at frame rate while it plays.

use super::controller::{self, Command};
use crate::icons;
use crate::kit::{icon, label, mono};
use crate::theme::Ui;
use rune::{Color, Cursor, Cx, El, Live, LoopSpec, Phase, Pixels, Shape, Spring, SurfaceFit, Tx, canvas, el, row, text};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// The theme the QML reads: the palette, the type scale and whether corners are rounded.
#[derive(Clone)]
pub struct Style {
    pub ui: Ui,
    pub body: f32,
    pub small: f32,
    pub caption: f32,
    pub rounded: bool,
    /// `Qt.lighter(theme.record, 1.08)` and `Qt.darker(theme.record, 1.12)`.
    pub record_hover: Color,
    pub record_down: Color,
}

impl Style {
    pub fn load() -> Style {
        let theme = omareel_core::theme::Theme::new(None);
        let size = |name: &str| theme.palette.type_sizes.iter().find(|(k, _)| *k == name).map_or(13, |(_, v)| *v) as f32;
        Style {
            ui: Ui::load(),
            body: size("body"),
            small: size("small"),
            caption: size("caption"),
            rounded: theme.rounded(),
            record_hover: crate::theme::color(&theme.palette.record.lighter(108)),
            record_down: crate::theme::color(&theme.palette.record.darker(112)),
        }
    }
}

/// Qt's `Easing.OutBack` with its overshoot.
fn out_back(t: f32, overshoot: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * ((overshoot + 1.0) * t + overshoot) + 1.0
}

/// Qt's `Easing.OutCubic`.
fn out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * t + 1.0
}

const FRAME: Duration = Duration::from_millis(16);

// ---------------------------------------------------------------------------------------------
// The bar.

/// What the bar shows: `recordingBar`'s properties the QML binds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BarModel {
    pub elapsed: String,
    pub webcam: bool,
    pub self_view_visible: bool,
    pub flip: bool,
    pub round: bool,
    pub stopping: bool,
}

pub struct BarView {
    style: Style,
    pub model: BarModel,
    send: Sender<Command>,
    /// When the bar was last shown: its entrance plays from then.
    pub entered: Option<Instant>,
    /// The discard button asks for a second press until three seconds pass.
    armed_at: Option<Instant>,
    hovered: Option<&'static str>,
    slide: Live<[f32; 2]>,
    fade: Live<f32>,
}

impl BarView {
    pub fn new(style: Style, send: Sender<Command>) -> BarView {
        BarView {
            style,
            model: BarModel { elapsed: "00:00".into(), ..BarModel::default() },
            send,
            entered: None,
            armed_at: None,
            hovered: None,
            slide: Live::new([0.0, -10.0]),
            fade: Live::new(0.0),
        }
    }
}

/// The bar's entrance (opacity over 200 ms, a 10-point drop with an overshoot over 380 ms) and
/// the discard confirmation's three seconds.
pub fn bar_poll(view: &mut BarView, _: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    if let Some(entered) = view.entered {
        let t = entered.elapsed().as_secs_f32();
        view.fade.set(out_cubic(t / 0.2));
        view.slide.set([0.0, -10.0 * (1.0 - out_back(t / 0.38, 1.4))]);
        if t < 0.38 {
            tx.wake_after(FRAME);
        } else {
            view.entered = None;
        }
    }
    if let Some(armed) = view.armed_at {
        let left = Duration::from_secs(3).saturating_sub(armed.elapsed());
        if left.is_zero() {
            view.armed_at = None;
            tx.animated();
            changed = true;
        } else {
            tx.wake_after(left);
        }
    }
    changed
}

/// A 28-point round icon button of the bar (`BarIcon`): accent while on, stronger on hover.
fn bar_icon(view: &BarView, key: &'static str, glyph: &'static rune::Icon, on: bool, name: &str, command: fn(&BarView) -> Command) -> El {
    let ui = &view.style.ui;
    let color = if on {
        ui.accent
    } else if view.hovered == Some(key) {
        ui.text_strong
    } else {
        ui.text_muted
    };
    el().key(key)
        .role("button")
        .label(name.to_owned())
        .size(28.0, 28.0)
        .radius(if view.style.rounded { 14.0 } else { 0.0 })
        .center()
        .hover(ui.fill_hover)
        .press(ui.fill_pressed)
        .press_scale(0.9)
        .cursor(Cursor::Pointer)
        .on_hover_change(move |v: &mut BarView, hovered, _| {
            v.hovered = if hovered { Some(key) } else { v.hovered.filter(|k| *k != key) };
        })
        .on_press_tx(move |v: &mut BarView, _| {
            let _ = v.send.send(command(v));
        })
        .child(icon(glyph, 15.0, color))
}

fn separator(ui: &Ui, left: f32, right: f32) -> El {
    el().size(1.0, 16.0).ml(left).mr(right).fill(ui.separator)
}

pub fn bar(view: &BarView, _: &mut Cx<BarView>) -> El {
    let style = &view.style;
    let ui = &style.ui;
    let m = &view.model;
    let pill_radius = if style.rounded { 20.0 } else { 0.0 };
    let button_radius = if style.rounded { 14.0 } else { 0.0 };
    let mut pill = row()
        .key("pill")
        .h(40.0)
        .px(8.0)
        .gap(4.0)
        .items_center()
        .radius(pill_radius)
        .fill(ui.surface)
        .border(1.0, ui.stroke_strong)
        .motion(Spring::new(0.26, 0.0))
        .live_offset(&view.slide)
        .live_opacity(&view.fade);
    // The pulsing record dot and the elapsed time.
    pill = pill
        .child(el().key("dot").size(8.0, 8.0).ml(6.0).radius(4.0).fill(ui.record).looping(LoopSpec::opacity(1.6, 0.0, 0.25, 1.0)))
        .child(text(m.elapsed.clone()).key("elapsed").ml(4.0).mr(6.0).font(mono(style.body, 500.0)).color(ui.text_strong));
    if m.webcam {
        pill = pill
            .child(separator(ui, 2.0, 2.0))
            .child(bar_icon(
                view,
                "selfview",
                if m.self_view_visible { &icons::EYE } else { &icons::EYE_OFF },
                m.self_view_visible,
                if m.self_view_visible { "Hide self-view" } else { "Show self-view" },
                |v| Command::SetSelfViewVisible(!v.model.self_view_visible),
            ))
            .child(bar_icon(view, "flip", &icons::FLIP_HORIZONTAL_2, m.flip, "Flip camera", |_| Command::Flip))
            .child(bar_icon(view, "rotate", &icons::ROTATE_CW, false, "Rotate camera", |_| Command::Rotate))
            .child(bar_icon(view, "shape", if m.round { &icons::CIRCLE } else { &icons::SQUARE }, false, "Camera shape", |_| Command::CycleShape))
            .child(bar_icon(view, "corner", &icons::SCAN, false, "Move self-view to the next corner", |_| Command::NextCorner));
    }
    pill = pill.child(separator(ui, 2.0, 4.0));

    // Stop: the recording is saved; "Saving…" until the bar goes.
    let stop = row()
        .key("stop")
        .role("button")
        .label("Stop recording")
        .h(28.0)
        .pl(10.0)
        .pr(12.0)
        .gap(7.0)
        .items_center()
        .radius(button_radius)
        .fill(ui.record)
        .hover(if m.stopping { ui.record } else { style.record_hover })
        .press(if m.stopping { ui.record } else { style.record_down })
        .press_scale(if m.stopping { 1.0 } else { 0.94 })
        .cursor(if m.stopping { Cursor::Default } else { Cursor::Pointer })
        .disabled(m.stopping)
        .on_press_tx(|v: &mut BarView, _| {
            if !v.model.stopping {
                let _ = v.send.send(Command::Stop);
            }
        })
        .child(el().size(9.0, 9.0).radius(if style.rounded { 2.0 } else { 0.0 }).fill(Color::WHITE))
        .child(text(if m.stopping { "Saving…" } else { "Stop" }).font(label(style.small, 600.0)).color(Color::WHITE));
    pill = pill.child(stop);

    // Discard: a first press asks "Discard?", a second within three seconds discards.
    let armed = view.armed_at.is_some();
    let hovered = view.hovered == Some("discard");
    let confirm_style = label(style.small, 600.0);
    let confirm_width = rune::text::measure("Discard?", &[], confirm_style, None).0.ceil();
    let width = if armed { confirm_width + 40.0 } else { 28.0 };
    let trash_x = if armed { 9.0 } else { (width - 15.0) / 2.0 };
    let trash_color = if armed {
        ui.record
    } else if hovered {
        ui.text_strong
    } else {
        ui.text_muted
    };
    let (fill, hover, press) = if armed {
        (ui.record.alpha(0.18), ui.record.alpha(0.26), ui.record.alpha(0.26))
    } else {
        (Color::TRANSPARENT, ui.fill_hover, ui.fill_pressed)
    };
    let discard = el()
        .key("discard")
        .role("button")
        .label(if armed { "Confirm discard recording" } else { "Discard recording" })
        .w(width)
        .h(28.0)
        .mr(2.0)
        .radius(button_radius)
        .clip_rounded(button_radius)
        .fill(fill)
        .hover(if m.stopping { fill } else { hover })
        .press(if m.stopping { fill } else { press })
        .cursor(if m.stopping { Cursor::Default } else { Cursor::Pointer })
        .disabled(m.stopping)
        .motion(Spring::new(0.24, 0.0))
        .on_hover_change(|v: &mut BarView, hovered, _| {
            v.hovered = if hovered { Some("discard") } else { v.hovered.filter(|k| *k != "discard") };
        })
        .on_press_tx(|v: &mut BarView, tx: &mut Tx| {
            if v.model.stopping {
                return;
            }
            if v.armed_at.is_some() {
                let _ = v.send.send(Command::Cancel);
            } else {
                v.armed_at = Some(Instant::now());
                tx.wake_after(Duration::from_secs(3));
            }
        })
        .child(icon(&icons::TRASH_2, 15.0, trash_color).key("trash").absolute(trash_x, 6.5))
        .child(
            text("Discard?")
                .key("confirm")
                .font(confirm_style)
                .color(ui.record)
                .absolute(trash_x + 15.0 + 6.0, 14.0 - confirm_style.size * 0.65)
                .opacity(if armed { 1.0 } else { 0.0 }),
        );
    pill = pill.child(discard);
    row().key("bar").w_full().h_full().center().child(pill)
}

// ---------------------------------------------------------------------------------------------
// The countdown.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CountdownModel {
    pub value: i32,
    /// Bumped on every `countdownChanged`, which restarts the sweep and the pop.
    pub generation: u64,
}

pub struct CountdownView {
    style: Style,
    model: CountdownModel,
    changed_at: Instant,
}

impl CountdownView {
    pub fn new(style: Style) -> CountdownView {
        CountdownView { style, model: CountdownModel { value: 3, generation: 0 }, changed_at: Instant::now() }
    }

    pub fn set_model(&mut self, model: CountdownModel) {
        if model.generation != self.model.generation {
            self.changed_at = Instant::now();
        }
        self.model = model;
    }
}

/// The sweep (one second) and the pop run at frame rate after each change.
pub fn countdown_poll(view: &mut CountdownView, _: f64, tx: &mut Tx) -> bool {
    if view.changed_at.elapsed() < Duration::from_millis(1000) {
        tx.wake_after(FRAME);
        tx.instant();
        return true;
    }
    false
}

/// The arc of a circle about (84, 84), radius 72, from `start` degrees through `sweep` degrees
/// clockwise (0 is three o'clock).
fn arc(start: f32, sweep: f32) -> Vec<[f32; 2]> {
    let steps = ((sweep.abs() / 3.0).ceil() as usize).max(2);
    (0..=steps)
        .map(|i| {
            let a = (start + sweep * i as f32 / steps as f32).to_radians();
            [84.0 + 72.0 * a.cos(), 84.0 + 72.0 * a.sin()]
        })
        .collect()
}

pub fn countdown(view: &CountdownView, _: &mut Cx<CountdownView>) -> El {
    let style = &view.style;
    let ui = &style.ui;
    let t = view.changed_at.elapsed().as_secs_f32();
    let sweep = (1.0 - t).clamp(0.0, 1.0);
    let mut shape = Shape::new().stroke_with(arc(0.0, 360.0), true, 4.0, rune::shape::LineCap::Butt, ui.foreground.alpha(0.12));
    if sweep > 0.0 {
        shape = shape.stroke(arc(-90.0, 360.0 * sweep), 4.0, ui.record);
    }
    let pop = 1.35 + (1.0 - 1.35) * out_back(t / 0.42, 1.8);
    let appear = (t / 0.2).clamp(0.0, 1.0);
    let number = el()
        .key("number")
        .absolute(0.0, 26.0)
        .size(168.0, 104.0)
        .center()
        .scale(pop)
        .opacity(appear)
        .instant_changes()
        .child(text(view.model.value.to_string()).font(mono(72.0, 500.0)).color(ui.text_strong));
    let dial = el()
        .key("dial")
        .size(168.0, 168.0)
        .radius(84.0)
        .fill(ui.surface.alpha(0.94))
        .border(1.0, ui.stroke_strong)
        .child(canvas(shape).key("rings").absolute(0.0, 0.0).size(168.0, 168.0))
        .child(number)
        .child(el().key("ready").absolute(0.0, 120.0).w(168.0).center().child(text("Get ready").font(label(style.small, 500.0)).color(ui.text_muted)));
    row().key("countdown").w_full().h_full().center().child(dial)
}

// ---------------------------------------------------------------------------------------------
// The self-view bubble.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BubbleModel {
    pub pixels: i32,
    pub visible: bool,
    pub round: bool,
    pub radius: i32,
    pub ready: bool,
    pub error: String,
}

pub struct BubbleView {
    style: Style,
    pub model: BubbleModel,
    preview: Pixels,
    send: Sender<Command>,
    hovered: bool,
    pressed: bool,
}

impl BubbleView {
    pub fn new(style: Style, preview: Pixels, send: Sender<Command>) -> BubbleView {
        BubbleView { style, model: BubbleModel { pixels: 160, ..BubbleModel::default() }, preview, send, hovered: false, pressed: false }
    }
}

pub fn bubble(view: &BubbleView, _: &mut Cx<BubbleView>) -> El {
    let style = &view.style;
    let ui = &style.ui;
    let m = &view.model;
    let side = m.pixels as f32;
    let inner = side - 10.0;
    let corner = if m.round { inner / 2.0 } else { (m.radius as f32).min(inner / 2.0) };
    let scale = if view.pressed {
        0.98
    } else if view.hovered {
        1.03
    } else {
        1.0
    };
    let mut clip = el()
        .key("camera")
        .size(inner, inner)
        .radius(corner)
        .clip_rounded(corner)
        .scale(scale)
        // From pressed it springs back (SpringAnimation spring 4, damping .35); else 120 ms.
        .scale_motion(if view.pressed || !view.hovered { Spring::new(0.3, 0.35) } else { Spring::new(0.12, 0.0) });
    if m.ready {
        clip = clip.child(el().key("picture").absolute(0.0, 0.0).size(inner, inner).pixels_fit(&view.preview, SurfaceFit::Fill));
    } else {
        clip = clip.child(el().key("placeholder").absolute(0.0, 0.0).size(inner, inner).fill(ui.surface.alpha(0.94)));
    }
    let (border, border_color) = if view.pressed { (2.0, ui.accent.alpha(0.7)) } else { (1.5, Color::WHITE.alpha(0.16)) };
    clip = clip.child(el().key("ring").absolute(0.0, 0.0).size(inner, inner).radius(corner).border(border, border_color));
    let caption = if !m.error.is_empty() {
        m.error.clone()
    } else if m.ready {
        String::new()
    } else {
        "Starting camera…".into()
    };
    let mut root = el()
        .key("bubble")
        .w_full()
        .h_full()
        .center()
        .opacity(if m.visible { 1.0 } else { 0.0 })
        .cursor(Cursor::Move)
        .on_hover_change(|v: &mut BubbleView, hovered, _| v.hovered = hovered)
        .on_drag(|v: &mut BubbleView, drag, tx| {
            let pointer = controller::global_cursor_pos();
            if std::env::var_os("OMAREEL_SELFVIEW_DRAG_LOG").is_some() {
                eprintln!("drag {:?} cancelled={} local=({},{}) global={pointer:?}", drag.phase, drag.cancelled(), drag.x, drag.y);
            }
            match drag.phase {
                Phase::Down => {
                    v.pressed = true;
                    let _ = v.send.send(Command::BeginDrag(f64::from(drag.start_x), f64::from(drag.start_y)));
                }
                Phase::Move => {
                    let _ = v.send.send(Command::DragTo(pointer));
                    tx.live_only();
                }
                Phase::Up => {
                    v.pressed = false;
                    if !drag.cancelled() {
                        let _ = v.send.send(Command::DragTo(pointer));
                    }
                    let _ = v.send.send(Command::EndDrag);
                }
            }
        })
        .child(clip);
    if !caption.is_empty() {
        root = root.child(
            el().key("caption").absolute(10.0, 0.0).size(side - 20.0, side).center().child(
                text(caption).w(side - 20.0).font(label(style.caption, 400.0)).color(ui.foreground).align(rune::Align::Center).wrap(),
            ),
        );
    }
    root
}
