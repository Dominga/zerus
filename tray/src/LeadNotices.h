#pragma once
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>
#include <QStringList>

#include "Settings.h"

// WhiteWh fork, task 244 (owner, 2026-11-07): how a team lead's session shows
// the team's notices (everything Zerus pastes into the lead) and the lead's
// replies to them: "full" as today, "collapsed" (the default) one folded line
// each and never a notification, "silent" not at all. The default is Settings
// > Teams (`teams/leadNotices`); a lead session's own choice is kept in
// `teams/leadNoticeSessions`, keyed "<host>\n<run:id | name:name>" as the
// Tauri client keys its rows, so both clients share it.
namespace LeadNotices {
inline const QStringList &modes() { static const QStringList list{"full", "collapsed", "silent"}; return list; }

inline QString sessionKey(const QString &host, const QString &runId, const QString &name)
{
    return host + QLatin1Char('\n') + (runId.isEmpty() ? QStringLiteral("name:") + name : QStringLiteral("run:") + runId);
}

inline QString defaultMode()
{
    const auto value = Settings::value(QStringLiteral("teams/leadNotices"), QStringLiteral("collapsed")).toString();
    return modes().contains(value) ? value : QStringLiteral("collapsed");
}

inline QJsonObject sessions()
{
    return QJsonDocument::fromJson(Settings::value(QStringLiteral("teams/leadNoticeSessions"), QStringLiteral("{}")).toString().toUtf8()).object();
}

inline QString mode(const QString &key)
{
    const auto own = sessions().value(key).toString();
    return modes().contains(own) ? own : defaultMode();
}

inline void setMode(const QString &key, const QString &mode)
{
    if (!modes().contains(mode)) return;
    auto map = sessions();
    map.remove(key);
    map.insert(key, mode);
    while (map.size() > 256) map.remove(map.begin().key());
    Settings::set(QStringLiteral("teams/leadNoticeSessions"), QString::fromUtf8(QJsonDocument(map).toJson(QJsonDocument::Compact)));
}
} // namespace LeadNotices
