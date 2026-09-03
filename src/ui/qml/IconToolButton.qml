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
    hoverEnabled: true
    property color toolIconColor: enabled ? theme.foreground : theme.textFaint
    property color hoverColor: theme.hairline
    icon.width: 20
    icon.height: 20
    icon.color: toolIconColor
    background: Item {
        Rectangle {
            anchors.fill: parent
            radius: 6
            color: control.hovered ? control.hoverColor : "transparent"
            Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: 9
            color: "transparent"
            border.width: 2
            border.color: theme.accent
            visible: control.visualFocus
        }
    }
}
