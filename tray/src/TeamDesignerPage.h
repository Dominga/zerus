#pragma once
#include <QJsonObject>
#include <QSet>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QLabel;
class QPlainTextEdit;
class QPushButton;
class QScrollArea;
class QSplitter;
class QStackedWidget;
class QVBoxLayout;
class TeamCatalogEditor;
class TeamController;
class TeamConversation;

// The Team designer mode:
// the universal catalog that New team builds teams from. Left: the designer
// agent of the selected session's project — its conversation, its proposals to
// keep or reject, or Start designer when it does not run. Right: the catalog,
// edited by hand or shown as the designer's draft with a diff.
class TeamDesignerPage : public QWidget {
public:
    explicit TeamDesignerPage(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark);
    // The folder and project the designer reads: the selected session's.
    void setContext(const QString &folder, const QString &project);
    // The designer session of that project, if it runs; and its live state.
    std::function<QString()> designerSession;
    std::function<Teams::Live()> designerLive;
    // agent: {"agent", "model", "effort"} of the designer's session.
    std::function<void(const QString &folder, const QString &goal, const QJsonObject &agent)> startDesigner;
    std::function<void(const QString &session)> openDesigner;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // After the session list or the catalog changes.
    void refresh();
    TeamCatalogEditor *editor() const { return m_editor; }
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void renderProposals();
    QWidget *emptyState();
    TeamController *m_teams;
    QString m_folder, m_project;
    TeamCatalogEditor *m_editor;
    TeamConversation *m_chat;
    QSplitter *m_split, *m_chatSplit;
    QStackedWidget *m_chatStack;
    QScrollArea *m_proposals;
    // Decided proposals closed in the list; Apply still writes the kept ones.
    QSet<QString> m_closed;
    QLabel *m_title, *m_status, *m_reads, *m_proposes, *m_version;
    QPlainTextEdit *m_goal = nullptr;
    class QComboBox *m_agent = nullptr, *m_model = nullptr, *m_effort = nullptr;
    QPushButton *m_start = nullptr, *m_open;
    bool m_dark = true;
};
