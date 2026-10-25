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
    void modelReadsTheTeamAndDerivesMemberStates();
    void modelReadsPhasesPinnedAndTheWaitingState();
    void modelReadsAttemptStatesQuestionsHoldsAndReports();
    void attemptPhasesAreDerivedForAnOlderHgs();
    void oldStatusJsonStillParses();
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
    // hgs decides when it sends the phase itself.
    attempt.phase = QStringLiteral("stopped");
    QCOMPARE(attemptPhase(task, attempt), QStringLiteral("stopped"));
    // No attempts, no phase.
    QCOMPARE(lastPhase(task), QString());
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

QTEST_MAIN(TestTeams)
#include "test_teams.moc"
