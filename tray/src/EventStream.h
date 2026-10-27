#pragma once

#include "HgsClient.h"

#include <QHash>
#include <QJsonObject>
#include <QObject>
#include <QPointer>
#include <QSet>
#include <QThread>
#include <QTimer>

#include <functional>

class EventStreamWorker;

// One computer's `hgs [@host] watch --json` (docs/reference.md, Event stream).
// The child process, NDJSON framing, JSON parsing and the computer's catalog
// and Activity state live on the stream's own thread. The GUI thread receives
// finished values: the same BoxState a listing poll delivers, inspect-shaped
// Activity increments and git-status objects, at most one of each every 50 ms.
//
// A computer whose CLI lacks the watch-v1 marker stays Unsupported and keeps
// polling. A stream that fails restarts with a growing delay; while it is not
// Live, callers poll as before.
class EventStream : public QObject {
    Q_OBJECT
public:
    enum class State { Stopped, Probing, Starting, Live, Unsupported, Failed };
    Q_ENUM(State)

    EventStream(const QString &hgs, const QString &host, QObject *parent = nullptr);
    ~EventStream() override;

    QString host() const { return m_host; }
    State state() const { return m_state; }
    bool live() const { return m_state == State::Live; }
    void start();
    void stop();

    // Follows one session's Activity; a later call for another session of this
    // computer replaces it. after is the journal seq to resume after (0: tail).
    void follow(const QString &name, const QString &runId, const QString &conversationId, qint64 after);
    void unfollow();
    QString followed() const { return m_followedName; }

    // Follows the git status of these checkout folders, preferred first; the
    // first GitFolders count. A later call replaces the set. A computer whose
    // stream lacks the git topic keeps polling them.
    void watchGit(const QStringList &paths);
    // Whether the live stream follows this folder, so it needs no git-status
    // poll: its changes arrive as gitStatusReady.
    bool coversGit(const QString &path) const;
    static constexpr int GitFolders = 64;

    // Tests: frames and the delay before a restart.
    static constexpr int PublishMs = 50;
    static constexpr int SilenceMs = 30000;
    static int restartDelayMs(int failures);
    // Tests: the least time between applied listings (2 s) and the time after
    // which an unchanged listing is applied again (10 s).
    static void setListingTimingForTests(int gapMs, int refreshMs);
    // A provider message between tools, from an Activity `event` frame, joins
    // the summary's provider_messages: it replaces the one with its non-empty
    // message_id and type, is dropped as a repeat of one with its type and
    // text within 3 s, or is inserted in time order; the newest 100 stay.
    static void mergeProviderMessage(QJsonObject &summary, const QJsonObject &message);

signals:
    void stateChanged(EventStream::State state);
    void listingReady(const BoxState &box);
    void activityReady(const QString &name, const QJsonObject &inspection);
    // The followed session left the catalog or changed run; poll it instead.
    void activityEnded(const QString &name);
    // The `git-status --json` object of a followed folder, when it changed.
    void gitStatusReady(const QString &path, const QJsonObject &status);

private:
    friend class EventStreamWorker;
    void setState(State state);
    void launch();
    void failed(const QString &reason);
    void sendFollow();
    void sendGit();

    QString m_hgs, m_host;
    State m_state = State::Stopped;
    QThread m_thread;
    EventStreamWorker *m_worker = nullptr;
    QTimer m_restart;
    int m_failures = 0;
    qint64 m_lastFailure = 0;
    QString m_followedName, m_followedRun, m_followedConversation;
    qint64 m_followedAfter = 0;
    QStringList m_gitPaths;
    QSet<QString> m_gitWanted, m_gitRefused;
    // The running stream offers the git topic (its hello).
    bool m_gitTopic = false;
};

// The desktop's streams by computer alias ("" is this computer), owned by the
// tray agent. Windows read it to follow Activity and to skip polls.
class EventStreams : public QObject {
    Q_OBJECT
public:
    explicit EventStreams(const QString &hgs, QObject *parent = nullptr);
    ~EventStreams() override;
    static EventStreams *instance();
    // Off with ZERUS_EVENT_STREAM=0 or the setting dataPlane/eventStream=false.
    static bool enabled();

    EventStream *stream(const QString &host) const { return m_streams.value(host); }
    bool live(const QString &host) const { const auto *s = stream(host); return s && s->live(); }
    // Starts streams for these computers and stops the others.
    void setHosts(const QStringList &hosts);
    // Follows one session's Activity on its computer's stream and stops
    // following on every other stream.
    void follow(const QString &host, const QString &name, const QString &runId, const QString &conversationId, qint64 after);
    void unfollowAll();
    // Follows these checkout folders on each computer's stream (preferred
    // first); a computer missing from the map follows none.
    void watchGit(const QHash<QString, QStringList> &pathsByHost);
    bool coversGit(const QString &host, const QString &path) const { const auto *s = stream(host); return s && s->coversGit(path); }

signals:
    void listingReady(const QString &host, const BoxState &box);
    void activityReady(const QString &host, const QString &name, const QJsonObject &inspection);
    void activityEnded(const QString &host, const QString &name);
    void gitStatusReady(const QString &host, const QString &path, const QJsonObject &status);
    void stateChanged(const QString &host, EventStream::State state);

private:
    QString m_hgs;
    QHash<QString, EventStream *> m_streams;
};
