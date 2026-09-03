import QtQuick
import QtQuick.Layouts

Item {
    id: root
    property bool opened: false
    property string message: ""
    property string cancelText: "Cancel"
    property string confirmText: "Confirm"
    signal canceled()
    signal confirmed()
    visible: opened
    Rectangle { anchors.fill: parent; color: theme.menuScrim; MouseArea { anchors.fill: parent; onClicked: root.canceled() } }
    OmPopupCard {
        width: Math.min(parent.width - 32, 370)
        height: messageText.implicitHeight + buttons.implicitHeight + theme.space.popupPadding * 3 + 4
        anchors.centerIn: parent
        Text { id: messageText; width: parent.width; text: root.message; wrapMode: Text.WordWrap; color: theme.foreground; font.family: theme.fontFamily; font.pixelSize: theme.font.title }
        RowLayout {
            id: buttons; width: parent.width; anchors.bottom: parent.bottom; spacing: theme.space.xl
            Item { Layout.fillWidth: true }
            OmButton { text: root.cancelText; bordered: true; onClicked: root.canceled() }
            OmButton { text: root.confirmText; bordered: true; primary: true; destructive: true; onClicked: root.confirmed() }
        }
    }
}
