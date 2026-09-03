import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects

Button {
    id: control
    property bool selected: checked
    property bool active: false
    property bool bordered: false
    property bool primary: false
    property bool destructive: false
    property string tooltipText: ""

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
    icon.width: theme.font.title
    icon.height: theme.font.title
    icon.color: destructive ? theme.record : selected ? theme.menuSelectedText : theme.foreground

    contentItem: RowLayout {
        id: contentRow
        spacing: control.spacing
        Item { Layout.fillWidth: true }
        Item {
            visible: control.icon.source.toString() !== ""
            Layout.preferredWidth: control.icon.width
            Layout.preferredHeight: control.icon.height
            Layout.alignment: Qt.AlignVCenter
            Image {
                id: buttonIcon
                anchors.fill: parent
                visible: false
                source: control.icon.source
                sourceSize: Qt.size(control.icon.width, control.icon.height)
            }
            MultiEffect {
                anchors.fill: parent
                source: buttonIcon
                colorization: 1
                colorizationColor: control.icon.color
            }
        }
        Text {
            visible: control.text !== ""
            text: control.text
            color: !control.enabled ? theme.textFaint
                 : control.destructive ? theme.record
                 : control.selected ? theme.menuSelectedText : theme.foreground
            font: control.font
            Layout.alignment: Qt.AlignVCenter
        }
        Item { Layout.fillWidth: true }
    }

    background: Rectangle {
        radius: theme.radius
        color: control.down ? theme.pressedFill
             : control.primary ? theme.selectedFill
             : theme.controlFill(control.visualFocus, control.hovered, control.selected || control.active)
        border.width: control.primary ? 1
                    : (control.bordered || control.hovered || control.visualFocus || control.selected)
                      ? theme.controlBorderWidth(control.visualFocus, control.hovered, control.selected) : 0
        border.color: control.primary ? theme.accent
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
