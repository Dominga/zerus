#pragma once
#include "JsonParse.h"
#include "ProcessRunner.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QPushButton>
#include <QSpinBox>
#include <QSignalBlocker>
#include <QVBoxLayout>

// Each owning CLI keeps its policy. Reads and writes use the selected route;
// opening Settings never removes a checkout or starts a maintenance pass.
class WorktreeSettings : public QWidget {
public:
    explicit WorktreeSettings(QString executable,QWidget *parent=nullptr):QWidget(parent),m_executable(std::move(executable)) {
        setObjectName("worktreeSettings");setMaximumWidth(850);
        auto *layout=new QVBoxLayout(this);layout->setContentsMargins(0,0,12,0);layout->setSpacing(18);
        auto *heading=new QLabel(tr("Worktrees"));heading->setObjectName("heading");layout->addWidget(heading);
        auto *scope=new QHBoxLayout;m_computer=new QComboBox;m_computer->setObjectName("worktreePolicyComputer");m_computer->addItem(tr("This computer"),QString());scope->addWidget(m_computer);
        m_refresh=new QPushButton(tr("Refresh"));scope->addWidget(m_refresh);scope->addStretch();layout->addLayout(scope);
        auto *hint=new QLabel(tr("Cleanup runs on the computer that owns each worktree, even when this window is closed. Rules are enabled by default and apply across its projects. Other computers keep their own settings."));hint->setWordWrap(true);layout->addWidget(hint);
        auto check=[&](const QString &text,const char *name){auto *c=new QCheckBox(text);c->setObjectName(name);layout->addWidget(c);return c;};
        m_enabled=check(tr("Enable automatic worktree cleanup"),"worktreeCleanupEnabled");
        m_after=check(tr("Clean integrated worktrees after the last session is archived or terminated"),"worktreeCleanupAfterSession");
        m_unused=check(tr("Periodically clean unused integrated worktrees"),"worktreeCleanupUnused");
        m_missing=check(tr("Forget verified managed worktrees whose folders no longer exist"),"worktreeCleanupMissing");
        auto *form=new QFormLayout;m_grace=new QSpinBox;m_grace->setObjectName("worktreeCleanupGrace");m_grace->setRange(1,8760);m_grace->setSuffix(tr(" hours"));form->addRow(tr("Minimum inactivity for periodic cleanup"),m_grace);layout->addLayout(form);
        auto *safety=new QLabel(tr("Only verified Zerus-created worktrees qualify. Cleanup keeps branches and conversation history. Changed, untracked and ignored local files, sessions, running programs, locked folders and incomplete checks protect a worktree. Build folders and local configuration require review in Projects > Worktrees."));safety->setWordWrap(true);layout->addWidget(safety);
        m_status=new QLabel(tr("Refresh to read this computer's cleanup rules."));m_status->setObjectName("worktreePolicyStatus");m_status->setTextFormat(Qt::PlainText);m_status->setWordWrap(true);layout->addWidget(m_status);
        m_apply=new QPushButton(tr("Apply to this computer"));m_apply->setObjectName("applyWorktreePolicy");layout->addWidget(m_apply,0,Qt::AlignLeft);layout->addStretch();
        connect(m_refresh,&QPushButton::clicked,this,[this]{refresh();});
        connect(m_computer,&QComboBox::currentIndexChanged,this,[this]{m_policy={};refresh();});
        connect(m_enabled,&QCheckBox::toggled,this,[this]{controls();});
        connect(m_unused,&QCheckBox::toggled,this,[this]{controls();});
        connect(m_apply,&QPushButton::clicked,this,[this]{
            auto p=m_policy;p["enabled"]=m_enabled->isChecked();p["after_session"]=m_after->isChecked();p["unused_integrated"]=m_unused->isChecked();p["missing"]=m_missing->isChecked();p["grace_hours"]=m_grace->value();request("set",p);
        });controls();
    }
    void setPeers(const QStringList &peers) {
        if(m_busy)return;
        const QString route=m_computer->currentData().toString();const QSignalBlocker block(m_computer);
        while(m_computer->count()>1)m_computer->removeItem(1);
        for(const auto &peer:peers)if(!peer.isEmpty()&&m_computer->findData(peer)<0&&m_computer->count()<33)m_computer->addItem(peer,peer);
        m_computer->setCurrentIndex(qMax(0,m_computer->findData(route)));
        if(m_computer->currentData().toString()!=route){m_policy={};controls();}
    }
    void refresh(){if(!m_busy)request("get",{});}
private:
    static bool success(const ProcessRunner::Result &r){return r.outcome==ProcessRunner::Result::Finished&&r.exitStatus==QProcess::NormalExit&&r.exitCode==0;}
    ProcessRunner::Request command(QStringList args)const {
        ProcessRunner::Request r;r.program=m_executable.isEmpty()?QStringLiteral("hgs"):m_executable;
        const auto route=m_computer->currentData().toString();if(!route.isEmpty())args.prepend('@'+route);
        r.arguments=args;r.input=QByteArray();r.timeoutMs=15000;r.maximumOutputBytes=128*1024;
        auto env=QProcessEnvironment::systemEnvironment();env.remove("HGS_SESSION");env.remove("HGS_RUN_ID");r.environment=env;return r;
    }
    void controls(){
        const bool ready=!m_busy&&!m_policy.isEmpty();m_computer->setEnabled(!m_busy);m_refresh->setEnabled(!m_busy);m_apply->setEnabled(ready);m_enabled->setEnabled(ready);
        for(auto *c:{m_after,m_unused,m_missing})c->setEnabled(ready&&m_enabled->isChecked());
        m_grace->setEnabled(ready&&m_enabled->isChecked()&&m_unused->isChecked());
    }
    void fail(const QString &text){m_busy=false;m_policy={};m_status->setText(text);controls();}
    void request(const QString &action,const QJsonObject &policy){
        if(m_busy)return;m_busy=true;m_status->setText(tr("Reading cleanup settings…"));controls();
        ProcessRunner::run(command({"--help"}),this,[this,action,policy](const ProcessRunner::Result &probe){
            if(!success(probe)||!probe.standardOutput.contains("worktree-maintenance-v1")){fail(tr("This computer is unavailable or needs an updated hgs CLI for cleanup settings."));return;}
            auto r=command({"worktrees","policy",action,"--json"});if(action=="set")r.input=QJsonDocument(policy).toJson(QJsonDocument::Compact);
            ProcessRunner::run(std::move(r),this,[](ProcessRunner::Result result){return std::make_pair(success(result),success(result)?JsonParse::object(result.standardOutput):QJsonObject());},
                               [this,action](const std::pair<bool,QJsonObject> &reply){
                if(!reply.first){fail(action=="set"?tr("Save was not confirmed. Refresh before changing settings again."):tr("Could not read cleanup settings."));return;}
                const auto &object=reply.second;const auto p=object.value("policy").toObject();
                bool valid=object.value("schema").toInt()==1&&p.value("version").toInt()==1&&p.value("revision").isDouble()&&p.value("grace_hours").toInt()>=1&&p.value("grace_hours").toInt()<=8760;
                for(const auto *key:{"enabled","after_session","unused_integrated","missing"})valid=valid&&p.value(QLatin1String(key)).isBool();
                if(!valid){fail(tr("Computer returned invalid cleanup settings."));return;}
                m_policy=p;m_busy=false;m_enabled->setChecked(p.value("enabled").toBool());m_after->setChecked(p.value("after_session").toBool());m_unused->setChecked(p.value("unused_integrated").toBool());m_missing->setChecked(p.value("missing").toBool());m_grace->setValue(p.value("grace_hours").toInt());
                m_status->setText(action=="set"?tr("Cleanup settings saved on the selected computer."):tr("Cleanup settings loaded. Uncertain removals stay for manual review."));controls();
            });
        });
    }
    QString m_executable;QJsonObject m_policy;bool m_busy=false;
    QComboBox *m_computer;QCheckBox *m_enabled,*m_after,*m_unused,*m_missing;QSpinBox *m_grace;QLabel *m_status;QPushButton *m_refresh,*m_apply;
};
