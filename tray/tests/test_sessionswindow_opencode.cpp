// U62 (#712): OpenCode sessions and native UIs, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void openCodeNativeTerminalIsExplicitAndKeepsDraft();
    void openCodeChildrenStayReadOnlyWithUnknownCounts();
    void openCodeModelLaunchUsesNativeAccountAndActivity();
    void openCodeCapabilitiesPreserveDraftWithoutTerminal();
    void openCodeAllModelsReadsTheSessionFolderCatalog();
    void openCodeAnotherFolderStartsThePickerAfresh();
    void openCodeAnswersFromBeforeLeavingASessionStayThere();
    void openCodeAnswersFromBeforeTheSessionVanishedStayThere();
    void openCodeRemovesUndeliveredMessageOnlyAfterConfirmation();
    void openCodeForgetsOnlyItsBindingWhileRunning();
    void openCodeForgetWithoutRunIdentityUsesKill();
    void openCodeSelectionForgetsEachBinding();
    void openCodeSelectionForgetRechecksEachRow_data();
    void openCodeSelectionForgetRechecksEachRow();
    void openCodeSelectionForgetsStoppedAndRemoteBindings();
    void openCodeForgetFinishesWhenTheCliCannotStart_data();
    void openCodeForgetFinishesWhenTheCliCannotStart();
    void openCodeOversizedFormShowsItsReason();
    void openCodeStoppedServiceOffersExplicitStart();
    void machineBatchCountsExcludeNativeOpenCodeSessions();
    void nativeSignInPreservesDraftAndUnblocks();
    void nativeUiOpensBrowserWithoutPlugin();
};

void TestSessionsWindow::openCodeNativeTerminalIsExplicitAndKeepsDraft()
{
    const QString name = "opencode/project/native-terminal", run = "native-terminal-run", conversation = "ses_native_terminal";
    const QJsonObject details{{"tracked", true}, {"cmd", "opencode"}, {"name", name},
        {"conversation_id", conversation}, {"run_id", run}, {"state", "running"},
        {"phase", "idle"}, {"activity", "idle"}, {"process_state", "running"}, {"runtime_state", "live"},
        {"terminal_supported", false}, {"terminal_attach_supported", true}, {"events", QJsonArray{}}, {"cursor", 0}};
    auto state = openCodeSelectionFleet(); auto box = state.local(); auto session = box.sessions.first();
    session.name = name; session.runId = run; session.conversationId = conversation;
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(programInspecting("opencode-native-terminal", details)); keepDetailsPollsOut(window);
    window.setFleet(state); window.show(); window.showSession({}, name);
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *terminal = window.findChild<TerminalView *>(); auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    QVERIFY(client && composer && terminal && tabs);
    QSignalSpy attached(terminal, &TerminalView::connectedChanged); QSignalSpy writes(client, &HgsClient::writeDone);
    settleSelectedInspection(composer, client, {}, name, details);
    setComposerText(composer, "Keep the Activity draft while opening the native TUI");
    QVERIFY(tabs->isTabVisible(1)); QCOMPARE(tabs->currentIndex(), 0);
    QVERIFY(!terminal->isConnected()); QCOMPARE(attached.size(), 0); QCOMPARE(writes.size(), 0);
    tabs->setCurrentIndex(1);
    QTRY_VERIFY(std::any_of(attached.cbegin(), attached.cend(), [](const QList<QVariant> &row) { return row[0].toBool(); }));
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep the Activity draft while opening the native TUI"));
    QCOMPARE(writes.size(), 0);
    tabs->setCurrentIndex(0);
    terminal->disconnectSession();
    QVERIFY(!terminal->isConnected());
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep the Activity draft while opening the native TUI"));
}

void TestSessionsWindow::openCodeChildrenStayReadOnlyWithUnknownCounts()
{
    // The inspector lists finished children here (owner, 2026-10-28: by default
    // they fold into a count with a link that opens them).
    QSettings().setValue("workspace/inspectorFinishedSubagents", true);
    const auto restoreFinished = qScopeGuard([] { QSettings().remove("workspace/inspectorFinishedSubagents"); });
    // `hgs inspect` shape for an OpenCode parent: one classic child finished,
    // and one whose turn end the API host has not observed.
    FleetState state; auto box = fleet().local(); box.sessions = {box.sessions[1]};
    auto &session = box.sessions[0]; session.name = "opencode/docs/review"; session.cmd = "opencode";
    session.project = "docs"; session.tag = "review"; session.conversationId = "ses_parent"; session.runId = "run-one";
    session.subagentSource = "opencode_native"; session.subagentCountsComplete = false;
    session.subagentActiveCount = 0; session.subagentCompletedCount = 1; session.subagentTotalCount = 2;
    session.subagents = {{"ses_done", QJsonObject{{"name", "Finished review (@general subagent)"}, {"state", "finished"}, {"detail", "Native child turn completed"}}},
        {"ses_open", QJsonObject{{"name", "Open review (@general subagent)"}, {"state", "unknown"}, {"detail", "Turn end not recorded"}}}};
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject details{{"tracked", true}, {"backend", "opencode"}, {"conversation_id", session.conversationId}, {"run_id", session.runId},
        {"subagent_source", "opencode_native"}, {"subagents", session.subagents}, {"subagent_counts_complete", false},
        {"subagent_active_count", 0}, {"subagent_completed_count", 1}, {"subagent_unknown_count", 1}, {"subagent_total_count", 2}};
    const QJsonObject childDetails{{"name", session.name}, {"parent_conversation_id", session.conversationId}, {"agent_id", "ses_open"},
        {"conversation_id", "ses_parent/ses_open"}, {"run_id", session.runId}, {"send_supported", false}, {"read_only", true},
        {"history_scope", "Native OpenCode subagent history"}, {"phase", "unknown"},
        {"events", QJsonArray{QJsonObject{{"seq", 1}, {"type", "UserPromptSubmit"}, {"detail", "Review the synthetic docs"}, {"at", 1700000000}}}}};
    const auto fixtureScript = m_dir->filePath("hgs-opencode-children");
    QVERIFY(FakeCli::replace(fixtureScript + ".parent", QJsonDocument(details).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(fixtureScript + ".child", QJsonDocument(childDetails).toJson(QJsonDocument::Compact)));
    QFile fixture(fixtureScript); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[:1]==['inspect']:sys.stdout.write(open(__file__+('.child' if '--agent' in a else '.parent')).read())\n");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    SessionsWindow window(FakeCli::program(fixtureScript)); auto *client = window.findChild<HgsClient *>();
    QSignalSpy parentInspected(client, &HgsClient::inspectionReady);
    QSignalSpy childInspected(client, &HgsClient::subagentInspectionReady);
    window.setFleet(state); window.show(); QTRY_VERIFY(!parentInspected.isEmpty());
    auto *list = window.findChild<SessionList *>("sessionList"); auto *roster = window.findChild<QTextBrowser *>("subagents");
    // No confirmed active child, and the unknown one keeps the uncertainty marker.
    QCOMPARE(list->item(0)->data(SessionRoles::Children).toString(), QString("0+"));
    QVERIFY(roster->toPlainText().contains("0 confirmed active / 2 observed; some states unknown"));
    QVERIFY(roster->toPlainText().contains("Turn end not recorded"));
    list->childrenToggled(list->item(0)->data(SessionRoles::Key).toString()); QCOMPARE(list->count(), 2);
    QCOMPARE(list->item(1)->data(SessionRoles::ChildId).toString(), QString("ses_open"));
    list->setCurrentRow(1); QTRY_VERIFY(!childInspected.isEmpty());
    auto *childComposer = window.findChild<MessageComposer *>("subagentComposer");
    QTRY_VERIFY(!childComposer->isInputVisible());
    bool readOnly = false;
    for (const auto *label : window.findChildren<QLabel *>())
        readOnly |= label->text().startsWith("Native OpenCode subagent history") && label->text().contains("Read only.");
    QVERIFY(readOnly);
    QTRY_VERIFY(window.findChild<ActivityView *>("subagentActivity")->browser()->toPlainText().contains("Review the synthetic docs"));
}

void TestSessionsWindow::openCodeModelLaunchUsesNativeAccountAndActivity()
{
    QTemporaryDir fixture;const auto fake=fixture.filePath("hgs");
    const QString longId = "ollama/" + QString(249, 'x');
    const QString unicodeId = "ollama/" + QString(124, QChar(0x00e9)) + "x";
    const QString id121 = "ollama/" + QString(114, 'x');
    QCOMPARE(longId.toUtf8().size(), 256); QCOMPARE(unicodeId.toUtf8().size(), 256);
    const QJsonArray modelsOffered{
        QJsonObject{{"value","ollama/qwen3:8b"},{"label","Qwen 3"}},
        QJsonObject{{"value","ollama/qwen3:14b"},{"label","Qwen 3 large"}},
        QJsonObject{{"value",id121}},QJsonObject{{"value",longId}},QJsonObject{{"value",unicodeId}},
        QJsonObject{{"value",longId+"x"}},QJsonObject{{"value","ollama/"+QString(125,QChar(0x00e9))}}};
    const QJsonObject accounts{{"profiles",QJsonArray{
        QJsonObject{{"id","native-opencode"},{"provider","opencode"},{"label","Native configuration"},{"installed",true},{"is_default",true},{"model_options",modelsOffered}},
        QJsonObject{{"id","native-codex"},{"provider","codex"},{"label","Native account"},{"installed",true}}}}};
    QVERIFY(FakeCli::install(fake,"#!/usr/bin/env python3\nimport json,sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[0]=='account':print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(accounts).toJson(QJsonDocument::Compact)))+")\n"
        "elif a[0]=='dirs':print('{\"path\":\"/fixture/project\",\"directories\":[]}')\n"
        "elif a[0]=='worktrees':print('{\"state\":\"not_repo\"}')\n"));
    QSettings().setValue("workspace/launchOpenTerminal",true);
    // This computer reports Linux: without a platform a Windows build takes it for
    // Windows, which offers no OpenCode (spec D16).
    auto state=fleet();auto local=state.local();local.metrics=QJsonObject{{"platform","linux"}};state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    NewSessionDialog dialog(FakeCli::program(fake),state,{},"opencode");dialog.selectPath("/fixture/project",false);dialog.show();
    auto *start=dialog.findChild<QPushButton *>("primary"),*none=dialog.findChild<QPushButton *>("launchWithoutWorktree");
    auto *account=dialog.findChild<QListWidget *>("launchAccount");auto *models=dialog.findChild<QComboBox *>("launchModel");
    auto *modelButton=dialog.findChild<QPushButton *>("launchModelButton");
    auto *picker=dialog.findChild<OpenCodeModelPicker *>("launchModelPicker");
    QVERIFY(account);QVERIFY(models);QVERIFY(modelButton);QVERIFY(picker);QVERIFY(!dialog.findChild<QComboBox *>("launchAgent"));
    auto *terminal=dialog.findChild<QCheckBox *>("launchOpenTerminal");
    QTRY_VERIFY(start->isEnabled());QTRY_COMPARE(models->count(),6);
    QCOMPARE(account->currentItem()->data(Qt::UserRole).toString(),QString("native-opencode"));QVERIFY(modelButton->isVisible());QVERIFY(modelButton->isEnabled());QVERIFY(!models->isVisible());QVERIFY(!terminal->isEnabled());
    QVERIFY(terminal->toolTip().contains("Activity"));QVERIFY(none->isChecked());
    QCOMPARE(models->currentData().toString(),QString());modelButton->click();QTRY_VERIFY(picker->isVisible());
    picker->select("ollama/qwen3:8b");QTRY_VERIFY(!picker->isVisible());QCOMPARE(models->currentData().toString(),QString("ollama/qwen3:8b"));
    QVERIFY(models->findData(id121)>=0);QVERIFY(models->findData(longId)>=0);QVERIFY(models->findData(unicodeId)>=0);
    QCOMPARE(models->findData(longId+"x"),-1);QCOMPARE(models->findData("ollama/"+QString(125,QChar(0x00e9))),-1);
    const auto selectAccount=[&](const QString &id){for(int i=0;i<account->count();++i)if(account->item(i)->data(Qt::UserRole).toString()==id){account->setCurrentRow(i);return true;}return false;};
    QVERIFY(selectAccount("native-codex"));QVERIFY(!modelButton->isVisible());QVERIFY(!models->isVisible());QCOMPARE(models->currentData().toString(),QString());
    QVERIFY(selectAccount("native-opencode"));QTRY_COMPARE(models->count(),6);QVERIFY(modelButton->isVisible());
    modelButton->click();QTRY_VERIFY(picker->isVisible());picker->select("ollama/qwen3:8b");QTRY_VERIFY(!picker->isVisible());
    QVERIFY(modelButton->geometry().right()<dialog.width());QVERIFY(start->geometry().bottom()<dialog.height());
    QSignalSpy launch(&dialog,&NewSessionDialog::launchRequested);start->click();QTRY_COMPARE(launch.size(),1);
    QCOMPARE(launch[0][1].toString(),QString("opencode"));QCOMPARE(launch[0][2].toString(),QString("/fixture/project"));
    QCOMPARE(launch[0][4].toString(),QString("native-opencode"));QVERIFY(!launch[0][6].toBool());QCOMPARE(launch[0][8].toString(),QString("ollama/qwen3:8b"));
    QSettings().remove("workspace/launchOpenTerminal");QSettings().remove("workspace/launchAgent");QSettings().remove("workspace/launchAccounts");
}

void TestSessionsWindow::openCodeCapabilitiesPreserveDraftWithoutTerminal()
{
    const QString name="opencode/project/local",run="open-run",conversation="open-session";
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"resumable",true},
        {"terminal_supported",false},{"terminal_reason","OpenCode runs through its native API"},{"attachments_supported",false},
        {"clear_context_supported",false},{"compact_context_supported",false},{"fork_supported",false},
        {"allowed_actions",QJsonArray{"pause","rename"}},{"events",QJsonArray{}},{"cursor",0}};
    const auto fake=programInspecting("opencode-capabilities",details);QVERIFY(!fake.isEmpty());
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.resumable=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";
    session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"pause","rename"};box.sessions={session};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(fake);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);setComposerText(composer,"Keep my local model draft");
    auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");QVERIFY(tabs);QVERIFY(!tabs->isTabVisible(1));QVERIFY(!tabs->isTabVisible(3));
    QTRY_VERIFY2(composer->findChild<QPushButton *>("sendMessage")->isEnabled(),qPrintable(composer->findChild<QLabel *>("messageStatus")->text()+" key="+composer->sessionKey()+" draft="+composer->editor()->toPlainText()));QVERIFY(!composer->canAttachFiles());
    QVERIFY(!composer->findChild<QPushButton *>("attachMessageFile")->isEnabled());
    QVERIFY(!window.findChild<QPushButton *>("clearSessionAction")->isEnabled());QVERIFY(!window.findChild<QAction *>("forgetSessionAction")->isEnabled());
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep my local model draft"));QVERIFY(!composer->editor()->isReadOnly());
}

// All models reads the selected session's own folder catalog on its computer. Only the
// newest read of that session, computer and folder is shown: an older answer, or one for
// a folder the session no longer reports, is dropped. An older hgs offers no All models.
void TestSessionsWindow::openCodeAllModelsReadsTheSessionFolderCatalog()
{
    const QString name="opencode/project/models",run="models-run",conversation="ses_models";
    const QJsonArray options{QJsonObject{{"id","fixture/local"},{"value","fixture/local"},{"label","Local fixture"},
        {"provider","fixture"},{"provider_label","fixture"}}};
    QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},{"settings_change_supported",true},
        {"cwd","/workspace/project"},{"settings_model","fixture/local"},{"model_options",options},{"model_options_truncated",false},
        {"allowed_actions",QJsonArray{"rename"}},{"events",QJsonArray{}},{"cursor",0}};
    const auto fixture=m_dir->filePath("opencode-folder-models");
    const auto writeDetails=[&]{QFile file(fixture+".details");return file.open(QIODevice::WriteOnly)&&file.write(QJsonDocument(details).toJson(QJsonDocument::Compact))>0;};
    QVERIFY(writeDetails());
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer="elif c=='account' and a[1:2]==['models']:\n"
        " import pathlib,time;h=sys.argv[1][1:] if sys.argv[1].startswith('@') else ''\n"
        " open(__file__+'.model-calls','a').write(json.dumps([h]+a)+'\\n')\n"
        " held=pathlib.Path(__file__+'.hold').exists()\n"
        " models=[{'id':'solo/solo-model','value':'solo/solo-model','label':'Solo model','provider':'solo','provider_label':'Solo fixture','free':False}]\n"
        " if held:models.append({'id':'stale/old','value':'stale/old','label':'Stale model','provider':'stale','provider_label':'Stale fixture','free':False})\n"
        " out=json.dumps({'catalog_state':'ready','model_options':[],'models':models,'favorites':[]})\n"
        " if held:pathlib.Path(__file__+'.held').touch()\n"
        " while held and not pathlib.Path(__file__+'.release').exists():time.sleep(.02)\n"
        " print(out)\n"
        "elif c=='inspect':print(open(__file__+'.details').read())\n";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    const auto calls=[&]{QFile file(fixture+".model-calls");return file.open(QIODevice::ReadOnly)?QString::fromUtf8(file.readAll()).split('\n',Qt::SkipEmptyParts):QStringList();};
    const auto mark=[&](const QString &suffix,bool on){QFile file(fixture+suffix);if(on)QVERIFY(file.open(QIODevice::WriteOnly));else file.remove();};
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";session.state="running";
    session.cwd="/workspace/project";session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename"};
    box.sessions={session};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    QSignalSpy answers(client,&HgsClient::openCodeModelsReady);
    composer->findChild<QPushButton *>("sessionModelSettings")->click();
    auto *picker=composer->findChild<OpenCodeModelPicker *>("sessionOpenCodeModels");QVERIFY(picker);QTRY_VERIFY(picker->isVisible());
    auto *all=picker->findChild<QPushButton *>("openCodeAllModels");QVERIFY(all->isEnabled());all->click();
    QTRY_VERIFY(picker->values().contains("solo/solo-model"));QVERIFY(picker->showingAll());
    QCOMPARE(QJsonDocument::fromJson(calls().value(0).toUtf8()).array(),
             QJsonArray({"","account","models","native-opencode","--location","/workspace/project","--json"}));
    // An older read that answers after a newer one is dropped.
    mark(".hold",true);emit composer->openCodeCatalogRequested(composer->sessionKey());
    QTRY_VERIFY(QFile::exists(fixture+".held"));mark(".hold",false);
    emit composer->openCodeCatalogRequested(composer->sessionKey());QTRY_COMPARE(answers.size(),2);
    mark(".release",true);QTRY_COMPARE(answers.size(),3);QTest::qWait(100);
    QVERIFY(!picker->values().contains("stale/old"));QVERIFY(picker->values().contains("solo/solo-model"));
    // So is a read made for the folder the session reported before.
    mark(".release",false);mark(".held",false);mark(".hold",true);
    emit composer->openCodeCatalogRequested(composer->sessionKey());
    QTRY_VERIFY(QFile::exists(fixture+".held"));mark(".hold",false);
    details["cwd"]="/workspace/moved";QVERIFY(writeDetails());settleSelectedInspection(composer,client,{},name,details);
    mark(".release",true);QTRY_COMPARE(answers.size(),4);QTest::qWait(100);
    QVERIFY(!picker->values().contains("stale/old"));QCOMPARE(calls().size(),4);
    // An hgs without `account models` offers no All models.
    details.remove("model_options_truncated");QVERIFY(writeDetails());settleSelectedInspection(composer,client,{},name,details);
    QTRY_VERIFY(!all->isEnabled());QCOMPARE(all->toolTip(),OpenCodeModels::updateText("arch"));
}

// The session's folder is part of the picker's context: when its details report another
// one, the picker starts afresh. A model only the earlier folder offered disappears, and a
// first read pending from there, answered or failed, never owns the new picker. Nothing is
// read on its own; the next explicit All models reads the new folder, once.
void TestSessionsWindow::openCodeAnotherFolderStartsThePickerAfresh()
{
    const QString name="opencode/project/folders",run="folders-run",conversation="ses_folders";
    const QJsonArray options{QJsonObject{{"id","fixture/local"},{"value","fixture/local"},{"label","Local fixture"},
        {"provider","fixture"},{"provider_label","fixture"}}};
    QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},{"settings_change_supported",true},
        {"cwd","/workspace/project"},{"settings_model","fixture/local"},{"model_options",options},{"model_options_truncated",false},
        {"allowed_actions",QJsonArray{"rename"}},{"events",QJsonArray{}},{"cursor",0}};
    const auto fixture=m_dir->filePath("opencode-folder-contexts");
    const auto writeDetails=[&]{QFile file(fixture+".details");return file.open(QIODevice::WriteOnly)&&file.write(QJsonDocument(details).toJson(QJsonDocument::Compact))>0;};
    QVERIFY(writeDetails());
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    // Each read records its folder and lists one model of that folder; it can be held, then fail.
    const QByteArray answer="elif c=='account' and a[1:2]==['models']:\n"
        " import pathlib,time;location=a[a.index('--location')+1]\n"
        " open(__file__+'.model-folders','a').write(location+'\\n')\n"
        " held=pathlib.Path(__file__+'.hold').exists()\n"
        " if held:pathlib.Path(__file__+'.held').touch()\n"
        " while held and not pathlib.Path(__file__+'.release').exists():time.sleep(.02)\n"
        " if pathlib.Path(__file__+'.fail').exists():sys.exit('The catalog is unavailable')\n"
        " folder=location.rsplit('/',1)[-1]\n"
        " print(json.dumps({'catalog_state':'ready','model_options':[],'models':[{'id':'folder/'+folder,'value':'folder/'+folder,'label':'Folder '+folder,'provider':'folder','provider_label':'Folder fixture','free':False}],'favorites':[]}))\n"
        "elif c=='inspect':print(open(__file__+'.details').read())\n";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    const auto folders=[&]{QFile file(fixture+".model-folders");return file.open(QIODevice::ReadOnly)?QString::fromUtf8(file.readAll()).split('\n',Qt::SkipEmptyParts):QStringList();};
    const auto mark=[&](const QString &suffix,bool on){QFile file(fixture+suffix);if(on)QVERIFY(file.open(QIODevice::WriteOnly));else file.remove();};
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";session.state="running";
    session.cwd="/workspace/project";session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename"};
    box.sessions={session};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    QSignalSpy answers(client,&HgsClient::openCodeModelsReady),failures(client,&HgsClient::openCodeModelsFailed);
    auto *open=composer->findChild<QPushButton *>("sessionModelSettings");
    auto *picker=composer->findChild<OpenCodeModelPicker *>("sessionOpenCodeModels");QVERIFY(picker);
    auto *all=picker->findChild<QPushButton *>("openCodeAllModels");auto *notice=picker->findChild<QLabel *>("openCodeModelNotice");
    const auto moveTo=[&](const QString &folder){details["cwd"]=folder;QVERIFY(writeDetails());settleSelectedInspection(composer,client,{},name,details);};
    const auto allModels=[&]{open->click();QTRY_VERIFY(picker->isVisible());all->click();};
    // A first read still pending when the details report another folder never shows there.
    mark(".hold",true);allModels();QTRY_VERIFY(QFile::exists(fixture+".held"));QCOMPARE(notice->text(),QString("Loading all models…"));
    moveTo("/workspace/moved");QVERIFY(!picker->showingAll());QVERIFY(notice->text().isEmpty());
    mark(".hold",false);mark(".release",true);QTRY_COMPARE(answers.size(),1);QTest::qWait(100);
    QVERIFY(!picker->showingAll());QVERIFY(!picker->values().contains("folder/project"));QVERIFY(notice->text().isEmpty());
    // The next explicit All models reads the new folder once and shows its catalog.
    mark(".release",false);mark(".held",false);allModels();QTRY_COMPARE(answers.size(),2);
    QTRY_VERIFY(picker->values().contains("folder/moved"));QVERIFY(picker->showingAll());
    QCOMPARE(folders(),QStringList({"/workspace/project","/workspace/moved"}));
    // A model only the earlier folder offered disappears with it; nothing is read on its own.
    moveTo("/workspace/third");QVERIFY(!picker->showingAll());QVERIFY(!picker->values().contains("folder/moved"));
    QTest::qWait(100);QCOMPARE(folders().size(),2);
    // Nor does the failure of a first read pending from there.
    mark(".hold",true);mark(".fail",true);allModels();QTRY_VERIFY(QFile::exists(fixture+".held"));
    moveTo("/workspace/fourth");mark(".hold",false);mark(".release",true);QTRY_COMPARE(failures.size(),1);QTest::qWait(100);
    QVERIFY(!picker->showingAll());QVERIFY(notice->text().isEmpty());
    mark(".fail",false);mark(".release",false);mark(".held",false);allModels();QTRY_COMPARE(answers.size(),3);
    QTRY_VERIFY(picker->values().contains("folder/fourth"));QVERIFY(!picker->values().contains("folder/third"));
    QCOMPARE(folders(),QStringList({"/workspace/project","/workspace/moved","/workspace/third","/workspace/fourth"}));
    QCOMPARE(failures.size(),1);
}

// Answers to a read and a star made before the session was left never show after a return
// to it, although its computer and folder are the same: the return is a new picker context.
// Nothing is read or starred again on its own; the next explicit All models reads, once.
void TestSessionsWindow::openCodeAnswersFromBeforeLeavingASessionStayThere()
{
    const QString name="opencode/project/first",second="opencode/project/second";
    const QJsonArray options{QJsonObject{{"id","fixture/local"},{"value","fixture/local"},{"label","Local fixture"},{"provider","fixture"},{"provider_label","fixture"}},
        QJsonObject{{"id","fixture/alternate"},{"value","fixture/alternate"},{"label","Alternate fixture"},{"provider","fixture"},{"provider_label","fixture"}}};
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id","first-run"},{"conversation_id","ses_first"},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},{"settings_change_supported",true},
        {"cwd","/workspace/project"},{"settings_model","fixture/local"},{"model_options",options},{"model_options_truncated",false},
        {"allowed_actions",QJsonArray{"rename"}},{"events",QJsonArray{}},{"cursor",0}};
    auto secondDetails=details;secondDetails["name"]=second;secondDetails["run_id"]="second-run";secondDetails["conversation_id"]="ses_second";
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    // Reads and stars record what they are and can be held; a star answers with its exact receipt.
    const QByteArray answer="elif c=='account' and a[1:2] in (['models'],['favorite']):\n"
        " import pathlib,time;open(__file__+'.calls','a').write(a[1]+'\\n')\n"
        " held=pathlib.Path(__file__+'.hold').exists()\n"
        " while held and not pathlib.Path(__file__+'.release').exists():time.sleep(.02)\n"
        " if a[1]=='favorite':print(json.dumps({'status':'applied','request_id':a[a.index('--request-id')+1],'model':a[a.index('--model')+1],'favorite':a[a.index('--favorite')+1]=='on'}))\n"
        " else:print(json.dumps({'catalog_state':'ready','model_options':[],'models':[{'id':'solo/solo-model','value':'solo/solo-model','label':'Solo model','provider':'solo','provider_label':'Solo fixture','free':False}],'favorites':[]}))\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(second)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(secondDetails).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-left-session");
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    const auto calls=[&]{QFile file(fixture+".calls");auto made=file.open(QIODevice::ReadOnly)?QString::fromUtf8(file.readAll()).split('\n',Qt::SkipEmptyParts):QStringList();made.sort();return made;};
    const auto mark=[&](const QString &suffix,bool on){QFile file(fixture+suffix);if(on)QVERIFY(file.open(QIODevice::WriteOnly));else file.remove();};
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId="first-run";session.conversationId="ses_first";
    session.tracked=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";session.state="running";
    session.cwd="/workspace/project";session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename"};
    auto secondSession=session;secondSession.name=second;secondSession.runId="second-run";secondSession.conversationId="ses_second";
    box.sessions={session,secondSession};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    QSignalSpy answers(client,&HgsClient::openCodeModelsReady),stars(client,&HgsClient::openCodeFavoriteReady);
    auto *open=composer->findChild<QPushButton *>("sessionModelSettings");
    auto *picker=composer->findChild<OpenCodeModelPicker *>("sessionOpenCodeModels");QVERIFY(picker);
    auto *all=picker->findChild<QPushButton *>("openCodeAllModels");
    mark(".hold",true);open->click();QTRY_VERIFY(picker->isVisible());all->click();emit picker->favoriteRequested("fixture/alternate",true);
    QTRY_COMPARE(calls(),QStringList({"favorite","models"}));
    // Leave the session and return to it: the same computer and folder, a new context.
    window.showSession({},second);QTRY_COMPARE(composer->sessionKey(),"\n"+second);settleSelectedInspection(composer,client,{},second,secondDetails);
    window.showSession({},name);QTRY_COMPARE(composer->sessionKey(),"\n"+name);settleSelectedInspection(composer,client,{},name,details);
    mark(".release",true);QTRY_COMPARE(answers.size(),1);QTRY_COMPARE(stars.size(),1);QTest::qWait(100);
    QVERIFY(!picker->showingAll());QVERIFY(!picker->values().contains("solo/solo-model"));QVERIFY(!picker->model("fixture/alternate")->favorite);
    QCOMPARE(calls(),QStringList({"favorite","models"}));
    mark(".hold",false);mark(".release",false);
    open->click();QTRY_VERIFY(picker->isVisible());all->click();QTRY_COMPARE(answers.size(),2);
    QTRY_VERIFY(picker->values().contains("solo/solo-model"));QVERIFY(picker->showingAll());
    QCOMPARE(calls(),QStringList({"favorite","models","models"}));
}

// A selected session that vanishes from the fleet leaves the composer without a session.
// Answers to a read, a star and a failing read made before never show after it returns
// and is selected again with the same computer and folder, and the star's answer reads no
// details. Nothing is read, starred or applied on its own; the next explicit All models
// reads once.
void TestSessionsWindow::openCodeAnswersFromBeforeTheSessionVanishedStayThere()
{
    const QString name="opencode/project/vanishing";
    const QJsonArray options{QJsonObject{{"id","fixture/local"},{"value","fixture/local"},{"label","Local fixture"},{"provider","fixture"},{"provider_label","fixture"}},
        QJsonObject{{"id","fixture/alternate"},{"value","fixture/alternate"},{"label","Alternate fixture"},{"provider","fixture"},{"provider_label","fixture"}}};
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id","vanishing-run"},{"conversation_id","ses_vanishing"},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},{"settings_change_supported",true},
        {"cwd","/workspace/project"},{"settings_model","fixture/local"},{"model_options",options},{"model_options_truncated",false},
        {"allowed_actions",QJsonArray{"rename"}},{"events",QJsonArray{}},{"cursor",0}};
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    // Reads and stars record what they are and can be held; a read can fail.
    const QByteArray answer="elif c=='account' and a[1:2] in (['models'],['favorite']):\n"
        " import pathlib,time;open(__file__+'.calls','a').write(a[1]+'\\n')\n"
        " held=pathlib.Path(__file__+'.hold').exists()\n"
        " while held and not pathlib.Path(__file__+'.release').exists():time.sleep(.02)\n"
        " if a[1]=='favorite':print(json.dumps({'status':'applied','request_id':a[a.index('--request-id')+1],'model':a[a.index('--model')+1],'favorite':a[a.index('--favorite')+1]=='on'}))\n"
        " elif pathlib.Path(__file__+'.fail').exists():sys.exit('The catalog is unavailable')\n"
        " else:print(json.dumps({'catalog_state':'ready','model_options':[],'models':[{'id':'solo/solo-model','value':'solo/solo-model','label':'Solo model','provider':'solo','provider_label':'Solo fixture','free':False}],'favorites':[]}))\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-vanished-session");
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    const auto lines=[&](const QString &suffix){QFile file(fixture+suffix);auto found=file.open(QIODevice::ReadOnly)?QString::fromUtf8(file.readAll()).split('\n',Qt::SkipEmptyParts):QStringList();found.sort();return found;};
    const auto mark=[&](const QString &suffix,bool on){QFile file(fixture+suffix);if(on)QVERIFY(file.open(QIODevice::WriteOnly));else file.remove();};
    SessionInfo session;session.name=name;session.cmd="opencode";session.runId="vanishing-run";session.conversationId="ses_vanishing";
    session.tracked=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";session.state="running";
    session.cwd="/workspace/project";session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename"};
    const auto fleetWith=[&](bool present){auto state=fleet();auto box=state.local();box.sessions=present?QList<SessionInfo>{session}:QList<SessionInfo>{};
        state.setLocal(box,QDateTime::currentMSecsSinceEpoch());return state;};
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(fleetWith(true));window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    QSignalSpy answers(client,&HgsClient::openCodeModelsReady),failures(client,&HgsClient::openCodeModelsFailed),stars(client,&HgsClient::openCodeFavoriteReady);
    QSignalSpy changes(composer,&MessageComposer::settingsRequested);
    auto *open=composer->findChild<QPushButton *>("sessionModelSettings");
    auto *picker=composer->findChild<OpenCodeModelPicker *>("sessionOpenCodeModels");QVERIFY(picker);
    auto *all=picker->findChild<QPushButton *>("openCodeAllModels");auto *notice=picker->findChild<QLabel *>("openCodeModelNotice");
    // The session vanishes, so nothing is selected; it returns and is selected again, its computer and folder unchanged.
    const auto vanishAndReturn=[&]{
        window.setFleet(fleetWith(false));QTRY_VERIFY(composer->sessionKey().isEmpty());
        window.setFleet(fleetWith(true));window.showSession({},name);QTRY_COMPARE(composer->sessionKey(),"\n"+name);
        settleSelectedInspection(composer,client,{},name,details);
    };
    mark(".hold",true);open->click();QTRY_VERIFY(picker->isVisible());all->click();emit picker->favoriteRequested("fixture/alternate",true);
    QTRY_COMPARE(lines(".calls"),QStringList({"favorite","models"}));
    vanishAndReturn();QTest::qWait(100);QTRY_VERIFY(!client->inspectionInFlight({},name));
    QSignalSpy inspections(client,&HgsClient::inspectionLoadingChanged);
    mark(".release",true);QTRY_COMPARE(answers.size(),1);QTRY_COMPARE(stars.size(),1);QTest::qWait(200);
    QVERIFY(!picker->showingAll());QVERIFY(!picker->values().contains("solo/solo-model"));QVERIFY(!picker->model("fixture/alternate")->favorite);
    // The star's answer read no details.
    for(const auto &row:inspections)QVERIFY(!(row[1].toString()==name&&row[4].toBool()));
    // A failing read held across the same vanish and return shows no failure either.
    mark(".release",false);mark(".fail",true);
    open->click();QTRY_VERIFY(picker->isVisible());all->click();QTRY_COMPARE(lines(".calls"),QStringList({"favorite","models","models"}));
    vanishAndReturn();mark(".release",true);QTRY_COMPARE(failures.size(),1);QTest::qWait(100);
    QVERIFY(!picker->showingAll());QVERIFY(notice->text().isEmpty());
    // Nothing was read, starred or applied on its own; the next explicit All models reads once.
    QCOMPARE(lines(".calls"),QStringList({"favorite","models","models"}));
    mark(".hold",false);mark(".release",false);mark(".fail",false);
    open->click();QTRY_VERIFY(picker->isVisible());all->click();QTRY_COMPARE(answers.size(),2);
    QTRY_VERIFY(picker->values().contains("solo/solo-model"));QVERIFY(picker->showingAll());
    QCOMPARE(lines(".calls"),QStringList({"favorite","models","models","models"}));
    QCOMPARE(failures.size(),1);QCOMPARE(stars.size(),1);QCOMPARE(changes.size(),0);
}

void TestSessionsWindow::openCodeRemovesUndeliveredMessageOnlyAfterConfirmation()
{
    const QString name="opencode/project/local",run="open-run",conversation="ses_conversation";
    const QJsonObject undelivered{{"type","UserPromptSubmit"},{"detail","Check the release notes"},{"tool",""},{"at",1791018005.0},{"source","opencode_native"},
        {"agent_id",""},{"message_id","msg_Queued01"},{"reply_id",QJsonValue::Null},{"delivery_state","not_delivered"}};
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},
        {"allowed_actions",QJsonArray{"rename","forget","remove_queued"}},{"events",QJsonArray{undelivered}},{"replace_events",true},{"cursor",0}};
    // A second OpenCode session with its own undelivered message.
    const QString second="opencode/project/second";auto secondDetails=details;secondDetails["name"]=second;
    secondDetails["run_id"]="second-run";secondDetails["conversation_id"]="ses_second";
    auto secondMessage=undelivered;secondMessage["message_id"]="msg_Second01";secondMessage["detail"]="Second session request";
    secondDetails["events"]=QJsonArray{secondMessage};
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer="elif c=='session-action':\n"
        " import pathlib,time;p=json.load(sys.stdin);staged=pathlib.Path(__file__+'.remove.tmp')\n"
        " staged.write_text(json.dumps({'args':sys.argv[1:],'payload':p}));staged.replace(__file__+'.remove')\n"
        " while pathlib.Path(__file__+'.hold').exists():time.sleep(.02)\n"
        " r={'request_id':p['request_id'],'name':a[1],'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'status':'completed'}\n"
        " if pathlib.Path(__file__+'.fail').exists():r.update(status='failed',error='This message is no longer queued')\n"
        " print(json.dumps(r))\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(second)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(secondDetails).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-remove-queued"),removal=fixture+".remove";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.activity="idle";session.phase="idle";session.processState="running";session.runtimeState="live";session.state="running";
    session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename","forget","remove_queued"};
    auto secondSession=session;secondSession.name=second;secondSession.runId="second-run";secondSession.conversationId="ses_second";
    box.sessions={session,secondSession};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    auto *activity=window.findChild<ActivityView *>("mainActivity");QVERIFY(activity);
    WAIT_VERIFY(activity->plainText().contains("Not delivered"));WAIT_VERIFY(!activity->renderPending());
    QString remove;
    for(auto block=activity->browser()->document()->begin();block.isValid();block=block.next())
        for(auto it=block.begin();!it.atEnd();++it)if(it.fragment().charFormat().anchorHref().startsWith("hgs-message:"))remove=it.fragment().charFormat().anchorHref();
    QVERIFY(!remove.isEmpty());
    const auto click=[&]{QMetaObject::invokeMethod(activity->browser(),"anchorClicked",Qt::DirectConnection,Q_ARG(QUrl,QUrl(remove)));};
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("removeQueuedConfirm");QVERIFY(dialog);asked=true;
        // Read everything, then close: a failed check must not leave the dialog open.
        const auto title=dialog->windowTitle(),shown=dialog->text();
        const bool cancelDefault=dialog->defaultButton()==dialog->button(QMessageBox::Cancel)&&dialog->escapeButton()==dialog->button(QMessageBox::Cancel);
        bool destructive=false;for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole)destructive=button->text()=="Remove";
        dialog->button(QMessageBox::Cancel)->click();
#ifdef Q_OS_MACOS
        // Qt ignores a message box's window title on macOS (QMessageBox::setWindowTitle).
        QVERIFY2(title.isEmpty(),qPrintable(title));
#else
        QCOMPARE(title,QString("Remove queued message"));
#endif
        QCOMPARE(shown,QString("Remove this queued message? It is still waiting in OpenCode's queue and runs first with your next message unless you remove it."));
        QVERIFY(cancelDefault);QVERIFY(destructive);
    });
    click();QVERIFY(asked);WAIT_SETTLED();QVERIFY(!QFileInfo::exists(removal));QCOMPARE(finished.size(),0);
    // A message that is not shown as not delivered asks nothing.
    bool prompted=false;
    QTimer::singleShot(0,&window,[&]{if(auto *dialog=window.findChild<QMessageBox *>("removeQueuedConfirm")){prompted=true;dialog->reject();}});
    activity->messageActionRequested("msg_Unknown01","remove_queued");WAIT_SETTLED();QVERIFY(!prompted);
    asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("removeQueuedConfirm");QVERIFY(dialog);asked=true;
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}
        QFAIL("Missing Remove");
    });
    click();QVERIFY(asked);WAIT_VERIFY(QFileInfo::exists(removal));
    QFile sent(removal);QVERIFY(sent.open(QIODevice::ReadOnly));const auto call=QJsonDocument::fromJson(sent.readAll()).object();
    QCOMPARE(call["args"].toArray(),QJsonArray({"session-action",name,"--json"}));
    const auto payload=call["payload"].toObject();
    QCOMPARE(payload["action"].toString(),QString("remove_queued"));QCOMPARE(payload["message_id"].toString(),QString("msg_Queued01"));
    QCOMPARE(payload["expected_run_id"].toString(),run);QCOMPARE(payload["expected_conversation_id"].toString(),conversation);
    WAIT_COMPARE(finished.size(), 1);QVERIFY2(finished[0][1].toBool(),qPrintable(finished[0][3].toString()));
    sent.close();QVERIFY(QFile::remove(removal));
    // While a removal runs, Remove is shown as in progress and cannot be clicked again.
    QVERIFY(FakeCli::replace(fixture+".hold",{}));QVERIFY(FakeCli::replace(fixture+".fail",{}));
    WAIT_VERIFY(!activity->renderPending());
    remove.clear();
    for(auto block=activity->browser()->document()->begin();block.isValid();block=block.next())
        for(auto it=block.begin();!it.atEnd();++it)if(it.fragment().charFormat().anchorHref().startsWith("hgs-message:"))remove=it.fragment().charFormat().anchorHref();
    QVERIFY(!remove.isEmpty());asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("removeQueuedConfirm");QVERIFY(dialog);asked=true;
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}
    });
    click();QVERIFY(asked);WAIT_VERIFY(QFileInfo::exists(removal));
    WAIT_VERIFY(activity->plainText().contains("Removing…"));WAIT_VERIFY(!activity->renderPending());
    bool linked=false;
    for(auto block=activity->browser()->document()->begin();block.isValid();block=block.next())
        for(auto it=block.begin();!it.atEnd();++it)linked|=it.fragment().charFormat().anchorHref().startsWith("hgs-message:");
    QVERIFY(!linked);
    // Another OpenCode session's Remove is muted too while this removal runs.
    window.showSession({},second);WAIT_COMPARE(composer->sessionKey(), "\n"+second);
    WAIT_VERIFY(activity->plainText().contains("Second session request"));WAIT_VERIFY(!activity->renderPending());
    QVERIFY(activity->plainText().contains("Remove"));QVERIFY(!activity->plainText().contains("Removing…"));
    linked=false;
    for(auto block=activity->browser()->document()->begin();block.isValid();block=block.next())
        for(auto it=block.begin();!it.atEnd();++it)linked|=it.fragment().charFormat().anchorHref().startsWith("hgs-message:");
    QVERIFY(!linked);
    // A failure that arrives after another session was selected names its session.
    window.showSession("mac","claude/infra/review");WAIT_COMPARE(composer->sessionKey(), QString("mac\nclaude/infra/review"));QVERIFY(QFile::remove(fixture+".hold"));
    WAIT_COMPARE(finished.size(), 2);QVERIFY(!finished[1][1].toBool());
    const auto noticed=[&](const QString &text){for(auto *label:window.findChildren<QLabel *>())if(label->text().contains(name)&&label->text().contains(text))return true;return false;};
    WAIT_VERIFY(noticed("This message is no longer queued"));
}

// OpenCode 2 has no process for Zerus to stop: while the session allows forget
// and not terminate, Forget removes only the Zerus binding, even while it is
// running. Other agents keep Terminate.
void TestSessionsWindow::openCodeForgetsOnlyItsBindingWhileRunning()
{
    const QString name="opencode/project/local",run="open-run",conversation="ses_conversation";
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","idle"},{"activity","idle"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},
        {"allowed_actions",QJsonArray{"rename","forget"}},{"action_reasons",QJsonObject{{"terminate","Use Stop for this OpenCode session"}}},
        {"events",QJsonArray{}},{"replace_events",true},{"cursor",0}};
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer="elif c=='session-action':\n"
        " import pathlib;p=json.load(sys.stdin);staged=pathlib.Path(__file__+'.forget.tmp')\n"
        " staged.write_text(json.dumps({'args':sys.argv[1:],'payload':p}));staged.replace(__file__+'.forget')\n"
        " r={'request_id':p['request_id'],'name':a[1],'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'status':'completed'}\n"
        " outcome=pathlib.Path(__file__+'.outcome')\n"
        " if outcome.exists():r.update(status=outcome.read_text(),error='This action is unavailable for the exact OpenCode session')\n"
        " print(json.dumps(r))\n"
        "elif c in ('kill','terminate'):open(__file__+'.ended','a').write(json.dumps(sys.argv[1:])+'\\n')\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-forget-binding"),forgotten=fixture+".forget",ended=fixture+".ended";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.project="project";session.tag="local";
    session.runId=run;session.conversationId=conversation;session.tracked=true;session.activity="idle";session.phase="idle";
    session.processState="running";session.runtimeState="live";session.state="running";session.terminalSupported=false;
    session.actionsReported=true;session.allowedActions={"rename","forget"};
    box.sessions.append(session);state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();
    auto *forget=window.findChild<QAction *>("forgetSessionAction");auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(forget);QVERIFY(list);
    const auto contextEntry=[&]{
        QTest::keyClick(list,Qt::Key_Menu);auto *menu=window.findChild<QMenu *>("sessionContextMenu");
        auto *action=menu?menu->findChild<QAction *>("contextTerminateSession"):nullptr;
        const auto result=action?QPair<QString,bool>{action->text(),action->isEnabled()}:QPair<QString,bool>{};if(menu)menu->hide();return result;
    };
    // A running Codex session still terminates first.
    window.showSession({},"codex/hgs/dashboard");QTRY_COMPARE(forget->text(),QString("Terminate session…"));
    QCOMPARE(contextEntry(),(QPair<QString,bool>{"Terminate session…",true}));
    window.showSession({},name);auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    QTRY_COMPARE(forget->text(),QString("Forget saved session…"));QVERIFY(forget->isEnabled());QVERIFY(!forget->toolTip().contains("Pause"));
    QCOMPARE(contextEntry(),(QPair<QString,bool>{"Forget saved session…",true}));
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;
        QVERIFY2(dialog->text().contains("stays in OpenCode"),qPrintable(dialog->text()));QVERIFY(dialog->text().contains(name));
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));dialog->button(QMessageBox::Cancel)->click();
    });
    forget->trigger();QVERIFY(asked);QTest::qWait(150);QVERIFY(!QFileInfo::exists(forgotten));QVERIFY(!QFileInfo::exists(ended));
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;
        QCOMPARE(dialog->button(QMessageBox::Yes)->text(),QString("Forget saved session"));dialog->button(QMessageBox::Yes)->click();
    });
    forget->trigger();QVERIFY(asked);QTRY_VERIFY(QFileInfo::exists(forgotten));
    QFile sent(forgotten);QVERIFY(sent.open(QIODevice::ReadOnly));const auto call=QJsonDocument::fromJson(sent.readAll()).object();
    QCOMPARE(call["args"].toArray(),QJsonArray({"session-action",name,"--json"}));
    const auto payload=call["payload"].toObject();
    QCOMPARE(payload.keys(),QStringList({"action","expected_conversation_id","expected_run_id","request_id"}));
    QCOMPARE(payload["action"].toString(),QString("forget"));
    QCOMPARE(payload["expected_run_id"].toString(),run);QCOMPARE(payload["expected_conversation_id"].toString(),conversation);
    QTRY_COMPARE(finished.size(),1);QVERIFY2(finished[0][1].toBool(),qPrintable(finished[0][3].toString()));
    QVERIFY(!QFileInfo::exists(ended));sent.close();
    // A failed or uncertain receipt ends the pending state and shows its error.
    const auto noticed=[&](const QString &text){for(auto *label:window.findChildren<QLabel *>())if(label->isVisible()&&label->text().contains(text))return true;return false;};
    for(const auto *outcome:{"failed","uncertain"}) {
        QVERIFY(FakeCli::replace(fixture+".outcome",outcome));QVERIFY(QFile::remove(forgotten));finished.clear();
        QTRY_VERIFY(forget->isEnabled());asked=false;
        QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;dialog->button(QMessageBox::Yes)->click();});
        forget->trigger();QVERIFY(asked);QTRY_COMPARE(finished.size(),1);QVERIFY2(!finished[0][1].toBool(),outcome);
        QTRY_VERIFY2(noticed("This action is unavailable for the exact OpenCode session"),outcome);
        QTRY_VERIFY2(forget->isEnabled(),outcome);
    }
    QVERIFY(!QFileInfo::exists(ended));
}

// Without a known run identity (an old binding), Forget keeps the CLI's
// own forget path: `hgs kill` only forgets an OpenCode binding.
void TestSessionsWindow::openCodeForgetWithoutRunIdentityUsesKill()
{
    const QString name="opencode/project/legacy";
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","stopped"},{"phase","idle"},{"activity","idle"},
        {"process_state","stopped"},{"runtime_state","stopped"},{"terminal_supported",false},{"allowed_actions",QJsonArray{"forget"}},
        {"events",QJsonArray{}},{"replace_events",true},{"cursor",0}};
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer="elif c=='session-action':open(__file__+'.scoped','a').write(json.dumps(sys.argv[1:])+'\\n')\n"
        "elif c in ('kill','terminate'):open(__file__+'.ended','a').write(json.dumps(sys.argv[1:])+'\\n')\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-forget-legacy"),ended=fixture+".ended";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.project="project";session.tag="legacy";
    session.tracked=true;session.state="stopped";session.activity="idle";session.phase="idle";session.terminalSupported=false;
    session.actionsReported=true;session.allowedActions={"forget"};
    box.sessions.append(session);state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    auto *forget=window.findChild<QAction *>("forgetSessionAction");QTRY_VERIFY(forget->isEnabled());
    QCOMPARE(forget->text(),QString("Forget saved session…"));
    bool asked=false;QSignalSpy writes(client,&HgsClient::writeDone);
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;
        QVERIFY(dialog->text().contains("stays in OpenCode"));dialog->button(QMessageBox::Yes)->click();
    });
    forget->trigger();QVERIFY(asked);QTRY_COMPARE(writes.size(),1);QVERIFY(writes[0][1].toBool());
    QFile calls(ended);QVERIFY(calls.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(calls.readAll().trimmed()).array(),QJsonArray({"kill",name}));
    QVERIFY(!QFileInfo::exists(fixture+".scoped"));
}

// A multi-selection forgets running OpenCode 2 sessions like a single Forget:
// after one confirmation, each forgets only its own Zerus binding with its own
// run and conversation, never through kill. Other rows keep their own Forget,
// and an OpenCode row without both identities is left as it is.
void TestSessionsWindow::openCodeSelectionForgetsEachBinding()
{
    auto state=openCodeSelectionFleet();auto box=state.local();
    const QString first=box.sessions[0].name,second=box.sessions[1].name,stopped=box.sessions[2].name;
    auto unknownRun=box.sessions[0];unknownRun.name="opencode/project/legacy";unknownRun.tag="legacy";unknownRun.runId.clear();
    auto unknownConversation=box.sessions[1];unknownConversation.name="opencode/project/unbound";unknownConversation.tag="unbound";unknownConversation.conversationId.clear();
    box.sessions<<unknownRun<<unknownConversation;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    const auto fixture=openCodeSelectionFixture("opencode-selection-forget");QVERIFY(!fixture.isEmpty());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.resize(1100,850);window.setFleet(state);window.show();
    window.showSession({},stopped);
    auto *list=window.findChild<SessionList *>("sessionList");auto *client=window.findChild<HgsClient *>();QVERIFY(list);QVERIFY(client);
    const auto toggle=[&](const QString &name){
        QListWidgetItem *item=nullptr;
        for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=='\n'+name){item=list->item(i);break;}
        QVERIFY(item);list->scrollToItem(item);QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(item).center());
    };
    const auto chosen=[&]{QSet<QString> result;for(const auto *item:list->selectedItems())result.insert(item->data(SessionRoles::Key).toString().section('\n',1,1));return result;};
    toggle(first);toggle(second);toggle(stopped);QCOMPARE(chosen(),(QSet<QString>{first,second}));
    auto *forget=window.findChild<QAction *>("selectiontrash"),*terminate=window.findChild<QAction *>("selectionstop");QVERIFY(forget);QVERIFY(terminate);
    QTRY_COMPARE(forget->text(),QString("Forget (2)"));QVERIFY(forget->isEnabled());QVERIFY(!terminate->isVisible());
    // OpenCode sessions alone: the question is only about their bindings.
    int asked=0;QString question;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());if(!dialog)return;++asked;
        question=dialog->text();dialog->button(QMessageBox::Cancel)->click();
    });
    forget->trigger();QCOMPARE(asked,1);
    QCOMPARE(question,QString("Forget 2 selected sessions?\n\nOpenCode sessions: only Zerus forgets them; their conversations stay in OpenCode."));
    toggle(stopped);toggle(unknownRun.name);toggle(unknownConversation.name);
    QCOMPARE(chosen(),(QSet<QString>{first,second,stopped,unknownRun.name,unknownConversation.name}));
    const auto control=[&](const char *name){auto *action=window.findChild<QAction *>(name);return action?action->text():QString();};
    QTRY_COMPARE(forget->text(),QString("Forget (3)"));QVERIFY(forget->isEnabled());
    // OpenCode rows never terminate; the other actions keep their counts.
    QCOMPARE(terminate->text(),QString("Terminate (0)"));QVERIFY(!terminate->isVisible());
    QCOMPARE(control("selectionpause"),QString("Pause (0)"));QCOMPARE(control("selectionplay"),QString("Resume / restore (1)"));
    QCOMPARE(control("selectionarchived"),QString("Move to archive (1)"));
    const auto calls=[&](const QString &suffix){
        QList<QJsonValue> result;QFile file(fixture+suffix);
        if(file.open(QIODevice::ReadOnly))for(const auto &line:file.readAll().split('\n'))if(!line.isEmpty()) {
            const auto document=QJsonDocument::fromJson(line);result<<(document.isArray()?QJsonValue(document.array()):QJsonValue(document.object()));
        }
        return result;
    };
    auto *notice=window.findChild<QLabel *>("notice");QVERIFY(notice);
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);QSignalSpy writes(client,&HgsClient::writeDone);
    // Escape cancels the only confirmation, and nothing is sent. The dialog
    // closes before the checks, so a failed check cannot leave it open.
    asked=0;QString objectName,text,detailed;bool cancelDefault=false,cancelEscape=false;QStringList destructive;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());if(!dialog)return;++asked;
        objectName=dialog->objectName();text=dialog->text();detailed=dialog->detailedText();
        cancelDefault=dialog->defaultButton()==dialog->button(QMessageBox::Cancel);cancelEscape=dialog->escapeButton()==dialog->button(QMessageBox::Cancel);
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole)destructive<<button->text();
        QTest::keyClick(dialog,Qt::Key_Escape);
    });
    forget->trigger();QCOMPARE(asked,1);QTest::qWait(150);
    QCOMPARE(objectName,QString("confirmSelectionAction"));
    QVERIFY2(text.startsWith("Forget 3 selected sessions?"),qPrintable(text));
    QVERIFY2(text.contains("OpenCode sessions: only Zerus forgets them; their conversations stay in OpenCode."),qPrintable(text));
    QVERIFY2(text.contains("These saved or archived entries are removed. Other live sessions and native conversation history are kept."),qPrintable(text));
    QVERIFY2(!text.contains("Only these"),qPrintable(text));
    QVERIFY2(!text.contains("Live sessions and native"),qPrintable(text));
    QVERIFY2(text.contains("2 unavailable or inapplicable sessions will be left as they are."),qPrintable(text));
    QStringList listed=detailed.split('\n');for(auto &row:listed)row=row.section(" / ",1);
    QCOMPARE(QSet<QString>(listed.cbegin(),listed.cend()),(QSet<QString>{first,second,stopped}));
    QVERIFY(cancelDefault);QVERIFY(cancelEscape);QCOMPARE(destructive,QStringList{"Forget 3 sessions"});
    QVERIFY(calls(".forget").isEmpty());QVERIFY(calls(".ended").isEmpty());QCOMPARE(finished.size(),0);QCOMPARE(writes.size(),0);
    // Confirmed once, each binding goes with its own identity and request.
    int dialogs=0;QTimer confirmations;confirmations.setInterval(10);
    connect(&confirmations,&QTimer::timeout,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());if(!dialog)return;++dialogs;
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}
        dialog->reject();
    });
    QTRY_VERIFY(forget->isEnabled());confirmations.start();forget->trigger();
    QTRY_VERIFY_WITH_TIMEOUT(notice->text().contains("Forget: 3 of 3 completed."),10000);confirmations.stop();
    QCOMPARE(dialogs,1);QVERIFY2(!notice->text().contains("failed"),qPrintable(notice->text()));
    QVERIFY(!notice->text().contains("Zerus forgot"));
    const auto forgotten=calls(".forget");QCOMPARE(forgotten.size(),2);
    const QMap<QString,QPair<QString,QString>> identities{{first,{"run-first","ses_first"}},{second,{"run-second","ses_second"}}};
    QSet<QString> names,requests;
    for(const auto &value:forgotten) {
        const auto call=value.toObject();const auto name=call["args"].toArray().at(1).toString();names.insert(name);
        QCOMPARE(call["args"].toArray(),QJsonArray({"session-action",name,"--json"}));
        const auto payload=call["payload"].toObject();
        QCOMPARE(payload.keys(),QStringList({"action","expected_conversation_id","expected_run_id","request_id"}));
        QCOMPARE(payload["action"].toString(),QString("forget"));
        QVERIFY(identities.contains(name));
        QCOMPARE(payload["expected_run_id"].toString(),identities[name].first);
        QCOMPARE(payload["expected_conversation_id"].toString(),identities[name].second);
        QVERIFY(!payload["request_id"].toString().isEmpty());requests.insert(payload["request_id"].toString());
    }
    QCOMPARE(names,(QSet<QString>{first,second}));QCOMPARE(requests.size(),2);
    const auto ended=calls(".ended");QCOMPARE(ended.size(),1);QCOMPARE(ended[0].toArray(),QJsonArray({"kill",stopped}));
    QCOMPARE(finished.size(),2);for(const auto &row:finished)QVERIFY2(row[1].toBool(),qPrintable(row[3].toString()));
}

void TestSessionsWindow::openCodeSelectionForgetRechecksEachRow_data()
{
    QTest::addColumn<QString>("change");
    for(const auto *change:{"changed-run","failed","uncertain"})QTest::newRow(change)<<QString::fromLatin1(change);
}

// Each row is checked again just before it goes: a row whose run changed after
// the confirmation is skipped as changed. A refused or unconfirmed receipt
// counts as failed with its reason, and nothing is retried.
void TestSessionsWindow::openCodeSelectionForgetRechecksEachRow()
{
    QFETCH(QString,change);
    auto state=openCodeSelectionFleet();auto box=state.local();
    const QString first=box.sessions[0].name,second=box.sessions[1].name,stopped=box.sessions[2].name;
    const auto fixture=openCodeSelectionFixture("opencode-selection-"+change);QVERIFY(!fixture.isEmpty());
    if(change!="changed-run")QVERIFY(FakeCli::replace(fixture+".outcome",(second+'\n'+change).toUtf8()));
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.resize(1100,850);window.setFleet(state);window.show();
    window.showSession({},stopped);
    auto *list=window.findChild<SessionList *>("sessionList");auto *client=window.findChild<HgsClient *>();QVERIFY(list);QVERIFY(client);
    for(const auto &name:{first,second}) {
        QListWidgetItem *item=nullptr;
        for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=='\n'+name){item=list->item(i);break;}
        QVERIFY(item);list->scrollToItem(item);QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(item).center());
    }
    QCOMPARE(list->selectedItems().size(),3);
    auto *forget=window.findChild<QAction *>("selectiontrash");QVERIFY(forget);QTRY_COMPARE(forget->text(),QString("Forget (3)"));QVERIFY(forget->isEnabled());
    auto *notice=window.findChild<QLabel *>("notice");QVERIFY(notice);
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());QVERIFY(dialog);asked=true;
        // OpenCode replaced the second session's run while the question was open.
        if(change=="changed-run"){box.sessions[1].runId="run-replacement";state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);}
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();break;}
    });
    forget->trigger();QVERIFY(asked);
    QTRY_VERIFY_WITH_TIMEOUT(notice->text().contains("Forget: 2 of 3 completed. 1 failed or changed; refresh before retrying."),10000);
    QVERIFY(notice->property("error").toBool());
    if(change=="changed-run")QVERIFY2(notice->text().contains(second+": session changed or is unavailable"),qPrintable(notice->text()));
    else QVERIFY2(notice->text().contains("forget "+second+": ")&&notice->text().contains("This action is unavailable for the exact OpenCode session"),qPrintable(notice->text()));
    QTest::qWait(200);
    QFile file(fixture+".forget");QVERIFY(file.open(QIODevice::ReadOnly));
    QStringList forgotten;for(const auto &line:file.readAll().split('\n'))if(!line.isEmpty()) {
        const auto call=QJsonDocument::fromJson(line).object();forgotten<<call["args"].toArray().at(1).toString();
        QCOMPARE(call["payload"].toObject()["expected_run_id"].toString(),call["args"].toArray().at(1).toString()==first?QString("run-first"):QString("run-second"));
    }
    forgotten.sort();
    // Exactly one request per row that went; a refused one is not sent again.
    QCOMPARE(forgotten,(change=="changed-run"?QStringList{first}:QStringList{first,second}));
    QCOMPARE(finished.size(),forgotten.size());
    QFile ended(fixture+".ended");QVERIFY(ended.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(ended.readAll().trimmed()).array(),QJsonArray({"kill",stopped}));
}

// The binding-only Forget of a batch also covers a stopped OpenCode session
// with a known identity (never kill) and a session on another machine, whose
// request goes to that machine.
void TestSessionsWindow::openCodeSelectionForgetsStoppedAndRemoteBindings()
{
    auto state=openCodeSelectionFleet();auto box=state.local();
    auto stopped=box.sessions[0];stopped.name="opencode/project/stopped";stopped.tag="stopped";stopped.runId="run-stopped";stopped.conversationId="ses_stopped";
    stopped.state="stopped";stopped.processState="exited";stopped.runtimeState="stopped";stopped.allowedActions={"forget"};
    box.sessions={stopped};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    auto peer=*state.peer("mac");auto remote=openCodeSelectionFleet().local().sessions[1];
    remote.name="opencode/infra/remote";remote.project="infra";remote.tag="remote";remote.runId="run-remote";remote.conversationId="ses_remote";
    peer.sessions<<remote;state.setPeer(peer,QDateTime::currentMSecsSinceEpoch());
    const auto fixture=openCodeSelectionFixture("opencode-selection-stopped-remote");QVERIFY(!fixture.isEmpty());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.resize(1100,850);window.setFleet(state);window.show();
    window.showSession({},stopped.name);
    auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(list);
    QListWidgetItem *item=nullptr;
    for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=="mac\n"+remote.name){item=list->item(i);break;}
    QVERIFY(item);list->scrollToItem(item);QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(item).center());
    QCOMPARE(list->selectedItems().size(),2);
    auto *forget=window.findChild<QAction *>("selectiontrash");QVERIFY(forget);QTRY_COMPARE(forget->text(),QString("Forget (2)"));QVERIFY(forget->isEnabled());
    QCOMPARE(window.findChild<QAction *>("selectionstop")->text(),QString("Terminate (0)"));
    bool asked=false;QString question;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());if(!dialog)return;asked=true;question=dialog->text();
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();break;}
    });
    forget->trigger();QVERIFY(asked);
    QCOMPARE(question,QString("Forget 2 selected sessions?\n\nOpenCode sessions: only Zerus forgets them; their conversations stay in OpenCode."));
    auto *notice=window.findChild<QLabel *>("notice");QVERIFY(notice);
    QTRY_VERIFY_WITH_TIMEOUT(notice->text().contains("Forget: 2 of 2 completed."),10000);QVERIFY2(!notice->text().contains("failed"),qPrintable(notice->text()));
    QFile file(fixture+".forget");QVERIFY(file.open(QIODevice::ReadOnly));
    QMap<QString,QJsonObject> calls;
    for(const auto &line:file.readAll().split('\n'))if(!line.isEmpty()) {
        const auto call=QJsonDocument::fromJson(line).object();const auto args=call["args"].toArray();
        QVERIFY(!calls.contains(args.at(args.size()-2).toString()));calls.insert(args.at(args.size()-2).toString(),call);
    }
    QCOMPARE(calls.keys(),QStringList({remote.name,stopped.name}));
    QCOMPARE(calls[stopped.name]["args"].toArray(),QJsonArray({"session-action",stopped.name,"--json"}));
    QCOMPARE(calls[remote.name]["args"].toArray(),QJsonArray({"@mac","session-action",remote.name,"--json"}));
    for(const auto &session:{stopped,remote}) {
        const auto payload=calls[session.name]["payload"].toObject();
        QCOMPARE(payload["action"].toString(),QString("forget"));
        QCOMPARE(payload["expected_run_id"].toString(),session.runId);QCOMPARE(payload["expected_conversation_id"].toString(),session.conversationId);
    }
    QVERIFY(!QFileInfo::exists(fixture+".ended"));
}

void TestSessionsWindow::openCodeForgetFinishesWhenTheCliCannotStart_data()
{
    QTest::addColumn<bool>("refusedAtStart");
    // QProcess refuses an empty program inside start() on every platform, as
    // Windows does for a missing executable; elsewhere a missing file fails later.
    QTest::newRow("refused-at-start")<<true;QTest::newRow("missing")<<false;
}

// An hgs that cannot start still finishes the request: a batch counts the
// failure and becomes available again, and a single Forget ends its pending state.
void TestSessionsWindow::openCodeForgetFinishesWhenTheCliCannotStart()
{
    QFETCH(bool,refusedAtStart);
    auto state=openCodeSelectionFleet();auto box=state.local();
    const QString first=box.sessions[0].name,second=box.sessions[1].name,stopped=box.sessions[2].name;
    const auto fixture=openCodeSelectionFixture(QStringLiteral("opencode-unstartable-%1").arg(refusedAtStart));QVERIFY(!fixture.isEmpty());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.resize(1100,850);window.setFleet(state);window.show();
    window.showSession({},stopped);
    auto *list=window.findChild<SessionList *>("sessionList");auto *client=window.findChild<HgsClient *>();QVERIFY(list);QVERIFY(client);
    for(const auto &name:{first,second}) {
        QListWidgetItem *item=nullptr;
        for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=='\n'+name){item=list->item(i);break;}
        QVERIFY(item);list->scrollToItem(item);QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(item).center());
    }
    auto *forget=window.findChild<QAction *>("selectiontrash");QVERIFY(forget);QTRY_COMPARE(forget->text(),QString("Forget (3)"));QVERIFY(forget->isEnabled());
    auto *notice=window.findChild<QLabel *>("notice");QVERIFY(notice);
    client->setExecutableForTests(refusedAtStart?QString():m_dir->filePath("missing-hgs"));
    QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());QVERIFY(dialog);asked=true;
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();break;}
    });
    forget->trigger();QVERIFY(asked);
    QTRY_VERIFY2_WITH_TIMEOUT(notice->text().contains("Forget: 0 of 3 completed. 3 failed or changed; refresh before retrying."),qPrintable(notice->text()),10000);
    QCOMPARE(finished.size(),2);for(const auto &row:finished)QVERIFY(!row[1].toBool());
    QTRY_VERIFY(forget->isEnabled());QCOMPARE(forget->text(),QString("Forget (3)"));
    // The single Forget of one of them ends its pending state with the error.
    list->clearBulkSelection();window.showSession({},first);
    auto *single=window.findChild<QAction *>("forgetSessionAction");QVERIFY(single);
    QTRY_COMPARE(single->text(),QString("Forget saved session…"));QTRY_VERIFY(single->isEnabled());
    finished.clear();asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;dialog->button(QMessageBox::Yes)->click();
    });
    single->trigger();QVERIFY(asked);
    QTRY_COMPARE(finished.size(),1);QVERIFY(!finished[0][1].toBool());
    QTRY_VERIFY(single->isEnabled());QVERIFY(notice->property("error").toBool());
    QVERIFY2(!notice->text().contains("Zerus forgot"),qPrintable(notice->text()));
    // Nothing is still outstanding: Forget asks again.
    asked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(dialog);asked=true;dialog->button(QMessageBox::Cancel)->click();
    });
    single->trigger();QVERIFY(asked);
    QVERIFY(!QFileInfo::exists(fixture+".forget"));QVERIFY(!QFileInfo::exists(fixture+".ended"));
}

// hgs empties an OpenCode form that is too large to show. Its card still
// appears with the reason instead of an unexplained "Needs attention".
void TestSessionsWindow::openCodeOversizedFormShowsItsReason()
{
    const QString name="opencode/project/forms",run="forms-run",conversation="ses_forms";
    const QJsonObject card{{"question_id","que_large"},{"question_hash","large-hash"},{"run_id",run},{"conversation_id",conversation},
        {"kind","opencode_form"},{"status","pending"},{"source","opencode_native"},{"answer_transport","native_opencode"},{"agent_id","main"},
        {"can_answer",false},{"answerable",false},{"approval",false},{"questions",QJsonArray{}},{"unavailable_reason","Answer this form in OpenCode"}};
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","running"},{"run_id",run},{"conversation_id",conversation},
        {"phase","input"},{"activity","attention"},{"process_state","running"},{"runtime_state","live"},{"terminal_supported",false},
        {"allowed_actions",QJsonArray{"rename","forget"}},{"pending_questions",QJsonArray{card}},{"attention_id","opencode_question:que_large"},
        {"events",QJsonArray{}},{"replace_events",true},{"cursor",0}};
    const auto fake=programInspecting("opencode-large-form",details);QVERIFY(!fake.isEmpty());
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.activity="attention";session.phase="input";session.processState="running";session.runtimeState="live";session.state="running";
    session.terminalSupported=false;session.actionsReported=true;session.allowedActions={"rename","forget"};
    box.sessions.append(session);state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(fake);keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    auto *question=window.findChild<QuestionCard *>("questionCard");QVERIFY(question);QTRY_VERIFY(question->isVisible());
    QTRY_COMPARE(question->findChild<QLabel *>("questionStatus")->text(),QString("Answer this form in OpenCode"));
    QVERIFY(question->findChild<QPushButton *>("submitQuestionAnswer")->isHidden());
}

// A stopped OpenCode service is started only by an explicit action on the
// session, as in Accounts and New session.
void TestSessionsWindow::openCodeStoppedServiceOffersExplicitStart()
{
    const QString name="opencode/project/stopped",run="stopped-run",conversation="ses_stopped";
    const QJsonObject details{{"name",name},{"cmd","opencode"},{"tracked",true},{"state","unknown"},{"run_id",run},{"conversation_id",conversation},
        {"phase","unknown"},{"activity","unknown"},{"process_state","unknown"},{"runtime_state","unknown"},{"terminal_supported",false},
        {"allowed_actions",QJsonArray{"forget"}},{"inspection_unavailable",true},{"inspection_reason","OpenCode is not running on fixture"},
        {"activity_detail","OpenCode is not running on fixture"},{"cursor",0}};
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer="elif c=='account' and a[1:2]==['start']:\n"
        " import time\n"
        " with open(__file__+'.started','a') as started:\n"
        "  time.sleep(0.2)\n"
        "  started.write(json.dumps(sys.argv[1:])+'\\n')\n"
        " print(json.dumps({'model_options':[],'settings_model':'','catalog_state':'ready','catalog_reason':'','catalog_at':1791018000}))\n"
        "elif c=='inspect' and a[1:2]==["+FakeCli::pythonString(name)+"]:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact)))+")\n";
    const auto fixture=m_dir->filePath("opencode-stopped-service"),started=fixture+".started";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto box=state.local();SessionInfo session;session.name=name;session.cmd="opencode";session.runId=run;session.conversationId=conversation;
    session.tracked=true;session.state="unknown";session.activity="unknown";session.terminalSupported=false;
    session.actionsReported=true;session.allowedActions={"forget"};
    box.sessions.append(session);state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},name);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *client=window.findChild<HgsClient *>();
    settleSelectedInspection(composer,client,{},name,details);
    auto *banner=window.findChild<QWidget *>("openCodeServiceBanner");auto *start=window.findChild<QPushButton *>("openCodeServiceStart");
    QVERIFY(banner);QVERIFY(start);QTRY_VERIFY(banner->isVisible());
    QCOMPARE(start->text(),QString("Start OpenCode on arch"));QVERIFY(start->isEnabled());
    bool reason=false;for(auto *label:banner->findChildren<QLabel *>())reason|=label->text().contains("OpenCode is not running on fixture");QVERIFY(reason);
    // Showing the session never starts anything.
    QTest::qWait(100);QVERIFY(!QFileInfo::exists(started));
    // Offline, the action waits for the computer.
    auto offline=box;offline.ok=false;state.setLocal(offline,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QTRY_VERIFY(!start->isEnabled());start->click();QTest::qWait(100);QVERIFY(!QFileInfo::exists(started));
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QTRY_VERIFY(start->isEnabled());
    const auto noticed=[&](const QString &text){for(auto *label:window.findChildren<QLabel *>())if(label->isVisible()&&label->text().contains(text))return true;return false;};
    start->click();
    // File creation precedes the fixture's write; completion proves it is closed.
    QTRY_VERIFY(noticed("OpenCode started on arch."));
    QFile calls(started);QVERIFY(calls.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(calls.readAll().trimmed()).array(),QJsonArray({"account","start","native-opencode","--json"}));
    // Other OpenCode states never show it.
    auto running=details;running["state"]="running";running.remove("inspection_unavailable");running.remove("inspection_reason");
    client->inspectionReady({},name,running);QTRY_VERIFY(!banner->isVisible());
}

void TestSessionsWindow::machineBatchCountsExcludeNativeOpenCodeSessions()
{
    BoxState box;box.host="fixture";box.ok=true;
    SessionInfo terminal;terminal.name="codex/project/terminal";terminal.cmd="codex";terminal.resumable=true;terminal.activity="idle";
    auto api=terminal;api.name="opencode/project/api";api.cmd="opencode";api.activity="busy";
    auto saved=api;saved.name="opencode/project/saved";saved.state="paused";saved.activity="idle";
    box.sessions={terminal,api,saved};FleetState state;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);
    auto *menu=window.findChild<QPushButton *>("batchActions")->menu();QMetaObject::invokeMethod(menu,"aboutToShow");
    auto *machine=menu->actions().first()->menu();QVERIFY(machine);
    const auto action=[&](const QString &text){for(auto *entry:machine->actions())if(entry->text()==text)return entry;return static_cast<QAction *>(nullptr);};
    auto *pause=action("Pause all (1)"),*resume=action("Resume all in background (0)");QVERIFY(pause);QVERIFY(resume);
    QVERIFY(pause->isEnabled());QVERIFY(!resume->isEnabled());QVERIFY(pause->toolTip().contains("OpenCode sessions are excluded"));
    box.sessions={api,saved};state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QMetaObject::invokeMethod(menu,"aboutToShow");
    machine=menu->actions().first()->menu();pause=action("Pause all (0)");resume=action("Resume all in background (0)");
    QVERIFY(pause);QVERIFY(resume);QVERIFY(!pause->isEnabled());QVERIFY(!resume->isEnabled());
}

void TestSessionsWindow::nativeSignInPreservesDraftAndUnblocks()
{
    FakeNativeBrowsers plugin; NativeBrowserPlugin::setFactoryForTesting(&plugin);
    const auto restore = qScopeGuard([] { NativeBrowserPlugin::setFactoryForTesting(nullptr); });
    auto state = fleet(); auto box = state.local();
    SessionInfo session; session.name = "dsh/infra/new"; session.cmd = "dsh"; session.project = "infra"; session.tag = "new";
    session.tracked = true; session.resumable = true; session.state = "running"; session.runtimeState = "live"; session.processState = "running";
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QTemporaryDir fixtureDir;const auto program=fixtureDir.filePath("hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import pathlib,sys
root=pathlib.Path(__file__).parent
if 'inspect' in sys.argv: print((root/'details.json').read_text())
else: print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QJsonObject details{{"backend", "dsh"}, {"tracked", true}, {"conversation_id", "conversation-one"}, {"run_id", "native-run"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"phase", "input"}, {"activity", "attention"},
        {"activity_summary", "Sign in required"}, {"auth_required", true}, {"replace_events", true}, {"events", QJsonArray{}}};
    const auto save=[&]{return FakeCli::replace(fixtureDir.filePath("details.json"),QJsonDocument(details).toJson());};QVERIFY(save());
    SessionsWindow window(FakeCli::program(program)); window.setFleet(state); window.show(); window.showSession({}, session.name);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    WAIT_VERIFY(!inspected.isEmpty());
    client->inspectionReady({}, session.name, details);
    auto *banner = window.findChild<QWidget *>("nativeSignInBanner"); QVERIFY(banner); QVERIFY(banner->isVisible());
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); auto *editor = composer->findChild<QPlainTextEdit *>(); QVERIFY(editor);
    setComposerText(composer, "Keep this first message");
    auto *send = composer->findChild<QPushButton *>("sendMessage"); QVERIFY(send); QVERIFY(!send->isEnabled());
    auto *activity = window.findChild<ActivityView *>("mainActivity")->browser();
    WAIT_VERIFY(activity->toPlainText().contains("After signing in"));
    // The native tab requests a private URL; this fixture intentionally has
    // no browser endpoint. It must not open an external browser automatically.
    QSignalSpy external(client, &HgsClient::writeDone), connection(client, &HgsClient::nativeUiFailed);
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    const int native=tabs->indexOf(window.findChild<QWidget *>("nativeUiView")); QVERIFY(native>=0);
    QVERIFY(!tabs->isTabVisible(tabs->indexOf(window.findChild<TerminalView *>())));
    QCOMPARE(tabs->tabText(native), QString("Native UI")); QVERIFY(tabs->isTabVisible(native));
    window.findChild<QPushButton *>("nativeSignInButton")->click();
    QCOMPARE(tabs->currentWidget()->objectName(), QString("nativeUiView"));
    WAIT_COMPARE(connection.size(), 1); QCOMPARE(external.size(), 0);
    tabs->setCurrentIndex(0);
    details["auth_required"] = false; details["phase"] = "idle"; details["activity"] = "idle"; details["activity_summary"] = "Ready";QVERIFY(save());
    client->inspectionReady({}, session.name, details);
    QVERIFY(!banner->isVisible()); QVERIFY(send->isEnabled()); QCOMPARE(editor->toPlainText(), QString("Keep this first message"));
    WAIT_VERIFY2(activity->toPlainText().contains("Ready for your first message"), qPrintable(activity->toPlainText()));
}

void TestSessionsWindow::nativeUiOpensBrowserWithoutPlugin()
{
    // This suite's directory has no plugin: no Native UI tab, and the routes to
    // it hand the native UI to the system browser.
    QVERIFY(!NativeBrowserPlugin::factory());
    auto state = fleet(); auto box = state.local();
    SessionInfo session; session.name = "dsh/infra/new"; session.cmd = "dsh"; session.project = "infra"; session.tag = "new";
    session.tracked = true; session.resumable = true; session.state = "running"; session.runtimeState = "live"; session.processState = "running";
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QTemporaryDir fixtureDir; const auto script = fixtureDir.filePath("hgs");
    QVERIFY(FakeCli::install(script, R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
with open(__file__+'.calls','a') as calls: calls.write(json.dumps(sys.argv[1:])+'\n')
if 'inspect' in sys.argv: print((root/'details.json').read_text())
else: print('{}')
)PY"));
    const QJsonObject details{{"backend", "dsh"}, {"tracked", true}, {"conversation_id", "conversation-one"}, {"run_id", "native-run"},
        {"runtime_state", "live"}, {"process_state", "running"}, {"phase", "input"}, {"activity", "attention"},
        {"activity_summary", "Sign in required"}, {"auth_required", true}, {"replace_events", true}, {"events", QJsonArray{}}};
    QVERIFY(FakeCli::replace(fixtureDir.filePath("details.json"), QJsonDocument(details).toJson()));
    SessionsWindow window(FakeCli::program(script)); window.setFleet(state); window.show(); window.showSession({}, session.name);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    QTRY_VERIFY(!inspected.isEmpty());
    client->inspectionReady({}, session.name, details);
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    QVERIFY(!tabs->isTabVisible(1)); QVERIFY(!tabs->isTabVisible(3));
    QSignalSpy written(client, &HgsClient::writeDone);
    window.findChild<QPushButton *>("nativeSignInButton")->click();
    QTRY_COMPARE(written.size(), 1); QCOMPARE(tabs->currentIndex(), 0);
    QFile calls(script + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly)); bool browser = false;
    for (const auto &line : calls.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).array();
        if (call.contains(QJsonValue("native-ui"))) { browser = true; QVERIFY(!call.contains(QJsonValue("--json"))); }
    }
    QVERIFY(browser);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_opencode.moc"
