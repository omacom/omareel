import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    height: 52
    color: theme.lighterBackground
    border.color: Qt.alpha(theme.foreground, .06)
    property real timelineScale: 1
    property bool cropMode: false
    readonly property var aspects: [
        {label:"Auto", caption:"Source proportions", value:"auto"},
        {label:"Wide 16:9", caption:"Landscape video and social", value:"16:9"},
        {label:"Vertical 9:16", caption:"Stories and short video", value:"9:16"},
        {label:"Square 1:1", caption:"Square feed posts", value:"1:1"},
        {label:"Portrait 4:5", caption:"Portrait feed posts", value:"4:5"},
        {label:"Classic 4:3", caption:"Classic landscape", value:"4:3"},
        {label:"Tall 3:4", caption:"Classic portrait", value:"3:4"}
    ]
    function aspectIndex(value) {
        for (let i = 0; i < aspects.length; ++i)
            if (aspects[i].value === value) return i
        return 0
    }
    function openAspectMenu() { aspectBox.popup.open() }
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
                    id: aspectBox
                    objectName: "aspectMenu"
                    Layout.preferredWidth: 220
                    Layout.preferredHeight: 32
                    model: root.aspects
                    textRole: "label"
                    font.pixelSize: 12
                    focusPolicy: Qt.TabFocus
                    currentIndex: root.aspectIndex(editor.project.aspect)
                    displayText: root.aspects[currentIndex].label
                    onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
                    delegate: ItemDelegate {
                        required property var modelData
                        width: ListView.view.width
                        height: 48
                        contentItem: Column {
                            leftPadding: 8
                            anchors.verticalCenter: parent.verticalCenter
                            Label { text: modelData.label; color: theme.foreground; font.pixelSize: 12; font.weight: Font.DemiBold }
                            Label { text: modelData.caption; color: Qt.alpha(theme.foreground, .52); font.pixelSize: 10 }
                        }
                        background: Rectangle {
                            radius: 6
                            color: parent.highlighted ? Qt.alpha(theme.accent, .16) : "transparent"
                            border.width: parent.highlighted ? 1 : 0
                            border.color: theme.accent
                        }
                    }
                    popup: Popup {
                        y: aspectBox.height + 4
                        width: 260
                        implicitHeight: contentItem.implicitHeight + 12
                        padding: 6
                        contentItem: ListView {
                            clip: true
                            implicitHeight: contentHeight
                            model: aspectBox.popup.visible ? aspectBox.delegateModel : null
                            currentIndex: aspectBox.highlightedIndex
                        }
                        background: Rectangle {
                            radius: 10; color: theme.lighterBackground
                            border.width: 1; border.color: Qt.alpha(theme.foreground, .16)
                        }
                    }
                }
                Button {
                    Layout.preferredWidth: 82
                    Layout.preferredHeight: 32
                    text: "Crop"
                    icon.source: "qrc:/omarecord/assets/icons/lucide/crop.svg"
                    icon.color: checked ? theme.accent : theme.foreground
                    icon.width: 16; icon.height: 16
                    checkable: true
                    checked: root.cropMode
                    focusPolicy: Qt.TabFocus
                    topInset: 0; bottomInset: 0
                    font.pixelSize: 12
                    onToggled: root.cropMode = checked
                    background: Rectangle {
                        radius: 6
                        color: parent.checked ? Qt.alpha(theme.accent, .22) : parent.hovered ? Qt.alpha(theme.foreground, .06) : "transparent"
                        border.color: parent.checked ? theme.accent : Qt.alpha(theme.foreground, .13)
                    }
                }
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 2
            IconToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/lucide/skip-back.svg"; icon.width: 19; icon.height: 19
                Accessible.name: "Go to start"; onClicked: editor.seekBoundary(-1)
            }
            ToolButton {
                Layout.preferredWidth: 36; Layout.preferredHeight: 36
                icon.source: editor.playing ? "qrc:/omarecord/assets/icons/lucide/pause.svg" : "qrc:/omarecord/assets/icons/lucide/play.svg"
                icon.width: 20; icon.height: 20; icon.color: theme.accentForeground
                focusPolicy: Qt.TabFocus; Accessible.name: editor.playing ? "Pause" : "Play"; onClicked: editor.playPause()
                topInset: 0; bottomInset: 0
                background: Rectangle { radius: 18; color: parent.pressed ? Qt.darker(theme.accent, 1.12) : theme.accent }
            }
            IconToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/lucide/skip-forward.svg"; icon.width: 19; icon.height: 19
                Accessible.name: "Go to end"; onClicked: editor.seekBoundary(1)
            }
            Rectangle { Layout.preferredWidth: 1; Layout.preferredHeight: 20; Layout.leftMargin: 6; Layout.rightMargin: 6; color: Qt.alpha(theme.foreground, .09) }
            IconToolButton {
                Layout.preferredWidth: 32; Layout.preferredHeight: 32
                icon.source: "qrc:/omarecord/assets/icons/lucide/scissors.svg"; icon.width: 19; icon.height: 19
                Accessible.name: "Split clip"; ToolTip.visible: hovered; ToolTip.text: "Split at playhead (S)"; onClicked: editor.splitAtPlayhead()
            }
        }
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            RowLayout {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                ToolButton {
                    enabled: false
                    opacity: .68
                    icon.source: "qrc:/omarecord/assets/icons/lucide/zoom-in.svg"
                    icon.color: theme.foreground
                    icon.width: 20; icon.height: 20
                    Layout.preferredWidth: 20; Layout.preferredHeight: 20
                    background: null
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
