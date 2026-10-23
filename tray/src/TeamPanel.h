#pragma once
#include <QPointer>
#include <QSet>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QScrollArea;
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
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(const QString &host, const QString &session, const QString &text)> sendMessage;
    // `?N` for the tab: questions and approvals waiting in member sessions.
    std::function<void(int answers)> answersChanged;
    void render();
protected:
    void resizeEvent(QResizeEvent *event) override;
    void showEvent(QShowEvent *event) override;
private:
    void scheduleRender();
    QWidget *header(const Teams::Team &team, const QString &status, bool narrow);
    QWidget *stuck(const Teams::Team &team, const QList<Teams::Problem> &problems);
    QWidget *answers(const Teams::Team &team);
    QWidget *members(const Teams::Team &team, bool narrow);
    QWidget *memberRow(const Teams::Team &team, const Teams::Member &member, bool narrow);
    QWidget *problemActions(const Teams::Team &team, const Teams::Member &member, const Teams::Status &status);
    QWidget *tasks(const Teams::Team &team, bool narrow);
    QWidget *feed(const Teams::Team &team);
    QWidget *planning(const Teams::Team &team);
    void taskMenu(const Teams::Team &team, int task, const QPoint &global);
    void reassignMenu(const Teams::Team &team, int task, const QPoint &global);
    void act(const QString &operation, const QJsonObject &input);
    bool confirm(const QString &name, const QString &title, const QString &text, const QString &action);
    void remind(const Teams::Team &team, const Teams::Member &member, int task);
    TeamController *m_controller;
    Teams::LiveLookup m_live;
    QString m_team, m_filter = QStringLiteral("all"), m_feedMember, m_notice;
    bool m_dark = true, m_showBlockers = false, m_pending = false, m_dirty = false;
    double m_zoom = 1.0;
    QScrollArea *m_scroll;
    QPointer<QWidget> m_content;
};
