//! tests/test_project.cpp, which checked the C++ `Project` before it was deleted: defaults,
//! round trips, clip layouts and inserted videos, and the defaults migrations.

use super::*;
use crate::compat::geometry::PointF;
use crate::compat::qcolor::QColor;
use crate::compat::qdir::tests::TempDir;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};

/// `QCOMPARE` on doubles: fuzzy, and an expected (fuzzy) zero needs an actual one.
fn qcompare(actual: f64, expected: f64) -> bool {
    if expected.is_nan() {
        actual.is_nan()
    } else if expected.is_infinite() {
        actual == expected
    } else if q_fuzzy_is_null(expected) {
        q_fuzzy_is_null(actual)
    } else {
        q_fuzzy_compare(actual, expected)
    }
}

/// `QCOMPARE` on `QPointF`s, which is `QPointF::operator==`: each coordinate fuzzily, and one
/// that is zero only against a (fuzzy) zero.
fn qcompare_point(actual: PointF, expected: PointF) -> bool {
    let coordinate = |a: f64, b: f64| if a == 0.0 || b == 0.0 { q_fuzzy_is_null(a - b) } else { q_fuzzy_compare(a, b) };
    coordinate(actual.x, expected.x) && coordinate(actual.y, expected.y)
}

/// The `QJsonObject` the C++ test spelled out as an initializer list.
fn object(json: &str) -> Object {
    qjson::object_from_json(json.as_bytes()).expect("the test's JSON is an object")
}

fn clip(id: &str, in_: f64, out: f64, speed: f64, layout: &str) -> Clip {
    Clip { id: id.into(), in_, out, speed, layout: layout.into(), ..Clip::default() }
}

#[test]
fn defaults_and_round_trip() {
    let original = Project::defaults("Demo", 12.5);
    let json = original.to_json();
    assert_eq!(json.value("version").to_int(0), 1);
    assert_eq!(json.value("defaultsVersion").to_int(0), Project::CURRENT_DEFAULTS_VERSION);
    assert!(json.value("aspect").is_null());
    assert!(qcompare(json.value("clips").to_array()[0].to_object().value("out").to_double(0.0), 12.5));
    assert!(qcompare(original.frame.padding, 0.10));
    assert!(qcompare(original.frame.shadow.intensity, 0.25));
    assert!(qcompare(original.frame.shadow.blur, 40.0));
    assert!(qcompare(original.frame.shadow.distance, 10.0));
    assert!(!original.frame.border.enabled);
    assert!(qcompare(original.frame.border.width, 7.0));
    assert_eq!(original.frame.border.color, QColor::parse("#000000"));
    assert!(qcompare(original.frame.border.alpha, 1.0));
    assert!(qcompare(original.zoom_style.spring.mass, 2.25));
    assert!(qcompare(original.cursor.spring.stiffness, 470.0));
    assert!(qcompare(original.cursor.click_shrink, 0.8));
    assert_eq!(original.cursor.style, "light-arrow");
    assert_eq!(original.cursor.click_sound, "none");
    assert!(qcompare(original.zoom_style.motion_blur, 0.0));
    assert!(!original.keystrokes.enabled);
    assert_eq!(original.keystrokes.position, "bottom-center");
    assert!(qcompare(original.keystrokes.size, 1.0));
    assert!(original.keystrokes.show_only_shortcuts);
    assert_eq!(original.keystrokes.hold_ms, 900);
    assert_eq!(original.export_settings.height, 1080);
    assert_eq!(original.export_settings.quality, "social");
    assert_eq!(original.export_settings.gif.height, 480);
    assert!(!original.camera.enabled);
    assert_eq!(original.camera.position, "bottom-right");
    assert!(qcompare(original.camera.size, 0.25));
    assert_eq!(original.camera.shape, "round");
    assert!(qcompare(original.camera.radius, 16.0));
    assert!(!original.camera.flip_horizontal);
    assert_eq!(original.camera.rotation, 0);
    assert!(original.camera.shadow.enabled);
    assert!(qcompare(original.camera.shadow.intensity, 0.28));
    assert!(qcompare(original.camera.shadow.blur, 23.0));
    assert!(qcompare(original.camera.shadow.distance, 20.0));
    assert!(qcompare(original.camera.shadow.angle, 45.0));
    assert!(original.camera.border.enabled);
    assert!(qcompare(original.camera.border.width, 6.0));
    assert_eq!(original.camera.border.color, QColor::from_rgb(255, 255, 255));
    assert!(qcompare(original.camera.border.alpha, 0.7));
    assert!(qcompare(original.camera.scale_during_zoom, 0.7));
    assert!(qcompare_point(original.camera.offset, PointF::new(0.035, 0.035)));
    assert!(allowed_aspects().contains(&"3:4"));
    assert!(qcompare(*allowed_clip_speeds().last().expect("speeds"), 24.0));
    let directory = TempDir::new("project-round-trip");
    let path = directory.join("project.json");
    original.save(&path).expect("save");
    let (loaded, result) = Project::load(&path);
    assert_eq!(result, Ok(()));
    assert_eq!(loaded.to_json(), original.to_json());
    let written_frame = loaded.to_json().value("frame").to_object();
    assert!(written_frame.contains("border"));
    assert!(!written_frame.contains("inset"));
}

#[test]
fn clip_layouts_and_full_frame_camera() {
    let mut project = Project::defaults("Layouts", 6.0);
    assert_eq!(project.clips[0].layout, "default");
    assert_eq!(project.camera.full_frame, "fill");
    // A clip that keeps the default layout is written exactly as before layouts existed.
    let plain = project.to_json().value("clips").to_array()[0].to_object();
    assert!(!plain.contains("layout"));
    assert_eq!(plain.keys().collect::<Vec<_>>(), ["id", "in", "out", "speed"]);

    project.clips = vec![
        clip("intro", 0.0, 2.0, 1.0, "camera"),
        clip("main", 2.0, 5.0, 1.0, "default"),
        clip("quiet", 5.0, 6.0, 1.0, "screen"),
    ];
    project.camera.full_frame = "square".into();
    let loaded = Project::from_json(&project.to_json());
    assert_eq!(loaded.clips[0].layout, "camera");
    assert_eq!(loaded.clips[1].layout, "default");
    assert_eq!(loaded.clips[2].layout, "screen");
    assert_eq!(loaded.camera.full_frame, "square");
    assert_eq!(loaded.to_json(), project.to_json());

    let mut json = project.to_json();
    let mut clips = json.value("clips").to_array();
    let mut odd = clips[0].to_object();
    odd.insert("layout", "sideways");
    clips[0] = odd.into();
    json.insert("clips", clips);
    let mut camera = json.value("camera").to_object();
    camera.insert("fullFrame", "stretch");
    json.insert("camera", camera);
    let fallback = Project::from_json(&json);
    assert_eq!(fallback.clips[0].layout, "default");
    assert_eq!(fallback.camera.full_frame, "fill");
}

#[test]
fn inserted_videos_stay_inside_the_bundle() {
    assert_eq!(sanitized_media_path("media/demo.mp4"), "media/demo.mp4");
    assert_eq!(sanitized_media_path("media/./demo.mp4"), "media/demo.mp4");
    assert_eq!(sanitized_media_path("media/talk..final.mp4"), "media/talk..final.mp4");
    assert_eq!(sanitized_media_path("/home/demo.mp4"), "");
    assert_eq!(sanitized_media_path("media/../screen.mp4"), "");
    assert_eq!(sanitized_media_path("media/../../etc/passwd"), "");
    assert_eq!(sanitized_media_path("demo.mp4"), "");
    // QString(): a null string, which Rust has no separate value for.
    assert_eq!(sanitized_media_path(""), "");

    let mut project = Project::defaults("Inserts", 4.0);
    let insert = Clip { media: "media/demo.mp4".into(), fit: "framed".into(), ..clip("m", 0.5, 2.0, 1.0, "default") };
    project.clips.push(insert);
    let written = project.to_json().value("clips").to_array().last().expect("clips").to_object();
    assert_eq!(written.value("media").to_string_or(""), "media/demo.mp4");
    assert_eq!(written.value("fit").to_string_or(""), "framed");
    assert!(!project.to_json().value("clips").to_array()[0].to_object().contains("media"));
    let loaded = Project::from_json(&project.to_json());
    assert_eq!(loaded.clips.len(), 2);
    assert!(!loaded.clips[0].is_media());
    assert!(loaded.clips[1].is_media());
    assert_eq!(loaded.clips[1].fit, "framed");
    assert_eq!(loaded.to_json(), project.to_json());

    // An escaping path drops the clip instead of turning it into recording footage; an
    // unknown fit falls back to fit.
    let mut json = project.to_json();
    let mut clips = json.value("clips").to_array();
    let mut escaping = clips[1].to_object();
    escaping.insert("media", "../outside.mp4");
    let mut odd_fit = clips[1].to_object();
    odd_fit.insert("fit", "stretch");
    clips[1] = escaping.into();
    clips.push(odd_fit.into());
    json.insert("clips", clips);
    let guarded = Project::from_json(&json);
    assert_eq!(guarded.clips.len(), 2);
    assert!(guarded.clips[1].is_media());
    assert_eq!(guarded.clips[1].fit, "fit");
}

#[test]
fn migrates_legacy_inset_to_border() {
    let migrated = Project::from_json(&object(
        r##"{
            "frame": {"inset": {"enabled": true, "width": 5.0, "color": "#123456", "alpha": 0.6}},
            "camera": {"inset": {"enabled": true, "width": 3.0, "color": "#abcdef", "alpha": 0.8}}
        }"##,
    ));
    assert!(migrated.frame.border.enabled);
    assert!(qcompare(migrated.frame.border.width, 5.0));
    assert_eq!(migrated.frame.border.color, QColor::parse("#123456"));
    assert!(qcompare(migrated.frame.border.alpha, 0.6));
    assert!(migrated.camera.border.enabled);
    assert!(qcompare(migrated.camera.border.width, 3.0));
    let json = migrated.to_json();
    assert!(json.value("frame").to_object().contains("border"));
    assert!(!json.value("frame").to_object().contains("inset"));
    assert!(json.value("camera").to_object().contains("border"));
    assert!(!json.value("camera").to_object().contains("inset"));
}

#[test]
fn migrates_only_untouched_old_frame_defaults() {
    let directory = TempDir::new("project-frame-defaults");
    let write_and_load = |name: &str, frame: &str| {
        let path = directory.join(&format!("{name}.json"));
        let document = Object::from_iter([("frame", object(frame))]).to_json(Format::Indented);
        if std::fs::write(&path, document).is_err() {
            return Project::default();
        }
        match Project::load(&path) {
            (loaded, Ok(())) => loaded,
            (_, Err(_)) => Project::default(),
        }
    };

    let untouched = write_and_load(
        "untouched",
        r##"{
            "shadow": {"enabled": true, "intensity": 0.75, "blur": 20.0, "distance": 25.0},
            "inset": {"enabled": false, "width": 0.0, "color": "#000000", "alpha": 0.5}
        }"##,
    );
    assert_eq!(untouched.defaults_version, Project::CURRENT_DEFAULTS_VERSION);
    assert!(qcompare(untouched.frame.shadow.intensity, 0.25));
    assert!(qcompare(untouched.frame.shadow.blur, 40.0));
    assert!(qcompare(untouched.frame.shadow.distance, 10.0));
    assert!(qcompare(untouched.frame.border.width, 7.0));
    assert!(qcompare(untouched.frame.border.alpha, 1.0));

    let customised = write_and_load(
        "customised",
        r##"{
            "shadow": {"enabled": true, "intensity": 0.61, "blur": 20.0, "distance": 25.0},
            "inset": {"enabled": true, "width": 3.0, "color": "#123456", "alpha": 0.5}
        }"##,
    );
    assert_eq!(customised.defaults_version, Project::CURRENT_DEFAULTS_VERSION);
    assert!(qcompare(customised.frame.shadow.intensity, 0.61));
    assert!(qcompare(customised.frame.shadow.blur, 20.0));
    assert!(qcompare(customised.frame.shadow.distance, 25.0));
    assert!(customised.frame.border.enabled);
    assert!(qcompare(customised.frame.border.width, 3.0));
    assert_eq!(customised.frame.border.color, QColor::parse("#123456"));
    assert!(qcompare(customised.frame.border.alpha, 0.5));

    let toggled = write_and_load(
        "toggled",
        r##"{
            "shadow": {"enabled": false, "intensity": 0.75, "blur": 20.0, "distance": 25.0, "angle": 90.0},
            "inset": {"enabled": true, "width": 0.0, "color": "#000000", "alpha": 0.5}
        }"##,
    );
    assert_eq!(toggled.defaults_version, Project::CURRENT_DEFAULTS_VERSION);
    assert!(!toggled.frame.shadow.enabled);
    assert!(qcompare(toggled.frame.shadow.intensity, 0.75));
    assert!(qcompare(toggled.frame.shadow.blur, 20.0));
    assert!(qcompare(toggled.frame.shadow.distance, 25.0));
    assert!(toggled.frame.border.enabled);
    assert!(qcompare(toggled.frame.border.width, 0.0));
    assert!(qcompare(toggled.frame.border.alpha, 0.5));
}

#[test]
fn migrates_only_untouched_old_camera_defaults() {
    let load = |shadow: &str, border: &str, offset: &str| {
        let camera = Object::from_iter([("shadow", object(shadow)), ("border", object(border)), ("offset", object(offset))]);
        Project::from_json(&Object::from_iter([("defaultsVersion", Value::from(3)), ("camera", camera.into())]))
    };
    let old_offset = r#"{"x": 0.02, "y": 0.02}"#;

    let untouched = load(
        r#"{"enabled": false, "intensity": 0.55, "blur": 18, "distance": 18}"#,
        r##"{"enabled": false, "width": 2, "color": "#ffffff", "alpha": 0.7}"##,
        old_offset,
    );
    assert_eq!(untouched.defaults_version, Project::CURRENT_DEFAULTS_VERSION);
    // The values move to the new defaults, but a switched-off shadow or border stays off.
    assert!(!untouched.camera.shadow.enabled);
    assert!(qcompare(untouched.camera.shadow.intensity, 0.28));
    assert!(qcompare(untouched.camera.shadow.blur, 23.0));
    assert!(qcompare(untouched.camera.shadow.distance, 20.0));
    assert!(qcompare(untouched.camera.shadow.angle, 45.0));
    assert!(!untouched.camera.border.enabled);
    assert!(qcompare(untouched.camera.border.width, 6.0));
    assert!(qcompare_point(untouched.camera.offset, PointF::new(0.035, 0.035)));

    let customised = load(
        r#"{"enabled": true, "intensity": 0.4, "blur": 18, "distance": 18}"#,
        r##"{"enabled": true, "width": 4, "color": "#ffffff", "alpha": 0.7}"##,
        r#"{"x": 0.05, "y": 0.02}"#,
    );
    assert!(customised.camera.shadow.enabled);
    assert!(qcompare(customised.camera.shadow.intensity, 0.4));
    assert!(qcompare(customised.camera.shadow.distance, 18.0));
    assert!(customised.camera.border.enabled);
    assert!(qcompare(customised.camera.border.width, 4.0));
    assert!(qcompare_point(customised.camera.offset, PointF::new(0.05, 0.02)));

    // A current project keeps a width of 2 that was chosen on purpose.
    let camera = Object::from_iter([("border", object(r#"{"enabled": true, "width": 2}"#)), ("offset", object(old_offset))]);
    let current = Project::from_json(&Object::from_iter([
        ("defaultsVersion", Value::from(Project::CURRENT_DEFAULTS_VERSION)),
        ("camera", camera.into()),
    ]));
    assert!(qcompare(current.camera.border.width, 2.0));
    assert!(qcompare_point(current.camera.offset, PointF::new(0.02, 0.02)));
    assert!(qcompare(Project::from_json(&current.to_json()).camera.shadow.angle, 45.0));
}

#[test]
fn camera_crop_folds_into_shape() {
    // label, defaultsVersion (0: none), shape, crop ("": none), expected shape and radius.
    let rows: [(&str, i32, &str, &str, &str, f64); 8] = [
        ("round", 2, "round", "original", "round", 12.0),
        ("rounded square", 2, "rounded", "square", "square", 12.0),
        ("rounded original", 2, "rounded", "original", "rectangle", 12.0),
        ("sharp square", 2, "square", "square", "square", 0.0),
        ("sharp original", 2, "square", "original", "rectangle", 0.0),
        ("no defaults version", 0, "rounded", "original", "rectangle", 12.0),
        ("current square keeps its corners", 3, "square", "", "square", 12.0),
        ("current rectangle", 3, "rectangle", "", "rectangle", 12.0),
    ];
    for (row, defaults_version, shape, crop, expected_shape, expected_radius) in rows {
        let mut camera = Object::from_iter([("shape", Value::from(shape)), ("radius", Value::from(12.0))]);
        if !crop.is_empty() {
            camera.insert("crop", crop);
        }
        let mut root = Object::from_iter([("camera", camera)]);
        if defaults_version > 0 {
            root.insert("defaultsVersion", defaults_version);
        }
        let loaded = Project::from_json(&root);
        assert_eq!(loaded.camera.shape, expected_shape, "{row}");
        assert!(qcompare(loaded.camera.radius, expected_radius), "{row}: radius {}", loaded.camera.radius);
        let saved = loaded.to_json().value("camera").to_object();
        assert!(!saved.contains("crop"), "{row}");
        assert_eq!(Project::from_json(&loaded.to_json()).camera.shape, expected_shape, "{row}");
    }
}
