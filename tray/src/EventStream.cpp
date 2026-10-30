#include "EventStream.h"
#include "Settings.h"

#include "ChildProcess.h"
#include "JsonParse.h"
#include "ProcessRunner.h"
#include "QuestionReply.h"

#include <QDateTime>
#include <QJsonArray>
#include <QJsonDocument>
#include <QProcess>
#include <QSettings>

#include <algorithm>
#include <atomic>
#include <utility>

namespace {
constexpr qsizetype MaximumLine = 1024 * 1024 + 64;
// Until fleet preparation leaves the GUI thread (M2), applying a listing costs
// a rebuild there (about 150 ms on a 2026-10-27 Mac sample); listings keep the
// 2 s polling cadence at most, while Activity arrives within PublishMs.
std::atomic<int> ListingGapMs{2000};
// Callers date a computer's data by its last listing: automatic approvals act
// on a peer listed within 30 s, and 3 minutes mark it stale. An unchanged
// listing is applied again after this long.
std::atomic<int> ListingRefreshMs{10000};
const QByteArray Marker = QByteArrayLiteral("Event stream ABI: watch-v1");
EventStreams *s_instance = nullptr;

QString sessionKey(const QString &name, const QString &runId)
{
    return runId.isEmpty() ? QStringLiteral("name:") + name : QStringLiteral("run:") + runId;
}

QByteArray line(const QJsonObject &object)
{
    return QJsonDocument(object).toJson(QJsonDocument::Compact) + '\n';
}
}

// Everything below runs on the stream's thread. It talks to its EventStream
// only through queued calls, which Qt drops once the stream is gone.
class EventStreamWorker : public QObject {
public:
    EventStreamWorker(EventStream *front, QString host) : m_front(front), m_host(std::move(host)) {}
    ~EventStreamWorker() override { stopProcess(); }

    void begin(const ProcessRunner::Request &request)
    {
        stopProcess(); reset();
        if (!m_publish) {
            m_publish = new QTimer(this); m_publish->setSingleShot(true);
            QObject::connect(m_publish, &QTimer::timeout, this, [this] { publish(); });
            m_silence = new QTimer(this); m_silence->setInterval(qMin(5000, ListingRefreshMs.load()));
            QObject::connect(m_silence, &QTimer::timeout, this, [this] {
                const auto now = QDateTime::currentMSecsSinceEpoch();
                if (now - m_lastFrame > EventStream::SilenceMs) return fail(QStringLiteral("no frame for 30 s"));
                if (m_ready && now - m_lastListing >= ListingRefreshMs.load()) { m_listingDirty = true; schedule(); }
            });
        }
        m_publish->stop();
        auto *process = m_process = new QProcess(this);
        process->setProgram(request.program); process->setArguments(request.arguments);
        if (request.environment) process->setProcessEnvironment(*request.environment);
        ChildProcess::inheritStdioOnly(*process);
        QObject::connect(process, &QProcess::readyReadStandardOutput, this, [this] { read(); });
        QObject::connect(process, &QProcess::readyReadStandardError, this, [this] {
            if (m_process) m_stderr = (m_stderr + m_process->readAllStandardError()).right(4000);
        });
        QObject::connect(process, &QProcess::finished, this, [this] {
            read();
            fail(m_stderr.trimmed().isEmpty() ? QStringLiteral("the stream ended") : QString::fromUtf8(m_stderr.trimmed()));
        });
        QObject::connect(process, &QProcess::errorOccurred, this, [this](QProcess::ProcessError error) {
            if (error == QProcess::FailedToStart) fail(QStringLiteral("cannot start the stream"));
        });
        m_lastFrame = QDateTime::currentMSecsSinceEpoch();
        m_silence->start();
        process->start();
    }
    void finish() { stopProcess(); reset(); }

    void follow(const QString &name, const QString &runId, const QString &conversationId, qint64 after)
    {
        if (!m_activity.key.isEmpty()) {
            write(QJsonObject{{"op", "unsubscribe"}, {"topic", "activity"},
                              {"session", QJsonObject{{"name", m_activity.name}, {"run_id", m_activity.run}}}});
        }
        m_activity = {};
        if (name.isEmpty()) return;
        m_activity.key = sessionKey(name, runId); m_activity.name = name; m_activity.run = runId;
        QJsonObject session{{"name", name}};
        if (!runId.isEmpty()) session["run_id"] = runId;
        if (!conversationId.isEmpty()) session["conversation_id"] = conversationId;
        QJsonObject subscribe{{"op", "subscribe"}, {"topic", "activity"}, {"session", session}};
        if (after > 0) subscribe["after"] = after; else subscribe["tail"] = 100;
        write(subscribe);
    }

    // Unsubscribes folders no longer wanted and subscribes new ones, in the
    // caller's order, up to the stream's limit.
    void watchGit(const QStringList &paths)
    {
        if (!m_gitTopic) return;
        const QSet<QString> wanted(paths.cbegin(), paths.cend());
        for (auto it = m_gitSubscribed.begin(); it != m_gitSubscribed.end();) {
            if (wanted.contains(*it)) { ++it; continue; }
            write(QJsonObject{{"op", "unsubscribe"}, {"topic", "git"}, {"path", *it}});
            m_gitStates.remove(*it);
            it = m_gitSubscribed.erase(it);
        }
        for (const auto &path : paths) {
            if (m_gitSubscribed.size() >= EventStream::GitFolders) break;
            if (m_gitSubscribed.contains(path)) continue;
            m_gitSubscribed.insert(path);
            write(QJsonObject{{"op", "subscribe"}, {"topic", "git"}, {"path", path}});
        }
    }

private:
    struct Activity {
        QString key, name, run;
        QJsonObject state;
        QJsonArray events;
        qint64 cursor = 0;
        bool replace = false, dirty = false;
    };

    void reset()
    {
        m_buffer.clear(); m_stderr.clear(); m_rows.clear(); m_pending.clear(); m_meta = {};
        m_catalog = m_haveMeta = m_listingDirty = m_ready = m_failed = false; m_activity = {};
        m_gitTopic = m_gitDirty = false; m_gitSubscribed.clear(); m_gitStates.clear();
    }
    void stopProcess()
    {
        if (!m_process) return;
        auto *process = std::exchange(m_process, nullptr);
        process->disconnect(this);
        if (process->state() != QProcess::NotRunning) {
            // EOF ends the stream; a peer's ssh closes with it.
            process->closeWriteChannel();
            if (!process->waitForFinished(1000)) { process->kill(); process->waitForFinished(1000); }
        }
        delete process;
        if (m_silence) m_silence->stop();
    }
    void write(const QJsonObject &operation)
    {
        if (m_process && m_process->state() == QProcess::Running) m_process->write(line(operation));
    }
    void fail(const QString &reason)
    {
        if (m_failed) return;
        // Nothing this stream prepared reaches the GUI after its failure.
        m_failed = true; m_publish->stop(); m_listingDirty = m_activity.dirty = m_gitDirty = false;
        stopProcess();
        QMetaObject::invokeMethod(m_front, [front = m_front, reason] { front->failed(reason); }, Qt::QueuedConnection);
    }
    void read()
    {
        if (!m_process || m_failed) return;
        m_buffer += m_process->readAllStandardOutput();
        qsizetype start = 0;
        for (;;) {
            const auto end = m_buffer.indexOf('\n', start);
            if (end < 0) break;
            if (end - start > MaximumLine) return fail(QStringLiteral("a frame exceeds 1 MiB"));
            QString error;
            const auto frame = JsonParse::object(m_buffer.mid(start, end - start), &error);
            start = end + 1;
            if (!error.isEmpty()) return fail(QStringLiteral("invalid frame: ") + error);
            m_lastFrame = QDateTime::currentMSecsSinceEpoch();
            handle(frame);
            if (m_failed) return;
        }
        m_buffer.remove(0, start);
        if (m_buffer.size() > MaximumLine) fail(QStringLiteral("a frame exceeds 1 MiB"));
    }
    void handle(const QJsonObject &frame)
    {
        if (frame.value("v").toInt() != 1) return fail(QStringLiteral("unsupported stream version"));
        const auto type = frame.value("t").toString(), topic = frame.value("topic").toString();
        if (type == "hello") {
            m_gitTopic = frame.value("topics").toArray().contains(QJsonValue(QStringLiteral("git")));
            QMetaObject::invokeMethod(m_front, [front = m_front, git = m_gitTopic] { front->m_gitTopic = git; }, Qt::QueuedConnection);
        } else if (type == "state" && topic == "git") {
            const auto path = frame.value("key").toString();
            if (m_gitSubscribed.contains(path)) { m_gitStates.insert(path, frame.value("value").toObject()); m_gitDirty = true; }
        } else if (type == "snapshot" && topic == "catalog") {
            if (frame.value("page").toInt() == 0) m_pending.clear();
            for (const auto &value : frame.value("items").toArray()) {
                const auto item = value.toObject();
                m_pending.insert(item.value("key").toString(), item.value("row").toObject());
            }
            if (frame.value("last").toBool()) { m_rows = std::exchange(m_pending, {}); m_catalog = m_listingDirty = true; }
        } else if (type == "delta" && topic == "catalog") {
            for (const auto &value : frame.value("upsert").toArray()) {
                const auto item = value.toObject();
                m_rows.insert(item.value("key").toString(), item.value("row").toObject());
            }
            for (const auto &value : frame.value("remove").toArray()) m_rows.remove(value.toObject().value("key").toString());
            m_listingDirty = true;
        } else if (type == "snapshot" && topic == "catalog.meta") {
            m_meta = frame.value("value").toObject(); m_haveMeta = m_listingDirty = true;
        } else if (topic == "activity" && frame.value("key").toString() == m_activity.key && !m_activity.key.isEmpty()) {
            if (type == "events") {
                for (const auto &event : frame.value("events").toArray()) m_activity.events.append(event);
                m_activity.cursor = frame.value("cursor").toInteger(); m_activity.dirty = true;
            } else if (type == "snapshot") {
                m_activity.events = frame.value("items").toArray(); m_activity.replace = true;
                m_activity.cursor = frame.value("cursor").toInteger(); m_activity.dirty = true;
            } else if (type == "state") {
                m_activity.state = frame.value("value").toObject(); m_activity.dirty = true;
            } else if (type == "event") {
                // A message between tools joins the summary; the next state
                // replaces provider_messages whole.
                if (!m_activity.state.isEmpty()) {
                    EventStream::mergeProviderMessage(m_activity.state, frame.value("event").toObject());
                    m_activity.dirty = true;
                }
            } else if (type == "gap") {
                m_activity.events = {}; m_activity.dirty = false;
            }
        } else if (type == "error") {
            if (frame.value("fatal").toBool()) return fail(frame.value("message").toString());
            const auto code = frame.value("code").toString();
            if (topic == "git") {
                // A refused or failing folder is polled instead.
                const auto path = frame.value("key").toString();
                if (code == "unsupported_topic") {
                    m_gitTopic = false; m_gitSubscribed.clear(); m_gitStates.clear();
                    QMetaObject::invokeMethod(m_front, [front = m_front] { front->m_gitTopic = false; }, Qt::QueuedConnection);
                } else if ((code == "too_many_subscriptions" || code == "bad_request" || code == "git_status_failed")
                           && m_gitSubscribed.remove(path)) {
                    // A folder whose checks fail stays subscribed on the server.
                    if (code == "git_status_failed") write(QJsonObject{{"op", "unsubscribe"}, {"topic", "git"}, {"path", path}});
                    m_gitStates.remove(path);
                    QMetaObject::invokeMethod(m_front, [front = m_front, path] { front->m_gitRefused.insert(path); }, Qt::QueuedConnection);
                }
            } else if ((code == "subscription_ended" || code == "session_changed") && frame.value("key").toString() == m_activity.key
                && !m_activity.key.isEmpty()) {
                const auto name = m_activity.name; m_activity = {};
                QMetaObject::invokeMethod(m_front, [front = m_front, name] { front->m_followedName.clear(); emit front->activityEnded(name); },
                                          Qt::QueuedConnection);
            }
        }
        // hello, heartbeat, pong and catalog gaps only prove the stream is alive;
        // after a catalog gap the previous rows stay until the next snapshot.
        schedule();
    }
    // Activity goes out within PublishMs; a listing waits out ListingGapMs
    // after the previous one, except the first.
    qint64 listingWait() const
    {
        if (!m_ready) return 0;
        return qMax<qint64>(0, ListingGapMs.load() - (QDateTime::currentMSecsSinceEpoch() - m_lastListing));
    }
    void schedule()
    {
        if (m_failed || m_publish->isActive()) return;
        if (m_activity.dirty || m_gitDirty) m_publish->start(EventStream::PublishMs);
        else if (m_listingDirty) m_publish->start(int(qMax<qint64>(EventStream::PublishMs, listingWait())));
    }
    void publish()
    {
        if (m_failed) return;
        if (m_listingDirty && m_catalog && m_haveMeta && listingWait() == 0) {
            // Rows in listing order, as ls --json sorts them.
            QList<QJsonObject> rows = m_rows.values();
            std::sort(rows.begin(), rows.end(), [](const QJsonObject &a, const QJsonObject &b) {
                const auto left = a.value("name").toString(), right = b.value("name").toString();
                const int folded = QString::compare(left, right, Qt::CaseInsensitive);
                return folded != 0 ? folded < 0 : left < right;
            });
            QJsonArray sessions; for (const auto &row : std::as_const(rows)) sessions.append(row);
            auto listing = m_meta; listing["sessions"] = sessions;
            BoxState box = HgsClient::listing(listing);
            // A peer's listing names the computer as it calls itself (requestPeer).
            if (!m_host.isEmpty()) box.reportedHost = std::exchange(box.host, m_host);
            m_listingDirty = false; m_lastListing = QDateTime::currentMSecsSinceEpoch();
            const bool first = !std::exchange(m_ready, true);
            QMetaObject::invokeMethod(m_front, [front = m_front, box, first] {
                if (first) front->setState(EventStream::State::Live);
                emit front->listingReady(box);
            }, Qt::QueuedConnection);
        }
        if (m_activity.dirty && !m_activity.state.isEmpty()) {
            auto inspection = m_activity.state;
            inspection["events"] = std::exchange(m_activity.events, {});
            inspection["cursor"] = m_activity.cursor;
            if (std::exchange(m_activity.replace, false)) inspection["replace_events"] = true;
            QuestionReply::annotate(inspection);
            m_activity.dirty = false;
            QMetaObject::invokeMethod(m_front, [front = m_front, name = m_activity.name, inspection] {
                if (front->m_followedName == name) {
                    front->m_followedAfter = inspection.value("cursor").toInteger();
                    emit front->activityReady(name, inspection);
                }
            }, Qt::QueuedConnection);
        }
        if (std::exchange(m_gitDirty, false)) {
            QList<std::pair<QString, QJsonObject>> states;
            for (auto it = m_gitStates.cbegin(); it != m_gitStates.cend(); ++it) states.append({it.key(), it.value()});
            m_gitStates.clear();
            QMetaObject::invokeMethod(m_front, [front = m_front, states] {
                for (const auto &[path, status] : states)
                    if (front->m_gitWanted.contains(path)) emit front->gitStatusReady(path, status);
            }, Qt::QueuedConnection);
        }
        schedule();
    }

    EventStream *m_front;
    QString m_host;
    QProcess *m_process = nullptr;
    QTimer *m_publish = nullptr, *m_silence = nullptr;
    QByteArray m_buffer, m_stderr;
    qint64 m_lastFrame = 0, m_lastListing = 0;
    QHash<QString, QJsonObject> m_rows, m_pending;
    QJsonObject m_meta;
    bool m_catalog = false, m_haveMeta = false, m_listingDirty = false, m_ready = false, m_failed = false;
    Activity m_activity;
    // Git: whether the stream offers the topic, the folders subscribed, and
    // the latest state of each not yet published.
    bool m_gitTopic = false, m_gitDirty = false;
    QSet<QString> m_gitSubscribed;
    QHash<QString, QJsonObject> m_gitStates;
};

EventStream::EventStream(const QString &hgs, const QString &host, QObject *parent)
    : QObject(parent), m_hgs(hgs), m_host(host)
{
    m_thread.setObjectName(host.isEmpty() ? QStringLiteral("hgs-stream") : QStringLiteral("hgs-stream-") + host);
    m_worker = new EventStreamWorker(this, host);
    m_worker->moveToThread(&m_thread);
    connect(&m_thread, &QThread::finished, m_worker, &QObject::deleteLater);
    m_thread.start();
    m_restart.setSingleShot(true);
    connect(&m_restart, &QTimer::timeout, this, [this] { start(); });
}

EventStream::~EventStream()
{
    m_restart.stop();
    QMetaObject::invokeMethod(m_worker, [worker = m_worker] { worker->finish(); }, Qt::BlockingQueuedConnection);
    m_thread.quit(); m_thread.wait();
}

void EventStream::setListingTimingForTests(int gapMs, int refreshMs)
{
    ListingGapMs = gapMs; ListingRefreshMs = refreshMs;
}

void EventStream::mergeProviderMessage(QJsonObject &summary, const QJsonObject &message)
{
    auto messages = summary.value(QStringLiteral("provider_messages")).toArray();
    const auto id = message.value(QStringLiteral("message_id")).toString();
    const auto type = message.value(QStringLiteral("type"));
    const double at = message.value(QStringLiteral("at")).toDouble();
    for (qsizetype i = 0; i < messages.size(); ++i) {
        const auto held = messages.at(i).toObject();
        if (held.value(QStringLiteral("type")) != type) continue;
        if (!id.isEmpty() && held.value(QStringLiteral("message_id")).toString() == id) {
            messages.removeAt(i);
            break;
        }
        if (held.value(QStringLiteral("detail")) == message.value(QStringLiteral("detail"))
            && qAbs(held.value(QStringLiteral("at")).toDouble() - at) < 3.0) return;
    }
    qsizetype at_index = messages.size();
    while (at_index > 0 && messages.at(at_index - 1).toObject().value(QStringLiteral("at")).toDouble() > at) --at_index;
    messages.insert(at_index, message);
    while (messages.size() > 100) messages.removeFirst();
    summary.insert(QStringLiteral("provider_messages"), messages);
}

int EventStream::restartDelayMs(int failures)
{
    static const int delays[] = {5000, 15000, 60000, 300000};
    return delays[qBound(0, failures - 1, 3)];
}

void EventStream::setState(State state)
{
    if (m_state == state) return;
    m_state = state;
    if (state == State::Live) { sendFollow(); sendGit(); }
    emit stateChanged(state);
}

void EventStream::start()
{
    if (m_state == State::Probing || m_state == State::Starting || m_state == State::Live) return;
    setState(State::Probing);
    QStringList help{QStringLiteral("--help")};
    if (!m_host.isEmpty()) help.prepend(QLatin1Char('@') + m_host);
    auto request = HgsClient::commandRequest(m_hgs, help); request.timeoutMs = m_host.isEmpty() ? 5000 : 15000;
    ProcessRunner::run(std::move(request), this,
        [](ProcessRunner::Result result) {
            return result.outcome == ProcessRunner::Result::Finished && result.exitCode == 0
                ? (result.standardOutput.contains(Marker) ? 1 : 0) : -1;
        },
        [this](int supported) {
            if (m_state != State::Probing) return;
            if (supported == 1) return launch();
            if (supported == 0) { setState(State::Unsupported); m_restart.start(600000); return; }
            failed(QStringLiteral("the computer did not answer"));
        });
}

void EventStream::launch()
{
    // A new stream answers again for the topics it offers and the folders it takes.
    m_gitTopic = false; m_gitRefused.clear();
    setState(State::Starting);
    QStringList args{QStringLiteral("watch"), QStringLiteral("--json")};
    if (!m_host.isEmpty()) args.prepend(QLatin1Char('@') + m_host);
    const auto request = HgsClient::commandRequest(m_hgs, args);
    QMetaObject::invokeMethod(m_worker, [worker = m_worker, request] { worker->begin(request); }, Qt::QueuedConnection);
}

void EventStream::stop()
{
    m_restart.stop();
    QMetaObject::invokeMethod(m_worker, [worker = m_worker] { worker->finish(); }, Qt::QueuedConnection);
    setState(State::Stopped);
}

void EventStream::failed(const QString &reason)
{
    if (m_state == State::Stopped) return;
    const auto now = QDateTime::currentMSecsSinceEpoch();
    // Failures more than ten minutes apart start the delays over.
    m_failures = now - m_lastFailure > 600000 ? 1 : m_failures + 1;
    m_lastFailure = now;
    qWarning().noquote() << "hgs zerus: event stream" << (m_host.isEmpty() ? QStringLiteral("(local)") : m_host) << "stopped:" << reason;
    setState(State::Failed);
    m_restart.start(restartDelayMs(m_failures));
}

void EventStream::follow(const QString &name, const QString &runId, const QString &conversationId, qint64 after)
{
    if (name == m_followedName && runId == m_followedRun && conversationId == m_followedConversation) return;
    m_followedName = name; m_followedRun = runId; m_followedConversation = conversationId; m_followedAfter = after;
    if (live()) sendFollow();
}

void EventStream::unfollow()
{
    if (m_followedName.isEmpty()) return;
    m_followedName.clear(); m_followedRun.clear(); m_followedConversation.clear(); m_followedAfter = 0;
    if (live()) sendFollow();
}

void EventStream::watchGit(const QStringList &paths)
{
    QStringList wanted; QSet<QString> set;
    for (const auto &path : paths) {
        if (wanted.size() >= GitFolders) break;
        if (path.isEmpty() || set.contains(path)) continue;
        set.insert(path); wanted.append(path);
    }
    if (wanted == m_gitPaths) return;
    m_gitPaths = wanted; m_gitWanted = set;
    m_gitRefused.intersect(m_gitWanted);
    if (live()) sendGit();
}

bool EventStream::coversGit(const QString &path) const
{
    return live() && m_gitTopic && m_gitWanted.contains(path) && !m_gitRefused.contains(path);
}

void EventStream::sendGit()
{
    QMetaObject::invokeMethod(m_worker, [worker = m_worker, paths = m_gitPaths] { worker->watchGit(paths); }, Qt::QueuedConnection);
}

void EventStream::sendFollow()
{
    QMetaObject::invokeMethod(m_worker, [worker = m_worker, name = m_followedName, run = m_followedRun,
                                         conversation = m_followedConversation, after = m_followedAfter] {
        worker->follow(name, run, conversation, after);
    }, Qt::QueuedConnection);
}

EventStreams::EventStreams(const QString &hgs, QObject *parent) : QObject(parent), m_hgs(hgs)
{
    Q_ASSERT(!s_instance);
    s_instance = this;
}

EventStreams::~EventStreams()
{
    if (s_instance == this) s_instance = nullptr;
    qDeleteAll(std::exchange(m_streams, {}));
}

EventStreams *EventStreams::instance() { return s_instance; }

bool EventStreams::enabled()
{
    if (qEnvironmentVariable("ZERUS_EVENT_STREAM") == QLatin1String("0")) return false;
    return Settings::value(QStringLiteral("dataPlane/eventStream"), true).toBool();
}

void EventStreams::setHosts(const QStringList &hosts)
{
    for (auto it = m_streams.begin(); it != m_streams.end();) {
        if (hosts.contains(it.key())) { ++it; continue; }
        delete it.value(); it = m_streams.erase(it);
    }
    for (const auto &host : hosts) {
        if (m_streams.contains(host)) continue;
        auto *stream = new EventStream(m_hgs, host, this);
        m_streams.insert(host, stream);
        connect(stream, &EventStream::listingReady, this, [this, host](const BoxState &box) { emit listingReady(host, box); });
        connect(stream, &EventStream::activityReady, this, [this, host](const QString &name, const QJsonObject &inspection) {
            emit activityReady(host, name, inspection);
        });
        connect(stream, &EventStream::activityEnded, this, [this, host](const QString &name) { emit activityEnded(host, name); });
        connect(stream, &EventStream::gitStatusReady, this, [this, host](const QString &path, const QJsonObject &status) {
            emit gitStatusReady(host, path, status);
        });
        connect(stream, &EventStream::stateChanged, this, [this, host](EventStream::State state) { emit stateChanged(host, state); });
        stream->start();
    }
}

void EventStreams::follow(const QString &host, const QString &name, const QString &runId, const QString &conversationId, qint64 after)
{
    for (auto it = m_streams.cbegin(); it != m_streams.cend(); ++it) {
        if (it.key() == host) it.value()->follow(name, runId, conversationId, after);
        else it.value()->unfollow();
    }
}

void EventStreams::unfollowAll()
{
    for (auto *stream : std::as_const(m_streams)) stream->unfollow();
}

void EventStreams::watchGit(const QHash<QString, QStringList> &pathsByHost)
{
    for (auto it = m_streams.cbegin(); it != m_streams.cend(); ++it) it.value()->watchGit(pathsByHost.value(it.key()));
}
