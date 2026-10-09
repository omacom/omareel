#include "ui/Editor.h"
#include "core/Theme.h"
#include "render/VideoDecoding.h"

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
#include <QImage>
#include <QUrl>
#include <algorithm>
#include <functional>
#include <QVideoFrame>
#include <QVideoSink>
#include <QtTest>

using namespace Omareel;

class EditorTest : public QObject
{
    Q_OBJECT
private slots:
    void initTestCase()
    {
        configureVideoDecoding();
        if (QStandardPaths::findExecutable(QStringLiteral("ffmpeg")).isEmpty()
            || QStandardPaths::findExecutable(QStringLiteral("ffprobe")).isEmpty())
            QSKIP("ffmpeg/ffprobe unavailable");
        QVERIFY(m_temporary.isValid());
        const QString fixture = QFINDTESTDATA("fixtures/synthetic");
        QVERIFY(!fixture.isEmpty());
        m_bundle = m_temporary.filePath(QStringLiteral("editor.omareel"));
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
        QVERIFY(editor.selectedClipId().isEmpty());
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

    void opensLegacyBundleExtension()
    {
        const QString legacyBundle = m_temporary.filePath(QStringLiteral("legacy.omarecord"));
        QVERIFY(QDir().mkpath(legacyBundle));
        for (const QString &file : {QStringLiteral("capture.json"), QStringLiteral("input.jsonl"),
                                    QStringLiteral("screen.mp4")})
            QVERIFY(QFile::copy(QDir(m_bundle).filePath(file), QDir(legacyBundle).filePath(file)));

        Editor editor(legacyBundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QCOMPARE(editor.bundleName(), QStringLiteral("legacy"));
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
            import Omareel
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
            import Omareel
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

    void realTimelineBlockBodiesDrag()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        editor.seek(1.0);
        QVERIFY(editor.splitAtPlayhead());
        const QString clipId = editor.clips().first().toMap().value(QStringLiteral("id")).toString();
        QVERIFY(editor.trimClip(clipId, 0.0, 0.7));
        const double originalClipIn = editor.clips().first().toMap().value(QStringLiteral("in")).toDouble();
        while (!editor.zooms().isEmpty())
            QVERIFY(editor.removeZoom(editor.zooms().first().toMap().value(QStringLiteral("id")).toString()));
        const QString zoomId = editor.addZoomAt(0.1, 1.0);
        QVERIFY(!zoomId.isEmpty());
        const double originalZoomStart = editor.zooms().first().toMap().value(QStringLiteral("start")).toDouble();

        qputenv("OMAREEL_INPUT_TRACE", "1");
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omareel
            Window {
                width: 800; height: 180; visible: true
                Timeline { anchors.fill: parent; scaleFactor: 1 }
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
        auto *clipBlock = findItem(window->contentItem(), QStringLiteral("clipBlock-") + clipId);
        QTRY_VERIFY(clipBlock);
        const auto dragBlock = [window](QQuickItem *block, double fraction,
                                        Qt::KeyboardModifiers modifiers) {
            const QPoint start = block->mapToScene(
                QPointF(block->width() * fraction, block->height() / 2)).toPoint();
            const QPoint finish = start + QPoint(80, 0);
            QTest::mousePress(window, Qt::LeftButton, modifiers, start);
            QTest::mouseMove(window, finish, 20);
            QTest::mouseRelease(window, Qt::LeftButton, modifiers, finish);
        };
        dragBlock(clipBlock, .25, Qt::ControlModifier);
        QTRY_VERIFY(editor.clips().first().toMap().value(QStringLiteral("in")).toDouble()
                    > originalClipIn + 0.05);
        auto *zoomBlock = findItem(window->contentItem(), QStringLiteral("zoomBlock-") + zoomId);
        QTRY_VERIFY(zoomBlock);
        dragBlock(zoomBlock, .5, Qt::NoModifier);
        QVariantMap movedZoom;
        for (const QVariant &entry : editor.zooms())
            if (entry.toMap().value(QStringLiteral("id")).toString() == zoomId)
                movedZoom = entry.toMap();
        QVERIFY(movedZoom.value(QStringLiteral("start")).toDouble() > originalZoomStart + 0.05);
        qunsetenv("OMAREEL_INPUT_TRACE");
    }

    void clipHandleHoverKeepsAccentBounds()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        const QString clipId = editor.clips().first().toMap().value(QStringLiteral("id")).toString();
        editor.setSelectedClipId(clipId);
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omareel
            Window {
                width: 600; height: 80; visible: true; color: theme.surface
                Item { id: focusItem; anchors.fill: parent }
                ClipTrack { anchors.fill: parent; pixelsPerSecond: 200; focusTarget: focusItem }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        const QString handleName = QStringLiteral("clipTrimHandle-") + clipId
            + QStringLiteral("-right");
        std::function<QQuickItem *(QQuickItem *)> findHandle = [&](QQuickItem *item) -> QQuickItem * {
            if (item->objectName() == handleName) return item;
            for (QQuickItem *child : item->childItems())
                if (QQuickItem *match = findHandle(child)) return match;
            return nullptr;
        };
        auto *handle = findHandle(window->contentItem());
        QTRY_VERIFY(handle);
        QTest::qWait(180);
        const QImage normal = window->grabWindow();
        QVERIFY(!normal.isNull());
        QVERIFY(handle->setProperty("debugHovered", true));
        QTest::qWait(180);
        const QImage hovered = window->grabWindow();
        QVERIFY(!hovered.isNull());

        const QRect sample = QRect(handle->mapToScene(QPointF()).toPoint(),
                                   QSize(qRound(handle->width()), qRound(handle->height())))
                                 .adjusted(-1, -1, 1, 1);
        const QColor accent = theme.accent();
        const auto accentBounds = [sample, accent](const QImage &image) {
            QRect bounds;
            const int accentHue = accent.hsvHue();
            for (int y = std::max(0, sample.top()); y <= std::min(image.height() - 1, sample.bottom()); ++y) {
                for (int x = std::max(0, sample.left()); x <= std::min(image.width() - 1, sample.right()); ++x) {
                    const QColor pixel = image.pixelColor(x, y);
                    const int hueDistance = std::abs(pixel.hsvHue() - accentHue);
                    if (pixel.hsvSaturationF() < accent.hsvSaturationF() * 0.45
                        || std::min(hueDistance, 360 - hueDistance) > 12) continue;
                    bounds |= QRect(x, y, 1, 1);
                }
            }
            return bounds;
        };
        const QRect normalBounds = accentBounds(normal);
        const QRect hoverBounds = accentBounds(hovered);
        qInfo().noquote() << QStringLiteral("handle accent bounds normal=%1,%2 %3x%4 hover=%5,%6 %7x%8")
            .arg(normalBounds.x()).arg(normalBounds.y())
            .arg(normalBounds.width()).arg(normalBounds.height())
            .arg(hoverBounds.x()).arg(hoverBounds.y())
            .arg(hoverBounds.width()).arg(hoverBounds.height());
        QVERIFY(normalBounds.isValid());
        QCOMPARE(hoverBounds, normalBounds);
    }

    void timelineWheelZoomsAroundPointerAndRightDragPans()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omareel
            Window {
                id: testWindow
                width: 400; height: 180; visible: true
                property real timelineScale: 1
                Timeline {
                    objectName: "timeline"
                    anchors.fill: parent
                    scaleFactor: testWindow.timelineScale
                    onScaleFactorRequested: value => testWindow.timelineScale = value
                }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *timeline = window->findChild<QQuickItem *>(QStringLiteral("timeline"));
        QTRY_VERIFY(timeline);
        auto *flickable = window->findChild<QQuickItem *>(QStringLiteral("timelineFlickable"));
        QTRY_VERIFY(flickable);
        QCOMPARE(flickable->property("contentX").toDouble(), 0.0);
        QVERIFY(std::abs(flickable->property("contentWidth").toDouble() - flickable->width()) < .01);
        const double pointerX = 220.0 - flickable->x();
        const auto timeAtPointer = [&] {
            return (flickable->property("contentX").toDouble() + pointerX)
                / timeline->property("pixelsPerSecond").toDouble();
        };
        const double beforeZoom = timeAtPointer();
        QTest::mouseMove(window, QPoint(220, 90));
        QTest::wheelEvent(window, QPointF(220, 90), QPoint(0, 120));
        QTRY_VERIFY(std::abs(timeline->property("scaleFactor").toDouble() - 1.25) < .0001);
        QTRY_VERIFY(std::abs(timeAtPointer() - beforeZoom) < .02);
        QTest::wheelEvent(window, QPointF(220, 90), QPoint(120, 0));
        QTRY_VERIFY(std::abs(timeline->property("scaleFactor").toDouble() - 1.5625) < .0001);
        QTRY_VERIFY(std::abs(timeAtPointer() - beforeZoom) < .02);

        const double beforePan = flickable->property("contentX").toDouble();
        QTest::mousePress(window, Qt::RightButton, Qt::NoModifier, QPoint(220, 90));
        QTest::mouseMove(window, QPoint(190, 90), 5);
        QTest::mouseMove(window, QPoint(170, 90), 5);
        QTest::mouseRelease(window, Qt::RightButton, Qt::NoModifier, QPoint(170, 90));
        QTRY_VERIFY(flickable->property("contentX").toDouble() > beforePan + 20);

        QTest::wheelEvent(window, QPointF(220, 90), QPoint(0, -120));
        QTest::wheelEvent(window, QPointF(220, 90), QPoint(0, -120));
        QTRY_VERIFY(std::abs(timeline->property("scaleFactor").toDouble() - 1) < .0001);
        QTRY_VERIFY(std::abs(flickable->property("contentWidth").toDouble()
            - flickable->width()) < .01);
        QTRY_VERIFY(std::abs(flickable->property("contentX").toDouble()) < .01);

        const double centerTime = (flickable->property("contentX").toDouble()
            + flickable->width() / 2) / timeline->property("pixelsPerSecond").toDouble();
        QVERIFY(window->setProperty("timelineScale", 16.0));
        QTRY_VERIFY(std::abs(timeline->property("scaleFactor").toDouble() - 16) < .0001);
        QTRY_VERIFY(std::abs((flickable->property("contentX").toDouble()
            + flickable->width() / 2) / timeline->property("pixelsPerSecond").toDouble()
            - centerTime) < .02);
    }

    void timelineZoomButtonsKeepSliderInSync()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import Omareel
            Main { width: 1200; height: 800; visible: true })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *slider = window->findChild<QQuickItem *>(QStringLiteral("timelineZoomSlider"));
        auto *zoomIn = window->findChild<QQuickItem *>(QStringLiteral("timelineZoomIn"));
        auto *zoomOut = window->findChild<QQuickItem *>(QStringLiteral("timelineZoomOut"));
        QTRY_VERIFY(slider && zoomIn && zoomOut);
        const auto center = [](QQuickItem *item) {
            return item->mapToScene(QPointF(item->width() / 2, item->height() / 2)).toPoint();
        };

        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, center(zoomIn));
        QTRY_VERIFY(std::abs(window->property("timelineScale").toDouble() - 1.25) < .001);
        QTRY_VERIFY(std::abs(slider->property("value").toDouble() - 1.25) < .001);
        QVERIFY(window->setProperty("timelineScale", 2.0));
        QTRY_VERIFY(std::abs(slider->property("value").toDouble() - 2.0) < .001);
        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, center(zoomOut));
        QTRY_VERIFY(std::abs(window->property("timelineScale").toDouble() - 1.6) < .001);
        QTRY_VERIFY(std::abs(slider->property("value").toDouble() - 1.6) < .001);
    }

    void timelineScrubPausesAndSeeksToLastPosition()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Window
            import Omareel
            Window {
                width: 800; height: 180; visible: true
                Timeline { objectName: "timeline"; anchors.fill: parent; scaleFactor: 1 }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *timeline = window->findChild<QQuickItem *>(QStringLiteral("timeline"));
        QTRY_VERIFY(timeline);
        const double pixelsPerSecond = timeline->property("pixelsPerSecond").toDouble();
        QVERIFY(pixelsPerSecond > 0);

        editor.play();
        QTRY_VERIFY(editor.playing());
        const QPoint rulerStart(72 + qRound(.3 * pixelsPerSecond), 16);
        const QPoint rulerEnd(72 + qRound(1.4 * pixelsPerSecond), 16);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, rulerStart);
        QVERIFY(!editor.playing());
        QVERIFY(qAbs(editor.position() - .3) < .02);
        QTest::mouseMove(window, rulerEnd, 1);
        QVERIFY(qAbs(editor.position() - 1.4) < .02);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, rulerEnd);
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 1400) < 35);

        const QPoint clipClick(72 + qRound(.7 * pixelsPerSecond), 56);
        const double originalClipIn = editor.clips().first().toMap().value(QStringLiteral("in")).toDouble();
        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, clipClick);
        QVERIFY(qAbs(editor.position() - .7) < .02);
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 700) < 35);
        editor.play();
        QTRY_VERIFY(editor.playing());
        const QPoint clipDragEnd(72 + qRound(1.1 * pixelsPerSecond), 56);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, clipClick);
        QVERIFY(!editor.playing());
        QTest::mouseMove(window, clipDragEnd, 1);
        QVERIFY(qAbs(editor.position() - 1.1) < .02);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, clipDragEnd);
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 1100) < 35);
        QCOMPARE(editor.clips().first().toMap().value(QStringLiteral("in")).toDouble(), originalClipIn);

        const QPoint badgeStart(72 + qRound(1.0 * pixelsPerSecond), 56);
        const QPoint badgeEnd(72 + qRound(1.15 * pixelsPerSecond), 56);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, badgeStart);
        QTest::mouseMove(window, badgeEnd, 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, badgeEnd);
        QVERIFY(qAbs(editor.position() - 1.15) < .02);
        QCOMPARE(editor.clips().first().toMap().value(QStringLiteral("in")).toDouble(), originalClipIn);

        const QPoint emptyClipClick(72 + qRound(editor.duration() * pixelsPerSecond) + 10, 56);
        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, emptyClipClick);
        QVERIFY(qAbs(editor.position() - editor.duration()) < .001);

        editor.seek(0);
        const qint64 decoderBeforeDrag = editor.playerPositionMsForTests();
        editor.beginScrub();
        editor.scrubTo(.2);
        editor.scrubTo(.8);
        editor.scrubTo(1.2);
        QVERIFY(qAbs(editor.position() - 1.2) < .001);
        QCOMPARE(editor.playerPositionMsForTests(), decoderBeforeDrag);
        editor.endScrub();
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 1200) < 35);

        editor.seek(editor.duration());
        editor.beginScrub();
        editor.scrubTo(.5);
        editor.endScrub();
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 500) < 35);
        QVERIFY(qAbs(editor.position() - .5) < .001);
    }

    void shiftDragCutsMiddleAsOneUndoableEdit()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        while (!editor.zooms().isEmpty())
            QVERIFY(editor.removeZoom(editor.zooms().first().toMap().value(QStringLiteral("id")).toString()));
        const QString zoomId = editor.addZoomAt(.9, 1.1);
        QVERIFY(!zoomId.isEmpty());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import Omareel
            Main { width: 1200; height: 800; visible: true })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *timeline = window->findChild<QQuickItem *>(QStringLiteral("editorTimeline"));
        QTRY_VERIFY(timeline);
        const double pixelsPerSecond = timeline->property("pixelsPerSecond").toDouble();
        const QPoint start = timeline->mapToScene(
            QPointF(72 + .4 * pixelsPerSecond, 56)).toPoint();
        const QPoint end = timeline->mapToScene(
            QPointF(72 + 1.3 * pixelsPerSecond, 56)).toPoint();
        QTest::mousePress(window, Qt::LeftButton, Qt::ShiftModifier, start);
        QTest::mouseMove(window, end, 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::ShiftModifier, end);
        QVERIFY(timeline->property("hasRange").toBool());
        auto *selection = window->findChild<QQuickItem *>(QStringLiteral("clipRangeSelection"));
        QVERIFY(selection);
        QVERIFY(selection->isVisible());
        QVERIFY(qAbs(timeline->property("rangeStart").toDouble() - .4) < .02);
        QVERIFY(qAbs(timeline->property("rangeEnd").toDouble() - 1.3) < .02);
        QVERIFY(editor.hasPlaybackRange());
        QVERIFY(qAbs(editor.position() - .4) < .02);

        QTest::keyClick(window, Qt::Key_Delete);
        QTRY_VERIFY(qAbs(editor.duration() - 1.1) < .03);
        QVERIFY(!timeline->property("hasRange").toBool());
        QVERIFY(!editor.hasPlaybackRange());
        QCOMPARE(editor.clips().size(), 2);
        QVERIFY(qAbs(editor.duration() - 1.1) < .03);
        QCOMPARE(editor.zooms().size(), 1);
        QVERIFY(qAbs(editor.zooms().first().toMap().value(QStringLiteral("start")).toDouble() - 1.3) < .03);
        QVERIFY(editor.sourceToOutput(editor.zooms().first().toMap()
            .value(QStringLiteral("start")).toDouble()) >= 0);
        QVERIFY(qAbs(editor.outputToSource(editor.clips().first().toMap()
            .value(QStringLiteral("out")).toDouble()) - 1.3) < .03);
        QVERIFY(editor.canUndo());
        editor.undo();
        QCOMPARE(editor.clips().size(), 1);
        QVERIFY(qAbs(editor.duration() - 2.0) < .03);
        QVERIFY(!editor.selectedClipId().isEmpty());
        QCOMPARE(editor.selectedClipId(), editor.clips().first().toMap().value(QStringLiteral("id")).toString());
        QTRY_VERIFY(qAbs(editor.playerPositionMsForTests() - 400) < 35);
        QVERIFY(qAbs(editor.zooms().first().toMap().value(QStringLiteral("start")).toDouble() - .9) < .03);
        editor.redo();
        QCOMPARE(editor.clips().size(), 2);
        QVERIFY(qAbs(editor.duration() - 1.1) < .03);
        QVERIFY(editor.saveNow());
        Editor reopened(m_bundle);
        QVERIFY(reopened.isValid());
        QCOMPARE(reopened.clips().size(), 2);
        QVERIFY(qAbs(reopened.duration() - 1.1) < .03);
    }

    void videoThumbnailsFollowSourceTimes()
    {
        const QString thumbnailBundle = m_temporary.filePath(QStringLiteral("thumbnails.omareel"));
        QVERIFY(QDir().mkpath(thumbnailBundle));
        for (const QString &file : {QStringLiteral("screen.mp4"),
                                    QStringLiteral("capture.json"), QStringLiteral("input.jsonl")})
            QVERIFY(QFile::copy(QDir(m_bundle).filePath(file), QDir(thumbnailBundle).filePath(file)));
        Editor editor(thumbnailBundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QString firstUrl;
        QTRY_VERIFY_WITH_TIMEOUT(!(firstUrl = editor.videoThumbnailUrl(.25)).isEmpty(), 30000);
        const QImage first(QUrl(firstUrl).toLocalFile());
        QCOMPARE(first.size(), QSize(160, 90));
        QCOMPARE(editor.videoThumbnailUrlForClip(.4, 0, .5), firstUrl);
        QString secondUrl;
        QTRY_VERIFY_WITH_TIMEOUT((secondUrl = editor.videoThumbnailUrl(1.25),
            QFileInfo(QUrl(secondUrl).toLocalFile()).completeBaseName().toInt() == 1300), 30000);
        QVERIFY(secondUrl != firstUrl);
        const QImage second(QUrl(secondUrl).toLocalFile());
        QCOMPARE(second.size(), QSize(160, 90));
        QVERIFY(first != second);
        QString tightUrl;
        QTRY_VERIFY_WITH_TIMEOUT(!(tightUrl = editor.videoThumbnailUrlForClip(.17, .137, .181)).isEmpty(), 30000);
        const int tightSample = QFileInfo(QUrl(tightUrl).toLocalFile()).completeBaseName().toInt();
        QVERIFY(tightSample >= 137 && tightSample <= 180);
        QCOMPARE(QImage(QUrl(tightUrl).toLocalFile()).size(), QSize(160, 90));
    }

    void playbackStaysWithinSelectedRange()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QVideoSink sink;
        QObject holder;
        holder.setProperty("videoSink", QVariant::fromValue(&sink));
        editor.attachVideoOutput(&holder);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playerMediaStatusForTests() >= QMediaPlayer::LoadedMedia, 15000);
        editor.setPlaybackRange(.4, .9);
        QVERIFY(editor.hasPlaybackRange());
        editor.seek(0);
        QVERIFY(qAbs(editor.position() - .4) < .002);
        editor.seek(1.5);
        QVERIFY(qAbs(editor.position() - .899) < .005);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(editor.playing(), 5000);
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 10000);
        QVERIFY(editor.position() >= .89 && editor.position() < .9);
        QVERIFY(editor.playheadPosition() >= .89 && editor.playheadPosition() < .9);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(editor.playing(), 5000);
        QVERIFY(editor.position() >= .4 && editor.position() < .9);
        editor.pause();
        editor.clearPlaybackRange();
        editor.seek(0);
        QVERIFY(qAbs(editor.position()) < .002);

        editor.seek(1);
        QVERIFY(editor.splitAtPlayhead());
        editor.setPlaybackRange(.8, 1.3);
        editor.seek(.8);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(editor.playing(), 5000);
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 10000);
        QVERIFY(editor.position() >= 1.28 && editor.position() < 1.3);
        QVERIFY(editor.playheadPosition() >= 1.28 && editor.playheadPosition() < 1.3);
    }

    void playRequestedDuringWarmUpKeepsPlaying()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QVideoSink sink;
        QObject holder;
        holder.setProperty("videoSink", QVariant::fromValue(&sink));
        // No event has run yet, so the opening preview cannot have delivered its first frame.
        editor.attachVideoOutput(&holder);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(editor.position() >= .5, 10000);
        QVERIFY(editor.playing());
    }

    void playheadAdvancesWithIdleQuickWindow()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        QVideoSink sink;
        QQuickWindow window;
        window.resize(320, 180);
        QQuickItem output(window.contentItem());
        output.setProperty("videoSink", QVariant::fromValue(&sink));
        window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        editor.attachVideoOutput(&output);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playerMediaStatusForTests() >= QMediaPlayer::LoadedMedia, 15000);

        editor.seek(.2);
        QVERIFY(std::abs(editor.playheadPosition() - .2) < .002);
        QSignalSpy playheadUpdates(&editor, &Editor::playheadPositionChanged);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(editor.playheadPosition() >= .4, 5000);
        QVERIFY(playheadUpdates.count() >= 5);
        editor.pause();
        QVERIFY(std::abs(editor.playheadPosition() - editor.position()) < .002);
        editor.seek(1.0);
        QVERIFY(std::abs(editor.playheadPosition() - 1.0) < .002);
    }

    void spaceStartsPlaybackWhenEditorOpens()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import Omareel
            Main { width: 1200; height: 800; visible: true })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        QTRY_VERIFY_WITH_TIMEOUT(!editor.loading() && !editor.playing(), 15000);
        QVERIFY(!window->property("singleKeyShortcutsBlocked").toBool());
        QTest::keyClick(window, Qt::Key_Space);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playing(), 3000);
        QTest::keyClick(window, Qt::Key_Space);
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 3000);
    }

    void backspaceDeletesOnlySelectedRange()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import Omareel
            Main { width: 1200; height: 800; visible: true })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *timeline = window->findChild<QQuickItem *>(QStringLiteral("editorTimeline"));
        QTRY_VERIFY(timeline);

        const QString clipId = editor.clips().first().toMap().value(QStringLiteral("id")).toString();
        editor.setSelectedClipId(clipId);
        QTest::keyClick(window, Qt::Key_Backspace);
        QCOMPARE(editor.selectedClipId(), clipId);
        QVERIFY(qAbs(editor.duration() - 2.0) < .02);

        const double pps = timeline->property("pixelsPerSecond").toDouble();
        const QPoint start = timeline->mapToScene(QPointF(72 + .4 * pps, 56)).toPoint();
        const QPoint end = timeline->mapToScene(QPointF(72 + 1.3 * pps, 56)).toPoint();
        QTest::mousePress(window, Qt::LeftButton, Qt::ShiftModifier, start);
        QTest::mouseMove(window, end, 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::ShiftModifier, end);
        QVERIFY(timeline->property("hasRange").toBool());
        QTest::keyClick(window, Qt::Key_Backspace);
        QTRY_VERIFY(qAbs(editor.duration() - 1.1) < .03);
        QVERIFY(!timeline->property("hasRange").toBool());
        QVERIFY(!editor.hasPlaybackRange());
    }

    void timelineRangeHandlesPreviewAndCtrlSelect()
    {
        QFile::remove(QDir(m_bundle).filePath(QStringLiteral("project.json")));
        Editor editor(m_bundle);
        QVERIFY2(editor.isValid(), qPrintable(editor.errorString()));
        editor.seek(1);
        QVERIFY(editor.splitAtPlayhead());
        const QString firstId = editor.clips().first().toMap().value(QStringLiteral("id")).toString();
        const QString secondId = editor.clips().last().toMap().value(QStringLiteral("id")).toString();
        QVERIFY(editor.trimClip(firstId, 0, .83));
        editor.setSelectedClipId(QString());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("comp"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import Omareel
            Main { width: 1200; height: 800; visible: true })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *timeline = window->findChild<QQuickItem *>(QStringLiteral("editorTimeline"));
        QTRY_VERIFY(timeline);
        const double pps = timeline->property("pixelsPerSecond").toDouble();
        std::function<QQuickItem *(QQuickItem *, const QString &)> findVisualItem =
            [&](QQuickItem *item, const QString &name) -> QQuickItem * {
                if (item->objectName() == name) return item;
                for (QQuickItem *child : item->childItems())
                    if (QQuickItem *match = findVisualItem(child, name)) return match;
                return nullptr;
            };
        const auto scenePoint = [timeline, pps](double time, double y) {
            return timeline->mapToScene(QPointF(72 + time * pps, y)).toPoint();
        };
        auto *firstBlock = findVisualItem(window->contentItem(), QStringLiteral("clipBlock-") + firstId);
        auto *secondBlock = findVisualItem(window->contentItem(), QStringLiteral("clipBlock-") + secondId);
        QVERIFY(firstBlock && secondBlock);
        QVERIFY(qAbs(firstBlock->width() - .83 * pps) < .01);
        QVERIFY(qAbs(secondBlock->x() - firstBlock->x() - firstBlock->width()) < .01);
        auto *firstTrim = findVisualItem(window->contentItem(),
            QStringLiteral("clipTrimHandle-") + firstId + QStringLiteral("-left"));
        QVERIFY(firstTrim);
        QTest::mouseMove(window, scenePoint(.4, 56));
        QVERIFY(!firstTrim->isVisible());
        const double beforeSelection = editor.position();
        QTest::mouseClick(window, Qt::LeftButton, Qt::ControlModifier, scenePoint(.4, 56));
        QCOMPARE(editor.selectedClipId(), firstId);
        QVERIFY(firstTrim->isVisible());
        QVERIFY(qAbs(editor.position() - beforeSelection) < .002);
        QTest::qWait(800);
        window->grabWindow().save(QStringLiteral("/tmp/omareel-clip-fallback.png"));
        QTest::mouseClick(window, Qt::LeftButton, Qt::ControlModifier, scenePoint(.4, 56));
        QVERIFY(editor.selectedClipId().isEmpty());
        QVERIFY(!firstTrim->isVisible());
        QTest::mouseClick(window, Qt::LeftButton, Qt::ControlModifier, scenePoint(.4, 56));
        QCOMPARE(editor.selectedClipId(), firstId);

        QTest::mousePress(window, Qt::LeftButton, Qt::ShiftModifier, scenePoint(.35, 56));
        QTest::mouseMove(window, scenePoint(1.25, 56), 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::ShiftModifier, scenePoint(1.25, 56));
        QVERIFY(timeline->property("hasRange").toBool());
        QVERIFY(editor.hasPlaybackRange());
        QVERIFY(editor.selectedClipId().isEmpty());
        auto *right = window->findChild<QQuickItem *>(QStringLiteral("rangeTrimHandle-right"));
        auto *left = window->findChild<QQuickItem *>(QStringLiteral("rangeTrimHandle-left"));
        QVERIFY(right && left);
        const QPoint rightStart = right->mapToScene(QPointF(7, right->height() / 2)).toPoint();
        const QPoint rightFinish = rightStart + QPoint(qRound(.2 * pps), 0);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, rightStart);
        QTest::mouseMove(window, rightFinish, 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, rightFinish);
        QVERIFY(qAbs(timeline->property("rangeEnd").toDouble() - 1.45) < .04);
        QVERIFY(qAbs(editor.position() - timeline->property("rangeEnd").toDouble()) < .01);
        const QPoint leftStart = left->mapToScene(QPointF(7, left->height() / 2)).toPoint();
        const QPoint leftFinish = leftStart - QPoint(qRound(.15 * pps), 0);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, leftStart);
        QTest::mouseMove(window, leftFinish, 1);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, leftFinish);
        QVERIFY(qAbs(timeline->property("rangeStart").toDouble() - .2) < .04);
        QVERIFY(qAbs(editor.position() - timeline->property("rangeStart").toDouble()) < .01);
        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, scenePoint(0, 16));
        QVERIFY(qAbs(editor.position() - timeline->property("rangeStart").toDouble()) < .01);
        QTest::mouseClick(window, Qt::LeftButton, Qt::NoModifier, scenePoint(1.8, 16));
        QVERIFY(qAbs(editor.position() - timeline->property("rangeEnd").toDouble()) < .01);
        QTest::keyClick(window, Qt::Key_Space);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playing(), 5000);
        QVERIFY(editor.position() >= timeline->property("rangeStart").toDouble());
        QTest::keyClick(window, Qt::Key_Space);
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 5000);
        QTest::keyClick(window, Qt::Key_Escape);
        QVERIFY(!timeline->property("hasRange").toBool());
        QVERIFY(!editor.hasPlaybackRange());

        QTest::mouseClick(window, Qt::LeftButton, Qt::ControlModifier, scenePoint(1.5, 56));
        QCOMPARE(editor.selectedClipId(), secondId);
        auto *trim = findVisualItem(window->contentItem(),
            QStringLiteral("clipTrimHandle-") + secondId + QStringLiteral("-left"));
        QVERIFY(trim);
        const double originalIn = editor.clips().last().toMap().value(QStringLiteral("in")).toDouble();
        const QPoint trimStart = trim->mapToScene(QPointF(7, trim->height() / 2)).toPoint();
        const QPoint trimFinish = trimStart + QPoint(qRound(.12 * pps), 0);
        QTest::mousePress(window, Qt::LeftButton, Qt::NoModifier, trimStart);
        QTest::mouseMove(window, trimFinish, 1);
        QVERIFY(secondBlock->property("displayedIn").toDouble() > originalIn + .07);
        QTest::mouseRelease(window, Qt::LeftButton, Qt::NoModifier, trimFinish);
        QVERIFY(editor.clips().last().toMap().value(QStringLiteral("in")).toDouble() > originalIn + .07);
        QTest::keyClick(window, Qt::Key_Escape);
        QVERIFY(editor.selectedClipId().isEmpty());
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
            import Omareel
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
            import Omareel
            CameraOverlay {
                settings: ({
                    enabled: true, position: "bottom-right", size: .25,
                    shape: "rounded", radius: 16, crop: "original",
                    rotation: 90, flipHorizontal: true, scaleDuringZoom: .7,
                    offset: {x: .02, y: .02},
                    shadow: {enabled: false, intensity: .55, blur: 18, distance: 18},
                    border: {enabled: false, width: 2, color: "white", alpha: .7}
                })
                zoomScale: 1
                outputWidth: 1280; outputHeight: 720
                sourceWidth: 1920; sourceHeight: 1080
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> overlay(component.create());
        QVERIFY(overlay);
        auto *rotated = overlay->findChild<QQuickItem *>(QStringLiteral("cameraRotated"));
        QVERIFY(rotated);
        auto *source = overlay->findChild<QQuickItem *>(QStringLiteral("cameraFrameSource"));
        QVERIFY(source);
        QCOMPARE(rotated->rotation(), 90.0);
        QVERIFY(rotated->width() > rotated->height());
        QCOMPARE(rotated->width(), overlay->property("height").toDouble());
        QCOMPARE(rotated->height(), overlay->property("width").toDouble());
        QCOMPARE(source->width(), rotated->width());
        QCOMPARE(source->height(), rotated->height());
        QObject *flip = overlay->findChild<QObject *>(QStringLiteral("cameraFlipTransform"));
        QVERIFY(flip);
        QCOMPARE(flip->property("xScale").toDouble(), -1.0);
    }

    void newProjectUsesCapturedCameraOrientation()
    {
        const QString bundle = m_temporary.filePath(QStringLiteral("camera-default.omareel"));
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

    // Regression: after the player reaches the end of the media, seeking must still land the
    // player in a paused state at the requested position (previously it stayed stopped at the
    // end and the preview kept showing the final black frame while scrubbing).
    void seekAndScrubAfterEndOfMediaResumePausedDecoding()
    {
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        // Attach a sink up front (as the editor window does) so playback really decodes and
        // runs off the end the way it does in the app.
        QVideoSink sink;
        QObject holder;
        holder.setProperty("videoSink", QVariant::fromValue(&sink));
        editor.attachVideoOutput(&holder);
        int framesAfterSeek = 0;
        bool counting = false;
        QObject::connect(&sink, &QVideoSink::videoFrameChanged, &sink, [&](const QVideoFrame &f) {
            if (counting && f.isValid()) ++framesAfterSeek;
        });
        QTRY_VERIFY_WITH_TIMEOUT(editor.playerMediaStatusForTests() >= QMediaPlayer::LoadedMedia, 15000);
        QTRY_VERIFY_WITH_TIMEOUT(editor.duration() > 1.0, 15000);
        editor.seek(editor.duration() - 0.3);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 15000);          // ran off the end
        counting = true;
        editor.seek(0.5);
        QTRY_VERIFY_WITH_TIMEOUT(framesAfterSeek > 0, 5000);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playerPlaybackStateForTests() == QMediaPlayer::PausedState, 5000);
        QVERIFY(qAbs(editor.position() - 0.5) < 0.2);
        QVERIFY(editor.playerMediaStatusForTests() != QMediaPlayer::EndOfMedia);

        editor.seek(editor.duration() - 0.3);
        editor.play();
        QTRY_VERIFY_WITH_TIMEOUT(!editor.playing(), 15000);
        framesAfterSeek = 0;
        editor.beginScrub();
        editor.scrubTo(0.5);
        editor.endScrub();
        QTRY_VERIFY_WITH_TIMEOUT(framesAfterSeek > 0, 5000);
        QTRY_VERIFY_WITH_TIMEOUT(editor.playerPlaybackStateForTests() == QMediaPlayer::PausedState, 5000);
        QVERIFY(qAbs(editor.position() - 0.5) < 0.2);
    }

    void exportThroughEditorApi()
    {
        Editor editor(m_bundle);
        QVERIFY(editor.isValid());
        Theme theme;
        QQmlEngine engine;
        engine.rootContext()->setContextProperty(QStringLiteral("editor"), &editor);
        engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
        QQmlComponent component(&engine);
        component.setData(R"(
            import QtQuick
            import QtQuick.Controls
            import Omareel
            ApplicationWindow {
                width: 800; height: 950; visible: true
                ExportDialog { Component.onCompleted: open() }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *status = object->findChild<QObject *>(QStringLiteral("exportStatusLabel"));
        auto *bar = object->findChild<QObject *>(QStringLiteral("exportProgressBar"));
        QVERIFY(status);
        QVERIFY(bar);
        bool sawFinishing = false;
        bool prematureCompletion = false;
        connect(&editor, &Editor::exportProgressChanged, this, [&] {
            if (!editor.exporting()) return;
            prematureCompletion |= bar->property("value").toDouble() >= 1.0
                || status->property("text").toString() == QLatin1String("100%");
            if (editor.exportProgress() >= 1.0)
                sawFinishing = status->property("text").toString() == QStringLiteral("Finishing encoding…");
        });
        const QString output = m_temporary.filePath(QStringLiteral("editor-api.mp4"));
        QSignalSpy finished(&editor, &Editor::exportFinished);
        QSignalSpy errors(&editor, &Editor::exportErrorChanged);
        editor.exportTo(output, {{QStringLiteral("fps"), 10}, {QStringLiteral("height"), 180}, {QStringLiteral("quality"), QStringLiteral("web-low")}});
        QTRY_VERIFY_WITH_TIMEOUT(finished.count() > 0 || (!editor.exporting() && !editor.exportError().isEmpty()), 120000);
        QVERIFY2(finished.count() > 0, qPrintable(editor.exportError()));
        QVERIFY(QFileInfo(output).size() > 0);
        QVERIFY(sawFinishing);
        QVERIFY(!prematureCompletion);
        QCOMPARE(bar->property("value").toDouble(), 1.0);
        Q_UNUSED(errors);
    }

private:
    QTemporaryDir m_temporary;
    QString m_bundle;
};

QTEST_MAIN(EditorTest)
#include "test_editor.moc"
