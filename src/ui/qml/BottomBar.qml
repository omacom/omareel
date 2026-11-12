import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Layouts
import Omareel.Ui

Rectangle {
    id: root
    height: 52
    color: theme.surface
    property real timelineScale: 1
    signal timelineScaleRequested(real value)
    property bool cropMode: false
    readonly property bool compact: width < 1100
    readonly property bool narrow: width < 900
    readonly property var aspects: [
        {text:"Auto", value:"auto"}, {text:"Wide 16:9", value:"16:9"},
        {text:"Vertical 9:16", value:"9:16"}, {text:"Square 1:1", value:"1:1"},
        {text:"Portrait 4:5", value:"4:5"}, {text:"Classic 4:3", value:"4:3"},
        {text:"Tall 3:4", value:"3:4"}
    ]
    readonly property var shortcuts: [
        {key:"Space", action:"Play / pause"}, {key:"← →", action:"Step one frame"},
        {key:"Shift ← →", action:"Jump one second"}, {key:"S", action:"Split at playhead"},
        {key:"Z", action:"Add zoom"}, {key:"I", action:"Insert a video"},
        {key:"T R A N", action:"Add text, box, arrow, step"},
        {key:"B", action:"Add a blur region"}, {key:"Shift T", action:"Add a title card"},
        {key:"M", action:"Next marker"}, {key:"Shift M", action:"Previous marker"},
        {key:"Ctrl M", action:"Add marker"},
        {key:"Delete", action:"Remove selection"},
        {key:"Backspace", action:"Remove range"},
        {key:"Ctrl Z", action:"Undo"}, {key:"Ctrl Shift Z", action:"Redo"},
        {key:"Ctrl S", action:"Save"}, {key:"Ctrl E", action:"Export"},
        {key:"Scroll", action:"Zoom timeline at pointer"},
        {key:"Right drag", action:"Pan timeline"},
        {key:"Esc", action:"Cancel point picking"}, {key:"?", action:"Show shortcuts"}
    ]

    function aspectIndex(value) {
        for (let i = 0; i < aspects.length; ++i)
            if (aspects[i].value === value) return i
        return 0
    }
    function shortAspect(value) { return value === "auto" ? "Auto" : value }
    // Reframing needs a canvas of another shape than the (cropped) recording, such as 9:16 or
    // 1:1. As reframe::active: the window would be within 2% of the whole crop otherwise, and a
    // project set to reframe such a canvas draws exactly as one that isn't.
    readonly property bool reframeUseful: {
        const source = editor.sourceWidth * editor.project.crop.w / Math.max(1, editor.sourceHeight * editor.project.crop.h)
        const canvas = editor.outputWidth / Math.max(1, editor.outputHeight)
        return Math.min(canvas / Math.max(.0001, source), source / Math.max(.0001, canvas)) < 1 - .02
    }
    readonly property bool reframing: editor.project.reframe === "follow"
    function toggleReframe() { editor.setReframe(root.reframing ? "off" : "follow") }
    function openAspectMenu() {
        if (root.narrow) overflowMenu.open()
        else aspectBox.popup.open()
    }
    function openShortcuts() { shortcutsSheet.open() }
    function openInsertDialog() { if (!editor.importing) insertDialog.open() }
    InsertClipDialog { id: insertDialog }
    function formatTenths(seconds) {
        const tenths = Math.max(0, Math.round(seconds * 10))
        const minutes = Math.floor(tenths / 600)
        const remainder = tenths % 600
        const whole = Math.floor(remainder / 10)
        return String(minutes).padStart(2, "0") + ":"
             + String(whole).padStart(2, "0") + "." + (remainder % 10)
    }

    Rectangle {
        anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
        height: 1; color: theme.hairline
    }
    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12; anchors.rightMargin: 12
        spacing: 8

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: root.narrow ? 36 : root.compact ? 240 : 336
            Layout.fillHeight: true
            RowLayout {
                anchors.left: parent.left; anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                EditorComboBox {
                    id: aspectBox
                    objectName: "aspectMenu"
                    visible: !root.narrow
                    Layout.preferredWidth: root.compact ? 104 : 138
                    Layout.minimumWidth: Layout.preferredWidth
                    leadingIconSource: "qrc:/omareel/assets/icons/lucide/square-dashed.svg"
                    model: root.aspects
                    textRole: "text"
                    displayText: root.compact
                        ? root.shortAspect(root.aspects[Math.max(0, currentIndex)].value)
                        : root.aspects[Math.max(0, currentIndex)].text
                    currentIndex: root.aspectIndex(editor.project.aspect)
                    enabled: !editor.readOnly
                    onActivated: editor.setProjectValue("aspect", model[currentIndex].value)
                }
                OmButton {
                    visible: !root.narrow
                    Layout.preferredWidth: implicitWidth
                    text: root.cropMode ? "Done" : "Crop"
                    icon.source: root.cropMode ? "qrc:/omareel/assets/icons/lucide/check.svg"
                                               : "qrc:/omareel/assets/icons/lucide/crop.svg"
                    icon.color: root.cropMode ? theme.accent : contentColor
                    checkable: true; checked: root.cropMode; selected: checked
                    enabled: !editor.readOnly
                    tooltipText: root.cropMode ? "" : "Crop the recording"
                    onToggled: root.cropMode = checked
                }
                OmButton {
                    objectName: "reframeButton"
                    visible: !root.narrow
                    Layout.preferredWidth: implicitWidth
                    text: root.compact ? "" : "Reframe"
                    icon.source: "qrc:/omareel/assets/icons/lucide/scan.svg"
                    icon.color: root.reframing ? theme.accent : contentColor
                    checkable: true; checked: root.reframing; selected: checked
                    enabled: root.reframing || root.reframeUseful
                    tooltipText: root.reframing && !root.reframeUseful ? "Nothing to reframe: the canvas has the recording's shape"
                        : root.reframing ? "The canvas follows the cursor and zooms"
                        : root.reframeUseful ? "Fill the canvas and follow the action"
                        : "Pick a vertical or square canvas to reframe"
                    Accessible.name: "Auto-reframe"
                    onToggled: root.toggleReframe()
                }
                OmIconButton {
                    id: overflowButton
                    visible: root.narrow
                    width: 32; height: 32
                    icon.source: "qrc:/omareel/assets/icons/lucide/square-dashed.svg"
                    tooltipText: "Canvas options"
                    Accessible.name: "Canvas options"
                    onClicked: overflowMenu.visible ? overflowMenu.close() : overflowMenu.open()
                }
            }
        }

        RowLayout {
            Layout.alignment: Qt.AlignHCenter
            spacing: 4
            Label {
                Layout.preferredWidth: positionMetrics.advanceWidth
                horizontalAlignment: Text.AlignRight
                Layout.rightMargin: 8
                textFormat: Text.PlainText
                text: root.formatTenths(editor.position)
                color: theme.textStrong
                font.family: theme.monoFamily
                font.pixelSize: theme.type.small
                font.weight: Font.Medium
                TextMetrics { id: positionMetrics; font.family: theme.monoFamily; font.pixelSize: theme.type.small; text: "00:00.0" }
            }
            OmIconButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/skip-back.svg"
                Accessible.name: "Go to start"
                tooltipText: "Go to start"
                onClicked: editor.seekBoundary(-1)
            }
            Button {
                id: playButton
                Layout.preferredWidth: 36; Layout.preferredHeight: 36
                Layout.leftMargin: 2; Layout.rightMargin: 2
                hoverEnabled: true
                focusPolicy: Qt.NoFocus
                Accessible.name: editor.playing ? "Pause" : "Play"
                onClicked: editor.playPause()
                scale: down ? .92 : 1
                Behavior on scale { NumberAnimation { duration: down ? 70 : 260; easing.type: Easing.OutBack; easing.overshoot: 2.4 } }
                background: Rectangle {
                    radius: theme.rounded ? width / 2 : 0
                    color: playButton.hovered ? Qt.lighter(theme.textStrong, 1.08) : theme.textStrong
                    Behavior on color { ColorAnimation { duration: 140 } }
                }
                contentItem: Item {
                    IconImage {
                        anchors.centerIn: parent
                        anchors.horizontalCenterOffset: editor.playing ? 0 : 1
                        width: 16; height: 16
                        source: editor.playing ? "qrc:/omareel/assets/icons/lucide/pause.svg"
                                               : "qrc:/omareel/assets/icons/lucide/play.svg"
                        sourceSize: Qt.size(16, 16)
                        color: theme.surface
                    }
                }
                OmLazyToolTip { target: playButton; shown: playButton.hovered; text: editor.playing ? "Pause" : "Play"; shortcut: "Space" }
            }
            OmIconButton {
                icon.source: "qrc:/omareel/assets/icons/lucide/skip-forward.svg"
                Accessible.name: "Go to end"
                tooltipText: "Go to end"
                onClicked: editor.seekBoundary(1)
            }
            Label {
                Layout.preferredWidth: positionMetrics.advanceWidth
                Layout.leftMargin: 8
                textFormat: Text.PlainText
                text: root.formatTenths(editor.duration)
                color: theme.textFaint
                font.family: theme.monoFamily
                font.pixelSize: theme.type.small
            }
        }

        Item {
            Layout.fillWidth: true
            Layout.minimumWidth: root.compact ? 268 : 482
            Layout.fillHeight: true
            RowLayout {
                anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                OmButton {
                    objectName: "insertClipToolButton"
                    Layout.preferredWidth: root.compact ? 32 : implicitWidth
                    text: root.compact ? "" : "Insert"
                    icon.source: "qrc:/omareel/assets/icons/lucide/film.svg"
                    enabled: !editor.importing && !editor.readOnly
                    Accessible.name: "Insert video"
                    tooltipText: "Insert a video at the playhead"
                    tooltipShortcut: "I"
                    onClicked: root.openInsertDialog()
                }
                OmButton {
                    Layout.preferredWidth: root.compact ? 32 : implicitWidth
                    text: root.compact ? "" : "Split"
                    enabled: !editor.readOnly
                    icon.source: "qrc:/omareel/assets/icons/lucide/scissors.svg"
                    Accessible.name: "Split clip"
                    tooltipText: "Split at playhead"
                    tooltipShortcut: "S"
                    onClicked: editor.splitAtPlayhead()
                }
                OmButton {
                    objectName: "annotateToolButton"
                    Layout.preferredWidth: root.compact ? 32 : implicitWidth
                    text: root.compact ? "" : "Annotate"
                    icon.source: "qrc:/omareel/assets/icons/lucide/pen-line.svg"
                    Accessible.name: "Annotate"
                    tooltipText: "Text, boxes, arrows, steps, blur, and cards"
                    tooltipShortcut: "T"
                    onClicked: annotateMenu.visible ? annotateMenu.close() : annotateMenu.open()
                }
                Rectangle { Layout.preferredWidth: 1; Layout.preferredHeight: 18; Layout.leftMargin: 6; Layout.rightMargin: 6; color: theme.separator }
                OmIconButton {
                    objectName: "timelineZoomOut"
                    Layout.preferredWidth: 26; Layout.preferredHeight: 26
                    width: 26; height: 26
                    icon.source: "qrc:/omareel/assets/icons/lucide/zoom-out.svg"
                    icon.width: 15; icon.height: 15
                    Accessible.name: "Zoom timeline out"
                    onClicked: root.timelineScaleRequested(Math.max(1, root.timelineScale / 1.25))
                }
                EditorSlider {
                    objectName: "timelineZoomSlider"
                    from: 1; to: 16; value: root.timelineScale
                    Layout.preferredWidth: root.compact ? 76 : 96
                    Layout.minimumWidth: Layout.preferredWidth
                    onMoved: root.timelineScaleRequested(value)
                    Accessible.name: "Timeline zoom"
                }
                OmIconButton {
                    objectName: "timelineZoomIn"
                    Layout.preferredWidth: 26; Layout.preferredHeight: 26
                    width: 26; height: 26
                    icon.source: "qrc:/omareel/assets/icons/lucide/zoom-in.svg"
                    icon.width: 15; icon.height: 15
                    Accessible.name: "Zoom timeline in"
                    onClicked: root.timelineScaleRequested(Math.min(16, root.timelineScale * 1.25))
                }
                Button {
                    id: shortcutsButton
                    Layout.leftMargin: 6
                    Layout.preferredWidth: 28; Layout.preferredHeight: 28
                    hoverEnabled: true
                    focusPolicy: Qt.NoFocus
                    Accessible.name: "Keyboard shortcuts"
                    onClicked: shortcutsSheet.visible ? shortcutsSheet.close() : shortcutsSheet.open()
                    background: Item {}
                    contentItem: Item {
                        OmKeycap {
                            anchors.centerIn: parent
                            text: "?"
                            subtle: !shortcutsButton.hovered && !shortcutsSheet.visible
                            scale: shortcutsButton.down ? .92 : 1
                            Behavior on scale { NumberAnimation { duration: 160; easing.type: Easing.OutBack } }
                        }
                    }
                    OmLazyToolTip { target: shortcutsButton; shown: shortcutsButton.hovered && !shortcutsSheet.visible; text: "Keyboard shortcuts" }
                }
            }
        }
    }

    OmPopup {
        id: annotateMenu
        parent: root
        fromBelow: false
        x: root.width - width - 12; y: -height - 8
        width: 224
        padding: 8
        property bool wanted: false
        onAboutToShow: wanted = true
        contentItem: Loader {
            active: annotateMenu.wanted
            sourceComponent: ColumnLayout {
                spacing: 2
                PanelHeading { text: "Add at the playhead"; Layout.leftMargin: 8; Layout.topMargin: 4; Layout.bottomMargin: 4 }
                Repeater {
                    model: [
                        {kind: "text", label: "Text", icon: "type.svg", key: "T"},
                        {kind: "box", label: "Box or spotlight", icon: "square-dashed.svg", key: "R"},
                        {kind: "arrow", label: "Arrow", icon: "arrow-up-right.svg", key: "A"},
                        {kind: "badge", label: "Step number", icon: "hash.svg", key: "N"},
                        {kind: "blur", label: "Blur region", icon: "eye-off.svg", key: "B"},
                        {kind: "card", label: "Title card", icon: "presentation.svg", key: "Shift T"}
                    ]
                    delegate: OmButton {
                        required property var modelData
                        objectName: "annotateMenu-" + modelData.kind
                        Layout.fillWidth: true
                        text: modelData.label
                        icon.source: "qrc:/omareel/assets/icons/lucide/" + modelData.icon
                        tooltipText: modelData.label
                        tooltipShortcut: modelData.key
                        onClicked: {
                            editor.addOverlayAt(modelData.kind, editor.position, 3)
                            annotateMenu.close()
                        }
                    }
                }
            }
        }
    }

    OmPopup {
        id: overflowMenu
        parent: root
        fromBelow: false
        x: 12; y: -height - 8
        width: 224
        padding: 8
        // Made on first open: the editor's start doesn't build menus nobody has opened.
        property bool wanted: false
        onAboutToShow: wanted = true
        contentItem: Loader { active: overflowMenu.wanted; sourceComponent: overflowContent }
    }
    Component {
        id: overflowContent
        ColumnLayout {
            spacing: 2
            PanelHeading { text: "Canvas"; Layout.leftMargin: 8; Layout.topMargin: 4; Layout.bottomMargin: 4 }
            Repeater {
                model: root.aspects
                delegate: OmButton {
                    required property var modelData
                    Layout.fillWidth: true
                    enabled: !editor.readOnly
                    selected: editor.project.aspect === modelData.value
                    text: modelData.text
                    onClicked: {
                        editor.setProjectValue("aspect", modelData.value)
                        overflowMenu.close()
                    }
                }
            }
            OmSeparator { Layout.fillWidth: true; Layout.topMargin: 4; Layout.bottomMargin: 4 }
            OmButton {
                Layout.fillWidth: true
                bordered: true
                selected: root.cropMode
                enabled: !editor.readOnly
                text: root.cropMode ? "Finish crop" : "Crop"
                icon.source: "qrc:/omareel/assets/icons/lucide/crop.svg"
                onClicked: { root.cropMode = !root.cropMode; overflowMenu.close() }
            }
            OmButton {
                Layout.fillWidth: true
                bordered: true
                selected: root.reframing
                enabled: root.reframing || root.reframeUseful
                text: root.reframing ? "Stop reframing" : "Reframe"
                icon.source: "qrc:/omareel/assets/icons/lucide/scan.svg"
                onClicked: { root.toggleReframe(); overflowMenu.close() }
            }
        }
    }

    OmPopup {
        id: shortcutsSheet
        objectName: "shortcutsSheet"
        parent: root
        fromBelow: false
        x: Math.max(12, root.width - width - 12)
        y: -height - 8
        width: Math.min(560, root.width - 24)
        padding: 20
        property bool wanted: false
        onAboutToShow: wanted = true
        contentItem: Loader { active: shortcutsSheet.wanted; sourceComponent: shortcutsContent }
    }
    Component {
        id: shortcutsContent
        ColumnLayout {
            spacing: 16
            RowLayout {
                Layout.fillWidth: true
                Label {
                    text: "Keyboard shortcuts"
                    color: theme.textStrong
                    font.pixelSize: theme.type.title
                    font.weight: Font.DemiBold
                }
                Item { Layout.fillWidth: true }
                OmIconButton {
                    width: 24; height: 24
                    icon.width: 14; icon.height: 14
                    icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
                    Accessible.name: "Close shortcuts"
                    onClicked: shortcutsSheet.close()
                }
            }
            GridLayout {
                Layout.fillWidth: true
                columns: 2
                columnSpacing: 28; rowSpacing: 10
                Repeater {
                    model: root.shortcuts
                    delegate: RowLayout {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.preferredWidth: 1
                        spacing: 8
                        Label {
                            Layout.fillWidth: true
                            textFormat: Text.PlainText
                            text: modelData.action
                            color: theme.foreground
                            font.pixelSize: theme.type.body
                            elide: Text.ElideRight
                        }
                        Row {
                            spacing: 3
                            Repeater {
                                model: modelData.key.split(" ")
                                delegate: OmKeycap { required property string modelData; text: modelData }
                            }
                        }
                    }
                }
            }
        }
    }
}
