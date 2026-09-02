import QtQuick
import QtQuick.Effects
import Omarecord

Item {
    id: root
    readonly property alias frameSource: video
    required property var settings
    required property var crop
    property bool softwareRendering: false
    property real ref: 1

    Rectangle {
        id: shadowShape
        anchors.fill: parent
        radius: root.settings.radius * root.ref
        color: "#000000"
        visible: true
    }
    MultiEffect {
        anchors.fill: shadowShape
        source: shadowShape
        visible: root.settings.shadow.enabled && !root.softwareRendering
        shadowEnabled: true
        shadowColor: "#000000"
        shadowOpacity: root.settings.shadow.intensity
        shadowBlur: Math.min(1, root.settings.shadow.blur / 64)
        shadowHorizontalOffset: Math.cos(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        shadowVerticalOffset: Math.sin(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        autoPaddingEnabled: true
    }
    Rectangle {
        anchors.fill: parent
        x: Math.cos(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        y: Math.sin(root.settings.shadow.angle * Math.PI / 180) * root.settings.shadow.distance * root.ref
        radius: root.settings.radius * root.ref
        color: "black"
        opacity: root.softwareRendering && root.settings.shadow.enabled ? root.settings.shadow.intensity * 0.45 : 0
    }
    Rectangle {
        id: mask
        anchors.fill: parent
        radius: root.settings.radius * root.ref
        color: "white"
        visible: false
        layer.enabled: !root.softwareRendering
        antialiasing: true
    }
    Item {
        anchors.fill: parent
        clip: true
        layer.enabled: !root.softwareRendering
        layer.smooth: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: mask
            maskThresholdMin: 0.0
            maskSpreadAtMin: 1.0
        }
        FrameSource {
            id: video
            objectName: "videoFrameSource"
            x: -root.crop.x * root.width / Math.max(0.0001, root.crop.w)
            y: -root.crop.y * root.height / Math.max(0.0001, root.crop.h)
            width: root.width / Math.max(0.0001, root.crop.w)
            height: root.height / Math.max(0.0001, root.crop.h)
        }
    }
    Rectangle {
        anchors.fill: parent
        radius: root.settings.radius * root.ref
        color: "transparent"
        border.width: root.settings.inset.enabled ? root.settings.inset.width * root.ref : 0
        border.color: Qt.rgba(Qt.color(root.settings.inset.color).r,
                              Qt.color(root.settings.inset.color).g,
                              Qt.color(root.settings.inset.color).b,
                              root.settings.inset.alpha)
        antialiasing: true
    }
}
