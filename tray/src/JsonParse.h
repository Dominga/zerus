#pragma once

#include <QByteArray>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QString>

#include <functional>

// The tray's only JSON parser: scripts/ci/check-source.py rejects
// QJsonDocument::fromJson elsewhere in tray/src. Data from hgs, the event
// stream and files is parsed off the GUI thread, in a ProcessRunner prepare
// step or on another worker, and the GUI thread applies the finished value.
//
// A parse() on the application thread of a GUI application is a violation.
// It is counted and logged once per call site; with
// ZERUS_GUI_THREAD_CHECKS=fatal, which every Qt suite sets, it aborts with that
// call site. A command-line or test program without a GUI has no GUI thread.
//
// local() is the one exception, for small values the tray wrote itself
// (preferences, drag data) that a widget needs synchronously. It is limited to
// LocalLimit bytes, and check-source.py lists the files allowed to call it and
// how often; that list only shrinks.
namespace JsonParse {

constexpr qsizetype LocalLimit = 64 * 1024;

// error, which may be null, receives the parse error or is cleared. object()
// and array() also report a document of the other kind as an error.
QJsonDocument parse(const QByteArray &bytes, QString *error = nullptr,
                    const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonObject object(const QByteArray &bytes, QString *error = nullptr,
                   const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonArray array(const QByteArray &bytes, QString *error = nullptr,
                 const char *file = __builtin_FILE(), int line = __builtin_LINE());

QJsonDocument local(const QByteArray &bytes, QString *error = nullptr,
                    const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonObject localObject(const QByteArray &bytes, const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonArray localArray(const QByteArray &bytes, const char *file = __builtin_FILE(), int line = __builtin_LINE());

// True on the application thread of a GUI application.
bool onGuiThread();

// Tests: violations so far, and a handler that replaces logging and the fatal
// check (an empty handler restores them).
int violations();
void setViolationHandler(std::function<void(const char *file, int line)> handler);

}
