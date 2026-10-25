#pragma once
#include <QDialog>
#include <QJsonObject>
#include "HgsClient.h"
class QLabel;
class QLineEdit;
class QPushButton;
class QCheckBox;

// Operator-selected lexical alias. Preview never grants filesystem access;
// the enclosing directory picker must still read the target native directory.
class WslPathMappingDialog : public QDialog {
    Q_OBJECT
public:
    WslPathMappingDialog(const QString &hgsPath, const QString &host,
                         const QString &machineId, const QJsonObject &profile,
                         QWidget *parent = nullptr);
    QString mappedPath() const { return m_mapped["native_path"].toString(); }
    void reject() override;
private:
    QJsonObject mapping() const;
    void invalidate(bool changedMapping);
    void controls();
    void preview();
    void fail(const QString &message);
    HgsClient m_client;
    QString m_host, m_machine, m_key, m_mappingId, m_environment;
    QJsonObject m_profile, m_mapping, m_input, m_mapped;
    QLineEdit *m_source, *m_root, *m_mount, *m_workspace;
    QLabel *m_notice;
    QCheckBox *m_remember;
    QPushButton *m_preview, *m_use, *m_cancel;
    quint64 m_pending = 0, m_profileRequest = 0;
    bool m_mappingPhase = false;
};
