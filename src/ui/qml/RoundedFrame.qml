import QtQuick
import QtQuick.Effects
import QtMultimedia
import Omareel

Item {
    id: root
    readonly property alias frameSource: video
    readonly property alias videoOutput: nativeVideo
    required property var settings
    required property var crop
    property bool softwareRendering: false
    property real ref: 1
    property bool unframed: false
    property bool nativePreview: false
    readonly property real borderWidth: !unframed && settings.border.enabled
                                        ? settings.border.width * ref : 0
    readonly property real outerRadius: unframed ? 0 : settings.radius * ref + borderWidth

    Rectangle {
        id: shadowShape
        x: -root.borderWidth
        y: -root.borderWidth
        width: parent.width + 2 * root.borderWidth
        height: parent.height + 2 * root.borderWidth
        radius: root.outerRadius
        color: Qt.rgba(0, 0, 0, 1)
        layer.enabled: true
        antialiasing: true
    }
    MultiEffect {
        anchors.fill: shadowShape
        source: shadowShape
        visible: root.settings.shadow.enabled && !root.unframed
        shadowEnabled: true
        shadowColor: Qt.rgba(0, 0, 0, 1)
        shadowOpacity: root.settings.shadow.intensity
        shadowBlur: Math.min(1, root.settings.shadow.blur / 64)
        shadowHorizontalOffset: Math.cos(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        shadowVerticalOffset: Math.sin(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        autoPaddingEnabled: true
    }
    Rectangle {
        x: -root.borderWidth + Math.cos(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        y: -root.borderWidth + Math.sin(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        width: parent.width + 2 * root.borderWidth
        height: parent.height + 2 * root.borderWidth
        radius: root.outerRadius
        color: "black"
        opacity: root.softwareRendering && root.settings.shadow.enabled && !root.unframed ? root.settings.shadow.intensity * 0.45 : 0
    }
    Item {
        id: videoClip
        z: 2
        anchors.fill: parent
        clip: true
        layer.enabled: !root.unframed && !root.softwareRendering
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: videoMask
        }
        Rectangle {
            id: videoMask
            anchors.fill: parent
            radius: root.settings.radius * root.ref
            color: "white"
            visible: false
            layer.enabled: true
        }
        VideoOutput {
            id: nativeVideo
            visible: root.nativePreview
            x: -root.crop.x * width
            y: -root.crop.y * height
            width: parent.width / Math.max(.0001, root.crop.w)
            height: parent.height / Math.max(.0001, root.crop.h)
            fillMode: VideoOutput.Stretch
        }
        FrameSource {
            id: video
            objectName: "videoFrameSource"
            anchors.fill: parent
            visible: !root.nativePreview
            cropRect: Qt.rect(root.crop.x, root.crop.y, root.crop.w, root.crop.h)
            cornerRadiusRatio: !root.softwareRendering || root.unframed ? 0
                : root.settings.radius * root.ref / Math.max(1, Math.min(root.width, root.height))
        }
    }
    Rectangle {
        x: -root.borderWidth
        y: -root.borderWidth
        width: parent.width + 2 * root.borderWidth
        height: parent.height + 2 * root.borderWidth
        visible: !root.unframed
        radius: root.outerRadius
        color: root.settings.border.enabled
            ? Qt.rgba(Qt.color(root.settings.border.color).r,
                      Qt.color(root.settings.border.color).g,
                      Qt.color(root.settings.border.color).b,
                      root.settings.border.alpha) : "transparent"
        antialiasing: true
        z: 1
    }
}
