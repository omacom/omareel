import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.lighterBackground
    border.color: Qt.alpha(theme.foreground, .06)
    property int section: 0
    property string forcedTooltip: ""
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
    readonly property var sections: [
        {name:"Background", icon:"palette.svg"}, {name:"Shape", icon:"square.svg"},
        {name:"Cursor", icon:"mouse-pointer-2.svg"}, {name:"Zoom", icon:"zoom-in.svg"},
        {name:"Clip", icon:"film.svg"}, {name:"Camera", icon:"camera.svg"},
        {name:"Keystrokes", icon:"keyboard.svg"}, {name:"Audio", icon:"volume-2.svg"}
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
                        property bool tooltipReady: false
                        objectName: "railButton-" + modelData.name
                        anchors.horizontalCenter: parent.horizontalCenter
                        icon.source: "qrc:/omarecord/assets/icons/lucide/" + modelData.icon
                        toolIconColor: checked ? theme.accentForeground : theme.foreground
                        checked: root.section === index
                        checkable: true
                        Accessible.name: modelData.name
                        onClicked: root.section = index
                        onHoveredChanged: {
                            if (hovered) tooltipDelay.restart()
                            else {
                                tooltipDelay.stop()
                                tooltipReady = false
                            }
                        }
                        hoverColor: checked ? theme.accent : Qt.alpha(theme.foreground, .07)
                        background: Rectangle {
                            radius: 8
                            color: railButton.checked ? Qt.alpha(theme.accent, .22)
                                : railButton.hovered ? railButton.hoverColor : "transparent"
                            border.width: railButton.checked ? 2 : 0
                            border.color: theme.accent
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
                            color: Qt.alpha(theme.darkBackground, .96)
                            border.width: 1
                            border.color: Qt.alpha(theme.foreground, .13)
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
                    id: panelLoader
                    x: 16
                    width: scroller.availableWidth - 32
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
