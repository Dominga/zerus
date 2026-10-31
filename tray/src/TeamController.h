#pragma once
#include <QJsonArray>
#include <QJsonObject>
#include <QHash>
#include <QMap>
#include <QObject>
#include <QTimer>
#include <functional>
#include "TeamModel.h"

class HgsClient;

// WhiteWh fork: agent teams on this computer, read from `hgs team list`, and
// the watched team's full state and feed (`team status`, `team events`).
// Reads poll only while active. Changes go out once with their own request_id
// and are never repeated; a refusal is reported to the caller.
class TeamController : public QObject {
    Q_OBJECT
public:
    explicit TeamController(HgsClient *client, QObject *parent = nullptr);
    void setActive(bool active);
    // This controller's pace; setPollInterval sets the default of new ones.
    void setInterval(int milliseconds) { m_poll.setInterval(milliseconds); }
    void refresh();
    const QList<Teams::Team> &teams() const { return m_teams; }
    const Teams::Team *team(const QString &id) const;
    // The team whose members include this session, if any.
    const Teams::Team *teamOfSession(const QString &host, const QString &session) const;
    // Full state and the feed of one team; empty stops watching.
    void watch(const QString &team);
    QString watchedId() const { return m_watch; }
    const Teams::Team *watched() const { return m_watched.id.isEmpty() || m_watched.id != m_watch ? nullptr : &m_watched; }
    // #693: what the views draw of a team (Teams::visibleShape): of the
    // watched team's status when it is here, else of its `team list` item;
    // and of the lead's inbox. Polls that change neither need no rebuild.
    QByteArray shapeOf(const QString &team) const;
    QByteArray inboxShape() const { return m_inboxShape; }
    // Feed entries, newest first, and whether older pages exist.
    QList<QJsonObject> feed() const;
    bool feedHasOlder() const { return !m_feedCursor.isNull(); }
    void loadOlder();
    // C2: the lead's inbox for the watched team, read with `hgs team inbox`.
    const QList<Teams::InboxItem> &inbox() const { return m_inbox; }
    void refreshInbox();
    // A change: `operation` with `input`; team, by and request_id are added.
    using Done = std::function<void(bool ok, const QString &message)>;
    void act(const QString &operation, QJsonObject input, const Done &done = {});
    // Owner: approve or discard the one draft phase. Fresh request ids.
    void approvePhase(int phase, const Done &done = {});
    void discardPhase(int phase, const Done &done = {});
    // Lead or owner: finish a team whose tasks are all decided.
    void finishTeam(const QString &summary, const Done &done = {});
    // A5: park a task (a pending one, or a report that goes back later) and let
    // it go again; a rejected report may be returned with a hold.
    void holdTask(int task, const QString &reason, const Done &done = {});
    void releaseTask(int task, const Done &done = {});
    void rejectWithHold(int task, const QString &reason, const Done &done = {});
    // A3: answer a member's question; the reply reaches it as a message.
    void answerQuestion(const QString &question, const QString &text, const Done &done = {});
    // Any `hgs team` operation ("catalog get", "create", ...) with its whole
    // answer. Changes get by and request_id like act().
    using Reply = std::function<void(bool ok, const QJsonObject &result, const QString &error)>;
    void call(const QString &operation, QJsonObject input, const Reply &done, bool change = true, int timeoutMs = 0);
    // The catalog, read on request and again after each change through call().
    const Teams::Catalog &catalog() const { return m_catalog; }
    // U36: the catalog's pipeline presets as hgs sends them ({id, name,
    // stages, max_rounds, folder, accept}); the plan editor's picker lists them.
    bool catalogReady() const { return m_catalogReady; }
    void refreshCatalog();
    // The catalog is read with every poll while its draft is on screen.
    void setWatchCatalog(bool watch) { m_watchCatalog = watch; if (watch) refreshCatalog(); }
    // The Team designer page polls the catalog too, apart from the Team tab.
    void setWatchCatalogPage(bool watch) { m_watchCatalogPage = watch; if (watch) refreshCatalog(); }
    // U15: the owner keeps, rejects or undoes a proposal and applies or
    // discards the draft whatever the designer session's state. A refusal
    // stays next to its proposal (or the draft) until that action succeeds,
    // and the draft is read again either way.
    void decideProposal(const QString &proposal, const QString &decision, const Reply &done = {});
    void applyDraft(const Reply &done = {});
    void discardDraft(const Reply &done = {});
    QString proposalError(const QString &proposal) const { return m_proposalErrors.value(proposal); }
    QString draftError() const { return m_draftError; }
    // U58: the reads ask for cancelled work (hgs hides it by default since
    // U58); Graph, Board, Layers and the plan hide it unless this is on.
    bool showCancelled() const { return m_showCancelled; }
    void setShowCancelled(bool on);
    bool available() const { return m_available; }
    HgsClient *client() const { return m_client; }
    static void setPollInterval(int milliseconds);
signals:
    void teamsChanged();
    void watchedChanged();
    void feedChanged();
    void catalogChanged();
    void inboxChanged();
    void showCancelledChanged();
private:
    void received(quint64 request, const QString &operation, const QJsonObject &result);
    void failed(quint64 request, const QString &operation, const QString &error);
    void readWatched();
    void draftCall(const QString &operation, const Reply &done);
    HgsClient *m_client;
    QTimer m_poll;
    bool m_active = false, m_available = false, m_showCancelled = false;
    quint64 m_listRequest = 0, m_statusRequest = 0, m_feedRequest = 0, m_olderRequest = 0;
    // A status read was asked for while one was in flight.
    bool m_statusAgain = false;
    QList<Teams::Team> m_teams;
    // The last answers, so an unchanged poll changes nothing on screen.
    QJsonObject m_lastList, m_lastStatus;
    QByteArray m_watchedShape, m_inboxShape;
    QHash<QString, QByteArray> m_listShapes;   // by team id
    QStringList m_listOrder;   // the ids as listed
    QString m_watch;
    Teams::Team m_watched;
    QMap<qint64, QJsonObject> m_feed;   // by seq
    QJsonValue m_feedCursor;
    QList<Teams::InboxItem> m_inbox;
    QJsonObject m_lastInbox;
    quint64 m_inboxRequest = 0;
    QMap<quint64, std::function<void(bool, const QString &)>> m_actions;
    // A failed change keeps its request_id, so a retry is answered idempotently.
    QHash<QString, QString> m_retryIds;
    QMap<quint64, Reply> m_calls;
    Teams::Catalog m_catalog;
    bool m_catalogReady = false, m_watchCatalog = false, m_watchCatalogPage = false, m_catalogInFlight = false;
    // Only the newest catalog read is shown: an older one may land after it.
    quint64 m_catalogRead = 0;
    QHash<QString, QString> m_proposalErrors;
    QString m_draftError;
};
