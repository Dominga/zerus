#pragma once
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QMap>
#include <QSet>
#include <QString>
#include <QStringList>
#include <functional>

// Agent teams as the desktop shows them. hgs owns the team
// (`hgs team list|status|events`); this reads its JSON and derives what a row
// or the Team tab says: member and team states, problems, progress, the feed.
// Statuses here never change a session's own status; they are a layer on top.
namespace Teams {

// Model suggestions per agent; a model field still takes any name the
// agent's command line knows. DeepSeek runs its own model.
inline QStringList knownModels(const QString &agent)
{
    static const QMap<QString, QStringList> known{{"claude", {"opus", "sonnet", "haiku"}}, {"codex", {"gpt-5.5-codex", "gpt-5.5"}}, {"kimi", {"kimi-k2"}}};
    return known.value(agent);
}

struct Member {
    QString id, role, agent, model, computer, state, error;
    QString account, effort, permissions, folder, directory, prompt;
    // The member's session once it exists: computer alias (empty = the lead's
    // computer) and name.
    QString host, session;
    bool lead = false;
    QJsonObject json() const;
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
    QJsonObject json() const;
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
// The plan as `hgs team plan` takes it: members besides the lead, and tasks.
QJsonObject planInput(const Team &team);
// The next free task number and member id.
int nextTaskId(const Team &team);
QString nextMemberId(const Team &team, const QString &role);
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

// The team catalog (`hgs team catalog get`): roles, templates, routing rules
// and the designer's draft of proposed changes.
struct Role {
    QString id, name, color, prompt, agent, account, model, effort, permissions, computer, folder, canMessage;
    QStringList tags;
    bool dangerous = false;
    QJsonObject json() const;
};
struct Seat { QString role; int count = 1; bool optional = false; };
struct SkeletonTask { QString title, role; QList<int> deps; };
struct Template {
    QString id, name, description, leadProtocol, planning, approval;
    QList<Seat> roster;
    QList<SkeletonTask> tasks;
    int maxMembers = 6;
    QJsonObject json() const;
};
struct Rule { QString id, when, templateId, approval; QJsonObject json() const; };
struct Proposal { QString id, kind, action, target, title, reason, state; QJsonObject value; double at = 0; };
struct Catalog {
    qint64 version = 0;
    bool builtin = false;
    QList<Role> roles;
    QList<Template> templates;
    QList<Rule> routing;
    qint64 draftBase = 0;
    QList<Proposal> proposals;
    const Role *role(const QString &id) const;
    // A role by id or by its name, as members name it.
    const Role *roleNamed(const QString &name) const;
    const Template *templ(const QString &id) const;
    QJsonObject json() const;
};
Role parseRole(const QJsonObject &value);
Template parseTemplate(const QJsonObject &value);
Rule parseRule(const QJsonObject &value);
Catalog parseCatalog(const QJsonObject &get);
// The catalog with the kept (or also the undecided) changes of its draft applied.
Catalog withProposals(const Catalog &catalog, bool includeUndecided);

struct FeedLine { double at = 0; qint64 seq = 0; QString kind, by, text; Tone tone = Tone::Neutral; };
FeedLine feedLine(const Team &team, const QJsonObject &event);

}
