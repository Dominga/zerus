// WhiteWh fork: agent teams in the desktop, M3–M6 (design/agent-teams S3, S4,
// S6, S7, S10, S11): the task card, planning edits, New team, the catalog
// editor, the Team designer's draft and the team workspace. Each sends one
// `hgs team` request per change, recorded by a fake CLI.
#include <QtTest>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTemporaryDir>
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

QJsonObject catalog()
{
    return QJsonDocument::fromJson(R"({"builtin":false,
 "catalog":{"version":3,
  "roles":[{"id":"lead","name":"Lead","agent":"claude","prompt":"You lead."},
           {"id":"backend","name":"Backend","agent":"codex","folder":"worktree","prompt":"Build the API."},
           {"id":"tester","name":"Tester","agent":"kimi","model":"kimi-k2","folder":"worktree","prompt":"Test it."}],
  "templates":[{"id":"feature","name":"Feature delivery","roster":[{"role":"backend","count":1},{"role":"tester","count":1}],
                "tasks":[{"title":"Plan","role":"lead","deps":[]},{"title":"Build","role":"backend","deps":[0]},{"title":"Test","role":"tester","deps":[1]}],
                "planning":"template","approval":"review","max_members":6}],
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
    widget->grab().save(QDir(dir).filePath(name + ".png"));
}

void darkPalette()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) return;
    QApplication::setStyle("Fusion");
    QPalette p;
    p.setColor(QPalette::Window, QColor("#15191f")); p.setColor(QPalette::WindowText, QColor("#e6e8eb"));
    p.setColor(QPalette::Base, QColor("#1b1f26")); p.setColor(QPalette::Button, QColor("#232830"));
    p.setColor(QPalette::ButtonText, QColor("#e6e8eb")); p.setColor(QPalette::Text, QColor("#e6e8eb"));
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
    void planningEditsSendTheWholePlan();
    void newTeamStartsTheLeadWithTheTemplate();
    void catalogEditorSavesTheNextVersion();
    void designerDraftIsKeptThenApplied();
    void workspaceShowsGraphBoardAndFocus();
    void emptyPanelOffersNewTeamAndDesigner();
    void embeddedNewTeamIsLedByTheSession();
    void workspaceAddsAMember();
    void designerPageStartsOrOpensTheDesigner();
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
body=json.load(sys.stdin)
open(__file__+'.calls','a').write(json.dumps({'args':a,'input':body})+'\n')
if a[:2]==['team','list']:
 t=json.load(open(__file__+'.status'))
 for key in ('layers','cycle','ready','validation'):t.pop(key)
 print(json.dumps({'items':[t]}))
elif a[:2]==['team','status']:sys.stdout.write(open(__file__+'.status').read())
elif a[:2]==['team','events']:sys.stdout.write(open(__file__+'.feed').read())
elif a[:3]==['team','catalog','get']:sys.stdout.write(open(__file__+'.catalog').read())
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
    auto *approve = panel.findChild<QPushButton *>("teamApprove");
    QVERIFY(approve && !approve->isEnabled());   // #6 has no assignee
    QVERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));
    QCOMPARE(panel.findChildren<QComboBox *>("teamMemberAgent").size(), 3);
    shot(&panel, "planning-420");
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
    // Not a Git folder: members share it, no worktrees.
    QTRY_VERIFY(dialog.findChild<QLabel *>("teamNewGitNote")->isVisible());
    QVERIFY(!dialog.findChild<QCheckBox *>("teamNewWorktrees")->isEnabled());
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
    workspace.resize(1100, 720); workspace.show();
    controller.refresh();
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    QVERIFY(workspace.findChild<QFrame *>("teamLeadDock"));
    shot(&workspace, "workspace-graph");
    for (auto *b : workspace.findChildren<QPushButton *>("teamWorkspaceView")) if (b->property("view") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QCOMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 5);
    shot(&workspace, "workspace-board");
    // Focusing a member keeps only its tasks.
    for (auto *b : workspace.findChildren<QPushButton *>("teamWorkspaceFocusMember")) if (b->parentWidget()->property("teamMember") == "tester") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceFocus"));
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 1);
    // A card opens the drawer.
    workspace.findChildren<QPushButton *>("teamBoardOpen").first()->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDrawer")->isVisible());
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
    QWidget tab;
    auto *layout = new QVBoxLayout(&tab);
    auto *form = new TeamNewDialog(&controller, &client, {{QString(), "this computer", true}}, {}, QDir::toNativeSeparators(folder.path()), "Checkout", &tab);
    form->embedFor("claude/checkout/coord");
    QString created;
    form->created = [&](const QString &team) { created = team; };
    layout->addWidget(form);
    tab.resize(820, 640); tab.show();
    QVERIFY(form->isVisible());
    QVERIFY(!form->isWindow());
    QVERIFY(!form->findChild<QComboBox *>("teamNewLeadAgent")->isVisible());
    form->findChild<QLineEdit *>("teamNewName")->setText("Checkout redesign");
    auto *create = form->findChild<QPushButton *>("teamNewCreate");
    QTRY_VERIFY(create->isEnabled());
    shot(&tab, "new-team-tab");
    create->click();
    QTRY_COMPARE(created, QStringLiteral("team-new"));
    QVERIFY(form->isVisible());   // the tab keeps it until Team replaces the tab
    const auto lead = calls(script(), "create").first()["lead"].toObject();
    QCOMPARE(lead["session"].toObject()["name"].toString(), QStringLiteral("claude/checkout/coord"));
    QVERIFY(!lead.contains("account"));
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
    workspace.addMemberHook = [](QDialog *dialog) {
        auto *role = dialog->findChild<QComboBox *>("teamAddMemberRole");
        role->setCurrentIndex(role->findText("Tester"));
        QCOMPARE(dialog->findChild<QComboBox *>("teamAddMemberAgent")->currentData().toString(), QStringLiteral("kimi"));
        QCOMPARE(dialog->findChild<QPlainTextEdit *>("teamAddMemberPrompt")->toPlainText(), QStringLiteral("Test it."));
        QTimer::singleShot(0, dialog, [dialog] { dialog->findChild<QPushButton *>("teamAddMemberOk")->click(); });
    };
    workspace.findChild<QPushButton *>("teamWorkspaceAddMember")->click();
    QTRY_COMPARE(calls(script(), "add-member").size(), 1);
    const auto input = calls(script(), "add-member").first();
    QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
    const auto member = input["member"].toObject();
    QCOMPARE(member["id"].toString(), QStringLiteral("tester-2"));
    QCOMPARE(member["role"].toString(), QStringLiteral("Tester"));
    QCOMPARE(member["model"].toString(), QStringLiteral("kimi-k2"));
}

void TestTeamsUi::designerPageStartsOrOpensTheDesigner()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    QString running; int started = 0; QString opened;
    page.designerSession = [&] { return running; };
    page.startDesigner = [&] { ++started; };
    page.openDesigner = [&](const QString &name) { opened = name; };
    page.resize(1300, 820); page.show();
    QTRY_VERIFY(controller.catalogReady());   // the page reads the catalog while shown
    auto *start = page.findChild<QPushButton *>("teamDesignerStart");
    auto *open = page.findChild<QPushButton *>("teamDesignerOpen");
    QVERIFY(start->isVisible()); QVERIFY(!open->isVisible());
    start->click(); QCOMPARE(started, 1);
    running = "claude/checkout/team-designer";
    page.refresh();
    QVERIFY(!start->isVisible()); QVERIFY(open->isVisible());
    open->click(); QCOMPARE(opened, running);
    QTRY_VERIFY(page.findChild<QWidget *>("teamDesignerContent"));
    shot(&page, "designer-page");
}

#include "test_teamsui.moc"
