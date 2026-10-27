#pragma once
#include "WorktreeBatchCleanupDialog.h"
#include <QFrame>
#include <QHBoxLayout>
#include <QHash>
#include <QSet>
#include <utility>

// Bounded desktop-owned background work. Only an explicit action opens review;
// termination itself never opens another modal confirmation.
class WorktreeCleanupStatus : public QFrame {
public:
    explicit WorktreeCleanupStatus(HgsClient *client,QWidget *parent=nullptr):QFrame(parent),m_client(client){
        setObjectName("terminationCleanupStatus");
        auto *layout=new QVBoxLayout(this);layout->setContentsMargins(12,8,12,8);
        m_label=new QLabel;m_label->setObjectName("terminationCleanupSummary");m_label->setWordWrap(true);m_label->setTextFormat(Qt::PlainText);layout->addWidget(m_label,1);
        auto *actions=new QHBoxLayout;actions->addStretch();layout->addLayout(actions);
        m_check=new QPushButton(tr("Check again"));m_check->setObjectName("recheckTerminationCleanup");actions->addWidget(m_check);
        m_files=new QPushButton(tr("View files"));m_files->setObjectName("inspectTerminationCleanup");actions->addWidget(m_files);
        auto *details=new QPushButton(tr("Details"));details->setObjectName("reviewTerminationCleanup");actions->addWidget(details);
        m_keep=new QPushButton(tr("Keep"));m_keep->setObjectName("dismissTerminationCleanup");actions->addWidget(m_keep);
        for(auto *button:{m_check,m_files,details,m_keep}){button->setAutoDefault(false);button->setAttribute(Qt::WA_LayoutUsesWidgetRect);}
        connect(m_check,&QPushButton::clicked,this,[this]{if(auto *dialog=reviewTarget()){dialog->reviewRemaining();showReview(dialog);}});
        connect(details,&QPushButton::clicked,this,[this]{if(auto *dialog=reviewTarget())showReview(dialog);});
        connect(m_files,&QPushButton::clicked,this,[this]{if(auto *dialog=reviewTarget()){
            auto *list=dialog->findChild<QTreeWidget *>("batchCleanupList");
            for(int i=0;i<list->topLevelItemCount();++i){auto *item=list->topLevelItem(i);const auto entry=item->data(0,Qt::UserRole).toJsonObject();
                if(entry["cleanup_status"]!="removed" && !entry["common_dir"].toString().isEmpty()){list->setCurrentItem(item);dialog->inspectCurrent();return;}}
            showReview(dialog);
        }});
        connect(m_keep,&QPushButton::clicked,this,[this]{if(anyBusy())return;qDeleteAll(m_operations);m_operations.clear();m_published.clear();m_overflow=0;hide();});
        hide();
    }
    std::function<void(const QList<QJsonObject> &)> foldersRemoved;
    void add(const QList<QJsonObject> &entries){
        // Retain failed/uncertain outcomes until dismissed; successful old jobs
        // need no recovery surface. Bound the combined inventory to 64 targets.
        for(auto it=m_operations.begin();it!=m_operations.end();){auto *dialog=*it;
            if(!dialog->busy() && dialog->count("removed")==dialog->targetCount()){m_published.remove(dialog);delete dialog;it=m_operations.erase(it);}else ++it;}
        int total=m_pending.size();QSet<QString> known;
        for(const auto &entry:m_pending)known.insert(key(entry));
        for(auto *dialog:m_operations){total+=dialog->targetCount();auto *list=dialog->findChild<QTreeWidget *>("batchCleanupList");
            for(int i=0;i<list->topLevelItemCount();++i){const auto e=list->topLevelItem(i)->data(0,Qt::UserRole).toJsonObject();known.insert(key(e));}}
        QList<QJsonObject> accepted;
        for(const auto &entry:entries){const auto identity=key(entry);if(known.contains(identity))continue;known.insert(identity);
            if(total+accepted.size()>=64){++m_overflow;continue;}accepted<<entry;}
        m_pending.append(accepted);startNext();render();
    }
private:
    static QString key(const QJsonObject &e){return e["host"].toString()+'\n'+e["common_dir"].toString()+'\n'+e["row"].toObject()["path"].toString();}
    bool anyBusy() const {return !m_pending.isEmpty() || std::any_of(m_operations.cbegin(),m_operations.cend(),[](auto *d){return d->busy();});}
    void startNext(){
        if(m_pending.isEmpty() || std::any_of(m_operations.cbegin(),m_operations.cend(),[](auto *d){return d->busy();}))return;
        const auto entries=std::exchange(m_pending,{});
        auto *dialog=new WorktreeBatchCleanupDialog(m_client,entries,this,true);m_operations<<dialog;
        dialog->stateChanged=[this,dialog]{
            const auto removed=dialog->removedEntries();const int published=m_published.value(dialog);
            if(!dialog->busy() && removed.size()>published){m_published[dialog]=removed.size();if(foldersRemoved)foldersRemoved(removed.mid(published));}
            startNext();render();
        };
    }
    WorktreeBatchCleanupDialog *reviewTarget() const {
        for(auto *dialog:m_operations)if(!dialog->busy() && dialog->canRefresh())return dialog;
        for(auto *dialog:m_operations)if(!dialog->busy() && dialog->count("unconfirmed"))return dialog;
        return m_operations.isEmpty()?nullptr:m_operations.last();
    }
    static void showReview(WorktreeBatchCleanupDialog *dialog){dialog->setWindowModality(Qt::NonModal);dialog->show();dialog->raise();dialog->activateWindow();}
    void render(){
        int removed=0,kept=0,unknown=0;
        for(auto *dialog:m_operations){removed+=dialog->count("removed");kept+=dialog->count("kept");unknown+=dialog->count("unconfirmed");}
        const bool busy=anyBusy();
        const auto targetLabel=m_operations.size()==1?m_operations[0]->targetLabel():QString();
        QString text=busy?tr("Cleaning working folders…"):kept==1 && !unknown && !removed && !targetLabel.isEmpty()?tr("Folder kept: %1.").arg(targetLabel)
            :removed==1 && !kept && !unknown && !targetLabel.isEmpty()?tr("Folder removed: %1.").arg(targetLabel)
            :tr("Working folders: %1 removed  %2 kept  %3 unknown").arg(removed).arg(kept).arg(unknown);
        if(!busy && kept)for(auto *dialog:m_operations)if(!dialog->retainedReason().isEmpty()){text+=' '+dialog->retainedReason();break;}
        if(!busy && unknown)text+=' '+tr("An unknown result will not be retried.");
        if(m_overflow)text+=' '+tr("%1 additional folders need review in Projects > Worktrees.").arg(m_overflow);
        m_label->setText(text);
        auto *target=reviewTarget();m_check->setVisible(kept>0);m_check->setEnabled(target&&!target->busy()&&target->canRefresh());
        m_files->setVisible(kept>0||unknown>0);m_files->setEnabled(target&&!target->busy());
        m_keep->setText(kept||unknown?tr("Keep"):tr("Dismiss"));m_keep->setEnabled(!busy);
        setVisible(!m_operations.isEmpty()||m_overflow);
    }
    HgsClient *m_client;QList<QJsonObject> m_pending;QList<WorktreeBatchCleanupDialog *> m_operations;QHash<WorktreeBatchCleanupDialog *,int> m_published;
    QLabel *m_label;QPushButton *m_check,*m_files,*m_keep;int m_overflow=0;
};
