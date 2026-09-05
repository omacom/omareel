import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import QtQuick.Effects
import QtMultimedia
import Omareel.Ui

ApplicationWindow {
    id: barWindow
    objectName: "recordingBarWindow"
    visible: false
    width: recordingBar.webcam ? 520 : 276
    height: 40
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.Tool

    OmPopupCard {
        anchors.fill: parent
        padding: 0

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 7
            spacing: 7

            IconImage {
                Layout.preferredWidth: 18; Layout.preferredHeight: 18
                source: "qrc:/omareel/assets/icons/reel.svg"
                sourceSize: Qt.size(18, 18)
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

            OmIconButton {
                visible: recordingBar.webcam
                Layout.preferredWidth: 24; Layout.preferredHeight: 24
                focusPolicy: Qt.NoFocus
                icon.source: "qrc:/omareel/assets/icons/lucide/rotate-cw.svg"
                icon.color: theme.foreground
                icon.width: 15; icon.height: 15
                tooltipText: ""
                onClicked: recordingBar.rotateCamera()
                Accessible.name: "Rotate camera"
            }

            OmIconButton {
                visible: recordingBar.webcam
                Layout.preferredWidth: 24; Layout.preferredHeight: 24
                focusPolicy: Qt.NoFocus
                icon.source: "qrc:/omareel/assets/icons/lucide/flip-horizontal-2.svg"
                icon.color: theme.foreground
                icon.width: 15; icon.height: 15
                tooltipText: ""
                selected: recordingBar.cameraFlipHorizontal
                onClicked: recordingBar.flipCamera()
                Accessible.name: "Flip camera"
            }

            OmButton {
                visible: recordingBar.webcam
                Layout.preferredWidth: 130
                Layout.preferredHeight: 28
                focusPolicy: Qt.NoFocus
                font.pixelSize: theme.font.caption
                text: recordingBar.selfViewVisible ? "Hide self-view" : "Show self-view"
                tooltipText: ""
                bordered: true
                onClicked: recordingBar.setSelfViewVisible(!recordingBar.selfViewVisible)
                Accessible.name: text
            }

            OmButton {
                Layout.preferredWidth: 58
                Layout.preferredHeight: 28
                focusPolicy: Qt.NoFocus
                font.pixelSize: theme.font.bodySmall
                font.weight: Font.DemiBold
                onClicked: recordingBar.stop()
                text: "Stop"
                tooltipText: ""
                bordered: true
                primary: true
                Accessible.name: "Stop recording"
            }

            OmButton {
                id: cancelButton
                Layout.fillWidth: true
                Layout.preferredHeight: 28
                text: armed ? "Discard?" : "Discard"
                tooltipText: ""
                focusPolicy: Qt.NoFocus
                font.pixelSize: theme.font.caption
                property bool armed: false
                bordered: true
                destructive: armed
                Accessible.name: armed ? "Confirm discard recording" : "Discard recording"
                onClicked: {
                    if (armed) recordingBar.cancel()
                    else { armed = true; disarm.restart() }
                }
                Timer { id: disarm; interval: 3000; onTriggered: cancelButton.armed = false }
            }
        }
    }

    Window {
        id: countdownWindow
        objectName: "countdownWindow"
        visible: false
        width: 240
        height: 240
        color: "transparent"
        flags: Qt.FramelessWindowHint | Qt.Tool

        Rectangle {
            anchors.centerIn: parent
            width: 180
            height: 180
            radius: 90
            color: Qt.alpha(theme.surface, .94)
            border.width: 3
            border.color: theme.record
            Label {
                anchors.centerIn: parent
                text: recordingBar.countdownValue
                color: theme.foreground
                font.pixelSize: 104
                font.weight: Font.Bold
            }
        }
    }

    Window {
        id: selfViewWindow
        objectName: "selfViewWindow"
        visible: false
        width: recordingBar.selfViewPixels
        height: recordingBar.selfViewPixels
        color: "transparent"
        flags: Qt.FramelessWindowHint | Qt.Tool

        Item {
            id: selfViewContent
            anchors.fill: parent
            opacity: recordingBar.selfViewVisible ? 1 : 0
            enabled: recordingBar.selfViewVisible
            Behavior on opacity { NumberAnimation { duration: 100 } }

            Item {
                id: cameraClip
                anchors.centerIn: parent
                width: parent.width - 10
                height: width
                states: [
                    State { name: "pressed"; when: dragArea.pressed
                        PropertyChanges { target: cameraClip; scale: .98 } },
                    State { name: "hovered"; when: dragArea.containsMouse
                        PropertyChanges { target: cameraClip; scale: 1.03 } }
                ]
                transitions: [
                    Transition { from: "pressed"
                        SpringAnimation { property: "scale"; spring: 4; damping: .35 } },
                    Transition {
                        NumberAnimation { property: "scale"; duration: 120; easing.type: Easing.OutCubic } }
                ]
                clip: true
                layer.enabled: true
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: cameraMask
                }
                Rectangle {
                    id: cameraMask
                    anchors.fill: parent
                    radius: width / 2
                    color: "white"
                    visible: false
                    layer.enabled: true
                }
                Item {
                    id: cameraRotated
                    anchors.centerIn: parent
                    width: parent.width
                    height: parent.height
                    rotation: recordingBar.cameraRotation
                    transform: Scale {
                        origin.x: cameraRotated.width / 2
                        origin.y: cameraRotated.height / 2
                        xScale: recordingBar.cameraFlipHorizontal ? -1 : 1
                    }
                    VideoOutput {
                        id: cameraOutput
                        anchors.fill: parent
                        fillMode: VideoOutput.PreserveAspectCrop
                        Component.onCompleted: recordingBar.attachCameraOutput(cameraOutput)
                    }
                }
                Rectangle {
                    anchors.fill: parent
                    radius: width / 2
                    color: "transparent"
                    border.width: dragArea.pressed ? 2 : 0
                    border.color: Qt.alpha(theme.accent, .4)
                }
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - 20
                visible: recordingBar.cameraError !== ""
                text: recordingBar.cameraError
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                color: theme.foreground
                font.pixelSize: theme.font.caption
            }
            MouseArea {
                id: dragArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.SizeAllCursor
                onPressed: function(mouse) {
                    recordingBar.beginDrag(recordingBar.globalCursorPos())
                }
                onPositionChanged: function(mouse) {
                    if (!pressed) return
                    recordingBar.dragTo(recordingBar.globalCursorPos())
                }
                onReleased: { recordingBar.dragTo(recordingBar.globalCursorPos()); recordingBar.endDrag() }
                onCanceled: recordingBar.endDrag()
            }
        }
    }
}
