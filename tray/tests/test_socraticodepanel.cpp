#include <QtTest>
#include <QTemporaryDir>
#include <QFile>
#include <QLabel>
#include <QPushButton>
#include "FakeCli.h"
#include "SocratiCodePanel.h"

// WhiteWh fork: the session folder's SocratiCode strip offers one next step.
class TestSocratiCodePanel : public QObject {
    Q_OBJECT
    QTemporaryDir m_fixture;
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
private slots:
    void init()
    {
        for (const auto *name : {"no-plugin", "no-docker", "indexed", "running", "calls"}) flag(name, false);
        QVERIFY(FakeCli::install(m_fixture.filePath("hgs"), R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
with open(root/'calls','a') as f:f.write(' '.join(a)+'\n')
on=lambda name:(root/name).exists()
if a[:2]==['socraticode','install']:
    (root/'no-plugin').unlink(missing_ok=True);print(json.dumps({'installed':True}));sys.exit()
if a[:2]==['socraticode','start']:
    (root/'running').touch();print(json.dumps({'started':True}));sys.exit()
if a[:2]==['socraticode','stop']:
    (root/'running').unlink(missing_ok=True);print(json.dumps({'stopped':True}));sys.exit()
index={'state':'indexed','chunks':12345,'files_indexed':321,'last_indexed_at':'2026-10-20T08:00:00.000Z'} if on('indexed') else {'state':'none'}
print(json.dumps({'path':a[2],'project_id':'abc','plugin':{'installed':not on('no-plugin')},'docker':not on('no-docker'),
  'qdrant':{'reachable':not on('no-docker')},'ollama':{'reachable':True,'model':'nomic-embed-text','model_present':True},'node':True,
  'index':index,'watching':on('running'),'runner':{'running':on('running'),'phase':'watching' if on('running') else None}}))
)PY"));
    }
    void offersTheNextStep()
    {
        HgsClient client(program());
        SocratiCodePanel panel(&client);
        panel.show();
        flag("no-plugin");
        flag("no-docker");
        panel.setContext({}, "/workspace/example");
        auto *action = button(panel, "socratiCodeAction");
        QTRY_COMPARE(action->text(), QString("Install plugin"));
        QVERIFY(text(panel, "socratiCodeDetails").contains("Docker"));
        QTRY_VERIFY2(action->isEnabled(),qPrintable(calls().join(" / ")));action->click();
        QTRY_VERIFY2(calls().contains("socraticode install"),qPrintable(calls().join(" / ")));
        // Without Docker, the step is getting it.
        QTRY_COMPARE(action->text(), QString("Get Docker…"));
        flag("no-docker", false);
        flag("indexed");
        button(panel, "socratiCodeRefresh")->click();
        QTRY_COMPARE(action->text(), QString("Watch"));
        QVERIFY2(text(panel, "socratiCodeState").startsWith("Indexed · 12"), qPrintable(text(panel, "socratiCodeState")));
        QVERIFY(text(panel, "socratiCodeState").endsWith("not watched"));
        QVERIFY(!button(panel, "socratiCodeStop")->isVisible());
        action->click();
        QTRY_VERIFY(calls().contains("socraticode start /workspace/example"));
        QTRY_VERIFY(button(panel, "socratiCodeStop")->isVisible());
        QVERIFY(!action->isVisible());
        QVERIFY(text(panel, "socratiCodeState").endsWith("watching"));
        button(panel, "socratiCodeStop")->click();
        QTRY_VERIFY(calls().contains("socraticode stop /workspace/example"));
        QTRY_VERIFY(!button(panel, "socratiCodeStop")->isVisible());
    }
    void remoteFoldersAskTheirComputer()
    {
        HgsClient client(program());
        SocratiCodePanel panel(&client);
        panel.show();
        panel.setContext("mac", "/workspace/example");
        QTRY_VERIFY(calls().contains("@mac socraticode status /workspace/example"));
        panel.setContext({}, {});
        QVERIFY(!panel.isVisible());
    }
};

QTEST_MAIN(TestSocratiCodePanel)
#include "test_socraticodepanel.moc"
