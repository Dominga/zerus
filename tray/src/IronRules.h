#pragma once
#include "SettingsJson.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QSettings>
#include <QString>
#include <QStringList>

// WhiteWh fork: a project's iron rules. They live in this Zerus's settings, per
// project, and Zerus sends them at the start of every message it delivers to an
// agent of that project, between markers that Activity removes again before it
// shows or matches the message.
namespace IronRules {

inline constexpr int MaxRules = 20;
inline constexpr int MaxLength = 300;

inline QString opening() { return QStringLiteral("[Iron rules: follow every one; never skip or override them]"); }
inline QString closing() { return QStringLiteral("[/Iron rules]"); }

inline QStringList rules(const QString &project)
{
    if (project.isEmpty()) return {};
    const auto all = SettingsJson::object(QStringLiteral("workspace/ironRules"));
    QStringList result;
    for (const auto &value : all.value(project).toArray()) {
        const auto rule = value.toString().simplified().left(MaxLength);
        if (!rule.isEmpty() && result.size() < MaxRules) result << rule;
    }
    return result;
}

inline void setRules(const QString &project, const QStringList &rules)
{
    if (project.isEmpty()) return;
    QSettings settings;
    auto all = SettingsJson::object(QStringLiteral("workspace/ironRules"));
    QJsonArray list;
    for (const auto &rule : rules) {
        const auto text = rule.simplified().left(MaxLength);
        if (!text.isEmpty() && list.size() < MaxRules) list.append(text);
    }
    if (list.isEmpty()) all.remove(project); else all[project] = list;
    SettingsJson::store(settings, QStringLiteral("workspace/ironRules"), QJsonDocument(all));
}

// The message an agent receives: the rules first, then the user's text.
inline QString wrap(const QStringList &rules, const QString &text)
{
    if (rules.isEmpty()) return text;
    QString block = opening() + '\n';
    for (int at = 0; at < rules.size(); ++at) block += QString::number(at + 1) + QStringLiteral(". ") + rules[at] + '\n';
    return block + closing() + QStringLiteral("\n\n") + text;
}

// The user's text without a leading rules block; `found` says whether one was there.
inline QString strip(const QString &text, bool *found = nullptr)
{
    static const QRegularExpression block(QStringLiteral("\\A\\s*\\[Iron rules[^\\]\\n]*\\]\\n(?:[^\\n]*\\n){0,%1}?\\[/Iron rules\\]\\n*").arg(MaxRules + 1));
    const auto match = block.match(text);
    if (found) *found = match.hasMatch();
    return match.hasMatch() ? text.mid(match.capturedEnd()) : text;
}
}
