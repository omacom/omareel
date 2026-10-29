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
    // Every wallpaper, flattened. Reading editor.wallpaperGroups converts the whole list again,
    // so it is read once per change, not in the loops or in each binding that needs it.
    readonly property var wallpapers: wallpaperEntries(editor.wallpaperGroups)
    function wallpaperEntries(groups) {
        const result = []
        for (let groupIndex = 0; groupIndex < groups.length; ++groupIndex) {
            const group = groups[groupIndex]
            for (let wallpaperIndex = 0; wallpaperIndex < group.wallpapers.length; ++wallpaperIndex) {
                const wallpaper = group.wallpapers[wallpaperIndex]
                result.push({theme: group.label, file: wallpaper.file, path: wallpaper.path,
                    url: wallpaper.url, thumbnailUrl: wallpaper.thumbnailUrl})
            }
        }
        return result
    }
    function visibleWallpapers() {
        const all = wallpapers
        const count = showAllWallpapers ? all.length : Math.min(11, all.length)
        const result = all.slice(0, count)
        result.push({picker: true})
        return result
    }
    function hiddenWallpaperCount() {
        return Math.max(0, wallpapers.length - 11)
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

    // Selection draws an accent ring with a small gap around the swatch.
    component SwatchTile: Item {
        id: tile
        property bool selected: false
        property bool forceHot: false
        readonly property bool hot: tileMouse.containsMouse || forceHot
        property bool interactive: true
        default property alias content: face.data
        signal activated()
        scale: hot ? 1.04 : 1
        z: hot ? 2 : 0
        Behavior on scale { NumberAnimation { duration: 180; easing.type: Easing.OutBack; easing.overshoot: 1.6 } }
        Rectangle {
            anchors.fill: parent
            radius: theme.controlRadius + (theme.rounded ? 3 : 0)
            color: "transparent"
            border.width: 2
            border.color: tile.selected ? theme.accent : tile.hot ? theme.strokeStrong : "transparent"
            Behavior on border.color { ColorAnimation { duration: 140 } }
        }
        Rectangle {
            id: face
            anchors.fill: parent
            anchors.margins: tile.selected ? 4 : 0
            radius: theme.controlRadius
            color: theme.fill
            clip: true
            border.width: 1
            border.color: theme.stroke
            Behavior on anchors.margins { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
        }
        MouseArea {
            id: tileMouse
            anchors.fill: parent
            enabled: tile.interactive
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: tile.activated()
        }
    }

    component SectionToggle: OmButton {
        property bool expanded: false
        property int hiddenCount: 0
        Layout.fillWidth: true
        implicitHeight: 28
        text: expanded ? "Show less" : "Show " + hiddenCount + " more"
        font.pixelSize: theme.type.small
        contentColor: hovered ? theme.textStrong : theme.textMuted
        trailingIconSource: "qrc:/omareel/assets/icons/lucide/chevron-down.svg"
    }

    OmButton {
        Layout.fillWidth: true
        bordered: true
        checkable: true
        checked: root.background.type === "none"
        selected: checked
        text: checked ? "No background" : "Remove background"
        icon.source: "qrc:/omareel/assets/icons/lucide/x.svg"
        onClicked: root.choose("none", "", null)
    }

    OmSectionHeader { text: "Wallpaper"; Layout.topMargin: 4 }
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
            delegate: SwatchTile {
                id: wallpaperTile
                required property var modelData
                Layout.preferredWidth: wallpaperGrid.tileWidth
                Layout.minimumWidth: wallpaperGrid.tileWidth
                Layout.maximumWidth: wallpaperGrid.tileWidth
                Layout.preferredHeight: wallpaperGrid.tileWidth * 10 / 16
                readonly property bool picker: modelData.picker === true
                interactive: !picker
                selected: !picker && root.background.type === "wallpaper"
                    && modelData.url === root.background.resolvedImage
                onActivated: root.choose("wallpaper", "background.wallpaper", wallpaperTile.modelData.path)
                Image {
                    anchors.fill: parent
                    anchors.margins: 1
                    visible: !wallpaperTile.picker
                    source: wallpaperTile.picker ? "" : wallpaperTile.modelData.thumbnailUrl
                    asynchronous: true
                    sourceSize: Qt.size(400, 250)
                    fillMode: Image.PreserveAspectCrop
                    opacity: status === Image.Ready ? 1 : 0
                    Behavior on opacity { NumberAnimation { duration: 220 } }
                }
                OmIconButton {
                    anchors.fill: parent
                    visible: wallpaperTile.picker
                    icon.source: "qrc:/omareel/assets/icons/lucide/plus.svg"
                    tooltipText: "Choose an image…"
                    Accessible.name: "Pick file"
                    onClicked: imageDialog.open()
                }
                OmToolTip {
                    target: wallpaperTile
                    visible: wallpaperTile.hot && !wallpaperTile.picker
                    text: wallpaperTile.picker ? "" : wallpaperTile.modelData.theme + " · " + wallpaperTile.modelData.file
                }
            }
        }
    }
    SectionToggle {
        visible: root.hiddenWallpaperCount() > 0
        expanded: root.showAllWallpapers
        hiddenCount: root.hiddenWallpaperCount()
        onClicked: root.showAllWallpapers = !root.showAllWallpapers
    }

    OmSectionHeader { text: "Gradient"; Layout.topMargin: 4 }
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
            delegate: SwatchTile {
                id: gradientTile
                required property int index
                readonly property int presetIndex: index
                objectName: "gradientTile-" + presetIndex
                readonly property var colours: editor.gradients[presetIndex]
                selected: root.background.type === "gradient" && root.gradientMatches(colours)
                forceHot: root.proofHoveredIndex === presetIndex
                Layout.preferredWidth: gradientGrid.cellWidth
                Layout.minimumWidth: gradientGrid.cellWidth
                Layout.maximumWidth: gradientGrid.cellWidth
                Layout.preferredHeight: gradientGrid.cellWidth
                onActivated: editor.applyGradientPreset(gradientTile.presetIndex)
                Rectangle {
                    anchors.fill: parent
                    gradient: Gradient {
                        orientation: Gradient.Vertical
                        GradientStop { position: 0; color: gradientTile.colours[0] }
                        GradientStop { position: 1; color: gradientTile.colours[gradientTile.colours.length - 1] }
                    }
                }
            }
        }
    }
    SectionToggle {
        visible: editor.gradients.length > 10
        expanded: root.showAllGradients
        hiddenCount: editor.gradients.length - 10
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
        suffix: "°"
    }

    OmSectionHeader { text: "Color"; Layout.topMargin: 4 }
    RowLayout {
        Layout.fillWidth: true
        spacing: 6
        Repeater {
            model: root.plainColours.length + 1
            delegate: SwatchTile {
                id: colourTile
                required property int index
                Layout.fillWidth: true
                Layout.preferredHeight: width
                Layout.maximumHeight: 34
                readonly property bool custom: index === root.plainColours.length
                readonly property color swatch: custom ? root.background.color : root.plainColours[index]
                selected: !custom && root.background.type === "color"
                    && String(root.background.color).toLowerCase() === String(swatch).toLowerCase()
                interactive: !custom
                onActivated: root.choose("color", "background.color", colourTile.swatch)
                Rectangle {
                    anchors.fill: parent
                    visible: !colourTile.custom
                    color: colourTile.swatch
                }
                OmIconButton {
                    anchors.fill: parent
                    visible: colourTile.custom
                    icon.source: "qrc:/omareel/assets/icons/lucide/palette.svg"
                    tooltipText: "Custom color…"
                    Accessible.name: "Custom color"
                    onClicked: colourDialog.open()
                }
            }
        }
    }

    OmSeparator { Layout.fillWidth: true; Layout.topMargin: 4 }
    PanelSlider {
        Layout.fillWidth: true
        enabled: root.background.type !== "none"
        opacity: enabled ? 1 : .42
        label: "Background blur"
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
