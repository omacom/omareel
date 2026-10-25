//! Style presets (`savePreset`, `loadPreset`, `deletePreset`, `presetNames`): the look of a
//! project without its edits, as `<name>.json` in `$XDG_CONFIG_HOME/omareel/presets`.

use super::{Document, Effect};
use crate::compat::qdir::{self, FileInfo, Filters, SortFlags};
use crate::compat::qfile;
use crate::compat::qjson::{self, Format, Object, Value};
use crate::compat::qsavefile;
use crate::project::Project;

/// The parts of a project a preset holds.
pub const PRESET_KEYS: [&str; 7] = ["background", "frame", "cursor", "zoomStyle", "camera", "audio", "export"];

/// `cleanPresetName`: trimmed (Unicode white space), every character but ASCII letters, digits,
/// space, `_`, `.` and `-` replaced by `_`, leading dots dropped, at most 80 characters.
pub fn clean_preset_name(name: &str) -> String {
    let name = super::bundle::replace_unsafe_name_characters(name.trim_matches(char::is_whitespace));
    let mut name = name.trim_start_matches('.').to_owned();
    // Only ASCII is left, so 80 UTF-16 units are 80 bytes.
    name.truncate(80);
    name
}

impl Document {
    /// `presetsDirectory()`.
    pub fn presets_directory(&self) -> String {
        qdir::file_path(&self.config_location(), "omareel/presets")
    }

    fn preset_path(&self, raw_name: &str) -> String {
        qdir::file_path(&self.presets_directory(), &format!("{}.json", clean_preset_name(raw_name)))
    }

    /// `presetNames`: the `*.json` files (any case) by name, without the suffix.
    pub fn preset_names(&self) -> Vec<String> {
        qdir::entry_list(&self.presets_directory(), &["*.json"], Filters::FILES, SortFlags::NAME)
            .iter()
            .map(|file| FileInfo::new(file.as_str()).complete_base_name())
            .collect()
    }

    /// `stylePreset()`: what a preset file holds.
    pub fn style_preset(&self) -> Object {
        let json = self.project.to_json();
        let mut preset = Object::from_iter([
            ("version", Value::from(1)),
            ("defaultsVersion", Project::CURRENT_DEFAULTS_VERSION.into()),
        ]);
        for key in PRESET_KEYS {
            preset.insert(key, json.value(key).clone());
        }
        preset
    }

    /// `savePreset(name)`: written atomically and indented; a failure is reported with
    /// `errorOccurred`. `presetsChanged` either way, unless the name cleans to nothing.
    pub fn save_preset(&mut self, raw_name: &str) {
        let name = clean_preset_name(raw_name);
        if name.is_empty() {
            return;
        }
        qdir::mkpath(&self.presets_directory());
        let path = qdir::file_path(&self.presets_directory(), &format!("{name}.json"));
        if let Err(error) = qsavefile::save(&path, self.style_preset().to_json(Format::Indented).as_bytes()) {
            self.emit(Effect::ErrorOccurred(error));
        }
        self.emit(Effect::PresetsChanged);
    }

    /// `loadPreset(name)`: the preset's parts (migrated to the current defaults) replace the
    /// project's. Always an edit once the file opens, even if nothing changes; an unreadable
    /// document applies nothing.
    pub fn load_preset(&mut self, raw_name: &str) {
        use std::io::Read;
        let mut file = match qfile::open_read(self.preset_path(raw_name)) {
            Ok(file) => file,
            Err(error) => {
                self.emit(Effect::ErrorOccurred(error));
                return;
            }
        };
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        let (preset, _) = Project::migrate_defaults(&qjson::object_from_json(&bytes).unwrap_or_default());
        let mut project = self.project.to_json();
        for key in PRESET_KEYS {
            if preset.contains(key) {
                project.insert(key, preset.value(key).clone());
            }
        }
        self.snapshot("");
        self.project = Project::from_json(&project);
        self.changed(true);
    }

    /// `deletePreset(name)`.
    pub fn delete_preset(&mut self, raw_name: &str) {
        let _ = std::fs::remove_file(self.preset_path(raw_name));
        self.emit(Effect::PresetsChanged);
    }
}
