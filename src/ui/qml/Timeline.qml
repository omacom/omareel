import QtQuick
import QtQuick.Controls

Rectangle {
    id: root
    required property real scaleFactor
    color: "#15161b"
    border.color: "#292b32"
    property real labelWidth: 82
    property real basePixels: Math.max(55, (width-labelWidth-24) / Math.max(1, editor.duration))
    property real pixelsPerSecond: basePixels * scaleFactor

    Column {
        x: 0; y: 0; width: labelWidth; height: parent.height
        Item { width: parent.width; height: 40 }
        Label { width: parent.width; height: 70; leftPadding: 16; verticalAlignment: Text.AlignVCenter; text: "Clip"; color: "#a8aab3" }
        Label { width: parent.width; height: 58; leftPadding: 16; verticalAlignment: Text.AlignVCenter; text: "Zoom"; color: "#a8aab3" }
    }
    Flickable {
        id: flick
        x: root.labelWidth; y: 0
        width: parent.width - x; height: parent.height
        contentWidth: Math.max(width, editor.duration * root.pixelsPerSecond + 40)
        contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        Item {
            id: content
            width: flick.contentWidth; height: flick.height
            Item {
                id: ruler
                width: parent.width; height: 40
                Repeater {
                    model: Math.ceil(editor.duration) + 1
                    delegate: Item {
                        required property int index
                        x: index * root.pixelsPerSecond
                        width: 1; height: ruler.height
                        Rectangle { width: 1; height: index % 5 === 0 ? 12 : 6; color: "#555864" }
                        Label { visible: index % Math.max(1, Math.ceil(65/root.pixelsPerSecond)) === 0; x: 5; y: 2; text: editor.formatTime(index); color: "#727581"; font.pixelSize: 10 }
                    }
                }
                MouseArea { anchors.fill: parent; onClicked: editor.seek(mouse.x / root.pixelsPerSecond) }
            }
            Rectangle { x: 0; y: 40; width: parent.width; height: 70; color: "#1b1c22" }
            ClipTrack { x: 0; y: 46; width: parent.width; pixelsPerSecond: root.pixelsPerSecond }
            Rectangle { x: 0; y: 110; width: parent.width; height: 58; color: "#181a20" }
            ZoomTrack { x: 0; y: 117; width: parent.width; pixelsPerSecond: root.pixelsPerSecond }
            Rectangle {
                id: playhead
                x: editor.position * root.pixelsPerSecond - 1
                y: 30; width: 2; height: 145
                color: "#ff5e72"
                z: 20
                Rectangle { x: -5; y: -2; width: 12; height: 12; radius: 6; color: parent.color }
                MouseArea {
                    x: -8; width: 18; y: -8; height: parent.height + 16
                    cursorShape: Qt.SizeHorCursor
                    onPositionChanged: if (pressed) editor.seek(Math.max(0, Math.min(editor.duration, mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond)))
                }
            }
        }
        ScrollBar.horizontal: ScrollBar { }
    }
}
