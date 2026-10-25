//! recordingbar: `recording_bar` against `RecordingBar.h`'s inline decisions and the arithmetic
//! copied from `RecordingBar.cpp` (README "recordingbar"): when the standby host exits, the
//! clamped drag, the elapsed text and countdown, the camera shape cycle, corners, clamping and
//! the saved position, and the standby host's first placement with `hyprctl -j monitors`
//! replayed.

use crate::hermetic::*;
use crate::replay::*;
use crate::support::*;
use omareel_core::compat::geometry::PointF;
use omareel_core::compat::qjson::{self, Format, Value};
use omareel_core::recording_preferences::RecordingPreferences;
use omareel_record::recording_bar::*;

fn point_text(point: Point) -> String {
    format!("{},{}", point.x, point.y)
}

fn point_f_text(point: PointF) -> String {
    format!("{},{}", num(point.x), num(point.y))
}

fn size_text(size: Size) -> String {
    format!("{}x{}", size.width, size.height)
}

#[test]
fn self_view_host_should_exit_txt() {
    let mut out = Text::new();
    for active in [false, true] {
        for quit in [false, true] {
            for owner in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
                for alive in [false, true] {
                    out.row([
                        boolean(active).to_owned(),
                        boolean(quit).into(),
                        owner.to_string(),
                        boolean(alive).into(),
                        boolean(self_view_host_should_exit(active, quit, owner, alive)).into(),
                    ]);
                }
            }
        }
    }
    assert_golden("recordingbar/self-view-host-should-exit.txt", &out.into_string());
}

/// The drag's delta goes through x86_64's qRound.
#[cfg(target_arch = "x86_64")]
#[test]
fn clamped_drag_txt() {
    let presses = [PointF::new(100.0, 100.0), PointF::new(0.5, 0.5), PointF::new(-0.5, 2.5)];
    let bubbles = [Point::new(400, 300), Point::new(2200, 1200), Point::new(20, 20), Point::new(0, 0)];
    let pointers = [
        PointF::new(100.0, 100.0),
        PointF::new(160.0, 140.0),
        PointF::new(500.0, 500.0),
        PointF::new(-100.0, -100.0),
        PointF::new(100.5, 100.5),
        PointF::new(99.5, 99.5),
        PointF::new(101.49999999999999, 98.50000000000001),
        PointF::new(1e9, -1e9),
        PointF::new(3000.25, 1700.75),
    ];
    let screens =
        [Size::new(2400, 1350), Size::new(160, 160), Size::new(100, 100), Size::new(0, 0), Size::new(-10, -10)];
    let mut out = Text::new();
    for &press in &presses {
        for &bubble in &bubbles {
            for &pointer in &pointers {
                for &screen in &screens {
                    for pixels in [160, 220, 0, -5] {
                        out.row([
                            point_f_text(press),
                            point_text(bubble),
                            point_f_text(pointer),
                            size_text(screen),
                            pixels.to_string(),
                            point_text(clamped_self_view_drag_position(press, bubble, pointer, screen, pixels)),
                        ]);
                    }
                }
            }
        }
    }
    assert_golden("recordingbar/clamped-drag.txt", &out.into_string());
}

#[test]
fn methods_txt() {
    let mut out = Text::new();
    for started in [0i64, 1_000_000, -5] {
        for now in [
            0i64,
            999_999,
            1_000_000,
            1_999_999,
            59_999_999,
            60_000_000,
            61_500_000,
            3_599_999_999,
            3_600_000_000,
            5_999_999_999,
            6_000_000_000,
            359_999_999_999,
            -1_000_000,
        ] {
            out.row(["elapsed".to_owned(), now.to_string(), started.to_string(), quote(&elapsed_text(now, started))]);
        }
    }
    for now in [1_000_000i64, 0] {
        for end in [
            0i64,
            1,
            999_999,
            1_000_000,
            1_000_001,
            2_000_000,
            2_000_001,
            3_000_000,
            3_000_001,
            4_000_000,
            4_000_001,
            100_000_000,
            -5,
        ] {
            out.row([
                "countdownValue".to_owned(),
                end.to_string(),
                now.to_string(),
                countdown_value(end, now).to_string(),
            ]);
        }
    }
    for shape in ["round", "square", "rectangle", ""] {
        for radius in [16, 0, 8, 80, -1] {
            let mut camera = CameraShape { shape: shape.into(), radius };
            let mut cells = vec!["cycleCameraShape".to_owned(), quote(shape), radius.to_string()];
            for _ in 0..4 {
                camera.cycle();
                cells.push(format!("{} {}", quote(&camera.shape), camera.radius));
            }
            out.row(cells);
        }
    }
    for screen in
        [Size::new(2400, 1350), Size::new(1920, 1080), Size::new(200, 200), Size::new(0, 0), Size::new(100, 0)]
    {
        for pixels in [120, 160, 220] {
            let mut geometry = SelfViewGeometry { pixels, screen_size: screen, ..SelfViewGeometry::default() };
            let mut cells = vec!["snapSelfViewToNextCorner".to_owned(), size_text(screen), pixels.to_string()];
            for _ in 0..5 {
                geometry.snap_to_next_corner();
                cells.push(format!("{}:{}", geometry.corner, point_text(Point::new(geometry.x, geometry.y))));
            }
            out.row(cells);
        }
    }
    let positions = [
        Point::new(16, 16),
        Point::new(0, 0),
        Point::new(-50, 3000),
        Point::new(2224, 1174),
        Point::new(1000, 500),
        Point::new(17, 15),
    ];
    for screen in
        [Size::new(2400, 1350), Size::new(1920, 1080), Size::new(192, 192), Size::new(100, 100), Size::INVALID]
    {
        for pixels in [120, 160, 220] {
            for position in positions {
                let mut geometry = SelfViewGeometry { pixels, x: 500, y: 400, ..SelfViewGeometry::default() };
                geometry.set_screen_size(screen);
                let resized = Point::new(geometry.x, geometry.y);
                geometry.set_position(position.x, position.y);
                let placed = Point::new(geometry.x, geometry.y);
                let (saved_x, saved_y) = geometry.saved_position().unwrap_or((-7.0, -7.0));
                out.row([
                    "position".to_owned(),
                    size_text(screen),
                    pixels.to_string(),
                    point_text(resized),
                    point_text(position),
                    point_text(placed),
                    num(saved_x),
                    num(saved_y),
                ]);
            }
        }
    }
    let webcams = [
        r#"{}"#,
        r#"{"shape": "square"}"#,
        r#"{"shape": "square", "radius": 0}"#,
        r#"{"shape": "rectangle", "radius": 40}"#,
        r#"{"shape": "Square"}"#,
        r#"{"radius": 81}"#,
        r#"{"radius": -3}"#,
        r#"{"radius": 12.5}"#,
        r#"{"radius": "20"}"#,
        r#"{"shape": 1, "radius": 1e10}"#,
    ];
    for webcam in webcams {
        let preferences = RecordingPreferences {
            webcam: qjson::object_from_json(webcam.as_bytes()).expect("an object"),
            ..RecordingPreferences::default()
        };
        let shape = CameraShape::from_preferences(&preferences);
        out.row([
            "shapeFromPreferences".to_owned(),
            Value::Object(preferences.webcam.clone()).to_json(Format::Compact),
            quote(&shape.shape),
            shape.radius.to_string(),
        ]);
    }
    assert_golden("recordingbar/methods.txt", &out.into_string());
}

/// The placement goes through x86_64's qRound.
#[cfg(target_arch = "x86_64")]
#[test]
fn standby_placement_txt() {
    let h = Hermetic::new("recordingbar");
    let files = [
        "hyprctl/monitors/crafted-two.json",
        "hyprctl/monitors/crafted-focus-none.json",
        "hyprctl/monitors/crafted-focus-both.json",
        "hyprctl/monitors/crafted-scales.json",
        "hyprctl/monitors/crafted-odd-size.json",
        "hyprctl/monitors/crafted-wrong-types.json",
        "hyprctl/monitors/crafted-empty-array.json",
        "hyprctl/monitors/crafted-garbage.txt",
        "recorder/monitors-tiny.json",
        "hyprctl/monitors/real-desktop.json",
    ];
    let preferences =
        [("M", 1.0, 1.0), ("S", 0.0, 0.0), ("L", 0.5, 0.25), ("XL", 0.3333333333333333, 2.0), ("l", -1.0, 0.5)];
    let mut out = Text::new();
    for file in files {
        for &(size, x, y) in &preferences {
            for status in [0, 1] {
                if status != 0 && size != "M" {
                    continue;
                }
                let mut replay = Replay::new(&format!("{}/replay", h.scratch()));
                replay.answer(
                    "hyprctl",
                    &["-j", "monitors"],
                    &Answer { status, ..Answer::stdout(read_corpus(file)) },
                    0,
                );
                replay.activate(&h);
                let prefs = RecordingPreferences {
                    self_view_size: size.into(),
                    self_view_x: x,
                    self_view_y: y,
                    ..RecordingPreferences::default()
                };
                let placement = standby_placement(&prefs);
                out.row([
                    file.to_owned(),
                    status.to_string(),
                    quote(size),
                    num(x),
                    num(y),
                    quote(&placement.recorded_monitor),
                    Value::Object(placement.state).to_json(Format::Compact),
                ]);
            }
        }
    }
    assert_golden("recordingbar/standby-placement.txt", &out.into_string());
}
