#pragma once

// Helpers shared by the recording-side families (recorder, regionpicker, captureexclusion, camera,
// recordingbar, microphone, screencapture, launcher).

#include "common.h"

#include <QByteArray>
#include <QJsonObject>
#include <QList>
#include <QPair>
#include <QString>

namespace Oracle {

// Sets and unsets environment variables, putting every one it touched back on destruction.
class ScopedEnvironment
{
public:
    ScopedEnvironment() = default;
    ~ScopedEnvironment();
    ScopedEnvironment(const ScopedEnvironment &) = delete;
    ScopedEnvironment &operator=(const ScopedEnvironment &) = delete;
    void set(const char *name, const QByteArray &value);
    void unset(const char *name);

private:
    void remember(const char *name);
    struct Saved { QByteArray name; bool wasSet; QByteArray value; };
    QList<Saved> m_saved;
};

// A file's bytes with the realtime column of omareel's timestamp files masked: the files are
// "monotonic_microsec\trealtime_microsec\n<monotonic>\t<realtime>\n", the realtime value being
// the wall clock when they were written. A realtime value within [before, after] (microseconds
// since the epoch) is replaced by {realtime}, any other by {realtime out of range}.
QByteArray maskRealtime(const QByteArray &bytes, qint64 beforeUs, qint64 afterUs);
qint64 realtimeNowUs();

// The entries of a directory, sorted byte-wise, as "<name>:<type><mode>" (type d, f or l, mode
// four octal digits), joined by spaces; "-" when it does not exist, "(empty)" when it is empty.
QByteArray directoryListing(const QString &path);
QByteArray octalMode(const QString &path);

// The stdoutFile/argv answers of a corpus scenario object.
QJsonObject corpusObject(const Context &context, const QString &relative);

} // namespace Oracle
