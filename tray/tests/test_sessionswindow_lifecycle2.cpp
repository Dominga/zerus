// U62 (#712): Archive, termination, worktrees, fork and rename, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void worktreeFilterKeepsConversationDraftAndSearchScope();
    void terminationWorktreeChoiceKeepsItsOriginalFolder_data();
    void terminationWorktreeChoiceKeepsItsOriginalFolder();
    void nativeCleanupCanSupplyAnUncachedFolderAndArchiveTransitionsExpireOnlyTheirRepository();
    void renameValidationCancelAndSelection();
    void renameValidationKeepsDialogGeometry_data();
    void renameValidationKeepsDialogGeometry();
    void forkValidationCancelAndPinnedPayload();
    void forkKeepsSourceAndGroup_data();
    void forkKeepsSourceAndGroup();
    void forkDisabledForUnavailableSources();
    void renameOtherSessionKinds();
    void renameFailureKeepsSelection();
    void terminationProgress_data();
    void terminationProgress();
    void listTerminationKeepsUnrelatedConversations_data();
    void listTerminationKeepsUnrelatedConversations();
};

void TestSessionsWindow::worktreeFilterKeepsConversationDraftAndSearchScope()
{
    auto state=fleet();auto local=state.local();local.sessions[0].cwd="/repo";local.sessions[0].canonicalCwd="/repo";local.sessions[0].gitRoot="/repo";
    local.sessions[1].cwd="/linked";local.sessions[1].canonicalCwd="/linked";local.sessions[1].gitRoot="/linked";state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    auto remote=*state.peer("mac");remote.sessions[0].cwd="/linked";remote.sessions[0].gitRoot="/linked";state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.resize(1200,850);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer, "Keep my draft");window.activateWindow();QTest::qWait(30);composer->editor()->setFocus();QTRY_COMPARE(QApplication::focusWidget(),composer->editor());
    auto *title=window.findChild<QLabel *>("detailTitle");const auto selected=title->text();
    auto *page=window.findChild<ProjectsDialog *>("projectsPage");page->folderSessionsRequested({},"/linked",true);
    for (auto *button : window.findChildren<QPushButton *>()) {
        if (button->property("filter") == "all") QCOMPARE(button->property("count").toInt(), 1);
        if (button->property("filter") == "active") QCOMPARE(button->property("count").toInt(), 0);
    }
    auto *list=window.findChild<SessionList *>("sessionList");QStringList keys;
    for(int i=0;i<list->count();++i)if(!list->item(i)->data(SessionRoles::Key).toString().isEmpty())keys<<list->item(i)->data(SessionRoles::Key).toString();
    QCOMPARE(keys,QStringList{"\nkimi/docs/research"});QCOMPARE(title->text(),selected);QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft"));QCOMPARE(QApplication::focusWidget(),composer->editor());
    window.setFleet(state);QCOMPARE(title->text(),selected);QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft"));
    window.findChild<QLineEdit *>("search")->setText("research");QTest::qWait(300);
    auto *hits=window.findChild<QListWidget *>("searchResults");QVERIFY(hits->count()>0);for(int i=0;i<hits->count();++i)QVERIFY(hits->item(i)->data(Qt::AccessibleTextRole).toString().contains("research"));
    window.findChild<QLineEdit *>("search")->clear();window.findChild<QPushButton *>("clearFolderFilter")->click();QCOMPARE(title->text(),selected);QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft"));
    page->folderSessionsRequested({},"/empty",true);QCOMPARE(title->text(),selected);QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft"));
    for (auto *button : window.findChildren<QPushButton *>())
        if (button->property("filter").isValid()) QCOMPARE(button->property("count").toInt(), 0);
    page->relatedSessionRequested({},"kimi/docs/research",{});QVERIFY(title->text()!=selected);QVERIFY(!window.findChild<QPushButton *>("clearFolderFilter")->isVisible());
    window.showSession({},"codex/hgs/dashboard");QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft"));
    page->folderSessionsRequested({},"/empty",true);window.findChild<DashboardPage *>()->filterRequested("@all","all");QVERIFY(!window.findChild<QPushButton *>("clearFolderFilter")->isVisible());
}

void TestSessionsWindow::terminationWorktreeChoiceKeepsItsOriginalFolder_data()
{
    QTest::addColumn<QString>("mode");QTest::addColumn<QString>("theme");
    for(const auto *mode:{"checked","unchecked","cancel","bulk","native-removed","native-unconfirmed"})QTest::newRow(mode)<<QString::fromLatin1(mode)<<QString("light");
    QTest::newRow("checked-dark")<<QString("checked")<<QString("dark");
}

void TestSessionsWindow::terminationWorktreeChoiceKeepsItsOriginalFolder()
{
    QFETCH(QString,mode);QFETCH(QString,theme);QSettings().setValue("workspace/theme",theme);QSettings().setValue("workspace/removeWorktreeAfterTermination",false);
    auto state=fleet();auto local=state.local();const int count=mode=="bulk"?2:1;
    QSet<QString> expected;
    for(int i=0;i<count;++i){auto &s=local.sessions[i];s.state="running";s.cmd="codex";s.tracked=true;s.runId="cleanup-run-"+QString::number(i);s.worktreeOwnerId="original-owner-"+QString::number(i);s.gitWorktree=true;s.gitRoot="/workspace/owned-"+QString::number(i);s.gitCommonDir="/workspace/project/.git";expected.insert(s.gitRoot);}
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());SessionsWindow window(program());keepDetailsPollsOut(window);window.resize(960,760);window.setFleet(state);window.show();window.showSession({},local.sessions[0].name);
    auto *client=window.findChild<HgsClient *>();QSignalSpy writes(client,&HgsClient::writeDone);
    QSignalSpy reviews(client,&HgsClient::worktreeReviewReady);
    if(mode.startsWith("native-"))connect(client,&HgsClient::writeDone,&window,[&](const QString &operation,bool ok,const QString &){
        if(ok && operation.startsWith("terminate "))client->worktreeCleanupFinished({},operation,{{"status",mode.mid(7)},{"path",local.sessions[0].gitRoot},{"common_dir",local.sessions[0].gitCommonDir},{"reason","Fixture native outcome"}});
    });
    bool checkboxSeen=false;QSet<QString> reviewed;QTimer closer;
    closer.setInterval(10);connect(&closer,&QTimer::timeout,&window,[&]{
        if(auto *dialog=window.findChild<QDialog *>("worktreeBatchCleanupDialog")){
            QVERIFY(!dialog->isVisible());QCOMPARE(dialog->windowModality(),Qt::NonModal);
            auto *list=dialog->findChild<QTreeWidget *>("batchCleanupList");
            for(int i=0;i<list->topLevelItemCount();++i){const auto entry=list->topLevelItem(i)->data(0,Qt::UserRole).toJsonObject();const auto path=entry["row"].toObject()["path"].toString();reviewed.insert(path);QCOMPARE(entry["expected_owner_id"].toString(),"original-owner-"+path.section('-',-1));}
            dialog->reject();
        }
    });closer.start();
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());QVERIFY(dialog);
        auto *choice=dialog->findChild<QCheckBox *>(mode=="bulk"?"removeTerminatedWorktrees":"removeTerminatedWorktree");QVERIFY(choice);checkboxSeen=true;
        choice->setChecked(mode!="unchecked");
        if(mode=="cancel"){dialog->done(QMessageBox::Cancel);return;}
        if(mode=="bulk"){
            for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}
        }else dialog->done(QMessageBox::Yes);
    });
    if(mode=="bulk"){
        auto *list=window.findChild<SessionList *>("sessionList");
        QListWidgetItem *second=nullptr;
        for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=="\n"+local.sessions[1].name)second=list->item(i);
        QVERIFY(second);list->scrollToItem(second);
        QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(second).center());
        QSet<QString> chosen;for(const auto *item:list->selectedItems())chosen.insert(item->data(SessionRoles::Key).toString());
        QCOMPARE(chosen,(QSet<QString>{"\n"+local.sessions[0].name,"\n"+local.sessions[1].name}));
        window.findChild<QAction *>("selectionstop")->trigger();
    }else window.findChild<QAction *>("forgetSessionAction")->trigger();
    QVERIFY(checkboxSeen);
    if(mode=="cancel"){QTest::qWait(80);QCOMPARE(writes.size(),0);QVERIFY(reviewed.isEmpty());}
    else{
        window.showSession({},local.sessions[2].name);
        QTRY_COMPARE(writes.size(),count);
        if(mode=="unchecked"){QTest::qWait(100);QVERIFY(reviewed.isEmpty());}
        else {
            QTRY_COMPARE(reviewed,expected);
            if(mode.startsWith("native-")){
                auto *dialog=window.findChild<QDialog *>("worktreeBatchCleanupDialog");QVERIFY(dialog);
                QTRY_COMPARE(dialog->findChild<QTreeWidget *>("batchCleanupList")->topLevelItem(0)->text(3),mode=="native-removed"?QString("Folder removed"):QString("Result unknown"));
                QCOMPARE(reviews.size(),0);
            }
            auto *bar=window.findChild<QFrame *>("terminationCleanupStatus");auto *label=bar->findChild<QLabel *>("terminationCleanupSummary");QVERIFY(bar->isVisible());
            QTRY_VERIFY(!label->text().startsWith("Cleaning"));
            QVERIFY(label->height()>=label->heightForWidth(label->width()));
            for(auto *button:bar->findChildren<QPushButton *>())if(button->isVisible())QVERIFY(bar->rect().contains(QRect(button->mapTo(bar,QPoint()),button->size())));
            const auto preview=qEnvironmentVariable("HGS_WORKTREE_PREVIEW");
            if(mode=="checked"){
                closer.stop();bar->findChild<QPushButton *>("reviewTerminationCleanup")->click();
                auto *review=bar->findChild<QDialog *>("worktreeBatchCleanupDialog");QVERIFY(review);QVERIFY(review->isVisible());
                const auto canvas=review->findChild<QTreeWidget *>("batchCleanupList")->viewport()->grab().toImage();
                const auto background=canvas.pixelColor(canvas.width()/2,canvas.height()-1);
                QCOMPARE(background.lightness()<128,theme=="dark");
                if(!preview.isEmpty()){
                    QDir().mkpath(preview);QVERIFY(bar->grab().save(preview+"/workspace-cleanup-"+theme+".png"));
                    QVERIFY(review->grab().save(preview+"/workspace-review-"+theme+".png"));
                }
            }
        }
    }
}

void TestSessionsWindow::nativeCleanupCanSupplyAnUncachedFolderAndArchiveTransitionsExpireOnlyTheirRepository()
{
    auto state=fleet();auto box=state.local();box.sessions[0].runId="owned-run";box.sessions[0].conversationId="owned-conversation";box.sessions[0].gitWorktree=true;box.sessions[0].gitRoot="/owned";box.sessions[0].gitCommonDir="/repo/.git";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());SessionsWindow window(program());window.setFleet(state);window.show();
    auto *client=window.findChild<HgsClient *>();QSignalSpy expired(client,&HgsClient::worktreeCacheInvalidated);
    auto completed=box;completed.sessions[0].state="archived";completed.sessions[0].gitRoot.clear();completed.sessions[0].gitWorktree=false;completed.sessions[0].runId="other-run";
    auto replacement=state;replacement.setLocal(completed,QDateTime::currentMSecsSinceEpoch());window.setFleet(replacement);QCOMPARE(expired.size(),0);
    window.setFleet(state);completed.sessions[0].runId="owned-run";replacement.setLocal(completed,QDateTime::currentMSecsSinceEpoch());window.setFleet(replacement);QCOMPARE(expired.size(),1);QCOMPARE(expired[0][1].toString(),QString("/repo/.git"));
    window.setFleet(state);window.showSession({},"codex/website/navigation");QSignalSpy writes(client,&HgsClient::writeDone);window.findChild<QAction *>("archiveSessionAction")->trigger();QTRY_COMPARE(writes.size(),1);
    auto *offer=window.findChild<QPushButton *>("archivedWorktreeCleanup");QVERIFY(!offer->isVisible());
    client->worktreeCleanupFinished({},"archive codex/website/navigation",{{"status","kept"},{"path","/uncached-worktree"},{"common_dir","/repo/.git"},{"reason","Local files remain"}});QVERIFY(offer->isVisible());QVERIFY(offer->toolTip().contains("Local files remain"));
}

void TestSessionsWindow::renameValidationCancelAndSelection()
{
    auto state = fleet(); auto box = state.local();
    auto occupied = box.sessions[0]; occupied.name = "codex/hgs/taken"; occupied.tag = "taken"; box.sessions.append(occupied);
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *action = window.findChild<QAction *>("renameSessionAction"); QVERIFY(action->isEnabled()); // Busy is allowed.
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        auto *name = dialog->findChild<QLineEdit *>("renameSessionName"); QCOMPARE(name->text(), QString("dashboard"));
        auto *save = dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Save); QVERIFY(!save->isEnabled());
        for (const auto &invalid : QStringList{"", " leading", "trailing ", "a/b", "a:b", "a.b", "a\tb", "taken"}) {
            name->setText(invalid); QVERIFY(!save->isEnabled());
            QVERIFY(!dialog->findChild<QLabel *>("renameError")->text().isEmpty());
        }
        name->setText("Новый план"); QVERIFY(save->isEnabled());
        QCOMPARE(dialog->findChild<QLabel *>("renameFullName")->text(), QString("codex/hgs/Новый план"));
        dialog->reject();
    });
    action->trigger(); QCOMPARE(writes.size(), 0);
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog); dialog->accept();
    });
    action->trigger(); QCOMPARE(writes.size(), 0); // Even a programmatic unchanged accept is a no-op.
    WAIT_SETTLED();
    client->inspectionReady({}, "codex/hgs/dashboard", QJsonObject{{"tracked", true}, {"conversation_id", "conversation-one"},
        {"cursor", 500}, {"events", QJsonArray{QJsonObject{{"seq", 500}, {"type", "Stop"}, {"detail", "old activity"}}}}});
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        dialog->findChild<QLineEdit *>("renameSessionName")->setText("Новый <план>");
        dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Save)->click();
    });
    action->trigger(); QVERIFY(!window.findChild<QPushButton *>("more")->isEnabled());
    WAIT_COMPARE(writes.size(), 1); QVERIFY(writes[0][1].toBool());
    QCOMPARE(writes[0][2].toString(), QString("rename\ncodex/hgs/dashboard\ncodex/hgs/Новый <план>"));
    auto *activity = window.findChild<QTextBrowser *>("activity");
    const QJsonObject late{{"tracked", true}, {"conversation_id", "conversation-one"}, {"cursor", 501},
        {"events", QJsonArray{QJsonObject{{"seq", 501}, {"type", "Stop"}, {"detail", "stale rename response"}}}}};
    client->inspectionReady({}, "codex/hgs/dashboard", late);
    QVERIFY(!activity->toPlainText().contains("stale rename response"));
    box.sessions[0].name = "codex/hgs/Новый <план>"; box.sessions[0].tag = "Новый <план>";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/hgs/Новый <план>"));
    auto *title = window.findChild<QLabel *>("detailTitle"); QCOMPARE(title->accessibleName(), QString("General / Новый <план>"));
    QCOMPARE(title->textFormat(), Qt::PlainText);
    WAIT_SETTLED();
    client->inspectionReady({}, box.sessions[0].name, QJsonObject{{"tracked", true}, {"conversation_id", "conversation-one"},
        {"cursor", 1}, {"events", QJsonArray{QJsonObject{{"seq", 1}, {"type", "Stop"}, {"detail", "new inspection starts at zero"}}}}});
    WAIT_VERIFY(activity->toPlainText().contains("new inspection starts at zero"));
    client->inspectionReady({}, "codex/hgs/dashboard", late);
    client->inspectionFailed({}, "codex/hgs/dashboard", "late old failure");
    QVERIFY(!activity->toPlainText().contains("stale rename response"));
    QVERIFY(!window.findChild<QLabel *>("hint")->text().contains("late old failure"));
}

void TestSessionsWindow::renameValidationKeepsDialogGeometry_data()
{
    QTest::addColumn<QString>("theme"); QTest::addColumn<int>("width");
    for (const auto &theme : {QString("dark"), QString("light")})
        for (const int width : {430, 640}) QTest::newRow(qPrintable(theme + QString::number(width))) << theme << width;
}

void TestSessionsWindow::renameValidationKeepsDialogGeometry()
{
    QFETCH(QString, theme); QFETCH(int, width); QSettings().setValue("workspace/theme", theme);
    auto state = fleet(); auto box = state.local();
    auto occupied = box.sessions[0]; occupied.name = "codex/hgs/taken"; occupied.tag = "taken"; box.sessions.append(occupied);
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/hgs/dashboard");
    QSignalSpy writes(window.findChild<HgsClient *>(), &HgsClient::writeDone);
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        const auto close = qScopeGuard([dialog] { dialog->reject(); });
        dialog->resize(width, dialog->height()); QTest::qWait(30);
        auto *name = dialog->findChild<QLineEdit *>("renameSessionName"); QVERIFY(name);
        auto *preview = dialog->findChild<QLabel *>("renameFullName"); auto *error = dialog->findChild<QLabel *>("renameError");
        auto *buttons = dialog->findChild<QDialogButtonBox *>(); QVERIFY(preview && error && buttons);
        auto *save = buttons->button(QDialogButtonBox::Save); auto *cancel = buttons->button(QDialogButtonBox::Cancel);
        const auto rectangles = [&] { return QList<QRect>{dialog->geometry(), name->geometry(), preview->geometry(),
            error->geometry(), buttons->geometry(), save->geometry(), cancel->geometry()}; };
        const auto before = rectangles();
        for (const auto &tag : QStringList{"new name", "new name ", "new name", "", " leading", "a/b", "a:b", "a.b", "a\tb", "taken", "Новый план", QString(300, 'a'), "final name"}) {
            name->setText(tag); QCoreApplication::sendPostedEvents(nullptr, QEvent::LayoutRequest); QTest::qWait(10);
            QCOMPARE(rectangles(), before);
            QCOMPARE(preview->text(), "codex/hgs/" + tag); QCOMPARE(save->isEnabled(), error->text().isEmpty());
        }
        QCOMPARE(preview->toolTip(), preview->text()); QCOMPARE(error->toolTip(), error->text());
        const auto directory = qEnvironmentVariable("HGS_RENAME_PREVIEW");
        if (!directory.isEmpty()) {
            QVERIFY(QDir().mkpath(directory)); name->setText("final name "); QTest::qWait(10);
            QVERIFY(dialog->grab().save(directory + "/rename-" + theme + QString::number(width) + ".png"));
        }
    });
    window.findChild<QAction *>("renameSessionAction")->trigger(); QCOMPARE(writes.size(), 0);
}

void TestSessionsWindow::forkValidationCancelAndPinnedPayload()
{
    auto state = fleet(); auto box = state.local();
    box.sessions[0].runId = "source-run";
    for (int i = 1; i <= 3; ++i) {
        auto collision = box.sessions[0];
        collision.tag = i == 1 ? "dashboard-fork" : QString("dashboard-fork-%1").arg(i);
        collision.name = "codex/hgs/" + collision.tag; collision.runId = QString("collision-%1").arg(i);
        collision.state = i == 3 ? "archived" : i == 2 ? "paused" : "running";
        collision.archiveId = i == 3 ? "occupied-archive-name" : "";
        box.sessions.append(collision);
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject details{{"tracked", true}, {"run_id", "source-run"}, {"conversation_id", "conversation-one"},
        {"fork_supported", true}, {"runtime_state", "live"}, {"activity", "idle"}, {"phase", "idle"},
        {"last_event_at", 2000000000}, {"events", QJsonArray{}}, {"cursor", 0}};
    // Polls (also those the dialogs' loops deliver) keep reporting fork support.
    const auto fixture = programInspecting("fork-validation-hgs", details); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(state); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    QSignalSpy writes(client, &HgsClient::writeDone); QTRY_VERIFY(!inspected.isEmpty());
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    auto *action = window.findChild<QAction *>("forkSessionAction"); QVERIFY(action && action->isEnabled());
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        QTimer::singleShot(1000, dialog, &QDialog::reject);
        QCOMPARE(dialog->objectName(), QString("forkSessionDialog"));
        auto *name = dialog->findChild<QLineEdit *>("forkSessionName"); QVERIFY(name);
        QCOMPARE(name->text(), QString("dashboard-fork-4"));
        auto *create = dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok); QVERIFY(create->isEnabled());
        for (const QString &invalid : {QString(), QString(" leading"), QString("a/b"), QString("dashboard-fork"), QString("dashboard-fork-2"), QString("dashboard-fork-3")}) {
            name->setText(invalid); QVERIFY(!create->isEnabled());
        }
        name->setText("Новая ветка"); QVERIFY(create->isEnabled()); dialog->reject();
    });
    action->trigger(); QCOMPARE(writes.size(), 0);
    QCOMPARE(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/hgs/dashboard"));
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        dialog->findChild<QLineEdit *>("forkSessionName")->setText("Новая ветка");
        dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Ok)->click();
    });
    // The fork has started; its fixture process may take a while under load.
    action->trigger(); QVERIFY(!action->isEnabled()); QTRY_COMPARE_WITH_TIMEOUT(writes.size(), 1, 15000);
    QVERIFY(writes[0][1].toBool());
    QCOMPARE(writes[0][2].toString(), QString("fork\ncodex/hgs/dashboard\n-n\nНовая ветка\n-d\n--expected-run-id\nsource-run\n--expected-conversation-id\nconversation-one"));
}

void TestSessionsWindow::forkKeepsSourceAndGroup_data()
{
    QTest::addColumn<QString>("sourceState");
    QTest::newRow("running") << QString("running");
    QTest::newRow("paused-filter") << QString("paused");
    QTest::newRow("archive-filter") << QString("archived");
}

void TestSessionsWindow::forkKeepsSourceAndGroup()
{
    QFETCH(QString, sourceState);
    SessionInfo original; original.name = "codex/project/topic"; original.tag = "topic";
    original.cmd = "codex"; original.project = "project"; original.tracked = true; original.resumable = true;
    original.runId = "original-run"; original.state = sourceState; original.activity = "idle"; original.phase = "idle";
    original.archiveId = sourceState == "archived" ? "source-archive" : ""; original.cwd = "/work/project";
    original.prompt = "Original public request";
    auto sibling = original; sibling.name = "codex/project/tail"; sibling.tag = "tail"; sibling.runId = "sibling-run";
    sibling.archiveId.clear(); sibling.state = "running";
    SessionOrganization org; const auto group = org.createGroup("My release");
    const auto sourceIdentity = org.observe("arch", original.name, original.runId, original.archiveId);
    const auto siblingIdentity = org.observe("arch", sibling.name, sibling.runId, {});
    org.moveSession(sourceIdentity, group); org.moveSession(siblingIdentity, group);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    BoxState box; box.host = "arch"; box.ok = true; box.sessions = {original, sibling};
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject details{{"tracked", true}, {"run_id", original.runId},
        {"conversation_id", "conversation-one"}, {"fork_supported", true}, {"cwd", original.cwd},
        {"activity", "idle"}, {"phase", "idle"}, {"last_event_at", 2000000000}, {"events", QJsonArray{}}, {"cursor", 0}};
    // Polls (also those the dialog's loop delivers) keep reporting fork support.
    const auto fixture = programInspecting(QString("fork-hgs-") + QTest::currentDataTag(), details); QVERIFY(!fixture.isEmpty());
    SessionsWindow window(fixture); window.setFleet(state); window.show();
    if (sourceState != "running") for (auto *button : window.findChildren<QPushButton *>())
        if (button->property("filter") == (sourceState == "archived" ? "archived" : "paused")) button->click();
    auto *list = window.findChild<SessionList *>("sessionList");
    for (int i = 0; i < list->count(); ++i)
        if (list->item(i)->data(SessionRoles::Identity).toString() == sourceIdentity) { list->setCurrentRow(i); break; }
    QVERIFY(list->currentItem());
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), sourceIdentity);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    QSignalSpy writes(client, &HgsClient::writeDone); QTRY_VERIFY(!inspected.isEmpty());
    client->inspectionReady({}, original.name, details, original.archiveId);
    auto *action = window.findChild<QAction *>("forkSessionAction"); QVERIFY(action->isEnabled());
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QLineEdit *>("forkSessionName")->text(), QString("topic-fork"));
        dialog->accept();
    });
    // The fork has started; its fixture process may take a while under load.
    action->trigger(); QVERIFY(!action->isEnabled()); QTRY_COMPARE_WITH_TIMEOUT(writes.size(), 1, 15000); QVERIFY(writes[0][1].toBool());
    if (sourceState == "archived") QVERIFY(writes[0][2].toString().contains("--archive\nsource-archive"));
    auto created = original; created.name = "codex/project/topic-fork"; created.tag = "topic-fork";
    created.runId = "independent-fork-run"; created.archiveId.clear(); created.state = "running";
    box.sessions.append(created); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QTRY_VERIFY(list->currentItem());
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/project/topic-fork"));
    QVERIFY(!list->currentItem()->isHidden());
    const auto persisted = SessionOrganization(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
    const QString createdIdentity = "arch\ncodex/project/topic-fork";
    QCOMPARE(persisted.groupFor(sourceIdentity), group); QCOMPARE(persisted.groupFor(createdIdentity), group);
    QVERIFY(persisted.group(group));
    QCOMPARE(persisted.group(group)->sessions, QStringList({sourceIdentity, createdIdentity, siblingIdentity}));
    QVERIFY(!persisted.group(group)->collapsed);
    QTRY_COMPARE(window.findChild<QTabWidget *>("sessionDetailTabs")->currentWidget()->objectName(), QString("terminalView"));
    QCOMPARE(state.local().sessions[0].name, original.name); QCOMPARE(state.local().sessions[0].runId, original.runId);
    QCOMPARE(state.local().sessions[0].archiveId, original.archiveId); QCOMPARE(state.local().sessions[0].prompt, original.prompt);
    if (sourceState == "archived") {
        for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
        for (int i = 0; i < list->count(); ++i)
            if (list->item(i)->data(SessionRoles::Identity).toString() == sourceIdentity) { list->setCurrentRow(i); break; }
        QVERIFY(list->currentItem()); QCOMPARE(list->currentItem()->data(SessionRoles::Identity).toString(), sourceIdentity);
    }
}

void TestSessionsWindow::forkDisabledForUnavailableSources()
{
    SessionsWindow window(program()); auto state = fleet(); window.setFleet(state); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    QSignalSpy writes(client, &HgsClient::writeDone); WAIT_VERIFY(!inspected.isEmpty());
    auto *action = window.findChild<QAction *>("forkSessionAction"); QVERIFY(!action->isEnabled());
    QJsonObject details{{"tracked", true}, {"run_id", "source-run"}, {"conversation_id", "conversation-one"},
        {"fork_supported", false}, {"fork_reason", "Wait for Ready to fork"}, {"activity", "busy"}, {"phase", "tool"},
        {"last_event_at", 2000000000}, {"events", QJsonArray{}}, {"cursor", 0}};
    client->inspectionReady({}, "codex/hgs/dashboard", details); QVERIFY(!action->isEnabled());
    action->trigger(); QCOMPARE(writes.size(), 0); QVERIFY(!window.findChild<QDialog *>("forkSessionDialog"));
    details["fork_reason"] = "Provider does not support independent forks";
    client->inspectionReady({}, "codex/hgs/dashboard", details); QVERIFY(!action->isEnabled());
    QVERIFY(action->toolTip().contains("Provider"));
    details["fork_supported"] = true; details["activity"] = "idle"; details["phase"] = "idle";
    client->inspectionReady({}, "codex/hgs/dashboard", details); QVERIFY(action->isEnabled());
    auto box = state.local(); box.ok = false; state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(!action->isEnabled()); action->trigger(); QCOMPARE(writes.size(), 0);
    QVERIFY(!window.findChild<QDialog *>("forkSessionDialog"));
}

void TestSessionsWindow::renameOtherSessionKinds()
{
    for (const QString kind : {QString("untracked"), QString("paused"), QString("stopped"), QString("archived")}) {
        const bool archived = kind == "archived", remote = kind == "stopped";
        SessionInfo session; session.name = "codex/project"; session.cmd = "codex"; session.project = "project";
        session.state = kind == "untracked" ? "running" : kind; session.tracked = kind != "untracked";
        session.archiveId = archived ? "archive-original" : "";
        BoxState box; box.host = remote ? "mac" : "arch"; box.ok = true; box.sessions = {session};
        if (archived) { auto live = session; live.name = "codex/project/new label"; live.state = "running"; live.archiveId.clear(); box.sessions.append(live); }
        FleetState state;
        if (remote) { BoxState local; local.host = "arch"; local.ok = true; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); state.setPeer(box, QDateTime::currentMSecsSinceEpoch()); }
        else state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
        SessionsWindow window(program()); window.setFleet(state); window.show();
        if (archived) {
            for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
            window.findChild<SessionList *>("sessionList")->setCurrentRow(0);
        }
        auto *action = window.findChild<QAction *>("renameSessionAction"); QVERIFY(action->isEnabled());
        auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
        QTimer::singleShot(0, &window, [&]() {
            auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
            QTimer::singleShot(1000, dialog, &QDialog::reject);
            auto *name = dialog->findChild<QLineEdit *>("renameSessionName"); QVERIFY(name->text().isEmpty());
            name->setText("new label"); auto *save = dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Save);
            QVERIFY(save->isEnabled()); save->click();
        });
        action->trigger(); QTRY_COMPARE(writes.size(), 1); QVERIFY(writes[0][1].toBool());
        QCOMPARE(writes[0][2].toString(), (remote ? QString("@mac\n") : QString()) + "rename\ncodex/project\ncodex/project/new label" + (archived ? "\n--archive\narchive-original" : ""));
        box.sessions[0].name = "codex/project/new label"; box.sessions[0].tag = "new label";
        if (remote) state.setPeer(box, QDateTime::currentMSecsSinceEpoch()); else state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
        window.setFleet(state);
        QCOMPARE(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(),
                 (remote ? QString("mac") : QString()) + "\ncodex/project/new label" + (archived ? "\narchive-original" : ""));
    }
}

void TestSessionsWindow::renameFailureKeepsSelection()
{
    QTemporaryDir dir; QFile fake(dir.filePath("hgs")); QVERIFY(fake.open(QIODevice::WriteOnly));
    fake.write("#!/usr/bin/env python3\nimport sys\nif sys.argv[1:2]==['inspect']:print('{\"events\":[],\"cursor\":0}')\n"
        "else:print('Name is already used by another session',file=sys.stderr);sys.exit(1)\n"); fake.close();
    QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    SessionsWindow window(FakeCli::program(fake.fileName())); window.setFleet(fleet()); window.show(); window.showSession({}, "codex/hgs/dashboard");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        dialog->findChild<QLineEdit *>("renameSessionName")->setText("racing collision"); dialog->accept();
    });
    window.findChild<QAction *>("renameSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1);
    QVERIFY(!writes[0][1].toBool()); QVERIFY(window.findChild<QPushButton *>("more")->isEnabled());
    QCOMPARE(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/hgs/dashboard"));
    QVERIFY(window.findChild<QLabel *>("notice")->text().contains("already used"));
}

void TestSessionsWindow::terminationProgress_data()
{
    QTest::addColumn<QString>("mode");
    for(const auto &mode:{"success","failure","offline","replacement","archive","search"})QTest::newRow(mode)<<QString(mode);
}

void TestSessionsWindow::terminationProgress()
{
    QFETCH(QString,mode);
    QTemporaryDir directory;QFile fake(directory.filePath("hgs"));QVERIFY(fake.open(QIODevice::WriteOnly));
    fake.write(R"(#!/usr/bin/env python3
import json,sys,time,pathlib
args=sys.argv[1:]
if args and args[0].startswith('@'):args=args[1:]
if args and args[0] in ('kill','terminate'):
 time.sleep(0.3)
 if pathlib.Path(__file__).with_name('fail').exists():
  print('Could not stop this session',file=sys.stderr);sys.exit(1)
print(json.dumps({'tracked':True,'events':[],'cursor':0}))
)");fake.close();QVERIFY(FakeCli::makeRunnable(fake.fileName()));
    if(mode=="failure"){QFile failure(directory.filePath("fail"));QVERIFY(failure.open(QIODevice::WriteOnly));}
    SessionOrganization organization;const auto group=organization.createGroup("Work");organization.moveSession("arch\ncodex/hgs/dashboard",group);
    setJsonSetting("workspace/organization",QJsonDocument(organization.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(FakeCli::program(fake.fileName()));auto state=fleet();window.setFleet(state);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *list=window.findChild<SessionList *>("sessionList");auto *client=window.findChild<HgsClient *>();
    auto *composer=window.findChild<MessageComposer *>("messageComposer");
    setComposerText(composer,"Keep my draft after termination");
    if(mode=="search")window.findChild<QLineEdit *>("search")->setText("dashboard");
    QSignalSpy writes(client,&HgsClient::writeDone);QSignalSpy refresh(&window,&SessionsWindow::refreshRequested);
    const auto row=[&]() -> QListWidgetItem * {for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=="\ncodex/hgs/dashboard")return list->item(i);return nullptr;};
    QTimer::singleShot(0,&window,[&]{auto *question=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());QVERIFY(question);question->button(QMessageBox::Yes)->click();});
    window.findChild<QAction *>("forgetSessionAction")->trigger();
    QVERIFY(row());QCOMPARE(row()->data(SessionRoles::Status).toString(),QString("Terminating…"));
    QCOMPARE(window.findChild<QLabel *>("badge")->text(),QString("Terminating…"));
    QVERIFY(!window.findChild<QAction *>("pauseAction")->isEnabled());
    window.setFleet(state);QCOMPARE(row()->data(SessionRoles::Status).toString(),QString("Terminating…"));
    QTRY_COMPARE(writes.size(),1);QVERIFY(!refresh.isEmpty());
    if(mode=="failure"){
        QVERIFY(row()->data(SessionRoles::Status).toString()!="Terminating…");
        QCOMPARE(composer->sessionKey(),QString("\ncodex/hgs/dashboard"));
        QCOMPARE(composer->editor()->toPlainText(),QString("Keep my draft after termination"));
        bool error=false;for(auto *label:window.findChildren<QLabel *>())error|=label->text().contains("Could not stop this session");QVERIFY(error);return;
    }
    window.setFleet(state);QCOMPARE(row()->data(SessionRoles::Status).toString(),QString("Terminating…"));
    // A second activation while waiting for the next snapshot must not issue another kill.
    window.findChild<QAction *>("forgetSessionAction")->trigger();QCOMPARE(writes.size(),1);
    auto local=state.local();
    if(mode=="offline"){
        local.ok=false;state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
        QCOMPARE(row()->data(SessionRoles::Status).toString(),QString("Terminating…"));local.ok=true;
    }
    if(mode=="replacement"){
        local.sessions[0].runId="replacement-run";local.sessions[0].created+=1;
        state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
        QVERIFY(row());QVERIFY(row()->data(SessionRoles::Status).toString()!="Terminating…");return;
    }
    if(mode=="archive"){
        auto archived=local.sessions[0];archived.state="archived";archived.archiveId="terminated-archive";local.sessions.append(archived);
    }
    local.sessions.removeFirst();state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QVERIFY(!row());
    QVERIFY(!list->currentItem());QVERIFY(list->selectedItems().isEmpty());
    QCOMPARE(composer->sessionKey(),QString());
    QCOMPARE(window.findChild<QStackedWidget *>("detail")->currentIndex(),0);
    ComposerDraftStore::flushWrites();
    QCOMPARE(ComposerDraftStore().load("\ncodex/hgs/dashboard").text,QString("Keep my draft after termination"));
    // Polling and a newly arriving session cannot choose another conversation.
    window.setFleet(state);QVERIFY(!list->currentItem());QCOMPARE(composer->sessionKey(),QString());
    auto incoming=local.sessions[0];incoming.name="codex/new/incoming";local.sessions.append(incoming);
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    window.findChild<QLineEdit *>("search")->clear();
    QVERIFY(!list->currentItem());QCOMPARE(composer->sessionKey(),QString());
    if(mode=="archive"){
        SessionOrganization saved(QJsonDocument::fromJson(jsonSetting("workspace/organization")).object());
        QCOMPARE(saved.groupFor("arch\narchive\nterminated-archive"),group);
    }
    QListWidgetItem *next=nullptr;
    for(int i=0;i<list->count();++i)if(!list->item(i)->isHidden()&&(list->item(i)->flags()&Qt::ItemIsSelectable)){next=list->item(i);break;}
    QVERIFY(next);const auto nextKey=next->data(SessionRoles::Key).toString();list->scrollToItem(next);
    QTest::mouseClick(list->viewport(),Qt::LeftButton,{},list->visualItemRect(next).center());
    QCOMPARE(composer->sessionKey(),nextKey);QCOMPARE(window.findChild<QStackedWidget *>("detail")->currentIndex(),1);
}

void TestSessionsWindow::listTerminationKeepsUnrelatedConversations_data()
{
    QTest::addColumn<QString>("mode");
    for(const auto *mode:{"context","bulk-current","bulk-other"})QTest::newRow(mode)<<QString::fromLatin1(mode);
}

void TestSessionsWindow::listTerminationKeepsUnrelatedConversations()
{
    QFETCH(QString,mode);
    auto state=fleet();auto local=state.local();
    SessionsWindow window(program());window.setFleet(state);window.resize(1100,850);window.show();window.showSession({},local.sessions[0].name);
    auto *list=window.findChild<SessionList *>("sessionList");auto *composer=window.findChild<MessageComposer *>("messageComposer");
    const auto key=composer->sessionKey();setComposerText(composer,"Keep the current draft");
    const auto toggle=[&](int index){
        QListWidgetItem *item=nullptr;
        for(int i=0;i<list->count();++i)if(list->item(i)->data(SessionRoles::Key).toString()=='\n'+local.sessions[index].name){item=list->item(i);break;}
        QVERIFY(item);list->scrollToItem(item);QTest::mouseClick(list->viewport(),Qt::LeftButton,Qt::ControlModifier,list->visualItemRect(item).center());
    };
    QSet<QString> removed;
    QAction *action=nullptr;
    if(mode=="context"){
        QTest::keyClick(list,Qt::Key_Menu);auto *menu=window.findChild<QMenu *>("sessionContextMenu");
        action=menu->findChild<QAction *>("contextTerminateSession");removed={local.sessions[0].name};menu->hide();
    }else{
        if(mode=="bulk-other"){toggle(0);toggle(1);toggle(3);removed={local.sessions[1].name,local.sessions[3].name};}
        else{toggle(1);removed={local.sessions[0].name,local.sessions[1].name};}
        QSet<QString> chosen;for(const auto *item:list->selectedItems())chosen.insert(item->data(SessionRoles::Key).toString().section('\n',1,1));
        QCOMPARE(chosen,removed);
        action=window.findChild<QAction *>("selectionstop");
    }
    QVERIFY(action);QVERIFY(action->isEnabled());
    QTimer::singleShot(0,&window,[&]{auto *dialog=qobject_cast<QMessageBox *>(QApplication::activeModalWidget());QVERIFY(dialog);
        if(auto *yes=dialog->button(QMessageBox::Yes))yes->click();
        else for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();break;}
    });
    QSignalSpy writes(window.findChild<HgsClient *>(),&HgsClient::writeDone);action->trigger();QTRY_COMPARE(writes.size(),removed.size());
    for(const auto &write:writes)QVERIFY(write[1].toBool());
    local.sessions.removeIf([&](const SessionInfo &session){return removed.contains(session.name);});
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);window.setFleet(state);
    if(mode=="bulk-other"){
        QCOMPARE(composer->sessionKey(),key);QCOMPARE(composer->editor()->toPlainText(),QString("Keep the current draft"));
        QVERIFY(list->currentItem());QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(),key);
    }else{
        QVERIFY(!list->currentItem());QVERIFY(list->selectedItems().isEmpty());QCOMPARE(composer->sessionKey(),QString());
        QCOMPARE(window.findChild<QStackedWidget *>("detail")->currentIndex(),0);
    }
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_lifecycle2.moc"
