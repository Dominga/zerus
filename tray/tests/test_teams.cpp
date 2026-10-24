// Agent teams in the desktop:
// the model read from `hgs team`, the controller's polls and changes, and the
// inspector's Team tab.
#include <QtTest>
#include <QApplication>
#include <QComboBox>
#include <QJsonDocument>
#include <QLabel>
#include <QPushButton>
#include <QScrollArea>
#include <QSettings>
#include <QTemporaryDir>
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamModel.h"
#include "TeamPanel.h"
#include "support/FakeCli.h"

using namespace Teams;

namespace {
// The design's test team: Checkout redesign, six members, ten tasks.
QJsonObject checkout()
{
    return QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Checkout redesign","status":"running","project":"Checkout","folder":"/work/checkout","lead":"lead",
 "revision":12,"created":1000,"updated":5000,"settings":{"all_hard":false,"report_timeout_seconds":120},
 "members":[
  {"id":"lead","role":"Architect","agent":"claude","model":"Opus","state":"active","session":{"host":"","name":"claude/checkout/lead"}},
  {"id":"backend","role":"Backend","agent":"codex","model":"gpt-5.5-codex","state":"active","session":{"host":"","name":"codex/checkout/backend"}},
  {"id":"frontend","role":"Frontend","agent":"dsh","model":"deepseek-v4","state":"active","session":{"host":"","name":"dsh/checkout/frontend"}},
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2","computer":"devbox","state":"active","session":{"host":"devbox","name":"kimi/checkout/tester"}},
  {"id":"reviewer","role":"Reviewer","agent":"claude","model":"Sonnet","state":"active","session":{"host":"","name":"claude/checkout/reviewer"}},
  {"id":"integrator","role":"Integrator","agent":"opencode","model":"qwen-coder","state":"active","session":{"host":"","name":"opencode/checkout/integrator"}},
  {"id":"gone","role":"Writer","agent":"codex","state":"removed"}],
 "tasks":[
  {"id":1,"title":"Requirements and acceptance","assignee":"lead","status":"accepted","decided_by":"lead",
   "attempts":[{"n":1,"id":"a1","member":"lead","state":"reported","report":{"text":"Done","at":1100}}]},
  {"id":2,"title":"Address form UI","assignee":"frontend","status":"submitted","deps":[{"task":1,"kind":"hard"}],
   "attempts":[{"n":1,"id":"a2","member":"frontend","state":"reported","notice":"sent","report":{"text":"Form ready","at":1200}}]},
  {"id":3,"title":"Saved cards API","assignee":"backend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a3","member":"backend","state":"rejected"},{"n":2,"id":"a3b","member":"backend","state":"delivered","sent_at":1300}]},
  {"id":4,"title":"Payment token storage","assignee":"backend","status":"pending","deps":[{"task":3}]},
  {"id":5,"title":"Checkout page wiring","assignee":"frontend","status":"pending","deps":[{"task":2},{"task":3}]},
  {"id":6,"title":"Unit tests","assignee":"tester","status":"assigned","deps":[{"task":3,"kind":"soft"}],
   "attempts":[{"n":1,"id":"a6","member":"tester","state":"unconfirmed","error":"devbox stopped answering"}]},
  {"id":7,"title":"E2E tests","assignee":"tester","status":"pending","deps":[{"task":5}]},
  {"id":8,"title":"Code review","assignee":"reviewer","status":"pending","deps":[{"task":4},{"task":5},{"task":6}]},
  {"id":9,"title":"Merge branches","assignee":"integrator","status":"pending","deps":[{"task":8}]},
  {"id":10,"title":"Release notes","assignee":"lead","status":"pending","deps":[{"task":9}]}],
 "layers":{"1":0,"2":1,"3":1,"4":2,"5":2,"6":2,"7":3,"8":3,"9":4,"10":5},"cycle":[],"ready":[],
 "validation":{"errors":[],"warnings":[{"kind":"offline","text":"devbox is offline"}]}})").object();
}

QJsonObject feedPage()
{
    return QJsonDocument::fromJson(R"({"items":[
 {"seq":9,"at":4800,"kind":"unconfirmed","detail":{"task":6,"attempt":1,"error":"devbox stopped answering"}},
 {"seq":8,"at":4700,"kind":"reported","detail":{"by":"Frontend","task":2,"title":"Address form UI","attempt":1}},
 {"seq":7,"at":4600,"kind":"handed_out","detail":{"by":"Lead","task":3,"to":"Backend","attempt":2}},
 {"seq":6,"at":4500,"kind":"rejected","detail":{"by":"Lead","task":3,"title":"Saved cards API","attempt":1,"reason":"cards list must paginate"}}],
 "next_cursor":6})").object();
}

Live live(const Member &member)
{
    Live state; state.known = true;
    if (member.id == "backend") { state.attention = true; state.attentionText = "Approval: Run command: pnpm prisma migrate dev"; }
    if (member.id == "tester") state.offline = true;
    if (member.id == "lead") state.working = true;
    return state;
}
}

class TestTeams : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void modelReadsTheTeamAndDerivesMemberStates();
    void aRunningTeamIsStuckWhenNothingCanMove();
    void feedLinesAndChains();
    void controllerReadsOnceAndSendsChangesAsYou();
    void panelShowsProblemsWithTheirActions_data();
    void panelShowsProblemsWithTheirActions();
private:
    QString script() const { return m_dir.filePath("hgs"); }
    QTemporaryDir m_dir;
};

void TestTeams::initTestCase()
{
    QVERIFY(m_dir.isValid());
    TeamController::setPollInterval(60000);
    QFile status(script() + ".status"); QVERIFY(status.open(QIODevice::WriteOnly)); status.write(QJsonDocument(checkout()).toJson(QJsonDocument::Compact));
    status.close();
    QFile feed(script() + ".feed"); QVERIFY(feed.open(QIODevice::WriteOnly)); feed.write(QJsonDocument(feedPage()).toJson(QJsonDocument::Compact));
    feed.close();
    QVERIFY(FakeCli::install(script(), R"PY(#!/usr/bin/env python3
import json,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
body=json.load(sys.stdin)
open(__file__+'.calls','a').write(json.dumps({'args':a,'input':body})+'\n')
if a[:2]==['team','list']:
 t=json.load(open(__file__+'.status'))
 for key in ('layers','cycle','ready','validation'):t.pop(key)
 print(json.dumps({'items':[t]}))
elif a[:2]==['team','status']:sys.stdout.write(open(__file__+'.status').read())
elif a[:2]==['team','events']:sys.stdout.write(open(__file__+'.feed').read())
elif a[:2]==['team','reassign'] and body.get('member')=='gone':
 print('hgs: Unknown member gone',file=sys.stderr);sys.exit(1)
else:print(json.dumps({'status':'ok','request_id':body.get('request_id')}))
)PY"));
}

void TestTeams::modelReadsTheTeamAndDerivesMemberStates()
{
    const auto team = parse(checkout());
    QVERIFY(team.full);
    QCOMPARE(team.members.size(), 6);   // the removed member is left out
    QVERIFY(team.member("lead")->lead);
    QCOMPARE(team.roleOf("lead"), QStringLiteral("Lead"));
    QCOMPARE(team.layers.value(10), 5);
    QCOMPARE(team.task(3)->attempts.size(), 2);
    QCOMPARE(team.memberOfSession("devbox", "kimi/checkout/tester")->id, QStringLiteral("tester"));
    QVERIFY(!team.memberOfSession({}, "kimi/checkout/tester"));
    const auto status = [&](const QString &id) { return memberStatus(team, *team.member(id), live(*team.member(id))); };
    // A question outranks the work it interrupts; offline outranks an unconfirmed hand-off.
    QCOMPARE(status("backend").kind, QStringLiteral("answer"));
    QCOMPARE(status("backend").task, 3);
    QCOMPARE(status("tester").kind, QStringLiteral("offline"));
    QVERIFY(status("tester").problem);
    QCOMPARE(status("frontend").kind, QStringLiteral("submitted"));
    QCOMPARE(status("reviewer").kind, QStringLiteral("idle"));
    QCOMPARE(status("lead").kind, QStringLiteral("working"));
    auto quiet = live(*team.member("tester")); quiet.offline = false;
    QCOMPARE(memberStatus(team, *team.member("tester"), quiet).kind, QStringLiteral("unconfirmed"));
    QCOMPARE(taskStatus(*team.task(3)).text, QStringLiteral("In progress · attempt 2"));
    QCOMPARE(taskStatus(*team.task(6)).tone, Tone::Danger);
    QCOMPARE(accepted(team), 1);
    QCOMPARE(answers(team, live), 1);
    QCOMPARE(problems(team, live).size(), 2);
    // A list summary has no layers: they are computed the same way.
    auto summary = checkout(); summary.remove("layers"); summary.remove("ready");
    QCOMPARE(parse(summary).layers, team.layers);
    // Planning members show what approval creates.
    auto planning = checkout(); planning["status"] = "planning";
    auto planned = parse(planning); planned.members[2].session.clear();
    QCOMPARE(memberStatus(planned, planned.members[2], {}).text, QStringLiteral("Created on approval"));
}

void TestTeams::aRunningTeamIsStuckWhenNothingCanMove()
{
    auto team = parse(checkout());
    QCOMPARE(displayStatus(team, live), QStringLiteral("running"));   // the lead decides #2
    for (auto &task : team.tasks) if (task.id == 2) task.status = "accepted";
    // #3 waits on a question, #6 on an unconfirmed hand-off, the rest on them.
    QCOMPARE(displayStatus(team, live), QStringLiteral("stuck"));
    QVERIFY(nextLine(team).contains("wait for #3 Saved cards API"));
    const auto calm = [](const Member &) { Live state; state.known = true; return state; };
    QCOMPARE(displayStatus(team, calm), QStringLiteral("running"));
    team.status = "paused";
    QCOMPARE(displayStatus(team, live), QStringLiteral("paused"));
}

void TestTeams::feedLinesAndChains()
{
    const auto team = parse(checkout());
    const auto items = feedPage().value("items").toArray();
    QCOMPARE(feedLine(team, items[2].toObject()).text, QStringLiteral("Task to Backend · #3 Saved cards API · attempt 2"));
    QCOMPARE(feedLine(team, items[0].toObject()).tone, Tone::Danger);
    QVERIFY(feedLine(team, items[3].toObject()).text.contains("cards list must paginate"));
    const auto chain = Teams::chain(team, 3);
    QVERIFY(chain.contains(1) && chain.contains(10) && chain.contains(6));
    QVERIFY(!chain.contains(2) && chain.contains(7));   // #7 follows #5, which follows #3
}

void TestTeams::controllerReadsOnceAndSendsChangesAsYou()
{
    QFile::remove(script() + ".calls");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    QSignalSpy teams(&controller, &TeamController::teamsChanged);
    controller.refresh();
    QTRY_COMPARE(teams.count(), 1);
    QCOMPARE(controller.teams().size(), 1);
    QVERIFY(controller.teamOfSession("devbox", "kimi/checkout/tester"));
    controller.refresh();
    QTRY_VERIFY(QFile(script() + ".calls").size() > 0);
    QTest::qWait(600);
    QCOMPARE(teams.count(), 1);   // an unchanged answer changes nothing
    controller.watch("team-1");
    QTRY_VERIFY(controller.watched());
    QTRY_COMPARE(controller.feed().size(), 4);
    QVERIFY(controller.feedHasOlder());
    bool done = false, ok = false; QString message;
    controller.act("reassign", {{"task", 6}, {"member", "gone"}}, [&](bool success, const QString &text) { done = true; ok = success; message = text; });
    QTRY_VERIFY(done);
    QVERIFY(!ok);
    QVERIFY(message.contains("Unknown member"));
    done = false;
    controller.act("pause", {}, [&](bool success, const QString &) { done = true; ok = success; });
    QTRY_VERIFY(done);
    QVERIFY(ok);
    QFile calls(script() + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QJsonObject pause;
    for (const auto &line : calls.readAll().split('\n'))
        if (const auto call = QJsonDocument::fromJson(line).object(); call["args"].toArray().at(1) == "pause") pause = call["input"].toObject();
    QCOMPARE(pause["by"].toString(), QStringLiteral("human"));
    QCOMPARE(pause["team"].toString(), QStringLiteral("team-1"));
    QVERIFY(!QUuid(pause["request_id"].toString()).isNull());
}

void TestTeams::panelShowsProblemsWithTheirActions_data()
{
    QTest::addColumn<int>("width");
    QTest::addColumn<bool>("dark");
    QTest::addColumn<bool>("stuck");
    QTest::newRow("420-dark") << 420 << true << false;
    QTest::newRow("420-light") << 420 << false << false;
    QTest::newRow("290-dark") << 290 << true << false;
    QTest::newRow("290-light-stuck") << 290 << false << true;
    QTest::newRow("420-dark-stuck") << 420 << true << true;
}

void TestTeams::panelShowsProblemsWithTheirActions()
{
    QFETCH(int, width); QFETCH(bool, dark); QFETCH(bool, stuck);
    auto team = checkout();
    if (stuck) { auto tasks = team["tasks"].toArray(); auto two = tasks[1].toObject(); two["status"] = "accepted"; tasks[1] = two; team["tasks"] = tasks; }
    QVERIFY(FakeCli::replace(script() + ".status", QJsonDocument(team).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (!shots.isEmpty()) {
        // Pictures only: the window's stylesheet is not here, so a plain palette stands in.
        QApplication::setStyle("Fusion");
        QPalette palette;
        palette.setColor(QPalette::Window, QColor(dark ? "#15191f" : "#f5f7f9"));
        palette.setColor(QPalette::WindowText, QColor(dark ? "#e6e8eb" : "#1a2733"));
        palette.setColor(QPalette::Base, QColor(dark ? "#1b1f26" : "#ffffff"));
        palette.setColor(QPalette::Button, QColor(dark ? "#232830" : "#edf2f5"));
        palette.setColor(QPalette::ButtonText, QColor(dark ? "#e6e8eb" : "#1a2733"));
        palette.setColor(QPalette::Text, QColor(dark ? "#e6e8eb" : "#1a2733"));
        QApplication::setPalette(palette);
    }
    panel.setTheme(dark);
    panel.setLive(live);
    int answered = -1;
    panel.answersChanged = [&](int n) { answered = n; };
    panel.resize(width, 1900);
    panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    // A running team reads as a summary; its work is in the Team tab.
    QTRY_COMPARE(panel.findChildren<QWidget *>("teamSummaryMember").size(), 6);
    QCOMPARE(answered, 1);
    QCOMPARE(bool(panel.findChild<QWidget *>("teamStuck")), stuck);
    QVERIFY(panel.findChild<QPushButton *>("teamGoToQuestion"));
    QVERIFY(panel.findChild<QPushButton *>("teamOpenTeam"));
    // Nothing is wider than the panel.
    QVERIFY(panel.findChild<QWidget *>("teamPanelContent")->minimumSizeHint().width() <= width);
    // Optional: ZERUS_TEAM_SHOTS=<dir> keeps a picture for review against the design.
    if (!shots.isEmpty()) {
        auto *content = panel.findChild<QWidget *>("teamPanelContent");
        content->resize(width, content->sizeHint().height());
        content->grab().save(QDir(shots).filePath(QStringLiteral("team-panel-%1.png").arg(QTest::currentDataTag())));
    }
}

QTEST_MAIN(TestTeams)
#include "test_teams.moc"
