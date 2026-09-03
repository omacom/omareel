import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    property var selected: {
        for (let i = 0; i < editor.clips.length; ++i)
            if (editor.clips[i].id === editor.selectedClipId) return editor.clips[i]
        return null
    }
    Label {
        visible: root.selected === null
        Layout.fillWidth: true
        text: "Select a clip on the timeline"
        color: theme.textMuted
        font.pixelSize: theme.font.bodySmall
        wrapMode: Text.WordWrap
    }
    ColumnLayout {
        visible: root.selected !== null
        Layout.fillWidth: true
        spacing: 10
        PanelLabel { text: "Speed" }
        EditorComboBox {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            model: [0.5, 0.75, 1, 1.2, 1.4, 1.6, 1.8, 2, 3, 4, 8, 16, 24]
            currentIndex: root.selected ? model.indexOf(root.selected.speed) : -1
            textRole: ""
            displayText: currentIndex >= 0 ? model[currentIndex] + "×" : ""
            onActivated: editor.setClipSpeed(editor.selectedClipId, model[currentIndex])
        }
        EditorButton { Layout.fillWidth: true; text: "Remove trims"; onClicked: editor.resetClipTrims(editor.selectedClipId) }
        EditorButton { Layout.fillWidth: true; text: "Delete clip"; destructive: true; enabled: editor.clips.length > 1; onClicked: editor.removeClip(editor.selectedClipId) }
    }
}
