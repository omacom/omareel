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
                tooltipText: "Hide self-view while recording"
                bordered: true
                onClicked: recordingBar.setSelfViewVisible(!recordingBar.selfViewVisible)
            }

            OmButton {
                Layout.preferredWidth: 58
                Layout.preferredHeight: 28
                focusPolicy: Qt.NoFocus
                font.pixelSize: theme.font.bodySmall
                font.weight: Font.DemiBold
                onClicked: recordingBar.stop()
                text: "Stop"
                bordered: true
                primary: true
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
                    else { armed = true; disarm.restart() }
                }
                Timer { id: disarm; interval: 3000; onTriggered: cancelButton.armed = false }
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

            VideoOutput {
                id: cameraOutput
                anchors.centerIn: parent
                width: recordingBar.cameraRotation === 90 || recordingBar.cameraRotation === 270
                    ? parent.height : parent.width
                height: recordingBar.cameraRotation === 90 || recordingBar.cameraRotation === 270
                    ? parent.width : parent.height
                fillMode: VideoOutput.PreserveAspectCrop
                rotation: recordingBar.cameraRotation
                transform: Scale {
                    origin.x: cameraOutput.width / 2
                    origin.y: cameraOutput.height / 2
                    xScale: recordingBar.cameraFlipHorizontal ? -1 : 1
                }
                Component.onCompleted: recordingBar.attachCameraOutput(cameraOutput)
            }
            ShaderEffectSource {
                id: cameraTexture
                anchors.fill: parent
                sourceItem: cameraOutput
                hideSource: true
                live: true
            }
            Rectangle {
                id: cameraMask
                anchors.fill: parent
                radius: width / 2
                color: "white"
                visible: false
                layer.enabled: true
            }
            MultiEffect {
                anchors.fill: parent
                source: cameraTexture
                maskEnabled: true
                maskSource: cameraMask
                maskSpreadAtMin: 1
                maskThresholdMin: .5
            }
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: "transparent"
                border.width: 2
                border.color: theme.normalBorder
                antialiasing: true
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
                anchors.fill: parent
                cursorShape: Qt.SizeAllCursor
                property real lastX: 0
                property real lastY: 0
                onPressed: function(mouse) { lastX = mouse.x; lastY = mouse.y }
                onPositionChanged: function(mouse) {
                    if (!pressed) return
                    const dx = Math.round(mouse.x - lastX)
                    const dy = Math.round(mouse.y - lastY)
                    if (dx !== 0 || dy !== 0) recordingBar.moveSelfView(dx, dy)
                    lastX = mouse.x
                    lastY = mouse.y
                }
            }
        }
    }
}
