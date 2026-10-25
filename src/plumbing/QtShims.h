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

} // namespace Omareel::Shims
