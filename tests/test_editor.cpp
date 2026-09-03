#include "ui/Editor.h"
#include "core/Theme.h"

#include <QDir>
#include <QFile>
#include <QProcess>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QQmlComponent>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <algorithm>
#include <functional>
#include <QtTest>

using namespace OmaRecord;

class EditorTest : public QObject
{
    Q_OBJECT
private slots:
    void initTestCase()
    {
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QVERIFY(m_temporary.isValid());
        const QString fixture = QFINDTESTDATA("fixtures/synthetic");
        QVERIFY(!fixture.isEmpty());
        m_bundle = m_temporary.filePath(QStringLiteral("editor.omarecord"));
        QVERIFY(QDir().mkpath(m_bundle));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("capture.json")), QDir(m_bundle).filePath(QStringLiteral("capture.json"))));
        QVERIFY(QFile::copy(QDir(fixture).filePath(QStringLiteral("input.jsonl")), QDir(m_bundle).filePath(QStringLiteral("input.jsonl"))));
        QProcess makeVideo;
        makeVideo.start(QStringLiteral("ffmpeg"), {QStringLiteral("-y"), QStringLiteral("-v"), QStringLiteral("error"),
            QStringLiteral("-f"), QStringLiteral("lavfi"), QStringLiteral("-i"), QStringLiteral("testsrc2=size=320x180:rate=30"),
            QStringLiteral("-t"), QStringLiteral("2"), QStringLiteral("-c:v"), QStringLiteral("libx264"),
            QDir(m_bundle).filePath(QStringLiteral("screen.mp4"))});
        QVERIFY(makeVideo.waitForFinished(30000));
        QCOMPARE(makeVideo.exitCode(), 0);
    }

    void operationsHistoryAutosaveAndMapping()
    {
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QCOMPARE(editor.sourceWidth(), 320);
        QVERIFY(!editor.hasAudio());
        QVERIFY(!editor.hasDesktopAudio());
        QVERIFY(!editor.hasMicrophoneAudio());
        QVERIFY(!editor.hasCamera());
        QVERIFY(editor.duration() > 1.9);
        editor.seek(1.0);
        QVERIFY(editor.splitAtPlayhead());
        QCOMPARE(editor.clips().size(), 2);
        const QString second = editor.clips()[1].toMap().value(QStringLiteral("id")).toString();
        QVERIFY(editor.trimClip(second, 1.2, 2.0));
        QCOMPARE(editor.clips()[1].toMap().value(QStringLiteral("in")).toDouble(), 1.2);
        editor.undo();
        QCOMPARE(editor.clips()[1].toMap().value(QStringLiteral("in")).toDouble(), 1.0);
        editor.redo();
        QCOMPARE(editor.clips()[1].toMap().value(QStringLiteral("in")).toDouble(), 1.2);
        QVERIFY(editor.setClipSpeed(second, 2.0));
        QCOMPARE(editor.outputToSource(1.2), 1.6);
        QCOMPARE(editor.sourceToOutput(1.6, 1), 1.2);
        QTRY_VERIFY_WITH_TIMEOUT(QFileInfo(QDir(m_bundle).filePath(QStringLiteral("project.json"))).size() > 0, 2000);
    }

    void zoomConstraints()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        const QString id = editor.addZoomAt(0.2, 1.2);
        QVERIFY(!id.isEmpty());
        QVERIFY(editor.moveZoom(id, 1.8));
        QVariantMap zoom;
        for (const auto &entry : editor.zooms()) if (entry.toMap().value(QStringLiteral("id")) == id) zoom = entry.toMap();
        QVERIFY(zoom.value(QStringLiteral("end")).toDouble() <= editor.sourceDuration());
        QVERIFY(!editor.resizeZoom(id, 0.5, 1.0));
        QVERIFY(editor.resizeZoom(id, 0.4, 1.5));
        QVERIFY(editor.setZoomLevel(id, 9.0));
        for (const auto &entry : editor.zooms()) if (entry.toMap().value(QStringLiteral("id")) == id) zoom = entry.toMap();
        QCOMPARE(zoom.value(QStringLiteral("level")).toDouble(), 4.0);
        QVERIFY(editor.removeZoom(id));
    }

    void gradientPresetUsesRequestedIndex()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());

        const auto expectedStops = [&editor](int index) {
            QVariantList result;
            const QVariantList colours = editor.gradients().at(index).toList();
            for (qsizetype i = 0; i < colours.size(); ++i)
                result << QVariant::fromValue(QVariantList{colours.at(i).toString(),
                    i / double(std::max<qsizetype>(1, colours.size() - 1))});
            return result;
        };

        editor.applyGradientPreset(7);
        QVariantMap background = editor.projectMap().value(QStringLiteral("background")).toMap();
        QCOMPARE(background.value(QStringLiteral("type")).toString(), QStringLiteral("gradient"));
        QCOMPARE(background.value(QStringLiteral("gradient")).toMap()
                     .value(QStringLiteral("stops")).toList(), expectedStops(7));

        editor.applyGradientPreset(3);
        background = editor.projectMap().value(QStringLiteral("background")).toMap();
        QCOMPARE(background.value(QStringLiteral("type")).toString(), QStringLiteral("gradient"));
        QCOMPARE(background.value(QStringLiteral("gradient")).toMap()
                     .value(QStringLiteral("stops")).toList(), expectedStops(3));
        QVERIFY(expectedStops(7) != expectedStops(3));

        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omarecord
            Window {
                width: 284; height: 900; visible: true
                BackgroundPanel { anchors.fill: parent }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        const auto findItem = [](QQuickItem *parent, const QString &name) {
            std::function<QQuickItem *(QQuickItem *)> visit = [&](QQuickItem *item) -> QQuickItem * {
                if (item->objectName() == name) return item;
                for (QQuickItem *child : item->childItems())
                    if (QQuickItem *match = visit(child)) return match;
                return nullptr;
            };
            return visit(parent);
        };
        auto *tile7 = findItem(window->contentItem(), QStringLiteral("gradientTile-7"));
        auto *tile3 = findItem(window->contentItem(), QStringLiteral("gradientTile-3"));
        QTRY_VERIFY(tile7);
        QTRY_VERIFY(tile3);

        const auto clickTile = [window](QQuickItem *tile) {
            const QPoint point = tile->mapToScene(QPointF(tile->width() / 2,
                                                         tile->height() / 2)).toPoint();
            QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, point);
        };
        clickTile(tile7);
        QTRY_VERIFY(tile7->property("selected").toBool());
        QVERIFY(!tile3->property("selected").toBool());
        background = editor.projectMap().value(QStringLiteral("background")).toMap();
        QCOMPARE(background.value(QStringLiteral("gradient")).toMap()
                     .value(QStringLiteral("stops")).toList(), expectedStops(7));

        clickTile(tile3);
        QTRY_VERIFY(tile3->property("selected").toBool());
        QVERIFY(!tile7->property("selected").toBool());
        background = editor.projectMap().value(QStringLiteral("background")).toMap();
        QCOMPARE(background.value(QStringLiteral("gradient")).toMap()
                     .value(QStringLiteral("stops")).toList(), expectedStops(3));
    }

    void zoomTrackDragUsesTrackCoordinates()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        while (!editor.zooms().isEmpty())
            QVERIFY(editor.removeZoom(editor.zooms().first().toMap().value(QStringLiteral("id")).toString()));
        const QString id = editor.addZoomAt(0.1, 1.0);
        QVERIFY(!id.isEmpty());
        const double originalStart = editor.zooms().first().toMap().value(QStringLiteral("start")).toDouble();

        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omarecord
            Window {
                width: 600; height: 80; visible: true
                Item { id: focusItem; anchors.fill: parent }
                ZoomTrack { anchors.fill: parent; pixelsPerSecond: 200; focusTarget: focusItem }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        const QString bodyName = QStringLiteral("zoomBody-") + id;
        std::function<QQuickItem *(QQuickItem *)> findItem = [&](QQuickItem *parent) -> QQuickItem * {
            if (parent->objectName() == bodyName) return parent;
            for (QQuickItem *child : parent->childItems())
                if (QQuickItem *match = findItem(child)) return match;
            return nullptr;
        };
        auto *body = findItem(window->contentItem());
        QTRY_VERIFY(body);
        const QPoint start = body->mapToScene(QPointF(body->width() / 2, body->height() / 2)).toPoint();
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, start);
        QTest::mouseMove(window, start + QPoint(80, 0), 20);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, start + QPoint(80, 0));

        QVariantMap moved;
        for (const QVariant &entry : editor.zooms())
            if (entry.toMap().value(QStringLiteral("id")).toString() == id) moved = entry.toMap();
        QVERIFY(!moved.isEmpty());
        QVERIFY(qAbs(moved.value(QStringLiteral("start")).toDouble() - (originalStart + 0.4)) < 0.06);
    }

    void zoomTrackRubberBandSelectsAndDeletesTwo()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        while (!editor.zooms().isEmpty())
            QVERIFY(editor.removeZoom(editor.zooms().first().toMap().value(QStringLiteral("id")).toString()));
        QVERIFY(!editor.addZoomAt(0.2, 1.0).isEmpty());
        QVERIFY(!editor.addZoomAt(1.0, 1.0).isEmpty());
        const int originalCount = editor.zooms().size();

        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omarecord
            Window {
                width: 600; height: 80; visible: true
                Item { id: focusItem; anchors.fill: parent }
                ZoomTrack { anchors.fill: parent; pixelsPerSecond: 200; focusTarget: focusItem }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *selectionArea = window->findChild<QQuickItem *>(QStringLiteral("zoomSelectionArea"));
        QTRY_VERIFY(selectionArea);
        const QPoint start = selectionArea->mapToScene(QPointF(2, selectionArea->height() / 2)).toPoint();
        const QPoint finish = selectionArea->mapToScene(QPointF(420, selectionArea->height() / 2)).toPoint();
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, start);
        QTest::mouseMove(window, finish, 20);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, finish);

        QCOMPARE(editor.selectedZoomIds().size(), 2);
        QCOMPARE(editor.removeSelectedZooms(), 2);
        QCOMPARE(editor.zooms().size(), originalCount - 2);
    }

    void cameraOverlayAppliesRotationAndHorizontalFlip()
    {
        QQmlEngine engine;
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import Omarecord
            CameraOverlay {
                settings: ({
                    enabled: true, position: "bottom-right", size: .25,
                    shape: "rounded", radius: 16, crop: "original",
                    rotation: 90, flipHorizontal: true, scaleDuringZoom: .7,
                    offset: {x: .02, y: .02},
                    shadow: {enabled: false, intensity: .55, blur: 18, distance: 18},
                    inset: {enabled: false, width: 2, color: "white", alpha: .7}
                })
                zoomScale: 1
                outputWidth: 1280; outputHeight: 720
                sourceWidth: 1920; sourceHeight: 1080
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> overlay(component.create());
        QVERIFY(overlay);
        auto *source = overlay->findChild<QQuickItem *>(QStringLiteral("cameraFrameSource"));
        QVERIFY(source);
        QCOMPARE(source->rotation(), 90.0);
        QVERIFY(source->width() < source->height());
        QObject *flip = overlay->findChild<QObject *>(QStringLiteral("cameraFlipTransform"));
        QVERIFY(flip);
        QCOMPARE(flip->property("xScale").toDouble(), -1.0);
    }

    void newProjectUsesCapturedCameraOrientation()
    {
        const QString bundle = m_temporary.filePath(QStringLiteral("camera-default.omarecord"));
        QVERIFY(QDir().mkpath(bundle));
        QVERIFY(QFile::copy(QDir(m_bundle).filePath(QStringLiteral("screen.mp4")),
                            QDir(bundle).filePath(QStringLiteral("screen.mp4"))));
        QVERIFY(QFile::copy(QDir(m_bundle).filePath(QStringLiteral("input.jsonl")),
                            QDir(bundle).filePath(QStringLiteral("input.jsonl"))));
        QVERIFY(QFile::copy(QDir(m_bundle).filePath(QStringLiteral("screen.mp4")),
                            QDir(bundle).filePath(QStringLiteral("camera.mp4"))));
        QFile capture(QDir(bundle).filePath(QStringLiteral("capture.json")));
        QVERIFY(capture.open(QIODevice::WriteOnly));
        capture.write(QJsonDocument(QJsonObject{
            {QStringLiteral("first_frame_us"), 1000000},
            {QStringLiteral("stopped_us"), 3000000},
            {QStringLiteral("camera"), QJsonObject{
                {QStringLiteral("first_frame_us"), 1000000},
                {QStringLiteral("rotation"), 270},
                {QStringLiteral("flipHorizontal"), true}
            }}
        }).toJson());
        capture.close();

        Editor editor(bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        const QVariantMap camera = editor.projectMap().value(QStringLiteral("camera")).toMap();
        QCOMPARE(camera.value(QStringLiteral("rotation")).toInt(), 270);
        QVERIFY(camera.value(QStringLiteral("flipHorizontal")).toBool());
    }

    void exportThroughEditorApi()
    {
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        const QString output = m_temporary.filePath(QStringLiteral("editor-api.mp4"));
        QSignalSpy finished(&editor, &Editor::exportFinished);
        QSignalSpy errors(&editor, &Editor::exportErrorChanged);
        editor.exportTo(output, {{QStringLiteral("fps"), 10}, {QStringLiteral("height"), 180}, {QStringLiteral("quality"), QStringLiteral("web-low")}});
        QTRY_VERIFY_WITH_TIMEOUT(finished.count() > 0 || (!editor.exporting() && !editor.exportError().isEmpty()), 120000);
        QVERIFY2(finished.count() > 0, qPrintable(editor.exportError()));
        QVERIFY(QFileInfo(output).size() > 0);
        Q_UNUSED(errors);
    }

private:
    QTemporaryDir m_temporary;
    QString m_bundle;
};

QTEST_MAIN(EditorTest)
#include "test_editor.moc"
