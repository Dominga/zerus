#include "TeamTests.h"
#include "NewSessionDense.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QDateTime>
#include <QHBoxLayout>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QVBoxLayout>

using namespace Teams;
using namespace TeamUi;

namespace TeamTests {

Run Run::parse(const QJsonObject &record)
{
    Run run;
    run.id = record.value("id").toString();
    run.suite = record.value("suite").toString();
    run.status = record.value("status").toString();
    run.askedBy = record.value("asked_by").toString();
    run.commit = record.value("commit").toString();
    run.excerpt = record.value("excerpt").toString();
    run.task = record.value("task").toInt();
    run.at = record.value("at").toDouble();
    run.durationMs = qint64(record.value("duration_ms").toDouble());
    for (const auto &target : record.value("affected").toArray()) run.affected << target.toString();
    // #718's marks, read when a record carries them.
    run.reused = record.value("reused").toBool() || run.status == QLatin1String("reused");
    run.reusedFrom = record.value("reused_from").toString();
    run.overBudget = record.value("over_budget").toBool();
    run.budgetMinutes = record.value("budget_minutes").toDouble();
    const auto flaky = record.value("flaky").toObject();
    run.flakyRuns = flaky.value("runs").toInt();
    run.flakyFailed = flaky.value("failed").toInt();
    return run;
}

Pending Pending::parse(const QJsonObject &item)
{
    Pending pending;
    pending.id = item.value("id").toString();
    pending.suite = item.value("suite").toString();
    pending.askedBy = item.value("asked_by").toString();
    pending.state = item.value("state").toString();
    pending.resources = item.value("resources").toString();
    pending.task = item.value("task").toInt();
    pending.forced = item.value("forced").toBool();
    pending.at = item.value("at").toDouble();
    pending.started = item.value("started").toDouble();
    pending.seconds = item.value("seconds").toDouble();
    pending.budgetMinutes = item.value("budget_minutes").toDouble();
    return pending;
}

QString runTime(qint64 ms)
{
    const qint64 seconds = (ms + 500) / 1000;
    if (ms > 0 && ms < 1000) return QStringLiteral("%1 ms").arg(ms);
    if (seconds < 60) return QStringLiteral("%1s").arg(seconds);
    if (seconds < 3600) return QStringLiteral("%1m %2s").arg(seconds / 60).arg(seconds % 60, 2, 10, QLatin1Char('0'));
    return QStringLiteral("%1h %2m").arg(seconds / 3600).arg(seconds / 60 % 60, 2, 10, QLatin1Char('0'));
}

QString statusName(const QString &status)
{
    if (status == QLatin1String("passed")) return QObject::tr("Passed");
    if (status == QLatin1String("failed")) return QObject::tr("Failed");
    if (status == QLatin1String("timeout")) return QObject::tr("Timed out");
    if (status == QLatin1String("reused")) return QObject::tr("Reused");
    return status.isEmpty() ? QObject::tr("Unknown") : status;
}

}   // namespace TeamTests

using namespace TeamTests;

namespace {

// A status as the team views draw it: the glyph and its tone.
std::pair<QString, QColor> statusLook(const QString &status, const Colors &c)
{
    if (status == QLatin1String("passed") || status == QLatin1String("reused")) return {QStringLiteral("done"), c.accent};
    if (status == QLatin1String("failed")) return {QStringLiteral("failed"), c.danger};
    if (status == QLatin1String("timeout")) return {QStringLiteral("late"), c.warn};
    if (status == QLatin1String("running")) return {QStringLiteral("running"), c.accent};
    return {QStringLiteral("pending"), c.muted};
}

QLabel *glyphLabel(const QString &status, const Colors &c)
{
    const auto [kind, tone] = statusLook(status, c);
    auto *icon = new QLabel; icon->setPixmap(glyphPixmap(tone, kind, 14)); icon->setFixedSize(16, 16);
    icon->setToolTip(statusName(status));
    return icon;
}

// A suite's latest runs, oldest left: one painted square each.
class StatusStrip : public QWidget {
public:
    StatusStrip(const QStringList &statuses, const Colors &c, QWidget *parent = nullptr) : QWidget(parent), m_statuses(statuses), m_c(c)
    {
        setObjectName("teamTestsStrip");
        setFixedSize(int(statuses.size()) * 9 + 2, 14);
        QStringList names; for (const auto &status : statuses) names << statusName(status);
        setToolTip(QObject::tr("Latest runs, oldest first: %1").arg(names.join(QStringLiteral(", "))));
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        int x = 1;
        for (const auto &status : m_statuses) {
            p.setPen(Qt::NoPen); p.setBrush(statusLook(status, m_c).second);
            p.drawRoundedRect(QRectF(x, 3, 7, 8), 2, 2);
            x += 9;
        }
    }
private:
    QStringList m_statuses;
    Colors m_c;
};

// A row's frame under its own name, styled as the team cards are.
QFrame *box(const Colors &c, const char *name, const QColor &border = {})
{
    auto *frame = new QFrame; frame->setObjectName(name);
    frame->setStyleSheet(QStringLiteral("QFrame#%1{background:%2;border:1px solid %3;border-radius:9px;}")
                             .arg(QLatin1String(name), c.surface.name(QColor::HexArgb), (border.isValid() ? border : c.border).name(QColor::HexArgb)));
    return frame;
}

QLabel *section(const QString &text, const Colors &c, const char *name)
{
    auto *l = label(text, c.text3, 12, true, name);
    l->setContentsMargins(0, 10, 0, 2);
    return l;
}

QString clockOf(double at)
{
    const auto when = QDateTime::fromSecsSinceEpoch(qint64(at));
    return when.date() == QDate::currentDate() ? when.toString(QStringLiteral("HH:mm")) : when.toString(QStringLiteral("d MMM HH:mm"));
}

}   // namespace

TeamTestsView::TeamTestsView(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamTestsView");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0);
    auto *scroll = new QScrollArea; scroll->setObjectName("teamTestsScroll");
    scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *page = new QWidget; page->setObjectName("teamTestsPage");
    m_body = new QVBoxLayout(page); m_body->setContentsMargins(20, 14, 20, 20); m_body->setSpacing(6);
    scroll->setWidget(page);
    outer->addWidget(scroll);
    m_poll.setInterval(PollMs);
    connect(&m_poll, &QTimer::timeout, this, &TeamTestsView::reload);
}

void TeamTestsView::setDark(bool dark)
{
    if (m_dark == dark) return;
    m_dark = dark;
    render();
}

void TeamTestsView::setTeam(const Team &team)
{
    const bool other = team.id != m_teamId;
    m_team = team;
    if (other) {
        m_teamId = team.id; ++m_generation;
        m_pending.clear(); m_runs.clear(); m_open.clear(); m_error.clear(); m_forcing.clear(); m_forceError.clear();
        m_read = false; m_hasMore = false; m_pages = 0; m_queueLoading = m_resultsLoading = false;
        render();
    }
    if (!m_read && isVisible()) reload();
}

void TeamTestsView::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_poll.start();
    if (!m_teamId.isEmpty()) reload();
}

void TeamTestsView::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_poll.stop();
}

void TeamTestsView::reload()
{
    if (m_teamId.isEmpty()) return;
    m_read = true;
    readQueue();
    readResults(false);
}

void TeamTestsView::showRecord(const QString &id)
{
    m_open = id;
    render();
}

void TeamTestsView::readQueue()
{
    if (m_queueLoading) return;
    m_queueLoading = true;
    QPointer<TeamTestsView> self(this);
    const quint64 generation = m_generation;
    m_teams->call(QStringLiteral("test"), {{"team", m_teamId}, {"action", "queue"}}, [self, generation](bool ok, const QJsonObject &result, const QString &error) {
        if (!self || self->m_generation != generation) return;
        self->m_queueLoading = false;
        if (!ok) { self->m_error = error; self->render(); return; }
        QList<Pending> pending;
        for (const auto &item : result.value("items").toArray()) pending << Pending::parse(item.toObject());
        bool same = pending.size() == self->m_pending.size() && self->m_error.isEmpty();
        for (int i = 0; same && i < pending.size(); ++i)
            same = pending[i].id == self->m_pending[i].id && pending[i].state == self->m_pending[i].state;
        self->m_pending = pending; self->m_error.clear();
        if (!same) self->render();
    }, false);
}

// The newest page on every poll (it replaces what was read when it changed);
// More reads the page before the oldest run shown, at most MaxPages pages.
void TeamTestsView::readResults(bool more)
{
    if (m_resultsLoading) return;
    m_resultsLoading = true;
    QJsonObject input{{"team", m_teamId}, {"action", "results"}, {"limit", Page}};
    if (more && !m_runs.isEmpty()) input.insert("before", m_runs.last().at);
    QPointer<TeamTestsView> self(this);
    const quint64 generation = m_generation;
    m_teams->call(QStringLiteral("test"), input, [self, generation, more](bool ok, const QJsonObject &result, const QString &error) {
        if (!self || self->m_generation != generation) return;
        self->m_resultsLoading = false;
        if (!ok) { self->m_error = error; self->render(); return; }
        QList<Run> page;
        for (const auto &item : result.value("items").toArray()) page << Run::parse(item.toObject());
        const bool full = page.size() >= Page;
        if (more) {
            self->m_runs += page; ++self->m_pages;
            self->m_hasMore = full && self->m_pages < MaxPages;
            self->render();
            return;
        }
        const bool same = self->m_error.isEmpty() && !self->m_runs.isEmpty() && !page.isEmpty() && self->m_runs.first().id == page.first().id
                          && self->m_runs.size() >= page.size();
        if (same) return;
        self->m_runs = page; self->m_pages = 1; self->m_hasMore = full; self->m_error.clear();
        self->render();
    }, false);
}

// Force run: the suite goes to the queue as forced; the worker runs it first,
// at its next tick and in the day too, and never reuses an earlier result.
void TeamTestsView::force(const QString &suite)
{
    if (m_forcing.contains(suite)) return;
    m_forcing.insert(suite); m_forceError.remove(suite);
    render();
    QPointer<TeamTestsView> self(this);
    const quint64 generation = m_generation;
    m_teams->call(QStringLiteral("test"), {{"team", m_teamId}, {"action", "submit"}, {"suite", suite}, {"forced", true}, {"asked_by", "You"}},
                  [self, generation, suite](bool ok, const QJsonObject &, const QString &error) {
        if (!self || self->m_generation != generation) return;
        self->m_forcing.remove(suite);
        if (!ok) self->m_forceError.insert(suite, error);
        self->render();
        if (ok) self->readQueue();
    }, true);
}

QWidget *TeamTestsView::taskLink(int task)
{
    const Colors c = colorsFor(m_dark);
    const Task *found = m_team.task(task);
    QString text = QStringLiteral("#%1").arg(task);
    if (found) text += QLatin1Char(' ') + found->title;
    auto *link = new QPushButton(text);
    link->setObjectName("teamTestsTask"); link->setProperty("task", task);
    link->setFlat(true); link->setCursor(Qt::PointingHandCursor); link->setToolTip(text);
    link->setStyleSheet(QStringLiteral("QPushButton{color:%1;border:none;padding:0;text-align:left;}").arg(c.accent.name()));
    link->setMaximumWidth(260);
    link->setText(link->fontMetrics().elidedText(text, Qt::ElideRight, 250));
    connect(link, &QPushButton::clicked, this, [this, task] { if (openTask) openTask(task); });
    return link;
}

QWidget *TeamTestsView::runningRow(const Pending &item)
{
    const Colors c = colorsFor(m_dark);
    auto *frame = box(c, "teamTestsRunning", alpha(c.accent, .5)); frame->setProperty("request", item.id);
    auto *h = new QHBoxLayout(frame); h->setContentsMargins(10, 7, 10, 7); h->setSpacing(8);
    h->addWidget(glyphLabel(QStringLiteral("running"), c));
    h->addWidget(label(item.suite, c.fg, 13, true, "teamTestsSuiteName"));
    if (item.task) h->addWidget(taskLink(item.task));
    const double now = double(QDateTime::currentSecsSinceEpoch());
    h->addWidget(label(item.started > 0 ? tr("running %1").arg(runTime(qint64((now - item.started) * 1000))) : tr("running"), c.text2, 12));
    if (item.seconds > 0) h->addWidget(label(tr("usually %1").arg(runTime(qint64(item.seconds * 1000))), c.muted, 11));
    h->addStretch();
    if (!item.askedBy.isEmpty()) h->addWidget(label(tr("asked by %1").arg(item.askedBy), c.muted, 11));
    return frame;
}

QWidget *TeamTestsView::queueRow(const Pending &item)
{
    const Colors c = colorsFor(m_dark);
    auto *frame = box(c, "teamTestsQueued"); frame->setProperty("request", item.id);
    auto *h = new QHBoxLayout(frame); h->setContentsMargins(10, 6, 10, 6); h->setSpacing(8);
    h->addWidget(glyphLabel(QStringLiteral("queued"), c));
    h->addWidget(label(item.suite, c.fg, 13, true, "teamTestsSuiteName"));
    if (item.forced) {
        auto *forced = new Pill(tr("Forced"), c.warn); forced->setObjectName("teamTestsForced");
        forced->setToolTip(tr("Runs first, at day too, and never reuses an earlier result"));
        h->addWidget(forced);
    }
    if (item.task) h->addWidget(taskLink(item.task));
    h->addWidget(label(tr("waiting since %1").arg(clockOf(item.at)), c.text2, 12));
    if (item.seconds > 0) h->addWidget(label(tr("about %1").arg(runTime(qint64(item.seconds * 1000))), c.muted, 11));
    if (!item.resources.isEmpty()) {
        auto *cls = label(item.resources, c.muted, 11, false, "teamTestsClass");
        cls->setToolTip(tr("The suite's resource class: the scheduler runs it when the machine has room"));
        h->addWidget(cls);
    }
    h->addStretch();
    if (!item.askedBy.isEmpty()) h->addWidget(label(tr("asked by %1").arg(item.askedBy), c.muted, 11));
    return frame;
}

QWidget *TeamTestsView::suiteRow(const QString &suite)
{
    const Colors c = colorsFor(m_dark);
    QList<const Run *> runs;
    for (const auto &run : m_runs) if (run.suite == suite) runs << &run;
    auto *frame = box(c, "teamTestsSuite"); frame->setProperty("suite", suite);
    auto *h = new QHBoxLayout(frame); h->setContentsMargins(10, 6, 10, 6); h->setSpacing(8);
    const Run *latest = runs.isEmpty() ? nullptr : runs.first();
    h->addWidget(glyphLabel(latest ? latest->status : QStringLiteral("queued"), c));
    h->addWidget(label(suite, c.fg, 13, true, "teamTestsSuiteName"));
    if (latest) {
        h->addWidget(label(statusName(latest->status), statusLook(latest->status, c).second, 12));
        h->addWidget(label(runTime(latest->durationMs), c.text2, 12));
        QStringList statuses;
        for (int i = std::min<int>(10, int(runs.size())) - 1; i >= 0; --i) statuses << runs[i]->status;
        h->addWidget(new StatusStrip(statuses, c));
        if (latest->flaky()) {
            auto *flaky = new Pill(tr("Flaky"), c.warn); flaky->setObjectName("teamTestsSuiteFlaky");
            flaky->setToolTip(tr("%1 of the last %2 runs failed").arg(latest->flakyFailed).arg(latest->flakyRuns));
            h->addWidget(flaky);
        }
        if (latest->overBudget) {
            auto *over = new Pill(tr("Over budget"), c.danger); over->setObjectName("teamTestsSuiteOverBudget");
            over->setToolTip(tr("Took %1; its budget is %2 min").arg(runTime(latest->durationMs)).arg(latest->budgetMinutes));
            h->addWidget(over);
        }
    } else h->addWidget(label(tr("no result yet"), c.muted, 12));
    h->addStretch();
    if (m_forceError.contains(suite)) {
        auto *refused = refusal(m_forceError.value(suite), c, "teamTestsForceError");
        refused->setMaximumWidth(320); h->addWidget(refused);
    }
    bool waiting = false;
    for (const auto &item : m_pending) waiting = waiting || (item.suite == suite && item.forced);
    auto *force = button(m_forcing.contains(suite) ? tr("Queuing…") : waiting ? tr("Forced, waiting") : tr("Force run"), "teamTestsForce");
    force->setProperty("suite", suite); force->setProperty("ghost", true);
    force->setEnabled(!m_forcing.contains(suite) && !waiting);
    force->setToolTip(tr("Run %1 now: first in the queue, at day too, never a reused result").arg(suite));
    connect(force, &QPushButton::clicked, this, [this, suite] { this->force(suite); });
    h->addWidget(force);
    return frame;
}

QWidget *TeamTestsView::historyRow(const Run &run)
{
    const Colors c = colorsFor(m_dark);
    const bool open = run.id == m_open;
    auto *frame = box(c, "teamTestsRun", open ? c.accent : QColor()); frame->setProperty("record", run.id);
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(10, 6, 10, 6); v->setSpacing(4);
    auto *top = new QPushButton; top->setObjectName("teamTestsRunToggle"); top->setProperty("record", run.id);
    top->setFlat(true); top->setCursor(Qt::PointingHandCursor);
    top->setStyleSheet(QStringLiteral("QPushButton{border:none;padding:0;text-align:left;}"));
    top->setToolTip(open ? tr("Hide the excerpt") : tr("Show the excerpt and the record"));
    auto *h = new QHBoxLayout(top); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
    auto *chevron = new QLabel; chevron->setPixmap(workspaceIcon(open ? "chevron-down" : "chevron-right", c.muted).pixmap(12, 12));
    chevron->setAttribute(Qt::WA_TransparentForMouseEvents); h->addWidget(chevron);
    auto *icon = glyphLabel(run.status, c); icon->setAttribute(Qt::WA_TransparentForMouseEvents); h->addWidget(icon);
    // The toggle's own style sheet would paint its labels in the button's color:
    // each keeps its tone in its own.
    const auto inert = [](QLabel *l) {
        l->setAttribute(Qt::WA_TransparentForMouseEvents);
        l->setStyleSheet(QStringLiteral("color:%1;").arg(l->palette().color(QPalette::WindowText).name(QColor::HexArgb)));
        return l;
    };
    h->addWidget(inert(label(run.suite, c.fg, 13, true, "teamTestsSuiteName")));
    h->addWidget(inert(label(statusName(run.status), statusLook(run.status, c).second, 12, false, "teamTestsStatus")));
    h->addWidget(inert(label(runTime(run.durationMs), c.text2, 12, false, "teamTestsDuration")));
    if (run.reused) {
        auto *reused = new Pill(tr("Reused"), c.folder); reused->setObjectName("teamTestsReused");
        reused->setToolTip(run.reusedFrom.isEmpty() ? tr("The inputs did not change: the last result stands, due for re-measuring")
                                                    : tr("The inputs did not change: the result of %1 stands, due for re-measuring").arg(run.reusedFrom));
        h->addWidget(reused);
    }
    if (run.flaky()) {
        auto *flaky = new Pill(tr("Flaky"), c.warn); flaky->setObjectName("teamTestsFlaky");
        flaky->setToolTip(tr("%1 of the last %2 runs failed").arg(run.flakyFailed).arg(run.flakyRuns));
        h->addWidget(flaky);
    }
    if (run.overBudget) {
        auto *over = new Pill(tr("Over budget"), c.danger); over->setObjectName("teamTestsOverBudget");
        over->setToolTip(tr("Took %1; its budget is %2 min").arg(runTime(run.durationMs)).arg(run.budgetMinutes));
        h->addWidget(over);
    }
    h->addStretch();
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto *when = inert(label(clockOf(run.at), c.text2, 12, false, "teamTestsWhen")); when->setToolTip(agoText(run.at, now));
    h->addWidget(when);
    if (!run.askedBy.isEmpty()) h->addWidget(inert(label(tr("asked by %1").arg(run.askedBy), c.muted, 11)));
    connect(top, &QPushButton::clicked, this, [this, id = run.id] { m_open = m_open == id ? QString() : id; render(); });
    auto *head = row(8); head->addWidget(top, 1);
    if (run.task) head->addWidget(taskLink(run.task));
    v->addLayout(head);
    if (open) {
        auto *meta = label(tr("Record %1").arg(run.id), c.muted, 11, false, "teamTestsRecordId");
        meta->setTextInteractionFlags(Qt::TextSelectableByMouse);
        v->addWidget(meta);
        if (!run.commit.isEmpty()) v->addWidget(label(tr("Commit %1").arg(run.commit.left(12)), c.muted, 11));
        if (!run.affected.isEmpty()) {
            auto *targets = label(tr("Targets: %1").arg(run.affected.join(QStringLiteral(", "))), c.text2, 11, false, "teamTestsTargets");
            targets->setWordWrap(true); v->addWidget(targets);
        }
        auto *excerpt = new QPlainTextEdit(run.excerpt.isEmpty() ? tr("No output was kept.") : run.excerpt);
        excerpt->setObjectName("teamTestsExcerpt"); excerpt->setReadOnly(true); excerpt->setFont(NewSessionDense::mono(11));
        excerpt->setLineWrapMode(QPlainTextEdit::NoWrap);
        excerpt->setStyleSheet(QStringLiteral("QPlainTextEdit{background:%1;color:%2;border:1px solid %3;border-radius:6px;}")
                                   .arg(c.bg.name(), c.fg.name(), c.border.name()));
        const int lines = std::clamp(int(run.excerpt.count(QLatin1Char('\n'))) + 2, 3, 16);
        excerpt->setFixedHeight(excerpt->fontMetrics().lineSpacing() * lines + 14);
        v->addWidget(excerpt);
    }
    return frame;
}

void TeamTestsView::render()
{
    const Colors c = colorsFor(m_dark);
    if (auto *scroll = findChild<QScrollArea *>("teamTestsScroll"))
        scroll->setStyleSheet(QStringLiteral("QScrollArea#teamTestsScroll,QWidget#teamTestsPage{background:%1;}").arg(c.bg.name()));
    if (m_content) { m_body->removeWidget(m_content); retire(m_content); }
    m_content = new QWidget; m_content->setObjectName("teamTestsContent");
    auto *v = new QVBoxLayout(m_content); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    // The header: what runs, what waits and what failed among the runs shown.
    auto *head = row(8);
    head->addWidget(label(tr("Tests"), c.fg, 16, true));
    int running = 0, waiting = 0, failed = 0;
    for (const auto &item : m_pending) (item.running() ? running : waiting)++;
    QSet<QString> seen;
    for (const auto &run : m_runs) {
        if (seen.contains(run.suite)) continue;
        seen.insert(run.suite);
        if (run.status == QLatin1String("failed") || run.status == QLatin1String("timeout")) ++failed;
    }
    const auto count = [&](const QString &text, const QColor &tone, const char *name) {
        auto *pill = new Pill(text, tone); pill->setObjectName(name); head->addWidget(pill);
    };
    count(tr("%n running", nullptr, running), running ? c.accent : c.muted, "teamTestsRunningCount");
    count(tr("%n waiting", nullptr, waiting), waiting ? c.text2 : c.muted, "teamTestsWaitingCount");
    if (failed) count(failed == 1 ? tr("1 suite failing") : tr("%1 suites failing").arg(failed), c.danger, "teamTestsFailingCount");
    head->addStretch();
    v->addLayout(head);
    auto *hint = label(tr("Test requests wait in a queue; Zerus runs them when the machine has room and keeps each result with its excerpt."),
                       c.muted, 11);
    hint->setWordWrap(true); v->addWidget(hint);
    if (!m_error.isEmpty()) v->addWidget(refusal(tr("Could not read the tests: %1").arg(m_error), c, "teamTestsError"));
    if (!m_read || (m_runs.isEmpty() && m_pending.isEmpty() && (m_queueLoading || m_resultsLoading) && m_error.isEmpty())) {
        v->addWidget(label(tr("Reading the tests…"), c.muted, 12, false, "teamTestsLoading"));
    } else {
        QList<Pending> runningItems, queued;
        for (const auto &item : m_pending) (item.running() ? runningItems : queued) << item;
        if (!runningItems.isEmpty()) {
            v->addWidget(section(tr("Running now"), c, "teamTestsRunningTitle"));
            for (const auto &item : runningItems) v->addWidget(runningRow(item));
        }
        v->addWidget(section(tr("Queue"), c, "teamTestsQueueTitle"));
        if (queued.isEmpty()) v->addWidget(label(tr("Nothing waits."), c.muted, 12, false, "teamTestsQueueEmpty"));
        for (const auto &item : queued) v->addWidget(queueRow(item));
        // The suites seen in the queue and the history, by name.
        QStringList suites;
        for (const auto &item : m_pending) if (!suites.contains(item.suite)) suites << item.suite;
        for (const auto &run : m_runs) if (!suites.contains(run.suite)) suites << run.suite;
        std::sort(suites.begin(), suites.end());
        if (!suites.isEmpty()) {
            v->addWidget(section(tr("Suites"), c, "teamTestsSuitesTitle"));
            for (const auto &suite : suites) v->addWidget(suiteRow(suite));
        }
        v->addWidget(section(tr("History"), c, "teamTestsHistoryTitle"));
        if (m_runs.isEmpty()) v->addWidget(label(tr("No test has run for this team yet."), c.muted, 12, false, "teamTestsHistoryEmpty"));
        for (const auto &run : m_runs) v->addWidget(historyRow(run));
        if (m_hasMore) {
            auto *older = button(tr("Older runs"), "teamTestsOlder"); older->setProperty("ghost", true);
            connect(older, &QPushButton::clicked, this, [this] { readResults(true); });
            auto *line = row(); line->addWidget(older); line->addStretch(); v->addLayout(line);
        }
    }
    v->addStretch();
    m_body->addWidget(m_content);
    // Shown at once: a layout shows a new child only after the event loop runs.
    m_content->show();
}
