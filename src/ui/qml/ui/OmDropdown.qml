import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts

ComboBox {
    id: control
    property url leadingIconSource
    implicitHeight: theme.space.controlHeight
    leftPadding: theme.space.controlPaddingX
    rightPadding: theme.space.controlPaddingX + 20
    topPadding: 0; bottomPadding: 0
    hoverEnabled: true; focusPolicy: Qt.TabFocus
    font.family: theme.fontFamily; font.pixelSize: theme.type.body; font.weight: Font.Medium
    contentItem: RowLayout {
        spacing: theme.space.md
        Item {
            visible: control.leadingIconSource.toString() !== ""
            Layout.preferredWidth: 15; Layout.preferredHeight: 15
            IconImage {
                anchors.fill: parent
                source: control.leadingIconSource
                sourceSize: Qt.size(15, 15)
                color: control.enabled ? theme.textMuted : theme.textFaint
            }
        }
        Text {
            Layout.fillWidth: true
            textFormat: Text.PlainText
            text: control.displayText
            color: control.enabled ? theme.textStrong : theme.textFaint
            font: control.font
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
    }
    indicator: IconImage {
        x: control.width - width - theme.space.controlPaddingX + 2
        anchors.verticalCenter: parent.verticalCenter
        width: 14; height: 14
        source: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
        sourceSize: Qt.size(14, 14)
        color: control.hovered || control.popup.visible ? theme.foreground : theme.textMuted
        rotation: control.popup.visible ? 180 : 0
        Behavior on rotation { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
    }
    background: Rectangle {
        radius: theme.controlRadius
        color: control.down ? theme.fillPressed : control.hovered || control.popup.visible ? theme.fillHover : theme.fill
        border.width: 1
        border.color: control.visualFocus ? Qt.alpha(theme.accent, .8) : theme.stroke
        Behavior on color { ColorAnimation { duration: 140 } }
    }
    delegate: ItemDelegate {
        id: row
        required property int index
        width: ListView.view ? ListView.view.width : control.width
        height: theme.space.popupRowHeight + 2
        leftPadding: 10; rightPadding: 10
        hoverEnabled: true
        contentItem: Item {
            Text {
                anchors.left: parent.left; anchors.right: tick.left; anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                textFormat: Text.PlainText
                text: control.textAt(row.index)
                color: control.currentIndex === row.index || row.highlighted ? theme.textStrong : theme.foreground
                font.family: theme.fontFamily; font.pixelSize: theme.type.body
                font.weight: control.currentIndex === row.index ? Font.Medium : Font.Normal
                elide: Text.ElideRight
            }
            IconImage {
                id: tick
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                width: 14; height: 14
                visible: control.currentIndex === row.index
                source: "qrc:/omareel/assets/icons/lucide/check.svg"
                sourceSize: Qt.size(14, 14)
                color: theme.accent
            }
        }
        background: Rectangle {
            radius: theme.controlRadius
            color: row.highlighted || row.hovered ? theme.fillHover : "transparent"
        }
    }
    popup: Popup {
        y: control.height + 4; width: Math.max(control.width, 160)
        implicitHeight: Math.min(contentItem.implicitHeight + 8, (theme.space.popupRowHeight + 2) * 9 + 8)
        padding: 4
        transformOrigin: Popup.Top
        contentItem: ListView { clip: true; implicitHeight: contentHeight; model: control.popup.visible ? control.delegateModel : null; currentIndex: control.highlightedIndex }
        background: OmPopupCard { padding: 0 }
        enter: Transition {
            NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 140; easing.type: Easing.OutCubic }
            NumberAnimation { property: "scale"; from: .96; to: 1; duration: 200; easing.type: Easing.OutQuint }
        }
        exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: 100 } }
    }
}
