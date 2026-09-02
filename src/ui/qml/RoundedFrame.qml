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
    property bool unframed: false

    Rectangle {
        id: shadowShape
        anchors.fill: parent
        radius: root.unframed ? 0 : root.settings.radius * root.ref
        color: "#000000"
        layer.enabled: true
        antialiasing: true
    }
    MultiEffect {
        anchors.fill: shadowShape
        source: shadowShape
        visible: root.settings.shadow.enabled && !root.unframed
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
        radius: root.unframed ? 0 : root.settings.radius * root.ref
        color: "black"
        opacity: root.softwareRendering && root.settings.shadow.enabled && !root.unframed ? root.settings.shadow.intensity * 0.45 : 0
    }
    Item {
        anchors.fill: parent
        clip: true
        FrameSource {
            id: video
            objectName: "videoFrameSource"
            anchors.fill: parent
            cropRect: Qt.rect(root.crop.x, root.crop.y, root.crop.w, root.crop.h)
            cornerRadiusRatio: root.unframed ? 0
                : root.settings.radius * root.ref / Math.max(1, Math.min(root.width, root.height))
        }
    }
    Rectangle {
        anchors.fill: parent
        visible: !root.unframed
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
