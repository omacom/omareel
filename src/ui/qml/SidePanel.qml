import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.surfaceRaised
    property int section: 0
    property string forcedTooltip: ""
    readonly property var sections: [
        {name:"Background", caption:"Set the canvas behind your recording", icon:"palette.svg"},
        {name:"Shape", caption:"Frame, spacing, and edge treatment", icon:"square.svg"},
        {name:"Cursor", caption:"Pointer style, motion, and clicks", icon:"mouse-pointer-2.svg"},
        {name:"Zoom", caption:"Focus areas and camera motion", icon:"zoom-in.svg"},
        {name:"Clip", caption:"Timing, speed, and trims", icon:"film.svg"},
        {name:"Camera", caption:"Webcam placement and appearance", icon:"camera.svg"},
        {name:"Keystrokes", caption:"Show keyboard activity on screen", icon:"keyboard.svg"},
        {name:"Audio", caption:"Recorded tracks and output volume", icon:"volume-2.svg"}
    ]

    function setBackgroundExpanded(value) {
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
                    delegate: ToolButton {
                        id: railButton
                        required property var modelData
                        required property int index
                        property bool tooltipReady: false
                        objectName: "railButton-" + modelData.name
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 40
                        height: 40
                        padding: 0
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        checkable: true
                        checked: root.section === index
                        icon.width: 20
                        icon.height: 20
                        icon.source: "qrc:/omarecord/assets/icons/lucide/" + modelData.icon
                        icon.color: checked ? theme.accent : theme.textMuted
                        Accessible.name: modelData.name
                        onClicked: root.section = index
                        onHoveredChanged: {
                            if (hovered) tooltipDelay.restart()
                            else { tooltipDelay.stop(); tooltipReady = false }
                        }
                        background: Item {
                            Rectangle {
                                anchors.fill: parent
                                radius: 10
                                color: railButton.checked ? theme.accentSoft
                                     : railButton.down ? theme.hairlineStrong
                                     : railButton.hovered ? theme.hairline : "transparent"
                                border.width: railButton.hovered && !railButton.checked ? 1 : 0
                                border.color: theme.hairlineStrong
                                Behavior on color { ColorAnimation { duration: 120; easing.type: Easing.OutCubic } }
                            }
                            Rectangle {
                                x: -8
                                anchors.verticalCenter: parent.verticalCenter
                                width: 2
                                height: 24
                                radius: 1
                                color: theme.accent
                                visible: railButton.checked
                            }
                            Rectangle {
                                anchors.fill: parent
                                anchors.margins: -3
                                radius: 13
                                color: "transparent"
                                border.width: 2
                                border.color: theme.accent
                                visible: railButton.visualFocus
                            }
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
                            radius: 7
                            color: theme.surfaceRaised
                            border.width: 1
                            border.color: theme.hairlineStrong
                            z: 1000
                            Label {
                                id: tooltipLabel
                                anchors.centerIn: parent
                                text: modelData.name
                                color: theme.foreground
                                font.pixelSize: 11
                                font.weight: Font.DemiBold
                            }
                        }
                    }
                }
            }
        }

        Rectangle {
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
                    Layout.preferredHeight: 84
                    Layout.minimumHeight: 84
                    Layout.maximumHeight: 84
                    Layout.leftMargin: 20
                    Layout.rightMargin: 20
                    spacing: 2
                    Item { Layout.preferredHeight: 13 }
                    Label {
                        Layout.fillWidth: true
                        text: root.sections[root.section].name
                        color: theme.foreground
                        font.weight: Font.DemiBold
                        font.pixelSize: 17
                    }
                    Label {
                        Layout.fillWidth: true
                        text: root.sections[root.section].caption
                        color: theme.textMuted
                        font.pixelSize: 11
                        elide: Text.ElideRight
                    }
                    Item { Layout.fillHeight: true }
                    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
                }
                ScrollView {
                    id: scroller
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    contentWidth: availableWidth
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    ScrollBar.vertical.width: 6
                    Item {
                        width: scroller.availableWidth
                        implicitHeight: panelLoader.item ? panelLoader.item.implicitHeight + 40 : 0
                        height: implicitHeight
                        Loader {
                            id: panelLoader
                            x: 20
                            y: 20
                            width: parent.width - 40
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
