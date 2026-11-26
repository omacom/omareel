import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    readonly property var camera: editor.project.camera

    OmEmptyState {
        visible: !editor.hasCamera
        iconSource: "qrc:/omareel/assets/icons/lucide/video-off.svg"
        title: "No webcam in this recording"
        hint: "Turn on Webcam in the launcher before recording to add a camera bubble."
    }
    Label {
        visible: editor.hasCamera && editor.cameraStatus !== ""
        Layout.fillWidth: true
        textFormat: Text.PlainText
        text: editor.cameraStatus
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.type.small
    }
    EditorSwitch {
        visible: editor.hasCamera
        text: "Show camera"
        description: "Overlay the webcam on the recording"
        enabled: editor.hasCamera
        checked: editor.hasCamera && root.camera.enabled
        onToggled: editor.setProjectValue("camera.enabled", checked)
    }
    ColumnLayout {
        visible: editor.hasCamera
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        enabled: editor.hasCamera && root.camera.enabled
        opacity: enabled ? 1 : .42
        Behavior on opacity { NumberAnimation { duration: 160 } }

        PanelHeading { text: "Position"; Layout.topMargin: 2 }
        // A miniature canvas: click a quadrant and the bubble glides to that corner.
        Rectangle {
            id: positionPad
            Layout.fillWidth: true
            Layout.preferredHeight: Math.round(width * 9 / 16 * .62)
            radius: theme.controlRadius
            color: theme.fill
            border.width: 1
            border.color: theme.stroke
            readonly property real inset: 10
            readonly property real bubble: Math.max(16, height * (.22 + root.camera.size * .6))
            Repeater {
                model: ["top-left", "top-right", "bottom-left", "bottom-right"]
                delegate: Rectangle {
                    required property string modelData
                    readonly property bool atRight: modelData.endsWith("right")
                    readonly property bool atBottom: modelData.startsWith("bottom")
                    x: atRight ? positionPad.width / 2 : 0
                    y: atBottom ? positionPad.height / 2 : 0
                    width: positionPad.width / 2
                    height: positionPad.height / 2
                    color: quadrantMouse.containsMouse && root.camera.position !== modelData ? theme.fillHover : "transparent"
                    radius: theme.controlRadius
                    Behavior on color { ColorAnimation { duration: 120 } }
                    Rectangle {
                        width: 6; height: 6; radius: 3
                        x: parent.atRight ? parent.width - width - positionPad.inset - 4 : positionPad.inset + 4
                        y: parent.atBottom ? parent.height - height - positionPad.inset - 4 : positionPad.inset + 4
                        color: theme.textFaint
                        opacity: root.camera.position === parent.modelData ? 0 : 1
                    }
                    MouseArea {
                        id: quadrantMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: editor.setProjectValue("camera.position", parent.modelData)
                    }
                }
            }
            Rectangle {
                id: bubble
                readonly property bool atRight: root.camera.position.endsWith("right")
                readonly property bool atBottom: root.camera.position.startsWith("bottom")
                width: positionPad.bubble
                height: root.camera.shape === "rectangle" ? width * 9 / 16 : width
                x: atRight ? positionPad.width - width - positionPad.inset : positionPad.inset
                y: atBottom ? positionPad.height - height - positionPad.inset : positionPad.inset
                radius: root.camera.shape === "round" ? width / 2 : Math.min(width / 3, root.camera.radius / 4)
                color: Qt.alpha(theme.accent, .85)
                border.width: root.camera.border.enabled ? 2 : 0
                border.color: root.camera.border.color
                Behavior on x { NumberAnimation { duration: 320; easing.type: Easing.OutBack; easing.overshoot: 1.1 } }
                Behavior on y { NumberAnimation { duration: 320; easing.type: Easing.OutBack; easing.overshoot: 1.1 } }
                Behavior on width { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
                Behavior on radius { NumberAnimation { duration: 200; easing.type: Easing.OutCubic } }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "camera.size"; from: .1; to: .5; value: root.camera.size; stepSize: .01; decimals: 2 }

        OmSettingRow {
            label: "Shape"
            OmButtonGroup {
                objectName: "cameraShapeGroup"
                Layout.preferredWidth: 212
                options: [{value:"round", label:"Round"}, {value:"square", label:"Square"}, {value:"rectangle", label:"Rectangle"}]
                value: root.camera.shape
                onChanged: value => editor.setProjectValue("camera.shape", value)
            }
        }
        PanelSlider { visible: root.camera.shape !== "round"; Layout.fillWidth: true; label: "Corner radius"; path: "camera.radius"; from: 0; to: 80; value: root.camera.radius; stepSize: 1; suffix: " px" }
        PanelSlider { Layout.fillWidth: true; label: "Shrink while zoomed"; path: "camera.scaleDuringZoom"; from: .4; to: 1; value: root.camera.scaleDuringZoom; stepSize: .01; decimals: 2 }

        OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
        EditorSwitch { text: "Shadow"; checked: root.camera.shadow.enabled; onToggled: editor.setProjectValue("camera.shadow.enabled", checked) }
        ColumnLayout {
            visible: root.camera.shadow.enabled
            Layout.fillWidth: true
            spacing: theme.space.panelGap
            PanelSlider { Layout.fillWidth: true; label: "Intensity"; path: "camera.shadow.intensity"; from: 0; to: 1; value: root.camera.shadow.intensity; stepSize: .01; decimals: 2 }
            PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "camera.shadow.blur"; from: 0; to: 64; value: root.camera.shadow.blur; stepSize: 1 }
            PanelSlider { Layout.fillWidth: true; label: "Distance"; path: "camera.shadow.distance"; from: 0; to: 80; value: root.camera.shadow.distance; stepSize: 1 }
        }

        OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
        EditorSwitch { text: "Border"; checked: root.camera.border.enabled; onToggled: editor.setProjectValue("camera.border.enabled", checked) }
        ColumnLayout {
            visible: root.camera.border.enabled
            Layout.fillWidth: true
            spacing: theme.space.panelGap
            PanelSlider { Layout.fillWidth: true; label: "Width"; path: "camera.border.width"; from: 0; to: 16; value: root.camera.border.width; stepSize: 1; suffix: " px" }
            PanelSlider { Layout.fillWidth: true; label: "Opacity"; path: "camera.border.alpha"; from: 0; to: 1; value: root.camera.border.alpha; stepSize: .01; decimals: 2 }
            OmSettingRow {
                label: "Color"
                OmColorWell { swatch: root.camera.border.color; onClicked: borderColourDialog.open() }
            }
        }
    }
    // Orientation and the full-screen look apply to camera clips too, so they stay available
    // with the bubble switched off.
    ColumnLayout {
        visible: editor.hasCamera
        Layout.fillWidth: true
        spacing: theme.space.panelGap

        OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
        PanelHeading { text: "Full screen" }
        OmButtonGroup {
            objectName: "cameraFullFrameGroup"
            Layout.fillWidth: true
            options: [{value:"fill", label:"Fill frame"}, {value:"square", label:"Square + blur"}]
            value: root.camera.fullFrame
            onChanged: value => editor.setProjectValue("camera.fullFrame", value)
        }
        Label {
            Layout.fillWidth: true
            text: "Clips with the Camera layout show the webcam full screen. Set a clip's layout in Clip."
            color: theme.textMuted
            font.pixelSize: theme.type.small
            wrapMode: Text.WordWrap
        }

        OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
        OmSettingRow {
            label: "Rotation"
            OmButtonGroup {
                Layout.preferredWidth: 176
                options: [{value:0,label:"0°"}, {value:90,label:"90°"},
                    {value:180,label:"180°"}, {value:270,label:"270°"}]
                value: root.camera.rotation
                onChanged: value => editor.setProjectValue("camera.rotation", value)
            }
        }
        EditorSwitch {
            text: "Mirror"
            description: "Flip the camera horizontally"
            checked: root.camera.flipHorizontal
            onToggled: editor.setProjectValue("camera.flipHorizontal", checked)
        }
    }
    ColorDialog {
        id: borderColourDialog
        selectedColor: root.camera.border.color
        onAccepted: editor.setProjectValue("camera.border.color", selectedColor)
    }
}
