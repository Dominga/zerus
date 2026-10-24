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

// New team: a template from the catalog,
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
    QLineEdit *m_name, *m_folder;
    QComboBox *m_template, *m_leadAgent, *m_leadAccount, *m_leadModel, *m_leadEffort, *m_existingLead;
    QCheckBox *m_useExisting, *m_worktrees, *m_review;
    QComboBox *m_planning;
    QSpinBox *m_maxMembers;
    QPlainTextEdit *m_goal;
    QLabel *m_gitNote, *m_summary;
    QPushButton *m_create;
    QVBoxLayout *m_rows;
    QList<Row *> m_rowList;
    QString m_project, m_created;
};
