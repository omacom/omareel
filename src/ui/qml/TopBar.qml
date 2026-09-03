import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 48
    color: theme.lighterBackground
    border.color: Qt.alpha(theme.foreground, .06)
    signal showExport()
    signal restoreEditorFocus()
    onRestoreEditorFocus: window.restoreEditorFocus()
    function commitProjectName() {
        if (projectName.text !== editor.bundleName)
            editor.setProjectValue("name", projectName.text)
    }

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
                color: projectName.activeFocus ? Qt.alpha(theme.darkBackground, .32) : "transparent"
                border.width: 1
                border.color: projectName.activeFocus ? theme.accent : Qt.alpha(theme.foreground, .09)
            }
            onEditingFinished: root.commitProjectName()
            onAccepted: {
                root.commitProjectName()
                focus = false
                root.restoreEditorFocus()
            }
            Keys.onEscapePressed: event => {
                text = editor.bundleName
                focus = false
                root.restoreEditorFocus()
                event.accepted = true
            }
        }
        Item { Layout.fillWidth: true }
        IconToolButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            enabled: editor.canUndo
            icon.source: "qrc:/omarecord/assets/icons/lucide/undo-2.svg"
            Accessible.name: "Undo"
            ToolTip.visible: hovered
            ToolTip.text: "Undo"
            onClicked: editor.undo()
        }
        IconToolButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            enabled: editor.canRedo
            icon.source: "qrc:/omarecord/assets/icons/lucide/redo-2.svg"
            Accessible.name: "Redo"
            ToolTip.visible: hovered
            ToolTip.text: "Redo"
            onClicked: editor.redo()
        }
        IconToolButton {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            enabled: editor.dirty
            icon.source: "qrc:/omarecord/assets/icons/lucide/save.svg"
            Accessible.name: "Save project"
            ToolTip.visible: hovered
            ToolTip.delay: 300
            ToolTip.text: "Save project (Ctrl+S)"
            onClicked: { root.commitProjectName(); editor.saveNow() }
        }
        Label {
            text: editor.dirty ? "Unsaved changes" : "Saved"
            color: editor.dirty ? theme.accent : Qt.alpha(theme.foreground, .52)
            font.pixelSize: 10
            font.weight: editor.dirty ? Font.DemiBold : Font.Normal
        }
        Button {
            id: presetButton
            Layout.preferredWidth: 104
            Layout.preferredHeight: 32
            text: "Presets  ▾"
            icon.source: "qrc:/omarecord/assets/icons/lucide/layers.svg"
            icon.color: theme.foreground
            icon.width: 16; icon.height: 16
            font.pixelSize: 12
            focusPolicy: Qt.TabFocus
            topInset: 0; bottomInset: 0
            onClicked: presetsMenu.open()
            background: Rectangle {
                radius: 6
                color: parent.hovered ? Qt.alpha(theme.foreground, .06) : "transparent"
                border.color: parent.hovered ? Qt.alpha(theme.foreground, .22) : Qt.alpha(theme.foreground, .13)
            }
        }
        Button {
            Layout.preferredWidth: 96
            Layout.preferredHeight: 32
            text: "Export"
            icon.source: "qrc:/omarecord/assets/icons/lucide/download.svg"
            icon.color: theme.accentForeground
            icon.width: 16; icon.height: 16
            font.pixelSize: 12
            font.weight: Font.DemiBold
            focusPolicy: Qt.TabFocus
            topInset: 0; bottomInset: 0
            palette.buttonText: theme.accentForeground
            background: Rectangle {
                radius: 6
                color: parent.pressed ? Qt.darker(theme.accent, 1.12) : parent.hovered ? Qt.lighter(theme.accent, 1.08) : theme.accent
            }
            onClicked: root.showExport()
        }
    }

    Menu {
        id: presetsMenu
        parent: presetButton
        y: presetButton.height
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
