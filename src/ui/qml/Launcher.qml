import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Effects
import QtMultimedia
import Omareel.Ui

ApplicationWindow {
    id: window
    visible: true
    width: 380
    height: desiredHeight
    minimumWidth: 340
    maximumWidth: 380
    minimumHeight: desiredHeight - 40
    maximumHeight: desiredHeight
    title: "Omareel"
    color: theme.surface
    font.family: theme.fontFamily
    font.pixelSize: theme.font.body

    readonly property int desiredHeight: launcher.webcam && !showingRecording ? 580 : 460
    readonly property string iconRoot: "qrc:/omareel/assets/icons/" + "luc" + "ide/"
    readonly property bool cameraQuarterTurn: launcher.webcamRotation === 90
                                               || launcher.webcamRotation === 270
    property bool screenshotRecording: false
    property string errorMessage: ""
    readonly property bool showingRecording: launcher.recording || screenshotRecording
    readonly property bool busy: showingRecording || launcher.startingRecording

    Behavior on height { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }

    function beginRecording() {
        if (!launcher.recording)
            launcher.record()
    }

    function prepareScreenshot(view) {
        if (view === "webcam-menu") webcamMenu.popup(webcamRow, 28, webcamRow.height + 4)
        else if (view === "webcam-on") launcher.webcam = true
        else if (view === "recording") screenshotRecording = true
        else if (view === "settings") settingsPopover.open()
    }

    component LauncherMenuItem: MenuItem {
        id: menuItem
        implicitHeight: 32
        leftPadding: 28
        rightPadding: 8
        indicator: Item {
            x: 8
            anchors.verticalCenter: parent.verticalCenter
            width: 14; height: 14
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
            color: menuItem.enabled ? theme.foreground : theme.textFaint
            font.pixelSize: theme.font.bodySmall
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        background: Rectangle {
            radius: theme.radius
            color: menuItem.highlighted || menuItem.checked ? theme.menuSelectedBackground : "transparent"
        }
    }

    component CaptureRow: Button {
        id: row
        property string iconFile: ""
        property bool selected: false
        property bool hasMenu: false
        signal menuRequested()
        Layout.fillWidth: true
        Layout.preferredHeight: 28
        enabled: !window.busy
        hoverEnabled: true
        leftPadding: 8; rightPadding: 8; topPadding: 0; bottomPadding: 0
        contentItem: RowLayout {
            spacing: 8
            IconImage {
                Layout.preferredWidth: 14; Layout.preferredHeight: 14
                source: window.iconRoot + row.iconFile
                sourceSize: Qt.size(14, 14)
                color: row.selected ? theme.foreground : theme.textMuted
            }
            Label {
                Layout.fillWidth: true
                text: row.text
                color: row.selected ? theme.foreground : theme.textMuted
                font.pixelSize: theme.font.bodySmall
                font.weight: Font.Normal
                verticalAlignment: Text.AlignVCenter
            }
            Rectangle {
                width: 7; height: 7; radius: 4
                color: row.selected ? theme.accent : theme.textFaint
            }
            IconImage {
                visible: row.hasMenu
                Layout.preferredWidth: 14; Layout.preferredHeight: 14
                source: window.iconRoot + "chevron-down.svg"
                sourceSize: Qt.size(14, 14)
                color: row.selected ? theme.foreground : theme.textMuted
            }
        }
        background: Rectangle {
            radius: theme.radius
            color: row.down ? theme.pressedFill
                 : row.hovered || row.visualFocus ? theme.hoverFill : theme.normalFill
            border.width: 1
            border.color: theme.normalBorder
        }
        MouseArea {
            visible: row.hasMenu
            anchors.top: parent.top; anchors.right: parent.right; anchors.bottom: parent.bottom
            width: 36
            cursorShape: Qt.PointingHandCursor
            onClicked: row.menuRequested()
        }
    }

    Camera {
        id: previewCamera
        cameraDevice: launcher.webcamCameraDevice
        active: launcher.webcam && launcher.webcamPreviewAvailable && !window.busy
    }
    CaptureSession { camera: previewCamera; videoOutput: cameraSource }

    Connections {
        target: launcher
        function onRecordingStarting() { previewCamera.stop() }
        function onErrorOccurred(message) {
            window.errorMessage = message
            errorTimer.restart()
        }
    }
    Timer { id: errorTimer; interval: 5000; onTriggered: window.errorMessage = "" }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 24
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.minimumWidth: window.width - 48
            Layout.maximumWidth: window.width - 48
            Layout.preferredHeight: 44
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                IconImage {
                    id: logoReel
                    Layout.preferredWidth: 20; Layout.preferredHeight: 20
                    source: "qrc:/omareel/assets/icons/reel.svg"
                    sourceSize: Qt.size(20, 20)
                    color: theme.foreground
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                Label {
                    text: "omareel"
                    color: theme.foreground
                    font.pixelSize: theme.font.heading
                    font.weight: Font.DemiBold
                }
                Label {
                    text: "Screen recordings that look great"
                    color: theme.textMuted
                    font.pixelSize: 13
                }
                }
            }
            OmIconButton {
                id: settingsButton
                icon.source: window.iconRoot + "settings-2.svg"
                icon.color: theme.foreground
                tooltipText: "Settings"
                Accessible.name: "Settings"
                onClicked: settingsPopover.visible ? settingsPopover.close() : settingsPopover.open()
            }
        }

        Item { Layout.fillHeight: true; Layout.minimumHeight: 8; Layout.maximumHeight: 28 }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 168

            Button {
                id: recordControl
                visible: !window.showingRecording
                enabled: !launcher.startingRecording
                anchors.centerIn: parent
                width: 236; height: 150
                hoverEnabled: true
                Accessible.name: "Record"
                onClicked: window.beginRecording()
                background: Item {}
                HoverHandler { cursorShape: Qt.PointingHandCursor }
                contentItem: ColumnLayout {
                    spacing: 8
                    Item {
                        Layout.alignment: Qt.AlignHCenter
                        width: 72; height: 72
                        property real entranceScale: 1
                        scale: recordControl.down ? .96 : entranceScale
                        y: recordControl.hovered && !recordControl.down ? -2 : 0
                        Behavior on y { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
                        SequentialAnimation on entranceScale {
                            running: true
                            NumberAnimation { from: 1; to: 1.06; duration: 150; easing.type: Easing.OutCubic }
                            NumberAnimation { from: 1.06; to: 1; duration: 150; easing.type: Easing.InCubic }
                        }
                        Rectangle {
                            anchors.fill: parent
                            radius: 36
                            color: "transparent"
                            border.width: 3
                            border.color: Qt.alpha(theme.record, recordControl.hovered ? .70 : .35)
                            Behavior on border.color { ColorAnimation { duration: 120 } }
                        }
                        Rectangle {
                            id: recordDisc
                            anchors.centerIn: parent
                            width: 64; height: 64; radius: 32
                            color: recordControl.down ? Qt.darker(theme.record, 1.08) : theme.record
                            SequentialAnimation on opacity {
                                running: launcher.startingRecording
                                loops: Animation.Infinite
                                NumberAnimation { from: 1; to: .38; duration: 480; easing.type: Easing.InOutSine }
                                NumberAnimation { from: .38; to: 1; duration: 480; easing.type: Easing.InOutSine }
                            }
                            layer.enabled: recordControl.hovered && !recordControl.down
                            layer.effect: MultiEffect {
                                shadowEnabled: true
                                shadowColor: Qt.rgba(0, 0, 0, .30)
                                shadowOpacity: .30
                                shadowBlur: .25
                                blurMax: 32
                                shadowVerticalOffset: 2
                                autoPaddingEnabled: true
                            }
                            Rectangle {
                                anchors.centerIn: parent
                                width: 22; height: 22; radius: 11
                                color: "white"
                            }
                        }
                    }
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        text: launcher.startingRecording ? "Starting…" : "Record"
                        color: theme.foreground
                        font.pixelSize: theme.font.title
                        font.bold: true
                    }
                    RowLayout {
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 6
                        Label {
                            text: "Area · Window · Screen"
                            color: theme.textMuted
                            font.pixelSize: theme.font.caption
                        }
                        Rectangle {
                            width: shortcutLabel.implicitWidth + 8; height: 18
                            radius: Math.min(4, theme.radius)
                            color: theme.normalFill
                            border.width: 1; border.color: theme.normalBorder
                            Label {
                                id: shortcutLabel
                                anchors.centerIn: parent
                                text: "Ctrl R"
                                color: theme.textMuted
                                font.family: theme.monoFamily
                                font.pixelSize: theme.font.caption
                            }
                        }
                    }
                }
            }

            ColumnLayout {
                visible: window.showingRecording
                anchors.centerIn: parent
                spacing: 12
                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 10
                    Rectangle {
                        width: 16; height: 16; radius: 8
                        color: theme.record
                        SequentialAnimation on opacity {
                            loops: Animation.Infinite
                            NumberAnimation { from: 1; to: .35; duration: 700 }
                            NumberAnimation { from: .35; to: 1; duration: 700 }
                        }
                    }
                    Label {
                        text: window.screenshotRecording ? "00:12" : launcher.recordingElapsed
                        color: theme.foreground
                        font.family: theme.monoFamily
                        font.pixelSize: theme.font.heading
                        font.weight: Font.DemiBold
                    }
                }
                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 8
                    OmButton {
                        width: 104; height: 32
                        text: "Stop"
                        primary: true; bordered: true
                        onClicked: launcher.stopRecording()
                    }
                    OmButton {
                        id: discardButton
                        width: 104; height: 32
                        property bool armed: false
                        text: armed ? "Discard?" : "Discard"
                        destructive: armed; bordered: true
                        onClicked: {
                            if (armed) launcher.cancelRecording()
                            else { armed = true; discardTimer.restart() }
                        }
                        Timer { id: discardTimer; interval: 3000; onTriggered: discardButton.armed = false }
                    }
                }
            }
        }

        Item { Layout.preferredHeight: 8 }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: launcher.webcam && !window.showingRecording ? 136 : 0
            opacity: launcher.webcam && !window.showingRecording ? 1 : 0
            clip: true
            Behavior on Layout.preferredHeight { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
            Behavior on opacity { NumberAnimation { duration: 120 } }

            ColumnLayout {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: 0
                Item {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: 160; Layout.preferredHeight: 96
                    Rectangle {
                        visible: launcher.webcamPreviewAvailable
                        anchors.centerIn: parent
                        width: 96; height: 96; radius: 48
                        color: theme.normalFill
                        clip: true
                        VideoOutput {
                            id: cameraSource
                            anchors.centerIn: parent
                            width: window.cameraQuarterTurn ? parent.height : parent.width
                            height: window.cameraQuarterTurn ? parent.width : parent.height
                            fillMode: VideoOutput.PreserveAspectCrop
                            rotation: launcher.webcamRotation
                            transform: Scale {
                                origin.x: cameraSource.width / 2
                                origin.y: cameraSource.height / 2
                                xScale: launcher.webcamFlipHorizontal ? -1 : 1
                            }
                        }
                    }
                    Label {
                        visible: !launcher.webcamPreviewAvailable
                        anchors.centerIn: parent
                        text: "Starting camera…"
                        color: theme.textMuted
                        font.pixelSize: theme.font.caption
                    }
                }
                Item { Layout.preferredHeight: 8 }
                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 8
                    OmIconButton {
                        width: 28; height: 28
                        bordered: true
                        icon.source: window.iconRoot + "rotate-cw.svg"
                        icon.color: theme.foreground
                        tooltipText: "Rotate camera"
                        onClicked: launcher.webcamRotation = (launcher.webcamRotation + 90) % 360
                    }
                    OmIconButton {
                        width: 28; height: 28
                        bordered: true
                        selected: launcher.webcamFlipHorizontal
                        icon.source: window.iconRoot + "flip-horizontal-2.svg"
                        icon.color: theme.foreground
                        tooltipText: "Flip camera"
                        onClicked: launcher.webcamFlipHorizontal = !launcher.webcamFlipHorizontal
                    }
                }
                Item { Layout.preferredHeight: 4 }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 6
            CaptureRow {
                text: "System audio"; iconFile: "volume-2.svg"; selected: launcher.systemAudio
                onClicked: launcher.systemAudio = !launcher.systemAudio
            }
            CaptureRow {
                id: microphoneRow
                text: "Microphone"; iconFile: "mic.svg"; selected: launcher.microphone; hasMenu: true
                onClicked: launcher.microphone = !launcher.microphone
                onMenuRequested: microphoneMenu.popup(microphoneRow, 28, microphoneRow.height + 4)
            }
            CaptureRow {
                id: webcamRow
                text: "Webcam"; iconFile: "video.svg"; selected: launcher.webcam; hasMenu: true
                enabled: !window.busy && (launcher.webcam || launcher.webcamDevices.length > 0)
                onClicked: launcher.webcam = !launcher.webcam
                onMenuRequested: webcamMenu.popup(webcamRow, 28, webcamRow.height + 4)
            }
        }

        Item { Layout.fillHeight: true; Layout.minimumHeight: 8; Layout.maximumHeight: 28 }

        OmButton {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            text: "Open recording…"
            icon.source: window.iconRoot + "folder-open.svg"
            bordered: true
            onClicked: folderDialog.open()
        }
        Label {
            Layout.fillWidth: true
            Layout.preferredHeight: 20
            text: "Recordings are saved to ~/Videos/omareel"
            color: folderMouse.containsMouse ? theme.foreground : theme.textMuted
            font.pixelSize: theme.font.caption
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            MouseArea {
                id: folderMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: launcher.showRecordingsFolder()
            }
        }
    }

    Menu {
        id: microphoneMenu
        width: 280; padding: 4
        background: Rectangle { radius: theme.radius; color: theme.menuBackground; border.width: 1; border.color: theme.popupBorder }
        Instantiator {
            model: launcher.audioDevices
            delegate: LauncherMenuItem {
                required property var modelData
                text: modelData.text; checkable: true
                checked: modelData.value === launcher.microphoneDevice
                onTriggered: { launcher.microphoneDevice = modelData.value; launcher.microphone = true }
            }
            onObjectAdded: function(index, object) { microphoneMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { microphoneMenu.removeItem(object) }
        }
    }

    Menu {
        id: webcamMenu
        width: 300; padding: 4
        background: Rectangle { radius: theme.radius; color: theme.menuBackground; border.width: 1; border.color: theme.popupBorder }
        Instantiator {
            model: launcher.webcamDevices
            delegate: LauncherMenuItem {
                required property var modelData
                text: modelData.text; checkable: true
                checked: modelData.value === launcher.webcamDevice
                onTriggered: { launcher.webcamDevice = modelData.value; launcher.webcam = true }
            }
            onObjectAdded: function(index, object) { webcamMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { webcamMenu.removeItem(object) }
        }
    }

    Popup {
        id: settingsPopover
        parent: window.contentItem
        x: window.width - width - 24
        y: 60
        width: 292
        padding: 16
        modal: false; focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside
        background: OmPopupCard {}
        contentItem: ColumnLayout {
            spacing: 8
            Label { text: "Capture settings"; color: theme.foreground; font.pixelSize: theme.font.title; font.bold: true }
            Label { text: "Defaults for new recordings"; color: theme.textMuted; font.pixelSize: theme.font.caption }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
            OmToggle { Layout.fillWidth: true; text: "Show camera self-view"; checked: launcher.selfViewEnabled; onToggled: launcher.selfViewEnabled = checked }
            OmToggle { Layout.fillWidth: true; text: "Countdown before recording"; checked: launcher.countdownBeforeRecording; onToggled: launcher.countdownBeforeRecording = checked }
            Label { text: "Self-view size"; color: theme.textMuted; font.pixelSize: theme.font.caption }
            OmButtonGroup {
                Layout.fillWidth: true
                options: ["S", "M", "L"]
                value: launcher.selfViewSize
                onChanged: value => launcher.selfViewSize = value
            }
            Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 1; color: theme.hairline }
            Label {
                Layout.fillWidth: true
                text: "Version " + Qt.application.version + " · Made for Omarchy"
                color: theme.textMuted
                font.pixelSize: theme.font.caption
                horizontalAlignment: Text.AlignHCenter
            }
        }
    }

    NumberAnimation {
        id: reelSpin
        target: logoReel
        property: "rotation"
        from: 0; to: 360; duration: 500
        easing.type: Easing.InOutCubic
    }

    Rectangle {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom; anchors.bottomMargin: 12
        width: Math.min(window.width - 32, errorLabel.implicitWidth + 24)
        height: 36; radius: theme.radius
        visible: window.errorMessage !== ""
        color: theme.foreground
        z: 40
        Label {
            id: errorLabel
            anchors.centerIn: parent
            width: parent.width - 16
            text: window.errorMessage
            color: theme.surface
            font.pixelSize: theme.font.caption
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
        }
    }

    Shortcut { sequence: "Ctrl+R"; enabled: !window.busy; onActivated: window.beginRecording() }
    Shortcut {
        sequence: "Ctrl+Shift+R"
        onActivated: { reelSpin.stop(); logoReel.rotation = 0; reelSpin.start() }
    }
    Shortcut { sequence: "Return"; enabled: !window.busy && !settingsPopover.visible; onActivated: window.beginRecording() }
    Shortcut { sequence: "Enter"; enabled: !window.busy && !settingsPopover.visible; onActivated: window.beginRecording() }

    FolderDialog {
        id: folderDialog
        title: "Open an omareel bundle"
        onAccepted: launcher.openBundle(selectedFolder)
    }
}
