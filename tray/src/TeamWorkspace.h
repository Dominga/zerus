#pragma once
#include <QElapsedTimer>
#include <QHash>
#include <QPoint>
#include <QPointer>
#include <QWidget>
#include <deque>
#include <functional>
#include <memory>
#include "TeamAccountMove.h"
#include "TeamModel.h"

class QDialog;
class QFrame;
class QLabel;
class QMenu;
class QScrollArea;
class QSplitter;
class QTimer;
class QVBoxLayout;
class TeamController;
class TeamConversation;
namespace TeamProfile { class Cache; }
class TeamPanel;
class TeamTaskCard;
namespace TeamUi { class TaskView; struct Heat; }

// WhiteWh fork: agent teams. The Team tab next to Activity (design D3, HANDOFF
// §14): the Stuck and plan-error strips (the team's status and actions are in
// the lead session's header, U25), Needs your answer (its
// slot always there), the canvas — the graph fitted and centered, the board or
// the layer list — with the feed below, and a side column split between the
// members and the coordinator's conversation; the divider is the owner's. The
// team's roster changes here: + Member, and each member's menu. The task card
// opens as a drawer. Actions go through the Team tab's own handlers.
class TeamWorkspace : public QWidget {
public:
    struct Computer { QString alias, label; bool online = true; };
    TeamWorkspace(TeamController *teams, TeamPanel *panel, QWidget *parent = nullptr);
    ~TeamWorkspace() override;
    void setTheme(bool dark);
    void showTeam(const QString &team);
    void setLive(const Teams::LiveLookup &live) { m_live = live; scheduleRender(); }
    void setComputers(const QList<Computer> &computers) { m_computers = computers; }
    std::function<void(const QString &host, const QString &session)> openSession;
    // U35: the lead's terminal, when the coordinator's send meets a dialog or draft.
    std::function<void(const QString &host, const QString &session)> openTerminal;
    std::function<void(const QString &host, const QString &session, const QString &text)> sendMessage;
    std::function<void()> openTeamTab;
    // U25 (spec 3): the lead session's header carries the team. headerStatus()
    // goes after the session title: the team status, "Phase N waits for you"
    // and "Waiting for you to finish", the phase, tasks and phases bars, the
    // problems and the time since the start. headerActions() goes at the right
    // of the chips row: the team's actions by state, with their confirmations.
    // Both show above the canvas until a window takes them into its header.
    QWidget *headerStatus() const { return m_headerStatus; }
    QWidget *headerActions() const { return m_headerActions; }
    void takeHeader();
    // U55: what waits for the owner, above the lead's conversation (the
    // window's banner, TeamWaitingBanner); the conversation takes it.
    void setWaitingBanner(QWidget *banner);
    // Off while the shown session is not the team's lead (or the team is archived).
    void setHeaderShown(bool shown);
    // "Phase N waits for you": opens the Team tab with that phase unfolded.
    std::function<void()> openWorkspace;
    void showPhase(int phase);
    // U62 (#719): the Tests view with a record's row open (a drawer's link).
    void showTestRecord(const QString &record);
    void addMember(const Teams::Team &team);
    void render();
    // U38: how old a shown heat tint is before it is read again (a minute);
    // a test makes it due at once.
    static void setHeatAge(int milliseconds);
protected:
    void showEvent(QShowEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    // A change of the owner's (a click, a view, the theme) always rebuilds.
    void scheduleRender();
    // #693: a poll (a team list, status, inbox or feed read, a session list's
    // showTeam) rebuilds only when what is drawn changed: the team's visible
    // shape, the inbox, the feed's head or a member's live state. Running
    // times and ages tick in place (tickInPlace) on the minute timer.
    void pollRender();
    void queueRender();
    QByteArray pollKey() const;
    void tickInPlace();
    bool m_forced = false;
    QByteArray m_renderedKey;
    // #706: a member's live state alone (working, idle) rebuilds only the
    // side column, the header and the coordinator's head; the canvas's
    // activity labels and working dots change in place. A live change that
    // moves the team's status or its problems is in pollKey and rebuilds.
    QByteArray liveKey(bool working = true) const;
    void renderLive();
    void renderSide(const Teams::Team &team, const QString &status);
    bool refreshMembersLive(const Teams::Team &team);
    void refreshCanvasLive(const Teams::Team &team);
    QByteArray m_renderedLive, m_renderedLiveRest;
    void renderHeader();
    QWidget *headerStatusContent(const Teams::Team &team, const QString &status);
    QWidget *headerActionsContent(const Teams::Team &team);
    QWidget *finishSummary(const Teams::Team &team);
    // B5: the widening notice, and the approve that asks for the owner's
    // confirmation and sends confirm_widening.
    QWidget *wideningStrip(const Teams::Team &team);
    void approveTeam(const Teams::Team &team);
    // C2/C4: the lead's inbox and the owner's attention list.
    // G9: the phase band header's decisions. Approve and Discard ask for the
    // owner's confirmation and return true only when the request was sent; the
    // F10 arrows move a draft among the drafts.
    bool approvePhase(const QString &team, int phase);
    bool discardPhase(const QString &team, int phase);
    // U42: an accepted phase whose work has not started goes back to a draft
    // that waits for your approval again; asks first.
    bool returnPhaseToDraft(const QString &team, int phase);
    void movePhase(const Teams::Team &team, int phase, int to, const QString &position);
    // U63 (owner, 2026-10-31): an accepted phase whose work has not started
    // (no attempt but a refused one) is edited, moved and paused in place and
    // stays accepted. The phases Move earlier/later pass are the movable ones
    // (drafts and such accepted phases) in display order. A phase may require
    // the owner's acceptance once its tasks are decided; he accepts it or
    // sends it back to the lead with a comment.
    static bool acceptedUnstarted(const Teams::Team &team, int phase);
    static QList<int> movablePhases(const Teams::Team &team);
    void addPhaseMoves(QMenu *menu, const QString &team, int phase);
    void addPhaseAcceptance(QMenu *menu, const QString &team, int phase);
    void editPhaseDialog(const Teams::Team &team, int phase);
    void pausePhaseDialog(const Teams::Team &team, int phase);
    void unpausePhase(const QString &team, int phase);
    bool acceptOwnerPhase(const QString &team, int phase);
    void sendBackPhaseDialog(const Teams::Team &team, int phase);
    QWidget *inboxStrip(const Teams::Team &team, const QList<Teams::InboxItem> &items);
    // F8, U57: the proposals answered (the coordinator's cards, the plan-mode
    // dialog) until the store takes them; a decision goes out once.
    QSet<QString> m_decidedProposals;
    // F7: a Done team starts the next goal with the same folder, settings,
    // lead and members (their live sessions adopted).
    void nextGoalDialog(const Teams::Team &team);
    // U29: the owner adds a task to a running phase: the warning first, then
    // the typed confirmation ("add" or the phase number).
    void addLateDialog(const Teams::Team &team, int phase);
    // H6: swap a member's session, agent, model or account (the lead too),
    // confirmed; the old session keeps running and is listed for you to stop.
    void swapDialog(const Teams::Team &team, const Teams::Member &member);
    // U59 (TeamWorkspacePlanMode.cpp): the plan mode. While a plan waits for
    // approval, the lead's proposed mode with its reason and the owner's
    // choice (none keeps standard). While the team runs, the switch to free
    // is its own dialog bound to the request's id and hash; the mode shows
    // beside each phase; the open free phase reads idle, never done, folds its
    // accepted tasks, adds a build task on request and closes only on the
    // owner's confirmation.
    QWidget *planModeCard(const Teams::Team &team);
    QWidget *phaseModeBadge(const Teams::Team &team, int phase);
    static bool freeOpenPhase(const Teams::Team &team, int phase);
    QWidget *freeAcceptedFold(const Teams::Team &team, int phase, const QList<int> &accepted);
    QWidget *freeIdleCard(const Teams::Team &team, int phase);
    QString freeBuildLine(const Teams::Team &team) const;
    void popSwitchRequest(const Teams::Team &team);
    void switchModeDialog(const Teams::Team &team, const QString &proposal);
    void closePhaseDialog(const Teams::Team &team);
    void integrateBuild(const Teams::Team &team);
    // Why Close phase is off now (tasks still run, a close waits), or "".
    static QString closeBlocked(const Teams::Team &team);
    // The open build task of the free phase, 0 when none.
    static int openBuildTask(const Teams::Team &team);
    QSet<QString> m_switchShown;    // team/proposal: popped up once
    QSet<QString> m_freeUnfolded;   // team/phase: its accepted tasks show
    QSet<QString> m_buildSent;      // teams whose build request is on its way
    // U54 (TeamWorkspaceAccounts.cpp): move every member of one agent on an
    // account to another, asked once, with each member's progress; the
    // account each card shows, read from its session; the review action for a
    // member folder left on another branch.
    void accountMoveDialog(const Teams::Team &team, const Teams::Member &member);
    void startAccountMove(const Teams::Team &team, const QString &agent, const QString &from, const QString &to);
    void renderAccountMove();
    void requestAccounts();
    QString accountLabel(const Teams::Member &member) const;
    Teams::AccountMove::Step moveStep(const Teams::Team &team, const QString &member) const;
    QWidget *memberBlock(const Teams::Team &team, const Teams::Member &member);
    void checkoutMemberBranch(const Teams::Team &team, const Teams::Member &member, const Teams::AccountMove::Block &block);
    QWidget *swappedList(const Teams::Team &team);
    // U38 (TeamWorkspaceHeat.cpp): Graph and Board's heat tint. The Heat
    // switch (Off, Time lost, Tokens) is kept per team; its legend sits under
    // the toolbar; the profile's losses are read in pages and again a minute
    // after the last read while the tint is on.
    QString heatShown(const Teams::Team &team) const;   // "", "time" or "tokens"
    const TeamUi::Heat *heat(const Teams::Team &team);  // null while off, reading or refused
    QWidget *heatSwitch(const Teams::Team &team);
    QWidget *heatLegendRow(const Teams::Team &team);
    void readHeat(const QString &team, const QString &metric, const QJsonValue &cursor, const std::shared_ptr<TeamUi::Heat> &into);
    struct HeatRead {
        QString team;
        std::shared_ptr<TeamUi::Heat> heat;
        QElapsedTimer at;
        bool reading = false;
        QString error;
        quint64 generation = 0;
        QByteArray digest;   // #709 L6: heatDigest (or the error) of the last read that rebuilt
    };
    QHash<QString, HeatRead> m_heat;   // by metric
    // F7/F8: opens another team in this workspace (previous, next, a new one).
    void openOtherTeam(const QString &team);
    void teamSettings(const Teams::Team &team);
    void scaleRole(const Teams::Team &team, const Teams::Member &member);
    void attentionRows(const Teams::Team &team, QVBoxLayout *into);
    // N7: Answer on a permission item: the member's Allow / Deny box, or its
    // command on the clipboard when this hgs sends no box.
    // attempts: a member's box not shown yet is looked for again (fork CI).
    void answerAttention(const QString &teamId, const QString &member, const QString &command, int attempts = 10,
                         const QString &question = {}, const QString &hash = {}, quint64 generation = 0);
    // A planning, running or paused team takes a settings change (ops.rs settings).
    static bool settingsOpen(const Teams::Team &team)
    {
        return team.status == QLatin1String("planning") || team.status == QLatin1String("running") || team.status == QLatin1String("paused");
    }
    QWidget *errorStrip();
    // The team as the controller currently has it, by id (a poll may have
    // replaced the copy a widget was built with).
    const Teams::Team *currentTeam(const QString &id) const;
    // C4 (#548): the full copy (`team status`) a plan edit is built from; null,
    // after asking for it and showing "Loading the plan", while only the
    // `team list` summary is here.
    const Teams::Team *planTeam(const QString &id);
    QWidget *toolbar(const Teams::Team &team);
    QWidget *canvas(const Teams::Team &team);
    // U38 (#665): Profile beside Graph, Board and Layers.
    QWidget *profileView(const Teams::Team &team);
    QWidget *testsView(const Teams::Team &team);
    void linkDraft(const Teams::Team &team, int dependency, int task, bool add);
    QWidget *board(const Teams::Team &team);
    // U25 (spec 5.3, 5.5): every phase a collapsible scroll in Graph, Board and
    // Layers: header (number, status, accepted/tasks, proposed by or health,
    // the collapsed summary, the owner's actions), the lead's note, the
    // send-back form and the phase's body in the chosen view.
    QWidget *phaseColumn(const Teams::Team &team);
    QWidget *phaseScroll(const Teams::Team &team, const Teams::PhaseGroup &group);
    QWidget *phaseGraph(const Teams::Team &team, const Teams::PhaseGroup &group, const QList<int> &ids = {});
    // U27: a phase's body split into its lanes, in lane order, each a labelled
    // row that collapses (QSettings); the tasks without a lane render as the
    // dependency layers they always were.
    QWidget *laneSections(const Teams::Team &team, const Teams::PhaseGroup &group);
    QWidget *laneBody(const Teams::Team &team, const QList<int> &ids, const Teams::PhaseGroup &group);
    // U27: the lane a task moves to, and a lane's new name, in the plan or in a
    // draft phase (a plan change in Planning, edit-draft otherwise).
    void setTaskLane(const Teams::Team &team, int task, int lane);
    void renameLane(const Teams::Team &team, int phase, int lane, const QString &label);
    void sendLaneChanges(const Teams::Team &team, int phase, const QList<QPair<int, QJsonValue>> &updates);
    // U27: the menu that moves a task to a lane, or takes it out.
    void laneMenu(const Teams::Team &team, int task, const QPoint &global);
    // U25 part 5 (spec 5.4): the nodes of `order` in two lines - the
    // dependencies from other phases and the problem - with the working dots
    // and the full tooltip. `phase` holds the tasks drawn in this view (empty:
    // a team without phases).
    void graphNodes(TeamUi::TaskView *view, const Teams::Team &team, const QList<int> &order, const QSet<int> &phase);
    // #465 N18: a graph view takes back the owner's half-made link and
    // selected edge, and reports their changes, per team; #570: the same for
    // the task under the resting pointer, so its linked tasks stay lit.
    void keepGraphEdit(TeamUi::TaskView *view, const QString &team);
    // U33: the narrowest readable Board column (two-line cards); wider windows
    // share their room. Five fit the Board of a 1600 px workspace.
    static constexpr int BoardColumnWidth = 200;
    static void readableColumn(QWidget *column);
    QWidget *boardColumns(const Teams::Team &team, const QList<int> &ids, int phase);
    QWidget *layerRows(const Teams::Team &team, const QList<int> &ids);
    // The tasks with a problem and the tone each shows.
    QMap<int, Teams::Tone> problemTasks(const Teams::Team &team) const;
    // The text that names a task's problem on its card ("Needs your answer").
    QString problemText(const Teams::Team &team, int task) const;
    QMap<int, QString> problemTexts(const Teams::Team &team) const;
    QSet<int> blockers(const Teams::Team &team) const;
    void setAutoAccept(const Teams::Team &team, bool on);
    void setWorkAhead(const Teams::Team &team, bool on);
    void rejectPhase(const QString &team, int phase);
    // U25 (spec 5.7): the right column's collapsed lists above the members:
    // Needs your answer, Team stuck (with Show on graph) and Plan problems.
    QWidget *sideLists(const Teams::Team &team, const QString &status);
    QWidget *members(const Teams::Team &team);
    QWidget *feed(const Teams::Team &team);
    void renderCoordinator(const Teams::Team &team);
    void memberMenu(const Teams::Team &team, const Teams::Member &member, const QPoint &global);
    void assignMenu(const Teams::Team &team, const QString &member, const QPoint &global);
    void placeDrawer();
    void placePopover();
    // #708: a large Board or Layers view is built over bounded event-loop
    // turns, so one render never stalls the window; a small view is drained
    // inside the render and behaves as before.
    void queueSlice(std::function<void()> work);
    void runSlices();
    void cancelSlices();
    QTimer *m_sliceTimer = nullptr;
    std::deque<std::function<void()>> m_slices;
    // A view rebuilt by a render keeps where the owner scrolled it.
    void keepScroll(QScrollArea *area, const QString &key);
    TeamController *m_teams;
    TeamPanel *m_panel;
    Teams::LiveLookup m_live;
    QList<Computer> m_computers;
    QString m_team, m_view = QStringLiteral("graph"), m_filter = QStringLiteral("all"), m_focus;
    // Owner, 2026-10-30: Graph, Board and Layers leave out finished phases.
    bool m_hideFinished = false;
    QString m_error;
    // Answer focused this exact question's Allow; rebuilds in the next few
    // seconds focus it again, even when the focus was already lost.
    QString m_answerTeam, m_answerMember, m_answerQuestion, m_answerHash;
    QElapsedTimer m_answerAt;
    quint64 m_answerGeneration = 0;
    bool m_answerRendering = false;
    bool m_dark = true, m_pending = false, m_feedOpen = false, m_inboxOpen = false, m_fit = true;
    // G9: one phase decision is in flight at a time; a second click on a header
    // before the answer arrives sends nothing. After an answer, that phase stays
    // decided until the team's revision moves (the store has taken the
    // decision); a failed send lets it go. Only a sent decision marks it.
    int m_phaseAct = 0;
    QSet<QString> m_phasesDecided;   // "team/phase" sent at m_decidedRevision
    qint64 m_decidedRevision = -1;
    bool phaseDecided(const Teams::Team &team, int phase) const;
    QString markPhaseDecided(const Teams::Team &team, int phase);
    void phaseSent(const QString &key, bool ok, const QString &message);
    // G5: the members block folds, and the owner's choice survives a restart.
    // (G9 moved the draft phases into the graph's band headers.)
    bool m_membersOpen = true;
    // Spec 5.6: the sub-leads whose reports are folded in Members.
    QSet<QString> m_subFolded;
    // U25: the draft whose send-back form is open, its text kept across
    // renders, and Team stuck's Show on graph.
    // N14 (#383): the open send-back form: its team, phase (number and when
    // it was added) and the phase numbers' revision (rejectKey); the comment.
    QString m_rejectPhase;
    QString m_rejectText;
    // #465 N18: per team, the task a click-to-connect started from and the
    // selected edge (dependency, task), across re-renders.
    QHash<QString, std::pair<int, QPair<int, int>>> m_graphEdit;
    QHash<QString, int> m_graphHover;   // #570: the hovered task per team
    static QString rejectKey(const Teams::Team &team, int phase);
    bool m_showBlockers = false, m_rejectFocus = false, m_zoomManual = false;
    double m_zoom = 1.0;
    int m_drawerTask = 0;
    QHash<QString, QPoint> m_scroll;
    // Rebuilt on every render.
    QPointer<QWidget> m_top, m_left, m_membersContent, m_headerStatusContent, m_headerActionsContent;
    // A window's header owns them once taken (takeHeader) and may go first.
    QPointer<QWidget> m_headerStatus, m_headerActions;
    QFrame *m_headerStrip;
    bool m_headerTaken = false, m_headerShown = true;
    QVBoxLayout *m_topHost, *m_leftHost;
    // Kept between renders.
    QWidget *m_body;
    QSplitter *m_split, *m_sideSplit;
    QScrollArea *m_members;
    QWidget *m_coordinator, *m_coordinatorHead;
    TeamConversation *m_conversation;
    class TeamProposalCards *m_proposalCards = nullptr;
    TeamProfile::Cache *m_profileCache = nullptr;
    QPointer<class TeamProfileView> m_profileView;
    QPointer<class TeamTestsView> m_testsView;
    QLabel *m_empty;
    TeamTaskCard *m_drawer;
    QPointer<QFrame> m_popover;
    // U54: the move this desktop started (one at a time), the lead's
    // computer's accounts and each member's checkout refusal.
    Teams::AccountMove::Move m_move;
    QPointer<QDialog> m_moveDialog;
    QList<QJsonObject> m_accountProfiles;
    quint64 m_accountsRequest = 0;
    bool m_accountsRead = false;
    QHash<QString, QString> m_checkoutErrors;
    QSet<QString> m_checkingOut;
};
