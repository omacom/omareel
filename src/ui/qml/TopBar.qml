import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 58
    color: "#18191f"
    border.color: "#292b32"
    signal showExport()

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 18
        anchors.rightMargin: 18
        spacing: 10
        Label { text: "omarecord"; font.bold: true; font.pixelSize: 16; color: theme.accent }
        TextField {
            Layout.preferredWidth: 280
            text: editor.bundleName
            placeholderText: "Recording name"
            selectByMouse: true
            onEditingFinished: editor.setProjectValue("name", text)
        }
        Item { Layout.fillWidth: true }
        ToolButton {
            enabled: editor.canUndo
            icon.source: "qrc:/omarecord/assets/icons/undo.svg"
            Accessible.name: "Undo"
            ToolTip.visible: hovered
            ToolTip.text: "Undo"
            onClicked: editor.undo()
        }
        ToolButton {
            enabled: editor.canRedo
            icon.source: "qrc:/omarecord/assets/icons/redo.svg"
            Accessible.name: "Redo"
            ToolTip.visible: hovered
            ToolTip.text: "Redo"
            onClicked: editor.redo()
        }
        Button { text: "Presets"; onClicked: presetsMenu.open() }
        Button { text: "Export"; highlighted: true; onClicked: root.showExport() }
    }

    Menu {
        id: presetsMenu
        x: root.width - width - 100
        y: root.height
        MenuItem { text: "Save current…"; onTriggered: savePresetDialog.open() }
        MenuSeparator { }
        Repeater {
            model: editor.presetNames
            delegate: MenuItem {
                required property string modelData
                text: modelData
                onTriggered: editor.loadPreset(modelData)
            }
        }
        MenuSeparator { }
        MenuItem { text: "Delete preset…"; enabled: editor.presetNames.length > 0; onTriggered: deletePresetDialog.open() }
    }
    Dialog {
        id: savePresetDialog
        title: "Save preset"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Save | Dialog.Cancel
        TextField { id: presetName; width: 260; placeholderText: "Preset name" }
        onAccepted: editor.savePreset(presetName.text)
    }
    Dialog {
        id: deletePresetDialog
        title: "Delete preset"
        modal: true
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel
        ComboBox { id: deletePresetName; width: 260; model: editor.presetNames }
        onAccepted: editor.deletePreset(deletePresetName.currentText)
    }
}
