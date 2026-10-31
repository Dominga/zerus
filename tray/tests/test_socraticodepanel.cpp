#include <QtTest>
#include "Settings.h"
#include <QTemporaryDir>
#include <QApplication>
#include <QFile>
#include <QLabel>
#include <QMessageBox>
#include <QPushButton>
#include <QSettings>
#include "FakeCli.h"
#include "SocratiCodePanel.h"
#include "Extensions.h"

// WhiteWh fork: the session folder's SocratiCode strip offers one next step.
class TestSocratiCodePanel : public QObject {
    Q_OBJECT
    QTemporaryDir m_fixture, m_settings;
    QString program() const { return FakeCli::program(m_fixture.filePath("hgs")); }
    void flag(const QString &name, bool enabled = true)
    {
        QFile f(m_fixture.filePath(name));
        if (enabled) QVERIFY(f.open(QIODevice::WriteOnly)); else f.remove();
    }
    QStringList calls() const
    {
        QFile f(m_fixture.filePath("calls"));
        if (!f.open(QIODevice::ReadOnly)) return {};
        return QString::fromUtf8(f.readAll()).split('\n', Qt::SkipEmptyParts);
    }
    static QString text(QWidget &panel, const char *name) { return panel.findChild<QLabel *>(name)->text(); }
    static QPushButton *button(QWidget &panel, const char *name) { return panel.findChild<QPushButton *>(name); }
    // Answers the next confirmation with Yes.
    static void confirmNext()
    {
        QTimer::singleShot(50, [] {
            if (auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget())) box->button(QMessageBox::Yes)->click();
            else confirmNext();
        });
    }
private slots:
    void initTestCase()
    {
        QCoreApplication::setOrganizationName("hgs-tests");
        QCoreApplication::setApplicationName("socraticode-panel");
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
    }
    void init()
    {
        Settings::flush();QSettings().clear();
        for (const auto *name : {"no-plugin", "native", "connected", "indexed", "running", "calls"}) flag(name, false);
        QVERIFY(FakeCli::install(m_fixture.filePath("hgs"), R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
with open(root/'calls','a') as f:f.write(' '.join(a)+'\n')
on=lambda name:(root/name).exists()
if a[:2]==['socraticode','install']:
    (root/'no-plugin').unlink(missing_ok=True);print(json.dumps({'installed':True}));sys.exit()
if a[:2]==['socraticode','install-qdrant']:
    (root/'native').touch();print(json.dumps({'installed':True}));sys.exit()
if a[:2]==['socraticode','connect']:
    (root/'connected').touch();print(json.dumps({'connected':True}));sys.exit()
if a[:2]==['socraticode','start']:
    (root/'running').touch();print(json.dumps({'started':True}));sys.exit()
if a[:2]==['socraticode','stop']:
    (root/'running').unlink(missing_ok=True);print(json.dumps({'stopped':True}));sys.exit()
index={'state':'indexed','chunks':12345,'files_indexed':321,'last_indexed_at':'2026-10-20T08:00:00.000Z'} if on('indexed') else {'state':'none'}
mode='external' if on('connected') else 'native' if a[-2:]==['--qdrant','native'] else 'managed'
print(json.dumps({'path':a[2],'project_id':'abc','plugin':{'installed':not on('no-plugin')},'docker':False,
  'qdrant':{'mode':mode,'url':'http://127.0.0.1:6333','reachable':on('native'),'agents':mode=='external',
            'native':{'installed':on('native'),'version':'1.19.1','installable':True}},
  'ollama':{'provider':'ollama','reachable':True,'model':'nomic-embed-text','model_present':True},'node':True,
  'index':index,'watching':on('running'),'runner':{'running':on('running'),'phase':'watching' if on('running') else None}}))
)PY"));
    }
    // Off by default: SocratiCode's Docker Qdrant, and a hint at the setting.
    // Extensions are off unless the build turns them on (the fork builds do).
    void extensionFollowsTheBuildDefault()
    {
        QSettings().remove("extensions/socraticode");
        QCOMPARE(Extensions::enabled("socraticode"), Extensions::enabledByDefault());
        Extensions::setEnabled("socraticode", !Extensions::enabledByDefault());
        QCOMPARE(Extensions::enabled("socraticode"), !Extensions::enabledByDefault());
        QSettings().remove("extensions/socraticode");
    }
    void dockerIsTheDefault()
    {
        HgsClient client(program());
        SocratiCodePanel panel(&client);
        panel.show();
        panel.setContext({}, "/workspace/example");
        QTRY_VERIFY(calls().contains("socraticode status /workspace/example --qdrant docker"));
        QTRY_VERIFY2(text(panel, "socratiCodeDetails").contains("turn on a native Qdrant"), qPrintable(text(panel, "socratiCodeDetails")));
        QVERIFY(!button(panel, "socratiCodeAction")->isVisible());
        QVERIFY(!button(panel, "socratiCodeConnect")->isVisible());
    }
    void offersTheNextStep()
    {
        QSettings().setValue("socraticode/nativeQdrant", true);
        HgsClient client(program());
        SocratiCodePanel panel(&client);
        panel.show();
        flag("no-plugin");
        panel.setContext({}, "/workspace/example");
        auto *action = button(panel, "socratiCodeAction");
        QTRY_COMPARE(action->text(), QString("Install plugin"));
        QTRY_VERIFY2(text(panel, "socratiCodeDetails").contains("not installed yet"), qPrintable(text(panel, "socratiCodeDetails")));
        QTRY_VERIFY(action->isEnabled());
        action->click();
        QTRY_VERIFY(calls().contains("socraticode install"));
        // Without Docker or Qdrant, the step is the native Qdrant.
        QTRY_COMPARE(action->text(), QString("Install Qdrant…"));
        QTRY_VERIFY(action->isEnabled());
        confirmNext();
        action->click();
        QTRY_VERIFY(calls().contains("socraticode install-qdrant"));
        // Zerus uses it; the folder's agents are not pointed at it yet.
        QTRY_COMPARE(action->text(), QString("Index and watch"));
        auto *connectAgents = button(panel, "socratiCodeConnect");
        QVERIFY(connectAgents->isVisible());
        QVERIFY(text(panel, "socratiCodeDetails").contains("agents still look"));
        QTRY_VERIFY(connectAgents->isEnabled());
        confirmNext();
        connectAgents->click();
        QTRY_VERIFY(calls().contains("socraticode connect /workspace/example --qdrant native"));
        QTRY_VERIFY(!connectAgents->isVisible());
        flag("indexed");
        QTRY_VERIFY(button(panel, "socratiCodeRefresh")->isEnabled());
        button(panel, "socratiCodeRefresh")->click();
        QTRY_COMPARE(action->text(), QString("Watch"));
        QVERIFY2(text(panel, "socratiCodeState").startsWith("Indexed · 12"), qPrintable(text(panel, "socratiCodeState")));
        QVERIFY(text(panel, "socratiCodeState").endsWith("not watched"));
        QVERIFY(!button(panel, "socratiCodeStop")->isVisible());
        QTRY_VERIFY(action->isEnabled());
        action->click();
        QTRY_VERIFY(calls().contains("socraticode start /workspace/example --qdrant native"));
        QTRY_VERIFY(button(panel, "socratiCodeStop")->isVisible());
        QVERIFY(!action->isVisible());
        QVERIFY(text(panel, "socratiCodeState").endsWith("watching"));
        QTRY_VERIFY(button(panel, "socratiCodeStop")->isEnabled());
        button(panel, "socratiCodeStop")->click();
        QTRY_VERIFY(calls().contains("socraticode stop /workspace/example --qdrant native"));
        QTRY_VERIFY(!button(panel, "socratiCodeStop")->isVisible());
    }
    void remoteFoldersAskTheirComputer()
    {
        HgsClient client(program());
        SocratiCodePanel panel(&client);
        panel.show();
        panel.setContext("mac", "/workspace/example");
        QTRY_VERIFY(calls().contains("@mac socraticode status /workspace/example --qdrant docker"));
        panel.setContext({}, {});
        QVERIFY(!panel.isVisible());
    }
};

QTEST_MAIN(TestSocratiCodePanel)
#include "test_socraticodepanel.moc"
