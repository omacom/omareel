//! Launcher.qml: the header with the settings button, the record button (or the recording's
//! time with Stop and Discard), the capture exclusion note, the sources card (system audio,
//! microphone with its meter and device menu, webcam with its device menu, status, rotate and
//! mirror), the footer (open a recording, show the recordings folder), the capture settings
//! popover and the error toast. Sizes and colours follow the QML and its Om* components.

use super::{DeviceMenu, Launcher};
use crate::icons;
use crate::kit::{label, mono};
use rune::{Color, Cursor, Cx, El, Icon, LoopSpec, Spring, Tx, col, el, row, text};
use rune_kit::{Menu, MenuItem, Placement};

type S = Launcher;

pub const WIDTH: f32 = 380.0;

/// omareel's reel (`assets/icons/reel.svg`), which isn't a Lucide icon.
static REEL: Icon = Icon {
    name: "omareel-reel",
    view: 24.0,
    body: r#"<circle cx="11" cy="11" r="8.6" fill="none" stroke="currentColor" stroke-width="2.6"/><circle cx="11" cy="11" r="2.1" fill="currentColor"/><path d="M11 8.9V4.2M8.9 12.1 4.6 14.6M13.1 12.1l4.3 2.5" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/><path d="M16.2 17.6c1.6 1.9 3.6 3 6.3 3" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"/>"#,
};

/// `Qt.application.version`: the project version CMake builds omareel with.
fn version() -> &'static str {
    let cmake = include_str!("../../../../CMakeLists.txt");
    cmake
        .lines()
        .find_map(|line| line.trim().strip_prefix("project(omareel VERSION "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or("")
}

fn icon(i: &'static Icon, size: f32, color: Color) -> El {
    rune::icon(i, size).color(color)
}

fn lucide(name: &str) -> &'static Icon {
    icons::named(name).unwrap_or(&icons::CIRCLE)
}

pub fn view(s: &S, _: &mut Cx<S>) -> El {
    let ui = &s.ui;
    let mut layer = Vec::new();
    let content = col()
        .key("content")
        .w_full()
        .pl(20.0)
        .pr(20.0)
        .pt(18.0)
        .pb(16.0)
        .child(header(s, &mut layer))
        .child(el().h(18.0))
        .child(hero(s))
        .child(el().h(22.0))
        .children((!s.model.capture_exclusion_note.is_empty()).then(|| note(s)))
        .child(sources(s, &mut layer))
        .child(el().h(18.0))
        .child(footer(s))
        .on_layout(|s: &mut S, r: [f32; 4]| s.content_height = r[3]);
    el().key("launcher").w_full().fill(ui.surface).child(content).children(layer).child(toast(s))
}

// ---------------------------------------------------------------------------------------------
// Header

fn header(s: &S, layer: &mut Vec<El>) -> El {
    let ui = &s.ui;
    let reel = el()
        .key("reel")
        .size(18.0, 18.0)
        .center()
        .rotate(s.reel_turns as f32 * std::f32::consts::TAU)
        .rotate_motion(Spring::new(0.5, 0.0))
        .child(icon(&REEL, 18.0, ui.text_strong));
    let settings = icon_button(s, "settings-button", lucide("settings-2"), "Settings", s.settings_open, 32.0, 16.0, |s: &mut S, _| {
        s.settings_open = !s.settings_open;
    })
    .help("Capture settings");
    if s.settings_open {
        layer.push(settings_popover(s));
    }
    row()
        .key("header")
        .w_full()
        .items_center()
        .gap(8.0)
        .child(reel)
        .child(text("omareel").font(mono(s.m.body, 700.0)).color(ui.text_strong).grow(1.0))
        .child(settings)
}

// ---------------------------------------------------------------------------------------------
// The record button, or the recording

fn hero(s: &S) -> El {
    let body = if s.showing_recording() { recording_panel(s) } else { record_control(s) };
    el().key("hero").w_full().h(176.0).center().child(body)
}

fn record_control(s: &S) -> El {
    let ui = &s.ui;
    let starting = s.model.starting_recording;
    let hot = s.hot && !starting;
    let glow = if hot { 0.55 } else if starting { 0.4 } else { 0.18 };
    let dot = if starting { 20.0 } else if hot { 24.0 } else { 22.0 };
    let ring = if hot { 84.0 } else { 78.0 };
    let centered = |e: El| el().absolute(0.0, 0.0).size(96.0, 96.0).center().child(e);
    let mut disc = el()
        .key("record-disc")
        .size(64.0, 64.0)
        .radius(32.0)
        .center()
        .fill(ui.record)
        .press(Color::BLACK.alpha(0.1))
        // The soft glow that blooms on hover.
        .shadow(0.0, 44.0, ui.record.alpha(glow))
        .child(el().absolute(0.0, 0.0).size(64.0, 64.0).radius(32.0).linear_gradient(std::f32::consts::PI, [(0.0, Color::WHITE.alpha(0.22)), (0.55, Color::WHITE.alpha(0.0))]))
        // The dot becomes a stop square while the recorder spins up.
        .child(
            el().key("record-dot")
                .size(dot, dot)
                .radius(if starting { 4.0 } else { dot / 2.0 })
                .fill(Color::WHITE)
                .motion(Spring::new(0.26, 0.35)),
        );
    if starting {
        disc = disc.looping(LoopSpec::opacity(0.96, 0.0, 0.55, 1.0));
    }
    let button = el()
        .key("record-button")
        .size(96.0, 96.0)
        .scale(if s.entered { 1.0 } else { 0.85 })
        .scale_motion(Spring::new(0.32, 0.35))
        .press_scale(0.94)
        .child(centered(el().key("record-ring").size(ring, ring).radius(ring / 2.0).border(2.0, ui.record.alpha(if hot { 0.55 } else { 0.28 })).motion(Spring::new(0.26, 0.0))))
        .child(centered(disc));
    let button = if starting { button } else { button.on_press(|s: &mut S| s.begin_recording()) };
    let mut control = col()
        .key("record")
        .role("button")
        .label("Record")
        .size(260.0, 172.0)
        .items_center()
        .justify_center()
        .on_hover_change(|s: &mut S, over, _: &mut Tx| s.hot = over)
        .child(button)
        .child(text(if starting { "Starting…" } else { "Start recording" }).font(label(s.m.title, 600.0)).color(ui.text_strong).mt(6.0))
        .child(text("Drag an area, click a window, or the desktop").font(label(s.m.small, 400.0)).color(ui.text_muted).mt(3.0))
        .child(row().mt(10.0).gap(3.0).child(keycap(s, "Ctrl", !hot)).child(keycap(s, "R", !hot)));
    if starting {
        control = control.disabled(true);
    } else {
        control = control.cursor(Cursor::Pointer).on_press(|s: &mut S| s.begin_recording());
    }
    control
}

/// OmKeycap: the Omarchy monospace on a raised key with a deeper bottom edge.
fn keycap(s: &S, key: &str, subtle: bool) -> El {
    let ui = &s.ui;
    let radius = s.m.small_radius.min(4.0);
    el().h(20.0)
        .min_w(20.0)
        .px(5.0)
        .center()
        .radius(radius)
        .fill(if subtle { ui.fill } else { ui.raised })
        .border(1.0, ui.stroke_strong)
        .child(el().absolute(1.0, 18.0).h(1.0).w_full().fill(Color::BLACK.alpha(if s.m.dark { 0.35 } else { 0.08 })))
        .child(text(key.to_owned()).font(mono(s.m.caption, 500.0)).color(if subtle { ui.text_muted } else { ui.text_strong }))
}

fn recording_panel(s: &S) -> El {
    let ui = &s.ui;
    let stopping = s.model.stopping_recording;
    let armed = s.discard_armed.is_some();
    let elapsed = if s.screenshot_recording { "00:12".to_owned() } else { s.model.recording_elapsed.clone() };
    let stop = om_button(
        s,
        "stop",
        if stopping { "Saving…" } else { "Stop" },
        Some(lucide("square")),
        Style { primary: true, destructive: true, bordered: false, enabled: !stopping },
        |s: &mut S| {
            if s.model.live {
                s.model.stop_recording();
            }
        },
    )
    .size(112.0, 34.0);
    let discard = om_button(
        s,
        "discard",
        if armed { "Discard?" } else { "Discard" },
        Some(lucide("trash-2")),
        Style { primary: false, destructive: armed, bordered: true, enabled: !stopping },
        |s: &mut S| {
            if s.discard_armed.is_some() {
                if s.model.live {
                    s.model.cancel_recording();
                }
            } else {
                s.discard_armed = Some(s.now);
            }
        },
    )
    .size(112.0, 34.0);
    col()
        .key("recording")
        .items_center()
        .gap(14.0)
        .child(
            row()
                .items_center()
                .gap(8.0)
                .child(el().key("recording-dot").size(8.0, 8.0).radius(4.0).fill(ui.record).looping(LoopSpec::opacity(1.6, 0.0, 0.25, 1.0)))
                .child(text("Recording").font(label(s.m.small, 500.0)).color(ui.text_muted)),
        )
        .child(text(elapsed).key("elapsed").font(mono(s.m.hero + 8.0, 500.0)).color(ui.text_strong))
        .child(row().mt(4.0).gap(8.0).child(stop).child(discard))
}

// ---------------------------------------------------------------------------------------------
// The capture exclusion note

fn note(s: &S) -> El {
    let mut note = text(s.model.capture_exclusion_note.clone()).key("exclusion-note").font(label(s.m.caption, 400.0)).color(s.ui.text_muted).wrap();
    if !s.model.capture_exclusion_reason.is_empty() {
        note = note.help(s.model.capture_exclusion_reason.clone());
    }
    el().w_full().mb(8.0).child(note)
}

// ---------------------------------------------------------------------------------------------
// The sources card

/// `deviceName`: the listed name of `value`, else the first's, else `fallback`.
fn device_name(devices: &[(String, String)], value: &str, fallback: &str) -> String {
    devices.iter().find(|(v, _)| v == value).or(devices.first()).map_or_else(|| fallback.to_owned(), |(_, t)| t.clone())
}

struct SourceRow {
    key: &'static str,
    title: &'static str,
    icon_on: &'static str,
    icon_off: &'static str,
    device: String,
    menu: Option<DeviceMenu>,
    checked: bool,
    enabled: bool,
    /// The meter beside the switch, 0 to 1; None hides it.
    level: Option<f32>,
    toggle: fn(&mut S),
}

fn source_row(s: &S, r: SourceRow) -> El {
    let ui = &s.ui;
    let checked = r.checked;
    let mut device_line = None;
    if !r.device.is_empty() {
        let mut line = row()
            .key(format!("{}-device", r.key))
            .items_center()
            .gap(3.0)
            .max_w(WIDTH - 140.0)
            .child(text(r.device.clone()).font(label(s.m.caption, 400.0)).color(ui.text_muted).group_color(ui.foreground).truncate().shrink());
        if let Some(menu) = r.menu {
            line = line
                .hover_group()
                .role("button")
                .label(format!("{} device", r.title))
                .cursor(Cursor::Pointer)
                .child(icon(lucide("chevron-down"), 12.0, ui.text_muted).group_color(ui.foreground));
            if r.enabled {
                line = line.on_press(move |s: &mut S| s.menu = Some(menu));
            }
        }
        device_line = Some(line);
    }
    let labels = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(1.0)
        .child(text(r.title).font(label(s.m.body, 500.0)).color(if checked { ui.text_strong } else { ui.foreground }))
        .children(device_line);
    let badge = el()
        .size(30.0, 30.0)
        .radius(s.m.control_radius)
        .center()
        .fill(if checked { ui.accent.alpha(0.16) } else { ui.fill })
        .child(icon(lucide(if checked { r.icon_on } else { r.icon_off }), 16.0, if checked { ui.accent } else { ui.text_muted }));
    let meter = r.level.map(|level| {
        let radius = if s.m.rounded { 2.0 } else { 0.0 };
        el().key(format!("{}-level", r.key))
            .size(28.0, 4.0)
            .radius(radius)
            .fill(ui.fill)
            .child(el().h(4.0).w_frac(level.clamp(0.0, 1.0)).radius(radius).fill(ui.accent))
            // The level moves 20 times a second, as Qt's meter: each read lands at once.
            .instant_changes()
    });
    let mut inner = row()
        .key(r.key)
        .role("switch")
        .label(r.title)
        .checked(checked)
        .w_full()
        .h_full()
        .px(12.0)
        .gap(12.0)
        .items_center()
        .radius((s.m.card_radius - 2.0).max(0.0))
        .child(badge)
        .child(labels)
        .children(meter)
        .child(switch(s, checked));
    if r.enabled {
        let toggle = r.toggle;
        inner = inner.hover(ui.fill).cursor(Cursor::Pointer).focusable(true).on_press(move |s: &mut S| toggle(s));
    } else {
        inner = inner.disabled(true);
    }
    el().key(format!("{}-row", r.key)).w_full().h(52.0).padding(2.0).opacity(if r.enabled { 1.0 } else { 0.5 }).child(inner)
}

/// OmToggle's switch, drawn only: the row owns the state.
fn switch(s: &S, on: bool) -> El {
    let ui = &s.ui;
    let radius = |h: f32| if s.m.rounded { h / 2.0 } else { 0.0 };
    let knob = if on || !s.m.dark { Color::WHITE } else { ui.text_strong };
    el().size(34.0, 20.0)
        .radius(radius(20.0))
        .fill(if on { ui.accent } else { ui.foreground.alpha(0.14) })
        .child(
            el().absolute(if on { 17.0 } else { 3.0 }, 3.0)
                .size(14.0, 14.0)
                .radius(radius(14.0))
                .fill(knob)
                .motion(Spring::new(0.26, 0.3)),
        )
}

fn separator(s: &S) -> El {
    el().w_full().pl(56.0).child(el().w_full().h(1.0).fill(s.ui.separator))
}

fn sources(s: &S, layer: &mut Vec<El>) -> El {
    let ui = &s.ui;
    let model = &s.model;
    let busy = s.busy();
    let audio: Vec<(String, String)> = model.audio_devices.iter().map(|d| (d.value.clone(), d.text.clone())).collect();
    let cameras: Vec<(String, String)> = model.webcam_devices.iter().map(|d| (d.value.clone(), d.text.clone())).collect();
    let system = source_row(
        s,
        SourceRow {
            key: "system-audio",
            title: "System audio",
            icon_on: "volume-2",
            icon_off: "volume-x",
            device: String::new(),
            menu: None,
            checked: model.system_audio,
            enabled: !busy,
            level: None,
            toggle: |s| {
                let on = !s.model.system_audio;
                s.model.set_system_audio(on);
            },
        },
    );
    let microphone = source_row(
        s,
        SourceRow {
            key: "microphone",
            title: "Microphone",
            icon_on: "mic",
            icon_off: "mic-off",
            device: device_name(&audio, &model.microphone_device, "Default input"),
            menu: (!audio.is_empty()).then_some(DeviceMenu::Microphone),
            checked: model.microphone,
            enabled: !busy,
            level: (model.microphone && !busy).then(|| model.microphone_level() as f32),
            toggle: |s| {
                let on = !s.model.microphone;
                s.model.set_microphone(on);
            },
        },
    );
    let webcam = source_row(
        s,
        SourceRow {
            key: "webcam",
            title: "Webcam",
            icon_on: "video",
            icon_off: "video-off",
            device: if cameras.is_empty() { "No camera found".into() } else { device_name(&cameras, &model.webcam_device, "Camera") },
            menu: (!cameras.is_empty()).then_some(DeviceMenu::Webcam),
            checked: model.webcam,
            enabled: !busy && (model.webcam || !cameras.is_empty()),
            level: None,
            toggle: |s| {
                let on = !s.model.webcam;
                s.model.set_webcam(on);
            },
        },
    );
    let microphone = device_menu(s, "microphone-menu", microphone, s.menu == Some(DeviceMenu::Microphone), &audio, &model.microphone_device, layer, |s, v| {
        s.model.set_microphone_device(v);
        s.model.set_microphone(true);
    });
    let webcam = device_menu(s, "webcam-menu", webcam, s.menu == Some(DeviceMenu::Webcam), &cameras, &model.webcam_device, layer, |s, v| {
        s.model.set_webcam_device(v);
        s.model.set_webcam(true);
    });
    let camera_tools = model.webcam && !s.showing_recording();
    let tools = row()
        .key("webcam-tools")
        .w_full()
        .h(if camera_tools { 40.0 } else { 0.0 })
        .opacity(if camera_tools { 1.0 } else { 0.0 })
        .clip_rounded(0.0)
        .pl(58.0)
        .pr(12.0)
        .pb(8.0)
        .gap(4.0)
        .items_center()
        .child(text(model.self_view_status.clone()).key("self-view-status").font(label(s.m.caption, 400.0)).color(ui.text_muted).truncate().grow(1.0).min_w(0.0))
        .child(
            icon_button(s, "rotate-camera", lucide("rotate-cw"), "Rotate camera", false, 26.0, 14.0, |s: &mut S, _| {
                let rotation = (s.model.webcam_rotation + 90) % 360;
                s.model.set_webcam_rotation(rotation);
            })
            .help("Rotate camera"),
        )
        .child(
            icon_button(s, "mirror-camera", lucide("flip-horizontal-2"), "Mirror camera", model.webcam_flip_horizontal, 26.0, 14.0, |s: &mut S, _| {
                let flip = !s.model.webcam_flip_horizontal;
                s.model.set_webcam_flip_horizontal(flip);
            })
            .help("Mirror camera"),
        );
    let tools = if camera_tools { tools } else { tools.inert() };
    col()
        .key("sources")
        .w_full()
        .padding(2.0)
        .radius(s.m.card_radius)
        .fill(ui.card)
        .border(1.0, ui.stroke)
        .opacity(if s.showing_recording() { 0.5 } else { 1.0 })
        .child(system)
        .child(separator(s))
        .child(microphone)
        .child(separator(s))
        .child(webcam)
        .child(tools)
}

/// OmMenu with a checkable OmMenuItem per device, as wide as its row, which is its trigger: it
/// drops below the row, or opens upward when the window has no room below.
#[allow(clippy::too_many_arguments)]
fn device_menu(s: &S, key: &str, row: El, open: bool, devices: &[(String, String)], current: &str, layer: &mut Vec<El>, pick: fn(&mut S, &str)) -> El {
    let mut menu = Menu::new(key.to_owned())
        .trigger(row)
        .open(open)
        .standard()
        .placement(Placement::BottomStart)
        .gutter(-6.0)
        .min_width(WIDTH - 44.0)
        .on_close(|s: &mut S| s.menu = None);
    for (value, name) in devices {
        let value = value.clone();
        menu = menu.item(MenuItem::new(name.clone()).checked(value == current).on_select(move |s: &mut S| {
            s.menu = None;
            pick(s, &value);
        }));
    }
    menu.build(&s.kit, layer).w_full()
}

// ---------------------------------------------------------------------------------------------
// Footer

fn footer(s: &S) -> El {
    let ui = &s.ui;
    let open = om_button(s, "open-recording", "Open recording…", Some(lucide("folder-open")), Style::default(), |_: &mut S| {})
        .on_press_tx(|_: &mut S, tx: &mut Tx| {
            let options = rune::FileDialogOptions { title: "Open an omareel bundle".into(), ..Default::default() };
            tx.open_folder(options, |s: &mut S, result, _| match result {
                Ok(Some(path)) => s.model.open_bundle(&path.to_string_lossy()),
                Ok(None) => {}
                Err(error) => {
                    s.model.errors.push(format!("Could not open the folder dialog: {error}"));
                }
            });
        });
    let folder = el()
        .key("recordings-folder")
        .role("link")
        .label("Show recordings")
        .hover_group()
        .cursor(Cursor::Pointer)
        .py(6.0)
        .pl(6.0)
        .mt(-6.0)
        .mb(-6.0)
        .on_press(|s: &mut S| s.model.show_recordings_folder())
        .help("Show recordings")
        .child(text("~/Videos/omareel").font(mono(s.m.caption, 400.0)).color(ui.text_faint).group_color(ui.accent));
    row().key("footer").w_full().items_center().gap(6.0).child(open).child(el().grow(1.0)).child(folder)
}

// ---------------------------------------------------------------------------------------------
// Capture settings popover

fn settings_popover(s: &S) -> El {
    let ui = &s.ui;
    let model = &s.model;
    let size_enabled = model.self_view_enabled;
    let sizes = ["S", "M", "L"].iter().fold(row().key("self-view-size").w(120.0).h(28.0).padding(2.0).radius(s.m.control_radius).fill(ui.fill), |group, &size| {
        let on = model.self_view_size == size;
        let mut segment = el()
            .key(format!("self-view-size-{size}"))
            .role("radio")
            .label(size)
            .checked(on)
            .grow(1.0)
            .h_full()
            .center()
            .radius((s.m.control_radius - 2.0).max(0.0))
            .fill(if on { ui.raised } else { Color::TRANSPARENT })
            .child(text(size).font(label(s.m.body, 500.0)).color(if on { ui.text_strong } else { ui.text_muted }));
        if size_enabled {
            segment = segment.cursor(Cursor::Pointer).hover(ui.fill_hover).on_press(move |s: &mut S| s.model.set_self_view_size(size));
        }
        group.child(segment)
    });
    let body = col()
        .key("settings")
        .role("dialog")
        .label("Capture settings")
        .w(300.0)
        .padding(16.0)
        .gap(10.0)
        .radius(s.m.card_radius)
        .fill(ui.popup_background)
        .border(1.0, ui.stroke_strong)
        .shadow(8.0, 28.0, ui.shadow)
        .child(
            col()
                .gap(2.0)
                .child(text("Capture settings").font(label(s.m.title, 600.0)).color(ui.text_strong))
                .child(text("Defaults for new recordings").font(label(s.m.small, 400.0)).color(ui.text_muted)),
        )
        .child(el().w_full().h(1.0).fill(ui.separator))
        .child(toggle(s, "countdown", "Countdown", "Three seconds to get ready", model.countdown_before_recording, |s| {
            let on = !s.model.countdown_before_recording;
            s.model.set_countdown_before_recording(on);
        }))
        .child(toggle(s, "self-view", "Camera self-view", "Floating preview of your webcam", model.self_view_enabled, |s| {
            let on = !s.model.self_view_enabled;
            s.model.set_self_view_enabled(on);
        }))
        .child(
            row()
                .w_full()
                .items_center()
                .gap(8.0)
                .opacity(if size_enabled { 1.0 } else { 0.5 })
                .child(text("Self-view size").font(label(s.m.body, 400.0)).color(ui.foreground).grow(1.0))
                .child(sizes),
        )
        .child(el().w_full().h(1.0).fill(ui.separator))
        .child(
            row().w_full().justify_center().child(
                text(format!("omareel {}  ·  made for Omarchy", version())).font(mono(s.m.caption, 400.0)).color(ui.text_faint),
            ),
        )
        .modal(true, |s: &mut S| s.settings_open = false);
    let scrim = el().key("settings-scrim").absolute(0.0, 0.0).w_full().h_full().on_press(|s: &mut S| s.settings_open = false);
    el().key("settings-layer")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .child(scrim)
        .child(el().absolute(WIDTH - 300.0 - 16.0, 52.0).child(body).transition(rune::Transition::opacity()))
}

/// OmToggle: a name and description on the left, the switch on the right; the row toggles.
fn toggle(s: &S, key: &str, name: &str, description: &str, on: bool, toggle: fn(&mut S)) -> El {
    let ui = &s.ui;
    row()
        .key(key.to_owned())
        .role("switch")
        .label(name.to_owned())
        .checked(on)
        .focusable(true)
        .cursor(Cursor::Pointer)
        .w_full()
        .gap(8.0)
        .items_center()
        .on_press(move |s: &mut S| toggle(s))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(name.to_owned()).font(label(s.m.body, 400.0)).color(ui.foreground))
                .child(text(description.to_owned()).font(label(s.m.caption, 400.0)).color(ui.text_muted).wrap()),
        )
        .child(switch(s, on))
}

// ---------------------------------------------------------------------------------------------
// The error toast

fn toast(s: &S) -> El {
    let ui = &s.ui;
    let shown = s.error.is_some();
    let message = s.error.as_ref().map_or(s.last_error.clone(), |(m, _)| m.clone());
    let pill = row()
        .key("error-toast")
        .role("status")
        .label(message.clone())
        .h(36.0)
        .max_w(WIDTH - 32.0)
        .px(14.0)
        .items_center()
        .radius(s.m.control_radius)
        .fill(ui.text_strong)
        .offset(0.0, if shown { 0.0 } else { 52.0 })
        .opacity(if shown { 1.0 } else { 0.0 })
        .motion(Spring::new(0.26, 0.3))
        .child(text(message).font(label(s.m.small, 500.0)).color(ui.surface).truncate());
    col().key("toast-layer").absolute(0.0, 0.0).w_full().h_full().justify_end().items_center().pb(14.0).clip_rounded(0.0).inert().child(pill)
}

// ---------------------------------------------------------------------------------------------
// Buttons

#[derive(Clone, Copy)]
struct Style {
    primary: bool,
    destructive: bool,
    bordered: bool,
    enabled: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style { primary: false, destructive: false, bordered: false, enabled: true }
    }
}

/// OmButton: primary (accent, or record red when destructive) with a sheen, bordered, or
/// quiet; a leading icon.
fn om_button(s: &S, key: &str, name: &str, i: Option<&'static Icon>, style: Style, press: impl Fn(&mut S) + 'static) -> El {
    let ui = &s.ui;
    let base = if style.destructive { ui.record } else { ui.accent };
    let fg = if !style.enabled {
        ui.text_faint
    } else if style.primary {
        if style.destructive { Color::WHITE } else { ui.accent_foreground }
    } else if style.destructive {
        ui.record
    } else {
        ui.foreground
    };
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .h(28.0)
        .px(12.0)
        .gap(6.0)
        .center()
        .radius(s.m.control_radius)
        .opacity(if style.enabled { 1.0 } else { 0.55 });
    b = if style.primary {
        b.fill(base)
            .linear_gradient(std::f32::consts::PI, [(0.0, Color::WHITE.alpha(0.14)), (0.5, Color::WHITE.alpha(0.0))])
            .hover(Color::WHITE.alpha(0.08))
            .press(Color::BLACK.alpha(0.12))
    } else if style.destructive {
        b.fill(if style.bordered { ui.fill } else { Color::TRANSPARENT }).hover(ui.record.alpha(0.14)).press(ui.record.alpha(0.22))
    } else {
        b.fill(if style.bordered { ui.fill } else { Color::TRANSPARENT }).hover(ui.fill_hover).press(ui.fill_pressed)
    };
    if style.bordered && !style.primary {
        b = b.border(1.0, ui.stroke);
    }
    if style.enabled {
        b = b.cursor(Cursor::Pointer).focusable(true).press_scale(0.97).on_press(press);
    } else {
        b = b.disabled(true);
    }
    if let Some(i) = i {
        b = b.child(icon(i, 15.0, fg));
    }
    b.child(text(name.to_owned()).font(label(s.m.body, 500.0)).color(fg))
}

/// OmIconButton: a square icon button, `selected` filled.
#[allow(clippy::too_many_arguments)]
fn icon_button(s: &S, key: &str, i: &'static Icon, name: &str, selected: bool, size: f32, icon_size: f32, press: impl Fn(&mut S, &mut Tx) + 'static) -> El {
    let ui = &s.ui;
    el().key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .size(size, size)
        .radius(s.m.control_radius)
        .center()
        .fill(if selected { ui.fill_pressed } else { Color::TRANSPARENT })
        .hover(ui.fill_hover)
        .press(ui.fill_pressed)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .on_press_tx(press)
        .child(icon(i, icon_size, if selected { ui.text_strong } else { ui.foreground }))
}
