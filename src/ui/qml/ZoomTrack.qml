import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    required property Item focusTarget
    MouseArea {
        id: selectionArea
        objectName: "zoomSelectionArea"
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        preventStealing: true
        property real originX: 0
        property real currentX: 0
        property bool selecting: false
        onPressed: mouse => {
            root.focusTarget.forceActiveFocus()
            originX = mouse.x
            currentX = mouse.x
            selecting = false
        }
        onPositionChanged: mouse => {
            if (!pressed) return
            currentX = Math.max(0, Math.min(width, mouse.x))
            selecting = Math.abs(currentX - originX) >= 2
        }
        onReleased: {
            if (selecting)
                editor.selectZoomsInOutputRange(Math.min(originX, currentX) / root.pixelsPerSecond,
                                                Math.max(originX, currentX) / root.pixelsPerSecond)
            else
                editor.selectedZoomIds = []
            selecting = false
        }
        onCanceled: selecting = false
        onDoubleClicked: editor.addZoomAt(mouse.x / root.pixelsPerSecond, 2)
    }
    Rectangle {
        visible: selectionArea.selecting
        x: Math.min(selectionArea.originX, selectionArea.currentX)
        width: Math.abs(selectionArea.currentX - selectionArea.originX)
        y: 1
        height: parent.height - 2
        radius: theme.radius
        color: Qt.alpha(theme.accent, .18)
        border.width: 1
        border.color: theme.accent
        z: 100
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
            readonly property bool selected: editor.selectedZoomIds.indexOf(modelData.id) >= 0
            visible: outputStart >= 0 && outputEnd >= 0
            x: gestureActive ? gestureX : Math.max(0, outputStart) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth : Math.max(18, (outputEnd-outputStart) * root.pixelsPerSecond)
            height: root.height
            radius: theme.radius
            color: Qt.alpha(theme.accent, .18)
            border.width: 1
            border.color: selected ? theme.accent
                        : zoomHover.hovered ? Qt.alpha(theme.accent, .80)
                        : Qt.alpha(theme.accent, .60)
            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
            Behavior on border.color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
            HoverHandler { id: zoomHover }
            Label {
                anchors.centerIn: parent
                width: Math.max(0, parent.width - 20)
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                text: "Zoom " + Number(modelData.level).toFixed(1) + "× · " + (typeof modelData.target === "object" ? "Manual" : "Auto")
                color: theme.foreground
                font.pixelSize: theme.font.body
                font.weight: Font.Medium
            }
            MouseArea {
                objectName: "zoomBody-" + modelData.id
                anchors.fill: parent
                anchors.leftMargin: 8; anchors.rightMargin: 8
                cursorShape: Qt.OpenHandCursor
                property real pressTrackX
                property real pressBlockX
                onPressed: mouse => {
                    root.focusTarget.forceActiveFocus()
                    if (editor.selectedZoomIds.indexOf(modelData.id) < 0)
                        editor.selectedZoomId = modelData.id
                    pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                    pressBlockX = zoomBlock.x
                    editor.beginCoalescedEdit("move-selection")
                }
                onPositionChanged: mouse => { if (pressed) {
                    const trackX = mapToItem(root, mouse.x, mouse.y).x
                    const nextX = Math.max(0, Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.width,
                        pressBlockX + trackX - pressTrackX))
                    editor.moveSelectedZooms(modelData.id, editor.outputToSource(nextX / root.pixelsPerSecond))
                } }
                onReleased: {
                    editor.endCoalescedEdit()
                }
                onCanceled: editor.endCoalescedEdit()
            }
            Rectangle {
                width: 8; height: parent.height; radius: theme.radius; color: zoomBlock.border.color
                opacity: zoomBlock.selected || zoomHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 120 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressTrackX
                    property real pressBlockX
                    property real pressRight
                    onPressed: mouse => {
                        root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                        pressBlockX = zoomBlock.x; pressRight = zoomBlock.x + zoomBlock.width
                        zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                        zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                    }
                    onPositionChanged: mouse => { if (pressed) {
                        const trackX = mapToItem(root, mouse.x, mouse.y).x
                        const nextX = Math.max(0, Math.min(pressRight - root.pixelsPerSecond,
                            pressBlockX + trackX - pressTrackX))
                        zoomBlock.gestureX = nextX; zoomBlock.gestureWidth = pressRight - nextX
                    } }
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
                anchors.right: parent.right; width: 8; height: parent.height; radius: theme.radius; color: zoomBlock.border.color
                opacity: zoomBlock.selected || zoomHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 120 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressTrackX
                    property real pressWidth
                    onPressed: mouse => {
                        root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x; pressWidth = zoomBlock.width
                        zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                        zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                    }
                    onPositionChanged: mouse => { if (pressed) {
                        const trackX = mapToItem(root, mouse.x, mouse.y).x
                        zoomBlock.gestureWidth = Math.max(root.pixelsPerSecond,
                            Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.gestureX,
                                pressWidth + trackX - pressTrackX))
                    } }
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
