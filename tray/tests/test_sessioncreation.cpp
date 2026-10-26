#include <QtTest>
#include <QFile>
#include <QDir>
#include <QFileInfo>
#include <QJsonDocument>
#include <QSettings>
#include <QSemaphore>
#include <QScopeGuard>
#include <atomic>
#include <memory>
#include "SessionCreationController.h"
#include "FakeCli.h"

class TestSessionCreation:public QObject {
    Q_OBJECT
    QTemporaryDir m_settings,m_fixture;
    QString program() const{return FakeCli::program(m_fixture.filePath("hgs"));}
    void flag(const QString &name){QFile f(m_fixture.filePath(name));QVERIFY(f.open(QIODevice::WriteOnly));}
    QStringList calls() const{QFile f(m_fixture.filePath("calls"));if(!f.open(QIODevice::ReadOnly))return {};return QString::fromUtf8(f.readAll()).split('\n',Qt::SkipEmptyParts);}
    QJsonObject plan(bool automatic=false) const{
        return {{"host",""},{"machine","fixture"},{"cmd","codex"},{"tag","creation-fixture"},
            {"group","ungrouped"},{"target","/workspace/example"},{"source","/workspace/example"},
            {"auto",automatic},{"branch","work-fixture"},{"repository",false}};
    }
private slots:
    void initTestCase(){
        QCoreApplication::setOrganizationName("hgs-tests");QCoreApplication::setApplicationName("session-creation");
        QSettings::setDefaultFormat(QSettings::IniFormat);QSettings::setPath(QSettings::IniFormat,QSettings::UserScope,m_settings.path());
        QVERIFY(FakeCli::install(m_fixture.filePath("hgs"),R"PY(#!/usr/bin/env python3
import json,pathlib,sys,time
r=pathlib.Path(__file__).parent;a=sys.argv[1:]
if a and a[0].startswith('@'):a.pop(0)
def arg(k):return a[a.index(k)+1]
with (r/'calls').open('a') as f:f.write(json.dumps(a)+'\n')
if a==['--help']:
 print('repository-clone-v1 guarded-create-v1' if (r/'identity').exists() else 'repository-clone-v1');sys.exit(0)
if a[:1]==['--expect-machine']:
 expected=a[1];a=a[2:]
 if not (r/'identity').exists() or (r/'identity').read_text()!=expected:sys.stderr.write('computer identity changed');sys.exit(2)
if a==['--help']:
 print('repository-clone-v1 guarded-create-v1');sys.exit(0)
if a[0]=='dirs':
 (r/'checking').touch()
 while (r/'hold-check').exists() and not (r/'release').exists():time.sleep(.01)
 if (r/'fail-check').exists():sys.stderr.write('folder unavailable');sys.exit(2)
 answer={'path':a[-1],'directories':[]}
 if '--creation' in a:
  answer.update(creation_identity_v1=True,computer_id=(r/'identity').read_text())
  if (r/'switch-identity').exists():(r/'identity').write_text('22222222-2222-4222-8222-222222222222')
 print(json.dumps(answer))
elif a[0]=='worktrees' and a[1]=='create':
 (r/'creating').touch()
 while (r/'hold-create').exists() and not (r/'release').exists():time.sleep(.01)
 if (r/'unknown-create').exists():sys.stderr.write('connection lost');sys.exit(2)
 print(json.dumps({'status':'created','request_id':arg('--request-id'),'path':arg('--destination'),'branch':arg('--branch'),'common_dir':arg('--common-dir')}))
elif a[:2]==['worktrees','status']:
 print(json.dumps({'schema':1,'status':'created','request_id':arg('--request-id'),'path':'/workspace/example-work-fixture','branch':'work-fixture','common_dir':'/workspace/example/.git'}))
elif a[0]=='repository':
 action=a[1]
 if action=='check':
  parent=arg('--parent');name=arg('--name')
  print(json.dumps({'status':'ready','parent':parent,'name':name,'path':parent+'/'+name}))
 elif action=='clone':
  parent=arg('--parent');value={'status':'cloned','request_id':arg('--request-id'),'parent':parent,'path':parent+'/'+arg('--name')}
  (r/'clone-receipt').write_text(json.dumps(value))
  if (r/'unknown-clone').exists():sys.stderr.write('connection lost');sys.exit(2)
  print(json.dumps(value))
 else:print((r/'clone-receipt').read_text())
elif a[0]=='worktrees':
 path=arg('--path')
 print(json.dumps({'state':'ok','stale':False,'path':path,'selected_root':path,'selected_branch':'work-fixture','common_dir':'/workspace/example/.git','creation_receipts_v1':not (r/'legacy-worktree').exists(),
 'worktrees':[] if '--creation' in a else [{'path':path,'available':True,'kind':'linked','branch':'work-fixture'}]}))
)PY"));
    }
    void init(){
        QVERIFY(QDir().mkpath(QFileInfo(QSettings().fileName()).absolutePath()));
        for(const auto &name:{"calls","checking","creating","hold-check","hold-create","release","fail-check","unknown-create","identity","switch-identity","clone-receipt","unknown-clone","legacy-worktree"})QFile::remove(m_fixture.filePath(name));
        QFile::remove(QSettings().fileName()+".creations.json");
    }
    void publishesBeforeAnySlowCheckAndRetainsFailure(){
        flag("hold-check");flag("fail-check");SessionCreationController controller(program());
        QSignalSpy changed(&controller,&SessionCreationController::changed),launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan());QCOMPARE(changed.size(),1);QCOMPARE(controller.operation(id)["state"].toString(),QString("saving"));QVERIFY(calls().isEmpty());
        QTRY_VERIFY(QFile::exists(m_fixture.filePath("checking")));QCOMPARE(launch.size(),0);
        flag("release");QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("failed"));
        QVERIFY(controller.operation(id)["error"].toString().contains("folder unavailable"));QVERIFY(controller.operation(id)["show_pending"].toBool());
        QFile::remove(m_fixture.filePath("fail-check"));controller.resume(id);QTRY_COMPARE(launch.size(),1);QCOMPARE(launch.first().first().toString(),id);
    }
    void admissionPaintDoesNotWaitForDurableStorage(){
        QSemaphore entered,release;std::atomic_bool once=false;SessionCreationController controller(program());
        const auto unblock=qScopeGuard([&]{release.release();});
        controller.setWriteObserver([&]{if(!once.exchange(true)){entered.release();release.acquire();}});
        const auto id=controller.submit(plan());QCOMPARE(controller.operation(id)["state"].toString(),QString("saving"));
        QTRY_VERIFY(entered.available()>0);QVERIFY(calls().isEmpty());release.release();
        QTRY_VERIFY(QFile::exists(m_fixture.filePath("checking")));
    }
    void stopDuringReadNeverCreatesCheckoutOrAgent(){
        flag("hold-check");SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan(true));QTRY_VERIFY(QFile::exists(m_fixture.filePath("checking")));
        controller.stop(id);flag("release");QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("stopped"));
        QCOMPARE(launch.size(),0);for(const auto &line:calls())QVERIFY(!line.contains("create"));
    }
    void hideDuringDurableBarrierDoesNotStrandPreparation(){
        QSemaphore entered,release;std::atomic_bool once=false;SessionCreationController controller(program());
        const auto unblock=qScopeGuard([&]{release.release();});
        controller.setWriteObserver([&]{if(!once.exchange(true)){entered.release();release.acquire();}});
        QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan());QTRY_VERIFY(entered.available()>0);
        controller.hide(id);QVERIFY(!controller.operation(id)["show_pending"].toBool());release.release();
        QTRY_COMPARE(launch.size(),1);QVERIFY(!controller.operation(id)["show_pending"].toBool());
    }
    void stopBeforeFirstEventNeverDispatches(){
        SessionCreationController controller(program());const auto id=controller.submit(plan(true));controller.stop(id);
        QTest::qWait(100);QCOMPARE(controller.operation(id)["state"].toString(),QString("stopped"));QVERIFY(calls().isEmpty());
    }
    void restartRecoversOriginalWorktreeWithoutReplay(){
        QString id,token;{
            flag("unknown-create");SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
            id=controller.submit(plan(true));QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("unknown"));
            token=controller.operation(id)["worktree_id"].toString();QVERIFY(!token.isEmpty());QCOMPARE(launch.size(),0);
        }
        SessionCreationController restored(program());QSignalSpy launch(&restored,&SessionCreationController::launchRequested);
        QTRY_VERIFY(restored.owns(id));QCOMPARE(restored.operation(id)["state"].toString(),QString("recover"));
        restored.resume(id);QTest::qWait(50);QCOMPARE(launch.size(),0);
        restored.check(id);QTRY_VERIFY(restored.operation(id)["worktree_done"].toBool());
        QCOMPARE(restored.operation(id)["worktree_id"].toString(),token);QCOMPARE(launch.size(),0);
        restored.resume(id);QTRY_COMPARE(launch.size(),1);
        int creates=0;for(const auto &line:calls())if(line.contains("\"create\""))++creates;QCOMPARE(creates,1);
    }
    void provenSessionWinsOverLateLaunchFailure(){
        SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan());QTRY_COMPARE(launch.size(),1);
        controller.complete(id);controller.launchFinished(id,false,"late transport failure");controller.ended(id,id,"late ended evidence");
        QCOMPARE(controller.operation(id)["state"].toString(),QString("bound"));
    }
    void endedStartupCanRetryWithoutConsumingStaleEvidence(){
        SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan());QTRY_COMPARE(launch.size(),1);
        controller.ended(id,id,"exit code 3");QCOMPARE(controller.operation(id)["state"].toString(),QString("failed"));
        const auto successor=controller.operation(id)["launch_id"].toString();QVERIFY(successor!=id);
        controller.launchFinished(id,true,{});QCOMPARE(controller.operation(id)["state"].toString(),QString("failed"));
        controller.resume(id);QTRY_COMPARE(launch.size(),2);
        controller.ended(id,id,"stale exit code 3");QCOMPARE(controller.operation(id)["state"].toString(),QString("launching"));
        QCOMPARE(controller.operation(id)["launch_id"].toString(),successor);
    }
    void legacyUnknownCancellationCannotClaimStoppedOrOfferReplay(){
        flag("legacy-worktree");flag("unknown-create");SessionCreationController controller(program());
        const auto id=controller.submit(plan(true));QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("unknown"));
        controller.stop(id);QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("unknown"));
        QVERIFY(controller.operation(id)["stop_requested"].toBool());QVERIFY(!SessionCreationController::canResume(controller.operation(id)));QVERIFY(!controller.canEdit(id));
        int creates=0;for(const auto &line:calls())if(line.contains("\"create\""))++creates;QCOMPARE(creates,1);
    }
    void cloneRecoveryRetainsOriginalIdentityAndRequiresContinue(){
        flag("unknown-clone");QString id,token;
        auto p=plan();p["repository"]=true;p["url"]="git@example.test:team/example.git";p["parent"]="/workspace";p["repository_name"]="example";
        {
            SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
            id=controller.submit(p);QVERIFY2(!id.isEmpty(),qPrintable(controller.admissionError()));
            QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("unknown"));
            token=controller.operation(id)["clone_id"].toString();QVERIFY(!token.isEmpty());QCOMPARE(launch.size(),0);
        }
        SessionCreationController restored(program());QSignalSpy launch(&restored,&SessionCreationController::launchRequested);
        int acquisitions=0;restored.resolvePlacement=[&](QJsonObject &,const QJsonObject &result,QString &){if(result["cloned"].toBool())++acquisitions;return true;};
        QTRY_VERIFY(restored.owns(id));restored.resume(id);QTest::qWait(50);QCOMPARE(launch.size(),0);
        restored.check(id);QTRY_VERIFY(restored.operation(id)["clone_done"].toBool());QCOMPARE(acquisitions,1);
        QCOMPARE(restored.operation(id)["clone_id"].toString(),token);QCOMPARE(launch.size(),0);
        restored.resume(id);QTRY_COMPARE(launch.size(),1);
        int clones=0;for(const auto &line:calls())if(line.contains("\"clone\""))++clones;QCOMPARE(clones,1);
    }
    void computerChangeBeforeCheckoutRefusesMutation(){
        const auto original=QStringLiteral("11111111-1111-4111-8111-111111111111");QFile identity(m_fixture.filePath("identity"));
        QVERIFY(identity.open(QIODevice::WriteOnly));identity.write(original.toUtf8());identity.close();flag("switch-identity");
        SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan(true));QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("failed"));
        QCOMPARE(controller.operation(id)["expected_machine"].toString(),original);QCOMPARE(launch.size(),0);
        for(const auto &line:calls())QVERIFY(!line.contains("\"create\""));
    }
    void modernCheckoutRevalidatesReceiptAndCarriesComputerIdentityToLaunch(){
        const auto computer=QStringLiteral("11111111-1111-4111-8111-111111111111");
        QFile identity(m_fixture.filePath("identity"));QVERIFY(identity.open(QIODevice::WriteOnly));identity.write(computer.toUtf8());identity.close();
        SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan(true));QTRY_COMPARE(launch.size(),1);
        const auto value=launch[0][1].toJsonObject();QCOMPARE(value["expected_machine"].toString(),computer);
        QCOMPARE(value["target"].toString(),QString("/workspace/example-work-fixture"));QVERIFY(value["worktree_done"].toBool());
        int creates=0,verifications=0;for(const auto &line:calls()){
            if(line.contains("\"create\"")){++creates;QVERIFY(line.contains("--expect-machine"));QVERIFY(line.contains("--receipt"));QVERIFY(line.contains(value["worktree_id"].toString()));}
            if(line.contains("\"status\""))++verifications;
        }
        QCOMPARE(creates,1);QCOMPARE(verifications,1);QVERIFY(!id.isEmpty());
    }
    void queueAdmissionHasAFixedBound(){
        SessionCreationController controller(program());
        for(int i=0;i<32;++i){auto p=plan();p["tag"]=QString("queued-%1").arg(i);QVERIFY(!controller.submit(p).isEmpty());}
        QVERIFY(controller.submit(plan()).isEmpty());QCOMPARE(controller.operations().size(),32);QVERIFY(!controller.admissionError().isEmpty());QVERIFY(calls().isEmpty());
    }
    void credentialAddressIsRejectedBeforeJournalReservation(){
        SessionCreationController controller(program());auto p=plan();p["repository"]=true;p["url"]="https://token@example.test/repository.git";
        QVERIFY(controller.submit(p).isEmpty());QVERIFY(controller.operations().isEmpty());QVERIFY(calls().isEmpty());
        QTest::qWait(50);QVERIFY(!QFile::exists(QSettings().fileName()+".creations.json"));
    }
    void malformedDeliveryFlagCannotReplayAfterRestart(){
        auto p=plan();const auto id=QUuid::createUuid().toString(QUuid::WithoutBraces);
        p["id"]=id;p["launch_id"]=id;p["creation_schema"]=1;p["state"]="confirming";p["created"]=QDateTime::currentSecsSinceEpoch();p["launch_dispatched"]="true";
        QFile file(QSettings().fileName()+".creations.json");QVERIFY(file.open(QIODevice::WriteOnly));
        const auto original=QJsonDocument(QJsonObject{{"schema",1},{"operations",QJsonObject{{id,p}}}}).toJson(QJsonDocument::Compact);file.write(original);file.close();
        SessionCreationController controller(program());const auto fresh=controller.submit(plan());
        QTRY_COMPARE(controller.operation(fresh)["state"].toString(),QString("storage_error"));QVERIFY(calls().isEmpty());
        QVERIFY(file.open(QIODevice::ReadOnly));QCOMPARE(file.readAll(),original);
    }
    void corruptJournalCannotBeOverwrittenOrDispatch(){
        QFile file(QSettings().fileName()+".creations.json");QVERIFY(file.open(QIODevice::WriteOnly));file.write("{broken");file.close();
        SessionCreationController controller(program());QSignalSpy launch(&controller,&SessionCreationController::launchRequested);
        const auto id=controller.submit(plan());QTRY_COMPARE(controller.operation(id)["state"].toString(),QString("storage_error"));
        QCOMPARE(launch.size(),0);QVERIFY(calls().isEmpty());QVERIFY(file.open(QIODevice::ReadOnly));QCOMPARE(file.readAll(),QByteArray("{broken"));
    }
};
QTEST_GUILESS_MAIN(TestSessionCreation)
#include "test_sessioncreation.moc"
