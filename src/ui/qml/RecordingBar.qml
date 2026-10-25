import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import QtQuick.Effects
import QtQuick.Shapes
import QtMultimedia
import Omareel.Ui

ApplicationWindow {
    id: barWindow
    objectName: "recordingBarWindow"
    visible: false
    // The window leaves room for the discard confirmation to unfold; the pill stays centred.
    width: recordingBar.webcam ? 448 : 288
    height: 40
    font.family: theme.fontFamily
    font.pixelSize: theme.type.body
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.Tool

    component BarIcon: Button {
        id: barIcon
        property url iconSource
        property bool on: false
        implicitWidth: 28; implicitHeight: 28
        focusPolicy: Qt.NoFocus
        hoverEnabled: true
        scale: down ? .9 : 1
        Behavior on scale { NumberAnimation { duration: down ? 70 : 240; easing.type: Easing.OutBack; easing.overshoot: 2.4 } }
        background: Rectangle {
            radius: theme.rounded ? width / 2 : 0
            color: barIcon.down ? theme.fillPressed : barIcon.hovered ? theme.fillHover : "transparent"
            Behavior on color { ColorAnimation { duration: 120 } }
        }
        contentItem: Item {
            IconImage {
                anchors.centerIn: parent
                width: 15; height: 15
                source: barIcon.iconSource
                sourceSize: Qt.size(15, 15)
                color: barIcon.on ? theme.accent : barIcon.hovered ? theme.textStrong : theme.textMuted
                Behavior on color { ColorAnimation { duration: 120 } }
            }
        }
    }

    onVisibleChanged: if (visible) barEntrance.restart()
    ParallelAnimation {
        id: barEntrance
        NumberAnimation { target: pill; property: "opacity"; from: 0; to: 1; duration: 200; easing.type: Easing.OutCubic }
        NumberAnimation { target: pill; property: "slide"; from: -10; to: 0; duration: 380; easing.type: Easing.OutBack; easing.overshoot: 1.4 }
    }

    Rectangle {
        id: pill
        property real slide: 0
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.verticalCenter: parent.verticalCenter
        anchors.verticalCenterOffset: slide
        width: content.implicitWidth + 16
        height: 40
        radius: theme.rounded ? height / 2 : 0
        color: theme.surface
        border.width: 1
        border.color: theme.strokeStrong
        Behavior on width { NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }

        RowLayout {
            id: content
            anchors.centerIn: parent
            spacing: 4

            Rectangle {
                Layout.leftMargin: 6
                Layout.preferredWidth: 8; Layout.preferredHeight: 8
                radius: 4
                color: theme.record
                SequentialAnimation on opacity {
                    loops: Animation.Infinite
                    NumberAnimation { from: 1; to: .25; duration: 800; easing.type: Easing.InOutSine }
                    NumberAnimation { from: .25; to: 1; duration: 800; easing.type: Easing.InOutSine }
                }
            }
            Label {
                Layout.leftMargin: 4
                Layout.rightMargin: 6
                text: recordingBar.elapsed
                color: theme.textStrong
                font.family: theme.monoFamily
                font.pixelSize: theme.type.body
                font.weight: Font.Medium
            }

            Rectangle {
                visible: recordingBar.webcam
                Layout.preferredWidth: 1; Layout.preferredHeight: 16
                Layout.leftMargin: 2; Layout.rightMargin: 2
                color: theme.separator
            }
            BarIcon {
                visible: recordingBar.webcam
                iconSource: recordingBar.selfViewVisible ? "qrc:/omareel/assets/icons/lucide/eye.svg"
                                                         : "qrc:/omareel/assets/icons/lucide/eye-off.svg"
                on: recordingBar.selfViewVisible
                onClicked: recordingBar.setSelfViewVisible(!recordingBar.selfViewVisible)
                Accessible.name: recordingBar.selfViewVisible ? "Hide self-view" : "Show self-view"
            }
            BarIcon {
                visible: recordingBar.webcam
                iconSource: "qrc:/omareel/assets/icons/lucide/flip-horizontal-2.svg"
                on: recordingBar.cameraFlipHorizontal
                onClicked: recordingBar.flipCamera()
                Accessible.name: "Flip camera"
            }
            BarIcon {
                visible: recordingBar.webcam
                iconSource: "qrc:/omareel/assets/icons/lucide/rotate-cw.svg"
                onClicked: recordingBar.rotateCamera()
                Accessible.name: "Rotate camera"
            }
            BarIcon {
                visible: recordingBar.webcam
                iconSource: recordingBar.cameraShape === "round" ? "qrc:/omareel/assets/icons/lucide/circle.svg"
                                                                 : "qrc:/omareel/assets/icons/lucide/square.svg"
                onClicked: recordingBar.cycleCameraShape()
                Accessible.name: "Camera shape"
            }
            BarIcon {
                visible: recordingBar.webcam
                iconSource: "qrc:/omareel/assets/icons/lucide/scan.svg"
                onClicked: recordingBar.snapSelfViewToNextCorner()
                Accessible.name: "Move self-view to the next corner"
            }

            Rectangle {
                Layout.preferredWidth: 1; Layout.preferredHeight: 16
                Layout.leftMargin: 2; Layout.rightMargin: 4
                color: theme.separator
            }

            Button {
                id: stopButton
                Layout.preferredHeight: 28
                focusPolicy: Qt.NoFocus
                hoverEnabled: true
                leftPadding: 10; rightPadding: 12
                onClicked: recordingBar.stop()
                enabled: !recordingBar.stopping
                Accessible.name: "Stop recording"
                scale: down ? .94 : 1
                Behavior on scale { NumberAnimation { duration: stopButton.down ? 70 : 240; easing.type: Easing.OutBack; easing.overshoot: 2.4 } }
                background: Rectangle {
                    radius: theme.rounded ? height / 2 : 0
                    color: stopButton.down ? Qt.darker(theme.record, 1.12)
                         : stopButton.hovered ? Qt.lighter(theme.record, 1.08) : theme.record
                    Behavior on color { ColorAnimation { duration: 120 } }
                }
                contentItem: Row {
                    spacing: 7
                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 9; height: 9
                        radius: theme.rounded ? 2 : 0
                        color: "white"
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: recordingBar.stopping ? "Saving…" : "Stop"
                        color: "white"
                        font.pixelSize: theme.type.small
                        font.weight: Font.DemiBold
                    }
                }
            }

            Button {
                id: cancelButton
                objectName: "discardButton"
                enabled: !recordingBar.stopping
                property bool armed: false
                Layout.preferredHeight: 28
                Layout.preferredWidth: armed ? confirmLabel.implicitWidth + 40 : 28
                Layout.rightMargin: 2
                focusPolicy: Qt.NoFocus
                hoverEnabled: true
                clip: true
                Accessible.name: armed ? "Confirm discard recording" : "Discard recording"
                Behavior on Layout.preferredWidth { NumberAnimation { duration: 240; easing.type: Easing.OutCubic } }
                onClicked: {
                    if (armed) recordingBar.cancel()
                    else { armed = true; disarm.restart() }
                }
                Timer { id: disarm; interval: 3000; onTriggered: cancelButton.armed = false }
                background: Rectangle {
                    radius: theme.rounded ? height / 2 : 0
                    color: cancelButton.armed ? Qt.alpha(theme.record, cancelButton.hovered ? .26 : .18)
                         : cancelButton.down ? theme.fillPressed : cancelButton.hovered ? theme.fillHover : "transparent"
                    Behavior on color { ColorAnimation { duration: 140 } }
                }
                contentItem: Item {
                    IconImage {
                        id: trashIcon
                        x: cancelButton.armed ? 9 : (parent.width - width) / 2
                        anchors.verticalCenter: parent.verticalCenter
                        width: 15; height: 15
                        source: "qrc:/omareel/assets/icons/lucide/trash-2.svg"
                        sourceSize: Qt.size(15, 15)
                        color: cancelButton.armed ? theme.record : cancelButton.hovered ? theme.textStrong : theme.textMuted
                        Behavior on x { NumberAnimation { duration: 240; easing.type: Easing.OutCubic } }
                    }
                    Label {
                        id: confirmLabel
                        x: trashIcon.x + trashIcon.width + 6
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Discard?"
                        color: theme.record
                        font.pixelSize: theme.type.small
                        font.weight: Font.DemiBold
                        opacity: cancelButton.armed ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: 180 } }
                    }
                }
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

        Item {
            id: countdownDial
            anchors.centerIn: parent
            width: 168; height: 168
            property real sweep: 1
            Connections {
                target: recordingBar
                function onCountdownChanged() {
                    countdownPop.restart()
                    countdownSweep.restart()
                }
            }
            NumberAnimation { id: countdownSweep; target: countdownDial; property: "sweep"; from: 1; to: 0; duration: 1000 }
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: Qt.alpha(theme.surface, .94)
                border.width: 1
                border.color: theme.strokeStrong
            }
            Shape {
                anchors.fill: parent
                preferredRendererType: Shape.CurveRenderer
                ShapePath {
                    strokeColor: Qt.alpha(theme.foreground, .12)
                    strokeWidth: 4
                    fillColor: "transparent"
                    PathAngleArc { centerX: 84; centerY: 84; radiusX: 72; radiusY: 72; startAngle: 0; sweepAngle: 360 }
                }
                ShapePath {
                    strokeColor: theme.record
                    strokeWidth: 4
                    capStyle: ShapePath.RoundCap
                    fillColor: "transparent"
                    PathAngleArc { centerX: 84; centerY: 84; radiusX: 72; radiusY: 72; startAngle: -90; sweepAngle: 360 * countdownDial.sweep }
                }
            }
            Label {
                id: countdownNumber
                anchors.centerIn: parent
                anchors.verticalCenterOffset: -6
                text: recordingBar.countdownValue
                color: theme.textStrong
                font.family: theme.monoFamily
                font.pixelSize: 72
                font.weight: Font.Medium
                SequentialAnimation {
                    id: countdownPop
                    ParallelAnimation {
                        NumberAnimation { target: countdownNumber; property: "scale"; from: 1.35; to: 1; duration: 420; easing.type: Easing.OutBack; easing.overshoot: 1.8 }
                        NumberAnimation { target: countdownNumber; property: "opacity"; from: 0; to: 1; duration: 200 }
                    }
                }
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.top: countdownNumber.bottom
                anchors.topMargin: -4
                text: "Get ready"
                color: theme.textMuted
                font.pixelSize: theme.type.small
                font.weight: Font.Medium
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
                readonly property real cornerRadius: recordingBar.cameraShape === "round"
                    ? width / 2 : Math.min(width / 2, recordingBar.cameraRadius)
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
                    radius: cameraClip.cornerRadius
                    color: "white"
                    visible: false
                    layer.enabled: true
                }
                Rectangle {
                    anchors.fill: parent
                    visible: !recordingBar.cameraReady
                    color: Qt.alpha(theme.surface, .94)
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
                    radius: cameraClip.cornerRadius
                    color: "transparent"
                    border.width: dragArea.pressed ? 2 : 1.5
                    border.color: dragArea.pressed ? Qt.alpha(theme.accent, .7) : Qt.alpha("white", .16)
                    Behavior on border.color { ColorAnimation { duration: 140 } }
                }
            }
            Label {
                anchors.centerIn: parent
                width: parent.width - 20
                visible: text !== ""
                text: recordingBar.cameraError !== "" ? recordingBar.cameraError
                    : recordingBar.cameraReady ? "" : "Starting camera…"
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignHCenter
                color: theme.foreground
                font.pixelSize: theme.type.caption
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
