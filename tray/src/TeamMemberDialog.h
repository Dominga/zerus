#pragma once
#include "TeamModel.h"
#include <QDialog>
class TeamController;
class QCheckBox;
class QComboBox;
class QDialogButtonBox;
class QLabel;
class QPlainTextEdit;
class QSpinBox;
class QPushButton;
class QTableWidget;
class QVBoxLayout;

// The owner's live member edit: a partial settings update, never a session swap.
class TeamMemberDialog : public QDialog {
public:
    TeamMemberDialog(TeamController *teams, const Teams::Team &team, const Teams::Member &member, bool dark, QWidget *parent = nullptr);
    void reject() override;
private:
    void modelList(QVBoxLayout *layout);
    QJsonObject changes() const;
    void sync();
    void save();
    void busy(bool saving);
    TeamController *m_teams;
    QString m_team;
    Teams::Member m_before;
    bool m_planning, m_running, m_saving = false;
    QWidget *m_fields;
    QPlainTextEdit *m_prompt, *m_rules;
    QComboBox *m_model, *m_effort, *m_permissions, *m_profile, *m_context;
    QComboBox *m_afterTask, *m_afterOn;
    QSpinBox *m_afterAt;
    QCheckBox *m_clear, *m_widen;
    QTableWidget *m_models;
    QPushButton *m_addModel, *m_save;
    QLabel *m_error;
    QDialogButtonBox *m_buttons;
};
