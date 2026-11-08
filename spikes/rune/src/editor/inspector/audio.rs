//! AudioPanel.qml: the recording's tracks (system audio, microphone) and the volume.

use super::widgets::{SliderSpec, empty_state, om, project_slider, toggle};
use crate::editor::set;
use crate::preview::Preview;
use rune::{El, col};

pub fn view(s: &Preview, _w: f32) -> El {
    let ui = &s.ui;
    let d = &s.document;
    let audio = &d.project().audio;
    let mut panel = col().w_full().gap(om().panel_gap);
    if !d.has_audio() {
        panel = panel.child(empty_state(
            ui,
            &crate::icons::VOLUME_X,
            "No audio in this recording",
            "Turn on System audio or Microphone in the launcher to record sound.",
            "",
        ));
    }
    if d.has_desktop_audio() {
        panel = panel.child(toggle(s, "audio-desktop", "System audio", "Sound from apps on your desktop", audio.desktop, true, |s, v| {
            set(s, "audio.desktop", v.into(), false, "")
        }));
    }
    if d.has_microphone_audio() {
        panel = panel.child(toggle(s, "audio-mic", "Microphone", "Your voice", audio.mic, true, |s, v| set(s, "audio.mic", v.into(), false, "")));
    }
    if d.has_audio() {
        panel = panel.child(project_slider(s, SliderSpec::new("audio-volume", "Volume", 0.0, 2.0, audio.volume, 0.01).scale(100.0).suffix("%"), "audio.volume"));
    }
    panel
}
