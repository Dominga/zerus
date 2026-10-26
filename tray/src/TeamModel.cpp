#include "TeamModel.h"
#include <QCoreApplication>
#include <QDateTime>
#include <QJsonValue>
#include <QRegularExpression>
#include <algorithm>
#include <functional>

namespace Teams {
namespace {
QString tr(const char *text) { return QCoreApplication::translate("Teams", text); }

QString str(const QJsonObject &object, const char *key) { return object.value(QLatin1String(key)).toString(); }

bool openAttempt(const QString &state) { return state == "delivering" || state == "delivered" || state == "unconfirmed"; }

const Attempt *openOf(const Task &task)
{
    for (int i = task.attempts.size() - 1; i >= 0; --i)
        if (openAttempt(task.attempts[i].state)) return &task.attempts[i];
    return nullptr;
}

// Layers by longest path from the roots; DFS back edges are ignored, so a
// cycle does not reshuffle the rest (src/state/teams/plan.rs).
QMap<int, int> computeLayers(const Team &team)
{
    QSet<int> ids;
    for (const auto &task : team.tasks) ids.insert(task.id);
    QMap<int, int> depth;
    QSet<int> active;
    std::function<int(int)> visit = [&](int id) -> int {
        if (depth.contains(id)) return depth[id];
        if (active.contains(id)) return -1;
        active.insert(id);
        int level = 0;
        if (const auto *task = team.task(id))
            for (const auto &dep : task->deps)
                if (ids.contains(dep.task)) { const int d = visit(dep.task); if (d >= 0) level = qMax(level, d + 1); }
        active.remove(id);
        depth[id] = level;
        return level;
    };
    for (const auto &task : team.tasks) visit(task.id);
    return depth;
}
}

const Member *Team::member(const QString &id) const
{
    for (const auto &m : members) if (m.id == id) return &m;
    return nullptr;
}

const Member *Team::memberOfSession(const QString &host, const QString &session) const
{
    for (const auto &m : members) if (!m.session.isEmpty() && m.session == session && m.host == host) return &m;
    return nullptr;
}

const Task *Team::task(int id) const
{
    for (const auto &t : tasks) if (t.id == id) return &t;
    return nullptr;
}

const Phase *Team::phase(int n) const
{
    for (const auto &p : phases) if (p.n == n) return &p;
    return nullptr;
}

const Team::PhaseSummary *Team::phaseSummaryOf(int n) const
{
    for (const auto &summary : phaseSummary) if (summary.n == n) return &summary;
    return nullptr;
}

const Team::Health *Team::phaseHealthOf(int n) const
{
    const auto it = phasesHealth.constFind(n);
    return it == phasesHealth.constEnd() ? nullptr : &it.value();
}

const LeftRunning *Team::leftOf(const QString &member) const
{
    for (const auto &left : leftRunning) if (left.member == member) return &left;
    return nullptr;
}

QList<int> Team::draftPhases() const
{
    QList<int> drafts;
    // A finished team opens nothing new, even if a record was left in draft.
    if (status == "done" || status == "stopped" || status == "archived") return drafts;
    for (const auto &phase : phases) if (phase.draft()) drafts.append(phase.n);
    std::sort(drafts.begin(), drafts.end());
    return drafts;
}

QString Team::roleOf(const QString &id) const
{
    if (id == lead) return tr("Lead");
    if (const auto *m = member(id)) return m->role.isEmpty() ? m->id : m->role;
    return id;
}

QList<int> memberQueue(const Team &team, const QString &member)
{
    // The same list hgs sends for an older hgs: every task with an open attempt
    // of this member (ascending, as hgs sorts them), then the tasks returned to
    // it (pending, with attempts), also ascending (L7).
    QList<int> open, returned;
    for (const auto &task : team.tasks) {
        if (const auto *attempt = openOf(task); attempt && attempt->member == member) open.append(task.id);
        // Returned to it: handed out before, not finished, and waiting.
        if (task.assignee == member && !task.finished() && task.status == "pending" && !task.attempts.isEmpty())
            returned.append(task.id);
    }
    std::sort(open.begin(), open.end());
    std::sort(returned.begin(), returned.end());
    return open + returned;
}

QString attemptPhase(const Task &task, const Attempt &attempt)
{
    // hgs derives the phase and sends null on purpose when the task or the
    // attempt has none: a finished task, a revoked attempt that another attempt
    // replaced, or a refused attempt nothing reached. Only an older hgs sends no
    // key at all; then the same answer is derived here, mirroring
    // ops::attempt_phase (which also knows the live session's state, so an
    // older hgs never yields "stopped").
    if (attempt.phaseKnown) return attempt.phase;
    if (task.finished()) return {};
    if (task.held) return QStringLiteral("held");
    if (attempt.state == "unconfirmed") return QStringLiteral("unconfirmed");
    if (attempt.state == "delivering") return QStringLiteral("sent");
    if (attempt.state == "rejected") return QStringLiteral("rejected");
    if (attempt.state == "refused" || attempt.state == "revoked") return {};
    if (attempt.state != "delivered" && attempt.state != "reported") return {};
    // A question is not a report, and the attempt stays open (A3).
    if (attempt.openQuestion()) return QStringLiteral("asking");
    if (attempt.state == "reported") return QStringLiteral("reported");
    if (attempt.noReport) return QStringLiteral("no_report");
    if (attempt.startedAt > 0) return QStringLiteral("working");
    if (attempt.startWarned) return QStringLiteral("late");
    return QStringLiteral("delivered");
}

QString lastPhase(const Task &task)
{
    const auto *last = task.last();
    return last ? attemptPhase(task, *last) : QString();
}

QList<Question> openQuestions(const Task &task)
{
    QList<Question> open;
    if (const auto *attempt = task.last())
        for (const auto &question : attempt->questions) if (!question.answered()) open.append(question);
    return open;
}

const Question *question(const Task &task, const QString &id)
{
    if (const auto *attempt = task.last())
        for (const auto &item : attempt->questions) if (item.id == id) return &item;
    return nullptr;
}

// C7: the plan-health numbers, with the starving members while running.
// Shared by the plan health and the G9 per-phase health.
Team::Health parseHealth(const QJsonObject &h)
{
    Team::Health health;
    if (h.isEmpty()) return health;
    health.known = true;
    health.tasks = h.value("tasks").toInt();
    health.criticalPath = h.value("critical_path").toInt();
    health.widestReady = h.value("widest_ready").toInt();
    health.parallelism = h.value("parallelism").toDouble();
    const auto members = h.value("per_member").toObject();
    for (auto it = members.begin(); it != members.end(); ++it) {
        const auto load = it.value().toObject();
        health.perMember.append({it.key(), {load.value("tasks").toInt(), load.value("on_critical_path").toInt()}});
    }
    for (const auto &value : h.value("fan_in").toArray()) {
        const auto entry = value.toObject();
        Team::Health::FanIn fanIn;
        fanIn.task = entry.value("task").toInt(); fanIn.kind = str(entry, "kind");
        for (const auto &id : entry.value("waits_for").toArray()) fanIn.waitsFor.append(id.toInt());
        health.fanIn.append(fanIn);
    }
    for (const auto &value : h.value("warnings").toArray()) health.warnings.append(value.toString());
    for (const auto &value : h.value("starved").toArray()) {
        const auto entry = value.toObject();
        Team::Health::Starved starved;
        starved.member = str(entry, "member"); starved.role = str(entry, "role");
        starved.minutes = entry.value("minutes").toInt();
        for (const auto &id : entry.value("blocking").toArray()) starved.blocking.append(id.toInt());
        health.starved.append(starved);
    }
    return health;
}

Team parse(const QJsonObject &v)
{
    Team team;
    team.id = str(v, "id"); team.name = str(v, "name"); team.status = str(v, "status");
    team.project = str(v, "project"); team.folder = str(v, "folder"); team.lead = str(v, "lead");
    team.revision = v.value("revision").toInteger(); team.created = v.value("created").toDouble(); team.updated = v.value("updated").toDouble();
    const auto settings = v.value("settings").toObject();
    team.allHard = settings.value("all_hard").toBool();
    team.reportTimeout = settings.value("report_timeout_seconds").toInt(120);
    team.approval = str(settings, "approval");
    team.phaseApproval = str(settings, "phase_approval");
    for (const auto &value : v.value("members").toArray()) {
        const auto m = value.toObject();
        if (str(m, "state") == "removed") continue;
        Member member;
        member.id = str(m, "id"); member.role = str(m, "role"); member.agent = str(m, "agent"); member.model = str(m, "model");
        member.computer = str(m, "computer"); member.state = str(m, "state"); member.error = str(m, "error");
        member.account = str(m, "account"); member.effort = str(m, "effort"); member.permissions = str(m, "permissions");
        member.folder = str(m, "folder"); member.directory = str(m, "directory"); member.prompt = str(m, "prompt");
        const auto session = m.value("session").toObject();
        member.host = str(session, "host"); member.session = str(session, "name");
        member.runId = str(session, "run_id"); member.conversationId = str(session, "conversation_id");
        if (m.value("queue").isArray()) {
            member.queueKnown = true;
            for (const auto &item : m.value("queue").toArray()) {
                const auto object = item.toObject();
                const int id = item.isDouble() ? item.toInt() : object.value("task").toInt(object.value("id").toInt());
                if (id > 0) member.queue.append(id);
            }
        }
        // G7: the services the member's session runs (MCP servers and the like).
        for (const auto &value : m.value("services").toArray()) {
            const auto service = value.toObject();
            Member::Service entry;
            entry.name = str(service, "name"); entry.command = str(service, "command");
            entry.since = service.value("since").toDouble();
            if (!entry.name.isEmpty()) member.services.append(entry);
        }
        // C4: the member's pending permission request and its answers, as hgs
        // sends them (members[].question, members[].permission_answers).
        if (m.value("question").isObject()) {
            const auto q = m.value("question").toObject();
            member.permission.id = str(q, "question_id");
            member.permission.tool = str(q, "tool");
            member.permission.command = str(q, "command");
            member.permission.commandSource = str(q, "command_source");
            member.permission.reason = str(q, "reason");
            member.permission.at = q.value("asked_at").toDouble();
            member.permission.hash = q.value("question_hash").toString(q.value("expected_question_hash").toString());
            // C4: the card's own ids for Allow and Deny.
            if (q.value("answer").isObject()) {
                const auto a = q.value("answer").toObject();
                member.permission.native.questionId = str(a, "question_id");
                member.permission.native.allow = str(a, "allow");
                member.permission.native.deny = str(a, "deny");
                member.permission.native.known = !member.permission.native.questionId.isEmpty()
                    && !member.permission.native.allow.isEmpty() && !member.permission.native.deny.isEmpty();
            }
        }
        for (const auto &value : m.value("permission_answers").toArray()) {
            const auto a = value.toObject();
            PermissionAnswer answer;
            answer.questionId = str(a, "question_id"); answer.decision = str(a, "decision");
            answer.by = str(a, "by"); answer.reason = str(a, "reason"); answer.at = a.value("at").toDouble();
            if (!answer.decision.isEmpty()) member.permissionAnswers.append(answer);
        }
        if (m.value("budget").isObject()) {
            const auto budget = m.value("budget").toObject();
            member.budget.known = true;
            member.budget.label = str(budget, "label");
            member.budget.state = str(budget, "state");
            member.budget.command = str(budget, "command");
            member.budget.minutes = budget.value("minutes").toInt();
            member.budget.redMinutes = budget.value("red_minutes").toInt();
            member.budget.elapsedMinutes = budget.value("elapsed_minutes").toInt();
            member.budget.since = budget.value("since").toDouble();
        }
        member.lead = member.id == team.lead;
        if (member.lead && member.role.isEmpty()) member.role = tr("Lead");
        team.members.append(member);
        // B5: a member whose plan widens its role's permissions profile.
        if (m.value("profile_widening").isObject()) {
            const auto w = m.value("profile_widening").toObject();
            Team::Widening widening;
            widening.member = str(w, "member"); if (widening.member.isEmpty()) widening.member = member.id;
            widening.role = member.role; widening.from = str(w, "from"); widening.to = str(w, "to");
            if (!widening.from.isEmpty() || !widening.to.isEmpty()) team.widenings.append(widening);
        }
    }
    if (v.value("profile_widening").isObject()) {
        const auto w = v.value("profile_widening").toObject();
        Team::Widening widening;
        widening.member = str(w, "member"); widening.role = str(w, "role");
        widening.from = str(w, "from"); widening.to = str(w, "to");
        if (!widening.from.isEmpty() || !widening.to.isEmpty()) team.widenings.append(widening);
    }
    for (const auto &value : v.value("profile_widening").toArray()) {
        const auto w = value.toObject();
        Team::Widening widening;
        widening.member = str(w, "member"); widening.role = str(w, "role");
        widening.from = str(w, "from"); widening.to = str(w, "to");
        if (!widening.from.isEmpty() || !widening.to.isEmpty()) team.widenings.append(widening);
    }
    for (const auto &value : v.value("tasks").toArray()) {
        const auto t = value.toObject();
        Task task;
        task.id = t.value("id").toInt(); task.phase = qMax(1, t.value("phase").toInt(1)); task.pinned = t.value("pinned").toBool();
        task.title = str(t, "title"); task.description = str(t, "description");
        task.status = str(t, "status"); task.assignee = str(t, "assignee");
        task.decidedBy = str(t, "decided_by"); task.reason = str(t, "decision_reason");
        for (const auto &c : t.value("criteria").toArray()) task.criteria.append(c.toString());
        for (const auto &d : t.value("deps").toArray()) task.deps.append({d.toObject().value("task").toInt(), str(d.toObject(), "kind") == "soft"});
        if (t.value("hold").isObject()) {
            // A5: parked by the lead or by you until `release`.
            const auto h = t.value("hold").toObject();
            task.held = true;
            task.hold.by = str(h, "by"); task.hold.reason = str(h, "reason"); task.hold.at = h.value("at").toDouble();
        }
        // C3: why an open task waits, and the command that unblocks it.
        if (t.value("waits").isObject()) {
            const auto w = t.value("waits").toObject();
            task.wait.known = true;
            task.wait.for_ = str(w, "for"); task.wait.unblock = str(w, "unblock");
            for (const auto &value : w.value("on").toArray()) task.wait.on.append(value.isDouble() ? QString::number(value.toInt()) : value.toString());
        }
        // C5: a check task and the exit its run reported.
        task.kind = str(t, "kind");
        if (t.value("check").isObject()) {
            const auto c = t.value("check").toObject();
            task.check.command = str(c, "command");
            task.check.expectExit = c.value("expect_exit").toInt();
            task.check.timeoutMinutes = c.value("timeout_minutes").toInt();
        }
        if (t.value("check_result").isObject()) {
            const auto r = t.value("check_result").toObject();
            task.checkResult.known = true;
            task.checkResult.exit = r.value("exit").toInt();
            task.checkResult.tail = str(r, "tail");
        }
        const auto readAttempt = [&](const QJsonObject &a) {
            Attempt attempt;
            attempt.n = a.value("n").toInt(); attempt.id = str(a, "id"); attempt.member = str(a, "member");
            attempt.state = str(a, "state"); attempt.error = str(a, "error"); attempt.notice = str(a, "notice");
            attempt.noReport = a.value("no_report").toBool(); attempt.sentAt = a.value("sent_at").toDouble();
            // N6: the check this attempt ran, from attempts[].check.
            if (a.value("check").isObject()) {
                const auto c = a.value("check").toObject();
                attempt.checkResult.known = true;
                attempt.checkResult.exit = c.value("exit").toInt();
                attempt.checkResult.tail = str(c, "tail");
            }
            // A1/A2: when it was delivered and started, and the turn that ended.
            attempt.deliveredAt = a.value("delivered_at").toDouble(); attempt.startedAt = a.value("started_at").toDouble();
            attempt.startWarned = a.value("start_warned").toBool();
            attempt.replyAt = a.value("reply_at").toDouble(); attempt.turnEndedAt = a.value("turn_ended_at").toDouble();
            attempt.turnReply = str(a, "turn_reply");
            // The key's presence is the contract: hgs sends null when the task
            // or the attempt has no phase, and an older hgs sends no key.
            attempt.phaseKnown = a.contains("phase");
            attempt.phase = str(a, "phase");
            const auto report = a.value("report").toObject();
            attempt.report = report.value("text").toString();
            attempt.reportAt = report.value("at").toDouble(); attempt.captured = report.value("captured").toBool();
            for (const auto &r : report.value("replaced").toArray()) {
                const auto item = r.toObject();
                attempt.replaced.append({item.value("text").toString(), item.value("at").toDouble()});
            }
            // A3: questions to the lead, answered or open.
            for (const auto &q : a.value("questions").toArray()) {
                const auto item = q.toObject();
                Question entry;
                entry.id = str(item, "id"); entry.text = str(item, "text"); entry.answer = str(item, "answer"); entry.by = str(item, "by");
                entry.at = item.value("at").toDouble(); entry.answeredAt = item.value("answered_at").toDouble();
                // B1: a permission request carries the exact command the agent
                // asked about, where it came from, and the agent's reason; the
                // answer records the decision, who made it and the deny text.
                entry.tool = str(item, "tool");
                entry.command = str(item, "command");
                entry.commandSource = str(item, "command_source");
                entry.reason = str(item, "reason");
                entry.decision = str(item, "decision");
                entry.hash = item.value("question_hash").toString(item.value("expected_question_hash").toString());
                attempt.questions.append(entry);
            }
            // B1: the permission answers kept on the attempt.
            for (const auto &value : a.value("permissions").toArray()) {
                const auto item = value.toObject();
                PermissionAnswer answer;
                answer.questionId = str(item, "question_id");
                answer.decision = str(item, "decision");
                answer.by = str(item, "by");
                answer.reason = str(item, "reason");
                answer.at = item.value("at").toDouble();
                attempt.permissions.append(answer);
            }
            if (attempt.member.isEmpty()) attempt.member = task.assignee;
            return attempt;
        };
        if (t.value("attempts").isArray()) for (const auto &a : t.value("attempts").toArray()) task.attempts.append(readAttempt(a.toObject()));
        else if (t.value("attempt").isObject()) task.attempts.append(readAttempt(t.value("attempt").toObject()));
        team.tasks.append(task);
    }
    // A5: the queue is in `team status`; an older hgs (or a list summary) gets
    // the same list from the attempts.
    for (auto &member : team.members) if (!member.queueKnown) member.queue = memberQueue(team, member.id);
    for (const auto &value : v.value("phases").toArray()) {
        const auto p = value.toObject();
        Phase phase;
        phase.n = p.value("n").toInt(); phase.title = str(p, "title"); phase.barrier = p.value("barrier").toBool();
        phase.approval = str(p, "approval"); phase.state = str(p, "state");
        phase.addedBy = str(p, "added_by"); phase.decidedBy = str(p, "decided_by");
        phase.addedAt = p.value("added_at").toDouble(); phase.decidedAt = p.value("decided_at").toDouble();
        for (const auto &id : p.value("tasks").toArray()) phase.tasks.append(id.toInt());
        // F10: the lead's diff for this draft, when the store reports one.
        for (const auto &value : p.value("changes").toArray()) {
            const auto entry = value.toObject();
            const auto text = [](const QJsonValue &value) {
                return value.isDouble() ? QString::number(value.toInt()) : value.toString();
            };
            PhaseChange change;
            change.kind = str(entry, "kind");
            change.task = entry.value("task").toInt();
            change.title = str(entry, "title");
            change.from = text(entry.value("from"));
            change.to = text(entry.value("to"));
            if (!change.kind.isEmpty()) phase.changes.append(change);
        }
        // F12: the draft's predecessor is not ready, with its open tasks.
        if (p.value("waiting").isObject()) {
            const auto w = p.value("waiting").toObject();
            phase.waiting.text = str(w, "text"); phase.waiting.phase = w.value("phase").toInt();
            for (const auto &id : w.value("open").toArray()) phase.waiting.open.append(id.toInt());
        }
        // F13: the owner's sentence the lead approved on.
        const auto word = p.value("owner_word");
        phase.ownerWord = word.isObject() ? str(word.toObject(), "text") : word.toString();
        if (phase.n < 1) continue;   // phase 0 and negative numbers collide with task ids
        team.phases.append(phase);
    }
    team.idle = str(v, "idle");
    if (v.value("finish").isObject()) {
        const auto f = v.value("finish").toObject();
        team.hasFinish = true;
        team.finish.by = str(f, "by"); team.finish.summary = str(f, "summary"); team.finish.at = f.value("at").toDouble();
    }
    // hgs derives both; a team file without them (an older hgs) gets the same
    // answer from the tasks and the phase records.
    team.waitingForLead = v.value("waiting_for_lead").toBool() || (team.status == "running" && allDecided(team));
    // Spec 10: several drafts may wait; they keep their order. A team that is
    // over shows none, even if an old record was left in draft.
    const bool over = team.status == "done" || team.status == "stopped" || team.status == "archived";
    if (!over) {
        team.pendingPhase = v.value("pending_phase").toInt();
        for (const auto &value : v.value("pending_phases").toArray()) if (value.toInt() > 0) team.pendingPhases.append(value.toInt());
        if (team.pendingPhases.isEmpty())
            for (const auto &phase : team.phases) if (phase.draft()) team.pendingPhases.append(phase.n);
        std::sort(team.pendingPhases.begin(), team.pendingPhases.end());
        if (!team.pendingPhase) team.pendingPhase = team.pendingPhases.isEmpty() ? 0 : team.pendingPhases.first();
    }
    // F7: the member sessions that still run after the team ended.
    team.leftRunningKnown = v.contains("left_running");
    for (const auto &value : v.value("left_running").toArray()) {
        const auto l = value.toObject();
        LeftRunning left;
        left.member = str(l, "member"); left.role = str(l, "role");
        const auto session = l.value("session").toObject();
        left.host = str(session, "host"); left.session = str(session, "name");
        left.runId = str(session, "run_id"); left.conversationId = str(session, "conversation_id");
        if (!left.session.isEmpty()) team.leftRunning.append(left);
    }
    if (v.contains("layers")) {
        team.full = true;
        const auto layers = v.value("layers").toObject();
        for (auto it = layers.begin(); it != layers.end(); ++it) team.layers.insert(it.key().toInt(), it.value().toInt());
        for (const auto &id : v.value("cycle").toArray()) team.cycle.insert(id.toInt());
        for (const auto &id : v.value("ready").toArray()) team.ready.append(id.toInt());
        const auto validation = v.value("validation").toObject();
        for (const auto &f : validation.value("errors").toArray()) team.errors.append(str(f.toObject(), "text"));
        for (const auto &f : validation.value("warnings").toArray()) team.warnings.append(str(f.toObject(), "text"));
        // C7: the plan-health numbers, with the starving members while running.
        // hgs puts them in validation.health; a bare health is accepted too.
        const auto h = validation.value("health").isObject() ? validation.value("health").toObject() : v.value("health").toObject();
        if (!h.isEmpty()) {
            team.health.known = true;
            team.health.tasks = h.value("tasks").toInt();
            team.health.criticalPath = h.value("critical_path").toInt();
            team.health.widestReady = h.value("widest_ready").toInt();
            team.health.parallelism = h.value("parallelism").toDouble();
            const auto members = h.value("per_member").toObject();
            for (auto it = members.begin(); it != members.end(); ++it) {
                const auto load = it.value().toObject();
                team.health.perMember.append({it.key(), {load.value("tasks").toInt(), load.value("on_critical_path").toInt()}});
            }
            for (const auto &value : h.value("fan_in").toArray()) {
                const auto entry = value.toObject();
                Team::Health::FanIn fanIn;
                fanIn.task = entry.value("task").toInt(); fanIn.kind = str(entry, "kind");
                for (const auto &id : entry.value("waits_for").toArray()) fanIn.waitsFor.append(id.toInt());
                team.health.fanIn.append(fanIn);
            }
            for (const auto &value : h.value("warnings").toArray()) team.health.warnings.append(value.toString());
            for (const auto &value : h.value("starved").toArray()) {
                const auto entry = value.toObject();
                Team::Health::Starved starved;
                starved.member = str(entry, "member"); starved.role = str(entry, "role");
                starved.minutes = entry.value("minutes").toInt();
                for (const auto &id : entry.value("blocking").toArray()) starved.blocking.append(id.toInt());
                team.health.starved.append(starved);
            }
        }
        // G9 (spec 10.2): the phase headers, in display order, phase 1 included.
        for (const auto &value : v.value("phase_summary").toArray()) {
            const auto entry = value.toObject();
            Team::PhaseSummary summary;
            summary.n = entry.value("n").toInt();
            summary.title = str(entry, "title");
            summary.status = str(entry, "status");
            summary.healthLine = str(entry, "health_line");
            summary.done = entry.value("done").toInt();
            summary.total = entry.value("total").toInt();
            summary.barrier = entry.value("barrier").toBool();
            if (summary.n > 0) team.phaseSummary.append(summary);
        }
        // G9 (spec 10.1): each phase's own plan health, keyed by phase number.
        const auto phaseHealth = v.value("phases_health").toObject();
        for (auto it = phaseHealth.begin(); it != phaseHealth.end(); ++it) {
            const int n = it.key().toInt();
            if (n <= 0 || !it.value().isObject()) continue;
            const auto health = parseHealth(it.value().toObject());
            if (health.known) team.phasesHealth.insert(n, health);
        }
    } else {
        team.layers = computeLayers(team);
        for (const auto &task : team.tasks) if (ready(team, task)) team.ready.append(task.id);
    }
    // C4: the undecided reports on the owner's attention list, also from the
    // polled team list.
    for (const auto &value : v.value("attention").toArray()) {
        const auto entry = value.toObject();
        Team::Attention attention;
        attention.kind = str(entry, "kind"); attention.task = entry.value("task").toInt();
        attention.member = str(entry, "member"); attention.since = entry.value("since").toDouble();
        attention.minutes = entry.value("minutes").toInt();
        team.attention.append(attention);
    }
    // F13: the plan's approval records the owner's sentence.
    team.approvedBy = str(v, "approved_by");
    const auto word = v.value("owner_word");
    team.ownerWord = word.isObject() ? str(word.toObject(), "text") : word.toString();
    return team;
}

QJsonObject Member::json() const
{
    QJsonObject o{{"id", id}, {"role", role}, {"agent", agent}, {"account", account}, {"model", model}, {"effort", effort},
                  {"permissions", permissions}, {"computer", computer}, {"directory", directory}, {"prompt", prompt}};
    if (!folder.isEmpty()) o.insert("folder", folder);
    if (!session.isEmpty()) {
        // F8: an adopted session keeps its identity, so hgs finds the same one.
        QJsonObject ref{{"host", host}, {"name", session}};
        if (!runId.isEmpty()) ref.insert("run_id", runId);
        if (!conversationId.isEmpty()) ref.insert("conversation_id", conversationId);
        o.insert("session", ref);
    }
    return o;
}

QJsonObject Task::json() const
{
    QJsonArray d, c;
    for (const auto &dep : deps) d.append(QJsonObject{{"task", dep.task}, {"kind", dep.soft ? "soft" : "hard"}});
    for (const auto &criterion : criteria) c.append(criterion);
    return {{"id", id}, {"title", title}, {"description", description}, {"criteria", c}, {"assignee", assignee}, {"deps", d}};
}

QJsonObject planInput(const Team &team)
{
    QJsonArray members, tasks;
    for (const auto &m : team.members) if (!m.lead) members.append(m.json());
    for (const auto &t : team.tasks) if (t.status != "cancelled") tasks.append(t.json());
    return {{"members", members}, {"tasks", tasks}};
}

int nextTaskId(const Team &team)
{
    int next = 1;
    for (const auto &t : team.tasks) next = qMax(next, t.id + 1);
    return next;
}

QString nextMemberId(const Team &team, const QString &role)
{
    QString base;
    for (const auto ch : role.toLower()) base += ch.isLetterOrNumber() && ch.unicode() < 128 ? ch : QChar('-');
    while (base.contains("--")) base.replace("--", "-");
    base = base.left(32).remove(QRegularExpression("^-+|-+$"));
    if (base.isEmpty()) base = QStringLiteral("member");
    QString id = base;
    for (int n = 2; team.member(id); ++n) id = QStringLiteral("%1-%2").arg(base).arg(n);
    return id;
}

QList<Team> parseList(const QJsonObject &value)
{
    QList<Team> teams;
    for (const auto &item : value.value("items").toArray()) teams.append(parse(item.toObject()));
    return teams;
}

bool ready(const Team &team, const Task &task)
{
    if (task.held) return false;   // A5: a held task is never ready
    if (task.status != "pending" || team.cycle.contains(task.id)) return false;
    for (const auto &dep : task.deps) {
        const auto *d = team.task(dep.task);
        if (!d) return false;
        const bool soft = dep.soft && !team.allHard;
        const bool ok = d->status == "accepted" || (soft && (d->status == "in_progress" || d->status == "submitted"));
        if (!ok) return false;
    }
    return true;
}

Status memberStatus(const Team &team, const Member &member, const Live &live)
{
    const auto make = [](const QString &kind, const QString &text, const QString &shortText, Tone tone, int task = 0, bool problem = false) {
        Status s; s.kind = kind; s.text = text; s.shortText = shortText; s.tone = tone; s.task = task; s.problem = problem; return s;
    };
    if (team.status == "planning")
        return member.session.isEmpty() ? make("not_created", tr("Created on approval"), tr("Planned"), Tone::Dim)
                                        : make("planning", tr("Planning"), tr("Planning"), Tone::Planning);
    if (!member.error.isEmpty()) return make("failed", tr("Failed to start"), tr("Failed"), Tone::Danger, 0, true);
    if (member.session.isEmpty()) return make("not_started", tr("Not started"), tr("Not started"), Tone::Dim);
    if (member.state == "paused") return make("paused", tr("Paused"), tr("Paused"), Tone::Muted);
    // The member's open attempt, if any.
    const Task *current = nullptr; const Attempt *attempt = nullptr;
    for (const auto &task : team.tasks)
        if (const auto *a = openOf(task); a && a->member == member.id) { current = &task; attempt = a; break; }
    if (live.offline) return make("offline", tr("Offline"), tr("Offline"), Tone::Danger, current ? current->id : 0, true);
    if (live.attention) return make("answer", tr("Needs your answer"), tr("Answer"), Tone::Warn, current ? current->id : 0, true);
    if (attempt && attempt->state == "unconfirmed")
        return make("unconfirmed", tr("Delivery unconfirmed"), tr("Unconfirmed"), Tone::Danger, current->id, true);
    if (attempt && attempt->noReport) return make("no_report", tr("No report"), tr("No report"), Tone::Warn, current->id, true);
    // A3: a question to the lead leaves the attempt open and pauses the work.
    if (attempt && attempt->openQuestion())
        return make("asking", tr("Waiting for your answer"), tr("Answer"), Tone::Warn, current->id, true);
    // QA: an attempt that only left for the member reads as sending, not as
    // work (its icon says the same).
    if (attempt && attempt->state == "delivering")
        return make("sent", tr("Sending #%1 %2").arg(current->id).arg(current->title), QStringLiteral("Sending"), Tone::Assigned, current->id);
    if (attempt)
        return make("working", tr("Working on #%1 %2").arg(current->id).arg(current->title), QStringLiteral("#%1").arg(current->id), Tone::Active, current->id);
    for (const auto &task : team.tasks)
        if (task.status == "submitted" && task.last() && task.last()->member == member.id)
            return make("submitted", tr("Submitted"), tr("Submitted"), Tone::Done, task.id);
    if (team.status == "done") return make("done", tr("Done"), tr("Done"), Tone::Done);
    if (live.working) return make("working", tr("Working"), tr("Working"), Tone::Active);
    return make("idle", tr("Idle"), tr("Idle"), Tone::Muted);
}

Status taskStatus(const Task &task)
{
    Status s; s.kind = task.status; s.task = task.id;
    const auto *open = openOf(task);
    const int n = task.last() ? task.last()->n : 0;
    const QString phase = lastPhase(task);
    if (open && open->state == "unconfirmed") { s.text = tr("Delivery unconfirmed"); s.tone = Tone::Danger; s.problem = true; }
    else if (open && open->noReport) { s.text = tr("No report"); s.tone = Tone::Warn; s.problem = true; }
    else if (task.held) { s.text = tr("Held"); s.tone = Tone::Muted; }
    // A1/A3: the attempt's own phase where it changes what the task is doing.
    else if (phase == "asking") { s.text = tr("Waiting for an answer"); s.tone = Tone::Warn; }
    else if (phase == "late") { s.text = tr("Not started"); s.tone = Tone::Warn; }
    else if (phase == "stopped") { s.text = tr("Stopped"); s.tone = Tone::Danger; }
    else if (task.status == "draft") { s.text = tr("Draft"); s.tone = Tone::Dim; }
    else if (task.status == "pending") { s.text = tr("Pending"); s.tone = Tone::Muted; }
    else if (task.status == "assigned") { s.text = tr("Assigned"); s.tone = Tone::Assigned; }
    else if (task.status == "in_progress") { s.text = tr("In progress"); s.tone = Tone::Active; }
    else if (task.status == "submitted") { s.text = tr("Submitted"); s.tone = Tone::Done; }
    else if (task.status == "accepted") { s.text = tr("Accepted"); s.tone = Tone::Done; }
    else if (task.status == "rejected") { s.text = tr("Rejected"); s.tone = Tone::Danger; }
    else if (task.status == "cancelled") { s.text = tr("Cancelled"); s.tone = Tone::Dim; }
    else s.text = task.status;
    s.shortText = s.text;
    if (n > 1 && !task.finished()) s.text += tr(" · attempt %1").arg(n);
    return s;
}

QList<Problem> problems(const Team &team, const LiveLookup &live)
{
    QList<Problem> list;
    for (const auto &member : team.members) {
        const auto status = memberStatus(team, member, live ? live(member) : Live{});
        if (!status.problem) continue;
        Problem p; p.kind = status.kind; p.member = member.id; p.task = status.task; p.tone = status.tone;
        const auto *task = team.task(status.task);
        p.text = task ? QStringLiteral("#%1 %2 — %3 · %4").arg(task->id).arg(task->title, team.roleOf(member.id), status.text)
                      : QStringLiteral("%1 · %2").arg(team.roleOf(member.id), status.text);
        list.append(p);
    }
    return list;
}

QString displayStatus(const Team &team, const LiveLookup &live)
{
    if (team.status != "running") return team.status;
    bool open = false;
    for (const auto &task : team.tasks) {
        if (task.finished() || task.status == "draft") continue;
        open = true;
        if (task.status == "submitted") return team.status;   // the lead decides
        const Member *assignee = team.member(task.assignee);
        const Live state = assignee && live ? live(*assignee) : Live{};
        const auto *attempt = openOf(task);
        if (attempt) {
            if (attempt->state != "unconfirmed" && !attempt->noReport && !state.offline && !state.attention) return team.status;
            continue;
        }
        if (task.assignee.isEmpty() && task.status == "in_progress") return team.status;   // taken over by you
        if (ready(team, task) && assignee && assignee->error.isEmpty() && assignee->state != "paused" && !state.offline)
            return team.status;
    }
    return open ? QStringLiteral("stuck") : team.status;
}

QString statusLabel(const QString &status)
{
    if (status == "planning") return tr("Planning");
    if (status == "running") return tr("Running");
    if (status == "paused") return tr("Paused");
    if (status == "stuck") return tr("Stuck");
    if (status == "done") return tr("Done");
    if (status == "stopped") return tr("Stopped");
    if (status == "archived") return tr("Archived");
    return status;
}

Tone statusTone(const QString &status)
{
    if (status == "planning") return Tone::Planning;
    if (status == "running") return Tone::Active;
    if (status == "paused") return Tone::Muted;
    if (status == "stuck") return Tone::Warn;
    if (status == "done") return Tone::Done;
    if (status == "stopped") return Tone::Danger;
    return Tone::Dim;
}

QString nextLine(const Team &team)
{
    if (team.status == "planning") return tr("Plan from Lead — review before running");
    QStringList readyIds;
    for (int id : team.ready) readyIds << QStringLiteral("#%1").arg(id);
    if (!readyIds.isEmpty()) return tr("Ready: %1").arg(readyIds.join(", "));
    // The first unfinished dependency that holds pending tasks back.
    QMap<int, QStringList> waiting;
    for (const auto &task : team.tasks) {
        if (task.status != "pending") continue;
        for (const auto &dep : task.deps)
            if (const auto *d = team.task(dep.task); d && d->status != "accepted") { waiting[d->id] << QStringLiteral("#%1").arg(task.id); break; }
    }
    if (!waiting.isEmpty()) {
        const auto *blocker = team.task(waiting.firstKey());
        return tr("Next: %1 wait for #%2 %3").arg(waiting.first().join(", ")).arg(blocker->id).arg(blocker->title);
    }
    bool open = false;
    for (const auto &task : team.tasks) open = open || (!task.finished() && task.status != "draft");
    return open ? QString() : tr("Every task is decided");
}

QList<Tone> progress(const Team &team)
{
    QList<Tone> tones;
    for (const auto &task : team.tasks) tones.append(taskStatus(task).tone);
    return tones;
}

int accepted(const Team &team)
{
    int n = 0;
    for (const auto &task : team.tasks) n += task.status == "accepted";
    return n;
}

bool allDecided(const Team &team)
{
    if (team.tasks.isEmpty()) return false;
    for (const auto &task : team.tasks) if (!task.finished()) return false;
    return true;
}

int answers(const Team &team, const LiveLookup &live)
{
    int n = 0;
    if (live)
        for (const auto &member : team.members) n += !member.session.isEmpty() && live(member).attention;
    // A3: a question asked with `hgs team ask` waits for the lead too.
    for (const auto &task : team.tasks) n += openQuestions(task).size();
    return n;
}

QSet<int> chain(const Team &team, int id)
{
    QSet<int> result{id};
    QList<int> pending{id};
    while (!pending.isEmpty()) {   // ancestors
        const auto *task = team.task(pending.takeLast());
        if (task) for (const auto &dep : task->deps) if (!result.contains(dep.task)) { result.insert(dep.task); pending.append(dep.task); }
    }
    QSet<int> down{id};
    pending = {id};
    while (!pending.isEmpty()) {   // descendants
        const int current = pending.takeLast();
        for (const auto &task : team.tasks)
            for (const auto &dep : task.deps)
                if (dep.task == current && !down.contains(task.id)) { down.insert(task.id); pending.append(task.id); }
    }
    return result.unite(down);
}

QMap<int, QList<int>> layered(const Team &team)
{
    QMap<int, QList<int>> layers;
    for (const auto &task : team.tasks) layers[team.layers.value(task.id) + 1].append(task.id);
    return layers;
}

QList<PhaseGroup> phaseGroups(const Team &team)
{
    QList<PhaseGroup> groups;
    if (!team.phased()) return groups;
    QMap<int, PhaseGroup> byPhase;
    for (const auto &task : team.tasks) {
        PhaseGroup &group = byPhase[task.phase];
        group.n = task.phase;
        group.tasks.append(task.id);
        group.done += task.finished();
    }
    // A team whose remaining tasks are all phase 1 looks as it did before:
    // grouping starts with the first task appended after the plan.
    bool appended = false;
    for (auto it = byPhase.cbegin(); it != byPhase.cend(); ++it) appended = appended || it.key() > 1;
    if (!appended) return groups;
    for (auto it = byPhase.cbegin(); it != byPhase.cend(); ++it) {
        PhaseGroup group = it.value();
        if (const auto *record = team.phase(it.key())) {
            group.title = record->title;
            group.barrier = record->barrier;
            group.draft = record->draft();
        }
        groups.append(group);
    }
    return groups;
}

QString PhaseProgress::tooltip() const
{
    if (!valid()) return {};
    // G8/spec 11: the tooltip names the phase the tasks bar counts.
    const QString name = title.isEmpty() ? tr("Phase %1").arg(phase) : tr("Phase %1 %2").arg(phase).arg(title);
    QString line = QStringLiteral("%1: %2/%3").arg(name).arg(done).arg(total);
    if (draft) line += QStringLiteral(" · ") + tr("waiting for your approval");
    return line + QLatin1Char('\n')
        + tr("%1 of %2 phases finished; discarded drafts do not count").arg(phasesDone).arg(phasesTotal);
}

PhaseProgress phaseProgress(const Team &team)
{
    const auto discarded = [&team](int phase) {
        const auto *record = team.phase(phase);
        return record && record->state == "discarded";
    };
    PhaseProgress progress;
    const auto groups = phaseGroups(team);
    for (const auto &group : groups) {
        if (discarded(group.n)) continue;   // a discarded draft is not planned
        ++progress.phasesTotal;
        progress.phasesDone += group.finished();
    }
    // The phase in work: the lowest non-draft phase that still has open tasks.
    for (const auto &group : groups) {
        if (discarded(group.n) || group.draft || group.finished()) continue;
        progress.phase = group.n; progress.title = group.title;
        progress.done = group.done; progress.total = group.tasks.size();
        break;
    }
    // Nothing in work: the next draft waiting for approval.
    if (!progress.phase) for (const auto &group : groups) {
        if (discarded(group.n) || !group.draft) continue;
        progress.phase = group.n; progress.title = group.title; progress.draft = true;
        progress.done = group.done; progress.total = group.tasks.size();
        break;
    }
    // Everything is decided and no draft waits: the last phase stands for the
    // finished plan, so the bar reads done/done.
    if (!progress.phase) for (const auto &group : groups) {
        if (discarded(group.n)) continue;
        progress.phase = group.n; progress.title = group.title;
        progress.done = group.done; progress.total = group.tasks.size();
    }
    // A team without phase records still gets the two bars: its plan is phase 1.
    if (!progress.phase && !team.tasks.isEmpty()) {
        int done = 0; for (const auto &task : team.tasks) done += task.finished();
        progress.phase = 1; progress.done = done; progress.total = team.tasks.size();
        progress.phasesTotal = 1; progress.phasesDone = done == team.tasks.size();
    }
    return progress;
}

QString phaseHeading(const PhaseGroup &group)
{
    const QString phase = group.title.isEmpty() ? tr("Phase %1").arg(group.n)
                                                : tr("Phase %1 · %2").arg(group.n).arg(group.title);
    return tr("%1 · %2/%3").arg(phase).arg(group.done).arg(group.tasks.size());
}

bool leftMember(const Team &team, const QString &member)
{
    return team.leftOf(member) != nullptr;
}

// C2: one thing on the lead's desk, as `hgs team inbox` lists it.
QList<InboxItem> parseInbox(const QJsonObject &value)
{
    QList<InboxItem> items;
    for (const auto &entry : value.value("items").toArray()) {
        const auto item = entry.toObject();
        InboxItem inbox;
        inbox.kind = str(item, "kind"); inbox.member = str(item, "member");
        inbox.summary = str(item, "summary"); inbox.command = str(item, "command");
        inbox.task = item.value("task").toInt(); inbox.since = item.value("since").toDouble();
        items.append(inbox);
    }
    return items;
}

namespace {
// Why phase N may not be approved yet: the nearest lower phase that is still a
// draft or unapproved, or the nearest one with open tasks.
struct PhaseBlocker { int phase = 0; bool approved = false; int open = 0; };
PhaseBlocker nearestBlocker(const Team &team, int phase)
{
    for (int lower = phase - 1; lower >= 1; --lower) {
        const auto *previous = team.phase(lower);
        // A discarded phase is out of the plan, and phase 1 has no record: the
        // approved initial plan is approved by definition.
        if (previous && previous->state == "discarded") continue;
        if (previous && (previous->draft() || previous->state != "approved"))
            return {lower, false, 0};
        int open = 0;
        for (const auto &task : team.tasks) if (task.phase == lower && !task.finished()) ++open;
        if (open) return {lower, true, open};
    }
    return {};
}
}

QString approveBlocked(const Team &team, int phase)
{
    const auto *record = team.phase(phase);
    if (!record || !record->draft()) return tr("Phase %1 is not a draft").arg(phase);
    // QA: name the nearest phase that still has to open or end, not the first
    // one above the plan: for drafts 2, 3 and 4, phase 4 waits for phase 3.
    const auto blocker = nearestBlocker(team, phase);
    if (!blocker.phase) return {};
    if (!blocker.approved) return tr("Phase %1 opens after phase %2 is approved").arg(phase).arg(blocker.phase);
    return QCoreApplication::translate("Teams", "Phase %1 opens after phase %2 ends: %n task(s) still open", nullptr, blocker.open)
        .arg(phase).arg(blocker.phase);
}

// The same reason, short enough for the compact draft row; the full sentence
// stays in the row's tooltip.
QString approveBlockedShort(const Team &team, int phase)
{
    const auto *record = team.phase(phase);
    if (!record || !record->draft()) return {};
    const auto blocker = nearestBlocker(team, phase);
    if (!blocker.phase) return {};
    if (!blocker.approved) return tr("after phase %1").arg(blocker.phase);
    return QCoreApplication::translate("Teams", "after phase %1 ends · %n open", nullptr, blocker.open).arg(blocker.phase);
}

QMap<int, int> phaseLayers(const Team &team, const QList<int> &tasks)
{
    QSet<int> ids;
    for (int id : tasks) ids.insert(id);
    QMap<int, int> depth;
    QSet<int> active;
    std::function<int(int)> visit = [&](int id) -> int {
        if (depth.contains(id)) return depth[id];
        if (active.contains(id)) return -1;
        active.insert(id);
        int level = 0;
        if (const auto *task = team.task(id))
            for (const auto &dep : task->deps)
                if (ids.contains(dep.task)) { const int d = visit(dep.task); if (d >= 0) level = qMax(level, d + 1); }
        active.remove(id);
        depth[id] = level;
        return level;
    };
    for (int id : tasks) visit(id);
    return depth;
}

FeedLine feedLine(const Team &team, const QJsonObject &event)
{
    FeedLine line;
    line.at = event.value("at").toDouble(); line.seq = event.value("seq").toInteger(); line.kind = str(event, "kind");
    const auto d = event.value("detail").toObject();
    line.by = d.value("by").toString(d.value("member").toString());
    const int id = d.value("task").toInt();
    const auto *task = team.task(id);
    const QString title = d.value("title").toString(task ? task->title : QString());
    const QString ref = id ? (title.isEmpty() ? QStringLiteral("#%1").arg(id) : QStringLiteral("#%1 %2").arg(id).arg(title)) : QString();
    const QString attempt = d.value("attempt").isDouble() ? tr(" · attempt %1").arg(d.value("attempt").toInt()) : QString();
    const QString k = line.kind;
    auto &t = line.text;
    if (k == "created") t = tr("Created the team");
    else if (k == "planned") t = d.value("errors").toInt() ? tr("Changed the plan · %1 errors").arg(d.value("errors").toInt()) : tr("Changed the plan · %1 tasks").arg(d.value("tasks").toInt());
    else if (k == "approved") t = tr("Approved the plan");
    else if (k == "discarded") t = tr("Discarded the plan");
    else if (k == "paused") t = tr("Paused the team");
    else if (k == "resumed") t = tr("Resumed the team");
    else if (k == "stopped") { t = tr("Stopped the team"); line.tone = Tone::Danger; }
    else if (k == "archived") t = tr("Archived the team");
    else if (k == "done") { t = tr("Done · %1 of %2 tasks accepted").arg(d.value("accepted").toInt()).arg(d.value("tasks").toInt()); line.tone = Tone::Done; }
    else if (k == "phase_added") {
        const int n = d.value("phase").toInt();
        const QString title = d.value("title").toString();
        t = title.isEmpty() ? tr("Added phase %1 · %2 tasks").arg(n).arg(d.value("tasks").toInt())
                            : tr("Added phase %1 · %2 · %3 tasks").arg(n).arg(title).arg(d.value("tasks").toInt());
    }
    else if (k == "phase_approved") t = tr("Approved phase %1 · %2 tasks").arg(d.value("phase").toInt()).arg(d.value("tasks").toInt());
    else if (k == "phase_moved") {
        // F10: the drafts are numbered live; a move renumbers them, and the
        // feed says where the phase went.
        // hgs names the moved phase "phase"; "from" is the older test shape.
        const int moved = d.contains("phase") ? d.value("phase").toInt() : d.value("from").toInt();
        t = d.value("renumbered").toBool() ? tr("Moved phase %1 to position %2 · drafts renumbered").arg(moved).arg(d.value("to").toInt())
                                           : tr("Moved phase %1 to position %2").arg(moved).arg(d.value("to").toInt());
    }
    else if (k == "phase_discarded") {
        // A discarded draft lists its tasks' titles; older events counted them.
        const QJsonValue tasks = d.value("tasks");
        t = tr("Discarded phase %1 · %2 tasks").arg(d.value("phase").toInt()).arg(tasks.isArray() ? tasks.toArray().size() : tasks.toInt());
        line.tone = Tone::Muted;
    }
    else if (k == "phase_renumbered") {
        // A discard or move closes the gap: the drafts after it take new numbers.
        QStringList moves;
        for (const auto &value : d.value("phases").toArray())
            moves << QStringLiteral("%1→%2").arg(value.toObject().value("from").toInt()).arg(value.toObject().value("to").toInt());
        t = moves.isEmpty() ? tr("Drafts renumbered") : tr("Drafts renumbered: phases %1").arg(moves.join(QStringLiteral(", ")));
        line.tone = Tone::Muted;
    }
    else if (k == "all_decided") { t = tr("All tasks decided · waiting for the lead"); line.tone = Tone::Active; }
    else if (k == "finished") { t = tr("Finished · %1 of %2 tasks accepted").arg(d.value("accepted").toInt()).arg(d.value("tasks").toInt()); line.tone = Tone::Done; }
    else if (k == "lead_told" && d.value("kind").toString() == "all_decided") t = tr("Told the Lead that all tasks are decided");
    else if (k == "handed_out") { line.by = tr("Lead"); t = tr("Task to %1 · %2%3").arg(d.value("to").toString(), ref, attempt); }
    else if (k == "delivered") t = tr("Delivered %1%2").arg(ref, attempt);
    else if (k == "started") t = tr("Started %1%2").arg(ref, attempt);
    else if (k == "start_late") { t = tr("Delivered and not started yet · %1%2").arg(ref, attempt); line.tone = Tone::Warn; }
    else if (k == "refused") { t = tr("Not delivered %1%2 (%3); handed out again later").arg(ref, attempt, d.value("error").toString()); line.tone = Tone::Warn; }
    else if (k == "unconfirmed") { t = tr("Delivery unconfirmed · %1%2 (%3)").arg(ref, attempt, d.value("error").toString()); line.tone = Tone::Danger; }
    else if (k == "reported") t = tr("Report to Lead · %1%2").arg(ref, attempt);
    else if (k == "report_replaced") t = tr("Replaced the captured reply · %1%2").arg(ref, attempt);
    else if (k == "question_asked") {
        const QString text = d.value("text").toString();
        t = text.isEmpty() ? tr("A question for the Lead · %1%2").arg(ref, attempt)
                           : tr("Question from the member · %1%2: %3").arg(ref, attempt, text);
        line.tone = Tone::Warn;
    }
    else if (k == "question_answered") t = tr("Answered the question · %1%2").arg(ref, attempt);
    else if (k == "held") {
        const QString reason = d.value("reason").toString();
        t = reason.isEmpty() ? tr("Held %1%2").arg(ref, attempt) : tr("Held %1%2: %3").arg(ref, attempt, reason);
        line.tone = Tone::Muted;
    }
    else if (k == "released") t = tr("Released %1%2").arg(ref, attempt);
    else if (k == "member_adopted") t = tr("Adopted the existing session · %1").arg(d.value("session").toString());
    else if (k == "stale_report") { t = tr("Late report refused · %1%2").arg(ref, attempt); line.tone = Tone::Warn; }
    else if (k == "accepted") { t = tr("Accepted %1").arg(ref); line.tone = Tone::Done; }
    else if (k == "rejected") { t = tr("Rejected %1%2: “%3”").arg(ref, attempt, d.value("reason").toString()); line.tone = Tone::Danger; }
    else if (k == "reassigned") t = tr("Reassigned %1 to %2").arg(ref, d.value("to").toString());
    else if (k == "retried") t = tr("Retried %1").arg(ref);
    else if (k == "taken_over") t = tr("Took over %1").arg(ref);
    else if (k == "cancelled") t = tr("Cancelled %1").arg(ref);
    else if (k == "checked") t = d.value("delivered").toBool() ? tr("Confirmed delivery of %1%2").arg(ref, attempt) : tr("Marked %1%2 as not delivered").arg(ref, attempt);
    else if (k == "no_report") { t = tr("Turn ended without a report · %1%2").arg(ref, attempt); line.tone = Tone::Warn; }
    else if (k == "notice_unconfirmed") { t = tr("Report for %1 may not have reached the Lead").arg(ref); line.tone = Tone::Warn; }
    else if (k == "member_started") t = tr("Session started · %1").arg(d.value("session").toString());
    else if (k == "launch_failed") { t = tr("Session failed to start: %1").arg(d.value("error").toString()); line.tone = Tone::Danger; }
    else if (k == "relaunch") t = tr("Start %1 again").arg(d.value("member").toString());
    else if (k == "member_paused") t = tr("Paused %1").arg(d.value("member").toString());
    else if (k == "member_resumed") t = tr("Resumed %1").arg(d.value("member").toString());
    else if (k == "member_removed") t = tr("Removed %1 from the team").arg(d.value("member").toString());
    else t = k;
    return line;
}


Role parseRole(const QJsonObject &v)
{
    Role r;
    r.id = str(v, "id"); r.name = str(v, "name"); r.color = str(v, "color"); r.prompt = str(v, "prompt");
    r.agent = str(v, "agent"); r.account = str(v, "account"); r.model = str(v, "model"); r.effort = str(v, "effort");
    r.permissions = str(v, "permissions"); r.computer = str(v, "computer"); r.folder = str(v, "folder");
    r.permissionsProfile = str(v, "permissions_profile");   // B5: read, write, build, commit, network
    r.canMessage = str(v, "can_message"); r.dangerous = v.value("dangerous").toBool();
    for (const auto &tag : v.value("tags").toArray()) r.tags.append(tag.toString());
    return r;
}

QJsonObject Role::json() const
{
    return {{"id", id}, {"name", name}, {"color", color}, {"prompt", prompt}, {"agent", agent}, {"account", account},
            {"model", model}, {"effort", effort}, {"permissions", permissions}, {"computer", computer}, {"folder", folder},
            {"can_message", canMessage}, {"dangerous", dangerous}, {"tags", QJsonArray::fromStringList(tags)}};
}

Template parseTemplate(const QJsonObject &v)
{
    Template t;
    t.id = str(v, "id"); t.name = str(v, "name"); t.description = str(v, "description"); t.leadProtocol = str(v, "lead_protocol");
    t.planning = str(v, "planning"); t.approval = str(v, "approval"); t.phaseApproval = str(v, "phase_approval");
    t.maxMembers = v.value("max_members").toInt(6);
    for (const auto &seat : v.value("roster").toArray()) {
        const auto o = seat.toObject();
        t.roster.append({str(o, "role"), o.value("count").toInt(1), o.value("optional").toBool()});
    }
    for (const auto &task : v.value("tasks").toArray()) {
        const auto o = task.toObject();
        SkeletonTask k{str(o, "title"), str(o, "role"), {}};
        for (const auto &d : o.value("deps").toArray()) k.deps.append(d.toInt());
        t.tasks.append(k);
    }
    return t;
}

QJsonObject Template::json() const
{
    QJsonArray seats, skeleton;
    for (const auto &s : roster) seats.append(QJsonObject{{"role", s.role}, {"count", s.count}, {"optional", s.optional}});
    for (const auto &k : tasks) {
        QJsonArray deps; for (int d : k.deps) deps.append(d);
        skeleton.append(QJsonObject{{"title", k.title}, {"role", k.role}, {"deps", deps}});
    }
    return {{"id", id}, {"name", name}, {"description", description}, {"lead_protocol", leadProtocol}, {"roster", seats},
            {"tasks", skeleton}, {"planning", planning}, {"approval", approval}, {"phase_approval", phaseApproval},
            {"max_members", maxMembers}};
}

Rule parseRule(const QJsonObject &v) { return {str(v, "id"), str(v, "when"), str(v, "template"), str(v, "approval")}; }

QJsonObject Rule::json() const { return {{"id", id}, {"when", when}, {"template", templateId}, {"approval", approval}}; }

const Role *Catalog::role(const QString &id) const
{
    for (const auto &r : roles) if (r.id == id) return &r;
    return nullptr;
}

const Role *Catalog::roleNamed(const QString &name) const
{
    for (const auto &r : roles)
        if (r.id.compare(name, Qt::CaseInsensitive) == 0 || r.name.compare(name, Qt::CaseInsensitive) == 0) return &r;
    return nullptr;
}

const Template *Catalog::templ(const QString &id) const
{
    for (const auto &t : templates) if (t.id == id) return &t;
    return nullptr;
}

QJsonObject Catalog::json() const
{
    QJsonArray r, t, u;
    for (const auto &x : roles) r.append(x.json());
    for (const auto &x : templates) t.append(x.json());
    for (const auto &x : routing) u.append(x.json());
    return {{"version", version}, {"roles", r}, {"templates", t}, {"routing", u}};
}

Catalog parseCatalog(const QJsonObject &get)
{
    Catalog c;
    const auto catalog = get.value("catalog").toObject();
    c.version = catalog.value("version").toInteger(); c.builtin = get.value("builtin").toBool();
    for (const auto &v : catalog.value("roles").toArray()) c.roles.append(parseRole(v.toObject()));
    for (const auto &v : catalog.value("templates").toArray()) c.templates.append(parseTemplate(v.toObject()));
    for (const auto &v : catalog.value("routing").toArray()) c.routing.append(parseRule(v.toObject()));
    const auto draft = get.value("draft").toObject();
    c.draftBase = draft.value("base").toInteger();
    for (const auto &v : draft.value("proposals").toArray()) {
        const auto o = v.toObject();
        c.proposals.append({str(o, "id"), str(o, "kind"), str(o, "action"), str(o, "target"), str(o, "title"), str(o, "reason"),
                            o.value("state").toString(QStringLiteral("proposed")), o.value("value").toObject(), o.value("at").toDouble()});
    }
    return c;
}

Catalog withProposals(const Catalog &catalog, bool includeUndecided)
{
    Catalog c = catalog;
    for (const auto &p : catalog.proposals) {
        if (p.state == "rejected" || (!includeUndecided && p.state != "kept")) continue;
        const bool remove = p.action == "remove";
        const auto apply = [&](auto &items, const auto &parsed) {
            int at = -1;
            for (int i = 0; i < items.size(); ++i) if (items[i].id == p.target) { at = i; break; }
            if (remove) { if (at >= 0) items.removeAt(at); }
            else if (at >= 0) items[at] = parsed;
            else items.append(parsed);
        };
        if (p.kind == "role") apply(c.roles, parseRole(p.value));
        else if (p.kind == "template") apply(c.templates, parseTemplate(p.value));
        else if (p.kind == "rule") apply(c.routing, parseRule(p.value));
    }
    return c;
}

}
