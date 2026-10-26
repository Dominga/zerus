#pragma once
#include "HgsClient.h"
#include "WorktreeReviewUi.h"
#include "WorktreeInspectionPanel.h"
#include <QCheckBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFileInfo>
#include <QHeaderView>
#include <QLabel>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QTreeWidget>
#include <QVBoxLayout>
#include <QTimer>
#include <algorithm>
#include <functional>

// One exact host/common-dir/path snapshot per target. A definite refusal can be
// checked again; an attempted removal with an unknown result is never replayed.
class WorktreeBatchCleanupDialog : public QDialog {
public:
    WorktreeBatchCleanupDialog(HgsClient *client, const QList<QJsonObject> &entries,
                              QWidget *parent = nullptr, bool background = false)
        : QDialog(parent), m_client(client), m_entries(entries), m_background(background) {
        setObjectName("worktreeBatchCleanupDialog"); setWindowTitle(tr("Remove working folders"));
        resize(760, 420); setMinimumSize(560, 320);
        auto *layout = new QVBoxLayout(this); layout->setContentsMargins(20,18,20,18); layout->setSpacing(10);
        auto *heading = new QLabel(tr("Review %1 selected worktrees").arg(entries.size())); heading->setObjectName("heading"); layout->addWidget(heading);
        const bool exitCleanup=std::any_of(entries.cbegin(),entries.cend(),[](const QJsonObject &entry){return entry["exit_cleanup"]==true;});
        auto *hint = new QLabel(exitCleanup?tr("Conversations and branches are kept. Removing a working folder permanently deletes its local files."):tr("Conversations are kept. Branches stay unless you choose to delete them. Local files in removed folders are permanently deleted."));
        hint->setWordWrap(true); layout->addWidget(hint);
        m_list = new QTreeWidget; m_list->setObjectName("batchCleanupList");
        m_list->setHeaderLabels({tr("Folder / branch"), tr("Computer"), tr("Size"), tr("Result")});
        m_list->setRootIsDecorated(false); m_list->header()->setSectionResizeMode(0, QHeaderView::Stretch); layout->addWidget(m_list, 1);
        auto *actions = new QHBoxLayout;
        m_refresh = new QPushButton(tr("Check again")); m_refresh->setObjectName("refreshCleanupReview"); m_refresh->setAutoDefault(false); actions->addWidget(m_refresh);
        m_inspect = new QPushButton(tr("View files and commits…")); m_inspect->setObjectName("inspectCleanupWorktree"); m_inspect->setAutoDefault(false); actions->addWidget(m_inspect);
        auto *detailsButton = new QPushButton(tr("Details")); detailsButton->setObjectName("toggleCleanupDetails"); detailsButton->setCheckable(true); detailsButton->setAutoDefault(false); actions->addWidget(detailsButton); actions->addStretch(); layout->addLayout(actions);
        m_details = new QPlainTextEdit; m_details->setObjectName("worktreeCleanupDecision"); m_details->setReadOnly(true); m_details->setMaximumHeight(180); m_details->hide(); layout->addWidget(m_details);
        connect(detailsButton, &QPushButton::toggled, m_details, &QWidget::setVisible);
        connect(m_refresh, &QPushButton::clicked, this, [this] { reviewRemaining(); });
        connect(m_inspect, &QPushButton::clicked, this, [this] { inspectCurrent(); });
        for (const auto &entry : entries) {
            const auto row = entry["row"].toObject();
            auto *item = new QTreeWidgetItem(m_list, {folder(row), entry["machine"].toString(), WorktreeReviewUi::size(row), tr("Checking…")});
            item->setData(0, Qt::UserRole, entry); item->setToolTip(0, row["path"].toString());
            if(entry["cleanup_status"]=="removed"){m_changed=true;m_removedEntries<<entry;}
        }
        m_local = new QCheckBox(tr("Delete the local files outside Git too")); m_local->setObjectName("confirmIgnoredWorktreeFiles"); m_local->hide(); layout->addWidget(m_local);
        m_discard = new QCheckBox(tr("Permanently discard uncommitted and untracked files")); m_discard->setObjectName("confirmDiscardWorktreeChanges"); m_discard->hide(); layout->addWidget(m_discard);
        m_processes = new QCheckBox(tr("Remove even if some running programs cannot be checked")); m_processes->setObjectName("confirmUncheckedWorktreeProcesses");
        m_processes->setToolTip(tr("Accept this uncertainty for this folder for five minutes. Known programs and sessions using the folder still prevent removal."));
        m_processes->hide(); layout->addWidget(m_processes);
        m_branches = new QCheckBox(tr("Also delete branches proven merged by Git")); m_branches->setObjectName("batchDeleteMergedBranches"); layout->addWidget(m_branches);
        if(exitCleanup)m_branches->hide();
        m_status = new QLabel; m_status->setTextFormat(Qt::PlainText); m_status->setObjectName("batchCleanupStatus"); m_status->setWordWrap(true); layout->addWidget(m_status);
        auto *buttons = new QDialogButtonBox;
        m_cancel = buttons->addButton(QDialogButtonBox::Cancel); m_cancel->setText(tr("Keep folders"));
        m_remove = buttons->addButton(tr("Remove folders"), QDialogButtonBox::DestructiveRole);
        m_remove->setObjectName("confirmBatchWorktreeRemoval"); m_remove->setAutoDefault(false); m_cancel->setDefault(true); layout->addWidget(buttons);
        connect(m_cancel, &QPushButton::clicked, this, [this] {
            if(m_background && busy()) { hide(); return; }
            if(m_request) { m_stop=true; m_cancel->setEnabled(false); m_status->setText(tr("Finishing the current folder; remaining folders will be kept.")); }
            else reject();
        });
        for(auto *box:{m_local,m_discard,m_processes})connect(box,&QCheckBox::toggled,this,[this]{updateAction();});
        connect(m_list,&QTreeWidget::currentItemChanged,this,[this]{details();});
        connect(m_remove,&QPushButton::clicked,this,[this]{if(m_remove->isEnabled())startRemoval(false);});
        connect(client,&HgsClient::worktreeRemoved,this,[this](quint64 request,bool ok,const QJsonObject &result,const QString &error){
            if(request!=m_request)return; m_request=0;
            auto &entry=m_entries[m_index];
            entry["cleanup_status"]=ok?QStringLiteral("removed"):result["status"]=="kept"?QStringLiteral("kept"):QStringLiteral("unconfirmed");
            entry["cleanup_detail"]=ok?result["branch_error"].toString():error;
            if(ok){entry["forgotten"]=result["status"]=="forgotten";m_changed=true;m_removedEntries<<entry;}
            else entry["verified"]=false; // A new review is required after a definite refusal.
            publishRow(m_index);++m_index;next();
        });
        connect(client,&HgsClient::worktreeReviewReady,this,[this](quint64 request,const QString &host,const QString &path,const QJsonObject &data){
            if(request!=m_reviewRequest)return;
            auto &entry=m_entries[m_reviewIndex]; const auto target=entry["row"].toObject()["path"].toString();
            QJsonObject verified; int matches=0;
            if(host==entry["host"].toString() && path==entry["common_dir"].toString() && data["state"]=="ok" && data["common_dir"]==entry["common_dir"])
                for(const auto &value:data["worktrees"].toArray())if(value.toObject()["path"]==target){verified=value.toObject();++matches;}
            if(matches==1){entry["row"]=verified;entry["verified"]=true;}
            else entry["review_error"]=tr("Could not verify this folder. Check again after reconnecting.");
            finishReview();
        });
        connect(client,&HgsClient::worktreeReviewFailed,this,[this](quint64 request,const QString &,const QString &,const QString &error){
            if(request!=m_reviewRequest)return;m_entries[m_reviewIndex]["review_error"]=error;finishReview();
        });
        for(int i=0;i<m_entries.size();++i)if(terminal(m_entries[i]))publishRow(i);
        if(m_list->topLevelItemCount())m_list->setCurrentItem(m_list->topLevelItem(0));
        updateAction();QTimer::singleShot(0,this,[this]{reviewNext();});
    }
    std::function<void()> stateChanged;
    bool changed() const {return m_changed;}
    bool attempted() const {return m_attempted;}
    bool busy() const {return m_running || m_autoPending || m_reviewIndex<m_entries.size();}
    const QList<QJsonObject> &removedEntries() const {return m_removedEntries;}
    int targetCount() const {return m_entries.size();}
    QString targetLabel() const {return m_entries.size()==1?QFileInfo(m_entries[0]["row"].toObject()["path"].toString()).fileName()+tr(" on %1").arg(m_entries[0]["machine"].toString()):QString();}
    int count(const QString &status) const {return std::count_if(m_entries.cbegin(),m_entries.cend(),[&](const QJsonObject &e){return e["cleanup_status"]==status;});}
    bool canRefresh() const {return std::any_of(m_entries.cbegin(),m_entries.cend(),[](const QJsonObject &e){return !terminal(e);});}
    QString summary() const {return tr("%1 removed  %2 kept  %3 unknown").arg(count("removed")).arg(count("kept")).arg(count("unconfirmed"));}
    QString retainedReason() const {
        for(const auto &entry:m_entries)if(entry["cleanup_status"]=="kept"){
            if(!entry["cleanup_detail"].toString().isEmpty())return entry["cleanup_detail"].toString().section('\n',0,0).left(180);
            if(!entry["review_error"].toString().isEmpty())return entry["review_error"].toString().section('\n',0,0).left(180);
            const auto row=entry["row"].toObject();
            if(WorktreeReviewUi::hasReason(row,{"processes"}))return tr("A running program uses the folder.");
            if(WorktreeReviewUi::hasReason(row,{"sessions"}))return tr("Another session uses the folder.");
            if(WorktreeReviewUi::hasReason(row,{"processes_unavailable"}))return tr("Some running programs could not be checked.");
            if(discardable(row))return tr("Uncommitted changes need review.");
            if(WorktreeReviewUi::localData(row))return tr("Local files need review.");
            if(entry["expected_owner_id"].toString().isEmpty() || entry["expected_owner_id"]!=row["ownership_id"])return tr("Checkout ownership needs review.");
            return WorktreeReviewUi::summary(row).left(180);
        }
        return {};
    }
    void reviewRemaining() {
        if(busy() || !canRefresh())return;
        m_autoScheduled=true; m_manualReview=true; m_stop=false; m_index=0;m_reviewIndex=0;
        for(auto &entry:m_entries)if(!terminal(entry)){entry["verified"]=false;entry.remove("review_error");entry.remove("cleanup_detail");entry.remove("cleanup_status");}
        for(auto *box:{m_local,m_discard,m_processes,m_branches})box->setChecked(false);
        updateAction();reviewNext();
    }
    void inspectCurrent() {
        const auto *item=m_list->currentItem();if(!item)return;
        const auto entry=item->data(0,Qt::UserRole).toJsonObject(),row=entry["row"].toObject();
        if(entry["offline"]==true || row["verdict"]=="missing" || entry["cleanup_status"]=="removed" || entry["common_dir"].toString().isEmpty() || row["path"].toString().isEmpty())return;
        m_client->invalidateWorktreeInspection(entry["host"].toString(),row["path"].toString(),entry["common_dir"].toString());
        QDialog dialog(this);dialog.setObjectName("cleanupFileInspection");dialog.setWindowTitle(tr("Inspect working folder"));dialog.resize(900,680);
        auto *layout=new QVBoxLayout(&dialog);auto *panel=new WorktreeInspectionPanel(m_client);layout->addWidget(panel);
        panel->setContext(entry["host"].toString(),entry["machine"].toString(),row["path"].toString(),entry["common_dir"].toString(),row,{},true);
        if(WorktreeReviewUi::localData(row))panel->findChild<QTabWidget *>("worktreeInspectionTabs")->setCurrentIndex(2);
        auto *buttons=new QDialogButtonBox(QDialogButtonBox::Close);layout->addWidget(buttons);connect(buttons,&QDialogButtonBox::rejected,&dialog,&QDialog::reject);dialog.exec();
    }
    void reject() override {if(m_background){hide();return;}if(!busy())QDialog::reject();else if(!m_request){m_stop=true;QDialog::reject();}}
private:
    static bool terminal(const QJsonObject &entry){return entry["cleanup_status"]=="removed" || entry["cleanup_status"]=="unconfirmed";}
    static QString folder(const QJsonObject &row){const auto branch=row["branch"].toString();return QFileInfo(row["path"].toString()).fileName()+(branch.isEmpty()?QString():" / "+branch);}
    static bool discardable(const QJsonObject &row){return row["discardable"]==true && !row["discard_fingerprint"].toString().isEmpty();}
    bool eligible(const QJsonObject &entry) const {
        const auto row=entry["row"].toObject();
        return !terminal(entry) && entry["verified"]==true && (WorktreeReviewUi::removable(row)||discardable(row)||WorktreeReviewUi::processReviewable(row));
    }
    bool automaticReady(const QJsonObject &entry) const {
        return eligible(entry) && entry["exit_cleanup"]==true && !entry["expected_owner_id"].toString().isEmpty()
            && entry["expected_owner_id"]==entry["row"].toObject()["ownership_id"] && WorktreeReviewUi::exitCleanupReady(entry["row"].toObject());
    }
    void notify(){if(stateChanged)stateChanged();}
    void updateAction(){
        int count=0;m_requiresLocal=false;m_requiresDiscard=false;m_requiresProcesses=false;
        for(const auto &entry:m_entries)if(eligible(entry)){++count;const auto row=entry["row"].toObject();m_requiresLocal|=WorktreeReviewUi::localData(row);m_requiresDiscard|=discardable(row);m_requiresProcesses|=WorktreeReviewUi::processReviewable(row);}
        m_local->setVisible(m_requiresLocal);m_discard->setVisible(m_requiresDiscard);m_processes->setVisible(m_requiresProcesses);
        for(auto *box:{m_local,m_discard,m_processes,m_branches})box->setEnabled(!busy());
        m_remove->setText(count==1?tr("Remove folder"):tr("Remove %1 folders").arg(count));
        m_remove->setEnabled(!busy() && count>0 && (!m_requiresLocal||m_local->isChecked()) && (!m_requiresDiscard||m_discard->isChecked()) && (!m_requiresProcesses||m_processes->isChecked()));
        m_refresh->setEnabled(!busy()&&canRefresh());
        if(!busy())m_status->setText(!m_manualReview&&(m_attempted||m_background)?summary():!count?tr("Folders are kept. Select a folder for its reason."):m_requiresProcesses?tr("Some programs cannot be checked. Review this uncertainty before removing the folder."):m_requiresLocal?tr("Inspect the local files before confirming their deletion."):tr("Review the selected folders, then choose Remove or Keep folders."));
        details();
    }
    void reviewNext(){
        while(m_reviewIndex<m_entries.size()){
            auto &entry=m_entries[m_reviewIndex];
            if(terminal(entry)){++m_reviewIndex;continue;}
            entry["verified"]=false;
            if(entry["offline"]==true||entry["common_dir"].toString().isEmpty()){entry["review_error"]=tr("Reconnect this computer to check the folder.");finishReview();return;}
            m_status->setText(tr("Checking folder %1 of %2…").arg(m_reviewIndex+1).arg(m_entries.size()));notify();
            m_reviewRequest=m_client->reviewWorktrees(entry["host"].toString(),entry["common_dir"].toString(),entry["row"].toObject()["path"].toString());return;
        }
        const bool automatic=!m_autoScheduled && (m_background || (!m_entries.isEmpty() && std::all_of(m_entries.cbegin(),m_entries.cend(),[this](const QJsonObject &e){return automaticReady(e);})));
        if(automatic){m_autoScheduled=true;m_autoPending=true;QTimer::singleShot(0,this,[this]{m_autoPending=false;if(!m_stop && !busy() && (m_background||isVisible()))startRemoval(true);else {updateAction();notify();}});}
        updateAction();notify();
    }
    void finishReview(){
        m_reviewRequest=0;auto &entry=m_entries[m_reviewIndex];
        if(!eligible(entry))entry["cleanup_status"]="kept";
        publishRow(m_reviewIndex);++m_reviewIndex;details();reviewNext();
    }
    void publishRow(int index){
        const auto entry=m_entries[index],row=entry["row"].toObject();auto *item=m_list->topLevelItem(index);
        item->setData(0,Qt::UserRole,entry);item->setText(0,folder(row));item->setText(2,WorktreeReviewUi::size(row));
        const auto state=entry["cleanup_status"].toString();
        item->setText(3,state=="removed"?(entry["forgotten"]==true?tr("Record forgotten"):tr("Folder removed")):state=="unconfirmed"?tr("Result unknown"):state=="kept"?tr("Folder kept"):WorktreeReviewUi::processReviewable(row)?tr("Programs not checked"):discardable(row)?tr("Unsaved work"):WorktreeReviewUi::localData(row)?tr("Review local files"):tr("Ready"));
        item->setToolTip(3,entry["cleanup_detail"].toString(entry["review_error"].toString(WorktreeReviewUi::summary(row))));
    }
    void details(){
        const auto *item=m_list->currentItem();if(!item)return;
        const auto entry=item->data(0,Qt::UserRole).toJsonObject(),row=entry["row"].toObject();
        QStringList lines{entry["machine"].toString()+": "+row["path"].toString()};
        if(!entry["cleanup_reason"].toString().isEmpty())lines<<tr("Why the folder stayed: %1").arg(entry["cleanup_reason"].toString());
        if(!entry["cleanup_detail"].toString().isEmpty())lines<<entry["cleanup_detail"].toString();
        if(row["fingerprint"].isString())lines<<WorktreeReviewUi::decision(row);else lines<<entry["review_error"].toString(tr("Checking this folder…"));
        lines<<tr("After removal: conversations remain. Restoring an archived session may require recreating its working folder.");
        if(entry["cleanup_status"]=="unconfirmed")lines<<tr("The result is unknown. This operation will not be repeated. Inspect the folder before deciding what to do next.");
        for(const auto &value:row["changed_files"].toArray())lines<<tr("Discard: %1").arg(value.toString());
        for(const auto &value:row["ignored"].toArray()){const auto file=value.toObject();lines<<tr("Delete local data: %1 (%2)").arg(file["path"].toString(),QLocale().formattedDataSize(qint64(file["bytes"].toDouble())));}
        m_details->setPlainText(lines.join('\n'));
        m_inspect->setEnabled(!busy() && entry["offline"]!=true && entry["cleanup_status"]!="removed" && row["verdict"]!="missing" && !entry["common_dir"].toString().isEmpty() && !row["path"].toString().isEmpty());
    }
    void startRemoval(bool automatic){m_running=true;m_manualReview=false;m_attempted=true;m_automatic=automatic;m_index=0;m_cancel->setText(m_background?tr("Hide"):tr("Stop after this folder"));updateAction();notify();next();}
    void next(){
        while(m_index<m_entries.size()){
            auto &entry=m_entries[m_index];
            if(terminal(entry)){++m_index;continue;}
            if(m_stop || !eligible(entry) || (m_automatic&&!automaticReady(entry))){entry["cleanup_status"]="kept";if(m_stop)entry["verified"]=false;publishRow(m_index++);continue;}
            const auto row=entry["row"].toObject();
            m_list->topLevelItem(m_index)->setText(3,tr("Removing…"));m_status->setText(tr("Removing folder %1 of %2…").arg(m_index+1).arg(m_entries.size()));notify();
            m_request=m_client->removeWorktree(entry["host"].toString(),row["path"].toString(),entry["common_dir"].toString(),row["fingerprint"].toString(),
                !m_automatic&&m_branches->isChecked()&&row["merged"]==true,
                (m_automatic||m_local->isChecked())&&WorktreeReviewUi::localData(row)?row["ignored_fingerprint"].toString():QString(),
                !m_automatic&&m_discard->isChecked()?row["discard_fingerprint"].toString():QString(),
                !m_automatic&&m_processes->isChecked()&&WorktreeReviewUi::processReviewable(row)?row["process_fingerprint"].toString():QString(),
                entry["exit_cleanup"]==true&&row["archive_restore_ready"]==true);return;
        }
        m_running=false;m_automatic=false;m_cancel->setText(tr("Close"));m_cancel->setEnabled(true);updateAction();notify();
        if(count("removed")==m_entries.size())accept();
    }
    HgsClient *m_client;QList<QJsonObject> m_entries,m_removedEntries;QTreeWidget *m_list;QPlainTextEdit *m_details;
    QCheckBox *m_local,*m_discard,*m_processes,*m_branches;QLabel *m_status;QPushButton *m_cancel,*m_remove,*m_inspect,*m_refresh;
    quint64 m_request=0,m_reviewRequest=0;int m_reviewIndex=0,m_index=0;
    bool m_background=false,m_autoPending=false,m_running=false,m_attempted=false,m_changed=false,m_stop=false,m_autoScheduled=false,m_automatic=false,m_manualReview=false;
    bool m_requiresLocal=false,m_requiresDiscard=false,m_requiresProcesses=false;
};
