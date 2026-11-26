import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Omareel.Ui

Rectangle {
    id: root
    objectName: "sidePanel"
    color: theme.surface
    property int section: 0
    property int displayedSection: 0
    property string forcedTooltip: ""
    readonly property var sections: [
        {name:"Background", meta:"Canvas behind the video", icon:"palette.svg"},
        {name:"Shape", meta:"Frame, shadow, and border", icon:"square.svg"},
        {name:"Cursor", meta:"Pointer, clicks, and motion", icon:"mouse-pointer-2.svg"},
        {name:"Zoom", meta:"Focus and camera motion", icon:"zoom-in.svg"},
        {name:"Clip", meta:"Trim and speed", icon:"film.svg"},
        {name:"Camera", meta:"Webcam bubble", icon:"camera.svg"},
        {name:"Keystrokes", meta:"Shortcut overlay", icon:"keyboard.svg"},
        {name:"Audio", meta:"Tracks and volume", icon:"volume-2.svg"},
        {name:"Speech", meta:"Captions, pauses, and filler words", icon:"captions.svg"},
        {name:"Annotate", meta:"Text, boxes, arrows, blur, and cards", icon:"pen-line.svg"}
    ]

    onSectionChanged: {
        if (displayedSection !== section) {
            panelSwap.slideFrom = section > displayedSection ? 10 : -10
            panelSwap.restart()
        }
    }
    SequentialAnimation {
        id: panelSwap
        property real slideFrom: 10
        ParallelAnimation {
            NumberAnimation { target: panelLoader; property: "opacity"; to: 0; duration: 70; easing.type: Easing.InCubic }
        }
        ScriptAction { script: { root.displayedSection = root.section; panelLoader.slide = panelSwap.slideFrom } }
        ParallelAnimation {
            NumberAnimation { target: panelLoader; property: "opacity"; to: 1; duration: 180; easing.type: Easing.OutCubic }
            NumberAnimation { target: panelLoader; property: "slide"; to: 0; duration: 260; easing.type: Easing.OutQuint }
        }
    }

    function panelMeta(index) {
        return root.sections[index].meta
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
    function setBackgroundHoverProof(index) {
        root.section = 0
        Qt.callLater(function() {
            if (panelLoader.item) panelLoader.item.proofHoveredIndex = index
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
            id: rail
            Layout.preferredWidth: 56
            Layout.minimumWidth: 56
            Layout.maximumWidth: 56
            Layout.fillHeight: true
            color: theme.surface

            Rectangle {
                anchors.left: parent.left
                width: 1
                height: parent.height
                color: theme.hairline
            }
            Rectangle {
                anchors.right: parent.right
                width: 1
                height: parent.height
                color: theme.hairline
            }
            // One indicator glides between sections instead of each button lighting its own.
            Rectangle {
                id: railIndicator
                readonly property Item target: railRepeater.count > root.section ? railRepeater.itemAt(root.section) : null
                x: 0
                y: target ? railColumn.y + target.y + (target.height - height) / 2 : 0
                width: 2
                height: 18
                radius: 1
                color: theme.accent
                Behavior on y { NumberAnimation { duration: 280; easing.type: Easing.OutQuint } }
            }
            Column {
                id: railColumn
                y: 10
                width: parent.width
                spacing: 4
                Repeater {
                    id: railRepeater
                    model: root.sections
                    delegate: OmIconButton {
                        id: railButton
                        required property var modelData
                        required property int index
                        objectName: "railButton-" + modelData.name
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 36
                        height: 36
                        checkable: true
                        checked: root.section === index
                        icon.width: 17
                        icon.height: 17
                        icon.source: "qrc:/omareel/assets/icons/lucide/" + modelData.icon
                        icon.color: checked ? theme.accent : hovered ? theme.textStrong : theme.textMuted
                        Accessible.name: modelData.name
                        onClicked: root.section = index
                        tooltipText: modelData.name
                        tooltipPlacement: "left"
                        OmLazyToolTip {
                            target: railButton
                            placement: "left"
                            delay: 0
                            shown: root.forcedTooltip === railButton.modelData.name
                            text: railButton.modelData.name
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
            color: theme.surface

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                OmHero {
                    Layout.fillWidth: true
                    Layout.leftMargin: theme.space.panelPadding
                    Layout.rightMargin: theme.space.panelPadding
                    Layout.topMargin: 18
                    Layout.bottomMargin: 4
                    title: root.sections[root.section].name
                    meta: root.panelMeta(root.section)
                }
                ScrollView {
                    id: scroller
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    contentWidth: width
                    ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
                    ScrollBar.vertical: ScrollBar {
                        id: verticalBar
                        parent: scroller
                        x: scroller.width - width - 2
                        y: 2
                        height: scroller.availableHeight - 4
                        width: 8
                        padding: 2
                        policy: ScrollBar.AsNeeded
                        contentItem: Rectangle {
                            implicitWidth: 4
                            radius: theme.rounded ? 2 : 0
                            color: verticalBar.pressed ? theme.textMuted : theme.textFaint
                            opacity: verticalBar.active || verticalBar.hovered ? .9 : 0
                            Behavior on opacity { NumberAnimation { duration: 240 } }
                        }
                        background: Item {}
                    }
                    Item {
                        width: scroller.width
                        implicitHeight: panelLoader.item ? panelLoader.item.implicitHeight + 20 + theme.space.panelPadding * 2 : 0
                        height: implicitHeight
                        Loader {
                            id: panelLoader
                            property real slide: 0
                            x: theme.space.panelPadding + slide
                            y: 16
                            width: inspectorPanel.width - theme.space.panelPadding * 2
                            height: item ? item.implicitHeight : 0
                            sourceComponent: root.displayedSection === 0 ? backgroundPanel
                                : root.displayedSection === 1 ? shapePanel
                                : root.displayedSection === 2 ? cursorPanel
                                : root.displayedSection === 3 ? zoomPanel
                                : root.displayedSection === 4 ? clipPanel
                                : root.displayedSection === 5 ? cameraPanel
                                : root.displayedSection === 6 ? keystrokesPanel
                                : root.displayedSection === 7 ? audioPanel
                                : root.displayedSection === 8 ? speechPanel : overlayPanel
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
    Component { id: speechPanel; SpeechPanel { } }
    Component { id: overlayPanel; OverlayPanel { } }

    Connections {
        target: editor
        function onSelectionChanged() {
            if (editor.selectedOverlayId) root.section = 9
            else if (editor.selectedZoomId) root.section = 3
            else if (editor.selectedClipId) root.section = 4
        }
    }
}
