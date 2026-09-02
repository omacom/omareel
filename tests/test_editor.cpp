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
