#include "TeamSettingsDialog.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QApplication>
#include <QCheckBox>
#include <QClipboard>
#include <QComboBox>
#include <QDateTime>
#include <QDialogButtonBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QPointer>
#include <QPushButton>
#include <QScrollArea>
#include <QSignalBlocker>
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
    // #737: the theme, not the system's palette, behind the sections too.
    setStyleSheet(dialogSheet(c) + QStringLiteral("QScrollArea#teamSettingsScroll,QScrollArea#teamSettingsScroll>QWidget,"
                                                  "QScrollArea#teamSettingsScroll>QWidget>QWidget{background:transparent;border:0;}"));
    setWindowTitle(tr("Team settings"));
    setModal(true);
    resize(560, 640);
    auto *root = new QVBoxLayout(this); root->setContentsMargins(16, 14, 16, 12); root->setSpacing(8);
    root->addWidget(label(tr("Settings of %1").arg(team.name), c.fg, 14, true, "teamSettingsTitle"));
    // The sections scroll; the problem line and the buttons stay in view.
    auto *scroll = new QScrollArea; scroll->setObjectName("teamSettingsScroll"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->viewport()->setAutoFillBackground(false);
    auto *body = new QWidget; body->setObjectName("teamSettingsBody"); body->setAutoFillBackground(false);
    auto *v = new QVBoxLayout(body); v->setContentsMargins(0, 0, 6, 0); v->setSpacing(8);
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
    // U48, #581 N5: a land stage runs only where the owner allows landing,
    // because its command may push; turning it on asks first.
    v->addWidget(label(tr("Landing"), c.text2, 12, true));
    m_land = new QCheckBox(tr("Let land stages land tasks (land_allowed)")); m_land->setObjectName("teamSettingsLandAllowed");
    m_land->setChecked(m_settings.value("land_allowed").toBool());
    v->addWidget(m_land);
    v->addWidget(note(tr("A pipeline's land stage runs the catalog's command on the task's commit, and that command may merge or push. "
                         "Off: the plan refuses a preset with a land stage, and a land stage reached anyway holds the task for the lead."), c.muted));
    // U52: a member session that exits on its own is launched again.
    v->addWidget(label(tr("Members"), c.text2, 12, true));
    m_restart = new QCheckBox(tr("Restart a member whose session crashed")); m_restart->setObjectName("teamSettingsRestartCrashed");
    m_restart->setChecked(m_settings.value("restart_crashed").toBool());
    v->addWidget(m_restart);
    v->addWidget(note(tr("At most three times an hour, into the same folder and conversation; its open task goes back to the lead as unconfirmed. "
                         "A pause, an archive or a usage limit never restarts it."), c.muted));
    // #756: the build broker for members sharing one folder (#752), off by default.
    v->addWidget(label(tr("Builds"), c.text2, 12, true));
    m_broker = new QCheckBox(tr("Build broker for members sharing a folder")); m_broker->setObjectName("teamSettingsBuildBroker");
    m_broker->setChecked(m_settings.value("build_broker").toBool());
    v->addWidget(m_broker);
    v->addWidget(note(tr("Each member builds its own snapshot of the shared folder, one build at a time per build folder, and an unchanged tree builds once."), c.muted, "teamSettingsBuildBrokerNote"));
    // U45, #581 N6: the models each member may switch to before a task; only
    // the owner edits them, a running member included.
    v->addWidget(label(tr("Member models"), c.text2, 12, true));
    v->addWidget(note(tr("Before a task that names one of these, Zerus switches the member to it. None: only the model it launched with. "
                         "A change applies from the member's next task."), c.muted));
    for (const auto &member : team.members) {
        if (member.lead || member.state == QLatin1String("removed")) continue;
        MemberModels entry{member.id, team.roleOf(member.id), member.agent, member.model, member.models, member.models};
        auto *line = new QWidget; line->setObjectName("teamSettingsMemberModels"); line->setProperty("member", member.id);
        auto *mv = new QVBoxLayout(line); mv->setContentsMargins(0, 2, 0, 2); mv->setSpacing(4);
        auto *head = new QHBoxLayout; head->setSpacing(8);
        head->addWidget(label(entry.role, c.fg, 12, true));
        head->addWidget(label(entry.model.isEmpty() ? tr("launches with its agent's model") : tr("launches with %1").arg(entry.model), c.muted, 11), 1);
        mv->addLayout(head);
        entry.chipsBox = new QWidget; entry.chips = new FlowLayout(6); entry.chipsBox->setLayout(entry.chips);
        mv->addWidget(entry.chipsBox);
        auto *add = new QHBoxLayout; add->setSpacing(6);
        auto *model = new QComboBox; model->setObjectName("teamSettingsMemberModelAdd"); model->setEditable(true); model->setInsertPolicy(QComboBox::NoInsert);
        model->addItems(Teams::knownModels(member.agent));
        model->setCurrentIndex(-1); model->lineEdit()->setPlaceholderText(tr("+ model")); model->setFixedWidth(220);
        auto *effort = new QComboBox; effort->setObjectName("teamSettingsMemberModelEffort");
        effort->addItem(tr("its effort"), QString());
        for (const char *level : {"low", "medium", "high", "xhigh"}) effort->addItem(QString::fromLatin1(level), QString::fromLatin1(level));
        auto *addButton = button(tr("Add"), "teamSettingsMemberModelAddButton"); addButton->setProperty("ghost", true);
        add->addWidget(model); add->addWidget(effort); add->addWidget(addButton); add->addStretch();
        mv->addLayout(add);
        v->addWidget(line);
        const int index = int(m_members.size());
        m_members.append(entry);
        const auto addModel = [this, index, model, effort] {
            const Teams::ModelChoice next{model->currentText().trimmed(), effort->currentData().toString()};
            auto &list = m_members[index].now;
            if (next.model.isEmpty() || next.model.contains(QLatin1Char(' ')) || list.contains(next) || list.size() >= 20) return;
            list.append(next);
            model->setCurrentIndex(-1); model->clearEditText();
            showMemberModels(index); update();
        };
        connect(addButton, &QPushButton::clicked, this, addModel);
        connect(model->lineEdit(), &QLineEdit::returnPressed, this, addModel);
        showMemberModels(index);
    }
    if (m_members.isEmpty()) v->addWidget(label(tr("No members yet."), c.muted, 12));
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
    scroll->setWidget(body);
    root->addWidget(scroll, 1);
    m_problem = note(QString(), c.danger, "teamSettingsProblem"); m_problem->hide();
    root->addWidget(m_problem);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Cancel);
    m_save = buttons->addButton(tr("Save"), QDialogButtonBox::AcceptRole); m_save->setObjectName("teamSettingsSave"); m_save->setDefault(true);
    buttons->button(QDialogButtonBox::Cancel)->setObjectName("teamSettingsCancel");
    root->addWidget(buttons);
    connect(m_branch, &QCheckBox::toggled, this, [this] { update(); });
    connect(m_restart, &QCheckBox::toggled, this, [this] { update(); });
    connect(m_broker, &QCheckBox::toggled, this, [this] { update(); });
    // Turning landing on asks first; turning it off, or back to what the owner
    // already allowed, does not.
    connect(m_land, &QCheckBox::clicked, this, [this](bool on) {
        if (on && !m_settings.value("land_allowed").toBool() && !confirmLanding()) { const QSignalBlocker block(m_land); m_land->setChecked(false); }
        update();
    });
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
    if (m_land->isChecked() != m_settings.value("land_allowed").toBool()) out.insert("land_allowed", m_land->isChecked());
    if (m_broker->isChecked() != m_settings.value("build_broker").toBool()) out.insert("build_broker", m_broker->isChecked());
    return out;
}

QJsonArray TeamSettingsDialog::memberChanges() const
{
    QJsonArray out;
    for (const auto &member : m_members)
        if (member.now != member.before) out.append(QJsonObject{{"id", member.id}, {"models", Teams::modelChoicesJson(member.now)}});
    return out;
}

bool TeamSettingsDialog::confirmLanding()
{
    QMessageBox box(QMessageBox::Warning, tr("Allow landing"), tr("Let land stages land this team's tasks?"), QMessageBox::NoButton, this);
    box.setObjectName("teamSettingsLandConfirm");
    box.setInformativeText(tr("A land stage runs the catalog's command on the task's commit without asking again, and that command may merge or push. "
                              "Its exit code decides whether the task is accepted, returned or held for the lead."));
    auto *allow = box.addButton(tr("Allow landing"), QMessageBox::AcceptRole);
    box.setDefaultButton(box.addButton(QMessageBox::Cancel));
    box.exec();
    return box.clickedButton() == allow;
}

void TeamSettingsDialog::showMemberModels(int index)
{
    const Colors c = colorsFor(m_dark);
    auto &member = m_members[index];
    while (auto *item = member.chips->takeAt(0)) { if (auto *w = item->widget()) w->deleteLater(); delete item; }
    for (int i = 0; i < member.now.size(); ++i) {
        auto *chip = button(member.now[i].text() + QStringLiteral("  ×"), "teamSettingsMemberModel"); chip->setProperty("chip", true);
        chip->setProperty("model", member.now[i].text());
        chip->setToolTip(tr("Remove %1").arg(member.now[i].text()));
        connect(chip, &QPushButton::clicked, this, [this, index, i] { m_members[index].now.removeAt(i); showMemberModels(index); update(); });
        member.chips->addWidget(chip);
    }
    if (member.now.isEmpty()) member.chips->addWidget(label(tr("Only its launch model"), c.muted, 11, false, "teamSettingsMemberModelsNone"));
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
    m_save->setEnabled(issue.isEmpty() && (!changes().isEmpty() || !memberChanges().isEmpty()));
}

void TeamSettingsDialog::save()
{
    const auto changed = changes();
    const auto members = memberChanges();
    if (!problem().isEmpty() || (changed.isEmpty() && members.isEmpty())) return;
    m_save->setEnabled(false);
    // One call: the team's settings and the members' models, each only when changed.
    QJsonObject input{{"team", m_team}};
    if (!changed.isEmpty()) input.insert("settings", changed);
    if (!members.isEmpty()) input.insert("members_update", members);
    m_teams->act(QStringLiteral("settings"), input, [guard = QPointer<TeamSettingsDialog>(this)](bool ok, const QString &message) {
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
