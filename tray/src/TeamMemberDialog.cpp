#include "TeamMemberDialog.h"
#include "IronRulesEditor.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QHeaderView>
#include <QLabel>
#include <QPointer>
#include <QPushButton>
#include <QScrollArea>
#include <QSpinBox>
#include <QTableWidget>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
QComboBox *editable(const QStringList &items, const QString &current, const char *name)
{
    auto *field = new QComboBox;
    field->setObjectName(name); field->setEditable(true); field->setInsertPolicy(QComboBox::NoInsert);
    field->setMinimumContentsLength(16); field->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    field->addItems(items); field->setCurrentText(current);
    return field;
}
void select(QComboBox *field, const QString &value)
{
    if (field->findData(value) < 0) field->addItem(value, value);
    field->setCurrentIndex(field->findData(value));
}
}

TeamMemberDialog::TeamMemberDialog(TeamController *teams, const Teams::Team &team, const Teams::Member &member, bool dark, QWidget *parent)
    : QDialog(parent), m_teams(teams), m_team(team.id), m_before(member), m_planning(team.status == "planning"), m_running(team.status != "planning" && !member.session.isEmpty())
{
    setObjectName("teamMemberEditDialog"); setWindowTitle(tr("Edit member: %1").arg(team.roleOf(member.id)));
    setAttribute(Qt::WA_DeleteOnClose); setModal(true); resize(560, 720);
    const auto c = colorsFor(dark); setStyleSheet(dialogSheet(c));
    auto *outer = new QVBoxLayout(this);
    auto *note = new QLabel(tr("The start prompt replaces earlier instructions. Model and effort apply before the next task when supported, otherwise at the next start; permissions apply at the next start. Use Swap member for agent, account, computer or folder."));
    note->setWordWrap(true); outer->addWidget(note);
    auto *scroll = new QScrollArea; scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    m_fields = new QWidget; auto *fields = new QVBoxLayout(m_fields);
    auto *form = new QFormLayout;
    m_prompt = new QPlainTextEdit(member.prompt); m_prompt->setObjectName("teamMemberEditPrompt"); m_prompt->setMaximumHeight(130);
    form->addRow(tr("Start prompt"), m_prompt);
    m_clear = new QCheckBox(tr("Clear context first")); m_clear->setObjectName("teamMemberEditClear");
    m_clear->setEnabled(m_running); m_clear->setToolTip(tr("Optional for a new prompt sent to a running member. Zerus checks whether its agent supports clearing."));
    form->addRow(QString(), m_clear);
    m_model = editable(Teams::knownModels(member.agent), member.model, "teamMemberEditModel"); form->addRow(tr("Model"), m_model);
    m_effort = editable({"", "low", "medium", "high", "xhigh"}, member.effort, "teamMemberEditEffort"); form->addRow(tr("Effort"), m_effort);
    m_permissions = new QComboBox; m_permissions->setObjectName("teamMemberEditPermissions");
    m_permissions->addItem(tr("Account's mode"), ""); m_permissions->addItem(tr("Auto"), "auto"); m_permissions->addItem(tr("Bypass"), "bypass");
    select(m_permissions, member.permissions); form->addRow(tr("Permissions"), m_permissions);
    m_profile = new QComboBox; m_profile->setObjectName("teamMemberEditProfile"); m_profile->addItem(tr("Default (write)"), "");
    for (const auto *profile : {"read", "write", "build", "commit", "network"}) m_profile->addItem(QString::fromLatin1(profile), QString::fromLatin1(profile));
    select(m_profile, member.permissionsProfile); form->addRow(tr("Permissions profile"), m_profile);
    m_widen = new QCheckBox(tr("Confirm wider permissions for this member")); m_widen->setObjectName("teamMemberEditWiden");
    form->addRow(QString(), m_widen);
    m_context = new QComboBox; m_context->setObjectName("teamMemberEditContext");
    m_context->addItem(tr("Inherit role"), ""); m_context->addItem(tr("Keep context"), "none");
    m_context->addItem(tr("Compact"), "compact"); m_context->addItem(tr("Clear"), "clear");
    select(m_context, member.contextBeforeTask); form->addRow(tr("Before each task"), m_context);
    // Task #7: after a task, when the free member's context passes the
    // threshold, compact or clear it before the next task (keep by default).
    m_afterTask = new QComboBox; m_afterTask->setObjectName("teamMemberEditAfterTask");
    m_afterTask->addItem(tr("Keep context"), ""); m_afterTask->addItem(tr("Compact"), "compact");
    m_afterTask->addItem(tr("Clear"), "clear");
    select(m_afterTask, member.afterTask); form->addRow(tr("After a task"), m_afterTask);
    m_afterOn = new QComboBox; m_afterOn->setObjectName("teamMemberEditAfterOn");
    m_afterOn->addItem(tr("Percent of context"), "percent"); m_afterOn->addItem(tr("Tokens used"), "tokens");
    select(m_afterOn, member.afterTaskOn.isEmpty() ? QStringLiteral("percent") : member.afterTaskOn);
    form->addRow(tr("Criterion"), m_afterOn);
    m_afterAt = new QSpinBox; m_afterAt->setObjectName("teamMemberEditAfterAt");
    m_afterAt->setRange(1, 10000000); m_afterAt->setValue(member.afterTaskAt > 0 ? member.afterTaskAt : 80);
    m_afterAt->setToolTip(tr("The action runs before the next task only when the context passes this threshold."));
    form->addRow(tr("Threshold"), m_afterAt);
    fields->addLayout(form);
    fields->addWidget(new QLabel(tr("Allowed models (empty: only the launch model)")));
    modelList(fields);
    fields->addWidget(new QLabel(tr("Member iron rules")));
    m_rules = IronRules::editor(member.ironRules); m_rules->setObjectName("teamMemberEditRules"); fields->addWidget(m_rules);
    fields->addStretch(); scroll->setWidget(m_fields); outer->addWidget(scroll, 1);
    m_error = new QLabel; m_error->setObjectName("teamMemberEditError"); m_error->setWordWrap(true); m_error->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name())); outer->addWidget(m_error);
    m_buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel); outer->addWidget(m_buttons);
    m_save = m_buttons->button(QDialogButtonBox::Save); m_save->setObjectName("teamMemberEditSave");
    connect(m_buttons, &QDialogButtonBox::accepted, this, [this] { save(); });
    connect(m_buttons, &QDialogButtonBox::rejected, this, &TeamMemberDialog::reject);
    for (auto *field : {m_model, m_effort, m_permissions, m_profile, m_context})
        connect(field, &QComboBox::currentTextChanged, this, [this] { sync(); });
    for (auto *field : {m_afterTask, m_afterOn})
        connect(field, &QComboBox::currentTextChanged, this, [this] { sync(); });
    connect(m_afterAt, &QSpinBox::valueChanged, this, [this] { sync(); });
    for (auto *field : {m_prompt, m_rules}) connect(field, &QPlainTextEdit::textChanged, this, [this] { sync(); });
    for (auto *field : {m_clear, m_widen}) connect(field, &QCheckBox::toggled, this, [this] { sync(); });
    connect(m_models, &QTableWidget::cellChanged, this, [this] { sync(); });
    sync();
}

void TeamMemberDialog::modelList(QVBoxLayout *layout)
{
    m_models = new QTableWidget(int(m_before.models.size()), 2); m_models->setObjectName("teamMemberEditModels");
    m_models->setHorizontalHeaderLabels({tr("Model"), tr("Effort")});
    m_models->horizontalHeader()->setSectionResizeMode(QHeaderView::Stretch); m_models->setMaximumHeight(140);
    for (int row = 0; row < m_before.models.size(); ++row) {
        m_models->setItem(row, 0, new QTableWidgetItem(m_before.models[row].model));
        m_models->setItem(row, 1, new QTableWidgetItem(m_before.models[row].effort));
    }
    layout->addWidget(m_models);
    auto *buttons = new QHBoxLayout;
    m_addModel = new QPushButton(tr("Add model")); auto *remove = new QPushButton(tr("Remove selected model"));
    m_addModel->setObjectName("teamMemberEditModelAdd"); m_addModel->setToolTip(tr("At most 20 allowed models")); remove->setObjectName("teamMemberEditModelRemove");
    buttons->addWidget(m_addModel); buttons->addWidget(remove); buttons->addStretch(); layout->addLayout(buttons);
    connect(m_addModel, &QPushButton::clicked, this, [this] {
        if (m_models->rowCount() >= 20) return;
        m_models->insertRow(m_models->rowCount()); sync();
    });
    connect(remove, &QPushButton::clicked, this, [this] {
        if (m_models->currentRow() >= 0) m_models->removeRow(m_models->currentRow());
        sync();
    });
}

QJsonObject TeamMemberDialog::changes() const
{
    QJsonObject out;
    const auto put = [&out](const char *key, const QJsonValue &value, const QJsonValue &before) { if (value != before) out.insert(key, value); };
    put("prompt", m_prompt->toPlainText(), m_before.prompt);
    put("model", m_model->currentText().trimmed(), m_before.model);
    put("effort", m_effort->currentText().trimmed(), m_before.effort);
    put("permissions", m_permissions->currentData().toString(), m_before.permissions);
    put("permissions_profile", m_profile->currentData().toString(), m_before.permissionsProfile);
    put("context_before_task", m_context->currentData().toString(), m_before.contextBeforeTask);
    // Task #7: the three fields go together; keep sends empty mode and zero.
    const auto after = m_afterTask->currentData().toString();
    const auto afterOn = after.isEmpty() ? QString() : m_afterOn->currentData().toString();
    const int afterAt = after.isEmpty() ? 0 : m_afterAt->value();
    if (after != m_before.afterTask || afterOn != m_before.afterTaskOn || afterAt != m_before.afterTaskAt) {
        out.insert("after_task", after); out.insert("after_task_on", afterOn); out.insert("after_task_at", afterAt);
    }
    put("iron_rules", QJsonArray::fromStringList(IronRules::lines(m_rules->toPlainText())), QJsonArray::fromStringList(m_before.ironRules));
    QList<Teams::ModelChoice> models;
    for (int row = 0; row < m_models->rowCount(); ++row) {
        const auto *model = m_models->item(row, 0); const auto *effort = m_models->item(row, 1);
        models.append({model ? model->text().trimmed() : QString(), effort ? effort->text().trimmed() : QString()});
    }
    put("models", Teams::modelChoicesJson(models), Teams::modelChoicesJson(m_before.models));
    if (out.contains("prompt") && m_clear->isChecked()) out.insert("clear_context_first", true);
    if (!out.isEmpty()) out.insert("id", m_before.id);
    return out;
}

void TeamMemberDialog::sync()
{
    const bool prompt = m_prompt->toPlainText() != m_before.prompt;
    m_clear->setVisible(prompt); if (!prompt) m_clear->setChecked(false);
    const bool after = !m_afterTask->currentData().toString().isEmpty();
    m_afterOn->setEnabled(after); m_afterAt->setEnabled(after);
    m_afterAt->setMaximum(m_afterOn->currentData().toString() == "percent" ? 100 : 10000000);
    const bool profile = m_profile->currentData().toString() != m_before.permissionsProfile && !m_planning;
    m_widen->setVisible(profile); if (!profile) m_widen->setChecked(false);
    m_addModel->setEnabled(m_models->rowCount() < 20);
    const auto problem = m_rules->toPlainText() == m_before.ironRules.join(QLatin1Char(10)) ? QString() : IronRules::problem(m_rules->toPlainText());
    m_error->setText(problem); m_error->setVisible(!problem.isEmpty());
    m_save->setEnabled(!m_saving && problem.isEmpty() && !changes().isEmpty());
}

void TeamMemberDialog::busy(bool saving)
{
    m_saving = saving; m_fields->setEnabled(!saving); m_save->setEnabled(!saving);
    m_buttons->button(QDialogButtonBox::Cancel)->setEnabled(!saving);
}

void TeamMemberDialog::save()
{
    const auto update = changes(); if (m_saving || update.isEmpty()) return;
    QJsonObject input{{"team", m_team}, {"members_update", QJsonArray{update}}};
    if (m_widen->isChecked()) input.insert("confirm_widening", true);
    busy(true);
    m_teams->act("settings", input, [self = QPointer<TeamMemberDialog>(this)](bool ok, const QString &error) {
        if (!self) return;
        self->busy(false);
        if (ok) { self->accept(); return; }
        self->m_error->setText(error.isEmpty() ? tr("The member was not changed") : error); self->m_error->show();
    });
}

void TeamMemberDialog::reject() { if (!m_saving) QDialog::reject(); }
