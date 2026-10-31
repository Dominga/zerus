#include "TeamController.h"
#include "HgsClient.h"
#include "SlowEventLog.h"
#include <QJsonDocument>
#include <QUuid>

namespace {
int pollInterval = 3000;
// #706: reads change nothing, so their answers ask for no new poll (the
// waiting list's every 30 s, the profile and heat reads).
bool readOnly(const QString &operation)
{
    // #719: `test` reads the test database; a submit changes no team state.
    static const QStringList reads{"waiting", "profile", "spans", "status", "events", "inbox", "list", "test"};
    return reads.contains(operation);
}
}

void TeamController::setPollInterval(int milliseconds) { pollInterval = milliseconds; }

TeamController::TeamController(HgsClient *client, QObject *parent) : QObject(parent), m_client(client)
{
    m_poll.setObjectName(QStringLiteral("teamPoll"));
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
    if (!m_listRequest) m_listRequest = m_client->requestTeam({}, QStringLiteral("list"), {{"include_cancelled", true}});
    readWatched();
    if ((m_watchCatalog || m_watchCatalogPage) && !m_catalogInFlight) refreshCatalog();
}

// C2: the lead's inbox, read with the rest of the watched team's state.
void TeamController::refreshInbox()
{
    if (m_watch.isEmpty() || m_inboxRequest) return;
    m_inboxRequest = m_client->requestTeam({}, QStringLiteral("inbox"), {{"team", m_watch}});
}

void TeamController::readWatched()
{
    if (m_watch.isEmpty()) return;
    if (!m_statusRequest) m_statusRequest = m_client->requestTeam({}, QStringLiteral("status"), {{"team", m_watch}, {"include_cancelled", true}});
    if (!m_feedRequest) m_feedRequest = m_client->requestTeam({}, QStringLiteral("events"), {{"team", m_watch}});
    if (!m_inboxRequest) refreshInbox();
}

void TeamController::setShowCancelled(bool on)
{
    if (m_showCancelled == on) return;
    m_showCancelled = on;
    emit showCancelledChanged();
}

QByteArray TeamController::shapeOf(const QString &team) const
{
    if (watched() && m_watched.id == team && !m_watchedShape.isEmpty()) return m_watchedShape;
    return m_listShapes.value(team);
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
    m_watched = {}; m_lastStatus = {}; m_watchedShape.clear(); m_inboxShape.clear();
    m_feed.clear(); m_feedCursor = QJsonValue();
    m_statusRequest = m_feedRequest = m_olderRequest = 0;
    m_inbox.clear(); m_lastInbox = {}; m_inboxRequest = 0;
    emit watchedChanged(); emit feedChanged(); emit inboxChanged();
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

void TeamController::act(const QString &operation, QJsonObject input, const Done &done)
{
    if (!input.contains("team")) input.insert("team", m_watch);
    input.insert("by", "human");
    // A resend after a failure keeps its request_id: the store answers the same
    // receipt instead of applying the change twice. A success drops the id, so
    // a later deliberate action gets a fresh one.
    const QString key = operation + QLatin1Char('\n') + QString::fromUtf8(QJsonDocument(input).toJson(QJsonDocument::Compact));
    QString requestId = m_retryIds.value(key);
    if (requestId.isEmpty()) {
        requestId = QUuid::createUuid().toString(QUuid::WithoutBraces);
        if (m_retryIds.size() >= 64) m_retryIds.erase(m_retryIds.begin());
    }
    m_retryIds.insert(key, requestId);
    input.insert("request_id", requestId);
    const quint64 request = m_client->requestTeam({}, operation, input);
    m_actions.insert(request, [this, key, done](bool ok, const QString &message) {
        if (ok) m_retryIds.remove(key);
        if (done) done(ok, message);
    });
}

void TeamController::approvePhase(int phase, const Done &done)
{
    act(QStringLiteral("approve-phase"), {{"phase", phase}}, done);
}

void TeamController::discardPhase(int phase, const Done &done)
{
    act(QStringLiteral("discard-phase"), {{"phase", phase}}, done);
}

void TeamController::finishTeam(const QString &summary, const Done &done)
{
    act(QStringLiteral("finish"), {{"summary", summary}}, done);
}

void TeamController::holdTask(int task, const QString &reason, const Done &done)
{
    act(QStringLiteral("hold"), {{"task", task}, {"reason", reason}}, done);
}

void TeamController::releaseTask(int task, const Done &done)
{
    act(QStringLiteral("release"), {{"task", task}}, done);
}

void TeamController::rejectWithHold(int task, const QString &reason, const Done &done)
{
    // A5: the same reject, with the task parked for a later release.
    act(QStringLiteral("reject"), {{"task", task}, {"reason", reason}, {"hold", true}}, done);
}

void TeamController::answerQuestion(const QString &question, const QString &text, const Done &done)
{
    // A3: `reply`, not `answer` (that one stays for native questions).
    act(QStringLiteral("reply"), {{"question", question}, {"text", text}}, done);
}

void TeamController::call(const QString &operation, QJsonObject input, const Reply &done, bool change, int timeoutMs)
{
    if (change) {
        input.insert("by", "human");
        input.insert("request_id", QUuid::createUuid().toString(QUuid::WithoutBraces));
    }
    m_calls.insert(m_client->requestTeam({}, operation, input, timeoutMs), done);
}

void TeamController::refreshCatalog()
{
    // A poll's read started before a decision may land after the read that
    // follows the decision; it would show the draft as it was.
    const quint64 read = ++m_catalogRead;
    m_catalogInFlight = true;
    call(QStringLiteral("catalog get"), {}, [this, read](bool ok, const QJsonObject &result, const QString &) {
        if (read != m_catalogRead) return;
        m_catalogInFlight = false;
        if (!ok) return;
        const auto next = Teams::parseCatalog(result);
        if (m_catalogReady && next.json() == m_catalog.json() && next.proposals.size() == m_catalog.proposals.size()) {
            bool same = true;
            for (int i = 0; i < next.proposals.size(); ++i) same = same && next.proposals[i].id == m_catalog.proposals[i].id && next.proposals[i].state == m_catalog.proposals[i].state;
            if (same) return;
        }
        m_catalog = Teams::parseCatalog(result);
        Teams::rememberRoleColors(m_catalog);
        m_catalogReady = true;
        emit catalogChanged();
    }, false);
}

void TeamController::decideProposal(const QString &proposal, const QString &decision, const Reply &done)
{
    call(QStringLiteral("catalog decide"), {{"proposal", proposal}, {"decision", decision}}, [this, proposal, done](bool ok, const QJsonObject &result, const QString &error) {
        if (!ok) { m_proposalErrors.insert(proposal, error); refreshCatalog(); emit catalogChanged(); }
        else if (m_proposalErrors.remove(proposal)) emit catalogChanged();
        if (done) done(ok, result, error);
    });
}

void TeamController::applyDraft(const Reply &done) { draftCall(QStringLiteral("catalog apply"), done); }

void TeamController::discardDraft(const Reply &done) { draftCall(QStringLiteral("catalog discard"), done); }

void TeamController::draftCall(const QString &operation, const Reply &done)
{
    call(operation, {}, [this, done](bool ok, const QJsonObject &result, const QString &error) {
        // Applied or discarded, the draft and its refusals are gone.
        if (ok) m_proposalErrors.clear();
        if (!ok || !m_draftError.isEmpty()) {
            m_draftError = ok ? QString() : error;
            if (!ok) refreshCatalog();
            emit catalogChanged();
        }
        if (done) done(ok, result, error);
    });
}

void TeamController::received(quint64 request, const QString &operation, const QJsonObject &result)
{
    // The slow-event log ranks each read apart (SlowEventLog.h).
    const SlowEventLog::Scope scope(this, request == m_listRequest ? "team list" : request == m_statusRequest ? "team status"
                                          : request == m_inboxRequest ? "team inbox" : request == m_feedRequest ? "team events" : "team call");
    if (m_calls.contains(request)) {
        const auto done = m_calls.take(request);
        if (done) done(true, result, {});
        if (operation.startsWith("catalog ")) {
            if (operation != "catalog get" && operation != "catalog route") refreshCatalog();
        } else if (!readOnly(operation)) refresh();
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
        const bool first = !m_available;
        m_available = true;
        if (result == m_lastList) return;
        m_lastList = result;
        m_teams = Teams::parseList(result);
        // #706: a running team's item moves its revision, updated time and
        // running times on every read. The teams keep the new values; their
        // listeners (the session list's rebuild among them) hear only of a
        // change they show.
        QStringList order;
        QHash<QString, QByteArray> shapes;
        for (const auto &item : result.value("items").toArray()) {
            const auto object = item.toObject();
            order << object.value("id").toString();
            shapes.insert(order.last(), Teams::visibleShape(object));
        }
        const bool changed = first || order != m_listOrder || shapes != m_listShapes;
        m_listOrder = order; m_listShapes = shapes;
        if (changed) emit teamsChanged();
    } else if (request == m_statusRequest) {
        m_statusRequest = 0;
        const auto team = Teams::parse(result);
        if (team.id != m_watch || (result == m_lastStatus && m_watched.id == m_watch)) return;
        m_lastStatus = result;
        m_watched = team;
        m_watchedShape = Teams::visibleShape(result);
        emit watchedChanged();
    } else if (request == m_inboxRequest) {
        m_inboxRequest = 0;
        if (result == m_lastInbox) return;
        m_lastInbox = result;
        m_inboxShape = QJsonDocument(result).toJson(QJsonDocument::Compact);
        m_inbox = Teams::parseInbox(result);
        emit inboxChanged();
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
        if (m_available || !m_teams.isEmpty()) { m_teams.clear(); m_lastList = {}; m_listShapes.clear(); m_listOrder.clear(); emit teamsChanged(); }
        m_available = false;
    } else if (request == m_statusRequest) m_statusRequest = 0;
    else if (request == m_inboxRequest) m_inboxRequest = 0;
    else if (request == m_feedRequest) m_feedRequest = 0;
    else if (request == m_olderRequest) m_olderRequest = 0;
    Q_UNUSED(operation);
}
