import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    objectName: "backgroundPanel"
    clip: true
    spacing: 16
    property bool showAllGradients: false
    readonly property var background: editor.project.background
    readonly property var plainColours: [
        Qt.rgba(0, 0, 0, 1), Qt.rgba(1, 1, 1, 1),
        Qt.rgba(17/255, 24/255, 39/255, 1), Qt.rgba(55/255, 65/255, 81/255, 1),
        Qt.rgba(107/255, 114/255, 128/255, 1), Qt.rgba(156/255, 163/255, 175/255, 1),
        Qt.rgba(209/255, 213/255, 219/255, 1), Qt.rgba(243/255, 244/255, 246/255, 1),
        Qt.rgba(127/255, 29/255, 29/255, 1), Qt.rgba(194/255, 65/255, 12/255, 1),
        Qt.rgba(202/255, 138/255, 4/255, 1), Qt.rgba(63/255, 98/255, 18/255, 1),
        Qt.rgba(4/255, 120/255, 87/255, 1), Qt.rgba(14/255, 116/255, 144/255, 1),
        Qt.rgba(29/255, 78/255, 216/255, 1), Qt.rgba(67/255, 56/255, 202/255, 1),
        Qt.rgba(126/255, 34/255, 206/255, 1), Qt.rgba(190/255, 24/255, 93/255, 1)
    ]

    function choose(type, path, value) {
        editor.beginCoalescedEdit("background-choice")
        editor.setProjectValue("background.type", type, true)
        if (path) editor.setProjectValue(path, value, true)
        editor.endCoalescedEdit()
    }
    function visibleGradients() {
        const result = []
        const count = showAllGradients ? editor.gradients.length : Math.min(10, editor.gradients.length)
        for (let i = 0; i < count; ++i) result.push(editor.gradients[i])
        return result
    }
    function gradientMatches(colors) {
        const stops = background.gradient.stops
        if (!stops || stops.length !== colors.length) return false
        for (let i = 0; i < colors.length; ++i) {
            if (String(stops[i][0]).toLowerCase() !== String(colors[i]).toLowerCase()) return false
            if (Math.abs(Number(stops[i][1]) - i / Math.max(1, colors.length - 1)) > .0001) return false
        }
        return true
    }

    EditorButton {
        Layout.fillWidth: true
        checkable: true
        checked: root.background.type === "none"
        selected: checked
        text: "None"
        icon.source: "qrc:/omarecord/assets/icons/lucide/x.svg"
        icon.color: checked ? theme.accent : theme.foreground
        icon.width: 18; icon.height: 18
        onClicked: root.choose("none", "", null)
    }

    PanelHeading { text: "Gradients" }
    GridLayout {
        id: gradientGrid
        Layout.fillWidth: true
        columns: 5
        columnSpacing: 9
        rowSpacing: 9
        readonly property real cellWidth: Math.max(1,
            (root.width - columnSpacing * (columns - 1)) / columns)
        Repeater {
            model: root.visibleGradients()
            delegate: Item {
                required property var modelData
                Layout.preferredWidth: gradientGrid.cellWidth
                Layout.minimumWidth: gradientGrid.cellWidth
                Layout.maximumWidth: gradientGrid.cellWidth
                Layout.preferredHeight: 44
                readonly property bool selected: root.background.type === "gradient"
                    && root.gradientMatches(modelData)
                Rectangle {
                    anchors.fill: parent
                    radius: 10; color: selected ? theme.accentSoft : "transparent"
                    border.width: selected ? 2 : 0; border.color: theme.accent
                }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 3
                    radius: 8
                    gradient: Gradient {
                        GradientStop { position: 0; color: modelData[0] }
                        GradientStop { position: 1; color: modelData[modelData.length - 1] }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            const stops = []
                            for (let i = 0; i < modelData.length; ++i)
                                stops.push([modelData[i], i / Math.max(1, modelData.length - 1)])
                            root.choose("gradient", "background.gradient.stops", stops)
                        }
                    }
                }
            }
        }
    }
    EditorButton {
        Layout.fillWidth: true
        visible: editor.gradients.length > 10
        text: root.showAllGradients ? "Show less" : "Show more"
        icon.source: "qrc:/omarecord/assets/icons/lucide/chevron-down.svg"
        icon.color: theme.foreground
        icon.width: 15; icon.height: 15
        onClicked: root.showAllGradients = !root.showAllGradients
    }
    PanelSlider {
        visible: root.background.type === "gradient"
        Layout.fillWidth: true; label: "Angle"; path: "background.gradient.angle"
        from: 0; to: 360; value: root.background.gradient.angle; stepSize: 1
    }

    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; Layout.topMargin: 3; Layout.bottomMargin: 3; color: theme.hairline }
    PanelHeading { text: "Wallpapers" }
    Repeater {
        model: editor.wallpaperGroups
        delegate: ColumnLayout {
            id: themeRow
            required property var modelData
            required property int index
            Layout.fillWidth: true
            Layout.minimumWidth: 0
            Layout.maximumWidth: root.width
            spacing: 5
            Label {
                text: modelData.label + (modelData.current ? "  ·  Current" : "")
                color: modelData.current ? theme.accent : theme.textMuted
                font.pixelSize: 11
                font.weight: modelData.current ? Font.DemiBold : Font.Normal
            }
            ListView {
                id: wallpaperList
                Layout.fillWidth: true
                Layout.minimumWidth: 0
                Layout.maximumWidth: themeRow.width
                Layout.preferredHeight: 68
                orientation: ListView.Horizontal
                spacing: 8
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.horizontal: ScrollBar {
                    policy: wallpaperList.contentWidth > wallpaperList.width
                        ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
                    active: true
                    interactive: true
                    height: 8
                }
                model: themeRow.modelData.wallpapers.length + (themeRow.index === 0 ? 1 : 0)
                delegate: Item {
                    required property int index
                    width: 84; height: 56
                    readonly property bool picker: themeRow.index === 0
                        && index === themeRow.modelData.wallpapers.length
                    readonly property var wallpaper: picker ? null : themeRow.modelData.wallpapers[index]
                    readonly property bool selected: !picker && root.background.type === "wallpaper"
                        && wallpaper.url === root.background.resolvedImage
                    Rectangle {
                        anchors.centerIn: parent
                        width: 82; height: 54; radius: 10
                        color: selected ? theme.accentSoft : "transparent"
                        border.width: selected ? 2 : 0
                        border.color: theme.accent
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: 76; height: 48; radius: 8
                        color: theme.surface
                        clip: true
                        Image {
                            anchors.fill: parent
                            visible: !picker
                            source: picker ? "" : wallpaper.thumbnailUrl
                            asynchronous: true
                            sourceSize.width: 400; sourceSize.height: 240
                            fillMode: Image.PreserveAspectCrop
                        }
                        ToolButton {
                            anchors.fill: parent
                            visible: picker
                            icon.source: "qrc:/omarecord/assets/icons/lucide/plus.svg"
                            icon.color: theme.accent
                            icon.width: 20; icon.height: 20
                            Accessible.name: "Pick file"
                            ToolTip.visible: hovered; ToolTip.text: "Pick file…"
                            background: Rectangle { color: parent.hovered ? theme.accentSoft : "transparent" }
                            onClicked: imageDialog.open()
                        }
                        MouseArea {
                            anchors.fill: parent
                            visible: !picker
                            onClicked: root.choose("wallpaper", "background.wallpaper", wallpaper.path)
                        }
                    }
                }
            }
        }
    }

    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; Layout.topMargin: 3; Layout.bottomMargin: 3; color: theme.hairline }
    PanelHeading { text: "Plain color" }
    GridLayout {
        id: colourGrid
        Layout.fillWidth: true
        columns: 5
        columnSpacing: 9
        rowSpacing: 9
        readonly property real cellWidth: Math.max(1,
            (root.width - columnSpacing * (columns - 1)) / columns)
        Repeater {
            model: root.plainColours.length + 1
            delegate: Item {
                required property int index
                Layout.preferredWidth: colourGrid.cellWidth
                Layout.minimumWidth: colourGrid.cellWidth
                Layout.maximumWidth: colourGrid.cellWidth
                Layout.preferredHeight: 40
                readonly property bool custom: index === root.plainColours.length
                readonly property color swatch: custom ? root.background.color : root.plainColours[index]
                readonly property bool selected: !custom && root.background.type === "color"
                    && String(root.background.color).toLowerCase() === String(swatch).toLowerCase()
                Rectangle {
                    anchors.centerIn: parent
                    width: 38
                    height: 38
                    radius: 10
                    color: selected ? theme.accentSoft : "transparent"
                    border.width: selected ? 2 : 0
                    border.color: theme.accent
                }
                Rectangle {
                    anchors.centerIn: parent
                    width: 32
                    height: 32
                    radius: 8
                    color: swatch
                    border.width: 1
                    border.color: theme.hairlineStrong
                    ToolButton {
                        anchors.fill: parent
                        visible: custom
                        icon.source: "qrc:/omarecord/assets/icons/lucide/palette.svg"
                        icon.color: theme.foreground
                        icon.width: 16
                        icon.height: 16
                        Accessible.name: "Custom colour"
                        background: null
                        onClicked: colourDialog.open()
                    }
                    MouseArea {
                        anchors.fill: parent
                        visible: !custom
                        onClicked: root.choose("color", "background.color", swatch)
                    }
                }
            }
        }
    }
    PanelSlider {
        Layout.fillWidth: true
        enabled: root.background.type !== "none"
        opacity: enabled ? 1 : .42
        label: "Blur"; path: "background.blur"; from: 0; to: 100
        value: root.background.blur; stepSize: 1
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
