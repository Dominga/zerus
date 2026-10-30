#include "HangTrace.h"

#include <QFileInfo>
#include <QStringList>

#ifdef Q_OS_WIN
#include <windows.h>
#endif

namespace HangTrace {
namespace {
#ifdef Q_OS_WIN
HANDLE s_main = nullptr;
#endif
}

void rememberMainThread()
{
#ifdef Q_OS_WIN
    if (!s_main)
        DuplicateHandle(GetCurrentProcess(), GetCurrentThread(), GetCurrentProcess(), &s_main,
                        THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, FALSE, 0);
#endif
}

QString mainThreadStack()
{
#if defined(Q_OS_WIN) && defined(_M_X64)
    if (!s_main || GetThreadId(s_main) == GetCurrentThreadId()) return {};
    // While the thread is suspended nothing here allocates: a thread stopped
    // inside the heap would hold its lock.
    DWORD64 frames[40];
    int count = 0;
    if (SuspendThread(s_main) == DWORD(-1)) return {};
    CONTEXT context{};
    context.ContextFlags = CONTEXT_FULL;
    if (GetThreadContext(s_main, &context)) {
        while (count < 40 && context.Rip) {
            frames[count++] = context.Rip;
            DWORD64 base = 0;
            PRUNTIME_FUNCTION function = RtlLookupFunctionEntry(context.Rip, &base, nullptr);
            if (!function) {
                // A leaf function: its return address is on top of the stack.
                context.Rip = *reinterpret_cast<const DWORD64 *>(context.Rsp);
                context.Rsp += 8;
                continue;
            }
            void *handler = nullptr;
            DWORD64 establisher = 0;
            RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, context.Rip, function, &context, &handler, &establisher, nullptr);
        }
    }
    ResumeThread(s_main);
    QStringList lines;
    for (int i = 0; i < count; ++i) {
        HMODULE module = nullptr;
        wchar_t path[MAX_PATH] = {};
        if (GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                               reinterpret_cast<LPCWSTR>(frames[i]), &module)
            && GetModuleFileNameW(module, path, MAX_PATH)) {
            lines << QStringLiteral("%1+0x%2").arg(QFileInfo(QString::fromWCharArray(path)).fileName())
                         .arg(frames[i] - reinterpret_cast<DWORD64>(module), 0, 16);
        } else {
            lines << QStringLiteral("0x%1").arg(frames[i], 0, 16);
        }
    }
    return lines.join(QLatin1Char('\n'));
#else
    return {};
#endif
}

QString report(qint64 hungMs, int boundMs, const char *activity)
{
    const QString stack = mainThreadStack();
    return QStringLiteral("The main thread did not answer for %1 s (the bound is %2 s); Zerus aborts so it starts again.\n"
                          "Main thread: %3\nStack:\n%4")
        .arg(double(hungMs) / 1000.0, 0, 'f', 1).arg(boundMs / 1000)
        .arg(activity && *activity ? QString::fromLatin1(activity) : QStringLiteral("no known activity"),
             stack.isEmpty() ? QStringLiteral("not available") : stack);
}

}
