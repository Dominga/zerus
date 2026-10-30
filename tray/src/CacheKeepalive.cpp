#include "CacheKeepalive.h"
#include "SessionRowPreferences.h"
#include "SettingsJson.h"
#include <QJsonDocument>
#include <QSet>
#include <QSettings>
#include <algorithm>

namespace {
const char *const SettingsKey = "workspace/cacheKeepWarm";
// Bounded like the other device-local session preferences.
constexpr int MaxSessions = 10000;
QJsonObject saved() { return SettingsJson::object(QString::fromLatin1(SettingsKey)); }
QString id(const QString &host, const SessionInfo &session) { return QString::fromLatin1(SessionRowPreferences::key(host, session).toHex()); }
}

QString CacheKeepalive::message()
{
    // hgs recognizes the envelope (CACHE_KEEPALIVE in src/state/journal.rs).
    return QStringLiteral("<zerus-cache-keepalive>Zerus keeps this conversation's prompt cache warm while the person is away. "
                          "Do not use tools or continue any task. Reply with exactly: ok</zerus-cache-keepalive>");
}

int CacheKeepalive::hours(const QString &host, const SessionInfo &session)
{
    return hours(saved(), host, session);
}

QJsonObject CacheKeepalive::savedHours() { return saved(); }

int CacheKeepalive::hours(const QJsonObject &saved, const QString &host, const SessionInfo &session)
{
    return saved.isEmpty() ? 0 : std::clamp(saved.value(id(host, session)).toInt(), 0, MaxHours);
}

void CacheKeepalive::setHours(const QString &host, const SessionInfo &session, int hours)
{
    auto values = saved(); const auto key = id(host, session);
    if (hours <= 0) values.remove(key);
    else if (values.contains(key) || values.size() < MaxSessions) values.insert(key, std::min(hours, int(MaxHours)));
    SettingsJson::store(QString::fromLatin1(SettingsKey), QJsonDocument(values));
}

int CacheKeepalive::ttl(const QJsonObject &details)
{
    const int seconds = details.value("session_usage").toObject().value("prompt_cache").toObject().value("ttl_seconds").toInt();
    return seconds > 0 ? seconds : DefaultTtl;
}

CacheKeepalive::CacheKeepalive(const QString &executable, QObject *parent) : QObject(parent), m_client(executable)
{
    // An undelivered or uncertain keepalive is never resent: the cache counts
    // as cold until the next real turn.
    connect(&m_client, &HgsClient::messageFailed, this, [this](quint64 request, const QString &, const QString &, const QString &, bool) {
        const auto key = m_requests.take(request);
        if (auto it = m_states.find(key); it != m_states.end() && it->request == request) { it->sent = 0; it->request = 0; it->cold = true; }
    });
    connect(&m_client, &HgsClient::messageSent, this, [this](quint64 request, const QString &, const QString &, const QJsonObject &) {
        m_requests.remove(request);
    });
    connect(&m_client, &HgsClient::inspectionReady, this, [this](const QString &host, const QString &name, const QJsonObject &data, const QString &) {
        const auto key = m_inspections.take(host + '\n' + name);
        auto it = m_states.find(key); if (it == m_states.end() || data.value("conversation_id").toString() != it->conversation) return;
        it->ttl = ttl(data);
        const double expires = data.value("session_usage").toObject().value("prompt_cache").toObject().value("expires_at").toDouble();
        if (expires > 0) it->warmed = std::max(it->warmed, expires - it->ttl);
    });
    connect(&m_client, &HgsClient::inspectionFailed, this, [this](const QString &host, const QString &name, const QString &, const QString &) {
        m_inspections.remove(host + '\n' + name);
    });
}

bool CacheKeepalive::idle(const SessionInfo &s)
{
    return s.state == "running" && s.processState != "exited" && s.activity == "idle" && s.phase == "idle"
        && s.backgroundProcesses <= 0 && s.subagentActiveCount == 0 && !s.hasCoordinationWait() && !s.needsAction();
}

// A build Zerus runs: the agent itself is idle; the build is its work.
bool CacheKeepalive::building(const SessionInfo &s)
{
    return s.sessionState == "build" && s.state == "running" && s.processState != "exited" && s.activity == "idle"
        && s.phase == "idle" && !s.needsAction();
}

bool CacheKeepalive::pending(const QString &host, const SessionInfo &session) const
{
    return m_states.value(SessionRowPreferences::key(host, session)).sent > 0;
}

QList<CacheKeepalive::Reply> CacheKeepalive::observe(const QString &host, const BoxState &box, double now)
{
    QList<Reply> consumed;
    if (!box.ok) return consumed;
    const auto values = saved();
    QSet<QByteArray> listed;
    for (const auto &s : box.sessions) {
        const auto key = SessionRowPreferences::key(host, s);
        const int limit = std::clamp(values.value(QString::fromLatin1(key.toHex())).toInt(), 0, int(MaxHours));
        const bool build = building(s);
        if ((!limit && !build) || s.state != "running" || s.conversationId.isEmpty()) continue;
        listed.insert(key);
        auto &st = m_states[key];
        if (st.run != s.runId || st.conversation != s.conversationId) {
            // Enabled, resumed or replaced: the cache was last used by the last reply.
            st = State{}; st.host = host; st.run = s.runId; st.conversation = s.conversationId;
            st.replyId = s.replyId; st.real = st.warmed = s.replyAt;
        }
        if (s.replyId != st.replyId) {
            if (st.sent > 0 && s.replyAt + 1 >= st.sent) { consumed << Reply{host, s.name, s.conversationId, s.replyId}; st.warmed = s.replyAt; }
            else { st.real = st.warmed = s.replyAt > 0 ? s.replyAt : now; st.cold = false; st.inspected = false; }
            st.sent = 0; st.request = 0; st.replyId = s.replyId;
        }
        if (st.sent > 0) {
            // hgs does not count a keepalive's reply: idle again later means it ran.
            if ((idle(s) || build) && now - st.sent >= 20) { st.warmed = st.sent; st.sent = 0; st.request = 0; }
            else if (now - st.sent > 600) { st.sent = 0; st.request = 0; st.cold = true; }
            continue;
        }
        // A turn in progress uses the cache.
        if (s.activity == "busy") { st.real = st.warmed = now; st.cold = false; continue; }
        // A build keeps the context warm every 30 minutes, with the same
        // message and reply handling; the hours and the cold mark wait for it.
        if (build) {
            // Counted from the build's start or the last warming, whichever is later.
            const double since = std::max(st.warmed, s.build.value("since").toDouble());
            if (s.runId.isEmpty()) continue;
            if (since <= 0) { st.warmed = now; continue; }
            if (now - since < BuildInterval) continue;
            st.sent = now;
            st.request = m_client.requestSendMessage(host, s.name, message(), {}, s.runId, s.conversationId);
            m_requests.insert(st.request, key);
            continue;
        }
        if (!idle(s) || st.cold || s.runId.isEmpty() || st.warmed <= 0) continue;
        if (s.cmd == "claude" && !st.inspected) {
            st.inspected = true;
            if (m_client.requestInspection(host, s.name, 0, {}, false)) m_inspections.insert(host + '\n' + s.name, key);
        }
        if (now - st.real > limit * 3600.) continue;
        // Polls of other computers come about once a minute.
        const double margin = std::min(90., st.ttl / 3.);
        if (now >= st.warmed + st.ttl) { st.cold = true; continue; }
        if (now < st.warmed + st.ttl - margin) continue;
        st.sent = now;
        st.request = m_client.requestSendMessage(host, s.name, message(), {}, s.runId, s.conversationId);
        m_requests.insert(st.request, key);
    }
    for (auto it = m_states.begin(); it != m_states.end();) {
        if (it->host == host && !listed.contains(it.key())) it = m_states.erase(it); else ++it;
    }
    return consumed;
}
