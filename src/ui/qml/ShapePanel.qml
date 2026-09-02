import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    readonly property bool frameEnabled: editor.project.background.type !== "none"
    Label {
        visible: !root.frameEnabled
        Layout.fillWidth: true
        text: "Shape settings stay saved but are ignored while the background type is None."
        wrapMode: Text.WordWrap
        color: theme.accent
        font.pixelSize: 11
    }
    ColumnLayout {
        Layout.fillWidth: true
        spacing: 12
        enabled: root.frameEnabled
        opacity: enabled ? 1 : .42
        PanelSlider { Layout.fillWidth: true; label: "Padding"; path: "frame.padding"; from: 0; to: 0.3; value: editor.project.frame.padding; stepSize: 0.005; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Roundness"; path: "frame.radius"; from: 0; to: 80; value: editor.project.frame.radius; stepSize: 1 }
        Switch { Layout.preferredHeight: 32; text: "Shadow"; checked: editor.project.frame.shadow.enabled; onToggled: editor.setProjectValue("frame.shadow.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Intensity"; path: "frame.shadow.intensity"; from: 0; to: 1; value: editor.project.frame.shadow.intensity; stepSize: 0.01; decimals: 2 }
        PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "frame.shadow.blur"; from: 0; to: 100; value: editor.project.frame.shadow.blur; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Distance"; path: "frame.shadow.distance"; from: 0; to: 100; value: editor.project.frame.shadow.distance; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Angle"; path: "frame.shadow.angle"; from: 0; to: 360; value: editor.project.frame.shadow.angle; stepSize: 1 }
        Rectangle { Layout.fillWidth: true; height: 1; color: Qt.alpha(theme.foreground, .10) }
        Switch { Layout.preferredHeight: 32; text: "Inset border"; checked: editor.project.frame.inset.enabled; onToggled: editor.setProjectValue("frame.inset.enabled", checked) }
        PanelSlider { Layout.fillWidth: true; label: "Inset width"; path: "frame.inset.width"; from: 0; to: 16; value: editor.project.frame.inset.width; stepSize: 1 }
        PanelSlider { Layout.fillWidth: true; label: "Inset opacity"; path: "frame.inset.alpha"; from: 0; to: 1; value: editor.project.frame.inset.alpha; stepSize: 0.01; decimals: 2 }
    }
}
