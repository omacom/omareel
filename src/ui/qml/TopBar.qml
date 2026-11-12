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
    // Set for a few seconds after project.json changed on disk (omareel apply, an agent).
    property bool updatedExternally: false
    // project.json was changed outside the editor and can't be read: autosave waits.
    property bool projectUnreadable: false

    onRestoreEditorFocus: window.restoreEditorFocus()
    function commitProjectName() {
        if (projectName.text !== editor.bundleName)
            editor.setProjectValue("name", projectName.text)
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
        spacing: 6

        IconImage {
            id: logo
            Layout.preferredWidth: 18; Layout.preferredHeight: 18
            source: "qrc:/omareel/assets/icons/reel.svg"
            sourceSize: Qt.size(18, 18)
            color: logoHover.hovered ? theme.accent : theme.textStrong
            Behavior on color { ColorAnimation { duration: 200 } }
            HoverHandler { id: logoHover; onHoveredChanged: if (hovered) logoSpin.restart() }
            RotationAnimator on rotation {
                id: logoSpin
                running: false
                from: 0; to: 360; duration: 700
                easing.type: Easing.OutCubic
            }
        }
        Label {
            text: "omareel"
            font.family: theme.monoFamily
            font.weight: Font.Bold
            font.pixelSize: theme.type.body
            color: theme.textStrong
        }
        Label {
            Layout.leftMargin: 4
            Layout.rightMargin: 0
            text: "/"
            font.family: theme.monoFamily
            font.pixelSize: theme.type.body
            color: theme.textFaint
        }
        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: 120
            Layout.preferredHeight: 30
            TextField {
                id: projectName
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(parent.width, Math.max(80, Math.min(360, nameMetrics.advanceWidth + 22)))
                height: 30
                text: editor.bundleName
                selectByMouse: true
                hoverEnabled: true
                font.family: theme.fontFamily
                font.pixelSize: theme.type.body
                font.weight: Font.DemiBold
                leftPadding: 8; rightPadding: 8; topPadding: 0; bottomPadding: 0
                verticalAlignment: TextInput.AlignVCenter
                color: theme.textStrong
                selectionColor: Qt.alpha(theme.accent, .35)
                selectedTextColor: theme.textStrong
                focusPolicy: Qt.StrongFocus
                TextMetrics { id: nameMetrics; font: projectName.font; text: projectName.text }
                background: Rectangle {
                    radius: theme.controlRadius
                    color: projectName.activeFocus ? theme.canvas
                         : projectName.hovered ? theme.fillHover : "transparent"
                    border.width: projectName.activeFocus ? 1 : 0
                    border.color: theme.accent
                    Behavior on color { ColorAnimation { duration: 140 } }
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
        }
        Row {
            visible: root.width >= 1000
            spacing: 6
            Layout.rightMargin: 6
            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                width: 6; height: 6; radius: 3
                color: editor.dirty ? theme.zoomAccent : theme.textFaint
                Behavior on color { ColorAnimation { duration: 200 } }
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: root.projectUnreadable ? "project.json can't be read · Ctrl+S overwrites it"
                    : root.updatedExternally ? "Updated from project.json"
                    : editor.dirty ? "Unsaved changes" : "Saved " + root.savedTime
                color: theme.textMuted
                font.pixelSize: theme.type.small
            }
        }
        OmIconButton {
            enabled: editor.canUndo
            icon.source: "qrc:/omareel/assets/icons/lucide/undo-2.svg"
            Accessible.name: "Undo"
            tooltipText: "Undo"
            tooltipShortcut: "Ctrl Z"
            onClicked: editor.undo()
        }
        OmIconButton {
            enabled: editor.canRedo
            icon.source: "qrc:/omareel/assets/icons/lucide/redo-2.svg"
            Accessible.name: "Redo"
            tooltipText: "Redo"
            tooltipShortcut: "Ctrl Shift Z"
            onClicked: editor.redo()
        }
        OmIconButton {
            enabled: editor.dirty
            icon.source: "qrc:/omareel/assets/icons/lucide/save.svg"
            Accessible.name: "Save project"
            tooltipText: "Save project"
            tooltipShortcut: "Ctrl S"
            onClicked: { root.commitProjectName(); editor.saveNow() }
        }
        Rectangle {
            Layout.preferredWidth: 1
            Layout.preferredHeight: 18
            Layout.leftMargin: 6
            Layout.rightMargin: 6
            color: theme.separator
        }
        OmButton {
            objectName: "polishButton"
            visible: window.agentRunner !== null
            Layout.preferredWidth: implicitWidth
            Layout.minimumWidth: implicitWidth
            text: window.agentRunner && window.agentRunner.running ? "Working…" : "Polish"
            icon.source: "qrc:/omareel/assets/icons/lucide/sparkles.svg"
            selected: window.agentOpen
            tooltipText: !window.agentRunner ? ""
                : !window.agentRunner.available ? window.agentRunner.problem
                : "Polish with " + window.agentRunner.agentName + ". It reads the timeline, transcript and frames"
                  + (window.agentRunner.provider ? " and sends them to " + window.agentRunner.provider : "")
            tooltipShortcut: "Ctrl Shift P"
            onClicked: window.agentRunner.running ? window.openAgent("") : window.openAgent("polish")
        }
        OmIconButton {
            visible: window.agentRunner !== null
            icon.source: "qrc:/omareel/assets/icons/lucide/message-square.svg"
            Accessible.name: "Ask the editor"
            tooltipText: "Ask the editor, write a description"
            onClicked: window.agentOpen ? window.closeAgent() : window.openAgent("")
        }
        OmButton {
            id: presetButton
            Layout.preferredWidth: implicitWidth
            Layout.minimumWidth: implicitWidth
            text: "Presets"
            trailingIconSource: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
            selected: presetsMenu.visible
            onClicked: presetsMenu.visible ? presetsMenu.close() : presetsMenu.open()
        }
        OmButton {
            Layout.preferredWidth: implicitWidth
            Layout.minimumWidth: implicitWidth
            Layout.leftMargin: 2
            primary: true
            text: "Export"
            icon.source: "qrc:/omareel/assets/icons/lucide/download.svg"
            tooltipText: "Export video"
            tooltipShortcut: "Ctrl E"
            onClicked: root.showExport()
        }
    }

    OmMenu {
        id: presetsMenu
        parent: presetButton
        x: presetButton.width - width
        y: presetButton.height + 6
        width: 210
        OmMenuItem { text: "Save current as preset…"; onTriggered: savePresetDialog.open() }
        OmMenuSeparator { visible: editor.presetNames.length > 0; height: visible ? implicitHeight : 0 }
        Repeater {
            model: editor.presetNames
            delegate: OmMenuItem {
                required property string modelData
                text: modelData
                onTriggered: editor.loadPreset(modelData)
            }
        }
        OmMenuSeparator { }
        OmMenuItem {
            text: "Delete preset…"
            destructive: true
            enabled: editor.presetNames.length > 0
            onTriggered: deletePresetDialog.open()
        }
    }

    component PresetDialog: Dialog {
        id: dialog
        property string heading: ""
        property string detail: ""
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 380
        modal: true
        focus: true
        padding: 22
        standardButtons: Dialog.NoButton
        Overlay.modal: Rectangle { color: Qt.alpha(theme.canvas, .6) }
        background: OmPopupCard { padding: 0 }
        enter: Transition {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 160; easing.type: Easing.OutCubic }
            NumberAnimation { property: "scale"; from: .96; to: 1; duration: 220; easing.type: Easing.OutQuint }
        }
        exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: 100 } }
    }

    PresetDialog {
        id: savePresetDialog
        onOpened: { presetName.text = ""; presetName.forceActiveFocus() }
        contentItem: ColumnLayout {
            spacing: 14
            ColumnLayout {
                spacing: 4
                Label { text: "Save preset"; font.pixelSize: theme.type.title; font.weight: Font.DemiBold; color: theme.textStrong }
                Label { text: "Background, frame, cursor, and camera styling."; font.pixelSize: theme.type.small; color: theme.textMuted }
            }
            OmTextField {
                id: presetName
                Layout.fillWidth: true
                placeholderText: "Preset name"
                selectByMouse: true
                onAccepted: if (text.trim().length > 0) { editor.savePreset(text); savePresetDialog.close() }
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 4
                spacing: 8
                Item { Layout.fillWidth: true }
                OmButton { text: "Cancel"; bordered: true; onClicked: savePresetDialog.close() }
                OmButton {
                    text: "Save preset"
                    primary: true
                    enabled: presetName.text.trim().length > 0
                    onClicked: { editor.savePreset(presetName.text); savePresetDialog.close() }
                }
            }
        }
    }

    PresetDialog {
        id: deletePresetDialog
        contentItem: ColumnLayout {
            spacing: 14
            ColumnLayout {
                spacing: 4
                Label { text: "Delete preset"; font.pixelSize: theme.type.title; font.weight: Font.DemiBold; color: theme.textStrong }
                Label { text: "This can't be undone."; font.pixelSize: theme.type.small; color: theme.textMuted }
            }
            OmDropdown { id: deletePresetName; Layout.fillWidth: true; model: editor.presetNames }
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 4
                spacing: 8
                Item { Layout.fillWidth: true }
                OmButton { text: "Cancel"; bordered: true; onClicked: deletePresetDialog.close() }
                OmButton {
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
        function onAutosaved(path) { root.savedTime = Qt.formatTime(new Date(), "HH:mm"); root.projectUnreadable = false }
        function onProjectReloaded() { root.projectUnreadable = false; root.updatedExternally = true; externalUpdateTimer.restart() }
        function onProjectUnreadable() { root.projectUnreadable = true }
    }

    Timer {
        id: externalUpdateTimer
        interval: 4000
        onTriggered: root.updatedExternally = false
    }
}
