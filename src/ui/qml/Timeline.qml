import QtQuick
import QtQuick.Controls

FocusScope {
    id: root
    required property real scaleFactor
    property real labelWidth: 76
    property real basePixels: Math.max(55, (width-labelWidth-24) / Math.max(1, editor.duration))
    property real pixelsPerSecond: basePixels * scaleFactor
    property int ticksPerSecond: pixelsPerSecond >= 120 ? 5 : pixelsPerSecond >= 72 ? 2 : 1

    Rectangle { anchors.fill: parent; color: theme.background; border.color: Qt.alpha(theme.foreground, .06) }

    Column {
        x: 0; y: 0; width: labelWidth; height: parent.height
        Item { width: parent.width; height: 40 }
        Label { width: parent.width; height: 56; leftPadding: 14; verticalAlignment: Text.AlignVCenter; text: "Clip"; color: Qt.alpha(theme.foreground, .58); font.pixelSize: 11 }
        Label { width: parent.width; height: 56; leftPadding: 14; verticalAlignment: Text.AlignVCenter; text: "Zoom"; color: Qt.alpha(theme.foreground, .58); font.pixelSize: 11 }
    }
    Flickable {
        id: flick
        x: root.labelWidth; y: 0
        width: parent.width - x; height: parent.height
        contentWidth: Math.max(width, editor.duration * root.pixelsPerSecond + 40)
        contentHeight: height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        readonly property int firstVisibleTick: Math.max(0, Math.floor(contentX * root.ticksPerSecond / root.pixelsPerSecond) - 1)
        readonly property int visibleTickCount: Math.ceil(width * root.ticksPerSecond / root.pixelsPerSecond) + 4
        Item {
            id: content
            width: flick.contentWidth; height: flick.height
            MouseArea { anchors.fill: parent; onPressed: root.forceActiveFocus() }
            Item {
                id: ruler
                width: parent.width; height: 40
                Repeater {
                    model: flick.visibleTickCount
                    delegate: Item {
                        id: tickDelegate
                        required property int index
                        property int tickIndex: flick.firstVisibleTick + index
                        property real tickTime: tickIndex / root.ticksPerSecond
                        property bool major: tickIndex % root.ticksPerSecond === 0
                        visible: tickTime <= editor.duration
                        x: tickTime * root.pixelsPerSecond
                        width: 1; height: ruler.height
                        Rectangle { anchors.bottom: parent.bottom; width: 1; height: tickDelegate.major ? 11 : 5; color: Qt.alpha(theme.foreground, tickDelegate.major ? .28 : .14) }
                        Loader {
                            active: tickDelegate.major && Math.round(tickDelegate.tickTime) % Math.max(1, Math.ceil(62/root.pixelsPerSecond)) === 0
                            x: 5; y: 6
                            sourceComponent: Text { text: editor.formatTime(tickDelegate.tickTime); color: Qt.alpha(theme.foreground, .46); font.pixelSize: 10; font.family: "monospace" }
                        }
                    }
                }
                MouseArea { anchors.fill: parent; onPressed: root.forceActiveFocus(); onClicked: editor.seek(mouse.x / root.pixelsPerSecond) }
            }
            Rectangle { x: 0; y: 40; width: parent.width; height: 56; color: Qt.lighter(theme.background, 1.08); border.color: Qt.alpha(theme.foreground, .05) }
            ClipTrack { x: 0; y: 46; width: parent.width; height: 44; pixelsPerSecond: root.pixelsPerSecond; focusTarget: root }
            Rectangle { x: 0; y: 96; width: parent.width; height: 56; color: Qt.lighter(theme.background, 1.04); border.color: Qt.alpha(theme.foreground, .05) }
            ZoomTrack { x: 0; y: 102; width: parent.width; height: 44; pixelsPerSecond: root.pixelsPerSecond; focusTarget: root }
            Rectangle {
                id: playhead
                x: editor.position * root.pixelsPerSecond - 1
                y: 35; width: 2; height: 117
                color: theme.accent
                z: 20
                Rectangle { x: -4; y: -2; width: 10; height: 10; radius: 5; color: parent.color }
                MouseArea {
                    x: -8; width: 18; y: -8; height: parent.height + 16
                    cursorShape: Qt.SizeHorCursor
                    onPressed: root.forceActiveFocus()
                    onPositionChanged: if (pressed) editor.seek(Math.max(0, Math.min(editor.duration, mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond)))
                }
            }
        }
        ScrollBar.horizontal: ScrollBar { }
    }
}
