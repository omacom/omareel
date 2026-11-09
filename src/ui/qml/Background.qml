import QtQuick
import QtQuick.Effects

Item {
    id: root
    required property var settings
    property bool softwareRendering: false
    // The editor's preview decodes the image off the GUI thread (a wallpaper is 70 ms of WebP
    // that held up the first frame) and shows the colour until it is ready; an export draws
    // each frame complete, so it waits for it.
    property bool loadAsynchronously: false

    Rectangle {
        anchors.fill: parent
        color: root.settings.color || Qt.rgba(26/255, 27/255, 38/255, 1)
        visible: root.settings.type !== "none"
                 && (root.settings.type === "color" || !backgroundImage.visible || backgroundImage.status !== Image.Ready)
    }
    Canvas {
        id: gradientCanvas
        anchors.fill: parent
        visible: root.settings.type === "gradient"
        // Painted only while shown: a hidden Canvas still repaints on request, a whole-frame
        // QPainter fill and image copy (an export asked for one every frame).
        onWidthChanged: if (visible) requestPaint()
        onHeightChanged: if (visible) requestPaint()
        onVisibleChanged: if (visible) requestPaint()
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
            function onSettingsChanged() { if (gradientCanvas.visible) gradientCanvas.requestPaint() }
        }
    }
    Image {
        id: backgroundImage
        anchors.fill: parent
        visible: (root.settings.type === "image" || root.settings.type === "wallpaper") && source.toString() !== ""
        // Unsized, sourceSize would be 0x0, which loads the whole image.
        source: width > 0 && height > 0 ? root.settings.resolvedImage || "" : ""
        fillMode: Image.PreserveAspectCrop
        // Decode at the size drawn (in 256 px steps, so resizing the window rarely reloads it),
        // not the wallpaper's own: a 5K WebP is 80 MB of pixels.
        sourceSize: Qt.size(Math.ceil(width * Screen.devicePixelRatio / 256) * 256,
                            Math.ceil(height * Screen.devicePixelRatio / 256) * 256)
        asynchronous: root.loadAsynchronously
        layer.enabled: root.settings.blur > 0 && !root.softwareRendering
        layer.effect: MultiEffect {
            blurEnabled: true
            blur: Math.min(1, root.settings.blur / 100)
            blurMax: 64
        }
    }
}
