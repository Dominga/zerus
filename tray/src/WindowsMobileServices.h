#pragma once
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
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
// progress is recovered at its next start). They run from a copy under
// %USERPROFILE%\.local\share\hgs\mobile\<hash>, like the session host: a
// program running from the portable folder would block its update.
namespace WindowsMobileServices {
inline QString bundled(const QString &name) { return QDir(QCoreApplication::applicationDirPath()).filePath(name); }
inline bool available() { return QFileInfo(bundled(QStringLiteral("zerus-mobile-connector.exe"))).isExecutable(); }
// The bundled program's copy for this exact build, made once.
inline QString program(const QString &name)
{
    const auto source = bundled(name);
    QFile file(source);
    QCryptographicHash hash(QCryptographicHash::Sha256);
    if (!file.open(QIODevice::ReadOnly) || !hash.addData(&file)) return source;
    const auto folder = QDir::homePath() + QStringLiteral("/.local/share/hgs/mobile/") + QString::fromLatin1(hash.result().toHex().left(16));
    const auto target = QDir(folder).filePath(name);
    if (QFileInfo::exists(target)) return target;
    QDir().mkpath(folder);
    const auto partial = target + QStringLiteral(".partial");
    QFile::remove(partial);
    if (!QFile::copy(source, partial) || !QFile::rename(partial, target)) { QFile::remove(partial); return source; }
    return target;
}
inline QString connector() { return program(QStringLiteral("zerus-mobile-connector.exe")); }
inline QString relay() { return program(QStringLiteral("zerus-relay.exe")); }
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

// The connector runs `hgs_path`; it names the stable hgs the desktop uses (the
// session host's copy), never one inside the portable folder. True when the
// configuration changed.
inline bool pinHgs(const QString &config, const QString &hgs)
{
    if (hgs.isEmpty() || !QFileInfo(hgs).isAbsolute()) return false;
    QFile file(config);
    if (!file.open(QIODevice::ReadOnly) || file.size() > 65536) return false;
    auto object = QJsonDocument::fromJson(file.readAll()).object();
    file.close();
    const auto native = QDir::toNativeSeparators(hgs);
    if (object.isEmpty() || object.value(QStringLiteral("hgs_path")).toString() == native) return false;
    object.insert(QStringLiteral("hgs_path"), native);
    QFile out(config + QStringLiteral(".tmp"));
    if (!out.open(QIODevice::WriteOnly | QIODevice::Truncate)) return false;
    out.write(QJsonDocument(object).toJson());
    out.close();
    QFile::remove(config + QStringLiteral(".previous"));
    if (!QFile::rename(config, config + QStringLiteral(".previous"))) { QFile::remove(out.fileName()); return false; }
    if (!QFile::rename(out.fileName(), config)) { QFile::rename(config + QStringLiteral(".previous"), config); return false; }
    QFile::remove(config + QStringLiteral(".previous"));
    return true;
}

// At Zerus start, off the GUI thread: the chosen relay, then a configured
// connector, each only when it is not running already. A connector whose hgs
// changed restarts.
inline void ensureInBackground(const QString &config, const QString &hgs)
{
    if (!available()) return;
    auto *thread = QThread::create([config, hgs] {
        if (runRelay() && QFileInfo(bundled(QStringLiteral("zerus-relay.exe"))).isExecutable() && !relayRunning()) { startRelay(); QThread::sleep(1); }
        if (!runConnector() || !QFileInfo::exists(config)) return;
        const bool changed = pinHgs(config, hgs);
        const auto state = connectorState(config);
        if (state == QLatin1String("running") && changed) { stopConnector(); QThread::sleep(1); startConnector(config); }
        else if (state == QLatin1String("stopped")) startConnector(config);
    });
    QObject::connect(thread, &QThread::finished, thread, &QObject::deleteLater);
    thread->start();
}
}
