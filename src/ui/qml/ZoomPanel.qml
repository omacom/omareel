import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 16
    property var selected: {
        for (let i=0;i<editor.zooms.length;i++) if (editor.zooms[i].id === editor.selectedZoomId) return editor.zooms[i]
        return null
    }
    readonly property var zoomStyle: editor.project.zoomStyle
    readonly property var motionPresets: [[2.6,170,44], [2.25,200,40], [1.4,300,34]]
    readonly property var motionCaptions: [
        "Smooth — relaxed movement with a gentle settle.",
        "Default — balanced movement for most recordings.",
        "Snappy — quick response with a crisp settle."
    ]
    function motionIndex() {
        const spring = zoomStyle.spring
        for (let i = 0; i < motionPresets.length; ++i) {
            const p = motionPresets[i]
            if (Math.abs(spring.mass-p[0]) < .001 && Math.abs(spring.stiffness-p[1]) < .001 && Math.abs(spring.damping-p[2]) < .001) return i
        }
        return -1
    }
    ColumnLayout {
        visible: root.selected === null
        Layout.fillWidth: true
        spacing: 6
        PanelHeading { text: "No zoom selected" }
        Label {
            Layout.fillWidth: true
            text: "Select a zoom on the timeline or press Z to add one"
            wrapMode: Text.WordWrap
            color: theme.textMuted
            font.pixelSize: 11
        }
    }
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
        EditorSlider {
            Layout.fillWidth: true; from: 1; to: 4; stepSize: 0.05; value: root.selected ? root.selected.level : 2
            onPressedChanged: pressed ? editor.beginCoalescedEdit("zoom-level") : editor.endCoalescedEdit()
            onMoved: editor.setZoomLevel(editor.selectedZoomId, value, true)
        }
        PanelLabel { text: "Target" }
        EditorComboBox {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            model: ["Auto", "Manual"]
            currentIndex: root.selected && typeof root.selected.target === "object" ? 1 : 0
            onActivated: currentIndex === 0 ? editor.setZoomTarget(editor.selectedZoomId, "auto") : editor.setPickingZoomTarget(true)
        }
        EditorButton { Layout.fillWidth: true; text: editor.pickingZoomTarget ? "Click the preview…" : "Pick point on preview"; selected: editor.pickingZoomTarget; onClicked: editor.setPickingZoomTarget(!editor.pickingZoomTarget) }
        EditorButton {
            Layout.fillWidth: true
            text: editor.selectedZoomIds.length > 1 ? "Remove selected zooms" : "Remove zoom"
            destructive: true
            onClicked: editor.removeSelectedZooms()
        }
    }
    Rectangle { Layout.fillWidth: true; height: 1; color: theme.hairline }
    PanelHeading { text: "Motion" }
    EditorComboBox {
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
    Label {
        Layout.fillWidth: true
        text: root.motionCaptions[Math.max(0, springPreset.currentIndex)]
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: 11
    }
    PanelSlider { Layout.fillWidth: true; label: "Edge snapping"; path: "zoomStyle.snapToEdgesRatio"; from: 0; to: 0.5; value: editor.project.zoomStyle.snapToEdgesRatio; stepSize: 0.01; decimals: 2 }
    Label {
        Layout.fillWidth: true
        text: "How close to the frame edge the zoom may look. 0 keeps the camera centered on the target; higher lets it slide to the very edge"
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: 11
    }
    EditorSwitch { Layout.preferredHeight: 32; text: "Cut instead of glide"; checked: editor.project.zoomStyle.instantAnimation; onToggled: editor.setProjectValue("zoomStyle.instantAnimation", checked) }
    Label {
        Layout.fillWidth: true
        text: "Jump straight to the zoom at its start/end instead of animating"
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: 11
    }
    PanelSlider { Layout.fillWidth: true; label: "Motion blur"; path: "zoomStyle.motionBlur"; from: 0; to: 1; value: editor.project.zoomStyle.motionBlur; stepSize: .05; decimals: 2 }
    EditorButton { Layout.fillWidth: true; text: "Regenerate zooms"; onClicked: editor.regenerateZooms() }
}
