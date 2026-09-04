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
            y: clipHover.hovered ? -1 : 0
            radius: theme.radius
            color: clipBlock.selected ? theme.selectedFill
                : clipHover.hovered ? theme.hoverFill : theme.normalFill
            border.width: clipBlock.selected ? Math.max(1, theme.selectedBorderWidth)
                : clipHover.hovered ? theme.hoverBorderWidth : theme.normalBorderWidth
            border.color: clipBlock.selected ? theme.accent
                : clipHover.hovered ? Qt.alpha(theme.accent, .72) : theme.normalBorder
            Behavior on y {
                enabled: !editor.loading
                NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            Behavior on color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            Behavior on border.color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            clip: false
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
                cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                onPressed: root.focusTarget.forceActiveFocus()
                onClicked: editor.selectedClipId = modelData.id
            }
            Rectangle {
                id: clipBadge
                anchors.centerIn: parent
                visible: clipBlock.width >= 68
                width: Math.min(parent.width - 16, clipLabel.implicitWidth + 16)
                height: 24
                opacity: leftTrim.engaged || rightTrim.engaged ? .60 : 1
                radius: theme.radius
                color: Qt.alpha(theme.surface, .80)
                border.width: 1
                border.color: theme.hairline
                Behavior on opacity {
                    enabled: !editor.loading
                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
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
            OmTrimHandle {
                id: leftTrim
                objectName: "clipTrimHandle-" + modelData.id + "-left"
                edge: "left"
                accent: theme.accent
                active: clipBlock.selected || clipHover.hovered
                dragging: mouseArea.pressed
                mouseArea.enabled: clipBlock.selected
                property real pressTrackX
                property real pressWidth
                property real originalIn
                mouseArea.onPressed: mouse => {
                    root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                    leftTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    leftTrim.pressWidth = clipBlock.width; leftTrim.originalIn = modelData.in
                    clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                    editor.beginCoalescedEdit("trim-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    const delta = mouseArea.mapToItem(root, mouse.x, mouse.y).x - leftTrim.pressTrackX
                    clipBlock.gestureWidth = Math.max(.1 / modelData.speed * root.pixelsPerSecond,
                                                      leftTrim.pressWidth - delta)
                } }
                mouseArea.onReleased: {
                    const facade = editor
                    const delta = leftTrim.pressWidth - clipBlock.gestureWidth
                    clipBlock.gestureActive = false
                    facade.trimClip(modelData.id,
                                    leftTrim.originalIn + delta / root.pixelsPerSecond * modelData.speed,
                                    modelData.out)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    clipBlock.gestureActive = false
                    editor.endCoalescedEdit()
                }
            }
            OmTrimHandle {
                id: rightTrim
                objectName: "clipTrimHandle-" + modelData.id + "-right"
                anchors.right: parent.right
                edge: "right"
                accent: theme.accent
                active: clipBlock.selected || clipHover.hovered
                dragging: mouseArea.pressed
                mouseArea.enabled: clipBlock.selected
                property real pressTrackX
                property real pressWidth
                property real originalOut
                mouseArea.onPressed: mouse => {
                    root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                    rightTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    rightTrim.pressWidth = clipBlock.width; rightTrim.originalOut = modelData.out
                    clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                    editor.beginCoalescedEdit("trim-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    const delta = mouseArea.mapToItem(root, mouse.x, mouse.y).x - rightTrim.pressTrackX
                    clipBlock.gestureWidth = Math.max(.1 / modelData.speed * root.pixelsPerSecond,
                                                      rightTrim.pressWidth + delta)
                } }
                mouseArea.onReleased: {
                    const facade = editor
                    const delta = clipBlock.gestureWidth - rightTrim.pressWidth
                    clipBlock.gestureActive = false
                    facade.trimClip(modelData.id, modelData.in,
                                    rightTrim.originalOut + delta / root.pixelsPerSecond * modelData.speed)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    clipBlock.gestureActive = false
                    editor.endCoalescedEdit()
                }
            }
        }
    }
}
