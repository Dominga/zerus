#include "TeamModel.h"
#include <QCoreApplication>
#include <QDateTime>
#include <QJsonValue>
#include <QRegularExpression>
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

QString Team::roleOf(const QString &id) const
{
    if (id == lead) return tr("Lead");
    if (const auto *m = member(id)) return m->role.isEmpty() ? m->id : m->role;
    return id;
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
        member.lead = member.id == team.lead;
        if (member.lead && member.role.isEmpty()) member.role = tr("Lead");
        team.members.append(member);
    }
    for (const auto &value : v.value("tasks").toArray()) {
        const auto t = value.toObject();
        Task task;
        task.id = t.value("id").toInt(); task.title = str(t, "title"); task.description = str(t, "description");
        task.status = str(t, "status"); task.assignee = str(t, "assignee");
        task.decidedBy = str(t, "decided_by"); task.reason = str(t, "decision_reason");
        for (const auto &c : t.value("criteria").toArray()) task.criteria.append(c.toString());
        for (const auto &d : t.value("deps").toArray()) task.deps.append({d.toObject().value("task").toInt(), str(d.toObject(), "kind") == "soft"});
        const auto readAttempt = [&](const QJsonObject &a) {
            Attempt attempt;
            attempt.n = a.value("n").toInt(); attempt.id = str(a, "id"); attempt.member = str(a, "member");
            attempt.state = str(a, "state"); attempt.error = str(a, "error"); attempt.notice = str(a, "notice");
            attempt.noReport = a.value("no_report").toBool(); attempt.sentAt = a.value("sent_at").toDouble();
            attempt.report = a.value("report").toObject().value("text").toString();
            if (attempt.member.isEmpty()) attempt.member = task.assignee;
            return attempt;
        };
        if (t.value("attempts").isArray()) for (const auto &a : t.value("attempts").toArray()) task.attempts.append(readAttempt(a.toObject()));
        else if (t.value("attempt").isObject()) task.attempts.append(readAttempt(t.value("attempt").toObject()));
        team.tasks.append(task);
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
    } else {
        team.layers = computeLayers(team);
        for (const auto &task : team.tasks) if (ready(team, task)) team.ready.append(task.id);
    }
    return team;
}

QJsonObject Member::json() const
{
    QJsonObject o{{"id", id}, {"role", role}, {"agent", agent}, {"account", account}, {"model", model}, {"effort", effort},
                  {"permissions", permissions}, {"computer", computer}, {"directory", directory}, {"prompt", prompt}};
    if (!folder.isEmpty()) o.insert("folder", folder);
    if (!session.isEmpty()) o.insert("session", QJsonObject{{"host", host}, {"name", session}});
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
    if (open && open->state == "unconfirmed") { s.text = tr("Delivery unconfirmed"); s.tone = Tone::Danger; s.problem = true; }
    else if (open && open->noReport) { s.text = tr("No report"); s.tone = Tone::Warn; s.problem = true; }
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

int answers(const Team &team, const LiveLookup &live)
{
    int n = 0;
    if (!live) return 0;
    for (const auto &member : team.members) n += !member.session.isEmpty() && live(member).attention;
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
    else if (k == "handed_out") { line.by = tr("Lead"); t = tr("Task to %1 · %2%3").arg(d.value("to").toString(), ref, attempt); }
    else if (k == "delivered") t = tr("Delivered %1%2").arg(ref, attempt);
    else if (k == "refused") { t = tr("Not delivered %1%2 (%3); handed out again later").arg(ref, attempt, d.value("error").toString()); line.tone = Tone::Warn; }
    else if (k == "unconfirmed") { t = tr("Delivery unconfirmed · %1%2 (%3)").arg(ref, attempt, d.value("error").toString()); line.tone = Tone::Danger; }
    else if (k == "reported") t = tr("Report to Lead · %1%2").arg(ref, attempt);
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
    t.planning = str(v, "planning"); t.approval = str(v, "approval"); t.maxMembers = v.value("max_members").toInt(6);
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
            {"tasks", skeleton}, {"planning", planning}, {"approval", approval}, {"max_members", maxMembers}};
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
