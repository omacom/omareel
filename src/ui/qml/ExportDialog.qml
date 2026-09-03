import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Dialog {
    id: root
    objectName: "exportDialog"
    width: 420
    anchors.centerIn: parent
    modal: true
    padding: 24
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

    component SegmentRow: RowLayout {
        id: segment
        required property var choices
        required property var selectedValue
        signal chosen(var value)
        spacing: 4
        Repeater {
            model: segment.choices
            delegate: EditorButton {
                required property var modelData
                Layout.fillWidth: true
                Layout.preferredHeight: 30
                checkable: true
                checked: segment.selectedValue === modelData.value
                selected: checked
                text: modelData.label
                font.pixelSize: 11
                font.weight: checked ? Font.DemiBold : Font.Normal
                onClicked: segment.chosen(modelData.value)
            }
        }
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

    background: Rectangle {
        radius: 14
        color: theme.surfaceRaised
        border.width: 1
        border.color: theme.hairlineStrong
    }
    Overlay.modal: Rectangle { color: Qt.alpha(theme.surface, .55) }
    contentItem: ColumnLayout {
        spacing: 12
        RowLayout {
            Layout.fillWidth: true
            Label { text: "Export"; font.pixelSize: 17; font.weight: Font.DemiBold; color: theme.foreground }
            Item { Layout.fillWidth: true }
            IconToolButton {
                enabled: !editor.exporting
                icon.source: "qrc:/omarecord/assets/icons/lucide/x.svg"
                Accessible.name: "Close export"
                onClicked: root.close()
            }
        }
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 154
            radius: 10
            color: theme.surface
            clip: true
            Image {
                anchors.fill: parent
                source: root.previewUrl
                fillMode: Image.PreserveAspectCrop
                smooth: true
            }
            Label {
                anchors.centerIn: parent
                visible: root.previewUrl.toString() === ""
                text: "Current frame"
                color: theme.textFaint
            }
        }

        PanelLabel { text: "Format" }
        SegmentRow {
            Layout.fillWidth: true
            choices: [{label:"MP4",value:"mp4"},{label:"GIF",value:"gif"}]
            selectedValue: root.format
            onChosen: value => root.selectFormat(value)
        }
        PanelLabel { text: "Resolution" }
        SegmentRow {
            Layout.fillWidth: true; choices: root.heights; selectedValue: root.heightValue
            onChosen: value => {
                root.heightValue = value
                editor.setProjectValue(root.format === "mp4" ? "export.height" : "export.gif.height", value)
            }
        }
        PanelLabel { text: "Frame rate" }
        SegmentRow {
            Layout.fillWidth: true; choices: root.rates; selectedValue: root.fpsValue
            onChosen: value => {
                root.fpsValue = value
                editor.setProjectValue(root.format === "mp4" ? "export.fps" : "export.gif.fps", value)
            }
        }
        PanelLabel { text: "Quality" }
        SegmentRow {
            Layout.fillWidth: true; choices: root.qualities; selectedValue: root.qualityValue
            onChosen: value => {
                root.qualityValue = value
                editor.setProjectValue(root.format === "mp4" ? "export.quality" : "export.gif.quality", value)
            }
        }
        Label {
            Layout.fillWidth: true
            text: root.qualityCaption()
            color: theme.textMuted
            font.pixelSize: 11
        }
        Label {
            Layout.fillWidth: true
            text: "Estimated size  ·  " + root.estimatedMegabytes().toFixed(root.estimatedMegabytes() < 10 ? 1 : 0) + " MB"
            color: theme.textMuted
            font.pixelSize: 11
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: 7
            IconToolButton {
                icon.source: "qrc:/omarecord/assets/icons/lucide/folder-open.svg"
                Accessible.name: "Choose output path"
                onClicked: outputDialog.open()
            }
            Label {
                Layout.fillWidth: true
                text: root.outputPath
                elide: Text.ElideMiddle
                color: theme.textMuted
                font.pixelSize: 11
            }
            EditorButton { text: "Choose…"; enabled: !editor.exporting; onClicked: outputDialog.open() }
        }

        ProgressBar {
            Layout.fillWidth: true
            visible: editor.exporting || root.successPath.length > 0
            value: editor.exportProgress
        }
        RowLayout {
            Layout.fillWidth: true
            visible: editor.exporting
            Label { text: Math.round(editor.exportProgress * 100) + "%"; font.weight: Font.DemiBold }
            Item { Layout.fillWidth: true }
            Label {
                text: editor.exportFps.toFixed(1) + " fps  ·  ETA " + root.formatEta(editor.exportEtaSeconds)
                color: theme.textMuted
                font.pixelSize: 11
            }
        }
        Label {
            Layout.fillWidth: true
            visible: editor.exportError.length > 0 && !editor.exporting
            text: editor.exportError
            color: theme.record
            wrapMode: Text.WordWrap
            font.pixelSize: 11
        }
        Label {
            Layout.fillWidth: true
            visible: root.successPath.length > 0
            text: "Saved to " + root.successPath
            elide: Text.ElideMiddle
            color: theme.accent
            font.pixelSize: 11
        }
        RowLayout {
            Layout.fillWidth: true
            visible: root.successPath.length > 0
            EditorButton { text: "Open folder"; icon.source: "qrc:/omarecord/assets/icons/lucide/folder-open.svg"; onClicked: editor.openContainingFolder(root.successPath) }
            EditorButton { text: "Copy path"; onClicked: editor.copyPath(root.successPath) }
            Item { Layout.fillWidth: true }
        }
        RowLayout {
            Layout.fillWidth: true
            visible: root.successPath.length === 0
            Item { Layout.fillWidth: true }
            EditorButton {
                visible: editor.exporting
                text: "Cancel"
                onClicked: editor.cancelExport()
            }
            EditorButton {
                visible: !editor.exporting
                Layout.preferredWidth: 112
                Layout.preferredHeight: 36
                primary: true
                text: "Export"
                icon.source: "qrc:/omarecord/assets/icons/lucide/download.svg"
                font.weight: Font.DemiBold
                onClicked: editor.exportTo(root.outputPath, {
                    fps: root.fpsValue, height: root.heightValue, quality: root.qualityValue
                })
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
