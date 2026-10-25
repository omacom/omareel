// qcolor: QColor parsing, names, lighter/darker, setAlphaF and fromRgbF.

#include "common.h"

#include <QColor>
#include <QDir>
#include <QJsonArray>
#include <QRandomGenerator>

#include <cmath>
#include <cstdio>
#include <limits>

namespace Oracle {

namespace {

QByteArray floats(const QColor &color)
{
    return num(color.redF()) + ' ' + num(color.greenF()) + ' ' + num(color.blueF()) + ' '
        + num(color.alphaF());
}

QByteArray rgba64Hex(const QColor &color)
{
    const QRgba64 rgba = color.rgba64();
    char buffer[32];
    std::snprintf(buffer, sizeof buffer, "%04x%04x%04x%04x", rgba.red(), rgba.green(),
                  rgba.blue(), rgba.alpha());
    return buffer;
}

QString out(const Context &context, const char *name)
{
    return QDir(context.output).filePath(QString::fromLatin1(name));
}

const QList<int> Factors{50, 100, 101, 106, 108, 110, 112, 115, 118, 120, 150, 200, 300};

} // namespace

void runQColor(const Context &context)
{
    Text parse;
    const QJsonArray inputs = readJson(QDir(context.corpus).filePath(QStringLiteral("qcolor/parse.json"))).toArray();
    for (const QJsonValue &input : inputs) {
        const QString text = input.toString();
        const QColor color(text);
        parse.row({quote(text), boolText(color.isValid()), colorText(color),
                   color.name().toLatin1(), floats(color),
                   boolText(QColor::isValidColorName(text))});
    }
    writeFile(out(context, "parse.txt"), parse.data());

    Text names;
    for (const QString &name : QColor::colorNames())
        names.row({quote(name), colorText(QColor(name))});
    writeFile(out(context, "color-names.txt"), names.data());

    // Every 17th value per channel, so all of 0x00, 0x11, ... 0xff.
    Text grid;
    for (int r = 0; r <= 255; r += 17)
        for (int g = 0; g <= 255; g += 17)
            for (int b = 0; b <= 255; b += 17) {
                const QColor color(r, g, b);
                const QByteArray name = color.name().toLatin1();
                for (const int factor : Factors) {
                    const QColor lighter = color.lighter(factor);
                    const QColor darker = color.darker(factor);
                    QList<QByteArray> cells{name, QByteArray::number(factor), rgba64Hex(lighter),
                                            lighter.name().toLatin1(), rgba64Hex(darker),
                                            darker.name().toLatin1()};
                    if (lighter.spec() != QColor::Rgb || darker.spec() != QColor::Rgb)
                        cells << specName(lighter.spec()) << specName(darker.spec());
                    grid.row(cells);
                }
            }
    writeFile(out(context, "lighter-darker-grid.txt"), grid.data());

    // Colours with 16-bit channels, alpha, other specs, and the factors <= 0 that are no-ops.
    Text special;
    QList<QColor> colors;
    for (const char *name : {"#7aa2f7", "#1a1b26", "#a9b1d6", "#faf4ed", "#80ff0000", "#00000000",
                             "#123456789abc", "#fff000fff", "#ffffff", "#000000", "#808080",
                             "#ff0000", "#00ff00", "#0000ff", "#010101", "#fefefe"})
        colors << QColor(QString::fromLatin1(name));
    colors << QColor::fromRgbF(0.1f, 0.2f, 0.3f, 0.4f) << QColor::fromRgbF(1.2f, -0.1f, 0.5f)
           << QColor::fromHsv(200, 128, 250) << QColor::fromHsl(10, 200, 100)
           << QColor::fromCmyk(10, 20, 30, 40) << QColor();
    QRandomGenerator random(1234);
    for (int i = 0; i < 64; ++i)
        colors << QColor::fromRgba64(quint16(random.bounded(65536)), quint16(random.bounded(65536)),
                                     quint16(random.bounded(65536)), quint16(random.bounded(65536)));
    for (const QColor &color : colors) {
        for (const int factor : QList<int>{-10, 0, 1, 99} + Factors) {
            special.row({colorText(color), QByteArray::number(factor),
                         colorText(color.lighter(factor)), colorText(color.darker(factor))});
        }
    }
    writeFile(out(context, "lighter-darker-special.txt"), special.data());

    Text alpha;
    QVector<double> alphas;
    for (int i = 0; i <= 1000; ++i) alphas << i / 1000.0;
    for (int i = 0; i <= 255; ++i) alphas << i / 255.0;
    for (const double value : {.04, .06, .07, .08, .10, .11, .12, .14, .15, .16, .18, .2, .22,
                               .25, .28, .35, .36, .40, .5, .56, .7, .95, .97, 1.0 / 3.0,
                               0.5 / 65535.0, 1.5 / 65535.0, 1e-9, 0.99999999, -0.0})
        alphas << value;
    alphas << -0.5 << 1.5 << -1e-9 << 1.0000001 << std::numeric_limits<double>::quiet_NaN();
    for (const double value : alphas) {
        QColor color(QStringLiteral("#7aa2f7"));
        color.setAlphaF(value); // float parameter: the double converts first, as in Theme
        alpha.row({hexBits(value), num(value), QByteArray::number(color.alpha()),
                   QByteArray::number(color.rgba64().alpha()), num(color.alphaF()),
                   colorText(color)});
    }
    writeFile(out(context, "set-alpha-f.txt"), alpha.data());

    Text fromRgbF;
    QVector<double> channel;
    for (int i = 0; i <= 512; ++i) channel << i / 512.0;
    for (int i = 0; i <= 255; ++i) channel << i / 255.0 << (i + 0.5) / 255.0;
    for (const double value : {-0.5, -1e-9, -0.0, 1e-9, 1.0000001, 1.5, 2.0, 0.123456789,
                               0.999999, 1.0 / 3.0, 2.0 / 3.0})
        channel << value;
    const auto row = [&fromRgbF](double r, double g, double b, double a) {
        const QColor color = QColor::fromRgbF(r, g, b, a);
        fromRgbF.row({num(r), num(g), num(b), num(a), colorText(color), floats(color)});
    };
    for (const double value : channel) row(value, 0.25, 0.5, 1.0);
    for (const double value : channel) row(0.0, 0.0, 0.0, value);
    for (int i = 0; i < 1500; ++i)
        row(random.generateDouble(), random.generateDouble(), random.generateDouble(),
            random.generateDouble());
    for (int i = 0; i < 100; ++i)
        row(random.generateDouble() * 2.0 - 0.5, random.generateDouble() * 2.0 - 0.5,
            random.generateDouble() * 2.0 - 0.5, random.generateDouble());
    writeFile(out(context, "from-rgb-f.txt"), fromRgbF.data());
}

} // namespace Oracle
