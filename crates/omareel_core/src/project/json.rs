//! Reading, migrating and writing `project.json` (`Project.cpp`).

use super::model::*;
use super::styles::*;
use crate::compat::cpp;
use crate::compat::geometry::{PointF, RectF};
use crate::compat::qcolor::QColor;
use crate::compat::qdir;
use crate::compat::qfile;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qround::{q_fuzzy_compare, q_fuzzy_is_null};
use crate::compat::qsavefile;

fn parse_color(value: &Value, fallback: &QColor) -> QColor {
    let parsed = QColor::parse(&value.to_string_or(""));
    if parsed.is_valid() { parsed } else { *fallback }
}

fn black() -> QColor {
    QColor::from_rgb(0, 0, 0)
}

fn obj<const N: usize>(members: [(&str, Value); N]) -> Value {
    Value::Object(Object::from_iter(members))
}

pub fn spring_to_json(s: &Spring) -> Value {
    obj([("mass", s.mass.into()), ("stiffness", s.stiffness.into()), ("damping", s.damping.into())])
}

pub fn spring_from_json(json: &Object, fallback: &Spring) -> Spring {
    Spring {
        mass: json.value("mass").to_double(fallback.mass),
        stiffness: json.value("stiffness").to_double(fallback.stiffness),
        damping: json.value("damping").to_double(fallback.damping),
    }
}

pub fn allowed_clip_speeds() -> Vec<f64> {
    vec![0.5, 0.75, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0, 3.0, 4.0, 8.0, 16.0, 24.0]
}

pub fn allowed_clip_layouts() -> [&'static str; 3] {
    ["default", "camera", "screen"]
}

pub fn allowed_clip_fits() -> [&'static str; 3] {
    ["fit", "fill", "framed"]
}

/// The bundle-relative path of an inserted video, or empty unless the path stays inside
/// media/.
pub fn sanitized_media_path(path: &str) -> String {
    if path.is_empty() || qdir::is_absolute_path(path) || path.contains('\\') {
        return String::new();
    }
    let clean = qdir::clean_path(path);
    if !clean.starts_with("media/") || clean.split('/').any(|part| part == "..") {
        return String::new();
    }
    clean
}

pub fn allowed_aspects() -> [&'static str; 7] {
    ["auto", "16:9", "1:1", "4:3", "9:16", "3:4", "4:5"]
}

impl Project {
    /// A new recording's project: one clip over the whole recording and the default
    /// gradient.
    pub fn defaults(name: &str, duration: f64) -> Project {
        let mut result = Project { name: name.to_owned(), ..Project::default() };
        result.clips = vec![Clip { id: "c1".into(), in_: 0.0, out: duration, speed: 1.0, ..Clip::default() }];
        result.background.gradient.stops = vec![
            GradientStop { color: QColor::parse("#ff8a00"), position: 0.0 },
            GradientStop { color: QColor::parse("#e52e71"), position: 1.0 },
        ];
        result
    }

    /// Brings a project written with older defaults up to date, replacing only values
    /// nobody changed. The flag says whether anything was written.
    pub fn migrate_defaults(json: &Object) -> (Object, bool) {
        let mut migrated = json.clone();
        let stored_version = json.value("defaultsVersion").to_int(0);
        let did_change = stored_version < Project::CURRENT_DEFAULTS_VERSION;
        if stored_version < 2 {
            let mut frame = migrated.value("frame").to_object();
            let shadow = frame.value("shadow").to_object();
            let intensity = shadow.value("intensity").to_double(shadow.value("opacity").to_double(0.0));
            let blur = shadow.value("blur").to_double(0.0);
            let distance = shadow.value("distance").to_double(shadow.value("offsetY").to_double(0.0));
            let old_shadow_defaults = !shadow.is_empty()
                && shadow.value("enabled").to_bool(true)
                && q_fuzzy_compare(intensity, 0.75)
                && q_fuzzy_compare(blur, 20.0)
                && q_fuzzy_compare(distance, 25.0)
                && q_fuzzy_compare(shadow.value("angle").to_double(90.0), 90.0);
            if old_shadow_defaults {
                let d = Shadow::default();
                frame.insert(
                    "shadow",
                    obj([
                        ("enabled", d.enabled.into()),
                        ("intensity", d.intensity.into()),
                        ("blur", d.blur.into()),
                        ("distance", d.distance.into()),
                        ("angle", d.angle.into()),
                    ]),
                );
            }

            let border_key = if frame.contains("border") { "border" } else { "inset" };
            let border = frame.value(border_key).to_object();
            let old_border_defaults = !border.is_empty()
                && !border.value("enabled").to_bool(false)
                && q_fuzzy_is_null(border.value("width").to_double(0.0))
                && parse_color(border.value("color"), &black()) == black()
                && q_fuzzy_compare(border.value("alpha").to_double(0.0), 0.5);
            if old_border_defaults {
                let d = Border::default();
                frame.remove("inset");
                frame.insert(
                    "border",
                    obj([
                        ("enabled", d.enabled.into()),
                        ("width", d.width.into()),
                        ("color", d.color.name().into()),
                        ("alpha", d.alpha.into()),
                    ]),
                );
            }
            migrated.insert("frame", frame);
        }
        // Version 3 folds the camera's crop (square or original) into its shape. The old
        // "rounded" and "square" shapes differed only in corners, so a sharp square is now a
        // radius of 0.
        if stored_version < 3 && migrated.value("camera").is_object() {
            let mut camera = migrated.value("camera").to_object();
            let shape = camera.value("shape").to_string_or("round");
            if shape == "rounded" || shape == "square" {
                if shape == "square" {
                    camera.insert("radius", 0);
                }
                let crop_square = camera.value("crop").to_string_or("") == "square";
                camera.insert("shape", if crop_square { "square" } else { "rectangle" });
            }
            camera.remove("crop");
            migrated.insert("camera", camera);
        }
        // Version 4 gives the camera a softer, wider shadow and border and measures its corner
        // gap from the shorter side. Like version 2 it only replaces values nobody changed,
        // and leaves the shadow and border switched on or off as they were.
        if stored_version < 4 && migrated.value("camera").is_object() {
            let mut camera = migrated.value("camera").to_object();
            let d = Camera::default();
            let mut shadow = camera.value("shadow").to_object();
            if q_fuzzy_compare(shadow.value("intensity").to_double(0.0), 0.55)
                && q_fuzzy_compare(shadow.value("blur").to_double(0.0), 18.0)
                && q_fuzzy_compare(shadow.value("distance").to_double(0.0), 18.0)
            {
                shadow.insert("intensity", d.shadow.intensity);
                shadow.insert("blur", d.shadow.blur);
                shadow.insert("distance", d.shadow.distance);
                camera.insert("shadow", shadow);
            }
            let mut border = camera.value("border").to_object();
            if q_fuzzy_compare(border.value("width").to_double(0.0), 2.0) {
                border.insert("width", d.border.width);
                camera.insert("border", border);
            }
            let offset = camera.value("offset").to_object();
            if q_fuzzy_compare(offset.value("x").to_double(0.0), 0.02)
                && q_fuzzy_compare(offset.value("y").to_double(0.0), 0.02)
            {
                camera.insert("offset", obj([("x", d.offset.x.into()), ("y", d.offset.y.into())]));
            }
            migrated.insert("camera", camera);
        }
        if did_change {
            migrated.insert("defaultsVersion", Project::CURRENT_DEFAULTS_VERSION);
        }
        (migrated, did_change)
    }

    /// Reads a project, filling in defaults and dropping values outside what the editor
    /// offers.
    pub fn from_json(root: &Object) -> Project {
        let (source, _) = Project::migrate_defaults(root);
        let mut p = Project::defaults(&source.value("name").to_string_or("Untitled Recording"), 0.0);
        p.version = source.value("version").to_int(1);
        p.defaults_version = source.value("defaultsVersion").to_int(Project::CURRENT_DEFAULTS_VERSION);
        let aspect = if source.value("aspect").is_null() { "auto".to_owned() } else { source.value("aspect").to_string_or("auto") };
        if allowed_aspects().contains(&aspect.as_str()) {
            p.aspect = aspect;
        }
        let crop = source.value("crop").to_object();
        p.crop = RectF::new(
            crop.value("x").to_double(0.0),
            crop.value("y").to_double(0.0),
            crop.value("w").to_double(1.0),
            crop.value("h").to_double(1.0),
        );

        p.clips.clear();
        for value in source.value("clips").to_array() {
            let o = value.to_object();
            let mut clip = Clip {
                id: o.value("id").to_string_or(""),
                in_: o.value("in").to_double(0.0),
                out: o.value("out").to_double(0.0),
                speed: o.value("speed").to_double(1.0),
                ..Clip::default()
            };
            let layout = o.value("layout").to_string_or("");
            if allowed_clip_layouts().contains(&layout.as_str()) {
                clip.layout = layout;
            }
            // A media path that leaves media/ is dropped with its clip rather than read as
            // footage of the recording.
            if o.contains("media") {
                clip.media = sanitized_media_path(&o.value("media").to_string_or(""));
                if clip.media.is_empty() {
                    continue;
                }
                let fit = o.value("fit").to_string_or("");
                if allowed_clip_fits().contains(&fit.as_str()) {
                    clip.fit = fit;
                }
            }
            p.clips.push(clip);
        }
        p.zooms.clear();
        for value in source.value("zooms").to_array() {
            let o = value.to_object();
            let target = o.value("target");
            let mut z = ZoomSegment {
                id: o.value("id").to_string_or(""),
                start: o.value("start").to_double(0.0),
                end: o.value("end").to_double(0.0),
                level: o.value("level").to_double(2.0),
                automatic_target: !target.is_object(),
                ..ZoomSegment::default()
            };
            if let Some(t) = target.as_object() {
                z.target = PointF::new(t.value("x").to_double(0.5), t.value("y").to_double(0.5));
            }
            p.zooms.push(z);
        }
        let zs = source.value("zoomStyle").to_object();
        p.zoom_style.spring = spring_from_json(&zs.value("spring").to_object(), &p.zoom_style.spring);
        p.zoom_style.snap_to_edges_ratio = zs.value("snapToEdgesRatio").to_double(0.25);
        p.zoom_style.instant_animation = zs.value("instantAnimation").to_bool(false);
        p.zoom_style.motion_blur = cpp::clamp(zs.value("motionBlur").to_double(0.0), 0.0, 1.0);

        let bg = source.value("background").to_object();
        p.background.type_ = bg.value("type").to_string_or(&p.background.type_);
        p.background.wallpaper = bg.value("wallpaper").to_string_or(&p.background.wallpaper);
        p.background.color = parse_color(bg.value("color"), &p.background.color);
        p.background.image = bg.value("image").to_string_or("");
        p.background.blur = bg.value("blur").to_double(0.0);
        let gradient = bg.value("gradient").to_object();
        p.background.gradient.angle = gradient.value("angle").to_double(135.0);
        if gradient.contains("stops") {
            p.background.gradient.stops.clear();
            for v in gradient.value("stops").to_array() {
                let a = v.to_array();
                if a.len() >= 2 {
                    p.background.gradient.stops.push(GradientStop { color: parse_color(&a[0], &black()), position: a[1].to_double(0.0) });
                }
            }
        }

        let f = source.value("frame").to_object();
        p.frame.padding = f.value("padding").to_double(0.10);
        p.frame.radius = f.value("radius").to_double(12.0);
        let sh = f.value("shadow").to_object();
        p.frame.shadow.enabled = sh.value("enabled").to_bool(p.frame.shadow.enabled);
        p.frame.shadow.intensity = sh.value("intensity").to_double(sh.value("opacity").to_double(p.frame.shadow.intensity));
        p.frame.shadow.blur = sh.value("blur").to_double(p.frame.shadow.blur);
        p.frame.shadow.distance = sh.value("distance").to_double(sh.value("offsetY").to_double(p.frame.shadow.distance));
        p.frame.shadow.angle = sh.value("angle").to_double(p.frame.shadow.angle);
        let border = if f.contains("border") { f.value("border") } else { f.value("inset") }.to_object();
        p.frame.border.enabled = border.value("enabled").to_bool(p.frame.border.enabled);
        p.frame.border.width = border.value("width").to_double(p.frame.border.width);
        p.frame.border.color = parse_color(border.value("color"), &p.frame.border.color);
        p.frame.border.alpha = border.value("alpha").to_double(p.frame.border.alpha);

        let c = source.value("cursor").to_object();
        p.cursor.visible = c.value("visible").to_bool(true);
        p.cursor.size = c.value("size").to_double(1.5);
        p.cursor.smoothing = if c.value("smoothing").is_bool() {
            c.value("smoothing").to_bool(false)
        } else {
            c.value("smoothing").to_double(0.8) > 0.0
        };
        p.cursor.spring = spring_from_json(&c.value("spring").to_object(), &p.cursor.spring);
        p.cursor.click_effect = c.value("clickEffect").to_string_or("ripple");
        p.cursor.click_shrink = c.value("clickShrink").to_double(0.8);
        p.cursor.rotate_on_x_movement_ratio = c.value("rotateOnXMovementRatio").to_double(0.5);
        p.cursor.hide_when_idle_ms = if c.value("hideWhenIdleMs").is_null() { -1 } else { c.value("hideWhenIdleMs").to_int(-1) };
        p.cursor.style = c.value("style").to_string_or("light-arrow");
        if !["light-arrow", "dark-arrow", "dot", "hand"].contains(&p.cursor.style.as_str()) {
            p.cursor.style = "light-arrow".into();
        }
        p.cursor.ring_color = parse_color(c.value("ringColor"), &p.cursor.ring_color);
        p.cursor.click_sound = c.value("clickSound").to_string_or("none");
        if p.cursor.click_sound != "soft" {
            p.cursor.click_sound = "none".into();
        }

        let keystrokes = source.value("keystrokes").to_object();
        p.keystrokes.enabled = keystrokes.value("enabled").to_bool(false);
        p.keystrokes.position = keystrokes.value("position").to_string_or("bottom-center");
        const KEYSTROKE_POSITIONS: [&str; 6] = ["top-left", "top-center", "top-right", "bottom-left", "bottom-center", "bottom-right"];
        if !KEYSTROKE_POSITIONS.contains(&p.keystrokes.position.as_str()) {
            p.keystrokes.position = "bottom-center".into();
        }
        p.keystrokes.size = cpp::clamp(keystrokes.value("size").to_double(1.0), 0.5, 2.0);
        p.keystrokes.show_only_shortcuts = keystrokes.value("showOnlyShortcuts").to_bool(true);
        p.keystrokes.hold_ms = cpp::clamp(keystrokes.value("holdMs").to_int(900), 100, 5000);

        let a = source.value("audio").to_object();
        p.audio.desktop = a.value("desktop").to_bool(true);
        p.audio.mic = a.value("mic").to_bool(true);
        p.audio.volume = a.value("volume").to_double(1.0);
        let camera = source.value("camera").to_object();
        p.camera.enabled = camera.value("enabled").to_bool(false);
        p.camera.position = camera.value("position").to_string_or("bottom-right");
        p.camera.size = camera.value("size").to_double(0.25);
        let camera_shape = camera.value("shape").to_string_or("round");
        if ["round", "square", "rectangle"].contains(&camera_shape.as_str()) {
            p.camera.shape = camera_shape;
        }
        p.camera.radius = camera.value("radius").to_double(16.0);
        p.camera.flip_horizontal = if camera.contains("flipHorizontal") {
            camera.value("flipHorizontal").to_bool(false)
        } else {
            camera.value("mirror").to_bool(false)
        };
        let camera_rotation = camera.value("rotation").to_int(0);
        if [0, 90, 180, 270].contains(&camera_rotation) {
            p.camera.rotation = camera_rotation;
        }
        let camera_shadow_value = camera.value("shadow");
        if let Some(camera_shadow) = camera_shadow_value.as_object() {
            let s = &mut p.camera.shadow;
            s.enabled = camera_shadow.value("enabled").to_bool(s.enabled);
            s.intensity = camera_shadow.value("intensity").to_double(s.intensity);
            s.blur = camera_shadow.value("blur").to_double(s.blur);
            s.distance = camera_shadow.value("distance").to_double(s.distance);
            s.angle = camera_shadow.value("angle").to_double(s.angle);
        } else {
            p.camera.shadow.enabled = camera_shadow_value.to_bool(p.camera.shadow.enabled);
        }
        let camera_border = if camera.contains("border") { camera.value("border") } else { camera.value("inset") }.to_object();
        p.camera.border.enabled = camera_border.value("enabled").to_bool(p.camera.border.enabled);
        p.camera.border.width = camera_border.value("width").to_double(p.camera.border.width);
        p.camera.border.color = parse_color(camera_border.value("color"), &p.camera.border.color);
        p.camera.border.alpha = camera_border.value("alpha").to_double(p.camera.border.alpha);
        p.camera.scale_during_zoom = camera.value("scaleDuringZoom").to_double(0.7);
        let camera_offset = camera.value("offset").to_object();
        p.camera.offset = PointF::new(
            camera_offset.value("x").to_double(p.camera.offset.x),
            camera_offset.value("y").to_double(p.camera.offset.y),
        );
        if camera.value("fullFrame").to_string_or("") == "square" {
            p.camera.full_frame = "square".into();
        }
        let e = source.value("export").to_object();
        p.export_settings.format = e.value("format").to_string_or("mp4");
        p.export_settings.fps = e.value("fps").to_int(60);
        p.export_settings.height = e.value("height").to_int(1080);
        p.export_settings.quality = e.value("quality").to_string_or("social");
        let g = e.value("gif").to_object();
        p.export_settings.gif.fps = g.value("fps").to_int(15);
        p.export_settings.gif.height = g.value("height").to_int(480);
        p.export_settings.gif.quality = g.value("quality").to_string_or("studio");
        p.export_settings.gif.loop_ = g.value("loop").to_bool(true);
        p
    }

    pub fn to_json(&self) -> Object {
        let clips: Vec<Value> = self
            .clips
            .iter()
            .map(|c| {
                let mut clip = Object::from_iter([
                    ("id", Value::from(c.id.as_str())),
                    ("in", c.in_.into()),
                    ("out", c.out.into()),
                    ("speed", c.speed.into()),
                ]);
                // Clips that keep the default layout stay as they were written before
                // layouts existed.
                if c.layout != "default" {
                    clip.insert("layout", c.layout.as_str());
                }
                if c.is_media() {
                    clip.insert("media", c.media.as_str());
                    clip.insert("fit", c.fit.as_str());
                }
                clip.into()
            })
            .collect();
        let zooms: Vec<Value> = self
            .zooms
            .iter()
            .map(|z| {
                let target =
                    if z.automatic_target { Value::from("auto") } else { obj([("x", z.target.x.into()), ("y", z.target.y.into())]) };
                obj([
                    ("id", z.id.as_str().into()),
                    ("start", z.start.into()),
                    ("end", z.end.into()),
                    ("level", z.level.into()),
                    ("target", target),
                ])
            })
            .collect();
        let stops: Vec<Value> = self
            .background
            .gradient
            .stops
            .iter()
            .map(|s| Value::Array(vec![s.color.name().into(), s.position.into()]))
            .collect();
        let bg = &self.background;
        let background = obj([
            ("type", bg.type_.as_str().into()),
            ("wallpaper", bg.wallpaper.as_str().into()),
            ("gradient", obj([("angle", bg.gradient.angle.into()), ("stops", stops.into())])),
            ("color", bg.color.name().into()),
            ("image", if bg.image.is_empty() { Value::Null } else { bg.image.as_str().into() }),
            ("blur", bg.blur.into()),
        ]);
        let shadow = |s: &Shadow| {
            obj([
                ("enabled", s.enabled.into()),
                ("intensity", s.intensity.into()),
                ("blur", s.blur.into()),
                ("distance", s.distance.into()),
                ("angle", s.angle.into()),
            ])
        };
        let border = |b: &Border| {
            obj([
                ("enabled", b.enabled.into()),
                ("width", b.width.into()),
                ("color", b.color.name().into()),
                ("alpha", b.alpha.into()),
            ])
        };
        let frame = obj([
            ("padding", self.frame.padding.into()),
            ("radius", self.frame.radius.into()),
            ("shadow", shadow(&self.frame.shadow)),
            ("border", border(&self.frame.border)),
        ]);
        let c = &self.cursor;
        let cursor = obj([
            ("visible", c.visible.into()),
            ("size", c.size.into()),
            ("smoothing", c.smoothing.into()),
            ("spring", spring_to_json(&c.spring)),
            ("clickEffect", c.click_effect.as_str().into()),
            ("clickShrink", c.click_shrink.into()),
            ("rotateOnXMovementRatio", c.rotate_on_x_movement_ratio.into()),
            ("hideWhenIdleMs", if c.hide_when_idle_ms < 0 { Value::Null } else { c.hide_when_idle_ms.into() }),
            ("style", c.style.as_str().into()),
            ("ringColor", c.ring_color.name().into()),
            ("clickSound", c.click_sound.as_str().into()),
        ]);
        let k = &self.keystrokes;
        let a = &self.audio;
        let cam = &self.camera;
        let e = &self.export_settings;
        Object::from_iter([
            ("version", Value::from(self.version)),
            ("defaultsVersion", self.defaults_version.into()),
            ("name", self.name.as_str().into()),
            ("aspect", if self.aspect == "auto" { Value::Null } else { self.aspect.as_str().into() }),
            (
                "crop",
                obj([
                    ("x", self.crop.x.into()),
                    ("y", self.crop.y.into()),
                    ("w", self.crop.width.into()),
                    ("h", self.crop.height.into()),
                ]),
            ),
            ("clips", clips.into()),
            ("zooms", zooms.into()),
            (
                "zoomStyle",
                obj([
                    ("spring", spring_to_json(&self.zoom_style.spring)),
                    ("snapToEdgesRatio", self.zoom_style.snap_to_edges_ratio.into()),
                    ("instantAnimation", self.zoom_style.instant_animation.into()),
                    ("motionBlur", self.zoom_style.motion_blur.into()),
                ]),
            ),
            ("background", background),
            ("frame", frame),
            ("cursor", cursor),
            (
                "keystrokes",
                obj([
                    ("enabled", k.enabled.into()),
                    ("position", k.position.as_str().into()),
                    ("size", k.size.into()),
                    ("showOnlyShortcuts", k.show_only_shortcuts.into()),
                    ("holdMs", k.hold_ms.into()),
                ]),
            ),
            ("audio", obj([("desktop", a.desktop.into()), ("mic", a.mic.into()), ("volume", a.volume.into())])),
            (
                "camera",
                obj([
                    ("enabled", cam.enabled.into()),
                    ("position", cam.position.as_str().into()),
                    ("size", cam.size.into()),
                    ("shape", cam.shape.as_str().into()),
                    ("radius", cam.radius.into()),
                    ("flipHorizontal", cam.flip_horizontal.into()),
                    ("rotation", cam.rotation.into()),
                    ("shadow", shadow(&cam.shadow)),
                    ("border", border(&cam.border)),
                    ("scaleDuringZoom", cam.scale_during_zoom.into()),
                    ("offset", obj([("x", cam.offset.x.into()), ("y", cam.offset.y.into())])),
                    ("fullFrame", cam.full_frame.as_str().into()),
                ]),
            ),
            (
                "export",
                obj([
                    ("format", e.format.as_str().into()),
                    ("fps", e.fps.into()),
                    ("height", e.height.into()),
                    ("quality", e.quality.as_str().into()),
                    (
                        "gif",
                        obj([
                            ("fps", e.gif.fps.into()),
                            ("height", e.gif.height.into()),
                            ("quality", e.gif.quality.as_str().into()),
                            ("loop", e.gif.loop_.into()),
                        ]),
                    ),
                ]),
            ),
        ])
    }

    /// Reads `project.json`, writing it back when a migration changed it. On failure the
    /// error comes with a bare project, as the C++ returned one.
    pub fn load(path: &str) -> (Project, Result<(), String>) {
        let bytes = match qfile::read_all(path) {
            Ok(bytes) => bytes,
            Err(error) => return (Project::default(), Err(error)),
        };
        let object = match qjson::from_json(&bytes) {
            Ok(Value::Object(object)) => object,
            Ok(_) => return (Project::default(), Err("no error occurred".into())),
            Err(error) => return (Project::default(), Err(error.error_string().into())),
        };
        let (json, migrated) = Project::migrate_defaults(&object);
        let project = Project::from_json(&json);
        if migrated {
            if let Err(error) = project.save(path) {
                return (project, Err(error));
            }
        }
        (project, Ok(()))
    }

    /// Writes `project.json` atomically, indented as `QJsonDocument` writes it.
    pub fn save(&self, path: &str) -> Result<(), String> {
        qsavefile::save(path, self.to_json().to_json(Format::Indented).as_bytes())
    }
}
