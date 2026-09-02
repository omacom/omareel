import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    color: "#18191f"
    border.color: "#292b32"
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
            Layout.preferredWidth: 58
            Layout.fillHeight: true
            color: "#131419"
            Column {
                width: parent.width
                topPadding: 12
                Repeater {
                    model: root.sections
                    delegate: ToolButton {
                        required property var modelData
                        required property int index
                        width: 58; height: 52
                        icon.source: "qrc:/omarecord/assets/icons/" + modelData.icon
                        icon.width: 21; icon.height: 21
                        checked: root.section === index
                        checkable: true
                        Accessible.name: modelData.name
                        ToolTip.visible: hovered
                        ToolTip.text: modelData.name
                        onClicked: root.section = index
                        background: Rectangle { radius: 8; color: parent.checked ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.22) : parent.hovered ? "#272932" : "transparent" }
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
                Layout.preferredHeight: 52
                leftPadding: 16
                verticalAlignment: Text.AlignVCenter
                text: root.sections[root.section].name
                font.bold: true
                font.pixelSize: 16
            }
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                Loader {
                    x: 14
                    width: parent.width - 28
                    sourceComponent: root.section === 0 ? backgroundPanel : root.section === 1 ? shapePanel : root.section === 2 ? cursorPanel : root.section === 3 ? zoomPanel : audioPanel
                }
            }
        }
    }
    Component { id: backgroundPanel; BackgroundPanel { width: 254 } }
    Component { id: shapePanel; ShapePanel { width: 254 } }
    Component { id: cursorPanel; CursorPanel { width: 254 } }
    Component { id: zoomPanel; ZoomPanel { width: 254 } }
    Component { id: audioPanel; AudioPanel { width: 254 } }
}
