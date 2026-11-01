#pragma once
#include <QJsonObject>
#include <QSet>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QComboBox;
class QLabel;
class QPushButton;
class QScrollArea;
class QSplitter;
class QVBoxLayout;
class TeamCatalogEditor;
class TeamController;
class TeamConversation;

// WhiteWh fork: agent teams. The Team designer mode (design D1, HANDOFF §12;
// U25 spec 11.1): the universal catalog that New team builds teams from. Left:
// the designer agent's chat - before the first message its intro and the
// composer in the middle (the first message starts the designer with it as the
// goal), then its conversation, its proposals to keep or reject and the DRAFT
// bar right above the composer. Right: the catalog, edited by hand or shown as
// the designer's draft.
class TeamDesignerPage : public QWidget {
public:
    struct Project {
        QString id, name, folder, folderName;
    };
    explicit TeamDesignerPage(TeamController *teams, QWidget *parent = nullptr);
    void setDark(bool dark);
    // Local project folders are available without a selected or running session.
    void setProjects(const QList<Project> &projects, const QString &defaultProject = {});
    // A selected session can also supply its folder and project directly.
    void setContext(const QString &folder, const QString &project);
    // The designer session of that project, if it runs; and its live state.
    std::function<QString()> designerSession;
    std::function<Teams::Live()> designerLive;
    // agent: {"agent", "model", "effort"} of the designer's session.
    std::function<void(const QString &folder, const QString &goal, const QJsonObject &agent)> startDesigner;
    // The start failed: the first message stays in the composer with the reason.
    void startFailed(const QString &error);
    std::function<void(const QString &session)> openDesigner;
    // U35: the designer's terminal, when a send meets a dialog or draft.
    std::function<void(const QString &host, const QString &session)> openTerminal;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // After the session list or the catalog changes.
    void refresh();
    TeamCatalogEditor *editor() const { return m_editor; }
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void renderProposals();
    void renderDraftBar();
    void showModelMenu();
    void updateModelButton();
    QWidget *intro();
    TeamController *m_teams;
    QString m_folder, m_project;
    TeamCatalogEditor *m_editor;
    TeamConversation *m_chat;
    QSplitter *m_split;
    QScrollArea *m_proposals;
    QWidget *m_draftBar;
    QLabel *m_draftError = nullptr, *m_draftText;
    QPushButton *m_draftApply, *m_draftDiscard;
    // Decided proposals closed in the list; Apply still writes the kept ones.
    QSet<QString> m_closed;
    QLabel *m_title, *m_status, *m_reads, *m_proposes, *m_version;
    QComboBox *m_projectChoice;
    QPushButton *m_open;
    // The designer's agent, model and effort before it starts.
    QString m_agent = QStringLiteral("claude"), m_model = QStringLiteral("opus"), m_effort = QStringLiteral("high");
    bool m_dark = true;
};
