import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Button {
    id: control
    property bool primary: false
    property bool destructive: false
    property bool selected: false

    implicitHeight: primary ? 36 : 32
    topInset: 0
    bottomInset: 0
    leftPadding: 12
    rightPadding: 12
    spacing: 7
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.pixelSize: 12
    font.weight: Font.Medium
    icon.width: 16
    icon.height: 16
    icon.color: primary ? theme.accentForeground
                              : destructive ? theme.record
                              : selected ? theme.accent : theme.foreground

    contentItem: RowLayout {
        spacing: control.spacing
        Item { Layout.fillWidth: true }
        ToolButton {
            visible: control.icon.source.toString() !== ""
            Layout.preferredWidth: control.icon.width
            Layout.preferredHeight: control.icon.height
            padding: 0
            enabled: false
            opacity: control.enabled ? 1 : .42
            icon.source: control.icon.source
            icon.width: control.icon.width
            icon.height: control.icon.height
            icon.color: control.icon.color
            background: null
        }
        Label {
            text: control.text
            visible: text.length > 0
            color: !control.enabled ? theme.textFaint
                   : control.primary ? theme.accentForeground
                   : control.destructive ? theme.record
                   : control.selected ? theme.accent : theme.foreground
            font: control.font
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        Item { Layout.fillWidth: true }
    }

    background: Rectangle {
        radius: control.primary ? 8 : 6
        color: control.primary
               ? (control.down ? Qt.darker(theme.accent, 1.10)
                  : control.hovered ? Qt.lighter(theme.accent, 1.06) : theme.accent)
               : control.selected ? theme.accentSoft
               : control.down ? theme.hairlineStrong
               : control.hovered ? theme.hairline : "transparent"
        border.width: control.primary ? 0 : 1
        border.color: control.selected ? Qt.alpha(theme.accent, .40) : theme.hairlineStrong
        Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
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
}
