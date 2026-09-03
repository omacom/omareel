import QtQuick
import QtQuick.Controls

Slider {
    id: control
    property int tickCount: 0
    implicitHeight: theme.space.controlHeight
    focusPolicy: Qt.TabFocus
    hoverEnabled: true
    background: Item {
        x: control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        width: control.availableWidth; height: 4
        Rectangle { anchors.fill: parent; radius: theme.rounded ? 2 : 0; color: theme.selectedFill }
        Rectangle { width: control.visualPosition * parent.width; height: 4; radius: theme.rounded ? 2 : 0; color: theme.foreground }
        Repeater {
            model: control.tickCount > 1 ? control.tickCount : 0
            Rectangle {
                required property int index
                width: 1; height: 8; color: theme.background
                x: index * (parent.width - width) / Math.max(1, control.tickCount - 1)
                anchors.verticalCenter: parent.verticalCenter
            }
        }
    }
    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + (control.availableHeight - height) / 2
        width: 14; height: 14; radius: width / 2
        color: theme.foreground
        border.width: 2; border.color: theme.background
        scale: control.hovered || control.pressed ? 1.15 : 1
        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
}
