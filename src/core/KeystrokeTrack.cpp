#include "KeystrokeTrack.h"

#include <algorithm>

using namespace OmaRecord;

namespace {

bool isModifier(const QString &name)
{
    return name.contains(QLatin1String("CTRL")) || name.contains(QLatin1String("SHIFT"))
        || name.contains(QLatin1String("ALT")) || name.contains(QLatin1String("META"));
}

QString modifierLabel(const QString &modifier)
{
    if (modifier == QLatin1String("ctrl")) return QStringLiteral("Ctrl");
    if (modifier == QLatin1String("shift")) return QStringLiteral("Shift");
    if (modifier == QLatin1String("alt")) return QStringLiteral("Alt");
    if (modifier == QLatin1String("meta")) return QStringLiteral("Super");
    return {};
}

QString keyLabel(QString name)
{
    if (name.startsWith(QLatin1String("KEY_"))) name.remove(0, 4);
    if (name.size() == 1) return name;
    if (name.startsWith(QLatin1String("KP")) && name.size() == 3)
        return QStringLiteral("Num %1").arg(name.back());
    static const QHash<QString, QString> labels{
        {QStringLiteral("ESC"), QStringLiteral("Esc")},
        {QStringLiteral("ENTER"), QStringLiteral("Enter")},
        {QStringLiteral("SPACE"), QStringLiteral("Space")},
        {QStringLiteral("TAB"), QStringLiteral("Tab")},
        {QStringLiteral("BACKSPACE"), QStringLiteral("Backspace")},
        {QStringLiteral("DELETE"), QStringLiteral("Delete")},
        {QStringLiteral("UP"), QStringLiteral("Up")},
        {QStringLiteral("DOWN"), QStringLiteral("Down")},
        {QStringLiteral("LEFT"), QStringLiteral("Left")},
        {QStringLiteral("RIGHT"), QStringLiteral("Right")},
        {QStringLiteral("PAGEUP"), QStringLiteral("Page Up")},
        {QStringLiteral("PAGEDOWN"), QStringLiteral("Page Down")},
        {QStringLiteral("LEFTBRACE"), QStringLiteral("[")},
        {QStringLiteral("RIGHTBRACE"), QStringLiteral("]")},
        {QStringLiteral("MINUS"), QStringLiteral("-")},
        {QStringLiteral("EQUAL"), QStringLiteral("=")},
        {QStringLiteral("COMMA"), QStringLiteral(",")},
        {QStringLiteral("DOT"), QStringLiteral(".")},
        {QStringLiteral("SLASH"), QStringLiteral("/")},
        {QStringLiteral("SEMICOLON"), QStringLiteral(";")},
        {QStringLiteral("APOSTROPHE"), QStringLiteral("'")}
    };
    if (labels.contains(name)) return labels.value(name);
    name.replace(QLatin1Char('_'), QLatin1Char(' '));
    QStringList words = name.toLower().split(QLatin1Char(' '), Qt::SkipEmptyParts);
    for (QString &word : words) if (!word.isEmpty()) word[0] = word[0].toUpper();
    return words.join(QLatin1Char(' '));
}

bool isPlainTyping(const InputEvent &event)
{
    if (!event.modifiers.isEmpty()) return false;
    QString name = event.keyName;
    if (name.startsWith(QLatin1String("KEY_"))) name.remove(0, 4);
    return name.size() == 1 && name.front().isLetterOrNumber();
}

} // namespace

KeystrokeTrack KeystrokeTrack::build(const QVector<InputEvent> &events,
                                     const Keystrokes &settings)
{
    KeystrokeTrack track;
    track.m_settings = settings;
    if (!settings.enabled) return track;
    for (const InputEvent &event : events) {
        if (event.kind != InputKind::KeyDown || event.keyName.isEmpty()
            || isModifier(event.keyName)
            || (settings.showOnlyShortcuts && isPlainTyping(event))) continue;

        QStringList keys;
        for (const QString &modifier : event.modifiers) {
            const QString label = modifierLabel(modifier);
            if (!label.isEmpty() && !keys.contains(label)) keys << label;
        }
        const QString label = keyLabel(event.keyName);
        if (!label.isEmpty()) keys << label;
        if (keys.isEmpty()) continue;

        if (track.m_groups.isEmpty() || event.time - track.m_groups.last().last > 0.6
            || track.m_groups.last().keys.size() + keys.size() > 6) {
            track.m_groups << Group{event.time, event.time, {}};
        }
        Group &group = track.m_groups.last();
        group.last = event.time;
        for (const QString &key : keys) {
            if (group.keys.size() >= 6) break;
            group.keys << key;
        }
    }
    return track;
}

QVector<KeystrokePill> KeystrokeTrack::sample(double time) const
{
    QVector<KeystrokePill> result;
    const double hold = m_settings.holdMs / 1000.0;
    constexpr double fade = 0.25;
    for (const Group &group : m_groups) {
        if (time < group.start || time > group.last + hold + fade) continue;
        const double opacity = time <= group.last + hold ? 1.0
            : std::clamp(1.0 - (time - group.last - hold) / fade, 0.0, 1.0);
        result << KeystrokePill{group.keys, group.keys.join(QStringLiteral(" + ")), opacity};
    }
    return result;
}
