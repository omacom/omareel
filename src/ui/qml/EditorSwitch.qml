import QtQuick
import QtQuick.Controls

Switch {
    id: control
    implicitHeight: 32
    spacing: 10
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.pixelSize: 12
    font.weight: Font.Medium

    indicator: Rectangle {
        implicitWidth: 32
        implicitHeight: 18
        x: control.mirrored ? control.width - width - control.rightPadding : control.leftPadding
        y: control.topPadding + (control.availableHeight - height) / 2
        radius: 9
        color: control.checked ? theme.accent
                               : control.hovered ? theme.hairlineStrong : theme.hairline
        border.width: 1
        border.color: control.checked ? theme.accent : theme.hairlineStrong
        Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
        Rectangle {
            x: control.checked ? parent.width - width - 3 : 3
            anchors.verticalCenter: parent.verticalCenter
            width: 12
            height: 12
            radius: 6
            color: control.checked ? theme.accentForeground : theme.textMuted
            Behavior on x { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: parent.radius + 3
            color: "transparent"
            border.width: 2
            border.color: theme.accent
            visible: control.visualFocus
        }
    }
    contentItem: Label {
        leftPadding: control.indicator.width + control.spacing
        text: control.text
        color: control.enabled ? theme.foreground : theme.textFaint
        font: control.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
}
