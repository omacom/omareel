import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
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

    component GhostIconButton: OmIconButton {
        property string iconFile: ""
        property string tip: ""
        tooltipText: tip
        icon.color: enabled ? theme.foreground : theme.textFaint
        icon.source: window.iconRoot + iconFile
    }

    component QuietButton: OmButton {
        bordered: true
        primary: highlighted
    }

    component CaptureChip: OmButton {
        id: chip
        property string iconFile: ""
        property bool hasMenu: false
        signal toggleRequested()
        signal menuRequested()
        bordered: true
        icon.source: window.iconRoot + iconFile
        trailingIconSource: hasMenu ? window.iconRoot + "chevron-down.svg" : ""
        onClicked: toggleRequested()
        MouseArea {
            visible: chip.hasMenu
            anchors.top: parent.top
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            width: 28
            z: 2
            cursorShape: Qt.PointingHandCursor
            onClicked: chip.menuRequested()
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
        indicator: Item {
            x: 10
            anchors.verticalCenter: parent.verticalCenter
            width: 14
            height: 14
            visible: menuItem.checkable && menuItem.checked
            IconImage {
                anchors.fill: parent
                source: window.iconRoot + "check.svg"
                sourceSize: Qt.size(14, 14)
                color: theme.accent
            }
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
                OmButton {
                    id: recordButton
                    Layout.preferredWidth: 112
                    Layout.preferredHeight: theme.space.controlHeight
                    Layout.alignment: Qt.AlignVCenter
                    primary: true
                    bordered: true
                    text: "Record"
                    icon.source: window.iconRoot + "circle.svg"
                    icon.color: theme.record
                    onClicked: window.beginRecording()
                    Accessible.name: "Record"
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
                            OmIconButton {
                                anchors.centerIn: parent
                                width: 20
                                height: 20
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
                                OmIconButton {
                                    anchors.centerIn: parent
                                    width: 24
                                    height: 24
                                    visible: recentDelegate.modelData.thumbnail === ""
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
                                OmIconButton {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: 1
                                    width: 17
                                    height: 17
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
                OmIconButton {
                    Layout.preferredWidth: 28
                    Layout.preferredHeight: 28
                    Layout.alignment: Qt.AlignHCenter
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
                    delegate: OmButton {
                        required property int modelData
                        Layout.preferredWidth: 38
                        Layout.preferredHeight: theme.space.controlHeight
                        text: modelData + "°"
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.caption
                        bordered: true
                        selected: launcher.webcamRotation === modelData
                        onClicked: launcher.webcamRotation = modelData
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
                    delegate: OmButton {
                        required property int modelData
                        Layout.preferredWidth: 58
                        Layout.preferredHeight: theme.space.controlHeight
                        text: modelData + "p"
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.caption
                        bordered: true
                        selected: launcher.webcamHeight === modelData
                        onClicked: launcher.webcamHeight = modelData
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
                    destructive: true
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
