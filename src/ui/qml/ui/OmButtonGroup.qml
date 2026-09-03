import QtQuick
import QtQuick.Layouts

RowLayout {
    id: root
    property var options: []
    property var value
    property int focusedIndex: -1
    signal changed(var value)
    spacing: theme.space.md
    activeFocusOnTab: true
    onActiveFocusChanged: if (activeFocus && focusedIndex < 0) focusedIndex = selectedIndex()
    function optionValue(option) { return typeof option === "object" ? option.value : option }
    function optionLabel(option) { return typeof option === "object" ? option.label : option }
    function optionEnabled(option) { return typeof option !== "object" || option.enabled === undefined || option.enabled }
    function selectedIndex() {
        for (let i = 0; i < options.length; ++i) if (optionValue(options[i]) === value) return i
        return 0
    }
    Keys.onLeftPressed: event => { focusedIndex = Math.max(0, focusedIndex - 1); event.accepted = true }
    Keys.onRightPressed: event => { focusedIndex = Math.min(options.length - 1, focusedIndex + 1); event.accepted = true }
    Keys.onReturnPressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }
    Keys.onEnterPressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }
    Keys.onSpacePressed: event => { changed(optionValue(options[focusedIndex])); event.accepted = true }
    Repeater {
        model: root.options
        OmButton {
            required property var modelData
            required property int index
            Layout.fillWidth: true
            text: root.optionLabel(modelData)
            enabled: root.optionEnabled(modelData)
            bordered: true
            selected: root.optionValue(modelData) === root.value
            activeFocusOnTab: false
            onHoveredChanged: if (hovered) root.focusedIndex = index
            onClicked: root.changed(root.optionValue(modelData))
        }
    }
}
