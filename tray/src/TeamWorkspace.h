#pragma once
#include <QHash>
#include <QPoint>
#include <QPointer>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QFrame;
class QLabel;
class QScrollArea;
class QSplitter;
class QVBoxLayout;
class TeamController;
class TeamConversation;
class TeamPanel;
class TeamTaskCard;
namespace TeamUi { class TaskView; }

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
    // Off while the shown session is not the team's lead (or the team is archived).
    void setHeaderShown(bool shown);
    // "Phase N waits for you": opens the Team tab with that phase unfolded.
    std::function<void()> openWorkspace;
    void showPhase(int phase);
    void addMember(const Teams::Team &team);
    void render();
protected:
    void showEvent(QShowEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    void scheduleRender();
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
    void movePhase(const Teams::Team &team, int phase, int to, const QString &position);
    QWidget *inboxStrip(const Teams::Team &team, const QList<Teams::InboxItem> &items);
    // F8: the lead's open membership proposals with Accept and Reject; remove
    // and swap ask for confirmation. A decision goes out once.
    QWidget *proposalsStrip(const Teams::Team &team);
    QSet<QString> m_decidedProposals;
    // F7: a Done team starts the next goal with the same folder, settings,
    // lead and members (their live sessions adopted).
    void nextGoalDialog(const Teams::Team &team);
    // H6: swap a member's session, agent, model or account (the lead too),
    // confirmed; the old session keeps running and is listed for you to stop.
    void swapDialog(const Teams::Team &team, const Teams::Member &member);
    QWidget *swappedList(const Teams::Team &team);
    // F7/F8: opens another team in this workspace (previous, next, a new one).
    void openOtherTeam(const QString &team);
    QWidget *attentionStrip(const Teams::Team &team);
    // N7: Answer on a permission item: the member's Allow / Deny box, or its
    // command on the clipboard when this hgs sends no box.
    void answerAttention(const QString &teamId, const QString &member, const QString &command);
    // A planning, running or paused team takes a settings change (ops.rs settings).
    static bool settingsOpen(const Teams::Team &team)
    {
        return team.status == QLatin1String("planning") || team.status == QLatin1String("running") || team.status == QLatin1String("paused");
    }
    QWidget *errorStrip();
    // The team as the controller currently has it, by id (a poll may have
    // replaced the copy a widget was built with).
    const Teams::Team *currentTeam(const QString &id) const;
    QWidget *toolbar(const Teams::Team &team);
    QWidget *canvas(const Teams::Team &team);
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
    // selected edge, and reports their changes, per team.
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
    void rejectPhase(const QString &team, int phase);
    // U25 (spec 5.7): the right column's collapsed lists above the members:
    // Needs your answer, Team stuck (with Show on graph) and Plan problems.
    QWidget *sideLists(const Teams::Team &team, const QString &status);
    QWidget *members(const Teams::Team &team);
    QWidget *feed(const Teams::Team &team);
    void renderCoordinator(const Teams::Team &team, const QString &status);
    void memberMenu(const Teams::Team &team, const Teams::Member &member, const QPoint &global);
    void assignMenu(const Teams::Team &team, const QString &member, const QPoint &global);
    void placeDrawer();
    void placePopover();
    // A view rebuilt by a render keeps where the owner scrolled it.
    void keepScroll(QScrollArea *area, const QString &key);
    TeamController *m_teams;
    TeamPanel *m_panel;
    Teams::LiveLookup m_live;
    QList<Computer> m_computers;
    QString m_team, m_view = QStringLiteral("graph"), m_filter = QStringLiteral("all"), m_focus;
    QString m_error;
    bool m_dark = true, m_pending = false, m_feedOpen = false, m_fit = true;
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
    QLabel *m_empty;
    TeamTaskCard *m_drawer;
    QPointer<QFrame> m_popover;
};
