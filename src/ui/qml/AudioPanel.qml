import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    // project.music, present only while there is music.
    readonly property var music: editor.project.music || ({})
    readonly property bool hasMusic: editor.music.present === true

    // audio.cleanup is only in the project once something is switched on.
    readonly property var cleanup: (editor.project.audio && editor.project.audio.cleanup) || ({})
    readonly property bool denoise: cleanup.denoise === true
    readonly property real strength: cleanup.strength !== undefined ? cleanup.strength : 0.85
    readonly property string loudness: cleanup.loudness || "off"
    readonly property bool cleaning: (denoise && editor.hasMicrophoneAudio) || loudness !== "off"
    OmEmptyState {
        visible: !editor.hasAudio
        iconSource: "qrc:/omareel/assets/icons/lucide/volume-x.svg"
        title: "No audio in this recording"
        hint: "Turn on System audio or Microphone in the launcher to record sound."
    }
    EditorSwitch { visible: editor.hasDesktopAudio; text: "System audio"; description: "Sound from apps on your desktop"; checked: editor.hasDesktopAudio && editor.project.audio.desktop; onToggled: editor.setProjectValue("audio.desktop", checked) }
    EditorSwitch { visible: editor.hasMicrophoneAudio; text: "Microphone"; description: "Your voice"; checked: editor.hasMicrophoneAudio && editor.project.audio.mic; onToggled: editor.setProjectValue("audio.mic", checked) }
    PanelSlider { visible: editor.hasAudio; Layout.fillWidth: true; label: "Volume"; path: "audio.volume"; from: 0; to: 2; value: editor.project.audio.volume; stepSize: 0.01; displayScale: 100; suffix: "%" }

    OmSeparator { visible: editor.hasAudio; Layout.fillWidth: true; Layout.topMargin: 2 }
    PanelHeading { visible: editor.hasAudio; text: "Cleanup" }
    EditorSwitch {
        objectName: "denoiseSwitch"
        visible: editor.hasMicrophoneAudio
        text: "Reduce noise"
        // The recording keeps the microphone and the desktop in one track.
        description: editor.hasDesktopAudio
            ? "Removes fan, hiss and keyboard noise from your voice. System audio is filtered too, which can dull music."
            : "Removes fan, hiss and keyboard noise from your voice"
        checked: root.denoise
        onToggled: editor.setProjectValue("audio.cleanup.denoise", checked)
    }
    PanelSlider { visible: editor.hasMicrophoneAudio && root.denoise; Layout.fillWidth: true; label: "Strength"; path: "audio.cleanup.strength"; from: 0.1; to: 1; value: root.strength; stepSize: 0.05; displayScale: 100; suffix: "%" }
    PanelLabel { visible: editor.hasAudio; text: "Loudness" }
    OmButtonGroup {
        objectName: "loudnessButtons"
        visible: editor.hasAudio
        Layout.fillWidth: true
        options: [{label: "As recorded", value: "off"}, {label: "−16 LUFS", value: "-16"}, {label: "−14 LUFS", value: "-14"}]
        value: root.loudness
        onChanged: value => editor.setProjectValue("audio.cleanup.loudness", value)
    }
    Label {
        visible: editor.hasAudio
        Layout.fillWidth: true
        text: root.loudness === "-16" ? "Evens out the export to the level of podcasts and YouTube."
            : root.loudness === "-14" ? "Evens out the export to the level of social video, a little louder."
            : "The export keeps the recording's level."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.type.small
    }
    // The preview plays the cleaned sound once it is made; "Hear original" compares.
    EditorSwitch {
        objectName: "cleanAudioBypassSwitch"
        visible: root.cleaning
        text: "Hear original"
        description: editor.cleanAudioStatus === "preparing" ? "Preparing the cleaned sound for the preview…"
            : editor.cleanAudioStatus === "failed" ? "The preview can't play the cleaned sound: " + editor.cleanAudioError
            : checked ? "The preview plays the sound as recorded" : "The preview plays the cleaned sound"
        enabled: editor.cleanAudioStatus === "ready" || checked
        checked: editor.cleanAudioBypass
        onToggled: editor.cleanAudioBypass = checked
    }

    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    PanelHeading { text: "Background music" }
    RowLayout {
        visible: root.hasMusic
        Layout.fillWidth: true
        spacing: 8
        IconImage {
            Layout.preferredWidth: 14; Layout.preferredHeight: 14
            source: "qrc:/omareel/assets/icons/lucide/music.svg"
            sourceSize: Qt.size(14, 14)
            color: theme.textMuted
        }
        Label {
            objectName: "musicName"
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: editor.music.name || ""
            elide: Text.ElideMiddle
            color: theme.textStrong
            font.pixelSize: theme.type.body
            font.weight: Font.Medium
        }
        IconToolButton {
            objectName: "removeMusicButton"
            Layout.preferredWidth: 24; Layout.preferredHeight: 24
            icon.source: "qrc:/omareel/assets/icons/lucide/trash-2.svg"
            icon.width: 14; icon.height: 14
            Accessible.name: "Remove music"
            onClicked: editor.clearMusic()
        }
    }
    EditorButton {
        objectName: "addMusicButton"
        Layout.fillWidth: true
        icon.source: root.hasMusic ? "qrc:/omareel/assets/icons/lucide/music.svg" : "qrc:/omareel/assets/icons/lucide/plus.svg"
        text: editor.musicImporting ? "Adding music…" : root.hasMusic ? "Replace music…" : "Add music…"
        enabled: !editor.musicImporting
        onClicked: musicDialog.open()
    }
    Label {
        visible: !root.hasMusic
        Layout.fillWidth: true
        text: "Plays under the whole video. An audio file, or a video whose sound you want. It is copied into the recording."
        color: theme.textMuted
        font.pixelSize: theme.type.small
        wrapMode: Text.WordWrap
    }
    ColumnLayout {
        visible: root.hasMusic
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        PanelSlider { Layout.fillWidth: true; label: "Music volume"; path: "music.volume"; from: 0; to: 1; value: root.music.volume ?? 0.25; stepSize: 0.01; displayScale: 100; suffix: "%" }
        EditorSwitch { objectName: "musicDuckSwitch"; text: "Lower under the recording"; description: "Quieter while your voice or desktop sound plays"; checked: root.music.duck ?? true; onToggled: editor.setProjectValue("music.duck", checked) }
        EditorSwitch { text: "Loop"; description: "Start over when the music is shorter than the video"; checked: root.music.loop ?? true; onToggled: editor.setProjectValue("music.loop", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Fade in"; path: "music.fadeIn"; from: 0; to: 10; value: root.music.fadeIn ?? 1; stepSize: 0.1; decimals: 1; suffix: " s" }
        PanelSlider { Layout.fillWidth: true; label: "Fade out"; path: "music.fadeOut"; from: 0; to: 10; value: root.music.fadeOut ?? 2; stepSize: 0.1; decimals: 1; suffix: " s" }
    }

    FileDialog {
        id: musicDialog
        title: "Choose background music"
        nameFilters: ["Audio (*.mp3 *.m4a *.aac *.ogg *.oga *.opus *.flac *.wav)",
                      "Videos (*.mp4 *.mov *.mkv *.webm *.m4v *.avi)", "All files (*)"]
        onAccepted: editor.addMusic(selectedFile)
    }
}
