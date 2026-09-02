import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    property var selected: {
        for (let i=0;i<editor.zooms.length;i++) if (editor.zooms[i].id === editor.selectedZoomId) return editor.zooms[i]
        return null
    }
    readonly property var zoomStyle: editor.project.zoomStyle
    readonly property var motionPresets: [[2.6,170,44], [2.25,200,40], [1.4,300,34]]
    function motionIndex() {
        const spring = zoomStyle.spring
        for (let i = 0; i < motionPresets.length; ++i) {
            const p = motionPresets[i]
            if (Math.abs(spring.mass-p[0]) < .001 && Math.abs(spring.stiffness-p[1]) < .001 && Math.abs(spring.damping-p[2]) < .001) return i
        }
        return -1
    }
    PanelHeading { text: selected ? "Selected zoom" : "Select a zoom on the timeline"; wrapMode: Text.WordWrap; Layout.fillWidth: true }
    ColumnLayout {
        visible: root.selected !== null
        Layout.fillWidth: true
        PanelSlider {
            Layout.fillWidth: true; label: "Level"; path: "zoom-level"; from: 1; to: 4
            value: root.selected ? root.selected.level : 2; stepSize: 0.05; decimals: 2
            onValueChanged: {}
            // PanelSlider is project-path based, so the selected zoom uses its own control below.
            visible: false
        }
        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: "Level"; Layout.fillWidth: true }
            PanelValue { text: root.selected ? Number(root.selected.level).toFixed(1) + "×" : "" }
        }
        Slider {
            Layout.fillWidth: true; from: 1; to: 4; stepSize: 0.05; value: root.selected ? root.selected.level : 2
            onPressedChanged: pressed ? editor.beginCoalescedEdit("zoom-level") : editor.endCoalescedEdit()
            onMoved: editor.setZoomLevel(editor.selectedZoomId, value, true)
        }
        PanelLabel { text: "Target" }
        ComboBox {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            model: ["Auto", "Manual"]
            currentIndex: root.selected && typeof root.selected.target === "object" ? 1 : 0
            onActivated: currentIndex === 0 ? editor.setZoomTarget(editor.selectedZoomId, "auto") : editor.setPickingZoomTarget(true)
        }
        Button { Layout.fillWidth: true; text: editor.pickingZoomTarget ? "Click the preview…" : "Pick point on preview"; highlighted: editor.pickingZoomTarget; onClicked: editor.setPickingZoomTarget(!editor.pickingZoomTarget) }
        Button { Layout.fillWidth: true; text: "Remove zoom"; onClicked: editor.removeZoom(editor.selectedZoomId) }
    }
    Rectangle { Layout.fillWidth: true; height: 1; color: Qt.alpha(theme.foreground, .10) }
    PanelHeading { text: "Motion" }
    ComboBox {
        id: springPreset
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        model: ["Smooth", "Default", "Snappy"]
        currentIndex: root.motionIndex()
        onActivated: {
            let v = root.motionPresets[currentIndex]
            editor.beginCoalescedEdit("zoom-spring")
            editor.setProjectValue("zoomStyle.spring.mass", v[0], true)
            editor.setProjectValue("zoomStyle.spring.stiffness", v[1], true)
            editor.setProjectValue("zoomStyle.spring.damping", v[2], true)
            editor.endCoalescedEdit()
        }
    }
    PanelSlider { Layout.fillWidth: true; label: "Edge snapping"; path: "zoomStyle.snapToEdgesRatio"; from: 0; to: 0.5; value: editor.project.zoomStyle.snapToEdgesRatio; stepSize: 0.01; decimals: 2 }
    Switch { Layout.preferredHeight: 32; text: "Instant boundaries"; checked: editor.project.zoomStyle.instantAnimation; onToggled: editor.setProjectValue("zoomStyle.instantAnimation", checked) }
    Button { Layout.fillWidth: true; text: "Regenerate zooms"; onClicked: editor.regenerateZooms() }
}
