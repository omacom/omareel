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
    property bool releasing: false

    width: 14
    height: parent ? parent.height : 0
    z: 50

    onEngagedChanged: {
        if (!engaged) {
            releasing = true
            releaseTimer.restart()
        }
    }

    Timer {
        id: releaseTimer
        interval: 180
        onTriggered: root.releasing = false
    }

    Item {
        id: capVisual
        x: 0
        y: -2
        width: 14
        height: root.height + 4
        opacity: root.shown ? 1 : 0
        visible: opacity > 0
        scale: root.engaged ? 1.12 : root.hovered ? 1.08 : 1
        transformOrigin: Item.Center

        Behavior on opacity {
            enabled: !editor.loading
            NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
        }
        Behavior on scale {
            enabled: !editor.loading
            NumberAnimation {
                duration: root.releasing ? 180 : 140
                easing.type: root.releasing ? Easing.OutBack : Easing.OutCubic
                easing.overshoot: 1.6
            }
        }

        Rectangle {
            anchors.fill: cap
            anchors.margins: -2
            radius: cap.radius + 2
            color: "transparent"
            border.width: 1
            border.color: Qt.alpha(root.accent, .35)
            opacity: root.engaged ? 1 : 0
            Behavior on opacity {
                enabled: !editor.loading
                NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }

        Rectangle {
            id: cap
            anchors.fill: parent
            // Deliberate exception: a square theme still gets a 3 px cap radius.
            radius: Math.max(3, theme.radius)
            color: root.hovered || root.engaged ? Qt.lighter(root.accent, 1.08) : root.accent
            Behavior on color {
                enabled: !editor.loading
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }

            Rectangle {
                anchors.centerIn: parent
                width: root.engaged ? 3 : 2
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
