// U62 (#712): The session list: rows, states, filters, selection and menus, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void sessionAttentionMenuPersistsWithoutSelection();
    void sessionAttentionMenuSurvivesRoutinePoll();
    void sessionContextMenuGuardsIdentityAndChanges();
    void sessionContextMenuSavedAndArchive();
    void sessionContextMenuClearsSession();
    void sessionRowsHaveNoHoverPopup();
    void searchResultKeepsItsSessionAndSnapshot();
    void blankListContextMenu();
    void contextMenuAcrossSessionPanelBackground();
    void sessionListCollapsesIntoWorkingStrip();
    void collapsedStripExpandsOverContentOnlyWhenEnabled();
    void collapsedStripSearchOpensPanel();
    void collapsedStripDoesNotOpenSubagents();
    void collapsingKeepsRowsInPlace();
    void rowPreparationMatchesAppliedRows();
    void rowPreparationFollowsFiltersDraftsAndPeers();
    void rowPreparationNeverRunsOnTheGuiThread();
    void rowPreparationDropsStaleResults();
    void rowPreparationsNeverQueueBehindEachOther();
    void rowPreparationKeepsItemsSelectionAndScroll();
    void rowPreparationRevealsSessionsAfterFilterChanges();
};

void TestSessionsWindow::sessionAttentionMenuPersistsWithoutSelection()
{
    auto state = fleet(); auto box = state.local();
    box.sessions[0].conversationId = "review-conversation"; box.sessions[0].runId = "run";
    box.sessions[0].phase = "approval"; box.sessions[0].activity = "waiting";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList");
    auto *menu = window.findChild<QMenu *>("sessionContextMenu");
    QSignalSpy marks(&window, &SessionsWindow::attentionMarksChanged);
    list->setCurrentRow(1);
    const auto open = [&] {
        menu->hide(); const auto point = list->visualItemRect(list->item(0)).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &event);
    };
    open(); QVERIFY(!menu->findChild<QAction *>("contextReviewLater"));
    menu->findChild<QAction *>("contextMarkRead")->trigger();
    QCOMPARE(list->currentRow(), 1); QCOMPARE(marks.size(), 1);
    QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
    open(); QVERIFY(!menu->findChild<QAction *>("contextMarkRead"));
    menu->findChild<QAction *>("contextReviewLater")->trigger();
    QCOMPARE(list->currentRow(), 1); QCOMPARE(marks.size(), 2);
    QVERIFY(list->item(0)->data(SessionRoles::ReviewLater).toBool());
    window.setFleet(state); // Native polls do not contain UI reminders.
    QVERIFY(list->item(0)->data(SessionRoles::ReviewLater).toBool());
    open(); QVERIFY(menu->findChild<QAction *>("contextMarkRead")->isEnabled());
    QVERIFY(!menu->findChild<QAction *>("contextReviewLater"));
    menu->findChild<QAction *>("contextMarkRead")->trigger();
    QCOMPARE(list->currentRow(), 1); QCOMPARE(marks.size(), 3);
    QVERIFY(!list->item(0)->data(SessionRoles::ReviewLater).toBool());
    QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
    window.setFleet(state); QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
    open(); menu->findChild<QAction *>("contextReviewLater")->trigger(); menu->hide();
    SessionsWindow reopened(program()); reopened.setFleet(state); reopened.show();
    QVERIFY(reopened.findChild<QListWidget *>("sessionList")->item(0)->data(SessionRoles::ReviewLater).toBool());
}

void TestSessionsWindow::sessionAttentionMenuSurvivesRoutinePoll()
{
    auto state = fleet(); auto box = state.local(); auto &session = box.sessions[0];
    session.conversationId = "poll-conversation"; session.runId = "run";
    session.phase = "input"; session.activity = "waiting";
    session.attentionId = "question-1"; session.lastEventAt = 100;
    state.setLocal(box, 1000);
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList");
    auto *menu = window.findChild<QMenu *>("sessionContextMenu");
    QSignalSpy marks(&window, &SessionsWindow::attentionMarksChanged);
    list->setCurrentRow(1);
    const auto open = [&] {
        menu->hide(); const auto point = list->visualItemRect(list->item(0)).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &event);
    };
    open(); auto *read = menu->findChild<QAction *>("contextMarkRead"); QVERIFY(read);
    session.lastEventAt = 101;
    session.subagents = {{"worker", QJsonObject{{"state", "working"}, {"summary", "Another tool"}}}};
    state.setLocal(box, 2000); window.setFleet(state); read->trigger();
    QCOMPARE(marks.size(), 1); QCOMPARE(list->currentRow(), 1);
    QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
    session.lastEventAt = 102; state.setLocal(box, 3000); window.setFleet(state);
    QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
    session.attentionId = "question-2"; state.setLocal(box, 4000); window.setFleet(state);
    open(); read = menu->findChild<QAction *>("contextMarkRead"); QVERIFY(read);
    session.attentionId = "question-3"; state.setLocal(box, 5000); window.setFleet(state);
    read->trigger(); QCOMPARE(marks.size(), 1);
    QVERIFY(list->item(0)->data(SessionRoles::Attention).toBool());
    QVERIFY(window.findChild<QLabel *>("notice")->text().contains("A new reply or question arrived"));
    menu->hide();
}

void TestSessionsWindow::sessionContextMenuGuardsIdentityAndChanges()
{
    auto state = fleet(); auto box = state.local(); box.sessions[0].activity = "idle"; box.sessions[0].phase = "idle"; box.sessions[0].created = 100;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionContextMenu");
    auto show = [&]() { menu->hide(); list->setCurrentRow(0); QTest::keyClick(list, Qt::Key_Menu); };
    auto action = [&](const char *name) { return menu->findChild<QAction *>(name); };
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone), copies(&window, &SessionsWindow::copySessionCommandRequested);
    show(); QVERIFY(action("contextChangeSession")->isEnabled()); list->setCurrentRow(1);
    action("contextChangeSession")->trigger(); QTRY_COMPARE(writes.size(), 1);
    QCOMPARE(writes[0][2].toString(), QString("pause\ncodex/hgs/dashboard")); QCOMPARE(list->currentRow(), 0);
    show(); auto *pause = action("contextChangeSession"); box.sessions[0].activity = "busy";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); pause->trigger(); QTest::qWait(30); QCOMPARE(writes.size(), 1);
    box.sessions[0].activity = "idle"; state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    show(); pause = action("contextChangeSession"); box.sessions[0].state = "paused";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); pause->trigger(); QTest::qWait(30); QCOMPARE(writes.size(), 1);
    // A removed row cannot redirect a saved QAction to the newly selected row.
    show(); auto *copy = action("contextCopyCommand"); const auto removed = box.sessions.takeFirst();
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); copy->trigger(); QCOMPARE(copies.size(), 0);
    QVERIFY(window.findChild<QLabel *>("notice")->text().contains("no longer available"));
    box.sessions.prepend(removed); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    show(); copy = action("contextCopyCommand"); box.sessions[0].created = 200;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); copy->trigger(); QCOMPARE(copies.size(), 0);
    // Polling inside a confirmation dialog must also preserve the target.
    show(); QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QMessageBox *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        box.sessions.removeFirst(); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
        dialog->button(QMessageBox::Yes)->click();
    });
    action("contextTerminateSession")->trigger(); QTest::qWait(30); QCOMPARE(writes.size(), 1);
}

void TestSessionsWindow::sessionContextMenuSavedAndArchive()
{
    auto state = fleet(); auto box = state.local(); auto older = box.sessions[2];
    older.state = "archived"; older.archiveId = "older"; older.archivedAt = 100; older.cwd = "/saved/folder";
    auto recent = older; recent.archiveId = "recent"; recent.archivedAt = 200;
    box.sessions.append(older); box.sessions.append(recent); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, "codex/website/navigation");
    auto *list = window.findChild<QListWidget *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionContextMenu");
    auto show = [&]() { menu->hide(); QTest::keyClick(list, Qt::Key_Menu); };
    auto action = [&](const char *name) { return menu->findChild<QAction *>(name); };
    QSignalSpy copies(&window, &SessionsWindow::copySessionCommandRequested);
    auto *client = window.findChild<HgsClient *>(); QSignalSpy writes(client, &HgsClient::writeDone);
    show(); QCOMPARE(action("contextChangeSession")->text(), QString("Resume session")); QVERIFY(action("contextArchiveSession"));
    QVERIFY(action("contextStartFresh")); QVERIFY(!action("contextClearSession"));
    QCOMPARE(action("contextTerminateSession")->text(), QString("Forget saved session…"));
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget()); QVERIFY(dialog);
        QCOMPARE(dialog->findChild<QLineEdit *>("renameSessionName")->text(), QString("navigation")); dialog->reject();
    });
    action("contextRenameSession")->trigger(); QCOMPARE(writes.size(), 0); menu->hide();
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "archived") button->click();
    list->setCurrentRow(0);
    show(); QVERIFY(!action("contextOpenTerminal")); QVERIFY(!action("contextArchiveSession")); QVERIFY(!action("contextClearSession"));
    QCOMPARE(action("contextCopyCommand")->text(), QString("Copy restore command"));
    QCOMPARE(action("contextChangeSession")->text(), QString("Restore session")); QVERIFY(!action("contextChangeSession")->isEnabled()); // Saved name collision.
    list->setCurrentRow(1); action("contextCopyCommand")->trigger(); QCOMPARE(copies.size(), 1);
    QCOMPARE(copies[0][1].toString(), QString("codex/website/navigation")); QCOMPARE(copies[0][2].toString(), QString("recent"));
    QTimer::singleShot(0, &window, [&]() {
        auto *dialog = qobject_cast<QMessageBox *>(QApplication::activeModalWidget()); QVERIFY(dialog); dialog->button(QMessageBox::Yes)->click();
    });
    list->setCurrentRow(1); action("contextTerminateSession")->trigger(); QTRY_COMPARE(writes.size(), 1);
    QCOMPARE(writes[0][2].toString(), QString("kill\ncodex/website/navigation\n--archive\nrecent"));
}

void TestSessionsWindow::sessionContextMenuClearsSession()
{
    QTemporaryDir temp;const auto program=temp.filePath("hgs"),capture=temp.filePath("clear.json");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
if 'clear-context' in args:
 p=json.load(sys.stdin);p['args']=args;staged=root/'clear.tmp';staged.write_text(json.dumps(p));staged.replace(root/'clear.json');print(json.dumps(dict(request_id=p['request_id'],name=args[2],run_id=p['expected_run_id'],conversation_id=p['expected_conversation_id'],status='confirmed')))
elif 'inspect' in args:
 name=next((a for a in args if '/' in a),'');f=root/(name.replace('/','_')+'.json');print(f.read_text() if f.exists() else '{}')
else: print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    // Each session reports its own details; background inspections may read them at any time.
    const auto writeDetails=[&](const QString &name,const QJsonObject &details){
        QVERIFY(FakeCli::replace(temp.filePath(QString(name).replace('/','_')+".json"),QJsonDocument(details).toJson()));};
    const QJsonObject live{{"tracked",true},{"run_id","run-one"},{"phase","idle"},{"activity","idle"},{"runtime_state","live"},
        {"process_state","running"},{"conversation_id","conversation-one"},{"clear_context_supported",true}};
    auto busy=live;busy["run_id"]="run-two";busy["conversation_id"]="conversation-two";busy["clear_context_supported"]=false;
    busy["clear_context_reason"]="Kimi is still working";
    writeDetails("claude/infra/review",live);writeDetails("kimi/docs/research",busy);
    auto state=fleet();auto box=state.local();box.sessions[1].runId="run-two";box.sessions[1].conversationId="conversation-two";
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    auto mac=*state.peer("mac");
    mac.sessions[0].runId="run-one";mac.sessions[0].conversationId="conversation-one";state.setPeer(mac,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program));window.resize(1080,760);window.setFleet(state);window.show();
    auto *list=window.findChild<QListWidget *>("sessionList");auto *menu=window.findChild<QMenu *>("sessionContextMenu");QVERIFY(menu);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");
    window.showSession("mac","claude/infra/review");const int target=list->currentRow();setComposerText(composer,"Keep this draft");
    window.showSession({},"kimi/docs/research");const int other=list->currentRow();QVERIFY(target!=other);
    const auto showMenu=[&](int row){menu->hide();const QPoint point=list->visualItemRect(list->item(row)).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse,point,list->viewport()->mapToGlobal(point));QApplication::sendEvent(list->viewport(),&event);
        return menu->findChild<QAction *>("contextClearSession");};
    auto *client=window.findChild<HgsClient *>();QSignalSpy finished(client,&HgsClient::sessionActionFinished);
    // Another session's Clear waits for its own inspection, then selects it and confirms.
    auto *clear=showMenu(target);QVERIFY(clear);QCOMPARE(clear->text(),QString("Clear session…"));QCOMPARE(list->currentRow(),other);
    QTRY_VERIFY_WITH_TIMEOUT(clear->isEnabled(),10000);
    bool asked=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);asked=true;
        QCOMPARE(dialog->defaultButton(),dialog->button(QMessageBox::Cancel));
        QVERIFY(dialog->text().contains("claude/infra/review"));QVERIFY(dialog->text().contains("mac"));
        for(auto *button:dialog->buttons())if(dialog->buttonRole(button)==QMessageBox::DestructiveRole){button->click();return;}QFAIL("Missing clear confirmation");});
    clear->trigger();QVERIFY(asked);QCOMPARE(list->currentRow(),target);QTRY_VERIFY(QFileInfo::exists(capture));
    QFile sent(capture);QVERIFY(sent.open(QIODevice::ReadOnly));const auto payload=QJsonDocument::fromJson(sent.readAll()).object();sent.close();
    QCOMPARE(payload["args"].toArray(),QJsonArray({"@mac","clear-context","claude/infra/review","--json"}));
    QCOMPARE(payload["expected_run_id"].toString(),QString("run-one"));QCOMPARE(payload["expected_conversation_id"].toString(),QString("conversation-one"));
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    QTRY_COMPARE(finished.size(),1);QVERIFY(QFile::remove(capture));
    // The selected session follows its header icon at once; Cancel sends nothing.
    auto *headerClear=window.findChild<QPushButton *>("clearSessionAction");QTRY_VERIFY(headerClear->isEnabled());
    clear=showMenu(target);QVERIFY(clear->isEnabled());asked=false;
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("clearSessionConfirm");QVERIFY(dialog);asked=true;dialog->button(QMessageBox::Cancel)->click();});
    clear->trigger();QVERIFY(asked);QTest::qWait(100);QVERIFY(!QFileInfo::exists(capture));
    // An unavailable reset explains itself and stays disabled.
    clear=showMenu(other);QVERIFY(clear);QTRY_VERIFY_WITH_TIMEOUT(clear->toolTip().contains("Kimi is still working"),10000);
    QVERIFY(!clear->isEnabled());QCOMPARE(list->currentRow(),target);menu->hide();
}

void TestSessionsWindow::sessionRowsHaveNoHoverPopup()
{
    auto state = fleet(); auto box = state.local();
    auto &session = box.sessions[0];
    session.cwd = "/workspace/" + QString(6000, 'p');
    session.gitBranch = "feature/" + QString(6000, 'b');
    session.activitySummary = "<img src='file:///private'> " + QString(12000, 'x');
    session.activityDetail = QString(12000, 'y');
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.showSession({}, session.name);
    auto *list = window.findChild<QListWidget *>("sessionList");
    QVERIFY(list->currentItem());
    QVERIFY(list->currentItem()->toolTip().isEmpty());
    const auto accessible = list->currentItem()->data(Qt::AccessibleTextRole).toString();
    QVERIFY(accessible.size() < 1200); QVERIFY(accessible.count('\n') <= 11);
    QVERIFY(accessible.contains(session.name));
    window.setFleet(state);
    QVERIFY(list->currentItem()->toolTip().isEmpty());
}

void TestSessionsWindow::searchResultKeepsItsSessionAndSnapshot()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    auto *search = window.findChild<QLineEdit *>("search"); search->setText("review");
    auto *results = window.findChild<QListWidget *>("searchResults"); QCOMPARE(results->count(), 1);
    results->setCurrentRow(0);
    auto *title = window.findChild<QLabel *>("detailTitle"); QVERIFY(title->text().contains("review"));
    window.setFleet(fleet()); QVERIFY(title->text().contains("review")); // Poll must not select hidden regular-list row.
    search->clear(); QVERIFY(window.findChild<QListWidget *>("sessionList")->isVisible());
    QCOMPARE(window.findChild<QListWidget *>("sessionList")->currentItem()->data(Qt::UserRole).toString(), QString("mac\nclaude/infra/review"));
}

void TestSessionsWindow::blankListContextMenu()
{
    SessionsWindow window(program()); FleetState empty; BoxState box; box.ok = true; box.host = "arch"; empty.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    window.setFleet(empty); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionProjectsMenu");
    QVERIFY(menu); list->customContextMenuRequested(QPoint(20,80));
    QVERIFY(menu->isVisible()); QVERIFY(menu->actions().first()->text().contains("New session"));
    QVERIFY(menu->actions().at(1)->text().contains("New project")); menu->hide();
}

void TestSessionsWindow::contextMenuAcrossSessionPanelBackground()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.resize(1000, 750); window.show();
    auto *filter = window.findChild<MachineFilter *>("machineFilterChips"); filter->setSelection({"mac"});
    QTest::qWait(20);
    auto *list = window.findChild<SessionList *>("sessionList");
    auto *panel = window.findChild<QWidget *>("sessionListPanel");
    auto *groups = window.findChild<QMenu *>("sessionProjectsMenu"); QVERIFY(groups);
    auto *session = window.findChild<QMenu *>("sessionContextMenu");
    const auto sendContext = [](QWidget *target, const QPoint &point) {
        QContextMenuEvent event(QContextMenuEvent::Mouse, point, target->mapToGlobal(point));
        QApplication::sendEvent(target, &event);
    };
    const auto expectGroups = [&](QWidget *target, const QPoint &point) {
        sendContext(target, point);
        const bool correct = groups->isVisible() && !session->isVisible();
        groups->hide(); session->hide(); return correct;
    };
    QCOMPARE(list->count(), 1);
    const auto row = list->visualItemRect(list->item(0));
    const int panelY = list->viewport()->mapTo(panel, row.center()).y();
    QVERIFY2(expectGroups(panel, QPoint(1, panelY)), "Left panel gutter");
    QVERIFY2(expectGroups(panel, QPoint(panel->width() - 2, panelY)), "Right panel gutter");
    QVERIFY2(expectGroups(list->viewport(), QPoint(row.left(), row.center().y())), "Left card gutter");
    QVERIFY2(expectGroups(list->viewport(), QPoint(row.right(), row.center().y())), "Right card gutter");
    QVERIFY2(expectGroups(list->viewport(), QPoint(row.center().x(), row.top())), "Gap above card");
    QVERIFY2(expectGroups(list->viewport(), QPoint(20, row.bottom() + 20)), "Below last session");
    QVERIFY2(expectGroups(filter, QPoint(filter->width() - 2, 10)), "Empty tag strip");
    auto *chip = filter->findChild<QPushButton *>("machineFilterChip"); QVERIFY(chip);
    QVERIFY2(expectGroups(chip, chip->rect().center()), "Machine filter chip");
    QCOMPARE(filter->selection(), QSet<QString>{"mac"});
    sendContext(list->viewport(), row.center()); QVERIFY(session->isVisible()); QVERIFY(!groups->isVisible()); session->hide();

    // Text fields retain their native editing menu instead of the panel menu.
    auto *search = window.findChild<QLineEdit *>("search"); bool nativeMenu = false;
    QTimer::singleShot(0, &window, [&] {
        auto *popup = qobject_cast<QMenu *>(QApplication::activePopupWidget());
        nativeMenu = popup && popup != groups && popup != session && !groups->isVisible();
        if (popup) popup->close();
    });
    sendContext(search, search->rect().center()); QCoreApplication::processEvents();
    QVERIFY(nativeMenu);
}

void TestSessionsWindow::sessionListCollapsesIntoWorkingStrip()
{
    QSettings().setValue("workspace/expandSessionsOnHover", false);
    if (!qEnvironmentVariable("HGS_STRIP_PREVIEW").isEmpty()) QSettings().setValue("workspace/theme", "dark");
    auto window = std::make_unique<SessionsWindow>(program()); window->resize(1280, 860); window->setFleet(fleet()); window->show();
    QVERIFY(QTest::qWaitForWindowExposed(window.get()));
    auto *panel = window->findChild<QWidget *>("sessionListPanel"), *slot = window->findChild<QWidget *>("sessionListSlot");
    auto *toggle = window->findChild<QPushButton *>("sessionPanelToggle"); auto *list = window->findChild<SessionList *>("sessionList");
    QVERIFY(panel && slot && toggle && list);
    QTRY_COMPARE(panel->width(), slot->width()); QVERIFY(slot->width() >= 270);
    const int docked = slot->width();
    auto *detail = window->findChild<QStackedWidget *>("detail"); const int detailWidth = detail->width();
    const auto preview = qEnvironmentVariable("HGS_STRIP_PREVIEW"); if (!preview.isEmpty()) QDir().mkpath(preview);
    const auto shot = [&](QWidget *target, const QString &name) { if (!preview.isEmpty()) QVERIFY(target->grab().save(preview + "/" + name)); };
    shot(window.get(), "docked.png");

    toggle->click();
    if (!preview.isEmpty()) { QTest::qWait(70); shot(window.get(), "collapsing.png"); }
    QTRY_COMPARE(list->property("expansion").toReal(), 0.0);
    QCOMPARE(panel->width(), SessionStrip::width(true)); QCOMPARE(slot->width(), SessionStrip::width(true)); QVERIFY(detail->width() > detailWidth);
    QVERIFY(Settings::value("workspace/sessionsCollapsed").toBool());
    QVERIFY(!window->findChild<QLineEdit *>("search")->isVisible()); QVERIFY(window->findChild<QPushButton *>("sessionStripSearch")->isVisible());
    auto *readAll = window->findChild<QPushButton *>("markAllReadButton"); QVERIFY(readAll);
    QVERIFY(!readAll->isVisible()); QVERIFY(!window->findChild<QPushButton *>("savedDrafts")->isVisible());
    // The strip keeps the active filter and the most urgent one, together as wide
    // as a tile, under a search button of the same width.
    const auto shownFilters = [&] {
        QStringList ids; for (auto *filter : window->findChildren<QPushButton *>("sessionFilter")) if (filter->isVisible()) ids << filter->property("filter").toString();
        return ids;
    };
    const auto filterButton = [&](const QString &id) {
        for (auto *filter : window->findChildren<QPushButton *>("sessionFilter")) if (filter->property("filter") == id) return filter;
        return static_cast<QPushButton *>(nullptr);
    };
    QCOMPARE(shownFilters(), QStringList({"all", "attention"}));
    {
        auto *all = filterButton("all"), *attention = filterButton("attention"), *search = window->findChild<QPushButton *>("sessionStripSearch");
        const auto *first = list->item(0)->data(SessionRoles::Header).toBool() ? list->item(1) : list->item(0);
        const QRect tile = sessionCardRect(list->visualItemRect(first)).translated(list->viewport()->mapTo(panel, QPoint()));
        const auto edges = [panel](QWidget *w) { const QPoint at = w->mapTo(panel, QPoint()); return std::pair{at.x(), at.x() + w->width()}; };
        QTRY_COMPARE(edges(search), std::pair(tile.x(), tile.x() + tile.width()));
        QCOMPARE(edges(all).first, tile.x()); QCOMPARE(edges(attention).second, tile.x() + tile.width());
        QVERIFY(edges(attention).first > edges(all).second);
    }
    auto *summary = window->findChild<QLabel *>("listSummary");
    QVERIFY(summary->toolTip().contains("shown")); QCOMPARE(summary->text(), summary->toolTip().section(' ', 0, 0));
    QCOMPARE(toggle->property("glyph").toString(), QString("expand-sessions"));

    // The strip keeps every row, as narrow as the strip, and still selects sessions.
    int target = -1;
    for (int i = 0; i < list->count(); ++i) {
        const auto *row = list->item(i);
        QVERIFY(list->visualItemRect(row).width() <= list->viewport()->width());
        // Each session is a square tile.
        if (!row->data(SessionRoles::Header).toBool() && !row->isHidden()) {
            const QRect card = sessionCardRect(list->visualItemRect(row)); QCOMPARE(card.width(), card.height());
        }
        if (!row->data(SessionRoles::Header).toBool() && row != list->currentItem() && !row->isHidden()) target = i;
    }
    QVERIFY(target >= 0);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, list->visualItemRect(list->item(target)).center());
    QCOMPARE(list->currentRow(), target);
    QImage strip = list->viewport()->grab().toImage(); QVERIFY(!strip.isNull());
    shot(window.get(), "collapsed.png");
    // A tile's frame takes its state's color; the card's attention edge stays away.
    const bool dark = list->property("hgsDark").toBool(); int framed = 0;
    const auto near = [](QColor a, QColor b) { return qAbs(a.red() - b.red()) + qAbs(a.green() - b.green()) + qAbs(a.blue() - b.blue()) <= 24; };
    for (int i = 0; i < list->count(); ++i) {
        const auto *row = list->item(i);
        if (row->isHidden() || row->data(SessionRoles::Header).toBool() || !row->data(SessionRoles::ChildId).toString().isEmpty()) continue;
        const auto status = SessionDelegate::statusOf(list->model()->index(i, 0), row == list->currentItem());
        const QColor edge = SessionStatusBadge::edge(status.kind, dark);
        if (!edge.isValid()) continue;
        const QRect card = sessionCardRect(list->visualItemRect(row)); ++framed;
        QVERIFY2(near(strip.pixelColor(card.x(), card.center().y()), edge), qPrintable(row->data(SessionRoles::Title).toString()));
        QVERIFY(!near(strip.pixelColor(card.x() + 4, card.center().y()), IdentityBadges::attentionColor(dark, status.kind == SessionStatusBadge::Error)));
    }
    QVERIFY(framed >= 3);
    // Taking the urgent filter offers the next one: active sessions.
    filterButton("attention")->click();
    QTRY_COMPARE(shownFilters(), QStringList({"attention", "active"}));
    filterButton("all")->click(); QTRY_COMPARE(shownFilters(), QStringList({"all", "attention"}));

    // The collapsed state survives a restart and keeps the docked width for later.
    window.reset(); QSettings().setValue("workspace/sessionsWidth", 360);
    window = std::make_unique<SessionsWindow>(program()); window->resize(1280, 860); window->setFleet(fleet()); window->show();
    QVERIFY(QTest::qWaitForWindowExposed(window.get()));
    panel = window->findChild<QWidget *>("sessionListPanel"); slot = window->findChild<QWidget *>("sessionListSlot");
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    window->findChild<QPushButton *>("sessionPanelToggle")->click();
    QTRY_COMPARE(slot->width(), 360); QTRY_COMPARE(panel->width(), 360); QVERIFY(docked != 360);
    QVERIFY(!Settings::value("workspace/sessionsCollapsed").toBool());
    QCOMPARE(window->findChild<SessionList *>("sessionList")->property("expansion").toReal(), 1.0);
}

void TestSessionsWindow::collapsedStripExpandsOverContentOnlyWhenEnabled()
{
    QSettings().setValue("workspace/sessionsCollapsed", true);
    {
        SessionsWindow window(program());
        auto *option = window.findChild<QCheckBox *>("workspaceExpandSessionsOnHover"); QVERIFY(option); QVERIFY(option->isChecked());
    }
    QSettings().setValue("workspace/expandSessionsOnHover", false);
    {
        SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
        QVERIFY(QTest::qWaitForWindowExposed(&window));
        auto *option = window.findChild<QCheckBox *>("workspaceExpandSessionsOnHover"); QVERIFY(!option->isChecked());
        auto *panel = window.findChild<QWidget *>("sessionListPanel");
        QTRY_COMPARE(panel->width(), SessionStrip::width(true));
        QTest::mouseMove(panel, QPoint(30, 300)); QTest::qWait(450);
        QCOMPARE(panel->width(), SessionStrip::width(true));
    }
    QSettings().setValue("workspace/expandSessionsOnHover", true);
    if (!qEnvironmentVariable("HGS_STRIP_PREVIEW").isEmpty()) QSettings().setValue("workspace/theme", "dark");
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    QVERIFY(window.findChild<QCheckBox *>("workspaceExpandSessionsOnHover")->isChecked());
    auto *panel = window.findChild<QWidget *>("sessionListPanel"), *slot = window.findChild<QWidget *>("sessionListSlot");
    auto *detail = window.findChild<QStackedWidget *>("detail");
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    const int detailWidth = detail->width();
    QTest::mouseMove(panel, QPoint(30, 300));
    QTRY_VERIFY(panel->width() >= 270);
    const auto preview = qEnvironmentVariable("HGS_STRIP_PREVIEW");
    if (!preview.isEmpty()) { QTest::qWait(300); QDir().mkpath(preview); QVERIFY(window.grab().save(preview + "/hover.png")); }
    QCOMPARE(slot->width(), SessionStrip::width(true)); QCOMPARE(detail->width(), detailWidth);
    // Nothing dims the conversation: only the panel and its edge shadow lie over it.
    auto *splitter = slot->parentWidget();
    for (auto *layer : panel->parentWidget()->findChildren<QWidget *>(QString(), Qt::FindDirectChildrenOnly))
        if (layer->isVisible() && layer != panel && layer != splitter && layer->geometry().intersects(splitter->geometry()))
            QVERIFY(layer->testAttribute(Qt::WA_TransparentForMouseEvents) && layer->width() <= 18);
    QCOMPARE(window.findChild<QPushButton *>("sessionPanelToggle")->property("glyph").toString(), QString("pin"));
    QTest::mouseMove(detail, QPoint(detail->width() - 40, 300));
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    QVERIFY(Settings::value("workspace/sessionsCollapsed").toBool());
}

void TestSessionsWindow::collapsedStripSearchOpensPanel()
{
    QSettings().setValue("workspace/sessionsCollapsed", true); QSettings().setValue("workspace/expandSessionsOnHover", false);
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window)); window.activateWindow();
    auto *panel = window.findChild<QWidget *>("sessionListPanel"); auto *search = window.findChild<QLineEdit *>("search");
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    window.findChild<QPushButton *>("sessionStripSearch")->click();
    QTRY_VERIFY(panel->width() >= 270); QTRY_VERIFY(search->hasFocus());
    QTest::keyClick(search, Qt::Key_Escape);
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    window.findChild<QPushButton *>("sessionStripSearch")->click(); QTRY_VERIFY(panel->width() >= 270);
    // A click on the conversation closes the panel and still reaches its target.
    struct Presses : QObject {
        int count = 0;
        bool eventFilter(QObject *, QEvent *event) override { count += event->type() == QEvent::MouseButtonPress; return false; }
    } presses;
    auto *detail = window.findChild<QStackedWidget *>("detail"); const QPoint point(detail->width() - 60, detail->height() / 2);
    QWidget *target = detail->childAt(point) ? detail->childAt(point) : detail; target->installEventFilter(&presses);
    QTest::mouseClick(target, Qt::LeftButton, {}, target->mapFrom(detail, point));
    QTRY_COMPARE(panel->width(), SessionStrip::width(true)); QCOMPARE(presses.count, 1); target->removeEventFilter(&presses);
    // Pinning the open panel docks it; the conversation then makes room once.
    window.findChild<QPushButton *>("sessionStripSearch")->click(); QTRY_VERIFY(panel->width() >= 270);
    window.findChild<QPushButton *>("sessionPanelToggle")->click();
    QTRY_COMPARE(window.findChild<QWidget *>("sessionListSlot")->width(), panel->width());
    QVERIFY(!Settings::value("workspace/sessionsCollapsed").toBool());
}

void TestSessionsWindow::collapsedStripDoesNotOpenSubagents()
{
    QSettings().setValue("workspace/sessionsCollapsed", true); QSettings().setValue("workspace/expandSessionsOnHover", false);
    FleetState state; auto box = fleet().local(); auto session = box.sessions[0];
    session.subagents = {{"agent-a", QJsonObject{{"name","Review"},{"state","working"}}}, {"agent-b", QJsonObject{{"name","Tests"},{"state","finished"}}}};
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(state); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    auto *panel = window.findChild<QWidget *>("sessionListPanel"); auto *list = window.findChild<SessionList *>("sessionList");
    QTRY_COMPARE(panel->width(), SessionStrip::width(true));
    int parent = -1; for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::HasChildren).toBool()) parent = i;
    QVERIFY(parent >= 0); const int count = list->count();
    // Where a card has its agents counter, a tile only selects the session; Right opens nothing either.
    const QPoint corner = sessionCardRect(list->visualItemRect(list->item(parent))).bottomRight() - QPoint(12, 10);
    QTest::mouseMove(list->viewport(), corner); QVERIFY(list->viewport()->cursor().shape() != Qt::PointingHandCursor);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, corner);
    QCOMPARE(list->count(), count); QCOMPARE(list->currentRow(), parent);
    QTest::keyClick(list, Qt::Key_Right); QCOMPARE(list->count(), count);
    // The docked list opens active children; finished turns stay hidden.
    window.findChild<QPushButton *>("sessionPanelToggle")->click();
    QTRY_COMPARE(list->property("expansion").toReal(), 1.0);
    QTest::keyClick(list, Qt::Key_Right); QTRY_COMPARE(list->count(), count + 1);
    QCOMPARE(list->item(parent + 1)->data(SessionRoles::ChildId).toString(), QString("agent-a"));
}

void TestSessionsWindow::collapsingKeepsRowsInPlace()
{
    QSettings().setValue("workspace/expandSessionsOnHover", false);
    SessionsWindow window(program()); window.resize(1280, 860); window.setFleet(fleet()); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    auto *panel = window.findChild<QWidget *>("sessionListPanel"); auto *list = window.findChild<SessionList *>("sessionList");
    auto *toggle = window.findChild<QPushButton *>("sessionPanelToggle"); auto *chips = window.findChild<MachineFilter *>("machineFilterChips");
    auto *projects = window.findChild<ProjectFilter *>("projectFilterChips");
    const auto top = [&] { return list->mapTo(panel, QPoint()).y(); };
    for (bool filtered : {false, true}) {
        if (filtered) { chips->setSelection({"mac"}); projects->setSelection({"ungrouped"}); QVERIFY(chips->isVisible()); QVERIFY(projects->isVisible()); }
        // The chip row shows at once and moves the list down when the layout runs,
        // which can take a while under load: sample the docked position once the
        // list sits below the laid-out row and stays there.
        int previous = -1;
        QTRY_VERIFY((!filtered || (chips->height() > 0 && top() > projects->geometry().bottom()))
                    && [&] { const int now = top(); return std::exchange(previous, now) == now; }());
        const int docked = top();
        // Each frame of the transition keeps the list where it was, also before
        // the layout requests of an animation step run.
        QList<int> frames;
        ExpansionSteps steps([&] { frames << top(); }); list->installEventFilter(&steps);
        toggle->click();
        for (int i = 0; i < 30 && list->property("expansion").toReal() > 0; ++i) { QTest::qWait(10); frames << top(); }
        QTRY_COMPARE(list->property("expansion").toReal(), 0.0); frames << top();
        // A poll rebuilds the collapsed list; at no point is it laid out lower.
        window.setFleet(fleet()); frames << top(); QTest::qWait(20); frames << top();
        QVERIFY(projects->isHidden());
        toggle->click(); QTRY_COMPARE(list->property("expansion").toReal(), 1.0); frames << top();
        for (int frame : frames) QCOMPARE(frame, docked);
    }
}

void TestSessionsWindow::rowPreparationMatchesAppliedRows()
{
    AsyncRows async;
    storeRowProjects();
    QSettings().setValue("workspace/expandedSessions", QStringList{"\nclaude/hgs/subs"});
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 760);
    QSignalSpy applied(&window, &SessionsWindow::rowsApplied);
    window.setFleet(rowFleet(QDateTime::currentMSecsSinceEpoch())); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    settleRows(window); QVERIFY(!applied.isEmpty());
    // The rows applied are those a preparation of the window's current inputs gives.
    const auto mismatch = [&]() -> QString {
        if (window.rowsPendingForTests()) return "pending";
        const auto rows = prepared(window.rowInputsForTests());
        if (list->count() != rows.rows.size()) return QString("%1 rows, %2 prepared").arg(list->count()).arg(rows.rows.size());
        for (int i = 0; i < rows.rows.size(); ++i) {
            const auto &row = rows.rows[i]; const auto *item = list->item(i);
            if (item->data(SessionRoles::Key).toString() != row.key) return "key of row " + QString::number(i) + ": " + row.key;
            if (item->flags() != row.flags || item->isHidden() != row.hidden) return "flags or hidden of " + row.key;
            if (item->data(SessionRoles::Digest).toByteArray() != row.digest) return "digest of " + row.key;
            const auto roles = itemRoles(item);
            for (auto it = row.roles.cbegin(); it != row.roles.cend(); ++it)
                if (roles.value(it.key()) != it.value()) return QString("role %1 of %2").arg(it.key()).arg(row.key);
            if (roles.size() != row.roles.size()) return "other roles of " + row.key;
        }
        return {};
    };
    const auto compare = [&] { QTRY_COMPARE_WITH_TIMEOUT(mismatch(), QString(), 20000); };
    compare();
    // Filters, a collapsed project and a changed listing apply the same rows.
    for (const auto *id : {"attention", "active", "paused", "all"}) {
        for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter") == id) button->click();
        settleRows(window); compare();
    }
    auto changed = rowFleet(QDateTime::currentMSecsSinceEpoch());
    auto local = changed.local(); local.sessions[0].activityDetail = "step 2"; local.sessions.removeAt(1);
    changed.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    window.setFleet(changed); settleRows(window); compare();
    QVERIFY(!preparedKeys(prepared(window.rowInputsForTests())).contains("\nkimi/docs/research"));
}

void TestSessionsWindow::rowPreparationFollowsFiltersDraftsAndPeers()
{
    const auto second = storeRowProjects();
    QSettings().setValue("workspace/expandedSessions", QStringList{"\nclaude/hgs/subs"});
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 760);
    window.setFleet(rowFleet(QDateTime::currentMSecsSinceEpoch())); window.show();
    const auto inputs = window.rowInputsForTests();
    auto rows = prepared(inputs);
    // Projects: a header per project, with members in their own project.
    const auto keys = preparedKeys(rows);
    QVERIFY(keys.contains("#group\nungrouped")); QVERIFY(keys.contains("#group\n" + second));
    QVERIFY(keys.indexOf("#group\n" + second) < keys.indexOf("mac\nclaude/infra/review"));
    QCOMPARE(preparedRow(rows, "#group\n" + second)->roles.value(SessionRoles::Total).toInt(), 2);
    // Subagents: active children of an expanded session, the finished one hidden.
    QVERIFY(keys.contains("\nclaude/hgs/subs\n#child\na")); QVERIFY(keys.contains("\nclaude/hgs/subs\n#child\nc"));
    QVERIFY(!keys.contains("\nclaude/hgs/subs\n#child\nb"));
    QVERIFY(preparedRow(rows, "\nclaude/hgs/subs")->roles.value(SessionRoles::HasChildren).toBool());
    QVERIFY(preparedRow(rows, "\nclaude/hgs/subs\n#child\nc")->roles.value(SessionRoles::Attention).toBool());
    auto completed = inputs; completed.includeCompleted = true;
    QVERIFY(preparedKeys(prepared(completed)).contains("\nclaude/hgs/subs\n#child\nb"));
    // A peer past its poll age is offline: never working, marked offline.
    const auto *stale = preparedRow(rows, "old\ncodex/old/stale"); QVERIFY(stale);
    QCOMPARE(stale->roles.value(SessionRoles::Host).toString(), QString("old (offline)"));
    QVERIFY(!stale->roles.value(SessionRoles::Working).toBool());
    QVERIFY(preparedRow(rows, "\ncodex/hgs/dashboard")->roles.value(SessionRoles::Working).toBool());
    QVERIFY(!preparedRow(rows, "\ncodex/hgs/dashboard")->roles.value(Qt::AccessibleTextRole).toString().isEmpty());
    // Active: current work, Scheduled and Needs attention; a draft adds a stopped session.
    auto active = inputs; active.filter = "active";
    rows = prepared(active);
    const auto sessions = [](const SessionsWindow::PreparedRows &value) {
        QStringList result;
        for (const auto &row : value.rows) if (row.kind == SessionsWindow::PreparedRow::Session) result << row.key;
        std::sort(result.begin(), result.end()); return result;
    };
    for (const auto *key : {"\nclaude/hgs/subs", "\ncodex/hgs/dashboard", "\ncodex/hgs/scheduled", "mac\nclaude/infra/review"})
        QVERIFY2(sessions(rows).contains(key), key);
    for (const auto *key : {"\ncodex/hgs/stopped", "\ncodex/website/navigation", "old\ncodex/old/stale"})
        QVERIFY2(!sessions(rows).contains(key), key);
    const int activeCount = rows.counts.value("active");
    QCOMPARE(activeCount, sessions(rows).size());
    active.drafts.insert("\ncodex/hgs/stopped");
    rows = prepared(active);
    QVERIFY(sessions(rows).contains("\ncodex/hgs/stopped")); QCOMPARE(rows.counts.value("active"), activeCount + 1);
    QVERIFY(preparedRow(rows, "\ncodex/hgs/stopped")->roles.value(SessionRoles::Draft).toBool());
    QVERIFY(rows.drafts.contains("\ncodex/hgs/stopped"));
    // A subagent's unsent draft counts for its session too.
    auto childDraft = inputs; childDraft.filter = "active";
    childDraft.childDrafts.insert("arch\ncodex/hgs/stopped\n\nchild");
    auto stoppedEntry = std::find_if(childDraft.entries.begin(), childDraft.entries.end(), [](const auto &e) { return e.key == "\ncodex/hgs/stopped"; });
    QVERIFY(stoppedEntry != childDraft.entries.end());
    stoppedEntry->session.subagents = QJsonObject{{"child", QJsonObject{{"name", "Child"}, {"state", "finished"}}}};
    QVERIFY(prepared(childDraft).drafts.contains("\ncodex/hgs/stopped"));
    // Needs attention: the approval on the peer and the child asking for approval.
    auto attention = inputs; attention.filter = "attention";
    rows = prepared(attention);
    QVERIFY(sessions(rows).contains("\nclaude/hgs/subs")); QVERIFY(sessions(rows).contains("mac\nclaude/infra/review"));
    for (const auto *key : {"\ncodex/hgs/dashboard", "\ncodex/hgs/scheduled", "\ncodex/hgs/stopped", "old\ncodex/old/stale"})
        QVERIFY2(!sessions(rows).contains(key), key);
    QCOMPARE(rows.counts.value("attention"), sessions(rows).size());
    // A machine filter keeps that machine's rows and counts sessions per machine.
    auto mac = inputs; mac.hostFilters = {"mac"};
    rows = prepared(mac);
    QCOMPARE(sessions(rows), QStringList{"mac\nclaude/infra/review"});
    QCOMPARE(rows.machineCounts.value("@local"), 6); QCOMPARE(rows.machineCounts.value("mac"), 1);
    // A collapsed project hides its members but keeps its header.
    auto collapsed = inputs; collapsed.organization.setCollapsed(second, true);
    rows = prepared(collapsed);
    QVERIFY(!preparedRow(rows, "#group\n" + second)->hidden);
    QVERIFY(preparedRow(rows, "mac\nclaude/infra/review")->hidden);
    QVERIFY(preparedRow(rows, "#group\n" + second)->roles.value(SessionRoles::Collapsed).toBool());
    // Equal inputs prepare equal rows; a change changes only that row's digest.
    QCOMPARE(prepared(inputs).rows.first().digest, prepared(inputs).rows.first().digest);
    auto detail = inputs;
    for (auto &entry : detail.entries) if (entry.key == "\ncodex/hgs/dashboard") entry.session.model = "another-model";
    const auto before = prepared(inputs), after = prepared(detail);
    QCOMPARE(before.rows.size(), after.rows.size());
    for (int i = 0; i < before.rows.size(); ++i)
        QCOMPARE(before.rows[i].digest == after.rows[i].digest, before.rows[i].key != "\ncodex/hgs/dashboard");
}

void TestSessionsWindow::rowPreparationNeverRunsOnTheGuiThread()
{
    for (const auto mode : {RowPreparation::Async, RowPreparation::Blocking}) {
        SessionsWindow::setRowPreparationForTests(mode);
        const auto restore = qScopeGuard([] { SessionsWindow::setRowPreparationForTests(RowPreparation::Blocking); });
        const int violations = JsonParse::violations();
        SessionsWindow window(program()); keepDetailsPollsOut(window);
        window.setFleet(rowFleet(QDateTime::currentMSecsSinceEpoch())); window.show();
        settleRows(window);
        auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list->count() > 0);
        QCOMPARE(JsonParse::violations(), violations);
    }
    // A preparation on the GUI thread is a violation, fatal in every Qt suite.
    SessionsWindow window(program()); keepDetailsPollsOut(window);
    window.setFleet(rowFleet(QDateTime::currentMSecsSinceEpoch())); window.show(); settleRows(window);
    QStringList reported;
    JsonParse::setViolationHandler([&reported](const char *file, int) { reported << QString::fromUtf8(file); });
    const auto restore = qScopeGuard([] { JsonParse::setViolationHandler({}); });
    const auto rows = SessionsWindow::prepareRows(window.rowInputsForTests());
    QVERIFY(!rows.rows.isEmpty());
    QCOMPARE(reported.size(), 1); QVERIFY2(reported[0].endsWith("SessionsWindowRows.cpp"), qPrintable(reported[0]));
}

void TestSessionsWindow::rowPreparationDropsStaleResults()
{
    AsyncRows async;
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 760);
    const auto now = QDateTime::currentMSecsSinceEpoch();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.setFleet(rowFleet(now)); window.show(); settleRows(window);
    // The first row is selected and inspected; its reply may rebuild the rows.
    QTRY_VERIFY(!inspected.isEmpty()); QTest::qWait(100); settleRows(window);
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto keys = [list] { QStringList result; for (int i = 0; i < list->count(); ++i) result << list->item(i)->data(SessionRoles::Key).toString(); return result; };
    // Hold the preparation thread so that two preparations wait behind it.
    QSemaphore gate; QObject holder; bool held = false;
    ProcessRunner::post(&holder, [&gate] { gate.acquire(); return true; }, [&held](bool) { held = true; });
    // An early assertion must release and drain the preparation worker before
    // the stack semaphore and its owner disappear.
    const auto unblock = qScopeGuard([&gate] {
        gate.release();
        ProcessRunner::prepareAndWait([] {});
    });
    const auto listing = [&](const QString &name) {
        auto state = rowFleet(now); auto local = state.local(); local.sessions.append(rowSession(name, "codex"));
        state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); return state;
    };
    QSignalSpy applied(&window, &SessionsWindow::rowsApplied);
    const auto dropped = window.droppedRowPreparationsForTests();
    const auto start = window.rowGenerationForTests();
    window.setFleet(listing("codex/hgs/first"));
    QTRY_VERIFY(window.rowGenerationForTests() > start);
    const auto first = window.rowGenerationForTests();
    window.setFleet(listing("codex/hgs/second"));
    QTRY_VERIFY(window.rowGenerationForTests() > first);
    // Nothing is applied while both wait: the list still shows the earlier rows.
    QVERIFY(applied.isEmpty()); QVERIFY(!keys().contains("\ncodex/hgs/first"));
    gate.release();
    QTRY_VERIFY(held);
    settleRows(window);
    QVERIFY(!applied.isEmpty());
    for (const auto &signal : applied) QVERIFY(signal[0].toULongLong() > first);
    QCOMPARE(applied.last()[0].toULongLong(), window.rowGenerationForTests());
    QVERIFY(window.droppedRowPreparationsForTests() > dropped);
    QVERIFY(keys().contains("\ncodex/hgs/second")); QVERIFY(!keys().contains("\ncodex/hgs/first"));
}

void TestSessionsWindow::rowPreparationsNeverQueueBehindEachOther()
{
    AsyncRows async;
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 760);
    const auto now = QDateTime::currentMSecsSinceEpoch();
    window.setFleet(rowFleet(now)); window.show(); settleRows(window);
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto keys = [list] { QStringList result; for (int i = 0; i < list->count(); ++i) result << list->item(i)->data(SessionRoles::Key).toString(); return result; };
    // Hold the preparation thread, as a slow preparation would.
    QSemaphore gate; QObject holder; bool held = false;
    ProcessRunner::post(&holder, [&gate] { gate.acquire(); return true; }, [&held](bool) { held = true; });
    // An early assertion must release and drain the preparation worker before
    // the stack semaphore and its owner disappear.
    const auto unblock = qScopeGuard([&gate] {
        gate.release();
        ProcessRunner::prepareAndWait([] {});
    });
    const auto posted = window.postedRowPreparationsForTests();
    for (int i = 0; i < 10; ++i) {
        auto state = rowFleet(now); auto local = state.local(); local.sessions.append(rowSession(QString("codex/hgs/burst-%1").arg(i), "codex"));
        state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
        QTest::qWait(5);
    }
    // One preparation waits behind the held one; the other nine requests wait for it.
    QTRY_COMPARE(window.postedRowPreparationsForTests(), posted + 1);
    QTest::qWait(50);
    QCOMPARE(window.postedRowPreparationsForTests(), posted + 1);
    // A reply posted now waits behind at most that one preparation: when it
    // arrives, no more than the folded follow-up has been posted.
    QObject reply; int postedAtReply = -1;
    ProcessRunner::post(&reply, [] { return true; }, [&postedAtReply, &window](bool) { postedAtReply = window.postedRowPreparationsForTests(); });
    gate.release();
    QTRY_VERIFY(held); QTRY_VERIFY(postedAtReply >= 0);
    QVERIFY2(postedAtReply <= posted + 2, qPrintable(QString::number(postedAtReply - posted)));
    settleRows(window);
    // The folded requests prepared the last listing.
    QVERIFY(keys().contains("\ncodex/hgs/burst-9")); QVERIFY(!keys().contains("\ncodex/hgs/burst-8"));
}

void TestSessionsWindow::rowPreparationKeepsItemsSelectionAndScroll()
{
    AsyncRows async;
    BoxState box; box.host = "arch"; box.ok = true;
    for (int i = 0; i < 60; ++i) box.sessions.append(rowSession(QString("codex/scaling/session-%1").arg(i, 2, 10, QChar('0')), "codex"));
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 600);
    window.setFleet(state); window.show(); QVERIFY(QTest::qWaitForWindowExposed(&window)); settleRows(window);
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const QString selectedKey = "\ncodex/scaling/session-40";
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.showSession({}, "codex/scaling/session-40"); settleRows(window);
    QTRY_COMPARE(list->currentItem() ? list->currentItem()->data(SessionRoles::Key).toString() : QString(), selectedKey);
    // Its inspection merges into its row; wait for it before comparing items.
    QTRY_VERIFY(std::any_of(inspected.cbegin(), inspected.cend(), [](const auto &row) { return row[1].toString() == "codex/scaling/session-40"; }));
    QTest::qWait(100); settleRows(window);
    auto *bar = list->verticalScrollBar(); QVERIFY(bar->maximum() > 0);
    bar->setValue(bar->maximum() / 2); const int scroll = bar->value();
    const auto items = [list] {
        QHash<QString, QListWidgetItem *> result;
        for (int i = 0; i < list->count(); ++i) result.insert(list->item(i)->data(SessionRoles::Key).toString(), list->item(i));
        return result;
    };
    const auto digests = [list] {
        QHash<QString, QByteArray> result;
        for (int i = 0; i < list->count(); ++i) result.insert(list->item(i)->data(SessionRoles::Key).toString(), list->item(i)->data(SessionRoles::Digest).toByteArray());
        return result;
    };
    const auto before = items(); const auto beforeDigests = digests();
    auto *selectedItem = list->currentItem();
    // One changed row: the same items stay, only its roles are written.
    QSignalSpy changedRoles(list->model(), &QAbstractItemModel::dataChanged);
    box.sessions[10].activity = "busy"; box.sessions[10].phase = "tool"; box.sessions[10].activityDetail = "Running tests";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); settleRows(window);
    QCOMPARE(items(), before);
    QCOMPARE(list->currentItem(), selectedItem); QVERIFY(selectedItem->isSelected());
    QCOMPARE(bar->value(), scroll);
    const QString changedKey = "\ncodex/scaling/session-10";
    QVERIFY(before.value(changedKey)->data(SessionRoles::Working).toBool());
    for (auto it = beforeDigests.cbegin(); it != beforeDigests.cend(); ++it)
        QCOMPARE(it.value() == digests().value(it.key()), it.key() != changedKey);
    QVERIFY(!changedRoles.isEmpty());
    for (const auto &signal : changedRoles) QCOMPARE(signal[0].toModelIndex().data(SessionRoles::Key).toString(), changedKey);
    // A new row and a removed one: the other items, selection and scroll stay.
    box.sessions.removeAt(5); box.sessions.append(rowSession("codex/scaling/session-60", "codex"));
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); settleRows(window);
    const auto after = items();
    QCOMPARE(after.size(), before.size());
    QVERIFY(!after.contains("\ncodex/scaling/session-05")); QVERIFY(after.contains("\ncodex/scaling/session-60"));
    for (auto it = before.cbegin(); it != before.cend(); ++it)
        if (it.key() != "\ncodex/scaling/session-05") QCOMPARE(after.value(it.key()), it.value());
    QCOMPARE(list->currentItem(), selectedItem); QCOMPARE(list->selectedItems(), QList<QListWidgetItem *>{selectedItem});
    QCOMPARE(bar->value(), scroll);
    // Rows in the order prepared, as a new list would show them.
    const auto rows = prepared(window.rowInputsForTests());
    QCOMPARE(list->count(), rows.rows.size());
    for (int i = 0; i < rows.rows.size(); ++i) QCOMPARE(list->item(i)->data(SessionRoles::Key).toString(), rows.rows[i].key);
}

void TestSessionsWindow::rowPreparationRevealsSessionsAfterFilterChanges()
{
    AsyncRows async;
    BoxState box; box.host = "arch"; box.ok = true;
    for (int i = 0; i < 40; ++i) box.sessions.append(rowSession(QString("codex/reveal/session-%1").arg(i, 2, 10, QChar('0')), "codex"));
    box.sessions[0].activity = "busy"; box.sessions[0].phase = "approval";
    FleetState state; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1100, 600);
    window.setFleet(state); window.show(); QVERIFY(QTest::qWaitForWindowExposed(&window)); settleRows(window);
    auto *list = window.findChild<SessionList *>("sessionList");
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) if (button->property("filter") == "attention") button->click();
    settleRows(window); QCOMPARE(list->count(), 1);
    // Opening a session the filter hides lists every session again, then selects
    // and shows it once those rows are applied.
    window.showSession({}, "codex/reveal/session-35");
    QVERIFY(window.rowsPendingForTests());
    settleRows(window);
    QCOMPARE(list->count(), 40);
    auto *item = list->currentItem(); QVERIFY(item);
    QCOMPARE(item->data(SessionRoles::Key).toString(), QString("\ncodex/reveal/session-35"));
    QVERIFY(list->viewport()->rect().intersects(list->visualItemRect(item)));
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QTRY_COMPARE(composer->sessionKey(), QString("\ncodex/reveal/session-35"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_list4.moc"
