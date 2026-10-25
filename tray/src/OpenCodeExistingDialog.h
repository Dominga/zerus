#pragma once
#include <QDialog>
#include <QJsonObject>
#include "HgsClient.h"
class QLineEdit;
class QLabel;
class QPushButton;

// Explicit selection: opening the dialog is read-only and sends no request.
// A verified preview is required before one local Zerus binding may be written.
class OpenCodeExistingDialog : public QDialog {
public:
    OpenCodeExistingDialog(const QString &hgsPath, const QString &host,
                           const QString &agentHome, QWidget *parent = nullptr);
    QJsonObject bound() const { return m_bound; }
    void reject() override;
private:
    QJsonObject selection() const;
    void invalidate();
    void controls();
    void submit(bool commit);
    void receive(quint64 request, const QString &host, const QJsonObject &result);
    HgsClient m_client;
    QString m_host, m_home, m_identity, m_requestId;
    QLineEdit *m_nativeId, *m_directory, *m_name;
    QLabel *m_notice;
    QPushButton *m_preview, *m_bind, *m_cancel;
    quint64 m_pending = 0;
    bool m_committing = false;
    QJsonObject m_sent, m_bound;
};
