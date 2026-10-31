// U62 (#712): The inspector: its cards, tasks, processes and subagents, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void sessionServicesAreNotBackgroundWork();
    void compactMetadataAndSubagentRoster();
    void tasksCardShowsBackgroundJobsAndFoldsFinishedChildren();
    void nativeSessionUsageStaysWithSelectedConversation();
    void subagentActivityKeepsMainDraftAndRejectsStaleHistory();
    void childTreeKeepsRoutineResultsQuiet();
    void completedSubagentsPreferenceKeepsOpenHistory();
    void inactiveSubagentsStayInInspectorAndOutOfSessionCounts();
    void subagentGroupsAndUnavailableActivity();
    void inspectorPreservesWorkspaceAndReportsTasks();
    void inspectorWidthSurvivesResizeHideAndRestartAndResetsOnDoubleClick();
    void detailsPollingPreservesReadingPosition();
    void nativeGoalIsIndependentOfTurnState();
    void tasksCardReadsBackgroundJobsWithTheProcessesTabOff();
};

// G7: a session that runs MCP servers and services shows them as an icon with
// their names and start times, and its status never reads Background for them.
void TestSessionsWindow::sessionServicesAreNotBackgroundWork()
{
    QTemporaryDir temp; const auto program = temp.filePath("hgs");
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a[0].startswith('@'):a=a[1:]
if a[0]=='inspect':print((root/'details.json').read_text() if (root/'details.json').exists() else '{}')
else:print('{}')
)PY");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    { QFile details(temp.filePath("details.json")); QVERIFY(details.open(QIODevice::WriteOnly));
      details.write(QJsonDocument(QJsonObject{{"tracked", true}, {"run_id", "run-services"}, {"conversation_id", "conversation-services"},
          {"runtime_state", "live"}, {"process_state", "running"}, {"activity", "idle"}, {"phase", "idle"},
          {"process_work", QJsonObject{{"count", 0}, {"sampled_at", 1792868400.0},
              {"services", QJsonArray{QJsonObject{{"name", "qdrant"}, {"command", "qdrant --mcp"}, {"since", 1792868000.0}},
                                      QJsonObject{{"name", "socraticode"}, {"command", "socraticode --mcp"}, {"since", 1792868100.0}}}}}}}).toJson()); }
    auto state = fleet(); auto box = state.local();
    for (auto &session : box.sessions) if (session.name == "codex/hgs/dashboard") {
        session.state = "running"; session.runId = "run-services"; session.conversationId = "conversation-services";
        session.phase = "idle"; session.activity = "idle"; session.processState = "running";
        // G7: the listing carries the services (hgs ls -> process_work.services);
        // the row model reads them from here, not from the inspection.
        session.services = {SessionInfo::Service{QStringLiteral("qdrant"), QStringLiteral("qdrant --mcp"), 1792868000.0},
                            SessionInfo::Service{QStringLiteral("socraticode"), QStringLiteral("socraticode --mcp"), 1792868100.0}};
    }
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(program)); window.resize(1280, 760); window.setFleet(state); window.show();
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    const auto rowOf = [&](const QString &name) { for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(SessionRoles::Key).toString().endsWith(name)) return i; return -1; };
    const int row = rowOf("codex/hgs/dashboard");
    QTRY_VERIFY(row >= 0);
    window.showSession({}, "codex/hgs/dashboard");
    QTRY_VERIFY(!list->item(row)->data(SessionRoles::Services).toList().isEmpty());
    const auto services = list->item(row)->data(SessionRoles::Services).toList();
    QCOMPARE(services.size(), 2);
    QCOMPARE(services[0].toMap().value("name").toString(), QStringLiteral("qdrant"));
    // L4: the header tooltip keeps the metadata and the model and adds the
    // services (they never replace the rest of the tooltip).
    auto *meta = window.findChild<QLabel *>("sessionMeta");
    QVERIFY(meta);
    QTRY_VERIFY2(meta->text().contains(QStringLiteral("service")), qPrintable(meta->text()));
    QVERIFY2(meta->toolTip().contains(QStringLiteral("qdrant")), qPrintable(meta->toolTip()));
    QVERIFY2(meta->toolTip().contains(QStringLiteral("since")), qPrintable(meta->toolTip()));
    QVERIFY2(meta->toolTip().contains(QStringLiteral("Services")), qPrintable(meta->toolTip()));
    QVERIFY2(meta->toolTip().contains(QStringLiteral("codex")), qPrintable(meta->toolTip()));
    QVERIFY2(meta->toolTip().contains(QStringLiteral("gpt")), qPrintable(meta->toolTip()));
    // L3: the services chip never overlaps the metadata - it sits left of the
    // git badge with the text's room kept on its left.
    const auto rowRect = list->visualItemRect(list->item(row));
    const auto chip = SessionDelegate::servicesRect(rowRect, list->model()->index(row, 0), list->font(), false);
    QVERIFY(!chip.isEmpty());
    QVERIFY(chip.left() > rowRect.x() + 80);
    QVERIFY(chip.right() <= rowRect.right());
    if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty()) {
        QTest::qWait(200);
        window.grab().save(QDir(shots).filePath("session-services.png"));
    }
}

void TestSessionsWindow::compactMetadataAndSubagentRoster()
{
    // The inspector lists finished children here (owner, 2026-10-28: by default
    // they fold into a count with a link that opens them).
    QSettings().setValue("workspace/inspectorFinishedSubagents", true);
    const auto restoreFinished = qScopeGuard([] { QSettings().remove("workspace/inspectorFinishedSubagents"); });
    QSettings().setValue("workspace/showCompletedSubagents", true);
    auto state = fleet(); auto box = state.local(); auto &session = box.sessions[0];
    session.gitBranch = "feat/compact"; session.gitWorktree = true; session.gitWorktreeName = "ui-tree"; session.gitCommonDir = "/work/ui/.git";
    session.cwd = "/work/ui-tree"; session.gitRoot = session.cwd; session.activitySummary = "Read"; session.activityDetail = "view.cpp";
    session.subagentSource = "hooks"; session.subagentCountsComplete = true; session.subagentActiveCount = 2; session.subagentTotalCount = 14;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); QTest::qWait(60);
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list->sizeHintForRow(0) <= 96);
    QCOMPARE(list->item(0)->data(Qt::UserRole + 1).toString(), QString("dashboard"));
    QCOMPARE(list->item(0)->data(Qt::UserRole + 7).toString(), QString("2/14"));
    // A linked worktree names its main checkout folder, not its own generated folder.
    QCOMPARE(list->item(0)->data(SessionRoles::Meta).toString(), QString("ui / feat/compact"));
    QCOMPARE(list->item(0)->data(SessionRoles::MetaIcon).toString(), QString("git-branch"));
    const auto accessible = list->item(0)->data(Qt::AccessibleTextRole).toString();
    QVERIFY(accessible.contains("ui / feat/compact")); QVERIFY(!accessible.contains("worktree: ui-tree")); QVERIFY(accessible.contains("/work/ui-tree"));
    QVERIFY(accessible.contains("Read: view.cpp")); QVERIFY(accessible.contains("2 active / 14 total"));
    auto *header = window.findChild<QLabel *>("sessionMeta");
    QCOMPARE(header->accessibleName(), QString("ui / feat/compact · worktree"));
    QVERIFY(header->toolTip().contains("/work/ui-tree")); QVERIFY(!header->toolTip().contains("ui-tree / "));
    // U25 part 6: the inspector's Environment rows name the branch and folder.
    QString branchRow, folderRow;
    for (auto *row : window.findChildren<QLabel *>("inspectorEnvValue")) {
        if (row->property("row") == "branch") branchRow = row->text();
        if (row->property("row") == "folder") folderRow = row->text();
    }
    QCOMPARE(branchRow, QString("ui / feat/compact · worktree")); QVERIFY(folderRow.contains("/work/ui-tree")); QVERIFY(!folderRow.contains("worktree: ui-tree"));
    auto *search = window.findChild<QLineEdit *>("search"); search->setText("feat/compact"); QCOMPARE(window.findChild<QListWidget *>("searchResults")->count(), 1); search->clear();
    QJsonObject children;
    for (int i = 0; i < 14; ++i) children[QString("agent-%1").arg(i)] = QJsonObject{{"label", QString("Child %1").arg(i)}, {"state", i < 2 ? "working" : "finished"}, {"detail", "Review code"}};
    children["unknown"] = QJsonObject{{"label", "Uncertain"}, {"state", "working"}, {"display_state", "unknown"}};
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"subagent_source", "hooks"}, {"subagents", children},
        {"subagent_active_count", 2}, {"subagent_total_count", 15}, {"subagent_counts_complete", false}};
    auto *client = window.findChild<HgsClient *>(); client->inspectionReady({}, session.name, details);
    auto *roster = window.findChild<QTextBrowser *>("subagents"); const auto text = roster->toPlainText();
    QVERIFY(text.contains("Child 13")); QVERIFY(text.contains("Ready")); QVERIFY(text.contains("Uncertain")); QVERIFY(text.contains("Unknown"));
    QVERIFY(!text.contains("completed tasks"));
    QVERIFY(text.contains("some states unknown"));
    bool hasFullTooltip = false;
    for (auto *frame : roster->document()->rootFrame()->childFrames()) if (auto *table = qobject_cast<QTextTable *>(frame)) {
        auto cursor = table->cellAt(0, 0).firstCursorPosition(); cursor.movePosition(QTextCursor::NextCharacter);
        hasFullTooltip = cursor.charFormat().toolTip().contains("Review code");
    }
    QVERIFY(!hasFullTooltip);
    QVERIFY(roster->toHtml().contains("hgs-agent:"));
    // A cached busy hook on a stopped terminal must not imply current activity.
    box.sessions[0].processState = "exited"; state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(list->item(0)->data(Qt::UserRole + 7).toString().isEmpty());
    QCOMPARE(list->item(0)->data(Qt::UserRole + 4).toString(), QString("Agent exited"));
    for (auto *button : window.findChildren<QPushButton *>()) if (button->property("filter") == "active") QCOMPARE(button->property("count").toInt(), 1);
    client->inspectionReady({}, session.name, details); QVERIFY(roster->toPlainText().contains("Last recorded"));
    QVERIFY(!roster->toPlainText().contains("2 active /"));
}

// Owner, 2026-10-28: the Tasks card shows the agent's background commands
// (a build, a test run), folds the finished subagents into a count with a
// link that opens them, and never shows the latest prompt as the state.
void TestSessionsWindow::tasksCardShowsBackgroundJobsAndFoldsFinishedChildren()
{
    QSettings().remove("workspace/inspectorFinishedSubagents");
    auto state = fleet(); auto box = state.local(); auto &session = box.sessions[0];
    session.subagentSource = "hooks"; session.subagentCountsComplete = true; session.subagentActiveCount = 1; session.subagentTotalCount = 3;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); QTest::qWait(60);
    QJsonObject children{{"a", QJsonObject{{"label", "Working child"}, {"state", "working"}, {"detail", "Review code"}}},
                         {"b", QJsonObject{{"label", "Done child one"}, {"state", "finished"}}},
                         {"c", QJsonObject{{"label", "Done child two"}, {"state", "finished"}}}};
    const double now = QDateTime::currentSecsSinceEpoch();
    QJsonArray jobs{QJsonObject{{"background", true}, {"status", "running"}, {"native_id", "b1"}, {"started_at", now - 600}, {"description", "Run phase 41 checks"}},
                    QJsonObject{{"background", true}, {"status", "completed"}, {"native_id", "b0"}, {"started_at", now - 900}, {"description", "Old build"}},
                    QJsonObject{{"background", false}, {"status", "starting"}, {"command", "hgs inspect"}}};
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"subagent_source", "hooks"}, {"subagents", children},
        {"subagent_active_count", 1}, {"subagent_total_count", 3}, {"subagent_counts_complete", true},
        {"activity", "busy"}, {"activity_summary", "Bash"}, {"activity_detail", "cargo test"},
        {"last_prompt", "a long prompt that must not be the state"}, {"processes", QJsonObject{{"items", jobs}}}};
    auto *client = window.findChild<HgsClient *>(); client->inspectionReady({}, session.name, details);
    auto *roster = window.findChild<QTextBrowser *>("subagents");
    // Read at once, as the roster test does: the fixture's own inspection
    // replaces these details on its next poll.
    const auto text = roster->toPlainText();
    QVERIFY2(text.contains("Background jobs"), qPrintable(text.left(600)));
    QVERIFY(text.contains("Run phase 41 checks")); QVERIFY(text.contains("Running"));
    QVERIFY(!text.contains("Old build")); QVERIFY(!text.contains("hgs inspect"));
    QVERIFY(text.contains("Working child"));
    QVERIFY(!text.contains("Done child one")); QVERIFY(text.contains("2 finished"));
    QVERIFY(!text.contains("must not be the state"));
    QVERIFY(text.contains("Bash: cargo test"));
    QVERIFY2(text.contains("Main agent · "), qPrintable(text.left(300)));
    QVERIFY(text.contains("1 active / 3 total")); QVERIFY(!text.contains("SUBAGENTS"));
    // #648: a summary that is only the state word, with the prompt as its
    // detail, shows nothing more than the state.
    {auto prompt = details; prompt["activity_summary"] = "Working"; prompt["activity_detail"] = "a long prompt that must not be the state";
     client->inspectionReady({}, session.name, prompt);
     const auto shown = roster->toPlainText();
     QVERIFY(shown.contains("Main agent · ")); QVERIFY2(!shown.contains("must not be the state"), qPrintable(shown.left(300)));
     QVERIFY(!shown.contains("Working: "));}
    // "show" opens the finished children.
    emit roster->anchorClicked(QUrl("hgs-children:finished"));
    client->inspectionReady({}, session.name, details);
    QVERIFY(roster->toPlainText().contains("Done child one"));
}

void TestSessionsWindow::nativeSessionUsageStaysWithSelectedConversation()
{
    SessionsWindow window(program());window.setFleet(fleet());window.show();QTest::qWait(60);
    auto *client=window.findChild<HgsClient *>();auto *info=window.findChild<QTextBrowser *>("sessionInfo");
    const QJsonObject usage{{"status","ok"},{"totals",QJsonObject{{"total",12345},{"input",12000},{"cache_read",10000},{"output",345},{"reasoning",100}}},
        {"context",QJsonObject{{"used",1500},{"limit",10000}}}};
    client->inspectionReady({},"codex/hgs/dashboard",{{"conversation_id","conversation-one"},{"session_usage",usage}});
    const auto text=info->toPlainText().simplified();QVERIFY(text.contains("Session usage"));QVERIFY2(text.contains(QLocale().toString(12345).simplified()),qPrintable(text));
    QVERIFY(text.contains("Reasoning (in output)"));QVERIFY(window.findChild<QLabel *>("detailsContext")->text().contains("15"));QVERIFY(text.contains("Not reported"));
    auto *context=window.findChild<QPushButton *>("activityContext");QVERIFY(context->isVisible());
    QCOMPARE(context->property("contextPercent").toDouble(),15.);QVERIFY(context->toolTip().contains(QLocale().toString(1500)));
    const auto indicatorSize=context->size();context->click();QVERIFY(inspectorCardOpen(window,"context"));
    window.findChild<QPushButton *>("toggleInspector")->setChecked(false);

    const auto preview=qEnvironmentVariable("HGS_SESSION_USAGE_PREVIEW");
    if(!preview.isEmpty()) {
        window.resize(1260,920);window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
        QTest::qWait(40);QVERIFY(window.grab().save(preview));
    }
    window.showSession("mac","claude/infra/review");
    QVERIFY(!info->toPlainText().simplified().contains(QLocale().toString(12345).simplified()));
    client->inspectionReady("mac","claude/infra/review",{{"conversation_id","conversation-one"}});
    QVERIFY(info->toPlainText().contains("No native usage reported"));
    QVERIFY(!context->property("contextPercent").isValid());QCOMPARE(context->height(),indicatorSize.height());   // width follows the text
    QVERIFY(context->text().isEmpty());QVERIFY(!context->isEnabled());
    auto noCapacity=usage;noCapacity["context"]=QJsonObject{{"used",32000}};
    client->inspectionReady("mac","claude/infra/review",{{"conversation_id","conversation-one"},{"session_usage",noCapacity}});
    QVERIFY(context->text().contains("32"));QVERIFY(!context->property("contextPercent").isValid());
    QVERIFY(context->toolTip().contains("Percentage is unavailable"));
    QVERIFY(!context->text().contains('%')); QVERIFY(!context->text().contains('/'));
    QVERIFY(!context->text().contains(QChar(0x2014))); QVERIFY(!context->icon().isNull());
    QVERIFY(context->accessibleName().contains("capacity unknown")); QVERIFY(context->isEnabled());
    client->inspectionReady("mac","claude/infra/review",{{"conversation_id","conversation-one"},{"session_usage",usage}});
    QVERIFY(context->text().contains('%')); QVERIFY(context->icon().isNull());
}

void TestSessionsWindow::subagentActivityKeepsMainDraftAndRejectsStaleHistory()
{
    QSettings().setValue("workspace/showCompletedSubagents", true);
    QJsonObject details{{"tracked",true},{"conversation_id","conversation-one"},{"run_id","run-one"},
        {"subagents",QJsonObject{{"child-1",QJsonObject{{"name","Review"},{"state","finished"},{"detail",QString(10000,'x')}}}}}};
    details["session_usage"]=QJsonObject{{"context",QJsonObject{{"used",8000},{"limit",10000}}}};
    QTemporaryDir fixtureDir; const auto program=fixtureDir.filePath("hgs");
    QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
if 'inspect' in sys.argv: print((root/('child.json' if '--agent' in sys.argv else 'parent.json')).read_text())
else: print('{}')
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QJsonObject child{{"parent_conversation_id","conversation-one"},{"conversation_id","conversation-one/child-1"},{"run_id","run-one"},
        {"history_scope","Native subagent history"},{"events",QJsonArray{QJsonObject{{"type","AgentMessage"},{"detail","Review finished"},{"at",1700000000}}}}};
    child["session_usage"]=QJsonObject{{"context",QJsonObject{{"used",500},{"limit",2000}}}};
    for(const auto &snapshot: {qMakePair(QString("parent.json"),details),qMakePair(QString("child.json"),child)}) {
        QVERIFY(FakeCli::replace(fixtureDir.filePath(snapshot.first),QJsonDocument(snapshot.second).toJson()));
    }
    SessionsWindow window(FakeCli::program(program));window.setFleet(fleet());window.show();
    auto *client=window.findChild<HgsClient *>();QSignalSpy inspected(client,&HgsClient::inspectionReady);WAIT_VERIFY(!inspected.isEmpty());
    auto *composer=window.findChild<MessageComposer *>("messageComposer");auto *editor=composer->editor();setComposerText(composer,"Keep my main draft");
    client->inspectionReady({},"codex/hgs/dashboard",details);
    auto *roster = window.findChild<QTextBrowser *>("subagents");
    auto *stack = window.findChild<QStackedWidget *>("activityStack");
    roster->anchorClicked(QUrl("hgs-agent:unknown")); QCOMPARE(stack->currentIndex(),0);
    QSignalSpy childInspected(client,&HgsClient::subagentInspectionReady);
    roster->anchorClicked(QUrl("hgs-agent:child-1")); QCOMPARE(stack->currentIndex(),1); QVERIFY(!composer->isVisible());
    QVERIFY(!window.findChild<QLabel *>("subagentId"));
    QVERIFY(window.findChild<QTextBrowser *>("sessionInfo")->toPlainText().contains("child-1"));
    auto *view = window.findChild<ActivityView *>("subagentActivity");
    WAIT_VERIFY(!childInspected.isEmpty());
    client->subagentInspectionReady({},"codex/hgs/dashboard","child-1",{},child);
    QCOMPARE(window.findChild<QLabel *>("detailsIdentityValue")->text(), QString("conversation-one/child-1"));
    QSignalSpy copied(&window, &SessionsWindow::copyTextRequested);
    window.findChild<QPushButton *>("copySessionIdAction")->click(); QCOMPARE(copied.size(), 1);
    QCOMPARE(copied[0][0].toString(), QString("conversation-one/child-1"));
    window.findChild<QPushButton *>("headerCopySessionIdAction")->click(); QCOMPARE(copied.size(), 2);
    QCOMPARE(copied[1][0].toString(), QString("conversation-one/child-1"));
    WAIT_VERIFY2(view->browser()->toPlainText().contains("Review finished"), qPrintable(view->browser()->toPlainText()));
    QCOMPARE(window.findChild<QPushButton *>("subagentContext")->property("contextPercent").toDouble(),25.);
    auto *childComposer=window.findChild<MessageComposer *>("subagentComposer");auto *childContext=window.findChild<QPushButton *>("subagentContext");
    QVERIFY(childComposer->isVisible());QVERIFY(!childComposer->isInputVisible());QVERIFY(childContext->isVisible());
    QVERIFY(childComposer->toolbar()->isAncestorOf(childContext));
    QCOMPARE(window.findChild<QPushButton *>("activityContext")->property("contextPercent").toDouble(),80.);
    window.findChild<QPushButton *>("subagentBack")->click(); QCOMPARE(stack->currentIndex(),0);
    QCOMPARE(editor->toPlainText(),QString("Keep my main draft"));
    roster->anchorClicked(QUrl("hgs-agent:child-1"));
    window.showSession({},"kimi/docs/research"); QCOMPARE(stack->currentIndex(),0);
    client->subagentInspectionReady({},"codex/hgs/dashboard","child-1",{},child);
    QCOMPARE(stack->currentIndex(),0);
}

void TestSessionsWindow::childTreeKeepsRoutineResultsQuiet()
{
    QSettings().setValue("workspace/showCompletedSubagents", true);
    FleetState state; auto box = fleet().local(); auto session = box.sessions[0];
    session.conversationId = "tree-conversation"; session.runId = "tree-run";
    session.activity = "idle"; session.phase = "idle"; session.currentTool.clear();
    session.subagentCountsComplete = true; session.subagentTotalCount = 2; session.subagentActiveCount = 1;
    session.subagents = {{"agent-a", QJsonObject{{"name","Review"},{"state","working"},{"detail","Checking changes"}}},
        {"agent-b", QJsonObject{{"name","Tests"},{"state","finished"},{"detail","All tests pass"},{"reply_id","first-result"}}}};
    box.sessions = {session};
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); QTest::qWait(60);
    auto *list = window.findChild<SessionList *>("sessionList"); QVERIFY(list);
    auto *parent = list->item(0); QVERIFY(parent->data(SessionRoles::HasChildren).toBool());
    QVERIFY(!parent->data(SessionRoles::Unread).toBool());
    QVERIFY(!parent->data(SessionRoles::Attention).toBool());
    auto *brand = window.findChild<QPushButton *>("brandMark");
    auto *badge = window.findChild<QLabel *>("railAttentionBadge");
    QCOMPARE(brand->property("attentionCount").toInt(), 0); QVERIFY(!badge->isVisible());
    const auto rootKey = parent->data(SessionRoles::Key).toString();
    const auto count = list->count();
    QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, list->visualItemRect(parent).topLeft() + QPoint(16, 21));
    QCOMPARE(list->count(), count); // The title selects the root; only the trailing counter expands it.
    const auto togglePoint = [&] { return list->visualItemRect(list->item(0)).bottomRight() - QPoint(20, 18); };
    QTest::mouseMove(list->viewport(), togglePoint());
    QCOMPARE(list->viewport()->cursor().shape(), Qt::PointingHandCursor);
    QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, togglePoint());
    QCOMPARE(list->count(), count + 2);
    QCOMPARE(list->item(1)->data(SessionRoles::ChildId).toString(), QString("agent-a"));
    QVERIFY(!list->item(1)->flags().testFlag(Qt::ItemIsDragEnabled));
    QVERIFY(!list->item(2)->data(SessionRoles::Unread).toBool());
    QVERIFY(list->item(2)->data(Qt::AccessibleTextRole).toString().contains("Ready"));
    auto *composer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(composer, "Parent draft");
    list->setCurrentRow(1);
    QCOMPARE(window.findChild<QStackedWidget *>("activityStack")->currentIndex(), 1);
    QVERIFY(!composer->isVisible());
    QVERIFY(!window.findChild<QPushButton *>("more")->isVisible());
    window.setFleet(state);
    QCOMPARE(list->currentItem()->data(SessionRoles::ChildId).toString(), QString("agent-a"));
    QTest::keyClick(list, Qt::Key_Left);
    QCOMPARE(list->currentItem()->data(SessionRoles::Key).toString(), rootKey);
    QCOMPARE(composer->editor()->toPlainText(), QString("Parent draft"));
    QTest::keyClick(list, Qt::Key_Left); QCOMPARE(list->count(), count);
    list->setProperty("compact", false); list->doItemsLayout();
    QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, togglePoint());
    QCOMPARE(list->count(), count + 2);
    QTest::keyClick(list, Qt::Key_Left); QCOMPARE(list->count(), count);
    QAction *readAll = nullptr;
    for (auto *action : window.findChildren<QAction *>()) if (action->text().startsWith("Mark all as read")) readAll = action;
    QVERIFY(readAll); QVERIFY(!readAll->isEnabled());
    // New child results and routine failures remain quiet without opening them.
    for (const auto &status : {"finished", "ready", "idle", "error"}) {
        box.sessions[0].subagents["agent-b"] = QJsonObject{{"name","Tests"},{"state",status},{"detail","New internal result"},{"reply_id","second-result"}};
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
        QVERIFY(!list->item(0)->data(SessionRoles::Unread).toBool());
        QVERIFY(!list->item(0)->data(SessionRoles::Attention).toBool());
        QCOMPARE(brand->property("attentionCount").toInt(), 0); QVERIFY(!readAll->isEnabled());
    }
    auto *filter = [&]() -> QPushButton * { for (auto *button : window.findChildren<QPushButton *>())
        if (button->property("filter") == "attention") return button; return nullptr; }();
    QVERIFY(filter); filter->click(); QCOMPARE(list->count(), 0);
    // An explicit child approval still reaches the attention filter and rail.
    box.sessions[0].subagents["agent-b"] = QJsonObject{{"name","Tests"},{"state","working"},{"display_state","approval"}};
    box.sessions[0].subagentActiveCount = 2;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(SessionRoles::Attention).toBool());
    QCOMPARE(brand->property("attentionCount").toInt(), 1); QVERIFY(badge->isVisible()); QVERIFY(!readAll->isEnabled());
    box.sessions[0].subagents["agent-b"] = QJsonObject{{"name","Tests"},{"state","finished"}};
    box.sessions[0].subagentActiveCount = 1;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), 0); QCOMPARE(brand->property("attentionCount").toInt(), 0);
    // An intermediate parent reply remains unread while work continues.
    box.sessions[0].activity = "busy"; box.sessions[0].phase = "tool";
    box.sessions[0].replyId = "12:120";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), 0); QCOMPARE(brand->property("attentionCount").toInt(), 0);
    QVERIFY(state.local().sessions[0].unreadReply);
    QVERIFY(readAll->isEnabled()); // The exact unread reply is retained for explicit reading.
    // Finishing the parent turn still waits for its active child.
    box.sessions[0].activity = "idle"; box.sessions[0].phase = "idle";
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), 0); QCOMPARE(brand->property("attentionCount").toInt(), 0);
    // Finishing that child reveals the same unread reply without a new reply ID.
    box.sessions[0].subagents["agent-a"] = QJsonObject{{"name","Review"},{"state","finished"}};
    box.sessions[0].subagentActiveCount = 0;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(SessionRoles::Unread).toBool());
    QVERIFY(readAll->isEnabled()); QSignalSpy read(&window, &SessionsWindow::repliesMarkedRead);
    readAll->trigger(); QCOMPARE(read.size(), 1); QCOMPARE(read[0][0].toJsonObject().size(), 1);
    QVERIFY(!list->item(0)->data(SessionRoles::Unread).toBool()); QVERIFY(!readAll->isEnabled());
}

void TestSessionsWindow::completedSubagentsPreferenceKeepsOpenHistory()
{
    // The inspector lists finished children here (owner, 2026-10-28: by default
    // they fold into a count with a link that opens them).
    QSettings().setValue("workspace/inspectorFinishedSubagents", true);
    const auto restoreFinished = qScopeGuard([] { QSettings().remove("workspace/inspectorFinishedSubagents"); });
    FleetState state; auto box = fleet().local(); auto session = box.sessions[0];
    session.conversationId = "conversation-one"; session.runId = "run-one";
    session.subagentSource = "hooks"; session.subagentCountsComplete = true;
    session.subagentActiveCount = 1; session.subagentTotalCount = 29;
    session.subagents = {{"a", QJsonObject{{"name", "Open child"}, {"state", "working"}}},
        {"b", QJsonObject{{"name", "Past child"}, {"state", "finished"}}},
        {"c", QJsonObject{{"name", "Approval child"}, {"state", "finished"}, {"display_state", "approval"}}},
        {"d", QJsonObject{{"name", "Unknown child"}}}};
    box.sessions = {session}; state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    QJsonObject details{{"tracked", true}, {"conversation_id", session.conversationId}, {"run_id", session.runId},
        {"subagent_source", "hooks"}, {"subagents", session.subagents}, {"subagent_counts_complete", true},
        {"subagent_active_count", 1}, {"subagent_total_count", 29}};
    QJsonArray events;
    for (int i = 0; i < 80; ++i) events.append(QJsonObject{{"seq", i+1}, {"type", "AgentMessage"}, {"detail", QString("Retained child history %1").arg(i)}, {"at", 1700000000+i}});
    const QJsonObject childDetails{{"parent_conversation_id", session.conversationId},
        {"conversation_id", "child-conversation"}, {"run_id", "run-one"}, {"send_supported", true}, {"events", events}};
    // Every asynchronous inspection sees the fixture's current roster, rather
    // than replacing an injected finished turn with the generic working fleet.
    const auto fixtureScript = m_dir->filePath("hgs-completed-subagents");
    const auto saveSnapshot = [&](const QString &suffix, const QJsonObject &snapshot) {
        QVERIFY(FakeCli::replace(fixtureScript + suffix, QJsonDocument(snapshot).toJson(QJsonDocument::Compact)));
    };
    saveSnapshot(".parent", details); saveSnapshot(".child", childDetails);
    QFile fixture(fixtureScript); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys\na=sys.argv[1:]\nif a and a[0].startswith('@'):a=a[1:]\n"
        "if a[:1]==['inspect']:sys.stdout.write(open(__file__+('.child' if '--agent' in a else '.parent')).read())\n");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    SessionsWindow window(FakeCli::program(fixtureScript)); auto *client = window.findChild<HgsClient *>();
    QSignalSpy parentInspected(client, &HgsClient::inspectionReady);
    QSignalSpy childInspected(client, &HgsClient::subagentInspectionReady);
    window.setFleet(state); window.show(); QTRY_VERIFY(!parentInspected.isEmpty());
    auto *list = window.findChild<SessionList *>("sessionList");
    auto *roster = window.findChild<QTextBrowser *>("subagents");
    auto *check = window.findChild<QCheckBox *>("workspaceShowCompletedSubagents"); QVERIFY(check); QVERIFY(!check->isChecked());
    const auto rootKey = list->item(0)->data(SessionRoles::Key).toString();
    list->childrenToggled(rootKey); QCOMPARE(list->count(), 4);
    QCOMPARE(list->item(1)->data(SessionRoles::ChildId).toString(), QString("a"));
    QCOMPARE(list->item(2)->data(SessionRoles::ChildId).toString(), QString("c"));
    QVERIFY(roster->toPlainText().contains("Needs input")); QVERIFY(roster->toPlainText().contains("Past child"));
    QCOMPARE(list->item(0)->data(SessionRoles::Children).toString(), QString("1"));
    check->setChecked(true); QCOMPARE(list->count(), 5); QVERIFY(roster->toPlainText().contains("Past child"));
    check->setChecked(false); QCOMPARE(list->count(), 4);
    auto *parentComposer = window.findChild<MessageComposer *>("messageComposer"); setComposerText(parentComposer, "Parent draft");
    const auto beforeOpen = parentInspected.size(); list->setCurrentRow(1);
    QTRY_VERIFY(parentInspected.size() > beforeOpen); QTRY_VERIFY(!childInspected.isEmpty());
    auto *childComposer = window.findChild<MessageComposer *>("subagentComposer"); setComposerText(childComposer, "Child draft");
    auto *view = window.findChild<ActivityView *>("subagentActivity"); auto *scroll = view->browser()->verticalScrollBar();
    scroll->setValue(scroll->maximum()/2); const auto position = scroll->value();
    const auto history = view->browser()->toPlainText();
    auto children = details.value("subagents").toObject(); auto child = children.value("a").toObject(); child["state"] = "finished"; children["a"] = child;
    details["subagents"] = children; saveSnapshot(".parent", details); client->inspectionReady({}, session.name, details);
    check->setChecked(true); check->setChecked(false);
    QCOMPARE(list->currentItem()->data(SessionRoles::ChildId).toString(), QString("a"));
    QCOMPARE(childComposer->editor()->toPlainText(), QString("Child draft"));
    QCOMPARE(view->browser()->toPlainText(), history); QCOMPARE(scroll->value(), position);
    childComposer->editor()->clear(); window.findChild<QPushButton *>("subagentBack")->click();
    QTRY_COMPARE(list->count(), 3); QCOMPARE(parentComposer->editor()->toPlainText(), QString("Parent draft"));
    // The window may still be inspecting the parent it returned to (slowly under
    // load); a request for the same session starts once that one has finished.
    bool requested = false;
    QTRY_VERIFY_WITH_TIMEOUT(requested || (requested = client->requestInspection({}, session.name, 0)), 10000);
    const auto beforeReinspect = parentInspected.size(); QTRY_VERIFY(parentInspected.size() > beforeReinspect);
    QCOMPARE(list->count(), 3); QCOMPARE(parentComposer->editor()->toPlainText(), QString("Parent draft"));
    // The inspector retains completed previews independently of the list filter.
    details["subagents"] = QJsonObject{};
    details["subagent_previews"] = QJsonArray{QJsonObject{{"id", "preview"}, {"name", "Past preview"}, {"state", "finished"}}};
    saveSnapshot(".parent", details); client->inspectionReady({}, session.name, details);
    QVERIFY(roster->toPlainText().contains("Past preview"));
    QVERIFY(!roster->toPlainText().contains("Completed turns are hidden"));
    check->setChecked(true); QVERIFY(roster->toPlainText().contains("Past preview"));
    SessionsWindow reopened(program()); QVERIFY(reopened.findChild<QCheckBox *>("workspaceShowCompletedSubagents")->isChecked());
    // A saved old-conversation draft must not pin a reused ID in a new roster.
    check->setChecked(false); details["subagents"] = children; details.remove("subagent_previews");
    saveSnapshot(".parent", details); client->inspectionReady({}, session.name, details);
    roster->anchorClicked(QUrl("hgs-agent:a")); setComposerText(childComposer, "Old conversation draft");
    box.sessions[0].conversationId = "replacement-conversation"; box.sessions[0].subagents = {{"a", child}};
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    for (int row = 0; row < list->count(); ++row) QVERIFY(list->item(row)->data(SessionRoles::ChildId).toString() != "a");
}

void TestSessionsWindow::inactiveSubagentsStayInInspectorAndOutOfSessionCounts()
{
    // The inspector lists finished children here (owner, 2026-10-28: by default
    // they fold into a count with a link that opens them).
    QSettings().setValue("workspace/inspectorFinishedSubagents", true);
    const auto restoreFinished = qScopeGuard([] { QSettings().remove("workspace/inspectorFinishedSubagents"); });
    FleetState state; auto box = fleet().local(); box.sessions = {box.sessions[0]};
    auto &session = box.sessions[0]; session.conversationId = "conversation-one"; session.runId = "run-one";
    session.subagentSource = "hooks"; session.subagentCountsComplete = true;
    session.subagentActiveCount = 0; session.subagentTotalCount = 175;
    for (int i = 0; i < 175; ++i)
        session.subagents[QString("child-%1").arg(i, 3, 10, QChar('0'))] = QJsonObject{
            {"name", QString("Completed child %1").arg(i)}, {"state", "finished"}};
    const auto program = m_dir->filePath("hgs-full-roster");
    QFile scriptFile(program); QVERIFY(scriptFile.open(QIODevice::WriteOnly));
    scriptFile.write("#!/usr/bin/env python3\nimport sys\nif sys.argv[1:2]==['inspect']:sys.stdout.write(open(__file__+'.parent').read())\n"); scriptFile.close();
    QVERIFY(FakeCli::makeRunnable(scriptFile.fileName()));
    const auto snapshot = [&] {
        return QJsonObject{{"tracked", true}, {"conversation_id", session.conversationId}, {"run_id", session.runId},
            {"subagent_source", session.subagentSource}, {"subagents", session.subagents},
            {"subagent_counts_complete", session.subagentCountsComplete},
            {"subagent_active_count", session.subagentActiveCount}, {"subagent_total_count", session.subagentTotalCount}};
    };
    const auto save = [&] {
        QVERIFY(FakeCli::replace(program + ".parent", QJsonDocument(snapshot()).toJson(QJsonDocument::Compact)));
        state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    };
    save(); SessionsWindow window(FakeCli::program(program)); auto *client = window.findChild<HgsClient *>();
    QSignalSpy inspected(client, &HgsClient::inspectionReady);
    window.setFleet(state); window.show(); QTRY_VERIFY(!inspected.isEmpty());
    auto *list = window.findChild<SessionList *>("sessionList"); auto *roster = window.findChild<QTextBrowser *>("subagents");
    auto *check = window.findChild<QCheckBox *>("workspaceShowCompletedSubagents");
    const auto key = list->item(0)->data(SessionRoles::Key).toString();
    QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(SessionRoles::Children).toString().isEmpty());
    QVERIFY(!list->item(0)->data(SessionRoles::HasChildren).toBool());
    QVERIFY(SessionList::childrenControlRect(list->visualItemRect(list->item(0)), list->model()->index(0,0), list->font()).isEmpty());
    QVERIFY(!list->item(0)->data(Qt::AccessibleTextRole).toString().contains("175"));
    QVERIFY(roster->toPlainText().contains("0 active / 175 total"));
    for (int i = 0; i < 175; ++i) QVERIFY(roster->toPlainText().contains(QString("Completed child %1").arg(i)));
    auto child = session.subagents["child-000"].toObject(); child["state"] = "working"; session.subagents["child-000"] = child;
    session.subagentActiveCount = 1; save(); window.setFleet(state); client->inspectionReady({}, session.name, snapshot());
    QCOMPARE(list->item(0)->data(SessionRoles::Children).toString(), QString("1"));
    QVERIFY(list->item(0)->data(Qt::AccessibleTextRole).toString().contains("1 active"));
    list->childrenToggled(key); QCOMPARE(list->count(), 2);
    QCOMPARE(list->item(1)->data(SessionRoles::ChildId).toString(), QString("child-000"));
    QVERIFY(roster->toPlainText().contains("1 active / 175 total"));
    check->setChecked(true); QCOMPARE(list->count(), 176);
    QCOMPARE(list->item(0)->data(SessionRoles::Children).toString(), QString("1/175"));
    check->setChecked(false); QCOMPARE(list->count(), 2);
    child["state"] = "finished"; session.subagents["child-000"] = child; session.subagentActiveCount = 0;
    save(); window.setFleet(state); client->inspectionReady({}, session.name, snapshot());
    QCOMPARE(list->count(), 1); QVERIFY(list->item(0)->data(SessionRoles::Children).toString().isEmpty());
    QVERIFY(!list->item(0)->data(SessionRoles::HasChildren).toBool());
    QVERIFY(roster->toPlainText().contains("Completed child 174"));
    // A failed turn has ended too: the inspector keeps its error, the tree hides it.
    child["display_state"] = "error"; child["detail"] = "Usage limit reached"; session.subagents["child-000"] = child;
    save(); window.setFleet(state); client->inspectionReady({}, session.name, snapshot());
    QCOMPARE(list->count(), 1); QVERIFY(!list->item(0)->data(SessionRoles::HasChildren).toBool());
    QVERIFY(roster->toPlainText().contains("Error")); QVERIFY(roster->toPlainText().contains("Usage limit reached"));
    check->setChecked(true); QCOMPARE(list->count(), 176);
    check->setChecked(false); QCOMPARE(list->count(), 1);
    // Unknown evidence remains explicit; an incomplete hook roster cannot claim zero active.
    session.subagentCountsComplete = false; save(); window.setFleet(state); client->inspectionReady({}, session.name, snapshot());
    QCOMPARE(list->item(0)->data(SessionRoles::Children).toString(), QString("0+"));
    QVERIFY(roster->toPlainText().contains("some states unknown"));
}

void TestSessionsWindow::subagentGroupsAndUnavailableActivity()
{
    auto state = fleet(); auto box = state.local(); auto &session = box.sessions[0];
    session.subagentSource = "hook_profiles"; session.subagentTotalCount = 3; session.subagentActiveCount = 3;
    session.subagentCountsComplete = false;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show(); QTest::qWait(60);
    auto *list = window.findChild<QListWidget *>("sessionList"); QCOMPARE(list->item(0)->data(Qt::UserRole + 7).toString(), QString("?"));
    QVERIFY(list->item(0)->toolTip().isEmpty());
    QVERIFY(list->item(0)->data(Qt::AccessibleTextRole).toString().contains("incomplete counts"));
    auto *client = window.findChild<HgsClient *>();
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"subagent_source", "hook_profiles"},
        {"subagent_counts_complete", true}, {"subagent_active_count", 2}, {"subagent_total_count", 8},
        {"subagents", QJsonObject{{"collapsed", QJsonObject{{"label", "Do not invent a child"}, {"state", "working"}}}}},
        {"subagent_groups", QJsonObject{
            {"code", QJsonObject{{"label", "code"}, {"active_count", 2}, {"total_count", 5}, {"detail", "Review implementation"}}},
            {"review", QJsonObject{{"label", "review"}, {"active_count", 0}, {"total_count", 3}, {"detail", "Read final diff"}}}}}};
    client->inspectionReady({}, session.name, details);
    auto *roster = window.findChild<QTextBrowser *>("subagents"); auto text = roster->toPlainText();
    QVERIFY(text.contains("Profile groups")); QVERIFY(text.contains("2 active / 8 observed runs"));
    QVERIFY(text.contains("code")); QVERIFY(text.contains("Read final diff")); QVERIFY(!text.contains("Do not invent a child"));
    auto *showCompleted = window.findChild<QCheckBox *>("workspaceShowCompletedSubagents"); QVERIFY(showCompleted);
    showCompleted->setChecked(true); QVERIFY(roster->toPlainText().contains("Read final diff"));
    showCompleted->setChecked(false);
    details["subagent_counts_complete"] = false; client->inspectionReady({}, session.name, details);
    text = roster->toPlainText(); QVERIFY(text.contains("incomplete counts")); QVERIFY(!text.contains("2/5")); QVERIFY(text.contains("Read final diff"));
    details["subagent_groups"] = QJsonObject{}; details["subagents"] = QJsonObject{};
    client->inspectionReady({}, session.name, details);
    QVERIFY(roster->toPlainText().contains("Exact subagent counts are unavailable"));
    QVERIFY(!roster->toPlainText().contains("No subagent activity yet"));
    details["subagent_source"] = "unavailable"; client->inspectionReady({}, session.name, details);
    QVERIFY(roster->toPlainText().contains("activity is unavailable")); QVERIFY(!roster->toPlainText().contains("0 active"));
}

void TestSessionsWindow::inspectorPreservesWorkspaceAndReportsTasks()
{
    QSettings().setValue("processes/enabled",true);
    SessionsWindow window(program()); window.setFleet(fleet()); window.resize(1120, 760); window.show(); QTest::qWait(80);
    auto *tabs = window.findChild<QTabWidget *>("sessionDetailTabs");
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    auto *toggle = window.findChild<QPushButton *>("toggleInspector");
    auto *panel = window.findChild<QFrame *>("sessionInspectorPanel");
    auto *splitter = window.findChild<QSplitter *>("sessionWorkSplitter");
    auto *editor = window.findChild<MessageComposer *>("messageComposer")->findChild<QPlainTextEdit *>("messageInput");
    QVERIFY(tabs && inspector && toggle && panel && splitter && editor);
    QCOMPARE(tabs->count(), 5); QCOMPARE(tabs->tabText(0), QString("Activity")); QCOMPARE(tabs->tabText(1), QString("Terminal"));
    QCOMPARE(tabs->tabText(2), QString("Native UI")); QCOMPARE(tabs->tabText(3), QString("Processes"));
    QVERIFY(tabs->isTabVisible(1)); QVERIFY(!tabs->isTabVisible(2)); QVERIFY(tabs->isTabVisible(3));
    QVERIFY(panel->isHidden()); QCOMPARE(tabs->cornerWidget(), toggle->parentWidget());
    editor->setPlainText("Keep this draft"); const int originalWidth = tabs->width();
    QTest::mouseClick(toggle, Qt::LeftButton); QTest::qWait(40);
    QVERIFY(inspector->isVisible()); QVERIFY(tabs->width() < originalWidth); QVERIFY(panel->width() >= 240);
    // U25 part 6: no tab bar in the inspector; the toggle's corner stays in the session's tabs.
    QCOMPARE(tabs->cornerWidget(), toggle->parentWidget()); QVERIFY(!inspector->findChild<QTabBar *>());
    auto *corner = tabs->cornerWidget();
    QVERIFY(toggle->mapTo(corner, QPoint()).y() >= 4);
    QVERIFY(toggle->mapTo(corner, toggle->rect().bottomRight()).y() <= corner->height() - 5);
    QVERIFY(toggle->iconSize() == QSize(18, 18));

    QCOMPARE(tabs->currentIndex(), 0); QCOMPARE(editor->toPlainText(), QString("Keep this draft"));
    QJsonObject data{{"tracked", true}, {"run_id", "run-1"}, {"conversation_id", "conversation-one"},
        {"subagent_source", "hooks"}, {"subagents", QJsonObject{{"review", QJsonObject{{"name", "Reviewer"}, {"state", "working"}}}}},
        {"task_lists", QJsonObject{
            {"main", QJsonObject{{"run_id", "run-1"}, {"items", QJsonArray{
                QJsonObject{{"title", "Inspect <source> & hooks"}, {"status", "completed"}},
                QJsonObject{{"title", "Implement right panel"}, {"status", "in_progress"}}}}}},
            {"agent:review", QJsonObject{{"run_id", "older-run"}, {"items", QJsonArray{
                QJsonObject{{"title", "Verify boundaries"}, {"status", "pending"}}}}}}
        }}};
    auto *client = window.findChild<HgsClient *>(); client->inspectionReady({}, "codex/hgs/dashboard", data);
    auto *roster = window.findChild<QTextBrowser *>("subagents"); const auto text = roster->toPlainText();
    QVERIFY(text.contains("Inspect <source> & hooks")); QVERIFY(!text.contains("1 of 2 completed"));
    QCOMPARE(inspectorCard(window, "tasks")->findChild<QLabel *>("inspectorCardValue")->text(), QString("1 of 2 completed"));
    QVERIFY(text.contains("In progress")); QVERIFY(text.contains("Reviewer")); QVERIFY(text.contains("Last recorded"));
    {   // WhiteWh fork: while the session works, the line above the message field names the plan's current step.
        auto *working = static_cast<WorkingLine *>(window.findChild<QWidget *>("workingLine")); QVERIFY(working); QVERIFY(!working->isVisible());
        auto busy = data; busy["activity"] = "busy"; busy["phase"] = "tool"; busy["tool_detail"] = "cargo build";
        busy["turn_started"] = double(QDateTime::currentSecsSinceEpoch() - 75);
        client->inspectionReady({}, "codex/hgs/dashboard", busy);
        QTRY_VERIFY(working->isVisible()); QCOMPARE(working->text(), QString("Implement right panel"));
        if (const auto preview = qEnvironmentVariable("HGS_WORKING_PREVIEW"); !preview.isEmpty()) QVERIFY(working->grab().save(preview));
        busy["task_lists"] = QJsonObject{}; client->inspectionReady({}, "codex/hgs/dashboard", busy);
        QTRY_COMPARE(working->text(), QString("cargo build"));
        QCOMPARE(WorkingLine::elapsed(564), QString("9m 24s")); QCOMPARE(WorkingLine::elapsed(3900), QString("1h 05m"));
        client->inspectionReady({}, "codex/hgs/dashboard", data); QTRY_VERIFY(!working->isVisible());
    }
    openInspectorCard(window, "worktrees"); QCOMPARE(tabs->currentIndex(), 0);
    splitter->setSizes({420, 310});
    QTest::mouseClick(toggle, Qt::LeftButton); QTest::qWait(40);
    QVERIFY(panel->isHidden()); QVERIFY(!toggle->isChecked()); QCOMPARE(editor->toPlainText(), QString("Keep this draft"));
    QCOMPARE(tabs->cornerWidget(), toggle->parentWidget()); QVERIFY(toggle->isVisible());
    tabs->setCurrentIndex(1); QTest::mouseClick(toggle, Qt::LeftButton); QCOMPARE(tabs->currentIndex(), 1);
    QVERIFY(inspectorCardOpen(window, "worktrees"));
    data["processes"]=QJsonObject{{"active_count",1},{"items",QJsonArray{
        QJsonObject{{"id","build"},{"command","cargo build --release"},{"description","Build the updated CLI"},{"owner","main"},{"cwd","/workspace/zerus"},{"status","running"},{"started_at",QDateTime::currentSecsSinceEpoch()-23}},
        QJsonObject{{"id","tests"},{"command","python3 tests/test_dsh_native.py -v"},{"owner","main"},{"cwd","/workspace/zerus"},{"status","completed"},{"started_at",QDateTime::currentSecsSinceEpoch()-65},{"ended_at",QDateTime::currentSecsSinceEpoch()-12},{"exit_code",0}}
    }}};
    client->inspectionReady({},"codex/hgs/dashboard",data);QCOMPARE(tabs->tabText(tabs->indexOf(window.findChild<ProcessView *>())),QString("Processes (1)"));
    tabs->setCurrentWidget(window.findChild<ProcessView *>());QVERIFY(window.findChild<QTreeWidget*>("processList")->isVisible());QCOMPARE(editor->toPlainText(),QString("Keep this draft"));
    if(!qEnvironmentVariable("HGS_PROCESSES_WINDOW_PREVIEW").isEmpty()){
        QTest::mouseClick(toggle,Qt::LeftButton);QTest::qWait(80);QVERIFY(window.grab().save(qEnvironmentVariable("HGS_PROCESSES_WINDOW_PREVIEW")));QTest::mouseClick(toggle,Qt::LeftButton);
    }
    window.close();
    SessionsWindow reopened(program()); reopened.setFleet(fleet()); reopened.show(); QTest::qWait(60);
    auto *restored = reopened.findChild<QWidget *>("sessionInspector"); QVERIFY(restored->isVisible()); QVERIFY(inspectorCardOpen(reopened, "worktrees"));
    // A smaller window still gives both panels a usable width and no overlay.
    reopened.resize(960, 650); QTest::qWait(40);
    auto *main = reopened.findChild<QTabWidget *>("sessionDetailTabs"); QVERIFY(main->width() >= 280);
    auto *restoredPanel = reopened.findChild<QFrame *>("sessionInspectorPanel");
    QCOMPARE(main->parentWidget(), restoredPanel->parentWidget());
    QVERIFY(main->geometry().right() < restoredPanel->geometry().left());
    // U25 part 6: the cards have no tab bar to widen the panel; it keeps 240 px.
    QVERIFY(!restored->findChild<QTabBar *>()); QVERIFY(restoredPanel->minimumWidth() >= 240);
    // Larger tabs raise the panel's minimum width; a slow runner may apply that relayout after a fixed wait.
    QTRY_VERIFY2(restoredPanel->mapTo(&reopened,restoredPanel->rect().topRight()).x()<reopened.width(),
        qPrintable(QString("panel right=%1 window width=%2 window minimum=%3").arg(restoredPanel->mapTo(&reopened,restoredPanel->rect().topRight()).x())
            .arg(reopened.width()).arg(reopened.minimumSizeHint().width())));
    const auto inspectorPreview=qEnvironmentVariable("HGS_INSPECTOR_PREVIEW");if(!inspectorPreview.isEmpty())QVERIFY(reopened.grab().save(inspectorPreview));
    reopened.showSession({}, "kimi/docs/research"); QTest::qWait(50);
    QVERIFY(!reopened.findChild<QTextBrowser *>("subagents")->toPlainText().contains("Implement right panel"));
}

void TestSessionsWindow::inspectorWidthSurvivesResizeHideAndRestartAndResetsOnDoubleClick()
{
    QSettings().remove("workspace");
    int preferred = 0;
    {
        SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(fleet()); window.resize(1840, 900); window.show();
        auto *toggle = window.findChild<QPushButton *>("toggleInspector");
        auto *panel = window.findChild<QFrame *>("sessionInspectorPanel");
        auto *splitter = window.findChild<QSplitter *>("sessionWorkSplitter");
        toggle->click(); QTRY_VERIFY(panel->isVisible());
        auto *handle = splitter->handle(1); QVERIFY(handle);
        const auto from = handle->rect().center();
        const auto destination = splitter->mapToGlobal(QPoint(splitter->width() - 760, handle->height() / 2));
        QTest::mousePress(handle, Qt::LeftButton, {}, from);
        QMouseEvent move(QEvent::MouseMove, QPointF(handle->mapFromGlobal(destination)), QPointF(destination), Qt::NoButton, Qt::LeftButton, Qt::NoModifier);
        QApplication::sendEvent(handle, &move);
        QTest::mouseRelease(handle, Qt::LeftButton, {}, handle->mapFromGlobal(destination));
        QTRY_VERIFY(panel->width() > 600);
        preferred = panel->width(); QTRY_COMPARE(QSettings().value("workspace/inspectorWidth").toInt(), preferred);
        window.resize(1100, 900); QTRY_VERIFY(panel->width() < preferred);
        QCOMPARE(QSettings().value("workspace/inspectorWidth").toInt(), preferred);
        toggle->click(); toggle->click(); QTest::qWait(30);
        QCOMPARE(QSettings().value("workspace/inspectorWidth").toInt(), preferred);
        window.resize(1840, 900); QTRY_COMPARE(panel->width(), preferred);
        toggle->click(); toggle->click(); QTRY_COMPARE(panel->width(), preferred);
        window.close();
    }
    SessionsWindow restored(program()); keepDetailsPollsOut(restored); restored.setFleet(fleet()); restored.resize(1840, 900); restored.show();
    auto *panel = restored.findChild<QFrame *>("sessionInspectorPanel"); auto *splitter = restored.findChild<QSplitter *>("sessionWorkSplitter");
    QTRY_VERIFY(panel->isVisible()); QTRY_COMPARE(panel->width(), preferred);
    auto *handle = splitter->handle(1); QTest::mouseDClick(handle, Qt::LeftButton); QTest::mouseRelease(handle, Qt::LeftButton);
    QTRY_COMPARE(QSettings().value("workspace/inspectorWidth").toInt(), 400);
    QTRY_COMPARE(panel->width(), qMax(400, panel->minimumWidth()));
}

void TestSessionsWindow::detailsPollingPreservesReadingPosition()
{
    SessionsWindow window(program()); window.resize(1150, 650); window.setFleet(fleet());
    // The window's own inspections answer with the base fixture's shorter details in
    // the same conversation. One arriving while the test waits replaces the document
    // under test, and the kept position is clamped to its end. Inspect only on
    // selection, then wait for an inspection of the test's own: HgsClient runs one per
    // session, so once its reply is in, none is pending and no poll follows.
    window.setDetailsPollForTests(500, 24 * 60 * 60 * 1000); window.show();
    auto *client = window.findChild<HgsClient *>(); QSignalSpy inspections(client, &HgsClient::inspectionReady);
    window.showSession({}, "codex/hgs/dashboard");
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    auto *info = window.findChild<QTextBrowser *>("sessionInfo");
    // U25 part 6: the inspector's column scrolls, not the usage text.
    auto *scroll = static_cast<QScrollArea *>(inspector)->verticalScrollBar();
    qsizetype issued = -1;
    WAIT_VERIFY(issued >= 0 || (client->requestInspection({}, "codex/hgs/dashboard") && (issued = inspections.size(), true)));
    WAIT_VERIFY(std::any_of(inspections.cbegin() + issued, inspections.cend(), [](const QList<QVariant> &reply) {
        return reply[0].toString().isEmpty() && reply[1].toString() == "codex/hgs/dashboard" && reply[3].toString().isEmpty();
    }));
    QJsonObject totals{{"total", 4280764}, {"input", 4169472}, {"uncached_input", 64},
        {"cache_read", 3770489}, {"cache_write", 398919}, {"output", 111292},
        {"reasoning", 62310}, {"web_searches", 0}, {"web_fetches", 0}};
    QJsonObject usage{{"status", "ok"}, {"totals", totals}, {"requests", 32},
        {"context", QJsonObject{{"used", 233246}}}};
    QJsonObject details{{"tracked", true}, {"conversation_id", "conversation-one"}, {"run_id", "run-one"},
        {"cwd", "/work/project"}, {"git_root", "/work/project"}, {"git_branch", "main"},
        {"last_event_at", 1791266000}, {"session_usage", usage}};
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    WAIT_VERIFY(scroll->maximum() > 100);
    auto selected = info->document()->find("Session usage"); QVERIFY(!selected.isNull());
    info->setTextCursor(selected);
    scroll->setValue(scroll->maximum() - 40); const int position = scroll->value();
    const int revision = info->document()->revision();
    // Identical polls must not replace the document at all.
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    QCOMPARE(info->document()->revision(), revision); QCOMPARE(scroll->value(), position);
    for (int i = 0; i < 4; ++i) {
        details["last_event_at"] = 1791266001 + i;
        totals["total"] = 10000000 + i; usage["totals"] = totals;
        usage["requests"] = 100 + i; details["session_usage"] = usage;
        client->inspectionReady({}, "codex/hgs/dashboard", details);
        QCoreApplication::sendPostedEvents(nullptr, QEvent::LayoutRequest);
        QCOMPARE(scroll->value(), position);
        QCOMPARE(info->textCursor().selectedText(), QString("Session usage"));
        QVERIFY(info->toPlainText().simplified().contains(QLocale().toString(10000000 + i).simplified()));
        QVERIFY(info->updatesEnabled());
    }
    // A poll and closing/reopening the inspector are still updates to the
    // same reading context.
    details["last_event_at"] = 1791266010; client->inspectionReady({}, "codex/hgs/dashboard", details);
    WAIT_SETTLED(); QCOMPARE(scroll->value(), position);
    auto *toggle = window.findChild<QPushButton *>("toggleInspector");
    toggle->setChecked(false);
    details["last_event_at"] = 1791266011; client->inspectionReady({}, "codex/hgs/dashboard", details);
    toggle->setChecked(true); WAIT_SETTLED(); QCOMPARE(scroll->value(), position);
    // A different conversation in the same terminal starts at the top.
    details["conversation_id"] = "conversation-two";
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    client->inspectionReady({}, "codex/hgs/dashboard", details);
    QCOMPARE(scroll->value(), 0); QVERIFY(!info->textCursor().hasSelection());
    scroll->setValue(scroll->maximum()); QVERIFY(scroll->value() > 0);
    window.showSession("mac", "claude/infra/review");
    QCOMPARE(scroll->value(), 0); QVERIFY(!info->textCursor().hasSelection());
}

void TestSessionsWindow::nativeGoalIsIndependentOfTurnState()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); QTest::qWait(80);
    auto *client = window.findChild<HgsClient *>();
    auto *goalButton = window.findChild<QPushButton *>("sessionGoal");
    QVERIFY(goalButton->isHidden());
    QJsonObject goal{{"id", "goal-one"}, {"objective", "Finish <migration> & verify restore"}, {"status", "active"},
        {"tokens_used", 1000}, {"token_budget", 9000}, {"time_used_seconds", 154800}};
    QJsonObject data{{"tracked", true}, {"conversation_id", "conversation-one"}, {"activity", "idle"}, {"goal", goal}};
    client->inspectionReady({}, "codex/hgs/dashboard", data);
    QVERIFY(goalButton->isVisible()); QVERIFY(goalButton->text().contains("Pursuing goal"));
    QVERIFY(goalButton->text().contains("1d 19h"));
    QTest::mouseClick(goalButton, Qt::LeftButton);
    auto *inspector = window.findChild<QWidget *>("sessionInspector");
    QVERIFY(inspector->isVisible()); QVERIFY(inspectorCardOpen(window, "tasks"));
    auto *roster = window.findChild<QTextBrowser *>("subagents");
    QVERIFY(roster->toPlainText().contains("Finish <migration> & verify restore"));
    QVERIFY(roster->toPlainText().contains("tokens"));
    for (const auto &state : {"paused", "blocked", "usage_limited", "budget_limited", "complete"}) {
        goal["status"] = state; data["goal"] = goal; client->inspectionReady({}, "codex/hgs/dashboard", data);
        QVERIFY(goalButton->isVisible()); QVERIFY(!goalButton->text().contains("Pursuing goal"));
    }
    data["goal"] = QJsonValue::Null; data["goal_source"] = "unavailable";
    client->inspectionReady({}, "codex/hgs/dashboard", data);
    QVERIFY(goalButton->isHidden()); QVERIFY(roster->toPlainText().contains("Goal data is unavailable"));
    window.showSession({}, "kimi/docs/research"); QTest::qWait(40);
    QVERIFY(goalButton->isHidden()); QVERIFY(!roster->toPlainText().contains("Finish <migration>"));
}

// User report 2026-10-28: switching sessions took up to 40 s. Every rebuild
// queued its own row preparation on the one preparation thread, stale ones
// included, and the selected session's inspection reply waited behind them.
// Rebuilds while a preparation runs fold into one more.
// User report 2026-10-28: a new session on a peer stayed "Confirming session"
// for a minute: peers are listed once a minute (or only through their event
// stream), and the one refresh after the launch came before the session was
// listed. Its computer is read every 2 s until the session is placed.
// Owner, 2026-10-28: with the Processes tab off (the default) the Tasks card
// never showed the agent's background jobs, since no read included them. While
// the card is on screen the reads include them, at the background interval.
void TestSessionsWindow::tasksCardReadsBackgroundJobsWithTheProcessesTabOff()
{
    QTemporaryDir temp; const auto program = temp.filePath("hgs"); QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"PY(#!/usr/bin/env python3
import sys,json,pathlib,time
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
if a and a[0]=='inspect':
 with (root/'inspections.jsonl').open('a') as f:f.write(json.dumps(a)+'\n')
 value=dict(tracked=True,run_id='run',conversation_id='conversation',activity='busy',phase='working',events=[],cursor=0)
 if '--skip-processes' not in a:value['processes']=dict(items=[dict(id='job',background=True,status='running',description='Wait for the fork build',started_at=time.time()-120)])
 print(json.dumps(value))
else:print('{}')
)PY");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QVERIFY(!ProcessSettings::enabled());
    SessionsWindow window(FakeCli::program(program)); window.resize(1440, 900); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    window.findChild<QPushButton *>("toggleInspector")->setChecked(true);
    auto *inspector = static_cast<InspectorCards *>(window.findChild<QScrollArea *>("sessionInspector")); QVERIFY(inspector);
    inspector->open(InspectorCards::Tasks); QTRY_VERIFY(inspector->isOpen(InspectorCards::Tasks));
    const auto log = [&] { QStringList result; QFile file(temp.filePath("inspections.jsonl")); if (file.open(QIODevice::ReadOnly)) while (!file.atEnd()) result << QString::fromUtf8(file.readLine()); return result; };
    const auto readsProcesses = [&] { const auto calls = log(); return std::any_of(calls.cbegin(), calls.cend(), [](const QString &call) { return !call.contains("--skip-processes"); }); };
    QTRY_VERIFY_WITH_TIMEOUT(readsProcesses(), 15000);
    auto *roster = window.findChild<QTextBrowser *>("subagents");
    QTRY_VERIFY_WITH_TIMEOUT(roster->toPlainText().contains("Wait for the fork build"), 15000);
    QVERIFY(roster->toPlainText().contains("Background jobs"));
    // Owner, 2026-10-28: switching took about 4 s while the first read of the
    // new session waited for its processes. That read skips them; the next has them.
    const auto reads = [&](const QString &name) {
        QStringList result; for (const auto &call : log()) if (call.contains(name)) result << call; return result; };
    window.showSession({}, "codex/website/navigation");
    QTRY_VERIFY_WITH_TIMEOUT(!reads("codex/website/navigation").isEmpty(), 15000);
    QVERIFY(reads("codex/website/navigation").first().contains("--skip-processes"));
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto calls = reads("codex/website/navigation");
        return std::any_of(calls.cbegin(), calls.cend(), [](const QString &call) { return !call.contains("--skip-processes"); }); })(), 15000);
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_inspector2.moc"
