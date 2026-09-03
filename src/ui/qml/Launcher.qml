import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Dialogs
import QtQuick.Layouts
import QtMultimedia

ApplicationWindow {
    id: window
    visible: true
    width: 900
    height: 640
    minimumWidth: 720
    minimumHeight: 520
    title: "omarecord"
    color: theme.background
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground

    readonly property color muted: Qt.alpha(theme.foreground, 0.58)
    readonly property color subtleBorder: Qt.alpha(theme.foreground, 0.11)
    readonly property string iconRoot: "qrc:/omarecord/assets/icons/" + "luc" + "ide/"
    readonly property bool cameraQuarterTurn: launcher.webcamRotation === 90
                                               || launcher.webcamRotation === 270
    property string pendingDeletePath: ""
    property string pendingDeleteName: ""
    property string errorMessage: ""

    function beginRecording() {
        if (!launcher.recording)
            launcher.record()
    }

    component CaptureChip: Button {
        id: chip
        property bool selected: false
        property string iconFile: ""
        property bool hasMenu: false
        signal toggleRequested()
        signal menuRequested()

        implicitHeight: 32
        height: 32
        hoverEnabled: true
        focusPolicy: Qt.TabFocus
        topInset: 0
        bottomInset: 0
        leftPadding: 8
        rightPadding: hasMenu ? 2 : 8
        onClicked: toggleRequested()

        contentItem: RowLayout {
            spacing: 4
            ToolButton {
                Layout.preferredWidth: 16
                Layout.preferredHeight: 16
                padding: 0
                leftInset: 0
                rightInset: 0
                topInset: 0
                bottomInset: 0
                enabled: false
                opacity: 1
                icon.width: 16
                icon.height: 16
                icon.color: chip.selected ? theme.accentForeground : theme.foreground
                icon.source: window.iconRoot + chip.iconFile
                background: null
            }
            Label {
                Layout.fillWidth: true
                text: chip.text
                elide: Text.ElideRight
                font.pixelSize: 12
                font.weight: Font.Medium
                color: chip.selected ? theme.accentForeground : theme.foreground
                verticalAlignment: Text.AlignVCenter
            }
            ToolButton {
                visible: chip.hasMenu
                Layout.preferredWidth: 18
                Layout.preferredHeight: 24
                topInset: 0
                bottomInset: 0
                icon.width: 13
                icon.height: 13
                icon.color: chip.selected ? theme.accentForeground : theme.foreground
                icon.source: window.iconRoot + "chevron-down.svg"
                onClicked: chip.menuRequested()
                Accessible.name: chip.text + " device menu"
                background: Rectangle {
                    radius: 11
                    color: parent.hovered ? Qt.alpha(chip.selected ? theme.accentForeground : theme.foreground, 0.12)
                                          : "transparent"
                }
            }
        }
        background: Rectangle {
            radius: 16
            color: chip.selected ? (chip.down ? Qt.darker(theme.accent, 1.12)
                                              : chip.hovered ? Qt.lighter(theme.accent, 1.06) : theme.accent)
                                 : chip.hovered ? Qt.alpha(theme.foreground, 0.06) : "transparent"
            border.width: chip.selected ? 0 : 1
            border.color: chip.hovered ? Qt.alpha(theme.accent, 0.72) : Qt.alpha(theme.foreground, 0.22)
        }
    }

    Camera {
        id: previewCamera
        cameraDevice: launcher.webcamCameraDevice
        active: launcher.webcam && launcher.webcamPreviewAvailable && !launcher.recording
    }
    CaptureSession {
        camera: previewCamera
        videoOutput: launcherCameraOutput
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
        anchors.margins: 32
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            spacing: 10

            Label {
                text: "omarecord"
                font.pixelSize: 26
                font.weight: Font.DemiBold
                color: theme.accent
                Layout.alignment: Qt.AlignVCenter
            }
            Label {
                text: "Capture once. Polish every detail."
                font.pixelSize: 13
                color: window.muted
                Layout.alignment: Qt.AlignVCenter
            }
            Item { Layout.fillWidth: true }
            Button {
                Layout.preferredHeight: 32
                Layout.preferredWidth: 88
                hoverEnabled: true
                focusPolicy: Qt.TabFocus
                topInset: 0
                bottomInset: 0
                onClicked: folderDialog.open()
                Accessible.name: "Open a recording bundle"
                contentItem: RowLayout {
                    spacing: 6
                    ToolButton {
                        Layout.preferredWidth: 20
                        Layout.preferredHeight: 20
                        padding: 0
                        leftInset: 0
                        rightInset: 0
                        topInset: 0
                        bottomInset: 0
                        enabled: false
                        opacity: 1
                        icon.width: 16
                        icon.height: 16
                        icon.color: theme.foreground
                        icon.source: window.iconRoot + "folder-open.svg"
                        background: null
                    }
                    Label {
                        text: "Open…"
                        font.pixelSize: 12
                        color: theme.foreground
                    }
                }
                background: Rectangle {
                    radius: 7
                    color: parent.hovered ? Qt.alpha(theme.foreground, 0.07) : "transparent"
                    border.width: 1
                    border.color: parent.hovered ? Qt.alpha(theme.foreground, 0.22) : "transparent"
                }
            }
            ToolButton {
                id: settingsButton
                Layout.preferredWidth: 32
                Layout.preferredHeight: 32
                hoverEnabled: true
                focusPolicy: Qt.TabFocus
                topInset: 0
                bottomInset: 0
                icon.width: 18
                icon.height: 18
                icon.color: theme.foreground
                icon.source: window.iconRoot + "settings-2.svg"
                onClicked: settingsPopover.visible ? settingsPopover.close() : settingsPopover.open()
                Accessible.name: "Capture settings"
                ToolTip.visible: hovered
                ToolTip.text: "Capture settings"
                background: Rectangle {
                    radius: 7
                    color: parent.hovered || settingsPopover.visible
                           ? Qt.alpha(theme.foreground, 0.07) : "transparent"
                }
            }
        }

        Item { Layout.preferredHeight: 16 }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 160
            Layout.minimumHeight: 160
            Layout.maximumHeight: 160
            radius: 16
            color: theme.lighterBackground
            border.width: 1
            border.color: window.subtleBorder

            RowLayout {
                anchors.fill: parent
                anchors.margins: 24
                spacing: launcher.webcam ? 8 : 16
                visible: !launcher.recording

                ColumnLayout {
                    Layout.preferredWidth: launcher.webcam
                                           ? (window.width < 820 ? 160 : 180) : 220
                    Layout.minimumWidth: Layout.preferredWidth
                    Layout.maximumWidth: Layout.preferredWidth
                    spacing: 8

                    Button {
                        id: recordButton
                        Layout.preferredWidth: 180
                        Layout.preferredHeight: 52
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        topInset: 0
                        bottomInset: 0
                        onClicked: window.beginRecording()
                        Accessible.name: "Record"
                        contentItem: RowLayout {
                            spacing: 10
                            Item { Layout.fillWidth: true }
                            ToolButton {
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22
                                padding: 0
                                leftInset: 0
                                rightInset: 0
                                topInset: 0
                                bottomInset: 0
                                enabled: false
                                opacity: 1
                                icon.width: 18
                                icon.height: 18
                                icon.color: "white"
                                icon.source: window.iconRoot + "circle.svg"
                                background: null
                            }
                            Label {
                                text: "Record"
                                color: theme.accentForeground
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                            }
                            Item { Layout.fillWidth: true }
                        }
                        background: Rectangle {
                            radius: 10
                            color: parent.down ? Qt.darker(theme.accent, 1.15)
                                               : parent.hovered ? Qt.lighter(theme.accent, 1.07)
                                                                : theme.accent
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        text: "Drag to select an area · click a window · click the desktop for the whole screen"
                        wrapMode: Text.WordWrap
                        font.pixelSize: 11
                        lineHeight: 1.08
                        color: window.muted
                    }
                }

                Item { Layout.fillWidth: true }

                RowLayout {
                    id: captureStrip
                    Layout.alignment: Qt.AlignVCenter
                    spacing: window.width < 820 ? 6 : 8

                    CaptureChip {
                        Layout.preferredWidth: 120
                        text: "System audio"
                        iconFile: "volume-2.svg"
                        selected: launcher.systemAudio
                        onToggleRequested: launcher.systemAudio = !launcher.systemAudio
                    }
                    CaptureChip {
                        id: microphoneChip
                        Layout.preferredWidth: 124
                        text: "Microphone"
                        iconFile: "mic.svg"
                        hasMenu: true
                        selected: launcher.microphone
                        onToggleRequested: launcher.microphone = !launcher.microphone
                        onMenuRequested: microphoneMenu.popup(microphoneChip, 0, microphoneChip.height + 6)
                    }
                    CaptureChip {
                        id: webcamChip
                        Layout.preferredWidth: 124
                        text: "Webcam"
                        iconFile: "video.svg"
                        hasMenu: true
                        enabled: launcher.webcam || launcher.webcamDevices.length > 0
                        selected: launcher.webcam
                        onToggleRequested: launcher.webcam = !launcher.webcam
                        onMenuRequested: webcamMenu.popup(webcamChip, 0, webcamChip.height + 6)
                    }
                }

                Rectangle {
                    id: previewFrame
                    visible: launcher.webcam
                    Layout.preferredWidth: window.width < 820 ? 154 : 200
                    Layout.preferredHeight: Layout.preferredWidth * 9 / 16
                    Layout.minimumWidth: Layout.preferredWidth
                    Layout.maximumWidth: Layout.preferredWidth
                    radius: 10
                    clip: true
                    color: Qt.darker(theme.background, 1.12)
                    border.width: 1
                    border.color: Qt.alpha(theme.foreground, 0.15)

                    VideoOutput {
                        id: launcherCameraOutput
                        anchors.centerIn: parent
                        width: window.cameraQuarterTurn ? parent.height : parent.width
                        height: window.cameraQuarterTurn ? parent.width : parent.height
                        fillMode: VideoOutput.PreserveAspectCrop
                        rotation: launcher.webcamRotation
                        visible: launcher.webcamPreviewAvailable
                        transform: Scale {
                            origin.x: launcherCameraOutput.width / 2
                            origin.y: launcherCameraOutput.height / 2
                            xScale: launcher.webcamFlipHorizontal ? -1 : 1
                        }
                    }
                    Column {
                        anchors.centerIn: parent
                        spacing: 5
                        visible: !launcher.webcamPreviewAvailable
                        ToolButton {
                            anchors.horizontalCenter: parent.horizontalCenter
                            width: 24
                            height: 24
                            padding: 0
                            leftInset: 0
                            rightInset: 0
                            topInset: 0
                            bottomInset: 0
                            enabled: false
                            opacity: 0.42
                            icon.width: 24
                            icon.height: 24
                            icon.color: theme.foreground
                            icon.source: window.iconRoot + "video.svg"
                            background: null
                        }
                        Label {
                            text: "Preview unavailable"
                            font.pixelSize: 10
                            color: Qt.alpha(theme.foreground, 0.48)
                        }
                    }
                    HoverHandler { id: previewHover }
                    Row {
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.margins: 6
                        spacing: 4
                        opacity: previewHover.hovered ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: 120 } }
                        ToolButton {
                            width: 27
                            height: 27
                            topInset: 0
                            bottomInset: 0
                            icon.width: 14
                            icon.height: 14
                            icon.color: "white"
                            icon.source: window.iconRoot + "rotate-cw.svg"
                            onClicked: launcher.webcamRotation = (launcher.webcamRotation + 90) % 360
                            Accessible.name: "Rotate camera"
                            background: Rectangle { radius: 6; color: Qt.rgba(0, 0, 0, 0.64) }
                        }
                        ToolButton {
                            width: 27
                            height: 27
                            topInset: 0
                            bottomInset: 0
                            icon.width: 14
                            icon.height: 14
                            icon.color: "white"
                            icon.source: window.iconRoot + "flip-horizontal-2.svg"
                            onClicked: launcher.webcamFlipHorizontal = !launcher.webcamFlipHorizontal
                            Accessible.name: "Flip camera"
                            background: Rectangle { radius: 6; color: Qt.rgba(0, 0, 0, 0.64) }
                        }
                    }
                }
            }

            RowLayout {
                anchors.fill: parent
                anchors.margins: 24
                visible: launcher.recording
                spacing: 12

                Rectangle {
                    Layout.preferredWidth: 11
                    Layout.preferredHeight: 11
                    radius: 6
                    color: "#ef4444"
                    SequentialAnimation on opacity {
                        loops: Animation.Infinite
                        NumberAnimation { from: 1; to: 0.35; duration: 700 }
                        NumberAnimation { from: 0.35; to: 1; duration: 700 }
                    }
                }
                Label {
                    text: "Recording · " + launcher.recordingElapsed
                    font.pixelSize: 17
                    font.weight: Font.DemiBold
                    color: theme.foreground
                }
                Item { Layout.fillWidth: true }
                Button {
                    Layout.preferredWidth: 92
                    Layout.preferredHeight: 40
                    focusPolicy: Qt.TabFocus
                    hoverEnabled: true
                    topInset: 0
                    bottomInset: 0
                    onClicked: launcher.stopRecording()
                    contentItem: Label {
                        text: "Stop"
                        color: theme.accentForeground
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                    }
                    background: Rectangle {
                        radius: 8
                        color: parent.down ? Qt.darker(theme.accent, 1.15)
                                           : parent.hovered ? Qt.lighter(theme.accent, 1.07) : theme.accent
                    }
                }
                Button {
                    id: cancelRecordingButton
                    Layout.preferredWidth: 92
                    Layout.preferredHeight: 40
                    text: armed ? "Discard?" : "Cancel"
                    flat: true
                    focusPolicy: Qt.TabFocus
                    property bool armed: false
                    onClicked: {
                        if (armed) launcher.cancelRecording()
                        else {
                            armed = true
                            cancelRecordingTimer.restart()
                        }
                    }
                }
                Timer {
                    id: cancelRecordingTimer
                    interval: 3000
                    onTriggered: cancelRecordingButton.armed = false
                }
            }
        }

        Item { Layout.preferredHeight: 24 }

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 24
            spacing: 7
            Label {
                text: "Recent recordings"
                font.pixelSize: 15
                font.weight: Font.DemiBold
                color: theme.foreground
            }
            Label {
                text: launcher.recentBundles.length
                font.pixelSize: 11
                color: window.muted
            }
            Item { Layout.fillWidth: true }
        }

        Item { Layout.preferredHeight: 8 }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            GridView {
                id: recentGrid
                anchors.fill: parent
                visible: launcher.recentBundles.length > 0
                model: launcher.recentBundles
                readonly property int columns: Math.max(1, Math.floor((width + 16) / 250))
                cellWidth: width / columns
                cellHeight: Math.ceil((cellWidth - 16) * 9 / 16) + 70
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

                delegate: Item {
                    id: recentDelegate
                    required property var modelData
                    width: GridView.view.cellWidth - 16
                    height: GridView.view.cellHeight - 16

                    Button {
                        id: recentCard
                        anchors.fill: parent
                        anchors.topMargin: hovered || itemMenu.visible ? 0 : 2
                        anchors.bottomMargin: hovered || itemMenu.visible ? 2 : 0
                        hoverEnabled: true
                        focusPolicy: Qt.TabFocus
                        topInset: 0
                        bottomInset: 0
                        leftPadding: 8
                        rightPadding: 8
                        topPadding: 8
                        bottomPadding: 8
                        onClicked: launcher.openBundle(recentDelegate.modelData.path)
                        Behavior on anchors.topMargin { NumberAnimation { duration: 100 } }

                        TapHandler {
                            acceptedButtons: Qt.RightButton
                            onTapped: itemMenu.popup()
                        }

                        background: Rectangle {
                            radius: 12
                            color: parent.pressed ? Qt.alpha(theme.foreground, 0.09)
                                                  : parent.hovered || itemMenu.visible
                                                    ? Qt.alpha(theme.foreground, 0.045) : "transparent"
                            border.width: 1
                            border.color: parent.hovered || itemMenu.visible
                                          ? Qt.alpha(theme.accent, 0.78) : "transparent"
                        }
                        contentItem: ColumnLayout {
                            spacing: 6
                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: width * 9 / 16
                                radius: 10
                                color: Qt.darker(theme.background, 1.12)
                                clip: true
                                Image {
                                    anchors.fill: parent
                                    source: recentDelegate.modelData.thumbnail
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    sourceSize.width: 560
                                    sourceSize.height: 315
                                    visible: source.toString() !== ""
                                }
                                Label {
                                    anchors.centerIn: parent
                                    visible: recentDelegate.modelData.thumbnail === ""
                                    text: "No preview"
                                    color: Qt.alpha(theme.foreground, 0.42)
                                    font.pixelSize: 11
                                }
                                ToolButton {
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.margins: 6
                                    width: 28
                                    height: 28
                                    visible: recentCard.hovered || itemMenu.visible
                                    text: "…"
                                    font.pixelSize: 18
                                    onClicked: itemMenu.popup()
                                    Accessible.name: "Recording actions"
                                    background: Rectangle {
                                        radius: 7
                                        color: Qt.rgba(0, 0, 0, parent.parent.hovered ? 0.72 : 0.58)
                                    }
                                    contentItem: Label {
                                        text: parent.text
                                        color: "white"
                                        font.pixelSize: 18
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                    }
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: recentDelegate.modelData.name
                                elide: Text.ElideRight
                                maximumLineCount: 1
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                                color: theme.foreground
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 6
                                Label {
                                    Layout.fillWidth: true
                                    text: recentDelegate.modelData.dateText
                                    elide: Text.ElideRight
                                    font.pixelSize: 11
                                    color: window.muted
                                }
                                Label {
                                    text: recentDelegate.modelData.durationText
                                    font.pixelSize: 11
                                    color: window.muted
                                }
                            }
                        }

                        Menu {
                            id: itemMenu
                            width: 170
                            MenuItem {
                                text: "Open"
                                onTriggered: launcher.openBundle(recentDelegate.modelData.path)
                            }
                            MenuItem {
                                text: "Show in folder"
                                onTriggered: launcher.showBundleInFolder(recentDelegate.modelData.path)
                            }
                            MenuSeparator {}
                            MenuItem {
                                text: "Delete"
                                onTriggered: {
                                    window.pendingDeletePath = recentDelegate.modelData.path
                                    window.pendingDeleteName = recentDelegate.modelData.name
                                    deleteDialog.open()
                                }
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                anchors.centerIn: parent
                visible: launcher.recentBundles.length === 0
                spacing: 7
                Label {
                    text: "No recordings yet"
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                    Layout.alignment: Qt.AlignHCenter
                    color: theme.foreground
                }
                Label {
                    text: "Your finished recordings will appear here."
                    color: window.muted
                    font.pixelSize: 12
                    Layout.alignment: Qt.AlignHCenter
                }
                Button {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 4
                    Layout.preferredWidth: 104
                    Layout.preferredHeight: 34
                    text: "Record"
                    focusPolicy: Qt.TabFocus
                    onClicked: window.beginRecording()
                    background: Rectangle {
                        radius: 7
                        color: parent.hovered ? Qt.alpha(theme.foreground, 0.06) : "transparent"
                        border.width: 1
                        border.color: Qt.alpha(theme.foreground, 0.20)
                    }
                }
            }
        }
    }

    Menu {
        id: microphoneMenu
        width: 270
        MenuItem { text: "Microphone device"; enabled: false }
        Instantiator {
            model: launcher.audioDevices
            delegate: MenuItem {
                required property var modelData
                text: modelData.text
                checkable: true
                checked: modelData.value === launcher.microphoneDevice
                onTriggered: {
                    launcher.microphoneDevice = modelData.value
                    launcher.microphone = true
                }
            }
            onObjectAdded: (index, object) => microphoneMenu.insertItem(index + 1, object)
            onObjectRemoved: (index, object) => microphoneMenu.removeItem(object)
        }
    }

    Menu {
        id: webcamMenu
        width: 270
        MenuItem { text: "Webcam device"; enabled: false }
        Instantiator {
            model: launcher.webcamDevices
            delegate: MenuItem {
                required property var modelData
                text: modelData.text
                checkable: true
                checked: modelData.value === launcher.webcamDevice
                onTriggered: {
                    launcher.webcamDevice = modelData.value
                    launcher.webcam = true
                }
            }
            onObjectAdded: (index, object) => webcamMenu.insertItem(index + 1, object)
            onObjectRemoved: (index, object) => webcamMenu.removeItem(object)
        }
        MenuSeparator {}
        MenuItem {
            text: "Rotate"
            onTriggered: launcher.webcamRotation = (launcher.webcamRotation + 90) % 360
        }
        MenuItem {
            text: launcher.webcamFlipHorizontal ? "Flip · On" : "Flip"
            onTriggered: launcher.webcamFlipHorizontal = !launcher.webcamFlipHorizontal
        }
        MenuSeparator {}
        MenuItem {
            text: "720p"
            checkable: true
            checked: launcher.webcamHeight === 720
            onTriggered: launcher.webcamHeight = 720
        }
        MenuItem {
            text: "1080p"
            checkable: true
            checked: launcher.webcamHeight === 1080
            onTriggered: launcher.webcamHeight = 1080
        }
    }

    Popup {
        id: settingsPopover
        parent: window.contentItem
        x: window.width - width - 32
        y: 76
        width: 310
        height: settingsColumn.implicitHeight + 32
        modal: false
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        padding: 16
        z: 20
        background: Rectangle {
            radius: 12
            color: theme.lighterBackground
            border.width: 1
            border.color: Qt.alpha(theme.foreground, 0.16)
        }
        contentItem: ColumnLayout {
            id: settingsColumn
            spacing: 10
            Label {
                text: "Capture settings"
                font.pixelSize: 15
                font.weight: Font.DemiBold
                color: theme.foreground
            }
            Label {
                Layout.fillWidth: true
                text: "Defaults used for each new recording."
                wrapMode: Text.WordWrap
                font.pixelSize: 11
                color: window.muted
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: window.subtleBorder }
            Switch {
                Layout.fillWidth: true
                text: "System audio"
                font.pixelSize: 12
                checked: launcher.systemAudio
                onToggled: launcher.systemAudio = checked
            }
            Switch {
                Layout.fillWidth: true
                text: "Microphone"
                font.pixelSize: 12
                checked: launcher.microphone
                onToggled: launcher.microphone = checked
            }
            Switch {
                Layout.fillWidth: true
                text: "Webcam"
                font.pixelSize: 12
                enabled: launcher.webcam || launcher.webcamDevices.length > 0
                checked: launcher.webcam
                onToggled: launcher.webcam = checked
            }
            RowLayout {
                Layout.fillWidth: true
                enabled: launcher.webcam
                Label {
                    text: "Webcam quality"
                    font.pixelSize: 12
                    color: enabled ? theme.foreground : window.muted
                }
                Item { Layout.fillWidth: true }
                Button {
                    Layout.preferredWidth: 76
                    Layout.preferredHeight: 30
                    text: launcher.webcamHeight + "p"
                    font.pixelSize: 11
                    onClicked: launcher.webcamHeight = launcher.webcamHeight === 720 ? 1080 : 720
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
        title: "Delete recording?"
        standardButtons: Dialog.NoButton
        contentItem: ColumnLayout {
            spacing: 16
            Label {
                Layout.fillWidth: true
                text: "“" + window.pendingDeleteName + "” will be permanently deleted."
                wrapMode: Text.WordWrap
                font.pixelSize: 12
                color: window.muted
            }
            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button { text: "Cancel"; onClicked: deleteDialog.close() }
                Button {
                    text: "Delete"
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
        radius: 8
        visible: window.errorMessage !== ""
        color: Qt.alpha(theme.foreground, 0.92)
        z: 40
        Label {
            id: errorLabel
            anchors.centerIn: parent
            text: window.errorMessage
            color: theme.background
            font.pixelSize: 12
            elide: Text.ElideRight
            width: parent.width - 24
            horizontalAlignment: Text.AlignHCenter
        }
    }

    Shortcut {
        sequence: "Ctrl+R"
        enabled: !launcher.recording && !deleteDialog.visible
        onActivated: window.beginRecording()
    }
    Shortcut {
        sequence: "Return"
        enabled: !launcher.recording && !deleteDialog.visible
        onActivated: window.beginRecording()
    }
    Shortcut {
        sequence: "Enter"
        enabled: !launcher.recording && !deleteDialog.visible
        onActivated: window.beginRecording()
    }

    FolderDialog {
        id: folderDialog
        title: "Open an omarecord bundle"
        onAccepted: launcher.openBundle(selectedFolder)
    }
}
