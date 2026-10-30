// WhiteWh fork, #713: the agent-team UI tests of member cards, their menus,
// queues and blocks, and the task card.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "ActivityView.h"
#include "CollapsibleBlock.h"
#include "HgsClient.h"
#include "TeamTaskCard.h"
#include "TeamWorkspace.h"

namespace {

// U25 part 10 (spec 5.6): the running team with a QA lead as a sub-lead; the
// Tester reports to it and its unit tests wait for its review.
QJsonObject subLeadTeam()
{
    auto t = team();
    auto members = t["members"].toArray();
    auto tester = members[2].toObject(); tester["parent"] = "qa-lead"; members[2] = tester;
    members.append(QJsonObject{{"id", "qa-lead"}, {"role", "QA lead"}, {"agent", "codex"}, {"model", "gpt-6.1-sol"}, {"state", "active"},
                               {"sub", true}, {"session", QJsonObject{{"host", ""}, {"name", "codex/checkout/qa-lead"}}}});
    t["members"] = members;
    auto tasks = t["tasks"].toArray();
    auto unit = tasks[3].toObject();
    unit["status"] = "submitted";
    unit["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a4"}, {"member", "tester"}, {"state", "reported"}, {"sent_at", 1500},
                                              {"report", QJsonObject{{"text", "All green"}, {"at", 1600}}}}};
    tasks[3] = unit;
    t["tasks"] = tasks;
    return t;
}

// Members with a queue: the open task first, then what was returned to it.
// #6 was never handed out, so it is in no queue.
QJsonObject queueTeam()
{
    auto t = QJsonDocument::fromJson(R"({
 "id":"team-3","name":"Queues","status":"running","project":"Checkout","folder":"/work/queues","lead":"lead",
 "revision":4,"created":1000,"updated":5000,"settings":{},
 "members":[
  {"id":"lead","role":"Architect","agent":"claude","state":"active","session":{"host":"","name":"claude/queues/lead"}},
  {"id":"backend","role":"Backend","agent":"codex","state":"active","session":{"host":"","name":"codex/queues/backend"}},
  {"id":"tester","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/queues/tester"}}],
 "tasks":[
  {"id":1,"title":"Saved cards API","assignee":"backend","status":"in_progress",
   "attempts":[{"n":1,"id":"a1","member":"backend","state":"delivered","delivered_at":1700,"started_at":1710}]},
  {"id":3,"title":"Old cards API","assignee":"backend","status":"pending",
   "hold":{"by":"Lead","at":1730,"reason":"wait for the owner"},
   "attempts":[{"n":1,"id":"a3","member":"backend","state":"rejected"}]},
  {"id":4,"title":"Returned once","assignee":"backend","status":"pending",
   "attempts":[{"n":1,"id":"a4","member":"backend","state":"rejected"}]},
  {"id":6,"title":"Never handed out","assignee":"backend","status":"pending"},
  {"id":20,"title":"Tester work","assignee":"tester","status":"in_progress",
   "attempts":[{"n":1,"id":"a20","member":"tester","state":"delivered","delivered_at":1700}]}]})").object();
    auto tasks = t["tasks"].toArray();
    QJsonObject layers{{"1", 0}, {"3", 1}, {"4", 1}, {"6", 1}, {"20", 0}};
    // Seven more tasks returned to the tester: the queue shows five and "+N more".
    for (int id = 21; id <= 27; ++id) {
        tasks.append(QJsonObject{{"id", id}, {"title", QStringLiteral("Returned %1").arg(id)}, {"assignee", "tester"}, {"status", "pending"},
                                 {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", QStringLiteral("r%1").arg(id)}, {"member", "tester"}, {"state", "rejected"}}}}});
        layers[QString::number(id)] = 1;
    }
    t["tasks"] = tasks;
    t["layers"] = layers;
    t["cycle"] = QJsonArray(); t["ready"] = QJsonArray();
    t["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
    return t;
}
}

class TestTeamsUiMembers : public QObject, public TeamsUiFixture {
    Q_OBJECT
private slots:
    void initTestCase()
    {
        setUpCase();
        // #525: a pending Activity render is work in flight for the waits. A hidden
        // view stays dirty until it is shown, so only a visible one counts.
        TestWait::addProbe([] {
            for (auto *widget : QApplication::allWidgets())
                if (auto *activity = qobject_cast<ActivityView *>(widget); activity && activity->isVisible() && activity->renderPending()) return true;
            return false;
        });
    }
    void init() { setUp(); }
    void cleanup() { tearDown(); }
    void taskCardOffersActionsByStatus();
    void attemptStatesShowPhaseIcons();
    void taskCardAnswersQuestionsAndHolds();
    void memberQueueListsCurrentAndReturned();
    void ownerActionsCarryNoAgentMarkers();
    void leftoverMembersAreShownApart();
    void memberEditorPicksAnExistingSession();
    void rolePromptFromTheMemberMenuKeepsItsButton();
    void pinMarkAndFinishedTeamShowWhoFinished();
    void deliveryShots();
    void swapMemberAsksFirstAndListsTheOldSession();
    void memberMenuWorksBeforeTheStatusReadLands();
    void limitedAndSharedHostMembersAreMarked();
    void reportPreviewsAndFindings();
    void hierarchyInTeamViewsAndMemberCards();
    void workspaceAddsAMember();
    void subLeadsAreATreeInMembersAndTheInspector();
    void aPollUnderTheMemberMenuKeepsItsButtonAndTheSwapDialog();
    void memberBlocksShowTheirReasonAndReviewAction();
    void memberMenuScalesAndDrains();
    void sideListsAndMemberTasks();
};

void TestTeamsUiMembers::taskCardOffersActionsByStatus()
{
    const auto t = parse(team());
    TeamTaskCard card;
    card.resize(420, 900); card.show();
    QList<std::pair<QString, QJsonObject>> acted;
    card.act = [&](const QString &op, const QJsonObject &input) { acted.append({op, input}); };
    card.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    card.problemActions = [](const Member &, const Status &) { auto *w = new QWidget; w->setObjectName("problemActions"); w->setLayout(new QHBoxLayout); return w; };
    // Submitted: Accept, Reject with comment, Reassign; the report shows.
    card.setTask(t, 2, live, true);
    QVERIFY(card.findChild<QPushButton *>("teamTaskAccept"));
    QVERIFY(card.findChild<QFrame *>("teamTaskReport"));
    QCOMPARE(card.findChildren<QCheckBox *>("teamTaskCriterion").size(), 2);
    shot(&card, "task-card-submitted");
    card.findChild<QPushButton *>("teamTaskReject")->click();
    QTRY_VERIFY(card.findChild<QFrame *>("teamRejectForm"));
    card.findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Paginate the list");
    card.findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acted.size(), 1);
    QCOMPARE(acted[0].first, QStringLiteral("reject"));
    QCOMPARE(acted[0].second["reason"].toString(), QStringLiteral("Paginate the list"));
    // In progress: Reassign, Take over, Cancel; Accept waits under More with its reason.
    card.setTask(t, 3, live, true);
    QVERIFY(card.findChild<QPushButton *>("teamTaskTakeOver"));
    QVERIFY(!card.findChild<QPushButton *>("teamTaskAccept"));
    QAction *accept = nullptr;
    for (auto *action : card.findChild<QMenu *>("teamTaskMoreMenu")->actions()) if (action->text() == "Accept") accept = action;
    QVERIFY(accept && !accept->isEnabled());
    QVERIFY(accept->toolTip().contains("no report"));
    QCOMPARE(card.findChildren<QFrame *>("teamTaskAttempt").size(), 2);
    // A problem shows its banner with the member's actions.
    card.setTask(t, 4, live, true);
    QVERIFY(card.findChild<QFrame *>("teamTaskProblem"));
    QVERIFY(card.findChild<QWidget *>("problemActions"));
    shot(&card, "task-card-problem");
    // Mark done manually is your acceptance with a reason.
    for (auto *action : card.findChild<QMenu *>("teamTaskMoreMenu")->actions()) if (action->text().startsWith("Mark done")) action->trigger();
    QTRY_VERIFY(card.findChild<QFrame *>("teamDoneForm"));
    card.findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Ran them myself");
    card.findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acted.last().first, QStringLiteral("accept"));
    QVERIFY(acted.last().second["manual"].toBool());
}

void TestTeamsUiMembers::attemptStatesShowPhaseIcons()
{
    setTeam(deliveryTeam());
    const auto team = parse(deliveryTeam());
    const double now = 1900;   // well after the fixture's own times
    const QList<std::pair<int, QString>> expected{
        {11, "sent"}, {12, "delivered"}, {13, "late"}, {14, "working"}, {15, "asking"},
        {16, "held"}, {17, "reported"}, {18, "no_report"}, {19, "stopped"}};
    for (const auto &[id, phase] : expected) {
        const auto *task = team.task(id);
        QVERIFY2(task, qPrintable(QStringLiteral("task %1").arg(id)));
        const auto mark = TeamUi::phaseMark(team, *task, now);
        QCOMPARE(mark.phase, phase);
        QCOMPARE(mark.kind, phase);        // the glyph the desktop draws
        QVERIFY(!mark.tooltip.isEmpty());
        QVERIFY(mark.tone != Tone::Neutral);
        QVERIFY2(mark.tooltip != phase, qPrintable(mark.tooltip));
        QCOMPARE(TeamUi::kindOfTask(*task, taskStatus(*task)), phase);
    }
    // N4: hgs sends "phase": null for a finished task and for revoked or
    // refused attempts. Those keep their own mark and invent no phase icon.
    for (const auto &[id, mark] : QList<std::pair<int, QString>>{{20, "rejected"}, {21, "accepted"}}) {
        const auto *task = team.task(id);
        QVERIFY(task && task->last() && task->last()->phaseKnown);
        QVERIFY2(TeamUi::phaseMark(team, *task).phase.isEmpty(), qPrintable(QStringLiteral("task %1").arg(id)));
        QCOMPARE(TeamUi::kindOfTask(*task, taskStatus(*task)), mark);
        QCOMPARE(taskStatus(*task).kind, task->status);
    }
    const auto *retried = team.task(24);
    QVERIFY(retried && retried->last()->phaseKnown);
    QVERIFY2(TeamUi::phaseMark(team, *retried).phase.isEmpty(), "a revoked or refused attempt has no phase");
    QCOMPARE(TeamUi::kindOfTask(*retried, taskStatus(*retried)), QStringLiteral("pending"));
    QVERIFY(taskStatus(*retried).text.startsWith(QStringLiteral("Pending")));
    // The tooltips name the time and the next step where the spec asks for it.
    QVERIFY(TeamUi::phaseMark(team, *team.task(13), now).tooltip.contains("not started"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(13), now).tooltip.contains("terminal"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(14), now).tooltip.contains("Backend started"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(15), now).tooltip.contains("Must I keep the old field?"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(15), now).tooltip.contains("Answer"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(16), now).tooltip.contains("the member is busy"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(17), now).tooltip.contains("Report from Tester"));
    QVERIFY(TeamUi::phaseMark(team, *team.task(23), now).kind.isEmpty());   // no attempt, no phase icon
    // The graph paints the same icons and tooltips.
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    panel.resize(420, 1500); panel.show();
    workspace.resize(1600, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-2");
    workspace.showTeam("team-2");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseGraph"));
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
    QVERIFY(view);
    const auto node = [&](int id) -> const TeamUi::TaskView::Node * {
        for (const auto &n : view->nodes) if (n.id == id) return &n;
        return nullptr;
    };
    for (const auto &[id, phase] : expected) {
        const auto *found = node(id);
        QVERIFY2(found, qPrintable(QStringLiteral("node %1").arg(id)));
        QCOMPARE(found->kind, phase);
        QVERIFY(!found->tooltip.isEmpty());
    }
    // N4 on the canvas: the accepted and rejected tasks carry their own mark.
    QVERIFY(node(20) && node(20)->kind == QStringLiteral("rejected"));
    QVERIFY(node(21) && node(21)->kind == QStringLiteral("accepted"));
    QVERIFY(node(24) && node(24)->kind == QStringLiteral("pending"));
    // QA: an attempt that only left for the member reads as sending, not as
    // work; the icon and the tooltip say the same.
    const auto quiet = [](const Member &) { Live state; state.known = true; return state; };
    const auto sending = memberStatus(team, *team.member("backend"), quiet(*team.member("backend")));
    QCOMPARE(sending.kind, QStringLiteral("sent"));
    QVERIFY2(sending.text.startsWith(QStringLiteral("Sending")), qPrintable(sending.text));
    QVERIFY(sending.text.contains(QStringLiteral("#11")));
    QVERIFY(sending.text != QStringLiteral("Working on #11 Pending hand-off"));
    // The inspector's member row says the same (its mark carries the text).
    const auto marks = panel.findChildren<QWidget *>("teamSummaryMemberState");
    QVERIFY(!marks.isEmpty());
    bool sawSending = false;
    for (auto *mark : marks) if (mark->toolTip().contains(QStringLiteral("Sending #11"))) sawSending = true;
    QVERIFY(sawSending);
    // QA: the board keeps rejected and cancelled tasks out of Accepted; only an
    // accepted task sits there, the others in their own Decided column.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto columnOf = [&](int id) {
        for (auto *boardCard : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (boardCard->property("task").toInt() == id)
                for (QWidget *w = boardCard->parentWidget(); w; w = w->parentWidget())
                    if (w->objectName() == "teamBoardColumn")
                        return w->property("column").toString();
        return QString();
    };
    QCOMPARE(columnOf(21), QStringLiteral("accepted"));
    QCOMPARE(columnOf(20), QStringLiteral("decided"));
    QCOMPARE(columnOf(22), QStringLiteral("submitted"));
    // The card header carries the same icon and tooltip (A1).
    TeamTaskCard card;
    card.resize(440, 900); card.show();
    card.setTask(team, 17, live, true);
    auto *state = card.findChild<QWidget *>("teamTaskState");
    QVERIFY(state);
    QVERIFY(state->toolTip().contains("Report from Tester"));
    card.setTask(team, 15, live, true);
    QVERIFY(card.findChild<QWidget *>("teamTaskState")->toolTip().contains("Answer"));
    // N4: an accepted card invents no report tooltip.
    card.setTask(team, 21, live, true);
    auto *accepted = card.findChild<QWidget *>("teamTaskState");
    QVERIFY(accepted);
    QVERIFY(!accepted->toolTip().contains(QStringLiteral("Report from")));
    // A3: the open question is in the lead's inbox, and Answer opens its task.
    QTRY_VERIFY(panel.findChild<QPushButton *>("teamGoToTaskAnswer"));
    panel.findChild<QPushButton *>("teamGoToTaskAnswer")->click();
    QTRY_COMPARE(panel.openedTask(), 15);
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceAnswerQuestion"));
    workspace.findChild<QPushButton *>("teamWorkspaceAnswerQuestion")->click();
    QTRY_VERIFY(dynamic_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))
                && dynamic_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->task() == 15);
}

void TestTeamsUiMembers::taskCardAnswersQuestionsAndHolds()
{
    const auto team = parse(deliveryTeam());
    TeamTaskCard card;
    card.resize(440, 1000); card.show();
    QList<std::pair<QString, QJsonObject>> acted;
    card.act = [&](const QString &op, const QJsonObject &input) { acted.append({op, input}); };
    card.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    card.problemActions = [](const Member &, const Status &) { auto *w = new QWidget; w->setObjectName("problemActions"); w->setLayout(new QHBoxLayout); return w; };
    // A3: a question lists an Answer field; Send is one `reply`.
    card.setTask(team, 15, live, true);
    QVERIFY(card.findChild<QWidget *>("teamTaskQuestion"));
    auto *answer = card.findChild<QLineEdit *>("teamAnswerText");
    QVERIFY(answer);
    card.findChild<QPushButton *>("teamAnswerSend")->click();   // empty: nothing goes out
    QVERIFY(acted.isEmpty());
    answer->setText("Yes, keep it.");
    card.findChild<QPushButton *>("teamAnswerSend")->click();
    QCOMPARE(acted.size(), 1);
    QCOMPARE(acted.last().first, QStringLiteral("reply"));
    QCOMPARE(acted.last().second["question"].toString(), QStringLiteral("q1"));
    QCOMPARE(acted.last().second["text"].toString(), QStringLiteral("Yes, keep it."));
    shot(&card, "delivery-card-question");
    // A5: a held task releases; a pending one can be held with its reason.
    card.setTask(team, 16, live, true);
    QVERIFY(card.findChild<QPushButton *>("teamTaskRelease"));
    QVERIFY(!card.findChild<QPushButton *>("teamTaskHold"));
    card.findChild<QPushButton *>("teamTaskRelease")->click();
    QCOMPARE(acted.last().first, QStringLiteral("release"));
    QCOMPARE(acted.last().second["task"].toInt(), 16);
    card.setTask(team, 23, live, true);
    auto *hold = card.findChild<QPushButton *>("teamTaskHold");
    QVERIFY(hold);
    hold->click();
    QTRY_VERIFY(card.findChild<QFrame *>("teamHoldForm"));
    card.findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("the member is busy");
    card.findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acted.last().first, QStringLiteral("hold"));
    QCOMPARE(acted.last().second["task"].toInt(), 23);
    QCOMPARE(acted.last().second["reason"].toString(), QStringLiteral("the member is busy"));
    // A6: a replaced report keeps the captured text, folded; a captured
    // report says so.
    card.setTask(team, 17, live, true);
    QVERIFY(card.findChild<QWidget *>("teamReportReplaced"));
    QVERIFY(!card.findChild<QWidget *>("teamReportCaptured"));
    auto *toggle = card.findChild<QToolButton *>("teamReportReplacedToggle");
    auto *old = card.findChild<QLabel *>("teamReportReplacedText");
    QVERIFY(toggle && old && old->isHidden());
    toggle->click();
    QVERIFY(!old->isHidden());
    QVERIFY(old->text().contains("Building and running the tests"));
    shot(&card, "delivery-card-replaced");
    card.setTask(team, 22, live, true);
    QVERIFY(card.findChild<QWidget *>("teamReportCaptured"));
    // A5: Reject offers "Return later (hold)" and sends hold: true.
    card.setTask(team, 17, live, true);
    card.findChild<QPushButton *>("teamTaskReject")->click();
    QTRY_VERIFY(card.findChild<QFrame *>("teamRejectForm"));
    card.findChild<QCheckBox *>("teamRejectHold")->setChecked(true);
    shot(&card, "delivery-card-hold");
    card.findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Needs the new field");
    card.findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acted.last().first, QStringLiteral("reject"));
    QCOMPARE(acted.last().second["hold"].toBool(), true);
    QCOMPARE(acted.last().second["reason"].toString(), QStringLiteral("Needs the new field"));
    // A5: the held card with its Release button, and the light shots of the
    // question, replaced and held cards (QA later).
    card.setTask(team, 16, live, true);
    QVERIFY(card.findChild<QPushButton *>("teamTaskRelease"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&card, "delivery-card-release-dark");
        lightPalette();
        card.setTask(team, 15, live, false); shot(&card, "delivery-card-question-light");
        card.setTask(team, 17, live, false); shot(&card, "delivery-card-replaced-light");
        card.setTask(team, 16, live, false); shot(&card, "delivery-card-release-light");
        darkPalette();
        card.setTask(team, 17, live, true);
    }
}

void TestTeamsUiMembers::memberQueueListsCurrentAndReturned()
{
    const auto fixture = queueTeam();
    setTeam(fixture);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    panel.resize(420, 1500); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-3");
    workspace.showTeam("team-3");
    QTRY_COMPARE(panel.findChildren<QWidget *>("teamSummaryMember").size(), 3);
    const auto rows = [](QWidget *queue) {
        QList<int> ids;
        if (!queue) return ids;
        for (auto *row : queue->findChildren<QPushButton *>("teamMemberQueueRow")) ids << row->property("task").toInt();
        return ids;
    };
    const auto queueFor = [](QWidget *root, const QString &member) -> QWidget * {
        if (!root) return nullptr;
        for (auto *queue : root->findChildren<QWidget *>("teamMemberQueue"))
            if (queue->property("member") == member) return queue;
        return nullptr;
    };
    // The inspector: the open task, then the two returned ones. #6 was never
    // handed out and is in no queue.
    auto *summary = panel.findChild<QWidget *>("teamSummaryMembers");
    QVERIFY(summary);
    auto *backend = queueFor(summary, "backend");
    QVERIFY(backend);
    QCOMPARE(rows(backend), (QList<int>{1, 3, 4}));
    QVERIFY(!rows(backend).contains(6));
    const auto backendRows = backend->findChildren<QPushButton *>("teamMemberQueueRow");
    QVERIFY(backendRows.first()->property("current").toBool());
    QVERIFY(!backendRows.at(1)->property("current").toBool());
    QCOMPARE(backendRows.at(1)->property("phase").toString(), QStringLiteral("held"));
    QVERIFY(backendRows.at(1)->toolTip().contains("Held by Lead"));
    QVERIFY(backendRows.at(1)->toolTip().contains("wait for the owner"));
    // At most five rows, then "+N more": the tester has eight.
    auto *tester = queueFor(summary, "tester");
    QVERIFY(tester);
    QCOMPARE(rows(tester).size(), 5);
    auto *more = tester->findChild<QLabel *>("teamMemberQueueMore");
    QVERIFY(more);
    QCOMPARE(more->text(), QStringLiteral("+3 more"));
    // The workspace member card shows the same queue, folded behind "3 tasks"
    // until opened (U25), and opens its task.
    QSettings().remove("ui/collapsed/teamMemberTasks/team-3/backend");
    const auto backendCard = [&workspace]() -> QWidget * {
        for (auto *m : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (m->property("teamMember") == "backend") return m;
        return nullptr;
    };
    QTRY_VERIFY(backendCard() && backendCard()->findChild<QToolButton *>("teamMemberTasksToggle"));
    QVERIFY(!queueFor(backendCard(), "backend"));
    QCOMPARE(backendCard()->findChild<QToolButton *>("teamMemberTasksToggle")->text(), QStringLiteral("3 tasks"));
    backendCard()->findChild<QToolButton *>("teamMemberTasksToggle")->click();
    QTRY_VERIFY(backendCard() && queueFor(backendCard(), "backend"));
    auto *memberCard = backendCard();
    auto *queue = queueFor(memberCard, "backend");
    QVERIFY(queue);
    QCOMPARE(rows(queue), (QList<int>{1, 3, 4}));
    queue->findChildren<QPushButton *>("teamMemberQueueRow").at(1)->click();
    QTRY_VERIFY(dynamic_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))
                && dynamic_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->task() == 3);
    // The inspector's row opens the same task over the tab (re-found: the tab
    // may have rendered again while the workspace opened its drawer).
    auto *fresh = queueFor(panel.findChild<QWidget *>("teamSummaryMembers"), "backend");
    QVERIFY(fresh);
    const auto freshRows = fresh->findChildren<QPushButton *>("teamMemberQueueRow");
    QVERIFY(freshRows.size() > 2);
    freshRows.at(2)->click();
    QTRY_COMPARE(panel.openedTask(), 4);
    shot(&panel, "delivery-queue-420");
}

// N5 (#359): a GUI started from an agent's shell (a dsh session included)
// runs every hgs as the person: no hgs session, run, dsh conversation or
// launch marker reaches it, so `resolve_actor` sees the owner, not an agent.
void TestTeamsUiMembers::ownerActionsCarryNoAgentMarkers()
{
    const char *markers[] = {"HGS_SESSION", "HGS_RUN_ID", "DSH_SESSION_ID", "HGS_LAUNCH_ID", "HGS_FRESH"};
    const auto restore = qScopeGuard([&] { for (const char *marker : markers) qunsetenv(marker); });
    for (const char *marker : markers) qputenv(marker, "from-the-agent-shell");
    QFile::remove(script() + ".env");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    bool answered = false;
    controller.call(QStringLiteral("pause"), {{"team", "team-1"}, {"by", "human"}}, [&](bool, const QJsonObject &, const QString &) { answered = true; });
    QTRY_VERIFY(answered);
    QFile file(script() + ".env"); QVERIFY(file.open(QIODevice::ReadOnly));
    int seen = 0;
    for (const auto &line : file.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).object(); if (call.isEmpty()) continue;
        ++seen;
        QVERIFY2(call["markers"].toArray().isEmpty(), line.constData());
    }
    QVERIFY(seen >= 1);
}

void TestTeamsUiMembers::leftoverMembersAreShownApart()
{
    auto done = phasedTeam();
    done["status"] = "done";
    done["finish"] = QJsonObject{{"by", "Lead"}, {"summary", "All phases are in."}, {"at", 1800}};
    done["left_running"] = QJsonArray{
        QJsonObject{{"member", "backend"}, {"role", "Backend"},
                    {"session", QJsonObject{{"host", ""}, {"name", "codex/checkout/backend"}, {"run_id", "r1"}, {"conversation_id", "c1"}}}},
        QJsonObject{{"member", "tester"}, {"role", "Tester"},
                    {"session", QJsonObject{{"host", "msi-old"}, {"name", "kimi/checkout/tester"}}}}};
    setTeam(done);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    QStringList stopped, taken;
    panel.stopSession = [&](const QString &host, const QString &session) { stopped << host + '\n' + session; };
    panel.takeIntoTeam = [&](const QString &host, const QString &session) { taken << host + '\n' + session; };
    panel.resize(420, 1400); panel.show();
    workspace.resize(1100, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(panel.findChild<QWidget *>("teamLeftover"));
    // F7: the leftovers are no longer grouped under the lead as active members.
    QCOMPARE(panel.findChildren<QWidget *>("teamSummaryMember").size(), 1);
    auto rows = panel.findChildren<QFrame *>("teamLeftoverMember");
    QCOMPARE(rows.size(), 2);
    QCOMPARE(rows[0]->property("member").toString(), QStringLiteral("backend"));
    QCOMPARE(rows[0]->property("session").toString(), QStringLiteral("codex/checkout/backend"));
    QVERIFY(panel.findChild<QLabel *>("teamLeftoverTitle")->text().contains(QStringLiteral("Checkout redesign")));
    shot(&panel, "leftover-members-dark");
    // N7: the Team tab promises no action of its own. Stop only asks the window,
    // which shows the session's own stop dialog with the real action (stop and
    // archive, terminate, or forget the binding) once its details are loaded.
    auto *stopButton = panel.findChild<QPushButton *>("teamLeftoverStop");
    QVERIFY(stopButton);
    QCOMPARE(stopButton->text(), QStringLiteral("Stop…"));
    QVERIFY(stopButton->toolTip().contains(QStringLiteral("session's own stop confirmation")));
    stopButton->click();
    QTest::qWait(100);
    QCOMPARE(stopped.size(), 1);   // the window gets the identity and decides
    QCOMPARE(stopped.first(), QStringLiteral("\ncodex/checkout/backend"));   // this computer
    QVERIFY(!panel.findChild<QWidget *>("teamStopLeftoverConfirm"));         // no invented dialog
    // Take into a new team names the same session.
    panel.findChild<QPushButton *>("teamLeftoverTake")->click();
    QCOMPARE(taken.size(), 1);
    QCOMPARE(taken.first(), QStringLiteral("\ncodex/checkout/backend"));
    // The workspace shows the same block, apart from its member cards.
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamLeftover"));
    QCOMPARE(workspace.findChildren<QFrame *>("teamWorkspaceMember").size(), 1);
    QCOMPARE(workspace.findChildren<QFrame *>("teamLeftoverMember").size(), 2);
    shot(&workspace, "leftover-workspace-dark");
    // Light theme.
    lightPalette();
    panel.setTheme(false); workspace.setTheme(false);
    panel.render(); workspace.render();
    QTRY_VERIFY(panel.findChild<QWidget *>("teamLeftover"));
    shot(&panel, "leftover-members-light");
    darkPalette();
    panel.setTheme(true); workspace.setTheme(true);
}

void TestTeamsUiMembers::memberEditorPicksAnExistingSession()
{
    setTeam(team("planning"));
    // Exercise the list-before-status ordering rather than racing the two
    // children: the first member menu captures a summary, not an editable plan.
    QFile hold(script() + ".statushold"); QVERIFY(hold.open(QIODevice::WriteOnly)); hold.close();
    const auto release = qScopeGuard([&] { QFile::remove(hold.fileName()); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.sessionChoices = [] {
        QList<Teams::SessionChoice> choices;
        Teams::SessionChoice free;
        free.name = QStringLiteral("claude/checkout/other"); free.runId = QStringLiteral("run-1");
        free.conversationId = QStringLiteral("conv-1"); free.agent = QStringLiteral("claude");
        free.directory = QStringLiteral("/work/checkout");
        choices.append(free);
        // Already in this team: never offered.
        Teams::SessionChoice taken;
        taken.name = QStringLiteral("claude/checkout/lead"); taken.agent = QStringLiteral("claude");
        choices.append(taken);
        return choices;
    };
    panel.resize(420, 1400); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    QVERIFY(!controller.watched() || !controller.watched()->full);
    QVERIFY(calls(script(), "plan").isEmpty());
    QVERIFY(QFile::remove(hold.fileName()));
    WAIT_VERIFY(controller.watched() && controller.watched()->full);
    // The full status schedules a render. Its validation box proves the new
    // member menu captures that full plan before we take its action pointer.
    WAIT_VERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));
    // The tester's row offers the free session only.
    QWidget *tester = nullptr;
    for (auto *card : panel.findChildren<QFrame *>("teamCard"))
        if (card->property("teamMember") == "tester") tester = card;
    QVERIFY(tester);
    auto *more = tester->findChild<QToolButton *>("teamMemberMore");
    QVERIFY(more);
    QMenu *pick = nullptr;
    for (auto *action : more->menu()->actions())
        if (action->menu() && action->text().contains(QStringLiteral("existing session"))) pick = action->menu();
    QVERIFY(pick);
    QCOMPARE(pick->actions().size(), 1);
    QVERIFY(pick->actions().first()->text().contains(QStringLiteral("claude/checkout/other")));
    // Picking it sends the session identity with the plan.
    pick->actions().first()->trigger();
    // Follow the actual child request; a loaded runner can take longer than
    // QTRY's default while the plan write is still in flight.
    WAIT_COMPARE(calls(script(), "plan").size(), 1);
    const auto plan = calls(script(), "plan").first();
    QJsonObject adopted;
    for (const auto &value : plan["members"].toArray())
        if (value.toObject()["id"].toString() == "tester") adopted = value.toObject();
    QVERIFY(!adopted.isEmpty());
    const auto session = adopted["session"].toObject();
    QCOMPARE(session["name"].toString(), QStringLiteral("claude/checkout/other"));
    QCOMPARE(session["run_id"].toString(), QStringLiteral("run-1"));
    QCOMPARE(session["conversation_id"].toString(), QStringLiteral("conv-1"));
    QCOMPARE(adopted["agent"].toString(), QStringLiteral("claude"));   // the session's own agent
    QCOMPARE(adopted["folder"].toString(), QStringLiteral("shared"));  // its folder is the session's
    QCOMPARE(adopted["directory"].toString(), QStringLiteral("/work/checkout"));
    QCOMPARE(plan["by"].toString(), QStringLiteral("human"));
    // The editor shows the adopted session on the member's row (shots, dark and
    // light): its own name replaces "Created on approval".
    auto withSession = team("planning");
    auto sessionMembers = withSession["members"].toArray();
    auto testerRow = sessionMembers[2].toObject();
    testerRow["session"] = QJsonObject{{"host", ""}, {"name", "claude/checkout/other"}, {"run_id", "run-1"}, {"conversation_id", "conv-1"}};
    sessionMembers[2] = testerRow; withSession["members"] = sessionMembers;
    setTeam(withSession);
    QTRY_VERIFY(([&] {
        controller.refresh();
        for (auto *card : panel.findChildren<QFrame *>("teamCard"))
            if (card->property("teamMember") == "tester") {
                auto *where = card->findChild<QLabel *>("teamMemberWhere");
                return where && where->text().contains(QStringLiteral("claude/checkout/other"));
            }
        return false;
    })());
    shot(&panel, "member-adopted-dark");
    lightPalette();
    panel.setTheme(false); panel.render();
    QTRY_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    shot(&panel, "member-adopted-light");
    darkPalette();
    panel.setTheme(true);
}

// #465 C1: "Role prompt…" opens its dialog from inside the member menu's
// activation. The menu's hold lasts until that activation has returned, so
// neither the hold's own release nor a poll re-renders the panel (deleting
// the ⋯ button and its menu) while the dialog is open.
void TestTeamsUiMembers::rolePromptFromTheMemberMenuKeepsItsButton()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(420, 1400); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    WAIT_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    const auto testerMore = [&]() -> QToolButton * {
        for (auto *card : panel.findChildren<QFrame *>("teamCard"))
            if (card->property("teamMember") == "tester") return card->findChild<QToolButton *>("teamMemberMore");
        return nullptr;
    };
    WAIT_VERIFY(testerMore());
    QPointer<QToolButton> more = testerMore();
    QPointer<QMenu> menu = more->menu();
    auto changed = team("planning");
    changed["revision"] = changed["revision"].toInt() + 1;
    bool checked = false;
    std::function<void(int)> lookDialog = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *dialog = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!dialog) { if (tries > 0) lookDialog(TestWait::busy() ? tries : tries - 1); return; }
            QPointer<QInputDialog> open = dialog;
            // A failed check closes the dialog, so the test ends instead of waiting on it.
            const auto close = qScopeGuard([open] { if (open) open->reject(); });
            // The menu's own release comes due inside the dialog's loop: no render.
            WAIT_SETTLED();
            QVERIFY(panel.confirming());
            QVERIFY(more && menu);
            // A poll under the dialog: still no render.
            setTeam(changed);
            controller.refresh();
            WAIT_COMPARE(controller.team("team-1")->revision, changed["revision"].toInt());
            WAIT_SETTLED();
            QVERIFY(more && menu);
            QVERIFY(more == testerMore());
            QVERIFY(open && open->isVisible());
            checked = true;
        });
    };
    std::function<void(int)> lookMenu = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *popup = qobject_cast<QMenu *>(QApplication::activePopupWidget());
            if (!popup) { if (tries > 0) lookMenu(TestWait::busy() ? tries : tries - 1); return; }
            QAction *prompt = nullptr;
            for (auto *action : popup->actions()) if (action->text().startsWith(QStringLiteral("Role prompt"))) prompt = action;
            QVERIFY(prompt);
            popup->setActiveAction(prompt);
            lookDialog(300);
            // The owner's choice: the menu hides, then the action opens its dialog.
            QTest::keyClick(popup, Qt::Key_Return);
        });
    };
    lookMenu(300);
    more->click();
    WAIT_VERIFY(checked);
    // Once everything has returned the panel catches up with the poll.
    WAIT_VERIFY(!panel.confirming());
    WAIT_VERIFY(testerMore() && testerMore() != more.data());
    QVERIFY(calls(script(), "plan").isEmpty());
}

void TestTeamsUiMembers::pinMarkAndFinishedTeamShowWhoFinished()
{
    auto t = phasedTeam();
    auto tasks = t["tasks"].toArray();
    auto pinned = tasks[1].toObject(); pinned["pinned"] = true; tasks[1] = pinned;   // #2 Checkout design
    t["tasks"] = tasks;
    const auto parsed = parse(t);
    TeamTaskCard card;
    card.resize(420, 900); card.show();
    card.setTask(parsed, 2, live, true);
    auto *pin = card.findChild<QLabel *>("teamTaskPinned");
    QVERIFY(pin);
    QCOMPARE(pin->toolTip(), QStringLiteral("Pinned: stays with Backend"));
    shot(&card, "task-card-pinned");
    // A Done team shows who finished it and the summary.
    t["status"] = "done";
    t["waiting_for_lead"] = false;
    t["pending_phase"] = QJsonValue();
    t["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    t["finish"] = QJsonObject{{"by", "Lead"}, {"summary", "Phases 1–2 are delivered; the retest is #11."}, {"at", 2000.0}};
    auto doneTasks = t["tasks"].toArray();
    for (int i = 0; i < doneTasks.size(); ++i) { auto task = doneTasks[i].toObject(); task["status"] = "accepted"; doneTasks[i] = task; }
    t["tasks"] = doneTasks;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.setLive(live); panel.resize(420, 1100); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    // A request may already be in flight; keep polling until the new team
    // shows, as every poll rebuilds the panel.
    const auto refreshed = [&](const QJsonObject &value, const std::function<bool()> &shown) {
        setTeam(value);
        QTRY_VERIFY(([&] { controller.refresh(); return shown(); })());
    };
    QTRY_VERIFY(([&] { controller.refresh(); return panel.findChild<QLabel *>("teamFinishBy") != nullptr; })());
    QCOMPARE(panel.findChild<QLabel *>("teamFinishBy")->text(), QStringLiteral("Finished by Lead"));
    QCOMPARE(panel.findChild<QLabel *>("teamFinishSummaryText")->text(), QStringLiteral("Phases 1–2 are delivered; the retest is #11."));
    QVERIFY(!panel.findChild<QPushButton *>("teamFinish"));   // Done is final
    shot(&panel, "team-done-finish");
    // The inspector repeats both header chips.
    refreshed(phasedTeam(), [&] { return panel.findChild<QWidget *>("teamPendingPhase") != nullptr; });
    QVERIFY(!panel.findChild<QWidget *>("teamWaitingForLead"));
    auto waiting = phasedTeam();
    auto waitingTasks = waiting["tasks"].toArray();
    for (int i = 0; i < waitingTasks.size(); ++i) { auto task = waitingTasks[i].toObject(); task["status"] = "accepted"; waitingTasks[i] = task; }
    waiting["tasks"] = waitingTasks;
    waiting["pending_phase"] = QJsonValue();
    waiting["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    waiting["waiting_for_lead"] = true;
    refreshed(waiting, [&] { return panel.findChild<QWidget *>("teamWaitingForLead") != nullptr; });
    QVERIFY(panel.findChild<QPushButton *>("teamFinish"));
}

void TestTeamsUiMembers::deliveryShots()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) QSKIP("Set ZERUS_TEAM_SHOTS to render the delivery shots.");
    const QPalette dark = QApplication::palette();
    setTeam(deliveryTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    panel.resize(420, 1500); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-2");
    workspace.showTeam("team-2");
    QTRY_COMPARE(panel.findChildren<QWidget *>("teamSummaryMember").size(), 3);
    // Dark: the inspector with every member's queue, the board and a card.
    shot(&panel, "delivery-members-dark");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    shot(&workspace, "delivery-board-dark");
    const auto team = parse(deliveryTeam());
    TeamTaskCard card;
    card.resize(440, 1000); card.show();
    card.setTask(team, 17, live, true);
    shot(&card, "delivery-card-dark");
    // Light: the same surfaces in the light skin.
    QPalette light;
    light.setColor(QPalette::Window, QColor("#f5f7f9")); light.setColor(QPalette::WindowText, QColor("#1a2733"));
    light.setColor(QPalette::Base, QColor("#ffffff")); light.setColor(QPalette::Button, QColor("#edf2f5"));
    light.setColor(QPalette::ButtonText, QColor("#1a2733")); light.setColor(QPalette::Text, QColor("#1a2733"));
    light.setColor(QPalette::PlaceholderText, QColor("#627082"));
    QApplication::setPalette(light);
    panel.setTheme(false); workspace.setTheme(false);
    panel.render(); workspace.render();
    controller.refresh();
    QTRY_COMPARE(panel.findChildren<QWidget *>("teamSummaryMember").size(), 3);
    shot(&panel, "delivery-members-light");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    shot(&workspace, "delivery-board-light");
    card.setTask(team, 17, live, false);
    shot(&card, "delivery-card-light");
    QApplication::setPalette(dark);
    panel.setTheme(true); workspace.setTheme(true);
}

void TestTeamsUiMembers::swapMemberAsksFirstAndListsTheOldSession()
{
    setTeam(proposalsTeam(QDateTime::currentSecsSinceEpoch() + 3600));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.sessionChoices = [] { return QList<SessionChoice>{{QString(), "claude/checkout/spare", "run-9", "conv-9", "claude", "/work/checkout"},
                                                            {QString(), "claude/checkout/lead", "run-1", "conv-1", "claude", "/work/checkout"}}; };
    QString opened;
    TeamWorkspace workspace(&controller, &panel);
    workspace.openSession = [&](const QString &, const QString &session) { opened = session; };
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The old session after an earlier swap, with Open.
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceSwapped"));
    auto *old = workspace.findChild<QWidget *>("teamSwappedSession");
    QVERIFY(old);
    QCOMPARE(old->property("session").toString(), QStringLiteral("dsh/checkout/backend-old"));
    old->findChild<QPushButton *>("teamSwappedOpen")->click();
    QCOMPARE(opened, QStringLiteral("dsh/checkout/backend-old"));
    shot(&workspace, "h6-swapped-sessions");
    // The member menu opens the swap dialog.
    const auto openSwap = [&](const QString &member) {
        QToolButton *more = nullptr;
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == member) more = card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
        QVERIFY(more);
        chooseMenuAction("teamWorkspaceMemberSwap");
        more->click();
    };
    openSwap("backend");
    QDialog *dialog = nullptr;
    QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    // This team's own sessions are not offered.
    auto *session = dialog->findChild<QComboBox *>("teamSwapSession");
    QCOMPARE(session->count(), 1);
    QCOMPARE(session->itemData(0).toJsonObject()["name"].toString(), QStringLiteral("claude/checkout/spare"));
    auto *agent = dialog->findChild<QComboBox *>("teamSwapAgent");
    agent->setCurrentIndex(agent->findData("codex"));
    dialog->findChild<QComboBox *>("teamSwapModel")->setCurrentText("gpt-5.5");
    shot(dialog, "h6-swap-dialog");
    // Cancel at the confirmation sends nothing and keeps the dialog.
    armConfirm(&panel, true);
    dialog->findChild<QPushButton *>("teamSwapApply")->click();
    QTest::qWait(100);
    QVERIFY(calls(script(), "swap-member").isEmpty());
    QVERIFY(workspace.findChild<QDialog *>("teamSwapDialog"));
    armConfirm(&panel, false);
    dialog->findChild<QPushButton *>("teamSwapApply")->click();
    QTRY_COMPARE(calls(script(), "swap-member").size(), 1);
    auto input = calls(script(), "swap-member").first();
    QCOMPARE(input["member"].toString(), QStringLiteral("backend"));
    QCOMPARE(input["agent"].toString(), QStringLiteral("codex"));
    QCOMPARE(input["model"].toString(), QStringLiteral("gpt-5.5"));
    QVERIFY(!input.contains("session"));
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamSwapDialog"));
    // The lead too, to an existing session.
    openSwap("lead");
    QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    dialog->findChild<QRadioButton *>("teamSwapExisting")->setChecked(true);
    armConfirm(&panel, false);
    dialog->findChild<QPushButton *>("teamSwapApply")->click();
    QTRY_COMPARE(calls(script(), "swap-member").size(), 2);
    input = calls(script(), "swap-member").last();
    QCOMPARE(input["member"].toString(), QStringLiteral("lead"));
    QCOMPARE(input["session"].toObject()["name"].toString(), QStringLiteral("claude/checkout/spare"));
    QCOMPARE(input["session"].toObject()["run_id"].toString(), QStringLiteral("run-9"));
    QVERIFY(!input.contains("agent"));
}

// H6: Swap from the member menu, a new session or an existing one, confirmed;
// the old session stays listed for the owner to stop.
// #465 (the lead's addition): the workspace draws a team from the list before
// its `team status` read lands. Until then the member "…" opened nothing, so
// the swap dialog never came on a loaded machine. Every handler now looks the
// team up as the render does.
void TestTeamsUiMembers::memberMenuWorksBeforeTheStatusReadLands()
{
    setTeam(proposalsTeam(QDateTime::currentSecsSinceEpoch() + 3600));
    { QFile hold(script() + ".statushold"); QVERIFY(hold.open(QIODevice::WriteOnly)); }
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QToolButton *more = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == "backend" && card->isVisibleTo(&workspace)) more = card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
        return more != nullptr;
    })());
    QVERIFY(!controller.watched());
    chooseMenuAction("teamWorkspaceMemberSwap");
    more->click();
    QDialog *dialog = nullptr;
    QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    QVERIFY(!controller.watched());
    dialog->close();
    QFile::remove(script() + ".statushold");
    QTRY_VERIFY(controller.watched());
}

// H1 and H3: a member at a usage limit shows its reset time, the team toggles
// resume after limits (the settings go back whole); members on one DeepSeek host
// say so with the host in the tooltip.
void TestTeamsUiMembers::limitedAndSharedHostMembersAreMarked()
{
    const double resetsAt = QDateTime::currentSecsSinceEpoch() + 3600;
    setTeam(proposalsTeam(resetsAt));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceMemberLimited"));
    auto *limited = workspace.findChild<QWidget *>("teamWorkspaceMemberLimited");
    const QString reset = QDateTime::fromSecsSinceEpoch(qint64(resetsAt)).toString(QStringLiteral("HH:mm"));
    QCOMPARE(limited->property("resetsAt").toDouble(), resetsAt);
    QVERIFY(limited->toolTip().contains("Usage limit reached"));
    QVERIFY(limited->toolTip().contains("Resume after limits is off"));
    QVERIFY(resetText(resetsAt).contains(reset) || resetText(resetsAt).contains(QStringLiteral("resets")));
    // Two members share the DeepSeek host; the tooltip names the host and both.
    const auto shared = workspace.findChildren<QWidget *>("teamWorkspaceMemberSharedHost");
    QCOMPARE(shared.size(), 2);
    QVERIFY(shared.first()->toolTip().contains("4242"));
    QVERIFY(shared.first()->toolTip().contains("Backend") && shared.first()->toolTip().contains("Tester"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "h1-h3-member-marks");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "h1-h3-member-marks-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
        workspace.render();
    }
    // Resume after limits: the team setting. N33 (#548): only its own field
    // goes; the server merges it, so the lead's newer settings stay.
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits"));
    auto *toggle = workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits");
    QVERIFY(toggle->isCheckable() && !toggle->isChecked());
    toggle->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 1);
    const auto settings = calls(script(), "settings").first()["settings"].toObject();
    QVERIFY(settings["resume_after_limits"].toBool());
    QCOMPARE(settings.keys(), QStringList{QStringLiteral("resume_after_limits")});
    // With it on and the member waiting for its reset, the mark says so.
    auto on = proposalsTeam(resetsAt);
    auto onSettings = on["settings"].toObject(); onSettings["resume_after_limits"] = true; on["settings"] = onSettings;
    auto members = on["members"].toArray(); auto backend = members[1].toObject(); backend["limit_resume"] = true; members[1] = backend; on["members"] = members;
    setTeam(on);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *mark = workspace.findChild<QWidget *>("teamWorkspaceMemberLimited");
        return mark && mark->toolTip().contains("Resume after limits is on");
    })());
    QVERIFY(workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits")->isChecked());
    // Without a reset time the mark says so and the inbox has it.
    auto unknown = proposalsTeam(resetsAt);
    members = unknown["members"].toArray(); backend = members[1].toObject();
    backend["limited"] = QJsonObject{{"at", 100.0}, {"detail", "Usage limit reached"}}; members[1] = backend; unknown["members"] = members;
    setTeam(unknown);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *mark = workspace.findChild<QWidget *>("teamWorkspaceMemberLimited");
        return mark && mark->toolTip().contains("no reset time");
    })());
}

void TestTeamsUiMembers::reportPreviewsAndFindings()
{
    QTemporaryDir work;
    QVERIFY(work.isValid());
    QImage image(8, 8, QImage::Format_RGB32);
    image.fill(Qt::red);
    QVERIFY(image.save(work.filePath("shot.png")));
    QFile page(work.filePath("page.html"));
    QVERIFY(page.open(QIODevice::WriteOnly));
    page.write("<html><body><b>HTML evidence</b></body></html>");
    page.close();

    auto t = team();
    t["folder"] = work.path();
    t["settings"] = QJsonObject{{"evidence_folders", QJsonArray{work.path()}}};
    auto tasks = t["tasks"].toArray();
    auto task = tasks[1].toObject();
    auto attempts = task["attempts"].toArray();
    auto attempt = attempts[0].toObject();
    attempt["report"] = QJsonObject{
        {"text", "Form ready"}, {"at", 1300.0},
        {"structured", QJsonObject{
             {"summary", "Form ready and checked"}, {"commit", "abc1234"},
             {"criteria", QJsonArray{QJsonObject{{"n", 1}, {"result", "pass"}, {"evidence", QJsonArray{0}}, {"note", "Validation works"}}}},
             {"evidence", QJsonArray{
                  QJsonObject{{"kind", "command"}, {"caption", "tests"}, {"command", "cargo test"}, {"exit", 0}, {"tail", "ok"}},
                  QJsonObject{{"kind", "image"}, {"caption", "Screenshot"}, {"path", work.filePath("shot.png")}},
                  QJsonObject{{"kind", "html"}, {"caption", "Report"}, {"path", work.filePath("page.html")}},
                  QJsonObject{{"kind", "url"}, {"caption", "PR"}, {"url", "https://example.com/pr/1"}}}},
             {"limits", QJsonArray{"Windows not checked"}}}}};
    attempts[0] = attempt;
    task["attempts"] = attempts;
    tasks[1] = task;
    t["tasks"] = tasks;
    t["findings"] = QJsonObject{
        {"items", QJsonArray{
             QJsonObject{{"id", "f1"}, {"severity", "critical"}, {"title", "Cache race"}, {"scenario", "Two writers"},
                         {"file", "src/cache.rs"}, {"line", 42}, {"state", "open"}, {"from_task", "2"}},
             QJsonObject{{"id", "f2"}, {"severity", "later"}, {"title", "Docs"}, {"state", "deferred"}, {"note", "after v2"}},
             // N17, N18: a replan keeps its proposal, fix_phase its phase.
             QJsonObject{{"id", "f3"}, {"severity", "normal"}, {"title", "Slow save"}, {"scenario", "Big carts"}, {"state", "replan"},
                         {"proposal", "Split the save into two steps"}},
             QJsonObject{{"id", "f4"}, {"severity", "normal"}, {"title", "Copy"}, {"scenario", "Labels"}, {"state", "fix_phase"}, {"phase", 3}}}},
        {"next_cursor", ""}};
    t["attention"] = QJsonArray{QJsonObject{{"kind", "replan"}, {"finding", "f3"}, {"proposal", "Split the save into two steps"},
                                            {"summary", "Replan proposed for f3: Split the save into two steps"}}};
    t["findings_summary"] = QJsonObject{{"open", QJsonObject{{"critical", 1}, {"normal", 0}, {"later", 0}}}, {"deferred", 1}};
    setTeam(t);

    const auto parsed = Teams::parse(t);
    const auto live = [](const Member &) { Live state; state.known = true; return state; };
    // D1: the structured report renders criteria, evidence with previews and
    // its limits.
    TeamTaskCard card;
    card.setTask(parsed, 2, live, true);
    card.resize(760, 1400);
    card.show();
    QTRY_VERIFY(card.findChild<QLabel *>("teamReportSummary"));
    QVERIFY(card.findChild<QWidget *>("teamReportCriteria"));
    // #744: the evidence files are checked and read by PathProbe's worker.
    QTRY_VERIFY2(card.findChild<QLabel *>("teamReportEvidenceImage"), "the image evidence previews");
    QTRY_VERIFY2(card.findChild<QTextBrowser *>("teamReportEvidenceHtml"), "the HTML evidence previews");
    QVERIFY(card.findChild<QLabel *>("teamReportLimits"));
    shot(&card, "report-structured");

    // D2: the header counts findings by severity, and the list offers every
    // action.
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 900);
    workspace.show();
    controller.refresh();
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamBarFindings"));
    shot(&workspace, "findings-header");
    auto *pill = dynamic_cast<TeamUi::Pill *>(workspace.findChild<QWidget *>("teamBarFindings"));
    QVERIFY(pill);
    QVERIFY2(pill->toolTip().contains("1 critical") && pill->toolTip().contains("1 deferred"), qPrintable(pill->toolTip()));
    workspace.findChild<QPushButton *>("teamWorkspaceFindings")->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamFindingsDialog"));
    auto *dialog = workspace.findChild<QWidget *>("teamFindingsDialog");
    for (const auto &action : {"reopen", "urgent", "replan", "defer", "fixed", "dropped"})
        QVERIFY2(dialog->findChild<QPushButton *>(QStringLiteral("teamFindingAction_%1").arg(action)), action);
    // N17: the proposal shows on its finding and in the attention strip.
    QStringList proposals;
    for (auto *found : dialog->findChildren<QLabel *>("teamFindingProposal")) proposals << found->text();
    QCOMPARE(proposals, QStringList{QStringLiteral("Proposal: Split the save into two steps")});
    bool phaseShown = false;
    for (auto *found : dialog->findChildren<QLabel *>()) phaseShown = phaseShown || found->text() == QStringLiteral("state: fix_phase · phase 3");
    QVERIFY(phaseShown);
    const auto replanLine = [&workspace]() -> QLabel * {
        for (auto *line : workspace.findChildren<QLabel *>("teamAttentionLine")) if (line->property("kind") == "replan") return line;
        return nullptr;
    };
    QTRY_VERIFY(replanLine());
    QVERIFY2(replanLine()->toolTip().contains("Replan proposed for f3: Split the save into two steps"), qPrintable(replanLine()->toolTip()));
    shot(dialog, "findings-dialog");
    // N17: Replan asks for the proposal and sends it; without one nothing goes.
    QPushButton *replan = nullptr;
    for (auto *row : dialog->findChildren<QWidget *>("teamFindingRow"))
        if (!replan && row->findChild<QLabel *>() && row->findChild<QLabel *>()->text().contains("Cache race"))
            replan = row->findChild<QPushButton *>("teamFindingAction_replan");
    QVERIFY(replan);
    const auto answer = [](const QString &text) {
        auto *timer = new QTimer(qApp); timer->setInterval(10);
        QObject::connect(timer, &QTimer::timeout, qApp, [timer, text] {
            auto *input = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!input) return;
            timer->stop(); timer->deleteLater();
            if (text.isEmpty()) { input->reject(); return; }
            input->setTextValue(text);
            input->accept();
        });
        timer->start();
    };
    answer(QString());
    replan->click();
    QTest::qWait(100);
    QVERIFY(calls(script(), "finding").isEmpty());
    answer(QStringLiteral("Split the save into two steps"));
    replan->click();
    QTRY_COMPARE(calls(script(), "finding").size(), 1);
    const auto sent = calls(script(), "finding").first();
    QCOMPARE(sent["finding"].toString(), QStringLiteral("f1"));
    QCOMPARE(sent["action"].toString(), QStringLiteral("replan"));
    QCOMPARE(sent["proposal"].toString(), QStringLiteral("Split the save into two steps"));
}

void TestTeamsUiMembers::hierarchyInTeamViewsAndMemberCards()
{
    auto t = team();
    t["parent"] = QJsonObject{{"team", "parent-team"}, {"task", 3}, {"lead", "lead"}, {"member", "backend"}};
    t["children"] = QJsonArray{QJsonObject{{"team", "child-team"}, {"name", "Child"}, {"status", "running"}, {"done", 1}, {"total", 3}}};
    auto tasks = t["tasks"].toArray();
    auto delegated = tasks[1].toObject();
    delegated["subteam"] = QJsonObject{{"team", "child-team"}, {"status", "running"}, {"done", 1}, {"total", 3}};
    tasks[1] = delegated;
    t["tasks"] = tasks;
    setTeam(t);
    const auto parsed = Teams::parse(t);
    const auto live = [](const Member &) { Live state; state.known = true; return state; };
    // G1 on the card: the sub-team with its rolled-up count and a way in.
    TeamTaskCard card;
    card.setTask(parsed, 2, live, true);
    card.resize(760, 1000); card.show();
    QTRY_VERIFY(card.findChild<QWidget *>("teamTaskSubteam"));
    QVERIFY(card.findChild<QPushButton *>("teamTaskOpenSubteam"));
    shot(&card, "hierarchy-subteam");
    card.setTask(parsed, 3, live, true);
    QTRY_VERIFY(card.findChild<QPushButton *>("teamTaskDelegate"));
    shot(&card, "hierarchy-delegate");

    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 900); workspace.show();
    controller.refresh(); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBreadcrumb"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceBreadcrumb")->toolTip().contains("task #3"));
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
    QVERIFY(view);
    const auto *node = [&]() -> const TeamUi::TaskView::Node * {
        for (const auto &n : view->nodes) if (n.id == 2) return &n;
        return nullptr;
    }();
    QVERIFY(node);
    QCOMPARE(node->kind, QStringLiteral("subteam"));
    QCOMPARE(node->subteam, QStringLiteral("child-team"));
    // G1: the tree impact counts sub-teams and their sessions; Cancel asks
    // whether the sub-team stops.
    auto child = t;
    child["id"] = "child-team";
    child["parent"] = QJsonObject{{"team", "team-1"}, {"task", 2}};
    child["children"] = QJsonArray();
    auto childMembers = child["members"].toArray();
    if (!childMembers.isEmpty()) {
        auto member = childMembers[0].toObject();
        member["session"] = QJsonObject{{"host", ""}, {"name", "claude/child/lead"}};
        childMembers[0] = member;
    }
    child["members"] = childMembers;
    const auto parsedChild = Teams::parse(child);
    QCOMPARE(TeamUi::subtreeImpact(parsed, {parsed, parsedChild}), qMakePair(1, 3));
    QTimer::singleShot(50, [] {
        for (auto *widget : QApplication::topLevelWidgets())
            if (auto *box = qobject_cast<QMessageBox *>(widget))
                for (auto *button : box->buttons())
                    if (button->text().contains("Stop sub-team")) { button->click(); return; }
    });
    QCOMPARE(TeamUi::subteamChoice(&card, parsed, 2, "Cancel"), QStringLiteral("stop"));
    // The member card names its current task's number.
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceMemberList"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "hierarchy-team-view");
        // The same hierarchy in the light theme.
        lightPalette();
        workspace.setTheme(false);
        card.setTask(parsed, 2, live, false);
        QTest::qWait(50);
        lightShot(&workspace, "hierarchy-team-view-light");
        lightShot(&card, "hierarchy-subteam-light");
    }
}

void TestTeamsUiMembers::workspaceAddsAMember()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    workspace.resize(1300, 820); workspace.show();
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceAddMember"));
    workspace.findChild<QPushButton *>("teamWorkspaceAddMember")->click();
    auto *popover = workspace.findChild<QFrame *>("teamAddMember");
    QVERIFY(popover && popover->isVisible());
    QVERIFY(!popover->findChild<QPushButton *>("teamAddMemberOk")->isEnabled());   // a role first
    for (auto *chip : popover->findChildren<QPushButton *>("teamAddMemberRole")) if (chip->text() == "Tester") chip->click();
    QCOMPARE(popover->findChild<QComboBox *>("teamAddMemberAgent")->currentData().toString(), QStringLiteral("kimi"));
    QCOMPARE(popover->findChild<QPlainTextEdit *>("teamAddMemberPrompt")->toPlainText(), QStringLiteral("Test it."));
    QCOMPARE(popover->findChild<QPushButton *>("teamAddMemberOk")->text(), QStringLiteral("Add Tester"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceHeader")->isVisible());
    shot(&workspace, "d3-add-member");
    popover->findChild<QPushButton *>("teamAddMemberOk")->click();
    QTRY_COMPARE(calls(script(), "add-member").size(), 1);
    const auto input = calls(script(), "add-member").first();
    QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
    const auto member = input["member"].toObject();
    QCOMPARE(member["id"].toString(), QStringLiteral("tester-2"));
    QCOMPARE(member["role"].toString(), QStringLiteral("Tester"));
    QCOMPARE(member["model"].toString(), QStringLiteral("kimi-k2"));
    QCOMPARE(member["folder"].toString(), QStringLiteral("shared"));
}

void TestTeamsUiMembers::subLeadsAreATreeInMembersAndTheInspector()
{
    setTeam(subLeadTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.resize(420, 900); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    darkPalette(); workspace.setTheme(true); panel.setTheme(true);
    const auto cards = [&] {
        QList<QFrame *> found;
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (card->isVisibleTo(&workspace)) found << card;
        std::sort(found.begin(), found.end(), [](QFrame *a, QFrame *b) { return a->mapToGlobal(QPoint()).y() < b->mapToGlobal(QPoint()).y(); });
        QStringList ids; for (auto *card : found) ids << card->property("teamMember").toString();
        return ids;
    };
    // The lead, its direct reports, then the sub-lead followed by its Tester.
    QTRY_COMPARE(cards(), (QStringList{"lead", "backend", "qa-lead", "tester"}));
    QMap<QString, int> levels;
    for (auto *indent : workspace.findChildren<QWidget *>("teamTreeIndent"))
        if (indent->isVisibleTo(&workspace))
        if (auto *card = indent->findChild<QFrame *>("teamWorkspaceMember")) levels.insert(card->property("teamMember").toString(), indent->property("level").toInt());
    QCOMPARE(levels.value("tester"), 1);
    QCOMPARE(levels.value("qa-lead"), 0);
    QCOMPARE(levels.value("backend"), 0);
    // The Tester's card sits 20 px right of the sub-lead's.
    QFrame *subCard = nullptr, *testerCard = nullptr;
    for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember")) {
        if (!card->isVisibleTo(&workspace)) continue;
        if (card->property("teamMember") == "qa-lead") subCard = card;
        if (card->property("teamMember") == "tester") testerCard = card;
    }
    QCOMPARE(testerCard->mapToGlobal(QPoint()).x() - subCard->mapToGlobal(QPoint()).x(), 20);
    QVERIFY(testerCard->toolTip().contains(QStringLiteral("Reports to QA lead")));
    // The sub-lead reviews its report's work, and its pill folds its reports.
    auto *pill = subCard->findChild<QToolButton *>("teamWorkspaceSubLead");
    QVERIFY(pill);
    QCOMPARE(pill->text(), QStringLiteral("sub-lead · 1 report"));
    shot(&workspace, "ux-10-members-tree-dark");
    // A shot may render again: click the pill on screen now.
    for (auto *shown : workspace.findChildren<QToolButton *>("teamWorkspaceSubLead"))
        if (shown->isVisibleTo(&workspace)) pill = shown;
    pill->click();
    QTRY_COMPARE(cards(), (QStringList{"lead", "backend", "qa-lead"}));
    for (auto *again : workspace.findChildren<QToolButton *>("teamWorkspaceSubLead"))
        if (again->isVisibleTo(&workspace)) { again->click(); break; }
    QTRY_COMPARE(cards().size(), 4);
    // "Reports to…" offers the coordinator and the sub-lead, with a check on
    // the current one, and never the member itself or one below it.
    inspectMemberMenu(&workspace, "tester", [](QMenu *menu) {
        QMenu *reports = nullptr;
        for (auto *action : menu->actions()) if (action->menu() && action->menu()->objectName() == "teamWorkspaceReportsTo") reports = action->menu();
        QVERIFY(reports);
        QStringList names; QString checked;
        for (auto *action : reports->actions()) { names << action->text(); if (action->isChecked()) checked = action->property("parent").toString(); }
        QCOMPARE(names, (QStringList{"Lead (coordinator)", "QA lead (sub-lead)"}));
        QCOMPARE(checked, QStringLiteral("qa-lead"));
        reports->actions().first()->trigger();
    });
    QTRY_COMPARE(calls(script(), "reports-to").size(), 1);
    QCOMPARE(calls(script(), "reports-to").first()["member"].toString(), QStringLiteral("tester"));
    QCOMPARE(calls(script(), "reports-to").first()["parent"].toString(), QStringLiteral("lead"));
    inspectMemberMenu(&workspace, "qa-lead", [](QMenu *menu) {
        QAction *toggle = nullptr;
        for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceSubLeadToggle") toggle = action;
        QVERIFY(toggle);
        QCOMPARE(toggle->text(), QStringLiteral("Stop being a sub-lead"));
        QVERIFY(!toggle->isEnabled());
        QCOMPARE(toggle->toolTip(), QStringLiteral("Move its reports to another lead first"));
        QMenu *reports = nullptr;
        for (auto *action : menu->actions()) if (action->menu() && action->menu()->objectName() == "teamWorkspaceReportsTo") reports = action->menu();
        QStringList names; for (auto *action : reports->actions()) names << action->text();
        QCOMPARE(names, QStringList{"Lead (coordinator)"});
    });
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceSubLeadToggle") { QCOMPARE(action->text(), QStringLiteral("Make sub-lead")); action->trigger(); }
    });
    QTRY_COMPARE(calls(script(), "sub-lead").size(), 1);
    QCOMPARE(calls(script(), "sub-lead").first()["sub"].toBool(), true);
    // Add member: "Reports to" the coordinator or the sub-lead.
    QPushButton *add = nullptr;
    for (auto *button : workspace.findChildren<QPushButton *>("teamWorkspaceAddMember")) if (button->isVisibleTo(&workspace)) add = button;
    QVERIFY(add);
    add->click();
    QFrame *popover = nullptr;
    QTRY_VERIFY((popover = workspace.findChild<QFrame *>("teamAddMember")) && popover->isVisible());
    auto *reportsTo = popover->findChild<QComboBox *>("teamAddMemberReportsTo");
    QVERIFY(reportsTo);
    QCOMPARE(reportsTo->count(), 2);
    QCOMPARE(reportsTo->itemText(0), QStringLiteral("Lead (coordinator)"));
    QCOMPARE(reportsTo->itemText(1), QStringLiteral("QA lead (sub-lead)"));
    reportsTo->setCurrentIndex(1);
    for (auto *chip : popover->findChildren<QPushButton *>("teamAddMemberRole")) if (chip->text() == "Tester") chip->click();
    shot(&workspace, "ux-10-add-member-reports-to-dark");
    popover->findChild<QPushButton *>("teamAddMemberOk")->click();
    QTRY_COMPARE(calls(script(), "add-member").size(), 1);
    QCOMPARE(calls(script(), "add-member").first()["member"].toObject()["parent"].toString(), QStringLiteral("qa-lead"));
    // The inspector lists the members as the same tree.
    const auto rows = [&] {
        QList<QWidget *> shown;
        for (auto *row : panel.findChildren<QWidget *>("teamSummaryMemberTree")) if (row->isVisibleTo(&panel)) shown << row;
        return shown;
    };
    QTRY_COMPARE(rows().size(), 4);
    QWidget *testerRow = nullptr;
    for (auto *row : rows()) if (row->property("level").toInt() == 1) testerRow = row;
    QVERIFY(testerRow);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&panel, "ux-10-inspector-tree-dark");
        workspace.setTheme(false); panel.setTheme(false);
        lightShot(&workspace, "ux-10-members-tree-light");
        lightShot(&panel, "ux-10-inspector-tree-light");
    }
}

// #380 attempt 2: the member's "…" menu and the swap dialog run their own
// event loops. A poll that changes the team arrives under each: the menu, the
// button that opened it and the open dialog stay alive, and the views catch
// up once the menu has closed.
void TestTeamsUiMembers::aPollUnderTheMemberMenuKeepsItsButtonAndTheSwapDialog()
{
    setTeam(subLeadTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.resize(420, 900); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    WAIT_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto visibleMore = [&](const QString &member) -> QToolButton * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == member && card->isVisibleTo(&workspace)) return card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
        return nullptr;
    };
    WAIT_VERIFY(controller.watched() && visibleMore("backend"));
    QPointer<QToolButton> more = visibleMore("backend");
    // The next poll brings a changed team: without the hold it rebuilds the
    // members column and deletes the button under its open menu.
    auto changed = subLeadTeam();
    changed["revision"] = 13;
    auto members = changed["members"].toArray();
    auto backend = members[1].toObject(); backend["model"] = "gpt-6.1-sol"; members[1] = backend;
    changed["members"] = members;
    setTeam(changed);
    bool checked = false;
    QPointer<QMenu> open;
    std::function<void(int)> look = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *menu = qobject_cast<QMenu *>(QApplication::activePopupWidget());
            if (!menu) { if (tries > 0) look(TestWait::busy() ? tries : tries - 1); return; }
            open = menu;
            QVERIFY(panel.confirming());
            controller.refresh();
            WAIT_SETTLED();
            QVERIFY(more);
            QVERIFY(open && open->isVisible());
            QCOMPARE(controller.team("team-1")->revision, 13);
            checked = true;
            // Open the swap dialog from the same menu.
            for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberSwap") action->trigger();
            menu->hide();
        });
    };
    look(300);
    more->click();
    WAIT_VERIFY(checked);
    QPointer<QDialog> dialog;
    WAIT_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    // The views catch up once the menu has closed: the column is rebuilt
    // with the new team, so the old button is not the one shown any more.
    WAIT_VERIFY(!panel.confirming());
    WAIT_VERIFY(visibleMore("backend") && visibleMore("backend") != more.data());
    // A poll while the swap dialog is open leaves it open.
    changed["revision"] = 14;
    setTeam(changed);
    controller.refresh();
    WAIT_COMPARE(controller.team("team-1")->revision, 14);
    WAIT_SETTLED();
    QVERIFY(dialog && dialog->isVisible());
    dialog->close();
}

// U54 (#619): a member whose old session is busy waits with that reason and
// Move again; a folder left on another branch names both branches and offers
// the checkout, only for a local folder: after one confirmation it runs git
// switch (a dirty folder is refused and the card says so), then relaunches.
void TestTeamsUiMembers::memberBlocksShowTheirReasonAndReviewAction()
{
    QTemporaryDir repo;
    QVERIFY(repo.isValid());
    const QString folder = QDir(repo.path()).filePath("backend");
    QVERIFY(QDir().mkpath(folder));
    const auto git = [&](const QStringList &args) {
        QProcess process; process.setWorkingDirectory(folder);
        process.start("git", QStringList{"-c", "user.name=Test", "-c", "user.email=test@example.invalid"} + args);
        process.waitForFinished(30000);
        return process.exitCode() == 0 ? QString::fromUtf8(process.readAllStandardOutput()).trimmed() : QStringLiteral("<failed>");
    };
    QVERIFY(git({"init", "-q"}) != "<failed>");
    QVERIFY(git({"checkout", "-q", "-b", "team/t/backend"}) != "<failed>");
    { QFile f(QDir(folder).filePath("a.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("a"); }
    QVERIFY(git({"add", "."}) != "<failed>");
    QVERIFY(git({"commit", "-qm", "base"}) != "<failed>");
    QVERIFY(git({"checkout", "-q", "-b", "team/t/backend-3"}) != "<failed>");
    { QFile f(QDir(folder).filePath("b.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("b"); }
    QVERIFY(git({"add", "."}) != "<failed>");
    QVERIFY(git({"commit", "-qm", "task"}) != "<failed>");
    const QString branchError = folder + " is on team/t/backend-3, not team/t/backend; team/t/backend-3 is not merged into team/t/backend. "
                                         "Merge it, then check out team/t/backend in the folder.";
    auto blocked = withMember(moveTeam(), "backend", [&](QJsonObject &m) {
        m.remove("session"); m["state"] = "not_created"; m["directory"] = folder; m["error"] = branchError;
    });
    blocked = withMember(blocked, "reviewer", [](QJsonObject &m) {
        m["error"] = "the account switch is rolled_back; waiting for the old session to finish its turn";
    });
    blocked = withMember(blocked, "qa", [](QJsonObject &m) {
        m["error"] = "the account switch is uncertain; the old session was not paused: it is gone";
    });
    // A folder on another computer gets the reason, never the checkout.
    blocked = withMember(blocked, "designer", [](QJsonObject &m) {
        m.remove("session"); m["state"] = "not_created";
        m["error"] = "/srv/designer is on team/t/designer-2, not team/t/designer; it has uncommitted changes, so Zerus did not switch it.";
    });
    for (const bool dark : {true, false}) {
        if (dark) darkPalette(); else lightPalette();
        const QString theme = dark ? QStringLiteral("dark") : QStringLiteral("light");
        QFile::remove(script() + ".calls");
        setTeam(blocked);
        QVERIFY(FakeCli::replace(script() + ".accounts", moveAccounts));
        HgsClient client(FakeCli::program(script()));
        TeamController controller(&client);
        TeamPanel panel(&controller);
        TeamWorkspace workspace(&controller, &panel);
        workspace.setTheme(dark);
        workspace.setLive(live);
        workspace.resize(1300, 900); workspace.show();
        controller.refresh();
        panel.showTeam("team-1");
        workspace.showTeam("team-1");
        QTRY_VERIFY(memberCard(&workspace, "backend") && memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout"));
        const auto stateOf = [&](const QString &id) {
            auto *card = memberCard(&workspace, id);
            auto *state = card ? card->findChild<QWidget *>("teamWorkspaceMemberState") : nullptr;
            return state ? state->toolTip() : QString();
        };
        QVERIFY2(stateOf("backend").contains(QStringLiteral("Folder on another branch")), qPrintable(stateOf("backend")));
        auto *reason = memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberBlockReason");
        QVERIFY(reason);
        QCOMPARE(reason->text(), QStringLiteral("Its folder is on team/t/backend-3, not team/t/backend"));
        QVERIFY(reason->toolTip().contains(QStringLiteral("Merge it, then check out team/t/backend")));
        auto *checkout = memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout");
        QCOMPARE(checkout->text(), QStringLiteral("Check out team/t/backend…"));
        // The remote folder: its reason only.
        QVERIFY(memberCard(&workspace, "designer")->findChild<QWidget *>("teamWorkspaceMemberBlockReason"));
        QVERIFY(!memberCard(&workspace, "designer")->findChild<QPushButton *>("teamWorkspaceMemberCheckout"));
        // A busy old session waits; Move again once it is idle.
        QVERIFY2(stateOf("reviewer").contains(QStringLiteral("Waiting for its old session")), qPrintable(stateOf("reviewer")));
        QVERIFY(memberCard(&workspace, "reviewer")->findChild<QPushButton *>("teamWorkspaceMemberMoveAgain"));
        QVERIFY2(stateOf("qa").contains(QStringLiteral("Account switch unconfirmed")), qPrintable(stateOf("qa")));
        QVERIFY(memberCard(&workspace, "qa")->findChild<QPushButton *>("teamWorkspaceMemberOpenSession"));
        shot(&workspace, "u54-member-blocks-" + theme);
        if (!dark) continue;
        // Move again opens the move with the member's account.
        memberCard(&workspace, "reviewer")->findChild<QPushButton *>("teamWorkspaceMemberMoveAgain")->click();
        QDialog *dialog = nullptr;
        QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamMoveAccountDialog")) && dialog->isVisible());
        QTRY_COMPARE(dialog->findChild<QComboBox *>("teamMoveAccountFrom")->currentData().toString(), QStringLiteral("native-claude"));
        dialog->close();
        QTRY_VERIFY(!workspace.findChild<QDialog *>("teamMoveAccountDialog"));
        // A dirty folder is never switched; the card says why.
        { QFile f(QDir(folder).filePath("dirty.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("x"); }
        QString asked;
        armConfirmRecord(&panel, false, nullptr, &asked);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTRY_VERIFY(memberCard(&workspace, "backend") && memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError"));
        QVERIFY2(asked.contains(folder) && asked.contains(QStringLiteral("from team/t/backend-3 to team/t/backend")), qPrintable(asked));
        QVERIFY(asked.contains(QStringLiteral("without --force")));
        QVERIFY(memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError")->text().contains(QStringLiteral("uncommitted changes")));
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend-3"));
        QVERIFY(calls(script(), "relaunch").isEmpty());
        shot(&workspace, "u54-checkout-refused-dark");
        // Clean: switched, the task branch kept, then relaunched.
        QVERIFY(QFile::remove(QDir(folder).filePath("dirty.txt")));
        armConfirm(&panel, false);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTRY_COMPARE(calls(script(), "relaunch").size(), 1);
        QCOMPARE(calls(script(), "relaunch").first()["member"].toString(), QStringLiteral("backend"));
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend"));
        QVERIFY(git({"rev-parse", "--verify", "team/t/backend-3"}) != "<failed>");
        QTRY_VERIFY(!memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError"));
        // Cancel at its confirmation changes nothing.
        QVERIFY(git({"checkout", "-q", "team/t/backend-3"}) != "<failed>");
        armConfirm(&panel, true);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTest::qWait(300);
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend-3"));
        QCOMPARE(calls(script(), "relaunch").size(), 1);
    }
    darkPalette();
}

// U49: the member menu adds more members of a role (scale) and drains one;
// a draining member is marked and its drain can be cancelled.
void TestTeamsUiMembers::memberMenuScalesAndDrains()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.resize(420, 900); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    darkPalette(); workspace.setTheme(true); panel.setTheme(true);
    const auto draining = [&] {
        for (auto *mark : workspace.findChildren<QWidget *>("teamWorkspaceMemberDraining")) if (mark->isVisibleTo(&workspace)) return true;
        return false;
    };
    QTRY_VERIFY(!workspace.findChildren<QFrame *>("teamWorkspaceMember").isEmpty());
    QVERIFY(!draining());
    // Scale: "Add more Backend…" asks how many and sends the member's agent and model.
    bool asked = false;
    std::function<void(int)> lookCount = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *input = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!input) { if (tries > 0) lookCount(TestWait::busy() ? tries : tries - 1); return; }
            asked = true;
            QVERIFY(input->labelText().contains("Backend"));
            input->setIntValue(2); input->accept();
        });
    };
    lookCount(1000);
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        QStringList names; for (auto *action : menu->actions()) names << action->objectName();
        QVERIFY(names.contains("teamWorkspaceMemberDrain") && !names.contains("teamWorkspaceMemberUndrain"));
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale") { QCOMPARE(action->text(), QStringLiteral("Add more Backend…")); action->trigger(); }
    });
    QTRY_VERIFY(asked);
    QTRY_COMPARE(calls(script(), "scale").size(), 1);
    QCOMPARE(calls(script(), "scale").first()["role"].toString(), QStringLiteral("Backend"));
    QCOMPARE(calls(script(), "scale").first()["agent"].toString(), QStringLiteral("codex"));
    QCOMPARE(calls(script(), "scale").first()["model"].toString(), QStringLiteral("gpt-5.5-codex"));
    QCOMPARE(calls(script(), "scale").first()["count"].toInt(), 2);
    // Drain asks first.
    armConfirm(&panel, false);
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberDrain") action->trigger();
    });
    QTRY_COMPARE(calls(script(), "drain").size(), 1);
    QCOMPARE(calls(script(), "drain").first()["member"].toString(), QStringLiteral("backend"));
    QVERIFY(!calls(script(), "drain").first().contains("draining"));
    // Draining: the card says so, and Cancel drain sends draining false.
    auto drained = team();
    auto members = drained["members"].toArray();
    auto backend = members[1].toObject(); backend["draining"] = true; members[1] = backend;
    drained["members"] = members;
    drained["revision"] = 13;
    setTeam(drained);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 13 && t->member("backend")->draining; })());
    QTRY_VERIFY(draining());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u49-member-draining-dark");
        lightPalette(); workspace.setTheme(false); panel.setTheme(false);
        lightShot(&workspace, "u49-member-draining-light");
        darkPalette(); workspace.setTheme(true); panel.setTheme(true);
    }
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberUndrain") action->trigger();
    });
    QTRY_COMPARE(calls(script(), "drain").size(), 2);
    QCOMPARE(calls(script(), "drain").last()["draining"].toBool(true), false);
    // A full team cannot scale.
    auto full = drained; full["settings"] = QJsonObject{{"max_members", 2}}; full["revision"] = 14;
    setTeam(full);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 14 && t->maxMembers == 2; })());
    inspectMemberMenu(&workspace, "tester", [](QMenu *menu) {
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale") { QVERIFY(!action->isEnabled()); QVERIFY(action->toolTip().contains("2 members")); }
    });
    // #581 L2: the lead takes a seat, as hgs's scale counts it; a removed member does not.
    int seated = 0;
    for (const auto &value : drained["members"].toArray()) seated += value.toObject()["state"].toString() != QLatin1String("removed");
    QVERIFY(seated >= 3);
    auto exact = drained; exact["settings"] = QJsonObject{{"max_members", seated}}; exact["revision"] = 15;
    QCOMPARE(scaleRoom(parse(exact)), 0);
    auto spare = exact; spare["settings"] = QJsonObject{{"max_members", seated + 2}};
    QCOMPARE(scaleRoom(parse(spare)), 2);
    auto gone = exact; auto seats = gone["members"].toArray(); auto last = seats.last().toObject(); last["state"] = "removed"; seats[seats.size() - 1] = last; gone["members"] = seats;
    QCOMPARE(scaleRoom(parse(gone)), 1);
    setTeam(exact);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 15 && t->maxMembers == seated; })());
    inspectMemberMenu(&workspace, "tester", [seated](QMenu *menu) {
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale")
                QVERIFY2(!action->isEnabled() && action->toolTip().contains(QStringLiteral("%1 members").arg(seated)), qPrintable(action->toolTip()));
    });
}

// U25 (spec 5.7): the right column's lists start collapsed: Needs your answer
// with a phase draft's Review, Team stuck with Show on graph, Plan problems
// that open the task; the members' task lists are folded too.
void TestTeamsUiMembers::sideListsAndMemberTasks()
{
    QSettings().setValue("workspace/skin", "whitewh");
    for (const auto &key : {"ui/collapsed/team/needs", "ui/collapsed/team/stuck", "ui/collapsed/team/plan"}) QSettings().remove(key);
    QSettings().remove("ui/collapsed/teamMemberTasks/team-1/frontend");
    // Stuck in Build: #2's question, #3 unconfirmed, #4 decided; #5 and #6 wait.
    auto stuck = prototypeTeam();
    auto tasks = stuck["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto k = tasks[i].toObject();
        if (k["id"] == 4) k["status"] = "accepted";
        if (k["id"] == 3) k["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "frontend"}, {"state", "unconfirmed"}, {"sent_at", 1400}}};
        tasks[i] = k;
    }
    stuck["tasks"] = tasks;
    stuck["stuck_scope"] = QJsonArray{2, 3, 5, 6};
    setTeam(stuck);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    int opened = 0; workspace.openWorkspace = [&opened] { ++opened; };
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto block = [&workspace](const char *name) { return dynamic_cast<CollapsibleBlock *>(workspace.findChild<QWidget *>(name)); };
    QTRY_VERIFY(block("teamStuckBlock"));
    auto *needs = block("teamWorkspaceAnswers"); QVERIFY(needs);
    QVERIFY(!needs->isExpanded() && !block("teamStuckBlock")->isExpanded());
    QCOMPARE(block("teamStuckBlock")->findChild<QLabel *>("collapsibleNote")->text(), QStringLiteral("2 blockers"));
    // Needs: the session question and the draft, its Review opening phase 3.
    needs->header()->click();
    QTRY_VERIFY(block("teamWorkspaceAnswers")->isExpanded());
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceReviewPhase") && workspace.findChild<QPushButton *>("teamWorkspaceReviewPhase")->isVisible());
    QVERIFY(workspace.findChild<QPushButton *>("teamWorkspaceGoToQuestion")->isVisible());
    workspace.findChild<QPushButton *>("teamWorkspaceReviewPhase")->click();
    QCOMPARE(opened, 1);
    // Team stuck: Show on graph lights the blockers in the running phase.
    block("teamStuckBlock")->header()->click();
    QTRY_VERIFY(block("teamStuckBlock")->isExpanded());
    QCOMPARE(workspace.findChildren<QFrame *>("teamStuckRow").size(), 2);
    workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->click();
    QTRY_VERIFY(([&] {
        for (auto *w : workspace.findChildren<QWidget *>("teamPhaseGraph"))
            if (auto *v = dynamic_cast<TeamUi::TaskView *>(w); v && w->property("phase").toInt() == 2) return v->emphasis == QSet<int>{2, 3};
        return false;
    })());
    QCOMPARE(workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->text(), QStringLiteral("Hide"));
    // N15 (#383): the team recovers: the phase graphs follow the filter again,
    // and a later stuck starts with Show on graph.
    {
        const auto phaseTwo = [&]() -> TeamUi::TaskView * {
            for (auto *w : workspace.findChildren<QWidget *>("teamPhaseGraph"))
                if (auto *v = dynamic_cast<TeamUi::TaskView *>(w); v && w->property("phase").toInt() == 2) return v;
            return nullptr;
        };
        // #3's delivery is confirmed: the team works again.
        auto recovered = stuck; recovered["revision"] = 60;
        auto recoveredTasks = recovered["tasks"].toArray();
        for (int i = 0; i < recoveredTasks.size(); ++i) {
            auto k = recoveredTasks[i].toObject();
            if (k["id"] == 3) k["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "frontend"}, {"state", "delivered"}, {"sent_at", 1400}}};
            recoveredTasks[i] = k;
        }
        recovered["tasks"] = recoveredTasks;
        setTeam(recovered);
        QTRY_VERIFY(([&] { controller.refresh(); auto *v = phaseTwo(); return !block("teamStuckBlock") && v && v->emphasis.isEmpty(); })());
        for (const auto &node : phaseTwo()->nodes) QVERIFY2(node.match, qPrintable(QString::number(node.id)));
        stuck["revision"] = 61; setTeam(stuck);
        QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QPushButton *>("teamStuckShowOnGraph"); })());
        QCOMPARE(workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->text(), QStringLiteral("Show on graph"));
        // A stuck team without phase groups: Show on graph lights the blockers
        // in its one graph.
        auto flat = stuck; flat.remove("phases"); flat.remove("phase_summary"); flat["revision"] = 62;
        auto flatTasks = flat["tasks"].toArray();
        for (int i = 0; i < flatTasks.size(); ++i) { auto k = flatTasks[i].toObject(); k["phase"] = 1; flatTasks[i] = k; }
        flat["tasks"] = flatTasks;
        setTeam(flat);
        QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QWidget *>("teamPhaseGraph") && workspace.findChild<QPushButton *>("teamStuckShowOnGraph"); })());
        workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->click();
        QTRY_VERIFY(([&] {
            auto *v = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
            return v && v->emphasis == QSet<int>{2, 3};
        })());
        auto *flatGraph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
        for (const auto &node : flatGraph->nodes) QCOMPARE(node.match, node.id == 2 || node.id == 3);
        QCOMPARE(workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->text(), QStringLiteral("Hide"));
        stuck["revision"] = 63; setTeam(stuck);
        QTRY_VERIFY(([&] { controller.refresh(); return phaseTwo() != nullptr; })());
    }
    // Members: the task lists start folded; "1 task" opens Frontend's.
    QWidget *toggle = nullptr;
    for (auto *t : workspace.findChildren<QToolButton *>("teamMemberTasksToggle")) if (t->property("member") == "frontend") toggle = t;
    QVERIFY(toggle); QCOMPARE(static_cast<QToolButton *>(toggle)->text(), QStringLiteral("1 task"));
    QVERIFY(!workspace.findChild<QWidget *>("teamMemberQueue"));
    static_cast<QToolButton *>(toggle)->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamMemberQueue"));
    QCOMPARE(workspace.findChild<QWidget *>("teamMemberQueue")->property("member").toString(), QStringLiteral("frontend"));
    QVERIFY(workspace.findChild<QPushButton *>("teamMemberQueueRow"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "ux-4-side-stuck-dark");
        workspace.setTheme(false);
        if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) lightShot(&workspace, "ux-4-side-stuck-light");
        darkPalette(); workspace.setTheme(true);
    }
    // Planning: the phases are "In the plan" without actions; Plan problems opens the task.
    auto planning = prototypeTeam();
    planning["status"] = "planning";
    auto planTasks = planning["tasks"].toArray();
    for (int i = 0; i < planTasks.size(); ++i) { auto k = planTasks[i].toObject(); k["status"] = "draft"; k.remove("attempts"); if (k["id"] == 6) k.remove("assignee"); planTasks[i] = k; }
    planning["tasks"] = planTasks;
    planning["validation"] = QJsonObject{{"errors", QJsonArray{QJsonObject{{"kind", "assignee"}, {"task", 6}, {"text", "#6 E2E checkout flow: no assignee"}}}},
                                         {"warnings", QJsonArray()}};
    setTeam(planning);
    QTRY_VERIFY(([&] { controller.refresh(); return block("teamPlanProblemsBlock") != nullptr; })());
    QCOMPARE(block("teamPlanProblemsBlock")->findChild<QLabel *>("collapsibleNote")->text(), QStringLiteral("1 error"));
    QVERIFY(!block("teamStuckBlock"));
    for (auto *f : workspace.findChildren<QFrame *>("teamPhaseScroll")) {
        QCOMPARE(f->property("state").toString(), QStringLiteral("planned"));
        QVERIFY(!f->findChild<QPushButton *>("teamPhaseAccept") && !f->findChild<QPushButton *>("teamPhaseReject"));
    }
    QVERIFY(workspace.findChild<QPushButton *>("teamWorkspaceAddTask"));
    QVERIFY(!workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept") && !workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    block("teamPlanProblemsBlock")->header()->click();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamPlanProblem") && workspace.findChild<QFrame *>("teamPlanProblem")->isVisible());
    QCOMPARE(workspace.findChild<QFrame *>("teamPlanProblem")->property("task").toInt(), 6);
    shot(&workspace, "ux-4-planning-dark");
    workspace.setTheme(false);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) lightShot(&workspace, "ux-4-planning-light");
    darkPalette();
    QSettings().remove("workspace/skin");
}

QTEST_MAIN(TestTeamsUiMembers)
#include "test_teamsui_members.moc"
