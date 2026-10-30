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
//
// WhiteWh fork (owner, 2026-10-30): the values too big for the INI (the worktree
// snapshots, the organization and swarm's observed copies, 250 KB of a 275 KB
// file) live in JSON files of their own beside it (filePath()). Each is read
// once - at start by preload(), or by its first read - and is the truth from
// then on; a store() writes it with QSaveFile on a worker thread. #729 (#709
// N7): for at least one release each also stays in the INI, with a stamp (the
// time and hash of the bytes this build stored), so the updater's rollback
// build keeps its projects and folders; on the first read the newer copy wins
// and is written to the other. Every other key is written through
// Settings::set() and read through Settings::value(), so no GUI-thread store
// waits for the INI's lock and a queued store reads back at once (Settings.h).
namespace SettingsJson {

// A stored value over maximumBytes (0: any size) reads as empty.
QJsonDocument document(const QString &key, qsizetype maximumBytes = 0,
                       const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonObject object(const QString &key, qsizetype maximumBytes = 0,
                   const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonArray array(const QString &key, const char *file = __builtin_FILE(), int line = __builtin_LINE());

// Stores document, compact, under key and records it, so no read parses it.
// The QSettings overload is kept for its callers; it writes through Settings.
void store(QSettings &settings, const QString &key, const QJsonDocument &document);
void store(const QString &key, const QJsonDocument &document);
// Stores bytes already serialized from document.
void store(const QString &key, const QByteArray &bytes, const QJsonDocument &document);
// For a reservation (Settings::setNow): stores now, or nothing and false.
bool storeNow(const QString &key, const QJsonDocument &document);
// Removes key's value, from its file or the INI.
void remove(const QString &key);

// Whether key lives in a file of its own, and that file.
bool inFile(const QString &key);
QString filePath(const QString &key);
// The stored bytes, unparsed, and whether there are any.
QByteArray bytes(const QString &key);
bool contains(const QString &key);
// Waits until the files' writer has written every store (quit, tests).
void flush();
// How often key was read since the start, for tests that bound a hot path's
// reads (#705).
quint64 readCount(const QString &key);
// Drops the recorded value, so the next read reads the stored one (tests).
void forget(const QString &key);
// Removes every file key's file and recorded value, as QSettings::clear()
// does for the INI (tests).
void reset();
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
