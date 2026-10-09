import QtQuick

Item {
    id: root

    property string edge: "left"
    property color accent: theme.accent
    property bool active: false
    property bool dragging: false
    property bool debugHovered: false
    property bool debugDragging: false
    property bool compact: false
    property alias mouseArea: hitArea
    readonly property bool hovered: hitArea.containsMouse || debugHovered
    readonly property bool engaged: dragging || hitArea.pressed || debugDragging
    readonly property bool shown: active || hovered || engaged

    width: compact ? 3 : 14
    height: parent ? parent.height : 0
    z: 50

    Item {
        id: capVisual
        x: 0
        y: 0
        width: root.width
        height: root.height
        opacity: root.shown ? 1 : 0
        visible: opacity > 0
        readonly property color capColor: root.engaged ? Qt.lighter(root.accent, 1.20)
                                         : root.hovered ? Qt.lighter(root.accent, 1.12)
                                                        : root.accent

        Behavior on opacity {
            enabled: !editor.loading
            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
        }
        Rectangle {
            id: roundedCap
            anchors.fill: parent
            radius: root.compact ? 0 : theme.radius
            color: capVisual.capColor
            Behavior on color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }
        Rectangle {
            visible: !root.compact
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
            visible: !root.compact
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

    MouseArea {
        id: hitArea
        objectName: root.objectName + "-hit"
        x: root.edge === "left" ? -6 : root.compact ? -11 : 0
        y: -6
        width: 20
        height: root.height + 12
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        preventStealing: true
        cursorShape: Qt.SizeHorCursor
    }
}
