import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 48
    color: theme.surface
    signal showExport()
    signal restoreEditorFocus()
    property string savedTime: Qt.formatTime(new Date(), "HH:mm")

    onRestoreEditorFocus: window.restoreEditorFocus()
    function commitProjectName() {
        if (projectName.text !== editor.bundleName)
            editor.setProjectValue("name", projectName.text)
    }

    component BarMenuItem: MenuItem {
        id: menuItem
        implicitHeight: 32
        leftPadding: 10
        rightPadding: 10
        topPadding: 0
        bottomPadding: 0
        hoverEnabled: true
        contentItem: Label {
            text: menuItem.text
            color: menuItem.enabled ? theme.foreground : theme.textFaint
            font.pixelSize: 12
            font.weight: Font.Medium
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: 6
            color: menuItem.highlighted ? theme.hairline : "transparent"
        }
    }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: theme.hairline
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 12
        spacing: 8

        Label {
            text: "omarecord"
            font.weight: Font.DemiBold
            font.pixelSize: 17
            color: theme.foreground
        }
        Rectangle {
            Layout.preferredWidth: 1
            Layout.preferredHeight: 16
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            color: theme.hairlineStrong
        }
        TextField {
            id: projectName
            Layout.preferredWidth: Math.max(160, Math.min(300, contentWidth + 24))
            Layout.preferredHeight: 32
            text: editor.bundleName
            selectByMouse: true
            hoverEnabled: true
            font.pixelSize: 17
            font.weight: Font.DemiBold
            leftPadding: 8
            rightPadding: 8
            topPadding: 0
            bottomPadding: 0
            color: theme.foreground
            focusPolicy: Qt.StrongFocus
            background: Item {
                Rectangle {
                    anchors.fill: parent
                    radius: 6
                    color: projectName.activeFocus ? theme.hairline : "transparent"
                    border.width: projectName.activeFocus ? 1 : 0
                    border.color: theme.hairlineStrong
                }
                Rectangle {
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    height: 1
                    color: theme.hairlineStrong
                    visible: projectName.hovered && !projectName.activeFocus
                }
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
        Label {
            text: editor.dirty ? "Unsaved changes" : "Saved · " + root.savedTime
            color: theme.textMuted
            font.pixelSize: 11
        }

        Item { Layout.fillWidth: true }

        IconToolButton {
            enabled: editor.canUndo
            icon.source: "qrc:/omarecord/assets/icons/lucide/undo-2.svg"
            Accessible.name: "Undo"
            ToolTip.visible: hovered
            ToolTip.text: "Undo"
            onClicked: editor.undo()
        }
        IconToolButton {
            enabled: editor.canRedo
            icon.source: "qrc:/omarecord/assets/icons/lucide/redo-2.svg"
            Accessible.name: "Redo"
            ToolTip.visible: hovered
            ToolTip.text: "Redo"
            onClicked: editor.redo()
        }
        IconToolButton {
            enabled: editor.dirty
            icon.source: "qrc:/omarecord/assets/icons/lucide/save.svg"
            Accessible.name: "Save project"
            ToolTip.visible: hovered
            ToolTip.text: "Save project (Ctrl+S)"
            onClicked: { root.commitProjectName(); editor.saveNow() }
        }
        Rectangle {
            Layout.preferredWidth: 1
            Layout.preferredHeight: 20
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            color: theme.hairlineStrong
        }
        EditorButton {
            id: presetButton
            Layout.preferredWidth: 92
            text: "Presets"
            icon.source: "qrc:/omarecord/assets/icons/lucide/chevron-down.svg"
            onClicked: presetsMenu.open()
        }
        EditorButton {
            Layout.preferredWidth: 104
            Layout.preferredHeight: 36
            primary: true
            text: "Export"
            icon.source: "qrc:/omarecord/assets/icons/lucide/download.svg"
            onClicked: root.showExport()
        }
    }

    Menu {
        id: presetsMenu
        parent: presetButton
        y: presetButton.height + 4
        width: 188
        padding: 4
        background: Rectangle {
            radius: 10
            color: theme.surfaceRaised
            border.width: 1
            border.color: theme.hairlineStrong
        }
        BarMenuItem { text: "Save current…"; onTriggered: savePresetDialog.open() }
        MenuSeparator { contentItem: Rectangle { implicitHeight: 1; color: theme.hairline } }
        Repeater {
            model: editor.presetNames
            delegate: BarMenuItem {
                required property string modelData
                text: modelData
                onTriggered: editor.loadPreset(modelData)
            }
        }
        MenuSeparator { contentItem: Rectangle { implicitHeight: 1; color: theme.hairline } }
        BarMenuItem {
            text: "Delete preset…"
            enabled: editor.presetNames.length > 0
            onTriggered: deletePresetDialog.open()
        }
    }

    Dialog {
        id: savePresetDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 380
        modal: true
        focus: true
        padding: 24
        standardButtons: Dialog.NoButton
        Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, .55) }
        background: Rectangle {
            radius: 14
            color: theme.surfaceRaised
            border.width: 1
            border.color: theme.hairlineStrong
        }
        contentItem: ColumnLayout {
            spacing: 16
            Label { text: "Save preset"; font.pixelSize: 17; font.weight: Font.DemiBold; color: theme.foreground }
            TextField { id: presetName; Layout.fillWidth: true; placeholderText: "Preset name"; selectByMouse: true }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                EditorButton { text: "Cancel"; onClicked: savePresetDialog.close() }
                EditorButton {
                    text: "Save"
                    primary: true
                    enabled: presetName.text.trim().length > 0
                    onClicked: { editor.savePreset(presetName.text); savePresetDialog.close() }
                }
            }
        }
    }

    Dialog {
        id: deletePresetDialog
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 380
        modal: true
        focus: true
        padding: 24
        standardButtons: Dialog.NoButton
        Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, .55) }
        background: Rectangle {
            radius: 14
            color: theme.surfaceRaised
            border.width: 1
            border.color: theme.hairlineStrong
        }
        contentItem: ColumnLayout {
            spacing: 16
            Label { text: "Delete preset"; font.pixelSize: 17; font.weight: Font.DemiBold; color: theme.foreground }
            EditorComboBox { id: deletePresetName; Layout.fillWidth: true; model: editor.presetNames }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                EditorButton { text: "Cancel"; onClicked: deletePresetDialog.close() }
                EditorButton {
                    text: "Delete"
                    destructive: true
                    onClicked: { editor.deletePreset(deletePresetName.currentText); deletePresetDialog.close() }
                }
            }
        }
    }

    Connections {
        target: editor
        function onAutosaved(path) { root.savedTime = Qt.formatTime(new Date(), "HH:mm") }
    }
}
