#include "TeamWorkspace.h"
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QComboBox>
#include <QInputDialog>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QScrollBar>
#include <QSettings>
#include <QSplitter>
#include <QStyle>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include <cmath>
#include <memory>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};

// The board's columns, by what a task needs.
QString column(const Task &task, bool problem)
{
    if (problem) return QStringLiteral("problems");
    if (task.status == "accepted" || task.status == "rejected" || task.status == "cancelled") return QStringLiteral("accepted");
    if (task.status == "submitted") return QStringLiteral("submitted");
    if (task.status == "assigned" || task.status == "in_progress") return QStringLiteral("active");
    return QStringLiteral("pending");
}

// The graph's scroll area tells the graph the space it has: the most it can
// show less room for both scroll bars, so a scroll bar that comes or goes
// never changes it (a graph that filled the viewport made bars come and go
// on every layout, and the canvas shook).
class GraphArea : public QScrollArea {
public:
    std::function<void(const QSize &)> resized;
protected:
    void resizeEvent(QResizeEvent *event) override
    {
        QScrollArea::resizeEvent(event);
        const int bar = style()->pixelMetric(QStyle::PM_ScrollBarExtent, nullptr, this);
        if (resized) resized(maximumViewportSize() - QSize(bar, bar));
    }
};

// A member card that focuses the canvas on its member when clicked.
class MemberCard : public QFrame {
public:
    std::function<void()> clicked;
protected:
    void mouseReleaseEvent(QMouseEvent *event) override { if (event->button() == Qt::LeftButton && clicked) clicked(); QFrame::mouseReleaseEvent(event); }
};
}

TeamWorkspace::TeamWorkspace(TeamController *teams, TeamPanel *panel, QWidget *parent) : QWidget(parent), m_teams(teams), m_panel(panel)
{
    setObjectName("teamWorkspace");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    auto *topBox = new QWidget; m_topHost = new QVBoxLayout(topBox); m_topHost->setContentsMargins(0, 0, 0, 0); m_topHost->setSpacing(0);
    outer->addWidget(topBox);
    m_empty = new QLabel(tr("No team selected")); m_empty->setAlignment(Qt::AlignCenter); m_empty->setObjectName("teamWorkspaceEmpty");
    outer->addWidget(m_empty, 1);
    m_body = new QWidget; auto *bh = new QHBoxLayout(m_body); bh->setContentsMargins(0, 0, 0, 0);
    m_split = new QSplitter(Qt::Horizontal); m_split->setObjectName("teamWorkspaceSplit"); m_split->setChildrenCollapsible(false);
    m_split->setHandleWidth(7);
    auto *leftBox = new QWidget; m_leftHost = new QVBoxLayout(leftBox); m_leftHost->setContentsMargins(0, 0, 0, 0); m_leftHost->setSpacing(0);
    // The side column: members above, the coordinator below; the owner moves the divider.
    m_sideSplit = new QSplitter(Qt::Vertical); m_sideSplit->setObjectName("teamWorkspaceSideSplit"); m_sideSplit->setChildrenCollapsible(false);
    m_sideSplit->setMinimumWidth(340); m_sideSplit->setHandleWidth(6);
    m_members = new QScrollArea; m_members->setObjectName("teamWorkspaceMembers"); m_members->setWidgetResizable(true);
    m_members->setFrameShape(QFrame::NoFrame); m_members->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff); m_members->setMinimumHeight(90);
    m_coordinator = new QFrame; m_coordinator->setObjectName("teamWorkspaceCoordinator"); m_coordinator->setMinimumHeight(160);
    auto *cv = new QVBoxLayout(m_coordinator); cv->setContentsMargins(0, 0, 0, 0); cv->setSpacing(0);
    m_coordinatorHead = new QWidget; m_coordinatorHead->setObjectName("teamWorkspaceCoordinatorHead");
    new QHBoxLayout(m_coordinatorHead);
    m_conversation = new TeamConversation(teams->client());
    m_conversation->setPlaceholder(tr("Message the coordinator…"));
    cv->addWidget(m_coordinatorHead); cv->addWidget(m_conversation, 1);
    m_sideSplit->addWidget(m_members); m_sideSplit->addWidget(m_coordinator);
    m_sideSplit->setStretchFactor(0, 45); m_sideSplit->setStretchFactor(1, 55);
    m_split->addWidget(leftBox); m_split->addWidget(m_sideSplit);
    m_split->setStretchFactor(0, 1); m_split->setStretchFactor(1, 0);
    bh->addWidget(m_split);
    outer->addWidget(m_body, 1);
    QSettings settings;
    if (!m_split->restoreState(settings.value("teams/workspaceSplit").toByteArray())) m_split->setSizes({1000, 380});
    if (!m_sideSplit->restoreState(settings.value("teams/workspaceSideSplit").toByteArray())) m_sideSplit->setSizes({420, 520});
    QObject::connect(m_split, &QSplitter::splitterMoved, this, [this] { QSettings().setValue("teams/workspaceSplit", m_split->saveState()); });
    QObject::connect(m_sideSplit, &QSplitter::splitterMoved, this, [this] { QSettings().setValue("teams/workspaceSideSplit", m_sideSplit->saveState()); });
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
    setTheme(true);
}

void TeamWorkspace::setTheme(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamWorkspace{background:%1;}"
        "QSplitter#teamWorkspaceSplit::handle{background:%2;margin:0 3px;}"
        "QSplitter#teamWorkspaceSplit::handle:hover{background:%4;}"
        "QSplitter#teamWorkspaceSideSplit::handle{background:%2;margin:2px 40px;border-radius:1px;}"
        "QFrame#teamWorkspaceCoordinator{border-top:1px solid %2;}"
        "QWidget#teamWorkspaceCoordinatorHead{border-bottom:1px solid %2;}"
        "QLabel#teamWorkspaceEmpty{color:%3;font-size:13px;}")
        .arg(c.surface.name(), c.border.name(), c.muted.name(), c.accent.name()));
    m_conversation->setTheme(dark);
    scheduleRender();
}

void TeamWorkspace::showTeam(const QString &team)
{
    if (m_team != team) { m_team = team; m_focus.clear(); m_drawerTask = 0; m_fit = true; if (m_popover) m_popover->hide(); }
    scheduleRender();
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
    placeDrawer(); placePopover();
}

void TeamWorkspace::placeDrawer()
{
    const int w = qMin(400, width());
    m_drawer->setGeometry(width() - w, 0, w, height());
    m_drawer->raise();
}

void TeamWorkspace::placePopover()
{
    if (!m_popover) return;
    const int w = qMin(440, width() - 24);
    m_popover->setFixedWidth(w);
    m_popover->adjustSize();
    const int right = m_split->mapTo(this, QPoint(m_split->sizes().value(0), 0)).x();
    m_popover->move(qMax(12, right - w - 12), 52);
    m_popover->raise();
}

void TeamWorkspace::render()
{
    if (!isVisible()) return;
    const Colors c = colorsFor(m_dark);
    const Team *team = m_teams->watched();
    if (!team || team->id != m_team) team = m_teams->team(m_team);
    m_empty->setText(m_team.isEmpty() ? tr("No team selected") : tr("Loading team…"));
    m_empty->setVisible(!team); m_body->setVisible(team);
    auto *top = new QWidget; top->setObjectName("teamWorkspaceTop");
    auto *tv = new QVBoxLayout(top); tv->setContentsMargins(0, 0, 0, 0); tv->setSpacing(0);
    if (team) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        tv->addWidget(teamBar(*team, status));
        // Strips: Stuck, plan errors.
        if (status == "stuck" || !team->errors.isEmpty()) {
            auto *strip = new QFrame; strip->setObjectName("teamWorkspaceStrip"); strip->setMinimumHeight(34);
            const QColor tone = status == "stuck" ? c.warn : c.danger;
            strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceStrip{background:%1;border-bottom:1px solid %2;}").arg(alpha(tone, .1).name(QColor::HexArgb), alpha(tone, .5).name(QColor::HexArgb)));
            auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 6, 12, 6);
            QStringList items;
            if (status == "stuck") for (const auto &p : problems(*team, live)) items << p.text;
            items += team->errors;
            h->addWidget(new StatusMark((status == "stuck" ? tr("Team stuck — nothing can start: ") : tr("Plan errors: ")) + items.join(QStringLiteral(" · ")), tone, "stuck"), 1);
            tv->addWidget(strip);
        }
        tv->addWidget(answers(*team));
        // The canvas column.
        auto *left = new QWidget; left->setObjectName("teamWorkspaceLeft");
        auto *lv = new QVBoxLayout(left); lv->setContentsMargins(0, 0, 0, 0); lv->setSpacing(0);
        lv->addWidget(toolbar(*team));
        lv->addWidget(canvas(*team), 1);
        lv->addWidget(feed(*team));
        if (m_left) { m_leftHost->removeWidget(m_left); retire(m_left); }
        m_leftHost->addWidget(left); left->show(); m_left = left;
        // Members keep their scroll position.
        const int scroll = m_members->verticalScrollBar()->value();
        auto *list = members(*team);
        retire(m_members->takeWidget());
        m_members->setWidget(list); m_membersContent = list;
        m_members->verticalScrollBar()->setValue(scroll);
        renderCoordinator(*team, status);
    }
    if (m_top) { m_topHost->removeWidget(m_top); retire(m_top); }
    m_topHost->addWidget(top); top->show(); m_top = top;
    if (team && m_drawerTask) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        m_drawer->setTask(*team, m_drawerTask, live, m_dark);
        m_drawer->setStyleSheet(QStringLiteral("QWidget#teamWorkspaceDrawer{background:%1;border-left:1px solid %2;}").arg(c.surface.name(), c.border.name()));
        m_drawer->setAttribute(Qt::WA_StyledBackground, true);
        placeDrawer();
        m_drawer->show();
    } else m_drawer->hide();
    placePopover();
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
    auto *counts = label(QStringLiteral("%1/%2").arg(accepted(team)).arg(team.tasks.size()), c.muted, 11); counts->setFont(TeamUi::mono(11));
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
        discard->setProperty("danger", true);
        add(tr("Send back"), "teamWorkspaceSendBack", false, [this] {
            bool ok = false;
            const auto comment = QInputDialog::getMultiLineText(this, tr("Send back to the coordinator"), tr("What should change in the plan?"), {}, &ok);
            if (ok && !comment.trimmed().isEmpty()) m_conversation->send(tr("Your plan needs changes before I approve it: %1").arg(comment.trimmed()));
        });
        auto *approve = add(tr("Approve && Run"), "teamWorkspaceApprove", true, [this] { m_panel->act("approve", {}); });
        approve->setEnabled(team.errors.isEmpty() && !team.tasks.isEmpty());
        if (!approve->isEnabled()) approve->setToolTip(team.tasks.isEmpty() ? tr("The plan has no tasks yet") : tr("Fix the plan errors first"));
    } else if (team.status == "running") {
        add(tr("Pause"), "teamWorkspacePause", false, [this] { m_panel->act("pause", {}); });
    } else if (team.status == "paused") {
        add(tr("Resume"), "teamWorkspaceResume", true, [this] { m_panel->act("resume", {}); });
    }
    if (team.status == "running" || team.status == "paused") {
        auto *stop = add(tr("Stop…"), "teamWorkspaceStop", false, [this, name = team.name] {
            if (m_panel->confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                          "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                m_panel->act("stop", {});
        });
        stop->setProperty("danger", true);
    }
    if (team.status == "done" || team.status == "stopped")
        add(tr("Archive"), "teamWorkspaceArchive", false, [this] { m_panel->act("archive", {}); });
    return bar;
}

QWidget *TeamWorkspace::answers(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *strip = new QFrame; strip->setObjectName("teamWorkspaceAnswers");
    auto *v = new QVBoxLayout(strip); v->setContentsMargins(12, 4, 12, 4); v->setSpacing(2);
    int waiting = 0;
    for (const auto &m : team.members) {
        const auto state = live(m);
        if (m.session.isEmpty() || !state.attention) continue;
        auto *r = row(8);
        if (!waiting++) r->addWidget(label(tr("Needs your answer"), c.fg, 12, true));
        else r->addSpacing(QFontMetrics(font()).horizontalAdvance(tr("Needs your answer")) + 4);
        r->addWidget(new Pill(team.roleOf(m.id), roleColor(team.roleOf(m.id), m_dark)));
        r->addWidget(elided(state.attentionText.isEmpty() ? tr("Question") : state.attentionText, c.fg, 12), 1);
        auto *go = button(tr("Go"), "teamWorkspaceGoToQuestion");
        QObject::connect(go, &QPushButton::clicked, this, [this, host = m.host, session = m.session] { if (openSession) openSession(host, session); });
        r->addWidget(go);
        v->addLayout(r);
    }
    // The slot stays when nothing waits, so the canvas never jumps.
    if (!waiting) {
        strip->setFixedHeight(28);
        v->addWidget(label(tr("No questions waiting"), c.dim, 11));
    }
    strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceAnswers{background:%1;border-bottom:1px solid %2;}")
                             .arg(waiting ? alpha(c.warn, .10).name(QColor::HexArgb) : QStringLiteral("transparent"), c.border.name()));
    return strip;
}

QWidget *TeamWorkspace::toolbar(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *bar = new QWidget; bar->setObjectName("teamWorkspaceToolbar");
    auto *h = new QHBoxLayout(bar); h->setContentsMargins(12, 8, 12, 6); h->setSpacing(8);
    h->addWidget(iconSegmented({{"graph", "view-graph", tr("Graph")}, {"board", "view-board", tr("Board")}, {"layers", "view-layers", tr("Layers")}},
                               m_view, "teamWorkspaceView", c.fg, [this](const QString &value) { m_view = value; m_fit = true; scheduleRender(); }));
    if (team.status == "planning") {
        auto *task = button(tr("+ Task"), "teamWorkspaceAddTask");
        QObject::connect(task, &QPushButton::clicked, this, [this] { if (const auto *t = m_teams->watched()) m_panel->editTask(*t, 0); });
        h->addWidget(task);
    } else if (m_view != "board") {
        // The board's columns already sort tasks by state; the graph dims and
        // the layers hide what the filter leaves out.
        h->addWidget(segmented({{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}, m_filter, "teamWorkspaceFilter",
                               [this](const QString &value) { m_filter = value; scheduleRender(); }));
    }
    if (!m_focus.isEmpty()) {
        auto *chip = button(tr("Member: %1  ×").arg(team.roleOf(m_focus)), "teamWorkspaceFocus"); chip->setProperty("chip", true);
        chip->setToolTip(tr("Show every member again"));
        QObject::connect(chip, &QPushButton::clicked, this, [this] { m_focus.clear(); scheduleRender(); });
        h->addWidget(chip);
    }
    h->addStretch();
    if (m_view == "graph") {
        h->addWidget(label(team.status == "planning" ? tr("Drag a node's right edge onto a task to add a dependency") : tr("Hover = chain · click = details"), c.dim, 11));
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); scheduleRender(); };
        auto *zoom = new QWidget; zoom->setProperty("seg", true); zoom->setAttribute(Qt::WA_StyledBackground);
        auto *zh = new QHBoxLayout(zoom); zh->setContentsMargins(2, 2, 2, 2); zh->setSpacing(0);
        auto *out = button(QStringLiteral("−"), "teamWorkspaceZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        auto *fit = button(tr("Fit"), "teamWorkspaceFit"); QObject::connect(fit, &QPushButton::clicked, this, [this] { m_fit = true; scheduleRender(); });
        auto *in = button(QStringLiteral("+"), "teamWorkspaceZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        fit->setToolTip(tr("Zoom %1%").arg(int(std::round(m_zoom * 100))));
        zh->addWidget(out); zh->addWidget(fit); zh->addWidget(in);
        h->addWidget(zoom);
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
        node.match = (m_filter == "all" || team.status == "planning" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem))
                     && (m_focus.isEmpty() || task->assignee == m_focus);
        view->nodes.append(node);
    }
    // Planning edits the plan here as in the inspector: a click edits the task,
    // a drag from a node's handle adds a dependency, a selected edge goes.
    if (team.status == "planning") {
        view->editable = true;
        view->clicked = [this](int id, const QPoint &) { if (const auto *t = m_teams->watched()) m_panel->editTask(*t, id); };
        view->linked = [this](int dependency, int task) { if (const auto *t = m_teams->watched()) m_panel->link(*t, dependency, task, true); };
        view->unlinked = [this](int dependency, int task) { if (const auto *t = m_teams->watched()) m_panel->link(*t, dependency, task, false); };
    } else view->clicked = [this](int id, const QPoint &) { m_drawerTask = id; scheduleRender(); };
    auto *area = new GraphArea; area->setObjectName("teamWorkspaceCanvas");
    area->setFrameShape(QFrame::NoFrame);
    // Fit starts the graph afresh; otherwise it stays where it was scrolled.
    if (m_fit) m_scroll.remove(QStringLiteral("canvas/") + m_view);
    keepScroll(area, QStringLiteral("canvas/") + m_view);
    area->setWidget(view);
    area->setWidgetResizable(m_view != "graph");
    area->viewport()->setAutoFillBackground(true);
    QPalette p = area->viewport()->palette(); p.setColor(QPalette::Window, c.surface); area->viewport()->setPalette(p);
    if (m_view == "graph") {
        // Opened or Fit: the graph fits the canvas, clamped to 0.8–1.15 (below
        // that it scrolls), and centers; the layer bands fill its height.
        area->resized = [this, view](const QSize &size) {
            view->area = size;
            if (m_fit && size.width() > 40 && size.height() > 40) {
                const QSizeF natural = view->natural();
                const double fitted = qBound(0.8, qMin(size.width() / natural.width(), size.height() / natural.height()), 1.15);
                m_fit = false;
                if (std::abs(fitted - m_zoom) > .005) { m_zoom = fitted; view->zoom = fitted; }
            }
            view->layout();
        };
    } else view->layout();
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
            // Role, agent, state and dependencies share a line while the column
            // is wide enough and wrap when it is not.
            auto *r = new FlowLayout(6);
            const QString role = task.assignee.isEmpty() ? tr("You") : team.roleOf(task.assignee);
            auto *pill = new Pill(role, roleColor(role, m_dark)); pill->setObjectName("teamBoardRole"); r->addWidget(pill);
            if (const auto *m = team.member(task.assignee)) { auto *i = new QLabel; i->setPixmap(workspaceIcon(agentIcon(m->agent), c.text3).pixmap(14, 14)); r->addWidget(i); }
            auto *mark = new StatusMark(s.text, toneColor(c, s.tone), kindOfTask(task, s)); mark->setObjectName("teamBoardState"); r->addWidget(mark);
            QStringList deps; for (const auto &d : task.deps) deps << QStringLiteral("#%1").arg(d.task);
            if (!deps.isEmpty()) r->addWidget(label(QStringLiteral("← ") + deps.join(' '), c.dim, 10));
            b->addLayout(r);
            auto *open = new QPushButton(cardBox); open->setFlat(true); open->setStyleSheet(QStringLiteral("background:transparent;border:none;"));
            open->setGeometry(0, 0, 4000, 4000); open->setObjectName("teamBoardOpen");
            QObject::connect(open, &QPushButton::clicked, this, [this, id = task.id] { m_drawerTask = id; scheduleRender(); });
            list->addWidget(cardBox);
        }
        // Columns by content; an empty one says so.
        if (!n) {
            auto *none = label(tr("Nothing here"), c.dim, 11); none->setAlignment(Qt::AlignCenter); none->setObjectName("teamBoardEmpty");
            none->setStyleSheet(QStringLiteral("color:%1;border:1px dashed %2;border-radius:8px;padding:10px;").arg(c.dim.name(), c.border.name()));
            list->addWidget(none);
        }
        list->addStretch();
        cv->addWidget(label(QStringLiteral("%1 · %2").arg(name.toUpper()).arg(n), c.dim, 10, true));
        auto *inner = new QWidget; inner->setLayout(list);
        auto *scroll = new QScrollArea; scroll->setWidget(inner); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
        scroll->setObjectName("teamBoardColumn"); scroll->setProperty("column", key);
        keepScroll(scroll, QStringLiteral("board/") + key + QLatin1Char('/') + m_focus);
        cv->addWidget(scroll, 1);
        h->addWidget(col, 1);
    }
    return w;
}

QWidget *TeamWorkspace::members(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *w = new QWidget; w->setObjectName("teamWorkspaceMemberList");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(6);
    auto *title = row(6); title->addWidget(label(tr("Members"), c.fg, 12, true)); title->addWidget(label(QString::number(team.members.size()), c.muted, 12)); title->addStretch();
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *add = button(tr("+ Member"), "teamWorkspaceAddMember");
        add->setToolTip(tr("Add a member; it starts with the first task it gets"));
        QObject::connect(add, &QPushButton::clicked, this, [this, id = team.id] { if (const auto *t = m_teams->team(id)) addMember(*t); });
        title->addWidget(add);
    }
    v->addLayout(title);
    for (const auto &m : team.members) {
        const auto state = live(m);
        const auto status = memberStatus(team, m, state);
        auto *b = new MemberCard; b->setObjectName("teamWorkspaceMember"); b->setCursor(Qt::PointingHandCursor); b->setProperty("teamMember", m.id);
        const QColor edge = m.id == m_focus ? c.accent : status.problem ? toneColor(c, status.tone) : c.border;
        b->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceMember{background:%1;border:1px solid %2;border-radius:9px;}").arg(c.card.name(), edge.name()));
        b->setToolTip(m.id == m_focus ? tr("Show every member") : tr("Show only %1's tasks").arg(team.roleOf(m.id)));
        b->clicked = [this, id = m.id] { m_focus = m_focus == id ? QString() : id; scheduleRender(); };
        auto *bh = new QHBoxLayout(b); bh->setContentsMargins(8, 6, 6, 6); bh->setSpacing(8);
        auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(m.agent), c.fg).pixmap(16, 16)); bh->addWidget(tile, 0, Qt::AlignTop);
        auto *col = new QVBoxLayout; col->setSpacing(2);
        auto *top = row(4);
        const QString role = team.roleOf(m.id);
        top->addWidget(new Pill(role, roleColor(role, m_dark)));
        if (state.attention) { auto *q = label(QStringLiteral("?1"), c.warn, 10, true); q->setToolTip(tr("A question waits in its session")); top->addWidget(q); }
        top->addWidget(elided(m.model.isEmpty() ? agentNames.value(m.agent, m.agent) : m.model, c.text3, 11), 1);
        col->addLayout(top);
        int done = 0, total = 0, open = 0;
        for (const auto &t : team.tasks) if (t.assignee == m.id) { ++total; done += t.status == "accepted"; open += !t.finished(); }
        // A member added to a running team waits for its first task.
        const bool waiting = !m.lead && m.session.isEmpty() && !open && m.error.isEmpty() && (team.status == "running" || team.status == "paused");
        if (waiting) {
            auto *wait = row(6);
            wait->addWidget(label(tr("Waiting for a task"), c.muted, 11));
            auto *assign = button(tr("Assign task…"), "teamWorkspaceAssign"); assign->setProperty("ghost", true);
            QObject::connect(assign, &QPushButton::clicked, this, [this, id = m.id, assign] { if (const auto *t = m_teams->watched()) assignMenu(*t, id, assign->mapToGlobal(QPoint(0, assign->height()))); });
            auto *ask = button(tr("Ask lead to plan for it"), "teamWorkspaceAskLead"); ask->setProperty("ghost", true);
            QObject::connect(ask, &QPushButton::clicked, this, [this, role] {
                m_conversation->send(tr("I added %1 to the team. Plan a task for it and reassign it with hgs team reassign.").arg(role));
            });
            wait->addWidget(assign); wait->addWidget(ask); wait->addStretch();
            col->addLayout(wait);
        } else col->addWidget(new StatusMark(team.status == "planning" && m.session.isEmpty() && !m.lead ? tr("On approval") : status.text, toneColor(c, status.tone), status.kind));
        bh->addLayout(col, 1);
        auto *right = new QVBoxLayout; right->setSpacing(3);
        auto *ratio = label(QStringLiteral("%1/%2").arg(done).arg(total), c.muted, 11); ratio->setFont(TeamUi::mono(11)); ratio->setAlignment(Qt::AlignRight);
        right->addWidget(ratio);
        QList<QColor> parts; for (int i = 0; i < qMin(total, 8); ++i) parts << (i < done * qMin(total, 8) / qMax(1, total) ? c.accent : c.border);
        if (parts.isEmpty()) parts << c.border;
        auto *bar = new Progress(parts, 4, c.border); bar->setFixedWidth(26); right->addWidget(bar, 0, Qt::AlignRight);
        bh->addLayout(right);
        auto *more = new QToolButton; more->setIcon(workspaceIcon("more", c.muted)); more->setAutoRaise(true); more->setObjectName("teamWorkspaceMemberMenu");
        more->setToolTip(tr("Member actions"));
        QObject::connect(more, &QToolButton::clicked, this, [this, id = m.id, more] {
            const auto *t = m_teams->watched();
            if (const auto *member = t ? t->member(id) : nullptr) memberMenu(*t, *member, more->mapToGlobal(QPoint(0, more->height())));
        });
        bh->addWidget(more, 0, Qt::AlignTop);
        v->addWidget(b);
    }
    v->addStretch();
    return w;
}

void TeamWorkspace::memberMenu(const Team &team, const Member &member, const QPoint &global)
{
    QMenu menu(this);
    menu.setObjectName("teamWorkspaceMemberActions");
    const QString role = team.roleOf(member.id);
    auto *open = menu.addAction(tr("Open session"), this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
    open->setEnabled(!member.session.isEmpty());
    auto *message = menu.addAction(tr("Message…"), this, [this, host = member.host, session = member.session, role] {
        bool ok = false;
        const auto text = QInputDialog::getMultiLineText(this, tr("Message %1").arg(role), tr("Message"), {}, &ok);
        if (ok && !text.trimmed().isEmpty() && sendMessage) sendMessage(host, session, text.trimmed());
    });
    message->setEnabled(!member.session.isEmpty());
    if (!member.lead) {
        if (member.state == "paused") menu.addAction(tr("Resume member"), this, [this, id = member.id] { m_panel->act("resume-member", {{"member", id}}); });
        else menu.addAction(tr("Pause member"), this, [this, id = member.id] { m_panel->act("pause-member", {{"member", id}}); });
        auto *relaunch = menu.addAction(tr("Relaunch"), this, [this, id = member.id] { m_panel->act("relaunch", {{"member", id}}); });
        const bool failed = member.session.isEmpty() && !member.error.isEmpty();
        relaunch->setEnabled(failed);
        if (!failed) relaunch->setToolTip(tr("Starts automatically with its first task"));
        QStringList open;
        for (const auto &t : team.tasks) if (t.assignee == member.id && !t.finished()) open << QStringLiteral("#%1").arg(t.id);
        auto *remove = menu.addAction(tr("Remove from team…"), this, [this, id = member.id, role, name = team.name] {
            if (m_panel->confirm("teamRemoveMemberConfirm", tr("Remove member?"), tr("Remove %1 from %2? Its session keeps running; it gets no more tasks.").arg(role, name), tr("Remove")))
                m_panel->act("remove-member", {{"member", id}});
        });
        remove->setEnabled(open.isEmpty());
        if (!open.isEmpty()) remove->setToolTip(tr("Has open tasks %1 — reassign or cancel them first").arg(open.join(QStringLiteral(", "))));
        auto *replace = menu.addAction(tr("Replace model… (future)"));
        replace->setEnabled(false); replace->setToolTip(tr("Not supported yet: use Remove + Add member"));
    }
    menu.setToolTipsVisible(true);
    menu.exec(global);
}

void TeamWorkspace::assignMenu(const Team &team, const QString &member, const QPoint &global)
{
    QMenu menu(this);
    menu.setObjectName("teamWorkspaceAssignMenu");
    for (const auto &t : team.tasks) {
        if (t.finished() || t.assignee == member) continue;
        const bool ready = team.ready.contains(t.id);
        auto *action = menu.addAction(QStringLiteral("#%1 %2%3").arg(t.id).arg(t.title, ready ? tr("  · ready") : QString()));
        QObject::connect(action, &QAction::triggered, this, [this, task = t.id, member] { m_panel->act("reassign", {{"task", task}, {"member", member}}); });
    }
    if (menu.isEmpty()) menu.addAction(tr("No open task"))->setEnabled(false);
    menu.exec(global);
}

void TeamWorkspace::renderCoordinator(const Team &team, const QString &status)
{
    const Colors c = colorsFor(m_dark);
    const auto *lead = team.member(team.lead);
    const auto leadStatus = memberStatus(team, lead ? *lead : Member{}, lead && m_live ? m_live(*lead) : Live{});
    // The header is rebuilt whole, and only when what it shows changes, so
    // polls never make it flicker; the conversation below stays.
    const QString shown = lead ? QStringList{lead->agent, lead->host, lead->session, lead->model, leadStatus.text, QString::number(int(leadStatus.tone)),
                                             QString::number(int(m_dark))}.join(QChar(31)) : QString();
    if (m_coordinatorHead->property("shown").toString() != shown || m_coordinatorHead->layout()->isEmpty()) {
    m_coordinatorHead->setProperty("shown", shown);
    auto *outer = static_cast<QHBoxLayout *>(m_coordinatorHead->layout());
    outer->setContentsMargins(0, 0, 0, 0);
    while (auto *item = outer->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    auto *inner = new QWidget;
    auto *h = new QHBoxLayout(inner); h->setContentsMargins(12, 8, 10, 8); h->setSpacing(8);
    if (lead) { auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(lead->agent), c.fg).pixmap(20, 20)); h->addWidget(tile, 0, Qt::AlignTop); }
    auto *col = new QVBoxLayout; col->setSpacing(1);
    auto *top = row(6);
    top->addWidget(label(tr("Coordinator"), c.fg, 13, true));
    if (lead) top->addWidget(new StatusMark(leadStatus.text, toneColor(c, leadStatus.tone), leadStatus.kind));
    top->addStretch();
    col->addLayout(top);
    if (lead) col->addWidget(elided(lead->session + (lead->model.isEmpty() ? QString() : QStringLiteral(" · ") + lead->model), c.dim, 11));
    h->addLayout(col, 1);
    auto *open = new QToolButton; open->setIcon(workspaceIcon("external", c.fg)); open->setAutoRaise(true); open->setToolTip(tr("Open in Activity"));
    open->setObjectName("teamCoordinatorOpen");
    open->setEnabled(lead && !lead->session.isEmpty());
    if (lead) QObject::connect(open, &QToolButton::clicked, this, [this, host = lead->host, session = lead->session] { if (openSession) openSession(host, session); });
    h->addWidget(open, 0, Qt::AlignTop);
    outer->addWidget(inner); inner->show();
    }
    m_conversation->setSession(lead ? lead->host : QString(), lead ? lead->session : QString());
    // Quick asks by the team's state.
    QStringList asks;
    if (team.status == "planning") asks = {tr("Explain the plan"), tr("Split the biggest task")};
    else if (status == "stuck") asks = {tr("What blocks the team?"), tr("Re-plan around the problem")};
    else if (team.status == "running") asks = {tr("Summarize status"), tr("Re-plan remaining work")};
    m_conversation->setQuickAsks(asks);
}

QWidget *TeamWorkspace::feed(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamWorkspaceFeed");
    w->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceFeed{border-top:1px solid %1;}").arg(c.border.name()));
    auto *v = new QVBoxLayout(w); v->setContentsMargins(12, 4, 12, 6); v->setSpacing(2);
    auto *head = row(6);
    auto *toggle = new QToolButton; toggle->setIcon(workspaceIcon(m_feedOpen ? "chevron-down" : "chevron-right", c.fg)); toggle->setAutoRaise(true);
    toggle->setObjectName("teamWorkspaceFeedToggle");
    QObject::connect(toggle, &QToolButton::clicked, this, [this] { m_feedOpen = !m_feedOpen; scheduleRender(); });
    head->addWidget(toggle);
    head->addWidget(label(tr("Feed"), c.fg, 12, true));
    const auto items = m_teams->feed();
    head->addWidget(label(items.size() == 1 ? tr("1 event") : tr("%1 events").arg(items.size()), c.muted, 11));
    // Collapsed, the header carries the latest event.
    if (!m_feedOpen && !items.isEmpty()) {
        const auto line = feedLine(team, items.first());
        head->addWidget(elided(QStringLiteral("· %1 %2").arg(clock(line.at), line.text), c.text3, 11), 1);
    } else head->addStretch();
    if (m_teams->feedHasOlder() && m_feedOpen) {
        auto *older = button(tr("Load older"), "teamWorkspaceFeedOlder"); older->setProperty("ghost", true);
        QObject::connect(older, &QPushButton::clicked, this, [this] { m_teams->loadOlder(); });
        head->addWidget(older);
    }
    v->addLayout(head);
    if (!m_feedOpen) return w;
    auto *rows = new QWidget; auto *rv = new QVBoxLayout(rows); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(1);
    const QString focusRole = m_focus.isEmpty() ? QString() : team.roleOf(m_focus);
    for (const auto &event : items) {
        const auto line = feedLine(team, event);
        if (!focusRole.isEmpty() && line.by != focusRole && !line.text.contains(focusRole)) continue;
        auto *r = row(6);
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(TeamUi::mono(11)); time->setFixedWidth(38); r->addWidget(time);
        if (!line.by.isEmpty()) r->addWidget(new Pill(line.by, roleColor(line.by, m_dark)));
        auto *t = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 12); t->setWordWrap(true);
        r->addWidget(t, 1);
        rv->addLayout(r);
    }
    rv->addStretch();
    auto *scroll = new QScrollArea; scroll->setWidget(rows); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setFixedHeight(170);
    keepScroll(scroll, QStringLiteral("feed"));
    v->addWidget(scroll);
    return w;
}

void TeamWorkspace::keepScroll(QScrollArea *area, const QString &key)
{
    const QPoint kept = m_scroll.value(key);
    for (auto *bar : {area->horizontalScrollBar(), area->verticalScrollBar()}) {
        const bool vertical = bar->orientation() == Qt::Vertical;
        const int want = vertical ? kept.y() : kept.x();
        // Back to the kept place once the content has its size; until then
        // the new bar's own changes are not the owner's.
        auto restored = std::make_shared<bool>(want == 0);
        QObject::connect(bar, &QScrollBar::rangeChanged, bar, [bar, want, restored](int, int max) {
            if (*restored || max <= 0) return;
            bar->setValue(qMin(want, max));
            *restored = true;
        });
        QObject::connect(bar, &QScrollBar::valueChanged, area, [this, area, key, vertical, restored](int value) {
            if (!*restored || !area->isVisible()) return;
            QPoint &place = m_scroll[key];
            (vertical ? place.ry() : place.rx()) = value;
        });
    }
}

void TeamWorkspace::addMember(const Team &team)
{
    if (!m_teams->catalogReady()) m_teams->refreshCatalog();
    if (m_popover) { retire(m_popover); m_popover = nullptr; }
    const Colors c = colorsFor(m_dark);
    auto *pop = new QFrame(this); pop->setObjectName("teamAddMember");
    pop->setStyleSheet(QStringLiteral("QFrame#teamAddMember{background:%1;border:1px solid %2;border-radius:12px;}").arg(c.surface.name(), c.border.name()));
    pop->setAttribute(Qt::WA_StyledBackground);
    m_popover = pop;
    auto *v = new QVBoxLayout(pop); v->setContentsMargins(14, 12, 14, 12); v->setSpacing(8);
    auto *head = row(6);
    head->addWidget(label(tr("Add member"), c.fg, 13, true));
    head->addWidget(elided(tr("to %1").arg(team.name), c.dim, 12), 1);
    auto *close = new QToolButton; close->setIcon(workspaceIcon("close", c.muted)); close->setAutoRaise(true);
    head->addWidget(close);
    v->addLayout(head);
    const auto &catalog = m_teams->catalog();
    v->addWidget(label(catalog.builtin ? tr("Role from the built-in catalog") : tr("Role from catalog v%1").arg(catalog.version), c.muted, 11));
    // Role chips: picking one brings its defaults.
    auto *chips = new QGridLayout; chips->setSpacing(4);
    auto *agent = new QComboBox; agent->setObjectName("teamAddMemberAgent");
    for (const auto &a : agents) agent->addItem(workspaceIcon(a, c.fg), agentNames.value(a), a);
    auto *model = new QLineEdit; model->setObjectName("teamAddMemberModel"); model->setPlaceholderText(tr("default model"));
    auto *effort = new QComboBox; effort->setObjectName("teamAddMemberEffort"); effort->addItem(QStringLiteral("—"), QString());
    for (const auto *e : {"low", "medium", "high", "xhigh"}) effort->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
    auto *account = new QComboBox; account->setObjectName("teamAddMemberAccount"); account->addItem(tr("Default account"), QString());
    auto *computer = new QComboBox; computer->setObjectName("teamAddMemberComputer");
    computer->addItem(tr("this computer"), QString());
    for (const auto &k : m_computers) if (!k.alias.isEmpty()) computer->addItem(k.label, k.alias);
    auto *permissions = label(QString(), c.text2, 12); permissions->setObjectName("teamAddMemberPermissions");
    auto *prompt = new QPlainTextEdit; prompt->setObjectName("teamAddMemberPrompt"); prompt->setFixedHeight(70);
    prompt->setPlaceholderText(tr("What this member is for"));
    auto *offline = label(QString(), c.warn, 11); offline->setWordWrap(true); offline->hide();
    auto *ok = button(tr("Add member"), "teamAddMemberOk", true);
    auto picked = std::make_shared<QString>();
    int at = 0;
    for (const auto &r : catalog.roles) {
        if (r.id == "lead") continue;
        auto *chip = button(r.name, "teamAddMemberRole"); chip->setProperty("chip", true); chip->setCheckable(true); chip->setProperty("role", r.id);
        const QColor tone = r.color.isEmpty() ? roleColor(r.name, m_dark) : QColor(r.color);
        chip->setStyleSheet(QStringLiteral("QPushButton{color:%1;border-color:%2;} QPushButton:checked{background:%3;border-color:%1;}")
                                .arg(tone.name(), alpha(tone, .5).name(QColor::HexArgb), alpha(tone, .18).name(QColor::HexArgb)));
        chips->addWidget(chip, at / 4, at % 4);
        ++at;
        QObject::connect(chip, &QPushButton::clicked, this, [this, id = r.id, picked, chip, agent, model, effort, permissions, prompt, ok] {
            *picked = id;
            for (auto *b : chip->parentWidget()->findChildren<QPushButton *>("teamAddMemberRole")) b->setChecked(b == chip);
            const auto *role = m_teams->catalog().role(id);
            if (!role) return;
            agent->setCurrentIndex(qMax(0, agent->findData(role->agent.isEmpty() ? QStringLiteral("claude") : role->agent)));
            model->setText(role->model);
            effort->setCurrentIndex(qMax(0, effort->findData(role->effort)));
            const QString mode = role->permissions == "auto" ? tr("Auto") : role->permissions == "bypass" ? tr("Bypass") : tr("Account's mode");
            const QString folder = role->folder == "read_only" ? tr("Read only") : role->folder == "shared" ? tr("Shared folder") : tr("Own worktree");
            permissions->setText(QStringLiteral("%1 · %2").arg(mode, folder));
            prompt->setPlainText(role->prompt);
            ok->setText(tr("Add %1").arg(role->name));
            ok->setEnabled(true);
        });
    }
    v->addLayout(chips);
    auto *form = new QGridLayout; form->setHorizontalSpacing(8); form->setVerticalSpacing(6); form->setColumnStretch(1, 1);
    const auto field = [&](int r, const QString &text, QWidget *w) { auto *l = label(text, c.muted, 12); l->setAlignment(Qt::AlignRight | Qt::AlignVCenter); form->addWidget(l, r, 0); form->addWidget(w, r, 1); };
    field(0, tr("Agent"), agent);
    auto *modelRow = new QWidget; auto *mh = new QHBoxLayout(modelRow); mh->setContentsMargins(0, 0, 0, 0); mh->setSpacing(6); mh->addWidget(model, 1); mh->addWidget(effort);
    field(1, tr("Model"), modelRow);
    field(2, tr("Account"), account);
    field(3, tr("Computer"), computer);
    field(4, tr("Permissions"), permissions);
    field(5, tr("Prompt"), prompt);
    v->addLayout(form);
    v->addWidget(offline);
    auto *note = label(tr("Defaults come from the role. The session starts when the member gets its first task."), c.dim, 11); note->setWordWrap(true);
    v->addWidget(note);
    auto *buttons = row(8);
    auto *cancel = button(tr("Cancel"), "teamAddMemberCancel");
    buttons->addWidget(cancel); buttons->addWidget(ok); buttons->addStretch();
    v->addLayout(buttons);
    ok->setEnabled(false);
    // Accounts of the chosen computer and agent.
    const auto fillAccounts = [this, account, agent, computer] { m_teams->client()->requestAccounts(computer->currentData().toString()); Q_UNUSED(agent); Q_UNUSED(account); };
    QObject::connect(m_teams->client(), &HgsClient::accountsReady, pop, [account, agent, computer](quint64, const QString &host, const QJsonObject &result) {
        if (host != computer->currentData().toString()) return;
        const QString keep = account->currentData().toString();
        account->clear(); account->addItem(QCoreApplication::translate("TeamWorkspace", "Default account"), QString());
        for (const auto &value : result.value("profiles").toArray()) {
            const auto p = value.toObject();
            if (p.value("provider").toString() != agent->currentData().toString()) continue;
            const bool out = p.value("account_status").toObject().value("status").toString() == "signed_out";
            account->addItem(p.value("label").toString(p.value("id").toString()) + (out ? QCoreApplication::translate("TeamWorkspace", " (signed out)") : QString()), p.value("id").toString());
        }
        account->setCurrentIndex(qMax(0, account->findData(keep)));
    });
    QObject::connect(agent, &QComboBox::activated, pop, fillAccounts);
    QObject::connect(computer, &QComboBox::activated, pop, [this, fillAccounts, computer, offline] {
        fillAccounts();
        QString warn;
        for (const auto &k : m_computers) if (k.alias == computer->currentData().toString() && !k.online) warn = tr("Computer %1 is offline; the member starts once it is back.").arg(k.label);
        offline->setText(warn); offline->setVisible(!warn.isEmpty());
        placePopover();
    });
    fillAccounts();
    QObject::connect(close, &QToolButton::clicked, pop, [this] { if (m_popover) { retire(m_popover); m_popover = nullptr; } });
    QObject::connect(cancel, &QPushButton::clicked, pop, [this] { if (m_popover) { retire(m_popover); m_popover = nullptr; } });
    QObject::connect(ok, &QPushButton::clicked, pop, [this, team, picked, agent, model, effort, account, computer, prompt] {
        const auto *role = m_teams->catalog().role(*picked);
        if (!role) return;
        // New members share the team's folder policy: a worktree when others have one.
        QString folder = QStringLiteral("shared");
        for (const auto &m : team.members) if (!m.lead && m.folder == "worktree" && role->folder != "read_only" && role->folder != "shared") folder = m.folder;
        const QJsonObject member{{"id", nextMemberId(team, role->name)}, {"role", role->name}, {"agent", agent->currentData().toString()},
                                 {"model", model->text().trimmed()}, {"effort", effort->currentData().toString()}, {"account", account->currentData().toString()},
                                 {"computer", computer->currentData().toString()}, {"permissions", role->permissions}, {"folder", folder},
                                 {"prompt", prompt->toPlainText().trimmed()}};
        m_panel->act(QStringLiteral("add-member"), {{"member", member}});
        if (m_popover) { retire(m_popover); m_popover = nullptr; }
    });
    pop->show();
    placePopover();
}
