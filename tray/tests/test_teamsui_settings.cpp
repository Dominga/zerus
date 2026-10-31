// WhiteWh fork, #713: the agent-team UI tests of Settings, team settings,
// agent accounts and the dialogs' theme.
// The fixture (the fake hgs, its teams and the shot helpers) is shared:
// support/TeamsUiFixture.h.
#include "TeamsUiFixture.h"
#include "AccountSwitch.h"
#include "ActivityView.h"
#include "ForkUpdater.h"
#include "HgsClient.h"
#include "MessageComposer.h"
#include "SettingsPage.h"
#include "TeamConversation.h"
#include "TeamSettingsDialog.h"
#include "TeamWorkspace.h"

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

// A shot while a held hgs call runs: shot() would wait for that call.
void shotWhileHeld(QWidget *widget, const QString &name)
{
    const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (dir.isEmpty()) return;
    QCoreApplication::processEvents();
    if (!widget->grab().save(QDir(dir).filePath(name + ".png"))) qWarning() << "screenshot not saved:" << name;
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

class TestTeamsUiSettings : public QObject, public TeamsUiFixture {
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
    void settingsPageShowsTheSubagentSetting();
    void hgsSettingsAreReadInTheBackground();
    void aSessionRecordShowsTheUnconfirmableSubagentNote();
    void conversationAccountPickerConfirmsAndSwitches();
    void accountPickerStatesInShots();
    void moveToAnotherAccountNamesTheMembersAndAsksOnce();
    void teamCustomizationIsOptIn();
    void teamSettingsSendOnlyWhatChanged();
    void teamSettingsAllowLandingAndEditMemberModels();
    void teamSettingsPausesAndPhaseAcceptance();
    void aboutNamesTheForkAndItsUpdate();
    void teamDialogsDrawTheThemeNotTheSystem();
};

void TestTeamsUiSettings::settingsPageShowsTheSubagentSetting()
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
void TestTeamsUiSettings::hgsSettingsAreReadInTheBackground()
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

void TestTeamsUiSettings::aSessionRecordShowsTheUnconfirmableSubagentNote()
{
    const QJsonArray rows{ QJsonObject{{"name", "codex/x/dev"}, {"cmd", "codex"},
        {"subagent_confirm", "subagent confirmation not supported by codex"}} };
    const auto parsed = HgsClient::parseSessions(rows);
    QCOMPARE(parsed.size(), 1);
    QCOMPARE(parsed.first().subagentConfirm, QStringLiteral("subagent confirmation not supported by codex"));
}

// Choosing an account asks first; Cancel sends nothing; Move sends one
// switch-account with the account and a new request id, the chip says
// Switching… until the receipt, then the conversation names the result and
// the next read the new account. A refusal names its reason and changes
// nothing; while the agent works the chip is disabled and says why.
void TestTeamsUiSettings::conversationAccountPickerConfirmsAndSwitches()
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
void TestTeamsUiSettings::accountPickerStatesInShots()
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

// U54: the members menu's Move to another account names every member of the
// agent whose session runs on the chosen account, the lead included, asks
// once, sends one swap-member with from_account, to_account and provider, and
// shows each member's progress: moving, then resumed, fresh and launched; a
// refusal names its reason. The cards show the account each session runs on.
void TestTeamsUiSettings::moveToAnotherAccountNamesTheMembersAndAsksOnce()
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

// U45-U52 (spec 0): a team, a role and a task that set none of the new fields
// look and send exactly as before: no new key in a plan or a catalog, nothing
// to save in the team settings, the task editor keeps today's fields.
void TestTeamsUiSettings::teamCustomizationIsOptIn()
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

// U48/U51/U52: the team settings send only what changed (a branch per task
// switched off is null), refuse a branch name without {id}, and keep the
// intake keys: a new key is shown once, a revoke asks first.
void TestTeamsUiSettings::teamSettingsSendOnlyWhatChanged()
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

// U63 (owner, 2026-10-31): who removes the lead's pause (only with the
// owner's consent unless he lets the lead) and whether new phases require his
// acceptance; both off by default and sent only when changed.
void TestTeamsUiSettings::teamSettingsPausesAndPhaseAcceptance()
{
    HgsClient client(FakeCli::program(script()));
    TeamController controller(&client);
    {
        TeamSettingsDialog dialog(&controller, parse(team()), true);
        dialog.show();
        auto *unpause = dialog.findChild<QComboBox *>("teamSettingsLeadUnpause");
        auto *acceptance = dialog.findChild<QCheckBox *>("teamSettingsPhaseAcceptance");
        QVERIFY(unpause && acceptance);
        QCOMPARE(unpause->currentData().toBool(), false);
        QCOMPARE(unpause->currentText(), QStringLiteral("Only with my consent"));
        QVERIFY(!acceptance->isChecked());
        QVERIFY(dialog.changes().isEmpty());
        auto *save = dialog.findChild<QPushButton *>("teamSettingsSave");
        QVERIFY(!save->isEnabled());
        unpause->setCurrentIndex(unpause->findData(true));
        QCOMPARE(unpause->currentText(), QStringLiteral("The lead removes its own pause"));
        acceptance->setChecked(true);
        QVERIFY(save->isEnabled());
        save->click();
        QTRY_COMPARE(calls(script(), "settings").size(), 1);
        QCOMPARE(calls(script(), "settings").first()["settings"].toObject(),
                 (QJsonObject{{"lead_removes_own_pause", true}, {"phase_acceptance", "owner"}}));
    }
    auto set = team();
    set["settings"] = QJsonObject{{"lead_removes_own_pause", true}, {"phase_acceptance", "owner"}};
    TeamSettingsDialog dialog(&controller, parse(set), true);
    auto *unpause = dialog.findChild<QComboBox *>("teamSettingsLeadUnpause");
    auto *acceptance = dialog.findChild<QCheckBox *>("teamSettingsPhaseAcceptance");
    QCOMPARE(unpause->currentData().toBool(), true);
    QVERIFY(acceptance->isChecked());
    QVERIFY(dialog.changes().isEmpty());
    acceptance->setChecked(false);
    QCOMPARE(dialog.changes(), (QJsonObject{{"phase_acceptance", "lead"}}));
    unpause->setCurrentIndex(unpause->findData(false));
    QCOMPARE(dialog.changes(), (QJsonObject{{"phase_acceptance", "lead"}, {"lead_removes_own_pause", false}}));
}

// #581 N5: Landing (land_allowed) asks before it turns on, since a land stage's
// command may push; Cancel keeps it off, turning it off asks nothing. #581 N6:
// each member but the lead lists its models; Add and the chip's remove edit
// them, and Save sends the changed members in members_update with the
// changed settings in one `team settings` call.
void TestTeamsUiSettings::teamSettingsAllowLandingAndEditMemberModels()
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
void TestTeamsUiSettings::aboutNamesTheForkAndItsUpdate()
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

// #737: the team's dialogs draw the workspace theme on a system palette of
// the other one: Team settings, the plan-mode switch and the account move.
void TestTeamsUiSettings::teamDialogsDrawTheThemeNotTheSystem()
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

QTEST_MAIN(TestTeamsUiSettings)
#include "test_teamsui_settings.moc"
