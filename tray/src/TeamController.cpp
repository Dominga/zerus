#include "TeamController.h"
#include "HgsClient.h"
#include <QUuid>

namespace {
int pollInterval = 3000;
}

void TeamController::setPollInterval(int milliseconds) { pollInterval = milliseconds; }

TeamController::TeamController(HgsClient *client, QObject *parent) : QObject(parent), m_client(client)
{
    m_poll.setInterval(pollInterval);
    connect(&m_poll, &QTimer::timeout, this, &TeamController::refresh);
    connect(m_client, &HgsClient::teamReady, this, [this](quint64 request, const QString &host, const QString &operation, const QJsonObject &result) {
        if (host.isEmpty()) received(request, operation, result);
    });
    connect(m_client, &HgsClient::teamFailed, this, [this](quint64 request, const QString &host, const QString &operation, const QString &error) {
        if (host.isEmpty()) failed(request, operation, error);
    });
}

void TeamController::setActive(bool active)
{
    if (m_active == active) return;
    m_active = active;
    if (active) { m_poll.start(); refresh(); } else m_poll.stop();
}

void TeamController::refresh()
{
    if (!m_listRequest) m_listRequest = m_client->requestTeam({}, QStringLiteral("list"), QJsonObject{});
    readWatched();
    if ((m_watchCatalog || m_watchCatalogPage) && !m_catalogInFlight) refreshCatalog();
}

void TeamController::readWatched()
{
    if (m_watch.isEmpty()) return;
    if (!m_statusRequest) m_statusRequest = m_client->requestTeam({}, QStringLiteral("status"), {{"team", m_watch}});
    if (!m_feedRequest) m_feedRequest = m_client->requestTeam({}, QStringLiteral("events"), {{"team", m_watch}});
}

const Teams::Team *TeamController::team(const QString &id) const
{
    for (const auto &t : m_teams) if (t.id == id) return &t;
    return nullptr;
}

const Teams::Team *TeamController::teamOfSession(const QString &host, const QString &session) const
{
    for (const auto &t : m_teams) if (t.memberOfSession(host, session)) return &t;
    return nullptr;
}

void TeamController::watch(const QString &team)
{
    if (m_watch == team) return;
    m_watch = team;
    m_watched = {}; m_lastStatus = {};
    m_feed.clear(); m_feedCursor = QJsonValue();
    m_statusRequest = m_feedRequest = m_olderRequest = 0;
    emit watchedChanged(); emit feedChanged();
    readWatched();
}

QList<QJsonObject> TeamController::feed() const
{
    QList<QJsonObject> items;
    for (auto it = m_feed.cend(); it != m_feed.cbegin();) items.append(*--it);
    return items;
}

void TeamController::loadOlder()
{
    if (m_watch.isEmpty() || m_olderRequest || m_feedCursor.isNull()) return;
    m_olderRequest = m_client->requestTeam({}, QStringLiteral("events"), {{"team", m_watch}, {"before", m_feedCursor}});
}

void TeamController::act(const QString &operation, QJsonObject input, const std::function<void(bool, const QString &)> &done)
{
    if (!input.contains("team")) input.insert("team", m_watch);
    input.insert("by", "human");
    input.insert("request_id", QUuid::createUuid().toString(QUuid::WithoutBraces));
    const quint64 request = m_client->requestTeam({}, operation, input);
    m_actions.insert(request, done);
}

void TeamController::call(const QString &operation, QJsonObject input, const Reply &done, bool change)
{
    if (change) {
        input.insert("by", "human");
        input.insert("request_id", QUuid::createUuid().toString(QUuid::WithoutBraces));
    }
    m_calls.insert(m_client->requestTeam({}, operation, input), done);
}

void TeamController::refreshCatalog()
{
    m_catalogInFlight = true;
    call(QStringLiteral("catalog get"), {}, [this](bool ok, const QJsonObject &result, const QString &) {
        m_catalogInFlight = false;
        if (!ok) return;
        const auto next = Teams::parseCatalog(result);
        if (m_catalogReady && next.json() == m_catalog.json() && next.proposals.size() == m_catalog.proposals.size()) {
            bool same = true;
            for (int i = 0; i < next.proposals.size(); ++i) same = same && next.proposals[i].id == m_catalog.proposals[i].id && next.proposals[i].state == m_catalog.proposals[i].state;
            if (same) return;
        }
        m_catalog = Teams::parseCatalog(result);
        m_catalogReady = true;
        emit catalogChanged();
    }, false);
}

void TeamController::received(quint64 request, const QString &operation, const QJsonObject &result)
{
    if (m_calls.contains(request)) {
        const auto done = m_calls.take(request);
        if (done) done(true, result, {});
        if (operation.startsWith("catalog ")) {
            if (operation != "catalog get" && operation != "catalog route") refreshCatalog();
        } else refresh();
        return;
    }
    if (m_actions.contains(request)) {
        const auto done = m_actions.take(request);
        if (done) done(true, result.value("status").toString());
        refresh();
        return;
    }
    if (request == m_listRequest) {
        m_listRequest = 0;
        m_available = true;
        if (result == m_lastList) return;
        m_lastList = result;
        m_teams = Teams::parseList(result);
        emit teamsChanged();
    } else if (request == m_statusRequest) {
        m_statusRequest = 0;
        const auto team = Teams::parse(result);
        if (team.id != m_watch || (result == m_lastStatus && m_watched.id == m_watch)) return;
        m_lastStatus = result;
        m_watched = team;
        emit watchedChanged();
    } else if (request == m_feedRequest || request == m_olderRequest) {
        const bool older = request == m_olderRequest;
        (older ? m_olderRequest : m_feedRequest) = 0;
        const auto items = result.value("items").toArray();
        bool changed = false;
        for (const auto &item : items) {
            const auto object = item.toObject();
            const qint64 seq = object.value("seq").toInteger();
            if (!m_feed.contains(seq)) { m_feed.insert(seq, object); changed = true; }
        }
        // The newest page sets the cursor only for the first read; older
        // pages move it back. Entries arriving meanwhile never shift a page.
        if (older || m_feedCursor.isNull() && m_feed.size() == items.size()) { m_feedCursor = result.value("next_cursor"); changed = true; }
        if (changed) emit feedChanged();
    }
    Q_UNUSED(operation);
}

void TeamController::failed(quint64 request, const QString &operation, const QString &error)
{
    if (m_calls.contains(request)) {
        const auto done = m_calls.take(request);
        if (done) done(false, {}, error);
        return;
    }
    if (m_actions.contains(request)) {
        const auto done = m_actions.take(request);
        if (done) done(false, error);
        refresh();
        return;
    }
    if (request == m_listRequest) {
        m_listRequest = 0;
        // No teams yet (or an hgs without them): nothing to show.
        if (m_available || !m_teams.isEmpty()) { m_teams.clear(); m_lastList = {}; emit teamsChanged(); }
        m_available = false;
    } else if (request == m_statusRequest) m_statusRequest = 0;
    else if (request == m_feedRequest) m_feedRequest = 0;
    else if (request == m_olderRequest) m_olderRequest = 0;
    Q_UNUSED(operation);
}
