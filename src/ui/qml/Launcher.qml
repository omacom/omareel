import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: true
    width: 900
    height: 600
    minimumWidth: 760
    minimumHeight: 520
    title: "omarecord"
    color: theme.background
    Material.theme: Material.Dark
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground

    readonly property var recordModes: [
        { mode: "fullscreen", label: "Full screen", hint: "Ctrl + 1", icon: "monitor.svg" },
        { mode: "region", label: "Region", hint: "Ctrl + 2", icon: "region.svg" },
        { mode: "window", label: "Window", hint: "Ctrl + 3", icon: "window.svg" }
    ]

    ColumnLayout {
        anchors.fill: parent
        anchors.leftMargin: 28
        anchors.rightMargin: 28
        anchors.topMargin: 24
        anchors.bottomMargin: 24
        spacing: 16

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Label {
                text: "omarecord"
                font.pixelSize: 27
                font.weight: Font.DemiBold
                color: theme.accent
            }
            Label {
                text: "Capture once. Polish every detail."
                font.pixelSize: 13
                color: "#9a9eab"
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 104
            Layout.minimumHeight: 104
            Layout.maximumHeight: 104
            spacing: 12
            Repeater {
                model: window.recordModes
                delegate: Button {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    hoverEnabled: true
                    focusPolicy: Qt.TabFocus
                    onClicked: launcher.record(modelData.mode)
                    contentItem: RowLayout {
                        spacing: 13
                        Item {
                            Layout.preferredWidth: 48
                            Layout.preferredHeight: 48
                            Rectangle {
                                anchors.fill: parent
                                radius: 12
                                color: Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .16)
                            }
                            Image {
                                anchors.centerIn: parent
                                width: 24; height: 24
                                sourceSize.width: 48; sourceSize.height: 48
                                source: "qrc:/omarecord/assets/icons/" + modelData.icon
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Label {
                                Layout.fillWidth: true
                                text: modelData.label
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                color: theme.foreground
                            }
                            Label {
                                text: modelData.hint
                                font.pixelSize: 10
                                font.family: "monospace"
                                color: "#7f8391"
                            }
                        }
                    }
                    background: Rectangle {
                        radius: 10
                        color: parent.pressed ? "#1affffff" : parent.hovered ? "#12ffffff" : theme.lighterBackground
                        border.width: 1
                        border.color: parent.hovered ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .72) : "#16ffffff"
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 2
            Label {
                text: "Recent recordings"
                font.pixelSize: 16
                font.weight: Font.DemiBold
            }
            Label { text: launcher.recentBundles.length; color: "#777b89"; font.pixelSize: 11 }
            Item { Layout.fillWidth: true }
            Button {
                Layout.preferredHeight: 32
                text: "Open bundle…"
                font.pixelSize: 12
                focusPolicy: Qt.TabFocus
                onClicked: folderDialog.open()
                background: Rectangle {
                    radius: 6
                    color: parent.hovered ? "#10ffffff" : "transparent"
                    border.color: parent.hovered ? "#3bffffff" : "#24ffffff"
                }
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            GridView {
                id: recentGrid
                anchors.fill: parent
                visible: launcher.recentBundles.length > 0
                model: launcher.recentBundles
                readonly property int columns: Math.max(1, Math.floor(width / 260))
                cellWidth: width / columns
                cellHeight: 202
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                delegate: Item {
                    required property var modelData
                    width: GridView.view.cellWidth
                    height: GridView.view.cellHeight
                    Button {
                        anchors.fill: parent
                        anchors.margins: 6
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        onClicked: launcher.openBundle(modelData.path)
                        background: Rectangle {
                            radius: 10
                            color: parent.pressed ? "#1affffff" : parent.hovered ? "#10ffffff" : theme.lighterBackground
                            border.width: 1
                            border.color: parent.hovered ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .72) : "#14ffffff"
                        }
                        contentItem: ColumnLayout {
                            spacing: 7
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: width * 9 / 16
                                radius: 7
                                color: Qt.darker(theme.background, 1.2)
                                clip: true
                                Image {
                                    anchors.fill: parent
                                    source: modelData.thumbnail
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    sourceSize.width: 560
                                    sourceSize.height: 315
                                    visible: source.toString() !== ""
                                }
                                Label {
                                    anchors.centerIn: parent
                                    visible: modelData.thumbnail === ""
                                    text: "No preview"
                                    color: "#646875"
                                    font.pixelSize: 11
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: modelData.name
                                elide: Text.ElideRight
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                                color: theme.foreground
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: window.formatDate(modelData.date); color: "#858997"; font.pixelSize: 11 }
                                Item { Layout.fillWidth: true }
                                Label { text: window.formatTime(modelData.duration); color: "#858997"; font.pixelSize: 11; font.family: "monospace" }
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                anchors.centerIn: parent
                visible: launcher.recentBundles.length === 0
                spacing: 6
                Image {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: 30; Layout.preferredHeight: 30
                    sourceSize.width: 60; sourceSize.height: 60
                    source: "qrc:/omarecord/assets/icons/film.svg"
                    opacity: .45
                }
                Label { text: "No recordings yet"; font.pixelSize: 14; font.weight: Font.DemiBold; Layout.alignment: Qt.AlignHCenter }
                Label { text: "Your finished recordings will appear here."; color: "#7f8391"; font.pixelSize: 12; Layout.alignment: Qt.AlignHCenter }
            }
        }
    }

    function formatTime(seconds) {
        let s = Math.round(seconds)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }
    function formatDate(value) { return Qt.formatDate(new Date(value), "MMM d, yyyy") }

    Shortcut { sequence: "Ctrl+1"; onActivated: launcher.record("fullscreen") }
    Shortcut { sequence: "Ctrl+2"; onActivated: launcher.record("region") }
    Shortcut { sequence: "Ctrl+3"; onActivated: launcher.record("window") }
    FolderDialog { id: folderDialog; title: "Open an omarecord bundle"; onAccepted: launcher.openBundle(selectedFolder) }
}
