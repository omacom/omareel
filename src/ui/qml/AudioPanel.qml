import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    spacing: 16
    PanelHeading { text: editor.hasAudio ? "Recorded audio" : "No audio in this recording" }
    Label {
        visible: !editor.hasAudio
        Layout.fillWidth: true
        text: "Audio controls become available when a recording contains a system or microphone track."
        color: theme.textMuted
        font.pixelSize: 11
        wrapMode: Text.WordWrap
    }
    Rectangle { visible: !editor.hasAudio; Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
    EditorSwitch { Layout.preferredHeight: 32; text: "System audio"; enabled: editor.hasDesktopAudio; checked: editor.hasDesktopAudio && editor.project.audio.desktop; onToggled: editor.setProjectValue("audio.desktop", checked) }
    EditorSwitch { Layout.preferredHeight: 32; text: "Microphone audio"; enabled: editor.hasMicrophoneAudio; checked: editor.hasMicrophoneAudio && editor.project.audio.mic; onToggled: editor.setProjectValue("audio.mic", checked) }
    PanelSlider { enabled: editor.hasAudio; opacity: enabled ? 1 : .45; Layout.fillWidth: true; label: "Volume"; path: "audio.volume"; from: 0; to: 2; value: editor.project.audio.volume; stepSize: 0.01; decimals: 2 }
}
