//! The launcher (`omareel` with no arguments): Launcher.qml and `crates/omareel_qt/src/launcher.rs`
//! in Rune.
//!
//! - `model`: the bridge (`launcher` in QML): preferences, devices, the self-view host, `omareel
//!   record` and the recording's state.
//! - `v4l2`, `audio`: what `LauncherBase` took from Qt Multimedia (cameras, the microphone meter,
//!   device changes).
//! - `view`: Launcher.qml.
//!
//! The window is 380 points wide and as tall as its content (Launcher.qml pins both): a fixed
//! width is how it asks Hyprland to float it. Rune flags (`--snap`, `--script`, `--tree`,
//! `--scene`) follow `--`: `omareel-rune-spike -- --snap launcher.png --scene recording`.

mod audio;
mod fit;
mod model;
mod v4l2;
mod view;

use crate::theme::Ui;
use model::Model;
use rune::{App, AppSpec, Key, KeyEvent, Tx, WindowOptions};
use std::time::Duration;

/// Which device menu is open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceMenu {
    Microphone,
    Webcam,
}

/// The type scale and radii of omareel's theme (`theme.type.*`, `theme.cardRadius`, ...).
#[derive(Clone, Copy)]
pub struct Metrics {
    pub caption: f32,
    pub small: f32,
    pub body: f32,
    pub title: f32,
    pub hero: f32,
    pub card_radius: f32,
    pub control_radius: f32,
    pub small_radius: f32,
    pub rounded: bool,
    pub dark: bool,
}

impl Metrics {
    fn load() -> Metrics {
        let theme = omareel_core::theme::Theme::new(None);
        let size = |name: &str| theme.palette.type_sizes.iter().find(|(n, _)| *n == name).map_or(12.0, |(_, v)| *v as f32);
        let radii = theme.radii();
        Metrics {
            caption: size("caption"),
            small: size("small"),
            body: size("body"),
            title: size("title"),
            hero: size("hero"),
            card_radius: radii.card as f32,
            control_radius: radii.control as f32,
            small_radius: radii.small as f32,
            rounded: theme.rounded(),
            dark: theme.palette.dark,
        }
    }
}

pub struct Launcher {
    pub ui: Ui,
    pub kit: rune_kit::Kit,
    pub m: Metrics,
    pub model: Model,
    /// The poll clock, in seconds.
    pub now: f64,
    /// The record button has had its first frame: its entrance scale springs from .85 to 1.
    pub entered: bool,
    /// The pointer is over the record control (`recordButton.hot`).
    pub hot: bool,
    pub settings_open: bool,
    pub menu: Option<DeviceMenu>,
    /// Discard asks again for three seconds after the first press: when it was armed.
    pub discard_armed: Option<f64>,
    /// The error toast and when it showed (`errorMessage`, cleared after five seconds).
    pub error: Option<(String, f64)>,
    /// The last error shown, kept so the toast slides out with its text.
    pub last_error: String,
    /// `screenshotRecording`: a snapshot shows the recording state with 00:12.
    pub screenshot_recording: bool,
    /// Ctrl+Shift+R turns the reel once each time.
    pub reel_turns: u32,
    /// The content's laid-out height, and the window height last asked of Hyprland.
    pub content_height: f32,
    window_height: i32,
}

impl Launcher {
    pub fn showing_recording(&self) -> bool {
        self.model.recording || self.screenshot_recording
    }

    pub fn busy(&self) -> bool {
        self.showing_recording() || self.model.starting_recording
    }

    /// `beginRecording`. A headless run shows the start and goes no further (the model runs
    /// nothing there).
    pub fn begin_recording(&mut self) {
        if !self.model.recording {
            let now = self.now;
            self.model.record(now);
        }
    }
}

fn poll(s: &mut Launcher, t: f64, tx: &mut Tx) -> bool {
    s.now = t;
    let mut changed = false;
    if !s.entered {
        s.entered = true;
        changed = true;
    }
    s.model.tick(t);
    for message in std::mem::take(&mut s.model.errors) {
        s.last_error = message.clone();
        s.error = Some((message, t));
        changed = true;
    }
    if s.error.as_ref().is_some_and(|(_, at)| t - at >= 5.0) {
        s.error = None;
        changed = true;
    }
    if s.discard_armed.is_some_and(|at| t - at >= 3.0) {
        s.discard_armed = None;
        changed = true;
    }
    // Hyprland keeps a floating window at its size: ask it for the content's height.
    let height = s.content_height.ceil() as i32;
    if s.model.live && height > 0 && height != s.window_height {
        s.window_height = height;
        fit::resize_window(view::WIDTH as i32, height);
    }
    if s.model.quit_requested {
        tx.quit();
    }
    // The QTimers' 50 ms.
    tx.wake_after(Duration::from_millis(50));
    std::mem::take(&mut s.model.dirty) || changed
}

fn on_key(s: &mut Launcher, key: &KeyEvent, _tx: &mut Tx) -> bool {
    match &key.key {
        // Ctrl+Shift+R spins the reel.
        Key::Command('r') if key.mods.shift => {
            s.reel_turns += 1;
            true
        }
        Key::Command('r') if !s.busy() => {
            s.begin_recording();
            true
        }
        Key::Enter if !s.busy() && !s.settings_open && s.menu.is_none() => {
            s.begin_recording();
            true
        }
        _ => false,
    }
}

/// Launcher.qml's `prepareScreenshot` views, as Rune scenes (`--scene <name>`).
const SCENES: &[(&str, fn(&mut Launcher))] = &[
    ("recording", |s| {
        s.screenshot_recording = true;
    }),
    ("settings", |s| s.settings_open = true),
    ("webcam-on", |s| s.model.scene_webcam()),
    ("webcam-menu", |s| {
        s.model.scene_webcam();
        s.menu = Some(DeviceMenu::Webcam);
    }),
    ("microphone-menu", |s| {
        s.model.microphone = true;
        s.menu = Some(DeviceMenu::Microphone);
    }),
    ("starting", |s| s.model.starting_recording = true),
    ("error", |s| {
        s.last_error = "Could not start recording".into();
        s.error = Some((s.last_error.clone(), 0.0));
    }),
    ("note", |s| {
        s.model.capture_exclusion_note = omareel_record::launcher::capture_exclusion_note(3).into();
        s.model.capture_exclusion_reason = "Capture exclusion plugin is not installed or was not built (it needs Hyprland's headers)".into();
    }),
];

pub fn run(rune_args: Vec<String>) -> Result<(), String> {
    let spec = AppSpec {
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: SCENES,
        families: ("Inter", "ui-monospace"),
        min_size: Some((view::WIDTH, 0.0)),
        ..AppSpec::new("Omareel", (view::WIDTH, 520.0), || unreachable!("App::init supplies the state"), view::view)
    };
    // `app.setDesktopFileName("omareel")`.
    rune::set_app_id("omareel");
    App::new(spec)
        .args(rune_args)
        // A fixed width floats it in Hyprland. The height follows the content (see `fit`); a
        // window that can't be resized would keep the height it opened at.
        .window_options(WindowOptions {
            min_size: Some((view::WIDTH, 0.0)),
            max_size: Some((view::WIDTH, 4000.0)),
            resizable: true,
            ..Default::default()
        })
        .fit_content_height()
        .init(|| {
            let ui = Ui::load();
            let kit = crate::kit::rune_kit(&ui);
            Launcher {
                kit,
                m: Metrics::load(),
                model: Model::new(!rune::is_headless()),
                ui,
                now: 0.0,
                entered: false,
                hot: false,
                settings_open: false,
                menu: None,
                discard_armed: None,
                error: None,
                last_error: String::new(),
                screenshot_recording: false,
                reel_turns: 0,
                content_height: 0.0,
                window_height: 0,
            }
        })
        .run();
    Ok(())
}
