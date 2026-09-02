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
    color: "#111216"
    title: editor.bundleName + " — omarecord"
    Material.theme: Material.Dark
    Material.accent: theme.accent
    property real timelineScale: 1
    property bool cropMode: false

    ColumnLayout {
        anchors.fill: parent
        spacing: 0
        TopBar {
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
                Layout.preferredWidth: 300
                Layout.minimumWidth: 300
                Layout.maximumWidth: 300
                Layout.fillHeight: true
            }
        }
        Timeline {
            Layout.fillWidth: true
            Layout.preferredHeight: 230
            scaleFactor: window.timelineScale
        }
    }

    ExportDialog { id: exportDialog; parent: Overlay.overlay }

    Shortcut { sequence: "Space"; onActivated: editor.playPause() }
    Shortcut { sequence: "Left"; onActivated: editor.stepFrames(-1) }
    Shortcut { sequence: "Right"; onActivated: editor.stepFrames(1) }
    Shortcut { sequence: "Shift+Left"; onActivated: editor.seek(editor.position - 1) }
    Shortcut { sequence: "Shift+Right"; onActivated: editor.seek(editor.position + 1) }
    Shortcut { sequence: "S"; onActivated: editor.splitAtPlayhead() }
    Shortcut { sequence: "Z"; onActivated: editor.addZoomAt(editor.position) }
    Shortcut { sequence: "Delete"; onActivated: editor.selectedZoomId ? editor.removeZoom(editor.selectedZoomId) : editor.removeClip(editor.selectedClipId) }
    Shortcut { sequence: "Ctrl+Z"; onActivated: editor.undo() }
    Shortcut { sequence: "Ctrl+Shift+Z"; onActivated: editor.redo() }
    Shortcut { sequence: "Ctrl+S"; onActivated: editor.saveNow() }
    Shortcut { sequence: "Ctrl+E"; onActivated: exportDialog.open() }
}
