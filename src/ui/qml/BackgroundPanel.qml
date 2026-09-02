import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    spacing: 12
    readonly property var background: editor.project.background
    function gradientMatches(colors) {
        const stops = background.gradient.stops
        if (!stops || stops.length !== colors.length) return false
        for (let i = 0; i < colors.length; ++i) {
            if (String(stops[i][0]).toLowerCase() !== String(colors[i]).toLowerCase()) return false
            const expected = i / Math.max(1, colors.length - 1)
            if (Math.abs(Number(stops[i][1]) - expected) > .0001) return false
        }
        return true
    }
    RowLayout {
        Layout.fillWidth: true
        spacing: 4
        Repeater {
            model: [{t:"wallpaper",l:"Wallpaper"},{t:"gradient",l:"Gradient"},{t:"color",l:"Color"},{t:"image",l:"Image"},{t:"none",l:"None"}]
            delegate: Rectangle {
                id: tab
                required property var modelData
                implicitWidth: tabLabel.implicitWidth + 10
                Layout.preferredWidth: implicitWidth
                Layout.preferredHeight: 30
                radius: 6
                color: root.background.type === modelData.t
                    ? Qt.alpha(theme.accent, .28) : theme.darkBackground
                border.color: root.background.type === modelData.t ? theme.accent : Qt.alpha(theme.foreground, .13)
                Label {
                    id: tabLabel
                    anchors.fill: parent
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    text: modelData.l
                    font.pixelSize: 10
                    font.bold: root.background.type === modelData.t
                    color: root.background.type === modelData.t ? theme.accentForeground : Qt.alpha(theme.foreground, .76)
                }
                MouseArea { anchors.fill: parent; onClicked: editor.setProjectValue("background.type", modelData.t) }
            }
        }
    }
    GridView {
        Layout.fillWidth: true
        Layout.preferredHeight: 230
        visible: root.background.type === "wallpaper"
        model: editor.wallpapers
        cellWidth: width / 2
        cellHeight: 82
        clip: true
        delegate: Item {
            required property var modelData
            width: GridView.view.cellWidth; height: GridView.view.cellHeight
            Rectangle {
                anchors.fill: parent; anchors.margins: 4; radius: 6; clip: true
                property bool selected: modelData.url === root.background.resolvedImage
                border.width: selected ? 2 : 1
                border.color: selected ? theme.accent : Qt.alpha(theme.foreground, .16)
                Image { anchors.fill: parent; anchors.margins: 2; source: modelData.thumbnailUrl; asynchronous: true; sourceSize.width: 400; sourceSize.height: 240; fillMode: Image.PreserveAspectCrop }
                MouseArea { anchors.fill: parent; onClicked: editor.setProjectValue("background.wallpaper", modelData.path) }
            }
        }
    }
    GridView {
        Layout.fillWidth: true
        Layout.preferredHeight: 260
        visible: root.background.type === "gradient"
        model: editor.gradients
        cellWidth: width / 3
        cellHeight: 66
        clip: true
        delegate: Rectangle {
            required property var modelData
            property bool selected: root.gradientMatches(modelData)
            width: GridView.view.cellWidth - 8; height: GridView.view.cellHeight - 8; radius: 6
            border.width: selected ? 2 : 1
            border.color: selected ? theme.accent : Qt.alpha(theme.foreground, .13)
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
    PanelSlider { visible: root.background.type === "gradient"; Layout.fillWidth: true; label: "Angle"; path: "background.gradient.angle"; from: 0; to: 360; value: root.background.gradient.angle; stepSize: 1 }
    Button {
        visible: root.background.type === "wallpaper" || root.background.type === "image"
        Layout.fillWidth: true
        text: "Pick file…"
        onClicked: imageDialog.open()
    }
    RowLayout {
        visible: root.background.type === "color"
        Layout.fillWidth: true
        PanelLabel { text: "Background color"; Layout.fillWidth: true }
        Rectangle { width: 44; height: 28; radius: 5; color: root.background.color; border.color: Qt.alpha(theme.foreground, .4); MouseArea { anchors.fill: parent; onClicked: colorDialog.open() } }
    }
    PanelSlider { visible: root.background.type !== "none"; Layout.fillWidth: true; label: "Blur"; path: "background.blur"; from: 0; to: 100; value: root.background.blur; stepSize: 1 }
    Label {
        visible: root.background.type === "none"
        Layout.fillWidth: true
        text: "The video fills the output with no backdrop, padding, shadow, or rounded mask."
        wrapMode: Text.WordWrap
        color: Qt.alpha(theme.foreground, .62)
        font.pixelSize: 11
    }
    FileDialog {
        id: imageDialog
        title: "Choose a background image"
        nameFilters: ["Images (*.png *.jpg *.jpeg *.webp)"]
        onAccepted: editor.setProjectValue(root.background.type === "wallpaper" ? "background.wallpaper" : "background.image", selectedFile)
    }
    ColorDialog { id: colorDialog; selectedColor: root.background.color; onAccepted: editor.setProjectValue("background.color", selectedColor) }
}
