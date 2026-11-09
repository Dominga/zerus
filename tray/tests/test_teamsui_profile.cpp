#include "TeamArchive.h"
#include "TeamTaskCard.h"
// WhiteWh fork, #713: the agent-team UI tests of the pipeline profile and the
// heat tint.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include <QFile>
#include <QWheelEvent>
#include "ActivityView.h"
#include "HgsClient.h"
#include "TeamHeat.h"
#include "TeamProfile.h"
#include "TeamWorkspace.h"

namespace {
// U38 (#665): phase 2 of phasedTeam as `hgs team profile` and `hgs team
// spans` answer it: #3 returned once on backend, #4 waited for #3 and for
// tester, the tester idled while #3 ran.
double profileStart() { return double(QDateTime(QDate::currentDate(), QTime(16, 47)).toSecsSinceEpoch()); }

QJsonObject profileOfPhase2()
{
    const double t0 = profileStart();
    const auto loss = [](const QJsonObject &fields) { return fields; };
    const QJsonArray byTime{
        loss({{"id", "idle_member:tester"}, {"kind", "idle_member"}, {"member", "tester"}, {"role", "Tester"}, {"s", 9000}, {"open_s", 21600}}),
        loss({{"id", "member_wait:4"}, {"kind", "member_wait"}, {"task", 4}, {"member", "tester"}, {"s", 4000}}),
        loss({{"id", "rework:3:a1"}, {"kind", "rework"}, {"task", 3}, {"attempt", 1}, {"member", "backend"}, {"cause", "returned"}, {"s", 3000},
              {"share", 0.45}, {"tokens", 900000}}),
        loss({{"id", "blocked_chain:4"}, {"kind", "blocked_chain"}, {"task", 4}, {"on", QJsonArray{3}}, {"deps_s", 1200}, {"folder_held_s", 0}, {"s", 1200}})};
    const QJsonArray byTokens{byTime[2]};
    return {
        {"team", "team-1"}, {"scope", QJsonObject{{"phase", 2}}},
        {"summary", QJsonObject{{"tasks", 2}, {"accepted", 1}, {"from", t0}, {"to", t0 + 21600}, {"elapsed_s", 21600}, {"worked_s", 15000},
                                {"totals", QJsonObject{{"work", 12000}, {"rework", 3000}, {"member", 4000}, {"deps", 1200}, {"lead", 600}}},
                                {"tokens", QJsonObject{{"total", 2400000}, {"uncached_input", 1600000}, {"output", 800000},
                                                        {"cache_read", 9000000}, {"cache_write", 1000000}}}}},
        {"losses", QJsonObject{{"by_time", QJsonObject{{"items", byTime}, {"next_cursor", QJsonValue()}}},
                               {"by_tokens", QJsonObject{{"items", byTokens}, {"next_cursor", QJsonValue()}}}}},
        {"critical_path", QJsonObject{{"planned", QJsonObject{{"tasks", QJsonArray{3, 4}}, {"length", 2}}},
                                      {"real", QJsonObject{{"tasks", QJsonArray{3, 4}}, {"s", 21600}}}, {"off_plan", QJsonArray()}, {"matches", true}}},
        {"members", QJsonArray{
            QJsonObject{{"member", "backend"}, {"role", "Backend"}, {"model", "gpt-5.5-codex"}, {"busy_s", 15000}, {"idle_s", 3000}, {"utilization", 0.83},
                        {"accepted", 1}, {"tokens", QJsonObject{{"total", 2000000}}}, {"tokens_per_accepted", 2000000}},
            QJsonObject{{"member", "tester"}, {"role", "Tester"}, {"model", "kimi-k2"}, {"busy_s", 2000}, {"idle_s", 9000}, {"utilization", 0.18},
                        {"accepted", 0}, {"tokens", QJsonObject{{"total", 400000}}}}}},
        {"roles", QJsonArray{QJsonObject{{"role", "Backend"}, {"busy_s", 15000}, {"idle_s", 3000}, {"utilization", 0.83}, {"accepted", 1}, {"tokens", 2000000}, {"tokens_per_accepted", 2000000}},
                             QJsonObject{{"role", "Tester"}, {"busy_s", 2000}, {"idle_s", 9000}, {"utilization", 0.18}, {"accepted", 0}, {"tokens", 400000}}}},
        {"models", QJsonArray{QJsonObject{{"model", "gpt-5.5-codex"}, {"busy_s", 15000}, {"idle_s", 3000}, {"utilization", 0.83}, {"accepted", 1}, {"tokens", 2000000}}}},
        {"tasks", QJsonObject{{"items", QJsonArray{
            QJsonObject{{"task", 3}, {"title", "Address form UI"}, {"status", "accepted"}, {"phase", 2}, {"elapsed_s", 15600}, {"worked_s", 14640},
                        {"rework_s", 3000}, {"tokens", QJsonObject{{"total", 2000000}, {"uncached_input", 1400000}, {"output", 600000},
                                                                  {"cache_read", 7000000}, {"cache_write", 800000}}}, {"rework_tokens", 900000}, {"rework_share", 0.45}},
            QJsonObject{{"task", 4}, {"title", "Unit tests"}, {"status", "in_progress"}, {"phase", 2}, {"elapsed_s", 21600}, {"worked_s", 2000},
                        {"rework_s", 0}, {"tokens", QJsonObject{{"total", 400000}, {"uncached_input", 300000}, {"output", 100000},
                                                                {"cache_read", 2000000}, {"cache_write", 200000}}}},
            QJsonObject{{"task", 5}, {"title", "Never handed out"}, {"status", "pending"}, {"phase", 2}, {"elapsed_s", 0}, {"worked_s", 0},
                        {"started", false}, {"tokens", QJsonObject{{"not_started", true}}}}}}, {"next_cursor", QJsonValue()}}},
        {"events", QJsonObject{{"read", 40}, {"truncated", false}}}};
}

QJsonArray spansOfPhase2()
{
    const double t0 = profileStart();
    const auto span = [t0](const QString &kind, double from, double to, const QJsonObject &extra = {}) {
        QJsonObject value{{"kind", kind}, {"from", t0 + from}, {"to", t0 + to}, {"s", to - from}};
        for (auto it = extra.begin(); it != extra.end(); ++it) value.insert(it.key(), it.value());
        return value;
    };
    return {
        QJsonObject{{"task", 3}, {"status", "accepted"}, {"overlay", QJsonArray()},
                    {"spans", QJsonArray{span("delivery", 0, 60, {{"member", "backend"}, {"attempt", 1}}),
                                         span("rework", 60, 3060, {{"member", "backend"}, {"attempt", 1}}),
                                         span("work", 3300, 15000, {{"member", "backend"}, {"attempt", 2}}),
                                         span("lead", 15000, 15600)}}},
        QJsonObject{{"task", 4}, {"status", "in_progress"},
                    {"overlay", QJsonArray{span("build", 20800, 21400, {{"member", "tester"}, {"template", "qt-build"}, {"exit", 0}})}},
                    {"spans", QJsonArray{span("deps", 0, 15600), span("member", 15600, 19600),
                                         span("work", 19600, 21000, {{"member", "tester"}, {"attempt", 1}}),
                                         span("stage", 21000, 21600, {{"member", "tester"}, {"attempt", 1}, {"stage", 1}, {"round", 1}, {"stage_kind", "test"}})}}}};
}

// Task #79: the owner's case (Profile > Time, phase 2): long builds and a merge
// run while #3's work bar shows its title, with every kind of span and wait.
QJsonArray spansOfOwnersCase()
{
    const double t0 = profileStart();
    const auto span = [t0](const QString &kind, double from, double to, const QJsonObject &extra = {}) {
        QJsonObject value{{"kind", kind}, {"from", t0 + from}, {"to", t0 + to}, {"s", to - from}};
        for (auto it = extra.begin(); it != extra.end(); ++it) value.insert(it.key(), it.value());
        return value;
    };
    const QJsonObject backend{{"member", "backend"}, {"attempt", 2}};
    return {
        QJsonObject{{"task", 3}, {"status", "accepted"},
                    {"overlay", QJsonArray{span("build", 4200, 7400, {{"member", "backend"}, {"template", "cargo-build"}, {"exit", 0}}),
                                           span("build", 9000, 12600, {{"member", "backend"}, {"template", "qt-build"}, {"exit", 0}})}},
                    {"spans", QJsonArray{span("delivery", 0, 60, {{"member", "backend"}, {"attempt", 1}}),
                                         span("rework", 60, 3060, {{"member", "backend"}, {"attempt", 1}}),
                                         span("start_delay", 3060, 3300, backend),
                                         span("work", 3300, 13000, backend),
                                         span("question", 13000, 13600, backend),
                                         span("background", 13600, 14200, backend),
                                         span("stage", 14200, 15000, {{"member", "backend"}, {"attempt", 2}, {"stage", 1}, {"round", 1}, {"stage_kind", "review"}}),
                                         span("lead", 15000, 15600)}}},
        QJsonObject{{"task", 4}, {"status", "in_progress"},
                    {"overlay", QJsonArray{span("build", 20800, 21400, {{"member", "tester"}, {"template", "qt-build"}, {"exit", 0}})}},
                    {"spans", QJsonArray{span("deps", 0, 9000), span("folder_held", 9000, 12000), span("hold", 12000, 15600), span("member", 15600, 18600),
                                         span("stage_wait", 18600, 19600),
                                         span("work", 19600, 21000, {{"member", "tester"}, {"attempt", 1}}),
                                         span("stage", 21000, 21600, {{"member", "tester"}, {"attempt", 1}, {"stage", 1}, {"round", 1}, {"stage_kind", "test"}})}}}};
}

QJsonObject compareOf1And2()
{
    const QJsonObject first{{"tasks", 2}, {"accepted", 2}, {"elapsed_s", 9000}, {"worked_s", 8000}, {"totals", QJsonObject{{"work", 8000}, {"member", 600}}},
                            {"tokens", QJsonObject{{"total", 1200000}}}};
    const QJsonObject second = profileOfPhase2().value("summary").toObject();
    return {{"team", "team-1"}, {"compare", QJsonObject{
        {"a", QJsonObject{{"phase", 1}, {"summary", first}, {"top_losses", QJsonArray{QJsonObject{{"id", "member_wait:2"}, {"kind", "member_wait"}, {"task", 2}, {"s", 600}}}}}},
        {"b", QJsonObject{{"phase", 2}, {"summary", second}, {"top_losses", profileOfPhase2()["losses"].toObject()["by_time"].toObject()["items"]}}},
        {"delta", QJsonObject{{"elapsed_s", 12600}, {"worked_s", 7000}, {"tasks", 0}, {"accepted", -1}, {"tokens", 1200000},
                              {"totals", QJsonObject{{"rework", 3000}, {"member", 3400}}}}}}}};
}

TeamProfileView *profileViewOf(QWidget *root)
{
    return static_cast<TeamProfileView *>(root->findChild<QWidget *>("teamProfileView"));
}
}

// U38: phase 38 of the heat mock-up (design/agent-teams/profile, heat and
// heatboard): nine tasks into #482 and the integration #483, its review, test
// and build cancelled.
static QJsonObject heatTeam()
{
    QJsonArray members;
    for (const auto &[id, role, agent] : QList<std::tuple<QString, QString, QString>>{
             {"lead", "Lead", "claude"}, {"backend", "Backend", "dsh"}, {"backend2", "Backend2", "dsh"}, {"backend3", "Backend3", "claude"},
             {"backend4", "Backend4", "dsh"}, {"reviewer", "Reviewer", "claude"}, {"qa", "QA", "dsh"}})
        members.append(QJsonObject{{"id", id}, {"role", role}, {"agent", agent}, {"state", "active"},
                                   {"session", QJsonObject{{"host", ""}, {"name", agent + "/heat/" + id}}}});
    QJsonArray tasks;
    QJsonObject layers;
    const auto add = [&](int id, const QString &title, const QString &who, const QString &status, const QList<int> &deps, int layer) {
        QJsonArray on; for (int d : deps) on.append(QJsonObject{{"task", d}});
        tasks.append(QJsonObject{{"id", id}, {"phase", 38}, {"title", title}, {"assignee", who}, {"status", status}, {"deps", on},
                                 {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", QStringLiteral("a%1").arg(id)}, {"member", who}, {"state", "reported"}}}}});
        layers[QString::number(id)] = layer;
    };
    add(474, "Review fixes: drafts, proposals", "backend", "accepted", {}, 0);
    add(475, "Draft follow-ups: owner edits", "backend", "accepted", {}, 0);
    add(515, "Approve a draft while the previous runs", "backend", "accepted", {}, 0);
    add(476, "MCP servers from plugins", "backend3", "accepted", {}, 0);
    add(480, "Review fixes: limit resume", "backend3", "accepted", {}, 0);
    add(478, "Designer starts trusted", "backend4", "accepted", {}, 0);
    add(477, "Review fixes: sub-team questions", "backend2", "accepted", {}, 0);
    add(479, "Confirm subagents", "backend2", "accepted", {}, 0);
    add(481, "Block session-log upload", "backend2", "accepted", {}, 0);
    add(482, "Settings: confirm subagents", "backend2", "accepted", {479}, 1);
    add(483, "Integrate phase 38", "lead", "accepted", {474, 475, 515, 476, 480, 478, 477, 481, 482}, 2);
    add(484, "Review phase 38", "reviewer", "cancelled", {483}, 3);
    add(485, "Test phase 38", "qa", "pending", {483}, 3);
    add(486, "Build phase 38", "lead", "cancelled", {484, 485}, 4);
    return QJsonObject{{"id", "team-1"}, {"name", "Teams v2: delivery and reports"}, {"status", "running"}, {"project", "Zerus"}, {"folder", "/work/zerus"},
                       {"lead", "lead"}, {"revision", 7}, {"created", 1000}, {"updated", 9000}, {"settings", QJsonObject{}},
                       {"phases", QJsonArray{QJsonObject{{"n", 38}, {"title", "Server fixes and the Sessions settings"}, {"barrier", false}, {"approval", "review"},
                                                          {"state", "approved"}, {"added_by", "Lead"}, {"added_at", 1000.0}, {"decided_by", "You"}, {"decided_at", 1100.0}}}},
                       {"members", members}, {"tasks", tasks}, {"layers", layers}, {"cycle", QJsonArray{}}, {"ready", QJsonArray{}},
                       {"validation", QJsonObject{{"errors", QJsonArray{}}, {"warnings", QJsonArray{}}}}};
}

// Its profile as `hgs team profile` pages it: by time in two pages (an idle
// member's loss has no task), by tokens in one, some tokens unknown.
static QJsonObject heatProfile(int leadWait = 7200)
{
    const auto loss = [](const QString &kind, int task, int s, QJsonObject detail = {}) {
        detail["kind"] = kind; detail["task"] = task; detail["s"] = s; detail["id"] = QStringLiteral("%1:%2").arg(kind).arg(task);
        return detail;
    };
    const QJsonArray time{
        QJsonObject{{"kind", "idle_member"}, {"id", "idle_member:backend4"}, {"s", 9000}, {"member", "backend4"}},
        loss("lead_wait", 483, leadWait, {{"lead_s", leadWait - 1200}, {"question_s", 1200}}),
        loss("blocked_chain", 483, 4560, {{"deps_s", 4560}, {"folder_held_s", 0}, {"on", QJsonArray{474, 482}}, {"held_by", QJsonArray{}}}),
        loss("member_wait", 481, 4320, {{"member", "backend2"}}),
        loss("rework", 482, 3900, {{"attempt", 1}, {"cause", "returned"}, {"member", "backend2"}}),
        loss("rework", 480, 2820, {{"attempt", 1}, {"cause", "returned"}, {"member", "backend3"}}),
        loss("member_wait", 482, 2520, {{"member", "backend2"}}),
        loss("blocked_chain", 515, 2460, {{"deps_s", 0}, {"folder_held_s", 2460}, {"on", QJsonArray{}}, {"held_by", QJsonArray{474}}}),
        loss("member_wait", 477, 2460, {{"member", "backend2"}}),
        loss("conflict", 483, 2400, {{"template", "git-merge-lead"}, {"exit", 1}}),
        loss("lead_wait", 475, 2220, {{"lead_s", 2220}, {"question_s", 0}}),
        loss("long_test", 476, 2100, {{"stage", "test"}, {"round", 1}}),
        loss("member_wait", 479, 2100, {{"member", "backend2"}}),
        loss("permission_wait", 478, 1500, {{"answered_by", QJsonArray{"You"}}}),
        loss("lead_wait", 478, 1320, {{"lead_s", 1320}, {"question_s", 0}}),
        loss("rework", 481, 1200, {{"attempt", 1}, {"cause", "rejected"}, {"member", "backend2"}}),
        loss("lead_wait", 515, 1200, {{"lead_s", 1200}, {"question_s", 0}})};
    const auto page = [](const QString &list, const QJsonArray &items, const QJsonValue &next, bool partial) {
        return QJsonObject{{"summary", QJsonObject{{"tasks", 14}, {"tokens", partial ? QJsonObject{{"total", 1800000}, {"partial", true}} : QJsonObject{{"unknown", true}}}}},
                           {"losses", QJsonObject{{list, QJsonObject{{"items", items}, {"next_cursor", next}, {"total", 17}}}}}};
    };
    // Task 82: the task rows: lateness against the plan and overrun against
    // the estimate. #483 is late by its own 2h 20m and 1h 16m from #482;
    // #475 only inherits 20m from #474; #474 ran on time; #485, a later
    // layer before its planned start, is on time and not started.
    const auto late = [](int start, int inherited, int from) {
        return QJsonObject{{"planned_start", 1000.0}, {"planned_end", 4600.0}, {"start_late_s", start}, {"inherited_s", inherited},
                           {"own_s", start - inherited}, {"end_late_s", QJsonValue()}, {"from", from ? QJsonValue(from) : QJsonValue()},
                           {"actual_start", start > 0 ? QJsonValue(1000.0 + start) : QJsonValue()}, {"actual_end", QJsonValue()}};
    };
    const auto over = [](int estimate, int work) {
        return QJsonObject{{"estimate_s", estimate}, {"work_s", work}, {"overrun_s", work - estimate}, {"ratio", double(work) / estimate}, {"running", false}};
    };
    const QJsonArray rows{
        QJsonObject{{"task", 474}, {"lateness", late(0, 0, 0)}, {"overrun", over(3600, 3600)}},
        QJsonObject{{"task", 475}, {"lateness", late(1200, 1200, 474)}, {"overrun", over(3600, 2880)}},
        QJsonObject{{"task", 482}, {"lateness", late(2400, 0, 0)}, {"overrun", over(1800, 5400)}},
        QJsonObject{{"task", 483}, {"lateness", late(leadWait + 1200 + 4560, 4560, 482)}, {"overrun", over(3600, 6120)}},
        QJsonObject{{"task", 485}, {"lateness", late(0, 0, 0)}, {"overrun", QJsonObject{{"not_started", true}}}}};
    QJsonArray first, second;
    for (int i = 0; i < time.size(); ++i) (i < 9 ? first : second).append(time.at(i));
    const QJsonArray tokens{
        loss("rework", 482, 3900, {{"attempt", 1}, {"cause", "returned"}, {"tokens", 1240000}, {"cost_usd", 3.1}, {"share", .58}}),
        loss("rework", 480, 2820, {{"attempt", 1}, {"cause", "returned"}, {"tokens", 236000}, {"cost_usd", .6}, {"share", .41}}),
        loss("rework", 481, 1200, {{"attempt", 1}, {"cause", "rejected"}, {"tokens", 98000}, {"cost_usd", .25}, {"share", .22}}),
        loss("long_test", 476, 2100, {{"tokens", 61000}, {"cost_usd", .15}})};
    QJsonObject firstPage = page("by_time", first, QJsonObject{{"s", 2460}, {"id", "member_wait:477"}, {"page", 1}}, false);
    firstPage["tasks"] = QJsonObject{{"items", rows}, {"next_cursor", QJsonValue()}};
    return QJsonObject{{"time", QJsonArray{firstPage, page("by_time", second, QJsonValue(), false)}},
                       {"tokens", QJsonArray{page("by_tokens", tokens, QJsonValue(), true)}}};
}

class TestTeamsUiProfile : public QObject, public TeamsUiFixture {
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
    void archivedPhasesAndProfileUseSummaries();
    void archivedReportsLoadOnDemandAndRetry();
    void archivedReportResponsesAreBounded();
    void profileViewShowsThePhaseAndItsLosses();
    void profileTimelineZoomsAndTheSidesFold();
    void profileReadsInBoundedPagesAndKeepsABoundedCache();
    void profileShowsTheTestRunsOfItsWindow();
    void profileComparesEstimateAndActualPerTask();
    void profileOfALargeTeamStaysResponsive();
    void heatTintsGraphAndBoardByTimeOrTokens();
    void aHeatReadThatDrawsTheSameRebuildsNothing();
    void timelineKindsAreDistinctAndBuildsNeverHideTitles();
    void timelineLanesMatchTheTauriCoreOnTheTeamsReplies();
    void waitingForAPersonHasItsOwnKindAndTheLegendWrapsAtEveryWidth();
};

static QJsonObject archivedTeam()
{
    auto value = team();
    const QJsonObject archive{{"tasks", 120}, {"accepted", 118}, {"rejected", 1}, {"cancelled", 1}, {"closed_at", 5000}};
    value["phases"] = QJsonArray{QJsonObject{{"n", 7}, {"title", "Archived checkout"}, {"state", "approved"}, {"archived", archive}}};
    value["tasks"] = QJsonArray{QJsonObject{{"id", 701}, {"phase", 7}, {"title", "Saved task"}, {"status", "accepted"}, {"assignee", "backend"},
        {"archived", QJsonObject{{"accepted_at", 5000}, {"timing", QJsonObject{{"worked_s", 1234}}}}}}};
    return value;
}

void TestTeamsUiProfile::archivedPhasesAndProfileUseSummaries()
{
    const auto value = archivedTeam();
    const auto parsed = Teams::parse(value);
    const auto groups = Teams::phaseGroups(parsed);
    QCOMPARE(groups.size(), 1);
    QVERIFY(groups.first().finished());
    QCOMPARE(groups.first().archivedTotal, 120);
    QVERIFY(Teams::phaseHeading(groups.first()).endsWith("120/120"));
    QCOMPARE(parsed.tasks.first().timing.worked, 1234);
    auto noStubs = parsed; noStubs.tasks.clear();
    QCOMPARE(Teams::phaseGroups(noStubs).size(), 1); // headers survive stub eviction
    HgsClient client(FakeCli::program(script())); TeamController controller(&client);
    TeamProfile::Cache cache(&controller); TeamProfileView profile(&controller, &cache);
    profile.setTeam(parsed); profile.setPhase(7); profile.show();
    QVERIFY(profile.findChild<QPushButton *>("teamProfileArchivedTask"));
    QCOMPARE(cache.reads(), 0); // no fabricated timeline from the stub
    auto headerOnly = value; headerOnly["tasks"] = QJsonArray();
    setTeam(headerOnly);
    TeamPanel panel(&controller); TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1400, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QLabel *>("teamPhaseArchivedSummary"));
    QVERIFY(workspace.findChild<QLabel *>("teamPhaseArchivedSummary")->text().contains("120 tasks"));
    workspace.hide();

    int opened = 0; profile.openTask = [&opened](int task) { opened = task; };
    profile.findChild<QPushButton *>("teamProfileArchivedTask")->click(); QCOMPARE(opened, 701);

    QJsonArray headers, tasks;
    for (int n = 1; n <= 50; ++n) {
        headers.append(QJsonObject{{"n", n}, {"title", QString("Phase %1").arg(n)}, {"archived", QJsonObject{{"tasks", 120}, {"accepted", 120}}}});
        tasks.append(QJsonObject{{"id", n}, {"title", QString("Task %1").arg(n)}, {"status", "accepted"},
            {"archived", QJsonObject{{"accepted_at", 5000}, {"timing", QJsonObject{{"worked_s", 1234}}}}}});
    }
    const auto pages = [](const QJsonArray &items) {
        return QJsonDocument(QJsonObject{{"0", QJsonObject{{"items", items}, {"next_cursor", 50}}},
            {"50", QJsonObject{{"items", QJsonArray{items.first()}}, {"next_cursor", QJsonValue()}}}}).toJson();
    };
    QVERIFY(FakeCli::replace(script()+".archivephases", pages(headers)));
    QVERIFY(FakeCli::replace(script()+".archivetasks", pages(tasks)));
    auto *archive = new TeamArchive(&controller, parsed); archive->show(); QPointer<TeamArchive> guard(archive);
    auto *list = archive->findChild<QListWidget *>("teamArchivePhases");
    auto *table = archive->findChild<QTableWidget *>("teamArchiveTasks");
    QTRY_COMPARE(list->count(), 50);
    list->setCurrentRow(0); QTRY_COMPARE(table->rowCount(), 50);
    QCOMPARE(table->item(0, 3)->text(), TeamProfile::duration(1234));
    archive->findChild<QPushButton *>("teamArchiveNextTasks")->click();
    QTRY_COMPARE(table->rowCount(), 1); // replace, never accumulate
    archive->findChild<QPushButton *>("teamArchiveNextPhases")->click();
    QTRY_COMPARE(list->count(), 1);
    int archiveReads = 0;
    for (const auto &input : calls(script(), "status")) {
        if (!input.contains("archived_phase") && !input.contains("archived_phases")) continue;
        ++archiveReads;
        QCOMPARE(input.value("limit").toInt(), 50);
        QVERIFY(input.contains("archived_phase") || input.contains("archived_phases"));
    }
    QVERIFY(archiveReads >= 4);
    archive->close(); QTRY_VERIFY(!guard);
    // A phase link seeks directly, even when thousands of earlier phases exist.
    const QJsonObject directPage{{"6", QJsonObject{{"items", QJsonArray{value.value("phases").toArray().first()}}, {"next_cursor", QJsonValue()}}}};
    QVERIFY(FakeCli::replace(script()+".archivephases", QJsonDocument(directPage).toJson()));
    auto *direct = new TeamArchive(&controller, parsed, nullptr, 7); direct->show();
    QPointer<TeamArchive> directGuard(direct);
    QTRY_COMPARE(direct->findChild<QTableWidget *>("teamArchiveTasks")->rowCount(), 50);
    QVERIFY(calls(script(), "status").last().value("archived_phase").toInt() == 7);
    direct->close(); QTRY_VERIFY(!directGuard);

}

void TestTeamsUiProfile::archivedReportsLoadOnDemandAndRetry()
{
    const auto parsed = Teams::parse(archivedTeam());
    auto full = parsed.tasks.first().json();
    full["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"state", "reported"}, {"member", "backend"}, {"report", QJsonObject{{"text", "Saved full report"}, {"at", 5000}}}}};
    QVERIFY(FakeCli::replace(script()+".task", QJsonDocument(full).toJson()));
    QVERIFY(FakeCli::replace(script()+".taskhold", "hold"));
    HgsClient client(FakeCli::program(script())); TeamController controller(&client);
    TeamTaskCard card; card.setController(&controller); card.show();
    QCOMPARE(calls(script(), "status").size(), 0);
    card.setTask(parsed, 701, {}, true);
    QVERIFY(card.findChild<QLabel *>("teamTaskHistoryState"));
    int ticks = 0; QTimer timer; timer.setInterval(1); connect(&timer, &QTimer::timeout, [&ticks] { ++ticks; }); timer.start();
    QTRY_VERIFY(ticks > 5); // the held CLI leaves the GUI event loop free
    card.setTask(parsed, 701, {}, true); // polling must not issue another read
    QFile::remove(script()+".taskhold");
    QTRY_VERIFY(!card.findChild<QLabel *>("teamTaskHistoryState"));
    QCOMPARE(calls(script(), "status").size(), 1);
    QCOMPARE(calls(script(), "status").first().value("task").toInt(), 701);
    QVERIFY(!calls(script(), "status").first().contains("request_id"));
    QVERIFY(card.findChild<QLabel *>("teamReportText"));
    QCOMPARE(card.findChild<QLabel *>("teamReportText")->text(), QString("Saved full report"));
    // An explicit restore to live work must replace the archived detail.
    auto restored = parsed; restored.tasks.first().archived = false;
    restored.tasks.first().status = "in_progress";
    card.setTask(restored, 701, {}, true);
    QVERIFY(!card.findChild<QLabel *>("teamReportText"));
    // Selecting another team with the same task id releases the prior result.
    auto other = parsed; other.id = "team-2";
    QVERIFY(FakeCli::replace(script()+".taskrefuse", "refuse"));
    card.setTask(other, 701, {}, true);
    QTRY_VERIFY(card.findChild<QPushButton *>("teamTaskHistoryRetry"));
    QVERIFY(card.findChild<QLabel *>("teamTaskHistoryState")->text().contains("Archive unavailable"));
    QFile::remove(script()+".taskrefuse");
    card.findChild<QPushButton *>("teamTaskHistoryRetry")->click();
    QTRY_VERIFY(!card.findChild<QLabel *>("teamTaskHistoryState"));
}

void TestTeamsUiProfile::archivedReportResponsesAreBounded()
{
    QByteArray oversized(8 * 1024 * 1024 + 1, 'x');
    QVERIFY(FakeCli::replace(script()+".task", oversized));
    HgsClient client(FakeCli::program(script())); TeamController controller(&client);
    TeamTaskCard card; card.setController(&controller); card.setTask(Teams::parse(archivedTeam()), 701, {}, true);
    QTRY_VERIFY(card.findChild<QPushButton *>("teamTaskHistoryRetry"));
    QVERIFY(!card.findChild<QLabel *>("teamTaskHistoryState")->text().isEmpty());
}

// U38 (#665): Profile beside Graph, Board and Layers, from hgs team profile
// and hgs team spans: the summary, the member lanes with the real and planned
// critical paths, the top losses (a click opens the task and marks it), the
// tokens per task, the members table, the phase compare and Export JSON.
void TestTeamsUiProfile::profileViewShowsThePhaseAndItsLosses()
{
    setTeam(phasedTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profileOfPhase2()).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(script() + ".spans", QJsonDocument(spansOfPhase2()).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(script() + ".compare", QJsonDocument(compareOf1And2()).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *profile = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *b : workspace.findChildren<QPushButton *>()) if (b->property("segment") == "profile" && b->isVisible()) profile = b;
        return profile != nullptr;
    })());
    profile->click();
    TeamProfileView *view = nullptr;
    QTRY_VERIFY((view = profileViewOf(&workspace)) && view->isVisible());
    QCOMPARE(view->phase(), 2);
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamProfileTimeline"));
    // The reads: the profile of phase 2, then its spans.
    QTRY_VERIFY(!calls(script(), "profile").isEmpty() && !calls(script(), "spans").isEmpty());
    QCOMPARE(calls(script(), "profile").first()["phase"].toInt(), 2);
    QVERIFY(!calls(script(), "profile").first().contains("request_id"));   // a read, not a change
    // The summary tiles.
    const auto tile = [&](const char *name) { auto *found = workspace.findChild<QLabel *>(name); return found ? found->text() : QString(); };
    QCOMPARE(tile("teamProfileTook"), QStringLiteral("6h 00m"));
    QCOMPARE(tile("teamProfileWorked"), QStringLiteral("4h 10m"));
    QCOMPARE(tile("teamProfileRework"), QStringLiteral("1 return"));
    QCOMPARE(tile("teamProfileMemberWait"), QStringLiteral("1h 06m"));
    QCOMPARE(tile("teamProfileAccepted"), QStringLiteral("1 of 2"));
    // The lanes: backend and tester; the real and planned paths.
    const auto &model = view->model();
    QCOMPARE(model.lanes.size(), 2);
    QCOMPARE(model.lanes[0].member, QStringLiteral("backend"));
    QCOMPARE(model.lanes[0].spans.size(), 2);   // the returned attempt and the accepted one
    QCOMPARE(model.lanes[1].member, QStringLiteral("tester"));
    QCOMPARE(model.lanes[1].waits.size(), 2);   // #4 waited for #3 and for tester
    QVERIFY(!model.lanes[1].idle.isEmpty());    // tester idled while #3 ran
    QCOMPARE(model.real.size(), 2); QCOMPARE(model.real[0].task, 3);
    QCOMPARE(model.planned.size(), 2);
    QCOMPARE(model.plannedSeconds, qint64(3000 + 1400 + 600));   // each task's first attempt
    // The timeline paints: no widget per span.
    QVERIFY(workspace.findChild<QWidget *>("teamProfileTimeline")->findChildren<QWidget *>().isEmpty());
    // The losses, by time: the idle tester first; a chip opens the task and marks it.
    QStringList kinds;
    for (auto *card : workspace.findChildren<QFrame *>("teamProfileLoss")) kinds << card->property("kind").toString();
    QCOMPARE(kinds, (QStringList{"idle_member", "member_wait", "rework", "blocked_chain"}));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u38-profile-time");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u38-profile-time-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // By tokens: the rework of #3.
    view->setLossOrder("tokens");
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamProfileLoss").size(), 1);
    QCOMPARE(workspace.findChild<QLabel *>("teamProfileLossValue")->text(), QStringLiteral("900k"));
    view->setLossOrder("time");
    // Tokens per task.
    view->setTab("tokens");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamProfileTokens"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u38-profile-tokens");
        workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-tokens-light"); darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // Members: one row each, then the role and model groups.
    view->setTab("members");
    QTableWidget *members = nullptr;
    QTRY_VERIFY((members = workspace.findChild<QTableWidget *>("teamProfileMembers")));
    QCOMPARE(members->rowCount(), 2 + 2 + 1);
    QCOMPARE(members->item(0, 0)->text(), QStringLiteral("backend"));
    QCOMPARE(members->item(0, 3)->text(), QStringLiteral("2"));   // attempts
    QCOMPARE(members->item(0, 4)->text(), QStringLiteral("1"));   // returned
    QCOMPARE(members->item(0, 6)->text(), QStringLiteral("83%"));
    QCOMPARE(members->item(0, 9)->text(), QStringLiteral("2.00M"));
    QCOMPARE(members->item(2, 0)->text(), QStringLiteral("Role Backend"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u38-profile-members");
        workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-members-light"); darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // Compare: phases 1 and 2 side by side.
    view->setTab("compare");
    QTRY_VERIFY(!calls(script(), "profile").isEmpty() && calls(script(), "profile").last().contains("compare"));
    QCOMPARE(calls(script(), "profile").last()["compare"].toArray(), (QJsonArray{1, 2}));
    QTRY_VERIFY(!workspace.findChildren<QLabel *>("teamProfileCompareDelta").isEmpty());
    QStringList deltas;
    for (auto *delta : workspace.findChildren<QLabel *>("teamProfileCompareDelta")) deltas << delta->text();
    QVERIFY2(deltas.contains("+3h 30m") && deltas.contains("+1.20M"), qPrintable(deltas.join(", ")));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&workspace, "u38-profile-compare");
        workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-compare-light"); darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // A loss's task chip opens the task's card and marks it in the timeline.
    view->setTab("time");
    QPushButton *chip = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *card : workspace.findChildren<QFrame *>("teamProfileLoss"))
            if (card->property("kind") == "rework") chip = card->findChild<QPushButton *>("teamProfileLossTask");
        return chip != nullptr;
    })());
    QCOMPARE(chip->property("task").toInt(), 3);
    chip->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDrawer") && workspace.findChild<QWidget *>("teamWorkspaceDrawer")->isVisible());
    QCOMPARE(view->selectedTask(), 3);
    // Export JSON: the profile and its spans.
    view->setTab("time");
    const QString path = m_dir.filePath("profile-export.json");
    view->exportPath = [path] { return path; };
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamProfileExport"));
    workspace.findChild<QPushButton *>("teamProfileExport")->click();
    QFile exported(path); QVERIFY(exported.open(QIODevice::ReadOnly));
    const auto json = QJsonDocument::fromJson(exported.readAll()).object();
    QCOMPARE(json["profile"].toObject()["summary"].toObject()["elapsed_s"].toInt(), 21600);
    QCOMPARE(json["spans"].toArray().size(), 2);
    // Back to Graph: the view leaves the canvas and stays for the next visit.
    for (auto *b : workspace.findChildren<QPushButton *>()) if (b->property("segment") == "graph" && b->isVisible()) { b->click(); break; }
    QTRY_VERIFY(!view->isVisible());
    QVERIFY(profileViewOf(&workspace) == view);
}

// U38 (#665): the spans are read in pages of 50, at most 20 pages; a scope
// past that is named, never read whole. At most six scopes stay cached.
void TestTeamsUiProfile::profileReadsInBoundedPagesAndKeepsABoundedCache()
{
    setTeam(phasedTeam());
    QJsonArray many;
    const double t0 = profileStart();
    for (int id = 1; id <= 1300; ++id)
        many.append(QJsonObject{{"task", id}, {"status", "accepted"}, {"overlay", QJsonArray()},
                                {"spans", QJsonArray{QJsonObject{{"kind", "work"}, {"from", t0 + id}, {"to", t0 + id + 600}, {"s", 600}, {"member", "backend"}, {"attempt", 1}}}}});
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profileOfPhase2()).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(script() + ".spans", QJsonDocument(many).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamProfile::Cache cache(&controller);
    QSignalSpy changed(&cache, &TeamProfile::Cache::changed);
    const auto *entry = cache.entry("team-1", "plan", 1);
    QVERIFY(entry->loading);
    QTRY_VERIFY_WITH_TIMEOUT(cache.entry("team-1", "plan", 1)->ready, 60000);
    entry = cache.entry("team-1", "plan", 1);
    QCOMPARE(entry->timelines.size(), TeamProfile::Cache::MaxSpanPages * TeamProfile::Cache::SpanPage);
    QVERIFY(entry->truncated);
    QCOMPARE(calls(script(), "spans").size(), TeamProfile::Cache::MaxSpanPages);
    for (const auto &call : calls(script(), "spans")) QCOMPARE(call["limit"].toInt(), 50);
    // Read again only when the team changed and the entry is old: the same revision reads nothing.
    const int reads = cache.reads();
    cache.entry("team-1", "plan", 1);
    QCOMPARE(cache.reads(), reads);
    // At most six scopes.
    for (int phase = 1; phase <= 9; ++phase) cache.entry("team-1", QStringLiteral("phase:%1").arg(phase), 1);
    QCOMPARE(cache.size(), TeamProfile::Cache::MaxEntries);
}

// U38 (#665): a large team - 600 tasks over 60 members, 6000 spans - builds
// and paints in a bounded time; the timeline is one painted widget.
void TestTeamsUiProfile::profileOfALargeTeamStaysResponsive()
{
    auto big = phasedTeam();
    QJsonArray members, tasks, timelines, rows;
    const double t0 = profileStart();
    for (int m = 0; m < 60; ++m) {
        const QString id = QStringLiteral("member-%1").arg(m);
        members.append(QJsonObject{{"id", id}, {"role", QStringLiteral("Role %1").arg(m % 6)}, {"agent", "codex"}, {"state", "active"},
                                   {"session", QJsonObject{{"host", ""}, {"name", "codex/big/" + id}}}});
        rows.append(QJsonObject{{"member", id}, {"role", QStringLiteral("Role %1").arg(m % 6)}, {"model", "gpt-5.5"}, {"busy_s", 20000}, {"idle_s", 4000},
                                {"utilization", 0.83}, {"accepted", 10}, {"tokens", QJsonObject{{"total", 1000000}}}, {"tokens_per_accepted", 100000}});
    }
    big["members"] = members;
    for (int id = 1; id <= 600; ++id) {
        const QString member = QStringLiteral("member-%1").arg(id % 60);
        tasks.append(QJsonObject{{"id", id}, {"title", QStringLiteral("Task %1").arg(id)}, {"assignee", member}, {"status", "accepted"}, {"phase", 2}});
        QJsonArray spans;
        for (int k = 0; k < 10; ++k) {
            const double from = t0 + id * 60 + k * 300;
            spans.append(QJsonObject{{"kind", k % 3 == 0 ? "member" : k % 4 == 1 ? "rework" : "work"}, {"from", from}, {"to", from + 280}, {"s", 280},
                                     {"member", k % 3 == 0 ? QJsonValue() : QJsonValue(member)}, {"attempt", 1 + k / 5}});
        }
        timelines.append(QJsonObject{{"task", id}, {"status", "accepted"}, {"spans", spans}, {"overlay", QJsonArray()}});
    }
    big["tasks"] = tasks;
    auto profile = profileOfPhase2();
    profile["members"] = rows;
    auto summary = profile["summary"].toObject(); summary["to"] = t0 + 600 * 60 + 3000; profile["summary"] = summary;
    const auto team = Teams::parse(big);
    QElapsedTimer clock; clock.start();
    const auto model = TeamProfile::Model::build(team, profile, timelines, t0 + 50000);
    const qint64 built = clock.elapsed();
    QCOMPARE(model.lanes.size(), 60);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamProfile::Cache cache(&controller);
    TeamProfileView view(&controller, &cache);
    view.resize(1600, 980); view.show();
    clock.restart();
    view.showModel(model);
    view.grab();
    const qint64 painted = clock.elapsed();
    QVERIFY2(built < 1500 && painted < 1500, qPrintable(QStringLiteral("built %1 ms, painted %2 ms").arg(built).arg(painted)));
    auto *timeline = view.findChild<QWidget *>("teamProfileTimeline");
    QVERIFY(timeline && timeline->findChildren<QWidget *>().isEmpty());
    // The losses list stays a page, never one card per task.
    QVERIFY(view.findChildren<QFrame *>("teamProfileLoss").size() <= TeamProfile::Cache::LossPage);
    // The other tabs build just as fast.
    for (const char *tab : {"tokens", "members", "time"}) {
        clock.restart(); view.setTab(QLatin1String(tab)); view.grab();
        QVERIFY2(clock.elapsed() < 1500, tab);
    }
}

void TestTeamsUiProfile::heatTintsGraphAndBoardByTimeOrTokens()
{
    // Off by default: no remembered choice for this team or another one.
    Settings::remove("teams/heat/team-1"); Settings::remove("teams/heat/team-2");
    const auto forget = qScopeGuard([this] {
        Settings::remove("teams/heat/team-1"); Settings::remove("teams/heat/team-2");
        QFile::remove(script() + ".profilerefuse");
    });
    setTeam(heatTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(heatProfile()).toJson(QJsonDocument::Compact)));
    QFile::remove(script() + ".profilerefuse");
    const int profileCalls = calls(script(), "profile").size();
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.setShowCancelled(true);
    TeamPanel panel(&controller);
    auto workspace = std::make_unique<TeamWorkspace>(&controller, &panel);
    panel.setLive(live); workspace->setLive(live);
    darkPalette(); workspace->setTheme(true);
    workspace->resize(2140, 900); workspace->show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace->showTeam("team-1");
    QTRY_VERIFY(phaseScrollOf(workspace.get(), 38));
    if (!phaseOpen(workspace.get(), 38)) togglePhase(workspace.get(), 38);
    QTRY_VERIFY(phaseOpen(workspace.get(), 38));
    const auto heatButton = [&](TeamWorkspace *w, const char *value) -> QPushButton * {
        auto *box = w->findChild<QWidget *>("teamWorkspaceHeat");
        if (!box) return nullptr;
        for (auto *b : box->findChildren<QPushButton *>()) if (b->property("segment") == value) return b;
        return nullptr;
    };
    const auto node = [&](int id) {
        for (auto *widget : workspace->findChildren<QWidget *>("teamPhaseGraph"))
            if (auto *view = dynamic_cast<TeamUi::TaskView *>(widget))
                for (const auto &n : view->nodes) if (n.id == id) return n;
        return TeamUi::TaskView::Node{};
    };
    QTRY_VERIFY(node(483).id == 483);
    // Off: the switch is there, nothing is tinted and no profile is read.
    QTRY_VERIFY(heatButton(workspace.get(), "off"));
    QVERIFY(heatButton(workspace.get(), "off")->isChecked());
    // #755: the choices are icons alone, named with their purpose in their
    // tooltips and accessible names.
    QTRY_VERIFY(heatButton(workspace.get(), "time") && heatButton(workspace.get(), "time")->text().isEmpty());
    for (const auto &[value, name] : QList<std::pair<const char *, QString>>{{"off", "Off"}, {"time", "Lateness"}, {"tokens", "Tokens"}}) {
        auto *choice = heatButton(workspace.get(), value);
        QVERIFY2(choice->text().isEmpty() && !choice->icon().isNull(), value);
        QVERIFY2(choice->toolTip().startsWith(name + QLatin1Char('\n')), qPrintable(choice->toolTip()));
        QCOMPARE(choice->accessibleName(), QStringLiteral("Heat: ") + name);
    }
    QVERIFY(!workspace->findChild<QWidget *>("teamHeatLegend"));
    QVERIFY(node(483).heat.isEmpty() && !node(483).heatFill.isValid());
    QCOMPARE(calls(script(), "profile").size(), profileCalls);
    // Lateness: the plan's losses by time and the task rows in two pages, without the
    // sessions' tokens; the choice is this team's.
    heatButton(workspace.get(), "time")->click();
    QTRY_COMPARE(calls(script(), "profile").size(), profileCalls + 2);
    const auto asked = calls(script(), "profile").mid(profileCalls);
    QCOMPARE(asked.at(0)["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(asked.at(0)["tokens"].toBool(), false);
    QCOMPARE(asked.at(0)["limit"].toInt(), 100);
    QVERIFY(!asked.at(0).contains("phase") && !asked.at(0).contains("after_time"));
    QCOMPARE(asked.at(1)["after_time"].toObject()["id"].toString(), QStringLiteral("member_wait:477"));
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("time"));
    QCOMPARE(TeamUi::heatMode("team-2"), QStringLiteral("off"));
    // Task 82: Lateness is measured against the plan, not every wait.
    QTRY_COMPARE(node(483).heat, QStringLiteral("late 2h 20m (1h 16m from #482)"));
    QVERIFY(asked.at(0)["after_task"].isUndefined() && asked.at(1)["after_task"].isUndefined());
    const auto c = TeamUi::colorsFor(true);
    // Its own lateness is over 2 h: the warmest tint, the text in the heat
    // color; the overrun is its own badge beside it, violet past x1.5.
    QCOMPARE(node(483).heatFill, TeamUi::heatFill(c, 1));
    QCOMPARE(node(483).heatColor, c.danger);
    QCOMPARE(node(483).heatBadge, QStringLiteral("x1.7"));
    QCOMPARE(node(482).heat, QStringLiteral("late 40m"));
    QVERIFY(node(482).heatFill.isValid() && node(482).heatFill != node(483).heatFill);
    QCOMPARE(node(482).heatBadge, QStringLiteral("x3.0"));
    QCOMPARE(node(482).heatBadgeColor, c.violet);
    QVERIFY(node(483).heatBadgeColor != c.violet && node(483).heatBadgeColor != c.muted);
    // Waiting on a predecessor that ran late: muted, "from #474", no tint.
    QCOMPARE(node(475).heat, QStringLiteral("late 20m from #474"));
    QVERIFY(!node(475).heatFill.isValid());
    QCOMPARE(node(475).heatColor, c.dim);
    QCOMPARE(node(475).heatBadge, QStringLiteral("x0.8"));
    QCOMPARE(node(475).heatBadgeColor, c.muted);
    QCOMPARE(node(474).heat, QStringLiteral("on time"));
    QVERIFY(!node(474).heatFill.isValid());
    // The owner's case: a later-layer task before its planned start is cold,
    // and an unstarted task has no overrun badge.
    QCOMPARE(node(485).heat, QStringLiteral("on time"));
    QVERIFY(!node(485).heatFill.isValid());
    QVERIFY(node(485).heatBadge.isEmpty());
    // A task with losses but no plan baseline: no lateness to show.
    QVERIFY(node(515).heat.isEmpty() && !node(515).heatFill.isValid());
    // Cancelled work and the node's own lines: no tint; the working time
    // gives way to the lateness.
    QVERIFY(node(484).heat.isEmpty() && !node(484).heatFill.isValid() && node(484).heatBadge.isEmpty());
    QVERIFY(node(483).worked.isEmpty());
    // The tooltip: planned and actual moments, own and inherited lateness,
    // the overrun and the causes.
    const QString tip = node(483).tooltip;
    QVERIFY2(tip.contains("Planned: start ") && tip.contains("Actual: start "), qPrintable(tip));
    QVERIFY2(tip.contains("Late 3h 36m: own 2h 20m, 1h 16m from #482 (it ran late)"), qPrintable(tip));
    QVERIFY2(tip.contains("Overrun: own work 1h 42m against a 1h 00m estimate (x1.7)"), qPrintable(tip));
    QVERIFY2(tip.contains("Causes: Waited for the lead (decision 1h 40m, questions 20m) 2h 00m; Waited for dependencies #474, #482 1h 16m; Merge conflict 40m"), qPrintable(tip));
    QVERIFY2(node(474).tooltip.contains("On schedule"), qPrintable(node(474).tooltip));
    QVERIFY2(node(485).tooltip.contains("Actual: start not yet, end not yet") && node(485).tooltip.contains("Overrun: not started"), qPrintable(node(485).tooltip));
    // The legend explains both measures: the lateness scale and the overrun's.
    auto *legend = workspace->findChild<QWidget *>("teamHeatLegend");
    QVERIFY(legend);
    QCOMPARE(legend->findChild<QLabel *>("teamHeatMetric")->text(), QStringLiteral("Late against the plan: own lateness tints, inherited is muted (from #N)"));
    QCOMPARE(legend->findChild<QLabel *>("teamHeatEnd")->text(), QStringLiteral("2h or more late"));
    QCOMPARE(legend->findChild<QLabel *>("teamHeatOverrunLegend")->text(), QStringLiteral("Overrun, own work against the estimate:"));
    QCOMPARE(legend->findChildren<QLabel *>("teamHeatOverrunSample").size(), 3);
    QVERIFY(legend->findChild<QWidget *>("teamHeatScale"));
    QCOMPARE(legend->findChild<QLabel *>("teamHeatNote")->text(), QStringLiteral("Tint only; the state icon and border stay."));
    const auto shoot = [&](const QString &name) {
        if (shotsWanted()) {   // #713: the light half of the shots
            shot(workspace.get(), name + "-dark");
            lightPalette(); workspace->setTheme(false); workspace->render(); QTest::qWait(200);
            lightShot(workspace.get(), name + "-light");
            darkPalette(); workspace->setTheme(true); workspace->render(); QTest::qWait(150);
        }
    };
    shoot("u38-heat-graph-time");
    // The Board: the same tint on the cards and the loss at the line's end.
    const auto view = [&](const char *segment) {
        for (auto *b : workspace->findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == segment) { b->click(); break; }
    };
    const auto card = [&](int id) -> QFrame * {
        for (auto *frame : workspace->findChildren<QFrame *>("teamBoardCard")) if (frame->property("task").toInt() == id) return frame;
        return nullptr;
    };
    const auto lost = [&](int id) -> QLabel * { auto *frame = card(id); return frame ? frame->findChild<QLabel *>("teamBoardHeat") : nullptr; };
    view("board");
    QTRY_VERIFY(lost(483));
    QVERIFY(heatButton(workspace.get(), "time")->isChecked());
    QCOMPARE(lost(483)->text(), QStringLiteral("late 2h 20m (1h 16m from #482)"));
    QVERIFY(lost(483)->font().bold());
    QVERIFY(card(483)->styleSheet().contains(TeamUi::heatFill(c, 1).name()));
    QVERIFY(card(474)->styleSheet().contains("background:" + c.surface.name()));
    QCOMPARE(lost(474)->text(), QStringLiteral("on time"));
    // Task 82: the overrun badge sits beside the lateness, in words.
    const auto badge = [&](int id) -> QLabel * { auto *frame = card(id); return frame ? frame->findChild<QLabel *>("teamHeatOverrun") : nullptr; };
    QVERIFY(badge(483));
    QCOMPARE(badge(483)->text(), QStringLiteral("+42m over 1h 00m"));
    QCOMPARE(badge(475)->text(), QStringLiteral("within 1h 00m"));
    // The badge never covers the lateness: beside it, or under it on a
    // card too narrow for both.
    QTRY_VERIFY(badge(483) && lost(483) && badge(483)->isVisible() && !badge(483)->geometry().intersects(lost(483)->geometry())
                && (badge(483)->geometry().left() > lost(483)->geometry().right() || badge(483)->geometry().top() > lost(483)->geometry().bottom()));
    QCOMPARE(lost(475)->text(), QStringLiteral("late 20m from #474"));
    QVERIFY(card(475)->styleSheet().contains("background:" + c.surface.name()));
    QCOMPARE(lost(485)->text(), QStringLiteral("on time"));
    QVERIFY(card(485)->styleSheet().contains("background:" + c.surface.name()));
    QVERIFY(!badge(485));
    QVERIFY(!lost(484));
    QVERIFY(!card(483)->findChild<QLabel *>("teamBoardWorked"));
    QVERIFY(card(483)->toolTip().contains("Causes: Waited for the lead"));
    QTRY_VERIFY(([&] {
        auto *heat = lost(483);
        auto *role = card(483) ? card(483)->findChild<QWidget *>("teamBoardRole") : nullptr;
        // Task 82: the lateness has its own line under the role's.
        return heat && role && heat->geometry().top() > role->geometry().bottom();
    })());
    shoot("u38-heat-board-time");
    // Task 82: Layers takes the tint, the lateness and the badge too.
    view("layers");
    const auto layerRow = [&](int id) -> QFrame * {
        for (auto *frame : workspace->findChildren<QFrame *>("teamLayerRow")) if (frame->property("task").toInt() == id) return frame;
        return nullptr;
    };
    QTRY_VERIFY(layerRow(483) && layerRow(483)->findChild<QLabel *>("teamBoardHeat"));
    QVERIFY(workspace->findChild<QWidget *>("teamHeatLegend"));
    QCOMPARE(layerRow(483)->findChild<QLabel *>("teamBoardHeat")->text(), QStringLiteral("late 2h 20m (1h 16m from #482)"));
    QCOMPARE(layerRow(483)->findChild<QLabel *>("teamHeatOverrun")->text(), QStringLiteral("+42m over 1h 00m"));
    QVERIFY(layerRow(483)->styleSheet().contains(TeamUi::heatFill(c, 1).name()));
    QVERIFY(layerRow(485)->styleSheet().contains("background:" + c.surface.name()));
    shoot("t82-heat-layers-time");
    // The task card shows the same two measures.
    for (auto *frame : workspace->findChildren<QFrame *>("teamLayerRow"))
        if (frame->property("task").toInt() == 483) { QTest::mouseClick(frame, Qt::LeftButton, {}, QPoint(40, 10)); break; }
    QTRY_VERIFY(workspace->findChild<QWidget *>("teamCardHeat"));
    auto *cardHeat = workspace->findChild<QWidget *>("teamCardHeat");
    QCOMPARE(cardHeat->findChild<QLabel *>("teamBoardHeat")->text(), QStringLiteral("late 2h 20m (1h 16m from #482)"));
    QCOMPARE(cardHeat->findChild<QLabel *>("teamHeatOverrun")->text(), QStringLiteral("+42m over 1h 00m"));
    shoot("t82-heat-card-time");
    view("board");
    QTRY_VERIFY(lost(483));
    // Tokens: the losses by tokens, read with the sessions' tokens; the scale
    // ends at the largest task's and the unknown tokens are named.
    heatButton(workspace.get(), "tokens")->click();
    QTRY_VERIFY(!calls(script(), "profile").isEmpty() && calls(script(), "profile").last()["tokens"].toBool());
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("tokens"));
    QTRY_COMPARE(lost(482) ? lost(482)->text() : QString(), QStringLiteral("lost 1.2M tokens"));
    QCOMPARE(lost(480)->text(), QStringLiteral("lost 236k tokens"));
    QCOMPARE(lost(483)->text(), QStringLiteral("no loss"));
    QVERIFY(!badge(483));
    QVERIFY(card(482)->styleSheet().contains(TeamUi::heatFill(c, 1).name()));
    QVERIFY2(lost(482)->toolTip().contains("Lost 1.2M tokens to rework and long tests\nTop loss: Rework: attempt 1 returned, 1.2M tokens, 58% of the task's tokens"),
             qPrintable(lost(482)->toolTip()));
    legend = workspace->findChild<QWidget *>("teamHeatLegend");
    QCOMPARE(legend->findChild<QLabel *>("teamHeatMetric")->text(), QStringLiteral("Tokens lost: rework and long tests"));
    QCOMPARE(legend->findChild<QLabel *>("teamHeatEnd")->text(), QStringLiteral("1.2M, the most"));
    QVERIFY(legend->findChild<QLabel *>("teamHeatNote")->text().contains("Some tokens are unknown."));
    shoot("u38-heat-board-tokens");
    view("graph");
    QTRY_COMPARE(node(482).heat, QStringLiteral("lost 1.2M tokens"));
    QCOMPARE(node(476).heat, QStringLiteral("lost 61k tokens"));
    shoot("u38-heat-graph-tokens");
    // Tests has no tint and no switch; the choice stays for the views.
    view("tests");
    QTRY_VERIFY(!workspace->findChild<QWidget *>("teamWorkspaceHeat"));
    QVERIFY(!workspace->findChild<QWidget *>("teamHeatLegend"));
    view("graph");
    QTRY_VERIFY(heatButton(workspace.get(), "tokens") && heatButton(workspace.get(), "tokens")->isChecked());
    // Remembered per team: a new workspace opens team-1 with Tokens; another
    // team is still off.
    workspace.reset();
    auto again = std::make_unique<TeamWorkspace>(&controller, &panel);
    again->setLive(live); again->setTheme(true); again->resize(2140, 900); again->show();
    again->showTeam("team-1");
    QTRY_VERIFY(heatButton(again.get(), "tokens"));
    QVERIFY(heatButton(again.get(), "tokens")->isChecked());
    QCOMPARE(TeamUi::heatMode("team-2"), QStringLiteral("off"));
    // A refused read names its reason in the legend and tints nothing.
    QVERIFY(FakeCli::replace(script() + ".profilerefuse", "usage: hgs [@host] team create|plan|status --json"));
    heatButton(again.get(), "time")->click();
    QTRY_VERIFY(again->findChild<QLabel *>("teamHeatError"));
    QVERIFY2(again->findChild<QLabel *>("teamHeatError")->text().startsWith("The profile could not be read: ")
                 && again->findChild<QLabel *>("teamHeatError")->toolTip().contains("usage: hgs"), qPrintable(again->findChild<QLabel *>("teamHeatError")->toolTip()));
    QVERIFY(again->findChildren<QLabel *>("teamBoardHeat").isEmpty());
    // Off forgets the choice.
    heatButton(again.get(), "off")->click();
    QTRY_VERIFY(!again->findChild<QWidget *>("teamHeatLegend"));
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("off"));
    QVERIFY(!Settings::contains("teams/heat/team-1"));
}

// #709 L6: the tint is read again a minute later; that read rebuilds Graph
// and Board only when what the tint draws changed. A wait that grew by
// seconds the loss does not show keeps every widget, its hover and scroll.
void TestTeamsUiProfile::aHeatReadThatDrawsTheSameRebuildsNothing()
{
    TeamUi::setHeatMode("team-1", "time");
    const auto forget = qScopeGuard([] { TeamUi::setHeatMode("team-1", "off"); TeamWorkspace::setHeatAge(60 * 1000); });
    setTeam(heatTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(heatProfile()).toJson(QJsonDocument::Compact)));
    QFile::remove(script() + ".profilerefuse");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.setShowCancelled(true);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2140, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(phaseScrollOf(&workspace, 38));
    if (!phaseOpen(&workspace, 38)) togglePhase(&workspace, 38);
    QTRY_VERIFY(phaseOpen(&workspace, 38));
    const auto graph = [&]() -> TeamUi::TaskView * {
        for (auto *widget : workspace.findChildren<QWidget *>("teamPhaseGraph"))
            if (auto *view = dynamic_cast<TeamUi::TaskView *>(widget)) return view;
        return nullptr;
    };
    const auto node = [&](int id) {
        if (auto *view = graph()) for (const auto &n : view->nodes) if (n.id == id) return n;
        return TeamUi::TaskView::Node{};
    };
    QTRY_COMPARE(node(483).heat, QStringLiteral("late 2h 20m (1h 16m from #482)"));
    TestWait::settle();
    QPointer<QWidget> view = graph();
    QPointer<QWidget> legend = workspace.findChild<QWidget *>("teamHeatLegend");
    QVERIFY(view && legend);
    // The minute timer finds the tint due and reads both pages again.
    const auto reread = [&](const QJsonObject &profile) {
        QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profile).toJson(QJsonDocument::Compact)));
        const int before = calls(script(), "profile").size();
        TeamWorkspace::setHeatAge(0);
        QTest::qWait(5);
        QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout"));
        TeamWorkspace::setHeatAge(60 * 1000);
        QTRY_COMPARE(calls(script(), "profile").size(), before + 2);
        QTest::qWait(300);
    };
    reread(heatProfile());
    QVERIFY2(view && view == graph(), "the same profile rebuilt the Graph");
    QVERIFY(legend && legend == workspace.findChild<QWidget *>("teamHeatLegend"));
    // #483 is ten seconds later: still "late 2h 20m".
    reread(heatProfile(7210));
    QVERIFY2(view && view == graph(), "ten seconds no lateness text shows rebuilt the Graph");
    QCOMPARE(node(483).heat, QStringLiteral("late 2h 20m (1h 16m from #482)"));
    // Two minutes more: it reads 2h 22m, drawn again.
    reread(heatProfile(7320));
    QTRY_VERIFY(!view || view != graph());
    QTRY_COMPARE(node(483).heat, QStringLiteral("late 2h 22m (1h 16m from #482)"));
    QVERIFY2(node(483).tooltip.contains("Causes: Waited for the lead (decision 1h 42m, questions 20m) 2h 02m"), qPrintable(node(483).tooltip));
}

// U62 (#719): the profiler's timeline shows the team's test runs of the scope's
// window in a Tests row under the lanes, colored by status; one bounded read
// after the spans, a refusal leaves the row out.
void TestTeamsUiProfile::profileShowsTheTestRunsOfItsWindow()
{
    setTeam(phasedTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profileOfPhase2()).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(script() + ".spans", QJsonDocument(spansOfPhase2()).toJson(QJsonDocument::Compact)));
    const double t0 = profileStart();
    const auto run = [](const QString &id, const QString &suite, const QString &status, double at, qint64 ms, int task) {
        return QJsonObject{{"id", id}, {"team", "team-1"}, {"suite", suite}, {"status", status}, {"at", at}, {"duration_ms", ms},
                           {"task", task}, {"excerpt", "x"}, {"asked_by", "frontend"}, {"affected", QJsonArray()}};
    };
    const QJsonArray runs{run("before", "qt-teams", "passed", t0 - 5000, 60000, 3), run("one", "qt-teams", "failed", t0 + 9000, 600000, 3),
                          run("two", "cargo-teams", "passed", t0 + 15600, 900000, 0), run("three", "android", "timeout", t0 + 20000, 1200000, 4),
                          run("after", "qt-teams", "passed", t0 + 30000, 60000, 3)};
    QVERIFY(FakeCli::replace(script() + ".testresults", QJsonDocument(runs).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *profile = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *b : workspace.findChildren<QPushButton *>()) if (b->property("segment") == "profile" && b->isVisible()) profile = b;
        return profile != nullptr;
    })());
    profile->click();
    TeamProfileView *view = nullptr;
    QTRY_VERIFY((view = profileViewOf(&workspace)) && view->isVisible());
    QTRY_COMPARE(view->model().tests.size(), 3);
    // One read of the window, after the spans: a read, bounded.
    const auto reads = calls(script(), "test");
    QCOMPARE(reads.size(), 1);
    QCOMPARE(reads[0]["action"].toString(), QStringLiteral("results"));
    QCOMPARE(reads[0]["from"].toDouble(), t0);
    QCOMPARE(reads[0]["before"].toDouble(), t0 + 21601);
    QCOMPARE(reads[0]["limit"].toInt(), TeamProfile::Cache::TestPage);
    QVERIFY(!reads[0].contains("request_id"));
    QStringList ids, suites;
    for (const auto &span : view->model().tests) { ids << span.record; suites << span.member; }
    QCOMPARE(ids, (QStringList{"one", "two", "three"}));
    QCOMPARE(suites, (QStringList{"qt-teams", "cargo-teams", "android"}));
    QCOMPARE(view->model().tests[0].stageKind, QStringLiteral("failed"));
    QCOMPARE(view->model().tests[0].task, 3);
    QCOMPARE(view->model().tests[0].seconds(), 600.0);
    // The row under the lanes: the timeline is one row taller.
    auto *timeline = workspace.findChild<QWidget *>("teamProfileTimeline");
    QTRY_COMPARE(timeline->height(), 34 + 52 * (2 + int(view->model().lanes.size()) + 1) + 8);
    if (shotsWanted()) {
        shot(&workspace, "u719-profile-tests-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u719-profile-tests-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // Built directly: a run outside the window stays out; no runs, no row.
    const auto model = TeamProfile::Model::build(Teams::parse(phasedTeam()), profileOfPhase2(), spansOfPhase2(), t0 + 21600, runs);
    QCOMPARE(model.tests.size(), 3);
    QVERIFY(TeamProfile::Model::build(Teams::parse(phasedTeam()), profileOfPhase2(), spansOfPhase2(), t0 + 21600).tests.isEmpty());
}

// Owner 2026-10-31 (#746): the Time view compares each task's estimate with
// its work and rework, the largest overrun first; a task without one says so.
void TestTeamsUiProfile::profileComparesEstimateAndActualPerTask()
{
    setTeam(phasedTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profileOfPhase2()).toJson(QJsonDocument::Compact)));
    auto spans = spansOfPhase2();
    auto three = spans[0].toObject(); three["estimate_minutes"] = 180; three["estimate_source"] = "history"; spans[0] = three;
    auto four = spans[1].toObject(); four["estimate_minutes"] = 30; four["estimate_source"] = "lead"; spans[1] = four;
    QVERIFY(FakeCli::replace(script() + ".spans", QJsonDocument(spans).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *profile = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *b : workspace.findChildren<QPushButton *>()) if (b->property("segment") == "profile" && b->isVisible()) profile = b;
        return profile != nullptr;
    })());
    profile->click();
    TeamProfileView *view = nullptr;
    QTRY_VERIFY((view = profileViewOf(&workspace)) && view->isVisible());
    QTRY_COMPARE(view->model().estimates.size(), 2);
    // #3: rework 50 min and work 195 min against 180; #4: 23 min against 30.
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamProfileEstimates"));
    QStringList order, values, actual, diff;
    for (auto *task : workspace.findChildren<QPushButton *>("teamProfileEstimateTask")) order << task->property("task").toString();
    for (auto *l : workspace.findChildren<QLabel *>("teamProfileEstimateValue")) values << l->text();
    for (auto *l : workspace.findChildren<QLabel *>("teamProfileEstimateActual")) actual << l->text();
    for (auto *l : workspace.findChildren<QLabel *>("teamProfileEstimateDiff")) diff << l->text();
    QCOMPARE(order, (QStringList{"3", "4"}));
    QCOMPARE(values, (QStringList{"~180 min", "~30 min"}));
    QCOMPARE(actual, (QStringList{"245 min", "23 min"}));
    QCOMPARE(diff, (QStringList{"65 min over", "7 min under"}));
    QCOMPARE(workspace.findChildren<QLabel *>("teamProfileEstimateDiff").first()->palette().color(QPalette::WindowText), TeamUi::colorsFor(true).warn);
    QCOMPARE(workspace.findChild<QLabel *>("teamProfileEstimatesSummary")->text(), QStringLiteral("1 of 2 over the estimate"));
    if (shotsWanted()) {
        shot(&workspace, "u746-profile-estimates-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u746-profile-estimates-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // A task row opens the task.
    workspace.findChildren<QPushButton *>("teamProfileEstimateTask").first()->click();
    QTRY_COMPARE(view->selectedTask(), 3);
    // Without estimate fields (an older hgs) there is no card.
    const auto model = TeamProfile::Model::build(Teams::parse(phasedTeam()), profileOfPhase2(), spansOfPhase2(), profileStart() + 21600);
    QVERIFY(model.estimates.isEmpty());
}

// Owner 2026-10-31: the timeline zooms (buttons, Ctrl+wheel, double click fits);
// the Top losses column and the estimates table fold, and stay so.
void TestTeamsUiProfile::profileTimelineZoomsAndTheSidesFold()
{
    Settings::set(QStringLiteral("teams/profileLossesOpen"), true);
    Settings::set(QStringLiteral("teams/profileEstimatesOpen"), true);
    setTeam(phasedTeam());
    QVERIFY(FakeCli::replace(script() + ".profile", QJsonDocument(profileOfPhase2()).toJson(QJsonDocument::Compact)));
    QVERIFY(FakeCli::replace(script() + ".spans", QJsonDocument(spansOfPhase2()).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *profile = nullptr;
    QTRY_VERIFY(([&] {
        for (auto *b : workspace.findChildren<QPushButton *>()) if (b->property("segment") == "profile" && b->isVisible()) profile = b;
        return profile != nullptr;
    })());
    profile->click();
    QWidget *timeline = nullptr;
    QTRY_VERIFY((timeline = workspace.findChild<QWidget *>("teamProfileTimeline")) && timeline->isVisible());
    const auto zoom = [&] { auto *now = workspace.findChild<QWidget *>("teamProfileTimeline"); return now ? now->property("zoom").toDouble() : 0.0; };
    QCOMPARE(zoom(), 1.0);
    workspace.findChild<QPushButton *>("teamProfileZoomIn")->click();
    QCOMPARE(zoom(), 1.5);
    // Ctrl+wheel at the pointer zooms further; a plain wheel does not.
    const QPointF at(timeline->width() * 0.6, 60);
    QWheelEvent ctrl(at, timeline->mapToGlobal(at), {}, QPoint(0, 120), Qt::NoButton, Qt::ControlModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(timeline, &ctrl);
    QVERIFY(zoom() > 1.5);
    const double zoomed = zoom();
    QWheelEvent plain(at, timeline->mapToGlobal(at), {}, QPoint(0, 120), Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(timeline, &plain);
    QCOMPARE(zoom(), zoomed);
    QTest::mouseDClick(timeline, Qt::LeftButton, {}, QPoint(timeline->width() / 2, 60));
    QCOMPARE(zoom(), 1.0);
    // The Top losses column folds and the timeline widens.
    auto *losses = workspace.findChild<QFrame *>("teamProfileLosses");
    QVERIFY(losses && losses->isVisible());
    const int wide = timeline->width();
    workspace.findChild<QPushButton *>("teamProfileLossesToggle")->click();
    QTRY_VERIFY(!losses->isVisible());
    QTRY_VERIFY(timeline->width() > wide);
    QCOMPARE(Settings::value(QStringLiteral("teams/profileLossesOpen")).toBool(), false);
    workspace.findChild<QPushButton *>("teamProfileLossesToggle")->click();
    QTRY_VERIFY(losses->isVisible());
    // The estimates table folds to its line, and unfolds.
    if (auto *toggle = workspace.findChild<QPushButton *>("teamProfileEstimatesToggle")) {
        QVERIFY(!workspace.findChildren<QPushButton *>("teamProfileEstimateTask").isEmpty());
        toggle->click();
        QTRY_VERIFY(workspace.findChildren<QPushButton *>("teamProfileEstimateTask").isEmpty());
        QCOMPARE(Settings::value(QStringLiteral("teams/profileEstimatesOpen")).toBool(), false);
        QTRY_VERIFY(workspace.findChild<QPushButton *>("teamProfileEstimatesToggle"));
        workspace.findChild<QPushButton *>("teamProfileEstimatesToggle")->click();
        QTRY_VERIFY(!workspace.findChildren<QPushButton *>("teamProfileEstimateTask").isEmpty());
    }
}

QTEST_MAIN(TestTeamsUiProfile)
#include "test_teamsui_profile.moc"

// Task #79 (owner, 2026-11-06: "All colours must be clearly readable"): the
// owner's case in both themes, before and after; every kind of span and wait.
void TestTeamsUiProfile::timelineKindsAreDistinctAndBuildsNeverHideTitles()
{
    setTeam(phasedTeam());
    const auto team = Teams::parse(phasedTeam());
    const auto model = TeamProfile::Model::build(team, profileOfPhase2(), spansOfOwnersCase(), profileStart() + 21600);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamProfile::Cache cache(&controller);
    TeamProfileView view(&controller, &cache);
    view.setDark(true);
    view.resize(1180, 820); view.show();
    view.showModel(model);
    view.setTab("time");
    QTRY_VERIFY(view.findChild<QWidget *>("teamProfileTimeline"));
    shot(&view, "t79-profile-time-dark");
    // Task titles on work bars are never covered: build and merge are a strip
    // along the bottom of the lane, below the work bar and the waits.
    using Bands = TeamProfile::LaneBands;
    QVERIFY(Bands::BuildTop >= Bands::WorkTop + Bands::WorkHeight && Bands::BuildTop >= Bands::WaitTop + Bands::WaitHeight);
    QVERIFY(Bands::BuildTop + Bands::BuildHeight <= Bands::Row && Bands::WaitTop >= Bands::WorkTop + Bands::WorkHeight);
    {
        // Drawn so: no pixel row of the timeline holds both a work bar and a build.
        const QImage image = view.findChild<QWidget *>("teamProfileTimeline")->grab().toImage().convertToFormat(QImage::Format_RGB32);
        const QRgb work = TeamProfile::kindColor("work", {}, true).rgb(), build = TeamProfile::kindColor("build", {}, true).rgb();
        int workRows = 0, buildRows = 0;
        for (int y = 0; y < image.height(); ++y) {
            bool hasWork = false, hasBuild = false;
            const auto *line = reinterpret_cast<const QRgb *>(image.constScanLine(y));
            for (int x = 0; x < image.width(); ++x) { hasWork = hasWork || line[x] == work; hasBuild = hasBuild || line[x] == build; }
            workRows += hasWork; buildRows += hasBuild;
            QVERIFY2(!(hasWork && hasBuild), qPrintable(QStringLiteral("row %1 has a build over a work bar").arg(y)));
        }
        QVERIFY(workRows > 0 && buildRows > 0);
    }
    // The legend shows every kind, its name in full (the items wrap).
    const auto labels = view.findChildren<QLabel *>("teamProfileLegendLabel");
    QCOMPARE(labels.size(), TeamProfile::kindStyles().size());
    for (auto *label : labels) {
        // The legend's flow layout places its items on the first layout pass: wait for it.
        QTRY_VERIFY2(label->isVisible() && label->width() >= label->fontMetrics().horizontalAdvance(label->text()), qPrintable(label->text()));
        QVERIFY(!label->text().endsWith(QChar(0x2026)));
    }
    for (const auto &key : {QStringLiteral("member"), QStringLiteral("deps"), QStringLiteral("person"), QStringLiteral("folder_held"), QStringLiteral("hold"), QStringLiteral("stage_wait")}) {
        bool found = false;
        for (auto *label : labels) found = found || label->text() == TeamProfile::keyName(key);
        QVERIFY2(found, qPrintable(key));
    }
    // No two kinds share a colour in any shipped skin, light or dark: CIE76
    // distance of 20 or more, 3:1 or more against the skin's lane background,
    // a bar's title 4.5:1 on its fill, every wait its own hatch and (task 93)
    // no colour above the chroma cap.
    const auto lab = [](const QColor &c) {
        const auto lin = [](double v) { return v <= 0.04045 ? v / 12.92 : std::pow((v + 0.055) / 1.055, 2.4); };
        const double r = lin(c.redF()), g = lin(c.greenF()), b = lin(c.blueF());
        const double x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047, yy = 0.2126 * r + 0.7152 * g + 0.0722 * b,
                     z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
        const auto f = [](double t) { return t > 0.008856 ? std::cbrt(t) : 7.787 * t + 16.0 / 116; };
        return std::array<double, 3>{116 * f(yy) - 16, 500 * (f(x) - f(yy)), 200 * (f(yy) - f(z))};
    };
    const auto distance = [&](const QColor &a, const QColor &b) {
        const auto p = lab(a), q = lab(b);
        return std::sqrt((p[0] - q[0]) * (p[0] - q[0]) + (p[1] - q[1]) * (p[1] - q[1]) + (p[2] - q[2]) * (p[2] - q[2]));
    };
    const auto &styles = TeamProfile::kindStyles();
    QVERIFY(!WorkspaceSkin::builtins().isEmpty());
    for (const auto &skin : WorkspaceSkin::builtins()) {
        for (const bool dark : {true, false}) {
            const QColor lane = WorkspaceSkin::color(skin, dark, QStringLiteral("card"));
            for (int i = 0; i < styles.size(); ++i) {
                const QColor a = dark ? styles[i].dark : styles[i].light;
                QVERIFY2(TeamProfile::contrast(a, lane) >= 3.0, qPrintable(QStringLiteral("%1 %2 %3 on its lane").arg(skin.id, dark ? "dark" : "light", styles[i].key)));
                QVERIFY2(TeamProfile::contrast(TeamProfile::labelOn(a), a) >= 4.5, qPrintable(styles[i].key));
                // Task 93: calm colours, no kind above the chroma cap.
                const auto ab = lab(a);
                QVERIFY2(std::hypot(ab[1], ab[2]) <= (dark ? TeamProfile::ChromaCapDark : TeamProfile::ChromaCapLight),
                         qPrintable(QStringLiteral("%1 %2 %3 %4 chroma %5").arg(skin.id, dark ? "dark" : "light", styles[i].key, a.name()).arg(std::hypot(ab[1], ab[2]))));
                for (int j = i + 1; j < styles.size(); ++j) {
                    const QColor b = dark ? styles[j].dark : styles[j].light;
                    QVERIFY2(distance(a, b) >= 20, qPrintable(QStringLiteral("%1 %2: %3 and %4").arg(skin.id, dark ? "dark" : "light", styles[i].key, styles[j].key)));
                    if (styles[i].pattern != Qt::SolidPattern && styles[j].pattern != Qt::SolidPattern) QVERIFY(styles[i].pattern != styles[j].pattern);
                }
            }
        }
    }
    // Every kind the timeline draws has its own key; the old look-alikes are apart.
    QVERIFY(TeamProfile::kindColor("build", {}, true) != TeamProfile::kindColor("stage", "test", true));
    QVERIFY(TeamProfile::kindColor("member", {}, true) != TeamProfile::kindColor("stage", "test", true));
    QCOMPARE(TeamProfile::kindKey("permission", {}), QStringLiteral("blocked"));
    QCOMPARE(TeamProfile::kindKey("start_delay", {}), QStringLiteral("delivery"));
    QCOMPARE(TeamProfile::kindKey("stage", "lint"), QStringLiteral("stage"));
    // The light shot with the light skin's window colours, as the app sets them.
    const QPalette before = QApplication::palette();
    {
        QPalette light = before;
        const auto token = [](const char *name) { return WorkspaceSkin::color(WorkspaceSkin::active(), false, QString::fromLatin1(name)); };
        for (const auto role : {QPalette::Window, QPalette::Base, QPalette::Button}) light.setColor(role, token("bg"));
        for (const auto role : {QPalette::WindowText, QPalette::Text, QPalette::ButtonText}) light.setColor(role, token("fg"));
        QApplication::setPalette(light);
    }
    const auto restore = qScopeGuard([before] { QApplication::setPalette(before); });
    view.setDark(false); view.showModel(model); view.setTab("time");
    shot(&view, "t79-profile-time-light");
    view.setDark(true);
}

// Task 92: the Tauri Time view projects its lanes in client/core
// (teams/timeline.rs). On the same real replies of this team (client/core's
// fixtures) Model::build gives the table the core's test asserts too:
// each lane's own span kinds and its waits, in the profile's member order.
void TestTeamsUiProfile::timelineLanesMatchTheTauriCoreOnTheTeamsReplies()
{
    const auto read = [](const QString &name) {
        QFile file(QFINDTESTDATA(QStringLiteral("../../client/core/tests/fixtures/teams/team-%1.json").arg(name)));
        if (!file.open(QIODevice::ReadOnly)) return QJsonObject();
        return QJsonDocument::fromJson(file.readAll()).object();
    };
    const auto status = read("status"), profile = read("profile"), spans = read("spans");
    QVERIFY2(!status.isEmpty() && !profile.isEmpty() && !spans.isEmpty(), "the core's team fixtures");
    const double now = profile.value("summary").toObject().value("to").toDouble() + 600;
    const auto model = TeamProfile::Model::build(Teams::parse(status), profile, spans.value("tasks").toArray(), now);
    const auto counts = [](const QList<TeamProfile::Span> &list) {
        QMap<QString, int> kinds;
        for (const auto &span : list) ++kinds[span.kind];
        QStringList out;
        for (auto it = kinds.cbegin(); it != kinds.cend(); ++it) out << QStringLiteral("%1:%2").arg(it.key()).arg(it.value());
        return out.join(QLatin1Char(','));
    };
    QStringList table;
    for (const auto &lane : model.lanes) table << QStringLiteral("%1|%2|%3").arg(lane.member, counts(lane.spans), counts(lane.waits));
    const QStringList expected{
        QStringLiteral("core|build:6,question:1,work:5|delivery:3,deps:2,lead:2,member:3,start_delay:3"),
        QStringLiteral("hgs||deps:1"),
        QStringLiteral("ui1|build:13,work:5|delivery:5,deps:2,lead:5,member:5,start_delay:5"),
        QStringLiteral("ui2|permission:2,question:8,work:11|delivery:3,deps:3,lead:1,member:6,start_delay:3"),
        QStringLiteral("merger||deps:4"),
        QStringLiteral("qa|work:1|delivery:1,deps:2,member:1,start_delay:1"),
    };
    QCOMPARE(table, expected);
    QCOMPARE(model.realSeconds, qint64(profile.value("critical_path").toObject().value("real").toObject().value("s").toDouble()));
}

// Task 118 (owner, 2026-11-06): waiting for a person's answer is its own kind,
// with its own colour, hatch, name and loss; and the legend (one line that cut
// every label in the owner's build) wraps and shows every label in full at
// narrow and wide widths.
void TestTeamsUiProfile::waitingForAPersonHasItsOwnKindAndTheLegendWrapsAtEveryWidth()
{
    setTeam(phasedTeam());
    const auto team = Teams::parse(phasedTeam());
    const double t0 = profileStart();
    // #3's report went to the owner after the lead's quarter hour: a wait for a person.
    auto spans = spansOfOwnersCase();
    auto first = spans.at(0).toObject();
    auto list = first.value("spans").toArray();
    list.append(QJsonObject{{"kind", "person"}, {"reason", "report"}, {"member", "backend"}, {"attempt", 2}, {"from", t0 + 15600}, {"to", t0 + 17400}, {"s", 1800}});
    first.insert("spans", list);
    spans.replace(0, first);
    auto profile = profileOfPhase2();
    auto losses = profile.value("losses").toObject();
    auto byTime = losses.value("by_time").toObject();
    auto items = byTime.value("items").toArray();
    items.prepend(QJsonObject{{"id", "person_wait:3"}, {"kind", "person_wait"}, {"task", 3}, {"s", 1800},
                              {"reasons", QJsonObject{{"report", 1500}, {"acceptance", 300}}}});
    byTime.insert("items", items);
    losses.insert("by_time", byTime);
    profile.insert("losses", losses);
    const auto model = TeamProfile::Model::build(team, profile, spans, t0 + 21600);

    // The kind: its own key, calm colour in each theme, its own hatch (a texture) and name.
    QCOMPARE(TeamProfile::kindKey("person", {}), QStringLiteral("person"));
    QCOMPARE(TeamProfile::keyName("person"), QStringLiteral("Waiting for a person's answer"));
    QCOMPARE(TeamProfile::kindName("person", {}), QStringLiteral("Waiting for a person's answer"));
    QCOMPARE(TeamProfile::kindColor("person", {}, true), QColor("#d8eea5"));
    QCOMPARE(TeamProfile::kindColor("person", {}, false), QColor("#8a9269"));
    QCOMPARE(TeamProfile::kindPattern("person", {}), Qt::TexturePattern);
    QVERIFY(TeamProfile::kindColor("person", {}, true) != TeamProfile::kindColor("lead", {}, true));
    QCOMPARE(TeamProfile::kindName("lead", {}), QStringLiteral("Waited for the lead"));
    const QBrush brush = TeamProfile::kindBrush(QColor("#d8eea5"), Qt::TexturePattern);
    QCOMPARE(brush.style(), Qt::TexturePattern);
    QVERIFY(!brush.texture().isNull());
    QCOMPARE(TeamProfile::kindBrush(QColor("#7dd3b0"), Qt::SolidPattern).style(), Qt::SolidPattern);
    QCOMPARE(TeamProfile::personReasons(QJsonObject{{"report", 1500}, {"acceptance", 300}}), QStringLiteral("phase acceptance 5m, a report decision 25m"));
    {
        // The heat names the loss the same way.
        TeamUi::HeatLoss heat;
        heat.kind = QStringLiteral("person_wait");
        heat.seconds = 1800;
        heat.detail = QJsonObject{{"reasons", QJsonObject{{"report", 1500}, {"acceptance", 300}}}};
        QCOMPARE(TeamUi::heatLossName(heat), QStringLiteral("Waiting for a person's answer (phase acceptance 5m, a report decision 25m)"));
        heat.detail = {};
        QCOMPARE(TeamUi::heatLossName(heat), QStringLiteral("Waiting for a person's answer"));
    }

    // A wait, never the member's own time: it sits in the waits under its task's lane.
    int waits = 0, own = 0;
    for (const auto &lane : model.lanes) {
        for (const auto &span : lane.waits) waits += span.kind == QLatin1String("person");
        for (const auto &span : lane.spans) own += span.kind == QLatin1String("person");
    }
    QCOMPARE(waits, 1);
    QCOMPARE(own, 0);

    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamProfile::Cache cache(&controller);
    TeamProfileView view(&controller, &cache);
    view.setDark(true);
    view.resize(1180, 820); view.show();
    view.showModel(model);
    view.setTab("time");
    QTRY_VERIFY(view.findChild<QWidget *>("teamProfileTimeline"));
    // Its loss: titled by the task, with the reasons it waited on.
    QStringList titles, details;
    for (auto *card : view.findChildren<QFrame *>("teamProfileLoss"))
        if (card->property("kind").toString() == QLatin1String("person_wait")) {
            for (auto *label : card->findChildren<QLabel *>()) (label->objectName() == QLatin1String("teamProfileLossTitle") ? titles : details) << label->text();
        }
    QCOMPARE(titles.size(), 1);
    QVERIFY2(titles.first().startsWith(QStringLiteral("Waiting for a person's answer: #3")), qPrintable(titles.first()));
    QVERIFY2(details.join(QLatin1Char('|')).contains(QStringLiteral("phase acceptance 5m, a report decision 25m")), qPrintable(details.join(QLatin1Char('|'))));
    shot(&view, "t118-profile-time-person-dark");

    // The legend: every label in full, on as many lines as the width needs.
    const auto itemsOf = [&view] { return view.findChildren<QWidget *>("teamProfileLegendItem"); };
    QCOMPARE(itemsOf().size(), TeamProfile::kindStyles().size());
    auto *legend = view.findChild<QWidget *>("teamProfileLegend");
    QVERIFY(legend);
    // Every label whole (no cut, no ellipsis) and every item inside the legend, not cut at its bottom.
    const auto fits = [&]() {
        if (legend->width() <= 0) return false;
        for (auto *label : view.findChildren<QLabel *>("teamProfileLegendLabel")) {
            if (!label->isVisible() || label->width() < label->fontMetrics().horizontalAdvance(label->text())) return false;
            if (label->text().endsWith(QChar(0x2026))) return false;
        }
        for (auto *item : itemsOf())
            if (item->geometry().right() > legend->width() || item->geometry().left() < 0 || item->geometry().bottom() > legend->height()) return false;
        return true;
    };
    const auto rows = [&] {
        QSet<int> tops;
        for (auto *item : itemsOf()) tops.insert(item->y());
        return int(tops.size());
    };
    view.resize(620, 820);
    QTRY_VERIFY2(fits(), "the legend at 620 px");
    const int narrow = rows();
    view.resize(900, 820);
    QTRY_VERIFY2(fits(), "the legend at 900 px");
    const int middle = rows();
    view.resize(1900, 820);
    QTRY_VERIFY2(fits(), "the legend at 1900 px");
    const int wide = rows();
    QVERIFY2(narrow >= 2, qPrintable(QString::number(narrow)));
    QVERIFY2(narrow >= middle && middle >= wide, qPrintable(QStringLiteral("%1 %2 %3").arg(narrow).arg(middle).arg(wide)));
    QVERIFY2(narrow > wide, qPrintable(QStringLiteral("%1 %2").arg(narrow).arg(wide)));
    view.resize(620, 820);
    QTest::qWait(100);
    shot(&view, "t118-legend-narrow-dark");
    view.resize(1900, 820);
    QTest::qWait(100);
    shot(&view, "t118-legend-wide-dark");
    view.setDark(false);
    view.resize(620, 820);
    QTest::qWait(100);
    shot(&view, "t118-legend-narrow-light");
}
