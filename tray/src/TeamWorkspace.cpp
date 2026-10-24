#include "TeamWorkspace.h"
#include "TeamController.h"
#include "TeamPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QLineEdit>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include <cmath>

using namespace TeamUi;

namespace {
// The board's columns, by what a task needs.
QString column(const Task &task, bool problem)
{
    if (problem) return QStringLiteral("problems");
    if (task.status == "accepted" || task.status == "rejected" || task.status == "cancelled") return QStringLiteral("accepted");
    if (task.status == "submitted") return QStringLiteral("submitted");
    if (task.status == "assigned" || task.status == "in_progress") return QStringLiteral("active");
    return QStringLiteral("pending");
}
}

TeamWorkspace::TeamWorkspace(TeamController *teams, TeamPanel *panel, QWidget *parent) : QWidget(parent), m_teams(teams), m_panel(panel)
{
    setObjectName("teamWorkspace");
    m_layout = new QVBoxLayout(this);
    m_layout->setContentsMargins(0, 0, 0, 0);
    m_layout->setSpacing(0);
    m_drawer = new TeamTaskCard(this);
    m_drawer->setObjectName("teamWorkspaceDrawer");
    m_drawer->hide();
    m_drawer->back = [this] { m_drawerTask = 0; m_drawer->hide(); };
    m_drawer->act = [this](const QString &operation, const QJsonObject &input) { m_panel->act(operation, input); };
    m_drawer->openTask = [this](int task) { m_drawerTask = task; scheduleRender(); };
    m_drawer->openSession = [this](const QString &host, const QString &session) { if (openSession) openSession(host, session); };
    m_drawer->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return m_panel->confirm(name, title, text, action); };
    m_drawer->reassign = [this](int task, const QPoint &global) { if (const auto *team = m_teams->watched()) m_panel->reassignMenu(*team, task, global); };
    m_drawer->problemActions = [this](const Member &member, const Status &status) -> QWidget * {
        const auto *team = m_teams->watched();
        return team ? m_panel->problemActions(*team, member, status) : nullptr;
    };
    QObject::connect(m_teams, &TeamController::teamsChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::watchedChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::feedChanged, this, [this] { scheduleRender(); });
}

void TeamWorkspace::scheduleRender()
{
    if (m_pending) return;
    m_pending = true;
    QTimer::singleShot(0, this, [this] { m_pending = false; render(); });
}

void TeamWorkspace::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    render();
}

void TeamWorkspace::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    placeDrawer();
}

void TeamWorkspace::placeDrawer()
{
    const int w = qMin(400, width());
    m_drawer->setGeometry(width() - w, 0, w, height());
    m_drawer->raise();
}

void TeamWorkspace::render()
{
    if (!isVisible()) return;
    const Colors c = colorsFor(m_dark);
    const Team *team = m_teams->watched();
    if (!team || team->id != m_team) team = m_teams->team(m_team);
    auto *content = new QWidget;
    content->setObjectName("teamWorkspaceContent");
    content->setStyleSheet(QStringLiteral("QWidget#teamWorkspaceContent{background:%1;}").arg(c.surface.name()));
    auto *v = new QVBoxLayout(content); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(0);
    if (!team) {
        auto *empty = label(m_team.isEmpty() ? tr("No team selected") : tr("Loading team…"), c.muted, 13);
        empty->setAlignment(Qt::AlignCenter);
        v->addWidget(empty, 1);
    } else {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        v->addWidget(teamBar(*team, status));
        // Strips: Stuck, plan errors.
        if (status == "stuck" || !team->errors.isEmpty()) {
            auto *strip = new QFrame; strip->setObjectName("teamWorkspaceStrip");
            const QColor tone = status == "stuck" ? c.warn : c.danger;
            strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceStrip{background:%1;border-bottom:1px solid %2;}").arg(alpha(tone, .1).name(QColor::HexArgb), alpha(tone, .5).name(QColor::HexArgb)));
            auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 6, 12, 6);
            QStringList items;
            if (status == "stuck") for (const auto &p : problems(*team, live)) items << p.text;
            items += team->errors;
            h->addWidget(new StatusMark((status == "stuck" ? tr("Team stuck — nothing can start: ") : tr("Plan errors: ")) + items.join(QStringLiteral(" · ")), tone, "stuck"), 1);
            v->addWidget(strip);
        }
        auto *middle = new QHBoxLayout; middle->setContentsMargins(0, 0, 0, 0); middle->setSpacing(0);
        auto *left = new QVBoxLayout; left->setSpacing(0);
        left->addWidget(toolbar(*team));
        left->addWidget(canvas(*team), 1);
        left->addWidget(feed(*team));
        middle->addLayout(left, 1);
        middle->addWidget(side(*team));
        v->addLayout(middle, 1);
    }
    if (m_content) { m_layout->removeWidget(m_content); retire(m_content); }
    m_layout->addWidget(content);
    m_content = content;
    if (team && m_drawerTask) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        m_drawer->setTask(*team, m_drawerTask, live, m_dark);
        m_drawer->setStyleSheet(QStringLiteral("QWidget#teamWorkspaceDrawer{background:%1;border-left:1px solid %2;}").arg(c.surface.name(), c.border.name()));
        m_drawer->setAttribute(Qt::WA_StyledBackground, true);
        placeDrawer();
        m_drawer->show();
    } else m_drawer->hide();
}

QWidget *TeamWorkspace::teamBar(const Team &team, const QString &status)
{
    const Colors c = colorsFor(m_dark);
    auto *bar = new QFrame; bar->setObjectName("teamWorkspaceBar"); bar->setFixedHeight(44);
    bar->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceBar{border-bottom:1px solid %1;}").arg(c.border.name()));
    auto *h = new QHBoxLayout(bar); h->setContentsMargins(12, 0, 12, 0); h->setSpacing(10);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); h->addWidget(icon);
    auto *name = label(team.name, c.fg, 14, true, "teamWorkspaceName"); h->addWidget(name);
    h->addWidget(new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false));
    QList<QColor> segments; for (auto tone : progress(team)) segments.append(toneColor(c, tone));
    auto *bar2 = new Progress(segments, 6, c.border); bar2->setFixedWidth(200); h->addWidget(bar2);
    auto *counts = label(QStringLiteral("%1/%2").arg(accepted(team)).arg(team.tasks.size()), c.muted, 11); counts->setFont(NewSessionDense::mono(11));
    h->addWidget(counts);
    if (team.created > 0) h->addWidget(label(duration(QDateTime::currentSecsSinceEpoch() - team.created), c.muted, 11));
    h->addStretch();
    const auto add = [&](const QString &text, const char *objectName, bool primary, const std::function<void()> &run) {
        auto *b = button(text, objectName, primary); QObject::connect(b, &QPushButton::clicked, this, run); h->addWidget(b); return b;
    };
    if (team.status == "planning") {
        auto *discard = add(tr("Discard plan…"), "teamWorkspaceDiscard", false, [this, name = team.name] {
            if (m_panel->confirm("teamDiscardConfirm", tr("Discard plan?"), tr("Discard the plan of %1? The team stops before any member starts.").arg(name), tr("Discard plan")))
                m_panel->act("discard", {});
        });
        discard->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name()));
        add(tr("Edit plan"), "teamWorkspaceEdit", false, [this] { if (openTeamTab) openTeamTab(); });
        auto *approve = add(tr("Approve && Run"), "teamWorkspaceApprove", true, [this] { m_panel->act("approve", {}); });
        approve->setEnabled(team.errors.isEmpty() && !team.tasks.isEmpty());
    } else if (team.status == "running") {
        add(tr("Pause"), "teamWorkspacePause", false, [this] { m_panel->act("pause", {}); });
    } else if (team.status == "paused") {
        add(tr("Resume"), "teamWorkspaceResume", true, [this] { m_panel->act("resume", {}); });
    }
    if (team.status == "running" || team.status == "paused")
        add(tr("Stop…"), "teamWorkspaceStop", false, [this, name = team.name] {
            if (m_panel->confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                          "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                m_panel->act("stop", {});
        });
    return bar;
}

QWidget *TeamWorkspace::toolbar(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *bar = new QWidget; bar->setObjectName("teamWorkspaceToolbar");
    auto *h = new QHBoxLayout(bar); h->setContentsMargins(12, 8, 12, 6); h->setSpacing(4);
    for (const auto &[key, text] : QList<std::pair<QString, QString>>{{"graph", tr("Graph")}, {"board", tr("Board")}, {"layers", tr("Layers")}}) {
        auto *b = button(text, "teamWorkspaceView"); b->setCheckable(true); b->setChecked(m_view == key); b->setProperty("view", key);
        QObject::connect(b, &QPushButton::clicked, this, [this, key = key] { m_view = key; scheduleRender(); });
        h->addWidget(b);
    }
    h->addSpacing(10);
    for (const auto &[key, text] : QList<std::pair<QString, QString>>{{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}) {
        auto *b = button(text, "teamWorkspaceFilter"); b->setCheckable(true); b->setChecked(m_filter == key); b->setProperty("filter", key);
        QObject::connect(b, &QPushButton::clicked, this, [this, key = key] { m_filter = key; scheduleRender(); });
        h->addWidget(b);
    }
    if (!m_focus.isEmpty()) {
        auto *chip = button(tr("Member: %1  ×").arg(team.roleOf(m_focus)), "teamWorkspaceFocus");
        chip->setToolTip(tr("Show every member again"));
        QObject::connect(chip, &QPushButton::clicked, this, [this] { m_focus.clear(); scheduleRender(); });
        h->addWidget(chip);
    }
    h->addStretch();
    if (m_view == "graph") {
        h->addWidget(label(tr("Hover = chain · click = details"), c.muted, 11));
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); scheduleRender(); };
        auto *out = button(QStringLiteral("−"), "teamWorkspaceZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        auto *level = label(QStringLiteral("%1%").arg(int(std::round(m_zoom * 100))), c.text3, 11);
        auto *in = button(QStringLiteral("+"), "teamWorkspaceZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        auto *fit = button(tr("Fit"), "teamWorkspaceFit");
        QObject::connect(fit, &QPushButton::clicked, this, [this, zoomTo] {
            int layers = 1; if (const auto *t = m_teams->watched()) for (int l : t->layers) layers = qMax(layers, l + 1);
            const double natural = 12 + layers * 170;
            const int available = width() - 330;
            zoomTo(qMin(1.5, available / natural));
        });
        h->addWidget(out); h->addWidget(level); h->addWidget(in); h->addWidget(fit);
    }
    return bar;
}

QWidget *TeamWorkspace::canvas(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    if (m_view == "board") return board(team);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QSet<int> problemTasks; QMap<int, Tone> problemTone;
    for (const auto &p : problems(team, live)) if (p.task) { problemTasks.insert(p.task); problemTone[p.task] = p.tone; }
    for (const auto &task : team.tasks) if (const auto s = taskStatus(task); s.problem) { problemTasks.insert(task.id); problemTone[task.id] = s.tone; }
    auto *view = new TaskView(c, m_view == "graph" ? TaskView::Wide : TaskView::List);
    view->setObjectName(m_view == "graph" ? "teamWorkspaceGraph" : "teamWorkspaceLayers");
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    const auto byLayer = layered(team);
    for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) for (int id : it.value()) {
        const auto *task = team.task(id);
        const auto status = taskStatus(*task);
        TaskView::Node node;
        node.id = id; node.layer = team.layers.value(id); node.title = task->title;
        node.role = task->assignee.isEmpty() ? tr("You") : team.roleOf(task->assignee);
        node.problem = problemTasks.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status); node.statusText = status.text; node.roleTone = roleColor(node.role, m_dark);
        node.draft = task->status == "draft"; node.error = team.cycle.contains(id);
        node.attempt = task->last() ? task->last()->n : 0;
        if (const auto *m = team.member(task->assignee)) node.agent = agentIcon(m->agent);
        QStringList deps;
        for (const auto &d : task->deps) { deps << QStringLiteral("#%1").arg(d.task); view->edges[id].append(d.task); if (d.soft && !team.allHard) view->soft.insert({d.task, id}); }
        node.deps = deps.isEmpty() ? QString() : QStringLiteral("← ") + deps.join(' ');
        const bool active = QStringList{"assigned", "in_progress", "submitted"}.contains(task->status);
        node.match = (m_filter == "all" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem))
                     && (m_focus.isEmpty() || task->assignee == m_focus);
        view->nodes.append(node);
    }
    view->clicked = [this](int id, const QPoint &) { m_drawerTask = id; scheduleRender(); };
    view->layout();
    auto *area = new QScrollArea; area->setObjectName("teamWorkspaceCanvas");
    area->setFrameShape(QFrame::NoFrame);
    area->setWidget(view);
    area->setWidgetResizable(m_view != "graph");
    area->viewport()->setAutoFillBackground(true);
    QPalette p = area->viewport()->palette(); p.setColor(QPalette::Window, c.surface); area->viewport()->setPalette(p);
    return area;
}

QWidget *TeamWorkspace::board(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QSet<int> problemTasks;
    for (const auto &p : problems(team, live)) if (p.task) problemTasks.insert(p.task);
    for (const auto &task : team.tasks) if (taskStatus(task).problem) problemTasks.insert(task.id);
    auto *w = new QWidget; w->setObjectName("teamWorkspaceBoard");
    auto *h = new QHBoxLayout(w); h->setContentsMargins(12, 4, 12, 8); h->setSpacing(8);
    for (const auto &[key, name] : QList<std::pair<QString, QString>>{{"pending", tr("Pending")}, {"active", tr("Active")}, {"problems", tr("Problems")},
                                                                     {"submitted", tr("Submitted")}, {"accepted", tr("Accepted")}}) {
        auto *col = new QWidget; auto *cv = new QVBoxLayout(col); cv->setContentsMargins(0, 0, 0, 0); cv->setSpacing(6);
        int n = 0;
        auto *list = new QVBoxLayout; list->setSpacing(6);
        for (const auto &task : team.tasks) {
            if (column(task, problemTasks.contains(task.id)) != key) continue;
            if (!m_focus.isEmpty() && task.assignee != m_focus) continue;
            ++n;
            const auto s = taskStatus(task);
            auto *cardBox = card(c, problemTasks.contains(task.id) ? c.danger : QColor());
            cardBox->setObjectName("teamBoardCard"); cardBox->setCursor(Qt::PointingHandCursor);
            auto *b = new QVBoxLayout(cardBox); b->setContentsMargins(8, 6, 8, 6); b->setSpacing(3);
            auto *title = label(QStringLiteral("#%1 %2").arg(task.id).arg(task.title), c.fg, 12, true); title->setWordWrap(true); b->addWidget(title);
            auto *r = row(4);
            const QString role = task.assignee.isEmpty() ? tr("You") : team.roleOf(task.assignee);
            r->addWidget(new Pill(role, roleColor(role, m_dark)));
            if (const auto *m = team.member(task.assignee)) { auto *i = new QLabel; i->setPixmap(workspaceIcon(agentIcon(m->agent), c.text3).pixmap(14, 14)); r->addWidget(i); }
            r->addStretch(); b->addLayout(r);
            b->addWidget(new StatusMark(s.text, toneColor(c, s.tone), kindOfTask(task, s)));
            QStringList deps; for (const auto &d : task.deps) deps << QStringLiteral("#%1").arg(d.task);
            if (!deps.isEmpty()) b->addWidget(label(QStringLiteral("← ") + deps.join(' '), c.dim, 10));
            auto *open = new QPushButton(cardBox); open->setFlat(true); open->setStyleSheet(QStringLiteral("background:transparent;border:none;"));
            open->setGeometry(0, 0, 4000, 4000); open->setObjectName("teamBoardOpen");
            QObject::connect(open, &QPushButton::clicked, this, [this, id = task.id] { m_drawerTask = id; scheduleRender(); });
            list->addWidget(cardBox);
        }
        list->addStretch();
        cv->addWidget(label(QStringLiteral("%1 · %2").arg(name.toUpper()).arg(n), c.dim, 10, true));
        auto *inner = new QWidget; inner->setLayout(list);
        auto *scroll = new QScrollArea; scroll->setWidget(inner); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
        cv->addWidget(scroll, 1);
        h->addWidget(col, 1);
    }
    return w;
}

QWidget *TeamWorkspace::side(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *w = new QFrame; w->setObjectName("teamWorkspaceSide"); w->setFixedWidth(320);
    w->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceSide{border-left:1px solid %1;}").arg(c.border.name()));
    auto *v = new QVBoxLayout(w); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(8);
    // Needs your answer.
    int waiting = 0;
    for (const auto &m : team.members) {
        const auto state = live(m);
        if (m.session.isEmpty() || !state.attention) continue;
        if (!waiting++) { auto *t = row(6); t->addWidget(label(tr("Needs your answer"), c.fg, 12, true)); t->addStretch(); v->addLayout(t); }
        auto *r = row(6);
        r->addWidget(new Pill(team.roleOf(m.id), roleColor(team.roleOf(m.id), m_dark)));
        r->addWidget(elided(state.attentionText.isEmpty() ? tr("Question") : state.attentionText, c.fg, 12), 1);
        auto *go = button(tr("Go"), "teamWorkspaceGoToQuestion");
        QObject::connect(go, &QPushButton::clicked, this, [this, host = m.host, session = m.session] { if (openSession) openSession(host, session); });
        r->addWidget(go);
        v->addLayout(r);
    }
    // Members: a click focuses the canvas and feed on one.
    auto *title = row(6); title->addWidget(label(tr("Members"), c.fg, 12, true)); title->addWidget(label(QString::number(team.members.size()), c.muted, 12)); title->addStretch();
    v->addLayout(title);
    auto *membersBox = new QWidget; auto *mv = new QVBoxLayout(membersBox); mv->setContentsMargins(0, 0, 0, 0); mv->setSpacing(4);
    for (const auto &m : team.members) {
        const auto status = memberStatus(team, m, live(m));
        auto *b = card(c, m.id == m_focus ? c.accent : status.problem ? toneColor(c, status.tone) : QColor());
        b->setObjectName("teamWorkspaceMember"); b->setCursor(Qt::PointingHandCursor); b->setProperty("teamMember", m.id);
        auto *bh = new QHBoxLayout(b); bh->setContentsMargins(6, 4, 6, 4); bh->setSpacing(6);
        auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(m.agent), c.fg).pixmap(16, 16)); bh->addWidget(tile);
        auto *col = new QVBoxLayout; col->setSpacing(1);
        auto *top = row(4);
        const QString role = team.roleOf(m.id);
        top->addWidget(new Pill(role, roleColor(role, m_dark)));
        top->addWidget(elided(m.model.isEmpty() ? m.agent : m.model, c.text3, 11), 1);
        col->addLayout(top);
        col->addWidget(new StatusMark(status.text, toneColor(c, status.tone), status.kind));
        bh->addLayout(col, 1);
        int done = 0, total = 0; for (const auto &t : team.tasks) if (t.assignee == m.id) { ++total; done += t.status == "accepted"; }
        auto *ratio = label(QStringLiteral("%1/%2").arg(done).arg(total), c.muted, 11); ratio->setFont(NewSessionDense::mono(11)); bh->addWidget(ratio);
        auto *focus = new QPushButton(b); focus->setFlat(true); focus->setStyleSheet(QStringLiteral("background:transparent;border:none;"));
        focus->setGeometry(0, 0, 4000, 4000); focus->setObjectName("teamWorkspaceFocusMember");
        focus->setToolTip(m.id == m_focus ? tr("Show every member") : tr("Show only %1's tasks").arg(role));
        QObject::connect(focus, &QPushButton::clicked, this, [this, id = m.id] { m_focus = m_focus == id ? QString() : id; scheduleRender(); });
        mv->addWidget(b);
    }
    mv->addStretch();
    auto *scroll = new QScrollArea; scroll->setWidget(membersBox); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    v->addWidget(scroll, 1);
    v->addWidget(leadDock(team));
    return w;
}

QWidget *TeamWorkspace::leadDock(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto *lead = team.member(team.lead);
    auto *dock = card(c);
    dock->setObjectName("teamLeadDock");
    auto *v = new QVBoxLayout(dock); v->setContentsMargins(8, 6, 8, 6); v->setSpacing(4);
    auto *head = row(6);
    if (lead) { auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(lead->agent), c.fg).pixmap(14, 14)); head->addWidget(tile); }
    head->addWidget(label(tr("Lead"), c.fg, 12, true));
    if (lead) {
        head->addWidget(label(lead->model.isEmpty() ? lead->agent : lead->model, c.muted, 11));
        const auto s = memberStatus(team, *lead, m_live ? m_live(*lead) : Live{});
        head->addWidget(new StatusMark(s.text, toneColor(c, s.tone), s.kind), 1);
    } else head->addStretch();
    auto *open = new QToolButton; open->setIcon(workspaceIcon("external", c.fg)); open->setAutoRaise(true); open->setToolTip(tr("Open the lead's conversation"));
    open->setObjectName("teamLeadDockOpen");
    open->setEnabled(lead && !lead->session.isEmpty());
    if (lead) QObject::connect(open, &QToolButton::clicked, this, [this, host = lead->host, session = lead->session] { if (openSession) openSession(host, session); });
    auto *minimize = new QToolButton; minimize->setIcon(workspaceIcon(m_leadMinimized ? "chevron-right" : "chevron-down", c.fg)); minimize->setAutoRaise(true);
    minimize->setToolTip(m_leadMinimized ? tr("Show the lead's recent team events") : tr("Minimize"));
    minimize->setObjectName("teamLeadDockMinimize");
    QObject::connect(minimize, &QToolButton::clicked, this, [this] { m_leadMinimized = !m_leadMinimized; scheduleRender(); });
    head->addWidget(open); head->addWidget(minimize);
    v->addLayout(head);
    if (m_leadMinimized) return dock;
    // The lead's last team events: reports, decisions, plans.
    static const QStringList kinds{"reported", "accepted", "rejected", "planned", "approved", "briefed", "notice_unconfirmed", "done"};
    int shown = 0;
    for (const auto &event : m_teams->feed()) {
        if (!kinds.contains(event.value("kind").toString())) continue;
        const auto line = feedLine(team, event);
        auto *l = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 11); l->setWordWrap(true);
        v->addWidget(l);
        if (++shown == 4) break;
    }
    if (team.status == "planning") v->addWidget(label(team.tasks.isEmpty() ? tr("Waiting for the plan") : tr("Plan ready for review"), c.folder, 11));
    auto *composer = row(4);
    auto *text = new QLineEdit; text->setObjectName("teamLeadDockMessage"); text->setPlaceholderText(tr("Message Lead…"));
    text->setEnabled(lead && !lead->session.isEmpty());
    auto *send = button(tr("Send"), "teamLeadDockSend", true); send->setEnabled(text->isEnabled());
    const auto deliver = [this, text, lead = lead ? *lead : Member{}] {
        const auto message = text->text().trimmed();
        if (message.isEmpty() || lead.session.isEmpty() || !sendMessage) return;
        sendMessage(lead.host, lead.session, message);
        text->clear();
    };
    QObject::connect(send, &QPushButton::clicked, this, deliver);
    QObject::connect(text, &QLineEdit::returnPressed, this, deliver);
    composer->addWidget(text, 1); composer->addWidget(send);
    v->addLayout(composer);
    return dock;
}

QWidget *TeamWorkspace::feed(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamWorkspaceFeed");
    w->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceFeed{border-top:1px solid %1;}").arg(c.border.name()));
    auto *v = new QVBoxLayout(w); v->setContentsMargins(12, 4, 12, 6); v->setSpacing(2);
    auto *head = row(6);
    auto *toggle = new QToolButton; toggle->setText(m_feedOpen ? QStringLiteral("⌄") : QStringLiteral("›")); toggle->setAutoRaise(true);
    toggle->setObjectName("teamWorkspaceFeedToggle");
    QObject::connect(toggle, &QToolButton::clicked, this, [this] { m_feedOpen = !m_feedOpen; scheduleRender(); });
    head->addWidget(toggle);
    head->addWidget(label(tr("Feed"), c.fg, 12, true));
    head->addWidget(label(tr("%n event(s)", nullptr, int(m_teams->feed().size())), c.muted, 11));
    head->addStretch();
    if (m_teams->feedHasOlder() && m_feedOpen) {
        auto *older = button(tr("Load older"), "teamWorkspaceFeedOlder");
        QObject::connect(older, &QPushButton::clicked, this, [this] { m_teams->loadOlder(); });
        head->addWidget(older);
    }
    v->addLayout(head);
    if (!m_feedOpen) return w;
    auto *rows = new QWidget; auto *rv = new QVBoxLayout(rows); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(1);
    const QString focusRole = m_focus.isEmpty() ? QString() : team.roleOf(m_focus);
    for (const auto &event : m_teams->feed()) {
        const auto line = feedLine(team, event);
        if (!focusRole.isEmpty() && line.by != focusRole && !line.text.contains(focusRole)) continue;
        auto *r = row(6);
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(NewSessionDense::mono(11)); time->setFixedWidth(38); r->addWidget(time);
        if (!line.by.isEmpty()) r->addWidget(new Pill(line.by, roleColor(line.by, m_dark)));
        auto *t = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 12); t->setWordWrap(true);
        r->addWidget(t, 1);
        rv->addLayout(r);
    }
    rv->addStretch();
    auto *scroll = new QScrollArea; scroll->setWidget(rows); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setFixedHeight(180);
    v->addWidget(scroll);
    return w;
}
