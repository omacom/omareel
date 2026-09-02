import QtQuick
import QtQuick.Shapes

Item {
    id: root
    required property var cursor
    required property var ripples
    required property var settings
    property real ref: 1

    Repeater {
        model: root.ripples
        delegate: Shape {
            required property var modelData
            x: modelData.x * root.width - width / 2
            y: modelData.y * root.height - height / 2
            width: 32 * root.settings.size * root.ref * (0.2 + 3.3 * Math.min(1, modelData.progress / (0.15 / 0.45)))
            height: width
            opacity: 0.6 * (modelData.progress < 0.05 ? modelData.progress / 0.05
                     : modelData.progress < 0.8 ? 1 - (modelData.progress - 0.05) / 0.75 : 0)
            ShapePath {
                strokeColor: root.settings.ringColor
                strokeWidth: 2 * root.ref
                fillColor: "transparent"
                PathAngleArc { centerX: width / 2; centerY: height / 2; radiusX: width / 2; radiusY: height / 2; startAngle: 0; sweepAngle: 360 }
            }
        }
    }
    Image {
        id: arrow
        source: "qrc:/omarecord/assets/cursors/arrow.svg"
        sourceSize: Qt.size(Math.round(24 * root.settings.size * root.ref * 4),
                            Math.round(24 * root.settings.size * root.ref * 4))
        width: 24 * root.settings.size * root.ref
        height: width
        x: root.cursor.x * root.width - (3 / 24) * width
        y: root.cursor.y * root.height - (2 / 24) * height
        visible: root.settings.visible
        opacity: root.cursor.opacity
        scale: root.cursor.scale
        rotation: root.cursor.rotation
        transformOrigin: Item.TopLeft
        smooth: true
    }
}
