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
// set() and remove() on the GUI thread therefore try the lock first with
// tryLock(0). Free, the change is written at once. Busy, it joins an ordered
// queue that one background thread writes as soon as the lock is free; every
// later change joins that queue too, so a newer value is never overwritten by
// an older one. The background thread waits for a free lock before it enters
// QSettings, which holds its in-process mutex during a sync: GUI-thread reads
// never wait behind it. A queued change is not visible to QSettings reads until
// it is written. Other threads write at once unless the queue is busy.
//
// watchLock() starts the janitor: a lock file older than a second is logged to
// zerus.log with its holder (pid, application, host, age, executable). A lock
// whose process is gone is removed; one naming this live process never is,
// since another of its threads may be inside a sync.
namespace Settings {

void set(const QString &key, const QVariant &value);
void remove(const QString &key);

// For a reservation that must be stored before its action (a message identity,
// a clone's recovery): writes now and says whether it was stored. On the GUI
// thread with the lock busy or changes queued it writes nothing and returns
// false, so the caller takes its "could not save" path instead of waiting.
bool setNow(const QString &key, const QVariant &value);
bool removeNow(const QString &key);

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
