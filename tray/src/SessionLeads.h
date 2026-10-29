#pragma once
#include "SettingsJson.h"
#include <QJsonDocument>
#include <QJsonObject>
#include <QSettings>
#include <QString>

// WhiteWh fork: subordinate sessions. A session may report to a lead session
// of the same project; the link lives in this Zerus's settings, by session
// identity, one level deep (a lead does not report to anyone).
namespace SessionLeads {

inline QJsonObject all()
{
    return SettingsJson::object(QStringLiteral("workspace/sessionLeads"));
}

inline QString lead(const QString &identity)
{
    return identity.isEmpty() ? QString() : all().value(identity).toString();
}

// Links `identity` to `leadIdentity`, or unlinks it with an empty lead. A lead
// that reports to someone, or a session that already leads others, is refused.
inline bool setLead(const QString &identity, const QString &leadIdentity)
{
    if (identity.isEmpty() || identity == leadIdentity) return false;
    auto links = all();
    if (!leadIdentity.isEmpty()) {
        if (links.contains(leadIdentity)) return false;
        for (auto it = links.begin(); it != links.end(); ++it) if (it.value().toString() == identity) return false;
        links[identity] = leadIdentity;
    } else links.remove(identity);
    SettingsJson::store(QStringLiteral("workspace/sessionLeads"), QJsonDocument(links));
    return true;
}
}
