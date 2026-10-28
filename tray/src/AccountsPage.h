#pragma once
#include <functional>
#include <QWidget>
#include <QHash>
#include <QJsonObject>
#include <QSet>
#include "FleetState.h"
class QComboBox; class QListWidget; class QLabel; class QPushButton; class QLayout; class QStackedLayout;
class QTreeWidget;
class BusyIndicator;
class IdentityBadge;
class AccountUsageStore;
namespace AccountUsage {class Panel; class RefreshButton;}
class AccountsPage : public QWidget {
    Q_OBJECT
public:
    explicit AccountsPage(const QString &hgsPath, QWidget *parent = nullptr, AccountUsageStore *usage = nullptr);
    void setFleet(const FleetState &fleet);
    void showMachine(const QString &host);
    void showAccount(const QString &host, const QString &id);
    void reload();
    void ensureCatalogs(bool force = false);
    QJsonArray profiles(bool includeRemoved = false) const;
    QJsonArray catalogProfiles() const;
    void setTheme(bool dark);
    void refreshAppearance();
    // U25 part 7 (spec 9): the teams whose members use a profile, for "Used
    // by teams": one line per team, its name and the roles.
    struct TeamUse { QString team, name, roles; };
    void setTeamUses(std::function<QList<TeamUse>(const QJsonObject &profile)> uses) { m_teamUses = std::move(uses); selectionChanged(); }
signals:
    void openTeamRequested(const QString &team);
    void loginRequested(const QString &host, const QString &id);
    void installRequested(const QString &host, const QString &provider);
    void accountsChanged();
    void catalogChanged();
    void notice(const QString &message, bool error = false);
protected:
    void showEvent(QShowEvent *) override;
private:
    void render();
    // U22/N17: the session-log upload warning a DeepSeek account's sessions
    // carry on this machine, empty when none does.
    QString deepSeekUploadNote(const QString &host, const QString &home, const QString &provider) const;
    void renderTeams(const QJsonObject &account);
    void selectionChanged();
    bool changing() const;
    void add();
    void copy(const QString &target = QStringLiteral("*"));
    void setupOllama(const QString &host, const QString &accountLabel);
    void setupDeepSeek(const QString &host, const QString &label = {});
    void updateRefreshState();
    int pendingAccounts() const;
    void remove();
    void useSavedProfile(const QJsonObject &profile);
    QJsonArray savedProfiles(const QString &host, const QString &provider) const;
    QJsonArray savedForAccount(const QString &host) const;
    void rename();
    void permissions();
    void modelDefaults();
    void editModelDefaults(const QJsonObject &profile, const QJsonObject &data);
    void inspectSelected(bool refresh = false);
    void inspectProfiles(bool refresh = false);
    void renameNext();
    void request(const QString &host, const QStringList &args);
    void requestQueuedCatalog(const QString &host);
    QJsonObject m_modelProfile;
    QJsonObject selected() const;
    QJsonObject selectedAccount() const;
    bool online(const QString &host) const;
    void renderMachines();
    HgsClient m_client;
    FleetState m_fleet;
    QComboBox *m_machine, *m_profileChoice;
    // U25 part 7 (spec 9): All / Signed in N / Signed out N, the empty
    // result with Clear filters, and the Used by teams card.
    QString m_signFilter = QStringLiteral("all");
    class QPushButton *m_signAll = nullptr, *m_signIn = nullptr, *m_signOut = nullptr, *m_clearFilters = nullptr;
    QWidget *m_empty = nullptr;
    class QFrame *m_teamsCard = nullptr;
    class QVBoxLayout *m_teamsList = nullptr;
    int m_totalAccounts = 0;
    std::function<QList<TeamUse>(const QJsonObject &)> m_teamUses;
    QListWidget *m_list;
    QStackedLayout *m_bodyStack;
    BusyIndicator *m_loadingSpinner, *m_summarySpinner;
    QJsonArray m_displayProfiles;
    bool m_hasSnapshot = false;
    IdentityBadge *m_providerBadge;
    QLabel *m_summary, *m_title, *m_detail, *m_identity, *m_permissionState, *m_modelState;
    QPushButton *m_add, *m_login, *m_copy, *m_remove, *m_rename, *m_install, *m_permissions, *m_default, *m_startOpenCode, *m_bindOpenCode, *m_modelDefaults;
    // OpenCode's connected providers and their notes, read-only text from that machine's catalog.
    QTreeWidget *m_openCodeProviders;
    QLabel *m_openCodeProvidersNote;
    AccountUsage::RefreshButton *m_refresh;
    QLayout *m_machineTags;
    QWidget *m_machineCard;
    AccountUsage::Panel *m_usage;
    AccountUsageStore *m_usageStore;
    QHash<QString,qint64> m_catalogChecked;
    QSet<QString> m_catalogRefreshQueued;
    QString m_requestedProfile;
    QString m_activeProfile, m_machineSignature;
    QList<QJsonObject> m_renameQueue;
    QString m_renameLabel;
    bool m_renderQueued = false, m_dark = false;
    QHash<QString, QJsonObject> m_catalogs;
    QHash<quint64, QPair<QString, QString>> m_requests;
    QString m_loginAfterAddHost, m_loginAfterAddId;
    // One machine explicitly opened from Machines, including peers disabled for polling.
    QString m_explicitHost;
};
