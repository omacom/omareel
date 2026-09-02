import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 62
    color: "#18191f"
    border.color: "#292b32"
    property real timelineScale: 1
    property bool cropMode: false
    readonly property var aspects: [
        {label:"Auto", value:"auto"}, {label:"Wide 16:9", value:"16:9"},
        {label:"Square 1:1", value:"1:1"}, {label:"Classic 4:3", value:"4:3"},
        {label:"Vertical 9:16", value:"9:16"}, {label:"Tall 3:4", value:"3:4"},
        {label:"Portrait 4:5", value:"4:5"}
    ]
    RowLayout {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 10
        anchors.rightMargin: 10
        ComboBox {
            Layout.preferredWidth: 145
            model: root.aspects
            textRole: "label"
            Component.onCompleted: {
                for (let i = 0; i < model.length; ++i) {
                    if (model[i].value === editor.project.aspect) currentIndex = i
                }
            }
            onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
        }
        ToolButton { text: "Crop"; checkable: true; checked: root.cropMode; onToggled: root.cropMode = checked }
        Item { Layout.fillWidth: true }
        ToolButton { icon.source: "qrc:/omarecord/assets/icons/skip-back.svg"; Accessible.name: "Go to start"; onClicked: editor.seekBoundary(-1) }
        ToolButton { icon.source: editor.playing ? "qrc:/omarecord/assets/icons/pause.svg" : "qrc:/omarecord/assets/icons/play.svg"; Accessible.name: editor.playing ? "Pause" : "Play"; onClicked: editor.playPause() }
        ToolButton { icon.source: "qrc:/omarecord/assets/icons/skip-forward.svg"; Accessible.name: "Go to end"; onClicked: editor.seekBoundary(1) }
        ToolButton { icon.source: "qrc:/omarecord/assets/icons/scissors.svg"; Accessible.name: "Split clip"; ToolTip.visible: hovered; ToolTip.text: "Split at playhead (S)"; onClicked: editor.splitAtPlayhead() }
        Item { Layout.fillWidth: true }
        Label { text: "Timeline"; color: "#92959f" }
        Slider { from: 1; to: 5; value: root.timelineScale; Layout.preferredWidth: 130; onMoved: root.timelineScale = value }
    }
}
