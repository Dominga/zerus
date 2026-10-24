#include "TeamNewDialog.h"
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPointer>
#include <QScrollArea>
#include <QSpinBox>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
QComboBox *combo(const char *name, int width = 0)
{
    auto *c = new QComboBox; c->setObjectName(name);
    if (width) c->setFixedWidth(width);
    return c;
}
void fillAgents(QComboBox *box, const QString &agent)
{
    for (const auto &a : agents) box->addItem(workspaceIcon(a, box->palette().color(QPalette::Text)), agentNames.value(a), a);
    box->setCurrentIndex(qMax(0, box->findData(agent.isEmpty() ? QStringLiteral("claude") : agent)));
}
void fillEfforts(QComboBox *box, const QString &effort)
{
    box->addItem(QStringLiteral("—"), QString());
    for (const auto &e : {"low", "medium", "high", "xhigh"}) box->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
    box->setCurrentIndex(qMax(0, box->findData(effort)));
}
void fillPermissions(QComboBox *box, const QString &permissions)
{
    box->addItem(QCoreApplication::translate("TeamNewDialog", "Account"), QString());
    box->addItem(QCoreApplication::translate("TeamNewDialog", "Auto"), QStringLiteral("auto"));
    box->addItem(QCoreApplication::translate("TeamNewDialog", "Bypass"), QStringLiteral("bypass"));
    box->setCurrentIndex(qMax(0, box->findData(permissions)));
}
}

TeamNewDialog::TeamNewDialog(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const QList<Session> &sessions,
                             const QString &folder, const QString &project, QWidget *parent)
    : QDialog(parent), m_teams(teams), m_client(client), m_computers(computers), m_sessions(sessions), m_project(project)
{
    setObjectName("teamNewDialog");
    setWindowTitle(tr("New team"));
    resize(780, 760);
    auto *outer = new QVBoxLayout(this);
    auto *form = new QFormLayout; form->setLabelAlignment(Qt::AlignRight);
    m_name = new QLineEdit; m_name->setObjectName("teamNewName"); m_name->setPlaceholderText(tr("Checkout redesign"));
    form->addRow(tr("Name"), m_name);
    m_template = combo("teamNewTemplate");
    m_template->addItem(tr("No template"), QString());
    for (const auto &t : teams->catalog().templates) m_template->addItem(t.name, t.id);
    form->addRow(tr("Template"), m_template);
    m_goal = new QPlainTextEdit; m_goal->setObjectName("teamNewGoal"); m_goal->setFixedHeight(64);
    m_goal->setPlaceholderText(tr("What the team should deliver"));
    form->addRow(tr("Goal"), m_goal);
    auto *folderRow = new QHBoxLayout;
    m_folder = new QLineEdit(folder); m_folder->setObjectName("teamNewFolder");
    auto *browse = new QPushButton(tr("Browse…")); browse->setAutoDefault(false);
    connect(browse, &QPushButton::clicked, this, [this] {
        const auto dir = QFileDialog::getExistingDirectory(this, tr("Team folder"), m_folder->text());
        if (!dir.isEmpty()) m_folder->setText(QDir::toNativeSeparators(dir));
    });
    folderRow->addWidget(m_folder, 1); folderRow->addWidget(browse);
    form->addRow(tr("Folder"), folderRow);
    m_worktrees = new QCheckBox(tr("Worktree per member")); m_worktrees->setObjectName("teamNewWorktrees"); m_worktrees->setChecked(true);
    m_gitNote = new QLabel; m_gitNote->setObjectName("teamNewGitNote"); m_gitNote->setWordWrap(true);
    form->addRow(QString(), m_worktrees);
    form->addRow(QString(), m_gitNote);
    // The lead: a new session on this computer, or an existing one.
    auto *leadRow = new QHBoxLayout;
    m_leadAgent = combo("teamNewLeadAgent", 130); fillAgents(m_leadAgent, "claude");
    m_leadAccount = combo("teamNewLeadAccount", 130);
    m_leadModel = combo("teamNewLeadModel", 120); m_leadModel->setEditable(true); m_leadModel->lineEdit()->setPlaceholderText(tr("default model"));
    m_leadEffort = combo("teamNewLeadEffort", 80); fillEfforts(m_leadEffort, "high");
    leadRow->addWidget(m_leadAgent); leadRow->addWidget(m_leadAccount); leadRow->addWidget(m_leadModel, 1); leadRow->addWidget(m_leadEffort);
    form->addRow(tr("Lead"), leadRow);
    auto *existingRow = new QHBoxLayout;
    m_useExisting = new QCheckBox(tr("Use an existing session as lead")); m_useExisting->setObjectName("teamNewUseExisting");
    m_existingLead = combo("teamNewExistingLead");
    for (const auto &s : sessions) if (s.host.isEmpty()) m_existingLead->addItem(s.name, s.name);
    m_useExisting->setEnabled(m_existingLead->count() > 0);
    m_existingLead->setEnabled(false);
    existingRow->addWidget(m_useExisting); existingRow->addWidget(m_existingLead, 1);
    form->addRow(QString(), existingRow);
    outer->addLayout(form);
    // Members.
    auto *membersHead = new QHBoxLayout;
    auto *membersTitle = new QLabel(tr("Members")); QFont f = membersTitle->font(); f.setWeight(QFont::DemiBold); membersTitle->setFont(f);
    membersHead->addWidget(membersTitle); membersHead->addStretch();
    m_maxMembers = new QSpinBox; m_maxMembers->setObjectName("teamNewMaxMembers"); m_maxMembers->setRange(1, 32); m_maxMembers->setValue(6);
    membersHead->addWidget(new QLabel(tr("limit"))); membersHead->addWidget(m_maxMembers);
    auto *addMember = new QPushButton(tr("+ Member")); addMember->setObjectName("teamNewAddMember"); addMember->setAutoDefault(false);
    connect(addMember, &QPushButton::clicked, this, [this] { addRow(nullptr, tr("Member")); validate(); });
    membersHead->addWidget(addMember);
    outer->addLayout(membersHead);
    auto *rowsBox = new QWidget; m_rows = new QVBoxLayout(rowsBox); m_rows->setContentsMargins(0, 0, 0, 0); m_rows->setSpacing(6); m_rows->addStretch();
    auto *scroll = new QScrollArea; scroll->setObjectName("teamNewMembers"); scroll->setWidget(rowsBox); scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    outer->addWidget(scroll, 1);
    auto *options = new QFormLayout; options->setLabelAlignment(Qt::AlignRight);
    m_planning = combo("teamNewPlanning");
    m_planning->addItem(tr("Lead plans"), QStringLiteral("lead")); m_planning->addItem(tr("Use template tasks"), QStringLiteral("template"));
    options->addRow(tr("Planning"), m_planning);
    m_review = new QCheckBox(tr("Review plan before running")); m_review->setObjectName("teamNewReview"); m_review->setChecked(true);
    options->addRow(tr("Approval"), m_review);
    outer->addLayout(options);
    // Footer.
    auto *footer = new QHBoxLayout;
    m_summary = new QLabel; m_summary->setObjectName("teamNewSummary"); m_summary->setWordWrap(true);
    footer->addWidget(m_summary, 1);
    auto *cancel = new QPushButton(tr("Cancel")); cancel->setAutoDefault(false);
    connect(cancel, &QPushButton::clicked, this, &QDialog::reject);
    m_create = new QPushButton(tr("Create team")); m_create->setObjectName("teamNewCreate"); m_create->setDefault(true);
    connect(m_create, &QPushButton::clicked, this, &TeamNewDialog::create);
    footer->addWidget(cancel); footer->addWidget(m_create);
    outer->addLayout(footer);

    connect(m_template, &QComboBox::currentIndexChanged, this, [this] { applyTemplate(); });
    connect(m_name, &QLineEdit::textChanged, this, [this] { validate(); });
    connect(m_folder, &QLineEdit::textChanged, this, [this] { validate(); });
    connect(m_leadAgent, &QComboBox::currentIndexChanged, this, [this] { fillAccounts(m_leadAccount, m_leadAgent->currentData().toString(), {}); validate(); });
    connect(m_leadAccount, &QComboBox::currentIndexChanged, this, [this] { validate(); });
    connect(m_useExisting, &QCheckBox::toggled, this, [this](bool on) {
        m_existingLead->setEnabled(on);
        for (auto *w : {static_cast<QWidget *>(m_leadAgent), static_cast<QWidget *>(m_leadAccount), static_cast<QWidget *>(m_leadModel), static_cast<QWidget *>(m_leadEffort)})
            w->setEnabled(!on);
        validate();
    });
    connect(m_worktrees, &QCheckBox::toggled, this, [this](bool on) { for (auto *r : m_rowList) r->worktree->setChecked(on); });
    // Accounts per computer, for the account combos and the signed-in check.
    if (m_client) {
        connect(m_client, &HgsClient::accountsReady, this, [this](quint64, const QString &host, const QJsonObject &result) {
            QList<Account> list;
            for (const auto &value : result.value("profiles").toArray()) {
                const auto p = value.toObject();
                const auto status = p.value("account_status").toObject().value("status").toString();
                list.append({p.value("id").toString(), p.value("provider").toString(), p.value("label").toString(), status != "signed_out"});
            }
            m_accounts.insert(host, list);
            fillAccounts(m_leadAccount, m_leadAgent->currentData().toString(), {});
            for (auto *r : m_rowList) fillAccounts(r->account, r->agent->currentData().toString(), r->computer->currentData().toString(), r->account->currentData().toString());
            validate();
        });
        m_client->requestAccounts({});
        for (const auto &c : computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
    }
    // A template that the goal's words route to is preselected.
    const auto &routing = teams->catalog().routing;
    Q_UNUSED(routing);
    fillAccounts(m_leadAccount, "claude", {});
    applyTemplate();
}

void TeamNewDialog::fillAccounts(QComboBox *box, const QString &agent, const QString &host, const QString &preferred)
{
    const QSignalBlocker block(box);
    const QString keep = preferred.isEmpty() ? box->currentData().toString() : preferred;
    box->clear();
    box->addItem(tr("Default account"), QString());
    for (const auto &a : m_accounts.value(host))
        if (a.provider == agent) box->addItem(a.label.isEmpty() ? a.id : a.label + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
    box->setCurrentIndex(qMax(0, box->findData(keep)));
}

TeamNewDialog::Row *TeamNewDialog::addRow(const Role *role, const QString &roleName)
{
    auto *row = new Row;
    row->box = new QFrame; row->box->setObjectName("teamNewMemberRow");
    auto *v = new QVBoxLayout(row->box); v->setContentsMargins(6, 4, 6, 4); v->setSpacing(2);
    auto *h = new QHBoxLayout; h->setSpacing(4);
    row->role = combo("teamNewRole", 104); row->role->setEditable(true);
    for (const auto &r : m_teams->catalog().roles) if (r.id != "lead") row->role->addItem(r.name, r.id);
    row->role->setCurrentText(role ? role->name : roleName);
    row->agent = combo("teamNewAgent", 96); fillAgents(row->agent, role ? role->agent : QStringLiteral("claude"));
    row->computer = combo("teamNewComputer", 90);
    for (const auto &c : m_computers) row->computer->addItem(c.label, c.alias);
    row->computer->setCurrentIndex(qMax(0, row->computer->findData(role ? role->computer : QString())));
    row->account = combo("teamNewAccount", 96);
    fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString(), role ? role->account : QString());
    row->model = combo("teamNewModel", 104); row->model->setEditable(true); row->model->setCurrentText(role ? role->model : QString());
    row->model->lineEdit()->setPlaceholderText(tr("default"));
    row->effort = combo("teamNewEffort", 66); fillEfforts(row->effort, role ? role->effort : QString());
    row->permissions = combo("teamNewPermissions", 70); fillPermissions(row->permissions, role ? role->permissions : QString());
    row->worktree = new QCheckBox; row->worktree->setObjectName("teamNewWorktree"); row->worktree->setToolTip(tr("Own worktree"));
    row->worktree->setChecked(m_worktrees->isChecked() && (!role || role->folder != "read_only" && role->folder != "shared"));
    row->prompt = role ? role->prompt : QString();
    auto *dup = new QToolButton; dup->setText(QStringLiteral("⧉")); dup->setToolTip(tr("Duplicate")); dup->setAutoRaise(true);
    auto *del = new QToolButton; del->setText(QStringLiteral("✕")); del->setToolTip(tr("Remove")); del->setAutoRaise(true); del->setObjectName("teamNewRemove");
    for (auto *w : std::initializer_list<QWidget *>{row->role, row->agent, row->account, row->model, row->effort, row->permissions, row->computer, row->worktree, dup, del})
        h->addWidget(w);
    v->addLayout(h);
    row->error = new QLabel; row->error->setObjectName("teamNewRowError"); row->error->setWordWrap(true); row->error->hide();
    v->addWidget(row->error);
    m_rows->insertWidget(m_rows->count() - 1, row->box);
    row->box->show();   // rows come and go while the dialog shows
    m_rowList.append(row);
    connect(row->agent, &QComboBox::currentIndexChanged, this, [this, row] {
        fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString()); validate(); });
    connect(row->computer, &QComboBox::currentIndexChanged, this, [this, row] {
        fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString()); validate(); });
    connect(row->account, &QComboBox::currentIndexChanged, this, [this] { validate(); });
    connect(row->role, &QComboBox::currentTextChanged, this, [this, row](const QString &text) {
        if (const auto *r = m_teams->catalog().roleNamed(text)) row->prompt = r->prompt;
        validate();
    });
    connect(dup, &QToolButton::clicked, this, [this, row] {
        auto *copy = addRow(nullptr, row->role->currentText());
        copy->agent->setCurrentIndex(row->agent->currentIndex()); copy->computer->setCurrentIndex(row->computer->currentIndex());
        copy->account->setCurrentIndex(row->account->currentIndex()); copy->model->setCurrentText(row->model->currentText());
        copy->effort->setCurrentIndex(row->effort->currentIndex()); copy->permissions->setCurrentIndex(row->permissions->currentIndex());
        copy->worktree->setChecked(row->worktree->isChecked()); copy->prompt = row->prompt;
        validate();
    });
    connect(del, &QToolButton::clicked, this, [this, row] {
        m_rowList.removeAll(row); row->box->deleteLater(); delete row; validate();
    });
    return row;
}

void TeamNewDialog::applyTemplate()
{
    for (auto *r : m_rowList) { r->box->deleteLater(); delete r; }
    m_rowList.clear();
    const auto *t = m_teams->catalog().templ(m_template->currentData().toString());
    if (t) {
        for (const auto &seat : t->roster) {
            const auto *role = m_teams->catalog().role(seat.role);
            for (int i = 0; i < seat.count; ++i) {
                auto *row = addRow(role, seat.role);
                if (seat.optional) row->box->setToolTip(tr("Optional: keep it only when the goal needs this role"));
            }
        }
        m_planning->setCurrentIndex(qMax(0, m_planning->findData(t->planning.isEmpty() ? QStringLiteral("lead") : t->planning)));
        m_review->setChecked(t->approval != "auto");
        if (t->maxMembers > 0) m_maxMembers->setValue(t->maxMembers);
        if (const auto *lead = m_teams->catalog().role("lead")) {
            m_leadAgent->setCurrentIndex(qMax(0, m_leadAgent->findData(lead->agent.isEmpty() ? QStringLiteral("claude") : lead->agent)));
            m_leadModel->setCurrentText(lead->model);
        }
    }
    validate();
}

void TeamNewDialog::validate()
{
    const Colors c = colorsFor(palette().color(QPalette::Window).lightness() < 128);
    const QString folder = m_folder->text().trimmed();
    const bool git = !folder.isEmpty() && QDir(folder).exists(QStringLiteral(".git"));
    m_worktrees->setEnabled(git);
    if (!git) m_worktrees->setChecked(false);
    m_gitNote->setVisible(!folder.isEmpty() && !git);
    m_gitNote->setText(tr("Folder is not a Git repository — members will share one folder."));
    m_gitNote->setStyleSheet(QStringLiteral("color:%1;").arg(c.warn.name()));
    int bad = 0;
    for (auto *r : m_rowList) {
        r->worktree->setEnabled(git);
        if (!git) r->worktree->setChecked(false);
        QString error;
        const QString host = r->computer->currentData().toString();
        for (const auto &computer : m_computers) if (computer.alias == host && !computer.online) error = tr("Computer %1 is offline").arg(computer.label);
        const QString account = r->account->currentData().toString();
        for (const auto &a : m_accounts.value(host)) if (a.id == account && !a.signedIn) error = tr("Account “%1” is not signed in").arg(a.label.isEmpty() ? a.id : a.label);
        if (r->role->currentText().trimmed().isEmpty()) error = tr("The member needs a role");
        r->error->setText(QStringLiteral("⚠ ") + error);
        r->error->setVisible(!error.isEmpty());
        r->error->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name()));
        r->box->setStyleSheet(error.isEmpty() ? QString() : QStringLiteral("QFrame#teamNewMemberRow{background:%1;border-radius:6px;}").arg(alpha(c.danger, .08).name(QColor::HexArgb)));
        bad += !error.isEmpty();
    }
    QStringList problems;
    if (m_name->text().trimmed().isEmpty()) problems << tr("Name the team");
    if (folder.isEmpty() || !QDir(folder).exists()) problems << tr("Choose an existing folder");
    if (bad) problems << tr("Fix %n member row(s) to create the team", nullptr, bad);
    if (m_rowList.size() + 1 > m_maxMembers->value()) problems << tr("More members than the limit");
    m_summary->setText(problems.isEmpty() ? tr("Create starts the lead; it plans, and you approve before members start.") : QStringLiteral("⚠ ") + problems.join(QStringLiteral(" · ")));
    m_summary->setStyleSheet(QStringLiteral("color:%1;").arg((problems.isEmpty() ? c.muted : c.warn).name()));
    m_create->setEnabled(problems.isEmpty());
    m_create->setToolTip(problems.join('\n'));
}

QJsonObject TeamNewDialog::memberJson(const Row &row, const QString &id) const
{
    return {{"id", id}, {"role", row.role->currentText().trimmed()}, {"agent", row.agent->currentData().toString()},
            {"account", row.account->currentData().toString()}, {"model", row.model->currentText().trimmed()},
            {"effort", row.effort->currentData().toString()}, {"permissions", row.permissions->currentData().toString()},
            {"computer", row.computer->currentData().toString()}, {"folder", row.worktree->isChecked() ? "worktree" : "shared"},
            {"prompt", row.prompt}};
}

void TeamNewDialog::create()
{
    validate();
    if (!m_create->isEnabled()) return;
    QJsonObject lead{{"id", "lead"}, {"role", "Lead"}};
    if (m_useExisting->isChecked()) {
        const QString name = m_existingLead->currentData().toString();
        QString agent;
        for (const auto &s : m_sessions) if (s.host.isEmpty() && s.name == name) agent = s.agent;
        lead.insert("agent", agent.isEmpty() ? name.section('/', 0, 0) : agent);
        lead.insert("session", QJsonObject{{"host", ""}, {"name", name}});
    } else {
        lead.insert("agent", m_leadAgent->currentData().toString());
        lead.insert("account", m_leadAccount->currentData().toString());
        lead.insert("model", m_leadModel->currentText().trimmed());
        lead.insert("effort", m_leadEffort->currentData().toString());
    }
    if (const auto *role = m_teams->catalog().role("lead")) lead.insert("prompt", role->prompt);
    QJsonArray members;
    Team draft; draft.lead = "lead";
    Member leadMember; leadMember.id = "lead"; draft.members.append(leadMember);
    QMap<QString, QString> byRole;   // catalog role id -> first member id
    for (auto *r : m_rowList) {
        const QString id = nextMemberId(draft, r->role->currentText());
        Member m; m.id = id; draft.members.append(m);
        members.append(memberJson(*r, id));
        const auto *role = m_teams->catalog().roleNamed(r->role->currentText());
        if (role && !byRole.contains(role->id)) byRole.insert(role->id, id);
    }
    QJsonArray tasks;
    const auto *t = m_teams->catalog().templ(m_template->currentData().toString());
    if (t && m_planning->currentData() == "template") {
        for (int i = 0; i < t->tasks.size(); ++i) {
            const auto &k = t->tasks[i];
            QJsonArray deps; for (int d : k.deps) deps.append(QJsonObject{{"task", d + 1}, {"kind", "hard"}});
            tasks.append(QJsonObject{{"id", i + 1}, {"title", k.title}, {"assignee", k.role == "lead" ? QStringLiteral("lead") : byRole.value(k.role)}, {"deps", deps}});
        }
    }
    const bool worktrees = m_worktrees->isChecked();
    QJsonObject input{{"name", m_name->text().trimmed()}, {"goal", m_goal->toPlainText().trimmed()}, {"folder", m_folder->text().trimmed()},
                      {"project", m_project}, {"template", t ? t->id : QString()}, {"lead", lead}, {"members", members}, {"tasks", tasks},
                      {"start", true},
                      {"settings", QJsonObject{{"folder_policy", worktrees ? "worktree" : "shared"}, {"approval", m_review->isChecked() ? "review" : "auto"},
                                               {"max_members", m_maxMembers->value()}}}};
    m_create->setEnabled(false);
    m_summary->setText(tr("Creating the team…"));
    m_teams->call(QStringLiteral("create"), input, [this, guard = QPointer<TeamNewDialog>(this)](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard) return;
        if (!ok) { m_summary->setText(QStringLiteral("⚠ ") + error); m_create->setEnabled(true); return; }
        m_created = result.value("team").toString();
        accept();
    });
}
