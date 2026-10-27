#include "JsonParse.h"
#include "SessionCreationController.h"
#include "MachinePlatform.h"
#include <QCoreApplication>
#include <QDateTime>
#include <QFile>
#include <QDir>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QPointer>
#include <QSaveFile>
#include <QUrl>
#include <QRegularExpression>
#include <QSettings>
#include <QTimer>
#include <QUuid>
#include <algorithm>

namespace {
QString uuid() { return QUuid::createUuid().toString(QUuid::WithoutBraces); }
bool terminal(const QJsonObject &v) {
    const auto state=v.value("state").toString();
    return state=="bound"||state=="failed"||state=="stopped";
}
QString requestKey(const char *kind, quint64 request) { return QLatin1String(kind)+QString::number(request); }
constexpr qint64 JournalLimit=8*1024*1024;
bool safeRepositoryUrl(const QString &url){
    if(!url.contains("://"))return true;
    const QUrl parsed(url,QUrl::StrictMode);
    return parsed.isValid()&&parsed.password().isEmpty()&&!parsed.hasQuery()&&!parsed.hasFragment()
        &&(parsed.scheme()=="ssh"||parsed.userName().isEmpty());
}
QString diagnostic(QString value){
    value.replace(QRegularExpression("(https?://)[^/\\s@]+@",QRegularExpression::CaseInsensitiveOption),"\\1[redacted]@");
    value.replace(QRegularExpression("\\b(authorization|api[_-]?key|access[_-]?token|password)\\s*[:=]\\s*\\S+",QRegularExpression::CaseInsensitiveOption),"\\1=[redacted]");
    return value.left(4000);
}
bool validOperation(const QString &id,const QJsonObject &v){
    static const QSet<QString> states{"saving","queued","checking","checking_repository","cloning","worktree","verifying","launching","confirming","recover","unknown","stopping","stopped","failed","storage_error","bound"};
    if(QUuid(id).isNull()||v.value("id")!=id||v.value("creation_schema")!=1||!states.contains(v.value("state").toString())
        ||v.value("cmd").toString().isEmpty()||v.value("tag").toString().isEmpty()||v.value("target").toString().isEmpty()||!v.value("created").isDouble())return false;
    if(v.value("cmd").toString().size()>64||v.value("cmd").toString().startsWith('-')||v.value("cmd").toString().contains(' '))return false;
    for(const auto &key:{"auto","repository","clone_done","worktree_done","launch_dispatched","stop_requested","show_pending","new_project","implicit_inbox","worktree_receipts"})
        if(v.contains(key)&&!v[key].isBool())return false;
    if(v.value("history").toArray().size()>32||v.value("previous_launches").toArray().size()>16||v.value("worktree_previous").toArray().size()>16)return false;
    for(const auto &key:{"host","cmd","tag","target"}){
        const auto text=v[key].toString();if(text.size()>8192)return false;
        for(const auto character:text)if(character.unicode()<32)return false;
    }
    for(const auto &key:{"launch_id","clone_id","worktree_id","expected_machine"})if(v.contains(key)&&QUuid(v[key].toString()).isNull())return false;
    if(v.value("repository").toBool()&&!safeRepositoryUrl(v.value("url").toString()))return false;
    return true;
}
}

SessionCreationController::SessionCreationController(const QString &hgs,QObject *parent)
    :QObject(parent),m_client(hgs,this),m_path(QSettings().fileName()+".creations.json"),m_writer(new QObject)
{
    m_writer->moveToThread(&m_thread);
    connect(&m_thread,&QThread::finished,m_writer,&QObject::deleteLater);
    connect(QCoreApplication::instance(),&QCoreApplication::aboutToQuit,this,[this]{m_quitting=true;flush();});
    m_thread.setObjectName("zerus-creation-journal");m_thread.start();
    auto *clock=new QTimer(this);clock->setInterval(1000);connect(clock,&QTimer::timeout,this,&SessionCreationController::clockTick);clock->start();
    const QPointer<SessionCreationController> guard(this);const auto path=m_path;
    QMetaObject::invokeMethod(m_writer,[guard,path]{
        QFile file(path);QJsonObject saved;QString error;
        if(file.exists()){
            if(!file.open(QIODevice::ReadOnly)||file.size()>JournalLimit)error=tr("Could not read the creation journal. Existing requests are kept.");
            else{
                QString parse;saved=JsonParse::object(file.readAll(),&parse);
                if(!parse.isEmpty()||saved.value("schema")!=1||!saved.value("operations").isObject()
                    ||saved.value("operations").toObject().size()>128)error=tr("Unsupported creation journal. Existing requests are kept.");
            }
        }
        QMetaObject::invokeMethod(QCoreApplication::instance(),[guard,saved,error]{
            if(!guard)return;auto *self=guard.data();self->m_loaded=true;self->m_storageError=error;
            const auto operations=saved.value("operations").toObject();
            if(error.isEmpty())for(auto it=operations.begin();it!=operations.end();++it){
                if(!it.value().isObject()||!validOperation(it.key(),it.value().toObject())){self->m_storageError=tr("Invalid creation journal identity. Requests are kept.");break;}
                if(self->m_operations.contains(it.key()))continue;
                auto value=it.value().toObject();
                if(!terminal(value)){
                    value["interrupted_stage"]=value.value("state");value["state"]="recover";
                    value["error"]=tr("Setup was interrupted. Check the original result before continuing.");
                }
                self->m_operations[it.key()]=value;
                if(value.value("state")!="bound")emit self->changed(it.key(),value);
            }
            const auto current=self->m_operations;
            int unfinished=0;
            for(auto it=current.begin();it!=current.end();++it)if(it.value().toObject().value("state")!="saving"&&!terminal(it.value().toObject()))++unfinished;
            for(auto it=current.begin();it!=current.end();++it)if(it.value().toObject().value("state")=="saving"){
                const auto id=it.key();
                if(!self->m_storageError.isEmpty())self->failed(id,self->m_storageError);
                else if(unfinished>=32||self->m_operations.size()>128){
                    auto value=self->operation(id);value["unsaved_admission"]=true;self->put(id,value);
                    self->failed(id,tr("The saved creation queue is full. No operation was sent. Resolve existing requests before retrying."));
                }
                else {++unfinished;self->stage(id,"queued");self->persist(id,[self]{self->pump();});}
            }
        },Qt::QueuedConnection);
    },Qt::QueuedConnection);
    connect(&m_client,&HgsClient::directoriesReady,this,[this](quint64 request,const QString &,const QJsonObject &result){
        auto id=take("identity",request);const bool identityOnly=!id.isEmpty();
        if(id.isEmpty())id=take("directory",request);if(id.isEmpty())return;
        auto value=operation(id);const auto path=result.value("path").toString();
        const auto expected=value.value("expected_machine").toString(),actual=result.value("computer_id").toString();
        if(!expected.isEmpty()&&(actual!=expected||!result.value("creation_identity_v1").toBool()))return failed(id,tr("The computer identity changed. The original setup has been kept."));
        if(result.value("creation_identity_v1").toBool()){
            if(QUuid(actual).isNull()||QUuid(actual).toString(QUuid::WithoutBraces)!=actual)return failed(id,tr("The computer did not confirm its identity."));
            value["expected_machine"]=actual;
        }
        if(identityOnly){put(id,value);persist(id,[this,id]{repositoryCheck(id);});return;}
        const auto style=MachinePlatform::windowsShaped(path)?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
        if(!MachinePlatform::absolute(path,style))return failed(id,tr("The computer did not confirm this folder."));
        value["target"]=path;if(value.value("add_folder").toBool()&&!value.value("worktree_done").toBool())value["folder"]=path;
        auto evidence=result;if(value.value("acquisition_confirmed").toBool()){evidence["cloned"]=true;value.remove("acquisition_confirmed");}
        QString error;if(resolvePlacement&&!resolvePlacement(value,evidence,error))return failed(id,error);
        put(id,value);checkout(id);
    });
    connect(&m_client,&HgsClient::directoriesFailed,this,[this](quint64 request,const QString &,const QString &error){
        auto id=take("identity",request);if(id.isEmpty())id=take("directory",request);if(!id.isEmpty())failed(id,error);
    });
    connect(&m_client,&HgsClient::worktreesReady,this,[this](quint64 request,const QString &,const QString &,const QJsonObject &result){
        const auto id=take("catalog",request);if(id.isEmpty())return;auto value=operation(id);
        if(value.value("stop_requested").toBool()){stage(id,"stopped");release(id);persist(id);return;}
        if(result.value("state")!="ok"||result.value("stale").toBool()||result.value("common_dir").toString().isEmpty())
            return failed(id,tr("The repository could not be confirmed. Choose the working folder again."));
        value["catalog"]=result;put(id,value);
        if(value.value("worktree_done").toBool()){
            const auto style=MachinePlatform::windowsShaped(value.value("target").toString())?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
            if(!MachinePlatform::same(result.value("common_dir").toString(),value.value("worktree_common").toString(),style))return failed(id,tr("The prepared checkout now belongs to another repository."));
            bool matches=MachinePlatform::same(result.value("selected_root").toString(),value.value("target").toString(),style)&&result.value("selected_branch")==value.value("branch");
            for(const auto &entry:result.value("worktrees").toArray()){
                const auto row=entry.toObject();const auto path=row.value("path").toString();
                const auto style=MachinePlatform::windowsShaped(path)?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
                if(MachinePlatform::same(path,value.value("target").toString(),style)&&row.value("available").toBool()&&row.value("branch")==value.value("branch"))matches=true;
            }
            if(!matches)return failed(id,tr("The prepared worktree branch could not be confirmed. Inspect its current state before continuing."));
            if(value.value("worktree_receipts").toBool())flight("verify_receipt",m_client.worktreeReceipt(value.value("host").toString(),"status",value.value("worktree_id").toString(),value.value("expected_machine").toString()),id);
            else launch(id);return;
        }
        const auto rawRoot=result.value("selected_root").toString();
        const auto root=MachinePlatform::clean(rawRoot,MachinePlatform::windowsShaped(rawRoot)?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix);
        if(root.isEmpty())return failed(id,tr("The computer did not confirm the checkout root."));
        const int slash=root.lastIndexOf('/');
        const auto destination=root.left(slash+1)+root.mid(slash+1)+'-'+value.value("branch").toString();
        const auto style=MachinePlatform::windowsShaped(destination)?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
        if(!value.value("destination").toString().isEmpty()&&!MachinePlatform::same(value.value("destination").toString(),destination,style))
            return failed(id,tr("The checkout destination changed. Review the creation choices before continuing."));
        value["destination"]=destination;
        if(value.value("worktree_id").toString().isEmpty())value["worktree_id"]=uuid();
        value["worktree_receipts"]=result.value("creation_receipts_v1").toBool();
        value["worktree_common"]=result.value("common_dir");put(id,value);stage(id,"worktree");
        persist(id,[this,id]{
            const auto v=operation(id);if(v.value("stop_requested").toBool())return failed(id,tr("Creation stopped before checkout."));
            flight("create",m_client.createWorktree(v.value("host").toString(),v.value("target").toString(),v.value("worktree_common").toString(),
                v.value("branch").toString(),"HEAD",v.value("destination").toString(),v.value("worktree_id").toString(),v.value("worktree_receipts").toBool(),v.value("expected_machine").toString()),id);
        });
    });
    connect(&m_client,&HgsClient::worktreesFailed,this,[this](quint64 request,const QString &,const QString &,const QString &error){
        const auto id=take("catalog",request);if(!id.isEmpty())failed(id,error);
    });
    connect(&m_client,&HgsClient::worktreeCreated,this,[this](quint64 request,bool okay,const QJsonObject &result,const QString &error){
        const auto id=take("create",request);if(id.isEmpty())return;
        if(!okay)return worktreeFailed(id,result,error);prepared(id,result,false);
    });
    connect(&m_client,&HgsClient::worktreeReceiptReady,this,[this](quint64 request,bool okay,const QJsonObject &result,const QString &error){
        auto id=take("verify_receipt",request);const bool verifying=!id.isEmpty();
        if(id.isEmpty())id=take("receipt",request);if(id.isEmpty())return;
        if(!okay)return failed(id,error,true);
        if(!result.value("resource_error").toString().isEmpty())return failed(id,result.value("resource_error").toString());
        if(verifying&&result.value("status")=="created")launch(id);
        else if(result.value("status")=="created")prepared(id,result,true);
        else if(result.value("status")=="failed")worktreeFailed(id,result,result.value("detail").toString(tr("Worktree preparation failed. Retained files have been kept.")));
        else {stage(id,"unknown",tr("The original worktree is still running or its result is unknown. Check again; it will not be recreated."));persist(id);}
    });
    connect(&m_client,&HgsClient::repositoryFinished,this,[this](quint64 request,bool okay,const QJsonObject &result,const QString &error){
        const auto id=take("repository",request);if(id.isEmpty())return;auto value=operation(id);
        if(!okay)return failed(id,error,value.value("repository_action")!="check");
        const auto action=value.value("repository_action").toString();
        if(action=="check"){
            if(value.value("stop_requested").toBool()){stage(id,"stopped");release(id);persist(id);return;}
            const auto path=result.value("path").toString();
            const auto style=MachinePlatform::windowsShaped(path)?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
            const auto expected=MachinePlatform::clean(value.value("target").toString(),style);
            // A relative ~/ suggestion is resolved by the owning computer. An
            // already absolute choice cannot silently become another destination.
            if(MachinePlatform::absolute(expected,style)&&!MachinePlatform::same(expected,path,style))
                return failed(id,tr("The computer proposed another destination. Review it before cloning."));
            value["target"]=path;value["parent"]=result.value("parent");value["clone_id"]=uuid();value["repository_action"]="clone";
            put(id,value);stage(id,"cloning");persist(id,[this,id]{
                const auto v=operation(id);if(v.value("stop_requested").toBool())return;
                flight("repository",m_client.repositoryOperation(v.value("host").toString(),"clone",v.value("url").toString(),
                    v.value("parent").toString(),v.value("repository_name").toString(),v.value("clone_id").toString(),v.value("expected_machine").toString()),id);
            });return;
        }
        const auto style=MachinePlatform::windowsShaped(value.value("target").toString())?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
        if(result.value("request_id")!=value.value("clone_id")||!MachinePlatform::same(result.value("path").toString(),value.value("target").toString(),style))
            return failed(id,tr("The clone receipt does not match this request."),true);
        if(result.value("status")!="cloned")return failed(id,result.value("detail").toString(tr("The original clone result is unknown. Check again before continuing.")),result.value("status")!="failed");
        value["clone_done"]=true;value["add_folder"]=false;put(id,value);
        QString failure;auto evidence=result;evidence["cloned"]=true;
        if(resolvePlacement&&!resolvePlacement(value,evidence,failure))return failed(id,failure);
        put(id,value);
        if(action=="status"){stage(id,"recover",tr("Checkout recovered. Continue setup to start the session."));release(id);persist(id);}
        else {stage(id,"checking");persist(id,[this,id]{directory(id);});}
    });
}

SessionCreationController::~SessionCreationController(){m_quitting=true;flush();m_thread.quit();m_thread.wait();}
void SessionCreationController::flush(){QMetaObject::invokeMethod(m_writer,[]{},Qt::BlockingQueuedConnection);}
QString SessionCreationController::take(const char *kind,quint64 request){
    const auto id=m_flights.take(requestKey(kind,request));return operation(id).value("state")=="bound"?QString():id;
}
void SessionCreationController::flight(const char *kind,quint64 request,const QString &id){m_flights.insert(requestKey(kind,request),id);}
void SessionCreationController::put(const QString &id,QJsonObject value){
    value["revision"]=operation(id).value("revision").toInteger()+1;m_operations[id]=value;emit changed(id,value);
}
void SessionCreationController::stage(const QString &id,const QString &state,const QString &error){
    auto v=operation(id);v["state"]=state;v["error"]=diagnostic(error);v["updated"]=QDateTime::currentSecsSinceEpoch();
    auto history=v.value("history").toArray();history.append(QJsonObject{{"state",state},{"at",v.value("updated")}});
    while(history.size()>32)history.removeFirst();v["history"]=history;put(id,v);
}
void SessionCreationController::persist(const QString &id,std::function<void()> after){
    if(!m_loaded)return;
    if(!m_storageError.isEmpty()){stage(id,"storage_error",m_storageError);return;}
    const auto revision=operation(id).value("revision").toInteger();
    auto journal=m_operations;
    for(auto it=journal.begin();it!=journal.end();)if(it.value().toObject().value("unsaved_admission").toBool())it=journal.erase(it);else ++it;
    if(journal.size()>128){stage(id,"storage_error",tr("Creation history is full. Hide resolved requests before continuing."));release(id);return;}
    const auto bytes=QJsonDocument(QJsonObject{{"schema",1},{"operations",journal}}).toJson(QJsonDocument::Compact);
    const auto path=m_path;const QPointer<SessionCreationController> guard(this);
    const auto observer=m_writeObserver;
    const bool remember=m_rememberChoices.remove(id)>0;const auto choices=operation(id);
    QMetaObject::invokeMethod(m_writer,[guard,path,bytes,id,revision,after,observer,remember,choices]{
        if(observer)observer();
        QSaveFile file(path);QString error;
        if(bytes.size()>JournalLimit||!QDir().mkpath(QFileInfo(path).absolutePath())||!file.open(QIODevice::WriteOnly)||!file.setPermissions(QFile::ReadOwner|QFile::WriteOwner)
            ||file.write(bytes)!=bytes.size()||!file.commit())error=tr("Could not save creation progress. No further setup step was sent.");
        if(error.isEmpty()&&remember){
            QSettings settings;settings.setValue("workspace/launchOpenTerminal",choices.value("terminal").toBool());
            settings.setValue("workspace/launchAgent",choices.value("cmd").toString());
            if(!choices.value("account").toString().isEmpty()){
                auto accounts=JsonParse::object(settings.value("workspace/launchAccounts").toByteArray());
                auto machine=accounts[choices.value("host").toString()].toObject();machine[choices.value("cmd").toString()]=choices.value("account");
                accounts[choices.value("host").toString()]=machine;settings.setValue("workspace/launchAccounts",QJsonDocument(accounts).toJson(QJsonDocument::Compact));
            }
        }
        QMetaObject::invokeMethod(QCoreApplication::instance(),[guard,id,revision,error,after]{
            if(!guard||guard->m_quitting||guard->operation(id).value("revision").toInteger()!=revision)return;
            if(!error.isEmpty()){guard->stage(id,"storage_error",error);guard->release(id);return;}
            if(after)after();
        },Qt::QueuedConnection);
    },Qt::QueuedConnection);
}

QString SessionCreationController::submit(QJsonObject plan){
    m_admissionMessage.clear();
    if(plan.value("repository").toBool()&&!safeRepositoryUrl(plan.value("url").toString())){
        m_admissionMessage=tr("Use a repository address without embedded passwords, tokens, query or fragment.");return {};
    }
    // Only settled, hidden history is eligible for bounded local eviction.
    if(m_operations.size()>=96){
        QStringList candidates;
        for(auto it=m_operations.begin();it!=m_operations.end();++it)if(terminal(it.value().toObject())&&!it.value().toObject().value("show_pending").toBool())candidates<<it.key();
        std::sort(candidates.begin(),candidates.end(),[this](const QString &a,const QString &b){return operation(a).value("created").toInteger()<operation(b).value("created").toInteger();});
        for(const auto &key:candidates){
            if(m_operations.size()<96)break;const auto old=operation(key);auto launches=old.value("previous_launches").toArray();launches.append(old.value("launch_id"));
            for(const auto &launch:launches)if(!launch.toString().isEmpty()){
                m_retiredLaunches[launch.toString()]=key;m_retiredOrder.append(launch.toString());
                while(m_retiredOrder.size()>256)m_retiredLaunches.remove(m_retiredOrder.takeFirst());
            }
            m_operations.remove(key);emit removed(key);
        }
    }
    if(!admissionError().isEmpty())return {};
    const auto id=uuid();plan["id"]=id;plan["creation_schema"]=1;plan["created"]=QDateTime::currentSecsSinceEpoch();
    if(plan.value("new_project").toBool())plan["reserved_project"]=uuid();
    plan["show_pending"]=true;plan["launch_id"]=id;plan["state"]="saving";
    if(!validOperation(id,plan)){m_admissionMessage=tr("Creation choices are incomplete. Review the folder, agent and session name.");return {};}
    put(id,plan);m_rememberChoices.insert(id);
    QTimer::singleShot(0,this,[this,id]{if(m_loaded&&operation(id).value("state")=="saving"&&!operation(id).value("stop_requested").toBool()){stage(id,"queued");persist(id,[this]{pump();});}});
    return id;
}
QString SessionCreationController::admissionError() const{
    if(!m_admissionMessage.isEmpty())return m_admissionMessage;
    int unsettled=0;for(auto it=m_operations.begin();it!=m_operations.end();++it)if(!terminal(it.value().toObject()))++unsettled;
    if(unsettled>=32)return tr("There are already 32 unfinished creations. Resolve or stop an existing request first.");
    if(m_operations.size()>=96)return tr("Creation history is full. Hide resolved creation cards before starting another.");return {};
}
void SessionCreationController::pump(){
    if(m_quitting)return;
    const auto all=m_operations;
    for(auto it=all.begin();it!=all.end()&&m_running.size()<4;++it){
        if(it.value().toObject().value("state")!="queued"||m_running.contains(it.key()))continue;
        const auto v=it.value().toObject();const auto catalog=v.value("catalog").toObject();
        const auto repo=v.value("host").toString()+'\n'+catalog.value("common_dir").toString(v.value("target").toString());
        bool busy=false;for(const auto &running:m_running){const auto other=operation(running);busy|=repo==other.value("host").toString()+'\n'+other.value("catalog").toObject().value("common_dir").toString(other.value("target").toString());}
        if(busy)continue;m_running.insert(it.key());begin(it.key());
    }
}
void SessionCreationController::begin(const QString &id){
    const auto v=operation(id);
    if(v.value("repository").toBool()&&!v.value("clone_done").toBool()){
        stage(id,"checking_repository");persist(id,[this,id]{const auto p=operation(id);
            flight("identity",m_client.requestCreationDirectory(p.value("host").toString(),p.value("parent").toString(),p.value("expected_machine").toString()),id);
        });
    }else {stage(id,"checking");persist(id,[this,id]{directory(id);});}
}
void SessionCreationController::directory(const QString &id){const auto v=operation(id);flight("directory",m_client.requestCreationDirectory(v.value("host").toString(),v.value("target").toString(),v.value("expected_machine").toString()),id);}
void SessionCreationController::repositoryCheck(const QString &id){
    auto p=operation(id);if(p.value("stop_requested").toBool()){stage(id,"stopped");release(id);persist(id);return;}
    p["repository_action"]="check";put(id,p);
    flight("repository",m_client.repositoryOperation(p.value("host").toString(),"check",p.value("url").toString(),p.value("parent").toString(),p.value("repository_name").toString(),{},p.value("expected_machine").toString()),id);
}
void SessionCreationController::checkout(const QString &id){
    const auto v=operation(id);if(v.value("stop_requested").toBool()){stage(id,"stopped");release(id);persist(id);return;}
    if(v.value("auto").toBool()){
        stage(id,"verifying");flight("catalog",m_client.requestWorktrees(v.value("host").toString(),v.value("target").toString(),true,v.value("expected_machine").toString()),id);
    }else launch(id);
}
void SessionCreationController::launch(const QString &id){
    auto value=operation(id);if(value.value("stop_requested").toBool()){stage(id,"stopped");release(id);persist(id);return;}
    if(value.value("launch_dispatched").toBool())return failed(id,tr("The original launch is unconfirmed. Check its result before starting another agent."),true);
    value["launch_dispatched"]=true;put(id,value);stage(id,"launching");
    persist(id,[this,id]{if(!operation(id).value("stop_requested").toBool())emit launchRequested(id,operation(id));});
}
void SessionCreationController::prepared(const QString &id,const QJsonObject &result,bool recovered){
    auto v=operation(id);
    const auto style=MachinePlatform::windowsShaped(v.value("destination").toString())?MachinePlatform::Style::Windows:MachinePlatform::Style::Posix;
    if(result.value("request_id")!=v.value("worktree_id")||!MachinePlatform::same(result.value("path").toString(),v.value("destination").toString(),style)
        ||!MachinePlatform::same(result.value("common_dir").toString(),v.value("worktree_common").toString(),style)||result.value("branch")!=v.value("branch"))
        return failed(id,tr("The worktree result does not match this creation."),true);
    v["worktree_done"]=true;v["target"]=result.value("path");put(id,v);
    if(v.value("stop_requested").toBool()){stage(id,"stopped",tr("Preparation ended. The checkout and branch are kept."));release(id);persist(id);}
    else if(recovered){stage(id,"recover",tr("Worktree recovered. Continue setup to start the session."));release(id);persist(id);}
    else {stage(id,"verifying");persist(id,[this,id]{directory(id);});}
}
void SessionCreationController::worktreeFailed(const QString &id,const QJsonObject &result,const QString &error){
    if(result.value("status")=="failed"&&result.value("effects_started")==QJsonValue(false)){
        auto value=operation(id);auto previous=value.value("worktree_previous").toArray();previous.append(value.value("worktree_id"));
        while(previous.size()>16)previous.removeFirst();value["worktree_previous"]=previous;value.remove("worktree_id");put(id,value);
    }
    failed(id,error,result.value("status")!="failed");
}
void SessionCreationController::failed(const QString &id,const QString &error,bool uncertain){
    auto value=operation(id);value["failed_stage"]=value.value("state");put(id,value);
    stage(id,uncertain?"unknown":operation(id).value("stop_requested").toBool()?"stopped":"failed",error);release(id);persist(id);
}
void SessionCreationController::release(const QString &id){m_running.remove(id);QTimer::singleShot(0,this,[this]{pump();});}
bool SessionCreationController::canResume(const QJsonObject &v){
    const auto state=v.value("state").toString();
    if(state!="failed"&&state!="recover"&&state!="stopped"&&state!="storage_error")return false;
    return !v.value("launch_dispatched").toBool()&&(!v.contains("clone_id")||v.value("clone_done").toBool())&&(!v.contains("worktree_id")||v.value("worktree_done").toBool());
}
void SessionCreationController::resume(const QString &id){
    if(!canResume(operation(id))||m_flights.values().contains(id))return;
    int unsettled=0;for(auto it=m_operations.begin();it!=m_operations.end();++it)if(!terminal(it.value().toObject()))++unsettled;
    if(terminal(operation(id))&&unsettled>=32)return failed(id,tr("There are already 32 unfinished creations. Resolve or stop an existing request before continuing."));
    auto v=operation(id);v["stop_requested"]=false;v.remove("unsaved_admission");put(id,v);stage(id,"queued");persist(id,[this]{pump();});
}
bool SessionCreationController::canEdit(const QString &id) const{
    const auto v=operation(id);return !v.isEmpty()&&!m_flights.values().contains(id)&&!v.value("launch_dispatched").toBool()
        &&(v.value("state")=="failed"||v.value("state")=="stopped"||canResume(v));
}
void SessionCreationController::check(const QString &id){
    auto v=operation(id);if(m_flights.values().contains(id)||v.isEmpty()||v.value("state")=="bound")return;
    if(v.contains("worktree_id")&&!v.value("worktree_done").toBool()&&v.value("worktree_receipts").toBool()){
        flight("receipt",m_client.worktreeReceipt(v.value("host").toString(),"status",v.value("worktree_id").toString(),v.value("expected_machine").toString()),id);
    }else if(v.contains("worktree_id")&&!v.value("worktree_done").toBool()){
        stage(id,"unknown",tr("This computer's older hgs cannot confirm the original worktree result. Inspect the retained destination before creating a separate session; this request will not be repeated."));persist(id);
    }else if(v.contains("clone_id")&&!v.value("clone_done").toBool()){
        v["repository_action"]="status";put(id,v);
        flight("repository",m_client.repositoryOperation(v.value("host").toString(),"status",{},{},{},v.value("clone_id").toString(),v.value("expected_machine").toString()),id);
    }else emit changed(id,v); // The session listing reconciles exact launch IDs.
}
void SessionCreationController::stop(const QString &id){
    auto v=operation(id);if(v.isEmpty()||v.value("state")=="bound"||v.value("launch_dispatched").toBool())return;
    v["stop_requested"]=true;put(id,v);
    const bool active=m_flights.values().contains(id);
    const bool uncertain=(v.contains("worktree_id")&&!v.value("worktree_done").toBool())||(v.contains("clone_id")&&!v.value("clone_done").toBool());
    stage(id,active||uncertain?"stopping":"stopped",active||uncertain?tr("Stopping setup. Retained files will be kept."):QString());
    persist(id,[this,id,active,uncertain]{
        const auto p=operation(id);
        if(p.contains("worktree_id")&&p.value("worktree_receipts").toBool()&&!p.value("worktree_done").toBool())
            flight("receipt",m_client.worktreeReceipt(p.value("host").toString(),"cancel",p.value("worktree_id").toString(),p.value("expected_machine").toString()),id);
        else if(!active&&uncertain)check(id);
        if(!active&&!uncertain)release(id);
    });
}
void SessionCreationController::hide(const QString &id){
    if(!owns(id))return;auto v=operation(id);v["show_pending"]=false;m_operations[id]=v;emit changed(id,v);persist(id);
}
void SessionCreationController::show(const QString &id){
    if(!owns(id)||operation(id).value("state")=="bound")return;
    // Presentation changes do not invalidate an already reserved stage's durable
    // acknowledgement. The ordered writer still saves the new visibility.
    auto v=operation(id);v["show_pending"]=true;m_operations[id]=v;emit changed(id,v);persist(id);
}
void SessionCreationController::complete(const QString &id){if(!owns(id)||operation(id).value("state")=="bound")return;auto v=operation(id);v["show_pending"]=false;put(id,v);stage(id,"bound");release(id);persist(id);}
QString SessionCreationController::forLaunch(const QString &launch) const{
    for(auto it=m_operations.begin();it!=m_operations.end();++it){const auto v=it.value().toObject();
        if(v.value("launch_id")==launch||v.value("previous_launches").toArray().contains(launch))return it.key();}return m_retiredLaunches.value(launch);
}
void SessionCreationController::launchFinished(const QString &launch,bool okay,const QString &error){
    const auto id=forLaunch(launch);if(id.isEmpty()||operation(id).value("state")=="bound"||operation(id).value("launch_id")!=launch)return;
    if(okay){stage(id,"confirming");persist(id);}else failed(id,error,true);
}
void SessionCreationController::ended(const QString &id,const QString &launch,const QString &error){
    if(!owns(id)||operation(id).value("state")=="bound"||operation(id).value("launch_id")!=launch)return;
    auto v=operation(id);auto previous=v.value("previous_launches").toArray();previous.append(v.value("launch_id"));
    while(previous.size()>16)previous.removeFirst();v["previous_launches"]=previous;
    v["launch_dispatched"]=false;v["launch_id"]=uuid();put(id,v);failed(id,error);
}
QString SessionCreationController::caption(const QJsonObject &v){
    const auto s=v.value("state").toString();
    if(s=="saving")return tr("Saving request…");if(s=="queued")return tr("Queued");
    if(s=="checking")return tr("Checking folder…");if(s=="checking_repository")return tr("Checking repository access…");
    if(s=="cloning")return tr("Cloning repository…");if(s=="worktree")return tr("Creating worktree…");
    if(s=="verifying")return tr("Verifying checkout…");if(s=="launching")return tr("Starting agent…");
    if(s=="confirming")return tr("Confirming session…");if(s=="stopping")return tr("Stopping setup…");
    if(s=="stopped")return tr("Setup stopped");if(s=="recover")return tr("Continue setup");
    if(s=="unknown")return tr("Creation result unknown");if(s=="storage_error")return tr("Could not save creation");
    if(s=="failed")return tr("Creation failed");return tr("Session created");
}
QString SessionCreationController::details(const QJsonObject &v){
    QStringList lines{v.value("tag").toString(),tr("Computer: %1").arg(v.value("machine").toString(v.value("host").toString())),
        tr("Agent: %1").arg(v.value("cmd").toString()),tr("Folder: %1").arg(v.value("target").toString()),caption(v)};
    if(!v.value("group_name").toString().isEmpty())lines.insert(1,tr("Project: %1").arg(v.value("new_project").toBool()?v.value("project_name").toString():v.value("group_name").toString()));
    if(!v.value("model").toString().isEmpty())lines<<tr("Model: %1\nEffort: %2").arg(v.value("model").toString(),v.value("effort").toString());
    if(!terminal(v)&&v.value("state")!="recover"&&v.value("updated").toInteger()>0&&QDateTime::currentSecsSinceEpoch()-v.value("updated").toInteger()>=30)
        lines<<tr("Taking longer than usual. The last confirmed stage is shown above; a quiet operation may still be running.");
    if(!v.value("error").toString().isEmpty())lines<<v.value("error").toString();
    if(!v.value("failed_stage").toString().isEmpty()&&(v.value("state")=="failed"||v.value("state")=="unknown"))
        lines<<tr("Last setup stage: %1").arg(caption(QJsonObject{{"state",v.value("failed_stage")}}));
    if(v.contains("destination"))lines<<tr("Worktree destination: %1\nBranch: %2").arg(v.value("destination").toString(),v.value("branch").toString());
    lines<<tr("Created: %1").arg(QDateTime::fromSecsSinceEpoch(v.value("created").toInteger()).toString(Qt::ISODate));
    for(const auto &entry:v.value("history").toArray()){
        const auto step=entry.toObject();lines<<QDateTime::fromSecsSinceEpoch(step.value("at").toInteger()).toString("HH:mm:ss")+"  "+caption(step);
    }
    return lines.join("\n\n").left(65536);
}
