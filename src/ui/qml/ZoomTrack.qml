import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    required property Item focusTarget
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        onPressed: root.focusTarget.forceActiveFocus()
        onDoubleClicked: editor.addZoomAt(mouse.x / root.pixelsPerSecond, 2)
    }
    Repeater {
        model: editor.zooms
        delegate: Rectangle {
            id: zoomBlock
            objectName: "zoomBlock-" + modelData.id
            required property var modelData
            required property int index
            property real outputStart: editor.sourceToOutput(modelData.start)
            property real outputEnd: editor.sourceToOutput(modelData.end)
            property bool gestureActive: false
            property real gestureX: 0
            property real gestureWidth: 0
            visible: outputStart >= 0 && outputEnd >= 0
            x: gestureActive ? gestureX : Math.max(0, outputStart) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth : Math.max(18, (outputEnd-outputStart) * root.pixelsPerSecond)
            height: root.height
            radius: 6
            color: Qt.alpha(theme.accent, .62)
            border.width: 1
            border.color: editor.selectedZoomId === modelData.id || zoomHover.hovered ? Qt.lighter(theme.accent, 1.25) : theme.accent
            HoverHandler { id: zoomHover }
            Label {
                anchors.centerIn: parent
                width: Math.max(0, parent.width - 20)
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                text: "Zoom " + Number(modelData.level).toFixed(1) + "× · " + (typeof modelData.target === "object" ? "Manual" : "Auto")
                color: theme.accentForeground
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }
            MouseArea {
                objectName: "zoomBody-" + modelData.id
                anchors.fill: parent
                anchors.leftMargin: 8; anchors.rightMargin: 8
                cursorShape: Qt.OpenHandCursor
                property real pressTrackX
                property real pressBlockX
                onPressed: {
                    root.focusTarget.forceActiveFocus()
                    editor.selectedZoomId = modelData.id
                    pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                    pressBlockX = zoomBlock.x
                    zoomBlock.gestureX = zoomBlock.x
                    zoomBlock.gestureWidth = zoomBlock.width
                    zoomBlock.gestureActive = true
                    editor.beginCoalescedEdit("move-" + modelData.id)
                }
                onPositionChanged: if (pressed) {
                    const trackX = mapToItem(root, mouse.x, mouse.y).x
                    zoomBlock.gestureX = Math.max(0, Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.gestureWidth,
                        pressBlockX + trackX - pressTrackX))
                }
                onReleased: {
                    const facade = editor
                    const start = editor.outputToSource(zoomBlock.gestureX / root.pixelsPerSecond)
                    zoomBlock.gestureActive = false
                    facade.moveZoom(modelData.id, start)
                    facade.endCoalescedEdit()
                }
                onCanceled: { zoomBlock.gestureActive = false; editor.endCoalescedEdit() }
            }
            Rectangle {
                width: 6; height: parent.height; radius: 3; color: Qt.lighter(theme.accent, 1.2)
                opacity: editor.selectedZoomId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressTrackX
                    property real pressBlockX
                    property real pressRight
                    onPressed: {
                        root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                        pressBlockX = zoomBlock.x; pressRight = zoomBlock.x + zoomBlock.width
                        zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                        zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                    }
                    onPositionChanged: if (pressed) {
                        const trackX = mapToItem(root, mouse.x, mouse.y).x
                        const nextX = Math.max(0, Math.min(pressRight - root.pixelsPerSecond,
                            pressBlockX + trackX - pressTrackX))
                        zoomBlock.gestureX = nextX; zoomBlock.gestureWidth = pressRight - nextX
                    }
                    onReleased: {
                        const facade = editor
                        const start = editor.outputToSource(zoomBlock.gestureX / root.pixelsPerSecond)
                        zoomBlock.gestureActive = false; facade.resizeZoom(modelData.id, start, modelData.end)
                        facade.endCoalescedEdit()
                    }
                    onCanceled: { zoomBlock.gestureActive = false; editor.endCoalescedEdit() }
                }
            }
            Rectangle {
                anchors.right: parent.right; width: 6; height: parent.height; radius: 3; color: Qt.lighter(theme.accent, 1.2)
                opacity: editor.selectedZoomId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressTrackX
                    property real pressWidth
                    onPressed: {
                        root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x; pressWidth = zoomBlock.width
                        zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                        zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                    }
                    onPositionChanged: if (pressed) {
                        const trackX = mapToItem(root, mouse.x, mouse.y).x
                        zoomBlock.gestureWidth = Math.max(root.pixelsPerSecond,
                            Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.gestureX,
                                pressWidth + trackX - pressTrackX))
                    }
                    onReleased: {
                        const facade = editor
                        const end = editor.outputToSource((zoomBlock.gestureX + zoomBlock.gestureWidth) / root.pixelsPerSecond)
                        zoomBlock.gestureActive = false; facade.resizeZoom(modelData.id, modelData.start, end)
                        facade.endCoalescedEdit()
                    }
                    onCanceled: { zoomBlock.gestureActive = false; editor.endCoalescedEdit() }
                }
            }
        }
    }
}
