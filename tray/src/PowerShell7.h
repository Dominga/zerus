#pragma once

#include "ChildProcess.h"
#include "MachinePlatform.h"

#include <QDir>
#include <QElapsedTimer>
#include <QFileInfo>
#include <QProcess>
#include <QProcessEnvironment>
#include <QStandardPaths>
#include <QStringList>

#include <functional>

// The same policy as platform::powershell in hgs: verified Core 7+, an
// authoritative absolute HGS_PWSH override, then PATH and standard installs.
namespace PowerShell7 {
inline QString required()
{
    return QStringLiteral("PowerShell 7 or newer (Core) is required. Install PowerShell 7 or set HGS_PWSH to its absolute pwsh.exe path.");
}

inline QString select(const QString &configured, const QStringList &candidates,
                      const std::function<bool(const QString &)> &probe, QString *error)
{
    if (error) error->clear();
    const auto absolute = [](const QString &path) {
        return MachinePlatform::absolute(path, MachinePlatform::Style::Windows);
    };
    if (!configured.isEmpty()) {
        if (absolute(configured) && probe(configured)) return configured;
        if (error) *error = QStringLiteral("HGS_PWSH must name an absolute, working PowerShell 7 or newer (Core) executable. ") + required();
        return {};
    }
    for (const auto &path : candidates)
        if (absolute(path) && probe(path)) return path;
    if (error) *error = required();
    return {};
}

inline bool probe(const QString &program)
{
#ifdef Q_OS_WIN
    if (!QFileInfo(program).isFile()) return false;
    QProcess process;
    process.setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments *arguments) {
        arguments->flags |= 0x08000000; // CREATE_NO_WINDOW: version probes never open a console.
    });
    process.setProgram(program);
    process.setArguments({QStringLiteral("-NoLogo"), QStringLiteral("-NoProfile"),
        QStringLiteral("-NonInteractive"), QStringLiteral("-Command"),
        QStringLiteral("$ErrorActionPreference='Stop'; if ($PSVersionTable.PSEdition -ne 'Core' -or $PSVersionTable.PSVersion.Major -lt 7) { exit 64 }; [Console]::Out.Write('hgs-pwsh-core-7'); exit 0")});
    auto environment = QProcessEnvironment::systemEnvironment();
    environment.insert(QStringLiteral("POWERSHELL_UPDATECHECK"), QStringLiteral("Off"));
    process.setProcessEnvironment(environment);
    process.setStandardInputFile(QProcess::nullDevice());
    process.setStandardErrorFile(QProcess::nullDevice());
    ChildProcess::inheritStdioOnly(process);
    QElapsedTimer timer;
    timer.start();
    process.start();
    if (!process.waitForStarted(1000)) return false;
    QByteArray output;
    while (process.state() != QProcess::NotRunning && timer.elapsed() < 5000) {
        process.waitForFinished(25);
        output += process.readAllStandardOutput();
        if (output.size() > 128) break;
    }
    if (process.state() != QProcess::NotRunning) {
        process.kill();
        process.waitForFinished(1000);
        return false;
    }
    output += process.readAllStandardOutput();
    return process.exitStatus() == QProcess::NormalExit && process.exitCode() == 0
        && output == QByteArrayLiteral("hgs-pwsh-core-7");
#else
    Q_UNUSED(program);
    return false;
#endif
}

// Without PowerShell 7 a shell falls back to Windows PowerShell 5.1, with this
// advice, instead of failing (owner, 2026-10-28: a clean Windows has only 5.1).
inline QString upgradeAdvice()
{
    return QStringLiteral("PowerShell 7 is not installed, so Windows PowerShell 5.1 opened. "
                          "Install PowerShell 7 for full support: winget install --id Microsoft.PowerShell --source winget");
}

inline QString windowsPowerShell()
{
    const QString root = qEnvironmentVariable("SystemRoot", QStringLiteral("C:\\Windows"));
    const QString path = QDir::fromNativeSeparators(root) + QStringLiteral("/System32/WindowsPowerShell/v1.0/powershell.exe");
    return QFileInfo(path).isFile() ? path : QString();
}

// PowerShell 7, else Windows PowerShell 5.1 with *legacy set; an explicit
// HGS_PWSH stays authoritative and never falls back.
inline QString locate(QString *error = nullptr, bool *legacy = nullptr)
{
    if (legacy) *legacy = false;
    QStringList candidates;
    const QString path = QStandardPaths::findExecutable(QStringLiteral("pwsh.exe"));
    if (!path.isEmpty()) candidates << path;
    for (const auto *variable : {"ProgramW6432", "ProgramFiles"}) {
        const auto folder = qEnvironmentVariable(variable);
        if (!folder.isEmpty()) candidates << QDir(folder).filePath(QStringLiteral("PowerShell/7/pwsh.exe"));
    }
    candidates.removeDuplicates();
    const QString found = select(qEnvironmentVariable("HGS_PWSH"), candidates, probe, error);
    if (!found.isEmpty() || !qEnvironmentVariable("HGS_PWSH").isEmpty()) return found;
    const QString fallback = windowsPowerShell();
    if (fallback.isEmpty()) return {};
    if (error) error->clear();
    if (legacy) *legacy = true;
    return fallback;
}
} // namespace PowerShell7
