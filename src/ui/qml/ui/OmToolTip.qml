import QtQuick
import QtQuick.Controls

// Small dark label that fades and drifts in toward its control. Placement prefers below the
// control and flips above (or clamps) when the window edge is in the way.
ToolTip {
    id: root
    property Item target: parent
    property string placement: "below"
    property string shortcut: ""
    delay: 450
    closePolicy: Popup.NoAutoClose
    modal: false
    dim: false
    focus: false
    padding: 0
    margins: 6
    readonly property int gap: 6
    readonly property bool horizontal: placement === "left" || placement === "right"
    readonly property bool flipped: {
        if (placement === "above") return true
        if (horizontal || !target || !target.Window.window) return false
        return target.mapToItem(null, 0, target.height + gap).y + height > target.Window.window.height
    }
    // Drift toward the control while entering.
    property real slide: 0
    x: {
        if (!target) return 0
        if (placement === "left") return -width - gap - slide
        if (placement === "right") return target.width + gap + slide
        return Math.round((target.width - width) / 2)
    }
    y: {
        if (!target) return 0
        if (horizontal) return Math.round((target.height - height) / 2)
        return flipped ? -height - gap - slide : target.height + gap + slide
    }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 140; easing.type: Easing.OutCubic }
        NumberAnimation { property: "slide"; from: 4; to: 0; duration: 200; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: 90; easing.type: Easing.InCubic }
    }
    contentItem: Item {
        implicitWidth: row.implicitWidth + 16
        implicitHeight: 26
        Row {
            id: row
            anchors.centerIn: parent
            spacing: 8
            Text {
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: root.text
                color: theme.textStrong
                font.family: theme.fontFamily
                font.pixelSize: theme.type.caption
                font.weight: Font.Medium
            }
            Text {
                visible: root.shortcut !== ""
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: root.shortcut
                color: theme.textMuted
                font.family: theme.monoFamily
                font.pixelSize: theme.type.caption
            }
        }
    }
    background: Rectangle {
        color: theme.raised
        radius: theme.controlRadius
        border.width: 1
        border.color: theme.strokeStrong
    }
}
