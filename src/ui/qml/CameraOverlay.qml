import QtQuick
import QtQuick.Effects
import Omarecord

Item {
    id: root
    required property var settings
    required property var frameShadow
    required property real zoomScale
    required property int outputWidth
    required property int outputHeight
    required property int sourceWidth
    required property int sourceHeight
    property bool softwareRendering: false
    property real ref: outputHeight / 1080
    readonly property bool squareCrop: settings.shape === "round" || settings.crop === "square"
    readonly property real aspect: squareCrop ? 1 : sourceWidth / Math.max(1, sourceHeight)
    readonly property real zoomProgress: Math.max(0, Math.min(1, (zoomScale - 1.05) / 0.1))
    readonly property real zoomMultiplier: 1 - (1 - settings.scaleDuringZoom) * zoomProgress
    width: outputHeight * settings.size * zoomMultiplier * aspect
    height: outputHeight * settings.size * zoomMultiplier
    x: settings.position.indexOf("left") >= 0
       ? outputWidth * settings.offset.x : outputWidth * (1 - settings.offset.x) - width
    y: settings.position.indexOf("top") >= 0
       ? outputHeight * settings.offset.y : outputHeight * (1 - settings.offset.y) - height

    function sourceCrop() {
        if (!squareCrop || sourceWidth <= 0 || sourceHeight <= 0) return Qt.rect(0, 0, 1, 1)
        if (sourceWidth > sourceHeight) {
            let visible = sourceHeight / sourceWidth
            return Qt.rect((1 - visible) / 2, 0, visible, 1)
        }
        let visible = sourceWidth / sourceHeight
        return Qt.rect(0, (1 - visible) / 2, 1, visible)
    }

    Rectangle {
        id: shadowShape
        anchors.fill: parent
        radius: root.settings.shape === "round" ? width / 2
              : root.settings.shape === "rounded" ? root.settings.radius * root.ref : 0
        color: "#000000"
        layer.enabled: true
        antialiasing: true
    }
    MultiEffect {
        anchors.fill: shadowShape
        source: shadowShape
        visible: root.settings.shadow && !root.softwareRendering
        shadowEnabled: true
        shadowColor: "#000000"
        shadowOpacity: root.frameShadow.intensity
        shadowBlur: Math.min(1, root.frameShadow.blur / 64)
        shadowHorizontalOffset: Math.cos(root.frameShadow.angle * Math.PI / 180)
                                * root.frameShadow.distance * root.ref
        shadowVerticalOffset: Math.sin(root.frameShadow.angle * Math.PI / 180)
                              * root.frameShadow.distance * root.ref
        autoPaddingEnabled: true
    }
    Rectangle {
        anchors.fill: parent
        x: Math.cos(root.frameShadow.angle * Math.PI / 180) * root.frameShadow.distance * root.ref
        y: Math.sin(root.frameShadow.angle * Math.PI / 180) * root.frameShadow.distance * root.ref
        radius: shadowShape.radius
        color: "black"
        opacity: root.softwareRendering && root.settings.shadow ? root.frameShadow.intensity * .45 : 0
    }
    FrameSource {
        id: cameraVideo
        objectName: "cameraFrameSource"
        anchors.fill: parent
        cropRect: root.sourceCrop()
        cornerRadiusRatio: root.settings.shape === "round" ? .5
            : root.settings.shape === "rounded"
              ? root.settings.radius * root.ref / Math.max(1, Math.min(root.width, root.height)) : 0
        scale: root.settings.mirror ? -1 : 1
        transformOrigin: Item.Center
    }
    readonly property alias cameraFrameSource: cameraVideo
}
