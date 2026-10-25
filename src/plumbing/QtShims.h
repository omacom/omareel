#pragma once

#include <QColor>
#include <QString>
#include <QVariant>

namespace Omareel::Shims {

// QML sees this as null; a default-constructed QVariant reads as undefined.
QVariant nullVariant();
// QJsonValue::fromVariant's JSON for a value from QML, wrapped in an array so any value is
// a document.
QString variantToJson(const QVariant &value);
// A colour with exactly these 16-bit channels.
QColor fromRgba64(quint16 red, quint16 green, quint16 blue, quint16 alpha);
// An invalid colour that still carries an alpha, as QColor::setAlphaF leaves one; Qt Quick
// draws it as black at that alpha.
QColor invalidWithAlpha(quint16 alpha);
// qWarning().noquote() << message: Qt's message handler, so the journal gets it when there is no
// terminal, as it got the recorder daemon's warnings before they moved to Rust.
void warning(const QString &message);

} // namespace Omareel::Shims
