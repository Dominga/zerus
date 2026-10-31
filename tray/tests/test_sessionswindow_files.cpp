// U62 (#712): File references, drops and downloads, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    // U22/N17: the DeepSeek session-log upload warning on line three.
    void deepSeekUploadWarningShowsOnTheCard();
    void fileDropsAcrossSessionPanelKeepDraftAndTarget();
    void terminalDropKeepsActivityDraftAndDoesNotSubmit();
    void fileReferencesKeepMachineAndWorkspace();
    void mappedFileActionRetainsItsOriginalConversation();
    void composerFileReferencesFollowHostAndChildSelection();
    void activityAttachmentsOpenLikeDownloads();
    void fileReferenceOpeningOutlivesDialog_data();
    void fileReferenceOpeningOutlivesDialog();
    void terminalDropQuotesPathsForTheMachine();
    void terminalDropStagesIntoAnUntrackedRemoteShell();
    void folderShellFollowsTheMachinePlatform();
    void remoteFileDownloads_data();
    void remoteFileDownloads();
    void cachedRemoteFilesOpenAndRevealWithoutAnotherDownload();
    void manualRemoteSaveRetiresTheOutstandingCacheLookup();
};

void TestSessionsWindow::deepSeekUploadWarningShowsOnTheCard()
{
    // U22/N17: a DeepSeek session whose own records still accept the upload, or
    // whose running host was started before the block, warns on line three of
    // its card - the block's promise is never hidden by an activity excerpt.
    // One DeepSeek session whose own records still accept the upload.
    FleetState state;
    BoxState box; box.host = "arch"; box.ok = true;
    SessionInfo upload; upload.name = "dsh/orbit/backend"; upload.cmd = "dsh"; upload.project = "orbit"; upload.tag = "backend";
    upload.tracked = true; upload.resumable = true; upload.activity = "idle"; upload.phase = "idle";
    upload.uploadNote = "DeepSeek session-log upload still active";
    upload.uploadSeen = QJsonObject{{"at", 1.0}, {"plugin", "session-log-deepseek"}};
    box.sessions = {upload};
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto warnedRow = [list]() -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i)
            if (list->item(i)->data(SessionRoles::Agent) == QLatin1String("dsh")) return list->item(i);
        return nullptr;
    };
    QTRY_VERIFY_WITH_TIMEOUT(warnedRow() != nullptr, 5000);
    QCOMPARE(warnedRow()->data(SessionRoles::Detail).toString(), QString("DeepSeek session-log upload still active"));
    QVERIFY(warnedRow()->data(SessionRoles::UploadWarning).toBool());
    QVERIFY(warnedRow()->data(Qt::AccessibleTextRole).toString().contains("upload still active"));
    // Optional shots: the same warning in both themes.
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) {
        QDir().mkpath(shots);
        list->setProperty("hgsDark", true); list->viewport()->update(); QTest::qWait(300);
        QVERIFY(list->grab().save(QDir(shots).filePath("upload-warning-dark.png")));
        list->setProperty("hgsDark", false); list->viewport()->update(); QTest::qWait(300);
        QVERIFY(list->grab().save(QDir(shots).filePath("upload-warning-light.png")));
    }
}

void TestSessionsWindow::fileDropsAcrossSessionPanelKeepDraftAndTarget()
{
    QTemporaryDir temporary;QVERIFY(temporary.isValid());QFile file(temporary.filePath("report #1.pdf"));
    QVERIFY(file.open(QIODevice::WriteOnly));file.write("%PDF-1.4\nA test report");file.close();
    QMimeData mime;mime.setUrls({QUrl::fromLocalFile(file.fileName())});
    const QJsonObject parent{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},
        {"subagents",QJsonObject{{"child-1",QJsonObject{{"name","Review"},{"state","working"}}}}}};
    const QJsonObject childDetails{{"parent_conversation_id","conversation-one"},{"conversation_id","child-one"},
        {"run_id","run-one"},{"send_supported",true}};
    SessionsWindow window(programInspecting("file-drop-hgs",parent,childDetails));window.resize(1100,760);window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep every word of this draft");
    auto cursor=composer->editor()->textCursor();cursor.select(QTextCursor::Document);composer->editor()->setTextCursor(cursor);
    auto *panel=window.findChild<QWidget *>("sessionDetailPanel");auto *overlay=window.findChild<QWidget *>("sessionFileDropOverlay");QVERIFY(panel);QVERIFY(overlay);
    auto *browser=window.findChild<ActivityView *>("mainActivity")->browser();
    QSignalSpy sent(composer,&MessageComposer::sendRequested);int count=0;
    for(auto *target:QList<QWidget *>{panel,window.findChild<QLabel *>("detailTitle"),browser->viewport()}) {
        QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(target,&enter);
        QVERIFY(enter.isAccepted());QVERIFY(overlay->isVisible());QCOMPARE(overlay->geometry(),panel->rect());
        const auto preview=qEnvironmentVariable("HGS_DROP_PREVIEW");if(!preview.isEmpty()&&count==0){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/session-file-drop.png"));}
        QDropEvent drop(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(target,&drop);
        QVERIFY(drop.isAccepted());QVERIFY(overlay->isHidden());QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),++count);
        QVERIFY(composer->editor()->toPlainText().contains("Keep every word of this draft"));
    }
    auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    for(auto *target:QList<QWidget *>{window.findChild<QTextBrowser *>("sessionInfo")->viewport()}) {
        if(target==tabs->widget(1))tabs->setCurrentIndex(1);
        QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(target,&enter);QVERIFY(enter.isAccepted());
        QDropEvent drop(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(target,&drop);
        QVERIFY(drop.isAccepted());QCOMPARE(tabs->currentIndex(),0);QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),++count);
    }
    QCOMPARE(sent.size(),0);QVERIFY(QFileInfo::exists(file.fileName()));
    QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&enter);QVERIFY(enter.isAccepted());
    window.showSession("mac","claude/infra/review");
    QDropEvent stale(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&stale);
    QVERIFY(!stale.isAccepted());QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),0);
    window.showSession({},"codex/hgs/dashboard");QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),count);
    QMimeData web;web.setUrls({QUrl("https://example.test/file.pdf")});
    QDragEnterEvent remote(QPoint(10,10),Qt::CopyAction,&web,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&remote);QVERIFY(!remote.isAccepted());
    // A visible subagent composer owns the panel's drop; the parent draft stays intact.
    auto *client=window.findChild<HgsClient *>();QTest::qWait(60);
    client->inspectionReady({},"codex/hgs/dashboard",parent);
    window.findChild<QTextBrowser *>("subagents")->anchorClicked(QUrl("hgs-agent:child-1"));
    client->subagentInspectionReady({},"codex/hgs/dashboard","child-1",{},childDetails);
    auto *child=window.findChild<MessageComposer *>("subagentComposer");QVERIFY(child->isInputVisible());setComposerText(child, "Child draft");
    QDragEnterEvent childEnter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&childEnter);QVERIFY(childEnter.isAccepted());
    QDropEvent childDrop(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&childDrop);QVERIFY(childDrop.isAccepted());
    QTRY_COMPARE(child->findChildren<QWidget *>("attachmentRow").size(),1);QVERIFY(child->editor()->toPlainText().contains("Child draft"));
    QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),count);QCOMPARE(sent.size(),0);
}

void TestSessionsWindow::terminalDropKeepsActivityDraftAndDoesNotSubmit()
{
    const auto program=m_dir->filePath("terminal-drop-hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport os,runpy,sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[:1]==['a']:\n if os.name!='nt':\n  import tty;tty.setraw(0)\n"
        " sys.stdout.write('\\x1b[?2004h');sys.stdout.flush();sys.stdin.buffer.read();sys.exit(0)\n"
        "sys.argv[0]="+FakeCli::pythonString(script())+";runpy.run_path(sys.argv[0],run_name='__main__')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const auto path=m_dir->filePath("report's draft.pdf");QFile file(path);QVERIFY(file.open(QIODevice::WriteOnly));file.write("fixture");file.close();
    // The dropped file is a real one of this computer, in the form of the GUI's platform.
    auto state=fleet();auto own=state.local();own.metrics.remove("platform");state.setLocal(own,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program));window.setFleet(state);window.resize(1100,760);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep Activity draft");
    auto *terminal=window.findChild<TerminalView *>();auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");
    tabs->setCurrentWidget(terminal);QTRY_VERIFY(terminal->isConnected());QTest::qWait(150);
    QSignalSpy output(terminal->terminalScreen(),&TerminalScreen::output);QSignalSpy sent(composer,&MessageComposer::sendRequested);
    QMimeData mime;mime.setUrls({QUrl::fromLocalFile(path)});
    auto *panel=window.findChild<QWidget *>("sessionDetailPanel");
    QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&enter);QVERIFY(enter.isAccepted());
    QDropEvent drop(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&drop);QVERIFY(drop.isAccepted());
    QCOMPARE(tabs->currentWidget(),terminal);QCOMPARE(composer->editor()->toPlainText(),QString("Keep Activity draft"));
    QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),0);QCOMPARE(sent.size(),0);
    QByteArray pasted;for(const auto &item:output)pasted+=item.first().toByteArray();
#ifdef Q_OS_WIN
    // This Windows computer reads native paths (terminalDropQuotesPathsForTheMachine).
    QVERIFY(pasted.contains(('\''+QDir::toNativeSeparators(path).replace('\'',QStringLiteral("''"))+'\'').toUtf8()));
#else
    QVERIFY(pasted.contains("report'\\''s draft.pdf"));
#endif
    QVERIFY(!pasted.contains('\r'));QVERIFY(!pasted.contains('\n'));
    output.clear();QDragEnterEvent second(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&second);
    tabs->setCurrentIndex(0);QDropEvent stale(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&stale);
    QVERIFY(!stale.isAccepted());
#ifdef Q_OS_WIN
    // Nothing reaches the terminal but the focus reports a pseudoconsole asks for (DECSET 1004).
    QByteArray rest;for(const auto &item:output)rest+=item.first().toByteArray();
    rest.replace("\x1b[I","").replace("\x1b[O","");QCOMPARE(rest,QByteArray());
#else
    QCOMPARE(output.size(),0);
#endif
    QCOMPARE(composer->findChildren<QWidget *>("attachmentRow").size(),0);
}

void TestSessionsWindow::fileReferencesKeepMachineAndWorkspace()
{
    // Polls during the first dialog re-inspect the session; they must keep its workspace.
    const auto fixture = programInspecting("file-reference-hgs", {{"tracked", true}, {"conversation_id", "conversation-one"},
        {"events", QJsonArray{}}, {"cursor", 0}, {"cwd", "/local/project"}}); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(fleet()); window.show(); QTest::qWait(80);
    auto *client = window.findChild<HgsClient *>();
    auto *activity = window.findChild<ActivityView *>("mainActivity");
    QSignalSpy copied(&window, &SessionsWindow::copyTextRequested);
    QSignalSpy ssh(&window, &SessionsWindow::machineSshRequested);
    client->inspectionReady({}, "codex/hgs/dashboard", {{"cwd", "/local/project"}, {"conversation_id", "conversation-one"}});
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QLineEdit *>("sessionFilePath")->text(), QString("/local/project/docs/README.md"));
        QVERIFY(dialog->findChild<QPushButton *>("openSessionFile"));
        QVERIFY(!dialog->findChild<QPushButton *>("openMappedSessionFile"));
        QVERIFY(!dialog->findChild<QPushButton *>("openSessionFileSsh"));
        auto *copy=dialog->findChild<QPushButton *>("copySessionFilePath");
        QVERIFY(copy->text().isEmpty());QVERIFY(!copy->icon().isNull());
        copy->click();QVERIFY(dialog->isVisible());
        dialog->reject();
    });
    activity->fileReferenceActivated("docs/README.md:12");
    QCOMPARE(copied.takeFirst().at(0).toString(), QString("/local/project/docs/README.md"));
    // A file link in a question's Markdown opens the same dialog.
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QLineEdit *>("sessionFilePath")->text(), QString("/local/project/docs/README.md"));
        dialog->reject();
    });
    window.findChild<QuestionCard *>("questionCard")->fileReferenceActivated("docs/README.md:12");
    QVERIFY(!window.findChild<QDialog *>("sessionFileDialog"));
    window.showSession("mac", "claude/infra/review"); QTest::qWait(50);
    client->inspectionReady("mac", "claude/infra/review", {{"cwd", "/Users/remote/project"}, {"conversation_id", "conversation-one"}});
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QLineEdit *>("sessionFilePath")->text(), QString("/Users/remote/project/docs/README.md"));
        QVERIFY(!dialog->findChild<QPushButton *>("openSessionFile"));
        dialog->findChild<QPushButton *>("copySessionFilePath")->click();QVERIFY(dialog->isVisible());
        dialog->findChild<QPushButton *>("openSessionFileSsh")->click();QVERIFY(!dialog->isVisible());
    });
    activity->fileReferenceActivated("docs/README.md");
    QCOMPARE(copied.size(), 1); QCOMPARE(copied.takeFirst().at(0).toString(), QString("/Users/remote/project/docs/README.md"));
    QCOMPARE(ssh.size(), 1); QCOMPARE(ssh.takeFirst().at(0).toString(), QString("mac"));
}

void TestSessionsWindow::mappedFileActionRetainsItsOriginalConversation()
{
#ifndef Q_OS_WIN
    QSKIP("The explicit original-file action belongs to a Windows desktop");
#else
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer=R"PY(elif c=='--help':print('wsl-file-proof-v1')
elif c=='machine' and a[1:]==['ls']:
 print(json.dumps({'local':'fixture-controller','machines':[{'alias':'mac','connection':{'transport':'wsl-local','enabled':True,'wsl':{'distribution':'Ubuntu-24.04','user':'alice','hgs_path':'/opt/zerus/hgs'}}}]}))
elif c=='machine' and a[1:3]==['file-proof','mac']:
 p=json.load(sys.stdin)
 open(__file__+'.proof','w').write(json.dumps(p))
 print('The selected fixture conversation changed; nothing was opened',file=sys.stderr);sys.exit(1)
elif c=='inspect':
 print(json.dumps({'tracked':True,'run_id':'run-source','conversation_id':'conversation-source','cwd':'/mnt/c/fixture','events':[],'cursor':0}))
)PY";
    const auto fixture=m_dir->filePath("mapped-file-scope-hgs");
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto peer=*state.peer("mac");
    peer.sessions[0].runId="run-source";peer.sessions[0].conversationId="conversation-source";
    state.setPeer(peer,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(fixture));window.setFleet(state);window.show();
    window.showSession("mac","claude/infra/review");QTest::qWait(80);
    auto *client=window.findChild<HgsClient *>();
    client->inspectionReady("mac","claude/infra/review",{{"tracked",true},{"run_id","run-source"},{"conversation_id","conversation-source"},{"cwd","/mnt/c/fixture"}});
    FolderUrlRecorder opened;
    bool observed=false;
    QTimer::singleShot(0,&window,[&]{
        auto *reference=window.findChild<QDialog *>("sessionFileDialog");QVERIFY(reference);
        const auto closeReference=qScopeGuard([&]{reference->reject();});
        auto *button=reference->findChild<QPushButton *>("openMappedSessionFile");QVERIFY(button);QVERIFY(button->isEnabled());
        QTimer::singleShot(0,&window,[&]{
            auto *dialog=reference->findChild<QDialog *>("wslFileOpenDialog");QVERIFY(dialog);
            const auto closeDialog=qScopeGuard([&]{dialog->reject();});observed=true;
            auto *root=dialog->findChild<QLineEdit *>("fileMappingWindowsRoot");QVERIFY(root);QTRY_VERIFY(root->isEnabled());
            root->setText("C:/fixture");dialog->findChild<QLineEdit *>("fileMappingMountRoot")->setText("/mnt");
            dialog->findChild<QLineEdit *>("fileMappingWorkspaceId")->setText("fixture");
            // A new run arrives while this modal stays open. Its request must
            // still name the original run, so the native guard can refuse it.
            auto changed=peer;changed.sessions[0].runId="run-replaced";changed.sessions[0].conversationId="conversation-replaced";
            state.setPeer(changed,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
            dialog->findChild<QPushButton *>("previewMappedFile")->click();
            QTRY_VERIFY(dialog->findChild<QLabel *>("mappedFileNotice")->text().contains("fixture conversation changed"));
            QVERIFY(!dialog->findChild<QPushButton *>("openMappedFile")->isEnabled());
            QFile record(fixture+".proof");QVERIFY(record.open(QIODevice::ReadOnly));const auto selection=QJsonDocument::fromJson(record.readAll()).object()["selection"].toObject();
            QCOMPARE(selection["session"].toString(),QString("claude/infra/review"));
            QCOMPARE(selection["run_id"].toString(),QString("run-source"));
            QCOMPARE(selection["conversation_id"].toString(),QString("conversation-source"));
            QCOMPARE(selection["path"].toString(),QString("/mnt/c/fixture/docs/README.md"));
        });
        button->click();
    });
    window.findChild<ActivityView *>("mainActivity")->fileReferenceActivated("docs/README.md");
    QVERIFY(observed);QVERIFY(opened.urls.isEmpty());QVERIFY(opened.revealed.isEmpty());
#endif
}

void TestSessionsWindow::composerFileReferencesFollowHostAndChildSelection()
{
    const QJsonObject parent{{"tracked", true}, {"conversation_id", "conversation-one"}, {"run_id", "run-one"},
        {"subagents", QJsonObject{{"child-1", QJsonObject{{"name", "Review"}, {"state", "working"}}}}}};
    const QJsonObject childDetails{{"parent_conversation_id", "conversation-one"}, {"conversation_id", "child-one"},
        {"run_id", "run-one"}, {"send_supported", true}};
    SessionsWindow window(programInspecting("composer-file-reference-hgs", parent, childDetails));
    keepDetailsPollsOut(window);
    window.resize(1400, 900); window.setFleet(fleet()); window.show(); window.showSession("mac", "claude/infra/review");
    auto *client = window.findChild<HgsClient *>();
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    setComposerText(composer, "Remote draft: ");
    settleSelectedInspection(composer, client, "mac", "claude/infra/review", parent);
    QSignalSpy sends(composer, &MessageComposer::sendRequested);
    auto *reference = composer->findChild<QPushButton *>("referenceMessageFile");
    QVERIFY(reference->isVisible()); QVERIFY(reference->isEnabled());
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        QVERIFY(!dialog->findChild<QPushButton *>("browseReferenceFile"));
        QVERIFY(dialog->findChild<QLabel *>("fileReferenceHint")->text().contains("mac"));
        dialog->findChild<QLineEdit *>("fileReferencePath")->setText("~/large existing.bin");
        dialog->findChild<QPushButton *>("insertFileReference")->click();
    });
    reference->click();
    QVERIFY(composer->editor()->toPlainText().contains("\"~/large existing.bin\"")); QCOMPARE(sends.size(), 0);
    window.showSession({}, "codex/hgs/dashboard");
    setComposerText(composer, "Local parent draft");
    settleSelectedInspection(composer, client, {}, "codex/hgs/dashboard", parent);
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        QVERIFY(dialog->findChild<QPushButton *>("browseReferenceFile"));
    });
    reference->click();
    client->inspectionReady({}, "codex/hgs/dashboard", parent);
    // A replacement run under the same row is a different reference context.
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        dialog->findChild<QLineEdit *>("fileReferencePath")->setText(script());
        auto replacement = parent; replacement["run_id"] = "run-two";
        client->inspectionReady({}, "codex/hgs/dashboard", replacement);
        dialog->findChild<QPushButton *>("insertFileReference")->click();
    });
    reference->click(); QCOMPARE(composer->editor()->toPlainText(), "Local parent draft");
    client->inspectionReady({}, "codex/hgs/dashboard", parent);
    // A new listing invalidates even while the accepted inspection is older.
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        dialog->findChild<QLineEdit *>("fileReferencePath")->setText(script());
        auto replacement = fleet(); auto box = replacement.local();
        for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") session.runId = "run-listed-two";
        replacement.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(replacement);
        dialog->findChild<QPushButton *>("insertFileReference")->click();
    });
    reference->click(); QCOMPARE(composer->editor()->toPlainText(), "Local parent draft");
    window.setFleet(fleet()); client->inspectionReady({}, "codex/hgs/dashboard", parent);
    // Main -> child -> main must invalidate the original main dialog too.
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        dialog->findChild<QLineEdit *>("fileReferencePath")->setText(script());
        window.findChild<QTextBrowser *>("subagents")->anchorClicked(QUrl("hgs-agent:child-1"));
        client->subagentInspectionReady({}, "codex/hgs/dashboard", "child-1", {}, childDetails);
        window.findChild<QPushButton *>("subagentBack")->click();
        dialog->findChild<QPushButton *>("insertFileReference")->click();
    });
    reference->click(); QCOMPARE(composer->editor()->toPlainText(), "Local parent draft");
    window.findChild<QTextBrowser *>("subagents")->anchorClicked(QUrl("hgs-agent:child-1"));
    client->subagentInspectionReady({}, "codex/hgs/dashboard", "child-1", {}, childDetails);
    auto *child = window.findChild<MessageComposer *>("subagentComposer"); setComposerText(child, "Child draft");
    auto *childReference = child->findChild<QPushButton *>("referenceMessageFile");
    QVERIFY(childReference->isVisible()); QVERIFY(childReference->isEnabled());
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("fileReferenceDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        QVERIFY(dialog->findChild<QPushButton *>("browseReferenceFile"));
        dialog->findChild<QLineEdit *>("fileReferencePath")->setText(script());
        window.findChild<QPushButton *>("subagentBack")->click();
        dialog->findChild<QPushButton *>("insertFileReference")->click();
    });
    childReference->click();
    QCOMPARE(child->editor()->toPlainText(), "Child draft");
    QCOMPARE(composer->editor()->toPlainText(), "Local parent draft"); QCOMPARE(sends.size(), 0);
}

void TestSessionsWindow::activityAttachmentsOpenLikeDownloads()
{
    // A conversation attachment opens quietly; a program is shown in its folder.
    QStandardPaths::setTestModeEnabled(true); const auto realPaths = qScopeGuard([] { QStandardPaths::setTestModeEnabled(false); });
    FolderUrlRecorder opened;
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *activity = window.findChild<ActivityView *>("mainActivity"); auto *notice = window.findChild<QLabel *>("notice");
    const auto document = AttachmentFiles::store("notes.txt", "attachment notes"), program = AttachmentFiles::store("setup.exe", "MZ fixture");
    const auto cleanup = qScopeGuard([&] { QFile::remove(document); QFile::remove(program); });
    QVERIFY(!document.isEmpty()); QVERIFY(!program.isEmpty());
    activity->attachmentActivated({{"name", "notes.txt"}, {"local_path", document}});
    QTRY_COMPARE(opened.urls.size(), 1); QCOMPARE(opened.urls.first().toLocalFile(), QFileInfo(document).canonicalFilePath());
    QTest::qWait(20); QVERIFY(!notice->text().contains("Operation failed")); QVERIFY(opened.revealed.isEmpty());
    activity->attachmentActivated({{"name", "setup.exe"}, {"local_path", program}});
    QTRY_COMPARE(opened.revealed.size(), 1); QCOMPARE(opened.urls.size(), 1);
    QTRY_VERIFY(notice->text().contains("setup.exe")); QVERIFY(!notice->property("error").toBool());
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
    QVERIFY(!ExternalFile::downloadMarker(program).isEmpty());
#endif
}

void TestSessionsWindow::fileReferenceOpeningOutlivesDialog_data()
{
    QTest::addColumn<bool>("folder");
    QTest::addColumn<QString>("name");
    QTest::addColumn<QString>("platform");
    QTest::newRow("file") << false << "sample image.png" << "linux";
    QTest::newRow("folder") << true << "sample image.png" << "linux";
    // An agent's script is shown in its folder rather than run.
    QTest::newRow("program") << false << "deploy.sh" << "linux";
    // A Windows computer shows and copies backslashes, yet opens the Qt path.
    QTest::newRow("windows-file") << false << "sample image.png" << "windows";
    QTest::newRow("windows-folder") << true << "sample image.png" << "windows";
    // Before the first poll the platform is the GUI's own.
    QTest::newRow("unreported-platform") << false << "sample image.png" << "";
}

void TestSessionsWindow::fileReferenceOpeningOutlivesDialog()
{
    QFETCH(bool, folder); QFETCH(QString, name); QFETCH(QString, platform);
    QTemporaryDir directory;
    QVERIFY(directory.isValid());
    const auto path = directory.filePath(name);
    QFile file(path); QVERIFY(file.open(QIODevice::WriteOnly)); file.write("synthetic file"); file.close();
    const auto shown = platform == "windows" ? QString(path).replace('/', '\\')
        : platform.isEmpty() ? QDir::toNativeSeparators(path) : path;
    FolderUrlRecorder opened;
    auto state = fleet(); auto local = state.local();
    if (platform.isEmpty()) local.metrics.remove("platform"); else local.metrics["platform"] = platform;
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); QTest::qWait(80);
    auto *activity = window.findChild<ActivityView *>("mainActivity");
    QSignalSpy copied(&window, &SessionsWindow::copyTextRequested);
    QPointer<QDialog> dialog;
    QTimer::singleShot(0, &window, [&] {
        dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([&] { if (dialog && dialog->isVisible()) dialog->reject(); });
        QCOMPARE(dialog->findChild<QLineEdit *>("sessionFilePath")->text(), shown);
        dialog->findChild<QPushButton *>("copySessionFilePath")->click();
        QVERIFY(dialog->isVisible());
        QCOMPARE(copied.size(), 1); QCOMPARE(copied.first().at(0).toString(), shown);
        QVERIFY(opened.urls.isEmpty());
        auto *button = dialog->findChild<QPushButton *>(folder ? "openSessionFileFolder" : "openSessionFile");
        QVERIFY(button); button->click();
        // #744: the path is checked by PathProbe's worker first.
        QTRY_VERIFY(!dialog->isVisible());
        // The desktop request must not belong to the disappearing modal window.
        QVERIFY(opened.urls.isEmpty());
    });
    activity->fileReferenceActivated(path);
    QVERIFY(dialog.isNull());
    QVERIFY(opened.urls.isEmpty()); QVERIFY(opened.revealed.isEmpty());
    if (name.endsWith(".sh")) {
        QTRY_COMPARE(opened.revealed, QStringList{path}); QVERIFY(opened.urls.isEmpty());
        QTRY_VERIFY(window.findChild<QLabel *>("notice")->text().contains("deploy.sh")); return;
    }
    QTRY_COMPARE(opened.urls.size(), 1);
    QCOMPARE(opened.urls.first(), QUrl::fromLocalFile(folder ? directory.path() : path));
    if (platform != "windows") return;
    // A peer's paths stay POSIX whatever this computer is.
    window.showSession("mac", "claude/infra/review"); QTest::qWait(50);
    QTimer::singleShot(0, &window, [&] {
        auto *remote = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(remote);
        const auto close = qScopeGuard([&] { remote->reject(); });
        QCOMPARE(remote->findChild<QLineEdit *>("sessionFilePath")->text(), QString("/Users/remote/docs/README.md"));
        remote->findChild<QPushButton *>("copySessionFilePath")->click();
    });
    activity->fileReferenceActivated("/Users/remote/docs/README.md");
    QCOMPARE(copied.size(), 2); QCOMPARE(copied.last().at(0).toString(), QString("/Users/remote/docs/README.md"));
}

void TestSessionsWindow::terminalDropQuotesPathsForTheMachine()
{
    // PowerShell accepts typographic quotes as delimiters, too. Keep every unsafe
    // Windows path literal; a peer's staged copies retain POSIX quoting.
    const auto program=m_dir->filePath("terminal-quoting-hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport json,os,runpy,sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[:1]==['a']:\n if os.name!='nt':\n  import tty;tty.setraw(0)\n"
        " sys.stdout.write('\\x1b[?2004h');sys.stdout.flush();sys.stdin.buffer.read();sys.exit(0)\n"
        "if a[:1]==['attachment']:\n p=json.load(sys.stdin)\n"
        " print(json.dumps(dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],"
        "status='staged',files=[dict(path=\"/srv/inbox/report's draft.pdf\") for _ in p['attachments']])));sys.exit(0)\n"
        "sys.argv[0]="+FakeCli::pythonString(script())+";runpy.run_path(sys.argv[0],run_name='__main__')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const QList<QPair<QString,QString>> cases{
        {QStringLiteral("plain.txt"),QStringLiteral("plain.txt")},
        {QStringLiteral("report's draft.pdf"),QStringLiteral("'report''s draft.pdf'")},
        {QStringLiteral("a&b.txt"),QStringLiteral("'a&b.txt'")},
        {QStringLiteral("cost $5's `q`.txt"),QStringLiteral("'cost $5''s `q`.txt'")},
        {QStringLiteral("left\u2018$probe`n.txt"),QStringLiteral("'left\u2018\u2018$probe`n.txt'")},
        {QStringLiteral("right\u2019$probe`n.txt"),QStringLiteral("'right\u2019\u2019$probe`n.txt'")},
        {QStringLiteral("low\u201a$probe`n.txt"),QStringLiteral("'low\u201a\u201a$probe`n.txt'")},
        {QStringLiteral("reversed\u201b$probe`n.txt"),QStringLiteral("'reversed\u201b\u201b$probe`n.txt'")},
        {QStringLiteral("left\u201cquote.txt"),QStringLiteral("'left\u201cquote.txt'")},
        {QStringLiteral("right\u201dquote.txt"),QStringLiteral("'right\u201dquote.txt'")},
        {QStringLiteral("\u043f\u0443\u0442\u044c \u043f\u0440\u0438\u043c\u0435\u0440.txt"),
         QStringLiteral("'\u043f\u0443\u0442\u044c \u043f\u0440\u0438\u043c\u0435\u0440.txt'")}
    };
    QStringList paths;
    for(const auto &item:cases) {
        QFile file(m_dir->filePath(item.first));QVERIFY(file.open(QIODevice::WriteOnly));file.write("fixture");paths<<file.fileName();
    }
    QMimeData mime;QList<QUrl> urls;for(const auto &path:paths)urls<<QUrl::fromLocalFile(path);mime.setUrls(urls);
    const auto native=[](QString path){return path.replace('/','\\');};
    const auto drop=[&](SessionsWindow &window,const QString &host,const QString &name) {
        window.showSession(host,name);
        auto *terminal=window.findChild<TerminalView *>();auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");
        tabs->setCurrentWidget(terminal);if(!QTest::qWaitFor([&]{return terminal->isConnected();},10000))return QByteArray();QTest::qWait(150);
        QSignalSpy output(terminal->terminalScreen(),&TerminalScreen::output);
        auto *panel=window.findChild<QWidget *>("sessionDetailPanel");
        QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&enter);
        QDropEvent event(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&event);
        if(!event.isAccepted())return QByteArray();
        QByteArray pasted;
        (void)QTest::qWaitFor([&]{pasted.clear();for(const auto &item:output)pasted+=item.first().toByteArray();return pasted.contains(".txt")||pasted.contains(".pdf");},20000);
        return pasted;
    };
    for(const auto &platform:{QString("windows"),QString("linux")}) {
        auto state=fleet();auto local=state.local();local.metrics["platform"]=platform;
        state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
        auto mac=*state.peer("mac");mac.sessions[0].runId="run-mac";mac.sessions[0].conversationId="conversation-mac";
        state.setPeer(mac,QDateTime::currentMSecsSinceEpoch());
        SessionsWindow window(FakeCli::program(program));window.setFleet(state);window.resize(1100,760);window.show();
        const auto pasted=QString::fromUtf8(drop(window,{},"codex/hgs/dashboard"));
        if(platform=="windows") {
            QStringList expected;
            const QString prefix=native(m_dir->path())+'\\';
            for(const auto &item:cases) {
                QString literal=item.second;
                if(literal.startsWith('\''))literal.insert(1,prefix);else literal.prepend(prefix);
                expected<<literal;
            }
            // First establish the injection-sensitive literals, independently of
            // the ordinary path presentation that also changes with this fix.
            for(int i=4;i<10;++i)QVERIFY2(pasted.contains(expected[i]),qPrintable(pasted));
            QVERIFY2(pasted.contains(expected.join(' ')+" "),qPrintable(pasted));
            QVERIFY(!pasted.contains('/'));QVERIFY(pasted.contains('\''+native(paths[1]).replace("'","''")+'\''));
        } else {
            QVERIFY2(pasted.contains('\''+paths[0]+"' '"+QString(paths[1]).replace("'","'\\''")+"' '"+paths[2]+"' '"
                                     +QString(paths[3]).replace("'","'\\''")+"' "),qPrintable(pasted));
            QVERIFY(!pasted.contains('"'));
        }
        QVERIFY(!pasted.contains('\r'));QVERIFY(!pasted.contains('\n'));
        // A peer's staged copies keep POSIX quoting whatever this computer runs.
        // Retain the original four-file remote fixture within its eight-file cap.
        mime.setUrls(urls.mid(0,4));
        const auto remote=QString::fromUtf8(drop(window,"mac","claude/infra/review"));
        mime.setUrls(urls);
        QVERIFY2(remote.contains("'/srv/inbox/report'\\''s draft.pdf' '/srv/inbox/report'\\''s draft.pdf' '/srv/inbox/report'\\''s draft.pdf' "),
                 qPrintable(remote));
        QVERIFY(!remote.contains('"'));QVERIFY(!remote.contains('\\'+QString("srv")));
        QVERIFY(!remote.contains('\r'));QVERIFY(!remote.contains('\n'));
    }
}

void TestSessionsWindow::terminalDropStagesIntoAnUntrackedRemoteShell()
{
    // A shell from `hgs sh -d` never gets a run identity: its listed creation time
    // binds the copy instead. A tracked session still waits for its identity.
    const auto program=m_dir->filePath("terminal-untracked-hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport json,os,runpy,sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[:1]==['a']:\n if os.name!='nt':\n  import tty;tty.setraw(0)\n"
        " sys.stdout.write('\\x1b[?2004h');sys.stdout.flush();sys.stdin.buffer.read();sys.exit(0)\n"
        "if a[:1]==['inspect'] and a[1].startswith('sh/'):print(json.dumps(dict(name=a[1],tracked=False,events=[],cursor=0)));sys.exit(0)\n"
        "if a[:1]==['attachment']:\n p=json.load(sys.stdin);p.pop('attachments')\n"
        " open(__file__+'.stage','a').write(json.dumps(dict(args=sys.argv[1:],request=p))+'\\n')\n"
        " if os.path.exists(__file__+'.old'):print(a[1]+': no saved binding; restart through hgs to enable tracking',file=sys.stderr);sys.exit(1)\n"
        " created=p.get('expected_created',0)+os.path.exists(__file__+'.skew')\n"
        " print(json.dumps(dict(request_id=p['request_id'],name=a[1],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],"
        "created=created,status='staged',files=[dict(path='/srv/inbox/notes.txt')])));sys.exit(0)\n"
        "sys.argv[0]="+FakeCli::pythonString(script())+";runpy.run_path(sys.argv[0],run_name='__main__')\n");
    fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QFile file(m_dir->filePath("notes.txt"));QVERIFY(file.open(QIODevice::WriteOnly));file.write("fixture");file.close();
    QMimeData mime;mime.setUrls({QUrl::fromLocalFile(file.fileName())});
    auto state=fleet();auto mac=*state.peer("mac");
    SessionInfo shell;shell.name="sh/scratch";shell.cmd="sh";shell.project="scratch";shell.created=1700000000;mac.sessions<<shell;
    state.setPeer(mac,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program));window.setFleet(state);window.resize(1100,760);window.show();
    auto *terminal=window.findChild<TerminalView *>();auto *tabs=window.findChild<QTabWidget *>("sessionDetailTabs");
    auto *panel=window.findChild<QWidget *>("sessionDetailPanel");auto *notice=window.findChild<QLabel *>("notice");
    const auto stages=[&] {
        QList<QJsonObject> result;QFile log(program+".stage");
        if(log.open(QIODevice::ReadOnly))for(const auto &line:log.readAll().split('\n'))if(!line.isEmpty())result<<QJsonDocument::fromJson(line).object();
        return result;
    };
    const auto drop=[&](const QString &name) {
        window.showSession("mac",name);tabs->setCurrentWidget(terminal);
        if(!QTest::qWaitFor([&]{return terminal->isConnected();},10000))return QByteArray();QTest::qWait(150);
        QSignalSpy output(terminal->terminalScreen(),&TerminalScreen::output);const auto before=stages().size();
        QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&enter);
        QDropEvent event(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&event);
        if(!event.isAccepted())return QByteArray();
        QByteArray pasted;
        // A transfer ends in a pasted path or a notice other than its progress.
        (void)QTest::qWaitFor([&]{
            pasted.clear();for(const auto &item:output)pasted+=item.first().toByteArray();
            return pasted.contains("notes.txt")||(stages().size()>before&&!notice->text().startsWith("Transferring"));
        },20000);
        return pasted;
    };
    const auto pasted=drop("sh/scratch");
    QVERIFY2(pasted.contains("'/srv/inbox/notes.txt' "),pasted.constData());
    auto staged=stages();QCOMPARE(staged.size(),1);
    QCOMPARE(staged[0]["args"].toArray(),QJsonArray({"@mac","attachment","sh/scratch","--stage","--json"}));
    const auto request=staged[0]["request"].toObject();
    QCOMPARE(request["expected_run_id"].toString(),QString());QCOMPARE(request["expected_conversation_id"].toString(),QString());
    QVERIFY(request["expected_run_id"].isString());QCOMPARE(request["expected_created"].toInteger(),qint64(1700000000));

    // An acknowledgement for another creation time pastes nothing.
    QVERIFY(QFile(program+".skew").open(QIODevice::WriteOnly));
    QVERIFY(!drop("sh/scratch").contains("notes.txt"));
    QCOMPARE(notice->text(),QString("Invalid acknowledgement. Check Terminal before retrying."));QCOMPARE(stages().size(),2);
    QVERIFY(QFile::remove(program+".skew"));

    // An hgs that predates untracked drops asks for a saved binding.
    QVERIFY(QFile(program+".old").open(QIODevice::WriteOnly));
    QVERIFY(!drop("sh/scratch").contains("notes.txt"));
    QCOMPARE(notice->text(),QString("Update hgs on mac to drop files into an untracked session."));QCOMPARE(stages().size(),3);
    QVERIFY(QFile::remove(program+".old"));

    // A tracked session whose run identity has not loaded transfers nothing.
    window.showSession("mac","claude/infra/review");tabs->setCurrentWidget(terminal);
    QTRY_VERIFY_WITH_TIMEOUT(terminal->isConnected(),10000);QTest::qWait(150);
    QDragEnterEvent enter(QPoint(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&enter);
    QDropEvent event(QPointF(10,10),Qt::CopyAction,&mime,Qt::LeftButton,Qt::NoModifier);QApplication::sendEvent(panel,&event);
    QCOMPARE(notice->text(),QString("Wait for the session identity to load, then drop the files again."));
    QTest::qWait(300);QCOMPARE(stages().size(),3);
}

void TestSessionsWindow::folderShellFollowsTheMachinePlatform()
{
    // The local Windows machine opens drive and UNC folders; a POSIX one, and
    // every peer, folders that start with '/'.
    for(const auto &platform:{QString("windows"),QString("linux")}) {
        const bool windows=platform=="windows";const auto now=QDateTime::currentMSecsSinceEpoch();
        auto state=fleet();auto local=state.local();local.metrics["platform"]=platform;
        local.sessions[0].cwd="C:\\work\\a folder";local.sessions[1].cwd="/work/docs";state.setLocal(local,now);
        auto mac=*state.peer("mac");mac.sessions[0].cwd="C:/work";state.setPeer(mac,now);
        SessionsWindow window(program());window.setFleet(state);window.show();
        auto *list=window.findChild<QListWidget *>("sessionList");auto *menu=window.findChild<QMenu *>("sessionContextMenu");
        auto *shell=window.findChild<QPushButton *>("openFolderShellAction");QVERIFY(shell);
        QSignalSpy requested(&window,&SessionsWindow::folderShellRequested);
        const auto menuAllows=[&](const QString &key) {
            for(int row=0;row<list->count();++row) if(list->item(row)->data(SessionRoles::Key).toString()==key) {
                const auto point=list->visualItemRect(list->item(row)).center();
                QContextMenuEvent event(QContextMenuEvent::Mouse,point,list->viewport()->mapToGlobal(point));
                QApplication::sendEvent(list->viewport(),&event);
                const bool enabled=menu->findChild<QAction *>("contextFolderShell")->isEnabled();menu->hide();return enabled;
            }
            return false;
        };
        window.showSession({},local.sessions[0].name);
        QCOMPARE(shell->isEnabled(),windows);QCOMPARE(menuAllows("\n"+local.sessions[0].name),windows);
        shell->click();
        if(windows){QCOMPARE(requested.size(),1);QCOMPARE(requested[0][0].toString(),QString());QCOMPARE(requested[0][1].toString(),QString("C:\\work\\a folder"));}
        else QCOMPARE(requested.size(),0);
        window.showSession({},local.sessions[1].name);
        QCOMPARE(shell->isEnabled(),!windows);QCOMPARE(menuAllows("\n"+local.sessions[1].name),!windows);
        window.showSession("mac",mac.sessions[0].name);
        QVERIFY(!shell->isEnabled());QVERIFY(!menuAllows("mac\n"+mac.sessions[0].name));
    }
}

void TestSessionsWindow::remoteFileDownloads_data()
{
    QTest::addColumn<int>("mode");
    QTest::newRow("download-and-open-local-copy") << 0;
    QTest::newRow("download-and-open-directly") << 1;
    QTest::newRow("failure-keeps-dialog-and-existing-copy") << 2;
    QTest::newRow("cancel-save-does-not-download") << 3;
    QTest::newRow("closing-dialog-keeps-requested-download-and-open") << 4;
    // A downloaded program is shown in its folder rather than run.
    QTest::newRow("download-and-open-program") << 5;
    QTest::newRow("automatic-name-collision") << 6;
    QTest::newRow("automatic-download-failure") << 7;
}

void TestSessionsWindow::remoteFileDownloads()
{
    QFETCH(int, mode);
    const auto nativeDialogs = QApplication::testAttribute(Qt::AA_DontUseNativeDialogs);
    QApplication::setAttribute(Qt::AA_DontUseNativeDialogs);
    const auto restoreDialogs = qScopeGuard([=] { QApplication::setAttribute(Qt::AA_DontUseNativeDialogs, nativeDialogs); });
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const bool automatic = mode == 1 || mode >= 4;
    const QString path = mode == 5 ? "/remote/project/deploy.sh" : "/remote/project/résumé ' $(literal).png";
    const auto cache = directory.filePath("Zerus Files"); QSettings().setValue("downloads/folder", cache);
    const auto destination = automatic ? QDir(cache).filePath(mode == 6 ? "résumé ' $(literal) (1).png" : QFileInfo(path).fileName())
                                       : directory.filePath("local résumé's copy.png");
    if (mode == 6) {
        QVERIFY(QDir().mkpath(cache)); QFile existing(QDir(cache).filePath(QFileInfo(path).fileName()));
        QVERIFY(existing.open(QIODevice::WriteOnly)); existing.write("existing copy");
    }
    if (mode == 2) { QFile file(destination); QVERIFY(file.open(QIODevice::WriteOnly)); file.write("previous copy"); }
    const auto fixture = m_dir->filePath("file-download-hgs");
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer = R"PY(elif c=='download':
 import pathlib,time
 open(__file__+'.calls','a').write(json.dumps(sys.argv[1:])+'\n')
 time.sleep(.1)
 if pathlib.Path(__file__+'.fail').exists():
  print('Permission denied (publickey).',file=sys.stderr);sys.exit(1)
 target=a[a.index('--output')+1];path=a[a.index('--path')+1]
 pathlib.Path(target).write_bytes(b'synthetic image')
 print(json.dumps(dict(host=sys.argv[1][1:],path=path,local_path=target,bytes=15)))
)PY";
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    QFile::remove(fixture + ".calls"); QFile::remove(fixture + ".fail");
    if (mode == 2 || mode == 7) { QFile fail(fixture + ".fail"); QVERIFY(fail.open(QIODevice::WriteOnly)); }
    FolderUrlRecorder opened;
    SessionsWindow window(FakeCli::program(fixture)); window.setFleet(fleet()); window.show();
    window.showSession("mac", "claude/infra/review");
    auto *client = window.findChild<HgsClient *>();
    QSignalSpy downloaded(client, &HgsClient::fileDownloaded), failed(client, &HgsClient::fileDownloadFailed);
    QPointer<QDialog> dialog;
    QTimer::singleShot(0, &window, [&] {
        dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([&] { if (dialog) dialog->reject(); });
        QVERIFY(opened.urls.isEmpty()); QVERIFY(!QFile::exists(fixture + ".calls"));
        if (mode == 0 && qEnvironmentVariableIsSet("HGS_TEST_FILE_DIALOG_PREVIEW"))
            QVERIFY(dialog->grab().save(qEnvironmentVariable("HGS_TEST_FILE_DIALOG_PREVIEW")));
        auto *download = dialog->findChild<QPushButton *>(automatic ? "downloadOpenSessionFile" : "downloadSessionFile");
        QVERIFY(download); QTRY_VERIFY(download->isEnabled());
        if (!automatic) QTimer::singleShot(0, &window, [&] {
            auto *save = window.findChild<QFileDialog *>("sessionFileSaveDialog"); QVERIFY(save);
            if (mode == 3) { save->reject(); return; }
            save->setOption(QFileDialog::DontConfirmOverwrite); save->selectFile(destination);
            QMetaObject::invokeMethod(save, "accept", Qt::DirectConnection);
        });
        download->click();
        if (automatic) QVERIFY(!window.findChild<QFileDialog *>("sessionFileSaveDialog"));
        if (mode == 3) {
            QVERIFY(dialog->isVisible()); QVERIFY(download->isEnabled());
            QVERIFY(!QFile::exists(fixture + ".calls")); QVERIFY(!QFile::exists(destination)); return;
        }
        QVERIFY(!download->isEnabled()); QVERIFY(dialog->findChild<QProgressBar *>("sessionFileProgress")->isVisible());
        if (mode == 4) { dialog->reject(); return; }
        // Navigation cannot rebind the file transfer to a different machine.
        window.showSession({}, "codex/hgs/dashboard");
        QTRY_COMPARE(downloaded.size() + failed.size(), 1);
        QVERIFY(opened.urls.isEmpty());
        if (mode == 2 || mode == 7) {
            QCOMPARE(failed.size(), 1); QVERIFY(dialog->isVisible()); QVERIFY(download->isEnabled());
            QVERIFY(dialog->findChild<QLabel *>("sessionFileHint")->text().contains("Permission denied"));
            if (mode == 2) { QFile file(destination); QVERIFY(file.open(QIODevice::ReadOnly)); QCOMPARE(file.readAll(), QByteArray("previous copy")); }
            else QVERIFY(!QFileInfo::exists(destination));
            return;
        }
        QCOMPARE(downloaded.size(), 1);
        QFile file(destination); QVERIFY(file.open(QIODevice::ReadOnly)); QCOMPARE(file.readAll(), QByteArray("synthetic image"));
        if (mode == 0) {
            QVERIFY(dialog->isVisible());
            QVERIFY(dialog->findChild<QLabel *>("sessionFileHint")->text().contains(destination));
            dialog->findChild<QPushButton *>("openDownloadedSessionFile")->click();
        }
        QVERIFY(!dialog->isVisible());
    });
    window.findChild<ActivityView *>("mainActivity")->fileReferenceActivated(QString::fromUtf8(QUrl::toPercentEncoding(path, "/")));
    QVERIFY(dialog.isNull());
    if (mode == 0 || (automatic && mode != 7)) {
        QTRY_COMPARE(downloaded.size(), 1);
        if (mode == 5) { QTRY_COMPARE(opened.revealed, QStringList{destination}); QVERIFY(opened.urls.isEmpty()); }
        else { QTRY_COMPARE(opened.urls.size(), 1); QCOMPARE(opened.urls.first(), QUrl::fromLocalFile(destination)); }
#if defined(Q_OS_WIN) || defined(Q_OS_MACOS)
        // Every completed download carries the system's downloaded-file marker.
        QVERIFY(!ExternalFile::downloadMarker(destination).isEmpty());
#endif
        QFile calls(fixture + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
        const auto args = QJsonDocument::fromJson(calls.readAll().trimmed()).array();
        QCOMPARE(args.size(), 6); auto prefix = args; prefix.removeLast();
        QCOMPARE(prefix, (QJsonArray{"@mac", "download", "--path", path, "--output"}));
        if (automatic) {
            const auto staging = args[5].toString();
            QCOMPARE(QFileInfo(staging).absolutePath(), cache); QVERIFY(QFileInfo(staging).fileName().startsWith(".zerus-part-"));
            QVERIFY(!QFileInfo::exists(staging));
        } else QCOMPARE(args[5].toString(), destination);
        if (mode == 6) { QFile existing(QDir(cache).filePath(QFileInfo(path).fileName())); QVERIFY(existing.open(QIODevice::ReadOnly)); QCOMPARE(existing.readAll(), QByteArray("existing copy")); }
    } else { QVERIFY(opened.urls.isEmpty()); }
}

void TestSessionsWindow::cachedRemoteFilesOpenAndRevealWithoutAnotherDownload()
{
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const auto cache = directory.filePath("Zerus Files"), fixture = directory.filePath("file-cache-hgs");
    const auto previous = Settings::value("downloads/folder");
    const auto restore = qScopeGuard([&] {
        if (previous.isValid()) QSettings().setValue("downloads/folder", previous);
        else QSettings().remove("downloads/folder");
    });
    QSettings().setValue("downloads/folder", cache);
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer = R"PY(elif c=='download':
 import pathlib
 open(__file__+'.calls','a').write(json.dumps(sys.argv[1:])+'\n')
 target=a[a.index('--output')+1];path=a[a.index('--path')+1]
 pathlib.Path(target).write_bytes(b'synthetic image')
 print(json.dumps(dict(host=sys.argv[1][1:],path=path,local_path=target,bytes=15)))
)PY";
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    FolderUrlRecorder opened;
    SessionsWindow window(FakeCli::program(fixture)); window.setFleet(fleet()); window.show();
    window.showSession("mac", "claude/infra/review");
    auto *activity = window.findChild<ActivityView *>("mainActivity"); QVERIFY(activity);
    const QString remote = "/remote/project/report.png";
    const auto activate = [&](const char *button, bool cached) {
        QTimer::singleShot(0, &window, [&, button, cached] {
            auto *dialog = window.findChild<QDialog *>("sessionFileDialog"); QVERIFY(dialog);
            const auto close = qScopeGuard([&] { if (dialog->isVisible()) dialog->reject(); });
            auto *action = dialog->findChild<QPushButton *>(button); QVERIFY(action);
            if (cached) {
                QTRY_VERIFY(dialog->findChild<QPushButton *>("showDownloadedSessionFile")->isVisible());
                QVERIFY(dialog->findChild<QPushButton *>("openDownloadedSessionFile")->isVisible());
                QCOMPARE(dialog->findChild<QPushButton *>("downloadOpenSessionFile")->text(), QString("Download again"));
                QVERIFY(dialog->findChild<QLabel *>("sessionFileHint")->text().contains("downloaded copy"));
            }
            QTRY_VERIFY(action->isEnabled()); action->click();
            QTRY_VERIFY(!dialog->isVisible());
        });
        activity->fileReferenceActivated(remote);
    };
    const auto transfers = [&] {
        QFile calls(fixture + ".calls");
        return calls.open(QIODevice::ReadOnly) ? calls.readAll().trimmed().split('\n').size() : 0;
    };
    activate("downloadOpenSessionFile", false);
    QTRY_COMPARE(opened.urls.size(), 1);
    const auto saved = opened.urls.first().toLocalFile(); QVERIFY(QFileInfo::exists(saved));
    QCOMPARE(transfers(), 1);
    activate("showDownloadedSessionFile", true);
    QTRY_COMPARE(opened.revealed, QStringList{saved}); QCOMPARE(opened.urls.size(), 1); QCOMPARE(transfers(), 1);
    activate("openDownloadedSessionFile", true);
    QTRY_COMPARE(opened.urls.size(), 2); QCOMPARE(opened.urls.last().toLocalFile(), saved); QCOMPARE(transfers(), 1);
    // Refresh is explicit and keeps the previously downloaded copy intact.
    activate("downloadOpenSessionFile", true);
    QTRY_COMPARE(opened.urls.size(), 3); QVERIFY(opened.urls.last().toLocalFile() != saved);
    QCOMPARE(transfers(), 2); QVERIFY(QFileInfo::exists(saved));
}

void TestSessionsWindow::manualRemoteSaveRetiresTheOutstandingCacheLookup()
{
    const auto nativeDialogs = QApplication::testAttribute(Qt::AA_DontUseNativeDialogs);
    QApplication::setAttribute(Qt::AA_DontUseNativeDialogs);
    const auto restoreDialogs = qScopeGuard([=] {
        QApplication::setAttribute(Qt::AA_DontUseNativeDialogs, nativeDialogs);
    });
    QTemporaryDir directory; QVERIFY(directory.isValid());
    const auto cache = directory.filePath("Zerus Files");
    const auto previous = Settings::value("downloads/folder");
    const auto restoreCache = qScopeGuard([&] {
        if (previous.isValid()) QSettings().setValue("downloads/folder", previous);
        else QSettings().remove("downloads/folder");
    });
    QSettings().setValue("downloads/folder", cache);
    const QString remote = "/remote/project/report.png";
    auto prepared = DownloadCache::prepare(cache);
    QVERIFY2(prepared.error.isEmpty(), qPrintable(prepared.error)); QVERIFY(prepared.pending);
    {
        QFile file(prepared.pending->path); QVERIFY(file.open(QIODevice::WriteOnly));
        QCOMPARE(file.write("old cached bytes"), qint64(16));
    }
    const auto cached = DownloadCache::publish(prepared.pending, remote, -1, {"mac", remote});
    QVERIFY2(cached.error.isEmpty(), qPrintable(cached.error)); QVERIFY(!cached.path.isEmpty());
    QVERIFY(QFileInfo::exists(cached.path));

    const auto fixture = directory.filePath("stale-cache-hgs");
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer = R"PY(elif c=='download':
 import pathlib
 open(__file__+'.calls','a').write(json.dumps(sys.argv[1:])+'\n')
 target=a[a.index('--output')+1];path=a[a.index('--path')+1]
 data=b'manually saved bytes'
 pathlib.Path(target).write_bytes(data)
 print(json.dumps(dict(host=sys.argv[1][1:],path=path,local_path=target,bytes=len(data))))
)PY";
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    const auto transfers = [&]() -> QList<QByteArray> {
        QFile file(fixture + ".calls");
        return file.open(QIODevice::ReadOnly) ? file.readAll().trimmed().split('\n') : QList<QByteArray>{};
    };
    FolderUrlRecorder opened;
    SessionsWindow window(FakeCli::program(fixture)); window.setFleet(fleet()); window.show();
    window.showSession("mac", "claude/infra/review");
    auto *activity = window.findChild<ActivityView *>("mainActivity"); QVERIFY(activity);
    auto *client = window.findChild<HgsClient *>(); QVERIFY(client);

    const QStringList actions{"showDownloadedSessionFile", "openDownloadedSessionFile"};
    for (int index = 0; index < actions.size(); ++index) {
        const auto destination = directory.filePath(QString("manual copy %1.png").arg(index));
        const auto previousTransfers = transfers().size();
        const auto previousUrls = opened.urls.size(), previousReveals = opened.revealed.size();
        QSignalSpy downloaded(client, &HgsClient::fileDownloaded);
        QSignalSpy failed(client, &HgsClient::fileDownloadFailed);
        QSignalSpy checked(client, &HgsClient::fileCacheChecked);
        QVERIFY(downloaded.isValid()); QVERIFY(failed.isValid()); QVERIFY(checked.isValid());
        struct WorkerGate {
            QSemaphore entered, release;
            std::atomic<bool> expired{false};
        };
        const auto gate = std::make_shared<WorkerGate>();
        QCOMPARE(DownloadCache::pool().maxThreadCount(), 1);
        QVERIFY(DownloadCache::pool().waitForDone(5000));
        // Shared state outlives every early assertion return and the worker itself.
        const auto unblock = qScopeGuard([gate] {
            gate->release.release();
            QVERIFY2(DownloadCache::pool().waitForDone(5000), "Download cache worker did not drain");
        });
        DownloadCache::pool().start(QRunnable::create([gate] {
            gate->entered.release();
            gate->expired.store(!gate->release.tryAcquire(1, 5000));
        }));
        QVERIFY(gate->entered.tryAcquire(1, 5000));
        QVERIFY(!gate->expired.load());
        QPointer<QDialog> dialog;
        QTimer::singleShot(0, &window, [&] {
            dialog = window.findChild<QDialog *>("sessionFileDialog");
            const QPointer<QDialog> active = qobject_cast<QDialog *>(QApplication::activeModalWidget());
            const auto close = qScopeGuard([dialog, active] {
                if (dialog && dialog->isVisible()) dialog->reject();
                else if (active && active->isVisible()) active->reject();
            });
            QVERIFY(dialog);
            auto *download = dialog->findChild<QPushButton *>("downloadSessionFile"); QVERIFY(download);
            auto *automatic = dialog->findChild<QPushButton *>("downloadOpenSessionFile"); QVERIFY(automatic);
            QVERIFY(download->isEnabled()); QVERIFY(!automatic->isEnabled());
            QCOMPARE(checked.size(), 0);
            QTimer::singleShot(0, &window, [&] {
                QPointer<QFileDialog> save = window.findChild<QFileDialog *>("sessionFileSaveDialog");
                const QPointer<QDialog> activeSave = qobject_cast<QDialog *>(QApplication::activeModalWidget());
                const auto closeSave = qScopeGuard([save, activeSave] {
                    if (save && save->isVisible()) save->reject();
                    else if (activeSave && activeSave->isVisible()) activeSave->reject();
                });
                QVERIFY(save);
                save->setOption(QFileDialog::DontConfirmOverwrite); save->selectFile(destination);
                QVERIFY(QMetaObject::invokeMethod(save, "accept", Qt::DirectConnection));
            });
            download->click();
            // Manual Save As uses raw CLI transfer, independent of the held cache pool.
            QTRY_COMPARE(downloaded.size(), 1);
            QCOMPARE(failed.size(), 0); QCOMPARE(checked.size(), 0);
            QVERIFY(!gate->expired.load());
            QCOMPARE(downloaded.first().at(1).toString(), destination);
            QFile saved(destination); QVERIFY(saved.open(QIODevice::ReadOnly));
            QCOMPARE(saved.readAll(), QByteArray("manually saved bytes"));
            QCOMPARE(transfers().size(), previousTransfers + 1);
            QVERIFY(dialog->isVisible());
            // The stale hit must actually arrive after manual completion. A miss,
            // cancellation or a lookup that won the race cannot satisfy this test.
            gate->release.release();
            QTRY_COMPARE(checked.size(), 1);
            QCOMPARE(checked.first().at(1).toString(), cached.path);
            QVERIFY(checked.first().at(2).toString().isEmpty());
            QVERIFY(checked.first().at(0).toULongLong() != downloaded.first().at(0).toULongLong());
            QVERIFY(!gate->expired.load());
            QVERIFY(DownloadCache::pool().waitForDone(5000));
            auto *action = dialog->findChild<QPushButton *>(actions[index]); QVERIFY(action);
            QVERIFY(action->isVisible()); QVERIFY(action->isEnabled()); action->click();
            QVERIFY(!dialog->isVisible());
        });
        activity->fileReferenceActivated(remote);
        QVERIFY(dialog.isNull());
        if (index == 0) {
            QTRY_COMPARE(opened.revealed.size(), previousReveals + 1);
            QCOMPARE(opened.revealed.last(), destination);
            QCOMPARE(opened.urls.size(), previousUrls);
        } else {
            QTRY_COMPARE(opened.urls.size(), previousUrls + 1);
            QCOMPARE(opened.urls.last(), QUrl::fromLocalFile(destination));
            QCOMPARE(opened.revealed.size(), previousReveals);
        }
        QCOMPARE(downloaded.size(), 1); QCOMPARE(failed.size(), 0); QCOMPARE(checked.size(), 1);
        const auto calls = transfers(); QCOMPARE(calls.size(), previousTransfers + 1);
        QCOMPARE(QJsonDocument::fromJson(calls.last()).array(),
                 (QJsonArray{"@mac", "download", "--path", remote, "--output", destination}));
        QFile oldCopy(cached.path); QVERIFY(oldCopy.open(QIODevice::ReadOnly));
        QCOMPARE(oldCopy.readAll(), QByteArray("old cached bytes"));
    }
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_files.moc"
