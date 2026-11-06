//! A test helper for the nested compositor: presses, drags and releases the left button through
//! wlr-virtual-pointer (Hyprland has no click dispatcher; place the cursor with
//! `hyprctl dispatch movecursor` first).
//!
//!   spike-pointer click
//!   spike-pointer drag DX STEPS      press, move DX pixels right in STEPS steps, release

use std::time::Duration;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

#[derive(Default)]
struct State {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ZwlrVirtualPointerManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global { name, interface, .. } = event {
            match interface.as_str() {
                "wl_seat" => state.seat = Some(registry.bind(name, 1, qh, ())),
                "zwlr_virtual_pointer_manager_v1" => state.manager = Some(registry.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

delegate_noop!(State: ignore wl_seat::WlSeat);
delegate_noop!(State: ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ZwlrVirtualPointerV1);

const BTN_LEFT: u32 = 0x110;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let connection = Connection::connect_to_env().expect("wayland display");
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    connection.display().get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state).expect("roundtrip");
    let manager = state.manager.as_ref().expect("zwlr_virtual_pointer_manager_v1");
    let pointer = manager.create_virtual_pointer(state.seat.as_ref(), &qh, ());
    let mut time = 1u32;
    let flush = |queue: &mut wayland_client::EventQueue<State>, state: &mut State| {
        pointer.frame();
        queue.roundtrip(state).expect("roundtrip");
    };
    // Nudge first so the compositor gives the client under the cursor pointer focus.
    pointer.motion(time, 1.0, 0.0);
    flush(&mut queue, &mut state);
    pointer.motion(time, -1.0, 0.0);
    flush(&mut queue, &mut state);
    std::thread::sleep(Duration::from_millis(50));
    time += 50;
    pointer.button(time, BTN_LEFT, wayland_client::protocol::wl_pointer::ButtonState::Pressed);
    flush(&mut queue, &mut state);
    if args.first().map(String::as_str) == Some("drag") {
        let dx: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100.0);
        let steps: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        for _ in 0..steps {
            std::thread::sleep(Duration::from_millis(16));
            time += 16;
            pointer.motion(time, dx / steps as f64, 0.0);
            flush(&mut queue, &mut state);
        }
    }
    std::thread::sleep(Duration::from_millis(50));
    time += 50;
    pointer.button(time, BTN_LEFT, wayland_client::protocol::wl_pointer::ButtonState::Released);
    flush(&mut queue, &mut state);
    pointer.destroy();
    queue.roundtrip(&mut state).ok();
}
