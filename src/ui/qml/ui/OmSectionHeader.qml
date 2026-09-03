import QtQuick

Text {
    textFormat: Text.PlainText
    color: theme.textMuted
    font.family: theme.fontFamily
    font.pixelSize: theme.font.caption
    font.bold: true
    font.capitalization: Font.AllUppercase
    font.letterSpacing: 1.2
    topPadding: Math.ceil(font.pixelSize * .15)
}
