#include <QtTest>
#include <QJsonArray>
#include <QSignalSpy>
#include <QTemporaryDir>
#include "FakeCli.h"
#include "WorkspaceSettings.h"

class TestWorkspaceSettings : public QObject {
    Q_OBJECT
    static QJsonObject active(bool running=false,const QString &revision=QString(64,'a')){
        return {{"state","active"},{"encrypted",true},{"config_revision",revision},{"server_url","https://relay.example.test"},{"workspace_id","workspace-fixture"},{"node_id","computer-fixture"},{"workspace_name","Synthetic workspace"},{"computer_name","Synthetic computer"},{"connector_state",running?"running":"stopped"},{"running",running},{"generation",running?QJsonValue(QString(32,'b')):QJsonValue(QJsonValue::Null)}};
    }
    static QJsonObject unconfigured(){return {{"state","unconfigured"},{"encrypted",false},{"config_revision",""}};}
    static bool reply(const QString &directory,const QJsonObject &responses){return FakeCli::replace(directory+"/responses",QJsonDocument(responses).toJson());}
    static QJsonObject legacyStatus(){return {{"state","legacy"},{"encrypted",false},{"config_revision",QString(64,'a')},{"server_url","https://relay.example.test"}};}
    static QJsonObject draining(){return {{"state","migration-draining"},{"encrypted",false},{"node_id","candidate-fixture"},{"config_revision",QString(64,'a')},{"migration",QJsonObject{{"phase","draining"},{"reason","draining"},{"retryable",true}}}};}
    static QJsonObject savedDraining(){
        auto value=legacyStatus();value.insert("pending",QJsonObject{{"action","switch-join"},{"node_id","candidate-fixture"},{"state","migration-draining"}});
        value.insert("migration",draining().value("migration"));return value;
    }
    static QJsonObject step(const QString &action,const QJsonObject &result){return {{"action",action},{"result",result}};}
    static bool sequence(const QString &directory,const QJsonArray &steps){return reply(directory,{{"sequence",steps}});}
    static bool observeAutomaticCommandWindow(){
        QTimer boundary;boundary.setSingleShot(true);QSignalSpy elapsed(&boundary,&QTimer::timeout);
        boundary.start(1250);return elapsed.wait(3000);
    }
    static bool install(const QString &script){return FakeCli::install(script,R"PY(#!/usr/bin/env python3
import json,pathlib,sys
root=pathlib.Path(__file__).parent
payload=json.load(sys.stdin)
action=sys.argv[2]
with (root/'calls').open('a+') as out:
    out.seek(0)
    index=sum(1 for _ in out)
    out.write(json.dumps({'argv':sys.argv[1:],'payload':payload})+'\n')
responses=json.loads((root/'responses').read_text())
if 'sequence' in responses:
    steps=responses['sequence']
    result=steps[index]['result'] if index<len(steps) and steps[index]['action']==action else {'ok':False,'error':'Unexpected synthetic fixture action'}
else:
    result=responses.get(action,responses.get('status',{}))
print(json.dumps(result))
sys.exit(1 if result.get('ok') is False else 0)
)PY");}
    static QList<QJsonObject> calls(const QString &directory){
        QFile file(directory+"/calls");if(!file.open(QIODevice::ReadOnly))return {};
        QList<QJsonObject> result;for(const auto &line:file.readAll().split('\n'))if(!line.isEmpty())result.append(QJsonDocument::fromJson(line).object());return result;
    }
private slots:
    void confirmedWorkspaceChangesInvalidateThePhoneList() {
        QTemporaryDir directory; QVERIFY(directory.isValid());
        const auto fixture=directory.path()+"/connector"; QVERIFY(install(fixture));
        QVERIFY(reply(directory.path(),{{"status",unconfigured()}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *refresh=panel.findChild<QPushButton*>("refreshWorkspaceStatus"); QTRY_VERIFY(refresh->isEnabled());
        int changes=0; panel.connectionChanged=[&]{++changes;};
        QVERIFY(reply(directory.path(),{{"status",active()}})); refresh->click();
        QTRY_COMPARE(changes,1); QTRY_VERIFY(refresh->isEnabled());
        refresh->click(); QTRY_VERIFY(refresh->isEnabled()); QCOMPARE(changes,1);
        QVERIFY(reply(directory.path(),{{"status",active(false,QString(64,'c'))}})); refresh->click();
        QTRY_COMPARE(changes,2); QTRY_VERIFY(refresh->isEnabled());
        for(const auto &call:calls(directory.path())) QCOMPARE(call.value("argv").toArray().at(1).toString(),QString("status"));
    }
    void openingIsReadOnlyAndJoinSecretsUseOnlyStdin(){
        QTemporaryDir directory;QVERIFY(directory.isValid());const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        QVERIFY(reply(directory.path(),{{"status",unconfigured()}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/private/connector.json");
        auto *submit=panel.findChild<QPushButton*>("submitWorkspaceEnrollment");QTRY_VERIFY(submit->isEnabled());
        QCOMPARE(calls(directory.path()).size(),1);QCOMPARE(calls(directory.path()).first().value("argv").toArray().at(1).toString(),QString("status"));
        QVERIFY(!QFileInfo::exists(directory.path()+"/private"));
        panel.findChild<QLineEdit*>("workspaceRelayUrl")->setText("https://relay.example.test");
        panel.findChild<QComboBox*>("workspaceEnrollmentMode")->setCurrentIndex(1);
        panel.findChild<QLineEdit*>("joinComputerWorkspaceCode")->setText("ze1.synthetic-secret-code");
        auto waiting=unconfigured();waiting.insert("pending",QJsonObject{{"action","join"},{"state","pending"}});
        QVERIFY(reply(directory.path(),{{"status",waiting},{"join",QJsonObject{{"state","pending"},{"node_id","candidate-fixture"},{"config_revision",""}}}}));
        submit->click();auto *check=panel.findChild<QPushButton*>("checkWorkspaceEnrollment");QTRY_VERIFY(check->isEnabled());
        const auto log=calls(directory.path());QVERIFY(log.size()>=3);const auto sent=log.at(1);QCOMPARE(sent.value("argv").toArray().at(1).toString(),QString("join"));
        QVERIFY(!QJsonDocument(sent.value("argv").toArray()).toJson().contains("synthetic-secret-code"));
        QCOMPARE(sent.value("payload").toObject().value("code").toString(),QString("ze1.synthetic-secret-code"));
        QCOMPARE(sent.value("payload").toObject().value("expected_revision").toString(),QString());
        QVERIFY(reply(directory.path(),{{"status",active()},{"check",active()}}));check->click();QTRY_VERIFY(panel.findChild<QPushButton*>("giveComputerWorkspaceCode")->isEnabled());
        QVERIFY(panel.findChild<QLineEdit*>("joinComputerWorkspaceCode")->text().isEmpty());
    }
    void computerInvitationRequiresExplicitCopyAndCanBeCanceled(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        const auto code=QStringLiteral("ze1.synthetic-computer-invitation");
        QVERIFY(reply(directory.path(),{{"status",active()},{"invite-computer",QJsonObject{{"code",code},{"server_url","https://relay.example.test"},{"purpose","computer"},{"invitation_id","invitation-fixture"},{"expires_at",double(QDateTime::currentSecsSinceEpoch()+600)}}},{"cancel-invite",QJsonObject{{"cancelled",true}}}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");auto *give=panel.findChild<QPushButton*>("giveComputerWorkspaceCode");QTRY_VERIFY(give->isEnabled());
        QApplication::clipboard()->setText("synthetic clipboard");give->click();auto *copy=panel.findChild<QPushButton*>("copyComputerWorkspaceCode");QTRY_VERIFY(copy->isEnabled());QCOMPARE(QApplication::clipboard()->text(),QString("synthetic clipboard"));
        copy->click();QCOMPARE(QApplication::clipboard()->text(),code);
        panel.findChild<QPushButton*>("cancelComputerWorkspaceCode")->click();QTRY_VERIFY(!copy->isEnabled());
        const auto log=calls(directory.path());for(const auto &call:log)QVERIFY(!QJsonDocument(call).toJson().contains(code.toUtf8()));
    }
    void failedActivationRecoversSavedEnrollmentWithoutRepeatingCreate(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        QJsonObject legacy{{"state","legacy"},{"encrypted",false},{"config_revision",QString(64,'a')},{"server_url","https://relay.example.test"}};
        QVERIFY(reply(directory.path(),{{"status",legacy}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        legacy.insert("pending",QJsonObject{{"action","switch-create"},{"state","ready"}});
        QVERIFY(reply(directory.path(),{{"status",legacy},{"switch-create",QJsonObject{{"ok",false},{"error","Saved connection is waiting for the current request to finish."}}},{"check",active()}}));
        panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->click();
        auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        auto *check=panel.findChild<QPushButton*>("checkWorkspaceEnrollment");QTRY_VERIFY(check->isVisibleTo(&panel)&&check->isEnabled());
        const auto log=calls(directory.path());QCOMPARE(log.size(),3);
        QCOMPARE(log.at(1).value("argv").toArray().at(1).toString(),QString("switch-create"));
        QCOMPARE(log.at(2).value("argv").toArray().at(1).toString(),QString("status"));
        QVERIFY(panel.findChild<QLabel*>("workspaceNotice")->text().contains("Saved connection"));
        QVERIFY(reply(directory.path(),{{"status",active()},{"check",active()}}));
        check->click();QTRY_VERIFY(panel.findChild<QPushButton*>("startWorkspaceConnector")->isVisibleTo(&panel));
        QVERIFY(panel.findChild<QWidget*>("workspaceEnrollment")->isHidden());
        QVERIFY(change->isVisibleTo(&panel));
        const auto resumed=calls(directory.path());QCOMPARE(resumed.at(3).value("argv").toArray().at(1).toString(),QString("check"));
        QCOMPARE(resumed.at(3).value("payload").toObject().value("expected_revision").toString(),QString(64,'a'));
    }
    void lostCreateReplyUsesCommittedStatusWithoutRepeatingMutation(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        const QJsonObject legacy{{"state","legacy"},{"encrypted",false},{"config_revision",QString(64,'a')}};
        QVERIFY(reply(directory.path(),{{"status",legacy}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        panel.findChild<QLineEdit*>("workspaceRelayUrl")->setText("https://relay.example.test");
        QVERIFY(reply(directory.path(),{{"status",active(false,QString(64,'c'))},{"switch-create",QJsonObject{{"ok",false},{"error","The operation result could not be confirmed."}}}}));
        panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->click();
        auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        QTRY_VERIFY(change->isVisibleTo(&panel)&&change->isEnabled());
        QVERIFY(panel.findChild<QWidget*>("workspaceEnrollment")->isHidden());
        const auto log=calls(directory.path());QCOMPARE(log.size(),3);
        QCOMPARE(log.at(1).value("argv").toArray().at(1).toString(),QString("switch-create"));
        QCOMPARE(log.at(2).value("argv").toArray().at(1).toString(),QString("status"));
    }
    void failedJoinClearsConsumedCodeAfterSavedPendingIsConfirmed(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        QJsonObject legacy{{"state","legacy"},{"encrypted",false},{"config_revision",QString(64,'a')}};
        QVERIFY(reply(directory.path(),{{"status",legacy}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        panel.findChild<QLineEdit*>("workspaceRelayUrl")->setText("https://relay.example.test");
        panel.findChild<QComboBox*>("workspaceEnrollmentMode")->setCurrentIndex(1);
        auto *code=panel.findChild<QLineEdit*>("joinComputerWorkspaceCode");code->setText("ze1.synthetic-consumed-invitation");
        legacy.insert("pending",QJsonObject{{"action","switch-join"},{"state","ready"}});
        QVERIFY(reply(directory.path(),{{"status",legacy},{"switch-join",QJsonObject{{"ok",false},{"error","The saved connection needs to finish switching."}}}}));
        panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->click();
        auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        QTRY_VERIFY(panel.findChild<QPushButton*>("checkWorkspaceEnrollment")->isEnabled());
        QVERIFY(code->text().isEmpty());QVERIFY(panel.findChild<QWidget*>("workspaceEnrollment")->isHidden());
        QCOMPARE(calls(directory.path()).size(),3);
    }
    void ordinaryStatusRefreshPreservesUnsubmittedJoinForm(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        QVERIFY(reply(directory.path(),{{"status",active()}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        panel.findChild<QComboBox*>("workspaceEnrollmentMode")->setCurrentIndex(1);
        auto *code=panel.findChild<QLineEdit*>("joinComputerWorkspaceCode");code->setText("ze1.synthetic-unsubmitted-invitation");
        auto *reload=panel.findChild<QPushButton*>("refreshWorkspaceStatus");reload->click();QTRY_VERIFY(reload->isEnabled());
        QCOMPARE(code->text(),QString("ze1.synthetic-unsubmitted-invitation"));
        QVERIFY(panel.findChild<QWidget*>("workspaceEnrollment")->isVisibleTo(&panel));QVERIFY(change->isHidden());
        const auto log=calls(directory.path());QCOMPARE(log.size(),2);
        for(const auto &call:log)QCOMPARE(call.value("argv").toArray().at(1).toString(),QString("status"));
    }
    void openingSavedMigrationDoesNotAuthorizeHandover(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        QVERIFY(reply(directory.path(),{{"status",savedDraining()}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *check=panel.findChild<QPushButton*>("checkWorkspaceEnrollment");QTRY_VERIFY(check->isVisibleTo(&panel)&&check->isEnabled());
        QCOMPARE(check->text(),QString("Continue connecting"));
        QVERIFY(observeAutomaticCommandWindow());QVERIFY(check->isEnabled());
        const auto log=calls(directory.path());QCOMPARE(log.size(),1);
        QCOMPARE(log.first().value("argv").toArray().at(1).toString(),QString("status"));
        QVERIFY(log.first().value("payload").toObject().isEmpty());
    }
    void confirmedLegacySwitchContinuesWithSavedCheckOnly(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        const auto connected=active(false,QString(64,'c'));
        QVERIFY(sequence(directory.path(),{step("status",legacyStatus()),step("switch-join",draining()),step("status",savedDraining()),step("check",connected),step("status",connected)}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        panel.findChild<QComboBox*>("workspaceEnrollmentMode")->setCurrentIndex(1);
        auto *code=panel.findChild<QLineEdit*>("joinComputerWorkspaceCode");code->setText("ze1.synthetic-handover-invitation");
        panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->click();
        auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        QTRY_VERIFY(change->isVisibleTo(&panel)&&change->isEnabled());
        QVERIFY(panel.findChild<QPushButton*>("startWorkspaceConnector")->isVisibleTo(&panel));
        QVERIFY(panel.findChild<QWidget*>("workspaceEnrollment")->isHidden());QVERIFY(code->text().isEmpty());
        const auto log=calls(directory.path());QCOMPARE(log.size(),5);
        const QStringList expected{"status","switch-join","status","check","status"};
        for(int index=0;index<log.size();++index)QCOMPARE(log.at(index).value("argv").toArray().at(1).toString(),expected.at(index));
        for(int index:{1,3}){
            const auto payload=log.at(index).value("payload").toObject();
            QCOMPARE(payload.value("expected_revision").toString(),QString(64,'a'));
            QCOMPARE(payload.value("allow_legacy_service_handover"),QJsonValue(true));
        }
        QCOMPARE(log.at(1).value("payload").toObject().value("code").toString(),QString("ze1.synthetic-handover-invitation"));
        QVERIFY(!log.at(3).value("payload").toObject().contains("code"));
        QVERIFY(!log.at(3).value("payload").toObject().contains("allow_unconfirmed_legacy_retirement"));
        for(int index:{0,2,4})QVERIFY(log.at(index).value("payload").toObject().isEmpty());
    }
    void invalidMigrationStatusStopsContinuation_data(){
        QTest::addColumn<bool>("missing");
        QTest::newRow("missing-migration")<<true;
        QTest::newRow("non-boolean-retryable")<<false;
    }
    void invalidMigrationStatusStopsContinuation(){
        QFETCH(bool,missing);
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        auto invalid=savedDraining();
        if(missing)invalid.remove("migration");
        else{auto metadata=invalid.value("migration").toObject();metadata.insert("retryable","true");invalid.insert("migration",metadata);}
        QVERIFY(sequence(directory.path(),{step("status",legacyStatus()),step("switch-create",draining()),step("status",invalid)}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");
        auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->click();
        auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        QTRY_VERIFY(panel.findChild<QLabel*>("workspaceSummary")->text().contains("unconfirmed"));
        auto *reload=panel.findChild<QPushButton*>("refreshWorkspaceStatus");QTRY_VERIFY(reload->isEnabled());
        QVERIFY(observeAutomaticCommandWindow());QVERIFY(reload->isEnabled());
        QVERIFY(!panel.findChild<QPushButton*>("checkWorkspaceEnrollment")->isEnabled());
        QVERIFY(!panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->isEnabled());
        const auto log=calls(directory.path());QCOMPARE(log.size(),3);
        QCOMPARE(log.at(1).value("argv").toArray().at(1).toString(),QString("switch-create"));
        QCOMPARE(log.at(1).value("payload").toObject().value("allow_legacy_service_handover"),QJsonValue(true));
        QCOMPARE(log.at(2).value("argv").toArray().at(1).toString(),QString("status"));
        QVERIFY(log.at(2).value("payload").toObject().isEmpty());
    }
    void malformedStatusCannotUnlockMutations(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));QVERIFY(reply(directory.path(),{{"status",QJsonObject{}}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");auto *reload=panel.findChild<QPushButton*>("refreshWorkspaceStatus");auto *submit=panel.findChild<QPushButton*>("submitWorkspaceEnrollment");
        QTRY_VERIFY(reload->isEnabled());QVERIFY(!submit->isEnabled());
        auto malformed=active();malformed.insert("encrypted","true");QVERIFY(reply(directory.path(),{{"status",malformed}}));reload->click();QTRY_VERIFY(reload->isEnabled());QVERIFY(!submit->isEnabled());
        QVERIFY(reply(directory.path(),{{"status",unconfigured()}}));reload->click();QTRY_VERIFY(submit->isEnabled());
    }
    void switchConfirmationCancelsWithoutMutationAndStopPrecedesChange(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));QVERIFY(reply(directory.path(),{{"status",active(true)}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());change->click();
        auto *submit=panel.findChild<QPushButton*>("submitWorkspaceEnrollment");submit->click();auto *dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Cancel)->click();QCoreApplication::processEvents();QCOMPARE(calls(directory.path()).size(),1);
        const auto changed=active(false,QString(64,'c'));
        QVERIFY(reply(directory.path(),{{"status",changed},{"stop",QJsonObject{{"state","stopped"},{"running",false},{"generation",QJsonValue::Null}}},{"switch-create",changed}}));
        submit->click();dialog=panel.findChild<QMessageBox*>("confirmWorkspaceChange");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();
        QTRY_VERIFY(calls(directory.path()).size()>=4);const auto log=calls(directory.path());QCOMPARE(log.at(1).value("argv").toArray().at(1).toString(),QString("stop"));QCOMPARE(log.at(2).value("argv").toArray().at(1).toString(),QString("switch-create"));
        QCOMPARE(log.at(2).value("payload").toObject().value("expected_revision").toString(),QString(64,'a'));
    }
    void unconfirmedLegacyDisconnectRequiresSeparateConfirmation(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));
        const QJsonObject legacy{{"state","legacy"},{"encrypted",false},{"config_revision",QString(64,'a')},{"pending",QJsonObject{{"action","switch-create"},{"state","legacy-retirement-unconfirmed"}}}};
        QVERIFY(reply(directory.path(),{{"status",legacy},{"check",active()}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");auto *check=panel.findChild<QPushButton*>("checkWorkspaceEnrollment");QTRY_VERIFY(check->isEnabled());
        check->click();auto *dialog=panel.findChild<QMessageBox*>("confirmLegacyLocalDisconnect");QVERIFY(dialog);QVERIFY(dialog->text().contains("may remain active"));dialog->button(QMessageBox::Cancel)->click();QCoreApplication::processEvents();QCOMPARE(calls(directory.path()).size(),1);
        check->click();dialog=panel.findChild<QMessageBox*>("confirmLegacyLocalDisconnect");QVERIFY(dialog);dialog->button(QMessageBox::Yes)->click();QTRY_VERIFY(calls(directory.path()).size()>=3);
        const auto sent=calls(directory.path()).at(1);QCOMPARE(sent.value("argv").toArray().at(1).toString(),QString("check"));QCOMPARE(sent.value("payload").toObject().value("allow_unconfirmed_legacy_retirement"),QJsonValue(true));
        QCOMPARE(sent.value("payload").toObject().value("expected_revision").toString(),QString(64,'a'));
    }
    void removedComputerCanChooseNewWorkspaceButCannotStartOldConnector(){
        QTemporaryDir directory;const auto fixture=directory.path()+"/connector";QVERIFY(install(fixture));auto retired=active();retired.insert("state","retired");QVERIFY(reply(directory.path(),{{"status",retired}}));
        WorkspaceSettings::Panel panel(FakeCli::program(fixture),directory.path()+"/connector.json");auto *change=panel.findChild<QPushButton*>("changeMobileWorkspace");QTRY_VERIFY(change->isEnabled());
        QVERIFY(panel.findChild<QPushButton*>("startWorkspaceConnector")->isHidden());QVERIFY(panel.findChild<QLabel*>("workspaceSummary")->text().contains("removed"));change->click();QVERIFY(panel.findChild<QPushButton*>("submitWorkspaceEnrollment")->isEnabled());
        QCOMPARE(panel.findChild<QLineEdit*>("workspaceRelayUrl")->text(),QString("https://relay.example.test"));
        auto draining=active(true);draining.insert("state","retired");QVERIFY(reply(directory.path(),{{"status",draining}}));panel.findChild<QPushButton*>("refreshWorkspaceStatus")->click();auto *stop=panel.findChild<QPushButton*>("stopWorkspaceConnector");QTRY_VERIFY(stop->isVisibleTo(&panel)&&stop->isEnabled());QVERIFY(panel.findChild<QPushButton*>("startWorkspaceConnector")->isHidden());
        QVERIFY(reply(directory.path(),{{"status",retired},{"stop",QJsonObject{{"state","stopped"},{"running",false},{"generation",QJsonValue(QJsonValue::Null)}}}}));stop->click();QTRY_VERIFY(calls(directory.path()).size()>=4);const auto sent=calls(directory.path()).at(2);QCOMPARE(sent.value("argv").toArray().at(1).toString(),QString("stop"));QCOMPARE(sent.value("payload").toObject().value("generation").toString(),QString(32,'b'));
    }
};
QTEST_MAIN(TestWorkspaceSettings)
#include "test_workspacesettings.moc"
