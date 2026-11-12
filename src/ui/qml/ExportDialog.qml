import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Shapes
import Omareel.Ui

Dialog {
    id: root
    objectName: "exportDialog"
    width: 460
    anchors.centerIn: parent
    modal: true
    padding: 20
    standardButtons: Dialog.NoButton
    closePolicy: editor.exporting ? Popup.NoAutoClose : Popup.CloseOnEscape
    property string format: "mp4"
    property int heightValue: 1080
    property int fpsValue: 60
    property string qualityValue: "social"
    property string outputPath: editor.defaultExportPath(format)
    property string successPath: ""
    property var previewGrab: null
    property url previewUrl: ""
    readonly property var heights: format === "mp4"
        ? [{label:"720p",value:720},{label:"1080p",value:1080},{label:"4K",value:2160}]
        : [{label:"480p",value:480},{label:"720p",value:720},{label:"1080p",value:1080}]
    readonly property var rates: format === "mp4"
        ? [{label:"30",value:30},{label:"60",value:60}]
        : [{label:"15",value:15},{label:"20",value:20},{label:"30",value:30}]
    readonly property var qualities: [
        {label:"Web",value:"web-high",caption:"Compact files for quick sharing."},
        {label:"Social",value:"social",caption:"Balanced size and crisp motion."},
        {label:"Studio",value:"studio",caption:"Maximum detail for further editing."}
    ]

    component SegmentRow: OmButtonGroup {
        id: segment
        required property var choices
        required property var selectedValue
        signal chosen(var value)
        options: choices
        value: selectedValue
        onChanged: value => segment.chosen(value)
    }

    function selectFormat(value) {
        format = value
        if (value === "mp4") {
            heightValue = editor.project.export.height
            fpsValue = editor.project.export.fps
            qualityValue = editor.project.export.quality
        } else {
            heightValue = editor.project.export.gif.height
            fpsValue = editor.project.export.gif.fps
            qualityValue = editor.project.export.gif.quality
        }
        outputPath = editor.defaultExportPath(format)
        editor.setProjectValue("export.format", value)
    }
    function qualityCaption() {
        for (let i = 0; i < qualities.length; ++i)
            if (qualities[i].value === qualityValue) return qualities[i].caption
        return ""
    }
    function estimatedMegabytes() {
        const scale = heightValue / 1080
        const qualityBase = qualityValue === "studio" ? 18 : qualityValue === "social" ? 10 : 5
        const bitrate = format === "gif" ? qualityBase * 1.8 * scale : qualityBase * scale * scale
        return Math.max(.1, bitrate * editor.duration / 8)
    }
    function formatEta(seconds) {
        if (seconds < 60) return seconds + "s"
        return Math.floor(seconds / 60) + "m " + (seconds % 60) + "s"
    }
    onOpened: {
        successPath = ""
        selectFormat(editor.project.export.format || "mp4")
    }

    background: OmPopupCard { padding: 0 }
    Overlay.modal: Rectangle { color: Qt.alpha(theme.canvas, .65) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 160; easing.type: Easing.OutCubic }
        NumberAnimation { property: "scale"; from: .96; to: 1; duration: 240; easing.type: Easing.OutQuint }
    }
    exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: 110 } }
    readonly property bool done: successPath.length > 0
    contentItem: ColumnLayout {
        spacing: 14
        RowLayout {
            Layout.fillWidth: true
            ColumnLayout {
                spacing: 2
                Label { textFormat: Text.PlainText; text: root.done ? "Export complete" : "Export"; font.pixelSize: theme.type.title; font.weight: Font.DemiBold; color: theme.textStrong }
                Label {
                    textFormat: Text.PlainText
                    text: editor.formatTime(editor.duration) + "  ·  " + editor.sourceWidth + "×" + editor.sourceHeight + " source"
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.caption
                    color: theme.textMuted
                }
            }
            Item { Layout.fillWidth: true }
            OmIconButton {
                enabled: !editor.exporting
                icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
                Accessible.name: "Close export"
                onClicked: root.close()
            }
        }
        // The current frame doubles as the progress surface while the export runs.
        Rectangle {
            id: thumbnail
            Layout.fillWidth: true
            Layout.preferredHeight: Math.round(width * 9 / 16)
            radius: theme.controlRadius
            color: theme.canvas
            clip: true
            Image {
                anchors.fill: parent
                source: root.previewUrl
                fillMode: Image.PreserveAspectCrop
                smooth: true
                scale: editor.exporting ? 1.03 : 1
                Behavior on scale { NumberAnimation { duration: 600; easing.type: Easing.OutCubic } }
            }
            Rectangle {
                anchors.fill: parent
                color: Qt.alpha(theme.canvas, .62)
                opacity: editor.exporting || root.done ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 260 } }
            }
            Label {
                anchors.centerIn: parent
                visible: root.previewUrl.toString() === ""
                text: "Current frame"
                color: theme.textFaint
            }
            Item {
                anchors.centerIn: parent
                width: 72; height: 72
                visible: editor.exporting
                Shape {
                    anchors.fill: parent
                    preferredRendererType: Shape.CurveRenderer
                    ShapePath {
                        strokeColor: Qt.alpha(theme.foreground, .16)
                        strokeWidth: 4
                        fillColor: "transparent"
                        PathAngleArc { centerX: 36; centerY: 36; radiusX: 32; radiusY: 32; startAngle: 0; sweepAngle: 360 }
                    }
                    ShapePath {
                        strokeColor: theme.accent
                        strokeWidth: 4
                        capStyle: theme.rounded ? ShapePath.RoundCap : ShapePath.FlatCap
                        fillColor: "transparent"
                        PathAngleArc {
                            centerX: 36; centerY: 36; radiusX: 32; radiusY: 32; startAngle: -90
                            sweepAngle: 360 * Math.min(.99, editor.exportProgress)
                            Behavior on sweepAngle { NumberAnimation { duration: 200 } }
                        }
                    }
                }
                Label {
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: Math.min(99, Math.round(editor.exportProgress * 100)) + "%"
                    color: theme.textStrong
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.title
                    font.weight: Font.DemiBold
                }
            }
            Rectangle {
                id: doneBadge
                anchors.centerIn: parent
                width: 56; height: 56
                radius: theme.rounded ? 28 : 0
                color: theme.accent
                visible: root.done
                scale: root.done ? 1 : .4
                Behavior on scale { NumberAnimation { duration: 420; easing.type: Easing.OutBack; easing.overshoot: 2.2 } }
                IconImage {
                    anchors.centerIn: parent
                    width: 26; height: 26
                    source: "qrc:/omareel/assets/icons/lucide/check.svg"
                    sourceSize: Qt.size(26, 26)
                    color: theme.accentForeground
                }
            }
            Rectangle {
                anchors.left: parent.left; anchors.bottom: parent.bottom
                anchors.margins: 10
                width: specLabel.implicitWidth + 14
                height: 22
                radius: theme.smallRadius
                color: Qt.alpha(theme.canvas, .8)
                visible: !editor.exporting && !root.done
                Label {
                    id: specLabel
                    anchors.centerIn: parent
                    textFormat: Text.PlainText
                    text: root.format.toUpperCase() + "  " + (root.heightValue === 2160 ? "4K" : root.heightValue + "p") + "  " + root.fpsValue + " fps"
                    color: theme.textStrong
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.caption
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: !root.done
            enabled: !editor.exporting
            opacity: enabled ? 1 : .5
            spacing: 10
            Behavior on opacity { NumberAnimation { duration: 200 } }
            OmSettingRow {
                label: "Format"
                SegmentRow {
                    Layout.preferredWidth: 216
                    choices: [{label:"MP4",value:"mp4"},{label:"GIF",value:"gif"}]
                    selectedValue: root.format
                    onChosen: value => root.selectFormat(value)
                }
            }
            OmSettingRow {
                label: "Resolution"
                SegmentRow {
                    Layout.preferredWidth: 216
                    choices: root.heights; selectedValue: root.heightValue
                    onChosen: value => {
                        root.heightValue = value
                        editor.setProjectValue(root.format === "mp4" ? "export.height" : "export.gif.height", value)
                    }
                }
            }
            OmSettingRow {
                label: "Frame rate"
                SegmentRow {
                    Layout.preferredWidth: 216
                    choices: root.rates; selectedValue: root.fpsValue
                    onChosen: value => {
                        root.fpsValue = value
                        editor.setProjectValue(root.format === "mp4" ? "export.fps" : "export.gif.fps", value)
                    }
                }
            }
            OmSettingRow {
                label: "Quality"
                description: root.qualityCaption()
                SegmentRow {
                    Layout.preferredWidth: 216
                    choices: root.qualities; selectedValue: root.qualityValue
                    onChosen: value => {
                        root.qualityValue = value
                        editor.setProjectValue(root.format === "mp4" ? "export.quality" : "export.gif.quality", value)
                    }
                }
            }
            OmSeparator { Layout.fillWidth: true; Layout.topMargin: 2 }
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                IconImage {
                    Layout.preferredWidth: 15; Layout.preferredHeight: 15
                    source: "qrc:/omareel/assets/icons/lucide/folder-open.svg"
                    sourceSize: Qt.size(15, 15)
                    color: theme.textMuted
                }
                Label {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    text: root.outputPath.replace(/^\/home\/[^/]+/, "~")
                    elide: Text.ElideMiddle
                    color: theme.foreground
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.caption
                }
                OmButton { text: "Change…"; font.pixelSize: theme.type.small; onClicked: outputDialog.open() }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            visible: editor.exporting
            spacing: 8
            ProgressBar {
                id: progress
                objectName: "exportProgressBar"
                Layout.fillWidth: true
                // The encoder still has to flush once every frame is rendered.
                value: editor.exporting ? Math.min(.99, editor.exportProgress) : editor.exportProgress
                background: Rectangle { implicitHeight: 4; color: Qt.alpha(theme.foreground, .12); radius: theme.rounded ? 2 : 0 }
                contentItem: Item {
                    implicitHeight: 4
                    clip: true
                    Rectangle {
                        width: progress.visualPosition * parent.width; height: parent.height
                        color: theme.accent; radius: theme.rounded ? 2 : 0
                        Behavior on width { NumberAnimation { duration: 200 } }
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Label {
                    objectName: "exportStatusLabel"
                    textFormat: Text.PlainText
                    text: editor.exportProgress >= 1 ? "Finishing encoding…" : "Rendering…"
                    color: theme.foreground
                    font.pixelSize: theme.type.small
                }
                Item { Layout.fillWidth: true }
                Label {
                    visible: editor.exportProgress < 1
                    textFormat: Text.PlainText
                    text: editor.exportFps.toFixed(0) + " fps  ·  " + root.formatEta(editor.exportEtaSeconds) + " left"
                    color: theme.textMuted
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.caption
                }
            }
        }
        Label {
            Layout.fillWidth: true
            visible: editor.exportError.length > 0 && !editor.exporting
            textFormat: Text.PlainText
            text: editor.exportError
            color: theme.record
            wrapMode: Text.WordWrap
            font.pixelSize: theme.type.small
        }
        Label {
            Layout.fillWidth: true
            visible: root.done
            textFormat: Text.PlainText
            text: root.successPath.replace(/^\/home\/[^/]+/, "~")
            elide: Text.ElideMiddle
            horizontalAlignment: Text.AlignHCenter
            color: theme.textMuted
            font.family: theme.monoFamily
            font.pixelSize: theme.type.caption
        }
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 2
            spacing: 8
            Label {
                visible: !editor.exporting && !root.done
                textFormat: Text.PlainText
                text: "≈ " + root.estimatedMegabytes().toFixed(root.estimatedMegabytes() < 10 ? 1 : 0) + " MB"
                color: theme.textMuted
                font.family: theme.monoFamily
                font.pixelSize: theme.type.small
            }
            OmButton {
                visible: root.done
                bordered: true
                text: "Show in folder"
                icon.source: "qrc:/omareel/assets/icons/lucide/folder-open.svg"
                onClicked: editor.openContainingFolder(root.successPath)
            }
            OmButton { visible: root.done; text: "Copy path"; onClicked: editor.copyPath(root.successPath) }
            Item { Layout.fillWidth: true }
            OmButton {
                visible: editor.exporting
                bordered: true
                text: "Cancel"
                onClicked: editor.cancelExport()
            }
            OmButton {
                visible: !editor.exporting && !root.done
                Layout.preferredWidth: 120
                primary: true
                text: "Export"
                icon.source: "qrc:/omareel/assets/icons/lucide/download.svg"
                onClicked: editor.exportTo(root.outputPath, {
                    fps: root.fpsValue, height: root.heightValue, quality: root.qualityValue
                })
            }
            OmButton {
                visible: root.done
                primary: true
                Layout.preferredWidth: 88
                text: "Done"
                onClicked: root.close()
            }
        }
    }

    Connections {
        target: editor
        function onExportFinished(path) { root.successPath = path }
    }
    FileDialog {
        id: outputDialog
        title: "Export to"
        fileMode: FileDialog.SaveFile
        defaultSuffix: root.format
        selectedFile: "file://" + root.outputPath
        nameFilters: root.format === "gif" ? ["GIF image (*.gif)"] : ["MP4 video (*.mp4)"]
        onAccepted: root.outputPath = selectedFile.toString().replace(/^file:\/\//, "")
    }
}
