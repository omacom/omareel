import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Omareel.Ui

Rectangle {
    id: root
    height: 56
    color: theme.surface
    property real timelineScale: 1
    signal timelineScaleRequested(real value)
    property bool cropMode: false
    readonly property bool compact: width < 1100
    readonly property bool narrow: width < 900
    readonly property var aspects: [
        {text:"Auto", value:"auto"}, {text:"Wide 16:9", value:"16:9"},
        {text:"Vertical 9:16", value:"9:16"}, {text:"Square 1:1", value:"1:1"},
        {text:"Portrait 4:5", value:"4:5"}, {text:"Classic 4:3", value:"4:3"},
        {text:"Tall 3:4", value:"3:4"}
    ]
    readonly property var shortcuts: [
        {key:"Space", action:"Play / pause"}, {key:"← / →", action:"Step one frame"},
        {key:"Shift ← / →", action:"Jump one second"}, {key:"S", action:"Split at playhead"},
        {key:"Z", action:"Add zoom"}, {key:"Delete", action:"Remove selection"},
        {key:"Backspace", action:"Remove range"},
        {key:"Ctrl Z", action:"Undo"}, {key:"Ctrl Shift Z", action:"Redo"},
        {key:"Ctrl S", action:"Save"}, {key:"Ctrl E", action:"Export"},
        {key:"Scroll", action:"Zoom timeline at pointer"},
        {key:"Right drag", action:"Pan timeline"},
        {key:"Esc", action:"Cancel point picking"}, {key:"?", action:"Show shortcuts"}
    ]

    function aspectIndex(value) {
        for (let i = 0; i < aspects.length; ++i)
            if (aspects[i].value === value) return i
        return 0
    }
    function shortAspect(value) { return value === "auto" ? "Auto" : value }
    function openAspectMenu() {
        if (root.narrow) overflowMenu.open()
        else aspectBox.popup.open()
    }
    function openShortcuts() { shortcutsSheet.open() }

    Rectangle {
        anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
        height: 1; color: theme.hairline
    }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12; anchors.rightMargin: 12
        spacing: 8

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: root.narrow ? 36 : root.compact ? 196 : 228
            Layout.fillHeight: true
            RowLayout {
                anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                EditorComboBox {
                    id: aspectBox
                    objectName: "aspectMenu"
                    visible: !root.narrow
                    Layout.preferredWidth: root.compact ? 112 : 136
                    Layout.minimumWidth: Layout.preferredWidth
                    leadingIconSource: "qrc:/omareel/assets/icons/lucide/square-dashed.svg"
                    model: root.aspects
                    textRole: "text"
                    displayText: root.compact
                        ? root.shortAspect(root.aspects[Math.max(0, currentIndex)].value)
                        : root.aspects[Math.max(0, currentIndex)].text
                    currentIndex: root.aspectIndex(editor.project.aspect)
                    onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
                }
                EditorButton {
                    visible: !root.narrow
                    Layout.preferredWidth: 76; Layout.minimumWidth: 76
                    text: "Crop"
                    icon.source: "qrc:/omareel/assets/icons/lucide/crop.svg"
                    checkable: true; checked: root.cropMode; selected: checked
                    onToggled: root.cropMode = checked
                }
                OmIconButton {
                    id: overflowButton
                    visible: root.narrow
                    width: 32; height: 32
                    text: "…"
                    bordered: true
                    tooltipText: "Canvas options"
                    Accessible.name: "Canvas options"
                    onClicked: overflowMenu.visible ? overflowMenu.close() : overflowMenu.open()
                }
            }
        }

        RowLayout {
            Layout.minimumWidth: 188; Layout.preferredWidth: 188; Layout.maximumWidth: 188
            Layout.alignment: Qt.AlignHCenter
            spacing: 10
            IconToolButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/skip-back.svg"
                icon.width: 19; icon.height: 19
                Accessible.name: "Go to start"
                onClicked: editor.seekBoundary(-1)
            }
            OmButton {
                Layout.preferredWidth: 40; Layout.preferredHeight: 40
                width: 40; height: 40
                leftPadding: 0; rightPadding: 0; topPadding: 0; bottomPadding: 0
                bordered: true; active: true
                icon.source: editor.playing
                    ? "qrc:/omareel/assets/icons/lucide/pause.svg"
                    : "qrc:/omareel/assets/icons/lucide/play.svg"
                icon.width: 20; icon.height: 20; icon.color: theme.foreground
                Accessible.name: editor.playing ? "Pause" : "Play"
                onClicked: editor.playPause()
            }
            IconToolButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/skip-forward.svg"
                icon.width: 19; icon.height: 19
                Accessible.name: "Go to end"
                onClicked: editor.seekBoundary(1)
            }
            IconToolButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/scissors.svg"
                icon.width: 19; icon.height: 19
                Accessible.name: "Split clip"
                ToolTip.visible: hovered
                ToolTip.text: "Split at playhead (S)"
                onClicked: editor.splitAtPlayhead()
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: root.compact ? 174 : 270
            Layout.fillHeight: true
            RowLayout {
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                spacing: 7
                Label {
                    visible: !root.compact
                    text: "Timeline"
                    color: theme.textMuted
                    font.pixelSize: theme.font.caption
                }
                IconToolButton {
                    objectName: "timelineZoomOut"
                    Layout.preferredWidth: 20; Layout.preferredHeight: 20
                    width: 20; height: 20
                    icon.source: "qrc:/omareel/assets/icons/lucide/zoom-out.svg"
                    icon.width: 16; icon.height: 16
                    Accessible.name: "Zoom timeline out"
                    onClicked: root.timelineScaleRequested(Math.max(1, root.timelineScale / 1.25))
                }
                EditorSlider {
                    objectName: "timelineZoomSlider"
                    from: 1; to: 16; value: root.timelineScale
                    Layout.preferredWidth: root.compact ? 82 : 104
                    Layout.minimumWidth: Layout.preferredWidth
                    Layout.preferredHeight: 28
                    onMoved: root.timelineScaleRequested(value)
                    Accessible.name: "Timeline zoom"
                }
                IconToolButton {
                    objectName: "timelineZoomIn"
                    Layout.preferredWidth: 20; Layout.preferredHeight: 20
                    width: 20; height: 20
                    icon.source: "qrc:/omareel/assets/icons/lucide/zoom-in.svg"
                    icon.width: 16; icon.height: 16
                    Accessible.name: "Zoom timeline in"
                    onClicked: root.timelineScaleRequested(Math.min(16, root.timelineScale * 1.25))
                }
                OmIconButton {
                    width: 28; height: 28
                    text: "?"
                    bordered: false
                    tooltipText: "Keyboard shortcuts"
                    Accessible.name: "Keyboard shortcuts"
                    onClicked: shortcutsSheet.visible ? shortcutsSheet.close() : shortcutsSheet.open()
                }
            }
        }
    }

    Popup {
        id: overflowMenu
        parent: root
        x: 12; y: -height - 8
        width: 224
        padding: 10
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: OmPopupCard { padding: 0 }
        contentItem: ColumnLayout {
            spacing: 4
            PanelHeading { text: "Canvas"; Layout.bottomMargin: 4 }
            Repeater {
                model: root.aspects
                delegate: OmButton {
                    required property var modelData
                    Layout.fillWidth: true
                    bordered: false
                    selected: editor.project.aspect === modelData.value
                    text: modelData.text
                    onClicked: {
                        editor.setProjectValue("aspect", modelData.value)
                        overflowMenu.close()
                    }
                }
            }
            OmSeparator { Layout.fillWidth: true; Layout.topMargin: 4; Layout.bottomMargin: 4 }
            OmButton {
                Layout.fillWidth: true
                bordered: true
                selected: root.cropMode
                text: root.cropMode ? "Finish crop" : "Crop"
                icon.source: "qrc:/omareel/assets/icons/lucide/crop.svg"
                onClicked: { root.cropMode = !root.cropMode; overflowMenu.close() }
            }
        }
    }

    Popup {
        id: shortcutsSheet
        objectName: "shortcutsSheet"
        parent: root
        x: Math.max(12, root.width - width - 12)
        y: -height - 8
        width: Math.min(620, root.width - 24)
        padding: 18
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: OmPopupCard { padding: 0 }
        contentItem: ColumnLayout {
            spacing: 14
            RowLayout {
                Layout.fillWidth: true
                Label {
                    text: "Keyboard shortcuts"
                    color: theme.foreground
                    font.pixelSize: theme.font.title
                    font.weight: Font.DemiBold
                }
                Item { Layout.fillWidth: true }
                OmIconButton {
                    icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
                    Accessible.name: "Close shortcuts"
                    onClicked: shortcutsSheet.close()
                }
            }
            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 24; rowSpacing: 8
                Repeater {
                    model: root.shortcuts
                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        spacing: 8
                        Label {
                            Layout.fillWidth: true
                            text: modelData.action
                            color: theme.textMuted
                            font.pixelSize: theme.font.bodySmall
                        }
                        Rectangle {
                            Layout.preferredWidth: shortcutKey.implicitWidth + 12
                            Layout.preferredHeight: 24
                            radius: Math.min(4, theme.radius)
                            color: theme.normalFill
                            border.width: 1; border.color: theme.normalBorder
                            Label {
                                id: shortcutKey
                                anchors.centerIn: parent
                                text: modelData.key
                                color: theme.foreground
                                font.family: theme.monoFamily
                                font.pixelSize: theme.font.caption
                                font.weight: Font.DemiBold
                            }
                        }
                    }
                }
            }
        }
    }
}
