#pragma once
#include <QByteArray>
#include <QElapsedTimer>
#include <QHash>
#include <QObject>
#include <QPointer>
#include <QString>
#include <QVariant>
#include <functional>

class QThreadPool;

// WhiteWh fork, #744 (the owner's tray, 2026-10-31 02:06: the GUI thread waited
// 39 s in QFileInfo::isDir for a session's folder while large folders on its disk
// were deleted). The GUI thread never asks the file system about a session's,
// project's or worktree's path itself: a worker does, bounded in time, and the
// answer is kept per path for a short while. A view shows "checking" until it
// answers and repaints on answered().
class PathProbe : public QObject {
    Q_OBJECT
public:
    struct Info {
        // Checking: no answer yet. Unresponsive: the worker has not answered
        // within the bound (the stat still runs; its answer replaces this one).
        enum State { Checking, Answered, Unresponsive } state = Checking;
        bool exists = false, dir = false, file = false;
        qint64 size = -1;
        QString canonical;
        bool answered() const { return state == Answered; }
    };
    // How long an answer is used as it is, how long it is still shown while it
    // is read again, and how long a stat may take before it counts as not
    // answering.
    static constexpr int FreshMs = 15 * 1000;
    static constexpr int StaleMs = 2 * 60 * 1000;
    static constexpr int DefaultTimeoutMs = 3000;
    static constexpr int MaxPaths = 512;

    static PathProbe &instance();
    // The kept answer for `path`: fresh, or a little stale while a worker reads
    // it again; Checking (a read under way) without one. Never waits.
    Info info(const QString &path);
    // A fresh answer for an action (open the folder): `done` runs on the GUI
    // thread once the worker answers or the bound passes (Unresponsive), and
    // not at all once `context` is gone.
    void check(const QString &path, QObject *context, std::function<void(const Info &)> done);
    // A folder and its .git (a team's or a project's folder), from info().
    struct Folder { bool checking = false, unresponsive = false, dir = false, git = false; };
    Folder folder(const QString &path);
    static QString gitOf(const QString &path);
    // Up to `limit` bytes of the file at `path`, read by a worker; ok is false
    // when it cannot be read, is larger than `limit` or does not answer in time.
    void read(const QString &path, qint64 limit, QObject *context, std::function<void(bool ok, const QByteArray &bytes)> done);
    // Other work on a path (decoding a picture), run by a worker with the same
    // bound: `done` gets ok=false when it has not answered in time.
    void run(QObject *context, std::function<QVariant()> work, std::function<void(bool ok, const QVariant &value)> done);

    // Tests: the stat a worker runs (a slow one stands for a blocked disk) and
    // the time bound; an empty stat restores QFileInfo. clear() forgets every
    // answer.
    using Stat = std::function<Info(const QString &)>;
    static void setStat(Stat stat);
    static void setTimeout(int milliseconds);
    static Info statOf(const QString &path);
    void clear();

signals:
    // An answer for `path` arrived, or its bound passed.
    void answered(const QString &path);

private:
    PathProbe();
    struct Waiter { QPointer<QObject> context; std::function<void(const Info &)> done; };
    struct Entry {
        Info info;
        QElapsedTimer at;            // since the last answer
        bool reading = false;
        bool late = false;           // the read under way passed the bound
        quint64 generation = 0;
        QList<Waiter> waiting;
    };
    void start(const QString &path);
    void finish(const QString &path, quint64 generation, const Info &info);
    void expire(const QString &path, quint64 generation);
    void trim();
    QHash<QString, Entry> m_entries;
    QThreadPool *m_pool = nullptr;   // never deleted: a stat that hangs must not hold the exit
    quint64 m_generation = 0;
};
