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

// WhiteWh fork: agent teams. The Team tab next to Activity (design D3, HANDOFF
// §14): the team bar, the Stuck and plan-error strips, Needs your answer (its
// slot always there), the canvas — the graph fitted and centered, the board or
// the layer list — with the feed below, and a side column split between the
// members and the coordinator's conversation; the divider is the owner's. The
// team's roster changes here: + Member, and each member's menu. The task card
// opens as a drawer. Actions go through the Team tab's own handlers.
class TeamWorkspace : public QWidget {
public:
    struct Computer { QString alias, label; bool online = true; };
    TeamWorkspace(TeamController *teams, TeamPanel *panel, QWidget *parent = nullptr);
    void setTheme(bool dark);
    void showTeam(const QString &team);
    void setLive(const Teams::LiveLookup &live) { m_live = live; scheduleRender(); }
    void setComputers(const QList<Computer> &computers) { m_computers = computers; }
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(const QString &host, const QString &session, const QString &text)> sendMessage;
    std::function<void()> openTeamTab;
    void addMember(const Teams::Team &team);
    void render();
protected:
    void showEvent(QShowEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    void scheduleRender();
    QWidget *teamBar(const Teams::Team &team, const QString &status);
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
    QWidget *errorStrip();
    // The team as the controller currently has it, by id (a poll may have
    // replaced the copy a widget was built with).
    const Teams::Team *currentTeam(const QString &id) const;
    QWidget *answers(const Teams::Team &team);
    QWidget *toolbar(const Teams::Team &team);
    QWidget *canvas(const Teams::Team &team);
    QWidget *board(const Teams::Team &team);
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
    double m_zoom = 1.0;
    int m_drawerTask = 0;
    QHash<QString, QPoint> m_scroll;
    // Rebuilt on every render.
    QPointer<QWidget> m_top, m_left, m_membersContent;
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
