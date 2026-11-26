import QtQuick
import QtQuick.Controls

// The caption at the current moment: `caption` is {lines, opacity, position, size, style,
// active} from the editor's preview or the exporter's frame. Styled like the keystroke pills.
// "phrase" shows the lines; "highlight" lights the word being spoken (`active`, its index among
// the cue's words) in the theme's accent; "short" shows a few large words, the spoken one lit.
Item {
    id: root
    required property var caption
    required property var colors
    property real ref: 1
    readonly property real size: caption && caption.size ? caption.size : 1
    readonly property string style: caption && caption.style ? caption.style : "phrase"
    readonly property bool isShort: style === "short"
    readonly property bool atTop: caption && caption.position === "top"
    readonly property int active: caption && caption.active !== undefined && style !== "phrase" ? caption.active : -1
    readonly property var cueLines: caption && caption.lines ? caption.lines : []
    readonly property color pillColor: Qt.alpha(colors.background, .85)
    readonly property color textColor: colors.foreground
    readonly property color litColor: colors.accent !== undefined ? colors.accent : colors.foreground
    // Text size at 1080 high before `size`: captions::font_px, which ASS files use too.
    readonly property real fontPx: (isShort ? 68 * Math.max(.75, Math.min(1, width / Math.max(1, height))) : 36) * size * ref
    readonly property real padX: 18 * size * ref * (isShort ? 1.4 : 1)
    readonly property real padY: 10 * size * ref * (isShort ? 1.4 : 1)
    // The widest a line may be: lines longer than this (portrait frames, large sizes) wrap
    // inside the pill rather than spilling out of it.
    readonly property real textWidth: Math.max(0, width - 80 * ref - 2 * padX)
    anchors.fill: parent

    function escaped(text) {
        return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
    }

    // The cue's words as styled text, the lit one in the accent colour, lines joined by
    // `separator`. Words are counted across lines, as captions::Cue::words lists them.
    function styled(separator) {
        let index = 0
        const lit = root.active
        const accent = root.litColor.toString()
        return root.cueLines.map(line => line.split(" ").map(word => {
            const text = root.escaped(word)
            return index++ === lit ? "<font color=\"" + accent + "\">" + text + "</font>" : text
        }).join(" ")).join(separator)
    }

    Rectangle {
        id: box
        visible: root.cueLines.length > 0
        opacity: root.caption && root.caption.opacity !== undefined ? root.caption.opacity : 1
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.top: root.atTop ? parent.top : undefined
        anchors.bottom: root.atTop ? undefined : parent.bottom
        // Short captions sit inside the frame, clear of the edges (as their ASS file places them);
        // the others hug the top, or sit above the keystroke pills' corner.
        anchors.topMargin: root.isShort ? parent.height * .18 : 40 * root.ref
        anchors.bottomMargin: root.isShort ? parent.height * .22 : 110 * root.ref
        width: Math.min(parent.width - 80 * root.ref, lines.implicitWidth + 2 * root.padX)
        height: lines.implicitHeight + 2 * root.padY
        radius: (root.isShort ? 18 : 12) * root.size * root.ref
        color: root.pillColor
        border.width: root.ref
        border.color: Qt.alpha(root.textColor, .12)

        // The cue's own lines when each fits; otherwise all of it rewrapped to the width, so
        // a line too long for a portrait frame doesn't leave its last word alone.
        Label {
            id: lines
            objectName: "captionText"
            readonly property bool fits: natural.implicitWidth <= root.textWidth
            anchors.centerIn: parent
            width: Math.min(implicitWidth, root.textWidth)
            text: root.styled(fits ? "<br>" : " ")
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
            lineHeight: root.isShort ? 1.0 : 1.06
            // While a word is lit the others step back, so it stands out even where the
            // theme's accent is close to its foreground.
            color: root.active >= 0 ? Qt.alpha(root.textColor, .62 /* captions::UNLIT_OPACITY */) : root.textColor
            textFormat: Text.StyledText
            font.pixelSize: root.fontPx
            font.weight: root.isShort ? Font.Black : Font.DemiBold
        }
        Label {
            id: natural
            visible: false
            text: root.escaped(root.cueLines.join("\n")).replace(/\n/g, "<br>")
            textFormat: Text.StyledText
            font: lines.font
        }
    }
}
