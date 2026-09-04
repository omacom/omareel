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
    property bool leadingDot: false
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

    // Centre the icon+text cluster without spacer items: spacers add layout
    // spacing to implicitWidth, which made grouped buttons overflow when squeezed.
    contentItem: Item {
        implicitWidth: contentRow.implicitWidth
        implicitHeight: contentRow.implicitHeight
        RowLayout {
        id: contentRow
        anchors.centerIn: parent
        width: Math.min(implicitWidth, parent.width)
        spacing: control.spacing
        Item {
            visible: control.leadingDot || control.icon.source.toString() !== ""
            Layout.preferredWidth: control.icon.width
            Layout.preferredHeight: control.icon.height
            Layout.alignment: Qt.AlignVCenter
            IconImage {
                visible: !control.leadingDot
                anchors.fill: parent
                source: control.icon.source
                sourceSize: Qt.size(control.icon.width, control.icon.height)
                color: control.icon.color
            }
            Rectangle {
                visible: control.leadingDot
                anchors.centerIn: parent
                width: 10; height: 10; radius: 5
                color: theme.record
            }
        }
        Text {
            visible: control.text !== ""
            text: control.text
            color: !control.enabled ? theme.textFaint
                 : control.destructive ? theme.record
                 : theme.foreground
            font: control.font
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            Layout.fillWidth: true
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
        }
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
        id: toolTip
        objectName: "omButtonToolTip"
        parent: control
        visible: control.tooltipText !== "" && control.hovered
        delay: 400
        x: (control.width - width) / 2
        y: {
            const below = control.height + 6
            if (!control.Window.window) return below
            const sceneBelow = control.mapToItem(null, 0, below).y
            if (sceneBelow + height <= control.Window.window.height) return below
            const above = -height - 6
            if (control.mapToItem(null, 0, above).y >= 0) return above
            const controlSceneY = control.mapToItem(null, 0, 0).y
            return Math.max(-controlSceneY,
                            control.Window.window.height - height - controlSceneY)
        }
        closePolicy: Popup.NoAutoClose
        modal: false
        dim: false
        focus: false
        padding: theme.space.controlPaddingY
        contentItem: Text {
            enabled: false
            text: control.tooltipText
            color: theme.tooltipText
            font.family: theme.fontFamily
            font.pixelSize: theme.font.bodySmall
        }
        background: Rectangle {
            enabled: false
            color: theme.tooltipBackground
            border.width: 1; border.color: theme.tooltipBorder
            radius: theme.radius
        }
    }
}
