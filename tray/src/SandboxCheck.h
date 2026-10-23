#pragma once
// WhiteWh fork: whether this Zerus was started inside another program's
// sandbox, such as an agent's tool (Codex, Claude): a job that does not let
// programs leave it, or a restricted access token. There the session host
// cannot outlive that program, and writes to Zerus's own folders and settings
// are refused ("access denied"), so Zerus says so at start instead of failing
// piece by piece.
#include <QString>

#ifdef Q_OS_WIN
#include <qt_windows.h>
#include <QCoreApplication>
#include <QDir>
#include <string>
#include <vector>

namespace SandboxCheck {

namespace detail {
// The process of this desktop's shell window, normally explorer.exe; 0 without one.
inline DWORD shellProcess()
{
    DWORD shell = 0;
    if (HWND window = GetShellWindow()) GetWindowThreadProcessId(window, &shell);
    return shell;
}

inline bool shellInAnyJob()
{
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, shellProcess());
    if (!process) return false;
    BOOL inJob = FALSE;
    const bool known = IsProcessInJob(process, nullptr, &inJob);
    CloseHandle(process);
    return known && inJob;
}

// Whether this process's own job also holds the shell.
inline bool shellInOwnJob()
{
    const DWORD shell = shellProcess();
    if (!shell) return false;
    constexpr DWORD capacity = 4096;
    std::vector<char> buffer(sizeof(JOBOBJECT_BASIC_PROCESS_ID_LIST) + capacity * sizeof(ULONG_PTR));
    auto *list = reinterpret_cast<JOBOBJECT_BASIC_PROCESS_ID_LIST *>(buffer.data());
    if (!QueryInformationJobObject(nullptr, JobObjectBasicProcessIdList, list, DWORD(buffer.size()), nullptr)) return false;
    for (DWORD i = 0; i < list->NumberOfProcessIdsInList; ++i)
        if (list->ProcessIdList[i] == shell) return true;
    return false;
}

// A job that forbids breakaway refuses CREATE_BREAKAWAY_FROM_JOB with
// ERROR_ACCESS_DENIED. The probe starts cmd.exe suspended and ends it at once,
// so nothing of it runs.
inline bool breakawayRefused()
{
    BOOL inJob = FALSE;
    if (!IsProcessInJob(GetCurrentProcess(), nullptr, &inJob) || !inJob) return false;
    wchar_t system[MAX_PATH];
    const UINT length = GetSystemDirectoryW(system, MAX_PATH);
    if (!length || length >= MAX_PATH) return false;
    std::wstring application = std::wstring(system, length) + L"\\cmd.exe";
    std::wstring line = L"\"" + application + L"\" /c exit";
    STARTUPINFOW startup{}; startup.cb = sizeof(startup);
    PROCESS_INFORMATION process{};
    if (CreateProcessW(application.c_str(), line.data(), nullptr, nullptr, FALSE,
                       CREATE_SUSPENDED | CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process)) {
        // Breakaway can succeed and still leave the child in a job further up,
        // which the session host refuses as well (src/host/client.rs, InJob).
        BOOL childInJob = FALSE;
        const bool known = IsProcessInJob(process.hProcess, nullptr, &childInJob);
        TerminateProcess(process.hProcess, 0);
        CloseHandle(process.hThread); CloseHandle(process.hProcess);
        if (!known || !childInJob) return false;
        // The child's job is out of reach; only a shell in no job settles it.
        return !shellInAnyJob();
    } else if (GetLastError() != ERROR_ACCESS_DENIED) return false;
    // Some desktops run the shell and everything else in such a job; the
    // session host accepts that (src/host/job.rs, desktop_job).
    return !shellInOwnJob();
}

inline bool restrictedToken()
{
    HANDLE token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token)) return false;
    bool restricted = IsTokenRestricted(token);
    DWORD container = 0, size = 0;
    if (GetTokenInformation(token, TokenIsAppContainer, &container, sizeof(container), &size) && container) restricted = true;
    CloseHandle(token);
    return restricted;
}
}

// Why sessions cannot work from this process; empty when they can.
inline QString reason()
{
    if (detail::restrictedToken())
        return QCoreApplication::translate("SandboxCheck", "Zerus runs with a restricted access token, as programs started from another program's sandbox do (for example an agent's tool).");
    if (detail::breakawayRefused())
        return QCoreApplication::translate("SandboxCheck", "Zerus runs inside a Windows job that does not let programs leave it, as programs started from another program's sandbox do (for example an agent's tool).");
    return {};
}

}
#else
namespace SandboxCheck {
inline QString reason() { return {}; }
}
#endif
