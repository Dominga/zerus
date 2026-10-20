#pragma once
#include <QCoreApplication>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QProcess>
#include <QSettings>
#include <QThread>

// WhiteWh fork: the mobile gateway on Windows. The connector and, optionally,
// a relay for this computer alone ship next to hgs-tray.exe and run as hidden
// background programs that Zerus starts: Windows has no user service manager
// like systemd or launchd. They outlive Zerus; native agents never run inside
// them, so stopping the connector stops only the connector (a request in
// progress is recovered at its next start).
namespace WindowsMobileServices {
inline QString program(const QString &name) { return QDir(QCoreApplication::applicationDirPath()).filePath(name); }
inline QString connector() { return program(QStringLiteral("zerus-mobile-connector.exe")); }
inline QString relay() { return program(QStringLiteral("zerus-relay.exe")); }
inline QString hgs() { return program(QStringLiteral("hgs.exe")); }
inline bool available() { return QFileInfo(connector()).isExecutable(); }
inline QString stateDirectory()
{
    const auto path = QDir::homePath() + QStringLiteral("/.local/state/hgs");
    QDir().mkpath(path);
    return path;
}
inline QString relayDatabase()
{
    QDir().mkpath(stateDirectory() + QStringLiteral("/relay"));
    return QDir::toNativeSeparators(stateDirectory() + QStringLiteral("/relay/relay.sqlite3"));
}
inline int relayPort() { return QSettings().value(QStringLiteral("mobile/localRelayPort"), 8787).toInt(); }
// Off until chosen: most gateways use a relay elsewhere.
inline bool runRelay() { return QSettings().value(QStringLiteral("mobile/localRelay"), false).toBool(); }
inline bool runConnector() { return QSettings().value(QStringLiteral("mobile/runConnector"), true).toBool(); }

// A hidden program that outlives Zerus, its output appended to a log.
inline bool startHidden(const QString &program, const QStringList &arguments, const QString &log)
{
    QProcess process;
    process.setProgram(program);
    process.setArguments(arguments);
    process.setWorkingDirectory(QDir::homePath());
    process.setStandardOutputFile(log, QIODevice::Append);
    process.setStandardErrorFile(log, QIODevice::Append);
    process.setStandardInputFile(QProcess::nullDevice());
#ifdef Q_OS_WIN
    process.setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments *arguments) {
        arguments->flags |= 0x08000000; // CREATE_NO_WINDOW
    });
#endif
    return process.startDetached();
}

// `--check --json` reads the journal lock without connecting: "held" means a
// connector runs for this configuration.
inline QString connectorState(const QString &config)
{
    QProcess check;
    check.start(connector(), {QStringLiteral("--check"), QStringLiteral("--json"), QStringLiteral("--config"), config});
    if (!check.waitForFinished(5000)) { check.kill(); check.waitForFinished(1000); return QStringLiteral("unknown"); }
    const auto report = QJsonDocument::fromJson(check.readAllStandardOutput()).object();
    if (report.isEmpty()) return QStringLiteral("unknown");
    if (!report.value(QStringLiteral("configuration")).toObject().value(QStringLiteral("valid")).toBool()) return QStringLiteral("invalid");
    const auto lock = report.value(QStringLiteral("state")).toObject().value(QStringLiteral("lock")).toString();
    return lock == QLatin1String("held") ? QStringLiteral("running") : QStringLiteral("stopped");
}
inline bool relayRunning()
{
    QProcess check;
    check.start(relay(), {QStringLiteral("healthcheck"), QStringLiteral("--port"), QString::number(relayPort())});
    if (!check.waitForFinished(5000)) { check.kill(); check.waitForFinished(1000); return false; }
    return check.exitStatus() == QProcess::NormalExit && check.exitCode() == 0;
}
inline bool startConnector(const QString &config)
{
    return startHidden(connector(), {QStringLiteral("--config"), config}, stateDirectory() + QStringLiteral("/mobile-connector.log"));
}
inline bool startRelay()
{
    return startHidden(relay(), {QStringLiteral("--database"), relayDatabase(), QStringLiteral("serve"), QStringLiteral("--port"), QString::number(relayPort())},
                       stateDirectory() + QStringLiteral("/relay.log"));
}
inline void stopConnector()
{
    QProcess::execute(QStringLiteral("taskkill"), {QStringLiteral("/F"), QStringLiteral("/IM"), QStringLiteral("zerus-mobile-connector.exe")});
}
inline void stopRelay()
{
    QProcess::execute(QStringLiteral("taskkill"), {QStringLiteral("/F"), QStringLiteral("/IM"), QStringLiteral("zerus-relay.exe")});
}

// At Zerus start, off the GUI thread: the chosen relay, then a configured
// connector, each only when it is not running already.
inline void ensureInBackground(const QString &config)
{
    if (!available()) return;
    auto *thread = QThread::create([config] {
        if (runRelay() && QFileInfo(relay()).isExecutable() && !relayRunning()) { startRelay(); QThread::sleep(1); }
        if (runConnector() && QFileInfo::exists(config) && connectorState(config) == QLatin1String("stopped")) startConnector(config);
    });
    QObject::connect(thread, &QThread::finished, thread, &QObject::deleteLater);
    thread->start();
}
}
