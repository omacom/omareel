#include "core/Project.h"

#include <QFile>
#include <QJsonDocument>
#include <QTemporaryDir>
#include <QtTest>

using namespace Omareel;

class ProjectTest : public QObject
{
    Q_OBJECT
private slots:
    void defaultsAndRoundTrip()
    {
        const Project original = Project::defaults(QStringLiteral("Demo"), 12.5);
        const auto json = original.toJson();
        QCOMPARE(json.value("version").toInt(), 1);
        QCOMPARE(json.value("defaultsVersion").toInt(), 2);
        QVERIFY(json.value("aspect").isNull());
        QCOMPARE(json.value("clips").toArray().first().toObject().value("out").toDouble(), 12.5);
        QCOMPARE(original.frame.padding, 0.10);
        QCOMPARE(original.frame.shadow.intensity, 0.25);
        QCOMPARE(original.frame.shadow.blur, 40.0);
        QCOMPARE(original.frame.shadow.distance, 10.0);
        QCOMPARE(original.frame.border.enabled, false);
        QCOMPARE(original.frame.border.width, 7.0);
        QCOMPARE(original.frame.border.color, QColor(QStringLiteral("#000000")));
        QCOMPARE(original.frame.border.alpha, 1.0);
        QCOMPARE(original.zoomStyle.spring.mass, 2.25);
        QCOMPARE(original.cursor.spring.stiffness, 470.0);
        QCOMPARE(original.cursor.clickShrink, 0.8);
        QCOMPARE(original.cursor.style, QStringLiteral("light-arrow"));
        QCOMPARE(original.cursor.clickSound, QStringLiteral("none"));
        QCOMPARE(original.zoomStyle.motionBlur, 0.0);
        QCOMPARE(original.keystrokes.enabled, false);
        QCOMPARE(original.keystrokes.position, QStringLiteral("bottom-center"));
        QCOMPARE(original.keystrokes.size, 1.0);
        QCOMPARE(original.keystrokes.showOnlyShortcuts, true);
        QCOMPARE(original.keystrokes.holdMs, 900);
        QCOMPARE(original.exportSettings.height, 1080);
        QCOMPARE(original.exportSettings.quality, QStringLiteral("social"));
        QCOMPARE(original.exportSettings.gif.height, 480);
        QCOMPARE(original.camera.enabled, false);
        QCOMPARE(original.camera.position, QStringLiteral("bottom-right"));
        QCOMPARE(original.camera.size, 0.25);
        QCOMPARE(original.camera.shape, QStringLiteral("round"));
        QCOMPARE(original.camera.radius, 16.0);
        QCOMPARE(original.camera.crop, QStringLiteral("original"));
        QCOMPARE(original.camera.flipHorizontal, false);
        QCOMPARE(original.camera.rotation, 0);
        QCOMPARE(original.camera.shadow.enabled, false);
        QCOMPARE(original.camera.border.enabled, false);
        QCOMPARE(original.camera.scaleDuringZoom, 0.7);
        QCOMPARE(original.camera.offset, QPointF(0.02, 0.02));
        QVERIFY(allowedAspects().contains(QStringLiteral("3:4")));
        QCOMPARE(allowedClipSpeeds().last(), 24.0);
        QTemporaryDir directory;
        const QString path = directory.filePath(QStringLiteral("project.json"));
        QString error;
        QVERIFY2(original.save(path, &error), qPrintable(error));
        const auto loaded = Project::load(path, &error);
        QVERIFY2(error.isEmpty(), qPrintable(error));
        QCOMPARE(loaded.toJson(), original.toJson());
        const QJsonObject writtenFrame = loaded.toJson().value(QStringLiteral("frame")).toObject();
        QVERIFY(writtenFrame.contains(QStringLiteral("border")));
        QVERIFY(!writtenFrame.contains(QStringLiteral("inset")));
    }

    void migratesLegacyInsetToBorder()
    {
        const Project migrated = Project::fromJson(QJsonObject{
            {QStringLiteral("frame"), QJsonObject{
                {QStringLiteral("inset"), QJsonObject{
                    {QStringLiteral("enabled"), true}, {QStringLiteral("width"), 5.0},
                    {QStringLiteral("color"), QStringLiteral("#123456")},
                    {QStringLiteral("alpha"), 0.6}}}}},
            {QStringLiteral("camera"), QJsonObject{
                {QStringLiteral("inset"), QJsonObject{
                    {QStringLiteral("enabled"), true}, {QStringLiteral("width"), 3.0},
                    {QStringLiteral("color"), QStringLiteral("#abcdef")},
                    {QStringLiteral("alpha"), 0.8}}}}}
        });
        QVERIFY(migrated.frame.border.enabled);
        QCOMPARE(migrated.frame.border.width, 5.0);
        QCOMPARE(migrated.frame.border.color, QColor(QStringLiteral("#123456")));
        QCOMPARE(migrated.frame.border.alpha, 0.6);
        QVERIFY(migrated.camera.border.enabled);
        QCOMPARE(migrated.camera.border.width, 3.0);
        const QJsonObject json = migrated.toJson();
        QVERIFY(json.value(QStringLiteral("frame")).toObject().contains(QStringLiteral("border")));
        QVERIFY(!json.value(QStringLiteral("frame")).toObject().contains(QStringLiteral("inset")));
        QVERIFY(json.value(QStringLiteral("camera")).toObject().contains(QStringLiteral("border")));
        QVERIFY(!json.value(QStringLiteral("camera")).toObject().contains(QStringLiteral("inset")));
    }

    void migratesOnlyUntouchedOldFrameDefaults()
    {
        QTemporaryDir directory;
        QVERIFY(directory.isValid());
        const auto writeAndLoad = [&directory](const QString &name, const QJsonObject &frame) {
            const QString path = directory.filePath(name + QStringLiteral(".json"));
            QFile file(path);
            if (!file.open(QIODevice::WriteOnly)) return Project{};
            file.write(QJsonDocument(QJsonObject{{QStringLiteral("frame"), frame}})
                           .toJson(QJsonDocument::Indented));
            file.close();
            QString error;
            const Project loaded = Project::load(path, &error);
            if (!error.isEmpty()) return Project{};
            return loaded;
        };

        const Project untouched = writeAndLoad(QStringLiteral("untouched"), QJsonObject{
            {QStringLiteral("shadow"), QJsonObject{
                {QStringLiteral("enabled"), true}, {QStringLiteral("intensity"), 0.75},
                {QStringLiteral("blur"), 20.0}, {QStringLiteral("distance"), 25.0}}},
            {QStringLiteral("inset"), QJsonObject{
                {QStringLiteral("enabled"), false}, {QStringLiteral("width"), 0.0},
                {QStringLiteral("color"), QStringLiteral("#000000")},
                {QStringLiteral("alpha"), 0.5}}}});
        QCOMPARE(untouched.defaultsVersion, 2);
        QCOMPARE(untouched.frame.shadow.intensity, 0.25);
        QCOMPARE(untouched.frame.shadow.blur, 40.0);
        QCOMPARE(untouched.frame.shadow.distance, 10.0);
        QCOMPARE(untouched.frame.border.width, 7.0);
        QCOMPARE(untouched.frame.border.alpha, 1.0);

        const Project customised = writeAndLoad(QStringLiteral("customised"), QJsonObject{
            {QStringLiteral("shadow"), QJsonObject{
                {QStringLiteral("enabled"), true}, {QStringLiteral("intensity"), 0.61},
                {QStringLiteral("blur"), 20.0}, {QStringLiteral("distance"), 25.0}}},
            {QStringLiteral("inset"), QJsonObject{
                {QStringLiteral("enabled"), true}, {QStringLiteral("width"), 3.0},
                {QStringLiteral("color"), QStringLiteral("#123456")},
                {QStringLiteral("alpha"), 0.5}}}});
        QCOMPARE(customised.defaultsVersion, 2);
        QCOMPARE(customised.frame.shadow.intensity, 0.61);
        QCOMPARE(customised.frame.shadow.blur, 20.0);
        QCOMPARE(customised.frame.shadow.distance, 25.0);
        QVERIFY(customised.frame.border.enabled);
        QCOMPARE(customised.frame.border.width, 3.0);
        QCOMPARE(customised.frame.border.color, QColor(QStringLiteral("#123456")));
        QCOMPARE(customised.frame.border.alpha, 0.5);

        const Project toggled = writeAndLoad(QStringLiteral("toggled"), QJsonObject{
            {QStringLiteral("shadow"), QJsonObject{
                {QStringLiteral("enabled"), false}, {QStringLiteral("intensity"), 0.75},
                {QStringLiteral("blur"), 20.0}, {QStringLiteral("distance"), 25.0},
                {QStringLiteral("angle"), 90.0}}},
            {QStringLiteral("inset"), QJsonObject{
                {QStringLiteral("enabled"), true}, {QStringLiteral("width"), 0.0},
                {QStringLiteral("color"), QStringLiteral("#000000")},
                {QStringLiteral("alpha"), 0.5}}}});
        QCOMPARE(toggled.defaultsVersion, 2);
        QVERIFY(!toggled.frame.shadow.enabled);
        QCOMPARE(toggled.frame.shadow.intensity, 0.75);
        QCOMPARE(toggled.frame.shadow.blur, 20.0);
        QCOMPARE(toggled.frame.shadow.distance, 25.0);
        QVERIFY(toggled.frame.border.enabled);
        QCOMPARE(toggled.frame.border.width, 0.0);
        QCOMPARE(toggled.frame.border.alpha, 0.5);
    }
};

QTEST_APPLESS_MAIN(ProjectTest)
#include "test_project.moc"
