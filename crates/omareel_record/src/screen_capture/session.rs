//! `ScreenCapture` (`src/record/ScreenCapture.cpp` at the tag): the in-process capture of one
//! output through `ext-image-copy-capture-v1`, on a Wayland connection of its own (not Qt's).
//!
//! `start` binds the globals, opens a capture session on the output and learns its buffer size,
//! shared-memory formats, and dmabuf device and formats. It then makes five frame-sized slots
//! and the encoder, in the first of these ways that works:
//!
//! - dmabufs on the compositor's GPU, encoded there by VA-API ([`super::gpu`]): the frames never
//!   leave the GPU. Not on NVIDIA, whose VA-API driver can't encode.
//! - linear dmabufs read back here and piped to ffmpeg (NVENC or libx264).
//! - one shared-memory pool piped to ffmpeg. Only when the compositor offers no usable dmabufs
//!   (or `OMAREEL_CAPTURE_BUFFERS=shm`): shared memory makes the compositor read every frame back
//!   from the GPU on its own render thread, which stalls the whole display.
//!
//! `capture_frame` is the daemon's loop body: at most one capture is asked for per 1/fps interval
//! (and only one frame object exists at a time, as the protocol requires); the connection is
//! waited on until an event arrives or the next capture is due; completed captures are queued for
//! the writer by presentation time (failed ones give their slot back); and once a second the
//! rate is printed. `finish` drains the writer into the encoder and waits for it; `abort` stops
//! both at once.

use super::encoder::{Encoder, EncoderExit, wait_for_encoder};
use super::gpu::{self, MappedSlots};
use super::writer::{run_writer, strerror};
use super::{
    CaptureCrop, EncoderProbeResults, FdSink, FrameQueue, RawSlotMemory, Rect, Size, WriterConfig,
    encoder_arguments_from_environment, encoder_probe_results, trim_capture_crop, writer_loop,
};
use crate::recorder::{debug_start_stage, monotonic_us, write_stderr};
use omareel_core::compat::qnumber;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Instant;
use wayland_client::backend::WaylandError;
use wayland_client::protocol::{wl_buffer, wl_output, wl_registry, wl_shm, wl_shm_pool};
use wayland_client::{Connection, Dispatch, DispatchError, EventQueue, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{self, ExtImageCopyCaptureManagerV1},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

/// `ScreenCaptureConfig`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenCaptureConfig {
    /// The output's name (`wl_output.name`).
    pub monitor: String,
    pub output_path: String,
    /// Where the first frame's presentation time goes.
    pub timestamp_path: String,
    /// The part of the output to encode; a rectangle that is not valid means all of it.
    pub crop: Rect,
    pub fps: i32,
}

/// One slot frames are captured into.
#[derive(Default)]
struct Slot {
    frame: Option<ExtImageCopyCaptureFrameV1>,
    buffer: Option<wl_buffer::WlBuffer>,
    ready: bool,
    failed: bool,
    presentation_us: i64,
}

/// An output as the registry announced it, with the name it sent.
struct Output {
    object: wl_output::WlOutput,
    name: String,
}

/// What the event handlers write: `RegistryState`'s globals and outputs, the session's
/// constraints, and the slots' frame events.
#[derive(Default)]
struct State {
    shm: Option<wl_shm::WlShm>,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    capture_manager: Option<ExtImageCopyCaptureManagerV1>,
    source_manager: Option<ExtOutputImageCaptureSourceManagerV1>,
    outputs: Vec<Output>,
    width: i32,
    height: i32,
    stride: i32,
    shm_formats: Vec<u32>,
    /// The device dmabufs must be allocated on (a `dev_t`), and each format with its modifiers.
    dmabuf_device: Option<u64>,
    dmabuf_formats: Vec<(u32, Vec<u64>)>,
    constraints_done: bool,
    stopped: bool,
    slots: Vec<Slot>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut State,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        queue: &QueueHandle<State>,
    ) {
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        match interface.as_str() {
            "wl_shm" => state.shm = Some(registry.bind(name, 1, queue, ())),
            // Version 2 brings create_immed; the format events of 3 and the feedback of 4 aren't
            // used (the capture session names its own formats).
            "zwp_linux_dmabuf_v1" if version >= 2 => state.dmabuf = Some(registry.bind(name, version.min(4), queue, ())),
            "ext_image_copy_capture_manager_v1" => state.capture_manager = Some(registry.bind(name, 1, queue, ())),
            "ext_output_image_capture_source_manager_v1" => {
                state.source_manager = Some(registry.bind(name, 1, queue, ()));
            }
            "wl_output" => {
                let object = registry.bind(name, version.min(4), queue, ());
                state.outputs.push(Output { object, name: String::new() });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        state: &mut State,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wl_output::Event::Name { name } = event {
            for entry in state.outputs.iter_mut().filter(|entry| entry.object == *output) {
                entry.name = name.clone();
            }
        }
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, ()> for State {
    fn event(
        state: &mut State,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        match event {
            Event::BufferSize { width, height } => {
                state.width = width as i32;
                state.height = height as i32;
                state.stride = state.width.wrapping_mul(4);
            }
            Event::ShmFormat { format } => state.shm_formats.push(match format {
                WEnum::Value(format) => format as u32,
                WEnum::Unknown(format) => format,
            }),
            Event::DmabufDevice { device } => {
                state.dmabuf_device = device.as_slice().try_into().ok().map(u64::from_ne_bytes);
            }
            Event::DmabufFormat { format, modifiers } => {
                let modifiers =
                    modifiers.chunks_exact(8).filter_map(|bytes| bytes.try_into().ok().map(u64::from_ne_bytes)).collect();
                state.dmabuf_formats.push((format, modifiers));
            }
            Event::Done => state.constraints_done = true,
            Event::Stopped => state.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, usize> for State {
    fn event(
        state: &mut State,
        frame: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        // libwayland drops the events of a destroyed frame; a slot only listens to its own.
        let Some(slot) = state.slots.get_mut(*index).filter(|slot| slot.frame.as_ref() == Some(frame)) else {
            return;
        };
        match event {
            Event::PresentationTime { tv_sec_hi, tv_sec_lo, tv_nsec } => {
                let seconds = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
                slot.presentation_us = seconds.wrapping_mul(1_000_000).wrapping_add(u64::from(tv_nsec / 1000)) as i64;
            }
            Event::Ready => slot.ready = true,
            Event::Failed { .. } => slot.failed = true,
            _ => {}
        }
    }
}

delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);
delegate_noop!(State: ignore ZwpLinuxDmabufV1);
delegate_noop!(State: ignore ZwpLinuxBufferParamsV1);
delegate_noop!(State: ExtImageCopyCaptureManagerV1);
delegate_noop!(State: ExtOutputImageCaptureSourceManagerV1);
delegate_noop!(State: ExtImageCaptureSourceV1);

/// The descriptor `WAYLAND_SOCKET` hands this process, taken by the first connection that asks
/// (made close-on-exec). libwayland also unsets the variable, but changing the environment
/// while other threads may read it (the encoder probe spawns ffmpeg beside the setup) is a data
/// race, so it is left set and only ever taken once.
fn take_wayland_socket() -> Option<UnixStream> {
    static TAKEN: AtomicBool = AtomicBool::new(false);
    let socket = std::env::var_os("WAYLAND_SOCKET")?;
    if TAKEN.swap(true, Ordering::SeqCst) {
        return None;
    }
    let text = String::from_utf8_lossy(socket.as_bytes()).into_owned();
    // strtol: leading whitespace and a sign, then digits to the end.
    let fd: i32 = text.trim_start().parse().ok()?;
    // SAFETY: fcntl on whatever descriptor the variable names; a bad one fails.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags == -1 {
            return None;
        }
        libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
    }
    // SAFETY: the environment hands this descriptor to the program to own, and only this call
    // takes it.
    Some(unsafe { UnixStream::from_raw_fd(fd) })
}

/// `wl_display_connect(NULL)` as libwayland does it: the descriptor in `WAYLAND_SOCKET` the first
/// time ([`take_wayland_socket`]), else the socket `WAYLAND_DISPLAY` names ("wayland-0" when
/// unset), inside `XDG_RUNTIME_DIR` unless it is absolute.
fn connect_display() -> Option<Connection> {
    if let Some(stream) = take_wayland_socket() {
        return Connection::from_socket(stream).ok();
    }
    let name = std::env::var_os("WAYLAND_DISPLAY").map(|value| value.as_bytes().to_vec()).unwrap_or(b"wayland-0".into());
    let path = if name.first() == Some(&b'/') {
        name
    } else {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
        if runtime.as_bytes().first() != Some(&b'/') {
            write_stderr("error: XDG_RUNTIME_DIR is invalid or not set in the environment.\n");
            return None;
        }
        [runtime.as_bytes(), b"/", &name].concat()
    };
    // sockaddr_un's path holds 107 bytes and the NUL.
    if path.len() >= 108 {
        return None;
    }
    let stream = UnixStream::connect(std::ffi::OsStr::from_bytes(&path)).ok()?;
    Connection::from_socket(stream).ok()
}

/// A connection with its globals bound and its outputs named (`connectRegistry`: two
/// roundtrips, the second for the outputs' names). `None` when no display could be reached;
/// `Some` with `ok` false when a roundtrip failed.
struct Registry {
    connection: Connection,
    queue: EventQueue<State>,
    _registry: wl_registry::WlRegistry,
}

fn connect_registry(state: &mut State) -> Option<(Registry, bool)> {
    let connection = connect_display()?;
    let mut queue = connection.new_event_queue();
    let registry = connection.display().get_registry(&queue.handle(), ());
    let ok = queue.roundtrip(state).is_ok() && queue.roundtrip(state).is_ok();
    Some((Registry { connection, queue, _registry: registry }, ok))
}

/// `releaseRegistry`'s requests: the two managers' destructors (outputs, the shm and the
/// registry are only forgotten, as their generated `_destroy` calls do).
fn release_registry(state: &mut State) {
    state.outputs.clear();
    if let Some(manager) = state.source_manager.take() {
        manager.destroy();
    }
    if let Some(manager) = state.capture_manager.take() {
        manager.destroy();
    }
    state.shm = None;
    if let Some(dmabuf) = state.dmabuf.take() {
        dmabuf.destroy();
    }
}

/// `ScreenCapture::isSupported`: whether the compositor offers the capture manager, the output
/// source manager and shared memory.
pub fn is_supported() -> bool {
    let mut state = State::default();
    let connected = connect_registry(&mut state);
    let supported = matches!(connected, Some((_, true)))
        && state.capture_manager.is_some()
        && state.source_manager.is_some()
        && state.shm.is_some();
    release_registry(&mut state);
    supported
}

/// The errno and `wl_display_get_error` a failed dispatch leaves, for the message.
fn dispatch_failure(error: &DispatchError) -> (i32, i32) {
    match error {
        DispatchError::Backend(WaylandError::Io(error)) => {
            let code = error.raw_os_error().unwrap_or(libc::EIO);
            (code, code)
        }
        DispatchError::Backend(WaylandError::Protocol(_)) => (libc::EPROTO, libc::EPROTO),
        DispatchError::BadMessage { .. } => (libc::EPROTO, libc::EPROTO),
    }
}

/// The encoder probe running beside the setup, waited for whichever way `start` returns (the
/// C++ `std::future` blocks in its destructor).
struct Probes(Option<JoinHandle<EncoderProbeResults>>);

impl Probes {
    fn get(&mut self) -> EncoderProbeResults {
        self.0.take().and_then(|thread| thread.join().ok()).unwrap_or_default()
    }
}

impl Drop for Probes {
    fn drop(&mut self) {
        if let Some(thread) = self.0.take() {
            let _ = thread.join();
        }
    }
}

/// The capture's shared memory: one memfd mapped whole, the slots back to back.
struct Memory {
    _fd: OwnedFd,
    base: *mut u8,
    size: usize,
}

impl Drop for Memory {
    fn drop(&mut self) {
        if !self.base.is_null() {
            // SAFETY: the mapping made in `start`, unmapped once.
            unsafe { libc::munmap(self.base.cast(), self.size) };
        }
    }
}

/// An environment variable, lower-cased; empty when unset.
fn environment(name: &str) -> String {
    std::env::var(name).unwrap_or_default().to_lowercase()
}

/// The VA-API encoder's constant quantiser: near-lossless, as the capture is re-encoded on
/// export, and lower still for small captures, whose high-contrast edges a lossy encode smears
/// (the ffmpeg path stores those losslessly). `OMAREEL_CAPTURE_QP` overrides it.
fn capture_qp(encoded: Size) -> i32 {
    if let Ok(qp) = environment("OMAREEL_CAPTURE_QP").parse::<i32>() {
        return qp.clamp(0, 51);
    }
    let megapixels = f64::from(encoded.width) * f64::from(encoded.height) / 1e6;
    if megapixels < 1.5 { 12 } else { 18 }
}

/// Slots in the pool: one holds the last frame written (see `writer_loop`), one is being
/// written, and the rest take captures and queue them.
const SLOT_COUNT: i32 = 5;

/// `ScreenCapture`.
pub struct ScreenCapture {
    state: State,
    registry: Option<Registry>,
    source: Option<ExtImageCaptureSourceV1>,
    session: Option<ExtImageCopyCaptureSessionV1>,
    pool: Option<wl_shm_pool::WlShmPool>,
    memory: Option<Memory>,
    /// The dmabuf slots' device, and their buffers: kept here when VA-API encodes them, shared
    /// with the writer when it reads them back.
    gpu_device: Option<gpu::Device>,
    gpu_buffers: Vec<gpu::Buffer>,
    mapped: Option<Arc<MappedSlots>>,
    /// VA-API encodes in the writer thread: there is no ffmpeg process.
    gpu_encoding: bool,
    config: ScreenCaptureConfig,
    crop: CaptureCrop,
    frames: Arc<FrameQueue>,
    writer: Option<JoinHandle<()>>,
    encoder: Option<Encoder>,
    conversion: String,
    last_us: i64,
    last_capture_request_us: i64,
    capture_failures: i32,
    rate_clock: Instant,
    rate_frames: i32,
    rate_drops: i32,
    last_rate: f64,
}

impl Default for ScreenCapture {
    fn default() -> ScreenCapture {
        ScreenCapture::new()
    }
}

impl ScreenCapture {
    pub fn new() -> ScreenCapture {
        ScreenCapture {
            state: State::default(),
            registry: None,
            source: None,
            session: None,
            pool: None,
            memory: None,
            gpu_device: None,
            gpu_buffers: Vec::new(),
            mapped: None,
            gpu_encoding: false,
            config: ScreenCaptureConfig { fps: 60, ..ScreenCaptureConfig::default() },
            crop: CaptureCrop::default(),
            frames: Arc::new(FrameQueue::new(SLOT_COUNT)),
            writer: None,
            encoder: None,
            conversion: "cpu".into(),
            last_us: 0,
            last_capture_request_us: 0,
            capture_failures: 0,
            rate_clock: Instant::now(),
            rate_frames: 0,
            rate_drops: 0,
            last_rate: 0.0,
        }
    }

    fn encoder_pid(&self) -> libc::pid_t {
        self.encoder.as_ref().map_or(-1, |encoder| encoder.pid)
    }

    /// `dispatchOnce`: what is queued, a flush, up to `timeout_ms` of waiting for the socket,
    /// then what arrived. A signal (the daemon's SIGUSR1/2) interrupting the wait is not a
    /// failure. `Err` holds the errno and display error of a failure.
    fn dispatch_once(&mut self, timeout_ms: i32) -> Result<(), (i32, i32)> {
        let Some(registry) = self.registry.as_mut() else { return Err((libc::EINVAL, 0)) };
        registry.queue.dispatch_pending(&mut self.state).map_err(|error| dispatch_failure(&error))?;
        match registry.connection.flush() {
            Ok(()) => {}
            Err(WaylandError::Io(error)) if error.raw_os_error() == Some(libc::EAGAIN) => {}
            Err(error) => return Err(dispatch_failure(&DispatchError::Backend(error))),
        }
        let backend = registry.connection.backend();
        let mut descriptor = libc::pollfd { fd: backend.poll_fd().as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: one pollfd.
        let result = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if result < 0 {
            let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            return if code == libc::EINTR { Ok(()) } else { Err((code, 0)) };
        }
        if result == 0 {
            return Ok(());
        }
        match registry.queue.blocking_dispatch(&mut self.state) {
            Ok(_) => Ok(()),
            Err(DispatchError::Backend(WaylandError::Io(error))) if error.raw_os_error() == Some(libc::EINTR) => Ok(()),
            Err(error) => Err(dispatch_failure(&error)),
        }
    }

    /// Five dmabuf slots on the compositor's device. With VA-API (not on NVIDIA, and not when
    /// `OMAREEL_CAPTURE_ENCODER=ffmpeg` or the lossless debug encoder is asked for) the buffers
    /// take the compositor's uncompressed modifiers, then linear, and the encoder that imports
    /// them is returned. Otherwise, or when VA-API can't import them, they are linear and read
    /// back for ffmpeg ([`MappedSlots`]), with the crop's row copies redone for their stride.
    fn make_dmabuf_slots(&mut self, handle: &QueueHandle<State>) -> Result<Option<gpu::Encoder>, String> {
        let (Some(dmabuf), Some(device)) = (self.state.dmabuf.clone(), self.state.dmabuf_device) else {
            return Err("the compositor offers none".into());
        };
        let (format, modifiers) = [gpu::XRGB8888, gpu::ARGB8888]
            .iter()
            .find_map(|wanted| self.state.dmabuf_formats.iter().find(|(format, _)| format == wanted).cloned())
            .ok_or("no XRGB8888 or ARGB8888 dmabufs are offered")?;
        let device = gpu::Device::open(device)?;
        let (width, height) = (self.state.width as u32, self.state.height as u32);
        let allocate = |modifiers: &[u64]| -> Result<Vec<gpu::Buffer>, String> {
            (0..SLOT_COUNT).map(|_| gpu::Buffer::new(&device, width, height, format, modifiers)).collect()
        };
        let linear = modifiers.contains(&gpu::MOD_LINEAR);
        let implicit = modifiers.is_empty() || modifiers == [gpu::MOD_INVALID];
        let mut made: Option<(Vec<gpu::Buffer>, Option<gpu::Encoder>)> = None;
        let vaapi = !device.is_nvidia()
            && environment("OMAREEL_CAPTURE_ENCODER") != "ffmpeg"
            && environment("OMAREEL_NATIVE_ENCODER") != "lossless";
        if vaapi {
            let plain: Vec<u64> = modifiers.iter().copied().filter(|&modifier| gpu::plain_modifier(modifier)).collect();
            let mut attempts = Vec::new();
            if !plain.is_empty() {
                attempts.push(plain.clone());
            }
            if linear && plain != [gpu::MOD_LINEAR] {
                attempts.push(vec![gpu::MOD_LINEAR]);
            }
            if implicit {
                attempts.push(Vec::new());
            }
            let crop = self.crop.crop;
            let encoded = self.crop.encoded_size;
            let config = gpu::EncoderConfig {
                width,
                height,
                format,
                crop: (crop.x() as u32, crop.y() as u32, encoded.width as u32, encoded.height as u32),
                fps: self.config.fps,
                qp: capture_qp(encoded),
                path: &self.config.output_path,
            };
            for modifiers in attempts {
                match allocate(&modifiers).and_then(|buffers| {
                    gpu::Encoder::open(&device, &config, &buffers).map(|encoder| (buffers, Some(encoder)))
                }) {
                    Ok(slots) => {
                        made = Some(slots);
                        break;
                    }
                    Err(reason) => write_stderr(&format!("capture vaapi unavailable: {reason}\n")),
                }
            }
        }
        let (buffers, encoder) = match made {
            Some(made) => made,
            None => {
                if !linear && !implicit {
                    return Err("no linear dmabufs are offered to read back".into());
                }
                let buffers = allocate(if linear { &[gpu::MOD_LINEAR] } else { &[] })?;
                (buffers, None)
            }
        };
        let (buffers, mapped) = match encoder {
            Some(_) => (buffers, None),
            None => {
                let mapped = MappedSlots::new(buffers)?;
                let mut crop = CaptureCrop::default();
                trim_capture_crop(width as i32, height as i32, mapped.stride() as i32, self.config.crop, &mut crop)?;
                self.crop = crop;
                (Vec::new(), Some(mapped))
            }
        };
        let slot_buffers: Vec<&gpu::Buffer> = match &mapped {
            Some(mapped) => mapped.buffers().collect(),
            None => buffers.iter().collect(),
        };
        for buffer in slot_buffers {
            let params = dmabuf.create_params(handle, ());
            let modifier = buffer.modifier();
            for (plane, (fd, offset, stride)) in buffer.planes().enumerate() {
                params.add(fd, plane as u32, offset, stride, (modifier >> 32) as u32, modifier as u32);
            }
            let created = params.create_immed(
                width as i32,
                height as i32,
                format,
                zwp_linux_buffer_params_v1::Flags::empty(),
                handle,
                (),
            );
            params.destroy();
            self.state.slots.push(Slot { buffer: Some(created), ..Slot::default() });
        }
        self.gpu_buffers = buffers;
        self.mapped = mapped.map(Arc::new);
        self.gpu_device = Some(device);
        Ok(encoder)
    }

    /// Five slots in one shared-memory pool, XRGB8888 or ARGB8888, which the compositor reads
    /// frames back into.
    fn make_shm_slots(&mut self, shm: &wl_shm::WlShm, handle: &QueueHandle<State>, error: &mut String) -> bool {
        let mut format = wl_shm::Format::Xrgb8888;
        if !self.state.shm_formats.contains(&(format as u32)) {
            format = wl_shm::Format::Argb8888;
            if !self.state.shm_formats.contains(&(format as u32)) {
                *error = "No supported shared-memory capture format was offered".into();
                return false;
            }
        }
        let (width, height, stride) = (self.state.width, self.state.height, self.state.stride);
        let slot_bytes = i64::from(stride) * i64::from(height);
        let memory_size = slot_bytes * i64::from(SLOT_COUNT);
        // SAFETY: memfd_create with a static name.
        let memory_fd = unsafe { libc::memfd_create(c"omareel-capture".as_ptr(), libc::MFD_CLOEXEC) };
        if memory_fd < 0 {
            *error = strerror(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
            return false;
        }
        // SAFETY: a new descriptor owned here.
        let memory_fd = unsafe { OwnedFd::from_raw_fd(memory_fd) };
        // SAFETY: ftruncate on the memfd.
        if unsafe { libc::ftruncate(memory_fd.as_raw_fd(), memory_size as libc::off_t) } != 0 {
            *error = strerror(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
            self.memory = Some(Memory { _fd: memory_fd, base: std::ptr::null_mut(), size: 0 });
            return false;
        }
        // SAFETY: maps the whole memfd shared; the result is checked.
        let base = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                memory_size as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                memory_fd.as_raw_fd(),
                0,
            )
        };
        if base == libc::MAP_FAILED {
            *error = strerror(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
            self.memory = Some(Memory { _fd: memory_fd, base: std::ptr::null_mut(), size: 0 });
            return false;
        }
        let pool = shm.create_pool(memory_fd.as_fd(), memory_size as i32, handle, ());
        self.memory = Some(Memory { _fd: memory_fd, base: base.cast(), size: memory_size as usize });
        self.state.slots = (0..SLOT_COUNT)
            .map(|index| Slot {
                buffer: Some(pool.create_buffer(
                    (slot_bytes * i64::from(index)) as i32,
                    width,
                    height,
                    stride,
                    format,
                    handle,
                    (),
                )),
                ..Slot::default()
            })
            .collect();
        self.pool = Some(pool);
        true
    }

    /// `start(config, &error)`. On failure `error` holds why and the capture is left for the
    /// destructor to clean up.
    pub fn start(&mut self, config: &ScreenCaptureConfig, error: &mut String) -> bool {
        let setup_start_us = monotonic_us();
        let mut probes = Probes(Some(std::thread::spawn(encoder_probe_results)));
        self.config = config.clone();
        self.config.fps = config.fps.max(1);
        let registry_start_us = monotonic_us();
        let Some((registry, connected)) = connect_registry(&mut self.state) else {
            *error = "Could not connect to the display server".into();
            return false;
        };
        self.registry = Some(registry);
        if !connected {
            *error = "Could not connect to the display server".into();
            return false;
        }
        debug_start_stage(
            "registry",
            setup_start_us,
            &format!(
                "duration_ms={}",
                qnumber::number_f64((monotonic_us() - registry_start_us) as f64 / 1000.0, 'f', 3)
            ),
        );
        let (Some(capture_manager), Some(source_manager), Some(shm)) =
            (self.state.capture_manager.clone(), self.state.source_manager.clone(), self.state.shm.clone())
        else {
            *error = "The in-process capture protocol is unavailable".into();
            return false;
        };
        let Some(output) = self.state.outputs.iter().find(|output| output.name == config.monitor) else {
            *error = "The requested monitor is unavailable".into();
            return false;
        };
        let output = output.object.clone();
        let Some(handle) = self.registry.as_ref().map(|registry| registry.queue.handle()) else { return false };
        let source = source_manager.create_source(&output, &handle, ());
        let session = capture_manager.create_session(
            &source,
            ext_image_copy_capture_manager_v1::Options::empty(),
            &handle,
            (),
        );
        self.source = Some(source);
        self.session = Some(session.clone());
        while !self.state.constraints_done && !self.state.stopped {
            if self.dispatch_once(100).is_err() {
                *error = "Capture setup failed".into();
                return false;
            }
        }
        if self.state.stopped || self.state.width <= 0 || self.state.height <= 0 {
            *error = "Capture stopped during setup".into();
            return false;
        }
        let (width, height, stride) = (self.state.width, self.state.height, self.state.stride);
        if let Err(message) = trim_capture_crop(width, height, stride, config.crop, &mut self.crop) {
            *error = message;
            return false;
        }
        let mut gpu_encoder = None;
        let dmabufs = environment("OMAREEL_CAPTURE_BUFFERS") != "shm"
            && match self.make_dmabuf_slots(&handle) {
                Ok(encoder) => {
                    gpu_encoder = encoder;
                    true
                }
                Err(reason) => {
                    write_stderr(&format!("capture dmabufs unavailable: {reason}\n"));
                    false
                }
            };
        if !dmabufs && !self.make_shm_slots(&shm, &handle, error) {
            return false;
        }

        self.rate_clock = Instant::now();
        let writer_config = WriterConfig {
            fps: self.config.fps,
            timestamp_path: self.config.timestamp_path.clone(),
            encoded_size: self.crop.encoded_size,
            crop_rows: self.crop.crop_rows.clone(),
        };
        let frames = Arc::clone(&self.frames);
        if let Some(mut encoder) = gpu_encoder {
            self.gpu_encoding = true;
            self.conversion = "vaapi".into();
            drop(probes);
            self.writer = Some(std::thread::spawn(move || run_writer(&frames, &writer_config, &mut encoder)));
        } else {
            let probe_results = probes.get();
            debug_start_stage(
                "probes",
                setup_start_us,
                &format!(
                    "cache={} preferred={} gpu={}",
                    if probe_results.cached { "hit" } else { "miss" },
                    u8::from(probe_results.preferred),
                    u8::from(probe_results.gpu_conversion)
                ),
            );
            let encoder = encoder_arguments_from_environment(
                self.crop.encoded_size,
                self.config.fps,
                &probe_results,
                &config.output_path,
            );
            self.conversion = encoder.conversion;
            match Encoder::start(&encoder.arguments) {
                Ok(started) => self.encoder = Some(started),
                Err(message) => {
                    *error = message;
                    return false;
                }
            }
            let mut sink =
                FdSink(self.encoder.as_ref().and_then(|e| e.input.as_ref()).map_or(-1, AsRawFd::as_raw_fd));
            if let Some(mapped) = self.mapped.clone() {
                self.writer =
                    Some(std::thread::spawn(move || writer_loop(&frames, &writer_config, mapped.as_ref(), &mut sink)));
            } else {
                let Some(memory) = self.memory.as_ref() else { return false };
                let slot_bytes = memory.size / SLOT_COUNT as usize;
                // SAFETY: the mapping holds SLOT_COUNT slots of `slot_bytes` and stays mapped until
                // `cleanup`, which joins the writer first.
                let memory =
                    unsafe { RawSlotMemory::new(memory.base.cast_const().cast(), slot_bytes, SLOT_COUNT as usize) };
                self.writer =
                    Some(std::thread::spawn(move || writer_loop(&frames, &writer_config, &memory, &mut sink)));
            }
        }
        write_stderr(&format!(
            "capture conversion={} buffers={} slots={} crop={}x{}\n",
            self.conversion,
            if dmabufs { "dmabuf" } else { "shm" },
            SLOT_COUNT,
            self.crop.encoded_size.width,
            self.crop.encoded_size.height
        ));
        debug_start_stage("capture_setup_ready", setup_start_us, "");
        true
    }

    /// `captureFrame(&error)`: one turn of the capture loop. False, with `error` set, once the
    /// capture or the encoder has stopped or the connection failed.
    pub fn capture_frame(&mut self, error: &mut String) -> bool {
        if self.state.stopped || (!self.gpu_encoding && self.encoder_pid() <= 1) || self.frames.writer_failed() {
            let writer_error = self.frames.writer_error();
            *error = if writer_error.is_empty() { "Capture or encoding stopped unexpectedly".into() } else { writer_error };
            return false;
        }
        let now = monotonic_us();
        let interval_us = i64::from(1_000_000 / self.config.fps);
        if self.last_capture_request_us == 0 || now - self.last_capture_request_us >= interval_us {
            // The protocol allows exactly one frame object per session; requesting another
            // while one is outstanding raises duplicate_frame and kills the connection.
            let frame_in_flight = self.state.slots.iter().any(|slot| slot.frame.is_some());
            let index = if frame_in_flight { -1 } else { self.frames.acquire() };
            // A copy that outlasts the interval leaves its tick pending, so the next request goes
            // out as soon as the copy completes instead of waiting for the tick after.
            if !frame_in_flight {
                if self.last_capture_request_us == 0 {
                    self.last_capture_request_us = now;
                } else {
                    self.last_capture_request_us += interval_us;
                    if now - self.last_capture_request_us > interval_us {
                        self.last_capture_request_us = now;
                    }
                }
            }
            if index >= 0 {
                if let (Some(session), Some(registry)) = (self.session.as_ref(), self.registry.as_ref()) {
                    let (width, height) = (self.state.width, self.state.height);
                    let slot = &mut self.state.slots[index as usize];
                    slot.ready = false;
                    slot.failed = false;
                    slot.presentation_us = 0;
                    let frame = session.create_frame(&registry.queue.handle(), index as usize);
                    if let Some(buffer) = slot.buffer.as_ref() {
                        frame.attach_buffer(buffer);
                    }
                    frame.damage_buffer(0, 0, width, height);
                    frame.capture();
                    slot.frame = Some(frame);
                }
            }
        }
        // Wait for the compositor rather than spin: until an event arrives while a capture is in
        // flight, else until the next capture is due. 50 ms at most, so a stopped encoder or a
        // stop request (whose signal may land on another thread) is noticed promptly.
        let in_flight = self.state.slots.iter().any(|slot| slot.frame.is_some());
        let due_in_us = self.last_capture_request_us + interval_us - monotonic_us();
        let wait_ms = if in_flight { 50 } else { (due_in_us + 999).div_euclid(1000).clamp(0, 50) as i32 };
        if let Err((code, display_error)) = self.dispatch_once(wait_ms) {
            *error = format!("Capture connection failed (errno {code}: {}, display error {display_error})", strerror(code));
            return false;
        }

        let mut completed: Vec<usize> = (0..self.state.slots.len())
            .filter(|&index| {
                let slot = &self.state.slots[index];
                slot.frame.is_some() && (slot.ready || slot.failed)
            })
            .collect();
        let slots = &self.state.slots;
        completed.sort_by(|&left, &right| {
            let (left, right) = (&slots[left], &slots[right]);
            left.failed.cmp(&right.failed).then(left.presentation_us.cmp(&right.presentation_us))
        });
        for index in completed {
            let slot = &mut self.state.slots[index];
            if let Some(frame) = slot.frame.take() {
                frame.destroy();
            }
            if slot.failed {
                self.capture_failures += 1;
                self.frames.release(index as i32);
                continue;
            }
            // The writer times frames by presentation; arrival stands in if the compositor sent
            // none.
            if slot.presentation_us <= 0 {
                slot.presentation_us = monotonic_us();
            }
            self.last_us = slot.presentation_us;
            if !self.frames.mark_queued(index as i32, slot.presentation_us) {
                *error = "Capture slot state became inconsistent".into();
                return false;
            }
        }

        if self.rate_clock.elapsed().as_millis() >= 1000 {
            let total_frames = self.frames.encoded_frames();
            let total_drops = self.frames.drops() + self.capture_failures;
            let interval_frames = total_frames - self.rate_frames;
            let interval_drops = total_drops - self.rate_drops;
            self.last_rate = f64::from(interval_frames) * 1000.0 / self.rate_clock.elapsed().as_millis() as f64;
            write_stderr(&format!(
                "capture frames={interval_frames} fps={} drops={interval_drops}\n",
                qnumber::number_f64(self.last_rate, 'f', 1)
            ));
            self.rate_frames = total_frames;
            self.rate_drops = total_drops;
            self.rate_clock = Instant::now();
        }
        true
    }

    /// `stopEncoder(drain, &error)`: stops the writer (draining what is queued, or aborting it
    /// and terminating the encoder), closes the encoder's input and waits for it (15 s when
    /// draining, 1 s otherwise).
    fn stop_encoder(&mut self, drain: bool, error: &mut String) -> bool {
        if let Some(writer) = self.writer.take() {
            self.frames.stop_writer(drain);
            if !drain && self.encoder_pid() > 1 {
                // SAFETY: the encoder is this process's child and not yet reaped.
                unsafe { libc::kill(self.encoder_pid(), libc::SIGTERM) };
            }
            let _ = writer.join();
        }
        if let Some(encoder) = self.encoder.as_mut() {
            encoder.input = None;
        }
        let Some(mut encoder) = self.encoder.take().filter(|encoder| encoder.pid > 1) else {
            return !self.frames.writer_failed();
        };
        let exit = wait_for_encoder(&mut encoder, if drain { 15000 } else { 1000 });
        if exit == EncoderExit::TimedOut {
            *error = "Timed out finalizing the screen video".into();
            return false;
        }
        encoder.read_error();
        encoder.error_pipe = None;
        if self.frames.writer_failed() {
            *error = self.frames.writer_error();
            let detail = encoder.error_text();
            if !detail.is_empty() {
                error.push('\n');
                error.push_str(&detail);
            }
            return false;
        }
        let exited_cleanly = matches!(exit, EncoderExit::Reaped(status)
            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
        if !exited_cleanly {
            *error = encoder.error_text();
            if error.is_empty() {
                *error = "The screen encoder exited unexpectedly".into();
            }
            return false;
        }
        true
    }

    /// `finish(&error)`: the video runs until now even if the screen has not changed since its
    /// last frame; captures in flight are dropped, the writer drained and the encoder waited
    /// for. True when it encoded cleanly and a frame was written.
    pub fn finish(&mut self, error: &mut String) -> bool {
        self.frames.set_stop_us(monotonic_us());
        for (index, slot) in self.state.slots.iter_mut().enumerate() {
            let Some(frame) = slot.frame.take() else { continue };
            frame.destroy();
            self.frames.release(index as i32);
        }
        let encoded = self.stop_encoder(true, error);
        encoded && self.frames.first_frame_us() > 0
    }

    /// `abort()`: stops the writer and the encoder at once.
    pub fn abort(&mut self) {
        self.stop_encoder(false, &mut String::new());
    }

    /// `firstFrameUs()`: the first frame's presentation time, 0 before it.
    pub fn first_frame_us(&self) -> i64 {
        self.frames.first_frame_us()
    }

    /// `lastFrameUs()`: the presentation time of the last capture queued.
    pub fn last_frame_us(&self) -> i64 {
        self.last_us
    }

    /// `videoDurationUs()`: from the first frame to the end of the last one written.
    pub fn video_duration_us(&self) -> i64 {
        self.frames.video_duration_us(self.config.fps)
    }

    /// `outputSize()`: the size encoded (`QSize()` until `start` got that far).
    pub fn output_size(&self) -> Size {
        self.crop.encoded_size
    }

    /// `encodedFrames()`.
    pub fn encoded_frames(&self) -> i32 {
        self.frames.encoded_frames()
    }

    /// `droppedFrames()`: captures that found no free slot, and failed captures.
    pub fn dropped_frames(&self) -> i32 {
        self.frames.drops() + self.capture_failures
    }

    /// `measuredFps()`: the rate printed last.
    pub fn measured_fps(&self) -> f64 {
        self.last_rate
    }

    /// `conversionMode()`: "vaapi" when VA-API converts and encodes in process, "gpu" when CUDA
    /// converts for ffmpeg, else "cpu".
    pub fn conversion_mode(&self) -> &str {
        &self.conversion
    }

    /// `Private::cleanup`, as the destructor runs it.
    fn cleanup(&mut self) {
        self.stop_encoder(false, &mut String::new());
        for slot in self.state.slots.drain(..) {
            if let Some(frame) = slot.frame {
                frame.destroy();
            }
            if let Some(buffer) = slot.buffer {
                buffer.destroy();
            }
        }
        if let Some(pool) = self.pool.take() {
            pool.destroy();
        }
        self.memory = None;
        // After the writer (and the VA-API encoder it owned) is gone and the compositor's buffers
        // are destroyed.
        self.gpu_buffers.clear();
        self.mapped = None;
        self.gpu_device = None;
        if let Some(session) = self.session.take() {
            session.destroy();
        }
        if let Some(source) = self.source.take() {
            source.destroy();
        }
        release_registry(&mut self.state);
        self.registry = None;
    }
}

impl Drop for ScreenCapture {
    fn drop(&mut self) {
        self.cleanup();
    }
}
