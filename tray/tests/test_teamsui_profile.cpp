// WhiteWh fork, #713: the agent-team UI tests of the pipeline profile and the
// heat tint.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
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
    add(485, "Test phase 38", "qa", "cancelled", {483}, 3);
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
    QJsonArray first, second;
    for (int i = 0; i < time.size(); ++i) (i < 9 ? first : second).append(time.at(i));
    const QJsonArray tokens{
        loss("rework", 482, 3900, {{"attempt", 1}, {"cause", "returned"}, {"tokens", 1240000}, {"cost_usd", 3.1}, {"share", .58}}),
        loss("rework", 480, 2820, {{"attempt", 1}, {"cause", "returned"}, {"tokens", 236000}, {"cost_usd", .6}, {"share", .41}}),
        loss("rework", 481, 1200, {{"attempt", 1}, {"cause", "rejected"}, {"tokens", 98000}, {"cost_usd", .25}, {"share", .22}}),
        loss("long_test", 476, 2100, {{"tokens", 61000}, {"cost_usd", .15}})};
    return QJsonObject{{"time", QJsonArray{page("by_time", first, QJsonObject{{"s", 2460}, {"id", "member_wait:477"}, {"page", 1}}, false),
                                           page("by_time", second, QJsonValue(), false)}},
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
    void profileViewShowsThePhaseAndItsLosses();
    void profileReadsInBoundedPagesAndKeepsABoundedCache();
    void profileOfALargeTeamStaysResponsive();
    void heatTintsGraphAndBoardByTimeOrTokens();
    void aHeatReadThatDrawsTheSameRebuildsNothing();
};

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
    // A wide toolbar says the choices in words (a narrow one keeps the icons).
    QTRY_COMPARE(heatButton(workspace.get(), "time")->text(), QStringLiteral("Time lost"));
    QCOMPARE(heatButton(workspace.get(), "tokens")->text(), QStringLiteral("Tokens"));
    QCOMPARE(heatButton(workspace.get(), "off")->text(), QStringLiteral("Off"));
    QVERIFY(!workspace->findChild<QWidget *>("teamHeatLegend"));
    QVERIFY(node(483).heat.isEmpty() && !node(483).heatFill.isValid());
    QCOMPARE(calls(script(), "profile").size(), profileCalls);
    // Time lost: the plan's losses by time in two pages, without the
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
    QTRY_COMPARE(node(483).heat, QStringLiteral("lost 3h 56m"));
    const auto c = TeamUi::colorsFor(true);
    // The integration lost more than 2 h: the warmest tint, the loss in the
    // heat color. A small loss is muted, a task without one says so untinted.
    QCOMPARE(node(483).heatFill, TeamUi::heatFill(c, 1));
    QCOMPARE(node(483).heatColor, c.danger);
    QCOMPARE(node(482).heat, QStringLiteral("lost 1h 47m"));
    QCOMPARE(node(475).heat, QStringLiteral("lost 37m"));
    QCOMPARE(node(475).heatColor, c.muted);
    QVERIFY(node(475).heatFill.isValid() && node(475).heatFill != node(483).heatFill && node(475).heatFill != c.surface);
    QCOMPARE(node(474).heat, QStringLiteral("no loss"));
    QVERIFY(!node(474).heatFill.isValid());
    // Cancelled work and the node's own lines: no tint; the working time
    // gives way to the loss.
    QVERIFY(node(484).heat.isEmpty() && !node(484).heatFill.isValid());
    QVERIFY(node(483).worked.isEmpty());
    // The tooltip names the top loss and the next two.
    const QString tip = node(483).tooltip;
    QVERIFY2(tip.contains("Lost 3h 56m to waits and rework\nTop loss: Waited for the lead (decision 1h 40m, questions 20m), 2h 00m"), qPrintable(tip));
    QVERIFY2(tip.contains("Then: Waited for dependencies #474, #482 1h 16m; Merge conflict 40m"), qPrintable(tip));
    QVERIFY2(node(515).tooltip.contains("Top loss: Folder held by #474, 41m"), qPrintable(node(515).tooltip));
    QVERIFY2(node(478).tooltip.contains("Top loss: Waited for a permission (answered by You), 25m"), qPrintable(node(478).tooltip));
    // The legend: the metric, the 0 to 2 h scale and the tint-only note.
    auto *legend = workspace->findChild<QWidget *>("teamHeatLegend");
    QVERIFY(legend);
    QCOMPARE(legend->findChild<QLabel *>("teamHeatMetric")->text(), QStringLiteral("Time lost: waits and rework"));
    QCOMPARE(legend->findChild<QLabel *>("teamHeatEnd")->text(), QStringLiteral("2h or more"));
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
    QCOMPARE(lost(483)->text(), QStringLiteral("lost 3h 56m"));
    QVERIFY(lost(483)->font().bold());
    QVERIFY(card(483)->styleSheet().contains(TeamUi::heatFill(c, 1).name()));
    QVERIFY(card(474)->styleSheet().contains("background:" + c.surface.name()));
    QCOMPARE(lost(474)->text(), QStringLiteral("no loss"));
    QVERIFY(!lost(485));
    QVERIFY(!card(483)->findChild<QLabel *>("teamBoardWorked"));
    QVERIFY(card(483)->toolTip().contains("Top loss: Waited for the lead"));
    QTRY_VERIFY(([&] {
        auto *heat = lost(483);
        auto *role = card(483) ? card(483)->findChild<QWidget *>("teamBoardRole") : nullptr;
        return heat && role && heat->geometry().left() > role->geometry().right();
    })());
    shoot("u38-heat-board-time");
    // Tokens: the losses by tokens, read with the sessions' tokens; the scale
    // ends at the largest task's and the unknown tokens are named.
    heatButton(workspace.get(), "tokens")->click();
    QTRY_VERIFY(!calls(script(), "profile").isEmpty() && calls(script(), "profile").last()["tokens"].toBool());
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("tokens"));
    QTRY_COMPARE(lost(482) ? lost(482)->text() : QString(), QStringLiteral("lost 1.2M tokens"));
    QCOMPARE(lost(480)->text(), QStringLiteral("lost 236k tokens"));
    QCOMPARE(lost(483)->text(), QStringLiteral("no loss"));
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
    // Layers has no tint and no switch; the choice stays for Graph and Board.
    view("layers");
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
    QTRY_COMPARE(node(483).heat, QStringLiteral("lost 3h 56m"));
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
    // The lead's wait on #483 grew by ten seconds: still "lost 3h 56m".
    reread(heatProfile(7210));
    QVERIFY2(view && view == graph(), "ten seconds no loss text shows rebuilt the Graph");
    QCOMPARE(node(483).heat, QStringLiteral("lost 3h 56m"));
    // Two minutes more: the loss reads 3h 58m, drawn again.
    reread(heatProfile(7320));
    QTRY_VERIFY(!view || view != graph());
    QTRY_COMPARE(node(483).heat, QStringLiteral("lost 3h 58m"));
    QVERIFY2(node(483).tooltip.contains("Top loss: Waited for the lead (decision 1h 42m, questions 20m), 2h 02m"), qPrintable(node(483).tooltip));
}

QTEST_MAIN(TestTeamsUiProfile)
#include "test_teamsui_profile.moc"
