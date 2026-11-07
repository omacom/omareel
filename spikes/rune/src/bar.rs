//! The recording bar: a pill with a pulsing dot, the elapsed time and a Stop button, as a
//! wlr-layer-shell surface on the overlay layer anchored to the top of the screen (what main.cpp
//! asks of LayerShellQt). Rune has this built in: `App::overlay` with `interactive: true`.
//!
//! The app has no window of its own (`windowless`); the overlay keeps it running and Stop quits.

use crate::theme::Ui;
use rune::tok::{Family, TextStyle};
use rune::{Align, App, AppSpec, Cursor, Cx, El, LiveText, LoopSpec, OverlaySpec, ScreenEdge, Tx, el, row, text};
use std::time::{Duration, Instant};

struct Bar {
    /// `Tx::quit` from an overlay's press handler is not acted on (Rune's overlay host only reads
    /// shell requests on focus changes), so Stop quits through the app's handle.
    app: rune::AppHandle<()>,
    ui: Ui,
    started: Instant,
    clock: LiveText,
}

fn view(s: &Bar, _: &mut Cx<Bar>) -> El {
    let ui = &s.ui;
    let mono = TextStyle { family: Family::SystemMono, ..TextStyle::new(14.0, 600.0) };
    let dot = el().key("dot").size(10.0, 10.0).radius(5.0).fill(ui.record).looping(LoopSpec::opacity(1.6, 0.0, 0.35, 1.0));
    let stop = row()
        .key("stop")
        .role("button")
        .label("Stop recording")
        .h(32.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .radius(16.0)
        .fill(ui.record.alpha(0.16))
        .hover(ui.record.alpha(0.28))
        .press(ui.record.alpha(0.4))
        .cursor(Cursor::Pointer)
        .on_press_tx(|s: &mut Bar, tx: &mut Tx| {
            println!("stop clicked");
            tx.quit();
            s.app.quit();
        })
        .child(el().size(10.0, 10.0).radius(2.0).fill(ui.record))
        .child(text("Stop").font(TextStyle::new(13.0, 600.0)).color(ui.text_strong));
    row().key("bar").w_full().h_full().center().child(
        row()
            .key("pill")
            .h(48.0)
            .pl(16.0)
            .pr(8.0)
            .gap(12.0)
            .items_center()
            .radius(24.0)
            .fill(ui.surface.alpha(0.96))
            .border(1.0, ui.hairline)
            .child(dot)
            .child(text("00:00").live_text(&s.clock).font(mono).color(ui.text_strong))
            .child(stop),
    )
}

fn poll(s: &mut Bar, _: f64, tx: &mut Tx) -> bool {
    let seconds = s.started.elapsed().as_secs();
    s.clock.set(format!("{:02}:{:02}", seconds / 60, seconds % 60));
    tx.wake_after(Duration::from_millis(1000 - (s.started.elapsed().subsec_millis() as u64).min(999)));
    false
}

pub fn run() -> Result<(), String> {
    let spec = AppSpec::new("omareel recording bar", (10.0, 10.0), || (), |_: &(), _| el());
    let mut app = App::new(spec).windowless();
    let handle = app.handle();
    let bar = app.overlay(
        rune::ConfiguredOverlay::from(OverlaySpec {
            name: "omareel-recording-bar",
            size: (220.0, 52.0),
            edge: ScreenEdge::Top,
            margin: 12.0,
            align: Align::Center,
            inset: 0.0,
            interactive: true,
            view,
        })
        .poll(poll),
        Bar { app: handle, ui: Ui::load(), started: Instant::now(), clock: LiveText::new("00:00") },
    );
    bar.show();
    app.run();
    Ok(())
}
