#pragma once
#include <QJsonArray>
#include <QJsonObject>
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
    void refresh();
    const QList<Teams::Team> &teams() const { return m_teams; }
    const Teams::Team *team(const QString &id) const;
    // The team whose members include this session, if any.
    const Teams::Team *teamOfSession(const QString &host, const QString &session) const;
    // Full state and the feed of one team; empty stops watching.
    void watch(const QString &team);
    QString watchedId() const { return m_watch; }
    const Teams::Team *watched() const { return m_watched.id.isEmpty() || m_watched.id != m_watch ? nullptr : &m_watched; }
    // Feed entries, newest first, and whether older pages exist.
    QList<QJsonObject> feed() const;
    bool feedHasOlder() const { return !m_feedCursor.isNull(); }
    void loadOlder();
    // A change: `operation` with `input`; team, by and request_id are added.
    void act(const QString &operation, QJsonObject input, const std::function<void(bool ok, const QString &message)> &done = {});
    bool available() const { return m_available; }
    static void setPollInterval(int milliseconds);
signals:
    void teamsChanged();
    void watchedChanged();
    void feedChanged();
private:
    void received(quint64 request, const QString &operation, const QJsonObject &result);
    void failed(quint64 request, const QString &operation, const QString &error);
    void readWatched();
    HgsClient *m_client;
    QTimer m_poll;
    bool m_active = false, m_available = false;
    quint64 m_listRequest = 0, m_statusRequest = 0, m_feedRequest = 0, m_olderRequest = 0;
    QList<Teams::Team> m_teams;
    // The last answers, so an unchanged poll changes nothing on screen.
    QJsonObject m_lastList, m_lastStatus;
    QString m_watch;
    Teams::Team m_watched;
    QMap<qint64, QJsonObject> m_feed;   // by seq
    QJsonValue m_feedCursor;
    QMap<quint64, std::function<void(bool, const QString &)>> m_actions;
};
