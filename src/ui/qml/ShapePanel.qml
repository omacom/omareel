import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    readonly property bool frameEnabled: editor.project.background.type !== "none"
    Label {
        visible: !root.frameEnabled
        Layout.fillWidth: true
        text: "Shape settings stay saved but are ignored while the background type is None."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.font.bodySmall
    }
    ColumnLayout {
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        enabled: root.frameEnabled
        opacity: enabled ? 1 : .42
        PanelHeading { text: "Frame" }
        PanelSlider { Layout.fillWidth: true; label: "Padding"; path: "frame.padding"; from: 0; to: 0.3; value: editor.project.frame.padding; stepSize: 0.005; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Roundness"; path: "frame.radius"; from: 0; to: 80; value: editor.project.frame.radius; stepSize: 1 }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.separator }
        PanelHeading { text: "Shadow" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Enable shadow"; checked: editor.project.frame.shadow.enabled; onToggled: editor.setProjectValue("frame.shadow.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Intensity"; path: "frame.shadow.intensity"; from: 0; to: 1; value: editor.project.frame.shadow.intensity; stepSize: 0.01; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "frame.shadow.blur"; from: 0; to: 100; value: editor.project.frame.shadow.blur; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Distance"; path: "frame.shadow.distance"; from: 0; to: 100; value: editor.project.frame.shadow.distance; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Angle"; path: "frame.shadow.angle"; from: 0; to: 360; value: editor.project.frame.shadow.angle; stepSize: 1 }
        Rectangle { Layout.fillWidth: true; height: 1; color: theme.separator }
        PanelHeading { text: "Border" }
        EditorSwitch { Layout.preferredHeight: 32; text: "Border"; checked: editor.project.frame.border.enabled; onToggled: editor.setProjectValue("frame.border.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Width"; path: "frame.border.width"; from: 0; to: 16; value: editor.project.frame.border.width; stepSize: 1 }
        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Color"; Layout.fillWidth: true }
            Rectangle {
                width: 44; height: 28; radius: theme.radius; color: editor.project.frame.border.color
                border.width: 1; border.color: editor.project.frame.border.enabled ? theme.selectedBorder : theme.normalBorder
                MouseArea { anchors.fill: parent; onClicked: borderColorDialog.open() }
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Opacity"; path: "frame.border.alpha"; from: 0; to: 1; value: editor.project.frame.border.alpha; stepSize: 0.01; decimals: 2 }
    }
    ColorDialog {
        id: borderColorDialog
        selectedColor: editor.project.frame.border.color
        onAccepted: editor.setProjectValue("frame.border.color", selectedColor)
    }
}
