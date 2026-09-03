import QtQuick
import QtQuick.Controls

Item {
    id: root
    required property var pills
    required property var settings
    required property var colors
    property real ref: 1
    readonly property color pillColor: Qt.alpha(colors.background, .85)
    readonly property color textColor: colors.foreground
    anchors.fill: parent

    Column {
        id: stack
        spacing: 8 * root.ref
        anchors.left: root.settings.position.endsWith("left") ? parent.left : undefined
        anchors.horizontalCenter: root.settings.position.endsWith("center") ? parent.horizontalCenter : undefined
        anchors.right: root.settings.position.endsWith("right") ? parent.right : undefined
        anchors.top: root.settings.position.startsWith("top") ? parent.top : undefined
        anchors.bottom: root.settings.position.startsWith("bottom") ? parent.bottom : undefined
        anchors.margins: 40 * root.ref

        Repeater {
            model: root.pills
            delegate: Rectangle {
                id: pillDelegate
                required property var modelData
                height: 52 * root.settings.size * root.ref
                width: keysRow.implicitWidth + 24 * root.settings.size * root.ref
                radius: 10 * root.settings.size * root.ref
                color: root.pillColor
                opacity: modelData.opacity
                border.width: root.ref
                border.color: Qt.alpha(root.textColor, .12)

                Row {
                    id: keysRow
                    anchors.centerIn: parent
                    spacing: 6 * root.settings.size * root.ref
                    Repeater {
                        model: modelData.keys
                        delegate: Row {
                            required property string modelData
                            required property int index
                            spacing: 6 * root.settings.size * root.ref
                            Rectangle {
                                width: keyLabel.implicitWidth + 14 * root.settings.size * root.ref
                                height: 30 * root.settings.size * root.ref
                                radius: 6 * root.settings.size * root.ref
                                color: Qt.alpha(root.textColor, .10)
                                border.width: root.ref
                                border.color: Qt.alpha(root.textColor, .22)
                                Label {
                                    id: keyLabel
                                    anchors.centerIn: parent
                                    text: modelData
                                    color: root.textColor
                                    font.family: "monospace"
                                    font.pixelSize: 14 * root.settings.size * root.ref
                                    font.weight: Font.DemiBold
                                }
                            }
                            Label {
                                visible: index < pillDelegate.modelData.keys.length - 1
                                text: "+"
                                color: Qt.alpha(root.textColor, .65)
                                font.family: "monospace"
                                font.pixelSize: 13 * root.settings.size * root.ref
                                anchors.verticalCenter: parent.verticalCenter
                            }
                        }
                    }
                }
            }
        }
    }
}
