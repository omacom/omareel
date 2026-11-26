import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Controls.impl
import Omareel.Ui

ColumnLayout {
    id: root
    spacing: theme.space.panelGap
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
    OmEmptyState {
        visible: root.selected === null
        iconSource: "qrc:/omareel/assets/icons/lucide/zoom-in.svg"
        title: "No zoom selected"
        hint: "Pick one on the timeline, or press"
        key: "Z"
    }
    ColumnLayout {
        visible: root.selected !== null
        Layout.fillWidth: true
        spacing: theme.space.panelGap
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            RowLayout {
                Layout.fillWidth: true
                PanelLabel { text: "Level"; Layout.fillWidth: true }
                PanelValue { text: root.selected ? Number(root.selected.level).toFixed(1) + "×" : ""; live: levelSlider.pressed }
            }
            EditorSlider {
                id: levelSlider
                Layout.fillWidth: true
                Layout.leftMargin: -7; Layout.rightMargin: -7
                from: 1; to: 4; stepSize: 0.05; value: root.selected ? root.selected.level : 2
                onPressedChanged: pressed ? editor.beginCoalescedEdit("zoom-level") : editor.endCoalescedEdit()
                onMoved: editor.setZoomLevel(editor.selectedZoomId, value, true)
            }
        }
        OmSettingRow {
            label: "Focus"
            OmButtonGroup {
                Layout.preferredWidth: 150
                options: [{label: "Auto", value: "auto"}, {label: "Manual", value: "manual"}]
                value: editor.pickingZoomTarget || (root.selected && typeof root.selected.target === "object") ? "manual" : "auto"
                onChanged: value => value === "auto" ? editor.setZoomTarget(editor.selectedZoomId, "auto")
                                                     : editor.setPickingZoomTarget(true)
            }
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            EditorButton {
                Layout.fillWidth: true
                text: editor.pickingZoomTarget ? "Click the preview…" : "Pick point"
                icon.source: "qrc:/omareel/assets/icons/lucide/mouse-pointer-2.svg"
                selected: editor.pickingZoomTarget
                onClicked: editor.setPickingZoomTarget(!editor.pickingZoomTarget)
            }
            EditorButton {
                Layout.fillWidth: true
                text: editor.selectedZoomIds.length > 1 ? "Remove " + editor.selectedZoomIds.length : "Remove"
                icon.source: "qrc:/omareel/assets/icons/lucide/trash-2.svg"
                destructive: true
                onClicked: editor.removeSelectedZooms()
            }
        }
    }
    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    PanelHeading { text: "Motion" }
    OmButtonGroup {
        id: springPreset
        Layout.fillWidth: true
        options: [{label: "Smooth", value: 0}, {label: "Default", value: 1}, {label: "Snappy", value: 2}]
        value: root.motionIndex()
        onChanged: index => {
            let v = root.motionPresets[index]
            editor.beginCoalescedEdit("zoom-spring")
            editor.setProjectValue("zoomStyle.spring.mass", v[0], true)
            editor.setProjectValue("zoomStyle.spring.stiffness", v[1], true)
            editor.setProjectValue("zoomStyle.spring.damping", v[2], true)
            editor.endCoalescedEdit()
        }
    }
    Label {
        Layout.fillWidth: true
        textFormat: Text.PlainText
        text: root.motionIndex() >= 0 ? root.motionCaptions[root.motionIndex()] : "Custom spring."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.type.small
    }
    PanelSlider { Layout.fillWidth: true; label: "Edge snapping"; path: "zoomStyle.snapToEdgesRatio"; from: 0; to: 0.5; value: editor.project.zoomStyle.snapToEdgesRatio; stepSize: 0.01; decimals: 2 }
    Label {
        Layout.fillWidth: true
        Layout.topMargin: -6
        text: "How close to the frame edge a zoom may look. 0 keeps the target centered; higher lets it slide to the edge."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.type.caption
    }
    PanelSlider { Layout.fillWidth: true; label: "Motion blur"; path: "zoomStyle.motionBlur"; from: 0; to: 1; value: editor.project.zoomStyle.motionBlur; stepSize: .05; decimals: 2 }
    EditorSwitch { text: "Cut instead of glide"; description: "Jump straight in and out instead of animating"; checked: editor.project.zoomStyle.instantAnimation; onToggled: editor.setProjectValue("zoomStyle.instantAnimation", checked) }
    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    EditorButton {
        Layout.fillWidth: true
        text: "Regenerate zooms"
        icon.source: "qrc:/omareel/assets/icons/lucide/sparkles.svg"
        onClicked: editor.regenerateZooms()
    }
}
