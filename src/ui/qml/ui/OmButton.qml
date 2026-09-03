import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

Button {
    id: control
    property bool selected: checked
    property bool active: false
    property bool bordered: false
    property bool primary: false
    property bool destructive: false
    property string tooltipText: ""
    property url trailingIconSource

    implicitHeight: theme.space.controlHeight
    implicitWidth: contentRow.implicitWidth + leftPadding + rightPadding
    topInset: 0; bottomInset: 0
    leftPadding: theme.space.controlPaddingX
    rightPadding: theme.space.controlPaddingX
    topPadding: theme.space.controlPaddingY
    bottomPadding: theme.space.controlPaddingY
    spacing: theme.space.controlGap
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body
    font.bold: selected
    icon.width: 14
    icon.height: 14
    icon.color: destructive ? theme.record : theme.foreground

    contentItem: RowLayout {
        id: contentRow
        spacing: control.spacing
        Item { Layout.fillWidth: true }
        Item {
            visible: control.icon.source.toString() !== ""
            Layout.preferredWidth: control.icon.width
            Layout.preferredHeight: control.icon.height
            Layout.alignment: Qt.AlignVCenter
            IconImage {
                anchors.fill: parent
                source: control.icon.source
                sourceSize: Qt.size(control.icon.width, control.icon.height)
                color: control.icon.color
            }
        }
        Text {
            visible: control.text !== ""
            text: control.text
            color: !control.enabled ? theme.textFaint
                 : control.destructive ? theme.record
                 : theme.foreground
            font: control.font
            Layout.alignment: Qt.AlignVCenter
        }
        Item {
            visible: control.trailingIconSource.toString() !== ""
            Layout.preferredWidth: 14
            Layout.preferredHeight: 14
            Layout.alignment: Qt.AlignVCenter
            IconImage {
                anchors.fill: parent
                source: control.trailingIconSource
                sourceSize: Qt.size(14, 14)
                color: control.destructive ? theme.record : theme.foreground
            }
        }
        Item { Layout.fillWidth: true }
    }

    background: Rectangle {
        radius: theme.radius
        color: control.down ? theme.pressedFill
             : control.primary ? theme.selectedFill
             : control.selected || control.active ? theme.selectedFill
             : control.visualFocus || control.hovered ? theme.hoverFill
             : control.bordered ? theme.normalFill : "transparent"
        border.width: control.primary ? 1
                    : (control.bordered || control.hovered || control.visualFocus || control.selected)
                      ? theme.controlBorderWidth(control.visualFocus, control.hovered, control.selected) : 0
        border.color: control.primary ? Qt.alpha(theme.selectedBorder, 1)
                     : theme.controlBorder(control.visualFocus, control.hovered, control.selected)
        Behavior on color { ColorAnimation { duration: 120 } }
        Behavior on border.color { ColorAnimation { duration: 120 } }
    }
    ToolTip {
        parent: control
        visible: control.tooltipText !== "" && control.hovered
        delay: 400
        padding: theme.space.controlPaddingY
        contentItem: Text {
            text: control.tooltipText
            color: theme.tooltipText
            font.family: theme.fontFamily
            font.pixelSize: theme.font.bodySmall
        }
        background: Rectangle {
            color: theme.tooltipBackground
            border.width: 1; border.color: theme.tooltipBorder
            radius: theme.radius
        }
    }
}
