//! Clicks in the nested compositor through wlr-virtual-pointer, for testing the bar's Stop button
//! and the preview's controls: `spike-pointer <x> <y> [click|move] [<width> <height>]`.

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_pointer, wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::virtual_pointer::v1::client::{zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1};

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
        _: zwlr_virtual_pointer_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
        _: zwlr_virtual_pointer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn millis() -> u32 {
    (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() & 0xffff_ffff) as u32
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let number = |i: usize, default: u32| args.get(i).and_then(|v| v.parse().ok()).unwrap_or(default);
    let (x, y) = (number(1, 0), number(2, 0));
    let click = args.get(3).is_none_or(|a| a == "click");
    let (width, height) = (number(4, 1920), number(5, 1080));
    let connection = Connection::connect_to_env().expect("Wayland");
    let (globals, mut queue) = registry_queue_init::<State>(&connection).expect("registry");
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).expect("wl_seat");
    let manager: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1 =
        globals.bind(&qh, 1..=2, ()).expect("zwlr_virtual_pointer_manager_v1");
    let pointer = manager.create_virtual_pointer(Some(&seat), &qh, ());
    pointer.motion_absolute(millis(), x, y, width, height);
    pointer.frame();
    queue.roundtrip(&mut State).expect("roundtrip");
    std::thread::sleep(std::time::Duration::from_millis(60));
    if click {
        pointer.button(millis(), 0x110, wl_pointer::ButtonState::Pressed);
        pointer.frame();
        queue.roundtrip(&mut State).expect("roundtrip");
        std::thread::sleep(std::time::Duration::from_millis(60));
        pointer.button(millis(), 0x110, wl_pointer::ButtonState::Released);
        pointer.frame();
    }
    queue.roundtrip(&mut State).expect("roundtrip");
    pointer.destroy();
    queue.roundtrip(&mut State).expect("roundtrip");
}
