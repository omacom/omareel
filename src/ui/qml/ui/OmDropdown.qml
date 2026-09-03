import QtQuick
import QtQuick.Controls

ComboBox {
    id: control
    implicitHeight: theme.space.controlHeight
    leftPadding: theme.space.controlPaddingX
    rightPadding: theme.space.controlPaddingX + 18
    topPadding: 0; bottomPadding: 0
    hoverEnabled: true; focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily; font.pixelSize: theme.font.body
    contentItem: Text { text: control.displayText; color: control.enabled ? theme.foreground : theme.textFaint; font: control.font; verticalAlignment: Text.AlignVCenter; elide: Text.ElideRight }
    indicator: Text { x: control.width - width - theme.space.controlPaddingX; anchors.verticalCenter: parent.verticalCenter; text: "⌄"; color: theme.textMuted; font.family: theme.fontFamily; font.pixelSize: theme.font.body }
    background: Rectangle {
        radius: theme.radius
        color: control.down ? theme.pressedFill : theme.controlFill(control.visualFocus, control.hovered, false)
        border.width: theme.controlBorderWidth(control.visualFocus, control.hovered, false)
        border.color: theme.controlBorder(control.visualFocus, control.hovered, false)
        Behavior on color { ColorAnimation { duration: 120 } }
    }
    delegate: ItemDelegate {
        id: row
        required property int index
        width: ListView.view ? ListView.view.width : control.width
        height: theme.space.popupRowHeight
        leftPadding: theme.space.controlPaddingX; rightPadding: theme.space.controlPaddingX
        contentItem: Text { text: control.textAt(row.index); color: control.currentIndex === row.index ? theme.menuSelectedText : theme.menuText; font.family: theme.fontFamily; font.pixelSize: theme.font.body; verticalAlignment: Text.AlignVCenter; elide: Text.ElideRight }
        background: Rectangle { radius: theme.radius; color: row.highlighted || control.currentIndex === row.index ? theme.menuSelectedBackground : "transparent" }
    }
    popup: Popup {
        y: control.height + theme.space.xxs; width: control.width
        implicitHeight: Math.min(contentItem.implicitHeight + 4, theme.space.popupRowHeight * 8 + 4)
        padding: 2
        contentItem: ListView { clip: true; implicitHeight: contentHeight; model: control.popup.visible ? control.delegateModel : null; currentIndex: control.highlightedIndex }
        background: Rectangle { radius: theme.radius; color: theme.popupBackground; border.width: 2; border.color: theme.popupBorder }
    }
}
