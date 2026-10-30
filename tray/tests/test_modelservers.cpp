#include "Wait.h"
#include <QtTest>
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QDialog>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QTemporaryDir>
#include <QDateTime>
#include <QStandardItemModel>
#include "FakeCli.h"
#include "HgsClient.h"
#include "ModelServerSettings.h"
#include "ModelServers.h"
#include "ModelServersPage.h"
#include "WorkspaceSkin.h"

// WhiteWh fork, U60 (#656): the Model servers tab and Settings > Model servers
// against a fake hgs that answers `hgs model-server ...` as docs/reference.md
// says and keeps every call (its argv and stdin) for the test to read. Names
// and addresses are synthetic (reserved example domains and documentation IPs).
class TestModelServers : public QObject {
    Q_OBJECT
private slots:
    void initTestCase();
    void init();
    void tabListsServersModelsAndStatesInTheBackground();
    void downAndDownAllAskFirstAndNameTheSharedServer();
    void apiAddressCopiesAndNewSessionStartsTheChosenAgent();
    void settingsAddEditTestAndRemoveWithoutShowingTheKey();
    void emptyReadOnlyAndUnreachableStates();
private:
    QString script() const { return m_dir.filePath("hgs"); }
    QList<QJsonObject> calls(const QString &command) const;
    void setServers(const QJsonArray &servers);
    void setStatus(const QJsonObject &status);
    QTemporaryDir m_dir;
};

namespace {
const char *program = R"PY(#!/usr/bin/env python3
import json,os,sys
a=sys.argv[1:]
data=sys.stdin.read() if a[:1]==['model-server'] and a[1:2] in (['save'],['test']) else ''
open(__file__+'.calls','a').write(json.dumps({'args':a,'input':data})+'\n')
def read(name,default):
    p=__file__+name
    return json.load(open(p)) if os.path.exists(p) else default
if a[:1]==['model-server']:
    c=a[1] if len(a)>1 else 'list'
    if c=='list':print(json.dumps({'servers':read('.servers',[])}))
    elif c=='status':
        s=read('.status',{}).get(a[2],{'status':'error','error':'refused','detail':'unknown'})
        print(json.dumps(s));sys.exit(1 if s.get('status')=='error' else 0)
    elif c=='up':print(json.dumps({'status':'started','server':a[2],'model':a[3],'request_id':'11111111-1111-4111-8111-111111111111','run':'hgs run','timeout_minutes':30}))
    elif c=='up-result':print(json.dumps(read('.upresult',{'request_id':a[2],'status':'running','exit':None,'timed_out':False,'tail':''})))
    elif c=='sessions':print(json.dumps(read('.sessions',{'server':a[2],'sessions':[]})))
    elif c=='down':print(json.dumps({'status':'ran','server':a[2],'model':a[3],'exit':0} if '--yes' in a else {'status':'confirm'}))
    elif c=='logs':print(json.dumps({'status':'ran','server':a[2],'model':a[3] if len(a)>3 and not a[3].startswith('-') else '','lines':['line one','line two'],'truncated':False}))
    elif c=='save':
        req=json.loads(data);old={s['name']:s for s in read('.servers',[])};rows=[]
        for s in req['servers']:
            was=old.get(s.get('was',''),{})
            key=('key' in s and s['key'] is not None) or ('key' not in s and was.get('key',False))
            rows.append({'name':s['name'],'base_url':s['base_url'],'kind':s['kind'],'ssh':s.get('ssh',''),'key':bool(key),'up_minutes':30,
                         'command':{k:bool(s.get(k)) for k in ('status','up','down','logs')},'argv':{k:s.get(k,[]) for k in ('status','up','down','logs')}})
        if any(r['base_url'].startswith('ftp') for r in rows):
            print(json.dumps({'status':'error','error':'refused','detail':'Server bad: The API URL must be http:// or https://'}));sys.exit(1)
        json.dump(rows,open(__file__+'.servers','w'));print(json.dumps({'status':'saved','servers':rows}))
    elif c=='test':print(json.dumps(read('.test',{'server':'x','source':'api','models':[{'name':'small','state':''}],'memory':None,'raw':[]})))
else:print('{}')
)PY";

QJsonObject server(const QString &name, const QString &url, const QString &kind, const QString &ssh, bool control, bool key = false)
{
    const QJsonArray none;
    const auto argv = [&](const QStringList &list) { return control ? QJsonArray::fromStringList(list) : none; };
    return {{"name", name}, {"base_url", url}, {"kind", kind}, {"ssh", ssh}, {"key", key}, {"up_minutes", 30},
            {"command", QJsonObject{{"status", control}, {"up", control}, {"down", control}, {"logs", control}}},
            {"argv", QJsonObject{{"status", argv({"model-ctl", "status", "--json"})}, {"up", argv({"model-ctl", "up", "{model}"})},
                                 {"down", argv({"model-ctl", "down", "{model}"})}, {"logs", argv({"model-ctl", "logs", "{model}"})}}}};
}

QJsonArray fourServers()
{
    return {server("lab-gpu", "https://gpu-a.lan.example:8000/v1", "openai", "gpu-a", true, true),
            server("workstation", "http://127.0.0.1:8080", "openai", "", true),
            server("shared-gateway", "https://llm.lan.example/", "anthropic", "", false, true),
            server("spare-box", "http://192.0.2.15:8000/v1", "openai", "lab-spare", true)};
}

QJsonObject fourStatuses()
{
    const auto model = [](const QString &name, const QString &state) { return QJsonObject{{"name", name}, {"state", state}}; };
    const auto since = [&](const QString &name, const QString &at) {
        auto row = model(name, "running");
        row["since"] = double(QDateTime(QDate::currentDate(), QTime::fromString(at, QStringLiteral("HH:mm"))).toSecsSinceEpoch());
        return row;
    };
    return {
        {"lab-gpu", QJsonObject{{"server", "lab-gpu"}, {"source", "command"},
                                {"models", QJsonArray{since("coder-a", "09:12"), since("chat-general", "09:40"), model("embed-text", "loading"), model("review-long", "stopped")}},
                                {"memory", QJsonObject{{"free", 38.2}, {"total", 48.0}}}, {"raw", QJsonArray()}}},
        {"workstation", QJsonObject{{"server", "workstation"}, {"source", "command"},
                                    {"models", QJsonArray{model("chat-local", "running"), model("coder-b", "stopped")}},
                                    {"memory", QJsonObject{{"free", 3.1}, {"total", 24.0}}}, {"raw", QJsonArray()}}},
        {"shared-gateway", QJsonObject{{"server", "shared-gateway"}, {"source", "api"},
                                       {"models", QJsonArray{model("assistant-a", ""), model("assistant-b", "")}}, {"memory", QJsonValue()}, {"raw", QJsonArray()}}},
        {"spare-box", QJsonObject{{"status", "error"}, {"error", "unreachable"},
                                  {"detail", "ssh lab-spare (BatchMode): Connection timed out"}}}};
}

QFrame *card(QWidget *root, const QString &name)
{
    for (auto *frame : root->findChildren<QFrame *>("modelServerCard")) if (frame->property("server") == name) return frame;
    return nullptr;
}

QFrame *modelRow(QWidget *root, const QString &server, const QString &model)
{
    auto *box = card(root, server);
    if (!box) return nullptr;
    for (auto *frame : box->findChildren<QFrame *>("modelServerModel")) if (frame->property("model") == model) return frame;
    return nullptr;
}

QStringList texts(QWidget *root)
{
    QStringList out;
    for (auto *label : root->findChildren<QLabel *>()) out << label->text();
    for (auto *edit : root->findChildren<QLineEdit *>()) out << edit->text();
    return out;
}

// Answers the next modal dialog with `answer` once it shows.
void answerDialog(const QString &name, const std::function<void(QDialog *)> &answer, int tries = 300)
{
    QTimer::singleShot(10, qApp, [name, answer, tries] {
        auto *dialog = qobject_cast<QDialog *>(QApplication::activeModalWidget());
        if (!dialog || dialog->objectName() != name) { if (tries > 0) answerDialog(name, answer, tries - 1); return; }
        answer(dialog);
    });
}

void shot(QWidget *widget, const QString &name)
{
    const auto dir = qEnvironmentVariable("ZERUS_TEAM_SHOTS");
    if (dir.isEmpty()) return;
    TestWait::settle();
    widget->grab().save(QDir(dir).filePath(name + QStringLiteral(".png")));
}

void lightPalette(bool light)
{
    QPalette palette;
    if (!light) {
        palette.setColor(QPalette::Window, QColor("#14181d")); palette.setColor(QPalette::WindowText, QColor("#e6e8eb"));
        palette.setColor(QPalette::Base, QColor("#11151a")); palette.setColor(QPalette::Text, QColor("#e6e8eb"));
        palette.setColor(QPalette::Button, QColor("#1b1f26")); palette.setColor(QPalette::ButtonText, QColor("#e6e8eb"));
    } else {
        palette.setColor(QPalette::Window, QColor("#f5f7f9")); palette.setColor(QPalette::WindowText, QColor("#1a2733"));
        palette.setColor(QPalette::Base, QColor("#ffffff")); palette.setColor(QPalette::Text, QColor("#1a2733"));
        palette.setColor(QPalette::Button, QColor("#ffffff")); palette.setColor(QPalette::ButtonText, QColor("#1a2733"));
    }
    QApplication::setPalette(palette);
}
}

void TestModelServers::initTestCase()
{
    QVERIFY(m_dir.isValid());
    QVERIFY(FakeCli::install(script(), program));
    lightPalette(false);
}

void TestModelServers::init()
{
    for (const char *suffix : {".calls", ".servers", ".status", ".test", ".upresult", ".sessions"}) QFile::remove(script() + QLatin1String(suffix));
}

QList<QJsonObject> TestModelServers::calls(const QString &command) const
{
    QList<QJsonObject> found;
    QFile file(script() + ".calls");
    if (!file.open(QIODevice::ReadOnly)) return found;
    for (const auto &line : file.readAll().split('\n')) {
        const auto call = QJsonDocument::fromJson(line).object();
        const auto args = call["args"].toArray();
        if (command.isEmpty() || (args.size() > 1 && args[0] == "model-server" && args[1] == command)) found << call;
    }
    return found;
}

void TestModelServers::setServers(const QJsonArray &servers)
{
    QVERIFY(FakeCli::replace(script() + ".servers", QJsonDocument(servers).toJson(QJsonDocument::Compact)));
}

void TestModelServers::setStatus(const QJsonObject &status)
{
    QVERIFY(FakeCli::replace(script() + ".status", QJsonDocument(status).toJson(QJsonDocument::Compact)));
}

// The tab lists every server with its host, API address, free memory and
// models; the page reads them in the background while it shows, never two
// reads of one server and at most two at once, and stops when hidden.
void TestModelServers::tabListsServersModelsAndStatesInTheBackground()
{
    setServers(fourServers()); setStatus(fourStatuses());
    HgsClient client(FakeCli::program(script()));
    ModelServers::Controller servers(&client);
    servers.setInterval(400);
    servers.refreshList();
    QTRY_VERIFY(servers.listed());
    QCOMPARE(servers.servers().size(), 4);
    QVERIFY(servers.available());
    QVERIFY(!servers.server("lab-gpu")->readOnly()); QVERIFY(servers.server("shared-gateway")->readOnly());
    QCOMPARE(servers.server("lab-gpu")->status, (QStringList{"model-ctl", "status", "--json"}));
    ModelServersPage page(&servers);
    page.machines = [] { return QStringList{"gpu-a", "build-01"}; };
    page.resize(1440, 1000); page.show();
    QTRY_VERIFY(servers.status("lab-gpu").known && servers.status("spare-box").known && servers.status("shared-gateway").known && servers.status("workstation").known);
    // One read of each server when the page opened; a refresh while one is read adds none.
    for (const auto &name : {"lab-gpu", "workstation", "shared-gateway", "spare-box"}) {
        int count = 0;
        for (const auto &call : calls("status")) count += call["args"].toArray()[2] == name;
        QVERIFY2(count >= 1, name);
    }
    QTRY_VERIFY(card(&page, "lab-gpu") && modelRow(&page, "lab-gpu", "review-long"));
    auto *lab = card(&page, "lab-gpu");
    QCOMPARE(lab->findChild<QLabel *>("modelServerHostText")->text(), QStringLiteral("gpu-a in Machines"));
    QCOMPARE(lab->findChild<QLabel *>("modelServerUrl")->text(), QStringLiteral("https://gpu-a.lan.example:8000/v1"));
    QCOMPARE(lab->findChild<QLabel *>("modelServerKind")->text(), QStringLiteral("OPENAI"));
    QCOMPARE(lab->findChild<QLabel *>("modelServerMemoryText")->text(), QStringLiteral("38.2 of 48.0 GB free"));
    QCOMPARE(lab->findChild<QWidget *>("modelServerState")->property("state").toString(), QStringLiteral("Online"));
    QCOMPARE(card(&page, "workstation")->findChild<QLabel *>("modelServerHostText")->text(), QStringLiteral("This computer"));
    // #703 (#658 N2): an entry saved as the root shows the URL its agents get;
    // an Anthropic root has no /v1.
    QCOMPARE(card(&page, "workstation")->findChild<QLabel *>("modelServerUrl")->text(), QStringLiteral("http://127.0.0.1:8080/v1"));
    QCOMPARE(card(&page, "shared-gateway")->findChild<QLabel *>("modelServerUrl")->text(), QStringLiteral("https://llm.lan.example"));
    // #703 (#659 QA): a model's since-time when its status carries one.
    QCOMPARE(modelRow(&page, "lab-gpu", "coder-a")->findChild<QLabel *>("modelServerModelSince")->text(), QStringLiteral("since 09:12"));
    QCOMPARE(modelRow(&page, "lab-gpu", "chat-general")->findChild<QLabel *>("modelServerModelSince")->text(), QStringLiteral("since 09:40"));
    QVERIFY(!modelRow(&page, "lab-gpu", "embed-text")->findChild<QLabel *>("modelServerModelSince"));
    QVERIFY(card(&page, "workstation")->findChild<QWidget *>("modelServerMemory")->property("low").toBool());
    QCOMPARE(card(&page, "spare-box")->findChild<QLabel *>("modelServerHostText")->text(), QStringLiteral("ssh alias lab-spare"));
    // The models with their states and actions.
    const auto state = [&](const QString &model) { return modelRow(&page, "lab-gpu", model)->findChild<QLabel *>("modelServerModelState")->text(); };
    QCOMPARE(state("coder-a"), QStringLiteral("Running")); QCOMPARE(state("embed-text"), QStringLiteral("Loading")); QCOMPARE(state("review-long"), QStringLiteral("Stopped"));
    QVERIFY(modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerDown"));
    QVERIFY(modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerNewSession"));
    QVERIFY(!modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerUp"));
    QVERIFY(modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp"));
    QVERIFY(!modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerDown"));
    QVERIFY(!modelRow(&page, "lab-gpu", "embed-text")->findChild<QPushButton *>("modelServerNewSession"));
    QVERIFY(lab->findChild<QPushButton *>("modelServerDownAll"));
    QCOMPARE(page.findChild<QLabel *>("modelServersSummary")->text(), QStringLiteral("4 servers, 8 models, 3 running"));
    for (const auto &text : texts(&page)) QVERIFY2(!text.contains(QStringLiteral(" · ")), qPrintable(text));
    // Up runs once, detached; #703 (#658 N5): the page then reads the run's
    // own outcome until it ends and shows it, not only that it started.
    servers.setUpPoll(60);
    modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->click();
    QTRY_COMPARE(calls("up").size(), 1);
    QCOMPARE(calls("up").first()["args"].toArray(), (QJsonArray{"model-server", "up", "lab-gpu", "review-long", "--detach"}));
    auto *notice = page.findChild<QLabel *>("modelServersNotice");
    QTRY_VERIFY2(notice->text().contains("Up review-long on lab-gpu is running once"), qPrintable(notice->text()));
    QTRY_VERIFY(calls("up-result").size() >= 2);
    QCOMPARE(calls("up-result").first()["args"].toArray(), (QJsonArray{"model-server", "up-result", "11111111-1111-4111-8111-111111111111", "--json"}));
    QVERIFY(servers.upRunning("lab-gpu", "review-long"));
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "review-long") && !modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->isEnabled());
    QCOMPARE(modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->text(), QStringLiteral("Up running"));
    QVERIFY(!notice->property("error").toBool());
    // The run ends with exit 3: the page says it failed and why.
    QVERIFY(FakeCli::replace(script() + ".upresult", R"({"request_id":"11111111-1111-4111-8111-111111111111","server":"lab-gpu","model":"review-long","status":"ran","exit":3,"timed_out":false,"tail":"loading review-long\nCUDA out of memory\n","timeout_secs":1800})"));
    QTRY_VERIFY2(notice->text() == QStringLiteral("Up review-long on lab-gpu failed: exit 3: CUDA out of memory"), qPrintable(notice->text()));
    QVERIFY(notice->property("error").toBool());
    QVERIFY(!servers.upRunning("lab-gpu", "review-long"));
    shot(&page, "u60-up-failed-dark");
    lightPalette(true); page.setDark(false); shot(&page, "u60-up-failed-light"); lightPalette(false); page.setDark(true);
    const int polls = int(calls("up-result").size());
    // A run that ends well says so; one past its time limit says that.
    QVERIFY(FakeCli::replace(script() + ".upresult", R"({"status":"ran","exit":0,"timed_out":false,"tail":"ready\n","timeout_secs":1800})"));
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "review-long") && modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->isEnabled());
    modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->click();
    QTRY_VERIFY2(notice->text() == QStringLiteral("Up review-long on lab-gpu finished: exit 0. Nothing starts it again on its own."), qPrintable(notice->text()));
    QVERIFY(!notice->property("error").toBool());
    QVERIFY(FakeCli::replace(script() + ".upresult", R"({"status":"ran","exit":null,"timed_out":true,"tail":"","timeout_secs":1800})"));
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "review-long") && modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->isEnabled());
    modelRow(&page, "lab-gpu", "review-long")->findChild<QPushButton *>("modelServerUp")->click();
    QTRY_VERIFY2(notice->text() == QStringLiteral("Up review-long on lab-gpu failed: stopped at its 30 min limit"), qPrintable(notice->text()));
    QVERIFY(calls("up-result").size() > polls);
    QCOMPARE(calls("up").size(), 3);
    notice->clear(); notice->hide();
    shot(&page, "u60-servers-dark");
    lightPalette(true); page.setDark(false); shot(&page, "u60-servers-light");
    lightPalette(false); page.setDark(true);
    // The background reads follow the interval while the page shows, at most
    // MaxReads at once; hidden, they stop.
    const int before = int(calls("status").size());
    QTRY_VERIFY(calls("status").size() >= before + 4);
    page.hide();
    QTest::qWait(700);
    const int hidden = int(calls("status").size());
    QTest::qWait(900);
    QCOMPARE(calls("status").size(), hidden);
    QVERIFY(!servers.active());
}

// Every Down and Down all asks first and names the shared server; Cancel runs
// nothing, the confirmation runs down once per model with --yes.
void TestModelServers::downAndDownAllAskFirstAndNameTheSharedServer()
{
    setServers(fourServers()); setStatus(fourStatuses());
    HgsClient client(FakeCli::program(script()));
    ModelServers::Controller servers(&client);
    servers.refreshList();
    ModelServersPage page(&servers);
    page.resize(1440, 1000); page.show();
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "coder-a"));
    // Cancel: nothing runs.
    QString shared;
    answerDialog("modelServerDownDialog", [&](QDialog *dialog) {
        shared = dialog->findChild<QLabel *>("modelServerDownShared")->text();
        dialog->findChild<QPushButton *>("modelServerDownCancel")->click();
    });
    modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerDown")->click();
    QTRY_VERIFY(!shared.isEmpty());
    QVERIFY2(shared.contains("lab-gpu may be shared") && shared.contains("Other people's sessions"), qPrintable(shared));
    QTest::qWait(100);
    QCOMPARE(calls("down").size(), 0);
    // Confirmed: once, with --yes.
    answerDialog("modelServerDownDialog", [](QDialog *dialog) { dialog->findChild<QPushButton *>("modelServerDownConfirm")->click(); });
    modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerDown")->click();
    QTRY_COMPARE(calls("down").size(), 1);
    QCOMPARE(calls("down").first()["args"].toArray(), (QJsonArray{"model-server", "down", "lab-gpu", "coder-a", "--yes"}));
    // Down all lists the running and loading models, which of your sessions
    // use each (#703, #659 QA), and downs each once.
    QVERIFY(FakeCli::replace(script() + ".sessions", R"({"server":"lab-gpu","sessions":[{"name":"codex/orbit-support/coder-a","model":"coder-a"},{"name":"codex/orbit/review","model":"coder-a"}]})"));
    QStringList listed; QString confirmText; QHash<QString, QString> uses;
    answerDialog("modelServerDownDialog", [&](QDialog *dialog) {
        for (auto *label : dialog->findChildren<QLabel *>("modelServerDownModel")) listed << label->text();
        confirmText = dialog->findChild<QPushButton *>("modelServerDownConfirm")->text();
        QTRY_VERIFY(!dialog->findChildren<QLabel *>("modelServerDownUse").isEmpty()
                    && !dialog->findChildren<QLabel *>("modelServerDownUse").first()->text().startsWith("reading"));
        for (auto *label : dialog->findChildren<QLabel *>("modelServerDownUse")) uses.insert(label->property("model").toString(), label->text());
        shot(dialog, "u60-down-all-dark");
        dialog->findChild<QPushButton *>("modelServerDownConfirm")->click();
    });
    QTRY_VERIFY(card(&page, "lab-gpu") && card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerDownAll"));
    card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerDownAll")->click();
    QTRY_COMPARE(calls("down").size(), 4);
    QCOMPARE(listed, (QStringList{"coder-a", "chat-general", "embed-text"}));
    QCOMPARE(confirmText, QStringLiteral("Down 3 models"));
    QCOMPARE(uses.value("coder-a"), QStringLiteral("2 of your sessions use it: codex/orbit-support/coder-a, codex/orbit/review"));
    QCOMPARE(uses.value("chat-general"), QStringLiteral("none of your sessions"));
    QCOMPARE(uses.value("embed-text"), QStringLiteral("stops loading"));
    QVERIFY(!calls("sessions").isEmpty());
    QCOMPARE(calls("sessions").last()["args"].toArray(), (QJsonArray{"model-server", "sessions", "lab-gpu", "--json"}));
    QStringList downed;
    for (const auto &call : calls("down").mid(1)) { QVERIFY(call["args"].toArray().contains("--yes")); downed << call["args"].toArray()[3].toString(); }
    QCOMPARE(downed, (QStringList{"coder-a", "chat-general", "embed-text"}));
    QTRY_VERIFY(page.findChild<QLabel *>("modelServersNotice")->text().contains("Down ran once for 3 models on lab-gpu"));
    // The light theme's Down all, opened in that theme and cancelled.
    lightPalette(true); page.setDark(false);
    bool lightShown = false;
    answerDialog("modelServerDownDialog", [&](QDialog *dialog) {
        QTRY_VERIFY(!dialog->findChildren<QLabel *>("modelServerDownUse").first()->text().startsWith("reading"));
        shot(dialog, "u60-down-all-light"); lightShown = true;
        dialog->findChild<QPushButton *>("modelServerDownCancel")->click();
    });
    QTRY_VERIFY(card(&page, "lab-gpu") && card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerDownAll"));
    card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerDownAll")->click();
    QTRY_VERIFY(lightShown);
    QTest::qWait(100);
    QCOMPARE(calls("down").size(), 4);
    lightPalette(false); page.setDark(true);
    // A read-only server has no Up, Down or Logs.
    auto *gateway = card(&page, "shared-gateway");
    for (const char *name : {"modelServerUp", "modelServerDown", "modelServerDownAll", "modelServerLogs", "modelServerModelLogs"})
        QVERIFY2(!gateway->findChild<QPushButton *>(name), name);
    QCOMPARE(modelRow(&page, "shared-gateway", "assistant-a")->findChild<QLabel *>("modelServerModelState")->text(), QStringLiteral("Listed"));
    QVERIFY(modelRow(&page, "shared-gateway", "assistant-a")->findChild<QPushButton *>("modelServerNewSession"));
}

// Copy puts the API address on the clipboard in one click; New session offers
// the agents the server's API takes and starts the chosen one on its model.
void TestModelServers::apiAddressCopiesAndNewSessionStartsTheChosenAgent()
{
    setServers(fourServers()); setStatus(fourStatuses());
    HgsClient client(FakeCli::program(script()));
    ModelServers::Controller servers(&client);
    servers.refreshList();
    ModelServersPage page(&servers);
    QStringList started;
    page.folders = [] { return QStringList{"/work/orbit-support", "/work/other"}; };
    page.startSession = [&](const QString &agent, const QString &folder, const QString &tag, const QString &server, const QString &model) {
        started = {agent, folder, tag, server, model};
    };
    page.resize(1440, 1000); page.show();
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "coder-a"));
    QGuiApplication::clipboard()->clear();
    card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerCopy")->click();
    QCOMPARE(QGuiApplication::clipboard()->text(), QStringLiteral("https://gpu-a.lan.example:8000/v1"));
    QTRY_COMPARE(card(&page, "lab-gpu")->findChild<QPushButton *>("modelServerCopy")->text(), QStringLiteral("Copied"));
    // OpenAI: Codex; Claude Code says why not; OpenCode is not offered (#702 N1).
    QString claudeWhy; bool claudeEnabled = true; QString summary; QStringList agents, items; QString name; bool stoppedEnabled = true;
    answerDialog("modelServerSessionDialog", [&](QDialog *dialog) {
        for (auto *radio : dialog->findChildren<QRadioButton *>("modelServerAgentChoice")) {
            agents << radio->property("agent").toString();
            if (radio->property("agent") == "claude") claudeEnabled = radio->isEnabled();
        }
        for (auto *frame : dialog->findChildren<QFrame *>("modelServerAgent"))
            if (frame->property("agent") == "claude") claudeWhy = frame->findChild<QLabel *>("modelServerAgentHow")->text();
        // #703 (#659 QA): every model with its state; a stopped one cannot be chosen.
        auto *models = dialog->findChild<QComboBox *>("modelServerSessionModel");
        for (int i = 0; i < models->count(); ++i) items << models->itemText(i);
        const int stopped = models->findData("review-long");
        stoppedEnabled = qobject_cast<QStandardItemModel *>(models->model())->item(stopped)->isEnabled();
        QCOMPARE(models->currentData().toString(), QStringLiteral("coder-a"));
        // The name as the mock-up shows it: agent/project/tag.
        name = dialog->findChild<QLabel *>("modelServerSessionPrefix")->text() + dialog->findChild<QLineEdit *>("modelServerSessionTag")->text();
        summary = dialog->findChild<QLabel *>("modelServerSessionSummary")->text();
        shot(dialog, "u60-session-dark");
        dialog->findChild<QPushButton *>("modelServerStart")->click();
    });
    modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerNewSession")->click();
    QTRY_VERIFY(!started.isEmpty());
    QVERIFY(!claudeEnabled);
    QCOMPARE(agents, (QStringList{"codex", "claude"}));
    QCOMPARE(items, (QStringList{"coder-a   Running", "chat-general   Running", "embed-text   Loading", "review-long   Stopped"}));
    QVERIFY(!stoppedEnabled);
    QCOMPARE(name, QStringLiteral("codex/orbit-support/coder-a"));
    // The light theme's dialog, opened in that theme and cancelled.
    lightPalette(true); page.setDark(false);
    bool lightShown = false;
    answerDialog("modelServerSessionDialog", [&](QDialog *dialog) {
        shot(dialog, "u60-session-light"); lightShown = true;
        dialog->findChild<QPushButton *>("modelServerSessionCancel")->click();
    });
    QTRY_VERIFY(modelRow(&page, "lab-gpu", "coder-a"));
    modelRow(&page, "lab-gpu", "coder-a")->findChild<QPushButton *>("modelServerNewSession")->click();
    QTRY_VERIFY(lightShown);
    lightPalette(false); page.setDark(true);
    QVERIFY2(claudeWhy.contains("needs an Anthropic-compatible API; lab-gpu serves OpenAI"), qPrintable(claudeWhy));
    QVERIFY2(summary.contains("provider  lab-gpu  https://gpu-a.lan.example:8000/v1") && summary.contains("key       from the private store"), qPrintable(summary));
    QCOMPARE(started, (QStringList{"codex", "/work/orbit-support", "coder-a", "lab-gpu", "coder-a"}));
    // An Anthropic server takes Claude Code, not Codex.
    started.clear();
    answerDialog("modelServerSessionDialog", [&](QDialog *dialog) {
        for (auto *radio : dialog->findChildren<QRadioButton *>("modelServerAgentChoice")) {
            if (radio->property("agent") == "codex") QVERIFY(!radio->isEnabled());
            if (radio->property("agent") == "claude") radio->setChecked(true);
        }
        dialog->findChild<QPushButton *>("modelServerStart")->click();
    });
    modelRow(&page, "shared-gateway", "assistant-b")->findChild<QPushButton *>("modelServerNewSession")->click();
    QTRY_VERIFY(!started.isEmpty());
    QCOMPARE(started, (QStringList{"claude", "/work/orbit-support", "assistant-b", "shared-gateway", "assistant-b"}));
    // The launch: this session's server and model, nothing else.
    QSignalSpy launched(&client, &HgsClient::nativeSessionLaunched);
    client.launchModelServerSession("codex", "/work/orbit-support", "coder-a", "lab-gpu", "coder-a", "launch-1");
    QTRY_COMPARE(launched.size(), 1);
    QJsonArray args;
    for (const auto &call : calls({})) if (call["args"].toArray().first() == "codex") args = call["args"].toArray();
    QCOMPARE(args, (QJsonArray{"codex", "/work/orbit-support", "--new", "-n", "coder-a", "--launch-id", "launch-1", "-d", "--model-server", "lab-gpu", "--model", "coder-a"}));
}

// Settings adds, edits, tests and removes a server through save/test --json;
// the key is sent once and never shown again; Up and Down have no Test here.
void TestModelServers::settingsAddEditTestAndRemoveWithoutShowingTheKey()
{
    setServers(QJsonArray{server("lab-gpu", "https://gpu-a.lan.example:8000/v1", "openai", "gpu-a", true, true)});
    setStatus(fourStatuses());
    HgsClient client(FakeCli::program(script()));
    ModelServers::Controller servers(&client);
    servers.refreshList();
    QTRY_VERIFY(servers.available());
    ModelServerSettings settings(&servers);
    settings.machines = [] { return QStringList{"gpu-a", "build-01"}; };
    settings.resize(1300, 1000); settings.show();
    settings.showServer("lab-gpu");
    auto *name = settings.findChild<QLineEdit *>("modelServerName");
    QCOMPARE(name->text(), QStringLiteral("lab-gpu"));
    QCOMPARE(settings.findChild<QLineEdit *>("modelServerStatusCommand")->text(), QStringLiteral("model-ctl status --json"));
    QVERIFY(settings.findChild<QPushButton *>("modelServerSshMachine")->isChecked());
    QCOMPARE(settings.findChild<QComboBox *>("modelServerMachine")->currentText(), QStringLiteral("gpu-a"));
    // A stored key: only that one is stored; Replace and Remove, no field.
    QVERIFY(settings.findChild<QLabel *>("modelServerKeyStored")->isVisible());
    QVERIFY(!settings.findChild<QLineEdit *>("modelServerKey")->isVisible());
    // Up and Down have no Test: they belong to the tab.
    QVERIFY(settings.findChild<QPushButton *>("modelServerTestStatus"));
    QVERIFY(settings.findChild<QPushButton *>("modelServerTestLogs"));
    QCOMPARE(settings.findChildren<QPushButton *>("modelServerTestStatus").size() + settings.findChildren<QPushButton *>("modelServerTestLogs").size(), 2);
    // #703 (#658 N2): the field asks for the server root and explains what is
    // derived from it; an existing entry with /v1 still reads the same server.
    auto *url = settings.findChild<QLineEdit *>("modelServerUrl");
    QCOMPARE(url->placeholderText(), QStringLiteral("http://host.example:8000"));
    QLabel *hint = nullptr;
    for (auto *label : settings.findChildren<QLabel *>()) if (label->property("urlHint").toBool()) hint = label;
    QVERIFY(hint);
    QCOMPARE(hint->text(), QStringLiteral("Model list: https://gpu-a.lan.example:8000/v1/models\nCodex: https://gpu-a.lan.example:8000/v1\n"
                                          "Claude Code: https://gpu-a.lan.example:8000\n"
                                          "Saved as its root https://gpu-a.lan.example:8000: the /v1 you typed is added where it is needed."));
    QVERIFY(texts(&settings).contains("Server root"));
    shot(&settings, "u60-settings-dark");
    lightPalette(true); settings.setDark(false); shot(&settings, "u60-settings-light"); lightPalette(false); settings.setDark(true);
    // Test API: the form's server without its commands, the stored key by `was`.
    settings.findChild<QPushButton *>("modelServerTestApi")->click();
    QTRY_COMPARE(calls("test").size(), 1);
    auto input = QJsonDocument::fromJson(calls("test").first()["input"].toString().toUtf8()).object();
    QCOMPARE(input["was"].toString(), QStringLiteral("lab-gpu")); QVERIFY(!input.contains("key")); QVERIFY(!input.contains("status"));
    QTRY_COMPARE(settings.findChild<QLabel *>("modelServerApiResult")->text(), QStringLiteral("GET https://gpu-a.lan.example:8000/v1/models answered: 1 model"));
    QCOMPARE(input["base_url"].toString(), QStringLiteral("https://gpu-a.lan.example:8000/v1"));   // hgs takes either spelling
    // A root spelling reads the same URL; a refusal names the URL it read.
    url->setText(QStringLiteral("https://gpu-a.lan.example:8000/"));
    QCOMPARE(hint->text(), QStringLiteral("Model list: https://gpu-a.lan.example:8000/v1/models\nCodex: https://gpu-a.lan.example:8000/v1\nClaude Code: https://gpu-a.lan.example:8000"));
    QVERIFY(FakeCli::replace(script() + ".test", R"({"status":"error","error":"refused","detail":"The server answered HTTP 404"})"));
    settings.findChild<QPushButton *>("modelServerTestApi")->click();
    QTRY_COMPARE(settings.findChild<QLabel *>("modelServerApiResult")->text(), QStringLiteral("GET https://gpu-a.lan.example:8000/v1/models: The server answered HTTP 404"));
    QFile::remove(script() + ".test");
    url->clear();
    QVERIFY(hint->text().startsWith("The address the server answers at, without /v1"));
    url->setText(QStringLiteral("https://gpu-a.lan.example:8000/v1"));
    // Test status: raw output is shown as raw text, with the warning.
    QVERIFY(FakeCli::replace(script() + ".test", R"({"server":"lab-gpu","source":"command","models":[],"memory":null,"raw":["PID UPTIME ARGS","4121 02:13 serve chat-local"]})"));
    settings.findChild<QPushButton *>("modelServerTestStatus")->click();
    QTRY_COMPARE(calls("test").size(), 3);
    input = QJsonDocument::fromJson(calls("test").last()["input"].toString().toUtf8()).object();
    QCOMPARE(input["status"].toArray(), (QJsonArray{"model-ctl", "status", "--json"}));
    QTRY_VERIFY(settings.findChild<QLabel *>("modelServerStatusResult")->text().startsWith("Neither the JSON contract"));
    QCOMPARE(settings.findChild<QPlainTextEdit *>("modelServerStatusRaw")->toPlainText(), QStringLiteral("PID UPTIME ARGS\n4121 02:13 serve chat-local"));
    // Edit and save: the key stays stored (no key sent), the server keeps its `was`.
    settings.findChild<QLineEdit *>("modelServerUrl")->setText("https://gpu-b.lan.example:8000/v1");
    settings.findChild<QPushButton *>("modelServerSave")->click();
    QTRY_COMPARE(calls("save").size(), 1);
    input = QJsonDocument::fromJson(calls("save").first()["input"].toString().toUtf8()).object();
    QCOMPARE(input["by"].toString(), QStringLiteral("human"));
    auto saved = input["servers"].toArray().first().toObject();
    QCOMPARE(saved["was"].toString(), QStringLiteral("lab-gpu")); QVERIFY(!saved.contains("key"));
    QCOMPARE(saved["base_url"].toString(), QStringLiteral("https://gpu-b.lan.example:8000/v1"));
    QCOMPARE(saved["down"].toArray(), (QJsonArray{"model-ctl", "down", "{model}"}));
    QTRY_COMPARE(servers.server("lab-gpu")->baseUrl, QStringLiteral("https://gpu-b.lan.example:8000/v1"));
    // Add a server with a key: sent once; after saving it is never shown again.
    settings.findChild<QPushButton *>("modelServerAdd")->click();
    QVERIFY(name->text().isEmpty());
    name->setText("spare-box");
    settings.findChild<QLineEdit *>("modelServerUrl")->setText("http://192.0.2.15:8000/v1");
    settings.findChild<QPushButton *>("modelServerSshAlias")->click();
    settings.findChild<QLineEdit *>("modelServerAlias")->setText("lab-spare");
    auto *key = settings.findChild<QLineEdit *>("modelServerKey");
    QVERIFY(key->isVisible()); QCOMPARE(key->echoMode(), QLineEdit::Password);
    key->setText("sk-example-secret");
    settings.findChild<QPushButton *>("modelServerSave")->click();
    QTRY_COMPARE(calls("save").size(), 2);
    input = QJsonDocument::fromJson(calls("save").last()["input"].toString().toUtf8()).object();
    QCOMPARE(input["servers"].toArray().size(), 2);
    saved = input["servers"].toArray().last().toObject();
    QCOMPARE(saved["key"].toString(), QStringLiteral("sk-example-secret")); QCOMPARE(saved["ssh"].toString(), QStringLiteral("lab-spare"));
    QVERIFY(!saved.contains("was"));
    QVERIFY(!input["servers"].toArray().first().toObject().contains("key"));   // the other keeps its key
    QTRY_VERIFY(servers.server("spare-box") && servers.server("spare-box")->hasKey);
    QTRY_COMPARE(name->text(), QStringLiteral("spare-box"));
    QVERIFY(settings.findChild<QLabel *>("modelServerKeyStored")->isVisible());
    QVERIFY(key->text().isEmpty());
    for (const auto &text : texts(&settings)) QVERIFY2(!text.contains("sk-example-secret"), qPrintable(text));
    // The key never travels in an argument.
    for (const auto &call : calls({})) QVERIFY(!QJsonDocument(call["args"].toArray()).toJson().contains("sk-example-secret"));
    // Remove the key: null.
    settings.findChild<QPushButton *>("modelServerKeyRemove")->click();
    settings.findChild<QPushButton *>("modelServerSave")->click();
    QTRY_COMPARE(calls("save").size(), 3);
    saved = QJsonDocument::fromJson(calls("save").last()["input"].toString().toUtf8()).object()["servers"].toArray().last().toObject();
    QVERIFY(saved.contains("key") && saved["key"].isNull());
    QTRY_VERIFY(servers.server("spare-box") && !servers.server("spare-box")->hasKey);
    // A refused save names the server and field and keeps the form.
    settings.findChild<QLineEdit *>("modelServerUrl")->setText("ftp://bad");
    settings.findChild<QPushButton *>("modelServerSave")->click();
    QTRY_VERIFY(settings.findChild<QLabel *>("modelServerSaveError")->isVisible());
    QVERIFY(settings.findChild<QLabel *>("modelServerSaveError")->text().contains("Server bad: The API URL"));
    QCOMPARE(settings.findChild<QLineEdit *>("modelServerUrl")->text(), QStringLiteral("ftp://bad"));
    // Delete asks first; No keeps it, Yes saves the list without it.
    settings.showServer("spare-box");
    QString asked;
    settings.confirm = [&](const QString &, const QString &text, const QString &) { asked = text; return false; };
    settings.findChild<QPushButton *>("modelServerDelete")->click();
    QVERIFY(asked.contains("Nothing on the server changes"));
    QCOMPARE(calls("save").size(), 4);
    settings.confirm = [](const QString &, const QString &, const QString &) { return true; };
    settings.findChild<QPushButton *>("modelServerDelete")->click();
    QTRY_COMPARE(calls("save").size(), 5);
    QCOMPARE(QJsonDocument::fromJson(calls("save").last()["input"].toString().toUtf8()).object()["servers"].toArray().size(), 1);
    QTRY_VERIFY(!servers.server("spare-box"));
    // Settings runs no up or down.
    QCOMPARE(calls("up").size(), 0); QCOMPARE(calls("down").size(), 0);
    lightPalette(true); settings.setDark(false); settings.showServer("lab-gpu"); shot(&settings, "u60-settings-light");
    lightPalette(false); settings.setDark(true);
}

// No server: no list, the empty state; a read-only server says so; an
// unreachable one says why, with Try again (one read) and Edit….
void TestModelServers::emptyReadOnlyAndUnreachableStates()
{
    HgsClient client(FakeCli::program(script()));
    ModelServers::Controller servers(&client);
    servers.refreshList();
    QTRY_VERIFY(servers.listed());
    QVERIFY(!servers.available());
    ModelServersPage page(&servers);
    QString edited = QStringLiteral("unset");
    page.editServers = [&](const QString &server) { edited = server; };
    page.resize(1200, 760); page.show();
    QTRY_VERIFY(page.findChild<QWidget *>("modelServersEmpty"));
    QVERIFY(texts(&page).contains("No model servers on this computer"));
    // #703 (#659 QA): with no server the page offers only Add server.
    QVERIFY(page.findChild<QPushButton *>("modelServersAdd")->isVisible());
    QVERIFY(!page.findChild<QPushButton *>("modelServersRefreshAll")->isVisible());
    QVERIFY(!page.findChild<QPushButton *>("modelServersEdit")->isVisible());
    page.findChild<QPushButton *>("modelServersAdd")->click();
    QCOMPARE(edited, QString());
    shot(&page, "u60-empty-dark");
    lightPalette(true); page.setDark(false); shot(&page, "u60-empty-light"); lightPalette(false); page.setDark(true);
    QCOMPARE(calls("status").size(), 0);
    // Read only and unreachable.
    setServers(QJsonArray{server("shared-gateway", "https://llm.lan.example/", "anthropic", "", false, true),
                          server("spare-box", "http://192.0.2.15:8000/v1", "openai", "lab-spare", true)});
    setStatus(fourStatuses());
    servers.refreshList();
    QTRY_VERIFY(servers.available());
    servers.refreshAll();
    QTRY_VERIFY(card(&page, "spare-box") && card(&page, "spare-box")->findChild<QLabel *>("modelServerProblemText"));
    auto *spare = card(&page, "spare-box");
    QVERIFY(spare->property("unreachable").toBool());
    QCOMPARE(spare->findChild<QWidget *>("modelServerState")->property("state").toString(), QStringLiteral("Unreachable"));
    const auto problem = spare->findChild<QLabel *>("modelServerProblemText")->text();
    QVERIFY2(problem.contains("Connection timed out") && problem.contains("Zerus tries again only when you ask"), qPrintable(problem));
    QTRY_VERIFY(card(&page, "shared-gateway") && card(&page, "shared-gateway")->findChild<QLabel *>("modelServerFootText"));
    auto *gateway = card(&page, "shared-gateway");
    QCOMPARE(gateway->findChild<QWidget *>("modelServerState")->property("state").toString(), QStringLiteral("Read only"));
    QVERIFY(gateway->findChild<QLabel *>("modelServerFootText")->text().startsWith("No control commands: the models come from the API's model list"));
    QCOMPARE(gateway->findChild<QLabel *>("modelServerMemoryText")->text(), QStringLiteral("memory not reported"));
    shot(&page, "u60-states-dark");
    lightPalette(true); page.setDark(false); shot(&page, "u60-states-light"); lightPalette(false); page.setDark(true);
    // Try again reads that server once; Edit… opens its settings.
    const int before = int(calls("status").size());
    QTest::qWait(200);
    QCOMPARE(int(calls("status").size()), before);   // nothing retries on its own
    card(&page, "spare-box")->findChild<QPushButton *>("modelServerTryAgain")->click();
    QTRY_COMPARE(int(calls("status").size()), before + 1);
    QCOMPARE(calls("status").last()["args"].toArray()[2].toString(), QStringLiteral("spare-box"));
    QTRY_VERIFY(card(&page, "spare-box") && card(&page, "spare-box")->findChild<QPushButton *>("modelServerEdit"));
    card(&page, "spare-box")->findChild<QPushButton *>("modelServerEdit")->click();
    QCOMPARE(edited, QStringLiteral("spare-box"));
    QVERIFY(page.findChild<QPushButton *>("modelServersRefreshAll")->isVisible() && page.findChild<QPushButton *>("modelServersEdit")->isVisible());
    // #703 (#658 N2): a read-only server's failure names the URL it read, from the root.
    auto statuses = fourStatuses();
    statuses["shared-gateway"] = QJsonObject{{"status", "error"}, {"error", "refused"}, {"detail", "The server answered HTTP 404"}};
    setStatus(statuses);
    card(&page, "shared-gateway")->findChild<QPushButton *>("modelServerRefresh")->click();
    QTRY_VERIFY(card(&page, "shared-gateway") && card(&page, "shared-gateway")->findChild<QLabel *>("modelServerProblemText"));
    const auto read = card(&page, "shared-gateway")->findChild<QLabel *>("modelServerProblemText")->text();
    QVERIFY2(read.startsWith("GET https://llm.lan.example/v1/models: The server answered HTTP 404"), qPrintable(read));
}

QTEST_MAIN(TestModelServers)
#include "test_modelservers.moc"
