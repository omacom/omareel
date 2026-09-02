import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    RowLayout {
        Layout.fillWidth: true
        spacing: 2
        Repeater {
            model: [{t:"wallpaper",l:"Wallpaper"},{t:"gradient",l:"Gradient"},{t:"color",l:"Color"},{t:"image",l:"Image"}]
            delegate: Rectangle {
                required property var modelData
                Layout.fillWidth: true
                Layout.preferredHeight: 30
                radius: 6
                color: editor.project.background.type === modelData.t
                    ? Qt.rgba(theme.accent.r, theme.accent.g, theme.accent.b, 0.28) : "#23252c"
                border.color: editor.project.background.type === modelData.t ? theme.accent : "#343741"
                Label {
                    anchors.fill: parent
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    text: modelData.l
                    font.pixelSize: 10
                    font.bold: editor.project.background.type === modelData.t
                    color: editor.project.background.type === modelData.t ? "white" : "#c1c3ca"
                }
                MouseArea { anchors.fill: parent; onClicked: editor.setProjectValue("background.type", modelData.t) }
            }
        }
    }
    GridView {
        Layout.fillWidth: true
        Layout.preferredHeight: 230
        visible: editor.project.background.type === "wallpaper"
        model: editor.wallpapers
        cellWidth: width / 2
        cellHeight: 82
        clip: true
        delegate: Item {
            required property var modelData
            width: GridView.view.cellWidth; height: GridView.view.cellHeight
            Rectangle {
                anchors.fill: parent; anchors.margins: 4; radius: 7
                border.width: editor.project.background.wallpaper === modelData.path ? 2 : 1
                border.color: editor.project.background.wallpaper === modelData.path ? theme.accent : "#3a3c45"
                Image { anchors.fill: parent; anchors.margins: 2; source: modelData.url; fillMode: Image.PreserveAspectCrop; clip: true }
                MouseArea { anchors.fill: parent; onClicked: editor.setProjectValue("background.wallpaper", modelData.path) }
            }
        }
    }
    GridView {
        Layout.fillWidth: true
        Layout.preferredHeight: 260
        visible: editor.project.background.type === "gradient"
        model: editor.gradients
        cellWidth: width / 3
        cellHeight: 66
        clip: true
        delegate: Rectangle {
            required property var modelData
            width: GridView.view.cellWidth - 8; height: GridView.view.cellHeight - 8; radius: 7
            gradient: Gradient {
                GradientStop { position: 0; color: modelData[0] }
                GradientStop { position: 1; color: modelData[modelData.length - 1] }
            }
            MouseArea {
                anchors.fill: parent
                onClicked: {
                    let stops=[]
                    for (let i=0;i<modelData.length;i++) stops.push([modelData[i], i / Math.max(1, modelData.length-1)])
                    editor.setProjectValue("background.gradient.stops", stops)
                }
            }
        }
    }
    PanelSlider { visible: editor.project.background.type === "gradient"; Layout.fillWidth: true; label: "Angle"; path: "background.gradient.angle"; from: 0; to: 360; value: editor.project.background.gradient.angle; stepSize: 1 }
    Button {
        visible: editor.project.background.type === "wallpaper" || editor.project.background.type === "image"
        Layout.fillWidth: true
        text: "Pick file…"
        onClicked: imageDialog.open()
    }
    RowLayout {
        visible: editor.project.background.type === "color"
        Layout.fillWidth: true
        Label { text: "Background color"; Layout.fillWidth: true }
        Rectangle { width: 44; height: 28; radius: 5; color: editor.project.background.color; border.color: "#777"; MouseArea { anchors.fill: parent; onClicked: colorDialog.open() } }
    }
    PanelSlider { Layout.fillWidth: true; label: "Blur"; path: "background.blur"; from: 0; to: 100; value: editor.project.background.blur; stepSize: 1 }
    FileDialog {
        id: imageDialog
        title: "Choose a background image"
        nameFilters: ["Images (*.png *.jpg *.jpeg *.webp)"]
        onAccepted: editor.setProjectValue(editor.project.background.type === "wallpaper" ? "background.wallpaper" : "background.image", selectedFile)
    }
    ColorDialog { id: colorDialog; selectedColor: editor.project.background.color; onAccepted: editor.setProjectValue("background.color", selectedColor) }
}
