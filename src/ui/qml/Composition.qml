import QtQuick
import Omarecord

Item {
    id: composition
    width: comp.outputWidth
    height: comp.outputHeight
    property real ref: height / 1080
    property real pad: Math.min(width, height) * comp.project.frame.padding
    property real availableWidth: width - 2 * pad
    property real availableHeight: height - 2 * pad
    property real sourceAspect: (comp.sourceWidth * comp.project.crop.w) / Math.max(1, comp.sourceHeight * comp.project.crop.h)
    property real videoWidth: Math.min(availableWidth, availableHeight * sourceAspect)
    property real videoHeight: videoWidth / sourceAspect
    clip: true

    Background { anchors.fill: parent; settings: comp.project.background; softwareRendering: comp.softwareRendering }

    Item {
        id: stage
        anchors.fill: parent
        transform: Scale {
            origin.x: frame.x + comp.zoom.cx * frame.width
            origin.y: frame.y + comp.zoom.cy * frame.height
            xScale: comp.zoom.scale
            yScale: comp.zoom.scale
        }
        RoundedFrame {
            id: frame
            x: (composition.width - width) / 2
            y: (composition.height - height) / 2
            width: composition.videoWidth
            height: composition.videoHeight
            settings: comp.project.frame
            crop: comp.project.crop
            softwareRendering: comp.softwareRendering
            ref: composition.ref
        }
        CursorOverlay {
            x: frame.x
            y: frame.y
            width: frame.width
            height: frame.height
            cursor: comp.cursor
            ripples: comp.ripples
            settings: comp.project.cursor
            ref: composition.ref
        }
    }
}
