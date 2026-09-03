import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 16
    readonly property var camera: editor.project.camera

    component ChoiceButton: EditorButton {
        checkable: true
        selected: checked
        Layout.fillWidth: true
        Layout.preferredHeight: 30
        font.pixelSize: 11
    }

    PanelHeading {
        visible: !editor.hasCamera
        Layout.fillWidth: true
        text: "No webcam in this recording"
    }
    Label {
        visible: !editor.hasCamera
        Layout.fillWidth: true
        text: "Enable it in the launcher before recording to add a camera overlay."
        color: theme.textMuted
        font.pixelSize: 11
        wrapMode: Text.WordWrap
    }
    PanelHeading { visible: editor.hasCamera; Layout.fillWidth: true; text: editor.cameraStatus; wrapMode: Text.WordWrap }
    EditorSwitch {
        visible: editor.hasCamera
        Layout.preferredHeight: 32
        text: "Enable webcam overlay"
        enabled: editor.hasCamera
        checked: editor.hasCamera && root.camera.enabled
        onToggled: editor.setProjectValue("camera.enabled", checked)
    }
    ColumnLayout {
        visible: editor.hasCamera
        Layout.fillWidth: true
        spacing: 16
        enabled: editor.hasCamera && root.camera.enabled
        opacity: enabled ? 1 : .42

        PanelLabel { text: "Position" }
        GridLayout {
            Layout.fillWidth: true; columns: 2; columnSpacing: 6; rowSpacing: 6
            Repeater {
                model: [
                    {value:"top-left", text:"Top left"}, {value:"top-right", text:"Top right"},
                    {value:"bottom-left", text:"Bottom left"}, {value:"bottom-right", text:"Bottom right"}
                ]
                delegate: ChoiceButton {
                    required property var modelData
                    text: modelData.text
                    checked: root.camera.position === modelData.value
                    onClicked: editor.setProjectValue("camera.position", modelData.value)
                }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "camera.size"; from: .1; to: .5; value: root.camera.size; stepSize: .01; decimals: 2 }

        PanelLabel { text: "Shape" }
        RowLayout {
            Layout.fillWidth: true; spacing: 5
            Repeater {
                model: [{value:"round", text:"Round"}, {value:"rounded", text:"Rounded"}, {value:"square", text:"Square"}]
                delegate: ChoiceButton {
                    required property var modelData
                    text: modelData.text
                    checked: root.camera.shape === modelData.value
                    onClicked: editor.setProjectValue("camera.shape", modelData.value)
                }
            }
        }
        PanelSlider { visible: root.camera.shape === "rounded"; Layout.fillWidth: true; label: "Corner radius"; path: "camera.radius"; from: 0; to: 80; value: root.camera.radius; stepSize: 1 }

        PanelLabel { text: "Crop" }
        RowLayout {
            Layout.fillWidth: true; spacing: 5
            ChoiceButton {
                text: "Square"
                checked: root.camera.shape === "round" || root.camera.crop === "square"
                onClicked: editor.setProjectValue("camera.crop", "square")
            }
            ChoiceButton {
                text: "Original"
                enabled: root.camera.shape !== "round"
                checked: root.camera.shape !== "round" && root.camera.crop === "original"
                onClicked: editor.setProjectValue("camera.crop", "original")
            }
        }

        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Rotation"; Layout.fillWidth: true }
            ToolButton {
                enabled: false; opacity: 1; background: null
                icon.source: "qrc:/omarecord/assets/icons/lucide/rotate-cw.svg"
                icon.color: theme.foreground
                icon.width: 15; icon.height: 15
            }
        }
        RowLayout {
            Layout.fillWidth: true; spacing: 5
            Repeater {
                model: [0, 90, 180, 270]
                delegate: ChoiceButton {
                    required property int modelData
                    text: modelData + "°"
                    checked: root.camera.rotation === modelData
                    onClicked: editor.setProjectValue("camera.rotation", modelData)
                }
            }
        }
        EditorSwitch {
            Layout.preferredHeight: 32
            text: "Flip horizontal"
            checked: root.camera.flipHorizontal
            onToggled: editor.setProjectValue("camera.flipHorizontal", checked)
        }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
        PanelHeading { text: "Camera shadow" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Shadow"; checked: root.camera.shadow.enabled; onToggled: editor.setProjectValue("camera.shadow.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Intensity"; path: "camera.shadow.intensity"; from: 0; to: 1; value: root.camera.shadow.intensity; stepSize: .01; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "camera.shadow.blur"; from: 0; to: 64; value: root.camera.shadow.blur; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Distance"; path: "camera.shadow.distance"; from: 0; to: 80; value: root.camera.shadow.distance; stepSize: 1 }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
        PanelHeading { text: "Inset border" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Border"; checked: root.camera.inset.enabled; onToggled: editor.setProjectValue("camera.inset.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Width"; path: "camera.inset.width"; from: 0; to: 16; value: root.camera.inset.width; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Opacity"; path: "camera.inset.alpha"; from: 0; to: 1; value: root.camera.inset.alpha; stepSize: .01; decimals: 2 }
        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Colour"; Layout.fillWidth: true }
            Rectangle {
                width: 44; height: 28; radius: 7; color: root.camera.inset.color
                border.width: 2; border.color: root.camera.inset.enabled ? theme.accent : theme.hairlineStrong
                MouseArea { anchors.fill: parent; onClicked: insetColourDialog.open() }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Shrink while zoomed"; path: "camera.scaleDuringZoom"; from: .4; to: 1; value: root.camera.scaleDuringZoom; stepSize: .01; decimals: 2 }
    }
    ColorDialog {
        id: insetColourDialog
        selectedColor: root.camera.inset.color
        onAccepted: editor.setProjectValue("camera.inset.color", selectedColor)
    }
}
