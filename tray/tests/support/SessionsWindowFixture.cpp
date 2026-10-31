#include "SessionsWindowFixture.h"
#include "ComposerDraftStore.h"
#include "Settings.h"

void SessionsWindowFixture::init()
{
    // The settings, and the composer drafts kept beside them.
    Settings::flush();
    ComposerDraftStore::flushWrites();
    QDir(ComposerDraftStore::directory()).removeRecursively();
    clearSettings();
}

void SessionsWindowFixture::initTestCase()
{
    // Most checks read Sessions rows right after setFleet or an action: rows are
    // prepared on the preparation thread while the GUI thread waits. The
    // rowPreparation* tests cover the asynchronous application default.
    SessionsWindow::setRowPreparationForTests(SessionsWindow::RowPreparation::Blocking);
    MessageBoxIcons::install();
    // #525: a pending Activity render is work in flight for the waits. A hidden
    // view stays dirty until it is shown, so only a visible one counts.
    TestWait::addProbe([] {
        for (auto *widget : QApplication::allWidgets())
            if (auto *activity = qobject_cast<ActivityView *>(widget); activity && activity->isVisible() && activity->renderPending()) return true;
        return false;
    });

    m_dir = std::make_unique<QTemporaryDir>(); QVERIFY(m_dir->isValid());
    QCoreApplication::setOrganizationName("hgs-tests");
    QCoreApplication::setApplicationName("sessions-window");
    QGuiApplication::setDesktopFileName("org.example.ZerusSessionsWindowTest");
    QCoreApplication::setApplicationVersion(QLatin1String(HGS_TRAY_VERSION));
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_dir->path());
    WorkspaceSkin::preload();
    QVERIFY(FakeCli::install(script(), R"PY(#!/usr/bin/env python3
import json,sys
for value in sys.argv[1:]:print(value,file=sys.stderr)
a=sys.argv[1:]
if a and a[0].startswith('@'):a=a[1:]
c=a[0] if a else ''
if False:pass
elif c=='account' and a[1:2]==['ls']:
 print(json.dumps({'profiles':[{'id':'native-'+p,'provider':p,'label':'Default account','home':'/profiles/'+p,'native':True,'installed':True,'is_default':True}
                              for p in ['codex','claude','kimi','dsh','opencode']]}))
elif c=='inspect':print('{"tracked":true,"conversation_id":"conversation-one","events":[],"cursor":0}')
elif c=='project':
 if a[1:2]==['ls']:print('[{"name":"infra","dir":"/remote/infra","exists":true,"src":"ansible"}]')
elif c=='worktrees':sys.stdout.write(open(__file__+'.worktrees').read())
elif c=='dirs':print('{"path":"/remote/work tree","parent":"/remote","home":"/remote","directories":[{"name":"docs","path":"/remote/work tree/docs"}]}')
elif c=='ls':print('{"host":"mac","ok":true,"sessions":[]}')
)PY"));
    QFile catalogFile(script()+".worktrees");QVERIFY(catalogFile.open(QIODevice::WriteOnly));
    auto catalog=QJsonDocument::fromJson(R"({"state":"ok","path":"/repo","selected_root":"/repo","common_dir":"/repo/.git","worktrees":[{"path":"/repo","kind":"main","branch":"main","available":true},{"path":"/linked","kind":"linked","branch":"feat/worktree-catalog","available":true},{"path":"/empty","kind":"linked","branch":"fix/empty-copy","available":true}]})").object();catalog["sampled_at"]=QDateTime::currentSecsSinceEpoch();catalogFile.write(QJsonDocument(catalog).toJson());
}

QString SessionsWindowFixture::programInspecting(const QString &name, const QJsonObject &details, const QJsonObject &child, const QJsonObject &usage) const
{
    QFile original(script()); if (!original.open(QIODevice::ReadOnly)) return {};
    QByteArray answer;
    if (!child.isEmpty()) answer = "elif c=='inspect' and '--agent' in a:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument(child).toJson(QJsonDocument::Compact))) + ")\n";
    answer += "elif c=='inspect':print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument(details).toJson(QJsonDocument::Compact))) + ")\n";
    if (!usage.isEmpty()) answer += "elif c=='account' and a[1:2]==['inspect']:print(" + FakeCli::pythonString(QString::fromUtf8(QJsonDocument(usage).toJson(QJsonDocument::Compact))) + ")\n";
    const QString fixture = m_dir->filePath(name);
    return FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)) ? FakeCli::program(fixture) : QString();
}

FleetState SessionsWindowFixture::openCodeSelectionFleet() const
{
    SessionInfo opencode; opencode.cmd = "opencode"; opencode.project = "project"; opencode.tracked = true;
    opencode.state = "running"; opencode.activity = "idle"; opencode.phase = "idle"; opencode.processState = "running";
    opencode.runtimeState = "live"; opencode.terminalSupported = false;
    opencode.actionsReported = true; opencode.allowedActions = {"rename", "forget"};
    auto first = opencode; first.name = "opencode/project/first"; first.tag = "first";
    first.runId = "run-first"; first.conversationId = "ses_first";
    auto second = opencode; second.name = "opencode/project/second"; second.tag = "second";
    second.runId = "run-second"; second.conversationId = "ses_second";
    SessionInfo stopped; stopped.name = "codex/project/stopped"; stopped.cmd = "codex"; stopped.project = "project"; stopped.tag = "stopped";
    stopped.tracked = true; stopped.resumable = true; stopped.state = "stopped"; stopped.activity = "idle"; stopped.phase = "idle";
    auto result = fleet(); auto box = result.local(); box.sessions = {first, second, stopped};
    result.setLocal(box, QDateTime::currentMSecsSinceEpoch()); return result;
}

QString SessionsWindowFixture::openCodeSelectionFixture(const QString &name) const
{
    QFile original(script()); if (!original.open(QIODevice::ReadOnly)) return {};
    const QByteArray answer = "elif c=='session-action':\n"
        " import pathlib;p=json.load(sys.stdin)\n"
        " open(__file__+'.forget','a').write(json.dumps({'args':sys.argv[1:],'payload':p})+'\\n')\n"
        " r={'request_id':p['request_id'],'name':a[1],'run_id':p['expected_run_id'],'conversation_id':p['expected_conversation_id'],'status':'completed'}\n"
        " outcome=pathlib.Path(__file__+'.outcome')\n"
        " refused=outcome.read_text().split('\\n') if outcome.exists() else []\n"
        " if refused[:1]==[a[1]]:r.update(status=refused[1],error='This action is unavailable for the exact OpenCode session')\n"
        " print(json.dumps(r))\n"
        "elif c in ('kill','terminate'):open(__file__+'.ended','a').write(json.dumps(sys.argv[1:])+'\\n')\n";
    const QString fixture = m_dir->filePath(name);
    return FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)) ? fixture : QString();
}

FleetState SessionsWindowFixture::fleet() const
{
    SessionInfo codex; codex.name = "codex/hgs/dashboard"; codex.cmd = "codex"; codex.project = "hgs"; codex.tag = "dashboard";
    codex.tracked = true; codex.resumable = true; codex.activity = "busy"; codex.phase = "tool";
    codex.currentTool = "Bash"; codex.toolDetail = "ctest --test-dir tray/build"; codex.model = "gpt-6.1-sol"; codex.effort = "high";
    codex.prompt = "Build a clear view of every agent across our machines.";
    auto kimi = codex; kimi.name = "kimi/docs/research"; kimi.cmd = "kimi"; kimi.project = "docs"; kimi.tag = "research";
    kimi.currentTool.clear(); kimi.activity = "idle"; kimi.phase = "idle"; kimi.prompt = "Compare the session lifecycle across our CLI agents.";
    auto saved = codex; saved.name = "codex/website/navigation"; saved.project = "website"; saved.tag = "navigation";
    saved.state = "paused"; saved.currentTool.clear(); saved.prompt = "Simplify the mobile navigation.";
    SessionInfo old; old.name = "sh/scratch"; old.cmd = "sh"; old.project = "scratch";
    BoxState arch; arch.host = "arch"; arch.ok = true; arch.sessions = {codex, kimi, saved, old};
    // Its folders are POSIX on every GUI; tests of local Windows paths set their own platform.
    arch.metrics = QJsonObject{{"platform", "linux"}};
    auto approval = codex; approval.name = "claude/infra/review"; approval.cmd = "claude"; approval.project = "infra";
    approval.tag = "review"; approval.phase = "approval"; approval.currentTool = "Bash";
    approval.toolDetail = "terraform plan"; approval.prompt = "Review infrastructure changes before the next deployment.";
    BoxState mac; mac.host = "mac"; mac.ok = true; mac.sessions = {approval};
    FleetState result; result.setLocal(arch, QDateTime::currentMSecsSinceEpoch()); result.setPeer(mac, QDateTime::currentMSecsSinceEpoch());
    return result;
}

QString SessionsWindowFixture::tasksFixture(const QString &name, const QJsonArray &tasks) const
{
    QFile original(script()); if (!original.open(QIODevice::ReadOnly)) return {};
    const QByteArray answer = "elif c=='task':\n open(__file__+'.tasks','a').write(json.dumps(a)+'\\n')\n if a[1:2]==['send'] and __import__('pathlib').Path(__file__+'.task-receipt').exists(): print(open(__file__+'.task-receipt').read())\n else:\n  tasks=json.loads("
        + FakeCli::pythonString(QString::fromUtf8(QJsonDocument(tasks).toJson(QJsonDocument::Compact))) + ")\n  if a[1:2]==['get']: print(json.dumps({'task':next(t for t in tasks if t['uuid']==a[2])}))\n  else: print(json.dumps({'project':{'id':a[a.index('--project')+1],'name':'Work'},'tasks':tasks,'damaged':[]}))\n";
    const QString fixture = m_dir->filePath(name);
    return FakeCli::install(fixture, original.readAll().replace("if False:pass\n", "if False:pass\n" + answer)) ? fixture : QString();
}
