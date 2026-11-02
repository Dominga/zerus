#pragma once
#include "PermissionCatalog.h"
#include <QDialog>
#include <QMap>
#include <QString>

class QComboBox;
class QLabel;
class QPushButton;

// The shared native permission editor: one selector per discovered control,
// unknown saved settings kept visible until explicitly removed, and Save
// disabled while the current map cannot be sent to the backend.
class PermissionCatalogDialog : public QDialog {
public:
    PermissionCatalogDialog(const PermissionCatalog::Catalog &catalog,
                            const QMap<QString, QString> &selection,
                            const QString &subtitle,
                            QWidget *parent = nullptr);
    QMap<QString, QString> selection() const { return m_selection; }
    QString mode() const;
private:
    QWidget *addRow(const PermissionCatalog::EditorRow &row);
    void validate();
    void removeUnknown(const QString &id);

    PermissionCatalog::Catalog m_catalog;
    QMap<QString, QString> m_selection;
    QLabel *m_warning = nullptr;
    QPushButton *m_save = nullptr;
    struct UnknownRow { QString id; QWidget *widget = nullptr; QPushButton *remove = nullptr; };
    QList<UnknownRow> m_unknown;
};
