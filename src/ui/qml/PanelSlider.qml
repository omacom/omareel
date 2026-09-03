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
    spacing: 4
    RowLayout {
        Layout.fillWidth: true
        PanelLabel { text: root.label }
        Item { Layout.fillWidth: true }
        PanelValue { text: Number(root.value).toFixed(root.decimals) }
    }
    EditorSlider {
        Layout.fillWidth: true
        Layout.preferredHeight: 28
        from: root.from; to: root.to; value: root.value; stepSize: root.stepSize
        onPressedChanged: pressed ? editor.beginCoalescedEdit(root.path) : editor.endCoalescedEdit()
        onMoved: editor.setProjectValue(root.path, value, true)
    }
}
