import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Shapes
import QtQuick.Controls.impl
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
    signal musicSelected()
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
    // The Overlays lane shows once there is an annotation; the camera lane goes below it.
    readonly property bool hasOverlays: editor.overlays.length > 0
    readonly property real cameraLaneY: hasOverlays ? 200 : 144
    implicitHeight: 148 + (editor.hasCamera ? 56 : 0) + (hasOverlays ? 56 : 0) + (hasMusic ? musicLaneHeight + 8 : 0)
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
    readonly property int trackCount: 2 + (editor.hasCamera ? 1 : 0) + (hasOverlays ? 1 : 0)
    // The background music's thin lane, under the other tracks while there is music.
    readonly property bool hasMusic: editor.music.present === true
    readonly property real musicLaneY: 32 + trackCount * 56
    readonly property real musicLaneHeight: 28
    readonly property real tracksBottom: 32 + trackCount * 48 + (trackCount - 1) * 8
        + (hasMusic ? musicLaneHeight + 8 : 0)

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
            model: [{name: "Clip", icon: "film.svg"}, {name: "Zoom", icon: "zoom-in.svg"}]
                .concat(root.hasOverlays ? [{name: "Overlay", icon: "pen-line.svg"}] : [])
                .concat(editor.hasCamera ? [{name: "Camera", icon: "camera.svg"}] : [])
            delegate: Row {
                required property var modelData
                required property int index
                x: 10
                y: 32 + index * 56 + (48 - height) / 2
                spacing: 5
                IconImage {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 12; height: 12
                    source: "qrc:/omareel/assets/icons/lucide/" + modelData.icon
                    sourceSize: Qt.size(12, 12)
                    color: theme.textFaint
                }
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    textFormat: Text.PlainText
                    text: modelData.name
                    color: theme.textMuted
                    font.pixelSize: theme.type.caption
                    font.weight: Font.Medium
                }
            }
        }
        Row {
            visible: root.hasMusic
            x: 10
            y: root.musicLaneY + (root.musicLaneHeight - height) / 2
            spacing: 5
            IconImage {
                anchors.verticalCenter: parent.verticalCenter
                width: 12; height: 12
                source: "qrc:/omareel/assets/icons/lucide/music.svg"
                sourceSize: Qt.size(12, 12)
                color: theme.textFaint
            }
            Label {
                anchors.verticalCenter: parent.verticalCenter
                text: "Music"
                color: theme.textMuted
                font.pixelSize: theme.type.caption
                font.weight: Font.Medium
            }
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
                    model: OmCountModel { size: flick.visibleTickCount }
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
                            anchors.bottomMargin: 4
                            width: 1
                            height: 6
                            color: theme.strokeStrong
                        }
                        Text {
                            x: 4
                            y: 6
                            textFormat: Text.PlainText
                            text: root.formatRulerTime(tickDelegate.tickTime)
                            color: theme.textFaint
                            font.pixelSize: theme.type.caption - 1
                            font.family: theme.monoFamily
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
                // Chapters (chapters.json), at their place on the edited timeline.
                Repeater {
                    model: typeof agent === "undefined" || !agent.chapters.exists ? []
                        : agent.chapterMarkers(JSON.stringify(editor.clips))
                    delegate: Item {
                        id: chapterMarker
                        required property var modelData
                        x: modelData.time * root.pixelsPerSecond
                        width: 9
                        height: ruler.height
                        Rectangle { x: 0; width: 1.5; height: parent.height; color: theme.accent }
                        Rectangle {
                            x: 0; y: 0; width: 8; height: 7
                            radius: theme.rounded ? 1.5 : 0
                            color: theme.accent
                        }
                        HoverHandler { id: chapterHover; cursorShape: Qt.PointingHandCursor }
                        TapHandler { onTapped: editor.seek(chapterMarker.modelData.time) }
                        OmLazyToolTip { target: chapterMarker; shown: chapterHover.hovered; text: chapterMarker.modelData.title }
                    }
                }
            }

            Rectangle {
                x: 0
                y: 32
                width: Math.max(parent.width, editor.duration * root.pixelsPerSecond)
                height: 48
                radius: theme.controlRadius
                color: Qt.alpha(theme.foreground, theme.dark ? .025 : .035)
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
                width: Math.max(parent.width, editor.duration * root.pixelsPerSecond)
                height: 48
                radius: theme.controlRadius
                color: Qt.alpha(theme.foreground, theme.dark ? .025 : .035)
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
                visible: root.hasOverlays
                x: 0
                y: 144
                width: Math.max(parent.width, editor.duration * root.pixelsPerSecond)
                height: 48
                radius: theme.controlRadius
                color: Qt.alpha(theme.foreground, theme.dark ? .025 : .035)
            }
            OverlayTrack {
                objectName: "overlayTrack"
                visible: root.hasOverlays
                x: 0
                y: 148
                width: parent.width
                height: 40
                pixelsPerSecond: root.pixelsPerSecond
                focusTarget: root
            }
            Rectangle {
                id: cameraTrack
                visible: editor.hasCamera
                x: 0
                y: root.cameraLaneY
                width: parent.width
                height: 48
                radius: theme.controlRadius
                color: Qt.alpha(theme.foreground, theme.dark ? .025 : .035)
            }
            Rectangle {
                id: cameraBlock
                visible: editor.hasCamera
                x: 0
                y: root.cameraLaneY + (cameraHover.hovered ? 3 : 4)
                width: Math.max(20, editor.duration * root.pixelsPerSecond)
                height: 40
                radius: theme.controlRadius
                color: theme.fill
                border.width: 1
                border.color: cameraHover.hovered ? Qt.alpha(theme.accent, .8) : theme.stroke
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
                                color: Qt.alpha(theme.accent, .12)
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
                            color: index % 2 ? theme.fill : theme.fillHover
                        }
                    }
                }
                Rectangle {
                    anchors.left: parent.left; anchors.leftMargin: 8
                    anchors.verticalCenter: parent.verticalCenter
                    width: cameraLabel.implicitWidth + 16
                    height: 22
                    radius: theme.smallRadius
                    color: Qt.alpha(theme.canvas, .78)
                    Label {
                        id: cameraLabel
                        anchors.centerIn: parent
                        text: "Camera"
                        color: theme.textStrong
                        font.pixelSize: theme.type.caption
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
                OmLazyToolTip {
                    target: cameraBlock
                    shown: cameraHover.hovered
                    text: "Camera settings"
                }
            }

            Rectangle {
                id: musicTrack
                visible: root.hasMusic
                x: 0
                y: root.musicLaneY
                width: parent.width
                height: root.musicLaneHeight
                radius: theme.controlRadius
                color: Qt.alpha(theme.foreground, theme.dark ? .025 : .035)
            }
            Rectangle {
                id: musicBlock
                objectName: "musicBlock"
                visible: root.hasMusic
                x: 0
                y: root.musicLaneY + 3
                width: Math.max(20, (editor.music.extent || 0) * root.pixelsPerSecond)
                height: root.musicLaneHeight - 6
                radius: theme.smallRadius
                color: Qt.alpha(theme.accent, musicHover.hovered ? .26 : .16)
                border.width: 1
                border.color: musicHover.hovered ? Qt.alpha(theme.accent, .8) : Qt.alpha(theme.accent, .45)
                clip: true
                // The music's loudness across the output, once measured.
                Repeater {
                    model: root.hasMusic ? editor.music.waveform : []
                    delegate: Rectangle {
                        required property real modelData
                        required property int index
                        readonly property real step: editor.duration * root.pixelsPerSecond
                            / Math.max(1, editor.music.waveform.length)
                        x: index * step
                        width: Math.max(1, step - (step > 3 ? 1 : 0))
                        height: Math.max(1, modelData * (musicBlock.height - 6))
                        y: (musicBlock.height - height) / 2
                        color: Qt.alpha(theme.accent, .55)
                        visible: x < musicBlock.width
                    }
                }
                Rectangle {
                    anchors.left: parent.left; anchors.leftMargin: 6
                    anchors.verticalCenter: parent.verticalCenter
                    width: Math.min(musicLabel.implicitWidth + 12, musicBlock.width - 12)
                    height: 16
                    radius: theme.smallRadius
                    color: Qt.alpha(theme.canvas, .78)
                    visible: width > 20
                    Label {
                        id: musicLabel
                        anchors.fill: parent
                        anchors.leftMargin: 6; anchors.rightMargin: 6
                        verticalAlignment: Text.AlignVCenter
                        textFormat: Text.PlainText
                        text: editor.music.name || ""
                        elide: Text.ElideRight
                        color: theme.textStrong
                        font.pixelSize: theme.type.caption
                        font.weight: Font.DemiBold
                    }
                }
                HoverHandler { id: musicHover }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onPressed: root.forceActiveFocus()
                    onClicked: root.musicSelected()
                }
                OmLazyToolTip {
                    target: musicBlock
                    shown: musicHover.hovered
                    text: "Music settings"
                }
            }

            Rectangle {
                id: playhead
                x: editor.playheadPosition * root.pixelsPerSecond - width / 2
                y: 22
                width: playheadDrag.pressed ? 2 : 1.5
                height: root.tracksBottom - y + 4
                color: theme.record
                z: 20
                Behavior on width {
                    enabled: !editor.loading
                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
                // Tag-shaped head: a small block with a point that sits on the ruler.
                Item {
                    id: playheadHead
                    readonly property real corner: theme.rounded ? 2.5 : 0
                    x: -width / 2 + parent.width / 2
                    y: -6
                    width: 11
                    height: 14
                    scale: playheadDrag.pressed ? 1.2 : playheadDrag.containsMouse ? 1.12 : 1
                    transformOrigin: Item.Bottom
                    Behavior on scale {
                        enabled: !editor.loading
                        NumberAnimation { duration: 180; easing.type: Easing.OutBack; easing.overshoot: 2 }
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: parent.width + 12
                        height: parent.height + 10
                        radius: theme.rounded ? 8 : 0
                        color: Qt.alpha(theme.record, .25)
                        opacity: playheadDrag.pressed ? 1 : 0
                        Behavior on opacity {
                            enabled: !editor.loading
                            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                        }
                    }
                    Shape {
                        anchors.fill: parent
                        preferredRendererType: Shape.CurveRenderer
                        ShapePath {
                            strokeWidth: 0
                            fillColor: theme.record
                            startX: playheadHead.corner; startY: 0
                            PathLine { x: 11 - playheadHead.corner; y: 0 }
                            PathQuad { x: 11; y: playheadHead.corner; controlX: 11; controlY: 0 }
                            PathLine { x: 11; y: 9 }
                            PathLine { x: 5.5; y: 14 }
                            PathLine { x: 0; y: 9 }
                            PathLine { x: 0; y: playheadHead.corner }
                            PathQuad { x: playheadHead.corner; y: 0; controlX: 0; controlY: 0 }
                        }
                    }
                }
                MouseArea {
                    id: playheadDrag
                    x: -10; y: -8
                    width: 21; height: 22
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
                }
                Rectangle {
                    id: playheadTime
                    x: 10
                    y: -9
                    width: playheadLabel.implicitWidth + 12
                    height: 18
                    radius: theme.smallRadius
                    color: theme.record
                    opacity: playheadDrag.pressed || playheadDrag.containsMouse ? 1 : 0
                    visible: opacity > 0
                    Behavior on opacity { NumberAnimation { duration: 140 } }
                    Label {
                        id: playheadLabel
                        anchors.centerIn: parent
                        textFormat: Text.PlainText
                        text: editor.formatTime(editor.position)
                        color: "white"
                        font.family: theme.monoFamily
                        font.pixelSize: theme.type.caption
                        font.weight: Font.Medium
                    }
                }
            }
        }

        ScrollBar.horizontal: ScrollBar {
            id: horizontalBar
            policy: flick.contentWidth > flick.width + 0.5
                ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
            interactive: true
            height: 12
            topPadding: 3
            bottomPadding: 3
            leftPadding: 4
            rightPadding: 4
            minimumSize: Math.min(1, 32 / Math.max(1, width))
            opacity: 1
            background: Item {}
            contentItem: Rectangle {
                implicitWidth: 32
                implicitHeight: horizontalBar.hovered || horizontalBar.pressed ? 6 : 4
                y: (horizontalBar.height - height) / 2
                radius: theme.rounded ? height / 2 : 0
                color: horizontalBar.pressed ? theme.textMuted
                     : horizontalBar.hovered ? Qt.alpha(theme.foreground, .35) : Qt.alpha(theme.foreground, .18)
                Behavior on implicitHeight { NumberAnimation { duration: 120 } }
                Behavior on color {
                    enabled: !editor.loading
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }
        }
    }
    // While the coding agent edits (Editor.readOnly), the timeline only shows its changes.
    Rectangle {
        objectName: "agentEditingOverlay"
        anchors.fill: parent
        visible: editor.readOnly
        color: Qt.alpha(theme.surface, .55)
        // Clicks and drags stop here; the wheel still scrolls and zooms the timeline.
        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.AllButtons
            onWheel: wheel => wheel.accepted = false
        }
        Rectangle {
            anchors.centerIn: parent
            width: lockRow.implicitWidth + 24
            height: lockRow.implicitHeight + 14
            radius: theme.controlRadius
            color: theme.raised
            border.width: 1
            border.color: theme.strokeStrong
            RowLayout {
                id: lockRow
                anchors.centerIn: parent
                spacing: 10
                Label {
                    textFormat: Text.PlainText
                    text: "The agent is editing… Stop to edit"
                    color: theme.textStrong
                    font.pixelSize: theme.type.small
                }
                OmButton {
                    visible: typeof agent !== "undefined"
                    bordered: true
                    destructive: true
                    text: "Stop"
                    icon.source: "qrc:/omareel/assets/icons/lucide/square.svg"
                    onClicked: agent.stop()
                }
            }
        }
    }
}
