import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 1440
    height: 900
    minimumWidth: 1100
    minimumHeight: 720
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body
    color: theme.surface
    title: editor.bundleName + " — Omareel"
    property real timelineScale: 1
    property bool cropMode: false
    readonly property bool singleKeyShortcutsBlocked: activeFocusItem !== null
        && !previewPane.activeFocus && !timeline.activeFocus
    function restoreEditorFocus() { previewPane.forceActiveFocus() }
    Component.onCompleted: restoreEditorFocus()
    function openExport() {
        topBar.commitProjectName()
        previewPane.grabPreview(function(result) {
            exportDialog.previewGrab = result
            exportDialog.previewUrl = result.url
            exportDialog.open()
        })
    }
    function prepareScreenshot(view) {
        if (view === "export") {
            editor.seek(2)
            Qt.callLater(window.openExport)
        }
        else if (view === "aspect") bottomBar.openAspectMenu()
        else if (view === "shortcuts") bottomBar.openShortcuts()
        else if (view === "background-hover") sidePanel.setBackgroundHoverProof(3)
        else if (view === "background-expanded"
                 || view === "background-wallpapers-expanded")
            sidePanel.setBackgroundExpanded(true)
        else if (view === "background-gradient-3") {
            sidePanel.section = 0
            editor.applyGradientPreset(3)
        } else if (view === "background-gradient-7") {
            sidePanel.section = 0
            editor.applyGradientPreset(7)
        }
        else if (view === "rail-tooltip") {
            sidePanel.section = 5
            sidePanel.forcedTooltip = "Camera"
            editor.seek(2)
        } else if (view === "camera-proof") {
            sidePanel.section = 5
            editor.seek(2)
            sidePanel.scrollInspectorToBottom()
        }
    }

    Item {
        id: editorUi
        anchors.fill: parent
        opacity: editor.loading ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

        ColumnLayout {
            anchors.fill: parent
            spacing: 0
            TopBar {
                id: topBar
                Layout.fillWidth: true
                onShowExport: window.openExport()
            }
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0
                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.minimumWidth: 528
                    spacing: 0
                    PreviewPane {
                        id: previewPane
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Layout.minimumHeight: 338
                        cropMode: window.cropMode
                    }
                    BottomBar {
                        id: bottomBar
                        Layout.fillWidth: true
                        timelineScale: window.timelineScale
                        cropMode: window.cropMode
                        onTimelineScaleRequested: value => window.timelineScale = value
                        onCropModeChanged: window.cropMode = cropMode
                    }
                }
                SidePanel {
                    id: sidePanel
                    Layout.preferredWidth: 376
                    Layout.minimumWidth: 376
                    Layout.maximumWidth: 376
                    Layout.fillHeight: true
                }
            }
            Timeline {
                id: timeline
                objectName: "editorTimeline"
                Layout.fillWidth: true
                Layout.preferredHeight: implicitHeight
                scaleFactor: window.timelineScale
                onScaleFactorRequested: value => window.timelineScale = value
                onCameraSelected: sidePanel.section = 5
            }
        }
    }

    Column {
        anchors.centerIn: parent
        spacing: 10
        visible: editor.loading
        opacity: visible ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: 160 } }
        IconImage {
            anchors.horizontalCenter: parent.horizontalCenter
            width: 24; height: 24
            source: "qrc:/omareel/assets/icons/reel.svg"
            sourceSize: Qt.size(24, 24)
            color: theme.accent
            RotationAnimator on rotation {
                running: editor.loading
                loops: Animation.Infinite
                from: 0; to: 360; duration: 720
                easing.type: Easing.InOutSine
            }
        }
        Label {
            text: "Opening recording…"
            color: theme.textMuted
            font.pixelSize: theme.font.body
        }
    }

    ExportDialog { id: exportDialog; parent: Overlay.overlay }

    Shortcut { sequence: "Space"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.playPause() }
    Shortcut { sequence: "Left"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.stepFrames(-1) }
    Shortcut { sequence: "Right"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.stepFrames(1) }
    Shortcut { sequence: "Shift+Left"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.seek(editor.position - 1) }
    Shortcut { sequence: "Shift+Right"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.seek(editor.position + 1) }
    Shortcut { sequence: "S"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.splitAtPlayhead() }
    Shortcut { sequence: "Z"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.addZoomAt(editor.position) }
    Shortcut {
        sequence: "Delete"
        enabled: !window.singleKeyShortcutsBlocked
        onActivated: {
            if (timeline.hasRange) timeline.deleteSelectedRange()
            else if (editor.selectedZoomIds.length) editor.removeSelectedZooms()
            else editor.removeClip(editor.selectedClipId)
        }
    }
    Shortcut {
        sequence: "Backspace"
        enabled: !window.singleKeyShortcutsBlocked && timeline.hasRange
        onActivated: timeline.deleteSelectedRange()
    }
    Shortcut { sequence: "Ctrl+Z"; onActivated: editor.undo() }
    Shortcut { sequence: "Ctrl+Shift+Z"; onActivated: editor.redo() }
    Shortcut { sequence: "Ctrl+S"; onActivated: { topBar.commitProjectName(); editor.saveNow() } }
    Shortcut { sequence: "Ctrl+E"; onActivated: window.openExport() }
    Shortcut { sequence: "?"; enabled: !window.singleKeyShortcutsBlocked; onActivated: bottomBar.openShortcuts() }
    Shortcut {
        sequence: "Escape"
        enabled: editor.pickingZoomTarget || timeline.hasRange
            || editor.selectedClipId || editor.selectedZoomId
        onActivated: {
            if (editor.pickingZoomTarget) editor.setPickingZoomTarget(false)
            else if (timeline.hasRange) timeline.clearRange()
            else editor.selectedClipId = ""
        }
    }
}
