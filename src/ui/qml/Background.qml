import QtQuick
import QtQuick.Effects

Item {
    id: root
    required property var settings
    property bool softwareRendering: false

    Rectangle {
        anchors.fill: parent
        color: root.settings.color || "#1a1b26"
        visible: root.settings.type !== "none" && (root.settings.type === "color" || !backgroundImage.visible)
    }
    Canvas {
        id: gradientCanvas
        anchors.fill: parent
        visible: root.settings.type === "gradient"
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onPaint: {
            const ctx = getContext("2d")
            ctx.reset()
            const angle = (root.settings.gradient.angle - 90) * Math.PI / 180
            const radius = Math.sqrt(width * width + height * height) / 2
            const cx = width / 2
            const cy = height / 2
            const dx = Math.cos(angle) * radius
            const dy = Math.sin(angle) * radius
            const gradient = ctx.createLinearGradient(cx - dx, cy - dy, cx + dx, cy + dy)
            const stops = root.settings.gradient.stops
            for (let i = 0; i < stops.length; ++i)
                gradient.addColorStop(stops[i][1], stops[i][0])
            ctx.fillStyle = gradient
            ctx.fillRect(0, 0, width, height)
        }
        Connections {
            target: root
            function onSettingsChanged() { gradientCanvas.requestPaint() }
        }
    }
    Image {
        id: backgroundImage
        anchors.fill: parent
        visible: (root.settings.type === "image" || root.settings.type === "wallpaper") && source.toString() !== ""
        source: root.settings.resolvedImage || ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: false
        layer.enabled: root.settings.blur > 0 && !root.softwareRendering
        layer.effect: MultiEffect {
            blurEnabled: true
            blur: Math.min(1, root.settings.blur / 100)
            blurMax: 64
        }
    }
}
