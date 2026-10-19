#include "ForkUpdater.h"
#include "ChildProcess.h"
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcess>
#include <QRegularExpression>
#include <QTimer>

namespace {
#ifdef ZERUS_FORK_FEED_URL
const char *feedUrl = ZERUS_FORK_FEED_URL;
#else
const char *feedUrl = "";
#endif
// Every build comes from this fork's own releases.
const char *releasePrefix = "https://github.com/WhiteWh/zerus/releases/";
// The folder at the top of the portable zip (scripts/windows_portable.py).
const char *top = "zerus-windows-x86_64";
constexpr qint64 feedLimit = 64 * 1024;
constexpr qint64 zipLimit = 1024LL * 1024 * 1024;
constexpr int firstCheckMs = 2 * 60 * 1000, checkIntervalMs = 60 * 60 * 1000;

// Runs detached once Zerus quits: waits until nothing runs from the installation,
// sets it aside as the single rollback copy, moves the verified build in and
// starts it. A build that exits within 15 seconds is replaced by the previous one.
const char *installScript = R"PS(param([int]$ParentPid, [string]$Install, [string]$New, [string]$Previous, [string]$Stage)
$ErrorActionPreference = 'Stop'
$log = Join-Path (Split-Path -Parent $Install) 'zerus-fork-update.log'
function Log([string]$message) { try { Add-Content -LiteralPath $log -Value ("{0:u} {1}" -f (Get-Date), $message) } catch { } }
function Running {
    $prefix = $Install.TrimEnd('\') + '\'
    @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and $_.Path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) } catch { $false }
    }).Count -gt 0
}
function WaitIdle([int]$seconds) {
    $deadline = (Get-Date).AddSeconds($seconds)
    while ((Running) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 500 }
    -not (Running)
}
$gui = Join-Path $Install 'hgs-tray.exe'
try { Wait-Process -Id $ParentPid -Timeout 120 -ErrorAction Stop } catch { }
if (-not (WaitIdle 120)) { Log 'Programs still run from the installation; the update waits for the next restart.'; Start-Process -FilePath $gui; exit 1 }
try {
    if (Test-Path -LiteralPath $Previous) { Remove-Item -LiteralPath $Previous -Recurse -Force }
    Move-Item -LiteralPath $Install -Destination $Previous
} catch { Log "Could not set the current build aside: $_"; Start-Process -FilePath $gui; exit 1 }
try { Move-Item -LiteralPath $New -Destination $Install }
catch {
    Log "Could not move the new build in: $_"
    Move-Item -LiteralPath $Previous -Destination $Install
    Start-Process -FilePath $gui; exit 1
}
$started = Start-Process -FilePath $gui -PassThru
Start-Sleep -Seconds 15
if ($started.HasExited) {
    Log "The new build exited with code $($started.ExitCode); restoring the previous build."
    [void](WaitIdle 30)
    $failed = $Install.TrimEnd('\') + '.failed'
    if (Test-Path -LiteralPath $failed) { Remove-Item -LiteralPath $failed -Recurse -Force }
    Move-Item -LiteralPath $Install -Destination $failed
    Move-Item -LiteralPath $Previous -Destination $Install
    Start-Process -FilePath $gui
    exit 1
}
Log "Updated to the build in $Install."
Remove-Item -LiteralPath $Stage -Recurse -Force -ErrorAction SilentlyContinue
)PS";
}

ForkUpdater *ForkUpdater::instance()
{
    static auto *value = new ForkUpdater(QCoreApplication::instance());
    return value;
}

qint64 ForkUpdater::installedBuild()
{
#ifdef ZERUS_FORK_BUILD
    return qint64(ZERUS_FORK_BUILD);
#else
    return 0;
#endif
}

ForkUpdater::ForkUpdater(QObject *parent) : QObject(parent)
{
    m_install = QFileInfo(QCoreApplication::applicationFilePath()).canonicalPath();
    m_stage = m_install + QStringLiteral(".update");
#ifdef Q_OS_WIN
    // Only the portable zip carries build-info.json next to hgs-tray.exe.
    m_enabled = *feedUrl && QString::fromLatin1(feedUrl).startsWith(QLatin1String(releasePrefix))
        && !m_install.isEmpty() && QFileInfo::exists(m_install + QStringLiteral("/build-info.json"));
#endif
    m_status = m_enabled ? tr("Fork updates have not been checked.") : QString();
    if (!m_enabled) return;
    m_network = new QNetworkAccessManager(this);
    QTimer::singleShot(firstCheckMs, this, &ForkUpdater::check);
    auto *periodic = new QTimer(this);
    periodic->setInterval(checkIntervalMs);
    connect(periodic, &QTimer::timeout, this, &ForkUpdater::check);
    periodic->start();
}

void ForkUpdater::finish(const QString &status)
{
    m_busy = false;
    m_status = status;
    emit changed();
}

QString ForkUpdater::unpacked() const
{
    return QDir(m_stage).filePath(QString::fromLatin1(top));
}

void ForkUpdater::check()
{
    if (!m_enabled || m_busy || ready()) return;
    m_busy = true;
    QNetworkRequest request{QUrl(QString::fromLatin1(feedUrl))};
    request.setAttribute(QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::NoLessSafeRedirectPolicy);
    request.setTransferTimeout(30000);
    auto *reply = m_network->get(request);
    connect(reply, &QNetworkReply::finished, this, [this, reply] { reply->deleteLater(); feedFinished(reply); });
}

void ForkUpdater::feedFinished(QNetworkReply *reply)
{
    if (reply->error() != QNetworkReply::NoError)
        return finish(tr("Fork update check failed: %1").arg(reply->errorString()));
    const auto data = reply->read(feedLimit + 1);
    if (data.size() > feedLimit) return finish(tr("The fork update feed is too large."));
    const auto feed = QJsonDocument::fromJson(data).object();
    m_build = feed.value("build").toInteger();
    m_commit = feed.value("commit").toString();
    m_url = QUrl(feed.value("url").toString());
    m_size = feed.value("size").toInteger();
    m_sha256 = QByteArray::fromHex(feed.value("sha256").toString().toLatin1());
    static const QRegularExpression commit(QStringLiteral("\\A[0-9a-f]{40}\\z"));
    if (feed.value("schema").toInt() != 1 || m_build <= 0 || !commit.match(m_commit).hasMatch()
        || m_url.scheme() != QLatin1String("https") || !m_url.toString().startsWith(QLatin1String(releasePrefix))
        || m_size <= 0 || m_size > zipLimit || m_sha256.size() != 32)
        return finish(tr("The fork update feed is not valid."));
    if (m_build <= installedBuild())
        return finish(tr("Fork build %1 is the newest.").arg(installedBuild()));
    download();
}

void ForkUpdater::download()
{
    QDir stage(m_stage);
    if (stage.exists() && !stage.removeRecursively())
        return finish(tr("Could not clear %1.").arg(QDir::toNativeSeparators(m_stage)));
    if (!QDir().mkpath(m_stage))
        return finish(tr("Could not create %1.").arg(QDir::toNativeSeparators(m_stage)));
    m_file = new QFile(stage.filePath(QStringLiteral("update.zip")), this);
    if (!m_file->open(QIODevice::WriteOnly)) {
        m_file->deleteLater();
        return finish(tr("Could not write the fork update: %1").arg(m_file->errorString()));
    }
    m_hash.reset();
    m_received = 0;
    m_status = tr("Downloading fork build %1…").arg(m_build);
    emit changed();
    QNetworkRequest request(m_url);
    request.setAttribute(QNetworkRequest::RedirectPolicyAttribute, QNetworkRequest::NoLessSafeRedirectPolicy);
    // An inactivity bound: the transfer itself may take longer.
    request.setTransferTimeout(120000);
    auto *reply = m_network->get(request);
    connect(reply, &QNetworkReply::readyRead, this, [this, reply] {
        const auto chunk = reply->readAll();
        m_received += chunk.size();
        if (m_received > m_size || !m_file || m_file->write(chunk) != chunk.size()) { reply->abort(); return; }
        m_hash.addData(chunk);
    });
    connect(reply, &QNetworkReply::finished, this, [this, reply] { reply->deleteLater(); downloadFinished(reply); });
}

void ForkUpdater::downloadFinished(QNetworkReply *reply)
{
    const QString zip = m_file ? m_file->fileName() : QString();
    if (m_file) { m_file->close(); m_file->deleteLater(); }
    if (reply->error() != QNetworkReply::NoError || m_received != m_size || m_hash.result() != m_sha256) {
        QDir(m_stage).removeRecursively();
        return finish(reply->error() != QNetworkReply::NoError
            ? tr("Fork build %1 did not download: %2").arg(m_build).arg(reply->errorString())
            : tr("Fork build %1 does not match its published size and SHA-256.").arg(m_build));
    }
    // Windows 10 and later ship bsdtar, which reads zip archives and refuses
    // absolute and parent paths.
    auto *process = new QProcess(this);
    process->setProgram(QDir(qEnvironmentVariable("SystemRoot", QStringLiteral("C:\\Windows"))).filePath(QStringLiteral("System32/tar.exe")));
    process->setArguments({QStringLiteral("-xf"), QDir::toNativeSeparators(zip), QStringLiteral("-C"), QDir::toNativeSeparators(m_stage)});
    process->setProcessChannelMode(QProcess::MergedChannels);
    connect(process, &QProcess::finished, this, &ForkUpdater::extracted);
    connect(process, &QProcess::errorOccurred, this, [this, process](QProcess::ProcessError error) {
        if (error == QProcess::FailedToStart) { process->deleteLater(); finish(tr("Could not unpack the fork update: %1").arg(process->errorString())); }
    });
    m_extract = process;
    ChildProcess::inheritStdioOnly(*process);
    process->start();
}

void ForkUpdater::extracted()
{
    const bool unpackedOk = m_extract && m_extract->exitStatus() == QProcess::NormalExit && m_extract->exitCode() == 0;
    const auto output = m_extract ? QString::fromLocal8Bit(m_extract->readAll()).trimmed() : QString();
    if (m_extract) m_extract->deleteLater();
    QFile::remove(QDir(m_stage).filePath(QStringLiteral("update.zip")));
    const QDir build(unpacked());
    QFile manifest(build.filePath(QStringLiteral("build-info.json")));
    const bool complete = unpackedOk && build.exists(QStringLiteral("hgs-tray.exe")) && build.exists(QStringLiteral("hgs.exe"))
        && manifest.open(QIODevice::ReadOnly);
    // The zip must be the build of the commit the feed names.
    const auto info = complete ? QJsonDocument::fromJson(manifest.read(feedLimit)).object() : QJsonObject();
    if (!complete || info.value("source_commit").toString() != m_commit) {
        QDir(m_stage).removeRecursively();
        return finish(output.isEmpty() ? tr("Fork build %1 is incomplete.").arg(m_build)
                                       : tr("Could not unpack fork build %1: %2").arg(m_build).arg(output));
    }
    m_readyBuild = m_build;
    finish(tr("Fork build %1 is ready. Restart Zerus to finish the update.").arg(m_build));
    emit buildReady(m_build);
}

bool ForkUpdater::startInstall()
{
    if (!ready() || !QFileInfo::exists(unpacked())) return false;
    const QString script = QDir::temp().filePath(QStringLiteral("zerus-fork-install-%1.ps1").arg(m_readyBuild));
    QFile file(script);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate) || file.write(installScript) < 0) return false;
    file.close();
    const auto native = [](const QString &path) { return QDir::toNativeSeparators(path); };
    QProcess process;
    process.setProgram(QDir(qEnvironmentVariable("SystemRoot", QStringLiteral("C:\\Windows"))).filePath(QStringLiteral("System32/WindowsPowerShell/v1.0/powershell.exe")));
    process.setArguments({QStringLiteral("-NoProfile"), QStringLiteral("-NonInteractive"), QStringLiteral("-ExecutionPolicy"), QStringLiteral("Bypass"),
                          QStringLiteral("-WindowStyle"), QStringLiteral("Hidden"), QStringLiteral("-File"), native(script),
                          QStringLiteral("-ParentPid"), QString::number(QCoreApplication::applicationPid()),
                          QStringLiteral("-Install"), native(m_install), QStringLiteral("-New"), native(unpacked()),
                          QStringLiteral("-Previous"), native(m_install + QStringLiteral(".previous")), QStringLiteral("-Stage"), native(m_stage)});
#ifdef Q_OS_WIN
    // CREATE_NO_WINDOW: the helper outlives Zerus without a console window.
    process.setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments *arguments) { arguments->flags |= 0x08000000; });
#endif
    ChildProcess::inheritStdioOnly(process);
    return process.startDetached();
}
