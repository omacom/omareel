import QtQml
import QtQuick

// An OmToolTip made the first time it is shown, then kept. Each tooltip is a popup with its
// own items, transitions and bindings, and most are never hovered, so making them all with the
// editor slowed its start. An Instantiator, not a Loader, so it takes no place in a layout.
Instantiator {
    id: root
    property Item target
    property bool shown: false
    property string text: ""
    property string shortcut: ""
    property string placement: "below"
    property int delay: 450
    property bool wanted: false
    onShownChanged: if (shown) wanted = true
    Component.onCompleted: if (shown) wanted = true
    active: wanted
    delegate: OmToolTip {
        parent: root.target
        target: root.target
        visible: root.shown
        text: root.text
        shortcut: root.shortcut
        placement: root.placement
        delay: root.delay
    }
}
