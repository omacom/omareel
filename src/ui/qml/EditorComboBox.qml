import QtQuick
import QtQuick.Controls

ComboBox {
    id: control
    implicitHeight: 32
    leftPadding: 10
    rightPadding: 30
    topPadding: 0
    bottomPadding: 0
    hoverEnabled: true
    focusPolicy: Qt.TabFocus
    font.pixelSize: 12
    font.weight: Font.Medium

    contentItem: Label {
        text: control.displayText
        color: control.enabled ? theme.foreground : theme.textFaint
        font: control.font
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    indicator: ToolButton {
        x: control.width - width - 10
        y: (control.height - height) / 2
        width: 14
        height: 14
        padding: 0
        enabled: false
        opacity: control.enabled ? .72 : .4
        icon.source: "qrc:/omarecord/assets/icons/lucide/chevron-down.svg"
        icon.width: 14
        icon.height: 14
        icon.color: theme.textMuted
        background: null
    }
    background: Rectangle {
        radius: 6
        color: control.down ? theme.hairlineStrong
                            : control.hovered ? theme.hairline : "transparent"
        border.width: 1
        border.color: control.activeFocus ? theme.accent : theme.hairlineStrong
        Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
    }
    delegate: ItemDelegate {
        id: row
        required property int index
        width: ListView.view ? ListView.view.width : control.width
        height: 32
        leftPadding: 32
        rightPadding: 10
        topPadding: 0
        bottomPadding: 0
        hoverEnabled: true
        contentItem: Label {
            text: control.textAt(row.index)
            color: control.currentIndex === row.index ? theme.accent : theme.foreground
            font.pixelSize: 12
            font.weight: control.currentIndex === row.index ? Font.DemiBold : Font.Medium
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        indicator: ToolButton {
            x: 10
            anchors.verticalCenter: parent.verticalCenter
            width: 14
            height: 14
            visible: control.currentIndex === row.index
            padding: 0
            enabled: false
            opacity: 1
            icon.source: "qrc:/omarecord/assets/icons/lucide/check.svg"
            icon.width: 14
            icon.height: 14
            icon.color: theme.accent
            background: null
        }
        background: Rectangle {
            radius: 6
            color: row.highlighted ? theme.hairline : "transparent"
        }
    }
    popup: Popup {
        y: control.height + 4
        width: control.width
        implicitHeight: Math.min(contentItem.implicitHeight + 8, 288)
        padding: 4
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: control.popup.visible ? control.delegateModel : null
            currentIndex: control.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator { }
        }
        background: Rectangle {
            radius: 10
            color: theme.surfaceRaised
            border.width: 1
            border.color: theme.hairlineStrong
        }
    }
}
