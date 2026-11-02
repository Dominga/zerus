#pragma once
#include <QHash>
#include <QPointer>
#include <QSet>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class TeamController;
class HgsClient;
class QComboBox;
class QFrame;
class QListWidget;
class QPushButton;
class QLabel;
class QScrollArea;
class QStackedWidget;
class QVBoxLayout;

// WhiteWh fork: agent teams. The team catalog (design D1, HANDOFF §12; U25
// spec 11.2-11.5): roles (name, about, icon and color, specialization,
// default agent, computers, permissions, folder policy, who it may message,
// when it joins, dangerous actions and the prompt order), templates (roster
// counts with "only if needed", work mode, task skeleton with phases,
// planning, approval, new phases, max members, checks) and routing rules with
// Try it. One widget, two hosts: the Team designer page and Settings > Teams.
// Manual edits stay local until Save writes the next version. With the draft
// shown (the designer page), the designer's proposals show in the same
// editors, read only, with a plate to keep or reject each; a draft made on an
// older version can't apply.
class TeamCatalogEditor : public QWidget {
public:
    explicit TeamCatalogEditor(TeamController *teams, QWidget *parent = nullptr);
    std::function<void()> askDesigner;
    // Asks the designer to redo its draft on the current version.
    std::function<void(qint64 version)> askRedo;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // The selected session's project rules, the first block of Prompt order.
    std::function<QString()> projectRules;
    void setDark(bool dark);
    // The designer page shows the draft; Settings edits by hand only.
    void setShowDraft(bool show) { m_showDraft = show; load(); }
    // Settings offers Open Team designer.
    void setDesignerButton(bool shown);
    // While the designer works, saving by hand makes its draft out of date.
    void setDesignerWorking(bool working) { if (working == m_designerWorking) return; m_designerWorking = working; updateHeader(); }
    // Selects a proposal's item: Show in catalog.
    void showProposal(const QString &proposal);
    // Apply as from the header, with its confirmation; off while it is.
    void applyKept();
    // N38 (#548): whether a designer runs to redo an out-of-date draft; the
    // banner offers "Ask designer to redo" only then.
    std::function<bool()> canRedo;
    // The banner again, after canRedo's answer may have changed.
    void refreshBanner() { updateHeader(); }
    // Discard as from the header, with its confirmation.
    void discardDraft();
    // The role's Computer and Account dropdowns: computers by alias (empty is
    // this computer) and the accounts each one reports.
    struct Computer { QString alias, label; bool online = true; };
    void setSources(HgsClient *client, const QList<Computer> &computers);
    // The template a goal would use, as Try it shows it (the server's route).
    struct Route { int rule = -1; QString templateId; QStringList added; QString approval; };
    static Route route(const Teams::Catalog &catalog, const QString &goal);
    // U36: Try it on the Pipelines tab: a sample task walked through a preset
    // with the team's members today (who takes each stage, where it works,
    // what holds the folder) and the warnings, such as a single Tester. Nothing
    // is sent.
    struct Step { QString kind, title, role, member, agent, detail, where; };
    struct Walk { QList<Step> steps; QStringList warnings; };
    static Walk walk(const Teams::Catalog &catalog, const Teams::Pipeline &pipeline, const Teams::Team &team, const Teams::Task &task);
private:
    struct Account { QString id, provider, label; bool signedIn = true; };
    void fillAccounts(QComboBox *box, const Teams::Role &role) const;
    QString modelKey(const Teams::Role &role) const;
    void fillModels(QComboBox *box, const Teams::Role &role) const;
    void requestModels(const Teams::Role &role);
    enum class Tab { Roles, Templates, Routing, Rules, Pipelines };
    bool draftMode() const;
    void load();
    // A newer saved catalog (the designer applied, another window saved)
    // while this editor has unsaved edits: the edits move onto it. An item
    // edited here keeps the edit; every other item is the newer one's.
    void rebase();
    void rebuildLists();
    void rebuildEditor();
    void updateHeader();
    void setTab(Tab tab);
    // U28: the lead's planning rules: the text, or a proposal's line diff.
    QWidget *planningRulesEditor();
    void resetPlanningRules();
    bool canResetPlanningRules() const;
    void updatePlanningRulesReset();
    void selectTemplate(const QString &id);
    QWidget *roleEditor(int index);
    QWidget *templateEditor(int index);
    QWidget *routingEditor();
    // U36: the Pipelines tab: a preset's stages and rules, the defaults per
    // template and role, and Try it; "none" explains a task without one.
    QWidget *pipelineEditor(int index);
    QWidget *pipelineStageDetails(int index, int stage);
    QWidget *pipelineDefaults(const Teams::Pipeline &pipeline);
    QWidget *pipelineTry(const Teams::Pipeline &pipeline);
    QWidget *pipelineDiff(const Teams::Pipeline &mine, const Teams::Proposal &proposal);
    QStringList pipelineUsers(const QString &id) const;
    QWidget *proposalPlate(const Teams::Proposal &proposal);
    QWidget *promptOrder(int index);
    QWidget *listRow(const QString &kind, int index, const Teams::Proposal *proposal, bool dirty);
    const Teams::Proposal *proposalOf(const QString &kind, const QString &id) const;
    int unsavedChanges() const;
    void changed();
    void decide(const QString &proposal, const QString &decision);
    TeamController *m_teams;
    HgsClient *m_client = nullptr;
    QList<Computer> m_computers;
    QHash<QString, QList<Account>> m_accounts;
    QPointer<QComboBox> m_accountChoice;
    int m_accountRole = -1;
    QPointer<QComboBox> m_modelChoice;
    int m_modelRole = -1;
    QHash<QString, QJsonArray> m_modelCatalogs;
    QSet<QString> m_modelPending;
    QHash<quint64, QString> m_modelRequests;
    // The catalog the editors show: the saved one with manual edits, or the
    // draft view (the saved one with every proposal over it), read only.
    Teams::Catalog m_catalog;
    // The saved catalog m_catalog's edits started from (rebase()).
    Teams::Catalog m_loaded;
    bool m_dirty = false, m_dark = true, m_showDraft = false, m_designerWorking = false, m_readOnly = false;
    Tab m_tab = Tab::Roles;
    QString m_applied;
    QLabel *m_title, *m_meta, *m_bannerTitle, *m_bannerText, *m_bannerIcon, *m_legend;
    QFrame *m_banner;
    QPushButton *m_bannerAction, *m_bannerDiscard;
    QWidget *m_tabs;
    QListWidget *m_roles, *m_templates, *m_pipelines;
    // U36: the stage whose details show, per preset.
    QHash<QString, int> m_openStage;
    QWidget *m_left;
    QPushButton *m_add;
    QVBoxLayout *m_editorHost;
    QWidget *m_editorCard = nullptr;
    QScrollArea *m_body;
    QPushButton *m_openDesigner, *m_import, *m_export, *m_save, *m_revert, *m_discard, *m_apply;
};
