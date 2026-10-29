#pragma once
#include "SettingsJson.h"
#include <QCryptographicHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QSettings>

// Device-local explicit presets, never catalog/project identity migration.
namespace WslPathMapping {
inline QString key(const QString &host,const QString &machine,const QJsonObject &profile) {
    const auto identity=QJsonDocument(QJsonObject{{"host",host},{"machine",machine},{"profile",profile}}).toJson(QJsonDocument::Compact);
    return "pathMappings/"+QString::fromLatin1(QCryptographicHash::hash(identity,QCryptographicHash::Sha256).toHex());
}
inline QJsonObject load(const QString &key) {
    return SettingsJson::object(key, 16384);
}
}
