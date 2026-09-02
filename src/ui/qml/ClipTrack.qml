import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    height: 44
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
            radius: 6
            color: Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .35)
            border.width: 1
            border.color: editor.selectedClipId === modelData.id || clipHover.hovered ? Qt.lighter(theme.accent, 1.22) : theme.accent
            clip: true
            HoverHandler { id: clipHover }
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
                width: Math.max(0, parent.width - 22)
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
                text: clipBlock.width < 68 ? "" : "Clip · " + Number(modelData.speed).toFixed(modelData.speed % 1 ? 2 : 0) + "×"
                font.weight: Font.DemiBold
                font.pixelSize: 11
                color: "white"
                padding: 3
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
                width: 6; height: parent.height; color: theme.accent; radius: 3
                opacity: editor.selectedClipId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    property real pressX; property real originalIn
                    onPressed: { pressX = mapToItem(root, mouse.x, mouse.y).x; originalIn = modelData.in; editor.beginCoalescedEdit("trim-"+modelData.id) }
                    onPositionChanged: if (pressed) editor.trimClip(modelData.id, originalIn + (mapToItem(root, mouse.x, mouse.y).x-pressX) / root.pixelsPerSecond * modelData.speed, modelData.out)
                    onReleased: editor.endCoalescedEdit()
                }
            }
            Rectangle {
                anchors.right: parent.right; width: 6; height: parent.height; color: theme.accent; radius: 3
                opacity: editor.selectedClipId === modelData.id ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
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
