#pragma once
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QMap>
#include <QSet>
#include <QString>
#include <QStringList>
#include <functional>

// WhiteWh fork: agent teams as the desktop shows them. hgs owns the team
// (`hgs team list|status|events`); this reads its JSON and derives what a row
// or the Team tab says: member and team states, problems, progress, the feed.
// Statuses here never change a session's own status; they are a layer on top.
namespace Teams {

struct Member {
    QString id, role, agent, model, computer, state, error;
    // The member's session once it exists: computer alias (empty = the lead's
    // computer) and name.
    QString host, session;
    bool lead = false;
};

struct Attempt {
    int n = 0;
    QString id, member, state, error, notice, report;
    bool noReport = false;
    double sentAt = 0;
};

struct Dep { int task = 0; bool soft = false; };

struct Task {
    int id = 0;
    QString title, description, status, assignee, decidedBy, reason;
    QStringList criteria;
    QList<Dep> deps;
    QList<Attempt> attempts;   // oldest first
    const Attempt *last() const { return attempts.isEmpty() ? nullptr : &attempts.last(); }
    bool finished() const { return status == "accepted" || status == "rejected" || status == "cancelled"; }
};

struct Team {
    QString id, name, status, project, folder, lead;
    qint64 revision = 0;
    double created = 0, updated = 0;
    bool allHard = false;
    int reportTimeout = 120;
    QList<Member> members;     // removed members are left out
    QList<Task> tasks;
    // Only from `team status`: layers by task, tasks in a cycle, ready tasks,
    // plan errors and warnings.
    bool full = false;
    QMap<int, int> layers;
    QSet<int> cycle;
    QList<int> ready;
    QStringList errors, warnings;
    const Member *member(const QString &id) const;
    const Member *memberOfSession(const QString &host, const QString &session) const;
    const Task *task(int id) const;
    QString roleOf(const QString &member) const;
};

Team parse(const QJsonObject &value);
QList<Team> parseList(const QJsonObject &value);

// A member's session as the fleet shows it.
struct Live {
    bool known = false;      // the session is listed
    bool offline = false;    // its computer does not answer
    bool attention = false;  // a question or approval waits for the owner
    bool working = false;
    QString attentionText;
};
using LiveLookup = std::function<Live(const Member &)>;

enum class Tone { Neutral, Active, Assigned, Done, Muted, Dim, Warn, Danger, Planning };

struct Status {
    QString kind;        // working, submitted, idle, no_report, unconfirmed, offline, answer, paused, ...
    QString text;        // "Working on #3 Saved cards API"
    QString shortText;   // "#3", "Offline"
    Tone tone = Tone::Neutral;
    int task = 0;
    bool problem = false;
};

Status memberStatus(const Team &team, const Member &member, const Live &live);
// "In progress · attempt 2" and its tone.
Status taskStatus(const Task &task);

struct Problem {
    QString kind, member, text;   // text: "#6 Unit tests — Tester · Delivery unconfirmed"
    int task = 0;
    Tone tone = Tone::Danger;
};
QList<Problem> problems(const Team &team, const LiveLookup &live);

// planning, running, paused, stuck, done, stopped, archived. Stuck: a running
// team where no open task can move without you.
QString displayStatus(const Team &team, const LiveLookup &live);
QString statusLabel(const QString &status);
Tone statusTone(const QString &status);
// "Next: #4, #5 wait for #3 Saved cards API" or "Ready: #4 Payment storage".
QString nextLine(const Team &team);
// One tone per task, in task order, for the segmented progress bar.
QList<Tone> progress(const Team &team);
int accepted(const Team &team);
// Questions and approvals waiting in member sessions.
int answers(const Team &team, const LiveLookup &live);
// Ancestors and descendants of a task, itself included.
QSet<int> chain(const Team &team, int task);
// Tasks by layer for the list: layer 1 first.
QMap<int, QList<int>> layered(const Team &team);
bool ready(const Team &team, const Task &task);

struct FeedLine { double at = 0; qint64 seq = 0; QString kind, by, text; Tone tone = Tone::Neutral; };
FeedLine feedLine(const Team &team, const QJsonObject &event);

}
