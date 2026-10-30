#pragma once
#include <QFrame>
#include <QHash>
#include <QJsonObject>
#include <QPointer>
#include <QTimer>
#include <functional>

#include "TeamWaitingNotices.h"
#include "TeamWaitingPage.h"

class QVBoxLayout;
class TeamController;
class TeamWaitingAnswer;

// WhiteWh fork, U55: what waits for the owner (U50's `hgs team waiting`)
// shown where the owner looks. One feed per window reads the list while the
// window shows (setActive), at most MaxItems a read, and raises one desktop
// notification per item the first time it waits (TeamWaitingNotices). Its
// banners sit at the top of the session's chat and Activity and above the
// lead's conversation in the team view.
class TeamWaitingFeed : public QObject {
    Q_OBJECT
public:
    explicit TeamWaitingFeed(TeamController *teams, QObject *parent = nullptr);
    // Reads now and every interval while active; nothing while not.
    void setActive(bool active);
    bool active() const { return m_poll.isActive(); }
    void setInterval(int milliseconds) { m_poll.setInterval(milliseconds); }
    void refresh();
    const QList<TeamWaitingPage::Item> &items() const { return m_items; }
    int total() const { return m_total; }
    static constexpr int MaxItems = 200;   // the server's page bound
    TeamController *teams() const { return m_teams; }
    // #724: the owner answered the item in place: it leaves the banners at
    // once (dropped), and its notifications, the waiting one and the team
    // attention one that covers it, are withdrawn.
    void answered(const TeamWaitingPage::Item &item);
signals:
    void changed();
    // A desktop notification: the tray shows it; a click opens the team.
    void notify(const QString &token, const QString &title, const QString &body);
    void dropped(const QString &id);
    void withdrawn(const QStringList &tokens);
private:
    TeamController *m_teams;
    QTimer m_poll;
    QList<TeamWaitingPage::Item> m_items;
    int m_total = 0;
    QJsonObject m_last;   // #706: an unchanged read changes nothing
    quint64 m_read = 0;
    bool m_inFlight = false;
};

// The banner: hidden while nothing waits, its margins included (an outer
// widget around the styled frame "teamWaitingBanner").
class TeamWaitingBanner : public QWidget {
public:
    explicit TeamWaitingBanner(TeamWaitingFeed *feed, QWidget *parent = nullptr);
    QFrame *frame() const { return m_frame; }
    void setDark(bool dark);
    // Open team and Accept...: the team at the item. #724: Approve… and
    // Answer decide in the banner's row (TeamWaitingAnswer).
    std::function<void(const TeamWaitingPage::Item &item)> openItem;
    // The answer of an item, open in its row; null when none is.
    TeamWaitingAnswer *answerOf(const QString &id) const;
    // More items than the banner lists: the U50 page.
    std::function<void()> openAll;
    bool expanded() const { return m_expanded; }
    void setExpanded(bool expanded);
    // The rows an open banner lists before "Open the waiting list".
    static constexpr int MaxRows = 5;
    // The button's words for an item's action.
    static QString actionText(const QString &action);
private:
    void render();
    QWidget *rowFor(const TeamWaitingPage::Item &item, bool first);
    TeamWaitingAnswer *answer(const TeamWaitingPage::Item &item);
    void closeAnswer(const QString &id);
    QHash<QString, QPointer<TeamWaitingAnswer>> m_answers;
    QPointer<TeamWaitingFeed> m_feed;
    QFrame *m_frame;
    QVBoxLayout *m_layout;
    bool m_dark = true, m_expanded = false;
};
