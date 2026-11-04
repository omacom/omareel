import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
    readonly property var settings: editor.project.keystrokes
    readonly property var positions: [
        {label:"Top left", value:"top-left"},
        {label:"Top center", value:"top-center"},
        {label:"Top right", value:"top-right"},
        {label:"Bottom left", value:"bottom-left"},
        {label:"Bottom center", value:"bottom-center"},
        {label:"Bottom right", value:"bottom-right"}
    ]
    function positionIndex(value) {
        for (let i = 0; i < positions.length; ++i)
            if (positions[i].value === value) return i
        return 4
    }

    EditorSwitch {
        text: "Show keystrokes"
        description: "Display shortcuts and special keys; typing is never recorded"
        checked: root.settings.enabled
        onToggled: editor.setProjectValue("keystrokes.enabled", checked)
    }
    ColumnLayout {
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        enabled: root.settings.enabled
        opacity: enabled ? 1 : .42
        Behavior on opacity { NumberAnimation { duration: 160 } }
        // Preview of how a shortcut will look on the video.
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            radius: theme.controlRadius
            color: theme.canvas
            border.width: 1
            border.color: theme.stroke
            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                Repeater {
                    model: ["Ctrl", "Shift", "P"]
                    delegate: OmKeycap { required property string modelData; text: modelData; implicitHeight: 24 }
                }
            }
        }
        OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
        OmSettingRow {
            label: "Position"
            EditorComboBox {
                Layout.preferredWidth: 150
                model: root.positions
                textRole: "label"
                currentIndex: root.positionIndex(root.settings.position)
                onActivated: editor.setProjectValue("keystrokes.position", model[currentIndex].value)
            }
        }
        PanelSlider { Layout.fillWidth: true; label: "Size"; path: "keystrokes.size"; from: .5; to: 2; value: root.settings.size; stepSize: .05; decimals: 2; suffix: "×" }
        PanelSlider { Layout.fillWidth: true; label: "Hold"; path: "keystrokes.holdMs"; from: 100; to: 3000; value: root.settings.holdMs; stepSize: 50; suffix: " ms" }
    }
}
