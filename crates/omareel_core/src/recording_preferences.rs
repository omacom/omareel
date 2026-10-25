//! `RecordingPreferences` (`src/core/RecordingPreferences.cpp`): what the launcher, the recording
//! bar and `omareel record` remember between runs, in `$XDG_CONFIG_HOME/omareel/settings.json`.
//!
//! As in the C++: a missing settings file is copied over from the legacy
//! `omarecord/settings.json` on load (or read from there when the copy fails); unknown or
//! malformed values fall back to the defaults; the `webcam` object is kept whole, with the
//! legacy `inset` key moved to `border` and `mirror` read as `flipHorizontal`, and saved back
//! with the recorder's camera fields set on it.

use crate::compat::qdir::{self, FileInfo};
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qround::q_bound;
use crate::compat::qsavefile::SaveFile;
use crate::compat::qstandardpaths::{self, Location};
use crate::compat::{qfile, qstring};

/// `RecordingPreferences`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordingPreferences {
    pub system_audio: bool,
    pub microphone: bool,
    pub microphone_device: String,
    pub webcam_enabled: bool,
    pub webcam_device: String,
    /// The camera last picked in the launcher; `webcam_device` stands in for it while it is
    /// unplugged.
    pub webcam_chosen_device: String,
    pub webcam_height: i32,
    pub webcam_rotation: i32,
    pub webcam_flip_horizontal: bool,
    pub self_view_enabled: bool,
    pub self_view_size: String,
    pub self_view_x: f64,
    pub self_view_y: f64,
    pub capture_backend: String,
    pub countdown_before_recording: bool,
    /// The project's camera style (`webcam` in settings.json), passed through as it is.
    pub webcam: Object,
}

impl Default for RecordingPreferences {
    fn default() -> RecordingPreferences {
        RecordingPreferences {
            system_audio: true,
            microphone: true,
            microphone_device: "default_input".into(),
            webcam_enabled: false,
            webcam_device: "/dev/video2".into(),
            webcam_chosen_device: "/dev/video2".into(),
            webcam_height: 1080,
            webcam_rotation: 0,
            webcam_flip_horizontal: false,
            self_view_enabled: true,
            self_view_size: "M".into(),
            self_view_x: 1.0,
            self_view_y: 1.0,
            capture_backend: "auto".into(),
            countdown_before_recording: false,
            webcam: default_webcam(),
        }
    }
}

fn object(members: Vec<(&str, Value)>) -> Object {
    let mut object = Object::new();
    for (key, value) in members {
        object.insert(key, value);
    }
    object
}

/// The `webcam` blob a fresh `RecordingPreferences` holds.
fn default_webcam() -> Object {
    object(vec![
        ("enabled", false.into()),
        ("position", "bottom-right".into()),
        ("size", 0.25.into()),
        ("shape", "round".into()),
        ("radius", 16.into()),
        ("crop", "original".into()),
        ("flipHorizontal", false.into()),
        ("rotation", 0.into()),
        (
            "shadow",
            object(vec![
                ("enabled", true.into()),
                ("intensity", 0.28.into()),
                ("blur", 23.into()),
                ("distance", 20.into()),
                ("angle", 45.into()),
            ])
            .into(),
        ),
        (
            "border",
            object(vec![
                ("enabled", true.into()),
                ("width", 6.into()),
                ("color", "#ffffff".into()),
                ("alpha", 0.7.into()),
            ])
            .into(),
        ),
        ("scaleDuringZoom", 0.7.into()),
        ("offset", object(vec![("x", 0.035.into()), ("y", 0.035.into())]).into()),
    ])
}

/// `QStandardPaths::writableLocation(ConfigLocation)`.
fn config_directory() -> String {
    qstandardpaths::writable_location(Location::Config)
}

/// `QJsonDocument::fromJson(bytes).object()`: an empty object for anything but an object.
fn object_or_empty(bytes: &[u8]) -> Object {
    match qjson::from_json(bytes) {
        Ok(Value::Object(object)) => object,
        _ => Object::new(),
    }
}

impl RecordingPreferences {
    /// `RecordingPreferences::path()`: `omareel/settings.json` in the configuration directory.
    pub fn path() -> String {
        qdir::file_path(&config_directory(), "omareel/settings.json")
    }

    /// `RecordingPreferences::load()`.
    pub fn load() -> RecordingPreferences {
        let mut preferences = RecordingPreferences::default();
        let current_path = Self::path();
        let legacy_path = qdir::file_path(&config_directory(), "omarecord/settings.json");
        let mut read_path = current_path.clone();
        if !qdir::file_exists(&current_path) && qdir::file_exists(&legacy_path) {
            qdir::mkpath(&FileInfo::new(current_path.clone()).absolute_path());
            if !qfile::copy(&legacy_path, &current_path) {
                read_path = legacy_path;
            }
        }
        let Ok(bytes) = qfile::read_all(&read_path) else { return preferences };
        let root = object_or_empty(&bytes);
        preferences.system_audio = root.value("systemAudio").to_bool(true);
        preferences.microphone = root.value("microphone").to_bool(true);
        preferences.microphone_device = root.value("microphoneDevice").to_string_or("default_input");
        preferences.self_view_enabled = root.value("selfViewEnabled").to_bool(true);
        let self_view_size = qstring::to_upper(&root.value("selfViewSize").to_string_or("M"));
        preferences.self_view_size =
            if self_view_size == "S" || self_view_size == "L" { self_view_size } else { "M".into() };
        let self_view_position = root.value("selfViewPosition").to_object();
        preferences.self_view_x = q_bound(0.0, self_view_position.value("x").to_double(1.0), 1.0);
        preferences.self_view_y = q_bound(0.0, self_view_position.value("y").to_double(1.0), 1.0);
        preferences.capture_backend = root.value("captureBackend").to_string_or("auto");
        preferences.countdown_before_recording = root.value("countdownBeforeRecording").to_bool(false);
        if root.value("webcam").is_object() {
            preferences.webcam = root.value("webcam").to_object();
            let webcam = &mut preferences.webcam;
            if !webcam.contains("border") && webcam.contains("inset") {
                let inset = webcam.take("inset");
                webcam.insert("border", inset);
            }
            preferences.webcam_enabled = webcam.value("enabled").to_bool(false);
            preferences.webcam_device = webcam.value("device").to_string_or("/dev/video2");
            preferences.webcam_chosen_device = webcam.value("chosenDevice").to_string_or(&preferences.webcam_device);
            preferences.webcam_height = webcam.value("captureHeight").to_int(1080);
            if preferences.webcam_height != 720 {
                preferences.webcam_height = 1080;
            }
            let rotation = webcam.value("rotation").to_int(0);
            preferences.webcam_rotation = if matches!(rotation, 90 | 180 | 270) { rotation } else { 0 };
            preferences.webcam_flip_horizontal = if webcam.contains("flipHorizontal") {
                webcam.value("flipHorizontal").to_bool(false)
            } else {
                webcam.value("mirror").to_bool(false)
            };
        }
        preferences
    }

    /// The bytes `save` writes: `QJsonDocument(root).toJson(Indented)`.
    pub fn to_json(&self) -> String {
        let mut saved_webcam = self.webcam.clone();
        saved_webcam.insert("enabled", self.webcam_enabled);
        saved_webcam.insert("device", self.webcam_device.as_str());
        saved_webcam.insert("chosenDevice", self.webcam_chosen_device.as_str());
        saved_webcam.insert("captureHeight", if self.webcam_height == 720 { 720 } else { 1080 });
        saved_webcam.insert("rotation", self.webcam_rotation);
        saved_webcam.insert("flipHorizontal", self.webcam_flip_horizontal);
        let root = object(vec![
            ("systemAudio", self.system_audio.into()),
            ("microphone", self.microphone.into()),
            ("microphoneDevice", self.microphone_device.as_str().into()),
            ("selfViewEnabled", self.self_view_enabled.into()),
            ("selfViewSize", self.self_view_size.as_str().into()),
            (
                "selfViewPosition",
                object(vec![
                    ("x", q_bound(0.0, self.self_view_x, 1.0).into()),
                    ("y", q_bound(0.0, self.self_view_y, 1.0).into()),
                ])
                .into(),
            ),
            ("captureBackend", self.capture_backend.as_str().into()),
            ("countdownBeforeRecording", self.countdown_before_recording.into()),
            ("webcam", saved_webcam.into()),
        ]);
        Value::Object(root).to_json(Format::Indented)
    }

    /// `save(&error)`: `Err` holds the message the C++ put in `error`.
    pub fn save(&self) -> Result<(), String> {
        let file_path = Self::path();
        if !qdir::mkpath(&FileInfo::new(file_path.clone()).absolute_path()) {
            return Err("Could not create the settings directory".into());
        }
        let mut file = SaveFile::new(file_path);
        if !file.open() {
            return Err(file.error_string());
        }
        if file.write(self.to_json().as_bytes()) < 0 || !file.commit() {
            return Err(file.error_string());
        }
        Ok(())
    }
}
