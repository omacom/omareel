//! The pure part of `CameraCapture.cpp`: which path settings keep for a camera
//! (`stableCameraPath`), which `/dev/videoN` node a path or a Qt camera id names, which of a
//! camera's formats the recorder picks (`selectedFormat`, `cameraCanRecord`), and the first-frame
//! timestamp file (`FirstFrameTimestamp`).
//!
//! Cameras are plain data here: the Qt side passes the facts of each `QCameraDevice` and
//! `QCameraFormat` in ([`CameraDevice`], [`CameraFormat`]). The capture itself (the `QCamera`,
//! the frame times, the frame packing and the ffmpeg encoder) stays in C++.

use omareel_core::compat::qdir::{self, FileInfo, Filters, SortFlags};
use omareel_core::compat::qsavefile::SaveFile;
use omareel_core::compat::{qfile, qstring};

/// What the recorder needs of a `QCameraFormat`. Qt keeps the frame rates as `float`, and the
/// format choice computes with them as such.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraFormat {
    /// The pixel format's name as the Qt side reports it (`Jpeg`, `YUYV`, ...); the choice does
    /// not look at it.
    pub pixel_format: String,
    pub width: i32,
    pub height: i32,
    pub min_frame_rate: f32,
    pub max_frame_rate: f32,
}

/// What the recorder needs of a `QCameraDevice`: its id (`QCameraDevice::id()`, raw bytes),
/// description and video formats. `CameraDevice::default()` stands for the null device.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraDevice {
    pub id: Vec<u8>,
    pub description: String,
    pub formats: Vec<CameraFormat>,
}

/// `selectedFormat(device, requestedHeight)` (file-static in `CameraCapture.cpp`): the format of
/// at least 30 fps closest to 1920x1080 (1280x720 when 720 is asked for), a frame rate away from
/// 30 counting a hundred pixels per frame per second; the first of equally good formats wins.
/// `None` is the null `QCameraFormat`.
pub fn selected_format(device: &CameraDevice, requested_height: i32) -> Option<&CameraFormat> {
    let mut best = None;
    let mut best_score = f64::MAX;
    let (requested_width, requested_height) = if requested_height == 720 { (1280, 720) } else { (1920, 1080) };
    for format in &device.formats {
        // float + double, compared as double.
        if f64::from(format.max_frame_rate) + 0.01 < 30.0 {
            continue;
        }
        let size_penalty = f64::from((format.width - requested_width).abs() + (format.height - requested_height).abs());
        // std::abs of the float difference, then widened.
        let frame_rate_penalty = f64::from((format.max_frame_rate - 30.0f32).abs()) * 100.0;
        let score = size_penalty + frame_rate_penalty;
        if score < best_score {
            best = Some(format);
            best_score = score;
        }
    }
    best
}

/// `cameraCanRecord`: whether the camera has a format the recorder records from (30 fps or
/// more). IR sensors that sit beside a laptop's webcam don't.
pub fn camera_can_record(device: &CameraDevice) -> bool {
    selected_format(device, 1080).is_some()
}

/// `QFile(path).readAll()` decoded and trimmed, or "" when it can't be read.
fn read_trimmed(path: &str) -> String {
    qfile::read_all(path).map(|bytes| qstring::trimmed(&qstring::from_utf8(&bytes)).to_owned()).unwrap_or_default()
}

/// `stableCameraPath(devicePath)` with the system's `/dev` and `/sys`.
pub fn stable_camera_path(device_path: &str) -> String {
    stable_camera_path_in(device_path, "/dev", "/sys")
}

/// `stableCameraPath(devicePath, devRoot, sysRoot)`: the path settings keep for a camera.
/// `/dev/videoN` numbers follow the order cameras are plugged in, so a dock reconnecting can swap
/// them; this returns the camera's `/dev/v4l/by-id` link, or its by-path link (USB port) when
/// another node shares the by-id name, or the node when neither exists. A path that resolves to
/// nothing comes back as it was given.
pub fn stable_camera_path_in(device_path: &str, dev_root: &str, sys_root: &str) -> String {
    let node = FileInfo::new(device_path).canonical_file_path();
    if node.is_empty() {
        return device_path.to_owned();
    }
    let link_to = |directory: &str| -> String {
        let mut found = String::new();
        let links = qdir::entry_info_list(
            directory,
            &[],
            Filters::FILES | Filters::SYSTEM | Filters::NO_DOT_AND_DOT_DOT,
            SortFlags::NAME,
        );
        for link in links {
            if link.canonical_file_path() != node {
                continue;
            }
            // by-path also names each node after the USB revision it enumerated at, which
            // changes with the cable or port speed.
            if !link.file_name().contains("-usbv") {
                return link.absolute_file_path();
            }
            if found.is_empty() {
                found = link.absolute_file_path();
            }
        }
        found
    };
    // udev names a by-id link after the USB device's serial and the node's index, so the IR
    // sensor beside a laptop webcam, or two identical cameras without serial numbers, share one
    // name and it points at whichever node udev handled last.
    let class_directory = format!("{sys_root}/class/video4linux");
    let identity = |name: &str| -> String {
        let directory = format!("{class_directory}/{name}");
        let interface = FileInfo::new(format!("{directory}/device")).canonical_file_path();
        if interface.is_empty() {
            return String::new();
        }
        let usb_device = FileInfo::new(interface).absolute_path();
        [
            read_trimmed(&format!("{usb_device}/idVendor")),
            read_trimmed(&format!("{usb_device}/idProduct")),
            read_trimmed(&format!("{usb_device}/serial")),
            read_trimmed(&format!("{directory}/index")),
        ]
        .join(":")
    };
    let name = FileInfo::new(node.clone()).file_name();
    let own = identity(&name);
    let mut shared = false;
    let others =
        qdir::entry_list(&class_directory, &[], Filters::ALL_ENTRIES | Filters::NO_DOT_AND_DOT_DOT, SortFlags::NAME);
    for other in &others {
        shared = shared || (*other != name && !own.is_empty() && identity(other) == own);
    }
    if !shared {
        let link = link_to(&format!("{dev_root}/v4l/by-id"));
        if !link.is_empty() {
            return link;
        }
    }
    let link = link_to(&format!("{dev_root}/v4l/by-path"));
    if !link.is_empty() {
        return link;
    }
    node
}

/// `cameraNodePath`: the `/dev/videoN` node a saved camera path refers to now, or "" when it
/// isn't plugged in.
pub fn camera_node_path(path: &str) -> String {
    FileInfo::new(path).canonical_file_path()
}

/// `cameraIdNode`: the `/dev/videoN` node named in a Qt camera id (from `/dev/video` to the next
/// space), or "".
pub fn camera_id_node(id: &[u8]) -> String {
    let text = qstring::from_utf8(id);
    match text.find("/dev/video") {
        None => String::new(),
        Some(marker) => text[marker..].split(' ').next().unwrap_or_default().to_owned(),
    }
}

/// `cameraDeviceForPath`: the camera whose id names the node `path` resolves to now, or `None`
/// (the null `QCameraDevice`).
pub fn camera_device_for_path<'a>(devices: &'a [CameraDevice], path: &str) -> Option<&'a CameraDevice> {
    let node = camera_node_path(path);
    if node.is_empty() {
        return None;
    }
    devices.iter().find(|device| camera_id_node(&device.id) == node)
}

/// `FirstFrameTimestamp`: writes the camera's first frame time, once, to the `.ts` file beside
/// `camera.mp4` (`monotonic_microsec\trealtime_microsec\n<monotonic>\t<realtime>\n`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FirstFrameTimestamp {
    path: String,
    first_frame_us: i64,
}

impl FirstFrameTimestamp {
    /// `FirstFrameTimestamp(path)`.
    pub fn new(path: impl Into<String>) -> FirstFrameTimestamp {
        FirstFrameTimestamp { path: path.into(), first_frame_us: 0 }
    }

    /// `setPath`: a new file, waiting for its own first frame.
    pub fn set_path(&mut self, path: impl Into<String>) {
        self.path = path.into();
        self.first_frame_us = 0;
    }

    /// `recordFrameArrival`: true once the first frame's time is in the file (at once for every
    /// later frame); false without a path, for a time that is not positive, or when the file
    /// could not be written (through a `QSaveFile`, so it appears whole or not at all).
    pub fn record_frame_arrival(&mut self, monotonic_us: i64, realtime_us: i64) -> bool {
        if self.first_frame_us > 0 {
            return true;
        }
        if self.path.is_empty() || monotonic_us <= 0 || realtime_us <= 0 {
            return false;
        }
        let mut file = SaveFile::new(self.path.clone());
        if !file.open() {
            return false;
        }
        file.write(b"monotonic_microsec\trealtime_microsec\n");
        file.write(format!("{monotonic_us}\t{realtime_us}\n").as_bytes());
        if !file.commit() {
            return false;
        }
        self.first_frame_us = monotonic_us;
        true
    }

    /// `firstFrameUs()`: 0 until a frame was recorded.
    pub fn first_frame_us(&self) -> i64 {
        self.first_frame_us
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A directory under the system's temporary directory, removed when dropped.
    struct ScratchDirectory(std::path::PathBuf);

    impl Drop for ScratchDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &str, contents: &[u8]) {
        std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    fn link(target: &str, path: &str) {
        std::fs::create_dir_all(Path::new(path).parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(path);
        std::os::unix::fs::symlink(target, path).unwrap();
    }

    fn canonical(path: &str) -> String {
        FileInfo::new(path).canonical_file_path()
    }

    // test_recorder.cpp's cameraPathsSurviveRenumbering.
    #[test]
    fn camera_paths_survive_renumbering() {
        // A laptop webcam whose IR sensor shares its USB serial, so both index-0 nodes claim one
        // by-id name and udev left it on the IR node, and an Insta360 on a dock.
        let root = ScratchDirectory(
            std::env::temp_dir().join(format!("omareel-camera-paths-{}-{:?}", std::process::id(), std::thread::current().id())),
        );
        let root_path = root.0.to_string_lossy().into_owned();
        let dev = format!("{root_path}/dev");
        let sys = format!("{root_path}/sys");
        let node = |name: &str, interface: &str, index: i32| {
            write(&format!("{dev}/{name}"), b"");
            std::fs::create_dir_all(format!("{sys}/devices/{interface}")).unwrap();
            let class_directory = format!("{sys}/class/video4linux/{name}");
            write(&format!("{class_directory}/index"), format!("{index}\n").as_bytes());
            link(&format!("{sys}/devices/{interface}"), &format!("{class_directory}/device"));
        };
        write(&format!("{sys}/devices/usb7/7-1/idVendor"), b"636e\n");
        write(&format!("{sys}/devices/usb7/7-1/idProduct"), b"0bdb\n");
        write(&format!("{sys}/devices/usb7/7-1/serial"), b"200901010001\n");
        write(&format!("{sys}/devices/usb1/1-1.1/idVendor"), b"2e1a\n");
        write(&format!("{sys}/devices/usb1/1-1.1/idProduct"), b"4c04\n");
        node("video0", "usb7/7-1/7-1:1.0", 0);
        node("video1", "usb7/7-1/7-1:1.0", 1);
        node("video2", "usb7/7-1/7-1:1.2", 0);
        node("video4", "usb1/1-1.1/1-1.1:1.0", 0);
        let by_id = format!("{dev}/v4l/by-id/");
        let by_path = format!("{dev}/v4l/by-path/");
        link(&format!("{dev}/video2"), &format!("{by_id}usb-ASUS_FHD_webcam_200901010001-video-index0"));
        link(&format!("{dev}/video1"), &format!("{by_id}usb-ASUS_FHD_webcam_200901010001-video-index1"));
        link(&format!("{dev}/video4"), &format!("{by_id}usb-Insta360_Insta360_Link_2-video-index0"));
        link(&format!("{dev}/video0"), &format!("{by_path}platform-x-usbv2-0:1:1.0-video-index0"));
        link(&format!("{dev}/video0"), &format!("{by_path}platform-x-usb-0:1:1.0-video-index0"));
        link(&format!("{dev}/video2"), &format!("{by_path}platform-x-usb-0:1:1.2-video-index0"));
        link(&format!("{dev}/video4"), &format!("{by_path}platform-y-usb-0:1.1:1.0-video-index0"));

        let insta360 = stable_camera_path_in(&format!("{dev}/video4"), &dev, &sys);
        assert_eq!(insta360, format!("{by_id}usb-Insta360_Insta360_Link_2-video-index0"));
        assert_eq!(
            stable_camera_path_in(&format!("{dev}/video0"), &dev, &sys),
            format!("{by_path}platform-x-usb-0:1:1.0-video-index0")
        );
        assert_eq!(
            stable_camera_path_in(&format!("{dev}/video2"), &dev, &sys),
            format!("{by_path}platform-x-usb-0:1:1.2-video-index0")
        );
        assert_eq!(stable_camera_path_in(&insta360, &dev, &sys), insta360);
        assert_eq!(camera_node_path(&insta360), canonical(&format!("{dev}/video4")));

        // The dock comes back with a capture card ahead of the camera.
        std::fs::remove_file(format!("{dev}/video4")).unwrap();
        node("video6", "usb1/1-1.1/1-1.1:1.0", 0);
        link(&format!("{dev}/video6"), &format!("{by_id}usb-Insta360_Insta360_Link_2-video-index0"));
        assert_eq!(camera_node_path(&insta360), canonical(&format!("{dev}/video6")));

        // Unplugged: the saved path stays, and there is no node.
        std::fs::remove_file(format!("{dev}/video6")).unwrap();
        assert_eq!(camera_node_path(&insta360), "");
        assert_eq!(stable_camera_path_in(&insta360, &dev, &sys), insta360);

        assert_eq!(camera_id_node(b"/dev/video6"), "/dev/video6");
        assert_eq!(camera_id_node(b"v4l2 /dev/video6 usb"), "/dev/video6");
        assert_eq!(camera_id_node(b"obs-virtual"), "");
    }
}
