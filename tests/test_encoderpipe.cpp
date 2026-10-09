#include "render/EncoderPipe.h"

#include <QCoreApplication>
#include <QElapsedTimer>
#include <QProcess>
#include <QThread>
#include <QtTest>
#include <csignal>
#include <cstdio>
#include <unistd.h>

using namespace Omareel;

namespace {
class Child : public QProcess {
public:
    ~Child() override {
        if (state() != NotRunning) {
            kill();
            waitForFinished(2000);
        }
    }
    bool launch(const QString &mode) {
        start(QCoreApplication::applicationFilePath(), {QStringLiteral("--pipe-child"), mode});
        return waitForStarted(2000);
    }
};
const auto neverCancelled = [] { return false; };
}

class EncoderPipeTest : public QObject {
    Q_OBJECT
private slots:
    void slowConsumerKeepsBufferBounded() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("slow")));
        const QByteArray frame(512 * 1024, 'x');
        qint64 maximumBuffered = 0;
        connect(&child, &QProcess::bytesWritten, this, [&] {
            maximumBuffered = std::max(maximumBuffered, child.bytesToWrite());
        });
        QElapsedTimer elapsed;
        elapsed.start();
        QString error;
        for (int i = 0; i < 8; ++i) {
            QVERIFY2(writeEncoderFrame(child, frame, neverCancelled, &error, 1000), qPrintable(error));
            QCOMPARE(child.bytesToWrite(), 0);
        }
        QVERIFY2(elapsed.elapsed() > 500, "Writes must wait for the deliberately slow consumer");
        QVERIFY(maximumBuffered <= frame.size());
        QVERIFY2(finishEncoderPipe(child, neverCancelled, &error), qPrintable(error));
        QCOMPARE(child.readAllStandardOutput().trimmed().toLongLong(), qint64(frame.size()) * 8);
    }

    void progressingFrameDoesNotTimeOut() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("slow")));
        QElapsedTimer elapsed;
        elapsed.start();
        QString error;
        QVERIFY2(writeEncoderFrame(child, QByteArray(2 * 1024 * 1024, 'x'),
                                   neverCancelled, &error, 250), qPrintable(error));
        QVERIFY(elapsed.elapsed() > 250);
        QCOMPARE(child.bytesToWrite(), 0);
        QVERIFY2(finishEncoderPipe(child, neverCancelled, &error), qPrintable(error));
    }

    void cancelsBlockedWrite() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("stalled")));
        QElapsedTimer elapsed;
        elapsed.start();
        QString error;
        QVERIFY(!writeEncoderFrame(child, QByteArray(1024 * 1024, 'x'),
                                   [&] { return elapsed.elapsed() >= 100; }, &error));
        QVERIFY2(error.contains(QStringLiteral("cancelled")), qPrintable(error));
        QVERIFY(elapsed.elapsed() < 2000);
    }

    void stalledWriteHasDiagnostic() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("stalled")));
        QString error;
        QVERIFY(!writeEncoderFrame(child, QByteArray(1024 * 1024, 'x'), neverCancelled, &error, 100));
        QVERIFY2(error.contains(QStringLiteral("timed out")), qPrintable(error));
    }

    void earlyExitHasDiagnostic() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("failed")));
        QString error;
        QVERIFY(!writeEncoderFrame(child, QByteArray(1024 * 1024, 'x'), neverCancelled, &error));
        QVERIFY(!error.isEmpty());
    }

    void finalizationFailure_data() {
        QTest::addColumn<QString>("mode");
        QTest::newRow("silent-nonzero") << QStringLiteral("failed");
        QTest::newRow("killed") << QStringLiteral("killed");
    }
    void finalizationFailure() {
        QFETCH(QString, mode);
        Child child;
        QVERIFY(child.launch(mode));
        QString error;
        QVERIFY(!finishEncoderPipe(child, neverCancelled, &error));
        QVERIFY2(error.contains(QStringLiteral("Encoder failed")), qPrintable(error));
    }

    void finalizationTimeoutHasDiagnostic() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("stalled")));
        QString error;
        QVERIFY(!finishEncoderPipe(child, neverCancelled, &error, 100));
        QVERIFY2(error.contains(QStringLiteral("timed out")), qPrintable(error));
    }

    void cancelsFinalization() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("stalled")));
        QElapsedTimer elapsed;
        elapsed.start();
        QString error;
        QVERIFY(!finishEncoderPipe(child, [&] { return elapsed.elapsed() >= 100; }, &error));
        QVERIFY2(error.contains(QStringLiteral("cancelled")), qPrintable(error));
        QVERIFY(elapsed.elapsed() < 2000);
    }

    void waitsForFinalization() {
        Child child;
        QVERIFY(child.launch(QStringLiteral("slow")));
        QString error;
        QVERIFY(writeEncoderFrame(child, QByteArray(16384, 'x'), neverCancelled, &error));
        QElapsedTimer elapsed;
        elapsed.start();
        QVERIFY2(finishEncoderPipe(child, neverCancelled, &error), qPrintable(error));
        QVERIFY(elapsed.elapsed() >= 100);
        QCOMPARE(child.exitStatus(), QProcess::NormalExit);
        QCOMPARE(child.exitCode(), 0);
    }
};

int main(int argc, char **argv)
{
    QCoreApplication app(argc, argv);
    const QStringList args = app.arguments();
    if (args.size() == 3 && args[1] == QLatin1String("--pipe-child")) {
        const QString mode = args[2];
        if (mode == QLatin1String("failed")) return 7;
        if (mode == QLatin1String("killed")) { std::raise(SIGKILL); return 1; }
        if (mode == QLatin1String("stalled")) { QThread::sleep(60); return 0; }
        char buffer[4096];
        qint64 total = 0;
        ssize_t amount;
        while ((amount = ::read(STDIN_FILENO, buffer, sizeof(buffer))) > 0) {
            for (ssize_t i = 0; i < amount; ++i) if (buffer[i] != 'x') return 8;
            total += amount;
            QThread::msleep(1);
        }
        QThread::msleep(150);
        std::printf("%lld\n", static_cast<long long>(total));
        return amount == 0 ? 0 : 9;
    }
    EncoderPipeTest test;
    return QTest::qExec(&test, argc, argv);
}

#include "test_encoderpipe.moc"
