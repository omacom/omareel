import QtQuick
import QtQuick.Effects
import QtQuick.Shapes

// Annotations (project.json `overlays`) over one space: the recording's frame for
// screen-anchored items (inside the zoomed stage, so they follow zooms) or the canvas for
// frame-anchored ones. `items` are the overlays showing now, each with its `opacity`
// (editor.visibleOverlays or the exporter's). Positions are 0–1; `space` is the part of that
// 0–1 space this item shows (the crop for the recording, all of it for the canvas). Sizes are
// pixels at 1080 high, times `ref`.
Item {
    id: root
    required property var items
    property var space: ({x: 0, y: 0, w: 1, h: 1})
    property real ref: 1
    // renderTheme: {background, foreground, accent}.
    property var colors: ({background: "#1a1b26", foreground: "#a9b1d6", accent: "#7aa2f7"})
    property var backgroundSettings: null
    property bool softwareRendering: false
    // What blur regions copy (the recording's untransformed frame), and whether the export has
    // already baked them into the recording's pixels (then they aren't drawn again).
    property Item blurSource: null
    property bool redactionBaked: false
    property bool nativePreview: false

    function px(x) { return (x - space.x) / Math.max(.0001, space.w) * width }
    function py(y) { return (y - space.y) / Math.max(.0001, space.h) * height }
    function pw(w) { return w / Math.max(.0001, space.w) * width }
    function ph(h) { return h / Math.max(.0001, space.h) * height }
    function colorOf(item, fallback) { return item.color ? item.color : fallback }

    component Placed: Item {
        required property var modelData
        // Inline components don't see this file's ids: the layer comes in as `host` (not `layer`, which every Item has).
        required property Item host
        x: host.px(modelData.rect.x)
        y: host.py(modelData.rect.y)
        width: host.pw(modelData.rect.w)
        height: host.ph(modelData.rect.h)
        opacity: modelData.opacity
    }

    component TextOverlay: Placed {
        id: textItem
        readonly property bool pill: modelData.background !== "none"
        Rectangle {
            anchors.centerIn: label
            width: label.contentWidth + 36 * host.ref
            height: label.contentHeight + 18 * host.ref
            radius: Math.min(height / 2, 18 * host.ref)
            visible: textItem.pill
            color: Qt.alpha(host.colors.background, .88)
            border.width: host.ref
            border.color: Qt.alpha(host.colors.foreground, .14)
        }
        Text {
            id: label
            anchors.centerIn: parent
            width: Math.min(implicitWidth, parent.width)
            text: textItem.modelData.text
            color: host.colorOf(textItem.modelData, textItem.pill ? host.colors.foreground : "white")
            font.pixelSize: textItem.modelData.size * host.ref
            font.weight: Font.DemiBold
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.Wrap
            style: textItem.pill ? Text.Normal : Text.Outline
            styleColor: Qt.alpha("black", .55)
        }
    }

    component BoxOverlay: Placed {
        id: boxItem
        readonly property real stroke: modelData.width * host.ref
        readonly property real corner: modelData.radius * host.ref
        // The spotlight: everything outside the box dimmed, by one huge border around it.
        Rectangle {
            readonly property real reach: 8000
            visible: boxItem.modelData.spotlight > 0
            x: -reach; y: -reach
            width: parent.width + 2 * reach
            height: parent.height + 2 * reach
            radius: boxItem.corner + reach
            color: "transparent"
            border.width: reach
            border.color: Qt.rgba(0, 0, 0, .75 * boxItem.modelData.spotlight)
        }
        Rectangle {
            x: -boxItem.stroke / 2; y: -boxItem.stroke / 2
            width: parent.width + boxItem.stroke
            height: parent.height + boxItem.stroke
            radius: boxItem.corner + boxItem.stroke / 2
            color: "transparent"
            border.width: boxItem.stroke
            border.color: host.colorOf(boxItem.modelData, host.colors.accent)
            antialiasing: true
        }
    }

    component ArrowOverlay: Item {
        id: arrowItem
        required property var modelData
        required property Item host
        readonly property real x1: host.px(modelData.from.x)
        readonly property real y1: host.py(modelData.from.y)
        readonly property real x2: host.px(modelData.to.x)
        readonly property real y2: host.py(modelData.to.y)
        readonly property real stroke: modelData.width * host.ref
        readonly property real angle: Math.atan2(y2 - y1, x2 - x1)
        readonly property real head: Math.max(14 * host.ref, stroke * 3.2)
        readonly property color tint: host.colorOf(modelData, host.colors.accent)
        opacity: modelData.opacity
        // The shaft stops where the head starts, so the round cap never pokes out of its tip.
        Rectangle {
            readonly property real length: Math.max(0, Math.hypot(arrowItem.x2 - arrowItem.x1, arrowItem.y2 - arrowItem.y1) - arrowItem.head * .8)
            x: arrowItem.x1
            y: arrowItem.y1 - height / 2
            width: length
            height: arrowItem.stroke
            radius: height / 2
            color: arrowItem.tint
            antialiasing: true
            transformOrigin: Item.Left
            rotation: arrowItem.angle * 180 / Math.PI
        }
        Shape {
            x: arrowItem.x2; y: arrowItem.y2
            rotation: arrowItem.angle * 180 / Math.PI
            transformOrigin: Item.TopLeft
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeWidth: -1
                fillColor: arrowItem.tint
                startX: 0; startY: 0
                PathLine { x: -arrowItem.head; y: -arrowItem.head * .6 }
                PathLine { x: -arrowItem.head * .78; y: 0 }
                PathLine { x: -arrowItem.head; y: arrowItem.head * .6 }
                PathLine { x: 0; y: 0 }
            }
        }
    }

    component BadgeOverlay: Placed {
        id: badgeItem
        Rectangle {
            anchors.centerIn: parent
            width: Math.min(parent.width, parent.height)
            height: width
            radius: width / 2
            color: host.colorOf(badgeItem.modelData, host.colors.accent)
            border.width: Math.max(1, width * .06)
            border.color: Qt.alpha("white", .9)
            antialiasing: true
            Text {
                anchors.centerIn: parent
                text: badgeItem.modelData.number
                color: "white"
                font.pixelSize: parent.width * .52
                font.weight: Font.Bold
            }
        }
    }

    // The preview's blur region (the export bakes it into the recording instead). An opaque
    // fill sits under the copy, so if the copy ever fails the region shows a plain block, never
    // what it hides.
    component BlurOverlay: Placed {
        id: blurItem
        // The cell size as a fraction of the recording's height, from the overlay track
        // (crates/omareel_core/src/export/redact.rs, block_fraction), so the export's grid
        // is this one.
        readonly property real block: host.height / Math.max(.0001, host.space.h)
            * Math.max(0.04, modelData.block || 0)
        readonly property size cells: Qt.size(Math.max(1, Math.round(width / Math.max(1, block))),
                                              Math.max(1, Math.round(height / Math.max(1, block))))
        readonly property bool blurStyle: modelData.style === "blur"
        clip: true
        Rectangle { anchors.fill: parent; color: "#3a3b45" }
        ShaderEffectSource {
            id: capture
            anchors.fill: parent
            sourceItem: host.blurSource
            sourceRect: Qt.rect(blurItem.x, blurItem.y, blurItem.width, blurItem.height)
            live: true
            hideSource: false
            // On the GPU the shader (or the blur) reads it; the software renderer shows it
            // itself, a cell a texel.
            visible: host.softwareRendering
            smooth: false
            mipmap: !host.softwareRendering && !blurItem.blurStyle
            textureSize: host.softwareRendering ? blurItem.cells
                : blurItem.blurStyle ? Qt.size(blurItem.cells.width * 4, blurItem.cells.height * 4)
                : Qt.size(Math.max(1, blurItem.width), Math.max(1, blurItem.height))
        }
        ShaderEffect {
            anchors.fill: parent
            visible: !host.softwareRendering && !blurItem.blurStyle
            property variant source: capture
            property size cells: blurItem.cells
            fragmentShader: "qrc:/omareel/shaders/pixelate.frag.qsb"
        }
        MultiEffect {
            anchors.fill: parent
            visible: !host.softwareRendering && blurItem.blurStyle
            source: capture
            autoPaddingEnabled: false
            blurEnabled: true
            blur: 1
            blurMax: 24
        }
    }

    component CardOverlay: Item {
        id: cardItem
        required property var modelData
        required property Item host
        width: host.width
        height: host.height
        opacity: modelData.opacity
        Background {
            anchors.fill: parent
            settings: host.backgroundSettings
            softwareRendering: host.softwareRendering
            loadAsynchronously: host.nativePreview
        }
        Rectangle {
            anchors.centerIn: parent
            width: Math.min(parent.width * .8, Math.max(title.implicitWidth, subtitle.implicitWidth) + 120 * host.ref)
            height: column.implicitHeight + 88 * host.ref
            radius: 28 * host.ref
            color: Qt.alpha(host.colors.background, .9)
            border.width: host.ref
            border.color: Qt.alpha(host.colors.foreground, .12)
            Column {
                id: column
                anchors.centerIn: parent
                width: parent.width - 80 * host.ref
                spacing: 18 * host.ref
                Text {
                    id: title
                    width: parent.width
                    text: cardItem.modelData.title
                    color: host.colors.foreground
                    font.pixelSize: 84 * host.ref
                    font.weight: Font.Bold
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                }
                Rectangle {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: 72 * host.ref; height: 6 * host.ref; radius: height / 2
                    color: host.colors.accent
                }
                Text {
                    id: subtitle
                    width: parent.width
                    visible: text.length > 0
                    text: cardItem.modelData.subtitle
                    color: Qt.alpha(host.colors.foreground, .72)
                    font.pixelSize: 38 * host.ref
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    // By count, not by list: the list is new every frame of playback, and delegates made again
    // each time would rebuild their text and the blur's copy every frame.
    Repeater {
        model: root.items ? root.items.length : 0
        delegate: Loader {
            id: overlayLoader
            required property int index
            readonly property var modelData: root.items[index] || ({})
            readonly property string kind: modelData.kind || ""
            sourceComponent: kind === "text" ? textComponent
                : kind === "box" ? boxComponent
                : kind === "arrow" ? arrowComponent
                : kind === "badge" ? badgeComponent
                : kind === "blur" && !root.redactionBaked ? blurComponent
                : kind === "card" ? cardComponent : null
            Component { id: textComponent; TextOverlay { modelData: overlayLoader.modelData; host: root } }
            Component { id: boxComponent; BoxOverlay { modelData: overlayLoader.modelData; host: root } }
            Component { id: arrowComponent; ArrowOverlay { modelData: overlayLoader.modelData; host: root } }
            Component { id: badgeComponent; BadgeOverlay { modelData: overlayLoader.modelData; host: root } }
            Component { id: blurComponent; BlurOverlay { modelData: overlayLoader.modelData; host: root } }
            Component { id: cardComponent; CardOverlay { modelData: overlayLoader.modelData; host: root } }
        }
    }
}
