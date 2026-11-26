import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    objectName: "cursorPanel"
    spacing: theme.space.panelGap
    readonly property var cursor: editor.project.cursor
    readonly property var clickEffects: [{label:"None",value:"none"},{label:"Circle",value:"circle"}]
    readonly property var cursorStyles: [
        {label:"Light arrow", value:"light-arrow", asset:"arrow.svg"},
        {label:"Dark arrow", value:"dark-arrow", asset:"arrow-dark.svg"},
        {label:"Dot", value:"dot", asset:""},
        {label:"Hand", value:"hand", asset:"pointer.svg"}
    ]
    function clickEffectIndex(value) {
        for (let i = 0; i < clickEffects.length; ++i)
            if (clickEffects[i].value === value) return i
        return value === "ripple" ? 1 : 0
    }
    function setMotion(mass, stiffness, damping) {
        editor.beginCoalescedEdit("cursor-motion")
        editor.setProjectValue("cursor.smoothing", true, true)
        editor.setProjectValue("cursor.spring.mass", mass, true)
        editor.setProjectValue("cursor.spring.stiffness", stiffness, true)
        editor.setProjectValue("cursor.spring.damping", damping, true)
        editor.endCoalescedEdit()
    }
    function isMotion(mass, stiffness, damping) {
        return root.cursor.smoothing && Math.abs(root.cursor.spring.mass-mass) < .001
            && Math.abs(root.cursor.spring.stiffness-stiffness) < .001
            && Math.abs(root.cursor.spring.damping-damping) < .001
    }
    function motionValue() {
        if (root.isMotion(1, 900, 60)) return "natural"
        if (root.isMotion(3, 470, 70)) return "smooth"
        return ""
    }

    EditorSwitch { text: "Show cursor"; description: "Draw the smoothed pointer over the video"; checked: root.cursor.visible; onToggled: editor.setProjectValue("cursor.visible", checked) }
    PanelSlider { Layout.fillWidth: true; label: "Size"; path: "cursor.size"; from: 0.5; to: 3; value: root.cursor.size; stepSize: 0.05; decimals: 2; suffix: "×" }

    PanelHeading { text: "Style"; Layout.topMargin: 4 }
    GridLayout {
        Layout.fillWidth: true
        columns: 4
        columnSpacing: 6
        rowSpacing: 6
        Repeater {
            model: root.cursorStyles
            delegate: Rectangle {
                id: styleTile
                required property var modelData
                readonly property bool selected: root.cursor.style === modelData.value
                Layout.fillWidth: true
                Layout.preferredWidth: 1
                Layout.preferredHeight: 58
                radius: theme.controlRadius
                color: selected ? Qt.alpha(theme.accent, .12) : styleMouse.containsMouse ? theme.fillHover : theme.fill
                border.width: selected ? 1.5 : 1
                border.color: selected ? theme.accent : theme.stroke
                Behavior on color { ColorAnimation { duration: 140 } }
                Item {
                    anchors.centerIn: parent
                    width: 26; height: 26
                    scale: styleMouse.pressed ? .9 : styleMouse.containsMouse ? 1.08 : 1
                    Behavior on scale { NumberAnimation { duration: 200; easing.type: Easing.OutBack; easing.overshoot: 2 } }
                    Image {
                        anchors.centerIn: parent
                        width: 24; height: 24
                        visible: modelData.value !== "dot"
                        source: visible ? "qrc:/omareel/assets/cursors/" + modelData.asset : ""
                        sourceSize: Qt.size(96, 96)
                        smooth: true
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        visible: modelData.value === "dot"
                        width: 20; height: 20; radius: 10
                        color: Qt.alpha(theme.accent, .68)
                        border.width: 1
                        border.color: theme.strokeStrong
                    }
                }
                MouseArea {
                    id: styleMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: editor.setProjectValue("cursor.style", modelData.value)
                }
                OmLazyToolTip { target: styleTile; shown: styleMouse.containsMouse; text: modelData.label }
            }
        }
    }

    PanelHeading { text: "Motion"; Layout.topMargin: 4 }
    OmButtonGroup {
        Layout.fillWidth: true
        options: [{label:"Natural", value:"natural"}, {label:"Smooth", value:"smooth"}]
        value: root.motionValue()
        onChanged: value => {
            if (value === "natural") root.setMotion(1, 900, 60)
            else root.setMotion(3, 470, 70)
        }
    }
    Label {
        Layout.fillWidth: true
        textFormat: Text.PlainText
        text: root.motionValue() === "" ? "Using a custom spring — tune it below."
            : "Natural stays close to raw movement; Smooth uses a softer spring."
        wrapMode: Text.WordWrap
        color: theme.textMuted
        font.pixelSize: theme.type.small
    }

    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    PanelHeading { text: "Clicks" }
    OmSettingRow {
        label: "Effect"
        EditorComboBox {
            Layout.preferredWidth: 120
            model: root.clickEffects
            textRole: "label"
            currentIndex: root.clickEffectIndex(root.cursor.clickEffect)
            onActivated: editor.setProjectValue("cursor.clickEffect", model[currentIndex].value)
        }
    }
    OmSettingRow {
        label: "Sound"
        EditorComboBox {
            Layout.preferredWidth: 120
            model: [{label:"None",value:"none"},{label:"Soft",value:"soft"}]
            textRole: "label"
            currentIndex: root.cursor.clickSound === "soft" ? 1 : 0
            onActivated: editor.setProjectValue("cursor.clickSound", model[currentIndex].value)
        }
    }
    OmSettingRow {
        label: "Ring color"
        OmColorWell { swatch: root.cursor.ringColor; onClicked: ringDialog.open() }
    }
    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    EditorSwitch {
        id: idleSwitch
        text: "Hide when idle"
        description: "Fade the cursor out when it stops moving"
        checked: root.cursor.hideWhenIdleMs !== null
        onToggled: editor.setProjectValue("cursor.hideWhenIdleMs", checked ? 1500 : null)
    }
    PanelSlider { visible: idleSwitch.checked; Layout.fillWidth: true; label: "Idle delay"; path: "cursor.hideWhenIdleMs"; from: 250; to: 5000; value: root.cursor.hideWhenIdleMs || 1500; stepSize: 250; suffix: " ms" }
    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
    PanelHeading { text: "Spring" }
    PanelSlider { Layout.fillWidth: true; label: "Mass"; path: "cursor.spring.mass"; from: 0.2; to: 6; value: root.cursor.spring.mass; stepSize: 0.05; decimals: 2 }
    PanelSlider { Layout.fillWidth: true; label: "Stiffness"; path: "cursor.spring.stiffness"; from: 50; to: 1000; value: root.cursor.spring.stiffness; stepSize: 5 }
    PanelSlider { Layout.fillWidth: true; label: "Damping"; path: "cursor.spring.damping"; from: 5; to: 120; value: root.cursor.spring.damping; stepSize: 1 }
    ColorDialog { id: ringDialog; selectedColor: root.cursor.ringColor; onAccepted: editor.setProjectValue("cursor.ringColor", selectedColor) }
}
