#include <QtTest>
#include "Settings.h"
#include <QFile>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QSettings>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QCryptographicHash>
#include <QScopeGuard>
#include <QProcess>
#ifdef Q_OS_WIN
#include <qt_windows.h>
#endif
#include "FakeCli.h"
#include "ExternalFile.h"
#include "MappedLocalFile.h"
#include "WslFileOpenDialog.h"

class TestWslFileOpen : public QObject {
    Q_OBJECT
private slots:
    void init();
    void cleanup(){ExternalFile::setRevealHandler({});}
    void strictDriveSyntaxRejectsAliasesAndSpecialNames();
    void localGuardPinsBytesAndRefusesChangedContents();
    void junctionParentsAreNeverFollowed();
    void previewNeverOpensAndCommitPreservesOriginalMarker();
    void localMismatchDoesNotOpen();
    void staleOrForeignProofNeverOpens();
    void editingMappingInvalidatesPreview();
    void runnableOriginalIsRevealedWithoutExecution();
    void closingAPendingDialogNeverOpensLater();
private:
    QJsonObject scope()const{return {{"session","opencode/project/existing"},{"run_id","run-a"},{"conversation_id","ses_a"},{"path","/mnt/c/fixture/result.txt"}};}
    QString program()const{return FakeCli::program(m_dir.filePath("hgs"));}
    QString file()const{return m_dir.filePath("result.txt");}
    void fill(WslFileOpenDialog &dialog){
        auto *root=dialog.findChild<QLineEdit *>("fileMappingWindowsRoot");QVERIFY(root);QTRY_VERIFY(root->isEnabled());
        root->setText(m_dir.path());dialog.findChild<QLineEdit *>("fileMappingMountRoot")->setText("/mnt");
        dialog.findChild<QLineEdit *>("fileMappingWorkspaceId")->setText("fixture");
    }
    void mode(const char *text){QFile f(m_dir.filePath("mode"));QVERIFY(f.open(QIODevice::WriteOnly));f.write(text);}
    void write(const QString &path,const QByteArray &data){QFile f(path);QVERIFY(f.open(QIODevice::WriteOnly));QCOMPARE(f.write(data),data.size());}
    void preview(WslFileOpenDialog &d){d.findChild<QPushButton *>("previewMappedFile")->click();QTRY_VERIFY_WITH_TIMEOUT(d.findChild<QPushButton *>("openMappedFile")->isEnabled(),5000);}
    QTemporaryDir m_dir;
};
void TestWslFileOpen::init()
{
    QVERIFY(m_dir.isValid());QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,m_dir.path());
    QCoreApplication::setOrganizationName("hgs-mapped-file-test");QCoreApplication::setApplicationName("mapped-files");Settings::flush();QSettings().clear();
    QFile::remove(m_dir.filePath("mode"));QFile::remove(m_dir.filePath("calls"));
    write(file(),"fixture content\n");write(m_dir.filePath("result.ps1"),"fixture content\n");
    QVERIFY(FakeCli::install(m_dir.filePath("hgs"),R"PY(#!/usr/bin/env python3
import hashlib,json,pathlib,sys,time
root=pathlib.Path(__file__).parent;a=sys.argv[1:]
mode=(root/'mode').read_text() if (root/'mode').exists() else ''
if a==['--help']:print('hgs machine file-proof <alias> --json (wsl-file-proof-v1)');sys.exit()
if a==['machine','ls']:
 print(json.dumps({'local':'fixture-machine','machines':[{'alias':'wsl-fixture','connection':{'transport':'wsl-local','enabled':True,'wsl':{'distribution':'Ubuntu-24.04','user':'alice','hgs_path':'/opt/zerus/hgs'}}}]}));sys.exit()
assert a==['machine','file-proof','wsl-fixture','--json']
p=json.load(sys.stdin);s=p['selection'];m=s['mapping']
with (root/'calls').open('a') as f:f.write(json.dumps(p)+'\n')
if mode=='delayed':time.sleep(4)
data=b'fixture content\n'
target=str(root/('result.ps1' if mode=='script' else 'result.txt'))
v={'schema_version':1,'selection':s,'alias':'wsl-fixture','native_path':s['path'],'native_changed':False,'local_verified':False,
 'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'preview_identity':'a'*64,
 'target':{'schema_version':1,'machine_id':s['machine_id'],'environment_id':'windows-native','path_flavor':'windows',
 'native_path':target,'canonical_path':target.replace('\\','/'),'workspace_id':m['workspace_id'],'mapping_id':m['mapping_id']}}
if mode=='foreign':v['selection']={**s,'conversation_id':'ses_wrong'}
if mode=='stale' and 'expected_preview' in p:v['preview_identity']='b'*64
print(json.dumps(v))
)PY"));
}
void TestWslFileOpen::strictDriveSyntaxRejectsAliasesAndSpecialNames()
{
    for(const auto &path:{QString("C:/Projects/a.txt"),QString("C:\\Projects\\with space\\x%20.txt")})QVERIFY(MappedLocalFile::safeDrivePath(path));
    for(const auto &path:{QString("relative.txt"),QString("//server/share/a.txt"),QString("\\\\?\\C:\\file.txt"),QString("C:/NUL.txt"),
        QString("C:/x/COM1.txt"),QString("C:/x/a."),QString("C:/x/a "),QString("C:/x/a:stream"),QString("C:/x/../file.txt"),
        QString("C:/x//file.txt"),QString("C:/x/a\n.txt"),QString("C:/")})QVERIFY2(!MappedLocalFile::safeDrivePath(path),qPrintable(path));
}
void TestWslFileOpen::localGuardPinsBytesAndRefusesChangedContents()
{
#ifndef Q_OS_WIN
    QSKIP("Original-file handles are a Windows-only gate");
#else
    const auto digest=QString::fromLatin1(QCryptographicHash::hash("fixture content\n",QCryptographicHash::Sha256).toHex());
    {
        const auto result=MappedLocalFile::verify(file(),digest,16);QVERIFY2(result.ok(),qPrintable(result.error));
        QFile modify(file());QVERIFY(!modify.open(QIODevice::WriteOnly));QVERIFY(!QFile::rename(file(),m_dir.filePath("moved.txt")));
    }
    QVERIFY(!MappedLocalFile::verify(file(),QString(64,'0'),16).ok());
    QVERIFY(!MappedLocalFile::verify(file(),digest,17).ok());
    QVERIFY(!MappedLocalFile::verify(m_dir.path(),digest,16).ok());
#endif
}
void TestWslFileOpen::junctionParentsAreNeverFollowed()
{
#ifndef Q_OS_WIN
    QSKIP("Reparse-point proof is a Windows-only filesystem gate");
#else
    const auto target=m_dir.filePath("junction-target"),link=m_dir.filePath("junction-alias");
    QVERIFY(QDir().mkpath(target));write(target+"/file.txt","fixture content\n");
    QCOMPARE(QProcess::execute("cmd.exe",{"/c","mklink","/J",QDir::toNativeSeparators(link),QDir::toNativeSeparators(target)}),0);
    const auto cleanup=qScopeGuard([&]{const auto path=QDir::toNativeSeparators(link).toStdWString();RemoveDirectoryW(path.c_str());});
    const auto digest=QString::fromLatin1(QCryptographicHash::hash("fixture content\n",QCryptographicHash::Sha256).toHex());
    const auto proof=MappedLocalFile::verify(link+"/file.txt",digest,16);
    QVERIFY(!proof.ok());QVERIFY(proof.error.contains("reparse"));
    QVERIFY(QFileInfo::exists(target+"/file.txt"));
#endif
}
void TestWslFileOpen::previewNeverOpensAndCommitPreservesOriginalMarker()
{
#ifndef Q_OS_WIN
    QSKIP("Positive original-file qualification requires Windows");
#else
    int opened=0;const auto marker=ExternalFile::downloadMarker(file());
    WslFileOpenDialog d(program(),"wsl-fixture",scope(),nullptr,{},[&](const QString &path){
        return ExternalFile::open(path,ExternalFile::Origin::ThisComputer,[&](const QUrl &url){++opened;return url.toLocalFile()==file();});
    });d.show();fill(d);QCOMPARE(opened,0);preview(d);QCOMPARE(opened,0);
    QCOMPARE(ExternalFile::downloadMarker(file()),marker);QSignalSpy accepted(&d,&QDialog::accepted);
    d.findChild<QPushButton *>("openMappedFile")->click();QTRY_COMPARE_WITH_TIMEOUT(accepted.size(),1,5000);QCOMPARE(opened,1);
    QCOMPARE(ExternalFile::downloadMarker(file()),marker);QVERIFY(QSettings().allKeys().isEmpty());
#endif
}
void TestWslFileOpen::localMismatchDoesNotOpen()
{
#ifndef Q_OS_WIN
    QSKIP("Local digest qualification requires Windows");
#else
    int opened=0;write(file(),"changed content\n");
    WslFileOpenDialog d(program(),"wsl-fixture",scope(),nullptr,{},[&](const QString &){++opened;return ExternalFile::Outcome::Opened;});d.show();fill(d);
    d.findChild<QPushButton *>("previewMappedFile")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("mappedFileNotice")->text().contains("different contents"));
    QVERIFY(!d.findChild<QPushButton *>("openMappedFile")->isEnabled());QCOMPARE(opened,0);
#endif
}
void TestWslFileOpen::staleOrForeignProofNeverOpens()
{
#ifndef Q_OS_WIN
    QSKIP("Positive preview before stale-proof test requires Windows");
#else
    int opened=0;WslFileOpenDialog d(program(),"wsl-fixture",scope(),nullptr,{},[&](const QString &){++opened;return ExternalFile::Outcome::Opened;});d.show();fill(d);
    mode("foreign");d.findChild<QPushButton *>("previewMappedFile")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("mappedFileNotice")->text().contains("no longer matches"));QCOMPARE(opened,0);
    mode("");preview(d);mode("stale");d.findChild<QPushButton *>("openMappedFile")->click();
    QTRY_VERIFY(d.findChild<QLabel *>("mappedFileNotice")->text().contains("no longer matches"));QCOMPARE(opened,0);
#endif
}
void TestWslFileOpen::editingMappingInvalidatesPreview()
{
#ifndef Q_OS_WIN
    QSKIP("Positive preview requires Windows");
#else
    WslFileOpenDialog d(program(),"wsl-fixture",scope());d.show();fill(d);preview(d);
    d.findChild<QLineEdit *>("fileMappingWorkspaceId")->setText("changed");
    QVERIFY(!d.findChild<QPushButton *>("openMappedFile")->isEnabled());
#endif
}
void TestWslFileOpen::runnableOriginalIsRevealedWithoutExecution()
{
#ifndef Q_OS_WIN
    QSKIP("Original Windows script handling requires Windows");
#else
    mode("script");int opened=0,revealed=0;ExternalFile::setRevealHandler([&](const QString &path){++revealed;return path.endsWith("result.ps1");});
    WslFileOpenDialog d(program(),"wsl-fixture",scope(),nullptr,{},[&](const QString &path){
        return ExternalFile::open(path,ExternalFile::Origin::ThisComputer,[&](const QUrl &){++opened;return true;});
    });d.show();fill(d);preview(d);QSignalSpy accepted(&d,&QDialog::accepted);d.findChild<QPushButton *>("openMappedFile")->click();
    QTRY_COMPARE(accepted.size(),1);QCOMPARE(opened,0);QCOMPARE(revealed,1);QVERIFY(d.outcomeNotice().contains("shown"));
#endif
}
void TestWslFileOpen::closingAPendingDialogNeverOpensLater()
{
#ifndef Q_OS_WIN
    QSKIP("Positive preview requires Windows");
#else
    mode("delayed");int opened=0;
    auto *d=new WslFileOpenDialog(program(),"wsl-fixture",scope(),nullptr,{},[&](const QString &){++opened;return ExternalFile::Outcome::Opened;});
    d->show();fill(*d);d->findChild<QPushButton *>("previewMappedFile")->click();delete d;QTest::qWait(100);QCOMPARE(opened,0);
#endif
}
QTEST_MAIN(TestWslFileOpen)
#include "test_wslfileopen.moc"
