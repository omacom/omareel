//! Export presets per platform (`omareel export B --preset ID`, the export dialog's Preset row):
//! each sets the format, size, frame rate, quality, aspect and reframing of one export, and the
//! file name it suggests. A preset changes only the export it is used for; the project keeps its
//! own settings unless [`ExportPreset::apply`] is asked to save them
//! ([`Document::apply_export_preset`](crate::editor::Document::apply_export_preset)).

use crate::compat::qdir::{self, FileInfo};
use crate::project::Project;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportPreset {
    /// What `--preset` takes.
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    /// "mp4" or "gif".
    pub format: &'static str,
    /// The output's height in pixels; the width follows from the aspect.
    pub height: i32,
    pub fps: i32,
    pub quality: &'static str,
    /// A project aspect ("auto" keeps the recording's shape).
    pub aspect: &'static str,
    /// A reframe mode ([`crate::reframe::MODES`]), or "" to fit the whole recording.
    pub reframe: &'static str,
    /// Appended to the project's name for the file name.
    pub suffix: &'static str,
}

pub const PRESETS: [ExportPreset; 5] = [
    ExportPreset {
        id: "youtube",
        label: "YouTube",
        description: "16:9 1080p at 60 fps.",
        format: "mp4",
        height: 1080,
        fps: 60,
        quality: "social",
        aspect: "16:9",
        reframe: "",
        suffix: "youtube",
    },
    ExportPreset {
        id: "shorts",
        label: "Shorts",
        description: "Vertical 1080×1920 that follows the action, for Shorts, Reels and TikTok.",
        format: "mp4",
        height: 1920,
        fps: 60,
        quality: "social",
        aspect: "9:16",
        reframe: "follow",
        suffix: "shorts",
    },
    ExportPreset {
        id: "x",
        label: "X",
        description: "1080p at 30 fps in the recording's shape.",
        format: "mp4",
        height: 1080,
        fps: 30,
        quality: "social",
        aspect: "auto",
        reframe: "",
        suffix: "x",
    },
    ExportPreset {
        id: "github-gif",
        label: "GitHub GIF",
        description: "A 480p GIF at 15 fps for READMEs and pull requests.",
        format: "gif",
        height: 480,
        fps: 15,
        quality: "studio",
        aspect: "auto",
        reframe: "",
        suffix: "github",
    },
    ExportPreset {
        id: "slack",
        label: "Slack",
        description: "A light 720p video at 30 fps that plays inline.",
        format: "mp4",
        height: 720,
        fps: 30,
        quality: "web-high",
        aspect: "auto",
        reframe: "",
        suffix: "slack",
    },
];

/// The preset called `id`.
pub fn find(id: &str) -> Option<&'static ExportPreset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// The ids, for messages: "youtube, shorts, x, github-gif, slack".
pub fn ids() -> String {
    PRESETS.iter().map(|preset| preset.id).collect::<Vec<_>>().join(", ")
}

impl ExportPreset {
    /// The file name the preset suggests for a project called `name`: `<name>-<suffix>.<format>`.
    pub fn file_name(&self, name: &str) -> String {
        format!("{name}-{}.{}", self.suffix, self.format)
    }

    /// Sets the project's aspect, reframing and its export settings for the preset's format to
    /// the preset's. The exporter does this to its own copy of the project; the editor saves it
    /// only when asked.
    pub fn apply(&self, project: &mut Project) {
        project.aspect = self.aspect.into();
        project.reframe = self.reframe.into();
        let e = &mut project.export_settings;
        e.format = self.format.into();
        if self.format == "gif" {
            e.gif.height = self.height;
            e.gif.fps = self.fps;
            e.gif.quality = self.quality.into();
        } else {
            e.height = self.height;
            e.fps = self.fps;
            e.quality = self.quality.into();
        }
    }
}

/// Where `omareel export BUNDLE --preset ID` writes without `-o`: the preset's file name for
/// the bundle's project (its name, or the bundle's for a bundle without `project.json`) in the
/// recordings directory, where the editor's export dialog suggests it too.
pub fn default_output_path(bundle_path: &str, preset: &ExportPreset) -> String {
    let project_path = qdir::file_path(bundle_path, "project.json");
    let name = if FileInfo::new(&project_path).is_file() {
        Project::load(&project_path).0.name
    } else {
        qdir::dir_name(bundle_path)
    };
    qdir::file_path(&crate::omarchy_paths::recordings_directory(), &preset.file_name(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::plan::output_aspect;
    use crate::project::allowed_aspects;

    #[test]
    fn presets_are_valid() {
        assert_eq!(ids(), "youtube, shorts, x, github-gif, slack");
        for preset in &PRESETS {
            assert!(allowed_aspects().contains(&preset.aspect), "{}", preset.id);
            assert!(preset.reframe.is_empty() || crate::reframe::MODES.contains(&preset.reframe));
            assert!(["mp4", "gif"].contains(&preset.format));
            assert!(["web-low", "web-high", "social", "studio"].contains(&preset.quality));
            assert!(preset.height > 0 && preset.fps > 0);
        }
        assert_eq!(find("nope"), None);
        let shorts = find("shorts").unwrap();
        assert_eq!(shorts.file_name("Demo"), "Demo-shorts.mp4");
        assert_eq!(find("github-gif").unwrap().file_name("Demo"), "Demo-github.gif");
        // 9:16 at 1920 high is 1080 wide.
        assert_eq!((f64::from(shorts.height) * output_aspect(shorts.aspect, 1920, 1080, &Default::default())).round(), 1080.0);
    }

    #[test]
    fn apply_sets_only_the_presets_format() {
        let mut project = Project::defaults("p", 1.0);
        let before = project.clone();
        find("shorts").unwrap().apply(&mut project);
        assert_eq!((project.aspect.as_str(), project.reframe.as_str()), ("9:16", "follow"));
        assert_eq!((project.export_settings.height, project.export_settings.fps), (1920, 60));
        assert_eq!(project.export_settings.gif, before.export_settings.gif);
        find("github-gif").unwrap().apply(&mut project);
        assert_eq!((project.aspect.as_str(), project.reframe.as_str()), ("auto", ""));
        assert_eq!(project.export_settings.format, "gif");
        assert_eq!((project.export_settings.gif.height, project.export_settings.gif.fps), (480, 15));
        assert_eq!(project.export_settings.height, 1920);
    }
}
