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
#include <QImage>
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

    void timelineWheelScrollsHorizontally()
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
                width: 400; height: 180; visible: true
                Timeline { anchors.fill: parent; scaleFactor: 5 }
            })", QUrl());
        QVERIFY2(component.isReady(), qPrintable(component.errorString()));
        std::unique_ptr<QObject> object(component.create());
        auto *window = qobject_cast<QQuickWindow *>(object.get());
        QVERIFY(window);
        QVERIFY(QTest::qWaitForWindowExposed(window));
        auto *flickable = window->findChild<QQuickItem *>(QStringLiteral("timelineFlickable"));
        QTRY_VERIFY(flickable);
        QCOMPARE(flickable->property("contentX").toDouble(), 0.0);
        QTest::mouseMove(window, QPoint(220, 90));
        QTest::wheelEvent(window, QPointF(220, 90), QPoint(0, -120));
        QTRY_VERIFY(flickable->property("contentX").toDouble() > 0.0);
        QCOMPARE(flickable->property("contentX").toDouble(), 80.0);
        QVERIFY(flickable->setProperty("contentX", 0.0));
        QTest::wheelEvent(window, QPointF(220, 90), QPoint(-120, 0));
        QTRY_COMPARE(flickable->property("contentX").toDouble(), 80.0);
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
