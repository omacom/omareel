//! `tests/test_recorder.cpp`'s cases for `RecordingPreferences`, which moved to
//! `omareel_core::recording_preferences`, as the C++ test had them: a settings file in a fresh
//! `$XDG_CONFIG_HOME`. The goldens (recordingpreferences) check `load` and `save` over many more
//! files; these keep the round trip and the legacy copy the C++ test was written for. They set
//! `XDG_CONFIG_HOME`, so they run one at a time in this test binary of their own.

use omareel_core::recording_preferences::RecordingPreferences;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

static ENVIRONMENT: Mutex<()> = Mutex::new(());

/// A fresh `$XDG_CONFIG_HOME`, held alone and removed (and the variable put back) at the end.
struct Config {
    path: PathBuf,
    previous: Option<std::ffi::OsString>,
    _lock: MutexGuard<'static, ()>,
}

impl Config {
    fn new(name: &str) -> Config {
        let lock = ENVIRONMENT.lock().unwrap_or_else(PoisonError::into_inner);
        let path = std::env::temp_dir().join(format!("omareel-preferences-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: the lock keeps this binary's other test out; nothing else here reads the
        // environment meanwhile.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &path) };
        Config { path, previous, _lock: lock }
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        // SAFETY: as in new; the lock is still held.
        unsafe {
            match self.previous.take() {
                Some(value) => std::env::set_var("XDG_CONFIG_HOME", value),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// test_recorder.cpp's recordingPreferencesRoundTrip.
#[test]
fn recording_preferences_round_trip() {
    let _config = Config::new("round-trip");
    let mut preferences = RecordingPreferences::load();
    assert!(preferences.system_audio);
    assert!(preferences.microphone);
    assert_eq!(preferences.microphone_device, "default_input");
    assert!(!preferences.webcam.value("enabled").to_bool(true));
    assert!(!preferences.webcam_enabled);
    assert_eq!(preferences.webcam_device, "/dev/video2");
    assert_eq!(preferences.webcam_chosen_device, preferences.webcam_device);
    assert_eq!(preferences.webcam_height, 1080);
    assert!(!preferences.countdown_before_recording);
    preferences.system_audio = false;
    preferences.microphone_device = "test_input".into();
    preferences.webcam_enabled = true;
    preferences.webcam_device = "/dev/video8".into();
    preferences.webcam_chosen_device = "/dev/v4l/by-id/usb-Insta360_Insta360_Link_2-video-index0".into();
    preferences.webcam_height = 720;
    preferences.webcam_rotation = 270;
    preferences.webcam_flip_horizontal = true;
    preferences.countdown_before_recording = true;
    preferences.save().unwrap();
    let loaded = RecordingPreferences::load();
    assert!(!loaded.system_audio);
    assert_eq!(loaded.microphone_device, "test_input");
    assert_eq!(loaded.webcam.value("shape").to_string_or(""), "round");
    assert!(loaded.webcam_enabled);
    assert_eq!(loaded.webcam_device, "/dev/video8");
    assert_eq!(loaded.webcam_chosen_device, "/dev/v4l/by-id/usb-Insta360_Insta360_Link_2-video-index0");
    assert_eq!(loaded.webcam_height, 720);
    assert_eq!(loaded.webcam_rotation, 270);
    assert!(loaded.webcam_flip_horizontal);
    assert!(loaded.countdown_before_recording);
}

// test_recorder.cpp's migratesLegacyRecordingPreferencesByCopy.
#[test]
fn migrates_legacy_recording_preferences_by_copy() {
    let config = Config::new("legacy");
    let legacy = config.path.join("omarecord/settings.json");
    std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    std::fs::write(&legacy, r#"{"systemAudio":false,"microphone":false}"#).unwrap();

    let loaded = RecordingPreferences::load();
    assert!(!loaded.system_audio);
    assert!(!loaded.microphone);
    assert!(legacy.exists());
    assert!(config.path.join("omareel/settings.json").exists());
}
