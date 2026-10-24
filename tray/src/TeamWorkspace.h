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

// The Team tab next to Activity: the team bar, the Stuck and plan-error strips, Needs your answer (its
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
    bool m_dark = true, m_pending = false, m_feedOpen = false, m_fit = true;
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
