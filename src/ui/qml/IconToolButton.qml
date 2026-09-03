import QtQuick
import QtQuick.Controls

ToolButton {
    id: control
    implicitWidth: theme.space.controlHeight
    implicitHeight: theme.space.controlHeight
    width: theme.space.controlHeight
    height: theme.space.controlHeight
    topInset: 0
    bottomInset: 0
    focusPolicy: Qt.TabFocus
    hoverEnabled: true
    property color toolIconColor: enabled ? theme.textMuted : theme.textFaint
    property color hoverColor: theme.hoverFill
    icon.width: 20
    icon.height: 20
    icon.color: toolIconColor
    background: Item {
        Rectangle {
            anchors.fill: parent
            radius: theme.radius
            color: control.down ? theme.pressedFill
                                : control.hovered ? control.hoverColor : "transparent"
            border.width: control.hovered ? theme.hoverBorderWidth : 0
            border.color: theme.hoverBorder
            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: theme.radius
            color: "transparent"
            border.width: theme.hoverBorderWidth
            border.color: theme.hoverBorder
            visible: control.visualFocus
        }
    }
}
