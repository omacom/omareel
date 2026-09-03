#include "record/RegionPicker.h"

#include <QtTest>

using namespace OmaRecord;

class RegionPickerTest : public QObject
{
    Q_OBJECT
private slots:
    void parsesCaptureOptions()
    {
        const QString output = QStringLiteral(
            "DP-3|3840x2160\n"
            "region\n"
            "  DP-5 | 2560x720  \r\n"
            "invalid|size\n"
            "DP-3|3840x2160\n");
        QCOMPARE(RegionPicker::parseCaptureOptions(output),
                 QStringList({QStringLiteral("DP-3"), QStringLiteral("DP-5")}));
    }

    void ignoresEmptyAndMalformedOutput()
    {
        QVERIFY(RegionPicker::parseCaptureOptions(QString()).isEmpty());
        QVERIFY(RegionPicker::parseCaptureOptions(
                    QStringLiteral("screen\nregion\nDP-3|3840-by-2160\n"))
                    .isEmpty());
    }

    void detectsBareClicksByArea()
    {
        QVERIFY(RegionPicker::isClickSelection(1, 19));
        QVERIFY(RegionPicker::isClickSelection(3, 6));
        QVERIFY(!RegionPicker::isClickSelection(4, 5));
        QVERIFY(!RegionPicker::isClickSelection(100, 100));
        QVERIFY(!RegionPicker::isClickSelection(0, 10));
    }
};

QTEST_APPLESS_MAIN(RegionPickerTest)
#include "test_regionpicker.moc"
