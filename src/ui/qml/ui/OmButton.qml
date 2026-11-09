import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

// One button for the whole app. Ghost by default; `bordered` gives the soft filled secondary
// look, `primary` the accent call to action, `destructive` turns either toward the record red.
Button {
    id: control
    property bool selected: checked
    property bool active: false
    property bool bordered: false
    property bool primary: false
    property bool destructive: false
    property bool leadingDot: false
    property string tooltipText: ""
    property string tooltipShortcut: ""
    property string tooltipPlacement: "below"
    property url trailingIconSource
    property color contentColor: !enabled ? theme.textFaint
        : primary ? (destructive ? "white" : theme.accentForeground)
        : destructive ? theme.record
        : selected || active || hovered || down ? theme.textStrong : theme.foreground

    implicitHeight: theme.space.controlHeight
    implicitWidth: contentRow.implicitWidth + leftPadding + rightPadding
    topInset: 0; bottomInset: 0
    leftPadding: theme.space.controlPaddingX + 2
    rightPadding: theme.space.controlPaddingX + 2
    topPadding: 0
    bottomPadding: 0
    spacing: theme.space.md
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily
    font.pixelSize: theme.type.body
    font.weight: Font.Medium
    icon.width: 15
    icon.height: 15
    icon.color: contentColor
    opacity: enabled ? 1 : .55
    scale: down ? .97 : 1
    Behavior on scale { NumberAnimation { duration: control.down ? 70 : 220; easing.type: Easing.OutBack; easing.overshoot: 2.2 } }

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
                // Icons are made only for buttons that have them: every button made both up
                // front, about 260 in the editor, most with no source.
                Loader {
                    anchors.fill: parent
                    active: !control.leadingDot && control.icon.source.toString() !== ""
                    sourceComponent: IconImage {
                        source: control.icon.source
                        sourceSize: Qt.size(control.icon.width, control.icon.height)
                        color: control.icon.color
                        Behavior on color { ColorAnimation { duration: 140 } }
                    }
                }
                Rectangle {
                    visible: control.leadingDot
                    anchors.centerIn: parent
                    width: 8; height: 8; radius: 4
                    color: theme.record
                }
            }
            Text {
                visible: control.text !== ""
                text: control.text
                color: control.contentColor
                font: control.font
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                Behavior on color { ColorAnimation { duration: 140 } }
            }
            Item {
                visible: control.trailingIconSource.toString() !== ""
                Layout.preferredWidth: 14
                Layout.preferredHeight: 14
                Layout.alignment: Qt.AlignVCenter
                Loader {
                    anchors.fill: parent
                    active: control.trailingIconSource.toString() !== ""
                    sourceComponent: IconImage {
                        source: control.trailingIconSource
                        sourceSize: Qt.size(14, 14)
                        color: control.primary ? control.contentColor : theme.textMuted
                    }
                }
            }
        }
    }

    background: Rectangle {
        radius: theme.controlRadius
        readonly property color primaryColor: control.destructive ? theme.record : theme.accent
        color: control.primary
               ? (control.down ? Qt.darker(primaryColor, 1.12)
                  : control.hovered ? Qt.lighter(primaryColor, 1.08) : primaryColor)
             : control.destructive && (control.hovered || control.down) ? Qt.alpha(theme.record, control.down ? .22 : .14)
             : control.down ? theme.fillPressed
             : control.selected || control.active ? theme.fillPressed
             : control.hovered || control.visualFocus ? theme.fillHover
             : control.bordered ? theme.fill : Qt.alpha(theme.foreground, 0)
        border.width: control.visualFocus ? 1 : control.bordered && !control.primary ? 1 : 0
        border.color: control.visualFocus ? Qt.alpha(theme.accent, .8)
                    : control.selected ? theme.strokeStrong : theme.stroke
        Behavior on color { ColorAnimation { duration: 140; easing.type: Easing.OutCubic } }
        // Light catches the top edge of filled buttons.
        Rectangle {
            visible: control.primary
            anchors.fill: parent
            radius: parent.radius
            gradient: Gradient {
                GradientStop { position: 0; color: Qt.rgba(1, 1, 1, .14) }
                GradientStop { position: .5; color: Qt.rgba(1, 1, 1, 0) }
            }
        }
    }
    // The tooltip is made the first time it is wanted, then kept: a window holds a hundred
    // buttons, and making every one's popup up front was much of the editor's QML start.
    property bool toolTipWanted: false
    onHoveredChanged: if (hovered && tooltipText !== "") toolTipWanted = true
    Loader {
        anchors.fill: parent
        active: control.toolTipWanted
        sourceComponent: OmToolTip {
            objectName: "omButtonToolTip"
            target: control
            visible: control.tooltipText !== "" && control.hovered && !control.down
            text: control.tooltipText
            shortcut: control.tooltipShortcut
            placement: control.tooltipPlacement
        }
    }
}
