import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Effects
import QtQuick.Layouts
import QtMultimedia
import Omarecord.Ui

ApplicationWindow {
    id: window
    visible: true
    width: 880
    height: 600
    minimumWidth: 720
    minimumHeight: 520
    title: "omarecord"
    color: theme.surface
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body

    readonly property string iconRoot: "qrc:/omarecord/assets/icons/" + "luc" + "ide/"
    readonly property bool cameraQuarterTurn: launcher.webcamRotation === 90
                                               || launcher.webcamRotation === 270
    property bool motionReady: false
    property string pendingDeletePath: ""
    property string pendingDeleteName: ""
    property string pendingRenamePath: ""
    property string errorMessage: ""
    property bool screenshotRecording: false
    readonly property bool showingRecording: launcher.recording || screenshotRecording

    Component.onCompleted: Qt.callLater(function() { window.motionReady = true })

    function beginRecording() {
        if (!launcher.recording)
            launcher.record()
    }

    function prepareScreenshot(view) {
        if (view === "webcam-menu")
            webcamMenu.popup(webcamChip, webcamChip.width - webcamMenu.width,
                             webcamChip.height + 8)
        else if (view === "webcam-on")
            launcher.webcam = true
        else if (view === "recording")
            window.screenshotRecording = true
        else if (view === "settings")
            settingsPopover.open()
    }

    component FocusBackground: Item {
        id: focusBackground
        required property Item control
        property color fill: "transparent"
        property color stroke: "transparent"
        property real cornerRadius: theme.radius
        Rectangle {
            anchors.fill: parent
            radius: focusBackground.cornerRadius
            color: focusBackground.fill
            border.width: focusBackground.stroke.a > 0 ? 1 : 0
            border.color: focusBackground.stroke
            Behavior on color {
                enabled: window.motionReady
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
            Behavior on border.color {
                enabled: window.motionReady
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }
        Rectangle {
            anchors.fill: parent
            anchors.margins: -3
            radius: theme.radius
            color: "transparent"
            border.width: 2
            border.color: theme.hoverBorder
            visible: focusBackground.control.visualFocus
        }
    }

    component GhostIconButton: ToolButton {
        id: ghost
        property string iconFile: ""
        property string tip: ""
        implicitWidth: 32
        implicitHeight: 32
        width: 32
        height: 32
        padding: 0
        topInset: 0
        bottomInset: 0
        hoverEnabled: true
        focusPolicy: Qt.TabFocus
        icon.width: 17
        icon.height: 17
        icon.color: enabled ? theme.foreground : theme.textFaint
        icon.source: window.iconRoot + iconFile
        ToolTip.visible: hovered
        ToolTip.text: tip
        background: FocusBackground {
            control: ghost
            cornerRadius: theme.radius
            fill: ghost.down ? theme.pressedFill
                             : ghost.hovered ? theme.hoverFill : "transparent"
            stroke: ghost.hovered ? theme.hoverBorder : "transparent"
        }
    }

    component QuietButton: Button {
        id: quiet
        implicitHeight: 32
        topInset: 0
        bottomInset: 0
        leftPadding: 12
        rightPadding: 12
        hoverEnabled: true
        focusPolicy: Qt.TabFocus
        font.pixelSize: theme.font.body
        font.weight: Font.Medium
        contentItem: Label {
            text: quiet.text
            color: quiet.enabled ? theme.foreground : theme.textFaint
            font: quiet.font
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
        }
        background: FocusBackground {
            control: quiet
            cornerRadius: theme.radius
            fill: quiet.highlighted ? theme.selectedFill
                : quiet.down ? theme.pressedFill
                : quiet.hovered ? theme.hoverFill : theme.normalFill
            stroke: quiet.highlighted ? "transparent"
                                      : quiet.hovered ? theme.hoverBorder : theme.normalBorder
        }
    }

    component CaptureChip: Button {
        id: chip
        property bool selected: false
        property string iconFile: ""
        property bool hasMenu: false
        signal toggleRequested()
        signal menuRequested()

        implicitHeight: 30
        height: 30
        hoverEnabled: true
        focusPolicy: Qt.TabFocus
        topInset: 0
        bottomInset: 0
        leftPadding: 10
        rightPadding: 8
        topPadding: 0
        bottomPadding: 0
        onClicked: toggleRequested()
        contentItem: RowLayout {
            spacing: 6
            ToolButton {
                Layout.preferredWidth: 16
                Layout.preferredHeight: 16
                Layout.alignment: Qt.AlignVCenter
                padding: 0
                topInset: 0
                bottomInset: 0
                leftInset: 0
                rightInset: 0
                display: AbstractButton.IconOnly
                enabled: false
                opacity: chip.enabled ? 1 : .42
                icon.width: 16
                icon.height: 16
                icon.source: window.iconRoot + chip.iconFile
                icon.color: chip.selected ? theme.foreground : theme.textMuted
                background: null
            }
            Label {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                text: chip.text
                elide: Text.ElideRight
                font.pixelSize: theme.font.body
                font.weight: Font.Medium
                color: chip.enabled
                       ? (chip.selected ? theme.foreground : theme.textMuted)
                       : theme.textFaint
                verticalAlignment: Text.AlignVCenter
            }
            Item {
                visible: chip.hasMenu
                Layout.preferredWidth: 14
                Layout.preferredHeight: 16
                Layout.alignment: Qt.AlignVCenter
                ToolButton {
                    anchors.centerIn: parent
                    width: 12
                    height: 12
                    padding: 0
                    topInset: 0
                    bottomInset: 0
                    leftInset: 0
                    rightInset: 0
                    display: AbstractButton.IconOnly
                    enabled: false
                    opacity: 1
                    icon.width: 12
                    icon.height: 12
                    icon.source: window.iconRoot + "chevron-down.svg"
                    icon.color: chip.selected ? theme.foreground : theme.textMuted
                    background: null
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: function(mouse) {
                        mouse.accepted = true
                        chip.menuRequested()
                    }
                }
            }
        }
        background: FocusBackground {
            control: chip
            cornerRadius: theme.radius
            fill: chip.down ? theme.pressedFill
                : chip.selected ? theme.selectedFill
                : chip.hovered ? theme.hoverFill : theme.normalFill
            stroke: chip.selected ? "transparent"
                : chip.hovered ? theme.hoverBorder : theme.normalBorder
        }
    }

    component StyledMenuItem: MenuItem {
        id: menuItem
        implicitHeight: theme.space.popupRowHeight
        height: theme.space.popupRowHeight
        leftPadding: 32
        rightPadding: 10
        topPadding: 0
        bottomPadding: 0
        hoverEnabled: true
        indicator: ToolButton {
            x: 10
            anchors.verticalCenter: parent.verticalCenter
            width: 14
            height: 14
            visible: menuItem.checkable && menuItem.checked
            padding: 0
            enabled: false
            opacity: 1
            icon.width: 14
            icon.height: 14
            icon.source: window.iconRoot + "check.svg"
            icon.color: theme.accent
            background: null
        }
        contentItem: Label {
            text: menuItem.text
            color: menuItem.checked ? theme.menuSelectedText
                                    : menuItem.enabled ? theme.menuText : theme.textMuted
            font.pixelSize: theme.font.body
            font.weight: Font.Medium
            elide: Text.ElideRight
            verticalAlignment: Text.AlignVCenter
        }
        background: Rectangle {
            radius: theme.radius
            color: menuItem.highlighted || menuItem.checked ? theme.menuSelectedBackground : "transparent"
            Behavior on color {
                enabled: window.motionReady
                ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
            }
        }
    }

    component MenuDivider: MenuSeparator {
        implicitHeight: 9
        contentItem: Rectangle {
            implicitHeight: 1
            color: theme.hairline
        }
    }

    Camera {
        id: previewCamera
        cameraDevice: launcher.webcamCameraDevice
        active: launcher.webcam && launcher.webcamPreviewAvailable && !launcher.recording
    }
    CaptureSession {
        camera: previewCamera
        videoOutput: cameraSource
    }
    Connections {
        target: launcher
        function onRecordingStarting() { previewCamera.stop() }
        function onErrorOccurred(message) {
            window.errorMessage = message
            errorTimer.restart()
        }
    }
    Timer {
        id: errorTimer
        interval: 5000
        onTriggered: window.errorMessage = ""
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 28
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            spacing: 12
            Label {
                text: "omarecord"
                font.pixelSize: theme.font.heading
                font.weight: Font.DemiBold
                color: theme.foreground
                lineHeight: 1.35
                Layout.alignment: Qt.AlignVCenter
            }
            Rectangle {
                Layout.preferredWidth: 1
                Layout.preferredHeight: 16
                color: theme.hairlineStrong
                Layout.alignment: Qt.AlignVCenter
            }
            Label {
                text: "SCREEN RECORDING · " + theme.themeName.toUpperCase()
                font.pixelSize: theme.font.caption
                font.bold: true
                font.letterSpacing: 1.2
                color: theme.textMuted
                lineHeight: 1.35
                Layout.alignment: Qt.AlignVCenter
            }
            Item { Layout.fillWidth: true }
            GhostIconButton {
                iconFile: "folder-open.svg"
                tip: "Open…"
                Accessible.name: "Open…"
                onClicked: folderDialog.open()
            }
            GhostIconButton {
                id: settingsButton
                iconFile: "settings-2.svg"
                tip: "Settings"
                Accessible.name: "Settings"
                onClicked: settingsPopover.visible ? settingsPopover.close() : settingsPopover.open()
            }
        }

        Item { Layout.preferredHeight: 28 }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 64

            RowLayout {
                anchors.fill: parent
                spacing: 0
                visible: !window.showingRecording
                Button {
                    id: recordButton
                    Layout.preferredWidth: Math.min(272, 223 + Math.max(0, window.width - 720))
                    Layout.preferredHeight: 64
                    topInset: 0
                    bottomInset: 0
                    leftPadding: 0
                    rightPadding: 0
                    hoverEnabled: true
                    focusPolicy: Qt.TabFocus
                    onClicked: window.beginRecording()
                    Accessible.name: "Record"
                    contentItem: RowLayout {
                        spacing: 12
                        Item {
                            Layout.preferredWidth: 52
                            Layout.preferredHeight: 52
                            Rectangle {
                                anchors.centerIn: parent
                                width: 50
                                height: 50
                                radius: 25
                                color: "transparent"
                                border.width: 2
                                border.color: Qt.alpha(theme.record, recordButton.hovered ? .60 : .30)
                                Behavior on border.color {
                                    enabled: window.motionReady
                                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                                }
                            }
                            Rectangle {
                                anchors.centerIn: parent
                                width: 44
                                height: 44
                                radius: 22
                                color: theme.record
                                scale: recordButton.down ? .96 : 1
                                Behavior on scale {
                                    enabled: window.motionReady
                                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                                }
                                Rectangle {
                                    anchors.centerIn: parent
                                    width: 16
                                    height: 16
                                    radius: 8
                                    color: Qt.rgba(1, 1, 1, 1)
                                }
                            }
                            Rectangle {
                                anchors.fill: parent
                                anchors.margins: -3
                                radius: width / 2
                                color: "transparent"
                                border.width: 2
                                border.color: theme.accent
                                visible: recordButton.visualFocus
                            }
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            spacing: 2
                            Label {
                                text: "Record"
                                font.pixelSize: theme.font.title
                                font.weight: Font.DemiBold
                                color: theme.foreground
                                lineHeight: 1.35
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Layout.minimumWidth: 0
                                spacing: 4
                                Label {
                                    Layout.fillWidth: true
                                    Layout.minimumWidth: 0
                                    text: "Area · Window · Screen"
                                    elide: Text.ElideRight
                                    font.pixelSize: theme.font.bodySmall
                                    color: theme.textMuted
                                    lineHeight: 1.35
                                }
                                Rectangle {
                                    Layout.preferredWidth: shortcutLabel.implicitWidth + 6
                                    Layout.preferredHeight: 20
                                    radius: theme.radius
                                    color: "transparent"
                                    border.width: 1
                                    border.color: theme.normalBorder
                                    Label {
                                        id: shortcutLabel
                                        anchors.centerIn: parent
                                        text: "Ctrl R"
                                        font.family: theme.monoFamily
                                        font.pixelSize: theme.font.bodySmall
                                        color: theme.textMuted
                                    }
                                }
                            }
                        }
                    }
                    background: Rectangle { color: "transparent" }
                }

                Item { Layout.fillWidth: true; Layout.minimumWidth: 8 }

                RowLayout {
                    Layout.alignment: Qt.AlignVCenter
                    spacing: 8
                    CaptureChip {
                        Layout.preferredWidth: implicitWidth
                        text: "System audio"
                        iconFile: "volume-2.svg"
                        selected: launcher.systemAudio
                        onToggleRequested: launcher.systemAudio = !launcher.systemAudio
                    }
                    CaptureChip {
                        id: microphoneChip
                        Layout.preferredWidth: implicitWidth
                        text: "Microphone"
                        iconFile: "mic.svg"
                        hasMenu: true
                        selected: launcher.microphone
                        onToggleRequested: launcher.microphone = !launcher.microphone
                        onMenuRequested: microphoneMenu.popup(microphoneChip, 0, microphoneChip.height + 8)
                    }
                    CaptureChip {
                        id: webcamChip
                        Layout.preferredWidth: implicitWidth
                        text: "Webcam"
                        iconFile: "video.svg"
                        hasMenu: true
                        enabled: launcher.webcam || launcher.webcamDevices.length > 0
                        selected: launcher.webcam
                        onToggleRequested: launcher.webcam = !launcher.webcam
                        onMenuRequested: webcamMenu.popup(webcamChip, webcamChip.width - webcamMenu.width,
                                                         webcamChip.height + 8)
                    }
                }

                Item {
                    id: previewSlot
                    Layout.preferredWidth: width
                    Layout.preferredHeight: 64
                    width: launcher.webcam ? 76 : 0
                    opacity: launcher.webcam ? 1 : 0
                    Behavior on width {
                        enabled: window.motionReady
                        NumberAnimation { duration: 160; easing.type: Easing.OutCubic }
                    }
                    Behavior on opacity {
                        enabled: window.motionReady
                        NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                    }
                    Item {
                        id: previewBubble
                        x: 12
                        width: 64
                        height: 64
                        visible: previewSlot.width > 8
                        VideoOutput {
                            id: cameraSource
                            anchors.centerIn: parent
                            width: window.cameraQuarterTurn ? parent.height : parent.width
                            height: window.cameraQuarterTurn ? parent.width : parent.height
                            fillMode: VideoOutput.PreserveAspectCrop
                            rotation: launcher.webcamRotation
                            visible: false
                            transform: Scale {
                                origin.x: cameraSource.width / 2
                                origin.y: cameraSource.height / 2
                                xScale: launcher.webcamFlipHorizontal ? -1 : 1
                            }
                        }
                        Rectangle {
                            id: cameraMask
                            anchors.fill: parent
                            radius: width / 2
                            color: Qt.rgba(1, 1, 1, 1)
                            visible: false
                            layer.enabled: true
                        }
                        MultiEffect {
                            anchors.fill: parent
                            source: cameraSource
                            maskEnabled: true
                            maskSource: cameraMask
                            visible: launcher.webcamPreviewAvailable
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: width / 2
                            color: theme.surfaceRaised
                            visible: !launcher.webcamPreviewAvailable
                            ToolButton {
                                anchors.centerIn: parent
                                width: 20
                                height: 20
                                padding: 0
                                enabled: false
                                opacity: 1
                                icon.width: 20
                                icon.height: 20
                                icon.source: window.iconRoot + "video.svg"
                                icon.color: theme.textFaint
                                background: null
                            }
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: width / 2
                            color: "transparent"
                            border.width: 1
                            border.color: theme.hairlineStrong
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: webcamMenu.popup(previewBubble, previewBubble.width - webcamMenu.width,
                                                       previewBubble.height + 8)
                        }
                    }
                }
            }

            RowLayout {
                anchors.fill: parent
                visible: window.showingRecording
                spacing: 12
                Rectangle {
                    Layout.preferredWidth: 12
                    Layout.preferredHeight: 12
                    radius: width / 2
                    color: theme.record
                    SequentialAnimation on opacity {
                        running: window.showingRecording
                        loops: Animation.Infinite
                        NumberAnimation { from: 1; to: .35; duration: 700; easing.type: Easing.InOutSine }
                        NumberAnimation { from: .35; to: 1; duration: 700; easing.type: Easing.InOutSine }
                    }
                }
                RowLayout {
                    spacing: 5
                    Label {
                        text: "Recording ·"
                        font.pixelSize: theme.font.title
                        font.weight: Font.DemiBold
                        color: theme.foreground
                    }
                    Label {
                        text: window.screenshotRecording ? "00:12" : launcher.recordingElapsed
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.title
                        font.weight: Font.DemiBold
                        color: theme.foreground
                    }
                }
                Item { Layout.fillWidth: true }
                QuietButton {
                    text: "Stop"
                    highlighted: true
                    onClicked: launcher.stopRecording()
                }
                QuietButton {
                    id: discardButton
                    text: armed ? "Discard?" : "Discard"
                    property bool armed: false
                    onClicked: {
                        if (armed) launcher.cancelRecording()
                        else {
                            armed = true
                            discardTimer.restart()
                        }
                    }
                }
                Timer {
                    id: discardTimer
                    interval: 3000
                    onTriggered: discardButton.armed = false
                }
            }
        }

        Item { Layout.preferredHeight: 28 }
        Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
        Item { Layout.preferredHeight: 24 }

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 24
            spacing: 8
            Label {
                text: "Recent"
                font.pixelSize: theme.font.title
                font.weight: Font.DemiBold
                color: theme.foreground
                lineHeight: 1.35
            }
            Label {
                text: launcher.recentBundles.length
                font.pixelSize: theme.font.caption
                font.bold: true
                font.letterSpacing: 1.2
                color: theme.textFaint
                lineHeight: 1.35
            }
            Item { Layout.fillWidth: true }
            QuietButton {
                text: "Show in folder"
                onClicked: launcher.showRecordingsFolder()
            }
        }

        Item { Layout.preferredHeight: 16 }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            GridView {
                id: recentGrid
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                anchors.left: parent.left
                width: parent.width + 16
                visible: launcher.recentBundles.length > 0
                model: launcher.recentBundles
                clip: true
                activeFocusOnTab: true
                focus: false
                keyNavigationEnabled: true
                keyNavigationWraps: false
                boundsBehavior: Flickable.StopAtBounds
                currentIndex: -1
                readonly property real availableWidth: width - 16
                readonly property int columns: Math.max(1, Math.floor((availableWidth + 16) / 212))
                readonly property real cardWidth: (availableWidth - (columns - 1) * 16) / columns
                readonly property real cardHeight: cardWidth * 9 / 16 + 49
                cellWidth: cardWidth + 16
                cellHeight: cardHeight + 16
                onActiveFocusChanged: {
                    if (activeFocus && currentIndex < 0 && count > 0)
                        currentIndex = 0
                }
                Keys.onReturnPressed: function(event) {
                    if (currentIndex >= 0) launcher.openBundle(model[currentIndex].path)
                    event.accepted = true
                }
                Keys.onEnterPressed: function(event) {
                    if (currentIndex >= 0) launcher.openBundle(model[currentIndex].path)
                    event.accepted = true
                }
                ScrollBar.vertical: ScrollBar {
                    width: 6
                    x: recentGrid.availableWidth - width
                    policy: ScrollBar.AlwaysOn
                    opacity: recentGrid.moving ? 1 : 0
                    padding: 0
                    background: null
                    contentItem: Rectangle {
                        implicitWidth: 6
                        radius: theme.radius
                        color: theme.textFaint
                    }
                    Behavior on opacity {
                        enabled: window.motionReady
                        NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                    }
                }
                delegate: Item {
                    id: recentDelegate
                    required property int index
                    required property var modelData
                    width: recentGrid.cardWidth
                    height: recentGrid.cardHeight
                    Column {
                        anchors.fill: parent
                        spacing: 0
                        Item {
                            id: thumbnail
                            width: parent.width
                            height: width * 9 / 16
                            Rectangle {
                                anchors.fill: parent
                                radius: theme.radius
                                color: theme.surfaceRaised
                                clip: true
                                Image {
                                    anchors.fill: parent
                                    source: recentDelegate.modelData.thumbnail
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    sourceSize: Qt.size(Math.ceil(thumbnail.width * 2),
                                                        Math.ceil(thumbnail.height * 2))
                                    visible: source.toString() !== ""
                                }
                                ToolButton {
                                    anchors.centerIn: parent
                                    width: 24
                                    height: 24
                                    visible: recentDelegate.modelData.thumbnail === ""
                                    padding: 0
                                    enabled: false
                                    opacity: 1
                                    icon.width: 24
                                    icon.height: 24
                                    icon.source: window.iconRoot + "film.svg"
                                    icon.color: theme.textFaint
                                    background: null
                                }
                            }
                            Rectangle {
                                anchors.fill: parent
                                radius: theme.radius
                                color: "transparent"
                                border.width: 1
                                border.color: recentMouse.containsMouse || recentGrid.currentIndex === recentDelegate.index
                                              ? theme.hoverBorder : theme.normalBorder
                                Behavior on border.color {
                                    enabled: window.motionReady
                                    ColorAnimation { duration: 120; easing.type: Easing.OutCubic }
                                }
                            }
                            Rectangle {
                                anchors.right: parent.right
                                anchors.bottom: parent.bottom
                                anchors.margins: 8
                                width: durationLabel.implicitWidth + 12
                                height: durationLabel.implicitHeight + 8
                                radius: theme.radius
                                color: Qt.alpha(theme.surface, .80)
                                Label {
                                    id: durationLabel
                                    anchors.centerIn: parent
                                    text: recentDelegate.modelData.durationText
                                    font.family: theme.monoFamily
                                    font.pixelSize: theme.font.bodySmall
                                    color: theme.foreground
                                }
                            }
                            Rectangle {
                                anchors.centerIn: parent
                                width: 36
                                height: 36
                                radius: theme.radius
                                color: Qt.alpha(theme.surface, .60)
                                opacity: recentMouse.containsMouse ? 1 : 0
                                Behavior on opacity {
                                    enabled: window.motionReady
                                    NumberAnimation { duration: 120; easing.type: Easing.OutCubic }
                                }
                                ToolButton {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: 1
                                    width: 17
                                    height: 17
                                    padding: 0
                                    enabled: false
                                    opacity: 1
                                    icon.width: 17
                                    icon.height: 17
                                    icon.source: window.iconRoot + "play.svg"
                                    icon.color: Qt.rgba(1, 1, 1, .90)
                                    background: null
                                }
                            }
                            MouseArea {
                                id: recentMouse
                                anchors.fill: parent
                                acceptedButtons: Qt.LeftButton | Qt.RightButton
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: function(mouse) {
                                    recentGrid.currentIndex = recentDelegate.index
                                    recentGrid.forceActiveFocus(Qt.MouseFocusReason)
                                    if (mouse.button === Qt.RightButton)
                                        itemMenu.popup(thumbnail, mouse.x, mouse.y)
                                    else
                                        launcher.openBundle(recentDelegate.modelData.path)
                                }
                            }
                        }
                        Item { width: 1; height: 10 }
                        Label {
                            width: parent.width
                            height: 16
                            text: recentDelegate.modelData.titleText
                            elide: Text.ElideRight
                            font.pixelSize: theme.font.body
                            font.weight: Font.Medium
                            color: theme.foreground
                            verticalAlignment: Text.AlignVCenter
                        }
                        Item { width: 1; height: 4 }
                        Label {
                            width: parent.width
                            height: 15
                            text: recentDelegate.modelData.detailText
                            elide: Text.ElideRight
                            font.pixelSize: theme.font.caption
                            color: theme.textMuted
                            verticalAlignment: Text.AlignVCenter
                        }
                    }
                    Menu {
                        id: itemMenu
                        width: 176
                        padding: 4
                        background: Rectangle {
                            radius: theme.radius
                            color: theme.menuBackground
                            border.width: 2
                            border.color: theme.popupBorder
                        }
                        StyledMenuItem {
                            text: "Open"
                            onTriggered: launcher.openBundle(recentDelegate.modelData.path)
                        }
                        StyledMenuItem {
                            text: "Rename"
                            onTriggered: {
                                window.pendingRenamePath = recentDelegate.modelData.path
                                renameField.text = recentDelegate.modelData.name
                                renameDialog.open()
                                renameField.forceActiveFocus()
                            }
                        }
                        StyledMenuItem {
                            text: "Show in folder"
                            onTriggered: launcher.showBundleInFolder(recentDelegate.modelData.path)
                        }
                        MenuDivider {}
                        StyledMenuItem {
                            text: "Delete…"
                            onTriggered: {
                                window.pendingDeletePath = recentDelegate.modelData.path
                                window.pendingDeleteName = recentDelegate.modelData.titleText
                                deleteDialog.open()
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                anchors.centerIn: parent
                visible: launcher.recentBundles.length === 0
                spacing: 6
                ToolButton {
                    Layout.preferredWidth: 28
                    Layout.preferredHeight: 28
                    Layout.alignment: Qt.AlignHCenter
                    padding: 0
                    enabled: false
                    opacity: 1
                    icon.width: 28
                    icon.height: 28
                    icon.source: window.iconRoot + "film.svg"
                    icon.color: theme.textFaint
                    background: null
                }
                Label {
                    text: "No recordings yet"
                    font.pixelSize: theme.font.body
                    font.weight: Font.Medium
                    color: theme.foreground
                    Layout.alignment: Qt.AlignHCenter
                }
                Label {
                    text: "Your recordings appear here after you stop."
                    font.pixelSize: theme.font.bodySmall
                    color: theme.textMuted
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }
    }

    Menu {
        id: microphoneMenu
        width: 280
        padding: 4
        background: Rectangle {
            radius: theme.radius
            color: theme.menuBackground
            border.width: 2
            border.color: theme.popupBorder
        }
        Instantiator {
            model: launcher.audioDevices
            delegate: StyledMenuItem {
                required property var modelData
                text: modelData.text
                checkable: true
                checked: modelData.value === launcher.microphoneDevice
                onTriggered: {
                    launcher.microphoneDevice = modelData.value
                    launcher.microphone = true
                }
            }
            onObjectAdded: function(index, object) { microphoneMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { microphoneMenu.removeItem(object) }
        }
    }

    Menu {
        id: webcamMenu
        width: 300
        padding: 4
        background: Rectangle {
            radius: theme.radius
            color: theme.menuBackground
            border.width: 2
            border.color: theme.popupBorder
        }
        Instantiator {
            model: launcher.webcamDevices
            delegate: StyledMenuItem {
                required property var modelData
                text: modelData.text
                checkable: true
                checked: modelData.value === launcher.webcamDevice
                onTriggered: {
                    launcher.webcamDevice = modelData.value
                    launcher.webcam = true
                }
            }
            onObjectAdded: function(index, object) { webcamMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { webcamMenu.removeItem(object) }
        }
        MenuDivider {}
        MenuItem {
            implicitHeight: 40
            hoverEnabled: false
            contentItem: RowLayout {
                spacing: 4
                Label { text: "Rotate"; font.pixelSize: theme.font.body; color: theme.textMuted; Layout.fillWidth: true }
                Repeater {
                    model: [0, 90, 180, 270]
                    delegate: Button {
                        required property int modelData
                        Layout.preferredWidth: 38
                        Layout.preferredHeight: 26
                        topInset: 0
                        bottomInset: 0
                        padding: 0
                        text: modelData + "°"
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.caption
                        onClicked: launcher.webcamRotation = modelData
                        contentItem: Label {
                            text: parent.text
                            font: parent.font
                            color: theme.foreground
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: theme.radius
                            color: launcher.webcamRotation === modelData ? theme.selectedFill : theme.normalFill
                            border.width: 1
                            border.color: launcher.webcamRotation === modelData
                                          ? theme.selectedBorder : theme.normalBorder
                        }
                    }
                }
            }
            background: Rectangle { color: "transparent" }
        }
        MenuItem {
            implicitHeight: 36
            hoverEnabled: false
            contentItem: RowLayout {
                Label { text: "Flip horizontal"; font.pixelSize: theme.font.body; color: theme.textMuted; Layout.fillWidth: true }
                OmToggle {
                    Layout.preferredWidth: 42
                    Layout.preferredHeight: 28
                    checked: launcher.webcamFlipHorizontal
                    onToggled: launcher.webcamFlipHorizontal = checked
                }
            }
            background: Rectangle { color: "transparent" }
        }
        MenuItem {
            implicitHeight: 40
            hoverEnabled: false
            contentItem: RowLayout {
                spacing: 4
                Label { text: "Capture size"; font.pixelSize: theme.font.body; color: theme.textMuted; Layout.fillWidth: true }
                Repeater {
                    model: [720, 1080]
                    delegate: Button {
                        required property int modelData
                        Layout.preferredWidth: 58
                        Layout.preferredHeight: 26
                        topInset: 0
                        bottomInset: 0
                        padding: 0
                        text: modelData + "p"
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.caption
                        onClicked: launcher.webcamHeight = modelData
                        contentItem: Label {
                            text: parent.text
                            font: parent.font
                            color: theme.foreground
                            horizontalAlignment: Text.AlignHCenter
                            verticalAlignment: Text.AlignVCenter
                        }
                        background: Rectangle {
                            radius: theme.radius
                            color: launcher.webcamHeight === modelData ? theme.selectedFill : theme.normalFill
                            border.width: 1
                            border.color: launcher.webcamHeight === modelData
                                          ? theme.selectedBorder : theme.normalBorder
                        }
                    }
                }
            }
            background: Rectangle { color: "transparent" }
        }
    }

    Popup {
        id: settingsPopover
        parent: window.contentItem
        x: window.width - width - 28
        y: 72
        width: 288
        padding: 16
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: OmPopupCard { }
        contentItem: ColumnLayout {
            spacing: 8
            Label {
                text: "Capture settings"
                font.pixelSize: theme.font.title
                font.weight: Font.DemiBold
                color: theme.foreground
            }
            Label {
                Layout.fillWidth: true
                text: "Defaults for new recordings"
                font.pixelSize: theme.font.bodySmall
                color: theme.textMuted
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
            OmToggle {
                Layout.fillWidth: true
                text: "System audio"
                font.pixelSize: theme.font.body
                checked: launcher.systemAudio
                onToggled: launcher.systemAudio = checked
            }
            OmToggle {
                Layout.fillWidth: true
                text: "Microphone"
                font.pixelSize: theme.font.body
                checked: launcher.microphone
                onToggled: launcher.microphone = checked
            }
            OmToggle {
                Layout.fillWidth: true
                text: "Webcam"
                font.pixelSize: theme.font.body
                enabled: launcher.webcam || launcher.webcamDevices.length > 0
                checked: launcher.webcam
                onToggled: launcher.webcam = checked
            }
        }
    }

    Dialog {
        id: renameDialog
        parent: window.contentItem
        x: (window.width - width) / 2
        y: (window.height - height) / 2
        width: 380
        modal: true
        focus: true
        padding: 20
        title: "Rename recording"
        standardButtons: Dialog.NoButton
        background: OmPopupCard { }
        contentItem: ColumnLayout {
            spacing: 16
            OmTextField {
                id: renameField
                Layout.fillWidth: true
                placeholderText: "Recording name"
                selectByMouse: true
                onAccepted: {
                    if (text.trim() !== "") {
                        launcher.renameBundle(window.pendingRenamePath, text)
                        renameDialog.close()
                    }
                }
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                QuietButton { text: "Cancel"; onClicked: renameDialog.close() }
                QuietButton {
                    text: "Rename"
                    highlighted: true
                    enabled: renameField.text.trim() !== ""
                    onClicked: {
                        launcher.renameBundle(window.pendingRenamePath, renameField.text)
                        renameDialog.close()
                    }
                }
            }
        }
    }

    Dialog {
        id: deleteDialog
        parent: window.contentItem
        x: (window.width - width) / 2
        y: (window.height - height) / 2
        width: 380
        modal: true
        focus: true
        padding: 20
        title: "Delete recording?"
        standardButtons: Dialog.NoButton
        background: OmPopupCard { }
        contentItem: ColumnLayout {
            spacing: 16
            Label {
                Layout.fillWidth: true
                text: "“" + window.pendingDeleteName + "” will be permanently deleted."
                wrapMode: Text.WordWrap
                font.pixelSize: theme.font.subtitle
                color: theme.textMuted
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                QuietButton { text: "Cancel"; onClicked: deleteDialog.close() }
                QuietButton {
                    text: "Delete"
                    highlighted: true
                    onClicked: {
                        launcher.deleteBundle(window.pendingDeletePath)
                        deleteDialog.close()
                    }
                }
            }
        }
    }

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 18
        width: Math.min(460, errorLabel.implicitWidth + 32)
        height: 38
        radius: theme.radius
        visible: window.errorMessage !== ""
        color: theme.foreground
        z: 40
        Label {
            id: errorLabel
            anchors.centerIn: parent
            width: parent.width - 24
            text: window.errorMessage
            color: theme.surface
            font.pixelSize: theme.font.body
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
        }
    }

    Shortcut {
        sequence: "Ctrl+R"
        enabled: !launcher.recording && !deleteDialog.visible && !renameDialog.visible
        onActivated: window.beginRecording()
    }
    Shortcut {
        sequence: "Return"
        enabled: !launcher.recording && !deleteDialog.visible && !renameDialog.visible
                 && !recentGrid.activeFocus
        onActivated: window.beginRecording()
    }
    Shortcut {
        sequence: "Enter"
        enabled: !launcher.recording && !deleteDialog.visible && !renameDialog.visible
                 && !recentGrid.activeFocus
        onActivated: window.beginRecording()
    }

    FolderDialog {
        id: folderDialog
        title: "Open an omarecord bundle"
        onAccepted: launcher.openBundle(selectedFolder)
    }
}
