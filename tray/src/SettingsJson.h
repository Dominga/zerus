#pragma once

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>
#include <QStringList>

class QObject;

// JSON values the desktop keeps in QSettings, parsed once per change: a read
// whose stored bytes equal the previous read's returns the parsed value again
// without parsing. warm() parses keys on the preparation thread
// (ProcessRunner.h) ahead of their first read, so the GUI thread normally
// parses only values it has just changed. A parse that is still needed on the
// GUI thread goes through JsonParse::local() and its size limit.
namespace SettingsJson {

// A stored value over maximumBytes (0: any size) reads as empty.
QJsonObject object(const QString &key, qsizetype maximumBytes = 0);
QJsonArray array(const QString &key);
void warm(QObject *owner, const QStringList &keys);
// A writer that has just stored bytes, serialized from document, under key
// records them, so the next read needs no parse.
void remember(const QString &key, const QByteArray &bytes, const QJsonDocument &document);

}
