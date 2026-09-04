import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import Omareel.Ui

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
            color: menuItem.highlighted ? theme.menuSelectedText
                                        : menuItem.enabled ? theme.menuText : theme.textFaint
            font.pixelSize: theme.font.body
            font.weight: Font.Medium
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: theme.radius
            color: menuItem.highlighted ? theme.menuSelectedBackground : "transparent"
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

        RowLayout {
            spacing: 6
            IconImage {
                Layout.preferredWidth: 20; Layout.preferredHeight: 20
                source: "qrc:/omareel/assets/icons/reel.svg"
                sourceSize: Qt.size(20, 20)
                color: theme.foreground
            }
            Label {
                text: "omareel"
                font.weight: Font.DemiBold
                font.pixelSize: theme.font.title
                color: theme.foreground
            }
        }
        Rectangle {
            Layout.preferredWidth: 1
            Layout.preferredHeight: 16
            Layout.leftMargin: 4
            Layout.rightMargin: 4
            color: theme.hairlineStrong
        }
        OmTextField {
            id: projectName
            Layout.preferredWidth: Math.max(160, Math.min(300, contentWidth + 24))
            Layout.preferredHeight: 32
            text: editor.bundleName
            selectByMouse: true
            hoverEnabled: true
            font.pixelSize: theme.font.title
            font.weight: Font.DemiBold
            leftPadding: 8
            rightPadding: 8
            topPadding: 0
            bottomPadding: 0
            color: theme.foreground
            focusPolicy: Qt.StrongFocus
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
            font.pixelSize: theme.font.bodySmall
        }

        Item { Layout.fillWidth: true }

        IconToolButton {
            enabled: editor.canUndo
            icon.source: "qrc:/omareel/assets/icons/lucide/undo-2.svg"
            Accessible.name: "Undo"
            ToolTip.visible: hovered
            ToolTip.text: "Undo"
            onClicked: editor.undo()
        }
        IconToolButton {
            enabled: editor.canRedo
            icon.source: "qrc:/omareel/assets/icons/lucide/redo-2.svg"
            Accessible.name: "Redo"
            ToolTip.visible: hovered
            ToolTip.text: "Redo"
            onClicked: editor.redo()
        }
        IconToolButton {
            enabled: editor.dirty
            icon.source: "qrc:/omareel/assets/icons/lucide/save.svg"
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
            trailingIconSource: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
            onClicked: presetsMenu.open()
        }
        EditorButton {
            Layout.preferredWidth: 104
            primary: true
            text: "Export"
            icon.source: "qrc:/omareel/assets/icons/lucide/download.svg"
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
            radius: theme.radius
            color: theme.menuBackground
            border.width: 2
            border.color: theme.popupBorder
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
        background: OmPopupCard { }
        contentItem: ColumnLayout {
            spacing: 16
            Label { text: "Save preset"; font.pixelSize: theme.font.title; font.weight: Font.DemiBold; color: theme.foreground }
            OmTextField { id: presetName; Layout.fillWidth: true; placeholderText: "Preset name"; selectByMouse: true }
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
        background: OmPopupCard { }
        contentItem: ColumnLayout {
            spacing: 16
            Label { text: "Delete preset"; font.pixelSize: theme.font.title; font.weight: Font.DemiBold; color: theme.foreground }
            EditorComboBox { id: deletePresetName; Layout.fillWidth: true; model: editor.presetNames }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                EditorButton { text: "Cancel"; onClicked: deletePresetDialog.close() }
                EditorButton {
                    text: "Delete"
                    primary: true
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
