import QtQuick

Rectangle {
    id: root
    default property alias content: holder.children
    property int padding: theme.space.popupPadding
    color: theme.popupBackground
    border.width: 2; border.color: theme.popupBorder
    radius: theme.radius
    implicitWidth: holder.childrenRect.width + padding * 2 + border.width * 2
    implicitHeight: holder.childrenRect.height + padding * 2 + border.width * 2
    Item { id: holder; anchors.fill: parent; anchors.margins: root.padding + root.border.width }
}
