#include "TeamWorkspace.h"

#include <QPointer>
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGridLayout>
#include <QInputDialog>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QRadioButton>
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

// The board's columns, by what a task needs. Accepted, rejected and cancelled
// are all decided, but only an accepted task is done: the others get their own
// column so a rejected task never sits under Accepted (QA).
QString column(const Task &task, bool problem)
{
    if (problem) return QStringLiteral("problems");
    if (task.status == "accepted") return QStringLiteral("accepted");
    if (task.status == "rejected" || task.status == "cancelled") return QStringLiteral("decided");
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
    // U25 (spec 3): the team's status and actions, for the lead session's header.
    // They stay here, above the canvas, until a window takes them (takeHeader).
    m_headerStrip = new QFrame; m_headerStrip->setObjectName("teamWorkspaceHeader"); m_headerStrip->hide();
    m_headerStatus = new QWidget; m_headerStatus->setObjectName("teamHeaderProgress");
    m_headerActions = new QWidget; m_headerActions->setObjectName("teamHeaderActions");
    for (QWidget *box : {m_headerStatus.data(), m_headerActions.data()}) {
        auto *l = new QHBoxLayout(box); l->setContentsMargins(0, 0, 0, 0); l->setSpacing(0);
        box->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed);
    }
    {auto *h = new QHBoxLayout(m_headerStrip); h->setContentsMargins(12, 6, 12, 6); h->setSpacing(10);
     h->addWidget(m_headerStatus, 1); h->addStretch(); h->addWidget(m_headerActions);}
    outer->addWidget(m_headerStrip);
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
    if (!m_split->restoreState(settings.value("teams/workspaceSplit").toByteArray())) m_split->setSizes({900, 520});
    if (!m_sideSplit->restoreState(settings.value("teams/workspaceSideSplit").toByteArray())) m_sideSplit->setSizes({420, 520});
    QObject::connect(m_split, &QSplitter::splitterMoved, this, [this] { QSettings().setValue("teams/workspaceSplit", m_split->saveState()); });
    QObject::connect(m_sideSplit, &QSplitter::splitterMoved, this, [this] { QSettings().setValue("teams/workspaceSideSplit", m_sideSplit->saveState()); });
    m_membersOpen = QSettings().value("teams/membersOpen", true).toBool();
    m_drawer = new TeamTaskCard(this);
    m_drawer->setObjectName("teamWorkspaceDrawer");
    m_drawer->hide();
    m_drawer->back = [this] { m_drawerTask = 0; m_drawer->hide(); };
    m_drawer->act = [this](const QString &operation, const QJsonObject &input) { m_panel->act(operation, input); };
    m_drawer->openTask = [this](int task) { m_drawerTask = task; scheduleRender(); };
    m_drawer->openTeam = [this](const QString &team) { m_teams->watch(team); showTeam(team); };
    m_drawer->openSession = [this](const QString &host, const QString &session) { if (openSession) openSession(host, session); };
    m_drawer->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return m_panel->confirm(name, title, text, action); };
    // The panel may outlive this workspace.
    m_panel->confirmClosed = [alive = QPointer<TeamWorkspace>(this)] { if (alive) alive->scheduleRender(); };
    m_drawer->reassign = [this](int task, const QPoint &global) { if (const auto *team = m_teams->watched()) m_panel->reassignMenu(*team, task, global); };
    m_drawer->problemActions = [this](const Member &member, const Status &status) -> QWidget * {
        const auto *team = m_teams->watched();
        return team ? m_panel->problemActions(*team, member, status) : nullptr;
    };
    QObject::connect(m_teams, &TeamController::teamsChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::watchedChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::feedChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::inboxChanged, this, [this] { scheduleRender(); });
    setTheme(true);
}

TeamWorkspace::~TeamWorkspace()
{
    // Header parts taken but never placed in a window have no other owner.
    for (QWidget *part : {m_headerStatus.data(), m_headerActions.data()}) if (part && !part->parentWidget()) delete part;
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
    // The prototype's .btn.sm buttons and its header pills (.pill.planp, .pill.acc).
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    const QString selBorder = WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("cardSelBorder")).name();
    if (!m_headerStatus || !m_headerActions) { m_conversation->setTheme(dark); scheduleRender(); return; }
    m_headerActions->setStyleSheet(QStringLiteral(
        "QPushButton{background:%1;color:%2;border:1px solid %3;border-radius:7px;padding:2px 9px;font-size:11px;min-height:17px;}"
        "QPushButton:hover{background:%4;color:%5;}"
        "QPushButton:disabled{color:%6;border-color:%7;}"
        "QPushButton[primary=\"true\"]{background:%8;border-color:%8;color:%9;font-weight:600;}"
        "QPushButton[primary=\"true\"]:hover{background:%10;border-color:%10;}"
        "QPushButton[primary=\"true\"]:disabled{background:%11;color:%12;border-color:transparent;}"
        "QPushButton[danger=\"true\"]{color:%13;}"
        "QPushButton[ghost=\"true\"]{background:transparent;border-color:transparent;}"
        "QPushButton[ghost=\"true\"]:hover{background:%4;}"
        "QPushButton:checked{background:%14;border-color:%15;color:%8;}")
        .arg(c.surface.name(), c.text2.name(), c.border.name(), c.hover.name(), c.fg.name(), a(c.text2, .42), a(c.border, .6),
             c.accent.name(), c.accentText.name())
        .arg(c.accentHover.name(), a(c.accent, .42), a(c.accentText, .7), c.danger.name(), c.segOn.name(), selBorder));
    m_headerStatus->setStyleSheet(QStringLiteral(
        "QPushButton[link=\"plan\"]{background:%1;color:%2;border:1px solid %3;border-radius:6px;padding:0 7px;font-size:11px;min-height:17px;max-height:17px;}"
        "QPushButton[link=\"plan\"]:hover{background:%4;}"
        "QPushButton[link=\"accent\"]{background:%5;color:%6;border:1px solid transparent;border-radius:6px;padding:0 7px;font-size:11px;min-height:17px;max-height:17px;}"
        "QPushButton[link=\"accent\"]:hover{background:%7;}")
        .arg(a(c.folder, .13), c.folder.name(), a(c.folder, .6), a(c.folder, .22), a(c.accent, .10), c.accent.name(), a(c.accent, .18)));
    m_headerStrip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceHeader{border-bottom:1px solid %1;}").arg(c.border.name()));
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
    renderHeader();
    if (!isVisible()) return;
    // A confirmation is open (TeamPanel::confirm): its asker stays alive.
    if (m_panel->confirming()) return;
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
        // G5: the drafts wait in the members column; the canvas keeps its height.
        if (team->hasFinish) tv->addWidget(finishSummary(*team));
        // B5: a plan that widens a role's permissions waits for the owner's
        // explicit confirmation; the strip names what widens.
        if (!team->widenings.isEmpty()) tv->addWidget(wideningStrip(*team));
        // F8: the lead's open membership proposals.
        if (auto *proposals = proposalsStrip(*team)) tv->addWidget(proposals);
        // C2/C4: the lead's inbox and the owner's attention list.
        if (const auto &items = m_teams->inbox(); !items.isEmpty()) tv->addWidget(inboxStrip(*team, items));
        if (!team->attention.isEmpty()) tv->addWidget(attentionStrip(*team));
        if (!m_error.isEmpty()) tv->addWidget(errorStrip());
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

void TeamWorkspace::takeHeader()
{
    if (m_headerTaken) return;
    m_headerTaken = true;
    m_headerStrip->layout()->removeWidget(m_headerStatus);
    m_headerStrip->layout()->removeWidget(m_headerActions);
    m_headerStatus->setParent(nullptr); m_headerActions->setParent(nullptr);
    m_headerStrip->hide();
    renderHeader();
}

void TeamWorkspace::setHeaderShown(bool shown)
{
    if (shown == m_headerShown) return;
    m_headerShown = shown;
    renderHeader();
}

void TeamWorkspace::showPhase(int phase)
{
    if (const auto *team = currentTeam(m_team)) {
        for (const auto &group : phaseGroups(*team))
            if (group.n == phase && !m_panel->phaseOpen(*team, group)) m_panel->togglePhase(*team, phase);
    }
    m_view = QStringLiteral("graph"); m_drawerTask = 0;
    if (openWorkspace) openWorkspace();
    scheduleRender();
}

// U25 (spec 3): rebuilt on every render, also while the Team tab is hidden: the
// header shows the team whichever tab is open. A confirmation keeps its asker.
void TeamWorkspace::renderHeader()
{
    if (m_panel->confirming() || !m_headerStatus || !m_headerActions) return;
    const Team *team = m_team.isEmpty() ? nullptr : currentTeam(m_team);
    const bool shown = team && m_headerShown && team->status != "archived" && !team->draft();
    for (auto *content : {m_headerStatusContent.data(), m_headerActionsContent.data()}) {
        if (!content) continue;
        if (auto *box = content->parentWidget(); box && box->layout()) box->layout()->removeWidget(content);
        retire(content);
    }
    m_headerStatusContent = nullptr; m_headerActionsContent = nullptr;
    if (shown) {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        m_headerStatusContent = headerStatusContent(*team, displayStatus(*team, live));
        m_headerActionsContent = headerActionsContent(*team);
        // Shown at once: a layout would show them only on the next event-loop pass.
        m_headerStatus->layout()->addWidget(m_headerStatusContent); m_headerStatusContent->show();
        m_headerActions->layout()->addWidget(m_headerActionsContent); m_headerActionsContent->show();
    }
    // Taken but not yet placed, they must not open as windows of their own.
    const bool placed = !m_headerTaken || (m_headerStatus->parentWidget() && m_headerActions->parentWidget());
    m_headerStatus->setVisible(shown && placed); m_headerActions->setVisible(shown && placed);
    m_headerStrip->setVisible(shown && !m_headerTaken);
}

QWidget *TeamWorkspace::headerStatusContent(const Team &team, const QString &status)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *row = new FitRow(12); row->setObjectName("teamHeaderProgressRow");
    // The window's own label size must not apply: these are the header's 11 px.
    const auto small = [](QLabel *text, const QColor &color) {
        text->setStyleSheet(QStringLiteral("QLabel{font-size:11px;color:%1;background:transparent;}").arg(color.name())); return text;
    };
    // G1: where this sub-team sits: parent name / this name.
    if (team.parent.known) {
        const auto *parent = m_teams->team(team.parent.team);
        auto *crumb = small(label(QStringLiteral("%1 / %2").arg(parent ? parent->name : team.parent.team, team.name), c.dim, 11, false, "teamWorkspaceBreadcrumb"), c.dim);
        crumb->setToolTip(tr("Sub-team of task #%1 in %2").arg(team.parent.task).arg(parent ? parent->name : team.parent.team));
        row->add(crumb, 9);
    }
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, status == "stuck");
    badge->setObjectName("teamHeaderStatus"); badge->setProperty("status", status);
    badge->setToolTip(tr("Team %1 · %2").arg(team.name, statusLabel(status)));
    row->add(badge, 0);
    // The phase draft that waits for the owner, as a link to it (not while planning).
    if (team.pendingPhase > 0 && team.status != "planning") {
        const auto drafts = team.draftPhases();
        QStringList ids; for (int n : drafts) ids << QString::number(n);
        auto *pending = button(tr("Phase %1 waits for you").arg(team.pendingPhase), "teamBarPendingPhase");
        pending->setProperty("link", "plan");
        pending->setToolTip(drafts.size() > 1 ? tr("phases %1 pending approval").arg(ids.join(QStringLiteral(", "))) : tr("Open the phase draft"));
        QObject::connect(pending, &QPushButton::clicked, this, [this, phase = team.pendingPhase] { showPhase(phase); });
        row->add(pending, 1);
    }
    if (allDecided(team) && (team.status == "running" || team.status == "paused")) {
        auto *finish = button(tr("Waiting for you to finish"), "teamHeaderFinishWaits");
        finish->setProperty("link", "accent");
        finish->setToolTip(tr("Every task is decided"));
        QObject::connect(finish, &QPushButton::clicked, this, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) m_panel->finishTeam(*current);
        });
        row->add(finish, 1);
    }
    if (team.waitingForLead) {
        auto *waiting = new Pill(tr("waiting for the lead"), c.accent);
        waiting->setObjectName("teamBarWaitingForLead");
        row->add(waiting, 6);
    }
    // Three bars: the phase in work (or the next draft), the team's tasks, the phases.
    const auto progress = phaseProgress(team);
    if (progress.valid()) {
        const QString caption = progress.title.isEmpty() ? tr("Phase %1").arg(progress.phase)
                                                         : tr("Phase %1 · %2").arg(progress.phase).arg(progress.title);
        auto *phase = new MiniProgress(c, caption, progress.done, progress.total, c.accent, 150);
        phase->setObjectName("teamHeaderPhase");
        phase->setToolTip(tr("Current phase: decided / tasks in phase %1").arg(progress.phase));
        row->add(phase, 5);
    }
    int decided = 0; for (const auto &task : team.tasks) decided += task.finished();
    auto *tasks = new MiniProgress(c, tr("Tasks"), decided, int(team.tasks.size()), c.accent, 72);
    tasks->setObjectName("teamHeaderTasks"); tasks->setToolTip(tr("Decided / all tasks of the team"));
    row->add(tasks, 7);
    if (progress.phasesTotal > 0) {
        auto *phases = new MiniProgress(c, tr("Phases"), progress.phasesDone, progress.phasesTotal, c.accentMuted, 72);
        phases->setObjectName("teamHeaderPhases"); phases->setToolTip(tr("Done / all phases"));
        row->add(phases, 8);
    }
    if (const auto found = problems(team, live); !found.isEmpty()) {
        auto *count = small(label(found.size() == 1 ? tr("1 problem") : tr("%1 problems").arg(found.size()), c.warn, 11, false, "teamHeaderProblems"), c.warn);
        QStringList lines; for (const auto &problem : found) lines << problem.text;
        count->setToolTip(lines.join(QLatin1Char('\n')));
        row->add(count, 2);
    }
    // D2: findings by severity, with the deferred count (Findings… is an action).
    const auto openFindings = team.findingsSummary.value("open").toObject();
    const int critical = openFindings.value("critical").toInt(), normal = openFindings.value("normal").toInt();
    const int later = openFindings.value("later").toInt(), deferred = team.findingsSummary.value("deferred").toInt();
    if (team.findingsKnown && critical + normal + later + deferred > 0) {
        auto *pill = new Pill(tr("%1 critical · %2 normal · %3 later · %4 deferred").arg(critical).arg(normal).arg(later).arg(deferred),
                              critical > 0 ? c.danger : c.warn);
        pill->setObjectName("teamBarFindings");
        row->add(pill, 4);
    }
    if (team.created > 0) {
        auto *elapsed = new QWidget; elapsed->setObjectName("teamHeaderElapsed");
        auto *h = new QHBoxLayout(elapsed); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(4);
        auto *clock = new QLabel; clock->setPixmap(workspaceIcon("clock", c.muted).pixmap(12, 12)); h->addWidget(clock);
        const QString text = duration(QDateTime::currentSecsSinceEpoch() - team.created);
        h->addWidget(small(label(text, c.muted, 11, false, "teamHeaderElapsedText"), c.muted));
        elapsed->setToolTip(tr("Elapsed"));
        row->add(elapsed, 3);
    }
    return row;
}

QWidget *TeamWorkspace::headerActionsContent(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *bar = new QWidget; bar->setObjectName("teamHeaderActionRow");
    auto *h = new QHBoxLayout(bar); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
    const auto iconFor = [&c](QPushButton *b, const char *glyph) {
        const QColor tone = b->property("primary").toBool() ? c.accentText : b->property("danger").toBool() ? c.danger : c.text2;
        b->setIcon(workspaceIcon(QString::fromLatin1(glyph), tone)); b->setIconSize(QSize(12, 12));
    };
    const auto add = [&](const QString &text, const char *objectName, bool primary, const std::function<void()> &run) {
        auto *b = button(text, objectName, primary); QObject::connect(b, &QPushButton::clicked, this, run); h->addWidget(b); return b;
    };
    // D2: the findings and their actions.
    const auto openFindings = team.findingsSummary.value("open").toObject();
    if (team.findingsKnown && openFindings.value("critical").toInt() + openFindings.value("normal").toInt()
                                  + openFindings.value("later").toInt() + team.findingsSummary.value("deferred").toInt() > 0) {
        auto *findings = add(tr("Findings…"), "teamWorkspaceFindings", false, [this, team] {
            auto *dialog = new QDialog(this);
            dialog->setObjectName("teamFindingsDialog");
            dialog->setAttribute(Qt::WA_DeleteOnClose);
            dialog->setWindowTitle(tr("Findings · %1").arg(team.name));
            dialog->resize(680, 520);
            auto *outer = new QVBoxLayout(dialog);
            auto *scroll = new QScrollArea; scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
            auto *body = new QWidget; body->setObjectName("teamFindingsList");
            auto *list = new QVBoxLayout(body); list->setContentsMargins(10, 10, 10, 10); list->setSpacing(8);
            const Colors fc = colorsFor(m_dark);
            for (const auto &finding : team.findings) {
                auto *row = new QWidget; row->setObjectName("teamFindingRow");
                auto *rv = new QVBoxLayout(row); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(3);
                const QColor severity = finding.severity == "critical" ? fc.danger : finding.severity == "normal" ? fc.warn : fc.muted;
                rv->addWidget(label(QStringLiteral("%1 · %2").arg(finding.severity.toUpper(), finding.title), severity, 12, true));
                if (!finding.scenario.isEmpty()) {
                    auto *scenario = label(finding.scenario, fc.text2, 11); scenario->setWordWrap(true); rv->addWidget(scenario);
                }
                QString where = finding.file;
                if (!where.isEmpty() && finding.line > 0) where += QStringLiteral(":%1").arg(finding.line);
                if (!where.isEmpty()) rv->addWidget(label(where, fc.muted, 11));
                if (!finding.state.isEmpty()) rv->addWidget(label(tr("state: %1%2").arg(finding.state,
                    finding.note.isEmpty() ? QString() : tr(" — %1").arg(finding.note)), fc.muted, 11));
                auto *actions = new QHBoxLayout; actions->setContentsMargins(0, 2, 0, 0); actions->setSpacing(6);
                // D2: the finding actions; each needs a confirmation.
                const QList<QPair<QString, QString>> choices{
                    {QStringLiteral("reopen"), tr("Reopen")}, {QStringLiteral("urgent"), tr("Urgent phase")},
                    {QStringLiteral("replan"), tr("Replan")}, {QStringLiteral("defer"), tr("Defer")},
                    {QStringLiteral("fixed"), tr("Fixed")}, {QStringLiteral("dropped"), tr("Dropped")}};
                for (const auto &choice : choices) {
                    auto *action = new QPushButton(choice.second);
                    action->setObjectName(QStringLiteral("teamFindingAction_%1").arg(choice.first));
                    action->setFlat(true);
                    QObject::connect(action, &QPushButton::clicked, this, [this, finding, choice] {
                        if (!m_panel->confirm("teamFindingActionConfirm", choice.second,
                                              tr("%1 finding \"%2\"?").arg(choice.second, finding.title), choice.second))
                            return;
                        QJsonObject input{{"finding", finding.id}, {"action", choice.first}, {"note", QString()}};
                        bool ok = false;
                        if (finding.task.toInt(&ok) && ok) input["task"] = finding.task.toInt();
                        m_teams->act("finding", input);
                    });
                    actions->addWidget(action);
                }
                actions->addStretch();
                rv->addLayout(actions);
                auto *line = new QFrame; line->setFrameShape(QFrame::HLine); line->setStyleSheet(QStringLiteral("color:%1;").arg(fc.border.name()));
                list->addWidget(row); list->addWidget(line);
            }
            if (team.findings.isEmpty()) list->addWidget(label(tr("No findings on this page."), fc.muted, 12));
            list->addStretch();
            scroll->setWidget(body);
            outer->addWidget(scroll, 1);
            auto *buttons = new QDialogButtonBox(QDialogButtonBox::Close);
            QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
            outer->addWidget(buttons);
            dialog->show();
        });
        findings->setProperty("ghost", true);
    }
    // F7: the team a next goal came from, and the one it led to.
    for (const auto &[id, text, objectName] : {std::tuple{team.previous, tr("← Previous"), "teamBarPrevious"},
                                               std::tuple{team.next, tr("Next goal →"), "teamBarNext"}}) {
        if (id.isEmpty()) continue;
        auto *link = button(text, objectName); link->setProperty("ghost", true);
        const auto *other = m_teams->team(id);
        link->setToolTip(other ? other->name : id);
        QObject::connect(link, &QPushButton::clicked, this, [this, id = id] { openOtherTeam(id); });
        h->addWidget(link);
    }
    // H1: members stopped at a usage limit wait for their reset and go on.
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *limits = button(tr("Resume after limits"), "teamWorkspaceResumeLimits");
        limits->setCheckable(true); limits->setChecked(team.resumeAfterLimits);
        limits->setToolTip(team.resumeAfterLimits
            ? tr("On: a member stopped by a usage limit with a known reset time continues after the reset")
            : tr("Off: a member stopped by a usage limit waits for you or the lead"));
        QObject::connect(limits, &QPushButton::clicked, this, [this, settings = team.settingsJson](bool on) {
            QJsonObject changed = settings;
            changed.insert("resume_after_limits", on);
            m_panel->act("settings", {{"settings", changed}});
        });
        h->addWidget(limits);
    }
    // F7: a Done team takes the next goal with the same people.
    if (team.status == "done" && team.next.isEmpty())
        add(tr("Next goal…"), "teamWorkspaceNextGoal", true, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) nextGoalDialog(*current);
        });
    if (allDecided(team) && (team.status == "running" || team.status == "paused")) {
        auto *finish = add(tr("Finish…"), "teamWorkspaceFinish", true, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) m_panel->finishTeam(*current);
        });
        finish->setToolTip(tr("Every task is decided")); iconFor(finish, "check-circle");
    }
    if (team.status == "planning") {
        auto *discard = add(tr("Discard plan…"), "teamWorkspaceDiscard", false, [this, name = team.name] {
            if (m_panel->confirm("teamDiscardConfirm", tr("Discard plan?"), tr("Discard the plan of %1? The team stops before any member starts.").arg(name), tr("Discard plan")))
                m_panel->act("discard", {});
        });
        discard->setProperty("danger", true); discard->setProperty("ghost", true); discard->setToolTip(tr("Drop the draft"));
        auto *back = add(tr("Send back"), "teamWorkspaceSendBack", false, [this] {
            bool ok = false;
            const auto comment = QInputDialog::getMultiLineText(this, tr("Send back to the coordinator"), tr("What should change in the plan?"), {}, &ok);
            if (ok && !comment.trimmed().isEmpty()) m_conversation->send(tr("Your plan needs changes before I approve it: %1").arg(comment.trimmed()));
        });
        back->setToolTip(tr("Ask the coordinator to change the plan")); iconFor(back, "arrow-left");
        auto *approve = add(tr("Approve && Run"), "teamWorkspaceApprove", true, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) approveTeam(*current);
        });
        iconFor(approve, "play");
        approve->setEnabled(team.errors.isEmpty() && !team.tasks.isEmpty());
        if (!approve->isEnabled()) approve->setToolTip(team.tasks.isEmpty() ? tr("The plan has no tasks yet") : tr("Fix the plan errors first"));
        else if (!team.widenings.isEmpty()) approve->setToolTip(tr("A permission widening needs your confirmation"));
        else approve->setToolTip(tr("Create members and hand out ready tasks"));
    } else if (team.status == "running") {
        auto *pause = add(tr("Pause"), "teamWorkspacePause", false, [this, team] {
            const auto impact = subtreeImpact(team, m_teams->teams());
            if (impact.first > 0 && !m_panel->confirm("teamPauseTreeConfirm", tr("Pause the team tree?"),
                    tr("Pause %1? It also pauses %2 sub-teams and %3 member sessions in them.").arg(team.name).arg(impact.first).arg(impact.second),
                    tr("Pause")))
                return;
            m_panel->act("pause", {});
        });
        pause->setToolTip(tr("Running turns finish; nothing new is handed out")); iconFor(pause, "pause");
    } else if (team.status == "paused") {
        auto *resume = add(tr("Resume"), "teamWorkspaceResume", true, [this] { m_panel->act("resume", {}); });
        resume->setToolTip(tr("Hand out tasks again")); iconFor(resume, "play");
    }
    if (team.status == "running" || team.status == "paused") {
        auto *stop = add(tr("Stop…"), "teamWorkspaceStop", false, [this, name = team.name, team] {
            const auto impact = subtreeImpact(team, m_teams->teams());
            QString text = tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                              "Member sessions keep running; their current turns finish.").arg(name);
            if (impact.first > 0)
                text += tr(" It stops %1 sub-teams and affects %2 member sessions in them.").arg(impact.first).arg(impact.second);
            if (m_panel->confirm("teamStopConfirm", tr("Stop team?"), text, tr("Stop team")))
                m_panel->act("stop", {});
        });
        stop->setProperty("danger", true); stop->setToolTip(tr("Cancel every open task")); iconFor(stop, "stop");
    }
    if (team.status == "done" || team.status == "stopped") {
        auto *archive = add(tr("Archive"), "teamWorkspaceArchive", false, [this] { m_panel->act("archive", {}); });
        archive->setToolTip(tr("Keep the history, hide the team")); iconFor(archive, "archive");
    }
    return bar;
}

QWidget *TeamWorkspace::finishSummary(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamWorkspaceFinishSummary");
    strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceFinishSummary{border-bottom:1px solid %1;}").arg(c.border.name()));
    auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 5, 12, 5); h->setSpacing(8);
    h->addWidget(label(tr("Finished by %1").arg(team.finish.by.isEmpty() ? tr("the owner") : team.finish.by), c.fg, 12, true, "teamFinishBy"));
    auto *text = elided(team.finish.summary, c.text2, 12);
    text->setObjectName("teamWorkspaceFinishText");
    h->addWidget(text, 1);
    return strip;
}

QWidget *TeamWorkspace::wideningStrip(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamWideningStrip"); strip->setMinimumHeight(34);
    strip->setStyleSheet(QStringLiteral("QFrame#teamWideningStrip{background:%1;border-bottom:1px solid %2;}")
                             .arg(alpha(c.warn, .12).name(QColor::HexArgb), alpha(c.warn, .5).name(QColor::HexArgb)));
    auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 6, 12, 6); h->setSpacing(8);
    QStringList lines;
    for (const auto &widening : team.widenings) {
        const QString who = widening.role.isEmpty() ? (widening.member.isEmpty() ? tr("A member") : widening.member) : widening.role;
        lines << tr("%1: %2 → %3").arg(who, widening.from.isEmpty() ? tr("(none)") : widening.from,
                                       widening.to.isEmpty() ? tr("(none)") : widening.to);
    }
    auto *text = label(tr("Permissions widened: %1. Approve only after confirming.").arg(lines.join(QStringLiteral(" · "))), c.warn, 12);
    text->setObjectName("teamWideningText");
    text->setWordWrap(true);
    h->addWidget(text, 1);
    return strip;
}

void TeamWorkspace::approveTeam(const Team &team)
{
    // B5: a widening is the owner's decision, every time; the approve request
    // carries confirm_widening only after this confirmation.
    if (!team.widenings.isEmpty()) {
        QStringList lines;
        for (const auto &widening : team.widenings) {
            const QString who = widening.role.isEmpty() ? widening.member : widening.role;
            lines << tr("%1: %2 → %3").arg(who, widening.from, widening.to);
        }
        if (!m_panel->confirm("teamWideningConfirm", tr("Widen permissions?"),
                              tr("This plan widens the permissions of its members beyond the template:\n\n%1\n\n"
                                 "Approve it only if they need it.").arg(lines.join(QStringLiteral("\n"))),
                              tr("Confirm and approve")))
            return;
        m_panel->act("approve", {{"confirm_widening", true}});
        return;
    }
    m_panel->act("approve", {});
}

// G9: a decision sent for this phase holds until the store's revision moves,
// whatever re-renders the header meanwhile; a failed send lets it go.
bool TeamWorkspace::phaseDecided(const Team &team, int phase) const
{
    return m_decidedRevision == team.revision && m_phasesDecided.contains(QStringLiteral("%1/%2").arg(team.id).arg(phase));
}

QString TeamWorkspace::markPhaseDecided(const Team &team, int phase)
{
    // Every phase decided at this revision stays decided; a new revision
    // means the store has taken them.
    if (m_decidedRevision != team.revision) { m_phasesDecided.clear(); m_decidedRevision = team.revision; }
    const QString key = QStringLiteral("%1/%2").arg(team.id).arg(phase);
    m_phasesDecided.insert(key);
    m_phaseAct = phase;
    m_error.clear();
    return key;
}

void TeamWorkspace::phaseSent(const QString &key, bool ok, const QString &message)
{
    m_phaseAct = 0;
    if (!ok) { m_phasesDecided.remove(key); m_error = message; }
    scheduleRender();
}

// G9: the band header's Approve, confirmed once, then sent through the same
// operation the draft strip used. Returns false when nothing was sent (a
// cancelled confirmation, a decision in flight or one already sent for this
// revision): a cancel leaves the header usable, a sent one never goes twice.
bool TeamWorkspace::approvePhase(const QString &id, int phase)
{
    const auto *team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    int count = 0;
    for (const auto &group : phaseGroups(*team)) if (group.n == phase) count = group.tasks.size();
    if (!m_panel->confirm("teamApprovePhaseConfirm", tr("Approve phase?"),
                          tr("Hand phase %1's %2 tasks to the members?").arg(phase).arg(count),
                          tr("Approve phase")))
        return false;
    // The team may have been refreshed while the dialog was open: read it again.
    team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    const QString key = markPhaseDecided(*team, phase);
    m_panel->act("approve-phase", {{"team", id}, {"phase", phase}}, [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
    return true;
}

bool TeamWorkspace::discardPhase(const QString &id, int phase)
{
    const auto *team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    int count = 0;
    for (const auto &group : phaseGroups(*team)) if (group.n == phase) count = group.tasks.size();
    if (!m_panel->confirm("teamDiscardPhaseConfirm", tr("Discard phase?"),
                          tr("Discard phase %1 and its %2 tasks?").arg(phase).arg(count), tr("Discard phase")))
        return false;
    team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    const QString key = markPhaseDecided(*team, phase);
    m_panel->act("discard-phase", {{"team", id}, {"phase", phase}}, [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
    return true;
}

void TeamWorkspace::movePhase(const Team &team, int phase, int to, const QString &position)
{
    if (to <= 0) return;
    m_error.clear();
    m_panel->act("move-phase", {{"team", team.id}, {"phase", phase}, {"to", to}, {"position", position}},
                 [this](bool ok, const QString &message) { if (!ok) m_error = message; scheduleRender(); });
}

QWidget *TeamWorkspace::inboxStrip(const Team &team, const QList<InboxItem> &items)
{
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamInboxStrip"); strip->setMinimumHeight(30);
    strip->setStyleSheet(QStringLiteral("QFrame#teamInboxStrip{background:%1;border-bottom:1px solid %2;}")
                             .arg(alpha(c.accent, .10).name(QColor::HexArgb), alpha(c.accent, .45).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(strip); v->setContentsMargins(12, 5, 12, 5); v->setSpacing(2);
    auto *head = row(6);
    head->addWidget(label(tr("Inbox"), c.fg, 12, true, "teamInboxTitle"));
    head->addWidget(label(items.size() == 1 ? tr("1 for the lead") : tr("%1 for the lead").arg(items.size()), c.muted, 11));
    head->addStretch();
    v->addLayout(head);
    // C2: oldest first, the command that answers it in the tooltip.
    int shown = 0;
    for (const auto &item : items) {
        if (shown == 5) break;
        const QString kind = item.kind == "report" ? tr("report") : item.kind == "question" ? tr("question")
                          : item.kind == "permission" ? tr("permission") : item.kind == "lead_task" ? tr("lead task")
                          : item.kind == "stalled" ? tr("stalled") : item.kind == "budget" ? tr("budget") : item.kind;
        auto *line = row(6);
        line->addWidget(new Pill(kind, item.kind == "report" ? c.warn : c.accent));
        line->addWidget(label(item.member, c.text2, 11));
        if (item.task > 0) line->addWidget(label(QStringLiteral("#%1").arg(item.task), c.muted, 11));
        auto *summary = elided(item.summary, c.fg, 11);
        summary->setObjectName("teamInboxSummary");
        summary->setToolTip(item.command);
        line->addWidget(summary, 1);
        v->addLayout(line);
        ++shown;
    }
    if (items.size() > shown) v->addWidget(label(tr("+%1 more").arg(items.size() - shown), c.muted, 10));
    Q_UNUSED(team);
    return strip;
}

QWidget *TeamWorkspace::attentionStrip(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamAttentionStrip"); strip->setMinimumHeight(26);
    strip->setStyleSheet(QStringLiteral("QFrame#teamAttentionStrip{background:%1;}")
                             .arg(alpha(c.warn, .12).name(QColor::HexArgb)));
    auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 4, 12, 4); h->setSpacing(8);
    h->addWidget(label(tr("Needs your attention"), c.warn, 11, true, "teamAttentionTitle"));
    QStringList lines;
    for (const auto &attention : team.attention) {
        QString line = attention.kind == "report" ? tr("report on #%1 from %2 · %3 min").arg(attention.task).arg(attention.member).arg(attention.minutes)
                                                  : tr("%1 · #%2 · %3 min").arg(attention.kind).arg(attention.task).arg(attention.minutes);
        lines << line;
    }
    auto *text = elided(lines.join(QStringLiteral("  ·  ")), c.text2, 11);
    text->setObjectName("teamAttentionText");
    text->setToolTip(lines.join(QStringLiteral("\n")));
    h->addWidget(text, 1);
    return strip;
}

QWidget *TeamWorkspace::errorStrip()
{
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamWorkspaceError");
    strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceError{background:%1;border-bottom:1px solid %2;}")
                             .arg(alpha(c.danger, .10).name(QColor::HexArgb), alpha(c.danger, .5).name(QColor::HexArgb)));
    auto *h = new QHBoxLayout(strip); h->setContentsMargins(12, 5, 12, 5); h->setSpacing(8);
    auto *text = label(m_error, c.danger, 12); text->setWordWrap(true); text->setObjectName("teamWorkspaceErrorText");
    h->addWidget(text, 1);
    auto *close = button(QStringLiteral("×"), "teamWorkspaceErrorClose");
    QObject::connect(close, &QPushButton::clicked, this, [this] { m_error.clear(); scheduleRender(); });
    h->addWidget(close);
    return strip;
}

const Teams::Team *TeamWorkspace::currentTeam(const QString &id) const
{
    if (const auto *watched = m_teams->watched(); watched && watched->id == id) return watched;
    return m_teams->team(id);
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
    // A3: a team question waits for the lead too; Answer opens its task card,
    // where the reply goes out.
    for (const auto &task : team.tasks) {
        for (const auto &asked : openQuestions(task)) {
            auto *r = row(8);
            if (!waiting++) r->addWidget(label(tr("Needs your answer"), c.fg, 12, true));
            else r->addSpacing(QFontMetrics(font()).horizontalAdvance(tr("Needs your answer")) + 4);
            r->addWidget(new Pill(team.roleOf(task.assignee), roleColor(team.roleOf(task.assignee), m_dark)));
            r->addWidget(elided(QStringLiteral("#%1 %2 — %3").arg(task.id).arg(task.title, asked.text), c.fg, 12), 1);
            auto *answer = button(tr("Answer"), "teamWorkspaceAnswerQuestion");
            QObject::connect(answer, &QPushButton::clicked, this, [this, id = task.id] { m_drawerTask = id; scheduleRender(); });
            r->addWidget(answer);
            v->addLayout(r);
        }
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
    view->bands = phaseBands(team, [this, &team](const PhaseGroup &group) { return m_panel->phaseOpen(team, group); });
    // G9: a decision already sent keeps its header controls off until the
    // store's revision moves, across re-renders.
    for (const auto &band : view->bands) if (phaseDecided(team, band.n) || m_phaseAct == band.n) view->decided.insert(band.n);
    const bool grouped = !view->bands.isEmpty();
    view->phaseToggled = [this, id = team.id](int phase) {
        if (const auto *current = currentTeam(id)) m_panel->togglePhase(*current, phase);
        scheduleRender();
    };
    // G9: the band header carries a draft's Approve and Discard (confirmed)
    // and the F10 move arrows.
    view->phaseApprove = [this, id = team.id](int phase) { return approvePhase(id, phase); };
    view->phaseDiscard = [this, id = team.id](int phase) { return discardPhase(id, phase); };
    view->phaseMoveUp = [this, id = team.id](int phase) {
        const auto *current = currentTeam(id);
        if (!current) return;
        const auto drafts = current->draftPhases();
        const int position = drafts.indexOf(phase);
        if (position > 0) movePhase(*current, phase, drafts.value(position - 1), QStringLiteral("before"));
    };
    view->phaseMoveDown = [this, id = team.id](int phase) {
        const auto *current = currentTeam(id);
        if (!current) return;
        const auto drafts = current->draftPhases();
        const int position = drafts.indexOf(phase);
        if (position >= 0 && position + 1 < drafts.size()) movePhase(*current, phase, drafts.value(position + 1), QStringLiteral("after"));
    };
    // Edges exist for every task: a collapsed phase's edges attach to its
    // summary node.
    for (const auto &task : team.tasks)
        for (const auto &d : task.deps) {
            view->edges[task.id].append(d.task);
            if (d.soft && !team.allHard) view->soft.insert({d.task, task.id});
        }
    QMap<int, int> localLayer;
    if (grouped) for (const auto &band : view->bands) {
        if (band.collapsed) continue;
        const auto layers = phaseLayers(team, band.tasks);
        for (auto it = layers.cbegin(); it != layers.cend(); ++it) localLayer.insert(it.key(), it.value());
    }
    QList<int> order;
    if (grouped) order = phaseOrder(team, view->bands);
    else {
        const auto byLayer = layered(team);
        for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    }
    for (int id : order) {
        const auto *task = team.task(id);
        const auto status = taskStatus(*task);
        TaskView::Node node;
        node.id = id; node.phase = task->phase; node.layer = grouped ? localLayer.value(id) : team.layers.value(id);
        node.title = task->title;
        node.role = task->assignee.isEmpty() ? tr("You") : team.roleOf(task->assignee);
        node.problem = problemTasks.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status); node.statusText = status.text; node.roleTone = roleColor(node.role, m_dark);
        if (task->subteam.known && !task->subteam.team.isEmpty()) {
            // G1: a delegated task is the group node for its sub-team.
            node.subteam = task->subteam.team;
            node.kind = QStringLiteral("subteam");
            node.statusText = tr("Sub-team \u00b7 %1/%2").arg(task->subteam.done).arg(task->subteam.total);
        }
        // A1: the hovered node's icon names the attempt phase; its tooltip
        // names the state, the time and the next step.
        const auto mark = phaseMark(team, *task);
        node.tooltip = mark.tooltip.isEmpty() ? status.text : mark.tooltip;
        node.draft = task->status == "draft"; node.error = team.cycle.contains(id);
        node.attempt = task->last() ? task->last()->n : 0;
        if (const auto *m = team.member(task->assignee)) node.agent = m->agent;
        QStringList deps;
        for (const auto &d : task->deps) deps << QStringLiteral("#%1").arg(d.task);
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
    } else view->clicked = [this](int id, const QPoint &) {
        const auto *team = m_teams->watched();
        if (team) if (const auto *task = team->task(id); task && task->subteam.known && !task->subteam.team.isEmpty()) {
            m_teams->watch(task->subteam.team);
            showTeam(task->subteam.team);
            return;
        }
        m_drawerTask = id; scheduleRender();
    };
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
    const auto groups = phaseGroups(team);
    // One status column over `ids`; `phase` only keeps each phase's scroll.
    const auto makeColumn = [&](const QString &key, const QString &name, const QList<int> &ids, int phase) -> QWidget * {
        auto *col = new QWidget; auto *cv = new QVBoxLayout(col); cv->setContentsMargins(0, 0, 0, 0); cv->setSpacing(6);
        int n = 0;
        auto *list = new QVBoxLayout; list->setSpacing(6);
        for (int id : ids) {
            const auto *found = team.task(id);
            if (!found) continue;
            const auto &task = *found;
            if (column(task, problemTasks.contains(task.id)) != key) continue;
            if (!m_focus.isEmpty() && task.assignee != m_focus) continue;
            ++n;
            const auto s = taskStatus(task);
            auto *cardBox = card(c, problemTasks.contains(task.id) ? c.danger : QColor());
            cardBox->setObjectName("teamBoardCard"); cardBox->setCursor(Qt::PointingHandCursor);
            cardBox->setProperty("task", task.id);
            auto *b = new QVBoxLayout(cardBox); b->setContentsMargins(8, 6, 8, 6); b->setSpacing(3);
            auto *title = label(QStringLiteral("#%1 %2").arg(task.id).arg(task.title), c.fg, 12, true); title->setWordWrap(true); b->addWidget(title);
            // Role, agent, state and dependencies share a line while the column
            // is wide enough and wrap when it is not.
            auto *r = new FlowLayout(6);
            const QString role = task.assignee.isEmpty() ? tr("You") : team.roleOf(task.assignee);
            auto *pill = new Pill(role, roleColor(role, m_dark)); pill->setObjectName("teamBoardRole"); r->addWidget(pill);
            if (const auto *m = team.member(task.assignee)) { auto *i = new QLabel; i->setPixmap(agentLogo(m->agent, c.text3).pixmap(14, 14)); r->addWidget(i); }
            const auto mark = phaseMark(team, task);
            auto *state = new StatusMark(s.text, toneColor(c, mark.tone != Tone::Neutral ? mark.tone : s.tone),
                                         mark.kind.isEmpty() ? kindOfTask(task, s) : mark.kind, 0, Qt::AlignLeft, nullptr, mark.tooltip);
            state->setObjectName("teamBoardState"); r->addWidget(state);
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
        if (groups.isEmpty()) scroll->setProperty("phase", 0);
        else { scroll->setProperty("phase", phase); scroll->setMinimumHeight(150); }
        keepScroll(scroll, groups.isEmpty() ? QStringLiteral("board/") + key + QLatin1Char('/') + m_focus
                                            : QStringLiteral("board/%1/%2/%3").arg(phase).arg(key).arg(m_focus));
        cv->addWidget(scroll, 1);
        return col;
    };
    const auto makeRow = [&](const QList<int> &ids, int phase) -> QWidget * {
        auto *rowBox = new QWidget; auto *h = new QHBoxLayout(rowBox);
        h->setContentsMargins(12, 4, 12, 8); h->setSpacing(8);
        for (const auto &[key, name] : QList<std::pair<QString, QString>>{{"pending", tr("Pending")}, {"active", tr("Active")}, {"problems", tr("Problems")},
                                                                         {"submitted", tr("Submitted")}, {"decided", tr("Decided")}, {"accepted", tr("Accepted")}})
            h->addWidget(makeColumn(key, name, ids, phase), 1);
        return rowBox;
    };
    if (groups.isEmpty()) {
        auto *w = makeRow([&] { QList<int> ids; for (const auto &task : team.tasks) ids.append(task.id); return ids; }(), 0);
        w->setObjectName("teamWorkspaceBoard");
        return w;
    }
    // Phases stack: a header each, its board columns under an open one and
    // nothing but the header under a collapsed one.
    auto *scroll = new QScrollArea; scroll->setObjectName("teamWorkspaceBoard");
    scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->viewport()->setAutoFillBackground(true);
    QPalette view = scroll->viewport()->palette(); view.setColor(QPalette::Window, c.surface); scroll->viewport()->setPalette(view);
    auto *content = new QWidget; content->setObjectName("teamBoardSections");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(12, 4, 12, 8); v->setSpacing(10);
    for (const auto &group : groups) {
        const bool open = m_panel->phaseOpen(team, group);
        auto *head = new QWidget; head->setObjectName("teamBoardPhase");
        head->setProperty("phase", group.n);
        auto *h = new QHBoxLayout(head); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
        auto *toggle = new QPushButton(open ? workspaceIcon("chevron-down", c.fg) : workspaceIcon("chevron-right", c.fg), phaseHeading(group));
        toggle->setObjectName("teamBoardPhaseToggle");
        toggle->setProperty("phase", group.n);
        toggle->setProperty("collapsed", !open);
        toggle->setFlat(true); toggle->setAutoDefault(false);
        toggle->setStyleSheet(QStringLiteral("QPushButton{text-align:left;font-size:12px;font-weight:600;color:%1;padding:2px 4px;}"
                                             "QPushButton:hover{color:%2;}").arg(c.fg.name(), c.accent.name()));
        QObject::connect(toggle, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] {
            if (const auto *current = currentTeam(id)) m_panel->togglePhase(*current, phase);
            scheduleRender();
        });
        h->addWidget(toggle, 1);
        if (group.barrier) h->addWidget(new Pill(tr("barrier"), c.muted));
        if (group.draft) h->addWidget(new Pill(tr("draft"), c.warn));
        // G9: the same header as the graph: a status chip, done/total and the
        // phase health line from phase_summary, and a draft's two decisions.
        if (const auto *summary = team.phaseSummaryOf(group.n)) {
            auto *chip = new QLabel(phaseStatusText(summary->status));
            chip->setObjectName("teamBoardPhaseStatus");
            const QColor tone = phaseStatusColor(c, summary->status);
            chip->setStyleSheet(QStringLiteral("QLabel{color:%1;border:1px solid %2;border-radius:7px;padding:0 6px;font-size:10px;}")
                                    .arg(tone.name(), alpha(tone, .5).name(QColor::HexArgb)));
            h->addWidget(chip);
            auto *progress = new QLabel(QStringLiteral("%1/%2").arg(summary->done).arg(summary->total));
            progress->setObjectName("teamBoardPhaseProgress");
            progress->setStyleSheet(QStringLiteral("QLabel{color:%1;font-size:10px;}").arg(c.muted.name()));
            h->addWidget(progress);
            if (!summary->healthLine.isEmpty()) {
                auto *health = new QLabel(summary->healthLine);
                health->setObjectName("teamBoardPhaseHealth");
                health->setStyleSheet(QStringLiteral("QLabel{color:%1;font-size:10px;}").arg(c.text3.name()));
                const QString tip = phaseHealthTip(team, group.n);
                health->setToolTip(tip.isEmpty() ? summary->healthLine : tip);
                h->addWidget(health);
            }
        }
        if (group.draft) {
            auto *approve = button(tr("Approve"), "teamBoardApprovePhase", true);
            approve->setEnabled(approveBlocked(team, group.n).isEmpty() && (team.phase(group.n) ? team.phase(group.n)->waiting.text.isEmpty() : true));
            approve->setToolTip(approveBlocked(team, group.n));
            QObject::connect(approve, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] {
                approvePhase(id, phase);
            });
            auto *discard = button(tr("Discard"), "teamBoardDiscardPhase");
            QObject::connect(discard, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] {
                discardPhase(id, phase);
            });
            h->addWidget(approve); h->addWidget(discard);
        }
        v->addWidget(head);
        if (!open) continue;
        v->addWidget(makeRow(group.tasks, group.n));
    }
    v->addStretch();
    scroll->setWidget(content);
    keepScroll(scroll, QStringLiteral("board/phases/") + m_focus);
    return scroll;
}

QWidget *TeamWorkspace::members(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *w = new QWidget; w->setObjectName("teamWorkspaceMemberList");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(6);
    const bool finished = team.status == "done" || team.status == "stopped";
    // G9: a draft's Approve, Discard and move arrows live in its phase band
    // header in the graph (and the board/list headers), not above this column.
    int active = 0;
    for (const auto &m : team.members) if (!(finished && leftMember(team, m.id))) ++active;
    auto *title = row(6);
    auto *membersToggle = new QToolButton; membersToggle->setIcon(workspaceIcon(m_membersOpen ? "chevron-down" : "chevron-right", c.fg));
    membersToggle->setAutoRaise(true); membersToggle->setObjectName("teamMembersToggle");
    membersToggle->setToolTip(m_membersOpen ? tr("Fold the members") : tr("Show the members"));
    QObject::connect(membersToggle, &QToolButton::clicked, this, [this] {
        m_membersOpen = !m_membersOpen; QSettings().setValue("teams/membersOpen", m_membersOpen); scheduleRender();
    });
    title->addWidget(membersToggle);
    title->addWidget(label(tr("Members"), c.fg, 12, true)); title->addWidget(label(QString::number(active), c.muted, 12)); title->addStretch();
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *add = button(tr("+ Member"), "teamWorkspaceAddMember");
        add->setToolTip(tr("Add a member; it starts with the first task it gets"));
        QObject::connect(add, &QPushButton::clicked, this, [this, id = team.id] { if (const auto *t = m_teams->team(id)) addMember(*t); });
        title->addWidget(add);
    }
    v->addLayout(title);
    if (m_membersOpen) {
    for (const auto &m : team.members) {
        // F7: a session left running after the team ended is shown apart below.
        if (finished && leftMember(team, m.id)) continue;
        const auto state = live(m);
        const auto status = memberStatus(team, m, state);
        auto *b = new MemberCard; b->setObjectName("teamWorkspaceMember"); b->setCursor(Qt::PointingHandCursor); b->setProperty("teamMember", m.id);
        const QColor edge = m.id == m_focus ? c.accent : status.problem ? toneColor(c, status.tone) : c.border;
        b->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceMember{background:%1;border:1px solid %2;border-radius:9px;}").arg(c.card.name(), edge.name()));
        b->setToolTip(m.id == m_focus ? tr("Show every member") : tr("Show only %1's tasks").arg(team.roleOf(m.id)));
        b->clicked = [this, id = m.id] { m_focus = m_focus == id ? QString() : id; scheduleRender(); };
        auto *bh = new QHBoxLayout(b); bh->setContentsMargins(8, 6, 6, 6); bh->setSpacing(8);
        auto *tile = new QLabel; tile->setPixmap(agentLogo(m.agent, c.fg).pixmap(16, 16)); bh->addWidget(tile, 0, Qt::AlignTop);
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
        } else {
            // A1: the attempt's phase icon and tooltip beside the member.
            const Task *current = status.task ? team.task(status.task) : nullptr;
            const auto mark = current ? phaseMark(team, *current) : PhaseMark{};
            const QString shown = team.status == "planning" && m.session.isEmpty() && !m.lead ? tr("On approval") : status.text;
            auto *state = new StatusMark(shown, toneColor(c, mark.tone != Tone::Neutral ? mark.tone : status.tone),
                                         mark.kind.isEmpty() ? status.kind : mark.kind, 0, Qt::AlignLeft, nullptr, mark.tooltip);
            state->setObjectName("teamWorkspaceMemberState");
            state->setProperty("phase", mark.phase);
            col->addWidget(state);
        }
        // A5: the member's queue, the current task first, then the tasks
        // returned to it.
        if (auto *queue = memberQueueWidget(team, m, c, [this](int id) { m_drawerTask = id; scheduleRender(); })) col->addWidget(queue);
        // G7: the MCP servers and services its session runs, as an icon with a
        // tooltip - they outlive the session and are not Background work.
        // C12: the declared time budget of its background job.
        if (!m.services.isEmpty() || m.budget.known || m.limited.known || m.hostShared) {
            auto *extra = row(6);
            // H1: stopped at a usage limit, with the reset time when known.
            if (m.limited.known) {
                const QString reset = resetText(m.limited.resetsAt);
                QString text = reset.isEmpty() ? tr("Usage limit · reset unknown") : tr("Usage limit · %1").arg(reset);
                if (m.limitResume && !reset.isEmpty()) text = tr("Usage limit · continues after the reset (%1)").arg(reset);
                QStringList lines;
                if (!m.limited.detail.isEmpty()) lines << m.limited.detail;
                if (reset.isEmpty()) lines << tr("The session named no reset time; the lead's inbox has it.");
                else lines << (team.resumeAfterLimits ? tr("Resume after limits is on: Zerus continues it after the reset.")
                                                      : tr("Resume after limits is off: continue it yourself, or turn it on in the team bar."));
                auto *limited = new StatusMark(text, c.warn, "paused", 0, Qt::AlignLeft, nullptr, lines.join(QLatin1Char('\n')));
                limited->setObjectName("teamWorkspaceMemberLimited");
                limited->setProperty("resetsAt", m.limited.resetsAt);
                extra->addWidget(limited);
            }
            // H3: its DeepSeek host serves other members of the team too.
            if (m.hostShared) {
                QString tip = tr("Its DeepSeek host serves other members too.");
                if (const auto *host = team.hostOf(m.id)) {
                    QStringList roles; for (const auto &id : host->members) roles << team.roleOf(id);
                    tip = tr("DeepSeek host %1 serves %2.").arg(host->pid).arg(roles.join(QStringLiteral(", ")));
                    if (host->started > 0) tip += QLatin1Char('\n') + tr("Started %1").arg(QDateTime::fromSecsSinceEpoch(qint64(host->started)).toString(QStringLiteral("d MMM HH:mm")));
                }
                auto *shared = new StatusMark(tr("Shared host"), c.muted, "services", 0, Qt::AlignLeft, nullptr, tip);
                shared->setObjectName("teamWorkspaceMemberSharedHost");
                extra->addWidget(shared);
            }
            if (!m.services.isEmpty()) {
                QStringList lines;
                for (const auto &service : m.services) {
                    const QString since = service.since > 0 ? QDateTime::fromSecsSinceEpoch(qint64(service.since)).toString(QStringLiteral("d MMM HH:mm")) : tr("unknown");
                    lines << tr("%1 · since %2").arg(service.name, since) + (service.command.isEmpty() ? QString() : QStringLiteral("\n") + service.command);
                }
                lines << tr("These outlive the session and are not Background work.");
                auto *services = new StatusMark(m.services.size() == 1 ? tr("1 service") : tr("%1 services").arg(m.services.size()),
                                                c.muted, "services", 0, Qt::AlignLeft, nullptr, lines.join(QLatin1Char('\n')));
                services->setObjectName("teamWorkspaceMemberServices");
                services->setProperty("services", m.services.size());
                extra->addWidget(services);
            }
            if (m.budget.known) {
                const QColor tone = m.budget.state == "red" ? c.danger : m.budget.state == "amber" ? c.warn : c.muted;
                QStringList lines;
                if (!m.budget.label.isEmpty()) lines << m.budget.label;
                lines << tr("%1 of %2 min · red at %3 min").arg(m.budget.elapsedMinutes).arg(m.budget.minutes).arg(m.budget.redMinutes);
                if (!m.budget.command.isEmpty()) lines << m.budget.command;
                auto *budget = new StatusMark(tr("Budget %1 · %2 min").arg(m.budget.state).arg(m.budget.elapsedMinutes),
                                              tone, "budget", 0, Qt::AlignLeft, nullptr, lines.join(QLatin1Char('\n')));
                budget->setObjectName("teamWorkspaceMemberBudget");
                budget->setProperty("budgetState", m.budget.state);
                extra->addWidget(budget);
            }
            extra->addStretch();
            col->addLayout(extra);
        }
        // C4: the member's pending permission request (members[].question): the
        // exact command, its source and the agent's reason. Allow/Deny sends
        // hgs team answer with the member, the question id, its hash and the
        // native answers array; a deny's reason travels as the answer text.
        if (m.permission.permission()) {
            auto *box = new QFrame; box->setObjectName("teamMemberPermission"); box->setProperty("member", m.id);
            box->setStyleSheet(QStringLiteral("QFrame#teamMemberPermission{background:%1;border:1px solid %2;border-radius:7px;}")
                                   .arg(alpha(c.warn, .10).name(QColor::HexArgb), alpha(c.warn, .40).name(QColor::HexArgb)));
            auto *pv = new QVBoxLayout(box); pv->setContentsMargins(8, 5, 8, 6); pv->setSpacing(3);
            auto *head = row(6);
            if (!m.permission.tool.isEmpty()) head->addWidget(new Pill(m.permission.tool, c.warn));
            head->addWidget(label(m.permission.commandSource == "process" ? tr("command from the running process")
                              : m.permission.commandSource == "tool_input" ? tr("command from the tool input")
                              : tr("the agent did not show the command"), c.text3, 10), 1);
            pv->addLayout(head);
            if (!m.permission.command.isEmpty()) {
                auto *command = label(m.permission.command, c.fg, 11, false, "teamMemberPermissionCommand");
                command->setFont(NewSessionDense::mono(11));
                command->setWordWrap(true);
                command->setTextInteractionFlags(Qt::TextSelectableByMouse);
                command->setToolTip(m.permission.command);
                pv->addWidget(command);
            }
            if (!m.permission.reason.isEmpty()) {
                auto *reason = label(tr("Agent: %1").arg(m.permission.reason), c.text2, 11, false, "teamMemberPermissionReason");
                reason->setWordWrap(true);
                pv->addWidget(reason);
            }
            auto *answerRow = row(6);
            auto *reasonEdit = new QLineEdit; reasonEdit->setObjectName("teamMemberPermissionDenyReason");
            reasonEdit->setPlaceholderText(tr("Why deny? The member receives this"));
            answerRow->addWidget(reasonEdit, 1);
            auto *deny = button(tr("Deny"), "teamMemberPermissionDeny"); deny->setProperty("danger", true);
            auto *allow = button(tr("Allow"), "teamMemberPermissionAllow", true);
            // C4: the answer must carry the card's own question id and option
            // ids (question.answer); without them the card cannot answer.
            const bool hasHash = !m.permission.id.isEmpty() && !m.permission.hash.isEmpty();
            const bool answerable = hasHash && m.permission.native.known;
            allow->setEnabled(answerable); deny->setEnabled(answerable);
            if (!answerable) {
                const auto tip = hasHash
                    ? tr("This hgs sends no answer ids for the question; answer it in the session.")
                    : tr("This hgs sends no question hash in status; answer it in the session.");
                allow->setToolTip(tip); deny->setToolTip(tip); reasonEdit->setToolTip(tip);
            }
            const auto decide = [this, member = m.id, question = m.permission.id, hash = m.permission.hash,
                                 answer = m.permission.native](bool allowIt, const QString &text) {
                m_panel->act("answer", {{"member", member}, {"question_id", question}, {"expected_question_hash", hash},
                                        {"answers", QJsonArray{QJsonObject{{"question_id", answer.questionId},
                                                                           {"selected_option_ids", QJsonArray{allowIt ? answer.allow : answer.deny}},
                                                                           {"text", text}}}}});
            };
            QObject::connect(deny, &QPushButton::clicked, this, [reasonEdit, decide] {
                decide(false, reasonEdit->text().trimmed());
            });
            QObject::connect(allow, &QPushButton::clicked, this, [decide] { decide(true, QString()); });
            answerRow->addWidget(deny); answerRow->addWidget(allow);
            pv->addLayout(answerRow);
            col->addWidget(box);
        }
        // The answers so far: who answered, and the deny reason.
        for (const auto &answer : m.permissionAnswers) {
            const QString by = answer.by == "owner" ? tr("you") : answer.by == "lead" ? tr("the lead")
                             : answer.by == "rule" ? tr("a rule") : answer.by == "timeout" ? tr("a timeout") : tr("unknown");
            QString line = answer.denied() ? tr("Denied by %1").arg(by) : tr("Allowed by %1").arg(by);
            if (answer.denied() && !answer.reason.isEmpty()) line += QStringLiteral(": ") + answer.reason;
            auto *mark = label(line, answer.denied() ? c.warn : c.accent, 10, false, "teamMemberPermissionAnswered");
            mark->setWordWrap(true);
            mark->setProperty("decision", answer.decision);
            col->addWidget(mark);
        }
        bh->addLayout(col, 1);
        auto *right = new QVBoxLayout; right->setSpacing(3);
        auto *ratio = label(QStringLiteral("%1/%2").arg(done).arg(total), c.muted, 11); ratio->setFont(NewSessionDense::mono(11)); ratio->setAlignment(Qt::AlignRight);
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
    // F7: the sessions a Done/Stopped team left running, apart from the members.
    if (finished && !team.leftRunning.isEmpty()) v->addWidget(m_panel->leftoverMembers(team, false));
    // H6: the sessions a swap left running.
    if (!team.swapped.isEmpty()) v->addWidget(swappedList(team));
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
    }
    // H6: swap the session, agent, model or account of any member, the lead too.
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *swap = menu.addAction(tr("Swap session, agent or model…"), this, [this, teamId = team.id, id = member.id] {
            const auto *current = currentTeam(teamId);
            if (const auto *found = current ? current->member(id) : nullptr) swapDialog(*current, *found);
        });
        swap->setObjectName("teamWorkspaceMemberSwap");
        swap->setToolTip(tr("Keeps its role, tasks, folder and branch; an open task goes to the new session with what was done so far"));
    }
    menu.setToolTipsVisible(true);
    menu.exec(global);
}

void TeamWorkspace::openOtherTeam(const QString &team)
{
    if (team.isEmpty()) return;
    m_teams->refresh();
    m_teams->watch(team);
    showTeam(team);
}

QWidget *TeamWorkspace::proposalsStrip(const Team &team)
{
    QList<Team::Proposal> open;
    QSet<QString> openIds;
    for (const auto &proposal : team.proposals) if (proposal.open()) { open << proposal; openIds << proposal.id; }
    m_decidedProposals.intersect(openIds);   // decided ones the store has taken
    if (open.isEmpty()) return nullptr;
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamWorkspaceProposals");
    strip->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceProposals{background:%1;border-bottom:1px solid %2;}")
                             .arg(alpha(c.accent, .07).name(QColor::HexArgb), alpha(c.accent, .35).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(strip); v->setContentsMargins(12, 6, 12, 6); v->setSpacing(4);
    for (const auto &proposal : open) {
        auto *line = new QWidget; line->setObjectName("teamProposal"); line->setProperty("proposal", proposal.id);
        line->setProperty("kind", proposal.kind);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
        const QString what = proposalText(team, proposal);
        h->addWidget(label(tr("%1 proposes").arg(proposal.by.isEmpty() ? tr("The lead") : proposal.by), c.muted, 11));
        h->addWidget(label(what, c.fg, 12, true, "teamProposalText"));
        auto *reason = elided(proposal.reason, c.text2, 11); reason->setToolTip(proposal.reason); h->addWidget(reason, 1);
        const bool sent = m_decidedProposals.contains(proposal.id);
        auto *reject = button(tr("Reject"), "teamProposalReject");
        auto *accept = button(tr("Accept"), "teamProposalAccept", true);
        reject->setEnabled(!sent); accept->setEnabled(!sent);
        // Remove and swap change who works: they ask first.
        const bool ask = proposal.kind == "remove-member" || proposal.kind == "swap-member";
        const auto decide = [this, id = proposal.id](bool yes) {
            if (m_decidedProposals.contains(id)) return;
            m_decidedProposals.insert(id);
            m_panel->act("decide-proposal", {{"proposal", id}, {"accept", yes}},
                         [this, id](bool ok, const QString &message) {
                             if (!ok) { m_decidedProposals.remove(id); m_error = message; }
                             scheduleRender();
                         });
            scheduleRender();
        };
        QObject::connect(accept, &QPushButton::clicked, this, [this, decide, ask, what, kind = proposal.kind] {
            if (ask && !m_panel->confirm("teamProposalConfirm", kind == "swap-member" ? tr("Swap member?") : tr("Remove member?"),
                                         kind == "swap-member"
                                             ? tr("%1? Its open task goes to the new session with what was done so far; the old session keeps running for you to stop.").arg(what)
                                             : tr("%1? Its session keeps running; it gets no more tasks.").arg(what),
                                         kind == "swap-member" ? tr("Swap") : tr("Remove")))
                return;
            decide(true);
        });
        QObject::connect(reject, &QPushButton::clicked, this, [decide] { decide(false); });
        h->addWidget(reject); h->addWidget(accept);
        v->addWidget(line);
    }
    return strip;
}

void TeamWorkspace::nextGoalDialog(const Team &team)
{
    if (auto *open = findChild<QDialog *>("teamNextGoalDialog")) { open->raise(); open->activateWindow(); return; }
    const Colors c = colorsFor(m_dark);
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamNextGoalDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(tr("Next goal \u00b7 %1").arg(team.name));
    dialog->resize(560, 380);
    auto *v = new QVBoxLayout(dialog);
    int running = 0;
    for (const auto &member : team.members) running += leftMember(team, member.id) || (member.lead && !member.session.isEmpty());
    auto *intro = label(tr("A new team takes the next goal with this folder, its settings, the lead and the members. "
                           "%1 of %2 sessions are still running and keep their context; the others start with their first task. "
                           "This team stays Done.").arg(running).arg(team.members.size()), c.text2, 12);
    intro->setWordWrap(true);
    v->addWidget(intro);
    v->addWidget(label(tr("Goal"), c.muted, 11));
    auto *goal = new QPlainTextEdit; goal->setObjectName("teamNextGoalText");
    goal->setPlaceholderText(tr("What the team delivers next, and what done means"));
    v->addWidget(goal, 1);
    v->addWidget(label(tr("Name"), c.muted, 11));
    auto *name = new QLineEdit(team.name); name->setObjectName("teamNextGoalName"); name->setMaxLength(80);
    v->addWidget(name);
    auto *error = label(QString(), c.danger, 11, false, "teamNextGoalError"); error->setWordWrap(true); error->hide();
    v->addWidget(error);
    auto *buttons = new QDialogButtonBox;
    auto *start = buttons->addButton(tr("Start next goal"), QDialogButtonBox::AcceptRole); start->setObjectName("teamNextGoalStart");
    buttons->addButton(QDialogButtonBox::Cancel);
    v->addWidget(buttons);
    const auto validate = [goal, start] {
        const QString text = goal->toPlainText().trimmed();
        start->setEnabled(!text.isEmpty() && text.toUcs4().size() <= 4000);
    };
    validate();
    QObject::connect(goal, &QPlainTextEdit::textChanged, dialog, validate);
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    QObject::connect(start, &QPushButton::clicked, dialog, [this, dialog, goal, name, error, start, id = team.id] {
        start->setEnabled(false);
        QJsonObject input{{"team", id}, {"goal", goal->toPlainText().trimmed()}};
        if (!name->text().trimmed().isEmpty()) input.insert("name", name->text().trimmed());
        m_teams->call(QStringLiteral("next-goal"), input,
            [this, guard = QPointer<QDialog>(dialog), error, start](bool ok, const QJsonObject &result, const QString &message) {
                if (!ok) {
                    if (guard) { error->setText(message); error->show(); start->setEnabled(true); }
                    return;
                }
                if (guard) guard->accept();
                openOtherTeam(result.value("team").toString());
            });
    });
    dialog->show();
}

void TeamWorkspace::swapDialog(const Team &team, const Member &member)
{
    if (auto *open = findChild<QDialog *>("teamSwapDialog")) open->close();
    const Colors c = colorsFor(m_dark);
    const QString role = team.roleOf(member.id);
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamSwapDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setWindowTitle(tr("Swap %1").arg(role));
    dialog->resize(520, 300);
    auto *v = new QVBoxLayout(dialog);
    auto *intro = label(member.lead
        ? tr("The lead keeps its role and the team's plan; its open items stay in the inbox and the new session gets one brief.")
        : tr("%1 keeps its id, role, tasks, folder and branch. An open task goes to the new session with what was done so far.").arg(role),
        c.text2, 12);
    intro->setWordWrap(true);
    v->addWidget(intro);
    auto *fresh = new QRadioButton(tr("A new session")); fresh->setObjectName("teamSwapNew"); fresh->setChecked(true);
    v->addWidget(fresh);
    auto *grid = new QGridLayout; grid->setContentsMargins(22, 0, 0, 0); grid->setHorizontalSpacing(6); grid->setVerticalSpacing(6);
    auto *agent = new QComboBox; agent->setObjectName("teamSwapAgent");
    for (const auto &a : agentNames.keys()) agent->addItem(workspaceIcon(a, c.fg), agentNames.value(a), a);
    agent->setCurrentIndex(qMax(0, agent->findData(member.agent)));
    auto *model = new QComboBox; model->setObjectName("teamSwapModel"); model->setEditable(true); model->setCurrentText(member.model);
    model->lineEdit()->setPlaceholderText(tr("default model"));
    auto *effort = new QComboBox; effort->setObjectName("teamSwapEffort"); effort->addItem(QStringLiteral("\u2014"), QString());
    for (const auto *e : {"low", "medium", "high", "xhigh"}) effort->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
    effort->setCurrentIndex(qMax(0, effort->findData(member.effort)));
    auto *account = new QComboBox; account->setObjectName("teamSwapAccount"); account->setEditable(true);
    account->addItem(tr("Default account"), QString());
    if (!member.account.isEmpty()) { account->addItem(member.account, member.account); account->setCurrentIndex(1); }
    grid->addWidget(label(tr("Agent"), c.muted, 11), 0, 0); grid->addWidget(agent, 0, 1);
    grid->addWidget(label(tr("Model"), c.muted, 11), 0, 2); grid->addWidget(model, 0, 3);
    grid->addWidget(label(tr("Effort"), c.muted, 11), 1, 0); grid->addWidget(effort, 1, 1);
    grid->addWidget(label(tr("Account"), c.muted, 11), 1, 2); grid->addWidget(account, 1, 3);
    grid->setColumnStretch(3, 1);
    v->addLayout(grid);
    auto *existing = new QRadioButton(tr("An existing session")); existing->setObjectName("teamSwapExisting");
    v->addWidget(existing);
    auto *session = new QComboBox; session->setObjectName("teamSwapSession");
    QList<SessionChoice> choices = m_panel->sessionChoices ? m_panel->sessionChoices() : QList<SessionChoice>();
    for (const auto &choice : choices) {
        if (team.memberOfSession(choice.host, choice.name)) continue;   // not one of this team's
        session->addItem(choice.host.isEmpty() ? choice.name : choice.host + QStringLiteral(": ") + choice.name,
                         QJsonObject{{"host", choice.host}, {"name", choice.name}, {"run_id", choice.runId}, {"conversation_id", choice.conversationId}});
    }
    existing->setEnabled(session->count() > 0);
    if (!session->count()) existing->setToolTip(tr("No other live session to adopt"));
    auto *sessionRow = new QHBoxLayout; sessionRow->setContentsMargins(22, 0, 0, 0); sessionRow->addWidget(session, 1);
    v->addLayout(sessionRow);
    v->addStretch();
    auto *buttons = new QDialogButtonBox;
    auto *apply = buttons->addButton(tr("Swap\u2026"), QDialogButtonBox::AcceptRole); apply->setObjectName("teamSwapApply");
    buttons->addButton(QDialogButtonBox::Cancel);
    v->addWidget(buttons);
    const auto sync = [fresh, agent, model, effort, account, session] {
        for (auto *w : std::initializer_list<QWidget *>{agent, model, effort, account}) w->setEnabled(fresh->isChecked());
        session->setEnabled(!fresh->isChecked());
    };
    sync();
    QObject::connect(fresh, &QRadioButton::toggled, dialog, sync);
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    QObject::connect(apply, &QPushButton::clicked, dialog, [=, guard = QPointer<QDialog>(dialog), id = member.id, lead = member.lead] {
        QJsonObject input{{"member", id}};
        QString to;
        if (fresh->isChecked()) {
            input.insert("agent", agent->currentData().toString());
            QStringList parts{agent->currentText()};
            if (!model->currentText().trimmed().isEmpty()) { input.insert("model", model->currentText().trimmed()); parts << model->currentText().trimmed(); }
            if (!effort->currentData().toString().isEmpty()) input.insert("effort", effort->currentData().toString());
            const QString chosen = account->currentData().toString().isEmpty() && account->currentIndex() != 0 ? account->currentText().trimmed()
                                                                                                               : account->currentData().toString();
            if (!chosen.isEmpty()) input.insert("account", chosen);
            to = tr("a new %1 session").arg(parts.join(QStringLiteral(" \u00b7 ")));
        } else {
            const auto picked = session->currentData().toJsonObject();
            input.insert("session", picked);
            to = tr("the session %1").arg(picked.value("name").toString());
        }
        const QString text = lead
            ? tr("Swap the lead to %1? Its open items stay in the inbox; the old session keeps running for you to stop.").arg(to)
            : tr("Swap %1 to %2? Its open task goes to the new session with what was done so far; the old session keeps running for you to stop.").arg(role, to);
        if (!m_panel->confirm("teamSwapConfirm", tr("Swap member?"), text, tr("Swap"))) return;
        m_panel->act("swap-member", input);
        if (guard) guard->accept();
    });
    dialog->show();
}

QWidget *TeamWorkspace::swappedList(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamWorkspaceSwapped");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 8, 0, 0); v->setSpacing(4);
    auto *head = label(tr("Old sessions after a swap \u2014 still running; stop them when you are done"), c.muted, 11, true);
    head->setWordWrap(true);
    v->addWidget(head);
    for (int i = team.swapped.size() - 1; i >= 0; --i) {
        const auto &entry = team.swapped[i];
        auto *line = new QWidget; line->setObjectName("teamSwappedSession"); line->setProperty("session", entry.old);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
        h->addWidget(new Pill(entry.member, roleColor(entry.member, m_dark)));
        auto *name = elided(entry.old.isEmpty() ? tr("no session") : entry.old, c.text2, 11); name->setToolTip(entry.old);
        h->addWidget(name, 1);
        if (entry.at > 0) h->addWidget(label(QDateTime::fromSecsSinceEpoch(qint64(entry.at)).toString(QStringLiteral("HH:mm")), c.muted, 11));
        auto *open = button(tr("Open"), "teamSwappedOpen"); open->setProperty("ghost", true); open->setEnabled(!entry.old.isEmpty());
        QObject::connect(open, &QPushButton::clicked, this, [this, session = entry.old] { if (openSession) openSession({}, session); });
        h->addWidget(open);
        v->addWidget(line);
    }
    return box;
}

void TeamWorkspace::assignMenu(const Team &team, const QString &member, const QPoint &global)
{
    QMenu menu(this);
    menu.setObjectName("teamWorkspaceAssignMenu");
    for (const auto &t : team.tasks) {
        if (t.finished() || t.assignee == member) continue;
        const bool ready = team.ready.contains(t.id);
        auto *action = menu.addAction(QStringLiteral("#%1 %2%3").arg(t.id).arg(t.title, ready ? tr("  · ready") : QString()));
        QObject::connect(action, &QAction::triggered, this, [this, team, task = t.id, member] {
            const auto choice = subteamChoice(this, team, task, tr("Reassign"));
            if (choice.isEmpty()) return;
            QJsonObject input{{"task", task}, {"member", member}};
            const auto *delegated = team.task(task);
            if (delegated && delegated->subteam.known) input["subteam"] = choice;
            m_panel->act("reassign", input);
        });
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
    if (lead) { auto *tile = new QLabel; tile->setPixmap(agentLogo(lead->agent, c.fg).pixmap(20, 20)); h->addWidget(tile, 0, Qt::AlignTop); }
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
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(NewSessionDense::mono(11)); time->setFixedWidth(38); r->addWidget(time);
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
