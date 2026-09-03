import QtQuick
import QtQuick.Controls

Switch {
    id: control
    implicitHeight: Math.max(theme.space.controlHeight, 22)
    spacing: theme.space.controlGap
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body
    indicator: Rectangle {
        implicitWidth: 42; implicitHeight: 22
        x: control.mirrored ? control.width - width : 0
        anchors.verticalCenter: parent.verticalCenter
        radius: theme.rounded ? height / 2 : 0
        color: control.checked ? theme.selectedFill : theme.normalFill
        border.width: control.checked ? theme.selectedBorderWidth : theme.normalBorderWidth
        border.color: control.checked ? theme.selectedBorder : theme.normalBorder
        Behavior on color { ColorAnimation { duration: 120 } }
        Rectangle {
            width: 16; height: 16
            x: control.checked ? parent.width - width - 3 : 3
            anchors.verticalCenter: parent.verticalCenter
            radius: theme.rounded ? height / 2 : 0
            color: control.checked ? theme.foreground : theme.textMuted
            Behavior on x { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
        Rectangle {
            anchors.fill: parent; anchors.margins: -3
            visible: control.visualFocus || control.hovered
            color: "transparent"; radius: theme.radius
            border.width: theme.hoverBorderWidth; border.color: theme.hoverBorder
        }
    }
    contentItem: Text {
        leftPadding: control.indicator.width + control.spacing
        text: control.text
        color: control.enabled ? theme.foreground : theme.textFaint
        font: control.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
}
