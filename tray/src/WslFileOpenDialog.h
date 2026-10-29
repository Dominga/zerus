#pragma once
#include <QDialog>
#include <QJsonObject>
#include "HgsClient.h"
#include "MappedLocalFile.h"
#include "ExternalFile.h"
#include <functional>
class QLabel;
class QLineEdit;
class QPushButton;
class QCheckBox;

// A separate explicit action. Neither constructing nor previewing this dialog
// opens an application, changes native state, or silently downloads a fallback.
class WslFileOpenDialog : public QDialog {
    Q_OBJECT
public:
    using Verify=std::function<MappedLocalFile::Result(const QString &,const QString &,qint64)>;
    using Open=std::function<ExternalFile::Outcome(const QString &)>;
    WslFileOpenDialog(const QString &hgsPath,const QString &host,const QJsonObject &scope,
                      QWidget *parent=nullptr,Verify verify={},Open open={});
    QString outcomeNotice()const{return m_outcome;}
    void reject()override;
private:
    void invalidate();
    void controls();
    QJsonObject selection()const;
    void submit(bool opening);
    void failure(const QString &message);
    void receive(const QJsonObject &proof);
    HgsClient m_client;
    QString m_host,m_machine,m_key,m_mappingId,m_requestId,m_token,m_outcome;
    QJsonObject m_scope,m_profile,m_sent,m_preview;
    QLineEdit *m_root,*m_mount,*m_workspace;
    QLabel *m_target,*m_notice;
    QCheckBox *m_remember;
    QPushButton *m_check,*m_open,*m_cancel;
    quint64 m_lookup=0,m_pending=0;
    bool m_localPending=false,m_opening=false,m_loaded=false;
    Verify m_verify;
    Open m_opener;
};
