import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Omareel.Ui

// Captions from the transcript, and cutting pauses and filler words. Transcription runs in the
// background with Omarchy's dictation engine; everything stays on this computer.
ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    readonly property var settings: editor.project.captions || ({enabled: false, position: "bottom", size: 1})
    readonly property string phase: editor.transcriptionState
    readonly property var positions: [{label: "Bottom", value: "bottom"}, {label: "Top", value: "top"}]
    property string savedPath: ""

    OmEmptyState {
        visible: !editor.hasAudio
        iconSource: "qrc:/omareel/assets/icons/lucide/volume-x.svg"
        title: "No audio in this recording"
        hint: "Captions and pause trimming need a recording with sound."
    }

    // What the transcript is, or why there is none.
    RowLayout {
        visible: editor.hasAudio
        Layout.fillWidth: true
        spacing: 8
        BusyIndicator {
            visible: root.phase === "analyzing" || root.phase === "transcribing"
            running: visible
            Layout.preferredWidth: 18
            Layout.preferredHeight: 18
        }
        Text {
            objectName: "transcriptionStatus"
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            color: root.phase === "failed" ? theme.record : theme.textMuted
            font.family: theme.fontFamily
            font.pixelSize: theme.type.small
            text: root.phase === "analyzing" ? "Listening for speech…"
                : root.phase === "transcribing" ? "Transcribing… " + Math.round(editor.transcriptionProgress * 100) + "%"
                : root.phase === "unavailable" || root.phase === "failed" ? editor.transcriptionMessage
                : editor.hasTranscript ? (editor.transcriptionMessage || editor.transcriptSummary)
                : "No transcript yet"
        }
        EditorButton {
            visible: root.phase !== "analyzing" && root.phase !== "transcribing"
            text: editor.hasTranscript ? "Redo" : "Transcribe"
            onClicked: editor.transcribe()
        }
    }

    OmSeparator { visible: editor.hasAudio; Layout.fillWidth: true }

    EditorSwitch {
        visible: editor.hasAudio
        objectName: "captionsSwitch"
        text: "Show captions"
        description: editor.hasTranscript ? "Words from the transcript, in the preview and the export"
                                          : "Shown once the recording is transcribed"
        checked: root.settings.enabled
        onToggled: editor.setProjectValue("captions.enabled", checked)
    }
    ColumnLayout {
        visible: editor.hasAudio
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        enabled: root.settings.enabled
        opacity: enabled ? 1 : .42
        Behavior on opacity { NumberAnimation { duration: 160 } }
        OmSettingRow {
            label: "Position"
            EditorComboBox {
                Layout.preferredWidth: 150
                model: root.positions
                textRole: "label"
                currentIndex: root.settings.position === "top" ? 1 : 0
                onActivated: editor.setProjectValue("captions.position", model[currentIndex].value)
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "captions.size"; from: .5; to: 2; value: root.settings.size; stepSize: .05; decimals: 2; suffix: "×" }
    }
    RowLayout {
        visible: editor.hasAudio && editor.hasTranscript
        Layout.fillWidth: true
        spacing: 8
        EditorButton {
            text: "Save .srt"
            onClicked: root.savedPath = editor.saveSubtitles("srt")
        }
        EditorButton {
            text: "Save .vtt"
            onClicked: root.savedPath = editor.saveSubtitles("vtt")
        }
        Item { Layout.fillWidth: true }
    }
    Text {
        visible: root.savedPath !== ""
        Layout.fillWidth: true
        elide: Text.ElideMiddle
        textFormat: Text.PlainText
        color: theme.textMuted
        font.family: theme.fontFamily
        font.pixelSize: theme.type.small
        text: "Saved " + root.savedPath
    }

    OmSeparator { visible: editor.hasAudio; Layout.fillWidth: true }

    PanelHeading { visible: editor.hasAudio; text: "Tighten" }
    ColumnLayout {
        visible: editor.hasAudio
        Layout.fillWidth: true
        spacing: 6
        EditorButton {
            objectName: "trimPausesButton"
            Layout.fillWidth: true
            enabled: editor.pauseCount > 0
            text: editor.pauseCount > 0 ? "Trim " + editor.pauseCount + (editor.pauseCount === 1 ? " pause" : " pauses")
                                        : "No pauses to trim"
            onClicked: editor.trimPauses()
        }
        Text {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            textFormat: Text.PlainText
            color: theme.textMuted
            font.family: theme.fontFamily
            font.pixelSize: theme.type.small
            text: "Silences over a second shrink to 0.4 s. Silences where you type, click or move the pointer stay."
        }
        EditorButton {
            objectName: "removeFillersButton"
            Layout.fillWidth: true
            enabled: editor.fillerCount > 0
            text: editor.fillerCount > 0 ? "Remove " + editor.fillerCount + (editor.fillerCount === 1 ? " filler word" : " filler words")
                : editor.hasTranscript ? "No filler words" : "Filler words need a transcript"
            onClicked: editor.removeFillerWords()
        }
    }
}
