import QtQuick
import QtQuick.Controls

Rectangle {
    id: root
    required property bool cropMode
    color: "#0d0e12"
    clip: true

    Rectangle {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, (parent.height - 54) * editor.outputWidth / editor.outputHeight)
        height: width * editor.outputHeight / editor.outputWidth
        color: "#08090b"
        border.color: "#30323a"
        border.width: 1
        clip: true
        Item {
            id: scaled
            anchors.centerIn: parent
            width: editor.outputWidth
            height: editor.outputHeight
            scale: parent.width / width
            Composition {
                id: composition
                anchors.fill: parent
                Component.onCompleted: editor.attachFrameSource(frameSource)
            }
            MouseArea {
                anchors.fill: parent
                enabled: editor.pickingZoomTarget
                cursorShape: Qt.CrossCursor
                onClicked: editor.setZoomTargetFromPreview(mouse.x / width, mouse.y / height)
            }
            Rectangle {
                id: cropRect
                visible: root.cropMode
                x: editor.project.crop.x * parent.width
                y: editor.project.crop.y * parent.height
                width: editor.project.crop.w * parent.width
                height: editor.project.crop.h * parent.height
                color: "transparent"
                border.color: theme.accent
                border.width: 3 / scaled.scale
                property real handleSize: 16 / scaled.scale
                Repeater {
                    model: [{x:0,y:0,corner:0},{x:1,y:0,corner:1},{x:0,y:1,corner:2},{x:1,y:1,corner:3}]
                    delegate: Rectangle {
                        required property var modelData
                        width: cropRect.handleSize; height: width; radius: width / 2
                        x: modelData.x * cropRect.width - width / 2
                        y: modelData.y * cropRect.height - height / 2
                        color: theme.accent; border.color: "white"
                        MouseArea {
                            anchors.fill: parent
                            drag.target: parent
                            cursorShape: Qt.SizeAllCursor
                            onPressed: editor.beginCoalescedEdit("crop")
                            onPositionChanged: {
                                if (!pressed) return
                                let px = Math.max(0, Math.min(scaled.width, cropRect.x + parent.x + parent.width / 2))
                                let py = Math.max(0, Math.min(scaled.height, cropRect.y + parent.y + parent.height / 2))
                                let left = modelData.x === 0 ? px / scaled.width : editor.project.crop.x
                                let top = modelData.y === 0 ? py / scaled.height : editor.project.crop.y
                                let right = modelData.x === 1 ? px / scaled.width : editor.project.crop.x + editor.project.crop.w
                                let bottom = modelData.y === 1 ? py / scaled.height : editor.project.crop.y + editor.project.crop.h
                                if (right - left >= 0.05 && bottom - top >= 0.05) {
                                    editor.setProjectValue("crop.x", left, true)
                                    editor.setProjectValue("crop.y", top, true)
                                    editor.setProjectValue("crop.w", right - left, true)
                                    editor.setProjectValue("crop.h", bottom - top, true)
                                }
                            }
                            onReleased: editor.endCoalescedEdit()
                        }
                    }
                }
            }
        }
    }
    Label {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 10
        text: editor.formatTime(editor.position) + "  /  " + editor.formatTime(editor.duration)
        color: "#8f929d"
        font.family: "monospace"
    }
}
