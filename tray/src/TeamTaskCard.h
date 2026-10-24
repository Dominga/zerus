#pragma once
#include <QJsonObject>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QVBoxLayout;
class QScrollArea;

// A task's card: its status,
// the problem with its actions, description, criteria, what it depends on and
// blocks, the assignee, attempts newest first, the report and inline Reject /
// Mark done forms; actions by status in the footer, the rest under More with
// the reason when they do not apply. The Team tab shows it over its content;
// the team workspace as a drawer.
class TeamTaskCard : public QWidget {
public:
    explicit TeamTaskCard(QWidget *parent = nullptr);
    void setTask(const Teams::Team &team, int task, const Teams::LiveLookup &live, bool dark);
    int task() const { return m_task; }
    std::function<void()> back;
    std::function<void(const QString &operation, const QJsonObject &input)> act;
    std::function<void(int task)> openTask;
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(int task, const QPoint &global)> reassign;
    // The actions a member's problem offers (the Team tab's own buttons).
    std::function<QWidget *(const Teams::Member &member, const Teams::Status &status)> problemActions;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
private:
    void rebuild();
    Teams::Team m_team;
    Teams::LiveLookup m_live;
    int m_task = 0;
    bool m_dark = true;
    QString m_form;   // "reject" or "done" while an inline form is open
    QScrollArea *m_scroll;
    QWidget *m_footer;
    QVBoxLayout *m_layout;
};
