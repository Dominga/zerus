#pragma once
#include <QDialog>
#include <QJsonArray>
#include <QJsonObject>
#include "TeamModel.h"

class QCheckBox;
class QPlainTextEdit;
class QComboBox;
class QLabel;
class QLineEdit;
class QPushButton;
class QVBoxLayout;
namespace TeamUi { class FlowLayout; }
class TeamController;

// U48, U51, U52: the owner's team settings that change how Zerus runs the
// team: a branch per task (and its base), whether land stages may land
// (#581 N5, asked first), restarting a member whose session crashed, each
// member's models (U45, #581 N6) and the intake keys for urgent tasks. A team
// that sets none of them runs as before, and Save sends only the fields that
// changed, in one `team settings` call.
class TeamSettingsDialog : public QDialog {
public:
    TeamSettingsDialog(TeamController *teams, const Teams::Team &team, bool dark, QWidget *parent = nullptr);
    // The settings that differ from the team's, as `team settings` merges them;
    // a branch per task switched off is null.
    QJsonObject changes() const;
    // The members whose models changed, as `members_update` takes them:
    // {id, models}; an emptied list is [] (only the launch model).
    QJsonArray memberChanges() const;
    // Why Save is refused, or empty.
    QString problem() const;
    // Task 88: the Tests section. The project's configuration as hgs reads it
    // (`team test-config`), and the owner's override, edited as JSON shaped
    // like .zerus/tests.toml: Check asks hgs without writing; only Save sends
    // it (as `settings.tests`, which hgs checks again).
    void loadTests();
    void checkTests();
    // The override the field holds: null for an empty field, or why it is not
    // a JSON object.
    QJsonValue testsOverride(QString *error = nullptr) const;
    // What the section lists from a `test-config` answer, one line each.
    static QStringList testsLines(const QJsonObject &config);

private:
    void update();
    void save();
    void createKey();
    void revokeKey(const QString &id, const QString &label);
    void showIntakes();
    void showMemberModels(int index);
    bool confirmLanding();

    struct MemberModels {
        QString id, role, agent, model;
        QList<Teams::ModelChoice> before, now;
        QWidget *chipsBox = nullptr;
        TeamUi::FlowLayout *chips = nullptr;
    };

    QPlainTextEdit *m_rules = nullptr;
    QPlainTextEdit *m_tests = nullptr, *m_testsProposal = nullptr;
    QLabel *m_testsSource = nullptr, *m_testsCheck = nullptr;
    QVBoxLayout *m_testsList = nullptr;
    QPushButton *m_testsCheckButton = nullptr, *m_testsProposalButton = nullptr;
    void showTests(const QJsonObject &config);
    struct MemberRules { QString id; QStringList before; QPlainTextEdit *field; };
    QList<MemberRules> m_ruleMembers;
    TeamController *m_teams;
    QString m_team;
    QJsonObject m_settings;
    QList<Teams::Team::Intake> m_intakes;
    bool m_dark = true;
    QCheckBox *m_branch = nullptr, *m_restart = nullptr, *m_land = nullptr, *m_broker = nullptr, *m_acceptance = nullptr;
    QCheckBox *m_targetCheck = nullptr, *m_requiresCheck = nullptr;
    QComboBox *m_leadUnpause = nullptr;   // U63: false = only with my consent
    QList<MemberModels> m_members;
    QLineEdit *m_template = nullptr, *m_base = nullptr, *m_label = nullptr, *m_key = nullptr;
    QLabel *m_problem = nullptr, *m_keyNote = nullptr;
    QPushButton *m_save = nullptr, *m_create = nullptr, *m_copy = nullptr;
    QWidget *m_keyBox = nullptr;
    QVBoxLayout *m_intakeList = nullptr;
};
