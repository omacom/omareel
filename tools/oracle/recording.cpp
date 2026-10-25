#include "recording.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonValue>

#include <algorithm>
#include <cstdio>
#include <ctime>
#include <sys/stat.h>

namespace Oracle {

ScopedEnvironment::~ScopedEnvironment()
{
    for (auto it = m_saved.crbegin(); it != m_saved.crend(); ++it) {
        if (it->wasSet) qputenv(it->name.constData(), it->value);
        else qunsetenv(it->name.constData());
    }
}

void ScopedEnvironment::remember(const char *name)
{
    for (const Saved &saved : std::as_const(m_saved))
        if (saved.name == name) return;
    m_saved << Saved{QByteArray(name), qEnvironmentVariableIsSet(name), qgetenv(name)};
}

void ScopedEnvironment::set(const char *name, const QByteArray &value)
{
    remember(name);
    qputenv(name, value);
}

void ScopedEnvironment::unset(const char *name)
{
    remember(name);
    qunsetenv(name);
}

qint64 realtimeNowUs()
{
    timespec now{};
    clock_gettime(CLOCK_REALTIME, &now);
    return qint64(now.tv_sec) * 1000000 + now.tv_nsec / 1000;
}

QByteArray maskRealtime(const QByteArray &bytes, qint64 beforeUs, qint64 afterUs)
{
    QList<QByteArray> lines = bytes.split('\n');
    for (qsizetype index = 1; index < lines.size(); ++index) {
        QList<QByteArray> cells = lines[index].split('\t');
        if (cells.size() != 2) continue;
        bool ok = false;
        const qint64 value = cells[1].toLongLong(&ok);
        if (!ok) continue;
        cells[1] = value >= beforeUs && value <= afterUs ? "{realtime}" : "{realtime out of range}";
        lines[index] = cells.join('\t');
    }
    return lines.join('\n');
}

QByteArray octalMode(const QString &path)
{
    struct stat info {};
    if (::lstat(QFile::encodeName(path).constData(), &info) != 0) return "-";
    char buffer[16];
    std::snprintf(buffer, sizeof buffer, "%04o", unsigned(info.st_mode & 07777));
    return buffer;
}

QByteArray directoryListing(const QString &path)
{
    if (!QFileInfo(path).isDir()) return "-";
    const QStringList names = QDir(path).entryList(QDir::AllEntries | QDir::NoDotAndDotDot | QDir::Hidden
                                                   | QDir::System);
    QList<QByteArray> sorted;
    for (const QString &name : names) sorted << name.toUtf8();
    std::sort(sorted.begin(), sorted.end());
    if (sorted.isEmpty()) return "(empty)";
    QList<QByteArray> cells;
    for (const QByteArray &name : sorted) {
        const QString full = QDir(path).filePath(QString::fromUtf8(name));
        struct stat info {};
        char type = '?';
        if (::lstat(QFile::encodeName(full).constData(), &info) == 0)
            type = S_ISLNK(info.st_mode) ? 'l' : S_ISDIR(info.st_mode) ? 'd' : 'f';
        cells << name + ':' + type + octalMode(full);
    }
    return cells.join(' ');
}

QJsonObject corpusObject(const Context &context, const QString &relative)
{
    return readJson(QDir(context.corpus).filePath(relative)).toObject();
}

} // namespace Oracle
