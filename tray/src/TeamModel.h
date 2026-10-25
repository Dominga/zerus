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
    // The identity a hand-written plan may carry to adopt an existing session
    // (F8); empty for a member Zerus launches itself.
    QString runId, conversationId;
    bool lead = false;
    // The member queue (A5): its open task first, then the tasks returned to it
    // (pending, with attempts). From `team status` when hgs sends it; an older
    // hgs gets the same list derived from the tasks.
    QList<int> queue;
    bool queueKnown = false;   // `queue` was in the JSON, even when empty
    QJsonObject json() const;
};

// An existing session a member may be given instead of starting one (F8): what
// `plan` and `add-member` accept as `session`. `directory` is the session's own
// folder, which an adopted member keeps.
struct SessionChoice {
    QString host, name, runId, conversationId, agent, directory;
};

// A member's question to the lead (A3); open until the lead answers it.
struct Question {
    QString id, text, answer, by;
    double at = 0, answeredAt = 0;
    bool answered() const { return answeredAt > 0 || !answer.isEmpty(); }
};

// A report that replaced the reply captured for an attempt (A6).
struct ReplacedReport {
    QString text;
    double at = 0;
};

struct Attempt {
    int n = 0;
    QString id, member, state, error, notice, report;
    bool noReport = false;
    double sentAt = 0;
    // Delivery and start (A1); zero or false in an older team file.
    double deliveredAt = 0, startedAt = 0, replyAt = 0, turnEndedAt = 0;
    bool startWarned = false;
    // The reply id kept with turn_ended_at, so a newer turn is not captured (A2).
    QString turnReply;
    double reportAt = 0;
    bool captured = false;              // the text was taken from the last reply
    QList<ReplacedReport> replaced;     // oldest first, at most 3
    // The phase hgs derives for the attempt (spec 2). `phaseKnown` records that
    // hgs sent the key at all: it sends null on purpose for a finished task and
    // for a revoked or refused attempt, and only an older hgs sends no key.
    QString phase;
    bool phaseKnown = false;
    QList<Question> questions;          // oldest first
    const Question *openQuestion() const
    {
        for (const auto &question : questions) if (!question.answered()) return &question;
        return nullptr;
    }
};

struct Dep { int task = 0; bool soft = false; };

// A task parked by the lead or the owner (A5): pending, but never ready,
// rebalanced or handed out until `release`.
struct Hold {
    QString by, reason;
    double at = 0;
};

struct Task {
    int id = 0;
    int phase = 1;             // the approved initial plan is phase 1
    bool pinned = false;       // stays with its assignee
    QString title, description, status, assignee, decidedBy, reason;
    QStringList criteria;
    QList<Dep> deps;
    QList<Attempt> attempts;   // oldest first
    bool held = false;         // A5: the lead holds the task
    Hold hold;
    const Attempt *last() const { return attempts.isEmpty() ? nullptr : &attempts.last(); }
    bool finished() const { return status == "accepted" || status == "rejected" || status == "cancelled"; }
    QJsonObject json() const;
};

// A phase appended by `extend`; the approved initial plan (phase 1) has no
// record. A `draft` phase waits for the owner, `auto` becomes approved at once.
// F10: one line of what the lead changed in a draft, as the store reports it,
// so the owner can read the diff before approving. `task` is the number the
// change names; `from`/`to` say what moved or changed.
struct PhaseChange {
    QString kind;     // added, removed, changed, moved
    int task = 0;
    QString title, from, to;
    bool added() const { return kind == "added"; }
    bool removed() const { return kind == "removed"; }
};

struct Phase {
    int n = 0;
    QString title, approval, state, addedBy, decidedBy;
    bool barrier = false;      // no task starts before every lower phase is finished
    double addedAt = 0, decidedAt = 0;
    QList<int> tasks;          // task ids; a discarded phase keeps them
    QList<PhaseChange> changes; // F10: the lead's changes since the owner last saw it
    bool draft() const { return state == "draft"; }
};

// Set by `hgs team finish`: who finished the team and the bounded summary.
struct Finish {
    QString by, summary;
    double at = 0;
};

// A member session that still runs after its team is Done or Stopped (F7,
// spec 9). The desktop shows it apart from the active members and never stops
// it by itself.
struct LeftRunning {
    QString member, role, host, session, runId, conversationId;
};

struct Team {
    QString id, name, status, project, folder, lead;
    qint64 revision = 0;
    double created = 0, updated = 0;
    bool allHard = false;
    int reportTimeout = 120;
    QString approval;          // settings.approval; empty when the file has none
    QString phaseApproval;     // settings.phase_approval; empty = same as approval
    QString idle;              // the "all tasks decided" notice: pending, sending, sent, unconfirmed
    QList<Member> members;     // removed members are left out
    QList<Task> tasks;
    QList<Phase> phases;       // phase 1 has no record
    bool hasFinish = false;
    Finish finish;
    // `team status` and `team list` derive these; old files are derived here.
    bool waitingForLead = false;
    int pendingPhase = 0;      // the first draft phase, 0 when none
    // Spec 10: any number of draft phases may wait; approved strictly in order.
    QList<int> pendingPhases;
    // Done and Stopped teams: the member sessions that still run (F7).
    QList<LeftRunning> leftRunning;
    bool leftRunningKnown = false;
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
    const Phase *phase(int n) const;
    const LeftRunning *leftOf(const QString &member) const;
    QString roleOf(const QString &member) const;
    // Only a team with a phase record groups its plan by phase.
    bool phased() const { return !phases.isEmpty(); }
    QString effectivePhaseApproval() const { return phaseApproval.isEmpty() ? approval : phaseApproval; }
    // The draft records in phase order (spec 10).
    QList<int> draftPhases() const;
};

// A phase as the plan, board, layers and graph group it. Phase 1 has no
// record; `done` counts its finished tasks and `total` all its tasks.
struct PhaseGroup {
    int n = 0;
    QString title;
    bool barrier = false, draft = false;
    QList<int> tasks;
    int done = 0;
    bool finished() const { return !tasks.isEmpty() && done == tasks.size(); }
};
// The phases to show, in phase order; empty while the plan is phase 1 only.
QList<PhaseGroup> phaseGroups(const Team &team);
// G6: the team's progress in two small numbers instead of one over every task.
// The first bar follows the phase in work — the lowest phase that is not a
// draft and still has open tasks — or, when none is in work, the next draft
// waiting for approval. The second counts finished phases out of the planned
// ones; a discarded draft is not planned and never counts.
struct PhaseProgress {
    int phase = 0;          // the live number of the phase the first bar shows
    QString title;
    bool draft = false;     // that phase waits for approval
    int done = 0, total = 0;
    int phasesDone = 0, phasesTotal = 0;
    bool valid() const { return phase > 0 && total > 0; }
    QString phaseText() const { return QStringLiteral("%1/%2").arg(done).arg(total); }
    QString phasesText() const { return QStringLiteral("%1/%2").arg(phasesDone).arg(phasesTotal); }
    QString tooltip() const;
};
PhaseProgress phaseProgress(const Team &team);
// Why phase N may not be approved yet (spec 10), empty when it may: every
// lower phase must be approved and its tasks decided. "Phase 2 opens after
// phase 1 ends."
QString approveBlocked(const Team &team, int phase);
// The same reason for the compact draft row ("after phase 3"); the full
// sentence stays its tooltip.
QString approveBlockedShort(const Team &team, int phase);
// True when this member's session still runs after the team ended (F7).
bool leftMember(const Team &team, const QString &member);
// "Phase 2 · QA fixes · 3/5" (the title is left out when empty).
QString phaseHeading(const PhaseGroup &group);
// Layers by longest path inside one phase; dependencies outside it are ignored
// (each phase draws its own layers).
QMap<int, int> phaseLayers(const Team &team, const QList<int> &tasks);

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
// The team has at least one task and every task is finished (draft is not).
bool allDecided(const Team &team);
// Questions and approvals waiting for the lead: a member session's native
// question or approval, and a team question asked with `hgs team ask` (A3).
int answers(const Team &team, const LiveLookup &live);
// Ancestors and descendants of a task, itself included.
QSet<int> chain(const Team &team, int task);
// Tasks by layer for the list: layer 1 first.
QMap<int, QList<int>> layered(const Team &team);
bool ready(const Team &team, const Task &task);

// The shown phase of an attempt (spec 2): sent, delivered, late, working,
// asking, reported, no_report, stopped, held, rejected, unconfirmed. hgs
// derives it into the attempt's `phase`; an older hgs gets the same answer
// from the attempt's fields and its task's hold.
QString attemptPhase(const Task &task, const Attempt &attempt);
// The last attempt's phase; empty without attempts.
QString lastPhase(const Task &task);
// Unanswered questions of the task's last attempt, oldest first.
QList<Question> openQuestions(const Task &task);
const Question *question(const Task &task, const QString &id);
// The member queue (A5): its open task, then the tasks returned to it
// (pending, with attempts). Nothing else.
QList<int> memberQueue(const Team &team, const QString &member);

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
    QString phaseApproval;     // "" = same as the plan's approval
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
