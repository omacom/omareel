// recordingpreferences: RecordingPreferences::path, load and save under hermetic
// XDG_CONFIG_HOME states: no file, every settings.json shape of the corpus, the legacy
// omarecord/settings.json copy-over, and directories or files that can't be read or written.

#include "common.h"

#include "core/RecordingPreferences.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>

#include <cmath>
#include <cstdio>
#include <functional>
#include <limits>
#include <sys/stat.h>
#include <unistd.h>

namespace Oracle {

using Omareel::RecordingPreferences;

namespace {

QByteArray octal(mode_t mode)
{
    char buffer[16];
    std::snprintf(buffer, sizeof buffer, "%04o", unsigned(mode & 07777));
    return buffer;
}

// The file's bytes, when it can be read.
void appendBytes(QList<QByteArray> &cells, const QString &path)
{
    bool readable = false;
    const QByteArray bytes = readFile(path, &readable);
    if (readable) cells << quoteBytes(bytes);
}

// README: missing | directory <mode> | file <mode> [<bytes>] | symlink <target str> <state>.
QList<QByteArray> fileState(const QString &path)
{
    struct stat info {};
    const QByteArray encoded = QFile::encodeName(path);
    if (::lstat(encoded.constData(), &info) != 0) return {"missing"};
    if (S_ISLNK(info.st_mode)) {
        char target[4096];
        const ssize_t size = ::readlink(encoded.constData(), target, sizeof target - 1);
        const QString text = QFile::decodeName(QByteArray(target, size < 0 ? 0 : int(size)));
        QList<QByteArray> cells{"symlink", quote(sanitize(text))};
        if (::stat(encoded.constData(), &info) != 0) return cells << "missing";
        if (S_ISDIR(info.st_mode)) return cells << "directory" << octal(info.st_mode);
        cells << "file" << octal(info.st_mode);
        appendBytes(cells, path);
        return cells;
    }
    if (S_ISDIR(info.st_mode)) return {"directory", octal(info.st_mode)};
    QList<QByteArray> cells{"file", octal(info.st_mode)};
    appendBytes(cells, path);
    return cells;
}

QList<QPair<QByteArray, QByteArray>> fields(const RecordingPreferences &p)
{
    return {
        {"systemAudio", boolText(p.systemAudio)},
        {"microphone", boolText(p.microphone)},
        {"microphoneDevice", quote(p.microphoneDevice)},
        {"webcamEnabled", boolText(p.webcamEnabled)},
        {"webcamDevice", quote(p.webcamDevice)},
        {"webcamChosenDevice", quote(p.webcamChosenDevice)},
        {"webcamHeight", QByteArray::number(p.webcamHeight)},
        {"webcamRotation", QByteArray::number(p.webcamRotation)},
        {"webcamFlipHorizontal", boolText(p.webcamFlipHorizontal)},
        {"selfViewEnabled", boolText(p.selfViewEnabled)},
        {"selfViewSize", quote(p.selfViewSize)},
        {"selfViewX", num(p.selfViewX)},
        {"selfViewY", num(p.selfViewY)},
        {"captureBackend", quote(p.captureBackend)},
        {"countdownBeforeRecording", boolText(p.countdownBeforeRecording)},
        {"webcam", QJsonDocument(p.webcam).toJson(QJsonDocument::Compact)},
    };
}

void dumpFields(Text &out, const RecordingPreferences &p)
{
    for (const auto &[name, value] : fields(p)) out.row({name, value});
}

QString currentFile(const QString &config) { return QDir(config).filePath(QStringLiteral("omareel/settings.json")); }
QString legacyFile(const QString &config) { return QDir(config).filePath(QStringLiteral("omarecord/settings.json")); }

void setMode(const QString &path, mode_t mode)
{
    if (::chmod(QFile::encodeName(path).constData(), mode) != 0)
        fail(QStringLiteral("chmod %1 failed").arg(path));
}

void restoreModes(const QString &root)
{
    // Give every directory and file back its owner bits so the scratch can be removed.
    std::function<void(const QString &)> walk = [&](const QString &path) {
        struct stat info {};
        if (::lstat(QFile::encodeName(path).constData(), &info) != 0 || S_ISLNK(info.st_mode)) return;
        ::chmod(QFile::encodeName(path).constData(), (info.st_mode & 07777) | S_IRWXU);
        if (S_ISDIR(info.st_mode))
            for (const QString &name : QDir(path).entryList(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden | QDir::System))
                walk(QDir(path).filePath(name));
    };
    walk(root);
}

struct Environment {
    QByteArray home;
    QByteArray config;
};

// Saves, then loads again and lists the fields that did not come back as they were saved.
void saveAndReload(Text &out, const RecordingPreferences &preferences, const QString &config)
{
    QString error = QStringLiteral("<untouched>");
    const bool ok = preferences.save(&error);
    out.row({"save", boolText(ok), quote(sanitize(error))});
    QList<QByteArray> after{"after-save", "current"};
    after << fileState(currentFile(config));
    out.row(after);
    const auto before = fields(preferences);
    const auto reloaded = fields(RecordingPreferences::load());
    QList<QByteArray> differing{"reload"};
    for (qsizetype i = 0; i < before.size(); ++i)
        if (before[i].second != reloaded[i].second) differing << before[i].first;
    if (differing.size() == 1) differing << "same";
    else differing.insert(1, "differs");
    out.row(differing);
}

} // namespace

void runRecordingPreferences(const Context &context)
{
    if (::geteuid() == 0) fail(QStringLiteral("run the oracle as an ordinary user: recordingpreferences checks permission errors"));
    const Environment original{qgetenv("HOME"), qgetenv("XDG_CONFIG_HOME")};
    const QString corpus = QDir(context.corpus).filePath(QStringLiteral("recordingpreferences"));
    const auto corpusFile = [&](const char *name) {
        return readFile(QDir(corpus).filePath(QString::fromLatin1(name)));
    };
    Text out;

    // A state: a fresh directory with config/ as XDG_CONFIG_HOME (setup may change that) and
    // home/ as HOME, prepared by setup; then load, dump, save, reload.
    const auto run = [&](const QString &label, const std::function<void(const QString &root, QString &config)> &setup) {
        const QString root = QDir(context.scratch).filePath(label);
        QString config = QDir(root).filePath(QStringLiteral("config"));
        QDir().mkpath(config);
        QDir().mkpath(QDir(root).filePath(QStringLiteral("home")));
        qputenv("HOME", QFile::encodeName(QDir(root).filePath(QStringLiteral("home"))));
        qputenv("XDG_CONFIG_HOME", QFile::encodeName(config));
        setup(root, config);
        out << "== " << label.toUtf8() << "\n";
        out.row({"path", quote(sanitize(RecordingPreferences::path()))});
        const RecordingPreferences preferences = RecordingPreferences::load();
        dumpFields(out, preferences);
        QList<QByteArray> current{"after-load", "current"}, legacy{"after-load", "legacy"};
        out.row(current << fileState(currentFile(config)));
        out.row(legacy << fileState(legacyFile(config)));
        saveAndReload(out, preferences, config);
        restoreModes(root);
        qputenv("HOME", original.home);
        qputenv("XDG_CONFIG_HOME", original.config);
    };

    for (const QString &file : sortedEntries(corpus, false)) {
        const QByteArray bytes = readFile(QDir(corpus).filePath(file));
        run(QStringLiteral("current-") + QFileInfo(file).completeBaseName(), [&](const QString &, QString &config) {
            writeFile(currentFile(config), bytes);
        });
    }
    for (const char *file : {"valid-full.json", "invalid-json.json", "webcam-legacy-inset.json"}) {
        const QByteArray bytes = corpusFile(file);
        run(QStringLiteral("legacy-") + QFileInfo(QString::fromLatin1(file)).completeBaseName(),
            [&](const QString &, QString &config) { writeFile(legacyFile(config), bytes); });
    }
    const QByteArray full = corpusFile("valid-full.json");
    run(QStringLiteral("special-no-file"), [](const QString &, QString &) {});
    run(QStringLiteral("special-config-home-missing"), [](const QString &root, QString &config) {
        config = QDir(root).filePath(QStringLiteral("missing-config"));
        qputenv("XDG_CONFIG_HOME", QFile::encodeName(config));
    });
    run(QStringLiteral("special-legacy-and-current"), [&](const QString &, QString &config) {
        writeFile(legacyFile(config), full);
        writeFile(currentFile(config), corpusFile("self-view-small-lowercase.json"));
    });
    run(QStringLiteral("special-legacy-omareel-is-file"), [&](const QString &, QString &config) {
        writeFile(legacyFile(config), full);
        writeFile(QDir(config).filePath(QStringLiteral("omareel")), "not a directory\n");
    });
    run(QStringLiteral("special-legacy-omareel-read-only"), [&](const QString &, QString &config) {
        writeFile(legacyFile(config), full);
        QDir().mkpath(QDir(config).filePath(QStringLiteral("omareel")));
        setMode(QDir(config).filePath(QStringLiteral("omareel")), 0500);
    });
    run(QStringLiteral("special-legacy-is-directory"), [&](const QString &, QString &config) {
        QDir().mkpath(legacyFile(config));
    });
    run(QStringLiteral("special-legacy-unreadable"), [&](const QString &, QString &config) {
        writeFile(legacyFile(config), full);
        setMode(legacyFile(config), 0000);
    });
    run(QStringLiteral("special-current-is-directory"), [&](const QString &, QString &config) {
        QDir().mkpath(currentFile(config));
    });
    run(QStringLiteral("special-current-unreadable"), [&](const QString &, QString &config) {
        writeFile(currentFile(config), full);
        setMode(currentFile(config), 0000);
    });
    run(QStringLiteral("special-current-read-only"), [&](const QString &, QString &config) {
        writeFile(currentFile(config), full);
        setMode(currentFile(config), 0444);
    });
    run(QStringLiteral("special-current-dangling-symlink"), [&](const QString &, QString &config) {
        QDir().mkpath(QDir(config).filePath(QStringLiteral("omareel")));
        writeFile(legacyFile(config), full);
        if (::symlink("missing-target.json", QFile::encodeName(currentFile(config)).constData()) != 0)
            fail(QStringLiteral("symlink failed"));
    });
    run(QStringLiteral("special-current-symlink"), [&](const QString &, QString &config) {
        writeFile(QDir(config).filePath(QStringLiteral("omareel/real.json")), full);
        if (::symlink("real.json", QFile::encodeName(currentFile(config)).constData()) != 0)
            fail(QStringLiteral("symlink failed"));
    });
    run(QStringLiteral("special-config-home-read-only"), [&](const QString &, QString &config) {
        setMode(config, 0500);
    });
    run(QStringLiteral("special-xdg-config-home-unset"), [&](const QString &root, QString &config) {
        qunsetenv("XDG_CONFIG_HOME");
        config = QDir(root).filePath(QStringLiteral("home/.config"));
        writeFile(currentFile(config), corpusFile("self-view-small-lowercase.json"));
    });
    run(QStringLiteral("special-xdg-config-home-relative"), [&](const QString &root, QString &config) {
        qputenv("XDG_CONFIG_HOME", "config");
        config = QDir(root).filePath(QStringLiteral("home/.config"));
        writeFile(currentFile(config), corpusFile("self-view-small-lowercase.json"));
    });

    // Preferences built in C++ and saved into an empty configuration directory.
    const auto runConstructed = [&](const QString &label, const RecordingPreferences &preferences) {
        const QString root = QDir(context.scratch).filePath(label);
        const QString config = QDir(root).filePath(QStringLiteral("config"));
        QDir().mkpath(config);
        qputenv("XDG_CONFIG_HOME", QFile::encodeName(config));
        out << "== " << label.toUtf8() << "\n";
        out.row({"path", quote(sanitize(RecordingPreferences::path()))});
        dumpFields(out, preferences);
        saveAndReload(out, preferences, config);
        qputenv("XDG_CONFIG_HOME", original.config);
    };
    runConstructed(QStringLiteral("constructed-defaults"), RecordingPreferences{});
    RecordingPreferences edge;
    edge.systemAudio = false;
    edge.microphone = false;
    edge.microphoneDevice = QStringLiteral(u"Mic “ü” \U0001F3A4");
    edge.webcamEnabled = true;
    edge.webcamDevice = QString();
    edge.webcamChosenDevice = QStringLiteral("/dev/video9");
    edge.webcamHeight = 480;
    edge.webcamRotation = 45;
    edge.webcamFlipHorizontal = true;
    edge.selfViewEnabled = false;
    edge.selfViewSize = QStringLiteral("xl");
    edge.selfViewX = -1.0;
    edge.selfViewY = 2.5;
    edge.captureBackend = QString();
    edge.countdownBeforeRecording = true;
    edge.webcam = QJsonDocument::fromJson(
        R"({"inset":{"width":3},"mirror":true,"captureHeight":"x","extra":{"list":[1,null,"s"]}})").object();
    runConstructed(QStringLiteral("constructed-edge-values"), edge);
    RecordingPreferences nonFinite;
    nonFinite.selfViewX = std::numeric_limits<double>::quiet_NaN();
    nonFinite.selfViewY = -std::numeric_limits<double>::infinity();
    nonFinite.webcamHeight = 720;
    nonFinite.webcam = QJsonObject();
    runConstructed(QStringLiteral("constructed-non-finite"), nonFinite);

    writeFile(QDir(context.output).filePath(QStringLiteral("cases.txt")), out.data());
}

} // namespace Oracle
