// project: Project::load / save / fromJson / toJson / migrateDefaults / defaults and the
// free helpers in Project.h.

#include "common.h"

#include "core/Project.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>

#include <cmath>
#include <cstdio>
#include <limits>
#include <sys/stat.h>
#include <unistd.h>

namespace Oracle {

using Omareel::Project;

namespace {

struct FileState {
    bool exists = false;
    ino_t inode = 0;
    mode_t mode = 0;
};

FileState fileState(const QString &path)
{
    struct stat info {};
    if (::stat(QFile::encodeName(path).constData(), &info) != 0) return {};
    return {true, info.st_ino, mode_t(info.st_mode & 07777)};
}

QByteArray octal(mode_t mode)
{
    char buffer[16];
    std::snprintf(buffer, sizeof buffer, "%04o", unsigned(mode));
    return buffer;
}

void field(Text &out, const QByteArray &path, const QByteArray &value)
{
    out.row({path, value});
}

void shadowFields(Text &out, const QByteArray &prefix, const Omareel::Shadow &s)
{
    field(out, prefix + ".enabled", boolText(s.enabled));
    field(out, prefix + ".intensity", num(s.intensity));
    field(out, prefix + ".blur", num(s.blur));
    field(out, prefix + ".distance", num(s.distance));
    field(out, prefix + ".angle", num(s.angle));
}

void borderFields(Text &out, const QByteArray &prefix, const Omareel::Border &b)
{
    field(out, prefix + ".enabled", boolText(b.enabled));
    field(out, prefix + ".width", num(b.width));
    field(out, prefix + ".color", colorText(b.color));
    field(out, prefix + ".alpha", num(b.alpha));
}

void springFields(Text &out, const QByteArray &prefix, const Omareel::Spring &s)
{
    field(out, prefix + ".mass", num(s.mass));
    field(out, prefix + ".stiffness", num(s.stiffness));
    field(out, prefix + ".damping", num(s.damping));
}

// Every member of Project in declaration order, including what toJson() drops (colour alpha
// and 16-bit channels, Clip::isMedia()).
void dumpProject(Text &out, const Project &p)
{
    field(out, "version", QByteArray::number(p.version));
    field(out, "defaultsVersion", QByteArray::number(p.defaultsVersion));
    field(out, "name", quote(p.name));
    field(out, "aspect", quote(p.aspect));
    field(out, "crop", num(p.crop.x()) + ' ' + num(p.crop.y()) + ' ' + num(p.crop.width()) + ' '
                           + num(p.crop.height()));
    field(out, "clips.size", QByteArray::number(p.clips.size()));
    for (qsizetype i = 0; i < p.clips.size(); ++i) {
        const auto &c = p.clips[i];
        const QByteArray prefix = "clips[" + QByteArray::number(i) + "]";
        field(out, prefix + ".id", quote(c.id));
        field(out, prefix + ".in", num(c.in));
        field(out, prefix + ".out", num(c.out));
        field(out, prefix + ".speed", num(c.speed));
        field(out, prefix + ".layout", quote(c.layout));
        field(out, prefix + ".media", quote(c.media));
        field(out, prefix + ".fit", quote(c.fit));
        field(out, prefix + ".isMedia", boolText(c.isMedia()));
    }
    field(out, "zooms.size", QByteArray::number(p.zooms.size()));
    for (qsizetype i = 0; i < p.zooms.size(); ++i) {
        const auto &z = p.zooms[i];
        const QByteArray prefix = "zooms[" + QByteArray::number(i) + "]";
        field(out, prefix + ".id", quote(z.id));
        field(out, prefix + ".start", num(z.start));
        field(out, prefix + ".end", num(z.end));
        field(out, prefix + ".level", num(z.level));
        field(out, prefix + ".automaticTarget", boolText(z.automaticTarget));
        field(out, prefix + ".target", num(z.target.x()) + ' ' + num(z.target.y()));
    }
    springFields(out, "zoomStyle.spring", p.zoomStyle.spring);
    field(out, "zoomStyle.snapToEdgesRatio", num(p.zoomStyle.snapToEdgesRatio));
    field(out, "zoomStyle.instantAnimation", boolText(p.zoomStyle.instantAnimation));
    field(out, "zoomStyle.motionBlur", num(p.zoomStyle.motionBlur));
    field(out, "background.type", quote(p.background.type));
    field(out, "background.wallpaper", quote(p.background.wallpaper));
    field(out, "background.gradient.angle", num(p.background.gradient.angle));
    field(out, "background.gradient.stops.size", QByteArray::number(p.background.gradient.stops.size()));
    for (qsizetype i = 0; i < p.background.gradient.stops.size(); ++i) {
        const QByteArray prefix = "background.gradient.stops[" + QByteArray::number(i) + "]";
        field(out, prefix + ".color", colorText(p.background.gradient.stops[i].color));
        field(out, prefix + ".position", num(p.background.gradient.stops[i].position));
    }
    field(out, "background.color", colorText(p.background.color));
    field(out, "background.image", quote(p.background.image));
    field(out, "background.blur", num(p.background.blur));
    field(out, "frame.padding", num(p.frame.padding));
    field(out, "frame.radius", num(p.frame.radius));
    shadowFields(out, "frame.shadow", p.frame.shadow);
    borderFields(out, "frame.border", p.frame.border);
    field(out, "cursor.visible", boolText(p.cursor.visible));
    field(out, "cursor.size", num(p.cursor.size));
    field(out, "cursor.smoothing", boolText(p.cursor.smoothing));
    springFields(out, "cursor.spring", p.cursor.spring);
    field(out, "cursor.clickEffect", quote(p.cursor.clickEffect));
    field(out, "cursor.clickShrink", num(p.cursor.clickShrink));
    field(out, "cursor.rotateOnXMovementRatio", num(p.cursor.rotateOnXMovementRatio));
    field(out, "cursor.hideWhenIdleMs", QByteArray::number(p.cursor.hideWhenIdleMs));
    field(out, "cursor.style", quote(p.cursor.style));
    field(out, "cursor.ringColor", colorText(p.cursor.ringColor));
    field(out, "cursor.clickSound", quote(p.cursor.clickSound));
    field(out, "keystrokes.enabled", boolText(p.keystrokes.enabled));
    field(out, "keystrokes.position", quote(p.keystrokes.position));
    field(out, "keystrokes.size", num(p.keystrokes.size));
    field(out, "keystrokes.showOnlyShortcuts", boolText(p.keystrokes.showOnlyShortcuts));
    field(out, "keystrokes.holdMs", QByteArray::number(p.keystrokes.holdMs));
    field(out, "audio.desktop", boolText(p.audio.desktop));
    field(out, "audio.mic", boolText(p.audio.mic));
    field(out, "audio.volume", num(p.audio.volume));
    field(out, "camera.enabled", boolText(p.camera.enabled));
    field(out, "camera.position", quote(p.camera.position));
    field(out, "camera.size", num(p.camera.size));
    field(out, "camera.shape", quote(p.camera.shape));
    field(out, "camera.radius", num(p.camera.radius));
    field(out, "camera.flipHorizontal", boolText(p.camera.flipHorizontal));
    field(out, "camera.rotation", QByteArray::number(p.camera.rotation));
    shadowFields(out, "camera.shadow", p.camera.shadow);
    borderFields(out, "camera.border", p.camera.border);
    field(out, "camera.scaleDuringZoom", num(p.camera.scaleDuringZoom));
    field(out, "camera.offset", num(p.camera.offset.x()) + ' ' + num(p.camera.offset.y()));
    field(out, "camera.fullFrame", quote(p.camera.fullFrame));
    field(out, "exportSettings.format", quote(p.exportSettings.format));
    field(out, "exportSettings.fps", QByteArray::number(p.exportSettings.fps));
    field(out, "exportSettings.height", QByteArray::number(p.exportSettings.height));
    field(out, "exportSettings.quality", quote(p.exportSettings.quality));
    field(out, "exportSettings.gif.fps", QByteArray::number(p.exportSettings.gif.fps));
    field(out, "exportSettings.gif.height", QByteArray::number(p.exportSettings.gif.height));
    field(out, "exportSettings.gif.quality", quote(p.exportSettings.gif.quality));
    field(out, "exportSettings.gif.loop", boolText(p.exportSettings.gif.loop));
}

QByteArray indented(const Project &project)
{
    return QJsonDocument(project.toJson()).toJson(QJsonDocument::Indented);
}

struct LoadResult {
    Project project;
    QString error;
    bool rewritten = false;
    QByteArray after;
    FileState before, afterState;
};

LoadResult loadAt(const QString &path)
{
    LoadResult result;
    result.before = fileState(path);
    const QByteArray original = readFile(path);
    result.error = QStringLiteral("<untouched>");
    result.project = Project::load(path, &result.error);
    result.afterState = fileState(path);
    result.after = readFile(path);
    result.rewritten = result.before.inode != result.afterState.inode || original != result.after;
    return result;
}

void recordLoad(const Context &context, Text &summary, Text &fields, const QString &name,
                const LoadResult &result)
{
    QList<QByteArray> cells{name.toUtf8(), result.rewritten ? "rewritten" : "unchanged",
                            quote(sanitize(result.error))};
    if (result.afterState.exists) cells << octal(result.afterState.mode);
    summary.row(cells);
    if (result.rewritten)
        writeFile(QDir(context.output).filePath(QStringLiteral("load/") + name), result.after);
    writeFile(QDir(context.output).filePath(QStringLiteral("tojson/") + name), indented(result.project));
    fields << "== " << name.toUtf8() << "\n";
    dumpProject(fields, result.project);
}

} // namespace

void runProject(const Context &context)
{
    if (::geteuid() == 0) fail(QStringLiteral("run the oracle as an ordinary user: the project family checks permission errors"));
    const QString corpus = QDir(context.corpus).filePath(QStringLiteral("project"));
    Text summary, fields, migrate;
    for (const QString &file : sortedEntries(corpus, false)) {
        const QByteArray input = readFile(QDir(corpus).filePath(file));
        const QString directory = QDir(context.scratch).filePath(file);
        QDir().mkpath(directory);
        const QString path = QDir(directory).filePath(QStringLiteral("project.json"));
        writeFile(path, input);
        QFile::setPermissions(path, QFileDevice::ReadOwner | QFileDevice::WriteOwner
                                        | QFileDevice::ReadGroup | QFileDevice::ReadOther);
        const LoadResult loaded = loadAt(path);
        recordLoad(context, summary, fields, file, loaded);

        QJsonParseError error;
        const QJsonDocument document = QJsonDocument::fromJson(input, &error);
        if (error.error != QJsonParseError::NoError || !document.isObject()) {
            migrate.row({file.toUtf8(), "-", quote(error.errorString())});
            continue;
        }
        bool changed = false;
        const QJsonObject migrated = Project::migrateDefaults(document.object(), &changed);
        if (Project::migrateDefaults(document.object()) != migrated)
            fail(QStringLiteral("migrateDefaults depends on its changed pointer for %1").arg(file));
        migrate.row({file.toUtf8(), boolText(changed), QJsonDocument(migrated).toJson(QJsonDocument::Compact)});
        bool changedAgain = true;
        const QJsonObject twice = Project::migrateDefaults(migrated, &changedAgain);
        migrate.row({file.toUtf8() + " (again)", boolText(changedAgain), twice == migrated ? "same" : QJsonDocument(twice).toJson(QJsonDocument::Compact)});
        // fromJson() migrates by itself, so it must agree with load() on the unmigrated input.
        if (Project::fromJson(document.object()).toJson() != loaded.project.toJson())
            migrate.row({file.toUtf8() + " (fromJson differs from load)",
                         QJsonDocument(Project::fromJson(document.object()).toJson()).toJson(QJsonDocument::Compact)});
    }

    // Loads that fail or cannot write the migrated file back.
    const QString special = QDir(context.scratch).filePath(QStringLiteral("special"));
    QDir().mkpath(special);
    recordLoad(context, summary, fields, QStringLiteral("special-missing-file.json"),
               loadAt(QDir(special).filePath(QStringLiteral("missing/project.json"))));
    QDir().mkpath(QDir(special).filePath(QStringLiteral("directory.json")));
    recordLoad(context, summary, fields, QStringLiteral("special-directory.json"),
               loadAt(QDir(special).filePath(QStringLiteral("directory.json"))));
    const QByteArray legacy = readFile(QDir(corpus).filePath(QStringLiteral("history-02-2026-09-02-96d74f9.json")));
    const QString unreadable = QDir(special).filePath(QStringLiteral("unreadable.json"));
    writeFile(unreadable, legacy);
    QFile::setPermissions(unreadable, QFileDevice::Permissions());
    recordLoad(context, summary, fields, QStringLiteral("special-unreadable.json"), loadAt(unreadable));
    QFile::setPermissions(unreadable, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    const QString readOnlyDirectory = QDir(special).filePath(QStringLiteral("read-only"));
    QDir().mkpath(readOnlyDirectory);
    const QString readOnly = QDir(readOnlyDirectory).filePath(QStringLiteral("project.json"));
    writeFile(readOnly, legacy);
    QFile::setPermissions(readOnlyDirectory, QFileDevice::ReadOwner | QFileDevice::ExeOwner);
    recordLoad(context, summary, fields, QStringLiteral("special-legacy-in-read-only-directory.json"), loadAt(readOnly));
    QFile::setPermissions(readOnlyDirectory, QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);
    const QString readOnlyFile = QDir(special).filePath(QStringLiteral("read-only-file.json"));
    writeFile(readOnlyFile, legacy);
    QFile::setPermissions(readOnlyFile, QFileDevice::ReadOwner | QFileDevice::ReadGroup);
    recordLoad(context, summary, fields, QStringLiteral("special-legacy-read-only-file.json"), loadAt(readOnlyFile));
    QFile::setPermissions(readOnlyFile, QFileDevice::ReadOwner | QFileDevice::WriteOwner);
    const QString executable = QDir(special).filePath(QStringLiteral("mode-0750.json"));
    writeFile(executable, legacy);
    QFile::setPermissions(executable, QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner
                                          | QFileDevice::ReadGroup | QFileDevice::ExeGroup);
    recordLoad(context, summary, fields, QStringLiteral("special-legacy-mode-0750.json"), loadAt(executable));

    writeFile(QDir(context.output).filePath(QStringLiteral("load.txt")), summary.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("fields.txt")), fields.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("migrate.txt")), migrate.data());

    // save()
    Text save;
    const Project defaults = Project::defaults(QStringLiteral("Saved"), 3.5);
    const auto trySave = [&](const QByteArray &label, const QString &path) {
        QString error = QStringLiteral("<untouched>");
        const bool ok = defaults.save(path, &error);
        const QByteArray written = readFile(path);
        save.row({label, boolText(ok), quote(sanitize(error)),
                  written == indented(defaults) ? "matches-toJson" : (written.isEmpty() ? "-" : "differs"),
                  fileState(path).exists ? octal(fileState(path).mode) : "-"});
    };
    trySave("new file", QDir(special).filePath(QStringLiteral("saved.json")));
    trySave("missing directory", QDir(special).filePath(QStringLiteral("no/such/dir/saved.json")));
    trySave("path is a directory", QDir(special).filePath(QStringLiteral("directory.json")));
    QFile::setPermissions(readOnlyDirectory, QFileDevice::ReadOwner | QFileDevice::ExeOwner);
    trySave("read-only directory", QDir(readOnlyDirectory).filePath(QStringLiteral("saved.json")));
    QFile::setPermissions(readOnlyDirectory, QFileDevice::ReadOwner | QFileDevice::WriteOwner | QFileDevice::ExeOwner);
    trySave("empty path", QString());
    writeFile(QDir(context.output).filePath(QStringLiteral("save.txt")), save.data());

    // defaults()
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const QList<QPair<QString, Project>> defaultsCases{
        {QStringLiteral("no-arguments"), Project::defaults()},
        {QStringLiteral("demo-12.5"), Project::defaults(QStringLiteral("Demo"), 12.5)},
        {QStringLiteral("empty-name-0"), Project::defaults(QString(), 0.0)},
        {QStringLiteral("unicode-tiny"), Project::defaults(QStringLiteral(u"R\u00e9sum\u00e9 \U0001F3AC \"q\""), 1e-9)},
        {QStringLiteral("negative"), Project::defaults(QStringLiteral("x"), -5.0)},
        {QStringLiteral("long"), Project::defaults(QStringLiteral("Long"), 86400.123456789)},
        {QStringLiteral("nan"), Project::defaults(QStringLiteral("NaN"), nan)},
        {QStringLiteral("default-constructed"), Project()},
    };
    Text defaultsFields;
    for (const auto &[label, project] : defaultsCases) {
        writeFile(QDir(context.output).filePath(QStringLiteral("defaults/") + label + QStringLiteral(".json")),
                   indented(project));
        defaultsFields << "== " << label.toUtf8() << "\n";
        dumpProject(defaultsFields, project);
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("defaults-fields.txt")), defaultsFields.data());

    // Free helpers.
    Text helpers;
    QList<QByteArray> speeds{"allowedClipSpeeds"};
    for (const double speed : Omareel::allowedClipSpeeds()) speeds << num(speed);
    helpers.row(speeds);
    helpers.row({"allowedClipLayouts", quoteList(Omareel::allowedClipLayouts())});
    helpers.row({"allowedClipFits", quoteList(Omareel::allowedClipFits())});
    helpers.row({"allowedAspects", quoteList(Omareel::allowedAspects())});
    helpers.row({"CurrentDefaultsVersion", QByteArray::number(Project::CurrentDefaultsVersion)});
    for (const QString &path : {
             QStringLiteral("media/demo.mp4"), QStringLiteral("media/./demo.mp4"),
             QStringLiteral("media//demo.mp4"), QStringLiteral("media/a/../demo.mp4"),
             QStringLiteral("media/talk..final.mp4"), QStringLiteral("media/..hidden.mp4"),
             QStringLiteral("media/../screen.mp4"), QStringLiteral("media/../../etc/passwd"),
             QStringLiteral("media/a/b/../../../x.mp4"), QStringLiteral("./media/demo.mp4"),
             QStringLiteral("media"), QStringLiteral("media/"), QStringLiteral("media/."),
             QStringLiteral("media/sub/"), QStringLiteral("media/sub/clip.mov"),
             QStringLiteral("Media/demo.mp4"), QStringLiteral("medias/demo.mp4"),
             QStringLiteral("mediademo.mp4"), QStringLiteral("demo.mp4"), QStringLiteral("/home/demo.mp4"),
             QStringLiteral("/media/demo.mp4"), QStringLiteral("media\\demo.mp4"),
             QStringLiteral("media/demo\\x.mp4"), QStringLiteral("~/media/x.mp4"),
             QStringLiteral("media/ spaced name.mp4"), QStringLiteral(u"media/\u00e9\U0001F600.mp4"),
             QStringLiteral("media/a:b.mp4"), QStringLiteral("C:/media/x.mp4"),
             QStringLiteral("media/..."), QStringLiteral("media/x/.."), QStringLiteral(""),
             QString(), QStringLiteral(" media/x.mp4"), QStringLiteral("media/x.mp4 ")})
        helpers.row({"sanitizedMediaPath", quote(path), quote(Omareel::sanitizedMediaPath(path))});

    const Omareel::Spring fallback{1.5, 250.0, 25.0};
    for (const QByteArray &json : QList<QByteArray>{
             "{}", R"({"mass":2})", R"({"mass":2,"stiffness":3,"damping":4})",
             R"({"mass":"2","stiffness":null,"damping":true})", R"({"mass":0,"stiffness":-1,"damping":1e300})",
             R"({"MASS":2,"extra":1})", R"({"mass":1.0000000000000002,"stiffness":470.5,"damping":0.1})"}) {
        const Omareel::Spring spring = Omareel::springFromJson(QJsonDocument::fromJson(json).object(), fallback);
        helpers.row({"springFromJson", json, num(spring.mass), num(spring.stiffness), num(spring.damping),
                     QJsonDocument(Omareel::springToJson(spring)).toJson(QJsonDocument::Compact)});
    }
    for (const Omareel::Spring &spring : {Omareel::Spring{}, Omareel::Spring{3.0, 470.0, 70.0},
                                          Omareel::Spring{2.25, 200.0, 40.0}, Omareel::Spring{-0.0, 1e21, 0.1},
                                          Omareel::Spring{nan, 1.0, 2.0}})
        helpers.row({"springToJson", num(spring.mass), num(spring.stiffness), num(spring.damping),
                     QJsonDocument(Omareel::springToJson(spring)).toJson(QJsonDocument::Compact)});

    // json() and setJson() are toJson() and fromJson() under other names.
    Project viaSetJson;
    viaSetJson.setJson(Project::defaults(QStringLiteral("Via setJson"), 2.0).json());
    helpers.row({"setJson(json()) of defaults(\"Via setJson\", 2)",
                 QJsonDocument(viaSetJson.toJson()).toJson(QJsonDocument::Compact)});
    writeFile(QDir(context.output).filePath(QStringLiteral("helpers.txt")), helpers.data());
}

} // namespace Oracle
