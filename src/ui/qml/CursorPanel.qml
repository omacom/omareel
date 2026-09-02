import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    spacing: 12
    Switch { text: "Show cursor"; checked: editor.project.cursor.visible; onToggled: editor.setProjectValue("cursor.visible", checked) }
    PanelSlider { Layout.fillWidth: true; label: "Size"; path: "cursor.size"; from: 0.5; to: 3; value: editor.project.cursor.size; stepSize: 0.05; decimals: 2 }
    Switch { text: "Smooth movement"; checked: editor.project.cursor.smoothing; onToggled: editor.setProjectValue("cursor.smoothing", checked) }
    Label { text: "Click effect"; color: "#bbbcc3" }
    ComboBox {
        Layout.fillWidth: true
        model: [{label:"None",value:"none"},{label:"Circle",value:"circle"}]
        textRole: "label"
        Component.onCompleted: currentIndex = editor.project.cursor.clickEffect === "none" ? 0 : 1
        onActivated: editor.setProjectValue("cursor.clickEffect", model[currentIndex].value)
    }
    RowLayout {
        Layout.fillWidth: true
        Label { text: "Ring color"; Layout.fillWidth: true }
        Rectangle { width: 44; height: 28; radius: 5; color: editor.project.cursor.ringColor; border.color: "#777"; MouseArea { anchors.fill: parent; onClicked: ringDialog.open() } }
    }
    Switch {
        id: idleSwitch
        text: "Hide when idle"
        checked: editor.project.cursor.hideWhenIdleMs !== null
        onToggled: editor.setProjectValue("cursor.hideWhenIdleMs", checked ? 1500 : null)
    }
    PanelSlider { visible: idleSwitch.checked; Layout.fillWidth: true; label: "Idle delay (ms)"; path: "cursor.hideWhenIdleMs"; from: 250; to: 5000; value: editor.project.cursor.hideWhenIdleMs || 1500; stepSize: 250 }
    Label { text: "Movement spring"; font.bold: true; topPadding: 8 }
    PanelSlider { Layout.fillWidth: true; label: "Mass"; path: "cursor.spring.mass"; from: 0.2; to: 6; value: editor.project.cursor.spring.mass; stepSize: 0.05; decimals: 2 }
    PanelSlider { Layout.fillWidth: true; label: "Stiffness"; path: "cursor.spring.stiffness"; from: 50; to: 800; value: editor.project.cursor.spring.stiffness; stepSize: 5 }
    PanelSlider { Layout.fillWidth: true; label: "Damping"; path: "cursor.spring.damping"; from: 5; to: 120; value: editor.project.cursor.spring.damping; stepSize: 1 }
    ColorDialog { id: ringDialog; selectedColor: editor.project.cursor.ringColor; onAccepted: editor.setProjectValue("cursor.ringColor", selectedColor) }
}
