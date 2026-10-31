#pragma once
#include "Settings.h"
#include "ChildProcess.h"
#include "JsonParse.h"
#include <QCheckBox>
#include <QCoreApplication>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QProcess>
#include <QProcessEnvironment>
#include <QSettings>
#include <QThread>
#include <QVBoxLayout>

// WhiteWh fork, Windows: what the fork adds around the bundled mobile gateway.
// The connector itself is managed through its own `--manage` interface
// (WindowsMobileSettings.h); Zerus may ask it to start when Zerus starts.
// A relay for this computer alone can run beside it on 127.0.0.1, from a copy
// under %USERPROFILE%\.local\share\hgs\mobile\<hash> so that the program never
// runs from the portable folder an update replaces.
namespace WindowsMobileServices {
inline QString bundled(const QString &name) { return QDir(QCoreApplication::applicationDirPath()).filePath(name); }
inline QString connector() { return bundled(QStringLiteral("zerus-mobile-connector.exe")); }
inline bool relayBundled() { return QFileInfo(bundled(QStringLiteral("zerus-relay.exe"))).isExecutable(); }
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
inline int relayPort() { return Settings::value(QStringLiteral("mobile/localRelayPort"), 8787).toInt(); }
// Off until chosen: most gateways use a relay elsewhere.
inline bool runRelay() { return Settings::value(QStringLiteral("mobile/localRelay"), false).toBool(); }
inline bool startConnectorWithZerus() { return Settings::value(QStringLiteral("mobile/runConnector"), true).toBool(); }

// The bundled relay's copy for this exact build, made once.
inline QString relay()
{
    const auto source = bundled(QStringLiteral("zerus-relay.exe"));
    QFile file(source);
    QCryptographicHash hash(QCryptographicHash::Sha256);
    if (!file.open(QIODevice::ReadOnly) || !hash.addData(&file)) return source;
    const auto folder = QDir::homePath() + QStringLiteral("/.local/share/hgs/mobile/") + QString::fromLatin1(hash.result().toHex().left(16));
    const auto target = QDir(folder).filePath(QStringLiteral("zerus-relay.exe"));
    if (QFileInfo::exists(target)) return target;
    QDir().mkpath(folder);
    const auto partial = target + QStringLiteral(".partial");
    QFile::remove(partial);
    if (!QFile::copy(source, partial) || !QFile::rename(partial, target)) { QFile::remove(partial); return source; }
    return target;
}

// A hidden program that outlives Zerus, its output appended to a log.
inline bool startHidden(const QString &program, const QStringList &arguments, const QString &log)
{
    QProcess process;
    ChildProcess::inheritStdioOnly(process);
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
inline bool relayRunning()
{
    QProcess check;
    ChildProcess::inheritStdioOnly(check);
    check.start(relay(), {QStringLiteral("healthcheck"), QStringLiteral("--port"), QString::number(relayPort())});
    if (!check.waitForFinished(5000)) { check.kill(); check.waitForFinished(1000); return false; }
    return check.exitStatus() == QProcess::NormalExit && check.exitCode() == 0;
}
inline bool startRelay()
{
    return startHidden(relay(), {QStringLiteral("--database"), relayDatabase(), QStringLiteral("serve"), QStringLiteral("--port"), QString::number(relayPort())},
                       stateDirectory() + QStringLiteral("/relay.log"));
}
inline void stopRelay()
{
    QProcess process;
    ChildProcess::inheritStdioOnly(process);
    process.start(QStringLiteral("taskkill"), {QStringLiteral("/F"), QStringLiteral("/IM"), QStringLiteral("zerus-relay.exe")});
    process.waitForFinished(-1);
}

// One `--manage` call, its JSON input on stdin, as WindowsMobileSettings sends it.
inline QJsonObject manage(const QString &action, const QString &config, const QJsonObject &input)
{
    QProcess process;
    ChildProcess::inheritStdioOnly(process);
    auto environment = QProcessEnvironment::systemEnvironment();
    for (const auto *key : {"HGS_SESSION", "HGS_RUN_ID", "HGS_EXPECTED_ID", "HGS_AGENT", "HGS_HOST_PANE", "HGS_HOST_SESSION", "TMUX", "TMUX_PANE"})
        environment.remove(QLatin1String(key));
    process.setProcessEnvironment(environment);
    process.start(connector(), {QStringLiteral("--manage"), action, QStringLiteral("--config"), config});
    if (!process.waitForStarted(5000)) return {};
    process.write(QJsonDocument(input).toJson(QJsonDocument::Compact));
    process.closeWriteChannel();
    if (!process.waitForFinished(12000)) { process.kill(); process.waitForFinished(1000); return {}; }
    if (process.exitStatus() != QProcess::NormalExit || process.exitCode() != 0) return {};
    return JsonParse::object(process.readAllStandardOutput());
}

// At Zerus start, off the GUI thread: the chosen relay, then a configured
// connector that is not running, started through its own management interface.
inline void ensureInBackground(const QString &config)
{
    if (!QFileInfo(connector()).isExecutable()) return;
    auto *thread = QThread::create([config] {
        if (runRelay() && relayBundled() && !relayRunning()) { startRelay(); QThread::sleep(1); }
        if (!startConnectorWithZerus() || !QFileInfo::exists(config)) return;
        const auto status = manage(QStringLiteral("status"), config, {{QStringLiteral("revision"), QString()}});
        if (!status.value(QStringLiteral("configured")).toBool() || status.value(QStringLiteral("running")).toBool()) return;
        manage(QStringLiteral("start"), config, {{QStringLiteral("revision"), status.value(QStringLiteral("revision")).toString()}});
    });
    QObject::connect(thread, &QThread::finished, thread, &QObject::deleteLater);
    thread->start();
}

// WhiteWh fork: a session host that stays for other computers. A Windows SSH
// peer (src/windows_peer.rs) never starts the host itself, and a host exits a
// few seconds after its last session, so this computer keeps one running with
// `hgs session-host start` while the option is on.
inline bool residentHost() { return Settings::value(QStringLiteral("machines/residentHost"), false).toBool(); }
inline void keepResidentHost(const QString &hgs)
{
    if (!residentHost() || hgs.isEmpty()) return;
    QProcess process;
    ChildProcess::inheritStdioOnly(process);
    process.setProgram(hgs);
    process.setArguments({QStringLiteral("session-host"), QStringLiteral("start"), QStringLiteral("--json")});
    process.setStandardInputFile(QProcess::nullDevice());
    process.setStandardOutputFile(QProcess::nullDevice());
    process.setStandardErrorFile(QProcess::nullDevice());
#ifdef Q_OS_WIN
    process.setCreateProcessArgumentsModifier([](QProcess::CreateProcessArguments *arguments) {
        arguments->flags |= 0x08000000; // CREATE_NO_WINDOW
    });
#endif
    process.startDetached();
}

// The fork's options under the Windows Mobile connection panel.
inline QWidget *optionsPanel()
{
    auto *panel = new QWidget;
    panel->setObjectName("windowsMobileForkOptions");
    auto *layout = new QVBoxLayout(panel);
    layout->setContentsMargins(0, 0, 12, 0);
    layout->setSpacing(8);
    auto *autostart = new QCheckBox(QCoreApplication::translate("WindowsMobileServices", "Start the connector when Zerus starts"));
    autostart->setObjectName("windowsConnectorAutostart");
    autostart->setChecked(startConnectorWithZerus());
    layout->addWidget(autostart);
    QObject::connect(autostart, &QCheckBox::toggled, panel, [](bool on) { Settings::set(QStringLiteral("mobile/runConnector"), on); });
    if (relayBundled()) {
        auto *local = new QCheckBox(QCoreApplication::translate("WindowsMobileServices", "Run a relay on this computer (127.0.0.1:%1)").arg(relayPort()));
        local->setObjectName("windowsLocalRelay");
        local->setChecked(runRelay());
        layout->addWidget(local);
        auto *hint = new QLabel(QCoreApplication::translate("WindowsMobileServices",
            "A phone reaches it only over HTTPS, for example with Tailscale: tailscale serve --bg --https=443 http://127.0.0.1:%1. Its database is %2.")
            .arg(relayPort()).arg(relayDatabase()));
        hint->setWordWrap(true);
        hint->setTextInteractionFlags(Qt::TextSelectableByMouse);
        layout->addWidget(hint);
        QObject::connect(local, &QCheckBox::toggled, panel, [](bool on) {
            Settings::set(QStringLiteral("mobile/localRelay"), on);
            if (on && !relayRunning()) startRelay();
            if (!on) stopRelay();
        });
    }
    return panel;
}
}
