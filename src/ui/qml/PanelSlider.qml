import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root
    required property string label
    required property string path
    required property real from
    required property real to
    required property real value
    property real stepSize: 0
    property int decimals: 0
    spacing: 2
    RowLayout {
        Layout.fillWidth: true
        Label { text: root.label; color: "#bbbcc3" }
        Item { Layout.fillWidth: true }
        Label { text: Number(root.value).toFixed(root.decimals); color: "#858894"; font.family: "monospace" }
    }
    Slider {
        Layout.fillWidth: true
        from: root.from; to: root.to; value: root.value; stepSize: root.stepSize
        onPressedChanged: pressed ? editor.beginCoalescedEdit(root.path) : editor.endCoalescedEdit()
        onMoved: editor.setProjectValue(root.path, value, true)
    }
}
