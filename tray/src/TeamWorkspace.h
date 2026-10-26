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
    // G5/F10: a draft waits for approval as one compact, foldable line at the
    // top of the members column; opened, it shows its tasks and the lead's diff,
    // and its arrows move it among the drafts (move-phase).
    QWidget *draftPhase(const Teams::Team &team, int phase);
    QWidget *draftDetail(const Teams::Team &team, const Teams::PhaseGroup &group);
    QWidget *finishSummary(const Teams::Team &team);
    // B5: the widening notice, and the approve that asks for the owner's
    // confirmation and sends confirm_widening.
    QWidget *wideningStrip(const Teams::Team &team);
    void approveTeam(const Teams::Team &team);
    // C2/C4: the lead's inbox and the owner's attention list.
    // G9: the phase band header's decisions. Approve and Discard ask for the
    // owner's confirmation and return true only when the request was sent; the
    // F10 arrows move a draft among the drafts.
    bool approvePhase(const Teams::Team &team, int phase);
    bool discardPhase(const Teams::Team &team, int phase);
    void movePhase(const Teams::Team &team, int phase, int to, const QString &position);
    QWidget *inboxStrip(const Teams::Team &team, const QList<Teams::InboxItem> &items);
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
    // before the answer arrives sends nothing.
    int m_phaseAct = 0;
    // G5: the two blocks of the members column fold independently, and the
    // owner's choice survives a restart; one draft at a time opens.
    bool m_phasesOpen = true, m_membersOpen = true;
    int m_openDraft = 0;
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
