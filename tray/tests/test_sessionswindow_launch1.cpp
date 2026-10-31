// U62 (#712): New sessions: folders, launches and their outcomes, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void taskExecutorStartsOnlyThroughTheLaunchDialog();
    void restoredTaskLaunchDeliveryIsReservedAndNeverReplayed_data();
    void restoredTaskLaunchDeliveryIsReservedAndNeverReplayed();
    void launchInitialFolderFollowsProject_data();
    void launchInitialFolderFollowsProject();
    void launchFolderFollowsSelectedSession_data();
    void launchFolderFollowsSelectedSession();
    void launchFolderPrefillPreservesUserChoices_data();
    void launchFolderPrefillPreservesUserChoices();
    void launchFolderPrefillRejectsUnverifiedMain_data();
    void launchFolderPrefillRejectsUnverifiedMain();
    void launchFoldersFollowProject();
    void launchListsSurviveFleetRefreshes();
    void sessionPanelHeaderStartsNewSession();
    void launchAddsOnlyFoldersOutsideTheProject();
    void automaticLaunchWorktree_data();
    void automaticLaunchWorktree();
    void arbitraryFolderAndTerminalPreference_data();
    void arbitraryFolderAndTerminalPreference();
    void remoteFolderLaunch();
    void launchNameFollowsRenameRules();
};

void TestSessionsWindow::taskExecutorStartsOnlyThroughTheLaunchDialog()
{
    SessionOrganization org; const auto project = org.createGroup("Work");
    org.moveSession(org.observe("arch", "codex/hgs/dashboard", ""), project);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *tasks = window.findChild<TasksView *>(); QVERIFY(tasks);
    QSignalSpy launches(&window, &SessionsWindow::newSessionRequested);
    TaskItem task; task.uuid = "task-uuid"; task.targetKind = "new_session"; task.targetSession = "codex/hgs/dashboard";
    task.targetId = "launch_id:parent"; task.targetMachine = "parent-node";
    QVERIFY(QSettings().value("tasks/pendingExecutorLaunches").toByteArray().isEmpty()); QCOMPARE(launches.size(), 0);
    bool submitted = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget()); if (!dialog) return;
        const auto close = qScopeGuard([dialog] { dialog->reject(); });
        QCOMPARE(launches.size(), 0);
        QVERIFY(QSettings().value("tasks/pendingExecutorLaunches").toByteArray().isEmpty());
        dialog->launchRequested({}, "codex", "/workspace/project", "task-helper", "native-codex", project, true, {}, {}, {}, {});
        submitted = true;
    });
    tasks->newExecutorRequested(task, project); QVERIFY(submitted); QCOMPARE(launches.size(), 1);
    const auto records = QJsonDocument::fromJson(QSettings().value("tasks/pendingExecutorLaunches").toByteArray()).object();
    QCOMPARE(records.size(), 1); const auto record = records.begin().value().toObject();
    QCOMPARE(record.value("task").toString(), task.uuid); QCOMPARE(record.value("parent").toString(), QString("parent-node/launch_id:parent"));
    QCOMPARE(record.value("phase").toString(), QString("waiting"));
    QVERIFY(tasks->hasExecutorLaunch(task.uuid));
    const auto token = records.begin().key(); window.newSessionLaunchFailed(token, "Fixture launch failure");
    const auto failed = QJsonDocument::fromJson(QSettings().value("tasks/pendingExecutorLaunches").toByteArray()).object().value(token).toObject();
    QCOMPARE(failed.value("phase").toString(), QString("review"));
}

void TestSessionsWindow::restoredTaskLaunchDeliveryIsReservedAndNeverReplayed_data()
{
    QTest::addColumn<bool>("confirmed");
    QTest::newRow("exact-receipt") << true;
    QTest::newRow("unconfirmed-reply") << false;
}

void TestSessionsWindow::restoredTaskLaunchDeliveryIsReservedAndNeverReplayed()
{
    QFETCH(bool, confirmed);
    const auto token = QUuid::createUuid().toString(QUuid::WithoutBraces);
    SessionOrganization org; const auto project = org.createGroup("Work");
    org.moveSession(org.observe("arch", "codex/hgs/task-helper", token), project);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    const QJsonObject record{{"task", "task-uuid"}, {"project", project}, {"host", ""}, {"cmd", "codex"}, {"tag", "task-helper"},
        {"parent", "parent-node/launch_id:parent"}, {"phase", "waiting"}, {"created", QDateTime::currentSecsSinceEpoch()}};
    setJsonSetting("tasks/pendingExecutorLaunches", QJsonDocument(QJsonObject{{token, record}}).toJson(QJsonDocument::Compact));
    const auto fixture = tasksFixture(confirmed ? "task-executor-confirmed" : "task-executor-unconfirmed", {}); QVERIFY(!fixture.isEmpty());
    if (confirmed) {
        const QJsonObject task{{"uuid", "task-uuid"}, {"stage", "active"}, {"session", QJsonObject{{"host", ""}, {"session_id", "launch_id:" + token}}},
            {"log", QJsonArray{QJsonObject{{"kind", "sent"}, {"request_id", "exact-receipt"}}}}};
        QVERIFY(FakeCli::replace(fixture + ".task-receipt", QJsonDocument(QJsonObject{{"task", task}, {"receipt", QJsonObject{{"request_id", "exact-receipt"}}}}).toJson()));
    }
    auto state = fleet(); auto box = state.local(); auto session = box.sessions.first();
    session.name = "codex/hgs/task-helper"; session.cmd = "codex"; session.launchId = token;
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    {
        SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(state); window.show();
        auto *client = window.findChild<HgsClient *>(); QSignalSpy replied(client, &HgsClient::taskReady);
        // A listing reply cannot be mistaken for a durable exact task-send receipt.
        if (confirmed) QTRY_VERIFY(!QJsonDocument::fromJson(QSettings().value("tasks/pendingExecutorLaunches").toByteArray()).object().contains(token));
        else QTRY_VERIFY(QJsonDocument::fromJson(QSettings().value("tasks/pendingExecutorLaunches").toByteArray()).object().value(token).toObject().value("phase") == "review");
        QFile calls(fixture + ".tasks"); QVERIFY(calls.open(QIODevice::ReadOnly)); const auto text = calls.readAll();
        QVERIFY(text.contains("--expect-new-session-parent")); QVERIFY(text.contains("parent-node/launch_id:parent"));
        window.setFleet(state); QTest::qWait(80); window.close();
    }
    QFile before(fixture + ".tasks"); QVERIFY(before.open(QIODevice::ReadOnly)); const auto previous = before.readAll(); before.close();
    {
        SessionsWindow restored(FakeCli::program(fixture)); keepDetailsPollsOut(restored); restored.setFleet(state); restored.show(); QTest::qWait(120);
    }
    QFile after(fixture + ".tasks"); QVERIFY(after.open(QIODevice::ReadOnly)); const auto final = after.readAll();
    int sends = 0; for (const auto &line : final.split('\n')) if (line.contains("\"send\"")) ++sends;
    QCOMPARE(sends, 1); QVERIFY(final.startsWith(previous));
}

void TestSessionsWindow::launchInitialFolderFollowsProject_data()
{
    QTest::addColumn<bool>("hasFolder");
    QTest::newRow("project-folder")<<true;
    QTest::newRow("empty-project")<<false;
}

void TestSessionsWindow::launchInitialFolderFollowsProject()
{
    QFETCH(bool,hasFolder);
    SessionOrganization projects;
    const auto first=projects.createGroup("Default"),target=projects.createGroup("Selected");
    projects.setDefaultProject(first);
    projects.addFolder(first,"arch","/default/unrelated");
    const auto expected=hasFolder?QString("/selected/work"):QString();
    if(hasFolder)projects.addFolder(target,"arch",expected);
    projects.moveSession("arch\ncodex/hgs/dashboard",target);
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));

    // Callers may supply a fresher organization after construction. An automatic
    // constructor default is not a folder explicitly chosen by the user.
    NewSessionDialog dialog(program(),fleet());
    dialog.setGroups(projects,target);
    QCOMPARE(dialog.findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),expected);
    QVERIFY(!dialog.findChild<QLabel *>("muted")->property("outsideProject").toBool());

    // The global action derives the project from the selected session.
    SessionsWindow window(program());window.setFleet(fleet());window.show();
    window.showSession({},"codex/hgs/dashboard");
    bool checked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *launch=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());
        if(!launch)return;
        const auto actualProject=launch->findChild<QComboBox *>("launchProject")->currentData().toString();
        const auto actualPath=launch->findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString();
        const bool outside=launch->findChild<QLabel *>("muted")->property("outsideProject").toBool();
        launch->reject();checked=true;
        QCOMPARE(actualProject,target);QCOMPARE(actualPath,expected);QVERIFY(!outside);
    });
    window.showNewSession();QVERIFY(checked);
}

void TestSessionsWindow::launchFolderFollowsSelectedSession_data()
{
    QTest::addColumn<QString>("machine");QTest::addColumn<QString>("cwd");
    QTest::addColumn<QString>("canonical");QTest::addColumn<QString>("expected");
    QTest::newRow("ordinary-folder")<<QString("arch")<<QString("/notes")<<QString()<<QString("/notes");
    QTest::newRow("nested-folder")<<QString("arch")<<QString("/notes/drafts")<<QString()<<QString("/notes");
    QTest::newRow("path-boundary")<<QString("arch")<<QString("/notes-copy")<<QString()<<QString("/notes-copy");
    QTest::newRow("canonical-alias")<<QString("arch")<<QString("/alias/drafts")<<QString("/notes/drafts")<<QString("/notes");
    QTest::newRow("remote-folder")<<QString("mac")<<QString("/notes/drafts")<<QString()<<QString("/notes");
    QTest::newRow("main-checkout")<<QString("arch")<<QString("/repo/src")<<QString()<<QString("/repo");
    QTest::newRow("linked-worktree")<<QString("arch")<<QString("/linked/src")<<QString()<<QString("/repo");
    QTest::newRow("remote-linked-worktree")<<QString("mac")<<QString("/linked/src")<<QString()<<QString("/repo");
}

void TestSessionsWindow::launchFolderFollowsSelectedSession()
{
    QFETCH(QString,machine);QFETCH(QString,cwd);QFETCH(QString,canonical);QFETCH(QString,expected);
    QTemporaryDir fixture;QFile program(fixture.filePath("hgs"));QFile original(script());
    QVERIFY(original.open(QIODevice::ReadOnly));QVERIFY(program.open(QIODevice::WriteOnly));program.write(original.readAll());program.close();
    QVERIFY(FakeCli::makeRunnable(program.fileName()));
    QFile response(program.fileName()+".worktrees");QVERIFY(response.open(QIODevice::WriteOnly));
    response.write(QJsonDocument(QJsonObject{{"state","ok"},{"selected_root",cwd.startsWith("/linked/")?"/linked":"/repo"},
        {"common_dir","/repo/.git"},{"sampled_at",QDateTime::currentSecsSinceEpoch()},
        {"worktrees",QJsonArray{QJsonObject{{"path","/repo"},{"kind","main"},{"available",true}},
            QJsonObject{{"path","/linked"},{"kind","linked"},{"available",true}}}}}).toJson());response.close();
    SessionOrganization projects;const auto project=projects.createGroup("Selected"),other=projects.createGroup("Other");
    projects.addFolder(project,machine,"/unrelated");projects.addFolder(project,machine,"/repo");
    projects.addFolder(project,machine,"/notes");projects.addFolder(other,machine,"/elsewhere");
    projects.moveSession("arch\nkimi/docs/research",other);
    // Same paths on another computer must never supply the launch target.
    projects.addFolder(project,machine=="mac"?"arch":"mac","/notes");
    auto state=fleet();const QString target=machine=="arch"?QString():machine;
    auto box=target.isEmpty()?state.local():*state.peer(target);auto &session=box.sessions[0];
    session.cwd=cwd;session.canonicalCwd=canonical;
    projects.moveSession(machine+'\n'+session.name,project);
    if(target.isEmpty())state.setLocal(box,QDateTime::currentMSecsSinceEpoch());else state.setPeer(box,QDateTime::currentMSecsSinceEpoch());
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(FakeCli::program(program.fileName()));window.setFleet(state);window.show();window.showSession(target,session.name);
    bool checked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        QTimer::singleShot(5000,dialog,&QDialog::reject);
        QCOMPARE(dialog->findChild<QComboBox *>("launchComputer")->currentData().toString(),target);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProject")->currentData().toString(),project);
        QTRY_COMPARE(dialog->findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),expected);
        QTRY_VERIFY(!dialog->findChild<QLabel *>("muted")->text().contains("Checking"));
        checked=true;dialog->reject();
    });
    window.findChild<QPushButton *>("newSession")->click();QVERIFY(checked);
    // Explicit launch paths keep linked worktrees; inheritance applies only to +.
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),QString("/linked"));
        QVERIFY(dialog->findChild<QPushButton *>("launchWithoutWorktree")->isChecked());dialog->reject();
    });
    window.showNewSession({},project,target,{},"/linked");
    auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(list);
    // Selecting a project heading leaves the previous conversation open. It
    // must not inherit that conversation's folder into the heading's project.
    for(int row=0;row<list->count();++row)if(list->item(row)->data(SessionRoles::Header).toBool()
        &&list->item(row)->data(SessionRoles::Group).toString()==other){list->setCurrentRow(row);break;}
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),QString("/elsewhere"));dialog->reject();
    });
    window.showNewSession({}, {},target);
}

void TestSessionsWindow::launchFolderPrefillPreservesUserChoices_data()
{
    QTest::addColumn<QString>("choice");
    for(const auto *choice:{"folder","same-folder","project","machine","path","fleet-refresh"})QTest::newRow(choice)<<QString(choice);
}

void TestSessionsWindow::launchFolderPrefillPreservesUserChoices()
{
    QFETCH(QString,choice);
    SessionOrganization projects;const auto project=projects.createGroup("First"),other=projects.createGroup("Other");
    const auto linked=projects.addFolder(project,"arch","/linked"),alternate=projects.addFolder(project,"arch","/alternate");
    projects.addFolder(project,"arch","/repo");projects.addFolder(project,"mac","/remote");projects.addFolder(other,"arch","/other");
    NewSessionDialog dialog(program(),fleet(),{},"codex",project);dialog.setGroups(projects,project);dialog.prefillSessionFolder("/linked");
    auto *folder=dialog.findChild<QComboBox *>("launchProjectFolder");auto *machine=dialog.findChild<QComboBox *>("launchComputer");
    auto *client=dialog.findChild<HgsClient *>();QVERIFY(client);
    QString expected=choice=="fleet-refresh"?"/repo":"/linked";
    if(choice=="folder"){folder->setCurrentIndex(folder->findData(alternate));expected="/alternate";}
    if(choice=="same-folder")folder->activated(folder->findData(linked));
    if(choice=="project"){auto *projectBox=dialog.findChild<QComboBox *>("launchProject");projectBox->setCurrentIndex(projectBox->findData(other));expected="/other";}
    if(choice=="machine"){machine->setCurrentIndex(machine->findData("mac"));expected="/remote";}
    if(choice=="path"){dialog.selectPath("/custom");expected="/custom";}
    if(choice=="fleet-refresh")dialog.setFleet(fleet());
    const QJsonObject catalog{{"state","ok"},{"selected_root","/linked"},{"common_dir","/repo/.git"},
        {"worktrees",QJsonArray{QJsonObject{{"path","/repo"},{"kind","main"},{"available",true}},QJsonObject{{"path","/linked"},{"kind","linked"},{"available",true}}}}};
    // Inject all outstanding IDs before the scripted process finishes. Only the
    // exact inherited request may change the folder, and only until user choice.
    for(quint64 id=1;id<=8;++id)client->worktreesReady(id,{},"/linked",catalog);
    QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),expected);
}

void TestSessionsWindow::launchFolderPrefillRejectsUnverifiedMain_data()
{
    QTest::addColumn<QString>("problem");
    for(const auto *problem:{"stale","partial","foreign-root","foreign-host","unavailable","bare","ambiguous","failed"})QTest::newRow(problem)<<QString(problem);
}

void TestSessionsWindow::launchFolderPrefillRejectsUnverifiedMain()
{
    QFETCH(QString,problem);
    SessionOrganization projects;const auto project=projects.createGroup("First");projects.addFolder(project,"arch","/linked");projects.addFolder(project,"arch","/repo");
    NewSessionDialog dialog(program(),fleet(),{},"codex",project);dialog.setGroups(projects,project);dialog.prefillSessionFolder("/linked/sub");
    auto *client=dialog.findChild<HgsClient *>();QVERIFY(client);
    QJsonObject main{{"path","/repo"},{"kind","main"},{"available",true}};
    if(problem=="unavailable")main["available"]=false;
    if(problem=="bare")main["kind"]="bare";
    QJsonArray trees{main,QJsonObject{{"path","/linked"},{"kind","linked"},{"available",true}}};
    if(problem=="ambiguous")trees.append(main);
    QJsonObject catalog{{"state","ok"},{"selected_root",problem=="foreign-root"?"/linked-other":"/linked"},{"common_dir","/repo/.git"},{"worktrees",trees}};
    if(problem=="stale"||problem=="partial")catalog[problem]=true;
    for(quint64 id=1;id<=8;++id){
        if(problem=="failed")client->worktreesFailed(id,{},"/linked/sub","Unavailable");
        else client->worktreesReady(id,problem=="foreign-host"?"mac":QString(),"/linked/sub",catalog);
    }
    QCOMPARE(dialog.findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),QString("/linked"));
}

void TestSessionsWindow::launchFoldersFollowProject()
{
    SessionOrganization projects;const auto first=projects.createGroup("First"),second=projects.createGroup("Second"),empty=projects.createGroup("Empty");
    const auto original=projects.addFolder(first,"mac","/remote/chosen");
    const auto alternative=projects.addFolder(first,"mac","/remote/list-choice");
    projects.addFolder(second,"mac","/remote/different");projects.addFolder(second,"arch","/local/second");
    NewSessionDialog dialog(program(),fleet(),"mac","codex");dialog.setGroups(projects,first);QVERIFY(dialog.selectFolder(original));dialog.show();
    auto *project=dialog.findChild<QComboBox *>("launchProject");auto *machine=dialog.findChild<QComboBox *>("launchComputer");auto *folder=dialog.findChild<QComboBox *>("launchProjectFolder");auto *account=dialog.findChild<QListWidget *>("launchAccount");
    auto *hint=dialog.findChild<QLabel *>("muted");QVERIFY(hint);
    QTRY_VERIFY(account->currentItem()&&account->currentItem()->data(Qt::UserRole).toString()=="native-codex");
    auto *agent=dialog.findChild<QListWidget *>("launchAccount");QTest::qWait(1);const auto agentTop=agent->geometry().top();
    auto change=[&](const QString &id){project->setCurrentIndex(project->findData(id));};
    folder->setCurrentIndex(folder->findData(alternative));dialog.setFleet(fleet());
    QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),QString("/remote/list-choice"));
    change(second);QCOMPARE(machine->currentData().toString(),QString("mac"));QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),QString("/remote/different"));
    QVERIFY(!hint->property("outsideProject").toBool());QCOMPARE(account->currentItem()->data(Qt::UserRole).toString(),QString("native-codex"));
    change(empty);QVERIFY(folder->currentData(Qt::UserRole+2).toString().isEmpty());
    QTest::qWait(1);QCOMPARE(agent->geometry().top(),agentTop);
    dialog.setFleet(fleet());QVERIFY(folder->currentData(Qt::UserRole+2).toString().isEmpty());
    change(first);QCOMPARE(folder->currentData().toString(),original);QVERIFY(!hint->property("outsideProject").toBool());QVERIFY(hint->styleSheet().isEmpty());
    dialog.selectPath("/remote/contextual");dialog.setFleet(fleet());QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),QString("/remote/contextual"));
    QTest::qWait(1);QCOMPARE(agent->geometry().top(),agentTop);
    auto *worktrees=dialog.findChild<QAction *>("chooseLaunchWorktree");
    auto *newWorktree=dialog.findChild<QAction *>("newLaunchWorktree");
    worktrees->setVisible(true);newWorktree->setVisible(true);QTest::qWait(1);QCOMPARE(agent->geometry().top(),agentTop);
    worktrees->setVisible(false);newWorktree->setVisible(false);QTest::qWait(1);QCOMPARE(agent->geometry().top(),agentTop);
    change(second);QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),QString("/remote/different"));
    machine->setCurrentIndex(machine->findData(QString()));QCOMPARE(folder->currentData(Qt::UserRole+2).toString(),QString("/local/second"));
    change(empty);QCOMPARE(machine->currentData().toString(),QString());QVERIFY(folder->currentData(Qt::UserRole+2).toString().isEmpty());
    machine->setCurrentIndex(machine->findData("mac"));QVERIFY(folder->currentData(Qt::UserRole+2).toString().isEmpty());
    const auto offline=projects.addFolder(first,"missing","/offline/project");dialog.setGroups(projects,first);QVERIFY(dialog.selectFolder(offline));change(empty);
    QCOMPARE(machine->currentData().toString(),QString("missing"));QVERIFY(folder->currentData(Qt::UserRole+2).toString().isEmpty());QVERIFY(!dialog.findChild<QPushButton *>("primary")->isEnabled());
}

void TestSessionsWindow::launchListsSurviveFleetRefreshes()
{
    // Polls arrive while a list is open; rebuilding its rows would reset the hovered row.
    SessionOrganization projects;const auto id=projects.createGroup("First");
    projects.addFolder(id,"mac","/remote/one");const auto two=projects.addFolder(id,"mac","/remote/two");
    NewSessionDialog dialog(program(),fleet(),"mac","codex");dialog.setGroups(projects,id);QVERIFY(dialog.selectFolder(two));dialog.show();
    auto *folder=dialog.findChild<QComboBox *>("launchProjectFolder");auto *machine=dialog.findChild<QComboBox *>("launchComputer");
    QSignalSpy folderRows(folder->model(),&QAbstractItemModel::rowsRemoved),machineRows(machine->model(),&QAbstractItemModel::rowsRemoved);
    dialog.setFleet(fleet());
    QCOMPARE(folderRows.count(),0);QCOMPARE(machineRows.count(),0);QCOMPARE(folder->currentData().toString(),two);
    QVERIFY(folder->currentData(Qt::UserRole+3).toBool());QVERIFY(dialog.findChild<QPushButton *>("browseLaunchFolder")->isEnabled());
    // Availability still follows the fleet, in place.
    auto offline=fleet();BoxState mac;mac.host="mac";mac.ok=false;offline.setPeer(mac,QDateTime::currentMSecsSinceEpoch());
    dialog.setFleet(offline);
    QCOMPARE(folderRows.count(),0);QCOMPARE(machineRows.count(),0);QCOMPARE(folder->currentData().toString(),two);
    // U25 part 8: the dense dialog keeps the name and frames an offline computer in warn.
    QVERIFY(!folder->currentData(Qt::UserRole+3).toBool());QCOMPARE(machine->currentText(),QString("mac"));QVERIFY(machine->property("denseBad").toBool());
    QVERIFY(!dialog.findChild<QPushButton *>("primary")->isEnabled());QCOMPARE(dialog.findChild<QPushButton *>("primary")->toolTip(),QString("mac is offline"));
    dialog.setFleet(fleet());QVERIFY(folder->currentData(Qt::UserRole+3).toBool());QCOMPARE(machine->currentText(),QString("mac"));QVERIFY(!machine->property("denseBad").toBool());
    // A changed folder list is still shown.
    projects.addFolder(id,"mac","/remote/three");dialog.setGroups(projects,id);
    QVERIFY(folder->findData("/remote/three",Qt::UserRole+2)>=0);
}

void TestSessionsWindow::sessionPanelHeaderStartsNewSession()
{
    QSettings().setValue("workspace/expandSessionsOnHover", false);
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    auto *create = window.findChild<QPushButton *>("sessionPanelNewSession"), *toggle = window.findChild<QPushButton *>("sessionPanelToggle");
    QVERIFY(create && toggle); QVERIFY(create->isVisible()); QCOMPARE(create->parentWidget()->objectName(), QString("sessionListPanel"));
    QCOMPARE(create->property("glyph").toString(), QString("add")); QVERIFY(create->x() < toggle->x());
    bool opened = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());
        opened = dialog != nullptr; if (dialog) dialog->reject();
    });
    create->click(); QVERIFY(opened);
    // The strip keeps only its toggle; the rail still offers New session.
    toggle->click(); QTRY_COMPARE(window.findChild<SessionList *>("sessionList")->property("expansion").toReal(), 0.0);
    QVERIFY(!create->isVisible());
    toggle->click(); QTRY_VERIFY(create->isVisible());
}

void TestSessionsWindow::launchAddsOnlyFoldersOutsideTheProject()
{
    // A worktree of a project folder is reached through that folder; only an
    // outside folder joins the project once its launch is observed.
    SessionOrganization organization;const auto group=organization.createGroup("Design");
    organization.addFolder(group,"mac","/repo");
    setJsonSetting("workspace/organization",QJsonDocument(organization.toJson()).toJson(QJsonDocument::Compact));
    auto state=fleet();auto box=*state.peer("mac");
    SessionsWindow window(program());window.setFleet(state);window.show();
    QSignalSpy launches(&window,&SessionsWindow::newSessionRequested);QStringList paths;
    const auto start=[&](const QString &path,const QString &name,const QString &hint){
        const auto previousLaunches=launches.size();
        QTimer::singleShot(0,&window,[&,name,hint]{
            auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
            // The safety reject outlasts the waits: slow Windows runners start each fake CLI through Python.
            QTimer::singleShot(20000,dialog,&QDialog::reject);
            QTRY_COMPARE_WITH_TIMEOUT(dialog->findChild<QLabel *>("muted")->text(),hint,15000);
            dialog->findChild<QPushButton *>("launchWithoutWorktree")->click();
            dialog->findChild<QCheckBox *>("launchOpenTerminal")->setChecked(true);
            dialog->findChild<QLineEdit *>("launchName")->setText(name);
            auto *button=dialog->findChild<QPushButton *>("primary");QTRY_VERIFY_WITH_TIMEOUT(button->isEnabled(),15000);button->click();
        });
        window.showNewSession("codex",group,"mac",{},path);QTRY_COMPARE(launches.size(),previousLaunches+1);
        SessionInfo created;created.name="codex/infra/"+name;created.cmd="codex";created.project="infra";created.tag=name;
        created.runId="run-"+name;created.state="running";created.launchId=launches.last()[5].toString();
        box.sessions.append(created);state.setPeer(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
        const SessionOrganization stored(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
        QCOMPARE(stored.groupFor("mac\ncodex/infra/"+name),group);
        paths.clear();for(const auto &folder:stored.group(group)->folders)paths<<folder.path;
    };
    // The fixture confirms every chosen folder as /remote/work tree.
    start("/linked","linked",{});QCOMPARE(launches.size(),1);QCOMPARE(paths,QStringList{"/repo"});
    start("/elsewhere","outside","This folder is outside the project.\nStarting here will add it to this project.");
    QCOMPARE(launches.size(),2);QCOMPARE(paths,(QStringList{"/repo","/remote/work tree"}));
}

void TestSessionsWindow::automaticLaunchWorktree_data()
{
    QTest::addColumn<QString>("mode");QTest::addColumn<QString>("host");
    QTest::newRow("local-auto")<<QString("created")<<QString();
    QTest::newRow("remote-auto")<<QString("created")<<QString("mac");
    QTest::newRow("outside-auto")<<QString("outside")<<QString("mac");
    QTest::newRow("no-worktree")<<QString("none")<<QString();
    QTest::newRow("ordinary-folder")<<QString("not_repo")<<QString();
    QTest::newRow("creation-failed")<<QString("failed")<<QString("mac");
    QTest::newRow("unconfirmed-receipt")<<QString("wrong_receipt")<<QString();
}

void TestSessionsWindow::automaticLaunchWorktree()
{
    QFETCH(QString,mode);QFETCH(QString,host);
    QTemporaryDir fixture;QFile program(fixture.filePath("hgs"));QVERIFY(program.open(QIODevice::WriteOnly));
    program.write(R"PY(#!/usr/bin/env python3
import json, pathlib, sys, time
root = pathlib.Path(__file__).parent
mode = (root / 'mode').read_text()
args = sys.argv[1:]
with (root / 'calls').open('a') as log:
    log.write(json.dumps(args) + '\n')
if args and args[0].startswith('@'):
    args.pop(0)
def option(name):
    return args[args.index(name) + 1]
if args[0] == 'account':
    print(json.dumps({'profiles': [{'id': 'native-codex', 'provider': 'codex', 'label': 'Native account', 'is_default': True}]}))
elif args[0] == 'dirs':
    print(json.dumps({'path': args[-1], 'directories': []}))
elif args[:2] == ['worktrees', 'create']:
    deadline = time.monotonic() + 10
    while not (root / 'release').exists() and time.monotonic() < deadline:
        time.sleep(.02)
    if mode == 'failed':
        print('Synthetic checkout failure', file=sys.stderr)
        sys.exit(1)
    print(json.dumps({'status': 'created', 'request_id': 'foreign-request' if mode == 'wrong_receipt' else option('--request-id'),
                     'branch': option('--branch'), 'common_dir': option('--common-dir'), 'path': option('--destination')}))
elif args[0] == 'worktrees':
    print(json.dumps({'state': 'not_repo' if mode == 'not_repo' else 'ok', 'path': args[1], 'selected_root': '/repo',
                     'common_dir': '/repo/.git', 'sampled_at': int(time.time()),
                     'worktrees': [] if mode == 'not_repo' else [{'path': '/repo', 'kind': 'main', 'available': True}]}))
)PY");program.close();
    QVERIFY(FakeCli::makeRunnable(program.fileName()));
    QFile modeFile(fixture.filePath("mode"));QVERIFY(modeFile.open(QIODevice::WriteOnly));modeFile.write(mode.toUtf8());modeFile.close();
    QFile release(fixture.filePath("release"));
    const auto releaseRequest=[&]{if(!release.exists()&&release.open(QIODevice::WriteOnly))release.close();};
    const auto cleanup=qScopeGuard(releaseRequest);
    const auto creations=[&]{
        QList<QJsonArray> result;QFile calls(fixture.filePath("calls"));if(!calls.open(QIODevice::ReadOnly))return result;
        while(!calls.atEnd()){
            const auto args=QJsonDocument::fromJson(calls.readLine()).array();
            if(args.contains(QString("create")))result.append(args);
        }
        return result;
    };
    SessionOrganization projects;const auto project=projects.createGroup("Launch");
    // An outside repository joins the project, never the worktree generated from it.
    projects.addFolder(project,host.isEmpty()?"arch":host,mode=="outside"?"/unrelated":"/repo");
    NewSessionDialog dialog(FakeCli::program(program.fileName()),fleet(),host,"codex",project);dialog.setGroups(projects,project);dialog.show();
    if(mode=="outside")dialog.selectPath("/repo");
    auto *automatic=dialog.findChild<QPushButton *>("automaticLaunchWorktree");
    auto *none=dialog.findChild<QPushButton *>("launchWithoutWorktree");
    auto *start=dialog.findChild<QPushButton *>("primary");
    auto *name=dialog.findChild<QLineEdit *>("launchName");
    auto *target=dialog.findChild<QLabel *>("launchWorktreeTarget");
    QSignalSpy launched(&dialog,&NewSessionDialog::launchRequested);
    QTRY_VERIFY(start->isEnabled());
    QCOMPARE(creations().size(),0);QTRY_COMPARE(dialog.findChild<QLabel *>("muted")->property("outsideProject").toBool(),mode=="outside");
    auto *manage=dialog.findChild<QToolButton *>("manageLaunchProject");QVERIFY(manage->text().isEmpty());QVERIFY(!manage->icon().isNull());
    auto *agents=dialog.findChild<QListWidget *>("launchAccount");for(int i=0;i<agents->count();++i)QVERIFY(!agents->item(i)->icon().isNull());
    const auto generated=name->text();const auto destination="/repo-"+generated;
    name->setText("review-session");
    if(mode=="not_repo"){
        QVERIFY(!automatic->isEnabled());QVERIFY(none->isChecked());QVERIFY(target->text().isEmpty());
    }else{
        QVERIFY(automatic->isChecked());QCOMPARE(target->text(),destination);
        dialog.setFleet(fleet());QCOMPARE(creations().size(),0);
    }
    if(mode=="none")none->click();
    start->click();
    if(mode=="none"||mode=="not_repo"){
        QTRY_COMPARE(launched.size(),1);QCOMPARE(launched[0][2].toString(),QString("/repo"));QVERIFY(launched[0][7].toString().isEmpty());QCOMPARE(creations().size(),0);return;
    }
    QTRY_COMPARE(creations().size(),1);QVERIFY(!start->isEnabled());QVERIFY(!name->isEnabled());
    QVERIFY(!dialog.findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Cancel)->isEnabled());
    dialog.reject();QVERIFY(dialog.isVisible());
    dialog.setFleet(fleet());QCOMPARE(dialog.findChild<QComboBox *>("launchComputer")->currentData().toString(),host);
    const auto args=creations().first();
    const auto option=[&](const QString &key){
        for(int i=0;i+1<args.size();++i)if(args.at(i).toString()==key)return args.at(i+1).toString();
        return QString();
    };
    QCOMPARE(option("--path"),QString("/repo"));QCOMPARE(option("--common-dir"),QString("/repo/.git"));
    QCOMPARE(option("--branch"),generated);QCOMPARE(option("--base"),QString("HEAD"));QCOMPARE(option("--destination"),destination);
    QVERIFY(!QUuid(option("--request-id")).isNull());if(!host.isEmpty())QCOMPARE(args.first().toString(),"@"+host);
    releaseRequest();
    if(mode=="created"||mode=="outside"){
        QTRY_COMPARE(launched.size(),1);QCOMPARE(launched[0][0].toString(),host);QCOMPARE(launched[0][2].toString(),destination);
        QCOMPARE(launched[0][3].toString(),QString("review-session"));QCOMPARE(launched[0][4].toString(),QString("native-codex"));
        QCOMPARE(launched[0][5].toString(),project);QCOMPARE(launched[0][7].toString(),mode=="outside"?QString("/repo"):QString());
    }else{
        QTRY_VERIFY(name->isEnabled());QVERIFY(!start->isEnabled());QCOMPARE(launched.size(),0);QVERIFY(dialog.isVisible());
        QVERIFY(!dialog.findChild<QLabel *>("launchError")->text().isEmpty());QCOMPARE(name->text(),QString("review-session"));
        start->click();dialog.setFleet(fleet());QCOMPARE(creations().size(),1);QCOMPARE(launched.size(),0);
        none->click();QTRY_VERIFY(start->isEnabled());start->click();QTRY_COMPARE(launched.size(),1);
        QCOMPARE(launched[0][2].toString(),QString("/repo"));
    }
    QCOMPARE(creations().size(),1);
}

void TestSessionsWindow::arbitraryFolderAndTerminalPreference_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark")<<QString("dark");QTest::newRow("light")<<QString("light");
}

void TestSessionsWindow::arbitraryFolderAndTerminalPreference()
{
    QFETCH(QString,theme);QSettings().setValue("workspace/theme",theme);
    QTemporaryDir folderFixture; QFile folderScript(folderFixture.filePath("hgs"));
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly)); QVERIFY(folderScript.open(QIODevice::WriteOnly));
    folderScript.write(original.readAll().replace("\"path\":\"/remote/work tree\"", "\"path\":\"/remote/work tree/\"")); folderScript.close();
    QVERIFY(FakeCli::makeRunnable(folderScript.fileName()));
    SessionsWindow parent(program());parent.setFleet(fleet());parent.show();
    SessionOrganization projects;const auto id=projects.createGroup("infra"),other=projects.createGroup("Other");
    projects.addFolder(other,"mac","/remote/other");
    NewSessionDialog dialog(FakeCli::program(folderScript.fileName()),fleet(),"mac","codex",{},&parent);dialog.setGroups(projects,id);dialog.show();
    auto *folders=dialog.findChild<QComboBox *>("launchProjectFolder");auto *start=dialog.findChild<QPushButton *>("primary");
    auto *terminal=dialog.findChild<QCheckBox *>("launchOpenTerminal");auto *browse=dialog.findChild<QPushButton *>("browseLaunchFolder");
    QCOMPARE(folders->count(),0);QVERIFY(!start->isEnabled());QVERIFY(!terminal->isChecked());
    QTimer::singleShot(0,&dialog,[&]{
        auto *browser=qobject_cast<DirectoryDialog *>(QApplication::activeModalWidget());QVERIFY(browser);browser->reject();
    });browse->click();QCOMPARE(folders->count(),0);
    QTimer::singleShot(0,&dialog,[&]{
        auto *browser=qobject_cast<DirectoryDialog *>(QApplication::activeModalWidget());QVERIFY(browser);
        QTimer::singleShot(3000,browser,&QDialog::reject);
        auto *choose=browser->findChild<QPushButton *>("chooseFolder");QTRY_VERIFY(choose->isEnabled());choose->click();
    });browse->click();QCOMPARE(folders->count(),1);QTRY_VERIFY(start->isEnabled());
    QCOMPARE(folders->currentText(),QString("work tree"));
    auto *project=dialog.findChild<QComboBox *>("launchProject");
    project->setCurrentIndex(project->findData(other));
    QCOMPARE(folders->currentData(Qt::UserRole+2).toString(),QString("/remote/work tree"));
    QVERIFY(dialog.findChild<QLabel *>("muted")->property("outsideProject").toBool());
    project->setCurrentIndex(project->findData(id));
    QCOMPARE(dialog.findChild<QLabel *>("launchFolderPath")->text(),QString("/remote/work tree"));
    QVERIFY(projects.group(id)->folders.isEmpty());
    dialog.setFleet(fleet());QCOMPARE(folders->currentData(Qt::UserRole+2).toString(),QString("/remote/work tree"));
    auto *machine=dialog.findChild<QComboBox *>("launchComputer");machine->setCurrentIndex(machine->findData(QString()));
    QCOMPARE(folders->count(),0);QVERIFY(!start->isEnabled());machine->setCurrentIndex(machine->findData("mac"));QCOMPARE(folders->count(),1);
    const auto preview=qEnvironmentVariable("HGS_NAVIGATION_PREVIEW");
    if(!preview.isEmpty()){QDir().mkpath(preview);QTest::qWait(40);QVERIFY(dialog.grab().save(preview+"/new-session-folder-"+theme+".png"));}
    QSignalSpy launches(&dialog,&NewSessionDialog::launchRequested);terminal->setChecked(true);QTRY_VERIFY(start->isEnabled());start->click();QTRY_COMPARE(launches.size(),1);
    QCOMPARE(launches[0][0].toString(),QString("mac"));QVERIFY(launches[0][6].toBool());QVERIFY(!launches[0][7].toString().isEmpty());QCOMPARE(launches[0][7].toString(),launches[0][2].toString());
    NewSessionDialog second(program(),fleet(),"mac");QVERIFY(second.findChild<QCheckBox *>("launchOpenTerminal")->isChecked());
    second.setGroups(projects,id);QCOMPARE(second.findChild<QComboBox *>("launchProjectFolder")->count(),0);
    projects.addFolder(id,"mac","/remote/work tree");second.setGroups(projects,id);
    second.findChild<QCheckBox *>("launchOpenTerminal")->setChecked(false);
    QSignalSpy secondLaunch(&second,&NewSessionDialog::launchRequested);QTRY_VERIFY(second.findChild<QPushButton *>("primary")->isEnabled());second.findChild<QPushButton *>("primary")->click();QTRY_COMPARE(secondLaunch.size(),1);
    QVERIFY(!secondLaunch[0][6].toBool());QVERIFY(secondLaunch[0][7].toString().isEmpty());
    NewSessionDialog third(program(),fleet());QVERIFY(!third.findChild<QCheckBox *>("launchOpenTerminal")->isChecked());
    // Selecting a project folder explicitly replaces the earlier Browse choice.
    project->setCurrentIndex(project->findData(other));
    folders->setCurrentIndex(folders->findData(QString("/remote/other"),Qt::UserRole+2));
    project->setCurrentIndex(project->findData(id));QCOMPARE(folders->count(),0);
}

void TestSessionsWindow::remoteFolderLaunch()
{
    NewSessionDialog dialog(program(), fleet(), "mac"); dialog.show();
    auto *projects = dialog.findChild<QComboBox *>("launchProject"); QTRY_COMPARE(projects->count(), 1);
    SessionOrganization organization; organization.addFolder("ungrouped","mac","/remote/work tree");
    dialog.setGroups(organization,"ungrouped");
    const auto name = dialog.findChild<QLineEdit *>("launchName")->text(); QVERIFY(name.startsWith("work-"));
    QSignalSpy launched(&dialog, &NewSessionDialog::launchRequested);
    auto *start = dialog.findChild<QPushButton *>("primary"); QTRY_VERIFY(start->isEnabled()); start->click();
    QTRY_COMPARE(launched.size(), 1);
    QCOMPARE(launched[0][0].toString(), QStringLiteral("mac"));
    QCOMPARE(launched[0][1].toString(), QStringLiteral("codex"));
    QCOMPARE(launched[0][2].toString(), QStringLiteral("/remote/work tree"));
    QCOMPARE(launched[0][3].toString(), name);
    QCOMPARE(launched[0][5].toString(), QString("ungrouped"));
    NewSessionDialog second(program(), fleet(), {}, "martty-dsh", "infra");
    QVERIFY(second.findChild<QLineEdit *>("launchName")->text() != name);
    auto *accounts=second.findChild<QListWidget *>("launchAccount");
    QTRY_VERIFY(accounts->currentItem()&&accounts->currentItem()->data(Qt::UserRole+1).toString()=="codex");
    for(int row=0;row<accounts->count();++row)QVERIFY(accounts->item(row)->data(Qt::UserRole+1).toString()!="martty-dsh");
}

void TestSessionsWindow::launchNameFollowsRenameRules()
{
    // Creating a session accepts exactly the tags that Rename accepts.
    NewSessionDialog dialog(program(), fleet(), "mac"); dialog.show();
    QTRY_COMPARE(dialog.findChild<QComboBox *>("launchProject")->count(), 1);
    SessionOrganization organization; organization.addFolder("ungrouped","mac","/remote/work tree");
    dialog.setGroups(organization,"ungrouped");
    auto *name = dialog.findChild<QLineEdit *>("launchName");
    auto *problem = dialog.findChild<QLabel *>("launchNameError"); QVERIFY(problem);
    auto *start = dialog.findChild<QPushButton *>("primary"); QTRY_VERIFY(start->isEnabled());
    for (const QString &bad : {QString(" plan"), QString("plan "), QString("a/b"), QString("a.b"), QString("a:b"), QString()}) {
        name->setText(bad);
        QVERIFY2(!start->isEnabled(), qPrintable(bad));
        QVERIFY2(problem->isVisible() && !problem->text().isEmpty(), qPrintable(bad));
    }
    name->setText("my plan");
    QTRY_VERIFY(start->isEnabled()); QVERIFY(!problem->isVisible());
    QSignalSpy launched(&dialog, &NewSessionDialog::launchRequested);
    start->click();
    QTRY_COMPARE(launched.size(), 1);
    QCOMPARE(launched[0][3].toString(), QStringLiteral("my plan"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_launch1.moc"
