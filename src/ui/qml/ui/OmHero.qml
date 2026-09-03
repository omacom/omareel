import QtQuick
import QtQuick.Layouts
import QtQuick.Effects

RowLayout {
    id: root
    property url iconSource
    property string title: ""
    property string meta: ""
    property Component trailingControl
    spacing: theme.space.xxxl
    implicitHeight: Math.max(theme.font.display, labels.implicitHeight)
    Item {
        Layout.preferredWidth: theme.font.display
        Layout.preferredHeight: theme.font.display
        Image {
            id: heroIcon
            anchors.fill: parent
            source: root.iconSource
            sourceSize: Qt.size(theme.font.display, theme.font.display)
            visible: false
        }
        MultiEffect {
            anchors.fill: parent
            source: heroIcon
            colorization: 1
            colorizationColor: theme.foreground
        }
    }
    ColumnLayout {
        id: labels
        Layout.fillWidth: true
        spacing: theme.space.xxs
        Text { text: root.title; color: theme.foreground; font.family: theme.fontFamily; font.pixelSize: theme.font.title; font.bold: true }
        Text {
            Layout.fillWidth: true; text: root.meta.toUpperCase(); elide: Text.ElideRight
            color: theme.textMuted; font.family: theme.fontFamily; font.pixelSize: theme.font.caption
            font.bold: true; font.letterSpacing: 1.2
        }
    }
    Loader { sourceComponent: root.trailingControl; Layout.alignment: Qt.AlignVCenter }
}
