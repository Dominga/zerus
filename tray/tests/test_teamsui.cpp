// WhiteWh fork: agent teams in the desktop, M3–M6 (design/agent-teams S3, S4,
// S6, S7, S10, S11): the task card, planning edits, New team, the catalog
// editor, the Team designer's draft and the team workspace. Each sends one
// `hgs team` request per change, recorded by a fake CLI.
#include <QPointer>
#include <QScrollBar>
#include <QtTest>
#include <algorithm>
#include <QStyle>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QSplitter>
#include <QTemporaryDir>
#include <QTextBlock>
#include <QTextBrowser>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>
#include "HgsClient.h"
#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "TeamDesignerPage.h"
#include "TeamDesignerPanel.h"
#include "TeamModel.h"
#include "TeamNewDialog.h"
#include "TeamNewTab.h"
#include "TeamPanel.h"
#include "TeamTaskCard.h"
#include "TeamWidgets.h"
#include "TeamWorkspace.h"
#include "support/FakeCli.h"

using namespace Teams;

namespace {
QJsonObject team(const QString &status = "running")
{
    auto t = QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Checkout redesign","status":"running","project":"Checkout","folder":"/work/checkout","lead":"lead",
 "revision":12,"created":1000,"updated":5000,"settings":{},
 "members":[
  {"id":"lead","role":"Architect","agent":"claude","model":"Opus","state":"active","session":{"host":"","name":"claude/checkout/lead"}},
  {"id":"backend","role":"Backend","agent":"codex","model":"gpt-5.5-codex","state":"active","session":{"host":"","name":"codex/checkout/backend"}},
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2","computer":"msi-old","state":"active","session":{"host":"msi-old","name":"kimi/checkout/tester"}}],
 "tasks":[
  {"id":1,"title":"Requirements","assignee":"lead","status":"accepted","attempts":[{"n":1,"id":"a1","member":"lead","state":"reported","report":{"text":"Done","at":1100}}]},
  {"id":2,"title":"Address form UI","assignee":"backend","status":"submitted","deps":[{"task":1}],"criteria":["Validation messages","Keyboard flow"],
   "description":"New address form.","attempts":[{"n":1,"id":"a2","member":"backend","state":"reported","sent_at":1200,"report":{"text":"Form ready in src/forms","at":1300}}]},
  {"id":3,"title":"Saved cards API","assignee":"backend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a3","member":"backend","state":"rejected"},{"n":2,"id":"a3b","member":"backend","state":"delivered","sent_at":1400}]},
  {"id":4,"title":"Unit tests","assignee":"tester","status":"assigned","deps":[{"task":3,"kind":"soft"}],
   "attempts":[{"n":1,"id":"a4","member":"tester","state":"unconfirmed","sent_at":1500}]},
  {"id":5,"title":"Code review","assignee":"backend","status":"pending","deps":[{"task":2},{"task":4}]}],
 "layers":{"1":0,"2":1,"3":1,"4":2,"5":3},"cycle":[],"ready":[],"validation":{"errors":[],"warnings":[]}})").object();
    if (status == "planning") {
        t["status"] = "planning";
        auto members = t["members"].toArray();
        for (int i = 1; i < members.size(); ++i) { auto m = members[i].toObject(); m.remove("session"); m["state"] = "not_created"; members[i] = m; }
        t["members"] = members;
        auto tasks = t["tasks"].toArray();
        for (int i = 0; i < tasks.size(); ++i) { auto k = tasks[i].toObject(); k["status"] = "draft"; k.remove("attempts"); tasks[i] = k; }
        auto extra = QJsonObject{{"id", 6}, {"title", "E2E tests"}, {"status", "draft"}};
        tasks.append(extra);
        t["tasks"] = tasks;
        t["validation"] = QJsonObject{{"errors", QJsonArray{QJsonObject{{"kind", "assignee"}, {"task", 6}, {"text", "#6 E2E tests: no assignee"}}}}, {"warnings", QJsonArray()}};
        auto layers = t["layers"].toObject(); layers["6"] = 0; t["layers"] = layers;
    }
    return t;
}

// A running team with two phases appended: phase 1 is finished (it starts
// collapsed), phase 2 has a barrier, phase 3 waits as a draft.
QJsonObject phasedTeam()
{
    auto t = team();
    t["settings"] = QJsonObject{{"all_hard", false}, {"approval", "review"}, {"phase_approval", "review"}};
    t["waiting_for_lead"] = false;
    t["pending_phase"] = 3;
    t["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"approval", "review"}, {"state", "approved"},
                    {"tasks", QJsonArray{3, 4}}, {"added_by", "Lead"}, {"added_at", 1500.0},
                    {"decided_by", "You"}, {"decided_at", 1600.0}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"barrier", false}, {"approval", "review"}, {"state", "draft"},
                    {"tasks", QJsonArray{5}}, {"added_by", "Lead"}, {"added_at", 1700.0}}};
    t["tasks"] = QJsonArray{
        QJsonObject{{"id", 1}, {"phase", 1}, {"title", "Requirements"}, {"assignee", "lead"}, {"status", "accepted"},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "lead"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 2}, {"phase", 1}, {"title", "Checkout design"}, {"assignee", "backend"}, {"status", "accepted"}},
        QJsonObject{{"id", 3}, {"phase", 2}, {"title", "Address form UI"}, {"assignee", "backend"}, {"status", "accepted"},
                    {"deps", QJsonArray{QJsonObject{{"task", 2}}}},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a3"}, {"member", "backend"}, {"state", "reported"}}}}},
        QJsonObject{{"id", 4}, {"phase", 2}, {"title", "Unit tests"}, {"assignee", "tester"}, {"status", "in_progress"},
                    {"deps", QJsonArray{QJsonObject{{"task", 3}, {"kind", "soft"}}}},
                    {"attempts", QJsonArray{QJsonObject{{"n", 1}, {"id", "a4"}, {"member", "tester"}, {"state", "delivered"}, {"sent_at", 1600.0}}}}},
        QJsonObject{{"id", 5}, {"phase", 3}, {"title", "Code review"}, {"assignee", "backend"}, {"status", "draft"},
                    {"deps", QJsonArray{QJsonObject{{"task", 4}}}}}};
    t["layers"] = QJsonObject{{"1", 0}, {"2", 0}, {"3", 0}, {"4", 1}, {"5", 2}};
    t["ready"] = QJsonArray{4};
    t["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
    return t;
}

// The delivery states of the spec (A1-A6): one task per attempt phase, a
// question, a held task, a replaced report and a captured one.
QJsonObject deliveryTeam()
{
    return QJsonDocument::fromJson(R"({
 "id":"team-2","name":"Delivery","status":"running","project":"Checkout","folder":"/work/delivery","lead":"lead",
 "revision":3,"created":1000,"updated":5000,"settings":{},
 "members":[
  {"id":"lead","role":"Architect","agent":"claude","state":"active","session":{"host":"","name":"claude/delivery/lead"}},
  {"id":"backend","role":"Backend","agent":"codex","state":"active","session":{"host":"","name":"codex/delivery/backend"}},
  {"id":"tester","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/delivery/tester"}}],
 "tasks":[
  {"id":11,"title":"Pending hand-off","assignee":"backend","status":"assigned",
   "attempts":[{"n":1,"id":"s1","member":"backend","state":"delivering","sent_at":1700}]},
  {"id":12,"title":"In the terminal","assignee":"backend","status":"assigned",
   "attempts":[{"n":1,"id":"s2","member":"backend","state":"delivered","sent_at":1700,"delivered_at":1701}]},
  {"id":13,"title":"Late start","assignee":"backend","status":"in_progress",
   "attempts":[{"n":1,"id":"s3","member":"backend","state":"delivered","delivered_at":1700,"start_warned":true}]},
  {"id":14,"title":"Building","assignee":"backend","status":"in_progress",
   "attempts":[{"n":2,"id":"s4","member":"backend","state":"delivered","delivered_at":1700,"started_at":1710}]},
  {"id":15,"title":"Asks the lead","assignee":"tester","status":"in_progress",
   "attempts":[{"n":1,"id":"s5","member":"tester","state":"delivered","delivered_at":1700,"started_at":1710,
    "questions":[{"id":"q1","text":"Must I keep the old field?","at":1720,"answer":"","answered_at":null,"by":""}]}]},
  {"id":16,"title":"Held","assignee":"tester","status":"pending",
   "hold":{"by":"Lead","at":1730,"reason":"the member is busy"},
   "attempts":[{"n":1,"id":"s6","member":"tester","state":"rejected"}]},
  {"id":17,"title":"Reported","assignee":"tester","status":"submitted",
   "attempts":[{"n":1,"id":"s7","member":"tester","state":"reported","delivered_at":1700,"started_at":1701,
    "report":{"text":"Done: the tests pass","at":1740,"captured":false,
              "replaced":[{"text":"Building and running the tests...","at":1735}]}}]},
  {"id":18,"title":"No report","assignee":"lead","status":"in_progress",
   "attempts":[{"n":1,"id":"s8","member":"lead","state":"delivered","delivered_at":1700,"turn_ended_at":1750,"no_report":true}]},
  {"id":19,"title":"Stopped","assignee":"lead","status":"in_progress",
   "attempts":[{"n":1,"id":"s9","member":"lead","state":"delivered","delivered_at":1700,"started_at":1701,"phase":"stopped"}]},
  {"id":20,"title":"Rejected","assignee":"backend","status":"rejected",
   "attempts":[{"n":1,"id":"s10","member":"backend","state":"rejected","phase":null}]},
  {"id":21,"title":"Accepted work","assignee":"backend","status":"accepted",
   "attempts":[{"n":1,"id":"s12","member":"backend","state":"reported","phase":null,
                "report":{"text":"Done long ago","at":1750}}]},
  {"id":22,"title":"Captured","assignee":"backend","status":"submitted",
   "attempts":[{"n":1,"id":"s11","member":"backend","state":"reported","report":{"text":"Captured reply","at":1760,"captured":true}}]},
  {"id":23,"title":"Park me","assignee":"backend","status":"pending"},
  {"id":24,"title":"Retried once","assignee":"backend","status":"pending",
   "attempts":[{"n":1,"id":"s13","member":"backend","state":"revoked","phase":null},
               {"n":2,"id":"s14","member":"backend","state":"refused","phase":null}]}],
 "layers":{"11":0,"12":1,"13":2,"14":2,"15":2,"16":2,"17":2,"18":2,"19":2,"20":2,"21":2,"22":2,"23":2,"24":2},
 "cycle":[],"ready":[],"validation":{"errors":[],"warnings":[]}})").object();
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

QJsonObject catalog()
{
    return QJsonDocument::fromJson(R"({"builtin":false,
 "catalog":{"version":3,
  "roles":[{"id":"lead","name":"Lead","agent":"claude","prompt":"You lead."},
           {"id":"backend","name":"Backend","agent":"codex","folder":"worktree","prompt":"Build the API."},
           {"id":"tester","name":"Tester","agent":"kimi","model":"kimi-k2","folder":"worktree","prompt":"Test it."}],
  "templates":[{"id":"feature","name":"Feature delivery","lead_protocol":"Plan in waves of three","roster":[{"role":"backend","count":1},{"role":"tester","count":1}],
                "tasks":[{"title":"Plan","role":"lead","deps":[]},{"title":"Build","role":"backend","deps":[0]},{"title":"Test","role":"tester","deps":[1]}],
                "planning":"template","approval":"review","phase_approval":"auto","max_members":6}],
  "routing":[]},
 "draft":{"base":3,"proposals":[
  {"id":"p1","kind":"role","action":"change","target":"tester","title":"Tester: Kimi → Claude Sonnet","reason":"Deliveries to msi-old fail",
   "value":{"id":"tester","name":"Tester","agent":"claude","model":"Sonnet","folder":"worktree","prompt":"Test it."},"state":"proposed"},
  {"id":"p2","kind":"role","action":"add","target":"security","title":"New role: Security","reason":"Payments code",
   "value":{"id":"security","name":"Security","agent":"claude","prompt":"Look for secrets."},"state":"kept"}]}})").object();
}

Live live(const Member &member)
{
    Live state; state.known = true;
    if (member.id == "tester") state.offline = false;
    return state;
}

QList<QJsonObject> calls(const QString &script, const QString &operation)
{
    QList<QJsonObject> found;
    QFile file(script + ".calls");
    if (!file.open(QIODevice::ReadOnly)) return found;
    for (const auto &line : file.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).object();
        QStringList args; for (const auto &a : call["args"].toArray()) args << a.toString();
        if (args.mid(1, operation.count(' ') + 1).join(' ') == operation) found << call["input"].toObject();
    }
    return found;
}

void shot(QWidget *widget, const QString &name)
{
    const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (dir.isEmpty()) return;
    QTest::qWait(50);
    const QString path = QDir(dir).filePath(name + ".png");
    if (!widget->grab().save(path)) qWarning() << "screenshot not saved:" << path;
}

void darkPalette()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) return;
    QApplication::setStyle("Fusion");
    QPalette p;
    p.setColor(QPalette::Window, QColor("#15191f")); p.setColor(QPalette::WindowText, QColor("#e6e8eb"));
    p.setColor(QPalette::Base, QColor("#1b1f26")); p.setColor(QPalette::Button, QColor("#232830"));
    p.setColor(QPalette::ButtonText, QColor("#e6e8eb")); p.setColor(QPalette::Text, QColor("#e6e8eb"));
    p.setColor(QPalette::PlaceholderText, QColor("#8b98a8"));
    QApplication::setPalette(p);
}

// The light half of the shots; no effect without ZERUS_TEAM_SHOTS, like darkPalette().
void lightPalette()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) return;
    QApplication::setStyle("Fusion");
    QPalette p;
    p.setColor(QPalette::Window, QColor("#f5f7f9")); p.setColor(QPalette::WindowText, QColor("#1a2733"));
    p.setColor(QPalette::Base, QColor("#ffffff")); p.setColor(QPalette::Button, QColor("#edf2f5"));
    p.setColor(QPalette::ButtonText, QColor("#1a2733")); p.setColor(QPalette::Text, QColor("#1a2733"));
    p.setColor(QPalette::PlaceholderText, QColor("#627082"));
    QApplication::setPalette(p);
}
}

class TestTeamsUi : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void init();
    void catalogModelAppliesKeptProposals();
    void taskCardOffersActionsByStatus();
    void attemptStatesShowPhaseIcons();
    void taskCardAnswersQuestionsAndHolds();
    void memberQueueListsCurrentAndReturned();
    void planningEditsSendTheWholePlan();
    void newTeamStartsTheLeadWithTheTemplate();
    void catalogEditorSavesTheNextVersion();
    void catalogRoleChoosesAccountModelAndComputer();
    void designerDraftIsKeptThenApplied();
    void workspaceShowsGraphBoardAndFocus();
    void workspaceGroupsTasksByPhase();
    void phaseApprovalAndFinishAskTheOwner();
    void leftoverMembersAreShownApart();
    void memberEditorPicksAnExistingSession();
    void severalDraftPhasesRenderAndApproveInOrder();
    void draftReorderSendsMovePhase();
    void pinMarkAndFinishedTeamShowWhoFinished();
    void catalogEditorCarriesNewPhaseApproval();
    void phaseRetriesReuseTheRequestId();
    void malformedPhasesFallBack();
    void lightThemeShots();
    void deliveryShots();
    void emptyPanelOffersNewTeamAndDesigner();
    void embeddedNewTeamIsLedByTheSession();
    void workspaceAddsAMember();
    void designerPageStartsOrOpensTheDesigner();
    void catalogEditorMarksManualEdits();
    void boardColumnsKeepTheirScroll();
private:
    QString script() const { return m_dir.filePath("hgs"); }
    void setTeam(const QJsonObject &value) { QVERIFY(FakeCli::replace(script() + ".status", QJsonDocument(value).toJson(QJsonDocument::Compact))); }
    QTemporaryDir m_dir;
};

void TestTeamsUi::initTestCase()
{
    QVERIFY(m_dir.isValid());
    TeamController::setPollInterval(60000);
    darkPalette();
    QFile c(script() + ".catalog"); QVERIFY(c.open(QIODevice::WriteOnly)); c.write(QJsonDocument(catalog()).toJson(QJsonDocument::Compact)); c.close();
    QFile f(script() + ".feed"); QVERIFY(f.open(QIODevice::WriteOnly)); f.write(R"({"items":[{"seq":2,"at":1300,"kind":"reported","detail":{"by":"Backend","task":2,"attempt":1}}],"next_cursor":null})"); f.close();
    QVERIFY(FakeCli::install(script(), R"PY(#!/usr/bin/env python3
import json,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
body=json.load(sys.stdin) if a[:1]==['team'] else {}
open(__file__+'.calls','a').write(json.dumps({'args':a,'input':body})+'\n')
if a[:2]==['team','list']:
 t=json.load(open(__file__+'.status'))
 for key in ('layers','cycle','ready','validation'):t.pop(key)
 print(json.dumps({'items':[t]}))
elif a[:2]==['team','status']:sys.stdout.write(open(__file__+'.status').read())
elif a[:2]==['team','events']:sys.stdout.write(open(__file__+'.feed').read())
elif a[:3]==['team','catalog','get']:sys.stdout.write(open(__file__+'.catalog').read())
elif a[:2]==['team','approve-phase'] and body.get('phase')==9:
 print('hgs: Phase 9 is not waiting for approval',file=sys.stderr);sys.exit(1)
elif a[:2]==['team','create']:print(json.dumps({'status':'created','team':'team-new','request_id':body.get('request_id')}))
elif a[:2]==['account','ls']:print(json.dumps({'profiles':[{'id':'native-claude','provider':'claude','label':'Default','account_status':{'status':'ok'}},
 {'id':'work','provider':'claude','label':'Claude work','account_status':{'status':'signed_out'}}]}))
else:print(json.dumps({'status':'ok','version':4,'request_id':body.get('request_id')}))
)PY"));
}

void TestTeamsUi::init()
{
    QFile::remove(script() + ".calls");
    setTeam(team());
}

void TestTeamsUi::catalogModelAppliesKeptProposals()
{
    const auto c = parseCatalog(catalog());
    QCOMPARE(c.version, 3);
    QCOMPARE(c.roles.size(), 3);
    QCOMPARE(c.templ("feature")->tasks.size(), 3);
    QCOMPARE(c.templ("feature")->tasks[2].deps, QList<int>{1});
    QCOMPARE(c.proposals.size(), 2);
    const auto kept = withProposals(c, false);
    QVERIFY(kept.role("security"));
    QCOMPARE(kept.role("tester")->agent, QStringLiteral("kimi"));   // p1 is undecided
    QCOMPARE(withProposals(c, true).role("tester")->model, QStringLiteral("Sonnet"));
    QCOMPARE(c.roleNamed("backend")->name, QStringLiteral("Backend"));
    // A role survives the trip through JSON.
    QCOMPARE(parseRole(c.role("tester")->json()).model, QStringLiteral("kimi-k2"));
}

void TestTeamsUi::taskCardOffersActionsByStatus()
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

void TestTeamsUi::attemptStatesShowPhaseIcons()
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
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
                    if (auto *area = qobject_cast<QScrollArea *>(w); area && area->objectName() == "teamBoardColumn")
                        return area->property("column").toString();
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

void TestTeamsUi::taskCardAnswersQuestionsAndHolds()
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
    shot(&card, "delivery-card-release-dark");
    lightPalette();
    card.setTask(team, 15, live, false); shot(&card, "delivery-card-question-light");
    card.setTask(team, 17, live, false); shot(&card, "delivery-card-replaced-light");
    card.setTask(team, 16, live, false); shot(&card, "delivery-card-release-light");
    darkPalette();
    card.setTask(team, 17, live, true);
}

// G5/F10: the drafts wait as compact lines at the top of the members column,
// and their arrows move one among the others with move-phase. Seven drafts show
// the live numbers, the disabled ends and the renumbering in the feed.
void TestTeamsUi::draftReorderSendsMovePhase()
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
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamWorkspaceDraftPhase").size(), 7);
    auto *membersList = workspace.findChild<QWidget *>("teamWorkspaceMemberList");
    QVERIFY(membersList && membersList->isAncestorOf(workspace.findChildren<QFrame *>("teamWorkspaceDraftPhase").first()));
    // The live numbers, in the store's order.
    QStringList headings;
    for (auto *label : workspace.findChildren<QLabel *>("teamDraftPhaseHeading")) headings << label->text();
    QCOMPARE(headings, (QStringList{"Phase 2", "Phase 3", "Phase 4", "Phase 5", "Phase 6", "Phase 7", "Phase 8"}));
    // The first draft cannot go up, the last cannot go down.
    auto ups = workspace.findChildren<QPushButton *>("teamMovePhaseUp");
    QCOMPARE(ups.size(), 7);
    QVERIFY(!ups.first()->isEnabled());
    QVERIFY(ups.last()->isEnabled());
    ups[1]->click();
    QTRY_COMPARE(calls(script(), "move-phase").size(), 1);
    auto moved = calls(script(), "move-phase").first();
    QCOMPARE(moved["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(moved["phase"].toInt(), 3);
    QCOMPARE(moved["to"].toInt(), 2);
    QCOMPARE(moved["position"].toString(), QStringLiteral("before"));
    QTRY_VERIFY(workspace.findChildren<QPushButton *>("teamMovePhaseDown").size() == 7);
    auto downs = workspace.findChildren<QPushButton *>("teamMovePhaseDown");
    QVERIFY(!downs.last()->isEnabled());
    downs.first()->click();
    QTRY_COMPARE(calls(script(), "move-phase").size(), 2);
    moved = calls(script(), "move-phase")[1];
    QCOMPARE(moved["phase"].toInt(), 2);
    QCOMPARE(moved["to"].toInt(), 3);
    QCOMPARE(moved["position"].toString(), QStringLiteral("after"));
    // The store renumbers the drafts after a move: the rows follow the live
    // numbers and titles it sends, not positions cached when the canvas was built.
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
    QTRY_VERIFY(([&] {
        controller.refresh();
        const auto headings = workspace.findChildren<QLabel *>("teamDraftPhaseHeading");
        const auto titles = workspace.findChildren<QLabel *>("teamDraftPhaseTitle");
        return headings.size() == 7 && titles.size() == 7 && headings[0]->text() == QStringLiteral("Phase 2")
               && headings[6]->text() == QStringLiteral("Phase 8") && titles[0]->text() == QStringLiteral("Polish");
    })());
    // QA fixes now waits for the draft that moved before it, at phase 2.
    QCOMPARE(workspace.findChildren<QLabel *>("teamDraftPhaseReason")[0]->text(), QStringLiteral("after phase 2"));
    shot(&workspace, "phases-7-drafts-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTRY_VERIFY(workspace.findChildren<QFrame *>("teamWorkspaceDraftPhase").size() == 7);
    shot(&workspace, "phases-7-drafts-light");
    darkPalette();
    workspace.setTheme(true);
}

void TestTeamsUi::memberQueueListsCurrentAndReturned()
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
    // The workspace member card shows the same queue and opens its task.
    QWidget *memberCard = nullptr;
    for (auto *m : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (m->property("teamMember") == "backend") memberCard = m;
    QVERIFY(memberCard);
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

void TestTeamsUi::planningEditsSendTheWholePlan()
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

void TestTeamsUi::newTeamStartsTheLeadWithTheTemplate()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}, {"msi-old", "msi-old", false}},
                         {{QString(), "claude/checkout/main", "claude"}}, QDir::toNativeSeparators(folder.path()), "Checkout");
    dialog.show();
    // The isolated dark capture gets the same readable placeholder color the
    // window palette gives the real dialog.
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        const QColor placeholder = dialog.findChild<QLineEdit *>("teamNewName")->palette().color(QPalette::PlaceholderText);
        QVERIFY(placeholder.isValid());
        QVERIFY(placeholder.lightness() > 90);
    }
    // Not a Git folder: members share it, no worktrees.
    QTRY_VERIFY(dialog.findChild<QLabel *>("teamNewGitNote")->isVisible());
    QVERIFY(!dialog.findChild<QCheckBox *>("teamNewWorktrees")->isEnabled());
    // F7/L7: "Take into a new team" names the leftover session; the dialog
    // offers it as the existing lead, and a session on another computer cannot
    // lead from here.
    auto *useExisting = dialog.findChild<QCheckBox *>("teamNewUseExisting");
    auto *existingLead = dialog.findChild<QComboBox *>("teamNewExistingLead");
    QVERIFY(useExisting && existingLead);
    QVERIFY(!useExisting->isChecked());
    dialog.preselectExistingLead(QString(), QStringLiteral("claude/checkout/main"));
    QVERIFY(useExisting->isChecked());
    QCOMPARE(existingLead->currentData().toString(), QStringLiteral("claude/checkout/main"));
    dialog.preselectExistingLead(QStringLiteral("msi-old"), QStringLiteral("kimi/checkout/tester"));
    QCOMPARE(existingLead->currentData().toString(), QStringLiteral("claude/checkout/main"));
    useExisting->setChecked(false);   // this test creates a fresh lead below
    auto *templ = dialog.findChild<QComboBox *>("teamNewTemplate");
    templ->setCurrentIndex(templ->findData("feature"));
    QTRY_COMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    auto *create = dialog.findChild<QPushButton *>("teamNewCreate");
    QVERIFY(!create->isEnabled());   // no name yet
    dialog.findChild<QLineEdit *>("teamNewName")->setText("Checkout redesign");
    QTRY_VERIFY(create->isEnabled());
    // A member on an offline computer blocks Create with its own row error.
    auto *computer = dialog.findChildren<QComboBox *>("teamNewComputer").first();
    computer->setCurrentIndex(computer->findData("msi-old"));
    QTRY_VERIFY(!create->isEnabled());
    QVERIFY(dialog.findChildren<QLabel *>("teamNewRowError").first()->isVisible());
    shot(&dialog, "new-team");
    computer->setCurrentIndex(computer->findData(QString()));
    QTRY_VERIFY(create->isEnabled());
    create->click();
    QTRY_COMPARE(dialog.result(), int(QDialog::Accepted));
    QCOMPARE(dialog.createdTeam(), QStringLiteral("team-new"));
    const auto input = calls(script(), "create").first();
    QVERIFY(input["start"].toBool());
    QCOMPARE(input["template"].toString(), QStringLiteral("feature"));
    QCOMPARE(input["members"].toArray().size(), 2);
    QCOMPARE(input["tasks"].toArray().size(), 3);   // the template plans
    QCOMPARE(input["tasks"].toArray()[0].toObject()["assignee"].toString(), QStringLiteral("lead"));
    QCOMPARE(input["settings"].toObject()["folder_policy"].toString(), QStringLiteral("shared"));
    QCOMPARE(input["settings"].toObject()["phase_approval"].toString(), QStringLiteral("auto"));
    QCOMPARE(input["lead"].toObject()["prompt"].toString(), QStringLiteral("You lead."));
}

void TestTeamsUi::catalogEditorSavesTheNextVersion()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(900, 700); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    QVERIFY(editor.findChild<QWidget *>("teamRoleEditor"));
    shot(&editor, "settings-roles");
    for (auto *b : editor.findChildren<QPushButton *>()) if (b->text() == "+ Role") { b->click(); break; }
    QCOMPARE(roles->count(), 4);
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave");
    QVERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto input = calls(script(), "catalog set").first();
    QCOMPARE(input["version"].toInt(), 3);
    QCOMPARE(input["catalog"].toObject()["roles"].toArray().size(), 4);
}

void TestTeamsUi::catalogRoleChoosesAccountModelAndComputer()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.setSources(&client, {{"devbox", "devbox", true}, {"laptop", "laptop", false}});
    editor.resize(900, 700); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 3);
    // The lead runs Claude: its accounts on this computer, its models, the computers.
    auto *computer = editor.findChild<QComboBox *>("teamRoleComputer"); QVERIFY(computer);
    QCOMPARE(computer->count(), 3); QCOMPARE(computer->currentData().toString(), QString());
    QVERIFY(computer->itemText(2).contains("offline"));
    auto *model = editor.findChild<QComboBox *>("teamRoleModel"); QVERIFY(model && model->isEditable());
    QVERIFY(model->findText("opus") >= 0); QCOMPARE(model->currentText(), QString());
    QTRY_COMPARE(editor.findChild<QComboBox *>("teamRoleAccount")->count(), 3);
    auto *account = editor.findChild<QComboBox *>("teamRoleAccount");
    QVERIFY(account->itemText(account->findData("work")).contains("signed out"));
    account->setCurrentIndex(account->findData("native-claude")); emit account->activated(account->currentIndex());
    model->setCurrentIndex(model->findText("sonnet")); emit model->activated(model->currentIndex());
    computer->setCurrentIndex(1); emit computer->activated(1);
    // Accounts belong to a computer: another one clears the choice.
    auto *rebuilt = editor.findChild<QComboBox *>("teamRoleAccount");
    QTRY_VERIFY(rebuilt && rebuilt != account); QCOMPARE(rebuilt->currentData().toString(), QString());
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto lead = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray()[0].toObject();
    QCOMPARE(lead["model"].toString(), QStringLiteral("sonnet"));
    QCOMPARE(lead["computer"].toString(), QStringLiteral("devbox"));
    QCOMPARE(lead.value("account").toString(), QString());
}

void TestTeamsUi::designerDraftIsKeptThenApplied()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPanel panel(&controller);
    panel.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    panel.resize(560, 900); panel.show();
    controller.refreshCatalog();
    QTRY_COMPARE(panel.findChildren<QFrame *>("teamProposal").size(), 2);
    shot(&panel, "designer-draft");
    auto *apply = panel.findChild<QPushButton *>("teamDesignerApply");
    QVERIFY(apply->isEnabled());   // p2 is kept
    panel.findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 1);
    QCOMPARE(calls(script(), "catalog decide").first()["decision"].toString(), QStringLiteral("keep"));
    QCOMPARE(calls(script(), "catalog decide").first()["proposal"].toString(), QStringLiteral("p1"));
    panel.findChild<QPushButton *>("teamDesignerApply")->click();
    QTRY_COMPARE(calls(script(), "catalog apply").size(), 1);
}

void TestTeamsUi::workspaceShowsGraphBoardAndFocus()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    // The coordinator shares the side column with the members, split by a divider.
    auto *side = workspace.findChild<QSplitter *>("teamWorkspaceSideSplit");
    QVERIFY(side && side->count() == 2);
    QVERIFY(side->widget(1)->findChild<QWidget *>("teamConversation"));
    // The answers slot stays when nothing waits.
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceAnswers"));
    // The graph fills the canvas: at least its height, centered. A render
    // (every poll) builds the canvas again, so each check finds it anew.
    const auto graph = [&workspace] { return workspace.findChild<QWidget *>("teamWorkspaceGraph"); };
    const auto canvas = [&workspace] { return workspace.findChild<QScrollArea *>("teamWorkspaceCanvas"); };
    const auto fills = [&] {
        if (!graph() || !canvas()) return false;
        const int bar = canvas()->style()->pixelMetric(QStyle::PM_ScrollBarExtent, nullptr, canvas());
        return graph()->height() >= canvas()->viewport()->height() - bar - 2;
    };
    QTRY_VERIFY(fills());
    // ...and holds still: no scroll bar comes and goes under it.
    QTest::qWait(150);
    QTRY_VERIFY(graph());
    const QSize settled = graph()->size();
    for (int i = 0; i < 5; ++i) { QTest::qWait(40); QTRY_VERIFY(graph()); QCOMPARE(graph()->size(), settled); }
    // A divider the owner moves stands between the canvas and the side column.
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split && split->handleWidth() >= 5);
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    shot(&workspace, "d3-running");
    // The coordinator's quick asks stay the same buttons across renders, so
    // they never flicker.
    const auto asks = workspace.findChildren<QPushButton *>("teamConversationAsk");
    QVERIFY(!asks.isEmpty());
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceFilter")->findChildren<QPushButton *>())
        if (b->property("segment") == "active") { b->click(); break; }
    QTest::qWait(150);
    QCOMPARE(workspace.findChildren<QPushButton *>("teamConversationAsk"), asks);
    // The views are icons with their names as tooltips.
    auto *views = workspace.findChild<QWidget *>("teamWorkspaceView");
    for (auto *b : views->findChildren<QPushButton *>()) QVERIFY(b->text().isEmpty() && !b->icon().isNull() && !b->toolTip().isEmpty());
    for (auto *b : views->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QCOMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 5);
    // Role and state share a line on a wide card and wrap on a narrow one.
    {
        // Reads the first card afresh: a resize may render the board again.
        const auto oneLine = [&workspace] {
            auto *boardCard = workspace.findChildren<QFrame *>("teamBoardCard").value(0);
            auto *pill = boardCard ? boardCard->findChild<QWidget *>("teamBoardRole") : nullptr;
            auto *mark = boardCard ? boardCard->findChild<QWidget *>("teamBoardState") : nullptr;
            return pill && mark && std::abs(pill->geometry().center().y() - mark->geometry().center().y()) <= 4 ? 1 : pill && mark ? 0 : -1;
        };
        const QSize before = workspace.size();
        workspace.resize(700, before.height());
        QTRY_COMPARE(oneLine(), 0);
        workspace.resize(1800, before.height());
        QTRY_COMPARE(oneLine(), 1);
        workspace.resize(before);
    }
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceFilter"));   // the board's columns sort by state
    shot(&workspace, "d3-board");
    // Focusing a member keeps only its tasks.
    for (auto *m : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (m->property("teamMember") == "tester") { QTest::mouseClick(m, Qt::LeftButton, {}, QPoint(4, 4)); break; }
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceFocus"));
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 1);
    QVERIFY(workspace.findChild<QLabel *>("teamBoardEmpty"));   // empty columns say so
    // A card opens the drawer.
    workspace.findChildren<QPushButton *>("teamBoardOpen").first()->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDrawer")->isVisible());
}

void TestTeamsUi::workspaceGroupsTasksByPhase()
{
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    const auto graph = [&workspace] { return dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph")); };
    QTRY_VERIFY(graph());
    QTRY_COMPARE(graph()->bands.size(), 3);
    const auto nodeIds = [](const TeamUi::TaskView *view) {
        QList<int> ids;
        for (const auto &node : view->nodes) ids.append(node.id);
        std::sort(ids.begin(), ids.end());
        return ids;
    };
    // Phases left to right; the finished one starts collapsed.
    QCOMPARE(graph()->bands[0].heading, QStringLiteral("Phase 1 · 2/2"));
    QVERIFY(graph()->bands[0].collapsed);
    QCOMPARE(graph()->bands[1].heading, QStringLiteral("Phase 2 · QA fixes · 1/2"));
    QVERIFY(graph()->bands[1].barrier);
    QVERIFY(!graph()->bands[1].collapsed);
    QCOMPARE(graph()->bands[2].heading, QStringLiteral("Phase 3 · Polish · 0/1"));
    QVERIFY(graph()->bands[2].draft);
    // A collapsed phase builds no task node; one summary stands in.
    QCOMPARE(nodeIds(graph()), (QList<int>{3, 4, 5}));
    QVERIFY(!graph()->summaryRect(1).isNull());
    shot(&workspace, "d3-phases-graph-collapsed");
    const auto clickHeader = [&](int phase) {
        auto *view = graph(); QVERIFY(view);
        QTest::mouseClick(view, Qt::LeftButton, {}, (view->bandRect(phase).center() * view->zoom).toPoint());
    };
    using Guard = QPointer<QWidget>;
    const Guard before = graph();
    clickHeader(1);   // the header opens the finished phase, its tasks come back
    QTRY_VERIFY(graph() && graph() != before);
    QTRY_VERIFY(!graph()->bands[0].collapsed);
    QTRY_COMPARE(nodeIds(graph()).size(), 5);
    QVERIFY(graph()->summaryRect(1).isNull());
    clickHeader(2);   // the current phase closes; its tasks go, the others stay
    QTRY_VERIFY(graph()->bands[1].collapsed);
    QTRY_COMPARE(nodeIds(graph()), (QList<int>{1, 2, 5}));
    // The collapsed phase's summary node opens it too, and its line says a
    // finished phase is done rather than just "open".
    const auto clickSummary = [&](int phase) {
        auto *view = graph(); QVERIFY(view);
        QVERIFY(!view->summaryRect(phase).isNull());
        QTest::mouseClick(view, Qt::LeftButton, {}, (view->summaryRect(phase).center() * view->zoom).toPoint());
    };
    clickSummary(2);
    QTRY_VERIFY(!graph()->bands[1].collapsed);
    QTRY_COMPARE(nodeIds(graph()).size(), 5);
    QCOMPARE(TeamUi::phaseSummaryText(2, 2), QStringLiteral("2/2 done · click to open"));
    QCOMPARE(TeamUi::phaseSummaryText(1, 2), QStringLiteral("1/2 · click to open"));
    clickHeader(2);   // closed again for the board below
    QTRY_VERIFY(graph()->bands[1].collapsed);
    shot(&workspace, "d3-phases-graph-open");
    const auto showView = [&workspace](const QString &view) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == view) { b->click(); break; }
    };
    // Layers group by phase too, each phase with its own layer rules.
    showView("layers");
    const auto layers = [&workspace] { return dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceLayers")); };
    QTRY_VERIFY(layers());
    QTRY_COMPARE(layers()->bands.size(), 3);
    QVERIFY(layers()->bands[1].collapsed);
    QCOMPARE(nodeIds(layers()), (QList<int>{1, 2, 5}));
    shot(&workspace, "d3-phases-layers");
    // The board stacks the phases; a collapsed one shows only its header.
    showView("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto toggles = [&workspace] { return workspace.findChildren<QPushButton *>("teamBoardPhaseToggle"); };
    const auto cards = [&workspace] {
        QList<int> ids;
        for (auto *card : workspace.findChildren<QFrame *>("teamBoardCard")) ids.append(card->property("task").toInt());
        std::sort(ids.begin(), ids.end());
        return ids;
    };
    QTRY_COMPARE(toggles().size(), 3);
    QCOMPARE(toggles()[0]->text(), QStringLiteral("Phase 1 · 2/2"));
    QCOMPARE(toggles()[1]->text(), QStringLiteral("Phase 2 · QA fixes · 1/2"));
    QCOMPARE(toggles()[2]->text(), QStringLiteral("Phase 3 · Polish · 0/1"));
    QCOMPARE(cards(), (QList<int>{1, 2, 5}));
    shot(&workspace, "d3-phases-board-collapsed");
    QPointer<QPushButton> closed = toggles()[1];
    closed->click();
    QTRY_COMPARE(cards(), (QList<int>{1, 2, 3, 4, 5}));
    QTRY_VERIFY(toggles().size() == 3 && !toggles()[1]->property("collapsed").toBool());
    shot(&workspace, "d3-phases-board");
}

void TestTeamsUi::phaseApprovalAndFinishAskTheOwner()
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    // The team header says the phase waits; the draft waits in the members
    // column, as one compact line with the two owner decisions (G5).
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamBarPendingPhase"));
    QCOMPARE(workspace.findChild<QLabel *>("teamDraftPhaseHeading")->text(), QStringLiteral("Phase 3"));
    auto *membersList = workspace.findChild<QWidget *>("teamWorkspaceMemberList");
    QVERIFY(membersList);
    auto *draft = workspace.findChild<QFrame *>("teamWorkspaceDraftPhase");
    QVERIFY(draft);
    // It sits in the members column, not in the rows above the canvas, and the
    // canvas gets that height back.
    QVERIFY(membersList->isAncestorOf(draft));
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceTop")->isAncestorOf(draft));
    QVERIFY(draft->height() <= 40);
    QCOMPARE(workspace.findChild<QLabel *>("teamDraftPhaseTitle")->text(), QStringLiteral("Polish"));
    QVERIFY(workspace.findChild<QPushButton *>("teamApprovePhase"));
    QVERIFY(workspace.findChild<QPushButton *>("teamDiscardPhase"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceFinish"));   // tasks are still open
    shot(&workspace, "d3-phases-draft");
    // G5: the Phases block folds and the members fold as well; each header stays.
    auto *phasesToggle = workspace.findChild<QToolButton *>("teamPhasesToggle");
    auto *membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    QVERIFY(phasesToggle && membersToggle);
    phasesToggle->click();
    QTRY_VERIFY(!workspace.findChild<QFrame *>("teamWorkspaceDraftPhase"));
    QVERIFY(workspace.findChild<QToolButton *>("teamPhasesToggle"));
    shot(&workspace, "d3-phases-folded");
    workspace.findChild<QToolButton *>("teamPhasesToggle")->click();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceDraftPhase"));
    membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    membersToggle->click();
    QTRY_VERIFY(!workspace.findChild<QFrame *>("teamWorkspaceMember"));
    QVERIFY(workspace.findChild<QFrame *>("teamWorkspaceDraftPhase"));   // the phases stay
    shot(&workspace, "d3-members-folded");
    workspace.findChild<QToolButton *>("teamMembersToggle")->click();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceMember"));
    // Opened, the draft shows its tasks and, when the store reports them, the
    // lead's changes as a diff.
    workspace.findChild<QToolButton *>("teamDraftOpen")->click();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamDraftDetail"));
    auto *detail = workspace.findChild<QFrame *>("teamDraftDetail");
    QVERIFY(detail->findChildren<QLabel *>().size() >= 2);   // the task and its state
    shot(&workspace, "d3-phases-draft-open");
    // The same one-draft column in the light theme.
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamDraftDetail"));
    shot(&workspace, "d3-phases-draft-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamApprovePhase"));
    // Approve sends the owner's decision once, with the phase. The buttons are
    // disabled at once, so a second click cannot send it again.
    auto *approve = workspace.findChild<QPushButton *>("teamApprovePhase");
    approve->click();
    QVERIFY(!approve->isEnabled());
    QVERIFY(!workspace.findChild<QPushButton *>("teamDiscardPhase")->isEnabled());
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "approve-phase").first()["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(calls(script(), "approve-phase").first()["by"].toString(), QStringLiteral("human"));
    // Discard asks to confirm first: "Discard phase N and its K tasks?".
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamDiscardPhase"));
    // Cancelling the confirmation sends nothing.
    QTimer::singleShot(0, &panel, [&panel] {
        if (auto *box = panel.findChild<QMessageBox *>("teamDiscardPhaseConfirm"))
            for (auto *b : box->buttons()) if (b->text() == QStringLiteral("Cancel")) { b->click(); break; }
    });
    workspace.findChild<QPushButton *>("teamDiscardPhase")->click();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamDiscardPhase"));
    QTimer::singleShot(0, &panel, [&panel] {
        if (auto *box = panel.findChild<QMessageBox *>("teamDiscardPhaseConfirm"))
            for (auto *b : box->buttons()) if (b->text() == QStringLiteral("Discard phase")) { b->click(); break; }
    });
    workspace.findChild<QPushButton *>("teamDiscardPhase")->click();
    QTRY_COMPARE(calls(script(), "discard-phase").size(), 1);
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
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QWidget *>("teamBarWaitingForLead") != nullptr; })());
    QVERIFY(!workspace.findChild<QWidget *>("teamBarPendingPhase"));
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceFinish"));
    shot(&workspace, "d3-phases-waiting");
    workspace.findChild<QPushButton *>("teamWorkspaceFinish")->click();
    auto *dialog = panel.findChild<QDialog *>("teamFinishDialog");
    QTRY_VERIFY(dialog);
    // Cancelling deletes the dialog instead of leaving it hidden under the panel.
    dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Cancel)->click();
    QTest::qWait(50);
    QCOMPARE(panel.findChildren<QDialog *>("teamFinishDialog").size(), 0);
    workspace.findChild<QPushButton *>("teamWorkspaceFinish")->click();
    dialog = panel.findChild<QDialog *>("teamFinishDialog");
    QTRY_VERIFY(dialog);
    auto *summary = dialog->findChild<QPlainTextEdit *>("teamFinishSummary");
    auto *finish = dialog->findChild<QPushButton *>("teamFinishConfirm");
    QVERIFY(summary && finish);
    QVERIFY(!finish->isEnabled());   // a summary is required
    summary->setPlainText(QString(4001, QLatin1Char('x')));
    QTRY_VERIFY(!finish->isEnabled());   // and bounded at 4,000 characters
    // Characters, not UTF-16 units: 4,000 emoji are accepted, 4,001 are not.
    const QString emoji = QString(QChar(0xD83D)) + QChar(0xDE00);
    QString many;
    many.reserve(8000);
    for (int i = 0; i < 4000; ++i) many += emoji;
    summary->setPlainText(many);
    QTRY_VERIFY(finish->isEnabled());
    summary->setPlainText(many + emoji);
    QTRY_VERIFY(!finish->isEnabled());
    summary->setPlainText(QStringLiteral("Phases 1–2 are delivered; the retest is #11."));
    QTRY_VERIFY(finish->isEnabled());
    finish->click();
    QTRY_COMPARE(calls(script(), "finish").size(), 1);
    QCOMPARE(calls(script(), "finish").first()["summary"].toString(), QStringLiteral("Phases 1–2 are delivered; the retest is #11."));
    QCOMPARE(calls(script(), "finish").first()["team"].toString(), QStringLiteral("team-1"));
}

void TestTeamsUi::leftoverMembersAreShownApart()
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

void TestTeamsUi::memberEditorPicksAnExistingSession()
{
    setTeam(team("planning"));
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
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
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

void TestTeamsUi::severalDraftPhasesRenderAndApproveInOrder()
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
    // Spec 10: three drafts, each with its own strip; only the next one opens.
    auto strips = workspace.findChildren<QFrame *>("teamWorkspaceDraftPhase");
    QCOMPARE(strips.size(), 3);
    QCOMPARE(workspace.findChild<QWidget *>("teamBarPendingPhase")->toolTip(), QStringLiteral("phases 2, 3, 4 pending approval"));
    QList<QPushButton *> approves = workspace.findChildren<QPushButton *>("teamApprovePhase");
    QCOMPARE(approves.size(), 3);
    QCOMPARE(approves[0]->property("phase").toInt(), 2);
    QVERIFY(approves[0]->isEnabled());
    QVERIFY(!approves[1]->isEnabled());
    QVERIFY(approves[1]->toolTip().contains(QStringLiteral("Phase 3 opens after phase 2 is approved")));
    QVERIFY(!approves[2]->isEnabled());
    // QA: the nearest open phase is named, not the first draft of the plan.
    QVERIFY(approves[2]->toolTip().contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    // Several drafts render in the graph, the layers and the board.
    auto *graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    QVERIFY(graph);
    QCOMPARE(graph->bands.size(), 4);
    for (int i = 1; i < graph->bands.size(); ++i) QVERIFY(graph->bands[i].draft);
    // G4: every band keeps the alternating layer stripes and a small label.
    const auto bandLayers = graph->bandLayers();
    QVERIFY(!bandLayers.isEmpty());
    QSet<int> striped;
    for (const auto &layer : bandLayers) {
        QVERIFY(layer.phase > 0);
        QVERIFY(layer.count > 0);
        QVERIFY(layer.rect.width() > 4 && layer.rect.height() > 4);
        striped.insert(layer.phase);
    }
    for (const auto &band : graph->bands) if (!band.collapsed) QVERIFY(striped.contains(band.n));
    shot(&workspace, "roadmap-drafts-dark");
    // The same banded graph in the light theme.
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    auto *lightGraph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    QVERIFY(lightGraph && !lightGraph->bandLayers().isEmpty());
    shot(&workspace, "roadmap-graph-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "layers") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    auto *layers = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QVERIFY(layers);
    QCOMPARE(layers->bands.size(), 4);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QCOMPARE(workspace.findChildren<QWidget *>("teamBoardPhase").size(), 4);
    shot(&workspace, "roadmap-board-dark");
    // Phase 2 approved and its tasks decided: phase 3 opens, phase 4 still waits.
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
    setTeam(next);
    QTRY_VERIFY(([&] {
        controller.refresh();
        const auto buttons = workspace.findChildren<QPushButton *>("teamApprovePhase");
        return buttons.size() == 2 && buttons[0]->property("phase").toInt() == 3 && buttons[0]->isEnabled()
               && !buttons[1]->isEnabled();
    })());
    QVERIFY(workspace.findChildren<QPushButton *>("teamApprovePhase")[1]->toolTip().contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    // Light theme.
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDraftPhase"));
    shot(&workspace, "roadmap-drafts-light");
    darkPalette();
    workspace.setTheme(true);
}

void TestTeamsUi::pinMarkAndFinishedTeamShowWhoFinished()
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

void TestTeamsUi::catalogEditorCarriesNewPhaseApproval()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(900, 700); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 3);
    for (auto *b : editor.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>())
        if (b->property("segment") == "templates") { b->click(); break; }
    auto *templates = editor.findChild<QListWidget *>("teamCatalogTemplates");
    QTRY_VERIFY(templates && templates->count() >= 1);
    templates->setCurrentRow(0);
    auto *choice = editor.findChild<QWidget *>("teamTemplatePhaseApproval");
    QTRY_VERIFY(choice);
    QPushButton *owner = nullptr, *lead = nullptr;
    for (auto *b : choice->findChildren<QPushButton *>()) {
        if (b->property("segment") == "review") owner = b;
        if (b->property("segment") == "auto") lead = b;
    }
    QVERIFY(owner && lead);
    QVERIFY(lead->isChecked());   // the template says the lead runs new phases
    owner->click();
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    bool found = false;
    for (const auto &value : calls(script(), "catalog set").first()["catalog"].toObject()["templates"].toArray()) {
        if (value.toObject()["id"].toString() != QStringLiteral("feature")) continue;
        QCOMPARE(value.toObject()["phase_approval"].toString(), QStringLiteral("review"));
        found = true;
    }
    QVERIFY(found);
}

void TestTeamsUi::phaseRetriesReuseTheRequestId()
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

void TestTeamsUi::malformedPhasesFallBack()
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

void TestTeamsUi::lightThemeShots()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) QSKIP("Set ZERUS_TEAM_SHOTS to render the light shots.");
    const QPalette dark = QApplication::palette();
    QPalette light;
    light.setColor(QPalette::Window, QColor("#f5f7f9")); light.setColor(QPalette::WindowText, QColor("#1a2733"));
    light.setColor(QPalette::Base, QColor("#ffffff")); light.setColor(QPalette::Button, QColor("#edf2f5"));
    light.setColor(QPalette::ButtonText, QColor("#1a2733")); light.setColor(QPalette::Text, QColor("#1a2733"));
    light.setColor(QPalette::PlaceholderText, QColor("#627082"));
    QApplication::setPalette(light);
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setTheme(false); panel.setTheme(false);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamApprovePhase"));
    // The coordinator's empty Activity text is light-theme text, not a dark
    // document left over from before the theme switch.
    auto *conversation = workspace.findChild<QWidget *>("teamConversationActivity");
    auto *browser = conversation ? conversation->findChild<QTextBrowser *>() : nullptr;
    QVERIFY(browser);
    const auto lightText = [&] {
        QColor color;
        for (auto block = browser->document()->begin(); block.isValid(); block = block.next())
            if (block.text().contains(QStringLiteral("Messages, tools and approvals"))) {
                QTextCursor cursor(block); cursor.movePosition(QTextCursor::StartOfBlock);
                cursor.movePosition(QTextCursor::Right, QTextCursor::KeepAnchor, 1);
                color = cursor.charFormat().foreground().color();
            }
        return color.isValid() && color.lightness() < 150;
    };
    QTRY_VERIFY(lightText());
    shot(&workspace, "d3-phases-draft-light");
    auto waiting = phasedTeam();
    auto tasks = waiting["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    waiting["tasks"] = tasks;
    waiting["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    waiting["pending_phase"] = QJsonValue();
    waiting["waiting_for_lead"] = true;
    setTeam(waiting);
    QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QPushButton *>("teamWorkspaceFinish") != nullptr; })());
    shot(&workspace, "d3-phases-waiting-light");
    QApplication::setPalette(dark);
}

void TestTeamsUi::deliveryShots()
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

void TestTeamsUi::boardColumnsKeepTheirScroll()
{
    // Thirty pending tasks: more than a column shows.
    auto t = team();
    auto tasks = t["tasks"].toArray();
    for (int id = 6; id < 36; ++id) tasks.append(QJsonObject{{"id", id}, {"title", QStringLiteral("Bin %1").arg(id)}, {"assignee", "tester"}, {"status", "pending"}});
    t["tasks"] = tasks;
    QJsonObject layers; for (const auto &k : tasks) layers[QString::number(k.toObject()["id"].toInt())] = 0;
    t["layers"] = layers;
    setTeam(t);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    const auto showBoard = [&workspace] {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    };
    showBoard();
    const auto pending = [&workspace]() -> QScrollBar * {
        for (auto *area : workspace.findChildren<QScrollArea *>("teamBoardColumn")) if (area->property("column") == "pending") return area->verticalScrollBar();
        return nullptr;
    };
    QTRY_VERIFY(pending() && pending()->maximum() > 0);
    pending()->setValue(pending()->maximum() / 2);
    const int scrolled = pending()->value();
    QVERIFY(scrolled > 0);
    // A poll renders the board again; the column stays where it was.
    const QPointer<QScrollBar> old = pending();
    showBoard();
    QTRY_VERIFY(old.isNull());
    QTRY_VERIFY(pending());
    QTRY_COMPARE(pending()->value(), scrolled);
}

QTEST_MAIN(TestTeamsUi)
void TestTeamsUi::emptyPanelOffersNewTeamAndDesigner()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    int created = 0, designed = 0;
    panel.newTeam = [&] { ++created; };
    panel.openDesigner = [&] { ++designed; };
    panel.resize(420, 600); panel.show();
    panel.render();
    QTRY_VERIFY(panel.findChild<QPushButton *>("teamEmptyNew"));
    panel.findChild<QPushButton *>("teamEmptyNew")->click();
    panel.findChild<QPushButton *>("teamEmptyDesigner")->click();
    QCOMPARE(created, 1); QCOMPARE(designed, 1);
}

void TestTeamsUi::embeddedNewTeamIsLedByTheSession()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    const TeamNewTab::Coordinator coordinator{"claude/checkout/coord", "claude", "Opus", "work", QDir::toNativeSeparators(folder.path()), "msi", "Ready", false};
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}, {"msi-old", "msi-old", false}}, coordinator, "Checkout");
    QString created, designer; bool designerStarted = false;
    tab.created = [&](const QString &team) { created = team; };
    tab.designerSession = [&] { return designer; };
    tab.startDesigner = [&] { designerStarted = true; };
    tab.resize(1038, 900); tab.show();
    // The designer's chat helps once it runs; until then it offers to start it.
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewHelperIdle")->isVisible());
    QVERIFY(!tab.findChild<QWidget *>("teamNewHelper")->isVisible());
    tab.findChild<QPushButton *>("teamNewHelperStart")->click(); QVERIFY(designerStarted);
    designer = "claude/team-designer/team-designer";
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewHelper")->isVisible());
    QVERIFY(!tab.findChild<QWidget *>("teamNewHelperIdle")->isVisible());
    // Nothing picked: the dashed members box, and Create waits for a goal.
    QVERIFY(tab.findChild<QLabel *>("teamNewEmptyMembers"));
    auto *create = tab.findChild<QPushButton *>("teamNewCreate");
    QVERIFY(!create->isEnabled());
    shot(&tab, "d2-empty-wide");
    // A template card fills the rows from its roster with the roles' defaults.
    QFrame *feature = nullptr;
    for (auto *card : tab.findChildren<QFrame *>("teamNewTemplate")) if (card->property("template") == "feature") feature = card;
    QVERIFY(feature);
    QTest::mouseClick(feature, Qt::LeftButton);
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);   // the table from 760 px
    // The template's lead protocol fills Team protocol, which you may edit.
    auto *protocol = tab.findChild<QPlainTextEdit *>("teamNewProtocol");
    QCOMPARE(protocol->toPlainText(), QStringLiteral("Plan in waves of three"));
    protocol->setPlainText("Plan in waves of two");
    tab.findChild<QPlainTextEdit *>("teamNewGoal")->setPlainText("Redesign the checkout flow: address form and tests");
    QCOMPARE(tab.findChild<QLineEdit *>("teamNewName")->text(), QStringLiteral("Redesign the checkout flow"));
    QTRY_VERIFY(create->isEnabled());
    // A member on an offline computer blocks Create with its own row error.
    auto *computer = tab.findChildren<QComboBox *>("teamNewComputer").at(1);
    computer->setCurrentIndex(computer->findData("msi-old")); emit computer->activated(computer->currentIndex());
    QTRY_VERIFY(tab.findChild<QLabel *>("teamNewRowError"));
    QVERIFY(!tab.findChild<QPushButton *>("teamNewCreate")->isEnabled());
    shot(&tab, "d2-row-errors-wide");
    tab.resize(588, 1300);
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberCard").size(), 2);   // cards below 760 px
    shot(&tab, "d2-row-errors-600");
    computer = tab.findChildren<QComboBox *>("teamNewComputer").at(1);
    computer->setCurrentIndex(computer->findData(QString())); emit computer->activated(computer->currentIndex());
    QTRY_VERIFY(tab.findChild<QPushButton *>("teamNewCreate")->isEnabled());
    // The rows as entered go to create, with this session as the lead.
    auto *model = tab.findChildren<QComboBox *>("teamNewModel").first();
    model->setCurrentText("gpt-5.5");
    tab.findChild<QPushButton *>("teamNewCreate")->click();
    QTRY_COMPARE(created, QStringLiteral("team-new"));
    const auto input = calls(script(), "create").first();
    const auto lead = input["lead"].toObject();
    QCOMPARE(lead["session"].toObject()["name"].toString(), QStringLiteral("claude/checkout/coord"));
    QCOMPARE(input["lead_protocol"].toString(), QStringLiteral("Plan in waves of two"));
    // What the designer is asked carries the draft and the two sections.
    const auto request = tab.helperRequest();
    QVERIFY(request.contains("Template: Feature delivery") && request.contains("Plan in waves of two") && request.contains("\nGOAL:\n"));
    QString goal, protocolText;
    QVERIFY(TeamNewTab::parseHelperReply("Here you go.\n**GOAL:**\nShip checkout.\nDone when tests pass.\n\n## PROTOCOL:\nTwo waves.", &goal, &protocolText));
    QCOMPARE(goal, QStringLiteral("Ship checkout.\nDone when tests pass.")); QCOMPARE(protocolText, QStringLiteral("Two waves."));
    QVERIFY(!TeamNewTab::parseHelperReply("No sections here", &goal, &protocolText));
    QCOMPARE(input["members"].toArray().size(), 2);
    QCOMPARE(input["members"].toArray()[0].toObject()["model"].toString(), QStringLiteral("gpt-5.5"));
    QCOMPARE(input["members"].toArray()[1].toObject()["role"].toString(), QStringLiteral("Tester"));
    QCOMPARE(input["tasks"].toArray().size(), 3);   // the template plans
}

void TestTeamsUi::workspaceAddsAMember()
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
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceBar")->isVisible());
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

void TestTeamsUi::designerPageStartsOrOpensTheDesigner()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    QString running; QString startedIn, startedGoal; QString opened;
    page.designerSession = [&] { return running; };
    QJsonObject startedAgent;
    page.startDesigner = [&](const QString &folder, const QString &goal, const QJsonObject &agent) { startedIn = folder; startedGoal = goal; startedAgent = agent; };
    page.openDesigner = [&](const QString &name) { opened = name; };
    page.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    page.setContext("/work/checkout", "checkout");
    page.resize(1600, 900); page.show();
    QTRY_VERIFY(controller.catalogReady());   // the page reads the catalog while shown
    // Not started: the empty state asks what is needed.
    auto *start = page.findChild<QPushButton *>("teamDesignerStart");
    QVERIFY(start->isVisible());
    QVERIFY(!page.findChild<QPushButton *>("teamDesignerOpen")->isVisible());
    QVERIFY(page.findChild<QLabel *>("teamDesignerReads")->text().contains("checkout"));
    page.findChild<QPlainTextEdit *>("teamDesignerGoal")->setPlainText("A security reviewer");
    shot(&page, "d1-not-started");
    start->click();
    QCOMPARE(startedIn, QStringLiteral("/work/checkout")); QCOMPARE(startedGoal, QStringLiteral("A security reviewer"));
    QCOMPARE(startedAgent["agent"].toString(), QStringLiteral("claude"));
    QCOMPARE(startedAgent["model"].toString(), QStringLiteral("opus"));
    QCOMPARE(startedAgent["effort"].toString(), QStringLiteral("high"));
    // Running: its conversation, the proposals and the draft with a diff.
    running = "claude/checkout/team-designer";
    page.refresh();
    QVERIFY(page.findChild<QWidget *>("teamConversation")->isVisible());
    QTRY_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 2);
    QCOMPARE(page.findChild<QLabel *>("teamCatalogTitle")->text(), QStringLiteral("Draft catalog"));
    QVERIFY(page.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 of 2 kept"));
    page.findChildren<QPushButton *>("teamProposalShow").first()->click();   // p1: the Tester role
    auto *roles = page.findChild<QListWidget *>("teamCatalogRoles");
    QCOMPARE(roles->currentItem()->data(Qt::UserRole).toString(), QStringLiteral("tester"));
    QCOMPARE(roles->count(), 4);   // + the proposed Security role
    shot(&page, "d1-draft-partly-decided");
    page.findChild<QPushButton *>("teamDesignerOpen")->click(); QCOMPARE(opened, running);
    QVERIFY(page.findChild<QPushButton *>("teamDesignerApply")->isEnabled());
    // A decided card closes and comes back; Apply stands with the proposals.
    QCOMPARE(page.findChildren<QPushButton *>("teamProposalClose").size(), 1);   // p2 is kept
    page.findChild<QPushButton *>("teamProposalClose")->click();
    QTRY_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 1);
    page.findChild<QPushButton *>("teamDesignerShowClosed")->click();
    QTRY_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 2);
    auto *apply = page.findChild<QPushButton *>("teamDesignerApplyKept");
    QVERIFY(apply && apply->isVisible() && apply->isEnabled()); QCOMPARE(apply->text(), QStringLiteral("Apply 1 change"));
    apply->click();
    QTRY_COMPARE(calls(script(), "catalog apply").size(), 1);
}

void TestTeamsUi::catalogEditorMarksManualEdits()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1000, 760); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    QCOMPARE(editor.findChild<QLabel *>("teamCatalogTitle")->text(), QStringLiteral("Team catalog"));   // Settings: never the draft
    roles->setCurrentRow(1);
    auto *name = editor.findChild<QLineEdit *>("teamRoleName");
    name->setText("Backend API"); emit name->editingFinished();
    QTRY_VERIFY(editor.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 unsaved change"));
    QVERIFY(editor.findChild<QPushButton *>("teamCatalogSave")->isEnabled());
    shot(&editor, "d1-manual-edits");
}

#include "test_teamsui.moc"
