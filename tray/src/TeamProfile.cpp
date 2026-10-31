#include "TeamProfile.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include "TeamTests.h"
#include <QComboBox>
#include <QFile>
#include <QFileDialog>
#include <QGridLayout>
#include <QHeaderView>
#include <QJsonDocument>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QScrollArea>
#include <QScrollBar>
#include <QTableWidget>
#include <QToolTip>
#include <algorithm>
#include <climits>
#include <cmath>

using namespace TeamUi;

namespace TeamProfile {

namespace {
const QStringList Busy{"work", "rework", "stage", "question", "permission", "background"};
const QStringList Waits{"deps", "member", "lead", "delivery", "start_delay", "folder_held", "hold", "stage_wait"};

QList<std::pair<double, double>> unite(QList<std::pair<double, double>> list)
{
    std::sort(list.begin(), list.end());
    QList<std::pair<double, double>> out;
    for (const auto &item : list) {
        if (!out.isEmpty() && item.first <= out.last().second) out.last().second = std::max(out.last().second, item.second);
        else out << item;
    }
    return out;
}

QList<std::pair<double, double>> minus(const QList<std::pair<double, double>> &open, const QList<std::pair<double, double>> &busy)
{
    QList<std::pair<double, double>> out;
    for (auto [from, to] : open) {
        for (const auto &[low, high] : busy) {
            if (high <= from || low >= to) continue;
            if (low > from) out << std::pair{from, low};
            from = std::max(from, high);
            if (from >= to) break;
        }
        if (to > from) out << std::pair{from, to};
    }
    return out;
}

double length(const QList<std::pair<double, double>> &list)
{
    double sum = 0;
    for (const auto &[from, to] : list) sum += to - from;
    return sum;
}
}

QString duration(qint64 seconds)
{
    if (seconds < 0) return QObject::tr("unknown");
    if (seconds < 60) return QObject::tr("%1s").arg(seconds);
    const qint64 minutes = seconds / 60;
    if (minutes < 60) return QObject::tr("%1m").arg(minutes);
    return QObject::tr("%1h %2m").arg(minutes / 60).arg(minutes % 60, 2, 10, QLatin1Char('0'));
}

QString tokens(qint64 count)
{
    if (count >= 1000000) return QStringLiteral("%1M").arg(count / 1e6, 0, 'f', 2);
    if (count >= 1000) return QStringLiteral("%1k").arg(qRound64(count / 1e3));
    return QString::number(count);
}

QColor kindColor(const QString &kind, const QString &stageKind, bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)); };
    if (kind == "work") return dark ? QColor("#7dd3b0") : QColor("#3f9f7b");
    if (kind == "rework") return t("warn");
    if (kind == "build") return dark ? QColor("#56c8e0") : QColor("#137f95");
    if (kind == "stage") {
        if (stageKind == "review") return t("violet");
        if (stageKind == "test") return t("folder");
        return dark ? QColor("#56c8e0") : QColor("#137f95");
    }
    if (kind == "question" || kind == "permission" || kind == "stop" || kind == "no_report") return t("danger");
    if (kind == "background") return dark ? QColor("#6fa8ff") : QColor("#2f66c9");
    if (kind == "member") return t("folder");
    if (kind == "lead") return t("violet");
    if (kind == "delivery" || kind == "start_delay") return t("muted");
    return t("dim");
}

QString kindName(const QString &kind, const QString &stageKind)
{
    if (kind == "work") return QObject::tr("Work");
    if (kind == "rework") return QObject::tr("Rework (a returned attempt)");
    if (kind == "build") return QObject::tr("Build and merge");
    if (kind == "stage") return stageKind == "review" ? QObject::tr("Review stage") : stageKind == "test" ? QObject::tr("Test stage") : QObject::tr("Stage: %1").arg(stageKind);
    if (kind == "question") return QObject::tr("Blocked on a question");
    if (kind == "permission") return QObject::tr("Blocked on a permission");
    if (kind == "background") return QObject::tr("Background work");
    if (kind == "delivery") return QObject::tr("Delivery");
    if (kind == "start_delay") return QObject::tr("Start delay");
    if (kind == "member") return QObject::tr("Waited for a free member");
    if (kind == "deps") return QObject::tr("Waited for dependencies");
    if (kind == "lead") return QObject::tr("Waited for the lead");
    if (kind == "folder_held") return QObject::tr("Folder held by another task");
    if (kind == "hold") return QObject::tr("Held");
    if (kind == "stage_wait") return QObject::tr("Waited for a stage");
    if (kind == "no_report") return QObject::tr("No report");
    if (kind == "stop") return QObject::tr("Stopped");
    return QObject::tr("Not recorded");
}

Model Model::build(const Teams::Team &team, const QJsonObject &profile, const QJsonArray &timelines, double now, const QJsonArray &tests)
{
    Model model;
    model.profile = profile;
    const auto summary = profile.value("summary").toObject();
    model.from = summary.value("from").toDouble(); model.to = summary.value("to").toDouble();
    for (const auto &task : team.tasks) { model.titles.insert(task.id, task.title); model.assignees.insert(task.id, task.assignee); }
    QHash<QString, QList<std::pair<double, double>>> busy;
    QHash<QString, QList<Span>> spans, waits;
    QHash<int, std::pair<double, double>> extents, worked;
    QHash<int, double> busySeconds;
    QList<std::pair<double, double>> open;
    double low = model.from > 0 ? model.from : 1e300, high = model.to > 0 ? model.to : 0;
    for (const auto &value : timelines) {
        const auto timeline = value.toObject();
        const int task = timeline.value("task").toInt();
        ++model.tasks;
        double first = 1e300, last = 0;
        bool running = false;
        const auto add = [&](const QJsonObject &raw, const QString &forcedKind) {
            if (raw.value("from").isNull() || !raw.contains("from")) return;
            Span span;
            span.task = task; span.kind = forcedKind.isEmpty() ? raw.value("kind").toString() : forcedKind;
            span.member = raw.value("member").toString(); span.stageKind = raw.value("stage_kind").toString();
            span.attempt = raw.value("attempt").toInt();
            span.from = raw.value("from").toDouble();
            span.running = raw.value("to").isNull();
            span.to = span.running ? now : raw.value("to").toDouble();
            if (span.to <= span.from) return;
            first = std::min(first, span.from); last = std::max(last, span.to);
            if (Busy.contains(span.kind)) {
                // #746: the work itself, as the estimate counts it (no question waits).
                if (span.kind == QLatin1String("work") || span.kind == QLatin1String("rework")) { busySeconds[task] += span.seconds(); running = running || span.running; }
                auto &at = worked[task];
                at = at.second > 0 ? std::pair{std::min(at.first, span.from), std::max(at.second, span.to)} : std::pair{span.from, span.to};
            }
            if (Busy.contains(span.kind) || span.kind == "build") {
                const QString who = span.member.isEmpty() ? model.assignees.value(task) : span.member;
                spans[who] << span;
                if (Busy.contains(span.kind)) busy[who] << std::pair{span.from, span.to};
            } else {
                // A wait shows under the lane of the member the task was given.
                waits[model.assignees.value(task)] << span;
            }
        };
        for (const auto &span : timeline.value("spans").toArray()) add(span.toObject(), {});
        for (const auto &build : timeline.value("overlay").toArray()) add(build.toObject(), QStringLiteral("build"));
        if (last > 0) { extents.insert(task, {first, last}); open << std::pair{first, last}; low = std::min(low, first); high = std::max(high, last); }
        // #746: the estimate #745 puts on each timeline.
        if (timeline.contains("estimate_source")) {
            Estimate row; row.task = task; row.running = running; row.worked = busySeconds.value(task);
            row.estimate = timeline.value("estimate_minutes").isDouble() ? timeline.value("estimate_minutes").toDouble() : -1;
            if (row.estimate > 0 || row.worked > 0) model.estimates << row;
        }
    }
    if (high > 0) { model.from = low; model.to = high; }
    open = unite(open);
    // Lanes: the members the profile lists, in its order, then the lead.
    QJsonArray members = profile.value("members").toArray();
    QSet<QString> listed;
    for (const auto &value : std::as_const(members)) listed.insert(value.toObject().value("member").toString());
    if (!team.lead.isEmpty() && !listed.contains(team.lead) && (spans.contains(team.lead) || waits.contains(team.lead)))
        members.append(QJsonObject{{"member", team.lead}, {"role", team.roleOf(team.lead)}});
    for (const auto &value : std::as_const(members)) {
        const auto row = value.toObject();
        Lane lane;
        lane.member = row.value("member").toString();
        lane.role = row.value("role").toString(team.roleOf(lane.member));
        lane.model = row.value("model").toString();
        lane.utilization = row.value("utilization").toDouble();
        lane.busy = qint64(row.value("busy_s").toDouble());
        lane.spans = spans.take(lane.member);
        lane.waits = waits.take(lane.member);
        std::sort(lane.spans.begin(), lane.spans.end(), [](const Span &a, const Span &b) { return a.from < b.from; });
        // The moments it had no work while a task was open.
        lane.idle = minus(open, unite(busy.value(lane.member)));
        lane.idleSeconds = row.contains("idle_s") ? qint64(row.value("idle_s").toDouble()) : qint64(length(lane.idle));
        model.lanes << lane;
    }
    // The critical paths: the real one as each task's extent, the planned one
    // end to end at each task's first attempt.
    const auto critical = profile.value("critical_path").toObject();
    // Each task of the real path from its first to its last worked moment.
    for (const auto &id : critical.value("real").toObject().value("tasks").toArray()) {
        if (!worked.contains(id.toInt())) continue;
        Span span; span.task = id.toInt(); span.kind = QStringLiteral("work");
        span.from = worked.value(span.task).first; span.to = worked.value(span.task).second;
        model.real << span;
    }
    std::sort(model.real.begin(), model.real.end(), [](const Span &a, const Span &b) { return a.from < b.from; });
    model.realSeconds = qint64(critical.value("real").toObject().value("s").toDouble());
    double at = model.from;
    for (const auto &id : critical.value("planned").toObject().value("tasks").toArray()) {
        // Its first attempt as it went, returned or not.
        int lowest = INT_MAX;
        for (const auto &lane : std::as_const(model.lanes))
            for (const auto &span : lane.spans) if (span.task == id.toInt() && span.kind != "build") lowest = std::min(lowest, span.attempt);
        double first = 0;
        for (const auto &lane : std::as_const(model.lanes))
            for (const auto &span : lane.spans) if (span.task == id.toInt() && span.attempt == lowest && span.kind != "build") first += span.seconds();
        if (first <= 0) continue;
        Span span; span.task = id.toInt(); span.kind = QStringLiteral("work"); span.from = at; span.to = at + first;
        at = span.to;
        model.planned << span;
        model.plannedSeconds += qint64(first);
    }
    // U62 (#719): the window's test runs, from the team's test database.
    for (const auto &value : tests) {
        const auto record = value.toObject();
        Span run; run.kind = QStringLiteral("tests"); run.task = record.value("task").toInt();
        run.record = record.value("id").toString(); run.member = record.value("suite").toString();
        run.stageKind = record.value("status").toString();
        run.from = record.value("at").toDouble(); run.to = run.from + std::max(1.0, record.value("duration_ms").toDouble() / 1000);
        if (model.to > model.from && (run.to < model.from || run.from > model.to)) continue;
        model.tests << run;
    }
    std::sort(model.tests.begin(), model.tests.end(), [](const Span &a, const Span &b) { return a.from < b.from; });
    return model;
}

Cache::Cache(TeamController *teams, QObject *parent) : QObject(parent), m_teams(teams) {}

void Cache::touch(const QString &key)
{
    m_order.removeAll(key);
    m_order << key;
    while (m_order.size() > MaxEntries) {
        // A read still running for an evicted scope is dropped when it answers.
        const QString oldest = m_order.takeFirst();
        m_entries.remove(oldest);
        m_generation.remove(oldest);
    }
}

const Cache::Entry *Cache::entry(const QString &team, const QString &scope, qint64 revision)
{
    const QString key = team + QLatin1Char('\n') + scope;
    auto found = m_entries.find(key);
    const qint64 now = QDateTime::currentMSecsSinceEpoch();
    if (found == m_entries.end()) { load(team, scope, revision); return &m_entries[key]; }
    touch(key);
    // A changed team is read again once the entry is old enough; nothing else re-reads it.
    if (!found->loading && found->revision != revision && now - found->at > Stale) load(team, scope, revision);
    return &m_entries[key];
}

void Cache::reload(const QString &team, const QString &scope)
{
    const QString key = team + QLatin1Char('\n') + scope;
    if (m_entries.value(key).loading) return;
    load(team, scope, m_entries.value(key).revision);
}

void Cache::load(const QString &team, const QString &scope, qint64 revision)
{
    const QString key = team + QLatin1Char('\n') + scope;
    auto &entry = m_entries[key];
    entry.loading = true; entry.error.clear(); entry.revision = revision;
    touch(key);
    const quint64 generation = ++m_generation[key];
    ++m_reads;
    QJsonObject input{{"team", team}, {"limit", LossPage}};
    if (scope.startsWith("compare:")) {
        input.insert("compare", QJsonArray{scope.section(':', 1, 1).toInt(), scope.section(':', 2, 2).toInt()});
    } else if (scope.startsWith("phase:")) input.insert("phase", scope.section(':', 1).toInt());
    m_teams->call(QStringLiteral("profile"), input, [this, key, team, scope, generation](bool ok, const QJsonObject &result, const QString &error) {
        if (!m_entries.contains(key) || m_generation.value(key) != generation) return;
        auto &entry = m_entries[key];
        if (!ok) { entry.loading = false; entry.error = error; entry.at = QDateTime::currentMSecsSinceEpoch(); emit changed(team, scope); return; }
        if (scope.startsWith("compare:")) {
            entry.compare = result.value("compare").toObject();
            entry.loading = false; entry.ready = true; entry.at = QDateTime::currentMSecsSinceEpoch();
            emit changed(team, scope);
            return;
        }
        entry.profile = result;
        entry.timelines = {};
        spans(key, team, scope.startsWith("phase:") ? scope.section(':', 1).toInt() : 0, 0, 0);
    }, false, 60000);
}

void Cache::spans(const QString &key, const QString &team, int phase, int after, int pages)
{
    const quint64 generation = m_generation.value(key);
    QJsonObject input{{"team", team}, {"limit", SpanPage}};
    if (phase > 0) input.insert("phase", phase);
    if (after > 0) input.insert("after", after);
    ++m_reads;
    m_teams->call(QStringLiteral("spans"), input, [this, key, team, phase, pages, generation](bool ok, const QJsonObject &result, const QString &error) {
        if (!m_entries.contains(key) || m_generation.value(key) != generation) return;
        auto &entry = m_entries[key];
        const QString scope = key.section(QLatin1Char('\n'), 1);
        if (!ok) { entry.loading = false; entry.error = error; entry.at = QDateTime::currentMSecsSinceEpoch(); emit changed(team, scope); return; }
        for (const auto &task : result.value("tasks").toArray()) entry.timelines.append(task);
        const int next = result.value("next_cursor").toInt();
        // Bounded: at most MaxSpanPages pages; the rest is named, never read.
        if (next > 0 && pages + 1 < MaxSpanPages) { spans(key, team, phase, next, pages + 1); return; }
        entry.truncated = next > 0;
        tests(key, team);
    }, false, 60000);
}

// U62 (#719): the test runs of the scope's window, one bounded read. A refusal
// (an older hgs) leaves the Tests row out; the profile still shows.
void Cache::tests(const QString &key, const QString &team)
{
    const quint64 generation = m_generation.value(key);
    const auto summary = m_entries.value(key).profile.value("summary").toObject();
    QJsonObject input{{"team", team}, {"action", "results"}, {"limit", TestPage}};
    if (summary.value("from").toDouble() > 0) input.insert("from", summary.value("from").toDouble());
    if (summary.value("to").toDouble() > 0) input.insert("before", summary.value("to").toDouble() + 1);
    ++m_reads;
    m_teams->call(QStringLiteral("test"), input, [this, key, team, generation](bool ok, const QJsonObject &result, const QString &) {
        if (!m_entries.contains(key) || m_generation.value(key) != generation) return;
        auto &entry = m_entries[key];
        entry.tests = ok ? result.value("items").toArray() : QJsonArray();
        entry.loading = false; entry.ready = true; entry.at = QDateTime::currentMSecsSinceEpoch();
        emit changed(team, key.section(QLatin1Char('\n'), 1));
    }, false);
}

void Cache::moreTasks(const QString &team, const QString &scope)
{
    const QString key = team + QLatin1Char('\n') + scope;
    auto found = m_entries.find(key);
    if (found == m_entries.end() || found->loading) return;
    const auto next = found->profile.value("tasks").toObject().value("next_cursor");
    if (next.isNull() || next.isUndefined()) return;
    found->loading = true;
    const quint64 generation = m_generation.value(key);
    QJsonObject input{{"team", team}, {"limit", SpanPage}, {"after_task", next.toInt()}, {"tokens", true}};
    if (scope.startsWith("phase:")) input.insert("phase", scope.section(':', 1).toInt());
    ++m_reads;
    m_teams->call(QStringLiteral("profile"), input, [this, key, team, scope, generation](bool ok, const QJsonObject &result, const QString &) {
        if (!m_entries.contains(key) || m_generation.value(key) != generation) return;
        auto &entry = m_entries[key];
        entry.loading = false;
        if (ok) {
            auto tasks = entry.profile.value("tasks").toObject();
            auto items = tasks.value("items").toArray();
            for (const auto &item : result.value("tasks").toObject().value("items").toArray()) items.append(item);
            tasks["items"] = items; tasks["next_cursor"] = result.value("tasks").toObject().value("next_cursor");
            entry.profile["tasks"] = tasks;
        }
        emit changed(team, scope);
    }, false, 60000);
}

}   // namespace TeamProfile

using namespace TeamProfile;

// The members' lanes as one painted widget: the axis, the real and planned
// critical paths, a lane per member with its spans, its tasks' waits under it
// and its idle time while tasks were open. Nothing is a child widget, so a
// large team paints as fast as a small one.
class TeamProfileTimeline : public QWidget {
public:
    explicit TeamProfileTimeline(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("teamProfileTimeline"); setMouseTracking(true);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    }
    void setModel(const Model *model, bool dark) { m_model = model; m_dark = dark; setFixedHeight(heightFor()); update(); }
    void select(int task) { m_selected = task; update(); }
    int selected() const { return m_selected; }
    std::function<void(int task)> clicked;
    static constexpr int Top = 34, Row = 52, Label = 210;
    int heightFor() const { return m_model ? Top + Row * (2 + int(m_model->lanes.size()) + (m_model->tests.isEmpty() ? 0 : 1)) + 8 : 120; }
    int laneY(const QString &member) const
    {
        if (!m_model) return -1;
        for (int i = 0; i < m_model->lanes.size(); ++i) if (m_model->lanes[i].member == member) return Top + Row * (2 + i);
        return -1;
    }
protected:
    double x(double at) const
    {
        const double span = std::max(1.0, m_model->to - m_model->from);
        return Label + (width() - Label - 12) * std::clamp((at - m_model->from) / span, 0.0, 1.0);
    }
    void paintEvent(QPaintEvent *) override
    {
        if (!m_model) return;
        const Colors c = colorsFor(m_dark);
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        QFont small = font(); small.setPixelSize(11); QFont bold = small; bold.setWeight(QFont::DemiBold);
        // The axis: about ten ticks, at whole minutes.
        p.setFont(small);
        const double span = std::max(60.0, m_model->to - m_model->from);
        const QList<int> steps{300, 600, 900, 1800, 3600, 7200, 10800, 21600, 43200, 86400};
        int step = steps.last();
        for (int candidate : steps) if (span / candidate <= 11) { step = candidate; break; }
        p.setPen(c.muted);
        p.drawText(QRect(12, 6, Label - 20, 20), Qt::AlignVCenter, tr("MEMBERS"));
        const double first = std::ceil(m_model->from / step) * step;
        for (double at = first; at <= m_model->to + 1; at += step) {
            const int px = int(x(at));
            p.setPen(alpha(c.border, .6)); p.drawLine(px, Top - 4, px, height() - 6);
            p.setPen(c.muted);
            p.drawText(QRect(px - 30, 6, 60, 20), Qt::AlignCenter, QDateTime::fromSecsSinceEpoch(qint64(at)).toString(QStringLiteral("HH:mm")));
        }
        int y = Top;
        const auto rowLabel = [&](const QString &text, const QString &right, bool strong) {
            p.setFont(strong ? bold : small); p.setPen(c.fg);
            p.drawText(QRect(12, y, Label - 70, Row), Qt::AlignVCenter, text);
            p.setFont(small); p.setPen(c.muted);
            p.drawText(QRect(Label - 66, y, 56, Row), Qt::AlignVCenter | Qt::AlignRight, right);
        };
        const auto bar = [&](const Span &span, int top, int h, const QColor &color, bool dashed, const QString &text) {
            const QRectF r(x(span.from), top, std::max(2.0, x(span.to) - x(span.from)), h);
            QColor fill = color;
            if (dashed) { p.setPen(QPen(color, 1.2, Qt::DashLine)); p.setBrush(Qt::NoBrush); }
            else { p.setPen(Qt::NoPen); p.setBrush(fill); }
            p.drawRoundedRect(r, 3, 3);
            if (span.task && span.task == m_selected) { p.setPen(QPen(c.fg, 2)); p.setBrush(Qt::NoBrush); p.drawRoundedRect(r.adjusted(-1, -1, 1, 1), 4, 4); }
            if (!text.isEmpty() && r.width() > 40) {
                p.setFont(small); p.setPen(dashed ? c.fg : QColor(m_dark ? "#0f1a16" : "#ffffff"));
                p.drawText(r.adjusted(5, 0, -4, 0), Qt::AlignVCenter, QFontMetrics(small).elidedText(text, Qt::ElideRight, int(r.width()) - 9));
            }
        };
        const auto label = [&](int task) { return QStringLiteral("#%1 %2").arg(task).arg(m_model->titles.value(task)); };
        // The real critical path, then the planned one.
        rowLabel(tr("Real critical path"), duration(m_model->realSeconds), true);
        for (const auto &span : m_model->real) bar(span, y + 14, 22, kindColor("work", {}, m_dark), false, label(span.task));
        y += Row;
        rowLabel(tr("Planned path"), duration(m_model->plannedSeconds), false);
        for (const auto &span : m_model->planned) bar(span, y + 14, 22, c.fg, true, QStringLiteral("#%1").arg(span.task));
        y += Row;
        // A lane per member.
        for (const auto &lane : m_model->lanes) {
            // The member as its role-colored pill, and its utilization.
            {
                const QColor tone = roleColor(lane.role.isEmpty() ? lane.member : lane.role, m_dark);
                p.setFont(small);
                const int w = std::min(Label - 90, QFontMetrics(small).horizontalAdvance(lane.member) + 16);
                const QRectF pill(12, y + Row / 2 - 10, w, 20);
                p.setPen(QPen(alpha(tone, .7), 1)); p.setBrush(alpha(tone, .14)); p.drawRoundedRect(pill, 10, 10);
                p.setPen(tone); p.drawText(pill, Qt::AlignCenter, QFontMetrics(small).elidedText(lane.member, Qt::ElideRight, w - 10));
                p.setPen(c.muted);
                p.drawText(QRect(Label - 66, y, 56, Row), Qt::AlignVCenter | Qt::AlignRight, QStringLiteral("%1%").arg(qRound(lane.utilization * 100)));
            }
            // Idle while tasks were open: a dashed outline under the work.
            for (const auto &[from, to] : lane.idle) {
                Span idle; idle.from = from; idle.to = to;
                const QRectF r(x(from), y + 8, std::max(1.0, x(to) - x(from)), 22);
                p.setPen(QPen(alpha(c.danger, .7), 1, Qt::DashLine)); p.setBrush(alpha(c.danger, .07)); p.drawRoundedRect(r, 3, 3);
                if (r.width() > 70) {
                    p.setFont(small); p.setPen(c.danger);
                    p.drawText(r.adjusted(5, 0, -4, 0), Qt::AlignVCenter, tr("idle %1").arg(duration(qint64(to - from))));
                }
            }
            for (const auto &span : lane.spans)
                bar(span, span.kind == "build" ? y + 12 : y + 8, span.kind == "build" ? 14 : 22, kindColor(span.kind, span.stageKind, m_dark), false,
                    span.kind == "work" || span.kind == "rework" ? label(span.task) : QString());
            // The waits of its tasks: a hatched strip under the lane.
            for (const auto &span : lane.waits) {
                const QRectF r(x(span.from), y + 34, std::max(2.0, x(span.to) - x(span.from)), 7);
                QBrush hatch(kindColor(span.kind, {}, m_dark), Qt::BDiagPattern);
                p.setPen(QPen(alpha(kindColor(span.kind, {}, m_dark), .8), 1)); p.setBrush(hatch); p.drawRoundedRect(r, 2, 2);
                if (span.task == m_selected) { p.setPen(QPen(c.fg, 1.5)); p.setBrush(Qt::NoBrush); p.drawRoundedRect(r.adjusted(-1, -1, 1, 1), 2, 2); }
            }
            p.setPen(alpha(c.border, .5)); p.drawLine(12, y + Row - 1, width() - 12, y + Row - 1);
            y += Row;
        }
        // U62 (#719): the test runs, colored by their status, named by suite.
        if (!m_model->tests.isEmpty()) {
            const int count = int(m_model->tests.size());
            rowLabel(tr("Tests"), count >= Cache::TestPage ? QStringLiteral("%1+").arg(count) : QString::number(count), true);
            for (const auto &run : m_model->tests) bar(run, y + 12, 18, testColor(run.stageKind, c), false, run.member);
        }
    }
    static QColor testColor(const QString &status, const Colors &c)
    {
        if (status == "passed" || status == "reused") return c.accent;
        if (status == "timeout") return c.warn;
        return c.danger;
    }
    const Span *spanAt(const QPoint &point, QString *lane = nullptr) const
    {
        if (!m_model) return nullptr;
        const int row = (point.y() - Top) / Row;
        const auto hit = [&](const QList<Span> &spans) -> const Span * {
            for (const auto &span : spans) if (point.x() >= x(span.from) - 1 && point.x() <= x(span.to) + 1) return &span;
            return nullptr;
        };
        if (row == 0) return hit(m_model->real);
        if (row == 1) return hit(m_model->planned);
        const int at = row - 2;
        if (at == m_model->lanes.size() && !m_model->tests.isEmpty()) return hit(m_model->tests);
        if (at < 0 || at >= m_model->lanes.size()) return nullptr;
        if (lane) *lane = m_model->lanes[at].member;
        const int inRow = (point.y() - Top) % Row;
        return inRow >= 32 ? hit(m_model->lanes[at].waits) : hit(m_model->lanes[at].spans);
    }
    bool event(QEvent *event) override
    {
        if (event->type() == QEvent::ToolTip) {
            const auto *help = static_cast<QHelpEvent *>(event);
            if (const auto *span = spanAt(help->pos()); span && span->kind == "tests") {
                const QString when = QStringLiteral("%1 to %2").arg(QDateTime::fromSecsSinceEpoch(qint64(span->from)).toString("HH:mm"),
                                                                     QDateTime::fromSecsSinceEpoch(qint64(span->to)).toString("HH:mm"));
                QString text = tr("Tests: %1\n%2, %3, %4").arg(span->member, TeamTests::statusName(span->stageKind), duration(qint64(span->seconds())), when);
                if (span->task) text += tr("\nAsked for #%1 %2").arg(span->task).arg(m_model->titles.value(span->task));
                QToolTip::showText(help->globalPos(), text, this);
            } else if (const auto *span = spanAt(help->pos())) {
                const QString when = QStringLiteral("%1 to %2").arg(QDateTime::fromSecsSinceEpoch(qint64(span->from)).toString("HH:mm"),
                                                                     span->running ? tr("now") : QDateTime::fromSecsSinceEpoch(qint64(span->to)).toString("HH:mm"));
                QToolTip::showText(help->globalPos(), tr("#%1 %2\n%3: %4, %5").arg(span->task).arg(m_model->titles.value(span->task),
                                                                                         kindName(span->kind, span->stageKind), duration(qint64(span->seconds())), when), this);
            } else QToolTip::hideText();
            return true;
        }
        return QWidget::event(event);
    }
    void mousePressEvent(QMouseEvent *event) override
    {
        if (const auto *span = spanAt(event->position().toPoint()); span && span->task) {
            m_selected = span->task; update();
            if (clicked) clicked(span->task);
        }
    }
private:
    const Model *m_model = nullptr;
    bool m_dark = true;
    int m_selected = 0;
};

// Tokens per task as one painted list: the task, its title and member, a bar
// (its accepted work and its returned attempts) and the total.
class TeamProfileBars : public QWidget {
public:
    explicit TeamProfileBars(QWidget *parent = nullptr) : QWidget(parent) { setObjectName("teamProfileTokens"); setMouseTracking(true); }
    void setRows(const QJsonArray &rows, const QHash<int, QString> &assignees, bool dark)
    {
        m_rows = rows; m_assignees = assignees; m_dark = dark;
        m_max = 1;
        for (const auto &row : rows) m_max = std::max<qint64>(m_max, qint64(row.toObject().value("tokens").toObject().value("total").toDouble()));
        setFixedHeight(30 + 46 * int(rows.size()));
        update();
    }
    std::function<void(int task)> clicked;
protected:
    void paintEvent(QPaintEvent *) override
    {
        const Colors c = colorsFor(m_dark);
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        QFont small = font(); small.setPixelSize(11); QFont normal = font(); normal.setPixelSize(13);
        QFont mono = NewSessionDense::mono(12);
        const int barLeft = int(width() * .5), barRight = width() - 170;
        p.setFont(small); p.setPen(c.muted);
        p.drawText(QRect(6, 0, 70, 26), Qt::AlignVCenter, tr("TASK"));
        p.drawText(QRect(80, 0, 300, 26), Qt::AlignVCenter, tr("TITLE"));
        p.drawText(QRect(barLeft - 150, 0, 140, 26), Qt::AlignVCenter, tr("MEMBER"));
        p.drawText(QRect(barLeft, 0, barRight - barLeft, 26), Qt::AlignVCenter, tr("TOKENS  IN / OUT / CACHE W / CACHE R"));
        p.drawText(QRect(width() - 160, 0, 154, 26), Qt::AlignVCenter | Qt::AlignRight, tr("TOTAL"));
        int y = 30;
        for (const auto &value : m_rows) {
            const auto row = value.toObject();
            const int task = row.value("task").toInt();
            const auto spent = row.value("tokens").toObject();
            const bool notStarted = spent.value("not_started").toBool();
            const bool unknown = !notStarted && (spent.value("unknown").toBool() || !spent.contains("total"));
            const qint64 total = qint64(spent.value("total").toDouble()), rework = qint64(row.value("rework_tokens").toDouble());
            const bool cancelled = row.value("status").toString() == "cancelled";
            p.setPen(alpha(c.border, .5)); p.drawLine(0, y + 45, width(), y + 45);
            p.setFont(mono); p.setPen(cancelled ? c.dim : c.fg);
            p.drawText(QRect(6, y, 70, 46), Qt::AlignVCenter, QStringLiteral("#%1").arg(task));
            p.setFont(normal);
            p.drawText(QRect(80, y, barLeft - 240, 46), Qt::AlignVCenter, QFontMetrics(normal).elidedText(row.value("title").toString(), Qt::ElideRight, barLeft - 240));
            p.setPen(c.text2);
            p.drawText(QRect(barLeft - 150, y, 140, 46), Qt::AlignVCenter, m_assignees.value(task));
            const QRectF track(barLeft, y + 7, barRight - barLeft, 12);
            if (unknown) {
                p.setPen(Qt::NoPen); p.setBrush(QBrush(c.muted, Qt::DiagCrossPattern)); p.drawRoundedRect(QRectF(track.left(), track.top(), 75, 12), 3, 3);
            } else if (!notStarted) {
                const double w = track.width() * double(total) / double(m_max), r = total > 0 ? w * double(rework) / double(total) : 0;
                p.setPen(Qt::NoPen);
                p.setBrush(kindColor("work", {}, m_dark)); p.drawRoundedRect(QRectF(track.left(), track.top(), w - r, 12), 3, 3);
                if (r > 0) { p.setBrush(kindColor("rework", {}, m_dark)); p.drawRoundedRect(QRectF(track.left() + w - r, track.top(), r, 12), 3, 3); }
                // #736: the four token buckets stay apart: fresh input, output,
                // cache writes and cache reads, never folded into the total.
                p.setFont(small); p.setPen(c.muted);
                p.drawText(QRect(barLeft, y + 24, barRight - barLeft, 16), Qt::AlignVCenter,
                           tr("in %1 · out %2 · cache w %3 · cache r %4")
                               .arg(tokens(qint64(spent.value("uncached_input").toDouble())), tokens(qint64(spent.value("output").toDouble())),
                                    tokens(qint64(spent.value("cache_write").toDouble())), tokens(qint64(spent.value("cache_read").toDouble()))));
            }
            p.setFont(mono); p.setPen(c.fg);
            const QString share = row.contains("rework_share") && row.value("rework_share").toDouble() > 0
                ? tr("%1% rework").arg(qRound(row.value("rework_share").toDouble() * 100)) : QString();
            p.drawText(QRect(width() - 170, y, 164, 46), Qt::AlignVCenter | Qt::AlignRight,
                       notStarted ? tr("not started") : unknown ? tr("unknown") : share.isEmpty() ? tokens(total) : QStringLiteral("%1  %2").arg(tokens(total), share));
            y += 46;
        }
    }
    void mousePressEvent(QMouseEvent *event) override
    {
        const int at = (int(event->position().y()) - 30) / 46;
        if (at >= 0 && at < m_rows.size() && clicked) clicked(m_rows[at].toObject().value("task").toInt());
    }
private:
    QJsonArray m_rows;
    QHash<int, QString> m_assignees;
    qint64 m_max = 1;
    bool m_dark = true;
};

namespace {
QString lossTitle(const QJsonObject &loss, const Model &model)
{
    const QString kind = loss.value("kind").toString();
    const int task = loss.value("task").toInt();
    const QString name = task ? QStringLiteral("#%1 %2").arg(task).arg(model.titles.value(task)) : QString();
    if (kind == "rework") return QObject::tr("Rework: #%1 attempt %2 %3").arg(task).arg(loss.value("attempt").toInt()).arg(loss.value("cause").toString());
    if (kind == "lead_wait") return QObject::tr("Waited for the lead: %1").arg(name);
    if (kind == "permission_wait") return QObject::tr("Permission wait: %1").arg(name);
    if (kind == "blocked_chain") return QObject::tr("Blocked chain: %1").arg(name);
    if (kind == "member_wait") return QObject::tr("Waited for a free member: %1").arg(name);
    if (kind == "idle_member") return QObject::tr("Idle member while tasks were open: %1").arg(loss.value("member").toString());
    if (kind == "long_test") return QObject::tr("Long test: %1").arg(name);
    if (kind == "merge") return QObject::tr("Merge: %1").arg(name);
    if (kind == "conflict") return QObject::tr("Conflict: %1").arg(name);
    return kind;
}

QString lossDetail(const QJsonObject &loss)
{
    const QString kind = loss.value("kind").toString();
    if (kind == "rework") {
        QString text = QObject::tr("%1 redid it").arg(loss.value("member").toString());
        if (loss.contains("share")) text += QObject::tr(", %1% of the task's tokens").arg(qRound(loss.value("share").toDouble() * 100));
        return text;
    }
    if (kind == "lead_wait") return QObject::tr("decisions %1, questions %2").arg(duration(qint64(loss.value("lead_s").toDouble())), duration(qint64(loss.value("question_s").toDouble())));
    if (kind == "permission_wait") {
        QStringList by; for (const auto &who : loss.value("answered_by").toArray()) by << who.toString();
        return by.isEmpty() ? QString() : QObject::tr("answered by %1").arg(by.join(QStringLiteral(", ")));
    }
    if (kind == "blocked_chain") {
        QStringList on; for (const auto &id : loss.value("on").toArray()) on << QStringLiteral("#%1").arg(id.toInt());
        QString text = on.isEmpty() ? QString() : QObject::tr("dependencies %1 for %2").arg(on.join(QStringLiteral(", ")), duration(qint64(loss.value("deps_s").toDouble())));
        if (loss.value("folder_held_s").toDouble() > 0) text += (text.isEmpty() ? QString() : QStringLiteral("; ")) + QObject::tr("folder held %1").arg(duration(qint64(loss.value("folder_held_s").toDouble())));
        return text;
    }
    if (kind == "member_wait") return QObject::tr("given to %1, which was busy").arg(loss.value("member").toString());
    if (kind == "idle_member") return QObject::tr("no work for %1 of %2 open").arg(duration(qint64(loss.value("s").toDouble())), duration(qint64(loss.value("open_s").toDouble())));
    if (kind == "long_test" || kind == "merge" || kind == "conflict") {
        const QString template_ = loss.value("template").toString();
        return template_.isEmpty() ? QObject::tr("stage %1, round %2").arg(loss.value("stage").toInt()).arg(loss.value("round").toInt()) : template_;
    }
    return {};
}
}

TeamProfileView::TeamProfileView(TeamController *teams, Cache *cache, QWidget *parent) : QWidget(parent), m_teams(teams), m_cache(cache)
{
    setObjectName("teamProfileView");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(12, 6, 12, 6); outer->setSpacing(8);
    auto *bar = new QHBoxLayout; bar->setSpacing(8);
    m_scope = new QComboBox; m_scope->setObjectName("teamProfileScope"); m_scope->setMinimumWidth(320);
    QObject::connect(m_scope, &QComboBox::activated, this, [this](int at) { setPhase(m_scope->itemData(at).toInt()); });
    bar->addWidget(m_scope);
    m_tabs = segmented({{"time", tr("Time")}, {"tokens", tr("Tokens")}, {"members", tr("Members")}, {"compare", tr("Compare")}}, m_tab, "teamProfileTabs",
                       [this](const QString &value) { setTab(value); });
    bar->addWidget(m_tabs);
    m_state = new QLabel; m_state->setObjectName("teamProfileState"); bar->addWidget(m_state, 1);
    auto *exportButton = button(tr("Export JSON"), "teamProfileExport");
    QObject::connect(exportButton, &QPushButton::clicked, this, [this] { exportJson(); });
    bar->addWidget(exportButton);
    outer->addLayout(bar);
    auto *split = new QHBoxLayout; split->setSpacing(12);
    auto *bodyBox = new QWidget; m_bodyHost = new QVBoxLayout(bodyBox); m_bodyHost->setContentsMargins(0, 0, 0, 0);
    split->addWidget(bodyBox, 1);
    // Top losses at the right, by time or by tokens.
    m_side = new QFrame; m_side->setObjectName("teamProfileLosses"); m_side->setFixedWidth(400);
    auto *side = new QVBoxLayout(m_side); side->setContentsMargins(12, 10, 12, 10); side->setSpacing(8);
    auto *sideHead = new QHBoxLayout;
    auto *title = new QLabel(tr("Top losses")); title->setObjectName("teamProfileLossesTitle"); sideHead->addWidget(title, 1);
    auto *order = segmented({{"time", tr("By time")}, {"tokens", tr("By tokens")}}, m_order, "teamProfileLossOrder", [this](const QString &value) { setLossOrder(value); });
    sideHead->addWidget(order);
    side->addLayout(sideHead);
    auto *scroll = new QScrollArea; scroll->setObjectName("teamProfileLossesScroll"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    auto *list = new QWidget; list->setObjectName("teamProfileLossList");
    m_lossList = new QVBoxLayout(list); m_lossList->setContentsMargins(0, 0, 0, 0); m_lossList->setSpacing(8);
    scroll->setWidget(list); side->addWidget(scroll, 1);
    auto *note = new QLabel(tr("The lead's phase report names the first three. Planning reads the per-role numbers in Members."));
    note->setObjectName("teamProfileLossesNote"); note->setWordWrap(true); side->addWidget(note);
    split->addWidget(m_side);
    outer->addLayout(split, 1);
    QObject::connect(m_cache, &Cache::changed, this, [this](const QString &team, const QString &) { if (team == m_team.id) read(); });
    setDark(true);
}

void TeamProfileView::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QFrame#teamProfileLosses{border-left:1px solid %1;}"
        "QLabel#teamProfileLossesTitle{color:%2;font-size:14px;font-weight:600;}"
        "QLabel#teamProfileLossesNote,QLabel#teamProfileState{color:%3;font-size:11px;}"
        "QFrame#teamProfileLoss{background:%4;border:1px solid %1;border-radius:10px;}"
        "QFrame#teamProfileLoss[first=\"true\"]{border-color:%5;}"
        "QFrame#teamProfileLoss QLabel{background:transparent;}"
        "QFrame#teamProfileTile,QFrame#teamProfileCard{background:%4;border:1px solid %1;border-radius:10px;}"
        "QFrame#teamProfileTile QLabel,QFrame#teamProfileCard QLabel{background:transparent;}"
        "QScrollArea#teamProfileLossesScroll,QWidget#teamProfileLossList,QScrollArea#teamProfileBodyScroll,QWidget#teamProfileBody{background:transparent;}"
        "QPushButton[lossChip=\"true\"]{border-radius:6px;padding:1px 8px;color:%6;background:%7;border:0;font-size:11px;}"
        "QTableWidget#teamProfileMembers{background:transparent;border:0;gridline-color:transparent;font-size:12px;}"
        "QTableWidget#teamProfileMembers QHeaderView::section{background:transparent;color:%3;border:0;border-bottom:1px solid %1;padding:4px 6px;font-size:11px;}")
        .arg(c.cardBorder.name(), c.fg.name(), c.muted.name(), c.cardFill.name(), c.text2.name(), c.folder.name(), alpha(c.folder, .12).name(QColor::HexArgb)));
    if (!m_team.id.isEmpty()) render();
}

QString TeamProfileView::scope() const
{
    if (m_tab == "compare") return QStringLiteral("compare:%1:%2").arg(m_compareA).arg(m_compareB);
    return m_phase > 0 ? QStringLiteral("phase:%1").arg(m_phase) : QStringLiteral("plan");
}

void TeamProfileView::setTeam(const Teams::Team &team)
{
    const bool other = team.id != m_team.id;
    m_team = team;
    if (other) {
        m_model = {}; m_shownKey.clear();
        // The phase in work, or the last one.
        m_phase = 0;
        for (const auto &phase : team.phases) if (phase.state == "approved" || phase.state == "running") m_phase = phase.n;
        if (!m_phase && !team.phases.isEmpty()) m_phase = team.phases.last().n;
        m_compareB = m_phase; m_compareA = m_phase > 1 ? m_phase - 1 : m_phase;
    }
    const QSignalBlocker block(m_scope);
    m_scope->clear();
    m_scope->addItem(tr("The plan, every phase"), 0);
    if (!team.phases.isEmpty() || !team.tasks.isEmpty()) {
        QList<int> numbers{1};
        for (const auto &phase : team.phases) if (!numbers.contains(phase.n)) numbers << phase.n;
        for (int n : numbers) {
            const auto *phase = team.phase(n);
            m_scope->addItem(phase && !phase->title.isEmpty() ? tr("Phase %1 · %2  %3").arg(n).arg(phase->title, phase->state) : tr("Phase %1").arg(n), n);
        }
    }
    m_scope->setCurrentIndex(std::max(0, m_scope->findData(m_phase)));
    read();
}

void TeamProfileView::setPhase(int phase)
{
    m_phase = phase;
    m_scope->setCurrentIndex(std::max(0, m_scope->findData(phase)));
    read();
}

void TeamProfileView::setTab(const QString &tab)
{
    m_tab = tab;
    for (auto *b : m_tabs->findChildren<QPushButton *>()) b->setChecked(b->property("segment").toString() == tab);
    m_shownKey.clear();
    read();
}

void TeamProfileView::setLossOrder(const QString &order)
{
    m_order = order;
    for (auto *b : m_side->findChildren<QPushButton *>()) if (b->property("segment").isValid()) b->setChecked(b->property("segment").toString() == order);
    renderLosses();
}

void TeamProfileView::showModel(const Model &model)
{
    m_model = model;
    m_shownKey = QStringLiteral("direct");
    render();
}

int TeamProfileView::selectedTask() const { return m_timeline ? m_timeline->selected() : 0; }

void TeamProfileView::read()
{
    if (m_team.id.isEmpty()) { render(); return; }
    const QString scopeKey = scope();
    const auto *entry = m_cache->entry(m_team.id, scopeKey, m_team.revision);
    m_state->setText(entry->loading ? tr("Reading the profile in the background…")
                     : !entry->error.isEmpty() ? tr("Not read: %1").arg(entry->error)
                     : entry->truncated ? tr("The first %1 tasks; the rest were not read").arg(Cache::MaxSpanPages * Cache::SpanPage) : QString());
    // A workspace render that changed nothing in this read draws nothing.
    const QString key = scopeKey + QLatin1Char('\n') + QString::number(entry->at) + (entry->loading ? QStringLiteral("+") : QString())
                        + QLatin1Char('\n') + entry->error;
    if (key == m_shownKey) return;
    m_shownKey = key;
    if (m_tab != "compare" && entry->ready) {
        m_model = Model::build(m_team, entry->profile, entry->timelines, double(QDateTime::currentSecsSinceEpoch()), entry->tests);
        m_model.truncated = entry->truncated;
    }
    render();
}

void TeamProfileView::pick(int task)
{
    if (m_tab != "time") setTab(QStringLiteral("time"));
    if (m_timeline) m_timeline->select(task);
    if (openTask) openTask(task);
}

void TeamProfileView::render()
{
    const QString tab = m_tab;
    QWidget *page = tab == "tokens" ? tokensPage() : tab == "members" ? membersPage() : tab == "compare" ? comparePage() : timePage();
    page->setObjectName(QStringLiteral("teamProfilePage"));
    page->setProperty("tab", tab);
    if (m_body) { m_bodyHost->removeWidget(m_body); retire(m_body); }
    m_bodyHost->addWidget(page); m_body = page;
    m_side->setVisible(tab != "compare" && tab != "members");
    renderLosses();
}

QWidget *TeamProfileView::summaryTiles()
{
    const Colors c = colorsFor(m_dark);
    const auto summary = m_model.profile.value("summary").toObject();
    const auto totals = summary.value("totals").toObject();
    auto *row = new QWidget; auto *h = new QHBoxLayout(row); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
    const auto tile = [&](const QString &name, const QString &value, const QString &sub, const QColor &tone, const char *object) {
        auto *box = new QFrame; box->setObjectName("teamProfileTile"); box->setProperty("tile", QString::fromLatin1(object));
        auto *v = new QVBoxLayout(box); v->setContentsMargins(12, 8, 12, 8); v->setSpacing(1);
        v->addWidget(label(name, c.muted, 11));
        auto *big = label(value, tone, 17, true, object); v->addWidget(big);
        auto *small = label(sub, c.muted, 11); small->setWordWrap(true); v->addWidget(small);
        h->addWidget(box, 1);
    };
    const auto clock = [](double at) { return at > 0 ? QDateTime::fromSecsSinceEpoch(qint64(at)).toString(QStringLiteral("HH:mm")) : QStringLiteral("?"); };
    tile(tr("Took"), duration(qint64(summary.value("elapsed_s").toDouble())), tr("%1 to %2").arg(clock(summary.value("from").toDouble()), clock(summary.value("to").toDouble())), c.fg, "teamProfileTook");
    const qint64 delta = m_model.realSeconds - m_model.plannedSeconds;
    tile(tr("Planned critical path"), duration(m_model.plannedSeconds),
         tr("real %1, %2%3").arg(duration(m_model.realSeconds), delta >= 0 ? QStringLiteral("+") : QStringLiteral("-"), duration(std::abs(delta))), c.fg, "teamProfilePlanned");
    int working = 0;
    for (const auto &lane : m_model.lanes) working += lane.busy > 0;
    tile(tr("Worked"), duration(qint64(summary.value("worked_s").toDouble())), tr("%1 of %2 members").arg(working).arg(m_model.lanes.size()), c.fg, "teamProfileWorked");
    int returns = 0;
    for (const auto &loss : m_model.profile.value("losses").toObject().value("by_time").toObject().value("items").toArray()) returns += loss.toObject().value("kind") == "rework";
    tile(tr("Rework"), returns == 1 ? tr("1 return") : tr("%1 returns").arg(returns), tr("%1 of work redone").arg(duration(qint64(totals.value("rework").toDouble()))),
         returns ? c.warn : c.fg, "teamProfileRework");
    int waited = 0;
    for (const auto &lane : m_model.lanes) for (const auto &span : lane.waits) waited += span.kind == "member";
    tile(tr("Waited for a free member"), duration(qint64(totals.value("member").toDouble())), tr("summed over %1 spans").arg(waited),
         totals.value("member").toDouble() > 0 ? c.warn : c.fg, "teamProfileMemberWait");
    const bool leadKnown = totals.contains("lead") || totals.contains("question");
    tile(tr("Waited for the lead"), leadKnown ? duration(qint64(totals.value("lead").toDouble() + totals.value("question").toDouble())) : tr("unknown"),
         leadKnown ? tr("decisions and questions") : tr("decisions not recorded"), c.fg, "teamProfileLeadWait");
    tile(tr("Accepted"), tr("%1 of %2").arg(summary.value("accepted").toInt()).arg(summary.value("tasks").toInt()), tr("tasks of this scope"), c.fg, "teamProfileAccepted");
    return row;
}

QWidget *TeamProfileView::timePage()
{
    const Colors c = colorsFor(m_dark);
    auto *page = new QWidget; auto *v = new QVBoxLayout(page); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    if (m_model.profile.isEmpty()) {
        v->addWidget(label(tr("Reading the profile in the background…"), c.muted, 13, false, "teamProfileEmpty"));
        v->addStretch(1);
        return page;
    }
    v->addWidget(summaryTiles());
    auto *card = new QFrame; card->setObjectName("teamProfileCard");
    auto *cv = new QVBoxLayout(card); cv->setContentsMargins(4, 6, 4, 6);
    auto *timeline = new TeamProfileTimeline; timeline->setModel(&m_model, m_dark);
    if (m_timeline) timeline->select(m_timeline->selected());
    timeline->clicked = [this](int task) { if (openTask) openTask(task); };
    m_timeline = timeline;
    // Many lanes scroll inside the card; the timeline is painted, never a widget per span.
    auto *scroll = new QScrollArea; scroll->setObjectName("teamProfileBodyScroll"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setWidget(timeline); scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    cv->addWidget(scroll);
    // As tall as its lanes; a team with many lanes scrolls inside it.
    scroll->setMaximumHeight(timeline->heightFor() + 4);
    card->setMaximumHeight(timeline->heightFor() + 18);
    v->addWidget(card, 1);
    // The legend.
    auto *legend = new QWidget; auto *lh = new QHBoxLayout(legend); lh->setContentsMargins(4, 0, 4, 0); lh->setSpacing(12);
    lh->addWidget(label(tr("SPANS"), c.muted, 11, true));
    const QList<std::pair<QString, QString>> kinds{{"work", ""}, {"rework", ""}, {"build", ""}, {"stage", "review"}, {"stage", "test"}, {"question", ""},
                                                   {"delivery", ""}, {"member", ""}, {"deps", ""}, {"lead", ""}};
    for (const auto &[kind, stage] : kinds) {
        auto *swatch = new QLabel; QPixmap pixmap(14, 10); pixmap.fill(Qt::transparent);
        QPainter p(&pixmap); p.setRenderHint(QPainter::Antialiasing);
        const bool wait = Waits.contains(kind) && kind != "delivery";
        p.setPen(wait ? QPen(kindColor(kind, stage, m_dark), 1) : Qt::NoPen);
        p.setBrush(wait ? QBrush(kindColor(kind, stage, m_dark), Qt::BDiagPattern) : QBrush(kindColor(kind, stage, m_dark)));
        p.drawRoundedRect(QRectF(.5, .5, 13, 9), 2, 2); p.end();
        swatch->setPixmap(pixmap);
        lh->addWidget(swatch); lh->addWidget(label(kindName(kind, stage), c.text2, 11));
    }
    lh->addStretch(1);
    if (m_model.truncated) lh->addWidget(label(tr("Only the first %1 tasks are drawn").arg(Cache::MaxSpanPages * Cache::SpanPage), c.warn, 11, false, "teamProfileTruncated"));
    v->addWidget(legend);
    if (auto *estimates = estimatesCard()) v->addWidget(estimates);
    v->addStretch(0);
    return page;
}

// Owner 2026-10-31 (#746): each task's estimate against its working time, the
// largest overrun first; at most EstimateRows rows, the rest counted.
QWidget *TeamProfileView::estimatesCard()
{
    if (m_model.estimates.isEmpty()) return nullptr;
    const Colors c = colorsFor(m_dark);
    auto rows = m_model.estimates;
    const auto over = [](const Model::Estimate &row) { return row.estimate > 0 ? row.worked / 60 - row.estimate : -1e9; };
    std::stable_sort(rows.begin(), rows.end(), [&](const Model::Estimate &a, const Model::Estimate &b) { return over(a) > over(b); });
    auto *card = new QFrame; card->setObjectName("teamProfileEstimates");
    card->setStyleSheet(QStringLiteral("QFrame#teamProfileEstimates{background:%1;border:1px solid %2;border-radius:9px;}")
                            .arg(c.surface.name(QColor::HexArgb), c.border.name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(card); v->setContentsMargins(12, 10, 12, 10); v->setSpacing(4);
    int known = 0, past = 0;
    for (const auto &row : rows) if (row.estimate > 0 && row.worked > 0) { ++known; if (row.worked / 60 > row.estimate) ++past; }
    auto *head = row(10);
    head->addWidget(label(tr("ESTIMATE AND ACTUAL"), c.muted, 11, true));
    if (known) head->addWidget(label(tr("%1 of %2 over the estimate").arg(past).arg(known), past ? c.warn : c.text2, 11, false, "teamProfileEstimatesSummary"));
    head->addStretch(1);
    v->addLayout(head);
    auto *grid = new QGridLayout; grid->setHorizontalSpacing(14); grid->setVerticalSpacing(3);
    int line = 0;
    for (const auto &column : {tr("TASK"), tr("ESTIMATE"), tr("ACTUAL"), tr("DIFFERENCE")}) grid->addWidget(label(column, c.muted, 10, true), 0, line++);
    line = 1;
    for (const auto &row : rows) {
        if (line > EstimateRows) break;
        const qint64 worked = qRound64(row.worked / 60);
        auto *task = new QPushButton(QStringLiteral("#%1 %2").arg(row.task).arg(m_model.titles.value(row.task)));
        task->setObjectName("teamProfileEstimateTask"); task->setProperty("task", row.task); task->setFlat(true); task->setCursor(Qt::PointingHandCursor);
        task->setStyleSheet(QStringLiteral("QPushButton{color:%1;border:none;padding:0;text-align:left;}").arg(c.fg.name()));
        task->setMaximumWidth(360);
        task->setText(task->fontMetrics().elidedText(task->text(), Qt::ElideRight, 350));
        QObject::connect(task, &QPushButton::clicked, this, [this, id = row.task] { pick(id); if (openTask) openTask(id); });
        grid->addWidget(task, line, 0);
        grid->addWidget(label(row.estimate > 0 ? tr("~%1 min").arg(qRound64(row.estimate)) : tr("no estimate yet"), c.text2, 12, false, "teamProfileEstimateValue"), line, 1);
        grid->addWidget(label(row.worked > 0 ? (row.running ? tr("%1 min so far").arg(worked) : tr("%1 min").arg(worked)) : tr("not started"),
                              c.text2, 12, false, "teamProfileEstimateActual"), line, 2);
        QString diff; QColor tone = c.muted;
        if (row.estimate > 0 && row.worked > 0) {
            const qint64 delta = worked - qRound64(row.estimate);
            diff = delta > 0 ? tr("%1 min over").arg(delta) : delta < 0 ? tr("%1 min under").arg(-delta) : tr("on the estimate");
            tone = delta > 0 ? c.warn : c.text2;
        }
        auto *d = label(diff, tone, 12, false, "teamProfileEstimateDiff"); d->setProperty("task", row.task);
        grid->addWidget(d, line, 3);
        ++line;
    }
    grid->setColumnStretch(4, 1);
    v->addLayout(grid);
    if (rows.size() > EstimateRows)
        v->addWidget(label(tr("%1 more tasks, the largest overruns first").arg(rows.size() - EstimateRows), c.muted, 11, false, "teamProfileEstimatesMore"));
    return card;
}

QWidget *TeamProfileView::tokensPage()
{
    const Colors c = colorsFor(m_dark);
    auto *page = new QWidget; auto *v = new QVBoxLayout(page); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    const auto tasks = m_model.profile.value("tasks").toObject();
    const auto summary = m_model.profile.value("summary").toObject().value("tokens").toObject();
    auto *head = row(10);
    head->addWidget(label(m_phase ? tr("TOKENS PER TASK, PHASE %1").arg(m_phase) : tr("TOKENS PER TASK, THE PLAN"), c.muted, 11, true));
    if (summary.value("partial").toBool() || summary.value("unknown").toBool())
        head->addWidget(label(tr("partial: some sessions are on another computer or not recorded"), c.violet, 11, false, "teamProfileTokensPartial"));
    // #736: the four buckets apart; the total counts the fresh input and output.
    if (summary.contains("total"))
        head->addWidget(label(tr("fresh in %1 · out %2 · cache w %3 · cache r %4")
                                  .arg(tokens(qint64(summary.value("uncached_input").toDouble())), tokens(qint64(summary.value("output").toDouble())),
                                       tokens(qint64(summary.value("cache_write").toDouble())), tokens(qint64(summary.value("cache_read").toDouble()))),
                              c.muted, 11, false, "teamProfileTokensBuckets"));
    head->addStretch(1);
    v->addLayout(head);
    auto *bars = new TeamProfileBars;
    QHash<int, QString> assignees;
    for (auto it = m_model.assignees.cbegin(); it != m_model.assignees.cend(); ++it) assignees.insert(it.key(), m_team.roleOf(it.value()).isEmpty() ? it.value() : it.value());
    bars->setRows(tasks.value("items").toArray(), assignees, m_dark);
    bars->clicked = [this](int task) { pick(task); };
    auto *scroll = new QScrollArea; scroll->setObjectName("teamProfileBodyScroll"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setWidget(bars);
    v->addWidget(scroll, 1);
    if (!tasks.value("next_cursor").isNull() && !tasks.value("next_cursor").isUndefined()) {
        auto *more = button(tr("Show more tasks"), "teamProfileMoreTasks");
        QObject::connect(more, &QPushButton::clicked, this, [this] { m_shownKey.clear(); m_cache->moreTasks(m_team.id, scope()); });
        v->addWidget(more, 0, Qt::AlignLeft);
    }
    return page;
}

QWidget *TeamProfileView::membersPage()
{
    const Colors c = colorsFor(m_dark);
    auto *page = new QWidget; auto *v = new QVBoxLayout(page); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    auto *table = new QTableWidget; table->setObjectName("teamProfileMembers");
    const QStringList headers{tr("Member"), tr("Agent and model"), tr("Accepted"), tr("Attempts"), tr("Returned"), tr("Worked"), tr("Utilization"),
                              tr("Its tasks waited"), tr("Work redone"), tr("Tokens per accepted")};
    table->setColumnCount(int(headers.size())); table->setHorizontalHeaderLabels(headers);
    table->verticalHeader()->hide(); table->setShowGrid(false); table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table->setSelectionMode(QAbstractItemView::NoSelection); table->verticalHeader()->setDefaultSectionSize(34);
    table->horizontalHeader()->setSectionResizeMode(QHeaderView::Stretch);
    table->horizontalHeader()->setSectionResizeMode(0, QHeaderView::ResizeToContents);
    QHash<QString, QJsonObject> rows;
    for (const auto &value : m_model.profile.value("members").toArray()) rows.insert(value.toObject().value("member").toString(), value.toObject());
    const auto item = [&](const QString &text, bool right = true, const QColor &tone = {}) {
        auto *cell = new QTableWidgetItem(text); cell->setTextAlignment(right ? Qt::AlignRight | Qt::AlignVCenter : Qt::AlignLeft | Qt::AlignVCenter);
        if (tone.isValid()) cell->setForeground(tone);
        return cell;
    };
    int at = 0;
    for (const auto &lane : std::as_const(m_model.lanes)) {
        if (!rows.contains(lane.member)) continue;
        const auto row = rows.value(lane.member);
        QSet<std::pair<int, int>> attempts, returned;
        double work = 0, rework = 0;
        for (const auto &span : lane.spans) {
            if (span.kind == "work" || span.kind == "rework") attempts.insert({span.task, span.attempt});
            if (span.kind == "rework") { returned.insert({span.task, span.attempt}); rework += span.seconds(); }
            if (span.kind == "work") work += span.seconds();
        }
        double waited = 0;
        for (const auto &span : lane.waits) waited += span.seconds();
        table->insertRow(at);
        table->setItem(at, 0, item(lane.member, false));
        table->setItem(at, 1, item(lane.model, true));
        table->setItem(at, 2, item(QString::number(row.value("accepted").toInt())));
        table->setItem(at, 3, item(QString::number(attempts.size())));
        table->setItem(at, 4, item(QString::number(returned.size()), true, returned.isEmpty() ? QColor() : c.warn));
        table->setItem(at, 5, item(duration(qint64(row.value("busy_s").toDouble()))));
        table->setItem(at, 6, item(QStringLiteral("%1%").arg(qRound(row.value("utilization").toDouble() * 100))));
        table->setItem(at, 7, item(duration(qint64(waited))));
        const int redone = work + rework > 0 ? qRound(100 * rework / (work + rework)) : 0;
        table->setItem(at, 8, item(QStringLiteral("%1%").arg(redone), true, redone ? c.warn : QColor()));
        table->setItem(at, 9, item(row.contains("tokens_per_accepted") ? tokens(qint64(row.value("tokens_per_accepted").toDouble())) : QStringLiteral("-"), true, c.violet));
        table->item(at, 0)->setData(Qt::UserRole, lane.member);
        ++at;
    }
    // The groups: per role and per model.
    const auto group = [&](const QJsonArray &list, const char *key, const QString &format) {
        for (const auto &value : list) {
            const auto row = value.toObject();
            table->insertRow(at);
            auto *name = item(format.arg(row.value(QLatin1String(key)).toString()), false); QFont bold = name->font(); bold.setBold(true); name->setFont(bold);
            table->setItem(at, 0, name);
            table->setItem(at, 1, item(QString()));
            table->setItem(at, 2, item(QString::number(row.value("accepted").toInt())));
            for (int column : {3, 4, 7, 8}) table->setItem(at, column, item(QString()));
            table->setItem(at, 5, item(duration(qint64(row.value("busy_s").toDouble()))));
            table->setItem(at, 6, item(QStringLiteral("%1%").arg(qRound(row.value("utilization").toDouble() * 100))));
            table->setItem(at, 9, item(row.contains("tokens_per_accepted") ? tokens(qint64(row.value("tokens_per_accepted").toDouble())) : QStringLiteral("-"), true, c.violet));
            for (int column = 0; column < table->columnCount(); ++column) if (auto *cell = table->item(at, column)) cell->setBackground(alpha(c.fg, .04));
            table->item(at, 0)->setData(Qt::UserRole + 1, QString::fromLatin1(key));
            ++at;
        }
    };
    group(m_model.profile.value("roles").toArray(), "role", tr("Role %1"));
    group(m_model.profile.value("models").toArray(), "model", tr("Model %1"));
    QObject::connect(table, &QTableWidget::cellClicked, this, [this, table](int row, int) {
        if (auto *cell = table->item(row, 0); cell && !cell->data(Qt::UserRole).toString().isEmpty()) setTab(QStringLiteral("time"));
    });
    v->addWidget(table, 1);
    v->addWidget(label(tr("Utilization counts worked spans only; waits for a dependency, a free member or the lead are not work. A row opens the timeline."), c.muted, 11));
    return page;
}

QWidget *TeamProfileView::comparePage()
{
    const Colors c = colorsFor(m_dark);
    auto *page = new QFrame; page->setObjectName("teamProfileCard");
    auto *grid = new QGridLayout(page); grid->setContentsMargins(14, 10, 14, 10); grid->setHorizontalSpacing(18); grid->setVerticalSpacing(10);
    grid->setColumnStretch(1, 1); grid->setColumnStretch(2, 1);
    grid->addWidget(label(tr("Measure"), c.fg, 13, true), 0, 0);
    const auto phases = [&](int current, const char *name, bool first) {
        auto *combo = new QComboBox; combo->setObjectName(name);
        for (const auto &phase : m_team.phases) combo->addItem(tr("Phase %1 · %2").arg(phase.n).arg(phase.title), phase.n);
        if (combo->findData(1) < 0) combo->insertItem(0, tr("Phase 1"), 1);
        combo->setCurrentIndex(std::max(0, combo->findData(current)));
        QObject::connect(combo, &QComboBox::activated, this, [this, combo, first](int at) {
            (first ? m_compareA : m_compareB) = combo->itemData(at).toInt(); read();
        });
        return combo;
    };
    grid->addWidget(phases(m_compareA, "teamProfileCompareA", true), 0, 1);
    grid->addWidget(phases(m_compareB, "teamProfileCompareB", false), 0, 2);
    grid->addWidget(label(tr("Delta"), c.fg, 13, true), 0, 3);
    const auto *entry = m_cache->entry(m_team.id, scope(), m_team.revision);
    const auto compare = entry ? entry->compare : QJsonObject();
    if (compare.isEmpty()) {
        grid->addWidget(label(entry && !entry->error.isEmpty() ? tr("Not read: %1").arg(entry->error) : tr("Reading both phases in the background…"), c.muted, 12, false, "teamProfileCompareState"), 1, 0, 1, 4);
        grid->setRowStretch(2, 1);
        return page;
    }
    const auto a = compare.value("a").toObject().value("summary").toObject(), b = compare.value("b").toObject().value("summary").toObject();
    const auto delta = compare.value("delta").toObject();
    int r = 1;
    // `unit`: 0 a count, 1 seconds, 2 tokens.
    const auto measure = [&](const QString &name, const QString &left, const QString &right, qint64 change, int unit, bool lowerIsBetter) {
        grid->addWidget(label(name, c.text2, 13), r, 0);
        grid->addWidget(label(left, c.fg, 13, false, "teamProfileCompareA"), r, 1);
        grid->addWidget(label(right, c.fg, 13, false, "teamProfileCompareB"), r, 2);
        if (change != 0) {
            const QString text = (change > 0 ? QStringLiteral("+") : QStringLiteral("-"))
                + (unit == 1 ? duration(std::abs(change)) : unit == 2 ? tokens(std::abs(change)) : QString::number(std::abs(change)));
            grid->addWidget(label(text, (change > 0) == lowerIsBetter ? c.warn : c.accent, 13, true, "teamProfileCompareDelta"), r, 3);
        }
        ++r;
    };
    const auto seconds = [](const QJsonObject &summary, const char *key) { return qint64(summary.value(QLatin1String(key)).toDouble()); };
    const auto total = [](const QJsonObject &summary, const char *kind) { return qint64(summary.value("totals").toObject().value(QLatin1String(kind)).toDouble()); };
    measure(tr("Took"), duration(seconds(a, "elapsed_s")), duration(seconds(b, "elapsed_s")), qint64(delta.value("elapsed_s").toDouble()), 1, true);
    measure(tr("Tasks"), tr("%1: %2 accepted").arg(a.value("tasks").toInt()).arg(a.value("accepted").toInt()),
            tr("%1: %2 accepted").arg(b.value("tasks").toInt()).arg(b.value("accepted").toInt()), qint64(delta.value("tasks").toDouble()), 0, false);
    measure(tr("Worked"), duration(seconds(a, "worked_s")), duration(seconds(b, "worked_s")), qint64(delta.value("worked_s").toDouble()), 1, false);
    for (const auto &[kind, name] : QList<std::pair<const char *, QString>>{{"rework", tr("Work redone")}, {"member", tr("Waited for a free member")},
                                                                           {"deps", tr("Waited for dependencies")}, {"lead", tr("Waited for the lead")}})
        measure(name, duration(total(a, kind)), duration(total(b, kind)), total(b, kind) - total(a, kind), 1, true);
    const auto tokensOf = [](const QJsonObject &summary) {
        const auto spent = summary.value("tokens").toObject();
        return spent.contains("total") ? tokens(qint64(spent.value("total").toDouble())) : QObject::tr("unknown");
    };
    measure(tr("Tokens"), tokensOf(a), tokensOf(b), delta.value("tokens").isNull() ? 0 : qint64(delta.value("tokens").toDouble()), 2, true);
    grid->addWidget(label(tr("Top loss"), c.text2, 13), r, 0);
    for (const auto &[side, column] : QList<std::pair<const char *, int>>{{"a", 1}, {"b", 2}}) {
        const auto top = compare.value(QLatin1String(side)).toObject().value("top_losses").toArray();
        if (top.isEmpty()) continue;
        auto *chip = button(lossTitle(top.first().toObject(), m_model) + QStringLiteral(": ") + duration(qint64(top.first().toObject().value("s").toDouble())), "teamProfileCompareLoss");
        chip->setProperty("lossChip", true);
        const int phase = compare.value(QLatin1String(side)).toObject().value("phase").toInt();
        QObject::connect(chip, &QPushButton::clicked, this, [this, phase] { setTab(QStringLiteral("time")); setPhase(phase); });
        grid->addWidget(chip, r, column, Qt::AlignLeft);
    }
    grid->setRowStretch(r + 1, 1);
    return page;
}

void TeamProfileView::renderLosses()
{
    const Colors c = colorsFor(m_dark);
    while (auto *item = m_lossList->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    const auto items = m_model.profile.value("losses").toObject().value(m_order == "tokens" ? "by_tokens" : "by_time").toObject().value("items").toArray();
    if (items.isEmpty()) {
        m_lossList->addWidget(label(m_model.profile.isEmpty() ? tr("Reading…") : tr("No losses recorded"), c.muted, 12, false, "teamProfileNoLosses"));
        m_lossList->addStretch(1);
        return;
    }
    const double top = m_order == "tokens" ? items.first().toObject().value("tokens").toDouble() : items.first().toObject().value("s").toDouble();
    int n = 0;
    for (const auto &value : items) {
        const auto loss = value.toObject();
        ++n;
        auto *card = new QFrame; card->setObjectName("teamProfileLoss"); card->setProperty("first", n == 1);
        card->setProperty("loss", loss.value("id").toString()); card->setProperty("kind", loss.value("kind").toString());
        auto *v = new QVBoxLayout(card); v->setContentsMargins(12, 9, 12, 9); v->setSpacing(4);
        auto *head = row(8);
        head->addWidget(label(QString::number(n), c.muted, 12, true));
        auto *title = label(lossTitle(loss, m_model), c.fg, 13, true, "teamProfileLossTitle"); title->setWordWrap(true);
        head->addWidget(title, 1);
        const double amount = m_order == "tokens" ? loss.value("tokens").toDouble() : loss.value("s").toDouble();
        head->addWidget(label(m_order == "tokens" ? tokens(qint64(amount)) : duration(qint64(amount)), c.fg, 13, true, "teamProfileLossValue"));
        v->addLayout(head);
        if (const QString detail = lossDetail(loss); !detail.isEmpty()) { auto *d = label(detail, c.text2, 12); d->setWordWrap(true); v->addWidget(d); }
        auto *share = new QFrame; share->setFixedHeight(4);
        share->setStyleSheet(QStringLiteral("background:qlineargradient(x1:0,y1:0,x2:1,y2:0,stop:0 %1,stop:%2 %1,stop:%3 %4,stop:1 %4);border-radius:2px;")
                                 .arg(c.warn.name()).arg(std::clamp(amount / std::max(1.0, top), 0.0, 1.0)).arg(std::clamp(amount / std::max(1.0, top) + .001, 0.0, 1.0))
                                 .arg(alpha(c.border, .8).name(QColor::HexArgb)));
        v->addWidget(share);
        auto *chips = row(6);
        if (const int task = loss.value("task").toInt()) {
            auto *chip = button(QStringLiteral("#%1").arg(task), "teamProfileLossTask"); chip->setProperty("lossChip", true); chip->setProperty("task", task);
            QObject::connect(chip, &QPushButton::clicked, this, [this, task] { pick(task); });
            chips->addWidget(chip);
        }
        for (const auto &id : loss.value("on").toArray()) {
            auto *chip = button(QStringLiteral("#%1").arg(id.toInt()), "teamProfileLossTask"); chip->setProperty("lossChip", true); chip->setProperty("task", id.toInt());
            QObject::connect(chip, &QPushButton::clicked, this, [this, task = id.toInt()] { pick(task); });
            chips->addWidget(chip);
        }
        if (loss.value("kind") == "idle_member") {
            auto *chip = button(tr("%1 lane").arg(loss.value("member").toString()), "teamProfileLossLane"); chip->setProperty("lossChip", true);
            QObject::connect(chip, &QPushButton::clicked, this, [this, member = loss.value("member").toString()] {
                if (m_tab != "time") setTab(QStringLiteral("time"));
                if (m_timeline) if (auto *scroll = qobject_cast<QScrollArea *>(m_timeline->parentWidget() ? m_timeline->parentWidget()->parentWidget() : nullptr))
                    scroll->ensureVisible(0, m_timeline->laneY(member) + 26, 0, 60);
            });
            chips->addWidget(chip);
        }
        chips->addStretch(1);
        v->addLayout(chips);
        m_lossList->addWidget(card);
        if (n >= Cache::LossPage) break;
    }
    m_lossList->addStretch(1);
}

void TeamProfileView::exportJson()
{
    QString path = exportPath ? exportPath()
                              : QFileDialog::getSaveFileName(this, tr("Export the profile"), QStringLiteral("profile-%1-%2.json").arg(m_team.id.left(8), scope().replace(':', '-')),
                                                             tr("JSON (*.json)"));
    if (path.isEmpty()) return;
    const auto *entry = m_cache->entry(m_team.id, scope(), m_team.revision);
    QJsonObject out{{"team", m_team.id}, {"scope", scope()}};
    if (entry) { out.insert("profile", entry->profile); out.insert("spans", entry->timelines); if (!entry->compare.isEmpty()) out.insert("compare", entry->compare); }
    QFile file(path);
    if (file.open(QIODevice::WriteOnly | QIODevice::Truncate)) file.write(QJsonDocument(out).toJson(QJsonDocument::Indented));
    m_state->setText(file.error() == QFile::NoError ? tr("Exported to %1").arg(QDir::toNativeSeparators(path)) : tr("Not exported: %1").arg(file.errorString()));
}
