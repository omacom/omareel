import QtQuick
import QtQuick.Controls
import QtQuick.Effects
import Omareel.Ui

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
    Rectangle { anchors.fill: parent; color: theme.canvas }
    MouseArea {
        anchors.fill: parent
        onPressed: { root.forceActiveFocus(); editor.selectedOverlayId = "" }
    }

    Item {
        id: previewArea
        anchors.fill: parent
        anchors.leftMargin: 32
        anchors.rightMargin: 32
        anchors.topMargin: 28
        anchors.bottomMargin: 28

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
            shadowColor: theme.shadow
            shadowOpacity: 1
            shadowBlur: 1
            blurMax: 40
            shadowVerticalOffset: 14
            shadowScale: .97
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
            // Zoom targets are normalised to the whole recording, like the cursor and
            // input.jsonl, with or without a crop or reframing; the frame shows the part of it
            // in composition.viewCrop, scaled by composition.zoomScale about zoomOrigin.
            readonly property real frameX: (width - composition.videoWidth) / 2
            readonly property real frameY: (height - composition.videoHeight) / 2
            readonly property real zoomOriginX: frameX + composition.zoomOrigin.x * composition.videoWidth
            readonly property real zoomOriginY: frameY + composition.zoomOrigin.y * composition.videoHeight
            function targetFromScene(px, py) {
                const scale = Math.max(.0001, composition.zoomScale)
                const c = composition.viewCrop
                const u = Math.max(0, Math.min(1, (zoomOriginX + (px - zoomOriginX) / scale - frameX) / composition.videoWidth))
                const v = Math.max(0, Math.min(1, (zoomOriginY + (py - zoomOriginY) / scale - frameY) / composition.videoHeight))
                return { x: c.x + u * c.w, y: c.y + v * c.h }
            }
            function sceneFromTarget(x, y) {
                const c = composition.viewCrop
                const fx = frameX + (x - c.x) / Math.max(.0001, c.w) * composition.videoWidth
                const fy = frameY + (y - c.y) / Math.max(.0001, c.h) * composition.videoHeight
                return Qt.point(zoomOriginX + composition.zoomScale * (fx - zoomOriginX),
                                zoomOriginY + composition.zoomScale * (fy - zoomOriginY))
            }
            Composition {
                id: composition
                objectName: "previewComposition"
                anchors.fill: parent
                nativePreview: true
                Component.onCompleted: {
                    editor.attachVideoOutput(videoOutput)
                    editor.attachCameraVideoOutput(cameraVideoOutput)
                    editor.attachMediaVideoOutput(mediaVideoOutput)
                }
            }
            OverlayEditLayer {
                objectName: "overlayEditLayer"
                anchors.fill: parent
                composition: composition
                viewScale: scaled.scale
                focusTarget: root
                visible: !editor.pickingZoomTarget && !root.cropMode
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
                objectName: "zoomTargetMarker"
                visible: scaled.selectedZoom && typeof scaled.selectedZoom.target === "object"
                readonly property real targetX: visible ? Number(scaled.selectedZoom.target.x) : .5
                readonly property real targetY: visible ? Number(scaled.selectedZoom.target.y) : .5
                readonly property point scenePoint: scaled.sceneFromTarget(targetX, targetY)
                x: scenePoint.x - width / 2
                y: scenePoint.y - height / 2
                width: 18 / scaled.scale
                height: width
                radius: width / 2
                color: theme.accentSoft
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
                        id: cropHandle
                        required property var modelData
                        width: cropRect.handleSize; height: width; radius: theme.smallRadius / scaled.scale
                        x: modelData.x * cropRect.width - width / 2
                        y: modelData.y * cropRect.height - height / 2
                        color: theme.accent
                        border.width: 1 / scaled.scale
                        border.color: theme.accentForeground
                        scale: cropDrag.containsMouse || cropDrag.pressed ? 1.15 : 1
                        transformOrigin: Item.Center
                        Behavior on scale {
                            enabled: !editor.loading
                            NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                        }
                        Rectangle {
                            anchors.fill: parent
                            anchors.margins: -6 / scaled.scale
                            radius: Math.max(cropHandle.radius, height / 2)
                            color: Qt.alpha(theme.accent, .30)
                            opacity: cropDrag.pressed ? 1 : 0
                            z: -1
                            Behavior on opacity {
                                enabled: !editor.loading
                                NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                            }
                        }
                        MouseArea {
                            id: cropDrag
                            anchors.fill: parent
                            drag.target: parent
                            hoverEnabled: true
                            cursorShape: modelData.corner === 0 || modelData.corner === 3
                                ? Qt.SizeBDiagCursor : Qt.SizeFDiagCursor
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
                            onCanceled: editor.endCoalescedEdit()
                        }
                    }
                }
            }
        }
        Rectangle {
            anchors.fill: parent
            color: "transparent"
            border.color: Qt.alpha("white", theme.dark ? .06 : 0)
            border.width: 1
            radius: theme.radius
            z: 30
        }
        Item {
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: previewFrame.top
            anchors.topMargin: 14
            width: pickHint.implicitWidth + 36
            height: 32
            z: 40
            opacity: editor.pickingZoomTarget ? 1 : 0
            visible: opacity > 0
            scale: editor.pickingZoomTarget ? 1 : .94
            Behavior on opacity { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
            Behavior on scale { NumberAnimation { duration: 220; easing.type: Easing.OutBack } }
            OmPopupCard { anchors.fill: parent; padding: 0; radius: theme.controlRadius }
            Row {
                anchors.centerIn: parent
                spacing: 8
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 8; height: 8; radius: 4
                    color: theme.accent
                    SequentialAnimation on opacity {
                        running: editor.pickingZoomTarget
                        loops: Animation.Infinite
                        NumberAnimation { from: 1; to: .3; duration: 650; easing.type: Easing.InOutSine }
                        NumberAnimation { from: .3; to: 1; duration: 650; easing.type: Easing.InOutSine }
                    }
                }
                Label {
                    id: pickHint
                    anchors.verticalCenter: parent.verticalCenter
                    text: "Click where the zoom should center"
                    color: theme.textStrong
                    font.pixelSize: theme.type.body
                    font.weight: Font.Medium
                }
            }
        }
        Item {
            id: importNotice
            objectName: "importNotice"
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.top: previewFrame.top
            anchors.topMargin: 14
            visible: editor.importing || editor.importError.length > 0
            width: Math.min(previewFrame.width - 28, importRow.implicitWidth + 32)
            height: 32
            z: 40
            OmPopupCard { anchors.fill: parent; padding: 0; radius: theme.controlRadius }
            Row {
                id: importRow
                anchors.centerIn: parent
                width: Math.min(implicitWidth, importNotice.width - 32)
                spacing: 8
                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 8; height: 8; radius: 4
                    color: editor.importError.length > 0 ? theme.record : theme.accent
                    SequentialAnimation on opacity {
                        running: editor.importing
                        loops: Animation.Infinite
                        NumberAnimation { from: 1; to: .3; duration: 650; easing.type: Easing.InOutSine }
                        NumberAnimation { from: .3; to: 1; duration: 650; easing.type: Easing.InOutSine }
                    }
                }
                Label {
                    width: Math.min(implicitWidth, importNotice.width - 32 - 16
                        - (dismissImport.visible ? dismissImport.width + importRow.spacing : 0))
                    anchors.verticalCenter: parent.verticalCenter
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                    text: editor.importing ? "Adding clip…" : editor.importError
                    color: theme.textStrong
                    font.pixelSize: theme.type.body
                    font.weight: Font.Medium
                }
                IconToolButton {
                    id: dismissImport
                    visible: !editor.importing
                    anchors.verticalCenter: parent.verticalCenter
                    width: 20; height: 20
                    icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
                    icon.width: 14; icon.height: 14
                    Accessible.name: "Dismiss"
                    onClicked: editor.clearImportError()
                }
            }
        }
        }
    }
}
