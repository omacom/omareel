#pragma once

// Scenarios for the replay shims (tests/golden/shims/replay.sh): the programs the recording side
// starts (hyprctl, gpu-screen-recorder, slurp, omarchy-capture-region, pactl, ffmpeg) answer from
// a directory of canned replies, and every call is logged, so the oracle records both what the
// C++ did with the answers and what it asked. tests/golden/README.md, "Replayed programs".

#include "common.h"

#include <QJsonArray>
#include <QJsonObject>
#include <QString>
#include <QStringList>

namespace Oracle {

class Replay
{
public:
    struct Answer {
        QByteArray out;
        QByteArray err;
        int status = 0;
        bool readStdin = false;
        int hangSeconds = 0;
        bool hasCreates = false;
        QByteArray creates;
    };

    // Creates (or empties) the directory the answers go to.
    explicit Replay(const QString &directory);
    ~Replay();

    // The shim's file name for a call: the program and every argument, each after a space, with
    // % written %25 and / written %2F.
    static QString key(const QString &program, const QStringList &arguments);

    // `call` > 0 answers only the call-th call with these arguments (1-based); 0 answers the rest.
    void answer(const QString &program, const QStringList &arguments, const Answer &answer, int call = 0);
    // The answer for every call of `program` that has no answer of its own.
    void answerAny(const QString &program, const Answer &answer);
    // Answers from corpus JSON: an array of {"argv": [program, arguments...], "status": int,
    // "stdout": text | "stdoutFile": corpus path, "stderr": text, "stdin": bool, "hang": seconds,
    // "creates": text, "call": n}. Missing members are empty, 0 or false.
    void answer(const QJsonArray &answers, const QString &corpus);

    // Points OMAREEL_ORACLE_REPLAY_DIR and OMAREEL_ORACLE_REPLAY_LOG at this scenario (the log
    // starts empty) until deactivate() or destruction.
    void activate();
    void deactivate();

    // The calls logged since activate(), each [program, arguments...].
    QList<QStringList> calls() const;
    // What call `index` (0-based) read on its standard input, if its answer asked it to.
    QByteArray standardInput(int index, bool *read = nullptr) const;
    // One `call <strlist>` row per logged call, plus `  stdin <bytes>` after a call that read it.
    void dumpCalls(Text &out) const;

private:
    QString m_directory;
    QString m_log;
    bool m_active = false;
};

// A directory holding links to the named shims and to the few tools the shims run on (sh, cat,
// cksum, cut, dirname, mkdir, readlink, sed, sleep, wc), for a PATH without the other programs (a
// picker that is not installed, a program missing).
QString shimDirectory(const QString &scratch, const QString &name, const QStringList &programs);

} // namespace Oracle
