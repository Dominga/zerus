#include "Settings.h"
#include "TrayAgent.h"
#include "EventStream.h"
#include "ProcessRunner.h"
#include "SettingsJson.h"
#include "QuestionNotificationSettings.h"
#include "AutomaticApprovals.h"
#include "CacheKeepalive.h"
#include "TeamAttentionWatch.h"

#include <QCoreApplication>
#include <QDateTime>
#include <QProcess>
#include <QSettings>
#include <QStandardPaths>
#include <QJsonDocument>
#include <QDebug>

#include <memory>

#include "ChildProcess.h"
#include "ClipboardBackend.h"
#include "CommandBackend.h"
#include "SessionsWindow.h"
#include "UpdateController.h"
#include "ForkUpdater.h"
#include "SessionCommands.h"
#ifdef Q_OS_LINUX
#include "KonsoleBackend.h"
#include "SniTrayIcon.h"
#include <KWindowSystem>
#else
#include "QtTrayIcon.h"
#include "QuietTrayIcon.h"
#endif
#ifdef Q_OS_MACOS
#include "MacBackend.h"
#endif
#ifdef Q_OS_WIN
#include "SwarmWorkerSupervisor.h"
#include "WindowsTerminalBackend.h"
#endif

namespace {

// Иконка трея -- по платформе сборки, без настройки: на Linux KStatusNotifierItem (ради
// ЛКМ, см. SniTrayIcon.h), везде остальное QSystemTrayIcon. Не по cfg: выбирать тут
// нечего, у пользователя нет причины хотеть иконку без работающего клика.
std::unique_ptr<TrayIcon> makeTrayIcon()
{
#ifdef Q_OS_LINUX
    return std::make_unique<SniTrayIcon>();
#else
    // Под декоратором: не дёргать NSStatusItem без изменений и при открытом меню --
    // страховка от дедлока AppKit на macOS 26, см. QuietTrayIcon.h (там же, почему на
    // Linux его нет). Владение меню не меняется: декоратор лишь передаёт его внутрь.
    return std::make_unique<QuietTrayIcon>(std::make_unique<QtTrayIcon>());
#endif
}

// Пиры не приходят в локальном ответе (см. HgsClient.h) -- опрашиваем каждого известного
// FleetState отдельным вызовом. Общая точка для минутного тика, "Refresh" и открытия
// меню -- не дублировать цикл по peerNames() в трёх местах. Три места означают, что один
// и тот же алиас может быть запрошен из них почти одновременно (например, минутный тик
// совпал с открытием меню) -- дублирующий вызов requestPeer() для уже летящего запроса
// не создаёт второй процесс: HgsClient сам игнорирует его (m_peerInFlight), эта функция
// вызывающего кода не гейтит.
// A computer with a live event stream (EventStream.h) needs no periodic poll.
void pollAllKnownPeers(HgsClient &client, const FleetState &state)
{
    const auto *streams = EventStreams::instance();
    for (const QString &alias : state.peerNames())
        if (!streams || !streams->live(alias)) client.requestPeer(alias);
}
void pollLocal(HgsClient &client)
{
    const auto *streams = EventStreams::instance();
    if (!streams || !streams->live({})) client.requestLocal();
}

// Терминал выбирается один раз на запуск. Явный шаблон в конфиге побеждает выбор
// бэкенда: пользователь, прописавший terminal=, хочет свой терминал, а не Konsole и не
// Terminal.app -- и вместе с ним теряет смысл платформенный выбор из cfg.openMode
// ("вкладка или окно" на Linux; на маке терминальный режим остался ровно один), у
// произвольного шаблона такого выбора нет и быть не может (CommandBackend его поэтому
// даже не принимает в конструктор). TrayAgent тут не более чем плоское сопоставление
// строка -> enum: ни строчки про D-Bus или Konsole-специфику сверх выбора самого класса
// бэкенда, который уже был здесь и до этой задачи.
std::unique_ptr<TerminalBackend> makeTerminal(const AppConfig &cfg)
{
    // clipboard -- РАНЬШЕ шаблона, и это не исключение из правила выше, а другой вопрос.
    // terminal= отвечает на "каким терминалом открывать", clipboard -- "не открывать
    // терминал вовсе": они ортогональны, а не конкурируют, и явное «положи в буфер»
    // сильнее любой настройки того, чем открывать. По той же причине он и вне #ifdef:
    // буфер обмена есть на обеих платформах, в отличие от Konsole.
    if (cfg.openMode == QLatin1String("clipboard"))
        return std::make_unique<ClipboardBackend>();
    if (!cfg.terminalTemplate.isEmpty())
        return std::make_unique<CommandBackend>(cfg.terminalTemplate);
#ifdef Q_OS_LINUX
    // Without Konsole (GNOME, Ubuntu) use the desktop's default terminal; the
    // tab/window modes are Konsole-specific and do not apply to it.
    if (QStandardPaths::findExecutable(QStringLiteral("konsole")).isEmpty()) {
        const QString systemTerminal = CommandBackend::systemTerminalTemplate([](const QString &program) {
            return !QStandardPaths::findExecutable(program).isEmpty();
        });
        if (!systemTerminal.isEmpty())
            return std::make_unique<CommandBackend>(systemTerminal);
    }
    KonsoleBackend::OpenMode mode = KonsoleBackend::OpenMode::Auto;
    if (cfg.openMode == QLatin1String("tab"))
        mode = KonsoleBackend::OpenMode::Tab;
    else if (cfg.openMode == QLatin1String("window"))
        mode = KonsoleBackend::OpenMode::Window;
    return std::make_unique<KonsoleBackend>(mode);
#elif defined(Q_OS_MACOS)
    // Разбирать cfg.openMode не во что: терминальный режим на маке ровно один (Tabby
    // удалён -- см. врезку в MacBackend.h), а "clipboard" перехвачен выше. Если в
    // AppConfig::openModes(Mac) когда-нибудь снова появится второй терминал, ветка должна
    // вернуться сюда вместе с ним -- иначе новый пункт меню молча откроет Terminal.app.
    return std::make_unique<MacBackend>();
#elif defined(Q_OS_WIN)
    return std::make_unique<WindowsTerminalBackend>(WindowsTerminalBackend::modeFromString(cfg.openMode));
#else
    // Ни бэкенда, ни шаблона -- open() честно скажет, что писать в tray.conf.
    return std::make_unique<CommandBackend>(QString());
#endif
}

// Explicit launch actions must execute even when ordinary session clicks copy commands.
// Preserve both the user's terminal template and the saved open-mode preference.
std::unique_ptr<TerminalBackend> makeLaunchTerminal(AppConfig cfg)
{
    if (cfg.openMode == QLatin1String("clipboard"))
        cfg.openMode = AppConfig::defaultOpenMode(AppConfig::currentPlatform());
    return makeTerminal(cfg);
}

} // namespace

TrayAgent::TrayAgent(const AppConfig &cfg, QObject *parent)
    : QObject(parent)
    , m_cfg(cfg)
    , m_client(cfg.hgsPath)
    , m_automaticApprovals(std::make_unique<AutomaticApprovals>(cfg.hgsPath))
    , m_keepalive(std::make_unique<CacheKeepalive>(cfg.hgsPath))
    , m_readSync(&m_client)
    , m_terminal(makeTerminal(cfg))
    // Набор режимов для подменю «Open in» спрашивается У AppConfig, а не строится
    // внутри меню: второго списка режимов в проекте нет и заводиться не должно (см.
    // AppConfig::openModes -- он же этот ввод и валидирует). terminalTemplateSet меняет
    // только заголовок подменю, см. конструктор TrayMenu.
    , m_menu(AppConfig::openModes(AppConfig::currentPlatform()), cfg.openMode,
             /*terminalTemplateSet=*/!cfg.terminalTemplate.isEmpty())
    , m_tray(makeTrayIcon())
#ifdef Q_OS_WIN
    , m_notifier(makeAttentionNotifier(*m_tray))
#else
    , m_notifier(makeAttentionNotifier())
#endif
    , m_badge(makeApplicationBadge())
    , m_attention(SettingsJson::object("attention/observed"))
    , m_localTimer(this)
    , m_peerTimer(this)
{
    m_icons.setForegroundOverride(m_cfg.iconColor);
    { const auto members = Settings::value("teams/mutedMembers").toStringList(); m_state.setTeamMembers(QSet<QString>(members.cbegin(), members.cend())); }
    auto *updates=UpdateController::instance();
    connect(updates,&UpdateController::changed,this,[this]{if(!m_menu.isOpen())m_menu.rebuild(m_state);});
    connect(updates,&UpdateController::newerReleaseVerified,this,[this](const QString &token,const QString &version){m_notifier->post(token,tr("Zerus update available"),tr("Zerus %1 is available. Open Updates for installation instructions.").arg(version));});
    connect(&m_menu,&TrayMenu::updatesRequested,this,[this]{showSessions();m_sessionsWindow->showUpdates();});
    // WhiteWh/zerus: a verified fork build waits; Restart to finish update installs it.
    connect(ForkUpdater::instance(),&ForkUpdater::buildReady,this,[this](qint64 build){
        m_notifier->post(QStringLiteral("zerus-fork-update:%1").arg(build),tr("Zerus update ready"),
                         tr("Fork build %1 is downloaded. Choose Restart to finish update; drafts are kept and sessions keep running.").arg(build));});
    connect(m_notifier.get(), &AttentionNotifier::activated, this, [this](const QString &token, const QString &activationToken) {
        if(token.startsWith(QLatin1String("zerus-fork-update:"))){showSessions();return;}
        // N7: a team's attention item opens its team.
        if(token.startsWith(QLatin1String("zerus-team-attention:"))){showSessions();if(m_sessionsWindow)m_sessionsWindow->showTeamAttention(token);return;}
        if(UpdateController::isUpdateToken(token)){
            auto *updates=UpdateController::instance();if(!updates->updateAvailable()||updates->updateToken()!=token)return;
            showSessions();m_sessionsWindow->showUpdates();
#ifdef Q_OS_LINUX
            if(!activationToken.isEmpty())KWindowSystem::setCurrentXdgActivationToken(activationToken);
            KWindowSystem::activateWindow(m_sessionsWindow->windowHandle());
#endif
            return;
        }
        // The token is decoded off the GUI thread (JsonParse.h).
        ProcessRunner::post(this, [token] { return AttentionTracker::target(token); },
                            [this, token, activationToken](const QJsonObject &target) {
            if (target.isEmpty()) return;
            m_pendingAttention = token; m_pendingTarget = target;
            showSessions();
#ifdef Q_OS_LINUX
            if (!activationToken.isEmpty()) KWindowSystem::setCurrentXdgActivationToken(activationToken);
            KWindowSystem::activateWindow(m_sessionsWindow->windowHandle());
#else
            Q_UNUSED(activationToken);
#endif
            resolveAttentionClick();
            if (m_pendingAttention.isEmpty()) return;
            const auto host = target.value("host").toString();
            if (host.isEmpty()) m_client.requestLocal(); else m_client.requestPeer(host);
            QTimer::singleShot(16000, this, [this, token] { if (m_pendingAttention == token) resolveAttentionClick(true); });
        });
    });
    // N32 of #499: Zerus started in the tray still learns of a stuck team
    // permission; the window's own team poll replaces this one (showSessions).
    m_teamWatch = new TeamAttentionWatch(&m_client, this);
    connect(m_teamWatch, &TeamAttentionWatch::raised, this, [this](const QString &token, const QString &title, const QString &body) { m_notifier->post(token, title, body); });
    connect(m_teamWatch, &TeamAttentionWatch::cleared, this, [this](const QString &token) { m_notifier->withdraw(token); });
    m_teamWatch->setActive(true);
    connect(m_notifier.get(), &AttentionNotifier::failed, this, [this](const QString &token, const QString &detail) {
        qWarning().noquote() << "Zerus: notification:" << detail;
        if(UpdateController::isUpdateToken(token)||token.startsWith(QLatin1String("zerus-fork-update:"))||token.startsWith(QLatin1String("zerus-team-attention:")))return;
        m_attention.deliveryFailed(token);
        SettingsJson::store(QStringLiteral("attention/observed"), QJsonDocument(m_attention.state()));
        if (m_sessionsWindow) m_sessionsWindow->showNotificationNotice(tr("System notification was not delivered: %1").arg(detail));
    });

    // Кому после этого принадлежит меню -- зависит от иконки (на маке по-прежнему
    // TrayMenu, на Linux -- самой иконке), см. TrayIcon::setContextMenu и ~TrayMenu.
    m_tray->setContextMenu(m_menu.menu());
    m_tray->setActivationHandler([this]() { showSessions(); });
    connect(&m_menu, &TrayMenu::aboutToShow, this, &TrayAgent::onMenuAboutToShow);

    // Намерения пользователя из меню -- каждое в свой обработчик. Меню сообщает, ЧТО
    // выбрали (сессия на таком-то боксе, такая-то команда в таком-то проекте, такой-то
    // режим открытия); ЧТО с этим делать -- решается здесь и только здесь. Ни про hgs и
    // его аргументы, ни про терминал, ни про tray.conf меню не знает.
    connect(&m_menu, &TrayMenu::sessionActivated, this, &TrayAgent::activateSession);
    connect(&m_menu, &TrayMenu::savedSessionRequested, this, [this](const QString &host, const QString &name) {
        showSessions(); m_sessionsWindow->showSession(host, name);
    });
    connect(&m_menu, &TrayMenu::openModeChosen, this, &TrayAgent::setOpenMode);
    connect(&m_menu, &TrayMenu::sessionsRequested, this, &TrayAgent::showSessions);
    connect(&m_menu, &TrayMenu::refreshRequested, this, [this]() {
        pollLocal(m_client);
        pollAllKnownPeers(m_client, m_state);
    });
    connect(&m_menu, &TrayMenu::quitRequested, qApp, &QCoreApplication::quit);

    connect(&m_client, &HgsClient::localReady, this, &TrayAgent::onLocalReady);
    connect(&m_client, &HgsClient::peerReady, this, &TrayAgent::onPeerReady);
    connect(&m_client, &HgsClient::failed, this, &TrayAgent::onFailed);
    connect(m_automaticApprovals.get(), &AutomaticApprovals::changed, this, [this](const QString &host) {
        if (host.isEmpty()) m_client.requestLocal(); else m_client.requestPeer(host);
    });
    // Reads in flight come from the client, keyed by the alias each one polled:
    // a peer's answer may name that machine differently.
    connect(&m_client, &HgsClient::stateReadStarted, this, [this]() {
        if (m_sessionsWindow) m_sessionsWindow->setPollingHosts(m_client.pollingHosts());
    });
    connect(&m_client, &HgsClient::stateReadFailed, this, [this](const QString &host, const QString &detail) {
        if (host.isEmpty()) {
            m_localReadError = detail;
            if (m_sessionsWindow) m_sessionsWindow->setConnectionError(detail);
        } else {
            BoxState unavailable; unavailable.host = host; unavailable.error = detail;
            m_state.setPeer(unavailable, QDateTime::currentMSecsSinceEpoch());
            if (m_sessionsWindow) m_sessionsWindow->setFleet(m_state);
        }
        if (m_sessionsWindow) m_sessionsWindow->setPollingHosts(m_client.pollingHosts());
    });

    // Иконка должна появиться сразу, даже без единого ответа: пустой трей выглядит как
    // упавшее приложение. forCounts(0, 0, false) -- ровно то же самое состояние, что и
    // "флот пуст", отличить их нечем, и это нормально -- первый ответ (доли секунды
    // для local) их развидит.
    rebuildMenu();
    refreshIcon();
    m_tray->show();

    // Локальный тик: опрашивает только этот бокс, никакого ssh, миллисекунды. Стартует
    // сразу же, не дожидаясь ни peerPollMs, ни обнаружения пиров ниже -- та часть, что
    // отвечает "тик и первая отрисовка не должны ждать пиров" из задания.
    // One long-lived event stream per computer replaces its periodic polls
    // while live; polling continues for a computer without one.
    if (EventStreams::enabled()) {
        m_streams = new EventStreams(m_client.executable(), this);
        connect(m_streams, &EventStreams::listingReady, this, [this](const QString &host, const BoxState &box) {
            if (host.isEmpty()) onLocalReady(box); else onPeerReady(box);
        });
        m_streams->setHosts({QString()});
    }
    m_localTimer.setInterval(cfg.localPollMs);
    connect(&m_localTimer, &QTimer::timeout, this, [this] {
        pollLocal(m_client);
        // Short-lived remote permission prompts must not wait for the ordinary
        // minute-long peer cadence. The client coalesces reads already in flight.
        const int interval = AutomaticApprovalPolicy::enabled() ? qMin(m_cfg.peerPollMs, 5000) : m_cfg.peerPollMs;
        if (m_peerTimer.interval() != interval) m_peerTimer.setInterval(interval);
    });
    m_localTimer.start();
    m_client.requestLocal();

#ifndef Q_OS_WIN
    // Phone alerts wait while the owner uses any Zerus desktop. This runs with
    // the window closed; failures are logged once per distinct message.
    // Windows is no mobile gateway in Phase 1 and records no presence.
    m_presence = std::make_unique<DesktopPresence>(makePresenceSource(),
        [this](double idleSeconds, bool locked) { m_client.setPresence(idleSeconds, locked); });
    connect(&m_client, &HgsClient::presenceFinished, this, [this](const QString &error) {
        if (error == m_presenceError) return;
        m_presenceError = error;
        if (!error.isEmpty()) qWarning().noquote() << "hgs zerus: desktop presence:" << error;
    });
    m_presence->start();
#else
    // No user service manager runs the project catalog worker on Windows.
    m_swarmWorker = std::make_unique<SwarmWorkerSupervisor>(cfg.hgsPath);
    m_swarmWorker->start();
#endif

    // Пиров локальный ответ не содержит; список известен только из полного `hgs ls
    // --json`, который сам по себе стоит времени самого медленного ssh во флоте.
    // Поэтому: локальный тик и первая отрисовка -- выше, синхронно; список пиров --
    // отдельным одноразовым процессом ниже, асинхронно, и как только он вернулся,
    // заводится m_peerTimer и делается первый (немедленный) опрос каждого пира. Меню
    // до этого момента показывает пустой список пиров -- секции появляются по мере
    // разрешения, а не блокируют иконку.
    m_peerTimer.setInterval(AutomaticApprovalPolicy::enabled() ? qMin(cfg.peerPollMs, 5000) : cfg.peerPollMs);
    connect(&m_peerTimer, &QTimer::timeout, this, [this]() {
        pollAllKnownPeers(m_client, m_state);
    });

    auto *discover = new QProcess(this);
    discover->setProgram(cfg.hgsPath.isEmpty() ? QStringLiteral("hgs") : cfg.hgsPath);
    discover->setArguments({QStringLiteral("ls"), QStringLiteral("--json")});
    ChildProcess::inheritStdioOnly(*discover);

    // done — общий флаг между таймаутом, ошибкой запуска и finished(): каждый из трёх
    // путей завершения процесса отчитывается ровно один раз, тот же приём, что и
    // reported в HgsClient.cpp::runHgs (не дублировать его здесь целиком ради одного
    // разового вызова при старте).
    auto done = std::make_shared<bool>(false);

    auto *timeoutTimer = new QTimer(discover);
    timeoutTimer->setSingleShot(true);
    connect(timeoutTimer, &QTimer::timeout, discover, [discover, done]() {
        if (*done)
            return;
        *done = true;
        discover->kill();
        discover->deleteLater();
    });

    connect(discover, &QProcess::errorOccurred, discover,
            [discover, timeoutTimer, done](QProcess::ProcessError e) {
        if (e != QProcess::FailedToStart || *done)
            return;
        *done = true;
        timeoutTimer->stop();
        discover->deleteLater();
        // hgs не найден вообще -- локальный тик уже сообщит об этом через
        // HgsClient::failed() (та же ошибка постигнет и requestLocal()); пиров тут
        // просто не будет до следующего запуска трея.
    });

    connect(discover, static_cast<void (QProcess::*)(int, QProcess::ExitStatus)>(
                          &QProcess::finished), this,
            [this, discover, timeoutTimer, done](int exitCode, QProcess::ExitStatus status) {
        timeoutTimer->stop();
        if (*done) {
            discover->deleteLater();
            return;
        }
        *done = true;

        if (status == QProcess::NormalExit && exitCode == 0) {
            // The whole fleet's listing is parsed off the GUI thread (ProcessRunner.h).
            struct Fleet { QList<BoxState> boxes; QString error; };
            ProcessRunner::post(this, [bytes = discover->readAllStandardOutput()] {
                Fleet parsed; parsed.boxes = HgsClient::parseFleet(bytes, &parsed.error); return parsed;
            }, [this](const Fleet &parsed) {
            const QList<BoxState> &fleet = parsed.boxes;
            if (parsed.error.isEmpty()) {
                // The first box is this computer (onLocalReady brings its state) and
                // its "peers" are the aliases to poll; requestPeer() below brings their
                // sessions. A reachable peer's own entry carries the name it reports
                // for itself, so only a CLI without that list falls back to the hosts.
                if (!m_state.local().peersKnown) {
                    if (!fleet.isEmpty() && fleet.first().peersKnown)
                        for (const auto &alias : fleet.first().peers) m_state.registerPeer(alias);
                    else
                        for (int i = 1; i < fleet.size(); ++i) m_state.registerPeer(fleet.at(i).host);
                }
                if (!m_menu.isOpen())
                    rebuildMenu();
                m_peerTimer.start();
                pollAllKnownPeers(m_client, m_state);
            }
            });
        }
        discover->deleteLater();
    });

    // ssh ConnectTimeout=3 внутри самого hgs, но обвязка (auth, motd) и то, что
    // HGS_PEERS может быть не один хост, оставляют запас -- 20с с большим отрывом
    // покрывает единственного пира ("mac") и не более чем на порядок больше для
    // нескольких.
    timeoutTimer->start(20000);
    discover->start();
}

TrayAgent::~TrayAgent()
{
    // Родителя-виджета у окна редактора нет (см. TrayAgent.h) -- без этого delete оно
    // пережило бы трей висячим top-level QWidget'ом. Само меню удаляет ~TrayMenu.
    delete m_sessionsWindow;
}

QStringList TrayAgent::jsonSettings()
{
    QStringList keys{QStringLiteral("attention/observed"), QStringLiteral("attention/readReplies"), QStringLiteral("attention/sessionMarks"),
            QStringLiteral("attention/pendingReads"), QStringLiteral("attention/sharedReadMigrated"), QStringLiteral("attention/linuxTokens"),
            QStringLiteral("workspace/organization"), QStringLiteral("workspace/pendingLaunches"), QStringLiteral("workspace/launchAccounts"),
            QStringLiteral("workspace/cacheKeepWarm"), QStringLiteral("workspace/ironRules"), QStringLiteral("workspace/sessionLeads"),
            QStringLiteral("workspace/worktreeSnapshots"), QStringLiteral("workspace/repositoryClone/*/pending"),
            QStringLiteral("swarm/observedOrganization"), QStringLiteral("swarm/observedVersions"),
            QStringLiteral("recovery/sharedPolicy"), QStringLiteral("speech/providerOperation"),
            QStringLiteral("tasks/pendingExecutorLaunches"), QStringLiteral("tasks/newCard/*/request")};
    // Mapping identities are direct children, rather than per-child groups.
    QSettings settings; settings.beginGroup(QStringLiteral("pathMappings"));
    for (const auto &key : settings.childKeys()) keys << QStringLiteral("pathMappings/") + key;
    return keys;
}

TrayAgent::ReadState TrayAgent::storedReadState()
{
    return {SettingsJson::object("attention/readReplies"), SettingsJson::object("attention/sessionMarks"), ReadMarkStore::load(), true};
}

bool TrayAgent::readStateApplied(const ReadState &stored) const
{
    // Unchanged stored values share their parsed objects (SettingsJson), so these compare at once.
    return m_appliedReadState.valid && stored.replies == m_appliedReadState.replies
        && stored.marks == m_appliedReadState.marks && stored.pending == m_appliedReadState.pending;
}

void TrayAgent::updateLocalPoll(const BoxState &box)
{
    if (box.peersKnown && !m_peerTimer.isActive()) m_peerTimer.start();
    // A control-only computer reports no sessions; its answer only carries the
    // machine list and metrics. Every poll is a process start on Windows, so it
    // polls less often, as does an enabled one until its host can push changes.
    const int localPoll = AppConfig::localPollInterval(m_cfg.localPollMs, box.localSessions,
                                                       MachinePlatform::of(box.metrics, true));
    if (m_localTimer.interval() != localPoll) m_localTimer.setInterval(localPoll);
}

// A listing equal to the last one but for its observation fields: unread and
// attention results, read marks, peers and the menu stay as they are. What
// depends on poll times, metrics or the clock is refreshed.
void TrayAgent::listingRefreshed(const QString &host)
{
    m_automaticApprovals->setFleet(m_state);
    observeAttention(host, host.isEmpty() ? m_state.local() : *m_state.peer(host));
    if (m_sessionsWindow) {
        if (host.isEmpty()) m_sessionsWindow->setConnectionError(m_localReadError);
        m_sessionsWindow->refreshFleet(m_state);
        m_sessionsWindow->setPollingHosts(m_client.pollingHosts());
    }
    refreshIcon();
}

void TrayAgent::onLocalReady(const BoxState &box)
{
    m_localReadError = box.ok ? QString() : (box.error.isEmpty() ? tr("Could not read local sessions") : box.error);
    m_lastError.clear();
    const auto stored = storedReadState();
    if (readStateApplied(stored) && m_state.refreshListing({}, box, QDateTime::currentMSecsSinceEpoch())) {
        updateLocalPoll(box);
        listingRefreshed({});
        return;
    }
    m_appliedReadState.valid = false;
    m_state.setReadState(stored.replies, stored.marks, stored.pending);
    const auto previousPeers = m_state.peerNames();
    m_state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    m_automaticApprovals->setFleet(m_state);
    migrateReadMarks({}); publishReadMarks(); warmCaches({}, m_state.local());
    for (const auto &peer : previousPeers) if (!m_state.peerNames().contains(peer)) {
        BoxState removed; removed.host = peer; removed.ok = true; observeAttention(peer, removed);
        m_privateStores.forget(peer);
    }
    for (const auto &peer : m_state.peerNames())
        if (!previousPeers.contains(peer)) m_client.requestPeer(peer);
    if (m_streams && previousPeers != m_state.peerNames()) m_streams->setHosts(QStringList{QString()} + m_state.peerNames());
    updateLocalPoll(box);
    observeAttention({}, m_state.local());
    observePrivateStores({}, m_state.local());
    if (m_sessionsWindow) { m_sessionsWindow->setConnectionError(m_localReadError); m_sessionsWindow->setFleet(m_state); m_sessionsWindow->setPollingHosts(m_client.pollingHosts()); }
    refreshIcon();
    // Тик раз в 2с не должен перестраивать меню, пока оно открыто под курсором --
    // QMenu::clear() + repopulate на видимом меню читается как мигание/сброс хайлайта.
    // Пропущенное здесь обновление не потеряно: следующее открытие меню само вызывает
    // rebuildMenu() (см. onMenuAboutToShow), так что к тому моменту данные уже свежие.
    if (!m_menu.isOpen())
        rebuildMenu();
    m_appliedReadState = storedReadState();
}

void TrayAgent::onPeerReady(const BoxState &box)
{
    // box.host is the alias this answer was polled under (see HgsClient::requestPeer).
    if (m_sessionsWindow) m_sessionsWindow->setPollingHosts(m_client.pollingHosts());
    const auto stored = storedReadState();
    if (readStateApplied(stored) && m_state.refreshListing(box.host, box, QDateTime::currentMSecsSinceEpoch())) {
        m_lastError.clear();
        listingRefreshed(box.host);
        return;
    }
    m_appliedReadState.valid = false;
    m_state.setReadState(stored.replies, stored.marks, stored.pending);
    if (m_state.local().peersKnown && !m_state.local().peers.contains(box.host)) return;
    m_lastError.clear();
    m_state.setPeer(box, QDateTime::currentMSecsSinceEpoch());
    m_automaticApprovals->setFleet(m_state);
    migrateReadMarks(box.host); publishReadMarks(); warmCaches(box.host, *m_state.peer(box.host));
    observeAttention(box.host, *m_state.peer(box.host));
    observePrivateStores(box.host, *m_state.peer(box.host));
    if (m_sessionsWindow) m_sessionsWindow->setFleet(m_state);
    refreshIcon();
    // В отличие от локального тика (каждые 2с) ответы пиров редки -- минутный фон или
    // разовый опрос из onMenuAboutToShow при открытии меню -- поэтому здесь можно себе
    // позволить перерисовать меню безусловно: это и есть "строка заменяется, когда
    // пришёл ответ" из задания, а не расплывчатое обновление когда-нибудь потом.
    rebuildMenu();
    m_appliedReadState = storedReadState();
}

void TrayAgent::onFailed(const QString &what, const QString &detail)
{
    // failed() -- сломан сам вызов hgs, а не "пир спит" (см. HgsClient.h). Текущий
    // HgsClient не размечает сигнал источником (локальный вызов или который из пиров),
    // поэтому это единый флаг "последний вызов куда-либо не задался": гасит цифру в "?"
    // до следующего успешного ответа откуда угодно -- ровно то поведение, которое
    // описывает refreshIcon() в задании.
    m_lastError = what + QStringLiteral(": ") + detail;

    refreshIcon();
}

void TrayAgent::onMenuAboutToShow()
{
    // Флаг «меню открыто» TrayMenu выставил ДО того, как отдал сюда сигнал (см. его
    // конструктор): собственный вызов rebuildMenu() ниже ничем не гейтится, но следующий
    // локальный тик (см. onLocalReady) уже увидит isOpen()==true и не станет
    // перестраивать список под курсором, пока пользователь его читает.
    // Перестраиваем на входе: aboutToShow летит до того, как меню реально показалось,
    // так что этот вызов ещё не попадает под собственный же фильтр isOpen() и
    // гарантированно подхватывает всё, что накопилось, пока меню было закрыто.
    rebuildMenu();
    // Пиров не долбим ssh каждые две секунды (см. AppConfig::peerPollMs) -- но раз уж
    // пользователь открыл меню, ждать до минутного фонового тика незачем: спрашиваем
    // сейчас. Меню уже открыто и не блокируется -- ответ подменит нужную строку, когда
    // придёт (см. onPeerReady).
    pollAllKnownPeers(m_client, m_state);

}

void TrayAgent::refreshIcon()
{
    const qint64 now = QDateTime::currentMSecsSinceEpoch();
    const int attention = m_state.attentionSessions(now);
    m_badge->setCount(m_lastError.isEmpty() ? attention : 0);
    if (!m_lastError.isEmpty()) {
        m_tray->setIcon(m_icons.forError());
        m_tray->setToolTip(QStringLiteral("Zerus: %1").arg(m_lastError));
        return;
    }
    m_tray->setIcon(m_icons.forCounts(m_state.totalSessions(), attention, m_state.isStale(now)));
    m_tray->setToolTip(m_state.tooltip(now));
}

void TrayAgent::observePrivateStores(const QString &host, const BoxState &box)
{
    // The listing digest covers "private_stores": an unchanged listing changes none.
    const auto before = m_privateStores.known();
    for (const auto &notice : m_privateStores.observe(host, host.isEmpty() ? box.host : host, box.ok, box.privateStores))
        notify(notice.title, notice.body, QSystemTrayIcon::Warning, 20000);
    if (m_privateStores.known() != before) Settings::set(QStringLiteral("privateStores/notified"), m_privateStores.known());
}

void TrayAgent::observeAttention(const QString &host, const BoxState &box)
{
    const auto before = m_attention.state();
    m_attention.setOptionalQuestions(QuestionNotificationSettings::enabled());
    const auto changes = m_attention.observe(host, box);
    if (m_attention.state() != before)
        SettingsJson::store(QStringLiteral("attention/observed"), QJsonDocument(m_attention.state()));
    for (const auto &token : changes.cleared) m_notifier->withdraw(token);
    for (const auto &notice : changes.raised) m_notifier->post(notice.token, notice.title, notice.body);
    if (m_sessionsWindow && !m_pendingAttention.isEmpty()) {
        m_sessionsWindow->setFleet(m_state);
        resolveAttentionClick(box.ok && m_pendingTarget.value("host").toString() == host);
    }
}

// WhiteWh fork: a keepalive's reply that the host still counts is read at once,
// before attention sees it, so it neither shows unread nor notifies.
void TrayAgent::warmCaches(const QString &host, const BoxState &box)
{
    bool marked = false;
    for (const auto &reply : m_keepalive->observe(host, box, QDateTime::currentMSecsSinceEpoch() / 1000.))
        marked |= m_state.markReplyRead(reply.host, reply.name, reply.conversation, reply.reply);
    if (!marked) return;
    Settings::set("attention/readReplies", QJsonDocument(m_state.readReplies()).toJson(QJsonDocument::Compact));
    publishReadMarks();
}

void TrayAgent::migrateReadMarks(const QString &host)
{
    // Once per host: marks this GUI kept in QSettings move to the host when it
    // first reports shared read state. The old keys stay for older hosts.
    if (ReadMarkStore::migrated(host) || !m_state.supportsSharedRead(host)) return;
    ReadMarkStore::add(m_state.legacyReadMarks(host));
    ReadMarkStore::setMigrated(host);
}

void TrayAgent::publishReadMarks()
{
    ReadMarkStore::add(m_state.takeNewReadMarks());
    if (m_state.local().peersKnown) {
        // Marks for machines removed from the configuration can never be written.
        QList<ReadMark> orphaned;
        const auto peers = m_state.peerNames();
        for (const auto &value : ReadMarkStore::load()) {
            const auto mark = ReadMark::fromJson(value.toObject());
            if (!mark.host.isEmpty() && !peers.contains(mark.host)) orphaned << mark;
        }
        ReadMarkStore::remove(orphaned);
    }
    m_state.setPendingReads(ReadMarkStore::load());
    ReadMarkStore::remove(m_state.takeSettledReadMarks());
    m_readSync.setPending(ReadMarkStore::load());
}

void TrayAgent::resolveAttentionClick(bool final)
{
    if (m_pendingAttention.isEmpty() || !m_sessionsWindow) return;
    const auto target = m_pendingTarget;
    const auto host = target.value("host").toString();
    const auto *box = host.isEmpty() ? &m_state.local() : m_state.peer(host);
    if (box) for (const auto &session : box->sessions) {
        if (!AttentionTracker::matches(target, session)) continue;
        m_sessionsWindow->showAttentionSession(host, session.name);
        m_pendingAttention.clear(); return;
    }
    if (final) {
        m_pendingAttention.clear();
        m_sessionsWindow->showNotificationNotice(tr("This session is no longer available: %1").arg(target.value("name").toString()));
    }
}

void TrayAgent::rebuildMenu()
{
    m_menu.rebuild(m_state);
}

void TrayAgent::activateSession(const QString &peer, const QString &session,
                                const QString &tty)
{
    activateSessionWith(*m_terminal, peer, session, tty);
}

void TrayAgent::openSessionTerminal(const QString &peer, const QString &session, const QString &tty)
{
    if (m_cfg.openMode != QLatin1String("clipboard")) {
        activateSession(peer, session, tty);
        return;
    }
    auto terminal = makeLaunchTerminal(m_cfg);
    activateSessionWith(*terminal, peer, session, tty);
}

void TrayAgent::activateSessionWith(TerminalBackend &backend, const QString &peer,
                                  const QString &session, const QString &tty)
{
    // Строка для оболочки собирается ЗДЕСЬ, в момент клика, а не в меню в момент
    // отрисовки: меню отдаёт только "какая сессия на каком боксе", про синтаксис hgs и
    // про cfg.hgsPath оно не знает вовсе (см. TrayMenu.h).
    const QStringList arguments = SessionCommands::openArguments(peer, session);
    const QString command = SessionCommands::command(m_cfg.hgsPath, arguments);

    QString error;
    // Приаттаченная сессия: сначала пробуем показать уже открытое окно. Второй клиент
    // на тот же экран -- это не «открылось», это два курсора в одном tmux; поэтому
    // ветка подъёма идёт первой и только её провал разрешает открыть вкладку.
    if (!tty.isEmpty() && backend.canRaise() && backend.raiseTty(tty, &error))
        return;

    error.clear();
    if (backend.openProgram(m_cfg.hgsPath, arguments, &error)) {
        reportAdvisory(backend);
        return;
    }

    // Автоматически сделать больше нечего, поэтому в тексте -- ровно та команда,
    // которую трей пытался выполнить: её можно скопировать в любой терминал.
    notify(QStringLiteral("Zerus: the terminal did not open"),
           QStringLiteral("%1\n\nCommand: %2\nRun it in a terminal by hand.")
               .arg(error, command),
           QSystemTrayIcon::Warning, 15000);
}

void TrayAgent::copyText(const QString &text)
{
    ClipboardBackend clipboard;
    QString error;
    if (!clipboard.open(text, &error))
        notify(tr("Zerus: could not copy"), error, QSystemTrayIcon::Warning, 8000);
}

void TrayAgent::launchSession(const QString &host, const QString &cmd, const QString &target, const QString &name, const QString &account, const QString &launchId, const QString &permissions, const QString &localModel, const QString &expectedMachine)
{
    QStringList args;
    if (!host.isEmpty()) args << QLatin1Char('@') + host;
    if(!expectedMachine.isEmpty())args<<"--expect-machine"<<expectedMachine;
    args << cmd << target << QStringLiteral("--new") << QStringLiteral("-n") << name;
    args << QStringLiteral("--launch-id") << launchId;
    if (!account.isEmpty()) args << QStringLiteral("--account") << account;
    if (!permissions.isEmpty()) args << QStringLiteral("--permissions") << permissions;
    if (!localModel.isEmpty()) args << QStringLiteral("--local-model") << localModel;
    const QString command = SessionCommands::command(m_cfg.hgsPath, args);
    auto launchTerminal = m_cfg.openMode == QLatin1String("clipboard")
        ? makeLaunchTerminal(m_cfg) : nullptr;
    TerminalBackend &terminal = launchTerminal ? *launchTerminal : *m_terminal;
    QString error;
    if (!terminal.openProgram(m_cfg.hgsPath, args, &error)) {
        if (m_sessionsWindow) m_sessionsWindow->newSessionLaunchFailed(launchId, error);
        notify(tr("Zerus: the terminal did not open"), error + QStringLiteral("\n\n") + command,
               QSystemTrayIcon::Warning, 8000);
    }
    else reportAdvisory(terminal);
}

void TrayAgent::showSessions()
{
    if (!m_sessionsWindow) {
        // The window reads the known team items the watch stored and polls from now on.
        if (m_teamWatch) m_teamWatch->setActive(false);
        m_sessionsWindow = new SessionsWindow(m_cfg.hgsPath);
        const auto publishReadState = [this]() {
            SettingsJson::store(QStringLiteral("attention/readReplies"), QJsonDocument(m_state.readReplies()));
            publishReadMarks();
            m_sessionsWindow->setFleet(m_state); refreshIcon();
            if (!m_menu.isOpen()) rebuildMenu();
        };
        connect(m_sessionsWindow, &SessionsWindow::replyViewed, this, [this, publishReadState](const QString &host, const QString &name, const QString &conversation, const QString &reply) {
            if (m_state.markReplyRead(host, name, conversation, reply)) publishReadState();
        });
        connect(m_sessionsWindow, &SessionsWindow::repliesMarkedRead, this, [this, publishReadState](const QJsonObject &replies) {
            if (m_state.markRepliesRead(replies)) publishReadState();
        });
        connect(m_sessionsWindow, &SessionsWindow::attentionMarksChanged, this, [this, publishReadState](const QJsonObject &marks, const QJsonObject &read) {
            m_state.setAttentionMarks(marks); m_state.setReadReplies(read);
            SettingsJson::store(QStringLiteral("attention/sessionMarks"), QJsonDocument(marks));
            // Consume muted events and withdraw current banners immediately.
            // Unmute keeps the recorded edge, so it never replays an old event.
            observeAttention({}, m_state.local());
            for (const auto &host : m_state.peerNames()) if (const auto *box = m_state.peer(host)) observeAttention(host, *box);
            publishReadState();
        });
        connect(m_sessionsWindow, &SessionsWindow::notificationPreferencesChanged, this,
                [this](const QString &host, const QString &name, const QString &run, const QString &conversation, const QJsonObject &preferences) {
            const auto *box = host.isEmpty() ? &m_state.local() : m_state.peer(host); if (!box) return;
            const auto found = std::find_if(box->sessions.cbegin(), box->sessions.cend(), [&](const SessionInfo &s) {
                return s.name == name && s.runId == run && s.conversationId == conversation;
            });
            if (found == box->sessions.cend()) return;
            const auto expected = *found;
            if (!m_state.applyNotificationPreferences(host, expected, preferences)) return;
            const auto *updated = host.isEmpty() ? &m_state.local() : m_state.peer(host);
            observeAttention(host, *updated);
            m_sessionsWindow->setFleet(m_state); refreshIcon(); if (!m_menu.isOpen()) rebuildMenu();
            // One fresh read of this host, not a retry of the mutation or an
            // inspection of whichever unrelated conversation is now selected.
            if (host.isEmpty()) m_client.requestLocal(); else m_client.requestPeer(host);
        });
        connect(m_sessionsWindow, &SessionsWindow::sessionActivated, this, &TrayAgent::activateSession);
        connect(m_sessionsWindow, &SessionsWindow::terminalOpenRequested, this, &TrayAgent::openSessionTerminal);
        connect(m_sessionsWindow, &SessionsWindow::folderShellRequested, this, [this](const QString &host, const QString &directory) {
            const auto command = SessionCommands::folderShell(m_cfg.hgsPath, host, directory);
            if (command.isEmpty()) return;
            auto backend = makeLaunchTerminal(m_cfg); QString error;
            const bool opened = host.isEmpty() ? backend->openShell(directory, &error)
                : backend->openProgram(m_cfg.hgsPath, {"machine", "ssh", host, "--directory", directory}, &error);
            if (!opened) notify(tr("Could not open folder shell"), error, QSystemTrayIcon::Warning, 8000);
            else reportAdvisory(*backend);
        });
        connect(m_sessionsWindow, &SessionsWindow::copySessionCommandRequested, this,
                [this](const QString &host, const QString &name, const QString &archiveId) {
                    copyText(SessionCommands::open(m_cfg.hgsPath, host, name, archiveId));
                });
        connect(m_sessionsWindow, &SessionsWindow::copyTextRequested, this, &TrayAgent::copyText);
        connect(m_sessionsWindow, &SessionsWindow::teamAttentionRaised, this, [this](const QString &token, const QString &title, const QString &body) {
            m_notifier->post(token, title, body);
        });
        connect(m_sessionsWindow, &SessionsWindow::teamAttentionCleared, this, [this](const QString &token) { m_notifier->withdraw(token); });
        // U25 (spec 2.3): team members are muted by default; known across restarts.
        connect(m_sessionsWindow, &SessionsWindow::teamMembersChanged, this, [this](const QSet<QString> &members) {
            QStringList kept(members.cbegin(), members.cend()); kept.sort(); if (kept.size() > 4096) kept = kept.mid(0, 4096);
            Settings::set("teams/mutedMembers", kept);
            if (!m_state.setTeamMembers(members)) return;
            observeAttention({}, m_state.local());
            for (const auto &host : m_state.peerNames()) if (const auto *box = m_state.peer(host)) observeAttention(host, *box);
            m_sessionsWindow->setFleet(m_state); refreshIcon();
        });
        connect(m_sessionsWindow, &SessionsWindow::newSessionRequested, this, &TrayAgent::launchSession);
        connect(m_sessionsWindow, &SessionsWindow::accountLoginRequested, this, [this](const QString &host, const QString &id) {
            auto backend = makeLaunchTerminal(m_cfg); QString error; QStringList args;
            if (!host.isEmpty()) args << QLatin1Char('@') + host;
            args << "account" << "login" << id;
            if (!backend->openProgram(m_cfg.hgsPath, args, &error)) notify(tr("Could not open agent sign-in"), error, QSystemTrayIcon::Warning, 8000);
            else reportAdvisory(*backend);
        });
        connect(m_sessionsWindow, &SessionsWindow::machineSshRequested, this, [this](const QString &alias) {
            auto backend = makeLaunchTerminal(m_cfg); QString error;
            if (!backend->openProgram(m_cfg.hgsPath, {"machine", "ssh", alias}, &error)) notify(tr("Could not open SSH terminal"), error, QSystemTrayIcon::Warning, 8000);
            else reportAdvisory(*backend);
        });
        connect(m_sessionsWindow, &SessionsWindow::accountInstallRequested, this, [this](const QString &host, const QString &provider) {
            auto backend = makeLaunchTerminal(m_cfg); QString error; QStringList args;
            if (!host.isEmpty()) args << QLatin1Char('@') + host;
            args << "account" << "install" << provider;
            if (!backend->openProgram(m_cfg.hgsPath, args, &error))
                notify(tr("Could not open agent installer"), error, QSystemTrayIcon::Warning, 8000);
            else reportAdvisory(*backend);
        });
        connect(m_sessionsWindow, &SessionsWindow::projectsChanged, this, [this](const QString &host) {
            if (host.isEmpty()) m_client.requestLocal();
            else m_client.requestPeer(host);
        });
        connect(m_sessionsWindow, &SessionsWindow::refreshRequested, this, [this]() {
            pollLocal(m_client); pollAllKnownPeers(m_client, m_state);
        });
        // A created session being confirmed: read its computer now, even one
        // with a live event stream, which would otherwise not be polled.
        connect(m_sessionsWindow, &SessionsWindow::hostRefreshRequested, this, [this](const QString &host) {
            if (host.isEmpty()) m_client.requestLocal(); else m_client.requestPeer(host);
        });
    }
    m_sessionsWindow->setFleet(m_state);
    m_sessionsWindow->setConnectionError(m_localReadError);
    m_sessionsWindow->setPollingHosts(m_client.pollingHosts());
    m_sessionsWindow->setClipboardMode(m_cfg.openMode == QLatin1String("clipboard"));
    m_sessionsWindow->showSessionList();
    if (m_sessionsWindow->isMinimized()) m_sessionsWindow->showNormal();
    else m_sessionsWindow->show();
    m_sessionsWindow->raise(); m_sessionsWindow->activateWindow();
}

void TrayAgent::reportAdvisory(TerminalBackend &backend)
{
    const QString advisory = backend.takeAdvisory();
    // Заголовок спрашиваем у самого бэкенда: «session opened, but not the way you asked»
    // верно для отката Konsole на окно и неверно для режима «в буфер обмена», где не
    // открылось ровно ничего (см. TerminalBackend::advisoryTitle).
    if (!advisory.isEmpty())
        notify(backend.advisoryTitle(), advisory, QSystemTrayIcon::Information, 12000);
}

void TrayAgent::notify(const QString &title, const QString &body,
                       QSystemTrayIcon::MessageIcon icon, int msecs)
{
#ifdef Q_OS_MACOS
    // На маке балун трея НЕ показывается, пока приложению не разрешили уведомления, --
    // а разрешение спрашивают ровно в тот момент, когда приложение шлёт своё ПЕРВОЕ
    // уведомление. Разбор живого случая (macOS 26.6.1, ad-hoc-подпись, лог usernoted):
    //
    //   14:19:24.373  Sending request for permission for com.hgdev.hgs-tray
    //   14:19:24.534  addOrUpdate listItem: com.hgdev.hgs-tray:682EACE9 …
    //                 canDisplayWhileCenterIsClosed: false, visibility: []   <- не видно
    //   14:19:31.679  Authorization set for com.hgdev.hgs-tray to allow: YES <- +7 с
    //   14:19:31.783  … visibility: [history, alert, …]   <- поздно, баннер не покажут
    //   14:19:42.790  removeDelivered … (Qt снимает балун по своему таймауту)
    //
    // То есть сообщение доставлено, но показано не было, а через 12 секунд удалено и из
    // истории: пользователь не увидел НИЧЕГО и не мог найти потом. Это не разовая
    // неудача -- так будет на каждой новой машине и после каждой смены bundle id.
    //
    // osascript такого не знает: `display notification` идёт от Script Editor, у
    // которого разрешение есть всегда, поэтому первое же сообщение видно (проверено на
    // той же машине -- в логе `Presenting … as banner`). Цена -- имя отправителя в
    // баннере не «hgs sessions», а Script Editor. Невидимое уведомление хуже.
    //
    // startDetached, а не execute: ждать osascript на GUI-потоке нельзя (клик по
    // сессии уже открыл терминал, трей не должен подвисать).
    if (ChildProcess::startDetached(QStringLiteral("osascript"),
                                    {QStringLiteral("-e"),
                                     MacBackend::notificationScript(title, body)}))
        return;
    // osascript не запустился -- пробуем штатный балун: он может не показаться, но это
    // всё же лучше, чем промолчать.
#endif
    m_tray->showMessage(title, body, icon, msecs);
}

void TrayAgent::setOpenMode(const QString &mode)
{
    // Клик по уже отмеченному пункту тоже даёт triggered() -- ни бэкенд пересоздавать,
    // ни файл переписывать в этом случае не за чем.
    if (mode == m_cfg.openMode)
        return;
    m_cfg.openMode = mode;
    if (m_sessionsWindow) m_sessionsWindow->setClipboardMode(mode == QLatin1String("clipboard"));

    // ПЕРЕСОЗДАНИЕ бэкенда, а не сеттер на KonsoleBackend -- по трём причинам.
    // 1. makeTerminal() и так единственное место, знающее перевод строки режима в
    //    KonsoleBackend::OpenMode; сеттер завёл бы второе такое место (или потребовал
    //    dynamic_cast к KonsoleBackend прямо здесь -- а TrayAgent сознательно не знает,
    //    какой бэкенд у него внутри, см. m_terminal в .h).
    // 2. Класть setOpenMode() в сам интерфейс TerminalBackend тоже нельзя: "вкладка или
    //    окно" -- konsole-специфика, у CommandBackend такого выбора нет и не будет, и
    //    интерфейс получил бы метод, который одна из двух реализаций обязана
    //    игнорировать.
    // 3. Заодно решается вопрос с "объяснили один раз за прогон" (m_tabAdvisoryShown в
    //    KonsoleBackend): новый объект -- новый счёт. И это именно то поведение, которое
    //    нужно: пользователь, который ТОЛЬКО ЧТО осознанно выбрал "Tab", обязан
    //    снова увидеть, почему вкладки всё равно не будет, пока API Konsole выключен --
    //    молчание в ответ на явный выбор читалось бы как "сделано".
    // Смена объекта безопасна: m_terminal живёт под unique_ptr, а лямбды меню держат
    // this, а не сам бэкенд, -- висячих ссылок на старый объект не остаётся.
    m_terminal = makeTerminal(m_cfg);

    // Флаг --open-mode сильнее файла ПРИ СТАРТЕ, но выбор из меню -- более поздняя явная
    // команда пользователя, поэтому он перекрывает флаг до конца прогона (иначе пункт
    // меню молча ничего бы не делал -- худший из возможных исходов) и всё равно
    // записывается в файл: следующий запуск с тем же флагом снова возьмёт флаг.
    QString err;
    if (!AppConfig::writeConfigValue(QStringLiteral("open-mode"), mode, &err)) {
        // Режим УЖЕ применён -- об этом и сообщаем: не потерялся выбор, потерялась только
        // его запись на диск, и пережить перезапуск трея он не сможет.
        notify(tr("Zerus: mode applied, but not saved"),
               tr("New sessions already open the new way, but the choice could "
                  "not be written to %1: %2")
                   .arg(AppConfig::configPath(), err),
               QSystemTrayIcon::Warning, 10000);
    }
}
