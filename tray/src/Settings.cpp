#include "Settings.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QList>
#include <QLockFile>
#include <QSettings>
#include <QSysInfo>
#include <QThread>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <iterator>
#include <mutex>
#include <thread>

#ifdef Q_OS_WIN
#include <windows.h>
#else
#include <cerrno>
#include <signal.h>
#endif

namespace Settings {
namespace {

struct Change {
    QString key;
    QVariant value;
    bool remove = false;
};

// The changes that wait for the lock, in order, and the thread that writes them.
struct Writer {
    std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable written;
    QList<Change> queue;
    bool writing = false;
    bool stop = false;
    std::thread thread;
};

Writer &writer()
{
    static Writer instance;
    return instance;
}

std::atomic<const char *> s_activity{nullptr};
std::atomic<LogLine> s_log{nullptr};

bool onGuiThread()
{
    const auto *app = QCoreApplication::instance();
    return app && QThread::currentThread() == app->thread();
}

// The lock is free now. tryLock(0) never waits; it takes over a lock whose
// process is gone, or one older than QLockFile's stale time.
bool lockFree()
{
    const QString path = lockPath();
    // QSettings creates its folder on its first write; the probe does too.
    QDir().mkpath(QFileInfo(path).absolutePath());
    QLockFile probe(path);
    if (probe.tryLock(0)) {
        probe.unlock();
        return true;
    }
    // Only a lock someone holds is busy; any other error is QSettings' to report.
    return probe.error() != QLockFile::LockFailedError;
}

bool apply(const QList<Change> &changes)
{
    QSettings settings;
    for (const auto &change : changes) {
        if (change.remove) settings.remove(change.key);
        else settings.setValue(change.key, change.value);
    }
    settings.sync();
    return settings.status() == QSettings::NoError;
}

void run()
{
    auto &w = writer();
    for (;;) {
        QList<Change> batch;
        {
            std::unique_lock lock(w.mutex);
            w.wake.wait(lock, [&w] { return !w.queue.isEmpty() || w.stop; });
            if (w.queue.isEmpty()) return;
            batch.swap(w.queue);
            w.writing = true;
        }
        // Never inside QSettings while the lock is held: its sync would keep
        // the in-process mutex every QSettings read takes.
        while (!lockFree()) {
            std::unique_lock lock(w.mutex);
            if (w.wake.wait_for(lock, std::chrono::milliseconds(100), [&w] { return w.stop; })) break;
        }
        apply(batch);
        std::lock_guard lock(w.mutex);
        w.writing = false;
        w.written.notify_all();
    }
}

void enqueue(Change change)
{
    auto &w = writer();
    std::lock_guard lock(w.mutex);
    w.queue.append(std::move(change));
    if (!w.thread.joinable()) {
        w.thread = std::thread(run);
        // Before the application goes, the queue is written, within a bound.
        qAddPostRoutine([] {
            flush();
            auto &w = writer();
            { std::lock_guard lock(w.mutex); w.stop = true; }
            w.wake.notify_all();
            if (w.thread.joinable()) w.thread.join();
        });
    }
    w.wake.notify_all();
}

void change(Change change)
{
    auto &w = writer();
    {
        std::lock_guard lock(w.mutex);
        // A queue in progress keeps the order: this change goes after it.
        if (!w.queue.isEmpty() || w.writing) {
            w.queue.append(std::move(change));
            w.wake.notify_all();
            return;
        }
    }
    if (onGuiThread() && !lockFree()) {
        enqueue(std::move(change));
        return;
    }
    const char *before = s_activity.exchange("writing the settings file");
    apply({change});
    s_activity.store(before);
}

#ifdef Q_OS_WIN
// Whether the process runs, and its executable when this user may read it.
bool running(qint64 pid, QString *image)
{
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE, FALSE, DWORD(pid));
    if (!process) return GetLastError() == ERROR_ACCESS_DENIED;
    const bool alive = WaitForSingleObject(process, 0) == WAIT_TIMEOUT;
    wchar_t buffer[1024];
    DWORD size = DWORD(std::size(buffer));
    if (QueryFullProcessImageNameW(process, 0, buffer, &size)) *image = QString::fromWCharArray(buffer, int(size));
    CloseHandle(process);
    return alive;
}
#else
bool running(qint64 pid, QString *image)
{
    *image = QFile::symLinkTarget(QStringLiteral("/proc/%1/exe").arg(pid));
    return ::kill(pid_t(pid), 0) == 0 || errno == EPERM;
}
#endif

// The last lock episode the janitor logged, so it says each once.
QString s_logged;
std::mutex s_janitor;

}

void set(const QString &key, const QVariant &value) { change({key, value, false}); }
void remove(const QString &key) { change({key, {}, true}); }

namespace {
bool now(const Change &change)
{
    {
        auto &w = writer();
        std::lock_guard lock(w.mutex);
        if (!w.queue.isEmpty() || w.writing) return false;
    }
    if (onGuiThread() && !lockFree()) return false;
    const char *before = s_activity.exchange("writing the settings file");
    const bool stored = apply({change});
    s_activity.store(before);
    return stored;
}
}

bool setNow(const QString &key, const QVariant &value) { return now({key, value, false}); }
bool removeNow(const QString &key) { return now({key, {}, true}); }

void flush()
{
    auto &w = writer();
    std::unique_lock lock(w.mutex);
    w.written.wait_for(lock, std::chrono::seconds(40), [&w] { return w.queue.isEmpty() && !w.writing; });
}

bool queued()
{
    auto &w = writer();
    std::lock_guard lock(w.mutex);
    return !w.queue.isEmpty() || w.writing;
}

QString lockPath() { return QSettings().fileName() + QStringLiteral(".lock"); }

QString checkLock(qint64 olderThanMs)
{
    std::lock_guard guard(s_janitor);
    const QString path = lockPath();
    const QFileInfo file(path);
    if (!file.exists()) { s_logged.clear(); return {}; }
    const qint64 age = file.lastModified().msecsTo(QDateTime::currentDateTime());
    if (age < olderThanMs) return {};
    qint64 pid = 0; QString host, app;
    if (!QLockFile(path).getLockInfo(&pid, &host, &app)) return {};
    const QString episode = QStringLiteral("%1@%2").arg(pid).arg(file.lastModified().toMSecsSinceEpoch());
    if (episode == s_logged) return {};
    s_logged = episode;
    QString image;
    const bool own = pid == QCoreApplication::applicationPid();
    const bool here = host.isEmpty() || host == QSysInfo::machineHostName();
    const bool alive = own || !here || running(pid, &image);
    if (own) image = QCoreApplication::applicationFilePath();
    QString text = QStringLiteral("Settings lock %1 held for %2 s by pid %3 (%4 on %5%6)%7")
        .arg(QDir::toNativeSeparators(path)).arg(double(age) / 1000.0, 0, 'f', 1).arg(pid)
        .arg(app.isEmpty() ? QStringLiteral("unknown application") : app, host.isEmpty() ? QStringLiteral("unknown host") : host,
             image.isEmpty() ? QString() : QStringLiteral(", ") + QDir::toNativeSeparators(image),
             own ? QStringLiteral("; this process, so it is kept and the settings changes wait in the background") : QString());
    // A lock whose process is gone protects nothing; one of a live process,
    // this one included, may be in the middle of a write.
    if (!alive && here) {
        text += QFile::remove(path) ? QStringLiteral("; the process is gone, so the lock was removed")
                                    : QStringLiteral("; the process is gone, but the lock could not be removed");
    }
    if (const auto log = s_log.load()) log("warning", text);
    return text;
}

void watchLock()
{
    static std::once_flag started;
    std::call_once(started, [] {
        static std::mutex mutex;
        static std::condition_variable wake;
        static bool stop = false;
        static std::thread thread([] {
            std::unique_lock lock(mutex);
            while (!wake.wait_for(lock, std::chrono::milliseconds(250), [] { return stop; })) {
                lock.unlock();
                checkLock();
                lock.lock();
            }
        });
        // It stops with the application, before QSettings loses its names.
        qAddPostRoutine([] {
            { std::lock_guard lock(mutex); stop = true; }
            wake.notify_all();
            if (thread.joinable()) thread.join();
        });
    });
}

void setLog(LogLine log) { s_log.store(log); }

const char *activity() { return s_activity.load(); }
void setActivity(const char *what) { s_activity.store(what); }

}
