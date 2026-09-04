#pragma once

#include "InputLog.h"
#include "Project.h"

#include <QString>
#include <QStringList>
#include <QVector>

namespace Omareel {

struct KeystrokePill {
    QStringList keys;
    QString text;
    double opacity = 0.0;
};

class KeystrokeTrack
{
public:
    static KeystrokeTrack build(const QVector<InputEvent> &events,
                                const Keystrokes &settings);
    QVector<KeystrokePill> sample(double time) const;

private:
    struct Group {
        double start = 0.0;
        double last = 0.0;
        QStringList keys;
    };

    Keystrokes m_settings;
    QVector<Group> m_groups;
};

} // namespace Omareel
