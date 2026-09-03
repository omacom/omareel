import QtQuick
import QtQuick.Controls

Slider {
    id: control
    implicitHeight: 28
    focusPolicy: Qt.TabFocus
    hoverEnabled: true

    background: Item {
        x: control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        width: control.availableWidth
        height: 4
        Rectangle {
            anchors.fill: parent
            radius: 2
            color: theme.hairlineStrong
        }
        Rectangle {
            width: control.visualPosition * parent.width
            height: parent.height
            radius: 2
            color: theme.accent
        }
    }
    handle: Rectangle {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding + (control.availableHeight - height) / 2
        width: 14
        height: 14
        radius: 7
        color: theme.surfaceRaised
        border.width: 2
        border.color: control.pressed || control.hovered ? theme.accent : theme.hairlineStrong
        scale: control.pressed ? .92 : 1
        Behavior on border.color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        Behavior on scale { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: width / 2
            color: "transparent"
            border.width: 2
            border.color: theme.accent
            visible: control.visualFocus
        }
    }
}
