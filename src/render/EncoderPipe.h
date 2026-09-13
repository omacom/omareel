#pragma once

#include <QByteArray>
#include <QString>
#include <functional>

class QProcess;

namespace Omareel {

// Blocking worker-thread operations. Never queue another frame until Qt's
// userspace write buffer is empty; bytesWritten only promises partial progress.
bool writeEncoderFrame(QProcess &process, const QByteArray &frame,
                       const std::function<bool()> &cancelled, QString *error,
                       int stallTimeoutMs = 30000);
bool finishEncoderPipe(QProcess &process, const std::function<bool()> &cancelled,
                       QString *error, int timeoutMs = 120000);

} // namespace Omareel
