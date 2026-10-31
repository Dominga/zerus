#include <QtTest>
#include "Settings.h"
#include <QCheckBox>
#include <QFile>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include "FakeCli.h"
#include "WslPathMappingDialog.h"

class TestWslPathMapping : public QObject {
    Q_OBJECT
private slots:
    void init();
    void onlyExplicitPreviewAndUsePersistsTheMapping();
    void changingAnInputInvalidatesTheResult();
    void changedConnectionRefusesTheMappedPath();
    void wrongEnvironmentReplyIsRejected();
    void backendDenialLeavesUseDisabled();
private:
    QJsonObject profile() const {return {{"distribution","Ubuntu-24.04"},{"user","alice"},{"hgs_path","/opt/zerus/hgs"}};}
    QString program() const {return FakeCli::program(m_dir.filePath("hgs"));}
    void fill(WslPathMappingDialog &d){
        d.findChild<QLineEdit *>("mappingWindowsPath")->setText("C:/Projects/Orbit/Folder");
        d.findChild<QLineEdit *>("mappingWindowsRoot")->setText("C:/Projects/Orbit");
        d.findChild<QLineEdit *>("mappingMountRoot")->setText("/mnt");
        d.findChild<QLineEdit *>("mappingWorkspaceId")->setText("orbit");
    }
    void mode(const char *text){QFile f(m_dir.filePath("mode"));QVERIFY(f.open(QIODevice::WriteOnly));f.write(text);}
    int calls()const{QFile f(m_dir.filePath("calls"));if(!f.open(QIODevice::ReadOnly))return 0;return f.readAll().count('\n');}
    QTemporaryDir m_dir;
};
void TestWslPathMapping::init()
{
    QVERIFY(m_dir.isValid());QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,m_dir.path());
    QCoreApplication::setOrganizationName("hgs-map-test");QCoreApplication::setApplicationName("mapping");Settings::flush();QSettings().clear();
    QFile::remove(m_dir.filePath("calls"));QFile::remove(m_dir.filePath("mode"));
    QVERIFY(FakeCli::install(m_dir.filePath("hgs"),R"PY(#!/usr/bin/env python3
import json,pathlib,sys
r=pathlib.Path(__file__).parent;a=sys.argv[1:]
with (r/'calls').open('a') as f:f.write(json.dumps(a)+'\n')
mode=(r/'mode').read_text() if (r/'mode').exists() else ''
if a==['machine','ls']:
 p={'distribution':'Ubuntu-24.04','user':'bob' if mode=='changed' else 'alice','hgs_path':'/opt/zerus/hgs'}
 print(json.dumps({'local':'machine-a','machines':[{'alias':'wsl-fixture','connection':{'transport':'wsl-local','wsl':p}}]}));sys.exit()
assert a[0]=='path-ref' and a[1] in ['describe','map']
x=json.load(sys.stdin);p=x['path'];m=x['mapping']
if mode=='denied':print('path is outside the mapped workspace',file=sys.stderr);sys.exit(2)
assert m['distribution']=='Ubuntu-24.04' and m['user']=='alice'
assert m['mount_root']=='/mnt' and m['windows_root']=='C:/Projects/Orbit'
y=dict(p,schema_version=1,mapping_id=m['mapping_id'],canonical_path='c:/Projects/Orbit/Folder')
if a[1]=='map':y.update(environment_id='wsl:Ubuntu-24.04:bob' if mode=='wrong-environment' else 'wsl:Ubuntu-24.04:alice',path_flavor='posix',native_path='/mnt/c/Projects/Orbit/Folder')
print(json.dumps(y))
)PY"));
}
void TestWslPathMapping::onlyExplicitPreviewAndUsePersistsTheMapping()
{
    WslPathMappingDialog d(program(),"wsl-fixture","machine-a",profile());d.show();QTest::qWait(20);QCOMPARE(calls(),0);
    fill(d);auto *preview=d.findChild<QPushButton *>("previewWslMapping"),*use=d.findChild<QPushButton *>("useWslMapping");QVERIFY(!use->isEnabled());
    preview->click();QTRY_VERIFY(use->isEnabled());QCOMPARE(calls(),2);QVERIFY(QSettings().allKeys().isEmpty());
    QCOMPARE(d.mappedPath(),QString("/mnt/c/Projects/Orbit/Folder"));
    d.findChild<QCheckBox *>("rememberWslMapping")->setChecked(true);QSignalSpy accepted(&d,&QDialog::accepted);use->click();QTRY_COMPARE(accepted.size(),1);
    QCOMPARE(calls(),3);QCOMPARE(QSettings().allKeys().size(),1);
    WslPathMappingDialog next(program(),"wsl-fixture","machine-a",profile());
    QCOMPARE(next.findChild<QLineEdit *>("mappingWindowsRoot")->text(),QString("C:/Projects/Orbit"));
    QVERIFY(next.findChild<QLineEdit *>("mappingWindowsPath")->text().isEmpty());
    QVERIFY(!next.findChild<QPushButton *>("useWslMapping")->isEnabled());
    auto changed=profile();changed["user"]="bob";WslPathMappingDialog other(program(),"wsl-fixture","machine-a",changed);
    QVERIFY(other.findChild<QLineEdit *>("mappingWindowsRoot")->text().isEmpty());
}
void TestWslPathMapping::changingAnInputInvalidatesTheResult()
{
    WslPathMappingDialog d(program(),"wsl-fixture","machine-a",profile());fill(d);d.show();
    auto *use=d.findChild<QPushButton *>("useWslMapping");d.findChild<QPushButton *>("previewWslMapping")->click();QTRY_VERIFY(use->isEnabled());
    d.findChild<QLineEdit *>("mappingWindowsPath")->setText("C:/Other");QVERIFY(!use->isEnabled());QVERIFY(d.mappedPath().isEmpty());QCOMPARE(calls(),2);
}
void TestWslPathMapping::changedConnectionRefusesTheMappedPath()
{
    WslPathMappingDialog d(program(),"wsl-fixture","machine-a",profile());fill(d);d.show();
    auto *use=d.findChild<QPushButton *>("useWslMapping");d.findChild<QPushButton *>("previewWslMapping")->click();QTRY_VERIFY(use->isEnabled());
    mode("changed");d.findChild<QCheckBox *>("rememberWslMapping")->setChecked(true);QSignalSpy accepted(&d,&QDialog::accepted);use->click();
    QTRY_VERIFY(d.findChild<QLabel *>("pathMappingNotice")->text().contains("connection changed"));QCOMPARE(accepted.size(),0);QVERIFY(!use->isEnabled());QVERIFY(QSettings().allKeys().isEmpty());
}
void TestWslPathMapping::wrongEnvironmentReplyIsRejected()
{
    mode("wrong-environment");WslPathMappingDialog d(program(),"wsl-fixture","machine-a",profile());fill(d);d.show();d.findChild<QPushButton *>("previewWslMapping")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("pathMappingNotice")->text().contains("does not match"));QVERIFY(!d.findChild<QPushButton *>("useWslMapping")->isEnabled());
}
void TestWslPathMapping::backendDenialLeavesUseDisabled()
{
    mode("denied");WslPathMappingDialog d(program(),"wsl-fixture","machine-a",profile());fill(d);d.show();d.findChild<QPushButton *>("previewWslMapping")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("pathMappingNotice")->text().contains("outside"));QVERIFY(!d.findChild<QPushButton *>("useWslMapping")->isEnabled());QCOMPARE(calls(),1);
}
QTEST_MAIN(TestWslPathMapping)
#include "test_wslpathmapping.moc"
