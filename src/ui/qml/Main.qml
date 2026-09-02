import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 1440
    height: 900
    minimumWidth: 1100
    minimumHeight: 700
    color: theme.background
    title: editor.bundleName + " — omarecord"
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground
    property real timelineScale: 1
    property bool cropMode: false
    readonly property bool singleKeyShortcutsBlocked: activeFocusItem !== null
        && !previewPane.activeFocus && !timeline.activeFocus
    function restoreEditorFocus() { previewPane.forceActiveFocus() }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        TopBar {
            id: topBar
            Layout.fillWidth: true
            onShowExport: exportDialog.open()
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
                    Layout.fillWidth: true
                    timelineScale: window.timelineScale
                    cropMode: window.cropMode
                    onTimelineScaleChanged: window.timelineScale = timelineScale
                    onCropModeChanged: window.cropMode = cropMode
                }
            }
            SidePanel {
                Layout.preferredWidth: 320
                Layout.minimumWidth: 320
                Layout.maximumWidth: 320
                Layout.fillHeight: true
            }
        }
        Timeline {
            id: timeline
            Layout.fillWidth: true
            Layout.preferredHeight: 168
            scaleFactor: window.timelineScale
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
    Shortcut { sequence: "Delete"; enabled: !window.singleKeyShortcutsBlocked; onActivated: editor.selectedZoomId ? editor.removeZoom(editor.selectedZoomId) : editor.removeClip(editor.selectedClipId) }
    Shortcut { sequence: "Ctrl+Z"; onActivated: editor.undo() }
    Shortcut { sequence: "Ctrl+Shift+Z"; onActivated: editor.redo() }
    Shortcut { sequence: "Ctrl+S"; onActivated: { topBar.commitProjectName(); editor.saveNow() } }
    Shortcut { sequence: "Ctrl+E"; onActivated: { topBar.commitProjectName(); exportDialog.open() } }
    Shortcut { sequence: "Escape"; enabled: editor.pickingZoomTarget; onActivated: editor.setPickingZoomTarget(false) }
}
