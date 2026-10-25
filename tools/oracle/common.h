#pragma once

// Shared formatting for every golden file. tests/golden/README.md describes these rules; a
// Rust test that regenerates a golden file has to print values exactly this way.

#include <QByteArray>
#include <QColor>
#include <QJsonValue>
#include <QString>
#include <QStringList>
#include <QVector>

#include <cstdint>
#include <functional>

namespace Oracle {

// %.17g, except NaN is "nan" (any sign or payload) and infinities are "inf" / "-inf".
QByteArray num(double value);
// The 64-bit pattern of a double as 16 lowercase hex digits.
QByteArray hexBits(double value);
double fromHexBits(const QByteArray &hex, bool *ok = nullptr);
QByteArray boolText(bool value);
// A QString as a quoted string: see README "Strings".
QByteArray quote(const QString &value);
// Raw bytes as a quoted string: printable ASCII stays, everything else is \xNN.
QByteArray quoteBytes(const QByteArray &value);
QByteArray quoteList(const QStringList &values);
// "<spec> <rrrrggggbbbbaaaa> <#aarrggbb>", plus four floats for ExtendedRgb.
QByteArray colorText(const QColor &color);
QByteArray specName(QColor::Spec spec);
QByteArray jsonTypeName(QJsonValue::Type type);
// The typed-value cells used by the qjson dumps (README "Typed rows").
QByteArray typedRow(const QJsonValue &value);
// A QJsonValue as Compact JSON, by wrapping it in a one-element array: "[<value>]".
QByteArray wrappedJson(const QJsonValue &value);

// Replaces the hermetic environment's directories in a path or message with $HOME, $CORPUS...
QString sanitize(const QString &text);
void captureSanitizeRoots(const QString &corpus);

QByteArray readFile(const QString &path, bool *ok = nullptr);
// Writes the bytes, creating the parent directories.
void writeFile(const QString &path, const QByteArray &data);
// File names in a directory, sorted byte-wise by their UTF-8 encoding.
QStringList sortedEntries(const QString &directory, bool directories);
// Lines of a text corpus file, skipping empty lines and lines that start with '#'.
QList<QByteArray> corpusLines(const QString &path);
QVector<double> readDoubles(const QString &path);
QJsonValue readJson(const QString &path);
void fail(const QString &message);

// An output text file assembled in memory and written once.
class Text
{
public:
    Text &operator<<(const QByteArray &bytes) { m_data += bytes; return *this; }
    Text &operator<<(const char *bytes) { m_data += bytes; return *this; }
    Text &operator<<(int value) { m_data += QByteArray::number(value); return *this; }
    Text &operator<<(qint64 value) { m_data += QByteArray::number(value); return *this; }
    Text &operator<<(long value) { m_data += QByteArray::number(qint64(value)); return *this; }
    // Joins cells with tabs and ends the line.
    void row(const QList<QByteArray> &cells);
    const QByteArray &data() const { return m_data; }

private:
    QByteArray m_data;
};

struct Context {
    QString corpus;
    QString output;
    QString scratch; // a fresh directory under $HOME for files the cases create
};

using Family = std::function<void(const Context &)>;

void runQJson(const Context &context);
void runQNumber(const Context &context);
void runQRound(const Context &context);
void runQColor(const Context &context);
void runEasing(const Context &context);
void runInputLog(const Context &context);
void runCursorPath(const Context &context);
void runZoomTimeline(const Context &context);
void runProject(const Context &context);
void runOmarchyPaths(const Context &context);
void runTheme(const Context &context);

} // namespace Oracle
