#pragma once
#include <QWidget>
#include <functional>

class QLabel;
class QPushButton;
class TeamCatalogEditor;
class TeamController;
class TeamDesignerPanel;

// WhiteWh fork: agent teams. The Team designer mode on the rail: the universal
// catalog that New team builds teams from. The catalog editor (roles, templates,
// routing) on the left; on the right the designer agent — start it, open its
// chat — and its draft of proposals to keep or reject, then apply.
class TeamDesignerPage : public QWidget {
public:
    explicit TeamDesignerPage(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark);
    // The running designer session's name, or empty.
    std::function<QString()> designerSession;
    std::function<void()> startDesigner;
    std::function<void(const QString &session)> openDesigner;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // The designer agent's line and buttons, after the session list changes.
    void refresh();
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    TeamController *m_teams;
    TeamCatalogEditor *m_editor;
    TeamDesignerPanel *m_draft;
    QLabel *m_title, *m_subtitle, *m_agentTitle, *m_agentStatus;
    QPushButton *m_start, *m_open;
    bool m_dark = true;
};
