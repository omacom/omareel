import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.lighterBackground
    border.color: Qt.alpha(theme.foreground, .06)
    property int section: 0
    readonly property var sections: [
        {name:"Background", icon:"background.svg"}, {name:"Shape", icon:"shape.svg"},
        {name:"Cursor", icon:"cursor.svg"}, {name:"Zoom", icon:"zoom.svg"},
        {name:"Audio", icon:"audio.svg"}
    ]
    RowLayout {
        anchors.fill: parent
        spacing: 0
        Rectangle {
            Layout.preferredWidth: 52
            Layout.minimumWidth: 52
            Layout.maximumWidth: 52
            Layout.fillHeight: true
            color: theme.background
            Column {
                width: parent.width
                topPadding: 8
                spacing: 4
                Repeater {
                    model: root.sections
                    delegate: IconToolButton {
                        id: railButton
                        required property var modelData
                        required property int index
                        anchors.horizontalCenter: parent.horizontalCenter
                        icon.source: "qrc:/omarecord/assets/icons/" + modelData.icon
                        toolIconColor: checked ? theme.accentForeground : theme.foreground
                        checked: root.section === index
                        checkable: true
                        Accessible.name: modelData.name
                        ToolTip.visible: hovered
                        ToolTip.text: modelData.name
                        onClicked: root.section = index
                        hoverColor: checked ? theme.accent : Qt.alpha(theme.foreground, .07)
                        background: Rectangle { radius: 7; color: railButton.checked ? theme.accent : railButton.hovered ? railButton.hoverColor : "transparent" }
                    }
                }
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0
            Label {
                Layout.fillWidth: true
                Layout.preferredHeight: 48
                leftPadding: 16
                verticalAlignment: Text.AlignVCenter
                text: root.sections[root.section].name
                font.weight: Font.DemiBold
                font.pixelSize: 16
            }
            ScrollView {
                id: scroller
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                contentWidth: availableWidth
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                Loader {
                    x: 16
                    width: scroller.availableWidth - 32
                    height: item ? item.implicitHeight : 0
                    sourceComponent: root.section === 0 ? backgroundPanel : root.section === 1 ? shapePanel : root.section === 2 ? cursorPanel : root.section === 3 ? zoomPanel : audioPanel
                }
            }
        }
    }
    Component { id: backgroundPanel; BackgroundPanel { } }
    Component { id: shapePanel; ShapePanel { } }
    Component { id: cursorPanel; CursorPanel { } }
    Component { id: zoomPanel; ZoomPanel { } }
    Component { id: audioPanel; AudioPanel { } }
}
