import QtQuick
import QtQuick.Controls
import QtQuick.Controls.impl
import QtQuick.Dialogs
import QtQuick.Layouts
import QtQuick.Effects
import Omareel.Ui

ApplicationWindow {
    id: window
    visible: true
    width: 380
    height: desiredHeight
    // A fixed width is how the launcher asks to float: Hyprland (like Sway) floats a toplevel
    // whose minimum and maximum size match in either dimension, so no window rule is needed.
    minimumWidth: 380
    maximumWidth: 380
    // Hyprland ignores a floating window's own resize requests and only clamps it into
    // [minimum, maximum], so the height is pinned to what the content needs.
    minimumHeight: desiredHeight
    maximumHeight: desiredHeight
    title: "Omareel"
    color: theme.surface
    font.family: theme.fontFamily
    font.pixelSize: theme.type.body

    readonly property int desiredHeight: Math.ceil(content.implicitHeight + content.anchors.topMargin
                                                   + content.anchors.bottomMargin)
    readonly property string iconRoot: "qrc:/omareel/assets/icons/" + "luc" + "ide/"
    property bool screenshotRecording: false
    property string errorMessage: ""
    readonly property bool showingRecording: launcher.recording || screenshotRecording
    readonly property bool busy: showingRecording || launcher.startingRecording

    function beginRecording() {
        if (!launcher.recording)
            launcher.record()
    }

    // Device menus drop below their row, or open upward when the window has no room below.
    function openDeviceMenu(menu, row) {
        const below = row.mapToItem(window.contentItem, 0, row.height - 6).y
        const fits = below + menu.implicitHeight <= window.height - menu.margins
        menu.transformOrigin = fits ? Popup.Top : Popup.Bottom
        menu.popup(row, 0, fits ? row.height - 6 : 6 - menu.implicitHeight)
    }
    function prepareScreenshot(view) {
        if (view === "webcam-menu") openDeviceMenu(webcamMenu, webcamRow)
        else if (view === "webcam-on") launcher.webcam = true
        else if (view === "recording") screenshotRecording = true
        else if (view === "settings") settingsPopover.open()
    }
    function deviceName(devices, value, fallback) {
        for (let i = 0; i < devices.length; ++i)
            if (devices[i].value === value) return devices[i].text
        return devices.length > 0 ? devices[0].text : fallback
    }

    // One row of the sources card: icon, name, optional device line that opens a menu, switch.
    component SourceRow: Item {
        id: row
        property string iconFile: ""
        property string iconOffFile: iconFile
        property string title: ""
        property string device: ""
        property bool checked: false
        property bool hasMenu: false
        // A level meter beside the switch, 0 to 1; below 0 hides it.
        property real level: -1
        signal toggled()
        signal menuRequested()
        Layout.fillWidth: true
        implicitHeight: 52
        opacity: enabled ? 1 : .5
        Rectangle {
            anchors.fill: parent
            anchors.margins: 2
            radius: Math.max(0, theme.cardRadius - 2)
            color: rowMouse.containsMouse ? theme.fill : "transparent"
            Behavior on color { ColorAnimation { duration: 140 } }
        }
        MouseArea {
            id: rowMouse
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: row.toggled()
        }
        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 14
            anchors.rightMargin: 14
            spacing: 12
            Rectangle {
                Layout.preferredWidth: 30; Layout.preferredHeight: 30
                radius: theme.controlRadius
                color: row.checked ? Qt.alpha(theme.accent, .16) : theme.fill
                Behavior on color { ColorAnimation { duration: 180 } }
                IconImage {
                    anchors.centerIn: parent
                    width: 16; height: 16
                    source: window.iconRoot + (row.checked ? row.iconFile : row.iconOffFile)
                    sourceSize: Qt.size(16, 16)
                    color: row.checked ? theme.accent : theme.textMuted
                    Behavior on color { ColorAnimation { duration: 180 } }
                }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                Label {
                    Layout.fillWidth: true
                    textFormat: Text.PlainText
                    text: row.title
                    color: row.checked ? theme.textStrong : theme.foreground
                    font.pixelSize: theme.type.body
                    font.weight: Font.Medium
                }
                Item {
                    visible: row.device !== ""
                    Layout.fillWidth: true
                    implicitHeight: deviceRow.implicitHeight
                    Row {
                        id: deviceRow
                        spacing: 3
                        width: Math.min(implicitWidth, parent.width)
                        Label {
                            width: Math.min(implicitWidth, deviceRow.parent.width - 16)
                            textFormat: Text.PlainText
                            text: row.device
                            elide: Text.ElideRight
                            color: deviceMouse.containsMouse ? theme.foreground : theme.textMuted
                            font.pixelSize: theme.type.caption
                        }
                        IconImage {
                            visible: row.hasMenu
                            anchors.verticalCenter: parent.verticalCenter
                            width: 12; height: 12
                            source: window.iconRoot + "chevron-down.svg"
                            sourceSize: Qt.size(12, 12)
                            color: deviceMouse.containsMouse ? theme.foreground : theme.textMuted
                        }
                    }
                    MouseArea {
                        id: deviceMouse
                        anchors.fill: deviceRow
                        anchors.margins: -4
                        enabled: row.hasMenu
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: row.menuRequested()
                    }
                }
            }
            Rectangle {
                visible: row.level >= 0
                Layout.preferredWidth: 28; Layout.preferredHeight: 4
                radius: theme.rounded ? 2 : 0
                color: theme.fill
                Rectangle {
                    width: parent.width * Math.min(1, Math.max(0, row.level))
                    height: parent.height
                    radius: parent.radius
                    color: theme.accent
                }
            }
            OmToggle {
                Layout.preferredWidth: 34
                checked: row.checked
                focusPolicy: Qt.NoFocus
                Accessible.name: row.title
                // The row owns the state; the switch only reflects it.
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: row.toggled() }
            }
        }
    }

    Connections {
        target: launcher
        function onErrorOccurred(message) {
            window.errorMessage = message
            errorTimer.restart()
        }
    }
    Timer { id: errorTimer; interval: 5000; onTriggered: window.errorMessage = "" }

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        anchors.topMargin: 18
        anchors.bottomMargin: 16
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            IconImage {
                id: logoReel
                Layout.preferredWidth: 18; Layout.preferredHeight: 18
                source: "qrc:/omareel/assets/icons/reel.svg"
                sourceSize: Qt.size(18, 18)
                color: theme.textStrong
            }
            Label {
                Layout.fillWidth: true
                text: "omareel"
                color: theme.textStrong
                font.family: theme.monoFamily
                font.pixelSize: theme.type.body
                font.weight: Font.Bold
            }
            OmIconButton {
                id: settingsButton
                icon.source: window.iconRoot + "settings-2.svg"
                selected: settingsPopover.visible
                tooltipText: "Capture settings"
                Accessible.name: "Settings"
                onClicked: settingsPopover.visible ? settingsPopover.close() : settingsPopover.open()
            }
        }

        Item { Layout.fillHeight: true; Layout.preferredHeight: 18 }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 176

            Button {
                id: recordControl
                visible: !window.showingRecording
                enabled: !launcher.startingRecording
                anchors.centerIn: parent
                width: 260; height: 172
                hoverEnabled: true
                Accessible.name: "Record"
                onClicked: window.beginRecording()
                background: Item {}
                HoverHandler { cursorShape: Qt.PointingHandCursor }
                contentItem: ColumnLayout {
                    spacing: 0
                    Item {
                        id: recordButton
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: 96; Layout.preferredHeight: 96
                        readonly property bool hot: recordControl.hovered && !recordControl.down
                        property real entrance: .85
                        scale: (recordControl.down ? .94 : 1) * entrance
                        Behavior on scale { NumberAnimation { duration: recordControl.down ? 80 : 320; easing.type: Easing.OutBack; easing.overshoot: 2 } }
                        Component.onCompleted: entrance = 1
                        // Soft glow that blooms on hover.
                        Rectangle {
                            id: glowShape
                            anchors.centerIn: parent
                            width: 64; height: 64; radius: 32
                            color: theme.record
                            visible: false
                            layer.enabled: true
                        }
                        MultiEffect {
                            anchors.fill: glowShape
                            source: glowShape
                            blurEnabled: true
                            blur: 1
                            blurMax: 32
                            autoPaddingEnabled: true
                            opacity: recordButton.hot ? .55 : launcher.startingRecording ? .4 : .18
                            Behavior on opacity { NumberAnimation { duration: 260 } }
                        }
                        Rectangle {
                            anchors.centerIn: parent
                            width: recordButton.hot ? 84 : 78; height: width
                            radius: width / 2
                            color: "transparent"
                            border.width: 2
                            border.color: Qt.alpha(theme.record, recordButton.hot ? .55 : .28)
                            Behavior on width { NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }
                            Behavior on border.color { ColorAnimation { duration: 200 } }
                        }
                        Rectangle {
                            id: recordDisc
                            anchors.centerIn: parent
                            width: 64; height: 64; radius: 32
                            color: recordControl.down ? Qt.darker(theme.record, 1.1) : theme.record
                            Rectangle {
                                anchors.fill: parent
                                radius: parent.radius
                                gradient: Gradient {
                                    GradientStop { position: 0; color: Qt.rgba(1, 1, 1, .22) }
                                    GradientStop { position: .55; color: Qt.rgba(1, 1, 1, 0) }
                                }
                            }
                            // The dot becomes a stop square while the recorder spins up.
                            Rectangle {
                                anchors.centerIn: parent
                                width: launcher.startingRecording ? 20 : recordButton.hot ? 24 : 22
                                height: width
                                radius: launcher.startingRecording ? 4 : width / 2
                                color: "white"
                                Behavior on width { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
                                Behavior on radius { NumberAnimation { duration: 260; easing.type: Easing.OutCubic } }
                            }
                            SequentialAnimation on opacity {
                                running: launcher.startingRecording
                                loops: Animation.Infinite
                                NumberAnimation { from: 1; to: .55; duration: 480; easing.type: Easing.InOutSine }
                                NumberAnimation { from: .55; to: 1; duration: 480; easing.type: Easing.InOutSine }
                            }
                        }
                    }
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.topMargin: 6
                        textFormat: Text.PlainText
                        text: launcher.startingRecording ? "Starting…" : "Start recording"
                        color: theme.textStrong
                        font.pixelSize: theme.type.title
                        font.weight: Font.DemiBold
                    }
                    Label {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.topMargin: 3
                        text: "Drag an area, click a window, or the desktop"
                        color: theme.textMuted
                        font.pixelSize: theme.type.small
                    }
                    Row {
                        Layout.alignment: Qt.AlignHCenter
                        Layout.topMargin: 10
                        spacing: 3
                        OmKeycap { text: "Ctrl"; subtle: !recordButton.hot }
                        OmKeycap { text: "R"; subtle: !recordButton.hot }
                    }
                }
            }

            ColumnLayout {
                visible: window.showingRecording
                anchors.centerIn: parent
                spacing: 14
                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 8
                    Rectangle {
                        width: 8; height: 8; radius: 4
                        color: theme.record
                        SequentialAnimation on opacity {
                            running: window.showingRecording
                            loops: Animation.Infinite
                            NumberAnimation { from: 1; to: .25; duration: 800; easing.type: Easing.InOutSine }
                            NumberAnimation { from: .25; to: 1; duration: 800; easing.type: Easing.InOutSine }
                        }
                    }
                    Label {
                        text: "Recording"
                        color: theme.textMuted
                        font.pixelSize: theme.type.small
                        font.weight: Font.Medium
                    }
                }
                Label {
                    Layout.alignment: Qt.AlignHCenter
                    textFormat: Text.PlainText
                    text: window.screenshotRecording ? "00:12" : launcher.recordingElapsed
                    color: theme.textStrong
                    font.family: theme.monoFamily
                    font.pixelSize: theme.type.hero + 8
                    font.weight: Font.Medium
                }
                RowLayout {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 4
                    spacing: 8
                    OmButton {
                        Layout.preferredWidth: 112; Layout.preferredHeight: 34
                        text: launcher.stoppingRecording ? "Saving…" : "Stop"
                        enabled: !launcher.stoppingRecording
                        primary: true; destructive: true
                        leadingDot: false
                        icon.source: window.iconRoot + "square.svg"
                        onClicked: launcher.stopRecording()
                    }
                    OmButton {
                        id: discardButton
                        Layout.preferredWidth: 112; Layout.preferredHeight: 34
                        enabled: !launcher.stoppingRecording
                        property bool armed: false
                        text: armed ? "Discard?" : "Discard"
                        icon.source: window.iconRoot + "trash-2.svg"
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

        Item { Layout.fillHeight: true; Layout.preferredHeight: 22 }

        Label {
            Layout.fillWidth: true
            Layout.bottomMargin: 8
            visible: launcher.captureExclusionNote.length > 0
            textFormat: Text.PlainText
            text: launcher.captureExclusionNote
            color: theme.textMuted
            font.pixelSize: theme.type.caption
            wrapMode: Text.WordWrap
            HoverHandler { id: noteHover }
            OmToolTip {
                visible: noteHover.hovered && launcher.captureExclusionReason !== ""
                text: launcher.captureExclusionReason
                placement: "above"
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: sources.implicitHeight + 4
            radius: theme.cardRadius
            color: theme.card
            border.width: 1
            border.color: theme.stroke
            opacity: window.showingRecording ? .5 : 1
            Behavior on opacity { NumberAnimation { duration: 200 } }
            ColumnLayout {
                id: sources
                anchors.fill: parent
                anchors.margins: 2
                spacing: 0
                SourceRow {
                    title: "System audio"
                    iconFile: "volume-2.svg"; iconOffFile: "volume-x.svg"
                    checked: launcher.systemAudio
                    enabled: !window.busy
                    onToggled: launcher.systemAudio = !launcher.systemAudio
                }
                Rectangle { Layout.fillWidth: true; Layout.leftMargin: 56; Layout.preferredHeight: 1; color: theme.separator }
                SourceRow {
                    id: microphoneRow
                    title: "Microphone"
                    iconFile: "mic.svg"; iconOffFile: "mic-off.svg"
                    device: window.deviceName(launcher.audioDevices, launcher.microphoneDevice, "Default input")
                    hasMenu: launcher.audioDevices.length > 0
                    checked: launcher.microphone
                    enabled: !window.busy
                    level: launcher.microphone && !window.busy ? launcher.microphoneLevel : -1
                    onToggled: launcher.microphone = !launcher.microphone
                    onMenuRequested: window.openDeviceMenu(microphoneMenu, microphoneRow)
                }
                Rectangle { Layout.fillWidth: true; Layout.leftMargin: 56; Layout.preferredHeight: 1; color: theme.separator }
                SourceRow {
                    id: webcamRow
                    title: "Webcam"
                    iconFile: "video.svg"; iconOffFile: "video-off.svg"
                    device: launcher.webcamDevices.length > 0
                        ? window.deviceName(launcher.webcamDevices, launcher.webcamDevice, "Camera")
                        : "No camera found"
                    hasMenu: launcher.webcamDevices.length > 0
                    checked: launcher.webcam
                    enabled: !window.busy && (launcher.webcam || launcher.webcamDevices.length > 0)
                    onToggled: launcher.webcam = !launcher.webcam
                    onMenuRequested: window.openDeviceMenu(webcamMenu, webcamRow)
                }
                Item {
                    Layout.fillWidth: true
                    Layout.preferredHeight: launcher.webcam && !window.showingRecording ? 40 : 0
                    opacity: launcher.webcam && !window.showingRecording ? 1 : 0
                    clip: true
                    Behavior on opacity { NumberAnimation { duration: 160 } }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 56
                        anchors.rightMargin: 10
                        anchors.bottomMargin: 6
                        spacing: 4
                        Label {
                            Layout.fillWidth: true
                            textFormat: Text.PlainText
                            text: launcher.selfViewStatus
                            elide: Text.ElideRight
                            color: theme.textMuted
                            font.pixelSize: theme.type.caption
                        }
                        OmIconButton {
                            width: 26; height: 26
                            icon.width: 14; icon.height: 14
                            icon.source: window.iconRoot + "rotate-cw.svg"
                            tooltipText: "Rotate camera"
                            onClicked: launcher.webcamRotation = (launcher.webcamRotation + 90) % 360
                        }
                        OmIconButton {
                            width: 26; height: 26
                            icon.width: 14; icon.height: 14
                            selected: launcher.webcamFlipHorizontal
                            icon.source: window.iconRoot + "flip-horizontal-2.svg"
                            tooltipText: "Mirror camera"
                            onClicked: launcher.webcamFlipHorizontal = !launcher.webcamFlipHorizontal
                        }
                    }
                }
            }
        }

        Item { Layout.fillHeight: true; Layout.preferredHeight: 18 }

        RowLayout {
            Layout.fillWidth: true
            spacing: 6
            OmButton {
                text: "Open recording…"
                icon.source: window.iconRoot + "folder-open.svg"
                onClicked: folderDialog.open()
            }
            Item { Layout.fillWidth: true }
            Label {
                text: "~/Videos/omareel"
                color: folderMouse.containsMouse ? theme.accent : theme.textFaint
                font.family: theme.monoFamily
                font.pixelSize: theme.type.caption
                Behavior on color { ColorAnimation { duration: 140 } }
                MouseArea {
                    id: folderMouse
                    anchors.fill: parent
                    anchors.margins: -6
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: launcher.showRecordingsFolder()
                }
                OmToolTip { target: parent; visible: folderMouse.containsMouse; text: "Show recordings"; placement: "above" }
            }
        }
    }

    OmMenu {
        id: microphoneMenu
        width: microphoneRow.width
        Instantiator {
            model: launcher.audioDevices
            delegate: OmMenuItem {
                required property var modelData
                text: modelData.text; checkable: true
                checked: modelData.value === launcher.microphoneDevice
                onTriggered: { launcher.microphoneDevice = modelData.value; launcher.microphone = true }
            }
            onObjectAdded: function(index, object) { microphoneMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { microphoneMenu.removeItem(object) }
        }
    }

    OmMenu {
        id: webcamMenu
        width: webcamRow.width
        Instantiator {
            model: launcher.webcamDevices
            delegate: OmMenuItem {
                required property var modelData
                text: modelData.text; checkable: true
                checked: modelData.value === launcher.webcamDevice
                onTriggered: { launcher.webcamDevice = modelData.value; launcher.webcam = true }
            }
            onObjectAdded: function(index, object) { webcamMenu.insertItem(index, object) }
            onObjectRemoved: function(index, object) { webcamMenu.removeItem(object) }
        }
    }

    OmPopup {
        id: settingsPopover
        parent: window.contentItem
        x: window.width - width - 16
        y: 52
        width: 300
        padding: 16
        focus: true
        transformOrigin: Popup.TopRight
        contentItem: ColumnLayout {
            spacing: 10
            ColumnLayout {
                spacing: 2
                Label { text: "Capture settings"; color: theme.textStrong; font.pixelSize: theme.type.title; font.weight: Font.DemiBold }
                Label { text: "Defaults for new recordings"; color: theme.textMuted; font.pixelSize: theme.type.small }
            }
            OmSeparator { Layout.fillWidth: true }
            OmToggle { Layout.fillWidth: true; text: "Countdown"; description: "Three seconds to get ready"; checked: launcher.countdownBeforeRecording; onToggled: launcher.countdownBeforeRecording = checked }
            OmToggle { Layout.fillWidth: true; text: "Camera self-view"; description: "Floating preview of your webcam"; checked: launcher.selfViewEnabled; onToggled: launcher.selfViewEnabled = checked }
            OmSettingRow {
                label: "Self-view size"
                enabled: launcher.selfViewEnabled
                opacity: enabled ? 1 : .5
                OmButtonGroup {
                    Layout.preferredWidth: 120
                    options: ["S", "M", "L"]
                    value: launcher.selfViewSize
                    onChanged: value => launcher.selfViewSize = value
                }
            }
            OmSeparator { Layout.fillWidth: true }
            Label {
                Layout.fillWidth: true
                textFormat: Text.PlainText
                text: "omareel " + Qt.application.version + "  ·  made for Omarchy"
                color: theme.textFaint
                font.family: theme.monoFamily
                font.pixelSize: theme.type.caption
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
        anchors.bottom: parent.bottom
        anchors.bottomMargin: window.errorMessage !== "" ? 14 : -height
        width: Math.min(window.width - 32, errorLabel.implicitWidth + 28)
        height: 36; radius: theme.controlRadius
        opacity: window.errorMessage !== "" ? 1 : 0
        color: theme.textStrong
        z: 40
        Behavior on anchors.bottomMargin { NumberAnimation { duration: 260; easing.type: Easing.OutBack } }
        Behavior on opacity { NumberAnimation { duration: 160 } }
        Label {
            id: errorLabel
            anchors.centerIn: parent
            width: parent.width - 20
            textFormat: Text.PlainText
            text: window.errorMessage
            color: theme.surface
            font.pixelSize: theme.type.small
            font.weight: Font.Medium
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
