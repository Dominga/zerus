// WhiteWh fork, #713: the agent-team UI tests of what waits for the owner and
// the lead: permissions, attention, the waiting page and banner, proposals and
// the coordinator's conversation.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include "ActivityView.h"
#include "HgsClient.h"
#include "TeamAttentionNotices.h"
#include "TeamAttentionWatch.h"
#include "TeamConversation.h"
#include "TeamProposalCard.h"
#include "TeamTaskCard.h"
#include "TeamWaitingBanner.h"
#include "TeamWaitingPage.h"
#include "TeamWaitingNotices.h"
#include "TeamWorkspace.h"

// G6: the team card, its header and the workspace bar carry two compact bars —
// the phase in work (or the next draft waiting) and the phases — readable at
// 420 and 290 px in both themes.
// B1/B2: the permission card shows the exact command, where it came from and
// the agent's reason, and offers allow or deny with a reason; an answered
// request names who answered and the deny text. B5: a plan that widens a
// profile says so and its approve asks for the owner's confirmation. G3: a
// dead-end task sinks below the tasks others depend on in its column.
// B3: a preflight finding arrives as a plan warning and shows in the plan card.
// C1-C4, C5, C7, C12, G7, F12, F13: the lead-load views in the desktop - the
// card's waits and check, the plan health and the owner's word, the attention
// strip, the member's services and budget, and the F12 draft wording.
// C4: the permission card reads members[].question and answers with the exact
// hgs team answer payload; N6: the check result comes from attempts[].check;
// N8: the plan footer's Approve asks for the widening confirmation; N9: a
// phase approved on the owner's word shows the quote; QA: the plan approval
// view lists every check command.
// The backend's own status fixture: one real member question from
// `hgs team status --json`, saved by the C4 backend task.
QJsonObject memberQuestionFixture()
{
    QFile file(QFileInfo(QString::fromUtf8(__FILE__)).dir().filePath(QStringLiteral("fixtures/hgs-team-member-question.json")));
    if (!file.open(QIODevice::ReadOnly)) return {};
    return QJsonDocument::fromJson(file.readAll()).object();
}
namespace {
// U50: one waiting item as `hgs team waiting` sends it.
QJsonObject waitingItem(const QString &team, const QString &name, const QString &kind, const QString &key, const QString &title,
                        double since, const QString &action, const QJsonObject &extra = {})
{
    QJsonObject item{{"id", team + "/" + kind + "/" + key}, {"team", team}, {"team_name", name}, {"kind", kind},
                     {"title", title}, {"since", since}, {"action", action}};
    for (auto it = extra.begin(); it != extra.end(); ++it) item.insert(it.key(), it.value());
    return item;
}

// One item of each kind, from six teams.
QJsonArray waitingKinds(double now)
{
    return QJsonArray{
        waitingItem("team-plan", "Billing export", "plan_draft", "plan", "Plan of 6 tasks waits for Approve & Run", now - 60, "approve_plan"),
        waitingItem("team-1", "Checkout", "phase_draft", "2", QString::fromUtf8("Phase 2 · Screens — draft waits for your Accept"), now - 300,
                    "approve_phase", {{"phase", 2}}),
        waitingItem("team-2", "Search index", "report_escalated", "4", "Report on #4 Ranking from Backend waits for your decision", now - 2400,
                    "decide_report", {{"task", 4}, {"member", "Backend"}}),
        waitingItem("team-3", "Mobile sync", "permission_escalated", "Tester", "Tester waits for your answer: Bash npm run e2e", now - 900,
                    "answer_permission", {{"member", "Tester"}}),
        waitingItem("team-2", "Search index", "owner_word_asked", "proposal-p3", QString::fromUtf8("The lead proposes: add member — a second reviewer"),
                    now - 1200, "decide_proposal", {{"proposal", "p3"}}),
        waitingItem("team-4", "Docs site", "task_waits_owner", "7", "#7 Publish the guide is held by you: after the release", now - 7200,
                    "release_task", {{"task", 7}}),
        waitingItem("team-5", "Release 0.43", "team_finished", "finish", "All 12 tasks decided: waits for you to finish", now - 86400, "finish"),
    };
}

bool writeWaiting(const QString &path, const QJsonArray &items)
{
    return FakeCli::replace(path, QJsonDocument(items).toJson(QJsonDocument::Compact));
}

QStringList waitingRowIds(QWidget *page)
{
    QStringList ids;
    for (auto *row : page->findChildren<QFrame *>("teamWaitingRow")) ids << row->property("itemId").toString();
    return ids;
}
// #724: liveTeam with what waits for the owner in it: the lead's proposals
// p1 and p2, an addition to running phase 1, Backend's permission question
// (escalated: on the attention list) and phase 2 as a draft.
QJsonObject answerTeam()
{
    auto t = liveTeam();
    auto members = t["members"].toArray();
    auto backend = members[1].toObject();
    backend["question"] = memberQuestionFixture();
    members[1] = backend;
    t["members"] = members;
    auto tasks = t["tasks"].toArray();
    auto second = tasks[1].toObject(); second["phase"] = 2; tasks[1] = second;
    t["tasks"] = tasks;
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 1}, {"title", "Build"}, {"state", "approved"}, {"tasks", QJsonArray{1}}, {"added_by", "Lead"}, {"added_at", 1000}},
        QJsonObject{{"n", 2}, {"title", "Release"}, {"state", "draft"}, {"tasks", QJsonArray{2}}, {"added_by", "Lead"}, {"added_at", 1500}}};
    t["proposals"] = QJsonArray{
        QJsonObject{{"id", "p1"}, {"kind", "add-member"}, {"by", "Lead"}, {"at", 2000.0}, {"state", "open"}, {"reason", "The UI needs a designer"},
                    {"member", QJsonObject{{"role", "Designer"}, {"agent", "claude"}, {"model", "sonnet"}}}, {"hash", "h1"}},
        QJsonObject{{"id", "p2"}, {"kind", "add-member"}, {"by", "Lead"}, {"at", 2100.0}, {"state", "open"}, {"reason", "Docs need an owner"},
                    {"member", QJsonObject{{"role", "Writer"}, {"agent", "claude"}, {"model", "haiku"}}}, {"hash", "h2"}}};
    t["pending_additions"] = QJsonArray{QJsonObject{{"id", "add-1"}, {"phase", 1}, {"by", "Lead"}, {"request", "add the card form tests"},
                                                    {"warned_at", 1800003000.0},
                                                    {"tasks", QJsonArray{QJsonObject{{"id", 3}, {"title", "Card form tests"}}}}}};
    t["attention"] = QJsonArray{QJsonObject{{"kind", "permission"}, {"member", "backend"}, {"since", 1800003100.0},
                                            {"summary", "Backend asks to run cargo test --locked"}}};
    return t;
}

QJsonObject waitingItem(const QString &key, const QString &kind, const QString &action, const QString &title, double since)
{
    return {{"id", QStringLiteral("team-1/%1/%2").arg(kind, key)}, {"team", "team-1"}, {"team_name", "Live"}, {"kind", kind},
            {"title", title}, {"since", since}, {"action", action}};
}

QJsonArray answerItems(double now)
{
    auto proposal = waitingItem("proposal-p1", "owner_word_asked", "decide_proposal", "The lead proposes: add member — The UI needs a designer", now - 60);
    proposal["proposal"] = "p1";
    auto stale = waitingItem("proposal-p2", "owner_word_asked", "decide_proposal", "The lead proposes: add member — Docs need an owner", now - 70);
    stale["proposal"] = "p2";
    auto addition = waitingItem("addition-add-1", "owner_word_asked", "confirm_addition", "Adding 1 task to running phase 1 waits for your confirmation", now - 120);
    addition["phase"] = 1;
    auto permission = waitingItem("backend", "permission_escalated", "answer_permission", "Backend asks to run cargo test --locked", now - 180);
    permission["member"] = "backend";
    auto draft = waitingItem("2", "phase_draft", "approve_phase", "Phase 2 Release waits for your approval", now - 240);
    draft["phase"] = 2;
    auto report = waitingItem("1", "report_escalated", "decide_report", "Report on #1 Saved cards API from Backend waits for your decision", now - 300);
    report["task"] = 1; report["member"] = "Backend";
    auto held = waitingItem("2", "task_waits_owner", "release_task", "#2 Release notes is held by you", now - 360);
    held["task"] = 2;
    return {proposal, stale, addition, permission, draft, report, held};
}
}

class TestTeamsUiWaiting : public QObject, public TeamsUiFixture {
    Q_OBJECT
private slots:
    void waitingToolOffersAreAnsweredInPlace();
    void aLeadsQuestionIsAnsweredFromWaitingForYou();
    void aConfirmedDecisionLeavesWaitingAtOnceAndARefusedOneStays();
    void aLowDriveOpensBuildFoldersAndItsNoticeNamesTheDrive();
    void aPausedTeamWaitsWithResumeTeamAndIsNeverNotified();
    void theLeadsChangesHaveTheirLabelSeenAndRevert();
    void rowsAreDismissedAndRestoredAndTheDigestIsGrouped();
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
    void teamAttentionWatchNotifiesWithoutTheWindow();
    void permissionAnswersFoldToOneLineAndOpenToFive();
    void permissionAttentionNamesTheMemberAndAnswers();
    void permissionAttentionFocusKeepsItsContext_data();
    void permissionAttentionFocusKeepsItsContext();
    void leadAnswersAMembersSubagentRequest();
    void permissionCardsAndGraphOrder();
    void followUpsShowThePermissionAnswerAndPlanFixes();
    void proposalsAreAcceptedOrRejectedOnce();
    void proposalCardsNameTheChangeAndAStaleCardSendsNothing();
    void everyOwnerGateIsACardAnsweredWithAClick();
    void aRefusedDecisionSaysWhyOnItsCard();
    void coordinatorQueuesASecondMessageWithCancel_data();
    void coordinatorQueuesASecondMessageWithCancel();
    void heldSentTakesEachLostMessageOnce();
    void coordinatorRestoresAHeldMessageThatWasNotDelivered();
    void coordinatorTakesPastedImagesAndFiles();
    void terminalDraftIsShownAndSentFromTheConversation();
    void waitingPageListsEveryKindAcrossTeams();
    void waitingBannerFoldsOpensAndHides();
    void waitingPagePagesStayStableWhileItemsArrive();
    void waitingPageEmptyAndRefused();
    void waitingItemsAreAnsweredInPlace();
    void waitingReportHeldRetryAndFinishAnswerInPlace();
    void waitingAdditionCancelDeclinesIt();
    void waitingBannerAnswersInPlaceAndWithdrawsTheNotice();
    void aWaitingReadThatChangesNothingRedrawsNothing();
    void aPhaseIsAcceptedOrSentBackFromWaitingForYou();
    void aStalledLeadShowsOnTheTeamBarAndNotifiesEachRound();
    void deferredRequestsAreAnsweredUnderDeferred();
    void phaseCheckCardShowsWarningAndSendsExplicitOverride();
};

// N32 of #499: Zerus started in the tray has no Sessions window, yet a team's
// stuck permission still reaches the notifications, once; the known items are
// shared with the window through teams/attentionNotified.
void TestTeamsUiWaiting::teamAttentionWatchNotifiesWithoutTheWindow()
{
    const auto restore = qScopeGuard([] { Settings::remove("teams/attentionNotified"); });
    Settings::remove("teams/attentionNotified");
    auto team = phasedTeam(); team["status"] = "running";
    team["attention"] = QJsonArray{QJsonObject{{"kind", "permission"}, {"member", "Backend"}, {"since", 1000}, {"minutes", 12},
        {"summary", "Backend waits for your answer: Bash cargo test"}}};
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString script = dir.filePath("fake-hgs");
    QVERIFY(FakeCli::install(script, R"PY(#!/usr/bin/env python3
import sys
a=sys.argv[1:]
if a[:2]==['team','list']:
    open(__file__+'.lists','a').write('list\n')
    print(open(__file__+'.json').read())
else:print('{}')
)PY"));
    const auto list = [&](const QJsonObject &value) {
        return FakeCli::replace(script + ".json", QJsonDocument(QJsonObject{{"items", QJsonArray{value}}}).toJson(QJsonDocument::Compact));
    };
    const auto lists = [&] { QFile file(script + ".lists"); return file.open(QIODevice::ReadOnly) ? int(file.readAll().count('\n')) : 0; };
    QVERIFY(list(team));
    HgsClient client(FakeCli::program(script));
    TeamAttentionWatch watch(&client, nullptr, 200);
    QSignalSpy raised(&watch, &TeamAttentionWatch::raised), cleared(&watch, &TeamAttentionWatch::cleared);
    QTest::qWait(400); QCOMPARE(lists(), 0);   // inactive: no reads
    watch.setActive(true);
    QTRY_COMPARE_WITH_TIMEOUT(raised.size(), 1, 10000);
    QCOMPARE(raised[0][1].toString(), QString("Team %1 needs you").arg(team["name"].toString()));
    QCOMPARE(raised[0][2].toString(), QString("Backend waits for your answer: Bash cargo test · 12 min"));
    const auto token = raised[0][0].toString();
    QCOMPARE(Settings::value("teams/attentionNotified").toStringList(), QStringList{token});
    // Later polls: the same item, no second notice.
    const int before = lists(); QTRY_VERIFY_WITH_TIMEOUT(lists() >= before + 2, 10000);
    QCOMPARE(raised.size(), 1);
    // The window took over and answered it: the watch stops; restarted, it
    // starts from what the window stored and raises nothing old.
    watch.setActive(false); const int stopped = lists(); QTest::qWait(600); QVERIFY(lists() <= stopped + 1);
    Settings::set("teams/attentionNotified", QStringList{token});
    TeamAttentionWatch again(&client, nullptr, 200);
    QSignalSpy raisedAgain(&again, &TeamAttentionWatch::raised), clearedAgain(&again, &TeamAttentionWatch::cleared);
    again.setActive(true);
    QTRY_VERIFY_WITH_TIMEOUT(lists() >= stopped + 3, 10000);
    QCOMPARE(raisedAgain.size(), 0);
    // Answered: the notice is withdrawn.
    team.remove("attention"); QVERIFY(list(team));
    QTRY_COMPARE_WITH_TIMEOUT(clearedAgain.size(), 1, 10000);
    QCOMPARE(clearedAgain[0][0].toString(), token);
    QVERIFY(Settings::value("teams/attentionNotified").toStringList().isEmpty());
}

void TestTeamsUiWaiting::permissionAnswersFoldToOneLineAndOpenToFive()
{
    // U17: a member with 20 stored answers shows one summary line; opening it
    // lists the newest five, one row each, with the long deny reason on one
    // line and its whole text in the tooltip; the choice is kept per team.
    const QString longReason = QStringLiteral(
        "This command would rewrite the build directory outside the worktree, and the lead asked to keep the cache for the next phase of the plan. ")
        + QStringLiteral("The cache is shared with the backend member and removing it would cost another full build. ")
        + QStringLiteral("Deny it and ask the lead to add a narrower command.");
    auto t = team();
    auto members = t["members"].toArray();
    auto backend = members[1].toObject();
    QJsonArray answers;
    for (int i = 0; i < 20; ++i) {
        const bool denied = i >= 18;   // the newest two are the denials
        answers.append(QJsonObject{{"question_id", QStringLiteral("q%1").arg(i)},
                                   {"decision", denied ? "deny" : "allow"},
                                   {"by", i % 2 ? "lead" : "owner"},
                                   {"at", 1700.0 + i},
                                   {"reason", denied ? longReason : QString()},
                                   {"tool", "Bash"},
                                   {"command", denied ? QStringLiteral("rm -rf build") : QStringLiteral("cargo test --locked")}});
    }
    backend["permission_answers"] = answers;
    members[1] = backend;
    t["members"] = members;
    // The task card's attempt carries its own answers (the newest last).
    auto tasks = t["tasks"].toArray();
    auto first = tasks[0].toObject();
    first["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "backend"}, {"state", "reported"},
                                               {"sent_at", 1600.0},
                                               {"permissions", QJsonArray{
                                                    QJsonObject{{"question_id", "q4"}, {"decision", "allow"}, {"by", "lead"}, {"at", 1702.0},
                                                                {"tool", "Bash"}, {"command", "cargo test --locked"}},
                                                    QJsonObject{{"question_id", "q5"}, {"decision", "deny"}, {"by", "owner"}, {"at", 1703.0},
                                                                {"reason", longReason}, {"tool", "Bash"}, {"command", "rm -rf build"}}}}}};
    tasks[0] = first;
    t["tasks"] = tasks;
    setTeam(t);
    Settings::remove("ui/expanded/teamPermissions/team-1/backend");
    Settings::remove("ui/expanded/taskPermissions/team-1/1/1");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live);
    workspace.setLive(live);
    workspace.resize(1300, 900);
    workspace.show();
    controller.refresh();
    workspace.showTeam("team-1");
    const auto card = [&workspace]() -> QFrame * {
        for (auto *frame : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (frame->property("teamMember") == "backend") return frame;
        return nullptr;
    };
    QTRY_VERIFY(card() && card()->findChild<QToolButton *>("teamMemberPermissionsToggle"));
    // Collapsed: one line, no rows.
    auto *toggle = card()->findChild<QToolButton *>("teamMemberPermissionsToggle");
    QVERIFY2(toggle->text().contains(QStringLiteral("18 allowed · 2 denied")), qPrintable(toggle->text()));
    QVERIFY2(toggle->text().contains(QStringLiteral("last:")), qPrintable(toggle->text()));
    QVERIFY(!card()->findChild<QWidget *>("teamMemberPermissions"));
    QCOMPARE(card()->findChildren<QWidget *>("teamPermissionAnswerRow").size(), 0);
    workspace.render();
    QTest::qWait(150);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u17-permissions-collapsed-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "u17-permissions-collapsed-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
    }
    // Expanded: the newest five, each one row, and "+15 more".
    card()->findChild<QToolButton *>("teamMemberPermissionsToggle")->click();
    QTRY_VERIFY(card() && card()->findChild<QWidget *>("teamMemberPermissions"));
    auto *list = card()->findChild<QWidget *>("teamMemberPermissions");
    QCOMPARE(list->findChildren<QWidget *>("teamPermissionAnswerRow").size(), 5);
    auto *more = list->findChild<QLabel *>("teamPermissionMore");
    QVERIFY(more && more->text() == QStringLiteral("+15 more"));
    QVERIFY(list->findChildren<QLabel *>("teamPermissionDecision").first()->text().contains(QStringLiteral("Denied")));
    // The newest is a denial by the lead, so the summary's last answer says so.
    QVERIFY2(card()->findChild<QToolButton *>("teamMemberPermissionsToggle")->text().contains(QStringLiteral("last: denied by the lead")),
             qPrintable(card()->findChild<QToolButton *>("teamMemberPermissionsToggle")->text()));
    // The long deny reason is one line (an ElidingLabel) with its whole text in
    // the tooltip, and so is the command.
    auto *reason = list->findChild<QLabel *>("teamPermissionReason");
    QVERIFY(reason);
    QCOMPARE(reason->text(), longReason);
    QCOMPARE(reason->toolTip(), longReason);
    QVERIFY(reason->sizePolicy().horizontalPolicy() == QSizePolicy::Ignored);
    auto *command = list->findChild<QLabel *>("teamPermissionCommand");
    QVERIFY(command && command->toolTip() == QStringLiteral("rm -rf build"));
    workspace.render();
    QTest::qWait(150);
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u17-permissions-expanded-dark");
        lightPalette();
        workspace.setTheme(false);
        workspace.render();
        QTest::qWait(250);
        shot(&workspace, "u17-permissions-expanded-light");
        darkPalette();
        workspace.setTheme(true);
        workspace.render();
        QTest::qWait(150);
        // The choice is remembered for the team: a re-render keeps it open.
        workspace.render();
        QTest::qWait(100);
    }
    QTRY_VERIFY(card() && card()->findChild<QWidget *>("teamMemberPermissions"));
    // The task card shows the same for its attempt, opened to all of its answers.
    TeamTaskCard taskCard;
    taskCard.setTask(parse(t), 1, live, true);
    QTRY_VERIFY(taskCard.findChild<QToolButton *>("teamTaskPermissionsToggle"));
    auto *taskToggle = taskCard.findChild<QToolButton *>("teamTaskPermissionsToggle");
    QVERIFY2(taskToggle->text().contains(QStringLiteral("1 allowed · 1 denied")), qPrintable(taskToggle->text()));
    QVERIFY(!taskCard.findChild<QWidget *>("teamTaskPermissions"));
    taskCard.findChild<QToolButton *>("teamTaskPermissionsToggle")->click();
    QTRY_VERIFY(taskCard.findChild<QWidget *>("teamTaskPermissions"));
    auto *taskList = taskCard.findChild<QWidget *>("teamTaskPermissions");
    QCOMPARE(taskList->findChildren<QWidget *>("teamPermissionAnswerRow").size(), 2);
    QVERIFY(taskList->findChild<QLabel *>("teamPermissionReason"));
}

// N7 (#364, U18): the owner's attention item for a permission the lead left
// unanswered names the member, what it asks and its age, and Answer leads to
// the member's Allow / Deny (or, with no answer box, puts the command on the
// clipboard). Each item is one desktop notice, raised once, withdrawn when gone.
void TestTeamsUiWaiting::permissionAttentionNamesTheMemberAndAnswers()
{
    auto team = phasedTeam();
    const QString command = QStringLiteral(R"({"request_id":"<new request id>","team":"team-1","member":"backend"} | hgs team answer --json)");
    team["attention"] = QJsonArray{
        QJsonObject{{"kind", "permission"}, {"member", "Backend"}, {"since", 1792866600.0}, {"minutes", 30},
                    {"summary", "Backend waits for your answer: Bash cargo test --locked"}, {"command", command}},
        QJsonObject{{"kind", "report"}, {"task", 2}, {"member", "Backend"}, {"since", 1792867000.0}, {"minutes", 24}}};
    auto members = team["members"].toArray(); auto backend = members[1].toObject();
    backend["question"] = memberQuestionFixture(); members[1] = backend; team["members"] = members;
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    controller.refresh(); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamAttentionAnswer") && workspace.findChild<QPushButton *>("teamMemberPermissionAllow"));
    QStringList lines;
    for (auto *line : workspace.findChildren<QLabel *>("teamAttentionLine")) lines << line->toolTip().section('\n', 0, 0);
    QCOMPARE(lines, QStringList({QStringLiteral("Backend waits for your answer: Bash cargo test --locked · 30 min"),
                                 QStringLiteral("Report on #2 from Backend waits for your decision · 24 min")}));
    for (auto *line : workspace.findChildren<QLabel *>("teamAttentionLine"))
        if (line->property("kind") == "permission") QVERIFY(line->toolTip().endsWith(command));
    QCOMPARE(workspace.findChildren<QPushButton *>("teamAttentionAnswer").size(), 1);
    shot(&workspace, "u18-permission-attention");
    // Answer: the member's Allow takes the focus.
    workspace.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&workspace));
    workspace.findChild<QPushButton *>("teamAttentionAnswer")->click();
    QTRY_COMPARE(QApplication::focusWidget(), static_cast<QWidget *>(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")));
    // A newer request must not inherit Allow focus (and a following Enter).
    auto changedQuestion = backend["question"].toObject();
    changedQuestion["question_hash"] = QStringLiteral("new-request-hash");
    changedQuestion["command"] = QStringLiteral("cargo test --new-request");
    backend["question"] = changedQuestion; members[1] = backend; team["members"] = members;
    setTeam(team);
    QTRY_VERIFY(([&] { controller.refresh(); auto *line = workspace.findChild<QLabel *>("teamMemberPermissionCommand");
                       return line && line->text() == QStringLiteral("cargo test --new-request"); })());
    QVERIFY(QApplication::focusWidget() != workspace.findChild<QPushButton *>("teamMemberPermissionAllow"));
    // No answer box from this hgs: the command goes to the clipboard.
    backend.remove("question"); members[1] = backend; team["members"] = members;
    setTeam(team);
    QTRY_VERIFY(([&] { controller.refresh(); return !workspace.findChild<QPushButton *>("teamMemberPermissionAllow") && workspace.findChild<QPushButton *>("teamAttentionAnswer"); })());
    QGuiApplication::clipboard()->clear();
    workspace.findChild<QPushButton *>("teamAttentionAnswer")->click();
    QTRY_COMPARE(QGuiApplication::clipboard()->text(), command);
    QTRY_VERIFY(workspace.findChild<QLabel *>("teamWorkspaceErrorText"));
    // The notices: one per item, raised once, withdrawn when the item is gone.
    const auto parsed = Teams::parse(team);
    TeamAttentionNotices notices;
    auto changes = notices.observe({parsed});
    QCOMPARE(changes.raised.size(), 2); QVERIFY(changes.cleared.isEmpty());
    QCOMPARE(changes.raised[0].title, QStringLiteral("Team Checkout redesign needs you"));
    QCOMPARE(changes.raised[0].body, QStringLiteral("Backend waits for your answer: Bash cargo test --locked · 30 min"));
    QCOMPARE(TeamAttentionNotices::teamOf(changes.raised[0].token), QStringLiteral("team-1"));
    auto later = team; auto items = later["attention"].toArray(); auto aged = items[0].toObject(); aged["minutes"] = 31; items[0] = aged; later["attention"] = items;
    QVERIFY(notices.observe({Teams::parse(later)}).raised.isEmpty());   // the same item, older: no second notice
    TeamAttentionNotices restarted(notices.known());
    QVERIFY(restarted.observe({Teams::parse(later)}).raised.isEmpty()); // a restart does not raise it again
    later["attention"] = QJsonArray{items[1]};
    changes = notices.observe({Teams::parse(later)});
    QVERIFY(changes.raised.isEmpty()); QCOMPARE(changes.cleared.size(), 1);
}

// Answer can outlive a hidden box or a lost focus, but never the owner's
// next focus choice, a team switch, or the exact question it was clicked for.
void TestTeamsUiWaiting::permissionAttentionFocusKeepsItsContext_data()
{
    QTest::addColumn<QString>("change");
    for (const auto &change : {"lost-focus", "another-input", "retry-same", "retry-input", "retry-team-switch",
                               "retry-question", "retry-hash", "retry-box-team"})
        QTest::newRow(change) << QString::fromLatin1(change);
}

void TestTeamsUiWaiting::permissionAttentionFocusKeepsItsContext()
{
    QFETCH(QString, change);
    auto fixture = phasedTeam();
    auto members = fixture["members"].toArray(); auto backend = members[1].toObject();
    backend["question"] = memberQuestionFixture(); members[1] = backend; fixture["members"] = members;
    fixture["attention"] = QJsonArray{QJsonObject{{"kind", "permission"}, {"member", "Backend"},
        {"summary", "Backend waits for your answer"}, {"command", "hgs team answer"}}};
    setTeam(fixture);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live); workspace.resize(1300, 900); workspace.show();
    controller.refresh(); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamAttentionAnswer")
                && workspace.findChild<QPushButton *>("teamMemberPermissionAllow"));
    workspace.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&workspace));
    // Freeze the loaded fixture: these cases control exactly when a rebuild
    // happens, rather than letting an unrelated initial poll replace the box.
    // U54: the accounts read for the member cards renders too.
    QObject::disconnect(&controller, nullptr, &workspace, nullptr);
    QObject::disconnect(&client, nullptr, &workspace, nullptr);
    QCoreApplication::processEvents(); workspace.render();
    // Outside the rebuilt Members subtree: its focus choice survives a render.
    QLineEdit other(&workspace); other.setGeometry(10, 10, 150, 24); other.show();
    const auto allow = [&] { return workspace.findChild<QPushButton *>("teamMemberPermissionAllow"); };
    if (!change.startsWith("retry-")) {
        workspace.findChild<QPushButton *>("teamAttentionAnswer")->click();
        QTRY_COMPARE(QApplication::focusWidget(), static_cast<QWidget *>(allow()));
        if (change == "another-input") {
            other.setFocus(Qt::OtherFocusReason);
            QCOMPARE(QApplication::focusWidget(), static_cast<QWidget *>(&other));
            workspace.render();
            QCOMPARE(QApplication::focusWidget(), static_cast<QWidget *>(&other));
            // Cancellation persists even if that field subsequently loses focus.
            other.clearFocus(); workspace.render();
            QVERIFY(QApplication::focusWidget() != allow());
        } else {
            allow()->clearFocus();
            QVERIFY(!QApplication::focusWidget());
            workspace.render();
            QTRY_COMPARE(QApplication::focusWidget(), static_cast<QWidget *>(allow()));
        }
        return;
    }
    // Queue the retry without depending on a slow process or a race with a
    // poll: the exact rendered box exists, but is temporarily hidden.
    other.setFocus(Qt::OtherFocusReason);
    auto *box = workspace.findChild<QFrame *>("teamMemberPermission");
    QVERIFY(box); box->hide();
    workspace.findChild<QPushButton *>("teamAttentionAnswer")->click();
    if (change == "retry-question") box->setProperty("question", "another-question");
    if (change == "retry-hash") box->setProperty("questionHash", "another-hash");
    if (change == "retry-box-team") box->setProperty("team", "another-team");
    if (change == "retry-team-switch") {
        workspace.showTeam("another-team"); workspace.showTeam("team-1");
    }
    box->show();
    if (change == "retry-input") { other.clearFocus(); other.setFocus(Qt::OtherFocusReason); }
    if (change == "retry-same") {
        QTRY_COMPARE(QApplication::focusWidget(), static_cast<QWidget *>(allow()));
    } else {
        // The one queued attempt runs at 100 ms; no callback may take focus.
        QTest::qWait(150);
        QVERIFY(QApplication::focusWidget() != allow());
        QCOMPARE(calls(script(), "answer").size(), 0);
    }
}

// U19 (#520): a team member's subagent request reaches the lead's permission
// box the same way: "Start N subagents", one row per subagent (type, model,
// effort when known, task), Allow all and Deny answering that card with its
// own ids through hgs team answer. Three from the record's list, one from the
// reason text an older hgs sends.
void TestTeamsUiWaiting::leadAnswersAMembersSubagentRequest()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const auto record = [](const QString &reason, const QJsonArray &list, int count) {
        QJsonArray ids; for (int i = 0; i < count; ++i) ids << QStringLiteral("toolu_%1").arg(i + 1);
        QJsonObject q{{"question_id", "claude-approval:subagents"}, {"question_hash", QString(64, QLatin1Char('h'))}, {"tool", "Subagents"},
                      {"command", QJsonValue()}, {"command_source", "unknown"}, {"reason", reason + "\n\nAgent\n\n{\"subagent_type\":\"Explore\"}"},
                      {"asked_at", 1792866600.0}, {"answer", QJsonObject{{"question_id", "approval"}, {"allow", "1"}, {"deny", "2"}}},
                      {"subagent_confirmation", true}, {"subagent_calls", ids}};
        if (!list.isEmpty()) q["subagents"] = list;
        return q;
    };
    auto team = phasedTeam();
    auto members = team["members"].toArray(); auto backend = members[1].toObject();
    backend["question"] = record("Start 3 subagents: Explore (model haiku, effort low): Find the callers; reviewer (model sonnet, effort high): Review the diff; "
                                 "general (model inherits claude-opus-4-6): Write the fix",
                                 QJsonArray{QJsonObject{{"id", "toolu_1"}, {"type", "Explore"}, {"model", "haiku"}, {"effort", "low"}, {"description", "Find the callers"}},
                                            QJsonObject{{"id", "toolu_2"}, {"type", "reviewer"}, {"model", "sonnet"}, {"effort", "high"}, {"description", "Review the diff"}},
                                            QJsonObject{{"id", "toolu_3"}, {"type", "general"}, {"model", "inherits claude-opus-4-6"}, {"description", "Write the fix"}}}, 3);
    members[1] = backend; team["members"] = members;
    team["attention"] = QJsonArray{QJsonObject{{"kind", "permission"}, {"member", "Backend"}, {"since", 1792866600.0}, {"minutes", 3},
                                               {"summary", "Backend waits for your answer: Subagents: Start 3 subagents"}, {"command", "hgs team answer"}}};
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QLabel *>("teamMemberSubagentsTitle"));
    QCOMPARE(workspace.findChild<QLabel *>("teamMemberSubagentsTitle")->text(), QStringLiteral("Start 3 subagents"));
    QStringList types, tasks;
    for (auto *row : workspace.findChildren<QWidget *>("teamMemberSubagent")) {
        types << row->findChild<QLabel *>("teamMemberSubagentType")->text();
        tasks << row->findChild<QLabel *>("teamMemberSubagentTask")->text();
    }
    QCOMPARE(types, (QStringList{"Explore", "reviewer", "general"}));
    QCOMPARE(tasks, (QStringList{"Find the callers", "Review the diff", "Write the fix"}));
    // Not a command: no command or agent-reason lines.
    QVERIFY(!workspace.findChild<QLabel *>("teamMemberPermissionCommand"));
    QVERIFY(!workspace.findChild<QLabel *>("teamMemberPermissionReason"));
    auto *allow = workspace.findChild<QPushButton *>("teamMemberPermissionAllow");
    QCOMPARE(allow->text(), QStringLiteral("Allow all"));
    QVERIFY(allow->isEnabled());
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->isVisible());
    // The attention strip's Answer brings the lead to the same box.
    workspace.activateWindow(); QVERIFY(QTest::qWaitForWindowActive(&workspace));
    workspace.findChild<QPushButton *>("teamAttentionAnswer")->click();
    // The window's focus: the application's is empty whenever the window is
    // briefly inactive on a loaded runner (fork builds 93 and 98).
    QTRY_COMPARE(workspace.focusWidget(), static_cast<QWidget *>(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")));
    // A status/feed refresh can arrive immediately after Answer. Rebuild
    // deterministically: the new button for this request keeps keyboard focus.
    const QPointer<QPushButton> oldAllow = workspace.findChild<QPushButton *>("teamMemberPermissionAllow");
    workspace.render();
    QVERIFY(workspace.findChild<QPushButton *>("teamMemberPermissionAllow") != oldAllow.data());
    QTRY_COMPARE(workspace.focusWidget(), static_cast<QWidget *>(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")));
    // Lost before a rebuild, it comes back to the same question's Allow.
    workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->clearFocus();
    workspace.render();
    QTRY_COMPARE(workspace.focusWidget(), static_cast<QWidget *>(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u19-lead-subagents-three-dark");
        workspace.setTheme(false); lightShot(&workspace, "u19-lead-subagents-three-light"); darkPalette(); workspace.setTheme(true);
    }
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamMemberPermissionAllow"));
    workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 1);
    auto answer = calls(script(), "answer").first();
    QCOMPARE(answer["member"].toString(), QStringLiteral("backend"));
    QCOMPARE(answer["question_id"].toString(), QStringLiteral("claude-approval:subagents"));
    QCOMPARE(answer["expected_question_hash"].toString(), QString(64, QLatin1Char('h')));
    QCOMPARE(answer["answers"].toArray(), (QJsonArray{QJsonObject{{"question_id", "approval"}, {"selected_option_ids", QJsonArray{"1"}}, {"text", ""}}}));
    // One subagent, from the reason text alone: Deny with the lead's words.
    backend["question"] = record("Start 1 subagent: Explore (model haiku): Find the callers", {}, 1);
    members[1] = backend; team["members"] = members; team["revision"] = team["revision"].toInt() + 1;
    setTeam(team);
    QTRY_VERIFY(([&] { controller.refresh(); auto *title = workspace.findChild<QLabel *>("teamMemberSubagentsTitle");
                       return title && title->text() == QStringLiteral("Start 1 subagent"); })());
    QCOMPARE(workspace.findChildren<QWidget *>("teamMemberSubagent").size(), 1);
    QCOMPARE(workspace.findChild<QLabel *>("teamMemberSubagentType")->text(), QStringLiteral("Explore"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u19-lead-subagents-one-dark");
        workspace.setTheme(false); lightShot(&workspace, "u19-lead-subagents-one-light"); darkPalette(); workspace.setTheme(true);
    }
    QTRY_VERIFY(workspace.findChild<QLineEdit *>("teamMemberPermissionDenyReason"));
    workspace.findChild<QLineEdit *>("teamMemberPermissionDenyReason")->setText("Not in this phase");
    workspace.findChild<QPushButton *>("teamMemberPermissionDeny")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 2);
    answer = calls(script(), "answer").last();
    QCOMPARE(answer["answers"].toArray(), (QJsonArray{QJsonObject{{"question_id", "approval"}, {"selected_option_ids", QJsonArray{"2"}}, {"text", "Not in this phase"}}}));
}

void TestTeamsUiWaiting::permissionCardsAndGraphOrder()
{
    // B1/B2, the card.
    auto team = deliveryTeam();
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 17) continue;
        task["description"] = QStringLiteral("Replace the captured report");
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a17"}, {"member", "backend"}, {"state", "asking"},
            {"questions", QJsonArray{
                QJsonObject{{"id", "q1"}, {"text", "Run the migration?"}, {"at", 1600}, {"tool", "Bash"},
                            {"command", "npm run migrate --force"}, {"command_source", "tool_input"},
                            {"reason", "The migration is not reversible"}},
                QJsonObject{{"id", "q2"}, {"text", "Edit the team file?"}, {"at", 1650}, {"tool", "Edit"},
                            {"command", ""}, {"command_source", "unknown"}, {"reason", "Needs a new phase"}}}},
            {"permissions", QJsonArray{
                QJsonObject{{"question_id", "q0"}, {"decision", "deny"}, {"by", "owner"}, {"at", 1700},
                            {"reason", "That kills every ninja on the machine"}},
                QJsonObject{{"question_id", "q1"}, {"decision", "allow"}, {"by", "lead"}, {"at", 1710}}}}}};
        tasks[i] = task;
    }
    team["tasks"] = tasks;
    const auto parsed = Teams::parse(team);
    const auto live = [](const Teams::Member &) { return Teams::Live{}; };
    QStringList acts; QStringList answers;
    TeamTaskCard card;
    card.act = [&](const QString &op, const QJsonObject &input) { acts << op; if (op == "answer") answers << QJsonDocument(input).toJson(QJsonDocument::Compact); };
    card.resize(440, 1100); card.show();
    card.setTask(parsed, 17, live, true);
    auto *command = card.findChild<QLabel *>("teamPermissionCommand");
    QVERIFY(command);
    QCOMPARE(command->text(), QStringLiteral("npm run migrate --force"));
    QVERIFY(card.findChild<QLabel *>("teamPermissionReason"));
    QVERIFY(card.findChild<QLabel *>("teamPermissionNoCommand"));   // the second request has none
    auto *allow = card.findChild<QPushButton *>("teamPermissionAllow");
    auto *deny = card.findChild<QPushButton *>("teamPermissionDeny");
    QVERIFY(!allow && !deny);
    QVERIFY(card.findChild<QLabel *>("teamPermissionWhere"));
    // U17: the answers fold behind one summary line; opening it lists the newest
    // first, the deny reason on one line with its whole text in the tooltip.
    auto *permissions = card.findChild<QToolButton *>("teamTaskPermissionsToggle");
    QVERIFY(permissions);
    QVERIFY2(permissions->text().contains(QStringLiteral("1 allowed · 1 denied")), qPrintable(permissions->text()));
    QVERIFY(permissions->text().contains(QStringLiteral("last: allowed by the lead")));
    QVERIFY(!card.findChild<QWidget *>("teamTaskPermissions"));
    permissions->click();
    QTRY_VERIFY(card.findChild<QWidget *>("teamTaskPermissions"));
    const auto answered = card.findChild<QWidget *>("teamTaskPermissions")->findChildren<QWidget *>("teamPermissionAnswerRow");
    QCOMPARE(answered.size(), 2);
    QVERIFY(answered[0]->findChild<QLabel *>("teamPermissionDecision")->text().contains(QStringLiteral("Allowed by the lead")));
    QVERIFY(answered[1]->findChild<QLabel *>("teamPermissionDecision")->text().contains(QStringLiteral("Denied by you")));
    auto *deniedReason = answered[1]->findChild<QLabel *>("teamPermissionReason");
    QVERIFY(deniedReason);
    QCOMPARE(deniedReason->text(), QStringLiteral("That kills every ninja on the machine"));
    QCOMPARE(deniedReason->toolTip(), QStringLiteral("That kills every ninja on the machine"));
    shot(&card, "permission-card-dark");
    lightPalette();
    card.setTask(parsed, 17, live, false);
    QTRY_VERIFY(card.findChild<QLabel *>("teamPermissionCommand"));
    shot(&card, "permission-card-light");
    darkPalette();

    // G3: the task another depends on stays above the dead end in its column.
    const auto colors = TeamUi::colorsFor(true);
    TeamUi::TaskView view(colors, TeamUi::TaskView::Wide);
    TeamUi::TaskView::Node dead; dead.id = 1; dead.layer = 0; dead.title = "Dead end"; dead.kind = "pending"; dead.statusText = "Pending"; dead.tone = colors.muted;
    TeamUi::TaskView::Node needed; needed.id = 2; needed.layer = 0; needed.title = "Needed"; needed.kind = "in_progress"; needed.statusText = "In progress"; needed.tone = colors.accent;
    TeamUi::TaskView::Node consumer; consumer.id = 3; consumer.layer = 1; consumer.title = "Consumer"; consumer.kind = "pending"; consumer.statusText = "Pending"; consumer.tone = colors.muted;
    view.nodes = {dead, needed, consumer};   // the dead end comes first in the input
    view.edges = {{3, {2}}};
    view.resize(520, 380); view.show();
    view.layout();
    QVERIFY(view.nodeRect(2).top() < view.nodeRect(1).top());
    shot(&view, "graph-dead-end-dark");

    // B5: the widening strip and the confirmation on approve.
    auto planning = phasedTeam();
    planning["status"] = "planning";
    auto members = planning["members"].toArray();
    auto first = members[1].toObject();
    first["profile_widening"] = QJsonObject{{"member", first["id"].toString()}, {"from", "read"}, {"to", "build"}};
    members[1] = first; planning["members"] = members;
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 860); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWideningStrip"));
    QVERIFY(workspace.findChild<QWidget *>("teamWideningStrip")->findChild<QLabel *>("teamWideningText")->text().contains("read → build"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "widening-strip-dark");
        workspace.setTheme(false);
        lightShot(&workspace, "widening-strip-light");
        workspace.setTheme(true); darkPalette();
    }
    // Approve & Run waits for `team status` (a list poll carries no validation).
    QPushButton *approve = nullptr;
    QTRY_VERIFY((approve = workspace.findChild<QPushButton *>("teamWorkspaceApprove")) && approve->isEnabled());
    // Declining the confirmation sends nothing; confirming sends approve with confirm_widening.
    QTimer::singleShot(0, &panel, [&panel] {
        if (auto *box = panel.findChild<QMessageBox *>("teamWideningConfirm"))
            box->button(QMessageBox::Cancel)->click();
    });
    approve->click();
    QTest::qWait(120);
    QCOMPARE(calls(script(), "approve").size(), 0);
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceApprove"));
    QTimer::singleShot(0, &panel, [&panel] {
        if (auto *box = panel.findChild<QMessageBox *>("teamWideningConfirm"))
            for (auto *b : box->buttons()) if (b->text() == QStringLiteral("Confirm and approve")) { b->click(); break; }
    });
    workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
    QTRY_COMPARE(calls(script(), "approve").size(), 1);
    QCOMPARE(calls(script(), "approve").first()["confirm_widening"].toBool(), true);
}

void TestTeamsUiWaiting::followUpsShowThePermissionAnswerAndPlanFixes()
{
    auto team = phasedTeam();
    team["status"] = "planning";
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 4) continue;
        task["kind"] = "check";
        task["check"] = QJsonObject{{"command", "cargo test --locked --bin hgs teams"}, {"expect_exit", 0}, {"timeout_minutes", 30}};
        // A finished check task: one attempt with the check it ran.
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "backend"}, {"state", "reported"},
            {"check", QJsonObject{{"exit", 0}, {"tail", "120 passed"}, {"at", 1792868400.0}}}}}; tasks[i] = task;
    }
    team["tasks"] = tasks;
    auto members = team["members"].toArray();
    auto backend = members[1].toObject();
    backend["profile_widening"] = QJsonObject{{"member", "backend"}, {"from", "read"}, {"to", "build"}};
    members[1] = backend; team["members"] = members;
    auto phases = team["phases"].toArray();
    if (phases.size() > 1) { auto draft = phases[1].toObject(); draft["owner_word"] = QJsonObject{{"text", "план принят"}, {"at", 1792868400.0}}; phases[1] = draft; team["phases"] = phases; }
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 900); workspace.show();
    panel.resize(460, 900); panel.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    // N6: the card shows the attempt's check result.
    TeamTaskCard card; card.resize(440, 1000); card.show();
    card.setTask(Teams::parse(team), 4, live, true);
    QTRY_VERIFY(card.findChild<QLabel *>("teamCheckResult"));
    QCOMPARE(card.findChild<QLabel *>("teamCheckResult")->text(), QStringLiteral("exit 0"));
    QVERIFY(card.findChild<QLabel *>("teamCheckTail"));
    // QA/N9: the approval footer lists the check command and the phase word,
    // and both are really visible inside the panel - not merely findable.
    TeamPanel planPanel(&controller);
    planPanel.resize(480, 1100); planPanel.show();
    planPanel.showTeam("team-1");
    const auto visibleLabel = [](QWidget *root, const char *name) -> QLabel * {
        for (auto *label : root->findChildren<QLabel *>(name))
            if (label->isVisible() && !label->geometry().isEmpty()) return label;
        return nullptr;
    };
    // The initial refresh already requested this unchanged fixture. Repeating
    // it in the predicate adds child processes and renders while we wait.
    WAIT_VERIFY(visibleLabel(&planPanel, "teamPlanCheck"));
    auto *checkLine = visibleLabel(&planPanel, "teamPlanCheck");
    QVERIFY(checkLine->text().contains(QStringLiteral("cargo test --locked --bin hgs teams")));
    QVERIFY(planPanel.rect().contains(checkLine->mapTo(&planPanel, QPoint(0, 0))));
    QTRY_VERIFY(visibleLabel(&planPanel, "teamPhaseOwnerWord"));
    auto *ownerLine = visibleLabel(&planPanel, "teamPhaseOwnerWord");
    QVERIFY(ownerLine->text().contains(QStringLiteral("план принят")));
    QVERIFY(planPanel.rect().contains(ownerLine->mapTo(&planPanel, QPoint(0, 0))));
    darkPalette(); planPanel.setTheme(true); planPanel.showTeam("team-1"); QTest::qWait(200);
    shot(&planPanel, "permission-answer-plan-checks-dark");
    // A second panel themed before its first render, so the light shot is light.
    TeamPanel lightPanel(&controller);
    lightPalette(); lightPanel.setTheme(false);
    lightPanel.resize(480, 1100); lightPanel.show(); lightPanel.showTeam("team-1");
    WAIT_VERIFY(visibleLabel(&lightPanel, "teamPlanCheck"));
    auto *lightCheck = visibleLabel(&lightPanel, "teamPlanCheck");
    QVERIFY(lightCheck->text().contains(QStringLiteral("cargo test --locked --bin hgs teams")));
    QVERIFY(lightPanel.rect().contains(lightCheck->mapTo(&lightPanel, QPoint(0, 0))));
    QTRY_VERIFY(visibleLabel(&lightPanel, "teamPhaseOwnerWord"));
    QVERIFY(visibleLabel(&lightPanel, "teamPhaseOwnerWord")->text().contains(QStringLiteral("план принят")));
    // Let the pinned footer lay out before the grab, and check it really is on
    // screen: the finding was labels that existed while the user saw nothing.
    auto *lightFooter = lightPanel.findChild<QWidget *>("teamPlanFooter");
    QTRY_VERIFY(lightFooter && lightFooter->isVisible() && lightFooter->height() > 24);
    QCoreApplication::processEvents(); QTest::qWait(300);
    // A late poll may re-render the panel here: wait for the shown label
    // instead of dereferencing a lookup that found none mid-render.
    QTRY_VERIFY(([&] {
        auto *shown = visibleLabel(&lightPanel, "teamPlanCheck");
        return shown && shown->mapTo(&lightPanel, QPoint(0, 0)).y() < lightPanel.height();
    })());
    shot(&lightPanel, "permission-answer-plan-checks");
    planPanel.setTheme(true); darkPalette(); planPanel.showTeam("team-1"); QTest::qWait(200);
    // N8: the footer's Approve asks for the widening confirmation.
    auto *approve = planPanel.findChild<QPushButton *>("teamApprove");
    QVERIFY(approve);
    QTimer::singleShot(0, &planPanel, [&planPanel] { if (auto *box = planPanel.findChild<QMessageBox *>("teamWideningConfirm")) box->button(QMessageBox::Cancel)->click(); });
    approve->click();
    QTest::qWait(150);
    QCOMPARE(calls(script(), "approve").size(), 0);
    QTimer::singleShot(0, &planPanel, [&planPanel] {
        if (auto *box = planPanel.findChild<QMessageBox *>("teamWideningConfirm"))
            for (auto *b : box->buttons()) if (b->text() == QStringLiteral("Confirm and approve")) { b->click(); break; }
    });
    planPanel.findChild<QPushButton *>("teamApprove")->click();
    QTRY_VERIFY(calls(script(), "approve").size() >= 1);
    QCOMPARE(calls(script(), "approve").last()["confirm_widening"].toBool(), true);
    // C4: a member's question becomes a permission card; Allow sends the exact
    // C4: the answer carries the card's own ids. The Claude question comes from
    // the backend's real status fixture; a DeepSeek card follows; a question
    // without an answer shape disables the buttons with the reason.
    const auto claudeQuestion = memberQuestionFixture();
    QVERIFY(!claudeQuestion.isEmpty());
    QCOMPARE(claudeQuestion.value("question_id").toString(), QStringLiteral("claude-approval:6f2b0e3a"));
    members = team["members"].toArray(); backend = members[1].toObject();
    backend["question"] = claudeQuestion;
    backend["permission_answers"] = QJsonArray{QJsonObject{{"question_id", "q0"}, {"decision", "deny"}, {"by", "lead"},
        {"at", 1792868300.0}, {"reason", "not now"}}};
    members[1] = backend; team["members"] = members;
    setTeam(team); workspace.showTeam("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QLabel *>("teamMemberPermissionCommand") != nullptr; })());
    QCOMPARE(workspace.findChild<QLabel *>("teamMemberPermissionCommand")->text(), QStringLiteral("cargo test --locked"));
    QVERIFY(workspace.findChild<QLabel *>("teamMemberPermissionReason"));
    const auto claudeAnswer = [&] { return calls(script(), "answer").last(); };
    auto *allow = workspace.findChild<QPushButton *>("teamMemberPermissionAllow");
    QVERIFY(allow);
    allow->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 1);
    QCOMPARE(claudeAnswer()["member"].toString(), QStringLiteral("backend"));
    QCOMPARE(claudeAnswer()["question_id"].toString(), QStringLiteral("claude-approval:6f2b0e3a"));
    QCOMPARE(claudeAnswer()["expected_question_hash"].toString(), QStringLiteral("6f2b0e3a"));
    auto native = claudeAnswer()["answers"].toArray()[0].toObject();
    QCOMPARE(native["question_id"].toString(), QStringLiteral("approval"));
    QCOMPARE(native["selected_option_ids"].toArray()[0].toString(), QStringLiteral("1"));
    // Deny sends the card's deny option and the reason as the text.
    auto *deny = workspace.findChild<QPushButton *>("teamMemberPermissionDeny");
    QVERIFY(deny);
    workspace.findChild<QLineEdit *>("teamMemberPermissionDenyReason")->setText(QStringLiteral("not this command"));
    deny->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 2);
    native = claudeAnswer()["answers"].toArray()[0].toObject();
    QCOMPARE(native["question_id"].toString(), QStringLiteral("approval"));
    QCOMPARE(native["selected_option_ids"].toArray()[0].toString(), QStringLiteral("2"));
    QCOMPARE(native["text"].toString(), QStringLiteral("not this command"));
    // U17: the stored answer folds to one summary line on the member card.
    auto *answerSummary = workspace.findChild<QToolButton *>("teamMemberPermissionsToggle");
    QVERIFY(answerSummary);
    QVERIFY2(answerSummary->text().contains(QStringLiteral("1 denied")), qPrintable(answerSummary->text()));
    QVERIFY(!workspace.findChild<QLabel *>("teamMemberPermissionAnswered"));
    shot(&workspace, "permission-answer-claude");
    // A DeepSeek card: its own question id and allow/deny native ids.
    members = team["members"].toArray(); backend = members[1].toObject();
    backend["question"] = QJsonObject{{"question_id", "deepseek-approval:9c1d"}, {"question_hash", "9c1d"}, {"tool", "Terminal"},
        {"command", "make test"}, {"command_source", "tool_input"}, {"reason", "run the suite"}, {"asked_at", 1792868400.0},
        {"answer", QJsonObject{{"question_id", "decision"}, {"allow", "allow"}, {"deny", "deny"}}}};
    backend["permission_answers"] = QJsonArray{};
    members[1] = backend; team["members"] = members;
    setTeam(team); workspace.showTeam("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QLabel *>("teamMemberPermissionCommand")
                       && workspace.findChild<QLabel *>("teamMemberPermissionCommand")->text() == QStringLiteral("make test"); })());
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->isEnabled());
    workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 3);
    QCOMPARE(claudeAnswer()["question_id"].toString(), QStringLiteral("deepseek-approval:9c1d"));
    QCOMPARE(claudeAnswer()["expected_question_hash"].toString(), QStringLiteral("9c1d"));
    native = claudeAnswer()["answers"].toArray()[0].toObject();
    QCOMPARE(native["question_id"].toString(), QStringLiteral("decision"));
    QCOMPARE(native["selected_option_ids"].toArray()[0].toString(), QStringLiteral("allow"));
    shot(&workspace, "permission-answer-deepseek");
    // No answer shape: the buttons are disabled and say why.
    members = team["members"].toArray(); backend = members[1].toObject();
    backend["question"] = QJsonObject{{"question_id", "old:h"}, {"question_hash", "h"}, {"tool", "Bash"}, {"command", "rm -rf build"},
        {"command_source", "tool_input"}, {"reason", "clean"}, {"asked_at", 1792868400.0}};
    backend["permission_answers"] = QJsonArray{};
    members[1] = backend; team["members"] = members;
    setTeam(team); workspace.showTeam("team-1");
    QTRY_VERIFY(([&] { controller.refresh(); auto *b = workspace.findChild<QPushButton *>("teamMemberPermissionAllow");
                       return b && !b->isEnabled(); })());
    QVERIFY(workspace.findChild<QPushButton *>("teamMemberPermissionAllow")->toolTip().contains(QStringLiteral("no answer ids")));
    QCOMPARE(calls(script(), "answer").size(), 3);
}

// F8, U57: the lead's open proposals are cards in the coordinator's
// conversation, drawn like the member permission card. Deny carries its
// reason; remove and swap ask first, a cancel sends nothing; every answer
// carries the proposal's hash and goes out once.
void TestTeamsUiWaiting::proposalsAreAcceptedOrRejectedOnce()
{
    setTeam(proposalsTeam(QDateTime::currentSecsSinceEpoch() + 3600));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto cardOf = [&](const QString &id) -> QFrame * {
        for (auto *card : workspace.findChildren<QFrame *>("teamProposalCard")) if (card->property("proposal") == id) return card;
        return nullptr;
    };
    QTRY_VERIFY(cardOf("p1") && cardOf("p2") && cardOf("p3"));
    QVERIFY(!cardOf("p4"));   // decided ones are gone
    QVERIFY(!workspace.findChild<QFrame *>("teamWorkspaceProposals"));
    // In the lead's conversation, above the composer.
    auto *conversation = workspace.findChild<QWidget *>("teamConversation");
    QVERIFY(conversation && conversation->isAncestorOf(cardOf("p1")));
    QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add Designer (claude, sonnet, default account)"));
    QCOMPARE(cardOf("p2")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Swap Backend to codex, gpt-5.5"));
    QCOMPARE(cardOf("p3")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Remove Tester"));
    QCOMPARE(cardOf("p3")->property("hash").toString(), QStringLiteral("h3"));
    QVERIFY(cardOf("p1")->findChild<QLabel *>("teamProposalBy")->text().startsWith("Lead asks"));
    QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalReason")->text(), QStringLiteral("Lead: The UI needs a designer"));
    // The member permission card's parts: a Deny reason, Deny and Allow.
    for (const char *name : {"teamProposalDenyReason", "teamProposalDeny", "teamProposalAllow"}) QVERIFY(cardOf("p1")->findChild<QWidget *>(name));
    QVERIFY(cardOf("p1")->findChild<QPushButton *>("teamProposalDeny")->property("danger").toBool());
    QVERIFY(cardOf("p1")->findChild<QPushButton *>("teamProposalAllow")->property("primary").toBool());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u57-proposal-cards");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u57-proposal-cards-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    // A typed reason survives a rebuild; Deny sends it with the hash.
    QTRY_VERIFY(cardOf("p1"));
    cardOf("p1")->findChild<QLineEdit *>("teamProposalDenyReason")->setText("Not before the release");
    workspace.setTheme(true); controller.refresh();
    QTRY_VERIFY(cardOf("p1"));
    QCOMPARE(cardOf("p1")->findChild<QLineEdit *>("teamProposalDenyReason")->text(), QStringLiteral("Not before the release"));
    cardOf("p1")->findChild<QPushButton *>("teamProposalDeny")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    auto input = calls(script(), "decide-proposal").first();
    QCOMPARE(input["proposal"].toString(), QStringLiteral("p1"));
    QVERIFY(!input["accept"].toBool());
    QCOMPARE(input["expected_hash"].toString(), QStringLiteral("h1"));
    QCOMPARE(input["reason"].toString(), QStringLiteral("Not before the release"));
    QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
    // A swap asks first: Cancel sends nothing; the confirmation sends once.
    QTRY_VERIFY(cardOf("p2"));
    armConfirm(&panel, true);
    cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "decide-proposal").size(), 1);
    QTRY_VERIFY(cardOf("p2") && cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
    armConfirm(&panel, false);
    cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 2);
    input = calls(script(), "decide-proposal").last();
    QCOMPARE(input["proposal"].toString(), QStringLiteral("p2"));
    QVERIFY(input["accept"].toBool());
    QCOMPARE(input["expected_hash"].toString(), QStringLiteral("h2"));
    QVERIFY(!input.contains("reason"));
    // Task 107: confirmed by hgs, the card leaves at once, before the store's read.
    QTRY_VERIFY(!cardOf("p2"));
    // A remove asks too.
    armConfirm(&panel, false);
    cardOf("p3")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 3);
    QCOMPARE(calls(script(), "decide-proposal").last()["proposal"].toString(), QStringLiteral("p3"));
    QCOMPARE(calls(script(), "decide-proposal").last()["expected_hash"].toString(), QStringLiteral("h3"));
}

// U57: a card names the exact change of every new kind (the members, a running
// phase's new tasks, the tasks to cancel, the phase to discard). A proposal
// that changed while the owner confirmed sends nothing and says so; a
// refusal or an earlier decision shows on its card.
void TestTeamsUiWaiting::proposalCardsNameTheChangeAndAStaleCardSendsNothing()
{
    auto running = phasedTeam();
    const QJsonObject designer{{"id", "designer"}, {"role", "Designer"}, {"agent", "claude"}, {"model", "sonnet"}};
    const QJsonObject writer{{"id", "writer"}, {"role", "Writer"}, {"agent", "codex"}};
    running["proposals"] = QJsonArray{
        QJsonObject{{"id", "p1"}, {"kind", "add-member"}, {"by", "Lead"}, {"at", 2000.0}, {"state", "open"}, {"hash", "h1"},
                    {"reason", "Two pages need design and copy"}, {"change", QJsonObject{{"members", QJsonArray{designer, writer}}}}},
        QJsonObject{{"id", "p2"}, {"kind", "add-to-phase"}, {"by", "Lead"}, {"at", 2100.0}, {"state", "open"}, {"hash", "h2"},
                    {"reason", "QA found two gaps"},
                    {"change", QJsonObject{{"phase", 2}, {"tasks", QJsonArray{QJsonObject{{"title", "Saved card list"}, {"assignee", "backend"}},
                                                                             QJsonObject{{"title", "Empty state"}}}}}}},
        QJsonObject{{"id", "p3"}, {"kind", "cancel-tasks"}, {"by", "Lead"}, {"at", 2200.0}, {"state", "open"}, {"hash", "h3"},
                    {"reason", "The tests moved to phase 3"}, {"change", QJsonObject{{"tasks", QJsonArray{4}}}}},
        QJsonObject{{"id", "p4"}, {"kind", "discard-phase"}, {"by", "Lead"}, {"at", 2300.0}, {"state", "open"}, {"hash", "h4"},
                    {"reason", "Nothing in it started"}, {"change", QJsonObject{{"phase", 3}}}}};
    setTeam(running);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1400, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto cardOf = [&](const QString &id) -> QFrame * {
        for (auto *card : workspace.findChildren<QFrame *>("teamProposalCard")) if (card->property("proposal") == id) return card;
        return nullptr;
    };
    const auto linesOf = [&](const QString &id) {
        QStringList lines; for (auto *line : cardOf(id)->findChildren<QLabel *>("teamProposalChange")) lines << line->text();
        return lines;
    };
    const auto noticeOf = [&](const QString &id) {
        auto *notice = cardOf(id) ? cardOf(id)->findChild<QLabel *>("teamProposalNotice") : nullptr;
        return notice ? notice->text() : QString();
    };
    QTRY_VERIFY(cardOf("p1") && cardOf("p2") && cardOf("p3") && cardOf("p4"));
    QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add 2 members"));
    QCOMPARE(linesOf("p1"), (QStringList{"Designer (claude, sonnet, default account)", "Writer (codex, default account)"}));
    QCOMPARE(cardOf("p2")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add 2 tasks to phase 2 (QA fixes)"));
    QCOMPARE(linesOf("p2"), (QStringList{"Saved card list, for Backend", "Empty state"}));
    QCOMPARE(cardOf("p3")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Cancel 1 task"));
    QCOMPARE(linesOf("p3").size(), 1);
    QVERIFY2(linesOf("p3").first().startsWith("#4 Unit tests ("), qPrintable(linesOf("p3").first()));
    QCOMPARE(cardOf("p4")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Discard phase 3 (Polish)"));
    QCOMPARE(linesOf("p4").size(), 1);
    QVERIFY2(linesOf("p4").first().startsWith("#5 Code review ("), qPrintable(linesOf("p4").first()));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u57-proposal-kinds");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u57-proposal-kinds-light");
        darkPalette();
        workspace.setTheme(true); workspace.render();
    }
    // The lead changes p3 while the owner reads the confirmation: nothing is sent.
    QTRY_VERIFY(cardOf("p3"));
    armConfirmAfter(&panel, [&] {
        auto proposals = running["proposals"].toArray();
        auto changed = proposals[2].toObject();
        changed["hash"] = "h3b"; changed["change"] = QJsonObject{{"tasks", QJsonArray{4, 5}}};
        proposals[2] = changed; running["proposals"] = proposals;
        setTeam(running);
        controller.refresh();
        QTRY_VERIFY(([&] {
            const auto *watched = controller.watched();
            if (!watched) return false;
            for (const auto &proposal : watched->proposals) if (proposal.id == "p3") return proposal.hash == "h3b";
            return false;
        })());
    });
    cardOf("p3")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_VERIFY(noticeOf("p3").contains("changed this proposal"));
    QVERIFY(calls(script(), "decide-proposal").isEmpty());
    QCOMPARE(cardOf("p3")->property("hash").toString(), QStringLiteral("h3b"));
    QCOMPARE(cardOf("p3")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Cancel 2 tasks"));
    // A server refusal of a changed card stays on it; nothing applied.
    { QFile refuse(script() + ".proposalrefuse"); QVERIFY(refuse.open(QIODevice::WriteOnly));
      refuse.write("hgs: The proposal changed; reload the card and answer again with its hash"); }
    cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    QCOMPARE(calls(script(), "decide-proposal").first()["expected_hash"].toString(), QStringLiteral("h2"));
    QTRY_COMPARE(noticeOf("p2"), QStringLiteral("Allow was refused, nothing was applied: The proposal changed; reload the card and answer again with its hash"));
    QTRY_VERIFY(cardOf("p2") && cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
    QFile::remove(script() + ".proposalrefuse");
    // Discard asks; an earlier decision (a replay) shows on a card of its own.
    { QFile status(script() + ".proposalstatus"); QVERIFY(status.open(QIODevice::WriteOnly)); status.write("rejected"); }
    armConfirm(&panel, false);
    cardOf("p4")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 2);
    QCOMPARE(calls(script(), "decide-proposal").last()["proposal"].toString(), QStringLiteral("p4"));
    QCOMPARE(calls(script(), "decide-proposal").last()["expected_hash"].toString(), QStringLiteral("h4"));
    QTRY_VERIFY(noticeOf("p4").contains("already denied"));
    QFile::remove(script() + ".proposalstatus");
    auto proposals = running["proposals"].toArray();
    auto decided = proposals[3].toObject(); decided["state"] = "rejected"; proposals[3] = decided;
    running["proposals"] = proposals; setTeam(running);
    QTRY_VERIFY(([&] { controller.refresh(); return cardOf("p4") && cardOf("p4")->property("stale").toBool(); })());
    QVERIFY(!cardOf("p4")->findChild<QPushButton *>("teamProposalAllow"));
    cardOf("p4")->findChild<QPushButton *>("teamProposalDismiss")->click();
    QTRY_VERIFY(!cardOf("p4"));
}

// Task #21 (owner, 2026-11-05): every owner-gated operation is a card the
// owner answers with a click, never a typed phrase. Each kind is drawn as hgs
// writes it (its summary and lines), Allow sends that card's decision with its
// hash once, a drain asks first, and Deny sends a rejection.
void TestTeamsUiWaiting::everyOwnerGateIsACardAnsweredWithAClick()
{
    struct Kind { const char *kind, *label; QJsonObject input; QString summary; QStringList lines; };
    const QList<Kind> kinds{
        {"relaunch", "Relaunch", {{"member", "backend"}}, "Relaunch Backend", {}},
        {"scale", "Members", {{"role", "Tester"}, {"agent", "claude"}, {"count", 2}}, "Add 2 × Tester", {"claude"}},
        {"drain", "Drain", {{"member", "tester"}}, "Drain Tester", {"It finishes its open work, takes nothing new, then its session is paused and it leaves the team."}},
        {"approve", "Approval", {}, "Approve the plan and run it", {"5 tasks, 2 members"}},
        {"approve-phase", "Approval", {{"phase", 2}}, "Approve phase 2 (QA fixes)", {}},
        {"return-to-draft", "Phase", {{"phase", 2}}, "Return phase 2 (QA fixes) to a draft", {}},
        {"edit-draft", "Phase", {{"phase", 3}, {"tasks_update", QJsonArray{QJsonObject{{"id", 5}, {"title", "Review"}}}}}, "Change phase 3 (Polish)", {"1 task changes"}},
        {"move-phase", "Phase", {{"phase", 3}, {"position", "before 2"}}, "Move phase 3 (Polish) before 2", {}},
        {"unpause-phase", "Phase", {{"phase", 2}}, "Remove the pause of phase 2 (QA fixes)", {}},
        {"phase-acceptance", "Phase", {{"phase", 3}, {"acceptance", "owner"}}, "phase 3 (Polish) is accepted by owner", {}},
        {"accept-phase", "Phase", {{"phase", 2}}, "Accept phase 2 (QA fixes)", {}},
        {"extend", "Phase", {{"tasks", QJsonArray{QJsonObject{{"title", "Load test"}}}}}, "Add a phase of 1 tasks", {"Load test"}},
        {"settings", "Setting", {{"settings", QJsonObject{{"max_members", 8}}}}, "Change the team's settings", {"max_members: 6 → 8"}},
        {"switch-setting", "Setting", {{"settings", QJsonObject{{"full_tests", "allowed"}}}}, "Change the team's settings", {"full_tests: \"ask\" → \"allowed\""}},
        {"full-tests", "Tests", {{"release", "v1.2"}}, "Allow one full test run for v1.2", {}},
        {"iron-rules", "Iron rules", {{"scope", "project"}, {"project", "proj-1"}, {"rules", QJsonArray{"Never push"}}, {"expected_revision", 3}},
         "Set the project's iron rules (proj-1)", {"Never push"}},
    };
    auto running = phasedTeam();
    QJsonArray proposals;
    int n = 0;
    for (const auto &kind : kinds) {
        ++n;
        QJsonArray lines; for (const auto &line : kind.lines) lines << line;
        const QString op = kind.kind == QLatin1String("switch-setting") ? QStringLiteral("settings") : QString::fromLatin1(kind.kind);
        QJsonObject change{{"op", op}, {"input", kind.input}, {"summary", kind.summary}, {"lines", lines}};
        if (kind.kind == QLatin1String("switch-setting")) { change["setting"] = "full_tests"; change["value"] = "allowed"; }
        proposals << QJsonObject{{"id", QStringLiteral("p%1").arg(n)}, {"kind", kind.kind}, {"by", "Lead"}, {"at", 2000.0 + n}, {"state", "open"},
                                 {"hash", QStringLiteral("h%1").arg(n)}, {"reason", QStringLiteral("because %1").arg(n)}, {"change", change}};
    }
    // One more settings card, answered with Deny.
    proposals << QJsonObject{{"id", "p99"}, {"kind", "settings"}, {"by", "Lead"}, {"at", 2999.0}, {"state", "open"}, {"hash", "h99"}, {"reason", "smaller"},
                             {"change", QJsonObject{{"op", "settings"}, {"input", QJsonObject{{"settings", QJsonObject{{"max_members", 3}}}}},
                                                    {"summary", "Change the team's settings"}, {"lines", QJsonArray{"max_members: 6 → 3"}}}}};
    running["proposals"] = proposals;
    setTeam(running);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1400, 900); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto cardOf = [&](const QString &id) -> QFrame * {
        for (auto *card : workspace.findChildren<QFrame *>("teamProposalCard")) if (card->property("proposal") == id) return card;
        return nullptr;
    };
    const auto linesOf = [&](const QString &id) {
        QStringList lines; for (auto *line : cardOf(id)->findChildren<QLabel *>("teamProposalChange")) lines << line->text();
        return lines;
    };
    QTRY_VERIFY(cardOf("p1") && cardOf("p99"));
    n = 0;
    for (const auto &kind : kinds) {
        const QString id = QStringLiteral("p%1").arg(++n);
        QTRY_VERIFY2(cardOf(id), kind.kind);
        auto *card = cardOf(id);
        QCOMPARE(card->property("kind").toString(), QString::fromLatin1(kind.kind));
        QCOMPARE(card->findChild<QLabel *>("teamProposalWhat")->text(), kind.summary);
        QCOMPARE(linesOf(id), kind.lines);
        // The kind's pill names it in words, never the raw kind.
        QStringList pills; for (auto *widget : card->findChildren<QWidget *>()) pills << widget->toolTip();
        QVERIFY2(pills.contains(QString::fromLatin1(kind.label)), qPrintable(QStringLiteral("%1: %2").arg(kind.kind, pills.join(" | "))));
        // No typed phrase anywhere: Allow and Deny are the answer.
        QVERIFY(card->findChild<QPushButton *>("teamProposalAllow") && card->findChild<QPushButton *>("teamProposalDeny"));
        QVERIFY(!card->findChild<QLineEdit *>("teamProposalAnswerText"));
    }
    if (shotsWanted()) shot(&workspace, "task21-owner-gate-cards");
    n = 0;
    for (const auto &kind : kinds) {
        const QString id = QStringLiteral("p%1").arg(++n);
        const int before = int(calls(script(), "decide-proposal").size());
        QTRY_VERIFY2(cardOf(id) && cardOf(id)->findChild<QPushButton *>("teamProposalAllow")->isEnabled(), kind.kind);
        if (kind.kind == QLatin1String("drain")) {
            // A drain asks first; Cancel sends nothing.
            armConfirm(&panel, true);
            cardOf(id)->findChild<QPushButton *>("teamProposalAllow")->click();
            QTest::qWait(100);
            QCOMPARE(int(calls(script(), "decide-proposal").size()), before);
            QTRY_VERIFY(cardOf(id) && cardOf(id)->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
            armConfirm(&panel, false);
        }
        QFile held(script() + ".proposalhold");
        QVERIFY(held.open(QIODevice::WriteOnly)); held.close();
        cardOf(id)->findChild<QPushButton *>("teamProposalAllow")->click();
        QTRY_COMPARE(int(calls(script(), "decide-proposal").size()), before + 1);
        const auto input = calls(script(), "decide-proposal").last();
        QCOMPARE(input["proposal"].toString(), id);
        QVERIFY2(input["accept"].toBool(), kind.kind);
        QCOMPARE(input["expected_hash"].toString(), QStringLiteral("h%1").arg(n));
        QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
        // Answered once: until the store takes it, its buttons stay off.
        QTRY_VERIFY(cardOf(id) && !cardOf(id)->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
        QVERIFY(QFile::remove(script() + ".proposalhold"));
        QTRY_VERIFY(!cardOf(id));
    }
    // Deny sends a rejection with the card's hash; nothing else is sent.
    const int before = int(calls(script(), "decide-proposal").size());
    cardOf("p99")->findChild<QLineEdit *>("teamProposalDenyReason")->setText("keep six");
    cardOf("p99")->findChild<QPushButton *>("teamProposalDeny")->click();
    QTRY_COMPARE(int(calls(script(), "decide-proposal").size()), before + 1);
    const auto denied = calls(script(), "decide-proposal").last();
    QCOMPARE(denied["proposal"].toString(), QStringLiteral("p99"));
    QVERIFY(!denied["accept"].toBool());
    QCOMPARE(denied["expected_hash"].toString(), QStringLiteral("h99"));
    QCOMPARE(denied["reason"].toString(), QStringLiteral("keep six"));
}

// #754 (owner, 2026-10-31: "I press Allow and nothing happens"): a refused
// decision says why on its card, in the server's words, on the team view and
// in Waiting for you; the card stays with Allow and Deny to answer again, and
// a later Deny goes through.
void TestTeamsUiWaiting::aRefusedDecisionSaysWhyOnItsCard()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    setTeam(answerTeam());
    QVERIFY(writeWaiting(script() + ".waiting", answerItems(now)));
    const QString words = "Task #3 depends on #9, which is neither in the plan nor among the new tasks";
    { QFile refuse(script() + ".proposalrefuse"); QVERIFY(refuse.open(QIODevice::WriteOnly)); refuse.write(("hgs: " + words).toUtf8()); }
    const QString refused = "Allow was refused, nothing was applied: " + words;
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    // The team view: the card in the lead's conversation.
    {
        TeamWorkspace workspace(&controller, &panel);
        workspace.setLive(live);
        workspace.resize(1300, 820); workspace.show();
        controller.refresh();
        panel.showTeam("team-1");
        workspace.showTeam("team-1");
        const auto cardOf = [&](const QString &id) -> QFrame * {
            for (auto *card : workspace.findChildren<QFrame *>("teamProposalCard")) if (card->property("proposal") == id) return card;
            return nullptr;
        };
        QTRY_VERIFY(cardOf("p1"));
        cardOf("p1")->findChild<QPushButton *>("teamProposalAllow")->click();
        QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
        QTRY_VERIFY(cardOf("p1") && cardOf("p1")->findChild<QLabel *>("teamProposalNotice")
                    && cardOf("p1")->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
        QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalNotice")->text(), refused);
        QTRY_VERIFY(cardOf("p1") && cardOf("p1")->findChild<QLabel *>("teamProposalNotice") && cardOf("p1")->findChild<QLabel *>("teamProposalNotice")->isVisible());
        QVERIFY(cardOf("p1")->findChild<QPushButton *>("teamProposalDeny")->isEnabled());
        // A refresh keeps it saying so.
        controller.refresh(); TestWait::settle();
        QVERIFY(cardOf("p1") && cardOf("p1")->findChild<QLabel *>("teamProposalNotice"));
        if (shotsWanted()) {
            shot(&workspace, "u754-team-refused-dark");
            workspace.setTheme(false); workspace.render();
            lightShot(&workspace, "u754-team-refused-light");
            darkPalette(); workspace.setTheme(true); workspace.render();
        }
    }
    // Waiting for you: the same card in its row.
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 1000); page.show();
    const QString proposal = "team-1/owner_word_asked/proposal-p1";
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(row(proposal));
    row(proposal)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(proposal)->findChild<QFrame *>("teamProposalCard"));
    row(proposal)->findChild<QFrame *>("teamProposalCard")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 2);
    const auto card = [&] { return row(proposal) ? row(proposal)->findChild<QFrame *>("teamProposalCard") : nullptr; };
    QTRY_VERIFY(card() && card()->findChild<QLabel *>("teamProposalNotice") && card()->isEnabled()
                && card()->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
    QCOMPARE(card()->findChild<QLabel *>("teamProposalNotice")->text(), refused);
    QTRY_VERIFY(card() && card()->findChild<QLabel *>("teamProposalNotice") && card()->findChild<QLabel *>("teamProposalNotice")->isVisible());
    QVERIFY(!row(proposal)->findChild<QLabel *>("teamWaitingAnswerSending"));
    QVERIFY(answered.isEmpty());
    TestWait::settle();
    if (shotsWanted()) {
        shot(&page, "u754-waiting-refused-dark");
        page.setDark(false);
        lightShot(&page, "u754-waiting-refused-light");
        darkPalette(); page.setDark(true);
    }
    // The owner denies it instead: that goes through and the row leaves.
    QFile::remove(script() + ".proposalrefuse");
    QTRY_VERIFY(card());
    card()->findChild<QLineEdit *>("teamProposalDenyReason")->setText("Renumber it first");
    card()->findChild<QPushButton *>("teamProposalDeny")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 3);
    QCOMPARE(calls(script(), "decide-proposal").last()["accept"].toBool(), false);
    QTRY_VERIFY(!row(proposal));
    QCOMPARE(answered, QStringList{proposal});
}

// U35: the coordinator's composer, against an hgs that holds a message while
// the lead has not taken the previous one. The held message shows "Queued
// until the lead takes your previous message" with Cancel; Cancel sends
// send-cancel with its request ID and returns the text; once hgs delivers it,
// the line goes. A dialog in the lead's terminal is refused with Open terminal.
// #511 N6: the designer's layout keeps both in its composer, under the messages.
void TestTeamsUiWaiting::coordinatorQueuesASecondMessageWithCancel_data()
{
    QTest::addColumn<bool>("designer");
    QTest::newRow("coordinator") << false;
    QTest::newRow("designer") << true;
}

void TestTeamsUiWaiting::coordinatorQueuesASecondMessageWithCancel()
{
    QFETCH(bool, designer);
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
held=json.load(open(here+'.held')) if os.path.exists(here+'.held') else []
if a[:1]==['inspect']:
 print(json.dumps({'name':a[1],'run_id':'run-1','conversation_id':'conv-1','runtime_state':'live','process_state':'running',
  'activity':'busy','phase':'working','cursor':0,'events':[],'input_hold_supported':True,'held_inputs':held}))
 open(here+'.inspected','a').close()
elif a[:1]==['send']:
 body=json.load(sys.stdin)
 open(here+'.sends','a').write(json.dumps(body)+'\n')
 if os.path.exists(here+'.dialog'):
  sys.stderr.write('hgs: terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt\n');sys.exit(1)
 status='queued' if body.get('queue_when_unconfirmed') else 'submitted'
 if status=='queued':
  held.append({'request_id':body['request_id'],'text':body['text'],'queued_at':1.0});json.dump(held,open(here+'.held','w'))
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'run_id':'run-1','conversation_id':'conv-1','first_message':False,
  'status':status,'text':body['text']}))
elif a[:1]==['send-cancel']:
 body=json.load(sys.stdin)
 open(here+'.cancels','a').write(json.dumps(body)+'\n')
 json.dump([h for h in held if h['request_id']!=body['request_id']],open(here+'.held','w'))
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'status':'cancelled'}))
else:print('{}')
)PY"));
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamConversation conversation(&client);
    const QString agent = designer ? QStringLiteral("the designer") : QStringLiteral("the lead");
    conversation.setAgentName(agent);
    QString opened;
    conversation.openTerminal = [&](const QString &host, const QString &name) { opened = host + '|' + name; };
    if (designer) conversation.useDesignerLayout(new QLabel("Describe the team"), nullptr);
    conversation.setSession({}, "claude/checkout/lead");
    conversation.resize(560, 420); conversation.show();
    const QString suffix = designer ? QStringLiteral("-designer") : QString();
    // The designer's held line and Open terminal: in its composer, above the
    // field, never in the hidden coordinator row or at the top of the column.
    const auto inComposer = [&](QWidget *w) {
        if (!designer) return true;
        auto *composer = conversation.findChild<QWidget *>("teamDesignerComposer");
        auto *card = conversation.findChild<QWidget *>("teamDesignerComposerCard");
        auto *view = conversation.findChild<QWidget *>("teamConversationActivity");
        const int top = w->mapTo(&conversation, QPoint()).y(), bottom = top + w->height();
        return composer->isAncestorOf(w) && view->isVisible()
            && top >= view->mapTo(&conversation, QPoint()).y() + view->height()
            && bottom <= card->mapTo(&conversation, QPoint()).y()
            && composer->mapTo(&conversation, QPoint()).y() + composer->height() <= conversation.height();
    };
    auto *input = conversation.findChild<QPlainTextEdit *>("teamConversationInput");
    auto *sendButton = conversation.findChild<QPushButton *>("teamConversationSend");
    auto *held = conversation.findChild<QFrame *>("teamConversationHeld");
    const auto sends = [&] {
        QList<QJsonObject> found; QFile file(program + ".sends");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) found << QJsonDocument::fromJson(line).object();
        return found;
    };
    const auto rows = [&] { return held->findChildren<QWidget *>("heldInput"); };
    // The send asks hgs to hold it: this hgs reads input_hold_supported.
    QTRY_VERIFY(sendButton->isEnabled());
    // Sent only once the session is read: a slow first read must not refuse it.
    QTRY_VERIFY(QFile::exists(program + ".inspected"));
    QTest::qWait(300);
    input->setPlainText("Also re-run the migrations"); sendButton->click();
    QTRY_COMPARE(sends().size(), 1);
    QVERIFY(sends().first()["queue_when_unconfirmed"].toBool());
    QTRY_COMPARE(rows().size(), 1);
    QVERIFY(held->isVisible());
    QCOMPARE(held->findChild<QLabel *>("heldInputText")->text(), QStringLiteral("Queued until %1 takes your previous message").arg(agent));
    // The held row can be read before the send's own messageSent callback;
    // wait for the callback, which is what clears the composer.
    QTRY_COMPARE(input->toPlainText(), QString());
    QTRY_VERIFY(inComposer(held));
    // No quick-ask chips here either.
    QVERIFY(conversation.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&conversation, "u35-coordinator-queued" + suffix + "-dark");
        lightPalette(); conversation.setTheme(false); lightShot(&conversation, "u35-coordinator-queued" + suffix + "-light"); darkPalette(); conversation.setTheme(true);
    }
    // Cancel: hgs is told its request ID; the line goes and the text comes back.
    held->findChild<QPushButton *>("heldInputCancel")->click();
    QTRY_VERIFY(QFile::exists(program + ".cancels"));
    QFile cancels(program + ".cancels"); QVERIFY(cancels.open(QIODevice::ReadOnly));
    QCOMPARE(QJsonDocument::fromJson(cancels.readAll().trimmed()).object()["request_id"].toString(), sends().first()["request_id"].toString());
    QTRY_VERIFY(rows().isEmpty());
    QTRY_VERIFY(!held->isVisible());
    QCOMPARE(input->toPlainText(), QStringLiteral("Also re-run the migrations"));
    // Held again, then delivered by hgs: the line clears on the next read.
    sendButton->click();
    QTRY_COMPARE(sends().size(), 2);
    QTRY_COMPARE(input->toPlainText(), QString());
    QTRY_COMPARE(rows().size(), 1);
    QVERIFY(FakeCli::replace(program + ".held", "[]"));
    QTRY_VERIFY(rows().isEmpty());
    // A dialog in the lead's terminal is refused, never held: Open terminal.
    QFile dialog(program + ".dialog"); QVERIFY(dialog.open(QIODevice::WriteOnly)); dialog.close();
    input->setPlainText("One more thing"); sendButton->click();
    auto *status = conversation.findChild<QLabel *>("teamConversationStatus");
    QTRY_VERIFY(status->isVisible());
    QVERIFY2(status->text().startsWith("Terminal has a draft, dialog, or unrecognized prompt") && !status->text().contains("open Terminal"), qPrintable(status->text()));
    auto *open = conversation.findChild<QPushButton *>("teamConversationOpenTerminal");
    QVERIFY(open->isVisible());
    QTRY_VERIFY(inComposer(open) && inComposer(status));
    QCOMPARE(input->toPlainText(), QStringLiteral("One more thing"));
    QVERIFY(rows().isEmpty());
    shot(&conversation, "u35-coordinator-refused" + suffix + "-dark");
    open->click();
    QCOMPARE(opened, QStringLiteral("|claude/checkout/lead"));
}

void TestTeamsUiWaiting::heldSentTakesEachLostMessageOnce()
{
    // #511 N5: only this composer's messages come back, each once, with why.
    HeldSent sent;
    sent.add("a", "Also re-run the migrations");
    sent.add("b", "Then the docs");
    const QJsonArray settled{
        QJsonObject{{"request_id", "other"}, {"text", "someone else's"}, {"status", "failed"}, {"error", "x"}},
        QJsonObject{{"request_id", "a"}, {"text", "Also re-run the migrations"}, {"status", "failed"}, {"error", "session identity changed; nothing was sent"}},
        QJsonObject{{"request_id", "b"}, {"text", "Then the docs"}, {"status", "uncertain"}, {"error", "delivery uncertain"}}};
    const auto lost = sent.take(settled);
    QCOMPARE(lost.size(), 2);
    QCOMPARE(lost[0].text, QString("Also re-run the migrations"));
    QVERIFY(!lost[0].uncertain && lost[1].uncertain);
    QCOMPARE(HeldSent::notice(lost[0]), QString("Not delivered: session identity changed; nothing was sent. The text is back in the composer."));
    QVERIFY(HeldSent::notice(lost[1]).startsWith(QString::fromUtf8("Delivery unconfirmed — check the conversation before sending again.")));
    QVERIFY(sent.take(settled).isEmpty());
    QCOMPARE(sent.size(), 0);
    // A draft stays and the lost text comes below it; it never comes twice.
    QCOMPARE(HeldSent::restored({}, "lost"), QString("lost"));
    QCOMPARE(HeldSent::restored("draft", "lost"), QString("draft\n\nlost"));
    QCOMPARE(HeldSent::restored("draft\n\nlost", "lost"), QString("draft\n\nlost"));
    for (int i = 0; i < 40; ++i) sent.add(QString::number(i), "x");
    QCOMPARE(sent.size(), 32);
}

void TestTeamsUiWaiting::coordinatorRestoresAHeldMessageThatWasNotDelivered()
{
    // #511 N5: hgs held the message, then could not deliver it: its text comes
    // back to the composer and the status says why; an uncertain one says
    // Delivery unconfirmed.
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
held=json.load(open(here+'.held')) if os.path.exists(here+'.held') else []
settled=json.load(open(here+'.settled')) if os.path.exists(here+'.settled') else []
if a[:1]==['inspect']:
 print(json.dumps({'name':a[1],'run_id':'run-1','conversation_id':'conv-1','runtime_state':'live','process_state':'running',
  'activity':'busy','phase':'working','cursor':0,'events':[],'input_hold_supported':True,'held_inputs':held,'held_settled':settled}))
elif a[:1]==['send']:
 body=json.load(sys.stdin)
 open(here+'.sends','a').write(json.dumps(body)+'\n')
 held.append({'request_id':body['request_id'],'text':body['text'],'queued_at':1.0});json.dump(held,open(here+'.held','w'))
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'run_id':'run-1','conversation_id':'conv-1','first_message':False,
  'status':'queued','text':body['text']}))
else:print('{}')
)PY"));
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamConversation conversation(&client);
    conversation.setAgentName("the lead");
    conversation.setSession({}, "claude/checkout/lead");
    conversation.resize(560, 420); conversation.show();
    auto *input = conversation.findChild<QPlainTextEdit *>("teamConversationInput");
    auto *sendButton = conversation.findChild<QPushButton *>("teamConversationSend");
    auto *status = conversation.findChild<QLabel *>("teamConversationStatus");
    const auto sends = [&] {
        QList<QJsonObject> found; QFile file(program + ".sends");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) found << QJsonDocument::fromJson(line).object();
        return found;
    };
    const auto settle = [&](const QString &id, const QString &text, const QString &state, const QString &error) {
        QJsonArray list; QFile file(program + ".settled");
        if (file.open(QIODevice::ReadOnly)) { list = QJsonDocument::fromJson(file.readAll()).array(); file.close(); }
        list.append(QJsonObject{{"request_id", id}, {"text", text}, {"status", state}, {"error", error}, {"queued_at", 1.0}, {"settled_at", 2.0}});
        QVERIFY(FakeCli::replace(program + ".held", "[]"));
        QVERIFY(FakeCli::replace(program + ".settled", QJsonDocument(list).toJson()));
    };
    QTRY_VERIFY(sendButton->isEnabled());
    QTest::qWait(300);
    // Another composer's lost message is not this one's.
    settle("someone-else", "Not mine", "failed", "session identity changed");
    input->setPlainText("Also re-run the migrations"); sendButton->click();
    QTRY_COMPARE(sends().size(), 1);
    QTRY_VERIFY(input->toPlainText().isEmpty());
    QTest::qWait(400);
    QVERIFY(input->toPlainText().isEmpty());
    // The session restarted under it: the text comes back, with why.
    settle(sends().first()["request_id"].toString(), "Also re-run the migrations", "failed", "session identity changed; nothing was sent");
    QTRY_COMPARE(input->toPlainText(), QStringLiteral("Also re-run the migrations"));
    QTRY_VERIFY(status->isVisible());
    QCOMPARE(status->text(), QStringLiteral("Not delivered: session identity changed; nothing was sent. The text is back in the composer."));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&conversation, "n5-held-not-delivered-dark");
        lightPalette(); conversation.setTheme(false); lightShot(&conversation, "n5-held-not-delivered-light"); darkPalette(); conversation.setTheme(true);
    }
    // It comes back once: the next reads keep the composer as the owner left it.
    input->setPlainText("A new draft");
    QTest::qWait(400);
    QCOMPARE(input->toPlainText(), QStringLiteral("A new draft"));
    // Sent again and left uncertain: the text comes back below the draft and
    // the status says Delivery unconfirmed; nothing is sent again.
    sendButton->click();
    QTRY_COMPARE(sends().size(), 2);
    QTRY_VERIFY(input->toPlainText().isEmpty());
    input->setPlainText("Draft meanwhile");
    settle(sends().last()["request_id"].toString(), "A new draft", "uncertain", "delivery uncertain: the terminal did not confirm");
    QTRY_COMPARE(input->toPlainText(), QStringLiteral("Draft meanwhile\n\nA new draft"));
    QTRY_VERIFY(status->text().startsWith(QString::fromUtf8("Delivery unconfirmed — check the conversation before sending again.")));
    QTest::qWait(400);
    QCOMPARE(sends().size(), 2);
}

// Owner, 2026-10-28: only Activity took files; in the lead's conversation a
// pasted image became "[Image #1]" text and the file was lost. Every
// conversation has Attach, and a pasted image is an attachment sent with it.
void TestTeamsUiWaiting::coordinatorTakesPastedImagesAndFiles()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
if a[:1]==['inspect']:
 print(json.dumps({'name':a[1],'run_id':'run-1','conversation_id':'conv-1','runtime_state':'live','process_state':'running',
  'activity':'idle','phase':'idle','cursor':0,'events':[]}))
elif a[:1]==['send']:
 body=json.load(sys.stdin)
 open(here+'.sends','a').write(json.dumps(body)+'\n')
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'run_id':'run-1','conversation_id':'conv-1','first_message':False,
  'status':'submitted','text':body['text']}))
else:print('{}')
)PY"));
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamConversation conversation(&client);
    QSignalSpy inspections(&client, &HgsClient::inspectionReady);
    conversation.setSession({}, "claude/checkout/lead");
    conversation.resize(560, 420); conversation.show();
    auto *attach = conversation.findChild<QPushButton *>("teamConversationAttach");
    QVERIFY(attach); QTRY_VERIFY(attach->isVisible()); QVERIFY(attach->isEnabled());
    auto *input = conversation.findChild<QPlainTextEdit *>("teamConversationInput");
    auto *sendButton = conversation.findChild<QPushButton *>("teamConversationSend");
    QTRY_VERIFY(sendButton->isEnabled());
    // Sending needs the exact inspected identity, not merely an enabled button.
    WAIT_VERIFY(std::any_of(inspections.cbegin(), inspections.cend(), [](const QList<QVariant> &reply) {
        const auto detail = reply.at(2).toJsonObject();
        return reply.at(0).toString().isEmpty() && reply.at(1).toString() == "claude/checkout/lead"
            && detail["run_id"] == "run-1" && detail["conversation_id"] == "conv-1";
    }));
    QImage image(8, 8, QImage::Format_ARGB32); image.fill(Qt::red);
    QGuiApplication::clipboard()->setImage(image);
    input->setFocus(); input->insertPlainText("Look at "); input->paste();
    QCOMPARE(input->toPlainText(), QString("Look at [Image #1] "));
    auto *files = conversation.findChild<QWidget *>("teamConversationFiles"); QVERIFY(files); QVERIFY(files->isVisible());
    sendButton->click();
    const auto sends = [&] {
        QList<QJsonObject> found; QFile file(program + ".sends");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) found << QJsonDocument::fromJson(line).object();
        return found;
    };
    QTRY_COMPARE(sends().size(), 1);
    const auto attachments = sends().first()["attachments"].toArray();
    QCOMPARE(attachments.size(), 1);
    QCOMPARE(attachments[0].toObject()["mime"].toString(), QString("image/png"));
    QVERIFY(sends().first()["text"].toString().contains("[Image #1]"));
    // Sent: the field and the files clear.
    QTRY_VERIFY(input->toPlainText().isEmpty()); QTRY_VERIFY(!files->isVisible());
}

// Owner, 2026-10-28: with text in the agent's Terminal input no window could
// send. The refusal shows that draft with Send it, which presses Enter in
// Terminal only for the draft shown; the owner's own message stays.
void TestTeamsUiWaiting::terminalDraftIsShownAndSentFromTheConversation()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
draft=os.path.exists(here+'.draft')
if a[:1]==['inspect']:
 print(json.dumps({'name':a[1],'run_id':'run-1','conversation_id':'conv-1','runtime_state':'live','process_state':'running',
  'activity':'idle','phase':'idle','cursor':0,'events':[]}))
elif a[:1]==['send']:
 body=json.load(sys.stdin)
 if draft:
  sys.stderr.write('hgs: terminal has a draft, dialog, or unrecognized prompt; open Terminal and return to an empty agent prompt\n');sys.exit(1)
 print(json.dumps({'request_id':body['request_id'],'name':a[1],'run_id':'run-1','conversation_id':'conv-1','status':'submitted','text':body['text']}))
elif a[:1]==['terminal']:
 body=json.load(sys.stdin)
 open(here+'.terminal','a').write(json.dumps(body)+'\n')
 base={'request_id':body['request_id'],'name':a[1],'run_id':'run-1','conversation_id':'conv-1'}
 if body['action']=='draft':
  print(json.dumps(dict(base,status='draft',draft_supported=True,draft='fix the build' if draft else None,draft_hash='a'*64)))
 else:
  os.remove(here+'.draft');print(json.dumps(dict(base,status='submitted')))
else:print('{}')
)PY"));
    QVERIFY(QFile(program + ".draft").open(QIODevice::WriteOnly));
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamConversation conversation(&client);
    QString opened;
    conversation.openTerminal = [&](const QString &host, const QString &name) { opened = host + '|' + name; };
    conversation.setSession({}, "claude/checkout/lead");
    conversation.resize(560, 420); conversation.show();
    auto *input = conversation.findChild<QPlainTextEdit *>("teamConversationInput");
    auto *sendButton = conversation.findChild<QPushButton *>("teamConversationSend");
    auto *bar = conversation.findChild<QFrame *>("teamConversationTerminalDraft");
    QVERIFY(bar); QVERIFY(!bar->isVisible());
    QTRY_VERIFY(sendButton->isEnabled());
    QTest::qWait(300);   // the first read of the session
    input->setPlainText("Also re-run the migrations"); sendButton->click();
    QTRY_VERIFY(bar->isVisible());
    QVERIFY(bar->findChild<QLabel *>("terminalDraftText")->text().contains("fix the build"));
    QCOMPARE(input->toPlainText(), QString("Also re-run the migrations"));
    bar->findChild<QPushButton *>("terminalDraftOpen")->click();
    QCOMPARE(opened, QString("|claude/checkout/lead"));
    bar->findChild<QPushButton *>("terminalDraftSend")->click();
    QTRY_VERIFY(!bar->isVisible());
    QList<QJsonObject> calls; QFile file(program + ".terminal"); QVERIFY(file.open(QIODevice::ReadOnly));
    for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) calls << QJsonDocument::fromJson(line).object();
    QCOMPARE(calls.size(), 2);
    QCOMPARE(calls[0]["action"].toString(), QString("draft"));
    QCOMPARE(calls[1]["action"].toString(), QString("submit_draft"));
    QCOMPARE(calls[1]["draft_hash"].toString(), QString(64, 'a'));
    QCOMPARE(calls[1]["expected_run_id"].toString(), QString("run-1"));
    // The owner's message is still there and now goes.
    QCOMPARE(input->toPlainText(), QString("Also re-run the migrations"));
    sendButton->click();
    QTRY_VERIFY(input->toPlainText().isEmpty());
}

void TestTeamsUiWaiting::waitingPageListsEveryKindAcrossTeams()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    QVERIFY(writeWaiting(script() + ".waiting", waitingKinds(now)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QList<TeamWaitingPage::Item> opened;
    page.openTeam = [&](const TeamWaitingPage::Item &item) { opened << item; };
    int counted = -1;
    page.countChanged = [&](int total) { counted = total; };
    page.resize(980, 640); page.show();
    WAIT_VERIFY(page.findChildren<QFrame *>("teamWaitingRow").size() == 7);
    QCOMPARE(counted, 7);
    QCOMPARE(waitingRowIds(&page), (QStringList{"team-plan/plan_draft/plan", "team-1/phase_draft/2", "team-3/permission_escalated/Tester",
                                               "team-2/owner_word_asked/proposal-p3", "team-2/report_escalated/4", "team-4/task_waits_owner/7",
                                               "team-5/team_finished/finish"}));
    QStringList kinds; QSet<QString> teams;
    for (auto *row : page.findChildren<QFrame *>("teamWaitingRow")) {
        kinds << row->findChild<QLabel *>("teamWaitingKind")->text();
        teams.insert(row->findChild<QLabel *>("teamWaitingTeam")->text());
        QVERIFY(!row->findChild<QLabel *>("teamWaitingKindIcon")->pixmap().isNull());
        QVERIFY(!row->findChild<QLabel *>("teamWaitingItemTitle")->text().isEmpty());
    }
    QCOMPARE(kinds, (QStringList{"Plan", "Phase draft", "Permission", "Proposal", "Report", "Held task", "All decided"}));
    // #724: every row opens its team; a one-click decision is answered in it
    // too (Approve… for the drafts, Answer for the proposal and the permission;
    // task 116: and Answer for a report, a held task and the end of the team).
    QStringList answers;
    for (auto *row : page.findChildren<QFrame *>("teamWaitingRow")) {
        QCOMPARE(row->findChild<QPushButton *>("teamWaitingOpen")->text(), QStringLiteral("Open team"));
        const auto *approve = row->findChild<QPushButton *>("teamWaitingApprove");
        const auto *answer = row->findChild<QPushButton *>("teamWaitingAnswerToggle");
        answers << (approve ? approve->text() : answer ? answer->text() : QString());
    }
    QCOMPARE(answers, (QStringList{"Approve…", "Approve…", "Answer", "Answer", "Answer", "Answer", "Answer"}));
    QCOMPARE(teams.size(), 6);
    const QString count = page.findChild<QLabel *>("teamWaitingCount")->text();
    QVERIFY2(count.contains("7 items") && count.contains("6 teams"), qPrintable(count));
    QVERIFY(!page.findChild<QPushButton *>("teamWaitingOlder"));
    // Read only: the page sends its reads and nothing else.
    const auto reads = calls(script(), "waiting");
    QVERIFY(!reads.isEmpty());
    for (const auto &input : reads) QVERIFY2(!input.contains("request_id"), qPrintable(QJsonDocument(input).toJson()));
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    shot(&page, "u50-waiting-dark");
    page.setDark(false);
    lightShot(&page, "u50-waiting-light");
    // Open team on the phase draft names its team and phase; a click on the row
    // itself opens the team as well.
    QFrame *phaseRow = nullptr;
    for (auto *row : page.findChildren<QFrame *>("teamWaitingRow")) if (row->property("kind") == "phase_draft") phaseRow = row;
    QVERIFY(phaseRow);
    phaseRow->findChild<QPushButton *>("teamWaitingOpen")->click();
    QCOMPARE(opened.size(), 1);
    QCOMPARE(opened[0].team, QString("team-1"));
    QCOMPARE(opened[0].kind, QString("phase_draft"));
    QCOMPARE(opened[0].phase, 2);
    QFrame *finished = page.findChildren<QFrame *>("teamWaitingRow").last();
    QTest::mouseClick(finished, Qt::LeftButton, {}, QPoint(40, finished->height() / 2));
    QCOMPARE(opened.size(), 2);
    QCOMPARE(opened[1].team, QString("team-5"));
}

// U50: the rail page lists every kind of item that waits for the owner, from
// several teams, newest first, each with its kind's icon and label and its
// team; a row (or its Open team) opens that team. Shots in both themes.
// U55: the banner: one waiting item is one line with its icon, title, team,
// age and action; several fold into one line with a count that opens; it
// hides when nothing waits; the feed reads only while active and notifies
// once per item. Shots in both themes.
void TestTeamsUiWaiting::waitingBannerFoldsOpensAndHides()
{
    Settings::set("workspace/skin", "whitewh");
    Settings::remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); Settings::remove(TeamWaitingNotices::kSetting); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const auto items = waitingKinds(now);
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{items[1]}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingFeed feed(&controller);
    QStringList notified;
    QObject::connect(&feed, &TeamWaitingFeed::notify, &feed, [&](const QString &token, const QString &, const QString &body) { notified << token + "|" + body; });
    QWidget host; host.setObjectName("bannerHost");
    auto *v = new QVBoxLayout(&host); v->setContentsMargins(0, 0, 0, 0);
    auto *banner = new TeamWaitingBanner(&feed); banner->setDark(true);
    QList<TeamWaitingPage::Item> opened; int all = 0;
    banner->openItem = [&](const TeamWaitingPage::Item &item) { opened << item; };
    banner->openAll = [&] { ++all; };
    v->addWidget(banner); v->addStretch();
    host.resize(760, 260); host.show();
    // Nothing read yet: hidden, and the feed reads nothing while inactive.
    QVERIFY(banner->isHidden());
    QTest::qWait(50);
    QVERIFY(calls(script(), "waiting").isEmpty());
    feed.setActive(true);
    // One item: its line with the kind's icon, title, team, age and action.
    QTRY_VERIFY(!banner->isHidden() && banner->findChild<QWidget *>("teamWaitingBannerLine"));
    auto *line = banner->findChild<QWidget *>("teamWaitingBannerLine");
    QCOMPARE(line->property("itemId").toString(), QStringLiteral("team-1/phase_draft/2"));
    QVERIFY(!line->findChild<QLabel *>("teamWaitingBannerIcon")->pixmap().isNull());
    QCOMPARE(line->findChild<QLabel *>("teamWaitingBannerTitle")->text(), items[1].toObject()["title"].toString());
    QCOMPARE(line->findChild<QLabel *>("teamWaitingBannerTeam")->text(), QStringLiteral("Checkout"));
    QCOMPARE(line->findChild<QLabel *>("teamWaitingBannerAge")->text(), QStringLiteral("5 min ago"));
    auto *action = line->findChild<QPushButton *>("teamWaitingBannerAction");
    QCOMPARE(action->text(), QStringLiteral("Approve…"));
    QVERIFY(!banner->findChild<QLabel *>("teamWaitingBannerCount"));
    // Its open button opens the team at the item; nothing is approved from
    // there (#724: Approve… asks in place, waitingBannerAnswersInPlace).
    line->findChild<QToolButton *>("teamWaitingBannerOpen")->click();
    QCOMPARE(opened.size(), 1);
    QCOMPARE(opened[0].phase, 2);
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    // One notification for it, with the team the click opens.
    QCOMPARE(notified, QStringList{"zerus-team-attention:team-1:waiting:team-1/phase_draft/2|" + items[1].toObject()["title"].toString()});
    shot(banner, "u55-banner-one-dark");
    // Every kind: one line with the count and the newest, folded.
    QVERIFY(writeWaiting(script() + ".waiting", items));
    feed.refresh();
    QTRY_VERIFY(banner->findChild<QLabel *>("teamWaitingBannerCount"));
    QCOMPARE(banner->findChild<QLabel *>("teamWaitingBannerCount")->text(), QStringLiteral("7 wait for you"));
    QCOMPARE(banner->property("count").toInt(), 7);
    QCOMPARE(banner->findChild<QLabel *>("teamWaitingBannerNewest")->text(), items[0].toObject()["title"].toString());
    QVERIFY(banner->findChildren<QWidget *>("teamWaitingBannerRow").isEmpty());
    // The six new ones: two notifications and one line for the other four;
    // the phase draft is not announced again.
    QCOMPARE(notified.size(), 4);
    QVERIFY(notified[3].endsWith("|4 more items wait for you"));
    QVERIFY(std::none_of(notified.cbegin() + 1, notified.cend(), [](const QString &n) { return n.contains("phase_draft/2"); }));
    shot(banner, "u55-banner-folded-dark");
    // Show all: five rows, newest first, each with its action, and the list.
    banner->findChild<QPushButton *>("teamWaitingBannerToggle")->click();
    QTRY_COMPARE(banner->findChildren<QWidget *>("teamWaitingBannerRow").size(), TeamWaitingBanner::MaxRows);
    QStringList ids, actions;
    for (auto *row : banner->findChildren<QWidget *>("teamWaitingBannerRow")) {
        ids << row->property("itemId").toString();
        actions << row->findChild<QPushButton *>("teamWaitingBannerAction")->text();
    }
    QCOMPARE(ids, (QStringList{"team-plan/plan_draft/plan", "team-1/phase_draft/2", "team-3/permission_escalated/Tester",
                               "team-2/owner_word_asked/proposal-p3", "team-2/report_escalated/4"}));
    QCOMPARE(actions, (QStringList{"Approve…", "Approve…", "Answer", "Answer", "Answer"}));
    // No dots as separators: the rows keep their parts apart with space.
    for (auto *label : banner->findChildren<QLabel *>()) QVERIFY2(label->text() != QStringLiteral("·"), "a dot separator");
    auto *more = banner->findChild<QPushButton *>("teamWaitingBannerAll");
    QVERIFY(more);
    QCOMPARE(more->text(), QStringLiteral("Open the waiting list (7)"));
    more->click();
    QCOMPARE(all, 1);
    shot(banner, "u55-banner-open-dark");
    banner->setDark(false);
    lightShot(banner, "u55-banner-open-light");
    banner->findChild<QPushButton *>("teamWaitingBannerToggle")->click();
    QTRY_VERIFY(banner->findChildren<QWidget *>("teamWaitingBannerRow").isEmpty());
    lightShot(banner, "u55-banner-folded-light");
    darkPalette(); banner->setDark(true);
    // Read again: no notification repeats.
    feed.refresh();
    QTest::qWait(300);
    QCOMPARE(notified.size(), 4);
    // Nothing waits: the banner hides.
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{}));
    feed.refresh();
    QTRY_VERIFY(banner->isHidden());
    // Inactive: no more reads.
    feed.setActive(false);
    const auto reads = calls(script(), "waiting").size();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "waiting").size(), reads);
    const auto inputs = calls(script(), "waiting");
    QVERIFY(std::all_of(inputs.cbegin(), inputs.cend(), [](const QJsonObject &input) { return input["limit"].toInt() == TeamWaitingFeed::MaxItems; }));
}

// U50: 300 items arrive in pages of 50 with Show older; a poll while new items
// arrive puts them on top, keeps every shown item in its order, each once, and
// Show older goes on after them.
void TestTeamsUiWaiting::waitingPagePagesStayStableWhileItemsArrive()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    QJsonArray items;
    // Pairs share a time; their ids order them, as in hgs.
    for (int n = 0; n < 300; ++n)
        items << waitingItem(QStringLiteral("team-%1%2").arg(n / 2 % 9).arg(n % 2), QStringLiteral("Team %1").arg(n / 2 % 9), "task_waits_owner", QString::number(n),
                             QStringLiteral("#%1 is held by you").arg(n), now - 1000 - (n / 2) * 10, "release_task", {{"task", n}});
    QVERIFY(writeWaiting(script() + ".waiting", items));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    page.resize(900, 700); page.show();
    WAIT_VERIFY(page.findChildren<QFrame *>("teamWaitingRow").size() == TeamWaitingPage::PageSize);
    QVERIFY(page.findChild<QLabel *>("teamWaitingCount")->text().contains("300 items"));
    page.findChild<QPushButton *>("teamWaitingOlder")->click();
    WAIT_VERIFY(page.findChildren<QFrame *>("teamWaitingRow").size() == 100);
    const QStringList shown = waitingRowIds(&page);
    QCOMPARE(QSet<QString>(shown.begin(), shown.end()).size(), 100);
    // Two items arrive, newer than everything; the poll reads the newest page.
    items.prepend(waitingItem("team-new", "Hotfix", "report_escalated", "1", "Report on #1 Patch waits for your decision", now - 5, "decide_report"));
    items.prepend(waitingItem("team-new", "Hotfix", "permission_escalated", "Dev", "Dev waits for your answer", now - 2, "answer_permission"));
    QVERIFY(writeWaiting(script() + ".waiting", items));
    page.refresh();
    WAIT_VERIFY(waitingRowIds(&page).value(0) == "team-new/permission_escalated/Dev");
    // The poll read every shown item and a page more.
    const auto polled = calls(script(), "waiting");
    QVERIFY(!polled.isEmpty());
    QCOMPARE(polled.last().value("limit").toInt(), 150);
    const QStringList after = waitingRowIds(&page);
    QCOMPARE(after.size(), 150);
    QCOMPARE(after.mid(0, 2), (QStringList{"team-new/permission_escalated/Dev", "team-new/report_escalated/1"}));
    QCOMPARE(QSet<QString>(after.begin(), after.end()).size(), after.size());
    // Every item shown before is still shown, in the same order.
    QStringList kept; for (const auto &id : after) if (shown.contains(id)) kept << id;
    QCOMPARE(kept, shown);
    // Show older goes on right after them: the list is the server's order.
    page.findChild<QPushButton *>("teamWaitingOlder")->click();
    WAIT_VERIFY(page.findChildren<QFrame *>("teamWaitingRow").size() == 200);
    const QStringList all = waitingRowIds(&page);
    QStringList expected;
    for (const auto &value : items) expected << value.toObject()["id"].toString();
    QCOMPARE(all, expected.mid(0, 200));
    QVERIFY(page.findChild<QPushButton *>("teamWaitingOlder"));
    for (const auto &input : calls(script(), "waiting")) QVERIFY(input.value("limit").toInt() <= 200);
}

// U50: an empty list says so, in both themes; a refused read is shown with its
// reason and the next read clears it.
void TestTeamsUiWaiting::waitingPageEmptyAndRefused()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    QVERIFY(writeWaiting(script() + ".waiting", {}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    int counted = -1;
    page.countChanged = [&](int total) { counted = total; };
    page.resize(980, 520); page.show();
    WAIT_VERIFY(page.findChild<QWidget *>("teamWaitingEmpty"));
    QCOMPARE(counted, 0);
    QVERIFY(page.findChildren<QFrame *>("teamWaitingRow").isEmpty());
    QCOMPARE(page.findChild<QLabel *>("teamWaitingCount")->text(), QString("Nothing waits for you"));
    shot(&page, "u50-waiting-empty-dark");
    page.setDark(false);
    lightShot(&page, "u50-waiting-empty-light");
    QVERIFY(FakeCli::replace(script() + ".waitingrefuse", "hgs: The team store is busy"));
    page.refresh();
    WAIT_VERIFY(page.findChild<QLabel *>("teamWaitingError"));
    QVERIFY(page.findChild<QLabel *>("teamWaitingError")->text().contains("The team store is busy"));
    QFile::remove(script() + ".waitingrefuse");
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{waitingKinds(double(QDateTime::currentSecsSinceEpoch())).first()}));
    page.refresh();
    WAIT_VERIFY(page.findChildren<QFrame *>("teamWaitingRow").size() == 1);
    QVERIFY(!page.findChild<QLabel *>("teamWaitingError"));
    QCOMPARE(counted, 1);
}

// #724 (owner, 2026-10-30): Waiting for you answers a one-click decision in
// its row, with the team view's command and confirmation, bound to the item:
// the lead's proposal (Deny with a reason, Allow), an addition (the typed
// "add" and Add), an escalated permission (Allow, Deny) and a phase
// draft (Approve… with the team's question). A report and a held task keep
// Open team only. A stale item says so and sends nothing; an answered one
// leaves at once.
void TestTeamsUiWaiting::waitingItemsAreAnsweredInPlace()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto team = answerTeam();
    setTeam(team);
    QVERIFY(writeWaiting(script() + ".waiting", answerItems(now)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);   // the confirmation helpers' context
    TeamWaitingPage page(&controller);
    QStringList answered, opened;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    page.openTeam = [&](const TeamWaitingPage::Item &item) { opened << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 1000); page.show();
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    const QString proposal = "team-1/owner_word_asked/proposal-p1", stale = "team-1/owner_word_asked/proposal-p2",
                  addition = "team-1/owner_word_asked/addition-add-1", permission = "team-1/permission_escalated/backend",
                  draft = "team-1/phase_draft/2", report = "team-1/report_escalated/1", held = "team-1/task_waits_owner/2";
    QTRY_VERIFY(row(proposal) && row(held));
    // The store drops what was answered: so does the fake's list, for the polls.
    QStringList gone;
    const auto storeDrops = [&](const QString &id) {
        gone << id;
        QJsonArray left;
        for (const auto &value : answerItems(now)) if (!gone.contains(value.toObject()["id"].toString())) left.append(value);
        QVERIFY(writeWaiting(script() + ".waiting", left));
    };
    // Task 116: a report and a held task are answered here too, never only in
    // the Team tab: each has its card.
    for (const auto &id : {report, held}) {
        QVERIFY2(row(id)->findChild<QPushButton *>("teamWaitingAnswerToggle"), qPrintable(id));
    }
    const auto toggle = [&](const QString &id) { row(id)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click(); };
    // Open the three cards: the proposal, the addition and the permission.
    toggle(proposal); toggle(addition); toggle(permission);
    QTRY_VERIFY(row(proposal)->findChild<QFrame *>("teamProposalCard") && row(addition)->findChild<QLineEdit *>("teamWaitingAdditionConfirm")
                && row(permission)->findChild<QLabel *>("teamWaitingPermissionCommand"));
    // A click inside an open answer stays there.
    QTest::mouseClick(row(proposal), Qt::LeftButton, {}, QPoint(row(proposal)->width() / 2, row(proposal)->height() - 4));
    QVERIFY(opened.isEmpty());
    QCOMPARE(row(addition)->findChild<QLabel *>("teamWaitingAdditionTask")->text(), QStringLiteral("Card form tests"));
    QCOMPARE(row(permission)->findChild<QLabel *>("teamWaitingPermissionCommand")->text(), QStringLiteral("cargo test --locked"));
    // No dots as separators in what the answers say.
    for (auto *label : page.findChildren<QLabel *>()) QVERIFY2(!label->text().contains(QStringLiteral(" · ")), qPrintable(label->text()));
    TestWait::settle();
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&page, "u724-page-answers-dark");
        page.setDark(false);
        lightShot(&page, "u724-page-answers-light");
        darkPalette(); page.setDark(true);
    }
    // The proposal: Deny with its reason, bound to its hash.
    row(proposal)->findChild<QLineEdit *>("teamProposalDenyReason")->setText("Not this week");
    row(proposal)->findChild<QPushButton *>("teamProposalDeny")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    auto sent = calls(script(), "decide-proposal").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["proposal"].toString(), QStringLiteral("p1"));
    QCOMPARE(sent["accept"].toBool(), false);
    QCOMPARE(sent["expected_hash"].toString(), QStringLiteral("h1"));
    QCOMPARE(sent["reason"].toString(), QStringLiteral("Not this week"));
    QCOMPARE(sent["by"].toString(), QStringLiteral("human"));
    // It leaves at once.
    QTRY_VERIFY(!row(proposal));
    storeDrops(proposal);
    QCOMPARE(answered, QStringList{proposal});
    // The permission: Allow with the question's id, hash and answer ids.
    row(permission)->findChild<QPushButton *>("teamWaitingPermissionAllow")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 1);
    sent = calls(script(), "answer").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["member"].toString(), QStringLiteral("backend"));
    QCOMPARE(sent["question_id"].toString(), QStringLiteral("claude-approval:6f2b0e3a"));
    QCOMPARE(sent["expected_question_hash"].toString(), QStringLiteral("6f2b0e3a"));
    const auto question = Teams::parse(team).member("backend")->permission;
    const auto native = sent["answers"].toArray()[0].toObject();
    QCOMPARE(native["question_id"].toString(), question.native.questionId);
    QCOMPARE(native["selected_option_ids"].toArray(), QJsonArray{question.native.allow});
    QTRY_VERIFY(!row(permission));
    storeDrops(permission);
    // The addition: Add wants the typed word (Cancel declines it: below).
    auto *typed = row(addition)->findChild<QLineEdit *>("teamWaitingAdditionConfirm");
    auto *add = row(addition)->findChild<QPushButton *>("teamWaitingAdditionAdd");
    QVERIFY(!add->isEnabled());
    typed->setText("ad");
    QVERIFY(!add->isEnabled());
    typed->setText("add");
    QVERIFY(add->isEnabled());
    add->click();
    QTRY_COMPARE(calls(script(), "add-to-phase").size(), 1);
    sent = calls(script(), "add-to-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["phase"].toInt(), 1);
    QCOMPARE(sent["confirm"].toString(), QStringLiteral("add-1"));
    QCOMPARE(sent["confirmation"].toString(), QStringLiteral("add"));
    QTRY_VERIFY(!row(addition));
    storeDrops(addition);
    // The phase draft: Approve… asks the team view's question; Cancel sends nothing.
    const auto approval = Teams::phaseApproval(Teams::parse(team), 2);
    QString name, text;
    armConfirmRecord(&panel, true, &name, &text);
    row(draft)->findChild<QPushButton *>("teamWaitingApprove")->click();
    QTRY_VERIFY(!name.isEmpty());
    QCOMPARE(name, approval.name);
    QCOMPARE(text, approval.text);
    QTest::qWait(100);
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    QVERIFY(row(draft));
    armConfirm(&panel, false);
    row(draft)->findChild<QPushButton *>("teamWaitingApprove")->click();
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    sent = calls(script(), "approve-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["phase"].toInt(), 2);
    QCOMPARE(sent["alongside"].toBool(), approval.input.value("alongside").toBool());
    QTRY_VERIFY(!row(draft));
    storeDrops(draft);
    // A stale item: p2 was decided after its card opened. Allow says so and
    // sends nothing; the item stays until the list is read again.
    toggle(stale);
    QTRY_VERIFY(row(stale)->findChild<QPushButton *>("teamProposalAllow"));
    auto decided = team;
    auto proposals = decided["proposals"].toArray();
    auto second = proposals[1].toObject(); second["state"] = "accepted"; proposals[1] = second;
    decided["proposals"] = proposals;
    setTeam(decided);
    row(stale)->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_VERIFY(([&] {
        auto *notice = row(stale) ? row(stale)->findChild<QLabel *>("teamWaitingAnswerNotice") : nullptr;
        return notice && notice->isVisible() && notice->text().contains(QStringLiteral("nothing was sent"));
    })());
    QCOMPARE(calls(script(), "decide-proposal").size(), 1);
    QCOMPARE(answered, (QStringList{proposal, permission, addition, draft}));
    shot(&page, "u724-page-stale-dark");
    // An answer between rebuilds never shows as a window of its own.
    for (auto *window : QApplication::topLevelWidgets())
        QVERIFY2(!(window->objectName() == QLatin1String("teamWaitingAnswer") && window->isVisible()), "a stray answer window");
}

// Task 116 (owner, 2026-11-06): a report, a held task, a retry and the end of
// the team are answered where they wait — the same operations as their task
// cards, not only Open team in the Team tab.
void TestTeamsUiWaiting::waitingReportHeldRetryAndFinishAnswerInPlace()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto team = answerTeam();
    {
        auto tasks = team["tasks"].toArray();
        auto first = tasks[0].toObject(); first["status"] = "submitted"; tasks[0] = first;
        auto second = tasks[1].toObject(); second["hold"] = QJsonObject{{"by", "You"}, {"reason", "After the release"}, {"at", 1}}; tasks[1] = second;
        // The team has two tasks: #5 is added whole (indexing past the end of the
        // array was undefined and crashed).
        tasks.append(QJsonObject{{"id", 5}, {"phase", 1}, {"title", "Code review"}, {"assignee", "backend"}, {"status", "pending"},
                                 {"waits", QJsonObject{{"for", "model"}, {"on", QJsonArray{"model"}}, {"unblock", "choose a model"}}}});
        team["tasks"] = tasks;
    }
    setTeam(team);
    auto report = waitingItem("report-1", "report_escalated", "decide_report", "Report on #1 Saved cards API from Backend waits for your decision", now - 300);
    report["task"] = 1; report["member"] = "Backend";
    auto held = waitingItem("held-2", "task_waits_owner", "release_task", "#2 Release notes is held by you: after the release", now - 360);
    held["task"] = 2;
    auto retry = waitingItem("retry-5", "task_waits_owner", "retry_task", "#5 Code review waits: hgs: unknown model", now - 400);
    retry["task"] = 5;
    auto finish = waitingItem("finish", "team_finished", "finish", "All 4 tasks decided: waits for you to finish", now - 900);
    QJsonArray waiting{report, held, retry, finish};
    QVERIFY(writeWaiting(script() + ".waiting", waiting));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 1000); page.show();
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    const QString reportId = report["id"].toString(), heldId = held["id"].toString(),
                  retryId = retry["id"].toString(), finishId = finish["id"].toString();
    // The store drops what was answered: so does the fake's list, for the polls.
    const auto leave = [&](const QString &id) {
        QJsonArray left;
        for (const auto &value : waiting) if (value.toObject()["id"].toString() != id) left.append(value);
        waiting = left;
        QVERIFY(writeWaiting(script() + ".waiting", waiting));
    };
    QTRY_VERIFY(row(reportId) && row(heldId) && row(retryId) && row(finishId));
    // The report: Reject with the member's reason.
    row(reportId)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(reportId)->findChild<QLineEdit *>("teamWaitingReportReason"));
    row(reportId)->findChild<QLineEdit *>("teamWaitingReportReason")->setText("Fix the flaky test first");
    row(reportId)->findChild<QPushButton *>("teamWaitingReportReject")->click();
    QTRY_COMPARE(calls(script(), "reject").size(), 1);
    auto sent = calls(script(), "reject").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["task"].toInt(), 1);
    QCOMPARE(sent["reason"].toString(), QStringLiteral("Fix the flaky test first"));
    QTRY_VERIFY(!row(reportId)); leave(reportId);
    // The held task: Release.
    row(heldId)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(heldId)->findChild<QPushButton *>("teamWaitingReleaseAction"));
    row(heldId)->findChild<QPushButton *>("teamWaitingReleaseAction")->click();
    QTRY_COMPARE(calls(script(), "release").size(), 1);
    sent = calls(script(), "release").first();
    QCOMPARE(sent["task"].toInt(), 2);
    QTRY_VERIFY(!row(heldId)); leave(heldId);
    // The step Zerus could not take: Retry.
    row(retryId)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(retryId)->findChild<QPushButton *>("teamWaitingReleaseAction"));
    row(retryId)->findChild<QPushButton *>("teamWaitingReleaseAction")->click();
    QTRY_COMPARE(calls(script(), "retry").size(), 1);
    QCOMPARE(calls(script(), "retry").first()["task"].toInt(), 5);
    QTRY_VERIFY(!row(retryId)); leave(retryId);
    // The end of the team: the summary reaches the lead.
    row(finishId)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(finishId)->findChild<QLineEdit *>("teamWaitingFinishSummary"));
    row(finishId)->findChild<QLineEdit *>("teamWaitingFinishSummary")->setText("Shipped 0.43");
    row(finishId)->findChild<QPushButton *>("teamWaitingFinishAction")->click();
    QTRY_COMPARE(calls(script(), "finish").size(), 1);
    QCOMPARE(calls(script(), "finish").first()["summary"].toString(), QStringLiteral("Shipped 0.43"));
    QTRY_VERIFY(!row(finishId)); leave(finishId);
    QCOMPARE(answered, (QStringList{reportId, heldId, retryId, finishId}));
}

// U29 (owner, 2026-10-31: an addition waited for the owner and could be
// neither added nor cancelled): Cancel declines the addition with its id, so
// nothing joins and the item leaves at once.
void TestTeamsUiWaiting::waitingAdditionCancelDeclinesIt()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    setTeam(answerTeam());
    QVERIFY(writeWaiting(script() + ".waiting", answerItems(now)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 1000); page.show();
    const QString addition = "team-1/owner_word_asked/addition-add-1";
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(row(addition));
    row(addition)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(addition)->findChild<QPushButton *>("teamWaitingAdditionCancel"));
    row(addition)->findChild<QPushButton *>("teamWaitingAdditionCancel")->click();
    QTRY_COMPARE(calls(script(), "add-to-phase").size(), 1);
    const auto sent = calls(script(), "add-to-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["phase"].toInt(), 1);
    QCOMPARE(sent["confirm"].toString(), QStringLiteral("add-1"));
    QCOMPARE(sent["decline"].toBool(), true);
    QVERIFY(!sent.contains("confirmation"));
    QTRY_VERIFY(!row(addition));
    QCOMPARE(answered, QStringList{addition});
}

// U63 (owner, 2026-10-31): a phase that waits for the owner's acceptance is
// answered in its row: how it ended, Send back with a comment (required) or
// Accept phase. The team is read first; the item leaves once answered.
void TestTeamsUiWaiting::aPhaseIsAcceptedOrSentBackFromWaitingForYou()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto team = answerTeam();
    auto phases = team["phases"].toArray();
    auto first = phases[0].toObject(); first["state"] = "awaiting_acceptance"; first["acceptance"] = "owner"; phases[0] = first;
    team["phases"] = phases;
    setTeam(team);
    auto item = waitingItem("1", "phase_acceptance", "accept_phase", "Phase 1 is ready for your acceptance", now - 60);
    item["phase"] = 1;
    item["summary"] = QJsonObject{{"accepted", 3}, {"rejected", 1}, {"cancelled", 0}, {"tests", "tests pass"}, {"minutes", 42}};
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{item}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &done) { answered << done.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 700); page.show();
    const QString id = "team-1/phase_acceptance/1";
    const auto row = [&]() -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(row());
    QCOMPARE(row()->findChild<QLabel *>("teamWaitingKind")->text(), QStringLiteral("Phase acceptance"));
    QCOMPARE(TeamWaitingPage::parse(item).summary.text(), QStringLiteral("3 accepted, 1 rejected, 0 cancelled; tests pass; 42 min"));
    row()->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row() && row()->findChild<QFrame *>("teamWaitingAcceptance"));
    QCOMPARE(row()->findChild<QLabel *>("teamWaitingAcceptanceSummary")->text(), QStringLiteral("3 accepted, 1 rejected, 0 cancelled; tests pass; 42 min"));
    // Send back wants a comment.
    auto *sendBack = row()->findChild<QPushButton *>("teamWaitingSendBack");
    QVERIFY(!sendBack->isEnabled());
    row()->findChild<QLineEdit *>("teamWaitingSendBackComment")->setText("Add the migration steps to the notes");
    QVERIFY(sendBack->isEnabled());
    sendBack->click();
    QTRY_COMPARE(calls(script(), "send-back-phase").size(), 1);
    auto sent = calls(script(), "send-back-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["phase"].toInt(), 1);
    QCOMPARE(sent["comment"].toString(), QStringLiteral("Add the migration steps to the notes"));
    QTRY_VERIFY(!row());
    QCOMPARE(answered, QStringList{id});
    QCOMPARE(calls(script(), "accept-phase").size(), 0);
    // The lead reworked it and it waits again: Accept phase.
    page.refresh();
    QTRY_VERIFY(row());
    row()->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row() && row()->findChild<QPushButton *>("teamWaitingAcceptPhase") && row()->findChild<QPushButton *>("teamWaitingAcceptPhase")->isEnabled());
    row()->findChild<QPushButton *>("teamWaitingAcceptPhase")->click();
    QTRY_COMPARE(calls(script(), "accept-phase").size(), 1);
    sent = calls(script(), "accept-phase").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["phase"].toInt(), 1);
    QTRY_VERIFY(!row());
    QCOMPARE(answered, (QStringList{id, id}));
    // A phase accepted meanwhile sends nothing.
    first["state"] = "done"; phases[0] = first; team["phases"] = phases;
    setTeam(team);
    page.refresh();
    QTRY_VERIFY(row());
    row()->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(([&] {
        auto *notice = row() ? row()->findChild<QLabel *>("teamWaitingAnswerNotice") : nullptr;
        return notice && notice->text().contains(QStringLiteral("nothing was sent"));
    })());
    QVERIFY(!row()->findChild<QPushButton *>("teamWaitingAcceptPhase"));
    QCOMPARE(calls(script(), "accept-phase").size(), 1);
}

// #724: the banner answers in its row too; an item answered there leaves the
// banner and the page at once, and its notifications (the waiting one and the
// team attention one that covers it) are withdrawn.
void TestTeamsUiWaiting::waitingBannerAnswersInPlaceAndWithdrawsTheNotice()
{
    Settings::set("workspace/skin", "whitewh");
    Settings::remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); Settings::remove(TeamWaitingNotices::kSetting); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const auto team = answerTeam();
    setTeam(team);
    const auto items = answerItems(now);
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{items[3], items[6]}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1") && !controller.team("team-1")->attention.isEmpty());
    TeamWaitingFeed feed(&controller);
    QSignalSpy withdrawn(&feed, &TeamWaitingFeed::withdrawn);
    TeamWaitingPage page(&controller);
    // SessionsWindow's wiring: either place answers, both lose the item.
    page.answered = [&](const TeamWaitingPage::Item &item) { feed.answered(item); };
    QObject::connect(&feed, &TeamWaitingFeed::dropped, &page, [&](const QString &id) { page.drop(id); });
    QWidget host; host.setObjectName("bannerHost");
    auto *v = new QVBoxLayout(&host); v->setContentsMargins(0, 0, 0, 0);
    auto *banner = new TeamWaitingBanner(&feed); banner->setDark(true);
    v->addWidget(banner); v->addStretch();
    darkPalette();
    host.resize(820, 360); host.show();
    page.resize(820, 600); page.show();
    feed.setActive(true);
    const QString permission = "team-1/permission_escalated/backend", held = "team-1/task_waits_owner/2";
    const auto pageRow = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(banner->findChild<QPushButton *>("teamWaitingBannerToggle") && pageRow(permission));
    banner->findChild<QPushButton *>("teamWaitingBannerToggle")->click();
    const auto bannerRow = [&](const QString &id) -> QWidget * {
        for (auto *row : banner->findChildren<QWidget *>("teamWaitingBannerRow")) if (row->property("itemId") == id) return row;
        return nullptr;
    };
    QTRY_VERIFY(bannerRow(permission) && bannerRow(held));
    // Task 116: a held task answers in its row too (Release), like the permission.
    QCOMPARE(bannerRow(held)->findChild<QPushButton *>("teamWaitingBannerAction")->text(), QStringLiteral("Answer"));
    QVERIFY(bannerRow(held)->findChild<QToolButton *>("teamWaitingBannerOpen"));
    auto *answer = bannerRow(permission)->findChild<QPushButton *>("teamWaitingBannerAction");
    QCOMPARE(answer->text(), QStringLiteral("Answer"));
    QVERIFY(bannerRow(permission)->findChild<QToolButton *>("teamWaitingBannerOpen"));
    answer->click();
    QTRY_VERIFY(bannerRow(permission) && bannerRow(permission)->findChild<QPushButton *>("teamWaitingPermissionDeny"));
    TestWait::settle();
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(banner, "u724-banner-permission-dark");
        banner->setDark(false);
        lightShot(banner, "u724-banner-permission-light");
        darkPalette(); banner->setDark(true);
    }
    QTRY_VERIFY(bannerRow(permission) && bannerRow(permission)->findChild<QLineEdit *>("teamWaitingPermissionDenyReason"));
    // Deny with a reason for the member.
    bannerRow(permission)->findChild<QLineEdit *>("teamWaitingPermissionDenyReason")->setText("Not on main");
    bannerRow(permission)->findChild<QPushButton *>("teamWaitingPermissionDeny")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 1);
    const auto sent = calls(script(), "answer").first();
    const auto question = Teams::parse(team).member("backend")->permission;
    const auto native = sent["answers"].toArray()[0].toObject();
    QCOMPARE(native["selected_option_ids"].toArray(), QJsonArray{question.native.deny});
    QCOMPARE(native["text"].toString(), QStringLiteral("Not on main"));
    QCOMPARE(sent["expected_question_hash"].toString(), question.hash);
    // At once: out of the banner and the page, before any read.
    QTRY_VERIFY(!bannerRow(permission) && !pageRow(permission));
    QVERIFY(pageRow(held));
    QCOMPARE(feed.items().size(), 1);
    QCOMPARE(withdrawn.count(), 1);
    const auto tokens = withdrawn.first().first().toStringList();
    const auto *listed = controller.team("team-1");
    QVERIFY(tokens.contains(QStringLiteral("zerus-team-attention:team-1:waiting:") + permission));
    QVERIFY2(tokens.contains(TeamAttentionNotices::token(*listed, listed->attention.first())), qPrintable(tokens.join(' ')));
    // One item left: the banner's single line.
    QTRY_VERIFY(banner->findChild<QWidget *>("teamWaitingBannerLine"));
    QCOMPARE(banner->findChild<QWidget *>("teamWaitingBannerLine")->property("itemId").toString(), held);
    // An answer between rebuilds never shows as a window of its own.
    for (auto *window : QApplication::topLevelWidgets())
        QVERIFY2(!(window->objectName() == QLatin1String("teamWaitingAnswer") && window->isVisible()), "a stray answer window");
    feed.setActive(false);
}

// #706: the waiting list read every 30 s redraws its banners only when the
// list changed, and asks for no team poll of its own.
void TestTeamsUiWaiting::aWaitingReadThatChangesNothingRedrawsNothing()
{
    Settings::remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { Settings::remove(TeamWaitingNotices::kSetting); });
    const auto items = waitingKinds(double(QDateTime::currentSecsSinceEpoch()));
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{items[1]}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingFeed feed(&controller);
    QSignalSpy changed(&feed, &TeamWaitingFeed::changed);
    const auto reads = [&] { return calls(script(), "waiting").size(); };
    feed.setActive(true);
    QTRY_COMPARE(changed.count(), 1);
    const auto listReads = calls(script(), "list").size();
    feed.refresh();
    QTRY_COMPARE(reads(), 2);
    QTest::qWait(100);
    QCOMPARE(changed.count(), 1);
    QCOMPARE(calls(script(), "list").size(), listReads);
    QVERIFY(writeWaiting(script() + ".waiting", items));
    feed.refresh();
    QTRY_COMPARE(changed.count(), 2);
    QCOMPARE(feed.total(), int(items.size()));
}

// #740 (owner, 2026-10-31): the lead's own question (its AskUserQuestion)
// and its question proposal wait in Waiting for you and are answered there:
// the session's options and Other, sent once with the question's id and hash;
// the proposal's options with the recommended one marked, or the owner's own
// words, sent with its hash. Each leaves at once.
void TestTeamsUiWaiting::aLeadsQuestionIsAnsweredFromWaitingForYou()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto team = answerTeam();
    auto members = team["members"].toArray();
    auto lead = members[0].toObject();
    lead["question"] = QJsonObject{{"question_id", "toolu_cache"}, {"question_hash", "hash-cache"}, {"tool", "Build cache"},
        {"command", ""}, {"reason", ""}, {"asked_at", now - 30},
        {"questions", QJsonArray{QJsonObject{{"id", "q_0"}, {"header", "Build cache"}, {"question", "Install the shared build cache?"},
            {"multi_select", false}, {"allow_other", true}, {"options", QJsonArray{
                QJsonObject{{"id", "1"}, {"label", "Yes, on D: (Recommended)"}, {"description", "A folder on D: with a 20 GB limit."}},
                QJsonObject{{"id", "2"}, {"label", "No"}, {"description", "Builds run as they do now."}}}}}}}};
    members[0] = lead;
    team["members"] = members;
    auto proposals = team["proposals"].toArray();
    proposals.append(QJsonObject{{"id", "p3"}, {"kind", "question"}, {"by", "Lead"}, {"at", now - 20}, {"state", "open"}, {"reason", ""},
        {"hash", "h3"}, {"change", QJsonObject{{"question", "Which cache size?"}, {"options", QJsonArray{"10 GB", "20 GB"}}, {"recommended", "20 GB"}}}});
    team["proposals"] = proposals;
    setTeam(team);
    auto asked = waitingItem("Lead", "permission_escalated", "answer_permission", "lead (Lead) waits for your answer: Install the shared build cache?", now - 30);
    asked["member"] = "Lead"; asked["member_id"] = "lead";
    auto proposal = waitingItem("proposal-p3", "owner_word_asked", "decide_proposal", "The lead asks: Which cache size?", now - 20);
    proposal["proposal"] = "p3";
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{proposal, asked}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 760); page.show();
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    const QString leadItem = asked["id"].toString(), proposalItem = proposal["id"].toString();
    QTRY_VERIFY(row(leadItem) && row(proposalItem));
    row(leadItem)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    row(proposalItem)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row(leadItem)->findChild<QLabel *>("teamWaitingQuestionPrompt") && row(proposalItem)->findChild<QRadioButton *>("teamProposalQuestionOption"));
    // The session's question whole, its options and Other.
    QCOMPARE(row(leadItem)->findChild<QLabel *>("teamWaitingQuestionPrompt")->text(), QStringLiteral("Install the shared build cache?"));
    QStringList offered;
    for (auto *option : row(leadItem)->findChildren<QAbstractButton *>("teamWaitingQuestionOption")) offered << option->text();
    QCOMPARE(offered, (QStringList{"Yes, on D: (Recommended)", "No"}));
    auto *send = row(leadItem)->findChild<QPushButton *>("teamWaitingQuestionAnswer");
    QVERIFY(!send->isEnabled());
    // The proposal's options, the recommended one marked.
    QStringList choices;
    for (auto *option : row(proposalItem)->findChildren<QRadioButton *>("teamProposalQuestionOption")) choices << option->text();
    QCOMPARE(choices, (QStringList{"10 GB", "20 GB (recommended)"}));
    // Other with the owner's words.
    row(leadItem)->findChild<QAbstractButton *>("teamWaitingQuestionOther")->click();
    QTRY_VERIFY(row(leadItem)->findChild<QLineEdit *>("teamWaitingQuestionOtherText"));
    row(leadItem)->findChild<QLineEdit *>("teamWaitingQuestionOtherText")->setText("Yes, on D:, but 30 GB");
    TestWait::settle();
    shot(&page, "u740-waiting-questions-dark");
    page.setDark(false);
    lightShot(&page, "u740-waiting-questions-light");
    darkPalette(); page.setDark(true);
    QTRY_VERIFY(row(leadItem)->findChild<QPushButton *>("teamWaitingQuestionAnswer")->isEnabled());
    row(leadItem)->findChild<QPushButton *>("teamWaitingQuestionAnswer")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 1);
    auto sent = calls(script(), "answer").first();
    QCOMPARE(sent["member"].toString(), QStringLiteral("lead"));
    QCOMPARE(sent["question_id"].toString(), QStringLiteral("toolu_cache"));
    QCOMPARE(sent["expected_question_hash"].toString(), QStringLiteral("hash-cache"));
    QCOMPARE(sent["answers"].toArray(), (QJsonArray{QJsonObject{{"question_id", "q_0"}, {"selected_option_ids", QJsonArray()},
                                                                {"text", "Yes, on D:, but 30 GB"}}}));
    QTRY_VERIFY(!row(leadItem));
    // The proposal: the recommended option, sent with its hash.
    for (auto *option : row(proposalItem)->findChildren<QRadioButton *>("teamProposalQuestionOption"))
        if (option->property("option") == "20 GB") option->click();
    QTRY_VERIFY(row(proposalItem)->findChild<QPushButton *>("teamProposalAnswer")->isEnabled());
    row(proposalItem)->findChild<QPushButton *>("teamProposalAnswer")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    sent = calls(script(), "decide-proposal").first();
    QCOMPARE(sent["proposal"].toString(), QStringLiteral("p3"));
    QCOMPARE(sent["accept"].toBool(), true);
    QCOMPARE(sent["expected_hash"].toString(), QStringLiteral("h3"));
    QCOMPARE(sent["answer"].toString(), QStringLiteral("20 GB"));
    QTRY_VERIFY(!row(proposalItem));
    QCOMPARE(answered, (QStringList{leadItem, proposalItem}));
}

// #742 (owner, 2026-10-31): a test or build tool the project lacks on this
// computer is offered in Waiting for you, without a team: why it is needed,
// what it saves and the install command; Deny with a reason, or Allow, sent as
// the owner's decide-tool. A failed install shows its output and retries.
void TestTeamsUiWaiting::waitingToolOffersAreAnsweredInPlace()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    const double now = double(QDateTime::currentSecsSinceEpoch());
    setTeam(answerTeam());
    const auto tool = [now](const QString &offer, const QString &name, const QString &state, const QString &command, double age) {
        return QJsonObject{{"id", "tools/" + offer}, {"team", ""}, {"team_name", "Zerus"}, {"kind", "tool_missing"},
                           {"title", QStringLiteral("Zerus needs %1: the shared build cache").arg(name)}, {"since", now - age},
                           {"action", "decide_tool"}, {"offer", offer}, {"tool", name},
                           {"why", "the shared build cache ([build_cache]) for Rust"},
                           {"saves", "rebuilding the same crates in every worktree of the team"}, {"command", command},
                           {"state", state}, {"output", state == "failed" ? QStringLiteral("No package found matching input criteria.") : QString()}};
    };
    const QJsonArray items{tool("a1", "sccache", "offered", "winget install --id Mozilla.sccache --exact --scope user --silent", 30),
                           tool("b2", "ccache", "failed", "winget install --id Ccache.Ccache --exact --scope user --silent", 60),
                           tool("c3", "gcovr", "offered", "", 90)};
    QVERIFY(writeWaiting(script() + ".waiting", items));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered, opened;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    page.openTeam = [&](const TeamWaitingPage::Item &item) { opened << item.id; };
    darkPalette(); page.setDark(true);
    page.resize(900, 900); page.show();
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(row("tools/a1") && row("tools/b2") && row("tools/c3"));
    // Offers of a project are no team's: the header counts no team for them.
    QCOMPARE(page.findChild<QLabel *>("teamWaitingCount")->text(), QStringLiteral("3 items"));
    // A project's offer has no team to open; a click on its row opens nothing.
    QVERIFY(!row("tools/a1")->findChild<QPushButton *>("teamWaitingOpen"));
    QCOMPARE(row("tools/a1")->findChild<QLabel *>("teamWaitingKind")->text(), QStringLiteral("Tool"));
    QCOMPARE(row("tools/a1")->findChild<QLabel *>("teamWaitingTeam")->toolTip(), QStringLiteral("Project Zerus"));
    QTest::mouseClick(row("tools/a1"), Qt::LeftButton, {}, QPoint(row("tools/a1")->width() / 3, row("tools/a1")->height() / 2));
    QVERIFY(opened.isEmpty());
    for (const auto &id : {"tools/a1", "tools/b2", "tools/c3"}) row(id)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row("tools/a1")->findChild<QFrame *>("teamWaitingTool") && row("tools/b2")->findChild<QFrame *>("teamWaitingTool")
                && row("tools/c3")->findChild<QFrame *>("teamWaitingTool"));
    auto *offer = row("tools/a1")->findChild<QFrame *>("teamWaitingTool");
    QCOMPARE(offer->findChild<QLabel *>("teamWaitingToolHead")->text(), QStringLiteral("Install sccache on this computer?"));
    QVERIFY(offer->findChild<QLabel *>("teamWaitingToolWhy")->text().contains(QStringLiteral("shared build cache")));
    QVERIFY(offer->findChild<QLabel *>("teamWaitingToolSaves")->text().contains(QStringLiteral("every worktree")));
    QCOMPARE(offer->findChild<QLabel *>("teamWaitingToolCommand")->text(), QStringLiteral("winget install --id Mozilla.sccache --exact --scope user --silent"));
    QVERIFY(offer->findChild<QPushButton *>("teamWaitingToolAllow")->isEnabled());
    // No manager here installs it: Allow is off, Deny still answers.
    auto *none = row("tools/c3")->findChild<QFrame *>("teamWaitingTool");
    QVERIFY(!none->findChild<QPushButton *>("teamWaitingToolAllow")->isEnabled());
    QVERIFY(none->findChild<QPushButton *>("teamWaitingToolDeny")->isEnabled());
    QVERIFY(none->findChild<QLabel *>("teamWaitingToolCommand")->text().contains(QStringLiteral("No package manager")));
    // A failed install shows its output and retries.
    auto *failed = row("tools/b2")->findChild<QFrame *>("teamWaitingTool");
    QCOMPARE(failed->findChild<QLabel *>("teamWaitingToolHead")->text(), QStringLiteral("Installing ccache failed"));
    QVERIFY(failed->findChild<QLabel *>("teamWaitingToolOutput")->text().contains(QStringLiteral("No package found")));
    QCOMPARE(failed->findChild<QPushButton *>("teamWaitingToolAllow")->text(), QStringLiteral("Retry install"));
    for (auto *label : page.findChildren<QLabel *>()) QVERIFY2(!label->text().contains(QStringLiteral(" · ")), qPrintable(label->text()));
    TestWait::settle();
    shot(&page, "u742-tool-offers-dark");
    page.setDark(false);
    lightShot(&page, "u742-tool-offers-light");
    darkPalette(); page.setDark(true);
    QTRY_VERIFY(row("tools/a1")->findChild<QFrame *>("teamWaitingTool"));
    // Deny with its reason: remembered by hgs, sent as the owner.
    row("tools/a1")->findChild<QLineEdit *>("teamWaitingToolDenyReason")->setText("Not on this laptop");
    row("tools/a1")->findChild<QPushButton *>("teamWaitingToolDeny")->click();
    QTRY_COMPARE(calls(script(), "decide-tool").size(), 1);
    auto sent = calls(script(), "decide-tool").first();
    QCOMPARE(sent["offer"].toString(), QStringLiteral("a1"));
    QCOMPARE(sent["allow"].toBool(), false);
    QCOMPARE(sent["reason"].toString(), QStringLiteral("Not on this laptop"));
    QCOMPARE(sent["by"].toString(), QStringLiteral("human"));
    QTRY_VERIFY(!row("tools/a1"));
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{items[1], items[2]}));
    // Allow of the failed one: the install runs again as the owner's operation.
    row("tools/b2")->findChild<QPushButton *>("teamWaitingToolAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-tool").size(), 2);
    sent = calls(script(), "decide-tool").at(1);
    QCOMPARE(sent["offer"].toString(), QStringLiteral("b2"));
    QCOMPARE(sent["allow"].toBool(), true);
    QVERIFY(!sent.contains("reason"));
    QTRY_COMPARE(answered, (QStringList{"tools/a1", "tools/b2"}));
}

// Task 95: the lead stalled while its decisions wait. The team bar says so
// and opens the lead's session; the owner is notified once per round, by one
// notice, never by both lists.
void TestTeamsUiWaiting::aStalledLeadShowsOnTheTeamBarAndNotifiesEachRound()
{
    auto team = phasedTeam();
    const auto stall = [](int round) {
        return QJsonObject{{"kind", "lead_stalled"}, {"member", "Architect"}, {"member_id", "lead"}, {"since", 1792866600.0},
                           {"minutes", 42}, {"reason", "rejected_tool"}, {"items", 3}, {"members", 2}, {"round", round},
                           {"summary", "The lead stalled (a refused tool call ended its turn): 3 decisions wait for it, the oldest 42 min, 2 members blocked"}};
    };
    team["attention"] = QJsonArray{stall(1)};
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    QStringList opened;
    workspace.openSession = [&opened](const QString &host, const QString &session) { opened << host + QLatin1Char('|') + session; };
    workspace.resize(1300, 900); workspace.show();
    controller.refresh(); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamHeaderLeadStalled"));
    auto *bar = workspace.findChild<QPushButton *>("teamHeaderLeadStalled");
    QCOMPARE(bar->text(), QStringLiteral("Lead stalled: 3 decisions wait"));
    QVERIFY(bar->toolTip().contains(QStringLiteral("a refused tool call ended its turn")));
    bar->click();
    QCOMPARE(opened, QStringList{QStringLiteral("|claude/checkout/lead")});
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamAttentionOpenLead"));
    workspace.findChild<QPushButton *>("teamAttentionOpenLead")->click();
    QCOMPARE(opened.size(), 2);
    shot(&workspace, "task95-lead-stalled");

    // One notice per round: the same round again raises nothing, the next one does.
    TeamAttentionNotices notices;
    const auto first = notices.observe({Teams::parse(team)});
    QCOMPARE(first.raised.size(), 1);
    QVERIFY(first.raised[0].body.startsWith(QStringLiteral("The lead stalled")));
    QVERIFY(notices.observe({Teams::parse(team)}).raised.isEmpty());
    auto next = team; next["attention"] = QJsonArray{stall(2)};
    const auto second = notices.observe({Teams::parse(next)});
    QCOMPARE(second.raised.size(), 1); QCOMPARE(second.cleared.size(), 1);
    // Waiting for you lists the same round: the team's notice covers it.
    const TeamWaitingNotices::Item waiting{QStringLiteral("team-1/lead_stalled/Architect-2"), QStringLiteral("team-1"),
                                           QStringLiteral("Checkout redesign"), QStringLiteral("lead_stalled"), QStringLiteral("The lead stalled")};
    QVERIFY(TeamWaitingNotices::covered({Teams::parse(next)}, waiting));
    QVERIFY(!TeamWaitingNotices::covered({Teams::parse(team)}, waiting));
    QCOMPARE(TeamWaitingPage::kindLabel(QStringLiteral("lead_stalled")), QStringLiteral("Lead stalled"));
    // The lead moved: the bar clears.
    team["attention"] = QJsonArray{};
    setTeam(team);
    QTRY_VERIFY(([&] { controller.refresh(); return !workspace.findChild<QPushButton *>("teamHeaderLeadStalled"); })());
}

// Task 97: what Zerus answered safely while the owner was away waits under
// Deferred with its exact text; Allow, Deny or an answer becomes one message
// (hgs team answer-deferred), with no team to read first.
void TestTeamsUiWaiting::deferredRequestsAreAnsweredUnderDeferred()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    setTeam(answerTeam());
    const QJsonArray items{
        QJsonObject{{"id", "deferred/ab12"}, {"team", "team-1"}, {"team_name", "Checkout redesign"}, {"kind", "deferred"},
                    {"title", "Deferred permission of Backend: Bash: cargo test --locked"}, {"since", now - 60}, {"action", "answer_deferred"},
                    {"deferred", "ab12"}, {"request", "permission"}, {"text", "Bash: cargo test --locked"}, {"session", "codex/x/dev"},
                    {"outcome", "Zerus denied it for now"}, {"questions", QJsonArray{}}},
        QJsonObject{{"id", "deferred/cd34"}, {"team", ""}, {"team_name", "claude/p/solo"}, {"kind", "deferred"},
                    {"title", "Deferred question of claude/p/solo: Which storage?"}, {"since", now - 30}, {"action", "answer_deferred"},
                    {"deferred", "cd34"}, {"request", "question"}, {"text", "Which storage?"}, {"session", "claude/p/solo"},
                    {"outcome", "Zerus chose Postgres (Recommended) for now"},
                    {"questions", QJsonArray{QJsonObject{{"id", "q"}, {"question", "Which storage?"},
                        {"options", QJsonArray{QJsonObject{{"id", "a"}, {"label", "SQLite"}}, QJsonObject{{"id", "b"}, {"label", "Postgres (Recommended)"}}}}}}}},
        QJsonObject{{"id", "team-1/phase_draft/3"}, {"team", "team-1"}, {"team_name", "Checkout redesign"}, {"kind", "phase_draft"},
                    {"title", "Phase 3 draft waits"}, {"since", now - 10}, {"action", "approve_phase"}, {"phase", 3}}};
    QVERIFY(writeWaiting(script() + ".waiting", items));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    QStringList answered;
    page.answered = [&](const TeamWaitingPage::Item &item) { answered << item.id; };
    page.resize(900, 900); page.show();
    const auto row = [&](const QString &id) -> QFrame * {
        for (auto *frame : page.findChildren<QFrame *>("teamWaitingRow")) if (frame->property("itemId") == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(row("deferred/ab12") && row("deferred/cd34") && row("team-1/phase_draft/3"));
    QVERIFY(page.findChild<QLabel *>("teamWaitingDeferredHead"));
    QCOMPARE(row("deferred/ab12")->findChild<QLabel *>("teamWaitingKind")->text(), QStringLiteral("Deferred"));
    // Deferred comes after everything else.
    const auto rows = page.findChildren<QFrame *>("teamWaitingRow");
    QCOMPARE(rows.first()->property("itemId").toString(), QStringLiteral("team-1/phase_draft/3"));
    const int statusReads = calls(script(), "status").size();
    for (const auto &id : {"deferred/ab12", "deferred/cd34"}) row(id)->findChild<QPushButton *>("teamWaitingAnswerToggle")->click();
    QTRY_VERIFY(row("deferred/ab12")->findChild<QFrame *>("teamWaitingDeferred") && row("deferred/cd34")->findChild<QFrame *>("teamWaitingDeferred"));
    auto *permission = row("deferred/ab12")->findChild<QFrame *>("teamWaitingDeferred");
    QCOMPARE(permission->findChild<QLabel *>("teamWaitingDeferredText")->text(), QStringLiteral("Bash: cargo test --locked"));
    QCOMPARE(permission->findChild<QLabel *>("teamWaitingDeferredOutcome")->text(), QStringLiteral("Zerus denied it for now"));
    QVERIFY(permission->findChild<QPushButton *>("teamWaitingDeferredAllow") && permission->findChild<QPushButton *>("teamWaitingDeferredDeny"));
    QVERIFY(!row("deferred/cd34")->findChild<QPushButton *>("teamWaitingDeferredAllow"));
    for (auto *label : page.findChildren<QLabel *>()) QVERIFY2(!label->text().contains(QStringLiteral(" · ")), qPrintable(label->text()));
    shot(&page, "task97-deferred");
    // Allow: one owner operation, never a team read or a replayed tool.
    permission->findChild<QPushButton *>("teamWaitingDeferredAllow")->click();
    QTRY_COMPARE(calls(script(), "answer-deferred").size(), 1);
    auto sent = calls(script(), "answer-deferred").first();
    QCOMPARE(sent["id"].toString(), QStringLiteral("ab12"));
    QCOMPARE(sent["decision"].toString(), QStringLiteral("allow"));
    QCOMPARE(sent["by"].toString(), QStringLiteral("human"));
    QTRY_VERIFY(!row("deferred/ab12"));
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{items[1], items[2]}));
    // A question: an option is the owner's answer in words.
    QPushButton *sqlite = nullptr;
    for (auto *pick : row("deferred/cd34")->findChildren<QPushButton *>("teamWaitingDeferredOption")) if (pick->text() == QStringLiteral("SQLite")) sqlite = pick;
    QVERIFY(sqlite);
    sqlite->click();
    QTRY_COMPARE(calls(script(), "answer-deferred").size(), 2);
    sent = calls(script(), "answer-deferred").at(1);
    QCOMPARE((QStringList{sent["id"].toString(), sent["decision"].toString(), sent["text"].toString()}),
             (QStringList{"cd34", "answer", "SQLite"}));
    QTRY_COMPARE(answered, (QStringList{"deferred/ab12", "deferred/cd34"}));
    QCOMPARE(calls(script(), "status").size(), statusReads);
}

void TestTeamsUiWaiting::phaseCheckCardShowsWarningAndSendsExplicitOverride()
{
    auto source = phasedTeam();
    auto phases = source.value("phases").toArray();
    auto phase = phases.first().toObject();
    const int n = phase.value("n").toInt();
    phase["target_hours"] = 1.0;
    phase["checks"] = QJsonObject{{"known_hours", 6.5}, {"missing_estimates", 2}, {"target_exceeded", true},
                                 {"unresolved", QJsonArray{"Task #5: task #1 has not been accepted"}}};
    phases[0] = phase; source["phases"] = phases;
    auto tasks = source.value("tasks").toArray();
    auto task = tasks.first().toObject();
    task.remove("estimate_source");
    const QJsonArray prerequisites{QJsonObject{{"task", 999}}};
    task["requires"] = prerequisites;
    tasks[0] = task; source["tasks"] = tasks;
    source["proposals"] = QJsonArray{QJsonObject{{"id", "phase-check"}, {"hash", "h1"}, {"kind", "approve-phase"}, {"state", "open"},
        {"change", QJsonObject{{"op", "approve-phase"}, {"input", QJsonObject{{"phase", n}}}, {"summary", "Approve phase"}}}}};
    const auto model = parse(source);
    const auto plan = planInput(model);
    QCOMPARE(plan.value("tasks").toArray().first().toObject().value("requires").toArray(), prerequisites);
    QCOMPARE(plan.value("phases").toArray().first().toObject().value("target_hours").toDouble(), 1.0);
    const auto approval = phaseApproval(model, n);
    QVERIFY(approval.text.contains(">= 6.5 h"));
    QVERIFY(approval.text.contains("2 tasks without an estimate"));
    QVERIFY(approval.text.contains("exceeds"));
    QCOMPARE(approval.action, QStringLiteral("Approve anyway"));
    QCOMPARE(approval.input.value("approve_anyway").toBool(), true);
    TeamProposalCards cards;
    QJsonObject sent;
    cards.decide = [&](const QJsonObject &input, const TeamProposalCards::Done &) { sent = input; };
    cards.setTeam(&model); cards.show();
    auto *allow = cards.findChild<QPushButton *>("teamProposalAllow");
    QVERIFY(allow); QCOMPARE(allow->text(), QStringLiteral("Approve anyway"));
    bool shown = false;
    for (auto *line : cards.findChildren<QLabel *>()) shown |= line->text().contains("has not been accepted");
    QVERIFY(shown);
    allow->click();
    QCOMPARE(sent.value("approve_anyway").toBool(), true);
    QCOMPARE(sent.value("expected_hash").toString(), QStringLiteral("h1"));
}

QTEST_MAIN(TestTeamsUiWaiting)
// Task 109: a drive under its threshold waits for the owner as no team's item;
// its row opens Settings > Build folders and its notice names the drive.
void TestTeamsUiWaiting::aLowDriveOpensBuildFoldersAndItsNoticeNamesTheDrive()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const QJsonObject low{{"id", "disk/D:"}, {"team", ""}, {"team_name", "D:"}, {"kind", "disk_low"}, {"since", now - 60},
                          {"title", "Drive D: has 12.0 GB free, under 90.0 GB; Zerus's build folders there hold 31.0 GB"}, {"action", "open_disk"}};
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{low}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    int opened = 0;
    page.openDisk = [&] { ++opened; };
    page.resize(980, 520); page.show();
    WAIT_VERIFY(page.findChild<QPushButton *>("teamWaitingOpenDisk"));
    QCOMPARE(page.findChild<QPushButton *>("teamWaitingOpenDisk")->text(), QStringLiteral("Open Build folders"));
    page.findChild<QPushButton *>("teamWaitingOpenDisk")->click();
    QCOMPARE(opened, 1);
    TeamWaitingNotices notices;
    const auto fresh = notices.observe({{"disk/D:", "", "D:", "disk_low", low["title"].toString()}}, {}, true);
    QCOMPARE(fresh.size(), 1);
    QCOMPARE(fresh[0].title, QStringLiteral("Low disk space on D:"));
    QCOMPARE(fresh[0].body, low["title"].toString());
}

// Task 112: a paused team waits for the owner's Resume team in Waiting for
// you, saying since when and by whom; it is a state the owner set, so it is
// listed but never notified, and Resume team resumes the team.
void TestTeamsUiWaiting::aPausedTeamWaitsWithResumeTeamAndIsNeverNotified()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const QJsonObject paused{{"id", "team-1/team_paused/1800000000"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "team_paused"},
                             {"since", now - 600}, {"title", "Team paused since 09:12 by You: members get no tasks"}, {"action", "resume_team"},
                             {"paused_by", "You"}, {"paused_at", now - 600}};
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{paused}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    page.resize(980, 520); page.show();
    WAIT_VERIFY(page.findChild<QPushButton *>("teamWaitingResumeTeam"));
    QCOMPARE(TeamWaitingPage::kindLabel("team_paused"), QStringLiteral("Team paused"));
    page.findChild<QPushButton *>("teamWaitingResumeTeam")->click();
    QTRY_COMPARE(calls(script(), "resume").size(), 1);
    QCOMPARE(calls(script(), "resume").first()["team"].toString(), QStringLiteral("team-1"));
    TeamWaitingNotices notices;
    QVERIFY(notices.observe({{paused["id"].toString(), "team-1", "Checkout", "team_paused", paused["title"].toString()}}, {}, true).isEmpty());
    QVERIFY(notices.known(paused["id"].toString()));
    // Resume team decides it: the item leaves at once.
    QList<TeamWaitingPage::Item> all{TeamWaitingPage::parse(paused)};
    QCOMPARE(TeamWaitingFeed::decidedIds(all, "team-1", "resume", {}), QStringList{paused["id"].toString()});
    QVERIFY(TeamWaitingFeed::decidedIds(all, "team-2", "resume", {}).isEmpty());
}

// Task 245 (hgs task 162): the lead's plan changes inside the approved goal
// are one digest: its own label, Seen, and Revert, which undoes the newest
// change and names it in its tooltip.
void TestTeamsUiWaiting::theLeadsChangesHaveTheirLabelSeenAndRevert()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const QJsonObject digest{{"id", "team-1/lead_changes/digest"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "lead_changes"},
                             {"since", now - 60}, {"title", "The lead changed the plan 2 times inside the approved goal, newest: split #4 into #4 and #9"},
                             {"action", "open_team"},
                             {"changes", QJsonArray{QJsonObject{{"id", "lc2"}, {"summary", "split #4 into #4 and #9"}}, QJsonObject{{"id", "lc1"}, {"summary", "moved #3 to phase 2"}}}}};
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{digest}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    page.resize(980, 520); page.show();
    WAIT_VERIFY(page.findChild<QPushButton *>("teamWaitingSeen"));
    QCOMPARE(TeamWaitingPage::kindLabel("lead_changes"), QStringLiteral("Lead's changes"));
    auto *revert = page.findChild<QPushButton *>("teamWaitingRevert");
    QVERIFY(revert);
    QCOMPARE(revert->toolTip(), QStringLiteral("Revert the newest change: split #4 into #4 and #9"));
    revert->click();
    QTRY_COMPARE(calls(script(), "revert-lead-change").size(), 1);
    QCOMPARE(calls(script(), "revert-lead-change").first()["change"].toString(), QStringLiteral("lc2"));
    page.findChild<QPushButton *>("teamWaitingSeen")->click();
    QTRY_COMPARE(calls(script(), "seen-lead-changes").size(), 1);
    QCOMPARE(calls(script(), "seen-lead-changes").first()["team"].toString(), QStringLiteral("team-1"));
}

// Task 304 (hgs #246, #286): a row with a state is dismissed and leaves the
// list; Show dismissed lists it with what a decision still blocks, and
// Restore brings it back. The lead-changes digest reads as one grouped line,
// the escalated wait is one row naming the lead, and the strip's header does
// not repeat the digest.
void TestTeamsUiWaiting::rowsAreDismissedAndRestoredAndTheDigestIsGrouped()
{
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const QJsonObject paused{{"id", "team-1/team_paused/1"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "team_paused"}, {"since", now - 600},
                             {"title", "Team paused since 09:12 by You: members get no tasks"}, {"action", "resume_team"}, {"state", "paused:100.0"}};
    const QJsonObject digest{{"id", "team-1/lead_changes/digest"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "lead_changes"}, {"since", now - 60},
                             {"title", "The lead changed the plan 3 times inside the approved goal, newest: added #9"}, {"action", "open_team"}, {"state", "digest:1"},
                             {"changes", QJsonArray{QJsonObject{{"id", "lc3"}, {"summary", "added #9"}}}},
                             {"groups", QJsonArray{QJsonObject{{"kind", "added"}, {"count", 2}, {"newest", QJsonArray{QJsonObject{{"summary", "added #9"}}, QJsonObject{{"summary", "added #8"}}}}},
                                                   QJsonObject{{"kind", "moved"}, {"count", 1}, {"newest", QJsonArray{QJsonObject{{"summary", "moved #3 to phase 2"}}}}}}}};
    const QJsonObject escalated{{"id", "team-1/task_wait_escalated/escalated-4"}, {"team", "team-1"}, {"team_name", "Checkout"}, {"kind", "task_wait_escalated"},
                                {"since", now - 300}, {"title", "#4 API still waits after 60 min: unknown model (Lead has not acted)"}, {"action", "open_team"},
                                {"state", "escalated:4"}, {"lead", "Lead"}, {"task", 4}};
    const QJsonObject plan{{"id", "team-2/plan_draft/plan"}, {"team", "team-2"}, {"team_name", "Orbit"}, {"kind", "plan_draft"}, {"since", now - 900},
                           {"title", "Plan of 3 tasks waits for Approve & Run"}, {"action", "approve_plan"}, {"state", "plan:7"}, {"blocking", true}, {"blocks", "Orbit"}};
    QVERIFY(writeWaiting(script() + ".waiting", QJsonArray{digest, escalated, paused}));
    QVERIFY(writeWaiting(script() + ".dismissed", QJsonArray{plan}));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingPage page(&controller);
    page.resize(980, 620); page.show();
    // One list per check: begin and end of the same calls.
    const auto sent = [this](const char *action) { const auto all = calls(script(), "waiting"); return std::any_of(all.cbegin(), all.cend(), [action](const QJsonObject &call) { return call["action"] == QLatin1String(action); }); };
    WAIT_VERIFY(page.findChild<QToolButton *>("teamWaitingDismiss"));
    QCOMPARE(page.findChildren<QToolButton *>("teamWaitingDismiss").size(), 3);
    // The digest as one grouped line; the newest few in its tooltip.
    auto *groups = page.findChild<QLabel *>("teamWaitingGroups");
    QVERIFY(groups);
    QCOMPARE(groups->text(), QStringLiteral("2 added, 1 moved"));
    QCOMPARE(groups->toolTip(), QStringLiteral("added #9\nadded #8\nmoved #3 to phase 2"));
    // The escalation is one row with its label.
    QCOMPARE(TeamWaitingPage::kindLabel("task_wait_escalated"), QStringLiteral("Lead has not acted"));
    int escalations = 0;
    for (auto *row : page.findChildren<QWidget *>("teamWaitingRow")) if (row->property("kind") == "task_wait_escalated") ++escalations;
    QCOMPARE(escalations, 1);
    // Dismiss the paused team: it leaves, the call names its state.
    QWidget *pausedRow = nullptr;
    for (auto *row : page.findChildren<QWidget *>("teamWaitingRow")) if (row->property("kind") == "team_paused") pausedRow = row;
    QVERIFY(pausedRow);
    pausedRow->findChild<QToolButton *>("teamWaitingDismiss")->click();
    QTRY_VERIFY(sent("dismiss"));
    QJsonObject dismissed;
    for (const auto &call : calls(script(), "waiting")) if (call["action"] == "dismiss") dismissed = call;
    QCOMPARE(dismissed["id"].toString(), QStringLiteral("team-1/team_paused/1"));
    QCOMPARE(dismissed["state"].toString(), QStringLiteral("paused:100.0"));
    QCOMPARE(dismissed["by"].toString(), QStringLiteral("human"));
    QTRY_VERIFY([&] { for (auto *row : page.findChildren<QWidget *>("teamWaitingRow")) if (row->property("kind") == "team_paused") return false; return true; }());
    // Show dismissed: both, the plan still blocking Orbit; Restore brings the pause back.
    QTRY_VERIFY(page.findChild<QPushButton *>("teamWaitingShowDismissed"));
    QTRY_COMPARE(page.findChild<QPushButton *>("teamWaitingShowDismissed")->text(), QStringLiteral("Show dismissed (2)"));
    page.findChild<QPushButton *>("teamWaitingShowDismissed")->click();
    QTRY_COMPARE(page.findChildren<QFrame *>("teamWaitingDismissedRow").size(), 2);
    auto *blocks = page.findChild<QLabel *>("teamWaitingStillBlocks");
    QVERIFY(blocks);
    QCOMPARE(blocks->text(), QStringLiteral("still blocks Orbit"));
    QFrame *pausedDismissed = nullptr;
    for (auto *row : page.findChildren<QFrame *>("teamWaitingDismissedRow")) if (row->property("kind") == "team_paused") pausedDismissed = row;
    QVERIFY(pausedDismissed);
    pausedDismissed->findChild<QPushButton *>("teamWaitingRestore")->click();
    QTRY_VERIFY(sent("undismiss"));
    QTRY_VERIFY([&] { for (auto *row : page.findChildren<QWidget *>("teamWaitingRow")) if (row->property("kind") == "team_paused") return true; return false; }());
    shot(&page, "task304-waiting-dismissed-dark");
    // The strip: its header names the newest item that is not the digest.
    TeamWaitingFeed feed(&controller);
    QWidget host;
    auto *v = new QVBoxLayout(&host); v->setContentsMargins(0, 0, 0, 0);
    auto *banner = new TeamWaitingBanner(&feed); banner->setDark(true);
    v->addWidget(banner); v->addStretch();
    host.resize(900, 200); host.show();
    feed.setActive(true);
    QTRY_VERIFY(banner->findChild<QLabel *>("teamWaitingBannerNewest"));
    QCOMPARE(banner->findChild<QLabel *>("teamWaitingBannerNewest")->text(), escalated["title"].toString());
    shot(banner, "task304-banner-dark");
}

#include "test_teamsui_waiting.moc"

// Task 107 (owner, 2026-11-06: proposals already allowed stayed in "4 wait
// for you" with Answer for minutes): hgs's receipt of a decision takes the
// card out of the lead's cards, the banner, its count and its notification
// at once, before any read; a read already in flight, which predates the
// decision, never shows it again; a refused decision keeps it with why.
void TestTeamsUiWaiting::aConfirmedDecisionLeavesWaitingAtOnceAndARefusedOneStays()
{
    Settings::remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([this] {
        Settings::remove(TeamWaitingNotices::kSetting);
        QFile::remove(script() + ".waitingslow");
        QFile::remove(script() + ".proposalrefuse");
    });
    QFile::remove(script() + ".waitingslow");
    QFile::remove(script() + ".proposalrefuse");
    const double now = double(QDateTime::currentSecsSinceEpoch());
    const auto team = answerTeam();
    setTeam(team);
    const auto items = answerItems(now);
    QVERIFY(writeWaiting(script() + ".waiting", items));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamWaitingFeed feed(&controller);
    feed.setInterval(600000);   // no poll: every read here is asked for
    QSignalSpy withdrawn(&feed, &TeamWaitingFeed::withdrawn), dropped(&feed, &TeamWaitingFeed::dropped);
    feed.setActive(true);
    QTRY_COMPARE(feed.items().size(), 7);
    const QString p1 = "team-1/owner_word_asked/proposal-p1", p2 = "team-1/owner_word_asked/proposal-p2";
    const auto listed = [&feed](const QString &id) {
        return std::any_of(feed.items().cbegin(), feed.items().cend(), [&id](const TeamWaitingPage::Item &item) { return item.id == id; });
    };
    // The lead's cards, as SessionsWindow wires them.
    const auto parsed = Teams::parse(team);
    TeamProposalCards cards;
    cards.decide = [&controller](const QJsonObject &input, const TeamProposalCards::Done &done) { controller.act("decide-proposal", input, done); };
    cards.setTeam(&parsed);
    QCOMPARE(cards.count(), 2);
    cards.resize(700, 500); cards.show();
    const auto card = [&cards](const QString &proposal) -> QFrame * {
        for (auto *frame : cards.findChildren<QFrame *>("teamProposalCard")) if (frame->isVisible() && frame->property("proposal") == proposal) return frame;
        return nullptr;
    };
    QTRY_VERIFY(card("p1") && card("p2"));
    // A read that predates the decision is still on its way, listing p1.
    QVERIFY(FakeCli::replace(script() + ".waitingslow", "2"));
    feed.refresh();
    // The store takes the decision: later reads no longer list p1.
    QJsonArray after = items; after.removeAt(0);
    QVERIFY(writeWaiting(script() + ".waiting", after));
    card("p1")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    // At once, before any read answered: out of the cards, the banner and its count.
    QTRY_VERIFY(!listed(p1));
    const auto seen = [&] {
        QStringList out;
        for (const auto &signal : std::as_const(dropped)) out << signal.first().toString();
        for (const auto &call : calls(script(), "decide-proposal")) out << QStringLiteral("decide:") + call["proposal"].toString();
        return out.join(QLatin1Char(' '));
    };
    QVERIFY2(feed.items().size() == 6 && feed.total() == 6, qPrintable(seen()));
    QVERIFY2(dropped.count() == 1 && dropped.first().first().toString() == p1, qPrintable(seen()));
    QCOMPARE(withdrawn.count(), 1);
    QTRY_VERIFY(!card("p1"));
    QCOMPARE(cards.count(), 1);
    // An older team read still listing the proposal does not bring the card back.
    cards.setTeam(&parsed);
    QCOMPARE(cards.count(), 1);
    QVERIFY(!card("p1"));
    // The read in flight lands and the read after it: p1 never comes back.
    QFile::remove(script() + ".waitingslow");
    QElapsedTimer watch; watch.start();
    while (watch.elapsed() < 3500) { QTest::qWait(100); QVERIFY(!listed(p1)); }
    QCOMPARE(feed.items().size(), 6);
    // Refused: p2 stays, on its card with the reason, and in the banner.
    QVERIFY(FakeCli::replace(script() + ".proposalrefuse", "hgs: The plan changed since the warning"));
    card("p2")->findChild<QLineEdit *>("teamProposalDenyReason")->setText("Not now");
    card("p2")->findChild<QPushButton *>("teamProposalDeny")->click();
    QTRY_VERIFY(card("p2") && card("p2")->findChild<QLabel *>() && [&] {
        for (auto *label : card("p2")->findChildren<QLabel *>()) if (label->text().contains("The plan changed since the warning")) return true;
        return false;
    }());
    QTest::qWait(300);
    QVERIFY(listed(p2));
    QCOMPARE(feed.items().size(), 6);
    QCOMPARE(dropped.count(), 1);
    // Task 111: hgs withdrew p2 (it can no longer apply): Allow is refused
    // with that reason and the card, the banner item and its notice leave at
    // once, as a confirmed decision's do.
    QVERIFY(FakeCli::replace(script() + ".proposalrefuse",
        "hgs: This proposal can no longer apply (Phase 2 is closed (done); propose the work in a new phase with extend instead); it was withdrawn and the lead is told why"));
    QTRY_VERIFY(card("p2") && card("p2")->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
    card("p2")->findChild<QPushButton *>("teamProposalAllow")->click();
    QTRY_VERIFY(!card("p2"));
    QTRY_COMPARE(dropped.count(), 2);
    QCOMPARE(dropped.last().first().toString(), p2);
    QVERIFY(!listed(p2));
    QCOMPARE(feed.items().size(), 5);
    QVERIFY(TeamController::withdrawn("hgs: This proposal can no longer apply (x); it was withdrawn and the lead is told why"));
    QVERIFY(!TeamController::withdrawn("hgs: The plan changed since the warning"));
    // What else a confirmed change decides: a phase by its number, a report or
    // a held task by its id; another team's or a change that names none, nothing.
    QList<TeamWaitingPage::Item> all;
    for (const auto &value : items) all << TeamWaitingPage::parse(value.toObject());
    QCOMPARE(TeamWaitingFeed::decidedIds(all, "team-1", "approve-phase", {{"phase", 2}}), QStringList{"team-1/phase_draft/2"});
    QCOMPARE(TeamWaitingFeed::decidedIds(all, "team-1", "accept", {{"task", 1}}), QStringList{"team-1/report_escalated/1"});
    QCOMPARE(TeamWaitingFeed::decidedIds(all, "team-1", "release", {{"task", 2}}), QStringList{"team-1/task_waits_owner/2"});
    QCOMPARE(TeamWaitingFeed::decidedIds(all, "team-1", "decide-proposal", {{"proposal", "p2"}}), QStringList{p2});
    QVERIFY(TeamWaitingFeed::decidedIds(all, "team-2", "decide-proposal", {{"proposal", "p2"}}).isEmpty());
    QVERIFY(TeamWaitingFeed::decidedIds(all, "team-1", "hold", {{"task", 2}}).isEmpty());
    QVERIFY(TeamWaitingFeed::decidedIds(all, "team-1", "approve-phase", {{"phase", 9}}).isEmpty());
}
