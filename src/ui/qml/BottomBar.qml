import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 52
    color: theme.lighterBackground
    border.color: "#10ffffff"
    property real timelineScale: 1
    property bool cropMode: false
    readonly property var aspects: [
        {label:"Auto", value:"auto"}, {label:"Wide 16:9", value:"16:9"},
        {label:"Square 1:1", value:"1:1"}, {label:"Classic 4:3", value:"4:3"},
        {label:"Vertical 9:16", value:"9:16"}, {label:"Tall 3:4", value:"3:4"},
        {label:"Portrait 4:5", value:"4:5"}
    ]
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 8
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            RowLayout {
                anchors.left: parent.left
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                ComboBox {
                    Layout.preferredWidth: 148
                    Layout.preferredHeight: 32
                    model: root.aspects
                    textRole: "label"
                    font.pixelSize: 12
                    focusPolicy: Qt.TabFocus
                    Component.onCompleted: {
                        for (let i = 0; i < model.length; ++i) {
                            if (model[i].value === editor.project.aspect) currentIndex = i
                        }
                    }
                    onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
                }
                Button {
                    Layout.preferredWidth: 64
                    Layout.preferredHeight: 32
                    text: "Crop"
                    checkable: true
                    checked: root.cropMode
                    focusPolicy: Qt.TabFocus
                    font.pixelSize: 12
                    onToggled: root.cropMode = checked
                    background: Rectangle {
                        radius: 6
                        color: parent.checked ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, .22) : parent.hovered ? "#10ffffff" : "transparent"
                        border.color: parent.checked ? theme.accent : "#22ffffff"
                    }
                }
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 2
            ToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/skip-back.svg"; icon.width: 19; icon.height: 19; icon.color: theme.foreground
                focusPolicy: Qt.TabFocus; Accessible.name: "Go to start"; onClicked: editor.seekBoundary(-1)
                background: Rectangle { radius: 6; color: parent.hovered ? "#12ffffff" : "transparent" }
            }
            ToolButton {
                Layout.preferredWidth: 36; Layout.preferredHeight: 36
                icon.source: editor.playing ? "qrc:/omarecord/assets/icons/pause.svg" : "qrc:/omarecord/assets/icons/play.svg"
                icon.width: 20; icon.height: 20; icon.color: theme.accentForeground
                focusPolicy: Qt.TabFocus; Accessible.name: editor.playing ? "Pause" : "Play"; onClicked: editor.playPause()
                background: Rectangle { radius: 18; color: parent.pressed ? Qt.darker(theme.accent, 1.12) : theme.accent }
            }
            ToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/skip-forward.svg"; icon.width: 19; icon.height: 19; icon.color: theme.foreground
                focusPolicy: Qt.TabFocus; Accessible.name: "Go to end"; onClicked: editor.seekBoundary(1)
                background: Rectangle { radius: 6; color: parent.hovered ? "#12ffffff" : "transparent" }
            }
            Rectangle { Layout.preferredWidth: 1; Layout.preferredHeight: 20; Layout.leftMargin: 6; Layout.rightMargin: 6; color: "#18ffffff" }
            ToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/scissors.svg"; icon.width: 19; icon.height: 19; icon.color: theme.foreground
                focusPolicy: Qt.TabFocus; Accessible.name: "Split clip"; ToolTip.visible: hovered; ToolTip.text: "Split at playhead (S)"; onClicked: editor.splitAtPlayhead()
                background: Rectangle { radius: 6; color: parent.hovered ? "#12ffffff" : "transparent" }
            }
        }
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            RowLayout {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                Image {
                    source: "qrc:/omarecord/assets/icons/zoom.svg"
                    sourceSize.width: 40; sourceSize.height: 40
                    Layout.preferredWidth: 20; Layout.preferredHeight: 20
                    opacity: .68
                }
                Slider {
                    from: 1; to: 5; value: root.timelineScale
                    Layout.preferredWidth: 128; Layout.preferredHeight: 28
                    onMoved: root.timelineScale = value
                    Accessible.name: "Timeline zoom"
                }
            }
        }
    }
}
