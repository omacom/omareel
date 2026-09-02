import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 48
    color: theme.lighterBackground
    border.color: "#10ffffff"
    property bool nameFieldFocused: projectName.activeFocus
    signal showExport()

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 12
        spacing: 8
        Label {
            text: "omarecord"
            font.weight: Font.DemiBold
            font.pixelSize: 16
            color: theme.accent
        }
        TextField {
            id: projectName
            Layout.preferredWidth: 270
            Layout.preferredHeight: 32
            text: editor.bundleName
            selectByMouse: true
            font.pixelSize: 13
            leftPadding: 10
            rightPadding: 10
            color: theme.foreground
            focusPolicy: Qt.StrongFocus
            background: Rectangle {
                radius: 6
                color: projectName.activeFocus ? "#16000000" : "transparent"
                border.width: 1
                border.color: projectName.activeFocus ? theme.accent : "#16ffffff"
            }
            onEditingFinished: editor.setProjectValue("name", text)
        }
        Item { Layout.fillWidth: true }
        ToolButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            enabled: editor.canUndo
            icon.source: "qrc:/omarecord/assets/icons/undo.svg"
            icon.width: 20; icon.height: 20
            icon.color: enabled ? theme.foreground : "#55ffffff"
            focusPolicy: Qt.TabFocus
            Accessible.name: "Undo"
            ToolTip.visible: hovered
            ToolTip.text: "Undo"
            onClicked: editor.undo()
            background: Rectangle { radius: 6; color: parent.hovered ? "#12ffffff" : "transparent" }
        }
        ToolButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            enabled: editor.canRedo
            icon.source: "qrc:/omarecord/assets/icons/redo.svg"
            icon.width: 20; icon.height: 20
            icon.color: enabled ? theme.foreground : "#55ffffff"
            focusPolicy: Qt.TabFocus
            Accessible.name: "Redo"
            ToolTip.visible: hovered
            ToolTip.text: "Redo"
            onClicked: editor.redo()
            background: Rectangle { radius: 6; color: parent.hovered ? "#12ffffff" : "transparent" }
        }
        Button {
            id: presetButton
            Layout.preferredWidth: 92
            Layout.preferredHeight: 32
            text: "Presets  ▾"
            font.pixelSize: 12
            focusPolicy: Qt.TabFocus
            onClicked: presetsMenu.open()
            background: Rectangle {
                radius: 6
                color: parent.hovered ? "#10ffffff" : "transparent"
                border.color: parent.hovered ? "#38ffffff" : "#22ffffff"
            }
        }
        Button {
            Layout.preferredWidth: 80
            Layout.preferredHeight: 32
            text: "Export"
            font.pixelSize: 12
            font.weight: Font.DemiBold
            focusPolicy: Qt.TabFocus
            contentItem: Label {
                text: parent.text
                color: theme.accentForeground
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                font: parent.font
            }
            background: Rectangle {
                radius: 6
                color: parent.pressed ? Qt.darker(theme.accent, 1.12) : parent.hovered ? Qt.lighter(theme.accent, 1.08) : theme.accent
            }
            onClicked: root.showExport()
        }
    }

    Menu {
        id: presetsMenu
        x: presetButton.mapToItem(root, 0, 0).x
        y: root.height - 3
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
