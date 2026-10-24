#pragma once
#include "HgsClient.h"
#include <QHash>
#include <QJsonObject>
#include <QObject>

// WhiteWh fork: keeps an idle conversation's prompt cache warm. Shortly before
// the provider's cache would expire, the tray sends the session a one-word
// service message. hgs does not count its reply; a reply it does count while a
// keepalive is pending (agents without a journaled prompt) is handed back to be
// marked read, so it neither shows unread nor notifies. Warming stops after the
// chosen hours without a turn of the person's, and never revives a cold cache.
class CacheKeepalive : public QObject {
    Q_OBJECT
public:
    static constexpr int DefaultHours = 1, MaxHours = 24;
    static constexpr int DefaultTtl = 300;
    static QString message();
    // Hours per session, 0 when off. Device-local, keyed like collapsed rows.
    static int hours(const QString &host, const SessionInfo &session);
    static void setHours(const QString &host, const SessionInfo &session, int hours);
    // The provider cache's lifetime: Claude's from its native usage once seen.
    static int ttl(const QJsonObject &details);

    explicit CacheKeepalive(const QString &executable, QObject *parent = nullptr);
    struct Reply { QString host, name, conversation, reply; };
    // One host's fresh sessions; returns keepalive replies to mark read.
    QList<Reply> observe(const QString &host, const BoxState &box, double now);
    bool pending(const QString &host, const SessionInfo &session) const;
    // Keepalives sent and not yet answered by hgs.
    int inFlight() const { return int(m_requests.size()); }

private:
    struct State {
        QString host, replyId, run, conversation;
        double real = 0;      // the person's or the agent's last turn
        double warmed = 0;    // the cache's last use
        double sent = 0;      // a keepalive in flight or unanswered
        quint64 request = 0;
        int ttl = DefaultTtl;
        bool cold = false;    // until the next real turn
        bool inspected = false;
    };
    static bool idle(const SessionInfo &session);
    HgsClient m_client;
    QHash<QByteArray, State> m_states;
    QHash<quint64, QByteArray> m_requests;
    QHash<QString, QByteArray> m_inspections;  // host\nname
};
