import QtQuick
import QtQuick.Controls
import QtQuick.Shapes
import Omareel.Ui

FocusScope {
    id: root
    required property real scaleFactor
    signal scaleFactorRequested(real value)
    property real previousScaleFactor: 1
    property real wheelRequestedScale: -1
    property real zoomAnchorX: -1
    property real zoomAnchorTime: -1
    function applyZoomAnchor() {
        if (zoomAnchorTime < 0) return
        flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width,
            zoomAnchorTime * pixelsPerSecond - zoomAnchorX))
        zoomAnchorX = -1
        zoomAnchorTime = -1
    }
    function zoomBy(factor, anchorX) {
        const next = Math.max(1, Math.min(16, scaleFactor * factor))
        if (Math.abs(next - scaleFactor) < 0.000001) return
        applyZoomAnchor()
        zoomAnchorX = Math.max(0, Math.min(flick.width, anchorX))
        zoomAnchorTime = (flick.contentX + zoomAnchorX) / pixelsPerSecond
        wheelRequestedScale = next
        scaleFactorRequested(next)
    }
    onScaleFactorChanged: {
        if (flick.width <= 0 || basePixels <= 0) {
            previousScaleFactor = scaleFactor
            return
        }
        if (Math.abs(scaleFactor - wheelRequestedScale) > 0.000001) {
            const oldPixelsPerSecond = basePixels * previousScaleFactor
            const oldMaxX = Math.max(0, editor.duration * oldPixelsPerSecond
                + timelinePadding - flick.width)
            const oldContentX = zoomAnchorTime < 0 ? flick.contentX
                : Math.max(0, Math.min(oldMaxX,
                    zoomAnchorTime * oldPixelsPerSecond - zoomAnchorX))
            zoomAnchorX = flick.width / 2
            zoomAnchorTime = (oldContentX + zoomAnchorX) / oldPixelsPerSecond
        } else if (zoomAnchorTime < 0) {
            zoomAnchorX = flick.width / 2
            zoomAnchorTime = (flick.contentX + zoomAnchorX)
                / (basePixels * previousScaleFactor)
        }
        wheelRequestedScale = -1
        previousScaleFactor = scaleFactor
        Qt.callLater(root.applyZoomAnchor)
    }
    signal cameraSelected()
    property real rangeAnchor: -1
    property real rangeHead: -1
    property bool selectingRange: false
    property bool resizingRange: false
    property string rangeResizeEdge: ""
    property real rangeResizeOffset: 0
    property real priorRangeStart: -1
    property real priorRangeEnd: -1
    readonly property real rangeStart: Math.min(rangeAnchor, rangeHead)
    readonly property real rangeEnd: Math.max(rangeAnchor, rangeHead)
    readonly property bool hasRange: rangeAnchor >= 0
        && rangeHead >= 0 && rangeEnd - rangeStart > 0.000001
    function clearRange() {
        selectingRange = false
        resizingRange = false
        rangeAnchor = -1
        rangeHead = -1
        editor.clearPlaybackRange()
    }
    function beginRange(time) {
        root.forceActiveFocus()
        editor.beginScrub()
        editor.selectedClipId = ""
        editor.clearPlaybackRange()
        rangeAnchor = Math.max(0, Math.min(editor.duration, time))
        rangeHead = rangeAnchor
        selectingRange = true
        resizingRange = false
    }
    function beginRangeResize(edge, pointerTime) {
        if (!hasRange) return
        root.forceActiveFocus()
        editor.beginScrub()
        priorRangeStart = rangeStart
        priorRangeEnd = rangeEnd
        rangeResizeEdge = edge
        rangeResizeOffset = (edge === "left" ? priorRangeStart : priorRangeEnd) - pointerTime
        resizingRange = true
        selectingRange = true
    }
    function updateRange(time) {
        if (selectingRange) {
            rangeHead = Math.max(0, Math.min(editor.duration, time))
            if (resizingRange)
                rangeAnchor = rangeResizeEdge === "left" ? priorRangeEnd : priorRangeStart
            if (hasRange) editor.setPlaybackRange(rangeStart, rangeEnd)
            else editor.clearPlaybackRange()
            editor.scrubTo(rangeHead)
        }
    }
    function finishRange(time) {
        updateRange(time)
        selectingRange = false
        if ((rangeEnd - rangeStart) * pixelsPerSecond < 2) {
            clearRange()
            editor.seek(editor.position)
        }
        else {
            editor.setPlaybackRange(rangeStart, rangeEnd)
            editor.seek(resizingRange ? rangeHead : rangeStart)
        }
        resizingRange = false
    }
    function cancelRange() {
        if (resizingRange) {
            rangeAnchor = priorRangeStart
            rangeHead = priorRangeEnd
            selectingRange = false
            resizingRange = false
            editor.setPlaybackRange(rangeStart, rangeEnd)
            editor.seek(rangeStart)
        } else {
            clearRange()
            editor.seek(editor.position)
        }
    }
    function deleteSelectedRange() {
        if (!hasRange || !editor.deleteOutputRange(rangeStart, rangeEnd)) return false
        clearRange()
        return true
    }
    Connections {
        target: editor
        function onDurationChanged() { root.clearRange() }
    }
    implicitHeight: editor.hasCamera ? 204 : 148
    property real labelWidth: 72
    readonly property real timelinePadding: 40
    property real basePixels: Math.max(1, width - labelWidth - timelinePadding)
        / Math.max(0.001, editor.duration)
    property real pixelsPerSecond: basePixels * scaleFactor
    readonly property real tickStepSeconds: {
        const target = 90 / Math.max(0.001, pixelsPerSecond)
        const power = Math.pow(10, Math.floor(Math.log10(target)))
        for (const multiple of [1, 2, 5, 10])
            if (multiple * power >= target) return multiple * power
        return 10 * power
    }
    function formatRulerTime(seconds) {
        const milliseconds = Math.max(0, Math.round(seconds * 1000))
        const minutes = Math.floor(milliseconds / 60000)
        const secondsPart = Math.floor(milliseconds / 1000) % 60
        const time = (minutes < 10 ? "0" : "") + minutes + ":"
            + (secondsPart < 10 ? "0" : "") + secondsPart
        if (tickStepSeconds >= 1) return time
        const precision = tickStepSeconds >= .1 ? 1 : tickStepSeconds >= .01 ? 2 : 3
        return time + "." + ("00" + (milliseconds % 1000)).slice(-3).slice(0, precision)
    }
    readonly property int trackCount: editor.hasCamera ? 3 : 2
    readonly property real tracksBottom: 32 + trackCount * 48 + (trackCount - 1) * 8

    Rectangle { anchors.fill: parent; color: theme.surface }
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 1
        color: theme.hairline
    }
    HoverHandler { id: timelineHover }

    Item {
        x: 0
        y: 0
        width: root.labelWidth
        height: parent.height
        Repeater {
            model: editor.hasCamera ? ["Clip", "Zoom", "Camera"] : ["Clip", "Zoom"]
            delegate: Label {
                required property string modelData
                required property int index
                x: 0
                y: 32 + index * 56
                width: root.labelWidth
                height: 48
                leftPadding: 14
                verticalAlignment: Text.AlignVCenter
                text: modelData
                color: theme.textFaint
                font.pixelSize: theme.font.caption
                font.weight: Font.Medium
                font.capitalization: Font.AllUppercase
                font.letterSpacing: .6
            }
        }
        Rectangle {
            anchors.right: parent.right
            width: 1
            height: root.tracksBottom
            color: theme.hairline
        }
    }

    Flickable {
        id: flick
        objectName: "timelineFlickable"
        x: root.labelWidth
        y: 0
        width: parent.width - x
        height: parent.height
        contentWidth: Math.max(width, editor.duration * root.pixelsPerSecond
            + root.timelinePadding)
        contentHeight: height
        clip: true
        interactive: false
        boundsBehavior: Flickable.StopAtBounds
        WheelHandler {
            id: timelineWheel
            target: null
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: event => {
                const pixel = event.pixelDelta.y !== 0 ? event.pixelDelta.y : event.pixelDelta.x
                const angle = event.angleDelta.y !== 0 ? event.angleDelta.y : event.angleDelta.x
                const amount = pixel !== 0 ? pixel : angle
                if (amount !== 0) {
                    const scene = timelineWheel.point.scenePosition
                    const pointerX = flick.mapFromItem(null, scene.x, scene.y).x
                    root.zoomBy(Math.pow(1.25, amount / 120), pointerX)
                }
                event.accepted = true
            }
        }
        DragHandler {
            target: null
            acceptedButtons: Qt.RightButton
            property real startContentX: 0
            onActiveChanged: if (active) startContentX = flick.contentX
            onTranslationChanged: if (active)
                flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width,
                    startContentX - translation.x))
        }
        readonly property int firstVisibleTick: Math.max(0,
            Math.floor(contentX / (root.tickStepSeconds * root.pixelsPerSecond)) - 1)
        readonly property int visibleTickCount: Math.ceil(width
            / (root.tickStepSeconds * root.pixelsPerSecond)) + 4

        Item {
            id: content
            width: flick.contentWidth
            height: flick.height
            MouseArea { anchors.fill: parent; onPressed: root.forceActiveFocus() }

            Item {
                id: ruler
                width: parent.width
                height: 32
                Repeater {
                    model: flick.visibleTickCount
                    delegate: Item {
                        id: tickDelegate
                        required property int index
                        property int tickIndex: flick.firstVisibleTick + index
                        property real tickTime: tickIndex * root.tickStepSeconds
                        visible: tickTime <= editor.duration
                        x: tickTime * root.pixelsPerSecond
                        width: 1
                        height: ruler.height
                        Rectangle {
                            anchors.bottom: parent.bottom
                            width: 1
                            height: 9
                            color: theme.hairlineStrong
                        }
                        Loader {
                            active: true
                            x: 5
                            y: 5
                            sourceComponent: Text {
                                text: root.formatRulerTime(tickDelegate.tickTime)
                                color: theme.textFaint
                                font.pixelSize: theme.font.caption
                                font.family: theme.fontFamily
                            }
                        }
                    }
                }
                MouseArea {
                    id: rulerMouse
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    preventStealing: true
                    property bool selectingRange: false
                    function timeAt(px) {
                        return Math.max(0, Math.min(editor.duration, px / root.pixelsPerSecond))
                    }
                    function seekAt(px) {
                        editor.scrubTo(timeAt(px))
                    }
                    onPressed: mouse => {
                        root.forceActiveFocus()
                        selectingRange = (mouse.modifiers & Qt.ShiftModifier) !== 0
                        if (selectingRange) root.beginRange(timeAt(mouse.x))
                        else { editor.beginScrub(); seekAt(mouse.x) }
                    }
                    onPositionChanged: mouse => {
                        if (!pressed) return
                        if (selectingRange) root.updateRange(timeAt(mouse.x))
                        else seekAt(mouse.x)
                    }
                    onReleased: mouse => {
                        if (selectingRange) root.finishRange(timeAt(mouse.x))
                        else { seekAt(mouse.x); editor.endScrub() }
                        selectingRange = false
                    }
                    onCanceled: {
                        if (selectingRange) root.cancelRange()
                        else editor.endScrub()
                        selectingRange = false
                    }
                }
            }

            Rectangle {
                x: 0
                y: 32
                width: parent.width
                height: 48
                color: theme.normalFill
            }
            ClipTrack {
                x: 0
                y: 36
                width: parent.width
                height: 40
                pixelsPerSecond: root.pixelsPerSecond
                viewportX: flick.contentX
                viewportWidth: flick.width
                focusTarget: root
                onScrubStarted: editor.beginScrub()
                onScrubMoved: time => editor.scrubTo(time)
                onScrubFinished: editor.endScrub()
                onRangeStarted: time => root.beginRange(time)
                onRangeMoved: time => root.updateRange(time)
                onRangeFinished: time => root.finishRange(time)
                onRangeCanceled: root.cancelRange()
                onClearRangeRequested: root.clearRange()
            }
            Rectangle {
                id: clipRangeSelection
                objectName: "clipRangeSelection"
                visible: root.rangeAnchor >= 0 && root.rangeHead >= 0
                    && (root.resizingRange
                        || Math.abs(root.rangeHead - root.rangeAnchor) * root.pixelsPerSecond >= 2)
                x: root.rangeStart * root.pixelsPerSecond
                y: 32
                width: Math.max(0, (root.rangeEnd - root.rangeStart) * root.pixelsPerSecond)
                height: 48
                color: Qt.alpha(theme.accent, .30)
                border.width: 2
                border.color: theme.accent
                z: 10
                OmTrimHandle {
                    objectName: "rangeTrimHandle-left"
                    edge: "left"
                    accent: theme.accent
                    active: true
                    visible: !root.selectingRange || root.resizingRange
                    mouseArea.onPressed: mouse => root.beginRangeResize("left",
                        mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond)
                    mouseArea.onPositionChanged: mouse => {
                        if (mouseArea.pressed)
                            root.updateRange(Math.max(0, Math.min(editor.duration,
                                mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond
                                    + root.rangeResizeOffset)))
                    }
                    mouseArea.onReleased: mouse => root.finishRange(Math.max(0, Math.min(editor.duration,
                        mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond
                            + root.rangeResizeOffset)))
                    mouseArea.onCanceled: root.cancelRange()
                }
                OmTrimHandle {
                    x: parent.width - width
                    objectName: "rangeTrimHandle-right"
                    edge: "right"
                    accent: theme.accent
                    active: true
                    visible: !root.selectingRange || root.resizingRange
                    mouseArea.onPressed: mouse => root.beginRangeResize("right",
                        mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond)
                    mouseArea.onPositionChanged: mouse => {
                        if (mouseArea.pressed)
                            root.updateRange(Math.max(0, Math.min(editor.duration,
                                mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond
                                    + root.rangeResizeOffset)))
                    }
                    mouseArea.onReleased: mouse => root.finishRange(Math.max(0, Math.min(editor.duration,
                        mouseArea.mapToItem(content, mouse.x, mouse.y).x / root.pixelsPerSecond
                            + root.rangeResizeOffset)))
                    mouseArea.onCanceled: root.cancelRange()
                }
            }
            Rectangle {
                x: 0
                y: 88
                width: parent.width
                height: 48
                color: theme.normalFill
                border.width: 1
                border.color: theme.normalBorder
            }
            ZoomTrack {
                x: 0
                y: 92
                width: parent.width
                height: 40
                pixelsPerSecond: root.pixelsPerSecond
                focusTarget: root
            }
            Rectangle {
                id: cameraTrack
                visible: editor.hasCamera
                x: 0
                y: 144
                width: parent.width
                height: 48
                color: theme.normalFill
                border.width: 1
                border.color: theme.normalBorder
            }
            Rectangle {
                id: cameraBlock
                visible: editor.hasCamera
                x: 0
                y: cameraHover.hovered ? 147 : 148
                width: Math.max(20, editor.duration * root.pixelsPerSecond)
                height: 40
                radius: theme.radius
                color: cameraHover.hovered ? theme.hoverFill : theme.normalFill
                border.width: 1
                border.color: cameraHover.hovered ? Qt.alpha(theme.accent, .72) : theme.normalBorder
                clip: true
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
                Row {
                    id: cameraStrip
                    anchors.fill: parent
                    anchors.margins: 3
                    spacing: 2
                    Repeater {
                        model: editor.cameraThumbnails
                        delegate: Item {
                            required property string modelData
                            width: (cameraStrip.width - cameraStrip.spacing
                                    * Math.max(0, editor.cameraThumbnails.length - 1))
                                   / Math.max(1, editor.cameraThumbnails.length)
                            height: cameraStrip.height
                            Image {
                                anchors.fill: parent
                                source: modelData
                                sourceSize: Qt.size(112, 72)
                                fillMode: Image.PreserveAspectCrop
                                smooth: true
                            }
                            Rectangle {
                                anchors.fill: parent
                                color: "transparent"
                                border.width: 1
                                border.color: theme.accent
                                opacity: cameraHover.hovered ? 1 : 0
                                Behavior on opacity {
                                    enabled: !editor.loading
                                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                                }
                            }
                        }
                    }
                }
                Row {
                    visible: editor.cameraThumbnails.length === 0
                    anchors.fill: parent
                    anchors.margins: 3
                    spacing: 2
                    Repeater {
                        model: 8
                        Rectangle {
                            required property int index
                            width: (parent.width - 14) / 8
                            height: parent.height
                            color: index % 2 ? theme.normalFill : theme.hoverFill
                            border.width: cameraHover.hovered ? 1 : 0
                            border.color: theme.accent
                        }
                    }
                }
                Rectangle {
                    anchors.left: parent.left; anchors.leftMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    width: cameraLabel.implicitWidth + 14
                    height: 22
                    radius: theme.radius
                    color: Qt.alpha(theme.surface, .82)
                    border.width: 1; border.color: theme.hairline
                    Label {
                        id: cameraLabel
                        anchors.centerIn: parent
                        text: "Camera"
                        color: theme.foreground
                        font.pixelSize: theme.font.caption
                        font.weight: Font.DemiBold
                    }
                }
                HoverHandler { id: cameraHover }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onPressed: root.forceActiveFocus()
                    onClicked: root.cameraSelected()
                }
                ToolTip {
                    visible: cameraHover.hovered
                    text: "Open Camera controls"
                }
            }

            Rectangle {
                id: playhead
                x: editor.playheadPosition * root.pixelsPerSecond - width / 2
                y: 25
                width: playheadDrag.pressed ? 2 : 1.5
                height: root.tracksBottom - y
                color: theme.record
                z: 20
                Behavior on width {
                    enabled: !editor.loading
                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
                Item {
                    id: playheadHead
                    x: -5.25
                    y: -1
                    width: 12
                    height: 8
                    scale: playheadDrag.pressed ? 1.25 : playheadDrag.containsMouse ? 1.15 : 1
                    transformOrigin: Item.Center
                    Behavior on scale {
                        enabled: !editor.loading
                        NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                    }
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: -6
                        radius: height / 2
                        color: Qt.alpha(theme.record, .30)
                        opacity: playheadDrag.pressed ? 1 : 0
                        Behavior on opacity {
                            enabled: !editor.loading
                            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                        }
                    }
                    Shape {
                        anchors.fill: parent
                        ShapePath {
                            strokeWidth: 0
                            fillColor: theme.record
                            startX: 0
                            startY: 0
                            PathLine { x: 12; y: 0 }
                            PathLine { x: 6; y: 8 }
                            PathLine { x: 0; y: 0 }
                        }
                    }
                }
                MouseArea {
                    id: playheadDrag
                    x: -10; y: -6
                    width: 21; height: 20
                    hoverEnabled: true
                    preventStealing: true
                    cursorShape: Qt.SizeHorCursor
                    function seekAt(mouseX, mouseY) {
                        const contentX = mapToItem(content, mouseX, mouseY).x
                        editor.scrubTo(Math.max(0, Math.min(editor.duration,
                                                           contentX / root.pixelsPerSecond)))
                    }
                    onPressed: mouse => {
                        root.forceActiveFocus()
                        editor.beginScrub()
                        seekAt(mouse.x, mouse.y)
                    }
                    onPositionChanged: mouse => {
                        if (pressed) seekAt(mouse.x, mouse.y)
                    }
                    onReleased: mouse => { seekAt(mouse.x, mouse.y); editor.endScrub() }
                    onCanceled: editor.endScrub()
                    ToolTip.visible: containsMouse
                    ToolTip.text: editor.formatTime(editor.position)
                    ToolTip.delay: 250
                }
            }
        }

        ScrollBar.horizontal: ScrollBar {
            id: horizontalBar
            policy: flick.contentWidth > flick.width + 0.5
                ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
            interactive: true
            height: 12
            topPadding: 2
            bottomPadding: 2
            minimumSize: Math.min(1, 32 / Math.max(1, width))
            opacity: 1
            background: Rectangle {
                width: parent.width
                height: 1
                y: (parent.height - height) / 2
                color: theme.hairline
            }
            contentItem: Rectangle {
                implicitWidth: 32
                implicitHeight: 8
                radius: theme.radius
                color: horizontalBar.hovered || horizontalBar.pressed
                    ? theme.foreground : theme.textFaint
                Behavior on color {
                    enabled: !editor.loading
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }
        }
    }
}
