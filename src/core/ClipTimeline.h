#pragma once

#include <QString>
#include <QVector>

namespace OmaRecord {

struct Clip {
    QString id;
    double in = 0.0;
    double out = 0.0;
    double speed = 1.0;
};

class ClipTimeline
{
public:
    explicit ClipTimeline(QVector<Clip> clips = {}): m_clips(std::move(clips)) {}
    const QVector<Clip> &clips() const { return m_clips; }
    double totalDuration() const;
    double sourceTime(double outputTime) const;
    double outputTime(int clipIndex, double sourceTime) const;
    bool split(double outputTime);
    bool trim(int index, double newIn, double newOut);
    bool remove(int index);

private:
    QVector<Clip> m_clips;
    int m_nextId = 1;
};

} // namespace OmaRecord
