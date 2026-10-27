#pragma once
#include <QHash>
#include <QMenu>
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

// WhiteWh fork: the inspector's Team tab (design/agent-teams, S2 and S8): the
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
    // F7: a member session a Done/Stopped team left running. The panel confirms
    // and then asks the window to stop it (or to start a new team with it); it
    // never stops one itself.
    std::function<void(const QString &host, const QString &session)> stopSession;
    std::function<void(const QString &host, const QString &session)> takeIntoTeam;
    // F8: the free sessions a member may adopt instead of starting its own.
    std::function<QList<Teams::SessionChoice>()> sessionChoices;
    // `?N` for the tab: questions and approvals waiting in member sessions.
    std::function<void(int answers)> answersChanged;
    void render();
    // Shared with the team workspace.
    QWidget *problemActions(const Teams::Team &team, const Teams::Member &member, const Teams::Status &status);
    // The "Left from <team>" block of a Done/Stopped team (F7).
    QWidget *leftoverMembers(const Teams::Team &team, bool narrow);
    // Phase groups in the plan, board and graph: a finished phase starts
    // collapsed, the owner's toggles are kept per team for the session.
    bool phaseOpen(const Teams::Team &team, const Teams::PhaseGroup &group) const;
    void togglePhase(const Teams::Team &team, int phase);
    // The owner finishes a decided team: a required summary, at most 4,000
    // characters. The dialog confirms; the request goes out once.
    void finishTeam(const Teams::Team &team);
    void reassignMenu(const Teams::Team &team, int task, const QPoint &global);
    void act(const QString &operation, const QJsonObject &input,
             const std::function<void(bool ok, const QString &message)> &done = {});
    bool confirm(const QString &name, const QString &title, const QString &text, const QString &action);
    // A confirmation runs its own event loop: while it is open the team views
    // do not rebuild, or the widget that asked would be deleted under it.
    bool confirming() const { return m_confirming > 0; }
    // The same for a menu or dialog with its own event loop opened from a row
    // (a member's "…" menu, a task menu, the task editor): while one is open
    // the team views do not rebuild, or the row and the button that opened it
    // would be deleted under its handler; they catch up once it has closed.
    class RenderHold {
    public:
        explicit RenderHold(TeamPanel *panel) : m_panel(panel) { if (panel) ++panel->m_confirming; }
        ~RenderHold() { if (m_panel) m_panel->releaseHold(); }
        RenderHold(const RenderHold &) = delete;
        RenderHold &operator=(const RenderHold &) = delete;
    private:
        QPointer<TeamPanel> m_panel;
    };
    friend class RenderHold;
    // A tool button's menu runs its own event loop as well: the views hold
    // from aboutToShow until its triggered action has run.
    void holdWhileOpen(QMenu *menu);
    std::function<void()> confirmClosed;
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
    // The inspector's short view (design D4) and its no-team state.
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
    int m_confirming = 0;
    void releaseHold();
    // Releases the hold once the event loop is back at `level` (the one the menu opened from).
    void releaseUnwound(int level);
    double m_zoom = 1.0;
    // team id -> phase -> open; at most a handful of teams, oldest dropped.
    QHash<QString, QHash<int, bool>> m_phaseOpen;
    QStringList m_phaseOrder;
    QPointer<QWidget> m_finishDialog;
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
