//! camera: `camera` against the pure parts of `CameraCapture.cpp` (README "camera"):
//! `stableCameraPath` and `cameraNodePath` over fake `/dev` and `/sys` trees, `cameraIdNode`,
//! `FirstFrameTimestamp`'s file, the format choice and `cameraDeviceForPath`. (The oracle's
//! `cameraFrameDeadlineUs` rows are left unchecked: the camera encoder no longer paces frames.)
//! The oracle built its cameras with Qt's private `QCameraDevicePrivate`; here they are the plain
//! data the Qt side will pass in.

use crate::hermetic::*;
use crate::replay::octal_mode;
use crate::support::*;
use omareel_core::compat::qjson::Value;
use omareel_record::camera::*;

/// The oracle's `fakeCameraFormat`: `[pixel format, width, height, minFps, maxFps]`, the rates
/// stored as `float`.
pub fn camera_format(format: &Value) -> CameraFormat {
    let at = |index: usize| format.to_array().get(index).cloned().unwrap_or_default();
    CameraFormat {
        pixel_format: at(0).to_string_or(""),
        width: at(1).to_int(0),
        height: at(2).to_int(0),
        min_frame_rate: at(3).to_double(0.0) as f32,
        max_frame_rate: at(4).to_double(0.0) as f32,
    }
}

/// The oracle's `fakeCamera`.
pub fn camera(id: &[u8], description: &str, formats: &Value) -> CameraDevice {
    CameraDevice {
        id: id.to_vec(),
        description: description.to_owned(),
        formats: formats.to_array().iter().map(camera_format).collect(),
    }
}

fn format_cells(format: Option<&CameraFormat>) -> String {
    match format {
        None => "null".into(),
        Some(f) => format!(
            "{} {}x{} {} {}",
            f.pixel_format,
            f.width,
            f.height,
            num(f64::from(f.min_frame_rate)),
            num(f64::from(f.max_frame_rate))
        ),
    }
}

fn link(target: &str, link: &str) {
    if let Some(parent) = std::path::Path::new(link).parent() {
        std::fs::create_dir_all(parent).expect("mkpath link directory");
    }
    let _ = std::fs::remove_file(link);
    std::os::unix::fs::symlink(target, link).unwrap_or_else(|e| panic!("could not link {link}: {e}"));
}

#[test]
fn stable_camera_path_txt() {
    let h = Hermetic::new("camera");
    let cases = read_json("camera/cases.json");
    let mut out = Text::new();
    for (tree, states) in cases.get("trees").to_object().iter() {
        let root = format!("{}/trees/{tree}", h.scratch());
        remove_path(&root);
        std::fs::create_dir_all(&root).expect("mkpath tree");
        let at = |relative: &str| {
            if relative.is_empty() || relative.starts_with('/') {
                relative.to_owned()
            } else {
                format!("{root}/{relative}")
            }
        };
        for state in states.to_array() {
            let state = state.to_object();
            for path in state.value("remove").to_array() {
                let _ = std::fs::remove_file(at(&path.to_string_or("")));
            }
            for (file, contents) in state.value("files").to_object().iter() {
                write_file(&at(file), contents.to_string_or(""));
            }
            for directory in state.value("dirs").to_array() {
                std::fs::create_dir_all(at(&directory.to_string_or(""))).expect("mkpath");
            }
            for (name, target) in state.value("links").to_object().iter() {
                link(&at(&target.to_string_or("")), &at(name));
            }
            out.section(&format!("{tree}: {}", state.value("label").to_string_or("")));
            for query in state.value("queries").to_array() {
                let query = query.to_string_or("");
                let path = at(&query);
                out.row([
                    quote(&query),
                    quote(&h.sanitize(&stable_camera_path_in(&path, &format!("{root}/dev"), &format!("{root}/sys")))),
                    quote(&h.sanitize(&camera_node_path(&path))),
                ]);
            }
        }
        remove_path(&root);
    }
    assert_golden("camera/stable-camera-path.txt", &out.into_string());
}

#[test]
fn camera_id_node_txt() {
    let cases = read_json("camera/cases.json");
    let mut out = Text::new();
    for id in cases.get("ids").to_array() {
        let id = id.to_string_or("");
        out.row([quote(&id), quote(&camera_id_node(id.as_bytes()))]);
    }
    out.row(["<bytes \\xff/dev/video3>".to_owned(), quote(&camera_id_node(b"\xff/dev/video3"))]);
    assert_golden("camera/camera-id-node.txt", &out.into_string());
}

#[test]
fn first_frame_timestamp_txt() {
    let h = Hermetic::new("camera");
    let files = format!("{}/timestamps", h.scratch());
    std::fs::create_dir_all(format!("{files}/directory.ts")).expect("mkpath");
    let mut out = Text::new();
    let show = |out: &mut Text, path: &str| {
        let bytes = std::path::Path::new(path).is_file().then(|| std::fs::read(path).ok()).flatten();
        out.row([
            "  file".to_owned(),
            quote(&h.sanitize(path)),
            bytes.map_or("-".to_owned(), |bytes| quote_bytes(&bytes)),
            octal_mode(path),
        ]);
    };
    let arrive = |out: &mut Text, timestamp: &mut FirstFrameTimestamp, monotonic: i64, realtime: i64| {
        let ok = timestamp.record_frame_arrival(monotonic, realtime);
        out.row([
            "recordFrameArrival".to_owned(),
            monotonic.to_string(),
            realtime.to_string(),
            boolean(ok).into(),
            timestamp.first_frame_us().to_string(),
        ]);
    };
    {
        out.section("the tag's test");
        let path = format!("{files}/camera.mp4.ts");
        let mut timestamp = FirstFrameTimestamp::new(path.clone());
        arrive(&mut out, &mut timestamp, 1_234_567, 9_876_543);
        arrive(&mut out, &mut timestamp, 2_345_678, 8_765_432);
        show(&mut out, &path);
        let next = format!("{files}/next.ts");
        timestamp.set_path(next.clone());
        out.row(["setPath".to_owned(), quote(&h.sanitize(&next)), timestamp.first_frame_us().to_string()]);
        arrive(&mut out, &mut timestamp, 42, 43);
        show(&mut out, &next);
        show(&mut out, &path);
    }
    {
        out.section("refused values");
        let path = format!("{files}/refused.ts");
        let mut timestamp = FirstFrameTimestamp::new(path.clone());
        arrive(&mut out, &mut timestamp, 0, 1);
        arrive(&mut out, &mut timestamp, -5, 1);
        arrive(&mut out, &mut timestamp, 1, 0);
        arrive(&mut out, &mut timestamp, 1, -1);
        show(&mut out, &path);
        arrive(&mut out, &mut timestamp, i64::MAX, i64::MAX);
        show(&mut out, &path);
    }
    {
        out.section("no path");
        let mut timestamp = FirstFrameTimestamp::default();
        arrive(&mut out, &mut timestamp, 1, 1);
        timestamp.set_path("");
        arrive(&mut out, &mut timestamp, 1, 1);
    }
    for (label, name) in [("missing directory", "missing/camera.mp4.ts"), ("path is a directory", "directory.ts")] {
        out.section(label);
        let path = format!("{files}/{name}");
        let mut timestamp = FirstFrameTimestamp::new(path.clone());
        arrive(&mut out, &mut timestamp, 10, 20);
        arrive(&mut out, &mut timestamp, 11, 21);
        show(&mut out, &path);
    }
    {
        out.section("replaces an existing file, keeping mode 0600");
        let path = format!("{files}/existing.ts");
        write_file(&path, "old contents\n");
        chmod(&path, 0o600);
        let mut timestamp = FirstFrameTimestamp::new(path.clone());
        arrive(&mut out, &mut timestamp, 7, 8);
        show(&mut out, &path);
    }
    remove_path(&files);
    assert_golden("camera/first-frame-timestamp.txt", &out.into_string());
}

#[test]
fn selected_format_and_device_for_path_txt() {
    let h = Hermetic::new("camera");
    let cases = read_json("camera/cases.json");
    let mut out = Text::new();
    for (name, formats) in cases.get("devices").to_object().iter() {
        let device = camera(b"/dev/video0", name, formats);
        let mut cells = vec![name.to_owned()];
        for height in [1080, 720, 0, 480] {
            cells.push(format!("{height}={}", format_cells(selected_format(&device, height))));
        }
        cells.push(format!("cameraCanRecord={}", boolean(camera_can_record(&device))));
        out.row(cells);
    }
    let null = CameraDevice::default();
    out.row([
        "<null device>".to_owned(),
        format!("1080={}", format_cells(selected_format(&null, 1080))),
        format!("cameraCanRecord={}", boolean(camera_can_record(&null))),
    ]);

    // cameraDeviceForPath compares the id's /dev/videoN with the path's canonical node, which is
    // a real /dev entry or nothing; only paths that resolve to nothing are hermetic.
    let mut paths = Text::new();
    let formats = omareel_core::compat::qjson::from_json(br#"[["Jpeg", 1920, 1080, 30, 30]]"#).expect("formats");
    let cameras = [camera(b"/dev/video0", "A", &formats), camera(b"v4l2 /dev/video2", "B", &formats)];
    for path in
        [String::new(), "/dev/oracle-camera-that-does-not-exist".into(), format!("{}/missing/video0", h.scratch())]
    {
        let device = camera_device_for_path(&cameras, &path);
        paths.row([
            quote(&h.sanitize(&path)),
            device.map_or("null".to_owned(), |device| quote(&String::from_utf8_lossy(&device.id))),
        ]);
    }
    assert_no_differences(
        [
            difference(
                "camera/selected-format.txt",
                &read_expected("camera/selected-format.txt"),
                out.into_string().as_bytes(),
            ),
            difference(
                "camera/camera-device-for-path.txt",
                &read_expected("camera/camera-device-for-path.txt"),
                paths.into_string().as_bytes(),
            ),
        ]
        .into_iter()
        .flatten()
        .collect(),
    );
}
