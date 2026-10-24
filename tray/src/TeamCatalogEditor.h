#pragma once
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class TeamController;
class QListWidget;
class QPushButton;
class QLabel;
class QTabWidget;
class QScrollArea;

// WhiteWh fork: agent teams. Settings > Teams (design S7): the catalog's roles
// (name, color, prompt with its place in what a member reads, default agent,
// permissions, folder policy, who it may message), templates (roster, task
// skeleton, planning and approval) and routing rules with Try it. Edits stay
// local until Save, which writes the next catalog version; Import and Export
// move the catalog as JSON.
class TeamCatalogEditor : public QWidget {
public:
    explicit TeamCatalogEditor(TeamController *teams, QWidget *parent = nullptr);
    std::function<void()> askDesigner;
    void setDark(bool dark) { m_dark = dark; rebuildEditor(); }
    // The Team designer page has its own title.
    void hideHeading() { for (auto *w : m_heading) w->hide(); }
private:
    void load();
    void rebuildLists();
    void rebuildEditor();
    QWidget *roleEditor(int index);
    QWidget *templateEditor(int index);
    QWidget *routingEditor();
    void changed();
    TeamController *m_teams;
    Teams::Catalog m_catalog;
    bool m_dirty = false, m_dark = true;
    QTabWidget *m_tabs;
    QListWidget *m_roles, *m_templates;
    QScrollArea *m_editor;
    QPushButton *m_save, *m_revert;
    QLabel *m_status;
    QList<QWidget *> m_heading;
};
