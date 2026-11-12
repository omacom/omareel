import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import Omareel.Ui

// The Overlays lane: one block per annotation where it plays on the output timeline, stacked in
// rows where they overlap. Drag a block to move it in time, its edges to trim it; each gesture is
// one undo step. Screen-anchored items keep source times, so a drag converts back through the
// clip under it.
Item {
    id: root
    required property real pixelsPerSecond
    required property Item focusTarget
    readonly property var kindIcons: ({text: "type.svg", box: "square-dashed.svg", arrow: "arrow-up-right.svg",
                                       badge: "hash.svg", blur: "eye-off.svg", card: "presentation.svg"})
    // Rows: each item takes the first row whose last item ended before it starts.
    readonly property var layout: {
        const order = editor.overlays.map((item, index) => ({index: index, start: item.outputStart, end: item.outputEnd}))
            .filter(entry => entry.start >= 0 && entry.end > entry.start)
            .sort((a, b) => a.start - b.start)
        const rowEnds = []
        const rows = {}
        for (const entry of order) {
            let row = rowEnds.findIndex(end => end <= entry.start + 1e-6)
            if (row < 0) { row = rowEnds.length; rowEnds.push(0) }
            rowEnds[row] = entry.end
            rows[entry.index] = row
        }
        return {rows: rows, count: Math.max(1, Math.min(3, rowEnds.length))}
    }
    function label(item) {
        if (item.kind === "text") return item.text || "Text"
        if (item.kind === "card") return item.title || "Card"
        if (item.kind === "badge") return "#" + item.number
        if (item.kind === "blur") return "Blur"
        if (item.kind === "box") return item.spotlight > 0 ? "Spotlight" : "Box"
        return "Arrow"
    }
    // An output time as the item's own time base.
    function itemTime(item, outputTime) {
        return item.anchor === "screen" ? editor.outputToSource(outputTime) : outputTime
    }

    MouseArea {
        anchors.fill: parent
        onPressed: { root.focusTarget.forceActiveFocus(); editor.selectedOverlayId = "" }
    }

    Repeater {
        model: editor.overlays
        delegate: Rectangle {
            id: block
            objectName: "overlayBlock-" + modelData.id
            required property var modelData
            required property int index
            readonly property int row: root.layout.rows[index] !== undefined ? Math.min(root.layout.rows[index], root.layout.count - 1) : 0
            readonly property bool selected: editor.selectedOverlayId === modelData.id
            readonly property color tint: modelData.kind === "blur" ? theme.record : theme.accent
            property bool gestureActive: false
            property real gestureX: 0
            property real gestureWidth: 0
            visible: modelData.outputStart >= 0 && modelData.outputEnd > modelData.outputStart
            x: gestureActive ? gestureX : Math.max(0, modelData.outputStart) * root.pixelsPerSecond
            width: gestureActive ? gestureWidth : Math.max(14, (modelData.outputEnd - modelData.outputStart) * root.pixelsPerSecond)
            height: (root.height - (root.layout.count - 1) * 2) / root.layout.count
            y: row * (height + 2)
            z: selected ? 2 : 1
            radius: theme.controlRadius
            color: Qt.alpha(tint, selected ? .32 : blockHover.hovered ? .24 : .18)
            border.width: selected ? 1.5 : 1
            border.color: selected ? tint : Qt.alpha(tint, blockHover.hovered ? .7 : .42)
            HoverHandler { id: blockHover }
            Row {
                anchors.verticalCenter: parent.verticalCenter
                x: 8
                width: parent.width - 16
                spacing: 5
                clip: true
                visible: block.width >= 30
                IconImage {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 12; height: 12
                    source: "qrc:/omareel/assets/icons/lucide/" + root.kindIcons[block.modelData.kind]
                    sourceSize: Qt.size(12, 12)
                    color: block.tint
                }
                Label {
                    anchors.verticalCenter: parent.verticalCenter
                    visible: block.height >= 16
                    width: Math.max(0, block.width - 36)
                    elide: Text.ElideRight
                    text: root.label(block.modelData)
                    color: theme.textStrong
                    font.pixelSize: theme.type.small
                }
            }
            MouseArea {
                objectName: "overlayBody-" + block.modelData.id
                anchors.fill: parent
                anchors.leftMargin: 8; anchors.rightMargin: 8
                cursorShape: pressed ? Qt.ClosedHandCursor : Qt.OpenHandCursor
                property real pressTrackX
                property real pressBlockX
                property bool moved: false
                onPressed: mouse => {
                    root.focusTarget.forceActiveFocus()
                    editor.selectedOverlayId = block.modelData.id
                    pressTrackX = mapToItem(root, mouse.x, mouse.y).x
                    pressBlockX = block.x
                    block.gestureX = block.x
                    block.gestureWidth = block.width
                    block.gestureActive = true
                    moved = false
                }
                onPositionChanged: mouse => {
                    if (!pressed) return
                    const trackX = mapToItem(root, mouse.x, mouse.y).x
                    moved = moved || Math.abs(trackX - pressTrackX) >= 2
                    block.gestureX = Math.max(0, Math.min(editor.duration * root.pixelsPerSecond - block.width,
                                                          pressBlockX + trackX - pressTrackX))
                }
                onReleased: {
                    const item = block.modelData
                    block.gestureActive = false
                    if (moved) {
                        const start = root.itemTime(item, block.gestureX / root.pixelsPerSecond)
                        editor.updateOverlay(item.id, {start: start, end: start + (item.end - item.start)}, false)
                    } else if (editor.position < item.outputStart || editor.position >= item.outputEnd) {
                        // A click brings the playhead into the item, where the preview shows it.
                        editor.seek(item.outputStart)
                    }
                }
                onCanceled: block.gestureActive = false
            }
            OmTrimHandle {
                objectName: "overlayTrimHandle-" + block.modelData.id + "-left"
                edge: "left"
                accent: block.tint
                active: block.selected || blockHover.hovered
                dragging: mouseArea.pressed
                property real pressTrackX
                property real pressBlockX
                property real pressRight
                mouseArea.onPressed: mouse => {
                    root.focusTarget.forceActiveFocus()
                    editor.selectedOverlayId = block.modelData.id
                    pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    pressBlockX = block.x
                    pressRight = block.x + block.width
                    block.gestureX = block.x; block.gestureWidth = block.width
                    block.gestureActive = true
                }
                mouseArea.onPositionChanged: mouse => {
                    if (!mouseArea.pressed) return
                    const trackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    const nextX = Math.max(0, Math.min(pressRight - root.pixelsPerSecond * .1, pressBlockX + trackX - pressTrackX))
                    block.gestureX = nextX; block.gestureWidth = pressRight - nextX
                }
                mouseArea.onReleased: {
                    block.gestureActive = false
                    editor.updateOverlay(block.modelData.id, {start: root.itemTime(block.modelData, block.gestureX / root.pixelsPerSecond)}, false)
                }
                mouseArea.onCanceled: block.gestureActive = false
            }
            OmTrimHandle {
                objectName: "overlayTrimHandle-" + block.modelData.id + "-right"
                x: parent.width - width
                edge: "right"
                accent: block.tint
                active: block.selected || blockHover.hovered
                dragging: mouseArea.pressed
                property real pressTrackX
                property real pressWidth
                mouseArea.onPressed: mouse => {
                    root.focusTarget.forceActiveFocus()
                    editor.selectedOverlayId = block.modelData.id
                    pressTrackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    pressWidth = block.width
                    block.gestureX = block.x; block.gestureWidth = block.width
                    block.gestureActive = true
                }
                mouseArea.onPositionChanged: mouse => {
                    if (!mouseArea.pressed) return
                    const trackX = mouseArea.mapToItem(root, mouse.x, mouse.y).x
                    block.gestureWidth = Math.max(root.pixelsPerSecond * .1,
                        Math.min(editor.duration * root.pixelsPerSecond - block.gestureX, pressWidth + trackX - pressTrackX))
                }
                mouseArea.onReleased: {
                    block.gestureActive = false
                    const item = block.modelData
                    // Just before the end, so the source time is the clip's, not the next one's.
                    const endOutput = (block.gestureX + block.gestureWidth) / root.pixelsPerSecond
                    const end = item.anchor === "screen" ? editor.outputToSource(Math.max(0, endOutput - 1e-4)) + 1e-4 : endOutput
                    editor.updateOverlay(item.id, {end: end}, false)
                }
                mouseArea.onCanceled: block.gestureActive = false
            }
        }
    }
}
