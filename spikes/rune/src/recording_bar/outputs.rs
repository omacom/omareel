//! Which monitors exist, for placing the bar's layer surfaces: what `QGuiApplication::screens()`
//! gave main.cpp. A Rune overlay targets an output by its `wl_output` registry name
//! (`DisplayTarget::Native(NativeDisplayId::Wayland(..))`), so this keeps its own Wayland
//! connection with every `wl_output` bound (version 4, for the connector name Hyprland calls the
//! monitor by) and follows outputs coming and going. Logical sizes, which keep the self-view on
//! its screen and centre the countdown, come from `hyprctl -j monitors` (fractional scales).

use omareel_core::compat::qjson::{self, Value};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};

struct Output {
    global: u32,
    _output: wl_output::WlOutput,
    name: Option<String>,
}

#[derive(Default)]
struct Registry {
    outputs: Vec<Output>,
    changed: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Registry {
    fn event(state: &mut Self, registry: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, qh: &QueueHandle<Self>) {
        match event {
            wl_registry::Event::Global { name, interface, version } if interface == "wl_output" && version >= 4 => {
                let output = registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, name);
                state.outputs.push(Output { global: name, _output: output, name: None });
                state.changed = true;
            }
            wl_registry::Event::GlobalRemove { name } => {
                let before = state.outputs.len();
                state.outputs.retain(|o| o.global != name);
                state.changed |= state.outputs.len() != before;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, u32> for Registry {
    fn event(state: &mut Self, _: &wl_output::WlOutput, event: wl_output::Event, global: &u32, _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = event
            && let Some(output) = state.outputs.iter_mut().find(|o| o.global == *global)
        {
            output.name = Some(name);
            state.changed = true;
        }
    }
}

/// The compositor's outputs by name, kept current by [`Outputs::refresh`].
pub struct Outputs {
    queue: Option<(Connection, EventQueue<Registry>)>,
    registry: Registry,
    /// Logical origins and sizes from hyprctl, by monitor name.
    sizes: Vec<(String, Layout)>,
}

impl Outputs {
    pub fn new() -> Outputs {
        let queue = Connection::connect_to_env().ok().map(|connection| {
            let queue = connection.new_event_queue();
            connection.display().get_registry(&queue.handle(), ());
            (connection, queue)
        });
        let mut outputs = Outputs { queue, registry: Registry::default(), sizes: Vec::new() };
        outputs.refresh();
        outputs
    }

    /// Reads what changed since the last call (outputs plugged or unplugged); true when
    /// something did, which also re-reads the logical sizes.
    pub fn refresh(&mut self) -> bool {
        if let Some((_, queue)) = &mut self.queue {
            // Two round trips: the globals, then the names of the outputs just bound.
            let _ = queue.roundtrip(&mut self.registry);
            if self.registry.changed {
                let _ = queue.roundtrip(&mut self.registry);
            }
        }
        let changed = std::mem::take(&mut self.registry.changed);
        if changed || self.sizes.is_empty() {
            self.sizes = logical_sizes();
        }
        changed
    }

    /// The `wl_output` registry name of the monitor called `name`, if it is connected.
    pub fn global(&self, name: &str) -> Option<u32> {
        if name.is_empty() {
            return None;
        }
        self.registry.outputs.iter().find(|o| o.name.as_deref() == Some(name)).map(|o| o.global)
    }

    /// The monitor's logical size (what `QScreen::geometry().size()` is), if it is connected.
    pub fn logical_size(&self, name: &str) -> Option<(i32, i32)> {
        self.global(name)?;
        self.sizes.iter().find(|(n, _)| n == name).map(|(_, layout)| layout.size)
    }

    /// The monitor's top-left in the compositor's logical layout (where `cursorpos` counts from).
    pub fn origin(&self, name: &str) -> Option<(i32, i32)> {
        self.global(name)?;
        self.sizes.iter().find(|(n, _)| n == name).map(|(_, layout)| layout.origin)
    }
}

#[derive(Clone, Copy)]
struct Layout {
    origin: (i32, i32),
    size: (i32, i32),
}

/// Every monitor's place and logical size: its mode divided by its scale, turned with its
/// transform.
fn logical_sizes() -> Vec<(String, Layout)> {
    let Some(done) = omareel_core::compat::qprocess::run("hyprctl", &["-j", "monitors"], 1000) else { return Vec::new() };
    let Ok(Value::Array(monitors)) = qjson::from_json(&done.stdout) else { return Vec::new() };
    monitors
        .iter()
        .map(|value| {
            let m = value.to_object();
            let scale = m.value("scale").to_double(1.0).max(0.01);
            let (w, h) = (m.value("width").to_double(0.0) / scale, m.value("height").to_double(0.0) / scale);
            let turned = m.value("transform").to_int(0) % 2 == 1;
            let (w, h) = if turned { (h, w) } else { (w, h) };
            let origin = (m.value("x").to_int(0), m.value("y").to_int(0));
            (m.value("name").to_string_or(""), Layout { origin, size: (w.round() as i32, h.round() as i32) })
        })
        .collect()
}
