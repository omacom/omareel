import QtQuick
import QtQuick.Effects
import QtMultimedia
import Omarecord

Item {
    id: root
    required property var settings
    required property real zoomScale
    required property int outputWidth
    required property int outputHeight
    required property int sourceWidth
    required property int sourceHeight
    property bool softwareRendering: false
    property bool nativePreview: false
    property real ref: outputHeight / 1080
    readonly property bool squareCrop: settings.shape === "round" || settings.crop === "square"
    readonly property bool quarterTurn: settings.rotation === 90 || settings.rotation === 270
    readonly property real sourceAspect: sourceWidth / Math.max(1, sourceHeight)
    readonly property real aspect: squareCrop ? 1 : (quarterTurn ? 1 / sourceAspect : sourceAspect)
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
        color: Qt.rgba(0, 0, 0, 1)
        layer.enabled: true
        antialiasing: true
    }
    MultiEffect {
        anchors.fill: shadowShape
        source: shadowShape
        visible: root.settings.shadow.enabled && !root.softwareRendering
        shadowEnabled: true
        shadowColor: Qt.rgba(0, 0, 0, 1)
        shadowOpacity: root.settings.shadow.intensity
        shadowBlur: Math.min(1, root.settings.shadow.blur / 64)
        shadowVerticalOffset: root.settings.shadow.distance * root.ref
        autoPaddingEnabled: true
    }
    Rectangle {
        anchors.fill: parent
        y: root.settings.shadow.distance * root.ref
        radius: shadowShape.radius
        color: "black"
        opacity: root.softwareRendering && root.settings.shadow.enabled
            ? root.settings.shadow.intensity * .45 : 0
    }
    Item {
        z: 2
        id: cameraClip
        anchors.fill: parent
        clip: true
        layer.enabled: !root.softwareRendering
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: cameraMask
        }
        Rectangle {
            id: cameraMask
            anchors.fill: parent
            radius: shadowShape.radius
            color: "white"
            visible: false
            layer.enabled: true
        }
        Item {
            id: cameraRotated
            objectName: "cameraRotated"
            anchors.centerIn: parent
            width: root.quarterTurn ? parent.height : parent.width
            height: root.quarterTurn ? parent.width : parent.height
            rotation: root.settings.rotation
            transform: Scale {
                objectName: "cameraFlipTransform"
                origin.x: cameraRotated.width / 2
                origin.y: cameraRotated.height / 2
                xScale: root.settings.flipHorizontal ? -1 : 1
            }
            VideoOutput {
                id: nativeCameraVideo
                visible: root.nativePreview
                anchors.fill: parent
                fillMode: VideoOutput.PreserveAspectCrop
            }
            FrameSource {
                id: cameraVideo
                objectName: "cameraFrameSource"
                visible: !root.nativePreview
                anchors.fill: parent
                cropRect: root.sourceCrop()
                cornerRadiusRatio: !root.softwareRendering ? 0
                    : root.settings.shape === "round" ? .5
                    : root.settings.shape === "rounded"
                      ? root.settings.radius * root.ref / Math.max(1, Math.min(width, height)) : 0
            }
        }
    }
    Rectangle {
        z: 3
        anchors.fill: parent
        radius: shadowShape.radius
        color: "transparent"
        border.width: root.settings.inset.enabled ? root.settings.inset.width * root.ref : 0
        border.color: Qt.rgba(Qt.color(root.settings.inset.color).r,
                              Qt.color(root.settings.inset.color).g,
                              Qt.color(root.settings.inset.color).b,
                              root.settings.inset.alpha)
        antialiasing: true
    }
    readonly property alias cameraFrameSource: cameraVideo
    readonly property alias cameraVideoOutput: nativeCameraVideo
}
