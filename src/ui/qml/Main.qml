import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 1440
    height: 900
    minimumWidth: 1200
    minimumHeight: 760
    font.family: theme.fontFamily
    font.pixelSize: 13
    color: theme.surface
    title: editor.bundleName + " — omarecord"
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.surface
    Material.foreground: theme.foreground
    property real timelineScale: 1
    property bool cropMode: false
    readonly property bool singleKeyShortcutsBlocked: activeFocusItem !== null
        && !previewPane.activeFocus && !timeline.activeFocus
    function restoreEditorFocus() { previewPane.forceActiveFocus() }
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
        else if (view === "background-expanded") sidePanel.setBackgroundExpanded(true)
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
                spacing: 0
                PreviewPane {
                    id: previewPane
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    cropMode: window.cropMode
                }
                BottomBar {
                    id: bottomBar
                    Layout.fillWidth: true
                    timelineScale: window.timelineScale
                    cropMode: window.cropMode
                    onTimelineScaleChanged: window.timelineScale = timelineScale
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
            Layout.fillWidth: true
            Layout.preferredHeight: implicitHeight
            scaleFactor: window.timelineScale
            onScaleFactorRequested: value => window.timelineScale = value
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
    Shortcut { sequence: "Delete"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.selectedZoomIds.length ? editor.removeSelectedZooms() : editor.removeClip(editor.selectedClipId) }
    Shortcut { sequence: "Ctrl+Z"; onActivated: editor.undo() }
    Shortcut { sequence: "Ctrl+Shift+Z"; onActivated: editor.redo() }
    Shortcut { sequence: "Ctrl+S"; onActivated: { topBar.commitProjectName(); editor.saveNow() } }
    Shortcut { sequence: "Ctrl+E"; onActivated: window.openExport() }
    Shortcut { sequence: "Escape"; enabled: editor.pickingZoomTarget; onActivated: editor.setPickingZoomTarget(false) }
}
