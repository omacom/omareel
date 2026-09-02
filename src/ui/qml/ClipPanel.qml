import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    property var selected: {
        for (let i = 0; i < editor.clips.length; ++i)
            if (editor.clips[i].id === editor.selectedClipId) return editor.clips[i]
        return null
    }
    PanelHeading {
        Layout.fillWidth: true
        text: root.selected ? "Selected clip" : "Select a clip on the timeline"
        wrapMode: Text.WordWrap
    }
    ColumnLayout {
        visible: root.selected !== null
        Layout.fillWidth: true
        spacing: 10
        PanelLabel { text: "Speed" }
        ComboBox {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            model: [0.5, 0.75, 1, 1.2, 1.4, 1.6, 1.8, 2, 3, 4, 8, 16, 24]
            currentIndex: root.selected ? model.indexOf(root.selected.speed) : -1
            textRole: ""
            displayText: currentIndex >= 0 ? model[currentIndex] + "×" : ""
            delegate: ItemDelegate { required property var modelData; width: parent.width; text: modelData + "×" }
            onActivated: editor.setClipSpeed(editor.selectedClipId, model[currentIndex])
        }
        Button { Layout.fillWidth: true; text: "Remove trims"; onClicked: editor.resetClipTrims(editor.selectedClipId) }
        Button { Layout.fillWidth: true; text: "Delete clip"; enabled: editor.clips.length > 1; onClicked: editor.removeClip(editor.selectedClipId) }
    }
}
