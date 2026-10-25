//! The recording bar as a wlr-layer-shell surface, with the compiled `RecordingBar` component.
//!
//! Slint's winit backend cannot make layer surfaces (winit has no wlr-layer-shell), so this is a
//! small custom Slint platform (the public `slint::platform` API): smithay-client-toolkit makes
//! the layer surface on the overlay layer, anchored to the top, and Slint's software renderer
//! draws the component into a wl_shm buffer; pointer events go to the Slint window. A 200x44
//! pill needs no GPU. (layer-shika, the community crate for this, works too, but only with
//! .slint files compiled at run time by the interpreter; see bar-layershika/.)

use crate::RecordingBar;
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType, TargetPixel};
use slint::platform::{Platform, PlatformError, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::SlotPool;
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_pointer, delegate_registry, delegate_seat,
    delegate_shm, registry_handlers,
};
use std::os::fd::AsRawFd;
use std::rc::Rc;
use std::time::{Duration, Instant};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, QueueHandle};

const WIDTH: u32 = 220;
const HEIGHT: u32 = 52;

/// wl_shm's ARGB8888: premultiplied, little-endian B, G, R, A.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Argb8888 {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

impl TargetPixel for Argb8888 {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let keep = u16::from(u8::MAX - color.alpha);
        self.r = (u16::from(self.r) * keep / 255) as u8 + color.red;
        self.g = (u16::from(self.g) * keep / 255) as u8 + color.green;
        self.b = (u16::from(self.b) * keep / 255) as u8 + color.blue;
        self.a = (u16::from(self.a) * keep / 255) as u8 + color.alpha;
    }
    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Argb8888 { r, g, b, a: 255 }
    }
    fn background() -> Self {
        Argb8888::default()
    }
}

struct BarPlatform {
    window: Rc<MinimalSoftwareWindow>,
    started: Instant,
}

impl Platform for BarPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.started.elapsed()
    }
}

struct Bar {
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    shm: Shm,
    pool: SlotPool,
    layer: LayerSurface,
    pointer: Option<wl_pointer::WlPointer>,
    window: Rc<MinimalSoftwareWindow>,
    configured: bool,
    frame_pending: bool,
}

impl Bar {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let stride = WIDTH as i32 * 4;
        let Ok((buffer, canvas)) = self.pool.create_buffer(WIDTH as i32, HEIGHT as i32, stride, wl_shm::Format::Argb8888) else {
            return;
        };
        let pixels: &mut [Argb8888] =
            // SAFETY: Argb8888 is four u8s with alignment 1, and the canvas holds WIDTH x HEIGHT of them.
            unsafe { std::slice::from_raw_parts_mut(canvas.as_mut_ptr().cast(), canvas.len() / 4) };
        let drew = self.window.draw_if_needed(|renderer| {
            pixels.fill(Argb8888::default());
            renderer.render(pixels, WIDTH as usize);
        });
        if !drew {
            return;
        }
        let surface = self.layer.wl_surface();
        surface.damage_buffer(0, 0, WIDTH as i32, HEIGHT as i32);
        surface.frame(qh, surface.clone());
        self.frame_pending = true;
        if buffer.attach_to(surface).is_ok() {
            self.layer.commit();
        }
    }
}

pub fn run() -> Result<(), String> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(WIDTH, HEIGHT));
    slint::platform::set_platform(Box::new(BarPlatform { window: window.clone(), started: Instant::now() }))
        .map_err(|e| format!("Slint platform: {e}"))?;
    let ui = RecordingBar::new().map_err(|e| e.to_string())?;
    if let Some(family) = omareel_core::theme::monospace_family() {
        ui.global::<crate::Comp>().set_mono_family(family.into());
    }
    ui.on_stop(|| {
        println!("stop clicked");
        if std::env::var_os("BAR_KEEP").is_none() {
            std::process::exit(0);
        }
    });
    ui.show().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let tick_ui = ui.as_weak();
    let ticker = slint::Timer::default();
    ticker.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
        if let Some(ui) = tick_ui.upgrade() {
            let seconds = started.elapsed().as_secs();
            ui.set_elapsed(format!("{:02}:{:02}", seconds / 60, seconds % 60).into());
            ui.set_blink(started.elapsed().as_millis() / 500 % 2 == 0);
        }
    });

    let connection = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let (globals, mut queue) = registry_queue_init(&connection).map_err(|e| format!("Wayland: {e}"))?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| format!("wlr-layer-shell: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let surface = compositor.create_surface(&qh);
    let layer = layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("omareel-recording-bar"), None);
    layer.set_anchor(Anchor::TOP);
    layer.set_margin(12, 0, 0, 0);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_exclusive_zone(0);
    layer.set_size(WIDTH, HEIGHT);
    layer.commit();
    let pool = SlotPool::new((WIDTH * HEIGHT * 4 * 2) as usize, &shm).map_err(|e| format!("shm pool: {e}"))?;
    let mut bar = Bar {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        shm,
        pool,
        layer,
        pointer: None,
        window,
        configured: false,
        frame_pending: false,
    };

    loop {
        slint::platform::update_timers_and_animations();
        bar.draw(&qh);
        queue.flush().map_err(|e| format!("Wayland: {e}"))?;
        // Sleep until Wayland has something or Slint's next timer or animation frame is due.
        let timeout = if bar.window.has_active_animations() {
            Duration::from_millis(16)
        } else {
            slint::platform::duration_until_next_timer_update().unwrap_or(Duration::from_secs(1))
        };
        if let Some(guard) = queue.prepare_read() {
            let mut fd = libc::pollfd { fd: guard.connection_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: one valid pollfd.
            let ready = unsafe { libc::poll(&mut fd, 1, timeout.as_millis().min(1000) as i32) };
            if ready > 0 {
                let _ = guard.read();
            }
        }
        queue.dispatch_pending(&mut bar).map_err(|e| format!("Wayland: {e}"))?;
    }
}

impl CompositorHandler for Bar {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Bar {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Bar {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        std::process::exit(0);
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface, _: LayerSurfaceConfigure, _: u32) {
        self.configured = true;
        self.window.request_redraw();
        self.draw(qh);
    }
}

impl SeatHandler for Bar {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer {
            if let Some(pointer) = self.pointer.take() {
                pointer.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for Bar {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if &event.surface != self.layer.wl_surface() {
                continue;
            }
            let position = LogicalPosition::new(event.position.0 as f32, event.position.1 as f32);
            let window = self.window.window();
            match event.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    let _ = window.dispatch_event_with_result(WindowEvent::PointerMoved { position });
                }
                PointerEventKind::Leave { .. } => {
                    let _ = window.dispatch_event_with_result(WindowEvent::PointerExited);
                }
                PointerEventKind::Press { button, .. } | PointerEventKind::Release { button, .. } => {
                    let button = match button {
                        0x110 => PointerEventButton::Left,
                        0x111 => PointerEventButton::Right,
                        0x112 => PointerEventButton::Middle,
                        _ => PointerEventButton::Other,
                    };
                    let _ = window.dispatch_event_with_result(if matches!(event.kind, PointerEventKind::Press { .. }) {
                        WindowEvent::PointerPressed { position, button }
                    } else {
                        WindowEvent::PointerReleased { position, button }
                    });
                }
                PointerEventKind::Axis { .. } => {}
            }
        }
    }
}

impl ShmHandler for Bar {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Bar {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_compositor!(Bar);
delegate_output!(Bar);
delegate_shm!(Bar);
delegate_seat!(Bar);
delegate_pointer!(Bar);
delegate_layer!(Bar);
delegate_registry!(Bar);
