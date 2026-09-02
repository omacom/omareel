import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    spacing: 12
    PanelHeading {
        Layout.fillWidth: true
        text: editor.hasCamera ? "Webcam recording detected" : "No webcam recording in this bundle"
        wrapMode: Text.WordWrap
    }
    Label {
        Layout.fillWidth: true
        visible: !editor.hasCamera
        text: "Webcam controls will be available for bundles that include camera.mp4."
        wrapMode: Text.WordWrap
        color: Qt.alpha(theme.foreground, .58)
        font.pixelSize: 11
    }
}
