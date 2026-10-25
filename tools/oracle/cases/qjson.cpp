// qjson: QJsonDocument parsing and writing, QJsonValue conversions.

#include "common.h"

#include <QDir>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QPointF>
#include <QVariant>

#include <cmath>
#include <limits>

namespace Oracle {

namespace {

void dumpTyped(Text &out, const QString &pointer, const QJsonValue &value)
{
    out.row({quote(pointer), typedRow(value)});
    if (value.isObject()) {
        const QJsonObject object = value.toObject();
        for (auto it = object.begin(); it != object.end(); ++it) {
            QString key = it.key();
            key.replace(QLatin1Char('~'), QStringLiteral("~0"));
            key.replace(QLatin1Char('/'), QStringLiteral("~1"));
            dumpTyped(out, pointer + QLatin1Char('/') + key, it.value());
        }
    } else if (value.isArray()) {
        const QJsonArray array = value.toArray();
        for (qsizetype i = 0; i < array.size(); ++i)
            dumpTyped(out, pointer + QLatin1Char('/') + QString::number(i), array.at(i));
    }
}

int depth(const QJsonValue &value)
{
    int deepest = 0;
    if (value.isObject()) {
        const QJsonObject object = value.toObject();
        for (auto it = object.begin(); it != object.end(); ++it) deepest = qMax(deepest, depth(it.value()));
        return deepest + 1;
    }
    if (value.isArray()) {
        for (const QJsonValue &child : value.toArray()) deepest = qMax(deepest, depth(child));
        return deepest + 1;
    }
    return 0;
}

// Documents nested deeper than this (the depth-limit probes) only get their parse.txt row and
// their Compact bytes; their Indented bytes and typed rows grow quadratically and say nothing
// the shallower documents do not.
constexpr int MaxDumpDepth = 64;

QJsonValue documentRoot(const QJsonDocument &document)
{
    if (document.isObject()) return document.object();
    if (document.isArray()) return document.array();
    return QJsonValue(QJsonValue::Undefined);
}

QByteArray documentKind(const QJsonDocument &document)
{
    if (document.isObject()) return "object";
    if (document.isArray()) return "array";
    if (document.isNull()) return "null";
    return "other";
}

QList<QByteArray> errorCells(const QJsonParseError &error)
{
    return {"error", QByteArray::number(int(error.error)), QByteArray::number(error.offset),
            quote(error.errorString())};
}

void runDocuments(const Context &context, Text &parse, Text &typed)
{
    const QString directory = QDir(context.corpus).filePath(QStringLiteral("qjson/docs"));
    for (const QString &file : sortedEntries(directory, false)) {
        const QString name = file.chopped(5);
        const QByteArray data = readFile(QDir(directory).filePath(file));
        QJsonParseError error;
        const QJsonDocument document = QJsonDocument::fromJson(data, &error);
        if (error.error != QJsonParseError::NoError) {
            parse.row(QList<QByteArray>{name.toUtf8()} + errorCells(error));
            continue;
        }
        parse.row({name.toUtf8(), "ok", documentKind(document),
                   QByteArray::number(error.offset), quote(error.errorString())});
        const QString base = QDir(context.output).filePath(QStringLiteral("docs/") + name);
        writeFile(base + QStringLiteral(".compact.json"), document.toJson(QJsonDocument::Compact));
        if (depth(documentRoot(document)) > MaxDumpDepth) continue;
        writeFile(base + QStringLiteral(".indented.json"), document.toJson(QJsonDocument::Indented));
        typed << "== " << name.toUtf8() << "\n";
        dumpTyped(typed, QString(), documentRoot(document));
    }
}

void runLiterals(const Context &context, const QString &corpusName, const QString &outputName)
{
    const QByteArray data = readFile(QDir(context.corpus).filePath(corpusName));
    QList<QByteArray> lines = data.split('\n');
    if (!lines.isEmpty() && lines.last().isEmpty()) lines.removeLast();
    Text out;
    int lineNumber = 0;
    for (const QByteArray &literal : lines) {
        ++lineNumber;
        QJsonParseError error;
        const QJsonDocument document = QJsonDocument::fromJson("[" + literal + "]", &error);
        QList<QByteArray> cells{QByteArray::number(lineNumber), quoteBytes(literal)};
        if (error.error != QJsonParseError::NoError) {
            out.row(cells + errorCells(error));
            continue;
        }
        const QJsonArray array = document.array();
        cells << "ok" << QByteArray::number(array.size())
              << document.toJson(QJsonDocument::Compact);
        if (!array.isEmpty()) cells << typedRow(array.at(0));
        out.row(cells);
    }
    writeFile(QDir(context.output).filePath(outputName), out.data());
}

void runDoubles(const Context &context)
{
    Text out;
    for (const double value : readDoubles(QDir(context.corpus).filePath(QStringLiteral("numbers/doubles.txt")))) {
        const QByteArray json = wrappedJson(QJsonValue(value));
        const QJsonValue back = QJsonDocument::fromJson(json).array().at(0);
        const char *variantType = back.toVariant().typeName();
        out.row({hexBits(value), json, jsonTypeName(back.type()),
                 variantType ? variantType : "<invalid>", hexBits(back.toDouble(-7))});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("doubles.txt")), out.data());

    Text ints;
    for (const QByteArray &line : corpusLines(QDir(context.corpus).filePath(QStringLiteral("numbers/int64.txt")))) {
        bool ok = false;
        const qint64 value = line.toLongLong(&ok);
        if (!ok) fail(QStringLiteral("bad int64 corpus line %1").arg(QString::fromLatin1(line)));
        const QByteArray json = wrappedJson(QJsonValue(value));
        const QJsonValue back = QJsonDocument::fromJson(json).array().at(0);
        const char *variantType = back.toVariant().typeName();
        ints.row({QByteArray::number(value), json, variantType ? variantType : "<invalid>",
                  QByteArray::number(back.toVariant().toLongLong()), hexBits(back.toDouble(-7))});
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("int64.txt")), ints.data());
}

QString loneSurrogates()
{
    QString value = QStringLiteral("a");
    value += QChar(0xd800);
    value += QStringLiteral("b");
    value += QChar(0xdc00);
    value += QChar(0xdbff);
    return value;
}

QList<QPair<QString, QJsonDocument>> builtDocuments()
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    QList<QPair<QString, QJsonDocument>> documents;

    QJsonObject scalars;
    scalars.insert(QStringLiteral("two"), 2.0);
    scalars.insert(QStringLiteral("negativeZero"), -0.0);
    scalars.insert(QStringLiteral("int"), 7);
    scalars.insert(QStringLiteral("int64Max"), std::numeric_limits<qint64>::max());
    scalars.insert(QStringLiteral("int64Min"), std::numeric_limits<qint64>::min());
    scalars.insert(QStringLiteral("int64Big"), qint64(1) << 60);
    scalars.insert(QStringLiteral("int64TwoPow53Plus1"), (qint64(1) << 53) + 1);
    scalars.insert(QStringLiteral("doubleTwoPow53"), 9007199254740992.0);
    scalars.insert(QStringLiteral("doubleTwoPow63"), 9223372036854775808.0);
    scalars.insert(QStringLiteral("double1e20"), 1e20);
    scalars.insert(QStringLiteral("double1e300"), 1e300);
    scalars.insert(QStringLiteral("doubleHalf"), 0.5);
    scalars.insert(QStringLiteral("doubleThird"), 1.0 / 3.0);
    scalars.insert(QStringLiteral("doubleTiny"), 5e-324);
    scalars.insert(QStringLiteral("float"), double(0.1f));
    scalars.insert(QStringLiteral("nan"), nan);
    scalars.insert(QStringLiteral("inf"), inf);
    scalars.insert(QStringLiteral("negativeInf"), -inf);
    scalars.insert(QStringLiteral("true"), true);
    scalars.insert(QStringLiteral("false"), false);
    scalars.insert(QStringLiteral("null"), QJsonValue());
    scalars.insert(QStringLiteral("string"), QStringLiteral("x"));
    scalars.insert(QStringLiteral("emptyString"), QString());
    documents << qMakePair(QStringLiteral("scalars"), QJsonDocument(scalars));

    QJsonObject undefinedObject{{QStringLiteral("kept"), 1}};
    undefinedObject.insert(QStringLiteral("undefined"), QJsonValue(QJsonValue::Undefined));
    undefinedObject.insert(QStringLiteral("kept"), QJsonValue(QJsonValue::Undefined));
    undefinedObject.insert(QStringLiteral("after"), 2);
    documents << qMakePair(QStringLiteral("undefined-in-object"), QJsonDocument(undefinedObject));
    QJsonArray undefinedArray;
    undefinedArray.append(QJsonValue(QJsonValue::Undefined));
    undefinedArray.append(1);
    undefinedArray.append(QJsonValue(QJsonValue::Undefined));
    documents << qMakePair(QStringLiteral("undefined-in-array"), QJsonDocument(undefinedArray));
    QJsonObject undefinedNested{{QStringLiteral("o"), QJsonObject{{QStringLiteral("u"),
        QJsonValue(QJsonValue::Undefined)}}}, {QStringLiteral("a"), QJsonArray{QJsonValue(QJsonValue::Undefined)}}};
    documents << qMakePair(QStringLiteral("undefined-initializer-list"), QJsonDocument(undefinedNested));

    QJsonObject nested;
    nested.insert(QStringLiteral("z"), QJsonObject{{QStringLiteral("b"), 1}, {QStringLiteral("a"), QJsonArray{}}});
    nested.insert(QStringLiteral("y"), QJsonArray{QJsonObject{}, QJsonArray{}, QJsonArray{QJsonArray{}}});
    nested.insert(QStringLiteral("x"), QJsonObject{{QStringLiteral("deep"), QJsonObject{
        {QStringLiteral("deeper"), QJsonObject{{QStringLiteral("deepest"), QJsonArray{1, 2.5, QStringLiteral("s")}}}}}}});
    nested.insert(QStringLiteral("a"), 1);
    nested.insert(QStringLiteral("a"), 2); // replaces
    nested.insert(QStringLiteral("removed"), 3);
    nested.remove(QStringLiteral("removed"));
    QJsonArray inserted{1, 2, 3};
    inserted.insert(1, QStringLiteral("one-and-a-half"));
    inserted.replace(0, QJsonValue());
    inserted.prepend(QJsonObject{{QStringLiteral("first"), true}});
    inserted.removeAt(3);
    nested.insert(QStringLiteral("array"), inserted);
    documents << qMakePair(QStringLiteral("nested"), QJsonDocument(nested));

    QJsonObject strings;
    strings.insert(QStringLiteral("control"), QStringLiteral("\x01\x02\t\n\r\b\f\x1f\x7f"));
    strings.insert(QStringLiteral("nul"), QString(QChar(0)) + QStringLiteral("x"));
    strings.insert(QStringLiteral("quotes"), QStringLiteral("\"'\\/"));
    strings.insert(QStringLiteral("unicode"), QStringLiteral(u"é€\U0001F600  ﻿￿"));
    strings.insert(QStringLiteral("c1"), QStringLiteral(u"\u0080\u009f "));
    strings.insert(QStringLiteral("lone"), loneSurrogates());
    strings.insert(loneSurrogates(), QStringLiteral("key with lone surrogates"));
    strings.insert(QStringLiteral(u"é"), 1);
    strings.insert(QStringLiteral("e"), 2);
    strings.insert(QStringLiteral(u"\U0001F600"), 3);
    strings.insert(QStringLiteral(u"！"), 4);
    strings.insert(QString(QChar(0)), 5);
    documents << qMakePair(QStringLiteral("strings"), QJsonDocument(strings));

    QJsonObject latin1;
    latin1.insert(QLatin1String("b"), 1);
    latin1.insert(QStringLiteral("a"), 2);
    latin1.insert(QLatin1String("\xe9"), 3);
    latin1.insert(QStringLiteral(u"è"), 4);
    latin1.insert(QLatin1String("\xff"), 5);
    latin1.insert(QStringLiteral(u"Ā"), 6);
    latin1.insert(QLatin1String("A"), 7);
    documents << qMakePair(QStringLiteral("latin1-keys"), QJsonDocument(latin1));

    QJsonArray numbers;
    for (const double value : {0.0, -0.0, 1.0, 1.5, 0.1, 1e15, 1e16, 1e17, 1e21, 1e22, 1e-7,
                               123456789012345678.0, 9007199254740993.0, -1e300, 2.5e-324})
        numbers.append(value);
    for (const qint64 value : {qint64(0), qint64(-1), qint64(1) << 53, (qint64(1) << 53) + 1,
                               std::numeric_limits<qint64>::max()})
        numbers.append(value);
    numbers.append(int(-2147483647 - 1));
    numbers.append(QJsonValue(qint64(42)).toDouble());
    documents << qMakePair(QStringLiteral("numbers"), QJsonDocument(numbers));

    QVariantMap map{{QStringLiteral("int"), 1}, {QStringLiteral("double"), 2.5},
                    {QStringLiteral("float"), 0.1f}, {QStringLiteral("bool"), true},
                    {QStringLiteral("string"), QStringLiteral("s")},
                    {QStringLiteral("null"), QVariant::fromValue(nullptr)},
                    {QStringLiteral("invalid"), QVariant()},
                    {QStringLiteral("longlong"), qlonglong(1) << 55},
                    {QStringLiteral("ulonglong"), qulonglong(18446744073709551615ULL)},
                    {QStringLiteral("uint"), 4294967295u},
                    {QStringLiteral("list"), QVariantList{1, QStringLiteral("two"), 3.0}},
                    {QStringLiteral("stringList"), QStringList{QStringLiteral("a"), QStringLiteral("b")}},
                    {QStringLiteral("map"), QVariantMap{{QStringLiteral("k"), 1}}},
                    {QStringLiteral("nan"), std::numeric_limits<double>::quiet_NaN()},
                    {QStringLiteral("integralDouble"), 3.0}};
    documents << qMakePair(QStringLiteral("from-variant-map"),
                           QJsonDocument(QJsonValue::fromVariant(map).toObject()));

    documents << qMakePair(QStringLiteral("null-document"), QJsonDocument());
    documents << qMakePair(QStringLiteral("empty-object"), QJsonDocument(QJsonObject()));
    documents << qMakePair(QStringLiteral("empty-array"), QJsonDocument(QJsonArray()));
    QJsonArray emptyNested{QJsonArray{}, QJsonObject{}, QJsonArray{QJsonObject{}}};
    documents << qMakePair(QStringLiteral("empty-nested"), QJsonDocument(emptyNested));
    return documents;
}

QList<QPair<QByteArray, QJsonValue>> builtValues()
{
    const double nan = std::numeric_limits<double>::quiet_NaN();
    const double inf = std::numeric_limits<double>::infinity();
    QList<QPair<QByteArray, QJsonValue>> values{
        {"QJsonValue()", QJsonValue()},
        {"Undefined", QJsonValue(QJsonValue::Undefined)},
        {"true", QJsonValue(true)},
        {"false", QJsonValue(false)},
        {"int 0", QJsonValue(0)},
        {"int -1", QJsonValue(-1)},
        {"int max", QJsonValue(std::numeric_limits<int>::max())},
        {"int min", QJsonValue(std::numeric_limits<int>::min())},
        {"qint64 max", QJsonValue(std::numeric_limits<qint64>::max())},
        {"qint64 min", QJsonValue(std::numeric_limits<qint64>::min())},
        {"qint64 2^31", QJsonValue(qint64(1) << 31)},
        {"qint64 2^53+1", QJsonValue((qint64(1) << 53) + 1)},
        {"double 0", QJsonValue(0.0)},
        {"double -0", QJsonValue(-0.0)},
        {"double 1.5", QJsonValue(1.5)},
        {"double -1.5", QJsonValue(-1.5)},
        {"double 2.5", QJsonValue(2.5)},
        {"double 2", QJsonValue(2.0)},
        {"double 2^31", QJsonValue(2147483648.0)},
        {"double -2^31-1", QJsonValue(-2147483649.0)},
        {"double 2^53", QJsonValue(9007199254740992.0)},
        {"double 2^63", QJsonValue(9223372036854775808.0)},
        {"double -2^63", QJsonValue(-9223372036854775808.0)},
        {"double 1e19", QJsonValue(1e19)},
        {"double 1e300", QJsonValue(1e300)},
        {"double 0.1", QJsonValue(0.1)},
        {"double nan", QJsonValue(nan)},
        {"double inf", QJsonValue(inf)},
        {"double -inf", QJsonValue(-inf)},
        {"string empty", QJsonValue(QString())},
        {"string abc", QJsonValue(QStringLiteral("abc"))},
        {"string 12", QJsonValue(QStringLiteral("12"))},
        {"string 1e3", QJsonValue(QStringLiteral("1e3"))},
        {"string -0x10", QJsonValue(QStringLiteral("-0x10"))},
        {"string true", QJsonValue(QStringLiteral("true"))},
        {"string 1.5", QJsonValue(QStringLiteral("1.5"))},
        {"string  7 ", QJsonValue(QStringLiteral(" 7 "))},
        {"string 9223372036854775808", QJsonValue(QStringLiteral("9223372036854775808"))},
        {"latin1 abc", QJsonValue(QLatin1String("abc"))},
        {"array empty", QJsonValue(QJsonArray())},
        {"array [1]", QJsonValue(QJsonArray{1})},
        {"object empty", QJsonValue(QJsonObject())},
        {"object {a:1}", QJsonValue(QJsonObject{{QStringLiteral("a"), 1}})},
        {"fromVariant(QVariant())", QJsonValue::fromVariant(QVariant())},
        {"fromVariant(nullptr)", QJsonValue::fromVariant(QVariant::fromValue(nullptr))},
        {"fromVariant(int 5)", QJsonValue::fromVariant(5)},
        {"fromVariant(uint max)", QJsonValue::fromVariant(4294967295u)},
        {"fromVariant(qlonglong 2^60)", QJsonValue::fromVariant(qlonglong(1) << 60)},
        {"fromVariant(qulonglong max)", QJsonValue::fromVariant(qulonglong(18446744073709551615ULL))},
        {"fromVariant(double 2.0)", QJsonValue::fromVariant(2.0)},
        {"fromVariant(double 0.1)", QJsonValue::fromVariant(0.1)},
        {"fromVariant(float 0.1)", QJsonValue::fromVariant(0.1f)},
        {"fromVariant(double nan)", QJsonValue::fromVariant(nan)},
        {"fromVariant(bool)", QJsonValue::fromVariant(true)},
        {"fromVariant(QString)", QJsonValue::fromVariant(QStringLiteral("s"))},
        {"fromVariant(QByteArray)", QJsonValue::fromVariant(QByteArray("bytes"))},
        {"fromVariant(QStringList)", QJsonValue::fromVariant(QStringList{QStringLiteral("a")})},
        {"fromVariant(QVariantList)", QJsonValue::fromVariant(QVariantList{1, QStringLiteral("x")})},
        {"fromVariant(QVariantMap)", QJsonValue::fromVariant(QVariantMap{{QStringLiteral("k"), 1.5}})},
        {"fromVariant(QColor)", QJsonValue::fromVariant(QColor(QStringLiteral("#7aa2f7")))},
        {"fromVariant(QPointF)", QJsonValue::fromVariant(QPointF(0.25, 0.75))},
    };
    return values;
}

void runBuilt(const Context &context)
{
    Text typed;
    for (const auto &[name, document] : builtDocuments()) {
        const QString base = QDir(context.output).filePath(QStringLiteral("built/") + name);
        writeFile(base + QStringLiteral(".indented.json"), document.toJson(QJsonDocument::Indented));
        writeFile(base + QStringLiteral(".compact.json"), document.toJson(QJsonDocument::Compact));
        typed << "== " << name.toUtf8() << "\n";
        dumpTyped(typed, QString(), documentRoot(document));
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("built-typed.txt")), typed.data());

    Text values;
    for (const auto &[label, value] : builtValues())
        values.row({label, typedRow(value), wrappedJson(value)});
    writeFile(QDir(context.output).filePath(QStringLiteral("values.txt")), values.data());
}

} // namespace

void runQJson(const Context &context)
{
    Text parse, typed;
    runDocuments(context, parse, typed);
    writeFile(QDir(context.output).filePath(QStringLiteral("parse.txt")), parse.data());
    writeFile(QDir(context.output).filePath(QStringLiteral("typed.txt")), typed.data());
    runLiterals(context, QStringLiteral("qjson/number-literals.txt"), QStringLiteral("number-literals.txt"));
    runLiterals(context, QStringLiteral("qjson/string-literals.txt"), QStringLiteral("string-literals.txt"));
    runDoubles(context);
    runBuilt(context);
}

} // namespace Oracle
