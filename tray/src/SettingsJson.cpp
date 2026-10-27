#include "SettingsJson.h"

#include "JsonParse.h"
#include "ProcessRunner.h"

#include <QHash>
#include <QJsonDocument>
#include <QMutex>
#include <QSettings>

namespace SettingsJson {
namespace {

struct Entry {
    QByteArray bytes;
    QJsonDocument document;
};

QMutex s_mutex;
QHash<QString, Entry> s_entries;

QJsonDocument read(const QString &key, qsizetype maximumBytes = 0)
{
    const QByteArray bytes = QSettings().value(key).toByteArray();
    if (maximumBytes > 0 && bytes.size() > maximumBytes) return {};
    {
        QMutexLocker lock(&s_mutex);
        const auto found = s_entries.constFind(key);
        if (found != s_entries.cend() && found->bytes == bytes) return found->document;
    }
    const QJsonDocument document = JsonParse::local(bytes);
    QMutexLocker lock(&s_mutex);
    s_entries.insert(key, {bytes, document});
    return document;
}

}

QJsonObject object(const QString &key, qsizetype maximumBytes) { return read(key, maximumBytes).object(); }
QJsonArray array(const QString &key) { return read(key).array(); }

void remember(const QString &key, const QByteArray &bytes, const QJsonDocument &document)
{
    QMutexLocker lock(&s_mutex);
    s_entries.insert(key, {bytes, document});
}

void warm(QObject *owner, const QStringList &keys)
{
    ProcessRunner::post(owner, [keys] {
        for (const auto &key : keys) {
            const QByteArray bytes = QSettings().value(key).toByteArray();
            const QJsonDocument document = JsonParse::parse(bytes);
            QMutexLocker lock(&s_mutex);
            s_entries.insert(key, {bytes, document});
        }
        return true;
    }, [](bool) {});
}

}
