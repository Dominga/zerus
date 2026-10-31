// U62 (#712): New sessions: folders, launches and their outcomes, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void localWindowsLaunchJoinsTheFolderInAnyCase();
    void launchLogDistinguishesASecondLaunch();
    void nativeLaunchStaysInWorkspace_data();
    void nativeLaunchStaysInWorkspace();
    void newSessionPersistsImplicitInboxIntent_data();
    void newSessionPersistsImplicitInboxIntent();
    void pendingImplicitInboxDoesNotOverrideNativeProject_data();
    void pendingImplicitInboxDoesNotOverrideNativeProject();
    void newSessionGroupFollowsLaunchIdentity_data();
    void newSessionGroupFollowsLaunchIdentity();
    void launchPlaceholderReportsASessionThatEndedUnlisted_data();
    void launchPlaceholderReportsASessionThatEndedUnlisted();
    void endedLaunchOutranksALaterStartedCallback_data();
    void endedLaunchOutranksALaterStartedCallback();
    void endedLaunchOutranksALaterFailure_data();
    void endedLaunchOutranksALaterFailure();
    void endedLaunchesInOneListingKeepUnshownFailures();
    void endedLaunchesWithoutAResultStayBounded();
    void folderBrowserIgnoresStaleResponses();
    void aConfirmingCreationReadsItsComputerUntilListed();
};

void TestSessionsWindow::localWindowsLaunchJoinsTheFolderInAnyCase()
{
    // A launch into a local Windows folder joins the project folder it names in any
    // case and with either separator (D15, D16); a new one is stored in Qt form.
    SessionOrganization projects;const auto project=projects.createGroup("Windows work");
    const auto existing=projects.addFolder(project,"arch","C:\\Work\\Repo","Repository",MachinePlatform::Style::Windows);
    projects.markImported("arch");projects.markImported("mac");
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    const QStringList tags{"same","new","rooted"};QStringList tokens;QJsonObject pending;
    const QList<QPair<QString,QString>> targets{{"c:\\work\\repo\\worktree","c:\\WORK\\repo"},{"D:\\New\\Folder","D:\\New\\Folder\\"},{"/repo","/repo"}};
    for(int i=0;i<tags.size();++i){
        tokens<<QUuid::createUuid().toString(QUuid::WithoutBraces);
        pending.insert(tokens[i],QJsonObject{{"host",""},{"cmd","codex"},{"tag",tags[i]},{"group",project},{"target",targets[i].first},
            {"add_folder",true},{"folder",targets[i].second},{"show_pending",true},{"created",QDateTime::currentSecsSinceEpoch()}});
    }
    setJsonSetting("workspace/pendingLaunches",QJsonDocument(pending).toJson(QJsonDocument::Compact));
    auto state=fleet();auto local=state.local();local.metrics["platform"]="windows";
    for(int i=0;i<tags.size();++i){
        SessionInfo created;created.name="codex/work/"+tags[i];created.cmd="codex";created.project="work";created.tag=tags[i];
        created.runId="run-"+tags[i];created.state="running";created.launchId=tokens[i];local.sessions.append(created);
    }
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.show();window.setFleet(state);
    const SessionOrganization saved(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QStringList paths;for(const auto &folder:saved.group(project)->folders)paths<<folder.path;
    // A drive-less rooted path is no folder of a Windows machine: the session still joins.
    QCOMPARE(paths,QStringList({"C:/Work/Repo","D:/New/Folder"}));QCOMPARE(saved.group(project)->folders[0].id,existing);
    for(const auto &tag:tags)QCOMPARE(saved.groupFor("arch\ncodex/work/"+tag),project);
    QVERIFY(QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().isEmpty());
}

// U3: the launch log separates the launches of one tray from the windows it
// opens. A launch that finds the tray already running says so and stops; the
// process that takes the single-instance lock says which lock it holds; and
// only the tray that really starts writes "started". Two real launches of the
// built program run here with a temporary home, lock and socket, and offscreen
// so no desktop or tray is involved.
void TestSessionsWindow::launchLogDistinguishesASecondLaunch()
{
#ifdef Q_OS_WIN
    QTemporaryDir home, locks;
    QVERIFY(home.isValid() && locks.isValid());
    const QString app = QCoreApplication::applicationDirPath() + QStringLiteral("/../hgs-tray.exe");
    if (!QFileInfo::exists(app)) QSKIP("hgs-tray.exe is not beside the tests; run the GUI build first");
    const QString logPath = home.filePath(QStringLiteral(".local/state/hgs/zerus.log"));
    const auto readLog = [&logPath] {
        QFile file(logPath);
        return file.open(QIODevice::ReadOnly) ? QString::fromUtf8(file.readAll()) : QString();
    };
    const auto launch = [&](const QString &lockDir) {
        QProcess process;
        QProcessEnvironment env = QProcessEnvironment::systemEnvironment();
        env.insert(QStringLiteral("HGS_TRAY_HOME"), home.path());   // log and single-instance socket
        env.insert(QStringLiteral("HGS_TRAY_LOCK_DIR"), lockDir);
        env.insert(QStringLiteral("QT_QPA_PLATFORM"), QStringLiteral("offscreen"));
        process.setProcessEnvironment(env);
        process.start(app, {});
        if (!process.waitForStarted(20000)) return -1;
        if (!process.waitForFinished(60000)) { process.kill(); process.waitForFinished(5000); return -2; }
        return process.exitCode();
    };
    // The lock is held by this test, as the running tray holds it: the launch
    // must not become a second window.
    QLockFile held(locks.filePath(QStringLiteral("hgs-tray.lock")));
    QVERIFY(held.tryLock(0));
    QCOMPARE(launch(locks.path()), 0);
    auto log = readLog();
    QVERIFY2(log.contains(QStringLiteral("Zerus launched")), qPrintable(log));
    QVERIFY2(log.contains(QStringLiteral("Already running")), qPrintable(log));
    QVERIFY2(!log.contains(QStringLiteral("Single instance")), qPrintable(log));
    QVERIFY2(!log.contains(QStringLiteral("started")), qPrintable(log));
    held.unlock();
    // Nothing holds the lock now: this launch owns the instance. With no system
    // tray in the test environment it stops before it becomes the window, so it
    // reports the lock but must not report "started" either.
    const int before = readLog().size();
    QCOMPARE(launch(locks.path()), 4);
    log = readLog().mid(before);
    QVERIFY2(log.contains(QStringLiteral("Single instance")), qPrintable(log));
    QVERIFY2(!log.contains(QStringLiteral("Already running")), qPrintable(log));
    QVERIFY2(!log.contains(QStringLiteral("started")), qPrintable(log));
#endif
}

void TestSessionsWindow::nativeLaunchStaysInWorkspace_data()
{
    QTest::addColumn<QString>("agent");
    QTest::newRow("native")<<QString("dsh");QTest::newRow("codex-detached")<<QString("codex");
}

void TestSessionsWindow::nativeLaunchStaysInWorkspace()
{
    QFETCH(QString,agent);
    SessionOrganization projects; projects.addFolder("ungrouped","arch","/workspace/infra");
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    // This computer reports Linux: without a platform a Windows build takes it for
    // Windows, which offers no DeepSeek (spec D16).
    auto state=fleet();auto local=state.local();local.metrics=QJsonObject{{"platform","linux"}};state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.show();
    constexpr int noticeMs=250;window.setNoticeDurationForTests(noticeMs);
    QSignalSpy terminal(&window,&SessionsWindow::newSessionRequested);
    auto *client=window.findChild<HgsClient *>();QSignalSpy launched(client,&HgsClient::nativeSessionLaunched);
    QTimer::singleShot(0,&window,[&] {
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        QTimer::singleShot(20000,dialog,&QDialog::reject);
        QTRY_VERIFY_WITH_TIMEOUT(dialog->findChild<QPushButton *>("primary")->isEnabled(),15000);
        dialog->findChild<QLineEdit *>("launchName")->setText("native-created");
        dialog->findChild<QComboBox *>("launchProject")->setCurrentIndex(0);
        dialog->findChild<QPushButton *>("primary")->click();
    });
    window.showNewSession(agent,"infra",{});
    QTRY_COMPARE(launched.size(),1);QCOMPARE(terminal.size(),0);QVERIFY(launched[0][1].toBool());
    const QString token=launched[0][0].toString();QVERIFY(!token.isEmpty());
    auto box=state.local();SessionInfo created;created.name=agent+"/infra/native-created";created.cmd=agent;created.project="infra";created.tag="native-created";
    created.launchId=token;created.runtimeState="live";created.processState="running";created.tracked=true;created.resumable=true;
    box.sessions.append(created);state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QCOMPARE(window.findChild<SessionList *>("sessionList")->currentItem()->data(SessionRoles::Identity).toString(),"arch\n"+created.name);
    QTest::qWait(20);
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto bounds = [&] { return QRect(list->mapTo(&window, QPoint()), list->size()); };
    const auto originalBounds = bounds();
    auto *statusBar = window.findChild<QFrame *>("workspaceStatus"); QVERIFY(statusBar); QCOMPARE(statusBar->height(), 28);
    // Only a Zerus that its login item started at background priority offers the fix.
    auto *loginItem = statusBar->findChild<QPushButton *>("fixLoginItem"); QVERIFY(loginItem); QVERIFY(loginItem->isHidden());
    window.showNotificationNotice(QString(2000, 'x') + "\nRefreshing…"); QCoreApplication::processEvents();
    QCOMPARE(bounds(), originalBounds);
    // A native failure must stay visible after the previous success notice expires.
    client->nativeSessionLaunched("failed-launch",false,"Cannot find official dsh");
    QLabel *notice=nullptr;
    for(auto *label:window.findChildren<QLabel *>()) if(label->text().contains("Cannot find official dsh")) notice=label;
    QVERIFY(notice);QVERIFY(notice->isVisible());QTest::qWait(3*noticeMs);QVERIFY(notice->isVisible());
    QCOMPARE(bounds(), originalBounds);
    window.showNotificationNotice("Session updated.");
    QTRY_VERIFY(notice->text().isEmpty());
    QVERIFY(statusBar->isVisible()); QCOMPARE(bounds(), originalBounds);
}

void TestSessionsWindow::newSessionPersistsImplicitInboxIntent_data()
{
    QTest::addColumn<bool>("explicitChoice");
    QTest::newRow("implicit") << false;
    QTest::newRow("explicit-inbox") << true;
}

void TestSessionsWindow::newSessionPersistsImplicitInboxIntent()
{
    QFETCH(bool, explicitChoice);
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    QSignalSpy native(&window,&SessionsWindow::newSessionRequested);
    bool submitted=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());if(!dialog)return;
        const auto close=qScopeGuard([dialog]{dialog->reject();});
        dialog->setProjectExplicit(explicitChoice);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProject")->currentData().toString(),QString("ungrouped"));
        QCOMPARE(dialog->implicitInbox(),!explicitChoice);
        // Exercise the real synchronous launch signal/intent writer without
        // starting a provider, shell, or fake native process.
        dialog->launchRequested({},"codex","/workspace/linked","new-inference","native-codex","ungrouped",true,{}, {}, {}, {});
        submitted=true;
    });
    window.findChild<QPushButton *>("newSession")->click();QVERIFY(submitted);QCOMPARE(native.size(),1);
    const auto pending=QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object();
    QCOMPARE(pending.size(),1);
    const auto intent=pending.begin().value().toObject();
    QCOMPARE(intent.value("implicit_inbox"),QJsonValue(!explicitChoice));
    QCOMPARE(intent.value("group").toString(),QString("ungrouped"));
}

void TestSessionsWindow::pendingImplicitInboxDoesNotOverrideNativeProject_data()
{
    QTest::addColumn<QString>("intent");QTest::addColumn<bool>("nativeFirst");
    QTest::newRow("implicit-native-first") << QString("implicit") << true;
    QTest::newRow("implicit-fleet-first") << QString("implicit") << false;
    QTest::newRow("explicit-inbox") << QString("explicit") << true;
    QTest::newRow("legacy-inbox") << QString("legacy") << true;
    QTest::newRow("invalid-flag-is-legacy") << QString("invalid") << true;
    QTest::newRow("concrete-even-with-implicit-flag") << QString("concrete") << true;
}

void TestSessionsWindow::pendingImplicitInboxDoesNotOverrideNativeProject()
{
    QFETCH(QString,intent);QFETCH(bool,nativeFirst);
    SessionOrganization initial;const auto work=initial.createGroup("Native work"),chosen=initial.createGroup("Chosen work");
    initial.markImported("arch");initial.markImported("mac");
    const QString name="codex/work/new-inference",identity="arch\n"+name;
    auto shared=initial;shared.moveSession(shared.observe("arch",name,"new-run"),work);
    setJsonSetting("workspace/organization",QJsonDocument((nativeFirst?shared:initial).toJson()).toJson(QJsonDocument::Compact));
    const auto token=QUuid::createUuid().toString(QUuid::WithoutBraces);
    QJsonObject launch{{"host",QString()},{"cmd","codex"},{"tag","new-inference"},
        {"group",intent=="concrete"?chosen:QString("ungrouped")},{"target","/workspace/linked"},
        {"add_folder",true},{"folder","/workspace/linked"},{"show_pending",true},{"created",QDateTime::currentSecsSinceEpoch()}};
    if(intent!="legacy")launch["implicit_inbox"]=intent=="invalid"?QJsonValue(QStringLiteral("true")):QJsonValue(intent=="implicit"||intent=="concrete");
    setJsonSetting("workspace/pendingLaunches",QJsonDocument(QJsonObject{{token,launch}}).toJson(QJsonDocument::Compact));
    auto state=fleet();auto local=state.local();SessionInfo created;created.name=name;created.cmd="codex";created.project="work";
    created.tag="new-inference";created.state="running";created.runId="new-run";created.launchId=token;local.sessions.append(created);
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.show();
    const auto saved=[] {return QJsonDocument::fromJson(jsonSetting("workspace/organization")).object();};
    const bool passive=intent=="implicit";
    const auto expected=passive?(nativeFirst?work:QString("ungrouped")):intent=="concrete"?chosen:QString("ungrouped");
    QCOMPARE(SessionOrganization(saved()).groupFor(identity),expected);
    QVERIFY(QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().isEmpty());
    if(passive){
        QVERIFY(SessionOrganization(saved()).group("ungrouped")->folders.isEmpty());
        QCOMPARE(saved().value("unassigned_sessions").toArray().contains(identity),!nativeFirst);
        // The fleet-first path remains a marked fallback, which the existing
        // controller serialization control proves is absent from shared writes.
        // Then exercise the real organizationReady -> rebuild path for the
        // native assignment arriving later; the launch was already consumed.
        auto *controller=window.findChild<SwarmController *>();QVERIFY(controller);
        controller->organizationReady(SwarmController::presentation(shared.toJson(),saved()));
        QCOMPARE(SessionOrganization(saved()).groupFor(identity),work);
        QVERIFY(!saved().value("unassigned_sessions").toArray().contains(identity));
    }else{
        QVERIFY(!saved().value("unassigned_sessions").toArray().contains(identity));
        QCOMPARE(SessionOrganization(saved()).group(expected)->folders.size(),1);
    }
}

void TestSessionsWindow::newSessionGroupFollowsLaunchIdentity_data()
{
    QTest::addColumn<QString>("host"); QTest::addColumn<QString>("mode");
    QTest::newRow("local-success") << QString() << QString("success");
    QTest::newRow("remote-success") << QString("mac") << QString("success");
    QTest::newRow("restart") << QString("mac") << QString("restart");
    QTest::newRow("deleted-group") << QString() << QString("deleted");
    QTest::newRow("failed-open") << QString("mac") << QString("failed");
    QTest::newRow("empty-project") << QString() << QString("empty");
    QTest::newRow("dismissed") << QString() << QString("dismissed");
    QTest::newRow("legacy-intent") << QString() << QString("legacy");
}

void TestSessionsWindow::newSessionGroupFollowsLaunchIdentity()
{
    QFETCH(QString, host); QFETCH(QString, mode);
    SessionOrganization organization;
    const QString group = organization.createGroup("Design");
    organization.addFolder(group,host.isEmpty()?QString("arch"):host,"/remote/infra");
    if (mode != "empty") organization.moveSession(organization.observe("arch", "codex/hgs/dashboard", ""), group);
    setJsonSetting("workspace/organization", QJsonDocument(organization.toJson()).toJson(QJsonDocument::Compact));
    auto state = fleet();
    auto window = std::make_unique<SessionsWindow>(program()); window->setFleet(state); window->show(); window->showSession({}, "codex/hgs/dashboard");
    QSignalSpy launches(window.get(), &SessionsWindow::newSessionRequested);
    const QString destination = group;
    QTimer::singleShot(0, window.get(), [&] {
        auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        QTimer::singleShot(20000, dialog, &QDialog::reject);
        auto *groups = dialog->findChild<QComboBox *>("launchProject"); QVERIFY(groups);
        QCOMPARE(groups->currentData().toString(), group);
        QVERIFY(groups->findData("pinned") < 0); QVERIFY(groups->findData("ungrouped") >= 0);
        groups->setCurrentIndex(groups->findData(destination));
        QCOMPARE(dialog->findChild<QComboBox *>("launchProjectFolder")->count(), 1);
        dialog->findChild<QCheckBox *>("launchOpenTerminal")->setChecked(true);
        dialog->findChild<QLineEdit *>("launchName")->setText("grouped");
        auto *start = dialog->findChild<QPushButton *>("primary"); QTRY_VERIFY_WITH_TIMEOUT(start->isEnabled(), 15000); start->click();
    });
    window->showNewSession("codex", group, host); QTRY_COMPARE(launches.size(), 1);
    const QString token = launches[0][5].toString(); QVERIFY(!QUuid(token).isNull()); QCOMPARE(launches[0][0].toString(), host);
    auto placeholder = [&]() -> QListWidgetItem * {
        auto *list = window->findChild<SessionList *>("sessionList");
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::LaunchId).toString() == token) return list->item(i);
        return nullptr;
    };
    QVERIFY(placeholder()); QCOMPARE(placeholder()->data(SessionRoles::Title).toString(), QString("grouped"));
    QCOMPARE(placeholder()->data(SessionRoles::Status).toString(), QString("Starting agent…"));
    QCOMPARE(placeholder()->data(SessionRoles::Group).toString(), destination);
    QVERIFY(!placeholder()->isHidden()); QVERIFY(placeholder()->flags() & Qt::ItemIsSelectable);
    QVERIFY(!(placeholder()->flags() & Qt::ItemIsDragEnabled));
    const auto preview = qEnvironmentVariable("HGS_NAVIGATION_PREVIEW");
    if (mode == "empty" && !preview.isEmpty()) { QDir().mkpath(preview); QVERIFY(window->grab().save(preview + "/starting-session.png")); }
    auto pending = window->findChild<SessionCreationController *>()->operations();
    QCOMPARE(pending.value(token).toObject().value("group").toString(), destination);
    QCOMPARE(SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object()).group(destination)->folders.size(),1);
    SessionInfo created; created.name = "codex/infra/grouped"; created.cmd = "codex"; created.project = "infra"; created.tag = "grouped";
    created.runId = "run-created"; created.state = "running"; created.launchId = "different-launch";
    auto box = host.isEmpty() ? state.local() : *state.peer(host);
    const QString machine = host.isEmpty() ? box.host : host;
    const auto deliver = [&] {
        box.sessions.removeIf([&](const SessionInfo &s) { return s.name == created.name; }); box.sessions.append(created);
        if (host.isEmpty()) state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); else state.setPeer(box, QDateTime::currentMSecsSinceEpoch());
        window->setFleet(state);
    };
    deliver(); // A matching name from someone else's launch is never moved.
    QVERIFY(placeholder());
    auto stored = SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(stored.groupFor(machine + '\n' + created.name), QString("ungrouped"));
    QVERIFY(window->findChild<SessionCreationController *>()->owns(token));
    if (mode == "failed") { window->newSessionLaunchFailed(token); QVERIFY(placeholder());QCOMPARE(placeholder()->data(SessionRoles::Status).toString(),QString("Creation result unknown")); }
    if (mode == "dismissed") {
        auto *list = window->findChild<SessionList *>("sessionList");
        list->customContextMenuRequested(list->visualItemRect(placeholder()).center());
        auto *menu = window->findChild<QMenu *>("pendingLaunchMenu"); QVERIFY(menu);
        menu->actions().first()->trigger(); menu->hide(); QVERIFY(!placeholder());
    }
    if (mode == "restart" || mode == "deleted" || mode == "legacy") {
        window.reset();
        if (mode == "legacy") {
            auto launch = pending.value(token).toObject(); launch.remove("show_pending");launch.remove("creation_schema");launch.remove("launch_id");
            QFile::remove(QSettings().fileName()+".creations.json");
            setJsonSetting("workspace/pendingLaunches", QJsonDocument(QJsonObject{{token,launch}}).toJson(QJsonDocument::Compact));
        }
        if (mode == "deleted") {
            stored.removeGroup(group); setJsonSetting("workspace/organization", QJsonDocument(stored.toJson()).toJson(QJsonDocument::Compact));
        }
        window = std::make_unique<SessionsWindow>(program()); window->setFleet(state); window->show();
        QTRY_COMPARE(placeholder() != nullptr, mode != "legacy");
    }
    created.launchId = token; created.runId = "run-receipt"; deliver();
    QVERIFY(!placeholder());
    stored = SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(stored.groupFor(machine + '\n' + created.name), mode == "deleted" ? QString("ungrouped") : destination);
    QVERIFY(!QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().contains(token));
    const auto *resultGroup=stored.group(mode=="deleted"?stored.defaultProject():destination);QVERIFY(resultGroup);
    // The listed folder already belongs to the project, whatever path confirmed it.
    const bool added=std::any_of(resultGroup->folders.cbegin(),resultGroup->folders.cend(),[&](const SessionOrganization::Folder &f){return f.machine==machine&&f.path=="/remote/work tree";});
    QVERIFY(!added);
}

void TestSessionsWindow::launchPlaceholderReportsASessionThatEndedUnlisted_data()
{
    QTest::addColumn<QString>("host"); QTest::addColumn<QString>("mode");
    QTest::newRow("local-ended") << QString() << QString("ended");
    QTest::newRow("peer-ended") << QString("mac") << QString("ended");
    QTest::newRow("unknown-exit") << QString("mac") << QString("unknown");
    QTest::newRow("hidden-status") << QString() << QString("hidden");
    QTest::newRow("listed-launch") << QString("mac") << QString("listed");
    QTest::newRow("other-machine") << QString() << QString("other");
    QTest::newRow("unavailable") << QString() << QString("unavailable");
}

void TestSessionsWindow::launchPlaceholderReportsASessionThatEndedUnlisted()
{
    QFETCH(QString, host); QFETCH(QString, mode);
    // A detached launch reported started five minutes ago; no session or record appeared.
    const QString token = QUuid::createUuid().toString(QUuid::WithoutBraces);
    setJsonSetting("workspace/pendingLaunches", QJsonDocument(QJsonObject{{token, QJsonObject{{"host", host},
        {"cmd", "claude"}, {"tag", "failing"}, {"group", "ungrouped"}, {"target", "/remote/infra"}, {"add_folder", false},
        {"show_pending", mode != "hidden"}, {"created", QDateTime::currentSecsSinceEpoch() - 300}}}}).toJson(QJsonDocument::Compact));
    auto state = fleet();
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); auto *notice = window.findChild<QLabel *>("notice");
    QVERIFY(list); QVERIFY(notice);
    auto placeholder = [&]() -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::LaunchId).toString() == token) return list->item(i);
        return nullptr;
    };
    QCOMPARE(placeholder() != nullptr, mode != "hidden");
    if (placeholder()) QCOMPARE(placeholder()->data(SessionRoles::Status).toString(), QString("Waiting for session"));
    // The machine reports that the launch's session ended without a listed session or record.
    const QString reporter = mode != "other" ? host : host.isEmpty() ? QString("mac") : QString();
    auto box = reporter.isEmpty() ? state.local() : *state.peer(reporter);
    box.endedLaunches.insert(token, QJsonObject{{"launch_id", token}, {"name", "claude/infra/failing"},
        {"code", mode == "unknown" ? QJsonValue() : QJsonValue(3)},
        {"output", mode == "unknown" ? QString() : QString("hgs: tracking needs trusted hooks\ncannot start: example failure")}});
    if (mode == "listed") {
        // A session that carries the launch ID under another name is not an ended launch.
        SessionInfo renamed; renamed.name = "claude/infra/renamed"; renamed.cmd = "claude"; renamed.project = "infra";
        renamed.tag = "renamed"; renamed.state = "running"; renamed.launchId = token; box.sessions.append(renamed);
    }
    // A listing that reports its machine unavailable is no evidence.
    if (mode == "unavailable") box.ok = false;
    if (reporter.isEmpty()) state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    else state.setPeer(box, QDateTime::currentMSecsSinceEpoch());
    window.setFleet(state);
    const bool ends = mode != "listed" && mode != "other" && mode != "unavailable";
    QCOMPARE(placeholder() == nullptr, ends || mode == "hidden");
    QCOMPARE(QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().contains(token), !ends);
    if (mode == "ended") {
        QCOMPARE(notice->text(), QString("Session failing ended before it appeared (exit code 3).\n"
            "hgs: tracking needs trusted hooks\ncannot start: example failure"));
        QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
    } else if (mode == "unknown") {
        QCOMPARE(notice->text(), QString("Session failing ended before it appeared."));
    } else {
        // A hidden launch status stays hidden; a launch that may still appear keeps waiting.
        QVERIFY(!notice->text().contains("ended before it appeared"));
    }
    if (mode == "ended" || mode == "unknown") {
        // The next listing changes nothing: the launch is no longer pending.
        notice->clear(); window.setFleet(state);
        QVERIFY(notice->text().isEmpty()); QVERIFY(!placeholder());
    }
}

void TestSessionsWindow::endedLaunchOutranksALaterStartedCallback_data()
{
    QTest::addColumn<QString>("order");
    QTest::newRow("ended-then-started") << QString("ended");
    QTest::newRow("started-then-ended") << QString("started");
    QTest::newRow("listed-then-started") << QString("listed");
}

void TestSessionsWindow::endedLaunchOutranksALaterStartedCallback()
{
    QFETCH(QString, order);
    // A detached launch waits for both its command's result and its first listing.
    const QString token = QUuid::createUuid().toString(QUuid::WithoutBraces);
    setJsonSetting("workspace/pendingLaunches", QJsonDocument(QJsonObject{{token, QJsonObject{{"host", QString()},
        {"cmd", "claude"}, {"tag", "racing"}, {"group", "ungrouped"}, {"target", "/remote/infra"}, {"add_folder", false},
        {"show_pending", true}, {"created", QDateTime::currentSecsSinceEpoch()}}}}).toJson(QJsonDocument::Compact));
    auto state = fleet();
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *notice = window.findChild<QLabel *>("notice");
    auto *list = window.findChild<SessionList *>("sessionList");
    QVERIFY(client); QVERIFY(notice); QVERIFY(list);
    SessionInfo created; created.name = "claude/infra/racing"; created.cmd = "claude"; created.project = "infra";
    created.tag = "racing"; created.state = "running"; created.launchId = token;
    const auto deliver = [&](bool ended, bool listed) {
        auto box = state.local();
        if (ended) box.endedLaunches.insert(token, QJsonObject{{"launch_id", token}, {"name", created.name},
            {"code", 3}, {"output", "cannot start: example failure"}});
        if (listed) box.sessions.append(created);
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    };
    const auto opened = [&] {
        const auto *current = list->currentItem();
        return current && current->data(SessionRoles::Identity).toString() == "arch\n" + created.name;
    };
    if (order == "started") {
        client->nativeSessionLaunched(token, true, {});
        QCOMPARE(notice->text(), QString("Session started. Loading activity…"));
    }
    deliver(order != "listed", order == "listed");
    QVERIFY(!QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().contains(token));
    if (order != "started") client->nativeSessionLaunched(token, true, {});
    if (order == "listed") {
        // A placed launch still opens its session after a late success callback.
        QCOMPARE(notice->text(), QString("Session started. Loading activity…"));
        deliver(false, false); QVERIFY(opened());
        return;
    }
    // The ended report wins whichever arrives first.
    QCOMPARE(notice->text(), QString("Session racing ended before it appeared (exit code 3).\ncannot start: example failure"));
    QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
    // Nothing waits to open the ended launch: a later session with its ID is not selected.
    deliver(false, true); QVERIFY(!opened());
}

void TestSessionsWindow::endedLaunchOutranksALaterFailure_data()
{
    QTest::addColumn<QString>("order");
    QTest::newRow("ended-then-failed") << QString("ended");
    QTest::newRow("ended-then-terminal-failed") << QString("terminal");
    QTest::newRow("hidden-ended-then-failed") << QString("hidden");
    QTest::newRow("failed-then-ended") << QString("failed");
}

void TestSessionsWindow::endedLaunchOutranksALaterFailure()
{
    QFETCH(QString, order);
    // A non-detached launch command can fail after its session already ended.
    const QString token = QUuid::createUuid().toString(QUuid::WithoutBraces);
    setJsonSetting("workspace/pendingLaunches", QJsonDocument(QJsonObject{{token, QJsonObject{{"host", QString()},
        {"cmd", "opencode"}, {"tag", "racing"}, {"group", "ungrouped"}, {"target", "/remote/infra"}, {"add_folder", false},
        {"show_pending", order != "hidden"}, {"created", QDateTime::currentSecsSinceEpoch()}}}}).toJson(QJsonDocument::Compact));
    auto state = fleet();
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *notice = window.findChild<QLabel *>("notice");
    QVERIFY(client); QVERIFY(notice);
    const auto end = [&] {
        auto box = state.local();
        box.endedLaunches.insert(token, QJsonObject{{"launch_id", token}, {"name", "opencode/infra/racing"},
            {"code", 3}, {"output", "cannot start: example failure"}});
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    };
    const QString ended("Session racing ended before it appeared (exit code 3).\ncannot start: example failure");
    if (order == "failed") {
        // A failure reported first ends the launch; the later listing adds nothing.
        client->nativeSessionLaunched(token, false, "timed out");
        QCOMPARE(notice->text(), QString("Could not start session racing: timed out"));
        end();
        QCOMPARE(notice->text(), QString("Could not start session racing: timed out"));
        QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
        return;
    }
    end();
    QVERIFY(!QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().contains(token));
    if (order == "hidden") QVERIFY(!notice->text().contains("ended before it appeared"));
    else QCOMPARE(notice->text(), ended);
    // A terminal launch reports its failure directly, possibly without a reason.
    if (order == "terminal") window.newSessionLaunchFailed(token);
    else client->nativeSessionLaunched(token, false, "timed out");
    // The ended report already carries the reason and its last lines. A hidden
    // launch status showed nothing, so its failure still names the launch.
    QCOMPARE(notice->text(), order == "hidden" ? QString("Could not start session racing: timed out") : ended);
    QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
}

void TestSessionsWindow::endedLaunchesInOneListingKeepUnshownFailures()
{
    // Pending launches are visited in key order; the last ended launch's
    // notice replaces the notices of those that ended in the same listing.
    const QString first("00000000-0000-4000-8000-000000000001"), second("00000000-0000-4000-8000-000000000002");
    QJsonObject pending;
    for (const auto &[token, tag] : {std::pair{first, QString("first")}, std::pair{second, QString("second")}})
        pending.insert(token, QJsonObject{{"host", QString()}, {"cmd", "opencode"}, {"tag", tag}, {"group", "ungrouped"},
            {"target", "/remote/infra"}, {"add_folder", false}, {"show_pending", true}, {"created", QDateTime::currentSecsSinceEpoch()}});
    setJsonSetting("workspace/pendingLaunches", QJsonDocument(pending).toJson(QJsonDocument::Compact));
    auto state = fleet();
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *notice = window.findChild<QLabel *>("notice");
    QVERIFY(client); QVERIFY(notice);
    auto box = state.local();
    for (const auto &token : {first, second}) box.endedLaunches.insert(token, QJsonObject{{"launch_id", token}, {"code", 3}});
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    const QString ended("Session second ended before it appeared (exit code 3).");
    QCOMPARE(notice->text(), ended);
    // The shown launch keeps its notice; the other one never showed it, so its
    // later failure still names it.
    client->nativeSessionLaunched(second, false, "timed out");
    QCOMPARE(notice->text(), ended);
    client->nativeSessionLaunched(first, false, "timed out");
    QCOMPARE(notice->text(), QString("Could not start session first: timed out"));
    QVERIFY(notice->isVisible()); QVERIFY(notice->property("error").toBool());
}

void TestSessionsWindow::endedLaunchesWithoutAResultStayBounded()
{
    // Terminal launches and launches restored after a restart never report a
    // result, so the window remembers only the newest ended launches.
    constexpr int remembered = 16;
    QStringList tokens; QJsonObject pending;
    for (int i = 0; i <= remembered; ++i) {
        tokens << QUuid::createUuid().toString(QUuid::WithoutBraces);
        pending.insert(tokens.last(), QJsonObject{{"host", QString()}, {"cmd", "claude"}, {"tag", QString("ended-%1").arg(i)},
            {"group", "ungrouped"}, {"target", "/remote/infra"}, {"add_folder", false}, {"show_pending", true},
            {"created", QDateTime::currentSecsSinceEpoch()}});
    }
    setJsonSetting("workspace/pendingLaunches", QJsonDocument(pending).toJson(QJsonDocument::Compact));
    auto state = fleet();
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *notice = window.findChild<QLabel *>("notice");
    QVERIFY(client); QVERIFY(notice);
    auto box = state.local();
    for (const auto &token : tokens) {
        // Each listing reports one more launch ended, oldest first.
        box.endedLaunches.insert(token, QJsonObject{{"launch_id", token}, {"code", 3}});
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    }
    QVERIFY(QJsonDocument::fromJson(Settings::value("workspace/pendingLaunches").toByteArray()).object().isEmpty());
    const QString newest = QString("Session ended-%1 ended before it appeared (exit code 3).").arg(remembered);
    QCOMPARE(notice->text(), newest);
    // The oldest remembered launch still keeps the ended notice over a late result.
    client->nativeSessionLaunched(tokens[1], true, {});
    QCOMPARE(notice->text(), newest);
    client->nativeSessionLaunched(tokens[2], false, "timed out");
    QCOMPARE(notice->text(), newest);
    // An older one was forgotten: its result is handled like that of any other launch.
    client->nativeSessionLaunched(tokens[0], true, {});
    QCOMPARE(notice->text(), QString("Session started. Loading activity…"));
}

void TestSessionsWindow::folderBrowserIgnoresStaleResponses()
{
    DirectoryDialog dialog(program(), "mac", "~"); dialog.show();
    auto *list = dialog.findChild<QListWidget *>("folderList"); QTRY_COMPARE(list->count(), 1);
    auto *client = dialog.findChild<HgsClient *>();
    client->directoriesReady(999, "mac", {{"path", "/stale"}, {"directories", QJsonArray{}}});
    QCOMPARE(list->count(), 1); QCOMPARE(dialog.directory(), QStringLiteral("/remote/work tree"));
    auto *path = dialog.findChild<QLineEdit *>("folderPath"); path->setFocus(); QTest::keyClicks(path, "/typed");
    QVERIFY(!dialog.findChild<QPushButton *>("chooseFolder")->isEnabled());
}

void TestSessionsWindow::aConfirmingCreationReadsItsComputerUntilListed()
{
    const auto id = QUuid::createUuid().toString(QUuid::WithoutBraces);
    const QJsonObject operation{{"id", id}, {"launch_id", id}, {"creation_schema", 1}, {"state", "confirming"},
        {"created", QDateTime::currentSecsSinceEpoch()}, {"launch_dispatched", true}, {"host", "peer"}, {"cmd", "claude"},
        {"tag", "confirm-me"}, {"target", "/work/confirm"}, {"group", ""}};
    { QFile file(QSettings().fileName() + ".creations.json"); QVERIFY(file.open(QIODevice::WriteOnly));
      file.write(QJsonDocument(QJsonObject{{"schema", 1}, {"operations", QJsonObject{{id, operation}}}}).toJson(QJsonDocument::Compact)); }
    SessionsWindow window(program()); keepDetailsPollsOut(window);
    QSignalSpy reads(&window, &SessionsWindow::hostRefreshRequested);
    window.setFleet(fleet()); window.show();
    QTRY_VERIFY_WITH_TIMEOUT(reads.size() >= 2, 7000);
    for (const auto &read : reads) QCOMPARE(read.at(0).toString(), QString("peer"));
    // Listed: the creation is placed and the reads stop.
    auto state = fleet(); BoxState peer; peer.host = "peer"; peer.ok = true;
    auto listed = state.local().sessions.first(); listed.name = "claude/project/confirm-me"; listed.cmd = "claude"; listed.launchId = id;
    listed.state = "running"; listed.tag = "confirm-me"; listed.project = "project";
    peer.sessions.append(listed); state.setPeer(peer, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QTRY_VERIFY(!window.findChild<SessionCreationController *>()->owns(id)
                || window.findChild<SessionCreationController *>()->operation(id).value("state") == "bound");
    QTest::qWait(2500); const auto settled = reads.size(); QTest::qWait(2500);
    QCOMPARE(reads.size(), settled);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_launch2.moc"
