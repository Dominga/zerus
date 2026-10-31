// U62 (#712): The session list: rows, states, filters, selection and menus, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void unreadRepliesNeedAnActiveVisibleResult();
    void markAllReadIgnoresFiltersAndKeepsCurrentDraft();
    void markAllReadFromTheRailAndBesideDraftsSkipsReviewReminders();
    void sessionMuteMenuPersistsWithoutAcknowledging();
    void unsentDraftBecomesARowStatus();
    void filtersSelectionAndOffline();
    void refreshedListingsKeepRowsUnlessTimeChangesThem();
    void organizationIsSavedOnlyWhenItChanges();
    void listFiltersKeepTheOpenConversation_data();
    void listFiltersKeepTheOpenConversation();
    void activeFilterCombinesWorkAndAttentionOnce();
    void activeFilterDraftsRestoreOfflineAndFollowEditsAndDelivery();
    void sessionContextMenuSignalsAndKeyboard();
    void compactSessionRowActionsPreserveSelectionAndDraft();
    void selectedSessionRowsCompactAndExpandLocally();
    void shiftRangeSurvivesFilteringReorderAndDeletedAnchor();
    void compactSessionRowsPersistAndExpandAllIncludesHidden();
};

void TestSessionsWindow::unreadRepliesNeedAnActiveVisibleResult()
{
    auto state = fleet(); auto box = state.local(); auto &session = box.sessions[0];
    session.activity = "idle"; session.phase = "idle"; session.conversationId = "read-conversation";
    session.replyId = "10:100"; session.runId = "read-run"; session.currentTool.clear();
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionOrganization org; const auto group = org.createGroup("Results");
    org.moveSession(org.observe("arch", session.name, session.runId), group); org.setCollapsed(group, true);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    const QString fixture = m_dir->filePath("unread-hgs"); QFile file(fixture); QVERIFY(file.open(QIODevice::WriteOnly));
    const QJsonObject details{{"tracked", true}, {"run_id", session.runId}, {"conversation_id", session.conversationId}, {"reply_id", session.replyId},
        {"activity", "idle"}, {"phase", "idle"}, {"cursor", 10}, {"events", QJsonArray{QJsonObject{{"seq", 10}, {"type", "Stop"}, {"at", 100},
        {"detail", QString("A long response paragraph.\n\n").repeated(90)}}}}};
    file.write("#!/usr/bin/env python3\nimport json\nprint(" + QJsonDocument(QJsonArray{QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact))}).toJson(QJsonDocument::Compact) + "[0])\n");
    file.close(); QVERIFY(FakeCli::makeRunnable(file.fileName()));
    SessionsWindow window(FakeCli::program(fixture)); window.setFleet(state); window.show(); window.findChild<QPushButton *>("brandMark")->click();
    QSignalSpy seen(&window, &SessionsWindow::replyViewed);
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto row = [&]() -> QListWidgetItem * { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Identity).toString() == org.observe("arch", session.name, session.runId)) return list->item(i); return nullptr; };
    QVERIFY(row()); QVERIFY(row()->data(SessionRoles::Unread).toBool()); QVERIFY(row()->isHidden());
    for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Header).toBool() && list->item(i)->data(SessionRoles::Group) == group)
        QCOMPARE(list->item(i)->data(SessionRoles::Unread).toInt(), 1);
    auto *dash = window.findChild<DashboardPage *>(); QVERIFY(dash->findChild<QPushButton *>("dashboardAttention")->text().startsWith("2"));
    window.showSession({}, session.name); window.hide(); QTest::qWait(1650); QCOMPARE(seen.size(), 0);
    window.show(); window.activateWindow(); QApplication::setActiveWindow(&window);
    auto *activity = window.findChild<ActivityView *>("mainActivity"); auto *scroll = activity->browser()->verticalScrollBar();
    QTRY_VERIFY(scroll->maximum() > 0); scroll->setValue(0); QTest::qWait(1650); QCOMPARE(seen.size(), 0);
    // A result loaded behind another app is still unread.
    QWidget other; other.show(); other.activateWindow(); QApplication::setActiveWindow(&other);
    activity->jumpToLatest(); QTest::qWait(1650); QCOMPARE(seen.size(), 0); other.hide();
    window.activateWindow(); QApplication::setActiveWindow(&window);
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "attention") button->click();
    QTRY_COMPARE_WITH_TIMEOUT(seen.size(), 1, 3500);
    QCOMPARE(seen[0], QVariantList({QString(), session.name, session.conversationId, session.replyId}));
    QVERIFY(row()); QVERIFY(!row()->data(SessionRoles::Unread).toBool());
    QCOMPARE(list->currentItem()->data(SessionRoles::Identity), row()->data(SessionRoles::Identity));
    QVERIFY(state.markReplyRead({}, session.name, session.conversationId, session.replyId));
    window.setFleet(state); QVERIFY(!row()->data(SessionRoles::Unread).toBool());
    window.hide(); box.sessions[0].replyId = "11:101"; state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    window.show(); window.findChild<QPushButton *>("brandMark")->click();
    QVERIFY(row()->data(SessionRoles::Unread).toBool());
    // A previous unread reply must not hide the next turn's live activity.
    box.sessions[0].activity = "busy"; box.sessions[0].phase = "tool";
    box.sessions[0].activitySummary = "Bash"; box.sessions[0].activityDetail = "Check the next deployment";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    // The hidden list renders when shown; reading the reply takes a visible dwell.
    window.showSessionList();
    QVERIFY(!row()->data(SessionRoles::Unread).toBool()); QVERIFY(row()->data(SessionRoles::Working).toBool());
    QVERIFY(state.local().sessions[0].unreadReply);
    QCOMPARE(row()->data(SessionRoles::Detail).toString(), QString("Bash: Check the next deployment"));
    QVERIFY(window.findChild<QLabel *>("listSummary")->text().contains("1 working"));
    box.sessions[0].activity = "idle"; box.sessions[0].phase = "idle";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(row()->data(SessionRoles::Unread).toBool());
    QCOMPARE(state.local().sessions[0].replyId, QString("11:101"));
}

void TestSessionsWindow::markAllReadIgnoresFiltersAndKeepsCurrentDraft()
{
    auto state = fleet(); auto box = state.local();
    for (int i : {0, 1, 2}) {
        box.sessions[i].conversationId = QString("bulk-%1").arg(i); box.sessions[i].replyId = "10:100";
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    auto remote = *state.peer("mac"); remote.sessions[0].conversationId = "bulk-remote"; remote.sessions[0].replyId = "20:200";
    state.setPeer(remote, QDateTime::currentMSecsSinceEpoch());
    remote.host = "sleeping"; state.setPeer(remote, 0); remote.ok = false; state.setPeer(remote, 1);
    SessionOrganization org; const auto group = org.createGroup("Collapsed");
    org.moveSession(org.observe("arch", box.sessions[1].name, box.sessions[1].runId), group); org.setCollapsed(group, true);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, box.sessions[0].name);
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({"@local"});
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "attention") button->click();
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep this draft");
    auto *list = window.findChild<SessionList *>("sessionList"); const auto selectedKey = composer->sessionKey();
    auto *action = window.findChild<QAction *>("markAllSessionsRead"); QVERIFY(action); QVERIFY(action->isEnabled());
    QVERIFY(action->text().contains("(5)"));
    // Marks are shared through each session's host, not kept on this device.
    QCOMPARE(action->toolTip(), QString("Mark current replies across all machines as read, including hidden sessions. "
                                        "Pending approvals, questions and errors stay highlighted. "
                                        "Sessions you marked as needing attention keep their reply unread."));
    auto *context = window.findChild<QMenu *>("sessionProjectsMenu"); QVERIFY(context); QVERIFY(context->actions().contains(action));
    auto *batch = window.findChild<QPushButton *>("batchActions")->menu(); QMetaObject::invokeMethod(batch, "aboutToShow");
    QVERIFY(batch->actions().contains(action));
    QSignalSpy seen(&window, &SessionsWindow::repliesMarkedRead);
    QSignalSpy writes(window.findChild<HgsClient *>(), &HgsClient::writeDone);
    // Model the root's persisted read state and its next regular fleet update.
    connect(&window, &SessionsWindow::repliesMarkedRead, &window, [&](const QJsonObject &replies) {
        QCOMPARE(state.markRepliesRead(replies), replies.size());
        setJsonSetting("attention/readReplies", QJsonDocument(state.readReplies()).toJson(QJsonDocument::Compact));
        window.setFleet(state);
    });
    action->trigger(); QCOMPARE(seen.size(), 1); QCOMPARE(seen[0][0].toJsonObject().size(), 5);
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("5 replies marked as read."));
    QVERIFY(!action->isEnabled()); QVERIFY(state.unreadReplies().isEmpty()); QCOMPARE(writes.size(), 0);
    QVERIFY(list->currentItem()); QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selectedKey);
    QCOMPARE(composer->sessionKey(), selectedKey); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
    // Overview renders when shown.
    window.findChild<QPushButton *>("brandMark")->click();
    QVERIFY(window.findChild<DashboardPage *>()->findChild<QPushButton *>("dashboardAttention")->text().startsWith("1"));
    window.showSessionList(); window.findChild<MachineFilter *>("machineFilterChips")->setSelection({});
    window.findChild<QLineEdit *>("search")->setText("review");
    QCOMPARE(window.findChild<QListWidget *>("searchResults")->count(), 1); // The real approval still needs attention.
    FleetState restarted; restarted.setReadReplies(QJsonDocument::fromJson(QSettings().value("attention/readReplies").toByteArray()).object());
    restarted.setLocal(box, QDateTime::currentMSecsSinceEpoch()); QVERIFY(restarted.unreadReplies().isEmpty());
    box.sessions[0].replyId = "11:101"; state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(action->isEnabled()); QVERIFY(action->text().contains("(1)")); // Even when that session is hidden by search.
    action->trigger(); QCOMPARE(seen.size(), 2); QCOMPARE(seen[1][0].toJsonObject().size(), 1); QVERIFY(!action->isEnabled());
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("1 reply marked as read."));
    QSettings().remove("attention/readReplies");
}

void TestSessionsWindow::markAllReadFromTheRailAndBesideDraftsSkipsReviewReminders()
{
    const auto previousMarks = QSettings().value("attention/sessionMarks");
    const auto restore = qScopeGuard([previousMarks] {
        setJsonSetting("attention/sessionMarks", previousMarks); QSettings().remove("attention/readReplies");
    });
    const auto now = QDateTime::currentMSecsSinceEpoch();
    auto state = fleet(); auto box = state.local();
    for (int i : {0, 1, 2}) { box.sessions[i].conversationId = QString("rail-%1").arg(i); box.sessions[i].replyId = "10:100"; }
    box.sessions[2].state = "running"; box.sessions[2].activity = "idle"; box.sessions[2].phase = "input"; // A pending question.
    state.setLocal(box, now);
    auto remote = *state.peer("mac"); remote.sessions[0].conversationId = "rail-remote"; remote.sessions[0].replyId = "20:200";
    state.setPeer(remote, now);
    // The person kept one session here and the remote approval for later.
    QVERIFY(state.setReviewLater({}, box.sessions[1], true)); QVERIFY(state.setReviewLater("mac", remote.sessions[0], true));
    setJsonSetting("attention/sessionMarks", QJsonDocument(state.attentionMarks()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, box.sessions[0].name);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep this draft");
    const auto selectedKey = composer->sessionKey();
    auto *action = window.findChild<QAction *>("markAllSessionsRead"); QVERIFY(action); QVERIFY(action->isEnabled());
    QCOMPARE(action->text(), QString("Mark all as read (2)"));
    // Right-clicking the Sessions icon in the rail offers the same action.
    auto *railMenu = window.findChild<QMenu *>("sessionsNavMenu"); QVERIFY(railMenu);
    QCOMPARE(railMenu->actions().size(), 2); QCOMPARE(railMenu->actions().constFirst(), action);
    auto *expandAll = window.findChild<QAction *>("expandAllCollapsedSessionRows"); QVERIFY(expandAll);
    QCOMPARE(railMenu->actions()[1], expandAll);
    auto *rail = qobject_cast<QPushButton *>(railMenu->parent()); QVERIFY(rail); QCOMPARE(rail->property("glyph").toString(), QString("sessions"));
    QContextMenuEvent event(QContextMenuEvent::Mouse, QPoint(8, 8), rail->mapToGlobal(QPoint(8, 8))); QApplication::sendEvent(rail, &event);
    QTRY_VERIFY(railMenu->isVisible()); railMenu->close();
    // The icon beside Saved drafts runs it too, with the same count for assistive technology.
    auto *button = window.findChild<QPushButton *>("markAllReadButton"), *drafts = window.findChild<QPushButton *>("savedDrafts");
    QVERIFY(button); QVERIFY(drafts); QVERIFY(button->isVisible()); QVERIFY(button->isEnabled());
    QHBoxLayout *summary = nullptr;
    for (auto *row : drafts->parentWidget()->findChildren<QHBoxLayout *>()) if (row->indexOf(drafts) >= 0) summary = row;
    QVERIFY(summary); QCOMPARE(summary->indexOf(button) + 1, summary->indexOf(drafts));
    QCOMPARE(button->accessibleName(), action->text()); QCOMPARE(button->toolTip(), action->text() + "\n" + action->toolTip());
    QSignalSpy seen(&window, &SessionsWindow::repliesMarkedRead);
    connect(&window, &SessionsWindow::repliesMarkedRead, &window, [&](const QJsonObject &replies) {
        QCOMPARE(state.markRepliesRead(replies), replies.size());
        setJsonSetting("attention/readReplies", QJsonDocument(state.readReplies()).toJson(QJsonDocument::Compact));
        window.setFleet(state);
    });
    button->click(); QCOMPARE(seen.size(), 1); QCOMPARE(seen[0][0].toJsonObject().size(), 2);
    QCOMPARE(window.findChild<QLabel *>("notice")->text(), QString("2 replies marked as read."));
    // Reminded sessions keep their unread reply and their reminder; the question stays highlighted.
    QVERIFY(state.local().sessions[1].unreadReply); QVERIFY(state.local().sessions[1].reviewLater);
    QVERIFY(state.peer("mac")->sessions[0].unreadReply); QVERIFY(state.peer("mac")->sessions[0].reviewLater);
    QVERIFY(!state.local().sessions[0].unreadReply);
    QVERIFY(!state.local().sessions[2].unreadReply); QVERIFY(state.local().sessions[2].needsAttention());
    QVERIFY(!action->isEnabled()); QVERIFY(!button->isEnabled()); QCOMPARE(action->text(), QString("Mark all as read"));
    QCOMPARE(button->accessibleName(), QString("Mark all as read"));
    QCOMPARE(composer->sessionKey(), selectedKey); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft"));
}

void TestSessionsWindow::sessionMuteMenuPersistsWithoutAcknowledging()
{
    const auto preferences=[](bool muted,qint64 revision) { return QJsonObject{{"conversation_id","mute-conversation"},{"muted",muted},
        {"revision",revision},{"suppressed",QJsonObject{{"reply_id",QJsonValue()},{"attention_signature",QJsonValue()},
        {"optional_question_ids",QJsonArray()},{"main_attention",QJsonValue()},{"child_attention",QJsonObject()}}}}; };
    const auto fixture=m_dir->filePath("shared-notifications");QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QByteArray behavior=R"PY(elif c=='--help':print('session-notifications-v1')
elif c=='notifications':
 import pathlib,time
 payload=json.load(sys.stdin)
 with open(__file__+'.calls','a') as output:output.write(json.dumps({'argv':sys.argv[1:],'payload':payload})+'\n')
 deadline=time.monotonic()+15
 while not pathlib.Path(__file__+'.release').exists():
  if time.monotonic()>=deadline:sys.exit(2)
  time.sleep(.01)
 print(pathlib.Path(__file__+'.reply').read_text())
)PY";
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+behavior)));
    const auto reply=[&](bool muted,qint64 revision,const QString &run="run") {
        return FakeCli::replace(fixture+".reply",QJsonDocument(QJsonObject{{"ok",true},{"applied",true},{"run_id",run},
            {"conversation_id","mute-conversation"},{"preferences",preferences(muted,revision)}}).toJson());
    };
    const auto calls=[&] { QFile file(fixture+".calls");return file.open(QIODevice::ReadOnly)?file.readAll().count('\n'):0; };
    auto state=fleet(); auto box=state.local();
    box.sessions[0].conversationId="mute-conversation"; box.sessions[0].runId="run";
    box.sessions[0].phase="approval"; box.sessions[0].activity="waiting"; box.sessions[0].replyId="reply-1";
    box.sessions[0].sharedNotifications=true; box.sessions[0].notificationPreferences=preferences(false,0);
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());
    const auto originalMarks=QSettings().value("attention/sessionMarks");
    SessionsWindow window(FakeCli::program(fixture)); keepDetailsPollsOut(window); window.setFleet(state); window.show();
    auto *list=window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    auto *menu=window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    auto *rail=window.findChild<QLabel *>("railAttentionBadge"); QVERIFY(rail);
    QSignalSpy marks(&window,&SessionsWindow::attentionMarksChanged);
    QSignalSpy shared(&window,&SessionsWindow::notificationPreferencesChanged), refreshed(&window,&SessionsWindow::refreshRequested);
    auto *client=window.findChild<HgsClient *>();QVERIFY(client);QSignalSpy completed(client,&HgsClient::notificationsFinished);
    bool applied=false;
    connect(&window,&SessionsWindow::notificationPreferencesChanged,&window,[&](const QString &host,const QString &,const QString &,const QString &,const QJsonObject &value) {
        applied=state.applyNotificationPreferences(host,state.local().sessions.first(),value);window.setFleet(state);
    });
    list->setCurrentRow(1);
    auto *composer=window.findChild<MessageComposer *>("messageComposer");setComposerText(composer,"Keep this draft");
    const auto open=[&] {
        menu->hide(); const auto point=list->visualItemRect(list->item(0)).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse,point,list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(),&event);
    };
    QVERIFY(list->item(0)->data(SessionRoles::Attention).toBool()); QVERIFY(list->item(0)->data(SessionRoles::Unread).toBool());
    const int count=rail->text().toInt(); QVERIFY(count>0);
    open(); auto *mute=menu->findChild<QAction *>("contextToggleMute"); QVERIFY(mute); QCOMPARE(mute->text(),QString("Mute notifications"));
    QVERIFY(mute->isEnabled());mute->trigger();menu->hide();QTRY_COMPARE(calls(),1);
    QVERIFY(!list->item(0)->data(SessionRoles::Muted).toBool());QCOMPARE(marks.size(),0);QCOMPARE(shared.size(),0);
    open();mute=menu->findChild<QAction *>("contextToggleMute");QVERIFY(!mute->isEnabled());menu->hide();
    QVERIFY(reply(true,1));QVERIFY(FakeCli::replace(fixture+".release","release"));QTRY_COMPARE(shared.size(),1);QVERIFY(applied);
    QCOMPARE(marks.size(),0); QCOMPARE(list->currentRow(),1);QCOMPARE(composer->editor()->toPlainText(),QString("Keep this draft"));
    QVERIFY(list->item(0)->data(SessionRoles::Muted).toBool()); QVERIFY(list->item(0)->data(SessionRoles::Attention).toBool());
    QVERIFY(list->item(0)->data(SessionRoles::Unread).toBool()); QCOMPARE(rail->text().toInt(),count-1);
    QVERIFY(list->item(0)->data(Qt::AccessibleTextRole).toString().contains("Muted on all devices"));
    QCOMPARE(QSettings().value("attention/sessionMarks"),originalMarks);
    QFile log(fixture+".calls");QVERIFY(log.open(QIODevice::ReadOnly));const auto call=QJsonDocument::fromJson(log.readLine()).object();log.close();
    QCOMPARE(call["argv"].toArray(),(QJsonArray{"notifications",box.sessions.first().name,"--json"}));
    QCOMPARE(call["payload"].toObject(),(QJsonObject{{"run_id","run"},{"conversation_id","mute-conversation"},{"muted",true},{"expected_revision",0}}));
    window.setFleet(state); QVERIFY(list->item(0)->data(SessionRoles::Muted).toBool());
    SessionsWindow reopened(FakeCli::program(fixture)); keepDetailsPollsOut(reopened); reopened.setFleet(state); reopened.show();
    QVERIFY(reopened.findChild<QListWidget *>("sessionList")->item(0)->data(SessionRoles::Muted).toBool());
    open(); mute=menu->findChild<QAction *>("contextToggleMute"); QVERIFY(mute); QCOMPARE(mute->text(),QString("Unmute notifications"));
    // Failure retains the last confirmed value. No local mark, read or retry is created.
    QVERIFY(QFile::remove(fixture+".release"));mute->trigger();menu->hide();QTRY_COMPARE(calls(),2);
    QVERIFY(FakeCli::replace(fixture+".reply","{\"ok\":false,\"error\":\"stale\"}"));
    QVERIFY(FakeCli::replace(fixture+".release","release"));QTRY_COMPARE(completed.size(),2);
    QCOMPARE(shared.size(),1);QVERIFY(list->item(0)->data(SessionRoles::Muted).toBool());QCOMPARE(calls(),2);
    QVERIFY(QFile::remove(fixture+".release"));open();mute=menu->findChild<QAction *>("contextToggleMute");QVERIFY(mute->isEnabled());
    mute->trigger();menu->hide();QTRY_COMPARE(calls(),3);
    QVERIFY(reply(false,2));QVERIFY(FakeCli::replace(fixture+".release","release"));QTRY_COMPARE(shared.size(),2);QVERIFY(applied);
    QVERIFY(!list->item(0)->data(SessionRoles::Muted).toBool());QCOMPARE(rail->text().toInt(),count);
    QVERIFY(list->item(0)->data(SessionRoles::Attention).toBool());QVERIFY(list->item(0)->data(SessionRoles::Unread).toBool());
    // A confirmed old-run reply cannot affect a replacement, even in the same conversation.
    QVERIFY(QFile::remove(fixture+".release"));open();mute=menu->findChild<QAction *>("contextToggleMute");QVERIFY(mute->isEnabled());
    mute->trigger();menu->hide();QTRY_COMPARE(calls(),4);
    box=state.local();box.sessions[0].runId="replacement";state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QVERIFY(reply(true,3));QVERIFY(FakeCli::replace(fixture+".release","release"));QTRY_COMPARE(completed.size(),4);
    QCOMPARE(shared.size(),2);QVERIFY(!list->item(0)->data(SessionRoles::Muted).toBool());QCOMPARE(refreshed.size(),0);
    box=state.local();box.ok=false;state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);open();
    mute=menu->findChild<QAction *>("contextToggleMute");QVERIFY(!mute->isEnabled());QVERIFY(mute->toolTip().contains("Connect"));menu->hide();QCOMPARE(calls(),4);
    // An older host has no local-only fallback or command admission.
    box=state.local();box.ok=true;box.sessions[0].sharedNotifications=false;box.sessions[0].notificationPreferences={};box.sessions[0].muted=false;
    state.setLocal(box,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);open();
    mute=menu->findChild<QAction *>("contextToggleMute");QVERIFY(!mute->isEnabled());QVERIFY(mute->toolTip().contains("Update hgs"));menu->hide();
    QCOMPARE(calls(),4);QCOMPARE(marks.size(),0);QCOMPARE(QSettings().value("attention/sessionMarks"),originalMarks);
    QVERIFY(state.takeNewReadMarks().isEmpty());
}

void TestSessionsWindow::unsentDraftBecomesARowStatus()
{
    // A composer draft marks its row (the delegate turns it into the Draft status).
    auto state = fleet(); const auto box = state.local();
    SessionsWindow window(program()); window.setFleet(state); window.show(); window.showSession({}, box.sessions[0].name);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    auto *list = window.findChild<SessionList *>("sessionList");
    const auto draft = [&](const QString &name) {
        for (int i = 0; i < list->count(); ++i)
            if (list->item(i)->data(SessionRoles::Key).toString().endsWith('\n' + name)) return list->item(i)->data(SessionRoles::Draft).toBool();
        return false;
    };
    setComposerText(composer, "Unsent question");
    QVERIFY(draft(box.sessions[0].name));
    window.showSession({}, box.sessions[1].name);
    window.setFleet(state);   // rows are refreshed in place; the draft stays marked
    QVERIFY(draft(box.sessions[0].name)); QVERIFY(!draft(box.sessions[1].name));
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({"@local"});   // other rows go: the list is rebuilt from scratch
    QVERIFY(draft(box.sessions[0].name));
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({});
    window.showSession({}, box.sessions[0].name); composer->editor()->clear();
    QVERIFY(!draft(box.sessions[0].name));
}

void TestSessionsWindow::filtersSelectionAndOffline()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    QCOMPARE(list->count(), 5); list->setCurrentRow(2);
    const auto key = list->currentItem()->data(Qt::UserRole);
    window.setFleet(fleet()); QCOMPARE(list->currentItem()->data(Qt::UserRole), key);
    auto *search = window.findChild<QLineEdit *>("search"); search->setText("navigation"); auto *results = window.findChild<QListWidget *>("searchResults"); QCOMPARE(results->count(), 1);
    search->setText("nothing matches"); QCOMPARE(results->count(), 0); search->clear();
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "attention") button->click();
    QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(Qt::UserRole).toString().startsWith("mac\n"));
    auto state = fleet(); BoxState offline; offline.host = "mac"; offline.ok = false;
    state.setPeer(offline, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state); QCOMPARE(list->count(), 0);
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "all") button->click();
    QCOMPARE(list->count(), 5); list->setCurrentRow(4);
    QVERIFY(!window.findChild<QAction *>("pauseAction")->isEnabled());
    QSignalSpy opened(&window, &SessionsWindow::sessionActivated);
    QTest::keyClick(list, Qt::Key_Return); QCOMPARE(opened.size(), 0);
    window.close(); // Closing a dashboard never invokes a session command.
}

void TestSessionsWindow::refreshedListingsKeepRowsUnlessTimeChangesThem()
{
    // Listings with digests, as polls and event streams deliver them (FleetState::refreshListing).
    const auto now = QDateTime::currentMSecsSinceEpoch();
    auto local = fleet().local(); local.digest = "arch-1";
    auto mac = *fleet().peer("mac"); mac.digest = "mac-1";
    // The peer's poll grows stale this long after the fixture's clock: room for a
    // slow runner to create and show the window before the checks below.
    constexpr qint64 StaleInMs = 4000;
    FleetState state; state.setLocal(local, now); state.setPeer(mac, now - FleetState::kPeerStaleMs + StaleInMs);
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.resize(1120, 680); window.setFleet(state); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    const auto row = [list](const QString &name) -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().endsWith('\n' + name)) return list->item(i);
        return nullptr;
    };
    QVERIFY(row("kimi/docs/research")); QCOMPARE(row("claude/infra/review")->data(SessionRoles::Status).toString(), QString("Needs approval"));
    // A rebuild prepares every row from the fleet again; this one stands in for an unchanged row.
    row("kimi/docs/research")->setData(SessionRoles::Detail, "kept");
    const auto rendered = window.rowGenerationForTests();
    auto later = local; later.metrics["cpu_percent"] = 42.0; later.sessions[0].backgroundObservedAt = 7;
    QVERIFY(state.refreshListing({}, later, now + 100)); window.refreshFleet(state);
    QCOMPARE(row("kimi/docs/research")->data(SessionRoles::Detail).toString(), QString("kept"));
    QVERIFY(state.refreshListing("mac", mac, now - FleetState::kPeerStaleMs + StaleInMs)); window.refreshFleet(state);
    QCOMPARE(row("kimi/docs/research")->data(SessionRoles::Detail).toString(), QString("kept"));
    // Unless the peer's poll grew stale meanwhile (on a very slow runner),
    // which shows it offline and rightly prepares the rows again.
    if (!state.isStale(QDateTime::currentMSecsSinceEpoch())) QCOMPARE(window.rowGenerationForTests(), rendered);
    // A changed listing renders at once.
    auto changed = later; changed.digest = "arch-2"; changed.sessions[1].prompt = "Another prompt";
    QVERIFY(!state.refreshListing({}, changed, now + 300)); state.setLocal(changed, now + 300); window.setFleet(state);
    QVERIFY(row("kimi/docs/research")->data(SessionRoles::Detail).toString() != "kept");
    // Rows whose text follows the clock render again: here a retry countdown.
    auto retrying = changed; retrying.digest = "arch-3";
    retrying.sessions[1].recovery = {{"state", "waiting"}, {"due_at", QDateTime::currentMSecsSinceEpoch() / 1000.0 + 30}};
    state.setLocal(retrying, now + 400); window.setFleet(state);
    QVERIFY(row("kimi/docs/research")->data(SessionRoles::Status).toString().startsWith("Retry in "));
    // An applied row whose text is unchanged keeps its item as it is (its digest
    // matches), so a refresh is seen as a new preparation of the rows.
    const auto counting = window.rowGenerationForTests();
    QVERIFY(state.refreshListing({}, retrying, now + 500)); window.refreshFleet(state);
    QVERIFY(window.rowGenerationForTests() > counting);
    QVERIFY(row("kimi/docs/research")->data(SessionRoles::Status).toString().startsWith("Retry in "));
    // A peer whose last poll grew stale is shown offline, even while the rest refreshes.
    retrying.sessions[1].recovery = {}; retrying.digest = "arch-4"; state.setLocal(retrying, now + 600); window.setFleet(state);
    QCOMPARE(row("claude/infra/review")->data(SessionRoles::Status).toString(), QString("Needs approval"));
    QTRY_VERIFY_WITH_TIMEOUT(state.isStale(QDateTime::currentMSecsSinceEpoch()), StaleInMs + 4000);
    QVERIFY(state.refreshListing({}, retrying, QDateTime::currentMSecsSinceEpoch())); window.refreshFleet(state);
    QCOMPARE(row("claude/infra/review")->data(SessionRoles::Status).toString(), QString("Offline"));
}

void TestSessionsWindow::organizationIsSavedOnlyWhenItChanges()
{
    removeSetting("workspace/organization");
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(fleet()); window.show();
    QTRY_VERIFY(SettingsJson::contains("workspace/organization"));
    // Polls with the same rows leave the saved layout alone, whatever else wrote it.
    setJsonSetting("workspace/organization", QByteArray("{}"));
    for (int i = 0; i < 3; ++i) window.setFleet(fleet());
    QCOMPARE(jsonSetting("workspace/organization"), QByteArray("{}"));
    // A newly observed session changes the layout, which is saved at once.
    auto state = fleet(); auto local = state.local(); auto added = local.sessions[1]; added.name = "kimi/docs/second"; added.tag = "second";
    local.sessions << added; state.setLocal(local, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    const auto saved = jsonSetting("workspace/organization");
    QVERIFY(saved.contains("kimi/docs/second"));
}

void TestSessionsWindow::listFiltersKeepTheOpenConversation_data()
{
    QTest::addColumn<QString>("scope");
    for (const auto *scope : {"attention", "active", "paused", "archived", "machine", "project", "empty"})
        QTest::newRow(scope) << QString::fromLatin1(scope);
}

void TestSessionsWindow::listFiltersKeepTheOpenConversation()
{
    QFETCH(QString, scope);
    auto state = fleet(); auto local = state.local();
    auto archive = local.sessions[0]; archive.state = "archived"; archive.archiveId = "archive-one";
    archive.archivedAt = QDateTime::currentSecsSinceEpoch(); local.sessions.append(archive);
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    const auto current = local.sessions[1]; const QString key = '\n' + current.name;
    SessionOrganization org; const auto otherProject = org.createGroup("Other project");
    org.moveSession(org.observe("mac", "claude/infra/review", ""), otherProject);
    setJsonSetting("workspace/organization", QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(state); keepDetailsPollsOut(window);
    window.resize(1100,760); window.show(); window.showSession({}, current.name);
    auto *list = window.findChild<SessionList *>("sessionList");
    auto *composer = window.findChild<MessageComposer *>("messageComposer");
    auto *title = window.findChild<QLabel *>("detailTitle"); const auto openedTitle = title->text();
    const QString draft = scope == "active" ? QString() : QStringLiteral("Keep the open session and this draft");
    setComposerText(composer, draft);
    auto cursor = composer->editor()->textCursor(); cursor.setPosition(qMin(5, int(draft.size())));
    cursor.setPosition(qMin(12, int(draft.size())), QTextCursor::KeepAnchor); composer->editor()->setTextCursor(cursor);
    const auto selection = cursor.selectedText();
    auto *machines = window.findChild<MachineFilter *>("machineFilterChips"); auto *projects = window.findChild<ProjectFilter *>("projectFilterChips");
    QHash<QString, QPushButton *> filters;
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) filters[button->property("filter").toString()] = button;
    const auto applyScope = [&] {
        if (scope == "machine") machines->setSelection({"mac"});
        else if (scope == "project") projects->setSelection({otherProject});
        else if (scope == "empty") projects->setSelection({"unavailable-project"});
        else filters[scope]->click();
    };
    const auto clearScope = [&] { filters["all"]->click(); machines->setSelection({}); projects->setSelection({}); };
    applyScope();
    for (int i = 0; i < list->count(); ++i) QVERIFY(list->item(i)->data(SessionRoles::Key).toString() != key);
    QVERIFY(!list->currentItem());
    QCOMPARE(composer->sessionKey(), key); QCOMPARE(title->text(), openedTitle);
    QCOMPARE(composer->editor()->toPlainText(), draft); QCOMPARE(composer->editor()->textCursor().selectedText(), selection);
    // Regular fleet refreshes must not replace a conversation outside the list scope.
    window.setFleet(state);
    QCOMPARE(composer->sessionKey(), key); QCOMPARE(title->text(), openedTitle);
    QCOMPARE(composer->editor()->toPlainText(), draft); QCOMPARE(composer->editor()->textCursor().selectedText(), selection);
    clearScope();
    QVERIFY(list->currentItem()); QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), key);
    QVERIFY(list->selectedItems().contains(list->currentItem()));
    QCOMPARE(composer->sessionKey(), key); QCOMPARE(composer->editor()->toPlainText(), draft);
    // An explicit row click still opens that target; the original draft remains recoverable.
    if (scope != "empty") applyScope();
    QListWidgetItem *next = nullptr;
    for (int i = 0; i < list->count(); ++i) {
        const auto candidate = list->item(i)->data(SessionRoles::Key).toString();
        if (!list->item(i)->isHidden() && !list->item(i)->data(SessionRoles::Header).toBool()
            && !candidate.isEmpty() && candidate != key) { next = list->item(i); break; }
    }
    QVERIFY(next); const auto nextKey = next->data(SessionRoles::Key).toString(); list->scrollToItem(next);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, {}, list->visualItemRect(next).center());
    QCOMPARE(composer->sessionKey(), nextKey);
    window.showSession({}, current.name);
    QTRY_COMPARE(composer->editor()->toPlainText(), draft); QCOMPARE(composer->sessionKey(), key);
}

void TestSessionsWindow::activeFilterCombinesWorkAndAttentionOnce()
{
    auto state = fleet(); auto box = state.local();
    auto &working = box.sessions[0];
    auto &idle = box.sessions[1]; idle.conversationId = "idle-conversation"; idle.replyId = "idle-reply";
    auto overlap = working; overlap.name = "codex/hgs/overlap";
    overlap.conversationId = "overlap-conversation"; overlap.replyId = "overlap-reply";
    auto child = idle; child.name = "codex/hgs/child-request"; child.replyId.clear();
    child.subagentSource = "hooks"; child.subagents = {{"child", QJsonObject{{"display_state", "input"}}}};
    auto preview = idle; preview.name = "codex/hgs/preview-request"; preview.replyId.clear();
    preview.subagentSource = "hooks"; preview.subagentPreviews = {QJsonObject{{"state", "input"}}};
    auto reminder = box.sessions[2]; reminder.name = "codex/hgs/reminder"; reminder.reviewLater = true;
    auto exited = working; exited.name = "codex/hgs/exited"; exited.processState = "exited";
    auto ended = working; ended.name = "codex/hgs/ended"; ended.conversationState = "ended";
    auto unknown = working; unknown.name = "codex/hgs/unknown"; unknown.activity = "unknown";
    auto archived = overlap; archived.name = "codex/hgs/archive"; archived.state = "archived"; archived.archiveId = "archive-one";
    box.sessions.append({overlap, child, preview, reminder, exited, ended, unknown, archived});
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QVERIFY(state.setReviewLater({}, reminder, true));
    const auto previousMarks = QSettings().value("attention/sessionMarks");
    const auto restoreMarks = qScopeGuard([previousMarks] { setJsonSetting("attention/sessionMarks", previousMarks); });
    setJsonSetting("attention/sessionMarks", QJsonDocument(state.attentionMarks()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program()); window.setFleet(state); window.show();
    QPushButton *active = nullptr, *attention = nullptr;
    for (auto *button : window.findChildren<QPushButton *>()) {
        if (button->property("filter") == "active") active = button;
        if (button->property("filter") == "attention") attention = button;
        QVERIFY(button->property("filter") != "working");
    }
    QVERIFY(active); QVERIFY(attention); QCOMPARE(active->accessibleName(), QString("Active"));
    // The working session's intermediate reply remains Active without adding attention.
    QCOMPARE(active->property("count").toInt(), 7); QCOMPARE(attention->property("count").toInt(), 5);
    auto *list = window.findChild<QListWidget *>("sessionList"); active->click(); QCOMPARE(list->count(), 7);
    QSet<QString> keys;
    for (int i = 0; i < list->count(); ++i) keys.insert(list->item(i)->data(Qt::UserRole).toString());
    QCOMPARE(keys.size(), 7); QVERIFY(keys.contains("\ncodex/hgs/dashboard"));
    QVERIFY(keys.contains("\nkimi/docs/research")); QVERIFY(keys.contains("mac\nclaude/infra/review"));
    QVERIFY(keys.contains("\ncodex/hgs/child-request")); QVERIFY(keys.contains("\ncodex/hgs/reminder"));
    QVERIFY(keys.contains("\ncodex/hgs/preview-request"));
    box.ok = false; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    auto peer = *state.peer("mac"); peer.ok = false; state.setPeer(peer, QDateTime::currentMSecsSinceEpoch());
    window.setFleet(state); QCOMPARE(active->property("count").toInt(), 1); QCOMPARE(list->count(), 1);
    QCOMPARE(list->item(0)->data(Qt::UserRole).toString(), QString("\ncodex/hgs/reminder"));
}

void TestSessionsWindow::activeFilterDraftsRestoreOfflineAndFollowEditsAndDelivery()
{
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) { session.phase = "idle"; session.activity = "idle"; }
    auto parent = box.sessions[0]; parent.name = "codex/hgs/child-parent"; parent.conversationId = "conversation-one";
    parent.subagentSource = "hooks"; parent.subagents = {{"child", QJsonObject{{"state", "idle"}}}};
    auto staleParent = parent; staleParent.name = "codex/hgs/stale-parent"; staleParent.conversationId = "new-conversation";
    auto archived = box.sessions[0]; archived.name = "codex/hgs/archived-draft"; archived.state = "archived"; archived.archiveId = "archive-one";
    box.sessions.append({parent, staleParent, archived}); state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    auto peer = *state.peer("mac"); peer.sessions[0].phase = "idle"; peer.sessions[0].activity = "idle";
    state.setPeer(peer, QDateTime::currentMSecsSinceEpoch()); peer.ok = false; state.setPeer(peer, QDateTime::currentMSecsSinceEpoch());
    ComposerDraftStore store; ComposerDraft text; text.text = "Restored draft";
    QVERIFY(store.save("\n" + box.sessions[2].name, text));
    QVERIFY(store.save("arch\n" + parent.name + "\nconversation-one\nchild", text));
    QVERIFY(store.save("arch\n" + staleParent.name + "\nconversation-one\nchild", text));
    QVERIFY(store.save("\n" + archived.name + "\narchive-one", text));
    ComposerDraft attachment; attachment.attachments = {{"notes.txt", "text/plain", "snapshot", {}}};
    QVERIFY(store.save("mac\n" + peer.sessions[0].name, attachment));
    SessionsWindow window(program()); window.setFleet(state); window.show();
    QPushButton *active = nullptr; QSet<QString> filterIds;
    for (auto *button : window.findChildren<QPushButton *>()) {
        const auto id = button->property("filter").toString();
        if (!id.isEmpty()) filterIds.insert(id);
        if (id == "active") active = button;
    }
    QCOMPARE(filterIds, QSet<QString>({"all", "attention", "active", "paused", "archived"}));
    QVERIFY(active); QTRY_COMPARE(active->property("count").toInt(), 3);
    auto *list = window.findChild<SessionList *>("sessionList"); active->click();
    QSet<QString> keys;
    for (int i = 0; i < list->count(); ++i)
        if (list->item(i)->data(SessionRoles::ChildId).toString().isEmpty()) keys.insert(list->item(i)->data(SessionRoles::Key).toString());
    QCOMPARE(keys, QSet<QString>({"\n" + box.sessions[2].name, "\n" + parent.name, "mac\n" + peer.sessions[0].name}));
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({"@local"}); QCOMPARE(active->property("count").toInt(), 2);
    window.findChild<MachineFilter *>("machineFilterChips")->setSelection({}); QCOMPARE(active->property("count").toInt(), 3);
    window.showSession({}, staleParent.name); QTest::qWait(50);
    QCOMPARE(active->property("count").toInt(), 3); // An older inspection cannot bind a child draft to a new parent conversation.
    window.showSession({}, box.sessions[1].name);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); QVERIFY(composer);
    composer->editor()->setFocus(); setComposerText(composer, "New draft");
    QTRY_COMPARE(active->property("count").toInt(), 4);
    const auto key = composer->sessionKey(); QCOMPARE(key, "\n" + box.sessions[1].name);
    QVERIFY(composer->editor()->hasFocus());
    composer->editor()->clear(); QTRY_COMPARE(active->property("count").toInt(), 3);
    setComposerText(composer, "Send this draft"); QTRY_COMPARE(active->property("count").toInt(), 4);
    QVERIFY(composer->setSending(key)); QTRY_COMPARE(active->property("count").toInt(), 3);
    composer->deliveryFinished(key, false, "Synthetic delivery failure"); QTRY_COMPARE(active->property("count").toInt(), 4);
    QVERIFY(composer->setSending(key)); composer->deliveryFinished(key, true);
    QTRY_COMPARE(active->property("count").toInt(), 3);
}

void TestSessionsWindow::sessionContextMenuSignalsAndKeyboard()
{
    // A POSIX folder, also when this GUI runs on Windows.
    auto state = fleet(); auto box = state.local(); box.sessions[0].cwd = "/work/a folder"; box.metrics["platform"] = "linux";
    box.sessions[0].attached = 1; box.sessions[0].clients = {"/dev/pts/42"};
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.setClipboardMode(true); window.show();
    auto *list = window.findChild<QListWidget *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionContextMenu"); QVERIFY(menu);
    QSignalSpy terminal(&window, &SessionsWindow::terminalOpenRequested), normal(&window, &SessionsWindow::sessionActivated), shell(&window, &SessionsWindow::folderShellRequested);
    QSignalSpy commands(&window, &SessionsWindow::copySessionCommandRequested), texts(&window, &SessionsWindow::copyTextRequested);
    const auto showMouseMenu = [&](int row) {
        const QPoint point = list->visualItemRect(list->item(row)).center();
        QContextMenuEvent event(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &event);
    };
    list->setCurrentRow(1);
    const auto targetPoint = list->visualItemRect(list->item(0)).center();
    QTest::mousePress(list->viewport(), Qt::RightButton, {}, targetPoint);
    QTest::mouseRelease(list->viewport(), Qt::RightButton, {}, targetPoint);
    QCOMPARE(list->currentRow(), 1);
    showMouseMenu(0); QVERIFY(menu->isVisible()); QCOMPARE(list->currentRow(), 1);
    auto action = [&](const char *name) { return menu->findChild<QAction *>(name); };
    QVERIFY(!action("contextChangeSession")->isEnabled()); // Busy stays busy.
    action("contextCopyName")->trigger(); QCOMPARE(texts.size(), 1); QCOMPARE(texts[0][0].toString(), QString("codex/hgs/dashboard"));
    action("contextCopyFolder")->trigger(); QCOMPARE(texts.size(), 2); QCOMPARE(texts[1][0].toString(), QString("/work/a folder"));
    action("contextFolderShell")->trigger(); QCOMPARE(shell.size(),1); QCOMPARE(shell.last()[1].toString(),QString("/work/a folder"));
    QCOMPARE(list->currentRow(), 1); // Passive actions leave the conversation and draft in place.
    action("contextCopyCommand")->trigger(); QCOMPARE(commands.size(), 1); QCOMPARE(commands[0][0].toString(), QString());
    QCOMPARE(commands[0][1].toString(), QString("codex/hgs/dashboard")); QVERIFY(commands[0][2].toString().isEmpty());
    action("contextOpenTerminal")->trigger(); QCOMPARE(terminal.size(), 1); QCOMPARE(normal.size(), 0);
    QCOMPARE(terminal[0][2].toString(), QString("/dev/pts/42")); // Explicit open ignores clipboard default.
    menu->hide(); list->setFocus(); QTest::keyClick(list, Qt::Key_F10, Qt::ShiftModifier); QVERIFY(menu->isVisible());
    menu->hide(); QTest::keyClick(list, Qt::Key_Menu); QVERIFY(menu->isVisible()); menu->hide();
    const QPoint emptyPoint(5, list->viewport()->height() - 1);
    QContextMenuEvent empty(QContextMenuEvent::Mouse, emptyPoint, list->viewport()->mapToGlobal(emptyPoint));
    QApplication::sendEvent(list->viewport(), &empty); QVERIFY(!menu->isVisible());
    showMouseMenu(4); action("contextOpenTerminal")->trigger(); QCOMPARE(terminal.size(), 2);
    QCOMPARE(terminal[1][0].toString(), QString("mac")); QVERIFY(terminal[1][2].toString().isEmpty()); menu->hide();
    BoxState offline; offline.host = "mac"; offline.ok = false; state.setPeer(offline, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    showMouseMenu(4); QVERIFY(!action("contextOpenTerminal")->isEnabled()); QVERIFY(!action("contextRenameSession")->isEnabled());
    QVERIFY(!action("contextTerminateSession")->isEnabled()); QVERIFY(action("contextCopyCommand")->isEnabled());
    action("contextCopyCommand")->trigger(); QCOMPARE(commands.size(), 2); QCOMPARE(commands[1][0].toString(), QString("mac"));
    QVERIFY(!action("contextCopyFolder")->isEnabled()); // Missing path is not guessed from a project label.
}

void TestSessionsWindow::compactSessionRowActionsPreserveSelectionAndDraft()
{
    auto state = fleet(); auto box = state.local();
    box.sessions[0].conversationId = "conversation-one"; box.sessions[0].runId = "run-one";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setDetailsPollForTests(60000, 60000); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionContextMenu");
    QVERIFY(list); QVERIFY(menu); list->setCurrentRow(1);
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Keep this other conversation draft");
    const QString selected = list->currentItem()->data(SessionRoles::Key).toString();
    auto *target = list->item(0); const QString targetKey = target->data(SessionRoles::Key).toString();
    const auto showMenu = [&] {
        const auto point = list->visualItemRect(target).center();
        QContextMenuEvent context(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point));
        QApplication::sendEvent(list->viewport(), &context);
    };
    QSignalSpy writes(window.findChild<HgsClient *>(), &HgsClient::writeDone);
    QSignalSpy reads(&window, &SessionsWindow::replyViewed), sends(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    QSignalSpy activated(&window, &SessionsWindow::sessionActivated), copied(&window, &SessionsWindow::copyTextRequested);
    const int normalHeight = list->visualItemRect(target).height(); showMenu(); QVERIFY(menu->isVisible());
    auto *density = menu->findChild<QAction *>("contextSessionRowDensity"); QVERIFY(density); QVERIFY(density->isEnabled());
    QVERIFY(menu->actions().contains(window.findChild<QAction *>("expandAllCollapsedSessionRows")));
    QCOMPARE(density->text(), QString("Compact session row")); density->trigger(); menu->hide();
    QVERIFY(target->data(SessionRoles::RowCollapsed).toBool()); QCOMPARE(list->visualItemRect(target).height(), 40); QVERIFY(normalHeight > 40);
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selected); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this other conversation draft"));
    showMenu(); density = menu->findChild<QAction *>("contextSessionRowDensity"); QCOMPARE(density->text(), QString("Expand session row"));
    menu->findChild<QAction *>("contextCopyName")->trigger(); QCOMPARE(copied.size(), 1); QCOMPARE(copied.last()[0].toString(), box.sessions[0].name);
    QVERIFY(menu->findChild<QAction *>("contextOpenTerminal")); QVERIFY(menu->findChild<QAction *>("contextRenameSession")); menu->hide();
    list->syncRowExpandButtons(); QToolButton *expand = nullptr;
    for (auto *button : list->findChildren<QToolButton *>("expandSessionRow")) if (button->property("sessionKey") == targetKey) expand = button;
    QVERIFY(expand); QVERIFY(expand->accessibleName().contains(target->data(SessionRoles::Title).toString())); QTest::mouseClick(expand, Qt::LeftButton);
    QVERIFY(!target->data(SessionRoles::RowCollapsed).toBool()); QCOMPARE(list->visualItemRect(target).height(), normalHeight);
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selected); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this other conversation draft"));
    QCOMPARE(writes.size(), 0); QCOMPARE(sends.size(), 0); QCOMPARE(reads.size(), 0); QCOMPARE(activated.size(), 0);
    QVERIFY(QSettings().value("workspace/collapsedSessionRows").toByteArray().isEmpty());
    // Double-click is the compact/expand gesture on the open conversation.
    // It persists density without a native action or a lost composer draft.
    for (bool collapsed : {true, false}) {
        const auto point = list->visualItemRect(list->currentItem()).center();
        QTest::mouseDClick(list->viewport(), Qt::LeftButton, {}, point);
        QTest::mouseRelease(list->viewport(), Qt::LeftButton, {}, point);
        QCOMPARE(list->currentItem()->data(SessionRoles::RowCollapsed).toBool(), collapsed);
        QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), selected);
        QCOMPARE(composer->editor()->toPlainText(), QString("Keep this other conversation draft"));
        QCOMPARE(QSettings().value("workspace/collapsedSessionRows").toByteArray().isEmpty(), !collapsed);
        QCOMPARE(writes.size(), 0); QCOMPARE(sends.size(), 0); QCOMPARE(reads.size(), 0); QCOMPARE(activated.size(), 0);
    }
}

void TestSessionsWindow::selectedSessionRowsCompactAndExpandLocally()
{
    auto state = fleet(); auto box = state.local();
    for (int i = 0; i < box.sessions.size(); ++i) box.sessions[i].conversationId = QString("density-%1").arg(i);
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    auto peer = *state.peer("mac"); peer.ok = false; state.setPeer(peer, QDateTime::currentMSecsSinceEpoch());
    SessionRowPreferences preferences; QVERIFY(preferences.setCollapsed({}, box.sessions[0], true));
    QSettings().setValue("workspace/collapsedSessionRows", preferences.save());
    SessionsWindow window(program()); window.setDetailsPollForTests(60000, 60000); window.setFleet(state); window.show();
    window.showSession({}, box.sessions[0].name);
    auto *list = window.findChild<SessionList *>("sessionList"); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QVERIFY(list); QVERIFY(composer); setComposerText(composer, "Keep the open conversation draft");
    const auto current = list->currentItem()->data(SessionRoles::Key).toString();
    const auto find = [&](const QString &name) -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().section('\n', 1, 1) == name) return list->item(i);
        return nullptr;
    };
    const auto toggle = [&](const QString &name, Qt::KeyboardModifiers modifiers) {
        auto *item = find(name); if (!item) return false;
        list->scrollToItem(item); QTest::mouseClick(list->viewport(), Qt::LeftButton, modifiers, list->visualItemRect(item).center()); return true;
    };
    QVERIFY(toggle(box.sessions[1].name, Qt::ControlModifier)); QVERIFY(toggle(peer.sessions[0].name, Qt::MetaModifier));
    QCOMPARE(list->selectedItems().size(), 3);
    auto *compact = window.findChild<QAction *>("selectioncompact-row"); auto *expand = window.findChild<QAction *>("selectionexpand-row");
    QVERIFY(compact); QVERIFY(expand); QVERIFY(compact->isVisible()); QVERIFY(expand->isVisible());
    QCOMPARE(compact->text(), QString("Compact session rows (2)")); QCOMPARE(expand->text(), QString("Expand session rows (1)"));
    QVERIFY(compact->isEnabled()); QVERIFY(expand->isEnabled()); // Offline rows are local presentation too.
    auto *bar = window.findChild<QFrame *>("sessionSelectionBar"); QVERIFY(bar); QVERIFY(bar->isVisible());
    auto *tools = window.findChild<QToolBar *>("selectionTools"); QVERIFY(tools);
    auto *button = qobject_cast<QToolButton *>(tools->widgetForAction(compact)); QVERIFY(button);
    QVERIFY(button->accessibleName().contains("2 of 3 selected sessions")); QVERIFY(!compact->icon().isNull());
    QSignalSpy writes(window.findChild<HgsClient *>(), &HgsClient::writeDone), sends(window.findChild<HgsClient *>(), &HgsClient::messageSent);
    QSignalSpy activated(&window, &SessionsWindow::sessionActivated), reads(&window, &SessionsWindow::replyViewed);
    QSet<QString> selected; for (const auto *item : list->selectedItems()) selected.insert(item->data(SessionRoles::Key).toString());
    compact->trigger();
    for (const auto *item : list->selectedItems()) QVERIFY(item->data(SessionRoles::RowCollapsed).toBool());
    QVERIFY(!compact->isVisible()); QCOMPARE(expand->text(), QString("Expand session rows (3)"));
    QVERIFY(!find(box.sessions[2].name)->data(SessionRoles::RowCollapsed).toBool());
    expand->trigger();
    for (const auto *item : list->selectedItems()) QVERIFY(!item->data(SessionRoles::RowCollapsed).toBool());
    QVERIFY(!expand->isVisible()); QCOMPARE(compact->text(), QString("Compact session rows (3)"));
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), current);
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep the open conversation draft"));
    QSet<QString> after; for (const auto *item : list->selectedItems()) after.insert(item->data(SessionRoles::Key).toString()); QCOMPARE(after, selected);
    QVERIFY(QSettings().value("workspace/collapsedSessionRows").toByteArray().isEmpty());
    QCOMPARE(writes.size(), 0); QCOMPARE(sends.size(), 0); QCOMPARE(activated.size(), 0); QCOMPARE(reads.size(), 0);
    window.setFleet(state);
    after.clear(); for (const auto *item : list->selectedItems()) after.insert(item->data(SessionRoles::Key).toString()); QCOMPARE(after, selected);
}

void TestSessionsWindow::shiftRangeSurvivesFilteringReorderAndDeletedAnchor()
{
    auto state = fleet(); auto box = state.local(); box.sessions[1].state = "paused";
    box.sessions[1].resumable = true; box.sessions[1].activity = "idle"; box.sessions[1].phase = "idle";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setDetailsPollForTests(60000, 60000); window.setFleet(state); window.show();
    window.showSession({}, box.sessions[0].name);
    auto *list = window.findChild<SessionList *>("sessionList"); auto *composer = window.findChild<MessageComposer *>("messageComposer");
    QVERIFY(list); QVERIFY(composer); setComposerText(composer, "Keep this draft through list filtering");
    const auto open = list->currentItem()->data(SessionRoles::Key).toString();
    const auto find = [&](const QString &name) -> QListWidgetItem * {
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().section('\n', 1, 1) == name) return list->item(i);
        return nullptr;
    };
    const auto click = [&](const QString &name, Qt::KeyboardModifiers modifiers) {
        auto *item = find(name); if (!item) return false;
        list->scrollToItem(item); QTest::mouseClick(list->viewport(), Qt::LeftButton, modifiers, list->visualItemRect(item).center()); return true;
    };
    const auto names = [&] { QSet<QString> result; for (const auto *item : list->selectedItems()) result.insert(item->data(SessionRoles::Key).toString().section('\n', 1, 1)); return result; };
    auto *anchor = find(box.sessions[1].name); auto *tip = find(box.sessions[2].name); QVERIFY(anchor); QVERIFY(tip);
    // Saved sessions preserve insertion order in the configured session list.
    QVERIFY(click(box.sessions[1].name, Qt::ControlModifier));
    QPushButton *saved = nullptr, *all = nullptr;
    for (auto *button : window.findChildren<QPushButton *>("sessionFilter")) {
        if (button->property("filter") == "paused") saved = button;
        if (button->property("filter") == "all") all = button;
    }
    QVERIFY(saved); QVERIFY(all); saved->click();
    QVERIFY(!list->currentItem()); QVERIFY(list->bulkSelecting());
    QCOMPARE(names(), (QSet<QString>{box.sessions[1].name}));
    QVERIFY(click(box.sessions[2].name, Qt::ShiftModifier));
    QCOMPARE(names(), (QSet<QString>{box.sessions[1].name, box.sessions[2].name})); QVERIFY(!list->currentItem());
    QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft through list filtering"));
    std::swap(box.sessions[1], box.sessions[2]); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(names(), (QSet<QString>{box.sessions[1].name, box.sessions[2].name}));
    QVERIFY(click(box.sessions[1].name, Qt::ShiftModifier));
    QCOMPARE(names(), (QSet<QString>{box.sessions[1].name, box.sessions[2].name}));
    // Remove the anchor while the open conversation is still filtered out.
    box.sessions.removeAt(2); state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(click(box.sessions[1].name, Qt::ShiftModifier)); QCOMPARE(names(), (QSet<QString>{box.sessions[1].name}));
    QVERIFY(!list->currentItem()); QCOMPARE(composer->editor()->toPlainText(), QString("Keep this draft through list filtering"));
    all->click(); QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), open);
    QCOMPARE(names(), (QSet<QString>{box.sessions[1].name}));
}

void TestSessionsWindow::compactSessionRowsPersistAndExpandAllIncludesHidden()
{
    auto state = fleet(); auto box = state.local(); box.sessions = box.sessions.mid(0, 3);
    for (int i = 0; i < box.sessions.size(); ++i) box.sessions[i].conversationId = "row-conversation-" + QString::number(i);
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    // Save real menu edits first. The paused row and a running row then live in
    // separate state filters, and the saved preference survives a fresh window.
    {
        SessionsWindow window(program()); window.setDetailsPollForTests(60000, 60000); window.setFleet(state); window.show();
        auto *list = window.findChild<SessionList *>("sessionList"); auto *menu = window.findChild<QMenu *>("sessionContextMenu");
        for (int row : {0, 1}) {
            const auto point = list->visualItemRect(list->item(row)).center();
            QContextMenuEvent context(QContextMenuEvent::Mouse, point, list->viewport()->mapToGlobal(point)); QApplication::sendEvent(list->viewport(), &context);
            auto *action = menu->findChild<QAction *>("contextSessionRowDensity"); QVERIFY(action); action->trigger(); menu->hide();
        }
        QVERIFY(!QSettings().value("workspace/collapsedSessionRows").toByteArray().isEmpty());
    }
    SessionsWindow restored(program()); restored.setDetailsPollForTests(60000, 60000); restored.setFleet(state); restored.show();
    auto *list = restored.findChild<SessionList *>("sessionList");
    QVERIFY(list->item(0)->data(SessionRoles::RowCollapsed).toBool()); QVERIFY(list->item(1)->data(SessionRoles::RowCollapsed).toBool());
    auto *expandAll = restored.findChild<QAction *>("expandAllCollapsedSessionRows"); QVERIFY(expandAll); QVERIFY(expandAll->isEnabled());
    QVERIFY(restored.findChild<QMenu *>("sessionProjectsMenu")->actions().contains(expandAll));
    QVERIFY(restored.findChild<QMenu *>("sessionsNavMenu")->actions().contains(expandAll));
    auto *batch = restored.findChild<QPushButton *>("batchActions"); QVERIFY(batch); QMetaObject::invokeMethod(batch->menu(), "aboutToShow"); QVERIFY(batch->menu()->actions().contains(expandAll));
    QPushButton *saved = nullptr, *all = nullptr;
    for (auto *filter : restored.findChildren<QPushButton *>("sessionFilter")) {
        if (filter->property("filter") == "paused") saved = filter;
        if (filter->property("filter") == "all") all = filter;
    }
    QVERIFY(saved); QVERIFY(all); saved->click(); QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(SessionRoles::RowCollapsed).toBool());
    QSignalSpy writes(restored.findChild<HgsClient *>(), &HgsClient::writeDone);
    expandAll->trigger(); QVERIFY(!list->item(0)->data(SessionRoles::RowCollapsed).toBool()); QVERIFY(!expandAll->isEnabled());
    QVERIFY(QSettings().value("workspace/collapsedSessionRows").toByteArray().isEmpty()); QCOMPARE(writes.size(), 0);
    all->click(); for (int i = 0; i < list->count(); ++i) QVERIFY(!list->item(i)->data(SessionRoles::RowCollapsed).toBool());
    SessionsWindow reopened(program()); reopened.setFleet(state); reopened.show();
    auto *again = reopened.findChild<SessionList *>("sessionList"); QCOMPARE(again->count(), list->count());
    for (int i = 0; i < again->count(); ++i) QVERIFY(!again->item(i)->data(SessionRoles::RowCollapsed).toBool());
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_list3.moc"
