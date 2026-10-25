#pragma once
#include "ChildProcess.h"
#include "RelayConfiguration.h"
#include <QApplication>
#include <QClipboard>
#include <QComboBox>
#include <QCoreApplication>
#include <QDateTime>
#include <QElapsedTimer>
#include <QFormLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QProcess>
#include <QProcessEnvironment>
#include <QPushButton>
#include <QStandardPaths>
#include <QSysInfo>
#include <QTimer>
#include <QVBoxLayout>
#include <functional>
#include <memory>

namespace WorkspaceSettings {
inline QString connectorProgram(const QString &hgs={}) {
#ifdef Q_OS_WIN
    const auto name=QStringLiteral("zerus-mobile-connector.exe");
#else
    const auto name=QStringLiteral("zerus-mobile-connector");
#endif
    if(QDir::isAbsolutePath(hgs)) {
        const auto beside=QFileInfo(hgs).dir().absoluteFilePath(name);
        if(QFileInfo(beside).isExecutable())return beside;
    }
    const auto bundled=QCoreApplication::applicationDirPath()+"/"+name;
    if(QFileInfo(bundled).isExecutable())return bundled;
    const auto found=QStandardPaths::findExecutable(name);
    return found.isEmpty()?bundled:found;
}

// The connector owns private persistence and cryptographic authorization.
// Opening this panel issues only a bounded read-only local status command.
class Panel : public QWidget {
public:
    std::function<void(bool)> protectionChanged;
    std::function<void()> connectionChanged;
    explicit Panel(const QString &hgs,QWidget *parent=nullptr)
        :Panel(connectorProgram(hgs),RelaySettings::configPath(),parent){}
    Panel(QString connector,QString configuration,QWidget *parent=nullptr)
        :QWidget(parent),program(std::move(connector)),path(std::move(configuration)) {
        setObjectName("workspaceSettings");
        auto *layout=new QVBoxLayout(this);layout->setContentsMargins(0,0,12,0);layout->setSpacing(14);
        auto label=[&](const QString &text,const QString &name={}) {
            auto *item=new QLabel(text);item->setWordWrap(true);item->setTextFormat(Qt::PlainText);
            if(!name.isEmpty())item->setObjectName(name);layout->addWidget(item);return item;
        };
        label(tr("Workspace"),"heading");
        label(tr("Connect your computers and phones to one workspace. Session content is encrypted between your devices; the relay keeps routing and delivery metadata."));
        summary=label(tr("Workspace: checking…"),"workspaceSummary");
        connection=label({},"workspaceConnectorStatus");
        auto *actions=new QHBoxLayout;
        change=new QPushButton(tr("Change workspace…"));change->setObjectName("changeMobileWorkspace");actions->addWidget(change);
        start=new QPushButton(tr("Start connector"));start->setObjectName("startWorkspaceConnector");actions->addWidget(start);
        stop=new QPushButton(tr("Stop connector"));stop->setObjectName("stopWorkspaceConnector");actions->addWidget(stop);
        reload=new QPushButton(tr("Refresh status"));reload->setObjectName("refreshWorkspaceStatus");actions->addWidget(reload);actions->addStretch();layout->addLayout(actions);
        enrollment=new QWidget;enrollment->setObjectName("workspaceEnrollment");
        auto *enrollLayout=new QVBoxLayout(enrollment);enrollLayout->setContentsMargins(0,0,0,0);enrollLayout->setSpacing(12);
        auto *form=new QFormLayout;form->setFieldGrowthPolicy(QFormLayout::ExpandingFieldsGrow);
        mode=new QComboBox;mode->setObjectName("workspaceEnrollmentMode");mode->addItem(tr("Create workspace"),"create");mode->addItem(tr("Join by code"),"join");form->addRow(tr("Connection"),mode);
        url=new QLineEdit(RelaySettings::canonicalRelay());url->setObjectName("workspaceRelayUrl");form->addRow(tr("Relay (HTTPS)"),url);
        code=new QLineEdit;code->setObjectName("joinComputerWorkspaceCode");code->setMaxLength(4096);code->setEchoMode(QLineEdit::PasswordEchoOnEdit);
        codeLabel=new QLabel(tr("Computer invitation code"));form->addRow(codeLabel,code);enrollLayout->addLayout(form);
        submit=new QPushButton(tr("Create workspace"));submit->setObjectName("submitWorkspaceEnrollment");enrollLayout->addWidget(submit,0,Qt::AlignLeft);layout->addWidget(enrollment);
        pending=new QLabel;pending->setObjectName("pendingWorkspaceEnrollment");pending->setWordWrap(true);pending->setTextFormat(Qt::PlainText);layout->addWidget(pending);
        check=new QPushButton(tr("Check connection"));check->setObjectName("checkWorkspaceEnrollment");layout->addWidget(check,0,Qt::AlignLeft);
        invitationRow=new QWidget;auto *invites=new QHBoxLayout(invitationRow);invites->setContentsMargins(0,0,0,0);
        giveCode=new QPushButton(tr("Give computer code"));giveCode->setObjectName("giveComputerWorkspaceCode");invites->addWidget(giveCode);
        copyCode=new QPushButton(tr("Copy computer code"));copyCode->setObjectName("copyComputerWorkspaceCode");invites->addWidget(copyCode);
        cancelCode=new QPushButton(tr("Cancel code"));cancelCode->setObjectName("cancelComputerWorkspaceCode");invites->addWidget(cancelCode);invites->addStretch();layout->addWidget(invitationRow);
        codeStatus=label({},"computerWorkspaceCodeStatus");notice=label({},"workspaceNotice");
        connect(mode,&QComboBox::currentIndexChanged,this,[this]{updateForm();});
        connect(change,&QPushButton::clicked,this,[this]{recoveringEnrollment=false;changing=true;enrollment->show();change->hide();updateForm();url->setFocus();});
        connect(reload,&QPushButton::clicked,this,[this]{refresh();});
        connect(check,&QPushButton::clicked,this,[this]{checkEnrollment();});
        connect(start,&QPushButton::clicked,this,[this]{request("start",{},[this](const QJsonObject &){refresh();});});
        connect(stop,&QPushButton::clicked,this,[this]{request("stop",{{"generation",snapshot.value("generation")}},[this](const QJsonObject &result){notice->setText(result.value("state")=="stopping"?tr("The connector is draining its current request. Refresh status to confirm it has stopped."):tr("Connector stopped."));refresh();});});
        connect(submit,&QPushButton::clicked,this,[this]{enroll();});
        connect(giveCode,&QPushButton::clicked,this,[this]{request("invite-computer",{},[this](const QJsonObject &result){
            copiedCode=result.value("code").toString();invitationId=result.value("invitation_id").toString();expires=qint64(result.value("expires_at").toDouble());
            codeStatus->setText(tr("Computer code ready until %1. Keep this computer online while the other computer joins.").arg(QDateTime::fromSecsSinceEpoch(expires).toLocalTime().toString("HH:mm")));controls(false);
        });});
        connect(copyCode,&QPushButton::clicked,this,[this]{if(!copiedCode.isEmpty()&&QDateTime::currentSecsSinceEpoch()<expires)QApplication::clipboard()->setText(copiedCode);});
        connect(cancelCode,&QPushButton::clicked,this,[this]{request("cancel-invite",{{"invitation_id",invitationId}},[this](const QJsonObject &){clearCode();notice->setText(tr("Code canceled. Computers already joined keep their access."));});});
        enrollmentPoll=new QTimer(this);enrollmentPoll->setSingleShot(true);
        connect(enrollmentPoll,&QTimer::timeout,this,[this]{
            if(!continueEnrollment||!enrollmentWait.isValid()||enrollmentWait.hasExpired(60000)){scheduleEnrollmentCheck();return;}
            if(process){scheduleEnrollmentCheck();return;}
            if(!known||snapshot.value("pending").toObject().value("state")!="migration-draining")return;
            checkEnrollment(true);
        });
        invitationPoll=new QTimer(this);invitationPoll->setInterval(5000);
        connect(invitationPoll,&QTimer::timeout,this,[this]{
            if(copiedCode.isEmpty())return;
            if(QDateTime::currentSecsSinceEpoch()>=expires){clearCode();codeStatus->setText(tr("Computer code expired."));return;}
            if(!process&&known&&!snapshot.contains("pending"))request("check",{},[](const QJsonObject &){});
        });invitationPoll->start();
        updateForm();controls(true);refresh();
    }
    ~Panel() override {
        invitationPoll->stop();enrollmentPoll->stop();copiedCode.clear();
        if(process){for(auto *timer:process->findChildren<QTimer*>())timer->stop();process->disconnect();process->kill();process->waitForFinished(1000);}
    }
    void refresh(){request("status");}
private:
    QString program,path,copiedCode,invitationId;QJsonObject snapshot;QProcess *process=nullptr;
    bool known=false,changing=false,recoveringEnrollment=false,continueEnrollment=false; qint64 expires=0;
    QString recoveryRevision;
    QLabel *summary,*connection,*notice,*pending,*codeStatus,*codeLabel;
    QLineEdit *url,*code;QComboBox *mode;
    QPushButton *change,*start,*stop,*reload,*submit,*check,*giveCode,*copyCode,*cancelCode;
    QWidget *enrollment,*invitationRow;QTimer *invitationPoll,*enrollmentPoll;
    QElapsedTimer enrollmentWait;
    static bool hex(const QJsonValue &value,int size){
        if(!value.isString()||value.toString().size()!=size)return false;
        for(auto c:value.toString())if(!((c>='0'&&c<='9')||(c>='a'&&c<='f')))return false;return true;
    }
    static bool lifecycle(const QJsonObject &value){
        const auto state=value.value("state").toString();if(!QStringList{"running","stopping","stopped"}.contains(state)||!value.value("running").isBool())return false;
        return state=="stopped"?!value.value("running").toBool()&&value.value("generation").isNull():value.value("running").toBool()&&hex(value.value("generation"),32);
    }
    static bool statusShape(const QJsonObject &value){
        if(!value.value("encrypted").isBool()||!value.value("config_revision").isString())return false;
        if(!value.value("config_revision").toString().isEmpty()&&!hex(value.value("config_revision"),64))return false;
        const auto state=value.value("state").toString();
        if(state=="unconfigured"||state=="legacy"){
            if(value.value("encrypted").toBool())return false;
            const auto pendingState=value.value("pending").toObject().value("state").toString();
            if(pendingState.startsWith("migration-"))return value.value("migration").isObject()&&migrationShape(value.value("migration").toObject(),pendingState);
            return !value.contains("migration");
        }
        if(state=="pending-activation")return value.value("encrypted").toBool();
        if((state!="active"&&state!="retired")||!value.value("encrypted").toBool()||!RelaySettings::validUrl(value.value("server_url").toString())||value.value("node_id").toString().isEmpty()||value.value("workspace_id").toString().isEmpty())return false;
        return lifecycle({{"state",value.value("connector_state")},{"running",value.value("running")},{"generation",value.value("generation")}});
    }
    bool confirmed(const QString &action,const QJsonObject &value,const QJsonObject &sent)const{
        if(value.contains("ok")&&value.value("ok")!=QJsonValue(true))return false;
        if(action=="status")return statusShape(value);
        if(action=="start"||action=="stop")return lifecycle(value)&&(action!="stop"||value.value("state")=="stopped"||value.value("generation")==sent.value("generation"));
        if(action=="invite-computer")return value.value("purpose")=="computer"&&value.value("code").toString().startsWith("ze1.")&&value.value("code").toString().size()<=4096&&value.value("server_url")==snapshot.value("server_url")&&!value.value("invitation_id").toString().isEmpty()&&value.value("expires_at").isDouble();
        if(action=="cancel-invite")return value.value("cancelled")==QJsonValue(true);
        if(value.value("state")=="migration-draining"||value.value("state")=="migration-blocked")return value.value("config_revision")==sent.value("expected_revision")&&!value.value("node_id").toString().isEmpty()&&value.value("encrypted")==QJsonValue(false)&&migrationShape(value.value("migration").toObject(),value.value("state").toString());
        if(value.value("state")=="pending"||value.value("state")=="legacy-retirement-unconfirmed")return value.value("config_revision")==sent.value("expected_revision")&&!value.value("node_id").toString().isEmpty();
        return value.value("state")=="active"&&value.value("encrypted")==QJsonValue(true)&&hex(value.value("config_revision"),64)&&!value.value("node_id").toString().isEmpty();
    }
    static bool migrationShape(const QJsonObject &migration,const QString &state){
        const auto reason=migration.value("reason").toString();
        if(!migration.value("retryable").isBool())return false;
        if(state=="migration-draining")return migration.value("phase")=="draining"&&migration.value("retryable").toBool()&&QStringList{"draining","drain-timeout"}.contains(reason);
        return state=="migration-blocked"&&migration.value("phase")=="blocked"&&!migration.value("retryable").toBool()&&QStringList{"unsafe-unit","unsupported-runtime","unsupported-platform","identity-changed","service-definition-conflict","service-unconfirmed","journal-unavailable"}.contains(reason);
    }
    void finishEnrollmentUi(){
        changing=false;recoveringEnrollment=false;code->clear();clearCode();
    }
    void scheduleEnrollmentCheck(){
        if(!continueEnrollment)return;
        if(enrollmentWait.isValid()&&!enrollmentWait.hasExpired(60000))enrollmentPoll->start(1000);
        else{continueEnrollment=false;notice->setText(tr("The connector is still finishing its current request. Your saved connection is kept; choose Continue connecting to check again."));}
    }
    void enrollmentResult(const QJsonObject &value){
        finishEnrollmentUi();
        const auto state=value.value("state").toString();
        continueEnrollment=state=="migration-draining";
        if(state=="migration-draining"){
            notice->setText(tr("Finishing the current phone request before switching. Local agents keep running."));
            scheduleEnrollmentCheck();
        }else if(state=="migration-blocked"){
            enrollmentPoll->stop();
            const auto reason=value.value("migration").toObject().value("reason").toString();
            if(reason=="unsupported-runtime")notice->setText(tr("The existing connector needs an update before this switch can continue. Your saved connection is kept."));
            else if(reason=="unsafe-unit"||reason=="service-definition-conflict")notice->setText(tr("The existing connector's service settings do not allow a safe automatic switch. Your saved connection is kept."));
            else notice->setText(tr("The connector switch could not be confirmed. Your saved connection is kept; refresh its status before continuing."));
        }else{
            enrollmentPoll->stop();
            if(state=="pending")notice->setText(tr("Waiting for the inviting computer. Keep it online, then check this connection."));
            else if(state=="legacy-retirement-unconfirmed")notice->setText(tr("The old relay has not confirmed retirement. The new connection is saved; your current configuration is unchanged."));
            else if(value.value("legacy_retirement")=="unconfirmed")notice->setText(tr("The encrypted workspace is connected. Legacy relay retirement remains unconfirmed; its old records or credentials may remain on that relay."));
            else notice->setText(tr("Workspace connected."));
        }
        refresh();
    }
    void clearCode(){copiedCode.clear();invitationId.clear();expires=0;codeStatus->clear();controls(process!=nullptr);}
    void updateForm(){const bool joining=mode->currentData()=="join";code->setVisible(joining);codeLabel->setVisible(joining);submit->setText(changing?(joining?tr("Change and join workspace"):tr("Change and create workspace")):(joining?tr("Join workspace"):tr("Create workspace")));}
    void controls(bool busy){
        const bool active=known&&snapshot.value("state")=="active",hasPending=known&&snapshot.value("pending").isObject();
        const bool writable=known&&!busy&&!hasPending&&snapshot.value("state")!="pending-activation";
        change->setVisible(known&&(active||snapshot.value("state")=="legacy"||snapshot.value("state")=="retired")&&!changing);change->setEnabled(writable);
        enrollment->setVisible((!active&&snapshot.value("state")!="retired"&&snapshot.value("state")!="legacy")||changing);enrollment->setEnabled(writable);
        start->setVisible(active);stop->setVisible(active||(snapshot.value("state")=="retired"&&snapshot.value("running").toBool()));start->setEnabled(writable&&!snapshot.value("running").toBool());stop->setEnabled(known&&!busy&&snapshot.value("running").toBool()&&hex(snapshot.value("generation"),32));
        reload->setEnabled(!busy);check->setVisible(hasPending);check->setEnabled(known&&!busy&&hasPending);
        const bool legacyUnconfirmed=snapshot.value("pending").toObject().value("state")=="legacy-retirement-unconfirmed";
        const auto pendingState=snapshot.value("pending").toObject().value("state").toString();
        check->setText(legacyUnconfirmed?tr("Disconnect legacy locally and switch…"):pendingState.startsWith("migration-")?tr("Continue connecting"):tr("Check connection"));
        pending->setVisible(hasPending);if(hasPending)pending->setText(legacyUnconfirmed?tr("The new encrypted workspace is ready. The old relay has not confirmed retirement. You can explicitly disconnect its local configuration before switching."):tr("A workspace connection is saved and waiting. Check its result before starting another connection."));
        invitationRow->setVisible(active);giveCode->setEnabled(writable);copyCode->setEnabled(writable&&!copiedCode.isEmpty()&&QDateTime::currentSecsSinceEpoch()<expires);cancelCode->setEnabled(writable&&!copiedCode.isEmpty());
    }
    void render(){
        const auto state=snapshot.value("state").toString();
        if(!changing&&(state=="active"||state=="retired")){
            url->setText(snapshot.value("server_url").toString());
        }
        if(!changing&&state=="legacy"&&RelaySettings::validUrl(snapshot.value("server_url").toString()))url->setText(snapshot.value("server_url").toString());
        if(state=="active"){
            summary->setText(tr("%1 · %2\nPrivate session content is encrypted.").arg(snapshot.value("workspace_name").toString(tr("Workspace")),snapshot.value("computer_name").toString(tr("Computer"))));
            const auto status=snapshot.value("connector_state").toString();connection->setText(status=="running"?tr("Connector: running"):status=="stopping"?tr("Connector: draining its current request"):tr("Connector: stopped"));
        }else if(state=="retired"){summary->setText(tr("This computer was removed from its workspace."));connection->setText(snapshot.value("running").toBool()?tr("The retired connector is draining. You can stop its current generation before changing workspace."):tr("Choose Change workspace to create a workspace or join with a new invitation. Saved local content stays on this computer."));}
        else if(state=="legacy"){summary->setText(tr("Legacy connection: session content is visible to this relay."));connection->setText(tr("Choose Change workspace to create or join an encrypted workspace."));}
        else if(state=="pending-activation"){summary->setText(tr("Workspace activation is incomplete."));connection->setText(tr("Check the saved connection before continuing."));}
        else{summary->setText(tr("No workspace connected"));connection->setText(tr("Create a workspace or use a computer invitation from an enrolled computer."));}
        if(protectionChanged)protectionChanged(snapshot.value("encrypted").toBool());
        controls(false);
    }
    void checkEnrollment(bool automatic=false){
        if(!automatic)enrollmentWait.restart();
        auto run=[this](bool allowLocal){
            QJsonObject payload;
            if(allowLocal)payload.insert("allow_unconfirmed_legacy_retirement",true);
            if(snapshot.value("state")=="legacy")payload.insert("allow_legacy_service_handover",true);
            request("check",payload,[this](const QJsonObject &result){enrollmentResult(result);});
        };
        if(snapshot.value("pending").toObject().value("state")!="legacy-retirement-unconfirmed"){run(false);return;}
        auto *dialog=new QMessageBox(QMessageBox::Warning,tr("Disconnect legacy locally"),tr("The old relay has not confirmed retirement. Its old records and credentials may remain active on that relay. This only disconnects the saved legacy configuration on this computer and switches to the new encrypted workspace. Local agents, projects and conversations stay on this computer."),QMessageBox::Yes|QMessageBox::Cancel,this);
        dialog->setObjectName("confirmLegacyLocalDisconnect");dialog->setDefaultButton(QMessageBox::Cancel);dialog->button(QMessageBox::Yes)->setText(tr("Disconnect legacy locally and switch"));dialog->setAttribute(Qt::WA_DeleteOnClose);
        connect(dialog,&QMessageBox::finished,this,[run](int result){if(result==QMessageBox::Yes)run(true);});dialog->open();
    }
    void enroll(){
        const auto relay=RelaySettings::normalizedUrl(url->text());
        if(!RelaySettings::validUrl(relay)){notice->setText(tr("Enter an HTTPS relay URL without credentials, query or fragment."));return;}
        const bool joining=mode->currentData()=="join";
        if(joining&&(!code->text().startsWith("ze1.")||code->text().size()>4096)){notice->setText(tr("Enter a computer invitation code from an enrolled computer."));return;}
        const auto computer=QSysInfo::machineHostName().trimmed().left(128);
        QJsonObject payload{{"expected_revision",snapshot.value("config_revision")},{"server_url",relay},{"workspace_name",tr("Workspace")},{"computer_name",computer.isEmpty()?tr("Computer"):computer}};
        if(changing&&snapshot.value("state")=="legacy")payload.insert("allow_legacy_service_handover",true);
        if(joining)payload.insert("code",code->text());
        const auto action=(changing?QStringLiteral("switch-"):QString())+(joining?QStringLiteral("join"):QStringLiteral("create"));
        auto submitAction=[this,action,payload]{request(action,payload,[this](const QJsonObject &value){
            enrollmentResult(value);
        });};
        if(!changing){enrollmentWait.restart();submitAction();return;}
        auto *dialog=new QMessageBox(QMessageBox::Warning,tr("Change workspace"),tr("This computer will leave its current workspace. Phone access through this computer will end. Local agents, projects and saved conversations stay on this computer."),QMessageBox::Yes|QMessageBox::Cancel,this);
        dialog->setObjectName("confirmWorkspaceChange");dialog->setDefaultButton(QMessageBox::Cancel);dialog->button(QMessageBox::Yes)->setText(tr("Change workspace"));dialog->setAttribute(Qt::WA_DeleteOnClose);
        connect(dialog,&QMessageBox::finished,this,[this,submitAction](int result){
            if(result!=QMessageBox::Yes)return;
            enrollmentWait.restart();
            if(snapshot.value("running").toBool())request("stop",{{"generation",snapshot.value("generation")}},[this,submitAction](const QJsonObject &value){
                if(value.value("state")=="stopped")submitAction();else{notice->setText(tr("The current phone request is still draining. Refresh status, then continue the workspace change."));refresh();}
            });else submitAction();
        });dialog->open();
    }
    void request(const QString &action,QJsonObject payload={},std::function<void(const QJsonObject&)> done={}){
        if(process)return;
        if(action!="status"&&!payload.contains("expected_revision"))payload.insert("expected_revision",snapshot.value("config_revision"));
        auto *p=new QProcess(this);process=p;controls(true);
        if(action!="status"&&protectionChanged)protectionChanged(true);
        auto environment=QProcessEnvironment::systemEnvironment();
        for(const auto &key:{"HGS_SESSION","HGS_RUN_ID","HGS_EXPECTED_ID","HGS_AGENT","HGS_HOST_PANE","HGS_HOST_SESSION","TMUX","TMUX_PANE"})environment.remove(QLatin1String(key));
        p->setProcessEnvironment(environment);p->setProcessChannelMode(QProcess::SeparateChannels);
        auto *deadline=new QTimer(p);deadline->setSingleShot(true);auto output=std::make_shared<QByteArray>();
        auto finish=[this,p,deadline,output,action,payload,done](bool success){
            if(process!=p)return;deadline->stop();process=nullptr;
            const auto bytes=*output+p->readAllStandardOutput();const auto doc=QJsonDocument::fromJson(bytes);const auto result=doc.object();
            if(!success||!doc.isObject()||bytes.size()>65536||!confirmed(action,result,payload)){
                enrollmentPoll->stop();
                continueEnrollment=false;
                if(QStringList{"create","join","switch-create","switch-join"}.contains(action)){recoveringEnrollment=true;recoveryRevision=payload.value("expected_revision").toString();}
                known=false;controls(true);reload->setEnabled(true);summary->setText(tr("Workspace status is unconfirmed"));
                if(protectionChanged)protectionChanged(true);
                const auto error=result.value("error").toString();notice->setText(error.isEmpty()||error.size()>1024?tr("Workspace operation was not confirmed. Refresh status before continuing. Verify that the matching computer connector is installed."):error);
                // Enrollment may already be durable even when activation failed.
                // Recover its saved state without repeating the mutation.
                if(action!="status")QTimer::singleShot(0,this,[this]{refresh();});
            }else{
                controls(false);
                if(action=="status"){
                    const bool changed=snapshot.value("config_revision")!=result.value("config_revision");
                    snapshot=result;known=true;
                    if(recoveringEnrollment&&(snapshot.value("pending").isObject()||(snapshot.value("state")=="active"&&snapshot.value("config_revision").toString()!=recoveryRevision)))finishEnrollmentUi();
                    render();
                    if(changed&&connectionChanged)connectionChanged();
                }
                else if(done)done(result);
            }
            p->deleteLater();
        };
        connect(deadline,&QTimer::timeout,p,[p]{p->kill();});
        connect(p,&QProcess::readyReadStandardOutput,p,[p,output]{*output+=p->readAllStandardOutput();if(output->size()>65536)p->kill();});
        connect(p,&QProcess::readyReadStandardError,p,[p]{p->readAllStandardError();});
        connect(p,&QProcess::started,p,[p,payload]{p->write(QJsonDocument(payload).toJson(QJsonDocument::Compact));p->closeWriteChannel();});
        connect(p,qOverload<int,QProcess::ExitStatus>(&QProcess::finished),this,[finish](int code,QProcess::ExitStatus status){finish(code==0&&status==QProcess::NormalExit);});
        connect(p,&QProcess::errorOccurred,this,[finish](QProcess::ProcessError error){if(error==QProcess::FailedToStart)finish(false);});
        ChildProcess::inheritStdioOnly(*p);p->start(program,{"--workspace",action,"--config",path});deadline->start(70000);
    }
};
}
