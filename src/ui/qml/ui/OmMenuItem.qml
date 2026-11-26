import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl

MenuItem {
    id: item
    property bool destructive: false
    implicitHeight: theme.space.popupRowHeight + 2
    leftPadding: 10
    rightPadding: 10
    topPadding: 0
    bottomPadding: 0
    hoverEnabled: true
    font.family: theme.fontFamily
    font.pixelSize: theme.type.body
    indicator: null
    arrow: null
    contentItem: Item {
        implicitHeight: label.implicitHeight
        implicitWidth: label.implicitWidth + 24
        Text {
            id: label
            anchors.left: parent.left
            anchors.right: check.left
            anchors.rightMargin: 8
            anchors.verticalCenter: parent.verticalCenter
            textFormat: Text.PlainText
            text: item.text
            color: !item.enabled ? theme.textFaint
                 : item.destructive ? theme.record
                 : item.highlighted || item.checked ? theme.textStrong : theme.foreground
            font: item.font
            elide: Text.ElideRight
        }
        IconImage {
            id: check
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            width: 14; height: 14
            visible: item.checkable && item.checked
            source: "qrc:/omareel/assets/icons/lucide/check.svg"
            sourceSize: Qt.size(14, 14)
            color: theme.accent
        }
    }
    background: Rectangle {
        radius: theme.controlRadius
        color: item.down ? theme.fillPressed
             : item.highlighted ? (item.destructive ? Qt.alpha(theme.record, .14) : theme.fillHover)
             : "transparent"
        Behavior on color { ColorAnimation { duration: 90 } }
    }
}
