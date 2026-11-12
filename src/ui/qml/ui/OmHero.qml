import QtQuick
import QtQuick.Controls.impl
import QtQuick.Layouts

ColumnLayout {
    id: root
    property url iconSource
    property string title: ""
    property string meta: ""
    property Component trailingControl
    spacing: 3
    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Text {
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: root.title
            color: theme.textStrong
            font.family: theme.fontFamily
            font.pixelSize: theme.type.headline
            font.weight: Font.DemiBold
            elide: Text.ElideRight
        }
        Loader { sourceComponent: root.trailingControl; Layout.alignment: Qt.AlignVCenter }
    }
    Text {
        Layout.fillWidth: true
        visible: root.meta !== ""
        textFormat: Text.PlainText
        text: root.meta
        elide: Text.ElideRight
        color: theme.textMuted
        font.family: theme.fontFamily
        font.pixelSize: theme.type.small
    }
}
