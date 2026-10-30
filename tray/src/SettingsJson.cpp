#include "SettingsJson.h"

#include "JsonParse.h"
#include "ProcessRunner.h"
#include "Settings.h"

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QHash>
#include <QJsonDocument>
#include <QList>
#include <QMutex>
#include <QPointer>
#include <QSaveFile>
#include <QSet>
#include <QSettings>

#include <chrono>
#include <condition_variable>
#include <mutex>
#include <optional>
#include <thread>

namespace SettingsJson {
namespace {

struct Entry {
    QByteArray bytes;
    QJsonDocument document;
    // A file key's value was read from its file (or migrated): it is the truth.
    bool loaded = false;
};

QMutex s_mutex;
QHash<QString, Entry> s_entries;
// Writes per key: a parse that started before a write does not replace it.
QHash<QString, quint64> s_writes;
// Keys whose stored bytes the preparation thread is parsing for the GUI thread.
QSet<QString> s_reparsing;

// The values too big for the INI: each lives in a file of its own beside it.
const QHash<QString, QString> &fileNames()
{
    static const QHash<QString, QString> names{
        {QStringLiteral("workspace/worktreeSnapshots"), QStringLiteral("worktree-snapshots")},
        {QStringLiteral("workspace/organization"), QStringLiteral("organization")},
        {QStringLiteral("swarm/observedOrganization"), QStringLiteral("swarm-observed-organization")},
        {QStringLiteral("swarm/observedVersions"), QStringLiteral("swarm-observed-versions")},
    };
    return names;
}

// A file key's value that is not a JSON value we would ever store.
constexpr qint64 MaximumFileBytes = 32 * 1024 * 1024;

// The files' writer: the newest bytes per key, written in order with QSaveFile.
struct Files {
    std::mutex mutex;
    std::condition_variable wake;
    std::condition_variable written;
    // Path and bytes (nullopt: remove) in the order they were asked for.
    QList<std::pair<QString, std::optional<QByteArray>>> queue;
    bool writing = false;
    bool stop = false;
    std::thread thread;
};

Files &files()
{
    static Files instance;
    return instance;
}

bool writeFile(const QString &path, const QByteArray &bytes)
{
    // The settings folder exists once QSettings wrote there; a fresh profile
    // may store a file key first.
    QDir().mkpath(QFileInfo(path).absolutePath());
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) return false;
    file.write(bytes);
    return file.commit();
}

void writeFiles()
{
    auto &f = files();
    for (;;) {
        QList<std::pair<QString, std::optional<QByteArray>>> batch;
        {
            std::unique_lock lock(f.mutex);
            f.wake.wait(lock, [&f] { return !f.queue.isEmpty() || f.stop; });
            if (f.queue.isEmpty()) return;
            batch.swap(f.queue);
            f.writing = true;
        }
        // Only the last write of each file in the batch matters.
        QSet<QString> done;
        for (auto it = batch.crbegin(); it != batch.crend(); ++it) {
            if (done.contains(it->first)) continue;
            done.insert(it->first);
            if (it->second) writeFile(it->first, *it->second);
            else QFile::remove(it->first);
        }
        std::lock_guard lock(f.mutex);
        f.writing = false;
        f.written.notify_all();
    }
}

void queueFile(const QString &path, std::optional<QByteArray> bytes)
{
    auto &f = files();
    std::lock_guard lock(f.mutex);
    f.queue.append({path, std::move(bytes)});
    if (!f.thread.joinable()) {
        f.thread = std::thread(writeFiles);
        qAddPostRoutine([] {
            flush();
            auto &f = files();
            { std::lock_guard lock(f.mutex); f.stop = true; }
            f.wake.notify_all();
            if (f.thread.joinable()) f.thread.join();
        });
    }
    f.wake.notify_all();
}

// A file key's stored bytes: its file, or once, the INI value it replaces,
// which then moves into the file and leaves the INI.
QByteArray loadFile(const QString &key)
{
    const QString path = filePath(key);
    QFile file(path);
    if (file.open(QIODevice::ReadOnly)) return file.size() > MaximumFileBytes ? QByteArray() : file.readAll();
    const QByteArray legacy = QSettings().value(key).toByteArray();
    if (!legacy.isEmpty() && writeFile(path, legacy)) Settings::remove(key);
    return legacy;
}

QStringList expand(const QStringList &patterns)
{
    QSettings settings; QStringList keys;
    for (const auto &pattern : patterns) {
        const auto star = pattern.indexOf(QLatin1String("/*/"));
        if (star < 0) { keys << pattern; continue; }
        const auto group = pattern.left(star);
        settings.beginGroup(group);
        const auto children = settings.childGroups();
        settings.endGroup();
        for (const auto &child : children) keys << group + QLatin1Char('/') + child + pattern.mid(star + 2);
    }
    return keys;
}

// Parses bytes read for key and records them unless a writer stored newer ones meanwhile.
QJsonDocument parseRead(const QString &key, const QByteArray &bytes, quint64 writes, const char *file, int line)
{
    const QJsonDocument document = bytes.isEmpty() ? QJsonDocument() : JsonParse::parse(bytes, nullptr, file, line);
    QMutexLocker lock(&s_mutex);
    if (s_writes.value(key) == writes) s_entries.insert(key, {bytes, document, inFile(key)});
    return document;
}

// Off the GUI thread: parses the stored value of each key.
void parseStored(const QStringList &keys)
{
    for (const auto &key : expand(keys)) {
        quint64 writes;
        {
            QMutexLocker lock(&s_mutex);
            // A file key is read once; its copy here stays the truth.
            if (inFile(key) && s_entries.value(key).loaded) continue;
            writes = s_writes.value(key);
        }
        parseRead(key, inFile(key) ? loadFile(key) : QSettings().value(key).toByteArray(), writes, __FILE__, __LINE__);
    }
}

QObject *reparser()
{
    static QPointer<QObject> owner;
    if (!owner) owner = new QObject(QCoreApplication::instance());
    return owner;
}

// The GUI thread found nothing parsed: the last value, and a parse for the
// reads after this one.
QJsonDocument unprepared(const QString &key, const QJsonDocument &last, const char *file, int line)
{
    const bool scheduled = s_reparsing.contains(key);
    s_reparsing.insert(key);
    s_mutex.unlock();
    JsonParse::reportUnprepared(file, line);
    if (!scheduled)
        ProcessRunner::post(reparser(), [key] { parseStored({key}); return true; },
                            [key](bool) { QMutexLocker done(&s_mutex); s_reparsing.remove(key); });
    return last;
}

QJsonDocument readFileKey(const QString &key, qsizetype maximumBytes, const char *file, int line)
{
    s_mutex.lock();
    const auto found = s_entries.constFind(key);
    if (found != s_entries.cend() && found->loaded) {
        const Entry entry = *found;
        s_mutex.unlock();
        return maximumBytes > 0 && entry.bytes.size() > maximumBytes ? QJsonDocument() : entry.document;
    }
    const quint64 writes = s_writes.value(key);
    if (!JsonParse::onGuiThread()) {
        s_mutex.unlock();
        const QByteArray bytes = loadFile(key);
        const QJsonDocument document = parseRead(key, bytes, writes, file, line);
        return maximumBytes > 0 && bytes.size() > maximumBytes ? QJsonDocument() : document;
    }
    // Nothing stored anywhere: nothing to parse.
    if (!QFileInfo::exists(filePath(key)) && !QSettings().contains(key)) {
        s_entries.insert(key, {QByteArray(), QJsonDocument(), true});
        s_mutex.unlock();
        return {};
    }
    return unprepared(key, found != s_entries.cend() ? found->document : QJsonDocument(), file, line);
}

QJsonDocument read(const QString &key, qsizetype maximumBytes, const char *file, int line)
{
    if (inFile(key)) return readFileKey(key, maximumBytes, file, line);
    const QByteArray bytes = QSettings().value(key).toByteArray();
    if (maximumBytes > 0 && bytes.size() > maximumBytes) return {};
    s_mutex.lock();
    const auto found = s_entries.constFind(key);
    if (found != s_entries.cend() && found->bytes == bytes) {
        const QJsonDocument document = found->document;
        s_mutex.unlock();
        return document;
    }
    // Nothing to parse.
    if (bytes.isEmpty()) { s_entries.insert(key, {}); s_mutex.unlock(); return {}; }
    const quint64 writes = s_writes.value(key);
    if (!JsonParse::onGuiThread()) {
        s_mutex.unlock();
        return parseRead(key, bytes, writes, file, line);
    }
    return unprepared(key, found != s_entries.cend() ? found->document : QJsonDocument(), file, line);
}

}

bool inFile(const QString &key) { return fileNames().contains(key); }

QString filePath(const QString &key)
{
    return QSettings().fileName() + QLatin1Char('.') + fileNames().value(key) + QStringLiteral(".json");
}

QJsonDocument document(const QString &key, qsizetype maximumBytes, const char *file, int line)
{
    return read(key, maximumBytes, file, line);
}
QJsonObject object(const QString &key, qsizetype maximumBytes, const char *file, int line)
{
    return read(key, maximumBytes, file, line).object();
}
QJsonArray array(const QString &key, const char *file, int line) { return read(key, 0, file, line).array(); }

QByteArray bytes(const QString &key)
{
    if (!inFile(key)) return QSettings().value(key).toByteArray();
    {
        QMutexLocker lock(&s_mutex);
        const auto found = s_entries.constFind(key);
        if (found != s_entries.cend() && found->loaded) return found->bytes;
    }
    return loadFile(key);
}

bool contains(const QString &key) { return inFile(key) ? !bytes(key).isEmpty() : QSettings().contains(key); }

void remember(const QString &key, const QByteArray &bytes, const QJsonDocument &document)
{
    QMutexLocker lock(&s_mutex);
    ++s_writes[key];
    s_entries.insert(key, {bytes, document, inFile(key)});
}

void store(const QString &key, const QByteArray &bytes, const QJsonDocument &document)
{
    remember(key, bytes, document);
    if (inFile(key)) queueFile(filePath(key), bytes);
    else Settings::set(key, bytes);
}

void store(QSettings &, const QString &key, const QJsonDocument &document)
{
    store(key, document);
}

void store(const QString &key, const QJsonDocument &document)
{
    store(key, document.toJson(QJsonDocument::Compact), document);
}

bool storeNow(const QString &key, const QJsonDocument &document)
{
    const QByteArray bytes = document.toJson(QJsonDocument::Compact);
    if (inFile(key)) { store(key, bytes, document); return true; }
    if (!Settings::setNow(key, bytes)) return false;
    remember(key, bytes, document);
    return true;
}

void remove(const QString &key)
{
    remember(key, QByteArray(), QJsonDocument());
    if (inFile(key)) {
        queueFile(filePath(key), std::nullopt);
        // A value an older build left in the INI goes too.
        if (QSettings().contains(key)) Settings::remove(key);
    } else {
        Settings::remove(key);
    }
}

void flush()
{
    auto &f = files();
    std::unique_lock lock(f.mutex);
    f.written.wait_for(lock, std::chrono::seconds(10), [&f] { return f.queue.isEmpty() && !f.writing; });
}

void reset()
{
    flush();
    for (auto it = fileNames().cbegin(); it != fileNames().cend(); ++it) {
        QFile::remove(filePath(it.key()));
        forget(it.key());
    }
}

void forget(const QString &key)
{
    QMutexLocker lock(&s_mutex);
    ++s_writes[key];
    s_entries.remove(key);
}

void warm(QObject *owner, const QStringList &keys)
{
    ProcessRunner::post(owner, [keys] { parseStored(keys); return true; }, [](bool) {});
}

void preload(const QStringList &keys)
{
    std::thread([&keys] { parseStored(keys); }).join();
}

}
