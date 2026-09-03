#include "core/KeystrokeTrack.h"

#include <QtTest>

using namespace OmaRecord;

class KeystrokeTrackTest : public QObject
{
    Q_OBJECT
private slots:
    void groupsConsecutiveKeysAndFadesFromLastKey()
    {
        Keystrokes settings;
        settings.enabled = true;
        settings.showOnlyShortcuts = false;
        settings.holdMs = 900;
        const QVector<InputEvent> events{
            {0, 1.0, InputKind::KeyDown, {}, {}, 0, 0, 25, QStringLiteral("KEY_P"),
             {QStringLiteral("ctrl"), QStringLiteral("shift")}},
            {0, 1.5, InputKind::KeyDown, {}, {}, 0, 0, 37, QStringLiteral("KEY_K"), {}}
        };
        const KeystrokeTrack track = KeystrokeTrack::build(events, settings);
        const QVector<KeystrokePill> grouped = track.sample(1.5);
        QCOMPARE(grouped.size(), 1);
        QCOMPARE(grouped.first().keys,
                 QStringList({QStringLiteral("Ctrl"), QStringLiteral("Shift"),
                              QStringLiteral("P"), QStringLiteral("K")}));
        QCOMPARE(grouped.first().text, QStringLiteral("Ctrl + Shift + P + K"));
        QCOMPARE(grouped.first().opacity, 1.0);

        const QVector<KeystrokePill> fading = track.sample(2.525);
        QCOMPARE(fading.size(), 1);
        QVERIFY(std::abs(fading.first().opacity - 0.5) < 0.001);
        QVERIFY(track.sample(2.76).isEmpty());
    }

    void shortcutFilterAndSixKeyLimit()
    {
        Keystrokes settings;
        settings.enabled = true;
        settings.showOnlyShortcuts = true;
        QVector<InputEvent> events{
            {0, .1, InputKind::KeyDown, {}, {}, 0, 0, 30, QStringLiteral("KEY_A"), {}},
            {0, .2, InputKind::KeyDown, {}, {}, 0, 0, 28, QStringLiteral("KEY_ENTER"), {}},
            {0, .3, InputKind::KeyDown, {}, {}, 0, 0, 25, QStringLiteral("KEY_P"),
             {QStringLiteral("ctrl")}}
        };
        for (int i = 0; i < 8; ++i)
            events << InputEvent{0, .4 + i * .05, InputKind::KeyDown, {}, {}, 0, 0,
                                 59 + i, QStringLiteral("KEY_F%1").arg(i + 1), {}};
        const KeystrokeTrack track = KeystrokeTrack::build(events, settings);
        const QVector<KeystrokePill> pills = track.sample(.7);
        QCOMPARE(pills.size(), 2);
        QCOMPARE(pills.first().keys.first(), QStringLiteral("Enter"));
        for (const KeystrokePill &pill : pills) {
            QVERIFY(!pill.keys.contains(QStringLiteral("A")));
            QVERIFY(pill.keys.size() <= 6);
        }
    }
};

QTEST_APPLESS_MAIN(KeystrokeTrackTest)
#include "test_keystroketrack.moc"
