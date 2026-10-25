// qnumber, qround and easing: Qt's number formatting and parsing, Qt's rounding and fuzzy
// comparisons, and Omareel::Easing.

#include "common.h"

#include "core/Easing.h"

#include <QDir>
#include <QJsonArray>
#include <QLocale>
#include <QtMath>

#include <algorithm>
#include <cmath>
#include <limits>

namespace Oracle {

namespace {

const QList<int> Precisions{0, 1, 2, 3, 6, 8, 17, QLocale::FloatingPointShortest};

QString path(const Context &context, const char *relative)
{
    return QDir(context.corpus).filePath(QString::fromLatin1(relative));
}

QString out(const Context &context, const char *name)
{
    return QDir(context.output).filePath(QString::fromLatin1(name));
}

QByteArray okCell(bool ok)
{
    return ok ? "ok" : "fail";
}

// Rounding inputs: every corpus double plus a grid of quarters and the doubles either side of
// every half, which is where qRound's add-a-half-and-truncate shows.
QVector<double> roundInputs(const Context &context)
{
    QVector<double> values = readDoubles(path(context, "numbers/doubles.txt"));
    for (int i = -400; i <= 400; ++i) {
        const double quarter = i / 4.0;
        values << quarter;
        if (i % 2 != 0) {
            values << std::nextafter(quarter, -std::numeric_limits<double>::infinity());
            values << std::nextafter(quarter, std::numeric_limits<double>::infinity());
        }
    }
    for (const double big : {2147483646.5, 2147483646.4999998, -2147483647.5, 4503599627370495.5,
                             4503599627370496.5, 9007199254740991.0, 4611686018427387904.5})
        values << big << -big;
    return values;
}

} // namespace

void runQNumber(const Context &context)
{
    Text number;
    for (const double value : readDoubles(path(context, "numbers/doubles-format.txt"))) {
        for (const char format : {'f', 'e', 'g'}) {
            QList<QByteArray> cells{hexBits(value), QByteArray(1, format)};
            for (const int precision : Precisions)
                cells << QString::number(value, format, precision).toUtf8();
            number.row(cells);
        }
    }
    writeFile(out(context, "number.txt"), number.data());

    Text shortest;
    for (const double value : readDoubles(path(context, "numbers/doubles.txt"))) {
        shortest.row({hexBits(value), QByteArray::number(value, 'g', QLocale::FloatingPointShortest),
                      QString::number(value).toUtf8()});
    }
    writeFile(out(context, "bytearray-shortest.txt"), shortest.data());

    Text parse;
    for (const QJsonValue &item : readJson(path(context, "numbers/strings.json")).toArray()) {
        const QString text = item.toString();
        bool doubleOk = false, intOk = false, longOk = false, baseZeroOk = false;
        const double asDouble = text.toDouble(&doubleOk);
        const int asInt = text.toInt(&intOk);
        const qlonglong asLong = text.toLongLong(&longOk);
        const int baseZero = text.toUtf8().toInt(&baseZeroOk, 0);
        parse.row({quote(text), num(asDouble), hexBits(asDouble), okCell(doubleOk),
                   QByteArray::number(asInt), okCell(intOk), QByteArray::number(asLong),
                   okCell(longOk), QByteArray::number(baseZero), okCell(baseZeroOk)});
    }
    writeFile(out(context, "parse.txt"), parse.data());
}

void runQRound(const Context &context)
{
    Text round;
    for (const double value : roundInputs(context)) {
        QByteArray asInt = "-";
        QByteArray asInt64 = "-";
        if (std::isfinite(value) && std::abs(value) < 2147483647.0)
            asInt = QByteArray::number(qRound(value));
        if (std::isfinite(value) && std::abs(value) < 9.2e18)
            asInt64 = QByteArray::number(qRound64(value));
        round.row({hexBits(value), num(value), asInt, asInt64});
    }
    writeFile(out(context, "round.txt"), round.data());

    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    const QVector<double> fuzzyValues{
        0.0, -0.0, 1e-12, std::nextafter(1e-12, 0.0), std::nextafter(1e-12, 1.0), 1e-13, 1e-300,
        5e-324, 0.5, 0.55, 0.55 + 1e-13, 0.55 + 1e-12, 0.75, 0.75 + 1e-13, 0.75 + 1e-12,
        0.7500000000001, 0.02, 0.020000000000001, 1.0, std::nextafter(1.0, 2.0), -1.0, 2.0,
        2.000000000002, 18.0, 18.00000000001, 20.0, 20.00000000002, 20.0000000001, 25.0,
        25.000000000025, 90.0, 90.0000000001, 1e300, inf, -inf, nan};
    Text fuzzy;
    for (const double a : fuzzyValues)
        for (const double b : fuzzyValues)
            fuzzy.row({hexBits(a), hexBits(b), boolText(qFuzzyCompare(a, b))});
    writeFile(out(context, "fuzzy-compare.txt"), fuzzy.data());
    Text isNull;
    for (const double a : fuzzyValues) isNull.row({hexBits(a), boolText(qFuzzyIsNull(a))});
    writeFile(out(context, "fuzzy-is-null.txt"), isNull.data());

    // Clamping as the core does it: qBound, std::clamp, and both argument orders of qMin/qMax
    // and std::min/std::max, which differ for NaN and signed zeros.
    const QVector<double> boundValues{nan, -nan, -inf, -1.0, -0.5, -0.0, 0.0, 1e-300, 0.25, 0.5,
                                      0.75, 1.0, std::nextafter(1.0, 2.0), 1.5, inf};
    Text bound;
    for (const double x : boundValues) {
        bound.row({hexBits(x), hexBits(qBound(0.0, x, 1.0)), hexBits(std::clamp(x, 0.0, 1.0)),
                   hexBits(qMax(0.0, x)), hexBits(qMax(x, 0.0)), hexBits(qMin(x, 1.0)),
                   hexBits(qMin(1.0, x)), hexBits(std::max(0.0, x)), hexBits(std::max(x, 0.0)),
                   hexBits(std::min(x, 1.0)), hexBits(std::min(1.0, x)),
                   hexBits(std::clamp(x, -0.0, 0.0)), hexBits(qBound(-0.0, x, 0.0))});
    }
    writeFile(out(context, "bound.txt"), bound.data());
}

void runEasing(const Context &context)
{
    QVector<double> inputs;
    for (int i = -1024; i <= 5120; ++i) inputs << i / 4096.0;
    for (int i = 0; i <= 1000; ++i) inputs << i / 1000.0;
    for (const double value : {0.5, 0.0, 1.0})
        inputs << std::nextafter(value, -1.0) << std::nextafter(value, 2.0);
    const double inf = std::numeric_limits<double>::infinity();
    inputs << -0.0 << 1e-300 << 5e-324 << 0.49999999999999994 << 1e300 << -1e300 << inf << -inf
           << std::numeric_limits<double>::quiet_NaN();
    for (const double value : readDoubles(QDir(context.corpus).filePath(QStringLiteral("numbers/doubles-format.txt"))))
        if (value >= -0.5 && value <= 1.5) inputs << value;
    Text easing;
    for (const double t : inputs)
        easing.row({hexBits(t), num(t), num(Omareel::Easing::easeInOutCubic(t)),
                    hexBits(Omareel::Easing::easeInOutCubic(t))});
    writeFile(out(context, "ease-in-out-cubic.txt"), easing.data());
}

} // namespace Oracle
