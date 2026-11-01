// WhiteWh fork, #713: the agent-team UI tests of New team (the dialog and the
// tab), its drafts and templates, and the Team designer.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "Settings.h"
#include "ActivityView.h"
#include "HgsClient.h"
#include "QuestionCard.h"
#include "TeamCatalogEditor.h"
#include "TeamConversation.h"
#include "TeamDesignerPage.h"
#include "TeamDesignerPanel.h"
#include "TeamNewDialog.h"
#include "TeamNewTab.h"
#include "TeamTemplateList.h"

namespace {

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

class TestTeamsUiNewteam : public QObject, public TeamsUiFixture {
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
        TeamNewTab::setDraftPollInterval(100);
    }
    void init() { setUp(); }
    void cleanup() { tearDown(); }
    void newTeamStartsTheLeadWithTheTemplate();
    void newTeamOffersEachAgentsModels();
    void newTeamRowErrorsKeepAnOpenListOpen();
    void newTeamTabKeepsAMemberListOpenUntilItCloses();
    void newTeamMaxMembersStepsUpAndDown();
    void designerDraftIsKeptThenApplied();
    void folderPolicyKeepsAutoWhileTyping();
    void typoThenGitCreatesWorktrees();
    void emptyPanelOffersNewTeamAndDesigner();
    void embeddedNewTeamIsLedByTheSession();
    void draftDiscardAsksAndCanStopTheLead();
    void draftUpdateRetriesAStaleRevision();
    void draftCreateSavesFirstAndShowsBlockersInRed();
    void templatesAreCompactWithSearchAndFavourites();
    void templateListHidesItsOldRowsAtOnce();
    void dialogTemplateListAppliesOnlyWhenChosen();
    void designerSessionKeepsItsNameAndLeadsNoTeam();
    void designerRedoAsksOnlyARunningDesigner();
    void designerPageStartsOrOpensTheDesigner();
    void draftIsDecidedWithoutADesignerSession();
    void designerQuestionIsAnsweredInTheWindow();
};

void TestTeamsUiNewteam::newTeamStartsTheLeadWithTheTemplate()
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
    QTRY_VERIFY(!dialog.findChild<QCheckBox *>("teamNewWorktrees")->isEnabled());   // #744: the worker's answer
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

// #738 (owner, 2026-10-31: with Kimi as the lead the model picker was empty):
// New team's model pickers list the selected account's native catalog (as the
// role editor reads it; OpenCode's model_options from its account row), fall
// back to Teams::knownModels when the catalog is empty, refill when the agent
// or the account changes, and keep a typed model. The effort picker follows
// the chosen model's efforts where the catalog names them.
void TestTeamsUiNewteam::newTeamOffersEachAgentsModels()
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

// A row error flips a property on its own frame instead of setting the row's
// style sheet: that re-styled every combo of the row, popup included, and the
// Windows 11 style closed an open list (the member's role) under the pointer.
void TestTeamsUiNewteam::newTeamRowErrorsKeepAnOpenListOpen()
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
void TestTeamsUiNewteam::newTeamTabKeepsAMemberListOpenUntilItCloses()
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
void TestTeamsUiNewteam::newTeamMaxMembersStepsUpAndDown()
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
    // macOS' native stepper reports its rects over the editor's; its clicks
    // below still step. Elsewhere an editor over a button eats its clicks.
    if (max->style()->name().compare(QLatin1String("macos"), Qt::CaseInsensitive) != 0) {
        QVERIFY2(!up.intersects(editor->geometry()), qPrintable(QStringLiteral("up %1,%2 %3x%4 under the editor").arg(up.x()).arg(up.y()).arg(up.width()).arg(up.height())));
        QVERIFY(!down.intersects(editor->geometry()));
    }
    QCOMPARE(max->style()->hitTestComplexControl(QStyle::CC_SpinBox, &option, up.center(), max), QStyle::SC_SpinBoxUp);
    QCOMPARE(max->style()->hitTestComplexControl(QStyle::CC_SpinBox, &option, down.center(), max), QStyle::SC_SpinBoxDown);
    QCOMPARE(max->value(), 6);
    QTest::mouseClick(max, Qt::LeftButton, {}, up.center());
    QCOMPARE(max->value(), 7);
    QTest::mouseClick(max, Qt::LeftButton, {}, down.center());
    QCOMPARE(max->value(), 6);
}

void TestTeamsUiNewteam::designerDraftIsKeptThenApplied()
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

// N10: the folder policy keeps auto while the path is typed; a finished non-Git
// path forces shared, and editing again returns to auto. Dialog and tab.
void TestTeamsUiNewteam::folderPolicyKeepsAutoWhileTyping()
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
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    // While the path is typed, auto stays.
    edit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/sub"));
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    // Finishing the path forces shared.
    QMetaObject::invokeMethod(edit, "editingFinished");
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    // Editing again returns to auto.
    edit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/sub2"));
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    shot(&dialog, "new-team-folder-policy-typing");
    // The same in the workspace tab.
    const QList<TeamNewTab::Computer> tabComputers{{QString(), "this computer", true}, {"msi-old", "msi-old", false}};
    TeamNewTab tab(&controller, &client, tabComputers, TeamNewTab::Coordinator{}, QStringLiteral("Checkout"));
    tab.resize(700, 900); tab.show();
    auto *tabEdit = tab.findChild<QLineEdit *>("teamNewFolder");
    auto *tabPolicy = tab.findChild<QComboBox *>("teamNewFolderPolicy");
    QVERIFY(tabEdit && tabPolicy);
    tabEdit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/one"));
    QTRY_COMPARE(tabPolicy->currentData().toString(), QStringLiteral("auto"));
    QMetaObject::invokeMethod(tabEdit, "editingFinished");
    QTRY_COMPARE(tabPolicy->currentData().toString(), QStringLiteral("shared"));
    tabEdit->setText(QDir::toNativeSeparators(folder.path()) + QStringLiteral("/two"));
    QTRY_COMPARE(tabPolicy->currentData().toString(), QStringLiteral("auto"));
    shot(&tab, "new-team-tab-folder-policy-typing");
}

// N10: a typo path that is not a Git folder keeps auto with no worktrees; fixing
// it to a Git folder restores the worktrees, and the created members get them.
void TestTeamsUiNewteam::typoThenGitCreatesWorktrees()
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
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    QTRY_VERIFY(worktrees->isChecked());
    edit->setText(typo);
    QMetaObject::invokeMethod(edit, "editingFinished");
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("shared"));
    QTRY_VERIFY(!worktrees->isChecked());
    // Fixing the path to the repository returns auto and the worktrees.
    edit->setText(gitFolder);
    QMetaObject::invokeMethod(edit, "editingFinished");
    QTRY_COMPARE(policy->currentData().toString(), QStringLiteral("auto"));
    QVERIFY(worktrees->isEnabled());
    QTRY_VERIFY(worktrees->isChecked());
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
void TestTeamsUiNewteam::emptyPanelOffersNewTeamAndDesigner()
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

void TestTeamsUiNewteam::embeddedNewTeamIsLedByTheSession()
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
    QTRY_VERIFY(start->isEnabled());   // #744: once the worker has seen the folder
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
void TestTeamsUiNewteam::draftDiscardAsksAndCanStopTheLead()
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
void TestTeamsUiNewteam::draftUpdateRetriesAStaleRevision()
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
void TestTeamsUiNewteam::draftCreateSavesFirstAndShowsBlockersInRed()
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

// F6: many templates in a compact list with a search field and favourites; the
// big card is not part of the list.
void TestTeamsUiNewteam::templatesAreCompactWithSearchAndFavourites()
{
    Settings::remove(QStringLiteral("teams/templateFavourites"));
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
        if (shotsWanted()) {   // #713: the light half of the shots
            list.setDark(false);
            lightShot(&list, "f6-templates-compact-light");
            darkPalette();
            list.setDark(true);
        }
    }
    Settings::remove(QStringLiteral("teams/templateFavourites"));
}

// Owner, 2026-10-30 (Debian 13, KDE): New team showed the empty list's "No
// template" row over the first template. A refill hides the old rows at once,
// before their deferred delete, which a dialog's own event loop postpones.
void TestTeamsUiNewteam::templateListHidesItsOldRowsAtOnce()
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

// F6 in the modal dialog: the list replaces the combo and applies a template
// only when chosen; building and closing the dialog apply nothing.
void TestTeamsUiNewteam::dialogTemplateListAppliesOnlyWhenChosen()
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
void TestTeamsUiNewteam::designerSessionKeepsItsNameAndLeadsNoTeam()
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

// N38 (#548): "Ask designer to redo" only asks a running designer; with none
// the click would start a new designer session, so only Discard is offered.
void TestTeamsUiNewteam::designerRedoAsksOnlyARunningDesigner()
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

// U25 part 9 (spec 11.1): the Team designer's chat. Before the first message
// the intro and the composer stand in the middle (no example chips); the
// field grows from 2 to 8 lines; the first message starts the designer with
// the agent, model and effort of the model menu, and a failed start keeps the
// text. Running: the proposals and the DRAFT bar right above the composer,
// Show in catalog, Apply from the bar, decided cards close and come back.
void TestTeamsUiNewteam::designerPageStartsOrOpensTheDesigner()
{
    Settings::set("workspace/skin", "whitewh");
    const auto restore = qScopeGuard([] { Settings::remove("workspace/skin"); darkPalette(); });
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&page, "ux-9-designer-start-dark");
        page.setDark(false); lightShot(&page, "ux-9-designer-start-light"); darkPalette(); page.setDark(true);
    }
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
    if (shotsWanted()) {   // #713: the light half of the shots
        shot(&page, "ux-9-designer-draft-dark");
        page.setDark(false); lightShot(&page, "ux-9-designer-draft-light"); darkPalette(); page.setDark(true);
    }
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

// U15: with no designer session the draft is still decided, from the
// proposals list and from the catalog, and applied; a refused decision shows
// its error next to the proposal in every view of the draft.
void TestTeamsUiNewteam::draftIsDecidedWithoutADesignerSession()
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

// U16: the Team designer's own session waits at Claude's folder trust. The
// window shows its recorded activity and the question as the session view's
// card, in place of the composer; answering it there sends the native answer
// with the question's id, hash and run, and the card goes once it is answered.
void TestTeamsUiNewteam::designerQuestionIsAnsweredInTheWindow()
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

QTEST_MAIN(TestTeamsUiNewteam)
#include "test_teamsui_newteam.moc"
