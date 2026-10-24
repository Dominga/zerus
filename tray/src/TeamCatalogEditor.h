#pragma once
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class TeamController;
class QFrame;
class QListWidget;
class QPushButton;
class QLabel;
class QScrollArea;
class QStackedWidget;
class QVBoxLayout;

// WhiteWh fork: agent teams. The team catalog (design D1, HANDOFF §12): roles
// (name, color, specialization, default agent, permissions, folder policy, who
// it may message, prompt with its place in what a member reads), templates
// (roster per role, task skeleton, planning, approval) and routing rules with
// Try it. One widget, two hosts: the Team designer page and Settings > Teams.
// Manual edits stay local until Save writes the next version. With the draft
// shown (the designer page), the designer's proposals read as a diff to keep
// or reject and Apply together; a draft made on an older version can't apply.
class TeamCatalogEditor : public QWidget {
public:
    explicit TeamCatalogEditor(TeamController *teams, QWidget *parent = nullptr);
    std::function<void()> askDesigner;
    // Asks the designer to redo its draft on the current version.
    std::function<void(qint64 version)> askRedo;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    void setDark(bool dark);
    // The designer page shows the draft; Settings edits by hand only.
    void setShowDraft(bool show) { m_showDraft = show; load(); }
    // Settings offers Open Team designer.
    void setDesignerButton(bool shown);
    // While the designer works, saving by hand makes its draft out of date.
    void setDesignerWorking(bool working) { m_designerWorking = working; updateHeader(); }
    // Selects a proposal's item: Show in catalog.
    void showProposal(const QString &proposal);
private:
    enum class Tab { Roles, Templates, Routing };
    bool draftMode() const;
    void load();
    void rebuildLists();
    void rebuildEditor();
    void updateHeader();
    void setTab(Tab tab);
    QWidget *roleEditor(int index);
    QWidget *templateEditor(int index);
    QWidget *routingEditor();
    QWidget *draftEditor(const QString &kind, const QString &id);
    QWidget *listRow(const QString &kind, const QString &id, const QString &name, const QColor &color, const QString &agent,
                     const Teams::Proposal *proposal, bool dirty);
    int unsavedChanges() const;
    void changed();
    TeamController *m_teams;
    Teams::Catalog m_catalog;
    bool m_dirty = false, m_dark = true, m_showDraft = false, m_designerWorking = false;
    Tab m_tab = Tab::Roles;
    QString m_applied;
    QLabel *m_title, *m_meta, *m_bannerText, *m_legend;
    QFrame *m_banner;
    QPushButton *m_bannerAction, *m_bannerDiscard;
    QWidget *m_tabs;
    QListWidget *m_roles, *m_templates;
    QWidget *m_left;
    QPushButton *m_add;
    QVBoxLayout *m_editorHost;
    QWidget *m_editorCard = nullptr;
    QScrollArea *m_body;
    QPushButton *m_openDesigner, *m_import, *m_export, *m_save, *m_revert, *m_discard, *m_apply;
};
