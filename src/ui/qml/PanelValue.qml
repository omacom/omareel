import QtQuick

Text {
    textFormat: Text.PlainText
    property bool live: false
    color: live ? theme.accent : theme.textMuted
    font.family: theme.monoFamily
    font.pixelSize: theme.type.small
    horizontalAlignment: Text.AlignRight
    Behavior on color { ColorAnimation { duration: 140 } }
}
