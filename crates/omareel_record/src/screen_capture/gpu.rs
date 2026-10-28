//! The capture's GPU side (`csrc/gpu_capture.c`): dmabufs the compositor renders frames into,
//! allocated with GBM on the device it names, and an encoder that crops, converts (BT.709) and
//! encodes them with VA-API into fragmented MP4, so captured frames never reach the CPU. Where
//! VA-API can't be used (NVIDIA's driver, or none that imports the buffers), linear dmabufs are
//! read back here instead ([`MappedSlots`]), which still keeps the copy off the compositor's
//! render thread: a shared-memory capture makes Hyprland read every frame back itself.

use super::writer::{FrameOutput, SlotMemory};
use std::ffi::{CStr, CString, c_char, c_int, c_void};

const MAX_PLANES: usize = 4;

/// `oc_buffer`.
#[repr(C)]
struct RawBuffer {
    bo: *mut c_void,
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    planes: c_int,
    fds: [c_int; MAX_PLANES],
    offsets: [u32; MAX_PLANES],
    strides: [u32; MAX_PLANES],
}

/// `oc_encoder_config`.
#[repr(C)]
struct RawEncoderConfig {
    width: u32,
    height: u32,
    format: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
    fps: c_int,
    qp: c_int,
    path: *const c_char,
}

#[repr(C)]
struct RawDevice {
    _private: [u8; 0],
}

#[repr(C)]
struct RawEncoder {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn oc_device_open(device: u64, out: *mut *mut RawDevice, error: *mut c_char, size: usize) -> c_int;
    fn oc_device_close(device: *mut RawDevice);
    fn oc_device_driver(device: *const RawDevice) -> *const c_char;
    fn oc_device_path(device: *const RawDevice) -> *const c_char;
    fn oc_buffer_create(
        device: *mut RawDevice,
        width: u32,
        height: u32,
        format: u32,
        modifiers: *const u64,
        count: usize,
        out: *mut RawBuffer,
        error: *mut c_char,
        size: usize,
    ) -> c_int;
    fn oc_buffer_destroy(buffer: *mut RawBuffer);
    fn oc_buffer_map(buffer: *mut RawBuffer, data: *mut *const u8, stride: *mut u32, mapping: *mut *mut c_void) -> c_int;
    fn oc_buffer_unmap(buffer: *mut RawBuffer, mapping: *mut c_void);
    fn oc_encoder_open(
        device: *mut RawDevice,
        config: *const RawEncoderConfig,
        buffers: *const RawBuffer,
        count: usize,
        out: *mut *mut RawEncoder,
        error: *mut c_char,
        size: usize,
    ) -> c_int;
    fn oc_encoder_submit(encoder: *mut RawEncoder, index: usize, pts: i64, error: *mut c_char, size: usize) -> c_int;
    fn oc_encoder_finish(encoder: *mut RawEncoder, error: *mut c_char, size: usize) -> c_int;
    fn oc_encoder_destroy(encoder: *mut RawEncoder);
}

/// Calls `call` with a buffer for the shim's message, `Err` with it when the call failed.
fn checked(call: impl FnOnce(*mut c_char, usize) -> c_int) -> Result<(), String> {
    let mut error = [0 as c_char; 512];
    if call(error.as_mut_ptr(), error.len()) >= 0 {
        return Ok(());
    }
    // SAFETY: the shim leaves a NUL-terminated message in the buffer (or nothing, all zeros).
    let message = unsafe { CStr::from_ptr(error.as_ptr()) }.to_string_lossy().into_owned();
    Err(if message.is_empty() { "The GPU capture failed".into() } else { message })
}

/// `DRM_FORMAT_XRGB8888` and `DRM_FORMAT_ARGB8888`, the formats a capture asks for (in that
/// order of preference).
pub const XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
/// `DRM_FORMAT_MOD_LINEAR`, and `DRM_FORMAT_MOD_INVALID` (the implicit modifier).
pub const MOD_LINEAR: u64 = 0;
pub const MOD_INVALID: u64 = 0x00ff_ffff_ffff_ffff;

/// Whether VA-API should be trusted with modifier `modifier`: compressed layouts (Intel's CCS
/// variants, AMD's DCC) can be rendered by the compositor in ways a video engine can't read
/// back, so only plain tilings are offered to it.
pub fn plain_modifier(modifier: u64) -> bool {
    const VENDOR_INTEL: u64 = 0x01;
    const VENDOR_AMD: u64 = 0x02;
    let vendor = modifier >> 56;
    let code = modifier & 0x00ff_ffff_ffff_ffff;
    match vendor {
        0 => code == 0,
        // X, Y, Yf and 4 tiling (1, 2, 3, 9); every other Intel code adds compression.
        VENDOR_INTEL => matches!(code, 1 | 2 | 3 | 9),
        // AMD_FMT_MOD_DCC is bit 13.
        VENDOR_AMD => code & (1 << 13) == 0,
        _ => false,
    }
}

/// An open render node (`oc_device`).
pub struct Device(*mut RawDevice);

// SAFETY: the device is only used by one thread at a time (the capture's setup, then dropped
// after the writer has been joined); the shim keeps no thread-local state.
unsafe impl Send for Device {}

impl Device {
    /// The render node of DRM device `device` (a `dev_t`, as `dmabuf_device` sends it).
    pub fn open(device: u64) -> Result<Device, String> {
        let mut raw = std::ptr::null_mut();
        // SAFETY: the shim writes the device on success.
        checked(|error, size| unsafe { oc_device_open(device, &mut raw, error, size) })?;
        Ok(Device(raw))
    }

    /// The kernel driver's name.
    pub fn driver(&self) -> String {
        // SAFETY: a NUL-terminated string the device owns.
        unsafe { CStr::from_ptr(oc_device_driver(self.0)) }.to_string_lossy().into_owned()
    }

    /// The render node's path.
    pub fn path(&self) -> String {
        // SAFETY: as for `driver`.
        unsafe { CStr::from_ptr(oc_device_path(self.0)) }.to_string_lossy().into_owned()
    }

    /// NVIDIA's driver, whose VA-API (nvidia-vaapi-driver) can't encode and crashes reading
    /// frames back on some machines.
    pub fn is_nvidia(&self) -> bool {
        self.driver().starts_with("nvidia")
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: opened by `open`, closed once.
        unsafe { oc_device_close(self.0) };
    }
}

/// A dmabuf frames are captured into (`oc_buffer`).
#[repr(transparent)]
pub struct Buffer(RawBuffer);

// SAFETY: the buffer's GBM object is only mapped by the writer thread while the ring hands it
// the slot, and destroyed after the writer has been joined.
unsafe impl Send for Buffer {}
// SAFETY: as for Send; mapping for reading doesn't change what other threads see.
unsafe impl Sync for Buffer {}

impl Buffer {
    /// A `width` by `height` buffer in `format`, with one of `modifiers` (the driver's choice
    /// when empty).
    pub fn new(device: &Device, width: u32, height: u32, format: u32, modifiers: &[u64]) -> Result<Buffer, String> {
        // SAFETY: an all-zero oc_buffer is what the shim starts from.
        let mut raw: RawBuffer = unsafe { std::mem::zeroed() };
        // SAFETY: the modifiers outlive the call; the shim fills `raw`.
        checked(|error, size| unsafe {
            oc_buffer_create(device.0, width, height, format, modifiers.as_ptr(), modifiers.len(), &mut raw, error, size)
        })?;
        Ok(Buffer(raw))
    }

    pub fn modifier(&self) -> u64 {
        self.0.modifier
    }

    /// Each plane's descriptor, offset and stride, for `zwp_linux_buffer_params_v1.add`.
    pub fn planes(&self) -> impl Iterator<Item = (std::os::fd::BorrowedFd<'_>, u32, u32)> {
        (0..self.0.planes.clamp(0, MAX_PLANES as c_int) as usize).map(|plane| {
            // SAFETY: the buffer owns the descriptor until it is dropped, after the borrow ends.
            let fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(self.0.fds[plane]) };
            (fd, self.0.offsets[plane], self.0.strides[plane])
        })
    }

    /// Calls `read` with the buffer mapped for reading (its first byte and row stride); false
    /// when it can't be mapped.
    fn read(&self, read: &mut dyn FnMut(*const u8, usize)) -> bool {
        let raw = std::ptr::from_ref(&self.0).cast_mut();
        let mut data = std::ptr::null();
        let mut stride = 0;
        let mut mapping = std::ptr::null_mut();
        // SAFETY: mapping only reads the buffer; the pointer is valid until the unmap below.
        if unsafe { oc_buffer_map(raw, &mut data, &mut stride, &mut mapping) } < 0 {
            return false;
        }
        read(data, stride as usize);
        // SAFETY: the mapping made above.
        unsafe { oc_buffer_unmap(raw, mapping) };
        true
    }

    /// Whether the buffer can be read on the CPU with rows `stride` apart.
    pub fn readable_with_stride(&self, stride: usize) -> bool {
        let mut matches = false;
        self.read(&mut |_, actual| matches = actual == stride) && matches
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: made by `new`, destroyed once.
        unsafe { oc_buffer_destroy(&mut self.0) };
    }
}

/// Linear dmabufs read back on the CPU for the ffmpeg encoder, one per slot, each
/// `stride * height` bytes.
pub struct MappedSlots {
    pub buffers: Vec<Buffer>,
    pub stride: usize,
    pub height: usize,
}

impl SlotMemory for MappedSlots {
    fn with_slot(&self, index: usize, read: &mut dyn FnMut(&[u8])) {
        let Some(buffer) = self.buffers.get(index) else { return };
        let bytes = self.stride * self.height;
        buffer.read(&mut |data, _| {
            // SAFETY: the mapping covers `height` rows of `stride` bytes (checked when the slots
            // were made) and stays mapped for this call.
            read(unsafe { std::slice::from_raw_parts(data, bytes) });
        });
    }
}

/// What the GPU encoder is given.
pub struct EncoderConfig<'a> {
    pub width: u32,
    pub height: u32,
    pub format: u32,
    /// x, y, width and height of the encoded part, even sizes.
    pub crop: (u32, u32, u32, u32),
    pub fps: i32,
    pub qp: i32,
    pub path: &'a str,
}

/// The VA-API encoder (`oc_encoder`), writing the screen video.
pub struct Encoder(*mut RawEncoder);

// SAFETY: made on the capture's thread, then used and dropped only by the writer thread.
unsafe impl Send for Encoder {}

impl Encoder {
    /// Opens the encoder with `buffers` imported, slot `i` being `buffers[i]`.
    pub fn open(device: &Device, config: &EncoderConfig, buffers: &[Buffer]) -> Result<Encoder, String> {
        let path = CString::new(config.path).map_err(|_| "The screen video's path holds a NUL".to_owned())?;
        let raw_config = RawEncoderConfig {
            width: config.width,
            height: config.height,
            format: config.format,
            crop_x: config.crop.0,
            crop_y: config.crop.1,
            crop_width: config.crop.2,
            crop_height: config.crop.3,
            fps: config.fps,
            qp: config.qp,
            path: path.as_ptr(),
        };
        let mut raw = std::ptr::null_mut();
        // SAFETY: `Buffer` is a transparent `oc_buffer`; everything passed outlives the call, and
        // the shim imports the buffers without keeping a pointer to them.
        checked(|error, size| unsafe {
            oc_encoder_open(device.0, &raw_config, buffers.as_ptr().cast(), buffers.len(), &mut raw, error, size)
        })?;
        Ok(Encoder(raw))
    }
}

impl FrameOutput for Encoder {
    fn write(&mut self, index: usize, frame_index: i64) -> Result<(), String> {
        // SAFETY: the encoder is open; the shim checks the index.
        checked(|error, size| unsafe { oc_encoder_submit(self.0, index, frame_index, error, size) })
    }

    fn end(&mut self) -> Result<(), String> {
        // SAFETY: as above.
        checked(|error, size| unsafe { oc_encoder_finish(self.0, error, size) })
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: opened by `open`, destroyed once.
        unsafe { oc_encoder_destroy(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_uncompressed_modifiers_are_plain() {
        assert!(plain_modifier(MOD_LINEAR));
        // I915_FORMAT_MOD_X_TILED, Y_TILED, 4_TILED.
        assert!(plain_modifier((1 << 56) | 1) && plain_modifier((1 << 56) | 2) && plain_modifier((1 << 56) | 9));
        // I915_FORMAT_MOD_Y_TILED_CCS, 4_TILED_DG2_RC_CCS.
        assert!(!plain_modifier((1 << 56) | 4) && !plain_modifier((1 << 56) | 10));
        // An AMD modifier with and without DCC.
        assert!(plain_modifier((2 << 56) | 0x6_0000_0f04) && !plain_modifier((2 << 56) | (1 << 13) | 0x6_0000_0f04));
        assert!(!plain_modifier(MOD_INVALID));
    }
}
