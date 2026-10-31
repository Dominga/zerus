#pragma once
#include <QWidget>
#include <QHash>
#include <QJsonObject>
#include "FleetState.h"
class QComboBox; class IdentityBadge; class HgsClient; class QListWidget; class QLabel; class QLineEdit; class QSpinBox; class QCheckBox;
class QFormLayout; class QPushButton; class QPlainTextEdit; class QFrame;
namespace PhonePairing { class Panel; }
class MachinesPage : public QWidget {
    Q_OBJECT
public:
    explicit MachinesPage(const QString &hgsPath, QWidget *parent = nullptr);
    void setFleet(const FleetState &fleet);
    void reload();
    void setTheme(bool dark);
    void selectMachine(const QString &host);
signals:
    void machinesChanged();
    void viewSessionsRequested(const QString &host);
    void sshTerminalRequested(const QString &alias);
    void appearanceChanged();
    void accountsRequested(const QString &host);
protected:
    void showEvent(QShowEvent *) override;
private:
    void renderRows();
    // The amber dot beside the machine with unsaved edits (every row rebuild too).
    void markUnsaved();
    void loadSelection();
    void updateControls();
    void changed();
    void save();
    void check();
    void install();
    void enableLocalSessions();
    void report(const QString &text, bool error = false);
    QJsonObject connection() const;
    QString alias() const;
    bool aliasExists() const;
    bool discardChanges();
    void resolveConnection();
    void showEffectiveConnection(const QJsonObject &data);
    // #647: the header's label, system and state line.
    void updateHeader();
    // #650 N2: the overrides of the connection type not saved, kept on this
    // device per machine so switching the type back restores them.
    static QString otherTransportKey(const QString &machine);
    void rememberOtherTransport();
    QString hostFacts(const QJsonObject &host) const;
    void updateAppearance();
    QString appearanceMachine() const;
    HgsClient *m_client;
    FleetState m_fleet;
    QJsonObject m_data;
    QListWidget *m_list;
    QLabel *m_summary, *m_title, *m_status, *m_origin, *m_effective, *m_colorHint, *m_localHost;
    // U25 part 7 (spec 8): the title's note, the duplicate-name error and the
    // unsaved-changes warning; the label swatches and the Soft/Bright segment.
    QLabel *m_titleNote, *m_aliasError, *m_unsaved;
    QList<QPushButton *> m_swatches;
    QPushButton *m_styleSoft, *m_styleBright;
    bool m_dark = false;
    QLineEdit *m_alias, *m_hostname, *m_user, *m_key;
    QLineEdit *m_wslDistribution, *m_wslUser, *m_wslHgs;
    QComboBox *m_transport;
    QWidget *m_keyRow;
    QSpinBox *m_port;
    QCheckBox *m_enabled;
    // U62 (#733): the platform class the Machine runs test suites for; the
    // first item is none.
    QComboBox *m_testRunner;
    QLabel *m_testRunnerHint;
    QFormLayout *m_form;
    QPushButton *m_save, *m_check, *m_ssh, *m_install, *m_remove, *m_reset, *m_sessions, *m_accounts, *m_add, *m_browse, *m_refresh;
    QPushButton *m_discardDraft, *m_enableLocal;
    QPushButton *m_color, *m_colorReset;
    QComboBox *m_colorStyle;
    IdentityBadge *m_colorPreview;
    QPlainTextEdit *m_log;
    // #647 (prototype, Machines): the list column, the header's state line,
    // a draft's hint, this computer's line and card, the SSH card, Test
    // session host and the Phones panel; the last check of each machine.
    QFrame *m_sidebar, *m_localCard, *m_connectionCard;
    QLabel *m_stateLine, *m_hint, *m_localLine;
    // #650 L4: the HGS_PEERS override and the alias's source, on the page.
    QLabel *m_peersNote, *m_sourceNote;
    QPushButton *m_testHost;
    PhonePairing::Panel *m_phones;
    quint64 m_hostTest = 0;
    QHash<QString, QJsonObject> m_checks;
    QString m_selected = "@local", m_operation, m_pendingSelection;
    quint64 m_request = 0, m_resolveRequest = 0, m_hostRequest = 0;
    QString m_resolveAlias;
    // Shown for this computer once it enabled local sessions: the stable CLI
    // follows from the next start (spec D2).
    QString m_localNotice;
    QJsonObject m_effectiveData;
    bool m_loading = false, m_dirty = false, m_new = false, m_installing = false;
    bool m_discardPrompt = false;
    bool m_rowsStale = false;
};
