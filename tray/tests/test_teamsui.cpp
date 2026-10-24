// Agent teams in the desktop, M3–M6: the task card, planning edits, New team, the catalog
// editor, the Team designer's draft and the team workspace. Each sends one
// `hgs team` request per change, recorded by a fake CLI.
#include <QPointer>
#include <QScrollBar>
#include <QtTest>
#include <QStyle>
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
#include <QScrollArea>
#include <QSplitter>
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
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2","computer":"devbox","state":"active","session":{"host":"devbox","name":"kimi/checkout/tester"}}],
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
  "templates":[{"id":"feature","name":"Feature delivery","lead_protocol":"Plan in waves of three","roster":[{"role":"backend","count":1},{"role":"tester","count":1}],
                "tasks":[{"title":"Plan","role":"lead","deps":[]},{"title":"Build","role":"backend","deps":[0]},{"title":"Test","role":"tester","deps":[1]}],
                "planning":"template","approval":"review","max_members":6}],
  "routing":[]},
 "draft":{"base":3,"proposals":[
  {"id":"p1","kind":"role","action":"change","target":"tester","title":"Tester: Kimi → Claude Sonnet","reason":"Deliveries to devbox fail",
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
    void catalogRoleChoosesAccountModelAndComputer();
    void designerDraftIsKeptThenApplied();
    void workspaceShowsGraphBoardAndFocus();
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
    // The list arrives before the full status with its plan errors.
    QTRY_VERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));
    auto *approve = panel.findChild<QPushButton *>("teamApprove");
    QTRY_VERIFY((approve = panel.findChild<QPushButton *>("teamApprove")) && !approve->isEnabled());   // #6 has no assignee
    QCOMPARE(panel.findChildren<QComboBox *>("teamMemberAgent").size(), 3);
    shot(&panel, "planning-420");
    // Nothing is wider than the panel.
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
    TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}, {"devbox", "devbox", false}},
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
    computer->setCurrentIndex(computer->findData("devbox"));
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
    const TeamNewTab::Coordinator coordinator{"claude/checkout/coord", "claude", "Opus", "work", QDir::toNativeSeparators(folder.path()), "laptop", "Ready", false};
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}, {"devbox", "devbox", false}}, coordinator, "Checkout");
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
    computer->setCurrentIndex(computer->findData("devbox")); emit computer->activated(computer->currentIndex());
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
