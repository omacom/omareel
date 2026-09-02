import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    height: 44
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton
        onDoubleClicked: editor.addZoomAt(mouse.x / root.pixelsPerSecond, 2)
    }
    Repeater {
        model: editor.zooms
        delegate: Rectangle {
            id: zoomBlock
            required property var modelData
            property real outputStart: editor.sourceToOutput(modelData.start)
            property real outputEnd: editor.sourceToOutput(modelData.end)
            visible: outputStart >= 0 && outputEnd >= 0
            x: Math.max(0, outputStart) * root.pixelsPerSecond
            width: Math.max(18, (outputEnd-outputStart) * root.pixelsPerSecond)
            height: root.height
            radius: 6
            color: "#4f5bd5"
            border.width: 1
            border.color: editor.selectedZoomId === modelData.id || zoomHover.hovered ? "#b9beff" : "#7882ed"
            HoverHandler { id: zoomHover }
            Label {
                anchors.centerIn: parent
                width: Math.max(0, parent.width - 20)
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                text: "Zoom " + Number(modelData.level).toFixed(1) + "× · " + (typeof modelData.target === "object" ? "Manual" : "Auto")
                color: "white"
                font.pixelSize: 11
                font.weight: Font.DemiBold
            }
            MouseArea {
                anchors.fill: parent
                anchors.leftMargin: 8; anchors.rightMargin: 8
                cursorShape: Qt.OpenHandCursor
                property real pressX; property real originalOutput
                onPressed: { editor.selectedZoomId = modelData.id; pressX = mapToItem(root, mouse.x, mouse.y).x; originalOutput = zoomBlock.outputStart; editor.beginCoalescedEdit("move-"+modelData.id) }
                onPositionChanged: if (pressed) editor.moveZoom(modelData.id, editor.outputToSource(Math.max(0, originalOutput + (mapToItem(root, mouse.x, mouse.y).x-pressX)/root.pixelsPerSecond)))
                onReleased: editor.endCoalescedEdit()
            }
            Rectangle {
                width: 6; height: parent.height; radius: 3; color: "#aeb5ff"
                opacity: editor.selectedZoomId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressX; property real originalOutput
                    onPressed: { editor.selectedZoomId = modelData.id; pressX=mapToItem(root,mouse.x,mouse.y).x; originalOutput=zoomBlock.outputStart; editor.beginCoalescedEdit("resize-"+modelData.id) }
                    onPositionChanged: if (pressed) editor.resizeZoom(modelData.id, editor.outputToSource(Math.max(0, originalOutput+(mapToItem(root,mouse.x,mouse.y).x-pressX)/root.pixelsPerSecond)), modelData.end)
                    onReleased: editor.endCoalescedEdit()
                }
            }
            Rectangle {
                anchors.right: parent.right; width: 6; height: parent.height; radius: 3; color: "#aeb5ff"
                opacity: editor.selectedZoomId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressX; property real originalOutput
                    onPressed: { editor.selectedZoomId = modelData.id; pressX=mapToItem(root,mouse.x,mouse.y).x; originalOutput=zoomBlock.outputEnd; editor.beginCoalescedEdit("resize-"+modelData.id) }
                    onPositionChanged: if (pressed) editor.resizeZoom(modelData.id, modelData.start, editor.outputToSource(Math.max(0, originalOutput+(mapToItem(root,mouse.x,mouse.y).x-pressX)/root.pixelsPerSecond)))
                    onReleased: editor.endCoalescedEdit()
                }
            }
        }
    }
}
