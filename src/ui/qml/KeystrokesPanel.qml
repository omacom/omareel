import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

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
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        text: "Show keystrokes"
        checked: root.settings.enabled
        onToggled: editor.setProjectValue("keystrokes.enabled", checked)
    }
    EditorSwitch {
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        text: "Shortcuts only"
        checked: root.settings.showOnlyShortcuts
        onToggled: editor.setProjectValue("keystrokes.showOnlyShortcuts", checked)
    }
    Label {
        Layout.fillWidth: true
        text: "Hide unmodified letters and digits while keeping shortcuts and editing keys."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.font.bodySmall
    }
    PanelLabel { text: "Position" }
    EditorComboBox {
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        model: root.positions
        textRole: "label"
        currentIndex: root.positionIndex(root.settings.position)
        onActivated: editor.setProjectValue("keystrokes.position", model[currentIndex].value)
    }
    PanelSlider { Layout.fillWidth: true; label: "Size"; path: "keystrokes.size"; from: .5; to: 2; value: root.settings.size; stepSize: .05; decimals: 2 }
    PanelSlider { Layout.fillWidth: true; label: "Hold (ms)"; path: "keystrokes.holdMs"; from: 100; to: 3000; value: root.settings.holdMs; stepSize: 50 }
}
