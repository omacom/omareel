import QtQuick
import QtQuick.Controls
import Omareel.Ui

Item {
    id: root
    required property real pixelsPerSecond
    required property Item focusTarget
    property real viewportX: 0
    property real viewportWidth: width
    signal scrubStarted()
    signal scrubMoved(real time)
    signal scrubFinished()
    signal rangeStarted(real time)
    signal rangeMoved(real time)
    signal rangeFinished(real time)
    signal rangeCanceled()
    signal clearRangeRequested()
    function timeAt(area, mouse) {
        return Math.max(0, Math.min(editor.duration,
            area.mapToItem(root, mouse.x, mouse.y).x / root.pixelsPerSecond))
    }
    function scrubAt(area, mouse) {
        root.scrubMoved(root.timeAt(area, mouse))
    }
    function outputStart(index) {
        let value = 0
        for (let i=0;i<index;i++) value += (editor.clips[i].out-editor.clips[i].in)/editor.clips[i].speed
        return value
    }
    MouseArea {
        id: emptyClipArea
        anchors.fill: parent
        preventStealing: true
        property bool selectingRange: false
        onPressed: mouse => {
            root.focusTarget.forceActiveFocus()
            selectingRange = (mouse.modifiers & Qt.ShiftModifier) !== 0
            if (selectingRange) root.rangeStarted(root.timeAt(emptyClipArea, mouse))
            else {
                editor.selectedClipId = ""
                root.scrubStarted(); root.scrubAt(emptyClipArea, mouse)
            }
        }
        onPositionChanged: mouse => {
            if (!pressed) return
            if (selectingRange) root.rangeMoved(root.timeAt(emptyClipArea, mouse))
            else root.scrubAt(emptyClipArea, mouse)
        }
        onReleased: mouse => {
            if (selectingRange) root.rangeFinished(root.timeAt(emptyClipArea, mouse))
            else { root.scrubAt(emptyClipArea, mouse); root.scrubFinished() }
            selectingRange = false
        }
        onCanceled: {
            if (selectingRange) root.rangeCanceled()
            else root.scrubFinished()
            selectingRange = false
        }
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
            property string gestureEdge: ""
            property real gestureX: 0
            property real gestureWidth: 0
            readonly property real displayedIn: gestureActive && gestureEdge === "left"
                ? leftTrim.previewIn : modelData.in
            readonly property real displayedOut: gestureActive && gestureEdge === "right"
                ? rightTrim.previewOut : modelData.out
            property real bodyPressTrackX: 0
            property real bodyPressBlockX: 0
            property real bodyOriginalIn: 0
            property bool bodyDragged: false
            property bool bodyWasSelected: false
            function beginBodyDrag(area, mouse) {
                root.focusTarget.forceActiveFocus()
                root.clearRangeRequested()
                bodyWasSelected = selected
                editor.selectedClipId = modelData.id
                bodyPressTrackX = area.mapToItem(root, mouse.x, mouse.y).x
                bodyPressBlockX = clipBlock.x
                bodyOriginalIn = modelData.in
                bodyDragged = false
                gestureX = clipBlock.x
                gestureWidth = clipBlock.width
                gestureEdge = "move"
                gestureActive = true
                editor.beginCoalescedEdit("move-clip-" + modelData.id)
                editor.traceInput(area.objectName, "press", mouse.x, mouse.y)
            }
            function updateBodyDrag(area, mouse) {
                if (!area.pressed) return
                const trackX = area.mapToItem(root, mouse.x, mouse.y).x
                const deltaPixels = trackX - bodyPressTrackX
                if (Math.abs(deltaPixels) >= 3) bodyDragged = true
                gestureX = Math.max(0, bodyPressBlockX + deltaPixels)
                editor.traceInput(area.objectName, "move", mouse.x, mouse.y)
            }
            function endBodyDrag(area, mouse, phase) {
                const facade = editor
                facade.traceInput(area.objectName, phase, mouse ? mouse.x : 0, mouse ? mouse.y : 0)
                const nextIn = bodyOriginalIn
                    + (gestureX - bodyPressBlockX) / root.pixelsPerSecond * modelData.speed
                gestureActive = false
                gestureEdge = ""
                if (phase === "release" && bodyDragged)
                    facade.moveClip(modelData.id, nextIn)
                else if (phase === "release" && bodyWasSelected)
                    facade.selectedClipId = ""
                facade.endCoalescedEdit()
            }
            x: gestureActive ? gestureX : root.outputStart(index) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth
                : (modelData.out-modelData.in)/modelData.speed * root.pixelsPerSecond
            height: root.height
            y: 0
            radius: 0
            color: theme.normalFill
            border.width: theme.normalBorderWidth
            border.color: theme.normalBorder
            clip: false
            Item {
                id: filmstrip
                anchors.fill: parent
                clip: true
                readonly property real tileWidth: height * 16 / 9
                readonly property int firstTile: Math.max(0,
                    Math.floor((root.viewportX - clipBlock.x) / tileWidth) - 1)
                readonly property int lastTile: Math.min(Math.ceil(clipBlock.width / tileWidth),
                    Math.ceil((root.viewportX + root.viewportWidth - clipBlock.x) / tileWidth) + 1)
                Repeater {
                    model: Math.max(0, filmstrip.lastTile - filmstrip.firstTile)
                    delegate: Image {
                        required property int index
                        readonly property int tileIndex: filmstrip.firstTile + index
                        x: tileIndex * filmstrip.tileWidth
                        width: Math.min(filmstrip.tileWidth + 1, clipBlock.width - x)
                        height: filmstrip.height
                        asynchronous: true
                        fillMode: Image.PreserveAspectCrop
                        sourceSize: Qt.size(160, 90)
                        source: {
                            const revision = editor.videoThumbnailRevision
                            const sourceTime = clipBlock.displayedIn
                                + (x + width / 2) / root.pixelsPerSecond * clipBlock.modelData.speed
                            return editor.videoThumbnailUrlForClip(sourceTime,
                                clipBlock.displayedIn, clipBlock.displayedOut)
                        }
                    }
                }
            }
            Rectangle {
                anchors.fill: parent
                z: 2
                color: "transparent"
                border.width: 1
                border.color: clipBlock.selected ? theme.accent : theme.normalBorder
                radius: 0
            }
            MouseArea {
                id: clipBody
                objectName: "clipBody-" + modelData.id
                anchors.fill: parent
                anchors.leftMargin: Math.min(14, clipBlock.width / 3)
                anchors.rightMargin: Math.min(14, clipBlock.width / 3)
                preventStealing: true
                property bool movingClip: false
                property bool selectingRange: false
                cursorShape: movingClip ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                onPressed: mouse => {
                    selectingRange = (mouse.modifiers & Qt.ShiftModifier) !== 0
                    movingClip = !selectingRange && (mouse.modifiers & Qt.ControlModifier) !== 0
                    if (selectingRange) root.rangeStarted(root.timeAt(clipBody, mouse))
                    else if (movingClip) clipBlock.beginBodyDrag(clipBody, mouse)
                    else {
                        root.focusTarget.forceActiveFocus()
                        root.scrubStarted()
                        root.scrubAt(clipBody, mouse)
                    }
                }
                onPositionChanged: mouse => {
                    if (!pressed) return
                    if (selectingRange) root.rangeMoved(root.timeAt(clipBody, mouse))
                    else if (movingClip) clipBlock.updateBodyDrag(clipBody, mouse)
                    else root.scrubAt(clipBody, mouse)
                }
                onReleased: mouse => {
                    if (selectingRange) root.rangeFinished(root.timeAt(clipBody, mouse))
                    else if (movingClip) clipBlock.endBodyDrag(clipBody, mouse, "release")
                    else { root.scrubAt(clipBody, mouse); root.scrubFinished() }
                    movingClip = false
                    selectingRange = false
                }
                onCanceled: {
                    if (selectingRange) root.rangeCanceled()
                    else if (movingClip) clipBlock.endBodyDrag(clipBody, null, "cancel")
                    else root.scrubFinished()
                    movingClip = false
                    selectingRange = false
                }
            }
            Rectangle {
                id: clipBadge
                z: 3
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
                    id: badgeMouse
                    objectName: "clipBadge-" + modelData.id
                    anchors.fill: parent
                    preventStealing: true
                    property bool movingClip: false
                    property bool selectingRange: false
                    property bool pressedWithControl: false
                    property bool pressedWithShift: false
                    property bool dragged: false
                    property real pressTrackX: 0
                    cursorShape: movingClip ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                    onPressed: mouse => {
                        selectingRange = (mouse.modifiers & Qt.ShiftModifier) !== 0
                        movingClip = !selectingRange && (mouse.modifiers & Qt.ControlModifier) !== 0
                        pressedWithControl = movingClip
                        pressedWithShift = selectingRange
                        dragged = false
                        pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                        if (selectingRange) root.rangeStarted(root.timeAt(badgeMouse, mouse))
                        else if (movingClip) clipBlock.beginBodyDrag(badgeMouse, mouse)
                        else {
                            root.focusTarget.forceActiveFocus()
                            root.scrubStarted()
                            root.scrubAt(badgeMouse, mouse)
                        }
                    }
                    onPositionChanged: mouse => {
                        if (!pressed) return
                        if (Math.abs(mapToItem(root, mouse.x, mouse.y).x - pressTrackX) >= 3)
                            dragged = true
                        if (selectingRange) root.rangeMoved(root.timeAt(badgeMouse, mouse))
                        else if (movingClip) clipBlock.updateBodyDrag(badgeMouse, mouse)
                        else root.scrubAt(badgeMouse, mouse)
                    }
                    onReleased: mouse => {
                        if (selectingRange) root.rangeFinished(root.timeAt(badgeMouse, mouse))
                        else if (movingClip) clipBlock.endBodyDrag(badgeMouse, mouse, "release")
                        else { root.scrubAt(badgeMouse, mouse); root.scrubFinished() }
                        movingClip = false
                        selectingRange = false
                    }
                    onCanceled: {
                        if (selectingRange) root.rangeCanceled()
                        else if (movingClip) clipBlock.endBodyDrag(badgeMouse, null, "cancel")
                        else root.scrubFinished()
                        movingClip = false
                        selectingRange = false
                    }
                    onClicked: {
                        if (!dragged && !pressedWithControl && !pressedWithShift)
                            speedMenu.open()
                    }
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
                compact: true
                active: clipBlock.selected
                visible: clipBlock.selected
                dragging: mouseArea.pressed
                mouseArea.enabled: clipBlock.selected
                property real pressTrackX
                property real pressBlockX
                property real pressWidth
                property real originalIn
                property real previewIn
                mouseArea.onPressed: mouse => {
                    editor.traceInput(mouseArea.objectName, "press", mouse.x, mouse.y)
                    root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                    root.clearRangeRequested()
                    editor.pause()
                    leftTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    leftTrim.pressBlockX = clipBlock.x
                    leftTrim.pressWidth = clipBlock.width; leftTrim.originalIn = modelData.in
                    leftTrim.previewIn = modelData.in
                    clipBlock.gestureX = clipBlock.x
                    clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                    clipBlock.gestureEdge = "left"
                    editor.beginCoalescedEdit("trim-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    editor.traceInput(mouseArea.objectName, "move", mouse.x, mouse.y)
                    const delta = mouseArea.mapToItem(root, mouse.x, mouse.y).x - leftTrim.pressTrackX
                    const lower = index > 0 ? editor.clips[index - 1].out : 0
                    leftTrim.previewIn = Math.max(lower, Math.min(modelData.out - .1,
                        leftTrim.originalIn + delta / root.pixelsPerSecond * modelData.speed))
                    const actualDelta = (leftTrim.previewIn - leftTrim.originalIn)
                        / modelData.speed * root.pixelsPerSecond
                    clipBlock.gestureX = leftTrim.pressBlockX + actualDelta
                    clipBlock.gestureWidth = leftTrim.pressWidth - actualDelta
                } }
                mouseArea.onReleased: {
                    editor.traceInput(mouseArea.objectName, "release", 0, 0)
                    const facade = editor
                    clipBlock.gestureActive = false
                    clipBlock.gestureEdge = ""
                    facade.trimClip(modelData.id, leftTrim.previewIn, modelData.out)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    editor.traceInput(mouseArea.objectName, "cancel", 0, 0)
                    clipBlock.gestureActive = false
                    clipBlock.gestureEdge = ""
                    editor.endCoalescedEdit()
                }
            }
            OmTrimHandle {
                id: rightTrim
                objectName: "clipTrimHandle-" + modelData.id + "-right"
                x: parent.width - width
                edge: "right"
                accent: theme.accent
                compact: true
                active: clipBlock.selected
                visible: clipBlock.selected
                dragging: mouseArea.pressed
                mouseArea.enabled: clipBlock.selected
                property real pressTrackX
                property real pressBlockX
                property real pressWidth
                property real originalOut
                property real previewOut
                mouseArea.onPressed: mouse => {
                    editor.traceInput(mouseArea.objectName, "press", mouse.x, mouse.y)
                    root.focusTarget.forceActiveFocus(); editor.selectedClipId = modelData.id
                    root.clearRangeRequested()
                    editor.pause()
                    rightTrim.pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    rightTrim.pressBlockX = clipBlock.x
                    rightTrim.pressWidth = clipBlock.width; rightTrim.originalOut = modelData.out
                    rightTrim.previewOut = modelData.out
                    clipBlock.gestureX = clipBlock.x
                    clipBlock.gestureWidth = clipBlock.width; clipBlock.gestureActive = true
                    clipBlock.gestureEdge = "right"
                    editor.beginCoalescedEdit("trim-" + modelData.id)
                }
                mouseArea.onPositionChanged: mouse => { if (mouseArea.pressed) {
                    editor.traceInput(mouseArea.objectName, "move", mouse.x, mouse.y)
                    const delta = mouseArea.mapToItem(root, mouse.x, mouse.y).x - rightTrim.pressTrackX
                    const upper = index + 1 < editor.clips.length
                        ? editor.clips[index + 1].in : editor.sourceDuration
                    rightTrim.previewOut = Math.max(modelData.in + .1, Math.min(upper,
                        rightTrim.originalOut + delta / root.pixelsPerSecond * modelData.speed))
                    clipBlock.gestureX = rightTrim.pressBlockX
                    clipBlock.gestureWidth = rightTrim.pressWidth
                        + (rightTrim.previewOut - rightTrim.originalOut)
                        / modelData.speed * root.pixelsPerSecond
                } }
                mouseArea.onReleased: {
                    editor.traceInput(mouseArea.objectName, "release", 0, 0)
                    const facade = editor
                    clipBlock.gestureActive = false
                    clipBlock.gestureEdge = ""
                    facade.trimClip(modelData.id, modelData.in, rightTrim.previewOut)
                    facade.endCoalescedEdit()
                }
                mouseArea.onCanceled: {
                    editor.traceInput(mouseArea.objectName, "cancel", 0, 0)
                    clipBlock.gestureActive = false
                    clipBlock.gestureEdge = ""
                    editor.endCoalescedEdit()
                }
            }
        }
    }
}
