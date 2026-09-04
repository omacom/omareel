import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Omarecord.Ui

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.surfaceRaised
    property int section: 0
    property string forcedTooltip: ""
    readonly property var sections: [
        {name:"Background", meta:"", icon:"palette.svg"},
        {name:"Shape", meta:"FRAME & SHADOW", icon:"square.svg"},
        {name:"Cursor", meta:"POINTER & CLICKS", icon:"mouse-pointer-2.svg"},
        {name:"Zoom", meta:"FOCUS & MOTION", icon:"zoom-in.svg"},
        {name:"Clip", meta:"TRIM & SPEED", icon:"film.svg"},
        {name:"Camera", meta:"WEBCAM BUBBLE", icon:"camera.svg"},
        {name:"Keystrokes", meta:"SHORTCUT OVERLAY", icon:"keyboard.svg"},
        {name:"Audio", meta:"TRACKS & VOLUME", icon:"volume-2.svg"}
    ]

    function panelMeta(index) {
        return index === 0 ? "CANVAS BEHIND THE VIDEO" : root.sections[index].meta
    }

    function setBackgroundExpanded(value) {
        root.section = 0
        Qt.callLater(function() {
            if (panelLoader.item) panelLoader.item.showAllWallpapers = value
        })
    }
    function setGradientExpanded(value) {
        root.section = 0
        Qt.callLater(function() {
            if (panelLoader.item) panelLoader.item.showAllGradients = value
        })
    }
    function scrollInspectorToBottom() {
        Qt.callLater(function() {
            scroller.contentItem.contentY = Math.max(0,
                scroller.contentItem.contentHeight - scroller.availableHeight)
        })
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        Rectangle {
            Layout.preferredWidth: 56
            Layout.minimumWidth: 56
            Layout.maximumWidth: 56
            Layout.fillHeight: true
            color: theme.surfaceRaised

            Rectangle {
                anchors.right: parent.right
                width: 1
                height: parent.height
                color: theme.hairline
            }
            Column {
                width: parent.width
                topPadding: 8
                spacing: 4
                Repeater {
                    model: root.sections
                    delegate: OmIconButton {
                        id: railButton
                        required property var modelData
                        required property int index
                        property bool tooltipReady: false
                        objectName: "railButton-" + modelData.name
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: theme.space.controlHeight
                        height: theme.space.controlHeight
                        checkable: true
                        checked: root.section === index
                        icon.width: 14
                        icon.height: 14
                        icon.source: "qrc:/omarecord/assets/icons/lucide/" + modelData.icon
                        icon.color: checked ? theme.accent : theme.textMuted
                        Accessible.name: modelData.name
                        onClicked: root.section = index
                        onHoveredChanged: {
                            if (hovered) tooltipDelay.restart()
                            else { tooltipDelay.stop(); tooltipReady = false }
                        }
                        Rectangle {
                            x: -14
                            anchors.verticalCenter: parent.verticalCenter
                            width: 2
                            height: 20
                            color: theme.foreground
                            visible: railButton.checked
                        }
                        Timer {
                            id: tooltipDelay
                            interval: 300
                            onTriggered: railButton.tooltipReady = true
                        }
                        Rectangle {
                            visible: root.forcedTooltip === modelData.name
                                || (railButton.hovered && railButton.tooltipReady)
                            anchors.right: parent.left
                            anchors.rightMargin: 8
                            anchors.verticalCenter: parent.verticalCenter
                            width: tooltipLabel.implicitWidth + 18
                            height: 28
                            radius: theme.radius
                            color: theme.tooltipBackground
                            border.width: 1
                            border.color: theme.tooltipBorder
                            z: 1000
                            Label {
                                id: tooltipLabel
                                anchors.centerIn: parent
                                text: modelData.name
                                color: theme.foreground
                                font.pixelSize: theme.font.bodySmall
                                font.weight: Font.DemiBold
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
            id: inspectorPanel
            Layout.preferredWidth: 320
            Layout.minimumWidth: 320
            Layout.maximumWidth: 320
            Layout.fillHeight: true
            color: theme.surfaceRaised

            Rectangle {
                anchors.left: parent.left
                width: 1
                height: parent.height
                color: theme.hairline
            }
            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 88
                    Layout.minimumHeight: 88
                    Layout.maximumHeight: 88
                    Layout.leftMargin: theme.space.panelPadding
                    Layout.rightMargin: theme.space.panelPadding
                    spacing: theme.space.sm
                    Item { Layout.preferredHeight: theme.space.lg }
                    OmHero {
                        Layout.fillWidth: true
                        iconSource: "qrc:/omarecord/assets/icons/lucide/" + root.sections[root.section].icon
                        title: root.sections[root.section].name
                        meta: root.panelMeta(root.section)
                    }
                    Item { Layout.fillHeight: true }
                    OmSeparator { Layout.fillWidth: true }
                }
                ScrollView {
                    id: scroller
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    contentWidth: width
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    ScrollBar.vertical.width: 6
                    Item {
                        width: scroller.width
                        implicitHeight: panelLoader.item ? panelLoader.item.implicitHeight + theme.space.panelPadding * 2 : 0
                        height: implicitHeight
                        Loader {
                            id: panelLoader
                            x: theme.space.panelPadding
                            y: theme.space.panelPadding
                            width: inspectorPanel.width - theme.space.panelPadding * 2
                            height: item ? item.implicitHeight : 0
                            sourceComponent: root.section === 0 ? backgroundPanel
                                : root.section === 1 ? shapePanel
                                : root.section === 2 ? cursorPanel
                                : root.section === 3 ? zoomPanel
                                : root.section === 4 ? clipPanel
                                : root.section === 5 ? cameraPanel
                                : root.section === 6 ? keystrokesPanel : audioPanel
                        }
                    }
                }
            }
        }
    }

    Component { id: backgroundPanel; BackgroundPanel { } }
    Component { id: shapePanel; ShapePanel { } }
    Component { id: cursorPanel; CursorPanel { } }
    Component { id: zoomPanel; ZoomPanel { } }
    Component { id: clipPanel; ClipPanel { } }
    Component { id: cameraPanel; CameraPanel { } }
    Component { id: keystrokesPanel; KeystrokesPanel { } }
    Component { id: audioPanel; AudioPanel { } }

    Connections {
        target: editor
        function onSelectionChanged() {
            if (editor.selectedZoomId) root.section = 3
            else if (editor.selectedClipId) root.section = 4
        }
    }
}
