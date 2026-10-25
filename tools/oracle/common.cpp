#include "common.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonObject>
#include <QJsonDocument>
#include <QJsonParseError>
#include <QVariant>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <iostream>

namespace Oracle {

QByteArray num(double value)
{
    if (std::isnan(value)) return "nan";
    if (std::isinf(value)) return value < 0 ? "-inf" : "inf";
    char buffer[64];
    std::snprintf(buffer, sizeof buffer, "%.17g", value);
    return buffer;
}

QByteArray hexBits(double value)
{
    std::uint64_t bits = 0;
    std::memcpy(&bits, &value, sizeof bits);
    char buffer[32];
    std::snprintf(buffer, sizeof buffer, "%016llx", static_cast<unsigned long long>(bits));
    return buffer;
}

double fromHexBits(const QByteArray &hex, bool *ok)
{
    bool parsed = false;
    const std::uint64_t bits = hex.trimmed().toULongLong(&parsed, 16);
    if (ok) *ok = parsed && hex.trimmed().size() == 16;
    double value = 0.0;
    std::memcpy(&value, &bits, sizeof value);
    return value;
}

QByteArray boolText(bool value)
{
    return value ? "true" : "false";
}

static void appendEscape(QByteArray &out, uint codePoint)
{
    char buffer[16];
    std::snprintf(buffer, sizeof buffer, "\\u{%x}", codePoint);
    out += buffer;
}

QByteArray quote(const QString &value)
{
    QByteArray out("\"");
    const qsizetype size = value.size();
    for (qsizetype i = 0; i < size; ++i) {
        const char16_t unit = value.at(i).unicode();
        uint codePoint = unit;
        if (QChar::isHighSurrogate(unit) && i + 1 < size
            && QChar::isLowSurrogate(value.at(i + 1).unicode())) {
            codePoint = QChar::surrogateToUcs4(unit, value.at(i + 1).unicode());
            ++i;
        } else if (QChar::isSurrogate(unit)) {
            appendEscape(out, unit);
            continue;
        }
        if (codePoint == '"') out += "\\\"";
        else if (codePoint == '\\') out += "\\\\";
        else if (codePoint < 0x20 || codePoint == 0x7f || (codePoint >= 0x80 && codePoint <= 0x9f)
                 || codePoint == 0x2028 || codePoint == 0x2029 || codePoint == 0xfeff
                 || codePoint == 0xfffe || codePoint == 0xffff)
            appendEscape(out, codePoint);
        else {
            const char32_t one[1] = {char32_t(codePoint)};
            out += QString::fromUcs4(one, 1).toUtf8();
        }
    }
    out += '"';
    return out;
}

QByteArray quoteBytes(const QByteArray &value)
{
    QByteArray out("\"");
    for (const char c : value) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte == '"') out += "\\\"";
        else if (byte == '\\') out += "\\\\";
        else if (byte >= 0x20 && byte < 0x7f) out += c;
        else {
            char buffer[8];
            std::snprintf(buffer, sizeof buffer, "\\x%02x", byte);
            out += buffer;
        }
    }
    out += '"';
    return out;
}

QByteArray quoteList(const QStringList &values)
{
    QByteArray out("[");
    for (qsizetype i = 0; i < values.size(); ++i) {
        if (i) out += ',';
        out += quote(values.at(i));
    }
    out += ']';
    return out;
}

QByteArray specName(QColor::Spec spec)
{
    switch (spec) {
    case QColor::Invalid: return "Invalid";
    case QColor::Rgb: return "Rgb";
    case QColor::Hsv: return "Hsv";
    case QColor::Cmyk: return "Cmyk";
    case QColor::Hsl: return "Hsl";
    case QColor::ExtendedRgb: return "ExtendedRgb";
    }
    return "Spec" + QByteArray::number(int(spec));
}

QByteArray colorText(const QColor &color)
{
    const QRgba64 rgba = color.rgba64();
    char buffer[32];
    std::snprintf(buffer, sizeof buffer, "%04x%04x%04x%04x", rgba.red(), rgba.green(),
                  rgba.blue(), rgba.alpha());
    QByteArray out = specName(color.spec()) + ' ' + buffer + ' '
        + color.name(QColor::HexArgb).toLatin1();
    if (color.spec() == QColor::ExtendedRgb) {
        out += ' ' + num(color.redF()) + ' ' + num(color.greenF()) + ' ' + num(color.blueF())
            + ' ' + num(color.alphaF());
    }
    return out;
}

QByteArray jsonTypeName(QJsonValue::Type type)
{
    switch (type) {
    case QJsonValue::Null: return "Null";
    case QJsonValue::Bool: return "Bool";
    case QJsonValue::Double: return "Double";
    case QJsonValue::String: return "String";
    case QJsonValue::Array: return "Array";
    case QJsonValue::Object: return "Object";
    case QJsonValue::Undefined: return "Undefined";
    }
    return "Type" + QByteArray::number(int(type));
}

QByteArray typedRow(const QJsonValue &value)
{
    const QVariant variant = value.toVariant();
    const char *typeName = variant.typeName();
    QList<QByteArray> cells{
        jsonTypeName(value.type()),
        QByteArray::number(value.toInt(-7)),
        QByteArray::number(value.toInteger(-7)),
        num(value.toDouble(-7)),
        boolText(value.toBool(false)),
        quote(value.toString(QStringLiteral("<def>"))),
        typeName ? QByteArray(typeName) : QByteArray("<invalid>"),
        quote(variant.toString()),
        QByteArray::number(variant.toLongLong()),
    };
    return cells.join('\t');
}

QByteArray wrappedJson(const QJsonValue &value)
{
    return QJsonDocument(QJsonArray{value}).toJson(QJsonDocument::Compact);
}

namespace {
struct Root { QString name; QString path; };
QVector<Root> &roots()
{
    static QVector<Root> value;
    return value;
}
bool pathCharacter(QChar c)
{
    return c.isLetterOrNumber() || c == QLatin1Char('.') || c == QLatin1Char('_')
        || c == QLatin1Char('-');
}
}

void captureSanitizeRoots(const QString &corpus)
{
    auto &list = roots();
    list.clear();
    for (const char *name : {"HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME",
                             "XDG_DATA_HOME", "XDG_RUNTIME_DIR", "TMPDIR"}) {
        const QString path = qEnvironmentVariable(name);
        if (!path.isEmpty() && path != QLatin1String("/"))
            list << Root{QLatin1Char('$') + QLatin1String(name), QDir::cleanPath(path)};
    }
    list << Root{QStringLiteral("$CORPUS"), QDir::cleanPath(QFileInfo(corpus).absoluteFilePath())};
    std::stable_sort(list.begin(), list.end(), [](const Root &a, const Root &b) {
        return a.path.size() > b.path.size();
    });
}

QString sanitize(const QString &text)
{
    QString out;
    qsizetype i = 0;
    while (i < text.size()) {
        bool replaced = false;
        for (const Root &root : roots()) {
            if (!text.mid(i).startsWith(root.path)) continue;
            const qsizetype end = i + root.path.size();
            if (end < text.size() && pathCharacter(text.at(end))) continue;
            out += root.name;
            i = end;
            replaced = true;
            break;
        }
        if (!replaced) out += text.at(i++);
    }
    return out;
}

QByteArray readFile(const QString &path, bool *ok)
{
    QFile file(path);
    if (!file.open(QIODevice::ReadOnly)) {
        if (ok) *ok = false;
        return {};
    }
    if (ok) *ok = true;
    return file.readAll();
}

void writeFile(const QString &path, const QByteArray &data)
{
    QDir().mkpath(QFileInfo(path).absolutePath());
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate) || file.write(data) != data.size())
        fail(QStringLiteral("could not write %1").arg(path));
}

QStringList sortedEntries(const QString &directory, bool directories)
{
    QStringList names = QDir(directory).entryList(
        (directories ? QDir::Dirs : QDir::Files) | QDir::NoDotAndDotDot | QDir::Hidden);
    std::sort(names.begin(), names.end(), [](const QString &a, const QString &b) {
        return a.toUtf8() < b.toUtf8();
    });
    return names;
}

QList<QByteArray> corpusLines(const QString &path)
{
    bool ok = false;
    const QByteArray data = readFile(path, &ok);
    if (!ok) fail(QStringLiteral("missing corpus file %1").arg(path));
    QList<QByteArray> lines;
    for (const QByteArray &line : data.split('\n')) {
        if (line.isEmpty() || line.startsWith('#')) continue;
        lines << line;
    }
    return lines;
}

QVector<double> readDoubles(const QString &path)
{
    QVector<double> values;
    for (const QByteArray &line : corpusLines(path)) {
        bool ok = false;
        values << fromHexBits(line, &ok);
        if (!ok) fail(QStringLiteral("bad hex double '%1' in %2").arg(QString::fromLatin1(line), path));
    }
    return values;
}

QJsonValue readJson(const QString &path)
{
    bool ok = false;
    const QByteArray data = readFile(path, &ok);
    if (!ok) fail(QStringLiteral("missing corpus file %1").arg(path));
    QJsonParseError error;
    const QJsonDocument document = QJsonDocument::fromJson(data, &error);
    if (error.error != QJsonParseError::NoError)
        fail(QStringLiteral("corpus file %1 is not JSON: %2").arg(path, error.errorString()));
    if (document.isArray()) return document.array();
    return document.object();
}

void fail(const QString &message)
{
    std::cerr << "omareel-oracle: " << message.toStdString() << std::endl;
    std::exit(1);
}

void Text::row(const QList<QByteArray> &cells)
{
    m_data += cells.join('\t');
    m_data += '\n';
}

} // namespace Oracle
