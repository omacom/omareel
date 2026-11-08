//! The cameras the launcher lists, read from V4L2 with the ioctls Qt Multimedia's V4L2 backend
//! uses (what `QMediaDevices::videoInputs()` gave `LauncherBase::refreshCameraDevices`): every
//! `/dev/videoN` that captures video, with its card name and each format's sizes and real frame
//! rates. `crate::camera::devices` asks ffmpeg, which prints no frame rates, so a 15 fps IR
//! sensor would pass `omareel_record::camera::camera_can_record` there; here it doesn't.
//!
//! Opening a node to ask it never starts streaming, so this is safe while the self-view host
//! has the camera open.

use omareel_record::camera::{CameraDevice, CameraFormat};
use std::os::fd::AsRawFd;

const VIDIOC_QUERYCAP: u64 = 0x8068_5600;
const VIDIOC_ENUM_FMT: u64 = 0xC040_5602;
const VIDIOC_ENUM_FRAMESIZES: u64 = 0xC02C_564A;
const VIDIOC_ENUM_FRAMEINTERVALS: u64 = 0xC034_564B;
const CAP_VIDEO_CAPTURE: u32 = 0x1;
const CAP_DEVICE_CAPS: u32 = 0x8000_0000;
const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
const DISCRETE: u32 = 1;

#[repr(C)]
struct Capability {
    driver: [u8; 16],
    card: [u8; 32],
    bus_info: [u8; 32],
    version: u32,
    capabilities: u32,
    device_caps: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct FmtDesc {
    index: u32,
    kind: u32,
    flags: u32,
    description: [u8; 32],
    pixel_format: u32,
    mbus_code: u32,
    reserved: [u32; 3],
}

#[repr(C)]
struct FrameSize {
    index: u32,
    pixel_format: u32,
    kind: u32,
    /// discrete: width, height; stepwise: min_w, max_w, step_w, min_h, max_h, step_h.
    data: [u32; 6],
    reserved: [u32; 2],
}

#[repr(C)]
struct FrameInterval {
    index: u32,
    pixel_format: u32,
    width: u32,
    height: u32,
    kind: u32,
    /// discrete: numerator, denominator; stepwise: min, max and step, each a fraction.
    data: [u32; 6],
    reserved: [u32; 2],
}

const _: () = assert!(size_of::<Capability>() == 104);
const _: () = assert!(size_of::<FmtDesc>() == 64);
const _: () = assert!(size_of::<FrameSize>() == 44);
const _: () = assert!(size_of::<FrameInterval>() == 52);

/// `ioctl` on a zeroed `T` filled by `fill`; the struct when the driver answered.
fn ask<T>(fd: i32, request: u64, fill: impl FnOnce(&mut T)) -> Option<T> {
    // SAFETY: every T here is a plain-old-data V4L2 struct, valid when zeroed, and its size is
    // the one encoded in `request`.
    let mut value: T = unsafe { std::mem::zeroed() };
    fill(&mut value);
    let result = unsafe { libc::ioctl(fd, request as _, &mut value as *mut T) };
    (result == 0).then_some(value)
}

fn c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

fn fourcc(code: u32) -> String {
    code.to_le_bytes().iter().map(|&b| b as char).collect::<String>().trim().to_owned()
}

/// The `/dev/videoN` nodes, in number order (what a refresh is triggered by when it changes).
pub fn nodes() -> Vec<String> {
    let mut nodes: Vec<String> = std::fs::read_dir("/sys/class/video4linux")
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("video")).collect())
        .unwrap_or_default();
    nodes.sort_by_key(|n| n.trim_start_matches("video").parse::<u32>().unwrap_or(u32::MAX));
    nodes.into_iter().map(|n| format!("/dev/{n}")).collect()
}

/// The capture devices, as `omareel_record::camera` takes them: the id names the node (Qt's
/// V4L2 ids are the node path), the description is the card name.
pub fn devices() -> Vec<CameraDevice> {
    nodes().into_iter().filter_map(|node| device(&node)).collect()
}

fn device(node: &str) -> Option<CameraDevice> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(node).ok()?;
    let fd = file.as_raw_fd();
    let caps = ask::<Capability>(fd, VIDIOC_QUERYCAP, |_| {})?;
    let device_caps = if caps.capabilities & CAP_DEVICE_CAPS != 0 { caps.device_caps } else { caps.capabilities };
    if device_caps & CAP_VIDEO_CAPTURE == 0 {
        return None;
    }
    let mut formats = Vec::new();
    for index in 0.. {
        let Some(desc) = ask::<FmtDesc>(fd, VIDIOC_ENUM_FMT, |d| {
            d.index = index;
            d.kind = BUF_TYPE_VIDEO_CAPTURE;
        }) else {
            break;
        };
        for (width, height) in sizes(fd, desc.pixel_format) {
            let (min_frame_rate, max_frame_rate) = frame_rates(fd, desc.pixel_format, width, height);
            formats.push(CameraFormat {
                pixel_format: fourcc(desc.pixel_format),
                width: width as i32,
                height: height as i32,
                min_frame_rate,
                max_frame_rate,
            });
        }
    }
    Some(CameraDevice { id: node.as_bytes().to_vec(), description: c_string(&caps.card), formats })
}

fn sizes(fd: i32, pixel_format: u32) -> Vec<(u32, u32)> {
    let mut sizes = Vec::new();
    for index in 0.. {
        let Some(size) = ask::<FrameSize>(fd, VIDIOC_ENUM_FRAMESIZES, |s| {
            s.index = index;
            s.pixel_format = pixel_format;
        }) else {
            break;
        };
        if size.kind == DISCRETE {
            sizes.push((size.data[0], size.data[1]));
        } else {
            // Stepwise or continuous: the largest size stands for the range.
            sizes.push((size.data[1], size.data[4]));
            break;
        }
    }
    sizes
}

/// The slowest and fastest frame rate of a format at a size, as Qt keeps them.
fn frame_rates(fd: i32, pixel_format: u32, width: u32, height: u32) -> (f32, f32) {
    let rate = |numerator: u32, denominator: u32| if numerator == 0 { 0.0 } else { denominator as f32 / numerator as f32 };
    let (mut low, mut high) = (f32::MAX, 0.0f32);
    for index in 0.. {
        let Some(interval) = ask::<FrameInterval>(fd, VIDIOC_ENUM_FRAMEINTERVALS, |i| {
            i.index = index;
            i.pixel_format = pixel_format;
            i.width = width;
            i.height = height;
        }) else {
            break;
        };
        if interval.kind == DISCRETE {
            let r = rate(interval.data[0], interval.data[1]);
            low = low.min(r);
            high = high.max(r);
        } else {
            // The shortest interval is the fastest rate.
            high = high.max(rate(interval.data[0], interval.data[1]));
            low = low.min(rate(interval.data[2], interval.data[3]));
            break;
        }
    }
    if high == 0.0 { (0.0, 0.0) } else { (low, high) }
}
