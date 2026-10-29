#include "TeamSettingsDialog.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QDateTime>
#include <QDialogButtonBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPointer>
#include <QPushButton>
#include <QVBoxLayout>
#include <algorithm>

using namespace TeamUi;

namespace {
QLabel *note(const QString &text, const QColor &color, const char *name = nullptr)
{
    auto *l = label(text, color, 11, false, name);
    l->setWordWrap(true);
    return l;
}
}

TeamSettingsDialog::TeamSettingsDialog(TeamController *teams, const Teams::Team &team, bool dark, QWidget *parent)
    : QDialog(parent), m_teams(teams), m_team(team.id), m_settings(team.settingsJson), m_intakes(team.intakes), m_dark(dark)
{
    const Colors c = colorsFor(dark);
    setObjectName("teamSettingsDialog");
    setWindowTitle(tr("Team settings"));
    setModal(true);
    resize(520, 520);
    auto *v = new QVBoxLayout(this); v->setContentsMargins(16, 14, 16, 12); v->setSpacing(8);
    v->addWidget(label(tr("Settings of %1").arg(team.name), c.fg, 14, true, "teamSettingsTitle"));
    // U48: each hand-out starts the task's own branch in the member's folder.
    v->addWidget(label(tr("Branches"), c.text2, 12, true));
    const QString branchTemplate = m_settings.value("branch_per_task").toObject().value("template").toString();
    m_branch = new QCheckBox(tr("A branch per task")); m_branch->setObjectName("teamSettingsBranchPerTask");
    m_branch->setChecked(!branchTemplate.isEmpty());
    v->addWidget(m_branch);
    auto *names = new QHBoxLayout; names->setSpacing(6);
    m_template = new QLineEdit(branchTemplate.isEmpty() ? QStringLiteral("task/{id}") : branchTemplate); m_template->setObjectName("teamSettingsBranchTemplate");
    m_template->setToolTip(tr("The branch name: {id} is the task, {member} the member and {team8} the team's first eight characters"));
    m_base = new QLineEdit(m_settings.value("base_branch").toString()); m_base->setObjectName("teamSettingsBaseBranch");
    m_base->setPlaceholderText(tr("the lead's branch"));
    m_base->setToolTip(tr("The branch a task's branch starts from"));
    names->addWidget(label(tr("Name"), c.muted, 12)); names->addWidget(m_template, 1);
    names->addWidget(label(tr("from"), c.muted, 12)); names->addWidget(m_base, 1);
    v->addLayout(names);
    v->addWidget(note(tr("Off: one branch per member, as before. On: a returned or reopened task gets its own branch again; a folder with changes waits."), c.muted));
    // U52: a member session that exits on its own is launched again.
    v->addWidget(label(tr("Members"), c.text2, 12, true));
    m_restart = new QCheckBox(tr("Restart a member whose session crashed")); m_restart->setObjectName("teamSettingsRestartCrashed");
    m_restart->setChecked(m_settings.value("restart_crashed").toBool());
    v->addWidget(m_restart);
    v->addWidget(note(tr("At most three times an hour, into the same folder and conversation; its open task goes back to the lead as unconfirmed. "
                         "A pause, an archive or a usage limit never restarts it."), c.muted));
    // U51: intake keys add one urgent phase; the key is shown once.
    v->addWidget(label(tr("Intake keys"), c.text2, 12, true));
    v->addWidget(note(tr("A process presenting a key, such as a health watcher, may add up to five urgent tasks as one phase, and nothing else."), c.muted));
    auto *list = new QWidget; list->setObjectName("teamSettingsIntakes");
    m_intakeList = new QVBoxLayout(list); m_intakeList->setContentsMargins(0, 0, 0, 0); m_intakeList->setSpacing(4);
    v->addWidget(list);
    auto *create = new QHBoxLayout; create->setSpacing(6);
    m_label = new QLineEdit; m_label->setObjectName("teamSettingsIntakeLabel"); m_label->setPlaceholderText(tr("What uses it, e.g. health watch"));
    m_label->setMaxLength(80);
    m_create = button(tr("Create key"), "teamSettingsIntakeCreate");
    create->addWidget(m_label, 1); create->addWidget(m_create);
    v->addLayout(create);
    m_keyBox = new QWidget; m_keyBox->setObjectName("teamSettingsIntakeKeyBox");
    auto *kv = new QVBoxLayout(m_keyBox); kv->setContentsMargins(0, 0, 0, 0); kv->setSpacing(4);
    auto *kh = new QHBoxLayout; kh->setSpacing(6);
    m_key = new QLineEdit; m_key->setObjectName("teamSettingsIntakeKey"); m_key->setReadOnly(true);
    m_copy = button(tr("Copy"), "teamSettingsIntakeCopy");
    kh->addWidget(m_key, 1); kh->addWidget(m_copy);
    kv->addLayout(kh);
    m_keyNote = note(tr("Shown only now: Zerus keeps only its hash. Copy it into the process that adds the tasks."), c.warn, "teamSettingsIntakeKeyNote");
    kv->addWidget(m_keyNote);
    m_keyBox->hide();
    v->addWidget(m_keyBox);
    v->addStretch();
    m_problem = note(QString(), c.danger, "teamSettingsProblem"); m_problem->hide();
    v->addWidget(m_problem);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Cancel);
    m_save = buttons->addButton(tr("Save"), QDialogButtonBox::AcceptRole); m_save->setObjectName("teamSettingsSave"); m_save->setDefault(true);
    buttons->button(QDialogButtonBox::Cancel)->setObjectName("teamSettingsCancel");
    v->addWidget(buttons);
    connect(m_branch, &QCheckBox::toggled, this, [this] { update(); });
    connect(m_restart, &QCheckBox::toggled, this, [this] { update(); });
    connect(m_template, &QLineEdit::textChanged, this, [this] { update(); });
    connect(m_base, &QLineEdit::textChanged, this, [this] { update(); });
    connect(m_label, &QLineEdit::textChanged, this, [this] { m_create->setEnabled(!m_label->text().trimmed().isEmpty() && m_intakes.size() < 8); });
    connect(m_label, &QLineEdit::returnPressed, this, [this] { if (m_create->isEnabled()) createKey(); });
    connect(m_create, &QPushButton::clicked, this, [this] { createKey(); });
    connect(m_copy, &QPushButton::clicked, this, [this] { QApplication::clipboard()->setText(m_key->text()); m_copy->setText(tr("Copied")); });
    connect(buttons, &QDialogButtonBox::accepted, this, [this] { save(); });
    connect(buttons, &QDialogButtonBox::rejected, this, &QDialog::reject);
    m_create->setEnabled(false);
    showIntakes();
    update();
}

QJsonObject TeamSettingsDialog::changes() const
{
    QJsonObject out;
    const QString before = m_settings.value("branch_per_task").toObject().value("template").toString();
    const QString after = m_branch->isChecked() ? m_template->text().trimmed() : QString();
    if (after != before) out.insert("branch_per_task", after.isEmpty() ? QJsonValue(QJsonValue::Null) : QJsonValue(QJsonObject{{"template", after}}));
    const QString base = m_base->text().trimmed();
    if (base != m_settings.value("base_branch").toString()) out.insert("base_branch", base);
    if (m_restart->isChecked() != m_settings.value("restart_crashed").toBool()) out.insert("restart_crashed", m_restart->isChecked());
    return out;
}

QString TeamSettingsDialog::problem() const
{
    if (!m_branch->isChecked()) return {};
    const QString name = m_template->text().trimmed();
    if (!name.contains(QLatin1String("{id}"))) return tr("The branch name needs {id}, so each task gets its own branch");
    if (name.contains(QLatin1Char(' ')) || name.contains(QLatin1String("..")) || name.startsWith(QLatin1Char('/')) || name.endsWith(QLatin1Char('/')))
        return tr("The branch name is not a valid Git branch name");
    return {};
}

void TeamSettingsDialog::update()
{
    m_template->setEnabled(m_branch->isChecked());
    m_base->setEnabled(m_branch->isChecked());
    const QString issue = problem();
    m_problem->setText(issue); m_problem->setVisible(!issue.isEmpty());
    m_save->setEnabled(issue.isEmpty() && !changes().isEmpty());
}

void TeamSettingsDialog::save()
{
    const auto changed = changes();
    if (!problem().isEmpty() || changed.isEmpty()) return;
    m_save->setEnabled(false);
    m_teams->act(QStringLiteral("settings"), {{"team", m_team}, {"settings", changed}}, [guard = QPointer<TeamSettingsDialog>(this)](bool ok, const QString &message) {
        if (!guard) return;
        if (ok) { guard->accept(); return; }
        guard->m_problem->setText(message.isEmpty() ? tr("The settings were not saved") : message); guard->m_problem->show();
        guard->m_save->setEnabled(true);
    });
}

void TeamSettingsDialog::createKey()
{
    const QString name = m_label->text().trimmed();
    if (name.isEmpty()) return;
    m_create->setEnabled(false);
    m_teams->call(QStringLiteral("intake-key"), {{"team", m_team}, {"label", name}},
                  [guard = QPointer<TeamSettingsDialog>(this), name](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard) return;
        if (!ok || result.value("key").toString().isEmpty()) {
            guard->m_problem->setText(error.isEmpty() ? tr("No key was created") : error); guard->m_problem->show();
            guard->m_create->setEnabled(true);
            return;
        }
        guard->m_intakes.append({result.value("id").toString(), result.value("label").toString(name), double(QDateTime::currentSecsSinceEpoch())});
        guard->m_key->setText(result.value("key").toString());
        guard->m_copy->setText(tr("Copy"));
        guard->m_keyBox->show();
        guard->m_label->clear();
        guard->showIntakes();
    });
}

void TeamSettingsDialog::revokeKey(const QString &id, const QString &name)
{
    QMessageBox box(QMessageBox::Warning, tr("Revoke intake key"), tr("Revoke the key \"%1\"?").arg(name), QMessageBox::NoButton, this);
    box.setObjectName("teamSettingsRevokeConfirm");
    box.setInformativeText(tr("A process presenting it can no longer add tasks. This cannot be undone; a new key is a new secret."));
    auto *revoke = box.addButton(tr("Revoke"), QMessageBox::DestructiveRole);
    box.setDefaultButton(box.addButton(QMessageBox::Cancel));
    box.exec();
    if (box.clickedButton() != revoke) return;
    m_teams->act(QStringLiteral("intake-key"), {{"team", m_team}, {"revoke", id}}, [guard = QPointer<TeamSettingsDialog>(this), id](bool ok, const QString &message) {
        if (!guard) return;
        if (!ok) { guard->m_problem->setText(message); guard->m_problem->show(); return; }
        guard->m_intakes.erase(std::remove_if(guard->m_intakes.begin(), guard->m_intakes.end(), [&](const auto &i) { return i.id == id; }), guard->m_intakes.end());
        guard->showIntakes();
    });
}

void TeamSettingsDialog::showIntakes()
{
    const Colors c = colorsFor(m_dark);
    while (auto *item = m_intakeList->takeAt(0)) { if (auto *w = item->widget()) w->deleteLater(); delete item; }
    if (m_intakes.isEmpty()) m_intakeList->addWidget(label(tr("No intake keys: only you and the lead change the plan."), c.muted, 12, false, "teamSettingsNoIntakes"));
    for (const auto &intake : std::as_const(m_intakes)) {
        auto *line = new QWidget; line->setObjectName("teamSettingsIntake"); line->setProperty("intake", intake.id);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
        h->addWidget(label(intake.label, c.fg, 12, true));
        const QString created = intake.created > 0 ? QDateTime::fromSecsSinceEpoch(qint64(intake.created)).toString(QStringLiteral("yyyy-MM-dd HH:mm")) : QString();
        h->addWidget(label(created.isEmpty() ? intake.id : tr("%1, created %2").arg(intake.id, created), c.muted, 11), 1);
        auto *revoke = button(tr("Revoke…"), "teamSettingsIntakeRevoke"); revoke->setProperty("danger", true); revoke->setProperty("ghost", true);
        connect(revoke, &QPushButton::clicked, this, [this, id = intake.id, name = intake.label] { revokeKey(id, name); });
        h->addWidget(revoke);
        m_intakeList->addWidget(line);
    }
    m_create->setEnabled(!m_label->text().trimmed().isEmpty() && m_intakes.size() < 8);
    m_create->setToolTip(m_intakes.size() >= 8 ? tr("A team keeps at most eight keys; revoke one first") : QString());
}
