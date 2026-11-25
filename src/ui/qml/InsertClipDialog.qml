import QtQuick
import Omareel

// Picks a video to insert at the playhead.
PortalFilePicker {
    objectName: "insertClipPicker"
    title: "Insert a video at the playhead"
    nameFilters: ["Videos (*.mp4 *.mov *.mkv *.webm *.m4v *.avi)", "All files (*)"]
    onAccepted: url => editor.insertMedia(url)
    onFailed: message => console.warn("omareel:", message)
}
