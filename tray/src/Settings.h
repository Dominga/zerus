#pragma once

#include <QString>
#include <QVariant>

// WhiteWh fork (owner, 2026-10-30): the one way the desktop changes its
// QSettings. A QSettings sync takes the INI's lock file (hgs-tray.ini.lock)
// with QLockFile::lock(), which waits without a bound until the holder lets go
// or the lock is 30 s old - the main thread watchdog's own bound. On Windows a
// lock file whose delete failed (a scanner had it open) still names this live
// process, so the tray waited on itself and the watchdog aborted it four times
// on 2026-10-29/30.
//
// #729 (#709 N5, L8): no thread of the desktop calls QSettings::sync with a
// change. set() and remove() queue the change for one background writer and
// return at once; value() and contains() read the queue first (an overlay), so
// a queued save reads back at once. The writer holds the INI's lock for the
// whole write: it applies the batch to a copy of the INI and renames the copy
// over it, so no other writer gets in between, and it removes a lock it left
// behind. Every QSettings read in this process then sees the new file. Only a
// format without an INI file (macOS' native preferences) is still written by
// QSettings itself, on the writer's thread.
//
// watchLock() starts the janitor: a lock file older than a second is logged to
// zerus.log with its holder (pid, application, host, age, executable). A lock
// whose process is gone is removed; one naming this live process never is,
// since another of its threads may be inside a sync.
namespace Settings {

void set(const QString &key, const QVariant &value);
void remove(const QString &key);

// What key reads now: the newest queued change of it (or of a removed group
// above it), else the stored value.
QVariant value(const QString &key, const QVariant &defaultValue = {});
bool contains(const QString &key);

// For a reservation that must be stored before its action (a message identity,
// a clone's recovery): queued like any change, then waited for, at most a
// second on the GUI thread (30 s elsewhere). One the writer has not started by
// then is withdrawn and false is returned, so the caller takes its "could not
// save" path; nothing of it is stored later.
bool setNow(const QString &key, const QVariant &value);
bool removeNow(const QString &key);

// HGS_TRAY_HOME (a test's tray): the settings are an INI file below home on
// every platform (main(), before any QSettings).
void useHome(const QString &home);

// Waits until every queued change is written (quit, tests).
void flush();
// True while changes wait for the lock.
bool queued();

// The INI's lock file.
QString lockPath();

// Where the janitor's lines go: ForkLog::write in the tray (main()).
using LogLine = void (*)(const char *level, const QString &text);
void setLog(LogLine log);

// Starts the janitor thread (main(), once).
void watchLock();
// One janitor pass, for tests: what it logged, empty when nothing.
QString checkLock(qint64 olderThanMs = 1000);

// What the main thread is doing now, for the watchdog's trace (empty: nothing
// known); set around the operations that may block.
const char *activity();
void setActivity(const char *what);

}
