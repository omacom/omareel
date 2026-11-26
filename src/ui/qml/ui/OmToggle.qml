import QtQuick
import QtQuick.Controls

// Setting row: label (and optional description) on the left, switch on the right. The knob
// springs across and the track fills with the accent.
Switch {
    id: control
    property string description: ""
    implicitHeight: Math.max(theme.space.controlHeight, labels.implicitHeight + 8)
    implicitWidth: contentItem.implicitWidth
    spacing: theme.space.controlGap
    padding: 0
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily
    font.pixelSize: theme.type.body
    indicator: Rectangle {
        id: track
        implicitWidth: 34; implicitHeight: 20
        x: control.width - width
        anchors.verticalCenter: parent.verticalCenter
        radius: theme.rounded ? height / 2 : 0
        color: control.checked ? (control.hovered ? theme.accentHover : theme.accent)
                               : (control.hovered ? theme.fillPressed : Qt.alpha(theme.foreground, .14))
        Behavior on color { ColorAnimation { duration: 160; easing.type: Easing.OutCubic } }
        Rectangle {
            id: knob
            readonly property real travel: track.width - width - 6
            width: control.pressed ? 18 : 14
            height: 14
            x: 3 + (control.checked ? travel : 0)
            anchors.verticalCenter: parent.verticalCenter
            radius: theme.rounded ? height / 2 : 0
            color: control.checked || !theme.dark ? "white" : theme.textStrong
            border.width: theme.dark ? 0 : 1
            border.color: Qt.alpha("black", .08)
            Behavior on x { NumberAnimation { duration: 260; easing.type: Easing.OutBack; easing.overshoot: 1.3 } }
            Behavior on width { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
            Behavior on color { ColorAnimation { duration: 160 } }
        }
        Rectangle {
            anchors.fill: parent; anchors.margins: -3
            visible: control.visualFocus
            color: "transparent"; radius: theme.rounded ? height / 2 : 0
            border.width: 1; border.color: Qt.alpha(theme.accent, .8)
        }
    }
    contentItem: Item {
        implicitWidth: labels.implicitWidth + control.indicator.width + control.spacing
        implicitHeight: labels.implicitHeight
        Column {
            id: labels
            anchors.verticalCenter: parent.verticalCenter
            width: parent.width - control.indicator.width - control.spacing
            spacing: 2
            Text {
                width: parent.width
                textFormat: Text.PlainText
                text: control.text
                color: control.enabled ? theme.foreground : theme.textFaint
                font: control.font
                elide: Text.ElideRight
            }
            Text {
                visible: control.description !== ""
                width: parent.width
                textFormat: Text.PlainText
                text: control.description
                color: theme.textMuted
                font.family: theme.fontFamily
                font.pixelSize: theme.type.caption
                wrapMode: Text.WordWrap
            }
        }
    }
}
