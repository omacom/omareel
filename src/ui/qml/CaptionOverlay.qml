import QtQuick
import QtQuick.Controls

// The caption at the current moment: `caption` is {lines, opacity, position, size} from the
// editor's preview or the exporter's frame. Styled like the keystroke pills.
Item {
    id: root
    required property var caption
    required property var colors
    property real ref: 1
    readonly property real size: caption && caption.size ? caption.size : 1
    readonly property color pillColor: Qt.alpha(colors.background, .85)
    readonly property color textColor: colors.foreground
    anchors.fill: parent

    Rectangle {
        id: box
        visible: root.caption && root.caption.lines && root.caption.lines.length > 0
        opacity: root.caption && root.caption.opacity !== undefined ? root.caption.opacity : 1
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: root.caption && root.caption.position === "top" ? parent.top : undefined
        anchors.bottom: root.caption && root.caption.position === "top" ? undefined : parent.bottom
        anchors.topMargin: 40 * root.ref
        // Above the keystroke pills' corner.
        anchors.bottomMargin: 110 * root.ref
        width: Math.min(parent.width - 80 * root.ref, lines.implicitWidth + 36 * root.size * root.ref)
        height: lines.implicitHeight + 20 * root.size * root.ref
        radius: 12 * root.size * root.ref
        color: root.pillColor
        border.width: root.ref
        border.color: Qt.alpha(root.textColor, .12)

        Column {
            id: lines
            anchors.centerIn: parent
            spacing: 2 * root.size * root.ref
            Repeater {
                model: root.caption && root.caption.lines ? root.caption.lines : []
                delegate: Label {
                    required property string modelData
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: modelData
                    color: root.textColor
                    textFormat: Text.PlainText
                    font.pixelSize: 36 * root.size * root.ref
                    font.weight: Font.DemiBold
                }
            }
        }
    }
}
