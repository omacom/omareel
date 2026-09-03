import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts
import QtMultimedia

ApplicationWindow {
    id: window
    visible: false
    width: recordingBar.webcam ? 408 : 236
    height: recordingBar.webcam ? 120 : 40
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.Tool
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground

    RowLayout {
        anchors.fill: parent
        spacing: 8

        Rectangle {
            Layout.preferredWidth: 120
            Layout.preferredHeight: 120
            visible: recordingBar.webcam
            radius: 60
            clip: true
            color: "#111111"
            border.width: 2
            border.color: Qt.alpha(theme.foreground, .22)

            VideoOutput {
                id: cameraOutput
                anchors.centerIn: parent
                width: parent.width
                height: parent.height
                fillMode: VideoOutput.PreserveAspectCrop
                rotation: recordingBar.cameraRotation
                transform: Scale {
                    origin.x: cameraOutput.width / 2
                    origin.y: cameraOutput.height / 2
                    xScale: recordingBar.cameraFlipHorizontal ? -1 : 1
                }
                Component.onCompleted: recordingBar.attachCameraOutput(cameraOutput)
            }

            Label {
                anchors.centerIn: parent
                width: parent.width - 16
                visible: recordingBar.cameraError !== ""
                text: recordingBar.cameraError
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                color: "white"
                font.pixelSize: 10
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            Layout.alignment: Qt.AlignVCenter
            radius: 20
            color: Qt.alpha(theme.background, .92)
            border.width: 1
            border.color: Qt.alpha(theme.foreground, .15)

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 7
                spacing: 7

                Rectangle {
                    width: 9
                    height: 9
                    radius: 5
                    color: "#ef4444"
                    SequentialAnimation on opacity {
                        loops: Animation.Infinite
                        NumberAnimation { from: 1; to: .35; duration: 700; easing.type: Easing.InOutSine }
                        NumberAnimation { from: .35; to: 1; duration: 700; easing.type: Easing.InOutSine }
                    }
                }

                Label {
                    Layout.preferredWidth: 44
                    text: recordingBar.elapsed
                    color: theme.foreground
                    font.family: "monospace"
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }

                ToolButton {
                    visible: recordingBar.webcam
                    Layout.preferredWidth: 24
                    Layout.preferredHeight: 24
                    focusPolicy: Qt.NoFocus
                    icon.source: "qrc:/omarecord/assets/icons/lucide/rotate-cw.svg"
                    icon.color: theme.foreground
                    icon.width: 15; icon.height: 15
                    onClicked: recordingBar.rotateCamera()
                    Accessible.name: "Rotate camera"
                }

                ToolButton {
                    visible: recordingBar.webcam
                    Layout.preferredWidth: 24
                    Layout.preferredHeight: 24
                    focusPolicy: Qt.NoFocus
                    icon.source: "qrc:/omarecord/assets/icons/lucide/flip-horizontal-2.svg"
                    icon.color: theme.foreground
                    icon.width: 15; icon.height: 15
                    onClicked: recordingBar.flipCamera()
                    Accessible.name: "Flip camera"
                }

                Button {
                    Layout.preferredWidth: 58
                    Layout.preferredHeight: 28
                    focusPolicy: Qt.NoFocus
                    hoverEnabled: true
                    font.pixelSize: 11
                    font.weight: Font.DemiBold
                    topInset: 0; bottomInset: 0
                    onClicked: recordingBar.stop()
                    contentItem: Label {
                        text: "Stop"
                        color: theme.accentForeground
                        font: parent.font
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Rectangle {
                        radius: 14
                        color: parent.down ? Qt.darker(theme.accent, 1.15)
                                           : parent.hovered ? Qt.lighter(theme.accent, 1.08) : theme.accent
                    }
                }

                Button {
                    id: cancelButton
                    Layout.fillWidth: true
                    Layout.preferredHeight: 28
                    text: armed ? "Discard?" : "Cancel"
                    flat: true
                    focusPolicy: Qt.NoFocus
                    font.pixelSize: 10
                    property bool armed: false
                    topInset: 0; bottomInset: 0
                    onClicked: {
                        if (armed) recordingBar.cancel()
                        else {
                            armed = true
                            disarm.restart()
                        }
                    }
                    Timer { id: disarm; interval: 3000; onTriggered: cancelButton.armed = false }
                }
            }
        }
    }
}
