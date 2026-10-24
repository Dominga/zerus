#pragma once
#include <QMap>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class HgsClient;
class TeamController;
class TeamConversation;
class QCheckBox;
class QGridLayout;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QProgressBar;
class QPushButton;
class QScrollArea;
class QSpinBox;
class QVBoxLayout;

// The New team tab next to Activity: the selected local session coordinates the team built here —
// it is not started again, it receives the planning brief once. Template cards
// from the catalog fill the member rows; the goal is the main input. Members
// read as a table from 760 px and as cards below, with an error on each row
// that cannot work. Create sends `create` with the rows as entered.
class TeamNewTab : public QWidget {
public:
    struct Computer { QString alias; QString label; bool online = true; };
    struct Coordinator { QString name, agent, model, account, folder, computer, status; bool busy = false; };
    struct Member { QString role, agent, account, model, effort, permissions, computer, prompt; bool worktree = true; };
    TeamNewTab(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const Coordinator &coordinator,
               const QString &project, QWidget *parent = nullptr);
    QString leadSession() const { return m_coordinator.name; }
    void setDark(bool dark);
    // The coordinator's live state changes while the tab is open.
    void setCoordinatorBusy(bool busy);
    std::function<void(const QString &team)> created;
    std::function<void()> askDesigner;
    // The Team designer's session, if it runs, and how to start it: its chat
    // here helps write the goal and the team protocol.
    std::function<QString()> designerSession;
    std::function<void()> startDesigner;
    // The message that asks for both texts, and the reply's two sections.
    QString helperRequest() const;
    static bool parseHelperReply(const QString &reply, QString *goal, QString *protocol);
protected:
    void resizeEvent(QResizeEvent *event) override;
    void showEvent(QShowEvent *event) override;
private:
    void updateHelper();
    TeamConversation *m_helper = nullptr;
    QWidget *m_helperIdle = nullptr;
    QPushButton *m_helperDraft = nullptr, *m_helperUse = nullptr;
    QLabel *m_helperNote = nullptr;
    void pickTemplate(const QString &id);
    void addMember(const Teams::Role *role);
    void renderCoordinator();
    void renderTemplates();
    void renderMembers();
    void validate();
    void create();
    QStringList rowErrors(const Member &member) const;
    QJsonObject memberJson(const Member &member, const QString &id) const;
    struct Account { QString id, provider, label; bool signedIn = true; };
    TeamController *m_teams;
    HgsClient *m_client;
    QList<Computer> m_computers;
    Coordinator m_coordinator;
    QString m_project, m_template;
    QList<Member> m_members;
    QMap<QString, QList<Account>> m_accounts;   // by computer alias
    bool m_dark = true, m_wide = true, m_creating = false;
    int m_templateColumns = 0;
    QScrollArea *m_scroll;
    QWidget *m_coordinatorCard, *m_templatesBox, *m_membersBox, *m_gitNote;
    QGridLayout *m_templates;
    QVBoxLayout *m_membersLayout;
    QLabel *m_busy, *m_membersCount, *m_summary, *m_reason, *m_branch;
    QPlainTextEdit *m_goal;
    // How the coordinator runs the team: the template's lead protocol until
    // you edit it; only the coordinator reads it.
    QPlainTextEdit *m_protocol;
    bool m_protocolEdited = false;
    QLineEdit *m_name, *m_folder;
    QCheckBox *m_worktrees, *m_review;
    QWidget *m_planning;
    QString m_planningValue = QStringLiteral("lead");
    QSpinBox *m_max;
    QPushButton *m_addMember, *m_create;
    QProgressBar *m_progress;
};
