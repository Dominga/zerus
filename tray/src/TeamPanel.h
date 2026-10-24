#pragma once
#include <QPointer>
#include <QSet>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QScrollArea;
class QStackedWidget;
class TeamTaskCard;
class QVBoxLayout;
class TeamController;

// The inspector's Team tab: the
// team header with progress and Pause/Resume/Stop/Archive, Stuck, Needs your
// answer, Members with the actions each problem offers, Tasks (a graph from
// 400px, else a list by layer) and the paged feed. Nothing here is retried:
// every change is one confirmed `hgs team` request.
class TeamPanel : public QWidget {
public:
    explicit TeamPanel(TeamController *controller, QWidget *parent = nullptr);
    void setTheme(bool dark);
    // The member sessions as the fleet shows them.
    void setLive(const Teams::LiveLookup &live) { m_live = live; scheduleRender(); }
    // Shows a team, or the team of a session; empty shows the empty state.
    void showTeam(const QString &team);
    QString shownTeam() const { return m_team; }
    // Opens a task's card over the tab (a plaque's task link, a click on a task).
    void openTask(int task);
    // The Team designer's draft instead of a team (its session is selected).
    void showDesigner(bool on);
    int openedTask() const { return m_openTask; }
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(const QString &host, const QString &session, const QString &text)> sendMessage;
    // `?N` for the tab: questions and approvals waiting in member sessions.
    std::function<void(int answers)> answersChanged;
    void render();
    // Shared with the team workspace.
    QWidget *problemActions(const Teams::Team &team, const Teams::Member &member, const Teams::Status &status);
    void reassignMenu(const Teams::Team &team, int task, const QPoint &global);
    void act(const QString &operation, const QJsonObject &input);
    bool confirm(const QString &name, const QString &title, const QString &text, const QString &action);
    // Planning, shared with the team workspace (TeamPanelPlan.cpp): the whole
    // plan once per edit; a task editor (0 = a new task); a dependency.
    void savePlan(const Teams::Team &edited);
    void editTask(const Teams::Team &team, int task);
    void link(const Teams::Team &team, int dependency, int task, bool add);
    // Opens the team workspace (the Team tab next to Activity).
    std::function<void()> openWorkspace;
    // New team and Team designer, offered while no team is shown.
    std::function<void()> newTeam, openDesigner;
protected:
    void resizeEvent(QResizeEvent *event) override;
    void showEvent(QShowEvent *event) override;
private:
    void scheduleRender();
    QWidget *header(const Teams::Team &team, const QString &status, bool narrow);
    // The inspector's short view and its no-team state.
    QWidget *emptyState(bool narrow);
    QWidget *summary(const Teams::Team &team, const QString &status, bool narrow);
    QWidget *summaryMembers(const Teams::Team &team, bool narrow);
    QWidget *stuck(const Teams::Team &team, const QList<Teams::Problem> &problems);
    QWidget *answers(const Teams::Team &team);
    QWidget *members(const Teams::Team &team, bool narrow);
    QWidget *memberRow(const Teams::Team &team, const Teams::Member &member, bool narrow);
    QWidget *tasks(const Teams::Team &team, bool narrow);
    QWidget *feed(const Teams::Team &team);
    QWidget *planning(const Teams::Team &team);
    void taskMenu(const Teams::Team &team, int task, const QPoint &global);
    void remind(const Teams::Team &team, const Teams::Member &member, int task);
    // Planning (TeamPanelPlan.cpp).
    QWidget *planMembers(const Teams::Team &team, bool narrow);
    void addMember(const Teams::Team &team, const QPoint &global);
    QWidget *planFooter(const Teams::Team &team);
    TeamController *m_controller;
    Teams::LiveLookup m_live;
    QString m_team, m_filter = QStringLiteral("all"), m_feedMember, m_notice;
    bool m_dark = true, m_showBlockers = false, m_pending = false, m_dirty = false;
    double m_zoom = 1.0;
    QScrollArea *m_scroll;
    QStackedWidget *m_stack;
    TeamTaskCard *m_card;
    class TeamDesignerPanel *m_designer;
    bool m_designerShown = false;
    int m_openTask = 0;
    bool m_sendBack = false;
    class QVBoxLayout *m_outer;
    QPointer<QWidget> m_footer;
    QPointer<QWidget> m_content;
};
