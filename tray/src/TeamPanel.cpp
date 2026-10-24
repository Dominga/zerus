#include "TeamPanel.h"
#include "TeamDesignerPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QStackedWidget>
#include "TeamController.h"
#include "WorkspaceIcons.h"
#include <QClipboard>
#include <QComboBox>
#include <QCursor>
#include <QGuiApplication>
#include <QInputDialog>
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QToolButton>
#include <QUuid>
#include <QVBoxLayout>
#include <cmath>
#include <tuple>

using namespace Teams;

using namespace TeamUi;

TeamPanel::TeamPanel(TeamController *controller, QWidget *parent) : QWidget(parent), m_controller(controller)
{
    setObjectName("teamPanel");
    setStyleSheet(teamSheet(colorsFor(true)));
    auto *layout = new QVBoxLayout(this);
    m_outer = layout;
    layout->setContentsMargins(0, 0, 0, 0);
    layout->setSpacing(0);
    m_scroll = new QScrollArea;
    m_scroll->setObjectName("teamPanelScroll");
    m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_stack = new QStackedWidget;
    m_stack->addWidget(m_scroll);
    m_card = new TeamTaskCard;
    m_stack->addWidget(m_card);
    m_designer = new TeamDesignerPanel(m_controller);
    m_designer->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm(name, title, text, action); };
    m_stack->addWidget(m_designer);
    layout->addWidget(m_stack);
    m_card->back = [this] { m_openTask = 0; render(); };
    m_card->act = [this](const QString &operation, const QJsonObject &input) { act(operation, input); };
    m_card->openTask = [this](int task) { openTask(task); };
    m_card->openSession = [this](const QString &host, const QString &session) { if (openSession) openSession(host, session); };
    m_card->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm(name, title, text, action); };
    m_card->reassign = [this](int task, const QPoint &global) {
        if (const auto *team = m_controller->watched()) reassignMenu(*team, task, global);
    };
    m_card->problemActions = [this](const Member &member, const Status &status) -> QWidget * {
        const auto *team = m_controller->watched();
        return team ? problemActions(*team, member, status) : nullptr;
    };
    QObject::connect(m_controller, &TeamController::teamsChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_controller, &TeamController::watchedChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_controller, &TeamController::feedChanged, this, [this] { scheduleRender(); });
    render();
}

void TeamPanel::setTheme(bool dark)
{
    m_dark = dark;
    setStyleSheet(teamSheet(colorsFor(dark)));   // accent primary buttons, segments, fields
    m_designer->setDark(dark);
    scheduleRender();
}

void TeamPanel::showDesigner(bool on)
{
    if (m_designerShown == on) return;
    m_designerShown = on;
    m_controller->setWatchCatalog(on);
    if (on) { m_designer->render(); m_stack->setCurrentWidget(m_designer); if (answersChanged) answersChanged(0); }
    else render();
}

void TeamPanel::openTask(int task)
{
    m_openTask = task;
    render();
}

void TeamPanel::showTeam(const QString &team)
{
    if (m_team == team) return;
    m_team = team; m_showBlockers = false; m_feedMember.clear(); m_notice.clear(); m_openTask = 0;
    m_controller->watch(team);
    render();
}

void TeamPanel::scheduleRender()
{
    if (m_pending) return;
    m_pending = true;
    QTimer::singleShot(0, this, [this] { m_pending = false; render(); });
}

void TeamPanel::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    // The graph needs 400px; below that the list by layer replaces it.
    static const int threshold = 400;
    if (m_content && (m_content->property("narrow").toBool() != (width() < threshold))) scheduleRender();
}

bool TeamPanel::confirm(const QString &name, const QString &title, const QString &text, const QString &action)
{
    if (findChild<QMessageBox *>(name)) return false;
    QMessageBox dialog(QMessageBox::Warning, title, text, QMessageBox::NoButton, this);
    dialog.setObjectName(name); dialog.setTextFormat(Qt::PlainText);
    auto *ok = dialog.addButton(action, QMessageBox::DestructiveRole);
    auto *cancel = dialog.addButton(QMessageBox::Cancel);
    dialog.setDefaultButton(cancel); dialog.setEscapeButton(cancel);
    dialog.exec();
    return dialog.clickedButton() == ok;
}

void TeamPanel::act(const QString &operation, const QJsonObject &input)
{
    QJsonObject body = input;
    body.insert("team", m_team);
    m_notice.clear();
    m_controller->act(operation, body, [this, guard = QPointer<TeamPanel>(this)](bool ok, const QString &message) {
        if (!guard) return;
        m_notice = ok ? QString() : message;
        scheduleRender();
    });
}

void TeamPanel::remind(const Team &team, const Member &member, int taskId)
{
    const auto *task = team.task(taskId);
    const auto *attempt = task ? task->last() : nullptr;
    if (!task || !attempt || !sendMessage) return;
    const QJsonObject report{{"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)}, {"team", team.id},
                             {"task", task->id}, {"attempt", attempt->id}, {"text", "..."}};
    const QString text = tr("[Zerus team · from: You · task: #%1 · attempt: %2]\nYour turn ended without a report for task #%1 %3. "
                            "Report once with JSON on stdin:\n  hgs %4team report --json\n  %5")
                             .arg(task->id).arg(attempt->n).arg(task->title, member.host.isEmpty() ? QString() : QStringLiteral("@HOST "),
                                  QString::fromUtf8(QJsonDocument(report).toJson(QJsonDocument::Compact)));
    sendMessage(member.host, member.session, text);
}

void TeamPanel::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    if (m_dirty) render();
}

void TeamPanel::render()
{
    // A hidden tab renders once it is shown; polls keep coming meanwhile.
    if (!isVisible() && m_content) { m_dirty = true; return; }
    if (m_designerShown) { m_stack->setCurrentWidget(m_designer); return; }
    m_dirty = false;
    const Colors c = colorsFor(m_dark);
    const int scroll = m_scroll->verticalScrollBar()->value();
    const bool narrow = width() < 400;
    auto *content = new QWidget;
    content->setObjectName("teamPanelContent");
    content->setProperty("narrow", narrow);
    auto *layout = new QVBoxLayout(content);
    layout->setContentsMargins(narrow ? 8 : 12, narrow ? 8 : 12, narrow ? 8 : 12, 12);
    layout->setSpacing(12);
    const Team *team = m_controller->watched();
    if (!team && !m_team.isEmpty()) team = m_controller->team(m_team);
    if (!team && m_team.isEmpty()) {
        layout->addWidget(emptyState(narrow));
        if (answersChanged) answersChanged(0);
    } else if (!team) {
        layout->addWidget(label(tr("Loading team…"), c.text2, 13, true, "teamEmptyTitle"));
        if (answersChanged) answersChanged(0);
    } else if (team->status != "planning") {
        // Running and later: a short summary; the work is in the Team tab (D4).
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        layout->addWidget(summary(*team, status, narrow));
        if (!m_notice.isEmpty()) {
            auto *notice = label(m_notice, c.danger, 12, false, "teamNotice");
            notice->setWordWrap(true);
            layout->addWidget(notice);
        }
        const int waiting = Teams::answers(*team, live);
        if (waiting) layout->addWidget(answers(*team));
        layout->addWidget(summaryMembers(*team, narrow));
        auto *foot = label(tr("Plan, graph and member changes live in the Team tab."), c.dim, 11); foot->setWordWrap(true);
        layout->addWidget(foot);
        if (answersChanged) answersChanged(waiting);
    } else {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        layout->addWidget(header(*team, status, narrow));
        if (!m_notice.isEmpty()) {
            auto *notice = label(m_notice, c.danger, 12, false, "teamNotice");
            notice->setWordWrap(true);
            layout->addWidget(notice);
        }
        layout->addWidget(planning(*team));
        const int waiting = Teams::answers(*team, live);
        if (waiting) layout->addWidget(answers(*team));
        layout->addWidget(planMembers(*team, narrow));
        layout->addWidget(tasks(*team, narrow));
        if (team->full) layout->addWidget(feed(*team));
        if (answersChanged) answersChanged(waiting);
    }
    layout->addStretch();
    retire(m_scroll->takeWidget());   // a click inside it may be running
    m_scroll->setWidget(content);
    m_content = content;
    m_scroll->verticalScrollBar()->setValue(scroll);
    // While planning, the plan's actions stay pinned below the content.
    if (m_footer) { retire(m_footer); m_footer = nullptr; }
    if (team && team->status == "planning" && !m_openTask) { m_footer = planFooter(*team); m_outer->addWidget(m_footer); }
    if (team && m_openTask) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        m_card->setTask(*team, m_openTask, live, m_dark);
        m_stack->setCurrentWidget(m_card);
    } else m_stack->setCurrentWidget(m_scroll);
}

QWidget *TeamPanel::emptyState(bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c); box->setObjectName("teamEmpty");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(narrow ? 12 : 20, 20, narrow ? 12 : 20, 20); v->setSpacing(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.text2).pixmap(36, 36)); icon->setAlignment(Qt::AlignCenter);
    v->addWidget(icon);
    auto *title = label(tr("No team in this session"), c.fg, 13, true, "teamEmptyTitle"); title->setAlignment(Qt::AlignCenter);
    v->addWidget(title);
    auto *hint = label(tr("Build a team around a goal with this session as the coordinator, or set up roles and templates first."), c.muted, 12);
    hint->setWordWrap(true); hint->setAlignment(Qt::AlignCenter); hint->setObjectName("teamEmptyHint");
    v->addWidget(hint);
    if (newTeam) {
        auto *create = button(tr("New team…"), "teamEmptyNew", true);
        QObject::connect(create, &QPushButton::clicked, this, [this] { newTeam(); });
        v->addWidget(create);
    }
    if (openDesigner) {
        auto *designer = button(tr("Team designer…"), "teamEmptyDesigner");
        QObject::connect(designer, &QPushButton::clicked, this, [this] { openDesigner(); });
        v->addWidget(designer);
    }
    return box;
}

QWidget *TeamPanel::summary(const Team &team, const QString &status, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *box = card(c); box->setObjectName("teamCard");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(7);
    auto *top = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); top->addWidget(icon);
    auto *name = elided(team.name, c.fg, 14); QFont f = name->font(); f.setWeight(QFont::DemiBold); name->setFont(f);
    name->setObjectName("teamName");
    top->addWidget(name, 1);
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false);
    badge->setObjectName("teamStatusBadge");
    top->addWidget(badge);
    v->addLayout(top);
    QList<QColor> segments;
    for (auto tone : progress(team)) segments.append(toneColor(c, tone));
    v->addWidget(new Progress(segments, 6, c.border));
    const int problemCount = problems(team, live).size();
    QString meta = tr("%1/%2 accepted").arg(accepted(team)).arg(team.tasks.size()).toHtmlEscaped();
    if (problemCount) meta += QStringLiteral(" &nbsp;<span style=\"color:%1;\">%2</span>").arg(c.warn.name(),
        problemCount == 1 ? tr("1 problem") : tr("%1 problems").arg(problemCount));
    if (team.created > 0) meta += QStringLiteral(" &nbsp;· %1").arg(duration(QDateTime::currentSecsSinceEpoch() - team.created).toHtmlEscaped());
    auto *metaLabel = new QLabel(meta); metaLabel->setTextFormat(Qt::RichText); metaLabel->setObjectName("teamLegend");
    metaLabel->setStyleSheet(QStringLiteral("color:%1;font-size:11px;").arg(c.text3.name()));
    v->addWidget(metaLabel);
    if (status == "stuck") {
        auto *stuckLine = label(tr("Stuck — nothing can start; see the Team tab."), c.warn, 12, false, "teamStuck"); stuckLine->setWordWrap(true);
        v->addWidget(stuckLine);
    }
    auto *open = button(tr("Open Team"), "teamOpenTeam", true);
    QObject::connect(open, &QPushButton::clicked, this, [this] { if (openWorkspace) openWorkspace(); });
    v->addWidget(open);
    Q_UNUSED(narrow);
    return box;
}

QWidget *TeamPanel::summaryMembers(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *box = new QWidget; box->setObjectName("teamSummaryMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(2);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.members.size()), c.muted, 12));
    title->addStretch();
    v->addLayout(title);
    for (const auto &m : team.members) {
        const auto status = memberStatus(team, m, live(m));
        auto *line = new QWidget; line->setObjectName("teamSummaryMember"); line->setFixedHeight(28);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(2, 0, 2, 0); h->setSpacing(6);
        auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(m.agent), c.fg).pixmap(16, 16)); h->addWidget(tile);
        const QString role = team.roleOf(m.id);
        h->addWidget(new Pill(role, roleColor(role, m_dark)));
        if (!narrow) h->addWidget(elided(m.model.isEmpty() ? m.agent : m.model, c.text3, 11), 1);
        else h->addStretch();
        auto *mark = new StatusMark(status.text, toneColor(c, status.tone), status.kind);
        mark->setMaximumWidth(narrow ? 120 : 170);
        h->addWidget(mark);
        v->addWidget(line);
    }
    return box;
}

QWidget *TeamPanel::header(const Team &team, const QString &status, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c);
    box->setObjectName("teamCard");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(7);
    auto *top = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); top->addWidget(icon);
    auto *name = elided(team.name, c.fg, 14); QFont f = name->font(); f.setWeight(QFont::DemiBold); name->setFont(f);
    name->setObjectName("teamName");
    top->addWidget(name, 1);
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false);
    badge->setObjectName("teamStatusBadge");
    top->addWidget(badge);
    auto *expand = new QToolButton; expand->setObjectName("teamOpenWorkspace"); expand->setIcon(workspaceIcon("external", c.fg)); expand->setAutoRaise(true);
    expand->setToolTip(tr("Open the team workspace"));
    QObject::connect(expand, &QToolButton::clicked, this, [this] { if (openWorkspace) openWorkspace(); });
    top->addWidget(expand);
    auto *more = new QToolButton; more->setObjectName("teamMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    menu->addAction(tr("Copy team ID"), this, [id = team.id] { QGuiApplication::clipboard()->setText(id); });
    if (QStringList{"running", "paused", "planning"}.contains(team.status))
        menu->addAction(tr("Stop…"), this, [this, name = team.name] {
            if (confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                 "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                act("stop", {});
        });
    more->setMenu(menu);
    top->addWidget(more);
    v->addLayout(top);
    QList<QColor> segments;
    for (auto tone : progress(team)) segments.append(toneColor(c, tone));
    v->addWidget(new Progress(segments, 6, c.border));
    QMap<Tone, int> counts;
    for (const auto &task : team.tasks) ++counts[taskStatus(task).tone];
    QStringList legend;
    legend << tr("%1/%2 accepted").arg(accepted(team)).arg(team.tasks.size());
    if (counts.value(Tone::Active) + counts.value(Tone::Assigned)) legend << tr("Active %1").arg(counts.value(Tone::Active) + counts.value(Tone::Assigned));
    if (counts.value(Tone::Danger) + counts.value(Tone::Warn)) legend << tr("Problems %1").arg(counts.value(Tone::Danger) + counts.value(Tone::Warn));
    int pending = 0; for (const auto &task : team.tasks) pending += task.status == "pending";
    if (pending) legend << tr("Pending %1").arg(pending);
    auto *legendLabel = label(legend.join(QStringLiteral("  ·  ")), c.text3, 11, false, "teamLegend");
    legendLabel->setWordWrap(true);
    v->addWidget(legendLabel);
    QStringList meta;
    if (team.created > 0) meta << tr("%1 elapsed").arg(duration(QDateTime::currentSecsSinceEpoch() - team.created));
    if (!team.project.isEmpty()) meta << team.project;
    else if (!team.folder.isEmpty()) meta << team.folder;
    if (!meta.isEmpty()) v->addWidget(elided(meta.join(QStringLiteral(" · ")), c.muted, 11));
    if (const auto next = nextLine(team); !next.isEmpty()) {
        auto *nextLabel = elided(next, c.text2, 12); nextLabel->setObjectName("teamNext");
        v->addWidget(nextLabel);
    }
    auto *actions = row(6);
    if (team.status == "running") {
        auto *pause = button(tr("Pause"), "teamPause");
        pause->setToolTip(tr("Running turns finish; nothing new is handed out"));
        QObject::connect(pause, &QPushButton::clicked, this, [this] { act("pause", {}); });
        actions->addWidget(pause);
    } else if (team.status == "paused") {
        auto *resume = button(tr("Resume"), "teamResume", true);
        QObject::connect(resume, &QPushButton::clicked, this, [this] { act("resume", {}); });
        actions->addWidget(resume);
    }
    if (team.status == "running" || team.status == "paused") {
        auto *stop = button(tr("Stop…"), "teamStop");
        QObject::connect(stop, &QPushButton::clicked, this, [this, name = team.name] {
            if (confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                 "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                act("stop", {});
        });
        actions->addWidget(stop);
    }
    if (team.status == "done" || team.status == "stopped") {
        auto *archive = button(tr("Archive"), "teamArchive");
        QObject::connect(archive, &QPushButton::clicked, this, [this, name = team.name] {
            if (confirm("teamArchiveConfirm", tr("Archive team?"), tr("Archive %1? Its history and feed are kept; it leaves the session list.").arg(name), tr("Archive")))
                act("archive", {});
        });
        actions->addWidget(archive);
    }
    actions->addStretch();
    if (actions->count() > 1) v->addLayout(actions); else delete actions;
    if (team.status == "paused") {
        auto *paused = label(tr("Paused — running turns finish, nothing new is handed out"), c.muted, 11);
        paused->setWordWrap(true);
        v->addWidget(paused);
    }
    Q_UNUSED(narrow);
    return box;
}

QWidget *TeamPanel::planning(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamPlanning");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    auto *banner = card(c, alpha(c.accent, .6));
    banner->setStyleSheet(banner->styleSheet() + QStringLiteral("QFrame#teamCard{background:%1;}").arg(alpha(c.accent, .08).name(QColor::HexArgb)));
    auto *b = new QVBoxLayout(banner); b->setContentsMargins(10, 8, 10, 8); b->setSpacing(3);
    b->addWidget(label(tr("Plan from Lead — review before running"), c.fg, 12, true));
    auto *hint = label(tr("Edit members and tasks here; drag a node's handle onto another task to add a dependency, click a dependency "
                          "to remove it. Members are created only after Approve & Run."), c.text2, 12);
    hint->setWordWrap(true); b->addWidget(hint);
    v->addWidget(banner);
    if (!team.errors.isEmpty()) {
        auto *errors = card(c, c.danger);
        errors->setObjectName("teamPlanErrorsBox");
        auto *e = new QVBoxLayout(errors); e->setContentsMargins(10, 8, 10, 8); e->setSpacing(3);
        e->addWidget(new StatusMark(tr("%n plan error(s)", nullptr, int(team.errors.size())), c.fg, "stuck"));
        auto *count = label(tr("%n plan error(s)", nullptr, int(team.errors.size())), c.danger, 12, true, "teamPlanErrors");
        count->hide(); e->addWidget(count);
        for (const auto &error : team.errors) { auto *l = label(QStringLiteral("• ") + error, c.danger, 12); l->setWordWrap(true); e->addWidget(l); }
        v->addWidget(errors);
    }
    for (const auto &warning : team.warnings) { auto *l = label(QStringLiteral("⚠ ") + warning, c.warn, 12); l->setWordWrap(true); v->addWidget(l); }
    return w;
}

QWidget *TeamPanel::stuck(const Team &team, const QList<Problem> &problems)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c, c.warn);
    box->setObjectName("teamStuck");
    box->setStyleSheet(box->styleSheet() + QStringLiteral("QFrame#teamStuck{background:%1;}").arg(alpha(c.warn, .1).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(5);
    auto *title = row(6);
    auto *mark = new StatusMark(tr("Team stuck — nothing can start"), c.fg, "stuck");
    title->addWidget(mark, 1); v->addLayout(title);
    int open = 0; for (const auto &task : team.tasks) open += !task.finished() && task.status != "draft";
    v->addWidget(label(problems.isEmpty() ? tr("%n open task(s) wait; none can start.", nullptr, open)
                                          : tr("%1 open tasks wait on %2 problems:").arg(open).arg(problems.size()), c.text2, 12));
    for (const auto &problem : problems) {
        auto *link = button(QStringLiteral("• ") + problem.text, "teamStuckProblem");
        link->setFlat(true); link->setStyleSheet(QStringLiteral("text-align:left;padding:0;color:%1;").arg(c.accent.name()));
        QObject::connect(link, &QPushButton::clicked, this, [this, task = problem.task, member = problem.member, t = team] {
            if (task) openTask(task);
            else if (const auto *m = t.member(member); m && openSession) openSession(m->host, m->session);
        });
        v->addWidget(link);
    }
    auto *blockers = button(m_showBlockers ? tr("Hide blockers") : tr("Show what blocks"), "teamShowBlockers");
    QObject::connect(blockers, &QPushButton::clicked, this, [this] { m_showBlockers = !m_showBlockers; scheduleRender(); });
    auto *r = row(); r->addWidget(blockers); r->addStretch(); v->addLayout(r);
    return box;
}

QWidget *TeamPanel::answers(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamAnswers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    int n = 0;
    auto *items = new QVBoxLayout; items->setSpacing(6);
    for (const auto &member : team.members) {
        const auto state = m_live ? m_live(member) : Live{};
        if (member.session.isEmpty() || !state.attention) continue;
        ++n;
        auto *line = card(c);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 5, 6, 5); h->setSpacing(6);
        h->addWidget(new StatusMark(QString(), c.warn, "answer", 16));
        if (width() >= 400) h->addWidget(new Pill(team.roleOf(member.id), roleColor(team.roleOf(member.id), m_dark)));
        h->addWidget(elided(state.attentionText.isEmpty() ? tr("Question in %1").arg(member.session) : state.attentionText, c.fg, 12), 1);
        auto *go = button(tr("Go to question"), "teamGoToQuestion");
        QObject::connect(go, &QPushButton::clicked, this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
        h->addWidget(go);
        items->addWidget(line);
    }
    auto *title = row(6);
    title->addWidget(label(tr("Needs your answer"), c.fg, 12, true));
    title->addWidget(new Pill(QString::number(n), c.warn));
    title->addStretch();
    v->addLayout(title); v->addLayout(items);
    return box;
}

QWidget *TeamPanel::members(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.members.size()), c.muted, 12));
    title->addStretch();
    v->addLayout(title);
    for (const auto &member : team.members) v->addWidget(memberRow(team, member, narrow));
    return box;
}

QWidget *TeamPanel::memberRow(const Team &team, const Member &member, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto state = m_live ? m_live(member) : Live{};
    const auto status = memberStatus(team, member, state);
    const QColor tone = toneColor(c, status.tone);
    auto *box = card(c, status.problem ? alpha(tone, .8) : QColor());
    box->setObjectName("teamCard");
    box->setProperty("teamMember", member.id);
    box->setAccessibleName(tr("%1, %2").arg(team.roleOf(member.id), status.text));
    if (member.session.isEmpty() && team.status == "planning")
        box->setStyleSheet(QStringLiteral("QFrame#teamCard{background:transparent;border:1px dashed %1;border-radius:9px;}").arg(c.border.name()));
    auto *outer = new QVBoxLayout(box); outer->setContentsMargins(8, 7, 6, 7); outer->setSpacing(4);
    auto *h = row(8);
    auto *tile = new QLabel; tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("background:%1;border-radius:7px;").arg(c.tile.name()));
    tile->setPixmap(workspaceIcon(agentIcon(member.agent), c.fg).pixmap(16, 16));
    tile->setToolTip(member.session.isEmpty() ? member.agent : member.agent + QStringLiteral(" · ") + member.session);
    h->addWidget(tile, 0, Qt::AlignTop);
    auto *body = new QVBoxLayout; body->setSpacing(3);
    auto *line1 = row(6);
    const QString role = team.roleOf(member.id);
    line1->addWidget(new Pill(member.lead && member.role != role ? role + QStringLiteral(" · ") + member.role : role, roleColor(role, m_dark), member.lead ? "lead" : QString()));
    if (state.attention) line1->addWidget(new Pill(QStringLiteral("?1"), c.warn));
    line1->addWidget(elided(member.model.isEmpty() ? member.agent : member.model, c.text3, 12), 1);
    if (!narrow) line1->addWidget(new StatusMark(status.text, tone, status.kind, 112, Qt::AlignRight));
    body->addLayout(line1);
    if (narrow) body->addWidget(new StatusMark(status.text, tone, status.kind));
    auto *line2 = row(8);
    const Task *current = team.task(status.task);
    QString doing = current ? QStringLiteral("#%1 %2").arg(current->id).arg(current->title) : tr("No task");
    if (current && current->last() && current->last()->n > 1) doing += tr(" · att %1").arg(current->last()->n);
    auto *doingLabel = elided(doing, current ? c.text2 : c.muted, 11);
    line2->addWidget(doingLabel, 1);
    line2->addWidget(label(member.computer.isEmpty() ? tr("this computer") : member.computer, c.muted, 11));
    if (!narrow) {
        // Task chips: this member's tasks, colored by status.
        int shown = 0;
        for (const auto &task : team.tasks) {
            if (task.assignee != member.id || shown >= 4) continue;
            ++shown;
            auto *chip = new QPushButton(QStringLiteral("#%1").arg(task.id));
            chip->setObjectName("teamTaskChip"); chip->setFlat(true); chip->setCursor(Qt::PointingHandCursor);
            const QColor chipColor = toneColor(c, taskStatus(task).tone);
            chip->setStyleSheet(QStringLiteral("QPushButton{font:10px;color:%1;border:1px solid %2;border-radius:4px;padding:0 4px;min-height:16px;}")
                                    .arg(chipColor.name(), alpha(chipColor, task.id == status.task ? 1 : .4).name(QColor::HexArgb)));
            chip->setToolTip(QStringLiteral("#%1 %2 · %3").arg(task.id).arg(task.title, taskStatus(task).text));
            QObject::connect(chip, &QPushButton::clicked, this, [this, id = task.id] { openTask(id); });
            line2->addWidget(chip);
        }
    }
    body->addLayout(line2);
    h->addLayout(body, 1);
    auto *more = new QToolButton; more->setObjectName("teamMemberMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setAccessibleName(tr("Member actions"));
    more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    auto *open = menu->addAction(tr("Open session"), this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
    open->setEnabled(!member.session.isEmpty());
    if (!member.lead) {
        if (member.state == "paused")
            menu->addAction(tr("Resume member"), this, [this, id = member.id] { act("resume-member", {{"member", id}}); });
        else if (team.status != "planning")
            menu->addAction(tr("Pause member"), this, [this, id = member.id] { act("pause-member", {{"member", id}}); });
        auto *replace = menu->addAction(tr("Replace agent / model…"));
        replace->setEnabled(false); replace->setToolTip(tr("Comes with the plan editor"));
        menu->addAction(tr("Remove from team…"), this, [this, id = member.id, role] {
            if (confirm("teamRemoveMemberConfirm", tr("Remove member?"), tr("Remove %1 from the team? Its session keeps running. "
                                                                            "Reassign or cancel its open tasks first.").arg(role), tr("Remove")))
                act("remove-member", {{"member", id}});
        });
    }
    more->setMenu(menu);
    h->addWidget(more, 0, Qt::AlignTop);
    outer->addLayout(h);
    if (status.problem) outer->addWidget(problemActions(team, member, status));
    return box;
}

QWidget *TeamPanel::problemActions(const Team &team, const Member &member, const Status &status)
{
    auto *box = new QWidget; box->setObjectName("teamProblemActions");
    auto *h = new QHBoxLayout(box); h->setContentsMargins(36, 0, 0, 0); h->setSpacing(6);
    const Task *task = team.task(status.task);
    const auto *attempt = task ? task->last() : nullptr;
    const auto add = [&](const QString &text, const char *name, bool primary, const std::function<void()> &run) {
        auto *b = button(text, name, primary); QObject::connect(b, &QPushButton::clicked, this, run); h->addWidget(b); return b;
    };
    const QString host = member.host, session = member.session;
    if (status.kind == "unconfirmed" && task && attempt) {
        add(tr("Check session"), "teamCheckSession", true, [this, host, session] { if (openSession) openSession(host, session); });
        auto *arrived = add(tr("It arrived"), "teamCheckDelivered", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "delivered"}}); });
        arrived->setToolTip(tr("The session shows the task: mark it delivered"));
        add(tr("Send again"), "teamSendAgain", false, [this, id = task->id, n = attempt->n, role = team.roleOf(member.id)] {
            if (confirm("teamSendAgainConfirm", tr("Send the task again?"),
                        tr("Attempt %1 may have reached %2. Send the task again as a new attempt? The old attempt is revoked; "
                           "a late report for it is refused.").arg(n).arg(role), tr("Send again")))
                act("retry", {{"task", id}});
        });
        auto *cancel = add(tr("Cancel attempt"), "teamCancelAttempt", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "not_delivered"}}); });
        cancel->setToolTip(tr("Nothing arrived: the task returns to Pending"));
    } else if (status.kind == "no_report" && task && attempt) {
        add(tr("Remind"), "teamRemind", true, [this, t = team, member, id = task->id] { remind(t, member, id); });
        add(tr("Use last reply as report"), "teamUseLastReply", false, [this, id = task->id, attemptId = attempt->id] {
            act("use-last-reply", {{"task", id}, {"attempt", attemptId}});
        });
        add(tr("Reassign…"), "teamReassign", false, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
    } else if (status.kind == "offline") {
        if (task) add(tr("Reassign…"), "teamReassign", true, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
        auto *wait = add(tr("Wait"), "teamWait", false, [] {});
        wait->setToolTip(tr("Open attempts are not revoked; the member continues when its computer answers"));
    } else if (status.kind == "answer") {
        add(tr("Go to question"), "teamGoToQuestion", true, [this, host, session] { if (openSession) openSession(host, session); });
    } else if (status.kind == "failed") {
        auto *again = add(tr("Start again"), "teamRelaunch", true, [this, id = member.id] { act("relaunch", {{"member", id}}); });
        again->setToolTip(member.error);
        for (const auto &t : team.tasks)
            if (t.assignee == member.id && !t.finished()) {
                add(tr("Reassign…"), "teamReassign", false, [this, tm = team, id = t.id] { reassignMenu(tm, id, QCursor::pos()); });
                break;
            }
    }
    h->addStretch();
    return box;
}

void TeamPanel::reassignMenu(const Team &team, int taskId, const QPoint &global)
{
    const auto *task = team.task(taskId);
    if (!task) return;
    QMenu menu(this);
    menu.setObjectName("teamReassignMenu");
    for (const auto &member : team.members) {
        if (member.id == task->assignee) continue;
        const auto status = memberStatus(team, member, m_live ? m_live(member) : Live{});
        auto *action = menu.addAction(QStringLiteral("%1 · %2 — %3").arg(team.roleOf(member.id), member.model.isEmpty() ? member.agent : member.model, status.text));
        QObject::connect(action, &QAction::triggered, this, [this, taskId, id = member.id] { act("reassign", {{"task", taskId}, {"member", id}}); });
    }
    if (menu.isEmpty()) menu.addAction(tr("No other member"))->setEnabled(false);
    menu.exec(global);
}

void TeamPanel::taskMenu(const Team &team, int taskId, const QPoint &global)
{
    const auto *task = team.task(taskId);
    if (!task) return;
    QMenu menu(this);
    menu.setObjectName("teamTaskMenu");
    auto *title = menu.addAction(QStringLiteral("#%1 %2 · %3").arg(task->id).arg(task->title, taskStatus(*task).text));
    title->setEnabled(false);
    menu.addSeparator();
    const auto *assignee = team.member(task->assignee);
    if (assignee && !assignee->session.isEmpty())
        menu.addAction(tr("Open %1's session").arg(team.roleOf(assignee->id)), this, [this, host = assignee->host, session = assignee->session] {
            if (openSession) openSession(host, session);
        });
    const bool open = !task->finished() && task->status != "draft";
    if (task->status == "submitted") {
        menu.addAction(tr("Accept"), this, [this, taskId] { act("accept", {{"task", taskId}}); });
        menu.addAction(tr("Reject with comment…"), this, [this, taskId] {
            bool ok = false;
            const auto reason = QInputDialog::getMultiLineText(this, tr("Reject with comment"), tr("Why is the report not accepted? The task goes back for another attempt."), {}, &ok);
            if (ok && !reason.trimmed().isEmpty()) act("reject", {{"task", taskId}, {"reason", reason.trimmed()}});
        });
    }
    if (open) {
        menu.addAction(tr("Reassign…"), this, [this, t = team, taskId] { reassignMenu(t, taskId, QCursor::pos()); });
        menu.addAction(tr("Retry"), this, [this, taskId, n = task->last() ? task->last()->n : 0] {
            if (n == 0 || confirm("teamRetryConfirm", tr("Retry task?"), tr("Revoke attempt %1 and hand the task out again?").arg(n), tr("Retry")))
                act("retry", {{"task", taskId}});
        });
        menu.addAction(tr("Take over"), this, [this, taskId] {
            if (confirm("teamTakeOverConfirm", tr("Take over task?"), tr("You do this task yourself; its current attempt is revoked and no member gets it."), tr("Take over")))
                act("take-over", {{"task", taskId}});
        });
        menu.addAction(tr("Cancel task…"), this, [this, taskId] {
            if (confirm("teamCancelTaskConfirm", tr("Cancel task?"), tr("Cancel this task? Tasks that depend on it hard never become ready."), tr("Cancel task")))
                act("cancel", {{"task", taskId}});
        });
    } else if (task->finished()) {
        auto *none = menu.addAction(tr("Decided — no actions"));
        none->setEnabled(false);
    }
    menu.exec(global);
}

QWidget *TeamPanel::tasks(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamTasks");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Tasks"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.tasks.size()), c.muted, 12));
    title->addStretch();
    const bool planningNow = team.status == "planning";
    if (planningNow) {
        auto *add = button(tr("+ Task"), "teamAddTask");
        QObject::connect(add, &QPushButton::clicked, this, [this, team] { editTask(team, 0); });
        title->addWidget(add);
    }
    for (const auto &[key, text] : QList<std::pair<QString, QString>>{{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}) {
        if (planningNow) break;
        auto *b = button(text, "teamTaskFilter");
        b->setCheckable(true); b->setChecked(m_filter == key); b->setProperty("filter", key);
        QObject::connect(b, &QPushButton::clicked, this, [this, key = key] { m_filter = key; scheduleRender(); });
        title->addWidget(b);
    }
    v->addLayout(title);
    // Problem tasks: their own state, or a problem of the member working on them.
    QSet<int> problemTasks;
    QMap<int, Tone> problemTone;
    for (const auto &p : problems(team, m_live ? m_live : LiveLookup([](const Member &) { return Live{}; })))
        if (p.task) { problemTasks.insert(p.task); if (problemTone.value(p.task, Tone::Warn) != Tone::Danger) problemTone[p.task] = p.tone; }
    for (const auto &task : team.tasks) if (const auto status = taskStatus(task); status.problem) { problemTasks.insert(task.id); problemTone[task.id] = status.tone; }
    auto *view = new TaskView(c, !narrow);
    view->setObjectName(narrow ? "teamTaskList" : "teamTaskGraph");
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    QList<int> order;
    const auto byLayer = layered(team);
    for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    for (int id : order) {
        const auto *task = team.task(id);
        const auto status = taskStatus(*task);
        const QString role = task->assignee.isEmpty() ? (planningNow ? tr("No assignee") : tr("You")) : team.roleOf(task->assignee);
        TaskView::Node node;
        node.id = id; node.layer = team.layers.value(id); node.title = task->title; node.role = role;
        node.problem = problemTasks.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status);
        node.statusText = status.text; node.roleTone = roleColor(role, m_dark);
        node.draft = task->status == "draft";
        node.error = team.cycle.contains(id) || (task->assignee.isEmpty() ? planningNow : !team.member(task->assignee));
        node.attempt = task->last() ? task->last()->n : 0;
        QStringList deps;
        for (const auto &dep : task->deps) {
            deps << QStringLiteral("#%1").arg(dep.task); view->edges[id].append(dep.task);
            if (dep.soft && !team.allHard) view->soft.insert({dep.task, id});
        }
        node.deps = deps.isEmpty() ? QString() : QStringLiteral("← ") + deps.join(' ');
        const bool active = QStringList{"assigned", "in_progress", "submitted"}.contains(task->status);
        node.match = m_filter == "all" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem);
        view->nodes.append(node);
    }
    if (m_showBlockers) for (int id : problemTasks) view->emphasis += Teams::chain(team, id);
    if (planningNow) {
        view->editable = !narrow;
        view->clicked = [this, team](int id, const QPoint &) { editTask(team, id); };
        view->linked = [this, team](int dependency, int task) { link(team, dependency, task, true); };
        view->unlinked = [this, team](int dependency, int task) { link(team, dependency, task, false); };
    } else view->clicked = [this](int id, const QPoint &) { openTask(id); };
    view->layout();
    if (narrow) v->addWidget(view);
    else {
        auto *tools = row(4);
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); scheduleRender(); };
        auto *out = button(QStringLiteral("−"), "teamZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        auto *level = label(QStringLiteral("%1%").arg(int(std::round(m_zoom * 100))), c.text3, 11, false, "teamZoomLevel");
        auto *in = button(QStringLiteral("+"), "teamZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        auto *fit = button(tr("Fit"), "teamZoomFit");
        out->setFixedWidth(28); in->setFixedWidth(28); fit->setFixedWidth(44);
        tools->addWidget(out); tools->addWidget(level); tools->addWidget(in); tools->addWidget(fit);
        // The hint gives way first: nothing may be wider than the panel.
        auto *hint = elided(tr("Hover = chain · click = actions"), c.muted, 11);
        hint->setAlignment(Qt::AlignRight | Qt::AlignVCenter); hint->setMinimumWidth(0); hint->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        tools->addWidget(hint, 1);
        v->addLayout(tools);
        auto *area = new QScrollArea; area->setObjectName("teamGraphArea");
        area->setWidget(view); area->setWidgetResizable(false); area->setFrameShape(QFrame::NoFrame);
        area->setMaximumHeight(430);
        area->setMinimumHeight(qMin(430, view->height() + 4));
        area->setStyleSheet(QStringLiteral("QScrollArea#teamGraphArea{background:%1;border:1px solid %2;border-radius:9px;}")
                                .arg(c.surface.name(), c.border.name()));
        area->viewport()->setAutoFillBackground(true);
        QPalette surface = area->viewport()->palette(); surface.setColor(QPalette::Window, c.surface); area->viewport()->setPalette(surface);
        QObject::connect(fit, &QPushButton::clicked, this, [=] {
            const double natural = view->width() / m_zoom;
            zoomTo(natural > 0 ? qMin(1.5, (area->viewport()->width() - 4) / natural) : 1.0);
        });
        v->addWidget(area);
        auto *legend = label(tr("L = layer: tasks that can run at the same time"), c.muted, 10);
        v->addWidget(legend);
    }
    return box;
}

QWidget *TeamPanel::feed(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamFeed");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(2);
    auto *title = row(6);
    title->addWidget(label(tr("Feed"), c.fg, 12, true));
    title->addStretch();
    auto *filter = new QComboBox; filter->setObjectName("teamFeedFilter");
    filter->addItem(tr("All members"), QString());
    for (const auto &member : team.members) filter->addItem(team.roleOf(member.id), team.roleOf(member.id));
    filter->setCurrentIndex(qMax(0, filter->findData(m_feedMember)));
    QObject::connect(filter, &QComboBox::currentIndexChanged, this, [this, filter] { m_feedMember = filter->currentData().toString(); scheduleRender(); });
    title->addWidget(filter);
    v->addLayout(title);
    int shown = 0;
    for (const auto &event : m_controller->feed()) {
        const auto line = feedLine(team, event);
        if (!m_feedMember.isEmpty() && line.by != m_feedMember) continue;
        ++shown;
        auto *r = new QWidget; r->setObjectName("teamFeedRow");
        auto *h = new QHBoxLayout(r); h->setContentsMargins(0, 3, 0, 3); h->setSpacing(6);
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(TeamUi::mono(11)); time->setFixedWidth(38);
        time->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(line.at)).toString(Qt::ISODate));
        h->addWidget(time, 0, Qt::AlignTop);
        if (!line.by.isEmpty()) h->addWidget(new Pill(line.by, line.by == tr("You") ? c.text3 : roleColor(line.by, m_dark)), 0, Qt::AlignTop);
        auto *text = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 12);
        text->setWordWrap(true);
        h->addWidget(text, 1);
        v->addWidget(r);
    }
    if (!shown) v->addWidget(label(tr("Nothing yet"), c.muted, 12));
    if (m_controller->feedHasOlder()) {
        auto *older = button(tr("Load older"), "teamFeedOlder");
        QObject::connect(older, &QPushButton::clicked, this, [this] { m_controller->loadOlder(); });
        auto *r = row(); r->addWidget(older); r->addStretch(); v->addLayout(r);
    } else if (shown) v->addWidget(label(tr("Beginning of team history"), c.muted, 11));
    return box;
}
