// WhiteWh fork: agent teams in the desktop, M3–M6 (design/agent-teams S3, S4,
// S6, S7, S10, S11): the task card, planning edits, New team, the catalog
// editor, the Team designer's draft and the team workspace. Each sends one
// `hgs team` request per change, recorded by a fake CLI.
#include <QPointer>
#include <QScrollBar>
#include <QtTest>
#include <QPainter>
#include <memory>
#include <tuple>
#include <QStandardItemModel>
#include <algorithm>
#include <QStyle>
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QJsonArray>
#include <QImage>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QScrollArea>
#include <QSpinBox>
#include <QSplitter>
#include <QTabWidget>
#include <QTemporaryDir>
#include <QTextBlock>
#include <QTextBrowser>
#include <QTimer>
#include <QUuid>
#include <QToolButton>
#include <QVBoxLayout>
#include "AgentWorking.h"
#include "CollapsibleBlock.h"
#include "HgsClient.h"
#include "SettingsPage.h"
#include "TeamCatalogEditor.h"
#include "TeamAttentionNotices.h"
#include <QClipboard>
#include "TeamController.h"
#include "TeamDesignerPage.h"
#include "TeamDesignerPanel.h"
#include "TeamModel.h"
#include "TeamNewDialog.h"
#include "TeamNewTab.h"
#include "TeamPanel.h"
#include "TeamTaskCard.h"
#include "TeamTemplateList.h"
#include "TeamWidgets.h"
#include "TeamWorkspace.h"
#include "TeamPhaseScroll.h"
#include "WorkspaceStyle.h"
#include <QScopeGuard>
#include <QElapsedTimer>
#include <QInputDialog>
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

// Opens the member menu of `member` and hands the active menu to `inspect`.
void inspectMemberMenu(QWidget *root, const QString &member, const std::function<void(QMenu *)> &inspect)
{
    QToolButton *more = nullptr;
    for (auto *card : root->findChildren<QFrame *>("teamWorkspaceMember"))
        if (card->property("teamMember") == member && card->isVisibleTo(root)) more = card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
    QVERIFY(more);
    bool seen = false;
    std::function<void(int)> look = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *menu = qobject_cast<QMenu *>(QApplication::activePopupWidget());
            if (!menu) { if (tries > 0) look(tries - 1); return; }
            seen = true;
            inspect(menu);
            menu->hide();
        });
    };
    // Up to 10 s: under a loaded machine the popup can take seconds to show.
    look(1000);
    more->click();
    QTRY_VERIFY_WITH_TIMEOUT(seen, 10000);
}

// U25 part 5 (spec 5.4): the prototype's checkout - phase 1 done, phase 2
// Build running (a question on #2, #3 at work, a long edge #3 -> #6 past #5),
// phase 3 a draft that takes edge edits.
QJsonObject graphTeam()
{
    auto t = QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Checkout v2","status":"running","project":"Checkout","folder":"/work/checkout","lead":"lead",
 "revision":21,"created":1000,"updated":5000,"settings":{"approval":"review","phase_approval":"review"},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","model":"claude-opus-4-6","state":"active","session":{"host":"","name":"claude/checkout/lead"}},
  {"id":"backend","role":"Backend","agent":"dsh","model":"deepseek-v4-pro","state":"active","session":{"host":"","name":"dsh/checkout/backend"}},
  {"id":"frontend","role":"Frontend","agent":"codex","model":"gpt-6.1-sol","state":"active","session":{"host":"","name":"codex/checkout/frontend"}},
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2.5","state":"active","session":{"host":"","name":"kimi/checkout/tester"}}],
 "phases":[
  {"n":2,"order":2,"title":"Build","state":"approved","tasks":[2,3,4,5,6],"added_by":"Lead","added_at":1500},
  {"n":3,"order":3,"title":"Verify & ship","state":"draft","tasks":[7,8,9],"added_by":"Lead","added_at":1700}],
 "tasks":[
  {"id":1,"phase":1,"title":"Requirements and acceptance","assignee":"lead","status":"accepted"},
  {"id":2,"phase":2,"title":"Saved cards API","assignee":"backend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a2","member":"backend","state":"delivered","sent_at":1600,"questions":[{"id":"q1","text":"Keep the old field?","at":1650}]}]},
  {"id":3,"phase":2,"title":"Card form UI","assignee":"frontend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a3","member":"frontend","state":"delivered","sent_at":1600,"started_at":1610}]},
  {"id":4,"phase":2,"title":"Address validation","assignee":"frontend","status":"submitted","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a4","member":"frontend","state":"reported","report":{"text":"Done","at":1700}}]},
  {"id":5,"phase":2,"title":"Contract tests","assignee":"tester","status":"pending","deps":[{"task":2},{"task":3}]},
  {"id":6,"phase":2,"title":"E2E checkout flow","assignee":"tester","status":"pending","deps":[{"task":3},{"task":4},{"task":5}]},
  {"id":7,"phase":3,"title":"Release notes","assignee":"lead","status":"draft","deps":[{"task":8},{"task":9}]},
  {"id":8,"phase":3,"title":"Rate limits on /v1/cards","assignee":"backend","status":"draft","deps":[{"task":2}]},
  {"id":9,"phase":3,"title":"Load test checkout","assignee":"tester","status":"draft","deps":[{"task":6}]}],
 "layers":{"1":0,"2":1,"3":1,"4":1,"5":2,"6":3,"7":5,"8":2,"9":4},"cycle":[],"ready":[],
 "validation":{"errors":[],"warnings":[]}})").object();
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

// U25: a phase's scroll in the Team tab, an action in its header, its graph,
// and every task node the open phase graphs show (sorted).
QFrame *phaseScrollOf(QWidget *root, int phase)
{
    for (auto *frame : root->findChildren<QFrame *>("teamPhaseScroll")) if (frame->property("phase").toInt() == phase) return frame;
    return nullptr;
}
QPushButton *phaseAction(QWidget *root, int phase, const char *name)
{
    auto *frame = phaseScrollOf(root, phase);
    return frame ? frame->findChild<QPushButton *>(name) : nullptr;
}
bool phaseOpen(QWidget *root, int phase)
{
    auto *frame = phaseScrollOf(root, phase);
    return frame && frame->findChild<QWidget *>("teamPhaseBody");
}
void togglePhase(QWidget *root, int phase)
{
    if (auto *frame = phaseScrollOf(root, phase)) if (auto *toggle = frame->findChild<QToolButton *>("teamPhaseToggle")) toggle->click();
}
TeamUi::TaskView *phaseGraphOf(QWidget *root, int phase)
{
    auto *frame = phaseScrollOf(root, phase);
    return frame ? dynamic_cast<TeamUi::TaskView *>(frame->findChild<QWidget *>("teamPhaseGraph")) : nullptr;
}
QList<int> phaseNodeIds(QWidget *root)
{
    QList<int> ids;
    for (auto *w : root->findChildren<QWidget *>("teamPhaseGraph"))
        if (auto *view = dynamic_cast<TeamUi::TaskView *>(w)) for (const auto &node : view->nodes) ids.append(node.id);
    std::sort(ids.begin(), ids.end());
    return ids;
}

// A point in a TaskView's widget coordinates: its painted rects are in graph
// coordinates, which the view scales by its zoom.
QPoint viewPoint(const TeamUi::TaskView *view, const QRectF &rect)
{
    return QPoint(int(std::round(rect.center().x() * view->zoom)), int(std::round(rect.center().y() * view->zoom)));
}

// Arms the owner's confirmation on the next modal dialog: `cancel` clicks the
// reject (Cancel) button, otherwise the destructive action button. The dialog
// runs its own event loop inside the click, so a zero timer lands inside it; the
// retry covers a timer that fires before the dialog is up.
// Clicks `point` on `view` and reports whether a confirmation opened; one that
// opens is cancelled, so a regression fails the check instead of hanging.
bool clickAsks(QWidget *view, const QPoint &point)
{
    bool asked = false;
    QTimer watch; watch.setInterval(5);
    QObject::connect(&watch, &QTimer::timeout, [&asked] {
        if (auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget())) { asked = true; box->reject(); }
    });
    watch.start();
    QTest::mouseClick(view, Qt::LeftButton, {}, point);
    QTest::qWait(60);
    return asked;
}

// Picks the action named `name` from the next popup menu (QMenu::exec blocks).
void chooseMenuAction(const QString &name, int tries = 300)
{
    QTimer::singleShot(10, qApp, [name, tries] {
        auto *menu = qobject_cast<QMenu *>(QApplication::activePopupWidget());
        if (!menu) { if (tries > 0) chooseMenuAction(name, tries - 1); return; }
        QAction *found = nullptr;
        for (auto *action : menu->actions()) if (action->objectName() == name) found = action;
        menu->hide();
        if (found && found->isEnabled()) found->trigger();
    });
}

// #344: a running team with what the lead proposes (F8), a swap's old session
// (H6), a member at a usage limit (H1) and two members on one DeepSeek host (H3),
// shaped like `hgs team status` (spec 4, 5, 6.1, 6.3).
QJsonObject proposalsTeam(double resetsAt)
{
    auto t = team();
    t["settings"] = QJsonObject{{"approval", "review"}, {"report_timeout_seconds", 120}, {"resume_after_limits", false}};
    auto members = t["members"].toArray();
    auto backend = members[1].toObject();
    backend["agent"] = "dsh";
    backend["limited"] = QJsonObject{{"at", resetsAt - 3600}, {"resets_at", resetsAt}, {"detail", "Usage limit reached \u00b7 resets 5pm"}};
    backend["host_shared"] = true;
    members[1] = backend;
    auto tester = members[2].toObject();
    tester["agent"] = "dsh";
    tester["host_shared"] = true;
    members[2] = tester;
    t["members"] = members;
    t["hosts"] = QJsonArray{QJsonObject{{"agent", "dsh"}, {"pid", 4242}, {"started", 1000.0}, {"members", QJsonArray{"backend", "tester"}}}};
    t["proposals"] = QJsonArray{
        QJsonObject{{"id", "p1"}, {"kind", "add-member"}, {"by", "Lead"}, {"at", 2000.0}, {"state", "open"}, {"reason", "The UI needs a designer"},
                    {"member", QJsonObject{{"role", "Designer"}, {"agent", "claude"}, {"model", "sonnet"}}}},
        QJsonObject{{"id", "p2"}, {"kind", "swap-member"}, {"by", "Lead"}, {"at", 2100.0}, {"state", "open"}, {"reason", "Backend hit its limit"},
                    {"member", QJsonObject{{"member", "backend"}, {"agent", "codex"}, {"model", "gpt-5.5"}}}},
        QJsonObject{{"id", "p3"}, {"kind", "remove-member"}, {"by", "Lead"}, {"at", 2200.0}, {"state", "open"}, {"reason", "Testing is done"},
                    {"member", "tester"}},
        QJsonObject{{"id", "p4"}, {"kind", "next-goal"}, {"by", "Lead"}, {"at", 1900.0}, {"state", "rejected"}, {"reason", "Later"},
                    {"goal", "Saved cards"}}};
    t["swapped"] = QJsonArray{QJsonObject{{"member", "Backend"}, {"old", "dsh/checkout/backend-old"}, {"at", 1800.0}, {"by", "You"}}};
    return t;
}

void armConfirm(TeamPanel *panel, bool cancel, int tries = 300)
{
    QTimer::singleShot(0, panel, [panel, cancel, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) QTimer::singleShot(10, panel, [panel, cancel, tries] { armConfirm(panel, cancel, tries - 1); });
            return;
        }
        for (auto *button : box->buttons())
            if (cancel ? box->buttonRole(button) == QMessageBox::RejectRole
                       : box->buttonRole(button) == QMessageBox::DestructiveRole) { button->click(); return; }
        for (auto *button : box->buttons())
            if (!button->text().contains(QStringLiteral("Cancel"))) { button->click(); return; }
    });
}

// Answers the next confirmation after recording its title and text: Cancel
// clicks the reject button, otherwise the destructive action. F12/N40 uses it
// to check what the owner is warned about.
void armConfirmRecord(TeamPanel *panel, bool cancel, QString *title, QString *text, int delay = 0, int tries = 300)
{
    QTimer::singleShot(delay, panel, [panel, cancel, title, text, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) armConfirmRecord(panel, cancel, title, text, 10, tries - 1);
            return;
        }
        if (title) *title = box->windowTitle();
        if (text) *text = box->text();
        for (auto *button : box->buttons())
            if (cancel ? box->buttonRole(button) == QMessageBox::RejectRole
                       : box->buttonRole(button) == QMessageBox::DestructiveRole) { button->click(); return; }
    });
}

// Answers the next confirmation after running `during` while it is open, as a
// poll arriving under the dialog does.
void armConfirmAfter(TeamPanel *panel, std::function<void()> during, int tries = 300)
{
    QTimer::singleShot(0, panel, [panel, during, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) QTimer::singleShot(10, panel, [panel, during, tries] { armConfirmAfter(panel, during, tries - 1); });
            return;
        }
        during();
        for (auto *button : box->buttons())
            if (box->buttonRole(button) == QMessageBox::DestructiveRole) { button->click(); return; }
    });
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

// True when the grab is really light: the average pixel lightness is well above
// the dark theme's. A "light" shot of a dark widget is a lie, so it fails.
bool looksLight(QWidget *widget)
{
    const auto image = widget->grab().toImage();
    if (image.isNull()) return false;
    qint64 sum = 0; int count = 0;
    for (int y = 0; y < image.height(); y += 5)
        for (int x = 0; x < image.width(); x += 5) { sum += image.pixelColor(x, y).lightness(); ++count; }
    return count > 0 && sum / count > 150;
}

// The light half of a shot, asserted to be light before it is saved.
void lightShot(QWidget *widget, const QString &name)
{
    lightPalette();
    QTest::qWait(200);
    QVERIFY2(looksLight(widget), qPrintable(QStringLiteral("the light shot is dark: ") + name));
    shot(widget, name);
}

// F6: chooses a template the way a click does, in the form's compact list.
bool chooseTemplate(QWidget *form, const QString &id)
{
    auto *rows = form->findChild<QListWidget *>("teamTemplateList");
    if (!rows) return false;
    for (int i = 0; i < rows->count(); ++i)
        if (rows->item(i)->data(Qt::UserRole).toString() == id) { rows->setCurrentRow(i); return true; }
    return false;
}

// F5: a draft as `hgs team status` returns it (spec 3): status "draft", the
// draft block with its revision and the fields the lead changed unseen, the
// goal, the lead protocol (the addendum), the full settings, the lead with its
// session, members not created yet, draft tasks and the preflight.
QJsonObject draftTeam(const QString &folder, int revision = 3, const QJsonArray &unseen = {"members", "goal"}, const QJsonArray &errors = {})
{
    auto t = QJsonDocument::fromJson(R"({
 "id":"draft-1","name":"Checkout redesign","status":"draft","project":"","lead":"lead",
 "goal":"Redesign checkout: address form and tests","lead_protocol":"Plan in waves of two",
 "revision":7,"created":1000,"updated":5000,
 "settings":{"all_hard":false,"report_timeout_seconds":120,"folder_policy":"auto","approval":"review","max_members":6},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","state":"active","session":{"host":"","name":"claude/checkout/coord"}},
  {"id":"backend","role":"Backend","agent":"codex","model":"gpt-5.5","folder":"worktree","state":"not_created","prompt":"Build the API."},
  {"id":"frontend","role":"Frontend","agent":"dsh","model":"flash","folder":"worktree","state":"not_created"}],
 "tasks":[
  {"id":1,"title":"Address form API","assignee":"backend","status":"draft"},
  {"id":2,"title":"Address form UI","assignee":"frontend","status":"draft","deps":[{"task":1,"kind":"hard"}]}],
 "layers":{"1":0,"2":1},"cycle":[],"ready":[]})").object();
    t["folder"] = folder;
    t["draft"] = QJsonObject{{"revision", revision}, {"unseen", unseen}, {"owner_edited", QJsonArray{"settings"}}};
    t["validation"] = QJsonObject{{"errors", errors},
                                  {"warnings", QJsonArray{QJsonObject{{"kind", "prompt"}, {"text", "Frontend prompt says commit; only the lead commits"}}}}};
    return t;
}
}

class TestTeamsUi : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void init();
    void settingsPageShowsTheSubagentSetting();
    void aSessionRecordShowsTheUnconfirmableSubagentNote();
    void catalogModelAppliesKeptProposals();
    void taskCardOffersActionsByStatus();
    void attemptStatesShowPhaseIcons();
    void taskCardAnswersQuestionsAndHolds();
    void memberQueueListsCurrentAndReturned();
    void permissionAnswersFoldToOneLineAndOpenToFive();
    void planningEditsSendTheWholePlan();
    void newTeamStartsTheLeadWithTheTemplate();
    void catalogEditorSavesTheNextVersion();
    void catalogEditorKeepsTheFieldsItDoesNotEdit();
    void ownerActionsCarryNoAgentMarkers();
    void permissionAttentionNamesTheMemberAndAnswers();
    void sendBackFormBelongsToItsTeamAndPhase();
    void catalogRoleChoosesAccountModelAndComputer();
    void designerDraftIsKeptThenApplied();
    void workspaceShowsGraphBoardAndFocus();
    void workspaceGroupsTasksByPhase();
    void phaseApprovalAndFinishAskTheOwner();
    void phaseApprovalAlongsideARunningPhaseAsksFirst();
    void aRefreshUnderThePhaseConfirmationKeepsItsAsker();
    void leftoverMembersAreShownApart();
    void memberEditorPicksAnExistingSession();
    void rolePromptFromTheMemberMenuKeepsItsButton();
    void phaseProgressShowsTwoBars();
    void permissionCardsAndGraphOrder();
    void preflightWarningsShowInThePlanCard();
    void leadLoadViewsShowInTheDesktop();
    void followUpsShowThePermissionAnswerAndPlanFixes();
    void folderPolicyKeepsAutoWhileTyping();
    void typoThenGitCreatesWorktrees();
    void severalDraftPhasesRenderAndApproveInOrder();
    void phaseBandsStackWithStatusDecisionAndHealth();
    void draftReorderSendsMovePhase();
    void pinMarkAndFinishedTeamShowWhoFinished();
    void catalogEditorCarriesNewPhaseApproval();
    void phaseRetriesReuseTheRequestId();
    void malformedPhasesFallBack();
    void lightThemeShots();
    void deliveryShots();
    void emptyPanelOffersNewTeamAndDesigner();
    void embeddedNewTeamIsLedByTheSession();
    void draftDiscardAsksAndCanStopTheLead();
    void draftUpdateRetriesAStaleRevision();
    void templatesAreCompactWithSearchAndFavourites();
    void dialogTemplateListAppliesOnlyWhenChosen();
    void designerSessionKeepsItsNameAndLeadsNoTeam();
    void nextGoalFromADoneTeam();
    void proposalsAreAcceptedOrRejectedOnce();
    void swapMemberAsksFirstAndListsTheOldSession();
    void memberMenuWorksBeforeTheStatusReadLands();
    void limitedAndSharedHostMembersAreMarked();
    void reportPreviewsAndFindings();
    void hierarchyInTeamViewsAndMemberCards();
    void workspaceAddsAMember();
    void graphRoutesEdgesAroundNodes();
    void graphEdgesAreEditedByHandleAndSelection();
    void graphRoutingStaysFastForTheLargestPlan();
    void graphNodesInTwoLinesWithDraftEdits();
    void subLeadsAreATreeInMembersAndTheInspector();
    void aPollUnderTheMemberMenuKeepsItsButtonAndTheSwapDialog();
    void designerPageStartsOrOpensTheDesigner();
    void catalogEditorMarksManualEdits();
    void boardColumnsKeepTheirScroll();
    void skinTokensMatchThePrototype();
    void rolePaletteReadsTheCatalog();
    void scrollBarsAreThin();
    void gearAndBellOffIcons();
    void collapsibleBlockKeepsItsState();
    void agentWorkingIndicatorFollowsTheSetting();
    void foundationShots();
    void leadHeaderFollowsTheTeamState();
    void leadHeaderShots();
    void phaseScrollsInEveryView();
    void phaseScrollDecisions();
    void sideListsAndMemberTasks();
private:
    QString script() const { return m_dir.filePath("hgs"); }
    void setTeam(const QJsonObject &value) { QVERIFY(FakeCli::replace(script() + ".status", QJsonDocument(value).toJson(QJsonDocument::Compact))); }
    QTemporaryDir m_dir;
};

void TestTeamsUi::initTestCase()
{
    QVERIFY(m_dir.isValid());
    TeamController::setPollInterval(60000);
    TeamNewTab::setDraftPollInterval(100);
    darkPalette();
    QFile c(script() + ".catalog"); QVERIFY(c.open(QIODevice::WriteOnly)); c.write(QJsonDocument(catalog()).toJson(QJsonDocument::Compact)); c.close();
    QFile f(script() + ".feed"); QVERIFY(f.open(QIODevice::WriteOnly)); f.write(R"({"items":[{"seq":2,"at":1300,"kind":"reported","detail":{"by":"Backend","task":2,"attempt":1}}],"next_cursor":null})"); f.close();
    QVERIFY(FakeCli::install(script(), R"PY(#!/usr/bin/env python3
import json,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
body=json.load(sys.stdin) if a[:1]==['team'] else {}
open(__file__+'.calls','a').write(json.dumps({'args':a,'input':body})+'\n')
import os
open(__file__+'.env','a').write(json.dumps({'args':a,'markers':sorted(k for k in os.environ if k in ('HGS_SESSION','HGS_RUN_ID','DSH_SESSION_ID','HGS_LAUNCH_ID','HGS_FRESH'))})+'\n')
if a[:2]==['team','list']:
 t=json.load(open(__file__+'.status'))
 for key in ('layers','cycle','ready','validation'):t.pop(key)
 print(json.dumps({'items':[t]}))
elif a[:2]==['team','status']:
 import os,time
 # A test holds the status read back while `.statushold` exists (30 s at most).
 for _ in range(300):
  if not os.path.exists(__file__+'.statushold'):break
  time.sleep(.1)
 sys.stdout.write(open(__file__+'.status').read())
elif a[:2]==['team','events']:sys.stdout.write(open(__file__+'.feed').read())
elif a[:3]==['team','catalog','get']:sys.stdout.write(open(__file__+'.catalog').read())
elif a[:2]==['team','approve-phase'] and body.get('phase')==9:
 print('hgs: Phase 9 is not waiting for approval',file=sys.stderr);sys.exit(1)
elif a[:2]==['team','create']:print(json.dumps({'status':'created','team':'team-new','request_id':body.get('request_id')}))
elif a[:2]==['team','next-goal']:print(json.dumps({'status':'planned','team':'team-2','previous':body.get('team')}))
elif a[:2]==['team','draft']:
 import os
 act=body.get('action')
 if act=='start':print(json.dumps({'team':'draft-1','revision':1}))
 elif act=='update':
  rev=int(open(__file__+'.draftrev').read()) if os.path.exists(__file__+'.draftrev') else body.get('revision')
  if body.get('revision')!=rev:print('hgs: Draft revision is %d now; read it again'%rev,file=sys.stderr);sys.exit(1)
  if os.path.exists(__file__+'.draftrev'):open(__file__+'.draftrev','w').write(str(rev+1))
  print(json.dumps({'status':'updated','draft_revision':rev+1,'revision':rev+40,'unseen':[]}))
 elif act=='seen':print(json.dumps({'status':'seen','unseen':[f for f in ['members','goal'] if f not in body.get('fields',[])]}))
 elif act=='create':print(json.dumps({'status':'planning','team':body.get('team')}))
 elif act=='discard':print(json.dumps({'status':'stopped','stop_lead':body.get('stop_lead',False)}))
 else:print(json.dumps({'status':'ok'}))
elif a[:2]==['account','ls']:print(json.dumps({'profiles':[{'id':'native-claude','provider':'claude','label':'Default','account_status':{'status':'ok'}},
 {'id':'work','provider':'claude','label':'Claude work','account_status':{'status':'signed_out'}}]}))
else:print(json.dumps({'status':'ok','version':4,'request_id':body.get('request_id')}))
)PY"));
}

void TestTeamsUi::init()
{
    QFile::remove(script() + ".calls");
    QFile::remove(script() + ".draftrev");
    QFile::remove(script() + ".statushold");
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
    // U25: the drafts are scrolls stacked in order, each with its Accept,
    // Reject… and a menu that moves it among the drafts or discards it.
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 8);
    for (int n = 2; n <= 8; ++n) QCOMPARE(phaseScrollOf(&workspace, n)->property("state").toString(), QStringLiteral("draft"));
    QVERIFY(phaseScrollOf(&workspace, 2)->findChild<QLabel *>("teamPhaseTitle")->text().contains(QStringLiteral("QA fixes")));
    const auto fromMenu = [&](int phase, const char *action) {
        chooseMenuAction(action);
        phaseScrollOf(&workspace, phase)->findChild<QToolButton *>("teamPhaseMore")->click();
        QTest::qWait(100);
    };
    // The first draft cannot go up.
    fromMenu(2, "teamPhaseMoveUp");
    QCOMPARE(calls(script(), "move-phase").size(), 0);
    fromMenu(3, "teamPhaseMoveUp");
    QTRY_COMPARE(calls(script(), "move-phase").size(), 1);
    auto moved = calls(script(), "move-phase").first();
    QCOMPARE(moved["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(moved["phase"].toInt(), 3);
    QCOMPARE(moved["to"].toInt(), 2);
    QCOMPARE(moved["position"].toString(), QStringLiteral("before"));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 2)->findChild<QToolButton *>("teamPhaseMore"));
    fromMenu(2, "teamPhaseMoveDown");
    QTRY_COMPARE(calls(script(), "move-phase").size(), 2);
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
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *second = phaseScrollOf(&workspace, 2);
        return second && second->findChild<QLabel *>("teamPhaseTitle")->text() == QStringLiteral("Phase 2 · Polish")
               && workspace.findChildren<QFrame *>("teamPhaseScroll").size() == 8;
    })());
    // QA fixes now waits for the draft that moved before it, at phase 2.
    auto *qaAccept = phaseAction(&workspace, 3, "teamPhaseAccept");
    QVERIFY(qaAccept && !qaAccept->isEnabled());
    QVERIFY(qaAccept->toolTip().contains(QStringLiteral("after phase 2")));
    shot(&workspace, "phases-7-drafts-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTest::qWait(250);
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

void TestTeamsUi::permissionAnswersFoldToOneLineAndOpenToFive()
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
    QSettings().remove("ui/expanded/teamPermissions/team-1/backend");
    QSettings().remove("ui/expanded/taskPermissions/team-1/1/1");
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
    // B6: the folder policy combo is the control and falls back to shared here.
    auto *policy = dialog.findChild<QComboBox *>("teamNewFolderPolicy");
    QVERIFY(policy);
    QCOMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    shot(&dialog, "new-team-folder-policy");
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
    // F6: the template comes from the compact list, after the form is built.
    QVERIFY(!dialog.findChild<QComboBox *>("teamNewTemplate"));
    QVERIFY(chooseTemplate(&dialog, "feature"));
    QCOMPARE(dialog.templateId(), QStringLiteral("feature"));
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

// N1 (#379): a desktop Save (and Export, the same json) carries the U25
// fields the editor does not show - a role's about, icon, skills, prompt
// order and notes, a template's work and phases, a seat's subteam, a skeleton
// task's phase, a rule's use - so fixing a typo keeps them, and a "+role"
// routing rule still names its role.
void TestTeamsUi::catalogEditorKeepsTheFieldsItDoesNotEdit()
{
    const auto restore = qScopeGuard([this] {
        QFile c(script() + ".catalog"); if (c.open(QIODevice::WriteOnly)) c.write(QJsonDocument(catalog()).toJson(QJsonDocument::Compact));
    });
    const auto u25 = QJsonDocument::fromJson(R"({"builtin":false,"catalog":{"version":7,
      "roles":[{"id":"lead","name":"Laed","agent":"claude","prompt":"You lead.","about":"Plans and reviews","icon":"crown",
                "skills":["planning"],"machines":["devbox"],"message":"all","message_to":["tester"],"joins":"always",
                "danger":{"level":"high"},"protocol_extra":"Ask before merging.","task_template":"Do {task}.",
                "prompt_order":["role","rules","task"],"prompt_off":["examples"],"notes":{"tone":"short"},
                "can_lead_subteam":true,"permissions_profile":"commit"},
               {"id":"tester","name":"Tester","agent":"kimi","prompt":"Test it."}],
      "templates":[{"id":"feature","name":"Feature delivery","roster":[{"role":"tester","count":1,"subteam":"qa"}],
                    "tasks":[{"title":"Plan","role":"lead","deps":[],"phase":1},{"title":"Test","role":"tester","deps":[0],"phase":2}],
                    "planning":"template","approval":"review","phase_approval":"auto","max_members":6,"work":"clone","phases":"lead"},
                   {"id":"qa","name":"QA","roster":[],"tasks":[],"planning":"lead","approval":"review","max_members":3}],
      "routing":[{"id":"add-tester","when":"tests","template":"","use":"+tester","approval":""},
                 {"id":"features","when":"feature","template":"","use":"feature","approval":"review"}]}})").object();
    { QFile c(script() + ".catalog"); QVERIFY(c.open(QIODevice::WriteOnly)); c.write(QJsonDocument(u25).toJson(QJsonDocument::Compact)); }
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(900, 700); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 2);
    // The typo, fixed in the lead's name.
    auto *name = editor.findChild<QLineEdit *>("teamRoleName"); QVERIFY(name); QCOMPARE(name->text(), QStringLiteral("Laed"));
    name->setText("Lead"); emit name->editingFinished();
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave"); QTRY_VERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto sent = calls(script(), "catalog set").first()["catalog"].toObject();
    const auto before = u25["catalog"].toObject();
    // The lead: the new name, every other field as it came.
    auto lead = sent["roles"].toArray()[0].toObject();
    QCOMPARE(lead["name"].toString(), QStringLiteral("Lead"));
    auto expected = before["roles"].toArray()[0].toObject(); expected["name"] = "Lead";
    for (auto it = expected.begin(); it != expected.end(); ++it) QVERIFY2(lead.value(it.key()) == it.value(), qPrintable(it.key()));
    // The template's work and phases, the seat's subteam, the tasks' phases.
    const auto feature = sent["templates"].toArray()[0].toObject();
    QCOMPARE(feature["work"].toString(), QStringLiteral("clone")); QCOMPARE(feature["phases"].toString(), QStringLiteral("lead"));
    QCOMPARE(feature["roster"].toArray()[0].toObject()["subteam"].toString(), QStringLiteral("qa"));
    QCOMPARE(feature["tasks"].toArray()[0].toObject()["phase"].toInt(), 1);
    QCOMPARE(feature["tasks"].toArray()[1].toObject()["phase"].toInt(), 2);
    // The rules keep their use: "+tester" adds a member, "feature" picks the template.
    const auto rules = sent["routing"].toArray();
    QCOMPARE(rules[0].toObject()["use"].toString(), QStringLiteral("+tester"));
    QCOMPARE(rules[1].toObject()["use"].toString(), QStringLiteral("feature"));
    QCOMPARE(rules[1].toObject()["template"].toString(), QStringLiteral("feature"));
}

// N5 (#359): a GUI started from an agent's shell (a dsh session included)
// runs every hgs as the person: no hgs session, run, dsh conversation or
// launch marker reaches it, so `resolve_actor` sees the owner, not an agent.
void TestTeamsUi::ownerActionsCarryNoAgentMarkers()
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
    // U25: nothing waits, so the right column has no Needs your answer list.
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceAnswers"));
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
    // U25: the views carry their icons and their names.
    auto *views = workspace.findChild<QWidget *>("teamWorkspaceView");
    for (auto *b : views->findChildren<QPushButton *>()) QVERIFY(!b->text().isEmpty() && !b->icon().isNull());
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
    // U25: every phase is a scroll, stacked top to bottom; the finished one
    // starts collapsed and builds no graph.
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 3);
    QCOMPARE(phaseScrollOf(&workspace, 1)->property("state").toString(), QStringLiteral("done"));
    QCOMPARE(phaseScrollOf(&workspace, 2)->property("state").toString(), QStringLiteral("running"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->property("state").toString(), QStringLiteral("draft"));
    QCOMPARE(phaseScrollOf(&workspace, 2)->findChild<QLabel *>("teamPhaseTitle")->text(), QStringLiteral("Phase 2 · QA fixes"));
    QVERIFY(!phaseOpen(&workspace, 1) && phaseOpen(&workspace, 2) && phaseOpen(&workspace, 3));
    QCOMPARE(phaseNodeIds(&workspace), (QList<int>{3, 4, 5}));
    QVERIFY(phaseScrollOf(&workspace, 1)->findChild<QLabel *>("teamPhaseSummary")->text().contains("#1 Requirements"));
    shot(&workspace, "d3-phases-graph-collapsed");
    togglePhase(&workspace, 1);   // the header opens the finished phase, its tasks come back
    QTRY_VERIFY(phaseOpen(&workspace, 1));
    QTRY_COMPARE(phaseNodeIds(&workspace).size(), 5);
    togglePhase(&workspace, 2);   // the current phase closes; its tasks go, the others stay
    QTRY_VERIFY(!phaseOpen(&workspace, 2));
    QTRY_COMPARE(phaseNodeIds(&workspace), (QList<int>{1, 2, 5}));
    togglePhase(&workspace, 2);
    QTRY_VERIFY(phaseOpen(&workspace, 2));
    QTRY_COMPARE(phaseNodeIds(&workspace).size(), 5);
    QCOMPARE(TeamUi::phaseSummaryText(2, 2), QStringLiteral("2/2 done · click to open"));
    QCOMPARE(TeamUi::phaseSummaryText(1, 2), QStringLiteral("1/2 · click to open"));
    togglePhase(&workspace, 2);   // closed again for the views below
    QTRY_VERIFY(!phaseOpen(&workspace, 2));
    shot(&workspace, "d3-phases-graph-open");
    const auto showView = [&workspace](const QString &view) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == view) { b->click(); break; }
    };
    const auto rowsOf = [&workspace](const char *name, const char *rowName) {
        QList<int> ids;
        for (auto *row : workspace.findChildren<QFrame *>(rowName)) ids.append(row->property("task").toInt());
        std::sort(ids.begin(), ids.end());
        Q_UNUSED(name);
        return ids;
    };
    // Layers keep the same scrolls, a row per task in the open ones.
    showView("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(rowsOf("teamWorkspaceLayers", "teamLayerRow"), (QList<int>{1, 2, 5}));
    QVERIFY(!phaseOpen(&workspace, 2));
    shot(&workspace, "d3-phases-layers");
    // The board too; a collapsed phase shows only its header.
    showView("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QTRY_COMPARE(rowsOf("teamWorkspaceBoard", "teamBoardCard"), (QList<int>{1, 2, 5}));
    QCOMPARE(workspace.findChildren<QFrame *>("teamPhaseScroll").size(), 3);
    shot(&workspace, "d3-phases-board-collapsed");
    togglePhase(&workspace, 2);
    QTRY_COMPARE(rowsOf("teamWorkspaceBoard", "teamBoardCard"), (QList<int>{1, 2, 3, 4, 5}));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2) && !phaseScrollOf(&workspace, 2)->findChild<QToolButton *>("teamPhaseToggle")->property("collapsed").toBool());
    shot(&workspace, "d3-phases-board");
}

void TestTeamsUi::aRefreshUnderThePhaseConfirmationKeepsItsAsker()
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

void TestTeamsUi::phaseApprovalAlongsideARunningPhaseAsksFirst()
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
    // Cancel: the warning names the cost and nothing is sent.
    QString title, text;
    armConfirmRecord(&panel, true, &title, &text);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTest::qWait(150);
    QVERIFY2(title.contains(QStringLiteral("Approve phase 3 now?")), qPrintable(title));
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
    // The team bar says the phase waits; the draft's band header carries the
    // status chip, progress and health, and the two owner decisions (G9).
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamBarPendingPhase"));
    // U25: the draft's scroll carries its status, progress and the owner's decisions.
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    QCOMPARE(phaseScrollOf(&workspace, 3)->property("state").toString(), QStringLiteral("draft"));
    workspace.render();
    QTest::qWait(100);
    shot(&workspace, "d3-phases-draft");
    // The draft is ready once `team status` has answered (a list poll carries no validation).
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY(phaseAction(&workspace, 3, "teamPhaseReject"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceFinish"));   // tasks are still open
    // The header folds the scroll to its header row; the members column folds independently.
    auto *membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    QVERIFY(membersToggle);
    togglePhase(&workspace, 3);
    QTRY_VERIFY(phaseScrollOf(&workspace, 3) && !phaseOpen(&workspace, 3));
    shot(&workspace, "d3-phases-folded");
    togglePhase(&workspace, 3);
    QTRY_VERIFY(phaseOpen(&workspace, 3));
    membersToggle = workspace.findChild<QToolButton *>("teamMembersToggle");
    membersToggle->click();
    QTRY_VERIFY(!workspace.findChild<QFrame *>("teamWorkspaceMember"));
    shot(&workspace, "d3-members-folded");
    workspace.findChild<QToolButton *>("teamMembersToggle")->click();
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceMember"));
    shot(&workspace, "d3-phases-draft-open");
    // The same draft in the light theme.
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTest::qWait(250);
    shot(&workspace, "d3-phases-draft-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    // Accept asks the owner first, then sends once with the phase; a second
    // click before the poll sends nothing. A cancelled confirmation sends
    // nothing and leaves Accept usable.
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    armConfirm(&panel, true);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "approve-phase").size(), 0);
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept") && phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    armConfirm(&panel, false);
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "approve-phase").first()["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(calls(script(), "approve-phase").first()["by"].toString(), QStringLiteral("human"));
    // The sent decision holds until the store's revision moves: neither Accept
    // nor Reject… nor the menu acts again (no double send from a double click
    // or a quick re-render).
    QTest::qWait(300);
    workspace.render();
    QTest::qWait(100);
    QTRY_VERIFY(phaseAction(&workspace, 3, "teamPhaseAccept"));
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseAccept")->isEnabled());
    QVERIFY(!phaseAction(&workspace, 3, "teamPhaseReject")->isEnabled());
    QVERIFY(!phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->isEnabled());
    phaseAction(&workspace, 3, "teamPhaseAccept")->click();
    QTest::qWait(80);
    QCOMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    // The store's next revision reopens the header for the same phase; the
    // Discard test runs on it.
    auto decidedTeam = ready;
    decidedTeam["revision"] = decidedTeam["revision"].toInt(1) + 1;
    setTeam(decidedTeam);
    QTRY_VERIFY(([&] { controller.refresh(); auto *accept = phaseAction(&workspace, 3, "teamPhaseAccept"); return accept && accept->isEnabled(); })());
    // Discard, from the phase's menu, asks to confirm: Cancel sends nothing, the confirmation sends once.
    const auto discardFromMenu = [&] {
        chooseMenuAction("teamPhaseDiscard");
        phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->click();
    };
    armConfirm(&panel, true);
    discardFromMenu();
    QTest::qWait(200);
    QCOMPARE(calls(script(), "discard-phase").size(), 0);
    QTRY_VERIFY(phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QToolButton *>("teamPhaseMore")->isEnabled());
    armConfirm(&panel, false);
    discardFromMenu();
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

// #465 C1: "Role prompt…" opens its dialog from inside the member menu's
// activation. The menu's hold lasts until that activation has returned, so
// neither the hold's own release nor a poll re-renders the panel (deleting
// the ⋯ button and its menu) while the dialog is open.
void TestTeamsUi::rolePromptFromTheMemberMenuKeepsItsButton()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.resize(420, 1400); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    const auto testerMore = [&]() -> QToolButton * {
        for (auto *card : panel.findChildren<QFrame *>("teamCard"))
            if (card->property("teamMember") == "tester") return card->findChild<QToolButton *>("teamMemberMore");
        return nullptr;
    };
    QTRY_VERIFY(testerMore());
    QPointer<QToolButton> more = testerMore();
    QPointer<QMenu> menu = more->menu();
    auto changed = team("planning");
    changed["revision"] = changed["revision"].toInt() + 1;
    bool checked = false;
    std::function<void(int)> lookDialog = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *dialog = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!dialog) { if (tries > 0) lookDialog(tries - 1); return; }
            QPointer<QInputDialog> open = dialog;
            // A failed check closes the dialog, so the test ends instead of waiting on it.
            const auto close = qScopeGuard([open] { if (open) open->reject(); });
            // The menu's own release comes due inside the dialog's loop: no render.
            QTest::qWait(400);
            QVERIFY(panel.confirming());
            QVERIFY(more && menu);
            // A poll under the dialog: still no render.
            setTeam(changed);
            controller.refresh();
            QTRY_COMPARE(controller.team("team-1")->revision, changed["revision"].toInt());
            QTest::qWait(400);
            QVERIFY(more && menu);
            QVERIFY(more == testerMore());
            QVERIFY(open && open->isVisible());
            checked = true;
        });
    };
    std::function<void(int)> lookMenu = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *popup = qobject_cast<QMenu *>(QApplication::activePopupWidget());
            if (!popup) { if (tries > 0) lookMenu(tries - 1); return; }
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
    QTRY_VERIFY(checked);
    // Once everything has returned the panel catches up with the poll.
    QTRY_VERIFY(!panel.confirming());
    QTRY_VERIFY(testerMore() && testerMore() != more.data());
    QVERIFY(calls(script(), "plan").isEmpty());
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

// N10: the folder policy keeps auto while the path is typed; a finished non-Git
// path forces shared, and editing again returns to auto. Dialog and tab.
void TestTeamsUi::folderPolicyKeepsAutoWhileTyping()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;   // not a Git repository
    const QList<TeamNewDialog::Computer> computers{{QString(), "this computer", true}, {"msi-old", "msi-old", false}};
    TeamNewDialog dialog(&controller, &client, computers, {{QString(), "claude/checkout/main", "claude"}},
                         QDir::toNativeSeparators(folder.path()), "Checkout");
    dialog.show();
    auto *edit = dialog.findChild<QLineEdit *>("teamNewFolder");
    auto *policy = dialog.findChild<QComboBox *>("teamNewFolderPolicy");
    QVERIFY(edit && policy);
    // A finished non-Git folder is shared.
    QCOMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    // While the path is typed, auto stays.
    edit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/sub"));
    QCOMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    // Finishing the path forces shared.
    QMetaObject::invokeMethod(edit, "editingFinished");
    QCOMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    // Editing again returns to auto.
    edit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/sub2"));
    QCOMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    shot(&dialog, "new-team-folder-policy-typing");
    // The same in the workspace tab.
    const QList<TeamNewTab::Computer> tabComputers{{QString(), "this computer", true}, {"msi-old", "msi-old", false}};
    TeamNewTab tab(&controller, &client, tabComputers, TeamNewTab::Coordinator{}, QStringLiteral("Checkout"));
    tab.resize(700, 900); tab.show();
    auto *tabEdit = tab.findChild<QLineEdit *>("teamNewFolder");
    auto *tabPolicy = tab.findChild<QComboBox *>("teamNewFolderPolicy");
    QVERIFY(tabEdit && tabPolicy);
    tabEdit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/one"));
    QCOMPARE(tabPolicy->currentData().toString(), QStringLiteral("auto"));
    QMetaObject::invokeMethod(tabEdit, "editingFinished");
    QCOMPARE(tabPolicy->currentData().toString(), QStringLiteral("shared"));
    tabEdit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/two"));
    QCOMPARE(tabPolicy->currentData().toString(), QStringLiteral("auto"));
    shot(&tab, "new-team-tab-folder-policy-typing");
}

// N10: a typo path that is not a Git folder keeps auto with no worktrees; fixing
// it to a Git folder restores the worktrees, and the created members get them.
void TestTeamsUi::typoThenGitCreatesWorktrees()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir root;
    const QString gitFolder = root.filePath("repo");
    QVERIFY(QDir().mkpath(gitFolder + QStringLiteral("/.git")));
    const QString typo = root.filePath("rep");   // a typo: not a Git folder
    const QList<TeamNewDialog::Computer> computers{{QString(), "this computer", true}, {"msi-old", "msi-old", false}};
    TeamNewDialog dialog(&controller, &client, computers, {{QString(), "claude/checkout/main", "claude"}},
                         gitFolder, "Checkout");
    dialog.show();
    auto *edit = dialog.findChild<QLineEdit *>("teamNewFolder");
    auto *policy = dialog.findChild<QComboBox *>("teamNewFolderPolicy");
    auto *worktrees = dialog.findChild<QCheckBox *>("teamNewWorktrees");
    QVERIFY(edit && policy && worktrees);
    // It starts as a Git folder with worktrees; the typo finished forces shared
    // with no worktrees.
    QCOMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    QVERIFY(worktrees->isChecked());
    edit->setText(typo);
    QMetaObject::invokeMethod(edit, "editingFinished");
    QCOMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    QVERIFY(!worktrees->isChecked());
    // Fixing the path to the repository returns auto and the worktrees.
    edit->setText(gitFolder);
    QMetaObject::invokeMethod(edit, "editingFinished");
    QCOMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    QVERIFY(worktrees->isEnabled());
    QVERIFY(worktrees->isChecked());
    // Create: every member works in its own worktree. The fake catalog really has
    // the "feature" template with its roster, so its rows must exist.
    QVERIFY(chooseTemplate(&dialog, QStringLiteral("feature")));
    QTRY_VERIFY(dialog.findChildren<QComboBox *>("teamNewComputer").size() >= 2);   // the template's member rows
    dialog.findChild<QLineEdit *>("teamNewName")->setText(QStringLiteral("Checkout redesign"));
    auto *create = dialog.findChild<QPushButton *>("teamNewCreate");
    QTRY_VERIFY(create->isEnabled());
    create->click();
    QTRY_COMPARE(dialog.result(), int(QDialog::Accepted));
    const auto input = calls(script(), "create").last();
    QCOMPARE(input["settings"].toObject()["folder_policy"].toString(), QStringLiteral("auto"));
    QVERIFY2(!input["members"].toArray().isEmpty(), qPrintable(QString::fromUtf8(QJsonDocument(input).toJson(QJsonDocument::Compact))));
    for (const auto &member : input["members"].toArray())
        QCOMPARE(member.toObject()["folder"].toString(), QStringLiteral("worktree"));
    // The same forced-shared-then-auto path in the workspace tab.
    const QList<TeamNewTab::Computer> tabComputers{{QString(), "this computer", true}, {"msi-old", "msi-old", false}};
    TeamNewTab tab(&controller, &client, tabComputers, TeamNewTab::Coordinator{}, QStringLiteral("Checkout"));
    tab.resize(760, 1000); tab.show();
    auto *tabEdit = tab.findChild<QLineEdit *>("teamNewFolder");
    auto *tabPolicy = tab.findChild<QComboBox *>("teamNewFolderPolicy");
    auto *tabWorktrees = tab.findChild<QCheckBox *>("teamNewWorktrees");
    QVERIFY(tabEdit && tabPolicy && tabWorktrees);
    tabEdit->setText(typo);
    QMetaObject::invokeMethod(tabEdit, "editingFinished");
    QCOMPARE(tabPolicy->currentData().toString(), QStringLiteral("shared"));
    QVERIFY(!tabWorktrees->isChecked());
    tabEdit->setText(gitFolder);
    QMetaObject::invokeMethod(tabEdit, "editingFinished");
    QCOMPARE(tabPolicy->currentData().toString(), QStringLiteral("auto"));
    QVERIFY(tabWorktrees->isEnabled());
    QVERIFY(tabWorktrees->isChecked());
}

void TestTeamsUi::followUpsShowThePermissionAnswerAndPlanFixes()
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
    QTRY_VERIFY(([&] { controller.refresh(); return visibleLabel(&planPanel, "teamPlanCheck") != nullptr; })());
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
    QTRY_VERIFY(([&] { controller.refresh(); return visibleLabel(&lightPanel, "teamPlanCheck") != nullptr; })());
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

// N7 (#364, U18): the owner's attention item for a permission the lead left
// unanswered names the member, what it asks and its age, and Answer leads to
// the member's Allow / Deny (or, with no answer box, puts the command on the
// clipboard). Each item is one desktop notice, raised once, withdrawn when gone.
void TestTeamsUi::permissionAttentionNamesTheMemberAndAnswers()
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

void TestTeamsUi::leadLoadViewsShowInTheDesktop()
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
    shot(&panel, "lead-load-plan");
    panel.setTheme(false);
    lightShot(&panel, "lead-load-plan-light");
    panel.setTheme(true); darkPalette(); QTest::qWait(50);
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
    QVERIFY(workspace.findChild<QFrame *>("teamAttentionStrip"));
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

void TestTeamsUi::preflightWarningsShowInThePlanCard()
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
    shot(&panel, "preflight-warning-dark");
    panel.setTheme(false);
    lightShot(&panel, "preflight-warning-light");
    panel.setTheme(true); darkPalette();
}

void TestTeamsUi::permissionCardsAndGraphOrder()
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
    shot(&workspace, "widening-strip-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "widening-strip-light");
    workspace.setTheme(true); darkPalette();
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

void TestTeamsUi::phaseProgressShowsTwoBars()
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
    QTRY_VERIFY(workspace.findChildren<QFrame *>("teamPhaseScroll").size() > 2);
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
    // The Activity loads in the background: a loaded machine needs over 5 s.
    QTRY_VERIFY_WITH_TIMEOUT(lightText(), 10000);
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

void TestTeamsUi::settingsPageShowsTheSubagentSetting()
{
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString script = dir.filePath("fake-hgs");
    QVERIFY(FakeCli::install(script, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a[:2]==['subagents-confirm','get']:
    print(json.dumps({'confirm': os.environ.get('FAKE_CONFIRM','true')=='true'}))
elif a[:2]==['subagents-confirm','set']:
    body=json.load(sys.stdin)
    open(__file__+'.calls','a').write(json.dumps(body)+'\n')
    print(json.dumps({'confirm':body.get('confirm')}))
elif a[:2]==['session-upload-block','get']:
    print(json.dumps({'block': True}))
)PY"));
    qputenv("FAKE_CONFIRM", "false");
    SettingsPage page(FakeCli::program(script));
    auto *box = page.findChild<QCheckBox *>("workspaceConfirmSubagents");
    QVERIFY(box);
    QCOMPARE(box->isChecked(), false);
    box->click();
    QFile calls(script + ".calls");
    QTRY_VERIFY(calls.exists());
    QVERIFY(calls.open(QIODevice::ReadOnly));
    const auto sent = QJsonDocument::fromJson(calls.readAll().trimmed()).object();
    QCOMPARE(sent.value("confirm").toBool(), true);
    const auto id = sent.value("request_id").toString();
    QVERIFY2(!QUuid(id).isNull() && QUuid(id).toString(QUuid::WithoutBraces) == id, qPrintable(id));
    // The same page carries the DeepSeek upload block from #481, on by default.
    auto *upload = page.findChild<QCheckBox *>("workspaceSessionUploadBlock");
    QVERIFY(upload && upload->isChecked());
    // Shots: setting on/off, light/dark (only with ZERUS_TEAM_SHOTS).
    for (bool enabled : {true, false}) {
        qputenv("FAKE_CONFIRM", enabled ? "true" : "false");
        lightPalette();
        SettingsPage page(FakeCli::program(script));
        page.resize(1200, 900);
        if (auto *nav = page.findChild<QListWidget *>("settingsSections")) nav->setCurrentRow(1);
        QCoreApplication::processEvents();
        const QString suffix = enabled ? "on" : "off";
        shot(&page, "u19-settings-subagents-" + suffix);
        darkPalette();
        shot(&page, "u19-settings-subagents-" + suffix + "-dark");
        lightPalette();
    }
    qputenv("FAKE_CONFIRM", "false");
    qunsetenv("FAKE_CONFIRM");
}

void TestTeamsUi::aSessionRecordShowsTheUnconfirmableSubagentNote()
{
    const QJsonArray rows{ QJsonObject{{"name", "codex/x/dev"}, {"cmd", "codex"},
        {"subagent_confirm", "subagent confirmation not supported by codex"}} };
    const auto parsed = HgsClient::parseSessions(rows);
    QCOMPARE(parsed.size(), 1);
    QCOMPARE(parsed.first().subagentConfirm, QStringLiteral("subagent confirmation not supported by codex"));
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
    const QString path = QDir::toNativeSeparators(folder.path());
    const TeamNewTab::Coordinator coordinator{"claude/checkout/coord", "claude", "Opus", "work", path, "msi", "Ready", false};
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}, {"msi-old", "msi-old", false}}, coordinator, "Checkout");
    QString created; int designer = 0;
    tab.created = [&](const QString &team) { created = team; };
    tab.askDesigner = [&] { ++designer; };
    tab.resize(1400, 900); tab.show();
    const auto leadTab = [&](const QString &page) {
        for (auto *b : tab.findChild<QWidget *>("teamNewLeadTabs")->findChildren<QPushButton *>()) if (b->property("segment") == page) b->click();
    };
    const auto draftCalls = [&](const QString &action) {
        QList<QJsonObject> found;
        for (const auto &c : calls(script(), "draft")) if (c["action"].toString() == action) found << c;
        return found;
    };
    // F5: two columns, settings and the lead; opening the tab creates nothing.
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewColumns")->isVisible());
    QVERIFY(!tab.findChild<QWidget *>("teamNewNarrow")->isVisible());
    QVERIFY(tab.findChild<QWidget *>("teamNewLeadSetup")->isVisible());
    QVERIFY(!tab.findChild<QWidget *>("teamNewLeadChat")->isVisible());
    QVERIFY(tab.findChild<QRadioButton *>("teamNewLeadThis")->isChecked());
    QVERIFY(tab.findChild<QLabel *>("teamNewLeadThisDetails")->text().startsWith("claude/checkout/coord"));
    QVERIFY(tab.findChild<QLabel *>("teamNewEmptyMembers"));
    auto *create = tab.findChild<QPushButton *>("teamNewCreate");
    auto *start = tab.findChild<QPushButton *>("teamNewStartPlanning");
    QVERIFY(!create->isEnabled()); QCOMPARE(create->toolTip(), QStringLiteral("Start planning first"));
    QVERIFY(start->isEnabled());
    QVERIFY(!tab.findChild<QPushButton *>("teamNewDiscard")->isVisible());
    // F4: the designer keeps the templates; New team only opens it.
    tab.findChild<QPushButton *>("teamNewAskDesigner")->click(); QCOMPARE(designer, 1);
    shot(&tab, "f5-new-team-open");
    // F6: the compact list fills the rows from the template's roster; only the
    // chosen template gets the full card.
    QVERIFY(!tab.findChild<QFrame *>("teamNewTemplate"));
    QVERIFY(chooseTemplate(&tab, "feature"));
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);   // the table in a wide settings column
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewTemplate").size(), 1);
    QCOMPARE(tab.findChild<QFrame *>("teamNewTemplate")->property("template").toString(), QStringLiteral("feature"));
    // The template's lead protocol fills the folded addendum.
    auto *protocol = tab.findChild<QPlainTextEdit *>("teamNewProtocol");
    QVERIFY(!protocol->isVisible());
    QCOMPARE(protocol->toPlainText(), QStringLiteral("Plan in waves of three"));
    tab.findChild<QToolButton *>("teamNewProtocolToggle")->click();
    QVERIFY(protocol->isVisible());
    protocol->setPlainText("Plan in waves of two");
    // The goal is on the lead's Goal tab.
    leadTab("goal");
    auto *goal = tab.findChild<QPlainTextEdit *>("teamNewGoal");
    QTRY_VERIFY(goal->isVisible());
    goal->setPlainText("Redesign the checkout flow: address form and tests");
    QCOMPARE(tab.findChild<QLineEdit *>("teamNewName")->text(), QStringLiteral("Redesign the checkout flow"));
    QTest::qWait(100);
    QVERIFY(calls(script(), "draft").isEmpty());   // still nothing created
    QVERIFY(!create->isEnabled());
    // Start planning: the draft, with this session as its lead.
    leadTab("chat");
    start->click();
    QTRY_COMPARE(calls(script(), "draft").size(), 2);   // start, then the form's first update
    const auto started = calls(script(), "draft").at(0);
    QCOMPARE(started["action"].toString(), QStringLiteral("start"));
    QCOMPARE(started["folder"].toString(), path);
    QCOMPARE(started["template"].toString(), QStringLiteral("feature"));
    QCOMPARE(started["name"].toString(), QStringLiteral("Redesign the checkout flow"));
    QCOMPARE(started["lead"].toObject()["session"].toObject()["name"].toString(), QStringLiteral("claude/checkout/coord"));
    QCOMPARE(started["lead"].toObject()["prompt"].toString(), QStringLiteral("You lead."));
    QVERIFY(!started["request_id"].toString().isEmpty());
    const auto first = calls(script(), "draft").at(1);
    QCOMPARE(first["action"].toString(), QStringLiteral("update"));
    QCOMPARE(first["team"].toString(), QStringLiteral("draft-1"));
    QCOMPARE(first["revision"].toInt(), 1);
    const auto set = first["set"].toObject();
    QCOMPARE(set["goal"].toString(), QStringLiteral("Redesign the checkout flow: address form and tests"));
    QCOMPARE(set["addendum"].toString(), QStringLiteral("Plan in waves of two"));
    QCOMPARE(set["members"].toArray().size(), 2);
    QCOMPARE(set["members"].toArray()[0].toObject()["id"].toString(), QStringLiteral("backend"));
    QCOMPARE(set["members"].toArray()[1].toObject()["role"].toString(), QStringLiteral("Tester"));
    QCOMPARE(set["tasks"].toArray().size(), 3);   // the template plans
    QCOMPARE(set["tasks"].toArray()[0].toObject()["assignee"].toString(), QStringLiteral("lead"));
    QCOMPARE(set["settings"].toObject()["phase_approval"].toString(), QStringLiteral("auto"));
    // The lead's chat replaces the setup; the folder belongs to the draft now.
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewLeadChat")->isVisible());
    QVERIFY(!tab.findChild<QWidget *>("teamNewLeadSetup")->isVisible());
    QVERIFY(tab.findChild<QLineEdit *>("teamNewFolder")->isReadOnly());
    QCOMPARE(tab.draftId(), QStringLiteral("draft-1"));
    QVERIFY(tab.findChild<QPushButton *>("teamNewDiscard")->isVisible());
    // QA #347: a folder that is not a Git repository has one folder for every
    // member; the draft shows the policy as Shared, fixed, with the reason.
    {
        auto *policy = tab.findChild<QComboBox *>("teamNewFolderPolicy");
        QVERIFY(policy);
        QCOMPARE(policy->currentData().toString(), QStringLiteral("shared"));
        QVERIFY(!policy->isEnabled());
        QVERIFY2(policy->toolTip().contains(QStringLiteral("every member works in this folder")), qPrintable(policy->toolTip()));
        QVERIFY(tab.findChild<QLabel *>("teamNewGitNote")->isVisible());
    }
    // The lead edits the draft: the form follows and marks what it changed.
    setTeam(draftTeam(path));
    QTRY_COMPARE(tab.findChild<QPlainTextEdit *>("teamNewGoal")->toPlainText(), QStringLiteral("Redesign checkout: address form and tests"));
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    QCOMPARE(tab.findChildren<QComboBox *>("teamNewRole").at(1)->currentText(), QStringLiteral("Frontend"));
    QStringList marked;
    for (auto *mark : tab.findChildren<QToolButton *>("teamNewUnseen")) if (mark->isVisible()) marked << mark->property("field").toString();
    QCOMPARE(marked, QStringList{"members"});   // the goal's mark is on the Goal tab, not shown now
    QVERIFY(tab.findChild<QLabel *>("teamNewUnseenLegend")->isVisible());
    for (auto *b : tab.findChild<QWidget *>("teamNewLeadTabs")->findChildren<QPushButton *>())
        if (b->property("segment") == "goal") QCOMPARE(b->text(), QStringLiteral("Goal ●"));
    // The preflight reads ok with its warning.
    QTRY_VERIFY(tab.findChild<QLabel *>("teamNewPreflight")->text().contains("Preflight: ok"));
    QVERIFY(tab.findChild<QLabel *>("teamNewPreflight")->text().contains("1 warning"));
    QVERIFY(tab.findChild<QLabel *>("teamNewSummary")->text().contains("rev 3"));
    QVERIFY(calls(script(), "create").isEmpty());
    shot(&tab, "f5-new-team-draft");
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        // The lead's chat repaints its document after the theme: wait for it.
        tab.setDark(false);
        lightPalette();
        QTRY_VERIFY(looksLight(&tab));
        shot(&tab, "f5-new-team-draft-light");
        darkPalette();
        tab.setDark(true);
    }
    // The plan the lead drafted.
    leadTab("plan");
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewPlanTask").size(), 2);
    shot(&tab, "f5-new-team-plan");
    leadTab("chat");
    // Seen: the owner clears the members mark.
    QToolButton *membersMark = nullptr;
    for (auto *mark : tab.findChildren<QToolButton *>("teamNewUnseen")) if (mark->property("field") == "members" && mark->isVisible()) membersMark = mark;
    QVERIFY(membersMark);
    membersMark->click();
    QTRY_COMPARE(draftCalls("seen").size(), 1);
    setTeam(draftTeam(path, 3, {"goal"}));   // what hgs keeps after seen
    QCOMPARE(draftCalls("seen").first()["fields"].toArray(), QJsonArray{"members"});
    QTRY_VERIFY(!membersMark->isVisible());
    // The owner's edit goes out with the revision read, alone.
    auto *name = tab.findChild<QLineEdit *>("teamNewName");
    name->setText("Checkout v2"); emit name->textEdited(name->text());
    QTRY_COMPARE(draftCalls("update").size(), 2);
    const auto renamed = draftCalls("update").last();
    QCOMPARE(renamed["revision"].toInt(), 3);
    QCOMPARE(renamed["set"].toObject().keys(), QStringList{"name"});
    QCOMPARE(renamed["set"].toObject()["name"].toString(), QStringLiteral("Checkout v2"));
    // A preflight error blocks Create; the fixed plan creates the team.
    setTeam(draftTeam(path, 4, {}, QJsonArray{QJsonObject{{"kind", "assignee"}, {"task", 2}, {"text", "#2 Address form UI: no assignee"}}}));
    QTRY_VERIFY(tab.findChild<QLabel *>("teamNewPreflight")->text().contains("1 error"));
    QVERIFY(!create->isEnabled());
    QCOMPARE(create->toolTip(), QStringLiteral("Fix 1 preflight error"));
    setTeam(draftTeam(path, 5, {}));
    QTRY_VERIFY(create->isEnabled());
    // A narrow tab shows the two columns as tabs; the settings tab keeps the
    // table while it is wide enough, and cards below that.
    tab.resize(820, 900);
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewNarrow")->isVisible());
    QCOMPARE(tab.findChild<QTabWidget *>("teamNewNarrow")->count(), 2);
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    tab.resize(600, 900);
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberCard").size(), 2);
    shot(&tab, "f5-new-team-narrow");
    tab.findChild<QTabWidget *>("teamNewNarrow")->setCurrentIndex(1);
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewLeadChat")->isVisible());
    shot(&tab, "f5-new-team-narrow-lead");
    tab.resize(1400, 900);
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewColumns")->isVisible());
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewLeadChat")->isVisible());
    create->click();
    QTRY_COMPARE(created, QStringLiteral("draft-1"));
    QCOMPARE(draftCalls("create").size(), 1);
    QCOMPARE(draftCalls("create").first()["team"].toString(), QStringLiteral("draft-1"));
    QVERIFY(calls(script(), "create").isEmpty());   // the draft becomes the team; no second team
    QVERIFY(tab.draftId().isEmpty());
}

// F5: Discard draft asks first and whether to stop the lead; a reopened tab
// shows the draft again without changing it.
void TestTeamsUi::draftDiscardAsksAndCanStopTheLead()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    const QString path = QDir::toNativeSeparators(folder.path());
    setTeam(draftTeam(path));
    const TeamNewTab::Coordinator coordinator{"claude/checkout/coord", "claude", "Opus", "work", path, "msi", "Ready", false};
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}}, coordinator, "Checkout");
    QString discarded; bool answer = false, stop = false; int asked = 0;
    tab.discarded = [&](const QString &team) { discarded = team; };
    tab.confirmDiscard = [&](bool *stopLead) { ++asked; *stopLead = stop; return answer; };
    tab.resize(1300, 860); tab.show();
    tab.openDraft("draft-1");
    QTRY_COMPARE(tab.findChild<QLineEdit *>("teamNewName")->text(), QStringLiteral("Checkout redesign"));
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewLeadChat")->isVisible());
    QCOMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    QCOMPARE(tab.findChild<QPlainTextEdit *>("teamNewProtocol")->toPlainText(), QStringLiteral("Plan in waves of two"));
    QTest::qWait(300);
    QVERIFY(calls(script(), "draft").isEmpty());   // reading a draft changes nothing
    auto *discard = tab.findChild<QPushButton *>("teamNewDiscard");
    QVERIFY(discard->isVisible());
    // Cancelled: nothing is sent.
    discard->click();
    QCOMPARE(asked, 1);
    QTest::qWait(100);
    QVERIFY(calls(script(), "draft").isEmpty());
    // Confirmed, with the lead's session stopped too.
    answer = true; stop = true;
    discard->click();
    QTRY_COMPARE(discarded, QStringLiteral("draft-1"));
    const auto input = calls(script(), "draft").last();
    QCOMPARE(input["action"].toString(), QStringLiteral("discard"));
    QCOMPARE(input["team"].toString(), QStringLiteral("draft-1"));
    QVERIFY(input["stop_lead"].toBool());
    QVERIFY(tab.draftId().isEmpty());
    QTRY_VERIFY(tab.findChild<QWidget *>("teamNewLeadSetup")->isVisible());
    QVERIFY(!discard->isVisible());
}

// F5: an owner edit refused for a stale revision goes again once with the
// revision the refusal names; the owner's edit wins.
void TestTeamsUi::draftUpdateRetriesAStaleRevision()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    const QString path = QDir::toNativeSeparators(folder.path());
    setTeam(draftTeam(path));
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}},
                   TeamNewTab::Coordinator{"claude/checkout/coord", "claude", "Opus", "", path, "msi", "Ready", false}, "Checkout");
    tab.resize(1300, 860); tab.show();
    tab.openDraft("draft-1");
    QTRY_COMPARE(tab.findChild<QLineEdit *>("teamNewName")->text(), QStringLiteral("Checkout redesign"));
    QTRY_VERIFY(tab.findChild<QLabel *>("teamNewSummary")->text().contains("rev 3"));
    // The lead wrote twice since the form read revision 3.
    QVERIFY(FakeCli::replace(script() + ".draftrev", "5"));
    tab.findChild<QSpinBox *>("teamNewMaxMembers")->setValue(5);
    QTRY_COMPARE(calls(script(), "draft").size(), 2);
    QCOMPARE(calls(script(), "draft").at(0)["revision"].toInt(), 3);
    QCOMPARE(calls(script(), "draft").at(1)["revision"].toInt(), 5);
    const auto settings = calls(script(), "draft").at(1)["set"].toObject()["settings"].toObject();
    QCOMPARE(settings["max_members"].toInt(), 5);
    // The settings go back whole: the fields the form does not own stay.
    QCOMPARE(settings["report_timeout_seconds"].toInt(), 120);
    QTRY_VERIFY(tab.pendingFields().isEmpty());
    QTRY_VERIFY(tab.findChild<QLabel *>("teamNewSummary")->text().contains("rev 6"));
}

// F6: many templates in a compact list with a search field and favourites; the
// big card is not part of the list.
void TestTeamsUi::templatesAreCompactWithSearchAndFavourites()
{
    QSettings().remove(QStringLiteral("teams/templateFavourites"));
    const QJsonArray featureRoster{QJsonObject{{"role", "backend"}, {"count", 1}}, QJsonObject{{"role", "tester"}, {"count", 1}}};
    const QJsonArray bugfixRoster{QJsonObject{{"role", "backend"}, {"count", 1}}};
    const QJsonArray devRoster{QJsonObject{{"role", "backend"}, {"count", 5}}};
    auto catalogObject = catalog()["catalog"].toObject();
    catalogObject["templates"] = QJsonArray{
        QJsonObject{{"id", "feature"}, {"name", "Feature delivery"}, {"description", "Full feature, backend and QA"}, {"roster", featureRoster}},
        QJsonObject{{"id", "bugfix"}, {"name", "Bugfix"}, {"description", "One bug, one fix"}, {"roster", bugfixRoster}},
        QJsonObject{{"id", "devteam"}, {"name", "Dev Team"}, {"description", "Five people"}, {"roster", devRoster}}};
    const auto parsed = Teams::parseCatalog(QJsonObject{{"catalog", catalogObject}});
    QCOMPARE(parsed.templates.size(), 3);
    TeamTemplateList list;
    QStringList picked;
    list.selected = [&](const QString &id) { picked << id; };
    list.setCatalog(parsed); list.resize(380, 320); list.show();
    // Filling the list picks nothing for the form.
    QVERIFY(picked.isEmpty());
    // Every template by name, with "No template" last; one line each, its size beside the name.
    QCOMPARE(list.visibleIds(), (QStringList{"bugfix", "devteam", "feature", "-"}));
    bool sized = false;
    for (auto *row : list.findChildren<QWidget *>("teamTemplateRow")) if (row->property("template") == "devteam")
        for (auto *l : row->findChildren<QLabel *>()) sized = sized || l->text() == QStringLiteral("5 members");
    QVERIFY(sized);
    auto *search = list.findChild<QLineEdit *>("teamTemplateSearch");
    search->setText(QStringLiteral("bug"));
    QCOMPARE(list.visibleIds(), (QStringList{"bugfix", "-"}));
    // The search also reads the roles a template seats, by id and by name.
    search->setText(QStringLiteral("tester"));
    QCOMPARE(list.visibleIds(), (QStringList{"feature", "-"}));
    search->setText(QStringLiteral("Backend"));
    QCOMPARE(list.visibleIds(), (QStringList{"bugfix", "devteam", "feature", "-"}));
    search->clear();
    // A favourite comes first and keeps its star, in a new list too.
    const auto starOf = [&](const QString &id) {
        QToolButton *found = nullptr;
        for (auto *button : list.findChildren<QToolButton *>("teamTemplateFavourite")) if (button->property("template") == id && !button->objectName().isEmpty()) found = button;
        return found;
    };
    QVERIFY(starOf("devteam")); QCOMPARE(starOf("devteam")->text(), QStringLiteral("☆"));
    starOf("devteam")->click();
    QTRY_COMPARE(list.visibleIds().first(), QStringLiteral("devteam"));
    QVERIFY(TeamTemplateList::isFavourite("devteam"));
    TeamTemplateList again; again.setCatalog(parsed);
    QCOMPARE(again.visibleIds().first(), QStringLiteral("devteam"));
    QTRY_VERIFY(starOf("devteam") && starOf("devteam")->text() == QStringLiteral("★"));
    QVERIFY(picked.isEmpty());   // a star is not a choice
    shot(&list, "f6-templates-compact");
    // The favourites-only filter keeps just them and the empty choice.
    auto *only = list.findChild<QToolButton *>("teamTemplateFavouritesOnly");
    only->click();
    QCOMPARE(list.visibleIds(), (QStringList{"devteam", "-"}));
    only->click();
    // Choosing a row tells the form and shows the template's roles and use.
    auto *rows = list.findChild<QListWidget *>("teamTemplateList");
    rows->setCurrentRow(list.visibleIds().indexOf(QStringLiteral("feature")));
    QCOMPARE(picked, QStringList{"feature"});
    QCOMPARE(list.current(), QStringLiteral("feature"));
    QCOMPARE(list.findChild<QLabel *>("teamTemplateDetail")->text(), QStringLiteral("Feature delivery — Backend, Tester\nFull feature, backend and QA"));
    // A rebuild keeps the choice without telling the form again.
    search->setText(QStringLiteral("e"));
    QCOMPARE(picked.size(), 1);
    QCOMPARE(rows->currentItem()->data(Qt::UserRole).toString(), QStringLiteral("feature"));
    search->clear();
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        list.setDark(false);
        lightShot(&list, "f6-templates-compact-light");
        darkPalette();
        list.setDark(true);
    }
    QSettings().remove(QStringLiteral("teams/templateFavourites"));
}

// F6 in the modal dialog: the list replaces the combo and applies a template
// only when chosen; building and closing the dialog apply nothing.
void TestTeamsUi::dialogTemplateListAppliesOnlyWhenChosen()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    {
        TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}}, {}, QDir::toNativeSeparators(folder.path()), "Checkout");
        dialog.show();
        QVERIFY(dialog.findChild<QListWidget *>("teamTemplateList"));
        QCOMPARE(dialog.templateId(), QString());
        QCOMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 0);
        QVERIFY(chooseTemplate(&dialog, "feature"));
        QTRY_COMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
        QVERIFY(chooseTemplate(&dialog, "-"));
        QTRY_COMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 0);
        dialog.selectTemplate("feature");
        QTRY_COMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
        shot(&dialog, "f6-new-team-dialog");
    }   // destroyed with its rows: the list calls nothing back
    QVERIFY(calls(script(), "create").isEmpty());
}

// F4: the Team designer keeps its fixed name and colour and is never offered
// as a lead; nothing else is filed into it.
void TestTeamsUi::designerSessionKeepsItsNameAndLeadsNoTeam()
{
    QVERIFY(isDesignerSession(QString(), "claude/team-designer/team-designer"));
    QVERIFY(!isDesignerSession("msi-old", "claude/team-designer/team-designer"));
    QVERIFY(!isDesignerSession(QString(), "claude/checkout/main"));
    QCOMPARE(designerTitle(), QStringLiteral("Team designer"));
    QCOMPARE(QColor(designerColor()), QColor("#a46ee0"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}},
                         {{QString(), "claude/team-designer/team-designer", "claude"}, {QString(), "claude/checkout/main", "claude"}},
                         QDir::toNativeSeparators(folder.path()), "Checkout");
    auto *existing = dialog.findChild<QComboBox *>("teamNewExistingLead");
    QCOMPARE(existing->count(), 1);
    QCOMPARE(existing->itemData(0).toString(), QStringLiteral("claude/checkout/main"));
    dialog.preselectExistingLead(QString(), "claude/team-designer/team-designer");
    QVERIFY(!dialog.findChild<QCheckBox *>("teamNewUseExisting")->isChecked());
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

void TestTeamsUi::subLeadsAreATreeInMembersAndTheInspector()
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
    shot(&panel, "ux-10-inspector-tree-dark");
    workspace.setTheme(false); panel.setTheme(false);
    lightShot(&workspace, "ux-10-members-tree-light");
    lightShot(&panel, "ux-10-inspector-tree-light");
}

// Spec 5.4: the Wide graph's geometry and routes - nodes 196x54, columns 236
// apart and centered, rows 74 apart, orthogonal edges that never cross a node
// of an intermediate column, 6 px lanes, back edges under every node.
void TestTeamsUi::graphRoutesEdgesAroundNodes()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) { TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend"; return n; };
    // Layer 0: #1; layer 1: #2 #3 #4; layer 2: #5 #6; layer 3: #7.
    view.nodes = {node(1, 0), node(2, 1), node(3, 1), node(4, 1), node(5, 2), node(6, 2), node(7, 3)};
    view.edges[2] = {1}; view.edges[3] = {1}; view.edges[4] = {1};
    view.edges[5] = {2, 3}; view.edges[6] = {4};
    view.edges[7] = {1, 2, 5, 6};   // #1 -> #7 and #2 -> #7 pass the columns between
    view.edges[3].append(7);        // a back edge (a cycle)
    view.cycle = {3, 7};
    view.area = QSize(1400, 700);
    view.layout();
    const auto r = [&](int id) { return view.nodeRect(id); };
    QCOMPARE(r(1).size(), QSizeF(196, 54));
    QCOMPARE(r(2).left() - r(1).left(), 236.0);
    QCOMPARE(r(3).top() - r(2).top(), 74.0);
    // A shorter column is centered on the tallest.
    QCOMPARE(r(1).center().y(), r(3).center().y());
    QCOMPARE(r(5).center().y(), (r(2).center().y() + r(3).center().y()) / 2);
    QMap<int, int> layerOf; for (const auto &n : view.nodes) layerOf.insert(n.id, n.layer);
    double lowest = 0; for (const auto &n : view.nodes) lowest = qMax(lowest, r(n.id).bottom());
    QList<double> gapLanes;
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        const int task = it.key();
        QVERIFY2(view.routed(dep, task), qPrintable(QStringLiteral("%1->%2").arg(dep).arg(task)));
        const auto route = view.route(dep, task).points;
        // It leaves the dependency's right side and ends at the arrow before the task.
        QCOMPARE(route.first(), QPointF(r(dep).right(), r(dep).center().y()));
        QCOMPARE(route.last(), QPointF(r(task).left() - 2, r(task).center().y()));
        for (int i = 1; i < route.size(); ++i) {
            const QPointF a = route.at(i - 1), b = route.at(i);
            QVERIFY2(qFuzzyCompare(a.x() + 1, b.x() + 1) || qFuzzyCompare(a.y() + 1, b.y() + 1), "every leg is horizontal or vertical");
            // No leg crosses a node of a column between the two ends.
            const QRectF leg = QRectF(a, b).normalized().adjusted(-.01, -.01, .01, .01);
            for (const auto &n : view.nodes) {
                const int l = n.layer;
                const bool between = layerOf[task] > layerOf[dep] ? (l > layerOf[dep] && l < layerOf[task]) : false;
                if (between) QVERIFY2(!leg.intersects(r(n.id).adjusted(1, 1, -1, -1)),
                                      qPrintable(QStringLiteral("edge #%1->#%2 crosses #%3").arg(dep).arg(task).arg(n.id)));
            }
        }
        if (layerOf[task] <= layerOf[dep]) {
            // A back edge runs under every node.
            double deepest = 0; for (const QPointF &point : route) deepest = qMax(deepest, point.y());
            QVERIFY(deepest > lowest);
        }
        if (dep == 1 && layerOf[task] == 1) gapLanes << route.at(1).x();
    }
    // The three edges from #1 through one gap spread over lanes 6 px apart.
    std::sort(gapLanes.begin(), gapLanes.end());
    QCOMPARE(gapLanes.size(), 3);
    QCOMPARE(gapLanes.at(1) - gapLanes.at(0), 6.0);
    QCOMPARE(gapLanes.at(2) - gapLanes.at(1), 6.0);
}

// Spec 5.4: in planning a click on a node's handle starts a link and a click
// on the dependent task makes it; a selected edge goes with its × or Delete.
// #465 N19: a 200-task plan (the plan limit) in one Wide graph, with 180 long
// edges, routes in well under a second per layout, as a resize lays it out
// again; no long edge crosses a node of the columns it passes.
void TestTeamsUi::graphRoutingStaysFastForTheLargestPlan()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    for (int id = 1; id <= 200; ++id) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = (id - 1) / 20; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "pending"; n.agent = "codex";
        view.nodes.append(n);
        // Two layers back, and three layers back for every other task.
        if (n.layer >= 2) view.edges[id].append(id - 40 + (id % 7 == 0 ? 3 : 0));
        if (n.layer >= 3 && id % 2) view.edges[id].append(id - 60);
    }
    int long_ = 0;
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) long_ += it.value().size();
    QVERIFY(long_ >= 80);
    view.area = QSize(1200, 800);
    QElapsedTimer timer; timer.start();
    for (int i = 0; i < 5; ++i) { view.zoom = 1 - i * .05; view.layout(); }
    const qint64 elapsed = timer.elapsed();
    QVERIFY2(elapsed < 2500, qPrintable(QStringLiteral("5 layouts took %1 ms").arg(elapsed)));
    const auto layerOf = [&](int id) { return (id - 1) / 20; };
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        const int task = it.key();
        QVERIFY(view.routed(dep, task));
        const auto route = view.route(dep, task).points;
        for (int other = 1; other <= 200; ++other) {
            if (layerOf(other) <= layerOf(dep) || layerOf(other) >= layerOf(task)) continue;
            const QRectF box = view.nodeRect(other).adjusted(1, 1, -1, -1);
            for (int i = 1; i < route.size(); ++i)
                QVERIFY2(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(box),
                         qPrintable(QStringLiteral("#%1 -> #%2 crosses #%3").arg(dep).arg(task).arg(other)));
        }
    }
}

void TestTeamsUi::graphEdgesAreEditedByHandleAndSelection()
{
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "pending"; n.agent = "codex";
        return n;
    };
    view.nodes = {node(1, 0), node(2, 1), node(3, 1)};
    view.edges[2] = {1};
    view.editable = true;
    QList<QPair<int, int>> linked, unlinked;
    view.linked = [&](int dep, int task) { linked << qMakePair(dep, task); };
    view.unlinked = [&](int dep, int task) { unlinked << qMakePair(dep, task); };
    view.area = QSize(900, 400);
    view.resize(900, 400); view.show();
    view.layout();
    QVERIFY(QTest::qWaitForWindowExposed(&view));
    const QRectF one = view.nodeRect(1), three = view.nodeRect(3);
    QTest::mouseMove(&view, one.center().toPoint());
    const QPoint port(int(one.right() - 4), int(one.center().y()));
    QTest::mouseClick(&view, Qt::LeftButton, {}, port);
    QCOMPARE(view.connecting(), 1);
    QTest::mouseMove(&view, three.center().toPoint());
    shot(&view, "ux-5-graph-connect-dark");
    QTest::mouseClick(&view, Qt::LeftButton, {}, three.center().toPoint());
    QCOMPARE(linked, (QList<QPair<int, int>>{{1, 3}}));
    QCOMPARE(view.connecting(), 0);
    // Escape leaves the connect mode without a link.
    QTest::mouseClick(&view, Qt::LeftButton, {}, port);
    QCOMPARE(view.connecting(), 1);
    QTest::keyClick(&view, Qt::Key_Escape);
    QCOMPARE(view.connecting(), 0);
    // Click the edge, then Delete.
    const auto route = view.route(1, 2).points;
    const QPointF onLeg = (route.at(0) + route.at(1)) / 2;
    QTest::mouseClick(&view, Qt::LeftButton, {}, onLeg.toPoint());
    QCOMPARE(view.selectedEdge(), qMakePair(1, 2));
    QTest::keyClick(&view, Qt::Key_Delete);
    QCOMPARE(unlinked, (QList<QPair<int, int>>{{1, 2}}));
    // Outside planning only the tasks of a draft take edits.
    view.editable = false;
    view.editableTasks = {3};
    QVERIFY(!view.canEdit(2) && view.canEdit(3));

    // #461: in a running team the draft phase's scroll takes the same handle
    // and selection edits through edit-draft, never `plan` (which the server
    // takes only in Planning); the running phase's scroll takes none.
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    TeamUi::TaskView *draft = nullptr;
    QTRY_VERIFY(([&] { draft = phaseGraphOf(&workspace, 3); return draft && draft->nodeRect(8).width() > 0 && draft->isVisible(); })());
    QCOMPARE(draft->property("phase").toInt(), 3);
    auto *build = phaseGraphOf(&workspace, 2); QVERIFY(build);
    QVERIFY(!build->editable && !build->canEdit(5) && !build->linked);
    QVERIFY(!draft->editable && draft->canEdit(7) && draft->canEdit(8) && draft->canEdit(9));
    const auto at = [&](const QPointF &point) { return QPoint(int(std::round(point.x() * draft->zoom)), int(std::round(point.y() * draft->zoom))); };
    // #465 N18: a poll rebuilds every view; the owner's half-made link and
    // selected edge come back in the new one.
    int revision = 21;
    const auto poll = [&] {
        QPointer<TeamUi::TaskView> old = draft;
        auto next = graphTeam(); next["revision"] = ++revision;
        setTeam(next);
        controller.refresh();
        QTRY_VERIFY(!old && (draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(8).width() > 0);
    };
    // #9's handle, then a poll, then #8: #8 gains #9.
    const QRectF nine = draft->nodeRect(9);
    QTest::mouseMove(draft, at(nine.center()));
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(9).width() > 0);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(QPointF(nine.right() - 4, nine.center().y())));
    QCOMPARE(draft->connecting(), 9);
    poll();
    QCOMPARE(draft->connecting(), 9);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(draft->nodeRect(8).center()));
    QCOMPARE(draft->connecting(), 0);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 1);
    auto input = calls(script(), "edit-draft").first();
    QCOMPARE(input["phase"].toInt(), 3);
    auto update = input["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 8);
    QCOMPARE(update["deps"].toArray().last().toObject()["task"].toInt(), 9);
    // The edge #8 -> #7 selected, then Delete: #7 keeps only #9.
    QTRY_VERIFY(([&] { draft = phaseGraphOf(&workspace, 3); return draft && draft->routed(8, 7); })());
    const auto legs = draft->route(8, 7).points;
    QTest::mouseClick(draft, Qt::LeftButton, {}, at((legs.at(0) + legs.at(1)) / 2));
    QCOMPARE(draft->selectedEdge(), qMakePair(8, 7));
    poll();
    QCOMPARE(draft->selectedEdge(), qMakePair(8, 7));
    QTest::keyClick(draft, Qt::Key_Delete);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 2);
    update = calls(script(), "edit-draft").last()["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 7);
    QCOMPARE(update["deps"].toArray().size(), 1);
    QCOMPARE(update["deps"].toArray().first().toObject()["task"].toInt(), 9);
    QVERIFY(calls(script(), "plan").isEmpty());
    // Escape ends a link for good: the next view starts clear. The reply to
    // edit-draft may rebuild the views, so the click goes to the one shown.
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->nodeRect(9).width() > 0);
    QTest::mouseClick(draft, Qt::LeftButton, {}, at(QPointF(draft->nodeRect(9).right() - 4, draft->nodeRect(9).center().y())));
    QTRY_VERIFY((draft = phaseGraphOf(&workspace, 3)) && draft->connecting() == 9);
    QTest::keyClick(draft, Qt::Key_Escape);
    poll();
    QCOMPARE(draft->connecting(), 0);
    QCOMPARE(draft->selectedEdge(), qMakePair(0, 0));
    QCOMPARE(calls(script(), "edit-draft").size(), 2);
}

void TestTeamsUi::graphNodesInTwoLinesWithDraftEdits()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    const LiveLookup live = [](const Member &m) { Live l; l.working = m.id == "frontend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // #461: a team with phases draws each phase in its own scroll; each one
    // draws the spec 5.4 nodes.
    TeamUi::TaskView *graph = nullptr, *draft = nullptr;
    QTRY_VERIFY(([&] { graph = phaseGraphOf(&workspace, 2); draft = phaseGraphOf(&workspace, 3); return graph && draft && graph->nodeRect(6).width() > 0; })());
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceGraph"));
    QCOMPARE(graph->property("phase").toInt(), 2);
    // #465 (spec 1.2): the phase scrolls sideways under the thin 8 px bar.
    auto *sideways = qobject_cast<QScrollArea *>(phaseScrollOf(&workspace, 2)->findChild<QWidget *>("teamPhaseGraphArea"));
    QVERIFY(sideways);
    QTRY_VERIFY(sideways->horizontalScrollBar()->isVisible());
    QCOMPARE(sideways->horizontalScrollBar()->sizeHint().height(), 8);
    const auto nodeIn = [](TeamUi::TaskView *view, int id) { for (const auto &n : view->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    const auto nodeOf = [&](int id) { return nodeIn(graph, id); };
    // Only the dependencies from other phases show on the node; the edge
    // from #1 (phase 1) is not drawn inside phase 2.
    QCOMPARE(nodeOf(2).ext, QStringLiteral("↤#1"));
    QVERIFY(nodeOf(5).ext.isEmpty());
    QVERIFY(nodeOf(2).deps.isEmpty());
    QVERIFY(!graph->routed(1, 2));
    QVERIFY(graph->routed(3, 6) && graph->routed(2, 5));
    // The second line: the problem as Board and Layers name it, never a
    // status word; the full status in the tooltip.
    QCOMPARE(nodeOf(2).problemText, QStringLiteral("Waiting for your answer"));
    QVERIFY(nodeOf(2).problem);
    QVERIFY(nodeOf(5).problemText.isEmpty());
    QVERIFY2(nodeOf(2).tooltip.startsWith(QStringLiteral("#2 Saved cards API — ")), qPrintable(nodeOf(2).tooltip));
    QVERIFY(nodeOf(2).tooltip.contains(QStringLiteral(" · after #1 (earlier phase) · Backend · deepseek-v4-pro")));
    QVERIFY(nodeOf(3).working);
    QVERIFY(!nodeOf(2).working);
    // The draft phase's nodes too: its other-phase dependencies and tooltip.
    QCOMPARE(nodeIn(draft, 8).ext, QStringLiteral("↤#2"));
    QCOMPARE(nodeIn(draft, 9).ext, QStringLiteral("↤#6"));
    QVERIFY(nodeIn(draft, 7).ext.isEmpty());
    QVERIFY(nodeIn(draft, 8).tooltip.contains(QStringLiteral(" · after #2 (earlier phase) · Backend · deepseek-v4-pro")));
    QVERIFY(draft->routed(8, 7) && draft->routed(9, 7));
    // The long edge #3 -> #6 passes #5's column without crossing it.
    const auto route = graph->route(3, 6).points;
    for (int i = 1; i < route.size(); ++i)
        QVERIFY(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(graph->nodeRect(5).adjusted(1, 1, -1, -1)));
    // The draft phase's tasks take edge edits through edit-draft; the running
    // phase's tasks take none.
    QVERIFY(draft->canEdit(7) && !graph->canEdit(5));
    QVERIFY(draft->linked && !graph->linked);
    draft->linked(9, 8);
    QTRY_COMPARE(calls(script(), "edit-draft").size(), 1);
    const auto input = calls(script(), "edit-draft").first();
    QCOMPARE(input["phase"].toInt(), 3);
    const auto update = input["tasks_update"].toArray().first().toObject();
    QCOMPARE(update["id"].toInt(), 8);
    QCOMPARE(update["deps"].toArray().size(), 2);
    QCOMPARE(update["deps"].toArray().last().toObject()["task"].toInt(), 9);
    QVERIFY(calls(script(), "plan").isEmpty());
    shot(&workspace, "ux-5-graph-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "ux-5-graph-light");
}

// #380 attempt 2: the member's "…" menu and the swap dialog run their own
// event loops. A poll that changes the team arrives under each: the menu, the
// button that opened it and the open dialog stay alive, and the views catch
// up once the menu has closed.
void TestTeamsUi::aPollUnderTheMemberMenuKeepsItsButtonAndTheSwapDialog()
{
    setTeam(subLeadTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.resize(420, 900); panel.show();
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto visibleMore = [&](const QString &member) -> QToolButton * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == member && card->isVisibleTo(&workspace)) return card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
        return nullptr;
    };
    QTRY_VERIFY(controller.watched() && visibleMore("backend"));
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
            if (!menu) { if (tries > 0) look(tries - 1); return; }
            open = menu;
            QVERIFY(panel.confirming());
            controller.refresh();
            QTest::qWait(600);   // the poll lands and a render is asked for
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
    QTRY_VERIFY(checked);
    QPointer<QDialog> dialog;
    QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    // The views catch up once the menu has closed: the column is rebuilt
    // with the new team, so the old button is not the one shown any more.
    QTRY_VERIFY(!panel.confirming());
    QTRY_VERIFY(visibleMore("backend") && visibleMore("backend") != more.data());
    // A poll while the swap dialog is open leaves it open.
    changed["revision"] = 14;
    setTeam(changed);
    controller.refresh();
    QTRY_COMPARE(controller.team("team-1")->revision, 14);
    QTest::qWait(300);
    QVERIFY(dialog && dialog->isVisible());
    dialog->close();
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

// G9: the workspace graph stacks phase bands top to bottom; each band's header
// carries the phase status, progress, health and a draft's decisions (spec
// 10.2). Shots: every status, collapsed and expanded, light and dark.
void TestTeamsUi::phaseBandsStackWithStatusDecisionAndHealth()
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
    // All expanded.
    for (int n = 1; n <= 2; ++n) {
        QTRY_VERIFY(phaseScrollOf(&workspace, n) && !phaseOpen(&workspace, n));
        togglePhase(&workspace, n);
        QTRY_VERIFY(phaseOpen(&workspace, n));
    }
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
    // All collapsed: one compact header row per scroll.
    workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")->click();
    QTRY_VERIFY(([&] { for (int n = 1; n <= 5; ++n) if (!phaseScrollOf(&workspace, n) || phaseOpen(&workspace, n)) return false; return true; })());
    for (int n = 1; n <= 5; ++n) QVERIFY(phaseScrollOf(&workspace, n)->height() <= 46);
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

void TestTeamsUi::reportPreviewsAndFindings()
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
    QVERIFY2(card.findChild<QLabel *>("teamReportEvidenceImage"), "the image evidence previews");
    QVERIFY2(card.findChild<QTextBrowser *>("teamReportEvidenceHtml"), "the HTML evidence previews");
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

void TestTeamsUi::hierarchyInTeamViewsAndMemberCards()
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
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
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
    shot(&workspace, "hierarchy-team-view");
    // The same hierarchy in the light theme.
    lightPalette();
    workspace.setTheme(false);
    card.setTask(parsed, 2, live, false);
    QTest::qWait(50);
    lightShot(&workspace, "hierarchy-team-view-light");
    lightShot(&card, "hierarchy-subteam-light");
}


// F7: a Done team starts the next goal with the same people; the new team
// opens, and the old one links to it.
void TestTeamsUi::nextGoalFromADoneTeam()
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
    shot(&workspace, "f7-next-goal-links");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "f7-next-goal-links-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
}

// F8: the lead's open proposals show with Accept and Reject; remove and swap
// ask first, a cancel sends nothing, and a decision goes out once.
void TestTeamsUi::proposalsAreAcceptedOrRejectedOnce()
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
    QTRY_VERIFY(workspace.findChild<QFrame *>("teamWorkspaceProposals"));
    const auto rowOf = [&](const QString &id) -> QWidget * {
        for (auto *row : workspace.findChildren<QWidget *>("teamProposal")) if (row->property("proposal") == id) return row;
        return nullptr;
    };
    QTRY_VERIFY(rowOf("p1") && rowOf("p2") && rowOf("p3"));
    QVERIFY(!rowOf("p4"));   // decided ones are gone
    QCOMPARE(rowOf("p1")->findChild<QLabel *>("teamProposalText")->text(), QStringLiteral("Add Designer (claude \u00b7 sonnet)"));
    QCOMPARE(rowOf("p2")->findChild<QLabel *>("teamProposalText")->text(), QStringLiteral("Swap Backend to codex \u00b7 gpt-5.5"));
    QCOMPARE(rowOf("p3")->findChild<QLabel *>("teamProposalText")->text(), QStringLiteral("Remove Tester"));
    shot(&workspace, "f8-proposals");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "f8-proposals-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
    workspace.render();
    // Reject sends at once.
    QTRY_VERIFY(rowOf("p1"));
    rowOf("p1")->findChild<QPushButton *>("teamProposalReject")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 1);
    QCOMPARE(calls(script(), "decide-proposal").first()["proposal"].toString(), QStringLiteral("p1"));
    QVERIFY(!calls(script(), "decide-proposal").first()["accept"].toBool());
    // A swap asks first: Cancel sends nothing; the confirmation sends once.
    QTRY_VERIFY(rowOf("p2"));
    armConfirm(&panel, true);
    rowOf("p2")->findChild<QPushButton *>("teamProposalAccept")->click();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "decide-proposal").size(), 1);
    QTRY_VERIFY(rowOf("p2") && rowOf("p2")->findChild<QPushButton *>("teamProposalAccept")->isEnabled());
    armConfirm(&panel, false);
    rowOf("p2")->findChild<QPushButton *>("teamProposalAccept")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 2);
    QCOMPARE(calls(script(), "decide-proposal").last()["proposal"].toString(), QStringLiteral("p2"));
    QVERIFY(calls(script(), "decide-proposal").last()["accept"].toBool());
    // Until the store has taken it, the row's buttons stay off.
    QTRY_VERIFY(rowOf("p2") && !rowOf("p2")->findChild<QPushButton *>("teamProposalAccept")->isEnabled());
    // A remove asks too.
    armConfirm(&panel, false);
    rowOf("p3")->findChild<QPushButton *>("teamProposalAccept")->click();
    QTRY_COMPARE(calls(script(), "decide-proposal").size(), 3);
    QCOMPARE(calls(script(), "decide-proposal").last()["proposal"].toString(), QStringLiteral("p3"));
}

// H6: Swap from the member menu, a new session or an existing one, confirmed;
// the old session stays listed for the owner to stop.
// #465 (the lead's addition): the workspace draws a team from the list before
// its `team status` read lands. Until then the member "…" opened nothing, so
// the swap dialog never came on a loaded machine. Every handler now looks the
// team up as the render does.
void TestTeamsUi::memberMenuWorksBeforeTheStatusReadLands()
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

void TestTeamsUi::swapMemberAsksFirstAndListsTheOldSession()
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

// H1 and H3: a member at a usage limit shows its reset time, the team toggles
// resume after limits (the settings go back whole); members on one DeepSeek host
// say so with the host in the tooltip.
void TestTeamsUi::limitedAndSharedHostMembersAreMarked()
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
    shot(&workspace, "h1-h3-member-marks");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "h1-h3-member-marks-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
    workspace.render();
    // Resume after limits: the team setting, sent with the other settings.
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits"));
    auto *toggle = workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits");
    QVERIFY(toggle->isCheckable() && !toggle->isChecked());
    toggle->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 1);
    const auto settings = calls(script(), "settings").first()["settings"].toObject();
    QVERIFY(settings["resume_after_limits"].toBool());
    QCOMPARE(settings["approval"].toString(), QStringLiteral("review"));
    QCOMPARE(settings["report_timeout_seconds"].toInt(), 120);
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

// U25 part 1: the Compact skin carries every prototype token for both themes,
// and any skin answers a token it does not name with the prototype's value.
void TestTeamsUi::skinTokensMatchThePrototype()
{
    const auto skins = WorkspaceSkin::builtins();
    QCOMPARE(skins[1].id, QString("whitewh"));
    for (const bool dark : {true, false})
        for (const auto &[name, d, l] : WorkspaceSkin::designTokens())
            QVERIFY2(WorkspaceSkin::color(skins[1], dark, QString::fromLatin1(name)) == QColor(QString::fromLatin1(dark ? d : l)),
                     qPrintable(QStringLiteral("%1 (%2)").arg(QString::fromLatin1(name), dark ? "dark" : "light")));
    QCOMPARE(WorkspaceSkin::color(skins[1], true, "cardBorder"), QColor("#232932"));
    QCOMPARE(WorkspaceSkin::color(skins[1], false, "text3"), QColor("#415060"));
    // The plain Zerus skin keeps its own workspace colors and takes the rest.
    QCOMPARE(WorkspaceSkin::color(skins[0], true, "bg"), QColor(skins[0].dark.background));
    QCOMPARE(WorkspaceSkin::color(skins[0], true, "label"), QColor("#8f97a2"));
    // A skin's view colors override a token, roles included.
    auto own = skins[1];
    own.darkViewColors.insert("roleCoder", "#123456");
    QCOMPARE(WorkspaceSkin::color(own, true, "roleCoder"), QColor("#123456"));
    // The team surfaces read the tokens of the selected skin.
    QSettings().setValue("workspace/skin", "whitewh");
    const auto c = TeamUi::colorsFor(false);
    QCOMPARE(c.text3, QColor("#415060")); QCOMPARE(c.accentMuted, QColor("#4f8f78")); QCOMPARE(c.label, QColor("#647181"));
    QCOMPARE(c.cardBorder, QColor("#e1e6eb")); QCOMPARE(c.segOn, QColor("#e0f0e9")); QCOMPARE(c.tile, QColor("#eef2f5"));
    QSettings().remove("workspace/skin");
}

// U25 part 1: the new roles have the prototype's colors; a color the catalog
// gives a role wins over the palette.
void TestTeamsUi::rolePaletteReadsTheCatalog()
{
    const auto saved = Teams::catalogRoleColors();
    Teams::catalogRoleColors().clear();
    QSettings().setValue("workspace/skin", "whitewh");
    const QList<std::tuple<QString, QString, QString>> roles{
        {"secretary", "#7fcf9a", "#2f7d4a"}, {"coder", "#e8a75a", "#a35f10"}, {"qa", "#4fd1c5", "#0f7a72"},
        {"designer", "#ef7d8d", "#b8384b"}, {"leadgpt", "#9aa5ff", "#4652c4"}, {"expert", "#e0b070", "#8a5d16"},
        {"proexpert", "#ef7d6d", "#b8402f"}, {"lead", "#b79cff", "#6f4fd8"}, {"backend", "#6fa8ff", "#2f66c9"}};
    for (const auto &[role, dark, light] : roles) {
        QCOMPARE(TeamUi::roleColor(role, true), QColor(dark));
        QCOMPARE(TeamUi::roleColor(role, false), QColor(light));
    }
    // Names as the catalog writes them: the longest palette role they contain.
    QCOMPARE(TeamUi::roleColor("Lead (GPT)", true), QColor("#9aa5ff"));
    QCOMPARE(TeamUi::roleColor("ProExpert", true), QColor("#ef7d6d"));
    QCOMPARE(TeamUi::roleColor("backend-2", true), QColor("#6fa8ff"));
    // Any other role gets a stable palette color, never a lead's.
    const QColor other = TeamUi::roleColor("cartographer", true);
    QCOMPARE(TeamUi::roleColor("cartographer", true), other);
    QVERIFY(other != QColor("#b79cff") && other != QColor("#9aa5ff"));
    // The Team designer recolors a role: the catalog's color wins, by id and name.
    Catalog recolored;
    Role coder; coder.id = "coder"; coder.name = "Coder"; coder.color = "#22aa55"; recolored.roles << coder;
    Role broken; broken.id = "qa"; broken.color = "url(x)"; recolored.roles << broken;
    Teams::rememberRoleColors(recolored);
    QCOMPARE(TeamUi::roleColor("coder", true), QColor("#22aa55"));
    QCOMPARE(TeamUi::roleColor("Coder", false), QColor("#22aa55"));
    QCOMPARE(TeamUi::roleColor("qa", true), QColor("#4fd1c5"));
    Teams::catalogRoleColors() = saved;
    QSettings().remove("workspace/skin");
}

// U25 part 1: thin 8 px scroll bars without arrows, the thumb text3 at 24 %.
void TestTeamsUi::scrollBarsAreThin()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const QString sheet = workspaceScrollbars(true);
    QVERIFY(sheet.contains("QScrollBar:vertical { width:8px;"));
    QColor thumb("#aab1ba"); thumb.setAlphaF(.24);
    QVERIFY(sheet.contains(thumb.name(QColor::HexArgb)));
    QColor hover("#aab1ba"); hover.setAlphaF(.45);
    QVERIFY(sheet.contains(hover.name(QColor::HexArgb)));
    QWidget box; box.setStyleSheet(sheet); box.resize(240, 160);
    auto *list = new QListWidget(&box); list->resize(240, 160);
    for (int i = 0; i < 60; ++i) list->addItem(QStringLiteral("Row %1").arg(i));
    box.show(); QTest::qWait(20);
    QTRY_VERIFY(list->verticalScrollBar()->isVisible());
    QCOMPARE(list->verticalScrollBar()->width(), 8);
    QSettings().remove("workspace/skin");
}

// U25 part 1: the toothed gear of the prototype (the title bar's 19 px and the
// services chip) and the crossed-out bell of mute.
void TestTeamsUi::gearAndBellOffIcons()
{
    const auto painted = [](const QString &name) {
        const QImage image = workspaceIcon(name, QColor("#ffffff")).pixmap(48, 48).toImage();
        int count = 0;
        for (int y = 0; y < image.height(); ++y) for (int x = 0; x < image.width(); ++x) count += image.pixelColor(x, y).alpha() > 128;
        return std::pair{image, count};
    };
    QVERIFY(prototypeIconPath("settings")); QVERIFY(prototypeIconPath("bell-off"));
    QPixmap probe(24, 24); probe.fill(Qt::transparent);
    { QPainter p(&probe); QVERIFY2(paintPrototypeIcon(p, 24, prototypeIconPath("settings"), Qt::white), "the SVG image plugin is missing"); }
    const auto [gear, gearInk] = painted("settings");
    const auto [bell, bellInk] = painted("bell");
    const auto [off, offInk] = painted("bell-off");
    QVERIFY(gearInk > 100); QVERIFY(bellInk > 60); QVERIFY(offInk > 60);
    QVERIFY(bell != off);
    // Mute is the crossed-out bell now.
    QCOMPARE(painted("mute").first, off);
    // The gear's teeth: its outline reaches the corners' diagonals, which the old
    // sun of spokes never did, and its centre hole is clear.
    QVERIFY(gear.pixelColor(24, 24).alpha() < 60);
    QVERIFY(!workspaceIcon("settings", QColor("#9aa3ad")).pixmap(19, 19).isNull());
}

// U25 part 1: the one collapsible block: chevron header, aria-expanded, the
// collapsed summary and its state in QSettings under its key.
void TestTeamsUi::collapsibleBlockKeepsItsState()
{
    QSettings().remove(CollapsibleBlock::settingsKey("test/folders"));
    QList<bool> changes;
    {
        CollapsibleBlock block("test/folders", "Folders", false);
        block.setNote("3"); block.setSummary("zerus, docs, mobile");
        auto *add = new QPushButton("Folder"); block.trailing()->addWidget(add);
        auto *row = new QLabel("D:/Work/zerus"); block.body()->addWidget(row);
        block.expandedChanged = [&](bool open) { changes << open; };
        block.resize(360, 120); block.show(); QTest::qWait(20);
        QVERIFY(!block.isExpanded());
        QVERIFY(!row->isVisible()); QVERIFY(add->isVisible());
        auto *header = block.findChild<QPushButton *>("collapsibleHeader");
        auto *summary = block.findChild<QPushButton *>("collapsibleSummary");
        QVERIFY(header && summary);
        QVERIFY(!header->isChecked()); QCOMPARE(header->property("expanded").toBool(), false);
        QCOMPARE(header->accessibleDescription(), QString("Collapsed")); QCOMPARE(header->accessibleName(), QString("Folders"));
        QVERIFY(summary->isVisible()); QCOMPARE(summary->text(), QString("zerus, docs, mobile"));
        QCOMPARE(block.findChild<QLabel *>("collapsibleNote")->text(), QString("3"));
        header->click();
        QVERIFY(block.isExpanded()); QVERIFY(row->isVisible()); QVERIFY(!summary->isVisible()); QVERIFY(add->isVisible());
        QVERIFY(header->isChecked()); QCOMPARE(header->accessibleDescription(), QString("Expanded"));
        QCOMPARE(QSettings().value(CollapsibleBlock::settingsKey("test/folders")).toBool(), true);
        // The keyboard toggles it as well.
        header->setFocus(); QTest::keyClick(header, Qt::Key_Space);
        QVERIFY(!block.isExpanded());
        summary->click();   // the summary expands the block
        QVERIFY(block.isExpanded());
    }
    QCOMPARE(changes, (QList<bool>{true, false, true}));
    // A new block under the same key starts as it was left.
    CollapsibleBlock again("test/folders", "Folders", false);
    QVERIFY(again.isExpanded());
    // No key keeps no state.
    CollapsibleBlock loose({}, "Loose", false);
    loose.setExpanded(true);
    QVERIFY(!QSettings().contains(CollapsibleBlock::settingsKey({})));
    QSettings().remove(CollapsibleBlock::settingsKey("test/folders"));
}

// U25 part 1: the agent-works indicator: an accent ring and three dots in a
// wave under the model tile; static when animations are off.
void TestTeamsUi::agentWorkingIndicatorFollowsTheSetting()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto frame = [](bool working, double phase) {
        QImage image(17, 17 + AgentWorking::Below, QImage::Format_ARGB32_Premultiplied); image.fill(Qt::transparent);
        QPainter p(&image);
        AgentWorking::paintTile(p, QRectF(0, 0, 17, 17), workspaceIcon("terminal", QColor("#c4cad1")), true, working, phase);
        return image;
    };
    AgentWorking::setAnimated(true);
    // The risen dot walks left, middle, right in one period.
    QCOMPARE(AgentWorking::rise(0, 0), 1.0); QCOMPARE(AgentWorking::rise(1, 1 / 3.0), 1.0); QCOMPARE(AgentWorking::rise(2, 2 / 3.0), 1.0);
    QCOMPARE(AgentWorking::rise(1, 0), 0.0);
    QVERIFY(frame(true, 0) != frame(true, 1 / 3.0));
    // Idle: no ring, no dots under the tile.
    const QImage idle = frame(false, 0), busy = frame(true, 0);
    bool dotsIdle = false, dotsBusy = false;
    for (int y = 18; y < idle.height(); ++y) for (int x = 0; x < 17; ++x) {
        dotsIdle = dotsIdle || idle.pixelColor(x, y).alpha() > 0;
        dotsBusy = dotsBusy || busy.pixelColor(x, y).alpha() > 0;
    }
    QVERIFY(!dotsIdle); QVERIFY(dotsBusy);
    // The ring: the accent mixed into the card border, on the tile's edge.
    const QColor edgeIdle = idle.pixelColor(8, 0), edgeBusy = busy.pixelColor(8, 0);
    QVERIFY(edgeBusy.green() > edgeIdle.green() + 40);
    // Animations off: the dots stand still at every phase.
    AgentWorking::setAnimated(false);
    QCOMPARE(frame(true, 0), frame(true, 1 / 3.0));
    AgentTile tile; tile.setAgent("claude"); tile.setWorking(true); tile.show(); QTest::qWait(20);
    QVERIFY(!tile.isAnimating());
    AgentWorking::setAnimated(true); tile.refreshAnimation();
    QVERIFY(tile.isAnimating());
    QCOMPARE(tile.size(), QSize(17, 17 + AgentWorking::Below));
    tile.hide(); QVERIFY(!tile.isAnimating());
    tile.show(); tile.setWorking(false); QVERIFY(!tile.isAnimating());
    QSettings().remove("workspace/skin");
}

namespace {
// The part-1 pieces together, as the shots show them in either theme.
QWidget *foundationShowcase(bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)); };
    const auto c = TeamUi::colorsFor(dark);
    auto *page = new QWidget; page->setObjectName("uxFoundation");
    page->setStyleSheet(QStringLiteral("QWidget#uxFoundation{background:%1;}QLabel{color:%2;background:transparent;}").arg(t("bg").name(), t("fg").name())
                        + TeamUi::teamSheet(c) + workspaceScrollbars(dark));
    auto *v = new QVBoxLayout(page); v->setContentsMargins(16, 14, 16, 14); v->setSpacing(12);
    const auto caption = [&](const QString &text) { auto *l = TeamUi::label(text, t("label"), 11); v->addWidget(l); };
    caption(QStringLiteral("Role palette"));
    auto *roles = new TeamUi::FlowLayout(6);
    for (const auto &role : WorkspaceSkin::paletteRoles()) roles->addWidget(new TeamUi::Pill(role, TeamUi::roleColor(role, dark)));
    v->addLayout(roles);
    caption(QStringLiteral("Icons"));
    auto *icons = TeamUi::row(10);
    auto *gear = new QPushButton; gear->setProperty("ghost", true); gear->setIcon(workspaceIcon("settings", t("muted")));
    gear->setIconSize(QSize(19, 19)); gear->setFixedSize(30, 28); gear->setToolTip("Settings");
    icons->addWidget(gear);
    auto *chip = new QPushButton(QStringLiteral("2")); chip->setProperty("chip", true); chip->setIcon(workspaceIcon("settings", t("muted")));
    chip->setIconSize(QSize(12, 12)); icons->addWidget(chip);
    for (const char *name : {"bell", "bell-off"}) {
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QString::fromLatin1(name), t(name == QByteArray("bell") ? "muted" : "dim")).pixmap(16, 16));
        icons->addWidget(icon);
    }
    icons->addStretch(1);
    v->addLayout(icons);
    caption(QStringLiteral("Agent works"));
    auto *tiles = TeamUi::row(14);
    for (const auto &[agent, working, size] : QList<std::tuple<QString, bool, int>>{{"claude", false, 17}, {"codex", true, 17}, {"kimi", true, 22}, {"dsh", false, 22}}) {
        auto *tile = new AgentTile(size); tile->setAgent(agent); tile->setDark(dark); tile->setWorking(working);
        tiles->addWidget(tile, 0, Qt::AlignTop);
    }
    tiles->addStretch(1);
    v->addLayout(tiles);
    auto *folders = new CollapsibleBlock({}, QStringLiteral("Folders"), true);
    folders->setDark(dark); folders->setNote(QStringLiteral("3"));
    auto *add = new QPushButton(QStringLiteral("Folder")); add->setProperty("ghost", true); add->setIcon(workspaceIcon("add", t("text2")));
    folders->trailing()->addWidget(add);
    for (const char *row : {"zerus  ·  D:/Work/zerus", "docs  ·  D:/Work/docs", "mobile  ·  D:/Work/mobile"})
        folders->body()->addWidget(TeamUi::label(QString::fromUtf8(row), t("text2"), 12));
    v->addWidget(folders);
    auto *templates = new CollapsibleBlock({}, QStringLiteral("Templates"), false);
    templates->setDark(dark); templates->setNote(QStringLiteral("3"));
    templates->setSummary(QStringLiteral("Feature delivery → Bugfix → Dev Team"));
    templates->body()->addWidget(TeamUi::label(QStringLiteral("hidden"), t("text2"), 12));
    v->addWidget(templates);
    caption(QStringLiteral("Scroll bars"));
    auto *list = new QListWidget; list->setFixedHeight(96);
    list->setStyleSheet(QStringLiteral("QListWidget{background:%1;border:1px solid %2;border-radius:8px;color:%3;}").arg(t("surface").name(), t("cardBorder").name(), t("text2").name()));
    for (int i = 1; i <= 30; ++i) list->addItem(QStringLiteral("Session %1").arg(i));
    v->addWidget(list);
    v->addStretch(1);
    return page;
}
}

// U25 part 1: the shots of the pieces, dark and light (ux-1-*).
void TestTeamsUi::foundationShots()
{
    QSettings().setValue("workspace/skin", "whitewh");
    darkPalette();
    std::unique_ptr<QWidget> dark(foundationShowcase(true));
    dark->resize(460, 600); dark->show(); QTest::qWait(60);
    shot(dark.get(), "ux-1-foundation-dark");
    dark.reset();
    std::unique_ptr<QWidget> light(foundationShowcase(false));
    light->resize(460, 600); light->show(); QTest::qWait(60);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) lightShot(light.get(), "ux-1-foundation-light");
    darkPalette();
    QSettings().remove("workspace/skin");
}

// U25 (spec 3): the team's part of the lead session's header, in every state:
// the status, the waiting links, the three bars and the actions by state.
void TestTeamsUi::leadHeaderFollowsTheTeamState()
{
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    int opened = 0; workspace.openWorkspace = [&opened] { ++opened; };
    workspace.resize(1500, 820); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    const auto find = [&workspace](const char *name) { return workspace.findChild<QWidget *>(name); };
    const auto status = [&] { auto *badge = find("teamHeaderStatus"); return badge ? badge->property("status").toString() : QString(); };
    const auto shows = [&](const char *name) { auto *w = find(name); return w && w->isVisible(); };
    // Until a window takes it, the header shows above the canvas; no name bar.
    QTRY_COMPARE(status(), QStringLiteral("planning"));
    QVERIFY(find("teamWorkspaceHeader")->isVisible());
    QVERIFY(!find("teamWorkspaceBar"));
    // Planning: Discard plan…, Send back, Approve & Run held by the plan error.
    QVERIFY(shows("teamWorkspaceDiscard") && shows("teamWorkspaceSendBack") && shows("teamWorkspaceApprove"));
    // Approve & Run reads the plan errors from `team status`: until it answers
    // it says it is checking, then it names the errors (#382 review: no fixed wait).
    const auto approveTip = [&] { auto *b = workspace.findChild<QPushButton *>("teamWorkspaceApprove"); return b && !b->isEnabled() ? b->toolTip() : QString(); };
    QTRY_COMPARE(approveTip(), QStringLiteral("Fix the plan errors first"));
    // Send back opens its dialog from the workspace: a poll that rebuilds the
    // header under the dialog deletes the button, never code that still runs.
    {
        QPointer<QWidget> sendBack = find("teamWorkspaceSendBack");
        bool rebuiltUnder = false, sawDialog = false;
        QTimer watch; watch.setInterval(5);
        QObject::connect(&watch, &QTimer::timeout, [&] {
            auto *dialog = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!dialog || sawDialog) return;
            sawDialog = true;
            controller.refresh(); QTest::qWait(150);
            workspace.render();
            // Rebuilt: the old button is retired (unnamed, hidden) or gone already.
            rebuiltUnder = sendBack.isNull() || (sendBack->objectName().isEmpty() && !sendBack->isVisible());
            dialog->reject();
        });
        watch.start();
        static_cast<QPushButton *>(sendBack.data())->click();
        QTRY_VERIFY(sawDialog);
        QTRY_VERIFY(rebuiltUnder);
        watch.stop();
        QTRY_VERIFY(sendBack.isNull());
        QVERIFY(find("teamWorkspaceSendBack"));
    }
    QVERIFY(!find("teamWorkspacePause") && !find("teamWorkspaceStop") && !find("teamBarPendingPhase"));
    // Running: Pause and Stop…; the bars count the team's tasks and phases.
    setTeam(team());
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "running"; })());
    QVERIFY(shows("teamWorkspacePause") && shows("teamWorkspaceStop"));
    QVERIFY(!find("teamWorkspaceApprove") && !find("teamWorkspaceFinish") && !find("teamHeaderFinishWaits"));
    QCOMPARE(find("teamHeaderTasks")->property("done").toInt(), 1);
    QCOMPARE(find("teamHeaderTasks")->property("total").toInt(), 5);
    QCOMPARE(find("teamHeaderPhases")->property("total").toInt(), 1);
    QVERIFY(shows("teamHeaderElapsed"));
    // Stop… asks first: a cancel sends nothing, a confirm sends stop once.
    armConfirm(&panel, true);
    workspace.findChild<QPushButton *>("teamWorkspaceStop")->click();
    QTest::qWait(50); QCOMPARE(calls(script(), "stop").size(), 0);
    armConfirm(&panel, false);
    workspace.findChild<QPushButton *>("teamWorkspaceStop")->click();
    QTRY_COMPARE(calls(script(), "stop").size(), 1);
    // A draft phase waits: "Phase 3 waits for you" opens the Team tab on it, unfolded.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); return find("teamBarPendingPhase") != nullptr; })());
    auto *waits = workspace.findChild<QPushButton *>("teamBarPendingPhase");
    QCOMPARE(waits->text(), QStringLiteral("Phase 3 waits for you"));
    QCOMPARE(find("teamHeaderPhase")->property("caption").toString(), QStringLiteral("Phase 2 · QA fixes"));
    QCOMPARE(find("teamHeaderPhases")->property("total").toInt(), 3);
    // N16 (#382): the bars count accepted tasks, as the phase's scroll does: a
    // cancelled task in phase 2 is decided, not done.
    {
        auto counted = phasedTeam(); counted["revision"] = 40;
        auto tasks = counted["tasks"].toArray();
        tasks.append(QJsonObject{{"id", 6}, {"phase", 2}, {"title", "Old fix"}, {"assignee", "backend"}, {"status", "cancelled"}});
        tasks.append(QJsonObject{{"id", 7}, {"phase", 2}, {"title", "Retry"}, {"assignee", "tester"}, {"status", "rejected"}});
        counted["tasks"] = tasks;
        auto phases = counted["phases"].toArray(); auto two = phases[0].toObject(); two["tasks"] = QJsonArray{3, 4, 6, 7}; phases[0] = two; counted["phases"] = phases;
        setTeam(counted);
        QTRY_VERIFY(([&] { controller.refresh(); auto *bar = find("teamHeaderTasks"); return bar && bar->property("total").toInt() == 7; })());
        QCOMPARE(find("teamHeaderPhase")->property("done").toInt(), 1);
        QCOMPARE(find("teamHeaderPhase")->property("total").toInt(), 4);
        QCOMPARE(find("teamHeaderTasks")->property("done").toInt(), 3);
        QVERIFY(find("teamHeaderTasks")->toolTip().startsWith(QStringLiteral("Accepted")));
        setTeam(phasedTeam());
        QTRY_VERIFY(([&] { controller.refresh(); auto *bar = find("teamHeaderTasks"); return bar && bar->property("total").toInt() == 5; })());
        waits = workspace.findChild<QPushButton *>("teamBarPendingPhase"); QVERIFY(waits);
    }
    waits->click();
    QCOMPARE(opened, 1);
    QTRY_VERIFY(([&] { const auto *t = controller.team("team-1"); if (!t) return false;
                       for (const auto &g : phaseGroups(*t)) if (g.n == 3) return panel.phaseOpen(*t, g); return false; })());
    // Every task decided: "Waiting for you to finish" and Finish….
    auto decided = team();
    auto tasks = decided["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto t = tasks[i].toObject(); t["status"] = "accepted"; tasks[i] = t; }
    decided["tasks"] = tasks;
    setTeam(decided);
    QTRY_VERIFY(([&] { controller.refresh(); return find("teamHeaderFinishWaits") != nullptr; })());
    QCOMPARE(workspace.findChild<QPushButton *>("teamHeaderFinishWaits")->text(), QStringLiteral("Waiting for you to finish"));
    QVERIFY(shows("teamWorkspaceFinish") && shows("teamWorkspacePause") && shows("teamWorkspaceStop"));
    // Paused: Resume and Stop…; Done: Archive and Next goal…; Stopped: Archive.
    auto paused = team(); paused["status"] = "paused"; setTeam(paused);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "paused"; })());
    QVERIFY(shows("teamWorkspaceResume") && shows("teamWorkspaceStop") && !find("teamWorkspacePause"));
    auto done = decided; done["status"] = "done"; setTeam(done);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "done"; })());
    QVERIFY(shows("teamWorkspaceArchive") && shows("teamWorkspaceNextGoal"));
    QVERIFY(!find("teamWorkspaceStop") && !find("teamWorkspaceFinish") && !find("teamHeaderFinishWaits") && !find("teamWorkspaceResumeLimits"));
    auto stopped = team(); stopped["status"] = "stopped"; setTeam(stopped);
    QTRY_VERIFY(([&] { controller.refresh(); return status() == "stopped"; })());
    QVERIFY(shows("teamWorkspaceArchive") && !find("teamWorkspaceNextGoal") && !find("teamWorkspaceStop"));
    // N17 (#382): Archive asks first; Cancel sends nothing, Archive sends it once for this team.
    armConfirm(&panel, true);
    workspace.findChild<QPushButton *>("teamWorkspaceArchive")->click();
    QTest::qWait(80); QCOMPARE(calls(script(), "archive").size(), 0);
    armConfirm(&panel, false);
    workspace.findChild<QPushButton *>("teamWorkspaceArchive")->click();
    QTRY_COMPARE(calls(script(), "archive").size(), 1);
    QCOMPARE(calls(script(), "archive").first()["team"].toString(), QStringLiteral("team-1"));
    // N17: Approve & Run always asks, and names the member sessions it starts.
    {
        auto clean = team("planning"); clean["revision"] = 50;
        clean["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}};
        auto members = clean["members"].toArray(); auto backend = members[1].toObject(); backend.remove("session"); members[1] = backend;
        auto tester = members[2].toObject(); tester.remove("session"); members[2] = tester; clean["members"] = members;
        setTeam(clean);
        QPushButton *approve = nullptr;
        QTRY_VERIFY(([&] { controller.refresh(); approve = workspace.findChild<QPushButton *>("teamWorkspaceApprove"); return approve && approve->isEnabled(); })());
        QString asked;
        QTimer watch; watch.setInterval(10);
        QObject::connect(&watch, &QTimer::timeout, &panel, [&] {
            auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
            if (!box || !asked.isEmpty()) return;
            asked = box->windowTitle() + QLatin1Char('|') + box->text();
            box->button(QMessageBox::Cancel)->click();
        });
        watch.start();
        approve->click();
        QTRY_VERIFY(!asked.isEmpty());
        watch.stop();
        QTest::qWait(50); QCOMPARE(calls(script(), "approve").size(), 0);
        QVERIFY2(asked.startsWith(QStringLiteral("Approve plan and run?|")), qPrintable(asked));
        QVERIFY2(asked.contains(QStringLiteral("It starts 2 member sessions: Backend (codex), Tester (kimi on msi-old).")), qPrintable(asked));
        armConfirm(&panel, false);
        workspace.findChild<QPushButton *>("teamWorkspaceApprove")->click();
        QTRY_COMPARE(calls(script(), "approve").size(), 1);
        QVERIFY(!calls(script(), "approve").first().contains("confirm_widening"));
        setTeam(stopped);
        QTRY_VERIFY(([&] { controller.refresh(); return status() == "stopped"; })());
    }
    // Not the lead's session: nothing; an archived team: nothing.
    workspace.setHeaderShown(false);
    QVERIFY(!find("teamHeaderStatus") && !workspace.headerStatus()->isVisible() && !find("teamWorkspaceHeader")->isVisible());
    workspace.setHeaderShown(true);
    QVERIFY(find("teamHeaderStatus"));
    auto archived = team(); archived["status"] = "archived"; setTeam(archived);
    QTRY_VERIFY(([&] { controller.refresh(); return !find("teamHeaderStatus"); })());
    // A window takes the two parts into its own header.
    workspace.takeHeader();
    QVERIFY(!workspace.headerStatus()->parentWidget() && !workspace.headerActions()->parentWidget());
    QVERIFY(!find("teamWorkspaceHeader")->isVisible());
}

namespace {
// The lead session's header as the window lays it out (spec 3): the title, the
// team after it and the session's pill; the chips, then the team's actions.
QWidget *leadHeaderMock(TeamWorkspace &workspace, bool dark)
{
    const auto &skin = WorkspaceSkin::active();
    const auto t = [&skin, dark](const char *name) { return WorkspaceSkin::color(skin, dark, QString::fromLatin1(name)).name(); };
    auto *frame = new QFrame; frame->setObjectName("leadHeaderMock");
    frame->setStyleSheet(QStringLiteral("QFrame#leadHeaderMock{background:%1;} QLabel#mockTitle{color:%2;font-size:16px;font-weight:600;}"
                                        "QLabel[chip=\"true\"]{background:%3;color:%4;border-radius:6px;padding:2px 7px;font-size:11px;}"
                                        "QLabel#mockMeta{color:%5;font-size:11px;} QLabel#mockPill{color:%4;border:1px solid %6;border-radius:12px;padding:3px 10px;font-size:11.5px;}")
                             .arg(t("bg"), t("fg"), t("hover"), t("text3"), t("dim"), t("border")));
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(14, 10, 14, 10); v->setSpacing(6);
    auto *top = new QHBoxLayout; top->setSpacing(4);
    auto *title = new QLabel(QStringLiteral("Orbit platform / checkout")); title->setObjectName("mockTitle");
    top->addWidget(title); top->addSpacing(10); top->addWidget(workspace.headerStatus()); top->addStretch(1);
    auto *pill = new QLabel(QStringLiteral("Ready")); pill->setObjectName("mockPill"); top->addWidget(pill);
    v->addLayout(top);
    auto *chips = new QHBoxLayout; chips->setSpacing(5);
    for (const auto &text : {QStringLiteral("Claude"), QStringLiteral("workstation"), QStringLiteral("feat/checkout-v2")}) {
        auto *chip = new QLabel(text); chip->setProperty("chip", true); chips->addWidget(chip);
    }
    auto *meta = new QLabel(QStringLiteral("orbit / feat/checkout-v2")); meta->setObjectName("mockMeta"); chips->addWidget(meta);
    chips->addStretch(1); chips->addWidget(workspace.headerActions());
    v->addLayout(chips);
    return frame;
}
}

// U25: the lead header in the prototype's scenarios (shots 03, 24, 25), dark and light.
void TestTeamsUi::leadHeaderShots()
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) QSKIP("Set ZERUS_TEAM_SHOTS=<dir> to keep the pictures.");
    QSettings().setValue("workspace/skin", "whitewh");
    auto phased = phasedTeam(); phased["created"] = QDateTime::currentSecsSinceEpoch() - 4140;
    auto decided = team(); decided["created"] = QDateTime::currentSecsSinceEpoch() - 8940;
    auto tasks = decided["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto t = tasks[i].toObject(); t["status"] = "accepted"; tasks[i] = t; }
    decided["tasks"] = tasks;
    auto planning = team("planning"); planning["created"] = QDateTime::currentSecsSinceEpoch() - 720;
    const QList<QPair<QString, QJsonObject>> scenarios{{"running", phased}, {"planning", planning}, {"done", decided}};
    for (const auto &[name, json] : scenarios) {
        for (const bool dark : {true, false}) {
            setTeam(json);
            HgsClient client(FakeCli::program(script()));
            TeamController controller(&client);
            TeamPanel panel(&controller);
            TeamWorkspace workspace(&controller, &panel);
            workspace.setLive(live);
            workspace.takeHeader();
            if (dark) darkPalette(); else lightPalette();
            workspace.setTheme(dark);
            std::unique_ptr<QWidget> header(leadHeaderMock(workspace, dark));
            header->resize(1180, 76); header->show();
            controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
            QTRY_VERIFY(workspace.headerStatus()->findChild<QWidget *>("teamHeaderStatus"));
            QTest::qWait(100);
            const QString file = QStringLiteral("ux-3-header-%1-%2").arg(name, dark ? "dark" : "light");
            if (dark) shot(header.get(), file); else lightShot(header.get(), file);
            header.reset();
        }
    }
    darkPalette();
    QSettings().remove("workspace/skin");
}

namespace {
// U25: the prototype's team (shots 10-19): Analysis done, Build running with a
// question, Verify & ship a draft with the lead's note.
QJsonObject prototypeTeam(const QString &third = QStringLiteral("draft"))
{
    auto t = QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Checkout","status":"running","project":"Orbit platform","folder":"/work/orbit","lead":"lead",
 "revision":20,"created":1000,"updated":5000,"settings":{},"pending_phase":3,
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","model":"claude-opus-4-6","state":"active","session":{"host":"","name":"claude/orbit/checkout"}},
  {"id":"backend","role":"Backend","agent":"dsh","model":"deepseek-v4-pro","state":"active","session":{"host":"","name":"dsh/orbit/checkout-api"}},
  {"id":"frontend","role":"Frontend","agent":"codex","model":"gpt-6.1-sol","state":"active","session":{"host":"","name":"codex/orbit/checkout-ui"}},
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2.5","state":"active","session":{"host":"","name":"kimi/orbit/checkout-tests"}}],
 "phases":[
  {"n":1,"title":"Analysis","state":"approved","tasks":[1],"added_by":"Lead","added_at":1000},
  {"n":2,"title":"Build","state":"approved","tasks":[2,3,4,5,6],"added_by":"Lead","added_at":1000},
  {"n":3,"title":"Verify & ship","state":"draft","tasks":[8,9,7],"added_by":"Lead","added_at":1700,
   "note":"Lead proposed the next phase while Build is running: rate limits, a load test and the release notes."}],
 "tasks":[
  {"id":1,"phase":1,"title":"Requirements and acceptance","assignee":"lead","status":"accepted"},
  {"id":2,"phase":2,"title":"Saved cards API","assignee":"backend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a2","member":"backend","state":"delivered","sent_at":1400}]},
  {"id":3,"phase":2,"title":"Card form UI","assignee":"frontend","status":"in_progress","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a3","member":"frontend","state":"delivered","sent_at":1400}]},
  {"id":4,"phase":2,"title":"Address validation","assignee":"frontend","status":"submitted","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a4","member":"frontend","state":"reported","sent_at":1300,"report":{"text":"Done","at":1500}}]},
  {"id":5,"phase":2,"title":"Contract tests","assignee":"tester","status":"pending","deps":[{"task":2}]},
  {"id":6,"phase":2,"title":"E2E checkout flow","assignee":"tester","status":"pending","deps":[{"task":3},{"task":5}]},
  {"id":8,"phase":3,"title":"Rate limits on /v1/cards","assignee":"backend","status":"draft","deps":[{"task":2}]},
  {"id":9,"phase":3,"title":"Load test checkout","assignee":"tester","status":"draft","deps":[{"task":6}]},
  {"id":7,"phase":3,"title":"Release notes","assignee":"lead","status":"draft","deps":[{"task":8},{"task":9}]}],
 "layers":{"1":0,"2":1,"3":1,"4":1,"5":2,"6":3,"8":2,"9":4,"7":5},"cycle":[],"ready":[],
 "validation":{"errors":[],"warnings":[]}})").object();
    auto phases = t["phases"].toArray();
    auto last = phases[2].toObject();
    if (third == "rejected") {
        last["state"] = "rejected";
        last["rejection"] = QJsonObject{{"comment", "Run the load test before the rate limits; release notes last."}, {"by", "owner"}, {"at", 1800}};
        t["pending_phase"] = QJsonValue();
    } else if (third == "queued") {
        last["state"] = "queued";
        last["waiting"] = QJsonObject{{"text", "Starts after phase 2"}, {"phase", 2}};
        auto tasks = t["tasks"].toArray();
        for (int i = 0; i < tasks.size(); ++i) { auto k = tasks[i].toObject(); if (k["status"] == "draft") k["status"] = "pending"; tasks[i] = k; }
        t["tasks"] = tasks; t["pending_phase"] = QJsonValue();
    }
    phases[2] = last; t["phases"] = phases;
    // The store's revision moves with every decision.
    t["revision"] = third == "rejected" ? 21 : third == "queued" ? 22 : 20;
    return t;
}

Live prototypeLive(const Member &member)
{
    Live state; state.known = true;
    if (member.id == "backend") { state.attention = true; state.attentionText = QStringLiteral("Approval: Run command: cargo sqlx migrate run"); }
    return state;
}
}

// U25 (spec 5.1, 5.3, 5.5): every phase a collapsible scroll in Graph, Board
// and Layers, with its number, status badge, accepted/tasks bar, proposed by or
// health, the lead's note and the owner's actions in its header.
void TestTeamsUi::phaseScrollsInEveryView()
{
    QSettings().setValue("workspace/skin", "whitewh");
    QSettings().remove("ui/collapsed/team/needs");
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
    QWidget *card2 = nullptr;
    for (auto *card : scroll(2)->findChildren<QFrame *>("teamBoardCard")) if (card->property("task").toInt() == 2) card2 = card;
    QVERIFY(card2); QCOMPARE(card2->parentWidget()->property("column").toString(), QStringLiteral("problems"));
    QVERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseAccept"));
    // Collapse Analysis again for the pictures.
    scroll(1)->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(scroll(1) && !scroll(1)->findChild<QWidget *>("teamPhaseBody"));
    shot(&workspace, "ux-4-board-dark");
    // Layers: one line per task, L1… inside each phase.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "layers") b->click();
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_COMPARE(scrolls().size(), 3);
    QCOMPARE(scroll(2)->findChildren<QFrame *>("teamLayerRow").size(), 5);
    QCOMPARE(scroll(3)->findChildren<QFrame *>("teamLayerRow").size(), 3);
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
    QSettings().remove("workspace/skin");
}

// U25 (spec 5.2, 5.3): a draft's Accept, Reject… with the lead's words, Undo
// of a send-back, the queued state and Auto-accept, from the scroll's header.
// N14 (#383): the send-back form belongs to one team's phase as it was when
// it opened: another team with a draft of the same number shows no form and no
// words, renumbered drafts close it, and the request names the revision it
// was written against (the server refuses it after a renumbering).
void TestTeamsUi::sendBackFormBelongsToItsTeamAndPhase()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); });
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

void TestTeamsUi::phaseScrollDecisions()
{
    QSettings().setValue("workspace/skin", "whitewh");
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
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPushButton *>("teamPhaseReject"));
    // Reject… opens the form under the note; Cancel closes it and sends nothing.
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    auto *text = scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText");
    QCOMPARE(text->placeholderText(), QStringLiteral("What should Lead change in this phase?"));
    QVERIFY(!scroll(3)->findChild<QPushButton *>("teamPhaseRejectSend")->isEnabled());
    scroll(3)->findChild<QPushButton *>("teamPhaseRejectCancel")->click();
    QTRY_VERIFY(scroll(3) && !scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    // The words survive a re-render while typing, then go to the lead.
    scroll(3)->findChild<QPushButton *>("teamPhaseReject")->click();
    QTRY_VERIFY(scroll(3) && scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"));
    QTest::keyClicks(scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText"), "Run the load test before the rate limits; release notes last.");
    workspace.render(); QTest::qWait(50);
    QTRY_COMPARE(scroll(3)->findChild<QPlainTextEdit *>("teamPhaseRejectText")->toPlainText(),
                 QStringLiteral("Run the load test before the rate limits; release notes last."));
    shot(&workspace, "ux-4-phase-reject-form-dark");
    scroll(3)->findChild<QPushButton *>("teamPhaseRejectSend")->click();
    QTRY_COMPARE(calls(script(), "reject-phase").size(), 1);
    QCOMPARE(calls(script(), "reject-phase").first()["phase"].toInt(), 3);
    QCOMPARE(calls(script(), "reject-phase").first()["comment"].toString(), QStringLiteral("Run the load test before the rate limits; release notes last."));
    // Sent back: the warn badge, the owner's words and Undo.
    setTeam(prototypeTeam("rejected"));
    QTRY_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "rejected"; })());
    QTRY_VERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseUndo") && scroll(3)->findChild<QPushButton *>("teamPhaseUndo")->isEnabled());
    QVERIFY(scroll(3)->findChild<QLabel *>("teamPhaseSentBack")->text().startsWith(QStringLiteral("Sent back: “Run the load test")));
    QVERIFY(!scroll(3)->findChild<QPushButton *>("teamPhaseAccept"));
    QCOMPARE(workspace.findChild<QPushButton *>("teamWorkspaceDrafts")->property("draftCount").toInt(), 1);
    shot(&workspace, "ux-4-phase-rejected-dark");
    scroll(3)->findChild<QPushButton *>("teamPhaseUndo")->click();
    QTRY_COMPARE(calls(script(), "undo-reject-phase").size(), 1);
    QCOMPARE(calls(script(), "undo-reject-phase").first()["phase"].toInt(), 3);
    // Queued: accepted, waiting for Build.
    setTeam(prototypeTeam("queued"));
    QTRY_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "queued"; })());
    QCOMPARE(scroll(3)->findChild<QLabel *>("teamPhaseQueued")->text(), QStringLiteral("Starts after phase 2"));
    QVERIFY(!workspace.findChild<QPushButton *>("teamWorkspaceDrafts"));
    shot(&workspace, "ux-4-phase-queued-dark");
    // A draft again: Accept asks first, then approves; Auto-accept is the team's setting.
    auto again = prototypeTeam(); again["revision"] = 23;
    setTeam(again);
    QTRY_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "draft" && scroll(3)->findChild<QPushButton *>("teamPhaseAccept"); })());
    QTRY_VERIFY(scroll(3)->findChild<QPushButton *>("teamPhaseAccept")->isEnabled());
    armConfirm(&panel, false);
    scroll(3)->findChild<QPushButton *>("teamPhaseAccept")->click();
    QTRY_COMPARE(calls(script(), "approve-phase").size(), 1);
    QCOMPARE(calls(script(), "approve-phase").first()["phase"].toInt(), 3);
    workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept")->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 1);
    QVERIFY(calls(script(), "settings").first()["settings"].toObject()["auto_accept"].toBool());
    // N13 (#383): only the switch's field goes; the server merges it, so a
    // lead's newer resume_after_limits or reminder stays.
    QCOMPARE(calls(script(), "settings").first()["settings"].toObject().keys(), QStringList{QStringLiteral("auto_accept")});
    // A stopped team takes no settings: both switches are off limits.
    auto stopped = again; stopped["status"] = "stopped"; stopped["revision"] = 24;
    setTeam(stopped);
    QTRY_VERIFY(([&] {
        controller.refresh();
        auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept");
        auto *phase = scroll(3) ? scroll(3)->findChild<QAbstractButton *>("teamPhaseAutoAccept") : nullptr;
        return (!top || !top->isEnabled()) && (!phase || !phase->isEnabled()) && (top || phase);
    })());
    if (auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept")) top->click();
    QTest::qWait(100);
    QCOMPARE(calls(script(), "settings").size(), 1);
    again["revision"] = 25; setTeam(again);
    QTRY_VERIFY(([&] { controller.refresh(); auto *top = workspace.findChild<QAbstractButton *>("teamWorkspaceAutoAccept"); return top && top->isEnabled(); })());
    workspace.setTheme(false);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) {
        setTeam(prototypeTeam("rejected"));
        QTRY_VERIFY(([&] { controller.refresh(); return scroll(3) && scroll(3)->property("state") == "rejected"; })());
        lightShot(&workspace, "ux-4-phase-rejected-light");
    }
    darkPalette();
    QSettings().remove("workspace/skin");
}

// U25 (spec 5.7): the right column's lists start collapsed: Needs your answer
// with a phase draft's Review, Team stuck with Show on graph, Plan problems
// that open the task; the members' task lists are folded too.
void TestTeamsUi::sideListsAndMemberTasks()
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
        QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QWidget *>("teamWorkspaceGraph") && workspace.findChild<QPushButton *>("teamStuckShowOnGraph"); })());
        workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->click();
        QTRY_VERIFY(([&] {
            auto *v = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
            return v && v->emphasis == QSet<int>{2, 3};
        })());
        auto *flatGraph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamWorkspaceGraph"));
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
    shot(&workspace, "ux-4-side-stuck-dark");
    workspace.setTheme(false);
    if (!qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) lightShot(&workspace, "ux-4-side-stuck-light");
    darkPalette(); workspace.setTheme(true);
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

#include "test_teamsui.moc"
