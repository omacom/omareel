import QtQuick
import QtQuick.Controls

TextField {
    id: control
    implicitHeight: theme.space.controlHeight
    leftPadding: theme.space.controlPaddingX; rightPadding: theme.space.controlPaddingX
    topPadding: theme.space.controlPaddingY; bottomPadding: theme.space.controlPaddingY
    color: theme.foreground; placeholderTextColor: theme.textFaint
    selectionColor: theme.selectionFill; selectedTextColor: theme.foreground
    font.family: theme.fontFamily; font.pixelSize: theme.font.body
    background: Rectangle {
        radius: theme.radius
        color: theme.controlFill(control.activeFocus, control.hovered, false)
        border.width: theme.controlBorderWidth(control.activeFocus, control.hovered, false)
        border.color: theme.controlBorder(control.activeFocus, control.hovered, false)
        Behavior on color { ColorAnimation { duration: 120 } }
    }
}
