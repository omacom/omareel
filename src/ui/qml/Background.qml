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
    // The gradient is a scene-graph Rectangle, drawn in the frame that shows it. A Canvas
    // painted a frame late, so an export's first frame (and the preview's) had no gradient.
    // The canvas drew a linear gradient along the angle across the diagonal, centred: the same
    // as a vertical gradient on a square the diagonal wide, turned about the centre and clipped.
    Item {
        id: gradientLayer
        anchors.fill: parent
        clip: true
        visible: root.settings.type === "gradient"

        Rectangle {
            readonly property real diagonal: Math.ceil(Math.sqrt(gradientLayer.width * gradientLayer.width
                                                                 + gradientLayer.height * gradientLayer.height))
            width: diagonal
            height: diagonal
            x: (gradientLayer.width - width) / 2
            y: (gradientLayer.height - height) / 2
            // A vertical gradient runs downwards, which is the canvas's angle 180.
            rotation: (root.settings.gradient ? root.settings.gradient.angle : 180) - 180
            gradient: Gradient { id: fill }
        }

        Component {
            id: stopComponent
            GradientStop {}
        }

        function rebuild() {
            const old = fill.stops
            const stops = root.settings.gradient ? root.settings.gradient.stops || [] : []
            const made = []
            for (let i = 0; i < stops.length; ++i)
                made.push(stopComponent.createObject(fill, { color: stops[i][0], position: stops[i][1] }))
            fill.stops = made
            for (let j = 0; j < old.length; ++j)
                old[j].destroy()
        }

        Component.onCompleted: rebuild()
        Connections {
            target: root
            function onSettingsChanged() { gradientLayer.rebuild() }
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
