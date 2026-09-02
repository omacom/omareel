#include "Launcher.h"

#include "core/Project.h"

#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QProcess>
#include <QStandardPaths>
#include <QUrl>

using namespace OmaRecord;

static double mediaDuration(const QString &path)
{
    QProcess process;
    process.start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("format=duration"),
        QStringLiteral("-of"), QStringLiteral("default=noprint_wrappers=1:nokey=1"), path});
    if (!process.waitForFinished(5000) || process.exitCode() != 0) return 0.0;
    return QString::fromUtf8(process.readAllStandardOutput()).trimmed().toDouble();
}

Launcher::Launcher(QObject *parent): QObject(parent) { refresh(); }

void Launcher::refresh()
{
    m_recentBundles.clear();
    const QDir root(QDir(QStandardPaths::writableLocation(QStandardPaths::MoviesLocation)).filePath(QStringLiteral("omarecord")));
    const auto entries = root.entryInfoList({QStringLiteral("*.omarecord")}, QDir::Dirs | QDir::NoDotAndDotDot, QDir::Time);
    for (const auto &entry : entries.mid(0, 12)) {
        QString name = entry.completeBaseName();
        const QString projectPath = QDir(entry.absoluteFilePath()).filePath(QStringLiteral("project.json"));
        if (QFileInfo(projectPath).isFile()) {
            QString ignored;
            const Project project = Project::load(projectPath, &ignored);
            if (ignored.isEmpty()) name = project.name;
        }
        const QString thumb = QDir(entry.absoluteFilePath()).filePath(QStringLiteral("thumb.jpg"));
        m_recentBundles << QVariantMap{{QStringLiteral("path"), entry.absoluteFilePath()},
            {QStringLiteral("name"), name}, {QStringLiteral("duration"), mediaDuration(QDir(entry.absoluteFilePath()).filePath(QStringLiteral("screen.mp4")))},
            {QStringLiteral("thumbnail"), QFileInfo(thumb).isFile() ? QUrl::fromLocalFile(thumb).toString() : QString()}};
    }
    emit recentBundlesChanged();
}

void Launcher::openBundle(const QString &pathValue)
{
    const QUrl url(pathValue);
    const QString path = url.isLocalFile() ? url.toLocalFile() : pathValue;
    if (!QFileInfo(path).isDir()) { emit errorOccurred(QStringLiteral("Choose an .omarecord bundle directory")); return; }
    if (!QProcess::startDetached(QCoreApplication::applicationFilePath(), {QStringLiteral("edit"), path})) {
        emit errorOccurred(QStringLiteral("Could not launch editor"));
        return;
    }
    emit quitRequested();
}

void Launcher::record(const QString &mode)
{
    const QString option = QStringLiteral("--") + mode;
    if (!QProcess::startDetached(QCoreApplication::applicationFilePath(), {QStringLiteral("record"), option})) {
        emit errorOccurred(QStringLiteral("Could not start recording"));
        return;
    }
    emit quitRequested();
}
