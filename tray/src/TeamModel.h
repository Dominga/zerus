#pragma once
#include <QCoreApplication>
#include <QHash>
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

// Fallback model suggestions per agent; the role editor replaces them with
// the selected account's native catalog. A model field still takes any name
// the agent's command line knows.
inline QStringList knownModels(const QString &agent)
{
    static const QMap<QString, QStringList> known{
        {"claude", {"opus", "sonnet", "haiku"}},
        {"codex", {"gpt-5.5-codex", "gpt-5.5"}},
        {"kimi", {"kimi-code/kimi-for-coding", "kimi-code/kimi-for-coding-highspeed", "kimi-code/k3",
                  "kimi-code/k3-256k", "kimi-code/k3.1-pro", "kimi-code/k3.1-pro-highspeed"}}
    };
    return known.value(agent);
}

// U45: a model a role's or member's session may switch to before a task, with
// its effort (empty: the model's own). Written as {model, effort}.
struct ModelChoice {
    QString model, effort;
    bool operator==(const ModelChoice &other) const { return model == other.model && effort == other.effort; }
    bool operator!=(const ModelChoice &other) const { return !(*this == other); }
    QString text() const { return effort.isEmpty() ? model : model + QLatin1Char('@') + effort; }
};
QList<ModelChoice> parseModelChoices(const QJsonValue &value);
QJsonArray modelChoicesJson(const QList<ModelChoice> &choices);

// A member's question to the lead (A3), open until the lead answers it. B1/C4:
// a permission request carries the exact command, its source ("tool_input",
// "process" or "unknown"), the agent's reason and the question hash hgs team
// answer expects.
struct Question {
    QString id, text, answer, by;
    double at = 0, answeredAt = 0;
    QString tool, command, commandSource, reason, decision;
    QString hash;
    // C4: the native answer this card needs: the card's own question id and the
    // option ids for allow and deny, as status sends them in question.answer.
    struct NativeAnswer { QString questionId, allow, deny; bool known = false; };
    NativeAnswer native;
    // U19 (#520): a subagent card ("Start N subagents"); `record` keeps the
    // status record, whose `subagents` list (or reason text) gives the rows.
    bool subagents = false;
    QJsonObject record;
    bool answered() const { return answeredAt > 0 || !answer.isEmpty(); }
    bool permission() const { return !tool.isEmpty() || !command.isEmpty() || !reason.isEmpty(); }
};

// C4: one answered permission request (members[].permission_answers keeps the
// last 20): what was decided, who decided it, the tool and command it asked
// about and the deny text.
struct PermissionAnswer {
    QString questionId, decision, by, reason, tool, command;
    double at = 0;
    bool denied() const { return decision == "deny"; }
};

struct Member {
    QString id, role, agent, model, computer, state, error;
    QString account, effort, permissions, folder, directory, prompt;
    // U54: the account its session really runs on (team status's
    // session_account), which may differ from the configured `account`.
    QString sessionAccount;
    bool sessionAccountKnown = false;
    // The member's session once it exists: computer alias (empty = the lead's
    // computer) and name.
    QString host, session;
    // The identity a hand-written plan may carry to adopt an existing session
    // (F8); empty for a member Zerus launches itself.
    QString runId, conversationId;
    bool lead = false;
    // H1: the session stopped at a usage limit; the reset time when the
    // session reported one, and whether Zerus waits for it (resume after limits).
    struct Limit { bool known = false; double at = 0, resetsAt = 0; QString detail; };
    Limit limited;
    bool limitResume = false;
    // H3: its DeepSeek host serves other members too.
    bool hostShared = false;
    // The member queue (A5): its open task first, then the tasks returned to it
    // (pending, with attempts). From `team status` when hgs sends it; an older
    // hgs gets the same list derived from the tasks.
    QList<int> queue;
    bool queueKnown = false;   // `queue` was in the JSON, even when empty
    // U36 (#532): the stage it has (delivered) first, then the waiting stages
    // it may take; derived from the tasks when hgs sends none.
    struct StageEntry { int task = 0, round = 0; QString stage, state; double since = 0; };
    QList<StageEntry> stageQueue;
    bool stageQueueKnown = false;
    // #635 N4: the task and stage whose hold keeps this member's folder, as
    // hgs's status sends it (absent when nothing holds it).
    struct Hold { int task = 0; QString stage; };
    Hold heldBy;
    // G7: the MCP servers and services this member's session runs; they outlive
    // the session and are not Background work.
    struct Service { QString name, command; double since = 0; };
    QList<Service> services;
    // C12: the declared time budget of the member's background job.
    struct Budget {
        QString label, state, command;
        int minutes = 0, redMinutes = 0, elapsedMinutes = 0;
        double since = 0;
        bool known = false;
    };
    Budget budget;
    // C4: the member's pending permission request and the answers so far, as
    // `team status` sends them in members[].question and permission_answers.
    Question permission;
    QList<PermissionAnswer> permissionAnswers;
    // Spec 5.6 (U25): the member it reports to (empty: the lead) and whether
    // it is a sub-lead, which hands out, reviews and accepts its reports' tasks.
    QString parent;
    bool sub = false;
    // U45/U46: the models it may switch between (empty: its launch model) and
    // what Zerus does to its context before a task ("": its role's, none,
    // compact or clear); sent back only when set.
    QList<ModelChoice> models;
    QString contextBeforeTask;
    // U49: it finishes its open work, takes nothing new and then leaves.
    bool draining = false;
    QJsonObject json() const;
};

// An existing session a member may be given instead of starting one (F8): what
// `plan` and `add-member` accept as `session`. `directory` is the session's own
// folder, which an adopted member keeps.
struct SessionChoice {
    QString host, name, runId, conversationId, agent, directory;
};

// A report that replaced the reply captured for an attempt (A6).
struct ReplacedReport {
    QString text;
    double at = 0;
};

// D1: one criterion and one evidence item of a structured report.
struct ReportCriterion {
    int n = 0;
    QString result, note;      // pass, fail, partial, n/a
    QList<int> evidence;       // indexes into the evidence list
};
struct ReportEvidence {
    QString kind;              // command, image, html, file, log, url, text
    QString caption, path, url, command, tail, text;
    int exit = 0;
    bool hasExit = false;
};
struct StructuredReport {
    bool present = false;
    QString summary, commit;
    QList<ReportCriterion> criteria;
    QList<ReportEvidence> evidence;
    QStringList limits;
};

// D2: one finding as `hgs team status` pages it.
struct TeamFinding {
    // N17, N18: the owner's replan proposal and a fix_phase finding's phase.
    QString id, fromTask, severity, title, file, scenario, task, state, by, note, proposal;
    int line = 0, phase = 0;
    double at = 0;
};

struct Attempt {
    int n = 0;
    QString id, member, state, error, notice, report;
    bool noReport = false;
    double sentAt = 0;
    // D1: attempts[].report.structured and .unstructured.
    StructuredReport structured;
    bool unstructured = false;
    // Delivery and start (A1); zero or false in an older team file.
    double deliveredAt = 0, startedAt = 0, replyAt = 0, turnEndedAt = 0;
    bool startWarned = false;
    // U31: this attempt worked a task of a draft phase.
    bool workedAhead = false;
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
    QList<PermissionAnswer> permissions; // B1: answered requests, oldest first
    // N6: the check this attempt ran (attempts[].check): exit and last lines.
    struct CheckResult { int exit = 0; QString tail; bool known = false; };
    CheckResult checkResult;
    const Question *openQuestion() const
    {
        for (const auto &question : questions) if (!question.answered()) return &question;
        return nullptr;
    }
};

struct Dep { int task = 0; bool soft = false; };

// U36: a task's pipeline as hgs snapshots it at the first hand-out
// (`pipeline_run`): its stages after the implementer's report, the returned
// rounds before the lead decides, where a test works (`hold` the task's folder
// or a detached `checkout`), the final acceptance (lead, sub_lead or auto) and
// every stage run, oldest first. A run checks one attempt (`attempt` is its n)
// and is waiting (for a member), delivered, passed, returned, skipped or
// revoked; #532 adds its folder, checkout, hold and what changed.
// U47/U48: a command or land stage carries the command Zerus runs, its
// expected exit and time limit, and a land stage's exit map; its run carries
// the exit and output tail (`check`), the commit it ran on, whether the same
// tail on the same commit made it flaky and a land stage's outcome. A stage
// member's runs of the build and test templates are kept on its run.
struct StageSpec {
    QString kind, role, member, command;
    int expectExit = 0, timeoutMinutes = 30;
    QMap<QString, QString> exits;
    bool runsCommand() const { return kind == QLatin1String("command") || kind == QLatin1String("land"); }
};
struct StageRun {
    QString id, kind, member, state, by, text, folder, checkout, commit, outcome;
    int stage = 0, round = 0, attempt = 0;
    double at = 0, deliveredAt = 0, endedAt = 0;
    QStringList findings;
    bool holds = false, flaky = false;
    QJsonArray changed;   // {commit, files} or {path, size, deleted}
    struct Check { int exit = 0; QString tail; bool known = false; };
    Check check;
    struct Ran { QString templateId, suite; int exit = 0; bool exited = false, timedOut = false; qint64 ms = 0; };
    QList<Ran> ran;       // oldest first
    bool open() const { return state == QLatin1String("waiting") || state == QLatin1String("delivered"); }
};
struct PipelineRun {
    QString id, name, folder, accept;
    int maxRounds = 0, round = 0;
    QList<StageSpec> stages;
    QList<StageRun> runs;
    // #635 L2: the returns among the runs hgs trimmed (it keeps 64); the
    // round limit still counts them.
    int trimmedReturns = 0;
    bool known = false;
    int rounds() const { return maxRounds > 0 ? maxRounds : 3; }
    const StageRun *openRun() const;
    int returned() const;   // the stage returns so far (the rounds used), trimmed ones too
};

// G1: the sub-team a task was delegated to (status children[] and
// tasks[].subteam), and the parent task a team came from.
struct Subteam {
    QString team, name, status;
    int done = 0, total = 0;
    bool known = false;
};
struct ParentLink {
    QString team, lead, member;
    int task = 0;
    bool known = false;
};

// A task parked by the lead or the owner (A5): pending, but never ready,
// rebalanced or handed out until `release`.
struct Hold {
    QString by, reason;
    double at = 0;
};

// U27: a visual lane of a phase, in order; the label names the row ("Checkpoint
// 1"). Lanes are drawn by the desktop and never change hand-out.
struct Lane {
    int n = 0;
    QString label;
    bool known() const { return n > 0; }
};

// U37: the task's working time as hgs status computes it (tasks[].timing):
// the worked seconds (a running attempt up to the status time), when the
// running attempt started (0 while none runs), the waits for dependencies,
// a free member and the lead, each attempt's span, and the waits an older
// team never recorded.
struct Timing {
    struct Span { int n = 0; QString member, state; double from = 0, to = 0; qint64 worked = 0, memberWait = 0, leadWait = 0; };
    qint64 worked = 0, deps = 0, member = 0, lead = 0;
    double runningSince = 0;
    QList<Span> attempts;
    QStringList unknown;
    bool known = false;
};

struct Task {
    int id = 0;
    int phase = 1;             // the approved initial plan is phase 1
    bool pinned = false;       // stays with its assignee
    QString title, description, status, assignee, decidedBy, reason;
    QString kind;              // C5: "check" for a check task
    // U45: the models allowed for it (`model` or `model@effort`; empty: any of
    // its member's) and the role it is for when it has no assignee.
    QStringList models;
    QString role;
    QStringList criteria;
    QList<Dep> deps;
    QList<Attempt> attempts;   // oldest first
    bool held = false;         // A5: the lead holds the task
    Hold hold;
    // U59: the free phase's integration-and-build task, a lead task like any.
    bool phaseBuild = false;
    // C3: why the task waits and what unblocks it.
    struct Wait { QString for_, unblock; QStringList on; bool known = false; };
    Wait wait;
    // C5: a check task runs exactly this command and reports its exit.
    struct Check { QString command; int expectExit = 0, timeoutMinutes = 0; };
    Check check;
    struct CheckResult { int exit = 0; QString tail; bool known = false; };
    CheckResult checkResult;
    // G1: the sub-team delegated from this task.
    Subteam subteam;
    // U27: the visual lane, read from `lane`; n 0 means none.
    Lane lane;
    // U29: joined its phase after the phase started, on the owner's word
    // twice: who added it, when, and the owner's two sentences.
    struct AddedLate { QString by, request, confirm; double at = 0; bool known = false; };
    AddedLate addedLate;
    Timing timing;
    // Owner 2026-10-31 (#745, #746): the time estimate in minutes (-1: none),
    // where it comes from (history, role, lead, none; empty: an older hgs
    // sends none), hgs's text, and the minutes the open attempt has run.
    double estimateMinutes = -1, estimateSpent = -1;
    QString estimateSource, estimateText;
    bool estimateKnown() const { return !estimateSource.isEmpty(); }
    // U36: the pipeline the plan chose ("" the default, "none", or a preset
    // id), its snapshot once handed out, and the members that may take its
    // open stage.
    QString pipeline;
    PipelineRun pipelineRun;
    QStringList stageCandidates;
    bool stageCandidatesKnown = false;   // hgs sent the list (an older one does not)
    bool inStage() const { return status == QLatin1String("in_review") || status == QLatin1String("in_test"); }
    bool isCheck() const { return kind == "check" || !check.command.isEmpty(); }
    const Attempt *last() const { return attempts.isEmpty() ? nullptr : &attempts.last(); }
    bool workedAhead() const { const auto *a = last(); return a && a->workedAhead; }
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
    // U63: recorded after the phase was accepted (the lead edited it while
    // its work had not started); queued and running headers count only these.
    bool accepted = false;
    bool added() const { return kind == "added"; }
    bool removed() const { return kind == "removed"; }
};

// U63 (owner, 2026-10-31): a pause on an accepted phase whose work has not
// started; it does not start until the pause is removed. `owner` when the
// owner paused it, else the lead ("by" is the server's name for who).
struct PhasePause {
    bool on = false;
    QString by, reason;
    bool owner = false;
    double at = 0;
};
PhasePause readPhasePause(const QJsonValue &value);

struct Phase {
    int n = 0;
    QString title, approval, state, addedBy, decidedBy;
    bool barrier = false;      // no task starts before every lower phase is finished
    double addedAt = 0, decidedAt = 0;
    QList<int> tasks;          // task ids; a discarded phase keeps them
    QList<PhaseChange> changes; // F10: the lead's changes since the owner last saw it
    // F12: why a draft waits - "Waiting for the previous phase to be ready"
    // with that phase and its open tasks. N40: an approved phase still running
    // no longer blocks the draft: "Can run alongside phase N", `alongside`.
    struct Waiting { QString text; int phase = 0; QList<int> open; bool alongside = false; };
    Waiting waiting;
    // F13: the owner's sentence the lead approved on.
    QString ownerWord;
    // U25 (server #378): the note under the phase header, and the owner's
    // send-back of a draft (state "rejected") until the lead reworks it.
    QString note;
    struct Rejection { QString comment, by; double at = 0, reworkedAt = 0; };
    Rejection rejection;
    bool draft() const { return state == "draft"; }
    bool rejected() const { return state == "rejected"; }
    bool queued() const { return state == "queued"; }
    // U61: a draft Auto accepts at its first hand-out; until then the lead
    // edits it. The lead may let idle members start it early (start_ahead).
    bool startAhead = false;
    // U63: the pause on an accepted phase, and "owner" when the owner accepts
    // the phase once its tasks are decided (absent or "lead" otherwise). The
    // state then reads awaiting_acceptance, and done once he accepted it.
    PhasePause pause;
    QString acceptance;
    bool ownerAccepts() const { return acceptance == QLatin1String("owner"); }
    bool awaitingAcceptance() const { return state == QLatin1String("awaiting_acceptance"); }
    bool accepted() const { return state == QLatin1String("approved") || state == QLatin1String("queued"); }
    bool autoDraft() const { return draft() && approval == "auto"; }
    bool acceptedByAuto() const { return (state == "approved" || state == "queued") && decidedBy == "Auto"; }
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

// #756: a member's build through the build broker: building (in its own
// snapshot) since `since`, or waiting with `ahead` builds before it.
struct BuildState {
    QString state, templateName;
    double since = 0;
    int ahead = 0;
    bool snapshot = false;
    bool building() const { return state == QLatin1String("building"); }
    bool waiting() const { return state == QLatin1String("waiting"); }
    // "building (snapshot) 2:10", "waiting for a build, 1 ahead".
    QString text(double now) const;
};

struct Team {
    QString id, name, status, project, folder, lead;
    // D1: settings.evidence_folders, where report evidence may be previewed.
    QStringList evidenceFolders;
    qint64 revision = 0;
    // F11: the revision that last renumbered draft phases or tasks.
    qint64 phaseNumbersRevision = 0;
    double created = 0, updated = 0;
    bool allHard = false;
    int reportTimeout = 120;
    QString approval;          // settings.approval; empty when the file has none
    QString phaseApproval;     // settings.phase_approval; empty = same as approval
    QString idle;              // the "all tasks decided" notice: pending, sending, sent, unconfirmed
    QList<Member> members;     // removed members are left out
    QList<Task> tasks;
    QList<Phase> phases;       // phase 1 has no record
    // G1: this team's parent task and its sub-teams, as status sends them.
    ParentLink parent;
    QList<Subteam> children;
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
    // B5: a role whose permissions profile the plan widens beyond the template;
    // a plan with one cannot be approved until the owner confirms it.
    struct Widening { QString member, role, from, to; };
    QList<Widening> widenings;
    // Only from `team status`: layers by task, tasks in a cycle, ready tasks,
    // plan errors and warnings.
    bool full = false;
    // U58: a copy Graph, Board, Layers and the plan show without its cancelled
    // work (withoutCancelled). It is for display only: a plan is never sent
    // from it, or the hidden tasks would leave the plan.
    bool cancelledHidden = false;
    QMap<int, int> layers;
    QSet<int> cycle;
    QList<int> ready;
    QStringList errors, warnings;
    // U25: the same errors and warnings with the task each names (0 for none),
    // for the plan problems list and a phase's error count.
    struct PlanIssue { int task = 0; QString text; };
    QList<PlanIssue> errorItems, warningItems;
    // C7: the plan-health numbers the lead reads before asking for approval and
    // the owner sees in the GUI, with the starving members while the team runs.
    struct Health {
        bool known = false;
        int tasks = 0, criticalPath = 0, widestReady = 0;
        double parallelism = 0;
        QList<QPair<QString, QPair<int, int>>> perMember;   // role, (tasks, on critical path)
        struct FanIn { int task = 0; QList<int> waitsFor; QString kind; };
        QList<FanIn> fanIn;
        QStringList warnings;
        struct Starved { QString member, role; int minutes = 0; QList<int> blocking; };
        QList<Starved> starved;
    };
    Health health;
    // G9 (spec 10.2): status.phase_summary - one header record per phase in
    // display order, phase 1 included. Empty on an older hgs; the desktop then
    // falls back to its own heading.
    struct PhaseSummary {
        int n = 0;
        QString title, status, healthLine;   // status: done|in_work|approved|draft|paused
        QString state;   // U25: planned|draft|rejected|queued|running|done (empty: older hgs); U63: awaiting_acceptance
        int done = 0, total = 0;
        bool barrier = false;
        PhasePause pause;    // U63: a paused phase reads "queued" with a pause
        QString acceptance;  // U63: "owner" or "lead"
    };
    QList<PhaseSummary> phaseSummary;
    // G9 (spec 10.1): status.phases_health - the plan health of each phase,
    // keyed by phase number; empty on an older hgs.
    QMap<int, Health> phasesHealth;
    const PhaseSummary *phaseSummaryOf(int n) const;
    const Health *phaseHealthOf(int n) const;
    // C4: the undecided reports on the owner's attention list.
    // U18/N7: `summary` is the server's sentence (member and what waits),
    // `command` the hgs command that answers it.
    struct Attention { QString kind, member, summary, command; int task = 0, minutes = 0, phase = 0, count = 0; double since = 0; };
    QList<Attention> attention;
    // D2: findings_* from status (open items and the deferred list).
    QList<TeamFinding> findings;
    bool findingsKnown = false;
    QJsonObject findingsSummary;
    QString findingsNext;
    // F13: "approved by Lead on the owner's word" and the sentence.
    QString approvedBy, ownerWord;
    // F5: a team in status "draft" is edited by the owner and the lead before
    // it exists: its goal, protocol addendum, folder policy and limit, the
    // draft revision every update carries and the fields the lead changed that
    // the owner has not seen yet.
    QString goal, leadProtocol, folderPolicy;
    int maxMembers = 0;
    qint64 draftRevision = 0;
    QStringList draftUnseen;
    bool draft() const { return status == QLatin1String("draft"); }
    // F7: the team a next goal came from, and the one it led to.
    QString previous, next;
    // H1: members limited by usage wait for their reset (settings).
    bool resumeAfterLimits = false;
    // U25: the owner's auto-accept of error-free phase drafts (settings.auto_accept),
    // and the open tasks of running phases, the only ones that make a team Stuck.
    bool autoAccept = false;
    // U59: how the team plans, "standard" or "free" (known only when hgs sent
    // it); the lead's proposal while it plans, and the open free phase.
    QString planMode;
    bool planModeKnown = false;
    QString planModeOffer, planModeReason;
    struct FreePhase {
        bool known = false;
        int phase = 0;
        QString state;          // open (tasks run) or idle
        QList<int> running;
        int accepted = 0, acceptedSince = 0;
        double lastBuildAt = 0;
        int closePending = 0;   // the phase a close waits for, 0 when none
        bool idle() const { return state == QLatin1String("idle"); }
    };
    FreePhase freePhase;
    bool freeMode() const { return planMode == QLatin1String("free"); }
    // U31: idle members may take independent tasks of the next draft phase.
    bool workAhead = false;
    // #756: the build broker (owner setting, off by default) and, while it
    // is on, each member's build state by member id, from team status.
    bool buildBroker = false;
    QHash<QString, BuildState> buildStates;
    QList<int> stuckScope;
    // The settings as stored, so a change sends them back whole.
    QJsonObject settingsJson;
    // U51: the owner's intake keys for urgent tasks (the key itself is shown
    // only once, when it is created).
    struct Intake { QString id, label; double created = 0; };
    QList<Intake> intakes;
    // F8: the lead's membership proposals, decided by the owner. `member` is
    // the proposed member (add), its id (remove) or the swap fields (swap).
    // U59: a switch-plan-mode proposal names its mode and the content hash the
    // owner's answer is bound to. U57: `change` is the exact change Allow
    // applies (members, a phase's new tasks, the tasks to cancel, a phase).
    struct Proposal {
        QString id, kind, by, state, reason, goal, note, mode, hash;
        double at = 0;
        QJsonValue member, change;
        bool open() const { return state == QLatin1String("open"); }
        QString memberId() const;
    };
    QList<Proposal> proposals;
    // U29: additions to a running phase that wait for the owner's typed
    // confirmation (pending_additions): the task titles that would join.
    struct Addition {
        QString id, by, request;
        int phase = 0;
        QStringList titles;
        double warnedAt = 0;
    };
    QList<Addition> additions;
    // H6: the sessions a swap left running for the owner to stop (last 20).
    struct Swapped { QString member, old, by; double at = 0; };
    QList<Swapped> swapped;
    // H3: the DeepSeek hosts that serve members.
    struct Host { QString agent; qint64 pid = 0; double started = 0; QStringList members; };
    QList<Host> hosts;
    const Host *hostOf(const QString &member) const;
    const Member *member(const QString &id) const;
    const Member *memberOfSession(const QString &host, const QString &session) const;
    // Spec 5.6: who reviews a member (its sub-lead parent, else the lead; the
    // lead has none), its direct reports, and the members as a tree: the lead,
    // then each member followed by its reports, depth 0 for the lead.
    QString reviewerOf(const QString &member) const;
    QList<const Member *> reportsOf(const QString &member) const;
    struct TreeNode { const Member *member = nullptr; int depth = 0; bool last = false; QList<bool> through; };
    // `last`: no later sibling under the same parent. `through[k]`: the line of
    // depth k+1 continues past this row (its ancestor there has a later sibling).
    // `shown`, when set, keeps only those members: one whose parent is not
    // shown hangs from its nearest shown ancestor (a folded team's list).
    QList<TreeNode> memberTree(const std::function<bool(const QString &)> &shown = {}) const;
    const Task *task(int id) const;
    const Phase *phase(int n) const;
    const LeftRunning *leftOf(const QString &member) const;
    QString roleOf(const QString &member) const;
    // Who a task goes to: its assignee's role, "Any free <role>" for a task
    // for a role (U45), otherwise `none`.
    QString assigneeRole(const Task &task, const QString &none) const;
    // Only a team with a phase record groups its plan by phase.
    bool phased() const { return !phases.isEmpty(); }
    QString effectivePhaseApproval() const { return phaseApproval.isEmpty() ? approval : phaseApproval; }
    // The draft records in phase order (spec 10).
    QList<int> draftPhases() const;
    // U27: the tasks of one phase, in plan order.
    QList<int> tasksOfPhase(int phase) const;
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
// U58: a phase whose tasks are all cancelled reads cancelled (a discarded or an
// empty phase does not). Graph, Board, Layers and the plan hide cancelled tasks
// and those phases unless the owner turns Show cancelled on; a team with no
// cancelled task is shown as it is.
bool phaseCancelled(const Team &team, int phase);
int cancelledTasks(const Team &team);
Team withoutCancelled(const Team &team);
// Owner, 2026-10-30: a finished phase has tasks and every one of them is
// decided (accepted, rejected or cancelled), not all cancelled, and it is not
// the open free phase. Hide finished leaves those phases and their tasks out
// of Graph, Board and Layers; like withoutCancelled the copy is for display
// only (cancelledHidden keeps a plan from being sent from it).
bool phaseFinished(const Team &team, int phase);
int finishedPhases(const Team &team);
Team withoutFinished(const Team &team);
// U58: the phases a task that has not started (no attempt that reached its
// member: a refused one never did) may move to with `move-task`, keeping its
// number: phase 1 and every phase record that is not discarded, except its own.
// `blocked` is why a target refuses it - a dependency that stays in a later
// phase - and empty when it may move there.
struct MoveTarget { int phase = 0; QString title, blocked; };
bool taskStarted(const Task &task);
QList<MoveTarget> moveTargets(const Team &team, int task);
// G6: the team's progress in two small numbers instead of one over every task.
// The first bar follows the phase in work — the lowest phase that is not a
// draft and still has open tasks — or, when none is in work, the next draft
// waiting for approval. The second counts finished phases out of the planned
// ones; a discarded draft is not planned and never counts.
struct PhaseProgress {
    int phase = 0;          // the live number of the phase the first bar shows
    QString title;
    bool draft = false;     // that phase waits for approval
    // U61: that draft is Auto's (accepted at its first hand-out), started
    // ahead by the lead; or that running phase was accepted by Auto.
    bool autoDraft = false, startAhead = false, byAuto = false;
    int done = 0, total = 0;
    int phasesDone = 0, phasesTotal = 0;
    bool valid() const { return phase > 0 && total > 0; }
    QString phaseText() const { return QStringLiteral("%1/%2").arg(done).arg(total); }
    QString phasesText() const { return QStringLiteral("%1/%2").arg(phasesDone).arg(phasesTotal); }
    QString tooltip() const;
};
PhaseProgress phaseProgress(const Team &team);
// U61: the words of an auto-accepted draft, the same in the phase header,
// the plan's band, the progress tooltip and the Auto-accept switch.
QString autoDraftBadge();                         // "Draft (auto)"
QString autoDraftTip(bool startAhead);            // when Auto fixes it
QString autoAcceptSwitchTip(bool on);
// Why phase N may not be approved yet (spec 10), empty when it may: every
// lower phase must be approved. An approved phase still at work no longer
// blocks it (F12/N40): the draft then runs alongside it.
QString approveBlocked(const Team &team, int phase);
// F12/N40: the earlier approved phase this draft would run alongside (0 when
// none); the desktop warns the owner before approving it.
int alongsidePhase(const Team &team, int phase);
// #724: the owner's question before approving a phase draft or the plan, the
// same in the team view and on Waiting for you: the dialog's object name,
// title, text and action, and what the approval sends besides the team.
struct Approval { QString name, title, text, action; QJsonObject input; };
Approval phaseApproval(const Team &team, int phase);
// `mode`: the owner's plan mode choice (U59), empty for none (standard).
Approval planApproval(const Team &team, const QString &mode);
// U25 (spec 5.2): the owner's state of a phase: planned (the team is planning),
// draft, rejected, queued, running or done. phase_summary[].state when the
// server sends it, else read from the record and the phase's tasks.
QString ownerPhaseState(const Team &team, const PhaseGroup &group);
// U25 (server #378): why the owner may not Accept draft N yet, empty when he
// may. An earlier phase still at work no longer blocks it (the phase waits as
// queued); an earlier draft does.
QString acceptBlocked(const Team &team, int phase);
// The same reason for the compact draft row ("after phase 3"); the full
// sentence stays its tooltip.
QString approveBlockedShort(const Team &team, int phase);
// True when this member's session still runs after the team ended (F7).
bool leftMember(const Team &team, const QString &member);
// C2: one thing on the lead's desk, as `hgs team inbox` lists it.
struct InboxItem {
    QString kind;      // report, question, permission, lead_task, stalled, budget
    QString member, summary, command;
    int task = 0;
    double since = 0;
};
QList<InboxItem> parseInbox(const QJsonObject &value);

// #693: what of a `team status` (or a `team list` item) the views draw, as
// compact JSON: without the revision and updated time every dispatcher write
// moves, a running task's growing worked seconds and an open task's waits
// (the views tick those on their own clock), and the minutes of the health
// warnings. Two polls with the same shape need no rebuild.
QByteArray visibleShape(QJsonObject status);
// "Phase 2 · QA fixes · 3/5" (the title is left out when empty).
QString phaseHeading(const PhaseGroup &group);
// Layers by longest path inside one phase; dependencies outside it are ignored
// (each phase draws its own layers).
QMap<int, int> phaseLayers(const Team &team, const QList<int> &tasks);
// U27: one phase's tasks by lane, in lane order, then the tasks without a lane
// (key 0) which fall back to the dependency layers. Lanes never change hand-out.
QList<QPair<int, QList<int>>> laneGroups(const Team &team, const QList<int> &ids);
// The label a lane carries in a phase (the first task that names one).
QString laneLabel(const Team &team, const QList<int> &ids, int lane);

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

// draft, planning, running, paused, stuck, done, stopped, archived. Stuck: a running
// team where no open task can move without you.
QString displayStatus(const Team &team, const LiveLookup &live);
QString statusLabel(const QString &status);
Tone statusTone(const QString &status);
// F8: what a proposal would do, in one line ("Swap Backend to Codex · gpt-5.5").
QString proposalText(const Team &team, const Team::Proposal &proposal);
// U57: the proposal's exact change, one line per member or task, for its card.
QStringList proposalChanges(const Team &team, const Team::Proposal &proposal);
// H1: "resets 14:30" (today) or "resets 3 Nov 14:30"; empty when unknown.
QString resetText(double resetsAt);
// F4: the one Team designer conversation hgs starts (`catalog designer`). It
// keeps its fixed name and colour, and the desktop files no other work in it:
// it never leads or joins a team.
inline bool isDesignerSession(const QString &host, const QString &session)
{
    return host.isEmpty() && session.endsWith(QLatin1String("/team-designer"));
}
// Its card's fixed title and colour, whatever project the folder falls in.
inline QString designerTitle() { return QCoreApplication::translate("Teams", "Team designer"); }
inline QString designerColor() { return QStringLiteral("#a46ee0"); }
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
// U36: who implemented the attempt a stage run checks (its assignee when the
// attempt is unknown); the task and stage that hold `member`'s folder (#635
// N4: the status's held_by; an older hgs: its waiting task's text, else the
// test rule it had), the task alone (0: none) and "the test stage of #4"; a
// member's stage queue; a waiting stage's place among the waiting stages (1
// first, 0 when it does not wait); "Review" or "Test".
QString implementerOf(const Task &task, const StageRun &run);
Member::Hold folderHold(const Team &team, const QString &member);
int folderHeldBy(const Team &team, const QString &member);
QString holderText(const Member::Hold &hold);
// #538 L6: the task's open stage waits for a member and hgs names no member
// that may take it (never a command or land stage, which Zerus runs).
bool stageWithoutCandidate(const Task &task);
// U49, #581 L2: how many members "Add more" may add: the team's limit less
// every member that is not removed, the lead included, as hgs's scale counts;
// 10 when hgs sends no limit.
int scaleRoom(const Team &team);
QList<Member::StageEntry> stageQueue(const Team &team, const QString &member);
int stageQueuePlace(const Team &team, int task);
QString stageName(const QString &kind);

// The team catalog (`hgs team catalog get`): roles, templates, routing rules
// and the designer's draft of proposed changes.
// N1 (#379): every catalog item keeps the object it was read from (`raw`);
// json() writes the fields the desktop edits over it, so a Save or an Export
// carries the fields it does not know (a role's about, prompt_order, notes...,
// a template's work and phases, a skeleton task's phase, a rule's use).
struct Role {
    QString id, name, color, prompt, agent, account, model, effort, permissions, computer, folder, canMessage;
    QString permissionsProfile;   // B5: read, write, build, commit or network
    QStringList tags;
    bool dangerous = false;
    // U25 (spec 11.3, 12): the editable role. `message` is lead, some or team
    // (can_message keeps lead or team for older readers); `danger` holds the
    // five allowed actions (push, deploy, del, pkgs, net); the prompt blocks
    // go in `promptOrder` (rules, role, protocol, task and note keys), the
    // ones switched off in `promptOff`, a note's text in `notes`.
    QString about, icon, message, joins, protocolExtra, taskTemplate;
    QStringList machines, messageTo, promptOrder, promptOff;
    QJsonObject danger;
    QMap<QString, QString> notes;
    // U45/U46: the models a member of this role may switch between, and its
    // context before a task (none, compact or clear); both copied into a
    // member when it joins.
    QList<ModelChoice> models;
    QString contextBeforeTask;
    // U36: the pipeline this role's tasks use unless the plan names one;
    // empty falls back to the template's.
    QString pipeline;
    QJsonObject raw;
    QJsonObject json() const;
};
struct Seat { QString role; int count = 1; bool optional = false; QJsonObject raw; };
struct SkeletonTask { QString title, role; QList<int> deps; QJsonObject raw; int phase = 1; };
struct Template {
    QString id, name, description, leadProtocol, planning, approval;
    QString phaseApproval;     // "" = same as the plan's approval
    QList<Seat> roster;
    QList<SkeletonTask> tasks;
    int maxMembers = 6;
    QString work;      // U25: worktree, clone, same or cloud ("" = worktree)
    QString phases;    // U25: same, owner or lead (new phases)
    QString pipeline;  // U36: the default pipeline of its tasks ("" = none, today's flow)
    QJsonObject raw;
    QJsonObject json() const;
};
// U25: `use` is a template id or "+role" (adds a member and keeps looking);
// empty uses templateId. An empty approval is the template's own.
struct Rule { QString id, when, templateId, approval; QJsonObject raw; QString use; QJsonObject json() const; };
// U28: a proposal on the planning rules ("planning_rules") carries its text
// in `text`; the others carry an object in `value`.
// U36: one stage of a pipeline preset after the implementer's report: review
// or test by a role (its first free member) or a pinned member; U47/U48: a
// command or land stage Zerus runs itself, with no role or member: its
// command, expected exit and time limit, and a land stage's exit codes mapped
// to accepted, returned or held (empty: 0 accepted, any other held).
struct PipelineStage {
    QString kind, role, member;
    bool hasCommand = false;
    QString command;
    int expectExit = 0, timeoutMinutes = 30;
    QMap<QString, QString> exits;
    QJsonObject raw;
    bool runByZerus() const { return kind == QLatin1String("command") || kind == QLatin1String("land"); }
    QJsonObject json() const;
};
// U36: a named preset ("Code: review, test, lead"): its stages, the returned
// rounds before the lead decides (0 reads as 3), where a test works (hold the
// task's folder or a detached checkout) and the final acceptance (lead,
// sub_lead or auto; empty is lead).
struct Pipeline {
    QString id, name;
    QList<PipelineStage> stages;
    int maxRounds = 0;
    QString folder, accept;
    QJsonObject raw;
    int rounds() const { return maxRounds > 0 ? maxRounds : 3; }
    QJsonObject json() const;
};
PipelineStage parsePipelineStage(const QJsonObject &value);
Pipeline parsePipeline(const QJsonObject &value);
struct Proposal { QString id, kind, action, target, title, reason, state; QJsonObject value; double at = 0; QString text; };
struct Catalog {
    qint64 version = 0;
    bool builtin = false;
    QList<Role> roles;
    QList<Template> templates;
    QList<Rule> routing;
    // U28: the lead's planning rules, versioned with the catalog. Known only
    // when hgs sent them: a catalog saved without them gets the built-in ones.
    QString planningRules;
    bool planningRulesKnown = false;
    // U36: the pipeline presets; an older hgs sends none and a save then
    // writes none.
    QList<Pipeline> pipelines;
    bool pipelinesKnown = false;
    qint64 draftBase = 0;
    QList<Proposal> proposals;
    const Role *role(const QString &id) const;
    // A role by id or by its name, as members name it.
    const Role *roleNamed(const QString &name) const;
    const Template *templ(const QString &id) const;
    const Pipeline *pipeline(const QString &id) const;
    QJsonObject json() const;
};
Role parseRole(const QJsonObject &value);
Template parseTemplate(const QJsonObject &value);
Rule parseRule(const QJsonObject &value);
Catalog parseCatalog(const QJsonObject &get);
// U25: the colors the catalog gives its roles (Team designer), under the role's
// id and name, lowercased with only letters and digits ("Lead (GPT)" is
// "leadgpt"). TeamController keeps them current; the desktop's role colors read
// them before the skin's palette.
QHash<QString, QString> &catalogRoleColors();
void rememberRoleColors(const Catalog &catalog);
QString roleColorKey(const QString &role);
// The catalog with the kept (or also the undecided) changes of its draft applied.
Catalog withProposals(const Catalog &catalog, bool includeUndecided);

struct FeedLine { double at = 0; qint64 seq = 0; QString kind, by, text; Tone tone = Tone::Neutral; };
FeedLine feedLine(const Team &team, const QJsonObject &event);

}
