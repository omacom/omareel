#pragma once
#include <QJsonObject>
#include <QStringList>

namespace Omareel {
class CaptureExclusion
{
public:
    struct Status {
        bool loaded = false;
        QString builtHash;
        QString reason;
        QJsonObject details;
    };
    static Status status();
    static Status ensureLoaded();
    static bool compatibleHash(const QString &built, const QString &running);
    static QString overlayMonitor(bool plugin, const QString &recorded, const QStringList &monitors);
    static bool overlaysOffMonitor(const QString &monitor);
};
}
