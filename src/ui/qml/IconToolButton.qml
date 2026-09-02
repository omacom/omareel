import QtQuick
import QtQuick.Controls

ToolButton {
    id: control
    implicitWidth: 32
    implicitHeight: 32
    width: 32
    height: 32
    topInset: 0
    bottomInset: 0
    focusPolicy: Qt.TabFocus
    property color toolIconColor: enabled ? theme.foreground : Qt.alpha(theme.foreground, .34)
    property color hoverColor: Qt.alpha(theme.foreground, .07)
    icon.width: 20
    icon.height: 20
    icon.color: toolIconColor
    background: Rectangle {
        radius: 6
        color: control.hovered ? control.hoverColor : "transparent"
    }
}
