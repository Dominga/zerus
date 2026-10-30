#pragma once

#include <QString>

// WhiteWh fork (owner, 2026-10-30): what the main thread was doing when the
// watchdog gave up on it, for zerus.log. The watchdog's abort leaves a
// minidump only where Windows Error Reporting keeps one; the log line says it
// where the person and the next session can read it.
namespace HangTrace {

// On the main thread, at start: remembers it for mainThreadStack().
void rememberMainThread();

// From another thread: the main thread's frames as "module+0xoffset", the
// innermost first, at most 40. It suspends the main thread only to read its
// registers and stack, and formats after resuming it. Windows x64 only; empty
// elsewhere or when the thread cannot be read.
QString mainThreadStack();

// The line the watchdog logs: the hang, its bound, the known activity and the
// stack.
QString report(qint64 hungMs, int boundMs, const char *activity);

}
