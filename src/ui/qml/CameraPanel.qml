import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    readonly property var camera: editor.project.camera

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
        font.pixelSize: theme.font.bodySmall
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
        spacing: theme.space.panelGap
        enabled: editor.hasCamera && root.camera.enabled
        opacity: enabled ? 1 : .42

        PanelLabel { text: "Position" }
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: theme.space.md
            rowSpacing: theme.space.md
            Repeater {
                model: [
                    {value:"top-left", label:"Top left"}, {value:"top-right", label:"Top right"},
                    {value:"bottom-left", label:"Bottom left"}, {value:"bottom-right", label:"Bottom right"}
                ]
                delegate: OmButton {
                    required property var modelData
                    Layout.fillWidth: true
                    bordered: true
                    selected: root.camera.position === modelData.value
                    text: modelData.label
                    onClicked: editor.setProjectValue("camera.position", modelData.value)
                }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "camera.size"; from: .1; to: .5; value: root.camera.size; stepSize: .01; decimals: 2 }

        PanelLabel { text: "Shape" }
        OmButtonGroup {
            Layout.fillWidth: true
            options: [{value:"round", label:"Round"}, {value:"rounded", label:"Rounded"}, {value:"square", label:"Square"}]
            value: root.camera.shape
            onChanged: value => editor.setProjectValue("camera.shape", value)
        }
        PanelSlider { visible: root.camera.shape === "rounded"; Layout.fillWidth: true; label: "Corner radius"; path: "camera.radius"; from: 0; to: 80; value: root.camera.radius; stepSize: 1 }

        PanelLabel { text: "Crop" }
        OmButtonGroup {
            Layout.fillWidth: true
            options: [{value:"square", label:"Square"},
                {value:"original", label:"Original", enabled: root.camera.shape !== "round"}]
            value: root.camera.shape === "round" ? "square" : root.camera.crop
            onChanged: value => editor.setProjectValue("camera.crop", value)
        }

        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Rotation"; Layout.fillWidth: true }
            OmIconButton {
                enabled: false; opacity: 1; background: null
                icon.source: "qrc:/omareel/assets/icons/lucide/rotate-cw.svg"
                icon.color: theme.foreground
                icon.width: 15; icon.height: 15
            }
        }
        OmButtonGroup {
            Layout.fillWidth: true
            options: [{value:0,label:"0°"}, {value:90,label:"90°"},
                {value:180,label:"180°"}, {value:270,label:"270°"}]
            value: root.camera.rotation
            onChanged: value => editor.setProjectValue("camera.rotation", value)
        }
        EditorSwitch {
            Layout.preferredHeight: 32
            text: "Flip horizontal"
            checked: root.camera.flipHorizontal
            onToggled: editor.setProjectValue("camera.flipHorizontal", checked)
        }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.separator }
        PanelHeading { text: "Camera shadow" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Shadow"; checked: root.camera.shadow.enabled; onToggled: editor.setProjectValue("camera.shadow.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Intensity"; path: "camera.shadow.intensity"; from: 0; to: 1; value: root.camera.shadow.intensity; stepSize: .01; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "camera.shadow.blur"; from: 0; to: 64; value: root.camera.shadow.blur; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Distance"; path: "camera.shadow.distance"; from: 0; to: 80; value: root.camera.shadow.distance; stepSize: 1 }

        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.separator }
        PanelHeading { text: "Border" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Border"; checked: root.camera.border.enabled; onToggled: editor.setProjectValue("camera.border.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Width"; path: "camera.border.width"; from: 0; to: 16; value: root.camera.border.width; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Opacity"; path: "camera.border.alpha"; from: 0; to: 1; value: root.camera.border.alpha; stepSize: .01; decimals: 2 }
        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Colour"; Layout.fillWidth: true }
            Rectangle {
                width: 44; height: 28; radius: theme.radius; color: root.camera.border.color
                border.width: 1; border.color: root.camera.border.enabled ? theme.selectedBorder : theme.normalBorder
                MouseArea { anchors.fill: parent; onClicked: borderColourDialog.open() }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Shrink while zoomed"; path: "camera.scaleDuringZoom"; from: .4; to: 1; value: root.camera.scaleDuringZoom; stepSize: .01; decimals: 2 }
    }
    ColorDialog {
        id: borderColourDialog
        selectedColor: root.camera.border.color
        onAccepted: editor.setProjectValue("camera.border.color", selectedColor)
    }
}
