// WhiteWh fork, #713: the agent-team UI tests of the catalog editor: roles,
// templates, routing, planning rules, pipelines and stages.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include "ActivityView.h"
#include "CollapsibleBlock.h"
#include "HgsClient.h"
#include "TeamCatalogEditor.h"
#include "TeamDesignerPage.h"
#include "TeamTaskCard.h"
#include "TeamWorkspace.h"

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

class TestTeamsUiCatalog : public QObject, public TeamsUiFixture {
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
    void catalogModelAppliesKeptProposals();
    void catalogEditorSavesTheNextVersion();
    void catalogEditorKeepsTheFieldsItDoesNotEdit();
    void catalogRoleChoosesAccountModelAndComputer();
    void catalogEditorCarriesNewPhaseApproval();
    void catalogApplySaysUndecidedChangesAreDropped();
    void planningRulesAreEditedSavedResetAndProposed();
    void catalogEditorMarksManualEdits();
    void unsavedEditsMoveOntoANewerCatalog();
    void catalogRoleModelsAndContextAreSaved();
    void catalogPipelinesAreEditedAndSaved();
    void catalogPipelineProposalsHavePlates();
    void catalogPipelineTryWalksASampleTask();
    void catalogRoleEditorEditsEveryField();
    void aPromptOrderSavesTheTaskTemplateItShows();
    void catalogRoleListGrowsToItsRows();
    void catalogTemplateEditorRosterSkeletonAndChecks();
    void catalogRoutingEditsAndTriesAGoal();
    void catalogHeaderRevertDraftAndBanners();
    void inspectorShowsThePipelineAndOverridesAskForAReason();
    void commandStagesShowTheirCommandExitAndTail();
    void catalogRoleDeleteNamesThePresetsItStrips();
    void stageOverridesReachHgsWithTheirReason();
    void planEditorPicksATasksPipeline();
};

void TestTeamsUiCatalog::catalogModelAppliesKeptProposals()
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

void TestTeamsUiCatalog::catalogEditorSavesTheNextVersion()
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
void TestTeamsUiCatalog::catalogEditorKeepsTheFieldsItDoesNotEdit()
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

void TestTeamsUiCatalog::catalogRoleChoosesAccountModelAndComputer()
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

void TestTeamsUiCatalog::catalogEditorCarriesNewPhaseApproval()
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

// N37 (#548): Apply ends the whole draft, so its confirmation names the
// undecided changes it drops instead of saying they stay.
void TestTeamsUiCatalog::catalogApplySaysUndecidedChangesAreDropped()
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

// U28: the catalog's Planning rules tab: edit with the unsaved-changes
// state, Save and Revert, Reset to built-in after a confirmation (a save
// without the rules, which hgs fills with its own), and a designer proposal
// shown as CHANGED with Keep / Reject / Undo and its line diff.
void TestTeamsUiCatalog::planningRulesAreEditedSavedResetAndProposed()
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

void TestTeamsUiCatalog::catalogEditorMarksManualEdits()
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

// The owner, 2026-11-01: the designer applied v6 while the catalog had a hand
// edit; Save then sent v5 and was refused ("The catalog changed"), and the
// editor never showed v6. A newer catalog now takes the edits along: the
// edited role keeps the edit, the rest is the newer version's, and Save
// writes on it.
void TestTeamsUiCatalog::unsavedEditsMoveOntoANewerCatalog()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    TeamCatalogEditor editor(&controller);
    editor.resize(1000, 760); editor.show();
    auto *roles = editor.findChild<QListWidget *>("teamCatalogRoles");
    QTRY_COMPARE(roles->count(), 3);
    roles->setCurrentRow(1);
    auto *name = editor.findChild<QLineEdit *>("teamRoleName");
    name->setText("Backend API"); emit name->editingFinished();
    QTRY_VERIFY(editor.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 unsaved change"));
    // Version 4 arrives: the tester changed and a role was added.
    auto next = catalog();
    auto body = next["catalog"].toObject();
    body["version"] = 4;
    auto list = body["roles"].toArray();
    auto tester = list[2].toObject(); tester["model"] = "kimi-k3"; list[2] = tester;
    list.append(QJsonObject{{"id", "ue"}, {"name", "UE World"}, {"agent", "claude"}, {"prompt", "Build levels."}});
    body["roles"] = list; next["catalog"] = body;
    next["draft"] = QJsonObject{{"base", 4}, {"proposals", QJsonArray{}}};
    QVERIFY(FakeCli::replace(script() + ".catalog", QJsonDocument(next).toJson(QJsonDocument::Compact)));
    controller.refreshCatalog();
    QTRY_COMPARE(roles->count(), 4);
    QVERIFY(editor.findChild<QLabel *>("teamCatalogStatus")->text().contains("1 unsaved change"));
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave");
    QVERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto input = calls(script(), "catalog set").first();
    QCOMPARE(input["version"].toInt(), 4);
    QStringList saved;
    for (const auto &value : input["catalog"].toObject()["roles"].toArray()) {
        const auto role = value.toObject();
        saved << role["name"].toString() + "/" + role["model"].toString();
    }
    QCOMPARE(saved, (QStringList{"Lead/", "Backend API/", "Tester/kimi-k3", "UE World/"}));
}

// U45/U46: a role's models (model and effort) and its context before a task
// are edited in the catalog and saved with the role; the other roles gain nothing.
void TestTeamsUiCatalog::catalogRoleModelsAndContextAreSaved()
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "u45-catalog-role-models-dark");
        lightPalette(); editor.setDark(false); lightShot(&editor, "u45-catalog-role-models-light"); darkPalette(); editor.setDark(true);
    }
    auto *save = editor.findChild<QPushButton *>("teamCatalogSave"); QTRY_VERIFY(save->isEnabled());
    save->click();
    QTRY_COMPARE(calls(script(), "catalog set").size(), 1);
    const auto sent = calls(script(), "catalog set").first()["catalog"].toObject()["roles"].toArray();
    const auto backend = sent[1].toObject();
    QCOMPARE(backend["models"].toArray(), (QJsonArray{QJsonObject{{"model", "gpt-5.5-codex"}, {"effort", "high"}}, QJsonObject{{"model", "gpt-5.5"}}}));
    QCOMPARE(backend["context_before_task"].toString(), QStringLiteral("clear"));
    for (int i : {0, 2}) QVERIFY(!sent[i].toObject().contains("models") && !sent[i].toObject().contains("context_before_task"));
}

void TestTeamsUiCatalog::catalogPipelinesAreEditedAndSaved()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { Settings::remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "u36-pipelines-dark");
        editor.setDark(false); lightShot(&editor, "u36-pipelines-light"); darkPalette(); editor.setDark(true);
    }
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "u36-pipelines-land-dark");
        editor.setDark(false); lightShot(&editor, "u36-pipelines-land-light"); darkPalette(); editor.setDark(true);
    }
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
void TestTeamsUiCatalog::catalogPipelineProposalsHavePlates()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { Settings::remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&page, "u36-designer-dark");
        page.setDark(false); lightShot(&page, "u36-designer-light"); darkPalette(); page.setDark(true);
    }
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
void TestTeamsUiCatalog::catalogPipelineTryWalksASampleTask()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { Settings::remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "u36-pipelines-try-dark");
        editor.setDark(false); lightShot(&editor, "u36-pipelines-try-light"); darkPalette(); editor.setDark(true);
    }
}

void TestTeamsUiCatalog::catalogRoleEditorEditsEveryField()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "ux-9-role-editor-dark");
        editor.setDark(false); lightShot(&editor, "ux-9-role-editor-light"); darkPalette(); editor.setDark(true);
    }
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

// C1 (#543): a role whose task template was never typed shows the default
// template. Whatever writes its prompt order (a move, or typing in Team
// protocol) saves that template too, so the server never gets an order with
// "task" and an empty template.
void TestTeamsUiCatalog::aPromptOrderSavesTheTaskTemplateItShows()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
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

void TestTeamsUiCatalog::catalogRoleListGrowsToItsRows()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
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

// U25 part 9 (spec 11.4): a template's roster counts with "only if needed",
// work mode with its warning, the task skeleton with phases, planning,
// approval, new phases, max members, the checks, and the folding blocks.
void TestTeamsUiCatalog::catalogTemplateEditorRosterSkeletonAndChecks()
{
    Settings::set("workspace/skin", "whitewh");
    const auto keys = {CollapsibleBlock::settingsKey("catalog-roster"), CollapsibleBlock::settingsKey("catalog-skeleton"), QStringLiteral("ui/catalogHideUnusedRoles")};
    for (const auto &key : keys) Settings::remove(key);
    const auto restore = qScopeGuard([keys] { Settings::remove("workspace/skin"); for (const auto &key : keys) Settings::remove(key); darkPalette(); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "ux-9-templates-issues-dark");
        editor.setDark(false); lightShot(&editor, "ux-9-templates-issues-light"); darkPalette(); editor.setDark(true);
    }
    // Hide the unused roles: none here.
    QCOMPARE(editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->text(), QStringLiteral("Hide unused roles (0)"));
    editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->click();
    QTRY_COMPARE(editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->text(), QStringLiteral("Show all catalog roles (2)"));
    editor.findChild<QPushButton *>("teamTemplateUnusedToggle")->click();
    // Roster and Task skeleton fold into summaries, remembered.
    editor.findChild<QPushButton *>("teamTemplateRosterToggle")->click();
    QTRY_VERIFY(editor.findChildren<QWidget *>("teamTemplateSeatCount").isEmpty());
    QCOMPARE(Settings::value(CollapsibleBlock::settingsKey("catalog-roster")).toBool(), false);
    QCOMPARE(editor.findChild<QLabel *>("teamTemplateRosterSummary")->text(), QStringLiteral("2 members + 1 only if needed · 2 roles"));
    editor.findChild<QPushButton *>("teamTemplateSkeletonToggle")->click();
    QTRY_VERIFY(editor.findChild<QLabel *>("teamTemplateSkeletonSummary"));
    QCOMPARE(editor.findChild<QLabel *>("teamTemplateSkeletonSummary")->text(), QStringLiteral("4 tasks in 2 phases: Plan → Build → Test → New task"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "ux-9-templates-folded-dark");
        editor.setDark(false); lightShot(&editor, "ux-9-templates-folded-light"); darkPalette(); editor.setDark(true);
    }
    editor.findChild<QPushButton *>("teamTemplateRosterToggle")->click();
    editor.findChild<QPushButton *>("teamTemplateSkeletonToggle")->click();
    QTRY_COMPARE(editor.findChildren<QWidget *>("teamTemplateSeatCount").size(), 2);
    segment("teamTemplateWork", "worktree");
    QTRY_VERIFY(!editor.findChild<QLabel *>("teamTemplateWorkWarning"));
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&editor, "ux-9-templates-dark");
        editor.setDark(false); lightShot(&editor, "ux-9-templates-light"); darkPalette(); editor.setDark(true);
    }
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

// U25 part 9 (spec 11.5): routing rules are edited in place - order, words,
// Use (a template or "+ role"), approval - and Try it names the template, the
// rule, the added roles, approval and work mode, with the roster as chips.
void TestTeamsUiCatalog::catalogRoutingEditsAndTriesAGoal()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
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
void TestTeamsUiCatalog::catalogHeaderRevertDraftAndBanners()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] {
        Settings::remove("workspace/skin"); darkPalette();
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&settings, "ux-9-settings-teams-dark");
        settings.setDark(false); lightShot(&settings, "ux-9-settings-teams-light"); darkPalette(); settings.setDark(true);
    }
    roles->setCurrentRow(1);
    auto *name = settings.findChild<QLineEdit *>("teamRoleName"); name->setText("Backend API"); emit name->editingFinished();
    QTRY_VERIFY(settings.findChild<QLabel *>("teamCatalogStatus")->text().contains("● 1 unsaved change"));
    QVERIFY(settings.findChild<QPushButton *>("teamCatalogRevert")->isEnabled() && settings.findChild<QPushButton *>("teamCatalogSave")->isEnabled());
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&settings, "ux-9-catalog-dirty-dark");
        settings.setDark(false); lightShot(&settings, "ux-9-catalog-dirty-light"); darkPalette(); settings.setDark(true);
    }
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&draft, "ux-9-draft-role-dark");
        draft.setDark(false); lightShot(&draft, "ux-9-draft-role-light"); darkPalette(); draft.setDark(true);
    }
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&draft, "ux-9-draft-stale-dark");
        draft.setDark(false); lightShot(&draft, "ux-9-draft-stale-light"); darkPalette(); draft.setDark(true);
    }
    draft.findChild<QPushButton *>("teamCatalogRedo")->click(); QCOMPARE(redo, 4);
    draft.findChild<QPushButton *>("teamCatalogBannerDiscard")->click();
    QTRY_COMPARE(calls(script(), "catalog discard").size(), 1);
}

// U36: the inspector's Pipeline section: the flow, where the test works, what
// changed and what it holds, every attempt and round with its verdict and
// findings (fixed ones dimmed), and what comes next; the overrides each ask
// with what they do, wait for a reason and send it, and Cancel sends nothing.
// Shots in both themes, with the Skip form open.
void TestTeamsUiCatalog::inspectorShowsThePipelineAndOverridesAskForAReason()
{
    Settings::set("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        Settings::remove("workspace/skin"); darkPalette();
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&card, "u36-inspector-dark");
        lightPalette(); card.setTask(team, 4, live, false);
        lightShot(&card, "u36-inspector-light");
        darkPalette(); card.setTask(team, 4, live, true);
    }
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

// #538 N8: a command or land stage shows the command, its exit and its output
// tail, never a member, a queue or what a review may do. L5: a task the lead
// sent back at the round limit shows no "returned without the lead" chip. L6:
// a stage no member may take warns on its card and makes the team Stuck. QA
// #539: the phase header counts the folders its tests hold, and a running test
// lists the suites hgs recorded (no shots line: hgs records none).
void TestTeamsUiCatalog::commandStagesShowTheirCommandExitAndTail()
{
    Settings::set("workspace/skin", "whitewh");
    TeamUi::workedClock() = &stageNow;
    const auto restore = qScopeGuard([] {
        Settings::remove("workspace/skin"); darkPalette();
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
void TestTeamsUiCatalog::catalogRoleDeleteNamesThePresetsItStrips()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([this] { Settings::remove("workspace/skin"); darkPalette(); writeCatalog(script(), catalog()); });
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
void TestTeamsUiCatalog::stageOverridesReachHgsWithTheirReason()
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
void TestTeamsUiCatalog::planEditorPicksATasksPipeline()
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

QTEST_MAIN(TestTeamsUiCatalog)
#include "test_teamsui_catalog.moc"
