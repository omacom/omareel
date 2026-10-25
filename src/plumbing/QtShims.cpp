#include "QtShims.h"

#include <QDebug>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonValue>
#include <QRgba64>

namespace Omareel::Shims {

QVariant nullVariant()
{
    return QVariant::fromValue(nullptr);
}

QString variantToJson(const QVariant &value)
{
    return QString::fromUtf8(
        QJsonDocument(QJsonArray{QJsonValue::fromVariant(value)}).toJson(QJsonDocument::Compact));
}

QColor fromRgba64(quint16 red, quint16 green, quint16 blue, quint16 alpha)
{
    return QColor(QRgba64::fromRgba64(red, green, blue, alpha));
}

QColor invalidWithAlpha(quint16 alpha)
{
    QColor color;
    color.setAlphaF(alpha / 65535.0f);
    return color;
}

void warning(const QString &message)
{
    qWarning().noquote() << message;
}

} // namespace Omareel::Shims
