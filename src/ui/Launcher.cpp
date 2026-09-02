#include "Launcher.h"

#include "core/Project.h"
#include "core/OmarchyPaths.h"
#include "core/RecordingMetadata.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFileInfo>
#include <QLocale>
#include <QProcess>
#include <QUrl>

using namespace OmaRecord;

static double processDuration(QProcess *process)
{
    if (process->exitStatus() != QProcess::NormalExit || process->exitCode() != 0) return 0.0;
    return QString::fromUtf8(process->readAllStandardOutput()).trimmed().toDouble();
}

Launcher::Launcher(QObject *parent): QObject(parent) { refresh(); }

void Launcher::refresh()
{
    m_recentBundles.clear();
    const QDir root(OmarchyPaths::recordingsDirectory());
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
        const double duration = RecordingMetadata::captureDuration(entry.absoluteFilePath());
        m_recentBundles << QVariantMap{{QStringLiteral("path"), entry.absoluteFilePath()},
            {QStringLiteral("name"), name}, {QStringLiteral("duration"), duration},
            {QStringLiteral("durationText"), duration > 0.0 ? formatDuration(duration) : QStringLiteral("--:--")},
            {QStringLiteral("dateText"), formatDate(entry.lastModified())},
            {QStringLiteral("thumbnail"), QFileInfo(thumb).isFile() ? QUrl::fromLocalFile(thumb).toString() : QString()}};
        if (duration <= 0.0) probeDurationAsync(entry.absoluteFilePath());
    }
    emit recentBundlesChanged();
}

QString Launcher::formatDuration(double seconds)
{
    const int total = std::max(0, qRound(seconds));
    return QStringLiteral("%1:%2").arg(total / 60, 2, 10, QLatin1Char('0'))
                                      .arg(total % 60, 2, 10, QLatin1Char('0'));
}

QString Launcher::formatDate(const QDateTime &dateTime)
{
    return QLocale().toString(dateTime.date(), QLocale::ShortFormat);
}

void Launcher::probeDurationAsync(const QString &bundlePath)
{
    auto *process = new QProcess(this);
    connect(process, qOverload<int, QProcess::ExitStatus>(&QProcess::finished), this,
            [this, process, bundlePath](int, QProcess::ExitStatus) {
        const double duration = processDuration(process);
        process->deleteLater();
        if (duration <= 0.0) return;
        for (QVariant &value : m_recentBundles) {
            QVariantMap bundle = value.toMap();
            if (bundle.value(QStringLiteral("path")).toString() != bundlePath) continue;
            bundle[QStringLiteral("duration")] = duration;
            bundle[QStringLiteral("durationText")] = formatDuration(duration);
            value = bundle;
            emit recentBundlesChanged();
            break;
        }
    });
    process->start(QStringLiteral("ffprobe"), {QStringLiteral("-v"), QStringLiteral("error"),
        QStringLiteral("-show_entries"), QStringLiteral("format=duration"),
        QStringLiteral("-of"), QStringLiteral("default=noprint_wrappers=1:nokey=1"),
        QDir(bundlePath).filePath(QStringLiteral("screen.mp4"))});
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
