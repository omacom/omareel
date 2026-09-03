import QtQuick
import QtQuick.Controls
import QtQuick.Shapes

FocusScope {
    id: root
    required property real scaleFactor
    signal scaleFactorRequested(real value)
    implicitHeight: editor.hasCamera ? 204 : 148
    property real labelWidth: 72
    property real basePixels: Math.max(55, (width - labelWidth - 24) / Math.max(1, editor.duration))
    property real pixelsPerSecond: basePixels * scaleFactor
    property int ticksPerSecond: pixelsPerSecond >= 120 ? 5 : pixelsPerSecond >= 72 ? 2 : 1
    readonly property int trackCount: editor.hasCamera ? 3 : 2
    readonly property real tracksBottom: 32 + trackCount * 48 + (trackCount - 1) * 8

    Rectangle { anchors.fill: parent; color: theme.surface }
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        height: 1
        color: theme.hairline
    }
    HoverHandler { id: timelineHover }

    Item {
        x: 0
        y: 0
        width: root.labelWidth
        height: parent.height
        Repeater {
            model: editor.hasCamera ? ["Clip", "Zoom", "Camera"] : ["Clip", "Zoom"]
            delegate: Label {
                required property string modelData
                required property int index
                x: 0
                y: 32 + index * 56
                width: root.labelWidth
                height: 48
                leftPadding: 14
                verticalAlignment: Text.AlignVCenter
                text: modelData
                color: theme.textFaint
                font.pixelSize: 11
                font.weight: Font.Medium
                font.capitalization: Font.AllUppercase
                font.letterSpacing: .6
            }
        }
        Rectangle {
            anchors.right: parent.right
            width: 1
            height: root.tracksBottom
            color: theme.hairline
        }
    }

    Flickable {
        id: flick
        objectName: "timelineFlickable"
        x: root.labelWidth
        y: 0
        width: parent.width - x
        height: parent.height
        contentWidth: Math.max(width, editor.duration * root.pixelsPerSecond + 40)
        contentHeight: height
        clip: true
        interactive: false
        boundsBehavior: Flickable.StopAtBounds

        WheelHandler {
            target: null
            onWheel: event => {
                if (event.modifiers & Qt.ControlModifier) {
                    const pointerX = event.position.x
                    const timeAtPointer = (flick.contentX + pointerX) / root.pixelsPerSecond
                    const delta = event.angleDelta.y !== 0 ? event.angleDelta.y : event.pixelDelta.y
                    const next = Math.max(1, Math.min(5, root.scaleFactor * Math.pow(1.12, delta / 120)))
                    root.scaleFactorRequested(next)
                    Qt.callLater(function() {
                        flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width,
                            timeAtPointer * root.pixelsPerSecond - pointerX))
                    })
                } else {
                    const pixel = Math.abs(event.pixelDelta.x) > Math.abs(event.pixelDelta.y)
                        ? event.pixelDelta.x : event.pixelDelta.y
                    const angle = Math.abs(event.angleDelta.x) > Math.abs(event.angleDelta.y)
                        ? event.angleDelta.x : event.angleDelta.y
                    const amount = pixel !== 0 ? pixel : angle / 120 * 72
                    flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width,
                        flick.contentX - amount))
                }
                event.accepted = true
            }
        }
        readonly property int firstVisibleTick: Math.max(0,
            Math.floor(contentX * root.ticksPerSecond / root.pixelsPerSecond) - 1)
        readonly property int visibleTickCount: Math.ceil(width * root.ticksPerSecond
                                                          / root.pixelsPerSecond) + 4

        Item {
            id: content
            width: flick.contentWidth
            height: flick.height
            MouseArea { anchors.fill: parent; onPressed: root.forceActiveFocus() }

            Item {
                id: ruler
                width: parent.width
                height: 32
                Repeater {
                    model: flick.visibleTickCount
                    delegate: Item {
                        id: tickDelegate
                        required property int index
                        property int tickIndex: flick.firstVisibleTick + index
                        property real tickTime: tickIndex / root.ticksPerSecond
                        property bool major: tickIndex % root.ticksPerSecond === 0
                        visible: tickTime <= editor.duration
                        x: tickTime * root.pixelsPerSecond
                        width: 1
                        height: ruler.height
                        Rectangle {
                            anchors.bottom: parent.bottom
                            width: 1
                            height: tickDelegate.major ? 9 : 5
                            color: theme.hairlineStrong
                        }
                        Loader {
                            active: tickDelegate.major
                                && Math.round(tickDelegate.tickTime)
                                   % Math.max(1, Math.ceil(62 / root.pixelsPerSecond)) === 0
                            x: 5
                            y: 5
                            sourceComponent: Text {
                                text: editor.formatTime(tickDelegate.tickTime)
                                color: theme.textFaint
                                font.pixelSize: 11
                                font.family: theme.monoFamily
                            }
                        }
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    preventStealing: true
                    function seekAt(px) {
                        editor.seek(Math.max(0, Math.min(editor.duration,
                                                        px / root.pixelsPerSecond)))
                    }
                    onPressed: mouse => { root.forceActiveFocus(); seekAt(mouse.x) }
                    onPositionChanged: mouse => { if (pressed) seekAt(mouse.x) }
                }
            }

            Rectangle {
                x: 0
                y: 32
                width: parent.width
                height: 48
                color: theme.surfaceRaised
                border.width: 1
                border.color: theme.hairline
            }
            ClipTrack {
                x: 0
                y: 36
                width: parent.width
                height: 40
                pixelsPerSecond: root.pixelsPerSecond
                focusTarget: root
            }
            Rectangle {
                x: 0
                y: 88
                width: parent.width
                height: 48
                color: theme.surfaceRaised
                border.width: 1
                border.color: theme.hairline
            }
            ZoomTrack {
                x: 0
                y: 92
                width: parent.width
                height: 40
                pixelsPerSecond: root.pixelsPerSecond
                focusTarget: root
            }
            Rectangle {
                visible: editor.hasCamera
                x: 0
                y: 144
                width: parent.width
                height: 48
                color: theme.surfaceRaised
                border.width: 1
                border.color: theme.hairline
            }
            Rectangle {
                visible: editor.hasCamera
                x: 0
                y: 148
                width: Math.max(20, editor.duration * root.pixelsPerSecond)
                height: 40
                radius: 6
                color: theme.hairline
                border.width: 1
                border.color: theme.hairlineStrong
                Label {
                    anchors.centerIn: parent
                    text: "Camera"
                    color: theme.textMuted
                    font.pixelSize: 12
                    font.weight: Font.Medium
                }
            }

            Rectangle {
                id: playhead
                x: editor.position * root.pixelsPerSecond - width / 2
                y: 25
                width: 1.5
                height: root.tracksBottom - y
                color: theme.record
                z: 20
                Shape {
                    x: -5.25
                    y: -1
                    width: 12
                    height: 8
                    ShapePath {
                        strokeWidth: 0
                        fillColor: theme.record
                        startX: 0
                        startY: 0
                        PathLine { x: 12; y: 0 }
                        PathLine { x: 6; y: 8 }
                        PathLine { x: 0; y: 0 }
                    }
                }
            }
        }

        ScrollBar.horizontal: ScrollBar {
            id: horizontalBar
            policy: flick.contentWidth > flick.width ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
            interactive: true
            height: 6
            opacity: timelineHover.hovered || flick.moving || pressed ? 1 : 0
            background: null
            contentItem: Rectangle {
                implicitHeight: 6
                radius: 3
                color: theme.textFaint
            }
            Behavior on opacity { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
        }
    }
}
