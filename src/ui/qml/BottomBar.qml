import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 56
    color: theme.surface
    property real timelineScale: 1
    property bool cropMode: false
    readonly property var aspects: [
        {text:"Auto", value:"auto"},
        {text:"Wide 16:9", value:"16:9"},
        {text:"Vertical 9:16", value:"9:16"},
        {text:"Square 1:1", value:"1:1"},
        {text:"Portrait 4:5", value:"4:5"},
        {text:"Classic 4:3", value:"4:3"},
        {text:"Tall 3:4", value:"3:4"}
    ]

    function aspectIndex(value) {
        for (let i = 0; i < aspects.length; ++i)
            if (aspects[i].value === value) return i
        return 0
    }
    function openAspectMenu() { aspectBox.popup.open() }

    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 1
        color: theme.hairline
    }
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
                EditorComboBox {
                    id: aspectBox
                    objectName: "aspectMenu"
                    Layout.preferredWidth: 136
                    model: root.aspects
                    textRole: "text"
                    currentIndex: root.aspectIndex(editor.project.aspect)
                    onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
                }
                EditorButton {
                    Layout.preferredWidth: 76
                    text: "Crop"
                    icon.source: "qrc:/omarecord/assets/icons/lucide/crop.svg"
                    checkable: true
                    checked: root.cropMode
                    selected: checked
                    onToggled: root.cropMode = checked
                }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 12
            IconToolButton {
                icon.source: "qrc:/omarecord/assets/icons/lucide/skip-back.svg"
                icon.width: 19
                icon.height: 19
                Accessible.name: "Go to start"
                onClicked: editor.seekBoundary(-1)
            }
            ToolButton {
                Layout.preferredWidth: 40
                Layout.preferredHeight: 40
                width: 40
                height: 40
                topInset: 0
                bottomInset: 0
                padding: 0
                hoverEnabled: true
                focusPolicy: Qt.TabFocus
                icon.source: editor.playing
                    ? "qrc:/omarecord/assets/icons/lucide/pause.svg"
                    : "qrc:/omarecord/assets/icons/lucide/play.svg"
                icon.width: 20
                icon.height: 20
                icon.color: theme.accentForeground
                Accessible.name: editor.playing ? "Pause" : "Play"
                onClicked: editor.playPause()
                background: Rectangle {
                    radius: theme.radius
                    color: parent.down ? theme.pressedFill
                         : parent.hovered ? theme.hoverFill : theme.selectedFill
                    border.width: 1
                    border.color: parent.hovered ? theme.hoverBorder : theme.normalBorder
                    Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: -3
                        radius: theme.radius
                        color: "transparent"
                        border.width: 2
                        border.color: theme.hoverBorder
                        visible: parent.parent.visualFocus
                    }
                }
            }
            IconToolButton {
                icon.source: "qrc:/omarecord/assets/icons/lucide/skip-forward.svg"
                icon.width: 19
                icon.height: 19
                Accessible.name: "Go to end"
                onClicked: editor.seekBoundary(1)
            }
            IconToolButton {
                Layout.leftMargin: 4
                icon.source: "qrc:/omarecord/assets/icons/lucide/scissors.svg"
                icon.width: 19
                icon.height: 19
                Accessible.name: "Split clip"
                ToolTip.visible: hovered
                ToolTip.text: "Split at playhead (S)"
                onClicked: editor.splitAtPlayhead()
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            RowLayout {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                IconToolButton {
                    Layout.preferredWidth: 20
                    Layout.preferredHeight: 20
                    width: 20
                    height: 20
                    icon.source: "qrc:/omarecord/assets/icons/lucide/zoom-out.svg"
                    icon.width: 16
                    icon.height: 16
                    Accessible.name: "Zoom timeline out"
                    onClicked: root.timelineScale = Math.max(1, root.timelineScale - .25)
                }
                EditorSlider {
                    from: 1
                    to: 5
                    value: root.timelineScale
                    Layout.preferredWidth: 104
                    Layout.preferredHeight: 28
                    onMoved: root.timelineScale = value
                    Accessible.name: "Timeline zoom"
                }
                IconToolButton {
                    Layout.preferredWidth: 20
                    Layout.preferredHeight: 20
                    width: 20
                    height: 20
                    icon.source: "qrc:/omarecord/assets/icons/lucide/zoom-in.svg"
                    icon.width: 16
                    icon.height: 16
                    Accessible.name: "Zoom timeline in"
                    onClicked: root.timelineScale = Math.min(5, root.timelineScale + .25)
                }
            }
        }
    }
}
