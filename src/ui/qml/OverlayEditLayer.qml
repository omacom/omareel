import QtQuick

// Editing annotations on the preview: click one to select it, drag it to move it, drag its
// corner handles to resize it (an arrow's two ends to re-aim it). Each drag is one undo step.
// Lives in the preview's canvas-sized item, over the composition, and maps the overlays' 0–1
// places the way Composition.qml draws them (screen-anchored ones through the crop and zoom).
Item {
    id: root
    // The composition this edits over, for its frame geometry.
    required property Item composition
    // Canvas pixels per screen pixel, to keep handles a constant size on screen.
    property real viewScale: 1
    // Takes the keyboard focus when an overlay is pressed, so Delete and the keys work.
    property Item focusTarget: null
    readonly property real handleSize: 14 / Math.max(.0001, viewScale)
    // What the frame shows (the crop, or the reframe window), and the zoom and a transition's
    // punch as the composition applies them.
    readonly property var crop: composition.viewCrop
    readonly property real frameX: (width - composition.videoWidth) / 2
    readonly property real frameY: (height - composition.videoHeight) / 2
    readonly property real zoomScale: composition.zoomScale
    readonly property real originX: frameX + composition.zoomOrigin.x * composition.videoWidth
    readonly property real originY: frameY + composition.zoomOrigin.y * composition.videoHeight
    readonly property real cutScale: composition.cutScale
    readonly property var selected: {
        const items = editor.visibleOverlays
        for (let i = 0; i < items.length; ++i)
            if (items[i].id === editor.selectedOverlayId) return items[i]
        return null
    }

    // An overlay's 0–1 point on the canvas, in pixels.
    function toCanvas(item, x, y) {
        if (item.anchor !== "screen") return Qt.point(x * width, y * height)
        const bx = frameX + (x - crop.x) / Math.max(.0001, crop.w) * composition.videoWidth
        const by = frameY + (y - crop.y) / Math.max(.0001, crop.h) * composition.videoHeight
        const zx = originX + zoomScale * (bx - originX), zy = originY + zoomScale * (by - originY)
        return Qt.point(width / 2 + cutScale * (zx - width / 2), height / 2 + cutScale * (zy - height / 2))
    }
    // 0–1 units per canvas pixel, across and down.
    function unitsPerPixel(item) {
        if (item.anchor !== "screen") return Qt.point(1 / width, 1 / height)
        return Qt.point(crop.w / Math.max(.0001, cutScale * zoomScale * composition.videoWidth),
                        crop.h / Math.max(.0001, cutScale * zoomScale * composition.videoHeight))
    }
    function bounds(item) {
        if (item.kind === "card") return Qt.rect(0, 0, width, height)
        if (item.kind === "arrow") {
            const a = toCanvas(item, item.from.x, item.from.y), b = toCanvas(item, item.to.x, item.to.y)
            const pad = 12 / Math.max(.0001, viewScale)
            return Qt.rect(Math.min(a.x, b.x) - pad, Math.min(a.y, b.y) - pad,
                           Math.abs(b.x - a.x) + 2 * pad, Math.abs(b.y - a.y) + 2 * pad)
        }
        const a = toCanvas(item, item.rect.x, item.rect.y)
        const b = toCanvas(item, item.rect.x + item.rect.w, item.rect.y + item.rect.h)
        return Qt.rect(a.x, a.y, b.x - a.x, b.y - a.y)
    }
    function copy(value) { return JSON.parse(JSON.stringify(value)) }

    // What a drag started from, and the patch it makes for a pointer moved by (dx, dy) pixels.
    QtObject {
        id: drag
        property var item: null
        property string handle: ""
        property point press
        property var start: null
        function begin(item, handle, point) {
            drag.item = item
            drag.handle = handle
            drag.press = point
            drag.start = {rect: root.copy(item.rect || {}), from: root.copy(item.from || {}), to: root.copy(item.to || {})}
            editor.selectedOverlayId = item.id
            if (root.focusTarget) root.focusTarget.forceActiveFocus()
            editor.beginCoalescedEdit("overlay-drag-" + item.id)
        }
        function move(point) {
            if (!drag.item) return
            const units = root.unitsPerPixel(drag.item)
            const dx = (point.x - drag.press.x) * units.x
            const dy = (point.y - drag.press.y) * units.y
            const r = drag.start.rect
            let patch
            if (drag.handle === "move" && drag.item.kind === "arrow") {
                patch = {from: {x: drag.start.from.x + dx, y: drag.start.from.y + dy},
                         to: {x: drag.start.to.x + dx, y: drag.start.to.y + dy}}
            } else if (drag.handle === "move") {
                patch = {rect: {x: Math.max(0, Math.min(1 - r.w, r.x + dx)), y: Math.max(0, Math.min(1 - r.h, r.y + dy))}}
            } else if (drag.handle === "from" || drag.handle === "to") {
                const p = drag.start[drag.handle]
                patch = {}
                patch[drag.handle] = {x: p.x + dx, y: p.y + dy}
            } else {
                // A corner: "tl", "tr", "bl" or "br".
                let left = r.x, top = r.y, right = r.x + r.w, bottom = r.y + r.h
                if (drag.handle[1] === "l") left = Math.min(right - .02, left + dx)
                else right = Math.max(left + .02, right + dx)
                if (drag.handle[0] === "t") top = Math.min(bottom - .02, top + dy)
                else bottom = Math.max(top + .02, bottom + dy)
                left = Math.max(0, left); top = Math.max(0, top)
                right = Math.min(1, right); bottom = Math.min(1, bottom)
                patch = {rect: {x: left, y: top, w: right - left, h: bottom - top}}
            }
            editor.updateOverlay(drag.item.id, patch, true)
        }
        function end() {
            if (!drag.item) return
            drag.item = null
            editor.endCoalescedEdit()
        }
    }

    // The overlay under a point: the topmost showing (later ones draw on top), cards last.
    function hitAt(x, y) {
        const items = editor.visibleOverlays
        let card = null
        for (let i = items.length - 1; i >= 0; --i) {
            const area = bounds(items[i])
            if (x < area.x || y < area.y || x > area.x + area.width || y > area.y + area.height) continue
            if (items[i].kind !== "card") return items[i]
            card = card || items[i]
        }
        return card
    }

    // One area for all of them, so a drag survives the overlay list being remade as it moves.
    // A press on no overlay passes through (to the crop handles, or the preview, which
    // clears the selection).
    MouseArea {
        id: hits
        objectName: "overlayHits"
        anchors.fill: parent
        hoverEnabled: true
        property bool over: false
        cursorShape: pressed ? Qt.ClosedHandCursor : over ? Qt.OpenHandCursor : Qt.ArrowCursor
        onPositionChanged: mouse => {
            if (pressed) drag.move(Qt.point(mouse.x, mouse.y))
            else over = root.hitAt(mouse.x, mouse.y) !== null
        }
        onPressed: mouse => {
            const item = root.hitAt(mouse.x, mouse.y)
            if (!item) { mouse.accepted = false; return }
            drag.begin(item, "move", Qt.point(mouse.x, mouse.y))
        }
        onReleased: drag.end()
        onCanceled: drag.end()
    }

    // The selection: its outline and handles.
    Item {
        id: selection
        z: 1000
        visible: root.selected !== null && root.selected.kind !== "card"
        anchors.fill: parent
        readonly property rect area: root.selected ? root.bounds(root.selected) : Qt.rect(0, 0, 0, 0)
        readonly property bool isArrow: root.selected !== null && root.selected.kind === "arrow"
        Rectangle {
            visible: !selection.isArrow
            x: selection.area.x; y: selection.area.y
            width: selection.area.width; height: selection.area.height
            color: "transparent"
            border.width: 1.5 / Math.max(.0001, root.viewScale)
            border.color: theme.accent
        }
        Repeater {
            model: selection.isArrow ? ["from", "to"] : ["tl", "tr", "bl", "br"]
            delegate: Rectangle {
                id: handle
                required property string modelData
                objectName: "overlayHandle-" + modelData
                readonly property point at: {
                    const item = root.selected
                    if (!item) return Qt.point(0, 0)
                    if (modelData === "from" || modelData === "to")
                        return root.toCanvas(item, item[modelData].x, item[modelData].y)
                    const a = selection.area
                    return Qt.point(modelData[1] === "l" ? a.x : a.x + a.width, modelData[0] === "t" ? a.y : a.y + a.height)
                }
                x: at.x - width / 2; y: at.y - height / 2
                width: root.handleSize; height: width
                radius: modelData === "from" || modelData === "to" ? width / 2 : theme.smallRadius / Math.max(.0001, root.viewScale)
                color: theme.accent
                border.width: 1 / Math.max(.0001, root.viewScale)
                border.color: theme.accentForeground
                MouseArea {
                    anchors.fill: parent
                    anchors.margins: -4 / Math.max(.0001, root.viewScale)
                    cursorShape: handle.modelData === "tl" || handle.modelData === "br" ? Qt.SizeFDiagCursor
                        : handle.modelData === "tr" || handle.modelData === "bl" ? Qt.SizeBDiagCursor : Qt.CrossCursor
                    onPressed: mouse => drag.begin(root.selected, handle.modelData, mapToItem(root, mouse.x, mouse.y))
                    onPositionChanged: mouse => { if (pressed) drag.move(mapToItem(root, mouse.x, mouse.y)) }
                    onReleased: drag.end()
                    onCanceled: drag.end()
                }
            }
        }
    }
}
