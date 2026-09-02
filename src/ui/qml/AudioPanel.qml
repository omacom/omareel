import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    spacing: 12
    Label { text: editor.hasAudio ? "Recorded audio" : "No audio track in this recording"; color: editor.hasAudio ? "#bbbcc3" : "#858894"; wrapMode: Text.WordWrap; Layout.fillWidth: true }
    Switch { text: "Desktop audio"; enabled: editor.hasAudio; checked: editor.project.audio.desktop; onToggled: editor.setProjectValue("audio.desktop", checked) }
    Switch { text: "Microphone audio"; enabled: editor.hasAudio; checked: editor.project.audio.mic; onToggled: editor.setProjectValue("audio.mic", checked) }
    PanelSlider { Layout.fillWidth: true; label: "Volume"; path: "audio.volume"; from: 0; to: 2; value: editor.project.audio.volume; stepSize: 0.01; decimals: 2 }
}
