//! What QML reads as `editor.project`, `editor.clips` and `editor.zooms` (`projectMap()`).

use super::Document;
use crate::compat::qcolor::QColor;
use crate::compat::qdir::FileInfo;
use crate::compat::qjson::{Object, Value};
use crate::compat::qurl;
use crate::omarchy_paths;
use crate::theme;

impl Document {
    /// `projectMap()` as JSON: `Project::toJson()` with `background.resolvedImage` (the image or
    /// wallpaper as a file URL, `omarchy:current` resolved, `""` for none) and, for each clip,
    /// `layout` (always present), `kind` ("media" or "recording"), `lower` and `upper` (how far
    /// its in and out may move), `missing`, and for inserted clips `mediaName`.
    ///
    /// The Qt side converts it with `QJsonValue::toVariant`'s rules, so the types stay what the
    /// C++ map held: `Value::Int` is a `qlonglong` (what `toVariantMap()` makes of every
    /// integral JSON number), `Value::Double` a `double` (`lower` and `upper` are always
    /// doubles, as the C++ inserted them), `Value::Null` `QVariant::fromValue(nullptr)`. It adds
    /// `renderTheme` itself, `{background, foreground}` as `QColor`s from
    /// [`Document::render_theme`], and sets the application font as the C++ did here
    /// (`ThemeFonts::setApplicationFont(interfaceFamily(), bodyTextSize())`). The C++ cached the
    /// map until the next edit or theme refresh; [`Document::project_map_revision`] says when.
    pub fn project_map(&self) -> Object {
        let mut result = self.project.to_json();
        let mut background = result.value("background").to_object();
        let image = match self.project.background.type_.as_str() {
            "image" => self.project.background.image.clone(),
            "wallpaper" if self.project.background.wallpaper == "omarchy:current" => omarchy_paths::current_background(),
            "wallpaper" => self.project.background.wallpaper.clone(),
            _ => String::new(),
        };
        background.insert("resolvedImage", if image.is_empty() { String::new() } else { qurl::from_local_file(&image) });
        result.insert("background", background);
        let mut clips = result.value("clips").to_array();
        let timeline = self.timeline();
        for (i, clip_value) in clips.iter_mut().enumerate().take(self.project.clips.len()) {
            let source = &self.project.clips[i];
            let mut clip = clip_value.to_object();
            if !clip.contains("layout") {
                clip.insert("layout", "default");
            }
            if source.is_media() {
                let info = self.inserts.get(&source.media).copied().unwrap_or_default();
                clip.insert("kind", "media");
                clip.insert("lower", Value::Double(0.0));
                clip.insert("upper", Value::Double(if info.is_valid() { info.duration } else { source.out }));
                clip.insert("mediaName", FileInfo::new(source.media.as_str()).file_name());
                clip.insert("missing", !info.is_valid());
            } else {
                let (lower, upper) = timeline.recording_bounds(i as i32, self.recording.source_duration);
                clip.insert("kind", "recording");
                clip.insert("lower", Value::Double(lower));
                clip.insert("upper", Value::Double(upper));
                clip.insert("missing", false);
            }
            *clip_value = clip.into();
        }
        result.insert("clips", Value::Array(clips));
        result
    }

    /// `clips`: the project map's clip list.
    pub fn clips(&self) -> Vec<Value> {
        self.project_map().value("clips").to_array()
    }

    /// `zooms`: the project map's zoom list.
    pub fn zooms(&self) -> Vec<Value> {
        self.project_map().value("zooms").to_array()
    }

    /// The project map's `renderTheme`: colors.toml's background and foreground.
    pub fn render_theme(&self) -> (QColor, QColor) {
        theme::render_colors()
    }
}
