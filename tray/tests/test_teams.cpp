// WhiteWh fork: agent teams in the desktop (design/agent-teams S1, S2, S8):
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
  {"id":"tester","role":"Tester","agent":"kimi","model":"kimi-k2","computer":"msi-old","state":"active","session":{"host":"msi-old","name":"kimi/checkout/tester"}},
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
   "attempts":[{"n":1,"id":"a6","member":"tester","state":"unconfirmed","error":"msi-old stopped answering"}]},
  {"id":7,"title":"E2E tests","assignee":"tester","status":"pending","deps":[{"task":5}]},
  {"id":8,"title":"Code review","assignee":"reviewer","status":"pending","deps":[{"task":4},{"task":5},{"task":6}]},
  {"id":9,"title":"Merge branches","assignee":"integrator","status":"pending","deps":[{"task":8}]},
  {"id":10,"title":"Release notes","assignee":"lead","status":"pending","deps":[{"task":9}]}],
 "layers":{"1":0,"2":1,"3":1,"4":2,"5":2,"6":2,"7":3,"8":3,"9":4,"10":5},"cycle":[],"ready":[],
 "validation":{"errors":[],"warnings":[{"kind":"offline","text":"msi-old is offline"}]}})").object();
}

QJsonObject feedPage()
{
    return QJsonDocument::fromJson(R"({"items":[
 {"seq":9,"at":4800,"kind":"unconfirmed","detail":{"task":6,"attempt":1,"error":"msi-old stopped answering"}},
 {"seq":8,"at":4700,"kind":"reported","detail":{"by":"Frontend","task":2,"title":"Address form UI","attempt":1}},
 {"seq":7,"at":4600,"kind":"handed_out","detail":{"by":"Lead","task":3,"to":"Backend","attempt":2}},
 {"seq":6,"at":4500,"kind":"rejected","detail":{"by":"Lead","task":3,"title":"Saved cards API","attempt":1,"reason":"cards list must paginate"}}],
 "next_cursor":6})").object();
}

// A running team with one phase appended: draft phase 2, a pinned task, the
// idle notice and the derived status fields hgs sends.
QJsonObject phasedTeam()
{
    auto team = checkout();
    team["settings"] = QJsonObject{{"all_hard", false}, {"report_timeout_seconds", 120},
                                   {"approval", "review"}, {"phase_approval", "auto"}};
    team["idle"] = "pending";
    team["waiting_for_lead"] = false;
    team["pending_phase"] = 2;
    team["phases"] = QJsonArray{QJsonObject{
        {"n", 2}, {"title", "QA fixes"}, {"barrier", true}, {"approval", "auto"}, {"state", "draft"},
        {"tasks", QJsonArray{11, 12}}, {"added_by", "Lead"}, {"added_at", 1792868401.5},
        {"decided_by", ""}, {"decided_at", QJsonValue()}}};
    auto tasks = team["tasks"].toArray();
    tasks.append(QJsonObject{{"id", 11}, {"phase", 2}, {"title", "Fix checkout totals"}, {"assignee", "backend"},
                             {"status", "draft"}, {"pinned", true},
                             {"deps", QJsonArray{QJsonObject{{"task", 4}, {"kind", "hard"}}}}});
    tasks.append(QJsonObject{{"id", 12}, {"phase", 2}, {"title", "Retest checkout"}, {"assignee", "tester"},
                             {"status", "draft"}, {"deps", QJsonArray{QJsonObject{{"task", 11}, {"kind", "soft"}}}}});
    team["tasks"] = tasks;
    return team;
}

// The delivery fixture of the team delivery spec (A1-A6): an attempt that
// started, a captured report that was replaced, a question to the lead, a late
// start and a task held after a rejection.
QJsonObject deliveryTeam()
{
    return QJsonDocument::fromJson(R"({
 "id":"team-2","name":"Delivery","status":"running","project":"Checkout","folder":"/work/delivery","lead":"lead",
 "revision":3,"created":1000,"updated":5000,
 "settings":{"all_hard":false,"report_timeout_seconds":120,"start_timeout_seconds":180},
 "members":[
  {"id":"lead","role":"Architect","agent":"claude","state":"active","session":{"host":"","name":"claude/delivery/lead"}},
  {"id":"backend","role":"Backend","agent":"codex","state":"active","session":{"host":"","name":"codex/delivery/backend"}},
  {"id":"tester","role":"Tester","agent":"kimi","state":"active","session":{"host":"","name":"kimi/delivery/tester"}},
  {"id":"reviewer","role":"Reviewer","agent":"claude","state":"active","session":{"host":"","name":"claude/delivery/reviewer"}}],
 "tasks":[
  {"id":1,"title":"Saved cards API","assignee":"backend","status":"in_progress",
   "attempts":[{"n":2,"id":"a2","member":"backend","state":"delivered","sent_at":1300,"delivered_at":1301,
                "started_at":1305,"phase":"working"}]},
  {"id":2,"title":"Address form","assignee":"backend","status":"submitted",
   "attempts":[{"n":1,"id":"a1","member":"backend","state":"reported","delivered_at":1100,"started_at":1105,
    "report":{"text":"The real report","at":1200,"captured":false,
              "replaced":[{"text":"Building and running the tests...","at":1150}]}}]},
  {"id":3,"title":"Held retry","assignee":"backend","status":"pending",
   "hold":{"by":"Lead","at":1400,"reason":"waiting for the owner"},
   "attempts":[{"n":1,"id":"a3","member":"backend","state":"rejected"}]},
  {"id":4,"title":"Ask about the field","assignee":"tester","status":"in_progress",
   "attempts":[{"n":1,"id":"a4","member":"tester","state":"delivered","delivered_at":1500,"started_at":1505,
    "questions":[{"id":"q1","text":"Must I keep the old JSON field?","at":1510,"answer":"","answered_at":null,"by":""},
                 {"id":"q2","text":"Which branch?","at":1501,"answer":"main","answered_at":1502,"by":"Lead"}]}]},
  {"id":5,"title":"Late start","assignee":"reviewer","status":"in_progress",
   "attempts":[{"n":1,"id":"a5","member":"reviewer","state":"delivered","delivered_at":1600,"start_warned":true}]},
  {"id":6,"title":"Untouched pending","assignee":"backend","status":"pending","deps":[{"task":1}]}]})").object();
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
    void subLeadsFormTheMemberTree();
    void modelReadsTheTeamAndDerivesMemberStates();
    void modelReadsPhasesPinnedAndTheWaitingState();
    void leftRunningMembersOfAFinishedTeam();
    void draftPhasesApproveInOrder();
    void phaseProgressShowsThePhaseInWorkAndThePhases();
    void permissionRequestsCarryTheCommandAndTheAnswer();
    void listSummaryCarriesTaskPhasesAndPhaseRecords();
    void leadLoadViewsCarryTheirNumbers();
    void permissionRequestFromTheMemberAndItsAnswer();
    void modelReadsAttemptStatesQuestionsHoldsAndReports();
    void modelReadsStructuredReportsAndFindings();
    void modelReadsTheHierarchy();
    void attemptPhasesAreDerivedForAnOlderHgs();
    void finishedRevokedAndRefusedAttemptsHaveNoPhase();
    void oldStatusJsonStillParses();
    void aDraftTeamReadsItsRevisionUnseenFieldsAndGoal();
    void proposalsSwapsLimitsAndHostsRead();
    void feedLinesSpeakOfStartsQuestionsAndHolds();
    void aRunningTeamIsStuckWhenNothingCanMove();
    void feedLinesAndChains();
    void feedLinesSpeakOfPhasesAndTheEndOfThePlan();
    void controllerReadsOnceAndSendsChangesAsYou();
    void controllerSendsPhaseDecisionsAndFinish();
    void controllerSendsHoldReleaseRejectAndReply();
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
    QCOMPARE(team.memberOfSession("msi-old", "kimi/checkout/tester")->id, QStringLiteral("tester"));
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

void TestTeams::modelReadsPhasesPinnedAndTheWaitingState()
{
    const auto team = parse(phasedTeam());
    QVERIFY(team.phased());
    QCOMPARE(team.phases.size(), 1);
    const auto *phase = team.phase(2);
    QVERIFY(phase);
    QCOMPARE(phase->title, QStringLiteral("QA fixes"));
    QVERIFY(phase->barrier);
    QCOMPARE(phase->approval, QStringLiteral("auto"));
    QVERIFY(phase->draft());
    QCOMPARE(phase->tasks, (QList<int>{11, 12}));
    QCOMPARE(phase->addedBy, QStringLiteral("Lead"));
    QCOMPARE(phase->addedAt, 1792868401.5);
    QVERIFY(!team.phase(1));
    QCOMPARE(team.pendingPhase, 2);
    QVERIFY(!team.waitingForLead);
    QCOMPARE(team.idle, QStringLiteral("pending"));
    QCOMPARE(team.task(11)->phase, 2);
    QVERIFY(team.task(11)->pinned);
    QVERIFY(!team.task(12)->pinned);
    QCOMPARE(team.phaseApproval, QStringLiteral("auto"));
    QCOMPARE(team.effectivePhaseApproval(), QStringLiteral("auto"));
    // Without phase_approval the plan's approval applies.
    auto review = phasedTeam();
    review["settings"] = QJsonObject{{"approval", "review"}};
    QCOMPARE(parse(review).effectivePhaseApproval(), QStringLiteral("review"));
    // A draft record implies pending_phase when hgs did not send it.
    auto older = phasedTeam(); older.remove("pending_phase");
    QCOMPARE(parse(older).pendingPhase, 2);
    // All decided without the derived flag: a running team waits for the lead.
    auto decided = checkout();
    auto tasks = decided["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    decided["tasks"] = tasks;
    const auto waiting = parse(decided);
    QVERIFY(waiting.waitingForLead);
    QVERIFY(!waiting.phased());
    QCOMPARE(waiting.pendingPhase, 0);
    decided["status"] = "paused";
    QVERIFY(!parse(decided).waitingForLead);
    // A finished team keeps who finished it and its summary.
    auto done = phasedTeam();
    done["status"] = "done"; done.remove("pending_phase");
    done["phases"] = QJsonArray{QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"},
                                            {"tasks", QJsonArray{11, 12}}, {"decided_by", "You"}}};
    done["finish"] = QJsonObject{{"by", "Lead"}, {"summary", "All phases are in."}, {"at", 1792868500.5}};
    const auto finished = parse(done);
    QVERIFY(finished.hasFinish);
    QCOMPARE(finished.finish.by, QStringLiteral("Lead"));
    QCOMPARE(finished.finish.summary, QStringLiteral("All phases are in."));
    QCOMPARE(finished.finish.at, 1792868500.5);
    QCOMPARE(finished.pendingPhase, 0);
    // Old files keep their meaning: phase 1, nothing pinned, no phases or notice.
    const auto old = parse(checkout());
    QVERIFY(!old.phased());
    QCOMPARE(old.task(1)->phase, 1);
    QVERIFY(!old.task(1)->pinned);
    QVERIFY(old.phaseApproval.isEmpty());
    QVERIFY(old.idle.isEmpty());
    QVERIFY(!old.hasFinish);
    QVERIFY(!old.waitingForLead);   // open tasks
    // The catalog template carries the new-phase approval.
    const auto catalog = parseTemplate(QJsonObject{{"id", "dev"}, {"name", "Dev"}, {"approval", "review"}, {"phase_approval", "auto"}});
    QCOMPARE(catalog.phaseApproval, QStringLiteral("auto"));
    QCOMPARE(catalog.json().value("phase_approval").toString(), QStringLiteral("auto"));
    QVERIFY(parseTemplate(QJsonObject{{"id", "dev"}}).phaseApproval.isEmpty());
}

void TestTeams::leftRunningMembersOfAFinishedTeam()
{
    auto done = phasedTeam();
    done["status"] = "done";
    done["left_running"] = QJsonArray{
        QJsonObject{{"member", "backend"}, {"role", "Backend"},
                    {"session", QJsonObject{{"host", ""}, {"name", "codex/checkout/backend"}, {"run_id", "r1"}, {"conversation_id", "c1"}}}},
        QJsonObject{{"member", "tester"}, {"role", "Tester"},
                    {"session", QJsonObject{{"host", "msi-old"}, {"name", "kimi/checkout/tester"}}}}};
    const auto team = parse(done);
    QVERIFY(team.leftRunningKnown);
    QCOMPARE(team.leftRunning.size(), 2);
    const auto &left = team.leftRunning.first();
    QCOMPARE(left.member, QStringLiteral("backend"));
    QCOMPARE(left.role, QStringLiteral("Backend"));
    QCOMPARE(left.host, QString());
    QCOMPARE(left.session, QStringLiteral("codex/checkout/backend"));
    QCOMPARE(left.runId, QStringLiteral("r1"));
    QCOMPARE(left.conversationId, QStringLiteral("c1"));
    QCOMPARE(team.leftRunning.last().host, QStringLiteral("msi-old"));
    QVERIFY(leftMember(team, "backend"));
    QVERIFY(leftMember(team, "tester"));
    QVERIFY(!leftMember(team, "lead"));
    QVERIFY(team.leftOf("backend"));
    QVERIFY(!team.leftOf("lead"));
    // A team list summary carries the same field.
    auto listed = done;
    listed.remove("layers"); listed.remove("ready");
    QCOMPARE(parse(listed).leftRunning.size(), 2);
    // A running team and an old file have none.
    QVERIFY(!parse(checkout()).leftRunningKnown);
    QVERIFY(parse(checkout()).leftRunning.isEmpty());
    // F8: an adopted member's session keeps its identity in the plan input.
    Member adopted;
    adopted.id = "adopted"; adopted.role = "Backend"; adopted.agent = "codex";
    adopted.host = QString(); adopted.session = QStringLiteral("codex/checkout/backend");
    adopted.runId = QStringLiteral("r1"); adopted.conversationId = QStringLiteral("c1");
    const auto session = adopted.json().value("session").toObject();
    QCOMPARE(session.value("name").toString(), QStringLiteral("codex/checkout/backend"));
    QCOMPARE(session.value("run_id").toString(), QStringLiteral("r1"));
    QCOMPARE(session.value("conversation_id").toString(), QStringLiteral("c1"));
    // A session without the new ids keeps the old shape.
    Member fresh;
    fresh.id = "fresh"; fresh.role = "Tester"; fresh.agent = "kimi"; fresh.session = QStringLiteral("kimi/x/t");
    QCOMPARE(fresh.json().value("session").toObject().keys(), (QStringList{"host", "name"}));
    // A status session carries its ids too.
    auto withIds = checkout();
    auto members = withIds["members"].toArray();
    auto m = members[1].toObject();
    m["session"] = QJsonObject{{"host", ""}, {"name", "codex/checkout/backend"}, {"run_id", "r9"}, {"conversation_id", "c9"}};
    members[1] = m; withIds["members"] = members;
    QCOMPARE(parse(withIds).member("backend")->runId, QStringLiteral("r9"));
    QCOMPARE(parse(withIds).member("backend")->conversationId, QStringLiteral("c9"));
}

// G6: two numbers instead of one over every task — the phase in work (or, when
// none is in work, the next draft waiting for approval) and the phases.
// B1/B2: a permission request carries the exact command, where it came from and
// the agent's reason; the answer keeps the decision, who answered and the deny
// text. B5: a plan that widens a role's profile says so.
// G8: the session row is built from `hgs team list`, whose summary now carries
// each task's phase and the phase records. A summary of a team like 528aebfe -
// phase 1 and 31 finished, 32 in work, 33 and 34 drafts - reads the phase-32
// tasks bar and 2/5 phases.
// C1-C4, C5, C7, C12, G7, F12, F13: the lead-load and services fields the
// desktop reads, from a status shaped like the spec's.
// C4: the permission comes from members[].question as hgs sends it, with the
// answers from members[].permission_answers; N6: the check result comes from
// attempts[].check.
void TestTeams::permissionRequestFromTheMemberAndItsAnswer()
{
    auto team = phasedTeam();
    team["status"] = "running";
    auto members = team["members"].toArray();
    auto backend = members[1].toObject();
    backend["question"] = QJsonObject{{"question_id", "toolu_1"}, {"tool", "Bash"},
        {"command", "npm run migrate --force"}, {"command_source", "tool_input"},
        {"reason", "the migration is not reversible"}, {"asked_at", 1792868400.0},
        {"question_hash", "h1"}};
    backend["permission_answers"] = QJsonArray{
        QJsonObject{{"question_id", "q0"}, {"decision", "deny"}, {"by", "lead"}, {"at", 1792868300.0}, {"reason", "not now"}}};
    members[1] = backend; team["members"] = members;
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 4) continue;
        task["kind"] = "check";
        task["check"] = QJsonObject{{"command", "cargo test --locked --bin hgs teams"}, {"expect_exit", 0}, {"timeout_minutes", 30}};
        // A finished check task: one attempt with the check it ran.
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "backend"}, {"state", "reported"},
            {"check", QJsonObject{{"exit", 0}, {"tail", "120 passed"}, {"at", 1792868400.0}}}}};
        tasks[i] = task;
    }
    team["tasks"] = tasks;
    const auto parsed = parse(team);
    const auto *member = parsed.member("backend");
    QVERIFY(member);
    QVERIFY(member->permission.permission());
    QCOMPARE(member->permission.id, QStringLiteral("toolu_1"));
    QCOMPARE(member->permission.tool, QStringLiteral("Bash"));
    QCOMPARE(member->permission.command, QStringLiteral("npm run migrate --force"));
    QCOMPARE(member->permission.commandSource, QStringLiteral("tool_input"));
    QCOMPARE(member->permission.hash, QStringLiteral("h1"));
    QCOMPARE(member->permissionAnswers.size(), 1);
    QVERIFY(member->permissionAnswers[0].denied());
    QCOMPARE(member->permissionAnswers[0].by, QStringLiteral("lead"));
    QCOMPARE(member->permissionAnswers[0].reason, QStringLiteral("not now"));
    // C4: the card's own answer ids, as status sends them in question.answer.
    auto answered = team;
    auto answeredMembers = answered["members"].toArray();
    auto answeredMember = answeredMembers[1].toObject();
    answeredMember["question"] = QJsonObject{{"question_id", "deepseek-approval:9c1d"}, {"question_hash", "9c1d"},
        {"tool", "Terminal"}, {"command", "make test"}, {"command_source", "tool_input"}, {"reason", "run the suite"},
        {"asked_at", 1792868400.0},
        {"answer", QJsonObject{{"question_id", "decision"}, {"allow", "allow"}, {"deny", "deny"}}}};
    answeredMembers[1] = answeredMember; answered["members"] = answeredMembers;
    const auto *answeredParsed = parse(answered).member("backend");
    QVERIFY(answeredParsed);
    QVERIFY(answeredParsed->permission.native.known);
    QCOMPARE(answeredParsed->permission.native.questionId, QStringLiteral("decision"));
    QCOMPARE(answeredParsed->permission.native.allow, QStringLiteral("allow"));
    QCOMPARE(answeredParsed->permission.native.deny, QStringLiteral("deny"));
    const auto *check = parsed.task(4);
    QVERIFY(check && check->isCheck());
    QCOMPARE(check->check.command, QStringLiteral("cargo test --locked --bin hgs teams"));
    QVERIFY(!check->attempts.isEmpty());
    QVERIFY(check->attempts[0].checkResult.known);
    QCOMPARE(check->attempts[0].checkResult.exit, 0);
    QCOMPARE(check->attempts[0].checkResult.tail, QStringLiteral("120 passed"));
}

void TestTeams::leadLoadViewsCarryTheirNumbers()
{
    auto team = phasedTeam();
    team["status"] = "running";
    team["approved_by"] = "Lead";
    team["owner_word"] = "план принят, делай";
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() == 2) {
            task["waits"] = QJsonObject{{"for", "lead"}, {"on", QJsonArray{"Architect"}}, {"unblock", "accept #2"}};
        }
        if (task["id"].toInt() == 4) {
            task["kind"] = "check";
            task["check"] = QJsonObject{{"command", "cargo test --locked --bin hgs teams"}, {"expect_exit", 0}, {"timeout_minutes", 30}};
            task["check_result"] = QJsonObject{{"exit", 0}, {"tail", "120 passed"}};
        }
        tasks[i] = task;
    }
    team["tasks"] = tasks;
    auto members = team["members"].toArray();
    auto backend = members[1].toObject();
    backend["services"] = QJsonArray{QJsonObject{{"name", "qdrant"}, {"command", "qdrant --mcp"}, {"since", 1792868400.0}}};
    backend["budget"] = QJsonObject{{"label", "socraticode"}, {"minutes", 20}, {"red_minutes", 40}, {"since", 1792868400.0},
                                    {"elapsed_minutes", 47}, {"state", "red"}, {"command", "cargo build --release"}};
    members[1] = backend; team["members"] = members;
    team["attention"] = QJsonArray{QJsonObject{{"kind", "report"}, {"task", 2}, {"member", "Frontend"}, {"since", 1792868400.0}, {"minutes", 70}}};
    auto phases = team["phases"].toArray();
    auto draft = phases[0].toObject();
    draft["waiting"] = QJsonObject{{"text", "Can run alongside phase 1"}, {"phase", 1}, {"open", QJsonArray{3, 4}},
                                   {"alongside", true}};
    phases[0] = draft; team["phases"] = phases;
    QJsonObject perMember{{"Backend", QJsonObject{{"tasks", 3}, {"on_critical_path", 2}}}};
    QJsonObject starvedEntry{{"member", "tester"}, {"role", "Tester"}, {"minutes", 25}, {"blocking", QJsonArray{4}}};
    QJsonObject health{{"tasks", 7}, {"critical_path", 3}, {"parallelism", 2.3}, {"widest_ready", 4}};
    health["per_member"] = perMember;
    health["fan_in"] = QJsonArray{QJsonObject{{"task", 5}, {"waits_for", QJsonArray{3, 4}}, {"kind", "review"}}};
    health["warnings"] = QJsonArray{"Parallelism is below 1.5"};
    health["starved"] = QJsonArray{starvedEntry};
    team["validation"] = QJsonObject{{"errors", QJsonArray()}, {"warnings", QJsonArray()}, {"health", health}};
    const auto parsed = parse(team);
    QCOMPARE(parsed.approvedBy, QStringLiteral("Lead"));
    QCOMPARE(parsed.ownerWord, QStringLiteral("план принят, делай"));
    QVERIFY(!parsed.attention.isEmpty());
    QCOMPARE(parsed.attention[0].kind, QStringLiteral("report"));
    QCOMPARE(parsed.attention[0].minutes, 70);
    const auto *checkTask = parsed.task(4);
    QVERIFY(checkTask && checkTask->isCheck());
    QCOMPARE(checkTask->check.command, QStringLiteral("cargo test --locked --bin hgs teams"));
    QCOMPARE(checkTask->check.expectExit, 0);
    QCOMPARE(checkTask->check.timeoutMinutes, 30);
    QVERIFY(checkTask->checkResult.known);
    QCOMPARE(checkTask->checkResult.exit, 0);
    QCOMPARE(checkTask->checkResult.tail, QStringLiteral("120 passed"));
    const auto *waiting = parsed.task(2);
    QVERIFY(waiting && waiting->wait.known);
    QCOMPARE(waiting->wait.for_, QStringLiteral("lead"));
    QCOMPARE(waiting->wait.unblock, QStringLiteral("accept #2"));
    QCOMPARE(waiting->wait.on, QStringList{QStringLiteral("Architect")});
    const auto *backendMember = parsed.member("backend");
    QVERIFY(backendMember);
    QCOMPARE(backendMember->services.size(), 1);
    QCOMPARE(backendMember->services[0].name, QStringLiteral("qdrant"));
    QVERIFY(backendMember->budget.known);
    QCOMPARE(backendMember->budget.state, QStringLiteral("red"));
    QCOMPARE(backendMember->budget.elapsedMinutes, 47);
    QCOMPARE(backendMember->budget.command, QStringLiteral("cargo build --release"));
    const auto *draftPhase = parsed.phase(2);
    QVERIFY(draftPhase && !draftPhase->waiting.text.isEmpty());
    QCOMPARE(draftPhase->waiting.phase, 1);
    QCOMPARE(draftPhase->waiting.open, (QList<int>{3, 4}));
    QVERIFY(parsed.health.known);
    QCOMPARE(parsed.health.tasks, 7);
    QCOMPARE(parsed.health.criticalPath, 3);
    QCOMPARE(parsed.health.widestReady, 4);
    QCOMPARE(parsed.health.perMember.size(), 1);
    QCOMPARE(parsed.health.perMember[0].second.second, 2);
    QCOMPARE(parsed.health.fanIn.size(), 1);
    QCOMPARE(parsed.health.fanIn[0].task, 5);
    QCOMPARE(parsed.health.warnings, QStringList{QStringLiteral("Parallelism is below 1.5")});
    QCOMPARE(parsed.health.starved.size(), 1);
    QCOMPARE(parsed.health.starved[0].minutes, 25);
    // C2: the inbox list, oldest first as hgs sends it.
    const auto inbox = parseInbox(QJsonObject{{"items", QJsonArray{
        QJsonObject{{"kind", "question"}, {"task", 4}, {"member", "Tester"}, {"since", 1792868401.0},
                    {"summary", "Which field is the key?"}, {"command", "{} | hgs team reply --json"}},
        QJsonObject{{"kind", "report"}, {"task", 2}, {"member", "Frontend"}, {"since", 1792868402.0},
                    {"summary", "Form ready"}, {"command", "{} | hgs team accept --json"}}}}});
    QCOMPARE(inbox.size(), 2);
    QCOMPARE(inbox[0].kind, QStringLiteral("question"));
    QCOMPARE(inbox[0].summary, QStringLiteral("Which field is the key?"));
    QCOMPARE(inbox[0].task, 4);
    QVERIFY(inbox[0].command.contains("reply"));
    QCOMPARE(inbox[1].kind, QStringLiteral("report"));
    QVERIFY(inbox[1].since > inbox[0].since);
}

void TestTeams::listSummaryCarriesTaskPhasesAndPhaseRecords()
{
    QJsonObject summary{{"id", "team-1"}, {"name", "Teams v2"}, {"status", "running"}, {"lead", "lead"},
                        {"pending_phase", 33}, {"revision", 40}};
    QJsonObject phase1{{"n", 1}, {"title", ""}, {"state", "approved"}, {"tasks", QJsonArray()}};
    QJsonArray tasks;
    const auto addTask = [&](int id, int phase, const QString &status) {
        tasks.append(QJsonObject{{"id", id}, {"phase", phase}, {"title", QStringLiteral("Task %1").arg(id)},
                                 {"status", status}, {"assignee", "backend"}});
    };
    QJsonArray phase1Tasks, phase31, phase32, phase33, phase34;
    for (int id = 1; id <= 30; ++id) { addTask(id, 1, "accepted"); phase1Tasks.append(id); }
    for (int id = 271; id <= 286; ++id) { addTask(id, 31, "accepted"); phase31.append(id); }
    for (int id = 287; id <= 297; ++id) { addTask(id, 32, id == 292 ? "in_progress" : "accepted"); phase32.append(id); }
    for (int id = 298; id <= 308; ++id) { addTask(id, 33, "draft"); phase33.append(id); }
    for (int id = 309; id <= 319; ++id) { addTask(id, 34, "draft"); phase34.append(id); }
    phase1["tasks"] = phase1Tasks;
    summary["tasks"] = tasks;
    summary["phases"] = QJsonArray{phase1,
        QJsonObject{{"n", 31}, {"title", "Phase 1 fixes and urgent"}, {"state", "approved"}, {"tasks", phase31}},
        QJsonObject{{"n", 32}, {"title", "Permissions and lead load"}, {"state", "approved"}, {"tasks", phase32}},
        QJsonObject{{"n", 33}, {"title", "Team creation"}, {"state", "draft"}, {"tasks", phase33}},
        QJsonObject{{"n", 34}, {"title", "Reports and findings"}, {"state", "draft"}, {"tasks", phase34}}};
    const auto teams = parseList(QJsonObject{{"items", QJsonArray{summary}}});
    QCOMPARE(teams.size(), 1);
    const auto &team = teams.first();
    QVERIFY(team.phased());
    QCOMPARE(team.tasks.size(), 79);
    const auto progress = phaseProgress(team);
    QCOMPARE(progress.phase, 32);       // the phase in work
    QCOMPARE(progress.draft, false);
    QCOMPARE(progress.phaseText(), QStringLiteral("10/11"));
    QCOMPARE(progress.phasesDone, 2);   // phase 1 and phase 31
    QCOMPARE(progress.phasesTotal, 5);  // phase 1, 31, 32, 33 and 34
    QCOMPARE(progress.phasesText(), QStringLiteral("2/5"));
    QCOMPARE(progress.tooltip().section('\n', 0, 0), QStringLiteral("Phase 32 Permissions and lead load: 10/11"));
}

void TestTeams::permissionRequestsCarryTheCommandAndTheAnswer()
{
    auto team = deliveryTeam();
    auto tasks = team["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        if (task["id"].toInt() != 4) continue;
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a4"}, {"member", "tester"}, {"state", "asking"},
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
    const auto parsed = parse(team);
    const auto *task = parsed.task(4);
    QVERIFY(task);
    QVERIFY(!task->attempts.isEmpty());
    const auto &attempt = task->attempts.last();
    QCOMPARE(attempt.questions.size(), 2);
    QVERIFY(attempt.questions[0].permission());
    QCOMPARE(attempt.questions[0].tool, QStringLiteral("Bash"));
    QCOMPARE(attempt.questions[0].command, QStringLiteral("npm run migrate --force"));
    QCOMPARE(attempt.questions[0].commandSource, QStringLiteral("tool_input"));
    QCOMPARE(attempt.questions[0].reason, QStringLiteral("The migration is not reversible"));
    QVERIFY(!attempt.questions[0].answered());
    QCOMPARE(attempt.questions[1].commandSource, QStringLiteral("unknown"));
    QCOMPARE(attempt.permissions.size(), 2);
    QVERIFY(attempt.permissions[0].denied());
    QCOMPARE(attempt.permissions[0].by, QStringLiteral("owner"));
    QCOMPARE(attempt.permissions[0].reason, QStringLiteral("That kills every ninja on the machine"));
    QCOMPARE(attempt.permissions[1].decision, QStringLiteral("allow"));
    // B5: the widening names the member the plan widens and how far.
    auto widened = deliveryTeam();
    auto members = widened["members"].toArray();
    auto tester = members[2].toObject();
    tester["profile_widening"] = QJsonObject{{"member", "tester"}, {"from", "read"}, {"to", "build"}};
    members[2] = tester; widened["members"] = members;
    const auto widening = parse(widened);
    QCOMPARE(widening.widenings.size(), 1);
    QCOMPARE(widening.widenings[0].member, QStringLiteral("tester"));
    QCOMPARE(widening.widenings[0].from, QStringLiteral("read"));
    QCOMPARE(widening.widenings[0].to, QStringLiteral("build"));
    // The catalog role carries the profile the plan may widen.
    QJsonObject role{{"id", "reviewer"}, {"name", "Reviewer"}, {"permissions_profile", "commit"}};
    const auto catalogRole = parseRole(role);
    QCOMPARE(catalogRole.permissionsProfile, QStringLiteral("commit"));
}

void TestTeams::phaseProgressShowsThePhaseInWorkAndThePhases()
{
    // phasedTeam: phase 1 holds the ten checkout tasks (one accepted), phase 2
    // is a draft of two tasks.
    const auto parsed = parse(phasedTeam());
    auto progress = phaseProgress(parsed);
    QCOMPARE(progress.phase, 1);          // phase 1 is in work
    QCOMPARE(progress.draft, false);
    QCOMPARE(progress.total, 10);
    QCOMPARE(progress.done, 1);
    QCOMPARE(progress.phaseText(), QStringLiteral("1/10"));
    QCOMPARE(progress.phasesTotal, 2);    // phase 1 and the draft
    QCOMPARE(progress.phasesDone, 0);
    QCOMPARE(progress.phasesText(), QStringLiteral("0/2"));
    QVERIFY(progress.tooltip().contains(QStringLiteral("Phase 1")));
    QVERIFY(progress.tooltip().contains(QStringLiteral("0 of 2 phases")));
    // Phase 1 decided: nothing is in work, so the next draft waits for approval.
    auto decided = phasedTeam();
    auto decidedTasks = decided["tasks"].toArray();
    for (int i = 0; i < decidedTasks.size(); ++i) {
        auto task = decidedTasks[i].toObject();
        if (task["phase"].toInt(1) == 1) task["status"] = "accepted";
        decidedTasks[i] = task;
    }
    decided["tasks"] = decidedTasks;
    const auto waiting = phaseProgress(parse(decided));
    QCOMPARE(waiting.phase, 2);
    QVERIFY(waiting.draft);
    QCOMPARE(waiting.phaseText(), QStringLiteral("0/2"));
    QVERIFY(waiting.tooltip().contains(QStringLiteral("waiting for your approval")));
    QCOMPARE(waiting.phasesDone, 1);
    QCOMPARE(waiting.phasesTotal, 2);
    // A discarded draft is not planned: it never counts, and it is never the
    // phase the bar follows.
    auto withDiscard = decided;
    withDiscard["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "Discarded"}, {"state", "discarded"}, {"tasks", QJsonArray{11, 12}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}}}};
    auto discardTasks = withDiscard["tasks"].toArray();
    auto polish = QJsonObject{{"id", 13}, {"phase", 3}, {"title", "Polish the card"}, {"assignee", "backend"}, {"status", "draft"}};
    discardTasks.append(polish);
    for (int i = 0; i < discardTasks.size(); ++i) {
        auto task = discardTasks[i].toObject();
        if (task["phase"].toInt() == 2) task["status"] = "cancelled";
        discardTasks[i] = task;
    }
    withDiscard["tasks"] = discardTasks;
    const auto afterDiscard = phaseProgress(parse(withDiscard));
    QCOMPARE(afterDiscard.phase, 3);          // the only draft left
    QVERIFY(afterDiscard.draft);
    QCOMPARE(afterDiscard.phasesTotal, 2);    // phase 1 and phase 3
    QCOMPARE(afterDiscard.phasesDone, 1);
    QCOMPARE(afterDiscard.phasesText(), QStringLiteral("1/2"));
    // Everything decided and no draft: the last phase stands for the finished plan.
    auto all = withDiscard;
    auto allTasks = all["tasks"].toArray();
    for (int i = 0; i < allTasks.size(); ++i) {
        auto task = allTasks[i].toObject();
        if (task["phase"].toInt() == 3) task["status"] = "accepted";
        allTasks[i] = task;
    }
    all["tasks"] = allTasks;
    const auto allDone = phaseProgress(parse(all));
    QCOMPARE(allDone.phase, 3);
    QCOMPARE(allDone.phaseText(), QStringLiteral("1/1"));
    QCOMPARE(allDone.phasesTotal, 2);
    QCOMPARE(allDone.phasesDone, 2);
    // A team without phase records still gets the two bars for its plan.
    const auto plainTeam = parse(deliveryTeam());
    const auto plain = phaseProgress(plainTeam);
    int plainDone = 0; for (const auto &task : plainTeam.tasks) plainDone += task.finished();
    QCOMPARE(plain.phase, 1);
    QCOMPARE(plain.total, 6);
    QCOMPARE(plain.done, plainDone);
    QCOMPARE(plain.phasesTotal, 1);
}

void TestTeams::draftPhasesApproveInOrder()
{
    auto roadmap = phasedTeam();
    roadmap["pending_phase"] = QJsonValue();
    roadmap["pending_phases"] = QJsonArray{2, 3, 4};
    roadmap["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "draft"}, {"tasks", QJsonArray{11, 12}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{14}}}};
    auto tasks = roadmap["tasks"].toArray();
    // Phase 1 is decided; the drafts carry draft tasks.
    for (int i = 0; i < tasks.size(); ++i) { auto task = tasks[i].toObject(); task["status"] = "accepted"; tasks[i] = task; }
    for (int id = 11; id <= 14; ++id)
        tasks.append(QJsonObject{{"id", id}, {"phase", id == 11 || id == 12 ? 2 : id == 13 ? 3 : 4},
                                 {"title", QStringLiteral("Draft %1").arg(id)}, {"assignee", "backend"}, {"status", "draft"}});
    roadmap["tasks"] = tasks;
    const auto team = parse(roadmap);
    QCOMPARE(team.draftPhases(), (QList<int>{2, 3, 4}));
    QCOMPARE(team.pendingPhases, (QList<int>{2, 3, 4}));
    QCOMPARE(team.pendingPhase, 2);
    // Only the next phase may be approved, and only once every lower one ended.
    QCOMPARE(approveBlocked(team, 2), QString());
    QVERIFY(approveBlocked(team, 3).contains(QStringLiteral("Phase 3 opens after phase 2 is approved")));
    // QA: the nearest open phase is named, not the first draft of the plan.
    QVERIFY(approveBlocked(team, 4).contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    // F10: the lead's diff travels with the draft, and a move renumbers the
    // drafts; the feed says where the phase went.
    auto withDiff = roadmap;
    withDiff["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "draft"}, {"tasks", QJsonArray{11, 12}},
                    {"changes", QJsonArray{QJsonObject{{"kind", "added"}, {"task", 15}, {"title", "New check"}},
                                           QJsonObject{{"kind", "removed"}, {"task", 9}, {"title", "Old check"}},
                                           QJsonObject{{"kind", "changed"}, {"task", 11}, {"title", "Draft 11"}, {"from", "Fix it"}, {"to", "Fix it now"}},
                                           QJsonObject{{"kind", "moved"}, {"from", 3}, {"to", 2}}}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{14}}}};
    const auto diffed = parse(withDiff);
    const auto *secondPhase = diffed.phase(2);
    QVERIFY(secondPhase);
    QCOMPARE(secondPhase->changes.size(), 4);
    QVERIFY(secondPhase->changes[0].added());
    QCOMPARE(secondPhase->changes[0].task, 15);
    QCOMPARE(secondPhase->changes[0].title, QStringLiteral("New check"));
    QVERIFY(secondPhase->changes[1].removed());
    QCOMPARE(secondPhase->changes[2].kind, QStringLiteral("changed"));
    QCOMPARE(secondPhase->changes[2].from, QStringLiteral("Fix it"));
    QCOMPARE(secondPhase->changes[2].to, QStringLiteral("Fix it now"));
    QCOMPARE(secondPhase->changes[3].kind, QStringLiteral("moved"));
    QCOMPARE(secondPhase->changes[3].from, QStringLiteral("3"));
    QCOMPARE(secondPhase->changes[3].to, QStringLiteral("2"));
    QJsonObject moved{{"kind", "phase_moved"}, {"seq", 9}, {"at", 1700.0},
                      {"detail", QJsonObject{{"from", 3}, {"to", 2}, {"renumbered", true}}}};
    const auto line = feedLine(diffed, moved);
    QVERIFY2(line.text.contains(QStringLiteral("Moved phase 3 to position 2")), qPrintable(line.text));
    QVERIFY(line.text.contains(QStringLiteral("renumbered")));
    // Approved but not finished yet (F12/N40): the next draft is approvable to
    // run alongside it, and no longer blocked by its open tasks.
    auto running = roadmap;
    running["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{11, 12}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{14}}}};
    const auto second = parse(running);
    QCOMPARE(approveBlocked(second, 3), QString());
    QCOMPARE(alongsidePhase(second, 3), 2);
    // The server's own text says so, and the flag travels with it.
    auto told = running;
    told["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "approved"}, {"tasks", QJsonArray{11, 12}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}},
                    {"waiting", QJsonObject{{"text", "Can run alongside phase 2"}, {"phase", 2},
                                            {"open", QJsonArray{11, 12}}, {"alongside", true}}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{14}}}};
    const auto along = parse(told);
    QVERIFY(along.phase(3)->waiting.alongside);
    QCOMPARE(along.phase(3)->waiting.text, QStringLiteral("Can run alongside phase 2"));
    QCOMPARE(alongsidePhase(along, 3), 2);
    // A draft behind a draft still waits for the approval.
    QVERIFY(approveBlocked(along, 4).contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    QCOMPARE(alongsidePhase(along, 4), 0);
    // Phase 2's tasks accepted: phase 3 opens, phase 4 still waits for phase 3.
    auto finished = running;
    auto finishedTasks = finished["tasks"].toArray();
    for (int i = 0; i < finishedTasks.size(); ++i) {
        auto task = finishedTasks[i].toObject();
        if (task["id"].toInt() == 11 || task["id"].toInt() == 12) task["status"] = "accepted";
        finishedTasks[i] = task;
    }
    finished["tasks"] = finishedTasks;
    const auto third = parse(finished);
    QCOMPARE(approveBlocked(third, 3), QString());
    QVERIFY(approveBlocked(third, 4).contains(QStringLiteral("Phase 4 opens after phase 3 is approved")));
    // A discarded lower phase never blocks the road ahead.
    auto discarded = finished;
    discarded["phases"] = QJsonArray{
        QJsonObject{{"n", 2}, {"title", "QA fixes"}, {"state", "discarded"}, {"tasks", QJsonArray{11, 12}}},
        QJsonObject{{"n", 3}, {"title", "Polish"}, {"state", "draft"}, {"tasks", QJsonArray{13}}},
        QJsonObject{{"n", 4}, {"title", "Docs"}, {"state", "draft"}, {"tasks", QJsonArray{14}}}};
    QCOMPARE(approveBlocked(parse(discarded), 3), QString());
    // A phase that is not a draft cannot be approved.
    QVERIFY(approveBlocked(third, 2).contains(QStringLiteral("not a draft")));
    QVERIFY(approveBlocked(third, 9).contains(QStringLiteral("not a draft")));
    // Old single-draft files keep their meaning (this fixture has phase 2 as
    // its one draft).
    const auto single = parse(phasedTeam());
    QCOMPARE(single.draftPhases(), (QList<int>{2}));
    QCOMPARE(single.pendingPhase, 2);
    QVERIFY(!single.leftRunningKnown);
}

void TestTeams::modelReadsAttemptStatesQuestionsHoldsAndReports()
{
    const auto team = parse(deliveryTeam());
    // A1: delivery and start are kept apart from the attempt's own state.
    const auto *working = team.task(1)->last();
    QVERIFY(working);
    QCOMPARE(working->n, 2);
    QCOMPARE(working->deliveredAt, 1301.0);
    QCOMPARE(working->startedAt, 1305.0);
    QCOMPARE(working->phase, QStringLiteral("working"));   // hgs derived it
    QVERIFY(!working->startWarned);
    QCOMPARE(lastPhase(*team.task(1)), QStringLiteral("working"));
    // A2/A6: the report that replaced the captured reply keeps the old text.
    const auto *replaced = team.task(2)->last();
    QCOMPARE(replaced->report, QStringLiteral("The real report"));
    QCOMPARE(replaced->reportAt, 1200.0);
    QVERIFY(!replaced->captured);
    QCOMPARE(replaced->replaced.size(), 1);
    QCOMPARE(replaced->replaced.first().text, QStringLiteral("Building and running the tests..."));
    QCOMPARE(replaced->replaced.first().at, 1150.0);
    QCOMPARE(lastPhase(*team.task(2)), QStringLiteral("reported"));
    // A5: a held task and its hold.
    const auto *held = team.task(3);
    QVERIFY(held->held);
    QCOMPARE(held->hold.by, QStringLiteral("Lead"));
    QCOMPARE(held->hold.reason, QStringLiteral("waiting for the owner"));
    QCOMPARE(held->hold.at, 1400.0);
    QCOMPARE(held->status, QStringLiteral("pending"));
    QCOMPARE(lastPhase(*held), QStringLiteral("held"));
    QCOMPARE(taskStatus(*held).text, QStringLiteral("Held"));
    QCOMPARE(taskStatus(*held).tone, Tone::Muted);
    QVERIFY(!ready(team, *held));
    auto released = parse(deliveryTeam());
    for (auto &task : released.tasks) if (task.id == 3) { task.held = false; task.hold = {}; }
    QVERIFY(ready(released, *released.task(3)));   // it waits for the release, not for a dependency
    // A3: open questions stay apart from answered ones.
    const auto *asking = team.task(4)->last();
    QCOMPARE(asking->questions.size(), 2);
    QCOMPARE(asking->openQuestion()->id, QStringLiteral("q1"));
    QCOMPARE(openQuestions(*team.task(4)).size(), 1);
    QCOMPARE(openQuestions(*team.task(4)).first().text, QStringLiteral("Must I keep the old JSON field?"));
    const auto *answered = question(*team.task(4), "q2");
    QVERIFY(answered && answered->answered());
    QCOMPARE(answered->answer, QStringLiteral("main"));
    QCOMPARE(answered->answeredAt, 1502.0);
    QCOMPARE(answered->by, QStringLiteral("Lead"));
    QVERIFY(!question(*team.task(4), "q9"));
    QCOMPARE(lastPhase(*team.task(4)), QStringLiteral("asking"));
    QVERIFY(openQuestions(*team.task(1)).isEmpty());
    // A question closed by the store (answered_at set, the answer left empty as
    // when an attempt ends) counts as closed: no open row, no "asking".
    auto withClosed = deliveryTeam();
    auto closedTasks = withClosed["tasks"].toArray();
    for (int i = 0; i < closedTasks.size(); ++i) {
        auto task = closedTasks[i].toObject();
        if (task["id"].toInt() != 4) continue;
        auto attempts = task["attempts"].toArray();
        auto attempt = attempts[0].toObject();
        auto questions = attempt["questions"].toArray();
        questions.append(QJsonObject{{"id", "q9"}, {"text", "Closed when the attempt ended"}, {"at", 1600},
                                     {"answer", ""}, {"answered_at", 1610}, {"by", "Zerus"}});
        attempt["questions"] = questions; attempts[0] = attempt;
        task["attempts"] = attempts; closedTasks[i] = task;
    }
    withClosed["tasks"] = closedTasks;
    const auto closedTeam = parse(withClosed);
    QCOMPARE(openQuestions(*closedTeam.task(4)).size(), 1);   // only q1 stays open
    const auto *closed = question(*closedTeam.task(4), "q9");
    QVERIFY(closed && closed->answered());
    QVERIFY(closed->answer.isEmpty());
    QCOMPARE(closed->answeredAt, 1610.0);
    QCOMPARE(closed->by, QStringLiteral("Zerus"));
    // The member card says it waits for the lead, and that counts as an answer.
    const auto quiet = [](const Member &) { Live state; state.known = true; return state; };
    const auto waiting = memberStatus(team, *team.member("tester"), quiet(*team.member("tester")));
    QCOMPARE(waiting.kind, QStringLiteral("asking"));
    QCOMPARE(waiting.task, 4);
    QVERIFY(waiting.problem);
    QCOMPARE(memberStatus(team, *team.member("backend"), quiet(*team.member("backend"))).kind, QStringLiteral("working"));
    QCOMPARE(answers(team, quiet), 1);
    // A5: the queue is the open task, then the tasks returned to the member.
    QCOMPARE(team.member("backend")->queue, (QList<int>{1, 3}));
    QCOMPARE(team.member("tester")->queue, (QList<int>{4}));
    QCOMPARE(team.member("reviewer")->queue, (QList<int>{5}));
    QVERIFY(team.member("lead")->queue.isEmpty());
    QCOMPARE(memberQueue(team, "backend"), (QList<int>{1, 3}));
    // A queue in the status wins over the derived one, even when empty.
    auto given = deliveryTeam();
    auto members = given["members"].toArray();
    auto tester = members[2].toObject(); tester["queue"] = QJsonArray{}; members[2] = tester;
    given["members"] = members;
    const auto explicitQueue = parse(given);
    QVERIFY(explicitQueue.member("tester")->queueKnown);
    QVERIFY(explicitQueue.member("tester")->queue.isEmpty());
    QCOMPARE(explicitQueue.member("backend")->queue, (QList<int>{1, 3}));
    // L7: the fallback lists every open task hgs would send, in hgs order, not
    // only the first one.
    auto twoOpen = deliveryTeam();
    auto someTasks = twoOpen["tasks"].toArray();
    auto fourth = someTasks[3].toObject();
    fourth["assignee"] = QStringLiteral("backend"); fourth["status"] = QStringLiteral("in_progress");
    fourth["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "o4"}, {"member", "backend"}, {"state", "delivered"}}};
    someTasks[3] = fourth; twoOpen["tasks"] = someTasks;
    // Open tasks ascending, then the returned one, as hgs sends them.
    QCOMPARE(memberQueue(parse(twoOpen), "backend"), (QList<int>{1, 4, 3}));
    // A start warning is the "late" phase.
    QCOMPARE(lastPhase(*team.task(5)), QStringLiteral("late"));
}

void TestTeams::attemptPhasesAreDerivedForAnOlderHgs()
{
    Task task;
    Attempt attempt;
    attempt.state = "delivering";
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("sent"));
    attempt.state = "delivered";
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("delivered"));
    attempt.startWarned = true;
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("late"));
    attempt.startedAt = 1305;
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("working"));
    Question open; open.id = "q1"; open.text = "Keep it?";
    attempt.questions.append(open);
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("asking"));
    attempt.questions.clear();
    attempt.noReport = true;
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("no_report"));
    attempt.noReport = false;
    attempt.state = "unconfirmed";
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("unconfirmed"));
    attempt.state = "rejected";
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("rejected"));
    task.held = true;
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("held"));
    task.held = false;
    attempt.state = "reported";
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("reported"));
    // hgs decides when it sends the phase itself, null included (N4).
    attempt.phaseKnown = true;
    attempt.phase = QStringLiteral("stopped");
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("stopped"));
    attempt.phase.clear();
    QCOMPARE(attemptPhase(task, attempt), QString());   // hgs said: no phase
    // An open question outranks a reported attempt, as in hgs (L7).
    attempt.phaseKnown = false;
    attempt.state = "reported";
    Question asked; asked.id = "q7"; asked.text = "Still true?";
    attempt.questions.append(asked);
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("asking"));
    attempt.questions.clear();
    // No attempts, no phase.
    QCOMPARE(lastPhase(task), QString());
}

void TestTeams::finishedRevokedAndRefusedAttemptsHaveNoPhase()
{
    // hgs sends "phase": null on purpose for a finished task and for a revoked
    // or refused attempt; the desktop then shows the task's own mark and never
    // invents an attempt phase (N4). Only an older hgs with no key at all gets
    // the derived answer.
    auto done = phasedTeam();
    auto tasks = done["tasks"].toArray();
    for (int i = 0; i < tasks.size(); ++i) {
        auto task = tasks[i].toObject();
        task["status"] = "accepted";
        task["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "backend"}, {"state", "reported"},
                                                 {"phase", QJsonValue()}, {"report", QJsonObject{{"text", "Done"}, {"at", 1800}}}}};
        tasks[i] = task;
    }
    done["tasks"] = tasks;
    const auto accepted = parse(done);
    const auto *first = accepted.task(1);
    QVERIFY(first);
    QVERIFY(first->last()->phaseKnown);          // the key was there, as null
    QCOMPARE(lastPhase(*first), QString());
    QCOMPARE(taskStatus(*first).kind, QStringLiteral("accepted"));   // its own mark, not "reported"
    QCOMPARE(taskStatus(*first).text, QStringLiteral("Accepted"));
    // A final rejection keeps the task's own rejected mark.
    auto rejected = parse(done);
    for (auto &task : rejected.tasks) { task.status = "rejected"; task.reason = QStringLiteral("No"); }
    const auto *final = rejected.task(2);
    QVERIFY(final);
    QCOMPARE(lastPhase(*final), QString());
    QCOMPARE(taskStatus(*final).kind, QStringLiteral("rejected"));
    QCOMPARE(taskStatus(*final).text, QStringLiteral("Rejected"));
    // A cancelled task too.
    auto cancelled = parse(done);
    for (auto &task : cancelled.tasks) task.status = "cancelled";
    QCOMPARE(taskStatus(*cancelled.task(1)).kind, QStringLiteral("cancelled"));
    // Revoked and refused attempts of a live task have no phase either, whether
    // hgs sent null or an older hgs sent no key at all.
    for (const char *state : {"revoked", "refused"}) {
        auto live = phasedTeam();
        auto liveTasks = live["tasks"].toArray();
        auto one = liveTasks[0].toObject();
        one["status"] = "pending";
        one["attempts"] = QJsonArray{QJsonObject{{"n", 1}, {"id", "a1"}, {"member", "backend"}, {"state", state}, {"phase", QJsonValue()}}};
        liveTasks[0] = one;
        live["tasks"] = liveTasks;
        const auto task = parse(live);
        QCOMPARE(lastPhase(*task.task(1)), QString());
        QCOMPARE(taskStatus(*task.task(1)).kind, QStringLiteral("pending"));
        auto older = live;
        auto olderTasks = older["tasks"].toArray();
        auto oldOne = olderTasks[0].toObject();
        auto oldAttempts = oldOne["attempts"].toArray();
        auto oldAttempt = oldAttempts[0].toObject(); oldAttempt.remove("phase"); oldAttempts[0] = oldAttempt;
        oldOne["attempts"] = oldAttempts; olderTasks[0] = oldOne; older["tasks"] = olderTasks;
        const auto derived = parse(older);
        QCOMPARE(lastPhase(*derived.task(1)), QString());
    }
}

// F5 (spec 3): a team in status "draft" carries the draft block, its goal,
// the protocol addendum and the settings the form edits; nothing but the lead
// exists yet. A team without a draft block reads as before.
void TestTeams::aDraftTeamReadsItsRevisionUnseenFieldsAndGoal()
{
    auto value = checkout();
    value["status"] = "draft";
    value["goal"] = "Ship the checkout";
    value["lead_protocol"] = "Two waves";
    value["settings"] = QJsonObject{{"folder_policy", "auto"}, {"approval", "review"}, {"max_members", 5}};
    value["draft"] = QJsonObject{{"revision", 7}, {"unseen", QJsonArray{"members", "goal"}}, {"owner_edited", QJsonArray{"settings"}}};
    const auto team = parse(value);
    QVERIFY(team.draft());
    QCOMPARE(team.draftRevision, qint64(7));
    QCOMPARE(team.draftUnseen, (QStringList{"members", "goal"}));
    QCOMPARE(team.goal, QStringLiteral("Ship the checkout"));
    QCOMPARE(team.leadProtocol, QStringLiteral("Two waves"));
    QCOMPARE(team.folderPolicy, QStringLiteral("auto"));
    QCOMPARE(team.maxMembers, 5);
    QCOMPARE(statusLabel("draft"), QStringLiteral("Draft"));
    QCOMPARE(statusTone("draft"), Tone::Dim);
    // The lead drafts with its session; the members are created with the team.
    const auto *lead = team.member(team.lead);
    QVERIFY(lead);
    QCOMPARE(memberStatus(team, *lead, Live{}).text, QStringLiteral("Drafting the team"));
    for (const auto &member : team.members)
        if (member.id != team.lead && member.session.isEmpty()) QCOMPARE(memberStatus(team, member, Live{}).shortText, QStringLiteral("Draft"));
    // An older team file has no draft block.
    const auto old = parse(checkout());
    QVERIFY(!old.draft());
    QCOMPARE(old.draftRevision, qint64(0));
    QVERIFY(old.draftUnseen.isEmpty());
}

// #344 (spec 4, 5, 6.1, 6.3): the lead's proposals, a swap's old session, a
// member at a usage limit, the shared DeepSeek host and the next-goal links;
// an older status without them reads as before.
void TestTeams::proposalsSwapsLimitsAndHostsRead()
{
    auto value = checkout();
    value["previous"] = "team-0"; value["next"] = "team-2";
    value["settings"] = QJsonObject{{"approval", "review"}, {"resume_after_limits", true}};
    auto members = value["members"].toArray();
    auto second = members[1].toObject();
    second["limited"] = QJsonObject{{"at", 100.0}, {"resets_at", 4000.0}, {"detail", "Usage limit reached"}};
    second["limit_resume"] = true;
    second["host_shared"] = true;
    members[1] = second;
    value["members"] = members;
    const QString secondId = second["id"].toString();
    value["hosts"] = QJsonArray{QJsonObject{{"agent", "dsh"}, {"pid", 77}, {"started", 10.0}, {"members", QJsonArray{secondId}}}};
    value["swapped"] = QJsonArray{QJsonObject{{"member", "Backend"}, {"old", "codex/x/old"}, {"at", 50.0}, {"by", "You"}}};
    value["proposals"] = QJsonArray{
        QJsonObject{{"id", "p1"}, {"kind", "swap-member"}, {"by", "Lead"}, {"state", "open"}, {"reason", "limit"},
                    {"member", QJsonObject{{"member", secondId}, {"agent", "codex"}, {"model", "gpt-5.5"}}}},
        QJsonObject{{"id", "p2"}, {"kind", "remove-member"}, {"by", "Lead"}, {"state", "accepted"}, {"member", secondId}},
        QJsonObject{{"id", "p3"}, {"kind", "next-goal"}, {"by", "Lead"}, {"state", "open"}, {"goal", "Saved cards\nand more"}},
        QJsonObject{{"id", "p4"}, {"kind", "add-member"}, {"by", "Lead"}, {"state", "open"},
                    {"member", QJsonObject{{"role", "Designer"}, {"agent", "claude"}}}},
        QJsonObject{{"id", "p5"}, {"kind", "swap-member"}, {"by", "Lead"}, {"state", "open"},
                    {"member", QJsonObject{{"member", secondId}, {"session", QJsonObject{{"host", ""}, {"name", "claude/x/spare"}}}}}}};
    const auto team = parse(value);
    QCOMPARE(team.previous, QStringLiteral("team-0")); QCOMPARE(team.next, QStringLiteral("team-2"));
    QVERIFY(team.resumeAfterLimits);
    QCOMPARE(team.settingsJson.value("approval").toString(), QStringLiteral("review"));
    const auto *member = team.member(secondId);
    QVERIFY(member && member->limited.known && member->limitResume && member->hostShared);
    QCOMPARE(member->limited.resetsAt, 4000.0);
    QCOMPARE(member->limited.detail, QStringLiteral("Usage limit reached"));
    QVERIFY(team.hostOf(secondId) && team.hostOf(secondId)->pid == 77);
    QCOMPARE(team.swapped.size(), 1); QCOMPARE(team.swapped[0].old, QStringLiteral("codex/x/old"));
    QCOMPARE(team.proposals.size(), 5);
    QVERIFY(team.proposals[0].open() && !team.proposals[1].open());
    QCOMPARE(team.proposals[0].memberId(), secondId);
    QCOMPARE(team.proposals[1].memberId(), secondId);
    const QString role = team.roleOf(secondId);
    QCOMPARE(proposalText(team, team.proposals[0]), QStringLiteral("Swap %1 to codex \u00b7 gpt-5.5").arg(role));
    QCOMPARE(proposalText(team, team.proposals[1]), QStringLiteral("Remove %1").arg(role));
    QCOMPARE(proposalText(team, team.proposals[2]), QStringLiteral("Next goal: Saved cards"));
    QCOMPARE(proposalText(team, team.proposals[3]), QStringLiteral("Add Designer (claude)"));
    QCOMPARE(proposalText(team, team.proposals[4]), QStringLiteral("Swap %1 to the session claude/x/spare").arg(role));
    QVERIFY(resetText(0).isEmpty());
    QVERIFY(resetText(4000).startsWith(QStringLiteral("resets ")));
    // An older status has none of it.
    const auto old = parse(checkout());
    QVERIFY(old.proposals.isEmpty() && old.swapped.isEmpty() && old.hosts.isEmpty() && !old.resumeAfterLimits);
    QVERIFY(old.previous.isEmpty() && old.next.isEmpty());
    for (const auto &m : old.members) QVERIFY(!m.limited.known && !m.hostShared);
}

void TestTeams::oldStatusJsonStillParses()
{
    // A team file written before A1-A6: every new field defaults, the old ones
    // keep their meaning.
    const auto team = parse(checkout());
    QCOMPARE(team.members.size(), 6);
    QCOMPARE(team.task(3)->attempts.size(), 2);
    const auto *reported = team.task(2)->last();
    QVERIFY(reported);
    QCOMPARE(reported->report, QStringLiteral("Form ready"));
    QVERIFY(!reported->captured);
    QVERIFY(reported->replaced.isEmpty());
    QVERIFY(reported->questions.isEmpty());
    QVERIFY(reported->phase.isEmpty());
    QCOMPARE(reported->deliveredAt, 0.0);
    QCOMPARE(reported->startedAt, 0.0);
    QCOMPARE(reported->turnEndedAt, 0.0);
    QVERIFY(!reported->startWarned);
    QVERIFY(reported->turnReply.isEmpty());
    QVERIFY(!reported->openQuestion());
    QVERIFY(!team.task(3)->held);
    QVERIFY(team.task(3)->hold.by.isEmpty());
    QCOMPARE(team.task(3)->hold.at, 0.0);
    // The phases an older hgs did not send are the same answer.
    QCOMPARE(lastPhase(*team.task(2)), QStringLiteral("reported"));
    QCOMPARE(lastPhase(*team.task(3)), QStringLiteral("delivered"));
    QCOMPARE(lastPhase(*team.task(6)), QStringLiteral("unconfirmed"));
    QCOMPARE(lastPhase(*team.task(4)), QString());
    QCOMPARE(taskStatus(*team.task(3)).text, QStringLiteral("In progress · attempt 2"));
    QVERIFY(openQuestions(*team.task(2)).isEmpty());
    QVERIFY(openQuestions(*team.task(6)).isEmpty());
    // The queue is derived from the attempts alone.
    QCOMPARE(team.member("backend")->queue, (QList<int>{3}));
    QCOMPARE(team.member("tester")->queue, (QList<int>{6}));
    QCOMPARE(team.member("frontend")->queue, (QList<int>{}));
    QVERIFY(team.member("backend")->queueKnown == false);
}

void TestTeams::feedLinesSpeakOfStartsQuestionsAndHolds()
{
    const auto team = parse(deliveryTeam());
    const auto line = [&](const char *kind, const QJsonObject &detail) {
        return feedLine(team, QJsonObject{{"at", 1}, {"seq", 1}, {"kind", kind}, {"detail", detail}});
    };
    QCOMPARE(line("started", QJsonObject{{"task", 1}, {"attempt", 2}, {"member", "Backend"}}).text,
             QStringLiteral("Started #1 Saved cards API · attempt 2"));
    QCOMPARE(line("started", QJsonObject{{"task", 1}, {"attempt", 2}, {"member", "Backend"}}).by, QStringLiteral("Backend"));
    QCOMPARE(line("start_late", QJsonObject{{"task", 5}, {"attempt", 1}}).tone, Tone::Warn);
    const auto asked = line("question_asked", QJsonObject{{"task", 4}, {"attempt", 1}, {"member", "Tester"}, {"text", "Keep it?"}});
    QCOMPARE(asked.text, QStringLiteral("Question from the member · #4 Ask about the field · attempt 1: Keep it?"));
    QCOMPARE(asked.tone, Tone::Warn);
    QCOMPARE(line("question_asked", QJsonObject{{"task", 4}}).text, QStringLiteral("A question for the Lead · #4 Ask about the field"));
    QCOMPARE(line("question_answered", QJsonObject{{"by", "Lead"}, {"task", 4}, {"attempt", 1}}).text,
             QStringLiteral("Answered the question · #4 Ask about the field · attempt 1"));
    QCOMPARE(line("held", QJsonObject{{"task", 3}, {"reason", "waiting for the owner"}}).text,
             QStringLiteral("Held #3 Held retry: waiting for the owner"));
    QCOMPARE(line("held", QJsonObject{{"task", 3}}).text, QStringLiteral("Held #3 Held retry"));
    QCOMPARE(line("released", QJsonObject{{"by", "Lead"}, {"task", 3}}).text, QStringLiteral("Released #3 Held retry"));
    QCOMPARE(line("report_replaced", QJsonObject{{"task", 2}, {"attempt", 1}}).text,
             QStringLiteral("Replaced the captured reply · #2 Address form · attempt 1"));
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

void TestTeams::feedLinesSpeakOfPhasesAndTheEndOfThePlan()
{
    const auto team = parse(checkout());
    const auto line = [&](const char *kind, const QJsonObject &detail) {
        return feedLine(team, QJsonObject{{"at", 1}, {"seq", 1}, {"kind", kind}, {"detail", detail}});
    };
    const auto added = line("phase_added", QJsonObject{{"by", "Lead"}, {"phase", 2}, {"title", "QA fixes"}, {"tasks", 4},
                                                      {"barrier", true}, {"approval", "review"}});
    QCOMPARE(added.by, QStringLiteral("Lead"));
    QCOMPARE(added.text, QStringLiteral("Added phase 2 · QA fixes · 4 tasks"));
    QCOMPARE(line("phase_added", QJsonObject{{"by", "Lead"}, {"phase", 3}, {"title", ""}, {"tasks", 2}}).text,
             QStringLiteral("Added phase 3 · 2 tasks"));
    QCOMPARE(line("phase_approved", QJsonObject{{"by", "You"}, {"phase", 2}, {"tasks", 4}}).text, QStringLiteral("Approved phase 2 · 4 tasks"));
    QCOMPARE(line("phase_discarded", QJsonObject{{"by", "You"}, {"phase", 2}, {"tasks", 4}}).text, QStringLiteral("Discarded phase 2 · 4 tasks"));
    // The shapes hgs writes now: a discarded draft lists its tasks' titles, a
    // move names the phase, and the drafts after it are renumbered.
    QCOMPARE(line("phase_discarded", QJsonObject{{"by", "Lead"}, {"phase", 3},
                  {"tasks", QJsonArray{QJsonObject{{"id", 9}, {"title", "A"}}, QJsonObject{{"id", 10}, {"title", "B"}}}}}).text,
             QStringLiteral("Discarded phase 3 · 2 tasks"));
    QCOMPARE(line("phase_moved", QJsonObject{{"by", "Lead"}, {"phase", 4}, {"to", 1}, {"renumbered", true}}).text,
             QStringLiteral("Moved phase 4 to position 1 · drafts renumbered"));
    QCOMPARE(line("phase_renumbered", QJsonObject{{"phases", QJsonArray{QJsonObject{{"from", 4}, {"to", 3}}, QJsonObject{{"from", 5}, {"to", 4}}}},
                                                  {"tasks", QJsonArray{}}}).text,
             QStringLiteral("Drafts renumbered: phases 4→3, 5→4"));
    QCOMPARE(line("all_decided", QJsonObject{{"accepted", 7}, {"rejected", 1}, {"cancelled", 0}, {"tasks", 8}}).text,
             QStringLiteral("All tasks decided · waiting for the lead"));
    const auto finished = line("finished", QJsonObject{{"by", "Lead"}, {"accepted", 8}, {"tasks", 8}, {"summary_chars", 120}});
    QCOMPARE(finished.by, QStringLiteral("Lead"));
    QCOMPARE(finished.text, QStringLiteral("Finished · 8 of 8 tasks accepted"));
    QCOMPARE(finished.tone, Tone::Done);
    QCOMPARE(line("lead_told", QJsonObject{{"member", "Lead"}, {"kind", "all_decided"}}).text,
             QStringLiteral("Told the Lead that all tasks are decided"));
    // Old feeds keep their done line.
    QCOMPARE(line("done", QJsonObject{{"accepted", 7}, {"tasks", 8}}).text, QStringLiteral("Done · 7 of 8 tasks accepted"));
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
    QVERIFY(controller.teamOfSession("msi-old", "kimi/checkout/tester"));
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

void TestTeams::controllerSendsPhaseDecisionsAndFinish()
{
    QFile::remove(script() + ".calls");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.watch("team-1");
    bool done = false, ok = false; QString message;
    controller.approvePhase(2, [&](bool success, const QString &text) { done = true; ok = success; message = text; });
    QTRY_VERIFY(done);
    QVERIFY2(ok, qPrintable(message));
    done = false;
    controller.discardPhase(3, [&](bool success, const QString &) { done = true; ok = success; });
    QTRY_VERIFY(done);
    QVERIFY(ok);
    done = false;
    controller.finishTeam(QStringLiteral("All phases are in."), [&](bool success, const QString &) { done = true; ok = success; });
    QTRY_VERIFY(done);
    QVERIFY(ok);
    QFile calls(script() + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QJsonObject approve, discard, finish;
    for (const auto &line : calls.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).object();
        const auto args = call["args"].toArray();
        if (args.size() < 2) continue;
        const auto operation = args.at(1).toString();
        if (operation == "approve-phase") approve = call["input"].toObject();
        else if (operation == "discard-phase") discard = call["input"].toObject();
        else if (operation == "finish") finish = call["input"].toObject();
    }
    // The spec's payloads: request_id, team, phase (or summary), by human.
    QCOMPARE(approve.keys(), (QStringList{"by", "phase", "request_id", "team"}));
    QCOMPARE(approve["phase"].toInt(), 2);
    QCOMPARE(approve["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(approve["by"].toString(), QStringLiteral("human"));
    QVERIFY(!QUuid(approve["request_id"].toString()).isNull());
    QCOMPARE(discard["phase"].toInt(), 3);
    QCOMPARE(discard["by"].toString(), QStringLiteral("human"));
    QVERIFY(!QUuid(discard["request_id"].toString()).isNull());
    QCOMPARE(finish.keys(), (QStringList{"by", "request_id", "summary", "team"}));
    QCOMPARE(finish["summary"].toString(), QStringLiteral("All phases are in."));
    QVERIFY(!QUuid(finish["request_id"].toString()).isNull());
    QVERIFY(approve["request_id"].toString() != finish["request_id"].toString());
}

void TestTeams::controllerSendsHoldReleaseRejectAndReply()
{
    QFile::remove(script() + ".calls");
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    controller.watch("team-1");
    bool done = false, ok = false; QString message;
    const auto send = [&](const std::function<void(TeamController::Done)> &call) {
        done = false; ok = false;
        call([&](bool success, const QString &text) { done = true; ok = success; message = text; });
        QTRY_VERIFY(done);
        QVERIFY2(ok, qPrintable(message));
    };
    send([&](const TeamController::Done &callback) { controller.holdTask(3, QStringLiteral("waiting for the owner"), callback); });
    send([&](const TeamController::Done &callback) { controller.releaseTask(3, callback); });
    send([&](const TeamController::Done &callback) { controller.rejectWithHold(4, QStringLiteral("not yet"), callback); });
    send([&](const TeamController::Done &callback) { controller.answerQuestion(QStringLiteral("q1"), QStringLiteral("Yes, keep it."), callback); });
    QFile calls(script() + ".calls"); QVERIFY(calls.open(QIODevice::ReadOnly));
    QJsonObject hold, release, reject, reply;
    for (const auto &line : calls.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).object();
        const auto args = call["args"].toArray();
        if (args.size() < 2) continue;
        const auto operation = args.at(1).toString();
        if (operation == "hold") hold = call["input"].toObject();
        else if (operation == "release") release = call["input"].toObject();
        else if (operation == "reject") reject = call["input"].toObject();
        else if (operation == "reply") reply = call["input"].toObject();
    }
    // The spec's payloads (A3, A5): request_id, team, and what the command takes.
    QCOMPARE(hold.keys(), (QStringList{"by", "reason", "request_id", "task", "team"}));
    QCOMPARE(hold["task"].toInt(), 3);
    QCOMPARE(hold["reason"].toString(), QStringLiteral("waiting for the owner"));
    QCOMPARE(hold["team"].toString(), QStringLiteral("team-1"));
    QCOMPARE(hold["by"].toString(), QStringLiteral("human"));
    QVERIFY(!QUuid(hold["request_id"].toString()).isNull());
    // Release names no reason and only the task.
    QCOMPARE(release.keys(), (QStringList{"by", "request_id", "task", "team"}));
    QCOMPARE(release["task"].toInt(), 3);
    QVERIFY(!QUuid(release["request_id"].toString()).isNull());
    // Reject with a hold is the same reject plus "hold": true.
    QCOMPARE(reject.keys(), (QStringList{"by", "hold", "reason", "request_id", "task", "team"}));
    QCOMPARE(reject["task"].toInt(), 4);
    QCOMPARE(reject["reason"].toString(), QStringLiteral("not yet"));
    QCOMPARE(reject["hold"].toBool(), true);
    QVERIFY(!QUuid(reject["request_id"].toString()).isNull());
    // Answering is `reply`, with the question and the text.
    QCOMPARE(reply.keys(), (QStringList{"by", "question", "request_id", "team", "text"}));
    QCOMPARE(reply["question"].toString(), QStringLiteral("q1"));
    QCOMPARE(reply["text"].toString(), QStringLiteral("Yes, keep it."));
    QCOMPARE(reply["team"].toString(), QStringLiteral("team-1"));
    QVERIFY(!QUuid(reply["request_id"].toString()).isNull());
    QVERIFY(hold["request_id"].toString() != release["request_id"].toString());
    QVERIFY(release["request_id"].toString() != reject["request_id"].toString());
    QVERIFY(reject["request_id"].toString() != reply["request_id"].toString());
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
    // A running team reads as a summary (design D4); its work is in the Team tab.
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

void TestTeams::modelReadsStructuredReportsAndFindings()
{
    const auto value = QJsonDocument::fromJson(R"({
 "id":"t","name":"T","status":"running","lead":"lead","members":[{"id":"lead","role":"Lead","agent":"claude"}],
 "tasks":[{"id":1,"title":"Work","assignee":"lead","status":"submitted",
   "attempts":[{"n":1,"id":"a1","member":"lead","state":"reported","report":{"text":"Done","at":1.0,
     "unstructured":false,
     "structured":{"summary":"Done and checked","commit":"abc1234",
       "criteria":[{"n":1,"result":"pass","evidence":[0],"note":"Checked"}],
       "evidence":[{"kind":"command","caption":"tests","command":"cargo test","exit":0,"tail":"ok"},
                   {"kind":"image","caption":"Shot","path":"/work/shot.png"},
                   {"kind":"url","caption":"PR","url":"https://example.com/pr/1"}],
       "limits":["Windows not checked"]}}}]}],
 "findings":{"items":[{"id":"f1","from_task":"1","severity":"critical","title":"Race","file":"src/x.rs","line":4,
   "scenario":"Two writers","task":"1","state":"open","by":"Reviewer","at":2.0,"note":""}],"next_cursor":"cursor-2"},
 "findings_summary":{"open":{"critical":1,"normal":0,"later":0},"deferred":3}})").object();
    const auto team = parse(value);
    QCOMPARE(team.tasks.size(), 1);
    const auto &attempt = team.tasks[0].attempts[0];
    QVERIFY(attempt.structured.present);
    QVERIFY(!attempt.unstructured);
    QCOMPARE(attempt.structured.summary, QStringLiteral("Done and checked"));
    QCOMPARE(attempt.structured.commit, QStringLiteral("abc1234"));
    QCOMPARE(attempt.structured.criteria.size(), 1);
    QCOMPARE(attempt.structured.criteria[0].result, QStringLiteral("pass"));
    QCOMPARE(attempt.structured.criteria[0].evidence, QList<int>{0});
    QCOMPARE(attempt.structured.evidence.size(), 3);
    QCOMPARE(attempt.structured.evidence[0].command, QStringLiteral("cargo test"));
    QVERIFY(attempt.structured.evidence[0].hasExit);
    QCOMPARE(attempt.structured.evidence[1].kind, QStringLiteral("image"));
    QCOMPARE(attempt.structured.limits, QStringList{QStringLiteral("Windows not checked")});
    QCOMPARE(team.findings.size(), 1);
    QCOMPARE(team.findings[0].severity, QStringLiteral("critical"));
    QCOMPARE(team.findings[0].line, 4);
    QCOMPARE(team.findingsNext, QStringLiteral("cursor-2"));
    QCOMPARE(team.findingsSummary.value("deferred").toInt(), 3);
}

void TestTeams::modelReadsTheHierarchy()
{
    const auto value = QJsonDocument::fromJson(R"({
 "id":"child","name":"Child","status":"running","lead":"lead",
 "parent":{"team":"parent","task":3,"lead":"lead","member":"backend"},
 "children":[{"team":"grand","name":"Grand","status":"running","done":1,"total":2}],
 "members":[{"id":"lead","role":"Lead","agent":"claude"}],
 "tasks":[{"id":1,"title":"Delegated","assignee":"lead","status":"in_progress",
   "subteam":{"team":"grand","status":"running","done":1,"total":2}}]})").object();
    const auto team = parse(value);
    QVERIFY(team.parent.known);
    QCOMPARE(team.parent.team, QStringLiteral("parent"));
    QCOMPARE(team.parent.task, 3);
    QCOMPARE(team.children.size(), 1);
    QCOMPARE(team.children[0].team, QStringLiteral("grand"));
    QCOMPARE(team.children[0].done, 1);
    QVERIFY(team.tasks[0].subteam.known);
    QCOMPARE(team.tasks[0].subteam.team, QStringLiteral("grand"));
    QCOMPARE(team.tasks[0].subteam.total, 2);
}

// U25 part 10 (spec 5.6): a sub-lead's reports hang under it; the tree, the
// reviewer, the feed and the member status follow.
void TestTeams::subLeadsFormTheMemberTree()
{
    auto json = checkout();
    auto members = json["members"].toArray();
    // The Integrator becomes a sub-lead, the Backend reports to it.
    const QString sub = QStringLiteral("integrator"), report = QStringLiteral("backend");
    for (int i = 0; i < members.size(); ++i) {
        auto m = members[i].toObject();
        if (m["id"].toString() == sub) m["sub"] = true;
        if (m["id"].toString() == report) m["parent"] = sub;
        members[i] = m;
    }
    json["members"] = members;
    const Team team = parse(json);
    QVERIFY(team.member(sub)->sub);
    QCOMPARE(team.member(report)->parent, sub);
    QCOMPARE(team.reviewerOf(report), sub);
    QCOMPARE(team.reviewerOf(QStringLiteral("frontend")), team.lead);
    QVERIFY(team.reviewerOf(team.lead).isEmpty());
    QCOMPARE(team.reportsOf(sub).size(), 1);
    QCOMPARE(team.member(report)->json()["parent"].toString(), sub);
    // Depth first: the lead, its direct reports in order, the sub-lead
    // followed by its report one level in.
    const auto tree = team.memberTree();
    QStringList order; QList<int> depths;
    for (const auto &node : tree) { order << node.member->id; depths << node.depth; }
    QCOMPARE(order.first(), team.lead);
    QCOMPARE(order.last(), report);
    QCOMPARE(order.at(order.size() - 2), sub);
    QCOMPARE(depths.last(), 2);
    QCOMPARE(tree.last().through, QList<bool>{false});   // the sub-lead is the last direct report: no tail
    QVERIFY(tree.last().last);
    QVERIFY(!order.mid(1, order.size() - 3).contains(report));
    // A folded list: with the sub-lead hidden its report hangs from the lead.
    const auto folded = team.memberTree([&](const QString &id) { return id != sub; });
    QVERIFY(std::none_of(folded.cbegin(), folded.cend(), [&](const Team::TreeNode &node) { return node.member->id == sub; }));
    for (const auto &node : folded) if (node.member->id == report) QCOMPARE(node.depth, 1);
    // A sub-lead's word in the feed.
    const auto line = [&](const QString &kind, const QJsonObject &detail) {
        return feedLine(team, QJsonObject{{"at", 1}, {"seq", 1}, {"kind", kind}, {"detail", detail}});
    };
    const auto handed = line("handed_out", {{"by", "QA lead"}, {"task", 2}, {"to", "Tester"}, {"attempt", 1}});
    QCOMPARE(handed.by, QStringLiteral("QA lead"));
    QVERIFY2(handed.text.startsWith(QStringLiteral("Task to Tester")), qPrintable(handed.text));
    QCOMPARE(line("handed_out", {{"by", "Lead"}, {"task", 2}, {"to", "Tester"}}).by, QStringLiteral("Lead"));
    QVERIFY(line("reported", {{"by", "Tester"}, {"to", "QA lead"}, {"task", 2}}).text.startsWith(QStringLiteral("Report to QA lead")));
    QVERIFY(line("reported", {{"by", "Tester"}, {"task", 2}}).text.startsWith(QStringLiteral("Report to Lead")));
    QCOMPARE(line("accepted", {{"by", "QA lead"}, {"task", 2}}).by, QStringLiteral("QA lead"));
    // Its status: reviewing a report's work, else coordinating its reports.
    Team running = team;
    running.status = "running";
    Member *subMember = nullptr;
    for (auto &m : running.members) if (m.id == sub) subMember = &m;
    subMember->session = "codex/x/sub"; subMember->error.clear(); subMember->state = "active";
    for (auto &t : running.tasks) if (t.assignee == sub || t.assignee == report) t.status = "pending";
    for (auto &t : running.tasks) for (auto &a : t.attempts) if (a.member == sub) a.state = "rejected";
    QCOMPARE(memberStatus(running, *subMember, Live{}).text, QStringLiteral("Coordinating 1 report"));
    Task *reviewed = nullptr;
    for (auto &t : running.tasks) if (!reviewed && t.assignee != sub) { reviewed = &t; }
    reviewed->assignee = report; reviewed->status = "submitted";
    const auto status = memberStatus(running, *subMember, Live{});
    QCOMPARE(status.kind, QStringLiteral("reviewing"));
    QCOMPARE(status.text, QStringLiteral("Reviewing #%1 %2").arg(reviewed->id).arg(reviewed->title));
}

QTEST_MAIN(TestTeams)
#include "test_teams.moc"
