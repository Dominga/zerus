// Task 127 (docs/lore.md 4.1): a team's memory modes as New team offers them,
// shared by the New team tab and dialog. With the project's RustCodeLore on,
// all five and `project` preselected; with it off, team, plain and off, and
// `team` preselected. Task 126's "lore" reads as the default.
#pragma once

#include <QCoreApplication>
#include <QJsonObject>
#include <QList>
#include <QPair>
#include <QString>
#include <QStringList>

namespace TeamMemoryModes {

inline QStringList modes(bool projectOn)
{
    return projectOn ? QStringList{"project", "team", "read-only", "plain", "off"} : QStringList{"team", "plain", "off"};
}

inline QString preselected(bool projectOn) { return projectOn ? QStringLiteral("project") : QStringLiteral("team"); }

// The mode a draft's settings name, when the project offers it; else the default.
inline QString of(const QJsonObject &settings, bool projectOn)
{
    const QString mode = settings.value("memory").toObject().value("mode").toString();
    return modes(projectOn).contains(mode) ? mode : preselected(projectOn);
}

inline QString label(const QString &mode)
{
    if (mode == "project") return QCoreApplication::translate("TeamMemoryModes", "Project memory");
    if (mode == "team") return QCoreApplication::translate("TeamMemoryModes", "Team memory");
    if (mode == "read-only") return QCoreApplication::translate("TeamMemoryModes", "Read only");
    if (mode == "plain") return QCoreApplication::translate("TeamMemoryModes", "Plain memory");
    return QCoreApplication::translate("TeamMemoryModes", "Off");
}

inline QString hint(const QString &mode)
{
    if (mode == "project")
        return QCoreApplication::translate("TeamMemoryModes", "A shared workspace in the project's RustCodeLore store; the team reads the project's records too");
    if (mode == "team")
        return QCoreApplication::translate("TeamMemoryModes", "RustCodeLore for this team only: its workspace (in the project's store when it has one, else under Zerus's data)");
    if (mode == "read-only") return QCoreApplication::translate("TeamMemoryModes", "Reads the project's memory and writes nothing");
    if (mode == "plain") return QCoreApplication::translate("TeamMemoryModes", "The team's own record and a generated digest; no RustCodeLore store");
    return QCoreApplication::translate("TeamMemoryModes", "The team's own record only; no digest and no store");
}

// The five modes as (value, label) pairs, in the order the forms list them.
inline QList<QPair<QString, QString>> all()
{
    QList<QPair<QString, QString>> out;
    for (const auto &mode : modes(true)) out.append({mode, label(mode)});
    return out;
}

} // namespace TeamMemoryModes
