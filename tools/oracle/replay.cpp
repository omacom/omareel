#include "replay.h"

#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QStandardPaths>

namespace Oracle {

Replay::Replay(const QString &directory): m_directory(directory), m_log(directory + QStringLiteral(".log"))
{
    QDir(m_directory).removeRecursively();
    QDir().mkpath(m_directory);
}

Replay::~Replay()
{
    if (m_active) deactivate();
}

QString Replay::key(const QString &program, const QStringList &arguments)
{
    QString text = program;
    for (const QString &argument : arguments) text += QLatin1Char(' ') + argument;
    text.replace(QLatin1Char('%'), QStringLiteral("%25"));
    text.replace(QLatin1Char('/'), QStringLiteral("%2F"));
    return text;
}

void Replay::answer(const QString &program, const QStringList &arguments, const Answer &answer, int call)
{
    QString base = QDir(m_directory).filePath(key(program, arguments));
    if (call > 0) base += QLatin1Char('.') + QString::number(call);
    if (QFileInfo::exists(base + QStringLiteral(".status")))
        fail(QStringLiteral("two answers for %1").arg(key(program, arguments)));
    writeFile(base + QStringLiteral(".status"), QByteArray::number(answer.status) + '\n');
    if (!answer.out.isEmpty()) writeFile(base + QStringLiteral(".stdout"), answer.out);
    if (!answer.err.isEmpty()) writeFile(base + QStringLiteral(".stderr"), answer.err);
    if (answer.readStdin) writeFile(base + QStringLiteral(".stdin"), QByteArray());
    if (answer.hangSeconds > 0)
        writeFile(base + QStringLiteral(".hang"), QByteArray::number(answer.hangSeconds) + '\n');
    if (answer.hasCreates) writeFile(base + QStringLiteral(".creates"), answer.creates);
}

void Replay::answerAny(const QString &program, const Answer &answer)
{
    const QString base = QDir(m_directory).filePath(program + QStringLiteral(".any"));
    writeFile(base + QStringLiteral(".status"), QByteArray::number(answer.status) + '\n');
    if (!answer.out.isEmpty()) writeFile(base + QStringLiteral(".stdout"), answer.out);
    if (!answer.err.isEmpty()) writeFile(base + QStringLiteral(".stderr"), answer.err);
    if (answer.readStdin) writeFile(base + QStringLiteral(".stdin"), QByteArray());
    if (answer.hasCreates) writeFile(base + QStringLiteral(".creates"), answer.creates);
}

void Replay::answer(const QJsonArray &answers, const QString &corpus)
{
    for (const QJsonValue &value : answers) {
        const QJsonObject object = value.toObject();
        QStringList argv;
        for (const QJsonValue &argument : object.value(QStringLiteral("argv")).toArray())
            argv << argument.toString();
        if (argv.isEmpty()) fail(QStringLiteral("a replay answer without argv"));
        Answer reply;
        reply.status = object.value(QStringLiteral("status")).toInt();
        if (object.contains(QStringLiteral("stdoutFile"))) {
            bool ok = false;
            reply.out = readFile(QDir(corpus).filePath(object.value(QStringLiteral("stdoutFile")).toString()), &ok);
            if (!ok) fail(QStringLiteral("missing replay file %1").arg(object.value(QStringLiteral("stdoutFile")).toString()));
        } else {
            reply.out = object.value(QStringLiteral("stdout")).toString().toUtf8();
        }
        reply.err = object.value(QStringLiteral("stderr")).toString().toUtf8();
        reply.readStdin = object.value(QStringLiteral("stdin")).toBool();
        reply.hangSeconds = object.value(QStringLiteral("hang")).toInt();
        reply.hasCreates = object.contains(QStringLiteral("creates"));
        reply.creates = object.value(QStringLiteral("creates")).toString().toUtf8();
        const QString program = argv.takeFirst();
        answer(program, argv, reply, object.value(QStringLiteral("call")).toInt());
    }
}

void Replay::activate()
{
    QFile::remove(m_log);
    for (const QString &name : QDir(QFileInfo(m_log).absolutePath()).entryList(
             {QFileInfo(m_log).fileName() + QStringLiteral(".*.stdin")}, QDir::Files))
        QFile::remove(QDir(QFileInfo(m_log).absolutePath()).filePath(name));
    qputenv("OMAREEL_ORACLE_REPLAY_DIR", QFile::encodeName(m_directory));
    qputenv("OMAREEL_ORACLE_REPLAY_LOG", QFile::encodeName(m_log));
    m_active = true;
}

void Replay::deactivate()
{
    qunsetenv("OMAREEL_ORACLE_REPLAY_DIR");
    qunsetenv("OMAREEL_ORACLE_REPLAY_LOG");
    m_active = false;
}

QList<QStringList> Replay::calls() const
{
    QList<QStringList> result;
    const QByteArray data = readFile(m_log);
    for (const QByteArray &line : data.split('\n')) {
        if (line.isEmpty()) continue;
        result << QString::fromLocal8Bit(line).split(QLatin1Char('\t'));
    }
    return result;
}

QByteArray Replay::standardInput(int index, bool *read) const
{
    return readFile(m_log + QLatin1Char('.') + QString::number(index) + QStringLiteral(".stdin"), read);
}

void Replay::dumpCalls(Text &out) const
{
    const QList<QStringList> list = calls();
    for (int index = 0; index < list.size(); ++index) {
        QStringList sanitized;
        for (const QString &argument : list.at(index)) sanitized << sanitize(argument);
        out.row({"call", quoteList(sanitized)});
        bool read = false;
        const QByteArray input = standardInput(index, &read);
        if (read) out.row({"  stdin", quoteBytes(input)});
    }
}

QString shimDirectory(const QString &scratch, const QString &name, const QStringList &programs)
{
    // Found once, while PATH still starts with tests/golden/shims.
    static const QString shims = QFileInfo(QStandardPaths::findExecutable(QStringLiteral("hyprctl"))).absolutePath();
    if (!QFileInfo::exists(QDir(shims).filePath(QStringLiteral("replay.sh"))))
        fail(QStringLiteral("the replay shims are not first on PATH"));
    // The shims themselves run on the tools below, linked from where PATH has them.
    static const QStringList tools = [] {
        QStringList found;
        for (const char *tool : {"sh", "cat", "cksum", "cut", "dirname", "mkdir", "readlink", "sed", "sleep", "wc"}) {
            const QString path = QStandardPaths::findExecutable(QString::fromLatin1(tool));
            if (path.isEmpty()) fail(QStringLiteral("%1 is not on PATH").arg(QLatin1String(tool)));
            found << path;
        }
        return found;
    }();
    const QString directory = QDir(scratch).filePath(QStringLiteral("bin-") + name);
    QDir(directory).removeRecursively();
    QDir().mkpath(directory);
    for (const QString &program : programs)
        if (!QFile::link(QDir(shims).filePath(program), QDir(directory).filePath(program)))
            fail(QStringLiteral("could not link the %1 shim").arg(program));
    for (const QString &tool : tools)
        if (!QFile::link(tool, QDir(directory).filePath(QFileInfo(tool).fileName())))
            fail(QStringLiteral("could not link %1").arg(tool));
    return directory;
}

} // namespace Oracle
