import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root
    property url iconSource
    property string label: ""
    property string value: ""
    property bool selected: false
    signal clicked()
    implicitHeight: theme.space.popupRowHeight
    radius: theme.radius
    color: selected ? theme.selectedFill : hover.hovered ? theme.hoverFill : "transparent"
    border.width: selected ? theme.selectedBorderWidth : 0
    border.color: theme.selectedBorder
    Behavior on color { ColorAnimation { duration: 120 } }
    RowLayout {
        anchors.fill: parent; anchors.leftMargin: theme.space.rowPaddingX; anchors.rightMargin: theme.space.rowPaddingX
        spacing: theme.space.rowGap
        Image { visible: root.iconSource.toString() !== ""; source: root.iconSource; sourceSize: Qt.size(theme.font.title, theme.font.title); Layout.preferredWidth: theme.font.title; Layout.preferredHeight: theme.font.title }
        Text { Layout.fillWidth: true; text: root.label; color: theme.foreground; font.family: theme.fontFamily; font.pixelSize: theme.font.body; elide: Text.ElideRight }
        Text { text: root.value; color: theme.textMuted; font.family: theme.fontFamily; font.pixelSize: theme.font.caption; horizontalAlignment: Text.AlignRight }
    }
    HoverHandler { id: hover }
    MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.clicked() }
}
