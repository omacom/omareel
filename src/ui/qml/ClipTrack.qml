import QtQuick
import QtQuick.Controls
import Omareel.Ui

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
            radius: theme.radius
            color: clipBlock.selected ? theme.selectedFill
                : clipHover.hovered ? theme.hoverFill : theme.normalFill
            border.width: clipBlock.selected ? theme.selectedBorderWidth
                : clipHover.hovered ? theme.hoverBorderWidth : theme.normalBorderWidth
            border.color: clipBlock.selected ? theme.selectedBorder
                : clipHover.hovered ? theme.hoverBorder : theme.normalBorder
            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
            Behavior on border.color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
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
                    color: theme.textMuted
                }
            }
            MouseArea {
                anchors.fill: parent
                onPressed: root.focusTarget.forceActiveFocus()
                onClicked: editor.selectedClipId = modelData.id
            }
            Rectangle {
                anchors.centerIn: parent
                visible: clipBlock.width >= 68
                width: Math.min(parent.width - 16, clipLabel.implicitWidth + 16)
                height: 24
                radius: theme.radius
                color: Qt.alpha(theme.surface, .80)
                border.width: 1
                border.color: theme.hairline
                Label {
                    id: clipLabel
                    anchors.fill: parent
                    anchors.leftMargin: 8; anchors.rightMargin: 8
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    elide: Text.ElideRight
                    text: "Clip · " + Number(modelData.speed).toFixed(modelData.speed % 1 ? 2 : 0) + "×"
                    font.weight: Font.Medium
                    font.pixelSize: theme.font.body
                    color: theme.foreground
                }
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
                width: 120
                padding: 4
                background: OmPopupCard { padding: 0 }
                Repeater {
                    model: [0.5,0.75,1,1.2,1.4,1.6,1.8,2,3,4,8,16,24]
                    delegate: MenuItem {
                        required property real modelData
                        height: theme.space.popupRowHeight
                        text: modelData + "×"
                        contentItem: Text { text: parent.text; color: parent.highlighted ? theme.menuSelectedText : theme.menuText; font.family: theme.fontFamily; font.pixelSize: theme.font.body; verticalAlignment: Text.AlignVCenter }
                        onTriggered: editor.setClipSpeed(clipBlock.modelData.id, modelData)
                        background: Rectangle { radius: theme.radius; color: parent.highlighted ? theme.menuSelectedBackground : "transparent" }
                    }
                }
            }
            Rectangle {
                width: 8; height: parent.height; color: clipBlock.border.color; radius: theme.radius
                opacity: clipBlock.selected || clipHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 120 } }
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
                anchors.right: parent.right; width: 8; height: parent.height; color: clipBlock.border.color; radius: theme.radius
                opacity: clipBlock.selected || clipHover.hovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 120 } }
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
