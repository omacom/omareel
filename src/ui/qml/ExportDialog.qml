import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

Dialog {
    id: root
    width: 430
    modal: true
    title: "Export video"
    standardButtons: Dialog.NoButton
    property string format: "mp4"
    property string outputPath: editor.defaultExportPath(format)
    ColumnLayout {
        width: parent.width
        spacing: 12
        Label { text: "Format" }
        ComboBox { id: formatBox; Layout.fillWidth: true; model: ["mp4", "gif"]; onActivated: { root.format = currentText; root.outputPath = editor.defaultExportPath(root.format) } }
        Label { text: "Height" }
        ComboBox { id: heightBox; Layout.fillWidth: true; model: root.format === "mp4" ? [720,1080,2160] : [480,720,1080]; currentIndex: root.format === "mp4" ? 1 : 0 }
        Label { text: "Frame rate" }
        ComboBox { id: fpsBox; Layout.fillWidth: true; model: root.format === "mp4" ? [60,50,30,25,24,20,10] : [50,30,25,20,15,10]; currentIndex: root.format === "mp4" ? 0 : 4 }
        Label { text: "Quality" }
        ComboBox { id: qualityBox; Layout.fillWidth: true; model: root.format === "mp4" ? ["studio","social","web-high","web-low"] : ["studio","social"]; currentIndex: 1 }
        RowLayout {
            Layout.fillWidth: true
            TextField { Layout.fillWidth: true; readOnly: true; text: root.outputPath }
            Button { text: "Choose…"; topInset: 0; bottomInset: 0; onClicked: outputDialog.open() }
        }
        ProgressBar { Layout.fillWidth: true; visible: editor.exporting; value: editor.exportProgress }
        Label { Layout.fillWidth: true; visible: editor.exportError.length > 0; text: editor.exportError; color: "#ff7676"; wrapMode: Text.WordWrap }
        RowLayout {
            Layout.fillWidth: true
            Item { Layout.fillWidth: true }
            Button { text: editor.exporting ? "Cancel" : "Close"; topInset: 0; bottomInset: 0; onClicked: editor.exporting ? editor.cancelExport() : root.close() }
            Button {
                text: "Export"; highlighted: true; enabled: !editor.exporting
                topInset: 0; bottomInset: 0
                onClicked: editor.exportTo(root.outputPath, {fps:Number(fpsBox.currentText), height:Number(heightBox.currentText), quality:qualityBox.currentText})
            }
        }
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
