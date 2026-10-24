#pragma once
#include <QPointer>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QDialog;
class QScrollArea;
class QVBoxLayout;
class TeamController;
class TeamPanel;
class TeamTaskCard;

// WhiteWh fork: agent teams. The team workspace (design S10): the Team tab next
// to Activity, where the team takes the place of the chat. A team bar, the
// Stuck and plan-error strips, a canvas with the graph left to right, a board
// by status or the layer list, a member focus that dims everything else, the
// feed below, Needs your answer, members and a small Lead dock beside it, and
// the task card as a drawer. Actions go through the Team tab's own handlers.
class TeamWorkspace : public QWidget {
public:
    TeamWorkspace(TeamController *teams, TeamPanel *panel, QWidget *parent = nullptr);
    void setTheme(bool dark) { m_dark = dark; scheduleRender(); }
    void showTeam(const QString &team) { if (m_team != team) { m_team = team; m_focus.clear(); m_drawerTask = 0; } scheduleRender(); }
    void setLive(const Teams::LiveLookup &live) { m_live = live; scheduleRender(); }
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(const QString &host, const QString &session, const QString &text)> sendMessage;
    std::function<void()> openTeamTab;
    // Tests answer the Add member dialog before it runs.
    std::function<void(QDialog *dialog)> addMemberHook;
    void addMember(const Teams::Team &team);
    void render();
protected:
    void showEvent(QShowEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    void scheduleRender();
    QWidget *teamBar(const Teams::Team &team, const QString &status);
    QWidget *toolbar(const Teams::Team &team);
    QWidget *canvas(const Teams::Team &team);
    QWidget *board(const Teams::Team &team);
    QWidget *side(const Teams::Team &team);
    QWidget *leadDock(const Teams::Team &team);
    QWidget *feed(const Teams::Team &team);
    void placeDrawer();
    TeamController *m_teams;
    TeamPanel *m_panel;
    Teams::LiveLookup m_live;
    QString m_team, m_view = QStringLiteral("graph"), m_filter = QStringLiteral("all"), m_focus;
    bool m_dark = true, m_pending = false, m_feedOpen = false, m_leadMinimized = false;
    double m_zoom = 1.0;
    int m_drawerTask = 0;
    QPointer<QWidget> m_content;
    TeamTaskCard *m_drawer;
    QVBoxLayout *m_layout;
};
