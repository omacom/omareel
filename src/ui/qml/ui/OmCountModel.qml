import QtQuick

// A model of `size` empty rows for a Repeater, which grows and shrinks at its end. A Repeater
// over a plain number throws every delegate away and makes them all again whenever the number
// changes; while the editor's window settles its size at start, the filmstrip and the ruler did
// that three or four times before the first frame. Over this model only the difference is made
// or destroyed.
ListModel {
    id: model
    property int size: 0
    onSizeChanged: sync()
    Component.onCompleted: sync()

    function sync() {
        const wanted = Math.max(0, size)
        if (count > wanted) {
            remove(wanted, count - wanted)
        } else if (count < wanted) {
            const rows = []
            for (let i = count; i < wanted; ++i) rows.push({})
            append(rows)
        }
    }
}
