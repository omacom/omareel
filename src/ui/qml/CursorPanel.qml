import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    objectName: "cursorPanel"
    spacing: 12
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

    Switch { Layout.preferredHeight: 32; text: "Show cursor"; checked: root.cursor.visible; onToggled: editor.setProjectValue("cursor.visible", checked) }
    PanelSlider { Layout.fillWidth: true; label: "Size"; path: "cursor.size"; from: 0.5; to: 3; value: root.cursor.size; stepSize: 0.05; decimals: 2 }

    PanelHeading { text: "Style"; topPadding: 4 }
    GridLayout {
        Layout.fillWidth: true
        columns: 2
        columnSpacing: 8
        rowSpacing: 8
        Repeater {
            model: root.cursorStyles
            delegate: Rectangle {
                required property var modelData
                Layout.fillWidth: true
                Layout.preferredHeight: 72
                radius: 8
                color: root.cursor.style === modelData.value
                    ? Qt.alpha(theme.accent, .16) : Qt.alpha(theme.foreground, .035)
                border.width: root.cursor.style === modelData.value ? 2 : 1
                border.color: root.cursor.style === modelData.value
                    ? theme.accent : Qt.alpha(theme.foreground, .13)
                Column {
                    anchors.centerIn: parent
                    spacing: 5
                    Item {
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: 30; height: 30
                        Image {
                            anchors.centerIn: parent
                            width: 26; height: 26
                            visible: modelData.value !== "dot"
                            source: visible ? "qrc:/omarecord/assets/cursors/" + modelData.asset : ""
                            sourceSize: Qt.size(104, 104)
                            smooth: true
                        }
                        Rectangle {
                            anchors.centerIn: parent
                            visible: modelData.value === "dot"
                            width: 26; height: 26; radius: 13
                            color: Qt.alpha(theme.accent, .68)
                            border.width: 1
                            border.color: Qt.alpha(theme.foreground, .45)
                        }
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: modelData.label
                        color: theme.foreground
                        font.pixelSize: 11
                        font.weight: root.cursor.style === modelData.value ? Font.DemiBold : Font.Normal
                    }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: editor.setProjectValue("cursor.style", modelData.value)
                }
            }
        }
    }

    PanelHeading { text: "Motion"; topPadding: 4 }
    Rectangle {
        Layout.fillWidth: true
        Layout.preferredHeight: 34
        radius: 7
        color: Qt.alpha(theme.foreground, .055)
        border.width: 1
        border.color: Qt.alpha(theme.foreground, .12)
        RowLayout {
            anchors.fill: parent
            spacing: 0
            Repeater {
                model: [
                    {label:"Natural", mass:1, stiffness:900, damping:60},
                    {label:"Smooth", mass:3, stiffness:470, damping:70}
                ]
                delegate: Button {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    flat: true
                    text: modelData.label
                    checked: root.isMotion(modelData.mass, modelData.stiffness, modelData.damping)
                    font.pixelSize: 11
                    font.weight: checked ? Font.DemiBold : Font.Normal
                    onClicked: root.setMotion(modelData.mass, modelData.stiffness, modelData.damping)
                    background: Rectangle {
                        radius: 6
                        color: parent.checked ? Qt.alpha(theme.accent, .22)
                            : parent.hovered ? Qt.alpha(theme.foreground, .06) : "transparent"
                        border.width: parent.checked ? 1 : 0
                        border.color: theme.accent
                    }
                }
            }
        }
    }
    Label {
        Layout.fillWidth: true
        text: "Natural stays close to raw movement; Smooth uses a softer spring."
        wrapMode: Text.WordWrap
        color: Qt.alpha(theme.foreground, .58)
        font.pixelSize: 11
    }

    PanelLabel { text: "Click effect" }
    ComboBox {
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        model: root.clickEffects
        textRole: "label"
        currentIndex: root.clickEffectIndex(root.cursor.clickEffect)
        onActivated: editor.setProjectValue("cursor.clickEffect", model[currentIndex].value)
    }
    PanelLabel { text: "Click sound" }
    ComboBox {
        Layout.fillWidth: true
        Layout.preferredHeight: 32
        model: [{label:"None",value:"none"},{label:"Soft",value:"soft"}]
        textRole: "label"
        currentIndex: root.cursor.clickSound === "soft" ? 1 : 0
        onActivated: editor.setProjectValue("cursor.clickSound", model[currentIndex].value)
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
    PanelHeading { text: "Custom spring"; topPadding: 8 }
    PanelSlider { Layout.fillWidth: true; label: "Mass"; path: "cursor.spring.mass"; from: 0.2; to: 6; value: root.cursor.spring.mass; stepSize: 0.05; decimals: 2 }
    PanelSlider { Layout.fillWidth: true; label: "Stiffness"; path: "cursor.spring.stiffness"; from: 50; to: 1000; value: root.cursor.spring.stiffness; stepSize: 5 }
    PanelSlider { Layout.fillWidth: true; label: "Damping"; path: "cursor.spring.damping"; from: 5; to: 120; value: root.cursor.spring.damping; stepSize: 1 }
    ColorDialog { id: ringDialog; selectedColor: root.cursor.ringColor; onAccepted: editor.setProjectValue("cursor.ringColor", selectedColor) }
}
