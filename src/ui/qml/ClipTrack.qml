import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    height: 58
    function outputStart(index) {
        let value = 0
        for (let i=0;i<index;i++) value += (editor.clips[i].out-editor.clips[i].in)/editor.clips[i].speed
        return value
    }
    Repeater {
        model: editor.clips
        delegate: Rectangle {
            id: clipBlock
            required property var modelData
            required property int index
            x: root.outputStart(index) * root.pixelsPerSecond
            width: Math.max(20, (modelData.out-modelData.in)/modelData.speed * root.pixelsPerSecond)
            height: root.height
            radius: 5
            color: editor.selectedClipId === modelData.id ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .52) : Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .30)
            border.color: editor.selectedClipId === modelData.id ? theme.accent : Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .65)
            clip: true
            Repeater {
                model: editor.waveform
                delegate: Rectangle {
                    required property real modelData
                    required property int index
                    visible: editor.hasAudio
                    x: index * clipBlock.width / Math.max(1, editor.waveform.length)
                    width: Math.max(1, clipBlock.width / Math.max(1, editor.waveform.length) - 1)
                    height: Math.max(1, modelData * (clipBlock.height - 14))
                    y: (clipBlock.height - height) / 2
                    color: "#73ffffff"
                }
            }
            MouseArea {
                anchors.fill: parent
                onClicked: editor.selectedClipId = modelData.id
            }
            Label {
                anchors.centerIn: parent
                text: Number(modelData.speed).toFixed(modelData.speed % 1 ? 2 : 0) + "×"
                font.bold: true
                background: Rectangle { color: "#16171c"; radius: 4 }
                padding: 4
                MouseArea { anchors.fill: parent; onClicked: speedMenu.open() }
            }
            Menu {
                id: speedMenu
                Repeater {
                    model: [0.5,0.75,1,1.2,1.4,1.6,1.8,2,3,4,8,16,24]
                    delegate: MenuItem { required property real modelData; text: modelData + "×"; onTriggered: editor.setClipSpeed(clipBlock.modelData.id, modelData) }
                }
            }
            Rectangle {
                width: 8; height: parent.height; color: editor.selectedClipId === modelData.id ? theme.accent : "#9ca0aa"; radius: 3
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressX; property real originalIn
                    onPressed: { pressX = mapToItem(root, mouse.x, mouse.y).x; originalIn = modelData.in; editor.beginCoalescedEdit("trim-"+modelData.id) }
                    onPositionChanged: if (pressed) editor.trimClip(modelData.id, originalIn + (mapToItem(root, mouse.x, mouse.y).x-pressX) / root.pixelsPerSecond * modelData.speed, modelData.out)
                    onReleased: editor.endCoalescedEdit()
                }
            }
            Rectangle {
                anchors.right: parent.right; width: 8; height: parent.height; color: editor.selectedClipId === modelData.id ? theme.accent : "#9ca0aa"; radius: 3
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressX; property real originalOut
                    onPressed: { pressX = mapToItem(root, mouse.x, mouse.y).x; originalOut = modelData.out; editor.beginCoalescedEdit("trim-"+modelData.id) }
                    onPositionChanged: if (pressed) editor.trimClip(modelData.id, modelData.in, originalOut + (mapToItem(root, mouse.x, mouse.y).x-pressX) / root.pixelsPerSecond * modelData.speed)
                    onReleased: editor.endCoalescedEdit()
                }
            }
        }
    }
}
