#pragma once
#include <QJsonArray>
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
        // #742: a missing tool's offer (no team: teamName is the project).
        QString offer, tool, why, saves, command, output, toolState;
        // Task 97: a request the owner queue deferred (kind "deferred"): its
        // id, permission or question, exact text, session, what Zerus did,
        // and a question's options.
        QString deferred, request, text, session, outcome;
        QJsonArray questions;
        // Task #230: the lead's plan changes the digest names, each revertible.
        QJsonArray changes;
        // Task 304 (hgs #246, #286): the situation the row stands for, which
        // Dismiss names; a dismissed decision's blocking flag and what it
        // still blocks; the lead-changes digest's groups (kind, count, newest).
        QString state, blocks;
        bool blocking = false;
        QJsonArray groups;
        double since = 0;
        int phase = 0, task = 0;
        // U63 (owner, 2026-10-31): a phase that waits for the owner's
        // acceptance (accept_phase) carries how its tasks ended, its tests
        // and the minutes it took.
        struct Summary {
            bool known = false;
            int accepted = 0, rejected = 0, cancelled = 0, minutes = 0;
            QString tests;
            QString text() const;   // "3 accepted, 1 rejected, 0 cancelled; tests pass; 42 min"
        };
        Summary summary;
    };
    explicit TeamWaitingPage(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark);
    // Reads the newest page again (the shown older pages stay).
    void refresh();
    void loadOlder();
    // A row's team: the window opens its Team tab (a draft phase unfolded).
    std::function<void(const Item &item)> openTeam;
    // Task 108: a drive low on space opens Settings > Build folders.
    std::function<void()> openDisk;
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
    // Task 304: the digest's groups as one line ("2 added, 1 moved") and the
    // newest changes of each, for its tooltip; empty without groups.
    static QString groupsText(const Item &item);
    static QString groupsTip(const Item &item);
    // Task 304 (hgs #246): the owner's Dismiss of a row in its state, and
    // Restore; `done` gets whether hgs took it.
    static void dismiss(TeamController *teams, const Item &item, const std::function<void(bool ok, const QString &message)> &done);
    static void restore(TeamController *teams, const Item &item, const std::function<void(bool ok, const QString &message)> &done);
    const QList<Item> &dismissedItems() const { return m_dismissed; }
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
    // Task 304: the dismissed rows (hgs `dismissed`), shown under Show dismissed.
    QList<Item> m_dismissed;
    int m_dismissedTotal = 0;
    bool m_showDismissed = false;
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
