import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    readonly property var cursor: editor.project.cursor
    readonly property var clickEffects: [{label:"None",value:"none"},{label:"Circle",value:"circle"}]
    function clickEffectIndex(value) {
        for (let i = 0; i < clickEffects.length; ++i)
            if (clickEffects[i].value === value) return i
        return value === "ripple" ? 1 : 0
    }
    Switch { Layout.preferredHeight: 32; text: "Show cursor"; checked: root.cursor.visible; onToggled: editor.setProjectValue("cursor.visible", checked) }
    PanelSlider { Layout.fillWidth: true; label: "Size"; path: "cursor.size"; from: 0.5; to: 3; value: root.cursor.size; stepSize: 0.05; decimals: 2 }
    Switch { Layout.preferredHeight: 32; text: "Smooth movement"; checked: root.cursor.smoothing; onToggled: editor.setProjectValue("cursor.smoothing", checked) }
    PanelLabel { text: "Click effect" }
    ComboBox {
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        model: root.clickEffects
        textRole: "label"
        currentIndex: root.clickEffectIndex(root.cursor.clickEffect)
        onActivated: editor.setProjectValue("cursor.clickEffect", model[currentIndex].value)
    }
    RowLayout {
        Layout.fillWidth: true
        PanelLabel { text: "Ring color"; Layout.fillWidth: true }
        Rectangle { width: 44; height: 28; radius: 5; color: root.cursor.ringColor; border.color: Qt.alpha(theme.foreground, .4); MouseArea { anchors.fill: parent; onClicked: ringDialog.open() } }
    }
    Switch {
        id: idleSwitch
        Layout.preferredHeight: 32
        text: "Hide when idle"
        checked: root.cursor.hideWhenIdleMs !== null
        onToggled: editor.setProjectValue("cursor.hideWhenIdleMs", checked ? 1500 : null)
    }
    PanelSlider { visible: idleSwitch.checked; Layout.fillWidth: true; label: "Idle delay (ms)"; path: "cursor.hideWhenIdleMs"; from: 250; to: 5000; value: root.cursor.hideWhenIdleMs || 1500; stepSize: 250 }
    PanelHeading { text: "Movement spring"; topPadding: 8 }
    PanelSlider { Layout.fillWidth: true; label: "Mass"; path: "cursor.spring.mass"; from: 0.2; to: 6; value: root.cursor.spring.mass; stepSize: 0.05; decimals: 2 }
    PanelSlider { Layout.fillWidth: true; label: "Stiffness"; path: "cursor.spring.stiffness"; from: 50; to: 800; value: root.cursor.spring.stiffness; stepSize: 5 }
    PanelSlider { Layout.fillWidth: true; label: "Damping"; path: "cursor.spring.damping"; from: 5; to: 120; value: root.cursor.spring.damping; stepSize: 1 }
    ColorDialog { id: ringDialog; selectedColor: root.cursor.ringColor; onAccepted: editor.setProjectValue("cursor.ringColor", selectedColor) }
}
