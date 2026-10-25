#include <QtTest>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLineEdit>
#include <QLabel>
#include <QPushButton>
#include <QSignalSpy>
#include <QTemporaryDir>
#include "FakeCli.h"
#include "OpenCodeExistingDialog.h"

class TestOpenCodeExisting : public QObject {
    Q_OBJECT
private slots:
    void init();
    void previewAndBindPreserveTheExactSelection();
    void editsInvalidateThePreview();
    void mismatchedReplyNeverEnablesBinding();
    void failedCommitCanBeReconciledWithTheSameRequest();
private:
    void fill(OpenCodeExistingDialog &d) {
        d.findChild<QLineEdit *>("existingConversationId")->setText("ses_selected");
        d.findChild<QLineEdit *>("existingConversationDirectory")->setText(QString::fromUtf8("/workspace/space 世界"));
        d.findChild<QLineEdit *>("existingConversationName")->setText("opencode/project/existing");
    }
    QList<QJsonObject> calls() const {
        QFile f(m_dir.filePath("calls")); if (!f.open(QIODevice::ReadOnly)) return {};
        QList<QJsonObject> out; for (auto row : f.readAll().split('\n')) if (!row.isEmpty()) out << QJsonDocument::fromJson(row).object(); return out;
    }
    void mode(const char *text) { QFile f(m_dir.filePath("mode")); QVERIFY(f.open(QIODevice::WriteOnly)); f.write(text); }
    QString program() const { return FakeCli::program(m_dir.filePath("hgs")); }
    QTemporaryDir m_dir;
};
void TestOpenCodeExisting::init()
{
    QVERIFY(m_dir.isValid()); QFile::remove(m_dir.filePath("calls")); QFile::remove(m_dir.filePath("mode"));
    QVERIFY(FakeCli::install(m_dir.filePath("hgs"), R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
assert sys.argv[1:]==['@wsl-fixture','account','bind-existing','native-opencode','--json']
p=json.load(sys.stdin)
with (root/'calls').open('a',encoding='utf-8') as f:f.write(json.dumps(p)+'\n')
mode=(root/'mode').read_text() if (root/'mode').exists() else ''
if mode=='commit-error' and not p['preview']:
 print('Native identity changed',file=sys.stderr);sys.exit(1)
a={k:p[k] for k in ['schema_version','request_id','name','conversation_id','directory','agent_home']}
a.update(status='preview' if p['preview'] else 'bound',native_changed=False,identity='a'*64,native_title='Existing title',run_id='11111111-2222-4333-8444-555555555555')
if mode=='wrong-id':a['conversation_id']='ses_wrong'
print(json.dumps(a))
)PY"));
}
void TestOpenCodeExisting::previewAndBindPreserveTheExactSelection()
{
    OpenCodeExistingDialog d(program(),"wsl-fixture","/home/alice/.config/opencode");d.show();
    auto *preview=d.findChild<QPushButton *>("previewExistingConversation"), *bind=d.findChild<QPushButton *>("bindExistingConversation");
    QVERIFY(preview && bind); QVERIFY(!bind->isEnabled());QTest::qWait(30);QVERIFY(calls().isEmpty());
    fill(d);QVERIFY(!bind->isEnabled());preview->click();QTRY_VERIFY(bind->isEnabled());
    QCOMPARE(calls().size(),1);auto first=calls().first();QVERIFY(first["preview"].toBool());QVERIFY(!first.contains("expected_identity"));
    QCOMPARE(first["agent_home"].toString(),QString("/home/alice/.config/opencode"));
    QSignalSpy accepted(&d,&QDialog::accepted);bind->click();QTRY_COMPARE(accepted.size(),1);
    QCOMPARE(calls().size(),2);auto second=calls().last();QVERIFY(!second["preview"].toBool());
    QCOMPARE(second["expected_identity"].toString(),QString(64,'a'));
    for(const char *key:{"request_id","name","conversation_id","directory","agent_home"})QCOMPARE(second.value(key),first.value(key));
    QCOMPARE(d.bound()["status"].toString(),QString("bound"));
}
void TestOpenCodeExisting::editsInvalidateThePreview()
{
    OpenCodeExistingDialog d(program(),"wsl-fixture","/home/alice/.config/opencode");fill(d);d.show();
    auto *preview=d.findChild<QPushButton *>("previewExistingConversation"), *bind=d.findChild<QPushButton *>("bindExistingConversation");
    preview->click();QTRY_VERIFY(bind->isEnabled());const QJsonValue before=calls().last().value("request_id");
    d.findChild<QLineEdit *>("existingConversationName")->setText("opencode/project/edited");QVERIFY(!bind->isEnabled());
    bind->click();QTest::qWait(20);QCOMPARE(calls().size(),1);
    preview->click();QTRY_VERIFY(bind->isEnabled());QVERIFY(calls().last()["request_id"]!=before);
}
void TestOpenCodeExisting::mismatchedReplyNeverEnablesBinding()
{
    mode("wrong-id");OpenCodeExistingDialog d(program(),"wsl-fixture","/home/alice/.config/opencode");fill(d);d.show();
    d.findChild<QPushButton *>("previewExistingConversation")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("existingConversationNotice")->text().contains("does not match"));
    QVERIFY(!d.findChild<QPushButton *>("bindExistingConversation")->isEnabled());QCOMPARE(calls().size(),1);
}
void TestOpenCodeExisting::failedCommitCanBeReconciledWithTheSameRequest()
{
    OpenCodeExistingDialog d(program(),"wsl-fixture","/home/alice/.config/opencode");fill(d);d.show();
    auto *preview=d.findChild<QPushButton *>("previewExistingConversation"), *bind=d.findChild<QPushButton *>("bindExistingConversation");
    preview->click();QTRY_VERIFY(bind->isEnabled());const QJsonValue id=calls().last().value("request_id");
    mode("commit-error");bind->click();QTRY_VERIFY(preview->isEnabled());QVERIFY(!bind->isEnabled());
    QVERIFY(d.findChild<QLabel *>("existingConversationNotice")->text().contains("not confirmed"));
    mode("");preview->click();QTRY_VERIFY(bind->isEnabled());QCOMPARE(calls().last()["request_id"],id);
}
QTEST_MAIN(TestOpenCodeExisting)
#include "test_opencodeexisting.moc"
