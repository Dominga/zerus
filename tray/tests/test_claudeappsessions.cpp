#include <QtTest>
#include <QApplication>
#include <QCheckBox>
#include <QFile>
#include <QLineEdit>
#include <QMessageBox>
#include <QPushButton>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QTreeWidget>
#include "FakeCli.h"
#include "ClaudeAppSessionsDialog.h"

// WhiteWh fork: Claude desktop app sessions continue as new Zerus sessions.
class TestClaudeAppSessions : public QObject {
    Q_OBJECT
    QTemporaryDir m_fixture;
    QString program() const { return FakeCli::program(m_fixture.filePath("hgs")); }
    QStringList calls() const
    {
        QFile f(m_fixture.filePath("calls"));
        if (!f.open(QIODevice::ReadOnly)) return {};
        return QString::fromUtf8(f.readAll()).split('\n', Qt::SkipEmptyParts);
    }
    static void answerNext(QMessageBox::StandardButton button)
    {
        QTimer::singleShot(50, [button] {
            if (auto *box = qobject_cast<QMessageBox *>(QApplication::activeModalWidget())) box->button(button)->click();
            else answerNext(button);
        });
    }
private slots:
    void init()
    {
        QFile::remove(m_fixture.filePath("calls"));
        QVERIFY(FakeCli::install(m_fixture.filePath("hgs"), R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
root=pathlib.Path(__file__).parent
a=sys.argv[1:]
with open(root/'calls','a') as f:f.write(' '.join(a)+'\n')
if a and a[0].startswith('@'):a=a[1:]
now=time.time()
def s(i,title,cwd,**k):
    d={'id':'local_'+i,'conversation':i*4+'-0000-4000-8000-000000000000','cwd':cwd,'title':title,'suggested_name':title.lower().replace(' ','-') or 'app-'+i,
       'model':'claude-opus-5-5','archived':False,'last_activity':now-600,'history':True,'folder_exists':True,'running':False,'running_process':False}
    d.update(k);return d
if a[:2]==['claude-app','ls']:
    print(json.dumps({'available':True,'truncated':False,'sessions':[
        s('aa','Video view','/workspace/example'),
        s('bb','Open one','/workspace/example',running=True,running_process=True),
        s('cc','Lost history','/workspace/example',history=False),
        s('dd','Elsewhere','/workspace/other')]}))
elif a[:2]==['claude-app','resume']:
    print('ok')
else:
    sys.exit(3)
)PY"));
    }
    void listsTheFolderAndContinuesAStoppedSession()
    {
        HgsClient client(program());
        ClaudeAppSessionsDialog dialog(&client, {}, "/workspace/example/");
        dialog.show();
        auto *list = dialog.findChild<QTreeWidget *>("claudeAppSessions");
        QTRY_COMPARE(list->topLevelItemCount(), 3);
        auto *name = dialog.findChild<QLineEdit *>("claudeAppName");
        auto *start = dialog.findChild<QPushButton *>("primary");
        QCOMPARE(list->currentItem()->text(0), QString("Video view"));
        QCOMPARE(name->text(), QString("video-view"));
        QVERIFY(start->isEnabled());
        // No history here: nothing to continue.
        list->setCurrentItem(list->topLevelItem(2));
        QVERIFY(!start->isEnabled());
        QCOMPARE(list->topLevelItem(2)->text(3), QString("No history"));
        // Every folder.
        dialog.findChild<QCheckBox *>("claudeAppOnlyFolder")->setChecked(false);
        QCOMPARE(list->topLevelItemCount(), 4);
        dialog.findChild<QCheckBox *>("claudeAppOnlyFolder")->setChecked(true);
        list->setCurrentItem(list->topLevelItem(0));
        name->setText("bad.name");
        QVERIFY(!start->isEnabled());
        name->setText("video");
        start->click();
        QCOMPARE(dialog.result(), int(QDialog::Accepted));
        QCOMPARE(dialog.sessionId(), QString("local_aa"));
        QCOMPARE(dialog.folder(), QString("/workspace/example"));
        QCOMPARE(dialog.name(), QString("video"));
        QVERIFY(!dialog.force());
    }
    void anOpenSessionAsksFirst()
    {
        HgsClient client(program());
        ClaudeAppSessionsDialog dialog(&client, {}, "/workspace/example");
        dialog.show();
        auto *list = dialog.findChild<QTreeWidget *>("claudeAppSessions");
        QTRY_COMPARE(list->topLevelItemCount(), 3);
        list->setCurrentItem(list->topLevelItem(1));
        QCOMPARE(list->currentItem()->text(3), QString("Open in the app"));
        answerNext(QMessageBox::Cancel);
        dialog.findChild<QPushButton *>("primary")->click();
        QVERIFY(dialog.isVisible());
        answerNext(QMessageBox::Yes);
        dialog.findChild<QPushButton *>("primary")->click();
        QTRY_COMPARE(dialog.result(), int(QDialog::Accepted));
        QVERIFY(dialog.force());
    }
    void resumeStartsADetachedLaunch()
    {
        HgsClient client(program());
        QSignalSpy launched(&client, &HgsClient::nativeSessionLaunched);
        client.resumeClaudeAppSession("mac", "local_aa", "video", "11111111-2222-4333-8444-555555555555", true);
        QTRY_COMPARE(launched.size(), 1);
        QCOMPARE(launched[0][1].toBool(), true);
        QVERIFY(calls().contains("@mac claude-app resume local_aa -n video --launch-id 11111111-2222-4333-8444-555555555555 -d --force"));
    }
};

QTEST_MAIN(TestClaudeAppSessions)
#include "test_claudeappsessions.moc"
