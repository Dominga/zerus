#pragma once
#include <QDialog>
#include <QJsonObject>
#include <QMap>
#include <functional>
#include "TeamModel.h"

class HgsClient;
class TeamController;
class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QVBoxLayout;

// WhiteWh fork: agent teams. New team (design S6): a template from the catalog,
// the goal, the folder (a Git folder offers a worktree per member), the lead —
// a new session or an existing one — and the members with an error on each row
// that cannot work (computer offline, account not signed in). Create team starts
// only the lead; it plans, and members start after Approve & Run.
class TeamNewDialog : public QDialog {
public:
    struct Computer { QString alias; QString label; bool online = true; };
    struct Session { QString host, name, agent; };
    TeamNewDialog(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const QList<Session> &sessions,
                  const QString &folder, const QString &project, QWidget *parent = nullptr);
    // The created team's id, once Create team succeeded.
    QString createdTeam() const { return m_created; }
    ~TeamNewDialog() override;
    // F6: the template the list shows as chosen, applied as a click would.
    void selectTemplate(const QString &id);
    QString templateId() const { return m_templateId; }
    // F7: take a session a finished team left running into a new team. The
    // dialog's only existing-session slot is the lead, so it is preselected
    // there; a session on another computer cannot lead from here.
    void preselectExistingLead(const QString &host, const QString &name);
    struct Row {
        QWidget *box; QComboBox *role, *agent, *account, *model, *effort, *permissions, *computer; QCheckBox *worktree; QLabel *error;
        QString prompt;
    };
private:
    Row *addRow(const Teams::Role *role, const QString &roleName = {});
    void applyTemplate();
    void validate();
    void create();
    void fillAccounts(QComboBox *box, const QString &agent, const QString &host, const QString &preferred = {});
    QJsonObject memberJson(const Row &row, const QString &id) const;
    TeamController *m_teams;
    HgsClient *m_client;
    QList<Computer> m_computers;
    QList<Session> m_sessions;
    // Accounts by computer: profile id -> (provider, label, signed in).
    struct Account { QString id, provider, label; bool signedIn = true; };
    QMap<QString, QList<Account>> m_accounts;
    QLineEdit *m_name = nullptr, *m_folder = nullptr;
    QComboBox *m_leadAgent = nullptr, *m_leadAccount = nullptr, *m_leadModel = nullptr, *m_leadEffort = nullptr, *m_existingLead = nullptr;
    // F6: the compact template list; "-" or empty is no template.
    class TeamTemplateList *m_templateList = nullptr;
    QString m_templateId;
    QCheckBox *m_useExisting = nullptr, *m_worktrees = nullptr, *m_review = nullptr;
    // B6: how members get their folders - auto (the lead picks), shared or a
    // worktree each. A folder that is not a Git repository allows only shared.
    QComboBox *m_folderPolicy = nullptr;
    // N10: set when the folder path is finished (focus out or Enter), so typing
    // keeps auto and only a finished non-Git path forces shared.
    bool m_folderCommitted = true;
    // N10: the current shared choice came from the non-Git folder rule, so
    // resuming typing returns it to auto.
    bool m_forcedShared = false;
    QComboBox *m_planning = nullptr;
    QSpinBox *m_maxMembers = nullptr;
    QPlainTextEdit *m_goal = nullptr;
    QLabel *m_gitNote = nullptr, *m_summary = nullptr;
    QPushButton *m_create = nullptr;
    QVBoxLayout *m_rows = nullptr;
    QList<Row *> m_rowList;
    QString m_project, m_created;
};
