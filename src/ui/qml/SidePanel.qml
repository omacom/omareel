import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl as ControlsImpl
import QtQuick.Layouts

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.lighterBackground
    border.color: "#10ffffff"
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
                    delegate: ToolButton {
                        id: railButton
                        required property var modelData
                        required property int index
                        x: 4; width: 44; height: 44
                        icon.source: "qrc:/omarecord/assets/icons/" + modelData.icon
                        icon.width: 20; icon.height: 20
                        icon.color: checked ? theme.accentForeground : theme.foreground
                        contentItem: Item {
                            ControlsImpl.IconImage {
                                anchors.centerIn: parent
                                source: railButton.icon.source
                                width: 20; height: 20
                                sourceSize.width: 40; sourceSize.height: 40
                                color: railButton.checked ? theme.accentForeground : theme.foreground
                            }
                        }
                        checked: root.section === index
                        checkable: true
                        focusPolicy: Qt.TabFocus
                        Accessible.name: modelData.name
                        ToolTip.visible: hovered
                        ToolTip.text: modelData.name
                        onClicked: root.section = index
                        background: Rectangle {
                            radius: 7
                            color: parent.checked ? theme.accent : parent.hovered ? "#12ffffff" : "transparent"
                        }
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
