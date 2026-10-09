#include "EncoderPipe.h"

#include <QElapsedTimer>
#include <QProcess>
#include <algorithm>

namespace Omareel {
namespace {
constexpr int pollMs = 50;

bool fail(QProcess &process, QString *error, const QString &reason)
{
    const QString details = QString::fromUtf8(process.readAllStandardError()).trimmed();
    if (error) *error = details.isEmpty() ? reason : reason + QStringLiteral(": ") + details;
    return false;
}
}

bool writeEncoderFrame(QProcess &process, const QByteArray &frame,
                       const std::function<bool()> &cancelled, QString *error,
                       int stallTimeoutMs)
{
    QElapsedTimer stalled;
    stalled.start();
    qint64 written = 0;
    while (written < frame.size() || process.bytesToWrite() > 0) {
        if (cancelled()) return fail(process, error, QStringLiteral("Export cancelled"));
        if (process.state() == QProcess::NotRunning)
            return fail(process, error, QStringLiteral("Encoder exited before receiving all frames"));
        if (process.bytesToWrite() == 0) {
            const qint64 amount = process.write(frame.constData() + written, frame.size() - written);
            if (amount <= 0)
                return fail(process, error, QStringLiteral("Could not write a frame to the encoder"));
            written += amount;
        }
        // A successful wait drains only part of the buffer, often just one pipe
        // page. Keep draining before accepting the next frame. The deadline is
        // for lack of progress, not total encoding time on a slow machine.
        if (process.waitForBytesWritten(std::min(pollMs, stallTimeoutMs))) stalled.restart();
        else if (process.error() == QProcess::WriteError)
            return fail(process, error, QStringLiteral("Could not write a frame to the encoder"));
        if (stalled.elapsed() >= stallTimeoutMs && process.bytesToWrite() > 0)
            return fail(process, error, QStringLiteral("Encoder stopped accepting frames (write timed out)"));
    }
    return true;
}

bool finishEncoderPipe(QProcess &process, const std::function<bool()> &cancelled,
                       QString *error, int timeoutMs)
{
    process.closeWriteChannel();
    QElapsedTimer timer;
    timer.start();
    while (process.state() != QProcess::NotRunning) {
        if (cancelled()) return fail(process, error, QStringLiteral("Export cancelled"));
        const qint64 remaining = timeoutMs - timer.elapsed();
        if (remaining <= 0)
            return fail(process, error, QStringLiteral("Encoder timed out while finishing the video"));
        process.waitForFinished(int(std::min<qint64>(pollMs, remaining)));
    }
    if (cancelled()) return fail(process, error, QStringLiteral("Export cancelled"));
    if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0)
        return fail(process, error, QStringLiteral("Encoder failed (exit code %1)").arg(process.exitCode()));
    return true;
}

} // namespace Omareel
