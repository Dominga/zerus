#include "SettingsJson.h"

#include "JsonParse.h"
#include "ProcessRunner.h"

#include <QCoreApplication>
#include <QHash>
#include <QJsonDocument>
#include <QMutex>
#include <QPointer>
#include <QSet>
#include <QSettings>

#include <thread>

namespace SettingsJson {
namespace {

struct Entry {
    QByteArray bytes;
    QJsonDocument document;
};

QMutex s_mutex;
QHash<QString, Entry> s_entries;
// Writes per key: a parse that started before a write does not replace it.
QHash<QString, quint64> s_writes;
// Keys whose stored bytes the preparation thread is parsing for the GUI thread.
QSet<QString> s_reparsing;

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
    const QJsonDocument document = JsonParse::parse(bytes, nullptr, file, line);
    QMutexLocker lock(&s_mutex);
    if (s_writes.value(key) == writes) s_entries.insert(key, {bytes, document});
    return document;
}

// Off the GUI thread: parses the stored value of each key.
void parseStored(const QStringList &keys)
{
    for (const auto &key : expand(keys)) {
        quint64 writes;
        { QMutexLocker lock(&s_mutex); writes = s_writes.value(key); }
        parseRead(key, QSettings().value(key).toByteArray(), writes, __FILE__, __LINE__);
    }
}

QObject *reparser()
{
    static QPointer<QObject> owner;
    if (!owner) owner = new QObject(QCoreApplication::instance());
    return owner;
}

QJsonDocument read(const QString &key, qsizetype maximumBytes, const char *file, int line)
{
    const QByteArray bytes = QSettings().value(key).toByteArray();
    if (maximumBytes > 0 && bytes.size() > maximumBytes) return {};
    QMutexLocker lock(&s_mutex);
    const auto found = s_entries.constFind(key);
    if (found != s_entries.cend() && found->bytes == bytes) return found->document;
    // Nothing to parse.
    if (bytes.isEmpty()) { s_entries.insert(key, {}); return {}; }
    const quint64 writes = s_writes.value(key);
    if (!JsonParse::onGuiThread()) {
        lock.unlock();
        return parseRead(key, bytes, writes, file, line);
    }
    const QJsonDocument last = found != s_entries.cend() ? found->document : QJsonDocument();
    const bool scheduled = s_reparsing.contains(key);
    s_reparsing.insert(key);
    lock.unlock();
    JsonParse::reportUnprepared(file, line);
    if (!scheduled)
        ProcessRunner::post(reparser(), [key] { parseStored({key}); return true; },
                            [key](bool) { QMutexLocker done(&s_mutex); s_reparsing.remove(key); });
    return last;
}

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

void remember(const QString &key, const QByteArray &bytes, const QJsonDocument &document)
{
    QMutexLocker lock(&s_mutex);
    ++s_writes[key];
    s_entries.insert(key, {bytes, document});
}

void store(QSettings &settings, const QString &key, const QJsonDocument &document)
{
    const QByteArray bytes = document.toJson(QJsonDocument::Compact);
    settings.setValue(key, bytes);
    remember(key, bytes, document);
}

void store(const QString &key, const QJsonDocument &document)
{
    QSettings settings;
    store(settings, key, document);
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
