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
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground

    readonly property var recordModes: [
        { mode: "fullscreen", label: "Full screen", shortcut: "Ctrl+1", icon: "monitor.svg" },
        { mode: "region", label: "Region", shortcut: "Ctrl+2", icon: "scan.svg" },
        { mode: "window", label: "Window", shortcut: "Ctrl+3", icon: "app-window.svg" }
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
                color: Qt.alpha(theme.foreground, .62)
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: launcher.recording ? 104 : 204
            Layout.minimumHeight: Layout.preferredHeight
            Layout.maximumHeight: Layout.preferredHeight
            ColumnLayout {
                anchors.fill: parent
                spacing: 8
                visible: !launcher.recording
                RowLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 48
                    spacing: 18
                    Switch {
                        text: "System audio"
                        checked: launcher.systemAudio
                        onToggled: launcher.systemAudio = checked
                    }
                    Switch {
                        text: "Microphone"
                        checked: launcher.microphone
                        onToggled: launcher.microphone = checked
                    }
                    ComboBox {
                        Layout.preferredWidth: 250
                        Layout.preferredHeight: 36
                        enabled: launcher.microphone
                        textRole: "text"
                        valueRole: "value"
                        model: launcher.audioDevices
                        Component.onCompleted: currentIndex = indexOfValue(launcher.microphoneDevice)
                        onModelChanged: currentIndex = indexOfValue(launcher.microphoneDevice)
                        onActivated: launcher.microphoneDevice = currentValue
                    }
                    Item { Layout.fillWidth: true }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 40
                    spacing: 18
                    Switch {
                        text: "Webcam"
                        enabled: launcher.webcamDevices.length > 0
                        checked: launcher.webcam
                        onToggled: launcher.webcam = checked
                    }
                    ComboBox {
                        Layout.preferredWidth: 330
                        Layout.preferredHeight: 36
                        enabled: launcher.webcam
                        textRole: "text"
                        valueRole: "value"
                        model: launcher.webcamDevices
                        Component.onCompleted: currentIndex = indexOfValue(launcher.webcamDevice)
                        onModelChanged: currentIndex = indexOfValue(launcher.webcamDevice)
                        onActivated: launcher.webcamDevice = currentValue
                    }
                    ComboBox {
                        Layout.preferredWidth: 112
                        Layout.preferredHeight: 36
                        enabled: launcher.webcam
                        model: [{text:"720p",value:720},{text:"1080p",value:1080}]
                        textRole: "text"
                        valueRole: "value"
                        currentIndex: launcher.webcamHeight === 720 ? 0 : 1
                        onActivated: launcher.webcamHeight = currentValue
                        Accessible.name: "Webcam capture resolution"
                    }
                    Item { Layout.fillWidth: true }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    spacing: 12
                    Repeater {
                        model: window.recordModes
                        delegate: Button {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        topInset: 0; bottomInset: 0
                        onClicked: launcher.record(modelData.mode)
                        contentItem: RowLayout {
                            spacing: 13
                            Item {
                                Layout.preferredWidth: 48
                                Layout.preferredHeight: 48
                                Rectangle {
                                    anchors.fill: parent
                                    radius: 12
                                    color: Qt.alpha(theme.accent, .16)
                                }
                                ToolButton {
                                    anchors.centerIn: parent
                                    width: 24; height: 24
                                    icon.width: 24; icon.height: 24
                                    icon.color: theme.accent
                                    icon.source: "qrc:/omarecord/assets/icons/lucide/" + modelData.icon
                                    enabled: false
                                    opacity: 1
                                    background: null
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
                                    text: modelData.shortcut.replace("+", " + ")
                                    font.pixelSize: 10
                                    font.family: "monospace"
                                    color: Qt.alpha(theme.foreground, .48)
                                }
                            }
                        }
                        background: Rectangle {
                            radius: 10
                            color: parent.pressed ? Qt.alpha(theme.foreground, .10) : parent.hovered ? Qt.alpha(theme.foreground, .07) : theme.lighterBackground
                            border.width: 1
                            border.color: parent.hovered ? Qt.alpha(theme.accent, .72) : Qt.alpha(theme.foreground, .09)
                        }
                        }
                    }
                }
            }

            Rectangle {
                anchors.fill: parent
                visible: launcher.recording
                radius: 10
                color: theme.lighterBackground
                border.width: 1
                border.color: Qt.alpha(theme.foreground, .12)

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 22
                    anchors.rightMargin: 16
                    spacing: 12
                    Rectangle {
                        width: 11; height: 11; radius: 6; color: "#ef4444"
                        SequentialAnimation on opacity {
                            loops: Animation.Infinite
                            NumberAnimation { from: 1; to: .35; duration: 700 }
                            NumberAnimation { from: .35; to: 1; duration: 700 }
                        }
                    }
                    Label {
                        text: "Recording in progress · " + launcher.recordingElapsed
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                    }
                    Item { Layout.fillWidth: true }
                    Button {
                        Layout.preferredWidth: 82
                        focusPolicy: Qt.TabFocus
                        hoverEnabled: true
                        onClicked: launcher.stopRecording()
                        contentItem: Label {
                            text: "Stop"
                            color: theme.accentForeground
                            font: parent.font
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: 6
                            color: parent.down ? Qt.darker(theme.accent, 1.15)
                                               : parent.hovered ? Qt.lighter(theme.accent, 1.08) : theme.accent
                        }
                    }
                    Button {
                        id: launcherCancelButton
                        Layout.preferredWidth: 90
                        text: armed ? "Discard?" : "Cancel"
                        flat: true
                        focusPolicy: Qt.TabFocus
                        property bool armed: false
                        onClicked: {
                            if (armed) launcher.cancelRecording()
                            else {
                                armed = true
                                launcherCancelTimer.restart()
                            }
                        }
                    }
                    Timer {
                        id: launcherCancelTimer
                        interval: 3000
                        onTriggered: launcherCancelButton.armed = false
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
            Label { text: launcher.recentBundles.length; color: Qt.alpha(theme.foreground, .46); font.pixelSize: 11 }
            Item { Layout.fillWidth: true }
            Button {
                Layout.preferredHeight: 32
                text: "Open bundle…"
                font.pixelSize: 12
                focusPolicy: Qt.TabFocus
                topInset: 0; bottomInset: 0
                onClicked: folderDialog.open()
                background: Rectangle {
                    radius: 6
                    color: parent.hovered ? Qt.alpha(theme.foreground, .06) : "transparent"
                    border.color: parent.hovered ? Qt.alpha(theme.foreground, .23) : Qt.alpha(theme.foreground, .14)
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
                cellHeight: Math.ceil((cellWidth - 36) * 9 / 16) + 84
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
                        topInset: 0; bottomInset: 0
                        onClicked: launcher.openBundle(modelData.path)
                        background: Rectangle {
                            radius: 10
                            color: parent.pressed ? Qt.alpha(theme.foreground, .10) : parent.hovered ? Qt.alpha(theme.foreground, .06) : theme.lighterBackground
                            border.width: 1
                            border.color: parent.hovered ? Qt.alpha(theme.accent, .72) : Qt.alpha(theme.foreground, .08)
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
                                    color: Qt.alpha(theme.foreground, .40)
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
                                Label { text: modelData.dateText; color: Qt.alpha(theme.foreground, .52); font.pixelSize: 11 }
                                Item { Layout.fillWidth: true }
                                Label { text: modelData.durationText; color: Qt.alpha(theme.foreground, .52); font.pixelSize: 11; font.family: "monospace" }
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                anchors.centerIn: parent
                visible: launcher.recentBundles.length === 0
                spacing: 6
                ToolButton {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: 30; Layout.preferredHeight: 30
                    icon.width: 30; icon.height: 30
                    icon.color: theme.foreground
                    icon.source: "qrc:/omarecord/assets/icons/lucide/film.svg"
                    enabled: false
                    background: null
                    opacity: .45
                }
                Label { text: "No recordings yet"; font.pixelSize: 14; font.weight: Font.DemiBold; Layout.alignment: Qt.AlignHCenter }
                Label { text: "Your finished recordings will appear here."; color: Qt.alpha(theme.foreground, .48); font.pixelSize: 12; Layout.alignment: Qt.AlignHCenter }
            }
        }
    }

    Instantiator {
        model: window.recordModes
        delegate: Shortcut {
            required property var modelData
            sequence: modelData.shortcut
            enabled: !launcher.recording
            onActivated: launcher.record(modelData.mode)
        }
    }
    FolderDialog { id: folderDialog; title: "Open an omarecord bundle"; onAccepted: launcher.openBundle(selectedFolder) }
}
