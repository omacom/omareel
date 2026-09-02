#pragma once

#include <QJsonObject>
#include <QString>

namespace OmaRecord {

class Project
{
public:
    static Project defaults(const QString &name = QStringLiteral("Untitled Recording"),
                            double duration = 0.0);
    static Project load(const QString &path, QString *error = nullptr);

    bool save(const QString &path, QString *error = nullptr) const;
    QJsonObject json() const { return m_json; }
    void setJson(const QJsonObject &json) { m_json = json; }

private:
    explicit Project(QJsonObject json = {}): m_json(std::move(json)) {}
    QJsonObject m_json;
};

} // namespace OmaRecord
