// U62 (#712): Machines, accounts, phones and recovery across computers, from test_sessionswindow.
#include "SessionsWindowFixture.h"

class TestSessionsWindow : public SessionsWindowFixture {
    Q_OBJECT
private slots:
    void phonePairingDialogUsesWorkspaceTheme_data();
    void phonePairingDialogUsesWorkspaceTheme();
    void settingsPhonesPanelFollowsTheTheme();
    void machinesPageMatchesThePrototype_data();
    void machinesPageMatchesThePrototype();
    void mobileConnectionSettingsOfferTheSharedDeliveryChoice();
    void controlOnlyComputerOffersOnlyRemoteMachines();
    void enablableConsoleNamesBothWaysToStartSessions();
    void controlOnlyComputerKeepsRecoveryOnRemoteMachines();
    void controlOnlyComputerAloneExplainsRecovery();
    void enabledWindowsComputerOffersMobileGateway();
    void localWindowsSessionExplainsDisabledVoice();
    void sharedReadsQueueOnTheSessionHost();
    void accountUsageRejectsOtherSessionReplies();
    void machineChooserCountsFollowStateScopeWithoutMachineOrTextFilters();
    void accountFilterCombinesScopesAndKeepsTheOpenDraft();
    void accountFilterWarmsAcrossMachinesAndRefreshesOnOpening();
    void accountChooserMatchesAccountsAndKeepsRemovedProfileDetails();
    void accountDetailsResolveOnlyTheRecordedBinding();
    void accountScopeLimitsArchiveRemoval();
    void localWindowsFolderFilterIgnoresCase();
    void connectionStatusSurvivesNoticesAndRetries();
    void peerNamedDifferentlyListsSessionsUnderItsAlias();
    void remoteSessionLoadingIsDelayedAndFollowsSelection();
    void recoverySettingsValidateAndSaveForSelectedMachine_data();
    void recoverySettingsValidateAndSaveForSelectedMachine();
    void sharedRecoverySyncCatchesUpOfflinePeer();
};

void TestSessionsWindow::phonePairingDialogUsesWorkspaceTheme_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark"); QTest::newRow("light") << QString("light");
}

void TestSessionsWindow::phonePairingDialogUsesWorkspaceTheme()
{
    QFETCH(QString, theme); QSettings().setValue("workspace/theme", theme);
    SessionsWindow window(program()); window.setFleet(fleet()); window.show();
    PhonePairing::Dialog dialog(program(), {}, 0, &window); dialog.show();
    QTRY_COMPARE(dialog.palette().color(QPalette::Window), QColor(theme == "dark" ? "#161b21" : "#f5f7f9"));
    auto *code = dialog.findChild<QPlainTextEdit *>("phonePairingCodeText"); QVERIFY(code); code->ensurePolished();
    QTRY_COMPARE(code->palette().color(QPalette::Base), QColor(theme == "dark" ? "#1c2229" : "#ffffff"));
    QCOMPARE(code->palette().color(QPalette::Text), QColor(theme == "dark" ? "#e8edf4" : "#1a2733"));
}

// #650 N1: the Phones panel in Settings > Mobile connection draws its table,
// intro and group rows in the window's theme, and follows a theme change;
// it used to keep the light colours it set itself. Shots in both themes.
void TestSessionsWindow::settingsPhonesPanelFollowsTheTheme()
{
    QSettings().setValue("workspace/theme", "dark");
    const auto restoreTheme = qScopeGuard([] { QSettings().remove("workspace/theme"); });
    QTemporaryDir fixture; QVERIFY(fixture.isValid()); const auto fake = fixture.filePath("hgs");
    QVERIFY(FakeCli::install(fake, (QStringLiteral("#!/usr/bin/env python3\nimport json,runpy,sys,time\na=sys.argv[1:]\n"
        "host=a.pop(0)[1:] if a and a[0].startswith('@') else ''\n"
        "if a==['--help']:print('usage: hgs [@host] <cmd>\\n       hgs [@host] mobile phones|invite|revoke-phone <id> --json')\n"
        "elif a[:2]==['machine','ls']:print(json.dumps({'local':'workstation','machines':[{'alias':'studio','enabled':True}]}))\n"
        "elif a==['mobile','phones','--json']:\n"
        " now=time.time()\n"
        " print(json.dumps({'configured':True,'server_url':'https://relay.example.test','workspace_id':'w1' if host=='' else 'w2',"
        "'workspace_name':'Orbit' if host=='' else 'Lab','devices':[] if host else [{'id':'00000000-0000-4000-8000-0000000000a1','name':'Pixel 9','platform':'android',"
        "'paired_at':now-86400*40,'last_seen_at':now-600,'added_by':{'name':'workstation'}}]}))\n"
        "else:\n sys.argv[0]=") + FakeCli::pythonString(script()) + ";runpy.run_path(sys.argv[0],run_name='__main__')\n").toUtf8()));
    SessionsWindow window(FakeCli::program(fake)); window.setFleet(fleet()); window.resize(1260, 820); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    window.showUpdates();
    auto *sections = window.findChild<QListWidget *>("settingsSections"); QVERIFY(sections);
    for (int i = 0; i < sections->count(); ++i) if (sections->item(i)->text() == "Mobile connection") sections->setCurrentRow(i);
    auto *panel = dynamic_cast<PhonePairing::Panel *>(window.findChild<QWidget *>("settingsPhonesPanel")); QVERIFY(panel);
    QTRY_VERIFY(panel->isVisible());
    auto *tree = panel->findChild<QTreeWidget *>("linkedPhones");
    QTRY_COMPARE(tree->topLevelItemCount(), 2);
    const auto fg = [](bool dark) { return PageCards::token(dark, "fg").name(QColor::HexArgb); };
    const auto muted = [](bool dark) { return PageCards::token(dark, "muted").name(QColor::HexArgb); };
    const auto check = [&](bool dark) {
        QTRY_COMPARE(panel->dark(), dark);
        QVERIFY2(tree->styleSheet().contains("color:" + fg(dark)), qPrintable(tree->styleSheet()));
        QVERIFY(panel->findChild<QLabel *>("phonesIntro")->styleSheet().contains(muted(dark)));
        QCOMPARE(tree->topLevelItem(1)->background(0).color(), PageCards::token(dark, "hover"));
    };
    check(true);
    const auto save = [&](const QString &name) {
        const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
        if (shots.isEmpty()) return;
        // The page around it too: scrolled to the panel, the whole window.
        for (auto *parent = panel->parentWidget(); parent; parent = parent->parentWidget())
            if (auto *scroll = qobject_cast<QScrollArea *>(parent)) { scroll->ensureWidgetVisible(panel, 0, 0); break; }
        QDir().mkpath(shots); QTest::qWait(80);
        QVERIFY(window.grab().save(shots + "/" + name + ".png"));
    };
    save("u701-settings-phones-dark");
    // A theme change reaches it at once.
    auto *theme = window.findChild<QComboBox *>("workspaceTheme"); QVERIFY(theme);
    theme->setCurrentIndex(theme->findData("light"));
    check(false);
    save("u701-settings-phones-light");
    theme->setCurrentIndex(theme->findData("dark"));
    check(true);
}

// #647: the Machines page as the prototype draws it (Zerus-prototype.html,
// Machines), in the app's own styling and both themes: the list column with
// its title, Refresh, Add machine, the summary and each machine's monitor,
// state dot and session count; the header's label, system and state; this
// computer's Label line and This machine card with no empty SSH card under
// it; a remote machine's form; the flat Phones table with its workspace row
// and short dates, and an unreachable computer's error on a line of its own.
// Shots of each state to ZERUS_TEAM_SHOTS (pm-machines-*).
void TestSessionsWindow::machinesPageMatchesThePrototype_data()
{
    QTest::addColumn<QString>("theme");
    QTest::newRow("dark") << QString("dark"); QTest::newRow("light") << QString("light");
}

void TestSessionsWindow::machinesPageMatchesThePrototype()
{
    QFETCH(QString, theme); QSettings().setValue("workspace/theme", theme);
    const auto restoreTheme = qScopeGuard([] { QSettings().remove("workspace/theme"); });
    QTemporaryDir fixture; QVERIFY(fixture.isValid()); const auto fake = fixture.filePath("hgs");
    const auto now = QDateTime::currentDateTime();
    const auto at = [&](int days, int hour, int minute) { return QString::number(QDateTime(now.date().addDays(-days), QTime(hour, minute)).toSecsSinceEpoch()); };
    const QString phones = QStringLiteral(
        "{'configured':True,'server_url':'https://relay.example.test','workspace_id':'w1','workspace_name':'Orbit','devices':["
        "{'id':'00000000-0000-4000-8000-0000000000a1','name':'Anna\\u2019s iPhone','platform':'ios','paired_at':%1,'last_seen_at':%2,'added_by':{'name':'workstation'}},"
        "{'id':'00000000-0000-4000-8000-0000000000a2','name':'Pixel 9','platform':'android','paired_at':%3,'last_seen_at':%4,'added_by':{'name':'macbook'}}]}")
        .arg(at(18, 10, 0), at(0, 23, 2), at(57, 9, 30), at(6, 18, 40));
    QVERIFY(FakeCli::install(fake, (QStringLiteral("#!/usr/bin/env python3\nimport json,runpy,sys\na=sys.argv[1:]\n"
        "host=a.pop(0)[1:] if a and a[0].startswith('@') else ''\n"
        "if host=='studio':print('ssh: connect to host studio.example.test port 22: Connection timed out\\nhgs: studio unreachable',file=sys.stderr);sys.exit(255)\n"
        "if a==['--help']:print('usage: hgs [@host] <cmd>\\n       hgs [@host] mobile phones|invite|revoke-phone <id> --json')\n"
        "elif a[:2]==['machine','ls']:print(json.dumps({'local':'workstation','revision':'r1','machines':["
        "{'alias':'macbook','connection':{'hostname':'macbook.example','user':'user','port':22,'identity_file':'~/.ssh/id_ed25519','enabled':True},'enabled':True,'source':'local','inherited':False},"
        "{'alias':'build-01','connection':{'hostname':'build-01.example.test','enabled':True},'enabled':True,'source':'local','inherited':False},"
        "{'alias':'studio','connection':{'hostname':'studio.example.test','enabled':True},'enabled':True,'source':'local','inherited':False}]}))\n"
        "elif a[:2]==['machine','resolve']:print(json.dumps({'alias':a[2],'effective':{'hostname':a[2]+'.example','user':'user','port':'22','identity_files':['~/.ssh/id_ed25519']}}))\n"
        "elif a==['mobile','phones','--json']:print(json.dumps(%1 if host=='' else {'configured':False}))\n"
        "elif a[:2]==['mobile','invite']:print(json.dumps({'workspace_id':'w1','workspace_name':'Orbit','pair_code':'synthetic_pair-code_0123456789','encrypted':False,"
        "'expires_at':__import__('time').time()+600,'server_url':'https://relay.example.test',"
        "'pair_url':'zerus://pair?server=https%3A%2F%2Frelay.example.test&code=synthetic_pair-code_0123456789'}))\n"
        "elif 'account' in a[:2]:print(json.dumps({'profiles':[]}))\n"
        "else:\n sys.argv[0]=").arg(phones) + FakeCli::pythonString(script()) + ";runpy.run_path(sys.argv[0],run_name='__main__')\n").toUtf8()));
    // A Windows workstation with six sessions, two connected machines and one that does not answer.
    const auto ms = QDateTime::currentMSecsSinceEpoch();
    auto base = fleet(); const auto sessions = base.local().sessions;
    BoxState local = base.local(); local.host = "workstation"; local.localSessions = true;
    QList<SessionInfo> six;
    for (int i = 0; i < 6; ++i) { auto s = sessions[i % sessions.size()]; s.name += QStringLiteral("-%1").arg(i); s.tag += QString::number(i); six << s; }
    local.sessions = six;
    local.metrics = QJsonObject{{"platform", "windows"}, {"sampled_at", ms / 1000.}};
    local.localHost = QJsonObject{{"state", "running"}, {"pid", 18244}, {"protocol", 1}, {"conpty", "bundled"}, {"integrity", "medium"}, {"windows_session", 1}};
    FleetState state; state.setLocal(local, ms);
    BoxState mac; mac.host = "macbook"; mac.ok = true; mac.sessions = sessions.mid(0, 2); mac.hgsVersion = "0.47.0"; mac.metrics = QJsonObject{{"platform", "macos"}};
    BoxState build; build.host = "build-01"; build.ok = true; build.sessions = sessions.mid(0, 3); build.metrics = QJsonObject{{"platform", "linux"}};
    BoxState studio; studio.host = "studio"; studio.ok = false; studio.error = "offline";
    state.setPeer(mac, ms); state.setPeer(build, ms); state.setPeer(studio, ms);
    SessionsWindow window(FakeCli::program(fake)); window.resize(1260, 760); window.setFleet(state); window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));
    for (auto *button : window.findChildren<QPushButton *>("railButton")) if (button->toolTip().startsWith("Machines")) button->click();
    auto *page = window.findChild<MachinesPage *>(); QVERIFY(page); QTRY_VERIFY(page->isVisible());
    auto *list = page->findChild<QListWidget *>("machineProfiles");
    QTRY_COMPARE(list->count(), 4);
    // The list: this machine with its sessions, each machine's state.
    QCOMPARE(list->item(0)->text(), QStringLiteral("workstation\nThis machine · 6 sessions"));
    QCOMPARE(list->item(1)->text(), QStringLiteral("macbook\nConnected, 2 sessions"));
    QCOMPARE(list->item(3)->text(), QStringLiteral("studio\nNot connected"));
    QCOMPARE(list->item(0)->data(Qt::UserRole + 2).toString(), QStringLiteral("ok"));
    QCOMPARE(list->item(3)->data(Qt::UserRole + 2).toString(), QStringLiteral("off"));
    QStringList summary; for (auto *label : page->findChildren<QLabel *>("muted")) summary << label->text();
    QVERIFY2(summary.contains(QStringLiteral("SSH connections and HGS setup · 3 connected, 4 machines")), qPrintable(summary.join(" | ")));
    // This computer: its label and system, its session host, the Label card's
    // line, the This machine card with Test session host, and no SSH card.
    QCOMPARE(page->findChild<QLabel *>("machineTitle")->text(), QStringLiteral("workstation"));
    QVERIFY(page->findChild<QLabel *>("machineSystem")->isVisible());
    QVERIFY(page->findChild<QLabel *>("localSessionHost")->text().startsWith("Session host: running, PID 18244"));
    QVERIFY(page->findChild<QLabel *>("machineLocalConnection")->isVisible());
    QVERIFY(page->findChild<QFrame *>("machineLocalCard")->isVisible());
    QVERIFY(page->findChild<QPushButton *>("testSessionHost")->isVisible());
    QVERIFY2(!page->findChild<QFrame *>("machineConnectionCard")->isVisible(), "no empty card under the Label card");
    // Phones: the count beside the title, the workspace row, short dates and
    // the unreachable computer's own line.
    auto *table = page->findChild<QTreeWidget *>("linkedPhones");
    QTRY_COMPARE(table->topLevelItemCount(), 1);
    QCOMPARE(table->topLevelItem(0)->text(0), QStringLiteral("Orbit · this computer"));
    QCOMPARE(table->topLevelItem(0)->childCount(), 2);
    QCOMPARE(table->topLevelItem(0)->child(0)->text(3), QStringLiteral("Today 23:02"));
    QCOMPARE(page->findChild<QLabel *>("phonesCount")->text(), QStringLiteral("2 phones"));
    QTRY_COMPARE(page->findChildren<QLabel *>("phonesProblem").size(), 1);
    QVERIFY(page->findChild<QLabel *>("phonesProblem")->text().startsWith("studio: ssh: connect to host studio.example.test"));
    QVERIFY(!page->findChild<QLabel *>("phonesProblem")->text().contains('\n'));
    QVERIFY(table->height() < 160);
    const auto shots = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    const auto save = [&](const QString &name) {
        if (shots.isEmpty()) return;
        QDir().mkpath(shots); QTest::qWait(60);
        QVERIFY(window.grab().save(shots + "/" + name + "-" + theme + ".png"));
    };
    save("pm-machines-local");
    // #651 QA: the pair and revoke dialogs, shot in the page's theme; Revoke
    // is cancelled, so nothing is revoked.
    {
        bool paired = false, asked = false;
        QTimer closer; closer.setInterval(30);
        QObject::connect(&closer, &QTimer::timeout, &window, [&] {
            for (auto *widget : QApplication::topLevelWidgets()) {
                if (auto *dialog = dynamic_cast<PhonePairing::Dialog *>(widget); dialog && dialog->isVisible()) {
                    if (dialog->findChild<QLabel *>("phonePairingStatus")->text().startsWith("Requesting")) continue;
                    if (!shots.isEmpty()) { QDir().mkpath(shots); dialog->grab().save(shots + "/pm-phone-pair-" + theme + ".png"); }
                    paired = true; dialog->reject();
                } else if (auto *box = qobject_cast<QMessageBox *>(widget); box && box->isVisible() && box->windowTitle().startsWith("Revoke")) {
                    if (!shots.isEmpty()) { QDir().mkpath(shots); box->grab().save(shots + "/pm-phone-revoke-" + theme + ".png"); }
                    asked = true; box->button(QMessageBox::Cancel)->click();
                }
            }
        });
        closer.start();
        auto *pair = page->findChild<QPushButton *>("pairPhone");
        QTRY_VERIFY(pair->isEnabled());
        QTimer::singleShot(0, pair, [pair] { pair->click(); });
        QTRY_VERIFY(paired);
        QTRY_COMPARE(page->findChild<QTreeWidget *>("linkedPhones")->topLevelItemCount(), 1);
        table = page->findChild<QTreeWidget *>("linkedPhones");
        table->setCurrentItem(table->topLevelItem(0)->child(1));
        auto *revoke = page->findChild<QPushButton *>("revokePhone");
        QTRY_VERIFY(revoke->isEnabled());
        QTimer::singleShot(0, revoke, [revoke] { revoke->click(); });
        QTRY_VERIFY(asked);
        closer.stop();
        QTRY_COMPARE(table->topLevelItem(0)->childCount(), 2);   // nothing revoked
    }
    // A remote machine: its system and state, and the prototype's form.
    list->setCurrentRow(1);
    QTRY_COMPARE(page->findChild<QLabel *>("machineTitle")->text(), QStringLiteral("macbook"));
    QCOMPARE(page->findChild<QLabel *>("machineSystem")->text(), QStringLiteral("macOS"));
    QCOMPARE(page->findChild<QLabel *>("machineState")->text(), QStringLiteral("Connected · hgs 0.47.0"));
    QVERIFY(page->findChild<QFrame *>("machineConnectionCard")->isVisible());
    QVERIFY(!page->findChild<QFrame *>("machineLocalCard")->isVisible());
    QTRY_VERIFY(page->findChild<QLabel *>("effectiveMachineConnection")->text().contains("user@macbook.example, port 22"));
    for (const char *name : {"saveMachine", "discardNewMachine", "checkMachine", "sshMachine", "installMachine", "resetMachine", "removeMachine", "browseMachineKey"})
        QVERIFY2(page->findChild<QPushButton *>(name)->isVisible(), name);
    // #650 N2: a saved SSH machine can change its connection type.
    QVERIFY(page->findChild<QComboBox *>("machineTransport")->isVisible());
    save("pm-machines-remote");
    // The machine that does not answer; #651 QA: the shot waits for its
    // effective connection in both themes.
    list->setCurrentRow(3);
    QTRY_COMPARE(page->findChild<QLabel *>("machineState")->text(), QStringLiteral("Not connected"));
    QTRY_VERIFY(page->findChild<QLabel *>("effectiveMachineConnection")->text().contains("user@studio.example, port 22"));
    save("pm-machines-unreachable");
}

void TestSessionsWindow::mobileConnectionSettingsOfferTheSharedDeliveryChoice(){
    SessionsWindow window(program()); window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *choice=window.findChild<QComboBox *>("mobileDelivery"); QVERIFY(choice); QCOMPARE(choice->count(),6);
#ifdef Q_OS_WIN
    QVERIFY(window.findChild<QWidget *>("windowsMobileSettings"));
#else
    QVERIFY(window.findChild<QWidget *>("relaySettings"));
#endif
}

// hgs.exe on Windows reports a computer without sessions of its own; the
// workspace then manages remote machines only (driven by data, not by #ifdef).
void TestSessionsWindow::controlOnlyComputerOffersOnlyRemoteMachines(){
    auto state=fleet();auto local=state.local();local.sessions.clear();local.localSessions=false;local.metrics={{"platform","linux"}};
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.resize(1100,760);window.setFleet(state);window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *sections=window.findChild<QListWidget *>("settingsSections");QVERIFY(sections);
    // Create/Join must be reachable before this computer has a local gateway.
    QVERIFY(!sections->item(5)->isHidden());QVERIFY(!sections->item(2)->isHidden());QVERIFY(!sections->item(4)->isHidden());
    auto *enrollment=window.findChild<QComboBox *>("workspaceEnrollmentMode");QVERIFY(enrollment);
    QVERIFY(enrollment->findData("create")>=0);QVERIFY(enrollment->findData("join")>=0);
    auto linuxFleet=fleet();auto linuxBox=linuxFleet.local();linuxBox.metrics=QJsonObject{{"platform","linux"}};
    linuxFleet.setLocal(linuxBox,QDateTime::currentMSecsSinceEpoch());
    window.setFleet(linuxFleet);QVERIFY(!sections->item(5)->isHidden());window.setFleet(state);
    NewSessionDialog launch(program(),state,{},"codex");launch.show();
    auto *machine=launch.findChild<QComboBox *>("launchComputer");QVERIFY(machine);
    QCOMPARE(machine->count(),1);QCOMPARE(machine->itemData(0).toString(),QString("mac"));
    FleetState alone;alone.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    NewSessionDialog lonely(program(),alone,{},"codex");lonely.show();
    QCOMPARE(lonely.findChild<QComboBox *>("launchComputer")->count(),0);
    QTRY_COMPARE(lonely.findChild<QLabel *>("launchError")->text(),QString("Add a remote machine in Machines to start sessions from this computer."));
    QVERIFY(!lonely.findChild<QPushButton *>("primary")->isEnabled());
    QVERIFY(!lonely.findChild<QPushButton *>("browseLaunchFolder")->isEnabled());
    QVERIFY(lonely.findChild<QPushButton *>("launchOpenMachines")->isVisible());
    QVERIFY(!launch.findChild<QPushButton *>("launchOpenMachines")->isVisible());
}

// Without remote machines a console that can enable local sessions names both
// ways, and New session only opens Machines with this computer selected.
void TestSessionsWindow::enablableConsoleNamesBothWaysToStartSessions(){
    const auto lonely=enablableConsole();
    SessionsWindow window(program());window.resize(1100,760);window.setFleet(lonely);window.show();
    bool checked=false;
    QTimer::singleShot(0,&window,[&]{
        auto *dialog=qobject_cast<NewSessionDialog *>(QApplication::activeModalWidget());QVERIFY(dialog);
        QTimer::singleShot(5000,dialog,&QDialog::reject);
        QCOMPARE(dialog->findChild<QComboBox *>("launchComputer")->count(),0);
        auto *error=dialog->findChild<QLabel *>("launchError");
        QTRY_COMPARE(error->text(),QString("Enable local sessions for this computer in Machines, or add a remote machine."));
        // The hint follows each poll: without a session host only a remote machine helps.
        auto plain=lonely;auto local=plain.local();local.localHost={};plain.setLocal(local,QDateTime::currentMSecsSinceEpoch());
        dialog->setFleet(plain);QCOMPARE(error->text(),QString("Add a remote machine in Machines to start sessions from this computer."));
        dialog->setFleet(lonely);QCOMPARE(error->text(),QString("Enable local sessions for this computer in Machines, or add a remote machine."));
        auto *open=dialog->findChild<QPushButton *>("launchOpenMachines");QVERIFY(open->isVisible());QVERIFY(open->isEnabled());
        if(const auto preview=qEnvironmentVariable("HGS_CONTROL_PREVIEW_DIR");!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(dialog->grab().save(preview+"/launch-enable.png"));}
        checked=true;open->click();
    });
    window.findChild<QPushButton *>("newSession")->click();QVERIFY(checked);
    QVERIFY(!QApplication::activeModalWidget());
    auto *machines=window.findChild<QWidget *>("machinesPage");QTRY_VERIFY(machines->isVisible());
    QTRY_COMPARE(machines->findChild<QListWidget *>("machineProfiles")->currentItem()->data(Qt::UserRole).toString(),QString("@local"));
    // Only navigation: enabling stays behind its confirmation on the page.
    auto *enable=machines->findChild<QPushButton *>("enableLocalSessions");QVERIFY(enable->isVisible());
    QVERIFY(!machines->findChild<QMessageBox *>("enableLocalSessionsConfirm"));
}

void TestSessionsWindow::controlOnlyComputerKeepsRecoveryOnRemoteMachines(){
    QTemporaryDir fixture;QVERIFY(fixture.isValid());const auto script=fixture.filePath("hgs");
    QVERIFY(FakeCli::install(script,R"PY(#!/usr/bin/env python3
import json,sys
a=sys.argv[1:]
open(__file__+'.calls','a').write(json.dumps(a)+'\n')
if a[:2]!=['@mac','recovery']:sys.exit(2)
if a[2:3]==['set']:open(__file__+'.saved','wb').write(sys.stdin.buffer.read())
print('{"order":[7,"writer"],"policy":{"version":2,"revision":7,"writer":"writer","enabled":false,"enabled_at":0,"service":true,"network":true,"rate_limit":true,"delays":[15,30,60,300]}}')
)PY"));
    QSettings().remove("recovery/sharedPolicy");
    auto state=fleet();auto local=state.local();local.sessions.clear();local.localSessions=false;
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(script));window.resize(1100,900);window.setFleet(state);window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();auto &page=*static_cast<SettingsPage *>(window.findChild<QWidget *>("settingsPage"));page.openRecovery();
    auto *status=window.findChild<QLabel *>("recoverySyncStatus");QVERIFY(status);
    QTRY_VERIFY_WITH_TIMEOUT(status->text().startsWith("Synchronized with 1 machine."),20000);
    auto *save=window.findChild<QPushButton *>("recoverySave");QTRY_VERIFY(save->isEnabled());
    window.findChild<QCheckBox *>("recoveryEnabled")->setChecked(true);save->click();
    QTRY_VERIFY_WITH_TIMEOUT(QFile::exists(script+".saved"),20000);
    // Recovery is never read from or written to this computer.
    QFile calls(script+".calls");QVERIFY(calls.open(QIODevice::ReadOnly));int recovery=0;
    for(const auto &line:calls.readAll().split('\n')){
        const auto call=QJsonDocument::fromJson(line).array();if(!call.contains(QJsonValue("recovery")))continue;
        ++recovery;QCOMPARE(call.first().toString(),QString("@mac"));
    }
    QVERIFY(recovery>=2);
}

void TestSessionsWindow::controlOnlyComputerAloneExplainsRecovery(){
    // A policy cached while remote machines were configured; none is left now.
    setJsonSetting("recovery/sharedPolicy",QByteArray(R"({"order":[7,"writer"],"policy":{"version":2,"revision":7,"writer":"writer","enabled":true,"enabled_at":0,"service":true,"network":true,"rate_limit":true,"delays":[15,30,60,300]}})"));
    const auto cleanup=qScopeGuard([]{QSettings().remove("recovery/sharedPolicy");});
    FleetState alone;BoxState local;local.host="windows-pc";local.ok=true;local.localSessions=false;alone.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.resize(1100,900);window.setFleet(alone);window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();static_cast<SettingsPage *>(window.findChild<QWidget *>("settingsPage"))->openRecovery();
    auto *status=window.findChild<QLabel *>("recoverySyncStatus");QVERIFY(status);
    QTRY_COMPARE(status->text(),QString("Add a remote machine in Machines to configure automatic recovery; this computer runs no agents."));
    QVERIFY(!status->text().contains("Synchronized"));
    QVERIFY(!window.findChild<QPushButton *>("recoverySave")->isEnabled());
    QVERIFY(!window.findChild<QCheckBox *>("recoveryEnabled")->isEnabled());
    QVERIFY(!window.findChild<QCheckBox *>("recoveryRule_service")->isEnabled());
    // Where Machines offers Enable local sessions, the status names it too.
    window.setFleet(enablableConsole());
    QTRY_COMPARE(status->text(),QString("To configure automatic recovery, enable local sessions for this computer in Machines or add a remote machine."));
    QVERIFY(!window.findChild<QPushButton *>("recoverySave")->isEnabled());
    window.setFleet(alone);QTRY_VERIFY(status->text().startsWith("Add a remote machine in Machines"));
}

// Windows retains recovery and exposes its real local mobile gateway configuration.
void TestSessionsWindow::enabledWindowsComputerOffersMobileGateway(){
    QTemporaryDir fixture;QVERIFY(fixture.isValid());const auto script=fixture.filePath("hgs");
    QVERIFY(FakeCli::install(script,R"PY(#!/usr/bin/env python3
import sys
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
if a[:1]!=['recovery']:sys.exit(2)
print('{"order":[7,"writer"],"policy":{"version":2,"revision":7,"writer":"writer","enabled":false,"enabled_at":0,"service":true,"network":true,"rate_limit":true,"delays":[15,30,60,300]}}')
)PY"));
    QSettings().remove("recovery/sharedPolicy");
    auto state=fleet();auto local=state.local();local.metrics=QJsonObject{{"platform","windows"}};
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(script));window.resize(1100,900);window.setFleet(state);window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();
    auto *sections=window.findChild<QListWidget *>("settingsSections");QVERIFY(sections);
    QVERIFY(!sections->item(5)->isHidden());QVERIFY(!sections->item(2)->isHidden());QVERIFY(!sections->item(4)->isHidden());
    // Recovery includes this computer as well as the remote machine.
    static_cast<SettingsPage *>(window.findChild<QWidget *>("settingsPage"))->openRecovery();
    auto *status=window.findChild<QLabel *>("recoverySyncStatus");QVERIFY(status);
    QTRY_VERIFY_WITH_TIMEOUT(status->text().startsWith("Synchronized across 2 machines."),20000);
    local.metrics=QJsonObject{{"platform","linux"}};state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    window.setFleet(state);QVERIFY(!sections->item(5)->isHidden());
    local.metrics=QJsonObject{{"platform","windows"}};state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    sections->setCurrentRow(5);window.setFleet(state);QVERIFY(!sections->item(5)->isHidden());QCOMPARE(sections->currentRow(),5);
    local.localSessions=false;state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);QVERIFY(!sections->item(5)->isHidden());
}

// Voice sends to local Windows sessions are a Phase 2 non-goal (spec D16): the
// disabled voice button says why.
void TestSessionsWindow::localWindowsSessionExplainsDisabledVoice(){
    const QJsonObject details{{"tracked",true},{"run_id","fixture-run"},{"conversation_id","fixture-conversation"},
        {"runtime_state","live"},{"process_state","running"},{"phase","idle"},{"activity","idle"},
        {"events",QJsonArray{}},{"cursor",0}};
    const auto fake=programInspecting("windows-voice",details);QVERIFY(!fake.isEmpty());
    auto state=fleet();auto local=state.local();local.metrics=QJsonObject{{"platform","linux"}};
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(fake);window.setFleet(state);window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<HgsClient *>();auto *composer=window.findChild<MessageComposer *>("messageComposer");
    QTRY_VERIFY(!composer->isDraftLoading(composer->sessionKey()));
    settleSelectedInspection(composer,client,{},"codex/hgs/dashboard",details);
    auto *voice=composer->findChild<ToolbarChip *>("voiceRecord");QVERIFY(voice);
    QTRY_VERIFY(voice->isEnabled());QVERIFY(voice->toolTip().startsWith("Click to start"));
    local.metrics=QJsonObject{{"platform","windows"}};state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QTRY_VERIFY(!voice->isEnabled());
    QCOMPARE(voice->toolTip(),QString("Voice messages cannot be sent to sessions on this Windows computer yet."));
}

void TestSessionsWindow::sharedReadsQueueOnTheSessionHost()
{
    QSettings().remove("attention/pendingReads");
    auto state = fleet(); auto box = state.local();
    box.sessions[0].runId = "run-local"; box.sessions[0].conversationId = "shared-local";
    box.sessions[0].replyId = "10:100"; box.sessions[0].sharedRead = true;
    state.setLocal(box, QDateTime::currentMSecsSinceEpoch());
    auto remote = *state.peer("mac");
    remote.sessions[0].runId = "run-remote"; remote.sessions[0].conversationId = "shared-remote";
    remote.sessions[0].replyId = "20:200"; remote.sessions[0].sharedRead = true;
    state.setPeer(remote, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *action = window.findChild<QAction *>("markAllSessionsRead"); QVERIFY(action); QVERIFY(action->isEnabled());
    action->trigger();
    const auto pending = ReadMarkStore::load();
    const auto local = ReadMark::fromJson(pending.value(ReadMark{{}, box.sessions[0].name, {}, "shared-local", {}, {}}.key()).toObject());
    QCOMPARE(local.runId, QString("run-local")); QCOMPARE(local.replyId, QString("10:100"));
    const auto peer = ReadMark::fromJson(pending.value(ReadMark{"mac", remote.sessions[0].name, {}, "shared-remote", {}, {}}.key()).toObject());
    QCOMPARE(peer.runId, QString("run-remote")); QCOMPARE(peer.replyId, QString("20:200"));
    QSettings().remove("attention/pendingReads");
}

void TestSessionsWindow::accountUsageRejectsOtherSessionReplies()
{
    SessionsWindow window(program());window.setFleet(fleet());window.show();window.showSession({},"codex/hgs/dashboard");
    auto *client=window.findChild<AccountUsageStore *>()->findChild<HgsClient *>();auto *usage=window.findChild<QPushButton *>("sessionAccountUsage");
    const QJsonObject first{{"id","native-codex"},{"identity",QJsonObject{{"email","first@example.test"}}},{"status","ok"}};
    auto *refresh=static_cast<AccountUsage::RefreshButton *>(window.findChild<QPushButton *>("sessionUsageRefresh"));
    QVERIFY(refresh->isRefreshing());QVERIFY(!refresh->isEnabled());
    client->accountsReady(1,{},first);QVERIFY(usage->toolTip().contains("first@example.test"));QVERIFY(!refresh->isRefreshing());QVERIFY(refresh->isEnabled());
    window.showSession("mac","claude/infra/review");QVERIFY(!usage->toolTip().contains("first@example.test"));
    client->accountsReady(1,{},first);QVERIFY(!usage->toolTip().contains("first@example.test"));
    const QJsonObject second{{"id","native-claude"},{"identity",QJsonObject{{"email","second@example.test"}}},{"status","ok"}};
    client->accountsReady(2,"mac",second);QVERIFY(usage->toolTip().contains("second@example.test"));
    refresh->click();QVERIFY(refresh->isRefreshing());QVERIFY(!refresh->isEnabled());QVERIFY(usage->toolTip().contains("second@example.test"));
    client->accountsFailed(3,"mac","offline");QVERIFY(!refresh->isRefreshing());QVERIFY(refresh->isEnabled());QVERIFY(usage->toolTip().contains("second@example.test"));
}

void TestSessionsWindow::machineChooserCountsFollowStateScopeWithoutMachineOrTextFilters()
{
    auto state=fleet(); auto local=state.local();
    auto archive=local.sessions[0]; archive.state="archived"; archive.archiveId="archive-one";
    local.sessions.append(archive); state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    BoxState empty; empty.host="empty"; empty.ok=true; state.setPeer(empty,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); keepDetailsPollsOut(window); window.setFleet(state); window.show();
    auto *filter=window.findChild<MachineFilter *>("machineFilterChips"); auto *menu=filter->button()->menu();
    menu->popup(window.mapToGlobal(QPoint(100,100))); QTRY_VERIFY(menu->isVisible());
    const auto count=[&](const QString &host) {
        for (auto *choice : menu->findChildren<QCheckBox *>("machineFilterChoice"))
            if (choice->property("host")==host) return choice->property("sessionCount").toInt();
        return -1;
    };
    QCOMPARE(count("@local"),4); QCOMPARE(count("mac"),1); QCOMPARE(count("empty"),0);
    filter->setSelection({"@local"}); QCOMPARE(count("mac"),1); // Unselected machines retain their counts.
    menu->hide();
    window.findChild<DashboardPage *>()->filterRequested("@all","paused");
    menu->popup(window.mapToGlobal(QPoint(100,100))); QCOMPARE(count("@local"),1); QCOMPARE(count("mac"),0);
    menu->hide(); window.findChild<DashboardPage *>()->filterRequested("@all","archived");
    menu->popup(window.mapToGlobal(QPoint(100,100))); QCOMPARE(count("@local"),1); QCOMPARE(count("mac"),0);
    menu->hide(); window.findChild<DashboardPage *>()->filterRequested("@all","all");
    window.findChild<QLineEdit *>("search")->setText("does-not-match-any-session");
    menu->popup(window.mapToGlobal(QPoint(100,100))); QCOMPARE(count("@local"),4); QCOMPARE(count("mac"),1);
    local.sessions.removeFirst(); state.setLocal(local,QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QCOMPARE(count("@local"),3); QCOMPARE(count("mac"),1); QVERIFY(menu->isVisible()); menu->hide();
}

void TestSessionsWindow::accountFilterCombinesScopesAndKeepsTheOpenDraft()
{
    auto state=fleet();auto local=state.local();local.sessions[0].accountId="work";local.sessions[2].accountId="personal";
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    auto remote=*state.peer("mac");remote.sessions[0].cmd="codex";remote.sessions[0].accountId="work";
    state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionOrganization org;const auto project=org.createGroup("Focus");
    org.moveSession(org.observe("arch",local.sessions[0].name,""),project);
    org.moveSession(org.observe("mac",remote.sessions[0].name,""),project);
    setJsonSetting("workspace/organization",QJsonDocument(org.toJson()).toJson(QJsonDocument::Compact));
    SessionsWindow window(program());keepDetailsPollsOut(window);window.setFleet(state);window.resize(1100,760);window.show();
    auto *accounts=window.findChild<AccountFilter *>("accountFilterChips");auto *projects=window.findChild<ProjectFilter *>("projectFilterChips");
    auto *machines=window.findChild<MachineFilter *>("machineFilterChips");auto *list=window.findChild<SessionList *>("sessionList");QVERIFY(accounts&&projects&&machines&&list);
    const auto count=[&]{int n=0;for(int i=0;i<list->count();++i)if(!list->item(i)->data(SessionRoles::Header).toBool())++n;return n;};
    const auto work=SessionAccounts::binding({},local.sessions[0]),personal=SessionAccounts::binding({},local.sessions[2]),mac=SessionAccounts::binding("mac",remote.sessions[0]);
    accounts->setSelection({work});QCOMPARE(count(),1);
    for(auto *button:window.findChildren<QPushButton *>("sessionFilter"))if(button->property("filter")=="all")QCOMPARE(button->property("count").toInt(),1);
    accounts->setSelection({work,personal,mac});QCOMPARE(count(),3);
    projects->setSelection({project});QCOMPARE(count(),2);machines->setSelection({"mac"});QCOMPARE(count(),1);
    machines->setSelection({});projects->setSelection({});accounts->setSelection({work});
    auto *search=window.findChild<QLineEdit *>("search");auto *results=window.findChild<QListWidget *>("searchResults");
    search->setText("navigation");QCOMPARE(results->count(),0);accounts->setSelection({personal});QCOMPARE(results->count(),1);search->clear();
    window.showSession({},local.sessions[0].name);auto *composer=window.findChild<MessageComposer *>("messageComposer");
    setComposerText(composer,"Keep this account-filter draft");const auto open=composer->sessionKey();
    accounts->setSelection({personal});QCOMPARE(count(),1);QCOMPARE(composer->sessionKey(),open);
    QCOMPARE(composer->editor()->toPlainText(),QString("Keep this account-filter draft"));
    window.showProjects();window.showSessionList();QCOMPARE(accounts->selection(),QSet<QString>{personal});QCOMPARE(count(),1);
    local.sessions[2].name="codex/website/renamed";state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QCOMPARE(accounts->selection(),QSet<QString>{personal});QCOMPARE(count(),1);
    local.sessions.removeAt(2);state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    QCOMPARE(accounts->selection(),QSet<QString>{personal});QCOMPARE(count(),0); // Disappearance must not broaden the scope.
}

void TestSessionsWindow::accountFilterWarmsAcrossMachinesAndRefreshesOnOpening()
{
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject profile{{"id","shared-profile"},{"provider","codex"},{"label","Shared"},{"home","/fixture/shared"},{"installed",true}};
    const auto catalog=QString::fromUtf8(QJsonDocument(QJsonObject{{"profiles",QJsonArray{profile}}}).toJson(QJsonDocument::Compact));
    const auto usage=QString::fromUtf8(QJsonDocument(QJsonObject{{"id","shared-profile"},{"provider","codex"},{"home","/fixture/shared"},{"status","ok"},
        {"identity",QJsonObject{{"account_id","shared-account"},{"email","shared@example.test"}}}}).toJson(QJsonDocument::Compact));
    const auto fixture=m_dir->filePath("filter-warm");
    const auto answer=QByteArray(R"PY(elif c=='account' and a[1:2]==['ls']:
 from pathlib import Path
 import time
 data=json.loads(CATALOG)
 label=Path(__file__+'.label')
 if label.exists():data['profiles'][0]['label']=label.read_text()
 host=sys.argv[1][1:] if sys.argv[1].startswith('@') else 'local'
 with Path(__file__+'.reads').open('a') as reads:reads.write(json.dumps([host,data['profiles'][0]['label']])+'\n')
 hold=Path(__file__+'.hold-'+host)
 if hold.exists():
  ready=Path(__file__+'.ready-'+host)
  staged=Path(str(ready)+'.tmp');staged.write_text(json.dumps(data));staged.replace(ready)
  while hold.exists():time.sleep(.01)
 print(json.dumps(data))
elif c=='account' and a[1:2]==['inspect']:
 from pathlib import Path
 import time
 deadline=time.monotonic()+30
 while Path(__file__+'.identity-hold').exists():
  if time.monotonic()>=deadline:raise RuntimeError('Identity fixture gate expired')
  time.sleep(.01)
 print(USAGE)
)PY").replace("CATALOG",FakeCli::pythonString(catalog)).replace("USAGE",FakeCli::pythonString(usage));
    QVERIFY(FakeCli::install(fixture,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto local=state.local();local.sessions[0].accountId="shared-profile";local.sessions[0].accountHome="/fixture/shared";
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());auto remote=*state.peer("mac");remote.sessions[0].cmd="codex";remote.sessions[0].accountId="shared-profile";remote.sessions[0].accountHome="/fixture/shared";state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    QFile identityHold(fixture+".identity-hold");QVERIFY(identityHold.open(QIODevice::WriteOnly));identityHold.close();
    SessionsWindow window(FakeCli::program(fixture));keepDetailsPollsOut(window);window.setFleet(state);window.show();
    auto *page=window.findChild<AccountsPage *>();QVERIFY(page);QVERIFY(!page->isVisible());
    // Select the recorded bindings before asynchronous identity publication so
    // the public chips can show when Sessions has actually grouped them.
    auto *startupFilter=window.findChild<AccountFilter *>("accountFilterChips");QVERIFY(startupFilter);
    const QSet<QString> startupBindings{SessionAccounts::binding({},local.sessions[0]),SessionAccounts::binding("mac",remote.sessions[0])};
    QVERIFY(startupFilter->selection().isEmpty());startupFilter->setSelection(startupBindings);
    const auto heldPresentationReady=[&]{const auto chips=startupFilter->findChildren<QPushButton *>("accountFilterChip");return chips.size()==1 && chips.first()->toolTip().contains("shared@example.test");};
    QVERIFY(!heldPresentationReady());QVERIFY(QFile::remove(fixture+".identity-hold"));
    const auto identitiesReady=[&]{const auto profiles=page->profiles();return profiles.size()==2 && std::all_of(profiles.cbegin(),profiles.cend(),[](const QJsonValue &v){return v.toObject()["usage"].toObject()["identity"].toObject()["account_id"]=="shared-account";});};
    QTRY_VERIFY(identitiesReady());
    const auto startupPresentationReady=[&]{const auto chips=startupFilter->findChildren<QPushButton *>("accountFilterChip");return chips.size()==1 && chips.first()->toolTip().contains("shared@example.test");};
    QTRY_VERIFY(startupPresentationReady());QCOMPARE(startupFilter->selection(),startupBindings);
    startupFilter->setSelection({});QVERIFY(startupFilter->selection().isEmpty());QVERIFY(!page->isVisible());
    auto *filter=window.findChild<AccountFilter *>("accountFilterChips");auto *menu=filter->button()->menu();
    const QSet<QString> bindings{SessionAccounts::binding({},local.sessions[0]),SessionAccounts::binding("mac",remote.sessions[0])};
    const auto sharedChoice=[&]() -> QCheckBox * {for(auto *choice:menu->findChildren<QCheckBox *>("accountFilterChoice")){const auto members=choice->property("bindings").toStringList();if(QSet<QString>(members.cbegin(),members.cend())==bindings)return choice;}return nullptr;};
    const QStringList hosts{"local","mac"};
    for(const auto &host:hosts){QFile hold(fixture+".hold-"+host);QVERIFY(hold.open(QIODevice::WriteOnly));}
    QElapsedTimer refreshWindow;refreshWindow.start();
    menu->popup(filter->button()->mapToGlobal(QPoint(0,30)));QTRY_VERIFY(menu->isVisible());QTRY_VERIFY(sharedChoice());
    // The first open has already captured an old snapshot on BOTH machines,
    // but neither read can finish before the next explicit open requests it.
    for(const auto &host:hosts){
        const auto ready=fixture+".ready-"+host;QTRY_VERIFY(QFile::exists(ready));
        QFile snapshot(ready);QVERIFY(snapshot.open(QIODevice::ReadOnly));
        QCOMPARE(QJsonDocument::fromJson(snapshot.readAll()).object()["profiles"].toArray().first().toObject()["label"].toString(),QString("Shared"));
    }
    QVERIFY(sharedChoice()->text().contains("shared@example.test"));sharedChoice()->click();QCOMPARE(filter->selection(),bindings);menu->hide();
    auto *list=window.findChild<SessionList *>("sessionList");const auto count=[&]{int n=0;for(int i=0;i<list->count();++i)if(!list->item(i)->data(SessionRoles::Header).toBool())++n;return n;};
    QCOMPARE(count(),2);auto *machines=window.findChild<MachineFilter *>("machineFilterChips");machines->setSelection({"mac"});QCOMPARE(count(),1);QCOMPARE(filter->selection(),bindings);
    QFile label(fixture+".label");QVERIFY(label.open(QIODevice::WriteOnly));label.write("Renamed shared account");label.close();
    menu->popup(filter->button()->mapToGlobal(QPoint(0,30)));QTRY_VERIFY(menu->isVisible());
    // Multiple opens while the same reads are pending must coalesce once per
    // machine. Releasing the old result must honor that explicit fresh read.
    menu->hide();menu->popup(filter->button()->mapToGlobal(QPoint(0,30)));QTRY_VERIFY(menu->isVisible());
    for(const auto &host:hosts)QVERIFY(QFile::remove(fixture+".hold-"+host));
    const auto refreshed=[&]{const auto profiles=page->profiles();return profiles.size()==2 && std::all_of(profiles.cbegin(),profiles.cend(),[](const QJsonValue &v){return v.toObject()["label"]=="Renamed shared account";});};
    QTRY_VERIFY(refreshed());QVERIFY(!page->isVisible());QCOMPARE(filter->selection(),bindings);
    QVERIFY(refreshWindow.elapsed()<30000);
    QFile reads(fixture+".reads");QVERIFY(reads.open(QIODevice::ReadOnly));QMap<QString,int> perHost;
    for(const auto &line:reads.readAll().split('\n'))if(!line.isEmpty()){const auto row=QJsonDocument::fromJson(line).array();QCOMPARE(row.size(),2);++perHost[row[0].toString()];}
    const QMap<QString,int> expectedReads{{"local",3},{"mac",3}};QCOMPARE(perHost,expectedReads);
    // Catalog publication queues the Sessions rebuild. Wait for its visible
    // presentation before taking a new menu snapshot; the open action stays old.
    const auto presentationRefreshed=[&]{const auto chips=filter->findChildren<QPushButton *>("accountFilterChip");return chips.size()==1 && chips.first()->toolTip().contains("Renamed shared account");};
    QTRY_VERIFY(presentationRefreshed());
    QVERIFY(!sharedChoice()->text().contains("Renamed shared account"));
    // Existing actions keep their snapshot; reopening presents the newly read label.
    menu->hide();menu->popup(filter->button()->mapToGlobal(QPoint(0,30)));QTRY_VERIFY(menu->isVisible());QTRY_VERIFY(sharedChoice());QVERIFY(sharedChoice()->text().contains("Renamed shared account"));
}

void TestSessionsWindow::accountChooserMatchesAccountsAndKeepsRemovedProfileDetails()
{
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject usage{{"id","current"},{"provider","claude"},{"home","/fixture/current"},{"status","ok"},
        {"identity",QJsonObject{{"account_id","shared-account"},{"email","shared@example.test"},{"organization","Workspace"}}}};
    const QJsonObject current{{"id","current"},{"provider","claude"},{"label","shared@example.test"},{"home","/fixture/current"},{"installed",true},{"account_status",usage}};
    const QJsonObject removedDefault{{"id","native-claude"},{"provider","claude"},{"label","Default account"},{"home","/fixture/default"},{"installed",true}};
    const QJsonObject retired{{"id","retired"},{"provider","claude"},{"label","shared@example.test"},{"home","/fixture/retired"},{"installed",true}};
    const auto catalog=QString::fromUtf8(QJsonDocument(QJsonObject{{"profiles",QJsonArray{current}},{"removed_profiles",QJsonArray{removedDefault,retired}}}).toJson(QJsonDocument::Compact));
    const auto path=m_dir->filePath("account-catalog");
    const auto answer="elif c=='account' and a[1:2]==['ls']:print("+FakeCli::pythonString(catalog)+")\nelif c=='account' and a[1:2]==['inspect']:print("+FakeCli::pythonString(QString::fromUtf8(QJsonDocument(usage).toJson(QJsonDocument::Compact)))+")\n";
    QVERIFY(FakeCli::install(path,original.readAll().replace("if False:pass\n","if False:pass\n"+answer)));
    auto state=fleet();auto local=state.local();local.sessions[0].cmd="claude";local.sessions[0].accountId="current";local.sessions[0].accountHome="/fixture/current";
    local.sessions[1].cmd="claude";local.sessions[1].accountId.clear();local.sessions[1].accountHome="/fixture/current";
    auto past=local.sessions[0];past.name="claude/project/retired";past.state="stopped";past.accountId="retired";past.accountHome="/fixture/retired";local.sessions.append(past);
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());auto remote=*state.peer("mac");remote.sessions[0].cmd="claude";remote.sessions[0].accountId="current";remote.sessions[0].accountHome="/fixture/current";state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(path));keepDetailsPollsOut(window);window.setFleet(state);window.show();
    auto *page=window.findChild<AccountsPage *>();QVERIFY(page);
    QTRY_COMPARE(page->profiles(true).size(),6);QTRY_COMPARE(page->catalogProfiles().size(),2);
    QPushButton *accountsNav=nullptr;for(auto *button:window.findChildren<QPushButton *>("railButton"))if(button->accessibleName()=="Agent accounts")accountsNav=button;
    QVERIFY(accountsNav);accountsNav->click();QTRY_VERIFY(page->isVisible());
    auto *accountRows=page->findChild<QListWidget *>("accountProfiles");QVERIFY(accountRows);QTRY_COMPARE(accountRows->count(),1);
    const auto published=accountRows->item(0)->data(Qt::UserRole).toJsonObject();QCOMPARE(published["members"].toArray().size(),2);
    window.showSessionList();QVERIFY(!page->isVisible());
    auto *filter=window.findChild<AccountFilter *>("accountFilterChips");auto *menu=filter->button()->menu();
    menu->popup(filter->button()->mapToGlobal(QPoint(0,30)));QTRY_VERIFY(menu->isVisible());
    const auto choices=menu->findChildren<QCheckBox *>("accountFilterChoice");QCOMPARE(choices.size(),accountRows->count());
    QCOMPARE(choices.first()->property("account").toString(),published["account_key"].toString());
    QVERIFY(!choices.first()->text().contains("Default account"));
    const QSet<QString> expected{SessionAccounts::binding({},local.sessions[0]),SessionAccounts::binding({},local.sessions[1]),SessionAccounts::binding("mac",remote.sessions[0])};
    const auto members=choices.first()->property("bindings").toStringList();QCOMPARE(QSet<QString>(members.cbegin(),members.cend()),expected);
    choices.first()->click();QCOMPARE(filter->selection(),expected);menu->hide();
    window.showSession({},past.name);
    // WhiteWh fork: the account lines are in the Context card, open by default.
    auto *inspector=window.findChild<QWidget *>("sessionInspector");auto *toggle=window.findChild<QPushButton *>("toggleInspector");QVERIFY(inspector&&toggle);toggle->setChecked(true);QVERIFY(inspectorCardOpen(window,"context"));
    auto *browser=window.findChild<QTextBrowser *>("sessionInfo");QVERIFY(browser);QTRY_VERIFY(browser->toPlainText().contains("/fixture/retired"));
    QVERIFY(browser->toPlainText().contains("shared@example.test"));QVERIFY(!browser->toPlainText().contains("Workspace"));
}

void TestSessionsWindow::accountDetailsResolveOnlyTheRecordedBinding()
{
    QFile original(script());QVERIFY(original.open(QIODevice::ReadOnly));
    const QJsonObject profile{{"id","profile-work"},{"provider","codex"},{"label","<Work>"},{"home","/fixture/work"},{"installed",true},
        {"account_status",QJsonObject{{"status","ok"},{"identity",QJsonObject{{"email","work@example.test"},{"account_id","identity-work"}}}}}};
    const auto answer=QJsonDocument(QJsonObject{{"profiles",QJsonArray{profile}}}).toJson(QJsonDocument::Compact);
    const auto path=m_dir->filePath("account-details");
    QVERIFY(FakeCli::install(path,original.readAll().replace("if False:pass\n","if False:pass\nelif c=='account' and a[1:2]==['ls']:print("+FakeCli::pythonString(QString::fromUtf8(answer))+")\n")));
    auto state=fleet();auto local=state.local();local.sessions[0].accountId="profile-work";local.sessions[0].accountHome="/fixture/work";
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(FakeCli::program(path));keepDetailsPollsOut(window);window.setFleet(state);window.show();window.showSession({},local.sessions[0].name);
    // WhiteWh fork: the account lines are in the Context card, open by default.
    auto *inspector=window.findChild<QWidget *>("sessionInspector");auto *toggle=window.findChild<QPushButton *>("toggleInspector");
    QVERIFY(inspector&&toggle);toggle->setChecked(true);QVERIFY(inspectorCardOpen(window,"context"));
    auto *browser=window.findChild<QTextBrowser *>("sessionInfo");QVERIFY(browser);
    window.findChild<AccountsPage *>()->ensureCatalogs();
    QTRY_VERIFY(browser->toPlainText().contains("work@example.test"));
    QVERIFY(browser->toPlainText().contains("<Work>"));QVERIFY(browser->toPlainText().contains("profile-work"));QVERIFY(browser->toPlainText().contains("/fixture/work"));
    window.showSession({},local.sessions[1].name);
    QTRY_VERIFY(browser->toPlainText().contains("Account not recorded"));QVERIFY(!browser->toPlainText().contains("work@example.test"));
}

void TestSessionsWindow::accountScopeLimitsArchiveRemoval()
{
    auto state=fleet();auto local=state.local();auto one=local.sessions[0];one.state="archived";one.archiveId="account-one";one.accountId="work";
    auto two=one;two.archiveId="account-two";two.accountId="personal";local.sessions<<one<<two;state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());keepDetailsPollsOut(window);window.setFleet(state);window.show();
    auto *accounts=window.findChild<AccountFilter *>("accountFilterChips");auto *clear=window.findChild<QAction *>("clearArchive");QVERIFY(accounts&&clear);
    accounts->setSelection({SessionAccounts::binding({},one)});QVERIFY(clear->isEnabled());
    auto *client=window.findChild<HgsClient *>();QSignalSpy writes(client,&HgsClient::writeDone);
    QTimer::singleShot(0,&window,[&]{auto *dialog=window.findChild<QMessageBox *>("confirmSelectionAction");QVERIFY(dialog);
        QVERIFY(dialog->text().contains("1 archived"));dialog->reject();});clear->trigger();QCOMPARE(writes.size(),0);
    accounts->setSelection({SessionAccounts::binding({},QString("codex"),QString("absent"))});QVERIFY(!clear->isEnabled());
}

void TestSessionsWindow::localWindowsFolderFilterIgnoresCase()
{
    // A local Windows folder filter matches the CLI's paths in any case and with either
    // separator, and names the folder natively; a peer's POSIX folder stays exact.
    auto state=fleet();auto local=state.local();local.metrics["platform"]="windows";
    local.sessions[0].cwd=local.sessions[0].canonicalCwd="c:\\work\\repo";local.sessions[0].gitRoot="C:\\WORK\\Repo";
    local.sessions[1].cwd=local.sessions[1].canonicalCwd="C:\\Work\\Repo\\sub";local.sessions[1].gitRoot="C:\\Work\\Repo";
    state.setLocal(local,QDateTime::currentMSecsSinceEpoch());
    // A POSIX peer keeps its folders exact.
    auto remote=*state.peer("mac");remote.metrics["platform"]="macos";remote.sessions[0].canonicalCwd=remote.sessions[0].gitRoot="/work/repo";
    state.setPeer(remote,QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program());window.setFleet(state);window.resize(1200,850);window.show();
    auto *page=window.findChild<ProjectsDialog *>("projectsPage");auto *list=window.findChild<SessionList *>("sessionList");
    auto *clear=window.findChild<QPushButton *>("clearFolderFilter");
    const auto keys=[&]{QStringList result;for(int i=0;i<list->count();++i){const auto key=list->item(i)->data(SessionRoles::Key).toString();if(!key.isEmpty())result<<key;}result.sort();return result;};
    const auto all=[&]{for(auto *button:window.findChildren<QPushButton *>())if(button->property("filter")=="all")return button->property("count").toInt();return -1;};
    page->folderSessionsRequested({},"C:/Work/Repo",false);
    QCOMPARE(keys(),QStringList{"\ncodex/hgs/dashboard"});QCOMPARE(all(),1);
    QCOMPARE(clear->text(),QString("Folder: Repo   ×"));QVERIFY(clear->toolTip().startsWith("C:\\Work\\Repo\n"));
    page->folderSessionsRequested({},"c:/work/repo",true);
    QCOMPARE(keys(),QStringList({"\ncodex/hgs/dashboard","\nkimi/docs/research"}));QCOMPARE(all(),2);
    page->folderSessionsRequested("mac","/Work/Repo",true);QVERIFY(keys().isEmpty());QCOMPARE(all(),0);
    QCOMPARE(clear->text(),QString("Folder: Repo   ×"));QVERIFY(clear->toolTip().startsWith("/Work/Repo\n"));
    page->folderSessionsRequested("mac","/work/repo",true);QCOMPARE(keys(),QStringList{"mac\nclaude/infra/review"});QCOMPARE(all(),1);
    // A Linux computer keeps exact POSIX names.
    local.metrics["platform"]="linux";state.setLocal(local,QDateTime::currentMSecsSinceEpoch());window.setFleet(state);
    page->folderSessionsRequested({},"c:/work/repo",true);QVERIFY(keys().isEmpty());QCOMPARE(all(),0);
    QVERIFY(clear->toolTip().startsWith("c:/work/repo\n"));
}

void TestSessionsWindow::connectionStatusSurvivesNoticesAndRetries()
{
    SessionsWindow window(program()); window.setFleet(fleet()); window.show(); window.setNoticeDurationForTests(250);
    auto *status = window.findChild<QLabel *>("workspaceConnectionStatus");
    auto *retry = window.findChild<QPushButton *>("connectionRetry");
    QVERIFY(status); QVERIFY(retry); QVERIFY(status->text().isEmpty()); QVERIFY(retry->isHidden());
    window.setConnectionError("timed out");
    QVERIFY(status->text().contains("arch: session update timed out"));
    QVERIFY(status->text().contains("automatic retry pending"));
    QSignalSpy refresh(&window, &SessionsWindow::refreshRequested); retry->click(); QCOMPARE(refresh.size(), 1);
    window.setPollingHosts({QString()});
    QVERIFY(status->text().contains("retrying…")); QVERIFY(status->text().contains("timed out")); QVERIFY(!retry->isEnabled());
    window.showNotificationNotice("Path copied");
    QTRY_VERIFY(window.findChild<QLabel *>("notice")->text().isEmpty());
    QVERIFY(status->text().contains("timed out"));
    auto state = fleet(); BoxState unavailable; unavailable.host = "mac"; unavailable.error = "ssh: Could not resolve hostname <mac>";
    state.setPeer(unavailable, QDateTime::currentMSecsSinceEpoch()); window.setFleet(state);
    QVERIFY(status->text().contains("<mac>")); QVERIFY(status->toolTip().contains("&lt;mac&gt;"));
    window.setConnectionError({}); window.setPollingHosts({});
    QVERIFY(!status->text().contains("arch:")); QVERIFY(status->text().contains("Could not resolve")); QVERIFY(retry->isEnabled());
    window.setPollingHosts({"mac"}); QVERIFY(status->text().contains("retrying…")); QVERIFY(!retry->isEnabled());
    window.setFleet(fleet()); window.setPollingHosts({});
    QVERIFY(status->text().isEmpty()); QVERIFY(retry->isHidden());
    state = fleet(); state.registerPeer("new-machine"); window.setFleet(state); window.setPollingHosts({"new-machine"});
    QVERIFY(status->text().contains("new-machine: connecting…"));
    const auto preview = qEnvironmentVariable("HGS_CONNECTION_PREVIEW");
    if (!preview.isEmpty()) { window.setConnectionError("timed out"); QTest::qWait(50); QVERIFY(window.grab().save(preview)); }
}

void TestSessionsWindow::peerNamedDifferentlyListsSessionsUnderItsAlias()
{
    // Saved as "winpeer", the machine calls itself "zpeer" (its HGS_SELF), and
    // `hgs @winpeer ls --json` forwards its own listing. The window is fed as
    // TrayAgent feeds it: the client's polls in flight and each answer's box.
    QTemporaryDir fixture; QVERIFY(fixture.isValid());
    const auto peerCli = fixture.filePath("hgs");
    QVERIFY(FakeCli::install(peerCli, "#!/usr/bin/env python3\nimport sys\n"
        "if sys.argv[1:]!=['@winpeer','ls','--json','--no-archives']:print('unexpected call',file=sys.stderr);sys.exit(1)\n"
        "print('{\"host\":\"zpeer\",\"ok\":true,\"projects\":{},\"sessions\":[{\"name\":\"codex/win/build\",\"cmd\":\"codex\","
        "\"project\":\"win\",\"tag\":\"build\",\"state\":\"running\",\"activity\":\"idle\"}]}')\n"));
    HgsClient client(FakeCli::program(peerCli)); QList<BoxState> answers;
    connect(&client, &HgsClient::peerReady, this, [&answers](const BoxState &box) { answers << box; });
    auto state = fleet(); auto local = state.local(); local.peersKnown = true; local.peers = {"mac", "winpeer"};
    state.setLocal(local, QDateTime::currentMSecsSinceEpoch());
    SessionsWindow window(program()); window.setFleet(state); window.show();
    auto *status = window.findChild<QLabel *>("workspaceConnectionStatus"); QVERIFY(status);
    auto *list = window.findChild<QListWidget *>("sessionList"); QVERIFY(list);
    client.requestPeer("winpeer"); window.setPollingHosts(client.pollingHosts());
    QVERIFY(status->text().contains("winpeer: connecting…"));
    QTRY_COMPARE(answers.size(), 1);
    state.setPeer(answers.first(), QDateTime::currentMSecsSinceEpoch());
    window.setFleet(state); window.setPollingHosts(client.pollingHosts());
    QVERIFY2(status->text().isEmpty(), qPrintable(status->text()));
    QVERIFY(!state.peer("zpeer")); QVERIFY(state.peer("winpeer") && state.peer("winpeer")->ok);
    QListWidgetItem *row = nullptr;
    for (int i = 0; i < list->count(); ++i)
        if (list->item(i)->data(SessionRoles::Key).toString().endsWith("\ncodex/win/build")) row = list->item(i);
    QVERIFY(row);
    QCOMPARE(row->data(SessionRoles::Key).toString(), QString("winpeer\ncodex/win/build"));
    QCOMPARE(row->data(SessionRoles::MachineName).toString(), QString("winpeer"));
    QCOMPARE(row->data(SessionRoles::GitHost).toString(), QString("winpeer"));
}

void TestSessionsWindow::remoteSessionLoadingIsDelayedAndFollowsSelection()
{
    const auto fixture = m_dir->filePath("slow-session-read");
    QVERIFY(FakeCli::install(fixture, R"PY(#!/usr/bin/env python3
import json,sys,time
a=sys.argv[1:]
if 'inspect' in a:
 if a and a[0]=='@mac':time.sleep(2.8)
 print(json.dumps({'tracked':True,'run_id':'fixture-run','conversation_id':'fixture-conversation',
  'runtime_state':'live','process_state':'running','phase':'idle','activity':'idle','events':[],'cursor':0}))
else:print('{}')
)PY"));
    SessionsWindow window(FakeCli::program(fixture)); window.setFleet(fleet()); window.show();
    window.showSession({}, "codex/hgs/dashboard");
    auto *loading = window.findChild<QWidget *>("sessionLoading"); auto *title = window.findChild<QLabel *>("detailTitle");
    QVERIFY(loading); QVERIFY(title);
    window.setPollingHosts({QString()}); QTest::qWait(2100); QVERIFY(loading->isHidden()); window.setPollingHosts({});
    window.showSession("mac", "claude/infra/review"); QTest::qWait(100);
    const auto titleText = title->text(); const auto titleRect = title->geometry();
    QVERIFY(loading->isHidden()); QTest::qWait(700); QVERIFY(loading->isHidden());
    QTRY_VERIFY_WITH_TIMEOUT(loading->isVisible(), 1800);
    QCOMPARE(title->text(), titleText); QCOMPARE(title->geometry(), titleRect);
    QVERIFY(loading->geometry().left() >= title->fontMetrics().horizontalAdvance(titleText));
    const auto preview = qEnvironmentVariable("ZERUS_INLINE_STATUS_PREVIEW");
    if (!preview.isEmpty()) { QDir().mkpath(preview); QVERIFY(window.grab().save(preview + "/remote-loading.png")); }
    QTRY_VERIFY_WITH_TIMEOUT(loading->isHidden(), 2000);
    // The same remote read is still real work, but cannot decorate another selection.
    auto *client = window.findChild<HgsClient *>(); client->requestInspection("mac", "claude/infra/review");
    window.showSession({}, "codex/hgs/dashboard"); QTest::qWait(2200); QVERIFY(loading->isHidden());
    client->inspectionLoadingChanged("mac", "claude/infra/review", {}, {}, false); QVERIFY(loading->isHidden());
}

void TestSessionsWindow::recoverySettingsValidateAndSaveForSelectedMachine_data()
{
    QTest::addColumn<QString>("sessionMode");
    QTest::newRow("wait-for-reset") << QString("reset");
    QTest::newRow("interval") << QString("interval");
}

void TestSessionsWindow::recoverySettingsValidateAndSaveForSelectedMachine()
{
    QFETCH(QString, sessionMode);
    const auto program=m_dir->filePath("recovery-hgs"), capture=m_dir->filePath("recovery-policy.json");
    QFile::remove(capture);
    QFile fixture(program); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write("#!/usr/bin/env python3\nimport sys\na=sys.argv[1:]\nif a[:1]==['@mac']:a=a[1:]\nif a[:1]!=['recovery']:sys.exit(2)\n"
                  "if a[1:2]==['set']:open("+FakeCli::pythonString(capture)+",'wb').write(sys.stdin.buffer.read())\n"
                  "print('{\"order\":[7,\"writer\"],\"policy\":{\"version\":2,\"revision\":7,\"writer\":\"writer\",\"enabled\":false,\"enabled_at\":0,\"service\":true,\"network\":true,\"rate_limit\":true,\"delays\":[15,30,60,300]}}')\n");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QSettings().remove("recovery/sharedPolicy");
    QSettings().setValue("workspace/theme","dark");SessionsWindow window(FakeCli::program(program));window.resize(1100,900);window.setFleet(fleet());window.show();
    window.findChild<QPushButton *>("workspaceSettings")->click();auto &page=*static_cast<SettingsPage *>(window.findChild<QWidget *>("settingsPage"));page.openRecovery();
    auto *enabled=page.findChild<QCheckBox *>("recoveryEnabled");QTRY_VERIFY(enabled->isEnabled());QVERIFY(!enabled->isChecked());
    auto *save=page.findChild<QPushButton *>("recoverySave");QTRY_VERIFY(save->isEnabled());
    auto *delays=page.findChild<QLineEdit *>("recoveryDelays_network");QCOMPARE(delays->text(),QString("15, 30, 60, 300, -1"));
    auto *limit=page.findChild<QCheckBox *>("recoveryRule_session_limit");QVERIFY(limit->isChecked());
    auto *mode=page.findChild<QComboBox *>("recoverySessionLimitMode");QCOMPARE(mode->currentData().toString(),QString("reset"));
    auto *limitDelays=page.findChild<QLineEdit *>("recoveryDelays_session_limit");QCOMPARE(limitDelays->text(),QString("300, 0"));QVERIFY(!limitDelays->isEnabled());
    const auto before=page.size();page.refresh();QTRY_VERIFY(save->isEnabled());QCOMPARE(page.size(),before);
    const auto preview=qEnvironmentVariable("HGS_SETTINGS_PREVIEW");if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(window.grab().save(preview+"/recovery.png"));window.resize(850,600);QTest::qWait(30);QVERIFY(window.grab().save(preview+"/recovery-narrow.png"));}
    enabled->setChecked(true);QVERIFY(mode->isEnabled());QVERIFY(!limitDelays->isEnabled());
    mode->setCurrentIndex(mode->findData(sessionMode));QCOMPARE(limitDelays->isEnabled(),sessionMode=="interval");
    if(sessionMode=="interval")limitDelays->setText("600, 0");
    limit->setChecked(false);QVERIFY(!mode->isEnabled());QVERIFY(!limitDelays->isEnabled());limit->setChecked(true);
    delays->setText("0");save->click();QVERIFY(!QFile::exists(capture));
    delays->setText("5, 20, 0");save->click();QTRY_VERIFY(QFile::exists(capture));
    const auto readPolicy=[&]{QFile saved(capture);if(!saved.open(QIODevice::ReadOnly))return QJsonObject();return QJsonDocument::fromJson(saved.readAll()).object();};
    QTRY_VERIFY(!readPolicy().isEmpty());const auto policy=readPolicy();
    QVERIFY(policy.value("enabled").toBool());QCOMPARE(policy.value("revision").toInt(),7);
    QCOMPARE(policy.value("version").toInt(),3);QVERIFY(policy.value("session_limit").toBool());QCOMPARE(policy.value("session_limit_mode").toString(),sessionMode);
    QCOMPARE(policy.value("schedules").toObject().value("session_limit").toArray(),QJsonArray({sessionMode=="interval"?600:300,0}));
    QCOMPARE(policy.value("schedules").toObject().value("network").toArray(),QJsonArray({5,20,0}));
    QCOMPARE(policy.value("schedules").toObject().value("service").toArray(),QJsonArray({15,30,60,300,-1}));
}

void TestSessionsWindow::sharedRecoverySyncCatchesUpOfflinePeer()
{
    QTemporaryDir temp;const auto program=temp.filePath("sync-hgs");QFile fixture(program);QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(QByteArray("#!/usr/bin/env python3\n")+R"PY(import sys,json,pathlib
root=pathlib.Path(__file__).parent
args=sys.argv[1:];host='local'
if args[0].startswith('@'):host=args.pop(0)[1:]
if host=='mac' and (root/'offline').exists():sys.exit(1)
path=root/(host+'.json')
p=json.loads(path.read_text())
def order(p):return [p['revision'],p['writer']]
if args[1]=='sync':
 incoming=json.load(sys.stdin)
 if order(incoming)>order(p):p=incoming;path.write_text(json.dumps(p))
print(json.dumps({'policy':p,'order':order(p)}))
)PY");fixture.close();QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    const QJsonObject latest{{"version",3},{"revision",4},{"writer","local-writer"},{"enabled",false},{"service",true},{"network",true},{"rate_limit",true},
        {"session_limit",true},{"session_limit_mode","interval"},
        {"delays",QJsonArray{15,30,-1}},{"schedules",QJsonObject{{"network",QJsonArray{5,0}},{"session_limit",QJsonArray{600,0}}}}};
    auto older=latest;older["revision"]=2;older["writer"]="peer";
    for(const auto &host:QStringList{"local","mac"}){QFile file(temp.filePath(host+".json"));QVERIFY(file.open(QIODevice::WriteOnly));file.write(QJsonDocument(host=="local"?latest:older).toJson());}
    QFile offline(temp.filePath("offline"));QVERIFY(offline.open(QIODevice::WriteOnly));offline.close();QSettings().remove("recovery/sharedPolicy");
    QObject parent;RecoverySync sync(FakeCli::program(program),&parent);RecoverySettings settings(&sync);sync.setPeers({"mac"});QTRY_VERIFY(!sync.busy());
    QCOMPARE(sync.envelope().value("policy").toObject(),latest);QVERIFY(sync.status().contains("mac"));
    QCOMPARE(settings.findChild<QComboBox *>("recoverySessionLimitMode")->currentData().toString(),QString("interval"));
    QCOMPARE(settings.findChild<QLineEdit *>("recoveryDelays_session_limit")->text(),QString("600, 0"));
    QVERIFY(QFile::remove(offline.fileName()));sync.refresh();QTRY_VERIFY(!sync.busy());
    QFile remote(temp.filePath("mac.json"));QVERIFY(remote.open(QIODevice::ReadOnly));QCOMPARE(QJsonDocument::fromJson(remote.readAll()).object(),latest);remote.close();
    auto newer=latest;newer["revision"]=5;newer["writer"]="remote-writer";newer["service"]=false;
    QVERIFY(remote.open(QIODevice::WriteOnly));remote.write(QJsonDocument(newer).toJson());remote.close();sync.refresh();QTRY_VERIFY(!sync.busy());
    QFile local(temp.filePath("local.json"));QVERIFY(local.open(QIODevice::ReadOnly));QCOMPARE(QJsonDocument::fromJson(local.readAll()).object(),newer);
    sync.save(latest,{{"policy",latest},{"order",QJsonArray{4,"local-writer"}}});QVERIFY(sync.error().contains("another Zerus"));
}

QTEST_MAIN(TestSessionsWindow)
#include "test_sessionswindow_machines.moc"
