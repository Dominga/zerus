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
// There is no exception: check-source.py rejects any other JSON decoder in
// tray/src, and a value the GUI thread needs synchronously is prepared before
// it is read (a prepare step, ProcessRunner::post, SettingsJson.h) or kept in a
// form that needs no parse, such as QDataStream for in-process drag data.
namespace JsonParse {

// error, which may be null, receives the parse error or is cleared. object()
// and array() also report a document of the other kind as an error.
QJsonDocument parse(const QByteArray &bytes, QString *error = nullptr,
                    const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonObject object(const QByteArray &bytes, QString *error = nullptr,
                   const char *file = __builtin_FILE(), int line = __builtin_LINE());
QJsonArray array(const QByteArray &bytes, QString *error = nullptr,
                 const char *file = __builtin_FILE(), int line = __builtin_LINE());

// True on the application thread of a GUI application.
bool onGuiThread();

// A GUI-thread reader that found a value it would have had to parse first, and
// did not: counted, logged and fatal like a parse (SettingsJson.h).
void reportUnprepared(const char *file, int line);

// Other preparation that belongs off the GUI thread, such as the Sessions rows
// (SessionsWindow::prepareRows), reports a call there the same way: counted,
// logged and fatal with ZERUS_GUI_THREAD_CHECKS=fatal.
void reportGuiThreadPreparation(const char *what, const char *file = __builtin_FILE(), int line = __builtin_LINE());

// Tests: violations so far, and a handler that replaces logging and the fatal
// check (an empty handler restores them).
int violations();
void setViolationHandler(std::function<void(const char *file, int line)> handler);

}
