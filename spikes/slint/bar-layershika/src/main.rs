//! The recording bar as a wlr-layer-shell surface through layer-shika 0.3.1: a pill anchored to
//! the top of the screen on the overlay layer, with the elapsed time and a stop button.
//!
//! layer-shika loads the .slint file at run time through slint-interpreter, so properties and
//! callbacks are set by name with dynamic values rather than through generated Rust types.

use layer_shika::calloop::TimeoutAction;
use layer_shika::prelude::*;
use layer_shika::slint::ComponentHandle;
use layer_shika::slint_interpreter::Value;
use std::time::{Duration, Instant};

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/ui/bar.slint");
    let mut shell = Shell::from_file(path)
        .surface("RecordingBar")
        .size(220, 52)
        .anchor(AnchorEdges::empty().with_top())
        .margin((12, 0, 0, 0))
        .layer(Layer::Overlay)
        .exclusive_zone(0)
        .namespace("omareel-recording-bar")
        .build()?;
    shell.select(Surface::named("RecordingBar")).on_callback("stop", |_ctx| {
        println!("stop clicked");
        if std::env::var_os("BAR_KEEP").is_none() {
            std::process::exit(0);
        }
    });
    let bar = shell.with_surface("RecordingBar", |component| component.clone_strong())?;
    let started = Instant::now();
    shell.event_loop_handle().add_timer(Duration::from_millis(0), move |_, _| {
        let seconds = started.elapsed().as_secs();
        let _ = bar.set_property("elapsed", Value::String(format!("{:02}:{:02}", seconds / 60, seconds % 60).into()));
        let _ = bar.set_property("blink", Value::Bool(started.elapsed().as_millis() / 500 % 2 == 0));
        TimeoutAction::ToDuration(Duration::from_millis(250))
    })?;
    shell.run()?;
    Ok(())
}
