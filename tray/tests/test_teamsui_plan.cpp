// WhiteWh fork, #713: the agent-team UI tests of the plan and its phases:
// drafts, approvals, moves, free planning and the lead load.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include "ActivityView.h"
#include "CollapsibleBlock.h"
#include "HgsClient.h"
#include "TeamPhaseScroll.h"
#include "TeamTaskCard.h"
#include "TeamWorkspace.h"
#include <QSpinBox>

namespace {
QPushButton *phaseAction(QWidget *root, int phase, const char *name)
{
    auto *frame = phaseScrollOf(root, phase);
    return frame ? frame->findChild<QPushButton *>(name) : nullptr;
}

// U59: a running team in free mode. Phase 1 closed in standard mode; phase 2,
// Support stream, is the open free phase: #3 and #6 are accepted (one since
// the last build at 10:00 today), #4 runs and #7 waits for it.
QJsonObject freeTeam()
{
    auto t = team();
    t["settings"] = QJsonObject{{"all_hard", false}, {"approval", "review"}, {"phase_approval", "review"}};
    t["waiting_for_lead"] = false;
    t["plan_mode"] = "free";
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 1}, {"title", "Analysis"}, {"state", "approved"}, {"tasks", QJsonArray{1, 2}}},
        QJsonObject{{"n", 2}, {"title", "Support stream"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4, 6, 7}}}};
    const auto task = [](int id, int phase, const char *title, const char *assignee, const char *status, const QJsonArray &deps = {}) {
        return QJsonObject{{"id", id}, {"phase", phase}, {"title", title}, {"assignee", assignee}, {"status", status}, {"deps", deps}};
    };
    t["tasks"] = QJsonArray{
        task(1, 1, "Requirements", "lead", "accepted"),
        task(2, 1, "Checkout design", "backend", "accepted"),
        task(3, 2, "Coupon field loses focus", "backend", "accepted"),
        [&task] {
            auto running = task(4, 2, "Refund webhook retries", "backend", "in_progress");
            running["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a4"}, {"member", "backend"}, {"state", "delivered"}, {"sent_at", 1600.0}}};
            return running;
        }(),
        task(6, 2, "Currency in receipts", "backend", "accepted"),
        task(7, 2, "Empty cart crash", "tester", "pending", QJsonArray{QJsonObject{{"task", 4}}})};
    t["layers"] = QJsonObject{{"1", 0}, {"2", 0}, {"3", 0}, {"4", 0}, {"6", 0}, {"7", 1}};
    t["ready"] = QJsonArray();
    t["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
    const double tenToday = double(QDateTime(QDate::currentDate(), QTime(10, 0)).toSecsSinceEpoch());
    t["free_phase"] = QJsonObject{{"phase", 2}, {"state", "open"}, {"running", QJsonArray{4, 7}}, {"accepted", 2},
                                  {"accepted_since", 1}, {"last_build_at", tenToday}, {"close_pending", QJsonValue()}};
    return t;
}

// The same team once #4 and #7 are accepted: phase 2 is open and idle.
QJsonObject idleFreeTeam()
{
    auto t = freeTeam();
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto k = tasks[i].toObject(); k["status"] = "accepted"; k.remove("attempts"); tasks[i] = k; }
    t["tasks"] = tasks;
    auto free = t["free_phase"].toObject();
    free["state"] = "idle"; free["running"] = QJsonArray(); free["accepted"] = 4; free["accepted_since"] = 3;
    t["free_phase"] = free;
    return t;
}
}

// U61: with Auto-accept drafts on, a drafted phase stays a draft the lead edits
// until its first task is handed out; then Auto accepts it.
static QJsonObject autoDraftTeam(bool startAhead, bool accepted)
{
    auto t = phasedTeam();
    auto settings = t["settings"].toObject(); settings["auto_accept"] = true; settings["phase_approval"] = "auto"; t["settings"] = settings;
    auto phases = t["phases"].toArray();
    auto three = phases[1].toObject();
    three["approval"] = "auto";
    if (startAhead) three["start_ahead"] = true;
    if (accepted) { three["state"] = "approved"; three["decided_by"] = "Auto"; three["decided_at"] = 1800.0; t["pending_phase"] = 0; }
    phases[1] = three; t["phases"] = phases;
    if (accepted) {
        auto tasks = t["tasks"].toArray();
        for (int i = 0; i < tasks.size(); ++i) {
            auto task = tasks[i].toObject();
            if (task["id"].toInt() == 5) { task["status"] = "assigned"; task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a5"}, {"member", "backend"}, {"state", "delivered"}, {"sent_at", 1800.0}}}; }
            tasks[i] = task;
        }
        t["tasks"] = tasks;
    }
    return t;
}

class TestTeamsUiPlan : public QObject, public TeamsUiFixture {
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
    void planningEditsSendTheWholePlan();
    void sendBackFormBelongsToItsTeamAndPhase();
    void phaseApprovalAndFinishAskTheOwner();
    void phaseApprovalAlongsideARunningPhaseAsksFirst();
    void aRefreshUnderThePhaseConfirmationKeepsItsAsker();
    void phaseProgressShowsTwoBars();
    void preflightWarningsShowInThePlanCard();
    void leadLoadViewsShowInTheDesktop();
    void severalDraftPhasesRenderAndApproveInOrder();
    void phaseBandsStackWithStatusDecisionAndHealth();
    void aQueuedPhaseCanBeDiscarded();
    void aQueuedPhaseCanReturnToDraft();
    void draftReorderSendsMovePhase();
    void phaseRetriesReuseTheRequestId();
    void malformedPhasesFallBack();
    void nextGoalFromADoneTeam();
    void planModeIsChosenWhenThePlanIsApproved();
    void switchToFreeOpensOneDialogBoundToTheRequest();
    void freePhaseReadsIdleBuildsAndClosesOnConfirmation();
    void planEditKeepsTheRoadmap();
    void planEditsWaitForTheFullPlan();
    void workAheadSwitchAndBadge();
    void thePlanHidesCancelledTasksAndNeverSendsTheCopy();
    void aTaskThatHasNotStartedMovesToAnotherPhase();
    void anAutoAcceptedDraftReadsAsDraftUntilItStarts();
    void addedLateTaskIsOutOfScopeEverywhere();
    void planTaskTakesModelsAndARole();
    void theEstimateIsSetInThePlanAndWhenATaskJoinsAPhase();
    void phaseScrollsInEveryView();
    void phaseScrollDecisions();
};

void TestTeamsUiPlan::planningEditsSendTheWholePlan()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.setLive(live);
    panel.resize(420, 1400); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    // The list arrives before the full status with its plan errors.
    QTRY_VERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));
    auto *approve = panel.findChild<QPushButton *>("teamApprove");
    QTRY_VERIFY((approve = panel.findChild<QPushButton *>("teamApprove")) && !approve->isEnabled());   // #6 has no assignee
    QCOMPARE(panel.findChildren<QComboBox *>("teamMemberAgent").size(), 3);
    shot(&panel, "planning-420");
    // Nothing is wider than the panel (design D4).
    QVERIFY(panel.findChild<QWidget *>("teamPanelContent")->minimumSizeHint().width() <= 420);
    // Dragging #1's handle onto #5 makes #5 depend on #1: the whole plan goes once.
    auto *graph = dynamic_cast<TeamUi::TaskView *>(panel.findChild<QWidget *>("teamTaskGraph"));
    QVERIFY(graph && graph->editable && graph->linked);
    graph->linked(3, 5);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    const auto plan = calls(script(), "plan").first();
    QCOMPARE(plan["revision"].toInt(), 12);
    QCOMPARE(plan["by"].toString(), QStringLiteral("human"));
    QCOMPARE(plan["members"].toArray().size(), 2);   // the lead is kept by hgs
    QJsonArray deps;
    for (const auto &value : plan["tasks"].toArray()) if (value.toObject()["id"].toInt() == 5) deps = value.toObject()["deps"].toArray();
    QCOMPARE(deps.size(), 3);
    // Send back to lead sends your comment to the lead's session.
    QString sent;
    panel.sendMessage = [&](const QString &, const QString &, const QString &text) { sent = text; };
    panel.findChild<QPushButton *>("teamSendBack")->click();
    QTRY_VERIFY(panel.findChild<QPlainTextEdit *>("teamSendBackText"));
    panel.findChild<QPlainTextEdit *>("teamSendBackText")->setPlainText("Split #3");
    panel.findChild<QPushButton *>("teamSendBackSubmit")->click();
    QVERIFY(sent.contains("Split #3") && sent.startsWith("[Zerus team"));
}

// U25 (spec 5.2, 5.3): a draft's Accept, Reject… with the lead's words, Undo
// of a send-back, the queued state and Auto-accept, from the scroll's header.
// N14 (#383): the send-back form belongs to one team's phase as it was when
// it opened: another team with a draft of the same number shows no form and no
// words, renumbered drafts close it, and the request names the revision it
// was written against (the server refuses it after a renumbering).
void TestTeamsUiPlan::sendBackFormBelongsToItsTeamAndPhase()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); });
    auto first = prototypeTeam(); first["revision"] = 30;
    setTeam(first);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto scroll = [&](int phase) -> QFrame * {
        for (auto *f : workspace.findChildren<QFrame *>("teamPhaseScroll")) if (f->property("phase").toInt() == phase) return f;
        return nullptr;
    };
    const auto form = [&] { return scroll(3) ? scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText") : nullptr; };
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(form());
    QTest::keyClicks(form(), "Words for team one");
    // Another team, also with a draft phase 3: no form, no words.
    auto second = prototypeTeam(); second["id"] = "team-2"; second["name"] = "Second"; second["revision"] = 31;
    setTeam(second);
    controller.refresh();
    QTRY_VERIFY(controller.team("team-2"));
    panel.showTeam("team-2"); workspace.showTeam("team-2");
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    QTest::qWait(100);
    QVERIFY(!form());
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(form());
    QCOMPARE(form()->toPlainText(), QString());
    // Back on team one: the drafts are renumbered (the lead discarded one): the form closes.
    setTeam(first); controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); return controller.watched() && controller.watched()->id == "team-1"; })());
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(form());
    QTest::keyClicks(form(), "Words for phase three");
    auto renumbered = first; renumbered["revision"] = 32; renumbered["phase_numbers_revision"] = 32;
    setTeam(renumbered);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *w = controller.watched(); return w && w->phaseNumbersRevision == 32 && scroll(3) && !form(); })());
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(form());
    QCOMPARE(form()->toPlainText(), QString());
    QCOMPARE(calls(script(), "reject-phase").size(), 0);
    // Sent: with the revision it was written against.
    QTest::keyClicks(form(), "Split the release notes out");
    scroll(3)->findChild<QPushButton *>("teamPhaseRejectSend")->click();
    QTRY_COMPARE(calls(script(), "reject-phase").size(), 1);
    const auto sent = calls(script(), "reject-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["comment"].toString(), QStringLiteral("Split the release notes out"));
    QCOMPARE(sent["revision"].toInteger(), 32);
}

void TestTeamsUiPlan::phaseApprovalAndFinishAskTheOwner()
{
    // Spec 10: phase 3 opens only after phase 2 is approved and its tasks are
    // decided. The fixture leaves #4 open, so close phase 2 first.
    auto ready = phasedTeam();
    auto readyTasks = ready["tasks"].toArray();
    for (int i = 0; i < readyTasks.size(); ++i) {
        auto task = readyTasks[i].toObject();
        if (task["phase"].toInt() == 2) task["status"] = "accepted";
        readyTasks[i] = task;
    }
    ready["tasks"] = readyTasks;
    setTeam(ready);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    WAIT_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    // The team bar says the phase waits; the draft's band header carries the
    // status chip, progress and health, and the two owner decisions (G9).
    WAIT_VERIFY(workspace.findChild<QWidget *>("teamBarPendingPhase"));
    // U25: the draft's scroll carries its status, progress and the owner's decisions.
    WAIT_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->property("state").toString(), QStringLiteral("draft"));
    workspace.render();
    WAIT_SETTLED();
    shot(&workspace, "d3-phases-draft");
    // The draft is ready once `team status` has answered (a list poll carries no validation).
    WAIT_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY(phaseAction(&workspace, 3, "teamPhaseReject"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceFinish"));   // tasks are still open
    // The header folds the scroll to its header row; the members column folds independently.
    auto *membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    QVERIFY(membersToggle);
    togglePhase(&workspace, 3);
    WAIT_VERIFY(phaseScrollOf(&workspace, 3) && !phaseOpen(&workspace, 3));
    shot(&workspace, "d3-phases-folded");
    togglePhase(&workspace, 3);
    WAIT_VERIFY(phaseOpen(&workspace, 3));
    membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    membersToggle->click();
    WAIT_VERIFY(!workspace.findChild<QFrame *>("teamWorkspaceMember"));
    shot(&workspace, "d3-members-folded");
    workspace.findChild<QToolButton *>("teamMembersToggle")->click();
    WAIT_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceMember"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "d3-phases-draft-open");
        // The same draft in the light theme.
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        WAIT_SETTLED();
        shot(&workspace, "d3-phases-draft-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
    }
    // Accept asks the owner first, then sends once with the phase; a second
    // click before the poll sends nothing. A cancelled confirmation sends
    // nothing and leaves Accept usable.
    WAIT_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    armConfirm(&panel, true);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    WAIT_SETTLED();
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    WAIT_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    armConfirm(&panel, false);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    WAIT_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "approve-phase").first()["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(calls(script(), "approve-phase").first()["by"].toString(), QStringLiteral("human"));
    // The sent decision holds until the store's revision moves: neither Accept
    // nor Reject… nor the menu acts again (no double send from a double click
    // or a quick re-render).
    WAIT_SETTLED();
    workspace.render();
    WAIT_SETTLED();
    WAIT_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseReject")->isEnabled());
    QVERIFY(!phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->isEnabled());
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    WAIT_SETTLED();
    QCOMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    // The store's next revision reopens the header for the same phase; the
    // Discard test runs on it.
    auto decidedTeam = ready;
    decidedTeam["revision"] = decidedTeam["revision"].toInt(1) + 1;
    setTeam(decidedTeam);
    WAIT_VERIFY(([&] { controller.refresh(); auto *accept = phaseAction(&workspace, 3, "teamPhaseAccept"); return accept && accept->isEnabled(); })());
    // Discard, from the phase's menu, asks to confirm: Cancel sends nothing, the confirmation sends once.
    const auto discardFromMenu = [&] {
        chooseMenuAction("teamPhaseDiscard");
        phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->click();
    };
    armConfirm(&panel, true);
    discardFromMenu();
    WAIT_SETTLED();
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    WAIT_VERIFY(phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->isEnabled());
    armConfirm(&panel, false);
    discardFromMenu();
    WAIT_COMPARE(calls(script(), "discard-phase").size(), 1);
    QCOMPARE(calls(script(), "discard-phase").first()["phase"].toInt(), 3);
    // All tasks decided: the header waits for the lead and Finish asks for a
    // bounded summary.
    auto waiting = phasedTeam();
    auto tasks = waiting["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    waiting["tasks"] = tasks;
    waiting["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"approval", "review"},
                                               {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    waiting["pending_phase"] = QJsonValue();
    waiting["waiting_for_lead"] = true;
    // A request may already be in flight; keep polling until the new team shows.
    setTeam(waiting);
    WAIT_VERIFY(([&] { controller.refresh(); return workspace.findChild<QWidget *>("teamBarWaitingForLead") != nullptr; })());
    QVERIFY(!workspace.findChild<QWidget *>("teamBarPendingPhase"));
    WAIT_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceFinish"));
    shot(&workspace, "d3-phases-waiting");
    workspace.findChild<QPushButton *>("teamWorkspaceFinish")->click();
    auto *dialog = panel.findChild<QDialog *>("teamFinishDialog");
    WAIT_VERIFY(dialog);
    // Cancelling deletes the dialog instead of leaving it hidden under the panel.
    dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Cancel)->click();
    WAIT_SETTLED();
    QCOMPARE(panel.findChildren<QDialog *>("teamFinishDialog").size(), 0);
    workspace.findChild<QPushButton *>("teamWorkspaceFinish")->click();
    dialog = panel.findChild<QDialog *>("teamFinishDialog");
    WAIT_VERIFY(dialog);
    auto *summary = dialog->findChild<QPlainTextEdit *>("teamFinishSummary");
    auto *finish = dialog->findChild<QPushButton *>("teamFinishConfirm");
    QVERIFY(summary && finish);
    QVERIFY(!finish->isEnabled());   // a summary is required
    summary->setPlainText(QString(4001, QLatin1Char('x')));
    WAIT_VERIFY(!finish->isEnabled());   // and bounded at 4,000 characters
    // Characters, not UTF-16 units: 4,000 emoji are accepted, 4,001 are not.
    const QString emoji = QString(QChar(0xD83D)) + QChar(0xDE00);
    QString many;
    many.reserve(8000);
    for (int i = 0; i < 4000; ++i) many += emoji;
    summary->setPlainText(many);
    WAIT_VERIFY(finish->isEnabled());
    summary->setPlainText(many + emoji);
    WAIT_VERIFY(!finish->isEnabled());
    summary->setPlainText(QStringLiteral("Phases 1–2 are delivered; the retest is #11."));
    WAIT_VERIFY(finish->isEnabled());
    finish->click();
    WAIT_COMPARE(calls(script(), "finish").size(), 1);
    QCOMPARE(calls(script(), "finish").first()["summary"].toString(), QStringLiteral("Phases 1–2 are delivered; the retest is #11."));
    QCOMPARE(calls(script(), "finish").first()["team"].toString(), QStringLiteral("team-1"));
}

void TestTeamsUiPlan::phaseApprovalAlongsideARunningPhaseAsksFirst()
{
    // F12/N40: a draft behind an approved phase that still runs stays
    // approvable; the owner is warned before it runs alongside, Cancel sends
    // nothing, Approve now sends one approve-phase.
    auto t = phasedTeam();   // phase 2 approved with #4 in progress, phase 3 draft
    auto phases = t["phases"].toArray();
    auto polish = phases[1].toObject();
    polish["waiting"] = QJsonObject{{"text", "Can run alongside phase 2"}, {"phase", 2},
                                    {"open", QJsonArray{4}}, {"alongside", true}};
    phases[1] = polish;
    t["phases"] = phases;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820);
    workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The model neither blocks the phase nor the band; it names the phase it
    // runs beside.
    const auto along = parse(t);
    QCOMPARE(approveBlocked(along, 3), QString());
    QCOMPARE(alongsidePhase(along, 3), 2);
    bool bandSeen = false;
    for (const auto &band : TeamUi::phaseBands(along, [](const PhaseGroup &) { return true; }))
        if (band.n == 3) { bandSeen = true; QVERIFY2(band.blocked.isEmpty(), qPrintable(band.blocked)); QVERIFY(band.alongside); }
    QVERIFY(bandSeen);
    // The phase's own Accept stays enabled with the alongside hint: the same
    // button the strip and the session header carry.
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY2(phaseAction(&workspace, 3, "teamPhaseAccept")->toolTip().contains(QStringLiteral("alongside phase 2")),
             qPrintable(phaseAction(&workspace, 3, "teamPhaseAccept")->toolTip()));
    // Shots of the draft with its approvable phase, both themes.
    workspace.render();
    QTest::qWait(150);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "ux-approve-alongside");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "ux-approve-alongside-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // Cancel: the warning names the cost and nothing is sent.
    QString name, text;
    armConfirmRecord(&panel, true, &name, &text);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTest::qWait(150);
    QCOMPARE(name, QStringLiteral("teamApproveAlongsideConfirm"));
    QVERIFY2(text.contains(QStringLiteral("approve phase 3 now")), qPrintable(text));
    QVERIFY2(text.contains(QStringLiteral("while phase 2 still runs")), qPrintable(text));
    QVERIFY2(text.contains(QStringLiteral("can no longer edit this phase")), qPrintable(text));
    QVERIFY2(text.contains(QStringLiteral("Are you sure?")), qPrintable(text));
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    // Approve now: one request, for phase 3.
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    armConfirmRecord(&panel, false, nullptr, nullptr);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "approve-phase").first()["team"].toString(), QStringLiteral("team-1"));
}

void TestTeamsUiPlan::aRefreshUnderThePhaseConfirmationKeepsItsAsker()
{
    // The installed build crashed after Approve: a poll under the dialog
    // rebuilt the Team tab, deleted the control that asked and replaced the
    // team it held. While a confirmation is open nothing is rebuilt; afterwards
    // the team is read again and the views catch up.
    auto ready = phasedTeam();
    auto readyTasks = ready["tasks"].toArray();
    for (int i = 0; i < readyTasks.size(); ++i) {
        auto task = readyTasks[i].toObject();
        if (task["phase"].toInt() == 2) task["status"] = "accepted";
        readyTasks[i] = task;
    }
    ready["tasks"] = readyTasks;
    setTeam(ready);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    workspace.render();
    QTest::qWait(100);
    auto *accept = phaseAction(&workspace, 3, "teamPhaseAccept");
    QVERIFY(accept && accept->isEnabled());
    const QPointer<QWidget> asker(accept);
    bool aliveUnderDialog = false;
    armConfirmAfter(&panel, [&] {
        // A poll lands and both views try to rebuild under the dialog.
        controller.refresh();
        QTest::qWait(300);
        workspace.render();
        QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
        QTest::qWait(50);
        aliveUnderDialog = !asker.isNull();
    });
    accept->click();
    QVERIFY(aliveUnderDialog);
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    // After the dialog the views rebuild once and the decision holds.
    QTRY_VERIFY(asker.isNull());
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && !phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
}

void TestTeamsUiPlan::phaseProgressShowsTwoBars()
{
    auto phased = phasedTeam();
    // Phase 1 is decided, phase 2 is in work (one of its two tasks done) and
    // phase 3 waits as a draft.
    setTeam(phased);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(420, 820); panel.show();
    panel.showTeam("team-1");
    QTRY_VERIFY(!panel.findChildren<QWidget *>("teamPhaseProgress").isEmpty());
    const auto check = [&](const char *shotName) {
        const auto bars = panel.findChildren<QWidget *>("teamPhaseProgress");
        QVERIFY(!bars.isEmpty());
        const auto *bar = bars.first();
        QCOMPARE(bar->property("phase").toInt(), 2);
        QCOMPARE(bar->property("phaseDone").toInt(), 1);
        QCOMPARE(bar->property("phaseTotal").toInt(), 2);
        QCOMPARE(bar->property("phaseDraft").toBool(), false);
        QCOMPARE(bar->property("phasesDone").toInt(), 1);
        QCOMPARE(bar->property("phasesTotal").toInt(), 3);
        QVERIFY(bar->toolTip().contains(QStringLiteral("Phase 2")));
        QVERIFY(bar->toolTip().contains(QStringLiteral("1 of 3 phases")));
        shot(&panel, shotName);
    };
    check("team-progress-420-dark");
    lightPalette();
    panel.setTheme(false); panel.render();
    QTRY_VERIFY(!panel.findChildren<QWidget *>("teamPhaseProgress").isEmpty());
    check("team-progress-420-light");
    panel.resize(290, 820);
    panel.render();
    QTRY_VERIFY(!panel.findChildren<QWidget *>("teamPhaseProgress").isEmpty());
    check("team-progress-290-light");
    darkPalette();
    panel.setTheme(true); panel.render();
    QTRY_VERIFY(!panel.findChildren<QWidget *>("teamPhaseProgress").isEmpty());
    check("team-progress-290-dark");
    // No phase in work: the bars follow the next draft waiting for approval.
    auto waiting = phased;
    auto waitingTasks = waiting["tasks"].toArray();
    for (int i = 0; i < waitingTasks.size(); ++i) {
        auto task = waitingTasks[i].toObject();
        if (task["phase"].toInt(1) == 2) task["status"] = "accepted";
        waitingTasks[i] = task;
    }
    waiting["tasks"] = waitingTasks;
    setTeam(waiting);
    QTRY_VERIFY(([&] {
        controller.refresh();
        const auto bars = panel.findChildren<QWidget *>("teamPhaseProgress");
        return !bars.isEmpty() && bars.first()->property("phase").toInt() == 3 && bars.first()->property("phaseDraft").toBool();
    })());
    QVERIFY(panel.findChildren<QWidget *>("teamPhaseProgress").first()->toolTip().contains(QStringLiteral("waiting for your approval")));
    shot(&panel, "team-progress-draft-dark");
}

void TestTeamsUiPlan::preflightWarningsShowInThePlanCard()
{
    auto planning = phasedTeam();
    planning["status"] = "planning";
    planning["validation"] = QJsonObject{{"errors", QJsonArray()},
        {"warnings", QJsonArray{QJsonObject{{"kind", "sandbox_worktree"},
            {"text", "Preflight: Backend runs in a worktree but its agent cannot write git metadata; commits will ask"}}}}};
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(460, 900); panel.show();
    panel.showTeam("team-1");
    QTRY_VERIFY(([&] {
        for (auto *label : panel.findChildren<QLabel *>())
            if (label->text().contains(QStringLiteral("Preflight:"))) return true;
        return false;
    })());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&panel, "preflight-warning-dark");
        panel.setTheme(false);
        lightShot(&panel, "preflight-warning-light");
        panel.setTheme(true); darkPalette();
    }
}

void TestTeamsUiPlan::leadLoadViewsShowInTheDesktop()
{
    auto load = phasedTeam();
    load["status"] = "running";
    load["approved_by"] = "Lead";
    load["owner_word"] = "план принят, делай";
    auto tasks = load["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() == 2)
            task["waits"] = QJsonObject{{"for", "lead"}, {"on", QJsonArray{"Architect"}}, {"unblock", "accept #2"}};
        if (task["id"].toInt() == 4) {
            task["kind"] = "check";
            task["check"] = QJsonObject{{"command", "cargo test --locked --bin hgs teams"}, {"expect_exit", 0}, {"timeout_minutes", 30}};
            task["check_result"] = QJsonObject{{"exit", 0}, {"tail", "120 passed"}};
        }
        tasks[i] = task;
    }
    load["tasks"] = tasks;
    auto members = load["members"].toArray();
    auto backend = members[1].toObject();
    backend["services"] = QJsonArray{QJsonObject{{"name", "qdrant"}, {"command", "qdrant --mcp"}, {"since", 1792868400.0}}};
    backend["budget"] = QJsonObject{{"label", "socraticode"}, {"minutes", 20}, {"red_minutes", 40}, {"since", 1792868400.0},
                                    {"elapsed_minutes", 47}, {"state", "red"}, {"command", "cargo build --release"}};
    members[1] = backend; load["members"] = members;
    load["attention"] = QJsonArray{QJsonObject{{"kind", "report"}, {"task", 2}, {"member", "Frontend"}, {"since", 1792868400.0}, {"minutes", 70}}};
    auto phases = load["phases"].toArray();
    auto draftPhase = phases[1].toObject();   // the draft (phase 3)
    draftPhase["waiting"] = QJsonObject{{"text", "Can run alongside phase 2"}, {"phase", 2}, {"open", QJsonArray{3, 4}},
                                        {"alongside", true}};
    phases[1] = draftPhase; load["phases"] = phases;
    QJsonObject loadMember{{"Backend", QJsonObject{{"tasks", 3}, {"on_critical_path", 2}}}};
    QJsonObject loadStarved{{"member", "tester"}, {"role", "Tester"}, {"minutes", 25}, {"blocking", QJsonArray{4}}};
    QJsonObject loadHealth{{"tasks", 7}, {"critical_path", 3}, {"parallelism", 2.3}, {"widest_ready", 4}};
    loadHealth["per_member"] = loadMember;
    loadHealth["fan_in"] = QJsonArray{QJsonObject{{"task", 5}, {"waits_for", QJsonArray{3, 4}}, {"kind", "review"}}};
    loadHealth["warnings"] = QJsonArray{"Parallelism is below 1.5"};
    loadHealth["starved"] = QJsonArray{loadStarved};
    load["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}, {"health", loadHealth}};
    setTeam(load);
    const auto parsed = Teams::parse(load);
    const auto live = [](const Teams::Member &) { return Teams::Live{}; };
    // The card: a check's command and result, and what an open task waits for.
    TeamTaskCard card;
    card.resize(440, 1100); card.show();
    card.setTask(parsed, 4, live, true);
    QVERIFY(card.findChild<QLabel *>("teamCheckCommand"));
    QCOMPARE(card.findChild<QLabel *>("teamCheckCommand")->text(), QStringLiteral("cargo test --locked --bin hgs teams"));
    QVERIFY(card.findChild<QLabel *>("teamCheckResult"));
    QCOMPARE(card.findChild<QLabel *>("teamCheckResult")->text(), QStringLiteral("exit 0"));
    shot(&card, "lead-load-card");
    card.setTask(parsed, 2, live, true);
    QVERIFY(card.findChild<QLabel *>("teamTaskWait"));
    QVERIFY(card.findChild<QLabel *>("teamTaskWait")->text().contains(QStringLiteral("accept #2")));
    // The plan card: health numbers, the starving member and the owner's word.
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(460, 1100); panel.show(); panel.showTeam("team-1");
    auto planning = load; planning["status"] = "planning";
    // L6: the same starved member twice and a warning that names it must not
    // become three lines.
    planning["validation"] = load["validation"].toObject();
    auto planningHealth = planning["validation"].toObject()["health"].toObject();
    planningHealth["starved"] = QJsonArray{
        QJsonObject{{"member", "tester"}, {"role", "Tester"}, {"minutes", 25}, {"blocking", QJsonArray{4}}},
        QJsonObject{{"member", "tester"}, {"role", "Tester"}, {"minutes", 25}, {"blocking", QJsonArray{4}}}};
    planningHealth["warnings"] = QJsonArray{"Tester has been starved for 25 min", "Parallelism is below 1.5"};
    auto planningValidation = planning["validation"].toObject(); planningValidation["health"] = planningHealth;
    planning["validation"] = planningValidation;
    setTeam(planning);
    QTRY_VERIFY(([&] { controller.refresh(); return panel.findChild<QWidget *>("teamPlanHealth") != nullptr; })());
    QVERIFY(panel.findChild<QLabel *>("teamPlanHealthNumbers")->text().contains(QStringLiteral("critical path 3")));
    QVERIFY(panel.findChild<QLabel *>("teamPlanStarved"));
    QCOMPARE(panel.findChildren<QLabel *>("teamPlanStarved").size(), 1);
    QVERIFY(panel.findChild<QLabel *>("teamPlanHealthWarning"));
    QVERIFY(!panel.findChild<QLabel *>("teamPlanHealthWarning")->text().contains(QStringLiteral("starved")));
    QVERIFY(panel.findChild<QLabel *>("teamOwnerWord"));
    QVERIFY(panel.findChild<QLabel *>("teamOwnerWord")->text().contains(QStringLiteral("план принят, делай")));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&panel, "lead-load-plan");
        panel.setTheme(false);
        lightShot(&panel, "lead-load-plan-light");
        panel.setTheme(true); darkPalette(); QTest::qWait(50);
    }
    // The workspace: F12 wording on the draft, services and budget on the
    // member, and the attention strip.
    setTeam(load);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    workspace.showTeam("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); return phaseScrollOf(&workspace, 3) != nullptr; })());
    // N40: the draft's header no longer blocks it - it can run alongside phase
    // 2 - so it carries no waiting sentence and its Accept stays enabled.
    QTRY_VERIFY(phaseScrollOf(&workspace, 3) && phaseAction(&workspace, 3, "teamPhaseAccept"));
    QVERIFY(!phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseWaiting"));
    QVERIFY(phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY2(phaseAction(&workspace, 3, "teamPhaseAccept")->toolTip().contains(QStringLiteral("alongside phase 2")),
             qPrintable(phaseAction(&workspace, 3, "teamPhaseAccept")->toolTip()));
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceMemberServices"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceMemberBudget"));
    // Owner, 2026-10-28: the attention list is the first group of the side
    // column, not a strip over the canvas.
    QVERIFY(workspace.findChild<QWidget *>("teamAttentionStrip"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceSide")->isAncestorOf(workspace.findChild<QWidget *>("teamAttentionStrip")));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "lead-load-workspace");
        // The light halves of the lead-load views: card, plan and workspace.
        card.setTask(parsed, 2, live, false);
        QTest::qWait(100);
        lightShot(&card, "lead-load-card-light");
        workspace.setTheme(false);
        lightShot(&workspace, "lead-load-workspace-light");
        workspace.setTheme(true); darkPalette();
        QTest::qWait(50);
    }
}

void TestTeamsUiPlan::severalDraftPhasesRenderAndApproveInOrder()
{
    auto roadmap = phasedTeam();
    roadmap["pending_phase"] = QJsonValue();
    roadmap["pending_phases"] = QJsonArray{2, 3, 4};
    roadmap["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "draft"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{6}}}};
    auto tasks = roadmap["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        task["status"] = task["phase"].toInt(1) > 1 ? "draft" : "accepted";
        task.remove("attempts");
        tasks[i] = task;
    }
    tasks.append(QJsonObject{{"id", 6}, {"phase", 4}, {"title", "Write the docs"}, {"assignee", "backend"}, {"status", "draft"}});
    roadmap["tasks"] = tasks;
    setTeam(roadmap);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    // Spec 10: three drafts, one band each; only the next one can be approved.
    QTRY_COMPARE(([&] { auto *pending = workspace.findChild<QWidget *>("teamBarPendingPhase"); return pending ? pending->toolTip() : QString(); })(),
                 QStringLiteral("phases 2, 3, 4 pending approval"));
    // Several drafts render as scrolls in the graph, the layers and the board;
    // only the next one can be accepted, the others say what they wait for.
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 4);
    for (int n = 2; n <= 4; ++n) QCOMPARE(phaseScrollOf(&workspace, n)->property("state").toString(), QStringLiteral("draft"));
    QTRY_VERIFY(phaseAction(&workspace, 2, "teamPhaseAccept") && phaseAction(&workspace, 2, "teamPhaseAccept")->isEnabled());
    QVERIFY(!phaseScrollOf(&workspace, 2)->findChild<QLabel *>("teamPhaseWaiting"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseWaiting")->text(), QStringLiteral("after phase 2"));
    QVERIFY(phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseWaiting")->toolTip().contains(QStringLiteral("Phase 3 opens after phase 2 is approved")));
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QCOMPARE(phaseScrollOf(&workspace, 4)->findChild<QLabel *>("teamPhaseWaiting")->text(), QStringLiteral("after phase 3"));
    QVERIFY(phaseAction(&workspace, 4, "teamPhaseAccept")->toolTip().contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    // The scrolls stack top to bottom, aligned.
    int lastTop = -1;
    for (int n = 1; n <= 4; ++n) {
        auto *frame = phaseScrollOf(&workspace, n);
        QVERIFY(frame->width() > 100 && frame->y() > lastTop);
        QCOMPARE(frame->x(), phaseScrollOf(&workspace, 1)->x());
        lastTop = frame->y();
    }
    shot(&workspace, "roadmap-drafts-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTest::qWait(200);
    QTRY_VERIFY(phaseGraphOf(&workspace, 2));
    shot(&workspace, "roadmap-graph-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    QTest::qWait(150);
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "layers") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 4);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 4);
    shot(&workspace, "roadmap-board-dark");
    // Phase 2 accepted and its tasks decided: phase 3 can be accepted, phase 4 still waits.
    auto next = roadmap;
    next["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{6}}}};
    auto nextTasks = next["tasks"].toArray();
    for (int i = 0; i < nextTasks.size(); ++i) {
        auto task = nextTasks[i].toObject();
        if (task["phase"].toInt() == 2) task["status"] = "accepted";
        nextTasks[i] = task;
    }
    next["tasks"] = nextTasks;
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "graph") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspacePhases"));
    setTeam(next);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *third = phaseScrollOf(&workspace, 3), *fourth = phaseScrollOf(&workspace, 4);
        return third && fourth && !third->findChild<QLabel *>("teamPhaseWaiting") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled()
               && fourth->findChild<QLabel *>("teamPhaseWaiting") && fourth->findChild<QLabel *>("teamPhaseWaiting")->text() == QStringLiteral("after phase 3");
    })());
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTRY_VERIFY(phaseScrollOf(&workspace, 3));
    shot(&workspace, "roadmap-drafts-light");
    darkPalette();
    workspace.setTheme(true);
}

void TestTeamsUiPlan::phaseBandsStackWithStatusDecisionAndHealth()
{
    auto t = phasedTeam();
    t["status"] = "running";
    t["pending_phase"] = QJsonValue();
    t["pending_phases"] = QJsonArray{5};
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "approved"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{7}},
                    {"waiting", QJsonObject{{"text", "Can run alongside phase 4"}, {"phase", 4},
                                            {"open", QJsonArray{6}}, {"alongside", true}}},
                    {"changes", QJsonArray{QJsonObject{{"kind", "added"}, {"task", 7}, {"title", "Screenshots"}},
                                           QJsonObject{{"kind", "changed"}, {"task", 7}, {"title", "Screenshots"}, {"from", "backend"}, {"to", "tester"}}}}}};
    t["tasks"] = QJsonArray{
        QJsonObject{{"id", 1}, {"phase", 1}, {"title", "Requirements"}, {"assignee", "lead"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "lead"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 2}, {"phase", 1}, {"title", "Checkout design"}, {"assignee", "backend"}, {"status", "accepted"}},
        QJsonObject{{"id", 3}, {"phase", 2}, {"title", "Address form UI"}, {"assignee", "backend"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "backend"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 4}, {"phase", 2}, {"title", "Unit tests"}, {"assignee", "tester"}, {"status", "accepted"}},
        QJsonObject{{"id", 5}, {"phase", 3}, {"title", "Code review"}, {"assignee", "backend"}, {"status", "in_progress"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a5"}, {"member", "backend"}, {"state", "delivered"}}}}},
        QJsonObject{{"id", 6}, {"phase", 4}, {"title", "Release notes"}, {"assignee", "backend"}, {"status", "assigned"}},
        QJsonObject{{"id", 7}, {"phase", 5}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"}}};
    t["phase_summary"] = QJsonArray{
        QJsonObject{{"n", 1}, {"title", QString()}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", false}, {"health_line", "path 2 · par 1.0"}},
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", true}, {"health_line", "path 2 · par 1.0 · 1 warning"}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"status", "in_work"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"status", "approved"}, {"done", 0}, {"total", 1}, {"barrier", true}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"status", "draft"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", ""}}};
    t["phases_health"] = QJsonObject{
        {"1", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"2", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray{"Backend holds 2 of 2"}}}},
        {"3", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"4", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"5", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}}};
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 1000); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 5);
    workspace.render();
    QTest::qWait(100);
    // U25: every state reads from phase_summary (an older hgs's G9 status: an
    // approved phase that has not started waits as queued).
    const auto state = [&](int n) { auto *f = phaseScrollOf(&workspace, n); return f ? f->property("state").toString() : QString(); };
    QTRY_COMPARE(state(1), QStringLiteral("done"));
    QTRY_COMPARE(state(2), QStringLiteral("done"));
    QTRY_COMPARE(state(3), QStringLiteral("running"));
    QTRY_COMPARE(state(4), QStringLiteral("queued"));
    QTRY_COMPARE(state(5), QStringLiteral("draft"));
    // The health tip of a running phase, its done/total bar.
    QVERIFY(phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseHealth"));
    QCOMPARE(phaseScrollOf(&workspace, 4)->findChild<QWidget *>("teamPhaseProgress")->property("total").toInt(), 1);
    QCOMPARE(phaseScrollOf(&workspace, 4)->findChild<QWidget *>("teamPhaseProgress")->property("done").toInt(), 0);
    // The scrolls stack: same left edge, increasing top.
    int lastTop = -1;
    for (int n = 1; n <= 5; ++n) {
        auto *frame = phaseScrollOf(&workspace, n);
        QVERIFY(frame->width() > 200 && frame->y() > lastTop);
        QCOMPARE(frame->x(), phaseScrollOf(&workspace, 1)->x());
        lastTop = frame->y();
    }
    // N40: the draft runs alongside phase 4, so it carries no waiting sentence
    // and its owner decisions stay usable; the lead's changes read as a diff.
    QVERIFY(!phaseScrollOf(&workspace, 5)->findChild<QLabel *>("teamPhaseWaiting"));
    QVERIFY(phaseAction(&workspace, 5, "teamPhaseAccept") && phaseAction(&workspace, 5, "teamPhaseAccept")->isEnabled());
    QVERIFY(phaseScrollOf(&workspace, 5)->findChild<QLabel *>("teamPhaseChanges")->toolTip().contains(QStringLiteral("+ #7 Screenshots\n~ #7 Screenshots: backend → tester")));
    // Default: finished phases start collapsed; the rest are open (both states
    // in one picture, dark and light).
    QVERIFY(!phaseOpen(&workspace, 1) && !phaseOpen(&workspace, 2) && phaseOpen(&workspace, 3));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "g9-statuses-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "g9-statuses-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // All expanded.
    for (int n = 1; n <= 2; ++n) {
        QTRY_VERIFY(phaseScrollOf(&workspace, n) && !phaseOpen(&workspace, n));
        togglePhase(&workspace, n);
        QTRY_VERIFY(phaseOpen(&workspace, n));
    }
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "g9-statuses-expanded-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "g9-statuses-expanded-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // All collapsed: one compact header row per scroll.
    workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->click();
    QTRY_VERIFY(([&] { for (int n = 1; n <= 5; ++n) if (!phaseScrollOf(&workspace, n) || phaseOpen(&workspace, n)) return false; return true; })());
    for (int n = 1; n <= 5; ++n) QVERIFY(phaseScrollOf(&workspace, n)->height() <= 46);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "g9-statuses-collapsed-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "g9-statuses-collapsed-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->click();
    QTRY_VERIFY(phaseScrollOf(&workspace, 3) && phaseOpen(&workspace, 3));
    // The paused phase: the team is Paused, so its in-work phase says Paused.
    auto paused = t;
    paused["status"] = "paused";
    auto summary = paused["phase_summary"].toArray();
    auto pausedSummary = summary[2].toObject(); pausedSummary["status"] = "paused"; summary[2] = pausedSummary;
    paused["phase_summary"] = summary;
    setTeam(paused);
    QTRY_VERIFY(([&] { controller.refresh(); return state(3) == QStringLiteral("paused"); })());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "g9-paused-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "g9-paused-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // A ready draft: Accept and Discard ask the owner first, then send once.
    auto ready = t;
    ready["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "approved"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"state", "approved"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{7}}},
        QJsonObject{{"n", 6}, {"title", "Screenshots"}, {"state", "draft"}, {"tasks", QJsonArray{8}}}};
    auto readyTasks = ready["tasks"].toArray();
    for (int i = 0; i < readyTasks.size(); ++i) {
        auto task = readyTasks[i].toObject();
        if (task["phase"].toInt(1) < 5) task["status"] = "accepted";
        readyTasks[i] = task;
    }
    readyTasks.append(QJsonObject{{"id", 8}, {"phase", 6}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"}});
    ready["tasks"] = readyTasks;
    auto readySummary = ready["phase_summary"].toArray();
    auto readyFourth = readySummary[3].toObject(); readyFourth["status"] = "done"; readyFourth["done"] = 1; readySummary[3] = readyFourth;
    auto readyLast = readySummary[4].toObject(); readyLast["status"] = "draft"; readySummary[4] = readyLast;
    ready["phase_summary"] = readySummary;
    setTeam(ready);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *accept = phaseAction(&workspace, 5, "teamPhaseAccept");
        return accept && accept->isEnabled() && !phaseScrollOf(&workspace, 5)->findChild<QLabel *>("teamPhaseWaiting");
    })());
    armConfirm(&panel, false);
    phaseAction(&workspace, 5, "teamPhaseAccept")->click();
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 5);
    // One decision is in flight at a time: wait for the answer and the
    // re-render before the next one.
    QTest::qWait(300);
    workspace.render();
    QTest::qWait(100);
    // Discard on the other draft: Cancel sends nothing, the confirmation sends once with phase 6.
    QTRY_VERIFY(phaseScrollOf(&workspace, 6) && phaseScrollOf(&workspace, 6)->property("state") == "draft");
    const auto discardSix = [&] {
        chooseMenuAction("teamPhaseDiscard");
        phaseScrollOf(&workspace, 6)->findChild<QToolButton *>("teamPhaseMore")->click();
    };
    armConfirm(&panel, true);
    discardSix();
    QTest::qWait(200);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    armConfirm(&panel, false);
    discardSix();
    QTRY_COMPARE(calls(script(), "discard-phase").size(), 1);
    QCOMPARE(calls(script(), "discard-phase").first()["phase"].toInt(), 6);
    // Both decisions hold at this revision: phase 5's Accept did not come back
    // when phase 6 was decided.
    QTest::qWait(300);
    workspace.render();
    QTest::qWait(100);
    QTRY_VERIFY(phaseAction(&workspace, 5, "teamPhaseAccept") && !phaseAction(&workspace, 5, "teamPhaseAccept")->isEnabled());
    QVERIFY(!phaseScrollOf(&workspace, 6)->findChild<QToolButton *>("teamPhaseMore")->isEnabled());
    phaseAction(&workspace, 5, "teamPhaseAccept")->click();
    QTest::qWait(80);
    QCOMPARE(calls(script(), "approve-phase").size(), 1);
}

// U39 (owner, 2026-10-28): an accepted phase that has not started (here
// queued) carries Discard phase… in its menu; it asks first and sends
// its number.
void TestTeamsUiPlan::aQueuedPhaseCanBeDiscarded()
{
    auto t = phasedTeam();
    t["status"] = "running";
    t["pending_phase"] = QJsonValue();
    t["pending_phases"] = QJsonArray{5};
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "approved"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{7}},
                    {"waiting", QJsonObject{{"text", "Can run alongside phase 4"}, {"phase", 4},
                                            {"open", QJsonArray{6}}, {"alongside", true}}},
                    {"changes", QJsonArray{QJsonObject{{"kind", "added"}, {"task", 7}, {"title", "Screenshots"}},
                                           QJsonObject{{"kind", "changed"}, {"task", 7}, {"title", "Screenshots"}, {"from", "backend"}, {"to", "tester"}}}}}};
    t["tasks"] = QJsonArray{
        QJsonObject{{"id", 1}, {"phase", 1}, {"title", "Requirements"}, {"assignee", "lead"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "lead"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 2}, {"phase", 1}, {"title", "Checkout design"}, {"assignee", "backend"}, {"status", "accepted"}},
        QJsonObject{{"id", 3}, {"phase", 2}, {"title", "Address form UI"}, {"assignee", "backend"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "backend"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 4}, {"phase", 2}, {"title", "Unit tests"}, {"assignee", "tester"}, {"status", "accepted"}},
        QJsonObject{{"id", 5}, {"phase", 3}, {"title", "Code review"}, {"assignee", "backend"}, {"status", "in_progress"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a5"}, {"member", "backend"}, {"state", "delivered"}}}}},
        QJsonObject{{"id", 6}, {"phase", 4}, {"title", "Release notes"}, {"assignee", "backend"}, {"status", "assigned"}},
        QJsonObject{{"id", 7}, {"phase", 5}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"}}};
    t["phase_summary"] = QJsonArray{
        QJsonObject{{"n", 1}, {"title", QString()}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", false}, {"health_line", "path 2 · par 1.0"}},
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", true}, {"health_line", "path 2 · par 1.0 · 1 warning"}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"status", "in_work"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"status", "approved"}, {"done", 0}, {"total", 1}, {"barrier", true}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"status", "draft"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", ""}}};
    t["phases_health"] = QJsonObject{
        {"1", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"2", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray{"Backend holds 2 of 2"}}}},
        {"3", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"4", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"5", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}}};
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 1000); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 5);
    workspace.render();
    QTest::qWait(100);
    // U25: every state reads from phase_summary (an older hgs's G9 status: an
    // approved phase that has not started waits as queued).
    const auto state = [&](int n) { auto *f = phaseScrollOf(&workspace, n); return f ? f->property("state").toString() : QString(); };
    QTRY_COMPARE(state(1), QStringLiteral("done"));
    QTRY_COMPARE(state(2), QStringLiteral("done"));
    QTRY_COMPARE(state(3), QStringLiteral("running"));
    QTRY_COMPARE(state(4), QStringLiteral("queued"));
    auto *more = phaseScrollOf(&workspace, 4)->findChild<QToolButton *>("teamPhaseMore");
    QVERIFY(more);
    armConfirm(&panel, false);
    chooseMenuAction("teamPhaseDiscard");
    more->click();
    QTRY_COMPARE(calls(script(), "discard-phase").size(), 1);
    QCOMPARE(calls(script(), "discard-phase").first()["phase"].toInt(), 4);
}

// U42 (#587): the same accepted, unstarted phase carries Return to draft… in
// its menu; it asks first and sends its number.
void TestTeamsUiPlan::aQueuedPhaseCanReturnToDraft()
{
    auto t = phasedTeam();
    t["status"] = "running";
    t["pending_phase"] = QJsonValue();
    t["pending_phases"] = QJsonArray{5};
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "approved"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"barrier", true}, {"state", "approved"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{7}},
                    {"waiting", QJsonObject{{"text", "Can run alongside phase 4"}, {"phase", 4},
                                            {"open", QJsonArray{6}}, {"alongside", true}}},
                    {"changes", QJsonArray{QJsonObject{{"kind", "added"}, {"task", 7}, {"title", "Screenshots"}},
                                           QJsonObject{{"kind", "changed"}, {"task", 7}, {"title", "Screenshots"}, {"from", "backend"}, {"to", "tester"}}}}}};
    t["tasks"] = QJsonArray{
        QJsonObject{{"id", 1}, {"phase", 1}, {"title", "Requirements"}, {"assignee", "lead"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "lead"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 2}, {"phase", 1}, {"title", "Checkout design"}, {"assignee", "backend"}, {"status", "accepted"}},
        QJsonObject{{"id", 3}, {"phase", 2}, {"title", "Address form UI"}, {"assignee", "backend"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "backend"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 4}, {"phase", 2}, {"title", "Unit tests"}, {"assignee", "tester"}, {"status", "accepted"}},
        QJsonObject{{"id", 5}, {"phase", 3}, {"title", "Code review"}, {"assignee", "backend"}, {"status", "in_progress"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a5"}, {"member", "backend"}, {"state", "delivered"}}}}},
        QJsonObject{{"id", 6}, {"phase", 4}, {"title", "Release notes"}, {"assignee", "backend"}, {"status", "assigned"}},
        QJsonObject{{"id", 7}, {"phase", 5}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"}}};
    t["phase_summary"] = QJsonArray{
        QJsonObject{{"n", 1}, {"title", QString()}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", false}, {"health_line", "path 2 · par 1.0"}},
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"status", "done"}, {"done", 2}, {"total", 2}, {"barrier", true}, {"health_line", "path 2 · par 1.0 · 1 warning"}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"status", "in_work"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 4}, {"title", "Review"}, {"status", "approved"}, {"done", 0}, {"total", 1}, {"barrier", true}, {"health_line", "path 1 · par 1.0"}},
        QJsonObject{{"n", 5}, {"title", "Docs"}, {"status", "draft"}, {"done", 0}, {"total", 1}, {"barrier", false}, {"health_line", ""}}};
    t["phases_health"] = QJsonObject{
        {"1", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"2", QJsonObject{{"tasks", 2}, {"critical_path", 2}, {"parallelism", 1.0}, {"warnings", QJsonArray{"Backend holds 2 of 2"}}}},
        {"3", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"4", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}},
        {"5", QJsonObject{{"tasks", 1}, {"critical_path", 1}, {"parallelism", 1.0}, {"warnings", QJsonArray()}}}};
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 1000); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 5);
    workspace.render();
    QTest::qWait(100);
    // U25: every state reads from phase_summary (an older hgs's G9 status: an
    // approved phase that has not started waits as queued).
    const auto state = [&](int n) { auto *f = phaseScrollOf(&workspace, n); return f ? f->property("state").toString() : QString(); };
    QTRY_COMPARE(state(1), QStringLiteral("done"));
    QTRY_COMPARE(state(2), QStringLiteral("done"));
    QTRY_COMPARE(state(3), QStringLiteral("running"));
    QTRY_COMPARE(state(4), QStringLiteral("queued"));
    auto *more = phaseScrollOf(&workspace, 4)->findChild<QToolButton *>("teamPhaseMore");
    QVERIFY(more);
    // A declined confirmation sends nothing; a confirmed one sends the phase.
    armConfirm(&panel, true);
    chooseMenuAction("teamPhaseReturnToDraft");
    more->click();
    QTest::qWait(300);
    QCOMPARE(calls(script(), "return-to-draft").size(), 0);
    // The workspace may have rebuilt its rows meanwhile: find the button again.
    more = phaseScrollOf(&workspace, 4)->findChild<QToolButton *>("teamPhaseMore");
    QVERIFY(more);
    armConfirm(&panel, false);
    chooseMenuAction("teamPhaseReturnToDraft");
    more->click();
    QTRY_COMPARE(calls(script(), "return-to-draft").size(), 1);
    QCOMPARE(calls(script(), "return-to-draft").first()["phase"].toInt(), 4);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
}

// G5/F10: the drafts wait as compact lines at the top of the members column,
// and their arrows move one among the others with move-phase. Seven drafts show
// the live numbers, the disabled ends and the renumbering in the feed.
void TestTeamsUiPlan::draftReorderSendsMovePhase()
{
    auto seven = phasedTeam();
    seven["pending_phase"] = 2;
    seven["pending_phases"] = QJsonArray{2, 3, 4, 5, 6, 7, 8};
    seven["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "draft"}, {"tasks", QJsonArray{3}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Release notes"}, {"state", "draft"}, {"tasks", QJsonArray{7}}},
        QJsonObject{{"n", 6}, {"title", "Screenshots"}, {"state", "draft"}, {"tasks", QJsonArray{8}}},
        QJsonObject{{"n", 7}, {"title", "Cleanup"}, {"state", "draft"}, {"tasks", QJsonArray{9}}},
        QJsonObject{{"n", 8}, {"title", "Archive"}, {"state", "draft"}, {"tasks", QJsonArray{10}}}};
    auto tasks = seven["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        task["status"] = task["phase"].toInt(1) > 1 ? "draft" : "accepted";
        task.remove("attempts");
        tasks[i] = task;
    }
    for (int id = 6; id <= 10; ++id)
        tasks.append(QJsonObject{{"id", id}, {"phase", id - 2}, {"title", QStringLiteral("Draft %1").arg(id)}, {"assignee", "backend"}, {"status", "draft"}});
    seven["tasks"] = tasks;
    setTeam(seven);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // U25: the drafts are scrolls stacked in order, each with its Accept,
    // Reject… and a menu that moves it among the drafts or discards it.
    WAIT_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 8);
    for (int n = 2; n <= 8; ++n) QCOMPARE(phaseScrollOf(&workspace, n)->property("state").toString(), QStringLiteral("draft"));
    QVERIFY(phaseScrollOf(&workspace, 2)->findChild<QLabel *>("teamPhaseTitle")->text().contains(QStringLiteral("QA fixes")));
    const auto fromMenu = [&](int phase, const char *action) {
        chooseMenuAction(action);
        phaseScrollOf(&workspace, phase)->findChild<QToolButton *>("teamPhaseMore")->click();
        WAIT_SETTLED();
    };
    // The first draft cannot go up.
    fromMenu(2, "teamPhaseMoveUp");
    QCOMPARE(calls(script(), "move-phase").size(), 0);
    fromMenu(3, "teamPhaseMoveUp");
    WAIT_COMPARE(calls(script(), "move-phase").size(), 1);
    auto moved = calls(script(), "move-phase").first();
    QCOMPARE(moved["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(moved["phase"].toInt(), 3);
    QCOMPARE(moved["to"].toInt(), 2);
    QCOMPARE(moved["position"].toString(), QStringLiteral("before"));
    WAIT_VERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 2)->findChild<QToolButton *>("teamPhaseMore"));
    fromMenu(2, "teamPhaseMoveDown");
    WAIT_COMPARE(calls(script(), "move-phase").size(), 2);
    moved = calls(script(), "move-phase")[1];
    QCOMPARE(moved["phase"].toInt(), 2);
    QCOMPARE(moved["to"].toInt(), 3);
    QCOMPARE(moved["position"].toString(), QStringLiteral("after"));
    // The store renumbers the drafts after a move: the scrolls follow the live
    // numbers and titles it sends, not positions cached when they were built.
    auto renumbered = seven;
    renumbered["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{5}}},
        QJsonObject{{"n", 3}, {"title", "QA fixes"}, {"state", "draft"}, {"tasks", QJsonArray{3}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{6}}},
        QJsonObject{{"n", 5}, {"title", "Release notes"}, {"state", "draft"}, {"tasks", QJsonArray{7}}},
        QJsonObject{{"n", 6}, {"title", "Screenshots"}, {"state", "draft"}, {"tasks", QJsonArray{8}}},
        QJsonObject{{"n", 7}, {"title", "Cleanup"}, {"state", "draft"}, {"tasks", QJsonArray{9}}},
        QJsonObject{{"n", 8}, {"title", "Archive"}, {"state", "draft"}, {"tasks", QJsonArray{10}}}};
    setTeam(renumbered);
    WAIT_VERIFY(([&] {
        controller.refresh();
        auto *second = phaseScrollOf(&workspace, 2);
        return second && second->findChild<QLabel *>("teamPhaseTitle")->text() == QStringLiteral("Phase 2 · Polish")
               && workspace.findChildren<QFrame *>("teamPhaseScroll").size() == 8;
    })());
    // QA fixes now waits for the draft that moved before it, at phase 2.
    auto *qaAccept = phaseAction(&workspace, 3, "teamPhaseAccept");
    QVERIFY(qaAccept && !qaAccept->isEnabled());
    QVERIFY(qaAccept->toolTip().contains(QStringLiteral("after phase 2")));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "phases-7-drafts-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        WAIT_SETTLED();
        shot(&workspace, "phases-7-drafts-light");
        darkPalette();
        workspace.setTheme(true);
    }
}

void TestTeamsUiPlan::phaseRetriesReuseTheRequestId()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.watch("team-1");
    QFile::remove(script() + ".calls");
    bool done = false, ok = true;
    const auto send = [&](int phase) {
        done = false; ok = false;
        controller.act("approve-phase", {{"team", "team-1"}, {"phase", phase}}, [&](bool success, const QString &) { done = true; ok = success; });
        QTRY_VERIFY(done);
    };
    send(9); QVERIFY(!ok);   // refused; the id stays for the next try
    send(9); QVERIFY(!ok);
    send(2); QVERIFY(ok);    // a success drops it
    send(2); QVERIFY(ok);    // so a later decision is a new request
    const auto sent = calls(script(), "approve-phase");
    QCOMPARE(sent.size(), 4);
    QCOMPARE(sent[0]["request_id"].toString(), sent[1]["request_id"].toString());
    QVERIFY(sent[2]["request_id"].toString() != sent[3]["request_id"].toString());
}

void TestTeamsUiPlan::malformedPhasesFallBack()
{
    auto raw = phasedTeam();
    auto tasks = raw["tasks"].toArray();
    auto zero = tasks[0].toObject(); zero["phase"] = 0; tasks[0] = zero;
    auto negative = tasks[1].toObject(); negative["phase"] = -4; tasks[1] = negative;
    raw["tasks"] = tasks;
    auto phases = raw["phases"].toArray();
    phases.append(QJsonObject{{"n", 0}, {"title", "Bad"}, {"state", "approved"}, {"tasks", QJsonArray{}}});
    raw["phases"] = phases;
    const auto team = parse(raw);
    QCOMPARE(team.task(1)->phase, 1);
    QCOMPARE(team.task(2)->phase, 1);
    QVERIFY(!team.phase(0));
    QCOMPARE(team.phases.size(), 2);   // the n=0 record is dropped
    for (const auto &group : phaseGroups(team)) QVERIFY(group.n >= 1);
}


// F7: a Done team starts the next goal with the same people; the new team
// opens, and the old one links to it.
void TestTeamsUiPlan::nextGoalFromADoneTeam()
{
    auto done = team();
    done["status"] = "done";
    auto tasks = done["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    done["tasks"] = tasks;
    done["previous"] = "team-0";
    setTeam(done);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceNextGoal"));
    QVERIFY(workspace.findChild<QPushButton *>("teamBarPrevious"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamBarNext"));
    workspace.findChild<QPushButton *>("teamWorkspaceNextGoal")->click();
    auto *dialog = workspace.findChild<QDialog *>("teamNextGoalDialog");
    QTRY_VERIFY(dialog && dialog->isVisible());
    auto *start = dialog->findChild<QPushButton *>("teamNextGoalStart");
    QVERIFY(!start->isEnabled());   // a goal first
    QCOMPARE(dialog->findChild<QLineEdit *>("teamNextGoalName")->text(), QStringLiteral("Checkout redesign"));
    dialog->findChild<QPlainTextEdit *>("teamNextGoalText")->setPlainText("Saved cards: store, list and delete them");
    dialog->findChild<QLineEdit *>("teamNextGoalName")->setText("Saved cards");
    QVERIFY(start->isEnabled());
    shot(dialog, "f7-next-goal-dialog");
    start->click();
    QTRY_COMPARE(calls(script(), "next-goal").size(), 1);
    const auto input = calls(script(), "next-goal").first();
    QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(input["goal"].toString(), QStringLiteral("Saved cards: store, list and delete them"));
    QCOMPARE(input["name"].toString(), QStringLiteral("Saved cards"));
    QCOMPARE(input["by"].toString(), QStringLiteral("human"));
    // The new team opens in the workspace.
    QTRY_COMPARE(controller.watchedId(), QStringLiteral("team-2"));
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamNextGoalDialog"));
    // The old team links to it and offers no second next goal.
    done["next"] = "team-2";
    setTeam(done);
    workspace.showTeam("team-1");
    controller.watch("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QPushButton *>("teamBarNext") != nullptr; })());
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceNextGoal"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "f7-next-goal-links");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "f7-next-goal-links-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
}

// U59: Start planning shows the lead's proposed mode and its reason; the owner
// chooses one of the two. With no choice the approval sends no mode (the team
// plans standard); a choice goes with the approval and shows on the button
// and the planned phase.
void TestTeamsUiPlan::planModeIsChosenWhenThePlanIsApproved()
{
    auto planning = team("planning");
    auto tasks = planning["tasks"].toArray();
    auto extra = tasks.last().toObject(); extra["assignee"] = "tester"; tasks[tasks.size() - 1] = extra;
    planning["tasks"] = tasks;
    planning["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
    planning["plan_mode"] = "standard";
    planning["plan_mode_offer"] = "free";
    planning["plan_mode_reason"] = "Support after 2.4 is a stream of small independent fixes from the QA bot, with no release date.";
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1400, 860); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamPlanModeCard"));
    auto *card = workspace.findChild<QFrame *>("teamPlanModeCard");
    // The lead's proposal and its reason; no choice reads Standard.
    auto *offer = card->findChild<QLabel *>("teamPlanModeOffer");
    QVERIFY(offer);
    QVERIFY(offer->text().contains("Lead proposes Free"));
    QVERIFY(offer->text().contains("stream of small independent fixes"));
    QCOMPARE(card->property("selected").toString(), QStringLiteral("standard"));
    const auto option = [&](const QString &mode) -> QFrame * {
        for (auto *frame : workspace.findChildren<QFrame *>("teamPlanModeOption")) if (frame->property("mode") == mode) return frame;
        return nullptr;
    };
    QVERIFY(option("standard") && option("standard")->property("selected").toBool());
    QVERIFY(option("free") && !option("free")->property("selected").toBool());
    QStringList tags; for (auto *tag : option("free")->findChildren<QLabel *>("teamPlanModeTag")) tags << tag->text();
    QCOMPARE(tags, QStringList{"Lead proposes"});
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceApprove") && workspace.findChild<QPushButton *>("teamWorkspaceApprove")->isEnabled());
    QCOMPARE(workspace.findChild<QPushButton *>("teamWorkspaceApprove")->text(), QStringLiteral("Approve && Run"));
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseMode"));
    QCOMPARE(workspace.findChild<QWidget *>("teamPhaseMode")->property("mode").toString(), QStringLiteral("standard"));
    // No dots as separators in what the card says.
    for (auto *text : card->findChildren<QLabel *>()) QVERIFY2(!text->text().contains(QChar(0x00b7)), qPrintable(text->text()));
    // No choice: the approval carries no mode, and hgs keeps standard.
    armConfirm(&panel, false);
    workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
    QTRY_COMPARE(calls(script(), "approve").size(), 1);
    QVERIFY(!calls(script(), "approve").first().contains("plan_mode"));
    // The owner picks Free: the card, the button and the planned phase say so.
    option("free")->findChild<QRadioButton *>("teamPlanModeRadio")->click();
    QTRY_COMPARE(workspace.findChild<QFrame *>("teamPlanModeCard")->property("selected").toString(), QStringLiteral("free"));
    QTRY_COMPARE(workspace.findChild<QPushButton *>("teamWorkspaceApprove")->text(), QStringLiteral("Approve && Run in free mode"));
    QTRY_COMPARE(workspace.findChild<QWidget *>("teamPhaseMode")->property("mode").toString(), QStringLiteral("free"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u59-start-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u59-start-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    // The panel's footer offers the same choice and button.
    panel.setLive(live);
    panel.resize(420, 1200); panel.show();
    QTRY_VERIFY(panel.findChild<QComboBox *>("teamPlanModeChoice"));
    QCOMPARE(panel.findChild<QComboBox *>("teamPlanModeChoice")->currentData().toString(), QStringLiteral("free"));
    QCOMPARE(panel.findChild<QPushButton *>("teamApprove")->text(), QStringLiteral("Approve && Run in free mode"));
    QString name, text;
    armConfirmRecord(&panel, false, &name, &text);
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceApprove"));
    workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
    QTRY_COMPARE(calls(script(), "approve").size(), 2);
    QCOMPARE(calls(script(), "approve").last()["plan_mode"].toString(), QStringLiteral("free"));
    QVERIFY2(text.contains("free planning"), qPrintable(text));
    // Back to Standard from the footer: the choice is sent as made.
    auto *combo = panel.findChild<QComboBox *>("teamPlanModeChoice");
    combo->setCurrentIndex(0); emit combo->activated(0);
    QCOMPARE(panel.planModeChoice("team-1"), QStringLiteral("standard"));
    armConfirm(&panel, false);
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceApprove"));
    workspace.render();
    workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
    QTRY_COMPARE(calls(script(), "approve").size(), 3);
    QCOMPARE(calls(script(), "approve").last()["plan_mode"].toString(), QStringLiteral("standard"));
    // An hgs without plan modes shows no card and sends no mode.
    planning.remove("plan_mode"); planning.remove("plan_mode_offer"); planning.remove("plan_mode_reason");
    setTeam(planning);
    QTRY_VERIFY(([&] { controller.refresh(); auto *t = controller.team("team-1"); return t && !t->planModeKnown; })());
    workspace.render();
    QTRY_VERIFY(!workspace.findChild<QFrame *>("teamPlanModeCard"));
}

// U59: the lead's request to switch to free opens one pop-up dialog with its
// reason, what changes, an optional reason and Switch to free / Keep standard;
// the answer is bound to the request's id and hash.
void TestTeamsUiPlan::switchToFreeOpensOneDialogBoundToTheRequest()
{
    auto running = phasedTeam();
    running["plan_mode"] = "standard";
    const QString hash = QStringLiteral("7f3a2c19e0d84b1c9a5e6f7081a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4");
    const double asked = double(QDateTime(QDate::currentDate(), QTime(11, 41)).toSecsSinceEpoch());
    running["proposals"] = QJsonArray{
        QJsonObject{{"id", "p5"}, {"kind", "switch-plan-mode"}, {"by", "Lead"}, {"at", asked}, {"state", "open"}, {"mode", "free"}, {"hash", hash},
                    {"reason", "The QA bot files 3-5 small fixes a day. Planning a phase for each costs more than the fixes."}}};
    setTeam(running);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1400, 860); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // It pops up by itself, once, however often the workspace renders.
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamSwitchModeDialog"));
    workspace.render(); workspace.render();
    QTest::qWait(100);
    QCOMPARE(workspace.findChildren<QDialog *>("teamSwitchModeDialog").size(), 1);
    auto *dialog = workspace.findChild<QDialog *>("teamSwitchModeDialog");
    QTRY_VERIFY(dialog->isVisible());
    QCOMPARE(dialog->property("proposal").toString(), QStringLiteral("p5"));
    QVERIFY(dialog->findChild<QLabel *>("teamSwitchModeRequest")->text().contains("Planning a phase for each"));
    QVERIFY(dialog->findChild<QLabel *>("teamSwitchModeRequest")->text().contains("Lead asks, 11:41"));
    QCOMPARE(dialog->findChildren<QWidget *>("teamSwitchModeChange").size(), 4);
    QVERIFY2(dialog->findChild<QLabel *>("teamSwitchModeUnchanged")->text().contains("the queued phase 3 stays as it is")
             || dialog->findChild<QLabel *>("teamSwitchModeUnchanged")->text().startsWith("Unchanged"),
             qPrintable(dialog->findChild<QLabel *>("teamSwitchModeUnchanged")->text()));
    QVERIFY(dialog->findChild<QLabel *>("teamSwitchModeBound")->text().contains("p5 (7f3a2c19)"));
    QVERIFY(dialog->findChild<QLineEdit *>("teamSwitchModeReason"));
    QVERIFY(dialog->findChild<QPushButton *>("teamSwitchModeKeep"));
    QCOMPARE(dialog->findChild<QPushButton *>("teamSwitchModeSwitch")->text(), QStringLiteral("Switch to free"));
    for (auto *text : dialog->findChildren<QLabel *>()) QVERIFY2(!text->text().contains(QChar(0x00b7)), qPrintable(text->text()));
    // U57: its card in the lead's conversation reopens the dialog (Details…).
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamProposalOpen"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamProposalAccept"));
    // The header names the mode beside the phase.
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamHeaderPlanMode"));
    QCOMPARE(workspace.findChild<QWidget *>("teamHeaderPlanMode")->property("mode").toString(), QStringLiteral("standard"));
    shot(dialog, "u59-switch-dark");
    // Closed without an answer it stays closed; Answer… opens it again.
    dialog->reject();
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamSwitchModeDialog"));
    workspace.render();
    QTest::qWait(100);
    QVERIFY(!workspace.findChild<QDialog *>("teamSwitchModeDialog"));
    workspace.setTheme(false); workspace.render();
    lightPalette();
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamProposalOpen"));
    workspace.findChild<QPushButton *>("teamProposalOpen")->click();
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamSwitchModeDialog"));
    dialog = workspace.findChild<QDialog *>("teamSwitchModeDialog");
    QTRY_VERIFY(dialog->isVisible());
    lightShot(dialog, "u59-switch-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
    // Keep standard with a reason: bound to p5 and its whole hash.
    dialog->findChild<QLineEdit *>("teamSwitchModeReason")->setText("We ship 2.5 next week");
    dialog->findChild<QPushButton *>("teamSwitchModeKeep")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    auto input = calls(script(), "decide-proposal").first();
    QCOMPARE(input["proposal"].toString(), QStringLiteral("p5"));
    QVERIFY(!input["accept"].toBool());
    QCOMPARE(input["expected_hash"].toString(), hash);
    QCOMPARE(input["reason"].toString(), QStringLiteral("We ship 2.5 next week"));
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamSwitchModeDialog"));
    // A new request pops its own dialog; Switch to free sends it without a reason.
    auto proposals = running["proposals"].toArray();
    auto first = proposals[0].toObject(); first["state"] = "rejected"; proposals[0] = first;
    const QString second = QStringLiteral("0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c");
    proposals.append(QJsonObject{{"id", "p6"}, {"kind", "switch-plan-mode"}, {"by", "Lead"}, {"at", asked + 600}, {"state", "open"}, {"mode", "free"},
                                 {"hash", second}, {"reason", "Three more fixes came in."}});
    running["proposals"] = proposals;
    setTeam(running);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *open = workspace.findChild<QDialog *>("teamSwitchModeDialog");
        return open && open->property("proposal").toString() == "p6";
    })());
    workspace.findChild<QDialog *>("teamSwitchModeDialog")->findChild<QPushButton *>("teamSwitchModeSwitch")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 2);
    input = calls(script(), "decide-proposal").last();
    QCOMPARE(input["proposal"].toString(), QStringLiteral("p6"));
    QVERIFY(input["accept"].toBool());
    QCOMPARE(input["expected_hash"].toString(), second);
    QVERIFY(!input.contains("reason"));
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamSwitchModeDialog"));
}

// U59: the mode shows beside the phase in the header, Graph and Board. The
// open free phase folds its accepted tasks, reads idle (not done) when its
// queue is empty, adds a build task on request and closes only after the
// owner confirms, building first when work was accepted since the last build.
void TestTeamsUiPlan::freePhaseReadsIdleBuildsAndClosesOnConfirmation()
{
    setTeam(freeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1400, 860); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(phaseScrollOf(&workspace, 2) && workspace.findChild<QWidget *>("teamHeaderBuild"));
    const auto modeOf = [&](int phase) {
        auto *frame = phaseScrollOf(&workspace, phase);
        auto *mode = frame ? frame->findChild<QWidget *>("teamPhaseMode") : nullptr;
        return mode ? mode->property("mode").toString() : QString();
    };
    const auto badgeOf = [&](int phase) {
        auto *frame = phaseScrollOf(&workspace, phase);
        auto *badge = frame ? dynamic_cast<TeamUi::PhaseBadge *>(frame->findChild<QWidget *>("teamPhaseBadge")) : nullptr;
        return badge ? badge->text() : QString();
    };
    // The header: Free, the last build and what was accepted since.
    QCOMPARE(workspace.findChild<QWidget *>("teamHeaderPlanMode")->property("mode").toString(), QStringLiteral("free"));
    QCOMPARE(workspace.findChild<QLabel *>("teamHeaderBuild")->text(), QStringLiteral("Last build 10:00, 1 accepted since"));
    // The phases: the closed one Standard, the open one Free and running.
    QCOMPARE(modeOf(1), QStringLiteral("standard"));
    QCOMPARE(modeOf(2), QStringLiteral("free"));
    QCOMPARE(badgeOf(2), QStringLiteral("Open, running"));
    QCOMPARE(phaseScrollOf(&workspace, 2)->property("state").toString(), QStringLiteral("running"));
    QVERIFY(!phaseScrollOf(&workspace, 2)->findChild<QToolButton *>("teamPhaseMore"));
    // Its accepted tasks fold into one line; Show brings them back.
    auto *fold = phaseScrollOf(&workspace, 2)->findChild<QWidget *>("teamFreeAccepted");
    QVERIFY(fold);
    QCOMPARE(fold->property("tasks").toInt(), 2);
    QCOMPARE(fold->findChild<QLabel *>("teamFreeAcceptedIds")->text(), QStringLiteral("#3 #6"));
    QCOMPARE(phaseNodeIds(phaseScrollOf(&workspace, 2)), (QList<int>{4, 7}));
    phaseAction(&workspace, 2, "teamFreeAcceptedShow")->click();
    QTRY_COMPARE(phaseNodeIds(phaseScrollOf(&workspace, 2)), (QList<int>{3, 4, 6, 7}));
    phaseAction(&workspace, 2, "teamFreeAcceptedShow")->click();
    QTRY_COMPARE(phaseNodeIds(phaseScrollOf(&workspace, 2)), (QList<int>{4, 7}));
    // Close phase waits for the running tasks and names them.
    auto *close = workspace.findChild<QPushButton *>("teamWorkspaceClosePhase");
    QVERIFY(close && !close->isEnabled());
    QCOMPARE(close->toolTip(), QStringLiteral("Phase 2 still has #4, #7 running; close it when they are decided"));
    // An empty queue is never "finished": no Finish, no "waiting for you to finish".
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceFinish"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u59-graph-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u59-graph-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    // Integrate & build adds the build task, once.
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceIntegrateBuild"));
    auto *build = workspace.findChild<QPushButton *>("teamWorkspaceIntegrateBuild");
    QVERIFY(build->isEnabled());
    build->click();
    QTRY_COMPARE(calls(script(), "integrate-build").size(), 1);
    QCOMPARE(calls(script(), "integrate-build").first()["team"].toString(), QStringLiteral("team-1"));
    // The Board folds the same way and carries the mode.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceBoard"));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 2)->findChild<QWidget *>("teamFreeAccepted"));
    QCOMPARE(modeOf(2), QStringLiteral("free"));
    QList<int> cards;
    for (auto *card : phaseScrollOf(&workspace, 2)->findChildren<QFrame *>("teamBoardCard")) cards << card->property("task").toInt();
    std::sort(cards.begin(), cards.end());
    QCOMPARE(cards, (QList<int>{4, 7}));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u59-board-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u59-board-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "graph") { b->click(); break; }
    // Every task accepted: the phase is open and idle, not done.
    setTeam(idleFreeTeam());
    QTRY_VERIFY(([&] { controller.refresh(); return badgeOf(2) == QStringLiteral("Open, idle"); })());
    QCOMPARE(phaseScrollOf(&workspace, 2)->property("state").toString(), QStringLiteral("idle"));
    QVERIFY(phaseScrollOf(&workspace, 2)->findChild<QFrame *>("teamFreeIdleCard"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceFinish"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamHeaderFinishWaits"));
    QCOMPARE(workspace.findChild<QLabel *>("teamHeaderBuild")->text(), QStringLiteral("Last build 10:00, 3 accepted since"));
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceClosePhase")->isEnabled());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u59-idle-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u59-idle-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    // A task joins the open phase at once: no warning, no typed confirmation.
    QTRY_VERIFY(phaseAction(&workspace, 2, "teamFreeIdleAdd"));
    phaseAction(&workspace, 2, "teamFreeIdleAdd")->click();
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamAddLateDialog"));
    auto *add = workspace.findChild<QDialog *>("teamAddLateDialog");
    QVERIFY(add->property("free").toBool());
    QVERIFY(!add->findChild<QPushButton *>("teamAddLateAdd")->isEnabled());
    add->findChild<QLineEdit *>("teamAddLateTitle")->setText("Tax rounding");
    add->findChild<QPushButton *>("teamAddLateAdd")->click();
    QTRY_COMPARE(calls(script(), "extend").size(), 1);
    QCOMPARE(calls(script(), "extend").first()["tasks"].toArray().first().toObject()["title"].toString(), QStringLiteral("Tax rounding"));
    QCOMPARE(calls(script(), "add-to-phase").size(), 0);
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamAddLateDialog"));
    // Close phase asks first: integrate and build is on, as work was accepted
    // since the last build; Cancel sends nothing.
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceClosePhase"));
    workspace.findChild<QPushButton *>("teamWorkspaceClosePhase")->click();
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamClosePhaseDialog"));
    auto *closeDialog = workspace.findChild<QDialog *>("teamClosePhaseDialog");
    QTRY_VERIFY(closeDialog->isVisible());
    auto *integrate = closeDialog->findChild<QCheckBox *>("teamClosePhaseIntegrate");
    QVERIFY(integrate->isChecked() && integrate->isEnabled());
    QCOMPARE(closeDialog->findChild<QPushButton *>("teamClosePhaseConfirm")->text(), QStringLiteral("Build and close"));
    QCOMPARE(closeDialog->findChild<QLabel *>("teamClosePhaseTitle")->text(), QStringLiteral("Close Phase 2: Support stream?"));
    QCOMPARE(closeDialog->findChild<QLabel *>("teamClosePhaseTasks")->text(), QStringLiteral("4 accepted, none running or pending"));
    for (auto *text : closeDialog->findChildren<QLabel *>()) QVERIFY2(!text->text().contains(QChar(0x00b7)), qPrintable(text->text()));
    shot(closeDialog, "u59-close-dark");
    integrate->setChecked(false);
    QCOMPARE(closeDialog->findChild<QPushButton *>("teamClosePhaseConfirm")->text(), QStringLiteral("Close phase"));
    integrate->setChecked(true);
    closeDialog->findChild<QPushButton *>("teamClosePhaseCancel")->click();
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamClosePhaseDialog"));
    QCOMPARE(calls(script(), "close-phase").size(), 0);
    workspace.setTheme(false); workspace.render();
    lightPalette();
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceClosePhase"));
    workspace.findChild<QPushButton *>("teamWorkspaceClosePhase")->click();
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamClosePhaseDialog"));
    closeDialog = workspace.findChild<QDialog *>("teamClosePhaseDialog");
    QTRY_VERIFY(closeDialog->isVisible());
    lightShot(closeDialog, "u59-close-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
    closeDialog->findChild<QPushButton *>("teamClosePhaseConfirm")->click();
    QTRY_COMPARE(calls(script(), "close-phase").size(), 1);
    QVERIFY(calls(script(), "close-phase").first()["integrate"].toBool());
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamClosePhaseDialog"));
    // The close waits for its build task: the header says so and Close phase is off.
    auto pending = idleFreeTeam();
    auto tasks = pending["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 8}, {"phase", 2}, {"title", "Build phase 2"}, {"assignee", "lead"}, {"status", "pending"}, {"phase_build", true}});
    pending["tasks"] = tasks;
    auto free = pending["free_phase"].toObject();
    free["state"] = "open"; free["running"] = QJsonArray{8}; free["close_pending"] = 2;
    pending["free_phase"] = free;
    setTeam(pending);
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QLabel *>("teamHeaderClosePending") != nullptr; })());
    QCOMPARE(workspace.findChild<QLabel *>("teamHeaderClosePending")->text(), QStringLiteral("Phase 2 closes when its build #8 is accepted"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceClosePhase")->isEnabled());
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceIntegrateBuild")->isEnabled());
    // Nothing accepted since the last build: the close is immediate, no build.
    auto quiet = idleFreeTeam();
    auto quietFree = quiet["free_phase"].toObject(); quietFree["accepted_since"] = 0; quiet["free_phase"] = quietFree;
    setTeam(quiet);
    QTRY_VERIFY(([&] { controller.refresh(); auto *b = workspace.findChild<QPushButton *>("teamWorkspaceClosePhase"); return b && b->isEnabled(); })());
    workspace.findChild<QPushButton *>("teamWorkspaceClosePhase")->click();
    QTRY_VERIFY(workspace.findChild<QDialog *>("teamClosePhaseDialog"));
    closeDialog = workspace.findChild<QDialog *>("teamClosePhaseDialog");
    QVERIFY(!closeDialog->findChild<QCheckBox *>("teamClosePhaseIntegrate")->isChecked());
    QVERIFY(!closeDialog->findChild<QCheckBox *>("teamClosePhaseIntegrate")->isEnabled());
    closeDialog->findChild<QPushButton *>("teamClosePhaseConfirm")->click();
    QTRY_COMPARE(calls(script(), "close-phase").size(), 2);
    QVERIFY(!calls(script(), "close-phase").last()["integrate"].toBool());
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamClosePhaseDialog"));
}

// C3 (#548): a desktop edit of a planning team with a roadmap keeps it: the
// plan carries each task's phase and the later phases, through phase 3's graph.
void TestTeamsUiPlan::planEditKeepsTheRoadmap()
{
    auto planning = graphTeam();
    planning["status"] = "planning";
    auto phases = planning["phases"].toArray();
    for (int i = 0; i < phases.size(); ++i) { auto p = phases[i].toObject(); p["state"] = "draft"; phases[i] = p; }
    auto last = phases[1].toObject(); last["barrier"] = true; phases[1] = last;
    planning["phases"] = phases;
    auto tasks = planning["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto t = tasks[i].toObject(); t["status"] = "draft"; t.remove("attempts");
        t["description"] = QStringLiteral("Description of #%1").arg(t["id"].toInt());
        tasks[i] = t;
    }
    planning["tasks"] = tasks;
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    TeamUi::TaskView *draft = nullptr;
    QTRY_VERIFY(([&] { draft = phaseGraphOf(&workspace, 3); return draft && draft->linked && controller.watched() && controller.watched()->full; })());
    draft->linked(9, 8);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    const auto plan = calls(script(), "plan").first();
    QMap<int, int> phaseOf;
    for (const auto &value : plan["tasks"].toArray()) phaseOf.insert(value.toObject()["id"].toInt(), value.toObject()["phase"].toInt());
    QCOMPARE(phaseOf, (QMap<int, int>{{1, 1}, {2, 2}, {3, 2}, {4, 2}, {5, 2}, {6, 2}, {7, 3}, {8, 3}, {9, 3}}));
    const auto roadmap = plan["phases"].toArray();
    QCOMPARE(roadmap.size(), 2);
    QCOMPARE(roadmap[0].toObject()["n"].toInt(), 2);
    QCOMPARE(roadmap[0].toObject()["title"].toString(), QStringLiteral("Build"));
    QCOMPARE(roadmap[1].toObject()["n"].toInt(), 3);
    QCOMPARE(roadmap[1].toObject()["title"].toString(), QStringLiteral("Verify & ship"));
    QVERIFY(roadmap[1].toObject()["barrier"].toBool());
    // The edit itself: #8 now depends on #2 and #9; the descriptions stay.
    for (const auto &value : plan["tasks"].toArray()) {
        const auto task = value.toObject();
        QCOMPARE(task["description"].toString(), QStringLiteral("Description of #%1").arg(task["id"].toInt()));
        if (task["id"].toInt() == 8) QCOMPARE(task["deps"].toArray().size(), 2);
    }
}

// C4 (#548): before the full plan (`team status`) arrives, a plan edit sends
// nothing built from the `team list` summary; it says the plan is loading and
// the same edit goes once the full plan is here.
void TestTeamsUiPlan::planEditsWaitForTheFullPlan()
{
    setTeam(team("planning"));
    QFile hold(script() + ".statushold"); QVERIFY(hold.open(QIODevice::WriteOnly)); hold.close();
    const auto release = qScopeGuard([&] { QFile::remove(hold.fileName()); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    QVERIFY(!controller.team("team-1")->full);
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    TeamUi::TaskView *graph = nullptr;
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); return graph && graph->linked; })());
    QVERIFY(!controller.watched() || !controller.watched()->full);
    graph->linked(3, 5);
    QTest::qWait(300);
    QVERIFY(calls(script(), "plan").isEmpty());
    QTRY_VERIFY(workspace.findChild<QLabel *>("teamWorkspaceErrorText")
                && workspace.findChild<QLabel *>("teamWorkspaceErrorText")->text().startsWith(QStringLiteral("Loading the plan")));
    // The panel's own save refuses a summary copy too.
    panel.link(*controller.team("team-1"), 3, 5, true);
    QTest::qWait(200);
    QVERIFY(calls(script(), "plan").isEmpty());
    // The full plan arrives: the same edit goes, with the full tasks.
    QFile::remove(hold.fileName());
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); return graph && graph->linked; })());
    graph->linked(3, 5);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    QCOMPARE(calls(script(), "plan").first()["revision"].toInt(), 12);
}

// U31: the owner's Work ahead switch in the Team tab's toolbar and the
// "worked ahead" mark on a task worked before its draft phase was approved,
// in one picture per theme.
void TestTeamsUiPlan::workAheadSwitchAndBadge()
{
    auto t = graphTeam();
    auto settings = t["settings"].toObject(); settings["work_ahead"] = true; t["settings"] = settings;
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 8) continue;
        task["status"] = "submitted";
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a8"}, {"member", "backend"}, {"state", "reported"}, {"worked_ahead", true},
            {"report", QJsonObject{{"text", "Rate limits on /v1/cards are in"}, {"at", 1750}}}}};
        tasks[i] = task;
    }
    t["tasks"] = tasks;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    QAbstractButton *ahead = nullptr;
    QTRY_VERIFY((ahead = workspace.findChild<QAbstractButton *>("teamWorkspaceWorkAhead")));
    QTRY_VERIFY(ahead->isChecked());
    QCOMPARE(ahead->accessibleName(), QString("Work ahead"));
    const auto parsed = Teams::parse(t);
    QVERIFY(parsed.task(8) && parsed.task(8)->workedAhead());
    const auto live = [](const Member &) { Live state; state.known = true; return state; };
    TeamTaskCard card;
    card.resize(560, 420); card.show();
    for (const bool dark : {true, false}) {
        if (dark) darkPalette(); else lightPalette();
        workspace.setTheme(dark);
        card.setTask(parsed, 8, live, dark);
        QLabel *badge = nullptr;
        QTRY_VERIFY((badge = card.findChild<QLabel *>("teamTaskWorkedAhead")));
        QCOMPARE(badge->text(), QString("worked ahead"));
        QTRY_VERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceWorkAhead"));
        const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
        if (dir.isEmpty()) continue;
        QTest::qWait(250);
        // The toolbar with the switch above the task card with the mark.
        auto *toolbar = workspace.findChild<QWidget *>("teamWorkspaceToolbar");
        QVERIFY(toolbar);
        const QPixmap top = toolbar->grab(), bottom = card.grab();
        QPixmap picture(qMax(top.width(), bottom.width()), top.height() + 12 + bottom.height());
        picture.fill(dark ? QColor("#15191f") : QColor("#f5f7f9"));
        QPainter painter(&picture);
        painter.drawPixmap(0, 0, top);
        painter.drawPixmap(0, top.height() + 12, bottom);
        painter.end();
        QVERIFY(picture.save(QDir(dir).filePath(QStringLiteral("ux-31-work-ahead-%1.png").arg(dark ? "dark" : "light"))));
    }
}

void TestTeamsUiPlan::thePlanHidesCancelledTasksAndNeverSendsTheCopy()
{
    // A roadmap phase 2 whose only task was cancelled: the copy on screen has
    // neither, and a plan sent from it would wipe the phase.
    auto planned = team("planning");
    auto tasks = planned["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 7}, {"phase", 2}, {"title", "Dropped idea"}, {"assignee", "backend"}, {"status", "cancelled"}});
    planned["tasks"] = tasks;
    planned["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "Later"}, {"barrier", false}, {"tasks", QJsonArray{7}}}};
    auto layers = planned["layers"].toObject(); layers["7"] = 0; planned["layers"] = layers;
    setTeam(planned);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.setLive(live);
    panel.resize(620, 1400); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    const auto nodes = [&] {
        QList<int> found;
        if (auto *graph = dynamic_cast<TeamUi::TaskView *>(panel.findChild<QWidget *>("teamTaskGraph"))) for (const auto &node : graph->nodes) found << node.id;
        return found;
    };
    QTRY_VERIFY(nodes().contains(6));
    QVERIFY(!nodes().contains(7));
    auto *show = panel.findChild<QPushButton *>("teamShowCancelled");
    QVERIFY(show && !show->isChecked());
    QCOMPARE(show->text(), QStringLiteral("Show cancelled (1)"));
    show->click();
    QTRY_VERIFY(nodes().contains(7));
    QVERIFY(panel.findChild<QPushButton *>("teamShowCancelled")->isChecked());
    controller.setShowCancelled(false);
    QTRY_VERIFY(!nodes().contains(7) && nodes().contains(6));
    // An edit from the plan sends the whole plan, the hidden phase included.
    auto *graph = dynamic_cast<TeamUi::TaskView *>(panel.findChild<QWidget *>("teamTaskGraph"));
    QVERIFY(graph && graph->linked);
    const int before = int(calls(script(), "plan").size());
    graph->linked(3, 5);
    QTRY_COMPARE(int(calls(script(), "plan").size()), before + 1);
    QCOMPARE(calls(script(), "plan").last()["phases"].toArray(), (QJsonArray{QJsonObject{{"n", 2}, {"title", "Later"}, {"barrier", false}}}));
    // The display copy is never sent: it would drop phase 2 from the plan.
    panel.savePlan(withoutCancelled(*controller.watched()));
    QTest::qWait(80);
    QCOMPARE(int(calls(script(), "plan").size()), before + 1);
    QTRY_VERIFY(panel.findChild<QLabel *>("teamNotice"));
}

void TestTeamsUiPlan::aTaskThatHasNotStartedMovesToAnotherPhase()
{
    // #5 waits in draft phase 3 with no attempt; #4 (phase 2) has one that
    // reached its member; #9 was refused once and never reached it.
    auto json = phasedTeam();
    auto phases = json["phases"].toArray();
    auto two = phases[0].toObject(); two["tasks"] = QJsonArray{3, 4, 9}; phases[0] = two;
    json["phases"] = phases;
    auto tasks = json["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 9}, {"phase", 2}, {"title", "Refused once"}, {"assignee", "backend"}, {"status", "pending"},
                             {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "r9"}, {"member", "backend"}, {"state", "refused"}}}}});
    json["tasks"] = tasks;
    const auto t = parse(json);
    TeamTaskCard card;
    card.resize(420, 900); card.show();
    QList<std::pair<QString, QJsonObject>> acted;
    card.act = [&](const QString &op, const QJsonObject &input) { acted.append({op, input}); };
    card.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    const auto moves = [&] {
        QList<QAction *> found;
        for (auto *action : card.findChild<QMenu *>("teamTaskMoreMenu")->actions()) if (action->objectName().startsWith("teamTaskMoveToPhase")) found << action;
        return found;
    };
    // #5 depends on #4 in phase 2: phase 1 refuses it with the reason, phase 2 takes it.
    card.setTask(t, 5, live, true);
    auto five = moves();
    QCOMPARE(five.size(), 2);
    QCOMPARE(five[0]->text(), QStringLiteral("Move to phase 1"));
    QVERIFY(!five[0]->isEnabled());
    QCOMPARE(five[0]->toolTip(), QStringLiteral("Depends on #4 in phase 2; move it first"));
    QCOMPARE(five[1]->text(), QStringLiteral("Move to phase 2 · QA fixes"));
    QVERIFY(five[1]->isEnabled());
    five[1]->trigger();
    QCOMPARE(acted.size(), 1);
    QCOMPARE(acted[0].first, QStringLiteral("move-task"));
    // It keeps its number: the same task id, the target phase and the revision read.
    QCOMPARE(acted[0].second, (QJsonObject{{"task", 5}, {"phase", 2}, {"revision", 12}}));
    // A refused attempt never reached the member: #9 still moves.
    card.setTask(t, 9, live, true);
    QList<int> nine; for (auto *action : moves()) nine << action->property("phase").toInt();
    QCOMPARE(nine, (QList<int>{1, 3}));
    QVERIFY(moves()[0]->isEnabled() && moves()[1]->isEnabled());
    card.findChild<QMenu *>("teamTaskMoreMenu")->popup(card.mapToGlobal(QPoint(40, 40)));
    TestWait::settle();
    shot(card.findChild<QMenu *>("teamTaskMoreMenu"), "u58-move-to-phase-dark");
    card.findChild<QMenu *>("teamTaskMoreMenu")->hide();
    card.setTask(t, 9, live, false);
    card.findChild<QMenu *>("teamTaskMoreMenu")->popup(card.mapToGlobal(QPoint(40, 40)));
    TestWait::settle();
    lightShot(card.findChild<QMenu *>("teamTaskMoreMenu"), "u58-move-to-phase-light");
    card.findChild<QMenu *>("teamTaskMoreMenu")->hide();
    darkPalette();
    // A task whose attempt reached its member, a decided task: no move.
    card.setTask(t, 4, live, true);
    QVERIFY(moves().isEmpty());
    card.setTask(t, 3, live, true);
    QVERIFY(moves().isEmpty());
    // A team without later phases offers none either, as before.
    card.setTask(parse(team()), 5, live, true);
    QVERIFY(moves().isEmpty());
}

void TestTeamsUiPlan::anAutoAcceptedDraftReadsAsDraftUntilItStarts()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    setTeam(autoDraftTeam(false, false));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1400, 860); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto badge = [&](int phase) -> QWidget * { auto *frame = phaseScrollOf(&workspace, phase); return frame ? frame->findChild<QWidget *>("teamPhaseBadge") : nullptr; };
    const auto view = [&](const QString &value) {
        QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == value) { b->click(); break; }
    };
    QTRY_VERIFY(phaseScrollOf(&workspace, 3));
    // Graph, Board and Layers: Draft (auto), with when Auto accepts it.
    for (const auto &name : {QStringLiteral("graph"), QStringLiteral("board"), QStringLiteral("layers")}) {
        view(name);
        QTRY_VERIFY(badge(3) && badge(3)->property("auto").toBool());
        auto *phaseBadge = dynamic_cast<TeamUi::PhaseBadge *>(badge(3));
        QVERIFY(phaseBadge);
        QCOMPARE(phaseBadge->text(), QStringLiteral("Draft (auto)"));
        QCOMPARE(badge(3)->property("state").toString(), QStringLiteral("draft"));
        QVERIFY(badge(3)->toolTip().startsWith(QStringLiteral("Auto accepts it when its first task starts")));
        QVERIFY(!phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseStartAhead"));
    }
    view("graph");
    QTRY_VERIFY(badge(3));
    // It does not wait for you: no draft count; the switch says when a draft is fixed.
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceDrafts"));
    auto *autoSwitch = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept");
    QVERIFY(autoSwitch && autoSwitch->isChecked());
    QCOMPARE(autoSwitch->toolTip(), QStringLiteral("On: a phase draft without errors stays a draft the lead edits, and Auto accepts it when its first task starts"));
    if (auto *phaseSwitch = workspace.findChild<QAbstractButton *>("teamPhaseAutoAccept")) QCOMPARE(phaseSwitch->toolTip(), autoSwitch->toolTip());
    // The plan's band says the same.
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    const auto bands = TeamUi::phaseBands(*controller.watched(), [](const PhaseGroup &) { return true; });
    QVERIFY(std::any_of(bands.cbegin(), bands.cend(), [](const TeamUi::TaskView::PhaseBand &band) { return band.n == 3 && band.autoDraft && !band.startAhead; }));
    QVERIFY(std::none_of(bands.cbegin(), bands.cend(), [](const TeamUi::TaskView::PhaseBand &band) { return band.n == 2 && band.autoDraft; }));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u61-auto-draft-dark");
        workspace.setTheme(false); lightShot(&workspace, "u61-auto-draft-light");
        darkPalette(); workspace.setTheme(true);
    }
    // The lead started it ahead: the header says so.
    setTeam(autoDraftTeam(true, false));
    QTRY_VERIFY(([&] { controller.refresh(); auto *frame = phaseScrollOf(&workspace, 3); return frame && frame->findChild<QLabel *>("teamPhaseStartAhead"); })());
    auto *ahead = phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseStartAhead");
    QCOMPARE(ahead->text(), QStringLiteral("started ahead by the lead"));
    QVERIFY(ahead->toolTip().startsWith(QStringLiteral("The lead started it ahead")));
    QVERIFY(badge(3)->toolTip().contains(QStringLiteral("Its reports need no approval from you")));
    QTRY_VERIFY(([&] { if (!controller.watched()) return false; const auto now = TeamUi::phaseBands(*controller.watched(), [](const PhaseGroup &) { return true; });
                       return std::any_of(now.cbegin(), now.cend(), [](const TeamUi::TaskView::PhaseBand &band) { return band.n == 3 && band.startAhead; }); })());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u61-started-ahead-dark");
        workspace.setTheme(false); lightShot(&workspace, "u61-started-ahead-light");
        darkPalette(); workspace.setTheme(true);
    }
    // Its first task was handed out: Auto accepted it, and it reads as accepted.
    setTeam(autoDraftTeam(true, true));
    QTRY_VERIFY(([&] { controller.refresh(); auto *frame = phaseScrollOf(&workspace, 3); return frame && frame->findChild<QLabel *>("teamPhaseByAuto"); })());
    QVERIFY(!badge(3)->property("auto").toBool());
    QVERIFY(badge(3)->property("state").toString() != QStringLiteral("draft"));
    QVERIFY(!phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseStartAhead"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseByAuto")->text(), QStringLiteral("accepted by Auto"));
    for (const auto &name : {QStringLiteral("board"), QStringLiteral("layers")}) {
        view(name);
        QTRY_VERIFY(phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseByAuto"));
    }
    view("graph");
    QTRY_VERIFY(phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseByAuto"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u61-accepted-by-auto-dark");
        workspace.setTheme(false); lightShot(&workspace, "u61-accepted-by-auto-light");
        darkPalette(); workspace.setTheme(true);
    }
    // A review draft keeps today's words.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); auto *b = dynamic_cast<TeamUi::PhaseBadge *>(badge(3)); return b && b->text() == QStringLiteral("Draft — waiting for you"); })());
    QVERIFY(!badge(3)->property("auto").toBool());
}

// U29: a task added to its running phase carries the "out of the phase's
// original scope" icon in Graph, Board, Layers and the task card, light and
// dark; the owner adds one through the warning and a typed confirmation.
void TestTeamsUiPlan::addedLateTaskIsOutOfScopeEverywhere()
{
    auto t = phasedTeam();
    auto tasks = t["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 6}, {"phase", 2}, {"title", "Fix the login crash"}, {"assignee", "tester"}, {"status", "pending"},
                             {"added_late", QJsonObject{{"by", "Lead"}, {"at", 1750.0}, {"request", "Please add a task to the current phase: the login crash"},
                                                        {"confirm", "Yes, add it"}}}});
    t["tasks"] = tasks;
    auto phases = t["phases"].toArray();
    auto second = phases[0].toObject(); second["tasks"] = QJsonArray{3, 4, 6}; phases[0] = second;
    t["phases"] = phases;
    auto layers = t["layers"].toObject(); layers["6"] = 1; t["layers"] = layers;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(phaseGraphOf(&workspace, 2));
    const auto lateNode = [&] {
        for (const auto &n : phaseGraphOf(&workspace, 2)->nodes) if (n.id == 6) return n;
        return TeamUi::TaskView::Node{};
    };
    QTRY_VERIFY(lateNode().late);
    QVERIFY(lateNode().tooltip.contains("Out of the phase's original scope: added by Lead"));
    for (const auto &n : phaseGraphOf(&workspace, 2)->nodes) if (n.id != 6) QVERIFY(!n.late);
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == segment) { b->click(); break; }
    };
    const auto marks = [&] {
        QList<QLabel *> found;
        for (auto *mark : workspace.findChildren<QLabel *>("teamOutOfScope")) if (mark->isVisible()) found << mark;
        return found;
    };
    const auto shoot = [&](const QString &name) {
        if (shotsWanted()) {   // #713: the light half of the shots
            shot(&workspace, name + "-dark");
            lightPalette(); workspace.setTheme(false); workspace.render(); QTest::qWait(200);
            lightShot(&workspace, name + "-light");
            darkPalette(); workspace.setTheme(true); workspace.render(); QTest::qWait(150);
        }
    };
    shoot("ux-u29-graph");
    view("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QTRY_COMPARE(marks().size(), 1);
    QCOMPARE(marks().first()->property("task").toInt(), 6);
    QVERIFY(marks().first()->toolTip().contains("Asked: “Please add a task to the current phase: the login crash”"));
    QVERIFY(marks().first()->toolTip().contains("Confirmed: “Yes, add it”"));
    shoot("ux-u29-board");
    view("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(marks().size(), 1);
    QCOMPARE(marks().first()->property("task").toInt(), 6);
    shoot("ux-u29-layers");
    // The task card.
    TeamTaskCard card;
    card.resize(420, 640); card.show();
    card.setTask(parse(t), 6, live, true);
    QVERIFY(card.findChild<QLabel *>("teamOutOfScope"));
    QVERIFY(card.findChild<QLabel *>("teamOutOfScope")->toolTip().contains("added by Lead"));
    card.setTask(parse(t), 4, live, true);
    QVERIFY(!card.findChild<QLabel *>("teamOutOfScope"));
    card.setTask(parse(t), 6, live, true);
    shot(&card, "ux-u29-task-card-dark");
    lightPalette(); card.setTask(parse(t), 6, live, false); QTest::qWait(150);
    lightShot(&card, "ux-u29-task-card-light");
    darkPalette();
    // The owner adds a task: the warning first, then the typed confirmation.
    view("graph");
    QTRY_VERIFY(phaseAction(&workspace, 2, "teamPhaseAddLate"));
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseAddLate"));   // a draft keeps edit-draft
    phaseAction(&workspace, 2, "teamPhaseAddLate")->click();
    QDialog *dialog = nullptr;
    QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamAddLateDialog")));
    auto *next = dialog->findChild<QPushButton *>("teamAddLateContinue");
    QVERIFY(!next->isEnabled());
    dialog->findChild<QLineEdit *>("teamAddLateTitle")->setText("Retry the payment webhook");
    dialog->findChild<QPlainTextEdit *>("teamAddLateDescription")->setPlainText("Retry with backoff");
    // #746: the estimate in minutes; From experience (0) sends none.
    auto *estimate = dialog->findChild<QSpinBox *>("teamAddLateEstimate");
    QVERIFY(estimate);
    QCOMPARE(estimate->text(), QStringLiteral("From experience"));
    estimate->setValue(30);
    QVERIFY(next->isEnabled());
    next->click();
    QTRY_COMPARE(calls(script(), "add-to-phase").size(), 1);
    const auto first = calls(script(), "add-to-phase").first();
    QCOMPARE(first["tasks"].toArray().first().toObject()["estimate_minutes"].toInt(), 30);
    QCOMPARE(first["phase"].toInt(), 2);
    QCOMPARE(first["tasks"].toArray().first().toObject()["id"].toInt(), 7);
    QCOMPARE(first["tasks"].toArray().first().toObject()["title"].toString(), QStringLiteral("Retry the payment webhook"));
    QCOMPARE(first["by"].toString(), QStringLiteral("human"));
    QVERIFY(!first.contains("confirm"));
    auto *warning = dialog->findChild<QLabel *>("teamAddLateWarning");
    QTRY_VERIFY(warning->isVisible());
    QVERIFY(warning->text().contains("load balance of the lanes"));
    auto *confirm = dialog->findChild<QLineEdit *>("teamAddLateConfirm");
    auto *add = dialog->findChild<QPushButton *>("teamAddLateAdd");
    QVERIFY(confirm->isVisible() && add->isVisible() && !add->isEnabled());
    confirm->setText("sure"); QVERIFY(!add->isEnabled());
    confirm->setText("3"); QVERIFY(!add->isEnabled());   // another phase
    confirm->setText("2"); QVERIFY(add->isEnabled());
    confirm->setText("Add"); QVERIFY(add->isEnabled());
    shot(dialog, "ux-u29-dialog-dark");
    lightPalette(); QTest::qWait(150);
    shot(dialog, "ux-u29-dialog-light");
    darkPalette();
    add->click();
    QTRY_COMPARE(calls(script(), "add-to-phase").size(), 2);
    const auto second2 = calls(script(), "add-to-phase").last();
    QCOMPARE(second2["confirm"].toString(), QStringLiteral("pending-1"));
    QCOMPARE(second2["confirmation"].toString(), QStringLiteral("Add"));
    QCOMPARE(second2["phase"].toInt(), 2);
    QTRY_VERIFY(!workspace.findChild<QDialog *>("teamAddLateDialog"));
    // The feed says so.
    const auto team = parse(t);
    const auto line = [&team](const QString &kind, const QJsonObject &detail) {
        return feedLine(team, QJsonObject{{"at", 1}, {"seq", 1}, {"kind", kind}, {"detail", detail}}).text;
    };
    QCOMPARE(line("phase_addition_warned", {{"phase", 2}, {"tasks", QJsonArray{7}}}),
             QStringLiteral("Asked to add 1 task to running phase 2 · waits for your confirmation"));
    QCOMPARE(line("phase_tasks_added", {{"phase", 2}, {"tasks", QJsonArray{7, 8}}}),
             QStringLiteral("Added #7, #8 to running phase 2 · outside its original scope"));
}

// Owner 2026-10-31 (#746): the plan editor shows and sets a task's estimate in
// minutes ("From experience" leaves it to the solved tasks, and shows their
// number); the add-to-phase dialog sends one; the lead's add-to-phase card
// shows each task's.
void TestTeamsUiPlan::theEstimateIsSetInThePlanAndWhenATaskJoinsAPhase()
{
    auto planning = team("planning");
    auto tasks = planning["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (i == 0) { task["estimate_minutes"] = 25; task["estimate_source"] = "history"; task["estimate_text"] = "25 min"; }
        else { task["estimate_minutes"] = QJsonValue(); task["estimate_source"] = "none"; task["estimate_text"] = "no estimate yet"; }
        tasks[i] = task;
    }
    planning["tasks"] = tasks;
    const int first = tasks.first().toObject()["id"].toInt();
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(420, 1200); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    std::function<void(QDialog *)> with;
    std::function<void(int)> look = [&](int tries) {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != "teamTaskEditor") { if (tries > 0) QTimer::singleShot(10, &panel, [&look, tries] { look(tries - 1); }); return; }
        with(dialog);
        for (auto *b : dialog->findChildren<QPushButton *>()) if (b->text() == QStringLiteral("Save")) { b->click(); return; }
        dialog->reject();
    };
    // From experience: the history's number shows; the lead sets 45 minutes.
    with = [&](QDialog *dialog) {
        auto *estimate = dialog->findChild<QSpinBox *>("teamTaskEditEstimate");
        QVERIFY(estimate);
        QCOMPARE(estimate->value(), 0);
        QCOMPARE(estimate->text(), QStringLiteral("From experience, ~25 min"));
        estimate->setValue(45);
        shot(dialog, "u746-plan-editor-dark");
        lightPalette(); lightShot(dialog, "u746-plan-editor-light"); darkPalette();
    };
    QTimer::singleShot(0, &panel, [&look] { look(300); });
    panel.editTask(*controller.watched(), first);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    QJsonObject sent;
    for (const auto &value : calls(script(), "plan").first()["tasks"].toArray()) {
        const auto task = value.toObject();
        if (task["id"].toInt() == first) sent = task;
        else QVERIFY(!task.contains("estimate_minutes"));   // the history's stays the history's
    }
    QCOMPARE(sent["estimate_minutes"].toDouble(), 45.0);
    // The lead's estimate reads back into the editor; clearing it leaves it out.
    auto read = planning; auto again = read["tasks"].toArray(); auto one = again[0].toObject();
    one["estimate_minutes"] = 45; one["estimate_source"] = "lead"; again[0] = one; read["tasks"] = again;
    setTeam(read);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->task(first) && t->task(first)->estimateSource == "lead"; })());
    with = [&](QDialog *dialog) {
        auto *estimate = dialog->findChild<QSpinBox *>("teamTaskEditEstimate");
        QCOMPARE(estimate->value(), 45);
        estimate->setValue(0);
    };
    QTimer::singleShot(0, &panel, [&look] { look(300); });
    panel.editTask(*controller.watched(), first);
    QTRY_COMPARE(calls(script(), "plan").size(), 2);
    for (const auto &value : calls(script(), "plan").last()["tasks"].toArray()) QVERIFY(!value.toObject().contains("estimate_minutes"));
    // The lead's add-to-phase card names each task's estimate.
    Team::Proposal proposal;
    proposal.kind = QStringLiteral("add-to-phase");
    proposal.change = QJsonObject{{"phase", 2}, {"tasks", QJsonArray{QJsonObject{{"title", "Retry the webhook"}, {"estimate_minutes", 30}},
                                                                       QJsonObject{{"title", "Log the retries"}}}}};
    const auto lines = Teams::proposalChanges(parse(phasedTeam()), proposal);
    QCOMPARE(lines, (QStringList{"Retry the webhook, ~30 min", "Log the retries"}));
}

// U45: the plan editor gives a task models and a role instead of an assignee;
// a task for a role reads "Any free <role>" and opens with its choice.
void TestTeamsUiPlan::planTaskTakesModelsAndARole()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(420, 1200); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    bool saved = false;
    std::function<void(QDialog *)> with;
    std::function<void(int)> look = [&](int tries) {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != "teamTaskEditor") { if (tries > 0) QTimer::singleShot(10, &panel, [&look, tries] { look(tries - 1); }); return; }
        with(dialog);
        for (auto *b : dialog->findChildren<QPushButton *>()) if (b->text() == QStringLiteral("Save")) { saved = true; b->click(); return; }
        dialog->reject();
    };
    with = [&](QDialog *dialog) {
        auto *assignee = dialog->findChild<QComboBox *>("teamTaskEditAssignee");
        QVERIFY(assignee->findData("role:Backend") > 0 && assignee->findData("role:Tester") > 0);
        QCOMPARE(assignee->findData("role:Architect"), -1);   // never the lead's role
        assignee->setCurrentIndex(assignee->findData("role:Backend"));
        dialog->findChild<QLineEdit *>("teamTaskEditModels")->setText("gpt-5.5-codex@high, gpt-5.5 gpt-5.5");
        shot(dialog, "u45-task-models-role-dark");
        lightPalette(); lightShot(dialog, "u45-task-models-role-light"); darkPalette();
    };
    QTimer::singleShot(0, &panel, [&look] { look(300); });
    panel.editTask(*controller.watched(), 6);
    QVERIFY(saved);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    QJsonObject six;
    for (const auto &value : calls(script(), "plan").first()["tasks"].toArray()) {
        const auto task = value.toObject();
        if (task["id"].toInt() == 6) six = task;
        else QVERIFY(!task.contains("models") && !task.contains("role"));
    }
    QCOMPARE(six["role"].toString(), QStringLiteral("Backend"));
    QCOMPARE(six["assignee"].toString(), QString());
    QCOMPARE(six["models"].toArray(), (QJsonArray{"gpt-5.5-codex@high", "gpt-5.5"}));
    // Read back: the task for a role shows it and the editor opens with it.
    auto planned = team("planning");
    auto tasks = planned["tasks"].toArray();
    auto task = tasks.last().toObject(); task["role"] = "Backend"; task["models"] = QJsonArray{"gpt-5.5"}; tasks[tasks.size() - 1] = task;
    planned["tasks"] = tasks;
    const Team read = parse(planned);
    QCOMPARE(read.assigneeRole(*read.task(6), QStringLiteral("You")), QStringLiteral("Any free Backend"));
    QCOMPARE(read.task(6)->models, QStringList{"gpt-5.5"});
    saved = false;
    with = [&](QDialog *dialog) {
        auto *assignee = dialog->findChild<QComboBox *>("teamTaskEditAssignee");
        QCOMPARE(assignee->currentData().toString(), QStringLiteral("role:Backend"));
        QCOMPARE(dialog->findChild<QLineEdit *>("teamTaskEditModels")->text(), QStringLiteral("gpt-5.5"));
        assignee->setCurrentIndex(assignee->findData("tester"));   // back to a member
        dialog->findChild<QLineEdit *>("teamTaskEditModels")->clear();
    };
    QTimer::singleShot(0, &panel, [&look] { look(300); });
    panel.editTask(read, 6);
    QVERIFY(saved);
    QTRY_COMPARE(calls(script(), "plan").size(), 2);
    for (const auto &value : calls(script(), "plan").last()["tasks"].toArray())
        if (value.toObject()["id"].toInt() == 6) {
            QCOMPARE(value.toObject()["assignee"].toString(), QStringLiteral("tester"));
            QVERIFY(!value.toObject().contains("role") && !value.toObject().contains("models"));
        }
}

void TestTeamsUiPlan::phaseScrollsInEveryView()
{
    Settings::set("workspace/skin", "whitewh");
    Settings::remove("ui/collapsed/team/needs");
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto scrolls = [&workspace] { return workspace.findChildren<QFrame *>("teamPhaseScroll"); };
    QTRY_COMPARE(scrolls().size(), 3);
    const auto scroll = [&](int phase) -> QFrame * { for (auto *f : scrolls()) if (f->property("phase").toInt() == phase) return f; return nullptr; };
    const auto badge = [&](int phase) { auto *b = dynamic_cast<TeamUi::PhaseBadge *>(scroll(phase)->findChild<QWidget *>("teamPhaseBadge")); return b ? b->text() : QString(); };
    QCOMPARE(scroll(1)->property("state").toString(), QStringLiteral("done"));
    QCOMPARE(badge(1), QStringLiteral("Done"));
    QCOMPARE(badge(2), QStringLiteral("Running"));
    QCOMPARE(badge(3), QStringLiteral("Draft — waiting for you"));
    // Done starts collapsed with its tasks in one line; the others are open.
    QVERIFY(scroll(1)->findChild<QLabel *>("teamPhaseSummary"));
    QVERIFY(scroll(1)->findChild<QLabel *>("teamPhaseSummary")->text().contains("#1 Requirements"));
    QVERIFY(!scroll(1)->findChild<QWidget *>("teamPhaseBody"));
    QVERIFY(scroll(2)->findChild<QWidget *>("teamPhaseBody") && scroll(3)->findChild<QWidget *>("teamPhaseBody"));
    // Bars count accepted tasks; Build's health names its problem; the draft says who proposed it.
    QCOMPARE(scroll(2)->findChild<QWidget *>("teamPhaseProgress")->property("total").toInt(), 5);
    QCOMPARE(scroll(2)->findChild<QLabel *>("teamPhaseHealth")->text(), QStringLiteral("Healthy 4/5"));
    QVERIFY(scroll(3)->findChild<QLabel *>("teamPhaseProposed")->text().startsWith("proposed by Lead"));
    QVERIFY(scroll(3)->findChild<QLabel *>("teamPhaseNoteText")->text().contains("rate limits"));
    // Graph: one graph per open phase; a dependency in another phase is named on the node.
    auto *build = dynamic_cast<TeamUi::TaskView *>(scroll(2)->findChild<QWidget *>("teamPhaseGraph")); QVERIFY(build);
    QCOMPARE(build->nodes.size(), 5);
    auto *verify = dynamic_cast<TeamUi::TaskView *>(scroll(3)->findChild<QWidget *>("teamPhaseGraph")); QVERIFY(verify);
    for (const auto &node : verify->nodes) if (node.id == 8) QCOMPARE(node.ext, QStringLiteral("↤#2"));
    QVERIFY(verify->edges.value(7).contains(8));
    // The draft's actions: Auto-accept, Reject…, Accept; Build's none.
    QVERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseAccept") && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    QVERIFY(scroll(3)->findChild<QAbstractButton *>("teamPhaseAutoAccept"));
    QVERIFY(!scroll(2)->findChild<QPushButton *>("teamPhaseAccept"));
    // The toolbar: the views, the filter, one draft, Auto-accept drafts, Expand/Collapse all, zoom.
    QCOMPARE(workspace.findChild<QPushButton *>("teamWorkspaceDrafts")->property("draftCount").toInt(), 1);
    QVERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept"));
    QCOMPARE(workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->text(), QStringLiteral("Expand all"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter") && workspace.findChild<QPushButton *>("teamWorkspaceZoomIn"));
    // The right column: Needs your answer collapsed with its count, then the members.
    auto *needs = dynamic_cast<CollapsibleBlock *>(workspace.findChild<QWidget *>("teamWorkspaceAnswers")); QVERIFY(needs);
    QVERIFY(!needs->isExpanded());
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceGoToQuestion")->isVisible());
    shot(&workspace, "ux-4-graph-dark");
    // Collapsing a phase from its header, then Expand all.
    scroll(2)->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(scroll(2) && !scroll(2)->findChild<QWidget *>("teamPhaseBody"));
    workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->click();
    QTRY_VERIFY(scroll(1) && scroll(1)->findChild<QWidget *>("teamPhaseBody") && scroll(2)->findChild<QWidget *>("teamPhaseBody"));
    QTRY_COMPARE(workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->text(), QStringLiteral("Collapse all"));
    // Board: five columns per phase, two-line cards, the problem on the card.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QTRY_COMPARE(scrolls().size(), 3);
    QCOMPARE(scroll(2)->findChildren<QFrame *>("teamBoardColumn").size(), 5);
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    const auto cardOf = [&](int id) -> QFrame * {
        for (auto *card : scroll(2)->findChildren<QFrame *>("teamBoardCard")) if (card->property("task").toInt() == id) return card;
        return nullptr;
    };
    QTRY_VERIFY(cardOf(2));
    QCOMPARE(cardOf(2)->parentWidget()->property("column").toString(), QStringLiteral("problems"));
    QVERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseAccept"));
    // Collapse Analysis again for the pictures.
    scroll(1)->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(scroll(1) && !scroll(1)->findChild<QWidget *>("teamPhaseBody"));
    shot(&workspace, "ux-4-board-dark");
    // Layers: one line per task, L1… inside each phase.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "layers") b->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(scrolls().size(), 3);
    QTRY_COMPARE(scroll(2)->findChildren<QFrame *>("teamLayerRow").size(), 5);
    QTRY_COMPARE(scroll(3)->findChildren<QFrame *>("teamLayerRow").size(), 3);
    QVERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    shot(&workspace, "ux-4-layers-dark");
    // Light.
    workspace.setTheme(false);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        lightPalette(); QTest::qWait(200);
        lightShot(&workspace, "ux-4-layers-light");
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
        QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
        lightShot(&workspace, "ux-4-board-light");
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "graph") b->click();
        QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspacePhases"));
        lightShot(&workspace, "ux-4-graph-light");
    }
    darkPalette();
    Settings::remove("workspace/skin");
}

void TestTeamsUiPlan::phaseScrollDecisions()
{
    Settings::set("workspace/skin", "whitewh");
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto scroll = [&](int phase) -> QFrame * {
        for (auto *f : workspace.findChildren<QFrame *>("teamPhaseScroll")) if (f->property("phase").toInt() == phase) return f;
        return nullptr;
    };
    WAIT_VERIFY(scroll(3) && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    // Reject… opens the form under the note; Cancel closes it and sends nothing.
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    WAIT_VERIFY(scroll(3) && scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    auto *text = scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText");
    QCOMPARE(text->placeholderText(), QStringLiteral("What should Lead change in this phase?"));
    QVERIFY(!scroll(3)->findChild<QPushButton *>("teamPhaseRejectSend")->isEnabled());
    scroll(3)->findChild<QPushButton *>("teamPhaseRejectCancel")->click();
    WAIT_VERIFY(scroll(3) && !scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    // The words survive a re-render while typing, then go to the lead.
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    WAIT_VERIFY(scroll(3) && scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    QTest::keyClicks(scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"), "Run the load test before the rate limits; release notes last.");
    workspace.render(); WAIT_SETTLED();
    WAIT_COMPARE(scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText")->toPlainText(), QStringLiteral("Run the load test before the rate limits; release notes last."));
    shot(&workspace, "ux-4-phase-reject-form-dark");
    scroll(3)->findChild<QPushButton *>("teamPhaseRejectSend")->click();
    WAIT_COMPARE(calls(script(), "reject-phase").size(), 1);
    QCOMPARE(calls(script(), "reject-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "reject-phase").first()["comment"].toString(), QStringLiteral("Run the load test before the rate limits; release notes last."));
    // Sent back: the warn badge, the owner's words and Undo.
    setTeam(prototypeTeam("rejected"));
    WAIT_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "rejected"; })());
    WAIT_VERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseUndo") && scroll(3)->findChild<QPushButton *>("teamPhaseUndo")->isEnabled());
    QVERIFY(scroll(3)->findChild<QLabel *>("teamPhaseSentBack")->text().startsWith(QStringLiteral("Sent back: “Run the load test")));
    QVERIFY(!scroll(3)->findChild<QPushButton *>("teamPhaseAccept"));
    QCOMPARE(workspace.findChild<QPushButton *>("teamWorkspaceDrafts")->property("draftCount").toInt(), 1);
    shot(&workspace, "ux-4-phase-rejected-dark");
    scroll(3)->findChild<QPushButton *>("teamPhaseUndo")->click();
    WAIT_COMPARE(calls(script(), "undo-reject-phase").size(), 1);
    QCOMPARE(calls(script(), "undo-reject-phase").first()["phase"].toInt(), 3);
    // Queued: accepted, waiting for Build.
    setTeam(prototypeTeam("queued"));
    WAIT_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "queued"; })());
    QCOMPARE(scroll(3)->findChild<QLabel *>("teamPhaseQueued")->text(), QStringLiteral("Starts after phase 2"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceDrafts"));
    shot(&workspace, "ux-4-phase-queued-dark");
    // A draft again: Accept asks first, then approves; Auto-accept is the team's setting.
    auto again = prototypeTeam(); again["revision"] = 23;
    setTeam(again);
    WAIT_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "draft" && scroll(3)->findChild<QPushButton *>("teamPhaseAccept"); })());
    WAIT_VERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseAccept")->isEnabled());
    armConfirm(&panel, false);
    scroll(3)->findChild<QPushButton *>("teamPhaseAccept")->click();
    WAIT_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept")->click();
    WAIT_COMPARE(calls(script(), "settings").size(), 1);
    QVERIFY(calls(script(), "settings").first()["settings"].toObject()["auto_accept"].toBool());
    // N13 (#383): only the switch's field goes; the server merges it, so a
    // lead's newer resume_after_limits or reminder stays.
    QCOMPARE(calls(script(), "settings").first()["settings"].toObject().keys(), QStringList{QStringLiteral("auto_accept")});
    // A stopped team takes no settings: both switches are off limits.
    auto stopped = again; stopped["status"] = "stopped"; stopped["revision"] = 24;
    setTeam(stopped);
    WAIT_VERIFY(([&] {
        controller.refresh();
        auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept");
        auto *phase = scroll(3) ? scroll(3)->findChild<QAbstractButton *>("teamPhaseAutoAccept") : nullptr;
        return (!top || !top->isEnabled()) && (!phase || !phase->isEnabled()) && (top || phase);
    })());
    if (auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept")) top->click();
    WAIT_SETTLED();
    QCOMPARE(calls(script(), "settings").size(), 1);
    again["revision"] = 25; setTeam(again);
    WAIT_VERIFY(([&] { controller.refresh(); auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept"); return top && top->isEnabled(); })());
    workspace.setTheme(false);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        setTeam(prototypeTeam("rejected"));
        WAIT_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "rejected"; })());
        lightShot(&workspace, "ux-4-phase-rejected-light");
    }
    darkPalette();
    Settings::remove("workspace/skin");
}

QTEST_MAIN(TestTeamsUiPlan)
#include "test_teamsui_plan.moc"
