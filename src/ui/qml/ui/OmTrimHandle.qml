import QtQuick

Item {
    id: root

    property string edge: "left"
    property color accent: theme.accent
    property bool active: false
    property bool dragging: false
    property bool debugHovered: false
    property bool debugDragging: false
    property alias mouseArea: hitArea
    readonly property bool hovered: hitArea.containsMouse || debugHovered
    readonly property bool engaged: dragging || hitArea.pressed || debugDragging
    readonly property bool shown: active || hovered || engaged

    width: 14
    height: parent ? parent.height : 0
    z: 50

    // The cap is made only while it shows (a timeline has a handle at each end of every clip
    // and zoom, nearly all hidden until hovered), fading as before.
    property real capOpacity: shown ? 1 : 0
    Behavior on capOpacity {
        enabled: !editor.loading
        NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
    }
    Loader {
        x: 0
        y: 0
        width: 14
        height: root.height
        active: root.capOpacity > 0
        sourceComponent: capComponent
    }
    Component {
        id: capComponent
        Item {
            id: capVisual
            opacity: root.capOpacity
            readonly property color capColor: root.engaged ? Qt.lighter(root.accent, 1.20)
                                             : root.hovered ? Qt.lighter(root.accent, 1.12)
                                                            : root.accent

            Rectangle {
                id: roundedCap
                anchors.fill: parent
                radius: theme.controlRadius
                color: capVisual.capColor
                Behavior on color {
                    enabled: !editor.loading
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }
            Rectangle {
                x: root.edge === "left" ? parent.width / 2 : 0
                y: 0
                width: parent.width / 2
                height: parent.height
                color: capVisual.capColor
                Behavior on color {
                    enabled: !editor.loading
                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
            }

            Rectangle {
                anchors.centerIn: parent
                width: root.hovered || root.engaged ? 3 : 2
                height: root.height * (root.hovered || root.engaged ? .60 : .40)
                radius: width / 2
                color: theme.surface
                Behavior on width {
                    enabled: !editor.loading
                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                }
                Behavior on height {
                    enabled: !editor.loading
                    NumberAnimation { duration: 140; easing.type: Easing.OutCubic }
                }
            }
        }
    }

    MouseArea {
        id: hitArea
        objectName: root.objectName + "-hit"
        x: root.edge === "left" ? -6 : 0
        y: -6
        width: 20
        height: root.height + 12
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        preventStealing: true
        cursorShape: Qt.SizeHorCursor
    }
}
