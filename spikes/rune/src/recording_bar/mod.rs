//! The recording bar process (`__record-bar`): RecordingBar.qml, its bridge
//! (`crates/omareel_qt/src/recording_bar.rs` with `RecordingBarBase`), the `__record-bar` branch
//! of main.cpp and the camera recording (`src/plumbing/CameraCapture.cpp`) in Rune.
//!
//! Three layer-shell overlays on the overlay layer, as main.cpp makes them with LayerShellQt:
//! the bar (top edge, 12 points down, centred; namespace `omareel-record-bar`), the countdown
//! (centred; same namespace) and, with the webcam, the self-view bubble (top-left anchored,
//! placed by its margins; `omareel-selfview`). The recorder looks for `omareel-` namespaces
//! when it checks that overlays left the recorded output.
//!
//!   __record-bar [--hidden]                    a recording's bar (the recorder starts it)
//!   __record-bar --standby --owner <pid>       the launcher's self-view host, which a recording
//!                                              adopts (`omareel_record::recorder::state`)
//!
//! `controller.rs` is the bar's logic, on its own thread; `view.rs` the three views;
//! `camera.rs` the camera; `outputs.rs` finds the monitors.

mod camera;
mod controller;
mod outputs;
mod view;

use controller::{COUNTDOWN_SIZE, Controller, Windows, bar_size};
use rune::{Align, App, AppSpec, ConfiguredOverlay, OverlaySpec, ScreenEdge, el};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use view::{BarView, BubbleView, CountdownView, Style};

/// main.cpp's `configureOverlayRule`: Hyprland's layer animation off for the namespace (named
/// Lua rules merge effects, so the previous one is retired first).
fn configure_overlay_rule(scope: &str) {
    let expression = format!(
        "hl.layer_rule({{ name = '{scope}-private', enabled = false }}); \
         hl.layer_rule({{ name = '{scope}-display', match = {{ namespace = '{scope}' }}, no_anim = true }})"
    );
    let done = omareel_core::compat::qprocess::run("hyprctl", &["eval", &expression], 3000);
    if !done.is_some_and(|d| d.succeeded()) {
        eprintln!("omareel: could not apply the overlay animation rule");
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    // The recorder starts the bar with no terminal: `OMAREEL_RECORDING_BAR_LOG=<file>` keeps
    // what it prints (the camera's lines, `OMAREEL_SELFVIEW_DRAG_LOG`'s).
    if let Some(path) = std::env::var_os("OMAREEL_RECORDING_BAR_LOG")
        && let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(path)
    {
        use std::os::fd::AsRawFd;
        // SAFETY: both descriptors are open; stderr now writes to the file.
        unsafe { libc::dup2(file.as_raw_fd(), 2) };
    }
    // `omareel __record-bar --hidden` (the flag right after the command, as main.cpp reads it).
    let hidden = args.first().is_some_and(|a| a == "--hidden");
    let standby = args.iter().any(|a| a == "--standby");
    let owner = args.iter().position(|a| a == "--owner").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0);

    let (send, receive) = std::sync::mpsc::channel();
    let controller = Controller::new(hidden, standby, owner, send.clone());
    if controller.finished() || (standby && !controller.owns_standby_host()) {
        return Ok(());
    }
    let webcam = controller.webcam();
    let style = Style::load();

    let spec = AppSpec::new("omareel recording bar", (10.0, 10.0), || (), |_: &(), _| el());
    let mut app = App::new(spec).windowless();
    let handle = app.handle();

    configure_overlay_rule("omareel-record-bar");
    let bar = app.overlay(
        ConfiguredOverlay::from(OverlaySpec {
            name: "omareel-record-bar",
            size: bar_size(webcam),
            edge: ScreenEdge::Top,
            margin: 12.0,
            align: Align::Center,
            inset: 0.0,
            interactive: true,
            view: view::bar,
        })
        .poll(view::bar_poll),
        BarView::new(style.clone(), send.clone()),
    );
    // The countdown takes no clicks (the QML window has nothing to click).
    let countdown = app.overlay(
        ConfiguredOverlay::from(OverlaySpec {
            name: "omareel-record-bar",
            size: COUNTDOWN_SIZE,
            edge: ScreenEdge::Top,
            margin: 0.0,
            align: Align::Center,
            inset: 0.0,
            interactive: false,
            view: view::countdown,
        })
        .poll(view::countdown_poll),
        CountdownView::new(style.clone()),
    );
    let bubble = webcam.then(|| {
        configure_overlay_rule("omareel-selfview");
        app.overlay(
            ConfiguredOverlay::from(OverlaySpec {
                name: "omareel-selfview",
                size: (160.0, 160.0),
                edge: ScreenEdge::Top,
                margin: 16.0,
                align: Align::Start,
                inset: 16.0,
                interactive: true,
                view: view::bubble,
            }),
            BubbleView::new(style.clone(), controller.preview.clone(), send.clone()),
        )
    });
    if !controller.standby() {
        controller.publish_bar_geometry();
        controller.publish_countdown_geometry();
    }
    drop(send);

    let quit = Arc::new(AtomicBool::new(false));
    let windows = Windows { app: handle, bar, countdown, bubble };
    let thread_quit = quit.clone();
    let worker = std::thread::Builder::new()
        .name("recording-bar".into())
        .spawn(move || controller.run(receive, windows, thread_quit))
        .map_err(|e| e.to_string())?;
    app.run();
    quit.store(true, Ordering::SeqCst);
    let _ = worker.join();
    Ok(())
}
