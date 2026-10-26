#pragma once
#include "ComposerDraftStore.h"
#include <QCryptographicHash>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QLockFile>
#include <QSaveFile>
#include <QSettings>
#include <QUuid>

// Private recovery snapshots, one file per task. Only a tiny marker goes in
// QSettings: polling/navigation must never load every pending image into memory.
// All methods that touch content run on the attachment worker, never the UI.
namespace TaskMessageOutbox {
struct Request { QJsonObject record; QString error; bool recovering = true; };
inline QString key(const QString &project, const QString &task) { return "tasks/messageRequest/" + project + '/' + task; }
inline QString path(const QString &project, const QString &task) {
    const auto digest = QCryptographicHash::hash((project + '\n' + task).toUtf8(), QCryptographicHash::Sha256).toHex();
    return ComposerDraftStore::directory() + "/task-delivery/" + QString::fromLatin1(digest) + ".json";
}
inline Request read(const QString &file) {
    const QFileInfo info(file); QFile input(file);
    if (info.isSymLink() || !info.isFile() || info.size() > 32 * 1024 * 1024 || !input.open(QIODevice::ReadOnly))
        return {{}, QObject::tr("The original message recovery snapshot is unavailable. This check sent nothing.")};
    const auto record = QJsonDocument::fromJson(input.readAll()).object();
    if (QUuid(record.value("id").toString()).isNull() || !record.value("intent").isObject())
        return {{}, QObject::tr("The message recovery snapshot is invalid. Nothing was submitted.")};
    return {record, {}};
}
inline Request reserve(const QString &project, const QString &task, const QJsonObject &intent) {
    const auto file = path(project, task); const auto folder = QFileInfo(file).absolutePath();
    if (!QDir().mkpath(folder) || QFileInfo(folder).isSymLink()) return {{}, QObject::tr("Could not create private message storage.")};
    QFile::setPermissions(ComposerDraftStore::directory(), QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner);
    QFile::setPermissions(folder, QFile::ReadOwner | QFile::WriteOwner | QFile::ExeOwner);
    QLockFile lock(file + ".lock"); if (!lock.tryLock(1000)) return {{}, QObject::tr("This task already has a message operation in progress.")};
    QSettings settings; const auto marker = key(project, task);
    Request request;
    if (QFileInfo::exists(file) || settings.contains(marker)) {
        request = read(file); if (!request.error.isEmpty()) return request;
        if (request.record.value("intent").toObject() != intent)
            return {{}, QObject::tr("Check the previous message before sending different content. Its delivery is still unconfirmed.")};
    } else {
        request.record = {{"id", QUuid::createUuid().toString(QUuid::WithoutBraces)}, {"intent", intent}}; request.recovering = false;
        QSaveFile output(file); const auto bytes = QJsonDocument(request.record).toJson(QJsonDocument::Compact);
        if (!output.open(QIODevice::WriteOnly) || !output.setPermissions(QFile::ReadOwner | QFile::WriteOwner)
            || output.write(bytes) != bytes.size() || !output.commit()) return {{}, QObject::tr("Could not save the original message. Nothing was submitted.")};
    }
    settings.setValue(marker, request.record.value("id").toString()); settings.sync();
    if (settings.status() != QSettings::NoError) return {{}, QObject::tr("Could not reserve the message identity. Nothing was submitted.")};
    return request;
}
inline QString release(const QString &project, const QString &task, const QString &id) {
    const auto file = path(project, task); QLockFile lock(file + ".lock");
    if (!lock.tryLock(1000)) return QObject::tr("The message recovery record is busy.");
    if (QFileInfo::exists(file)) {
        const auto request = read(file);
        if (!request.error.isEmpty()) return request.error;
        if (request.record.value("id").toString() != id) return QObject::tr("The pending message changed; refresh this task.");
        if (!QFile::remove(file)) return QObject::tr("Could not release the local message recovery record.");
    }
    QSettings settings; settings.remove(key(project, task)); settings.sync();
    return settings.status() == QSettings::NoError ? QString() : QObject::tr("Could not update message recovery state.");
}
}
