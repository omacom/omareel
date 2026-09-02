#pragma once

#include <QPointF>
#include <QString>
#include <QStringList>
#include <QVector>

namespace OmaRecord {

enum class InputKind { Move, ButtonDown, ButtonUp, Scroll, KeyDown, KeyUp };

struct InputEvent {
    qint64 monotonicUs = 0;
    double time = 0.0;
    InputKind kind = InputKind::Move;
    QPointF position;
    QString button;
    double dx = 0.0;
    double dy = 0.0;
    int keyCode = 0;
    QString keyName;
    QStringList modifiers;
};

class InputLog
{
public:
    static InputLog load(const QString &inputPath, qint64 firstFrameUs,
                         double duration = -1.0, QString *error = nullptr);
    static InputLog loadBundle(const QString &bundlePath, double duration = -1.0,
                               QString *error = nullptr);

    const QVector<InputEvent> &events() const { return m_events; }
    QVector<InputEvent> moves() const;
    QVector<InputEvent> clickDowns(bool leftRightOnly = false) const;
    int keyCount() const;

private:
    QVector<InputEvent> m_events;
};

} // namespace OmaRecord
