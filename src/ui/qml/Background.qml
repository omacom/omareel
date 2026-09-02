import QtQuick
import QtQuick.Effects

Item {
    id: root
    required property var settings
    property bool softwareRendering: false

    Rectangle {
        anchors.fill: parent
        color: root.settings.color || "#1a1b26"
        visible: root.settings.type === "color" || root.settings.type === "none" || !backgroundImage.visible
    }
    Rectangle {
        anchors.fill: parent
        visible: root.settings.type === "gradient"
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: root.settings.gradient.stops[0][0] }
            GradientStop { position: 1; color: root.settings.gradient.stops[root.settings.gradient.stops.length - 1][0] }
        }
        rotation: root.settings.gradient.angle - 90
        scale: 1.5
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
