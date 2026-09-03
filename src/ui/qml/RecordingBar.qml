import QtQuick
import QtQuick.Controls
import QtQuick.Controls.Material
import QtQuick.Layouts

ApplicationWindow {
    id: window
    visible: false
    width: recordingBar.webcam ? 264 : 236
    height: 40
    color: "transparent"
    flags: Qt.FramelessWindowHint | Qt.Tool
    Material.theme: theme.dark ? Material.Dark : Material.Light
    Material.accent: theme.accent
    Material.background: theme.background
    Material.foreground: theme.foreground

    Rectangle {
        anchors.fill: parent
        radius: 20
        color: Qt.alpha(theme.background, .92)
        border.width: 1
        border.color: Qt.alpha(theme.foreground, .15)

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 7
            spacing: 8

            Rectangle {
                width: 9
                height: 9
                radius: 5
                color: "#ef4444"
                SequentialAnimation on opacity {
                    loops: Animation.Infinite
                    NumberAnimation { from: 1; to: .35; duration: 700; easing.type: Easing.InOutSine }
                    NumberAnimation { from: .35; to: 1; duration: 700; easing.type: Easing.InOutSine }
                }
            }

            Label {
                Layout.preferredWidth: 44
                text: recordingBar.elapsed
                color: theme.foreground
                font.family: "monospace"
                font.pixelSize: 12
                font.weight: Font.DemiBold
            }

            Image {
                visible: recordingBar.webcam
                Layout.preferredWidth: 16
                Layout.preferredHeight: 16
                sourceSize.width: 32
                sourceSize.height: 32
                source: "qrc:/omarecord/assets/icons/camera.svg"
                opacity: .82
            }

            Button {
                Layout.preferredWidth: 58
                Layout.preferredHeight: 28
                focusPolicy: Qt.NoFocus
                hoverEnabled: true
                font.pixelSize: 11
                font.weight: Font.DemiBold
                topInset: 0; bottomInset: 0
                onClicked: recordingBar.stop()
                contentItem: Label {
                    text: "Stop"
                    color: theme.accentForeground
                    font: parent.font
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                background: Rectangle {
                    radius: 14
                    color: parent.down ? Qt.darker(theme.accent, 1.15)
                                       : parent.hovered ? Qt.lighter(theme.accent, 1.08) : theme.accent
                }
            }

            Button {
                id: cancelButton
                Layout.fillWidth: true
                Layout.preferredHeight: 28
                text: armed ? "Discard?" : "Cancel"
                flat: true
                focusPolicy: Qt.NoFocus
                font.pixelSize: 10
                property bool armed: false
                topInset: 0; bottomInset: 0
                onClicked: {
                    if (armed) recordingBar.cancel()
                    else {
                        armed = true
                        disarm.restart()
                    }
                }
                Timer { id: disarm; interval: 3000; onTriggered: cancelButton.armed = false }
            }
        }
    }
}
