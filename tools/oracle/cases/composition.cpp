// composition: the per-time composition state, which the C++ at rust-migration-base builds
// twice: Editor's preview getters (src/ui/Editor.cpp) and the frame loop of Exporter::run
// (src/render/Exporter.cpp). Both live in large QObject classes the oracle can't link (Qt
// Multimedia, Qt Quick, OpenGL), so the two code paths are copied here verbatim, with the
// members and locals they read turned into inputs. The variants differ on purpose (see
// tests/golden/README.md, composition); both are recorded.

#include "timeline.h"

#include "core/CameraTimeline.h"
#include "core/ClipTimeline.h"
#include "core/InputLog.h"
#include "core/KeystrokeTrack.h"
#include "core/LayoutTimeline.h"
#include "core/MotionTrack.h"
#include "render/MediaProbe.h"

#include <QDir>
#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QVariant>
#include <QVariantList>
#include <QVariantMap>

#include <algorithm>
#include <memory>

namespace Oracle {

namespace {

namespace EditorCopy {

using namespace Omareel;

// The members of Editor (src/ui/Editor.h at the tag) that the getters below read, with the
// same names and types, so the definitions after it are the tag's text unchanged.
struct Editor {
    Project m_project;
    KeystrokeTrack m_keystrokeTrack;
    bool m_hasCamera = false;
    double m_cameraDuration = 0.0;
    double m_cameraOffset = 0.0;
    QString m_mediaPlayerSource;
    bool m_mediaFrameReady = false;
    QHash<QString, VideoInfo> m_inserts;
    double m_outputPosition = 0.0;
    std::shared_ptr<const MotionTrack> m_motion;

    double duration() const;
    double sourcePosition() const;
    QVariantMap previewZoom() const;
    LayoutSample layoutAt(double outputTime) const;
    QVariantMap previewLayout() const;
    QVariantMap previewMedia() const;
    QVariantMap previewCursor() const;
    QVariantList previewRipples() const;
    QVariantList previewKeystrokePills() const;
    bool cameraVisible() const;
    bool clipIsMedia(int index) const;
    int clipForOutput(double output, double *clipOutputStart = nullptr) const;
    double outputToSource(double outputTime) const;
};

// src/ui/Editor.cpp:513 at rust-migration-base, verbatim.
double Editor::duration() const { return ClipTimeline(m_project.clips).totalDuration(); }

// src/ui/Editor.cpp:528-602 at rust-migration-base, verbatim.
double Editor::sourcePosition() const { return outputToSource(m_outputPosition); }

QVariantMap Editor::previewZoom() const
{
    const MotionSample s = m_motion ? m_motion->sample(sourcePosition()) : MotionSample{};
    return {{QStringLiteral("scale"), s.zoomScale}, {QStringLiteral("cx"), s.zoomCx},
        {QStringLiteral("cy"), s.zoomCy}, {QStringLiteral("velocity"), s.zoomCenterVelocity}};
}

LayoutSample Editor::layoutAt(double outputTime) const
{
    return sampleLayout(m_project.clips, outputTime, m_hasCamera, m_project.camera.enabled);
}

QVariantMap Editor::previewLayout() const
{
    const LayoutSample layout = layoutAt(m_outputPosition);
    return {{QStringLiteral("cameraFull"), layout.cameraFull},
            {QStringLiteral("bubble"), layout.bubble}};
}

QVariantMap Editor::previewMedia() const
{
    const int index = clipForOutput(m_outputPosition);
    if (!clipIsMedia(index))
        return {{QStringLiteral("active"), false}, {QStringLiteral("ready"), true},
                {QStringLiteral("fit"), QStringLiteral("fit")}, {QStringLiteral("width"), 16},
                {QStringLiteral("height"), 9}, {QStringLiteral("missing"), false}};
    const Clip &clip = m_project.clips[index];
    const VideoInfo info = m_inserts.value(clip.media);
    return {{QStringLiteral("active"), true},
            {QStringLiteral("ready"), m_mediaFrameReady && m_mediaPlayerSource == clip.media},
            {QStringLiteral("fit"), clip.fit}, {QStringLiteral("width"), std::max(1, info.width)},
            {QStringLiteral("height"), std::max(1, info.height)},
            {QStringLiteral("missing"), !info.isValid()}};
}

QVariantMap Editor::previewCursor() const
{
    const bool insert = clipIsMedia(clipForOutput(m_outputPosition));
    const MotionSample s = m_motion && !insert ? m_motion->sample(sourcePosition()) : MotionSample{};
    return {{QStringLiteral("x"), s.cursorX}, {QStringLiteral("y"), s.cursorY},
        {QStringLiteral("visible"), m_project.cursor.visible && !insert}, {QStringLiteral("scale"), s.cursorScale},
        {QStringLiteral("opacity"), s.cursorOpacity}, {QStringLiteral("rotation"), s.cursorRotation}};
}

QVariantList Editor::previewRipples() const
{
    QVariantList result;
    if (!m_motion || clipIsMedia(clipForOutput(m_outputPosition))
        || (m_project.cursor.clickEffect != QLatin1String("ripple")
            && m_project.cursor.clickEffect != QLatin1String("circle"))) return result;
    for (const auto &r : m_motion->ripples(sourcePosition()))
        result << QVariantMap{{QStringLiteral("x"), r.x}, {QStringLiteral("y"), r.y}, {QStringLiteral("progress"), r.progress}};
    return result;
}

QVariantList Editor::previewKeystrokePills() const
{
    QVariantList result;
    if (clipIsMedia(clipForOutput(m_outputPosition))) return result;
    for (const KeystrokePill &pill : m_keystrokeTrack.sample(sourcePosition()))
        result << QVariantMap{{QStringLiteral("keys"), pill.keys},
            {QStringLiteral("text"), pill.text}, {QStringLiteral("opacity"), pill.opacity}};
    return result;
}

bool Editor::cameraVisible() const
{
    if (!m_hasCamera || clipIsMedia(clipForOutput(m_outputPosition))) return false;
    const CameraTime mapped = mapCameraTime(sourcePosition(), m_cameraOffset, m_cameraDuration);
    // A full-frame camera holds its first frame until the camera started instead of leaving
    // the frame empty.
    return !mapped.beforeStart || layoutAt(m_outputPosition).cameraFull > 0.0;
}

// src/ui/Editor.cpp:707-710 at rust-migration-base, verbatim.
bool Editor::clipIsMedia(int index) const
{
    return index >= 0 && index < m_project.clips.size() && m_project.clips[index].isMedia();
}

// src/ui/Editor.cpp:999-1016 at rust-migration-base, verbatim.
int Editor::clipForOutput(double output, double *clipOutputStart) const
{
    double start = 0.0;
    for (int i = 0; i < m_project.clips.size(); ++i) {
        const double length = (m_project.clips[i].out - m_project.clips[i].in) / m_project.clips[i].speed;
        if (output < start + length || i == m_project.clips.size() - 1) {
            if (clipOutputStart) *clipOutputStart = start;
            return i;
        }
        start += length;
    }
    return -1;
}

double Editor::outputToSource(double outputTime) const
{
    return ClipTimeline(m_project.clips).recordingSourceTime(std::clamp(outputTime, 0.0, duration()));
}

} // namespace EditorCopy

namespace ExporterCopy {

using namespace Omareel;

// src/render/Exporter.cpp:61 at rust-migration-base, verbatim: what Exporter's own probe()
// returns for the camera video. Only width and duration are read below.
struct MediaInfo { int width = 0; int height = 0; double duration = 0.0; double fps = 30.0; bool audio = false; };

// The members of CompositionState (src/render/Exporter.h) that the frame loop writes, as a
// plain struct; notifyChanged() emits the QML change signal there and does nothing here.
struct CompositionState {
    bool cameraVisible = true;
    double time = 0.0;
    QVariantMap zoom;
    QVariantMap layout;
    QVariantMap media;
    QVariantMap cursor;
    QVariantList ripples;
    QVariantList keystrokePills;
    void notifyChanged() {}
};

// src/render/Exporter.cpp:426-434 at rust-migration-base, verbatim: the state the composition
// starts from, before the first frame.
void initialState(const Project &project, CompositionState &state)
{
    state.zoom = {{QStringLiteral("scale"), 1.0}, {QStringLiteral("cx"), 0.5},
        {QStringLiteral("cy"), 0.5}, {QStringLiteral("velocity"), 0.0}};
    state.layout = {{QStringLiteral("cameraFull"), 0.0}, {QStringLiteral("bubble"), 1.0}};
    state.media = {{QStringLiteral("active"), false}, {QStringLiteral("ready"), true},
        {QStringLiteral("fit"), QStringLiteral("fit")}, {QStringLiteral("width"), 16},
        {QStringLiteral("height"), 9}};
    state.cursor = {{QStringLiteral("x"), 0.5}, {QStringLiteral("y"), 0.5},
        {QStringLiteral("visible"), project.cursor.visible}, {QStringLiteral("scale"), 1.0},
        {QStringLiteral("opacity"), project.cursor.visible ? 1.0 : 0.0}, {QStringLiteral("rotation"), 0.0}};
}

// What the frame loop of Exporter::run does with frame `outputTime` (frame / fps). The decode
// thread picks the frame's clip with timeline.clipAt(outputTime) (Exporter.cpp:722) and sends
// its index as packet.clip; lines 874-879, which stop the export when the packet holds no image,
// are left out. The rest is src/render/Exporter.cpp:872-873 and 881-916 at rust-migration-base,
// re-indented by four spaces. state carries over from frame to frame, as in the exporter.
void exportFrame(const ClipTimeline &timeline, const Project &project, const MotionTrack &motion,
                 const KeystrokeTrack &keystrokes, const MediaInfo &cameraMedia, double cameraOffset,
                 const QHash<QString, VideoInfo> &inserts, double outputTime, CompositionState &state)
{
    struct { int clip = -1; } packet;
    packet.clip = timeline.clipAt(outputTime);
    const Clip *clip = packet.clip >= 0 ? &project.clips[packet.clip] : nullptr;
    const bool insert = clip && clip->isMedia();

    // An inserted clip shows none of the recording's motion, cursor, keys, or camera.
    const double sourceTime = timeline.recordingSourceTime(outputTime);
    const MotionSample sample = insert ? MotionSample{} : motion.sample(sourceTime);
    state.time = sourceTime;
    state.zoom = {{QStringLiteral("scale"), sample.zoomScale}, {QStringLiteral("cx"), sample.zoomCx},
        {QStringLiteral("cy"), sample.zoomCy},
        {QStringLiteral("velocity"), sample.zoomCenterVelocity}};
    state.cursor = {{QStringLiteral("x"), sample.cursorX}, {QStringLiteral("y"), sample.cursorY},
        {QStringLiteral("visible"), project.cursor.visible}, {QStringLiteral("scale"), sample.cursorScale},
        {QStringLiteral("opacity"), sample.cursorOpacity}, {QStringLiteral("rotation"), sample.cursorRotation}};
    state.ripples.clear();
    if (!insert && (project.cursor.clickEffect == QLatin1String("ripple")
                    || project.cursor.clickEffect == QLatin1String("circle"))) {
        for (const auto &ripple : motion.ripples(sourceTime))
            state.ripples << QVariantMap{{QStringLiteral("x"), ripple.x}, {QStringLiteral("y"), ripple.y},
                                         {QStringLiteral("progress"), ripple.progress}};
    }
    state.keystrokePills.clear();
    for (const KeystrokePill &pill : insert ? QVector<KeystrokePill>{} : keystrokes.sample(sourceTime))
        state.keystrokePills << QVariantMap{{QStringLiteral("keys"), pill.keys},
            {QStringLiteral("text"), pill.text}, {QStringLiteral("opacity"), pill.opacity}};
    const LayoutSample layout = sampleLayout(project.clips, outputTime, cameraMedia.width > 0,
                                             project.camera.enabled);
    state.layout = {{QStringLiteral("cameraFull"), layout.cameraFull},
                    {QStringLiteral("bubble"), layout.bubble}};
    const CameraTime cameraTime = mapCameraTime(sourceTime, cameraOffset, cameraMedia.duration);
    state.cameraVisible = !insert && (!cameraTime.beforeStart || layout.cameraFull > 0.0);
    if (insert) {
        const VideoInfo info = inserts.value(clip->media);
        state.media = {{QStringLiteral("active"), true}, {QStringLiteral("ready"), true},
            {QStringLiteral("fit"), clip->fit}, {QStringLiteral("width"), info.width},
            {QStringLiteral("height"), info.height}};
    } else {
        state.media.insert(QStringLiteral("active"), false);
    }
    state.notifyChanged();
}

} // namespace ExporterCopy

// README "variant".
QByteArray variantText(const QVariant &value)
{
    switch (value.metaType().id()) {
    case QMetaType::Double: return num(value.toDouble());
    case QMetaType::Int: return "int(" + QByteArray::number(value.toInt()) + ')';
    case QMetaType::Bool: return boolText(value.toBool());
    case QMetaType::QString: return quote(value.toString());
    case QMetaType::QStringList: return quoteList(value.toStringList());
    case QMetaType::QVariantMap: {
        const QVariantMap map = value.toMap();
        QList<QByteArray> items;
        for (auto it = map.begin(); it != map.end(); ++it)
            items << it.key().toUtf8() + '=' + variantText(it.value());
        return '{' + items.join(',') + '}';
    }
    case QMetaType::QVariantList: {
        QList<QByteArray> items;
        for (const QVariant &item : value.toList()) items << variantText(item);
        return '[' + items.join(',') + ']';
    }
    default: return '<' + QByteArray(value.typeName()) + '>' + quote(value.toString());
    }
}

QByteArray variantText(const QVariantMap &map) { return variantText(QVariant(map)); }
QByteArray variantText(const QVariantList &list) { return variantText(QVariant(list)); }

} // namespace

void runComposition(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("composition/cases.json"))).toObject();
    Text editorOut, exporterOut;
    for (auto it = cases.begin(); it != cases.end(); ++it) {
        const QJsonObject c = it.value().toObject();
        const double sourceDuration = c.value(QStringLiteral("sourceDuration")).toDouble();
        QString error = QStringLiteral("<untouched>");
        const Omareel::InputLog input = Omareel::InputLog::loadBundle(
            QDir(context.corpus).filePath(c.value(QStringLiteral("bundle")).toString()), sourceDuration, &error);
        if (!error.isEmpty()) fail(QStringLiteral("composition %1: %2").arg(it.key(), error));
        const Omareel::Project project = Omareel::Project::fromJson(c.value(QStringLiteral("project")).toObject());
        const Omareel::MotionTrack motion = Omareel::MotionTrack::build(
            sourceDuration, c.value(QStringLiteral("width")).toInt(), c.value(QStringLiteral("height")).toInt(),
            input.events(), project, c.value(QStringLiteral("captureFps")).toDouble());
        const Omareel::KeystrokeTrack keystrokes = Omareel::KeystrokeTrack::build(input.events(), project.keystrokes);
        const QJsonValue camera = c.value(QStringLiteral("camera"));
        const double cameraOffset = camera.toObject().value(QStringLiteral("offset")).toDouble();
        const double cameraDuration = camera.toObject().value(QStringLiteral("duration")).toDouble();
        QHash<QString, Omareel::VideoInfo> inserts;
        bool insertsValid = true;
        const QJsonObject insertCases = c.value(QStringLiteral("inserts")).toObject();
        for (auto insert = insertCases.begin(); insert != insertCases.end(); ++insert) {
            Omareel::VideoInfo info;
            if (insert.value().isArray()) {
                const QJsonArray a = insert.value().toArray();
                info = {a.at(0).toInt(), a.at(1).toInt(), a.at(2).toDouble(), a.at(3).toDouble(), a.at(4).toBool()};
            }
            insertsValid = insertsValid && info.isValid();
            inserts.insert(insert.key(), info);
        }

        EditorCopy::Editor editor;
        editor.m_project = project;
        if (c.value(QStringLiteral("motion")).toBool())
            editor.m_motion = std::make_shared<const Omareel::MotionTrack>(motion);
        editor.m_keystrokeTrack = keystrokes;
        editor.m_hasCamera = camera.isObject();
        editor.m_cameraOffset = cameraOffset;
        editor.m_cameraDuration = cameraDuration;
        editor.m_inserts = inserts;
        editor.m_mediaFrameReady = c.value(QStringLiteral("mediaFrameReady")).toBool();
        editor.m_mediaPlayerSource = c.value(QStringLiteral("mediaPlayerSource")).toString();
        const double duration = editor.duration();
        QVector<double> positions;
        for (int i = -3; i <= int(std::ceil(duration * 30)) + 3; ++i) positions << i / 30.0;
        double start = 0.0;
        positions << before(start) << start << after(start);
        for (const Omareel::Clip &clip : project.clips) {
            start += (clip.out - clip.in) / clip.speed;
            positions << before(start) << start << after(start);
        }
        positions = sortedUnique(positions);
        editorOut << "== " << it.key().toUtf8() << "\n";
        for (const double position : positions) {
            editor.m_outputPosition = position;
            editorOut.row({num(position), num(editor.sourcePosition()), variantText(editor.previewZoom()),
                           variantText(editor.previewLayout()), variantText(editor.previewMedia()),
                           variantText(editor.previewCursor()), variantText(editor.previewRipples()),
                           variantText(editor.previewKeystrokePills()), boolText(editor.cameraVisible())});
        }

        // Exporter::run refuses a project whose inserted videos don't probe, so such a case has
        // no exporter section.
        if (!insertsValid) continue;
        const Omareel::ClipTimeline timeline(project.clips);
        const ExporterCopy::MediaInfo cameraMedia = camera.isObject()
            ? ExporterCopy::MediaInfo{1280, 720, cameraDuration, 30.0, false} : ExporterCopy::MediaInfo{};
        ExporterCopy::CompositionState state;
        ExporterCopy::initialState(project, state);
        const auto stateRow = [&](const QByteArray &first) {
            exporterOut.row({first, num(state.time), variantText(state.zoom), variantText(state.layout),
                             variantText(state.media), variantText(state.cursor), variantText(state.ripples),
                             variantText(state.keystrokePills), boolText(state.cameraVisible)});
        };
        exporterOut << "== " << it.key().toUtf8() << "\n";
        stateRow("initial");
        constexpr int fps = 30;
        const int totalFrames = std::max(1, int(std::ceil(timeline.totalDuration() * fps)));
        for (int frame = 0; frame < totalFrames; ++frame) {
            const double outputTime = frame / double(fps);
            ExporterCopy::exportFrame(timeline, project, motion, keystrokes, cameraMedia, cameraOffset,
                                      inserts, outputTime, state);
            stateRow(num(outputTime));
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("editor.txt")), editorOut.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("exporter.txt")), exporterOut.data());
}

} // namespace Oracle
