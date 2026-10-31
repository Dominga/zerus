#pragma once
#include <QDialog>
#include <QJsonArray>
#include <QJsonObject>
#include "TeamModel.h"

class QCheckBox;
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

    TeamController *m_teams;
    QString m_team;
    QJsonObject m_settings;
    QList<Teams::Team::Intake> m_intakes;
    bool m_dark = true;
    QCheckBox *m_branch = nullptr, *m_restart = nullptr, *m_land = nullptr, *m_broker = nullptr;
    QList<MemberModels> m_members;
    QLineEdit *m_template = nullptr, *m_base = nullptr, *m_label = nullptr, *m_key = nullptr;
    QLabel *m_problem = nullptr, *m_keyNote = nullptr;
    QPushButton *m_save = nullptr, *m_create = nullptr, *m_copy = nullptr;
    QWidget *m_keyBox = nullptr;
    QVBoxLayout *m_intakeList = nullptr;
};
