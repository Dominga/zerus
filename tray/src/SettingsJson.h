#pragma once

#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>
#include <QStringList>

class QObject;
class QSettings;

// JSON values the desktop keeps in QSettings. The GUI thread never parses them:
// a read returns the value parsed for the stored bytes, which
//  - main() parses before the tray and its windows exist (preload()),
//  - warm() parses on the preparation thread (ProcessRunner.h), and
//  - every writer records together with the bytes it stores (store()).
// A GUI-thread read of bytes nobody parsed, because another process changed the
// value or a key is missing from the startup list, is reported like a parse
// (JsonParse.h, fatal in the Qt suites). It returns the value last parsed for
// that key, or an empty one, and parses the stored bytes on the preparation
// thread for the reads after it. Any other thread parses a changed value itself.
namespace SettingsJson {

// A stored value over maximumBytes (0: any size) reads as empty.
QJsonDocument document(const QString &key, qsizetype maximumBytes = 0,
                       const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonObject object(const QString &key, qsizetype maximumBytes = 0,
                   const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonArray array(const QString &key, const char *file = __builtin_FILE(), int line = __builtin_LINE());

// Stores document, compact, under key and records it, so no read parses it.
void store(QSettings &settings, const QString &key, const QJsonDocument &document);
void store(const QString &key, const QJsonDocument &document);
// A writer that has just stored bytes, serialized from document, under key
// records them, so the next read needs no parse.
void remember(const QString &key, const QByteArray &bytes, const QJsonDocument &document);

// Keys are exact, or have one "*" segment for each child group at that place,
// as in "tasks/newCard/*/request".
// Parses keys on the preparation thread ahead of their first read.
void warm(QObject *owner, const QStringList &keys);
// For main() only, before the tray and any window exist: parses keys on a
// worker thread and waits for it, so the startup reads find them parsed.
void preload(const QStringList &keys);

}
