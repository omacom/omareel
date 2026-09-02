import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 920
    height: 640
    minimumWidth: 760
    minimumHeight: 520
    title: "omarecord"
    color: "#111216"
    Material.theme: Material.Dark
    Material.accent: theme.accent
    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 28
        spacing: 18
        Label { text: "omarecord"; font.pixelSize: 28; font.bold: true; color: theme.accent }
        Label { text: "Start a recording"; font.pixelSize: 16; font.bold: true }
        RowLayout {
            Layout.fillWidth: true
            Button { Layout.fillWidth: true; text: "Record fullscreen"; highlighted: true; onClicked: launcher.record("fullscreen") }
            Button { Layout.fillWidth: true; text: "Record region"; onClicked: launcher.record("region") }
            Button { Layout.fillWidth: true; text: "Record window"; onClicked: launcher.record("window") }
        }
        RowLayout {
            Layout.fillWidth: true
            Label { text: "Recent recordings"; font.pixelSize: 16; font.bold: true }
            Item { Layout.fillWidth: true }
            Button { text: "Open bundle…"; onClicked: folderDialog.open() }
        }
        GridView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            model: launcher.recentBundles
            cellWidth: width / 3
            cellHeight: 180
            clip: true
            delegate: Item {
                required property var modelData
                width: GridView.view.cellWidth; height: GridView.view.cellHeight
                Rectangle {
                    anchors.fill: parent; anchors.margins: 7; radius: 10; color: "#1c1e25"; border.color: "#323540"
                    ColumnLayout {
                        anchors.fill: parent; anchors.margins: 8; spacing: 6
                        Rectangle {
                            Layout.fillWidth: true; Layout.preferredHeight: 105; radius: 6; color: "#0b0c10"; clip: true
                            Image { anchors.fill: parent; source: modelData.thumbnail; fillMode: Image.PreserveAspectCrop; visible: source.toString() !== "" }
                            Label { anchors.centerIn: parent; visible: modelData.thumbnail === ""; text: "Recording"; color: "#676a75" }
                        }
                        Label { Layout.fillWidth: true; text: modelData.name; elide: Text.ElideRight; font.bold: true }
                        RowLayout {
                            Layout.fillWidth: true
                            Label { text: editorTime(modelData.duration); color: "#818490" }
                            Item { Layout.fillWidth: true }
                            Button { text: "Open"; onClicked: launcher.openBundle(modelData.path) }
                        }
                    }
                }
            }
        }
    }
    function editorTime(seconds) { let s=Math.round(seconds); return Math.floor(s/60)+":"+String(s%60).padStart(2,"0") }
    FolderDialog { id: folderDialog; title: "Open an omarecord bundle"; onAccepted: launcher.openBundle(selectedFolder) }
}
