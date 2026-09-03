import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    PanelHeading {
        Layout.fillWidth: true
        text: editor.cameraStatus
        wrapMode: Text.WordWrap
    }
    Switch {
        Layout.preferredHeight: 32
        text: "Enable webcam overlay"
        enabled: editor.hasCamera
        checked: editor.hasCamera && editor.project.camera.enabled
        onToggled: editor.setProjectValue("camera.enabled", checked)
    }
    ColumnLayout {
        Layout.fillWidth: true
        spacing: 12
        enabled: editor.hasCamera && editor.project.camera.enabled
        opacity: enabled ? 1 : .42

        PanelLabel { text: "Position" }
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 6
            rowSpacing: 6
            Repeater {
                model: [
                    {value: "top-left", text: "Top left"},
                    {value: "top-right", text: "Top right"},
                    {value: "bottom-left", text: "Bottom left"},
                    {value: "bottom-right", text: "Bottom right"}
                ]
                delegate: Button {
                    required property var modelData
                    Layout.fillWidth: true
                    text: modelData.text
                    checkable: true
                    checked: editor.project.camera.position === modelData.value
                    onClicked: editor.setProjectValue("camera.position", modelData.value)
                }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "camera.size"; from: .1; to: .5; value: editor.project.camera.size; stepSize: .01; decimals: 2 }

        PanelLabel { text: "Shape" }
        RowLayout {
            Layout.fillWidth: true
            spacing: 4
            Repeater {
                model: [{value:"round", text:"Round"}, {value:"rounded", text:"Rounded"}, {value:"square", text:"Square"}]
                delegate: Button {
                    required property var modelData
                    Layout.fillWidth: true
                    text: modelData.text
                    checkable: true
                    checked: editor.project.camera.shape === modelData.value
                    onClicked: editor.setProjectValue("camera.shape", modelData.value)
                }
            }
        }
        PanelSlider {
            visible: editor.project.camera.shape === "rounded"
            Layout.fillWidth: true
            label: "Corner radius"
            path: "camera.radius"
            from: 0; to: 80
            value: editor.project.camera.radius
            stepSize: 1
        }

        PanelLabel { text: "Crop" }
        RowLayout {
            Layout.fillWidth: true
            spacing: 4
            Button {
                Layout.fillWidth: true
                text: "Square"
                checkable: true
                checked: editor.project.camera.shape === "round" || editor.project.camera.crop === "square"
                onClicked: editor.setProjectValue("camera.crop", "square")
            }
            Button {
                Layout.fillWidth: true
                text: "Original"
                checkable: true
                enabled: editor.project.camera.shape !== "round"
                checked: editor.project.camera.shape !== "round" && editor.project.camera.crop === "original"
                onClicked: editor.setProjectValue("camera.crop", "original")
            }
        }
        Switch { Layout.preferredHeight: 32; text: "Mirror"; checked: editor.project.camera.mirror; onToggled: editor.setProjectValue("camera.mirror", checked) }
        Switch { Layout.preferredHeight: 32; text: "Shadow"; checked: editor.project.camera.shadow; onToggled: editor.setProjectValue("camera.shadow", checked) }
        PanelSlider {
            Layout.fillWidth: true
            label: "Shrink while zoomed"
            path: "camera.scaleDuringZoom"
            from: .4; to: 1
            value: editor.project.camera.scaleDuringZoom
            stepSize: .01
            decimals: 2
        }
    }
}
