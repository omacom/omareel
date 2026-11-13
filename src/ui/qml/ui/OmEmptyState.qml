import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

// Centered icon, title, and hint for panels with nothing to edit yet.
ColumnLayout {
    id: empty
    property url iconSource
    property string title: ""
    property string hint: ""
    property string key: ""
    Layout.fillWidth: true
    spacing: 10
    Rectangle {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 8
        width: 40; height: 40
        radius: theme.rounded ? 20 : 0
        color: theme.fill
        border.width: 1
        border.color: theme.stroke
        IconImage {
            anchors.centerIn: parent
            width: 18; height: 18
            source: empty.iconSource
            sourceSize: Qt.size(18, 18)
            color: theme.textMuted
        }
    }
    Label {
        Layout.alignment: Qt.AlignHCenter
        textFormat: Text.PlainText
        text: empty.title
        color: theme.textStrong
        font.pixelSize: theme.type.body
        font.weight: Font.DemiBold
    }
    Row {
        visible: empty.key !== ""
        Layout.alignment: Qt.AlignHCenter
        Layout.bottomMargin: 8
        spacing: 5
        Label { anchors.verticalCenter: parent.verticalCenter; text: empty.hint; color: theme.textMuted; font.pixelSize: theme.type.small }
        OmKeycap { anchors.verticalCenter: parent.verticalCenter; text: empty.key }
    }
    Label {
        visible: empty.key === ""
        Layout.fillWidth: true
        Layout.leftMargin: 12; Layout.rightMargin: 12
        Layout.bottomMargin: 8
        textFormat: Text.PlainText
        text: empty.hint
        color: theme.textMuted
        font.pixelSize: theme.type.small
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
    }
}
