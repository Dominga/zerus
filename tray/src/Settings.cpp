#include "Settings.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
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
#include <cstdio>
#include <signal.h>
#endif

namespace Settings {
namespace {

struct Change {
    QString key;
    QVariant value;
    bool remove = false;
    // The order of every change, and a reservation's wish to hear its outcome.
    quint64 seq = 0;
    bool reported = false;
};

// The changes not yet written, in order, and the thread that writes them.
// `batch` is the part being written now; reads see both (value()).
struct Writer {
    std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable written;
    QList<Change> queue;
    QList<Change> batch;
    bool writing = false;
    bool stop = false;
    quint64 next = 0;
    // The outcome of the reservations written, by their seq; each is taken
    // by the caller that waits for it.
    QHash<quint64, bool> outcomes;
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

void log(const QString &text)
{
    if (const auto line = s_log.load()) line("warning", text);
}

// The INI file is written by Zerus itself under its lock; a format that is
// not an INI file (macOS' native preferences) has no lock to wait for and is
// written by QSettings as before.
bool iniFile(const QSettings &settings)
{
#if defined(Q_OS_MACOS) || defined(Q_OS_IOS)
    return settings.format() == QSettings::IniFormat;
#else
    return settings.format() == QSettings::IniFormat || settings.format() == QSettings::NativeFormat;
#endif
}

void applyTo(QSettings &settings, const QList<Change> &changes)
{
    for (const auto &change : changes) {
        if (change.remove) settings.remove(change.key);
        else settings.setValue(change.key, change.value);
    }
}

bool replaceFile(const QString &from, const QString &to)
{
    for (int attempt = 0; attempt < 40; ++attempt) {
#ifdef Q_OS_WIN
        if (MoveFileExW(reinterpret_cast<const wchar_t *>(QDir::toNativeSeparators(from).utf16()),
                        reinterpret_cast<const wchar_t *>(QDir::toNativeSeparators(to).utf16()),
                        MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH))
            return true;
#else
        if (::rename(QFile::encodeName(from).constData(), QFile::encodeName(to).constData()) == 0) return true;
#endif
        // A reader that has the INI open (another tray, a scanner) lets go soon.
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    return false;
}

// #709 N5: the lock is Zerus's own for the whole write, so no other writer
// gets in between a check and the write. QSettings would take the same lock in
// its sync, so the changes go into a copy (its own QSettings, its own lock)
// that replaces the INI in one rename. A leftover lock naming this process,
// whose delete failed, is removed again.
bool writeLocked(const QString &ini, const QList<Change> &changes, QLockFile &lock)
{
    const QString temp = ini + QStringLiteral(".writing");
    QFile::remove(temp);
    if (QFileInfo::exists(ini) && !QFile::copy(ini, temp)) return false;
    bool stored;
    {
        QSettings copy(temp, QSettings::IniFormat);
        applyTo(copy, changes);
        copy.sync();
        stored = copy.status() == QSettings::NoError;
    }
    stored = stored && replaceFile(temp, ini);
    if (!stored) QFile::remove(temp);
    lock.unlock();
    for (int attempt = 0; attempt < 10 && QFileInfo::exists(ini + QStringLiteral(".lock")); ++attempt) {
        qint64 pid = 0; QString host, app;
        if (!QLockFile(ini + QStringLiteral(".lock")).getLockInfo(&pid, &host, &app) || pid != QCoreApplication::applicationPid()) break;
        if (QFile::remove(ini + QStringLiteral(".lock"))) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }
    // Every QSettings of the INI in this process shares one cache: it reads the
    // new file now (a read-only sync takes no lock), before the queued copies
    // of these changes leave the overlay. Only after the lock is released: a
    // QSettings sync elsewhere in this process holds that cache while it waits
    // for the lock, and the two would wait for each other.
    if (stored) QSettings().sync();
    return stored;
}

// Writes one batch on the writer's thread. Waits for the lock while another
// process holds it, a bounded step at a time, so a stop ends the wait.
bool write(const QList<Change> &changes)
{
    QSettings settings;
    if (!iniFile(settings)) {
        applyTo(settings, changes);
        settings.sync();
        return settings.status() == QSettings::NoError;
    }
    const QString ini = settings.fileName();
    QDir().mkpath(QFileInfo(ini).absolutePath());
    QLockFile lock(ini + QStringLiteral(".lock"));
    auto &w = writer();
    while (!lock.tryLock(0)) {
        if (lock.error() != QLockFile::LockFailedError) return false;
        // Only this thread takes the lock in this process, so one naming this
        // process is a leftover whose delete failed (a scanner had it open):
        // it goes, rather than waiting the 30 s until it is stale.
        qint64 pid = 0; QString host, app;
        if (lock.getLockInfo(&pid, &host, &app) && pid == QCoreApplication::applicationPid()
            && (host.isEmpty() || host == QSysInfo::machineHostName()) && QFile::remove(ini + QStringLiteral(".lock")))
            continue;
        std::unique_lock wait(w.mutex);
        if (w.wake.wait_for(wait, std::chrono::milliseconds(100), [&w] { return w.stop; })) return false;
    }
    return writeLocked(ini, changes, lock);
}

void run()
{
    auto &w = writer();
    int failures = 0;
    for (;;) {
        QList<Change> batch;
        {
            std::unique_lock lock(w.mutex);
            w.wake.wait(lock, [&w] { return !w.queue.isEmpty() || w.stop; });
            if (w.queue.isEmpty()) return;
            w.batch.swap(w.queue);
            batch = w.batch;
            w.writing = true;
        }
        const bool stored = write(batch);
        bool retry = false, failed = false;
        {
            std::lock_guard lock(w.mutex);
            retry = !stored && !w.stop && ++failures < 5;
            if (retry) {
                // Tried again, before the changes queued behind it.
                w.queue = w.batch + w.queue;
            } else {
                failed = !stored;
                failures = 0;
                for (const auto &change : std::as_const(batch))
                    if (change.reported) w.outcomes.insert(change.seq, stored);
            }
            w.batch.clear();
            w.writing = false;
            w.written.notify_all();
        }
        if (failed) log(QStringLiteral("Settings: %1 changes could not be written to %2").arg(batch.size()).arg(QDir::toNativeSeparators(QSettings().fileName())));
        if (retry) std::this_thread::sleep_for(std::chrono::milliseconds(200));
    }
}

// Starts the writer once; called with the writer's mutex held.
void start(Writer &w)
{
    if (w.thread.joinable()) return;
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

quint64 enqueue(Change change)
{
    auto &w = writer();
    std::lock_guard lock(w.mutex);
    change.seq = ++w.next;
    const quint64 seq = change.seq;
    w.queue.append(std::move(change));
    start(w);
    w.wake.notify_all();
    return seq;
}

// The newest unwritten change that decides key: its own, or a removed group.
const Change *pending(const Writer &w, const QString &key)
{
    for (const auto *list : {&w.queue, &w.batch}) {
        for (auto it = list->crbegin(); it != list->crend(); ++it) {
            if (it->key == key || (it->remove && key.startsWith(it->key + QLatin1Char('/')))) return &*it;
        }
    }
    return nullptr;
}

// A reservation: queued like every change, then waited for within a bound.
// One that has not started when the bound passes is withdrawn, so false means
// nothing was stored.
bool now(Change change)
{
    change.reported = true;
    const quint64 seq = enqueue(std::move(change));
    auto &w = writer();
    std::unique_lock lock(w.mutex);
    const auto bound = std::chrono::milliseconds(onGuiThread() ? 1000 : 30000);
    const auto done = [&w, seq] { return w.outcomes.contains(seq); };
    if (!w.written.wait_for(lock, bound, done)) {
        for (qsizetype i = 0; i < w.queue.size(); ++i) {
            if (w.queue.at(i).seq == seq) { w.queue.removeAt(i); return false; }
        }
        // Being written now: that write is bounded once the lock is held.
        w.written.wait(lock, done);
    }
    return w.outcomes.take(seq);
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

void set(const QString &key, const QVariant &value) { enqueue({key, value, false}); }
void remove(const QString &key) { enqueue({key, {}, true}); }

bool setNow(const QString &key, const QVariant &value) { return now({key, value, false}); }
bool removeNow(const QString &key) { return now({key, {}, true}); }

QVariant value(const QString &key, const QVariant &defaultValue)
{
    {
        auto &w = writer();
        std::lock_guard lock(w.mutex);
        if (const Change *change = pending(w, key)) return change->remove ? defaultValue : change->value;
    }
    return QSettings().value(key, defaultValue);
}

bool contains(const QString &key)
{
    {
        auto &w = writer();
        std::lock_guard lock(w.mutex);
        if (const Change *change = pending(w, key)) return !change->remove;
    }
    return QSettings().contains(key);
}

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
    // A file just written may carry a time a little ahead of this clock.
    if (olderThanMs > 0 && age < olderThanMs) return {};
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
    log(text);
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

void useHome(const QString &home)
{
    QSettings::setDefaultFormat(QSettings::IniFormat);
#ifdef Q_OS_WIN
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, home + QStringLiteral("/AppData/Roaming"));
#else
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, home + QStringLiteral("/.config"));
#endif
}

void setLog(LogLine log) { s_log.store(log); }

const char *activity() { return s_activity.load(); }
void setActivity(const char *what) { s_activity.store(what); }

}
