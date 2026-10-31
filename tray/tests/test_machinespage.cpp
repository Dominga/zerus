#include "support/PageShots.h"
#include <QtTest>
#include <QApplication>
#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QDir>
#include <QFile>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QScrollBar>
#include <QSettings>
#include <QSpinBox>
#include <QTemporaryDir>
#include <QTimer>
#include "FakeCli.h"
#include "PopupMouse.h"
#include "MachinesPage.h"
#include "Settings.h"
#include "MessageBoxIcons.h"
#include "MachineAppearance.h"
#include "MachinePlatform.h"

class TestMachinesPage : public QObject {
    Q_OBJECT
private slots:
    void init();
    void unsavedEditsGateActionsAndTheLabelHasSwatches();
    void unsavedDotSurvivesRowRebuilds();
    void removalRequiresExplicitConfirmationOfSameConnection_data();
    void removalRequiresExplicitConfirmationOfSameConnection();
    void localReadOnlyAndNewProfileCannotOverwrite();
    void wslProfileKeepsItsTargetAcrossSaveAndReload();
    void aSavedMachineSwitchesToWslAndBackKeepingItsOverrides();
    void otherTransportSavedBehindAHeldLockReadsBack();
    void aConfiguredAliasSaysWhereItComesFrom();
    void newMachineDraftCancelAndDiscard_data();
    void newMachineDraftCancelAndDiscard();
    void editsKeepDraftWhileFleetRefreshesAndFailuresAreVisible();
    void testConnectionAndOpenSshKeepSelectedAlias();
    void installationStreamsOutputAndKeepsProfileLocked();
    void effectiveValuesNeverBecomeOverridesOrReplaceDrafts();
    void staleResolutionCannotPopulateAnotherMachine();
    void colorsWorkForLocalAndPersistWithoutSavingConnection();
    void colorPickerDismissalKeepsTheSavedColor_data();
    void colorPickerDismissalKeepsTheSavedColor();
    void dashboardSelectionWaitsForProfiles();
    void controlOnlyComputerIsAConsoleWithoutSessions();
    void countsReadInSingularAndPlural();
    void hiddenPageRendersFleetChangesWhenShown();
    void windowsConsoleEnablesLocalSessions();
    void enablingIsOfferedOnlyToAWindowsConsole();
    void windowsComputerPageHasNoSshConnection();
    void localSessionHostStates_data();
    void localSessionHostStates();
    void preview();
private:
    template<class T> T *widget(MachinesPage &page, const char *name) {
        auto *result = page.findChild<T *>(name); Q_ASSERT(result); return result;
    }
    void start(MachinesPage &page);
    QJsonArray calls() const;
    void flag(const QString &name, const QString &text = "1");
    QTemporaryDir m_dir;
    QString m_script;
};

void TestMachinesPage::init()
{
    MessageBoxIcons::install();
    QVERIFY(m_dir.isValid());
    QSettings::setDefaultFormat(QSettings::IniFormat); QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_dir.path());
    QCoreApplication::setOrganizationName("hgs-machines-test"); QCoreApplication::setApplicationName("MachinesPage"); Settings::flush();QSettings().clear(); MachineAppearance::invalidate();
    for (const auto &name : {"calls.jsonl", "stale", "probe-mode", "resolve-delay", "list-delay", "host-mode"}) QFile::remove(m_dir.filePath(name));
    m_script = m_dir.filePath("hgs");
    QFile fixture(m_script); QVERIFY(fixture.open(QIODevice::WriteOnly));
    fixture.write(R"(#!/usr/bin/env python3
import json, pathlib, sys, time
root=pathlib.Path(__file__).parent
args=sys.argv[1:]
with (root/'calls.jsonl').open('a') as out: out.write(json.dumps(args)+'\n')
if args[0]=='session-host':
 assert args==['session-host','install','--json'],args
 time.sleep(.05)
 if (root/'host-mode').exists():
  print('cannot enable local sessions: access denied',file=sys.stderr); sys.exit(1)
 print(json.dumps({'enabled':True,'version':'1.52.0','marker_created':True,
  'cli':{'path':'C:\\Users\\user\\.local\\bin\\hgs.exe','sha256':'a'*64,'version':'1.52.0','changed':True}}))
 sys.exit(0)
# The Phones card asks every reachable workspace (U25 shots): one list here.
if args and args[0].startswith('@') and args[1:2] in (['mobile'],['--help']): args=args[1:]
if args==['--help']:
 print('usage: hgs [@host] <cmd>'); print('       hgs [@host] mobile phones|invite|revoke-phone <id> --json   phones'); sys.exit(0)
if args[0:2]==['mobile','phones']:
 now=time.time()
 print(json.dumps({'configured':True,'workspace_id':'w1','workspace_name':'Orbit','server_url':'https://relay.example',
  'devices':[{'id':'p1','name':'Anna’s iPhone','platform':'ios','paired_at':now-86400*15,'last_seen_at':now-600,'added_by':{'name':'workstation'}},
             {'id':'p2','name':'Pixel 9','platform':'android','paired_at':now-86400*54,'last_seen_at':now-86400*3,'added_by':{'name':'macbook'}}]}))
 sys.exit(0)
assert args[0]=='machine'
command=args[1]
if command=='ls':
 if (root/'list-delay').exists(): time.sleep(.05)
 print((root/'data.json').read_text())
elif command=='resolve':
 if (root/'resolve-delay').exists(): time.sleep(.2 if args[2]=='mac' else .01)
 print(json.dumps({'alias':args[2],'effective':{'hostname':args[2]+'.example','user':'ssh-user','port':'2222','identity_files':['~/.ssh/id_ed25519'],'proxy_jump':'jump.example'}}))
elif command=='set':
 if (root/'stale').exists():
  print('Machine settings changed elsewhere. Reload the page before saving.',file=sys.stderr); sys.exit(1)
 data=json.loads((root/'data.json').read_text())
 profile=json.loads(args[args.index('--json')+1])
 data['machines']=[r for r in data['machines'] if r['alias']!=args[2]]
 data['machines'].append({'alias':args[2],'connection':profile,'enabled':True,'source':'local'})
 data['revision']='revision-2'
 (root/'data.json').write_text(json.dumps(data))
 print('{"ok":true}')
elif command=='remove':
 data=json.loads((root/'data.json').read_text());data['machines']=[r for r in data['machines'] if r['alias']!=args[2]]
 (root/'data.json').write_text(json.dumps(data));print('{"ok":true}')
elif command=='check':
 time.sleep(.03)
 if (root/'probe-mode').exists(): print('{"ssh_ok":true,"ok":false,"error":"SSH connected, but no setup report"}')
 else: print('{"ssh_ok":true,"ok":true,"os":"Darwin","arch":"arm64","hgs":"hgs 1.17.0","tmux":"tmux 3.5"}')
elif command=='setup':
 print('Building HGS for this machine…',flush=True); time.sleep(.06)
 print('Setup complete: hgs fixture')
else: raise AssertionError(args)
)");
    fixture.close(); QVERIFY(FakeCli::makeRunnable(fixture.fileName()));
    QFile data(m_dir.filePath("data.json")); QVERIFY(data.open(QIODevice::WriteOnly));
    data.write(R"({"local":"arch","revision":"revision-1","machines":[{"alias":"mac","connection":{"enabled":true},"enabled":true,"source":"config","inherited":true}]})");
}
void TestMachinesPage::removalRequiresExplicitConfirmationOfSameConnection_data()
{
    QTest::addColumn<QString>("choice");
    for (const auto *choice : {"cancel", "enter", "escape", "proceed", "selection", "revision"}) QTest::newRow(choice) << QString(choice);
}

void TestMachinesPage::removalRequiresExplicitConfirmationOfSameConnection()
{
    QFETCH(QString, choice);
    flag("data.json", R"({"local":"arch","revision":"revision-1","machines":[{"alias":"mac","connection":{"enabled":true,"hostname":"mac.example"},"enabled":true,"source":"local"}]})");
    MachinesPage page(FakeCli::program(m_script)); start(page); page.selectMachine("mac");
    auto *remove=widget<QPushButton>(page,"removeMachine");QTRY_VERIFY(remove->isEnabled());
    bool shown=false;
    QTimer respond;respond.setInterval(5);
    connect(&respond,&QTimer::timeout,&page,[&] {
        auto *box=page.findChild<QMessageBox *>("removeMachineConfirm");if(!box||!box->isVisible())return;
        respond.stop();shown=true;
        QCOMPARE(box->defaultButton(),qobject_cast<QPushButton *>(box->button(QMessageBox::Cancel)));
        QCOMPARE(box->escapeButton(),box->button(QMessageBox::Cancel));QVERIFY(box->text().contains("mac"));
        if(choice=="cancel"){box->button(QMessageBox::Cancel)->click();return;}
        if(choice=="enter"||choice=="escape"){QTest::keyClick(box,choice=="enter"?Qt::Key_Return:Qt::Key_Escape);return;}
        if(choice=="selection")page.selectMachine({});
        if(choice=="revision"){
            flag("data.json",R"({"local":"arch","revision":"revision-2","machines":[{"alias":"mac","connection":{"enabled":true,"hostname":"mac.example"},"enabled":true,"source":"local"}]})");
            page.reload();QTRY_VERIFY(remove->isEnabled());
        }
        for(auto *button:box->buttons())if(box->buttonRole(button)==QMessageBox::DestructiveRole)button->click();
    });respond.start();remove->click();respond.stop();QVERIFY(shown);
    const auto removals=[&]{QJsonArray result;for(const auto &call:calls())if(call.toArray().at(1)=="remove")result.append(call);return result;};
    // The confirmed removal runs a fake CLI process, which a loaded runner can start
    // later than any fixed wait; a refused one must send nothing within the window.
    if(choice=="proceed")QTRY_COMPARE(removals().size(),1);
    else{QTest::qWait(150);QCOMPARE(removals().size(),0);}
    if(choice=="proceed")QCOMPARE(removals().first().toArray(),QJsonArray({"machine","remove","mac","--revision","revision-1"}));
    if(choice=="selection"||choice=="revision")QVERIFY(widget<QLabel>(page,"machineNotice")->text().contains("connection changed"));
}

void TestMachinesPage::flag(const QString &name, const QString &text)
{
    QFile file(m_dir.filePath(name)); QVERIFY(file.open(QIODevice::WriteOnly)); file.write(text.toUtf8());
}
void TestMachinesPage::start(MachinesPage &page)
{
    BoxState local; local.host = "arch"; local.ok = true;
    FleetState fleet; fleet.setLocal(local, 0); page.setFleet(fleet);
    page.resize(980, 800); page.show();
    QTRY_COMPARE(widget<QListWidget>(page, "machineProfiles")->count(), 2);
    QTRY_VERIFY(widget<QPushButton>(page, "addMachine")->isEnabled());
}
QJsonArray TestMachinesPage::calls() const
{
    QFile file(m_dir.filePath("calls.jsonl")); if (!file.open(QIODevice::ReadOnly)) return {};
    QJsonArray result;
    for (const auto &line : file.readAll().split('\n')) if (!line.isEmpty()) result.append(QJsonDocument::fromJson(line).array());
    return result;
}
void TestMachinesPage::localReadOnlyAndNewProfileCannotOverwrite()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    QVERIFY(!widget<QPushButton>(page, "browseMachineKey")->isEnabled());
    QVERIFY(!widget<QPushButton>(page, "saveMachine")->isEnabled());
    QSignalSpy accounts(&page, &MachinesPage::accountsRequested);
    widget<QPushButton>(page, "machineAccounts")->click(); QCOMPARE(accounts.size(), 1); QCOMPARE(accounts[0][0].toString(), "");
    widget<QPushButton>(page, "addMachine")->click();
    QVERIFY(!widget<QPushButton>(page, "machineAccounts")->isEnabled());
    auto *alias = widget<QLineEdit>(page, "machineAlias");
    auto *save = widget<QPushButton>(page, "saveMachine");
    QVERIFY(!alias->isReadOnly());
    alias->setText("mac"); QVERIFY(!save->isEnabled()); QVERIFY(save->toolTip().contains("already exists"));
    alias->setText("arch"); QVERIFY(!save->isEnabled());
    alias->setText("studio"); QVERIFY(save->isEnabled());
    widget<QLineEdit>(page, "machineHostname")->setText("192.0.2.1");
    widget<QLineEdit>(page, "machineUser")->setText("alice");
    widget<QLineEdit>(page, "machineKey")->setText(QString::fromUtf8("~/Keys/ключ space"));
    widget<QSpinBox>(page, "machinePort")->setValue(2222);
    QSignalSpy changed(&page, &MachinesPage::machinesChanged); save->click();
    QVERIFY(!widget<QPushButton>(page, "browseMachineKey")->isEnabled());
    QTRY_COMPARE(changed.size(), 1);
    QTRY_COMPARE(widget<QListWidget>(page, "machineProfiles")->count(), 3);
    QTRY_VERIFY(alias->isReadOnly());
    QJsonArray saved;
    for (const auto &call : calls()) if (call.toArray().at(1).toString() == "set") saved = call.toArray();
    QVERIFY(saved.contains("--create")); QVERIFY(saved.contains("revision-1"));
    const auto profile = QJsonDocument::fromJson(saved.at(4).toString().toUtf8()).object();
    QCOMPARE(profile["hostname"].toString(), "192.0.2.1");
    QCOMPARE(profile["user"].toString(), "alice");
    QCOMPARE(profile["port"].toInt(), 2222);
    QCOMPARE(profile["identity_file"].toString(), QString::fromUtf8("~/Keys/ключ space"));
}
void TestMachinesPage::wslProfileKeepsItsTargetAcrossSaveAndReload()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    widget<QPushButton>(page,"addMachine")->click();
    widget<QLineEdit>(page,"machineAlias")->setText("linux-tools");
    auto *transport = widget<QComboBox>(page,"machineTransport");
    transport->setCurrentIndex(transport->findData("wsl-local"));
    auto *save = widget<QPushButton>(page,"saveMachine");
    QVERIFY(!save->isEnabled());
    widget<QLineEdit>(page,"machineWslDistribution")->setText("Ubuntu-24.04");
    widget<QLineEdit>(page,"machineWslUser")->setText("alice");
    widget<QLineEdit>(page,"machineWslHgs")->setText("/home/alice/My Tools/hgs");
    QVERIFY(save->isEnabled());
    QVERIFY(widget<QLineEdit>(page,"machineHostname")->isHidden());
    QSignalSpy changed(&page,&MachinesPage::machinesChanged); save->click();
    QTRY_COMPARE(changed.size(),1);
    QTRY_VERIFY(widget<QLineEdit>(page,"machineAlias")->isReadOnly());
    QCOMPARE(transport->currentData().toString(),QString("wsl-local"));
    QCOMPARE(widget<QLineEdit>(page,"machineWslUser")->text(),QString("alice"));
    QVERIFY(!widget<QPushButton>(page,"installMachine")->isEnabled());
    QCOMPARE(widget<QPushButton>(page,"sshMachine")->text(),QString("Open WSL terminal"));
    QTRY_VERIFY(widget<QLineEdit>(page,"machineWslUser")->isEnabled());
    widget<QLineEdit>(page,"machineWslUser")->setText("bob");
    QTRY_VERIFY(save->isEnabled()); save->click();
    QTRY_COMPARE(changed.size(),2);
    QTRY_VERIFY(widget<QLineEdit>(page,"machineAlias")->isReadOnly());
    QJsonObject profile;
    for(const auto &call:calls()) {
        const auto args=call.toArray();
        if(args.size()>4 && args[1].toString()=="set") profile=QJsonDocument::fromJson(args[4].toString().toUtf8()).object();
    }
    QCOMPARE(profile["transport"].toString(),QString("wsl-local"));
    QCOMPARE(profile["wsl"].toObject()["user"].toString(),QString("bob"));
    QVERIFY(!profile.contains("hostname")); QVERIFY(!profile.contains("identity_file"));
}

// #650 N2: a saved SSH machine changes to Local WSL and back from the same
// form; hgs keeps one type per machine, and the other type's overrides come
// back from this device. L4: the HGS_PEERS override and a configured alias
// say so on the page.
void TestMachinesPage::aSavedMachineSwitchesToWslAndBackKeepingItsOverrides()
{
    flag("data.json", R"({"local":"arch","revision":"revision-1","environment_override":true,"machines":[{"alias":"ubuntu","connection":{"enabled":true,"hostname":"ubuntu.example","user":"alice","port":2222,"identity_file":"~/.ssh/ubuntu"},"enabled":true,"source":"local"}]})");
    MachinesPage page(FakeCli::program(m_script)); start(page);
    QVERIFY(widget<QLabel>(page, "machinesPeersOverride")->isVisible());
    QVERIFY(widget<QLabel>(page, "machinesPeersOverride")->text().contains("HGS_PEERS"));
    page.selectMachine("ubuntu");
    auto *transport = widget<QComboBox>(page, "machineTransport");
    auto *hostname = widget<QLineEdit>(page, "machineHostname");
    QTRY_VERIFY(transport->isVisible() && transport->isEnabled());
    QCOMPARE(hostname->text(), QString("ubuntu.example"));
    QVERIFY(!widget<QLabel>(page, "machineSourceNote")->isVisible());   // a local alias
    const auto lastSet = [&] {
        QJsonObject profile;
        for (const auto &call : calls()) {
            const auto args = call.toArray();
            if (args.size() > 4 && args[1].toString() == "set") profile = QJsonDocument::fromJson(args[4].toString().toUtf8()).object();
        }
        return profile;
    };
    QSignalSpy changed(&page, &MachinesPage::machinesChanged);
    // To WSL: the row stays, the SSH fields hide; the save carries WSL only.
    transport->setCurrentIndex(transport->findData("wsl-local"));
    QVERIFY(transport->isVisible());
    QVERIFY(hostname->isHidden());
    widget<QLineEdit>(page, "machineWslDistribution")->setText("Ubuntu-24.04");
    widget<QLineEdit>(page, "machineWslUser")->setText("alice");
    widget<QLineEdit>(page, "machineWslHgs")->setText("/home/alice/bin/hgs");
    auto *save = widget<QPushButton>(page, "saveMachine");
    QTRY_VERIFY(save->isEnabled()); save->click();
    QTRY_COMPARE(changed.size(), 1);
    QTRY_VERIFY(!save->isEnabled() && transport->isEnabled());
    QCOMPARE(lastSet()["transport"].toString(), QString("wsl-local"));
    QVERIFY(!lastSet().contains("hostname"));
    QCOMPARE(transport->currentData().toString(), QString("wsl-local"));
    QVERIFY(transport->isVisible());
    // Back to SSH: the overrides it had come back, and are saved again.
    transport->setCurrentIndex(transport->findData("ssh"));
    QVERIFY(!hostname->isHidden());
    QCOMPARE(hostname->text(), QString("ubuntu.example"));
    QCOMPARE(widget<QLineEdit>(page, "machineUser")->text(), QString("alice"));
    QCOMPARE(widget<QSpinBox>(page, "machinePort")->value(), 2222);
    QCOMPARE(widget<QLineEdit>(page, "machineKey")->text(), QString("~/.ssh/ubuntu"));
    QTRY_VERIFY(save->isEnabled()); save->click();
    QTRY_COMPARE(changed.size(), 2);
    QTRY_VERIFY(!save->isEnabled() && transport->isEnabled());
    const auto ssh = lastSet();
    QCOMPARE(ssh["hostname"].toString(), QString("ubuntu.example"));
    QCOMPARE(ssh["port"].toInt(), 2222);
    QVERIFY(!ssh.contains("wsl"));
    // And the WSL profile is kept for the next switch.
    transport->setCurrentIndex(transport->findData("wsl-local"));
    QCOMPARE(widget<QLineEdit>(page, "machineWslDistribution")->text(), QString("Ubuntu-24.04"));
    QCOMPARE(widget<QLineEdit>(page, "machineWslHgs")->text(), QString("/home/alice/bin/hgs"));
}

// #729 (#709 L8): the other type's overrides are saved through Settings; with
// the INI's lock held by another program the save is queued, and the machine
// shown again still reads them back (the queue is an overlay), not empty fields.
void TestMachinesPage::otherTransportSavedBehindAHeldLockReadsBack()
{
    flag("data.json", R"({"local":"arch","revision":"revision-1","machines":[{"alias":"ubuntu","connection":{"enabled":true,"hostname":"ubuntu.example","user":"alice","port":2222},"enabled":true,"source":"local"}]})");
    // Another computer's program holds the lock (a lock of this process would
    // be a leftover the writer removes).
    QDir().mkpath(QFileInfo(Settings::lockPath()).absolutePath());
    QFile lock(Settings::lockPath());
    QVERIFY(lock.open(QIODevice::WriteOnly));
    lock.write("4\nhgs-tray\nelsewhere.example\n");
    lock.close();
    // A failed check must not leave the lock for the next tests.
    // The writer may be reading the lock that instant, which fails a delete on
    // Windows: the release tries for a while.
    struct Release {
        bool done()
        {
            for (int attempt = 0; attempt < 100; ++attempt) {
                if (QFile::remove(Settings::lockPath()) || !QFileInfo::exists(Settings::lockPath())) return true;
                QThread::msleep(20);
            }
            return false;
        }
        ~Release() { done(); Settings::flush(); }
    } release;
    MachinesPage page(FakeCli::program(m_script)); start(page);
    page.selectMachine("ubuntu");
    auto *transport = widget<QComboBox>(page, "machineTransport");
    QTRY_VERIFY(transport->isVisible() && transport->isEnabled());
    transport->setCurrentIndex(transport->findData("wsl-local"));
    widget<QLineEdit>(page, "machineWslDistribution")->setText("Ubuntu-24.04");
    widget<QLineEdit>(page, "machineWslHgs")->setText("/home/alice/bin/hgs");
    transport->setCurrentIndex(transport->findData("ssh"));
    widget<QSpinBox>(page, "machinePort")->setValue(2223);
    auto *save = widget<QPushButton>(page, "saveMachine");
    QTRY_VERIFY(save->isEnabled()); save->click();
    QTRY_VERIFY(!save->isEnabled() && transport->isEnabled());
    QVERIFY(Settings::queued());
    // Not yet in the file: the save waits for the lock.
    QVERIFY(!QSettings().contains(QStringLiteral("machines/otherTransport/ubuntu")));
    // Shown again: the WSL overrides come back from the queued save.
    page.selectMachine({});
    page.selectMachine("ubuntu");
    QTRY_VERIFY(transport->isVisible() && transport->isEnabled());
    QCOMPARE(widget<QLineEdit>(page, "machineWslDistribution")->text(), QString("Ubuntu-24.04"));
    QCOMPARE(widget<QLineEdit>(page, "machineWslHgs")->text(), QString("/home/alice/bin/hgs"));
    // The lock goes; the save lands.
    QVERIFY(release.done());
    Settings::flush();
    QVERIFY(!Settings::queued());
    QVERIFY(QSettings().value(QStringLiteral("machines/otherTransport/ubuntu")).toByteArray().contains("Ubuntu-24.04"));
}

// #650 L4: a machine whose alias comes from HGS configuration says so on the
// page, not only in a tooltip; without HGS_PEERS no override line shows.
void TestMachinesPage::aConfiguredAliasSaysWhereItComesFrom()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    QVERIFY(!widget<QLabel>(page, "machinesPeersOverride")->isVisible());
    page.selectMachine("mac");
    auto *note = widget<QLabel>(page, "machineSourceNote");
    QTRY_VERIFY(note->isVisible());
    QVERIFY(note->text().contains("HGS configuration"));
    page.selectMachine({});
    QTRY_VERIFY(!note->isVisible());
    QVERIFY(!widget<QComboBox>(page, "machineTransport")->isVisible());
}

void TestMachinesPage::newMachineDraftCancelAndDiscard_data()
{
    QTest::addColumn<QString>("answer");
    for(const auto *answer:{"cancel","escape","discard"})QTest::newRow(answer)<<QString(answer);
}
void TestMachinesPage::newMachineDraftCancelAndDiscard()
{
    QFETCH(QString,answer);MachinesPage page(FakeCli::program(m_script));start(page);
    auto *list=widget<QListWidget>(page,"machineProfiles");auto *alias=widget<QLineEdit>(page,"machineAlias");
    // Only two rows fit, so the draft scrolls the list and discarding it
    // scrolls back while the click that asked is still pressed. Otherwise the
    // phone panel decides the height whenever its own requests finish.
    const int twoRows=list->sizeHintForRow(0)*2+2*list->frameWidth();list->setFixedHeight(twoRows);QTRY_COMPARE(list->height(),twoRows);
    widget<QPushButton>(page,"addMachine")->click();QCOMPARE(list->count(),3);
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(),QString("@new"));QTRY_VERIFY(list->verticalScrollBar()->value()>0);
    alias->setText("studio");widget<QLineEdit>(page,"machineHostname")->setText("draft.example");
    widget<QPushButton>(page,"addMachine")->click();QCOMPARE(list->count(),3);QCOMPARE(alias->text(),QString("studio"));
    FleetState state;BoxState local;local.host="arch";local.ok=true;state.setLocal(local,0);
    int prompts=0;bool draftSelectedDuringPoll=false;
    QTimer respond;respond.setInterval(5);
    connect(&respond,&QTimer::timeout,&page,[&]{
        auto *dialog=page.findChild<QMessageBox *>("unsavedMachineDialog");if(!dialog)return;
        respond.stop(); // macOS animates Escape's button click; repeated keys restart that timer.
        ++prompts;page.setFleet(state);
        draftSelectedDuringPoll=list->currentItem()&&list->currentItem()->data(Qt::UserRole)=="@new";
        if(answer=="escape")QTest::keyClick(dialog,Qt::Key_Escape);
        else dialog->button(answer=="discard"?QMessageBox::Discard:QMessageBox::Cancel)->click();
    });respond.start();
    QTest::mouseClick(list->viewport(),Qt::LeftButton,{},list->visualItemRect(list->item(1)).center());
    respond.stop();QCOMPARE(prompts,1);QVERIFY(draftSelectedDuringPoll);
    const bool discarded=answer=="discard";
    QCOMPARE(list->count(),discarded?2:3);QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(),discarded?QString("mac"):QString("@new"));
    // The press selects the row under its old position after the dialog; the
    // highlight returns to the current row once that press has finished.
    const auto highlighted=[list]{QStringList keys;for(auto *item:list->selectedItems())keys<<item->data(Qt::UserRole).toString();return keys;};
    QTRY_COMPARE(highlighted(),QStringList{list->currentItem()->data(Qt::UserRole).toString()});
    if(!discarded){
        QCOMPARE(alias->text(),QString("studio"));QCOMPARE(widget<QLineEdit>(page,"machineHostname")->text(),QString("draft.example"));
        QVERIFY(widget<QPushButton>(page,"saveMachine")->isEnabled());
        const auto preview=qEnvironmentVariable("HGS_MACHINES_PREVIEW");
        if(!preview.isEmpty()){QDir().mkpath(preview);QVERIFY(page.grab().save(preview+"/new-machine-draft.png"));}
        answer="discard";respond.start();widget<QPushButton>(page,"discardNewMachine")->click();respond.stop();
        QCOMPARE(list->count(),2);QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(),QString("@local"));
    }
    for(const auto &call:calls())QVERIFY(call.toArray().at(1)!="set");
    // An untouched draft can also be abandoned without a dialog.
    widget<QPushButton>(page,"addMachine")->click();QCOMPARE(list->count(),3);
    widget<QPushButton>(page,"discardNewMachine")->click();QCOMPARE(list->count(),2);
}

void TestMachinesPage::editsKeepDraftWhileFleetRefreshesAndFailuresAreVisible()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    QVERIFY(widget<QLineEdit>(page, "machineAlias")->isReadOnly());
    auto *hostname = widget<QLineEdit>(page, "machineHostname"); hostname->setText("changed.local");
    QVERIFY(!widget<QPushButton>(page, "checkMachine")->isEnabled());
    FleetState fleet; BoxState local; local.host = "arch"; local.ok = true; fleet.setLocal(local, 0);
    page.setFleet(fleet); QCOMPARE(hostname->text(), "changed.local");
    flag("stale"); widget<QPushButton>(page, "saveMachine")->click();
    QTRY_VERIFY(widget<QLabel>(page, "machineNotice")->text().contains("changed elsewhere"));
    QCOMPARE(hostname->text(), "changed.local");
    QVERIFY(widget<QPushButton>(page, "saveMachine")->isEnabled());
    QVERIFY(!widget<QPushButton>(page, "checkMachine")->isEnabled());
    for (const auto &call : calls()) if (call.toArray().at(1).toString() == "set") QVERIFY(!call.toArray().contains("--create"));
}
void TestMachinesPage::testConnectionAndOpenSshKeepSelectedAlias()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    auto *check = widget<QPushButton>(page, "checkMachine"); check->click();
    QVERIFY(!widget<QListWidget>(page, "machineProfiles")->isEnabled());
    QTRY_VERIFY(widget<QLabel>(page, "machineNotice")->text().contains("Darwin arm64"));
    QSignalSpy terminal(&page, &MachinesPage::sshTerminalRequested);
    widget<QPushButton>(page, "sshMachine")->click();
    QCOMPARE(terminal.size(), 1); QCOMPARE(terminal[0][0].toString(), "mac");
    QSignalSpy accounts(&page, &MachinesPage::accountsRequested); widget<QPushButton>(page, "machineAccounts")->click();
    QCOMPARE(accounts.size(), 1); QCOMPARE(accounts[0][0].toString(), "mac");
    flag("probe-mode"); check->click();
    QTRY_VERIFY(widget<QLabel>(page, "machineNotice")->text().contains("no setup report"));
}
void TestMachinesPage::installationStreamsOutputAndKeepsProfileLocked()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    QTimer::singleShot(0, &page, [&] {
        auto *dialog = page.findChild<QDialog *>("machineSetupDialog"); QVERIFY(dialog);
        dialog->findChild<QLineEdit *>("machineSetupSource")->setText(QString::fromUtf8("/tmp/source дерево"));
        dialog->accept();
    });
    QSignalSpy changed(&page, &MachinesPage::machinesChanged);
    widget<QPushButton>(page, "installMachine")->click();
    QVERIFY(!widget<QListWidget>(page, "machineProfiles")->isEnabled());
    QVERIFY(!widget<QPushButton>(page, "browseMachineKey")->isEnabled());
    QTRY_VERIFY(widget<QPlainTextEdit>(page, "machineSetupLog")->toPlainText().contains("Setup complete"));
    QTRY_COMPARE(changed.size(), 1);
    QTRY_VERIFY(widget<QLabel>(page, "machineNotice")->text().contains("Darwin arm64"));
    QJsonArray setup;
    for (const auto &call : calls()) if (call.toArray().at(1).toString() == "setup") setup = call.toArray();
    QCOMPARE(setup, QJsonArray({"machine", "setup", "mac", "--source", QString::fromUtf8("/tmp/source дерево")}));
}
void TestMachinesPage::effectiveValuesNeverBecomeOverridesOrReplaceDrafts()
{
    MachinesPage page(FakeCli::program(m_script)); start(page); flag("resolve-delay");
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    auto *hostname = widget<QLineEdit>(page, "machineHostname"); auto *effective = widget<QLabel>(page, "effectiveMachineConnection");
    QCOMPARE(hostname->text(), "");
    widget<QLineEdit>(page, "machineUser")->setText("draft-user");
    QTRY_VERIFY(effective->text().contains("mac.example"));
    QVERIFY(effective->text().contains("Unsaved edits")); QVERIFY(effective->text().contains("jump.example"));
    QCOMPARE(hostname->text(), ""); QVERIFY(hostname->placeholderText().contains("mac.example"));
    QCOMPARE(widget<QLineEdit>(page, "machineUser")->text(), "draft-user");
    QCOMPARE(widget<QSpinBox>(page, "machinePort")->value(), 0);
    widget<QPushButton>(page, "saveMachine")->click();
    QTRY_VERIFY(widget<QPushButton>(page, "checkMachine")->isEnabled());
    QJsonObject saved;
    for (const auto &call : calls()) if (call.toArray().at(1).toString() == "set") saved = QJsonDocument::fromJson(call.toArray().at(4).toString().toUtf8()).object();
    QCOMPARE(saved["hostname"].toString(), ""); QCOMPARE(saved["user"].toString(), "draft-user");
    QVERIFY(saved["port"].isNull()); QCOMPARE(saved["identity_file"].toString(), "");
}
void TestMachinesPage::staleResolutionCannotPopulateAnotherMachine()
{
    flag("resolve-delay");
    QFile file(m_dir.filePath("data.json")); QVERIFY(file.open(QIODevice::ReadOnly)); auto data = QJsonDocument::fromJson(file.readAll()).object(); file.close();
    auto machines = data["machines"].toArray(); machines.append(QJsonObject{{"alias","studio"},{"connection",QJsonObject{}},{"enabled",true},{"source","config"}}); data["machines"] = machines;
    QVERIFY(file.open(QIODevice::WriteOnly)); file.write(QJsonDocument(data).toJson()); file.close();
    MachinesPage page(FakeCli::program(m_script)); BoxState box; box.host = "arch"; box.ok = true; FleetState fleet; fleet.setLocal(box, 0); page.setFleet(fleet); page.show();
    auto *list = widget<QListWidget>(page, "machineProfiles"); QTRY_COMPARE(list->count(), 3);
    list->setCurrentRow(1); list->setCurrentRow(2);
    auto *effective = widget<QLabel>(page, "effectiveMachineConnection");
    QTRY_VERIFY(effective->text().contains("studio.example"));
    QTest::qWait(250); QVERIFY(!effective->text().contains("mac.example"));
    QCOMPARE(widget<QLineEdit>(page, "machineHostname")->text(), "");
    list->setCurrentRow(1); list->setCurrentRow(0); QTest::qWait(250);
    QVERIFY(effective->isHidden()); QCOMPARE(widget<QLineEdit>(page, "machineAlias")->text(), "arch");
}
void TestMachinesPage::colorsWorkForLocalAndPersistWithoutSavingConnection()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    auto *color = widget<QPushButton>(page, "machineColor"); auto *reset = widget<QPushButton>(page, "resetMachineColor");
    QVERIFY(color->isEnabled()); QVERIFY(reset->isEnabled());
    const QColor automatic = MachineAppearance::color("arch");
    QSignalSpy changed(&page, &MachinesPage::appearanceChanged);
    QTimer::singleShot(0, &page, [&] {
        auto *dialog = page.findChild<QColorDialog *>(); QVERIFY(dialog);
        dialog->setCurrentColor(QColor("#e05999")); dialog->accept();
    });
    color->click(); QCOMPARE(changed.size(), 1); QCOMPARE(MachineAppearance::color("arch"), QColor("#e05999"));
    QVERIFY(!widget<QPushButton>(page, "saveMachine")->isEnabled());
    auto *style = widget<QComboBox>(page, "machineLabelStyle");
    QCOMPARE(style->currentIndex(), 0); style->setCurrentIndex(1);
    QCOMPARE(changed.size(), 2); QVERIFY(MachineAppearance::vivid("arch"));
    QCOMPARE(MachineAppearance::backgroundColor(MachineAppearance::color("arch"), true, true), QColor("#e05999"));
    for (const auto &hex : {"#ffee00", "#0000ff", "#ff0080", "#777777", "#00ff00"}) {
        const QColor bg(hex), fg = MachineAppearance::textColor(bg, true, true);
        const double a = MachineAppearance::luminance(bg), b = MachineAppearance::luminance(fg);
        QVERIFY((qMax(a, b) + .05) / (qMin(a, b) + .05) >= 4.5);
    }
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1); QCOMPARE(style->currentIndex(), 0);
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(0); QCOMPARE(style->currentIndex(), 1);
    QCOMPARE(widget<QLabel>(page, "machineLabelPreview")->text(), "arch");
    reset->click(); QCOMPARE(changed.size(), 3); QCOMPARE(MachineAppearance::color("arch"), automatic);
    widget<QPushButton>(page, "addMachine")->click(); QVERIFY(!color->isEnabled());
    for (const auto &call : calls()) QVERIFY(call.toArray().at(1).toString() != "set");
}
void TestMachinesPage::colorPickerDismissalKeepsTheSavedColor_data()
{
    QTest::addColumn<QString>("action");
    for (const auto &action : {"outside-left", "outside-right", "escape", "cancel"}) QTest::newRow(action) << QString(action);
}
void TestMachinesPage::colorPickerDismissalKeepsTheSavedColor()
{
    QFETCH(QString, action);
    MachinesPage page(FakeCli::program(m_script)); start(page); page.resize(1100,760);QVERIFY(QTest::qWaitForWindowExposed(&page));
    const auto original=MachineAppearance::color("arch");
    QSignalSpy changed(&page,&MachinesPage::appearanceChanged);
    auto *outside=widget<QPushButton>(page,"addMachine");QSignalSpy outsideClicked(outside,&QPushButton::clicked);
    bool expired=false,handled=false;
    QTimer::singleShot(0,&page,[&]{
        auto *dialog=page.findChild<QColorDialog *>();QVERIFY(dialog);
        QTimer::singleShot(5000,dialog,[&,dialog]{expired=true;dialog->reject();});
        dialog->setCurrentColor(QColor("#e05999"));
        QTest::mouseClick(dialog,Qt::LeftButton,Qt::NoModifier,QPoint(8,8));QVERIFY(dialog->isVisible());
        if(action.startsWith("outside")){
            const auto global=outside->mapToGlobal(outside->rect().center());QVERIFY(!dialog->frameGeometry().contains(global));
            QVERIFY(PopupMouse::click(&page,action=="outside-right"?Qt::RightButton:Qt::LeftButton,page.mapFromGlobal(global)));
        }else if(action=="escape")QTest::keyClick(dialog,Qt::Key_Escape);
        else dialog->findChild<QDialogButtonBox *>()->button(QDialogButtonBox::Cancel)->click();
        QTRY_VERIFY(!dialog->isVisible());handled=true;
    });
    widget<QPushButton>(page,"machineColor")->click();
    QVERIFY(handled);QVERIFY(!expired);QCoreApplication::processEvents();
    QCOMPARE(MachineAppearance::color("arch"),original);QCOMPARE(changed.size(),0);QCOMPARE(outsideClicked.size(),0);
    QVERIFY(!Settings::contains("machines/colors/arch"));
    QCOMPARE(widget<QLineEdit>(page,"machineAlias")->text(),QString("arch"));
    QVERIFY(!widget<QPushButton>(page,"saveMachine")->isEnabled());
    for(const auto &call:calls())QVERIFY(call.toArray().at(1).toString()!="set");
    if(action.startsWith("outside")){
        QVERIFY(PopupMouse::click(&page,Qt::LeftButton,outside->mapTo(&page,outside->rect().center())));
        QTRY_COMPARE(outsideClicked.size(),1); // The native stimulus reaches the owner after dismissal.
    }
}
void TestMachinesPage::dashboardSelectionWaitsForProfiles()
{
    flag("list-delay"); MachinesPage page(FakeCli::program(m_script));
    BoxState local; local.host = "arch"; local.ok = true; FleetState fleet; fleet.setLocal(local, 0); page.setFleet(fleet);
    page.show(); page.selectMachine("mac");
    QTRY_COMPARE(widget<QLineEdit>(page, "machineAlias")->text(), "mac");
    QCOMPARE(widget<QListWidget>(page, "machineProfiles")->currentRow(), 1);
    page.selectMachine("arch"); QCOMPARE(widget<QListWidget>(page, "machineProfiles")->currentRow(), 0);
    page.selectMachine("mac"); page.selectMachine({}); QCOMPARE(widget<QListWidget>(page, "machineProfiles")->currentRow(), 0);
}
// U25 part 7 (spec 8): unsaved edits show a dot in the list and a warning, and
// keep test, SSH, install, reset and remove off until saved or discarded;
// Discard changes asks first; the label takes a swatch and Soft / Bright; a
// new draft names a duplicate and cannot be saved.
void TestMachinesPage::unsavedEditsGateActionsAndTheLabelHasSwatches()
{
    MachinesPage page(FakeCli::program(m_script)); start(page); page.resize(1200, 900); page.show();
    auto *list = widget<QListWidget>(page, "machineProfiles");
    list->setCurrentRow(1);
    const QString machine = list->currentItem()->data(Qt::UserRole).toString();
    QTRY_VERIFY(widget<QLineEdit>(page, "machineHostname")->isEnabled());
    auto *discard = widget<QPushButton>(page, "discardNewMachine");
    auto *notice = widget<QLabel>(page, "machineUnsavedNotice");
    QVERIFY(discard->isVisible()); QCOMPARE(discard->text(), QString("Discard changes")); QVERIFY(!discard->isEnabled());
    QVERIFY(!notice->isVisible()); QVERIFY(!list->currentItem()->data(Qt::UserRole + 1).toBool());
    // An edit: the dot, the warning, Save on, the rest off with the reason.
    widget<QLineEdit>(page, "machineHostname")->setText("10.0.4.21");
    QVERIFY(list->currentItem()->data(Qt::UserRole + 1).toBool());
    QVERIFY(notice->isVisible());
    QCOMPARE(notice->text(), QString("Unsaved changes — test, SSH and install are off until you save or discard."));
    QVERIFY(widget<QPushButton>(page, "saveMachine")->isEnabled()); QVERIFY(discard->isEnabled());
    for (const char *name : {"checkMachine", "sshMachine", "installMachine", "resetMachine", "removeMachine"}) {
        QVERIFY2(!widget<QPushButton>(page, name)->isEnabled(), name);
        QCOMPARE(widget<QPushButton>(page, name)->toolTip(), QString("Save or discard the connection first"));
    }
    PageShots::shot(page, true, "ux-7-machines-dirty-dark");
    PageShots::shot(page, false, "ux-7-machines-dirty-light");
    // Discard changes asks, then reloads the saved values.
    bool asked = false, answerFound = false;
    QString prompt;
    QTimer::singleShot(0, &page, [&] {
        auto *dialog = page.findChild<QMessageBox *>("unsavedMachineDialog");
        if (!dialog) { if (auto *modal=qobject_cast<QDialog *>(QApplication::activeModalWidget())) modal->reject(); return; }
        asked = true; prompt = dialog->text();
        auto *answer = dialog->button(QMessageBox::Discard);
        answerFound = answer != nullptr;
        if (answer) answer->click(); else dialog->reject();
    });
    discard->click();
    QVERIFY(asked); QVERIFY(answerFound);
    QCOMPARE(prompt, QString("Discard the unsaved changes to this connection?"));
    QVERIFY(widget<QLineEdit>(page, "machineHostname")->text() != "10.0.4.21");
    QVERIFY(!notice->isVisible()); QVERIFY(!list->currentItem()->data(Qt::UserRole + 1).toBool());
    QVERIFY(widget<QPushButton>(page, "checkMachine")->isEnabled());
    QVERIFY(widget<QPushButton>(page, "checkMachine")->toolTip().isEmpty());
    // The label: a swatch and the Bright segment, saved on this device only.
    QList<QPushButton *> swatches;
    for (auto *swatch : page.findChildren<QPushButton *>("machineSwatch")) swatches << swatch;
    QCOMPARE(swatches.size(), 7);
    swatches.at(2)->click();
    QCOMPARE(MachineAppearance::color(machine), QColor(swatches.at(2)->property("color").toString()));
    QVERIFY(swatches.at(2)->isChecked() && !swatches.at(0)->isChecked());
    widget<QPushButton>(page, "machineLabelBright")->click();
    QVERIFY(MachineAppearance::vivid(machine)); QCOMPARE(widget<QComboBox>(page, "machineLabelStyle")->currentIndex(), 1);
    QVERIFY(widget<QPushButton>(page, "machineLabelBright")->isChecked() && !widget<QPushButton>(page, "machineLabelSoft")->isChecked());
    QVERIFY(!widget<QPushButton>(page, "saveMachine")->isEnabled());
    // A new draft: its note, Discard draft, and a duplicate name refused.
    widget<QPushButton>(page, "addMachine")->click();
    QCOMPARE(discard->text(), QString("Discard draft"));
    auto *alias = widget<QLineEdit>(page, "machineAlias");
    alias->setText(machine);
    QVERIFY(widget<QLabel>(page, "machineAliasError")->isVisible());
    QCOMPARE(widget<QLabel>(page, "machineAliasError")->text(), QString("This name already exists"));
    QVERIFY(alias->property("pageError").toBool());
    QVERIFY(!widget<QPushButton>(page, "saveMachine")->isEnabled());
    alias->setText("build-03"); widget<QLineEdit>(page, "machineHostname")->setText("10.0.4.23");
    QVERIFY(!widget<QLabel>(page, "machineAliasError")->isVisible()); QVERIFY(widget<QPushButton>(page, "saveMachine")->isEnabled());
    QVERIFY(notice->isVisible());
    PageShots::shot(page, true, "ux-7-machines-new-draft-dark");
    PageShots::shot(page, false, "ux-7-machines-new-draft-light");
}

// N20 (#386): every rebuild of the rows keeps the amber dot while the edits
// stay unsaved - a fleet poll, a swatch, Bright, Soft, Automatic and a
// cancelled switch to another machine.
void TestMachinesPage::unsavedDotSurvivesRowRebuilds()
{
    MachinesPage page(FakeCli::program(m_script)); start(page); page.resize(1200, 900); page.show();
    auto *list = widget<QListWidget>(page, "machineProfiles");
    list->setCurrentRow(1);
    const QString machine = list->currentItem()->data(Qt::UserRole).toString();
    QTRY_VERIFY(widget<QLineEdit>(page, "machineHostname")->isEnabled());
    const auto dot = [&] {
        for (int i = 0; i < list->count(); ++i)
            if (list->item(i)->data(Qt::UserRole).toString() == machine) return list->item(i)->data(Qt::UserRole + 1).toBool();
        return false;
    };
    const auto dots = [&] { int n = 0; for (int i = 0; i < list->count(); ++i) n += list->item(i)->data(Qt::UserRole + 1).toBool(); return n; };
    widget<QLineEdit>(page, "machineHostname")->setText("10.0.4.21");
    QVERIFY(dot());
    // A fleet poll rebuilds the rows.
    BoxState local; local.host = "arch"; local.ok = true; FleetState fleet; fleet.setLocal(local, 1); page.setFleet(fleet);
    QVERIFY(dot()); QCOMPARE(dots(), 1);
    // The label's swatch, Bright, Soft and Automatic.
    page.findChildren<QPushButton *>("machineSwatch").at(3)->click(); QVERIFY(dot());
    widget<QPushButton>(page, "machineLabelBright")->click(); QVERIFY(dot());
    widget<QPushButton>(page, "machineLabelSoft")->click(); QVERIFY(dot());
    widget<QPushButton>(page, "resetMachineColor")->click(); QVERIFY(dot());
    // Another machine, then Cancel on the prompt: still the same edits.
    bool asked = false;
    QTimer::singleShot(0, &page, [&] {
        auto *dialog = page.findChild<QMessageBox *>("unsavedMachineDialog"); QVERIFY(dialog);
        asked = true; dialog->button(QMessageBox::Cancel)->click();
    });
    list->setCurrentRow(0);
    QVERIFY(asked);
    QCOMPARE(list->currentItem()->data(Qt::UserRole).toString(), machine);
    QVERIFY(dot()); QCOMPARE(dots(), 1);
    QCOMPARE(widget<QLineEdit>(page, "machineHostname")->text(), QString("10.0.4.21"));
    QVERIFY(widget<QLabel>(page, "machineUnsavedNotice")->isVisible());
}

void TestMachinesPage::preview()
{
    const auto directory = qEnvironmentVariable("HGS_PREVIEW_DIR");
    if (directory.isEmpty()) QSKIP("Set HGS_PREVIEW_DIR to render the machines preview");
    QDir().mkpath(directory);
    MachinesPage page(FakeCli::program(m_script)); start(page); page.resize(1000, 680); page.setTheme(true);
    page.setStyleSheet("QWidget#machinesPage { background:#171e24; color:#e8edf4; } "
        "QLabel,QCheckBox { color:#e8edf4; } QLabel#muted { color:#a1adbb; } QLabel#heading { font-size:20px; } "
        "QLineEdit,QSpinBox,QListWidget,QPlainTextEdit { background:#1e262e; color:#e8edf4; border:1px solid #34414d; border-radius:6px; padding:6px; } "
        "QLineEdit:disabled,QSpinBox:disabled { color:#647386; } QListWidget::item { padding:10px; } "
        "QListWidget::item:selected { background:#233a35; } QPushButton { color:#e8edf4; background:#1e262e; border:1px solid #34414d; border-radius:6px; padding:7px 10px; } "
        "QPushButton:disabled { color:#647386; } QPushButton#saveMachine:enabled { background:#89d9bf; color:#142821; }");
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    MachineAppearance::setColor("mac", QColor("#ffda19"));
    widget<QComboBox>(page, "machineLabelStyle")->setCurrentIndex(1);
    widget<QLineEdit>(page, "machineHostname")->setText("studio-mac.local");
    widget<QLineEdit>(page, "machineUser")->setText("example");
    widget<QLineEdit>(page, "machineKey")->setText("~/.ssh/id_ed25519");
    QTest::qWait(30); QVERIFY(page.grab().save(directory + "/machines-dark.png"));
}
void TestMachinesPage::controlOnlyComputerIsAConsoleWithoutSessions()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    QVERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    BoxState local; local.host = "arch"; local.ok = true; local.localSessions = false;
    FleetState fleet; fleet.setLocal(local, 0); page.setFleet(fleet);
    auto *list = widget<QListWidget>(page, "machineProfiles");
    QCOMPARE(list->item(0)->text(), QString("arch\nThis computer: control console"));
    QVERIFY(!widget<QPushButton>(page, "machineSessions")->isVisible());
    QVERIFY(!widget<QPushButton>(page, "machineAccounts")->isVisible());
    bool console = false, counted = false;
    for (auto *label : page.findChildren<QLabel *>("muted")) {
        console |= label->text().contains("control console");
        // As on Overview, the console itself is not a machine of the fleet.
        counted |= label->text() == "SSH connections and HGS setup · 0 connected, 1 machine";
    }
    QVERIFY(console); QVERIFY(counted);
    // It has no SSH connection to edit, test or remove; only its color.
    QVERIFY(!widget<QLineEdit>(page, "machineAlias")->isVisible());
    QVERIFY(!widget<QLineEdit>(page, "machineHostname")->isVisible());
    QVERIFY(!widget<QCheckBox>(page, "machineEnabled")->isVisible());
    for (const char *name : {"saveMachine", "checkMachine", "sshMachine", "installMachine", "resetMachine", "removeMachine"})
        QVERIFY2(!widget<QPushButton>(page, name)->isVisible(), name);
    QVERIFY(widget<QPushButton>(page, "machineColor")->isVisible());
    // Remote machines keep every action.
    list->setCurrentRow(1);
    QTRY_VERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    QVERIFY(widget<QPushButton>(page, "machineAccounts")->isVisible());
    QVERIFY(widget<QLineEdit>(page, "machineHostname")->isVisible());
    QVERIFY(widget<QCheckBox>(page, "machineEnabled")->isVisible());
    for (const char *name : {"saveMachine", "checkMachine", "sshMachine", "installMachine", "removeMachine"})
        QVERIFY2(widget<QPushButton>(page, name)->isVisible(), name);
}

namespace {
// This computer as a Windows CLI reports it (spec D2): a control console until
// local sessions are enabled, with its session host.
FleetState windowsComputer(bool enabled, const QJsonObject &host, const QString &platform = "windows")
{
    BoxState local; local.host = "windows-pc"; local.ok = true; local.localSessions = enabled; local.localHost = host;
    if (!platform.isEmpty()) local.metrics = {{"platform", platform}};
    FleetState fleet; fleet.setLocal(local, 0); return fleet;
}
QJsonObject sessionHost(const QString &state)
{
    return {{"state", state}, {"pid", QJsonValue::Null}, {"protocol", QJsonValue::Null}, {"conpty", QJsonValue::Null},
            {"integrity", QJsonValue::Null}, {"windows_session", QJsonValue::Null}};
}
}

void TestMachinesPage::countsReadInSingularAndPlural()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    auto *list = widget<QListWidget>(page, "machineProfiles");
    const auto summary = [&] {
        QStringList texts; for (auto *label : page.findChildren<QLabel *>("muted")) texts << label->text(); return texts;
    };
    QVERIFY2(summary().contains("SSH connections and HGS setup · 1 connected, 2 machines"), qPrintable(summary().join(" | ")));
    QCOMPARE(list->item(1)->text(), QString("mac\nNot connected"));
    BoxState local; local.host = "arch"; local.ok = true;
    BoxState mac; mac.host = "mac"; mac.ok = true; mac.sessions.resize(1);
    FleetState fleet; fleet.setLocal(local, 0); fleet.setPeer(mac, QDateTime::currentMSecsSinceEpoch()); page.setFleet(fleet);
    QVERIFY2(summary().contains("SSH connections and HGS setup · 2 connected, 2 machines"), qPrintable(summary().join(" | ")));
    QCOMPARE(list->item(1)->text(), QString("mac\nConnected, 1 session"));
    mac.sessions.resize(2); fleet.setPeer(mac, QDateTime::currentMSecsSinceEpoch()); page.setFleet(fleet);
    QCOMPARE(list->item(1)->text(), QString("mac\nConnected, 2 sessions"));
    // A control console leaves one machine of the fleet.
    local.localSessions = false; fleet.setLocal(local, 0); page.setFleet(fleet);
    QVERIFY2(summary().contains("SSH connections and HGS setup · 1 connected, 1 machine"), qPrintable(summary().join(" | ")));
}
void TestMachinesPage::hiddenPageRendersFleetChangesWhenShown()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    auto *list = widget<QListWidget>(page, "machineProfiles");
    QCOMPARE(list->item(1)->text(), QString("mac\nNot connected"));
    page.hide();
    BoxState local; local.host = "arch"; local.ok = true;
    BoxState mac; mac.host = "mac"; mac.ok = true; mac.sessions.resize(1);
    FleetState fleet; fleet.setLocal(local, 0); fleet.setPeer(mac, QDateTime::currentMSecsSinceEpoch());
    page.setFleet(fleet); page.setFleet(fleet);
    QCOMPARE(list->item(1)->text(), QString("mac\nNot connected"));
    page.show();
    QCOMPARE(list->item(1)->text(), QString("mac\nConnected, 1 session"));
    // A shown page renders each change at once.
    mac.sessions.resize(2); fleet.setPeer(mac, QDateTime::currentMSecsSinceEpoch()); page.setFleet(fleet);
    QCOMPARE(list->item(1)->text(), QString("mac\nConnected, 2 sessions"));
}
void TestMachinesPage::windowsConsoleEnablesLocalSessions()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    page.setFleet(windowsComputer(false, sessionHost("not-installed")));
    auto *enable = widget<QPushButton>(page, "enableLocalSessions"); auto *notice = widget<QLabel>(page, "machineNotice");
    QVERIFY(enable->isVisible()); QVERIFY(enable->isEnabled()); QCOMPARE(enable->text(), QString("Enable local sessions"));
    QSignalSpy changed(&page, &MachinesPage::machinesChanged);
    const auto installs = [&] {
        int count = 0; for (const auto &call : calls()) count += call.toArray().at(0) == "session-host";
        return count;
    };
    // Enabling writes into the profile: it asks first, and Cancel is the default.
    const auto answer = [&](bool proceed) {
        bool shown = false; QTimer respond; respond.setInterval(5);
        connect(&respond, &QTimer::timeout, &page, [&] {
            auto *box = page.findChild<QMessageBox *>("enableLocalSessionsConfirm"); if (!box || !box->isVisible()) return;
            respond.stop(); shown = true;
            QCOMPARE(box->defaultButton(), qobject_cast<QPushButton *>(box->button(QMessageBox::Cancel)));
            QVERIFY(box->text().contains("hgs session-host uninstall")); QVERIFY(!box->text().contains("preview", Qt::CaseInsensitive));
            if (!proceed) { QTest::keyClick(box, Qt::Key_Escape); return; }
            for (auto *button : box->buttons()) if (box->buttonRole(button) == QMessageBox::AcceptRole) button->click();
        });
        respond.start(); enable->click(); respond.stop(); QVERIFY(shown);
    };
    answer(false); QTest::qWait(100); QCOMPARE(installs(), 0); QVERIFY(changed.isEmpty());
    // A failure is reported and changes nothing.
    flag("host-mode", "fail"); answer(true);
    QVERIFY(!enable->isEnabled()); QVERIFY(!widget<QListWidget>(page, "machineProfiles")->isEnabled());
    QTRY_VERIFY(notice->text().contains("access denied")); QVERIFY(notice->styleSheet().contains("color"));
    QVERIFY(enable->isEnabled()); QCOMPARE(installs(), 1); QVERIFY(changed.isEmpty());
    QFile::remove(m_dir.filePath("host-mode")); answer(true);
    QTRY_COMPARE(changed.size(), 1); QCOMPARE(installs(), 2);
    QVERIFY(notice->text().contains("Local sessions are enabled")); QVERIFY(notice->text().contains("next start"));
    QVERIFY(notice->text().contains("C:\\Users\\user\\.local\\bin\\hgs.exe")); QVERIFY(notice->styleSheet().isEmpty());
    for (const auto &call : calls())
        if (call.toArray().at(0) == "session-host") QCOMPARE(call.toArray(), QJsonArray({"session-host", "install", "--json"}));
    // The next poll reports an enabled computer: no second offer, and the notice
    // that the stable CLI follows from the next start stays.
    page.setFleet(windowsComputer(true, sessionHost("stopped")));
    QVERIFY(!enable->isVisible()); QVERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    QVERIFY(notice->text().contains("next start"));
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1); QTRY_VERIFY(!notice->text().contains("next start"));
}

void TestMachinesPage::enablingIsOfferedOnlyToAWindowsConsole()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    auto *enable = widget<QPushButton>(page, "enableLocalSessions"); auto *host = widget<QLabel>(page, "localSessionHost");
    const auto origin = [&] {
        for (auto *label : page.findChildren<QLabel *>("muted"))
            if (label->isVisible() && label->text().startsWith("This computer is a control console")) return label->text();
        return QString();
    };
    const QString remote("This computer is a control console: agents run on the remote machines you add here, over SSH.");
    // Linux, macOS and an older Windows CLI without local_host offer nothing.
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible());
    page.setFleet(windowsComputer(false, sessionHost("not-installed"), "linux"));
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible()); QCOMPARE(origin(), remote);
    page.setFleet(windowsComputer(false, sessionHost("not-installed"), "macos"));
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible()); QCOMPARE(origin(), remote);
    page.setFleet(windowsComputer(false, {}));
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible()); QCOMPARE(origin(), remote);
    // Before the first answer names the platform, this GUI's own platform counts.
    page.setFleet(windowsComputer(false, sessionHost("not-installed"), {}));
    QCOMPARE(enable->isVisible(), MachinePlatform::build() == MachinePlatform::Style::Windows);
    page.setFleet(windowsComputer(false, sessionHost("not-installed")));
    QVERIFY(enable->isVisible()); QVERIFY(host->isVisible());
    // The offer is named where the console explains where agents run.
    QCOMPARE(origin(), remote + " Enable local sessions to run them on this computer too.");
    page.setFleet(windowsComputer(false, {})); QCOMPARE(origin(), remote);
    page.setFleet(windowsComputer(false, sessionHost("not-installed")));
    // An enabled computer shows its host without the offer; a remote machine neither.
    page.setFleet(windowsComputer(true, sessionHost("stopped")));
    QVERIFY(!enable->isVisible()); QVERIFY(host->isVisible());
    page.setFleet(windowsComputer(false, sessionHost("not-installed")));
    widget<QListWidget>(page, "machineProfiles")->setCurrentRow(1);
    QTRY_VERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible());
    // A Unix computer's page keeps its actions.
    page.setFleet(windowsComputer(true, {}, "linux")); widget<QListWidget>(page, "machineProfiles")->setCurrentRow(0);
    QVERIFY(!enable->isVisible()); QVERIFY(!host->isVisible());
    QVERIFY(widget<QPushButton>(page, "machineSessions")->isVisible()); QVERIFY(widget<QPushButton>(page, "machineAccounts")->isVisible());
}

void TestMachinesPage::windowsComputerPageHasNoSshConnection()
{
    MachinesPage page(FakeCli::program(m_script)); start(page);
    auto *list = widget<QListWidget>(page, "machineProfiles");
    const QList<const char *> connection{"saveMachine", "checkMachine", "sshMachine", "installMachine", "resetMachine", "removeMachine"};
    // A Windows computer with local sessions is never reached over SSH: its own page
    // keeps sessions, accounts, the session host and its color, without SSH fields.
    page.setFleet(windowsComputer(true, sessionHost("stopped"))); list->setCurrentRow(0);
    QTRY_VERIFY(widget<QLabel>(page, "localSessionHost")->isVisible());
    QVERIFY(!widget<QLineEdit>(page, "machineAlias")->isVisible());
    QVERIFY(!widget<QLineEdit>(page, "machineHostname")->isVisible());
    QVERIFY(!widget<QCheckBox>(page, "machineEnabled")->isVisible());
    for (const char *name : connection) QVERIFY2(!widget<QPushButton>(page, name)->isVisible(), name);
    QVERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    QVERIFY(widget<QPushButton>(page, "machineAccounts")->isVisible());
    QVERIFY(widget<QPushButton>(page, "machineColor")->isVisible());
    // A remote machine keeps every action.
    list->setCurrentRow(1);
    QTRY_VERIFY(widget<QLineEdit>(page, "machineHostname")->isVisible());
    for (const char *name : {"saveMachine", "checkMachine", "sshMachine", "installMachine", "removeMachine"})
        QVERIFY2(widget<QPushButton>(page, name)->isVisible(), name);
    // This computer on Linux and macOS is unchanged.
    for (const QString platform : {"linux", "macos"}) {
        page.setFleet(windowsComputer(true, {}, platform)); list->setCurrentRow(0);
        QTRY_VERIFY(widget<QLineEdit>(page, "machineHostname")->isVisible());
        QVERIFY(widget<QLineEdit>(page, "machineAlias")->isVisible());
        for (const char *name : connection) QVERIFY2(widget<QPushButton>(page, name)->isVisible(), name);
        list->setCurrentRow(1); QTRY_VERIFY(widget<QPushButton>(page, "machineSessions")->isVisible());
    }
}

void TestMachinesPage::localSessionHostStates_data()
{
    QTest::addColumn<bool>("enabled");
    QTest::addColumn<QJsonObject>("host");
    QTest::addColumn<QString>("text");
    QTest::newRow("not installed") << false << sessionHost("not-installed") << "Session host: not installed";
    QTest::newRow("stopped") << true << sessionHost("stopped") << "Session host: stopped";
    QTest::newRow("running") << true << QJsonObject{{"state", "running"}, {"pid", 4242}, {"protocol", 1}, {"conpty", "bundled"},
        {"integrity", "medium"}, {"windows_session", 1}}
        << "Session host: running, PID 4242, bundled ConPTY, medium integrity, Windows session 1";
    QTest::newRow("inbox") << true << QJsonObject{{"state", "running"}, {"pid", 77}, {"protocol", 1}, {"conpty", "inbox"},
        {"integrity", "high"}, {"windows_session", 0}}
        << "Session host: running, PID 77, inbox ConPTY, high integrity, Windows session 0";
    auto unavailable = sessionHost("unavailable"); unavailable["error"] = "the session host on this computer runs elevated";
    QTest::newRow("unavailable") << true << unavailable << "Session host: unavailable: the session host on this computer runs elevated";
    QTest::newRow("console unavailable") << false << unavailable << "Session host: unavailable: the session host on this computer runs elevated";
}

void TestMachinesPage::localSessionHostStates()
{
    QFETCH(bool, enabled); QFETCH(QJsonObject, host); QFETCH(QString, text);
    MachinesPage page(FakeCli::program(m_script)); start(page);
    page.setFleet(windowsComputer(enabled, host));
    auto *label = widget<QLabel>(page, "localSessionHost");
    QVERIFY(label->isVisible()); QCOMPARE(label->text(), text);
    QCOMPARE(widget<QPushButton>(page, "enableLocalSessions")->isVisible(), !enabled);
    // Each poll shows the host as found.
    page.setFleet(windowsComputer(enabled, sessionHost("stopped"))); QCOMPARE(label->text(), QString("Session host: stopped"));
}

QTEST_MAIN(TestMachinesPage)
#include "test_machinespage.moc"
