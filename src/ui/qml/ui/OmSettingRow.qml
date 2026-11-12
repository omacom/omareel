import QtQuick
import QtQuick.Layouts

// Label (and optional description) on the left, any control on the right.
RowLayout {
    id: root
    default property alias trailing: trailingHolder.data
    property string label: ""
    property string description: ""
    spacing: theme.space.xxl
    Layout.fillWidth: true
    Layout.minimumHeight: theme.space.controlHeight
    ColumnLayout {
        Layout.fillWidth: true
        Layout.alignment: Qt.AlignVCenter
        spacing: 2
        Text {
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: root.label
            color: root.enabled ? theme.foreground : theme.textFaint
            font.family: theme.fontFamily
            font.pixelSize: theme.type.body
            elide: Text.ElideRight
        }
        Text {
            Layout.fillWidth: true
            visible: root.description !== ""
            textFormat: Text.PlainText
            text: root.description
            color: theme.textMuted
            font.family: theme.fontFamily
            font.pixelSize: theme.type.caption
            wrapMode: Text.WordWrap
        }
    }
    RowLayout {
        id: trailingHolder
        Layout.alignment: Qt.AlignVCenter | Qt.AlignRight
        spacing: theme.space.md
    }
}
