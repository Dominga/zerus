#include "PathProbe.h"

#include <QFile>
#include <QFileInfo>
#include <QMutex>
#include <QThreadPool>
#include <QTimer>
#include <algorithm>
#include <atomic>
#include <limits>

namespace {
QMutex statLock;
PathProbe::Stat customStat;
std::atomic<int> timeoutMs{PathProbe::DefaultTimeoutMs};

PathProbe::Stat currentStat()
{
    QMutexLocker lock(&statLock);
    return customStat ? customStat : PathProbe::Stat(&PathProbe::statOf);
}
}

PathProbe::PathProbe()
{
    m_pool = new QThreadPool;
    // A few reads at once: a disk that blocks holds at most these, and the
    // rest wait in the pool's queue (their bound still passes on time).
    m_pool->setMaxThreadCount(4);
}

PathProbe &PathProbe::instance()
{
    static auto *probe = new PathProbe;   // lives as long as the process
    return *probe;
}

void PathProbe::setStat(Stat stat)
{
    QMutexLocker lock(&statLock);
    customStat = std::move(stat);
}

void PathProbe::setTimeout(int milliseconds)
{
    timeoutMs = milliseconds > 0 ? milliseconds : DefaultTimeoutMs;
}

PathProbe::Info PathProbe::statOf(const QString &path)
{
    Info info;
    info.state = Info::Answered;
    const QFileInfo file(path);
    info.exists = file.exists();
    info.dir = info.exists && file.isDir();
    info.file = info.exists && (file.isFile() || file.isShortcut());
    info.size = info.file ? file.size() : -1;
    info.canonical = info.exists ? file.canonicalFilePath() : QString();
    return info;
}

void PathProbe::clear()
{
    // A read under way keeps its entry, so its answer still finds it.
    for (auto it = m_entries.begin(); it != m_entries.end();) {
        if (it->reading) { it->info = {}; it->late = false; ++it; }
        else it = m_entries.erase(it);
    }
}

PathProbe::Info PathProbe::info(const QString &path)
{
    if (path.isEmpty()) { Info none; none.state = Info::Answered; return none; }
    const Entry &entry = m_entries[path];
    if (entry.info.answered() && entry.at.isValid()) {
        if (!entry.at.hasExpired(FreshMs)) return entry.info;
        // A little stale: shown while a worker reads it again.
        if (!entry.at.hasExpired(StaleMs)) { const Info kept = entry.info; start(path); return kept; }
    }
    if (entry.reading && entry.late) { Info late; late.state = Info::Unresponsive; return late; }
    start(path);
    return Info{};
}

QString PathProbe::gitOf(const QString &path)
{
    return path.isEmpty() ? QString() : path + QStringLiteral("/.git");
}

PathProbe::Folder PathProbe::folder(const QString &path)
{
    Folder folder;
    if (path.isEmpty()) return folder;
    const Info dir = info(path), git = info(gitOf(path));
    folder.unresponsive = dir.state == Info::Unresponsive || git.state == Info::Unresponsive;
    folder.checking = !folder.unresponsive && (!dir.answered() || !git.answered());
    folder.dir = dir.answered() && dir.dir;
    folder.git = folder.dir && git.answered() && git.exists;
    return folder;
}

void PathProbe::check(const QString &path, QObject *context, std::function<void(const Info &)> done)
{
    if (path.isEmpty()) { Info none; none.state = Info::Answered; done(none); return; }
    auto &entry = m_entries[path];
    // An action uses only a very recent answer; anything older is read again.
    if (entry.info.answered() && entry.at.isValid() && !entry.at.hasExpired(1000)) { done(entry.info); return; }
    if (entry.reading && entry.late) { Info late; late.state = Info::Unresponsive; done(late); return; }
    entry.waiting.append({context, std::move(done)});
    start(path);
}

void PathProbe::read(const QString &path, qint64 limit, QObject *context, std::function<void(bool, const QByteArray &)> done)
{
    run(context, [path, limit] {
        QFile file(path);
        if (!file.open(QIODevice::ReadOnly) || file.size() > limit) return QVariant();
        return QVariant(file.read(limit));
    }, [done](bool ok, const QVariant &value) { done(ok && value.isValid(), value.toByteArray()); });
}

void PathProbe::run(QObject *context, std::function<QVariant()> work, std::function<void(bool, const QVariant &)> done)
{
    struct Pending { bool settled = false; };
    auto pending = std::make_shared<Pending>();
    QPointer<QObject> owner(context);
    const auto settle = [pending, owner, done](bool ok, const QVariant &value) {
        if (pending->settled || !owner) return;
        pending->settled = true;
        done(ok, value);
    };
    QPointer<PathProbe> self(this);
    m_pool->start([self, work, settle] {
        const QVariant value = work();
        if (self) QMetaObject::invokeMethod(self, [settle, value] { settle(true, value); }, Qt::QueuedConnection);
    });
    QTimer::singleShot(timeoutMs.load(), this, [settle] { settle(false, {}); });
}

void PathProbe::start(const QString &path)
{
    auto &entry = m_entries[path];
    if (entry.reading) return;
    entry.reading = true; entry.late = false;
    const quint64 generation = entry.generation = ++m_generation;
    QPointer<PathProbe> self(this);
    m_pool->start([self, path, generation, stat = currentStat()] {
        Info info = stat(path);
        info.state = Info::Answered;
        if (self) QMetaObject::invokeMethod(self, [self, path, generation, info] { if (self) self->finish(path, generation, info); }, Qt::QueuedConnection);
    });
    QTimer::singleShot(timeoutMs.load(), this, [this, path, generation] { expire(path, generation); });
    trim();
}

void PathProbe::finish(const QString &path, quint64 generation, const Info &info)
{
    const auto it = m_entries.find(path);
    if (it == m_entries.end() || it->generation != generation) return;
    it->reading = false; it->late = false;
    it->info = info; it->at.start();
    const auto waiting = std::exchange(it->waiting, {});
    for (const auto &waiter : waiting) if (waiter.context) waiter.done(info);
    emit answered(path);
}

void PathProbe::expire(const QString &path, quint64 generation)
{
    const auto it = m_entries.find(path);
    if (it == m_entries.end() || it->generation != generation || !it->reading || it->late) return;
    it->late = true;
    Info late; late.state = Info::Unresponsive;
    // A kept answer is no longer trusted: the path may have gone with its disk.
    it->info = late; it->at.invalidate();
    const auto waiting = std::exchange(it->waiting, {});
    for (const auto &waiter : waiting) if (waiter.context) waiter.done(late);
    emit answered(path);
}

void PathProbe::trim()
{
    if (m_entries.size() <= MaxPaths) return;
    // The oldest answers go first; a read under way stays.
    QList<QPair<qint64, QString>> answered;
    for (auto it = m_entries.cbegin(); it != m_entries.cend(); ++it)
        if (!it->reading) answered.append({it->at.isValid() ? it->at.elapsed() : std::numeric_limits<qint64>::max(), it.key()});
    std::sort(answered.begin(), answered.end(), [](const auto &a, const auto &b) { return a.first > b.first; });
    for (int i = 0; i < answered.size() && m_entries.size() > MaxPaths; ++i) m_entries.remove(answered[i].second);
}
