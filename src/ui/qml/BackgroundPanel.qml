import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import Omareel.Ui

ColumnLayout {
    id: root
    objectName: "backgroundPanel"
    spacing: theme.space.panelGap
    property bool showAllWallpapers: false
    property bool showAllGradients: false
    property int proofHoveredIndex: -1
    readonly property var background: editor.project.background
    readonly property var plainColours: [
        Qt.rgba(0, 0, 0, 1), Qt.rgba(1, 1, 1, 1),
        Qt.rgba(55/255, 65/255, 81/255, 1), Qt.rgba(156/255, 163/255, 175/255, 1),
        Qt.rgba(127/255, 29/255, 29/255, 1), Qt.rgba(4/255, 120/255, 87/255, 1),
        Qt.rgba(29/255, 78/255, 216/255, 1)
    ]

    function choose(type, path, value) {
        editor.beginCoalescedEdit("background-choice")
        editor.setProjectValue("background.type", type, true)
        if (path) editor.setProjectValue(path, value, true)
        editor.endCoalescedEdit()
    }
    function wallpaperEntries() {
        const result = []
        for (let groupIndex = 0; groupIndex < editor.wallpaperGroups.length; ++groupIndex) {
            const group = editor.wallpaperGroups[groupIndex]
            for (let wallpaperIndex = 0; wallpaperIndex < group.wallpapers.length; ++wallpaperIndex) {
                const wallpaper = group.wallpapers[wallpaperIndex]
                result.push({theme: group.label, file: wallpaper.file, path: wallpaper.path,
                    url: wallpaper.url, thumbnailUrl: wallpaper.thumbnailUrl})
            }
        }
        return result
    }
    function visibleWallpapers() {
        const all = wallpaperEntries()
        const count = showAllWallpapers ? all.length : Math.min(11, all.length)
        const result = all.slice(0, count)
        result.push({picker: true})
        return result
    }
    function hiddenWallpaperCount() {
        return Math.max(0, wallpaperEntries().length - 11)
    }
    function gradientMatches(colours) {
        const stops = background.gradient.stops
        if (!stops || stops.length !== colours.length) return false
        for (let i = 0; i < colours.length; ++i) {
            if (String(stops[i][0]).toLowerCase() !== String(colours[i]).toLowerCase()) return false
            if (Math.abs(Number(stops[i][1]) - i / Math.max(1, colours.length - 1)) > .0001) return false
        }
        return true
    }

    OmButton {
        Layout.fillWidth: true
        bordered: true
        checkable: true
        checked: root.background.type === "none"
        selected: checked
        text: "None"
        icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
        onClicked: root.choose("none", "", null)
    }

    OmSectionHeader { text: "Wallpapers" }
    GridLayout {
        id: wallpaperGrid
        Layout.fillWidth: true
        columns: 4
        columnSpacing: 8
        rowSpacing: 8
        readonly property real tileWidth: Math.max(1,
            (root.width - columnSpacing * (columns - 1)) / columns)
        Repeater {
            model: root.visibleWallpapers()
            delegate: Rectangle {
                id: wallpaperTile
                required property var modelData
                Layout.preferredWidth: wallpaperGrid.tileWidth
                Layout.minimumWidth: wallpaperGrid.tileWidth
                Layout.maximumWidth: wallpaperGrid.tileWidth
                Layout.preferredHeight: wallpaperGrid.tileWidth * 9 / 16
                radius: theme.radius
                color: selected ? theme.selectedFill : theme.normalFill
                border.width: 1
                border.color: selected ? theme.selectedBorder : theme.normalBorder
                readonly property bool picker: modelData.picker === true
                readonly property bool selected: !picker && root.background.type === "wallpaper"
                    && modelData.url === root.background.resolvedImage
                clip: true
                scale: wallpaperMouse.containsMouse ? 1.04 : 1
                z: wallpaperMouse.containsMouse ? 2 : 0
                Behavior on scale { NumberAnimation { duration: 100; easing.type: Easing.OutCubic } }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: wallpaperTile.selected ? 3 : 1
                    radius: theme.radius
                    color: theme.surface
                    clip: true
                    Image {
                        anchors.fill: parent
                        visible: !wallpaperTile.picker
                        source: wallpaperTile.picker ? "" : wallpaperTile.modelData.thumbnailUrl
                        asynchronous: true
                        sourceSize: Qt.size(400, 225)
                        fillMode: Image.PreserveAspectCrop
                    }
                }
                OmIconButton {
                    anchors.centerIn: parent
                    visible: wallpaperTile.picker
                    icon.source: "qrc:/omareel/assets/icons/lucide/plus.svg"
                    icon.color: theme.foreground
                    tooltipText: "Pick file…"
                    Accessible.name: "Pick file"
                    onClicked: imageDialog.open()
                }
                MouseArea {
                    id: wallpaperMouse
                    anchors.fill: parent
                    visible: !wallpaperTile.picker
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.choose("wallpaper", "background.wallpaper", wallpaperTile.modelData.path)
                }
                ToolTip {
                    visible: wallpaperMouse.containsMouse
                    delay: 400
                    text: wallpaperTile.picker ? "" : wallpaperTile.modelData.theme + " · " + wallpaperTile.modelData.file
                }
            }
        }
    }
    OmButton {
        Layout.fillWidth: true
        visible: root.hiddenWallpaperCount() > 0
        bordered: true
        text: root.showAllWallpapers ? "Show less"
            : "Show more (" + root.hiddenWallpaperCount() + ")"
        trailingIconSource: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
        onClicked: root.showAllWallpapers = !root.showAllWallpapers
    }

    OmSeparator { Layout.fillWidth: true }
    OmSectionHeader { text: "Gradients" }
    GridLayout {
        id: gradientGrid
        Layout.fillWidth: true
        columns: 5
        columnSpacing: 8
        rowSpacing: 8
        readonly property real cellWidth: Math.max(1,
            (root.width - columnSpacing * (columns - 1)) / columns)
        Repeater {
            model: root.showAllGradients ? editor.gradients.length
                                         : Math.min(10, editor.gradients.length)
            delegate: Rectangle {
                id: gradientTile
                required property int index
                readonly property int presetIndex: index
                objectName: "gradientTile-" + presetIndex
                readonly property var colours: editor.gradients[presetIndex]
                readonly property bool selected: root.background.type === "gradient"
                    && root.gradientMatches(colours)
                Layout.preferredWidth: gradientGrid.cellWidth
                Layout.minimumWidth: gradientGrid.cellWidth
                Layout.maximumWidth: gradientGrid.cellWidth
                Layout.preferredHeight: gradientGrid.cellWidth
                radius: theme.radius
                color: selected ? theme.selectedFill : "transparent"
                border.width: 1
                border.color: selected ? theme.selectedBorder : theme.normalBorder
                scale: gradientMouse.containsMouse || root.proofHoveredIndex === presetIndex ? 1.04 : 1
                z: gradientMouse.containsMouse || root.proofHoveredIndex === presetIndex ? 2 : 0
                Behavior on scale { NumberAnimation { duration: 100; easing.type: Easing.OutCubic } }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: gradientTile.selected ? 3 : 1
                    radius: theme.radius
                    gradient: Gradient {
                        GradientStop { position: 0; color: gradientTile.colours[0] }
                        GradientStop { position: 1; color: gradientTile.colours[gradientTile.colours.length - 1] }
                    }
                }
                MouseArea {
                    id: gradientMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: editor.applyGradientPreset(gradientTile.presetIndex)
                }
            }
        }
    }
    OmButton {
        Layout.fillWidth: true
        visible: editor.gradients.length > 10
        bordered: true
        text: root.showAllGradients ? "Show less"
            : "Show more (" + (editor.gradients.length - 10) + ")"
        trailingIconSource: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
        onClicked: root.showAllGradients = !root.showAllGradients
    }
    PanelSlider {
        visible: root.background.type === "gradient"
        Layout.fillWidth: true
        label: "Angle"
        path: "background.gradient.angle"
        from: 0
        to: 360
        value: root.background.gradient.angle
        stepSize: 1
    }

    OmSeparator { Layout.fillWidth: true }
    OmSectionHeader { text: "Colors" }
    RowLayout {
        Layout.fillWidth: true
        spacing: 8
        Repeater {
            model: root.plainColours.length + 1
            delegate: Rectangle {
                id: colourTile
                required property int index
                Layout.fillWidth: true
                Layout.preferredHeight: width
                Layout.maximumHeight: 32
                readonly property bool custom: index === root.plainColours.length
                readonly property color swatch: custom ? root.background.color : root.plainColours[index]
                readonly property bool selected: !custom && root.background.type === "color"
                    && String(root.background.color).toLowerCase() === String(swatch).toLowerCase()
                radius: theme.radius
                color: selected ? theme.selectedFill : "transparent"
                border.width: 1
                border.color: selected ? theme.selectedBorder : theme.normalBorder
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: colourTile.selected ? 3 : 1
                    radius: theme.radius
                    color: colourTile.swatch
                }
                OmIconButton {
                    anchors.centerIn: parent
                    visible: colourTile.custom
                    icon.source: "qrc:/omareel/assets/icons/lucide/palette.svg"
                    icon.color: theme.foreground
                    tooltipText: "Custom color"
                    Accessible.name: "Custom color"
                    onClicked: colourDialog.open()
                }
                MouseArea {
                    anchors.fill: parent
                    visible: !colourTile.custom
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.choose("color", "background.color", colourTile.swatch)
                }
            }
        }
    }

    OmSeparator { Layout.fillWidth: true }
    OmSectionHeader { text: "Blur" }
    PanelSlider {
        Layout.fillWidth: true
        enabled: root.background.type !== "none"
        opacity: enabled ? 1 : .42
        label: "Amount"
        path: "background.blur"
        from: 0
        to: 100
        value: root.background.blur
        stepSize: 1
    }

    FileDialog {
        id: imageDialog
        title: "Choose a background image"
        nameFilters: ["Images (*.png *.jpg *.jpeg *.webp)"]
        onAccepted: root.choose("image", "background.image", selectedFile)
    }
    ColorDialog {
        id: colourDialog
        selectedColor: root.background.color
        onAccepted: root.choose("color", "background.color", selectedColor)
    }
}
