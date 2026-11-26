import QtQuick
import QtQuick.Layouts

// Segmented control: a recessed track with a raised thumb that glides to the chosen option.
Item {
    id: root
    property var options: []
    property var value
    property int focusedIndex: -1
    property int hoveredIndex: -1
    signal changed(var value)
    implicitHeight: theme.space.controlHeight
    implicitWidth: segments.implicitWidth + 4
    activeFocusOnTab: true
    onActiveFocusChanged: if (activeFocus && focusedIndex < 0) focusedIndex = selectedIndex()
    function optionValue(option) { return typeof option === "object" ? option.value : option }
    function optionLabel(option) { return typeof option === "object" ? option.label : option }
    function optionEnabled(option) { return typeof option !== "object" || option.enabled === undefined || option.enabled }
    function selectedIndex() {
        for (let i = 0; i < options.length; ++i) if (optionValue(options[i]) === value) return i
        return -1
    }
    readonly property int currentIndex: selectedIndex()
    Keys.onLeftPressed: event => { focusedIndex = Math.max(0, focusedIndex - 1); event.accepted = true }
    Keys.onRightPressed: event => { focusedIndex = Math.min(options.length - 1, focusedIndex + 1); event.accepted = true }
    Keys.onReturnPressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }
    Keys.onEnterPressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }
    Keys.onSpacePressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }

    Rectangle {
        anchors.fill: parent
        radius: theme.controlRadius
        color: theme.fill
        border.width: root.activeFocus ? 1 : 0
        border.color: Qt.alpha(theme.accent, .8)
    }
    Rectangle {
        id: thumb
        readonly property Item segment: root.currentIndex >= 0 ? segmentRepeater.itemAt(root.currentIndex) : null
        visible: segment !== null
        x: segment ? segments.x + segment.x : 0
        y: 2
        width: segment ? segment.width : 0
        height: root.height - 4
        radius: Math.max(0, theme.controlRadius - 1)
        color: theme.raised
        border.width: 1
        border.color: theme.strokeStrong
        Behavior on x { enabled: thumb.visible; NumberAnimation { duration: 240; easing.type: Easing.OutQuint } }
        Behavior on width { enabled: thumb.visible; NumberAnimation { duration: 240; easing.type: Easing.OutQuint } }
    }
    RowLayout {
        id: segments
        anchors.fill: parent
        anchors.margins: 2
        spacing: 0
        Repeater {
            id: segmentRepeater
            model: root.options
            Item {
                id: segment
                required property var modelData
                required property int index
                readonly property bool selected: root.optionValue(modelData) === root.value
                readonly property bool enabledOption: root.optionEnabled(modelData)
                Layout.fillWidth: true
                Layout.fillHeight: true
                Layout.preferredWidth: label.implicitWidth + 20
                Text {
                    id: label
                    anchors.centerIn: parent
                    width: Math.min(implicitWidth, parent.width - 8)
                    textFormat: Text.PlainText
                    text: root.optionLabel(segment.modelData)
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignHCenter
                    color: !segment.enabledOption ? theme.textFaint
                         : segment.selected || mouse.containsMouse ? theme.textStrong : theme.textMuted
                    font.family: theme.fontFamily
                    font.pixelSize: theme.type.body
                    font.weight: segment.selected ? Font.DemiBold : Font.Medium
                    Behavior on color { ColorAnimation { duration: 140 } }
                }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 0
                    radius: Math.max(0, theme.controlRadius - 1)
                    color: theme.fillHover
                    opacity: mouse.containsMouse && !segment.selected && segment.enabledOption ? 1 : 0
                    z: -1
                    Behavior on opacity { NumberAnimation { duration: 120 } }
                }
                MouseArea {
                    id: mouse
                    anchors.fill: parent
                    hoverEnabled: true
                    enabled: segment.enabledOption
                    cursorShape: Qt.PointingHandCursor
                    onEntered: root.focusedIndex = segment.index
                    onClicked: root.changed(root.optionValue(segment.modelData))
                }
            }
        }
    }
}
