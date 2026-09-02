import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property real pixelsPerSecond
    required property Item focusTarget
    function outputStart(index) {
        let value = 0
        for (let i=0;i<index;i++) value += (editor.clips[i].out-editor.clips[i].in)/editor.clips[i].speed
        return value
    }
    Repeater {
        model: editor.clips
        delegate: Rectangle {
            id: clipBlock
            objectName: "clipBlock-" + modelData.id
            required property var modelData
            required property int index
            readonly property bool selected: editor.selectedClipId === modelData.id
            property bool gestureActive: false
            property real gestureX: 0
            property real gestureWidth: 0
            x: root.outputStart(index) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth : Math.max(20, (modelData.out-modelData.in)/modelData.speed * root.pixelsPerSecond)
            height: root.height
            radius: 6
            color: Qt.alpha(theme.accent, .35)
            border.width: 1
            border.color: clipBlock.selected || clipHover.hovered ? Qt.lighter(theme.accent, 1.22) : theme.accent
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
                    color: Qt.alpha(theme.foreground, .45)
                }
            }
            MouseArea {
                anchors.fill: parent
                onPressed: root.focusTarget.forceActiveFocus()
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
                color: theme.accentForeground
                padding: 3
                MouseArea {
                    anchors.fill: parent
                    onPressed: {
                        root.focusTarget.forceActiveFocus()
                        editor.selectedClipId = modelData.id
                    }
                    onClicked: speedMenu.open()
                }
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
                opacity: clipBlock.selected || clipHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    enabled: clipBlock.selected
                    property real pressTrackX; property real pressWidth; property real originalIn
                    onPressed: {
                        root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                        pressWidth = clipBlock.width; originalIn = modelData.in
                        clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                        editor.beginCoalescedEdit("trim-" + modelData.id)
                    }
                    onPositionChanged: if (pressed) {
                        const delta = mapToItem(root, mouse.x, mouse.y).x - pressTrackX
                        clipBlock.gestureWidth = Math.max(.1 / modelData.speed * root.pixelsPerSecond, pressWidth - delta)
                    }
                    onReleased: {
                        const facade = editor
                        const delta = pressWidth - clipBlock.gestureWidth
                        clipBlock.gestureActive = false
                        facade.trimClip(modelData.id, originalIn + delta / root.pixelsPerSecond * modelData.speed, modelData.out)
                        facade.endCoalescedEdit()
                    }
                    onCanceled: { clipBlock.gestureActive = false; editor.endCoalescedEdit() }
                }
            }
            Rectangle {
                anchors.right: parent.right; width: 6; height: parent.height; color: theme.accent; radius: 3
                opacity: clipBlock.selected || clipHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                MouseArea {
                    anchors.fill: parent; cursorShape: Qt.SizeHorCursor
                    enabled: clipBlock.selected
                    property real pressTrackX; property real pressWidth; property real originalOut
                    onPressed: {
                        root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                        pressWidth = clipBlock.width; originalOut = modelData.out
                        clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                        editor.beginCoalescedEdit("trim-" + modelData.id)
                    }
                    onPositionChanged: if (pressed) {
                        const delta = mapToItem(root, mouse.x, mouse.y).x - pressTrackX
                        clipBlock.gestureWidth = Math.max(.1 / modelData.speed * root.pixelsPerSecond, pressWidth + delta)
                    }
                    onReleased: {
                        const facade = editor
                        const delta = clipBlock.gestureWidth - pressWidth
                        clipBlock.gestureActive = false
                        facade.trimClip(modelData.id, modelData.in, originalOut + delta / root.pixelsPerSecond * modelData.speed)
                        facade.endCoalescedEdit()
                    }
                    onCanceled: { clipBlock.gestureActive = false; editor.endCoalescedEdit() }
                }
            }
        }
    }
}
