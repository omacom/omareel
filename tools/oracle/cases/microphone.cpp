// microphone: MicrophoneMeter::peakLevel (src/ui/MicrophoneMeter.cpp, compiled as it is) for every
// QAudioFormat sample format and several channel counts over crafted sample buffers, and the
// meter's rise-and-fall over a sequence of reads (MicrophoneMeter::read, copied below).
// tests/golden/README.md, microphone.

#include "common.h"

#include "ui/MicrophoneMeter.h"

#include <QAudioFormat>
#include <QDir>
#include <QJsonArray>
#include <QJsonObject>

#include <algorithm>
#include <cmath>

namespace Oracle {

namespace {

QAudioFormat::SampleFormat sampleFormat(const QString &name)
{
    if (name == QLatin1String("UInt8")) return QAudioFormat::UInt8;
    if (name == QLatin1String("Int16")) return QAudioFormat::Int16;
    if (name == QLatin1String("Int32")) return QAudioFormat::Int32;
    if (name == QLatin1String("Float")) return QAudioFormat::Float;
    return QAudioFormat::Unknown;
}

QAudioFormat audioFormat(const QString &name, int channels)
{
    QAudioFormat format;
    format.setSampleRate(48000);
    format.setChannelCount(channels);
    format.setSampleFormat(sampleFormat(name));
    return format;
}

// The meter's level (MicrophoneMeter's m_level) and its update on each read.
struct Meter {
    double m_level = 0.0;
    int emitted = 0;

    // src/ui/MicrophoneMeter.cpp:66-71 (MicrophoneMeter::read after its m_input check), with the
    // format and the bytes readAll() returned as parameters and levelChanged() counted.
    void read(const QAudioFormat &format, const QByteArray &bytes)
    {
        // Rises with each peak and falls back over a few hundred milliseconds, like a VU meter.
        double level = std::max(Omareel::MicrophoneMeter::peakLevel(format, bytes), m_level * 0.75);
        if (level < 0.01) level = 0.0;
        if (std::abs(level - m_level) < 0.005) return;
        m_level = level;
        ++emitted;
    }
};

} // namespace

void runMicrophone(const Context &context)
{
    const QJsonObject cases = readJson(QDir(context.corpus).filePath(QStringLiteral("microphone/cases.json"))).toObject();
    const QJsonObject buffers = cases.value(QStringLiteral("buffers")).toObject();
    Text out;
    for (const QString &formatName : {QStringLiteral("UInt8"), QStringLiteral("Int16"), QStringLiteral("Int32"),
                                      QStringLiteral("Float")}) {
        const QJsonObject set = buffers.value(formatName).toObject();
        for (const QJsonValue &channels : cases.value(QStringLiteral("channels")).toArray()) {
            const QAudioFormat format = audioFormat(formatName, channels.toInt());
            for (auto it = set.begin(); it != set.end(); ++it) {
                const QByteArray bytes = QByteArray::fromHex(it.value().toString().toLatin1());
                const double level = Omareel::MicrophoneMeter::peakLevel(format, bytes);
                out.row({formatName.toUtf8(), QByteArray::number(channels.toInt()), it.key().toUtf8(),
                         it.value().toString().toLatin1(), num(level), hexBits(level)});
            }
        }
    }
    // Buffers of every format read as each other format, and a format without a sample type.
    for (const QString &formatName : {QStringLiteral("UInt8"), QStringLiteral("Int16"), QStringLiteral("Int32"),
                                      QStringLiteral("Float"), QStringLiteral("Unknown")}) {
        const QAudioFormat format = audioFormat(formatName, 1);
        for (const QString &source : {QStringLiteral("Int16"), QStringLiteral("Float")}) {
            const QJsonObject set = buffers.value(source).toObject();
            for (auto it = set.begin(); it != set.end(); ++it) {
                if (formatName == source) continue;
                const QByteArray bytes = QByteArray::fromHex(it.value().toString().toLatin1());
                const double level = Omareel::MicrophoneMeter::peakLevel(format, bytes);
                out.row({formatName.toUtf8(), "1", source.toUtf8() + ':' + it.key().toUtf8(),
                         it.value().toString().toLatin1(), num(level), hexBits(level)});
            }
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("peak-level.txt")), out.data());

    Text meter;
    const QJsonObject sequences = cases.value(QStringLiteral("sequences")).toObject();
    for (auto it = sequences.begin(); it != sequences.end(); ++it) {
        const QJsonArray sequence = it.value().toArray();
        const QString formatName = sequence.at(0).toString();
        const QAudioFormat format = audioFormat(formatName, 1);
        const QJsonObject set = buffers.value(formatName).toObject();
        meter << "== " << it.key().toUtf8() << "\n";
        Meter state;
        for (const QJsonValue &name : sequence.at(1).toArray()) {
            const int before = state.emitted;
            state.read(format, QByteArray::fromHex(set.value(name.toString()).toString().toLatin1()));
            meter.row({name.toString().toUtf8(), num(state.m_level), hexBits(state.m_level),
                       boolText(state.emitted != before)});
        }
    }
    writeFile(QDir(context.output).filePath(QStringLiteral("meter.txt")), meter.data());
}

} // namespace Oracle
