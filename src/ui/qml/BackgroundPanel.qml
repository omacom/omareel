import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts

ColumnLayout {
    id: root
    objectName: "backgroundPanel"
    clip: true
    spacing: 13
    property bool showAllGradients: false
    readonly property var background: editor.project.background
    readonly property var plainColours: [
        "#000000", "#ffffff", "#111827", "#374151", "#6b7280", "#9ca3af",
        "#d1d5db", "#f3f4f6", "#7f1d1d", "#c2410c", "#ca8a04", "#3f6212",
        "#047857", "#0e7490", "#1d4ed8", "#4338ca", "#7e22ce", "#be185d"
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

    Button {
        Layout.fillWidth: true
        Layout.preferredHeight: 48
        checkable: true
        checked: root.background.type === "none"
        text: "None"
        icon.source: "qrc:/omarecord/assets/icons/lucide/x.svg"
        icon.color: checked ? theme.accent : theme.foreground
        icon.width: 18; icon.height: 18
        font.weight: Font.DemiBold
        onClicked: root.choose("none", "", null)
        background: Rectangle {
            radius: 9
            color: parent.checked ? Qt.alpha(theme.accent, .16)
                                  : parent.hovered ? Qt.alpha(theme.foreground, .06) : theme.darkBackground
            border.width: parent.checked ? 2 : 1
            border.color: parent.checked ? theme.accent : Qt.alpha(theme.foreground, .13)
        }
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
                    radius: 11; color: selected ? Qt.alpha(theme.accent, .12) : "transparent"
                    border.width: selected ? 2 : 0; border.color: theme.accent
                }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 3
                    radius: 9
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
    Button {
        Layout.fillWidth: true
        Layout.preferredHeight: 28
        visible: editor.gradients.length > 10
        flat: true
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
                color: modelData.current ? theme.accent : Qt.alpha(theme.foreground, .60)
                font.pixelSize: 10
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
                        color: selected ? Qt.alpha(theme.accent, .13) : "transparent"
                        border.width: selected ? 2 : 0
                        border.color: theme.accent
                    }
                    Rectangle {
                        anchors.centerIn: parent
                        width: 76; height: 48; radius: 8
                        color: theme.darkBackground
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
                            background: Rectangle { color: parent.hovered ? Qt.alpha(theme.accent, .12) : "transparent" }
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

    PanelHeading { text: "Plain color" }
    ListView {
        Layout.fillWidth: true
        Layout.preferredHeight: 38
        orientation: ListView.Horizontal
        spacing: 7
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        model: root.plainColours.length + 1
        delegate: Item {
            required property int index
            width: 36; height: 36
            readonly property bool custom: index === root.plainColours.length
            readonly property color swatch: custom ? root.background.color : root.plainColours[index]
            readonly property bool selected: !custom && root.background.type === "color"
                && String(root.background.color).toLowerCase() === String(swatch).toLowerCase()
            Rectangle {
                anchors.centerIn: parent; width: 36; height: 36; radius: 10
                color: selected ? Qt.alpha(theme.accent, .14) : "transparent"
                border.width: selected ? 2 : 0; border.color: theme.accent
            }
            Rectangle {
                anchors.centerIn: parent; width: 30; height: 30; radius: 8
                color: swatch
                border.width: 1; border.color: Qt.alpha(theme.foreground, .22)
                ToolButton {
                    anchors.fill: parent
                    visible: custom
                    icon.source: "qrc:/omarecord/assets/icons/lucide/palette.svg"
                    icon.color: theme.foreground
                    icon.width: 16; icon.height: 16
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
