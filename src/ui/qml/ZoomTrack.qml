import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import Omareel.Ui

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
        z: 0
        property real originX: 0
        property real currentX: 0
        property bool selecting: false
        property bool addedZoom: false
        onPressed: mouse => {
            editor.traceInput(objectName, "press", mouse.x, mouse.y)
            root.focusTarget.forceActiveFocus()
            originX = mouse.x
            currentX = mouse.x
            selecting = false
            addedZoom = false
        }
        onPositionChanged: mouse => {
            if (!pressed) return
            editor.traceInput(objectName, "move", mouse.x, mouse.y)
            currentX = Math.max(0, Math.min(width, mouse.x))
            selecting = Math.abs(currentX - originX) >= 2
        }
        onReleased: mouse => {
            editor.traceInput(objectName, "release", mouse.x, mouse.y)
            currentX = Math.max(0, Math.min(width, mouse.x))
            selecting = Math.abs(currentX - originX) >= 2
            if (selecting)
                editor.selectZoomsInOutputRange(Math.min(originX, currentX) / root.pixelsPerSecond,
                                                Math.max(originX, currentX) / root.pixelsPerSecond)
            else if (!addedZoom) // the release ending a double-click keeps the new zoom selected
                editor.selectedZoomIds = []
            selecting = false
        }
        onCanceled: selecting = false
        onDoubleClicked: mouse => {
            addedZoom = editor.addZoomAt(mouse.x / root.pixelsPerSecond, 2).length > 0
        }
    }
    Rectangle {
        visible: selectionArea.selecting
        x: Math.min(selectionArea.originX, selectionArea.currentX)
        width: Math.abs(selectionArea.currentX - selectionArea.originX)
        y: 1
        height: parent.height - 2
        radius: theme.controlRadius
        color: Qt.alpha(theme.zoomAccent, .14)
        border.width: 1
        border.color: Qt.alpha(theme.zoomAccent, .8)
        z: 100
    }
    Repeater {
        model: editor.zooms
        delegate: Rectangle {
            id: zoomBlock
            objectName: "zoomBlock-" + modelData.id
            required property var modelData
            required property int index
            // A zoom whose start or end lies in cut footage still plays over what survives, so
            // the block covers that part; only a zoom cut away entirely has nothing to show.
            readonly property point outputRange: editor.sourceRangeToOutput(modelData.start, modelData.end)
            property real outputStart: outputRange.x
            property real outputEnd: outputRange.y
            property bool gestureActive: false
            property real gestureX: 0
            property real gestureWidth: 0
            readonly property bool selected: editor.selectedZoomIds.indexOf(modelData.id) >= 0
            z: 1
            visible: outputStart >= 0 && outputEnd > outputStart
            x: gestureActive ? gestureX : Math.max(0, outputStart) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth : Math.max(18, (outputEnd-outputStart) * root.pixelsPerSecond)
            height: root.height
            y: 0
            radius: theme.controlRadius
            color: Qt.alpha(theme.zoomAccent, selected ? .30 : zoomHover.hovered ? .24 : .18)
            border.width: selected ? 1.5 : 1
            border.color: selected ? theme.zoomAccent
                        : Qt.alpha(theme.zoomAccent, zoomHover.hovered ? .7 : .42)
            Behavior on color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            Behavior on border.color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            HoverHandler { id: zoomHover }
            RowLayout {
                id: zoomLabel
                anchors.centerIn: parent
                width: Math.max(0, parent.width - 36)
                visible: zoomBlock.width >= 72
                opacity: leftTrim.engaged || rightTrim.engaged ? .60 : 1
                spacing: 5
                Behavior on opacity {
                    enabled: !editor.loading
                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
                Item {
                    Layout.preferredWidth: 13; Layout.preferredHeight: 13
                    IconImage {
                        anchors.fill: parent
                        source: "qrc:/omareel/assets/icons/lucide/zoom-in.svg"
                        sourceSize: Qt.size(13, 13)
                        color: theme.zoomAccent
                    }
                }
                Label {
                    textFormat: Text.PlainText
                    text: Number(modelData.level).toFixed(1) + "×"
                    color: theme.textStrong
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.caption
                    font.weight: Font.Medium
                }
                Label {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignLeft
                    textFormat: Text.PlainText
                    text: typeof modelData.target === "object" ? "Manual" : "Auto"
                    color: theme.textMuted
                    font.pixelSize: theme.type.small
                }
            }
            MouseArea {
                objectName: "zoomBody-" + modelData.id
                anchors.fill: parent
                anchors.leftMargin: 14; anchors.rightMargin: 14
                cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                property real pressTrackX
                property real pressBlockX
                onPressed: mouse => {
                    editor.traceInput(objectName, "press", mouse.x, mouse.y)
                    root.focusTarget.forceActiveFocus()
                    if (editor.selectedZoomIds.indexOf(modelData.id) < 0)
                        editor.selectedZoomId = modelData.id
                    pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                    pressBlockX = zoomBlock.x
                    zoomBlock.gestureX = zoomBlock.x
                    zoomBlock.gestureWidth = zoomBlock.width
                    zoomBlock.gestureActive = true
                    editor.beginCoalescedEdit("move-selection")
                }
                onPositionChanged: mouse => { if (pressed) {
                    editor.traceInput(objectName, "move", mouse.x, mouse.y)
                    const trackX = mapToItem(root, mouse.x, mouse.y).x
                    const nextX = Math.max(0, Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.width,
                        pressBlockX + trackX - pressTrackX))
                    zoomBlock.gestureX = nextX
                } }
                onReleased: mouse => {
                    const facade = editor
                    facade.traceInput(objectName, "release", mouse.x, mouse.y)
                    const nextStart = facade.outputToSource(
                        zoomBlock.gestureX / root.pixelsPerSecond)
                    zoomBlock.gestureActive = false
                    facade.moveSelectedZooms(modelData.id, nextStart)
                    facade.endCoalescedEdit()
                }
                onCanceled: {
                    editor.traceInput(objectName, "cancel", 0, 0)
                    zoomBlock.gestureActive = false
                    editor.endCoalescedEdit()
                }
            }
            OmTrimHandle {
                id: leftTrim
                objectName: "zoomTrimHandle-" + modelData.id + "-left"
                edge: "left"
                accent: theme.zoomAccent
                active: zoomBlock.selected || zoomHover.hovered
                dragging: mouseArea.pressed
                property real pressTrackX
                property real pressBlockX
                property real pressRight
                mouseArea.onPressed: mouse => {
                    editor.traceInput(mouseArea.objectName, "press", mouse.x, mouse.y)
                    root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                    leftTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    leftTrim.pressBlockX = zoomBlock.x
                    leftTrim.pressRight = zoomBlock.x + zoomBlock.width
                    zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                    zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    editor.traceInput(mouseArea.objectName, "move", mouse.x, mouse.y)
                    const trackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    const nextX = Math.max(0, Math.min(leftTrim.pressRight - root.pixelsPerSecond,
                        leftTrim.pressBlockX + trackX - leftTrim.pressTrackX))
                    zoomBlock.gestureX = nextX; zoomBlock.gestureWidth = leftTrim.pressRight - nextX
                } }
                mouseArea.onReleased: {
                    editor.traceInput(mouseArea.objectName, "release", 0, 0)
                    const facade = editor
                    const start = editor.outputToSource(zoomBlock.gestureX / root.pixelsPerSecond)
                    zoomBlock.gestureActive = false; facade.resizeZoom(modelData.id, start, modelData.end)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    editor.traceInput(mouseArea.objectName, "cancel", 0, 0)
                    zoomBlock.gestureActive = false
                    editor.endCoalescedEdit()
                }
            }
            OmTrimHandle {
                id: rightTrim
                objectName: "zoomTrimHandle-" + modelData.id + "-right"
                x: parent.width - width
                edge: "right"
                accent: theme.zoomAccent
                active: zoomBlock.selected || zoomHover.hovered
                dragging: mouseArea.pressed
                property real pressTrackX
                property real pressWidth
                mouseArea.onPressed: mouse => {
                    editor.traceInput(mouseArea.objectName, "press", mouse.x, mouse.y)
                    root.focusTarget.forceActiveFocus(); editor.selectedZoomId = modelData.id
                    rightTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    rightTrim.pressWidth = zoomBlock.width
                    zoomBlock.gestureX = zoomBlock.x; zoomBlock.gestureWidth = zoomBlock.width
                    zoomBlock.gestureActive = true; editor.beginCoalescedEdit("resize-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    editor.traceInput(mouseArea.objectName, "move", mouse.x, mouse.y)
                    const trackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    zoomBlock.gestureWidth = Math.max(root.pixelsPerSecond,
                        Math.min(editor.duration * root.pixelsPerSecond - zoomBlock.gestureX,
                            rightTrim.pressWidth + trackX - rightTrim.pressTrackX))
                } }
                mouseArea.onReleased: {
                    editor.traceInput(mouseArea.objectName, "release", 0, 0)
                    const facade = editor
                    const end = editor.outputToSource((zoomBlock.gestureX + zoomBlock.gestureWidth)
                                                       / root.pixelsPerSecond)
                    zoomBlock.gestureActive = false; facade.resizeZoom(modelData.id, modelData.start, end)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    editor.traceInput(mouseArea.objectName, "cancel", 0, 0)
                    zoomBlock.gestureActive = false
                    editor.endCoalescedEdit()
                }
            }
        }
    }
}
