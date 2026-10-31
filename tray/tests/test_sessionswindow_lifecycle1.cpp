// U62 (#712): Archive, termination, worktrees, fork and rename, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void archiveRestoreKeepsDraftAndConversation_data();
    void archiveRestoreKeepsDraftAndConversation();
    void archiveRestoresKeepIndependentDraftHandoffs();
    void restoredRunRequiresMatchingInspectionBeforeNativeControls();
    void clearArchiveKeepsLiveAndNewEntries();
    void archiveFiltersIdentityRestoreAndForget();
    void savedSessionsCanMoveToArchive();
    void archivingLastWorktreeSessionOffersCleanup();
    void keptWorktreeFooterFitsAndNamesItsSource_data();
    void keptWorktreeFooterFitsAndNamesItsSource();
    void keptWorktreeReviewFollowsItsStoredTargetAndRechecksOccupants();
    void keptWorktreeReviewUsesOnlyCurrentInspectionOccupancy();
    void keptWorktreeReviewFormatsTheSourceMachinePath();
    void terminatingWorktreeSessionOffersRemainingCleanupAndConsumesExactNativeOutcome();
    void batchActionsAndReturnToSessions();
};

void TestSessionsWindow::archiveRestoreKeepsDraftAndConversation_data()
{
    QTest::addColumn<bool>("navigateAway");
    QTest::addColumn<bool>("sameConversation");
    QTest::addColumn<bool>("destinationDraft");
    QTest::addColumn<QString>("scopeChange");
    QTest::newRow("same-conversation") << false << true << false << QString();
    QTest::newRow("newer-user-selection") << true << true << false << QString();
    QTest::newRow("different-conversation") << false << false << false << QString();
    QTest::newRow("existing-live-draft") << false << true << true << QString();
    QTest::newRow("filter-before-live-row") << false << true << false << QString("filter");
    QTest::newRow("search-before-live-row") << false << true << false << QString("search");
    QTest::newRow("preserved-archive-search") << false << true << false << QString("preserved-search");
    QTest::newRow("selection-toolbar-restore") << false << true << false << QString("bulk");
    QTest::newRow("selection-toolbar-disk-only") << false << true << false << QString("bulk-disk-only");
}

void TestSessionsWindow::archiveRestoreKeepsDraftAndConversation()
{
    QFETCH(bool, navigateAway);
    QFETCH(bool, sameConversation);
    QFETCH(bool, destinationDraft);
    QFETCH(QString, scopeChange);
    auto state = fleet(); auto local = state.local();
    auto archived = local.sessions[0];
    // Data rows share the isolated store, so each needs independent disk keys.
    archived.name = "codex/hgs/restore-draft-" + QString::fromLatin1(QTest::currentDataTag()); archived.tag = "restore-draft";
    archived.state = "archived"; archived.archiveId = "draft-archive";
    archived.runId = "archived-run"; archived.conversationId = "conversation-one";
    auto other = local.sessions[1];
    const bool diskOnly = scopeChange == "bulk-disk-only";
    if (diskOnly) {
        other.state = "archived"; other.archiveId = "other-retained-archive";
        other.runId = "other-archived-run"; other.conversationId = "other-conversation";
    }
    const QString otherKey = '\n' + other.name + (diskOnly ? '\n' + other.archiveId : QString());
    local.sessions = diskOnly ? QList<SessionInfo>{other} : QList<SessionInfo>{archived, other};
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    const QString oldKey = "\n" + archived.name + "\n" + archived.archiveId;
    const QString liveKey = "\n" + archived.name;
    ComposerDraftStore store; ComposerDraft draft;
    draft.text = "Keep this draft and selection"; draft.anchor = 2; draft.position = 9;
    draft.attachments = {{"notes.txt", "text/plain", "retained bytes", {}}};
    draft.nextAttachmentNumber = 7;
    QVERIFY(store.save(oldKey, draft));
    draft = store.load(oldKey);
    QVERIFY(draft.storageError.isEmpty()); QCOMPARE(draft.attachments.size(), 1);
    QCOMPARE(draft.attachments.first().data, QByteArray("retained bytes"));
    if (destinationDraft) { ComposerDraft newer; newer.text = "Existing live draft"; QVERIFY(store.save(liveKey, newer)); }
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    auto *list = window.findChild<QListWidget *>("sessionList");
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *activity = window.findChild<ActivityView *>("mainActivity");
    auto *machines = window.findChild<MachineFilter *>("machineFilterChips");
    QVERIFY(client && list && composer && activity && machines);
    machines->setSelection({"@local"});
    QPushButton *archiveFilter = nullptr;
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") archiveFilter = button;
    QVERIFY(archiveFilter); archiveFilter->click();
    auto *search = window.findChild<QLineEdit *>("search"); QVERIFY(search);
    if (scopeChange == "preserved-search") search->setText("restore-draft");
    QCOMPARE(list->count(), 1);
    if (diskOnly) {
        // The target has never appeared in this window: opening Archive cannot
        // preload its saved draft before the background Restore path is tested.
        QCOMPARE(list->item(0)->data(Qt::UserRole).toString(), otherKey);
        list->setCurrentRow(0);
        WAIT_COMPARE(composer->sessionKey(), otherKey);
        WAIT_VERIFY(!composer->isDraftLoading(otherKey));
        local.sessions = {archived, other}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
        QCOMPARE(list->count(), 2); QCOMPARE(composer->sessionKey(), otherKey);
        list->clearSelection();
        QListWidgetItem *target = nullptr;
        for (int row = 0; row < list->count(); ++row)
            if (list->item(row)->data(Qt::UserRole).toString() == oldKey) target = list->item(row);
        QVERIFY(target); QVERIFY(!target->isHidden());
        QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::ControlModifier, list->visualItemRect(target).center());
        WAIT_COMPARE(list->selectedItems().size(), 1);
        QCOMPARE(list->selectedItems().first()->data(Qt::UserRole).toString(), oldKey);
        QVERIFY(composer->sessionKey() != oldKey);
    } else {
        list->setCurrentRow(0);
        WAIT_COMPARE(composer->sessionKey(), oldKey);
        WAIT_VERIFY(!composer->isDraftLoading(oldKey));
        QCOMPARE(composer->editor()->toPlainText(), draft.text);
        QCOMPARE(composer->editor()->textCursor().anchor(), draft.anchor);
        QCOMPARE(composer->editor()->textCursor().position(), draft.position);
    }
    WAIT_VERIFY(!activity->renderPending());
    const auto history = activity->plainText(); QVERIFY(!history.isEmpty());
    QSignalSpy terminal(&window, &SessionsWindow::sessionActivated);
    auto *restore = window.findChild<QPushButton *>("restoreSessionAction");
    QVERIFY(restore);
    if (scopeChange == "bulk" || scopeChange == "bulk-disk-only") {
        auto *resume = window.findChild<QAction *>("selectionplay"); QVERIFY(resume && resume->isEnabled()); resume->trigger();
    } else { QVERIFY(restore->isEnabled()); restore->click(); }
    WAIT_COMPARE(writes.size(), 1);
    QCOMPARE(writes[0][0].toString(), "restore " + archived.name); QVERIFY(writes[0][1].toBool());
    if (navigateAway) {
        window.showSession({}, other.name);
        // showSession is an explicit global navigation action; reapply the
        // user's machine scope before the later Restore result arrives.
        machines->setSelection({"@local"});
    }
    if (scopeChange == "filter") {
        for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "paused") button->click();
    } else if (scopeChange == "search") {
        search->setFocus(); QTest::keyClicks(search, "restore-draft");
    }
    auto restored = archived; restored.state = "running"; restored.archiveId.clear(); restored.runId = "resumed-run";
    if (!sameConversation) restored.conversationId = "different-conversation";
    local.sessions = {restored, other}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(machines->selection(), QSet<QString>{"@local"});
    QCOMPARE(terminal.size(), 0); QCOMPARE(writes.size(), 1);
    if (scopeChange == "filter" || scopeChange == "search" || scopeChange == "bulk" || scopeChange == "bulk-disk-only") {
        WAIT_COMPARE(store.load(liveKey).text, draft.text);
        QVERIFY(store.load(oldKey).text.isEmpty());
        if (scopeChange == "bulk" || scopeChange == "bulk-disk-only") QVERIFY(archiveFilter->isChecked());
        else if (scopeChange == "filter") {
            for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "paused") QVERIFY(button->isChecked());
        } else QCOMPARE(search->text(), QString("restore-draft"));
        // The user's scope change canceled follow-navigation, not the handoff.
        QVERIFY(composer->sessionKey() != liveKey);
        if (diskOnly) QCOMPARE(composer->sessionKey(), otherKey);
        return;
    }
    if (scopeChange == "preserved-search") {
        QCOMPARE(search->text(), QString("restore-draft"));
        auto *results = window.findChild<SessionSearch *>(); QVERIFY(results && results->active());
    }
    if (!sameConversation) {
        QVERIFY(composer->sessionKey() != liveKey);
        QCOMPARE(store.load(oldKey).text, draft.text);
        QVERIFY(store.load(liveKey).text.isEmpty());
        return;
    }
    const auto expectedKey = navigateAway ? "\n" + other.name : liveKey;
    WAIT_COMPARE(composer->sessionKey(), expectedKey);
    WAIT_VERIFY(!composer->isDraftLoading(liveKey));
    if (destinationDraft) {
        QCOMPARE(store.load(oldKey).text, draft.text);
        QCOMPARE(store.load(liveKey).text, QString("Existing live draft"));
        WAIT_COMPARE(composer->editor()->toPlainText(), QString("Existing live draft"));
    } else {
        WAIT_COMPARE(store.load(liveKey).text, draft.text);
        const auto written = store.load(liveKey);
        QCOMPARE(written.attachments.size(), 1); QCOMPARE(written.attachments.first().data, QByteArray("retained bytes"));
        QCOMPARE(written.attachmentHashes, draft.attachmentHashes); QCOMPARE(written.nextAttachmentNumber, 7);
        QCOMPARE(written.anchor, draft.anchor); QCOMPARE(written.position, draft.position);
        QVERIFY(store.load(oldKey).text.isEmpty());
        if (!navigateAway) {
            WAIT_COMPARE(composer->editor()->toPlainText(), draft.text);
            QCOMPARE(composer->editor()->textCursor().anchor(), draft.anchor);
            QCOMPARE(composer->editor()->textCursor().position(), draft.position);
            QVERIFY(activity->plainText().contains(history));
        }
    }
}

void TestSessionsWindow::archiveRestoresKeepIndependentDraftHandoffs()
{
    auto state = fleet(); auto local = state.local();
    auto first = local.sessions[0], second = first;
    first.name = "codex/hgs/restore-independent-a"; second.name = "codex/hgs/restore-independent-b";
    first.state = second.state = "archived";
    first.archiveId = "independent-a"; second.archiveId = "independent-b";
    first.conversationId = "conversation-a"; second.conversationId = "conversation-b";
    local.sessions = {first, second}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    ComposerDraftStore store; ComposerDraft a, b; a.text = "First independent draft"; b.text = "Second independent draft";
    const QString oldA = '\n' + first.name + '\n' + first.archiveId, oldB = '\n' + second.name + '\n' + second.archiveId;
    QVERIFY(store.save(oldA, a)); QVERIFY(store.save(oldB, b));
    QFile source(script()); QVERIFY(source.open(QIODevice::ReadOnly));
    const QString fixture = m_dir->filePath("independent-restores");
    const QByteArray inspect = "elif c=='inspect':\n s=a[1].rsplit('-',1)[-1];print(json.dumps({'tracked':True,'conversation_id':'conversation-'+s,'events':[],'cursor':0}))\n";
    QVERIFY(FakeCli::install(fixture, source.readAll().replace("if False:pass\n", "if False:pass\n" + inspect)));
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *restore = window.findChild<QPushButton *>("restoreSessionAction");
    auto *list = window.findChild<QListWidget *>("sessionList");
    QVERIFY(client && composer && restore && list); QSignalSpy writes(client, &HgsClient::writeDone);
    auto selectArchive = [&](const QString &key) {
        for (int row = 0; row < list->count(); ++row) if (list->item(row)->data(Qt::UserRole).toString() == key) { list->setCurrentRow(row); return true; }
        return false;
    };
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
    QVERIFY(selectArchive(oldA)); QTRY_COMPARE(composer->sessionKey(), oldA); QTRY_VERIFY(!composer->isDraftLoading(oldA));
    restore->click(); QTRY_COMPARE(writes.size(), 1); QVERIFY(writes[0][1].toBool());
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
    QVERIFY(selectArchive(oldB)); QTRY_COMPARE(composer->sessionKey(), oldB); QTRY_VERIFY(!composer->isDraftLoading(oldB));
    restore->click(); QTRY_COMPARE(writes.size(), 2); QVERIFY(writes[1][1].toBool());
    first.state = second.state = "running"; first.archiveId.clear(); second.archiveId.clear();
    first.runId = "new-a"; second.runId = "new-b";
    local.sessions = {first, second}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QTRY_COMPARE(store.load('\n' + first.name).text, a.text);
    QTRY_COMPARE(store.load('\n' + second.name).text, b.text);
    QVERIFY(store.load(oldA).text.isEmpty()); QVERIFY(store.load(oldB).text.isEmpty());
    QTRY_COMPARE(composer->sessionKey(), '\n' + second.name);
    QCOMPARE(writes.size(), 2);
}

void TestSessionsWindow::restoredRunRequiresMatchingInspectionBeforeNativeControls()
{
    auto state = fleet(); auto local = state.local(); auto session = local.sessions[0];
    session.name = "codex/hgs/restore-held-inspection"; session.state = "archived";
    session.archiveId = "held-archive"; session.runId = "old-run"; session.conversationId = "held-conversation";
    local.sessions = {session}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject old{{"tracked",true},{"conversation_id",session.conversationId},{"run_id",session.runId},
        {"state","archived"},{"phase","working"},{"activity","working"},{"process_state","running"},
        {"runtime_state","live"},{"interrupt_supported",true},{"terminal_supported",true},{"events",QJsonArray{}},{"cursor",0}};
    QFile source(script()); QVERIFY(source.open(QIODevice::ReadOnly)); const QString fixture = m_dir->filePath("restore-held-inspection");
    const QByteArray inspect = "elif c=='inspect':sys.stdout.write(open(__file__+'.answer').read())\n";
    QVERIFY(FakeCli::install(fixture, source.readAll().replace("if False:pass\n", "if False:pass\n" + inspect)));
    auto answer = [&](const QJsonObject &data) {
        QSaveFile file(fixture + ".answer"); if (!file.open(QIODevice::WriteOnly)) return false;
        const auto bytes = QJsonDocument(data).toJson(QJsonDocument::Compact);
        return file.write(bytes) == bytes.size() && file.commit();
    };
    QVERIFY(answer(old));
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(state); window.show();
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *restore = window.findChild<QPushButton *>("restoreSessionAction"); auto *terminal = window.findChild<TerminalView *>();
    auto *open = window.findChild<QAction *>("openSessionAction"); auto *stop = composer ? composer->findChild<QPushButton *>("interruptAgent") : nullptr;
    auto *connect = terminal ? terminal->findChild<QPushButton *>("terminalConnect") : nullptr;
    QVERIFY(client && composer && restore && terminal && open && stop && connect);
    QSignalSpy writes(client, &HgsClient::writeDone), ready(client, &HgsClient::inspectionReady), opened(&window, &SessionsWindow::sessionActivated);
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list); QCOMPARE(list->count(), 1); list->setCurrentRow(0);
    QTRY_COMPARE(composer->sessionKey(), '\n' + session.name + '\n' + session.archiveId);
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    restore->click(); QTRY_COMPARE(writes.size(), 1); QVERIFY(writes[0][1].toBool());
    const auto archiveId = session.archiveId;
    session.state = "running"; session.archiveId.clear(); session.runId = "restored-run";
    local.sessions = {session}; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    const QString key = '\n' + session.name; QTRY_COMPARE(composer->sessionKey(), key);
    // The exact current inspection is held at the fixture source; old-run
    // replies still arrive and must not unlock controls or replace the view.
    QTRY_VERIFY(std::any_of(ready.cbegin(),ready.cend(),[&](const QList<QVariant> &row) { return row[1].toString()==session.name && row[3].toString().isEmpty(); }));
    QVERIFY(!stop->isEnabled()); QVERIFY(!open->isEnabled()); QVERIFY(!connect->isEnabled());
    terminal->connectSession(); QVERIFY(!terminal->isConnected());
    composer->interruptRequested(key); QCOMPARE(opened.size(), 0); QCOMPARE(writes.size(), 1);
    client->inspectionReady({}, session.name, old, archiveId);
    client->inspectionReady({}, session.name, old, {});
    QVERIFY(!open->isEnabled()); QVERIFY(!stop->isEnabled());
    auto current = old; current["state"] = "running"; current["run_id"] = session.runId;
    auto unavailable = current; unavailable["inspection_unavailable"] = true;
    client->inspectionReady({}, session.name, unavailable, {}); QVERIFY(!open->isEnabled());
    QVERIFY(answer(current)); settleSelectedInspection(composer, client, {}, session.name, current);
    QTRY_VERIFY(open->isEnabled()); QVERIFY(stop->isEnabled()); QVERIFY(connect->isEnabled());
    // Ordinary later inspection errors must not remove a verified Terminal.
    client->inspectionFailed({}, session.name, "Synthetic temporary inspection failure", {});
    QVERIFY(open->isEnabled()); QVERIFY(connect->isEnabled());
    QCOMPARE(opened.size(), 0); QCOMPARE(writes.size(), 1);
}

void TestSessionsWindow::clearArchiveKeepsLiveAndNewEntries()
{
    auto state=fleet();auto box=state.local();auto archived=box.sessions[0];archived.state="archived";archived.archiveId="one";
    auto second=archived;second.archiveId="two";box.sessions<<archived<<second;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    auto remote=*state.peer("mac");auto remoteArchive=archived;remoteArchive.archiveId="remote";remote.sessions<<remoteArchive;state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    remote.ok=false;state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.show();
    auto *clear=window.findChild<QAction *>("clearArchive");QVERIFY(clear);
    auto *context=window.findChild<QMenu *>("sessionProjectsMenu");QVERIFY(context);QVERIFY(context->actions().contains(clear));
    auto *batch=window.findChild<QPushButton *>("batchActions")->menu();
    // Archive cleanup is available from every filter, including the live list.
    for(auto *button:window.findChildren<QPushButton *>("sessionFilter")) {
        button->click();QVERIFY(clear->isVisible());QVERIFY(clear->isEnabled());
        QMetaObject::invokeMethod(batch,"aboutToShow");QVERIFY(batch->actions().contains(clear));
    }
    window.showSession({},box.sessions.first().name);
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({"mac"});QVERIFY(clear->isVisible());QVERIFY(!clear->isEnabled());
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({});QVERIFY(clear->isEnabled());
    auto *client=window.findChild<HgsClient *>();QSignalSpy writes(client,&HgsClient::writeDone);
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("confirmSelectionAction");QVERIFY(dialog);dialog->reject();});clear->trigger();QCOMPARE(writes.size(),0);
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=window.findChild<QMessageBox *>("confirmSelectionAction");QVERIFY(dialog);
        QVERIFY(dialog->text().contains("2 archived"));QVERIFY(dialog->text().contains("1 unavailable"));
        auto newer=archived;newer.archiveId="created-during-confirmation";box.sessions<<newer;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();break;}
    });clear->trigger();QTRY_COMPARE(writes.size(),2);
    QStringList payloads;for(const auto &write:writes)payloads<<write[2].toString();
    QVERIFY(payloads.contains("kill\ncodex/hgs/dashboard\n--archive\none"));QVERIFY(payloads.contains("kill\ncodex/hgs/dashboard\n--archive\ntwo"));
    QTest::qWait(30);QCOMPARE(writes.size(),2);
}

void TestSessionsWindow::archiveFiltersIdentityRestoreAndForget()
{
    auto state = fleet(); auto local = state.local();
    auto old = local.sessions[0]; old.state = "archived"; old.archiveId = "older"; old.archivedAt = 1790928000;
    auto recent = old; recent.archiveId = "recent"; recent.archivedAt += 10;
    local.sessions.append(old); local.sessions.append(recent); state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.setFleet(state); keepDetailsPollsOut(window); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList");
    auto filter = [&](const QString &id) -> QPushButton * {
        for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == id) return button;
        return nullptr;
    };
    QCOMPARE(list->count(), 5); QVERIFY(filter("all")->text().endsWith("5"));
    QVERIFY(filter("archived")->text().endsWith("2"));
    filter("paused")->click(); QCOMPARE(list->count(), 1);
    filter("archived")->click(); QCOMPARE(list->count(), 2);
    QCOMPARE(list->item(0)->data(Qt::UserRole).toString(), QString("\ncodex/hgs/dashboard\nrecent"));
    QCOMPARE(list->item(1)->data(Qt::UserRole).toString(), QString("\ncodex/hgs/dashboard\nolder"));
    list->setCurrentRow(0); // Filtering alone keeps the live conversation open.
    // An archived session keeps Restore in the header; the menu names the same action.
    auto *restore = window.findChild<QPushButton *>("restoreSessionAction"); QVERIFY(restore->isVisible());
    QCOMPARE(restore->accessibleName(), QString("Restore session")); QVERIFY(!restore->isEnabled());
    QCOMPARE(window.findChild<QAction *>("pauseAction")->text(), QString("Restore session"));
    QVERIFY(!window.findChild<QPushButton *>("clearSessionAction")->isVisible());
    QVERIFY(window.findChild<QLabel *>("hint")->text().contains("uses this name"));
    QVERIFY(!window.findChild<QAction *>("archiveSessionAction")->isVisible());
    QSignalSpy terminal(&window, &SessionsWindow::sessionActivated);
    QSignalSpy writes(client, &HgsClient::writeDone);
    QTest::keyClick(list, Qt::Key_Return); QCOMPARE(terminal.size(), 0); QCOMPARE(writes.size(), 0);
    QTest::mouseDClick(list->viewport(), Qt::LeftButton, {}, list->visualItemRect(list->item(0)).center());
    QCOMPARE(terminal.size(), 0); QCOMPARE(writes.size(), 0);
    // Wait for this archive's fixture reply before injecting late replies. A
    // fixed delay can let a slow subprocess replace the injected details. Selecting
    // the archive started the only inspection of it: with polls kept out, none
    // follows that reply to replace the injected prompt with the fixture's details.
    WAIT_VERIFY(std::any_of(inspected.cbegin(), inspected.cend(), [&](const QList<QVariant> &reply) {
        return reply[0].toString().isEmpty() && reply[1].toString() == recent.name
            && reply[3].toString() == recent.archiveId;
    }));
    auto *activity = window.findChild<QTextBrowser *>("activity");
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"prompt", "wrong live request"}, {"events", QJsonArray{}}, {"cursor", 0}};
    client->inspectionReady({}, recent.name, details); QVERIFY(!activity->toPlainText().contains("wrong live request"));
    details["prompt"] = "wrong older archive";
    client->inspectionReady({}, recent.name, details, "older"); QVERIFY(!activity->toPlainText().contains("wrong older archive"));
    details["prompt"] = "selected archived request";
    client->inspectionReady({}, recent.name, details, "recent"); WAIT_VERIFY(activity->toPlainText().contains("selected archived request"));
    client->inspectionFailed({}, recent.name, "wrong failure", "older");
    QVERIFY(!window.findChild<QLabel *>("hint")->text().contains("wrong failure"));
    // A paused binding occupies the name just as a live terminal does.
    local.sessions[0].state = "paused"; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(!restore->isEnabled());
    // Free the current name; Archive restoration still requires its dedicated button.
    local.sessions.removeFirst(); state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(restore->isEnabled()); restore->click(); WAIT_COMPARE(writes.size(), 1);
    QCOMPARE(writes[0][2].toString(), QString("resume\ncodex/hgs/dashboard\n--archive\nrecent\n-d"));
    QVERIFY(filter("all")->isChecked()); QCOMPARE(terminal.size(), 0);
    auto restored = recent; restored.state = "running"; restored.archiveId.clear(); restored.archivedAt = 0;
    local.sessions.removeLast(); local.sessions.append(restored);
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/hgs/dashboard"));
    filter("archived")->click(); QCOMPARE(list->count(), 1); list->setCurrentRow(0);
    QVERIFY(!restore->isEnabled()); // The newly restored terminal occupies the name.
    QTimer::singleShot(0, [&]() {
        if (auto *dialog = qobject_cast<QMessageBox *>(QApplication::activeModalWidget())) dialog->button(QMessageBox::Yes)->click();
    });
    window.findChild<QAction *>("forgetSessionAction")->trigger(); WAIT_COMPARE(writes.size(), 2);
    QCOMPARE(writes[1][2].toString(), QString("kill\ncodex/hgs/dashboard\n--archive\nolder"));
    // Archive-only machines never enable Resume all.
    BoxState archiveOnly; archiveOnly.host = "arch"; archiveOnly.ok = true; archiveOnly.sessions = {old};
    FleetState finalState; finalState.setLocal(archiveOnly, QDateTime::currentMSecsSinceEpoch()); window.setFleet(finalState);
    auto *menu = window.findChild<QPushButton *>("batchActions")->menu(); QMetaObject::invokeMethod(menu, "aboutToShow");
    QCOMPARE(menu->actions()[0]->menu()->actions().last()->text(), QString("Resume all in background (0)"));
    QVERIFY(!menu->actions()[0]->menu()->actions().last()->isEnabled());
}

void TestSessionsWindow::savedSessionsCanMoveToArchive()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/website/navigation");
    auto *action = window.findChild<QAction *>("archiveSessionAction"); QVERIFY(action->isVisible());
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    action->trigger(); QTRY_COMPARE(writes.size(), 1);
    QCOMPARE(writes[0][2].toString(), QString("archive\ncodex/website/navigation"));
    window.showSession({}, "codex/hgs/dashboard"); QVERIFY(!action->isVisible());
    action->trigger(); QTest::qWait(25); QCOMPARE(writes.size(), 1); // Live sessions cannot be archived from this action.
}

void TestSessionsWindow::archivingLastWorktreeSessionOffersCleanup()
{
    for (const bool shared : {false, true}) {
        auto state = fleet(); auto box = state.local();
        for (auto &session : box.sessions) {
            if (session.name == "codex/website/navigation") { session.runId = "archived-origin-run"; session.gitWorktree = true; session.gitRoot = "/workspace/website-nav"; session.gitCommonDir = "/workspace/website/.git"; }
            else if (shared && session.name == "codex/hgs/dashboard") session.gitRoot = "/workspace/website-nav";
        }
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
        SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/website/navigation");
        auto *offer = window.findChild<QPushButton *>("archivedWorktreeCleanup"); QVERIFY(!offer->isVisible());
        auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
        window.findChild<QAction *>("archiveSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1); QVERIFY(writes[0][1].toBool());
        if (shared) { QTest::qWait(50); QVERIFY(!offer->isVisible()); continue; } // Another session still uses the checkout.
        QTRY_VERIFY(offer->isVisible()); QVERIFY(offer->toolTip().contains("/workspace/website-nav")); bool opened = false;
        QTimer::singleShot(0, &window, [&] {
            auto *dialog = window.findChild<QDialog *>("worktreeBatchCleanupDialog"); QVERIFY(dialog); QTimer::singleShot(3000, dialog, [dialog] { dialog->done(0); });
            QCOMPARE(dialog->windowTitle(), QString("Remove working folders"));
            const auto target = dialog->findChild<QTreeWidget *>("batchCleanupList")->topLevelItem(0)->data(0,Qt::UserRole).toJsonObject();
            QCOMPARE(target["row"].toObject()["path"].toString(),QString("/workspace/website-nav")); opened = true; dialog->reject();
        });
        offer->click(); QVERIFY(opened); QVERIFY(!offer->isVisible());
    }
}

void TestSessionsWindow::keptWorktreeFooterFitsAndNamesItsSource_data()
{
    QTest::addColumn<QString>("theme"); QTest::addColumn<double>("scale");
    for (const QString theme : {QString("dark"), QString("light")})
        for (double scale : {0.75, 2.0}) QTest::newRow(qPrintable(theme + QString::number(scale))) << theme << scale;
}

void TestSessionsWindow::keptWorktreeFooterFitsAndNamesItsSource()
{
    QFETCH(QString, theme); QFETCH(double, scale);
    QSettings settings; const auto oldTheme = settings.value("workspace/theme"), oldScale = settings.value("workspace/contentScale");
    const auto restore = qScopeGuard([&] {
        if (oldTheme.isValid()) settings.setValue("workspace/theme", oldTheme); else settings.remove("workspace/theme");
        if (oldScale.isValid()) settings.setValue("workspace/contentScale", oldScale); else settings.remove("workspace/contentScale");
    });
    settings.setValue("workspace/theme", theme); settings.setValue("workspace/contentScale", scale);
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == "codex/website/navigation") {
        session.runId = "footer-origin-run"; session.gitWorktree = true;
        session.gitRoot = "/workspace/kept-checkout"; session.gitCommonDir = "/workspace/project/.git";
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1180, 820);
    window.setFleet(state); window.show(); window.showSession({}, "codex/website/navigation");
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    window.findChild<QAction *>("archiveSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1);
    auto *offer = window.findChild<QPushButton *>("archivedWorktreeCleanup"); QTRY_VERIFY(offer->isVisible());
    QCOMPARE(offer->text(), QString("Folder kept: kept-checkout"));
    QVERIFY(offer->toolTip().contains("codex/website/navigation")); QVERIFY(offer->toolTip().contains("arch"));
    QVERIFY(offer->toolTip().contains("/workspace/kept-checkout"));
    QVERIFY(offer->accessibleName().contains("codex/website/navigation"));
    auto *footer = window.findChild<QFrame *>("workspaceStatus"); QVERIFY(footer); QCOMPARE(footer->height(), 28);
    QTRY_VERIFY(footer->rect().contains(QRect(offer->mapTo(footer, QPoint()), offer->size())));
    QVERIFY2(offer->minimumSizeHint().height() <= 22, qPrintable(QString::number(offer->minimumSizeHint().height())));
    QVERIFY(offer->fontMetrics().height() <= offer->height());
    QVERIFY(offer->fontMetrics().horizontalAdvance(offer->text()) <= offer->width());
}

void TestSessionsWindow::keptWorktreeReviewFollowsItsStoredTargetAndRechecksOccupants()
{
    auto state = fleet(); auto box = state.local();
    const QString source = "codex/website/navigation", root = "/workspace/retained-checkout", common = "/workspace/project/.git";
    for (auto &session : box.sessions) if (session.name == source) {
        session.runId = "ended-run"; session.gitWorktree = true; session.gitRoot = root; session.gitCommonDir = common;
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(state); window.show(); window.showSession({}, source);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    window.findChild<QAction *>("archiveSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1);
    auto *offer = window.findChild<QPushButton *>("archivedWorktreeCleanup"); QTRY_VERIFY(offer->isVisible());
    // A global reminder names its source when another session is selected.
    window.showSession({}, "codex/hgs/dashboard"); QVERIFY(offer->isVisible());
    client->worktreeCleanupFinished({}, "archive " + source, {{"status", "kept"}, {"path", root}, {"common_dir", common}, {"reason", "Local edits remain"}});
    QVERIFY(offer->isVisible()); QVERIFY(offer->toolTip().contains(source)); QVERIFY(offer->toolTip().contains(root));
    QVERIFY(offer->toolTip().contains("Local edits remain")); QVERIFY(!offer->accessibleName().contains("dashboard"));
    // A same-name newly started run may now own that checkout; never offer it as idle.
    auto occupied = box;
    for (auto &session : occupied.sessions) if (session.name == source) { session.runId = "new-live-run"; session.state = "running"; }
    auto updated = state; updated.setLocal(occupied, QDateTime::currentMSecsSinceEpoch()); window.setFleet(updated);
    QVERIFY(!offer->isVisible());
    client->worktreeCleanupFinished({}, "archive " + source, {{"status", "unconfirmed"}, {"path", root}, {"common_dir", common}, {"reason", "Outcome unknown"}});
    QVERIFY(!offer->isVisible());
    // Once no live session uses the folder, the unresolved review remains reachable.
    occupied.sessions.removeIf([&](const SessionInfo &session) { return session.name == source; });
    updated.setLocal(occupied, QDateTime::currentMSecsSinceEpoch()); window.setFleet(updated);
    QVERIFY(offer->isVisible()); QCOMPARE(offer->text(), QString("Cleanup unconfirmed: retained-checkout"));
    QVERIFY(offer->toolTip().contains("Outcome unknown"));
    auto shared = occupied;
    for (auto &session : shared.sessions) if (session.name == "codex/hgs/dashboard") session.gitRoot = root;
    updated.setLocal(shared, QDateTime::currentMSecsSinceEpoch()); window.setFleet(updated); QVERIFY(!offer->isVisible());
    updated.setLocal(occupied, QDateTime::currentMSecsSinceEpoch()); window.setFleet(updated); QVERIFY(offer->isVisible());
    QCOMPARE(writes.size(), 1); // Navigation/list updates never launch cleanup.
    bool reviewed = false;
    QTimer::singleShot(0, &window, [&] {
        auto *dialog = window.findChild<QDialog *>("worktreeBatchCleanupDialog"); QVERIFY(dialog);
        const auto close = qScopeGuard([&] { dialog->reject(); });
        auto *list = dialog->findChild<QTreeWidget *>("batchCleanupList"); QVERIFY(list);
        const auto target = list->topLevelItem(0)->data(0, Qt::UserRole).toJsonObject();
        QCOMPARE(target["row"].toObject()["path"].toString(), root); QCOMPARE(target["common_dir"].toString(), common);
        QCOMPARE(target["cleanup_reason"].toString(), QString("Outcome unknown"));
        reviewed = true;
    });
    offer->click(); QVERIFY(reviewed); QVERIFY(!offer->isVisible()); QCOMPARE(writes.size(), 1);
}

void TestSessionsWindow::keptWorktreeReviewUsesOnlyCurrentInspectionOccupancy()
{
    const QString source = "codex/website/navigation", occupant = "kimi/docs/research";
    const QString root = "/workspace/inspection-checkout", common = "/workspace/project/.git";
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) {
        if (session.name == source) {
            session.runId = "ended-inspection-run"; session.conversationId = "source-conversation";
            session.gitWorktree = true; session.gitRoot = root; session.gitCommonDir = common;
        } else if (session.name == occupant) {
            session.runId = "current-occupant-run"; session.conversationId = "occupant-conversation";
            session.state = "running"; session.gitRoot.clear(); session.gitWorktree = false;
        }
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    const QJsonObject sourceDetails{{"tracked", true}, {"run_id", "ended-inspection-run"},
        {"conversation_id", "source-conversation"}, {"git_root", root}, {"git_worktree", true},
        {"events", QJsonArray{}}, {"cursor", 0}};
    const QJsonObject clear{{"tracked", true}, {"run_id", "current-occupant-run"},
        {"conversation_id", "occupant-conversation"}, {"activity", "idle"}, {"phase", "idle"},
        {"events", QJsonArray{}}, {"cursor", 0}};
    const auto fixture = m_dir->filePath("footer-inspection-occupancy-hgs");
    const auto save = [&](const QJsonObject &details) {
        return FakeCli::replace(fixture + ".details", QJsonDocument(QJsonObject{
            {source, sourceDetails}, {occupant, details}}).toJson(QJsonDocument::Compact));
    };
    QVERIFY(save(clear));
    QFile original(script()); QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray answer = R"PY(elif c=='inspect':
 with open(__file__+'.details',encoding='utf-8') as f:details=json.load(f)
 print(json.dumps(details.get(a[1],{})))
)PY";
    QVERIFY(FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)));
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window);
    window.setFleet(state); window.show(); window.showSession({}, source);
    auto *client = window.findChild<HgsClient *>(); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QSignalSpy writes(client, &HgsClient::writeDone);
    window.findChild<QAction *>("archiveSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1);
    auto *offer = window.findChild<QPushButton *>("archivedWorktreeCleanup"); QTRY_VERIFY(offer->isVisible());
    window.showSession({}, occupant); settleSelectedInspection(composer, client, {}, occupant, clear);
    QVERIFY(offer->isVisible());
    // Raw fleet has no Git root for this run. Fresh inspection alone proves use.
    auto occupied = clear; occupied["git_root"] = root; occupied["git_worktree"] = true;
    QVERIFY(save(occupied)); settleSelectedInspection(composer, client, {}, occupant, occupied);
    QVERIFY(!offer->isVisible());
    auto staleRun = occupied; staleRun["run_id"] = "unrelated-old-run";
    QVERIFY(save(staleRun)); settleSelectedInspection(composer, client, {}, occupant, staleRun);
    QVERIFY(offer->isVisible());
    auto staleConversation = occupied; staleConversation["conversation_id"] = "unrelated-conversation";
    QVERIFY(save(staleConversation)); settleSelectedInspection(composer, client, {}, occupant, staleConversation);
    QVERIFY(offer->isVisible());
    auto untracked = occupied; untracked["tracked"] = false;
    QVERIFY(save(untracked)); settleSelectedInspection(composer, client, {}, occupant, untracked);
    QVERIFY(offer->isVisible());
    QVERIFY(save(occupied)); settleSelectedInspection(composer, client, {}, occupant, occupied);
    QVERIFY(!offer->isVisible());
    QVERIFY(save(clear)); settleSelectedInspection(composer, client, {}, occupant, clear);
    QVERIFY(offer->isVisible());
    client->inspectionReady("mac", occupant, occupied); QVERIFY(offer->isVisible());
    client->inspectionReady({}, source, occupied); QVERIFY(offer->isVisible());
    QCOMPARE(writes.size(), 1); // Inspection and navigation do not launch cleanup.
}

void TestSessionsWindow::keptWorktreeReviewFormatsTheSourceMachinePath()
{
    struct Context { QString host, path, platform, shown; };
    const QList<Context> contexts{{{}, "C:/work/kept-checkout", "windows", "C:\\work\\kept-checkout"},
        {{}, "/workspace/kept-checkout", "linux", "/workspace/kept-checkout"},
        {"mac", "/remote/kept-checkout", "windows", "/remote/kept-checkout"}};
    for (const auto &context : contexts) {
        auto state = fleet(); auto local = state.local(); local.metrics["platform"] = context.platform;
        auto box = context.host.isEmpty() ? local : *state.peer(context.host);
        const auto source = context.host.isEmpty() ? QString("codex/website/navigation") : QString("claude/infra/review");
        for (auto &session : box.sessions) if (session.name == source) {
            session.state = "paused"; session.runId = "source-path-run"; session.gitWorktree = true;
            session.gitRoot = context.path; session.gitCommonDir = context.path + "/.git";
        }
        if (context.host.isEmpty()) state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
        else { state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); state.setPeer(box, QDateTime::currentMSecsSinceEpoch()); }
        SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(state); window.show(); window.showSession(context.host, source);
        auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
        window.findChild<QAction *>("archiveSessionAction")->trigger(); QTRY_COMPARE(writes.size(), 1);
        auto *offer = window.findChild<QPushButton *>("archivedWorktreeCleanup"); QTRY_VERIFY(offer->isVisible());
        QCOMPARE(offer->text(), QString("Folder kept: kept-checkout"));
        QVERIFY(offer->toolTip().contains("Folder: " + context.shown));
        QVERIFY(offer->accessibleDescription().contains("Folder: " + context.shown));
    }
}

void TestSessionsWindow::terminatingWorktreeSessionOffersRemainingCleanupAndConsumesExactNativeOutcome()
{
    auto state=fleet();auto box=state.local();
    for(auto &session:box.sessions)if(session.name=="codex/hgs/dashboard"){session.runId="terminated-origin-run";session.gitWorktree=true;session.gitRoot="/workspace/owned";session.gitCommonDir="/workspace/project/.git";}
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());SessionsWindow window(program());window.setFleet(state);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *offer=window.findChild<QPushButton *>("archivedWorktreeCleanup");QSignalSpy writes(client,&HgsClient::writeDone);
    QTimer::singleShot(0,&window,[&]{auto *confirm=window.findChild<QMessageBox *>("terminateSessionConfirm");QVERIFY(confirm);confirm->done(QMessageBox::Yes);});
    window.findChild<QAction *>("forgetSessionAction")->trigger();QTRY_COMPARE(writes.size(),1);QTRY_VERIFY(offer->isVisible());QVERIFY(offer->toolTip().contains("/workspace/owned"));
    client->worktreeCleanupFinished({},"terminate codex/hgs/dashboard",{{"status","removed"},{"path","/wrong"},{"common_dir","/workspace/project/.git"}});QVERIFY(offer->isVisible());
    client->worktreeCleanupFinished({},"terminate codex/hgs/dashboard",{{"status","kept"},{"path","/workspace/owned"},{"common_dir","/workspace/project/.git"},{"reason","Uncommitted changes remain"}});
    QVERIFY(offer->isVisible());QVERIFY(offer->toolTip().contains("Uncommitted changes"));
    client->worktreeCleanupFinished({},"terminate codex/hgs/dashboard",{{"status","removed"},{"path","/workspace/owned"},{"common_dir","/workspace/project/.git"}});QVERIFY(!offer->isVisible());
}

void TestSessionsWindow::batchActionsAndReturnToSessions()
{
    SessionsWindow window(program()); auto state = fleet(); window.setFleet(state); window.show();
    QCOMPARE(window.windowTitle(), QString("Zerus"));
    auto *button = window.findChild<QPushButton *>("batchActions"); QVERIFY(button);
    auto *menu = button->menu(); QVERIFY(menu);
    auto refreshMenu = [&]() { QMetaObject::invokeMethod(menu, "aboutToShow"); };
    refreshMenu();
    QCOMPARE(menu->actions().size(), 6); // Machines, separator, expand-all, mark-all-read and archive cleanup.
    auto *expandAll = window.findChild<QAction *>("expandAllCollapsedSessionRows"); QVERIFY(expandAll); QVERIFY(menu->actions().contains(expandAll));
    auto *clear=window.findChild<QAction *>("clearArchive");QVERIFY(menu->actions().contains(clear));
    QVERIFY(clear->isVisible());QVERIFY(!clear->isEnabled()); // No archived entries yet.
    auto *arch = menu->actions()[0]->menu(); auto *mac = menu->actions()[1]->menu();
    QVERIFY(!arch->actions().first()->isEnabled()); // Busy and untracked sessions block pause-all.
    QVERIFY(arch->actions().last()->isEnabled());
    QVERIFY(!mac->actions().first()->isEnabled()); // Awaiting approval is not idle.
    auto ready = *state.peer("mac"); ready.sessions[0].activity = "idle";
    state.setPeer(ready, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); refreshMenu();
    mac = menu->actions()[1]->menu(); QVERIFY(mac->actions().first()->isEnabled());
    auto *client = window.findChild<HgsClient *>(); QSignalSpy done(client, &HgsClient::writeDone);
    mac->actions().first()->trigger(); refreshMenu();
    QVERIFY(!menu->actions()[0]->menu()->actions().last()->isEnabled()); // Another write is pending.
    QTRY_COMPARE(done.size(), 1); QVERIFY(done[0][1].toBool());
    QCOMPARE(done[0][2].toString().trimmed(), QString("@mac\npause\n--all"));
    refreshMenu(); menu->actions()[0]->menu()->actions().last()->trigger();
    QTRY_COMPARE(done.size(), 2); QVERIFY(done[1][1].toBool());
    QCOMPARE(done[1][2].toString().trimmed(), QString("resume\n--all\n-d"));
    state.setPeer(ready, QDateTime::currentMSecsSinceEpoch() - FleetState::kPeerStaleMs - 1);
    window.setFleet(state); refreshMenu(); QVERIFY(!menu->actions()[1]->isEnabled());
    window.setConnectionError("offline"); refreshMenu(); QVERIFY(!menu->actions()[0]->isEnabled());
    for (int i = 0; i < 5; ++i) refreshMenu();
    QCOMPARE(menu->findChildren<QMenu *>().size(), 2);
    window.showProjects("mac");
    auto *projects = window.findChild<ProjectsDialog *>(); QVERIFY(projects->isVisible());
    window.showSessionList(); QVERIFY(!projects->isVisible());
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list->isVisible());
    window.findChild<QLineEdit *>("search")->setText("nothing matches");
    window.showProjects(); window.showSession({}, "codex/website/navigation");
    QVERIFY(!projects->isVisible());
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), QString("\ncodex/website/navigation"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_lifecycle1.moc"
