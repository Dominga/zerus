#include "Settings.h"
#include "SlowEventLog.h"
#include "TeamWorkspace.h"
#include "SubagentRequest.h"
#include "TeamAttentionNotices.h"

#include <QSpinBox>
#include <QPointer>
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamProposalCard.h"
#include "TeamProfile.h"
#include "TeamTests.h"
#include "TeamPanel.h"
#include "TeamSettingsDialog.h"
#include "TeamTaskCard.h"
#include "TeamTreeIndent.h"
#include "TeamWidgets.h"
#include "TeamPhaseScroll.h"
#include "TeamStages.h"
#include "TeamHeat.h"
#include "CollapsibleBlock.h"
#include "WorkspaceStyle.h"
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGridLayout>
#include <QInputDialog>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QRadioButton>
#include <QResizeEvent>
#include <QScrollArea>
#include <QScrollBar>
#include <QScopedValueRollback>
#include <QSettings>
#include <QShowEvent>
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

// #708: a Board or Layers view with many cards is built over event-loop turns.
// One turn builds for at most kSliceMs; a view of at most kSyncSlices cards is
// drained inside the render, so a small team keeps its old synchronous render.
constexpr int kSliceMs = 6;
constexpr std::size_t kSyncSlices = 24;
// A team small enough that its whole board is built in the render itself.
constexpr std::size_t kSmallTeam = 20;

// The board's columns, by what a task needs. Accepted, rejected and cancelled
// are all decided, but only an accepted task is done: the others get their own
// column so a rejected task never sits under Accepted (QA).
QString column(const Task &task, bool problem)
{
    if (problem) return QStringLiteral("problems");
    if (task.status == "accepted") return QStringLiteral("accepted");
    if (task.status == "rejected" || task.status == "cancelled") return QStringLiteral("decided");
    if (task.status == "submitted") return QStringLiteral("submitted");
    // U36: the pipeline's stages between Submitted and Accepted.
    if (task.status == "in_review" || task.status == "in_test") return task.status;
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
    m_sliceTimer = new QTimer(this);
    m_sliceTimer->setSingleShot(true);
    m_sliceTimer->setInterval(0);
    QObject::connect(m_sliceTimer, &QTimer::timeout, this, &TeamWorkspace::runSlices);
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
    m_conversation->setAgentName(tr("the lead"));
    m_conversation->openTerminal = [this](const QString &host, const QString &session) { if (openTerminal) openTerminal(host, session); };
    // U57: the lead's open proposals as cards in the lead's conversation.
    m_proposalCards = new TeamProposalCards;
    m_proposalCards->decide = [this](const QJsonObject &input, const TeamProposalCards::Done &done) {
        const QString id = input.value("proposal").toString();
        m_decidedProposals.insert(id);
        m_panel->act("decide-proposal", input, [this, id, done](bool ok, const QString &message) {
            if (!ok) m_decidedProposals.remove(id);
            done(ok, message);
            scheduleRender();
        });
    };
    m_proposalCards->current = [this]() -> const Team * { return currentTeam(m_team); };
    m_proposalCards->confirm = [this](const QString &title, const QString &text, const QString &action) {
        return m_panel->confirm("teamProposalConfirm", title, text, action);
    };
    m_proposalCards->details = [this](const QString &proposal) { if (const auto *team = currentTeam(m_team)) switchModeDialog(*team, proposal); };
    m_conversation->setCards(m_proposalCards);
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
    QObject::connect(m_split, &QSplitter::splitterMoved, this, [this] { Settings::set("teams/workspaceSplit", m_split->saveState()); });
    QObject::connect(m_sideSplit, &QSplitter::splitterMoved, this, [this] { Settings::set("teams/workspaceSideSplit", m_sideSplit->saveState()); });
    m_membersOpen = Settings::value("teams/membersOpen", true).toBool();
    m_hideFinished = Settings::value("teams/hideFinished", false).toBool();
    // U38 (#665): the Profile view's reads, kept across renders and teams.
    m_profileCache = new TeamProfile::Cache(teams, this);
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
    m_drawer->reassign = [this](int task, const QPoint &global) { if (const auto *team = currentTeam(m_team)) m_panel->reassignMenu(*team, task, global); };
    // U62 (#719): the drawer's test runs, from the team's test database.
    m_drawer->readTestRuns = [this](int task, const TeamTaskCard::RunsRead &done) {
        m_teams->call(QStringLiteral("test"), {{"team", m_team}, {"action", "results"}, {"task", task}, {"limit", 20}},
                      [done](bool ok, const QJsonObject &result, const QString &error) { done(ok ? result.value("items").toArray() : QJsonArray(), ok ? QString() : error); },
                      false);
    };
    m_drawer->openTestRecord = [this](const QString &record) { showTestRecord(record); };
    m_drawer->problemActions = [this](const Member &member, const Status &status) -> QWidget * {
        const auto *team = currentTeam(m_team);
        return team ? m_panel->problemActions(*team, member, status) : nullptr;
    };
    // #693: polls rebuild only what they changed; the minute timer ticks the
    // ages in place.
    QObject::connect(m_teams, &TeamController::teamsChanged, this, [this] { pollRender(); });
    QObject::connect(m_teams, &TeamController::showCancelledChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_teams, &TeamController::watchedChanged, this, [this] {
        if (const auto *team = m_teams->watched()) Teams::AccountMove::observe(m_move, *team);
        renderAccountMove();
        pollRender();
    });
    QObject::connect(&workedTicker(), &QTimer::timeout, this, [this] { tickInPlace(); });
    QObject::connect(m_teams->client(), &HgsClient::accountsReady, this, [this](quint64 request, const QString &host, const QJsonObject &result) {
        if (request != m_accountsRequest || !host.isEmpty()) return;
        m_accountsRequest = 0; m_accountsRead = true;
        QList<QJsonObject> profiles;
        for (const auto &value : result.value("profiles").toArray()) { auto profile = value.toObject(); profile["host"] = host; profiles << profile; }
        // The same accounts again change nothing on screen.
        const bool same = profiles == m_accountProfiles;
        m_accountProfiles = profiles;
        renderAccountMove();
        if (!same) scheduleRender();
    });
    QObject::connect(m_teams->client(), &HgsClient::accountsFailed, this, [this](quint64 request, const QString &host, const QString &) {
        if (request != m_accountsRequest || !host.isEmpty()) return;
        m_accountsRequest = 0; m_accountsRead = true;
        renderAccountMove();
    });
    QObject::connect(m_teams, &TeamController::feedChanged, this, [this] { pollRender(); });
    QObject::connect(m_teams, &TeamController::inboxChanged, this, [this] { pollRender(); });
    QObject::connect(qApp, &QApplication::focusChanged, this, [this](QWidget *, QWidget *now) {
        // Losing focus during a rebuild is recoverable; choosing another field
        // cancels both the recovery and an Answer still waiting for its box.
        if (now && !m_answerRendering) { m_answerAt.invalidate(); ++m_answerGeneration; }
    });
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
        "QScrollArea#teamWorkspaceMembers,QScrollArea#teamWorkspaceMembers>QWidget#qt_scrollarea_viewport,"
        "QWidget#teamWorkspaceSide,QWidget#teamSideLists{background:%1;border:0;}"
        "QSplitter#teamWorkspaceSplit::handle{background:%2;margin:0 3px;}"
        "QSplitter#teamWorkspaceSplit::handle:hover{background:%4;}"
        "QSplitter#teamWorkspaceSideSplit::handle{background:%2;margin:2px 40px;border-radius:1px;}"
        "QFrame#teamWorkspaceCoordinator{border-top:1px solid %2;}"
        "QWidget#teamWorkspaceCoordinatorHead{border-bottom:1px solid %2;}"
        "QLabel#teamWorkspaceEmpty{color:%3;font-size:13px;}"
        // U25: the prototype's .btn.dngf (Send back to Lead).
        "QPushButton[dangerFill=\"true\"]{background:%5;border-color:%5;color:#ffffff;font-weight:600;}"
        "QPushButton[dangerFill=\"true\"]:disabled{background:%6;border-color:transparent;color:%7;}")
        .arg(c.surface.name(), c.border.name(), c.muted.name(), c.accent.name(), c.danger.name(),
             alpha(c.danger, .35).name(QColor::HexArgb), alpha(QColor(Qt::white), .7).name(QColor::HexArgb)));
    // The prototype's .btn.sm buttons and its header pills (.pill.planp, .pill.acc).
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    const QString selBorder = WorkspaceSkin::color(WorkspaceSkin::active(), dark, QStringLiteral("cardSelBorder")).name();
    if (!m_headerStatus || !m_headerActions) { m_conversation->setTheme(dark); m_proposalCards->setDark(dark); scheduleRender(); return; }
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
    m_proposalCards->setDark(dark);
    if (m_profileView) m_profileView->setDark(dark);
    if (m_testsView) m_testsView->setDark(dark);
    scheduleRender();
}

void TeamWorkspace::showTeam(const QString &team)
{
    if (m_team != team) {
        m_answerAt.invalidate(); ++m_answerGeneration;
        m_team = team; m_focus.clear(); m_drawerTask = 0; m_fit = true; if (m_popover) m_popover->hide();
        // N14/N15: another team's send-back words and blocker emphasis stay behind.
        m_rejectPhase.clear(); m_rejectText.clear(); m_showBlockers = false;
        scheduleRender();
        return;
    }
    // #693: the session list names the same team after each of its polls.
    pollRender();
}

QString TeamWorkspace::rejectKey(const Team &team, int phase)
{
    double added = 0;
    for (const auto &p : team.phases) if (p.n == phase) added = p.addedAt;
    return team.id + QLatin1Char('\n') + QString::number(phase) + QLatin1Char('\n') + QString::number(added, 'f', 3)
           + QLatin1Char('\n') + QString::number(team.phaseNumbersRevision);
}

void TeamWorkspace::scheduleRender()
{
    m_forced = true;
    queueRender();
}

void TeamWorkspace::pollRender()
{
    queueRender();
}

void TeamWorkspace::queueRender()
{
    if (m_pending) return;
    m_pending = true;
    QTimer::singleShot(0, this, [this] {
        m_pending = false;
        const bool forced = std::exchange(m_forced, false);
        // #693: the same drawn content keeps every widget, its scroll and
        // hover; the header's ages tick in place.
        if (!forced && !m_renderedKey.isEmpty() && pollKey() == m_renderedKey) {
            if (liveKey() != m_renderedLive) renderLive();
            return;
        }
        render();
    });
}

QByteArray TeamWorkspace::pollKey() const
{
    QByteArray key = m_team.toUtf8() + '\n' + m_teams->shapeOf(m_team) + '\n' + m_teams->inboxShape() + '\n';
    const auto feed = m_teams->feed();
    key += QByteArray::number(feed.size()) + ' ' + QByteArray::number(feed.isEmpty() ? 0 : feed.first().value("seq").toInteger()) + '\n';
    const Team *team = currentTeam(m_team);
    // #706: members' live states reach the canvas beyond its activity labels
    // and working dots only through the team's status and its problems.
    if (team && m_live) {
        key += displayStatus(*team, m_live).toUtf8() + '\n';
        for (const auto &p : problems(*team, m_live))
            key += QByteArray::number(p.task) + ' ' + QByteArray::number(int(p.tone)) + ' ' + p.member.toUtf8() + ' ' + p.text.toUtf8() + '\n';
    }
    // G9: a phase decision stays marked until the revision moves.
    if (team && !m_phasesDecided.isEmpty()) key += "revision " + QByteArray::number(team->revision);
    return key;
}

QByteArray TeamWorkspace::liveKey(bool working) const
{
    // The text of what a member does shows only while it asks (Needs your answer).
    QByteArray key;
    const Team *team = currentTeam(m_team);
    if (team && m_live)
        for (const auto &member : team->members) {
            const Live live = m_live(member);
            key += member.id.toUtf8() + ' ' + char('0' + live.known) + char('0' + live.offline) + char('0' + live.attention)
                   + (working ? char('0' + live.working) : '-') + ' ' + (live.attention ? live.attentionText.toUtf8() : QByteArray()) + '\n';
        }
    return key;
}

void TeamWorkspace::renderLive()
{
    // Hidden or behind a confirmation, the whole render decides what to do.
    const Team *team = currentTeam(m_team);
    if (!team || !isVisible() || m_panel->confirming()) { render(); return; }
    const SlowEventLog::Scope scope(this, "live");
    const QScopedValueRollback<bool> rebuilding(m_answerRendering, true);
    renderHeader();
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    // Working and idle change only the members' marks; anything else of a
    // member's live state rebuilds the column.
    if (liveKey(false) != m_renderedLiveRest || !refreshMembersLive(*team)) renderSide(*team, displayStatus(*team, live));
    renderCoordinator(*team);
    refreshCanvasLive(*team);
    if (m_drawerTask) m_drawer->setTask(*team, m_drawerTask, live, m_dark);
    m_renderedLive = liveKey(); m_renderedLiveRest = liveKey(false);
}

static QString activityLabel(const Teams::Task &task, const Teams::Live &live);

namespace {
// A member's state mark in the Members column: its status, or the phase icon
// and tooltip of the attempt it works on (A1).
struct MemberMark { QString text, kind, tooltip, phase; QColor color; };
MemberMark memberMark(const Team &team, const Member &m, const Status &status, const Colors &c)
{
    const Task *current = status.task ? team.task(status.task) : nullptr;
    const auto mark = current ? phaseMark(team, *current) : PhaseMark{};
    const QString shown = team.status == "planning" && m.session.isEmpty() && !m.lead ? TeamWorkspace::tr("On approval") : status.text;
    MemberMark result;
    result.text = shown; result.kind = mark.kind.isEmpty() ? status.kind : mark.kind; result.tooltip = mark.tooltip;
    result.color = toneColor(c, mark.tone != Tone::Neutral ? mark.tone : status.tone); result.phase = mark.phase;
    return result;
}
}

bool TeamWorkspace::refreshMembersLive(const Team &team)
{
    if (!m_membersContent) return false;
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    for (auto *widget : m_membersContent->findChildren<QWidget *>(QStringLiteral("teamWorkspaceMemberState"))) {
        auto *state = dynamic_cast<StatusMark *>(widget);
        QWidget *card = widget->parentWidget();
        while (card && card->objectName() != QLatin1String("teamWorkspaceMember")) card = card->parentWidget();
        const auto *m = card ? team.member(card->property("teamMember").toString()) : nullptr;
        if (!state || !m) return false;
        const auto mark = memberMark(team, *m, memberStatus(team, *m, live(*m)), c);
        if (mark.phase != state->property("phase").toString()) return false;
        state->setState(mark.text, mark.color, mark.kind, mark.tooltip);
    }
    return true;
}

void TeamWorkspace::refreshCanvasLive(const Team &team)
{
    if (!m_left) return;
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    const auto activityOf = [&](const Task &task) {
        const auto *member = team.member(task.assignee);
        return activityLabel(task, member ? live(*member) : Live{});
    };
    // Board and Layers: the labels that show a task's activity carry its id.
    for (auto *label : m_left->findChildren<QLabel *>()) {
        const int id = label->property("liveTask").toInt();
        const auto *task = id ? team.task(id) : nullptr;
        if (!task) continue;
        const QString text = activityOf(*task);
        if (text == label->text()) continue;
        label->setText(text);
        if (!label->toolTip().isEmpty()) label->setToolTip(text);
    }
    // Graph: each view's nodes, as graphNodes sets them.
    for (auto *widget : m_left->findChildren<QWidget *>()) {
        auto *view = dynamic_cast<TaskView *>(widget);
        if (!view) continue;
        view->refreshLive([&](TaskView::Node &node) {
            const auto *task = team.task(node.id);
            if (!task) return;
            const auto *assignee = team.member(task->assignee);
            if (!node.activity.isEmpty()) node.activity = activityOf(*task);
            node.working = assignee && task->status == "in_progress" && !node.problem && assignee->state != "paused"
                           && team.status == "running" && !live(*assignee).offline && !live(*assignee).attention;
        });
    }
}

void TeamWorkspace::tickInPlace()
{
    // #693: the ages a skipped rebuild leaves (the header's elapsed time, a
    // stage queue's minutes) are set again where they are.
    const double now = workedClock()();
    tickAged(this, now);
    if (m_headerStatus) tickAged(m_headerStatus, now);
    // U38: a shown heat tint is read again a minute after its last read.
    if (const auto *team = currentTeam(m_team); team && !heatShown(*team).isEmpty()) heat(*team);
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
    const SlowEventLog::Scope scope(this, "render");
    const QScopedValueRollback<bool> rebuilding(m_answerRendering, true);
    // #708: a render replaces the view, so the previous render's sliced cards
    // must stop before their widgets go.
    cancelSlices();
    renderHeader();
    // #693: a skipped rebuild is made up by the next poll.
    m_renderedKey.clear();
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
        // N15: the blockers light up only while the team is stuck.
        if (status != "stuck") m_showBlockers = false;
        // N14: renumbered drafts (a discard, a move) close the send-back form.
        if (!m_rejectPhase.isEmpty() && !m_rejectPhase.endsWith(QLatin1Char('\n') + QString::number(team->phaseNumbersRevision))) {
            m_rejectPhase.clear(); m_rejectText.clear();
        }
        // G5: the drafts wait in the members column; the canvas keeps its height.
        if (team->hasFinish) tv->addWidget(finishSummary(*team));
        // B5: a plan that widens a role's permissions waits for the owner's
        // explicit confirmation; the strip names what widens.
        if (!team->widenings.isEmpty()) tv->addWidget(wideningStrip(*team));
        // F8, U57: the lead's open proposals are cards in the coordinator's
        // conversation; the decided ones leave the sent set.
        {
            QSet<QString> open;
            for (const auto &proposal : team->proposals) if (proposal.open()) open << proposal.id;
            m_decidedProposals.intersect(open);
        }
        // U59: a request to switch to free opens its own dialog, once.
        popSwitchRequest(*team);
        // C4: the owner's attention list is the first group of the side
        // column; the lead's inbox (C2) is a drawer under the canvas.
        if (!m_error.isEmpty()) tv->addWidget(errorStrip());
        // U25 (spec 5.7): Needs your answer, Team stuck and Plan problems are
        // collapsed lists at the top of the right column now.
        // The send-back form being typed in keeps the focus across the rebuild.
        if (const auto *focused = QApplication::focusWidget(); focused && focused->objectName() == "teamPhaseRejectText") m_rejectFocus = true;
        // The canvas column.
        auto *left = new QWidget; left->setObjectName("teamWorkspaceLeft");
        auto *lv = new QVBoxLayout(left); lv->setContentsMargins(0, 0, 0, 0); lv->setSpacing(0);
        lv->addWidget(toolbar(*team));
        if (auto *legend = heatLegendRow(*team)) lv->addWidget(legend);
        // U59: the lead's proposed plan mode and the owner's choice.
        if (team->status == "planning" && team->planModeKnown) {
            auto *modes = new QWidget; modes->setObjectName("teamPlanModeRow");
            auto *mv = new QVBoxLayout(modes); mv->setContentsMargins(12, 4, 12, 6); mv->setSpacing(0);
            mv->addWidget(planModeCard(*team));
            lv->addWidget(modes);
        }
        // U58: Graph, Board and Layers leave out cancelled tasks and the phases
        // they emptied unless Show cancelled is on; edits read the whole team.
        // Owner, 2026-10-30: Hide finished leaves out the phases whose tasks are
        // all decided.
        const Team shown = m_teams->showCancelled() ? *team : withoutCancelled(*team);
        lv->addWidget(canvas(m_hideFinished ? withoutFinished(shown) : shown), 1);
        if (const auto &items = m_teams->inbox(); !items.isEmpty()) lv->addWidget(inboxStrip(*team, items));
        lv->addWidget(feed(*team));
        // The Profile view outlives the canvas it was shown in.
        if (m_profileView && m_left && m_left->isAncestorOf(m_profileView)) { m_profileView->hide(); m_profileView->setParent(this); }
        if (m_testsView && m_left && m_left->isAncestorOf(m_testsView)) { m_testsView->hide(); m_testsView->setParent(this); }
        if (m_left) { m_leftHost->removeWidget(m_left); retire(m_left); }
        m_leftHost->addWidget(left); left->show(); m_left = left;
        renderSide(*team, status);
        renderCoordinator(*team);
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
    if (team) { m_renderedKey = pollKey(); m_renderedLive = liveKey(); m_renderedLiveRest = liveKey(false); }
    // A small view is finished inside this render, as it always was; a large
    // one keeps its remaining cards for the sliced turns and paints at once.
    if (m_slices.size() <= kSyncSlices) runSlices();
}

void TeamWorkspace::queueSlice(std::function<void()> work)
{
    m_slices.push_back(std::move(work));
    if (m_sliceTimer && !m_sliceTimer->isActive()) m_sliceTimer->start();
}

void TeamWorkspace::runSlices()
{
    QElapsedTimer budget;
    budget.start();
    while (!m_slices.empty()) {
        auto work = std::move(m_slices.front());
        m_slices.pop_front();
        work();
        if (budget.elapsed() >= kSliceMs) break;
    }
    if (!m_slices.empty() && m_sliceTimer) m_sliceTimer->start();
}

void TeamWorkspace::cancelSlices()
{
    m_slices.clear();
    if (m_sliceTimer) m_sliceTimer->stop();
}

void TeamWorkspace::renderSide(const Team &team, const QString &status)
{
    // Members keep their scroll position.
    const int scroll = m_members->verticalScrollBar()->value();
    // A poll may rebuild Members just after Answer focused Allow. Keep a
    // decision button focused only for the exact same pending question;
    // Enter must never transfer to another member or a newer request.
    QString focusTeam, focusMember, focusQuestion, focusHash, focusControl;
    if (const auto *focused = QApplication::focusWidget(); focused && m_members->isAncestorOf(focused)
        && (focused->objectName() == "teamMemberPermissionAllow" || focused->objectName() == "teamMemberPermissionDeny")) {
        for (auto *box = focused->parentWidget(); box; box = box->parentWidget()) {
            if (box->objectName() != "teamMemberPermission") continue;
            focusTeam = box->property("team").toString();
            focusMember = box->property("member").toString();
            focusQuestion = box->property("question").toString();
            focusHash = box->property("questionHash").toString();
            focusControl = focused->objectName();
            break;
        }
    }
    auto *list = new QWidget; list->setObjectName("teamWorkspaceSide");
    {auto *sv = new QVBoxLayout(list); sv->setContentsMargins(0, 0, 0, 0); sv->setSpacing(0);
     if (auto *lists = sideLists(team, status)) sv->addWidget(lists);
     sv->addWidget(members(team), 1);}
    retire(m_members->takeWidget());
    m_members->setWidget(list); m_membersContent = list;
    m_members->verticalScrollBar()->setValue(scroll);
    // Answer was clicked a moment ago and the focus has already gone (a
    // rebuild before this one, or the window briefly inactive): focus the
    // same question's Allow again (fork CI, leadAnswersAMembersSubagentRequest).
    if (focusControl.isEmpty() && m_answerAt.isValid() && m_answerAt.elapsed() < 3000 && m_answerTeam == team.id) {
        focusMember = m_answerMember; focusQuestion = m_answerQuestion; focusHash = m_answerHash;
        focusTeam = m_answerTeam; focusControl = QStringLiteral("teamMemberPermissionAllow");
    }
    if (!focusControl.isEmpty() && focusTeam == team.id && !focusQuestion.isEmpty() && !focusHash.isEmpty()) {
        for (auto *box : list->findChildren<QFrame *>("teamMemberPermission")) {
            if (box->property("member").toString() != focusMember || box->property("question").toString() != focusQuestion
                || box->property("questionHash").toString() != focusHash) continue;
            if (auto *control = box->findChild<QPushButton *>(focusControl); control && control->isEnabled() && control->isVisible())
                control->setFocus(Qt::OtherFocusReason);
            break;
        }
    }
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

QWidget *TeamWorkspace::profileView(const Team &team)
{
    if (!m_profileView) {
        m_profileView = new TeamProfileView(m_teams, m_profileCache, this);
        m_profileView->setDark(m_dark);
        m_profileView->openTask = [this](int task) { m_drawerTask = task; scheduleRender(); };
    }
    m_profileView->setTeam(team);
    m_profileView->show();
    return m_profileView;
}

QWidget *TeamWorkspace::testsView(const Team &team)
{
    if (!m_testsView) {
        m_testsView = new TeamTestsView(m_teams, this);
        m_testsView->setDark(m_dark);
        m_testsView->openTask = [this](int task) { m_drawerTask = task; scheduleRender(); };
    }
    m_testsView->setTeam(team);
    m_testsView->show();
    return m_testsView;
}

void TeamWorkspace::showTestRecord(const QString &record)
{
    m_view = QStringLiteral("tests"); m_drawerTask = 0; m_drawer->hide();
    if (const auto *team = currentTeam(m_team)) { testsView(*team); m_testsView->hide(); }
    if (m_testsView) m_testsView->showRecord(record);
    scheduleRender();
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
    // U59: the plan mode beside the phase; a free phase's last build and what
    // was accepted since, and a close that waits for its build.
    if (team.planModeKnown && team.status != "planning" && team.status != "draft") {
        const bool free = team.freeMode();
        auto *mode = new PhaseBadge(free ? tr("Free") : tr("Standard"), free ? c.violet : c.text3,
                                    free ? QStringLiteral("plan_free") : QStringLiteral("plan_standard"), false);
        mode->setObjectName("teamHeaderPlanMode"); mode->setProperty("mode", team.planMode);
        mode->setToolTip(free ? tr("Free planning: phase %1 stays open and takes tasks as they come until you close it").arg(team.freePhase.phase)
                              : tr("Standard planning: each phase is planned, approved and closed with its build"));
        row->add(mode, 0);
        if (free && team.freePhase.known) {
            auto *build = small(label(freeBuildLine(team), team.freePhase.acceptedSince > 0 ? c.warn : c.muted, 11, false, "teamHeaderBuild"),
                                team.freePhase.acceptedSince > 0 ? c.warn : c.muted);
            build->setToolTip(tr("The last integration and build of the open phase, and the tasks accepted since"));
            row->add(build, 4);
            if (team.freePhase.closePending > 0) {
                auto *pending = small(label(closeBlocked(team), c.accent, 11, false, "teamHeaderClosePending"), c.accent);
                row->add(pending, 4);
            }
        }
    }
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
        // N16 (#382): accepted tasks, as the phase's scroll counts them; a
        // rejected or cancelled task is decided, not done.
        int accepted = 0;
        for (const auto &task : team.tasks) accepted += task.phase == progress.phase && task.status == "accepted";
        if (phaseGroups(team).isEmpty()) { accepted = 0; for (const auto &task : team.tasks) accepted += task.status == "accepted"; }
        auto *phase = new MiniProgress(c, caption, accepted, progress.total, c.accent, 150);
        phase->setObjectName("teamHeaderPhase");
        phase->setToolTip(tr("Current phase: accepted / tasks in phase %1").arg(progress.phase));
        row->add(phase, 5);
    }
    int accepted = 0; for (const auto &task : team.tasks) accepted += task.status == "accepted";
    auto *tasks = new MiniProgress(c, tr("Tasks"), accepted, int(team.tasks.size()), c.accent, 72);
    tasks->setObjectName("teamHeaderTasks"); tasks->setToolTip(tr("Accepted / all tasks of the team"));
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
        // #693: it ticks in place on the minute timer.
        h->addWidget(small(aged(label(QString(), c.muted, 11, false, "teamHeaderElapsedText"), QStringLiteral("%1"), QStringLiteral("duration"), team.created), c.muted));
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
            scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
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
                if (!finding.state.isEmpty()) rv->addWidget(label(tr("state: %1%2%3").arg(finding.state,
                    finding.phase > 0 ? tr(" · phase %1").arg(finding.phase) : QString(),
                    finding.note.isEmpty() ? QString() : tr(" — %1").arg(finding.note)), fc.muted, 11));
                // N17: the owner's replan proposal stays with its finding.
                if (!finding.proposal.isEmpty()) {
                    auto *proposal = label(tr("Proposal: %1").arg(finding.proposal), fc.text2, 11, false, "teamFindingProposal");
                    proposal->setWordWrap(true);
                    rv->addWidget(proposal);
                }
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
                        QJsonObject input{{"finding", finding.id}, {"action", choice.first}, {"note", QString()}};
                        if (choice.first == "replan") {
                            // N17 (spec 8): a replan carries the owner's proposal; the
                            // team pauses until the owner resumes it.
                            bool given = false;
                            TeamPanel::RenderHold hold(m_panel);
                            const QString proposal = QInputDialog::getMultiLineText(
                                this, tr("Replan"), tr("What should the lead change in the plan for \"%1\"? The team pauses until you resume it.").arg(finding.title),
                                QString(), &given).trimmed();
                            if (!given || proposal.isEmpty()) return;
                            input["proposal"] = proposal;
                        } else if (!m_panel->confirm("teamFindingActionConfirm", choice.second,
                                                     tr("%1 finding \"%2\"?").arg(choice.second, finding.title), choice.second)) {
                            return;
                        }
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
        // N33 (#548): only the switch's own field, as N13 for the others; the
        // server merges it, so a lead's newer settings stay.
        QObject::connect(limits, &QPushButton::clicked, this, [this](bool on) {
            m_panel->act("settings", {{"settings", QJsonObject{{"resume_after_limits", on}}}});
        });
        h->addWidget(limits);
        // U48, U51, U52: branches, crashed members and intake keys, in one dialog.
        auto *settings = new QToolButton; settings->setObjectName("teamWorkspaceSettings"); settings->setAutoRaise(true);
        settings->setIcon(workspaceIcon("settings", c.text2)); settings->setToolTip(tr("Team settings…")); settings->setAccessibleName(tr("Team settings"));
        // The dialog runs its own event loop: it opens from the workspace, never
        // from this button, which a poll may rebuild meanwhile.
        QObject::connect(settings, &QToolButton::clicked, this, [this, id = team.id] {
            QTimer::singleShot(0, this, [this, id] { if (const auto *current = currentTeam(id)) teamSettings(*current); });
        });
        h->addWidget(settings);
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
            // The dialog runs its own event loop and a poll rebuilds the header
            // meanwhile: it opens from the workspace, never from this button.
            QTimer::singleShot(0, this, [this] {
                bool ok = false;
                const auto comment = QInputDialog::getMultiLineText(this, tr("Send back to the coordinator"), tr("What should change in the plan?"), {}, &ok);
                if (ok && !comment.trimmed().isEmpty()) m_conversation->send(tr("Your plan needs changes before I approve it: %1").arg(comment.trimmed()));
            });
        });
        back->setToolTip(tr("Ask the coordinator to change the plan")); iconFor(back, "arrow-left");
        // U59: the owner's free choice shows on the button.
        const bool free = team.planModeKnown && m_panel->planModeChoice(team.id) == "free";
        auto *approve = add(free ? tr("Approve && Run in free mode") : tr("Approve && Run"), "teamWorkspaceApprove", true, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) approveTeam(*current);
        });
        iconFor(approve, "play");
        // The plan's errors come only with `team status`: until it has answered
        // (a list poll carries no validation), Approve & Run waits.
        approve->setEnabled(team.full && team.errors.isEmpty() && !team.tasks.isEmpty());
        if (!team.full) approve->setToolTip(tr("Checking the plan…"));
        else if (!approve->isEnabled()) approve->setToolTip(team.tasks.isEmpty() ? tr("The plan has no tasks yet") : tr("Fix the plan errors first"));
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
    // U59: the open free phase builds when asked and closes on your word.
    if (team.freeMode() && team.freePhase.known && (team.status == "running" || team.status == "paused")) {
        auto *build = add(tr("Integrate && build"), "teamWorkspaceIntegrateBuild", false, [this, id = team.id] {
            if (const auto *current = currentTeam(id)) integrateBuild(*current);
        });
        const int open = openBuildTask(team);
        build->setEnabled(!open && !m_buildSent.contains(team.id));
        build->setToolTip(open ? tr("#%1 builds the phase").arg(open) : tr("Add a lead task that integrates the accepted work and runs the build"));
        iconFor(build, "terminal");
        auto *close = add(tr("Close phase…"), "teamWorkspaceClosePhase", false, [this, id = team.id] {
            // The dialog is asked from the workspace, never from this button,
            // which a poll may rebuild meanwhile.
            QTimer::singleShot(0, this, [this, id] { if (const auto *current = currentTeam(id)) closePhaseDialog(*current); });
        });
        const QString blocked = closeBlocked(team);
        close->setEnabled(blocked.isEmpty());
        close->setToolTip(blocked.isEmpty() ? tr("Close the free phase; the team returns to standard planning") : blocked);
        iconFor(close, "stop");
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
        // N17 (#382): Archive asks first, as the prototype does.
        auto *archive = add(tr("Archive"), "teamWorkspaceArchive", false, [this, id = team.id, name = team.name] {
            if (!m_panel->confirm("teamArchiveConfirm", tr("Archive team?"),
                                  tr("Archive %1? Its history, tasks and reports stay; the team leaves the list of teams.").arg(name), tr("Archive")))
                return;
            // Read it again: a poll may have changed it while the question was open.
            const auto *current = currentTeam(id);
            if (current && (current->status == "done" || current->status == "stopped")) m_panel->act("archive", {{"team", id}});
        });
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
    // B5/N17: always asked; a widening asks for its own confirmation, and the
    // approve request carries confirm_widening only after it.
    m_panel->approvePlan(team);
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
    // #724: the same question as Waiting for you's Approve (Teams::phaseApproval).
    const auto approval = phaseApproval(*team, phase);
    if (!m_panel->confirm(approval.name, approval.title, approval.text, approval.action)) return false;
    // The team may have been refreshed while the dialog was open: read it again.
    team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    const QString key = markPhaseDecided(*team, phase);
    QJsonObject request = approval.input;
    request.insert("team", id);
    m_panel->act("approve-phase", request, [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
    return true;
}

bool TeamWorkspace::discardPhase(const QString &id, int phase)
{
    const auto *team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    int count = 0;
    QString state;
    for (const auto &group : phaseGroups(*team)) if (group.n == phase) { count = group.tasks.size(); state = ownerPhaseState(*team, group); }
    // U39: an accepted phase can be discarded too, while none of its work has
    // started; the confirmation says it is already accepted.
    const bool accepted = state == "queued" || state == "running";
    if (!m_panel->confirm("teamDiscardPhaseConfirm", tr("Discard phase?"),
                          accepted ? tr("Phase %1 is already accepted. Discard it and its %2 tasks? Only a phase whose work has not started can go; this cannot be undone.").arg(phase).arg(count)
                                   : tr("Discard phase %1 and its %2 tasks?").arg(phase).arg(count),
                          tr("Discard phase")))
        return false;
    team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    const QString key = markPhaseDecided(*team, phase);
    m_panel->act("discard-phase", {{"team", id}, {"phase", phase}}, [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
    return true;
}

bool TeamWorkspace::returnPhaseToDraft(const QString &id, int phase)
{
    const auto *team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    if (!m_panel->confirm("teamReturnPhaseConfirm", tr("Return phase to draft?"),
                          tr("Phase %1 goes back to a draft: its tasks are not handed out until you approve it again. Only a phase whose work has not started can return.").arg(phase),
                          tr("Return to draft")))
        return false;
    team = currentTeam(id);
    if (!team || m_phaseAct || phaseDecided(*team, phase)) return false;
    const QString key = markPhaseDecided(*team, phase);
    m_panel->act("return-to-draft", {{"team", id}, {"phase", phase}}, [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
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
    // Owner, 2026-10-28: the lead's inbox is a drawer at the bottom like the
    // feed, not a strip over the canvas. Folded, its header names the count
    // and the oldest item; open, every item, oldest first.
    const Colors c = colorsFor(m_dark);
    auto *strip = new QFrame; strip->setObjectName("teamInboxStrip");
    strip->setStyleSheet(QStringLiteral("QFrame#teamInboxStrip{border-top:1px solid %1;}").arg(c.border.name()));
    auto *v = new QVBoxLayout(strip); v->setContentsMargins(12, 4, 12, 6); v->setSpacing(2);
    const auto kindOf = [](const InboxItem &item) {
        return item.kind == "report" ? tr("report") : item.kind == "question" ? tr("question")
             : item.kind == "permission" ? tr("permission") : item.kind == "lead_task" ? tr("lead task")
             : item.kind == "stalled" ? tr("stalled") : item.kind == "budget" ? tr("budget") : item.kind;
    };
    auto *head = row(6);
    auto *toggle = new QToolButton; toggle->setIcon(workspaceIcon(m_inboxOpen ? "chevron-down" : "chevron-right", c.fg)); toggle->setAutoRaise(true);
    toggle->setObjectName("teamInboxToggle");
    QObject::connect(toggle, &QToolButton::clicked, this, [this] { m_inboxOpen = !m_inboxOpen; scheduleRender(); });
    head->addWidget(toggle);
    head->addWidget(label(tr("Inbox"), c.fg, 12, true, "teamInboxTitle"));
    head->addWidget(label(items.size() == 1 ? tr("1 for the lead") : tr("%1 for the lead").arg(items.size()), c.accent, 11));
    if (!m_inboxOpen) {
        const auto &first = items.first();
        auto *latest = elided(QStringLiteral("· %1 %2%3 %4").arg(kindOf(first), first.member,
                                  first.task > 0 ? QStringLiteral(" #%1").arg(first.task) : QString(), first.summary), c.text3, 11);
        latest->setObjectName("teamInboxSummary");
        latest->setToolTip(first.command);
        head->addWidget(latest, 1);
    } else head->addStretch();
    v->addLayout(head);
    if (!m_inboxOpen) return strip;
    // C2: oldest first, the command that answers it in the tooltip.
    auto *rows = new QWidget; auto *rv = new QVBoxLayout(rows); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(2);
    for (const auto &item : items) {
        auto *line = row(6);
        line->addWidget(new Pill(kindOf(item), item.kind == "report" ? c.warn : c.accent));
        line->addWidget(label(item.member, c.text2, 11));
        if (item.task > 0) line->addWidget(label(QStringLiteral("#%1").arg(item.task), c.muted, 11));
        auto *summary = elided(item.summary, c.fg, 11);
        summary->setObjectName("teamInboxSummary");
        summary->setToolTip(item.command);
        line->addWidget(summary, 1);
        rv->addLayout(line);
    }
    rv->addStretch();
    auto *scroll = new QScrollArea; scroll->setWidget(rows); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setFixedHeight(qMin(170, int(items.size()) * 24 + 6));
    keepScroll(scroll, QStringLiteral("inbox"));
    v->addWidget(scroll);
    Q_UNUSED(team);
    return strip;
}

void TeamWorkspace::attentionRows(const Team &team, QVBoxLayout *v)
{
    const Colors c = colorsFor(m_dark);
    // N7 (U18): one line per item - the member, what waits and its age - and
    // Answer on a permission the lead left unanswered.
    int shown = 0;
    for (const auto &item : team.attention) {
        if (shown == 5) break;
        ++shown;
        const QString line = TeamAttentionNotices::what(item) + QStringLiteral(" · ") + TeamAttentionNotices::age(item);
        auto *r = row(6);
        auto *text = elided(line, c.text2, 12);
        text->setObjectName("teamAttentionLine"); text->setProperty("kind", item.kind);
        text->setToolTip(item.command.isEmpty() ? line : line + QLatin1Char('\n') + item.command);
        r->addWidget(text, 1);
        if (item.kind == "permission") {
            auto *answer = button(tr("Answer"), "teamAttentionAnswer", true);
            answer->setProperty("member", item.member);
            answer->setToolTip(tr("Allow or deny it in %1's card").arg(item.member));
            connect(answer, &QPushButton::clicked, this, [this, teamId = team.id, member = item.member, command = item.command] {
                answerAttention(teamId, member, command);
            });
            r->addWidget(answer);
        }
        v->addLayout(r);
    }
    if (team.attention.size() > shown) v->addWidget(label(tr("+%1 more").arg(team.attention.size() - shown), c.muted, 10));
}

void TeamWorkspace::answerAttention(const QString &teamId, const QString &member, const QString &command, int attempts,
                                    const QString &question, const QString &hash, quint64 generation)
{
    // A retry belongs to the question clicked, not whatever a later poll or
    // a switch to another team puts in the same member's place.
    if (m_team != teamId || (generation && generation != m_answerGeneration)) return;
    const auto *team = currentTeam(teamId);
    if (!team) return;
    QString id, currentQuestion, currentHash;
    for (const auto &m : team->members)
        if (m.id == member || team->roleOf(m.id) == member) {
            id = m.id; currentQuestion = m.permission.id; currentHash = m.permission.hash; break;
        }
    if (generation && (question != currentQuestion || hash != currentHash)) return;
    if (!generation) { generation = ++m_answerGeneration; m_answerAt.invalidate(); }
    for (auto *box : findChildren<QFrame *>("teamMemberPermission")) {
        if (box->property("member").toString() != id) continue;
        if (box->property("team").toString() != teamId || box->property("question").toString() != currentQuestion
            || box->property("questionHash").toString() != currentHash) return;
        // Built but not shown yet (a rebuild in progress on a loaded
        // computer): look again shortly instead of falling back to the command.
        if (!box->isVisible()) {
            if (attempts > 0) {
                QTimer::singleShot(100, this, [this, teamId, id, command, attempts, currentQuestion, currentHash, generation] {
                    answerAttention(teamId, id, command, attempts - 1, currentQuestion, currentHash, generation);
                });
                return;
            }
            continue;
        }
        m_members->ensureWidgetVisible(box, 0, 24);
        auto *allow = box->findChild<QPushButton *>("teamMemberPermissionAllow");
        if (allow && allow->isEnabled()) {
            allow->setFocus(Qt::OtherFocusReason);
            m_answerTeam = teamId; m_answerMember = box->property("member").toString();
            m_answerQuestion = box->property("question").toString(); m_answerHash = box->property("questionHash").toString();
            m_answerAt.start();
        }
        else if (auto *reason = box->findChild<QLineEdit *>("teamMemberPermissionDenyReason")) reason->setFocus(Qt::OtherFocusReason);
        return;
    }
    // No answer box here (an older hgs sends no question): the command.
    if (command.isEmpty()) return;
    QGuiApplication::clipboard()->setText(command);
    m_error = tr("The answer command is on the clipboard: fill in the option and run it.");
    render();
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

const Teams::Team *TeamWorkspace::planTeam(const QString &id)
{
    const auto *team = currentTeam(id);
    if (team && team->full) return team;
    m_error = tr("Loading the plan; try again in a moment.");
    m_teams->watch(id);
    scheduleRender();
    return nullptr;
}

// Owner, 2026-10-28: a sideways bar must not come and go. An as-needed bar
// appeared a moment after each poll's rebuild and flipped at the edge of the
// room; this one is on exactly while the content's minimum width is more
// than the area's room, decided at once and again on each resize.
class SideBarFit : public QObject {
public:
    SideBarFit(QScrollArea *area, QWidget *content) : QObject(area), m_area(area), m_content(content)
    {
        area->installEventFilter(this);
        content->installEventFilter(this);
        // #693: never decided before the content is laid out: the area's
        // first width is no room at all and showed the bar for a moment.
        area->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        m_pending = true;
        QMetaObject::invokeMethod(this, [this] { m_pending = false; fit(); }, Qt::QueuedConnection);
    }
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        // Deferred: a resize or layout request can arrive while the area is
        // being destroyed; the queued call dies with this object.
        if ((event->type() == QEvent::Resize || event->type() == QEvent::LayoutRequest || event->type() == QEvent::Show) && !m_pending) {
            m_pending = true;
            QMetaObject::invokeMethod(this, [this] { m_pending = false; fit(); }, Qt::QueuedConnection);
        }
        return QObject::eventFilter(watched, event);
    }
private:
    void fit()
    {
        if (!m_content || !m_area->isVisible() || m_area->width() <= 1) return;
        // #693: the vertical bar's room is kept while the content is taller
        // than the area, whether or not that bar shows yet, so the two bars
        // never flip each other at the edge.
        const int frame = 2 * m_area->frameWidth();
        const int tall = qMax(m_content->minimumSizeHint().height(), m_content->minimumHeight());
        const bool vertical = m_area->verticalScrollBarPolicy() == Qt::ScrollBarAlwaysOn
                              || (m_area->verticalScrollBarPolicy() == Qt::ScrollBarAsNeeded && tall > m_area->height() - frame);
        const int vbar = vertical ? m_area->verticalScrollBar()->sizeHint().width() : 0;
        // A view laid out by hand (the phase graph) has no size hint, only its
        // minimum width.
        const int want = qMax(m_content->minimumSizeHint().width(), m_content->minimumWidth());
        const bool need = want > m_area->width() - vbar - frame;
        const auto policy = need ? Qt::ScrollBarAlwaysOn : Qt::ScrollBarAlwaysOff;
        if (m_area->horizontalScrollBarPolicy() != policy) m_area->setHorizontalScrollBarPolicy(policy);
    }
    QScrollArea *m_area;
    QPointer<QWidget> m_content;
    bool m_pending = false;
};

// QA #500, finding 1 (SPEC 5.3): the Board scrolls every phase sideways as
// one page (U33), but a phase's header stays in the visible part: at most as
// wide as the viewport, and moved along with the sideways scroll, so its
// actions never leave the view; its info row yields first (FitRow).
class PinnedPhaseHeaders : public QObject {
public:
    PinnedPhaseHeaders(QScrollArea *area, QWidget *content) : QObject(area), m_area(area), m_content(content)
    {
        area->viewport()->installEventFilter(this);
        QObject::connect(area->horizontalScrollBar(), &QScrollBar::valueChanged, this, [this] { pin(); });
        pin();
    }
    bool eventFilter(QObject *watched, QEvent *event) override
    {
        if (event->type() == QEvent::Resize) pin();
        return QObject::eventFilter(watched, event);
    }
private:
    void pin()
    {
        if (!m_content) return;
        const int offset = qMax(0, m_area->horizontalScrollBar()->value());
        for (auto *holder : m_content->findChildren<QWidget *>(QStringLiteral("teamPhaseHeaderPin"))) {
            // The same margin on the right as the phase frame has on the left.
            const int left = holder->mapTo(m_content, QPoint(0, 0)).x();
            const int room = qMax(240, m_area->viewport()->width() - 2 * left);
            if (auto *layout = holder->layout(); layout && layout->contentsMargins().left() != offset) layout->setContentsMargins(offset, 0, 0, 0);
            if (auto *head = holder->findChild<QWidget *>(QStringLiteral("teamPhaseHeader")); head && head->maximumWidth() != room) head->setMaximumWidth(room);
        }
    }
    QScrollArea *m_area;
    QPointer<QWidget> m_content;
};

// Owner, 2026-10-28: on a narrow window the workspace toolbar keeps its icons
// and drops their words. A button marked "compactText" shows its text only
// while the whole row fits, its "compactIcon" only while it does not, and a
// label marked "compactHide" hides; the words move to the tooltips.
class ToolbarRow : public QWidget {
public:
    using QWidget::QWidget;
    bool compact() const { return m_level == 3; }
protected:
    void resizeEvent(QResizeEvent *event) override { QWidget::resizeEvent(event); refit(); }
    void showEvent(QShowEvent *event) override { QWidget::showEvent(event); refit(); }
private:
    void fitCompactSpacing()
    {
        auto *row = qobject_cast<QHBoxLayout *>(layout());
        if (!row) return;
        row->setSpacing(2); row->setContentsMargins(4, 8, 4, 6);
        // Style-dependent group frames and switch hints can leave the icon-only
        // row a few pixels too wide. Use its remaining whitespace before any
        // control is clipped, keeping each button's full click target.
        if (sizeHint().width() > width()) {
            row->setSpacing(0); row->setContentsMargins(0, 8, 0, 6);
        }
    }
    // Steps: first the switch labels go (their switches keep them as
    // tooltips) and the wide Heat switch gives way to its one-button menu
    // ("toolbarLevels"), then the filter's words, then every button's words
    // (the icon-only row leaves out Hide finished and the zoom group). Each
    // step is applied and measured, widest first, so the row keeps as many
    // words as truly fit.
    void refit()
    {
        if (m_fitting) return;
        const QScopedValueRollback<bool> once(m_fitting, true);
        // whitewh-115: the shortened toolbar jumped between two steps at the
        // boundaries because a step's own hiding changed the row it was
        // measured from. Measure every step with its own controls first, from
        // the same width, then choose: the step it was decided from never
        // depends on the step currently applied.
        int need[4] = {0, 0, 0, 0};
        for (int step = 0; step < 4; ++step) {
            apply(step);
            need[step] = sizeHint().width();
        }
        const int room = width();
        int level = m_level < 0 ? 0 : qBound(0, m_level, 3);
        while (level < 3 && need[level] > room) ++level;
        // Hysteresis: widening needs a little slack beyond the narrower step's
        // own measure, so a poll or a refit exactly at the boundary keeps the
        // step it is on instead of flipping back and forth.
        while (level > 0 && need[level - 1] <= room - kGrowSlack) --level;
        apply(level);
    }
    /// The slack a refit needs before it widens the toolbar by one step.
    static constexpr int kGrowSlack = 8;
    void apply(int level)
    {
        if (level == m_level) { if (level == 3) fitCompactSpacing(); return; }
        m_level = level;
        const bool compact = level == 3;
        for (auto *l : findChildren<QLabel *>()) if (l->property("compactHide").toBool()) l->setVisible(level == 0);
        // "toolbarLevels": "0" shows at the full row, "1" at the narrower
        // rows with words; the icon-only row leaves the Heat switch out.
        for (auto *w : findChildren<QWidget *>())
            if (const QString levels = w->property("toolbarLevels").toString(); !levels.isEmpty())
                w->setVisible(level < 3 && levels.contains(QLatin1Char(level == 0 ? '0' : '1')));
        // The icon-only row leaves out a view preference (Hide finished, which
        // stays set) and the zoom group.
        for (auto *w : findChildren<QWidget *>()) if (w->property("compactDrop").toBool()) w->setVisible(level < 3);
        // Icon-only buttons take their icon's width, not the style's text
        // minimum, and each narrower step draws the groups closer together.
        if (auto *row = qobject_cast<QHBoxLayout *>(layout())) {
            row->setSpacing(compact ? 2 : level == 0 ? 8 : 6);
            row->setContentsMargins(compact ? 4 : level == 0 ? 12 : 8, 8, compact ? 4 : level == 0 ? 12 : 8, 6);
        }
        for (auto *b : findChildren<QPushButton *>()) {
            const QString text = b->property("compactText").toString();
            const bool zoom = b->objectName() == "teamWorkspaceZoomOut" || b->objectName() == "teamWorkspaceZoomIn";
            if (zoom) { if (compact) b->setFixedWidth(30); else { b->setMinimumWidth(0); b->setMaximumWidth(QWIDGETSIZE_MAX); } }
            if (text.isEmpty()) continue;
            const bool iconOnly = compact || (level == 2 && b->property("compactFirst").toBool());
            if (iconOnly) b->setFixedWidth(b->iconSize().width() + 16);
            else { b->setMinimumWidth(0); b->setMaximumWidth(QWIDGETSIZE_MAX); }
            b->setText(iconOnly ? QString() : text);
            if (b->toolTip().isEmpty() || b->toolTip() == text) b->setToolTip(iconOnly ? text : QString());
            const QIcon icon = b->property("compactIcon").value<QIcon>();
            if (!icon.isNull()) b->setIcon(iconOnly ? icon : QIcon());
        }
        if (compact) fitCompactSpacing();
        // The row's hint follows the shown widgets at once.
        for (auto *w : findChildren<QWidget *>()) if (w->layout()) w->layout()->activate();
        if (layout()) layout()->activate();
    }
    int m_level = -1;
    bool m_fitting = false;
};

QWidget *TeamWorkspace::toolbar(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *bar = new ToolbarRow; bar->setObjectName("teamWorkspaceToolbar");
    bar->setMinimumWidth(0); bar->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    auto *h = new QHBoxLayout(bar); h->setContentsMargins(12, 8, 12, 6); h->setSpacing(8);
    // #755 (owner, 2026-10-31): the views as icons alone; each names itself
    // and what it shows in its tooltip and to a screen reader.
    const QHash<QString, QString> purposes{{"graph", tr("Tasks and the dependencies between them")},
                                           {"board", tr("Tasks in columns by their state")},
                                           {"layers", tr("Tasks in the waves they run in")},
                                           {"profile", tr("Where the phase's time and tokens went")},
                                           {"tests", tr("The tests the team ran and their results")},
                                           {"builds", tr("The builds the team ran and their queue")}};
    auto *views = segmented({{"graph", tr("Graph")}, {"board", tr("Board")}, {"layers", tr("Layers")}, {"profile", tr("Profile")}, {"tests", tr("Tests")}}, m_view, "teamWorkspaceView",
                            [this](const QString &value) { m_view = value; m_fit = true; scheduleRender(); });
    for (auto *b : views->findChildren<QPushButton *>()) {
        const QString value = b->property("segment").toString();
        b->setIcon(workspaceIcon(QStringLiteral("view-") + value, b->isChecked() ? c.accent : c.text2)); b->setIconSize(QSize(16, 16));
        b->setAccessibleName(b->text()); b->setAccessibleDescription(purposes.value(value));
        b->setToolTip(b->text() + QLatin1Char('\n') + purposes.value(value));
        b->setText(QString()); b->setFixedWidth(b->iconSize().width() + 16);
    }
    h->addWidget(views);
    const bool planning = team.status == "planning";
    if (planning) {
        auto *task = button(tr("+ Task"), "teamWorkspaceAddTask");
        QObject::connect(task, &QPushButton::clicked, this, [this] { if (const auto *t = planTeam(m_team)) m_panel->editTask(*t, 0); });
        h->addWidget(task);
        if (m_view == "graph") {
            auto *hint = elided(tr("Drag from a node's right edge onto a task to connect them"), c.dim, 11);
            hint->setObjectName("teamWorkspaceHint"); hint->setProperty("compactHide", true); h->addWidget(hint, 1);
        }
    } else if (m_view != "board" && m_view != "profile" && m_view != "tests") {
        // The board's columns already sort tasks by state; the graph dims and
        // the layers hide what the filter leaves out.
        auto *filter = segmented({{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}, m_filter, "teamWorkspaceFilter",
                                 [this](const QString &value) { m_filter = value; scheduleRender(); });
        for (auto *b : filter->findChildren<QPushButton *>()) {
            const QString value = b->property("segment").toString();
            b->setAccessibleName(b->text()); b->setProperty("compactText", b->text()); b->setIconSize(QSize(13, 13));
            b->setProperty("compactFirst", true);
            b->setProperty("compactIcon", QVariant::fromValue(workspaceIcon(value == "problems" ? QStringLiteral("attention") : value,
                                                                              b->isChecked() ? c.accent : c.text2)));
        }
        h->addWidget(filter);
    }
    // U38: Graph and Board's heat tint.
    if (auto *heat = heatSwitch(team)) h->addWidget(heat);
    // U58: only a team with cancelled work offers to show it.
    if (const int cancelled = cancelledTasks(team)) {
        auto *show = button(m_teams->showCancelled() ? tr("Hide cancelled (%1)").arg(cancelled) : tr("Show cancelled (%1)").arg(cancelled), "teamWorkspaceShowCancelled");
        show->setCheckable(true); show->setChecked(m_teams->showCancelled()); show->setProperty("chip", true);
        show->setProperty("cancelled", cancelled);
        show->setToolTip(m_teams->showCancelled() ? tr("Cancelled tasks and the phases they emptied are shown") : tr("Cancelled tasks and the phases they emptied are hidden"));
        QObject::connect(show, &QPushButton::clicked, this, [this](bool on) { m_teams->setShowCancelled(on); });
        h->addWidget(show);
    }
    if (!m_focus.isEmpty()) {
        auto *chip = button(tr("Member: %1  ×").arg(team.roleOf(m_focus)), "teamWorkspaceFocus"); chip->setProperty("chip", true);
        chip->setToolTip(tr("Show every member again"));
        QObject::connect(chip, &QPushButton::clicked, this, [this] { m_focus.clear(); scheduleRender(); });
        h->addWidget(chip);
    }
    if (!planning || m_view != "graph") h->addStretch();
    const auto groups = phaseGroups(team);
    if (!groups.isEmpty() && !planning && m_view != "profile" && m_view != "tests") {
        // The drafts waiting for the owner; a click opens the first.
        int drafts = 0, first = 0;
        for (const auto &group : groups) {
            const QString state = ownerPhaseState(team, group);
            // U61: an auto draft does not wait for you; Auto accepts it at its first hand-out.
            if (const auto *record = team.phase(group.n); record && record->autoDraft()) continue;
            if (state == "draft" || state == "rejected") { ++drafts; if (!first) first = group.n; }
        }
        if (drafts) {
            auto *count = button(QString::number(drafts), "teamWorkspaceDrafts"); count->setProperty("draftCount", drafts);
            count->setIcon(workspaceIcon("pencil", c.folder)); count->setIconSize(QSize(11, 11));
            count->setToolTip(drafts == 1 ? tr("1 phase draft") : tr("%1 phase drafts").arg(drafts));
            count->setStyleSheet(QStringLiteral("QPushButton#teamWorkspaceDrafts{background:%1;color:%2;border:1px solid %3;border-radius:10px;padding:1px 7px;font-size:11px;font-weight:600;}")
                                     .arg(alpha(c.folder, .13).name(QColor::HexArgb), c.folder.name(), alpha(c.folder, .6).name(QColor::HexArgb)));
            QObject::connect(count, &QPushButton::clicked, this, [this, first] { showPhase(first); });
            h->addWidget(count);
        }
        // Owner, 2026-10-30: hide the phases whose tasks are all decided.
        if (const int finished = finishedPhases(team); finished || m_hideFinished) {
            auto *hideSwitch = new ToggleSwitch(c); hideSwitch->setObjectName("teamWorkspaceHideFinished"); hideSwitch->setChecked(m_hideFinished);
            hideSwitch->setProperty("compactDrop", true);
            hideSwitch->setAccessibleName(tr("Hide finished"));
            hideSwitch->setToolTip(finished == 1 ? tr("1 phase has every task decided") : tr("%1 phases have every task decided").arg(finished));
            QObject::connect(hideSwitch, &QAbstractButton::clicked, this, [this](bool on) {
                m_hideFinished = on;
                Settings::set("teams/hideFinished", on);
                scheduleRender();
            });
            h->addWidget(hideSwitch);
            auto *hideLabel = label(tr("Hide finished"), c.text3, 11); hideLabel->setProperty("compactHide", true);
            h->addWidget(hideLabel);
        }
        auto *autoSwitch = new ToggleSwitch(c); autoSwitch->setObjectName("teamWorkspaceAutoAccept"); autoSwitch->setChecked(team.autoAccept);
        autoSwitch->setAccessibleName(tr("Auto-accept drafts"));
        autoSwitch->setToolTip(autoAcceptSwitchTip(team.autoAccept));
        autoSwitch->setEnabled(settingsOpen(team));
        QObject::connect(autoSwitch, &QAbstractButton::clicked, this, [this, id = team.id](bool on) { if (const auto *t = currentTeam(id)) setAutoAccept(*t, on); });
        h->addWidget(autoSwitch);
        auto *autoLabel = label(tr("Auto-accept drafts"), c.text3, 11); autoLabel->setProperty("compactHide", true);
        h->addWidget(autoLabel);
        auto *aheadSwitch = new ToggleSwitch(c); aheadSwitch->setObjectName("teamWorkspaceWorkAhead"); aheadSwitch->setChecked(team.workAhead);
        aheadSwitch->setAccessibleName(tr("Work ahead"));
        aheadSwitch->setToolTip(tr("On: an idle member may take an independent task of the next draft phase; its report waits for your approval"));
        aheadSwitch->setEnabled(settingsOpen(team));
        QObject::connect(aheadSwitch, &QAbstractButton::clicked, this, [this, id = team.id](bool on) { if (const auto *t = currentTeam(id)) setWorkAhead(*t, on); });
        h->addWidget(aheadSwitch);
        auto *aheadLabel = label(tr("Work ahead"), c.text3, 11); aheadLabel->setProperty("compactHide", true);
        h->addWidget(aheadLabel);
    }
    if (!groups.isEmpty() && m_view != "profile" && m_view != "tests") {
        bool allOpen = true;
        for (const auto &group : groups) allOpen = allOpen && m_panel->phaseOpen(team, group);
        auto *expand = button(allOpen ? tr("Collapse all") : tr("Expand all"), "teamWorkspaceExpandAll"); expand->setProperty("ghost", true);
        expand->setProperty("compactText", expand->text()); expand->setAccessibleName(expand->text());
        expand->setIcon(workspaceIcon(allOpen ? "chevron-down" : "chevron-right", c.text2)); expand->setIconSize(QSize(11, 11));
        QObject::connect(expand, &QPushButton::clicked, this, [this, id = team.id, open = !allOpen] {
            const auto *current = currentTeam(id);
            if (!current) return;
            for (const auto &group : phaseGroups(*current)) if (m_panel->phaseOpen(*current, group) != open) m_panel->togglePhase(*current, group.n);
            scheduleRender();
        });
        h->addWidget(expand);
    }
    if (m_view == "graph") {
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); m_zoomManual = true; scheduleRender(); };
        auto *zoom = new QWidget; zoom->setProperty("seg", true); zoom->setAttribute(Qt::WA_StyledBackground);
        zoom->setObjectName("teamWorkspaceZoom"); zoom->setProperty("compactDrop", true);
        auto *zh = new QHBoxLayout(zoom); zh->setContentsMargins(2, 2, 2, 2); zh->setSpacing(0);
        auto *out = button(QStringLiteral("−"), "teamWorkspaceZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        // "Fit" until the owner zooms, then the zoom itself; a click fits again.
        auto *fit = button(m_zoomManual ? QStringLiteral("%1%").arg(int(std::round(m_zoom * 100))) : tr("Fit"), "teamWorkspaceFit");
        QObject::connect(fit, &QPushButton::clicked, this, [this] { m_fit = true; m_zoomManual = false; scheduleRender(); });
        fit->setProperty("compactText", fit->text()); fit->setAccessibleName(fit->text()); fit->setIconSize(QSize(12, 12));
        fit->setProperty("compactIcon", QVariant::fromValue(workspaceIcon(QStringLiteral("reset"), c.text2)));
        auto *in = button(QStringLiteral("+"), "teamWorkspaceZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        fit->setToolTip(tr("Zoom %1% · click to fit").arg(int(std::round(m_zoom * 100))));
        zh->addWidget(out); zh->addWidget(fit); zh->addWidget(in);
        h->addWidget(zoom);
    }
    return bar;
}

// U25 part 5 (spec 5.4): a draft task gains or loses a dependency with
// edit-draft, the task's whole new list.
void TeamWorkspace::linkDraft(const Team &team, int dependency, int taskId, bool add)
{
    const auto *task = team.task(taskId);
    if (!task || task->status != "draft") return;
    QJsonArray deps;
    for (const auto &d : task->deps)
        if (d.task != dependency) deps.append(QJsonObject{{"task", d.task}, {"kind", d.soft ? "soft" : "hard"}});
    if (add) deps.append(QJsonObject{{"task", dependency}, {"kind", "hard"}});
    m_panel->act(QStringLiteral("edit-draft"), {{"phase", task->phase}, {"revision", team.revision},
                                                {"tasks_update", QJsonArray{QJsonObject{{"id", taskId}, {"deps", deps}}}}});
}

QWidget *TeamWorkspace::canvas(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    // U38 (#665): the Profile view stays across renders; it reads in the background.
    if (m_view == "profile") return profileView(team);
    // U62 (#719): the Tests view too; it reads the team's test database.
    if (m_view == "tests") return testsView(team);
    // U25: a team with phases shows each phase as its own scroll, in every view.
    if (!phaseGroups(team).isEmpty()) return phaseColumn(team);
    if (m_view == "board") return board(team);
    // A team with phases never gets here: no bands, no other phases, no drafts.
    auto *view = new TaskView(c, m_view == "graph" ? TaskView::Wide : TaskView::List);
    view->setObjectName(m_view == "graph" ? "teamWorkspaceGraph" : "teamWorkspaceLayers");
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    // N15: Show on graph lights the blockers here too, without phases.
    if (m_showBlockers) view->emphasis = blockers(team);
    for (const auto &task : team.tasks)
        for (const auto &d : task.deps) {
            view->edges[task.id].append(d.task);
            if (d.soft && !team.allHard) view->soft.insert({d.task, task.id});
        }
    QList<int> order;
    const auto byLayer = layered(team);
    for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    graphNodes(view, team, order, {});
    // Planning edits the plan here as in the inspector: a click edits the task,
    // a drag from a node's handle adds a dependency, a selected edge goes.
    if (team.status == "planning") {
        view->editable = true;
        view->clicked = [this](int id, const QPoint &) { if (const auto *t = planTeam(m_team)) m_panel->editTask(*t, id); };
        view->linked = [this](int dependency, int task) { if (const auto *t = planTeam(m_team)) m_panel->link(*t, dependency, task, true); };
        view->unlinked = [this](int dependency, int task) { if (const auto *t = planTeam(m_team)) m_panel->link(*t, dependency, task, false); };
    }
    if (team.status != "planning") view->clicked = [this](int id, const QPoint &) {
        const auto *team = currentTeam(m_team);
        if (team) if (const auto *task = team->task(id); task && task->subteam.known && !task->subteam.team.isEmpty()) {
            m_teams->watch(task->subteam.team);
            showTeam(task->subteam.team);
            return;
        }
        m_drawerTask = id; scheduleRender();
    };
    keepGraphEdit(view, team.id);
    auto *area = new GraphArea; area->setObjectName("teamWorkspaceCanvas");
    area->setFrameShape(QFrame::NoFrame);
    // Fit starts the graph afresh; otherwise it stays where it was scrolled.
    if (m_fit) m_scroll.remove(QStringLiteral("canvas/") + m_view);
    keepScroll(area, QStringLiteral("canvas/") + m_view);
    area->setWidget(view);
    area->setWidgetResizable(m_view != "graph");
    area->setStyleSheet(QStringLiteral("QScrollArea#teamWorkspaceCanvas,QScrollArea#teamWorkspaceCanvas>QWidget#qt_scrollarea_viewport{background:%1;border:0;}").arg(c.surface.name()));
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

// U25 part 5 (owner, 2026-10-27): the small activity label a graph node, board
// card and layers row carry right of the role chip and model tile: what happens
// now. A problem text wins over it (the callers check that first). Owner,
// 2026-10-28: a task in work always says where its work is - starting (handed
// over, not begun), working (its agent runs) or idle (the turn ended and the
// report has not come) - with "attempt N" first from the second attempt.
// Owner, 2026-10-29: every task says its state in words, not only by its icon:
// accepted, rejected, draft, and a waiting task what it waits for (after #N,
// for a member, the lead or you), "returned" first when it came back.
static QString waitLabel(const Teams::Task &task)
{
    if (!task.wait.known || task.wait.on.isEmpty()) return QString();
    QStringList on = task.wait.on;
    if (task.wait.for_ == QLatin1String("deps")) {
        for (auto &item : on) if (!item.startsWith('#')) item.prepend('#');
        return TeamWorkspace::tr("after %1").arg(on.join(QStringLiteral(", ")));
    }
    if (task.wait.for_ == QLatin1String("owner")) return TeamWorkspace::tr("waits for you");
    return TeamWorkspace::tr("waits for %1").arg(on.join(QStringLiteral(", ")));
}

static QString activityLabel(const Teams::Task &task, const Teams::Live &live = {})
{
    if (task.status == QLatin1String("submitted")) return TeamWorkspace::tr("review");
    // U36: the stage chip says what the stage does; the card names the attempt.
    if (task.inStage()) return TeamWorkspace::tr("attempt %1").arg(task.last() ? task.last()->n : 1);
    // Owner, 2026-10-28: a cancelled task says so; its icon alone was missed.
    if (task.status == QLatin1String("cancelled")) return TeamWorkspace::tr("cancelled");
    if (task.subteam.known && !task.subteam.team.isEmpty())
        return TeamWorkspace::tr("sub-team %1/%2").arg(task.subteam.done).arg(task.subteam.total);
    if (task.status == QLatin1String("in_progress") || task.status == QLatin1String("assigned")) {
        const auto *attempt = task.last();
        const int n = attempt ? attempt->n : 0;
        const QString state = !attempt || task.status == QLatin1String("assigned") ? TeamWorkspace::tr("starting")
                            : attempt->startedAt <= 0 && attempt->turnEndedAt <= 0 && !live.working ? TeamWorkspace::tr("starting")
                            : live.working ? TeamWorkspace::tr("working") : TeamWorkspace::tr("idle");
        if (n > 1) return TeamWorkspace::tr("attempt %1 · %2").arg(n).arg(state);
        return state;
    }
    if (task.held) return TeamWorkspace::tr("held");
    if (task.status == QLatin1String("accepted")) return TeamWorkspace::tr("accepted");
    if (task.status == QLatin1String("rejected")) return TeamWorkspace::tr("rejected");
    if (task.status == QLatin1String("draft")) return TeamWorkspace::tr("draft");
    if (task.status == QLatin1String("pending")) {
        const QString wait = waitLabel(task);
        if (!task.attempts.isEmpty()) return wait.isEmpty() ? TeamWorkspace::tr("returned") : TeamWorkspace::tr("returned · %1").arg(wait);
        return wait.isEmpty() ? TeamWorkspace::tr("pending") : wait;
    }
    return QString();
}

void TeamWorkspace::graphNodes(TaskView *view, const Team &team, const QList<int> &order, const QSet<int> &phase)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    const auto problemTone = problemTasks(team);
    // The short problem a node shows on its second line, as Board and Layers name it.
    const auto problemText = problemTexts(team);
    const bool grouped = !phase.isEmpty();
    const double now = workedClock()();
    const Heat *heatOn = heat(team);
    for (int id : order) {
        const auto *task = team.task(id);
        if (!task) continue;
        const auto status = taskStatus(*task);
        TaskView::Node node;
        node.id = id; node.phase = task->phase; node.layer = team.layers.value(id);
        node.title = task->title;
        node.role = team.assigneeRole(*task, tr("You"));
        node.problem = problemTone.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status); node.statusText = status.text; node.roleTone = roleColor(node.role, m_dark);
        if (task->subteam.known && !task->subteam.team.isEmpty()) {
            // G1: a delegated task is the group node for its sub-team.
            node.subteam = task->subteam.team;
            node.kind = QStringLiteral("subteam");
            node.statusText = tr("Sub-team \u00b7 %1/%2").arg(task->subteam.done).arg(task->subteam.total);
        }
        node.draft = task->status == "draft"; node.error = team.cycle.contains(id);
        node.late = task->addedLate.known;
        node.attempt = task->last() ? task->last()->n : 0;
        const auto *assignee = team.member(task->assignee);
        if (assignee) node.agent = assignee->agent;
        // The dependencies from other phases on the first line ("↤#1"); the
        // ones inside the phase are its edges.
        QStringList external;
        for (const auto &d : task->deps) if (grouped && !phase.contains(d.task)) external << QStringLiteral("#%1").arg(d.task);
        if (!external.isEmpty()) node.ext = QStringLiteral("↤") + external.join(' ');
        // A plan or draft task without an assignee is an error on its node.
        const bool unassigned = (team.status == "planning" || node.draft) && !assignee;
        if (unassigned) { node.error = true; node.problemText = tr("No assignee"); node.tone = c.danger; }
        else if (node.problem) node.problemText = problemText.value(id);
        // The owner's activity label; a problem text wins over it.
        if (!unassigned && !node.problem) node.activity = activityLabel(*task, assignee ? live(*assignee) : Live{});
        // U37: the working time after it; the tooltip breaks it down.
        // #746: against its estimate, over it in the warning color.
        const auto time = taskTime(*task, now);
        node.worked = time.text; node.timeOver = time.over; node.runningSince = task->timing.runningSince;
        node.estimate = task->estimateMinutes; node.estimateKnown = task->estimateKnown();
        // Spec 1.4: the dots under the model tile while its agent really runs
        // this task.
        node.working = assignee && task->status == "in_progress" && !node.problem && assignee->state != "paused"
                       && team.status == "running" && !live(*assignee).offline && !live(*assignee).attention;
        // The full status lives in the tooltip: "#id Title — Status · after
        // #x (earlier phase) · Role · model"; A1's attempt phase follows.
        QString tip = QStringLiteral("#%1 %2 — %3").arg(id).arg(task->title, unassigned ? tr("No assignee") : status.text);
        if (!external.isEmpty()) tip += tr(" · after %1 (earlier phase)").arg(external.join(QStringLiteral(", ")));
        if (assignee) tip += QStringLiteral(" · %1 · %2").arg(team.roleOf(assignee->id), assignee->model.isEmpty() ? agentNames.value(assignee->agent, assignee->agent) : assignee->model);
        const auto mark = phaseMark(team, *task);
        if (!mark.tooltip.isEmpty() && mark.tooltip != status.text) tip += QLatin1Char('\n') + mark.tooltip;
        if (node.late) tip += QLatin1Char('\n') + addedLateTip(*task);
        if (const QString worked = workedTip(*task, now); !worked.isEmpty()) tip += QLatin1Char('\n') + worked;
        if (const QString estimate = estimateTip(*task); !estimate.isEmpty()) tip += QLatin1Char('\n') + estimate;
        node.tooltip = tip;
        // U38: the heat tint, its loss and top loss.
        if (heatOn) applyHeat(node, *heatOn, *task, c);
        if (!grouped) {
            QStringList deps;
            for (const auto &d : task->deps) deps << QStringLiteral("#%1").arg(d.task);
            node.deps = deps.isEmpty() ? QString() : QStringLiteral("← ") + deps.join(' ');
        }
        const bool active = QStringList{"assigned", "in_progress", "submitted", "in_review", "in_test"}.contains(task->status);
        node.match = m_showBlockers ? view->emphasis.contains(id)
                   : (m_filter == "all" || team.status == "planning" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem))
                     && (m_focus.isEmpty() || task->assignee == m_focus);
        view->nodes.append(node);
    }
}

void TeamWorkspace::keepGraphEdit(TaskView *view, const QString &team)
{
    // #570: every poll rebuilds the views; the task under the resting pointer
    // stays lit with its linked tasks until the pointer moves or leaves, or
    // the task is gone.
    if (const int hovered = m_graphHover.value(team)) {
        const auto *current = currentTeam(team);
        if (current && current->task(hovered)) view->keepHover(hovered);
        else m_graphHover.remove(team);
    }
    view->hoverChanged = [this, team](int from, int to) {
        if (to) m_graphHover.insert(team, to);
        else if (m_graphHover.value(team) == from) m_graphHover.remove(team);
    };
    if (!view->editable && view->editableTasks.isEmpty()) return;
    const auto kept = m_graphEdit.value(team);
    view->keepEdit(kept.first, kept.second);
    view->editChanged = [this, team](int connectFrom, QPair<int, int> edge) {
        if (!connectFrom && !edge.first) m_graphEdit.remove(team);
        else m_graphEdit.insert(team, {connectFrom, edge});
    };
}

// U37: the board's and the layers' running times tick on the shared minute
// timer; only their labels change, nothing is rebuilt.
static QWidget *tickingBoard(QWidget *board)
{
    QObject::connect(&workedTicker(), &QTimer::timeout, board, [board] {
        const double now = workedClock()();
        for (auto *label : board->findChildren<QLabel *>()) {
            if (label->objectName() != QLatin1String("teamBoardWorked") && label->objectName() != QLatin1String("teamLayerWorked")) continue;
            const double since = label->property("runningSince").toDouble();
            if (since <= 0) continue;
            // #746: against the estimate the label was built with.
            const auto time = estimateTime(label->property("estimate").toDouble(), label->property("estimateKnown").toBool(),
                                           qint64(qMax(0.0, now - since)), true);
            label->setText(time.text);
            if (time.over != label->property("over").toBool()) {
                label->setProperty("over", time.over);
                QPalette pal = label->palette();
                pal.setColor(QPalette::WindowText, colorsFor(label->property("dark").toBool()).warn);
                if (time.over) label->setPalette(pal);
            }
        }
    });
    return board;
}

// U37: a task's working time right after its activity label, in the same color
// and size; a running one keeps the room it grows to and ticks with its view.
static QLabel *workedWidget(const Task &task, double now, const Colors &c, const char *name)
{
    // #746: the time against the estimate; over it, the warning color.
    const auto time = taskTime(task, now);
    if (time.text.isEmpty()) return nullptr;
    auto *w = label(time.text, time.over ? c.warn : c.text3, 11, false, name);
    w->setProperty("runningSince", task.timing.runningSince);
    w->setProperty("estimate", task.estimateMinutes); w->setProperty("estimateKnown", task.estimateKnown());
    w->setProperty("over", time.over); w->setProperty("dark", c.dark);
    if (task.timing.runningSince > 0) w->setMinimumWidth(estimateRoom(w->fontMetrics(), task.estimateMinutes, task.estimateKnown(), task.timing.runningSince));
    QStringList tip{workedTip(task, now), estimateTip(task)};
    tip.removeAll(QString());
    w->setToolTip(tip.join(QLatin1Char('\n')));
    return w;
}

static void addWorked(QHBoxLayout *row, const Task &task, double now, const Colors &c, const char *name)
{
    if (auto *w = workedWidget(task, now, c, name)) row->addWidget(w);
}

QWidget *TeamWorkspace::board(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QSet<int> problemTasks;
    for (const auto &p : problems(team, live)) if (p.task) problemTasks.insert(p.task);
    for (const auto &task : team.tasks) if (taskStatus(task).problem) problemTasks.insert(task.id);
    const auto groups = phaseGroups(team);
    const double now = workedClock()();
    const Heat *heatOn = heat(team);
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
            cardBox->setProperty("lane", task.lane.n);
            // U27: the card's menu moves it to another lane (planning and drafts).
            cardBox->setContextMenuPolicy(Qt::CustomContextMenu);
            QObject::connect(cardBox, &QWidget::customContextMenuRequested, this, [this, id = team.id, card = cardBox, taskId = task.id](const QPoint &at) {
                if (const auto *fresh = currentTeam(id)) laneMenu(*fresh, taskId, card->mapToGlobal(at));
            });
            auto *b = new QVBoxLayout(cardBox); b->setContentsMargins(8, 6, 8, 6); b->setSpacing(3);
            auto *title = label(QStringLiteral("#%1 %2").arg(task.id).arg(task.title), c.fg, 12, true); title->setWordWrap(true); b->addWidget(title);
            // Role, agent, state and dependencies share a line while the column
            // is wide enough and wrap when it is not.
            auto *r = new FlowLayout(6);
            const QString role = team.assigneeRole(task, tr("You"));
            auto *pill = new Pill(role, roleColor(role, m_dark)); pill->setObjectName("teamBoardRole"); r->addWidget(pill);
            if (const auto *m = team.member(task.assignee)) { auto *i = new QLabel; i->setPixmap(agentLogo(m->agent, c.text3).pixmap(14, 14)); r->addWidget(i); }
            // The same small activity label as the graph (a problem text wins).
            if (!problemTasks.contains(task.id)) {
                const auto *member = team.member(task.assignee);
                const QString activity = activityLabel(task, member ? live(*member) : Live{});
                if (!activity.isEmpty()) {
                    auto *a = label(activity, c.text3, 11); a->setObjectName("teamBoardActivity"); a->setProperty("liveTask", task.id); r->addWidget(a);
                }
            }
            // U38: the heat tint and its loss in the working time's place.
            if (auto *lost = heatOn ? heatBoardLabel(*heatOn, task, c) : nullptr) {
                const QColor edge = problemTasks.contains(task.id) ? c.danger : c.border;
                cardBox->setStyleSheet(QStringLiteral("QFrame#teamBoardCard{background:%1;border:1px solid %2;border-radius:9px;}")
                                           .arg(heatCardFill(heatOn, task, c).name(), edge.name()));
                cardBox->setToolTip(lost->toolTip());
                r->addWidget(lost);
            }
            // U37: the working time right after it, in the same color and size;
            // a running one keeps the room it grows to and ticks with the board.
            else if (auto *w = workedWidget(task, now, c, "teamBoardWorked")) r->addWidget(w);
            const auto mark = phaseMark(team, task);
            auto *state = new StatusMark(s.text, toneColor(c, mark.tone != Tone::Neutral ? mark.tone : s.tone),
                                         mark.kind.isEmpty() ? kindOfTask(task, s) : mark.kind, 0, Qt::AlignLeft, nullptr, mark.tooltip);
            state->setObjectName("teamBoardState"); r->addWidget(state);
            QStringList deps; for (const auto &d : task.deps) deps << QStringLiteral("#%1").arg(d.task);
            if (!deps.isEmpty()) r->addWidget(label(QStringLiteral("← ") + deps.join(' '), c.dim, 10));
            b->addLayout(r);
            // U36: the stage chip line.
            if (auto *stages = stageLineWidget(team, task, c)) b->addWidget(stages);
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
        scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        if (groups.isEmpty()) scroll->setProperty("phase", 0);
        else { scroll->setProperty("phase", phase); scroll->setMinimumHeight(150); }
        keepScroll(scroll, groups.isEmpty() ? QStringLiteral("board/") + key + QLatin1Char('/') + m_focus
                                            : QStringLiteral("board/%1/%2/%3").arg(phase).arg(key).arg(m_focus));
        cv->addWidget(scroll, 1);
        return col;
    };
    const auto makeRow = [&](const QList<int> &ids, int phase) -> QWidget * {
        auto *rowBox = new QWidget; rowBox->setObjectName("teamBoardRow"); auto *h = new QHBoxLayout(rowBox);
        h->setContentsMargins(12, 4, 12, 8); h->setSpacing(8);
        for (const auto &[key, name] : QList<std::pair<QString, QString>>{{"pending", tr("Pending")}, {"active", tr("Active")}, {"problems", tr("Problems")},
                                                                         {"submitted", tr("Submitted")}, {"in_review", tr("In review")}, {"in_test", tr("In test")},
                                                                         {"decided", tr("Decided")}, {"accepted", tr("Accepted")}}) {
            // U36: the stage columns once the team uses pipelines.
            if ((key == "in_review" || key == "in_test") && !usesPipelines(team)) continue;
            // Decided only when a task is there, as on the phases' Board.
            if (key == "decided" && std::none_of(team.tasks.cbegin(), team.tasks.cend(), [&](const Task &task) {
                    return column(task, problemTasks.contains(task.id)) == "decided"; })) continue;
            auto *column = makeColumn(key, name, ids, phase);
            readableColumn(column);
            h->addWidget(column, 1);
        }
        return rowBox;
    };
    if (groups.isEmpty()) {
        // U33: columns keep their readable width; a narrow window scrolls the
        // whole board sideways (each column still scrolls its own cards).
        auto *row = makeRow([&] { QList<int> ids; for (const auto &task : team.tasks) ids.append(task.id); return ids; }(), 0);
        auto *area = new QScrollArea; area->setObjectName("teamWorkspaceBoard");
        area->setWidgetResizable(true); area->setFrameShape(QFrame::NoFrame);
        area->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        area->setStyleSheet(QStringLiteral("QScrollArea#teamWorkspaceBoard{background:transparent;} QScrollArea#teamWorkspaceBoard > QWidget > QWidget#teamBoardRow{background:transparent;}")
                            + workspaceScrollbars(m_dark));
        area->viewport()->setAutoFillBackground(false);
        area->setWidget(row);
        new SideBarFit(area, row);
        keepScroll(area, QStringLiteral("board/row/") + m_focus);
        return tickingBoard(area);
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
                // #709 L4: its minutes grow in place between rebuilds.
                agedHealth(health, summary->healthLine, tip.isEmpty() ? summary->healthLine : tip);
                h->addWidget(health);
            }
        }
        if (group.draft) {
            auto *approve = button(tr("Approve"), "teamBoardApprovePhase", true);
            // F12/N40: a draft behind an approved phase that still runs stays
            // approvable; the warning comes on the click.
            const int alongside = alongsidePhase(team, group.n);
            const auto *record = team.phase(group.n);
            const bool blocked = !approveBlocked(team, group.n).isEmpty() || (!alongside && record && !record->waiting.text.isEmpty());
            approve->setEnabled(!blocked);
            approve->setToolTip(alongside ? (group.barrier ? tr("Approve phase %1 now; its tasks start when phase %2 is finished")
                                                           : tr("Approve runs phase %1 now, alongside phase %2"))
                                            .arg(group.n).arg(alongside)
                                          : approveBlocked(team, group.n));
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
    return tickingBoard(scroll);
}

// U25: the tasks with a problem (the team's problems and each task's own
// status) and the tone each shows.
QMap<int, Tone> TeamWorkspace::problemTasks(const Team &team) const
{
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QMap<int, Tone> found;
    for (const auto &p : problems(team, live)) if (p.task) found.insert(p.task, p.tone);
    for (const auto &task : team.tasks) if (const auto s = taskStatus(task); s.problem && !found.contains(task.id)) found.insert(task.id, s.tone);
    return found;
}

QString TeamWorkspace::problemText(const Team &team, int id) const
{
    return problemTexts(team).value(id);
}

QMap<int, QString> TeamWorkspace::problemTexts(const Team &team) const
{
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QMap<int, QString> found;
    for (const auto &p : problems(team, live)) {
        if (!p.task || found.contains(p.task)) continue;
        // "#6 Unit tests — Tester · Delivery unconfirmed": the reason after the dot.
        const int dot = p.text.lastIndexOf(QStringLiteral(" · "));
        found.insert(p.task, dot >= 0 ? p.text.mid(dot + 3) : p.text);
    }
    for (const auto &task : team.tasks) if (const auto s = taskStatus(task); s.problem && !found.contains(task.id)) found.insert(task.id, s.text);
    return found;
}

// Team stuck's blockers: the open tasks the problems name.
QSet<int> TeamWorkspace::blockers(const Team &team) const
{
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    QSet<int> found;
    for (const auto &p : problems(team, live)) if (p.task) found.insert(p.task);
    return found;
}

void TeamWorkspace::setAutoAccept(const Team &team, bool on)
{
    // N13 (#383): only the switch's own field; the server merges it, so a
    // lead's change after the last poll stays.
    if (!settingsOpen(team)) return;
    m_panel->act("settings", {{"settings", QJsonObject{{"auto_accept", on}}}});
}

void TeamWorkspace::setWorkAhead(const Team &team, bool on)
{
    if (!settingsOpen(team)) return;
    m_panel->act("settings", {{"settings", QJsonObject{{"work_ahead", on}}}});
}

// Spec 5.2: Reject… sends the draft back to the lead with the owner's words.
void TeamWorkspace::rejectPhase(const QString &id, int phase)
{
    const QString comment = m_rejectText.trimmed();
    const auto *team = currentTeam(id);
    if (!team || comment.isEmpty() || m_phaseAct || phaseDecided(*team, phase)) return;
    // N14: the words go only to the phase they were written for.
    if (m_rejectPhase != rejectKey(*team, phase)) { m_rejectPhase.clear(); m_rejectText.clear(); scheduleRender(); return; }
    const QString key = markPhaseDecided(*team, phase);
    m_rejectPhase.clear(); m_rejectText.clear();
    // The server refuses it when the drafts were renumbered since (F11).
    m_panel->act("reject-phase", {{"team", id}, {"phase", phase}, {"comment", comment.left(2000)}, {"revision", team->revision}},
                 [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
    scheduleRender();
}

QWidget *TeamWorkspace::phaseColumn(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *area = new QScrollArea;
    area->setObjectName(m_view == "board" ? "teamWorkspaceBoard" : m_view == "layers" ? "teamWorkspaceLayers" : "teamWorkspacePhases");
    area->setProperty("view", m_view);
    area->setWidgetResizable(true); area->setFrameShape(QFrame::NoFrame);
    // U33: the Board's columns keep their readable width, so a narrow window
    // scrolls every phase sideways together (one page, the thin bar of spec
    // 1.2); Graph and Layers fit the column as before.
    const bool boardView = m_view == "board";
    area->setHorizontalScrollBarPolicy(boardView ? Qt::ScrollBarAsNeeded : Qt::ScrollBarAlwaysOff);
    // The column is the window's surface: a palette alone loses to the
    // window's style sheet, which leaves Qt's default grey around the phases.
    area->setStyleSheet(QStringLiteral("QScrollArea#%1,QScrollArea#%1>QWidget#qt_scrollarea_viewport,QWidget#teamPhaseScrolls{background:%2;border:0;}")
                            .arg(area->objectName(), c.surface.name()) + (boardView ? workspaceScrollbars(m_dark) : QString()));
    auto *content = new QWidget; content->setObjectName("teamPhaseScrolls");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(12, 4, 12, 8); v->setSpacing(8);
    const auto groups = phaseGroups(team);
    // Opened or Fit: the widest phase graph fits the column, 80-115 %.
    if (m_view == "graph" && m_fit) {
        const int room = qMax(320, (m_left ? m_left->width() : width() - 440) - 70);
        double zoom = 1.15;
        for (const auto &group : groups) {
            if (!m_panel->phaseOpen(team, group)) continue;
            const int columns = TaskView::wideColumns(phaseLayers(team, group.tasks));
            zoom = qMin(zoom, room / (12.0 + columns * 236.0));
        }
        m_zoom = qBound(0.8, zoom, 1.15); m_fit = false;
    }
    for (const auto &group : groups) v->addWidget(phaseScroll(team, group));
    v->addStretch();
    area->setWidget(content);
    if (boardView) { new SideBarFit(area, content); new PinnedPhaseHeaders(area, content); }
    keepScroll(area, QStringLiteral("phases/") + m_view);
    return area;
}

QWidget *TeamWorkspace::phaseScroll(const Team &team, const PhaseGroup &group)
{
    const Colors c = colorsFor(m_dark);
    // U59: the open free phase is running while its tasks run and idle when its
    // queue is empty, never done.
    const bool freeOpen = freeOpenPhase(team, group.n);
    const QString state = freeOpen ? (team.freePhase.idle() ? QStringLiteral("idle") : QStringLiteral("running")) : ownerPhaseState(team, group);
    const bool open = m_panel->phaseOpen(team, group);
    const Phase *record = team.phase(group.n);
    // U61: Auto accepts this draft at its first hand-out; it does not wait for you.
    const bool autoDraft = !freeOpen && state == "draft" && record && record->autoDraft();
    const auto problemsByTask = problemTasks(team);
    auto *frame = new PhaseFrame(c, state); frame->setProperty("phase", group.n); frame->expanded = open;
    auto *fv = new QVBoxLayout(frame); fv->setContentsMargins(1, 1, 1, 1); fv->setSpacing(0);
    auto *head = new PhaseHeader; head->setObjectName("teamPhaseHeader"); head->setProperty("phase", group.n);
    head->setMinimumHeight(40); head->setCursor(Qt::PointingHandCursor);
    head->setToolTip(open ? tr("Collapse the phase") : tr("Expand the phase"));
    const auto toggle = [this, id = team.id, phase = group.n] {
        if (const auto *current = currentTeam(id)) m_panel->togglePhase(*current, phase);
        scheduleRender();
    };
    head->clicked = toggle;
    frame->header = head;
    auto *h = new QHBoxLayout(head); h->setContentsMargins(6, 3, 10, 3); h->setSpacing(9);
    auto *chevron = new QToolButton; chevron->setObjectName("teamPhaseToggle"); chevron->setProperty("phase", group.n);
    chevron->setProperty("collapsed", !open); chevron->setAutoRaise(true); chevron->setCursor(Qt::PointingHandCursor);
    chevron->setIcon(workspaceIcon(open ? "chevron-down" : "chevron-right", c.muted)); chevron->setIconSize(QSize(12, 12));
    chevron->setAccessibleName(tr("Phase %1").arg(group.n)); chevron->setAccessibleDescription(open ? tr("Expanded") : tr("Collapsed"));
    QObject::connect(chevron, &QToolButton::clicked, this, toggle);
    h->addWidget(chevron);
    const QColor tone = phaseStateColor(c, state);
    QColor numberTone = c.fg, numberFill = c.hover;
    if (state == "draft" || state == "planned") { numberTone = c.folder; numberFill = alpha(c.folder, .25); }
    else if (state == "running") { numberTone = c.accent; numberFill = alpha(c.accent, .10); }
    else if (state == "done") { numberTone = c.accentMuted; numberFill = alpha(c.accent, .10); }
    else if (state == "rejected") { numberTone = c.warn; numberFill = alpha(c.warn, .11); }
    h->addWidget(new PhaseNumber(group.n, numberTone, numberFill));
    const QString name = group.title.isEmpty() ? tr("Phase %1").arg(group.n) : tr("Phase %1 · %2").arg(group.n).arg(group.title);
    auto *title = new ShrinkLabel(name, 90); title->setObjectName("teamPhaseTitle");
    {QFont f = title->font(); f.setPixelSize(13); f.setWeight(QFont::DemiBold); title->setFont(f);
     QPalette pal = title->palette(); pal.setColor(QPalette::WindowText, c.fg); title->setPalette(pal);}
    title->setAttribute(Qt::WA_TransparentForMouseEvents);
    h->addWidget(title);
    const QString kind = state == "draft" || state == "planned" ? QStringLiteral("draft") : state == "running" ? QStringLiteral("running")
                       : state == "done" ? QStringLiteral("accepted") : state == "rejected" ? QStringLiteral("rejected") : QStringLiteral("pending");
    auto *badge = freeOpen ? new PhaseBadge(state == "idle" ? tr("Open, idle") : tr("Open, running"), state == "idle" ? c.text2 : tone,
                                            state == "idle" ? QStringLiteral("delivered") : QStringLiteral("running"), false)
                           : new PhaseBadge(autoDraft ? autoDraftBadge() : phaseStateBadge(state), tone, kind, state == "draft" || state == "planned");
    badge->setObjectName("teamPhaseBadge"); badge->setProperty("state", state); badge->setProperty("auto", autoDraft);
    if (autoDraft) badge->setToolTip(autoDraftTip(record->startAhead));
    h->addWidget(badge);
    // U61: the lead let idle members start this auto draft early; a phase
    // Auto accepted at its first hand-out says so.
    if (autoDraft && record->startAhead) {
        auto *ahead = label(tr("started ahead by the lead"), c.accent, 11, true, "teamPhaseStartAhead");
        ahead->setToolTip(autoDraftTip(true)); h->addWidget(ahead);
    } else if (!freeOpen && record && record->acceptedByAuto()) {
        auto *byAuto = label(tr("accepted by Auto"), c.dim, 11, false, "teamPhaseByAuto");
        byAuto->setToolTip(tr("Auto accepted this phase when its first task started")); h->addWidget(byAuto);
    }
    if (auto *mode = phaseModeBadge(team, group.n)) h->addWidget(mode);
    // A cancelled task is not work the phase still owes: a Done phase with one
    // cancelled task reads 7/7, not 7/8 (owner, 2026-10-28).
    int accepted = 0, owed = 0;
    for (int id : group.tasks) if (const auto *task = team.task(id); task && task->status != "cancelled") {
        ++owed;
        if (task->status == "accepted") ++accepted;
    }
    auto *bar = new PhaseBar(c, accepted, owed, state == "done" ? c.accentMuted : c.accent);
    bar->setObjectName("teamPhaseProgress");
    // The bar and the proposed-by or health line give way first on a narrow column.
    auto *info = new FitRow(9); info->setObjectName("teamPhaseInfo"); info->setAttribute(Qt::WA_TransparentForMouseEvents);
    info->add(bar, 1);
    // Open, it takes what the title and the actions leave; collapsed, the summary does.
    if (open) info->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    h->addWidget(info, open ? 1 : 0);
    // U59: the open free phase's last build and what was accepted since.
    if (freeOpen) {
        auto *build = label(freeBuildLine(team), c.dim, 11, false, "teamPhaseFreeInfo"); build->setAttribute(Qt::WA_TransparentForMouseEvents);
        info->add(build, 2);
    }
    // Proposed by (a draft) or the phase's health (running, queued).
    if ((state == "draft" || state == "rejected") && record && !record->addedBy.isEmpty()) {
        QString text = tr("proposed by %1").arg(record->addedBy);
        if (record->addedAt > 0) text += QStringLiteral(" · ") + QDateTime::fromSecsSinceEpoch(qint64(record->addedAt)).toString(QStringLiteral("HH:mm"));
        auto *by = label(text, c.dim, 11, false, "teamPhaseProposed"); by->setAttribute(Qt::WA_TransparentForMouseEvents);
        info->add(by, 2);
    } else if (state == "running" || state == "queued") {
        int healthy = 0;
        for (int id : group.tasks) healthy += !problemsByTask.contains(id);
        auto *health = label(tr("Healthy %1/%2").arg(healthy).arg(group.tasks.size()), healthy == group.tasks.size() ? c.accent : c.warn, 11, false, "teamPhaseHealth");
        const QString tip = phaseHealthTip(team, group.n);
        const auto *summary = team.phaseSummaryOf(group.n);
        agedHealth(health, QString(), !tip.isEmpty() ? tip : summary ? summary->healthLine : QString());
        info->add(health, 2);
        // QA #539: the folders the phase's tests hold, as the mock-up's "1 folder held".
        QString heldTip;
        if (const QString held = heldFoldersNote(team, group.tasks, &heldTip); !held.isEmpty()) {
            auto *note = label(held, c.warn, 11, false, "teamPhaseHeld"); note->setToolTip(heldTip);
            info->add(note, 1);
        }
    }
    // F12 and F10: why a draft waits, and the lead's edits since you last saw it.
    if (state == "draft" || state == "rejected") {
        for (const auto &band : phaseBands(team, [](const PhaseGroup &) { return true; })) {
            if (band.n != group.n) continue;
            if (!band.blocked.isEmpty()) {
                auto *waits = label(band.blocked, c.warn, 11, false, "teamPhaseWaiting");
                waits->setToolTip(band.blockedTip.isEmpty() ? band.blocked : band.blockedTip);
                info->add(waits, 3);
            }
            if (band.changes > 0) {
                auto *changes = label(band.changes == 1 ? tr("1 change from the lead") : tr("%1 changes from the lead").arg(band.changes), c.folder, 11, false, "teamPhaseChanges");
                changes->setToolTip(band.changeLines.join(QLatin1Char('\n')));
                info->add(changes, 4);
            }
        }
    }
    // Collapsed: its tasks in one line.
    if (!open) {
        QStringList titles;
        for (int id : group.tasks) if (const auto *task = team.task(id)) titles << QStringLiteral("#%1 %2").arg(id).arg(task->title);
        auto *summary = elided(titles.join(QStringLiteral(" · ")), c.dim, 12);
        summary->setObjectName("teamPhaseSummary"); summary->setToolTip(titles.join(QLatin1Char('\n')));
        summary->setAttribute(Qt::WA_TransparentForMouseEvents);
        h->addWidget(summary, 1);
    }
    // The owner's actions, by state; none while the team plans (Approve & Run decides).
    const bool decided = phaseDecided(team, group.n) || m_phaseAct == group.n;
    if (state == "draft" && team.status != "planning") {
        QStringList errors;
        for (const auto &issue : team.errorItems) if (issue.task && group.tasks.contains(issue.task)) errors << issue.text;
        if (!errors.isEmpty()) {
            auto *count = new StatusMark(errors.size() == 1 ? tr("1 error") : tr("%1 errors").arg(errors.size()), c.danger, "stuck", 0,
                                         Qt::AlignLeft, nullptr, errors.join(QLatin1Char('\n')));
            count->setObjectName("teamPhaseErrors"); count->setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
            count->setFixedWidth(count->sizeHint().width());
            h->addWidget(count);
        }
        auto *autoLabel = label(tr("Auto-accept"), c.text3, 11); h->addWidget(autoLabel);
        auto *autoSwitch = new ToggleSwitch(c); autoSwitch->setObjectName("teamPhaseAutoAccept"); autoSwitch->setChecked(team.autoAccept);
        autoSwitch->setToolTip(autoAcceptSwitchTip(team.autoAccept));
        autoSwitch->setAccessibleName(tr("Auto-accept drafts"));
        autoSwitch->setEnabled(settingsOpen(team));
        QObject::connect(autoSwitch, &QAbstractButton::clicked, this, [this, id = team.id](bool on) { if (const auto *t = currentTeam(id)) setAutoAccept(*t, on); });
        h->addWidget(autoSwitch);
        auto *reject = button(tr("Reject…"), "teamPhaseReject"); reject->setProperty("phase", group.n);
        reject->setToolTip(tr("Send the draft back to the lead with what should change"));
        reject->setEnabled(!decided);
        QObject::connect(reject, &QPushButton::clicked, this, [this, key = rejectKey(team, group.n)] {
            if (m_rejectPhase != key) m_rejectText.clear();
            m_rejectPhase = m_rejectPhase == key ? QString() : key; m_rejectFocus = !m_rejectPhase.isEmpty(); scheduleRender();
        });
        h->addWidget(reject);
        auto *accept = button(tr("Accept"), "teamPhaseAccept", true); accept->setProperty("phase", group.n);
        // Errors come only with `team status`; a list poll carries none.
        const QString blocked = !team.full ? tr("Checking the plan…")
                              : !errors.isEmpty() ? tr("Fix the plan errors first:\n%1").arg(errors.join(QLatin1Char('\n'))) : acceptBlocked(team, group.n);
        accept->setEnabled(blocked.isEmpty() && !decided);
        const int alongside = alongsidePhase(team, group.n);
        QString acceptTip = blocked;
        if (blocked.isEmpty()) {
            if (!alongside) {
                acceptTip = tr("Hand the phase's tasks out (it waits if an earlier phase is still running)");
            } else if (group.barrier) {
                // N6: the tasks wait for the earlier phase even when it is approved now.
                acceptTip = tr("Approve phase %1 now; its tasks start when phase %2 is finished").arg(group.n).arg(alongside);
            } else {
                acceptTip = tr("Hand the phase's tasks out now, alongside phase %1").arg(alongside);
            }
        }
        accept->setToolTip(acceptTip);
        QObject::connect(accept, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] { approvePhase(id, phase); });
        h->addWidget(accept);
        // F10 and G9: the order among the drafts and the owner's discard, in a menu.
        auto *more = new QToolButton; more->setObjectName("teamPhaseMore"); more->setProperty("phase", group.n);
        more->setIcon(workspaceIcon("more", c.muted)); more->setAutoRaise(true); more->setToolTip(tr("Phase actions"));
        more->setEnabled(!decided);
        QObject::connect(more, &QToolButton::clicked, this, [this, id = team.id, phase = group.n, more] {
            // The workspace rebuilds while a menu is open: the menu holds ids, not rows.
            auto *menu = new QMenu(this); menu->setObjectName("teamPhaseMenu"); menu->setAttribute(Qt::WA_DeleteOnClose);
            const auto *current = currentTeam(id);
            const auto drafts = current ? current->draftPhases() : QList<int>();
            const int position = drafts.indexOf(phase);
            auto *earlier = menu->addAction(tr("Move earlier")); earlier->setObjectName("teamPhaseMoveUp"); earlier->setEnabled(position > 0);
            auto *later = menu->addAction(tr("Move later")); later->setObjectName("teamPhaseMoveDown"); later->setEnabled(position >= 0 && position + 1 < drafts.size());
            menu->addSeparator();
            auto *discard = menu->addAction(tr("Discard phase…")); discard->setObjectName("teamPhaseDiscard");
            QObject::connect(earlier, &QAction::triggered, this, [this, id, phase] {
                const auto *t = currentTeam(id); if (!t) return;
                const auto list = t->draftPhases(); const int at = list.indexOf(phase);
                if (at > 0) movePhase(*t, phase, list.value(at - 1), QStringLiteral("before"));
            });
            QObject::connect(later, &QAction::triggered, this, [this, id, phase] {
                const auto *t = currentTeam(id); if (!t) return;
                const auto list = t->draftPhases(); const int at = list.indexOf(phase);
                if (at >= 0 && at + 1 < list.size()) movePhase(*t, phase, list.value(at + 1), QStringLiteral("after"));
            });
            QObject::connect(discard, &QAction::triggered, this, [this, id, phase] { discardPhase(id, phase); });
            menu->popup(more->mapToGlobal(QPoint(0, more->height())));
        });
        h->addWidget(more);
    } else if (state == "rejected" && record) {
        auto *sent = new ShrinkLabel(tr("Sent back: “%1”").arg(record->rejection.comment), 80);
        {QFont f = sent->font(); f.setPixelSize(11); sent->setFont(f);
         QPalette pal = sent->palette(); pal.setColor(QPalette::WindowText, c.warn); sent->setPalette(pal);}
        sent->setObjectName("teamPhaseSentBack"); sent->setMaximumWidth(280);
        sent->setToolTip(record->rejection.comment);
        h->addWidget(sent);
        if (record->rejection.reworkedAt <= 0) {
            auto *undo = button(tr("Undo"), "teamPhaseUndo"); undo->setProperty("ghost", true); undo->setProperty("phase", group.n);
            undo->setToolTip(tr("Take the send-back back; the draft waits for you again"));
            undo->setEnabled(!decided);
            QObject::connect(undo, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] {
                const auto *t = currentTeam(id);
                if (!t || m_phaseAct || phaseDecided(*t, phase)) return;
                const QString key = markPhaseDecided(*t, phase);
                m_panel->act("undo-reject-phase", {{"team", id}, {"phase", phase}},
                             [this, key](bool ok, const QString &message) { phaseSent(key, ok, message); });
                scheduleRender();
            });
            h->addWidget(undo);
        }
    } else if (state == "running" && (team.status == "running" || team.status == "paused")) {
        // U29: a running phase takes a task only on your word, after a warning.
        auto *add = button(tr("Add task…"), "teamPhaseAddLate"); add->setProperty("ghost", true); add->setProperty("phase", group.n);
        add->setToolTip(freeOpen ? tr("Add a task to the open phase: it starts when its member is free")
                                 : tr("Add a task to this running phase: Zerus warns first, then you confirm"));
        QObject::connect(add, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] { if (const auto *t = currentTeam(id)) addLateDialog(*t, phase); });
        h->addWidget(add);
    } else if (state == "queued") {
        const QString text = record && !record->waiting.text.isEmpty() ? record->waiting.text : tr("Starts after phase %1").arg(group.n - 1);
        auto *waits = label(text, c.dim, 11, false, "teamPhaseQueued"); waits->setAttribute(Qt::WA_TransparentForMouseEvents);
        h->addWidget(waits);
    }
    // U39 (owner, 2026-10-28): an accepted phase can be discarded, or (U42)
    // returned to a draft, while its work has not started; hgs refuses it
    // otherwise and names the tasks.
    if ((state == "queued" || state == "running") && team.status != "planning" && !freeOpen) {
        auto *more = new QToolButton; more->setObjectName("teamPhaseMore"); more->setProperty("phase", group.n);
        more->setIcon(workspaceIcon("more", c.muted)); more->setAutoRaise(true); more->setToolTip(tr("Phase actions"));
        QObject::connect(more, &QToolButton::clicked, this, [this, id = team.id, phase = group.n, more] {
            auto *menu = new QMenu(this); menu->setObjectName("teamPhaseMenu"); menu->setAttribute(Qt::WA_DeleteOnClose);
            // U42: back to a draft that waits for your approval again.
            auto *redraft = menu->addAction(tr("Return to draft…")); redraft->setObjectName("teamPhaseReturnToDraft");
            QObject::connect(redraft, &QAction::triggered, this, [this, id, phase] { returnPhaseToDraft(id, phase); });
            auto *discard = menu->addAction(tr("Discard phase…")); discard->setObjectName("teamPhaseDiscard");
            QObject::connect(discard, &QAction::triggered, this, [this, id, phase] { discardPhase(id, phase); });
            menu->popup(more->mapToGlobal(QPoint(0, more->height())));
        });
        h->addWidget(more);
    }
    if (m_view == "board") {
        // Pinned to the visible part of the sideways page (PinnedPhaseHeaders).
        auto *pin = new QWidget; pin->setObjectName("teamPhaseHeaderPin");
        auto *ph = new QHBoxLayout(pin); ph->setContentsMargins(0, 0, 0, 0); ph->setSpacing(0);
        ph->addWidget(head, 1);
        ph->addStretch(0);
        frame->header = pin;
        fv->addWidget(pin);
    } else {
        fv->addWidget(head);
    }
    if (!open) return frame;
    // The lead's note, then the send-back form, then the body.
    const QString note = record ? record->note : QString();
    if (!note.isEmpty()) {
        auto *noteRow = new QWidget; noteRow->setObjectName("teamPhaseNote");
        auto *nh = new QHBoxLayout(noteRow); nh->setContentsMargins(42, 0, 12, 8); nh->setSpacing(6);
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon("message", c.muted).pixmap(12, 12)); nh->addWidget(icon);
        auto *text = label(note, c.muted, 12, false, "teamPhaseNoteText"); text->setWordWrap(true); nh->addWidget(text, 1);
        fv->addWidget(noteRow);
    }
    if (!m_rejectPhase.isEmpty() && m_rejectPhase == rejectKey(team, group.n) && state == "draft" && team.status != "planning") {
        auto *form = new QWidget; form->setObjectName("teamPhaseRejectForm");
        auto *rh = new QHBoxLayout(form); rh->setContentsMargins(42, 0, 12, 10); rh->setSpacing(6);
        auto *edit = new QPlainTextEdit; edit->setObjectName("teamPhaseRejectText");
        edit->setPlaceholderText(tr("What should Lead change in this phase?"));
        edit->setPlainText(m_rejectText); edit->setFixedHeight(56); edit->setTabChangesFocus(true);
        edit->moveCursor(QTextCursor::End);
        rh->addWidget(edit, 1);
        auto *send = button(tr("Send back to Lead"), "teamPhaseRejectSend"); send->setProperty("dangerFill", true);
        send->setEnabled(!m_rejectText.trimmed().isEmpty());
        QObject::connect(edit, &QPlainTextEdit::textChanged, this, [this, edit, send] { m_rejectText = edit->toPlainText(); send->setEnabled(!m_rejectText.trimmed().isEmpty()); });
        QObject::connect(send, &QPushButton::clicked, this, [this, id = team.id, phase = group.n] { rejectPhase(id, phase); });
        auto *cancel = button(tr("Cancel"), "teamPhaseRejectCancel"); cancel->setProperty("ghost", true);
        QObject::connect(cancel, &QPushButton::clicked, this, [this] { m_rejectPhase.clear(); m_rejectText.clear(); scheduleRender(); });
        rh->addWidget(send, 0, Qt::AlignTop); rh->addWidget(cancel, 0, Qt::AlignTop);
        fv->addWidget(form);
        // Typing survives the workspace's re-renders: the text, and the focus it had.
        if (m_rejectFocus) { m_rejectFocus = false; QTimer::singleShot(0, edit, [edit] { edit->setFocus(Qt::OtherFocusReason); }); }
    }
    // U59: the open free phase folds its accepted tasks into one line (Show
    // brings them back) and, when idle, offers its next steps.
    PhaseGroup shown = group;
    if (freeOpen) {
        QList<int> accepted;
        for (int id : group.tasks) if (const auto *task = team.task(id); task && task->status == "accepted") accepted << id;
        if (!accepted.isEmpty()) {
            fv->addWidget(freeAcceptedFold(team, group.n, accepted));
            if (!m_freeUnfolded.contains(QStringLiteral("%1/%2").arg(team.id).arg(group.n)))
                shown.tasks.removeIf([&accepted](int id) { return accepted.contains(id); });
        }
    }
    if (!shown.tasks.isEmpty()) {
        QWidget *body = laneSections(team, shown);
        auto *bodyBox = new QWidget; bodyBox->setObjectName("teamPhaseBody"); bodyBox->setProperty("phase", group.n);
        auto *bv = new QVBoxLayout(bodyBox); bv->setContentsMargins(6, 6, 6, 8); bv->setSpacing(0);
        bv->addWidget(body);
        fv->addWidget(bodyBox);
    }
    if (freeOpen && state == "idle") fv->addWidget(freeIdleCard(team, group.n));
    return frame;
}

QWidget *TeamWorkspace::laneSections(const Team &team, const PhaseGroup &group)
{
    const Colors c = colorsFor(m_dark);
    const auto lanes = laneGroups(team, group.tasks);
    // No lane at all: the phase's body is exactly what it always was.
    if (lanes.isEmpty() || (lanes.size() == 1 && lanes.first().first == 0)) return laneBody(team, group.tasks, group);
    auto *box = new QWidget; box->setObjectName("teamPhaseLanes"); box->setProperty("phase", group.n);
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(4);
    for (const auto &[lane, ids] : lanes) {
        const QString key = QStringLiteral("ui/collapsed/teamLane/%1/%2/%3").arg(team.id).arg(group.n).arg(lane);
        const bool open = !Settings::value(key, false).toBool();
        auto *header = new QToolButton;
        header->setObjectName("teamLaneHeader");
        header->setProperty("lane", lane); header->setProperty("phase", group.n); header->setProperty("collapsed", !open);
        header->setProperty("tasks", ids.size());
        const QString label = lane > 0 ? laneLabel(team, group.tasks, lane) : QString();
        const QString name = lane > 0 ? (label.isEmpty() ? tr("Lane %1").arg(lane) : tr("Lane %1 · %2").arg(lane).arg(label))
                                      : tr("No lane · by dependency");
        header->setText(ids.size() == 1 ? tr("%1 · 1 task").arg(name) : tr("%1 · %2 tasks").arg(name).arg(ids.size()));
        header->setIcon(workspaceIcon(open ? "chevron-down" : "chevron-right", c.text2));
        header->setIconSize(QSize(10, 10));
        header->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
        header->setAutoRaise(true);
        header->setCursor(Qt::PointingHandCursor);
        header->setAccessibleDescription(open ? tr("Expanded") : tr("Collapsed"));
        header->setToolTip(lane > 0 ? tr("Lanes only shape the plan; hand-out still follows dependencies and phases")
                                    : tr("These tasks fall back to their dependency layers"));
        header->setStyleSheet(QStringLiteral("QToolButton#teamLaneHeader{color:%1;font-size:11px;font-weight:600;border:0;padding:2px 4px;background:transparent;}"
                                             "QToolButton#teamLaneHeader:hover{color:%2;}")
                                  .arg(c.text2.name(), c.fg.name()));
        QObject::connect(header, &QToolButton::clicked, this, [this, key](bool) {
            Settings::set(key, !Settings::value(key, false).toBool());
            scheduleRender();
        });
        // U27: a lane is renamed in the plan and in a draft phase.
        header->setContextMenuPolicy(Qt::CustomContextMenu);
        QObject::connect(header, &QWidget::customContextMenuRequested, this, [this, id = team.id, phase = group.n, lane, label](const QPoint &at) {
            const auto *current = currentTeam(id);
            if (!current || lane <= 0 || !(current->status == "planning" || (current->phase(phase) && current->phase(phase)->draft()))) return;
            QMenu menu; menu.setObjectName("teamLaneMenu");
            auto *rename = menu.addAction(tr("Rename lane…")); rename->setObjectName("teamLaneRename");
            auto *chosen = menu.exec(at);
            if (chosen != rename) return;
            bool ok = false;
            const QString text = QInputDialog::getText(this, tr("Rename lane %1").arg(lane), tr("Lane name"), QLineEdit::Normal,
                                                       label.isEmpty() ? tr("Checkpoint %1").arg(lane) : label, &ok);
            if (!ok) return;
            if (const auto *fresh = currentTeam(id)) renameLane(*fresh, phase, lane, text.trimmed());
        });
        v->addWidget(header);
        if (!open) continue;
        auto *body = laneBody(team, ids, group);
        body->setProperty("lane", lane);
        body->setProperty("phase", group.n);
        auto *wrap = new QWidget; wrap->setObjectName("teamLaneBody"); wrap->setProperty("lane", lane); wrap->setProperty("phase", group.n);
        auto *wv = new QVBoxLayout(wrap); wv->setContentsMargins(14, 0, 0, 2); wv->setSpacing(0);
        wv->addWidget(body);
        v->addWidget(wrap);
    }
    return box;
}

QWidget *TeamWorkspace::laneBody(const Team &team, const QList<int> &ids, const PhaseGroup &group)
{
    if (m_view == "board") return tickingBoard(boardColumns(team, ids, group.n));
    if (m_view == "layers") return tickingBoard(layerRows(team, ids));
    return phaseGraph(team, group, ids);
}

void TeamWorkspace::sendLaneChanges(const Team &team, int phase, const QList<QPair<int, QJsonValue>> &updates)
{
    if (updates.isEmpty()) return;
    // Planning edits the plan with `plan`; a running team's draft phase edits its
    // tasks with edit-draft (the server takes `plan` only in Planning).
    if (team.status == "planning") {
        // U58: the canvas may show a copy without cancelled tasks; the plan is
        // sent from the whole team.
        const auto *whole = planTeam(team.id);
        if (!whole) return;
        Team edited = *whole;
        for (const auto &[id, lane] : updates) {
            for (auto &candidate : edited.tasks) {
                if (candidate.id != id) continue;
                if (lane.isNull()) { candidate.lane = Lane{}; continue; }
                const auto value = lane.toObject();
                candidate.lane.n = value.value("n").toInt();
                candidate.lane.label = value.value("label").toString();
            }
        }
        m_panel->savePlan(edited);
        return;
    }
    QJsonArray change;
    for (const auto &[id, lane] : updates) change.append(QJsonObject{{"id", id}, {"lane", lane}});
    m_panel->act("edit-draft", {{"team", team.id}, {"phase", phase}, {"tasks_update", change}});
}

void TeamWorkspace::setTaskLane(const Team &team, int task, int lane)
{
    const auto *found = team.task(task);
    if (!found) return;
    const int phase = found->phase;
    QStringList ids;
    for (int id : team.tasksOfPhase(phase)) ids << QString::number(id);
    const QString label = lane > 0 ? laneLabel(team, team.tasksOfPhase(phase), lane) : QString();
    sendLaneChanges(team, phase, {{task, lane > 0 ? QJsonValue(QJsonObject{{"n", lane}, {"label", label}}) : QJsonValue()}});
}

void TeamWorkspace::renameLane(const Team &team, int phase, int lane, const QString &label)
{
    if (lane <= 0) return;
    QList<QPair<int, QJsonValue>> updates;
    for (int id : team.tasksOfPhase(phase)) {
        const auto *task = team.task(id);
        if (!task || task->lane.n != lane) continue;
        updates.append({id, QJsonValue(QJsonObject{{"n", lane}, {"label", label.left(40)}})});
    }
    sendLaneChanges(team, phase, updates);
}

void TeamWorkspace::laneMenu(const Team &team, int task, const QPoint &global)
{
    const auto *found = team.task(task);
    if (!found) return;
    const auto *record = team.phase(found->phase);
    if (!(team.status == "planning" || (record && record->draft()))) return;
    QMenu menu; menu.setObjectName("teamTaskLaneMenu");
    const auto phaseTasks = team.tasksOfPhase(found->phase);
    const auto lanes = laneGroups(team, phaseTasks);
    for (const auto &[lane, ids] : lanes) {
        if (lane <= 0) continue;
        const QString label = laneLabel(team, phaseTasks, lane);
        auto *action = menu.addAction(label.isEmpty() ? tr("Lane %1").arg(lane) : tr("Lane %1 · %2").arg(lane).arg(label));
        action->setObjectName(QStringLiteral("teamTaskLane%1").arg(lane));
        action->setCheckable(true); action->setChecked(found->lane.n == lane);
        QObject::connect(action, &QAction::triggered, this, [this, id = team.id, task, lane] {
            if (const auto *fresh = currentTeam(id)) setTaskLane(*fresh, task, lane);
        });
    }
    menu.addSeparator();
    auto *fresh = menu.addAction(tr("New lane…")); fresh->setObjectName("teamTaskLaneNew");
    QObject::connect(fresh, &QAction::triggered, this, [this, id = team.id, task, phaseTasks] {
        const auto *current = currentTeam(id);
        if (!current) return;
        int next = 1;
        for (int id : phaseTasks) if (const auto *t = current->task(id)) next = qMax(next, t->lane.n + 1);
        bool ok = false;
        const QString label = QInputDialog::getText(this, tr("New lane"), tr("Lane name"), QLineEdit::Normal, tr("Checkpoint %1").arg(next), &ok);
        if (!ok) return;
        const int lane = next;
        QList<QPair<int, QJsonValue>> updates = {{task, QJsonValue(QJsonObject{{"n", lane}, {"label", label.trimmed().left(40)}})}};
        if (const auto *freshTeam = currentTeam(id)) sendLaneChanges(*freshTeam, freshTeam->task(task) ? freshTeam->task(task)->phase : 1, updates);
    });
    auto *none = menu.addAction(tr("No lane")); none->setObjectName("teamTaskLaneNone");
    none->setCheckable(true); none->setChecked(!found->lane.known());
    QObject::connect(none, &QAction::triggered, this, [this, id = team.id, task] {
        if (const auto *freshTeam = currentTeam(id)) setTaskLane(*freshTeam, task, 0);
    });
    menu.exec(global);
}

QWidget *TeamWorkspace::phaseGraph(const Team &team, const PhaseGroup &group, const QList<int> &ids)
{
    const QList<int> shown = ids.isEmpty() ? group.tasks : ids;
    const Colors c = colorsFor(m_dark);
    const auto problemsByTask = problemTasks(team);
    auto *view = new TaskView(c, TaskView::Wide);
    view->setObjectName("teamPhaseGraph"); view->setProperty("phase", group.n);
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    if (m_showBlockers) view->emphasis = blockers(team);
    const QSet<int> inPhase(group.tasks.cbegin(), group.tasks.cend());
    // Edges inside the phase; a dependency in another phase is named on the node.
    for (int id : group.tasks) if (const auto *task = team.task(id)) for (const auto &d : task->deps) {
        if (!inPhase.contains(d.task)) continue;
        view->edges[id].append(d.task);
        if (d.soft && !team.allHard) view->soft.insert({d.task, id});
    }
    const auto layers = phaseLayers(team, shown);
    QList<int> order = shown;
    std::stable_sort(order.begin(), order.end(), [&layers](int a, int b) { return layers.value(a) < layers.value(b); });
    graphNodes(view, team, order, inPhase);
    for (auto &node : view->nodes) node.layer = layers.value(node.id);
    // Planning edits the plan with `plan`; a running team's draft phase edits
    // its tasks' dependencies with edit-draft (the server takes `plan` only
    // in Planning).
    // C4 (#548): every edit is built from the full copy (planTeam), never the
    // `team list` summary, which has no dependencies or descriptions.
    if (team.status == "planning") {
        view->editable = true;
        view->linked = [this](int dependency, int task) { if (const auto *t = planTeam(m_team)) m_panel->link(*t, dependency, task, true); };
        view->unlinked = [this](int dependency, int task) { if (const auto *t = planTeam(m_team)) m_panel->link(*t, dependency, task, false); };
    } else if (ownerPhaseState(team, group) == "draft") {
        for (int id : group.tasks) if (const auto *task = team.task(id); task && task->status == "draft") view->editableTasks.insert(id);
        view->linked = [this, id = team.id](int dependency, int task) { if (const auto *t = planTeam(id)) linkDraft(*t, dependency, task, true); };
        view->unlinked = [this, id = team.id](int dependency, int task) { if (const auto *t = planTeam(id)) linkDraft(*t, dependency, task, false); };
    }
    if (team.status == "planning") view->clicked = [this](int id, const QPoint &) { if (const auto *t = planTeam(m_team)) m_panel->editTask(*t, id); };
    else view->clicked = [this](int id, const QPoint &) {
        const auto *t = currentTeam(m_team);
        if (t) if (const auto *task = t->task(id); task && task->subteam.known && !task->subteam.team.isEmpty()) {
            m_teams->watch(task->subteam.team); showTeam(task->subteam.team); return;
        }
        m_drawerTask = id; scheduleRender();
    };
    keepGraphEdit(view, team.id);
    view->layout();
    // The phase scrolls sideways when its graph is wider than the column.
    auto *area = new QScrollArea; area->setObjectName("teamPhaseGraphArea"); area->setFrameShape(QFrame::NoFrame);
    area->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    area->setWidget(view); area->setWidgetResizable(false);
    view->setMinimumWidth(view->width());
    new SideBarFit(area, view);
    // Owner, 2026-10-28: a phase scrolled sideways keeps its place; every poll
    // rebuilt it at the left edge.
    keepScroll(area, QStringLiteral("phasegraph/%1/%2").arg(team.id).arg(group.n));
    // Spec 1.2: the thin 8 px bar without arrows here too, also outside a
    // window that styles its bars.
    area->setStyleSheet(QStringLiteral("QScrollArea#teamPhaseGraphArea{background:transparent;} QScrollArea#teamPhaseGraphArea > QWidget > QWidget{background:transparent;}")
                        + workspaceScrollbars(m_dark));
    area->viewport()->setAutoFillBackground(false);
    area->setFixedHeight(view->height() + 10);
    return area;
}

QWidget *TeamWorkspace::boardColumns(const Team &team, const QList<int> &ids, int phase)
{
    const Colors c = colorsFor(m_dark);
    const auto problemsByTask = problemTasks(team);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    const Heat *heatOn = heat(team);
    auto *box = new QWidget; box->setObjectName("teamBoardColumns"); box->setProperty("phase", phase);
    auto *h = new QHBoxLayout(box); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
    // U36: In review and In test sit between Submitted and Accepted once the
    // team uses pipelines, in every phase alike.
    QList<std::pair<QString, QString>> columns{{"pending", tr("Pending")}, {"active", tr("Active")}, {"problems", tr("Problems")},
                                               {"submitted", tr("Submitted")}, {"accepted", tr("Accepted")}};
    if (usesPipelines(team)) {
        columns.insert(4, {QStringLiteral("in_review"), tr("In review")});
        columns.insert(5, {QStringLiteral("in_test"), tr("In test")});
    }
    // A rejected or cancelled task never sits under Accepted (QA): its own
    // column, when the team has one. U33: every phase shows the same columns,
    // so they line up from phase to phase.
    bool decided = false;
    for (const auto &task : team.tasks) decided = decided || column(task, problemsByTask.contains(task.id)) == "decided";
    if (decided) columns.append({"decided", tr("Decided")});
    for (const auto &[key, name] : columns) {
        auto *col = new QFrame; col->setObjectName("teamBoardColumn"); col->setProperty("column", key); col->setProperty("phase", phase);
        auto *cv = new QVBoxLayout(col); cv->setContentsMargins(5, 5, 5, 5); cv->setSpacing(5);
        QList<const Task *> shown;
        for (int id : ids) {
            const auto *task = team.task(id);
            if (!task || column(*task, problemsByTask.contains(id)) != key) continue;
            if (!m_focus.isEmpty() && task->assignee != m_focus) continue;
            shown << task;
        }
        // #706: styled before its cards go in; a column styled after them
        // restyled every card it held.
        const bool problems = key == "problems" && !shown.isEmpty();
        col->setStyleSheet(QStringLiteral("QFrame#teamBoardColumn{background:%1;border:1px solid %2;border-radius:10px;}")
                               .arg(shown.isEmpty() ? QStringLiteral("transparent") : c.bg.name(),
                                          problems ? alpha(c.danger, .55).name(QColor::HexArgb) : shown.isEmpty() ? alpha(c.cardBorder, .6).name(QColor::HexArgb) : c.cardBorder.name()));
        auto *headRow = row(4);
        if (key == "in_review" || key == "in_test") {
            auto *icon = new QLabel; icon->setPixmap(glyphPixmap(c.text3, key, 11)); headRow->addWidget(icon);
        }
        auto *title = label(name.toUpper(), c.text3, 10, true); title->setObjectName("teamBoardColumnTitle");
        headRow->addWidget(title);
        // U36: the stages there that wait for a free member.
        if (const QString note = waitingNote(team, shown); !note.isEmpty())
            headRow->addWidget(label(QStringLiteral("· ") + note, c.text3, 10, false, "teamBoardColumnWaiting"));
        headRow->addStretch();
        headRow->addWidget(label(QString::number(shown.size()), c.text3, 10, true));
        cv->addLayout(headRow);
        auto held = std::make_shared<Team>(team);
        for (const auto *task : shown) {
            const int taskId = task->id;
            auto buildCard = [this, held, problemsByTask, cv, taskId, c] {
                const Team &team = *held;
                const auto *task = team.task(taskId);
                if (!task) return;
                const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
                const Heat *heatOn = heat(team);
            const bool problem = problemsByTask.contains(task->id);
            // U36: the stage line, and a warning border for a held folder or
            // a task out of rounds.
            auto *stages = stageLineWidget(team, *task, c);
            const bool stageWarn = stages && stages->property("warn").toBool();
            const QColor edge = problem ? toneColor(c, problemsByTask.value(task->id)) : stageWarn ? c.warn : c.border;
            auto *cardBox = new ClickFrame; cardBox->setObjectName("teamBoardCard"); cardBox->setCursor(Qt::PointingHandCursor);
            cardBox->setProperty("task", task->id);
            cardBox->setProperty("lane", task->lane.n);
            cardBox->setContextMenuPolicy(Qt::CustomContextMenu);
            QObject::connect(cardBox, &QWidget::customContextMenuRequested, this, [this, id = team.id, card = cardBox, taskId = task->id](const QPoint &at) {
                if (const auto *fresh = currentTeam(id)) laneMenu(*fresh, taskId, card->mapToGlobal(at));
            });
            // U38: the heat tint fills the card; its edge and state icon stay.
            cardBox->setStyleSheet(QStringLiteral("QFrame#teamBoardCard{background:%1;border:1px %4 %2;border-radius:8px;} QFrame#teamBoardCard:hover{border-color:%3;}")
                                       .arg(heatCardFill(heatOn, *task, c).name(), edge.name(), c.accent.name(), task->status == "draft" ? QStringLiteral("dashed") : QStringLiteral("solid")));
            cardBox->clicked = [this, id = task->id] { m_drawerTask = id; scheduleRender(); };
            const auto s = taskStatus(*task);
            const auto mark = phaseMark(team, *task);
            QString cardTip = QStringLiteral("#%1 %2 — %3").arg(task->id).arg(task->title, mark.tooltip.isEmpty() ? s.text : mark.tooltip);
            if (const QString tip = heatOn ? heatTip(*heatOn, *task) : QString(); !tip.isEmpty()) cardTip += QLatin1Char('\n') + tip;
            cardBox->setToolTip(cardTip);
            auto *b = new QVBoxLayout(cardBox); b->setContentsMargins(8, 6, 8, 6); b->setSpacing(4);
            auto *r1 = row(6);
            auto *state = new StatusMark(QString(), toneColor(c, problem ? problemsByTask.value(task->id) : mark.tone != Tone::Neutral ? mark.tone : s.tone),
                                         mark.kind.isEmpty() ? kindOfTask(*task, s) : mark.kind, 14, Qt::AlignLeft, nullptr, mark.tooltip.isEmpty() ? s.text : mark.tooltip);
            state->setObjectName("teamBoardState"); state->setProperty("statusText", s.text); r1->addWidget(state);
            auto *id = label(QStringLiteral("#%1").arg(task->id), c.fg, 12, true, "teamBoardId"); r1->addWidget(id);
            if (task->addedLate.known) r1->addWidget(outOfScopeMark(*task, c));
            QStringList deps; for (const auto &d : task->deps) deps << QStringLiteral("#%1").arg(d.task);
            if (!deps.isEmpty()) r1->addWidget(label(QStringLiteral("←") + deps.join(QStringLiteral(" ")), c.folder, 11, false, "teamBoardDeps"));
            auto *name = elided(task->title, c.fg, 12); name->setObjectName("teamBoardTitle");
            QFont bold = name->font(); bold.setWeight(QFont::DemiBold); name->setFont(bold);
            r1->addWidget(name, 1);
            b->addLayout(r1);
            auto *r2 = row(6);
            const QString role = team.assigneeRole(*task, tr("You"));
            auto *pill = new Pill(role, roleColor(role, m_dark)); pill->setObjectName("teamBoardRole"); r2->addWidget(pill);
            if (const auto *m = team.member(task->assignee)) { auto *i = new QLabel; i->setPixmap(agentLogo(m->agent, c.text3).pixmap(14, 14)); r2->addWidget(i); }
            // U38: with the heat on, the loss takes the working time's place,
            // at the line's end; the activity label elides before it does.
            QLabel *lost = heatOn ? heatBoardLabel(*heatOn, *task, c) : nullptr;
            if (lost) lost->setMinimumWidth(lost->sizeHint().width());
            // The same small activity label as the graph and the plain board
            // (a problem text wins).
            if (!problem) {
                const auto *member = team.member(task->assignee);
                const QString note = stages ? stages->property("note").toString() : QString();
                const QString activity = note.isEmpty() ? activityLabel(*task, member ? live(*member) : Live{}) : note;
                if (!activity.isEmpty()) {
                    const QColor tone = note.isEmpty() ? c.text3 : c.warn;
                    auto *a = lost ? elided(activity, tone, 11) : label(activity, tone, 11); a->setObjectName("teamBoardActivity");
                    if (note.isEmpty()) a->setProperty("liveTask", task->id);
                    if (lost) r2->addWidget(a, 1); else r2->addWidget(a);
                }
            }
            if (!lost) addWorked(r2, *task, workedClock()(), c, "teamBoardWorked");
            r2->addStretch(1);
            if (problem) {
                auto *why = elided(problemText(team, task->id), toneColor(c, problemsByTask.value(task->id)), 11); why->setObjectName("teamBoardProblem");
                r2->insertWidget(r2->count() - 1, why, 3);
            }
            if (lost) r2->addWidget(lost);
            b->addLayout(r2);
            if (stages) b->addWidget(stages);
                if (auto *last = cv->count() > 0 ? cv->itemAt(cv->count() - 1) : nullptr; last && last->spacerItem())
                    cv->insertWidget(cv->count() - 1, cardBox);
                else
                    cv->addWidget(cardBox);
            };
            if (team.tasks.size() <= kSmallTeam) buildCard(); else queueSlice(buildCard);
        }
        if (shown.isEmpty()) {
            auto *none = label(QStringLiteral("—"), c.dim, 11); none->setAlignment(Qt::AlignCenter); none->setObjectName("teamBoardEmpty");
            cv->addWidget(none);
        }
        cv->addStretch();
        readableColumn(col);
        h->addWidget(col, 1);
    }
    return box;
}

// U33: a board column is never narrower than a readable card (two lines,
// eliding only beyond that) and never sized by its cards, so the columns stay
// equal and still when a task moves; wider windows share the room.
void TeamWorkspace::readableColumn(QWidget *column)
{
    column->setMinimumWidth(BoardColumnWidth);
    column->setSizePolicy(QSizePolicy::Ignored, column->sizePolicy().verticalPolicy());
}

QWidget *TeamWorkspace::layerRows(const Team &team, const QList<int> &ids)
{
    const Colors c = colorsFor(m_dark);
    const auto problemsByTask = problemTasks(team);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    const auto layers = phaseLayers(team, ids);
    QList<int> order = ids;
    std::stable_sort(order.begin(), order.end(), [&layers](int a, int b) { return layers.value(a) < layers.value(b); });
    auto *box = new QWidget; box->setObjectName("teamLayerRows");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(2);
    auto held = std::make_shared<Team>(team);
    int matches = 0;
    for (int id : order) {
        const auto *task = team.task(id);
        if (!task) continue;
        const bool problem = problemsByTask.contains(id);
        const bool active = QStringList{"assigned", "in_progress", "submitted", "in_review", "in_test"}.contains(task->status);
        const bool match = (m_filter == "all" || team.status == "planning" || (m_filter == "active" && active) || (m_filter == "problems" && problem))
                           && (m_focus.isEmpty() || task->assignee == m_focus);
        if (!match) continue;
        ++matches;
        auto buildRow = [this, held, id, layers, problemsByTask, c, v] {
            const Team &team = *held;
            const auto *task = team.task(id);
            if (!task) return;
            const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
            const bool problem = problemsByTask.contains(id);
        const auto s = taskStatus(*task);
        const auto mark = phaseMark(team, *task);
        auto *line = new ClickFrame; line->setObjectName("teamLayerRow"); line->setProperty("task", id); line->setCursor(Qt::PointingHandCursor);
        line->setMinimumHeight(30);
        line->setStyleSheet(QStringLiteral("QFrame#teamLayerRow{background:%1;border:1px %4 %2;border-radius:8px;} QFrame#teamLayerRow:hover{border-color:%3;}")
                                .arg(c.surface.name(), problem ? toneColor(c, problemsByTask.value(id)).name() : c.cardBorder.name(), c.accent.name(),
                                     task->status == "draft" ? QStringLiteral("dashed") : QStringLiteral("solid")));
        line->setToolTip(QStringLiteral("#%1 %2 — %3").arg(id).arg(task->title, mark.tooltip.isEmpty() ? s.text : mark.tooltip));
        line->clicked = team.status == "planning" ? std::function<void()>([this, id] { if (const auto *t = currentTeam(m_team)) m_panel->editTask(*t, id); })
                                                  : std::function<void()>([this, id] { m_drawerTask = id; scheduleRender(); });
        auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 4, 8, 4); h->setSpacing(8);
        auto *layer = label(QStringLiteral("L%1").arg(layers.value(id) + 1), c.dim, 10, false, "teamLayerLabel"); layer->setFixedWidth(20); h->addWidget(layer);
        auto *state = new StatusMark(QString(), toneColor(c, problem ? problemsByTask.value(id) : mark.tone != Tone::Neutral ? mark.tone : s.tone),
                                     mark.kind.isEmpty() ? kindOfTask(*task, s) : mark.kind, 14, Qt::AlignLeft, nullptr, mark.tooltip.isEmpty() ? s.text : mark.tooltip);
        h->addWidget(state);
        h->addWidget(label(QStringLiteral("#%1").arg(id), c.fg, 12, true));
        if (task->addedLate.known) h->addWidget(outOfScopeMark(*task, c));
        QStringList deps; for (const auto &d : task->deps) deps << QStringLiteral("#%1").arg(d.task);
        if (!deps.isEmpty()) h->addWidget(label(QStringLiteral("←") + deps.join(QStringLiteral(" ")), c.folder, 11));
        auto *name = elided(task->title, c.text2, 13); name->setObjectName("teamLayerTitle"); h->addWidget(name, 3);
        if (problem) { auto *why = elided(problemText(team, id), toneColor(c, problemsByTask.value(id)), 11); why->setObjectName("teamLayerProblem"); h->addWidget(why, 1); }
        else if (const QString activity = activityLabel(*task, team.member(task->assignee) ? live(*team.member(task->assignee)) : Live{}); !activity.isEmpty()) {
            // The same small activity label as the graph and the board.
            auto *a = label(activity, c.text3, 11); a->setObjectName("teamLayerActivity"); a->setProperty("liveTask", id); h->addWidget(a);
        }
        addWorked(h, *task, workedClock()(), c, "teamLayerWorked");
        const QString role = team.assigneeRole(*task, tr("You"));
        // U36: the stage chip line.
        if (auto *stages = stageLineWidget(team, *task, c)) { stages->setMaximumWidth(260); h->addWidget(stages, 2); }
        h->addWidget(new Pill(role, roleColor(role, m_dark)));
        auto *logo = new QLabel; logo->setFixedWidth(16);
        if (const auto *m = team.member(task->assignee)) logo->setPixmap(agentLogo(m->agent, c.text3).pixmap(14, 14));
        h->addWidget(logo);
            v->addWidget(line);
        };
        if (order.size() <= kSmallTeam) buildRow(); else queueSlice(buildRow);
    }
    if (matches == 0) v->addWidget(label(tr("No task matches the filter"), c.dim, 11));
    return box;
}

QWidget *TeamWorkspace::sideLists(const Team &team, const QString &status)
{
    const Colors c = colorsFor(m_dark);
    const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
    auto *box = new QWidget; box->setObjectName("teamSideLists");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 0); v->setSpacing(8);
    // The prototype's .sgrp: a card, warn or danger tinted.
    const auto group = [&](CollapsibleBlock *block, const QColor &tone) {
        auto *frame = new QFrame; frame->setObjectName("teamSideGroup");
        frame->setStyleSheet(QStringLiteral("QFrame#teamSideGroup{background:%1;border:1px solid %2;border-radius:10px;}")
                                 .arg(QColor::fromRgbF(c.cardFill.redF() * .95 + tone.redF() * .05, c.cardFill.greenF() * .95 + tone.greenF() * .05,
                                                       c.cardFill.blueF() * .95 + tone.blueF() * .05).name(),
                                      QColor::fromRgbF(c.cardBorder.redF() * .55 + tone.redF() * .45, c.cardBorder.greenF() * .55 + tone.greenF() * .45,
                                                       c.cardBorder.blueF() * .55 + tone.blueF() * .45).name()));
        auto *fv = new QVBoxLayout(frame); fv->setContentsMargins(8, 4, 8, 4); fv->setSpacing(0);
        block->setDark(m_dark);
        fv->addWidget(block);
        v->addWidget(frame);
    };
    const auto listRow = [&](QVBoxLayout *into, const QString &role, const QString &text, const QString &action, const char *name,
                             const std::function<void()> &run) {
        auto *r = new QWidget; r->setObjectName("teamSideRow");
        auto *h = new QHBoxLayout(r); h->setContentsMargins(0, 2, 0, 2); h->setSpacing(7);
        if (!role.isEmpty()) h->addWidget(new Pill(role, roleColor(role, m_dark)));
        auto *t = elided(text, c.text2, 12); t->setToolTip(text); h->addWidget(t, 1);
        auto *go = button(action, name); QObject::connect(go, &QPushButton::clicked, this, run); h->addWidget(go);
        into->addWidget(r);
    };
    // 0. Needs your attention (owner, 2026-10-28): what the lead left to the
    // owner, open by default, before the answers.
    if (!team.attention.isEmpty()) {
        auto *block = new CollapsibleBlock(QStringLiteral("team/attention"), tr("Needs your attention"), true);
        block->setObjectName("teamAttentionStrip");
        attentionRows(team, block->body());
        block->setNote(QString::number(team.attention.size()));
        if (auto *note = block->findChild<QLabel *>("collapsibleNote"))
            note->setStyleSheet(QStringLiteral("QLabel#collapsibleNote{background:%1;color:%2;border-radius:8px;padding:0 6px;font-size:11px;font-weight:700;}")
                                    .arg(c.warn.name(), c.dark ? QStringLiteral("#1b1300") : QStringLiteral("#ffffff")));
        group(block, c.warn);
    }
    // 1. Needs your answer: questions in sessions, team questions, and a phase draft.
    {
        auto *block = new CollapsibleBlock(QStringLiteral("team/needs"), tr("Needs your answer"), false);
        block->setObjectName("teamWorkspaceAnswers");
        int waiting = 0;
        for (const auto &m : team.members) {
            const auto state = live(m);
            if (m.session.isEmpty() || !state.attention) continue;
            ++waiting;
            listRow(block->body(), team.roleOf(m.id), state.attentionText.isEmpty() ? tr("Question") : state.attentionText, tr("Go to question"),
                    "teamWorkspaceGoToQuestion", [this, host = m.host, session = m.session] { if (openSession) openSession(host, session); });
        }
        for (const auto &task : team.tasks) for (const auto &asked : openQuestions(task)) {
            ++waiting;
            listRow(block->body(), team.roleOf(task.assignee), QStringLiteral("#%1 %2 — %3").arg(task.id).arg(task.title, asked.text), tr("Answer"),
                    "teamWorkspaceAnswerQuestion", [this, id = task.id] { m_drawerTask = id; scheduleRender(); });
        }
        if (team.status != "planning") for (int phase : team.draftPhases()) {
            const auto *record = team.phase(phase);
            ++waiting;
            listRow(block->body(), team.roleOf(team.lead),
                    tr("Phase %1 · %2 — draft waits for your Accept").arg(phase).arg(record ? record->title : QString()), tr("Review"),
                    "teamWorkspaceReviewPhase", [this, phase] { showPhase(phase); });
            break;
        }
        if (waiting) {
            block->setNote(QString::number(waiting));
            // The prototype's count badge.
            if (auto *note = block->findChild<QLabel *>("collapsibleNote"))
                note->setStyleSheet(QStringLiteral("QLabel#collapsibleNote{background:%1;color:%2;border-radius:8px;padding:0 6px;font-size:11px;font-weight:700;}")
                                        .arg(c.warn.name(), c.dark ? QStringLiteral("#1b1300") : QStringLiteral("#ffffff")));
            group(block, c.warn);
        } else delete block;
    }
    // 2. Team stuck: the blockers, with Show on graph.
    if (status == "stuck") {
        const auto found = problems(team, live);
        auto *block = new CollapsibleBlock(QStringLiteral("team/stuck"), tr("Team stuck"), false);
        block->setObjectName("teamStuckBlock");
        block->setNote(found.size() == 1 ? tr("1 blocker") : tr("%1 blockers").arg(found.size()));
        if (auto *note = block->findChild<QLabel *>("collapsibleNote")) note->setStyleSheet(QStringLiteral("QLabel#collapsibleNote{color:%1;font-size:12px;}").arg(c.warn.name()));
        auto *show = button(m_showBlockers ? tr("Hide") : tr("Show on graph"), "teamStuckShowOnGraph"); show->setProperty("ghost", true);
        show->setIcon(workspaceIcon("eye", c.text2)); show->setIconSize(QSize(12, 12));
        QObject::connect(show, &QPushButton::clicked, this, [this] { m_showBlockers = !m_showBlockers; if (m_showBlockers) m_view = QStringLiteral("graph"); scheduleRender(); });
        block->trailing()->addWidget(show);
        block->body()->addWidget(label(tr("Nothing can start until one of these moves:"), c.muted, 11));
        for (const auto &p : found) {
            auto *r = new ClickFrame; r->setObjectName("teamStuckRow"); r->setProperty("task", p.task); r->setCursor(Qt::PointingHandCursor);
            auto *h = new QHBoxLayout(r); h->setContentsMargins(0, 2, 0, 2); h->setSpacing(7);
            const auto *task = team.task(p.task);
            h->addWidget(new StatusMark(QString(), toneColor(c, p.tone), "stuck", 14));
            if (task) { h->addWidget(label(QStringLiteral("#%1").arg(task->id), c.fg, 12, true)); h->addWidget(elided(task->title, c.fg, 12), 1); }
            else h->addStretch(1);
            const int dot = p.text.lastIndexOf(QStringLiteral(" · "));
            auto *why = elided(dot >= 0 ? p.text.mid(dot + 3) : p.text, c.warn, 11); why->setAlignment(Qt::AlignRight | Qt::AlignVCenter); h->addWidget(why, 1);
            r->setToolTip(p.text);
            if (p.task) r->clicked = [this, id = p.task] { m_drawerTask = id; scheduleRender(); };
            block->body()->addWidget(r);
        }
        group(block, c.warn);
    }
    // 3. Plan problems: in planning, each error opens its task.
    if (team.status == "planning" && (!team.errorItems.isEmpty() || !team.warningItems.isEmpty())) {
        auto *block = new CollapsibleBlock(QStringLiteral("team/plan"), tr("Plan problems"), false);
        block->setObjectName("teamPlanProblemsBlock");
        QStringList note;
        if (!team.errorItems.isEmpty()) note << (team.errorItems.size() == 1 ? tr("1 error") : tr("%1 errors").arg(team.errorItems.size()));
        if (!team.warningItems.isEmpty()) note << (team.warningItems.size() == 1 ? tr("1 warning") : tr("%1 warnings").arg(team.warningItems.size()));
        block->setNote(note.join(QStringLiteral(" · ")));
        if (auto *label = block->findChild<QLabel *>("collapsibleNote"))
            label->setStyleSheet(QStringLiteral("QLabel#collapsibleNote{color:%1;font-size:12px;}").arg((team.errorItems.isEmpty() ? c.warn : c.danger).name()));
        for (const auto &[issues, tone] : {std::pair{team.errorItems, c.danger}, std::pair{team.warningItems, c.warn}}) {
            for (const auto &issue : issues) {
                auto *r = new ClickFrame; r->setObjectName("teamPlanProblem"); r->setProperty("task", issue.task);
                auto *h = new QHBoxLayout(r); h->setContentsMargins(4, 3, 4, 3); h->setSpacing(7);
                auto *icon = new QLabel; icon->setPixmap(workspaceIcon("warning", tone).pixmap(12, 12)); h->addWidget(icon);
                auto *t = elided(issue.text, tone, 12); agedHealth(t, issue.text, issue.text); h->addWidget(t, 1);
                if (issue.task) {
                    r->setCursor(Qt::PointingHandCursor); r->setToolTip(tr("Open the task"));
                    r->clicked = [this, id = issue.task] { if (const auto *t = currentTeam(m_team)) m_panel->editTask(*t, id); };
                }
                block->body()->addWidget(r);
            }
        }
        group(block, team.errorItems.isEmpty() ? c.warn : c.danger);
    }
    if (v->count() == 0) { delete box; return nullptr; }
    return box;
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
        m_membersOpen = !m_membersOpen; Settings::set("teams/membersOpen", m_membersOpen); scheduleRender();
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
    // Spec 5.6: the members as a tree - each sub-lead followed by its reports,
    // indented with their lines; a folded sub-lead hides them.
    auto *tree = new QWidget; tree->setObjectName("teamWorkspaceMemberTree");
    auto *treeBox = new QVBoxLayout(tree); treeBox->setContentsMargins(0, 0, 0, 0); treeBox->setSpacing(0);
    v->addWidget(tree);
    int foldedBelow = -1;
    bool firstCard = true;
    for (const auto &node : team.memberTree()) {
        const auto &m = *node.member;
        if (foldedBelow >= 0 && node.depth > foldedBelow) continue;
        foldedBelow = m.sub && m_subFolded.contains(m.id) ? node.depth : -1;
        // F7: a session left running after the team ended is shown apart below.
        if (finished && leftMember(team, m.id)) continue;
        const auto state = live(m);
        const auto status = memberStatus(team, m, state);
        auto *b = new MemberCard; b->setObjectName("teamWorkspaceMember"); b->setCursor(Qt::PointingHandCursor); b->setProperty("teamMember", m.id);
        // U25: the prototype's .mcard: the card colour, accent and the selected fill
        // when focused, a problem's tone mixed into the border; spec 5.6: a
        // sub-lead's edge leans to its role's color.
        const QColor subEdge = colorMix(roleColor(QStringLiteral("leadgpt"), m_dark), c.cardBorder, 0.4);
        const QColor edge = m.id == m_focus ? c.accent
            : status.problem ? colorMix(toneColor(c, status.tone), c.cardBorder, 0.6)
            : m.sub ? subEdge : c.cardBorder;
        const QColor fill = m.id == m_focus ? WorkspaceSkin::color(WorkspaceSkin::active(), m_dark, QStringLiteral("cardSel")) : c.cardFill;
        b->setStyleSheet(QStringLiteral("QFrame#teamWorkspaceMember{background:%1;border:1px solid %2;border-radius:10px;}"
                                        "QFrame#teamWorkspaceMember:hover{background:%3;}")
                             .arg(fill.name(), edge.name(), m.id == m_focus ? fill.name() : WorkspaceSkin::color(WorkspaceSkin::active(), m_dark, QStringLiteral("cardHover")).name()));
        QString tip = m.id == m_focus ? tr("Show every member") : tr("Show only %1's tasks").arg(team.roleOf(m.id));
        if (const QString boss = team.reviewerOf(m.id); !boss.isEmpty() && boss != team.lead)
            tip += QStringLiteral("\n") + tr("Reports to %1").arg(team.roleOf(boss));
        b->setToolTip(tip);
        b->clicked = [this, id = m.id] { m_focus = m_focus == id ? QString() : id; scheduleRender(); };
        auto *bh = new QHBoxLayout(b); bh->setContentsMargins(8, 6, 6, 6); bh->setSpacing(8);
        auto *tile = new QLabel; tile->setPixmap(agentLogo(m.agent, c.fg).pixmap(16, 16)); bh->addWidget(tile, 0, Qt::AlignTop);
        auto *col = new QVBoxLayout; col->setSpacing(2);
        auto *top = row(4);
        const QString role = team.roleOf(m.id);
        top->addWidget(new Pill(role, roleColor(role, m_dark)));
        if (m.sub) {
            // Spec 5.6: "▾ sub-lead · N reports" folds its reports.
            const qsizetype reports = team.reportsOf(m.id).size();
            const bool folded = m_subFolded.contains(m.id);
            const QColor sub = roleColor(QStringLiteral("leadgpt"), m_dark);
            auto *pill = new QToolButton; pill->setObjectName("teamWorkspaceSubLead"); pill->setProperty("teamMember", m.id);
            pill->setToolButtonStyle(Qt::ToolButtonTextBesideIcon); pill->setCursor(Qt::PointingHandCursor);
            pill->setIcon(workspaceIcon(folded ? "chevron-right" : "chevron-down", sub)); pill->setIconSize(QSize(10, 10));
            pill->setText(reports == 1 ? tr("sub-lead · 1 report") : tr("sub-lead · %1 reports").arg(reports));
            pill->setToolTip(tr("Show or hide its reports"));
            pill->setProperty("folded", folded);
            QColor fill = sub; fill.setAlphaF(0.14);
            pill->setStyleSheet(QStringLiteral("QToolButton#teamWorkspaceSubLead{background:%1;color:%2;border:0;border-radius:6px;"
                                               "padding:0 7px 0 5px;font-size:11px;min-height:19px;max-height:19px;}")
                                    .arg(fill.name(QColor::HexArgb), sub.name()));
            QObject::connect(pill, &QToolButton::clicked, this, [this, id = m.id] {
                if (!m_subFolded.remove(id)) m_subFolded.insert(id);
                scheduleRender();
            });
            top->addWidget(pill);
        }
        if (state.attention) { auto *q = label(QStringLiteral("?1"), c.warn, 10, true); q->setToolTip(tr("A question waits in its session")); top->addWidget(q); }
        top->addWidget(elided(m.model.isEmpty() ? agentNames.value(m.agent, m.agent) : m.model, c.text3, 11), 1);
        // U54: the account its session really runs on, not only the configured one.
        if (team.status != "draft") {
            const QString account = Teams::AccountMove::memberAccount(m);
            // It keeps its width beside the model, up to 120 pixels.
            auto *shown = label(accountLabel(m), c.muted, 11, false, "teamWorkspaceMemberAccount");
            shown->setText(shown->fontMetrics().elidedText(accountLabel(m), Qt::ElideRight, 120));
            shown->setProperty("account", account);
            QString tip = m.session.isEmpty() || !m.sessionAccountKnown ? tr("Account: %1").arg(accountLabel(m))
                                                                         : tr("Its session runs on %1").arg(accountLabel(m));
            if (m.sessionAccountKnown && !m.session.isEmpty() && Teams::AccountMove::accountOf(m.agent, m.account) != account)
                tip += QLatin1Char('\n') + tr("The roster names %1; the session runs on another account.")
                                               .arg(Teams::AccountMove::labelOf(m_accountProfiles, m.agent, m.account));
            shown->setToolTip(tip);
            top->addWidget(shown);
        }
        col->addLayout(top);
        int done = 0, total = 0, open = 0;
        for (const auto &t : team.tasks) if (t.assignee == m.id) { ++total; done += t.status == "accepted"; open += !t.finished(); }
        // A member added to a running team waits for its first task.
        const bool waiting = !m.lead && m.session.isEmpty() && !open && m.error.isEmpty() && (team.status == "running" || team.status == "paused");
        if (waiting) {
            auto *wait = row(6);
            wait->addWidget(label(tr("Waiting for a task"), c.muted, 11));
            auto *assign = button(tr("Assign task…"), "teamWorkspaceAssign"); assign->setProperty("ghost", true);
            QObject::connect(assign, &QPushButton::clicked, this, [this, id = m.id, assign] { if (const auto *t = currentTeam(m_team)) assignMenu(*t, id, assign->mapToGlobal(QPoint(0, assign->height()))); });
            auto *ask = button(tr("Ask lead to plan for it"), "teamWorkspaceAskLead"); ask->setProperty("ghost", true);
            QObject::connect(ask, &QPushButton::clicked, this, [this, role] {
                m_conversation->send(tr("I added %1 to the team. Plan a task for it and reassign it with hgs team reassign.").arg(role));
            });
            wait->addWidget(assign); wait->addWidget(ask); wait->addStretch();
            col->addLayout(wait);
        } else {
            // A1: the attempt's phase icon and tooltip beside the member.
            const auto mark = memberMark(team, m, status, c);
            auto *state = new StatusMark(mark.text, mark.color, mark.kind, 0, Qt::AlignLeft, nullptr, mark.tooltip);
            state->setObjectName("teamWorkspaceMemberState");
            state->setProperty("phase", mark.phase);
            col->addWidget(state);
        }
        // U54: its step in a move this desktop runs, or what keeps it from its
        // move or launch, with the review action.
        if (auto *block = memberBlock(team, m)) col->addWidget(block);
        // U36: the stages it has and may take.
        if (team.status == "running" || team.status == "paused")
            if (auto *stages = memberStageQueue(team, m, c, workedClock()(), [this](int task) { m_drawerTask = task; scheduleRender(); }))
                col->addWidget(stages);
        // A5 and U25 (spec 5.7): the member's open tasks, the current one first,
        // folded by default behind "N tasks"; the choice is kept per member.
        if (!m.queue.isEmpty()) {
            const QString key = QStringLiteral("ui/collapsed/teamMemberTasks/%1/%2").arg(team.id, m.id);
            const bool tasksOpen = Settings::value(key, false).toBool();
            auto *tasks = new QToolButton; tasks->setObjectName("teamMemberTasksToggle"); tasks->setProperty("member", m.id);
            tasks->setText(m.queue.size() == 1 ? tr("1 task") : tr("%1 tasks").arg(m.queue.size()));
            tasks->setIcon(workspaceIcon(tasksOpen ? "chevron-down" : "chevron-right", c.text2)); tasks->setIconSize(QSize(10, 10));
            tasks->setToolButtonStyle(Qt::ToolButtonTextBesideIcon); tasks->setAutoRaise(true); tasks->setCheckable(true); tasks->setChecked(tasksOpen);
            tasks->setAccessibleDescription(tasksOpen ? tr("Expanded") : tr("Collapsed"));
            tasks->setStyleSheet(QStringLiteral("QToolButton#teamMemberTasksToggle{color:%1;font-size:12px;border:0;padding:1px 4px;background:transparent;}"
                                                "QToolButton#teamMemberTasksToggle:hover{color:%2;}").arg(c.text2.name(), c.fg.name()));
            QObject::connect(tasks, &QToolButton::clicked, this, [this, key](bool open) { Settings::set(key, open); scheduleRender(); });
            col->addWidget(tasks, 0, Qt::AlignLeft);
            if (tasksOpen) {
                auto *queue = new QWidget; queue->setObjectName("teamMemberQueue"); queue->setProperty("member", m.id);
                auto *qv = new QVBoxLayout(queue); qv->setContentsMargins(0, 0, 0, 0); qv->setSpacing(1);
                for (int i = 0; i < qMin(5, int(m.queue.size())); ++i) {
                    const auto *task = team.task(m.queue[i]);
                    if (!task) continue;
                    auto *line = memberQueueRow(team, *task, c, i == 0);
                    QObject::connect(line, &QPushButton::clicked, this, [this, id = task->id] { m_drawerTask = id; scheduleRender(); });
                    qv->addWidget(line);
                }
                if (m.queue.size() > 5) qv->addWidget(label(tr("+%1 more").arg(m.queue.size() - 5), c.dim, 11, false, "teamMemberQueueMore"));
                col->addWidget(queue);
            }
        }
        // G7: the MCP servers and services its session runs, as an icon with a
        // tooltip - they outlive the session and are not Background work.
        // C12: the declared time budget of its background job.
        if (!m.services.isEmpty() || m.budget.known || m.limited.known || m.hostShared || m.draining) {
            auto *extra = row(6);
            // U49: it finishes its open work, takes nothing new, then leaves.
            if (m.draining) {
                auto *draining = new StatusMark(tr("Draining"), c.warn, "paused", 0, Qt::AlignLeft, nullptr,
                                                tr("Finishes its open work, gets nothing new, then its session is paused and it leaves the team. Cancel drain in its menu."));
                draining->setObjectName("teamWorkspaceMemberDraining");
                extra->addWidget(draining);
            }
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
            box->setProperty("team", team.id); box->setProperty("question", m.permission.id); box->setProperty("questionHash", m.permission.hash);
            box->setStyleSheet(QStringLiteral("QFrame#teamMemberPermission{background:%1;border:1px solid %2;border-radius:7px;}")
                                   .arg(alpha(c.warn, .10).name(QColor::HexArgb), alpha(c.warn, .40).name(QColor::HexArgb)));
            auto *pv = new QVBoxLayout(box); pv->setContentsMargins(8, 5, 8, 6); pv->setSpacing(3);
            auto *head = row(6);
            const bool subagents = m.permission.subagents;
            if (!m.permission.tool.isEmpty()) head->addWidget(new Pill(m.permission.tool, c.warn));
            if (subagents) {
                // U19 (#520): "Start N subagents", one row each, Allow all / Deny.
                const auto calls = SubagentRequest::calls(m.permission.record, m.permission.reason);
                auto *title = label(SubagentRequest::title(SubagentRequest::count(m.permission.record, calls)), c.fg, 12, true, "teamMemberSubagentsTitle");
                head->addWidget(title, 1);
                pv->addLayout(head);
                for (const auto &call : calls) {
                    auto *line = new QWidget; line->setObjectName("teamMemberSubagent"); line->setProperty("subagentType", call.type);
                    auto *lv = new QVBoxLayout(line); lv->setContentsMargins(6, 2, 0, 2); lv->setSpacing(1);
                    auto *top = row(6);
                    top->addWidget(label(call.type.isEmpty() ? tr("Subagent") : call.type, c.fg, 11, true, "teamMemberSubagentType"));
                    QStringList meta;
                    if (!call.model.isEmpty()) meta << tr("model %1").arg(call.model);
                    if (!call.effort.isEmpty()) meta << tr("effort %1").arg(call.effort);
                    top->addWidget(elided(meta.join(QStringLiteral(" · ")), c.text3, 10), 1);
                    lv->addLayout(top);
                    if (!call.description.isEmpty()) {
                        auto *task = label(call.description, c.text2, 11, false, "teamMemberSubagentTask"); task->setWordWrap(true);
                        lv->addWidget(task);
                    }
                    pv->addWidget(line);
                }
            } else {
                head->addWidget(label(m.permission.commandSource == "process" ? tr("command from the running process")
                                  : m.permission.commandSource == "tool_input" ? tr("command from the tool input")
                                  : tr("the agent did not show the command"), c.text3, 10), 1);
                pv->addLayout(head);
            }
            if (!subagents && !m.permission.command.isEmpty()) {
                auto *command = label(m.permission.command, c.fg, 11, false, "teamMemberPermissionCommand");
                command->setFont(NewSessionDense::mono(11));
                command->setWordWrap(true);
                command->setTextInteractionFlags(Qt::TextSelectableByMouse);
                command->setToolTip(m.permission.command);
                pv->addWidget(command);
            }
            if (!subagents && !m.permission.reason.isEmpty()) {
                auto *reason = label(tr("Agent: %1").arg(m.permission.reason), c.text2, 11, false, "teamMemberPermissionReason");
                reason->setWordWrap(true);
                pv->addWidget(reason);
            }
            auto *answerRow = row(6);
            auto *reasonEdit = new QLineEdit; reasonEdit->setObjectName("teamMemberPermissionDenyReason");
            reasonEdit->setPlaceholderText(tr("Why deny? The member receives this"));
            answerRow->addWidget(reasonEdit, 1);
            auto *deny = button(tr("Deny"), "teamMemberPermissionDeny"); deny->setProperty("danger", true);
            auto *allow = button(subagents ? tr("Allow all") : tr("Allow"), "teamMemberPermissionAllow", true);
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
        // U17: the stored answers fold to one summary line and open to the
        // newest five; the choice is kept for the team and the member.
        if (!m.permissionAnswers.isEmpty()) {
            const QString key = QStringLiteral("ui/expanded/teamPermissions/%1/%2").arg(team.id, m.id);
            const bool open = Settings::value(key, false).toBool();
            auto *summary = permissionToggle(m.permissionAnswers, c, open, "teamMemberPermissionsToggle");
            summary->setProperty("member", m.id);
            QObject::connect(summary, &QToolButton::clicked, this, [this, key](bool expanded) {
                Settings::set(key, expanded);
                scheduleRender();
            });
            col->addWidget(summary, 0, Qt::AlignLeft);
            if (open) {
                auto *list = permissionAnswerList(m.permissionAnswers, c, QDateTime::currentSecsSinceEpoch(), 5, "teamMemberPermissions");
                list->setProperty("member", m.id);
                col->addWidget(list);
            }
        }
        bh->addLayout(col, 1);
        auto *right = new QVBoxLayout; right->setSpacing(3);
        auto *ratio = label(QStringLiteral("%1/%2").arg(done).arg(total), c.muted, 11); ratio->setFont(NewSessionDense::mono(11)); ratio->setAlignment(Qt::AlignRight);
        right->addWidget(ratio);
        // The prototype's .seg8: eight 4 px squares, done in accentMuted.
        QList<QColor> parts; for (int i = 0; i < 8; ++i) parts << (total > 0 && i < done * 8 / total ? c.accentMuted : c.border);
        auto *bar = new Progress(parts, 4, c.border); bar->setFixedWidth(8 * 4 + 7 * 2); bar->setObjectName("teamMemberProgress"); right->addWidget(bar, 0, Qt::AlignRight);
        bh->addLayout(right);
        auto *more = new QToolButton; more->setIcon(workspaceIcon("more", c.muted)); more->setAutoRaise(true); more->setObjectName("teamWorkspaceMemberMenu");
        more->setToolTip(tr("Member actions"));
        QObject::connect(more, &QToolButton::clicked, this, [this, id = m.id, more] {
            const auto *t = currentTeam(m_team);
            if (const auto *member = t ? t->member(id) : nullptr) memberMenu(*t, *member, more->mapToGlobal(QPoint(0, more->height())));
        });
        bh->addWidget(more, 0, Qt::AlignTop);
        // The lead and its direct reports stay flush; a sub-lead's reports go
        // one level in, with the line from their sub-lead.
        treeBox->addWidget(new TeamTreeIndent(b, qMax(0, node.depth - 1), node.last, node.through.mid(1), c.border, firstCard));
        firstCard = false;
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
        // U49: more members of this role at once, and a drain: the member
        // finishes its open work, takes nothing new and then leaves.
        if (team.status == "planning" || team.status == "running" || team.status == "paused") {
            const int room = scaleRoom(team);   // #581 L2: the lead takes a seat too
            auto *scale = menu.addAction(tr("Add more %1…").arg(role), this, [this, teamId = team.id, id = member.id] {
                QTimer::singleShot(0, this, [this, teamId, id] {
                    const auto *current = currentTeam(teamId);
                    if (const auto *found = current ? current->member(id) : nullptr) scaleRole(*current, *found);
                });
            });
            scale->setObjectName("teamWorkspaceMemberScale");
            scale->setEnabled(room > 0);
            scale->setToolTip(room > 0 ? tr("Adds members like this one: the same role, agent and model")
                                       : tr("The team has its %1 members; raise its limit first").arg(team.maxMembers));
            if (member.draining) {
                auto *cancel = menu.addAction(tr("Cancel drain"), this, [this, id = member.id] { m_panel->act("drain", {{"member", id}, {"draining", false}}); });
                cancel->setObjectName("teamWorkspaceMemberUndrain");
                cancel->setToolTip(tr("It takes tasks again"));
            } else {
                auto *drain = menu.addAction(tr("Drain…"), this, [this, id = member.id, role] {
                    if (m_panel->confirm("teamDrainMemberConfirm", tr("Drain member?"),
                                         tr("%1 finishes its current task and review, gets nothing new, then its session is paused and it leaves the team.").arg(role),
                                         tr("Drain")))
                        m_panel->act("drain", {{"member", id}});
                });
                drain->setObjectName("teamWorkspaceMemberDrain");
                drain->setToolTip(tr("Finish the open work, then leave"));
            }
        }
        QStringList open;
        for (const auto &t : team.tasks) if (t.assignee == member.id && !t.finished()) open << QStringLiteral("#%1").arg(t.id);
        auto *remove = menu.addAction(tr("Remove from team…"), this, [this, id = member.id, role, name = team.name] {
            if (m_panel->confirm("teamRemoveMemberConfirm", tr("Remove member?"), tr("Remove %1 from %2? Its session keeps running; it gets no more tasks.").arg(role, name), tr("Remove")))
                m_panel->act("remove-member", {{"member", id}});
        });
        remove->setEnabled(open.isEmpty() && team.reportsOf(member.id).isEmpty());
        if (!open.isEmpty()) remove->setToolTip(tr("Has open tasks %1 — reassign or cancel them first").arg(open.join(QStringLiteral(", "))));
        else if (!team.reportsOf(member.id).isEmpty()) remove->setToolTip(tr("Move its reports to another lead first"));
    }
    // Spec 5.6: "Reports to…" the lead or a sub-lead (never itself or one
    // below it), and "Make sub-lead" / "Stop being a sub-lead".
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *reportsTo = menu.addMenu(tr("Reports to…"));
        reportsTo->setObjectName("teamWorkspaceReportsTo");
        reportsTo->setEnabled(!member.lead);
        if (member.lead) reportsTo->setToolTip(tr("The coordinator reports to you"));
        QSet<QString> below{member.id};
        for (const auto &node : team.memberTree())
            if (below.contains(team.reviewerOf(node.member->id))) below.insert(node.member->id);
        const QString current = team.reviewerOf(member.id);
        for (const auto &node : team.memberTree()) {
            const Member &boss = *node.member;
            if (!(boss.lead || boss.sub) || below.contains(boss.id)) continue;
            const QString name = boss.lead ? tr("%1 (coordinator)").arg(team.roleOf(boss.id)) : tr("%1 (sub-lead)").arg(team.roleOf(boss.id));
            auto *choice = reportsTo->addAction(name, this, [this, id = member.id, parent = boss.id] {
                m_panel->act("reports-to", {{"member", id}, {"parent", parent}});
            });
            choice->setCheckable(true);
            choice->setChecked(boss.id == current);
            choice->setProperty("parent", boss.id);
        }
        const bool hasReports = !team.reportsOf(member.id).isEmpty();
        auto *sub = menu.addAction(member.sub ? tr("Stop being a sub-lead") : tr("Make sub-lead"), this, [this, id = member.id, on = !member.sub] {
            m_panel->act("sub-lead", {{"member", id}, {"sub", on}});
        });
        sub->setObjectName("teamWorkspaceSubLeadToggle");
        sub->setEnabled(!member.lead && !(member.sub && hasReports));
        if (member.lead) sub->setToolTip(tr("Already the coordinator"));
        else if (member.sub && hasReports) sub->setToolTip(tr("Move its reports to another lead first"));
    }
    // H6: swap the session, agent, model or account of any member, the lead too.
    if (team.status == "planning" || team.status == "running" || team.status == "paused") {
        auto *swap = menu.addAction(tr("Swap session, agent or model…"), this, [this, teamId = team.id, id = member.id] {
            const auto *current = currentTeam(teamId);
            if (const auto *found = current ? current->member(id) : nullptr) swapDialog(*current, *found);
        });
        swap->setObjectName("teamWorkspaceMemberSwap");
        swap->setToolTip(tr("Keeps its role, tasks, folder and branch; an open task goes to the new session with what was done so far"));
        // U54: every member of this agent on one account moves at once.
        auto *move = menu.addAction(tr("Move to another account…"), this, [this, teamId = team.id, id = member.id] {
            QTimer::singleShot(0, this, [this, teamId, id] {
                const auto *current = currentTeam(teamId);
                if (const auto *found = current ? current->member(id) : nullptr) accountMoveDialog(*current, *found);
            });
        });
        move->setObjectName("teamWorkspaceMemberMoveAccount");
        const bool movable = Teams::AccountMove::movable(member.agent) && member.host.isEmpty() && !member.session.isEmpty();
        move->setEnabled(movable && !m_move.running);
        move->setToolTip(m_move.running ? tr("A move is running; wait for it")
                         : !Teams::AccountMove::movable(member.agent) ? tr("Only Claude and Codex members change account in place")
                         : !member.host.isEmpty() ? tr("Its session runs on another computer")
                         : member.session.isEmpty() ? tr("It has no session yet; its account applies when it starts")
                         : tr("Moves every %1 member on its account to another one, the lead included").arg(agentNames.value(member.agent, member.agent)));
    }
    menu.setToolTipsVisible(true);
    // The menu runs its own event loop: a poll under it must not rebuild the
    // members column and delete the "…" button whose click opened it.
    TeamPanel::RenderHold hold(m_panel);
    menu.exec(global);
}

void TeamWorkspace::scaleRole(const Team &team, const Member &member)
{
    const int room = scaleRoom(team);
    if (room < 1) return;
    const QString role = team.roleOf(member.id);
    bool ok = false;
    int count = 0;
    {
        TeamPanel::RenderHold hold(m_panel);
        const QString agent = agentNames.value(member.agent, member.agent);
        const QString text = tr("How many more %1 members, each on %2?").arg(role, member.model.isEmpty() ? agent : agent + QLatin1Char(' ') + member.model);
        count = QInputDialog::getInt(this, tr("Add more %1").arg(role), text, 1, 1, room, 1, &ok);
    }
    if (!ok || count < 1) return;
    QJsonObject input{{"role", member.role}, {"agent", member.agent}, {"count", count}};
    if (!member.model.isEmpty()) input.insert("model", member.model);
    if (!member.effort.isEmpty()) input.insert("effort", member.effort);
    if (!member.account.isEmpty()) input.insert("account", member.account);
    m_panel->act("scale", input);
}

void TeamWorkspace::teamSettings(const Team &team)
{
    // The watched copy (`team status`) carries the intake keys a list poll lacks.
    TeamSettingsDialog dialog(m_teams, team, m_dark, this);
    TeamPanel::RenderHold hold(m_panel);
    if (dialog.exec() == QDialog::Accepted) m_teams->refresh();
}

void TeamWorkspace::openOtherTeam(const QString &team)
{
    if (team.isEmpty()) return;
    m_teams->refresh();
    m_teams->watch(team);
    showTeam(team);
}

void TeamWorkspace::nextGoalDialog(const Team &team)
{
    if (auto *open = findChild<QDialog *>("teamNextGoalDialog")) { open->raise(); open->activateWindow(); return; }
    const Colors c = colorsFor(m_dark);
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamNextGoalDialog");
    dialog->setStyleSheet(dialogSheet(c));   // #737
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

void TeamWorkspace::addLateDialog(const Team &team, int phase)
{
    if (auto *open = findChild<QDialog *>("teamAddLateDialog")) { open->raise(); open->activateWindow(); return; }
    const Colors c = colorsFor(m_dark);
    auto *dialog = new QDialog(this);
    dialog->setObjectName("teamAddLateDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    // U59: the open free phase takes a task as it comes: no warning, no typed
    // confirmation; it starts when its member is free.
    const bool free = freeOpenPhase(team, phase);
    dialog->setProperty("free", free);
    dialog->setWindowTitle(free ? tr("Add a task to open phase %1").arg(phase) : tr("Add a task to running phase %1").arg(phase));
    dialog->resize(520, 420);
    dialog->setStyleSheet(dialogSheet(c));
    auto *v = new QVBoxLayout(dialog);
    auto *intro = label(free ? tr("Phase %1 is open. The task joins it and starts when its member is free.").arg(phase)
                             : tr("Phase %1 is running. The task joins it only after Zerus warns what changes and you confirm.").arg(phase), c.text2, 12);
    intro->setWordWrap(true); v->addWidget(intro);
    v->addWidget(label(tr("Title"), c.muted, 11));
    auto *title = new QLineEdit; title->setObjectName("teamAddLateTitle"); title->setMaxLength(200);
    title->setPlaceholderText(tr("What the task delivers"));
    v->addWidget(title);
    v->addWidget(label(tr("Description"), c.muted, 11));
    auto *description = new QPlainTextEdit; description->setObjectName("teamAddLateDescription");
    description->setPlaceholderText(tr("What to do and how it is checked"));
    v->addWidget(description, 1);
    v->addWidget(label(tr("Assignee"), c.muted, 11));
    auto *assignee = new QComboBox; assignee->setObjectName("teamAddLateAssignee");
    for (const auto &member : team.members) if (!member.lead && member.state != "removed") assignee->addItem(team.roleOf(member.id), member.id);
    if (const auto *lead = team.member(team.lead)) assignee->addItem(team.roleOf(lead->id), lead->id);
    v->addWidget(assignee);
    // Owner 2026-10-31 (#746): the estimate in minutes; From experience (0)
    // leaves it to the team's solved tasks.
    v->addWidget(label(tr("Estimate"), c.muted, 11));
    auto *estimate = new QSpinBox; estimate->setObjectName("teamAddLateEstimate");
    estimate->setRange(0, 1440); estimate->setSuffix(tr(" min")); estimate->setSingleStep(5);
    estimate->setSpecialValueText(tr("From experience"));
    estimate->setToolTip(tr("Your estimate in minutes; From experience uses the median of this role's solved tasks"));
    v->addWidget(estimate);
    const auto withEstimate = [estimate](QJsonObject task) {
        if (estimate->value() > 0) task.insert("estimate_minutes", estimate->value());
        return task;
    };
    // The warning and the confirmation appear once Zerus answered the request.
    auto *warning = label(QString(), c.warn, 12, false, "teamAddLateWarning"); warning->setWordWrap(true); warning->hide();
    warning->setStyleSheet(QStringLiteral("QLabel{color:%1;background:%2;border:1px solid %3;border-radius:8px;padding:8px;}")
                               .arg(c.warn.name(), alpha(c.warn, .10).name(QColor::HexArgb), alpha(c.warn, .45).name(QColor::HexArgb)));
    v->addWidget(warning);
    auto *confirm = new QLineEdit; confirm->setObjectName("teamAddLateConfirm"); confirm->hide();
    confirm->setPlaceholderText(tr("Type add or %1 to confirm").arg(phase));
    v->addWidget(confirm);
    auto *error = label(QString(), c.danger, 11, false, "teamAddLateError"); error->setWordWrap(true); error->hide();
    v->addWidget(error);
    auto *buttons = new QDialogButtonBox;
    auto *next = buttons->addButton(tr("Continue"), QDialogButtonBox::AcceptRole); next->setObjectName("teamAddLateContinue");
    auto *add = buttons->addButton(tr("Add to phase %1").arg(phase), QDialogButtonBox::AcceptRole); add->setObjectName("teamAddLateAdd"); add->hide();
    buttons->addButton(QDialogButtonBox::Cancel);
    v->addWidget(buttons);
    next->setEnabled(false);
    QObject::connect(title, &QLineEdit::textChanged, dialog, [next, title] { next->setEnabled(!title->text().trimmed().isEmpty()); });
    if (free) {
        next->hide(); add->show(); add->setEnabled(false);
        QObject::connect(title, &QLineEdit::textChanged, dialog, [add, title] { add->setEnabled(!title->text().trimmed().isEmpty()); });
        int top = 0; for (const auto &task : team.tasks) top = qMax(top, task.id);
        QObject::connect(add, &QPushButton::clicked, dialog, [this, dialog, title, description, assignee, error, add, withEstimate, id = team.id, taskId = top + 1] {
            add->setEnabled(false);
            const QJsonObject task = withEstimate({{"id", taskId}, {"title", title->text().trimmed()}, {"description", description->toPlainText().trimmed()},
                                                   {"assignee", assignee->currentData().toString()}});
            m_teams->call(QStringLiteral("extend"), {{"team", id}, {"tasks", QJsonArray{task}}},
                [guard = QPointer<QDialog>(dialog), error, add](bool ok, const QJsonObject &, const QString &message) {
                    if (!guard) return;
                    if (!ok) { error->setText(message); error->show(); add->setEnabled(true); return; }
                    guard->accept();
                });
        });
        QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
        dialog->show();
        return;
    }
    QObject::connect(confirm, &QLineEdit::textChanged, dialog, [add, confirm, phase] {
        const QString typed = confirm->text().trimmed().toLower();
        add->setEnabled(typed == QLatin1String("add") || typed == QString::number(phase));
    });
    QObject::connect(buttons, &QDialogButtonBox::rejected, dialog, &QDialog::reject);
    auto pending = std::make_shared<QString>();
    int nextId = 0; for (const auto &task : team.tasks) nextId = qMax(nextId, task.id);
    QObject::connect(next, &QPushButton::clicked, dialog, [this, dialog, title, description, assignee, warning, confirm, error, next, add, pending,
                                                            withEstimate, id = team.id, phase, taskId = nextId + 1] {
        next->setEnabled(false);
        const QJsonObject task = withEstimate({{"id", taskId}, {"title", title->text().trimmed()}, {"description", description->toPlainText().trimmed()},
                                               {"assignee", assignee->currentData().toString()}});
        m_teams->call(QStringLiteral("add-to-phase"), {{"team", id}, {"phase", phase}, {"tasks", QJsonArray{task}}},
            [guard = QPointer<QDialog>(dialog), title, description, assignee, warning, confirm, error, next, add, pending](bool ok, const QJsonObject &result, const QString &message) {
                if (!guard) return;
                if (!ok) { error->setText(message); error->show(); next->setEnabled(true); return; }
                *pending = result.value("pending").toString();
                error->hide();
                warning->setText(result.value("warning").toString()); warning->show();
                for (QWidget *field : std::initializer_list<QWidget *>{title, description, assignee}) field->setEnabled(false);
                next->hide(); confirm->show(); add->show(); add->setEnabled(false); confirm->setFocus();
            });
    });
    QObject::connect(add, &QPushButton::clicked, dialog, [this, dialog, confirm, error, add, pending, id = team.id, phase] {
        add->setEnabled(false);
        m_teams->call(QStringLiteral("add-to-phase"), {{"team", id}, {"phase", phase}, {"confirm", *pending}, {"confirmation", confirm->text().trimmed()}},
            [guard = QPointer<QDialog>(dialog), error, add](bool ok, const QJsonObject &, const QString &message) {
                if (!guard) return;
                if (!ok) { error->setText(message); error->show(); add->setEnabled(true); return; }
                guard->accept();
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
    dialog->setStyleSheet(dialogSheet(c));   // #737
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
    TeamPanel::RenderHold hold(m_panel);
    menu.exec(global);
}

void TeamWorkspace::setWaitingBanner(QWidget *banner)
{
    m_conversation->setBanner(banner);
}

void TeamWorkspace::renderCoordinator(const Team &team)
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
    m_proposalCards->setTeam(&team);
    // U35: the coordinator has no quick-ask chips; the owner writes to the lead.
    m_conversation->setSession(lead ? lead->host : QString(), lead ? lead->session : QString());
    // U53: the lead is a member; its account changes only with swap-member.
    m_conversation->setAccountLocked(tr("The lead of %1: its account changes with Swap member in the team's Members.").arg(team.name));
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
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
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
    // Spec 5.6: "Reports to" the coordinator or one of its sub-leads.
    auto *reportsTo = new QComboBox; reportsTo->setObjectName("teamAddMemberReportsTo");
    for (const auto &node : team.memberTree()) {
        const Member &boss = *node.member;
        if (boss.lead) reportsTo->addItem(tr("%1 (coordinator)").arg(team.roleOf(boss.id)), QString());
        else if (boss.sub) reportsTo->addItem(tr("%1 (sub-lead)").arg(team.roleOf(boss.id)), boss.id);
    }
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
    field(3, tr("Reports to"), reportsTo);
    field(4, tr("Computer"), computer);
    field(5, tr("Permissions"), permissions);
    field(6, tr("Prompt"), prompt);
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
    QObject::connect(ok, &QPushButton::clicked, pop, [this, team, picked, agent, model, effort, account, computer, prompt, reportsTo] {
        const auto *role = m_teams->catalog().role(*picked);
        if (!role) return;
        // New members share the team's folder policy: a worktree when others have one.
        QString folder = QStringLiteral("shared");
        for (const auto &m : team.members) if (!m.lead && m.folder == "worktree" && role->folder != "read_only" && role->folder != "shared") folder = m.folder;
        QJsonObject member{{"id", nextMemberId(team, role->name)}, {"role", role->name}, {"agent", agent->currentData().toString()},
                           {"model", model->text().trimmed()}, {"effort", effort->currentData().toString()}, {"account", account->currentData().toString()},
                           {"computer", computer->currentData().toString()}, {"permissions", role->permissions}, {"folder", folder},
                           {"prompt", prompt->toPlainText().trimmed()}};
        if (const QString parent = reportsTo->currentData().toString(); !parent.isEmpty()) member.insert("parent", parent);
        m_panel->act(QStringLiteral("add-member"), {{"member", member}});
        if (m_popover) { retire(m_popover); m_popover = nullptr; }
    });
    pop->show();
    placePopover();
}
