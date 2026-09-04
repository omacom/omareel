import QtQuick
import QtQuick.Controls
import QtQuick.Effects

FocusScope {
    id: root
    required property bool cropMode
    function grabPreview(callback) {
        previewFrame.grabToImage(callback, Qt.size(640, 360))
    }
    function formatTenths(seconds) {
        const tenths = Math.max(0, Math.round(seconds * 10))
        const minutes = Math.floor(tenths / 600)
        const remainder = tenths % 600
        const whole = Math.floor(remainder / 10)
        return String(minutes).padStart(2, "0") + ":"
             + String(whole).padStart(2, "0") + "." + (remainder % 10)
    }
    clip: true
    Rectangle { anchors.fill: parent; color: theme.surface }
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
        anchors.bottomMargin: 44

        Rectangle {
            id: shadowShape
            anchors.fill: previewFrame
            radius: theme.radius
            color: theme.surfaceRaised
            visible: false
            layer.enabled: true
        }
        MultiEffect {
            anchors.fill: previewFrame
            source: shadowShape
            shadowEnabled: true
            shadowColor: theme.dark ? Qt.alpha(theme.surfaceRaised, .65)
                                    : Qt.alpha(theme.foreground, .16)
            shadowOpacity: 1
            shadowBlur: 1
            blurMax: 6
            shadowVerticalOffset: 2
            autoPaddingEnabled: true
        }
        Rectangle {
        id: previewFrame
        anchors.centerIn: parent
        height: Math.min(parent.height, 620,
                         parent.width * editor.outputHeight / Math.max(1, editor.outputWidth))
        width: height * editor.outputWidth / Math.max(1, editor.outputHeight)
        color: theme.normalFill
        radius: theme.radius
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
                nativePreview: true
                Component.onCompleted: {
                    editor.attachVideoOutput(videoOutput)
                    editor.attachCameraVideoOutput(cameraVideoOutput)
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
                readonly property real targetX: visible ? Number(scaled.selectedZoom.target.x) : .5
                readonly property real targetY: visible ? Number(scaled.selectedZoom.target.y) : .5
                x: scaled.zoomOriginX + editor.zoom.scale * (scaled.frameX + targetX * scaled.videoWidth - scaled.zoomOriginX) - width / 2
                y: scaled.zoomOriginY + editor.zoom.scale * (scaled.frameY + targetY * scaled.videoHeight - scaled.zoomOriginY) - height / 2
                width: 18 / scaled.scale
                height: width
                radius: theme.radius
                color: theme.accentSoft
                border.width: 2 / scaled.scale
                border.color: theme.accent
                z: 20
                Rectangle { anchors.centerIn: parent; width: 4 / scaled.scale; height: width; radius: theme.radius; color: theme.accent }
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
                        width: cropRect.handleSize; height: width; radius: theme.radius
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
            anchors.fill: parent
            color: "transparent"
            border.color: theme.hairlineStrong
            border.width: 1
            radius: theme.radius
            z: 30
        }
        Rectangle {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: previewFrame.top
            anchors.topMargin: 14
            visible: editor.pickingZoomTarget
            color: theme.popupBackground
            border.width: 2
            border.color: theme.popupBorder
            radius: theme.radius
            width: pickHint.implicitWidth + 20
            height: 28
            z: 40
            Label {
                id: pickHint
                anchors.centerIn: parent
                text: "Click where the zoom should center"
                color: theme.foreground
                font.pixelSize: theme.font.body
                font.weight: Font.Medium
            }
        }
        }
        Rectangle {
            anchors.right: previewFrame.right
            anchors.top: previewFrame.bottom
            anchors.topMargin: 8
            width: timeLabel.implicitWidth + 16
            height: 26
            radius: 13
            color: theme.normalFill
            border.width: 1
            border.color: theme.hairline
            Label {
                id: timeLabel
                anchors.centerIn: parent
                text: root.formatTenths(editor.position) + " / " + root.formatTenths(editor.duration)
                color: theme.textMuted
                font.family: theme.monoFamily
                font.pixelSize: theme.font.caption
            }
        }
    }
}
