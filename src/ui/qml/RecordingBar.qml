import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtMultimedia
import Omarecord.Ui

ApplicationWindow {
    id: window
    visible: false
    width: recordingBar.webcam ? 448 : 276
    height: recordingBar.webcam ? 120 : 40
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.Tool

    RowLayout {
        anchors.fill: parent
        spacing: 8

        Rectangle {
            Layout.preferredWidth: 120
            Layout.preferredHeight: 120
            visible: recordingBar.webcam
            radius: 60
            clip: true
            color: theme.surface
            border.width: 2
            border.color: theme.normalBorder

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
                color: theme.foreground
                font.pixelSize: theme.font.caption
            }
        }

        OmPopupCard {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            Layout.alignment: Qt.AlignVCenter
            padding: 0

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 7
                spacing: 7

                Rectangle {
                    width: 9
                    height: 9
                    radius: 5
                    color: theme.record
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
                    font.family: theme.monoFamily
                    font.pixelSize: theme.font.body
                    font.weight: Font.DemiBold
                }

                IconToolButton {
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

                IconToolButton {
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

                OmButton {
                    Layout.preferredWidth: 58
                    Layout.preferredHeight: 28
                    focusPolicy: Qt.NoFocus
                    hoverEnabled: true
                    font.pixelSize: theme.font.bodySmall
                    font.weight: Font.DemiBold
                    topInset: 0; bottomInset: 0
                    onClicked: recordingBar.stop()
                    text: "Stop"
                    bordered: true
                    active: true
                }

                OmButton {
                    id: cancelButton
                    Layout.fillWidth: true
                    Layout.preferredHeight: 28
                    text: armed ? "Discard?" : "Discard"
                    focusPolicy: Qt.NoFocus
                    font.pixelSize: theme.font.caption
                    property bool armed: false
                    bordered: true
                    destructive: armed
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
