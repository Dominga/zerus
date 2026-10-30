#pragma once
#include <QElapsedTimer>
#include <QHash>
#include <QJsonObject>
#include <QJsonValue>
#include <QList>
#include <QPointer>
#include <QTimer>
#include <QWidget>
#include <functional>

class QLabel;
class QPushButton;
class QScrollArea;
class QVBoxLayout;
class TeamController;
class TeamWaitingAnswer;

// WhiteWh fork: U50 (spec 2026-10-28-team-customization.md section 7). The
// rail page beside the Team designer: every item of every team that waits for
// the owner (draft phases and plans, reports and permissions escalated to the
// owner, the owner's word asked for, tasks the owner holds, finished teams),
// newest first, read with `hgs team waiting` while the page shows. A row opens
// its team. Pages of 50, "Show older" for the next. #724: a one-click owner
// decision is answered in its row (TeamWaitingAnswer); an answered item leaves
// at once.
class TeamWaitingPage : public QWidget {
public:
    struct Item {
        QString id, team, teamName, kind, title, action, member, proposal;
        QString memberId;   // #740: the member answered ("member" names its role)
        double since = 0;
        int phase = 0, task = 0;
    };
    explicit TeamWaitingPage(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark);
    // Reads the newest page again (the shown older pages stay).
    void refresh();
    void loadOlder();
    // A row's team: the window opens its Team tab (a draft phase unfolded).
    std::function<void(const Item &item)> openTeam;
    // How many items wait, for the rail button's caption; -1 before the first read.
    std::function<void(int total)> countChanged;
    // #724: an item answered here; the window drops it from the banner and
    // withdraws its notification.
    std::function<void(const Item &item)> answered;
    // The item leaves the list at once (answered here or in the banner).
    void drop(const QString &id);
    // The answer of an item, open in its row; null when none is.
    TeamWaitingAnswer *answerOf(const QString &id) const;
    const QList<Item> &items() const { return m_items; }
    static Item parse(const QJsonObject &item);
    // The kind's short name and its icon (WorkspaceIcons).
    static QString kindLabel(const QString &kind);
    static QString kindIcon(const QString &kind);
    static constexpr int PageSize = 50;
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void render();
    QWidget *rowFor(const Item &item);
    // The item's answer, made when first wanted; it outlives the rebuilds.
    TeamWaitingAnswer *answer(const Item &item);
    void closeAnswer(const QString &id);
    QHash<QString, QPointer<TeamWaitingAnswer>> m_answers;
    TeamController *m_teams;
    QTimer m_poll;
    // The newest page as last read, and the older pages "Show older" added;
    // a poll re-reads only the newest page, so the older ones never jump.
    QList<Item> m_head, m_older, m_items;
    QJsonValue m_headNext, m_olderNext;
    int m_total = -1;
    QString m_error;
    quint64 m_headRead = 0, m_olderRead = 0;
    bool m_loadingOlder = false, m_dark = true;
    QJsonObject m_lastHead;
    QElapsedTimer m_rendered;
    QLabel *m_title, *m_count, *m_tile;
    QPushButton *m_refresh;
    QScrollArea *m_scroll;
    QWidget *m_list;
    QVBoxLayout *m_rows;
};
