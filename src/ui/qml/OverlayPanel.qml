import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

// The Annotate section: add annotations, and edit the selected one. Every control is one undo
// step per gesture (sliders and typing coalesce).
ColumnLayout {
    id: root
    objectName: "overlayPanel"
    spacing: theme.space.panelGap
    readonly property var kinds: [
        {kind: "text", label: "Text", icon: "type.svg", key: "T"},
        {kind: "box", label: "Box", icon: "square-dashed.svg", key: "R"},
        {kind: "arrow", label: "Arrow", icon: "arrow-up-right.svg", key: "A"},
        {kind: "badge", label: "Step", icon: "hash.svg", key: "N"},
        {kind: "blur", label: "Blur", icon: "eye-off.svg", key: "B"},
        {kind: "card", label: "Card", icon: "presentation.svg", key: "Shift T"}
    ]
    readonly property var selected: {
        for (let i = 0; i < editor.overlays.length; ++i)
            if (editor.overlays[i].id === editor.selectedOverlayId) return editor.overlays[i]
        return null
    }
    readonly property string kind: selected ? selected.kind : ""
    function update(patch) { if (root.selected) editor.updateOverlay(root.selected.id, patch, true) }
    function sliding(pressed) {
        if (pressed) editor.beginCoalescedEdit("overlay-panel-" + (root.selected ? root.selected.id : ""))
        else editor.endCoalescedEdit()
    }
    function kindLabel(kind) {
        for (const entry of kinds) if (entry.kind === kind) return entry.label
        return ""
    }

    component OverlaySlider: ColumnLayout {
        id: sliderRow
        // Inline components don't see this file's ids: the panel comes in as a property.
        required property Item panel
        property string label
        property string field
        property real from: 0
        property real to: 1
        property real stepSize: .01
        property int decimals: 0
        property string suffix: ""
        Layout.fillWidth: true
        spacing: 4
        RowLayout {
            Layout.fillWidth: true
            PanelLabel { text: sliderRow.label; Layout.fillWidth: true }
            PanelValue {
                text: panel.selected ? Number(panel.selected[sliderRow.field]).toFixed(sliderRow.decimals) + sliderRow.suffix : ""
                live: slider.pressed
            }
        }
        EditorSlider {
            id: slider
            Layout.fillWidth: true
            Layout.leftMargin: -7; Layout.rightMargin: -7
            from: sliderRow.from; to: sliderRow.to; stepSize: sliderRow.stepSize
            value: panel.selected && panel.selected[sliderRow.field] !== undefined ? panel.selected[sliderRow.field] : from
            onPressedChanged: panel.sliding(pressed)
            onMoved: {
                const patch = {}
                patch[sliderRow.field] = value
                panel.update(patch)
            }
        }
    }

    GridLayout {
        Layout.fillWidth: true
        columns: 3
        rowSpacing: 6
        columnSpacing: 6
        Repeater {
            model: root.kinds
            delegate: OmButton {
                required property var modelData
                objectName: "addOverlay-" + modelData.kind
                Layout.fillWidth: true
                bordered: true
                text: modelData.label
                icon.source: "qrc:/omareel/assets/icons/lucide/" + modelData.icon
                tooltipText: "Add " + modelData.label.toLowerCase() + " at the playhead"
                tooltipShortcut: modelData.key
                onClicked: editor.addOverlayAt(modelData.kind, editor.position, 3)
            }
        }
    }
    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }

    OmEmptyState {
        visible: root.selected === null
        iconSource: "qrc:/omareel/assets/icons/lucide/pen-line.svg"
        title: "No annotation selected"
        hint: "Pick one on the preview or the timeline, or press"
        key: "T"
    }

    ColumnLayout {
        visible: root.selected !== null
        Layout.fillWidth: true
        spacing: theme.space.panelGap

        RowLayout {
            Layout.fillWidth: true
            PanelHeading { text: root.kindLabel(root.kind); Layout.fillWidth: true }
            PanelValue {
                text: root.selected && root.selected.outputStart >= 0
                    ? editor.formatTime(root.selected.outputStart) + " – " + editor.formatTime(root.selected.outputEnd) : ""
            }
        }

        // Text and card wording.
        OmTextField {
            objectName: "overlayTextField"
            visible: root.kind === "text" || root.kind === "card"
            Layout.fillWidth: true
            placeholderText: root.kind === "card" ? "Title" : "Text"
            text: root.selected ? (root.kind === "card" ? root.selected.title : root.selected.text) || "" : ""
            onTextEdited: root.update(root.kind === "card" ? {title: text} : {text: text})
            onEditingFinished: editor.endCoalescedEdit()
        }
        OmTextField {
            visible: root.kind === "card"
            Layout.fillWidth: true
            placeholderText: "Subtitle"
            text: root.selected && root.kind === "card" ? root.selected.subtitle || "" : ""
            onTextEdited: root.update({subtitle: text})
            onEditingFinished: editor.endCoalescedEdit()
        }

        OverlaySlider { panel: root; visible: root.kind === "text"; label: "Size"; field: "size"; from: 16; to: 160; stepSize: 1; suffix: " px" }
        OmSettingRow {
            visible: root.kind === "text"
            label: "Background"
            OmButtonGroup {
                Layout.preferredWidth: 150
                options: [{label: "Pill", value: "pill"}, {label: "None", value: "none"}]
                value: root.selected ? root.selected.background : "pill"
                onChanged: value => root.update({background: value})
            }
        }
        OverlaySlider { panel: root; visible: root.kind === "box" || root.kind === "arrow"; label: "Line"; field: "width"; from: 1; to: 24; stepSize: 1; suffix: " px" }
        OverlaySlider { panel: root; visible: root.kind === "box"; label: "Corners"; field: "radius"; from: 0; to: 80; stepSize: 1; suffix: " px" }
        OverlaySlider { panel: root; visible: root.kind === "box"; label: "Spotlight"; field: "spotlight"; decimals: 2 }
        OverlaySlider { panel: root; visible: root.kind === "badge"; label: "Number"; field: "number"; from: 1; to: 30; stepSize: 1 }
        OmSettingRow {
            visible: root.kind === "blur"
            label: "Style"
            OmButtonGroup {
                Layout.preferredWidth: 170
                options: [{label: "Pixelate", value: "pixelate"}, {label: "Blur", value: "blur"}]
                value: root.selected ? root.selected.style : "pixelate"
                onChanged: value => root.update({style: value})
            }
        }
        OverlaySlider { panel: root; visible: root.kind === "blur"; label: "Strength"; field: "amount"; decimals: 2 }
        OmSettingRow {
            visible: ["text", "box", "arrow", "badge"].indexOf(root.kind) >= 0
            label: "Color"
            RowLayout {
                spacing: 6
                OmButton {
                    visible: root.selected !== null && !!root.selected.color
                    text: "Theme"
                    onClicked: root.update({color: ""})
                }
                OmColorWell {
                    swatch: root.selected && root.selected.color ? root.selected.color
                        : root.kind === "text" ? editor.project.renderTheme.foreground : editor.project.renderTheme.accent
                    onClicked: colorDialog.open()
                }
            }
        }
        EditorSwitch {
            visible: root.kind !== "blur" && root.kind !== "card"
            text: "Follow the recording"
            description: "Moves with zooms and keeps to the recording's moment through cuts"
            checked: root.selected !== null && root.selected.anchor === "screen"
            onToggled: if (root.selected) editor.updateOverlay(root.selected.id, {anchor: checked ? "screen" : "frame"}, false)
        }
        OverlaySlider { panel: root; visible: root.kind !== "blur"; label: "Fade in"; field: "fadeIn"; to: 2; decimals: 1; suffix: " s" }
        OverlaySlider { panel: root; visible: root.kind !== "blur"; label: "Fade out"; field: "fadeOut"; to: 2; decimals: 1; suffix: " s" }
        Label {
            visible: root.kind === "blur"
            Layout.fillWidth: true
            text: "Hides the recording under it from the first frame to the last, in the preview and the export."
            wrapMode: Text.WordWrap
            color: theme.textMuted
            font.pixelSize: theme.type.caption
        }
        EditorButton {
            Layout.fillWidth: true
            text: "Remove"
            icon.source: "qrc:/omareel/assets/icons/lucide/trash-2.svg"
            destructive: true
            onClicked: editor.removeOverlay(editor.selectedOverlayId)
        }
    }
    ColorDialog {
        id: colorDialog
        selectedColor: root.selected && root.selected.color ? root.selected.color : editor.project.renderTheme.accent
        onAccepted: root.update({color: selectedColor.toString()})
    }
}
