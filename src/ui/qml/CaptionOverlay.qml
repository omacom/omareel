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
    // The widest a line may be: lines longer than this (portrait frames, large sizes) wrap
    // inside the pill rather than spilling out of it.
    readonly property real textWidth: Math.max(0, width - 80 * ref - 36 * size * ref)
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

        // The cue's own lines when each fits; otherwise all of it rewrapped to the width, so
        // a line too long for a portrait frame doesn't leave its last word alone.
        Label {
            id: lines
            readonly property var cueLines: root.caption && root.caption.lines ? root.caption.lines : []
            readonly property bool fits: natural.implicitWidth <= root.textWidth
            anchors.centerIn: parent
            width: Math.min(implicitWidth, root.textWidth)
            text: fits ? cueLines.join("\n") : cueLines.join(" ")
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
            lineHeight: 1.06
            color: root.textColor
            textFormat: Text.PlainText
            font.pixelSize: 36 * root.size * root.ref
            font.weight: Font.DemiBold
        }
        Label {
            id: natural
            visible: false
            text: lines.cueLines.join("\n")
            textFormat: Text.PlainText
            font: lines.font
        }
    }
}
