#pragma once
#include "SettingsJson.h"
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QRegularExpression>
#include <QSettings>
#include <QString>
#include <QStringList>
#include <QUuid>
#include <QPointer>
#include <QTimer>
#include <functional>
#include <memory>

// Rust owns cumulative project/team/member/session rules. The old QSettings
// map stays as an import backup and as compatibility for an older CLI.
namespace IronRules {

using Reply = std::function<void(const QJsonObject &, const QString &)>;
using Transport = std::function<void(const QString &, const QString &, const QJsonObject &, Reply)>;

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

// Keep the legacy value as a backup. Only a confirmed Rust receipt marks an
// import complete; failed imports remain retryable, including on remote hosts.
inline void loadProject(const Transport &send, const QString &host, const QString &project, Reply done)
{
    if (project.isEmpty()) { done(QJsonObject{{"rules", QJsonArray{}}, {"revision", 0}}, {}); return; }
    send(host, "get", {{"scope", "project"}, {"project", project}},
        [=](const QJsonObject &value, const QString &error) {
            if (!error.isEmpty() || value.value("unsupported").toBool() || value.value("migrated").toBool() || rules(project).isEmpty()) { done(value, error); return; }
            send(host, "migrate", {{"scope", "project"}, {"project", project},
                {"expected_revision", value.value("revision")}, {"rules", QJsonArray::fromStringList(rules(project))},
                {"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)}}, done);
        });
}

inline void migrateLegacy(const Transport &send, QObject *owner, std::function<void(const QString &)> failed)
{
    struct Import : std::enable_shared_from_this<Import> {
        Transport send;
        QPointer<QObject> owner;
        QStringList projects;
        qsizetype at = 0;
        std::function<void(const QString &)> failed;
        void next()
        {
            if (!owner || at >= projects.size()) return;
            const auto self = shared_from_this();
            loadProject(send, {}, projects[at++], [self](const QJsonObject &value, const QString &error) {
                if (!error.isEmpty()) self->failed(error);
                if (self->owner && !value.value("unsupported").toBool())
                    QTimer::singleShot(0, self->owner, [self] { self->next(); });
            });
        }
    };
    auto state = std::make_shared<Import>(); state->send = send; state->owner = owner;
    state->failed = std::move(failed);
    state->projects = SettingsJson::object(QStringLiteral("workspace/ironRules")).keys();
    state->next();
}

inline QStringList list(const QJsonValue &value)
{
    QStringList result;
    for (const auto &rule : value.toArray()) result << rule.toString();
    return result;
}

inline QString wrap(const QStringList &rules, const QString &text);

inline QString wrapEffective(const QString &rules, const QString &text)
{
    return rules.isEmpty() ? text : opening() + '\n' + rules + closing() + "\n\n" + text;
}

// Only a matching, proven startup may replace per-send delivery. Read fresh
// rules for each fallback send; the GUI never caches a session's policy.
inline void prepare(const Transport &send, const QString &host, const QString &project,
                    const QString &name, const QString &run, const QString &conversation,
                    const QString &text, std::function<void(const QString &, const QString &)> done)
{
    loadProject(send, host, project, [=](const QJsonObject &p, const QString &error) {
        if (p.value("unsupported").toBool()) { done(wrap(rules(project), text), {}); return; }
        send(host, "get", {{"scope", "session"}, {"name", name}, {"expected_run_id", run}, {"expected_conversation_id", conversation}},
            [=](const QJsonObject &s, const QString &failure) {
                if (!failure.isEmpty()) { done({}, failure); return; }
                if (error.isEmpty() && host.isEmpty() && s.value("start_delivery_supported").toBool() && s.value("start_project").toString() == project) { done(text, {}); return; }
                QString effective = s.value("effective_rules").toString();
                if (!error.isEmpty() || !host.isEmpty() || s.value("project").toString() != project) {
                    QString prefix;
                    const auto inherited = error.isEmpty() ? list(p.value("rules")) : rules(project);
                    if (!inherited.isEmpty()) {
                        prefix = "Project rules (also mandatory):\n";
                        for (const auto &rule : inherited) prefix += rule + '\n';
                    }
                    effective = prefix + effective;
                }
                done(wrapEffective(effective, text), {});
            });
    });
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
    static const QRegularExpression block(QStringLiteral("\\A\\s*\\[Iron rules[^\\]\\n]*\\]\\n(?:[^\\n]*\\n){0,%1}?\\[/Iron rules\\]\\n*").arg(MaxRules * 5 + 10));
    const auto match = block.match(text);
    if (found) *found = match.hasMatch();
    return match.hasMatch() ? text.mid(match.capturedEnd()) : text;
}
}
