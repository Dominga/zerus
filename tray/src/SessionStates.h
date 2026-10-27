#pragma once
#include "HgsClient.h"
#include <QCoreApplication>
#include <QVariantMap>
#include <algorithm>

// WhiteWh fork, U30 (#508): the seven session states on the desktop (design
// Status/SESSION-STATES.md). The host decides the state (#506: session_state,
// state_reason, last_active, cache_ttl, build); an older host sends none, and
// its card keeps today's presentation. `facts` are what a card
// stores (SessionRoles::SessionState); `present` turns them into the badge,
// line 3, tooltips and the cooling heat at the moment of painting, so Cooling
// counts down and turns Cold by itself.
namespace SessionStates {

inline QString tr(const char *text) { return QCoreApplication::translate("SessionStates", text); }

// "41m", "2h 05m": the minutes the design shows (fmtMin).
inline QString minutes(double seconds)
{
    const qint64 total = qMax<qint64>(0, qint64(seconds / 60));
    if (total < 60) return tr("%1m").arg(total);
    return tr("%1h %2m").arg(total / 60).arg(total % 60, 2, 10, QLatin1Char('0'));
}

// The duration in a reason text: "6m", "1h 05m".
inline QString elapsed(double seconds) { return minutes(seconds); }

// What a card needs to show its state. `keepWarmSeconds` is the inspector's
// Keep cache warm (its hours) when it is on for this session.
inline QVariantMap facts(const SessionInfo &s, bool online, double keepWarmSeconds = 0)
{
    QVariantMap f;
    // An older host sends no state: its card keeps today's presentation.
    QString key = s.sessionState;
    if (!online) key = QStringLiteral("offline");
    if (key == "cooling" || key == "cold") key = QStringLiteral("idle");
    f.insert("key", key);
    const QString reasonKind = s.stateReason.value("kind").toString();
    f.insert("reasonKind", reasonKind);
    f.insert("reason", s.stateReason.value("text").toString());
    // The user state names what waits: an approval, an input, a reply, an
    // error, or the lead's "Waiting for you".
    QString user = reasonKind;
    if (s.phase == "approval") user = QStringLiteral("approval");
    else if (s.phase == "input") user = QStringLiteral("input");
    else if (s.phase == "error") user = QStringLiteral("error");
    else if (reasonKind.startsWith(QLatin1String("team"))) user = QStringLiteral("you");
    else if (user.isEmpty() && s.visibleUnreadReply(online)) user = QStringLiteral("reply");
    f.insert("user", user);
    if (!s.build.isEmpty()) {
        f.insert("buildLabel", s.build.value("label").toString());
        f.insert("buildCommand", s.build.value("command").toString());
        f.insert("buildSince", s.build.value("since").toDouble());
    }
    // Idle time counts from the last native event (keepalive turns included).
    const double last = s.lastActive > 0 ? s.lastActive : qMax(s.lastEventAt, s.replyAt);
    f.insert("lastActive", last);
    const int ttl = s.cacheTtl.value("seconds").toInt();
    f.insert("ttl", keepWarmSeconds > 0 ? keepWarmSeconds : ttl > 0 ? double(ttl) : 300.0);
    f.insert("keptWarm", keepWarmSeconds > 0 || s.cacheTtl.value("kept_warm").toBool());
    f.insert("exited", s.processState == "exited");
    f.insert("paused", s.state == "paused");
    return f;
}

struct Shown {
    QString key;          // working, user, deps, build, cooling, cold, stopped, offline
    QString badge;        // the card's and the header pill's text (working's own timer aside)
    QString line;         // the card's line 3
    QString icon;         // hourglass, build, thermo, snow, stop, or empty
    QString tooltip;      // the badge's (§5)
    QString reason;       // the header pill's: the line's reason
    double heat = -1;     // Cooling's remaining warmth, 0.04..1; -1 otherwise
    bool coldActions = false;
};

inline QString tooltipOf(const QString &key)
{
    if (key == "working") return tr("Working — the model is writing code or running tools");
    if (key == "user") return tr("Waiting for you — an answer, an approval or a reply to read");
    if (key == "deps") return tr("Waiting for dependencies — subagents, a long operation, other members or the lead");
    if (key == "build") return tr("Building — a build or long test started through Zerus runs; the reply waits for it");
    if (key == "cooling") return tr("Cooling — done and waiting for a new task; the prompt cache is still warm");
    if (key == "cold") return tr("Cold — done and idle; the prompt cache expired, a new session is cheaper");
    if (key == "stopped") return tr("Stopped by you");
    if (key == "offline") return tr("The machine does not answer");
    return {};
}

inline Shown present(const QVariantMap &f, double now)
{
    Shown shown;
    QString key = f.value("key").toString();
    if (key.isEmpty()) return shown;
    const double ttl = qMax(1.0, f.value("ttl").toDouble());
    const double idle = qMax(0.0, now - f.value("lastActive").toDouble());
    if (key == "idle") key = f.value("lastActive").toDouble() > 0 && idle < ttl ? QStringLiteral("cooling") : QStringLiteral("cold");
    shown.key = key;
    shown.tooltip = tooltipOf(key);
    const QString reason = f.value("reason").toString();
    if (key == "user") {
        const QString user = f.value("user").toString();
        shown.badge = user == "approval" ? tr("Needs approval") : user == "input" ? tr("Needs input") : user == "you" ? tr("Needs you")
                    : user == "reply" ? tr("New reply") : user == "error" ? tr("Error") : tr("Needs you");
        shown.line = reason.isEmpty() && user == "reply" ? tr("Response ready to read") : reason;
    } else if (key == "deps") {
        shown.badge = tr("Waiting"); shown.icon = QStringLiteral("hourglass");
        // §2: the reason is always explicit.
        shown.line = reason.isEmpty() ? tr("Waiting for background work") : reason;
    } else if (key == "build") {
        shown.badge = tr("Building"); shown.icon = QStringLiteral("build");
        const QString label = f.value("buildLabel").toString().isEmpty() ? f.value("buildCommand").toString() : f.value("buildLabel").toString();
        const double since = f.value("buildSince").toDouble();
        shown.line = !reason.isEmpty() ? reason
                   : since > 0 ? tr("Running %1 · %2 so far").arg(label, elapsed(now - since)) : tr("Running %1").arg(label);
    } else if (key == "cooling") {
        const double left = ttl - idle;
        shown.badge = tr("Cooling %1").arg(minutes(left)); shown.icon = QStringLiteral("thermo");
        shown.heat = std::clamp(left / ttl, 0.04, 1.0);
        shown.line = tr("Done · waiting for a new task · cache warm for %1").arg(minutes(left));
        if (f.value("keptWarm").toBool()) shown.line += tr(" (kept warm)");
    } else if (key == "cold") {
        shown.badge = tr("Cold"); shown.icon = QStringLiteral("snow");
        shown.line = f.value("lastActive").toDouble() > 0 ? tr("Cache cold %1").arg(minutes(idle - ttl)) : tr("Cache cold");
        shown.coldActions = true;
    } else if (key == "stopped") {
        shown.badge = f.value("exited").toBool() ? tr("Agent exited") : tr("Stopped"); shown.icon = QStringLiteral("stop");
        shown.line = f.value("exited").toBool() ? tr("The agent exited · resume to continue") : tr("Stopped by you · resume to continue");
    } else if (key == "working") {
        shown.badge = tr("Working");
    } else if (key == "offline") {
        shown.badge = tr("Offline");
    }
    shown.reason = shown.line.isEmpty() ? shown.tooltip : shown.line;
    return shown;
}

}
