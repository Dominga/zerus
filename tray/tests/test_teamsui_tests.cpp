// WhiteWh fork, #719 (U62): the agent-team UI tests of the Tests view and a
// task's test runs in its drawer. The profiler's Tests row is tested with the
// profile (test_teamsui_profile.cpp).
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "HgsClient.h"
#include "TeamTaskCard.h"
#include "TeamTests.h"
#include "TeamWorkspace.h"
#include <QPlainTextEdit>

namespace {
double nowSeconds() { return double(QDateTime::currentSecsSinceEpoch()); }

QJsonObject record(const QString &id, const QString &suite, const QString &status, double at, qint64 ms, int task, const QJsonObject &extra = {})
{
    QJsonObject value{{"id", id}, {"team", "team-1"}, {"suite", suite}, {"commit", "4f1c2a9e8b7d"}, {"input_hash", "h"}, {"status", status},
                      {"duration_ms", ms}, {"excerpt", QStringLiteral("%1 %2 in %3 ms").arg(suite, status).arg(ms)}, {"asked_by", "frontend"},
                      {"at", at}, {"affected", QJsonArray{"test_teamsui_tests"}}, {"task", task ? QJsonValue(task) : QJsonValue()}};
    for (auto it = extra.begin(); it != extra.end(); ++it) value.insert(it.key(), it.value());
    return value;
}

// What runs, what waits: qt-teams runs for #3, cargo-teams waits forced and
// android waits for #4.
QJsonArray queueNow()
{
    const double now = nowSeconds();
    const auto item = [](const QString &id, const QString &suite, const QJsonValue &task, bool forced, double at, double seconds,
                         const QString &resources, const QString &state, const QJsonValue &started) {
        return QJsonObject{{"id", id}, {"team", "team-1"}, {"suite", suite}, {"task", task}, {"targets", QJsonArray()}, {"asked_by", "frontend"},
                           {"forced", forced}, {"at", at}, {"seconds", seconds}, {"budget_minutes", 10}, {"resources", resources},
                           {"command", "x"}, {"commit", "c"}, {"input_hash", "h"}, {"state", state}, {"started", started}};
    };
    return {item("r1", "qt-teams", 3, false, now - 300, 95, "medium", "running", now - 80),
            item("r2", "cargo-teams", QJsonValue(), true, now - 120, 240, "heavy", "queued", QJsonValue()),
            item("r3", "android", 4, false, now - 60, 1500, "heavy", "queued", QJsonValue())};
}

// The history: a flaky failure of qt-teams for #3, a reused cargo-teams, an
// android run past its budget for #4, and older passes.
QJsonArray resultsNow()
{
    const double now = nowSeconds();
    return {record("rec-a", "qt-teams", "failed", now - 900, 98000, 3,
                   {{"flaky", QJsonObject{{"runs", 10}, {"failed", 3}}},
                    {"excerpt", "FAIL!  : TestTeamsUi::aQueuedRowKeepsItsPlace() Compared values are not the same\n   Actual   (rows): 2\n   Expected (3): 3"}}),
            record("rec-b", "cargo-teams", "passed", now - 1800, 240000, 0, {{"reused", true}, {"reused_from", "rec-c"}}),
            record("rec-d", "android", "timeout", now - 3600, 1260000, 4, {{"over_budget", true}, {"budget_minutes", 20}}),
            record("rec-e", "qt-teams", "passed", now - 5400, 91000, 3),
            record("rec-c", "cargo-teams", "passed", now - 7200, 236000, 0),
            record("rec-f", "qt-teams", "passed", now - 9000, 90500, 3)};
}

void writeArray(const QString &path, const QJsonArray &rows)
{
    QVERIFY(FakeCli::replace(path, QJsonDocument(rows).toJson(QJsonDocument::Compact)));
}

TeamTestsView *testsViewOf(QWidget *root)
{
    return static_cast<TeamTestsView *>(root->findChild<QWidget *>("teamTestsView"));
}

template <typename T = QWidget>
T *withProperty(QWidget *root, const QString &name, const char *property, const QVariant &value)
{
    for (auto *found : root->findChildren<T *>(name)) if (found->property(property) == value && found->isVisible()) return found;
    return nullptr;
}
}

class TestTeamsUiTests : public QObject, public TeamsUiFixture {
    Q_OBJECT
private slots:
    void initTestCase() { setUpCase(); }
    void init() { setUp(); }
    void cleanup() { tearDown(); }
    void theTestsViewShowsTheQueueTheRunAndTheHistory();
    void aTasksDrawerListsItsRunsAndOpensTheirRecords();
    void eachRunNamesItsMachineAndAWaitingRunnerSaysWhy();
};

// #719: Tests beside Graph, Board, Layers and Profile: what runs now, the queue,
// each suite with its latest runs and Force run, the history with status,
// duration, the reused, flaky and over budget marks, and a run's excerpt only
// when its row opens. Reads are reads; Force run queues the suite as forced.
void TestTeamsUiTests::theTestsViewShowsTheQueueTheRunAndTheHistory()
{
    setTeam(phasedTeam());
    writeArray(script() + ".testqueue", queueNow());
    writeArray(script() + ".testresults", resultsNow());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1500, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *tests = nullptr;
    QTRY_VERIFY((tests = withProperty<QPushButton>(&workspace, QString(), "segment", "tests")));
    QCOMPARE(tests->text(), QStringLiteral("Tests"));
    QVERIFY(withProperty<QPushButton>(&workspace, QString(), "segment", "profile"));   // beside Profile
    tests->click();
    TeamTestsView *view = nullptr;
    QTRY_VERIFY((view = testsViewOf(&workspace)) && view->isVisible());
    QTRY_COMPARE(view->runs().size(), 6);
    QTRY_COMPARE(view->pending().size(), 3);
    // Reads change nothing: no request_id.
    const auto reads = calls(script(), "test");
    QVERIFY(!reads.isEmpty());
    for (const auto &call : reads) QVERIFY2(!call.contains("request_id"), qPrintable(QJsonDocument(call).toJson()));
    QCOMPARE(reads.first()["team"].toString(), QStringLiteral("team-1"));
    // What runs now and what waits.
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamTestsRunning").size(), 1);
    QCOMPARE(workspace.findChildren<QFrame *>("teamTestsQueued").size(), 2);
    const auto tip = [&](QWidget *root, const char *name) { auto *found = root ? root->findChild<QWidget *>(name) : nullptr; return found ? found->toolTip() : QStringLiteral("<none %1>").arg(name); };
    const auto text = [&](QWidget *root, const char *name) { auto *found = root ? root->findChild<QLabel *>(name) : nullptr; return found ? found->text() : QStringLiteral("<none %1>").arg(name); };
    QCOMPARE(tip(&workspace, "teamTestsRunningCount"), QStringLiteral("1 running"));
    QCOMPARE(tip(&workspace, "teamTestsWaitingCount"), QStringLiteral("2 waiting"));
    QCOMPARE(tip(&workspace, "teamTestsFailingCount"), QStringLiteral("2 suites failing"));
    QVERIFY(workspace.findChild<QWidget *>("teamTestsForced"));
    // The suites by name, each with its latest runs; the history newest first.
    QStringList suites;
    for (auto *row : workspace.findChildren<QFrame *>("teamTestsSuite")) suites << row->property("suite").toString();
    QCOMPARE(suites, (QStringList{"android", "cargo-teams", "qt-teams"}));
    QCOMPARE(workspace.findChildren<QWidget *>("teamTestsStrip").size(), 3);
    QStringList history;
    for (auto *row : workspace.findChildren<QFrame *>("teamTestsRun")) history << row->property("record").toString();
    QCOMPARE(history, (QStringList{"rec-a", "rec-b", "rec-d", "rec-e", "rec-c", "rec-f"}));
    // Status and duration, and the marks a record carries.
    auto *failed = withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-a");
    QCOMPARE(text(failed, "teamTestsStatus"), QStringLiteral("Failed"));
    QCOMPARE(text(failed, "teamTestsDuration"), QStringLiteral("1m 38s"));
    QCOMPARE(tip(failed, "teamTestsFlaky"), QStringLiteral("3 of the last 10 runs failed"));
    QVERIFY(tip(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-b"), "teamTestsReused").contains("rec-c"));
    auto *over = withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-d");
    QCOMPARE(text(over, "teamTestsStatus"), QStringLiteral("Timed out"));
    QCOMPARE(tip(over, "teamTestsOverBudget"), QStringLiteral("Took 21m 00s; its budget is 20 min"));
    QVERIFY(tip(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-e"), "teamTestsFlaky").startsWith("<none"));
    // The excerpt shows only when its row opens.
    QVERIFY(!workspace.findChild<QPlainTextEdit *>("teamTestsExcerpt"));
    QVERIFY(withProperty<QPushButton>(&workspace, "teamTestsRunToggle", "record", "rec-a"));
    withProperty<QPushButton>(&workspace, "teamTestsRunToggle", "record", "rec-a")->click();
    QTRY_VERIFY(workspace.findChild<QPlainTextEdit *>("teamTestsExcerpt"));
    QVERIFY(workspace.findChild<QPlainTextEdit *>("teamTestsExcerpt")->toPlainText().startsWith("FAIL!  : TestTeamsUi::aQueuedRowKeepsItsPlace()"));
    QCOMPARE(view->openRecord(), QStringLiteral("rec-a"));
    // No dots as separators in what the view says.
    for (auto *l : view->findChildren<QLabel *>()) QVERIFY2(!l->text().contains(QStringLiteral(" · ")), qPrintable(l->text()));
    if (shotsWanted()) {
        shot(&workspace, "u719-tests-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u719-tests-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
    // Force run: the forced cargo-teams already waits; android joins the queue as forced.
    QTRY_VERIFY(withProperty<QPushButton>(&workspace, "teamTestsForce", "suite", "cargo-teams"));
    auto *waiting = withProperty<QPushButton>(&workspace, "teamTestsForce", "suite", "cargo-teams");
    QCOMPARE(waiting->text(), QStringLiteral("Forced, waiting"));
    QVERIFY(!waiting->isEnabled());
    auto *force = withProperty<QPushButton>(&workspace, "teamTestsForce", "suite", "android");
    QVERIFY(force);
    QCOMPARE(force->text(), QStringLiteral("Force run"));
    force->click();
    QTRY_VERIFY(([&] {
        for (const auto &call : calls(script(), "test")) if (call["action"] == "submit") return true;
        return false;
    })());
    QJsonObject submit;
    for (const auto &call : calls(script(), "test")) if (call["action"] == "submit") submit = call;
    QCOMPARE(submit["suite"].toString(), QStringLiteral("android"));
    QVERIFY(submit["forced"].toBool());
    QVERIFY(submit.contains("request_id"));   // a change, bound to its request
    QTRY_COMPARE(view->pending().size(), 4);
    QTRY_VERIFY(withProperty<QPushButton>(&workspace, "teamTestsForce", "suite", "android")
                && withProperty<QPushButton>(&workspace, "teamTestsForce", "suite", "android")->text() == QStringLiteral("Forced, waiting"));
    // A refusal says what hgs answered.
    QVERIFY(FakeCli::replace(script() + ".testrefuse", "hgs: The test database is busy"));
    view->reload();
    QTRY_VERIFY(workspace.findChild<QLabel *>("teamTestsError"));
    QVERIFY(workspace.findChild<QLabel *>("teamTestsError")->text().contains("The test database is busy"));
    QFile::remove(script() + ".testrefuse");
    view->reload();
    QTRY_VERIFY(!workspace.findChild<QLabel *>("teamTestsError"));
}

// #719: a task's drawer lists the runs it asked for, newest first, with their
// status, duration and marks; Open record shows the record in the Tests view.
void TestTeamsUiTests::aTasksDrawerListsItsRunsAndOpensTheirRecords()
{
    setTeam(phasedTeam());
    writeArray(script() + ".testqueue", queueNow());
    writeArray(script() + ".testresults", resultsNow());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1500, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *tests = nullptr;
    QTRY_VERIFY((tests = withProperty<QPushButton>(&workspace, QString(), "segment", "tests")));
    tests->click();
    // A task link in the Tests view opens the task's drawer.
    QPushButton *link = nullptr;
    QTRY_VERIFY((link = withProperty<QPushButton>(&workspace, "teamTestsTask", "task", 3)));
    QVERIFY(link->toolTip().startsWith("#3 "));
    link->click();
    auto *drawer = static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"));
    QTRY_VERIFY(drawer->isVisible() && drawer->task() == 3);
    // Its runs, from the task's own read.
    QTRY_COMPARE(drawer->findChildren<QFrame *>("teamTaskTestRun").size(), 3);
    QStringList runs;
    for (auto *row : drawer->findChildren<QFrame *>("teamTaskTestRun")) runs << row->property("record").toString();
    QCOMPARE(runs, (QStringList{"rec-a", "rec-e", "rec-f"}));
    bool asked = false;
    for (const auto &call : calls(script(), "test")) asked = asked || (call["action"] == "results" && call["task"].toInt() == 3 && !call.contains("request_id"));
    QVERIFY(asked);
    auto *first = drawer->findChildren<QFrame *>("teamTaskTestRun").first();
    QVERIFY(first->findChildren<QLabel *>().size() >= 3);
    QStringList texts; for (auto *l : first->findChildren<QLabel *>()) texts << l->text();
    QVERIFY2(texts.contains("qt-teams") && texts.contains("Failed") && texts.contains("1m 38s"), qPrintable(texts.join('|')));
    // A task with no run shows no section.
    if (shotsWanted()) shot(&workspace, "u719-drawer-runs-dark");
    // Open record: the Tests view with that record's row open.
    QVERIFY(withProperty<QPushButton>(drawer, "teamTaskTestRecord", "record", "rec-e"));
    withProperty<QPushButton>(drawer, "teamTaskTestRecord", "record", "rec-e")->click();
    auto *view = testsViewOf(&workspace);
    QTRY_VERIFY(view && view->isVisible() && !drawer->isVisible());
    QCOMPARE(view->openRecord(), QStringLiteral("rec-e"));
    QTRY_VERIFY(workspace.findChild<QPlainTextEdit *>("teamTestsExcerpt"));
    QCOMPARE(workspace.findChild<QPlainTextEdit *>("teamTestsExcerpt")->toPlainText(), QStringLiteral("qt-teams passed in 91000 ms"));
    if (shotsWanted()) {
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u719-tests-record-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
}

// U62 (#733): a run on a test runner names that Machine, one here says this
// computer; a suite whose runner is unreachable (or missing) waits in the
// queue with the platform it needs and the reason; #718's flags show as marks.
void TestTeamsUiTests::eachRunNamesItsMachineAndAWaitingRunnerSaysWhy()
{
    setTeam(phasedTeam());
    const double now = nowSeconds();
    auto queue = queueNow();
    auto waiting = queue[2].toObject();
    waiting["platforms"] = QJsonArray{"linux"};
    waiting["reason"] = "test runner box unreachable: ssh: connect to host box.example port 22: Connection refused; it waits for it";
    queue[2] = waiting;
    auto none = queue[1].toObject();
    none["id"] = "r4"; none["suite"] = "macos-ui"; none["forced"] = false; none["platforms"] = QJsonArray{"macos"};
    none["reason"] = "no test runner for macos: the owner makes a Machine one (Settings > Machines)";
    queue.append(none);
    writeArray(script() + ".testqueue", queue);
    writeArray(script() + ".testresults", QJsonArray{
        record("rec-l", "android", "passed", now - 600, 300000, 4, {{"runner", "box"}}),
        record("rec-h", "qt-teams", "flaky", now - 1200, 120000, 3, {{"flags", QJsonArray{"retried", "slower"}}, {"recorded_ms", 90000}}),
        record("rec-r", "cargo-teams", "passed", now - 1800, 1000, 0, {{"flags", QJsonArray{"reused"}}, {"reused_from", "rec-c"}, {"due", true}}),
        record("rec-x", "qt-teams", "failed", now - 2400, 5000, 3, {{"flags", QJsonArray{"crash", "over_time_budget"}}})});
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1500, 980); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QPushButton *tests = nullptr;
    QTRY_VERIFY((tests = withProperty<QPushButton>(&workspace, QString(), "segment", "tests")));
    tests->click();
    TeamTestsView *view = nullptr;
    QTRY_VERIFY((view = testsViewOf(&workspace)) && view->isVisible());
    QTRY_COMPARE(view->runs().size(), 4);
    QTRY_COMPARE(view->pending().size(), 4);
    const auto tip = [](QWidget *root, const char *name) { auto *found = root ? root->findChild<QWidget *>(name) : nullptr; return found ? found->toolTip() : QStringLiteral("<none %1>").arg(name); };
    const auto text = [](QWidget *root, const char *name) { auto *found = root ? root->findChild<QLabel *>(name) : nullptr; return found ? found->text() : QStringLiteral("<none %1>").arg(name); };
    // Each run's machine.
    QTRY_VERIFY(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-l"));
    QCOMPARE(text(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-l"), "teamTestsRunner"), QStringLiteral("on box"));
    QCOMPARE(text(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-h"), "teamTestsRunner"), QStringLiteral("on this computer"));
    // #718's flags as marks.
    auto *flaky = withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-h");
    QCOMPARE(text(flaky, "teamTestsStatus"), QStringLiteral("Passed on retry"));
    QCOMPARE(tip(flaky, "teamTestsFlaky"), QStringLiteral("Failed, then passed on its automatic retry"));
    QCOMPARE(tip(flaky, "teamTestsSlower"), QStringLiteral("Took 2m 00s; recorded 1m 30s"));
    QVERIFY(tip(withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-r"), "teamTestsReused").endsWith("due for re-measuring"));
    auto *crash = withProperty<QFrame>(&workspace, "teamTestsRun", "record", "rec-x");
    QVERIFY(!tip(crash, "teamTestsCrash").startsWith("<none"));
    QVERIFY(!tip(crash, "teamTestsOverBudget").startsWith("<none"));
    // The queue: the platform it needs and why it waits.
    auto *unreachable = withProperty<QFrame>(&workspace, "teamTestsQueued", "request", "r3");
    QVERIFY(unreachable);
    QCOMPARE(text(unreachable, "teamTestsReason"), waiting["reason"].toString());
    QCOMPARE(unreachable->findChild<QWidget *>("teamTestsPlatform")->toolTip(),
             QStringLiteral("Runs only on linux: here when this computer serves it, else on a test runner of it"));
    QVERIFY(text(withProperty<QFrame>(&workspace, "teamTestsQueued", "request", "r4"), "teamTestsReason").startsWith("no test runner for macos"));
    QCOMPARE(text(withProperty<QFrame>(&workspace, "teamTestsQueued", "request", "r2"), "teamTestsReason"), QStringLiteral("<none teamTestsReason>"));
    QCOMPARE(tip(&workspace, "teamTestsRunnerWaitCount"), QStringLiteral("2 waiting for a runner"));
    // The drawer names the machine too.
    withProperty<QPushButton>(&workspace, "teamTestsTask", "task", 4)->click();
    auto *drawer = static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"));
    QTRY_VERIFY(drawer->isVisible() && drawer->task() == 4 && drawer->findChild<QLabel *>("teamTaskTestRunner"));
    QCOMPARE(drawer->findChild<QLabel *>("teamTaskTestRunner")->text(), QStringLiteral("on box"));
    drawer->findChild<QPushButton *>("teamTaskTestRecord")->click();
    QTRY_VERIFY(!drawer->isVisible());
    // No dots as separators.
    for (auto *l : view->findChildren<QLabel *>()) QVERIFY2(!l->text().contains(QStringLiteral(" · ")), qPrintable(l->text()));
    if (shotsWanted()) {
        shot(&workspace, "u733-tests-runners-dark");
        workspace.setTheme(false); workspace.render();
        lightShot(&workspace, "u733-tests-runners-light");
        darkPalette(); workspace.setTheme(true); workspace.render();
    }
}

QTEST_MAIN(TestTeamsUiTests)
#include "test_teamsui_tests.moc"
