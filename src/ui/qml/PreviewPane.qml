import QtQuick
import QtQuick.Controls

FocusScope {
    id: root
    required property bool cropMode
    function grabPreview(callback) {
        previewFrame.grabToImage(callback, Qt.size(640, 360))
    }
    clip: true
    Rectangle { anchors.fill: parent; color: theme.darkBackground }
    MouseArea {
        anchors.fill: parent
        onPressed: root.forceActiveFocus()
    }

    Item {
        id: previewArea
        anchors.fill: parent
        anchors.leftMargin: 24
        anchors.rightMargin: 24
        anchors.topMargin: 24
        anchors.bottomMargin: 46

        Rectangle {
        id: previewFrame
        anchors.centerIn: parent
        width: Math.min(parent.width, parent.height * editor.outputWidth / editor.outputHeight)
        height: width * editor.outputHeight / editor.outputWidth
        color: theme.darkBackground
        border.color: Qt.alpha(theme.foreground, .06)
        border.width: 1
        radius: 8
        clip: true
        Item {
            id: scaled
            anchors.centerIn: parent
            width: editor.outputWidth
            height: editor.outputHeight
            scale: parent.width / width
            readonly property var selectedZoom: {
                for (let i = 0; i < editor.zooms.length; ++i)
                    if (editor.zooms[i].id === editor.selectedZoomId) return editor.zooms[i]
                return null
            }
            readonly property bool noBackground: editor.project.background.type === "none"
            readonly property real pad: noBackground ? 0 : Math.min(width, height) * editor.project.frame.padding
            readonly property real sourceAspect: (editor.sourceWidth * editor.project.crop.w)
                / Math.max(1, editor.sourceHeight * editor.project.crop.h)
            readonly property real videoWidth: noBackground ? width
                : Math.min(width - 2 * pad, (height - 2 * pad) * sourceAspect)
            readonly property real videoHeight: noBackground ? height : videoWidth / sourceAspect
            readonly property real frameX: (width - videoWidth) / 2
            readonly property real frameY: (height - videoHeight) / 2
            readonly property real zoomOriginX: frameX + editor.zoom.cx * videoWidth
            readonly property real zoomOriginY: frameY + editor.zoom.cy * videoHeight
            function targetFromScene(px, py) {
                const scale = Math.max(.0001, editor.zoom.scale)
                const baseX = zoomOriginX + (px - zoomOriginX) / scale
                const baseY = zoomOriginY + (py - zoomOriginY) / scale
                return { x: Math.max(0, Math.min(1, (baseX - frameX) / videoWidth)),
                         y: Math.max(0, Math.min(1, (baseY - frameY) / videoHeight)) }
            }
            Composition {
                id: composition
                anchors.fill: parent
                Component.onCompleted: {
                    editor.attachFrameSource(frameSource)
                    editor.attachCameraFrameSource(cameraFrameSource)
                }
            }
            MouseArea {
                anchors.fill: parent
                enabled: editor.pickingZoomTarget
                cursorShape: Qt.CrossCursor
                onPressed: root.forceActiveFocus()
                onClicked: {
                    const point = scaled.targetFromScene(mouse.x, mouse.y)
                    editor.setZoomTargetFromPreview(point.x, point.y)
                }
            }
            Rectangle {
                id: manualTargetMarker
                visible: scaled.selectedZoom && typeof scaled.selectedZoom.target === "object"
                readonly property real targetX: scaled.selectedZoom ? scaled.selectedZoom.target.x : .5
                readonly property real targetY: scaled.selectedZoom ? scaled.selectedZoom.target.y : .5
                x: scaled.zoomOriginX + editor.zoom.scale * (scaled.frameX + targetX * scaled.videoWidth - scaled.zoomOriginX) - width / 2
                y: scaled.zoomOriginY + editor.zoom.scale * (scaled.frameY + targetY * scaled.videoHeight - scaled.zoomOriginY) - height / 2
                width: 18 / scaled.scale
                height: width
                radius: width / 2
                color: Qt.alpha(theme.accent, .22)
                border.width: 2 / scaled.scale
                border.color: theme.accent
                z: 20
                Rectangle { anchors.centerIn: parent; width: 4 / scaled.scale; height: width; radius: width / 2; color: theme.accent }
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
                        color: theme.accent; border.color: theme.accentForeground
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
        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: previewFrame.top
            anchors.topMargin: 14
            visible: editor.pickingZoomTarget
            color: Qt.alpha(theme.darkBackground, .88)
            radius: 7
            width: pickHint.implicitWidth + 24
            height: 34
            z: 40
            Label {
                id: pickHint
                anchors.centerIn: parent
                text: "Click where the zoom should center"
                color: theme.foreground
                font.pixelSize: 12
                font.weight: Font.DemiBold
            }
        }
        }
    }
    Label {
        anchors.right: previewArea.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 12
        text: editor.formatTime(editor.position) + "  /  " + editor.formatTime(editor.duration)
        color: Qt.alpha(theme.foreground, .58)
        font.family: "monospace"
        font.pixelSize: 11
    }
}
