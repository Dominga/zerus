// WhiteWh fork: agent teams in the desktop, M3–M6 (design/agent-teams S3, S4,
// S6, S7, S10, S11): the task card, planning edits, New team, the catalog
// editor, the Team designer's draft and the team workspace. Each sends one
// `hgs team` request per change, recorded by a fake CLI.
#include "Wait.h"
#include <QPointer>
#include <QScrollBar>
#include <QtTest>
#include <QPainter>
#include <memory>
#include <tuple>
#include <QStandardItemModel>
#include <algorithm>
#include <QStyle>
#include <QStyleFactory>
#include <QStyleOption>
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
#include <QProcess>
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
#include "ForkUpdater.h"
#include "TeamCatalogEditor.h"
#include "TeamSettingsDialog.h"
#include "TeamAttentionNotices.h"
#include "TeamAttentionWatch.h"
#include <QClipboard>
#include "TeamController.h"
#include "TeamDesignerPage.h"
#include "TeamWaitingPage.h"
#include "TeamWaitingBanner.h"
#include "TeamWaitingAnswer.h"
#include "TeamConversation.h"
#include "AccountSwitch.h"
#include "MessageComposer.h"
#include <QPainter>
#include <QUuid>
#include "QuestionCard.h"
#include "ActivityView.h"
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
#include "TeamStages.h"
#include "TeamHeat.h"
#include "WorkspaceStyle.h"
#include <QScopeGuard>
#include <QElapsedTimer>
#include <QTableWidget>
#include <QInputDialog>
#include "support/FakeCli.h"
#include "support/DialogTheme.h"
#include "TeamProfile.h"

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
            if (!menu) { if (tries > 0) look(TestWait::busy() ? tries : tries - 1); return; }
            seen = true;
            inspect(menu);
            menu->hide();
        });
    };
    // Retries count only while nothing is in flight: under a loaded machine
    // the popup can take seconds to show.
    look(1000);
    more->click();
    WAIT_VERIFY(seen);
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

// U25 part 5 (owner, 2026-10-27): one task per activity-label rule, plus a
// problem that must win over the label.
QJsonObject labelTeam()
{
    return QJsonDocument::fromJson(R"({
 "id":"team-1","name":"Labels","status":"running","project":"Checkout","folder":"/work/labels","lead":"lead",
 "revision":5,"created":1000,"updated":5000,"settings":{},"layers":{},"cycle":[],"ready":[],"validation":{"errors":[],"warnings":[]},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","state":"active","session":{"host":"","name":"claude/labels/lead"}},
  {"id":"backend","role":"Backend","agent":"dsh","state":"active","session":{"host":"","name":"dsh/labels/backend"}},
  {"id":"blocked","role":"Tester","agent":"codex","state":"active","session":{"host":"","name":"codex/labels/blocked"}}],
 "tasks":[
  {"id":1,"title":"Review me","assignee":"backend","status":"submitted","attempts":[{"n":1,"id":"a1","member":"backend","state":"reported"}]},
  {"id":2,"title":"Second try","assignee":"backend","status":"in_progress","attempts":[{"n":2,"id":"a2","member":"backend","state":"delivered","started_at":1200}]},
  {"id":3,"title":"First try","assignee":"backend","status":"in_progress","attempts":[{"n":1,"id":"a3","member":"backend","state":"delivered","started_at":1300}]},
  {"id":4,"title":"Held back","assignee":"backend","status":"pending","hold":{"by":"Lead","at":1400,"reason":"ask first"}},
  {"id":5,"title":"Delegated","assignee":"backend","status":"in_progress","subteam":{"team":"team-2","done":1,"total":2},"attempts":[{"n":1,"id":"a5","member":"backend","state":"delivered","started_at":1500}]},
  {"id":6,"title":"Queued","assignee":"backend","status":"pending","waits":{"for":"member","on":["Backend"],"unblock":"wait for Backend"},"attempts":[{"n":1,"id":"a6","member":"backend","state":"rejected"}]},
  {"id":7,"title":"Still a draft","assignee":"backend","status":"draft"},
  {"id":8,"title":"Done","assignee":"backend","status":"accepted"},
  {"id":9,"title":"Offline problem","assignee":"blocked","status":"in_progress","attempts":[{"n":2,"id":"a9","member":"blocked","state":"delivered","started_at":1600}]},
  {"id":10,"title":"Handed over","assignee":"lead","status":"in_progress","attempts":[{"n":1,"id":"a10","member":"lead","state":"delivered","delivered_at":1700}]},
  {"id":11,"title":"Turn ended","assignee":"lead","status":"in_progress","attempts":[{"n":1,"id":"a11","member":"lead","state":"delivered","started_at":1800,"turn_ended_at":1900}]},
  {"id":12,"title":"Not needed","assignee":"lead","status":"cancelled"}],
 "layers":{"1":0,"2":1,"3":1,"4":2,"5":2,"6":3,"7":3,"8":4,"9":1}})").object();
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
    TestWait::settle();
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
    TestWait::settle();
    return asked;
}

// Picks the action named `name` from the next popup menu (QMenu::exec blocks).
void chooseMenuAction(const QString &name, int tries = 300)
{
    QTimer::singleShot(10, qApp, [name, tries] {
        auto *menu = qobject_cast<QMenu *>(QApplication::activePopupWidget());
        if (!menu) { if (tries > 0) chooseMenuAction(name, TestWait::busy() ? tries : tries - 1); return; }
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
                    {"member", QJsonObject{{"role", "Designer"}, {"agent", "claude"}, {"model", "sonnet"}}}, {"hash", "h1"}},
        QJsonObject{{"id", "p2"}, {"kind", "swap-member"}, {"by", "Lead"}, {"at", 2100.0}, {"state", "open"}, {"reason", "Backend hit its limit"},
                    {"member", QJsonObject{{"member", "backend"}, {"agent", "codex"}, {"model", "gpt-5.5"}}}, {"hash", "h2"}},
        QJsonObject{{"id", "p3"}, {"kind", "remove-member"}, {"by", "Lead"}, {"at", 2200.0}, {"state", "open"}, {"reason", "Testing is done"},
                    {"member", "tester"}, {"hash", "h3"}},
        QJsonObject{{"id", "p4"}, {"kind", "next-goal"}, {"by", "Lead"}, {"at", 1900.0}, {"state", "rejected"}, {"reason", "Later"},
                    {"goal", "Saved cards"}}};
    t["swapped"] = QJsonArray{QJsonObject{{"member", "Backend"}, {"old", "dsh/checkout/backend-old"}, {"at", 1800.0}, {"by", "You"}}};
    return t;
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

void armConfirm(TeamPanel *panel, bool cancel, int tries = 300)
{
    QTimer::singleShot(0, panel, [panel, cancel, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) QTimer::singleShot(10, panel, [panel, cancel, tries] { armConfirm(panel, cancel, TestWait::busy() ? tries : tries - 1); });
            return;
        }
        for (auto *button : box->buttons())
            if (cancel ? box->buttonRole(button) == QMessageBox::RejectRole
                       : box->buttonRole(button) == QMessageBox::DestructiveRole) { button->click(); return; }
        for (auto *button : box->buttons())
            if (!button->text().contains(QStringLiteral("Cancel"))) { button->click(); return; }
    });
}

// Answers the next confirmation after recording its object name and text: Cancel
// clicks the reject button, otherwise the destructive action. F12/N40 uses it
// to check what the owner is warned about.
void armConfirmRecord(TeamPanel *panel, bool cancel, QString *name, QString *text, int delay = 0, int tries = 300)
{
    QTimer::singleShot(delay, panel, [panel, cancel, name, text, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) armConfirmRecord(panel, cancel, name, text, 10, TestWait::busy() ? tries : tries - 1);
            return;
        }
        if (name) *name = box->objectName();
        if (text) *text = box->text();
        for (auto *button : box->buttons())
            if (cancel ? box->buttonRole(button) == QMessageBox::RejectRole
                       : box->buttonRole(button) == QMessageBox::DestructiveRole) { button->click(); return; }
        box->reject();
    });
}

// Answers the next confirmation after running `during` while it is open, as a
// poll arriving under the dialog does.
void armConfirmAfter(TeamPanel *panel, std::function<void()> during, int tries = 300)
{
    QTimer::singleShot(0, panel, [panel, during, tries] {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) {
            if (tries > 0) QTimer::singleShot(10, panel, [panel, during, tries] { armConfirmAfter(panel, during, TestWait::busy() ? tries : tries - 1); });
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
    TestWait::settle();
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
    void cleanup();
    void settingsPageShowsTheSubagentSetting();
    void hgsSettingsAreReadInTheBackground();
    void teamAttentionWatchNotifiesWithoutTheWindow();
    void aSessionRecordShowsTheUnconfirmableSubagentNote();
    void catalogModelAppliesKeptProposals();
    void taskCardOffersActionsByStatus();
    void attemptStatesShowPhaseIcons();
    void taskCardAnswersQuestionsAndHolds();
    void memberQueueListsCurrentAndReturned();
    void permissionAnswersFoldToOneLineAndOpenToFive();
    void planningEditsSendTheWholePlan();
    void newTeamStartsTheLeadWithTheTemplate();
    void newTeamOffersEachAgentsModels();
    void newTeamRowErrorsKeepAnOpenListOpen();
    void newTeamTabKeepsAMemberListOpenUntilItCloses();
    void newTeamMaxMembersStepsUpAndDown();
    void catalogEditorSavesTheNextVersion();
    void catalogEditorKeepsTheFieldsItDoesNotEdit();
    void ownerActionsCarryNoAgentMarkers();
    void permissionAttentionNamesTheMemberAndAnswers();
    void permissionAttentionFocusKeepsItsContext_data();
    void permissionAttentionFocusKeepsItsContext();
    void leadAnswersAMembersSubagentRequest();
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
    void aQueuedPhaseCanBeDiscarded();
    void aQueuedPhaseCanReturnToDraft();
    void lanesGroupACollapseInEveryView();
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
    void draftCreateSavesFirstAndShowsBlockersInRed();
    void templatesAreCompactWithSearchAndFavourites();
    void templateListHidesItsOldRowsAtOnce();
    void hideFinishedLeavesOutDecidedPhases();
    void dialogTemplateListAppliesOnlyWhenChosen();
    void designerSessionKeepsItsNameAndLeadsNoTeam();
    void nextGoalFromADoneTeam();
    void proposalsAreAcceptedOrRejectedOnce();
    void proposalCardsNameTheChangeAndAStaleCardSendsNothing();
    void planModeIsChosenWhenThePlanIsApproved();
    void switchToFreeOpensOneDialogBoundToTheRequest();
    void profileViewShowsThePhaseAndItsLosses();
    void profileReadsInBoundedPagesAndKeepsABoundedCache();
    void profileOfALargeTeamStaysResponsive();
    void freePhaseReadsIdleBuildsAndClosesOnConfirmation();
    void swapMemberAsksFirstAndListsTheOldSession();
    void memberMenuWorksBeforeTheStatusReadLands();
    void limitedAndSharedHostMembersAreMarked();
    void reportPreviewsAndFindings();
    void hierarchyInTeamViewsAndMemberCards();
    void workspaceAddsAMember();
    void graphRoutesEdgesAroundNodes();
    void graphEdgesAreEditedByHandleAndSelection();
    void graphHoverLightSurvivesPolls();
    void graphRoutingStaysFastForTheLargestPlan();
    void aTallLayerWrapsIntoColumns();
    void graphNodesInTwoLinesWithDraftEdits();
    void planEditKeepsTheRoadmap();
    void planEditsWaitForTheFullPlan();
    void catalogApplySaysUndecidedChangesAreDropped();
    void designerRedoAsksOnlyARunningDesigner();
    void boardPhaseHeaderKeepsItsActionsInView();
    void workAheadSwitchAndBadge();
    void graphNodesCarryTheActivityLabel();
    void cancelledWorkIsHiddenUntilShown();
    void thePlanHidesCancelledTasksAndNeverSendsTheCopy();
    void aTaskThatHasNotStartedMovesToAnotherPhase();
    void anAutoAcceptedDraftReadsAsDraftUntilItStarts();
    void workingTimeFollowsTheActivityLabel();
    void subLeadsAreATreeInMembersAndTheInspector();
    void aPollUnderTheMemberMenuKeepsItsButtonAndTheSwapDialog();
    void designerPageStartsOrOpensTheDesigner();
    void addedLateTaskIsOutOfScopeEverywhere();
    void planningRulesAreEditedSavedResetAndProposed();
    void draftIsDecidedWithoutADesignerSession();
    void designerQuestionIsAnsweredInTheWindow();
    void coordinatorQueuesASecondMessageWithCancel_data();
    void coordinatorQueuesASecondMessageWithCancel();
    void heldSentTakesEachLostMessageOnce();
    void coordinatorRestoresAHeldMessageThatWasNotDelivered();
    void coordinatorTakesPastedImagesAndFiles();
    void terminalDraftIsShownAndSentFromTheConversation();
    void conversationAccountPickerConfirmsAndSwitches();
    void accountPickerStatesInShots();
    void moveToAnotherAccountNamesTheMembersAndAsksOnce();
    void memberBlocksShowTheirReasonAndReviewAction();
    void catalogEditorMarksManualEdits();
    void teamCustomizationIsOptIn();
    void catalogRoleModelsAndContextAreSaved();
    void planTaskTakesModelsAndARole();
    void teamSettingsSendOnlyWhatChanged();
    void teamSettingsAllowLandingAndEditMemberModels();
    void memberMenuScalesAndDrains();
    void catalogPipelinesAreEditedAndSaved();
    void catalogPipelineProposalsHavePlates();
    void catalogPipelineTryWalksASampleTask();
    void boardScrollsInsteadOfSqueezingColumns();
    void planningBoardScrollsInsteadOfSqueezingColumns();
    void catalogRoleEditorEditsEveryField();
    void aPromptOrderSavesTheTaskTemplateItShows();
    void catalogRoleListGrowsToItsRows();
    void aboutNamesTheForkAndItsUpdate();
    void catalogTemplateEditorRosterSkeletonAndChecks();
    void catalogRoutingEditsAndTriesAGoal();
    void catalogHeaderRevertDraftAndBanners();
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
    void aNarrowToolbarKeepsOnlyItsIcons();
    void theNarrowToolbarSettlesOnOneStep();
    void phaseScrollDecisions();
    void sideListsAndMemberTasks();
    void waitingPageListsEveryKindAcrossTeams();
    void waitingBannerFoldsOpensAndHides();
    void waitingPagePagesStayStableWhileItemsArrive();
    void waitingPageEmptyAndRefused();
    void waitingItemsAreAnsweredInPlace();
    void waitingToolOffersAreAnsweredInPlace();
    void waitingBannerAnswersInPlaceAndWithdrawsTheNotice();
    void stageColumnsAndChipsOnTheBoard();
    void inspectorShowsThePipelineAndOverridesAskForAReason();
    void commandStagesShowTheirCommandExitAndTail();
    void catalogRoleDeleteNamesThePresetsItStrips();
    void stageOverridesReachHgsWithTheirReason();
    void planEditorPicksATasksPipeline();
    void heatTintsGraphAndBoardByTimeOrTokens();
    void aHeatReadThatDrawsTheSameRebuildsNothing();
    void teamDialogsDrawTheThemeNotTheSystem();
    void heldByFromTheStatusOnTheBoard();
    void aPollThatChangesNothingDrawnRebuildsNothing();
    void aPhaseGraphThatFitsShowsNoSidewaysBar();
    void aMembersLiveStateChangesInPlace();
    void aListPollThatMovesOnlyTheRevisionChangesNothing();
    void aWaitingReadThatChangesNothingRedrawsNothing();
private:
    QString script() const { return m_dir.filePath("hgs"); }
    void setTeam(const QJsonObject &value) { QVERIFY(FakeCli::replace(script() + ".status", QJsonDocument(value).toJson(QJsonDocument::Compact))); }
    QTemporaryDir m_dir;
    std::unique_ptr<TestWait::ModalGuard> m_modals;
};

void TestTeamsUi::initTestCase()
{
    WorkspaceSkin::preload();
    QVERIFY(m_dir.isValid());
    // #525: a pending Activity render is work in flight for the waits. A hidden
    // view stays dirty until it is shown, so only a visible one counts.
    TestWait::addProbe([] {
        for (auto *widget : QApplication::allWidgets())
            if (auto *activity = qobject_cast<ActivityView *>(widget); activity && activity->isVisible() && activity->renderPending()) return true;
        return false;
    });

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
elif a[:2]==['team','profile']:
 # U38: Heat (#666) keeps the pages by metric in `.profile`, a cursor naming
 # its page; Profile (#665) keeps one answer there, or `.compare` for a compare.
 import os
 if os.path.exists(__file__+'.profilerefuse'):print(open(__file__+'.profilerefuse').read(),file=sys.stderr);sys.exit(1)
 name=__file__+('.compare' if 'compare' in body else '.profile')
 data=json.load(open(name)) if os.path.exists(name) else {'summary':{}}
 if 'compare' not in body and isinstance(data.get('time'),list) and isinstance(data.get('tokens'),list):
  pages=data['tokens' if body.get('tokens') else 'time']
  c=body.get('after_tokens' if body.get('tokens') else 'after_time')
  data=pages[c['page'] if c else 0]
 print(json.dumps(data))
elif a[:2]==['team','waiting']:
 # U50: pages `.waiting` as hgs does: newest first, ties by id, after the cursor.
 import os
 if os.path.exists(__file__+'.waitingrefuse'):print(open(__file__+'.waitingrefuse').read(),file=sys.stderr);sys.exit(1)
 items=json.load(open(__file__+'.waiting')) if os.path.exists(__file__+'.waiting') else []
 items.sort(key=lambda i:(-i['since'],i['id']))
 c=body.get('before');start=0
 if c:start=next((n for n,i in enumerate(items) if i['since']<c['since'] or (i['since']==c['since'] and i['id']>c['id'])),len(items))
 page=items[start:start+min(body.get('limit',50),200)]
 nxt={'since':page[-1]['since'],'id':page[-1]['id']} if page and start+len(page)<len(items) else None
 print(json.dumps({'items':page,'next':nxt,'total':len(items)}))
elif a[:3]==['team','catalog','get']:sys.stdout.write(open(__file__+'.catalog').read())
elif a[:2]==['team','add-to-phase']:
 # U29: the first call warns and keeps the addition pending; the second adds it.
 if body.get('confirm'):print(json.dumps({'status':'added','phase':body.get('phase'),'tasks':[7]}))
 else:print(json.dumps({'status':'warning','phase':body.get('phase'),'pending':'pending-1','tasks':[7],
  'warning':'Adding tasks to running phase %d changes its scope after you approved it. The load balance of the lanes shifts and members may sit idle: members do not work ahead and later phases are not approved.'%body.get('phase')}))
elif a[:3]==['team','catalog','decide'] or a[:3]==['team','catalog','apply']:
 # U15: decisions and Apply change the draft as hgs does; `.refuse` refuses one.
 import os
 if a[2]=='decide' and os.path.exists(__file__+'.refuse'):print(open(__file__+'.refuse').read(),file=sys.stderr);sys.exit(1)
 if a[2]=='apply' and os.path.exists(__file__+'.refuseapply'):print(open(__file__+'.refuseapply').read(),file=sys.stderr);sys.exit(1)
 c=json.load(open(__file__+'.catalog'));draft=c['draft']
 if a[2]=='decide':
  for p in draft['proposals']:
   if p['id']==body.get('proposal'):p['state']={'keep':'kept','reject':'rejected','undo':'proposed'}[body.get('decision')]
  out={'status':'decided','proposal':body.get('proposal')}
 else:
  kept=[p for p in draft['proposals'] if p['state']=='kept']
  for p in kept:
   roles=[r for r in c['catalog']['roles'] if r['id']!=p['target']]
   c['catalog']['roles']=roles+([p['value']] if p['action']!='remove' else [])
  c['catalog']['version']+=1;c['draft']={'base':0,'proposals':[]}
  out={'status':'applied','version':c['catalog']['version'],'changes':len(kept)}
 open(__file__+'.catalog.new','w').write(json.dumps(c))
 import time
 for _ in range(100):
  try:os.replace(__file__+'.catalog.new',__file__+'.catalog');break
  except PermissionError:time.sleep(.05)   # Windows: a read holds the file
 print(json.dumps(out))
elif a[:2]==['team','spans']:
 # U38: `.spans` in pages after `after`, `limit` at a time.
 import os
 tl=json.load(open(__file__+'.spans')) if os.path.exists(__file__+'.spans') else []
 rest=[t for t in tl if t['task']>body.get('after',0)];page=rest[:body.get('limit',50)]
 print(json.dumps({'team':body.get('team'),'tasks':page,'next_cursor':page[-1]['task'] if len(rest)>len(page) else None,'events':{'read':0,'truncated':False}}))
elif a[:2]==['team','decide-proposal']:
 # U57: the receipt names the stored decision; `.proposalrefuse` refuses it,
 # `.proposalstatus` names an earlier decision (a replay).
 import os
 if os.path.exists(__file__+'.proposalrefuse'):print(open(__file__+'.proposalrefuse').read(),file=sys.stderr);sys.exit(1)
 status=open(__file__+'.proposalstatus').read() if os.path.exists(__file__+'.proposalstatus') else ('accepted' if body.get('accept') else 'rejected')
 print(json.dumps({'status':status,'proposal':body.get('proposal')}))
elif a[:2]==['team','approve-phase'] and body.get('phase')==9:
 print('hgs: Phase 9 is not waiting for approval',file=sys.stderr);sys.exit(1)
elif a[:2]==['team','intake-key']:
 # U51: a new key is in the receipt once; a revoke names the id.
 if body.get('revoke'):print(json.dumps({'status':'revoked','id':body['revoke']}))
 else:print(json.dumps({'status':'created','id':'i2','label':body.get('label'),'key':'zik_0123456789abcdef'}))
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
elif a[:2]==['team','swap-member'] and body.get('from_account'):
 # U54: a bulk move waits while `.swaphold` exists, then answers `.swaprefuse` or `.swapreceipt`.
 import os,time
 for _ in range(300):
  if not os.path.exists(__file__+'.swaphold'):break
  time.sleep(.1)
 if os.path.exists(__file__+'.swaprefuse'):print(open(__file__+'.swaprefuse').read(),file=sys.stderr);sys.exit(1)
 sys.stdout.write(open(__file__+'.swapreceipt').read())
elif a[:2]==['account','ls'] and os.path.exists(__file__+'.accounts'):sys.stdout.write(open(__file__+'.accounts').read())
elif a[:2]==['account','ls']:print(json.dumps({'profiles':[{'id':'native-claude','provider':'claude','label':'Default','account_status':{'status':'ok'}},
 {'id':'work','provider':'claude','label':'Claude work','account_status':{'status':'signed_out'}}]}))
elif a[:2]==['account','model-settings']:
 # #738: `.models` holds the catalogs by account when a test sets them.
 import os
 models=json.load(open(__file__+'.models')) if os.path.exists(__file__+'.models') else {'native-claude':[{'id':'opus'},{'id':'sonnet'},{'id':'haiku'}],
         'native-kimi':[{'id':'kimi-code/kimi-for-coding'},{'id':'kimi-code/k3.1-pro'}]}
 print(json.dumps({'account_id':a[2],'models':models.get(a[2],[])}))
else:print(json.dumps({'status':'ok','version':4,'request_id':body.get('request_id')}))
)PY"));
}

// #525: a dialog left open fails the test instead of hanging the suite.
void TestTeamsUi::cleanup()
{
    const QString closed = m_modals ? m_modals->closed() : QString();
    m_modals.reset();
    if (!closed.isEmpty()) QFAIL(qPrintable(QStringLiteral("the dialog %1 stayed open unanswered and was closed").arg(closed)));
}

void TestTeamsUi::init()
{
    m_modals = std::make_unique<TestWait::ModalGuard>();
    QFile::remove(script() + ".calls");
    QFile::remove(script() + ".draftrev");
    QFile::remove(script() + ".statushold");
    for (const char *suffix : {".swaphold", ".swaprefuse", ".swapreceipt", ".accounts", ".models"}) QFile::remove(script() + QLatin1String(suffix));
    QFile::remove(script() + ".refuse");
    QFile::remove(script() + ".refuseapply");
    QFile::remove(script() + ".proposalrefuse");
    for (const char *suffix : {".profile", ".profilerefuse", ".spans", ".compare"}) QFile::remove(script() + QLatin1String(suffix));
    QFile::remove(script() + ".proposalstatus");
    QFile::remove(script() + ".waiting");
    QFile::remove(script() + ".waitingrefuse");
    // Decisions and Apply change the fake catalog: each test starts from the fixture.
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(catalog()).toJson(QJsonDocument::Compact)));
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseGraph"));
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
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
                    if (w->objectName() == "teamBoardColumn")
                        return w->property("column").toString();
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
    shot(&workspace, "phases-7-drafts-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    WAIT_SETTLED();
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

// A row error flips a property on its own frame instead of setting the row's
// style sheet: that re-styled every combo of the row, popup included, and the
// Windows 11 style closed an open list (the member's role) under the pointer.
void TestTeamsUi::newTeamRowErrorsKeepAnOpenListOpen()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}, {"msi-old", "msi-old", false}},
                         {}, QDir::toNativeSeparators(folder.path()), "Checkout");
    dialog.show();
    QVERIFY(chooseTemplate(&dialog, "feature"));
    QTRY_COMPARE(dialog.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    const auto rows = dialog.findChildren<QFrame *>("teamNewMemberRow");
    auto *role = rows.first()->findChild<QComboBox *>("teamNewRole");
    auto *computer = rows.last()->findChild<QComboBox *>("teamNewComputer");
    QVERIFY(role && computer);
    role->showPopup();
    QTRY_VERIFY(role->view()->isVisible());
    // validate runs while the list is open: another row gets an error, the
    // name changes, the role's own text changes.
    computer->setCurrentIndex(computer->findData("msi-old"));
    dialog.findChild<QLineEdit *>("teamNewName")->setText("Checkout redesign");
    role->setEditText(role->currentText() + QStringLiteral(" "));
    QVERIFY(role->view()->isVisible());
    QVERIFY(rows.last()->property("error").toBool());
    QVERIFY(!rows.first()->property("error").toBool());
    for (auto *row : rows) QVERIFY(row->styleSheet().isEmpty());
    role->hidePopup();
    computer->setCurrentIndex(computer->findData(QString()));
    QVERIFY(!rows.last()->property("error").toBool());
}

// New team's tab rebuilds its member rows when the policy, the draft or an
// account reply changes them; a rebuild while a member's list is open waits
// until the list closes instead of deleting the combo under the pointer.
void TestTeamsUi::newTeamTabKeepsAMemberListOpenUntilItCloses()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    QVERIFY(QDir(folder.path()).mkdir(".git"));   // worktrees are offered
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}},
                   TeamNewTab::Coordinator{"claude/checkout/coord", "claude", "Opus", "", QDir::toNativeSeparators(folder.path()), "msi", "Ready", false}, "Checkout");
    tab.resize(1300, 860); tab.show();
    QVERIFY(chooseTemplate(&tab, "feature"));
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    QVERIFY(tab.findChild<QCheckBox *>("teamNewWorktrees")->isChecked());
    QPointer<QComboBox> role = tab.findChildren<QComboBox *>("teamNewRole").first();
    role->showPopup();
    QTRY_VERIFY(role->view()->isVisible());
    // Shared folders turn every member's worktree off: the rows are rebuilt.
    auto *policy = tab.findChild<QComboBox *>("teamNewFolderPolicy");
    policy->setCurrentIndex(policy->findData("shared"));
    QTest::qWait(50);
    QVERIFY(role);
    QVERIFY(role->view()->isVisible());
    role->hidePopup();
    QTRY_VERIFY(!role);
    QTRY_COMPARE(tab.findChildren<QFrame *>("teamNewMemberRow").size(), 2);
    for (auto *worktree : tab.findChildren<QCheckBox *>("teamNewWorktree")) QVERIFY(!worktree->isChecked());
}

// The team style sheet gives the spin box a border; its buttons must come
// from the style sheet too. Windows 11 places them side by side, and the
// editor, sized for one stacked button, covered the up button: a click there
// started editing the number instead of stepping it.
void TestTeamsUi::newTeamMaxMembersStepsUpAndDown()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    // Outlives the tab: the spin box uses it until it is destroyed.
    std::unique_ptr<QStyle> windows11(QStyleFactory::create("windows11"));
    TeamNewTab tab(&controller, &client, {{QString(), "this computer", true}},
                   TeamNewTab::Coordinator{"claude/checkout/coord", "claude", "Opus", "", QDir::toNativeSeparators(folder.path()), "msi", "Ready", false}, "Checkout");
    auto *max = tab.findChild<QSpinBox *>("teamNewMaxMembers");
    QVERIFY(max);
    if (windows11) max->setStyle(windows11.get());
    tab.resize(1300, 860); tab.show();
    QVERIFY(QTest::qWaitForWindowExposed(&tab));
    QTRY_VERIFY(max->isVisible());
    QStyleOptionSpinBox option;
    option.initFrom(max);
    option.frame = max->hasFrame();
    option.buttonSymbols = max->buttonSymbols();
    option.stepEnabled = QAbstractSpinBox::StepUpEnabled | QAbstractSpinBox::StepDownEnabled;
    option.subControls = QStyle::SC_SpinBoxFrame | QStyle::SC_SpinBoxEditField | QStyle::SC_SpinBoxUp | QStyle::SC_SpinBoxDown;
    const QRect up = max->style()->subControlRect(QStyle::CC_SpinBox, &option, QStyle::SC_SpinBoxUp, max);
    const QRect down = max->style()->subControlRect(QStyle::CC_SpinBox, &option, QStyle::SC_SpinBoxDown, max);
    auto *editor = max->findChild<QLineEdit *>();
    QVERIFY(editor && up.isValid() && down.isValid());
    QVERIFY2(!up.intersects(editor->geometry()), qPrintable(QStringLiteral("up %1,%2 %3x%4 under the editor").arg(up.x()).arg(up.y()).arg(up.width()).arg(up.height())));
    QVERIFY(!down.intersects(editor->geometry()));
    QCOMPARE(max->style()->hitTestComplexControl(QStyle::CC_SpinBox, &option, up.center(), max), QStyle::SC_SpinBoxUp);
    QCOMPARE(max->style()->hitTestComplexControl(QStyle::CC_SpinBox, &option, down.center(), max), QStyle::SC_SpinBoxDown);
    QCOMPARE(max->value(), 6);
    QTest::mouseClick(max, Qt::LeftButton, {}, up.center());
    QCOMPARE(max->value(), 7);
    QTest::mouseClick(max, Qt::LeftButton, {}, down.center());
    QCOMPARE(max->value(), 6);
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
    // The lead runs Claude: its accounts on this computer, its models, the
    // computers (U25: "+ computer" adds a chip; none is Any computer).
    auto *computer = editor.findChild<QComboBox *>("teamRoleComputer"); QVERIFY(computer);
    QCOMPARE(computer->count(), 4); QCOMPARE(computer->currentData().toString(), QString());
    QCOMPARE(computer->itemData(1).toString(), QStringLiteral("any"));
    QVERIFY(computer->itemText(3).contains("offline"));
    QVERIFY(editor.findChild<QLabel *>("teamRoleAnyComputer"));
    auto *model = editor.findChild<QComboBox *>("teamRoleModel"); QVERIFY(model && model->isEditable());
    QVERIFY(model->findText("opus") >= 0); QCOMPARE(model->currentText(), QString());
    QTRY_COMPARE(editor.findChild<QComboBox *>("teamRoleAccount")->count(), 3);
    auto *account = editor.findChild<QComboBox *>("teamRoleAccount");
    QVERIFY(account->itemText(account->findData("work")).contains("signed out"));
    account->setCurrentIndex(account->findData("native-claude")); emit account->activated(account->currentIndex());
    model->setCurrentIndex(model->findText("sonnet")); emit model->activated(model->currentIndex());
    computer->setCurrentIndex(2); emit computer->activated(2);
    // Accounts belong to a computer: another one clears the choice.
    auto *rebuilt = editor.findChild<QComboBox *>("teamRoleAccount");
    QTRY_VERIFY(rebuilt && rebuilt != account); QCOMPARE(rebuilt->currentData().toString(), QString());
    // The chip with ×, and the offline one struck through.
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleMachine").size(), 1);
    QVERIFY(!editor.findChild<QLabel *>("teamRoleAnyComputer"));
    auto *more = editor.findChild<QComboBox *>("teamRoleComputer");
    more->setCurrentIndex(3); emit more->activated(3);
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleMachine").size(), 2);
    for (auto *chip : editor.findChildren<QPushButton *>("teamRoleMachine"))
        QCOMPARE(chip->font().strikeOut(), chip->property("machine").toString() == QStringLiteral("laptop"));
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto lead = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray()[0].toObject();
    QCOMPARE(lead["model"].toString(), QStringLiteral("sonnet"));
    QCOMPARE(lead["computer"].toString(), QStringLiteral("devbox"));
    QCOMPARE(lead["machines"].toArray(), (QJsonArray{"devbox", "laptop"}));
    QCOMPARE(lead.value("account").toString(), QString());

    auto *agent = editor.findChild<QComboBox *>("teamRoleAgent");
    agent->setCurrentIndex(agent->findData("kimi")); emit agent->activated(agent->currentIndex());
    QTRY_VERIFY(editor.findChild<QComboBox *>("teamRoleModel")->findText("kimi-code/k3.1-pro") >= 0);
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseGraph"));
    // The coordinator shares the side column with the members, split by a divider.
    auto *side = workspace.findChild<QSplitter *>("teamWorkspaceSideSplit");
    QVERIFY(side && side->count() == 2);
    QVERIFY(side->widget(1)->findChild<QWidget *>("teamConversation"));
    // U25: nothing waits, so the right column has no Needs your answer list.
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceAnswers"));
    // Owner, 2026-10-29: a one-phase plan is drawn as its phase scroll too,
    // so the graph sits in that scroll under the phase header. A render
    // (every poll) builds the canvas again, so each check finds it anew.
    const auto graph = [&workspace] { return workspace.findChild<QWidget *>("teamPhaseGraph"); };
    QTRY_VERIFY(graph() && graph()->height() > 0 && workspace.findChild<QWidget *>("teamPhaseHeader"));
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
    // U35: the coordinator offers no quick-ask chips, in any team state or filter.
    QVERIFY(workspace.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceFilter")->findChildren<QPushButton *>())
        if (b->property("segment") == "active") { b->click(); break; }
    QTest::qWait(150);
    QVERIFY(workspace.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    // U25: the views carry their icons and their names.
    auto *views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *b : views->findChildren<QPushButton *>()) QVERIFY(!b->text().isEmpty() && !b->icon().isNull());
    for (auto *b : views->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    QCOMPARE(workspace.findChildren<QFrame *>("teamBoardCard").size(), 5);
    // The role and the state stay on the card at any width (a phase card has
    // the state on its first line, the role on its second). U33: a narrow
    // window no longer squeezes the card; the Board scrolls sideways instead.
    {
        // Reads the first card afresh: a resize may render the board again.
        const auto oneLine = [&workspace] {
            auto *boardCard = workspace.findChildren<QFrame *>("teamBoardCard").value(0);
            auto *pill = boardCard ? boardCard->findChild<QWidget *>("teamBoardRole") : nullptr;
            auto *mark = boardCard ? boardCard->findChild<QWidget *>("teamBoardState") : nullptr;
            return pill && mark && pill->isVisible() && mark->isVisible() ? 1 : pill && mark ? 0 : -1;
        };
        const QSize before = workspace.size();
        workspace.resize(700, before.height());
        QTRY_COMPARE(oneLine(), 1);
        QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceBoard")->horizontalScrollBar()->isVisible());
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
    // A phase card is itself the button.
    auto *boardCard = workspace.findChildren<QFrame *>("teamBoardCard").value(0);
    QVERIFY(boardCard);
    QTest::mouseClick(boardCard, Qt::LeftButton, {}, QPoint(6, 6));
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceDrawer") && workspace.findChild<QWidget *>("teamWorkspaceDrawer")->isVisible());
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
    WAIT_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    const auto testerMore = [&]() -> QToolButton * {
        for (auto *card : panel.findChildren<QFrame *>("teamCard"))
            if (card->property("teamMember") == "tester") return card->findChild<QToolButton *>("teamMemberMore");
        return nullptr;
    };
    WAIT_VERIFY(testerMore());
    QPointer<QToolButton> more = testerMore();
    QPointer<QMenu> menu = more->menu();
    auto changed = team("planning");
    changed["revision"] = changed["revision"].toInt() + 1;
    bool checked = false;
    std::function<void(int)> lookDialog = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *dialog = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!dialog) { if (tries > 0) lookDialog(TestWait::busy() ? tries : tries - 1); return; }
            QPointer<QInputDialog> open = dialog;
            // A failed check closes the dialog, so the test ends instead of waiting on it.
            const auto close = qScopeGuard([open] { if (open) open->reject(); });
            // The menu's own release comes due inside the dialog's loop: no render.
            WAIT_SETTLED();
            QVERIFY(panel.confirming());
            QVERIFY(more && menu);
            // A poll under the dialog: still no render.
            setTeam(changed);
            controller.refresh();
            WAIT_COMPARE(controller.team("team-1")->revision, changed["revision"].toInt());
            WAIT_SETTLED();
            QVERIFY(more && menu);
            QVERIFY(more == testerMore());
            QVERIFY(open && open->isVisible());
            checked = true;
        });
    };
    std::function<void(int)> lookMenu = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *popup = qobject_cast<QMenu *>(QApplication::activePopupWidget());
            if (!popup) { if (tries > 0) lookMenu(TestWait::busy() ? tries : tries - 1); return; }
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
    WAIT_VERIFY(checked);
    // Once everything has returned the panel catches up with the poll.
    WAIT_VERIFY(!panel.confirming());
    WAIT_VERIFY(testerMore() && testerMore() != more.data());
    QVERIFY(calls(script(), "plan").isEmpty());
}

void TestTeamsUi::memberEditorPicksAnExistingSession()
{
    setTeam(team("planning"));
    // Exercise the list-before-status ordering rather than racing the two
    // children: the first member menu captures a summary, not an editable plan.
    QFile hold(script() + ".statushold"); QVERIFY(hold.open(QIODevice::WriteOnly)); hold.close();
    const auto release = qScopeGuard([&] { QFile::remove(hold.fileName()); });
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
    QVERIFY(!controller.watched() || !controller.watched()->full);
    QVERIFY(calls(script(), "plan").isEmpty());
    QVERIFY(QFile::remove(hold.fileName()));
    WAIT_VERIFY(controller.watched() && controller.watched()->full);
    // The full status schedules a render. Its validation box proves the new
    // member menu captures that full plan before we take its action pointer.
    WAIT_VERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));
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
    // Follow the actual child request; a loaded runner can take longer than
    // QTRY's default while the plan write is still in flight.
    WAIT_COMPARE(calls(script(), "plan").size(), 1);
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
void TestTeamsUi::permissionAttentionFocusKeepsItsContext_data()
{
    QTest::addColumn<QString>("change");
    for (const auto &change : {"lost-focus", "another-input", "retry-same", "retry-input", "retry-team-switch",
                               "retry-question", "retry-hash", "retry-box-team"})
        QTest::newRow(change) << QString::fromLatin1(change);
}

void TestTeamsUi::permissionAttentionFocusKeepsItsContext()
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
    // Owner, 2026-10-28: the attention list is the first group of the side
    // column, not a strip over the canvas.
    QVERIFY(workspace.findChild<QWidget *>("teamAttentionStrip"));
    QVERIFY(workspace.findChild<QWidget *>("teamWorkspaceSide")->isAncestorOf(workspace.findChild<QWidget *>("teamAttentionStrip")));
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
    WAIT_VERIFY(workspace.findChildren<QFrame *>("teamPhaseScroll").size() > 2);
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
    WAIT_VERIFY(lightText());
    shot(&workspace, "d3-phases-draft-light");
    auto waiting = phasedTeam();
    auto tasks = waiting["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    waiting["tasks"] = tasks;
    waiting["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{3, 4}}}};
    waiting["pending_phase"] = QJsonValue();
    waiting["waiting_for_lead"] = true;
    setTeam(waiting);
    WAIT_VERIFY(([&] { controller.refresh(); return workspace.findChild<QPushButton *>("teamWorkspaceFinish") != nullptr; })());
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
    // The first team read took 6.6 s on a loaded computer.
    QTRY_VERIFY_WITH_TIMEOUT(workspace.findChild<QWidget *>("teamWorkspaceView"), 30000);
    const auto showBoard = [&workspace] {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    };
    showBoard();
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; the board
    // scrolls as a whole (U33), and a render keeps where it was.
    const auto pending = [&workspace]() -> QScrollBar * {
        auto *area = workspace.findChild<QScrollArea *>("teamWorkspaceBoard");
        return area ? area->verticalScrollBar() : nullptr;
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
elif a[:2]==['session-upload-block','set']:
    body=json.load(sys.stdin)
    open(__file__+'.calls','a').write(json.dumps(body)+'\n')
    print(json.dumps({'block':body.get('block')}))
)PY"));
    qputenv("FAKE_CONFIRM", "false");
    SettingsPage page(FakeCli::program(script));
    auto *box = page.findChild<QCheckBox *>("workspaceConfirmSubagents");
    QVERIFY(box);
    QTRY_VERIFY(box->isEnabled());
    QCOMPARE(box->isChecked(), false);
    box->click();
    QFile calls(script + ".calls");
    QTRY_VERIFY(calls.exists());
    QVERIFY(calls.open(QIODevice::ReadOnly));
    const auto sent = QJsonDocument::fromJson(calls.readAll().trimmed()).object();
    QCOMPARE(sent.value("confirm").toBool(), true);
    // C6: the set names the owner, or the CLI refuses it as an agent session.
    QCOMPARE(sent.value("by").toString(), QStringLiteral("human"));
    const auto id = sent.value("request_id").toString();
    QVERIFY2(!QUuid(id).isNull() && QUuid(id).toString(QUuid::WithoutBraces) == id, qPrintable(id));
    // The same page carries the DeepSeek upload block from #481, on by default.
    auto *upload = page.findChild<QCheckBox *>("workspaceSessionUploadBlock");
    QVERIFY(upload); QTRY_VERIFY(upload->isEnabled());
    QVERIFY(upload->isChecked());
    // U22: toggling it reads get (the checked state above) and sends set with
    // block false and a canonical request id.
    calls.close();
    QFile::remove(script + ".calls");
    upload->click();
    QTRY_VERIFY(QFile::exists(script + ".calls"));
    {
        QFile wrote(script + ".calls");
        QVERIFY(wrote.open(QIODevice::ReadOnly));
        const auto sent = QJsonDocument::fromJson(wrote.readAll().trimmed()).object();
        QCOMPARE(sent.value("block").toBool(true), false);
        QCOMPARE(sent.value("by").toString(), QStringLiteral("human"));
        const auto id = sent.value("request_id").toString();
        QVERIFY2(!QUuid(id).isNull() && QUuid(id).toString(QUuid::WithoutBraces) == id, qPrintable(id));
    }
    // Shots: setting on/off, light/dark (only with ZERUS_TEAM_SHOTS).
    for (bool enabled : {true, false}) {
        qputenv("FAKE_CONFIRM", enabled ? "true" : "false");
        lightPalette();
        SettingsPage page(FakeCli::program(script));
        page.resize(1200, 900);
        if (auto *nav = page.findChild<QListWidget *>("settingsSections")) nav->setCurrentRow(1);
        QTRY_VERIFY(page.findChild<QCheckBox *>("workspaceConfirmSubagents")->isEnabled() && page.findChild<QCheckBox *>("workspaceSessionUploadBlock")->isEnabled());
        const QString suffix = enabled ? "on" : "off";
        shot(&page, "u19-settings-subagents-" + suffix);
        darkPalette();
        shot(&page, "u19-settings-subagents-" + suffix + "-dark");
        lightPalette();
    }
    qputenv("FAKE_CONFIRM", "false");
    qunsetenv("FAKE_CONFIRM");
}

// N34 of #499: the settings hgs keeps are read and written in the background:
// a slow hgs never stalls opening Zerus, a failed change says so and the box
// returns to what hgs stores, and after each change the box shows what hgs reads.
void TestTeamsUi::hgsSettingsAreReadInTheBackground()
{
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString script = dir.filePath("fake-hgs");
    QVERIFY(FakeCli::install(script, R"PY(#!/usr/bin/env python3
import json,os,sys,time
a=sys.argv[1:]
store=__file__+'.'+(a[0] if a else '')
key={'subagents-confirm':'confirm','session-upload-block':'block'}.get(a[0] if a else '')
if key and a[1:2]==['get']:
    time.sleep(float(open(__file__+'.sleep').read()) if os.path.exists(__file__+'.sleep') else 0)
    print(json.dumps({key: open(store).read()=='true' if os.path.exists(store) else True}))
elif key and a[1:2]==['set']:
    body=json.load(sys.stdin)
    if os.path.exists(__file__+'.fail'):
        print('the settings store is read-only',file=sys.stderr); sys.exit(1)
    # hgs may store something else than asked (here: never false for upload).
    value=body.get(key) or key=='block'
    open(store,'w').write('true' if value else 'false')
    print(json.dumps({key: value}))
)PY"));
    QVERIFY(FakeCli::replace(script + ".subagents-confirm", "false"));
    QVERIFY(FakeCli::replace(script + ".sleep", "2"));
    QElapsedTimer opening; opening.start();
    SettingsPage page(FakeCli::program(script));
    // Two gets of 2 s each used to run here, one after the other.
    QVERIFY2(opening.elapsed() < 1500, qPrintable(QString::number(opening.elapsed())));
    auto *box = page.findChild<QCheckBox *>("workspaceConfirmSubagents"); QVERIFY(box);
    auto *note = page.findChild<QLabel *>("workspaceConfirmSubagentsError"); QVERIFY(note);
    auto *upload = page.findChild<QCheckBox *>("workspaceSessionUploadBlock"); QVERIFY(upload);
    QVERIFY(!box->isEnabled() && !upload->isEnabled());   // reading
    QTRY_VERIFY_WITH_TIMEOUT(box->isEnabled() && upload->isEnabled(), 10000);
    QVERIFY(!box->isChecked() && upload->isChecked()); QVERIFY(note->isHidden());
    QVERIFY(FakeCli::replace(script + ".sleep", "0"));
    // A failed change: the box returns to the stored value and says why.
    QVERIFY(FakeCli::replace(script + ".fail", "1"));
    box->click(); QVERIFY(box->isChecked() && !box->isEnabled());
    QTRY_VERIFY_WITH_TIMEOUT(box->isEnabled(), 10000);
    QVERIFY(!box->isChecked());
    QVERIFY(!note->isHidden());
    QCOMPARE(note->text(), QString("Could not change this setting: the settings store is read-only"));
    // A change that works clears the note.
    QVERIFY(QFile::remove(script + ".fail"));
    box->click(); QTRY_VERIFY_WITH_TIMEOUT(box->isEnabled(), 10000);
    QVERIFY(box->isChecked()); QVERIFY(note->isHidden());
    // The box shows what hgs then reads, not what was clicked.
    upload->click(); QTRY_VERIFY_WITH_TIMEOUT(upload->isEnabled(), 10000);
    QVERIFY(upload->isChecked());
    // An hgs without the command: the default stays and the note says so.
    QVERIFY(FakeCli::replace(script + ".subagents-confirm", "true"));
    QTemporaryDir older; QVERIFY(older.isValid());
    const QString old = older.filePath("old-hgs");
    QVERIFY(FakeCli::install(old, "#!/usr/bin/env python3\nimport sys\nprint('unknown command',file=sys.stderr)\nsys.exit(2)\n"));
    SettingsPage oldPage(FakeCli::program(old));
    auto *oldBox = oldPage.findChild<QCheckBox *>("workspaceConfirmSubagents");
    QTRY_VERIFY_WITH_TIMEOUT(oldBox->isEnabled(), 10000);
    QVERIFY(oldBox->isChecked());
    QCOMPARE(oldPage.findChild<QLabel *>("workspaceConfirmSubagentsError")->text(), QString("Could not read this setting from hgs: unknown command"));
    // A page closed while hgs still answers: the late answer reaches nothing.
    QVERIFY(FakeCli::replace(script + ".sleep", "1"));
    { SettingsPage closing(FakeCli::program(script)); }
    QTest::qWait(2500);
}

// N32 of #499: Zerus started in the tray has no Sessions window, yet a team's
// stuck permission still reaches the notifications, once; the known items are
// shared with the window through teams/attentionNotified.
void TestTeamsUi::teamAttentionWatchNotifiesWithoutTheWindow()
{
    const auto restore = qScopeGuard([] { QSettings().remove("teams/attentionNotified"); });
    QSettings().remove("teams/attentionNotified");
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
    QCOMPARE(QSettings().value("teams/attentionNotified").toStringList(), QStringList{token});
    // Later polls: the same item, no second notice.
    const int before = lists(); QTRY_VERIFY_WITH_TIMEOUT(lists() >= before + 2, 10000);
    QCOMPARE(raised.size(), 1);
    // The window took over and answered it: the watch stops; restarted, it
    // starts from what the window stored and raises nothing old.
    watch.setActive(false); const int stopped = lists(); QTest::qWait(600); QVERIFY(lists() <= stopped + 1);
    QSettings().setValue("teams/attentionNotified", QStringList{token});
    TeamAttentionWatch again(&client, nullptr, 200);
    QSignalSpy raisedAgain(&again, &TeamAttentionWatch::raised), clearedAgain(&again, &TeamAttentionWatch::cleared);
    again.setActive(true);
    QTRY_VERIFY_WITH_TIMEOUT(lists() >= stopped + 3, 10000);
    QCOMPARE(raisedAgain.size(), 0);
    // Answered: the notice is withdrawn.
    team.remove("attention"); QVERIFY(list(team));
    QTRY_COMPARE_WITH_TIMEOUT(clearedAgain.size(), 1, 10000);
    QCOMPARE(clearedAgain[0][0].toString(), token);
    QVERIFY(QSettings().value("teams/attentionNotified").toStringList().isEmpty());
}

void TestTeamsUi::aSessionRecordShowsTheUnconfirmableSubagentNote()
{
    const QJsonArray rows{ QJsonObject{{"name", "codex/x/dev"}, {"cmd", "codex"},
        {"subagent_confirm", "subagent confirmation not supported by codex"}} };
    const auto parsed = HgsClient::parseSessions(rows);
    QCOMPARE(parsed.size(), 1);
    QCOMPARE(parsed.first().subagentConfirm, QStringLiteral("subagent confirmation not supported by codex"));
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
}

// U50: the rail page lists every kind of item that waits for the owner, from
// several teams, newest first, each with its kind's icon and label and its
// team; a row (or its Open team) opens that team. Shots in both themes.
// U55: the banner: one waiting item is one line with its icon, title, team,
// age and action; several fold into one line with a count that opens; it
// hides when nothing waits; the feed reads only while active and notifies
// once per item. Shots in both themes.
void TestTeamsUi::waitingBannerFoldsOpensAndHides()
{
    QSettings().setValue("workspace/skin", "whitewh");
    QSettings().remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); QSettings().remove(TeamWaitingNotices::kSetting); darkPalette(); });
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
    QCOMPARE(actions, (QStringList{"Approve…", "Approve…", "Answer", "Answer", "Accept…"}));
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

void TestTeamsUi::waitingPageListsEveryKindAcrossTeams()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
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
    // too (Approve… for the drafts, Answer for the proposal and the permission).
    QStringList answers;
    for (auto *row : page.findChildren<QFrame *>("teamWaitingRow")) {
        QCOMPARE(row->findChild<QPushButton *>("teamWaitingOpen")->text(), QStringLiteral("Open team"));
        const auto *approve = row->findChild<QPushButton *>("teamWaitingApprove");
        const auto *answer = row->findChild<QPushButton *>("teamWaitingAnswerToggle");
        answers << (approve ? approve->text() : answer ? answer->text() : QString());
    }
    QCOMPARE(answers, (QStringList{"Approve…", "Approve…", "Answer", "Answer", "", "", ""}));
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

// U50: 300 items arrive in pages of 50 with Show older; a poll while new items
// arrive puts them on top, keeps every shown item in its order, each once, and
// Show older goes on after them.
void TestTeamsUi::waitingPagePagesStayStableWhileItemsArrive()
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
void TestTeamsUi::waitingPageEmptyAndRefused()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
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

// Owner, 2026-10-29: an unsaved edit never blocks Create team; it is saved
// first and the team is created after it. A real blocker is red in its field.
void TestTeamsUi::draftCreateSavesFirstAndShowsBlockersInRed()
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
    auto *name = tab.findChild<QLineEdit *>("teamNewName");
    QTRY_COMPARE(name->text(), QStringLiteral("Checkout redesign"));
    auto *create = tab.findChild<QPushButton *>("teamNewCreate");
    QTRY_VERIFY(create->isEnabled());
    // A blocker: the empty goal is red in its own field and named on the button.
    auto *goal = tab.findChild<QPlainTextEdit *>("teamNewGoal");
    QVERIFY(goal);
    const QString goalText = goal->toPlainText();
    goal->setPlainText(QString());
    QTRY_VERIFY(!create->isEnabled());
    QVERIFY(goal->styleSheet().contains("border"));
    goal->setPlainText(goalText);
    QTRY_VERIFY(create->isEnabled());
    QVERIFY(goal->styleSheet().isEmpty());
    QTRY_VERIFY(tab.pendingFields().isEmpty());
    // An edit not yet saved: Create stays enabled, saves it, then creates.
    const int before = calls(script(), "draft").size();
    tab.findChild<QSpinBox *>("teamNewMaxMembers")->setValue(7);
    QVERIFY(!tab.pendingFields().isEmpty());
    QVERIFY(create->isEnabled());
    create->click();
    QTRY_VERIFY(calls(script(), "draft").size() >= before + 2);
    const auto sent = calls(script(), "draft").mid(before);
    QCOMPARE(sent.at(0)["action"].toString(), QStringLiteral("update"));
    QCOMPARE(sent.at(0)["set"].toObject()["settings"].toObject()["max_members"].toInt(), 7);
    QCOMPARE(sent.last()["action"].toString(), QStringLiteral("create"));
}

// Owner, 2026-10-30 (Debian 13, KDE): New team showed the empty list's "No
// template" row over the first template. A refill hides the old rows at once,
// before their deferred delete, which a dialog's own event loop postpones.
void TestTeamsUi::templateListHidesItsOldRowsAtOnce()
{
    const QJsonArray roster{QJsonObject{{"role", "backend"}, {"count", 1}}};
    auto catalogObject = catalog()["catalog"].toObject();
    catalogObject["templates"] = QJsonArray{
        QJsonObject{{"id", "bugfix"}, {"name", "Bugfix"}, {"description", "One bug, one fix"}, {"roster", roster}},
        QJsonObject{{"id", "feature"}, {"name", "Feature delivery"}, {"description", "Full feature"}, {"roster", roster}}};
    const auto parsed = Teams::parseCatalog(QJsonObject{{"catalog", catalogObject}});
    TeamTemplateList list;
    list.resize(380, 320); list.show();
    QCOMPARE(list.visibleIds(), QStringList{"-"});
    // No event is processed in between, as inside a dialog's exec().
    list.setCatalog(parsed);
    QStringList shown;
    for (auto *row : list.findChildren<QWidget *>("teamTemplateRow")) if (row->isVisibleTo(&list)) shown << row->property("template").toString();
    shown.sort();
    QCOMPARE(shown, (QStringList{"-", "bugfix", "feature"}));
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

// Owner, 2026-10-28: a layer is not always one column. Phase 39's plan, 13
// tasks into one integration, then review and QA, then the build: the first
// layer wraps into 3 columns of at most 5, the next layers move right of
// them, and no edge crosses a node on its way.
void TestTeamsUi::aTallLayerWrapsIntoColumns()
{
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 13}, {1, 1}, {2, 2}, {3, 1}}).value(0).span, 3);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 13}, {1, 1}, {2, 2}, {3, 1}}).value(1).start, 3);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 6}, {1, 1}}).value(0).span, 1);
    QCOMPARE(TeamUi::TaskView::layerColumns({{0, 20}, {1, 20}}).value(0).span, 1);
    TeamUi::TaskView view(TeamUi::colorsFor(true), TeamUi::TaskView::Wide);
    const auto node = [](int id, int layer) {
        TeamUi::TaskView::Node n; n.id = id; n.layer = layer; n.title = QStringLiteral("Task %1").arg(id); n.role = "Backend";
        n.tone = TeamUi::colorsFor(true).muted; n.roleTone = TeamUi::roleColor(n.role, true); n.kind = "accepted"; n.agent = "claude";
        return n;
    };
    for (int id = 1; id <= 13; ++id) view.nodes.append(node(id, 0));
    view.nodes.append(node(14, 1)); view.nodes.append(node(15, 2)); view.nodes.append(node(16, 2)); view.nodes.append(node(17, 3));
    for (int id = 1; id <= 13; ++id) view.edges[14].append(id);
    view.edges[15] = {14}; view.edges[16] = {14}; view.edges[17] = {15, 16};
    view.area = QSize(1400, 700);
    view.layout();
    QSet<double> firstColumns;
    QMap<double, int> perColumn;
    for (int id = 1; id <= 13; ++id) { firstColumns.insert(view.nodeRect(id).left()); ++perColumn[view.nodeRect(id).left()]; }
    QCOMPARE(firstColumns.size(), 3);
    for (int count : perColumn) QVERIFY(count <= 5);
    double right = 0;
    for (int id = 1; id <= 13; ++id) right = qMax(right, view.nodeRect(id).right());
    QVERIFY(view.nodeRect(14).left() > right);
    QVERIFY(view.nodeRect(15).left() > view.nodeRect(14).right());
    for (auto it = view.edges.cbegin(); it != view.edges.cend(); ++it) for (int dep : it.value()) {
        QVERIFY(view.routed(dep, it.key()));
        const auto route = view.route(dep, it.key()).points;
        for (const auto &other : view.nodes) {
            if (other.id == dep || other.id == it.key()) continue;
            const QRectF box = view.nodeRect(other.id).adjusted(1, 1, -1, -1);
            for (int i = 1; i < route.size(); ++i)
                QVERIFY2(!QRectF(route.at(i - 1), route.at(i)).normalized().adjusted(-.01, -.01, .01, .01).intersects(box),
                         qPrintable(QStringLiteral("#%1 -> #%2 crosses #%3").arg(dep).arg(it.key()).arg(other.id)));
        }
    }
    view.resize(view.size()); shot(&view, "graph-wrapped-layer");
}

// #570 (owner): hovering a task lights its linked tasks; every poll rebuilt
// the phase graphs and the light went out under a resting pointer.
void TestTeamsUi::graphHoverLightSurvivesPolls()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1500, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1"); workspace.showTeam("team-1");
    // Every render replaces the phase graphs: always ask for the current one.
    const auto graph = [&] { return phaseGraphOf(&workspace, 2); };
    QTRY_VERIFY(graph() && graph()->nodeRect(3).width() > 0 && graph()->isVisible());
    const auto at = [&](const QPointF &point) { return QPoint(int(std::round(point.x() * graph()->zoom)), int(std::round(point.y() * graph()->zoom))); };
    // #4 is not linked to #3: it dims while #3 is lit.
    const auto pixels = [&](int task) {
        const QRectF r = graph()->nodeRect(task);
        return graph()->grab(QRect(at(r.topLeft()), at(r.bottomRight()))).toImage();
    };
    TestWait::settle();
    const QImage plain = pixels(4);
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    QVERIFY(graph()->chain(3).contains(5) && graph()->chain(3).contains(6) && !graph()->chain(3).contains(4));
    TestWait::settle();
    const QImage lit = pixels(4);
    QVERIFY(lit != plain);
    int revision = 21;
    const auto poll = [&](QJsonObject next) {
        QPointer<TeamUi::TaskView> old = graph();
        next["revision"] = ++revision;
        // #693: a poll that changes nothing drawn rebuilds nothing; this one
        // carries a change, so the views are rebuilt.
        next["description"] = QStringLiteral("poll %1").arg(revision);
        setTeam(next);
        controller.refresh();
        QTRY_VERIFY(!old && graph() && graph()->nodeRect(4).width() > 0);
        TestWait::settle();
    };
    // Several polls with new revisions; the pointer does not move.
    for (int i = 0; i < 3; ++i) {
        poll(graphTeam());
        QCOMPARE(graph()->hovered(), 3);
        QCOMPARE(pixels(4), lit);
        QVERIFY(graph()->toolTip().startsWith(QStringLiteral("#3 Card form UI")));
    }
    // Another phase's graph never takes the light.
    QCOMPARE(phaseGraphOf(&workspace, 3)->hovered(), 0);
    // The pointer moves to the gutter right of #3: the light goes, and a poll
    // keeps it off.
    const QRectF three = graph()->nodeRect(3);
    QTest::mouseMove(graph(), at(QPointF(three.right() + 12, three.center().y())));
    QTRY_COMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
    QCOMPARE(pixels(4), plain);
    // Back on #3, then the pointer leaves the view: off, also after a poll.
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    QEvent leave(QEvent::Leave);
    QCoreApplication::sendEvent(graph(), &leave);
    QCOMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
    // A task that is gone takes its light with it. (A move to where the
    // pointer already is sends nothing: go by the gutter.)
    QTest::mouseMove(graph(), at(QPointF(graph()->nodeRect(3).right() + 12, graph()->nodeRect(3).center().y())));
    QTest::mouseMove(graph(), at(graph()->nodeRect(3).center()));
    QTRY_COMPARE(graph()->hovered(), 3);
    auto gone = graphTeam();
    QJsonArray tasks;
    for (const auto &value : gone["tasks"].toArray()) {
        auto task = value.toObject();
        if (task["id"].toInt() == 3) continue;
        QJsonArray deps;
        for (const auto &d : task["deps"].toArray()) if (d.toObject()["task"].toInt() != 3) deps.append(d);
        task["deps"] = deps;
        tasks.append(task);
    }
    gone["tasks"] = tasks;
    auto phases = gone["phases"].toArray();
    auto second = phases[0].toObject(); second["tasks"] = QJsonArray{2, 4, 5, 6}; phases[0] = second;
    gone["phases"] = phases;
    poll(gone);
    QCOMPARE(graph()->hovered(), 0);
    poll(graphTeam());
    QCOMPARE(graph()->hovered(), 0);
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
    // #465 N18: a poll that changes the team rebuilds every view; the owner's
    // half-made link and selected edge come back in the new one.
    int revision = 21;
    const auto poll = [&] {
        QPointer<TeamUi::TaskView> old = draft;
        auto next = graphTeam(); next["revision"] = ++revision;
        // #693: a change, so the views are rebuilt.
        next["description"] = QStringLiteral("poll %1").arg(revision);
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

// U31: the owner's Work ahead switch in the Team tab's toolbar and the
// "worked ahead" mark on a task worked before its draft phase was approved,
// in one picture per theme.
void TestTeamsUi::workAheadSwitchAndBadge()
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
    // #693: at 1500 px the graph fits, so no bar (one used to flash on each
    // rebuild); the narrow window below shows it.
    QCOMPARE(sideways->horizontalScrollBar()->sizeHint().height(), 8);
    // Owner, 2026-10-28: the place survives the rebuild of a poll. A render
    // rebuilds the phase, so the area is looked up each time.
    {
        const auto area = [&]() -> QScrollArea * {
            auto *phase = phaseScrollOf(&workspace, 2);
            return phase ? qobject_cast<QScrollArea *>(phase->findChild<QWidget *>("teamPhaseGraphArea")) : nullptr;
        };
        // A narrow window, so the phase graph needs its sideways bar.
        workspace.resize(760, 900);
        QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0 && area()->horizontalScrollBar()->isVisible());
        const int place = qMin(120, area()->horizontalScrollBar()->maximum());
        area()->horizontalScrollBar()->setValue(place);
        QCOMPARE(area()->horizontalScrollBar()->value(), place);
        workspace.render();
        QTest::qWait(200);
        QTRY_COMPARE(area()->horizontalScrollBar()->value(), place);
        workspace.resize(1500, 900);
        QTRY_VERIFY(([&] { graph = phaseGraphOf(&workspace, 2); draft = phaseGraphOf(&workspace, 3); return graph && draft && graph->nodeRect(6).width() > 0; })());
        sideways = area();
    }
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

void TestTeamsUi::graphNodesCarryTheActivityLabel()
{
    setTeam(labelTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    // #9's member is offline: its problem text must win over the label.
    // Backend's agent runs; the lead's does not.
    const LiveLookup live = [](const Member &m) { Live l; l.known = true; if (m.id == "blocked") l.offline = true; l.working = m.id == "backend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    TeamUi::TaskView *graph = nullptr;
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); return graph && graph->nodeRect(1).width() > 0; })());
    // U58: cancelled #12 is hidden until Show cancelled brings it back.
    QVERIFY(std::none_of(graph->nodes.cbegin(), graph->nodes.cend(), [](const TeamUi::TaskView::Node &n) { return n.id == 12; }));
    controller.setShowCancelled(true);
    QTRY_VERIFY(([&] { graph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
                       return graph && std::any_of(graph->nodes.cbegin(), graph->nodes.cend(), [](const TeamUi::TaskView::Node &n) { return n.id == 12; }); })());
    const auto node = [&](int id) { for (const auto &n : graph->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    // Each rule of the owner's addition: Submitted, attempt 2, Held, a
    // sub-team, and (below) words for Pending, Draft, Accepted. Owner, 2026-10-28:
    // work in progress says where it is - working, starting or idle.
    QCOMPARE(node(1).activity, QStringLiteral("review"));
    QCOMPARE(node(2).activity, QStringLiteral("attempt 2 · working"));
    QCOMPARE(node(3).activity, QStringLiteral("working"));
    QCOMPARE(node(10).activity, QStringLiteral("starting"));
    QCOMPARE(node(11).activity, QStringLiteral("idle"));
    QCOMPARE(node(12).activity, QStringLiteral("cancelled"));
    QCOMPARE(node(4).activity, QStringLiteral("held"));
    QCOMPARE(node(5).activity, QStringLiteral("sub-team 1/2"));
    // Owner, 2026-10-29: every task says its state in words: a returned task
    // and what it waits for, a draft, an accepted one.
    QCOMPARE(node(6).activity, QStringLiteral("returned · waits for Backend"));
    QCOMPARE(node(7).activity, QStringLiteral("draft"));
    QCOMPARE(node(8).activity, QStringLiteral("accepted"));
    // A problem text wins; the label stays empty.
    QVERIFY(node(9).problem);
    QVERIFY(!node(9).problemText.isEmpty());
    QVERIFY(node(9).activity.isEmpty());
    shot(&workspace, "ux-5-graph-label-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "ux-5-graph-label-light");
    darkPalette(); workspace.setTheme(true);
    // The same label on Board cards, right of the role chip and model tile.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto card = [&](int id) {
        for (auto *frame : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (frame->property("task").toInt() == id) {
                auto *a = frame->findChild<QLabel *>("teamBoardActivity");
                return a ? a->text() : QString();
            }
        return QStringLiteral("?");
    };
    QCOMPARE(card(1), QStringLiteral("review"));
    QCOMPARE(card(2), QStringLiteral("attempt 2 · working"));
    QCOMPARE(card(3), QStringLiteral("working"));
    QCOMPARE(card(4), QStringLiteral("held"));
    QCOMPARE(card(6), QStringLiteral("returned · waits for Backend"));
    QVERIFY(card(9).isEmpty());
    // The same label on Layers rows: the list view carries it on its nodes.
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "layers") { b->click(); break; }
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; its Layers
    // rows carry the label.
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers") && !workspace.findChildren<QFrame *>("teamLayerRow").isEmpty());
    const auto layerLabel = [&](int id) {
        for (auto *row : workspace.findChildren<QFrame *>("teamLayerRow"))
            if (row->property("task").toInt() == id) {
                auto *a = row->findChild<QLabel *>("teamLayerActivity");
                return a ? a->text() : QString();
            }
        return QStringLiteral("?");
    };
    QCOMPARE(layerLabel(1), QStringLiteral("review"));
    QCOMPARE(layerLabel(4), QStringLiteral("held"));
}

// U37: the labels' team with each task's timing as hgs status computes it:
// #2 runs its second attempt, #3 its first, #8 is done (its decision was not
// recorded), #9 runs behind a problem, #11's turn ended. The clock is 9000.
static QJsonObject timingTeam()
{
    auto t = labelTeam();
    const auto span = [](int n, const char *state, double from, double to, int worked, int member, int lead) {
        return QJsonObject{{"n", n}, {"member", "backend"}, {"state", state}, {"from", from}, {"to", to > 0 ? QJsonValue(to) : QJsonValue()},
                           {"worked_s", worked}, {"member_s", member}, {"lead_s", lead}};
    };
    const auto timing = [](int worked, double since, int deps, int member, int lead, const QJsonArray &attempts, const QJsonArray &unknown = {}) {
        QJsonObject o{{"worked_s", worked}, {"running_since", since > 0 ? QJsonValue(since) : QJsonValue()},
                      {"waits", QJsonObject{{"deps_s", deps}, {"member_s", member}, {"lead_s", lead}}}, {"attempts", attempts}};
        if (!unknown.isEmpty()) o["unknown"] = unknown;
        return o;
    };
    const QHash<int, QJsonObject> timings{
        {1, timing(600, 0, 0, 60, 300, {span(1, "reported", 1000, 1600, 600, 60, 300)})},
        {2, timing(5400, 5100, 2400, 180, 600, {span(1, "rejected", 2000, 3500, 1500, 120, 600), span(2, "delivered", 5100, 0, 3900, 60, 0)})},
        {3, timing(720, 8280, 0, 30, 0, {span(1, "delivered", 8280, 0, 720, 30, 0)})},
        {4, timing(0, 0, 0, 0, 300, {})},
        {8, timing(4500, 0, 0, 0, 0, {span(1, "reported", 1000, 5500, 4500, 0, 0)}, {"lead"})},
        {9, timing(30, 8970, 0, 0, 0, {span(2, "delivered", 8970, 0, 30, 0, 0)})},
        {11, timing(100, 0, 0, 0, 0, {span(1, "delivered", 1800, 1900, 100, 0, 0)})},
    };
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (timings.contains(task["id"].toInt())) task["timing"] = timings.value(task["id"].toInt());
        tasks[i] = task;
    }
    t["tasks"] = tasks;
    return t;
}

// Owner, 2026-10-30: Hide finished leaves out a phase whose tasks are all
// decided, with its tasks, in Graph, Board and Layers, and is remembered.
void TestTeamsUi::hideFinishedLeavesOutDecidedPhases()
{
    QSettings().remove("teams/hideFinished");
    const auto team = Teams::parse(phasedTeam());
    QVERIFY(phaseFinished(team, 1) && !phaseFinished(team, 2) && !phaseFinished(team, 3));
    QCOMPARE(finishedPhases(team), 1);
    const auto shown = withoutFinished(team);
    QVERIFY(shown.cancelledHidden);
    QVERIFY(!shown.task(1) && !shown.task(2) && shown.task(3) && shown.task(4) && shown.task(5));
    setTeam(phasedTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished"));
    QTRY_VERIFY(phaseScrollOf(&workspace, 2));
    QVERIFY(!workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->isChecked());
    QVERIFY(phaseScrollOf(&workspace, 1));
    workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->click();
    QTRY_VERIFY(!phaseScrollOf(&workspace, 1));
    QVERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 3));
    QVERIFY(QSettings().value("teams/hideFinished").toBool());
    QTRY_VERIFY(workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->isChecked());
    shot(&workspace, "hide-finished-dark");
    workspace.findChild<QAbstractButton *>("teamWorkspaceHideFinished")->click();
    QTRY_VERIFY(phaseScrollOf(&workspace, 1));
    QSettings().remove("teams/hideFinished");
}

// U58: the phased team with a cancelled task in phase 2 and an approved phase 4
// whose two tasks were cancelled (copies the lead made under new ids).
static QJsonObject cancelledTeam()
{
    auto t = phasedTeam();
    auto phases = t["phases"].toArray();
    auto two = phases[0].toObject(); two["tasks"] = QJsonArray{3, 4, 6}; phases[0] = two;
    phases.append(QJsonObject{{"n", 4}, {"title", "Dropped copies"}, {"barrier", false}, {"approval", "review"}, {"state", "approved"},
                              {"tasks", QJsonArray{7, 8}}, {"added_by", "Lead"}, {"added_at", 1800.0}, {"decided_by", "You"}, {"decided_at", 1900.0}});
    t["phases"] = phases;
    auto tasks = t["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 6}, {"phase", 2}, {"title", "Old fix"}, {"assignee", "backend"}, {"status", "cancelled"}});
    tasks.append(QJsonObject{{"id", 7}, {"phase", 4}, {"title", "Copy of the review"}, {"assignee", "backend"}, {"status", "cancelled"}});
    tasks.append(QJsonObject{{"id", 8}, {"phase", 4}, {"title", "Copy of the tests"}, {"assignee", "tester"}, {"status", "cancelled"}});
    t["tasks"] = tasks;
    auto layers = t["layers"].toObject(); layers["6"] = 1; layers["7"] = 0; layers["8"] = 0; t["layers"] = layers;
    return t;
}

void TestTeamsUi::cancelledWorkIsHiddenUntilShown()
{
    setTeam(cancelledTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1300, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The reads ask hgs for the cancelled work; the desktop hides it itself.
    QTRY_VERIFY(!calls(script(), "status").isEmpty());
    QVERIFY(calls(script(), "status").last()["include_cancelled"].toBool());
    QVERIFY(calls(script(), "list").last()["include_cancelled"].toBool());
    QVERIFY(!controller.showCancelled());
    // Graph: #6 is gone from phase 2, phase 4 is not shown at all.
    QTRY_VERIFY(phaseNodeIds(&workspace).contains(4));
    QVERIFY(!phaseNodeIds(&workspace).contains(6));
    QVERIFY(phaseScrollOf(&workspace, 2) && phaseScrollOf(&workspace, 3));
    QVERIFY(!phaseScrollOf(&workspace, 4));
    auto *show = workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled");
    QVERIFY(show && show->isCheckable() && !show->isChecked());
    QCOMPARE(show->text(), QStringLiteral("Show cancelled (3)"));
    shot(&workspace, "u58-cancelled-hidden-dark");
    const auto view = [&](const QString &value) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == value) { b->click(); break; }
    };
    const auto ids = [&](const char *name) {
        QList<int> found;
        for (auto *frame : workspace.findChildren<QFrame *>(name)) found << frame->property("task").toInt();
        std::sort(found.begin(), found.end());
        return found;
    };
    // Board and Layers leave them out too, and so does a fully cancelled phase.
    view("board");
    QTRY_VERIFY(ids("teamBoardCard").contains(4));
    QVERIFY(!ids("teamBoardCard").contains(6) && !ids("teamBoardCard").contains(7) && !ids("teamBoardCard").contains(8));
    QVERIFY(!phaseScrollOf(&workspace, 4));
    QVERIFY(workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled"));
    view("layers");
    QTRY_VERIFY(ids("teamLayerRow").contains(4));
    QVERIFY(!ids("teamLayerRow").contains(6) && !phaseScrollOf(&workspace, 4));
    // Show cancelled brings back the task and the phase, which reads cancelled.
    workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled")->click();
    QVERIFY(controller.showCancelled());
    QTRY_VERIFY(ids("teamLayerRow").contains(6));
    QVERIFY(phaseScrollOf(&workspace, 4));
    QCOMPARE(phaseScrollOf(&workspace, 4)->property("state").toString(), QStringLiteral("cancelled"));
    view("board");
    QTRY_VERIFY(ids("teamBoardCard").contains(6));
    view("graph");
    QTRY_VERIFY(phaseNodeIds(&workspace).contains(6));
    QCOMPARE(phaseScrollOf(&workspace, 4)->property("state").toString(), QStringLiteral("cancelled"));
    show = workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled");
    QVERIFY(show->isChecked());
    QCOMPARE(show->text(), QStringLiteral("Hide cancelled (3)"));
    shot(&workspace, "u58-cancelled-shown-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u58-cancelled-shown-light");
    darkPalette(); workspace.setTheme(true);
    // And hides them again (the theme change rebuilt the toolbar).
    workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled")->click();
    QTRY_VERIFY(!phaseScrollOf(&workspace, 4));
    QVERIFY(!phaseNodeIds(&workspace).contains(6));
    workspace.setTheme(false);
    lightShot(&workspace, "u58-cancelled-hidden-light");
    darkPalette(); workspace.setTheme(true);
    // A team without cancelled work has no toggle and shows every task, as before.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); return !workspace.findChild<QPushButton *>("teamWorkspaceShowCancelled") && phaseNodeIds(&workspace).contains(4); })());
}

void TestTeamsUi::thePlanHidesCancelledTasksAndNeverSendsTheCopy()
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

void TestTeamsUi::aTaskThatHasNotStartedMovesToAnotherPhase()
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

void TestTeamsUi::anAutoAcceptedDraftReadsAsDraftUntilItStarts()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
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
    shot(&workspace, "u61-auto-draft-dark");
    workspace.setTheme(false); lightShot(&workspace, "u61-auto-draft-light");
    darkPalette(); workspace.setTheme(true);
    // The lead started it ahead: the header says so.
    setTeam(autoDraftTeam(true, false));
    QTRY_VERIFY(([&] { controller.refresh(); auto *frame = phaseScrollOf(&workspace, 3); return frame && frame->findChild<QLabel *>("teamPhaseStartAhead"); })());
    auto *ahead = phaseScrollOf(&workspace, 3)->findChild<QLabel *>("teamPhaseStartAhead");
    QCOMPARE(ahead->text(), QStringLiteral("started ahead by the lead"));
    QVERIFY(ahead->toolTip().startsWith(QStringLiteral("The lead started it ahead")));
    QVERIFY(badge(3)->toolTip().contains(QStringLiteral("Its reports need no approval from you")));
    QTRY_VERIFY(([&] { if (!controller.watched()) return false; const auto now = TeamUi::phaseBands(*controller.watched(), [](const PhaseGroup &) { return true; });
                       return std::any_of(now.cbegin(), now.cend(), [](const TeamUi::TaskView::PhaseBand &band) { return band.n == 3 && band.startAhead; }); })());
    shot(&workspace, "u61-started-ahead-dark");
    workspace.setTheme(false); lightShot(&workspace, "u61-started-ahead-light");
    darkPalette(); workspace.setTheme(true);
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
    shot(&workspace, "u61-accepted-by-auto-dark");
    workspace.setTheme(false); lightShot(&workspace, "u61-accepted-by-auto-light");
    darkPalette(); workspace.setTheme(true);
    // A review draft keeps today's words.
    setTeam(phasedTeam());
    QTRY_VERIFY(([&] { controller.refresh(); auto *b = dynamic_cast<TeamUi::PhaseBadge *>(badge(3)); return b && b->text() == QStringLiteral("Draft — waiting for you"); })());
    QVERIFY(!badge(3)->property("auto").toBool());
}

void TestTeamsUi::workingTimeFollowsTheActivityLabel()
{
    // The test owns the clock; the minute ticks come by hand.
    double now = 9000;
    TeamUi::workedClock() = [&now] { return now; };
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    const auto tick = [] { QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout")); };
    QCOMPARE(TeamUi::workedTicker().interval(), 60 * 1000);
    setTeam(timingTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    const LiveLookup live = [](const Member &m) { Live l; l.known = true; if (m.id == "blocked") l.offline = true; l.working = m.id == "backend"; return l; };
    panel.setLive(live); workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    // Wide enough that the shots show the done task's layer and column.
    workspace.resize(1900, 820); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    // The text itself: minutes, then hours with two-digit minutes.
    QCOMPARE(TeamUi::workedText(0), QStringLiteral("<1m"));
    QCOMPARE(TeamUi::workedText(12 * 60 + 59), QStringLiteral("12m"));
    QCOMPARE(TeamUi::workedText(3600 + 5 * 60), QStringLiteral("1h 05m"));
    QCOMPARE(TeamUi::workedText(26 * 3600 + 40 * 60), QStringLiteral("26h 40m"));
    const auto graph = [&] { return dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph")); };
    QTRY_VERIFY(graph() && graph()->nodeRect(1).width() > 0);
    const auto node = [&](TeamUi::TaskView *view, int id) { for (const auto &n : view->nodes) if (n.id == id) return n; return TeamUi::TaskView::Node{}; };
    // Running: since the current attempt started; done: the sum; never
    // worked: nothing. A problem text keeps the time beside it.
    QCOMPARE(node(graph(), 2).worked, QStringLiteral("1h 05m"));
    QCOMPARE(node(graph(), 2).activity, QStringLiteral("attempt 2 · working"));
    QCOMPARE(node(graph(), 3).worked, QStringLiteral("12m"));
    QCOMPARE(node(graph(), 1).worked, QStringLiteral("10m"));
    QCOMPARE(node(graph(), 8).worked, QStringLiteral("1h 15m"));
    QCOMPARE(node(graph(), 11).worked, QStringLiteral("1m"));
    QCOMPARE(node(graph(), 9).worked, QStringLiteral("<1m"));
    QVERIFY(node(graph(), 9).problem);
    QVERIFY(node(graph(), 4).worked.isEmpty());
    QVERIFY(node(graph(), 6).worked.isEmpty());
    QVERIFY(node(graph(), 7).worked.isEmpty());
    // The tooltip: each attempt with its waits, the task's waits named apart.
    const QString retried = node(graph(), 2).tooltip;
    QVERIFY2(retried.contains("Working 1h 05m, since "), qPrintable(retried));
    QVERIFY2(retried.contains("Attempt 1 (backend, rejected): worked 25m · waited 2m for a free member · 10m for the lead"), qPrintable(retried));
    QVERIFY2(retried.contains("Attempt 2 (backend): working since "), qPrintable(retried));
    QVERIFY2(retried.contains("Waited 40m for dependencies · 3m for a free member · 10m for the lead, not counted"), qPrintable(retried));
    QVERIFY2(node(graph(), 8).tooltip.contains("Worked 1h 15m\n"), qPrintable(node(graph(), 8).tooltip));
    QVERIFY(node(graph(), 8).tooltip.contains("Not recorded: the lead's decision"));
    QVERIFY(!node(graph(), 6).tooltip.contains("Worked"));
    const auto shoot = [&](const QString &name) {
        shot(&workspace, name + "-dark");
        lightPalette(); workspace.setTheme(false); workspace.render(); QTest::qWait(200);
        lightShot(&workspace, name + "-light");
        darkPalette(); workspace.setTheme(true); workspace.render(); QTest::qWait(150);
    };
    shoot("ux-u37-graph");
    // One tick a minute: the view keeps its nodes and only the running times
    // change.
    QPointer<TeamUi::TaskView> ticked = graph();
    now = 12000;
    tick();
    QVERIFY(ticked);
    QCOMPARE(node(ticked, 3).worked, QStringLiteral("1h 02m"));
    QCOMPARE(node(ticked, 2).worked, QStringLiteral("1h 55m"));
    QCOMPARE(node(ticked, 9).worked, QStringLiteral("50m"));
    QCOMPARE(node(ticked, 8).worked, QStringLiteral("1h 15m"));
    QCOMPARE(node(ticked, 1).worked, QStringLiteral("10m"));
    now = 9000; tick();
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
            if (b->property("segment") == segment) { b->click(); break; }
    };
    // The Board card: right after the activity label, in its color and size.
    view("board");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    const auto card = [&](int id) -> QFrame * {
        for (auto *frame : workspace.findChildren<QFrame *>("teamBoardCard")) if (frame->property("task").toInt() == id) return frame;
        return nullptr;
    };
    const auto worked = [&](int id) -> QLabel * { auto *frame = card(id); return frame ? frame->findChild<QLabel *>("teamBoardWorked") : nullptr; };
    QTRY_VERIFY(worked(3));
    QCOMPARE(worked(3)->text(), QStringLiteral("12m"));
    QCOMPARE(worked(2)->text(), QStringLiteral("1h 05m"));
    QCOMPARE(worked(8)->text(), QStringLiteral("1h 15m"));
    QVERIFY(!worked(6));
    auto *activity = card(3)->findChild<QLabel *>("teamBoardActivity");
    QCOMPARE(activity->text(), QStringLiteral("working"));
    QCOMPARE(worked(3)->font().pixelSize(), activity->font().pixelSize());
    QCOMPARE(worked(3)->palette().color(QPalette::WindowText), activity->palette().color(QPalette::WindowText));
    QVERIFY(worked(3)->geometry().left() > activity->geometry().right());
    QVERIFY(worked(2)->toolTip().contains("Attempt 1 (backend, rejected): worked 25m"));
    // A running time keeps the room it grows to: "12m" becoming "1h 02m"
    // moves nothing after it.
    QVERIFY(worked(3)->minimumWidth() >= worked(3)->fontMetrics().horizontalAdvance(QStringLiteral("1h 02m")));
    auto *state = card(3)->findChild<QWidget *>("teamBoardState");
    TestWait::settle();
    const int stateLeft = state->geometry().left();
    QPointer<QLabel> label = worked(3);
    now = 12000;
    tick();
    QVERIFY(label);
    QCOMPARE(label->text(), QStringLiteral("1h 02m"));
    QCOMPARE(worked(8)->text(), QStringLiteral("1h 15m"));
    TestWait::settle();
    QCOMPARE(state->geometry().left(), stateLeft);
    now = 9000; tick();
    shoot("ux-u37-board");
    // The Layers row: the same text after the activity label (the phase
    // scroll's rows since the one-phase plan has its scroll too).
    view("layers");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers") && !workspace.findChildren<QFrame *>("teamLayerRow").isEmpty());
    const auto layer = [&](int id) -> QLabel * {
        for (auto *row : workspace.findChildren<QFrame *>("teamLayerRow"))
            if (row->property("task").toInt() == id) return row->findChild<QLabel *>("teamLayerWorked");
        return nullptr;
    };
    QTRY_VERIFY(layer(2));
    QCOMPARE(layer(2)->text(), QStringLiteral("1h 05m"));
    QCOMPARE(layer(8)->text(), QStringLiteral("1h 15m"));
    QCOMPARE(layer(11)->text(), QStringLiteral("1m"));
    QVERIFY(layer(2)->toolTip().contains("Attempt 1 (backend, rejected): worked 25m"));
    now = 12000; tick();
    QCOMPARE(layer(3)->text(), QStringLiteral("1h 02m"));
    now = 9000; tick();
    shoot("ux-u37-layers");
    // An older hgs sends no timing: nothing is shown.
    const auto old = parse(labelTeam());
    QVERIFY(!old.task(2)->timing.known);
    QVERIFY(TeamUi::workedLabel(*old.task(2), now).isEmpty());
    QVERIFY(TeamUi::workedTip(*old.task(2), now).isEmpty());
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
    WAIT_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    const auto visibleMore = [&](const QString &member) -> QToolButton * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == member && card->isVisibleTo(&workspace)) return card->findChild<QToolButton *>("teamWorkspaceMemberMenu");
        return nullptr;
    };
    WAIT_VERIFY(controller.watched() && visibleMore("backend"));
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
            if (!menu) { if (tries > 0) look(TestWait::busy() ? tries : tries - 1); return; }
            open = menu;
            QVERIFY(panel.confirming());
            controller.refresh();
            WAIT_SETTLED();
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
    WAIT_VERIFY(checked);
    QPointer<QDialog> dialog;
    WAIT_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwapDialog")) && dialog->isVisible());
    // The views catch up once the menu has closed: the column is rebuilt
    // with the new team, so the old button is not the one shown any more.
    WAIT_VERIFY(!panel.confirming());
    WAIT_VERIFY(visibleMore("backend") && visibleMore("backend") != more.data());
    // A poll while the swap dialog is open leaves it open.
    changed["revision"] = 14;
    setTeam(changed);
    controller.refresh();
    WAIT_COMPARE(controller.team("team-1")->revision, 14);
    WAIT_SETTLED();
    QVERIFY(dialog && dialog->isVisible());
    dialog->close();
}

// U25 part 9 (spec 11.1): the Team designer's chat. Before the first message
// the intro and the composer stand in the middle (no example chips); the
// field grows from 2 to 8 lines; the first message starts the designer with
// the agent, model and effort of the model menu, and a failed start keeps the
// text. Running: the proposals and the DRAFT bar right above the composer,
// Show in catalog, Apply from the bar, decided cards close and come back.
void TestTeamsUi::designerPageStartsOrOpensTheDesigner()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    QString running; QString startedIn, startedGoal; QString opened;
    page.designerSession = [&] { return running; };
    QJsonObject startedAgent; int starts = 0;
    page.startDesigner = [&](const QString &folder, const QString &goal, const QJsonObject &agent) { startedIn = folder; startedGoal = goal; startedAgent = agent; ++starts; };
    page.openDesigner = [&](const QString &name) { opened = name; };
    page.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    page.setContext("/work/checkout", "checkout");
    page.resize(1440, 900); page.show();
    WAIT_VERIFY(controller.catalogReady());   // the page reads the catalog while shown
    // Not started: the intro and the composer in the middle, no example chips.
    auto *intro = page.findChild<QWidget *>("teamDesignerEmpty"); QVERIFY(intro && intro->isVisible());
    QVERIFY(page.findChildren<QPushButton *>("teamDesignerExample").isEmpty());
    QVERIFY(!page.findChild<QPushButton *>("teamDesignerOpen")->isVisible());
    QVERIFY(page.findChild<QLabel *>("teamDesignerReads")->text().contains("checkout"));
    auto *chat = page.findChild<QWidget *>("teamConversation");
    auto *composer = page.findChild<QWidget *>("teamDesignerComposer");
    auto *card = page.findChild<QFrame *>("teamDesignerComposerCard"); QVERIFY(card && card->isVisible());
    const auto gaps = [&] {
        const int top = intro->mapTo(chat, QPoint(0, 0)).y();
        const int bottom = chat->height() - composer->mapTo(chat, QPoint(0, composer->height())).y();
        return std::pair{top, bottom};
    };
    WAIT_VERIFY(gaps().first > 100 && qAbs(gaps().first - gaps().second) <= 24);
    // The session's row: attach, the context and the model with its effort.
    auto *model = page.findChild<QPushButton *>("teamDesignerModel"); QVERIFY(model && model->isVisible());
    QVERIFY2(model->text().startsWith("opus high"), qPrintable(model->text()));
    QVERIFY(page.findChild<QPushButton *>("teamConversationAttach")->isVisible());
    QVERIFY(page.findChild<QWidget *>("teamDesignerComposerRow")->isVisible());
    auto *input = page.findChild<QPlainTextEdit *>("teamConversationInput");
    auto *send = page.findChild<QPushButton *>("teamConversationSend");
    QVERIFY(input->isEnabled() && !send->property("primary").toBool());
    // The field grows with its text from 2 to 8 lines, then scrolls.
    const int two = input->height();
    input->setPlainText("A security reviewer\nfor payments\nand\na quick team");
    WAIT_VERIFY(input->height() > two);
    input->setPlainText(QStringLiteral("line\n").repeated(12));
    WAIT_COMPARE(input->verticalScrollBarPolicy(), Qt::ScrollBarAsNeeded);
    const int eight = input->height();
    input->setPlainText(QStringLiteral("line\n").repeated(20)); WAIT_SETTLED();
    QCOMPARE(input->height(), eight);
    input->setPlainText("Payments code needs a security look. And I want a quick team for small bugs.");
    WAIT_VERIFY(send->property("primary").toBool());
    WAIT_COMPARE(input->height(), two);
    shot(&page, "ux-9-designer-start-dark");
    page.setDark(false); lightShot(&page, "ux-9-designer-start-light"); darkPalette(); page.setDark(true);
    // Another agent and model from the menu.
    model->click();
    QMenu *menu = nullptr;
    WAIT_VERIFY((menu = page.findChild<QMenu *>("teamDesignerModelMenu")));
    for (auto *action : menu->actions()) if (action->text() == "gpt-5.5-codex") { action->trigger(); break; }
    menu->close();
    WAIT_VERIFY(model->text().startsWith("gpt-5.5-codex high"));
    send->click();
    QCOMPARE(starts, 1);
    QCOMPARE(startedIn, QStringLiteral("/work/checkout"));
    QCOMPARE(startedGoal, QStringLiteral("Payments code needs a security look. And I want a quick team for small bugs."));
    QCOMPARE(startedAgent["agent"].toString(), QStringLiteral("codex"));
    QCOMPARE(startedAgent["model"].toString(), QStringLiteral("gpt-5.5-codex"));
    QCOMPARE(startedAgent["effort"].toString(), QStringLiteral("high"));
    QVERIFY(!send->isEnabled()); QCOMPARE(send->text(), QStringLiteral("Starting…"));
    send->click(); QCOMPARE(starts, 1);   // once
    // A failed start keeps the text and says why.
    page.startFailed("codex is not signed in");
    auto *status = page.findChild<QLabel *>("teamConversationStatus");
    WAIT_VERIFY(status->isVisible()); QVERIFY(status->text().contains("codex is not signed in"));
    QVERIFY(send->isEnabled()); QVERIFY(input->toPlainText().startsWith("Payments code"));
    send->click(); QCOMPARE(starts, 2);   // sent again; the session's arrival clears it
    // Running: its conversation, the proposals and the draft over the catalog.
    running = "claude/checkout/team-designer";
    page.refresh();
    WAIT_VERIFY(!intro->isVisible());
    QCOMPARE(input->toPlainText(), QString());
    QVERIFY(!model->isEnabled());
    WAIT_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 2);
    auto *bar = page.findChild<QFrame *>("teamDesignerDraftBar"); QVERIFY(bar->isVisible());
    QVERIFY(page.findChild<QLabel *>("teamDesignerDraftText")->text().contains("based on v3 · 1 of 2 kept · 1 undecided"));
    // The DRAFT bar stands right above the composer.
    WAIT_VERIFY(bar->mapTo(chat, QPoint(0, bar->height())).y() <= card->mapTo(chat, QPoint(0, 0)).y());
    QVERIFY(card->mapTo(chat, QPoint(0, 0)).y() - bar->mapTo(chat, QPoint(0, bar->height())).y() < 40);
    // A short conversation stays centred with the composer; a long one fills
    // the column and the composer keeps to the bottom.
    const auto events = [](int count) {
        QJsonArray list;
        for (int i = 1; i <= count; ++i)
            list.append(QJsonObject{{"seq", i}, {"at", 1791018000 + i}, {"type", i % 2 ? "UserPromptSubmit" : "Stop"},
                                    {"detail", QStringLiteral("Message %1 about the security reviewer and a quick team for small bugs.").arg(i)}});
        return list;
    };
    const auto feed = [&](int count) {
        emit client.inspectionReady(QString(), running, QJsonObject{{"run_id", "r1"}, {"conversation_id", "c1"}, {"events", events(count)},
                                                                    {"cursor", count}, {"replace_events", true}}, QString());
    };
    const auto below = [&] { return chat->height() - composer->mapTo(chat, QPoint(0, composer->height())).y(); };
    WAIT_VERIFY(([&] { feed(2); return below() > 60; })());
    WAIT_VERIFY2(([&] { feed(30); return below() < 12; })(), qPrintable(QStringLiteral("below %1, chat %2, view %3").arg(below()).arg(chat->height()).arg(chat->findChild<QWidget *>("teamConversationActivity")->height())));
    shot(&page, "ux-9-designer-working-dark");
    QCOMPARE(page.findChild<QLabel *>("teamCatalogTitle")->text(), QStringLiteral("Draft catalog"));
    QVERIFY(page.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 of 2 kept"));
    page.findChildren<QPushButton *>("teamProposalShow").first()->click();   // p1: the Tester role
    auto *roles = page.findChild<QListWidget *>("teamCatalogRoles");
    QCOMPARE(roles->currentItem()->data(Qt::UserRole).toString(), QStringLiteral("tester"));
    QCOMPARE(roles->count(), 4);   // + the proposed Security role
    QVERIFY(page.findChild<QFrame *>("teamProposalPlate"));
    shot(&page, "ux-9-designer-draft-dark");
    page.setDark(false); lightShot(&page, "ux-9-designer-draft-light"); darkPalette(); page.setDark(true);
    page.findChild<QPushButton *>("teamDesignerOpen")->click(); QCOMPARE(opened, running);
    QVERIFY(page.findChild<QPushButton *>("teamDesignerApply")->isEnabled());
    // A decided card closes and comes back; Apply stands in the DRAFT bar.
    QCOMPARE(page.findChildren<QPushButton *>("teamProposalClose").size(), 1);   // p2 is kept
    page.findChild<QPushButton *>("teamProposalClose")->click();
    WAIT_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 1);
    page.findChild<QPushButton *>("teamDesignerShowClosed")->click();
    WAIT_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 2);
    auto *apply = page.findChild<QPushButton *>("teamDesignerApplyKept");
    QVERIFY(apply && apply->isVisible() && apply->isEnabled()); QCOMPARE(apply->text(), QStringLiteral("✓ Apply 1 change"));
    apply->click();
    WAIT_COMPARE(calls(script(), "catalog apply").size(), 1);
    page.findChild<QPushButton *>("teamDesignerDraftDiscard")->click();
    WAIT_COMPARE(calls(script(), "catalog discard").size(), 1);
}

// U25 part 9 (spec 11.3): every field of a role is edited in place: name,
// about, the palette's 16 colors and 10 icons, skills as chips (Enter adds, ×
// and Backspace remove), another agent with its first model, computers,
// the five permissions, folder, who it may message, when it joins, the five
// dangerous actions and the prompt blocks with include and up/down.
// Owner, 2026-10-28: the role list runs down the whole column; it scrolled
// after five rows (a Maximum policy held it at the default size hint) with
// the column empty below "+ Role".
// Owner, 2026-10-28: Settings > About names the fork as the active repository
// beside upstream, and offers the update from the fork's releases. Outside a
// supported installation (this test) the button stays off and says why.
void TestTeamsUi::aboutNamesTheForkAndItsUpdate()
{
    SettingsPage page(FakeCli::program(script()));
    auto *upstream = page.findChild<QPushButton *>("aboutRepository");
    auto *fork = page.findChild<QPushButton *>("aboutForkRepository");
    QVERIFY(upstream && fork);
    QCOMPARE(upstream->text(), QStringLiteral("ufna/zerus"));
    QCOMPARE(fork->text(), QStringLiteral("WhiteWh/zerus"));
    QVERIFY(fork->isChecked());
    QVERIFY(fork->toolTip().contains(QStringLiteral("https://github.com/WhiteWh/zerus")));
    auto *update = page.findChild<QPushButton *>("aboutForkUpdate");
    QVERIFY(update);
    QCOMPARE(update->text(), QStringLiteral("Check for updates from WhiteWh/zerus"));
    QCOMPARE(update->isEnabled(), ForkUpdater::instance()->manualAvailable());
    if (!update->isEnabled()) QVERIFY(update->toolTip().contains(QStringLiteral("portable Windows build")));
    QVERIFY(page.findChild<QLabel *>("aboutForkStatus"));
}

void TestTeamsUi::catalogRoleListGrowsToItsRows()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    // Settings > Teams: the editor sits in a scrolling page at its size hint.
    QScrollArea page; page.setWidgetResizable(true);
    auto *editorWidget = new TeamCatalogEditor(&controller);
    auto &editor = *editorWidget;
    page.setWidget(editorWidget);
    page.resize(1180, 900); page.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    auto *add = editor.findChild<QPushButton *>("teamCatalogAdd");
    for (int i = 0; i < 11; ++i) add->click();
    QTRY_COMPARE(roles->count(), 14);
    int rows = 0;
    for (int i = 0; i < roles->count(); ++i) rows += roles->visualItemRect(roles->item(i)).height();
    QTRY_VERIFY2(roles->viewport()->height() >= rows, qPrintable(QStringLiteral("%1 px for %2 px of rows").arg(roles->viewport()->height()).arg(rows)));
    QCOMPARE(roles->verticalScrollBar()->maximum(), 0);
    // "+ Role" stays right under the last row.
    QVERIFY(add->geometry().top() - roles->geometry().bottom() < 16);
    shot(&page, "catalog-role-list-14-dark");
}

void TestTeamsUi::catalogRoleEditorEditsEveryField()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.projectRules = [] { return QStringLiteral("1. Use pnpm for web and cargo for services.\n2. Never log card numbers."); };
    editor.setSources(&client, {{"devbox", "devbox", true}});
    editor.resize(1180, 1500); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    roles->setCurrentRow(1);   // Backend
    QTRY_COMPARE(editor.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Backend"));
    const auto segment = [&](const char *name, const char *value) {
        for (auto *b : editor.findChild<QWidget *>(name)->findChildren<QPushButton *>()) if (b->property("segment") == value) { b->click(); return; }
        QFAIL(value);
    };
    auto *about = editor.findChild<QLineEdit *>("teamRoleAbout"); about->setText("Server code"); emit about->editingFinished();
    // Icon / color: the palette's 16 colors and its 10 icons.
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleSwatch").size(), 16);
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleIcon").size(), 10);
    for (auto *b : editor.findChildren<QPushButton *>("teamRoleSwatch")) if (b->property("swatch") == "security") { b->click(); break; }
    QTRY_VERIFY(editor.findChildren<QPushButton *>("teamRoleIcon").size() == 10);
    for (auto *b : editor.findChildren<QPushButton *>("teamRoleIcon")) if (b->property("roleIcon") == "backend") { b->click(); break; }
    QTRY_VERIFY(std::any_of(editor.findChildren<QPushButton *>("teamRoleIcon").cbegin(), editor.findChildren<QPushButton *>("teamRoleIcon").cend(),
                            [](QPushButton *b) { return b->property("roleIcon") == "backend" && b->isChecked(); }));
    // Specialization: Enter adds a chip, Backspace in the empty field drops the last, × any.
    for (const char *skill : {"Rust", "Postgres", "REST"}) {
        auto *field = editor.findChild<QLineEdit *>("teamRoleTags"); QVERIFY(field);
        field->setText(skill); emit field->returnPressed();
    }
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleTag").size(), 3);
    QTest::keyClick(editor.findChild<QLineEdit *>("teamRoleTags"), Qt::Key_Backspace);
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleTag").size(), 2);
    editor.findChildren<QPushButton *>("teamRoleTag").first()->click();   // Rust
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleTag").size(), 1);
    // Default agent: another agent takes its first model and the default account.
    auto *agent = editor.findChild<QComboBox *>("teamRoleAgent");
    agent->setCurrentIndex(agent->findData("claude")); emit agent->activated(agent->currentIndex());
    QTRY_COMPARE(editor.findChild<QComboBox *>("teamRoleModel")->currentText(), QStringLiteral("opus"));
    QCOMPARE(editor.findChild<QComboBox *>("teamRoleAccount")->currentData().toString(), QString());
    // Permissions: five; Bypass says what it does.
    auto *permissions = editor.findChild<QComboBox *>("teamRolePermissions");
    QCOMPARE(permissions->count(), 5);
    permissions->setCurrentIndex(permissions->findData("bypass")); emit permissions->activated(permissions->currentIndex());
    QTRY_VERIFY(editor.findChild<QLabel *>("teamRoleBypassNote"));
    QCOMPARE(editor.findChild<QLabel *>("teamRoleBypassNote")->text(), QStringLiteral("Runs every tool without asking"));
    permissions = editor.findChild<QComboBox *>("teamRolePermissions");
    permissions->setCurrentIndex(permissions->findData("read_only")); emit permissions->activated(permissions->currentIndex());
    QTRY_VERIFY(!editor.findChild<QLabel *>("teamRoleBypassNote"));
    segment("teamRoleFolder", "shared");
    // Can message: the lead and selected roles, chips with ×.
    segment("teamRoleMessage", "some");
    QTRY_VERIFY(editor.findChild<QComboBox *>("teamRoleMessageAdd"));
    auto *to = editor.findChild<QComboBox *>("teamRoleMessageAdd");
    to->setCurrentIndex(to->findData("tester")); emit to->activated(to->currentIndex());
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleMessageTo").size(), 1);
    auto *joins = editor.findChild<QLineEdit *>("teamRoleJoins"); joins->setText("services/payments/** **/auth/**"); emit joins->editingFinished();
    // Dangerous actions: five switches and their count.
    QCOMPARE(editor.findChildren<QCheckBox *>("teamRoleDanger").size(), 5);
    QVERIFY(editor.findChild<QLabel *>("teamRoleDangerSummary")->text().startsWith("0 of 5 allowed"));
    for (auto *check : editor.findChildren<QCheckBox *>("teamRoleDanger")) if (check->property("danger") == "pkgs") { check->click(); break; }
    QTRY_VERIFY(editor.findChild<QLabel *>("teamRoleDangerSummary")->text().startsWith("1 of 5 allowed"));
    // Prompt order: project rules, the role's prompt, the team protocol, the task.
    const auto blocks = [&] { QStringList keys; for (auto *b : editor.findChildren<QFrame *>("teamPromptBlock")) keys << b->property("block").toString(); return keys; };
    const auto block = [&](const QString &key) -> QFrame * { for (auto *b : editor.findChildren<QFrame *>("teamPromptBlock")) if (b->property("block") == key) return b; return nullptr; };
    QCOMPARE(blocks(), (QStringList{"rules", "role", "protocol", "task"}));
    QVERIFY(editor.findChild<QLabel *>("teamPromptRules")->text().contains("Use pnpm"));
    QCOMPARE(editor.findChild<QPlainTextEdit *>("teamRolePrompt")->toPlainText(), QStringLiteral("Build the API."));
    QVERIFY(!editor.findChild<QPlainTextEdit *>("teamRoleTaskTemplate")->toPlainText().isEmpty());
    editor.findChild<QPlainTextEdit *>("teamRoleProtocol")->setPlainText("Report each endpoint you change.");
    editor.findChild<QPlainTextEdit *>("teamRoleTaskTemplate")->setPlainText("#{id} {title}\n{description}\nAcceptance:\n{criteria}");
    block("rules")->findChild<QCheckBox *>("teamPromptInclude")->click();
    QTRY_VERIFY(block("rules") && !block("rules")->findChild<QCheckBox *>("teamPromptInclude")->isChecked());
    QVERIFY(block("role")->findChild<QToolButton *>("teamPromptUp")->isEnabled());
    QVERIFY(!block("rules")->findChild<QToolButton *>("teamPromptUp")->isEnabled());
    block("task")->findChild<QToolButton *>("teamPromptUp")->click();
    QTRY_COMPARE(blocks(), (QStringList{"rules", "role", "task", "protocol"}));
    editor.findChild<QPushButton *>("teamPromptAddBlock")->click();
    QTRY_COMPARE(blocks().size(), 5);
    editor.findChild<QPlainTextEdit *>("teamPromptNote")->setPlainText("Ask before a migration.");
    // Used in: the template that seats it.
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleUsedIn").size(), 1);
    QVERIFY(editor.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 unsaved change"));
    shot(&editor, "ux-9-role-editor-dark");
    editor.setDark(false); lightShot(&editor, "ux-9-role-editor-light"); darkPalette(); editor.setDark(true);
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE_WITH_TIMEOUT(calls(script(), "catalog set").size(), 1, 20000);
    const auto backend = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray()[1].toObject();
    QCOMPARE(backend["about"].toString(), QStringLiteral("Server code"));
    QCOMPARE(backend["color"].toString(), QStringLiteral("security"));
    QCOMPARE(backend["icon"].toString(), QStringLiteral("r_backend"));
    QCOMPARE(backend["skills"].toArray(), (QJsonArray{"Postgres"}));
    QCOMPARE(backend["tags"].toArray(), (QJsonArray{"Postgres"}));
    QCOMPARE(backend["agent"].toString(), QStringLiteral("claude"));
    QCOMPARE(backend["model"].toString(), QStringLiteral("opus"));
    QCOMPARE(backend["permissions"].toString(), QString());
    QCOMPARE(backend["permissions_profile"].toString(), QStringLiteral("read"));
    QCOMPARE(backend["folder"].toString(), QStringLiteral("shared"));
    QCOMPARE(backend["message"].toString(), QStringLiteral("some"));
    QCOMPARE(backend["can_message"].toString(), QStringLiteral("lead"));
    QCOMPARE(backend["message_to"].toArray(), (QJsonArray{"tester"}));
    QCOMPARE(backend["joins"].toString(), QStringLiteral("services/payments/** **/auth/**"));
    const auto danger = backend["danger"].toObject();
    QVERIFY(danger["pkgs"].toBool()); QVERIFY(!danger["push"].toBool()); QCOMPARE(danger.size(), 5);
    QVERIFY(backend["dangerous"].toBool());
    QCOMPARE(backend["prompt_order"].toArray(), (QJsonArray{"rules", "role", "task", "protocol", "note1"}));
    QCOMPARE(backend["prompt_off"].toArray(), (QJsonArray{"rules"}));
    QCOMPARE(backend["notes"].toObject()["note1"].toString(), QStringLiteral("Ask before a migration."));
    QCOMPARE(backend["protocol_extra"].toString(), QStringLiteral("Report each endpoint you change."));
    QCOMPARE(backend["task_template"].toString(), QStringLiteral("#{id} {title}\n{description}\nAcceptance:\n{criteria}"));
    // The Used in chip opens its template.
    QTRY_VERIFY(editor.findChild<QPushButton *>("teamRoleUsedIn"));
    editor.findChild<QPushButton *>("teamRoleUsedIn")->click();
    QTRY_VERIFY(editor.findChild<QWidget *>("teamTemplateEditor"));
    QCOMPARE(editor.findChild<QLineEdit *>("teamTemplateName")->text(), QStringLiteral("Feature delivery"));
}

// U25 part 9 (spec 11.4): a template's roster counts with "only if needed",
// work mode with its warning, the task skeleton with phases, planning,
// approval, new phases, max members, the checks, and the folding blocks.
void TestTeamsUi::catalogTemplateEditorRosterSkeletonAndChecks()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto keys = {CollapsibleBlock::settingsKey("catalog-roster"), CollapsibleBlock::settingsKey("catalog-skeleton"), QStringLiteral("ui/catalogHideUnusedRoles")};
    for (const auto &key : keys) QSettings().remove(key);
    const auto restore = qScopeGuard([keys] { QSettings().remove("workspace/skin"); for (const auto &key : keys) QSettings().remove(key); darkPalette(); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1180, 1100); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 3);
    const auto segment = [&](const char *name, const char *value) {
        for (auto *b : editor.findChild<QWidget *>(name)->findChildren<QPushButton *>()) if (b->property("segment") == value) { b->click(); return; }
        QFAIL(value);
    };
    segment("teamCatalogTabs", "templates");
    auto *templates = editor.findChild<QListWidget *>("teamCatalogTemplates");
    QTRY_COMPARE(templates->count(), 1);
    templates->setCurrentRow(0);
    QTRY_VERIFY(editor.findChild<QWidget *>("teamTemplateEditor"));
    // The list: name, then members · tasks · work.
    QCOMPARE(templates->itemWidget(templates->item(0))->findChild<QLabel *>("teamTemplateMeta")->text(), QStringLiteral("2 members · 3 tasks · worktree per member"));
    QVERIFY(editor.findChild<QLabel *>("teamTemplateUsedBy")->text().contains("manual pick only"));
    const auto stepper = [&](const QString &role) -> QWidget * {
        for (auto *w : editor.findChildren<QWidget *>("teamTemplateSeatCount")) if (w->property("role") == role) return w;
        return nullptr;
    };
    const auto value = [](QWidget *w) { return w->findChild<QLabel *>("teamStepperValue")->text().toInt(); };
    const auto issues = [&] { QStringList all; for (auto *l : editor.findChildren<QLabel *>("teamTemplateIssue")) all << l->text(); return all.join('\n'); };
    // Roster: the lead, then a count and "only if needed" for every other role.
    QCOMPARE(editor.findChildren<QWidget *>("teamTemplateSeatCount").size(), 2);
    QCOMPARE(value(stepper("backend")), 1);
    stepper("backend")->findChild<QPushButton *>("teamStepperPlus")->click();
    QTRY_COMPARE(value(stepper("backend")), 2);
    for (auto *check : editor.findChildren<QCheckBox *>("teamTemplateSeatOptional")) if (check->property("role") == "tester") { check->click(); break; }
    QTRY_COMPARE(editor.findChild<QLabel *>("teamTemplateRosterSummary")->text(), QStringLiteral("2 members + 1 only if needed · 2 roles"));
    // A role out of the roster: its only-if-needed is off, and a task of it is flagged.
    stepper("tester")->findChild<QPushButton *>("teamStepperMinus")->click();
    QTRY_COMPARE(value(stepper("tester")), 0);
    for (auto *check : editor.findChildren<QCheckBox *>("teamTemplateSeatOptional")) if (check->property("role") == "tester") QVERIFY(!check->isEnabled());
    QTRY_VERIFY(issues().contains("#3 uses Tester, which is not in the roster"));
    stepper("tester")->findChild<QPushButton *>("teamStepperPlus")->click();
    QTRY_COMPARE(value(stepper("tester")), 1);
    for (auto *check : editor.findChildren<QCheckBox *>("teamTemplateSeatOptional")) if (check->property("role") == "tester" && !check->isChecked()) { check->click(); break; }
    QTRY_VERIFY(!issues().contains("not in the roster"));
    // Work in: Same folder with writers warns.
    segment("teamTemplateWork", "same");
    QTRY_VERIFY(editor.findChild<QLabel *>("teamTemplateWorkWarning"));
    // Max members 3: Lead + roster is 4.
    for (int i = 0; i < 3; ++i) { QTRY_VERIFY(editor.findChild<QWidget *>("teamTemplateMax")); editor.findChild<QWidget *>("teamTemplateMax")->findChild<QPushButton *>("teamStepperMinus")->click(); }
    QTRY_COMPARE(value(editor.findChild<QWidget *>("teamTemplateMax")), 3);
    QTRY_VERIFY(issues().contains("Lead + roster = 4 > max members 3"));
    // Task skeleton: a phase, an "after" that points forward, + Task.
    auto *phase = editor.findChildren<QComboBox *>("teamTemplateTaskPhase").at(2);
    phase->setCurrentIndex(phase->findData("2")); emit phase->activated(phase->currentIndex());
    QTRY_COMPARE(editor.findChildren<QComboBox *>("teamTemplateTaskPhase").at(2)->currentData().toString(), QStringLiteral("2"));
    auto *after = editor.findChildren<QLineEdit *>("teamTemplateTaskAfter").at(1);
    after->setText("1, 5"); emit after->editingFinished();
    QTRY_VERIFY(issues().contains("#2: “after 1, 5” must point to an earlier task"));
    editor.findChild<QPushButton *>("teamTemplateAddTask")->click();
    QTRY_COMPARE(editor.findChildren<QLineEdit *>("teamTemplateTaskTitle").size(), 4);
    segment("teamTemplatePhaseApproval", "review");
    shot(&editor, "ux-9-templates-issues-dark");
    editor.setDark(false); lightShot(&editor, "ux-9-templates-issues-light"); darkPalette(); editor.setDark(true);
    // Hide the unused roles: none here.
    QCOMPARE(editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->text(), QStringLiteral("Hide unused roles (0)"));
    editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->click();
    QTRY_COMPARE(editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->text(), QStringLiteral("Show all catalog roles (2)"));
    editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->click();
    // Roster and Task skeleton fold into summaries, remembered.
    editor.findChild<QPushButton *>("teamTemplateRosterToggle")->click();
    QTRY_VERIFY(editor.findChildren<QWidget *>("teamTemplateSeatCount").isEmpty());
    QCOMPARE(QSettings().value(CollapsibleBlock::settingsKey("catalog-roster")).toBool(), false);
    QCOMPARE(editor.findChild<QLabel *>("teamTemplateRosterSummary")->text(), QStringLiteral("2 members + 1 only if needed · 2 roles"));
    editor.findChild<QPushButton *>("teamTemplateSkeletonToggle")->click();
    QTRY_VERIFY(editor.findChild<QLabel *>("teamTemplateSkeletonSummary"));
    QCOMPARE(editor.findChild<QLabel *>("teamTemplateSkeletonSummary")->text(), QStringLiteral("4 tasks in 2 phases: Plan → Build → Test → New task"));
    shot(&editor, "ux-9-templates-folded-dark");
    editor.setDark(false); lightShot(&editor, "ux-9-templates-folded-light"); darkPalette(); editor.setDark(true);
    editor.findChild<QPushButton *>("teamTemplateRosterToggle")->click();
    editor.findChild<QPushButton *>("teamTemplateSkeletonToggle")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamTemplateSeatCount").size(), 2);
    segment("teamTemplateWork", "worktree");
    QTRY_VERIFY(!editor.findChild<QLabel *>("teamTemplateWorkWarning"));
    shot(&editor, "ux-9-templates-dark");
    editor.setDark(false); lightShot(&editor, "ux-9-templates-light"); darkPalette(); editor.setDark(true);
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE_WITH_TIMEOUT(calls(script(), "catalog set").size(), 1, 20000);
    const auto feature = calls(script(), "catalog set").first()["catalog"].toObject()["templates"].toArray()[0].toObject();
    const auto seats = feature["roster"].toArray();
    QCOMPARE(seats.size(), 2);
    for (const auto &seat : seats) {
        const auto o = seat.toObject();
        if (o["role"] == "backend") { QCOMPARE(o["count"].toInt(), 2); QVERIFY(!o["optional"].toBool()); }
        else { QCOMPARE(o["role"].toString(), QStringLiteral("tester")); QCOMPARE(o["count"].toInt(), 1); QVERIFY(o["optional"].toBool()); }
    }
    QCOMPARE(feature["work"].toString(), QStringLiteral("worktree"));
    QCOMPARE(feature["max_members"].toInt(), 3);
    QCOMPARE(feature["phase_approval"].toString(), QStringLiteral("review"));
    QCOMPARE(feature["phases"].toString(), QStringLiteral("owner"));
    const auto tasks = feature["tasks"].toArray();
    QCOMPARE(tasks.size(), 4);
    QCOMPARE(tasks[2].toObject()["phase"].toInt(), 2);
    QCOMPARE(tasks[1].toObject()["deps"].toArray(), (QJsonArray{0}));   // "5" points forward and is left out
    QCOMPARE(tasks[3].toObject()["title"].toString(), QStringLiteral("New task"));
}

// C1 (#543): a role whose task template was never typed shows the default
// template. Whatever writes its prompt order (a move, or typing in Team
// protocol) saves that template too, so the server never gets an order with
// "task" and an empty template.
void TestTeamsUi::aPromptOrderSavesTheTaskTemplateItShows()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1180, 1500); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    const auto block = [&](const QString &key) -> QFrame * {
        for (auto *b : editor.findChildren<QFrame *>("teamPromptBlock")) if (b->property("block") == key) return b;
        return nullptr;
    };
    roles->setCurrentRow(1);   // Backend: typing in Team protocol
    QTRY_COMPARE(editor.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Backend"));
    const QString shown = editor.findChild<QPlainTextEdit *>("teamRoleTaskTemplate")->toPlainText();
    QVERIFY(shown.contains("{title}"));
    editor.findChild<QPlainTextEdit *>("teamRoleProtocol")->setPlainText("Report each endpoint.");
    roles->setCurrentRow(2);   // Tester: one move
    QTRY_COMPARE(editor.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Tester"));
    QTRY_VERIFY(block("task"));
    block("task")->findChild<QToolButton *>("teamPromptUp")->click();
    QTRY_VERIFY(block("task") && block("task")->findChild<QToolButton *>("teamPromptDown")->isEnabled());   // no longer last
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE_WITH_TIMEOUT(calls(script(), "catalog set").size(), 1, 20000);
    const auto saved = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray();
    for (const int row : {1, 2}) {
        const auto role = saved[row].toObject();
        QVERIFY2(role["prompt_order"].toArray().contains("task"), qPrintable(role["id"].toString()));
        QCOMPARE(role["task_template"].toString(), shown);
    }
    QCOMPARE(saved[0].toObject()["task_template"].toString(), QString());   // the untouched Lead keeps none
}

// U25 part 9 (spec 11.5): routing rules are edited in place - order, words,
// Use (a template or "+ role"), approval - and Try it names the template, the
// rule, the added roles, approval and work mode, with the roster as chips.
void TestTeamsUi::catalogRoutingEditsAndTriesAGoal()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    // The prototype's matching, as the server's route: "+ role" rules add and
    // keep looking, "everything else" always matches, stop words never do.
    auto c = parseCatalog(catalog());
    Rule add; add.id = "tests"; add.when = "tests"; add.use = "+tester";
    Rule all; all.id = "all"; all.when = "everything else"; all.templateId = "feature"; all.approval = "auto";
    c.routing = {add, all};
    auto found = TeamCatalogEditor::route(c, "Add tests for checkout");
    QCOMPARE(found.rule, 1); QCOMPARE(found.templateId, QStringLiteral("feature"));
    QCOMPARE(found.added, QStringList{"tester"}); QCOMPARE(found.approval, QStringLiteral("auto"));
    Rule words; words.id = "words"; words.when = "the and with"; words.templateId = "feature";
    c.routing = {words};
    found = TeamCatalogEditor::route(c, "the and with");
    QCOMPARE(found.rule, -1); QCOMPARE(found.templateId, QStringLiteral("feature"));   // the first template, no rule
    // N1: the prototype's own rules r1 and r2 and a "*" catch-all. "label",
    // "files" and "services" are stop words; a substring and a plural's
    // singular match; "*" alone always matches.
    Rule r1; r1.id = "r1"; r1.when = QStringLiteral("label \u201cbug\u201d \u00b7 \u2264 3 files"); r1.templateId = "feature";
    Rule r2; r2.id = "r2"; r2.when = QStringLiteral("services/payments/** \u00b7 **/auth/**"); r2.use = "+tester";
    Rule star; star.id = "star"; star.when = " * "; star.templateId = "feature";
    c.routing = {r1, r2, star};
    found = TeamCatalogEditor::route(c, "Tidy the label files of the services");
    QCOMPARE(found.rule, 2); QVERIFY(found.added.isEmpty());
    QCOMPARE(TeamCatalogEditor::route(c, "Debug the login").rule, 0);
    found = TeamCatalogEditor::route(c, "Round the payment total (OAuth)");
    QCOMPARE(found.rule, 2); QCOMPARE(found.added, QStringList{"tester"});
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1180, 760); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 3);
    for (auto *b : editor.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>()) if (b->property("segment") == "routing") { b->click(); break; }
    QTRY_VERIFY(editor.findChild<QWidget *>("teamRoutingEditor"));
    QVERIFY(!editor.findChild<QWidget *>("teamCatalogListColumn")->isVisible());
    for (int i = 0; i < 3; ++i) { QTRY_VERIFY(editor.findChild<QPushButton *>("teamRoutingAdd")); editor.findChild<QPushButton *>("teamRoutingAdd")->click(); }
    QTRY_COMPARE(editor.findChildren<QLineEdit *>("teamRuleWhen").size(), 3);
    editor.findChildren<QToolButton *>("teamRuleDelete").at(2)->click();
    QTRY_COMPARE(editor.findChildren<QLineEdit *>("teamRuleWhen").size(), 2);
    const auto when = [&](int i) { return editor.findChildren<QLineEdit *>("teamRuleWhen").at(i); };
    when(0)->setText("tests"); emit when(0)->editingFinished();
    when(1)->setText("everything else"); emit when(1)->editingFinished();
    // Use: a template or "+ role (add member)".
    auto *use = editor.findChildren<QComboBox *>("teamRuleTemplate").at(0);
    QCOMPARE(use->count(), 3);
    QCOMPARE(use->itemText(use->findData("+tester")), QStringLiteral("+ Tester (add member)"));
    use->setCurrentIndex(use->findData("+tester")); emit use->activated(use->currentIndex());
    QTRY_COMPARE(editor.findChildren<QComboBox *>("teamRuleTemplate").at(0)->currentData().toString(), QStringLiteral("+tester"));
    auto *approval = editor.findChildren<QComboBox *>("teamRuleApproval").at(1);
    QCOMPARE(approval->count(), 3);
    approval->setCurrentIndex(approval->findData("auto")); emit approval->activated(approval->currentIndex());
    approval = editor.findChildren<QComboBox *>("teamRuleApproval").at(0);
    approval->setCurrentIndex(approval->findData("")); emit approval->activated(approval->currentIndex());
    // Order: down, then up again.
    QVERIFY(!editor.findChildren<QToolButton *>("teamRuleUp").at(0)->isEnabled());
    editor.findChildren<QToolButton *>("teamRuleDown").at(0)->click();
    QTRY_COMPARE(when(0)->text(), QStringLiteral("everything else"));
    editor.findChildren<QToolButton *>("teamRuleUp").at(1)->click();
    QTRY_COMPARE(when(0)->text(), QStringLiteral("tests"));
    // Try it.
    auto *goal = editor.findChild<QLineEdit *>("teamRoutingTry");
    goal->setText("Add tests for checkout");
    const QString result = editor.findChild<QLabel *>("teamRoutingResult")->text();
    for (const char *part : {"Feature delivery", "rule 2", "(+ Tester)", "Run when the plan is valid", "Worktree per member"})
        QVERIFY2(result.contains(part), qPrintable(result));
    QCOMPARE(editor.findChild<QWidget *>("teamRoutingRoster")->findChildren<QWidget *>("teamRoleChip").size(), 4);   // Lead, Backend, Tester, + Tester
    shot(&editor, "ux-9-routing-dark");
    editor.setDark(false); QTRY_VERIFY(editor.findChild<QLineEdit *>("teamRoutingTry")->text() == "Add tests for checkout");
    lightShot(&editor, "ux-9-routing-light"); darkPalette(); editor.setDark(true);
    editor.findChild<QPushButton *>("teamCatalogSave")->click();
    QTRY_COMPARE_WITH_TIMEOUT(calls(script(), "catalog set").size(), 1, 20000);
    const auto rules = calls(script(), "catalog set").first()["catalog"].toObject()["routing"].toArray();
    QCOMPARE(rules.size(), 2);
    QCOMPARE(rules[0].toObject()["when"].toString(), QStringLiteral("tests"));
    QCOMPARE(rules[0].toObject()["use"].toString(), QStringLiteral("+tester"));
    QCOMPARE(rules[0].toObject()["approval"].toString(), QString());
    QCOMPARE(rules[1].toObject()["when"].toString(), QStringLiteral("everything else"));
    QCOMPARE(rules[1].toObject()["template"].toString(), QStringLiteral("feature"));
    QCOMPARE(rules[1].toObject()["approval"].toString(), QStringLiteral("auto"));
}

// U25 part 9 (spec 11.2): the catalog's header. By hand: Open Team designer,
// the unsaved count, Revert with its question, the working designer's banner.
// The designer's draft: its counts, Discard and Apply, counters on the tabs,
// marks in the list, the proposal plate over a read-only editor, and the
// out-of-date banner with Ask designer to redo.
void TestTeamsUi::catalogHeaderRevertDraftAndBanners()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] {
        QSettings().remove("workspace/skin"); darkPalette();
        QFile c(script() + ".catalog"); if (c.open(QIODevice::WriteOnly)) c.write(QJsonDocument(catalog()).toJson(QJsonDocument::Compact));
    });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor settings(&controller);
    settings.setDesignerButton(true);
    bool asked = false; settings.askDesigner = [&] { asked = true; };
    bool answer = false; QString question;
    settings.confirm = [&](const QString &name, const QString &, const QString &text, const QString &) { question = name + ':' + text; return answer; };
    settings.resize(1180, 820); settings.show();
    auto *roles = settings.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    QCOMPARE(settings.findChild<QLabel *>("teamCatalogTitle")->text(), QStringLiteral("Team catalog"));
    QVERIFY(settings.findChild<QLabel *>("teamCatalogStatus")->text().contains("v3"));
    for (const char *name : {"teamCatalogAskDesigner", "teamCatalogImport", "teamCatalogExport", "teamCatalogRevert", "teamCatalogSave"})
        QVERIFY2(settings.findChild<QPushButton *>(name)->isVisible(), name);
    QVERIFY(!settings.findChild<QPushButton *>("teamCatalogRevert")->isEnabled() && !settings.findChild<QPushButton *>("teamCatalogSave")->isEnabled());
    settings.findChild<QPushButton *>("teamCatalogAskDesigner")->click(); QVERIFY(asked);
    shot(&settings, "ux-9-settings-teams-dark");
    settings.setDark(false); lightShot(&settings, "ux-9-settings-teams-light"); darkPalette(); settings.setDark(true);
    roles->setCurrentRow(1);
    auto *name = settings.findChild<QLineEdit *>("teamRoleName"); name->setText("Backend API"); emit name->editingFinished();
    QTRY_VERIFY(settings.findChild<QLabel *>("teamCatalogStatus")->text().contains("● 1 unsaved change"));
    QVERIFY(settings.findChild<QPushButton *>("teamCatalogRevert")->isEnabled() && settings.findChild<QPushButton *>("teamCatalogSave")->isEnabled());
    shot(&settings, "ux-9-catalog-dirty-dark");
    settings.setDark(false); lightShot(&settings, "ux-9-catalog-dirty-light"); darkPalette(); settings.setDark(true);
    // Revert asks first.
    settings.findChild<QPushButton *>("teamCatalogRevert")->click();
    QCOMPARE(question, QStringLiteral("teamCatalogRevertConfirm:Drop 1 unsaved change?"));
    QCOMPARE(settings.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Backend API"));
    answer = true;
    settings.findChild<QPushButton *>("teamCatalogRevert")->click();
    QTRY_COMPARE(settings.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Backend"));
    QVERIFY(!settings.findChild<QPushButton *>("teamCatalogSave")->isEnabled());
    // While the designer works, its banner says what a manual save does.
    settings.setDesignerWorking(true);
    QTRY_VERIFY(settings.findChild<QFrame *>("teamCatalogBanner")->isVisible());
    QCOMPARE(settings.findChild<QLabel *>("teamCatalogBannerTitle")->text(), QStringLiteral("The designer is working"));
    shot(&settings, "ux-9-catalog-designer-working-dark");
    settings.setDesignerWorking(false);
    QTRY_VERIFY(!settings.findChild<QFrame *>("teamCatalogBanner")->isVisible());
    // The designer's draft: read only, decided on plates.
    TeamCatalogEditor draft(&controller);
    draft.setShowDraft(true);
    draft.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    qint64 redo = 0; draft.askRedo = [&](qint64 version) { redo = version; };
    draft.resize(1180, 1100); draft.show();
    QTRY_COMPARE(draft.findChild<QLabel *>("teamCatalogTitle")->text(), QStringLiteral("Draft catalog"));
    QVERIFY(draft.findChild<QLabel *>("teamCatalogStatus")->text().contains("based on v3 · 1 of 2 kept · 1 undecided"));
    for (const char *name : {"teamCatalogImport", "teamCatalogExport", "teamCatalogRevert", "teamCatalogSave"})
        QVERIFY2(!draft.findChild<QPushButton *>(name)->isVisible(), name);
    QVERIFY(draft.findChild<QPushButton *>("teamDesignerDiscard")->isVisible());
    QCOMPARE(draft.findChild<QPushButton *>("teamDesignerApply")->text(), QStringLiteral("✓ Apply 1 change"));
    QVERIFY(draft.findChild<QPushButton *>("teamDesignerApply")->isEnabled());
    QCOMPARE(draft.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>().first()->text(), QStringLiteral("Roles   2"));
    QVERIFY(draft.findChild<QLabel *>("teamCatalogLegend")->isVisible());
    auto *list = draft.findChild<QListWidget *>("teamCatalogRoles");
    QCOMPARE(list->count(), 4);
    QMap<QString, QString> marks;
    for (int i = 0; i < list->count(); ++i)
        if (auto *mark = list->itemWidget(list->item(i))->findChild<QLabel *>("teamCatalogMark")) marks.insert(list->item(i)->data(Qt::UserRole).toString(), mark->text());
    QCOMPARE(marks, (QMap<QString, QString>{{"tester", "CHANGED"}, {"security", "NEW · kept"}}));
    QVERIFY(!draft.findChild<QPushButton *>("teamCatalogAdd")->isVisible());
    list->setCurrentRow(2);   // tester
    QTRY_VERIFY(draft.findChild<QFrame *>("teamProposalPlate"));
    QVERIFY(!draft.findChild<QWidget *>("teamRoleEditor")->isEnabled());
    QCOMPARE(draft.findChild<QComboBox *>("teamRoleAgent")->currentData().toString(), QStringLiteral("claude"));   // the proposal's
    shot(&draft, "ux-9-draft-role-dark");
    draft.setDark(false); lightShot(&draft, "ux-9-draft-role-light"); darkPalette(); draft.setDark(true);
    QTRY_VERIFY(draft.findChild<QPushButton *>("teamProposalKeep"));
    draft.findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 1);
    QCOMPARE(calls(script(), "catalog decide").first()["proposal"].toString(), QStringLiteral("p1"));
    QCOMPARE(calls(script(), "catalog decide").first()["decision"].toString(), QStringLiteral("keep"));
    // The fixture logs the decision before it saves the draft. On a loaded
    // runner that save came after the catalog below and put version 3 back,
    // so the banner never showed (fork builds 88 and 95): wait for the save.
    QTRY_VERIFY_WITH_TIMEOUT(([&] {
        QFile saved(script() + ".catalog");
        if (!saved.open(QIODevice::ReadOnly)) return false;
        const auto proposals = QJsonDocument::fromJson(saved.readAll()).object()["draft"].toObject()["proposals"].toArray();
        return !proposals.isEmpty() && proposals[0].toObject()["state"].toString() == QStringLiteral("kept");
    })(), 30000);
    // The catalog moved on after the draft: Apply is off; redo or discard.
    auto moved = catalog(); auto inner = moved["catalog"].toObject(); inner["version"] = 4; moved["catalog"] = inner;
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(moved).toJson(QJsonDocument::Compact)));
    controller.refreshCatalog();
    // The fixture's catalog read takes more than QTRY's 5 s on a loaded Windows runner.
    QTRY_COMPARE_WITH_TIMEOUT(draft.findChild<QLabel *>("teamCatalogBannerTitle")->text(), QStringLiteral("Draft is out of date — Apply is off"), 30000);
    QVERIFY(draft.findChild<QFrame *>("teamCatalogBanner")->isVisible());
    QVERIFY(!draft.findChild<QPushButton *>("teamDesignerApply")->isEnabled());
    shot(&draft, "ux-9-draft-stale-dark");
    draft.setDark(false); lightShot(&draft, "ux-9-draft-stale-light"); darkPalette(); draft.setDark(true);
    draft.findChild<QPushButton *>("teamCatalogRedo")->click(); QCOMPARE(redo, 4);
    draft.findChild<QPushButton *>("teamCatalogBannerDiscard")->click();
    QTRY_COMPARE(calls(script(), "catalog discard").size(), 1);
}

// U16: the Team designer's own session waits at Claude's folder trust. The
// window shows its recorded activity and the question as the session view's
// card, in place of the composer; answering it there sends the native answer
// with the question's id, hash and run, and the card goes once it is answered.
void TestTeamsUi::designerQuestionIsAnsweredInTheWindow()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
h='a'*64
if a[:1]==['answer']:
 body=json.load(sys.stdin)
 open(here+'.answers','a').write(json.dumps({'name':a[1],'body':body})+'\n')
 open(here+'.answered','w').write('1')
 print(json.dumps({'status':'answered','request_id':body['request_id'],'name':a[1],'question_id':body['question_id'],
  'question_hash':body['expected_question_hash'],'run_id':body['expected_run_id'],'conversation_id':body['expected_conversation_id']}))
elif a[:1]==['inspect']:
 q={'question_id':'claude-trust:'+h,'question_hash':h,'source':'claude_folder_trust','answer_transport':'claude_tui',
    'run_id':'run-1','conversation_id':None,'can_answer':True,'created_at':1791018100,
    'questions':[{'id':'trust','question':'Do you trust the files in this folder?','body':'/zerus/team-designer','allow_other':False,
                  'options':[{'id':'trust_0','label':'Yes, proceed'},{'id':'trust_1','label':'No, exit'}]}]}
 print(json.dumps({'name':a[1],'run_id':'run-1','conversation_id':None,'runtime_state':'live','process_state':'running','cursor':1,
  'events':[{'seq':1,'at':1791018101,'type':'UserPromptSubmit','detail':'Design a security reviewer for payment code.'}],
  'pending_questions':[] if os.path.exists(here+'.answered') else [q]}))
else:print('{}')
)PY"));
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    // Its own name each run: the card keeps answered drafts by session.
    const QString designer = "claude/checkout/team-designer-" + QUuid::createUuid().toString(QUuid::Id128).left(8);
    bool waiting = true;
    page.designerSession = [&] { return designer; };
    page.designerLive = [&] { Teams::Live live; live.known = true; live.attention = waiting; return live; };
    page.setContext("/work/checkout", "checkout");
    page.resize(1400, 860); page.show();
    auto *card = page.findChild<QuestionCard *>("teamConversationQuestion");
    QVERIFY(card); card->setReviewDelayForTests(0);
    auto *input = page.findChild<QPlainTextEdit *>("teamConversationInput");
    // The question replaces the composer; the recorded activity stays above it.
    QTRY_VERIFY(card->isVisible());
    QVERIFY(!input->isVisible());
    QCOMPARE(page.findChild<QLabel *>("teamDesignerAgentStatus")->text(), QStringLiteral("Needs your answer"));
    // The Activity render is slow on a loaded machine.
    QTRY_VERIFY_WITH_TIMEOUT(page.findChild<QTextBrowser *>("teamConversationText")->toPlainText().contains("Design a security reviewer"), 20000);
    QCOMPARE(card->findChild<QLabel *>("questionPrompt")->text(), QStringLiteral("Do you trust the files in this folder?"));
    // Each shot after the Activity has drawn its theme.
    auto *activity = static_cast<ActivityView *>(page.findChild<QWidget *>("teamConversationActivity"));
    QTRY_VERIFY_WITH_TIMEOUT(!activity->renderPending(), 20000);
    shot(&page, "u16-designer-question-dark");
    lightPalette(); page.setDark(false);
    QTRY_VERIFY_WITH_TIMEOUT(!activity->renderPending(), 20000);
    lightShot(&page, "u16-designer-question-light"); darkPalette(); page.setDark(true);
    // Answered here: the native answer names the very question it answers.
    QAbstractButton *yes = nullptr;
    for (auto *option : card->findChildren<QAbstractButton *>("questionOption"))
        if (option->property("optionId").toString() == "trust_0") yes = option;
    QVERIFY(yes); yes->click();
    auto *submit = card->findChild<QPushButton *>("submitQuestionAnswer");
    QTRY_VERIFY(submit->isVisible() && submit->isEnabled());
    submit->click();
    QTRY_VERIFY(QFile::exists(program + ".answers"));
    QFile log(program + ".answers"); QVERIFY(log.open(QIODevice::ReadOnly));
    const auto lines = log.readAll().split('\n');
    QCOMPARE(lines.size() - (lines.last().isEmpty() ? 1 : 0), 1);
    const auto answer = QJsonDocument::fromJson(lines.first()).object();
    QCOMPARE(answer["name"].toString(), designer);
    const auto body = answer["body"].toObject();
    QCOMPARE(body["question_id"].toString(), QStringLiteral("claude-trust:") + QString(64, 'a'));
    QCOMPARE(body["expected_question_hash"].toString(), QString(64, 'a'));
    QCOMPARE(body["expected_run_id"].toString(), QStringLiteral("run-1"));
    QCOMPARE(body["expected_conversation_id"].toString(), QString());
    const auto chosen = body["answers"].toArray().first().toObject();
    QCOMPARE(chosen["question_id"].toString(), QStringLiteral("trust"));
    QVERIFY2(QJsonDocument(chosen).toJson().contains("trust_0"), QJsonDocument(chosen).toJson().constData());
    // Once the session no longer asks, the composer is back and nothing is sent again.
    waiting = false;
    QTRY_VERIFY(!card->isVisible());
    QTRY_VERIFY(input->isVisible());
    QTest::qWait(300);
    log.seek(0); QCOMPARE(log.readAll().count('\n'), 1);
}

// U15: with no designer session the draft is still decided, from the
// proposals list and from the catalog, and applied; a refused decision shows
// its error next to the proposal in every view of the draft.
void TestTeamsUi::draftIsDecidedWithoutADesignerSession()
{
    auto fixture = catalog();
    auto draft = fixture["draft"].toObject(); auto proposals = draft["proposals"].toArray();
    auto second = proposals[1].toObject(); second["state"] = "proposed"; proposals[1] = second;
    draft["proposals"] = proposals; fixture["draft"] = draft;
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(fixture).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    page.designerSession = [] { return QString(); };   // stopped or never started
    page.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    page.setContext("/work/checkout", "checkout");
    page.resize(1600, 900); page.show();
    TeamDesignerPanel panel(&controller);
    panel.resize(560, 900); panel.show();
    QTRY_COMPARE(page.findChildren<QFrame *>("teamDesignerProposal").size(), 2);
    QVERIFY(page.findChild<QWidget *>("teamDesignerEmpty")->isVisible());      // not started
    QVERIFY(page.findChild<QScrollArea *>("teamDesignerProposals")->isVisible());
    QVERIFY(page.findChild<QFrame *>("teamDesignerDraftBar")->isVisible());
    const auto card = [&page](int n) { return page.findChildren<QFrame *>("teamDesignerProposal").value(n); };
    // A refused decision: its error under the proposal, the draft unchanged.
    QVERIFY(FakeCli::replace(script() + ".refuse", "hgs: The catalog draft is locked by another change"));
    card(0)->findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_VERIFY(card(0) && card(0)->findChild<QLabel *>("teamProposalError"));
    QVERIFY(card(0)->findChild<QLabel *>("teamProposalError")->text().contains("locked by another change"));
    QVERIFY(!card(1)->findChild<QLabel *>("teamProposalError"));
    QCOMPARE(controller.catalog().proposals[0].state, QStringLiteral("proposed"));
    QTRY_VERIFY(panel.findChild<QLabel *>("teamProposalError"));
    QVERIFY(panel.findChild<QLabel *>("teamProposalError")->text().contains("locked by another change"));
    shot(&page, "u15-decide-refused");
    // Two proposals kept: p1 in the list, p2 in the catalog's draft card.
    QFile::remove(script() + ".refuse");
    card(0)->findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(controller.catalog().proposals[0].state, QStringLiteral("kept"));
    QTRY_VERIFY(!page.findChild<QLabel *>("teamProposalError"));
    QVERIFY(!panel.findChild<QLabel *>("teamProposalError"));
    page.editor()->showProposal("p2");
    QTRY_VERIFY(page.editor()->findChild<QPushButton *>("teamProposalKeep"));
    page.editor()->findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(controller.catalog().proposals[1].state, QStringLiteral("kept"));
    auto *apply = page.findChild<QPushButton *>("teamDesignerApplyKept");
    QTRY_VERIFY(apply && apply->isEnabled() && apply->text().contains(QStringLiteral("Apply 2 changes")));
    shot(&page, "u15-two-kept-no-session");
    // A refused Apply: its error under the DRAFT bar, the draft kept.
    QVERIFY(FakeCli::replace(script() + ".refuseapply", "hgs: The catalog changed (now v5); refresh and try again"));
    apply->click();
    auto *draftError = page.findChild<QLabel *>("teamDesignerDraftError");
    QTRY_VERIFY(draftError && draftError->isVisible());
    QVERIFY(draftError->text().contains("The catalog changed (now v5)"));
    QCOMPARE(controller.catalog().version, 3);
    shot(&page, "u15-apply-refused");
    QFile::remove(script() + ".refuseapply");
    apply = page.findChild<QPushButton *>("teamDesignerApplyKept");
    apply->click();
    QTRY_COMPARE(controller.catalog().version, 4);   // v3 + 1
    QCOMPARE(calls(script(), "catalog apply").size(), 2);
    QCOMPARE(calls(script(), "catalog decide").size(), 3);
    QVERIFY(controller.catalog().proposals.isEmpty());
    QTRY_VERIFY(!page.findChild<QLabel *>("teamDesignerDraftError")->isVisible());
    QVERIFY(!page.findChild<QScrollArea *>("teamDesignerProposals")->isVisible());
    QVERIFY(controller.catalog().role("security"));
    QCOMPARE(controller.catalog().role("tester")->agent, QStringLiteral("claude"));
}

// U29: a task added to its running phase carries the "out of the phase's
// original scope" icon in Graph, Board, Layers and the task card, light and
// dark; the owner adds one through the warning and a typed confirmation.
void TestTeamsUi::addedLateTaskIsOutOfScopeEverywhere()
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
        shot(&workspace, name + "-dark");
        lightPalette(); workspace.setTheme(false); workspace.render(); QTest::qWait(200);
        lightShot(&workspace, name + "-light");
        darkPalette(); workspace.setTheme(true); workspace.render(); QTest::qWait(150);
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
    QVERIFY(next->isEnabled());
    next->click();
    QTRY_COMPARE(calls(script(), "add-to-phase").size(), 1);
    const auto first = calls(script(), "add-to-phase").first();
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

// U28: the catalog's Planning rules tab: edit with the unsaved-changes
// state, Save and Revert, Reset to built-in after a confirmation (a save
// without the rules, which hgs fills with its own), and a designer proposal
// shown as CHANGED with Keep / Reject / Undo and its line diff.
void TestTeamsUi::planningRulesAreEditedSavedResetAndProposed()
{
    const QString rules = QStringLiteral("(1) At most 20-25 tasks per phase, each ending with an install-ready build.\n"
                                         "(2) Checkpoints on demand.\n(3) Balance every lane by volume and cost.");
    auto fixture = catalog();
    auto data = fixture["catalog"].toObject(); data["planning_rules"] = rules; fixture["catalog"] = data;
    fixture["draft"] = QJsonObject{{"base", 3}, {"proposals", QJsonArray()}};
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(fixture).toJson(QJsonDocument::Compact)));
    // The model keeps the rules for every save; an older hgs sends none.
    QCOMPARE(parseCatalog(fixture).json()["planning_rules"].toString(), rules);
    QVERIFY(!parseCatalog(catalog()).json().contains("planning_rules"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    QStringList asked;
    editor.confirm = [&](const QString &name, const QString &, const QString &, const QString &) { asked << name; return true; };
    editor.resize(1000, 760); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 3);
    const auto tab = [&](const char *segment) {
        for (auto *b : editor.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>()) if (b->property("segment") == segment) { b->click(); return true; }
        return false;
    };
    QVERIFY(tab("rules"));
    auto *text = editor.findChild<QPlainTextEdit *>("teamPlanningRules");
    QTRY_VERIFY(text && text->isVisible());
    QCOMPARE(text->toPlainText(), rules);
    QVERIFY(!editor.findChild<QListWidget *>("teamCatalogRoles")->isVisible());
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave");
    auto *reset = editor.findChild<QPushButton *>("teamPlanningRulesReset");
    QVERIFY(!save->isEnabled() && reset->isEnabled());
    const auto status = [&] { return editor.findChild<QLabel *>("teamCatalogStatus")->text(); };
    shot(&editor, "ux-u28-rules-dark");
    lightPalette(); editor.setDark(false); QTRY_VERIFY(editor.findChild<QPlainTextEdit *>("teamPlanningRules"));
    lightShot(&editor, "ux-u28-rules-light");
    darkPalette(); editor.setDark(true);
    // Edit by hand: one unsaved change; Reset waits for Save or Revert.
    text = editor.findChild<QPlainTextEdit *>("teamPlanningRules");
    text->setPlainText(rules + "\n(4) QA never implements tasks.");
    QTRY_VERIFY(status().contains("1 unsaved change"));
    QVERIFY(save->isEnabled());
    editor.findChild<QPushButton *>("teamCatalogRevert")->click();
    QCOMPARE(asked.last(), QStringLiteral("teamCatalogRevertConfirm"));
    QTRY_COMPARE(editor.findChild<QPlainTextEdit *>("teamPlanningRules")->toPlainText(), rules);
    QVERIFY(!status().contains("unsaved") && !save->isEnabled());
    text = editor.findChild<QPlainTextEdit *>("teamPlanningRules");
    text->setPlainText(rules + "\n(4) QA never implements tasks.");
    // Unsaved edits disable Reset at once: a click would drop them.
    QTRY_VERIFY(status().contains("unsaved"));
    QVERIFY(!editor.findChild<QPushButton *>("teamPlanningRulesReset")->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto saved = calls(script(), "catalog set").first();
    QCOMPARE(saved["version"].toInt(), 3);
    QCOMPARE(saved["catalog"].toObject()["planning_rules"].toString(), rules + "\n(4) QA never implements tasks.");
    QCOMPARE(saved["catalog"].toObject()["roles"].toArray().size(), 3);   // the rest of the catalog goes with them
    // Reset to built-in: confirmed, then a save without the rules.
    controller.refreshCatalog();
    QTRY_VERIFY(([&] { auto *r = editor.findChild<QPushButton *>("teamPlanningRulesReset"); return r && r->isEnabled(); })());
    editor.findChild<QPushButton *>("teamPlanningRulesReset")->click();
    QCOMPARE(asked.last(), QStringLiteral("teamPlanningRulesResetConfirm"));
    QTRY_COMPARE(calls(script(), "catalog set").size(), 2);
    const auto resetCall = calls(script(), "catalog set").last();
    QVERIFY(!resetCall["catalog"].toObject().contains("planning_rules"));
    QCOMPARE(resetCall["version"].toInt(), 3);
    QCOMPARE(resetCall["catalog"].toObject()["roles"].toArray().size(), 3);
    // A cancelled reset changes nothing.
    TeamCatalogEditor cancelled(&controller);
    cancelled.confirm = [](const QString &, const QString &, const QString &, const QString &) { return false; };
    cancelled.resize(1000, 760); cancelled.show();
    for (auto *b : cancelled.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>()) if (b->property("segment") == "rules") b->click();
    QTRY_VERIFY(cancelled.findChild<QPushButton *>("teamPlanningRulesReset"));
    cancelled.findChild<QPushButton *>("teamPlanningRulesReset")->click();
    QTest::qWait(200);
    QCOMPARE(calls(script(), "catalog set").size(), 2);
    cancelled.hide();
    // A designer proposal on the rules: CHANGED, Keep / Reject, the line diff.
    const QString proposed = QStringLiteral("(1) At most 20 tasks per phase, each ending with an install-ready build.\n"
                                            "(2) Checkpoints on demand.\n(3) Balance every lane by volume and cost.\n(4) QA never implements tasks.");
    fixture["draft"] = QJsonObject{{"base", 3}, {"proposals", QJsonArray{QJsonObject{
        {"id", "pr1"}, {"kind", "planning_rules"}, {"action", "change"}, {"target", "planning_rules"}, {"title", "Planning rules: 20 tasks, QA rule"},
        {"reason", "Phase 38 ran 27 tasks"}, {"value", proposed}, {"state", "proposed"}}}}};
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(fixture).toJson(QJsonDocument::Compact)));
    TeamCatalogEditor draft(&controller);
    draft.setShowDraft(true);
    draft.resize(1000, 760); draft.show();
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalog().proposals.size() == 1);
    QCOMPARE(controller.catalog().proposals.first().text, proposed);
    QCOMPARE(withProposals(controller.catalog(), true).planningRules, proposed);
    QTRY_VERIFY(([&] { for (auto *b : draft.findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>())
                           if (b->property("segment") == "rules") return b->text().contains("1"); return false; })());
    draft.showProposal("pr1");
    QTRY_VERIFY(draft.findChild<QFrame *>("teamProposalPlate"));
    QVERIFY(!draft.findChild<QPlainTextEdit *>("teamPlanningRules"));   // the draft shows the diff, not the editor
    QVERIFY(!draft.findChild<QPushButton *>("teamPlanningRulesReset")->isVisible());
    const auto lines = [&](const char *name) { QStringList found; for (auto *l : draft.findChildren<QLabel *>(name)) found << l->text(); return found; };
    QCOMPARE(lines("teamPlanningRulesRemoved"), QStringList{"(1) At most 20-25 tasks per phase, each ending with an install-ready build."});
    QCOMPARE(lines("teamPlanningRulesAdded"), (QStringList{"(1) At most 20 tasks per phase, each ending with an install-ready build.", "(4) QA never implements tasks."}));
    QCOMPARE(lines("teamPlanningRulesSame").size(), 2);
    shot(&draft, "ux-u28-rules-proposal-dark");
    lightPalette(); draft.setDark(false); QTRY_VERIFY(draft.findChild<QFrame *>("teamProposalPlate"));
    lightShot(&draft, "ux-u28-rules-proposal-light");
    darkPalette(); draft.setDark(true);
    QTRY_VERIFY(draft.findChild<QPushButton *>("teamProposalKeep"));
    draft.findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 1);
    QCOMPARE(calls(script(), "catalog decide").first()["proposal"].toString(), QStringLiteral("pr1"));
    QCOMPARE(calls(script(), "catalog decide").first()["decision"].toString(), QStringLiteral("keep"));
    // Kept: Undo; then Reject.
    QTRY_VERIFY(draft.findChild<QPushButton *>("teamProposalUndo"));
    draft.findChild<QPushButton *>("teamProposalUndo")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 2);
    QCOMPARE(calls(script(), "catalog decide").last()["decision"].toString(), QStringLiteral("undo"));
    QTRY_VERIFY(draft.findChild<QPushButton *>("teamProposalReject"));
    draft.findChild<QPushButton *>("teamProposalReject")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 3);
    QCOMPARE(calls(script(), "catalog decide").last()["decision"].toString(), QStringLiteral("reject"));
}

// Owner, 2026-10-28: only Activity took files; in the lead's conversation a
// pasted image became "[Image #1]" text and the file was lost. Every
// conversation has Attach, and a pasted image is an attachment sent with it.
void TestTeamsUi::coordinatorTakesPastedImagesAndFiles()
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
void TestTeamsUi::terminalDraftIsShownAndSentFromTheConversation()
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

void TestTeamsUi::heldSentTakesEachLostMessageOnce()
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

void TestTeamsUi::coordinatorRestoresAHeldMessageThatWasNotDelivered()
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
    shot(&conversation, "n5-held-not-delivered-dark");
    lightPalette(); conversation.setTheme(false); lightShot(&conversation, "n5-held-not-delivered-light"); darkPalette(); conversation.setTheme(true);
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

// U35: the coordinator's composer, against an hgs that holds a message while
// the lead has not taken the previous one. The held message shows "Queued
// until the lead takes your previous message" with Cancel; Cancel sends
// send-cancel with its request ID and returns the text; once hgs delivers it,
// the line goes. A dialog in the lead's terminal is refused with Open terminal.
// #511 N6: the designer's layout keeps both in its composer, under the messages.
void TestTeamsUi::coordinatorQueuesASecondMessageWithCancel_data()
{
    QTest::addColumn<bool>("designer");
    QTest::newRow("coordinator") << false;
    QTest::newRow("designer") << true;
}

void TestTeamsUi::coordinatorQueuesASecondMessageWithCancel()
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
    QCOMPARE(input->toPlainText(), QString());
    QTRY_VERIFY(inComposer(held));
    // No quick-ask chips here either.
    QVERIFY(conversation.findChildren<QPushButton *>("teamConversationAsk").isEmpty());
    shot(&conversation, "u35-coordinator-queued" + suffix + "-dark");
    lightPalette(); conversation.setTheme(false); lightShot(&conversation, "u35-coordinator-queued" + suffix + "-light"); darkPalette(); conversation.setTheme(true);
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

// U45-U52 (spec 0): a team, a role and a task that set none of the new fields
// look and send exactly as before: no new key in a plan or a catalog, nothing
// to save in the team settings, the task editor keeps today's fields.
void TestTeamsUi::teamCustomizationIsOptIn()
{
    const Team planned = parse(team("planning"));
    const auto plan = planInput(planned);
    for (const auto &value : plan["tasks"].toArray())
        QCOMPARE(value.toObject().keys(), (QStringList{"assignee", "criteria", "deps", "description", "id", "phase", "title"}));
    for (const auto &value : plan["members"].toArray())
        QCOMPARE(value.toObject().keys(), (QStringList{"account", "agent", "computer", "directory", "effort", "id", "model", "permissions", "prompt", "role"}));
    QCOMPARE(planned.assigneeRole(*planned.task(6), QStringLiteral("No assignee")), QStringLiteral("No assignee"));
    for (const auto &role : parseCatalog(catalog()).roles)
        QVERIFY2(!role.json().contains("models") && !role.json().contains("context_before_task"), qPrintable(role.id));
    // Team settings: nothing set, nothing to save, no intake keys.
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    {
        TeamSettingsDialog dialog(&controller, parse(team()), true);
        QVERIFY(!dialog.findChild<QCheckBox *>("teamSettingsBranchPerTask")->isChecked());
        QVERIFY(!dialog.findChild<QCheckBox *>("teamSettingsRestartCrashed")->isChecked());
        QVERIFY(dialog.findChild<QLabel *>("teamSettingsNoIntakes"));
        QVERIFY(dialog.changes().isEmpty());
        QVERIFY(!dialog.findChild<QPushButton *>("teamSettingsSave")->isEnabled());
    }
    // The task editor saved unchanged sends the task as before.
    setTeam(team("planning"));
    TeamPanel panel(&controller);
    panel.resize(420, 1200); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && controller.watched()->full);
    bool saved = false;
    std::function<void(int)> look = [&](int tries) {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != "teamTaskEditor") { if (tries > 0) QTimer::singleShot(10, &panel, [&look, tries] { look(tries - 1); }); return; }
        QCOMPARE(dialog->findChild<QComboBox *>("teamTaskEditAssignee")->currentData().toString(), QStringLiteral("backend"));
        QCOMPARE(dialog->findChild<QLineEdit *>("teamTaskEditModels")->text(), QString());
        for (auto *b : dialog->findChildren<QPushButton *>()) if (b->text() == QStringLiteral("Save")) { saved = true; b->click(); return; }
        dialog->reject();
    };
    QTimer::singleShot(0, &panel, [&look] { look(300); });
    panel.editTask(*controller.watched(), 2);
    QVERIFY(saved);
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    for (const auto &value : calls(script(), "plan").first()["tasks"].toArray()) {
        const auto task = value.toObject();
        QVERIFY2(!task.contains("models") && !task.contains("role"), qPrintable(QString::number(task["id"].toInt())));
        if (task["id"].toInt() == 2) QCOMPARE(task["assignee"].toString(), QStringLiteral("backend"));
    }
}

// U45/U46: a role's models (model and effort) and its context before a task
// are edited in the catalog and saved with the role; the other roles gain nothing.
void TestTeamsUi::catalogRoleModelsAndContextAreSaved()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1000, 900); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    roles->setCurrentRow(1);
    QTRY_COMPARE(editor.findChild<QLineEdit *>("teamRoleName")->text(), QStringLiteral("Backend"));
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").size(), 0);
    QVERIFY(editor.findChild<QLabel *>("teamRoleModelsHint")->text().startsWith("Only the default model"));
    const auto addModel = [&](const QString &model, const QString &effort) {
        auto *name = editor.findChild<QComboBox *>("teamRoleModelsAdd"); QVERIFY(name);
        name->lineEdit()->setText(model);
        auto *level = editor.findChild<QComboBox *>("teamRoleModelsEffort");
        level->setCurrentIndex(level->findData(effort));
        editor.findChild<QPushButton *>("teamRoleModelsAddButton")->click();
    };
    addModel("gpt-5.5-codex", "high");
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").size(), 1);
    addModel("gpt-5.5", QString());
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").size(), 2);
    addModel("gpt-5.5", QString());   // a model is listed once
    addModel("o-mini", "low");
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").size(), 3);
    // × removes one.
    for (auto *chip : editor.findChildren<QPushButton *>("teamRoleModelChoice")) if (chip->text().startsWith("o-mini")) { chip->click(); break; }
    QTRY_COMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").size(), 2);
    QCOMPARE(editor.findChildren<QPushButton *>("teamRoleModelChoice").first()->text(), QStringLiteral("gpt-5.5-codex@high  ×"));
    QVERIFY(editor.findChild<QLabel *>("teamRoleModelsHint")->text().startsWith("Before a task"));
    // Context before task: Clear.
    auto *context = editor.findChild<QWidget *>("teamRoleContext"); QVERIFY(context);
    for (auto *b : context->findChildren<QPushButton *>()) if (b->property("segment").toString() == "clear") b->click();
    QTRY_VERIFY(editor.findChild<QLabel *>("teamRoleContextHint")->text().contains("cleared"));
    shot(&editor, "u45-catalog-role-models-dark");
    lightPalette(); editor.setDark(false); lightShot(&editor, "u45-catalog-role-models-light"); darkPalette(); editor.setDark(true);
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave"); QTRY_VERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto sent = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray();
    const auto backend = sent[1].toObject();
    QCOMPARE(backend["models"].toArray(), (QJsonArray{QJsonObject{{"model", "gpt-5.5-codex"}, {"effort", "high"}}, QJsonObject{{"model", "gpt-5.5"}}}));
    QCOMPARE(backend["context_before_task"].toString(), QStringLiteral("clear"));
    for (int i : {0, 2}) QVERIFY(!sent[i].toObject().contains("models") && !sent[i].toObject().contains("context_before_task"));
}

// U45: the plan editor gives a task models and a role instead of an assignee;
// a task for a role reads "Any free <role>" and opens with its choice.
void TestTeamsUi::planTaskTakesModelsAndARole()
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

// U48/U51/U52: the team settings send only what changed (a branch per task
// switched off is null), refuse a branch name without {id}, and keep the
// intake keys: a new key is shown once, a revoke asks first.
void TestTeamsUi::teamSettingsSendOnlyWhatChanged()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    {
        TeamSettingsDialog dialog(&controller, parse(team()), true);
        dialog.show();
        auto *save = dialog.findChild<QPushButton *>("teamSettingsSave");
        dialog.findChild<QCheckBox *>("teamSettingsBranchPerTask")->setChecked(true);
        auto *name = dialog.findChild<QLineEdit *>("teamSettingsBranchTemplate");
        QCOMPARE(name->text(), QStringLiteral("task/{id}"));
        name->setText("task/{member}");
        QVERIFY(!save->isEnabled());
        QVERIFY(dialog.findChild<QLabel *>("teamSettingsProblem")->text().contains("{id}"));
        name->setText("task/{id}");
        dialog.findChild<QLineEdit *>("teamSettingsBaseBranch")->setText("main");
        dialog.findChild<QCheckBox *>("teamSettingsRestartCrashed")->setChecked(true);
        QVERIFY(save->isEnabled());
        shot(&dialog, "u48-team-settings-dark");
        save->click();
        QTRY_COMPARE(calls(script(), "settings").size(), 1);
        QTRY_COMPARE(dialog.result(), int(QDialog::Accepted));
        QCOMPARE(calls(script(), "settings").first()["settings"].toObject(),
                 (QJsonObject{{"branch_per_task", QJsonObject{{"template", "task/{id}"}}}, {"base_branch", "main"}, {"restart_crashed", true}}));
    }
    auto set = team();
    set["settings"] = QJsonObject{{"branch_per_task", QJsonObject{{"template", "task/{id}"}}}, {"restart_crashed", true}};
    set["intakes"] = QJsonArray{QJsonObject{{"id", "i1"}, {"label", "health watch"}, {"hash", "x"}, {"created", 1793170000.0}}};
    lightPalette();
    TeamSettingsDialog dialog(&controller, parse(set), false);
    dialog.show();
    const auto close = qScopeGuard([&] { dialog.reject(); darkPalette(); });
    QVERIFY(dialog.findChild<QCheckBox *>("teamSettingsBranchPerTask")->isChecked());
    QVERIFY(dialog.findChild<QCheckBox *>("teamSettingsRestartCrashed")->isChecked());
    const auto rows = [&] {
        QStringList ids;
        for (auto *row : dialog.findChildren<QWidget *>("teamSettingsIntake")) if (row->isVisibleTo(&dialog)) ids << row->property("intake").toString();
        return ids;
    };
    QCOMPARE(rows(), QStringList{"i1"});
    // A key: created once, shown once.
    dialog.findChild<QLineEdit *>("teamSettingsIntakeLabel")->setText("deploy bot");
    dialog.findChild<QPushButton *>("teamSettingsIntakeCreate")->click();
    QTRY_COMPARE(calls(script(), "intake-key").size(), 1);
    QCOMPARE(calls(script(), "intake-key").first()["label"].toString(), QStringLiteral("deploy bot"));
    QCOMPARE(calls(script(), "intake-key").first()["team"].toString(), QStringLiteral("team-1"));
    QTRY_VERIFY(dialog.findChild<QWidget *>("teamSettingsIntakeKeyBox")->isVisible());
    QCOMPARE(dialog.findChild<QLineEdit *>("teamSettingsIntakeKey")->text(), QStringLiteral("zik_0123456789abcdef"));
    QTRY_COMPARE(rows(), (QStringList{"i1", "i2"}));
    lightShot(&dialog, "u51-team-settings-light");
    // Revoke asks; Cancel sends nothing, Revoke sends the id.
    std::function<void(bool, int)> answer = [&answer](bool revoke, int tries) {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box) { if (tries > 0) QTimer::singleShot(10, qApp, [&answer, revoke, tries] { answer(revoke, tries - 1); }); return; }
        for (auto *b : box->buttons()) if ((box->buttonRole(b) == QMessageBox::DestructiveRole) == revoke) { b->click(); return; }
    };
    const auto revokeOne = [&] {
        for (auto *row : dialog.findChildren<QWidget *>("teamSettingsIntake"))
            if (row->isVisibleTo(&dialog) && row->property("intake") == "i1") return row->findChild<QPushButton *>("teamSettingsIntakeRevoke");
        return static_cast<QPushButton *>(nullptr);
    };
    QTimer::singleShot(0, qApp, [&answer] { answer(false, 300); });
    revokeOne()->click();
    QCOMPARE(calls(script(), "intake-key").size(), 1);
    QTimer::singleShot(0, qApp, [&answer] { answer(true, 300); });
    revokeOne()->click();
    QTRY_COMPARE(calls(script(), "intake-key").size(), 2);
    QCOMPARE(calls(script(), "intake-key").last()["revoke"].toString(), QStringLiteral("i1"));
    QTRY_COMPARE(rows(), QStringList{"i2"});
    // Off: the branch setting goes as null, the untouched restart stays out.
    dialog.findChild<QCheckBox *>("teamSettingsBranchPerTask")->setChecked(false);
    QCOMPARE(dialog.changes(), (QJsonObject{{"branch_per_task", QJsonValue(QJsonValue::Null)}}));
}

// #581 N5: Landing (land_allowed) asks before it turns on, since a land stage's
// command may push; Cancel keeps it off, turning it off asks nothing. #581 N6:
// each member but the lead lists its models; Add and the chip's remove edit
// them, and Save sends the changed members in members_update with the
// changed settings in one `team settings` call.
void TestTeamsUi::teamSettingsAllowLandingAndEditMemberModels()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    auto set = team();
    auto members = set["members"].toArray();
    auto backend = members[1].toObject(); backend["models"] = QJsonArray{QJsonObject{{"model", "gpt-5.5-codex"}, {"effort", "high"}}}; members[1] = backend;
    set["members"] = members;
    // Answers the next confirmation with Allow landing or Cancel.
    int asked = 0;
    std::function<void(bool, int)> answer = [&answer, &asked](bool allow, int tries) {
        auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
        if (!box || box->objectName() != "teamSettingsLandConfirm") { if (tries > 0) QTimer::singleShot(10, qApp, [&answer, allow, tries] { answer(allow, tries - 1); }); return; }
        ++asked;
        QVERIFY(box->informativeText().contains("may merge or push"));
        if (allow) shot(box, "u48-landing-confirm-dark");
        for (auto *b : box->buttons()) if ((box->buttonRole(b) == QMessageBox::AcceptRole) == allow) { b->click(); return; }
    };
    TeamSettingsDialog dialog(&controller, parse(set), true);
    dialog.resize(560, 900); dialog.show();
    auto *land = dialog.findChild<QCheckBox *>("teamSettingsLandAllowed");
    auto *save = dialog.findChild<QPushButton *>("teamSettingsSave");
    QVERIFY(land && !land->isChecked());
    QVERIFY(dialog.changes().isEmpty() && dialog.memberChanges().isEmpty() && !save->isEnabled());
    // Cancel keeps landing off and nothing to save.
    QTimer::singleShot(0, qApp, [&answer] { answer(false, 300); });
    land->click();
    QCOMPARE(asked, 1);
    QVERIFY(!land->isChecked());
    QVERIFY(dialog.changes().isEmpty() && !save->isEnabled());
    // Allow turns it on.
    QTimer::singleShot(0, qApp, [&answer] { answer(true, 300); });
    land->click();
    QCOMPARE(asked, 2);
    QVERIFY(land->isChecked());
    QCOMPARE(dialog.changes(), (QJsonObject{{"land_allowed", true}}));
    QVERIFY(save->isEnabled());
    // Member models: Backend and Tester, never the lead.
    const auto rowOf = [&](const QString &id) -> QWidget * {
        for (auto *row : dialog.findChildren<QWidget *>("teamSettingsMemberModels")) if (row->property("member") == id) return row;
        return nullptr;
    };
    QStringList ids;
    for (auto *row : dialog.findChildren<QWidget *>("teamSettingsMemberModels")) ids << row->property("member").toString();
    QCOMPARE(ids, (QStringList{"backend", "tester"}));
    const auto chipsOf = [&](const QString &id) {
        QStringList models;
        for (auto *chip : rowOf(id)->findChildren<QPushButton *>("teamSettingsMemberModel")) if (!chip->property("model").toString().isEmpty() && chip->parent()) models << chip->property("model").toString();
        return models;
    };
    QCOMPARE(chipsOf("backend"), QStringList{"gpt-5.5-codex@high"});
    QVERIFY(chipsOf("tester").isEmpty());
    QVERIFY(rowOf("tester")->findChild<QLabel *>("teamSettingsMemberModelsNone"));
    // Tester may also switch to kimi-k2.5; the same choice twice adds nothing.
    auto *model = rowOf("tester")->findChild<QComboBox *>("teamSettingsMemberModelAdd");
    QCOMPARE(model->count(), int(Teams::knownModels("kimi").size()));   // the agent's known models are offered
    const auto addTester = [&] {
        model->setEditText("kimi-k2.5");
        rowOf("tester")->findChild<QPushButton *>("teamSettingsMemberModelAddButton")->click();
    };
    addTester();
    QTRY_COMPARE(chipsOf("tester"), QStringList{"kimi-k2.5"});
    addTester();
    QCOMPARE(dialog.memberChanges(), (QJsonArray{QJsonObject{{"id", "tester"}, {"models", QJsonArray{QJsonObject{{"model", "kimi-k2.5"}}}}}}));
    // Backend's chip removes its only choice: back to its launch model.
    rowOf("backend")->findChild<QPushButton *>("teamSettingsMemberModel")->click();
    QTRY_VERIFY(chipsOf("backend").isEmpty());
    const QJsonArray expected{QJsonObject{{"id", "backend"}, {"models", QJsonArray{}}},
                              QJsonObject{{"id", "tester"}, {"models", QJsonArray{QJsonObject{{"model", "kimi-k2.5"}}}}}};
    QCOMPARE(dialog.memberChanges(), expected);
    shot(&dialog, "u48-team-settings-landing-dark");
    save->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 1);
    QTRY_COMPARE(dialog.result(), int(QDialog::Accepted));
    const auto sent = calls(script(), "settings").first();
    QCOMPARE(sent["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(sent["settings"].toObject(), (QJsonObject{{"land_allowed", true}}));
    QCOMPARE(sent["members_update"].toArray(), expected);
    // Allowed already: off asks nothing; models alone send no settings key.
    auto allowed = set; allowed["settings"] = QJsonObject{{"land_allowed", true}};
    lightPalette();
    const auto restore = qScopeGuard([] { darkPalette(); });
    TeamSettingsDialog light(&controller, parse(allowed), false);
    light.resize(560, 900); light.show();
    QVERIFY(light.findChild<QCheckBox *>("teamSettingsLandAllowed")->isChecked());
    lightShot(&light, "u48-team-settings-landing-light");
    light.findChild<QCheckBox *>("teamSettingsLandAllowed")->click();
    QCOMPARE(asked, 2);
    QCOMPARE(light.changes(), (QJsonObject{{"land_allowed", false}}));
    light.findChild<QCheckBox *>("teamSettingsLandAllowed")->click();   // back to what the owner allowed: no question
    QCOMPARE(asked, 2);
    QVERIFY(light.changes().isEmpty());
    auto *lightModel = light.findChildren<QComboBox *>("teamSettingsMemberModelAdd").at(1);
    lightModel->setEditText("kimi-k2.5");
    emit lightModel->lineEdit()->returnPressed();
    QTRY_VERIFY(light.findChild<QPushButton *>("teamSettingsSave")->isEnabled());
    light.findChild<QPushButton *>("teamSettingsSave")->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 2);
    QVERIFY(!calls(script(), "settings").last().contains("settings"));
    QCOMPARE(calls(script(), "settings").last()["members_update"].toArray(),
             (QJsonArray{QJsonObject{{"id", "tester"}, {"models", QJsonArray{QJsonObject{{"model", "kimi-k2.5"}}}}}}));
}

// U49: the member menu adds more members of a role (scale) and drains one;
// a draining member is marked and its drain can be cancelled.
void TestTeamsUi::memberMenuScalesAndDrains()
{
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
    const auto draining = [&] {
        for (auto *mark : workspace.findChildren<QWidget *>("teamWorkspaceMemberDraining")) if (mark->isVisibleTo(&workspace)) return true;
        return false;
    };
    QTRY_VERIFY(!workspace.findChildren<QFrame *>("teamWorkspaceMember").isEmpty());
    QVERIFY(!draining());
    // Scale: "Add more Backend…" asks how many and sends the member's agent and model.
    bool asked = false;
    std::function<void(int)> lookCount = [&](int tries) {
        QTimer::singleShot(10, qApp, [&, tries] {
            auto *input = qobject_cast<QInputDialog *>(QApplication::activeModalWidget());
            if (!input) { if (tries > 0) lookCount(TestWait::busy() ? tries : tries - 1); return; }
            asked = true;
            QVERIFY(input->labelText().contains("Backend"));
            input->setIntValue(2); input->accept();
        });
    };
    lookCount(1000);
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        QStringList names; for (auto *action : menu->actions()) names << action->objectName();
        QVERIFY(names.contains("teamWorkspaceMemberDrain") && !names.contains("teamWorkspaceMemberUndrain"));
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale") { QCOMPARE(action->text(), QStringLiteral("Add more Backend…")); action->trigger(); }
    });
    QTRY_VERIFY(asked);
    QTRY_COMPARE(calls(script(), "scale").size(), 1);
    QCOMPARE(calls(script(), "scale").first()["role"].toString(), QStringLiteral("Backend"));
    QCOMPARE(calls(script(), "scale").first()["agent"].toString(), QStringLiteral("codex"));
    QCOMPARE(calls(script(), "scale").first()["model"].toString(), QStringLiteral("gpt-5.5-codex"));
    QCOMPARE(calls(script(), "scale").first()["count"].toInt(), 2);
    // Drain asks first.
    armConfirm(&panel, false);
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberDrain") action->trigger();
    });
    QTRY_COMPARE(calls(script(), "drain").size(), 1);
    QCOMPARE(calls(script(), "drain").first()["member"].toString(), QStringLiteral("backend"));
    QVERIFY(!calls(script(), "drain").first().contains("draining"));
    // Draining: the card says so, and Cancel drain sends draining false.
    auto drained = team();
    auto members = drained["members"].toArray();
    auto backend = members[1].toObject(); backend["draining"] = true; members[1] = backend;
    drained["members"] = members;
    drained["revision"] = 13;
    setTeam(drained);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 13 && t->member("backend")->draining; })());
    QTRY_VERIFY(draining());
    shot(&workspace, "u49-member-draining-dark");
    lightPalette(); workspace.setTheme(false); panel.setTheme(false);
    lightShot(&workspace, "u49-member-draining-light");
    darkPalette(); workspace.setTheme(true); panel.setTheme(true);
    inspectMemberMenu(&workspace, "backend", [](QMenu *menu) {
        for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberUndrain") action->trigger();
    });
    QTRY_COMPARE(calls(script(), "drain").size(), 2);
    QCOMPARE(calls(script(), "drain").last()["draining"].toBool(true), false);
    // A full team cannot scale.
    auto full = drained; full["settings"] = QJsonObject{{"max_members", 2}}; full["revision"] = 14;
    setTeam(full);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 14 && t->maxMembers == 2; })());
    inspectMemberMenu(&workspace, "tester", [](QMenu *menu) {
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale") { QVERIFY(!action->isEnabled()); QVERIFY(action->toolTip().contains("2 members")); }
    });
    // #581 L2: the lead takes a seat, as hgs's scale counts it; a removed member does not.
    int seated = 0;
    for (const auto &value : drained["members"].toArray()) seated += value.toObject()["state"].toString() != QLatin1String("removed");
    QVERIFY(seated >= 3);
    auto exact = drained; exact["settings"] = QJsonObject{{"max_members", seated}}; exact["revision"] = 15;
    QCOMPARE(scaleRoom(parse(exact)), 0);
    auto spare = exact; spare["settings"] = QJsonObject{{"max_members", seated + 2}};
    QCOMPARE(scaleRoom(parse(spare)), 2);
    auto gone = exact; auto seats = gone["members"].toArray(); auto last = seats.last().toObject(); last["state"] = "removed"; seats[seats.size() - 1] = last; gone["members"] = seats;
    QCOMPARE(scaleRoom(parse(gone)), 1);
    setTeam(exact);
    QTRY_VERIFY(([&] { controller.refresh(); const auto *t = controller.watched(); return t && t->revision == 15 && t->maxMembers == seated; })());
    inspectMemberMenu(&workspace, "tester", [seated](QMenu *menu) {
        for (auto *action : menu->actions())
            if (action->objectName() == "teamWorkspaceMemberScale")
                QVERIFY2(!action->isEnabled() && action->toolTip().contains(QStringLiteral("%1 members").arg(seated)), qPrintable(action->toolTip()));
    });
}

// U36 (#533): a catalog with the server's built-in presets (catalog.rs
// builtin_pipelines), a template that names Code as its default and a team's
// own Docs template without one.
QJsonObject pipelineCatalog()
{
    return QJsonDocument::fromJson(R"({"builtin":false,
 "catalog":{"version":14,
  "roles":[{"id":"lead","name":"Lead","agent":"claude","prompt":"You lead."},
           {"id":"frontend","name":"Frontend","agent":"codex","model":"gpt-6.1-sol","prompt":"Build the UI."},
           {"id":"reviewer","name":"Reviewer","agent":"claude","model":"claude-sonnet-4-6","prompt":"Review it."},
           {"id":"tester","name":"Tester","agent":"kimi","model":"kimi-k2.5","prompt":"Test it."}],
  "templates":[{"id":"feature","name":"Feature delivery","roster":[{"role":"frontend","count":1},{"role":"reviewer","count":1},{"role":"tester","count":1}],
                "tasks":[],"planning":"lead","approval":"review","max_members":6,"pipeline":"code"},
               {"id":"docs","name":"Documentation","roster":[{"role":"reviewer","count":1}],"tasks":[],"planning":"lead","approval":"review","max_members":3}],
  "routing":[],
  "pipelines":[{"id":"code","name":"Code: review, test, lead","stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],"max_rounds":3,"folder":"hold","accept":"lead"},
               {"id":"docs","name":"Docs: review, auto","stages":[{"kind":"review","role":"reviewer"}],"max_rounds":2,"folder":"hold","accept":"auto"},
               {"id":"check","name":"Check: lead","stages":[],"max_rounds":3,"folder":"hold","accept":"lead"}]},
 "draft":{"base":0,"proposals":[]}})").object();
}

void writeCatalog(const QString &script, const QJsonObject &value)
{
    QVERIFY(FakeCli::replace(script + ".catalog", QJsonDocument(value).toJson(QJsonDocument::Compact)));
}

bool clickSegment(QWidget *segments, const QString &value)
{
    if (!segments) return false;
    for (auto *b : segments->findChildren<QPushButton *>()) if (b->property("segment") == value) { if (!b->isEnabled()) return false; b->click(); return true; }
    return false;
}

void TestTeamsUi::catalogPipelinesAreEditedAndSaved()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { QSettings().remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
    // Opt-in: a catalog without presets saves without them, and its roles and
    // templates gain no pipeline field.
    {
        const auto plain = parseCatalog(catalog()).json();
        QVERIFY(!plain.contains("pipelines"));
        for (const auto &key : {"roles", "templates"})
            for (const auto &item : plain[key].toArray()) QVERIFY(!item.toObject().contains("pipeline"));
    }
    writeCatalog(script(), pipelineCatalog());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    editor.resize(1320, 1180); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 4);
    QVERIFY(clickSegment(editor.findChild<QWidget *>("teamCatalogTabs"), "pipelines"));
    auto *list = editor.findChild<QListWidget *>("teamCatalogPipelines");
    QTRY_COMPARE(list->count(), 4);   // three presets, then None
    QCOMPARE(list->item(3)->data(Qt::UserRole).toString(), QStringLiteral("none"));
    QVERIFY(list->isVisible());
    QCOMPARE(editor.findChild<QLabel *>("teamPipelineTitle")->text(), QStringLiteral("Code: review, test, lead"));
    QVERIFY(editor.findChild<QLabel *>("teamPipelineUsedBy")->text().contains("Feature delivery"));
    // The flow in the list: Review › Test › Lead; Docs ends in Auto.
    QStringList flow;
    for (auto *chip : list->itemWidget(list->item(0))->findChildren<QWidget *>("teamPipelineStageChip")) flow << chip->property("kind").toString();
    QCOMPARE(flow, (QStringList{"review", "test", "lead"}));
    // The test stage's details are open: the task's folder holds it.
    auto *hold = editor.findChild<QRadioButton *>("teamPipelineFolderHold");
    QVERIFY(hold && hold->isChecked());
    shot(&editor, "u36-pipelines-dark");
    editor.setDark(false); lightShot(&editor, "u36-pipelines-light"); darkPalette(); editor.setDark(true);
    // A detached checkout, Auto, two rounds.
    editor.findChild<QRadioButton *>("teamPipelineFolderCheckout")->click();
    QTRY_VERIFY(editor.findChild<QRadioButton *>("teamPipelineFolderCheckout") && editor.findChild<QRadioButton *>("teamPipelineFolderCheckout")->isChecked());
    QVERIFY(clickSegment(editor.findChild<QWidget *>("teamPipelineAccept"), "auto"));
    QTRY_VERIFY(editor.findChild<QWidget *>("teamPipelineRounds"));
    editor.findChild<QWidget *>("teamPipelineRounds")->findChild<QPushButton *>("teamStepperMinus")->click();
    // The review is pinned to a member.
    editor.findChildren<QPushButton *>("teamPipelineStageOpen").at(0)->click();
    QTRY_VERIFY(editor.findChild<QWidget *>("teamPipelineStageWho"));
    QVERIFY(clickSegment(editor.findChild<QWidget *>("teamPipelineStageWho"), "member"));
    QTRY_VERIFY(editor.findChild<QLineEdit *>("teamPipelineStageMember"));
    QVERIFY(editor.findChild<QWidget *>("teamPipelineProblemLine"));   // a pinned stage with no member yet
    editor.findChild<QLineEdit *>("teamPipelineStageMember")->setText("reviewer2");
    emit editor.findChild<QLineEdit *>("teamPipelineStageMember")->editingFinished();
    QTRY_VERIFY(!editor.findChild<QWidget *>("teamPipelineProblemLine"));
    // U47: a command stage Zerus runs, with its time limit.
    editor.findChild<QPushButton *>("teamPipelineAddStage")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineStageKind").size(), 3);
    QVERIFY(clickSegment(editor.findChildren<QWidget *>("teamPipelineStageKind").at(2), "command"));
    QTRY_VERIFY(editor.findChild<QLineEdit *>("teamPipelineStageCommand"));
    QVERIFY(editor.findChild<QLabel *>("teamPipelineStageZerus"));
    QVERIFY(editor.findChild<QWidget *>("teamPipelineProblemLine"));   // no command yet
    editor.findChild<QLineEdit *>("teamPipelineStageCommand")->setText("ctest --output-on-failure");
    emit editor.findChild<QLineEdit *>("teamPipelineStageCommand")->editingFinished();
    QTRY_VERIFY(editor.findChild<QSpinBox *>("teamPipelineStageTimeout"));
    QCOMPARE(editor.findChild<QSpinBox *>("teamPipelineStageTimeout")->value(), 30);   // the server's default
    editor.findChild<QSpinBox *>("teamPipelineStageTimeout")->setValue(45);
    emit editor.findChild<QSpinBox *>("teamPipelineStageTimeout")->editingFinished();
    // U48: Land only on the last stage, with its exit codes and the owner's rule.
    editor.findChild<QPushButton *>("teamPipelineAddStage")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineStageKind").size(), 4);
    QVERIFY(!clickSegment(editor.findChildren<QWidget *>("teamPipelineStageKind").at(2), "land"));
    QVERIFY(clickSegment(editor.findChildren<QWidget *>("teamPipelineStageKind").at(3), "land"));
    QTRY_VERIFY(editor.findChild<QWidget *>("teamPipelineLandRuleLine"));
    QVERIFY(editor.findChild<QLabel *>("teamPipelineLandRule")->text().contains("land_allowed"));
    editor.findChild<QLineEdit *>("teamPipelineStageCommand")->setText("scripts/land.sh --squash");
    emit editor.findChild<QLineEdit *>("teamPipelineStageCommand")->editingFinished();
    QTRY_VERIFY(editor.findChild<QPushButton *>("teamPipelineAddExit"));
    editor.findChild<QPushButton *>("teamPipelineAddExit")->click();
    QTRY_COMPARE(editor.findChildren<QComboBox *>("teamPipelineExitOutcome").size(), 1);
    editor.findChild<QPushButton *>("teamPipelineAddExit")->click();
    QTRY_COMPARE(editor.findChildren<QComboBox *>("teamPipelineExitOutcome").size(), 2);
    auto *outcome = editor.findChildren<QComboBox *>("teamPipelineExitOutcome").at(1);
    outcome->setCurrentIndex(outcome->findData("held")); emit outcome->activated(outcome->currentIndex());
    shot(&editor, "u36-pipelines-land-dark");
    editor.setDark(false); lightShot(&editor, "u36-pipelines-land-light"); darkPalette(); editor.setDark(true);
    // A new stage goes before the land stage, which stays last; then it goes.
    editor.findChild<QPushButton *>("teamPipelineAddStage")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineStageKind").size(), 5);
    QVERIFY(!editor.findChildren<QToolButton *>("teamPipelineStageDown").at(3)->isEnabled());
    editor.findChildren<QToolButton *>("teamPipelineStageDelete").at(3)->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineStageKind").size(), 4);
    // Defaults: Documentation takes Docs; Frontend gets this preset as its own.
    QComboBox *docs = nullptr;
    for (auto *box : editor.findChildren<QComboBox *>("teamPipelineTemplateDefault")) if (box->property("template") == "docs") docs = box;
    QVERIFY(docs);
    docs->setCurrentIndex(docs->findData("docs")); emit docs->activated(docs->currentIndex());
    QTRY_VERIFY(editor.findChild<QComboBox *>("teamPipelineAddRole"));
    auto *addRole = editor.findChild<QComboBox *>("teamPipelineAddRole");
    addRole->setCurrentIndex(addRole->findData("frontend")); emit addRole->activated(addRole->currentIndex());
    QTRY_COMPARE(editor.findChildren<QComboBox *>("teamPipelineRoleDefault").size(), 1);
    // Check is deleted, after the confirmation.
    list->setCurrentRow(2);
    QTRY_COMPARE(editor.findChild<QLabel *>("teamPipelineTitle")->text(), QStringLiteral("Check: lead"));
    editor.findChild<QPushButton *>("teamPipelineDelete")->click();
    QTRY_COMPARE(list->count(), 3);
    // Nothing is sent until Save, which writes the next version.
    QVERIFY(calls(script(), "catalog set").isEmpty());
    QVERIFY(editor.findChild<QLabel *>("teamCatalogStatus")->text().contains("unsaved"));
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave"); QVERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto input = calls(script(), "catalog set").first();
    QCOMPARE(input["version"].toInt(), 14);
    const auto sent = input["catalog"].toObject();
    const auto code = QJsonDocument::fromJson(R"({"id":"code","name":"Code: review, test, lead","stages":[
        {"kind":"review","member":"reviewer2"},{"kind":"test","role":"tester"},
        {"kind":"command","command":{"command":"ctest --output-on-failure","expect_exit":0,"timeout_minutes":45}},
        {"kind":"land","command":{"command":"scripts/land.sh --squash","expect_exit":0,"timeout_minutes":30},"exits":{"0":"accepted","1":"held"}}],
        "max_rounds":2,"folder":"checkout","accept":"auto"})").object();
    const auto pipelines = sent["pipelines"].toArray();
    QCOMPARE(pipelines.size(), 2);
    QCOMPARE(pipelines[0].toObject(), code);
    QCOMPARE(pipelines[1].toObject(), pipelineCatalog().value("catalog").toObject().value("pipelines").toArray()[1].toObject());
    const auto templates = sent["templates"].toArray();
    QCOMPARE(templates[0].toObject()["pipeline"].toString(), QStringLiteral("code"));
    QCOMPARE(templates[1].toObject()["pipeline"].toString(), QStringLiteral("docs"));
    for (const auto &role : sent["roles"].toArray()) {
        const auto o = role.toObject();
        if (o["id"] == "frontend") QCOMPARE(o["pipeline"].toString(), QStringLiteral("code"));
        else QVERIFY2(!o.contains("pipeline"), qPrintable(o["id"].toString()));
    }
}

// U36: the designer's pipeline proposals in the draft: the tab's count, the
// marks in the list, a plate to keep or reject inside the editor and the
// changed fields old → new; the designer page's Show in catalog opens them.
void TestTeamsUi::catalogPipelineProposalsHavePlates()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { QSettings().remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
    auto drafted = pipelineCatalog();
    const auto presets = drafted.value("catalog").toObject().value("pipelines").toArray();
    auto checkout = presets[0].toObject(); checkout["folder"] = "checkout";
    auto docs = presets[1].toObject(); docs["id"] = "docs-lite"; docs["name"] = "Docs: review, auto"; docs["max_rounds"] = 1;
    drafted["draft"] = QJsonObject{{"base", 14}, {"proposals", QJsonArray{
        QJsonObject{{"id", "pp1"}, {"kind", "pipeline"}, {"action", "change"}, {"target", "code"}, {"title", "Code: the test works on a detached checkout"},
                    {"reason", "Frontend waited 52 min on average while the test held its folder (7 tasks)."}, {"value", checkout}, {"state", "proposed"}},
        QJsonObject{{"id", "pp2"}, {"kind", "pipeline"}, {"action", "add"}, {"target", "docs-lite"}, {"title", "Pipeline · Docs: review, auto"},
                    {"reason", "9 of the last 11 docs tasks were accepted unchanged by the lead."}, {"value", docs}, {"state", "kept"}}}}};
    writeCatalog(script(), drafted);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    // The model: a kept pipeline proposal applies, an undecided one only with them.
    QTRY_VERIFY(([&] { controller.refreshCatalog(); return controller.catalogReady() && controller.catalog().proposals.size() == 2; })());
    QVERIFY(withProposals(controller.catalog(), false).pipeline("docs-lite"));
    QCOMPARE(withProposals(controller.catalog(), false).pipeline("code")->folder, QStringLiteral("hold"));
    QCOMPARE(withProposals(controller.catalog(), true).pipeline("code")->folder, QStringLiteral("checkout"));
    TeamDesignerPage page(&controller);
    page.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    page.resize(1600, 1000); page.show();
    QTRY_VERIFY(page.findChildren<QPushButton *>("teamProposalShow").size() == 2);
    auto *editor = dynamic_cast<TeamCatalogEditor *>(page.findChild<QWidget *>("teamCatalogEditor"));
    QVERIFY(editor);
    QTRY_VERIFY(([&] { for (auto *b : editor->findChild<QWidget *>("teamCatalogTabs")->findChildren<QPushButton *>())
                           if (b->property("segment") == "pipelines") return b->text().contains("2"); return false; })());
    page.findChildren<QPushButton *>("teamProposalShow").at(0)->click();
    auto *list = editor->findChild<QListWidget *>("teamCatalogPipelines");
    QTRY_VERIFY(list->isVisible());
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), QStringLiteral("code"));
    QCOMPARE(list->count(), 5);   // the added preset shows in the draft
    QStringList marks;
    for (int i = 0; i < list->count(); ++i) for (auto *m : list->itemWidget(list->item(i))->findChildren<QLabel *>("teamCatalogMark")) marks << m->text();
    QCOMPARE(marks, (QStringList{"CHANGED", "NEW · kept"}));
    // The plate inside the editor, the changed field, every field read only.
    auto *card = editor->findChild<QWidget *>("teamPipelineEditor");
    QVERIFY(card);
    QVERIFY(editor->findChild<QFrame *>("teamProposalPlate"));
    const auto diff = editor->findChildren<QLabel *>("teamPipelineDiffLine");
    QCOMPARE(diff.size(), 1);
    QVERIFY(diff.first()->text().contains("Test works in") && diff.first()->text().contains("Detached checkout"));
    QVERIFY(editor->findChild<QLineEdit *>("teamPipelineName")->isReadOnly());
    QVERIFY(!editor->findChild<QPushButton *>("teamPipelineAddStage")->isEnabled());
    QVERIFY(editor->findChild<QRadioButton *>("teamPipelineFolderCheckout")->isChecked());
    shot(&page, "u36-designer-dark");
    page.setDark(false); lightShot(&page, "u36-designer-light"); darkPalette(); page.setDark(true);
    // Keep on the editor's plate decides that proposal, through the controller.
    editor->findChild<QFrame *>("teamProposalPlate")->findChild<QPushButton *>("teamProposalKeep")->click();
    QTRY_COMPARE(calls(script(), "catalog decide").size(), 1);
    QCOMPARE(calls(script(), "catalog decide").first()["proposal"].toString(), QStringLiteral("pp1"));
    QCOMPARE(calls(script(), "catalog decide").first()["decision"].toString(), QStringLiteral("keep"));
    // Kept: the plate offers Undo; an added preset has no old value to show.
    QTRY_VERIFY(editor->findChild<QFrame *>("teamProposalPlate") && editor->findChild<QFrame *>("teamProposalPlate")->findChild<QPushButton *>("teamProposalUndo"));
    page.findChildren<QPushButton *>("teamProposalShow").at(1)->click();
    QTRY_COMPARE(list->currentItem()->data(Qt::UserRole).toString(), QStringLiteral("docs-lite"));
    QVERIFY(editor->findChildren<QLabel *>("teamPipelineDiffLine").isEmpty());   // an added preset has no old value
}

// U36: Try it walks a task through a preset with today's members: the first
// free member of each stage's role (never the implementer), where it works,
// the folder hold, a single Tester and a land stage without land_allowed.
void TestTeamsUi::catalogPipelineTryWalksASampleTask()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { QSettings().remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
    const auto c = parseCatalog(pipelineCatalog());
    Team team; team.folder = "/work/orbit";
    const auto member = [&](const QString &id, const QString &role, const QString &agent, const QString &model) {
        Member m; m.id = id; m.role = role; m.agent = agent; m.model = model; m.lead = id == "lead"; m.directory = "/work/orbit-team/" + id;
        team.members.append(m);
    };
    member("lead", "Lead", "claude", "claude-opus"); member("frontend", "Frontend", "codex", "gpt-6.1-sol");
    member("reviewer", "Reviewer", "claude", "claude-sonnet-4-6"); member("build-01", "Tester", "kimi", "kimi-k2.5");
    Task busy; busy.id = 5; busy.title = "Cart"; busy.assignee = "build-01"; busy.status = "in_progress";
    Attempt working; working.n = 1; working.member = "build-01"; working.state = "delivered"; busy.attempts = {working};
    Task task; task.id = 4; task.title = "Address validation"; task.assignee = "frontend"; task.status = "in_progress";
    task.attempts.append(Attempt{}); task.attempts.first().n = 1;
    team.tasks = {busy, task};
    auto walked = TeamCatalogEditor::walk(c, *c.pipeline("code"), team, task);
    QStringList kinds, members;
    for (const auto &step : walked.steps) { kinds << step.kind; members << step.member; }
    QCOMPARE(kinds, (QStringList{"implement", "review", "test", "lead"}));
    QCOMPARE(members, (QStringList{"frontend", "reviewer", "build-01", "lead"}));
    QVERIFY(walked.steps[1].detail.contains("free now"));
    QVERIFY(walked.steps[1].where.contains("/work/orbit-team/frontend") && walked.steps[1].where.contains("attempt 2"));
    QVERIFY(walked.steps[2].detail.contains("busy with #5"));
    QVERIFY(walked.steps[2].where.contains("Holds the folder"));
    QCOMPARE(walked.warnings.size(), 1);
    QVERIFY(walked.warnings.first().startsWith("One Tester"));
    // The implementer never takes its own stage: with Frontend as the only
    // reviewer the review waits.
    Pipeline own = *c.pipeline("docs"); own.stages[0].role = "frontend";
    walked = TeamCatalogEditor::walk(c, own, team, task);
    QVERIFY(walked.steps[1].member.isEmpty());
    QVERIFY(walked.warnings.join('\n').contains("No Frontend besides the implementer"));
    QCOMPARE(walked.steps.last().kind, QStringLiteral("auto"));
    // A detached checkout frees the implementer; a land stage decides itself
    // and warns while the team's land_allowed is off.
    Pipeline land = *c.pipeline("code"); land.folder = "checkout";
    PipelineStage step; step.kind = "land"; step.hasCommand = true; step.command = "scripts/land.sh"; step.exits = {{"0", "accepted"}, {"2", "returned"}};
    land.stages.append(step);
    walked = TeamCatalogEditor::walk(c, land, team, task);
    QCOMPARE(walked.steps.last().kind, QStringLiteral("land"));
    QVERIFY(walked.steps[2].where.contains("is free while it runs"));
    QVERIFY(walked.steps.last().where.contains("exit 2: returned to the implementer"));
    QVERIFY(walked.warnings.join('\n').contains("land_allowed is off"));
    team.settingsJson = QJsonObject{{"land_allowed", true}};
    QVERIFY(!TeamCatalogEditor::walk(c, land, team, task).warnings.join('\n').contains("land_allowed"));
    // In the tab: Try shows the walk under the editor, and an edit walks again.
    writeCatalog(script(), pipelineCatalog());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1320, 1180); editor.show();
    QTRY_COMPARE(editor.findChild<QListWidget *>("teamCatalogRoles")->count(), 4);
    QVERIFY(clickSegment(editor.findChild<QWidget *>("teamCatalogTabs"), "pipelines"));
    QTRY_VERIFY(editor.findChild<QPushButton *>("teamPipelineTry"));
    QVERIFY(editor.findChildren<QWidget *>("teamPipelineTryStep").isEmpty());
    editor.findChild<QPushButton *>("teamPipelineTry")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineTryStep").size(), 4);
    QVERIFY(calls(script(), "catalog set").isEmpty());   // nothing is sent
    editor.findChild<QPushButton *>("teamPipelineAddStage")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineTryStep").size(), 5);
    editor.findChildren<QToolButton *>("teamPipelineStageDelete").last()->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamPipelineTryStep").size(), 4);
    editor.findChild<QScrollArea *>("teamCatalogEditorArea")->verticalScrollBar()->setValue(100000);
    shot(&editor, "u36-pipelines-try-dark");
    editor.setDark(false); lightShot(&editor, "u36-pipelines-try-light"); darkPalette(); editor.setDark(true);
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
void TestTeamsUi::lanesGroupACollapseInEveryView()
{
    // U27: a phase's tasks group by lane, in lane order, each lane a labelled
    // row that collapses; the tasks without a lane fall back to their layers.
    // Lanes show in Graph, Board and Layers.
    auto t = phasedTeam();
    auto tasks = t["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        // Phase 3 keeps #5; give it and two new tasks lanes, and leave one alone.
        if (task["id"].toInt() == 5) task["lane"] = QJsonObject{{"n", 1}, {"label", "Checkpoint 1"}};
        tasks[i] = task;
    }
    tasks.append(QJsonObject{{"id", 6}, {"phase", 3}, {"title", "Screenshots"}, {"assignee", "tester"}, {"status", "draft"},
                             {"lane", QJsonObject{{"n", 1}, {"label", "Checkpoint 1"}}}});
    tasks.append(QJsonObject{{"id", 7}, {"phase", 3}, {"title", "Review"}, {"assignee", "backend"}, {"status", "draft"},
                             {"lane", QJsonObject{{"n", 2}, {"label", "Review"}}}});
    tasks.append(QJsonObject{{"id", 8}, {"phase", 3}, {"title", "No lane work"}, {"assignee", "backend"}, {"status", "draft"}});
    t["tasks"] = tasks;
    auto phases = t["phases"].toArray();
    auto polish = phases[1].toObject();
    polish["tasks"] = QJsonArray{5, 6, 7, 8};
    phases[1] = polish;
    t["phases"] = phases;
    t["pending_phases"] = QJsonArray{3};
    setTeam(t);
    QSettings().remove("ui/collapsed/teamLane/team-1/3/1");
    QSettings().remove("ui/collapsed/teamLane/team-1/3/2");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1300, 1000);
    workspace.show();
    controller.refresh();
    workspace.showTeam("team-1");
    const auto laneHeaders = [&workspace](int phase) {
        QList<QToolButton *> found;
        for (auto *header : workspace.findChildren<QToolButton *>("teamLaneHeader"))
            if (header->property("phase").toInt() == phase && header->isVisible()) found << header;
        return found;
    };
    const auto laneBody = [&workspace](int phase, int lane) -> QWidget * {
        for (auto *body : workspace.findChildren<QWidget *>("teamLaneBody"))
            if (body->property("phase").toInt() == phase && body->property("lane").toInt() == lane && body->isVisible()) return body;
        return nullptr;
    };
    QTRY_VERIFY_WITH_TIMEOUT(laneHeaders(3).size() == 3, 20000);
    QCOMPARE(laneHeaders(3)[0]->property("lane").toInt(), 1);
    QVERIFY2(laneHeaders(3)[0]->text().contains(QStringLiteral("Checkpoint 1")), qPrintable(laneHeaders(3)[0]->text()));
    QCOMPARE(laneHeaders(3)[2]->property("lane").toInt(), 0);
    QVERIFY2(laneHeaders(3)[2]->text().contains(QStringLiteral("No lane")), qPrintable(laneHeaders(3)[2]->text()));
    QVERIFY(laneBody(3, 1));
    // Graph: the lane's graph shows only its own tasks.
    auto *graph = laneBody(3, 1)->findChild<QWidget *>("teamPhaseGraph");
    QVERIFY(graph);
    // Collapsing a lane hides its body and remembers the choice.
    laneHeaders(3)[0]->click();
    QTRY_VERIFY(!laneBody(3, 1));
    QCOMPARE(QSettings().value("ui/collapsed/teamLane/team-1/3/1").toBool(), true);
    workspace.render();
    QTest::qWait(150);
    shot(&workspace, "u27-lanes-collapsed-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTest::qWait(250);
    shot(&workspace, "u27-lanes-collapsed-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    QTest::qWait(150);
    QTRY_VERIFY(laneHeaders(3).size() == 3);
    laneHeaders(3)[0]->click();
    QTRY_VERIFY(laneBody(3, 1));
    workspace.render();
    QTest::qWait(150);
    shot(&workspace, "u27-lanes-expanded-dark");
    lightPalette();
    workspace.setTheme(false);
    workspace.render();
    QTest::qWait(250);
    shot(&workspace, "u27-lanes-expanded-light");
    darkPalette();
    workspace.setTheme(true);
    workspace.render();
    QTest::qWait(150);
    // Board and Layers group the phase the same way.
    QPointer<QWidget> views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *button : views->findChildren<QPushButton *>())
        if (button->property("segment") == "board") { button->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceBoard"));
    // Rendering another view replaces the toolbar too. Track its destruction
    // and fetch the current toolbar before the next click.
    QTRY_VERIFY(views.isNull());
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 2); })(), 20000);
    const auto cards = [&workspace](const QList<int> &ids) {
        int n = 0;
        for (auto *card : workspace.findChildren<QFrame *>("teamBoardCard"))
            if (ids.contains(card->property("task").toInt())) ++n;
        return n;
    };
    QCOMPARE(cards({5, 6}), 2);
    QCOMPARE(cards({7}), 1);
    QCOMPARE(cards({8}), 1);
    views = workspace.findChild<QWidget *>("teamWorkspaceView");
    QVERIFY(views);
    for (auto *button : views->findChildren<QPushButton *>())
        if (button->property("segment") == "layers") { button->click(); break; }
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceLayers"));
    QTRY_VERIFY(views.isNull());
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 2); })(), 20000);
    // The new layers view groups the same lanes after replacing the board.
    QTRY_VERIFY_WITH_TIMEOUT(([&] { const auto headers = laneHeaders(3); return headers.size() == 3 && laneBody(3, 1) && laneBody(3, 2); })(), 20000);
}

// U39 (owner, 2026-10-28): an accepted phase that has not started (here
// queued) carries Discard phase… in its menu; it asks first and sends
// its number.
void TestTeamsUi::aQueuedPhaseCanBeDiscarded()
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
void TestTeamsUi::aQueuedPhaseCanReturnToDraft()
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
    auto *view = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
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

// F8, U57: the lead's open proposals are cards in the coordinator's
// conversation, drawn like the member permission card. Deny carries its
// reason; remove and swap ask first, a cancel sends nothing; every answer
// carries the proposal's hash and goes out once.
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
    QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add Designer (claude, sonnet)"));
    QCOMPARE(cardOf("p2")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Swap Backend to codex, gpt-5.5"));
    QCOMPARE(cardOf("p3")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Remove Tester"));
    QCOMPARE(cardOf("p3")->property("hash").toString(), QStringLiteral("h3"));
    QVERIFY(cardOf("p1")->findChild<QLabel *>("teamProposalBy")->text().startsWith("Lead asks"));
    QCOMPARE(cardOf("p1")->findChild<QLabel *>("teamProposalReason")->text(), QStringLiteral("Lead: The UI needs a designer"));
    // The member permission card's parts: a Deny reason, Deny and Allow.
    for (const char *name : {"teamProposalDenyReason", "teamProposalDeny", "teamProposalAllow"}) QVERIFY(cardOf("p1")->findChild<QWidget *>(name));
    QVERIFY(cardOf("p1")->findChild<QPushButton *>("teamProposalDeny")->property("danger").toBool());
    QVERIFY(cardOf("p1")->findChild<QPushButton *>("teamProposalAllow")->property("primary").toBool());
    shot(&workspace, "u57-proposal-cards");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u57-proposal-cards-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
    // Until the store has taken it, the card's buttons stay off.
    QTRY_VERIFY(cardOf("p2") && !cardOf("p2")->findChild<QPushButton *>("teamProposalAllow")->isEnabled());
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
void TestTeamsUi::proposalCardsNameTheChangeAndAStaleCardSendsNothing()
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
    QCOMPARE(linesOf("p1"), (QStringList{"Designer (claude, sonnet)", "Writer (codex)"}));
    QCOMPARE(cardOf("p2")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Add 2 tasks to phase 2 (QA fixes)"));
    QCOMPARE(linesOf("p2"), (QStringList{"Saved card list, for Backend", "Empty state"}));
    QCOMPARE(cardOf("p3")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Cancel 1 task"));
    QCOMPARE(linesOf("p3").size(), 1);
    QVERIFY2(linesOf("p3").first().startsWith("#4 Unit tests ("), qPrintable(linesOf("p3").first()));
    QCOMPARE(cardOf("p4")->findChild<QLabel *>("teamProposalWhat")->text(), QStringLiteral("Discard phase 3 (Polish)"));
    QCOMPARE(linesOf("p4").size(), 1);
    QVERIFY2(linesOf("p4").first().startsWith("#5 Code review ("), qPrintable(linesOf("p4").first()));
    shot(&workspace, "u57-proposal-kinds");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u57-proposal-kinds-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
    QTRY_VERIFY(noticeOf("p2").contains("nothing was applied"));
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

// U38 (#665): Profile beside Graph, Board and Layers, from hgs team profile
// and hgs team spans: the summary, the member lanes with the real and planned
// critical paths, the top losses (a click opens the task and marks it), the
// tokens per task, the members table, the phase compare and Export JSON.
void TestTeamsUi::profileViewShowsThePhaseAndItsLosses()
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
    shot(&workspace, "u38-profile-time");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u38-profile-time-light");
    darkPalette(); workspace.setTheme(true); workspace.render();
    // By tokens: the rework of #3.
    view->setLossOrder("tokens");
    QTRY_COMPARE(workspace.findChildren<QFrame *>("teamProfileLoss").size(), 1);
    QCOMPARE(workspace.findChild<QLabel *>("teamProfileLossValue")->text(), QStringLiteral("900k"));
    view->setLossOrder("time");
    // Tokens per task.
    view->setTab("tokens");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamProfileTokens"));
    shot(&workspace, "u38-profile-tokens");
    workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-tokens-light"); darkPalette(); workspace.setTheme(true); workspace.render();
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
    shot(&workspace, "u38-profile-members");
    workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-members-light"); darkPalette(); workspace.setTheme(true); workspace.render();
    // Compare: phases 1 and 2 side by side.
    view->setTab("compare");
    QTRY_VERIFY(!calls(script(), "profile").isEmpty() && calls(script(), "profile").last().contains("compare"));
    QCOMPARE(calls(script(), "profile").last()["compare"].toArray(), (QJsonArray{1, 2}));
    QTRY_VERIFY(!workspace.findChildren<QLabel *>("teamProfileCompareDelta").isEmpty());
    QStringList deltas;
    for (auto *delta : workspace.findChildren<QLabel *>("teamProfileCompareDelta")) deltas << delta->text();
    QVERIFY2(deltas.contains("+3h 30m") && deltas.contains("+1.20M"), qPrintable(deltas.join(", ")));
    shot(&workspace, "u38-profile-compare");
    workspace.setTheme(false); workspace.render(); lightShot(&workspace, "u38-profile-compare-light"); darkPalette(); workspace.setTheme(true); workspace.render();
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
void TestTeamsUi::profileReadsInBoundedPagesAndKeepsABoundedCache()
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
void TestTeamsUi::profileOfALargeTeamStaysResponsive()
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

// U59: Start planning shows the lead's proposed mode and its reason; the owner
// chooses one of the two. With no choice the approval sends no mode (the team
// plans standard); a choice goes with the approval and shows on the button
// and the planned phase.
void TestTeamsUi::planModeIsChosenWhenThePlanIsApproved()
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
    shot(&workspace, "u59-start-dark");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u59-start-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
void TestTeamsUi::switchToFreeOpensOneDialogBoundToTheRequest()
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
void TestTeamsUi::freePhaseReadsIdleBuildsAndClosesOnConfirmation()
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
    shot(&workspace, "u59-graph-dark");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u59-graph-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
    shot(&workspace, "u59-board-dark");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u59-board-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
    shot(&workspace, "u59-idle-dark");
    workspace.setTheme(false); workspace.render();
    lightShot(&workspace, "u59-idle-light");
    darkPalette();
    workspace.setTheme(true); workspace.render();
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
    // Resume after limits: the team setting. N33 (#548): only its own field
    // goes; the server merges it, so the lead's newer settings stay.
    QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits"));
    auto *toggle = workspace.findChild<QPushButton *>("teamWorkspaceResumeLimits");
    QVERIFY(toggle->isCheckable() && !toggle->isChecked());
    toggle->click();
    QTRY_COMPARE(calls(script(), "settings").size(), 1);
    const auto settings = calls(script(), "settings").first()["settings"].toObject();
    QVERIFY(settings["resume_after_limits"].toBool());
    QCOMPARE(settings.keys(), QStringList{QStringLiteral("resume_after_limits")});
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
        QString asked, dialogName, dialogTitle, confirmAction;
        bool cancelIsDefault = false;
        QTimer watch; watch.setInterval(10);
        QObject::connect(&watch, &QTimer::timeout, &panel, [&] {
            auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget());
            if (!box || !asked.isEmpty()) return;
            asked = box->text(); dialogName = box->objectName(); dialogTitle = box->windowTitle();
            cancelIsDefault = box->defaultButton() == box->button(QMessageBox::Cancel);
            for (auto *button : box->buttons())
                if (box->buttonRole(button) == QMessageBox::DestructiveRole) confirmAction = button->text();
            box->button(QMessageBox::Cancel)->click();
        });
        watch.start();
        approve->click();
        QTRY_VERIFY(!asked.isEmpty());
        watch.stop();
        QTest::qWait(50); QCOMPARE(calls(script(), "approve").size(), 0);
        QCOMPARE(dialogName, QStringLiteral("teamApproveConfirm"));
        // Native macOS message boxes omit the window title. The body and the
        // destructive action carry the approval there as on other platforms.
#ifndef Q_OS_MACOS
        QCOMPARE(dialogTitle, QStringLiteral("Approve plan and run?"));
#endif
        QVERIFY2(asked.startsWith(QStringLiteral("Approve the plan of Checkout redesign and run it?")), qPrintable(asked));
        QCOMPARE(confirmAction, QStringLiteral("Approve and run")); QVERIFY(cancelIsDefault);
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
// Owner, 2026-10-28: on a small screen the workspace toolbar keeps its icons
// and drops their words (Graph / Board / Layers, All / Active / Problems,
// Auto-accept drafts, Expand all); the words come back when it fits again
// and stay in the tooltips and accessible names.
void TestTeamsUi::aNarrowToolbarKeepsOnlyItsIcons()
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    const auto segment = [&](const char *group, const char *value) -> QPushButton * {
        for (auto *b : workspace.findChild<QWidget *>(group)->findChildren<QPushButton *>()) if (b->property("segment") == value) return b;
        return nullptr;
    };
    QTRY_COMPARE(segment("teamWorkspaceView", "graph")->text(), QStringLiteral("Graph"));
    QCOMPARE(segment("teamWorkspaceFilter", "problems")->text(), QStringLiteral("Problems"));
    QVERIFY(segment("teamWorkspaceFilter", "problems")->icon().isNull());
    // U38: with the switch labels gone, Heat is one flame button whose menu
    // holds the same three choices; a choice is kept for the team.
    auto *heatMenu = workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu");
    QVERIFY(heatMenu && heatMenu->isVisible() && !heatMenu->icon().isNull());
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceHeatWide")->isVisible());
    QCOMPARE(heatMenu->toolTip(), QStringLiteral("Heat: Off"));
    QStringList choices; for (auto *action : heatMenu->menu()->actions()) choices << action->text();
    QCOMPARE(choices, (QStringList{"Off", "Time lost", "Tokens"}));
    QSettings().remove("teams/heat/team-1");
    heatMenu->menu()->actions().at(1)->trigger();
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("time"));
    QTRY_COMPARE(workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu")->toolTip(), QStringLiteral("Heat: Time lost"));
    workspace.findChild<QPushButton *>("teamWorkspaceHeatMenu")->menu()->actions().at(0)->trigger();
    QCOMPARE(TeamUi::heatMode("team-1"), QStringLiteral("off"));
    QTRY_COMPARE(segment("teamWorkspaceView", "graph")->text(), QStringLiteral("Graph"));
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split);
    split->setSizes({430, 1010});
    QTRY_VERIFY(segment("teamWorkspaceView", "graph")->text().isEmpty());
    QVERIFY(!segment("teamWorkspaceView", "graph")->icon().isNull());
    QCOMPARE(segment("teamWorkspaceView", "graph")->toolTip(), QStringLiteral("Graph"));
    QCOMPARE(segment("teamWorkspaceView", "graph")->accessibleName(), QStringLiteral("Graph"));
    QVERIFY(segment("teamWorkspaceFilter", "problems")->text().isEmpty());
    QVERIFY(!segment("teamWorkspaceFilter", "problems")->icon().isNull());
    if (auto *expand = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll")) QVERIFY(expand->text().isEmpty());
    // U38: the icon-only step leaves the Heat switch out.
    QVERIFY(!workspace.findChild<QWidget *>("teamWorkspaceHeatBox")->isVisible());
    // A render rebuilds the toolbar: look it up each time.
    const auto bar = [&] { return workspace.findChild<QWidget *>("teamWorkspaceToolbar"); };
    QTest::qWait(200); shot(bar(), "team-toolbar-narrow-dark");
    QTRY_VERIFY2(bar()->sizeHint().width() <= bar()->width() + 1, qPrintable(QStringLiteral("%1 px for a %2 px row").arg(bar()->sizeHint().width()).arg(bar()->width())));
    split->setSizes({1100, 340});
    QTRY_COMPARE(segment("teamWorkspaceView", "graph")->text(), QStringLiteral("Graph"));
    QTRY_COMPARE(segment("teamWorkspaceFilter", "problems")->text(), QStringLiteral("Problems"));
    QSettings().remove("workspace/skin");
}

// whitewh-115: in the shortened mode the toolbar jumped between its fit steps
// at the boundaries. Each step is measured with its own controls now, and the
// choice only widens with slack; at a boundary it must settle on one step and
// keep it across refits and polls.
void TestTeamsUi::theNarrowToolbarSettlesOnOneStep()
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
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceFilter"));
    auto *split = workspace.findChild<QSplitter *>("teamWorkspaceSplit");
    QVERIFY(split);
    const auto bar = [&] { return workspace.findChild<QWidget *>("teamWorkspaceToolbar"); };
    const auto segment = [&](const char *group, const char *value) -> QPushButton * {
        auto *found = workspace.findChild<QWidget *>(group);
        if (!found) return nullptr;
        for (auto *b : found->findChildren<QPushButton *>()) if (b->property("segment") == value) return b;
        return nullptr;
    };
    // What the row settled on: the wide Heat switch, its one-button menu, the
    // words on the view and filter segments, and Hide finished tell the four
    // steps apart.
    const auto signature = [&]() -> QString {
        QString s;
        auto *graph = segment("teamWorkspaceView", "graph");
        auto *problems = segment("teamWorkspaceFilter", "problems");
        const auto shown = [&](const char *name) { auto *w = workspace.findChild<QWidget *>(name); return w && w->isVisible(); };
        s += graph && !graph->text().isEmpty() ? QLatin1Char('v') : QLatin1Char('V');
        s += problems && !problems->text().isEmpty() ? QLatin1Char('f') : QLatin1Char('F');
        s += shown("teamWorkspaceHeatWide") ? QLatin1Char('w') : QLatin1Char('-');
        s += shown("teamWorkspaceHeatBox") ? QLatin1Char('b') : QLatin1Char('-');
        s += shown("teamWorkspaceHeatMenu") ? QLatin1Char('m') : QLatin1Char('-');
        s += shown("teamWorkspaceHideFinished") ? QLatin1Char('h') : QLatin1Char('-');
        return s;
    };
    const auto settle = [&](int width) {
        split->setSizes({width, 1440 - width});
        QCoreApplication::processEvents();
        return signature();
    };
    // One refit at the same width: a hide/show (the row refits on show) and a
    // one-pixel nudge there and back (two resize refits).
    const auto refit = [&](int width) {
        auto *row = bar();
        row->hide(); row->show();
        split->setSizes({width - 1, 1440 - width + 1});
        QCoreApplication::processEvents();
        split->setSizes({width, 1440 - width});
        QCoreApplication::processEvents();
        return signature();
    };
    // The steps across a range of widths; a change between neighbours is a
    // boundary.
    QMap<int, QString> steps;
    for (int width = 400; width <= 1040; width += 4) steps.insert(width, settle(width));
    QList<int> boundaries;
    for (int width = 404; width <= 1040; width += 4)
        if (steps.value(width) != steps.value(width - 4)) boundaries << width;
    QVERIFY2(boundaries.size() >= 2, "the scan found the toolbar's steps");
    // Repeated refits at both sides of every boundary keep exactly the step the
    // width decided on; hiding a control never changes the step it started from.
    for (int boundary : boundaries) {
        for (int width : {boundary - 1, boundary, boundary + 1}) {
            const QString settled = settle(width);
            for (int pass = 0; pass < 5; ++pass) {
                QCOMPARE(refit(width), settled);
                QCOMPARE(settle(width), settled);
            }
        }
    }
    // A poll rebuilds the row; at the same width it settles on the same step.
    for (int boundary : boundaries) {
        const QString settled = settle(boundary);
        controller.refresh();
        QTRY_VERIFY(bar() && bar()->isVisible());
        QTest::qWait(50);
        QCOMPARE(signature(), settled);
    }
    // Shots: the icon-only toolbar in both themes (the existing narrow test's
    // 430 px height of the split).
    const int narrow = 430;
    settle(narrow);
    QTest::qWait(100);
    shot(bar(), "team-toolbar-settled-dark");
    lightPalette(); workspace.setTheme(false);
    settle(narrow);
    QTest::qWait(100);
    lightShot(bar(), "team-toolbar-settled-light");
    QSettings().remove("workspace/skin");
}

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
        QTRY_VERIFY(([&] { controller.refresh(); return workspace.findChild<QWidget *>("teamPhaseGraph") && workspace.findChild<QPushButton *>("teamStuckShowOnGraph"); })());
        workspace.findChild<QPushButton *>("teamStuckShowOnGraph")->click();
        QTRY_VERIFY(([&] {
            auto *v = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
            return v && v->emphasis == QSet<int>{2, 3};
        })());
        auto *flatGraph = dynamic_cast<TeamUi::TaskView *>(workspace.findChild<QWidget *>("teamPhaseGraph"));
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

// U33: the Board scrolls sideways instead of squeezing its columns. At 700 px
// every column keeps its readable width and the thin bar shows; the columns of
// two open phases share their x positions; the horizontal place survives a
// poll with a new revision and a phase collapsed; at 1600 px there is no bar.
// Graph and Layers keep their columns without a horizontal bar.
void TestTeamsUi::boardScrollsInsteadOfSqueezingColumns()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    setTeam(prototypeTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(prototypeLive);
    workspace.resize(700, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamPhaseScrolls"));
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == segment) { b->click(); return; }
    };
    // Graph and Layers fit the column: no horizontal bar on their page.
    QCOMPARE(workspace.findChild<QScrollArea *>("teamWorkspacePhases")->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
    view("layers");
    QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceLayers"));
    QCOMPARE(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
    view("board");
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && !area()->findChildren<QFrame *>("teamBoardColumn").isEmpty());
    // The bar is on exactly while the columns need more room (it never flickers).
    QVERIFY(area()->horizontalScrollBarPolicy() != Qt::ScrollBarAsNeeded);
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all->text() == QStringLiteral("Expand all")) all->click();
    const auto columns = [&] { return area()->findChildren<QFrame *>("teamBoardColumn"); };
    const auto phasesOpen = [&] { QSet<int> open; for (auto *col : columns()) open.insert(col->property("phase").toInt()); return open.size(); };
    QTRY_VERIFY(phasesOpen() >= 2);
    // Narrow: every column keeps its readable width; the bar scrolls the Board.
    const auto readable = [&] {
        for (auto *col : columns()) if (col->width() < 200) return false;
        return true;
    };
    QTRY_VERIFY(readable());
    QTRY_VERIFY(area()->horizontalScrollBar()->isVisible() && area()->horizontalScrollBar()->maximum() > 0);
    // The columns of every open phase line up: the same set at the same x.
    const auto positions = [&] {
        QMap<int, QList<int>> xs;
        for (auto *col : columns()) xs[col->property("phase").toInt()] << col->mapTo(area()->widget(), QPoint(0, 0)).x();
        for (auto &list : xs) std::sort(list.begin(), list.end());
        return xs;
    };
    const auto xs = positions();
    QVERIFY(xs.size() >= 2);
    for (const auto &list : xs) QCOMPARE(list, xs.first());
    // The horizontal place survives a poll that rebuilds (#693: one with a
    // change; a new revision alone rebuilds nothing) and a phase collapsed.
    area()->horizontalScrollBar()->setValue(120);
    QCOMPARE(area()->horizontalScrollBar()->value(), 120);
    auto *before = area();
    auto next = prototypeTeam(); next["revision"] = 21; next["description"] = "changed"; setTeam(next);
    controller.refresh();
    QTRY_VERIFY(area() && area() != before);
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 120);
    shot(&workspace, "u33-board-narrow-dark");
    QFrame *firstOpen = nullptr;
    for (auto *frame : area()->findChildren<QFrame *>("teamPhaseScroll")) if (frame->findChild<QWidget *>("teamPhaseBody")) { firstOpen = frame; break; }
    QVERIFY(firstOpen);
    const int collapsed = firstOpen->property("phase").toInt();
    before = area();
    firstOpen->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(area() && area() != before);
    QTRY_VERIFY(!positions().contains(collapsed));
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 120);
    firstOpen = nullptr;
    for (auto *frame : area()->findChildren<QFrame *>("teamPhaseScroll")) if (frame->property("phase").toInt() == collapsed) firstOpen = frame;
    firstOpen->findChild<QToolButton *>("teamPhaseToggle")->click();
    QTRY_VERIFY(positions().contains(collapsed));
    workspace.setTheme(false);
    lightShot(&workspace, "u33-board-narrow-light");
    // Wide: the columns share the room, no bar.
    workspace.resize(1600, 900);
    QTRY_VERIFY2(!area()->horizontalScrollBar()->isVisible(), qPrintable(QStringLiteral("viewport %1, content min %2, columns %3")
        .arg(area()->viewport()->width()).arg(area()->widget()->minimumSizeHint().width()).arg(columns().size())));
    QVERIFY(readable());
    QSet<int> widths; for (auto *col : columns()) widths.insert(col->width());
    QVERIFY2(widths.size() <= 2, "the columns share the width");   // integer rounding at most
    lightShot(&workspace, "u33-board-wide-light");
    darkPalette(); workspace.setTheme(true);
    QTRY_VERIFY(area() && !area()->horizontalScrollBar()->isVisible());
    shot(&workspace, "u33-board-wide-dark");
}

// U33: the planning team's Board (no phases) gets the same: readable columns
// and one horizontal scroll at 700 px, none at 1600 px, the place kept.
void TestTeamsUi::planningBoardScrollsInsteadOfSqueezingColumns()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
    auto planning = team(); planning["status"] = "planning";
    setTeam(planning);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(700, 800); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY_WITH_TIMEOUT(workspace.findChild<QWidget *>("teamWorkspaceView"), 20000);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") { b->click(); break; }
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    // Owner, 2026-10-29: a one-phase plan is a phase scroll too; its columns
    // are frames in the board that scrolls as a whole (U33).
    QTRY_VERIFY(area() && area()->findChildren<QWidget *>("teamBoardColumn").size() == 5);   // no Decided task, no Decided column
    const auto readable = [&] {
        for (auto *col : area()->findChildren<QWidget *>("teamBoardColumn")) if (col->width() < 200) return false;
        return true;
    };
    QTRY_VERIFY(readable());
    QTRY_VERIFY(area()->horizontalScrollBar()->isVisible());
    area()->horizontalScrollBar()->setValue(90);
    auto *before = area();
    // #693: a poll with a change rebuilds; the place stays.
    auto next = planning; next["revision"] = planning["revision"].toInt() + 1; next["description"] = "changed"; setTeam(next);
    controller.refresh();
    QTRY_VERIFY(area() && area() != before);
    QTRY_COMPARE(area()->horizontalScrollBar()->value(), 90);
    workspace.resize(1600, 800);
    QTRY_VERIFY(!area()->horizontalScrollBar()->isVisible());
    QVERIFY(readable());
}

// U19 (#520): a team member's subagent request reaches the lead's permission
// box the same way: "Start N subagents", one row per subagent (type, model,
// effort when known, task), Allow all and Deny answering that card with its
// own ids through hgs team answer. Three from the record's list, one from the
// reason text an older hgs sends.
void TestTeamsUi::leadAnswersAMembersSubagentRequest()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
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
    shot(&workspace, "u19-lead-subagents-three-dark");
    workspace.setTheme(false); lightShot(&workspace, "u19-lead-subagents-three-light"); darkPalette(); workspace.setTheme(true);
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
    shot(&workspace, "u19-lead-subagents-one-dark");
    workspace.setTheme(false); lightShot(&workspace, "u19-lead-subagents-one-light"); darkPalette(); workspace.setTheme(true);
    QTRY_VERIFY(workspace.findChild<QLineEdit *>("teamMemberPermissionDenyReason"));
    workspace.findChild<QLineEdit *>("teamMemberPermissionDenyReason")->setText("Not in this phase");
    workspace.findChild<QPushButton *>("teamMemberPermissionDeny")->click();
    QTRY_COMPARE(calls(script(), "answer").size(), 2);
    answer = calls(script(), "answer").last();
    QCOMPARE(answer["answers"].toArray(), (QJsonArray{QJsonObject{{"question_id", "approval"}, {"selected_option_ids", QJsonArray{"2"}}, {"text", "Not in this phase"}}}));
}

// C3 (#548): a desktop edit of a planning team with a roadmap keeps it: the
// plan carries each task's phase and the later phases, through phase 3's graph.
void TestTeamsUi::planEditKeepsTheRoadmap()
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
void TestTeamsUi::planEditsWaitForTheFullPlan()
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

// N37 (#548): Apply ends the whole draft, so its confirmation names the
// undecided changes it drops instead of saying they stay.
void TestTeamsUi::catalogApplySaysUndecidedChangesAreDropped()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor draft(&controller);
    draft.setShowDraft(true);
    QString asked;
    draft.confirm = [&](const QString &name, const QString &, const QString &text, const QString &) { asked = name + ':' + text; return false; };
    draft.resize(1180, 900); draft.show();
    QTRY_VERIFY(draft.findChild<QPushButton *>("teamDesignerApply") && draft.findChild<QPushButton *>("teamDesignerApply")->isEnabled());
    draft.findChild<QPushButton *>("teamDesignerApply")->click();
    QTRY_VERIFY(!asked.isEmpty());
    QVERIFY2(asked.startsWith(QStringLiteral("teamCatalogApplyConfirm:Write 1 kept change as the next catalog version?")), qPrintable(asked));
    QVERIFY2(asked.contains(QStringLiteral("The 1 undecided change is dropped with the draft.")), qPrintable(asked));
    QVERIFY(!asked.contains(QStringLiteral("stay in the draft")));
    QVERIFY(calls(script(), "catalog apply").isEmpty());
}

// N38 (#548): "Ask designer to redo" only asks a running designer; with none
// the click would start a new designer session, so only Discard is offered.
void TestTeamsUi::designerRedoAsksOnlyARunningDesigner()
{
    auto moved = catalog(); auto inner = moved["catalog"].toObject(); inner["version"] = 4; moved["catalog"] = inner;
    { QFile c(script() + ".catalog"); QVERIFY(c.open(QIODevice::WriteOnly)); c.write(QJsonDocument(moved).toJson(QJsonDocument::Compact)); }
    const auto restore = qScopeGuard([this] { QFile c(script() + ".catalog"); if (c.open(QIODevice::WriteOnly)) c.write(QJsonDocument(catalog()).toJson(QJsonDocument::Compact)); });
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamDesignerPage page(&controller);
    QString running; int starts = 0;
    page.designerSession = [&] { return running; };
    page.startDesigner = [&](const QString &, const QString &, const QJsonObject &) { ++starts; };
    page.openDesigner = [](const QString &) {};
    page.confirm = [](const QString &, const QString &, const QString &, const QString &) { return true; };
    page.setContext("/work/checkout", "checkout");
    page.resize(1440, 900); page.show();
    QTRY_VERIFY(page.findChild<QLabel *>("teamCatalogBannerTitle")
                && page.findChild<QLabel *>("teamCatalogBannerTitle")->text() == QStringLiteral("Draft is out of date — Apply is off"));
    auto *redo = page.findChild<QPushButton *>("teamCatalogRedo");
    QVERIFY(redo);
    QVERIFY(!redo->isVisible());   // no designer runs
    QVERIFY(page.findChild<QPushButton *>("teamCatalogBannerDiscard")->isVisible());
    // A running designer is asked in its own conversation.
    running = QStringLiteral("claude/checkout/team-designer");
    page.refresh();   // as the window does on each fleet update while the page shows
    QTRY_VERIFY(redo->isVisible());
    QCOMPARE(starts, 0);
}

// QA #500, finding 1 (SPEC 5.3): on Board at 1440x900 a phase header keeps
// its actions in view while the five columns scroll sideways.
void TestTeamsUi::boardPhaseHeaderKeepsItsActionsInView()
{
    setTeam(graphTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.resize(1440, 900); workspace.show();
    controller.refresh();
    QTRY_VERIFY(controller.team("team-1"));
    panel.showTeam("team-1");
    workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>())
        if (b->property("segment") == "board") { b->click(); break; }
    // A poll rebuilds the board: the area is looked up each time.
    const auto area = [&] { return qobject_cast<QScrollArea *>(workspace.findChild<QWidget *>("teamWorkspaceBoard")); };
    QTRY_VERIFY(area() && phaseScrollOf(&workspace, 3) && phaseScrollOf(&workspace, 3)->findChild<QPushButton *>("teamPhaseAccept"));
    // The columns need more room than the viewport: the page scrolls sideways.
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0);
    const auto inView = [&](const char *name, int place) {
        auto *board = area();
        auto *scroll = phaseScrollOf(&workspace, 3);
        auto *widget = scroll ? scroll->findChild<QWidget *>(name) : nullptr;
        if (!board || !widget || !widget->isVisible()) return false;
        // Placed at a fraction of the sideways range: 0, half or the end.
        auto *bar = board->horizontalScrollBar();
        const int value = bar->maximum() * place / 2;
        if (bar->value() != value) { bar->setValue(value); return false; }
        const QRect rect(widget->mapTo(board->viewport(), QPoint(0, 0)), widget->size());
        return board->viewport()->rect().contains(rect);
    };
    for (int place : {0, 1, 2})
        for (const char *name : {"teamPhaseAutoAccept", "teamPhaseReject", "teamPhaseAccept", "teamPhaseMore"})
            QTRY_VERIFY2(inView(name, place), qPrintable(QStringLiteral("%1 at %2/2 of the sideways range").arg(name).arg(place)));
}

namespace {
// U36: the mock-up's Orbit checkout team (design/agent-teams/pipelines) in
// phase 2 with every stage state, shaped like `hgs team status` with #531's
// pipeline_run and #532's stage queue and folders. Times count from 1800000000;
// the tests set the views' clock to 1800004000.
QJsonObject pipelineTeam()
{
    return QJsonDocument::fromJson(R"JSON({
 "id":"team-1","name":"Checkout","status":"running","project":"Orbit platform","folder":"/work/orbit","lead":"lead",
 "revision":30,"created":1000,"updated":5000,"settings":{},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","model":"claude-opus-4-6","state":"active","session":{"host":"","name":"claude/orbit/checkout"}},
  {"id":"backend","role":"Backend","agent":"dsh","model":"deepseek-v4-pro","state":"active","directory":"/work/orbit-team/backend","session":{"host":"","name":"dsh/orbit/api"}},
  {"id":"frontend","role":"Frontend","agent":"codex","model":"gpt-6.1-sol","state":"active","directory":"/work/orbit-team/frontend","session":{"host":"","name":"codex/orbit/ui"}},
  {"id":"reviewer","role":"Reviewer","agent":"claude","model":"claude-sonnet-4-6","state":"active","session":{"host":"","name":"claude/orbit/review"},
   "stage_queue":[{"task":3,"stage":"review","round":1,"state":"delivered","since":1800003000},{"task":10,"stage":"review","round":1,"state":"waiting","since":1800003760}]},
  {"id":"reviewer2","role":"Reviewer 2","agent":"codex","model":"gpt-5.5-codex","state":"active","session":{"host":"","name":"codex/orbit/review2"},
   "stage_queue":[{"task":11,"stage":"review","round":2,"state":"delivered","since":1800003500},{"task":10,"stage":"review","round":1,"state":"waiting","since":1800003760}]},
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2.5","computer":"build-01","state":"active","session":{"host":"","name":"kimi/orbit/tests"},
   "stage_queue":[{"task":4,"stage":"test","round":1,"state":"delivered","since":1800003280}]},
  {"id":"tester2","role":"Tester 2","agent":"kimi","model":"kimi-k2.5","state":"active","session":{"host":"","name":"kimi/orbit/tests2"},
   "stage_queue":[{"task":17,"stage":"test","round":1,"state":"delivered","since":1800003600}]},
  {"id":"tester3","role":"Tester 3","agent":"opencode","model":"qwen3-coder","state":"active","session":{"host":"","name":"opencode/orbit/tests3"},
   "stage_queue":[{"task":18,"stage":"test","round":1,"state":"delivered","since":1800003900}]},
  {"id":"secretary","role":"Secretary","agent":"claude","model":"claude-haiku-4-5","state":"active","session":{"host":"","name":"claude/orbit/docs"}}],
 "phases":[
  {"n":1,"title":"Analysis","state":"approved","tasks":[1],"added_by":"Lead","added_at":1000},
  {"n":2,"title":"Build","state":"approved","tasks":[9,6,2,8,5,16,3,11,10,4,17,18,12,13],"added_by":"Lead","added_at":1000}],
 "tasks":[
  {"id":1,"phase":1,"title":"Requirements and acceptance","assignee":"lead","status":"accepted"},
  {"id":9,"phase":2,"title":"Card form keyboard order","assignee":"frontend","status":"pending","deps":[{"task":1}],
   "waits":{"for":"member","on":["Frontend"],"unblock":"folder held by the test of #4"}},
  {"id":6,"phase":2,"title":"E2E checkout flow","assignee":"tester","status":"pending","deps":[{"task":3},{"task":5}]},
  {"id":2,"phase":2,"title":"Saved cards API","assignee":"backend","status":"in_progress","deps":[{"task":1}],"pipeline":"code",
   "attempts":[{"n":1,"id":"a2","member":"backend","state":"rejected","sent_at":1800000400,"report":{"text":"Cards API with cursor paging","at":1800001000}},
               {"n":2,"id":"a2b","member":"backend","state":"delivered","sent_at":1800002000,"started_at":1800002010}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":2,
    "stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],
    "runs":[{"id":"r2a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"returned","by":"Reviewer","at":1800001000,"delivered_at":1800001100,"ended_at":1800001900,
             "text":"Three findings","findings":["major: cursor ids leak the row id","minor: no test for an empty page","nit: rename next_cursor"]}]}},
  {"id":8,"phase":2,"title":"Rate limits on /v1/cards","assignee":"backend","status":"pending","deps":[{"task":2}],"pipeline":"code"},
  {"id":5,"phase":2,"title":"Contract tests","assignee":"tester","status":"submitted","deps":[{"task":2,"kind":"soft"}],
   "attempts":[{"n":1,"id":"a5","member":"tester","state":"rejected","report":{"text":"Contract suite","at":1800000500}},
               {"n":2,"id":"a5b","member":"tester","state":"rejected","report":{"text":"Fixed the fixtures","at":1800001500}},
               {"n":3,"id":"a5c","member":"tester","state":"reported","report":{"text":"Sandbox port fixed","at":1800002500}}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":3,
    "stages":[{"kind":"test","role":"tester2"}],
    "runs":[{"id":"r5a","stage":0,"kind":"test","round":1,"attempt":1,"member":"tester2","state":"returned","by":"Tester 2","at":1800000600,"ended_at":1800000900,"findings":["e2e flake","port in use","fixture drift"]},
            {"id":"r5b","stage":0,"kind":"test","round":2,"attempt":2,"member":"tester2","state":"returned","by":"Tester 2","at":1800001600,"ended_at":1800001900,"findings":["port in use","sandbox timeout"]},
            {"id":"r5c","stage":0,"kind":"test","round":3,"attempt":3,"member":"tester2","state":"returned","by":"Tester 2","at":1800002600,"ended_at":1800002900,"findings":["sandbox timeout","teardown leaks"]}]}},
  {"id":16,"phase":2,"title":"Postal code table","assignee":"backend","status":"submitted","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a16","member":"backend","state":"reported","report":{"text":"Table loaded","at":1800002000}}],
   "pipeline_run":{"id":"check-review","name":"Review, lead","max_rounds":3,"folder":"hold","accept":"lead","round":1,
    "stages":[{"kind":"review","role":"reviewer"}],
    "runs":[{"id":"r16","stage":0,"kind":"review","round":1,"attempt":1,"state":"skipped","by":"Lead","at":1800002050,"ended_at":1800002100,"text":"Generated data, no logic"}]}},
  {"id":3,"phase":2,"title":"Card form UI","assignee":"frontend","status":"in_review","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a3","member":"frontend","state":"reported","sent_at":1800001000,"report":{"text":"Card form with masks","at":1800002900}}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":1,
    "stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],
    "runs":[{"id":"r3","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"delivered","at":1800002950,"delivered_at":1800003000,"folder":"/work/orbit-team/frontend"}]}},
  {"id":11,"phase":2,"title":"Ledger cursor index","assignee":"backend","status":"in_review","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a11","member":"backend","state":"rejected","report":{"text":"Index added","at":1800001500}},
               {"n":2,"id":"a11b","member":"backend","state":"reported","report":{"text":"Index on (ledger, cursor)","at":1800003400}}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":2,
    "stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],
    "runs":[{"id":"r11a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer2","state":"returned","by":"Reviewer 2","at":1800001600,"ended_at":1800002000,"findings":["missing down migration"]},
            {"id":"r11b","stage":0,"kind":"review","round":2,"attempt":2,"member":"reviewer2","state":"delivered","at":1800003450,"delivered_at":1800003500,"folder":"/work/orbit-team/backend"}]}},
  {"id":10,"phase":2,"title":"Saved cards API docs","assignee":"secretary","status":"in_review","deps":[{"task":1}],"stage_candidates":["reviewer","reviewer2"],
   "attempts":[{"n":1,"id":"a10","member":"secretary","state":"reported","report":{"text":"Docs page","at":1800003700}}],
   "pipeline_run":{"id":"docs","name":"Docs: review, auto","max_rounds":2,"folder":"hold","accept":"auto","round":1,
    "stages":[{"kind":"review","role":"reviewer"}],
    "runs":[{"id":"r10","stage":0,"kind":"review","round":1,"attempt":1,"state":"waiting","at":1800003760}]}},
  {"id":4,"phase":2,"title":"Address validation","assignee":"frontend","status":"in_test","deps":[{"task":1}],
   "criteria":["Postal codes are checked per country","Street, city, postal code keep keyboard order","The error shows under the field on blur"],
   "attempts":[{"n":1,"id":"a4","member":"frontend","state":"rejected","sent_at":1800000840,"report":{"text":"Reported: postal codes per country, message under the field. 14 tests passed.","at":1800002760}},
               {"n":2,"id":"a4b","member":"frontend","state":"reported","sent_at":1800003120,"report":{"text":"Fixed all 3; normalization in normalizePostal(). 17 tests passed.","at":1800004200}}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":2,
    "stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],
    "runs":[{"id":"r4a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"returned","by":"Reviewer","at":1800002800,"delivered_at":1800002850,"ended_at":1800003480,
             "text":"3 findings, sent to Frontend as attempt 2",
             "findings":["MAJOR: Canadian codes pass in lower case but are stored unnormalized. web/checkout/address/validate.ts:41",
                         "MINOR: Criterion 3: the error is announced only on submit, not on blur. AddressForm.tsx:88",
                         "NIT: Test name repeats \"should\". validate.test.ts:12"]},
            {"id":"r4b","stage":0,"kind":"review","round":2,"attempt":2,"member":"reviewer","state":"passed","by":"Reviewer","at":1800004210,"delivered_at":1800004220,"ended_at":1800004020,
             "text":"The three findings are fixed; criteria 1-3 met."},
            {"id":"r4c","stage":1,"kind":"test","round":2,"attempt":2,"member":"tester","state":"delivered","at":1800003270,"delivered_at":1800003280,
             "folder":"/work/orbit-team/frontend","holds":true,"changed":[{"commit":"e41b2a0","files":2},{"commit":"a3f9c1e","files":2}]}]}},
  {"id":17,"phase":2,"title":"Shipping copy","assignee":"secretary","status":"in_test","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a17","member":"secretary","state":"reported","report":{"text":"Copy updated","at":1800003500}}],
   "pipeline_run":{"id":"copy","name":"Copy: test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":1,
    "stages":[{"kind":"test","role":"tester"}],
    "runs":[{"id":"r17","stage":0,"kind":"test","round":1,"attempt":1,"member":"tester2","state":"delivered","at":1800003590,"delivered_at":1800003600,"folder":"/work/orbit","holds":false,
             "changed":[{"path":"copy/shipping.md","size":10},{"path":"copy/fees.md","size":10},{"path":"copy/eta.md","size":10},{"path":"copy/old.md","deleted":true}]}]}},
  {"id":18,"phase":2,"title":"Ledger export","assignee":"backend","status":"in_test","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a18","member":"backend","state":"reported","report":{"text":"CSV export","at":1800003000}}],
   "pipeline_run":{"id":"code-detached","name":"Code, detached test","max_rounds":3,"folder":"checkout","accept":"lead","round":1,
    "stages":[{"kind":"test","role":"tester"}],
    "runs":[{"id":"r18","stage":0,"kind":"test","round":1,"attempt":1,"member":"tester3","state":"delivered","at":1800003890,"delivered_at":1800003900,
             "folder":"/tmp/zerus-checkout-18","checkout":"/tmp/zerus-checkout-18","holds":false,"changed":[{"commit":"9c0ffee","files":3}]}]}},
  {"id":12,"phase":2,"title":"Provider token vault","assignee":"backend","status":"accepted","deps":[{"task":1}],"decided_by":"lead",
   "attempts":[{"n":1,"id":"a12","member":"backend","state":"reported","report":{"text":"Vault","at":1800001000}}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":1,
    "stages":[{"kind":"review","role":"reviewer"},{"kind":"test","role":"tester"}],
    "runs":[{"id":"r12a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"passed","at":1800001100,"ended_at":1800001300},
            {"id":"r12b","stage":1,"kind":"test","round":1,"attempt":1,"member":"tester","state":"passed","at":1800001300,"ended_at":1800001700}]}},
  {"id":13,"phase":2,"title":"Checkout copy review","assignee":"secretary","status":"accepted","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a13","member":"secretary","state":"reported","report":{"text":"Copy","at":1800001000}}],
   "pipeline_run":{"id":"docs","name":"Docs: review, auto","max_rounds":2,"folder":"hold","accept":"auto","round":1,
    "stages":[{"kind":"review","role":"reviewer"}],
    "runs":[{"id":"r13","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"passed","at":1800001100,"ended_at":1800001300}]}}],
 "layers":{"1":0,"9":1,"6":2,"2":1,"8":2,"5":2,"16":1,"3":1,"11":1,"10":1,"4":1,"17":1,"18":1,"12":1,"13":1},"cycle":[],"ready":[],
 "validation":{"errors":[],"warnings":[]}})JSON").object();
}

double stageNow() { return 1800004000; }

// Every chip of the open phase's Board: task id -> the chip texts.
QMap<int, QStringList> boardChips(QWidget *root)
{
    QMap<int, QStringList> chips;
    for (auto *line : root->findChildren<QWidget *>("teamStageLine")) {
        if (!line->isVisibleTo(root)) continue;
        QStringList texts;
        for (auto *chip : line->findChildren<QWidget *>("teamStageChip")) texts << chip->property("text").toString();
        chips[line->property("task").toInt()] = texts;
    }
    return chips;
}

QFrame *boardCard(QWidget *root, int task)
{
    for (auto *card : root->findChildren<QFrame *>("teamBoardCard")) if (card->property("task").toInt() == task) return card;
    return nullptr;
}

QString columnOf(QWidget *root, int task)
{
    for (auto *col : root->findChildren<QFrame *>("teamBoardColumn"))
        for (auto *card : col->findChildren<QFrame *>("teamBoardCard")) if (card->property("task").toInt() == task) return col->property("column").toString();
    return {};
}
}

// U36: the Board's In review and In test columns sit between Submitted and
// Accepted at U33's widths with the shared sideways scroll; each card's stage
// chip says its state (in review, in test, waiting for a stage member, returned,
// out of rounds, a held folder, detached, shared folder, skipped, accepted,
// auto) with the stage member's agent and the track; members show their stage
// and stage queue. Shots of the wide and the narrow Board in both themes.
void TestTeamsUi::stageColumnsAndChipsOnTheBoard()
{
    QSettings().setValue("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    setTeam(pipelineTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2000, 960); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 4));
    // The columns, in order, for phase 2: In review and In test between
    // Submitted and Accepted.
    QStringList order;
    QList<QFrame *> phase2;
    for (auto *col : area()->findChildren<QFrame *>("teamBoardColumn")) if (col->property("phase").toInt() == 2) phase2 << col;
    std::sort(phase2.begin(), phase2.end(), [&](QFrame *a, QFrame *b) { return a->mapTo(area()->widget(), QPoint()).x() < b->mapTo(area()->widget(), QPoint()).x(); });
    for (auto *col : phase2) order << col->property("column").toString();
    QCOMPARE(order, (QStringList{"pending", "active", "problems", "submitted", "in_review", "in_test", "accepted"}));
    for (auto *col : phase2) QVERIFY2(col->width() >= 200, qPrintable(col->property("column").toString()));   // U33: BoardColumnWidth
    QCOMPARE(columnOf(area(), 3), QString("in_review")); QCOMPARE(columnOf(area(), 11), QString("in_review")); QCOMPARE(columnOf(area(), 10), QString("in_review"));
    QCOMPARE(columnOf(area(), 4), QString("in_test")); QCOMPARE(columnOf(area(), 17), QString("in_test")); QCOMPARE(columnOf(area(), 18), QString("in_test"));
    QCOMPARE(columnOf(area(), 5), QString("submitted")); QCOMPARE(columnOf(area(), 2), QString("active")); QCOMPARE(columnOf(area(), 9), QString("pending"));
    // In review says one stage waits for a member (#10).
    QStringList waiting;
    for (auto *note : area()->findChildren<QLabel *>("teamBoardColumnWaiting")) waiting << note->text();
    QCOMPARE(waiting, (QStringList{QStringLiteral("· 1 waiting")}));
    // Every state's chip.
    const auto chips = boardChips(area());
    QCOMPARE(chips.value(3), QStringList{"Review r1/3"});
    QCOMPARE(chips.value(11), QStringList{"Review r2/3"});
    QCOMPARE(chips.value(10), QStringList{QStringLiteral("Review waits · 1st in queue")});
    QCOMPARE(chips.value(4), QStringList{"Test r2/3"});
    QCOMPARE(chips.value(17), QStringList{QStringLiteral("Test r1/3 · 4 files")});
    QCOMPARE(chips.value(18), QStringList{QStringLiteral("Test r1/3 · detached")});
    QCOMPARE(chips.value(9), QStringList{"Folder held by the test stage of #4"});
    QCOMPARE(chips.value(2), QStringList{"Review returned r1"});
    QCOMPARE(chips.value(5), QStringList{QStringLiteral("Lead decides · 7 findings")});
    QCOMPARE(chips.value(16), (QStringList{"Review skipped", "Lead"}));
    QCOMPARE(chips.value(12), (QStringList{QStringLiteral("Review ✓"), QStringLiteral("Test ✓")}));
    QCOMPARE(chips.value(13), (QStringList{QStringLiteral("Review ✓"), "Auto"}));
    QVERIFY(!chips.contains(8) && !chips.contains(6));   // not reported yet: no line
    // The stage member's agent beside a running stage; the skipped chip is struck.
    QVERIFY(boardCard(area(), 3)->findChild<QLabel *>("teamStageAgent"));
    QVERIFY(!boardCard(area(), 10)->findChild<QLabel *>("teamStageAgent"));
    for (auto *chip : boardCard(area(), 16)->findChildren<QWidget *>("teamStageChip"))
        QCOMPARE(chip->font().strikeOut(), chip->property("state") == "skipped");
    QCOMPARE(boardCard(area(), 4)->findChild<QWidget *>("teamStageTrack")->property("segments").toInt(), 4);
    // The returned task names its new attempt and findings; the round limit
    // its rounds; both warn, as does the held folder's border.
    QCOMPARE(boardCard(area(), 2)->findChild<QLabel *>("teamBoardActivity")->text(), QStringLiteral("Attempt 2 · 3 findings"));
    QCOMPARE(boardCard(area(), 5)->findChild<QLabel *>("teamBoardActivity")->text(), QStringLiteral("3 test rounds used"));
    const auto warnEdge = [&](int task) { return boardCard(area(), task)->styleSheet().contains(TeamUi::colorsFor(true).warn.name()); };
    QVERIFY(warnEdge(5) && warnEdge(9) && !warnEdge(3) && !warnEdge(4));
    // The test's tooltip names its member, where it works and what is next.
    QString tip;
    for (auto *chip : boardCard(area(), 4)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(tip.contains("Test round 2 of 3") && tip.contains("Tester · kimi-k2.5") && tip.contains("/work/orbit-team/frontend")
             && tip.contains("Holds Frontend's folder") && tip.contains("Next: Lead accepts"), qPrintable(tip));
    // Members: the stage each has and its queue; the implementer's folder waits.
    const auto memberCard = [&](const QString &id) -> QWidget * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember")) if (card->property("teamMember") == id) return card;
        return nullptr;
    };
    const auto queueTitle = [&](const QString &id) {
        auto *queue = memberCard(id) ? memberCard(id)->findChild<QFrame *>("teamMemberStageQueue") : nullptr;
        return queue ? queue->findChild<QLabel *>("teamMemberStageQueueTitle")->text() : QString();
    };
    QCOMPARE(queueTitle("reviewer"), QString("Stage queue 1"));
    QCOMPARE(queueTitle("reviewer2"), QString("Stage queue 1"));
    QCOMPARE(queueTitle("tester"), QStringLiteral("Stage queue empty · 12 min in this stage"));
    QVERIFY(queueTitle("frontend").isEmpty() && queueTitle("backend").isEmpty());
    auto *row = memberCard("reviewer")->findChild<QPushButton *>("teamMemberStageQueueRow");
    QVERIFY(row && row->property("task").toInt() == 10);
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("reviewer"), Live{}).text, QStringLiteral("Reviewing #3 Card form UI · round 1"));
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("tester"), Live{}).text, QStringLiteral("Testing #4 Address validation · round 2"));
    QCOMPARE(memberStatus(*controller.team("team-1"), *controller.team("team-1")->member("frontend"), Live{}).text, QStringLiteral("Waits: its folder is held by the test stage of #4"));
    // A queued stage opens its task.
    row->click();
    QTRY_VERIFY(static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->isVisible());
    QCOMPARE(static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->task(), 10);
    static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->back();
    QTRY_VERIFY(!static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"))->isVisible());
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all && all->text() == QStringLiteral("Expand all")) all->click();
    shot(&workspace, "u36-boardwide-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u36-boardwide-light");
    // Narrow: the whole Board scrolls sideways to the new columns.
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1180, 960);
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->isVisible() && area()->horizontalScrollBar()->maximum() > 0);
    area()->horizontalScrollBar()->setValue(area()->horizontalScrollBar()->maximum());
    QTRY_VERIFY(area()->horizontalScrollBar()->value() > 0);
    QVERIFY(boardCard(area(), 4)->visibleRegion().boundingRect().width() > 100);
    shot(&workspace, "u36-board-dark");
    workspace.setTheme(false);
    QTRY_VERIFY(area() && area()->horizontalScrollBar()->maximum() > 0);
    area()->horizontalScrollBar()->setValue(area()->horizontalScrollBar()->maximum());
    lightShot(&workspace, "u36-board-light");
    // The Layers rows carry the same chips.
    darkPalette(); workspace.setTheme(true);
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "layers") b->click();
    QTRY_VERIFY(workspace.findChild<QScrollArea *>("teamWorkspaceLayers"));
    QTRY_VERIFY(!boardChips(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")).isEmpty());
    QCOMPARE(boardChips(workspace.findChild<QScrollArea *>("teamWorkspaceLayers")).value(4), QStringList{"Test r2/3"});
    shot(&workspace, "u36-layers-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u36-layers-light");
}

// #635 N4: the status's held_by names the stage that holds a member's folder;
// a review holds the implementer's folder here (the server's rule since #683),
// which the old test-only rule could not see. The held card's chip, the
// member's status and the phase note say "the review stage of #3". Shots in
// both themes.
void TestTeamsUi::heldByFromTheStatusOnTheBoard()
{
    QSettings().setValue("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    auto json = pipelineTeam();
    auto members = json["members"].toArray();
    for (int i = 0; i < members.size(); ++i) {
        auto m = members[i].toObject();
        if (m["id"] == "frontend") { m["held_by"] = QJsonObject{{"task", 3}, {"stage", "review"}}; members[i] = m; }
    }
    json["members"] = members;
    auto tasks = json["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto t = tasks[i].toObject();
        if (t["id"].toInt() == 9) { t["waits"] = QJsonObject{{"for", "folder"}, {"on", QJsonArray{"#3"}}, {"unblock", "folder held by the stage of #3"}}; tasks[i] = t; }
    }
    json["tasks"] = tasks;
    setTeam(json);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2000, 960); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 9));
    const auto *team = controller.team("team-1");
    QVERIFY(team);
    QCOMPARE(team->member("frontend")->heldBy.task, 3);
    // #4's test no longer counts: the status names the review as the hold.
    QCOMPARE(boardChips(area()).value(9), QStringList{"Folder held by the review stage of #3"});
    QCOMPARE(memberStatus(*team, *team->member("frontend"), Live{}).text, QStringLiteral("Waits: its folder is held by the review stage of #3"));
    QString tip;
    for (auto *chip : boardCard(area(), 3)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(tip.contains("Holds Frontend's folder"), qPrintable(tip));
    tip.clear();
    for (auto *chip : boardCard(area(), 4)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip();
    QVERIFY2(!tip.contains("Holds Frontend's folder"), qPrintable(tip));
    auto *held = workspace.findChild<QLabel *>("teamPhaseHeld");
    QVERIFY(held);
    QCOMPARE(held->text(), QStringLiteral("1 folder held"));
    QVERIFY2(held->toolTip().contains("Frontend's folder: held by the review stage of #3"), qPrintable(held->toolTip()));
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all && all->text() == QStringLiteral("Expand all")) all->click();
    shot(&workspace, "u687-held-by-review-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u687-held-by-review-light");
}

// #693: the Board stops shaking. A poll that changes only what the views do not
// draw (the revision, the updated time, a running task's growing seconds and
// waits, the minutes of a health warning) or the session list naming the same
// team again rebuilds nothing; the running time, the header's elapsed time and
// a stage's minutes tick in place; a real change rebuilds and keeps the
// scroll, the open phases and the open task.
void TestTeamsUi::aPollThatChangesNothingDrawnRebuildsNothing()
{
    QSettings().setValue("workspace/skin", "whitewh");
    double now = stageNow();
    TeamUi::workedClock() = [&now] { return now; };
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    const auto tick = [] { QVERIFY(QMetaObject::invokeMethod(&TeamUi::workedTicker(), "timeout")); };
    // #2 runs since 1800003000; a health warning names idle minutes.
    const auto poll = [&](int revision, double updated, int worked, int deps, int idle, const QString &secondStatus = QStringLiteral("in_progress")) {
        auto json = pipelineTeam();
        json["revision"] = revision; json["updated"] = updated;
        json["health"] = QJsonObject{{"starved", QJsonArray{QJsonObject{{"member", "secretary"}, {"minutes", idle}}}},
                                     {"warnings", QJsonArray{QStringLiteral("starved: Secretary idle %1 min without a ready task").arg(idle)}}};
        // #709 L4: the running phase's health names the same minutes.
        json["phase_summary"] = QJsonArray{
            QJsonObject{{"n", 1}, {"title", "Analysis"}, {"status", "done"}, {"done", 1}, {"total", 1}, {"health_line", ""}},
            QJsonObject{{"n", 2}, {"title", "Build"}, {"status", "in_work"}, {"done", 0}, {"total", 14}, {"health_line", "path 4 · par 2.0 · 1 warning"}}};
        json["phases_health"] = QJsonObject{{"2", QJsonObject{{"tasks", 14}, {"critical_path", 4}, {"parallelism", 2.0},
                                                             {"warnings", QJsonArray{QStringLiteral("starved: Secretary idle %1 min without a ready task").arg(idle)}}}}};
        auto tasks = json["tasks"].toArray();
        for (int i = 0; i < tasks.size(); ++i) {
            auto t = tasks[i].toObject();
            if (t["id"].toInt() == 2) {
                t["status"] = secondStatus;
                t["timing"] = QJsonObject{{"worked_s", worked}, {"running_since", 1800003000.0},
                                          {"waits", QJsonObject{{"deps_s", deps}, {"member_s", 0}, {"lead_s", 0}}},
                                          {"attempts", QJsonArray{QJsonObject{{"n", 2}, {"member", "backend"}, {"state", "delivered"}, {"from", 1800003000.0},
                                                                              {"to", QJsonValue()}, {"worked_s", worked}}}}};
            }
            tasks[i] = t;
        }
        json["tasks"] = tasks;
        return json;
    };
    setTeam(poll(40, 1800003900, 1000, 300, 44));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1700, 640); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 4) && controller.watched() && controller.watched()->revision == 40);
    TestWait::settle();
    const auto statusReads = [&] { return calls(script(), "status").size(); };
    QPointer<QScrollArea> canvas = area();
    QPointer<QFrame> card = boardCard(area(), 4);
    QPointer<QWidget> members = workspace.findChild<QWidget *>("teamWorkspaceSide");
    // Polls that move only the revision, the updated time, #2's seconds and
    // waits and the health minutes: the same widgets stay.
    for (int i = 1; i <= 3; ++i) {
        setTeam(poll(40 + i, 1800003900 + 60 * i, 1000 + 60 * i, 300 + 60 * i, 44 + i));
        const int before = statusReads();
        controller.refresh();
        QTRY_VERIFY(statusReads() > before && controller.watched()->revision == 40 + i);
        QTest::qWait(150);
        QVERIFY2(canvas && canvas == area(), qPrintable(QStringLiteral("poll %1 rebuilt the Board").arg(i)));
        QVERIFY(card && card == boardCard(area(), 4));
        QVERIFY(members && members == workspace.findChild<QWidget *>("teamWorkspaceSide"));
    }
    // The session list names the same team after its own polls: nothing.
    workspace.showTeam("team-1");
    QTest::qWait(100);
    QVERIFY(canvas && canvas == area());
    // Running times and ages tick in place: #2's time, the header's elapsed
    // time and the tester's minutes in its stage.
    auto *worked = boardCard(area(), 2)->findChild<QLabel *>("teamBoardWorked");
    QVERIFY(worked);
    QCOMPARE(worked->text(), QStringLiteral("16m"));
    auto *elapsed = workspace.findChild<QLabel *>("teamHeaderElapsedText");
    QVERIFY(elapsed);
    const QString elapsedBefore = elapsed->text();
    QLabel *stageTitle = nullptr;
    for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
        if (card->property("teamMember") == "tester") stageTitle = card->findChild<QLabel *>("teamMemberStageQueueTitle");
    QVERIFY(stageTitle);
    QCOMPARE(stageTitle->text(), QStringLiteral("Stage queue empty · 12 min in this stage"));
    // #709 L4: the phase health's idle minutes as drawn at 44, three polls on.
    QLabel *health = nullptr;
    for (auto *l : workspace.findChildren<QLabel *>("teamPhaseHealth")) if (l->toolTip().contains("idle")) health = l;
    QVERIFY(health);
    const QString healthText = health->text();
    QVERIFY2(health->toolTip().contains("starved: Secretary idle 44 min without a ready task"), qPrintable(health->toolTip()));
    now += 600;
    tick();
    QCOMPARE(worked->text(), QStringLiteral("26m"));
    QCOMPARE(stageTitle->text(), QStringLiteral("Stage queue empty · 22 min in this stage"));
    QVERIFY2(health->toolTip().contains("starved: Secretary idle 54 min without a ready task"), qPrintable(health->toolTip()));
    QCOMPARE(health->text(), healthText);
    // The fixture's team is weeks old: a day later its elapsed time moves.
    now += 86400;
    tick();
    QVERIFY2(elapsed->text() != elapsedBefore, qPrintable(elapsed->text()));
    now -= 86400;
    tick();
    QVERIFY(canvas && canvas == area() && card == boardCard(area(), 4));
    // A real change rebuilds, and keeps where the owner was: the Board's
    // scroll, the finished phase 1 the owner opened and the open task.
    if (!phaseOpen(&workspace, 1)) togglePhase(&workspace, 1);
    QTRY_VERIFY(phaseOpen(&workspace, 1));
    workspace.resize(1700, 420);
    QTRY_VERIFY(area() && area()->verticalScrollBar()->maximum() > 40);
    area()->verticalScrollBar()->setValue(40);
    QTest::mouseClick(boardCard(area(), 4), Qt::LeftButton, {}, QPoint(boardCard(area(), 4)->width() / 2, 8));
    auto *drawer = static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"));
    QTRY_VERIFY(drawer->isVisible() && drawer->task() == 4);
    TestWait::settle();
    canvas = area();
    setTeam(poll(50, 1800004500, 1000, 300, 50, QStringLiteral("submitted")));
    controller.refresh();
    QTRY_VERIFY(controller.watched()->revision == 50);
    QTRY_VERIFY(!canvas || canvas != area());   // rebuilt
    QTRY_COMPARE(columnOf(area(), 2), QString("submitted"));
    QTRY_COMPARE(area()->verticalScrollBar()->value(), 40);
    QVERIFY(phaseOpen(&workspace, 1));
    QVERIFY(drawer->isVisible() && drawer->task() == 4);
    shot(&workspace, "u693-board-after-change-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u693-board-after-change-light");
}

// #693 (the lead's note): a phase graph's sideways bar is decided after its
// layout, never in a rebuild's first pass, and not flipped by the other bar:
// a graph that fits shows none across rebuilds and resizes; one wider than
// its column keeps it.
void TestTeamsUi::aPhaseGraphThatFitsShowsNoSidewaysBar()
{
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    setTeam(pipelineTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(2000, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    // Phase 1 is done and folded: the open phase 2's graph.
    const auto graphArea = [&]() -> QScrollArea * {
        for (auto *area : workspace.findChildren<QScrollArea *>("teamPhaseGraphArea")) if (area->isVisible()) return area;
        return nullptr;
    };
    QTRY_VERIFY(graphArea());
    TestWait::settle();
    const auto fits = [&] { auto *a = graphArea(); return a->widget()->minimumWidth() <= a->width(); };
    QVERIFY2(fits(), "the phase graph fits a 2000 px window");
    // Rebuilds (a real change each time) and resizes: the bar never shows,
    // not even before the new graph's layout.
    for (int i = 0; i < 4; ++i) {
        workspace.render();
        QCOMPARE(graphArea()->horizontalScrollBarPolicy(), Qt::ScrollBarAlwaysOff);
        TestWait::settle();
        QVERIFY(!graphArea()->horizontalScrollBar()->isVisible());
        workspace.resize(2000 - 20 * i, 900 - 10 * i);
        TestWait::settle();
        QVERIFY(fits());
        QVERIFY2(!graphArea()->horizontalScrollBar()->isVisible(), qPrintable(QStringLiteral("resize %1").arg(i)));
    }
    // Narrow, at full zoom: wider than its column, the bar stays.
    workspace.resize(760, 900);
    for (int i = 0; i < 4; ++i) {
        QTRY_VERIFY(workspace.findChild<QPushButton *>("teamWorkspaceZoomIn"));
        workspace.findChild<QPushButton *>("teamWorkspaceZoomIn")->click();
        TestWait::settle();
    }
    QTRY_VERIFY(!fits());
    QTRY_VERIFY(graphArea()->horizontalScrollBar()->isVisible());
    for (int i = 0; i < 3; ++i) {
        workspace.render();
        TestWait::settle();
        QVERIFY(graphArea()->horizontalScrollBar()->isVisible());
    }
}

// U36: the inspector's Pipeline section: the flow, where the test works, what
// changed and what it holds, every attempt and round with its verdict and
// findings (fixed ones dimmed), and what comes next; the overrides each ask
// with what they do, wait for a reason and send it, and Cancel sends nothing.
// Shots in both themes, with the Skip form open.
void TestTeamsUi::inspectorShowsThePipelineAndOverridesAskForAReason()
{
    QSettings().setValue("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    darkPalette();
    const Team team = parse(pipelineTeam());
    TeamTaskCard card;
    QList<std::pair<QString, QJsonObject>> acts;
    card.act = [&](const QString &operation, const QJsonObject &input) { acts.append({operation, input}); };
    card.resize(560, 1500); card.show();
    card.setTask(team, 4, live, true);
    auto *pipeline = card.findChild<QWidget *>("teamTaskPipeline");
    QVERIFY(pipeline);
    QCOMPARE(card.findChild<QLabel *>("teamTaskStageRound")->text(), QString("round 2 of 3"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineName")->text(), QString("Code: review, test, lead"));
    QStringList flow;
    for (auto *chip : pipeline->findChild<QWidget *>("teamTaskPipelineFlow")->findChildren<QWidget *>("teamStageChip")) flow << chip->property("text").toString();
    QCOMPARE(flow, (QStringList{QStringLiteral("Implement ✓ 2 attempts"), QStringLiteral("Review ✓ r2"), "Test r2/3", "Lead"}));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineWhere")->text(), QStringLiteral("/work/orbit-team/frontend · Frontend's folder"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineChanges")->text(), QStringLiteral("e41b2a0..a3f9c1e · 2 commits · 4 files"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineHolds")->text(), QStringLiteral("the folder: #9 Card form keyboard order waits"));
    QVERIFY(pipeline->findChild<QLabel *>("teamTaskPipelineMay")->text().contains("build"));
    // The rounds: attempt 1, its review returned with 3 findings (fixed by the
    // second review's pass, so dimmed), attempt 2, its review passed and the
    // test running, then the lead.
    QStringList rounds;
    for (auto *w : pipeline->findChild<QWidget *>("teamTaskRounds")->findChildren<QWidget *>(QString(), Qt::FindDirectChildrenOnly)) {
        const QString name = w->objectName();
        if (name == "teamRoundAttempt") rounds << QStringLiteral("attempt %1").arg(w->property("attempt").toInt());
        else if (name == "teamRoundStage") rounds << QStringLiteral("%1 %2 %3").arg(w->property("stage").toString()).arg(w->property("round").toInt()).arg(w->property("state").toString());
        else if (name == "teamRoundNext" || name == "teamRoundAccept") rounds << name;
    }
    QCOMPARE(rounds, (QStringList{"attempt 1", "review 1 returned", "attempt 2", "review 2 passed", "test 2 delivered", "teamRoundAccept"}));
    const auto findings = pipeline->findChildren<QFrame *>("teamRoundFinding");
    QCOMPARE(findings.size(), 3);
    for (auto *finding : findings) QVERIFY(finding->property("fixed").toBool());
    QStringList severities;
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundSeverity")) severities << label->text();
    QCOMPARE(severities, (QStringList{"MAJOR", "MINOR", "NIT"}));
    QStringList verdicts;
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundVerdict")) verdicts << label->text();
    QCOMPARE(verdicts, (QStringList{"RETURN", "PASS", "RUNNING 12 min", "NEXT"}));
    // The footer of a task in a stage: Accept now, Skip test, Send back, Reject.
    QStringList footer;
    for (auto *b : card.findChild<QWidget *>("teamTaskCardFooter")->findChildren<QPushButton *>()) footer << b->objectName();
    QCOMPARE(footer, (QStringList{"teamTaskAcceptNow", "teamTaskSkipStage", "teamTaskSendBack", "teamTaskReject"}));
    QCOMPARE(card.findChild<QPushButton *>("teamTaskSkipStage")->text(), QStringLiteral("Skip test…"));
    shot(&card, "u36-inspector-dark");
    lightPalette(); card.setTask(team, 4, live, false);
    lightShot(&card, "u36-inspector-light");
    darkPalette(); card.setTask(team, 4, live, true);
    // Skip asks first: what it does, a reason; the button waits for the reason.
    card.findChild<QPushButton *>("teamTaskSkipStage")->click();
    auto *form = card.findChild<QFrame *>("teamStageOverrideForm");
    QVERIFY(form);
    QCOMPARE(form->findChild<QLabel *>("teamStageOverrideTitle")->text(), QStringLiteral("Skip Test on #4 Address validation?"));
    QCOMPARE(form->findChild<QLabel *>("teamStageOverrideText")->text(), QStringLiteral("Tester stops; #4 goes to Lead to accept. The feed records that you skipped it and why."));
    auto *submit = form->findChild<QPushButton *>("teamTaskFormSubmit");
    QCOMPARE(submit->text(), QString("Skip Test"));
    QVERIFY(!submit->isEnabled());
    submit->click();
    QVERIFY(acts.isEmpty());
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("   ");
    QVERIFY(!submit->isEnabled());
    // Cancel sends nothing and closes the form.
    form->findChild<QPushButton *>("teamTaskFormCancel")->click();
    QVERIFY(acts.isEmpty());
    QVERIFY(!card.findChild<QFrame *>("teamStageOverrideForm"));
    card.findChild<QPushButton *>("teamTaskSkipStage")->click();
    form = card.findChild<QFrame *>("teamStageOverrideForm");
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Storybook already reviewed by the owner");
    shot(&card, "u36-override-skip-dark");
    lightPalette(); card.setTask(team, 4, live, false);
    form = card.findChild<QFrame *>("teamStageOverrideForm");
    QVERIFY2(form, "the open form survives a theme change");
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Storybook already reviewed by the owner");
    lightShot(&card, "u36-override-skip-light");
    darkPalette(); card.setTask(team, 4, live, true);
    form = card.findChild<QFrame *>("teamStageOverrideForm");
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Storybook already reviewed by the owner");
    submit = form->findChild<QPushButton *>("teamTaskFormSubmit");
    QVERIFY(submit->isEnabled());
    submit->click();
    QCOMPARE(acts.size(), 1);
    QCOMPARE(acts[0].first, QString("skip-stage"));
    QCOMPARE(acts[0].second, (QJsonObject{{"task", 4}, {"reason", "Storybook already reviewed by the owner"}}));
    QVERIFY(!card.findChild<QFrame *>("teamStageOverrideForm"));
    // Accept now names the stages it ends; Send back names the implementer.
    card.findChild<QPushButton *>("teamTaskAcceptNow")->click();
    form = card.findChild<QFrame *>("teamStageOverrideForm");
    QCOMPARE(form->findChild<QLabel *>("teamStageOverrideTitle")->text(), QStringLiteral("Accept #4 Address validation now?"));
    QCOMPARE(form->findChild<QLabel *>("teamStageOverrideText")->text(), QStringLiteral("Ends the pipeline here; the Test stage does not run. The feed records that you accepted it and why."));
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("sandbox flake, tracked in #14");
    form->findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acts.last().first, QString("accept"));
    QCOMPARE(acts.last().second, (QJsonObject{{"task", 4}, {"reason", "sandbox flake, tracked in #14"}}));
    card.findChild<QPushButton *>("teamTaskSendBack")->click();
    form = card.findChild<QFrame *>("teamStageOverrideForm");
    QCOMPARE(form->findChild<QLabel *>("teamStageOverrideTitle")->text(), QStringLiteral("Send #4 Address validation back to Frontend?"));
    form->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("keep cursor ids opaque");
    form->findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QCOMPARE(acts.last().first, QString("send-back"));
    QCOMPARE(acts.last().second, (QJsonObject{{"task", 4}, {"reason", "keep cursor ids opaque"}}));
    QCOMPARE(acts.size(), 3);
    // The round limit: the lead decides, and may still send it back.
    card.setTask(team, 5, live, true);
    footer.clear();
    // Retired buttons lose their names until they are deleted.
    for (auto *b : card.findChild<QWidget *>("teamTaskCardFooter")->findChildren<QPushButton *>()) if (!b->objectName().isEmpty()) footer << b->objectName();
    QCOMPARE(footer, (QStringList{"teamTaskAccept", "teamTaskReject", "teamTaskSendBack", "teamTaskReassign"}));
    QVERIFY(card.findChild<QWidget *>("teamRoundAccept")->findChild<QLabel *>("teamRoundVerdict")->text() == "NOW");
    // A task without a pipeline has no section and no overrides.
    card.setTask(team, 9, live, true);
    QVERIFY(!card.findChild<QWidget *>("teamTaskPipeline"));
    QVERIFY(!card.findChild<QPushButton *>("teamTaskSkipStage"));
}

namespace {
// #538 N8, L5, L6 and QA #539: the mock-up team plus a command stage running
// on a commit (#20), a command that returned with its exit and tail (#21), a
// land stage that waits for its folder (#22), a task the lead sent back at the
// round limit (#23) and a review no member may take (#24); #4's test has run
// two suites.
QJsonObject commandTeam()
{
    auto team = pipelineTeam();
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 4) continue;
        auto run = task["pipeline_run"].toObject(); auto runs = run["runs"].toArray(); auto test = runs[2].toObject();
        test["runs"] = QJsonDocument::fromJson(R"([{"template":"qt-test","args":{"suite":"test_teamsui","functions":["boardShows"]},"exit":0,"at":1800003500,"ms":29000},
            {"template":"cargo-test","args":{"filter":"state::teams"},"exit":1,"at":1800003800,"ms":125000}])").array();
        runs[2] = test; run["runs"] = runs; task["pipeline_run"] = run; tasks[i] = task;
    }
    const auto ship = R"("stages":[{"kind":"review","role":"reviewer"},{"kind":"command","command":{"command":"ctest --output-on-failure","expect_exit":0,"timeout_minutes":45}},
        {"kind":"land","command":{"command":"scripts/land.sh --squash","expect_exit":0,"timeout_minutes":30},"exits":{"0":"accepted","1":"held"}}])";
    const auto extra = QJsonDocument::fromJson(QStringLiteral(R"JSON([
  {"id":20,"phase":2,"title":"Paged ledger","assignee":"backend","status":"in_test","deps":[{"task":1}],"stage_candidates":[],
   "attempts":[{"n":1,"id":"a20","member":"backend","state":"reported","report":{"text":"Ledger pages","at":1800003000}}],
   "pipeline_run":{"id":"ship","name":"Ship: review, command, land","max_rounds":3,"folder":"hold","accept":"lead","round":1,%1,
    "runs":[{"id":"r20a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"passed","by":"Reviewer","at":1800003050,"ended_at":1800003600},
            {"id":"r20b","stage":1,"kind":"command","round":1,"attempt":1,"state":"delivered","by":"Zerus","at":1800003600,"delivered_at":1800003700,
             "folder":"/work/orbit-team/backend","commit":"abc1234def567"}]}},
  {"id":21,"phase":2,"title":"Cards cache","assignee":"secretary","status":"pending","deps":[{"task":1}],
   "attempts":[{"n":1,"id":"a21","member":"secretary","state":"rejected","report":{"text":"Cache added","at":1800002000}}],
   "pipeline_run":{"id":"ship","name":"Ship: review, command, land","max_rounds":3,"folder":"hold","accept":"lead","round":2,%1,
    "runs":[{"id":"r21a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"passed","by":"Reviewer","at":1800002050,"ended_at":1800002200},
            {"id":"r21b","stage":1,"kind":"command","round":1,"attempt":1,"state":"returned","by":"Zerus","at":1800002200,"delivered_at":1800002210,"ended_at":1800002400,
             "commit":"abc1234def567","text":"The command stage's command exited 1 (expected 0). Output tail:\nrunning 12 tests\ntest cards::paged ... FAILED",
             "check":{"exit":1,"tail":"running 12 tests\ntest cards::paged ... FAILED","at":1800002400}}]}},
  {"id":22,"phase":2,"title":"Release notes","assignee":"secretary","status":"in_test","deps":[{"task":1}],"stage_candidates":[],
   "attempts":[{"n":1,"id":"a22","member":"secretary","state":"reported","report":{"text":"Notes","at":1800003900}}],
   "pipeline_run":{"id":"ship","name":"Ship: review, command, land","max_rounds":3,"folder":"hold","accept":"lead","round":1,%1,
    "runs":[{"id":"r22a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"passed","by":"Reviewer","at":1800003910,"ended_at":1800003920},
            {"id":"r22b","stage":1,"kind":"command","round":1,"attempt":1,"state":"passed","by":"Zerus","at":1800003920,"ended_at":1800003940,"commit":"fedcba9876",
             "text":"The command stage's command exited 0 (expected 0).","check":{"exit":0,"tail":"","at":1800003940}},
            {"id":"r22c","stage":2,"kind":"land","round":1,"attempt":1,"state":"waiting","at":1800003950}]}},
  {"id":23,"phase":2,"title":"Fee rounding","assignee":"backend","status":"pending","deps":[{"task":1}],"decided_by":"lead","decision_reason":"Use the paged API",
   "attempts":[{"n":1,"id":"a23","member":"backend","state":"rejected"},{"n":2,"id":"a23b","member":"backend","state":"rejected"},{"n":3,"id":"a23c","member":"backend","state":"rejected"}],
   "pipeline_run":{"id":"code","name":"Code: review, test, lead","max_rounds":3,"folder":"hold","accept":"lead","round":4,"stages":[{"kind":"review","role":"reviewer"}],
    "runs":[{"id":"r23a","stage":0,"kind":"review","round":1,"attempt":1,"member":"reviewer","state":"returned","at":1800001000,"ended_at":1800001100,"findings":["a"]},
            {"id":"r23b","stage":0,"kind":"review","round":2,"attempt":2,"member":"reviewer","state":"returned","at":1800001200,"ended_at":1800001300,"findings":["b"]},
            {"id":"r23c","stage":0,"kind":"review","round":3,"attempt":3,"member":"reviewer","state":"returned","at":1800001400,"ended_at":1800001500,"findings":["c"]}]}},
  {"id":24,"phase":2,"title":"Webhook docs","assignee":"secretary","status":"in_review","deps":[{"task":1}],"stage_candidates":[],
   "attempts":[{"n":1,"id":"a24","member":"secretary","state":"reported","report":{"text":"Webhook page","at":1800003800}}],
   "pipeline_run":{"id":"edit","name":"Edit: review, lead","max_rounds":3,"folder":"hold","accept":"lead","round":1,"stages":[{"kind":"review","role":"editor"}],
    "runs":[{"id":"r24","stage":0,"kind":"review","round":1,"attempt":1,"state":"waiting","at":1800003810}]}}])JSON").arg(QLatin1String(ship)).toUtf8()).array();
    Q_ASSERT(extra.size() == 5);
    auto phases = team["phases"].toArray(); auto build = phases[1].toObject(); auto ids = build["tasks"].toArray();
    auto layers = team["layers"].toObject();
    for (const auto &value : extra) { const int id = value.toObject()["id"].toInt(); tasks.append(value); ids.append(id); layers[QString::number(id)] = 1; }
    build["tasks"] = ids; phases[1] = build;
    team["tasks"] = tasks; team["phases"] = phases; team["layers"] = layers;
    return team;
}
}

// #538 N8: a command or land stage shows the command, its exit and its output
// tail, never a member, a queue or what a review may do. L5: a task the lead
// sent back at the round limit shows no "returned without the lead" chip. L6:
// a stage no member may take warns on its card and makes the team Stuck. QA
// #539: the phase header counts the folders its tests hold, and a running test
// lists the suites hgs recorded (no shots line: hgs records none).
void TestTeamsUi::commandStagesShowTheirCommandExitAndTail()
{
    QSettings().setValue("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        QSettings().remove("workspace/skin"); darkPalette();
        TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); };
    });
    const QJsonObject value = commandTeam();
    const Team team = parse(value);
    QCOMPARE(team.tasks.size(), pipelineTeam()["tasks"].toArray().size() + 5);
    // L6: Stuck when the only open work is a stage nobody may take; a command
    // stage or a named candidate keeps it Running, as does an hgs without the list.
    {
        auto only = value; QJsonArray kept;
        for (const auto &task : value["tasks"].toArray()) if (task.toObject()["id"].toInt() == 1 || task.toObject()["id"].toInt() == 24) kept << task;
        only["tasks"] = kept;
        QCOMPARE(displayStatus(parse(only), {}), QStringLiteral("stuck"));
        auto named = kept[1].toObject(); named["stage_candidates"] = QJsonArray{"reviewer"}; kept[1] = named; only["tasks"] = kept;
        QCOMPARE(displayStatus(parse(only), {}), QStringLiteral("running"));
        named.remove("stage_candidates"); kept[1] = named; only["tasks"] = kept;
        QCOMPARE(displayStatus(parse(only), {}), QStringLiteral("running"));
        kept = QJsonArray{};
        for (const auto &task : value["tasks"].toArray()) if (task.toObject()["id"].toInt() == 1 || task.toObject()["id"].toInt() == 22) kept << task;
        only["tasks"] = kept;
        QCOMPARE(displayStatus(parse(only), {}), QStringLiteral("running"));
    }
    setTeam(value);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    darkPalette(); workspace.setTheme(true);
    workspace.resize(2000, 1100); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 20));
    // In review waits for members (#10, #24); the queued land stage (#22)
    // waits for its folder, not for a member.
    QStringList waiting;
    for (auto *note : area()->findChildren<QLabel *>("teamBoardColumnWaiting")) waiting << note->text();
    QCOMPARE(waiting, (QStringList{QStringLiteral("· 2 waiting")}));
    const auto chips = boardChips(area());
    QCOMPARE(chips.value(20), QStringList{"Command running"});
    QCOMPARE(chips.value(21), QStringList{QStringLiteral("Command returned r1 · exit 1")});
    QCOMPARE(chips.value(22), QStringList{"Land queued"});
    QCOMPARE(chips.value(24), QStringList{QStringLiteral("Review waits · no member")});
    QVERIFY2(!chips.contains(23), qPrintable(chips.value(23).join(", ")));   // L5: the lead sent it back
    QCOMPARE(chips.value(2), QStringList{"Review returned r1"});              // a stage's own return still says so
    QCOMPARE(boardCard(area(), 21)->findChild<QLabel *>("teamBoardActivity")->text(), QStringLiteral("Attempt 2 · exit 1"));
    QVERIFY(!boardCard(area(), 20)->findChild<QLabel *>("teamStageAgent"));
    const auto tipOf = [&](int task) { QString tip; for (auto *chip : boardCard(area(), task)->findChildren<QWidget *>("teamStageChip")) tip = chip->toolTip(); return tip; };
    QVERIFY2(tipOf(20).contains("ctest --output-on-failure") && tipOf(20).contains("abc1234") && tipOf(20).contains("Next: Land")
             && !tipOf(20).contains("member"), qPrintable(tipOf(20)));
    QVERIFY2(tipOf(21).contains("test cards::paged ... FAILED"), qPrintable(tipOf(21)));
    QVERIFY2(tipOf(22).contains("scripts/land.sh --squash") && !tipOf(22).contains("queue"), qPrintable(tipOf(22)));
    const auto warnEdge = [&](int task) { return boardCard(area(), task)->styleSheet().contains(TeamUi::colorsFor(true).warn.name()); };
    QVERIFY(warnEdge(24) && !warnEdge(20));
    // QA #539 1: the phase header says the folder its test holds.
    QLabel *held = nullptr;
    QTRY_VERIFY((held = area()->findChild<QLabel *>("teamPhaseHeld")) || (held = workspace.findChild<QLabel *>("teamPhaseHeld")));
    QCOMPARE(held->text(), QStringLiteral("1 folder held"));
    QVERIFY2(held->toolTip().contains("Frontend's folder: held by the test stage of #4"), qPrintable(held->toolTip()));
    QVERIFY(held->isVisibleTo(&workspace));
    if (auto *all = workspace.findChild<QPushButton *>("teamWorkspaceExpandAll"); all && all->text() == QStringLiteral("Expand all")) all->click();
    shot(&workspace, "u47-board-command-dark");
    workspace.setTheme(false);
    lightShot(&workspace, "u47-board-command-light");
    darkPalette();
    // The inspector: what Zerus runs, what it expects, on which commit.
    TeamTaskCard card;
    card.resize(560, 1300); card.show();
    card.setTask(team, 20, live, true);
    auto *pipeline = card.findChild<QWidget *>("teamTaskPipeline");
    QVERIFY(pipeline);
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineCommand")->text(), QStringLiteral("ctest --output-on-failure"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineExpect")->text(), QStringLiteral("exit 0 · 45 min limit"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineRuns")->text(), QStringLiteral("Zerus itself on commit abc1234, outside any member's sandbox"));
    QVERIFY(!pipeline->findChild<QLabel *>("teamTaskPipelineMay"));
    QStringList by;
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundCommand")) by << label->text();
    QCOMPARE(by, (QStringList{QStringLiteral("Zerus · running · abc1234")}));
    shot(&card, "u47-inspector-command-dark");
    lightPalette(); card.setTask(team, 20, live, false);
    lightShot(&card, "u47-inspector-command-light");
    darkPalette();
    // The returned command: hgs's summary, the exit and the output tail.
    card.setTask(team, 21, live, true);
    pipeline = card.findChild<QWidget *>("teamTaskPipeline");
    by.clear();
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundCommand")) by << label->text();
    QCOMPARE(by, (QStringList{QStringLiteral("Zerus · exit 1 · abc1234")}));
    QCOMPARE(pipeline->findChild<QLabel *>("teamRoundTail")->text(), QStringLiteral("running 12 tests\ntest cards::paged ... FAILED"));
    QStringList texts;
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundText")) texts << label->text();
    QVERIFY2(texts.contains(QStringLiteral("The command stage's command exited 1 (expected 0).")), qPrintable(texts.join(" | ")));
    // A land stage waiting for its folder: its exit map, no member.
    card.setTask(team, 22, live, true);
    pipeline = card.findChild<QWidget *>("teamTaskPipeline");
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineCommand")->text(), QStringLiteral("scripts/land.sh --squash"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineExpect")->text(), QStringLiteral("0 accepted · 1 held · 30 min limit"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineWhere")->text(), QStringLiteral("Secretary's folder, once it is free"));
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineRuns")->text(), QStringLiteral("Zerus itself, once the folder is free; no member takes it"));
    by.clear();
    for (auto *label : pipeline->findChildren<QLabel *>("teamRoundCommand")) by << label->text();
    QCOMPARE(by, (QStringList{QStringLiteral("Zerus · exit 0 · fedcba9"), QStringLiteral("Zerus · starts once the folder is free")}));
    // QA #539 2: the running test lists the suites hgs recorded; with none, no line.
    card.setTask(team, 4, live, true);
    pipeline = card.findChild<QWidget *>("teamTaskPipeline");
    QCOMPARE(pipeline->findChild<QLabel *>("teamTaskPipelineRan")->text(), QStringLiteral("qt-test test_teamsui ✓ in 29 s · cargo-test state::teams exit 1 in 2 min"));
    shot(&card, "u36-inspector-ran-dark");
    lightPalette(); card.setTask(team, 4, live, false);
    lightShot(&card, "u36-inspector-ran-light");
    darkPalette();
    card.setTask(parse(pipelineTeam()), 4, live, true);
    QVERIFY(!card.findChild<QLabel *>("teamTaskPipelineRan"));
    QVERIFY(card.findChild<QLabel *>("teamTaskPipelineMay"));
}

// #538 N9: deleting a role names the pipeline presets that lose its stages
// (and one left with none) before it strips them; No keeps everything.
void TestTeamsUi::catalogRoleDeleteNamesThePresetsItStrips()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { QSettings().remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
    writeCatalog(script(), pipelineCatalog());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    QString asked; bool yes = false;
    editor.confirm = [&](const QString &, const QString &, const QString &text, const QString &) { asked = text; return yes; };
    editor.resize(1320, 1000); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 4);
    const auto remove = [&](const QString &id) {
        for (int i = 0; i < roles->count(); ++i) if (roles->item(i)->data(Qt::UserRole).toString() == id) roles->setCurrentRow(i);
        QTRY_VERIFY(editor.findChild<QToolButton *>("teamRoleDelete") && editor.findChild<QToolButton *>("teamRoleDelete")->isVisible());
        asked.clear();
        editor.findChild<QToolButton *>("teamRoleDelete")->click();
    };
    remove("reviewer");
    QCOMPARE(asked, QStringLiteral("Delete the role Reviewer? These templates lose it: Feature delivery, Documentation. "
                                   "These pipelines lose its stages: Code: review, test, lead; Docs: review, auto (left with no stage). Running teams keep their members."));
    QCOMPARE(roles->count(), 4);   // No: nothing changes
    // Tester takes one stage of Code only; Yes strips it from Code.
    yes = true;
    remove("tester");
    QCOMPARE(asked, QStringLiteral("Delete the role Tester? These templates lose it: Feature delivery. "
                                   "These pipelines lose its stages: Code: review, test, lead. Running teams keep their members."));
    QTRY_COMPARE(roles->count(), 3);
    // A role no preset uses names only its templates.
    yes = false;
    remove("frontend");
    QCOMPARE(asked, QStringLiteral("Delete the role Frontend? These templates lose it: Feature delivery. Running teams keep their members."));
}

// U36: the override reaches hgs from the workspace's inspector with its reason:
// `hgs team skip-stage` and `send-back` carry the task and the reason once.
void TestTeamsUi::stageOverridesReachHgsWithTheirReason()
{
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] { TeamUi::workedClock() = [] { return double(QDateTime::currentSecsSinceEpoch()); }; });
    setTeam(pipelineTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    workspace.setLive(live);
    workspace.resize(1600, 900); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == "board") b->click();
    QTRY_VERIFY(boardCard(&workspace, 3));
    QTest::mouseClick(boardCard(&workspace, 3), Qt::LeftButton, {}, QPoint(20, 10));
    auto *drawer = static_cast<TeamTaskCard *>(workspace.findChild<QWidget *>("teamWorkspaceDrawer"));
    QTRY_VERIFY(drawer->isVisible() && drawer->task() == 3);
    QTRY_VERIFY(drawer->findChild<QPushButton *>("teamTaskSkipStage"));
    drawer->findChild<QPushButton *>("teamTaskSkipStage")->click();
    QTRY_VERIFY(drawer->findChild<QFrame *>("teamStageOverrideForm"));
    QCOMPARE(drawer->findChild<QLabel *>("teamStageOverrideTitle")->text(), QStringLiteral("Skip Review on #3 Card form UI?"));
    QCOMPARE(drawer->findChild<QLabel *>("teamStageOverrideText")->text(), QStringLiteral("Reviewer stops; #3 moves to Test. The feed records that you skipped it and why."));
    drawer->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Storybook already reviewed by the owner");
    drawer->findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QTRY_COMPARE(calls(script(), "skip-stage").size(), 1);
    const auto skip = calls(script(), "skip-stage").first();
    QCOMPARE(skip["task"].toInt(), 3);
    QCOMPARE(skip["reason"].toString(), QString("Storybook already reviewed by the owner"));
    QCOMPARE(skip["team"].toString(), QString("team-1"));
    QTRY_VERIFY(drawer->findChild<QPushButton *>("teamTaskSendBack"));
    drawer->findChild<QPushButton *>("teamTaskSendBack")->click();
    QTRY_VERIFY(drawer->findChild<QFrame *>("teamStageOverrideForm"));
    drawer->findChild<QPlainTextEdit *>("teamTaskFormText")->setPlainText("Split the masks out");
    drawer->findChild<QPushButton *>("teamTaskFormSubmit")->click();
    QTRY_COMPARE(calls(script(), "send-back").size(), 1);
    QCOMPARE(calls(script(), "send-back").first()["reason"].toString(), QString("Split the masks out"));
    QCOMPARE(calls(script(), "skip-stage").size(), 1);
}

// U36: the plan editor's Pipeline picker lists the role's default, the
// catalog's presets and None; the choice goes out with the plan.
void TestTeamsUi::planEditorPicksATasksPipeline()
{
    auto c = catalog();
    auto inner = c["catalog"].toObject();
    inner["pipelines"] = QJsonArray{
        QJsonObject{{"id", "code"}, {"name", "Code: review, test, lead"}, {"max_rounds", 3}, {"folder", "hold"}, {"accept", "lead"},
                    {"stages", QJsonArray{QJsonObject{{"kind", "review"}, {"role", "reviewer"}}, QJsonObject{{"kind", "test"}, {"role", "tester"}}}}},
        QJsonObject{{"id", "docs"}, {"name", "Docs: review, auto"}, {"max_rounds", 2}, {"accept", "auto"}, {"stages", QJsonArray{QJsonObject{{"kind", "review"}, {"role", "reviewer"}}}}},
        QJsonObject{{"id", "check"}, {"name", "Check: lead"}, {"stages", QJsonArray{}}}};
    auto roles = inner["roles"].toArray();
    auto backend = roles[1].toObject(); backend["pipeline"] = "code"; roles[1] = backend; inner["roles"] = roles;
    c["catalog"] = inner;
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(c).toJson(QJsonDocument::Compact)));
    const auto restoreCatalog = qScopeGuard([this] { FakeCli::replace(script() + ".catalog", QJsonDocument(catalog()).toJson(QJsonDocument::Compact)); });
    setTeam(team("planning"));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    panel.setLive(live);
    panel.resize(420, 1200); panel.show();
    controller.refresh();
    panel.showTeam("team-1");
    QTRY_VERIFY(panel.findChild<QWidget *>("teamPlanFooter"));
    QTRY_VERIFY(panel.findChild<QFrame *>("teamPlanErrorsBox"));   // the full plan
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady() && controller.catalog().pipelines.size() == 3);
    QVERIFY(controller.watched() && controller.watched()->full);
    QStringList items, tips;
    bool done = false;
    QTimer::singleShot(50, qApp, [&] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        QVERIFY(dialog && dialog->objectName() == "teamTaskEditor");
        const auto close = qScopeGuard([dialog] { if (dialog->isVisible()) dialog->reject(); });
        auto *picker = dialog->findChild<QComboBox *>("teamTaskEditPipeline");
        QVERIFY(picker);
        for (int i = 0; i < picker->count(); ++i) { items << picker->itemText(i); tips << picker->itemData(i, Qt::ToolTipRole).toString(); }
        QCOMPARE(picker->currentIndex(), 0);
        picker->setCurrentIndex(picker->findData("docs"));
        QVERIFY(dialog->findChild<QLabel *>("teamTaskEditPipelineNote")->text().startsWith(QStringLiteral("Review › Auto · 2 rounds")));
        shot(dialog, "u36-plan-picker-dark");
        lightPalette();
        lightShot(dialog, "u36-plan-picker-light");
        darkPalette();
        for (auto *b : dialog->findChildren<QPushButton *>()) if (dialog->findChild<QDialogButtonBox *>()->buttonRole(b) == QDialogButtonBox::AcceptRole) { b->click(); break; }
        done = true;
    });
    panel.editTask(*controller.watched(), 2);
    QVERIFY(done);
    QCOMPARE(items, (QStringList{QStringLiteral("Default: Code: review, test, lead (Backend's default)"), "Code: review, test, lead", "Docs: review, auto",
                                 "Check: lead", "None: the report goes to the lead"}));
    QCOMPARE(tips[1], QStringLiteral("Review › Test › Lead · 3 rounds"));
    QCOMPARE(tips[3], QStringLiteral("Lead"));
    QTRY_COMPARE(calls(script(), "plan").size(), 1);
    QString chosen; bool others = false;
    for (const auto &value : calls(script(), "plan").first()["tasks"].toArray()) {
        const auto task = value.toObject();
        if (task["id"].toInt() == 2) chosen = task["pipeline"].toString();
        else others = others || task.contains("pipeline");
    }
    QCOMPARE(chosen, QString("docs"));
    QVERIFY2(!others, "the other tasks keep their default");
}


// U53 (#618): the account picker of a composer. Profiles as `hgs account ls`
// lists them: two signed-in Claude accounts, a signed-out one and a Codex one.
static QJsonArray switchProfiles()
{
    const auto window = [](int used) { return QJsonArray{QJsonObject{{"window_minutes", 300}, {"used_percent", used}, {"resets_at", "2099-01-01T00:00:00Z"}}}; };
    return QJsonArray{
        QJsonObject{{"id", "work"}, {"provider", "claude"}, {"label", "Work"}, {"host", ""}, {"usage", QJsonObject{{"status", "ok"}, {"windows", window(64)}}}},
        QJsonObject{{"id", "second"}, {"provider", "claude"}, {"label", "Second"}, {"host", ""}, {"usage", QJsonObject{{"status", "ok"}, {"windows", window(12)}}}},
        QJsonObject{{"id", "old"}, {"provider", "claude"}, {"label", "Old"}, {"host", ""}, {"usage", QJsonObject{{"status", "signed_out"}}}},
        QJsonObject{{"id", "cx"}, {"provider", "codex"}, {"label", "Codex work"}, {"host", ""}, {"usage", QJsonObject{{"status", "ok"}}}},
        QJsonObject{{"id", "far"}, {"provider", "claude"}, {"label", "Far"}, {"host", "box"}, {"usage", QJsonObject{{"status", "ok"}}}}};
}

// The chip's rows in its open popover: the account id, its usage, enabled.
static QList<QPushButton *> accountRows(AccountSwitchChip *chip)
{
    QList<QPushButton *> rows;
    for (auto *row : chip->accountPopover()->findChildren<QPushButton *>("sessionAccountOption")) if (row->isVisible()) rows << row;
    return rows;
}

// A shot of a composer with its open popover above it, as the owner sees them.
static void shotWithPopover(QWidget *host, QWidget *popover, const QString &name)
{
    if (qEnvironmentVariable("ZERUS_TEAM_SHOTS").isEmpty()) return;
    TestWait::settle();
    const QRect hostRect(host->mapToGlobal(QPoint()), host->size());
    QRect all = hostRect;
    const bool open = popover && popover->isVisible();
    if (open) all = all.united(QRect(popover->mapToGlobal(QPoint()), popover->size()));
    QImage image(all.size(), QImage::Format_ARGB32_Premultiplied);
    image.fill(host->palette().color(QPalette::Window));
    QPainter painter(&image);
    painter.drawPixmap(hostRect.topLeft() - all.topLeft(), host->grab());
    if (open) painter.drawPixmap(popover->mapToGlobal(QPoint()) - all.topLeft(), popover->grab());
    painter.end();
    const QString path = QDir(qEnvironmentVariable("ZERUS_TEAM_SHOTS")).filePath(name + ".png");
    if (!image.save(path)) qWarning() << "screenshot not saved:" << path;
}

// Choosing an account asks first; Cancel sends nothing; Move sends one
// switch-account with the account and a new request id, the chip says
// Switching… until the receipt, then the conversation names the result and
// the next read the new account. A refusal names its reason and changes
// nothing; while the agent works the chip is disabled and says why.
void TestTeamsUi::conversationAccountPickerConfirmsAndSwitches()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    const QString program = dir.filePath("hgs");
    QVERIFY(FakeCli::install(program, R"PY(#!/usr/bin/env python3
import json,os,sys,time
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
here=__file__
moved=os.path.exists(here+'.moved')
if a[:1]==['inspect']:
 phase='working' if os.path.exists(here+'.busy') else 'idle'
 print(json.dumps({'name':a[1],'agent':'claude','account_id':'second' if moved else 'work','account_label':'Second' if moved else 'Work',
  'run_id':'run-2' if moved else 'run-1','conversation_id':'conv-1','runtime_state':'live','process_state':'running',
  'activity':'busy' if phase=='working' else 'idle','phase':phase,'cursor':0,'events':[]}))
elif a[:1]==['account']:
 w=lambda used:[{'window_minutes':300,'used_percent':used,'resets_at':'2099-01-01T00:00:00Z'}]
 print(json.dumps({'profiles':[
  {'id':'work','provider':'claude','label':'Work','account_status':{'status':'ok','windows':w(64)}},
  {'id':'second','provider':'claude','label':'Second','account_status':{'status':'ok','windows':w(12)}},
  {'id':'old','provider':'claude','label':'Old','account_status':{'status':'signed_out'}},
  {'id':'cx','provider':'codex','label':'Codex work','account_status':{'status':'ok'}}]}))
elif a[:1]==['switch-account']:
 open(here+'.switches','a').write(json.dumps(a)+'\n')
 time.sleep(0.8)
 if os.path.exists(here+'.refuse'):
  sys.stderr.write('hgs: claude/checkout/lead: the session is not idle; a turn is running\n');sys.exit(1)
 open(here+'.moved','w').write('1')
 print(json.dumps({'status':'switched','session':a[1],'request_id':a[5],'conversation_id':'conv-1',
  'from':{'account_id':'work','label':'Work'},'to':{'account_id':'second','label':'Second'},'run_id':'run-2'}))
else:print('{}')
)PY"));
    const auto switches = [&] {
        QList<QStringList> found; QFile file(program + ".switches");
        if (file.open(QIODevice::ReadOnly)) for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) {
            QStringList args; for (const auto &v : QJsonDocument::fromJson(line).array()) args << v.toString(); found << args;
        }
        return found;
    };
    TeamConversation::setPollInterval(100);
    const auto restore = qScopeGuard([] { TeamConversation::setPollInterval(2000); });
    HgsClient client(FakeCli::program(program));
    TeamConversation conversation(&client);
    conversation.setTheme(true);
    conversation.setSession({}, "claude/checkout/lead");
    conversation.resize(620, 420); conversation.show();
    auto *chip = conversation.accountChip();
    QVERIFY(chip); QTRY_VERIFY(chip->isVisible());
    QTRY_COMPARE(chip->fullLabel(), QStringLiteral("Work"));
    QVERIFY(chip->isEnabled());
    QVERIFY(chip->toolTip().contains("Account: Work"));
    // The menu: the same agent's signed-in accounts on this computer, each with its usage.
    QTRY_VERIFY(([&] { chip->click(); const bool two = accountRows(chip).size() == 2; if (!two) chip->accountPopover()->hide(); return two; }()));
    auto rows = accountRows(chip);
    QCOMPARE(rows[0]->property("account").toString(), QStringLiteral("work"));
    QVERIFY(!rows[0]->isEnabled());   // the current one
    QCOMPARE(rows[1]->property("account").toString(), QStringLiteral("second"));
    QVERIFY(rows[1]->isEnabled());
    QCOMPARE(rows[1]->findChild<QLabel *>("sessionAccountOptionUsage")->text(), QStringLiteral("5h 12% used"));
    QCOMPARE(rows[0]->findChild<QLabel *>("sessionAccountOptionCurrent")->text(), QStringLiteral("Current"));
    // Choosing asks first; Cancel goes back and sends nothing.
    rows[1]->click();
    auto *confirm = chip->accountPopover()->findChild<QWidget *>("sessionAccountConfirm");
    QVERIFY(confirm); QTRY_VERIFY(confirm->isVisible());
    QCOMPARE(confirm->findChild<QLabel *>("sessionAccountConfirmTitle")->text(), QStringLiteral("Move this session to Second?"));
    QVERIFY(confirm->findChild<QLabel *>("sessionAccountConfirmText")->text().contains("restarts there in the same conversation, folder and name"));
    // #581 L6: a busy session is refused, so the text promises no wait.
    QVERIFY(confirm->findChild<QLabel *>("sessionAccountConfirmText")->text().startsWith("Only an idle session moves: the agent stops,"));
    QVERIFY(!confirm->findChild<QLabel *>("sessionAccountConfirmText")->text().contains("once it is idle"));
    confirm->findChild<QPushButton *>("sessionAccountCancel")->click();
    QTRY_COMPARE(accountRows(chip).size(), 2);
    QTest::qWait(150); QVERIFY(switches().isEmpty());
    // Move: one switch-account, Switching… until its receipt.
    accountRows(chip)[1]->click();
    confirm = chip->accountPopover()->findChild<QWidget *>("sessionAccountConfirm");
    QVERIFY(confirm);
    confirm->findChild<QPushButton *>("sessionAccountMove")->click();
    QVERIFY(!chip->accountPopover()->isVisible());
    QCOMPARE(chip->fullLabel(), QStringLiteral("Switching to Second…"));
    QVERIFY(!chip->isEnabled());
    QTRY_COMPARE(switches().size(), 1);
    const auto args = switches().first();
    QCOMPARE(args.mid(0, 5), (QStringList{"switch-account", "claude/checkout/lead", "--account", "second", "--request-id"}));
    QVERIFY(!QUuid::fromString(args.value(5)).isNull());
    auto *status = conversation.findChild<QLabel *>("teamConversationStatus");
    QTRY_COMPARE(status->text(), QStringLiteral("Moved to Second; the agent resumed the same conversation."));
    QVERIFY(status->isVisible());
    QTRY_COMPARE(chip->fullLabel(), QStringLiteral("Second"));
    QVERIFY(chip->isEnabled());
    // A refusal: its reason, nothing changed, the chip says so.
    QVERIFY(QFile(program + ".refuse").open(QIODevice::WriteOnly));
    QTRY_VERIFY(([&] { chip->click(); const bool open = accountRows(chip).size() == 2; if (!open) chip->accountPopover()->hide(); return open; }()));
    accountRows(chip)[1]->click();   // Work: the current account, Second, comes first
    chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
    QTRY_COMPARE(switches().size(), 2);
    QTRY_COMPARE(status->text(), QStringLiteral("Not moved: claude/checkout/lead: the session is not idle; a turn is running"));
    QCOMPARE(chip->fullLabel(), QStringLiteral("Second"));
    QCOMPARE(chip->tone(), ChipTone::Danger);
    // While the agent works the chip is disabled and says why.
    QVERIFY(QFile(program + ".busy").open(QIODevice::WriteOnly));
    QTRY_VERIFY(!chip->isEnabled());
    QVERIFY(chip->toolTip().contains("The agent is working; change its account once it is idle."));
    chip->click(); QVERIFY(!chip->accountPopover()->isVisible());
    // The lead's conversation lists the accounts but changes none: swap-member does.
    QFile::remove(program + ".busy");
    conversation.setAccountLocked("The lead of Orbit: its account changes with Swap member in the team's Members.");
    QTRY_VERIFY(chip->isEnabled());
    chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
    QCOMPARE(chip->accountPopover()->findChild<QLabel *>("sessionAccountNote")->text(),
             QStringLiteral("The lead of Orbit: its account changes with Swap member in the team's Members."));
    for (auto *row : accountRows(chip)) QVERIFY(!row->isEnabled());
    chip->accountPopover()->hide();
    QCOMPARE(switches().size(), 2);
}

// Each state of the Activity composer's account chip, in both themes:
// the menu, the confirmation, Switching…, a rollback, an unconfirmed move,
// a refusal and the disabled chip of a working session.
void TestTeamsUi::accountPickerStatesInShots()
{
    for (const bool dark : {true, false}) {
        if (dark) darkPalette(); else lightPalette();
        const QString theme = dark ? QStringLiteral("dark") : QStringLiteral("light");
        QWidget window; window.setObjectName("accountShotWindow");
        window.setStyleSheet(QStringLiteral("QWidget#accountShotWindow{background:%1;}").arg(dark ? "#15191f" : "#f5f7f9"));
        auto *v = new QVBoxLayout(&window); v->setContentsMargins(16, 16, 16, 16);
        auto *composer = new MessageComposer; v->addWidget(composer);
        composer->setSessionKey("claude/orbit/api");
        QTRY_VERIFY(!composer->isDraftLoading("claude/orbit/api"));
        composer->setAvailability(true);
        composer->setModelSettings("claude-opus-5-5", "high", QJsonArray{QJsonObject{{"id", "claude-opus-5-5"}, {"effort_options", QJsonArray{"high"}}}}, true, {});
        composer->setCompact(true); composer->setTheme(dark);
        // Low on the screen: the popover opens above the chip, as in a window.
        window.resize(760, 190); window.move(20, 380); window.show();
        auto *chip = composer->accountChip();
        QStringList requested;
        chip->switchRequested = [&](const QString &key, const QString &account) { requested << key + '=' + account; };
        chip->setSession("claude/orbit/api", "claude", "work", "Work");
        chip->setOptions(AccountSwitch::options(switchProfiles(), {}, "claude", "work", dark));
        QTRY_VERIFY(chip->isVisible());
        // Beside the model, on its left.
        auto *model = composer->findChild<QPushButton *>("sessionModelSettings");
        QVERIFY(chip->mapTo(composer, QPoint()).x() + chip->width() <= model->mapTo(composer, QPoint()).x());
        QVERIFY(model->mapTo(composer, QPoint()).x() - (chip->mapTo(composer, QPoint()).x() + chip->width()) < 24);
        shot(&window, "u53-chip-" + theme);
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        // Only Work and Second: not the signed-out, Codex or other computer's accounts.
        QCOMPARE(accountRows(chip).size(), 2);
        QCOMPARE(accountRows(chip)[0]->findChild<QLabel *>("sessionAccountOptionUsage")->text(), QStringLiteral("5h 64% used"));
        shotWithPopover(&window, chip->accountPopover(), "u53-menu-" + theme);
        accountRows(chip)[1]->click();
        QVERIFY(chip->accountPopover()->findChild<QWidget *>("sessionAccountConfirm"));
        shotWithPopover(&window, chip->accountPopover(), "u53-confirm-" + theme);
        chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
        QCOMPARE(requested, QStringList{"claude/orbit/api=second"});
        QCOMPARE(chip->fullLabel(), QStringLiteral("Switching to Second…"));
        shot(&window, "u53-switching-" + theme);
        // A failed launch went back to Work.
        auto outcome = chip->finish("claude/orbit/api", QJsonObject{{"status", "rolled_back"}, {"reason", "launch refused"}, {"relaunched", true},
            {"from", QJsonObject{{"account_id", "work"}, {"label", "Work"}}}, {"to", QJsonObject{{"account_id", "work"}, {"label", "Work"}}}}, {});
        QCOMPARE(outcome.kind, AccountSwitch::Outcome::RolledBack);
        QCOMPARE(outcome.text, QStringLiteral("Stayed on Work: Second could not start the agent (launch refused). It resumed on Work."));
        QCOMPARE(chip->fullLabel(), QStringLiteral("Work")); QCOMPARE(chip->tone(), ChipTone::Warning);
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        QCOMPARE(chip->accountPopover()->findChild<QLabel *>("sessionAccountOutcome")->text(), outcome.text);
        shotWithPopover(&window, chip->accountPopover(), "u53-rolledback-" + theme);
        // An unconfirmed move names the new account and is never retried.
        accountRows(chip)[1]->click(); chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
        outcome = chip->finish("claude/orbit/api", QJsonObject{{"status", "uncertain"}, {"to", QJsonObject{{"account_id", "second"}, {"label", "Second"}}},
            {"from", QJsonObject{{"account_id", "work"}, {"label", "Work"}}}}, {});
        QCOMPARE(outcome.kind, AccountSwitch::Outcome::Uncertain);
        QCOMPARE(chip->fullLabel(), QStringLiteral("Second")); QCOMPARE(chip->tone(), ChipTone::Warning);
        QCOMPARE(requested.size(), 2);
        // A refusal changes nothing.
        chip->setSession("claude/orbit/api", "claude", "second", "Second");
        chip->setOptions(AccountSwitch::options(switchProfiles(), {}, "claude", "second", dark));
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        accountRows(chip)[1]->click(); chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
        QCOMPARE(requested.last(), QStringLiteral("claude/orbit/api=work"));
        outcome = chip->finish("claude/orbit/api", {}, "hgs: Account 'work' is not signed in on this computer; log in to it before moving a session.");
        QCOMPARE(outcome.kind, AccountSwitch::Outcome::Refused);
        QCOMPARE(outcome.text, QStringLiteral("Not moved: Account 'work' is not signed in on this computer; log in to it before moving a session."));
        QCOMPARE(chip->fullLabel(), QStringLiteral("Second")); QCOMPARE(chip->tone(), ChipTone::Danger);
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        shotWithPopover(&window, chip->accountPopover(), "u53-refused-" + theme);
        chip->accountPopover()->hide();
        // A working session: disabled, with the reason; nothing opens.
        chip->setBlocked("The agent is working; change its account once it is idle.");
        QVERIFY(!chip->isEnabled());
        QVERIFY(chip->toolTip().contains("The agent is working; change its account once it is idle."));
        chip->click(); QVERIFY(!chip->accountPopover()->isVisible());
        QCOMPARE(chip->tone(), ChipTone::Quiet);
        shot(&window, "u53-working-" + theme);
        // A team member's session: the accounts are listed, none is chosen here.
        chip->setBlocked({});
        chip->setLocked("A member of team Orbit: its account changes with Swap member in the team's Members.");
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        QTRY_VERIFY(!accountRows(chip).isEmpty());
        for (auto *row : accountRows(chip)) QVERIFY(!row->isEnabled());
        shotWithPopover(&window, chip->accountPopover(), "u53-member-" + theme);
        chip->accountPopover()->hide(); chip->setLocked({});
        // An agent that cannot change accounts lists none to choose.
        chip->setBlocked({}); chip->setSession("dsh/orbit/api", "dsh", "", "DeepSeek");
        chip->setOptions({});
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        QCOMPARE(chip->accountPopover()->findChild<QLabel *>("sessionAccountNote")->text(), QStringLiteral("Only Claude and Codex sessions change their account."));
        chip->accountPopover()->hide();
        // A receipt for a session no longer shown leaves the shown one alone,
        // and its outcome shows again on return.
        chip->setSession("claude/orbit/api", "claude", "work", "Work");
        chip->setOptions(AccountSwitch::options(switchProfiles(), {}, "claude", "work", dark));
        chip->click(); QTRY_VERIFY(chip->accountPopover()->isVisible());
        accountRows(chip)[1]->click(); chip->accountPopover()->findChild<QPushButton *>("sessionAccountMove")->click();
        chip->setSession("claude/orbit/web", "claude", "work", "Work");
        QVERIFY(!chip->isEnabled());   // one switch at a time
        outcome = chip->finish("claude/orbit/api", {}, "hgs: claude/orbit/api: the session is not running");
        QCOMPARE(outcome.kind, AccountSwitch::Outcome::Refused);
        QCOMPARE(chip->tone(), ChipTone::Quiet); QVERIFY(chip->isEnabled()); QCOMPARE(chip->fullLabel(), QStringLiteral("Work"));
        chip->setSession("claude/orbit/api", "claude", "work", "Work");
        QCOMPARE(chip->tone(), ChipTone::Danger); QCOMPARE(chip->lastOutcome().text, QStringLiteral("Not moved: claude/orbit/api: the session is not running"));
        QCOMPARE(requested.size(), 4);
        // An hgs without U53 names no account: no chip.
        chip->setSession("claude/orbit/old", "claude", "", "");
        QVERIFY(!chip->isVisible());
    }
}

// U54: the team the account move tests use. The lead and Backend run on Work,
// the Reviewer on the default account, QA is a Codex member, the Designer runs
// on another computer and the Tester is a Kimi member there.
QJsonObject moveTeam()
{
    auto t = team();
    const auto member = [](const char *id, const char *role, const char *agent, const char *session, const char *account, const char *host = "") {
        QJsonObject m{{"id", id}, {"role", role}, {"agent", agent}, {"state", "active"}, {"account", ""}, {"session_account", account},
                      {"session", QJsonObject{{"host", host}, {"name", session}, {"run_id", QStringLiteral("run-") + id}}}};
        if (*host) m["computer"] = host;
        return m;
    };
    t["members"] = QJsonArray{member("lead", "Architect", "claude", "claude/checkout/lead", "work"),
                              member("backend", "Backend", "claude", "claude/checkout/backend", "work"),
                              member("tester", "Tester", "kimi", "kimi/checkout/tester", "native-kimi", "msi-old"),
                              member("reviewer", "Reviewer", "claude", "claude/checkout/reviewer", "native-claude"),
                              member("qa", "QA", "codex", "codex/checkout/qa", "native-codex"),
                              member("designer", "Designer", "claude", "claude/checkout/designer", "work", "gb10")};
    return t;
}

QJsonObject withMember(QJsonObject team, const QString &id, const std::function<void(QJsonObject &)> &change)
{
    auto members = team["members"].toArray();
    for (int i = 0; i < members.size(); ++i) {
        auto m = members[i].toObject();
        if (m["id"].toString() == id) { change(m); members[i] = m; }
    }
    team["members"] = members;
    return team;
}

const char *moveAccounts = R"({"profiles":[
 {"id":"native-claude","provider":"claude","label":"Default account","account_status":{"status":"ok"}},
 {"id":"work","provider":"claude","label":"Work","account_status":{"status":"ok"}},
 {"id":"second","provider":"claude","label":"Second","account_status":{"status":"ok"}},
 {"id":"old","provider":"claude","label":"Old","account_status":{"status":"signed_out"}},
 {"id":"native-codex","provider":"codex","label":"Default account","account_status":{"status":"ok"}}]})";

// A shot while a held hgs call runs: shot() would wait for that call.
void shotWhileHeld(QWidget *widget, const QString &name)
{
    const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (dir.isEmpty()) return;
    QCoreApplication::processEvents();
    if (!widget->grab().save(QDir(dir).filePath(name + ".png"))) qWarning() << "screenshot not saved:" << name;
}

QFrame *memberCard(QWidget *root, const QString &id)
{
    for (auto *card : root->findChildren<QFrame *>("teamWorkspaceMember"))
        if (card->property("teamMember") == id && card->isVisibleTo(root)) return card;
    return nullptr;
}

QString cardAccount(QWidget *root, const QString &id)
{
    auto *card = memberCard(root, id);
    auto *shown = card ? card->findChild<QLabel *>("teamWorkspaceMemberAccount") : nullptr;
    return shown ? shown->text() : QString();
}

QString cardStep(QWidget *root, const QString &id)
{
    auto *card = memberCard(root, id);
    auto *step = card ? card->findChild<QWidget *>("teamWorkspaceMemberMove") : nullptr;
    return step ? step->property("step").toString() : QString();
}

// U54: the members menu's Move to another account names every member of the
// agent whose session runs on the chosen account, the lead included, asks
// once, sends one swap-member with from_account, to_account and provider, and
// shows each member's progress: moving, then resumed, fresh and launched; a
// refusal names its reason. The cards show the account each session runs on.
void TestTeamsUi::moveToAnotherAccountNamesTheMembersAndAsksOnce()
{
    for (const bool dark : {true, false}) {
        if (dark) darkPalette(); else lightPalette();
        const QString theme = dark ? QStringLiteral("dark") : QStringLiteral("light");
        QFile::remove(script() + ".calls");
        QFile::remove(script() + ".swaprefuse");
        setTeam(moveTeam());
        QVERIFY(FakeCli::replace(script() + ".accounts", moveAccounts));
        HgsClient client(FakeCli::program(script()));
        TeamController controller(&client);
        TeamPanel panel(&controller);
        TeamWorkspace workspace(&controller, &panel);
        workspace.setTheme(dark);
        workspace.setLive(live);
        workspace.resize(1300, 820); workspace.show();
        controller.refresh();
        panel.showTeam("team-1");
        workspace.showTeam("team-1");
        // Each card shows the account its session runs on, by its label.
        QTRY_COMPARE(cardAccount(&workspace, "lead"), QStringLiteral("Work"));
        QCOMPARE(cardAccount(&workspace, "backend"), QStringLiteral("Work"));
        QCOMPARE(cardAccount(&workspace, "reviewer"), QStringLiteral("Default account"));
        QCOMPARE(memberCard(&workspace, "lead")->findChild<QLabel *>("teamWorkspaceMemberAccount")->property("account").toString(), QStringLiteral("work"));
        // The lead's roster names no account: the empty one hid that it ran on Work.
        QVERIFY(memberCard(&workspace, "lead")->findChild<QLabel *>("teamWorkspaceMemberAccount")->toolTip().contains(QStringLiteral("The roster names Default account")));
        // A Kimi member cannot move in place; the menu says why.
        inspectMemberMenu(&workspace, "tester", [](QMenu *menu) {
            QAction *move = nullptr;
            for (auto *action : menu->actions()) if (action->objectName() == "teamWorkspaceMemberMoveAccount") move = action;
            QVERIFY(move);
            QVERIFY(!move->isEnabled());
        });
        chooseMenuAction("teamWorkspaceMemberMoveAccount");
        memberCard(&workspace, "backend")->findChild<QToolButton *>("teamWorkspaceMemberMenu")->click();
        QDialog *dialog = nullptr;
        QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamMoveAccountDialog")) && dialog->isVisible());
        auto *agent = dialog->findChild<QComboBox *>("teamMoveAccountAgent");
        auto *from = dialog->findChild<QComboBox *>("teamMoveAccountFrom");
        auto *to = dialog->findChild<QComboBox *>("teamMoveAccountTo");
        auto *apply = dialog->findChild<QPushButton *>("teamMoveAccountApply");
        QCOMPARE(agent->currentData().toString(), QStringLiteral("claude"));
        QCOMPARE(agent->findData("kimi"), -1);
        QTRY_COMPARE(from->currentData().toString(), QStringLiteral("work"));
        QVERIFY(from->findData("native-claude") >= 0);
        // To: the agent's other signed-in accounts on this computer.
        QTRY_VERIFY(to->findData("second") >= 0);
        QVERIFY(to->findData("native-claude") >= 0);
        QCOMPARE(to->findData("work"), -1);
        QCOMPARE(to->findData("old"), -1);
        const auto rows = [&] {
            QStringList ids;
            for (auto *line : dialog->findChildren<QWidget *>("teamMoveAccountMember")) ids << line->property("member").toString();
            return ids;
        };
        // The lead too; not the member on another computer.
        QCOMPARE(rows(), (QStringList{"lead", "backend"}));
        QVERIFY(dialog->findChild<QLabel *>("teamMoveAccountHead")->text().contains(QStringLiteral("2 members")));
        from->setCurrentIndex(from->findData("native-claude"));
        QTRY_COMPARE(rows(), QStringList{"reviewer"});
        QVERIFY(to->findData("work") >= 0);
        from->setCurrentIndex(from->findData("work"));
        QTRY_COMPARE(rows(), (QStringList{"lead", "backend"}));
        to->setCurrentIndex(to->findData("second"));
        QVERIFY(apply->isEnabled());
        shot(dialog, "u54-move-dialog-" + theme);
        // Cancel at the confirmation sends nothing.
        armConfirm(&panel, true);
        apply->click();
        QTest::qWait(100);
        QVERIFY(calls(script(), "swap-member").isEmpty());
        QVERIFY(workspace.findChild<QDialog *>("teamMoveAccountDialog"));
        // One confirmation names the members; one call moves them.
        { QFile hold(script() + ".swaphold"); QVERIFY(hold.open(QIODevice::WriteOnly)); }
        QString name, asked;
        armConfirmRecord(&panel, false, &name, &asked);
        apply->click();
        QTRY_COMPARE(calls(script(), "swap-member").size(), 1);
        QCOMPARE(name, QStringLiteral("teamMoveAccountConfirm"));
        QVERIFY2(asked.contains(QStringLiteral("Move Lead, Backend from Work to Second?")), qPrintable(asked));
        QVERIFY(!asked.contains(QStringLiteral(" · ")));
        const auto input = calls(script(), "swap-member").first();
        QCOMPARE(input["team"].toString(), QStringLiteral("team-1"));
        QCOMPARE(input["from_account"].toString(), QStringLiteral("work"));
        QCOMPARE(input["to_account"].toString(), QStringLiteral("second"));
        QCOMPARE(input["provider"].toString(), QStringLiteral("claude"));
        QVERIFY(!input.contains("member"));
        QVERIFY(!input["request_id"].toString().isEmpty());
        const auto steps = [&] {
            QStringList found;
            for (auto *mark : dialog->findChildren<QWidget *>("teamMoveAccountStep")) found << mark->property("step").toString();
            return found;
        };
        // While it runs every listed member shows Moving, in the dialog and on its card.
        QTRY_COMPARE(steps(), (QStringList{"moving", "moving"}));
        QTRY_COMPARE(cardStep(&workspace, "lead"), QStringLiteral("moving"));
        QCOMPARE(cardStep(&workspace, "reviewer"), QString());
        QVERIFY(!apply->isVisible());
        QVERIFY(!from->isEnabled());
        shotWhileHeld(dialog, "u54-move-progress-" + theme);
        shotWhileHeld(&workspace, "u54-move-moving-cards-" + theme);
        // The lead kept its conversation; Backend fell back to a fresh session.
        auto moved = withMember(moveTeam(), "lead", [](QJsonObject &m) { m["session_account"] = "second"; m["account"] = "second"; });
        moved = withMember(moved, "backend", [](QJsonObject &m) { m.remove("session"); m["state"] = "not_created"; m["account"] = "second"; m["session_account"] = "second"; });
        setTeam(moved);
        QVERIFY(FakeCli::replace(script() + ".swapreceipt",
            R"({"status":"swapped","members":[{"status":"swapped","member":"lead"},{"status":"swapped","member":"backend"}],"fresh":["backend"]})"));
        QFile::remove(script() + ".swaphold");
        QTRY_VERIFY2(steps() == (QStringList{"resumed", "fresh"}),
                     qPrintable(steps().join(',') + QLatin1Char(' ') + dialog->findChild<QLabel *>("teamMoveAccountError")->text()));
        QTRY_COMPARE(cardAccount(&workspace, "lead"), QStringLiteral("Second"));
        // Its new session starts: Launched.
        setTeam(withMember(moved, "backend", [](QJsonObject &m) {
            m["state"] = "active"; m["session"] = QJsonObject{{"host", ""}, {"name", "claude/checkout/backend"}, {"run_id", "run-new"}};
        }));
        controller.refresh();
        QTRY_COMPARE(steps(), (QStringList{"resumed", "launched"}));
        QTRY_COMPARE(cardStep(&workspace, "backend"), QStringLiteral("launched"));
        QVERIFY(dialog->findChild<QLabel *>("teamMoveAccountHead")->text().contains(QStringLiteral("done")));
        shot(dialog, "u54-move-done-" + theme);
        shot(&workspace, "u54-move-cards-" + theme);
        // Closing a finished move clears its steps from the cards.
        dialog->findChild<QPushButton *>("teamMoveAccountClose")->click();
        QTRY_VERIFY(!workspace.findChild<QDialog *>("teamMoveAccountDialog"));
        QTRY_COMPARE(cardStep(&workspace, "lead"), QString());
        // A refusal names its reason for each listed member.
        QVERIFY(FakeCli::replace(script() + ".swaprefuse", "hgs: No member of team Checkout redesign could move to account work"));
        chooseMenuAction("teamWorkspaceMemberMoveAccount");
        memberCard(&workspace, "reviewer")->findChild<QToolButton *>("teamWorkspaceMemberMenu")->click();
        QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamMoveAccountDialog")) && dialog->isVisible());
        from = dialog->findChild<QComboBox *>("teamMoveAccountFrom");
        to = dialog->findChild<QComboBox *>("teamMoveAccountTo");
        QTRY_COMPARE(from->currentData().toString(), QStringLiteral("native-claude"));
        QTRY_VERIFY(to->findData("work") >= 0);
        to->setCurrentIndex(to->findData("work"));
        armConfirm(&panel, false);
        dialog->findChild<QPushButton *>("teamMoveAccountApply")->click();
        QTRY_COMPARE(steps(), QStringList{"failed"});
        auto *error = dialog->findChild<QLabel *>("teamMoveAccountError");
        QTRY_VERIFY(error->isVisible());
        QCOMPARE(error->text(), QStringLiteral("No member of team Checkout redesign could move to account work"));
        shot(dialog, "u54-move-refused-" + theme);
        dialog->close();
        QTRY_VERIFY(!workspace.findChild<QDialog *>("teamMoveAccountDialog"));
    }
    darkPalette();
}

// U54 (#619): a member whose old session is busy waits with that reason and
// Move again; a folder left on another branch names both branches and offers
// the checkout, only for a local folder: after one confirmation it runs git
// switch (a dirty folder is refused and the card says so), then relaunches.
void TestTeamsUi::memberBlocksShowTheirReasonAndReviewAction()
{
    QTemporaryDir repo;
    QVERIFY(repo.isValid());
    const QString folder = QDir(repo.path()).filePath("backend");
    QVERIFY(QDir().mkpath(folder));
    const auto git = [&](const QStringList &args) {
        QProcess process; process.setWorkingDirectory(folder);
        process.start("git", QStringList{"-c", "user.name=Test", "-c", "user.email=test@example.invalid"} + args);
        process.waitForFinished(30000);
        return process.exitCode() == 0 ? QString::fromUtf8(process.readAllStandardOutput()).trimmed() : QStringLiteral("<failed>");
    };
    QVERIFY(git({"init", "-q"}) != "<failed>");
    QVERIFY(git({"checkout", "-q", "-b", "team/t/backend"}) != "<failed>");
    { QFile f(QDir(folder).filePath("a.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("a"); }
    QVERIFY(git({"add", "."}) != "<failed>");
    QVERIFY(git({"commit", "-qm", "base"}) != "<failed>");
    QVERIFY(git({"checkout", "-q", "-b", "team/t/backend-3"}) != "<failed>");
    { QFile f(QDir(folder).filePath("b.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("b"); }
    QVERIFY(git({"add", "."}) != "<failed>");
    QVERIFY(git({"commit", "-qm", "task"}) != "<failed>");
    const QString branchError = folder + " is on team/t/backend-3, not team/t/backend; team/t/backend-3 is not merged into team/t/backend. "
                                         "Merge it, then check out team/t/backend in the folder.";
    auto blocked = withMember(moveTeam(), "backend", [&](QJsonObject &m) {
        m.remove("session"); m["state"] = "not_created"; m["directory"] = folder; m["error"] = branchError;
    });
    blocked = withMember(blocked, "reviewer", [](QJsonObject &m) {
        m["error"] = "the account switch is rolled_back; waiting for the old session to finish its turn";
    });
    blocked = withMember(blocked, "qa", [](QJsonObject &m) {
        m["error"] = "the account switch is uncertain; the old session was not paused: it is gone";
    });
    // A folder on another computer gets the reason, never the checkout.
    blocked = withMember(blocked, "designer", [](QJsonObject &m) {
        m.remove("session"); m["state"] = "not_created";
        m["error"] = "/srv/designer is on team/t/designer-2, not team/t/designer; it has uncommitted changes, so Zerus did not switch it.";
    });
    for (const bool dark : {true, false}) {
        if (dark) darkPalette(); else lightPalette();
        const QString theme = dark ? QStringLiteral("dark") : QStringLiteral("light");
        QFile::remove(script() + ".calls");
        setTeam(blocked);
        QVERIFY(FakeCli::replace(script() + ".accounts", moveAccounts));
        HgsClient client(FakeCli::program(script()));
        TeamController controller(&client);
        TeamPanel panel(&controller);
        TeamWorkspace workspace(&controller, &panel);
        workspace.setTheme(dark);
        workspace.setLive(live);
        workspace.resize(1300, 900); workspace.show();
        controller.refresh();
        panel.showTeam("team-1");
        workspace.showTeam("team-1");
        QTRY_VERIFY(memberCard(&workspace, "backend") && memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout"));
        const auto stateOf = [&](const QString &id) {
            auto *card = memberCard(&workspace, id);
            auto *state = card ? card->findChild<QWidget *>("teamWorkspaceMemberState") : nullptr;
            return state ? state->toolTip() : QString();
        };
        QVERIFY2(stateOf("backend").contains(QStringLiteral("Folder on another branch")), qPrintable(stateOf("backend")));
        auto *reason = memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberBlockReason");
        QVERIFY(reason);
        QCOMPARE(reason->text(), QStringLiteral("Its folder is on team/t/backend-3, not team/t/backend"));
        QVERIFY(reason->toolTip().contains(QStringLiteral("Merge it, then check out team/t/backend")));
        auto *checkout = memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout");
        QCOMPARE(checkout->text(), QStringLiteral("Check out team/t/backend…"));
        // The remote folder: its reason only.
        QVERIFY(memberCard(&workspace, "designer")->findChild<QWidget *>("teamWorkspaceMemberBlockReason"));
        QVERIFY(!memberCard(&workspace, "designer")->findChild<QPushButton *>("teamWorkspaceMemberCheckout"));
        // A busy old session waits; Move again once it is idle.
        QVERIFY2(stateOf("reviewer").contains(QStringLiteral("Waiting for its old session")), qPrintable(stateOf("reviewer")));
        QVERIFY(memberCard(&workspace, "reviewer")->findChild<QPushButton *>("teamWorkspaceMemberMoveAgain"));
        QVERIFY2(stateOf("qa").contains(QStringLiteral("Account switch unconfirmed")), qPrintable(stateOf("qa")));
        QVERIFY(memberCard(&workspace, "qa")->findChild<QPushButton *>("teamWorkspaceMemberOpenSession"));
        shot(&workspace, "u54-member-blocks-" + theme);
        if (!dark) continue;
        // Move again opens the move with the member's account.
        memberCard(&workspace, "reviewer")->findChild<QPushButton *>("teamWorkspaceMemberMoveAgain")->click();
        QDialog *dialog = nullptr;
        QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamMoveAccountDialog")) && dialog->isVisible());
        QTRY_COMPARE(dialog->findChild<QComboBox *>("teamMoveAccountFrom")->currentData().toString(), QStringLiteral("native-claude"));
        dialog->close();
        QTRY_VERIFY(!workspace.findChild<QDialog *>("teamMoveAccountDialog"));
        // A dirty folder is never switched; the card says why.
        { QFile f(QDir(folder).filePath("dirty.txt")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write("x"); }
        QString asked;
        armConfirmRecord(&panel, false, nullptr, &asked);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTRY_VERIFY(memberCard(&workspace, "backend") && memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError"));
        QVERIFY2(asked.contains(folder) && asked.contains(QStringLiteral("from team/t/backend-3 to team/t/backend")), qPrintable(asked));
        QVERIFY(asked.contains(QStringLiteral("without --force")));
        QVERIFY(memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError")->text().contains(QStringLiteral("uncommitted changes")));
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend-3"));
        QVERIFY(calls(script(), "relaunch").isEmpty());
        shot(&workspace, "u54-checkout-refused-dark");
        // Clean: switched, the task branch kept, then relaunched.
        QVERIFY(QFile::remove(QDir(folder).filePath("dirty.txt")));
        armConfirm(&panel, false);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTRY_COMPARE(calls(script(), "relaunch").size(), 1);
        QCOMPARE(calls(script(), "relaunch").first()["member"].toString(), QStringLiteral("backend"));
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend"));
        QVERIFY(git({"rev-parse", "--verify", "team/t/backend-3"}) != "<failed>");
        QTRY_VERIFY(!memberCard(&workspace, "backend")->findChild<QLabel *>("teamWorkspaceMemberCheckoutError"));
        // Cancel at its confirmation changes nothing.
        QVERIFY(git({"checkout", "-q", "team/t/backend-3"}) != "<failed>");
        armConfirm(&panel, true);
        memberCard(&workspace, "backend")->findChild<QPushButton *>("teamWorkspaceMemberCheckout")->click();
        QTest::qWait(300);
        QCOMPARE(git({"rev-parse", "--abbrev-ref", "HEAD"}), QStringLiteral("team/t/backend-3"));
        QCOMPARE(calls(script(), "relaunch").size(), 1);
    }
    darkPalette();
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

void TestTeamsUi::heatTintsGraphAndBoardByTimeOrTokens()
{
    // Off by default: no remembered choice for this team or another one.
    QSettings().remove("teams/heat/team-1"); QSettings().remove("teams/heat/team-2");
    const auto forget = qScopeGuard([this] {
        QSettings().remove("teams/heat/team-1"); QSettings().remove("teams/heat/team-2");
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
        shot(workspace.get(), name + "-dark");
        lightPalette(); workspace->setTheme(false); workspace->render(); QTest::qWait(200);
        lightShot(workspace.get(), name + "-light");
        darkPalette(); workspace->setTheme(true); workspace->render(); QTest::qWait(150);
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
    QVERIFY(lost(483)->geometry().left() > card(483)->findChild<QWidget *>("teamBoardRole")->geometry().right());
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
    QVERIFY(!QSettings().contains("teams/heat/team-1"));
}

namespace {
// #706: a small running team: Backend works on #1, the lead waits on #2.
QJsonObject liveTeam()
{
    return QJsonDocument::fromJson(R"JSON({
 "id":"team-1","name":"Live","status":"running","project":"Orbit platform","folder":"/work/orbit","lead":"lead",
 "revision":1,"created":1000,"updated":5000,"settings":{},"layers":{},"cycle":[],"ready":[],"validation":{},
 "members":[
  {"id":"lead","role":"Lead","agent":"claude","model":"claude-opus-4-6","state":"active","session":{"host":"","name":"claude/orbit/live"}},
  {"id":"backend","role":"Backend","agent":"codex","model":"gpt-6.1-sol","state":"active","session":{"host":"","name":"codex/orbit/api"}}],
 "phases":[{"n":1,"title":"Build","state":"approved","tasks":[1,2],"added_by":"Lead","added_at":1000}],
 "tasks":[
  {"id":1,"phase":1,"title":"Saved cards API","assignee":"backend","status":"in_progress",
   "attempts":[{"n":1,"id":"a1","member":"backend","state":"delivered","sent_at":1800002000,"started_at":1800002010}]},
  {"id":2,"phase":1,"title":"Release notes","assignee":"lead","status":"pending","deps":[{"task":1}]}]})JSON").object();
}
}

// #706: a member going from working to idle changes its activity label on
// the Board and its node on the Graph in place, and its mark in Members is
// set again where it is: the canvas, the cards and the column stay. A live change that makes a problem
// (the member goes offline) rebuilds as before.
void TestTeamsUi::aMembersLiveStateChangesInPlace()
{
    setTeam(liveTeam());
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamPanel panel(&controller);
    TeamWorkspace workspace(&controller, &panel);
    bool working = true, offline = false;
    workspace.setLive([&](const Member &m) {
        Live l; l.known = true;
        if (m.id == "backend") { l.working = working; l.offline = offline; l.attentionText = working ? "cargo test" : "cargo build"; }
        return l;
    });
    darkPalette(); workspace.setTheme(true);
    workspace.resize(1500, 640); workspace.show();
    controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
    QTRY_VERIFY(workspace.findChild<QWidget *>("teamWorkspaceView"));
    const auto view = [&](const char *segment) {
        for (auto *b : workspace.findChild<QWidget *>("teamWorkspaceView")->findChildren<QPushButton *>()) if (b->property("segment") == segment) b->click();
    };
    view("board");
    const auto area = [&] { return workspace.findChild<QScrollArea *>("teamWorkspaceBoard"); };
    QTRY_VERIFY(area() && boardCard(area(), 1) && controller.watched());
    TestWait::settle();
    const auto activity = [&] { auto *l = boardCard(area(), 1)->findChild<QLabel *>("teamBoardActivity"); return l ? l->text() : QString(); };
    const auto mark = [&]() -> TeamUi::StatusMark * {
        for (auto *card : workspace.findChildren<QFrame *>("teamWorkspaceMember"))
            if (card->property("teamMember") == "backend") return dynamic_cast<TeamUi::StatusMark *>(card->findChild<QWidget *>("teamWorkspaceMemberState"));
        return nullptr;
    };
    const auto side = [&] { return workspace.findChild<QWidget *>("teamWorkspaceSide"); };
    QCOMPARE(activity(), QStringLiteral("working"));
    QVERIFY(mark());
    QPointer<QScrollArea> canvas = area();
    QPointer<QFrame> card = boardCard(area(), 1);
    QPointer<QWidget> column = side();
    QPointer<QWidget> state = mark();
    // The session list's poll names the same team: Backend is idle now.
    working = false;
    workspace.showTeam("team-1");
    QTRY_COMPARE(activity(), QStringLiteral("idle"));
    QVERIFY(canvas && canvas == area() && card && card == boardCard(area(), 1));
    QVERIFY(column && column == side() && state && state == mark());
    const Live idle = [] { Live l; l.known = true; return l; }();
    QCOMPARE(mark()->text(), Teams::memberStatus(*controller.watched(), *controller.watched()->member("backend"), idle).text);
    shot(&workspace, "u706-board-idle-in-place-dark");
    // The Graph's node: its activity, the view kept.
    view("graph");
    const auto graph = [&]() -> TeamUi::TaskView * {
        for (auto *w : workspace.findChildren<QWidget *>()) if (auto *v = dynamic_cast<TeamUi::TaskView *>(w); v && !v->nodes.isEmpty()) return v;
        return nullptr;
    };
    const auto node = [&](int id) { TeamUi::TaskView::Node found; if (auto *v = graph()) for (const auto &n : v->nodes) if (n.id == id) found = n; return found; };
    QTRY_COMPARE(node(1).activity, QStringLiteral("idle"));
    TestWait::settle();
    QPointer<QWidget> graphView = graph();
    working = true;
    workspace.showTeam("team-1");
    QTRY_COMPARE(node(1).activity, QStringLiteral("working"));
    QVERIFY(node(1).working);
    QVERIFY(graphView && graphView == graph());
    // Offline is a problem: the team's problems change, so it rebuilds.
    offline = true;
    workspace.showTeam("team-1");
    QTRY_VERIFY(!graphView || graphView != graph());
    QVERIFY(node(1).problem);
}

// #706: a `team list` read that moves only the revision and the updated time
// keeps the new values and tells nobody (the session list used to rebuild
// all its rows for it); a change it shows does. The team panel's refresh
// rebuilds only for a change it shows.
void TestTeamsUi::aListPollThatMovesOnlyTheRevisionChangesNothing()
{
    auto team = liveTeam();
    setTeam(team);
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    QSignalSpy changed(&controller, &TeamController::teamsChanged);
    const auto listReads = [&] { return calls(script(), "list").size(); };
    controller.refresh();
    QTRY_COMPARE(changed.count(), 1);
    for (int i = 1; i <= 2; ++i) {
        team["revision"] = 1 + i; team["updated"] = 5000 + 60 * i;
        setTeam(team);
        const auto before = listReads();
        controller.refresh();
        QTRY_VERIFY(listReads() > before && controller.team("team-1")->revision == 1 + i);
        QTest::qWait(50);
        QCOMPARE(changed.count(), 1);
    }
    team["name"] = "Live checkout";
    setTeam(team);
    controller.refresh();
    QTRY_COMPARE(changed.count(), 2);
    // The panel: a poll and the window's refresh with nothing new keep it.
    TeamPanel panel(&controller);
    panel.resize(360, 640); panel.show();
    panel.showTeam("team-1");
    QTRY_VERIFY(controller.watched() && panel.findChild<QWidget *>("teamPanelContent"));
    TestWait::settle();
    QPointer<QWidget> content = panel.findChild<QWidget *>("teamPanelContent");
    const auto statusReads = [&] { return calls(script(), "status").size(); };
    team["revision"] = 9; team["updated"] = 6000;
    setTeam(team);
    const auto before = statusReads();
    controller.refresh();
    QTRY_VERIFY(statusReads() > before && controller.watched()->revision == 9);
    panel.refresh();
    QTest::qWait(50);
    QVERIFY(content && content == panel.findChild<QWidget *>("teamPanelContent"));
    // A shown change: #1 is submitted.
    auto tasks = team["tasks"].toArray();
    auto first = tasks[0].toObject(); first["status"] = "submitted"; tasks[0] = first;
    team["tasks"] = tasks;
    setTeam(team);
    controller.refresh();
    QTRY_VERIFY(!content || content != panel.findChild<QWidget *>("teamPanelContent"));
}

// #706: the waiting list read every 30 s redraws its banners only when the
// list changed, and asks for no team poll of its own.
void TestTeamsUi::aWaitingReadThatChangesNothingRedrawsNothing()
{
    QSettings().remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { QSettings().remove(TeamWaitingNotices::kSetting); });
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

namespace {
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

// #724 (owner, 2026-10-30): Waiting for you answers a one-click decision in
// its row, with the team view's command and confirmation, bound to the item:
// the lead's proposal (Deny with a reason, Allow), an addition (the typed
// "add", Add or Cancel), an escalated permission (Allow, Deny) and a phase
// draft (Approve… with the team's question). A report and a held task keep
// Open team only. A stale item says so and sends nothing; an answered one
// leaves at once.
void TestTeamsUi::waitingItemsAreAnsweredInPlace()
{
    QSettings().setValue("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); darkPalette(); });
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
    // A report and a held task are read in the team first: Open team only.
    for (const auto &id : {report, held}) {
        QVERIFY(row(id)->findChild<QPushButton *>("teamWaitingOpen"));
        QVERIFY(!row(id)->findChild<QPushButton *>("teamWaitingAnswerToggle") && !row(id)->findChild<QPushButton *>("teamWaitingApprove"));
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
    shot(&page, "u724-page-answers-dark");
    page.setDark(false);
    lightShot(&page, "u724-page-answers-light");
    darkPalette(); page.setDark(true);
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
    // The addition: Cancel closes it and sends nothing; Add wants the typed word.
    row(addition)->findChild<QPushButton *>("teamWaitingAdditionCancel")->click();
    QTRY_VERIFY(!row(addition)->findChild<QLineEdit *>("teamWaitingAdditionConfirm"));
    QCOMPARE(calls(script(), "add-to-phase").size(), 0);
    toggle(addition);
    QTRY_VERIFY(row(addition)->findChild<QLineEdit *>("teamWaitingAdditionConfirm"));
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

// #742 (owner, 2026-10-31): a test or build tool the project lacks on this
// computer is offered in Waiting for you, without a team: why it is needed,
// what it saves and the install command; Deny with a reason, or Allow, sent as
// the owner's decide-tool. A failed install shows its output and retries.
void TestTeamsUi::waitingToolOffersAreAnsweredInPlace()
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

// #724: the banner answers in its row too; an item answered there leaves the
// banner and the page at once, and its notifications (the waiting one and the
// team attention one that covers it) are withdrawn.
void TestTeamsUi::waitingBannerAnswersInPlaceAndWithdrawsTheNotice()
{
    QSettings().setValue("workspace/skin", "whitewh");
    QSettings().remove(TeamWaitingNotices::kSetting);
    const auto restore = qScopeGuard([] { QSettings().remove("workspace/skin"); QSettings().remove(TeamWaitingNotices::kSetting); darkPalette(); });
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
    QCOMPARE(bannerRow(held)->findChild<QPushButton *>("teamWaitingBannerAction")->text(), QStringLiteral("Open team"));
    QVERIFY(!bannerRow(held)->findChild<QToolButton *>("teamWaitingBannerOpen"));
    auto *answer = bannerRow(permission)->findChild<QPushButton *>("teamWaitingBannerAction");
    QCOMPARE(answer->text(), QStringLiteral("Answer"));
    QVERIFY(bannerRow(permission)->findChild<QToolButton *>("teamWaitingBannerOpen"));
    answer->click();
    QTRY_VERIFY(bannerRow(permission) && bannerRow(permission)->findChild<QPushButton *>("teamWaitingPermissionDeny"));
    TestWait::settle();
    shot(banner, "u724-banner-permission-dark");
    banner->setDark(false);
    lightShot(banner, "u724-banner-permission-light");
    darkPalette(); banner->setDark(true);
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

// #738 (owner, 2026-10-31: with Kimi as the lead the model picker was empty):
// New team's model pickers list the selected account's native catalog (as the
// role editor reads it; OpenCode's model_options from its account row), fall
// back to Teams::knownModels when the catalog is empty, refill when the agent
// or the account changes, and keep a typed model. The effort picker follows
// the chosen model's efforts where the catalog names them.
void TestTeamsUi::newTeamOffersEachAgentsModels()
{
    const auto restore = qScopeGuard([] { darkPalette(); });
    darkPalette();
    const auto profile = [](const QString &id, const QString &provider, const QString &label) {
        return QJsonObject{{"id", id}, {"provider", provider}, {"label", label}, {"account_status", QJsonObject{{"status", "ok"}}}};
    };
    auto opencode = profile("native-opencode", "opencode", "OpenCode");
    opencode["model_options"] = QJsonArray{QJsonObject{{"id", "openrouter/qwen3-coder"}, {"label", "Qwen3 Coder"}, {"effort_options", QJsonArray{"low", "high"}}}};
    const QJsonArray profiles{profile("native-claude", "claude", "Claude"), profile("work", "claude", "Claude work"),
                              profile("native-codex", "codex", "Codex"), profile("native-kimi", "kimi", "Kimi"),
                              profile("kimi-new", "kimi", "Kimi new"), profile("native-dsh", "dsh", "DeepSeek"), opencode};
    QVERIFY(FakeCli::replace(script() + ".accounts", QJsonDocument(QJsonObject{{"profiles", profiles}}).toJson(QJsonDocument::Compact)));
    const auto model = [](const QString &id, const QStringList &efforts) {
        return QJsonObject{{"id", id}, {"effort_options", QJsonArray::fromStringList(efforts)}};
    };
    const QJsonObject catalogs{
        {"native-claude", QJsonArray{model("claude-opus-4-7", {"low", "medium", "high", "xhigh"}), model("claude-sonnet-4-6", {"low", "medium", "high"}),
                                  model("claude-haiku-4-5", {})}},
        {"work", QJsonArray{model("claude-opus-4-6", {"low", "high"})}},
        {"native-codex", QJsonArray{model("gpt-6.1-sol", {"low", "medium", "high", "xhigh"}), model("gpt-5.5", {"low", "medium"})}},
        {"native-kimi", QJsonArray{model("kimi-code/k3.1-pro", {"low", "high"}), model("kimi-code/kimi-for-coding", {})}},
        {"kimi-new", QJsonArray{}},   // not read yet: the known Kimi models
        {"native-dsh", QJsonArray{QJsonObject{{"id", "deepseek/deepseek-v4-pro"}, {"label", "DeepSeek V4 Pro (DeepSeek)"},
                                              {"effort_options", QJsonArray{"high", "max"}}}}}};
    QVERIFY(FakeCli::replace(script() + ".models", QJsonDocument(catalogs).toJson(QJsonDocument::Compact)));
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.refreshCatalog();
    QTRY_VERIFY(controller.catalogReady());
    QTemporaryDir folder;
    TeamNewDialog dialog(&controller, &client, {{QString(), "this computer", true}}, {}, QDir::toNativeSeparators(folder.path()), "Checkout");
    dialog.resize(900, 640);
    dialog.show();
    auto *agent = dialog.findChild<QComboBox *>("teamNewLeadAgent");
    auto *account = dialog.findChild<QComboBox *>("teamNewLeadAccount");
    auto *leadModel = dialog.findChild<QComboBox *>("teamNewLeadModel");
    auto *effort = dialog.findChild<QComboBox *>("teamNewLeadEffort");
    QVERIFY(agent && account && leadModel && effort);
    const auto items = [](QComboBox *box) { QStringList list; for (int i = 0; i < box->count(); ++i) list << box->itemText(i); return list; };
    const auto efforts = [](QComboBox *box) { QStringList list; for (int i = 0; i < box->count(); ++i) list << box->itemData(i).toString(); return list; };
    const auto choose = [](QComboBox *box, const QString &data) { box->setCurrentIndex(box->findData(data)); };
    const auto pick = [](QComboBox *box, const QString &text) { box->setCurrentIndex(box->findText(text)); };
    const QStringList anyEffort{"", "low", "medium", "high", "xhigh"};
    // Kimi leads: its kimi-code models, no model chosen (the agent's default).
    choose(agent, "kimi");
    QTRY_COMPARE(items(leadModel), (QStringList{"kimi-code/k3.1-pro", "kimi-code/kimi-for-coding"}));
    QCOMPARE(leadModel->currentText(), QString());
    pick(leadModel, "kimi-code/k3.1-pro");
    QCOMPARE(efforts(effort), (QStringList{"", "low", "high"}));
    QCOMPARE(effort->currentData().toString(), QStringLiteral("high"));   // the lead's high stays where the model has it
    shot(&dialog, "u738-new-team-kimi-dark");
    // Another Kimi account whose catalog is empty: the known Kimi models.
    choose(account, "kimi-new");
    QTRY_COMPARE(items(leadModel), Teams::knownModels("kimi"));
    QCOMPARE(leadModel->currentText(), QStringLiteral("kimi-code/k3.1-pro"));   // still listed: kept
    // Claude: the default account's catalog, then the work account's.
    choose(agent, "claude");
    QTRY_COMPARE(items(leadModel), (QStringList{"claude-opus-4-7", "claude-sonnet-4-6", "claude-haiku-4-5"}));
    pick(leadModel, "claude-sonnet-4-6");
    QCOMPARE(efforts(effort), (QStringList{"", "low", "medium", "high"}));
    shot(&dialog, "u738-new-team-claude-dark");
    choose(account, "work");
    QTRY_COMPARE(items(leadModel), QStringList{"claude-opus-4-6"});
    QCOMPARE(leadModel->currentText(), QStringLiteral("claude-sonnet-4-6"));   // a model the account lacks stays as typed
    QCOMPARE(efforts(effort), anyEffort);
    // Codex: its models and each one's efforts.
    choose(agent, "codex");
    QTRY_COMPARE(items(leadModel), (QStringList{"gpt-6.1-sol", "gpt-5.5"}));
    pick(leadModel, "gpt-6.1-sol");
    QCOMPARE(efforts(effort), (QStringList{"", "low", "medium", "high", "xhigh"}));
    pick(leadModel, "gpt-5.5");
    QCOMPARE(efforts(effort), (QStringList{"", "low", "medium"}));
    shot(&dialog, "u738-new-team-codex-dark");
    // A typed model still goes as typed, with any effort.
    leadModel->setEditText(QStringLiteral("gpt-6-preview"));
    QCOMPARE(efforts(effort), anyEffort);
    // Members: DeepSeek and OpenCode list theirs.
    dialog.findChild<QPushButton *>("teamNewAddMember")->click();
    QTRY_VERIFY(!dialog.findChildren<QComboBox *>("teamNewAgent").isEmpty());
    auto *memberAgent = dialog.findChildren<QComboBox *>("teamNewAgent").last();
    auto *memberModel = dialog.findChildren<QComboBox *>("teamNewModel").last();
    auto *memberEffort = dialog.findChildren<QComboBox *>("teamNewEffort").last();
    choose(memberAgent, "dsh");
    QTRY_COMPARE(items(memberModel), QStringList{"deepseek/deepseek-v4-pro"});
    QCOMPARE(memberModel->itemData(0, Qt::ToolTipRole).toString(), QStringLiteral("DeepSeek V4 Pro (DeepSeek)"));
    pick(memberModel, "deepseek/deepseek-v4-pro");
    QCOMPARE(efforts(memberEffort), (QStringList{"", "high", "max"}));
    choose(memberEffort, "max");
    choose(memberAgent, "opencode");
    QTRY_COMPARE(items(memberModel), QStringList{"openrouter/qwen3-coder"});
    pick(memberModel, "openrouter/qwen3-coder");
    QCOMPARE(efforts(memberEffort), (QStringList{"", "low", "high"}));
    choose(memberEffort, "high");
    // Each catalog was read once, by its account.
    QStringList read;
    QFile log(script() + ".calls");
    QVERIFY(log.open(QIODevice::ReadOnly));
    for (const auto &line : log.readAll().split('\n')) {
        const auto args = QJsonDocument::fromJson(line).object()["args"].toArray();
        if (args.size() > 2 && args[0] == "account" && args[1] == "model-settings") read << args[2].toString();
    }
    QCOMPARE(read.size(), QSet<QString>(read.cbegin(), read.cend()).size());
    QVERIFY(read.contains("native-kimi") && read.contains("native-codex") && read.contains("native-dsh") && read.contains("work"));
    QVERIFY(!read.contains("native-opencode"));
    // Create sends the typed lead model and the member's catalog model and effort.
    dialog.findChild<QLineEdit *>("teamNewName")->setText("Checkout models");
    auto *create = dialog.findChild<QPushButton *>("teamNewCreate");
    QTRY_VERIFY(create->isEnabled());
    create->click();
    QTRY_COMPARE(calls(script(), "create").size(), 1);
    const auto input = calls(script(), "create").first();
    QCOMPARE(input["lead"].toObject()["model"].toString(), QStringLiteral("gpt-6-preview"));
    const auto member = input["members"].toArray().last().toObject();
    QCOMPARE(member["agent"].toString(), QStringLiteral("opencode"));
    QCOMPARE(member["model"].toString(), QStringLiteral("openrouter/qwen3-coder"));
    QCOMPARE(member["effort"].toString(), QStringLiteral("high"));
    // The light theme: the form reads its colors when it opens.
    lightPalette();
    for (const auto &[lead, first] : {std::pair{QStringLiteral("kimi"), QStringLiteral("kimi-code/k3.1-pro")},
                                      std::pair{QStringLiteral("claude"), QStringLiteral("claude-opus-4-7")},
                                      std::pair{QStringLiteral("codex"), QStringLiteral("gpt-6.1-sol")}}) {
        TeamNewDialog light(&controller, &client, {{QString(), "this computer", true}}, {}, QDir::toNativeSeparators(folder.path()), "Checkout");
        light.resize(900, 640);
        light.show();
        auto *lightAgent = light.findChild<QComboBox *>("teamNewLeadAgent");
        auto *lightModel = light.findChild<QComboBox *>("teamNewLeadModel");
        choose(lightAgent, lead);
        QTRY_COMPARE(lightModel->itemText(0), first);
        pick(lightModel, first);
        lightShot(&light, QStringLiteral("u738-new-team-%1-light").arg(lead));
    }
}

// #709 L6: the tint is read again a minute later; that read rebuilds Graph
// and Board only when what the tint draws changed. A wait that grew by
// seconds the loss does not show keeps every widget, its hover and scroll.
void TestTeamsUi::aHeatReadThatDrawsTheSameRebuildsNothing()
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

// #737: the team's dialogs draw the workspace theme on a system palette of
// the other one: Team settings, the plan-mode switch and the account move.
void TestTeamsUi::teamDialogsDrawTheThemeNotTheSystem()
{
    const QPalette before = QApplication::palette();
    const auto restore = qScopeGuard([before] { QApplication::setPalette(before); });
    for (const bool dark : {true, false}) {
        const QString theme = dark ? QStringLiteral("dark") : QStringLiteral("light");
        QApplication::setPalette(DialogTheme::system(!dark));
        const auto check = [&](QWidget *dialog) {
            QTest::qWait(150);
            const QString found = DialogTheme::problem(dialog, dark);
            if (const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS"); !shots.isEmpty())
                dialog->grab().save(QDir(shots).filePath("u737-" + dialog->objectName() + "-" + theme + ".png"));
            QVERIFY2(found.isEmpty(), qPrintable(theme + ": " + found));
        };
        // The lead asks to switch to free planning: the dialog pops up.
        auto running = phasedTeam();
        running["plan_mode"] = "standard";
        running["proposals"] = QJsonArray{QJsonObject{{"id", "p5"}, {"kind", "switch-plan-mode"}, {"by", "Lead"}, {"at", 1800000000.0}, {"state", "open"},
                                                      {"mode", "free"}, {"hash", QStringLiteral("7f3a2c19e0d84b1c9a5e6f7081a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4")},
                                                      {"reason", "The QA bot files small fixes every day."}}};
        setTeam(running);
        {
            HgsClient client(FakeCli::program(script()));
            TeamController controller(&client);
            TeamPanel panel(&controller);
            TeamWorkspace workspace(&controller, &panel);
            workspace.setTheme(dark); workspace.setLive(live);
            workspace.resize(1400, 860); workspace.show();
            controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
            QDialog *dialog = nullptr;
            QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamSwitchModeDialog")) && dialog->isVisible());
            check(dialog);
            if (QTest::currentTestFailed()) return;
            dialog->close();
            // Team settings, as the toolbar opens it.
            QTRY_VERIFY(controller.team("team-1"));
            TeamSettingsDialog settings(&controller, *controller.team("team-1"), dark, &workspace);
            settings.show();
            QVERIFY(QTest::qWaitForWindowExposed(&settings));
            check(&settings);
            if (QTest::currentTestFailed()) return;
        }
        // Move to another account, from a member's menu.
        setTeam(moveTeam());
        QVERIFY(FakeCli::replace(script() + ".accounts", moveAccounts));
        {
            HgsClient client(FakeCli::program(script()));
            TeamController controller(&client);
            TeamPanel panel(&controller);
            TeamWorkspace workspace(&controller, &panel);
            workspace.setTheme(dark); workspace.setLive(live);
            workspace.resize(1300, 820); workspace.show();
            controller.refresh(); panel.showTeam("team-1"); workspace.showTeam("team-1");
            QTRY_COMPARE(cardAccount(&workspace, "backend"), QStringLiteral("Work"));
            chooseMenuAction("teamWorkspaceMemberMoveAccount");
            memberCard(&workspace, "backend")->findChild<QToolButton *>("teamWorkspaceMemberMenu")->click();
            QDialog *dialog = nullptr;
            QTRY_VERIFY((dialog = workspace.findChild<QDialog *>("teamMoveAccountDialog")) && dialog->isVisible());
            check(dialog);
            if (QTest::currentTestFailed()) return;
            dialog->close();
        }
    }
}

#include "test_teamsui.moc"
