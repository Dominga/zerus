// U62 (#712): Projects: order, colours, folders and filters, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void emptyProjectsSettingPreservesArchiveAndProjects();
    void projectSwarmFollowsWorkspaceTheme();
    void projectFilterCombinesScopesCountsAndNavigation();
    void projectHeaderFiltersOnlyThisProject();
    void projectFolderActionsKeepTargets();
    void projectFolderNewSessionPrefillsExactTarget();
    void projectOrderAndDefaultBadge();
    void projectsEmbeddedAndRemote();
    void projectColorsApplyOrCancel();
    void projectSessionBadgesFollowTheStatusContract();
    void legacyProjectsMigrateWithBackupAndLocalFolders();
};

void TestSessionsWindow::emptyProjectsSettingPreservesArchiveAndProjects()
{
    SessionOrganization org;
    const auto active = org.createGroup("Active"), empty = org.createGroup("Empty"), archived = org.createGroup("Archived");
    auto state = fleet(); auto box = state.local();
    auto past = box.sessions.first(); past.name = "codex/hgs/old"; past.state = "archived"; past.archiveId = "old-run"; past.runId = "old-run";
    box.sessions.append(past); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    org.moveSession(org.observe("arch", box.sessions.first().name, box.sessions.first().runId), active);
    org.moveSession(org.observe("arch", past.name, past.runId, past.archiveId), archived);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto hasHeader = [&](const QString &id) {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Header).toBool() && list->item(i)->data(SessionRoles::Group).toString() == id) return true;
        return false;
    };
    QVERIFY(hasHeader(active)); QVERIFY(!hasHeader(empty)); QVERIFY(!hasHeader(archived));
    const auto changeSetting = [&](bool enabled, bool save) {
        window.findChild<QPushButton *>("workspaceSettings")->click();
        QVERIFY(!QApplication::activeModalWidget());auto *page=window.findChild<QWidget *>("settingsPage");QVERIFY(page&&page->isVisible());
        page->findChild<QListWidget *>("settingsSections")->setCurrentRow(1);
        auto *check=page->findChild<QCheckBox *>("workspaceHideEmptyProjects");QVERIFY(check);
        if(save)check->setChecked(enabled);
        window.showSession({},"codex/hgs/dashboard");
    };
    changeSetting(false, false); QVERIFY(!hasHeader(empty));
    changeSetting(false, true); QVERIFY(hasHeader(empty)); QVERIFY(hasHeader(archived));
    QVERIFY(!QSettings().value("workspace/hideEmptyProjects").toBool());
    { SessionsWindow reopened(program()); reopened.setFleet(state); reopened.show();
      auto *rows = reopened.findChild<SessionList *>("sessionList"); bool found = false;
      for (int i = 0; i < rows->count(); ++i) found |= rows->item(i)->data(SessionRoles::Group).toString() == empty;
      QVERIFY(found); }
    changeSetting(true, true); QVERIFY(!hasHeader(empty)); QVERIFY(!hasHeader(archived));
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter") == "archived") button->click();
    QVERIFY(hasHeader(archived)); QVERIFY(!hasHeader(active));
    window.showSession({}, box.sessions.first().name);
    box.sessions.first().activity = "idle"; box.sessions.first().phase = "idle"; box.sessions.first().state = "paused";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); QVERIFY(hasHeader(active));
    box.sessions.removeFirst(); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); QVERIFY(!hasHeader(active));
    const SessionOrganization stored(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QVERIFY(stored.group(empty)); QVERIFY(stored.group(active)); QVERIFY(stored.group(archived));
}

void TestSessionsWindow::projectSwarmFollowsWorkspaceTheme()
{
    const QJsonObject organization{{"version",2},{"projects",QJsonArray{}}};
    for(const auto &theme:{QStringLiteral("dark"),QStringLiteral("light")}) {
        QSettings().setValue("workspace/theme",theme);SessionsWindow window(program());window.show();
        HgsClient client(program());SwarmController controller(&client);
        controller.acceptExternal({{"schema",1},{"organization",organization},{"machines",QJsonArray{}},{"conflicts",QJsonArray{}}});
        FleetState fleet;bool checked=false;
        QTimer::singleShot(30,&window,[&]{
            auto *dialog=qobject_cast<QDialog*>(QApplication::activeModalWidget());QVERIFY(dialog);
            QCOMPARE(dialog->palette().color(QPalette::Window),window.palette().color(QPalette::Window));
            checked=true;dialog->reject();
        });
        QTimer timeout;timeout.setSingleShot(true);connect(&timeout,&QTimer::timeout,&window,[]{if(auto *dialog=qobject_cast<QDialog*>(QApplication::activeModalWidget()))dialog->reject();});timeout.start(5000);
        showSwarmDialog(&client,&controller,fleet,&window,[]{});QVERIFY(checked);
    }
}

void TestSessionsWindow::projectFilterCombinesScopesCountsAndNavigation()
{
    SessionOrganization org;
    const auto work = org.createGroup("Focus"), saved = org.createGroup("Focus"); // Names never identify the scope.
    org.moveSession(org.observe("arch", "codex/hgs/dashboard", ""), work);
    org.moveSession(org.observe("mac", "claude/infra/review", ""), work);
    org.moveSession(org.observe("arch", "codex/website/navigation", ""), saved);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(fleet()); window.resize(1100,760); window.show();
    auto *projects = window.findChild<ProjectFilter *>("projectFilterChips"); auto *machines = window.findChild<MachineFilter *>("machineFilterChips");
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto parents = [&] { int count=0; for(int i=0;i<list->count();++i) if(!list->item(i)->data(SessionRoles::Header).toBool()) ++count; return count; };
    projects->setSelection({work}); QCOMPARE(parents(),2);
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter")=="all") QCOMPARE(button->property("count").toInt(),2);
    projects->setSelection({work,saved}); QCOMPARE(parents(),3);
    auto *menu = projects->button()->menu(); menu->popup(window.mapToGlobal(QPoint(100,100))); QTRY_VERIFY(menu->isVisible());
    const auto choice = [&](const QString &id) -> QCheckBox * {
        for (auto *check : menu->findChildren<QCheckBox *>("projectFilterChoice")) if (check->property("project")==id) return check;
        return nullptr;
    };
    auto *workChoice=choice(work), *savedChoice=choice(saved); QVERIFY(workChoice); QVERIFY(savedChoice);
    QCOMPARE(workChoice->property("sessionCount").toInt(),2); QCOMPARE(savedChoice->property("sessionCount").toInt(),1);
    workChoice->click(); QCOMPARE(projects->selection(),QSet<QString>{saved}); QVERIFY(menu->isVisible()); workChoice->click();
    machines->setSelection({"mac"}); QCOMPARE(parents(),1);
    QCOMPARE(choice(work),workChoice); QCOMPARE(choice(saved),savedChoice); QVERIFY(menu->isVisible());
    QCOMPARE(workChoice->property("sessionCount").toInt(),1); QCOMPARE(savedChoice->property("sessionCount").toInt(),0);
    menu->hide(); machines->setSelection({});
    // Search uses the exact logical membership for both metadata and content hits.
    projects->setSelection({work}); auto *search=window.findChild<QLineEdit *>("search"); search->setText("navigation");
    auto *results=window.findChild<QListWidget *>("searchResults"); QCOMPARE(results->count(),0);
    projects->setSelection({saved}); QCOMPARE(results->count(),1); search->clear();
    window.showProjects(); window.showSessionList();
    QCOMPARE(projects->selection(),QSet<QString>{saved}); QCOMPARE(parents(),1);
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter")=="active") button->click();
    QCOMPARE(parents(),0); // The selected project has a saved session only.
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter")=="all") button->click();
    QCOMPARE(projects->selection(),QSet<QString>{saved}); QCOMPARE(parents(),1);
    // A catalog refresh, session rename and explicit all-current-projects selection stay exact.
    auto state=fleet(); auto local=state.local(); local.sessions[2].runId="saved-run";
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    local.sessions[2].name="codex/website/renamed"; state.setLocal(local,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(parents(),1); QCOMPARE(projects->selection(),QSet<QString>{saved});
    const auto preview=qEnvironmentVariable("HGS_PROJECT_FILTER_PREVIEW");
    if(!preview.isEmpty()) {
        QDir().mkpath(preview); projects->setSelection({work,saved});
        for(const auto &theme:{QStringLiteral("dark"),QStringLiteral("light")}) {
            auto *themeChoice=window.findChild<QComboBox *>("workspaceTheme"); QVERIFY(themeChoice);
            themeChoice->setCurrentIndex(themeChoice->findData(theme));
            QTest::qWait(100); QVERIFY(window.grab().save(preview+"/projects-"+theme+".png"));
            menu->popup(window.mapToGlobal(QPoint(110,180))); QTest::qWait(50); QVERIFY(menu->grab().save(preview+"/chooser-"+theme+".png")); menu->hide();
        }
    }
    window.showSession("mac","claude/infra/review"); QVERIFY(projects->selection().isEmpty()); QCOMPARE(parents(),5);
}

void TestSessionsWindow::projectHeaderFiltersOnlyThisProject()
{
    SessionOrganization org; const auto work=org.createGroup("Work"), docs=org.createGroup("Docs");
    org.moveSession(org.observe("arch","codex/hgs/dashboard",""),work); org.moveSession(org.observe("mac","claude/infra/review",""),docs);
    setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(fleet()); window.show();
    auto *list=window.findChild<SessionList *>("sessionList"); auto *projects=window.findChild<ProjectFilter *>("projectFilterChips");
    const auto openMenu=[&](const QString &id) -> QMenu * {
        for(int i=0;i<list->count();++i) if(list->item(i)->data(SessionRoles::Header).toBool()&&list->item(i)->data(SessionRoles::Group)==id) {
            list->scrollToItem(list->item(i)); list->customContextMenuRequested(list->visualItemRect(list->item(i)).center());
            return window.findChild<QMenu *>("sessionGroupMenu");
        }
        return nullptr;
    };
    const auto action=[](QMenu *menu,const QString &name) -> QAction * { for(auto *a:menu->actions()) if(a->objectName()==name)return a;return nullptr; };
    auto *menu=openMenu(work); QVERIFY(menu); auto *filter=action(menu,"filterOnlyProject"); QVERIFY(filter);
    QCOMPARE(filter->text(),QStringLiteral("Filter this project"));
    QVERIFY(!action(menu,"toggleProjectFilter")); QVERIFY(!action(menu,"clearProjectFilter"));
    filter->trigger(); menu->hide();
    QCOMPARE(projects->selection(),QSet<QString>{work});
    QCoreApplication::sendPostedEvents(nullptr,QEvent::DeferredDelete);
    menu=openMenu(work); QVERIFY(menu); QVERIFY(!action(menu,"toggleProjectFilter")); QVERIFY(!action(menu,"clearProjectFilter"));
    action(menu,"filterOnlyProject")->trigger(); menu->hide(); QCOMPARE(projects->selection(),QSet<QString>{work});
    auto *chip=projects->findChild<QPushButton *>("projectFilterChip"); QVERIFY(chip); chip->click(); QVERIFY(projects->selection().isEmpty());
    projects->setSelection({work,docs}); QCoreApplication::sendPostedEvents(nullptr,QEvent::DeferredDelete);
    menu=openMenu(docs); QVERIFY(menu); action(menu,"filterOnlyProject")->trigger(); menu->hide(); QCOMPARE(projects->selection(),QSet<QString>{docs});
    auto *chooser=projects->button()->menu(); chooser->popup(window.mapToGlobal(QPoint(100,100))); QTRY_VERIFY(chooser->isVisible());
    auto *all=chooser->findChild<QPushButton *>("allProjectFilters"); QVERIFY(all); all->click(); chooser->hide(); QVERIFY(projects->selection().isEmpty());
}

void TestSessionsWindow::projectFolderActionsKeepTargets()
{
    // Local folders are real ones of this computer, in the form of the GUI's platform;
    // the peer's folder is POSIX.
    auto state=fleet();auto own=state.local();own.metrics.remove("platform");state.setLocal(own,QDateTime::currentMSecsSinceEpoch());
    const auto style=MachinePlatform::of(own.metrics,true);
    QTemporaryDir temporary;QVERIFY(temporary.isValid());
    const auto path=temporary.path()+"/folder #1";QVERIFY(QDir().mkpath(path));const QString remote="/remote/folder #1";
    SessionOrganization projects;const auto first=projects.createGroup("First"),second=projects.createGroup("Second");
    projects.addFolder(first,"arch",path,"Local",style);projects.addFolder(first,"mac",remote,"Remote");
    projects.addFolder(first,"arch",path+"/missing","Missing",style);projects.addFolder(second,"arch",path,{},style);
    projects.markImported("arch");projects.markImported("mac");
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program());window.setFleet(state);window.show();window.showProjects();
    auto *page=window.findChild<ProjectsDialog *>();page->selectProject(first);
    auto *table=page->findChild<QTableWidget *>("projectFolders");
    auto *open=page->findChild<QPushButton *>("openProjectFolder");QVERIFY(open);QVERIFY(!open->isEnabled());
    FolderUrlRecorder files;table->setCurrentCell(0,0);QVERIFY(open->isEnabled());open->click();
    QCOMPARE(files.urls.size(),1);QCOMPARE(files.urls[0].toLocalFile(),path);
    const auto preview=qEnvironmentVariable("HGS_BULK_PREVIEW");
    if(!preview.isEmpty()){QDir().mkpath(preview);QTest::qWait(30);QVERIFY(page->grab().save(preview+"/project-folders.png"));}
    table->setCurrentCell(2,0);QCOMPARE(table->item(2,0)->text(),QString("Remote"));QVERIFY(!open->isEnabled());QVERIFY(open->toolTip().contains("mac"));
    table->customContextMenuRequested(table->visualItemRect(table->item(2,0)).center());
    QPointer<QMenu> menu=page->findChild<QMenu *>("projectFolderMenu");QVERIFY(menu);
    QVERIFY(!menu->findChild<QAction *>("contextOpenProjectFolder")->isEnabled());
    menu->findChild<QAction *>("contextCopyProjectFolder")->trigger();QCOMPARE(QApplication::clipboard()->text(),remote);
    table->setCurrentCell(0,0); // A menu command still edits the folder it was opened on.
    bool editedRemote=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=page->findChild<QDialog *>("projectFolderDialog");QVERIFY(dialog);
        editedRemote=dialog->findChild<QComboBox *>("projectFolderMachine")->currentData().toString()=="mac";
        QCOMPARE(dialog->findChild<QLineEdit *>("projectFolderPath")->text(),remote);dialog->reject();});
    menu->findChild<QAction *>("contextEditProjectFolder")->trigger();QVERIFY(editedRemote);delete menu.data();
    table->setCurrentCell(1,0);QCOMPARE(table->item(1,0)->text(),QString("Missing"));QVERIFY(!open->isEnabled());
    table->customContextMenuRequested(table->visualItemRect(table->item(0,0)).center());
    menu=page->findChild<QMenu *>("projectFolderMenu");QVERIFY(menu);
    menu->findChild<QAction *>("contextOpenProjectFolder")->trigger();QCOMPARE(files.urls.size(),2);
    page->selectProject(second);
    // N25: Remove folder asks first; the question names the menu's project.
    // The question closes the menu, which deletes itself: its New session…
    // is kept to try on the removed folder afterwards.
    QPointer<QAction> launch=menu->findChild<QAction *>("contextNewProjectFolderSession");launch->setParent(page);
    bool askedRemove=false;
    QTimer::singleShot(0,page,[&]{
        auto *confirm=page->findChild<QMessageBox *>("removeProjectFolderConfirm");QVERIFY(confirm);askedRemove=true;
        QVERIFY(confirm->text().contains(QStringLiteral("from First?")));
        confirm->findChild<QPushButton *>("removeProjectFolderConfirmButton")->click();
    });
    menu->findChild<QAction *>("contextRemoveProjectFolder")->trigger();QVERIFY(askedRemove);delete menu.data();
    QSignalSpy launches(page,&ProjectsDialog::newSessionRequested);launch->trigger();QCOMPARE(launches.size(),0);delete launch.data();
    const SessionOrganization saved(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(saved.group(first)->folders.size(),2);QCOMPARE(saved.group(second)->folders.size(),1);QVERIFY(QDir(path).exists());
    table->customContextMenuRequested(QPoint(5,table->viewport()->height()-5));
    menu=page->findChild<QMenu *>("projectFolderMenu");QVERIFY(menu);QCOMPARE(menu->actions().size(),1);
    QVERIFY(menu->findChild<QAction *>("contextAddProjectFolder"));delete menu.data();
    QTimer::singleShot(0,&window,[&]{auto *dialog=page->findChild<QDialog *>("deleteProjectDialog");QVERIFY(dialog);
        auto *buttons=dialog->findChild<QDialogButtonBox *>();QVERIFY(buttons);
        QVERIFY(buttons->button(QDialogButtonBox::Cancel)->isDefault());
        QVERIFY(dialog->findChild<QLabel *>()->text().contains("First"));
        if(!preview.isEmpty()){QTest::qWait(30);QVERIFY(dialog->grab().save(preview+"/delete-project.png"));}
        QTest::keyClick(dialog,Qt::Key_Return);});
    page->deleteProject(first);
    const SessionOrganization afterDelete(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QVERIFY(afterDelete.group(first));
}

void TestSessionsWindow::projectFolderNewSessionPrefillsExactTarget()
{
    // The local folder is a real one of this computer, in the form of the GUI's platform.
    auto state=fleet();auto own=state.local();own.metrics.remove("platform");state.setLocal(own,QDateTime::currentMSecsSinceEpoch());
    const auto style=MachinePlatform::of(own.metrics,true);
    QTemporaryDir local;QVERIFY(local.isValid());
    SessionOrganization projects;const auto project=projects.createGroup("Chosen project");
    const auto localId=projects.addFolder(project,"arch",local.path(),"Local folder",style);
    projects.addFolder(project,"mac","/remote/other","Other remote folder");
    const auto remoteId=projects.addFolder(project,"mac","/remote/work tree","Chosen remote folder");
    projects.markImported("arch");projects.markImported("mac");
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program());window.resize(1150,760);window.setFleet(state);window.show();window.showProjects();
    auto *page=window.findChild<ProjectsDialog *>();page->selectProject(project);
    auto *table=page->findChild<QTableWidget *>("projectFolders");auto *create=page->findChild<QPushButton *>("newProjectFolderSession");
    const auto rowFor=[&](const QString &id){for(int row=0;row<table->rowCount();++row)if(table->item(row,0)->data(Qt::UserRole).toString()==id)return row;return -1;};
    QVERIFY(rowFor(localId)>=0);QVERIFY(rowFor(remoteId)>=0);
    QVERIFY(!create->isEnabled());table->setCurrentCell(rowFor(localId),0);QVERIFY(create->isEnabled());
    // U25 part 7 (spec 7), #646: Color… lives in the project header; the
    // default checkbox stays right under the project's name.
    auto *metadata=page->findChild<QLabel *>("projectHeaderName");auto *defaultProject=page->findChild<QCheckBox *>("defaultProject");
    for(int height:{760,1100}) {
        window.resize(1150,height);table->clearSelection();QTest::qWait(30);
        QVERIFY(defaultProject->mapTo(page,QPoint()).y()-metadata->mapTo(page,QPoint(0,metadata->height())).y()<45);
        const int full=table->height();table->setCurrentCell(rowFor(localId),0);QTest::qWait(30);QVERIFY(full>=table->height());
        // #646: the table keeps its rows' height at every window height.
        QCOMPARE(table->height(),table->horizontalHeader()->height()+table->rowCount()*table->verticalHeader()->defaultSectionSize()+8);
        // The folder actions under the table (Add folder moved to the Folders block's header).
        auto *folderActions=page->findChild<QPushButton *>("editProjectFolder");
        // The scrolling project lays out after the worktree panel shows: wait for it.
        QTRY_VERIFY(folderActions->mapTo(page,QPoint()).y()>=table->mapTo(page,QPoint(0,table->height())).y());
        auto *catalog=page->findChild<QTreeWidget *>("worktreeCatalog");auto *all=page->findChild<QPushButton *>("allWorktreeSessions");
        QVERIFY(all->mapTo(page,QPoint(0,all->height())).y()<=catalog->mapTo(page,QPoint()).y());
        // The compact heading keeps every worktree action visible without overlap.
        auto *panel=page->findChild<QWidget *>("worktreePanel");QWidget *previous=panel->findChild<QLabel *>("heading");
        for(const char *name:{"allWorktreeSessions","cleanUpWorktrees","refreshWorktrees","useWorktreeFolder"}){auto *button=panel->findChild<QPushButton *>(name);
            QVERIFY2(button->isVisible()&&previous->geometry().right()<button->geometry().left()&&button->geometry().right()<=panel->width(),name);previous=button;}
        if(const auto preview=qEnvironmentVariable("HGS_PROJECT_LAUNCH_PREVIEW");!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(page->grab().save(preview+QString("/projects-%1.png").arg(height)));}
        QVERIFY(defaultProject->mapTo(page,QPoint()).y()-metadata->mapTo(page,QPoint(0,metadata->height())).y()<45);
    }
    bool localOpened=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<NewSessionDialog *>();QVERIFY(dialog);QTimer::singleShot(3000,dialog,&QDialog::reject);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProject")->currentData().toString(),project);
        QCOMPARE(dialog->findChild<QComboBox *>("launchComputer")->currentData().toString(),QString());
        QCOMPARE(dialog->findChild<QComboBox *>("launchProjectFolder")->currentData().toString(),localId);
        QCOMPARE(dialog->findChild<QLabel *>("launchFolderPath")->text(),MachinePlatform::native(local.path(),style));localOpened=true;dialog->reject();});
    create->click();QVERIFY(localOpened);QVERIFY(page->isVisible());
    table->customContextMenuRequested(table->visualItemRect(table->item(rowFor(remoteId),0)).center());
    QPointer<QMenu> menu=page->findChild<QMenu *>("projectFolderMenu");QVERIFY(menu);auto *launch=menu->findChild<QAction *>("contextNewProjectFolderSession");
    QVERIFY(launch->isEnabled());table->setCurrentCell(rowFor(localId),0);
    QSignalSpy launched(&window,&SessionsWindow::newSessionRequested);bool remoteOpened=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<NewSessionDialog *>();QVERIFY(dialog);QTimer::singleShot(20000,dialog,&QDialog::reject);
        QCOMPARE(dialog->findChild<QComboBox *>("launchProject")->currentData().toString(),project);
        QCOMPARE(dialog->findChild<QComboBox *>("launchComputer")->currentData().toString(),QString("mac"));
        auto *folders=dialog->findChild<QComboBox *>("launchProjectFolder");QCOMPARE(folders->currentData().toString(),remoteId);
        window.setFleet(state);QCOMPARE(folders->currentData().toString(),remoteId);
        QCOMPARE(dialog->findChild<QLabel *>("launchFolderPath")->text(),QString("/remote/work tree"));remoteOpened=true;
        auto *accounts=dialog->findChild<QListWidget *>("launchAccount");
        const auto claude=[&]{for(int i=0;i<accounts->count();++i)if(accounts->item(i)->data(Qt::UserRole).toString()=="native-claude")return i;return -1;};
        QTRY_VERIFY_WITH_TIMEOUT(claude()>=0,15000);accounts->setCurrentRow(claude());dialog->findChild<QCheckBox *>("launchOpenTerminal")->setChecked(true);
        const auto preview=qEnvironmentVariable("HGS_PROJECT_LAUNCH_PREVIEW");if(!preview.isEmpty()){QDir().mkpath(preview);QTest::qWait(30);QVERIFY(dialog->grab().save(preview+"/launch.png"));}
        auto *start=dialog->findChild<QPushButton *>("primary");QTRY_VERIFY_WITH_TIMEOUT(start->isEnabled(),15000);start->click();});
    launch->trigger();QVERIFY(remoteOpened);QTRY_COMPARE(launched.size(),1);QCOMPARE(launched[0][0].toString(),QString("mac"));
    QCOMPARE(launched[0][1].toString(),QString("claude"));QCOMPARE(launched[0][2].toString(),QString("/remote/work tree"));delete menu.data();
    window.showProjects();page->selectProject(project);table->setCurrentCell(rowFor(remoteId),0);
    table->customContextMenuRequested(table->visualItemRect(table->item(rowFor(remoteId),0)).center());menu=page->findChild<QMenu *>("projectFolderMenu");
    auto offline=state;BoxState missing;missing.host="mac";missing.error="offline";offline.setPeer(missing,QDateTime::currentMSecsSinceEpoch());window.setFleet(offline);
    QVERIFY(!create->isEnabled());QSignalSpy requests(page,&ProjectsDialog::newSessionRequested);
    menu->findChild<QAction *>("contextNewProjectFolderSession")->trigger();QCOMPARE(requests.size(),0);delete menu.data();
}

void TestSessionsWindow::projectOrderAndDefaultBadge()
{
    SessionOrganization projects;const auto first=projects.createGroup("infra"),second=projects.createGroup("sample-app");
    projects.addFolder(first,"arch","/workspace/infra");projects.moveSession(projects.observe("arch","codex/hgs/dashboard",{}),first);
    QSettings().setValue("workspace/hideEmptyProjects",false);
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    QSettings().setValue("workspace/theme","dark");
    SessionsWindow window(program());window.setFleet(fleet());window.show();window.showProjects();
    auto *page=window.findChild<ProjectsDialog *>();auto *list=page->findChild<QListWidget *>("logicalProjects");
    QCOMPARE(list->dragDropMode(),QAbstractItemView::InternalMove);QVERIFY(list->showDropIndicator());
    QCOMPARE(list->count(),3);QVERIFY(list->item(2)->data(Qt::UserRole+3).toBool());
    QVERIFY(list->item(2)->data(Qt::AccessibleTextRole).toString().contains("default project"));
    list->setCurrentRow(2);QVERIFY(list->model()->moveRow({},2,{},0));
    const auto stored=[] {return SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());};
    QTRY_COMPARE(stored().groups().first().id,QString("ungrouped"));
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(),QString("ungrouped"));
    page->selectProject(first);page->findChild<QCheckBox *>("defaultProject")->click();
    QCOMPARE(stored().defaultProject(),first);QVERIFY(!list->item(0)->data(Qt::UserRole+3).toBool());
    QVERIFY(list->currentItem()->data(Qt::UserRole+3).toBool());
    page->findChild<QAction *>("moveProjectDown")->trigger();
    QCOMPARE(stored().groups().last().id,first);QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(),first);
    QVERIFY(!page->findChild<QAction *>("moveProjectDown")->isEnabled());
    page->findChild<QAction *>("moveProjectUp")->trigger();QCOMPARE(stored().groups()[1].id,first);
    // Order and default are shared with Sessions and survive reopening the page.
    page->refresh();QCOMPARE(list->item(1)->data(Qt::UserRole).toString(),first);QVERIFY(list->item(1)->data(Qt::UserRole+3).toBool());
    window.showSessionList();auto *sessions=window.findChild<SessionList *>("sessionList");QStringList headers;
    for(int i=0;i<sessions->count();++i)if(sessions->item(i)->data(SessionRoles::Header).toBool())headers.append(sessions->item(i)->data(SessionRoles::Group).toString());
    QCOMPARE(headers,QStringList({"ungrouped",first,second}));QCOMPARE(stored().group(first)->folders.size(),1);
    const auto preview=qEnvironmentVariable("HGS_NAVIGATION_PREVIEW");
    if(!preview.isEmpty()){QDir().mkpath(preview);QTest::qWait(30);QVERIFY(page->grab().save(preview+"/projects-default-order.png"));}
}

void TestSessionsWindow::projectsEmbeddedAndRemote()
{
    SessionOrganization projects; const auto id=projects.createGroup("Shared infrastructure");
    projects.setColor(id,"#ffcc00");projects.addFolder(id,"arch","/workspace/infra","Backend");projects.addFolder(id,"mac","/Users/test/infra","Mac checkout");
    setJsonSetting("workspace/organization",QJsonDocument(projects.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showProjects("mac");
    auto *page=window.findChild<ProjectsDialog *>();QVERIFY(page);QVERIFY(!page->isWindow());page->selectProject(id);
    auto *table=page->findChild<QTableWidget *>("projectFolders");QCOMPARE(table->rowCount(),2);
    QCOMPARE(table->item(1,1)->text(),QString("mac"));QCOMPARE(table->item(1,2)->text(),QString("/Users/test/infra"));
    auto *name=page->findChild<QLineEdit *>("projectName");name->setText("Infrastructure / renamed");name->editingFinished();
    const auto restored=SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(restored.group(id)->name,QString("Infrastructure / renamed"));QCOMPARE(restored.group(id)->folders.size(),2);
    QCOMPARE(restored.group(id)->color,QString("#ffcc00"));
    NewSessionDialog launch(program(),fleet(),"mac");launch.setGroups(restored,id);
    QCOMPARE(launch.findChild<QComboBox *>("launchProjectFolder")->count(),1);
    auto *computer=launch.findChild<QComboBox *>("launchComputer");computer->setCurrentIndex(computer->findData(QString()));
    QCOMPARE(launch.findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+2).toString(),QString("/workspace/infra"));
    computer->setCurrentIndex(computer->findData("mac"));
    QCOMPARE(launch.findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+1).toString(),QString("mac"));
    for(auto *button:window.findChildren<QPushButton *>())if(button->property("filter")=="all")button->click();
    QVERIFY(!table->isVisible());
}

void TestSessionsWindow::projectColorsApplyOrCancel()
{
    SessionOrganization model;const auto id=model.createGroup("Release");model.setColor(id,"#00aa7f");
    setJsonSetting("workspace/organization",QJsonDocument(model.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program());window.setFleet(fleet());window.show();window.showProjects();
    auto *page=window.findChild<ProjectsDialog *>();page->selectProject(id);
    bool opened=false;
    const auto edit=[&](bool apply){
        opened=false;bool expired=false;QTimer watchdog;watchdog.setSingleShot(true);
        connect(&watchdog,&QTimer::timeout,&window,[&]{expired=true;if(auto *dialog=page->findChild<QDialog *>("projectColorDialog"))dialog->reject();});watchdog.start(5000);
        QTimer::singleShot(0,&window,[&]{
            auto *dialog=page->findChild<QDialog *>("projectColorDialog");QVERIFY(dialog);
            auto *hex=dialog->findChild<QLineEdit *>("projectColorHex");
            opened=hex&&dialog->findChild<QWidget *>("projectFillPreview");
            if(hex)hex->setText("#ffe100");
            const auto path=qEnvironmentVariable("HGS_PREVIEW_DIR");
            if(!path.isEmpty())dialog->grab().save(path+"/project-color-dialog.png");
            if(apply)dialog->accept();else dialog->reject();
        });
        page->findChild<QPushButton *>("projectColor")->click();
        watchdog.stop();QVERIFY(!expired);QVERIFY(opened);
    };
    edit(false);QVERIFY(opened);
    auto restored=SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(restored.group(id)->color,QString("#00aa7f"));
    edit(true);restored=SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(restored.group(id)->color,QString("#ffe100"));QVERIFY(!restored.group(id)->vivid);
}

// N21 (#386): the Projects page's Sessions badge is the session list's status
// (SessionPresentation::status, the CLAUDE.md status contract): a tool call
// and a compaction are not Ready, an idle agent with live jobs is Background,
// Stopped and Agent exited say so, and a reply to read waits until the work
// is done. "Show all in Sessions" opens the list on the project.
void TestSessionsWindow::projectSessionBadgesFollowTheStatusContract()
{
    const auto restore=qScopeGuard([]{removeSetting("workspace/organization");});
    SessionOrganization model;const auto id=model.createGroup("Status contract");model.setDefaultProject(id);
    model.markImported("arch");model.markImported("mac");
    setJsonSetting("workspace/organization",QJsonDocument(model.toJson()).toJson(QJsonDocument::Compact));
    auto state=fleet();auto local=state.local();
    const auto base=local.sessions[1]; // kimi/docs/research: tracked and idle
    const auto make=[&](const QString &tag){auto s=base;s.name="kimi/docs/"+tag;s.tag=tag;return s;};
    auto tool=make("tool");tool.activity="busy";tool.phase="tool";tool.currentTool="Bash";
    auto compacting=make("compacting");compacting.activity="busy";compacting.phase="compacting";
    auto background=make("background");background.backgroundProcesses=1;
    auto stopped=make("stopped");stopped.state="stopped";
    auto exited=make("exited");exited.processState="exited";
    auto ready=make("ready");
    auto reply=make("reply");reply.unreadReply=true;reply.replyId="reply-1";reply.conversationId="conversation-1";
    auto busyReply=make("busy-reply");busyReply.activity="busy";busyReply.phase="working";busyReply.unreadReply=true;busyReply.replyId="reply-2";busyReply.conversationId="conversation-2";
    auto input=make("input");input.activity="waiting";input.phase="input";input.attentionId="question-1";
    local.sessions={tool,compacting,background,stopped,exited,ready,reply,busyReply,input};
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.resize(1300,900);window.show();window.showProjects();
    auto *page=window.findChild<ProjectsDialog *>("projectsPage");QVERIFY(page);page->selectProject(id);
    window.setFleet(state);
    auto *table=page->findChild<QTableWidget *>("projectSessions");QVERIFY(table);
    const auto badge=[&](const QString &tag){
        for(int row=0;row<table->rowCount();++row)if(table->item(row,0)->data(Qt::UserRole).toString()=="kimi/docs/"+tag)
            return table->item(row,3)->text()+'|'+table->item(row,3)->data(Qt::UserRole+1).toString();
        return QString();
    };
    QTRY_COMPARE(badge("tool"),QString("Using a tool|active"));
    QCOMPARE(badge("compacting"),QString("Compacting|active"));
    QCOMPARE(badge("background"),QString("Background|active"));
    QCOMPARE(badge("stopped"),QString("Stopped|muted"));
    QCOMPARE(badge("exited"),QString("Agent exited|muted"));
    QCOMPARE(badge("ready"),QString("Ready|muted"));
    QCOMPARE(badge("reply"),QString("New reply|warn"));
    QCOMPARE(badge("busy-reply"),QString("Working|active"));
    QCOMPARE(badge("input"),QString("Needs input|warn"));
    // A poll: the compaction ended, the page follows (N22).
    local.sessions[1].activity="idle";local.sessions[1].phase="idle";state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QTRY_COMPARE(badge("compacting"),QString("Ready|muted"));
    // The whole project in Sessions.
    emit page->projectSessionsRequested(id);
    auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(list);QTRY_VERIFY(list->isVisible());
    QStringList keys;for(int i=0;i<list->count();++i){const auto key=list->item(i)->data(SessionRoles::Key).toString();if(!key.isEmpty())keys<<key;}
    QVERIFY(keys.contains("\nkimi/docs/tool"));
    QCOMPARE(window.findChild<ProjectFilter *>("projectFilterChips")->selection(),QSet<QString>{id});
}

void TestSessionsWindow::legacyProjectsMigrateWithBackupAndLocalFolders()
{
    const auto legacy=QJsonDocument(QJsonObject{{"version",1},{"groups",QJsonArray{
        QJsonObject{{"id","work"},{"name","Work"},{"collapsed",true},{"sessions",QJsonArray{"mac\nclaude/infra/review"}}},
        QJsonObject{{"id","pinned"},{"name","Pinned"},{"sessions",QJsonArray{"arch\ncodex/hgs/dashboard"}}}}},
        {"before_pin",QJsonObject{{"arch\ncodex/hgs/dashboard","work"}}}}).toJson(QJsonDocument::Compact);
    setJsonSetting("workspace/organization",legacy);
    SessionsWindow window(program());window.setFleet(fleet());
    auto *client=window.findChild<HgsClient *>("projectClient");QVERIFY(client);
    ProjectInfo local;local.name="Work";local.dir="/workspace/shared";local.exists=true;
    auto ignored=local;ignored.name="Ansible";ignored.fromAnsible=true;
    client->projectsForHostReady({}, {local,ignored});
    auto remote=local;remote.dir="/Users/test/shared";client->projectsForHostReady("mac",{remote});
    QCOMPARE(QSettings().value("workspace/organizationBeforeProjects").toByteArray(),legacy);
    SessionOrganization migrated(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(migrated.groups().size(),2);QVERIFY(!migrated.group("pinned"));
    QCOMPARE(migrated.groupFor("arch\ncodex/hgs/dashboard"),QString("work"));
    QCOMPARE(migrated.group("work")->folders.size(),2);QVERIFY(migrated.group("work")->collapsed);
    QCOMPARE(migrated.group("work")->folders[0].machine,QString("arch"));
    QCOMPARE(migrated.group("work")->folders[1].machine,QString("mac"));
    local.dir="/unexpected";client->projectsForHostReady({}, {local});
    SessionOrganization unchanged(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    QCOMPARE(unchanged.group("work")->folders.size(),2);
    auto offline=fleet();BoxState sleeping;sleeping.host="mac";offline.setPeer(sleeping,QDateTime::currentMSecsSinceEpoch());
    NewSessionDialog launch(program(),offline,"mac");launch.setGroups(migrated,"work");
    auto *start=launch.findChild<QPushButton *>("primary");QVERIFY(!start->isEnabled());
    launch.findChild<QLineEdit *>("launchName")->setText("preserved-name");launch.setFleet(fleet());
    QTRY_VERIFY(start->isEnabled());QCOMPARE(launch.findChild<QLineEdit *>("launchName")->text(),QString("preserved-name"));
    QCOMPARE(launch.findChild<QComboBox *>("launchProjectFolder")->currentData(Qt::UserRole+1).toString(),QString("mac"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_projects.moc"
