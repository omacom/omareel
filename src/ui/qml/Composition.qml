import QtQuick
import QtQuick.Effects
import Omareel

Item {
    id: composition
    // Preview-only access point. Exporter continues finding the same objectName.
    readonly property alias frameSource: frame.frameSource
    readonly property alias videoOutput: frame.videoOutput
    readonly property alias cameraFrameSource: cameraOverlay.cameraFrameSource
    readonly property alias cameraVideoOutput: cameraOverlay.cameraVideoOutput
    readonly property alias mediaVideoOutput: mediaLayer.videoOutput
    property bool nativePreview: false
    width: comp.outputWidth
    height: comp.outputHeight
    property real ref: height / 1080
    // Auto-reframe (crates/omareel_core/src/reframe.rs): the frame fills the canvas with a
    // window of the crop that follows the action; a zoom is part of that window, not a scale.
    readonly property bool reframed: comp.zoom.reframe === true
    readonly property bool portrait: height > width
    readonly property var viewCrop: reframed
        ? ({ x: comp.project.crop.x + comp.zoom.rx * comp.project.crop.w,
             y: comp.project.crop.y + comp.zoom.ry * comp.project.crop.h,
             w: comp.zoom.rw * comp.project.crop.w,
             h: comp.zoom.rh * comp.project.crop.h })
        : comp.project.crop
    // The cursor, ripples, zoom and zoom targets are normalised to the whole recording (as in
    // input.jsonl); they are drawn where the crop, or the reframe window, shows them. Without a
    // crop or reframing this is the identity.
    function inView(point) {
        const c = viewCrop
        return Object.assign({}, point, { x: (point.x - c.x) / Math.max(.0001, c.w),
                                          y: (point.y - c.y) / Math.max(.0001, c.h) })
    }
    // The frame's scale and its fixed point (normalised to the frame) for a zoom; reframed, a
    // zoom is the window's size instead. PreviewPane maps clicks back through these.
    readonly property real zoomScale: reframed ? 1 : comp.zoom.scale
    readonly property point zoomOrigin: {
        const o = inView({ x: comp.zoom.cx, y: comp.zoom.cy })
        return Qt.point(Math.max(0, Math.min(1, o.x)), Math.max(0, Math.min(1, o.y)))
    }
    // The cursor keeps the size it had on screen: one logical screen pixel is captureScale
    // source pixels, and the (cropped) source is drawn at videoWidth output pixels. A small
    // region therefore gets a proportionally large cursor instead of a microscopic one.
    property real cursorRef: (comp.captureScale > 0 ? comp.captureScale : 1)
                             * videoWidth / Math.max(1, comp.sourceWidth * viewCrop.w)
    property bool noBackground: comp.project.background.type === "none"
    property real pad: noBackground ? 0 : Math.min(width, height) * comp.project.frame.padding
    property real availableWidth: width - 2 * pad
    property real availableHeight: height - 2 * pad
    property real borderWidth: !noBackground && comp.project.frame.border.enabled
                               ? comp.project.frame.border.width * ref : 0
    // The shape of what the frame shows: the crop, or reframed the window, which has the
    // canvas's shape (reframe.rs), so the frame is the canvas scaled down inside the padding.
    property real sourceAspect: (comp.sourceWidth * viewCrop.w) / Math.max(1, comp.sourceHeight * viewCrop.h)
    property real videoWidth: noBackground ? width
        : Math.max(1, Math.min(availableWidth - 2 * borderWidth,
                              (availableHeight - 2 * borderWidth) * sourceAspect))
    property real videoHeight: noBackground ? height : videoWidth / sourceAspect
    readonly property bool cameraShown: comp.cameraAvailable && comp.cameraVisible
    // How far the current clip's layout has grown the camera to full frame.
    readonly property real cameraFull: cameraShown ? comp.layout.cameraFull : 0
    // An inserted video replaces the recording, its camera and keys while it plays.
    readonly property bool mediaActive: comp.media.active
    // Once the camera or an inserted video covers the canvas, nothing beneath needs drawing.
    readonly property bool screenCovered: cameraFull >= 1 || mediaActive
    readonly property real cutScale: comp.transition && comp.transition.scale > 0 ? comp.transition.scale : 1
    readonly property bool backgroundCovered: cameraFull >= 1
        || (mediaActive && comp.media.ready && comp.media.fit !== "framed")
    // Annotations showing now: screen-anchored ones ride the zoomed stage, frame-anchored ones
    // sit on the canvas above everything.
    readonly property var visibleOverlays: comp.visibleOverlays || []
    readonly property var screenOverlays: visibleOverlays.filter(item => item.anchor === "screen")
    readonly property var frameOverlays: visibleOverlays.filter(item => item.anchor !== "screen")
    clip: true

    Background {
        anchors.fill: parent
        visible: !composition.backgroundCovered
        settings: comp.project.background
        softwareRendering: comp.softwareRendering
        loadAsynchronously: composition.nativePreview
    }

    Item {
        id: stage
        anchors.fill: parent
        visible: !composition.screenCovered
        layer.enabled: !comp.softwareRendering && comp.project.zoomStyle.motionBlur > 0
            && comp.zoom.velocity > 0.01
        layer.effect: MultiEffect {
            blurEnabled: true
            blurMax: 32
            blur: Math.min(1, comp.zoom.velocity * .32) * comp.project.zoomStyle.motionBlur
        }
        transform: [
            Scale {
                origin.x: frame.x + composition.zoomOrigin.x * frame.width
                origin.y: frame.y + composition.zoomOrigin.y * frame.height
                xScale: composition.zoomScale
                yScale: composition.zoomScale
            },
            // A transition's punch at a cut grows the whole shot from the canvas centre.
            Scale {
                origin.x: composition.width / 2
                origin.y: composition.height / 2
                xScale: composition.cutScale
                yScale: composition.cutScale
            }
        ]
        RoundedFrame {
            id: frame
            x: (composition.width - width) / 2
            y: (composition.height - height) / 2
            width: composition.videoWidth
            height: composition.videoHeight
            settings: comp.project.frame
            crop: composition.viewCrop
            softwareRendering: comp.softwareRendering
            ref: composition.ref
            unframed: composition.noBackground
            nativePreview: composition.nativePreview
        }
        OverlayLayer {
            x: frame.x
            y: frame.y
            width: frame.width
            height: frame.height
            visible: composition.screenOverlays.length > 0
            items: composition.screenOverlays
            space: composition.viewCrop
            ref: composition.ref
            colors: comp.project.renderTheme
            softwareRendering: comp.softwareRendering
            blurSource: frame.content
            redactionBaked: comp.redactionBaked === true
        }
        CursorOverlay {
            x: frame.x
            y: frame.y
            width: frame.width
            height: frame.height
            cursor: composition.inView(comp.cursor)
            ripples: comp.ripples.map(r => composition.inView(r))
            settings: comp.project.cursor
            ref: composition.cursorRef
        }
    }
    CameraOverlay {
        id: cameraOverlay
        visible: composition.cameraShown && !composition.mediaActive
            && (composition.cameraFull > 0 || comp.layout.bubble > 0)
        settings: comp.project.camera
        fullProgress: composition.cameraFull
        bubbleOpacity: comp.layout.bubble
        zoomScale: comp.zoom.scale
        outputWidth: composition.width
        outputHeight: composition.height
        sourceWidth: comp.cameraSourceWidth
        sourceHeight: comp.cameraSourceHeight
        softwareRendering: comp.softwareRendering
        nativePreview: composition.nativePreview
        // A reframed portrait keeps the bubble centred along the bottom (or top), under the action.
        centered: composition.reframed && composition.portrait
    }
    MediaLayer {
        id: mediaLayer
        anchors.fill: parent
        visible: composition.mediaActive
        // The preview holds the layer back until the clip's first frame, so the last frame of
        // an earlier clip never flashes.
        opacity: comp.media.ready ? 1 : 0
        media: comp.media
        frameSettings: comp.project.frame
        padding: composition.pad
        ref: composition.ref
        softwareRendering: comp.softwareRendering
        nativePreview: composition.nativePreview
    }
    // Frame annotations (labels, cards) sit over the picture; captions and keystroke pills
    // stay readable above them.
    OverlayLayer {
        anchors.fill: parent
        visible: composition.frameOverlays.length > 0
        items: composition.frameOverlays
        ref: composition.ref
        colors: comp.project.renderTheme
        backgroundSettings: comp.project.background
        softwareRendering: comp.softwareRendering
        nativePreview: composition.nativePreview
    }
    CaptionOverlay {
        visible: !composition.mediaActive
        caption: comp.caption
        colors: comp.project.renderTheme
        ref: composition.ref
    }
    KeystrokeOverlay {
        visible: comp.project.keystrokes.enabled && comp.keystrokePills.length > 0
            && !composition.screenCovered
        opacity: 1 - composition.cameraFull
        pills: comp.keystrokePills
        settings: comp.project.keystrokes
        colors: comp.project.renderTheme
        ref: composition.ref
        // Above a centred bubble at the bottom.
        bottomInset: cameraOverlay.centered && cameraOverlay.visible && cameraOverlay.topness < .5
            ? cameraOverlay.bubbleHeight + cameraOverlay.marginY : 0
    }
}
