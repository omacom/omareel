import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    spacing: 12
    PanelLabel { text: editor.hasAudio ? "Recorded audio" : "No audio track in this recording"; opacity: editor.hasAudio ? 1 : .72; wrapMode: Text.WordWrap; Layout.fillWidth: true }
    Switch { Layout.preferredHeight: 32; text: "System audio"; enabled: editor.hasDesktopAudio; checked: editor.hasDesktopAudio && editor.project.audio.desktop; onToggled: editor.setProjectValue("audio.desktop", checked) }
    Switch { Layout.preferredHeight: 32; text: "Microphone audio"; enabled: editor.hasMicrophoneAudio; checked: editor.hasMicrophoneAudio && editor.project.audio.mic; onToggled: editor.setProjectValue("audio.mic", checked) }
    PanelSlider { enabled: editor.hasAudio; opacity: enabled ? 1 : .45; Layout.fillWidth: true; label: "Volume"; path: "audio.volume"; from: 0; to: 2; value: editor.project.audio.volume; stepSize: 0.01; decimals: 2 }
}
