#include "TeamNewDialog.h"
#include "PathProbe.h"
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamTemplateList.h"
#include "TeamWidgets.h"
#include <QAbstractItemView>
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QFormLayout>
#include <QJsonArray>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPointer>
#include <QScrollArea>
#include <QSpinBox>
#include <QStandardItemModel>
#include <QStyle>
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
// #737: the icons in the theme's text colour (the box's palette is the system's).
void fillAgents(QComboBox *box, const QString &agent, const QColor &text)
{
    for (const auto &a : agents) box->addItem(workspaceIcon(a, text), agentNames.value(a), a);
    box->setCurrentIndex(qMax(0, box->findData(agent.isEmpty() ? QStringLiteral("claude") : agent)));
}
// #738: a model item keeps the efforts its catalog names for it.
constexpr int EffortsRole = Qt::UserRole + 1;
void fillEfforts(QComboBox *box, const QString &effort, const QStringList &efforts = {})
{
    const QSignalBlocker block(box);
    box->clear();
    box->addItem(QStringLiteral("—"), QString());
    const QStringList shown = efforts.isEmpty() ? QStringList{"low", "medium", "high", "xhigh"} : efforts;
    for (const auto &e : shown) box->addItem(e, e);
    box->setCurrentIndex(qMax(0, box->findData(effort)));
}
// The catalogs `account model-settings` reads for these agents; OpenCode's
// comes with its account row.
bool modelSettings(const QString &agent)
{
    return agent == "claude" || agent == "codex" || agent == "kimi" || agent == "dsh";
}
QList<TeamNewDialog::ModelOption> parseModels(const QJsonArray &models)
{
    QList<TeamNewDialog::ModelOption> options;
    for (const auto &value : models) {
        const auto model = value.toObject();
        const QString id = model.value("value").toString(model.value("id").toString());
        if (id.isEmpty() || options.size() >= 256) continue;
        QStringList efforts;
        for (const auto &effort : model.value("effort_options").toArray())
            if (const QString e = effort.toString(); !e.isEmpty() && !efforts.contains(e)) efforts << e;
        options.append({id, model.value("label").toString(), efforts});
    }
    return options;
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
    : QDialog(parent), m_teams(teams), m_client(client), m_computers(computers), m_sessions(sessions), m_project(project),
      m_dark(dialogDark(this, parent))
{
    setObjectName("teamNewDialog");
    setWindowTitle(tr("New team"));
    resize(780, 760);
    // #737: the window's theme, not the system's palette this window would
    // take; a member row with an error only flips its property (validate).
    // The goal and the members' list are not the window's styled fields.
    const Colors theme = colorsFor(m_dark);
    setStyleSheet(dialogSheet(theme) + QStringLiteral(
        "QPlainTextEdit#teamNewGoal{background:%2;color:%3;border:1px solid %4;border-radius:7px;padding:4px 6px;}"
        "QPlainTextEdit#teamNewGoal:focus{border-color:%5;}"
        "QScrollArea#teamNewMembers,QScrollArea#teamNewMembers>QWidget,QScrollArea#teamNewMembers>QWidget>QWidget{background:transparent;border:0;}"
        "QFrame#teamNewMemberRow[error=\"true\"]{background:%1;border-radius:6px;}")
                      .arg(alpha(theme.danger, .08).name(QColor::HexArgb), theme.surface.name(), theme.fg.name(), theme.border.name(), theme.accent.name()));
    auto *outer = new QVBoxLayout(this);
    auto *form = new QFormLayout; form->setLabelAlignment(Qt::AlignRight);
    m_name = new QLineEdit; m_name->setObjectName("teamNewName"); m_name->setPlaceholderText(tr("Checkout redesign"));
    form->addRow(tr("Name"), m_name);
    // F6: a compact list with search and favourites; it tells the form only
    // after the whole form exists (connected at the end of the constructor).
    m_templateList = new TeamTemplateList; m_templateList->setObjectName("teamNewTemplates");
    m_templateList->setDark(m_dark);
    m_templateList->setCatalog(teams->catalog());
    form->addRow(tr("Template"), m_templateList);
    m_goal = new QPlainTextEdit; m_goal->setObjectName("teamNewGoal"); m_goal->setFixedHeight(64);
    m_goal->setPlaceholderText(tr("What the team should deliver"));
    form->addRow(tr("Goal"), m_goal);
    auto *folderRow = new QHBoxLayout;
    m_folder = new QLineEdit(folder); m_folder->setObjectName("teamNewFolder");
    // QA #347: the folder the dialog opens with is given, not being typed: a
    // non-Git one forces the policy to Shared at once (the note says why).
    m_folderCommitted = !folder.isEmpty();
    auto *browse = new QPushButton(tr("Browse…")); browse->setAutoDefault(false);
    connect(browse, &QPushButton::clicked, this, [this] {
        const auto dir = QFileDialog::getExistingDirectory(this, tr("Team folder"), m_folder->text());
        if (!dir.isEmpty()) m_folder->setText(QDir::toNativeSeparators(dir));
    });
    folderRow->addWidget(m_folder, 1); folderRow->addWidget(browse);
    form->addRow(tr("Folder"), folderRow);
    m_worktrees = new QCheckBox(tr("Worktree per member"), this); m_worktrees->setObjectName("teamNewWorktrees"); m_worktrees->setChecked(true);
    m_worktrees->hide();   // B6: the folder policy combo below is the control; the box stays a child (row worktree defaults)
    m_gitNote = new QLabel; m_gitNote->setObjectName("teamNewGitNote"); m_gitNote->setWordWrap(true);
    // B6: auto is the default for a new team - the lead picks shared, worktree
    // or clone per member; a team that must share one folder picks Shared.
    m_folderPolicy = combo("teamNewFolderPolicy", 200);
    m_folderPolicy->addItem(tr("Auto — the lead picks per member"), QStringLiteral("auto"));
    m_folderPolicy->addItem(tr("Shared folder"), QStringLiteral("shared"));
    m_folderPolicy->addItem(tr("Worktree per member"), QStringLiteral("worktree"));
    m_folderPolicy->setToolTip(tr("Auto lets the lead choose the folder each member works in"));
    // N10: typing a path keeps auto; a finished non-Git path forces shared.
    QObject::connect(m_folder, &QLineEdit::editingFinished, this, [this] { m_folderCommitted = true; validate(); });
    QObject::connect(m_folder, &QLineEdit::textChanged, this, [this] {
        m_folderCommitted = false;
        // N10: a path that was being changed again is not finished, so the rule
        // that forced shared no longer applies: back to auto until it is.
        if (m_forcedShared) {
            m_forcedShared = false;
            const QSignalBlocker block(m_folderPolicy);
            m_folderPolicy->setCurrentIndex(m_folderPolicy->findData(QStringLiteral("auto")));
        }
    });
    connect(m_folderPolicy, &QComboBox::currentIndexChanged, this, [this] {
        m_worktrees->setChecked(m_folderPolicy->currentData().toString() != QStringLiteral("shared"));
        validate();
    });
    form->addRow(tr("Member folders"), m_folderPolicy);
    form->addRow(QString(), m_gitNote);
    // The lead: a new session on this computer, or an existing one.
    auto *leadRow = new QHBoxLayout;
    m_leadAgent = combo("teamNewLeadAgent", 130); fillAgents(m_leadAgent, "claude", colorsFor(m_dark).fg);
    m_leadAccount = combo("teamNewLeadAccount", 130);
    // #738: the lead's model takes the row's room; catalog ids such as
    // kimi-code/k3.1-pro-highspeed are long.
    m_leadModel = combo("teamNewLeadModel"); m_leadModel->setMinimumWidth(160); m_leadModel->setEditable(true);
    m_leadModel->lineEdit()->setPlaceholderText(tr("default model"));
    m_leadEffort = combo("teamNewLeadEffort", 80); fillEfforts(m_leadEffort, "high");
    leadRow->addWidget(m_leadAgent); leadRow->addWidget(m_leadAccount); leadRow->addWidget(m_leadModel, 1); leadRow->addWidget(m_leadEffort);
    form->addRow(tr("Lead"), leadRow);
    auto *existingRow = new QHBoxLayout;
    m_useExisting = new QCheckBox(tr("Use an existing session as lead")); m_useExisting->setObjectName("teamNewUseExisting");
    m_existingLead = combo("teamNewExistingLead");
    // F4: the Team designer never leads a team.
    for (const auto &s : sessions) if (s.host.isEmpty() && !isDesignerSession(s.host, s.name)) m_existingLead->addItem(s.name, s.name);
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

    connect(m_name, &QLineEdit::textChanged, this, [this] { validate(); });
    connect(m_folder, &QLineEdit::textChanged, this, [this] { validate(); });
    // #744: the folder's state comes from PathProbe's worker; its answer runs
    // validate() again.
    connect(&PathProbe::instance(), &PathProbe::answered, this, [this](const QString &path) {
        const QString folder = m_folder ? m_folder->text().trimmed() : QString();
        if (!folder.isEmpty() && (path == folder || path == PathProbe::gitOf(folder))) validate();
    });
    connect(m_leadAgent, &QComboBox::currentIndexChanged, this, [this] {
        fillAccounts(m_leadAccount, m_leadAgent->currentData().toString(), {}); refillLeadModels(); validate(); });
    connect(m_leadAccount, &QComboBox::currentIndexChanged, this, [this] { refillLeadModels(); validate(); });
    connect(m_leadModel, &QComboBox::currentTextChanged, this, [this] { fillEffortsFor(m_leadModel, m_leadEffort); });
    connect(m_useExisting, &QCheckBox::toggled, this, [this](bool on) {
        m_existingLead->setEnabled(on);
        for (auto *w : {static_cast<QWidget *>(m_leadAgent), static_cast<QWidget *>(m_leadAccount), static_cast<QWidget *>(m_leadModel), static_cast<QWidget *>(m_leadEffort)})
            w->setEnabled(!on);
        validate();
    });
    connect(m_worktrees, &QCheckBox::toggled, this, [this](bool on) { for (auto *r : m_rowList) r->worktree->setChecked(on); });
    // Accounts per computer, for the account combos and the signed-in check.
    if (m_client) {
        connect(m_client, &HgsClient::accountsReady, this, [this](quint64 request, const QString &host, const QJsonObject &result) {
            // #738: an account's model catalog; only the pickers it serves refill.
            if (m_modelRequests.contains(request)) {
                const QString key = m_modelRequests.take(request);
                m_modelPending.remove(key);
                m_models.insert(key, parseModels(result.value("models").toArray()));
                if (modelKey(m_leadAgent->currentData().toString(), {}, m_leadAccount->currentData().toString()) == key) refillLeadModels();
                for (auto *r : m_rowList)
                    if (modelKey(r->agent->currentData().toString(), r->computer->currentData().toString(), r->account->currentData().toString()) == key)
                        refillRowModels(r);
                return;
            }
            QList<Account> list;
            for (const auto &value : result.value("profiles").toArray()) {
                const auto p = value.toObject();
                const auto status = p.value("account_status").toObject().value("status").toString();
                list.append({p.value("id").toString(), p.value("provider").toString(), p.value("label").toString(), status != "signed_out"});
                // OpenCode lists its models in its account row (model_options).
                if (p.value("provider").toString() == "opencode" && p.value("model_options").isArray())
                    m_models.insert(host + QLatin1Char('\n') + p.value("id").toString(), parseModels(p.value("model_options").toArray()));
            }
            m_accounts.insert(host, list);
            fillAccounts(m_leadAccount, m_leadAgent->currentData().toString(), {});
            for (auto *r : m_rowList) fillAccounts(r->account, r->agent->currentData().toString(), r->computer->currentData().toString(), r->account->currentData().toString());
            refillLeadModels();
            for (auto *r : m_rowList) refillRowModels(r);
            validate();
        });
        connect(m_client, &HgsClient::accountsFailed, this, [this](quint64 request, const QString &, const QString &) {
            // A catalog that could not be read leaves the known models; it is not asked again.
            if (!m_modelRequests.contains(request)) return;
            const QString key = m_modelRequests.take(request);
            m_modelPending.remove(key);
            m_models.insert(key, {});
        });
        m_client->requestAccounts({});
        for (const auto &c : computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
    }
    // A template that the goal's words route to is preselected.
    const auto &routing = teams->catalog().routing;
    Q_UNUSED(routing);
    fillAccounts(m_leadAccount, "claude", {});
    refillLeadModels();
    applyTemplate();
    m_templateList->selected = [this](const QString &id) { m_templateId = id; applyTemplate(); };
}

TeamNewDialog::~TeamNewDialog()
{
    // The list is destroyed after this body; it must not apply a template then.
    if (m_templateList) m_templateList->selected = nullptr;
}

void TeamNewDialog::selectTemplate(const QString &id)
{
    m_templateList->setCurrent(id);
    m_templateId = id;
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

QString TeamNewDialog::modelKey(const QString &agent, const QString &host, const QString &account)
{
    // The default account reads the agent's native profile, as the role editor does.
    const QString a = agent.isEmpty() ? QStringLiteral("claude") : agent;
    return host + QLatin1Char('\n') + (account.isEmpty() ? QStringLiteral("native-") + a : account);
}

void TeamNewDialog::requestModels(const QString &agent, const QString &host, const QString &account)
{
    if (!m_client || !modelSettings(agent)) return;
    const QString key = modelKey(agent, host, account);
    if (m_models.contains(key) || m_modelPending.contains(key)) return;
    m_modelPending.insert(key);
    m_modelRequests.insert(m_client->requestAccounts(host, {"model-settings", key.section(QLatin1Char('\n'), 1), "--json"}), key);
}

void TeamNewDialog::fillModels(QComboBox *model, QComboBox *effort, const QString &agent, const QString &host, const QString &account)
{
    requestModels(agent, host, account);
    {
        const QSignalBlocker block(model);
        const QString keep = model->currentText().trimmed();
        model->clear();
        const auto options = m_models.value(modelKey(agent, host, account));
        if (options.isEmpty()) for (const auto &id : Teams::knownModels(agent)) model->addItem(id, id);
        for (const auto &option : options) {
            if (model->findData(option.id) >= 0) continue;
            model->addItem(option.id, option.id);
            model->setItemData(model->count() - 1, option.efforts, EffortsRole);
            if (!option.label.isEmpty() && option.label != option.id) model->setItemData(model->count() - 1, option.label, Qt::ToolTipRole);
        }
        // The model stays as chosen or typed; none keeps the agent's default.
        model->setCurrentIndex(model->findText(keep));
        if (model->currentIndex() < 0) model->setEditText(keep);
    }
    fillEffortsFor(model, effort);
}

void TeamNewDialog::fillEffortsFor(QComboBox *model, QComboBox *effort)
{
    const int index = model->findText(model->currentText().trimmed());
    const QStringList efforts = index >= 0 ? model->itemData(index, EffortsRole).toStringList() : QStringList();
    fillEfforts(effort, effort->currentData().toString(), efforts);
}

void TeamNewDialog::refillLeadModels()
{
    fillModels(m_leadModel, m_leadEffort, m_leadAgent->currentData().toString(), {}, m_leadAccount->currentData().toString());
}

void TeamNewDialog::refillRowModels(Row *row)
{
    fillModels(row->model, row->effort, row->agent->currentData().toString(), row->computer->currentData().toString(),
               row->account->currentData().toString());
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
    row->agent = combo("teamNewAgent", 96); fillAgents(row->agent, role ? role->agent : QStringLiteral("claude"), colorsFor(m_dark).fg);
    row->computer = combo("teamNewComputer", 90);
    for (const auto &c : m_computers) row->computer->addItem(c.label, c.alias);
    row->computer->setCurrentIndex(qMax(0, row->computer->findData(role ? role->computer : QString())));
    row->account = combo("teamNewAccount", 96);
    fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString(), role ? role->account : QString());
    row->model = combo("teamNewModel", 104); row->model->setEditable(true); row->model->setCurrentText(role ? role->model : QString());
    row->model->lineEdit()->setPlaceholderText(tr("default"));
    row->model->view()->setMinimumWidth(260);   // #738: the catalog's long ids read whole in the list
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
    row->error->setStyleSheet(QStringLiteral("color:%1;").arg(colorsFor(m_dark).danger.name()));
    v->addWidget(row->error);
    m_rows->insertWidget(m_rows->count() - 1, row->box);
    row->box->show();   // rows come and go while the dialog shows
    m_rowList.append(row);
    refillRowModels(row);
    connect(row->agent, &QComboBox::currentIndexChanged, this, [this, row] {
        fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString()); refillRowModels(row); validate(); });
    connect(row->computer, &QComboBox::currentIndexChanged, this, [this, row] {
        fillAccounts(row->account, row->agent->currentData().toString(), row->computer->currentData().toString()); refillRowModels(row); validate(); });
    connect(row->account, &QComboBox::currentIndexChanged, this, [this, row] { refillRowModels(row); validate(); });
    connect(row->model, &QComboBox::currentTextChanged, this, [row] { fillEffortsFor(row->model, row->effort); });
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
    const auto *t = m_teams->catalog().templ(m_templateId);
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

void TeamNewDialog::preselectExistingLead(const QString &host, const QString &name)
{
    if (!host.isEmpty() || name.isEmpty()) return;   // only this computer's sessions can lead here
    const int index = m_existingLead->findData(name);
    if (index < 0) return;
    m_existingLead->setCurrentIndex(index);
    m_useExisting->setChecked(true);
    validate();
}

void TeamNewDialog::validate()
{
    // B6: a combo signal may reach validate before the last widgets exist; the
    // constructor's own call runs after them all.
    if (!m_name || !m_folder || !m_folderPolicy || !m_summary || !m_create) return;
    const Colors c = colorsFor(m_dark);
    const QString folder = m_folder->text().trimmed();
    // #744: never a stat on this thread. Until the worker answers, what depends
    // on Git stays as it is and Create waits.
    const auto probed = PathProbe::instance().folder(folder);
    const bool git = probed.git;
    if (!probed.checking && !probed.unresponsive) {
        m_worktrees->setEnabled(git);
        if (!git) m_worktrees->setChecked(false);
        // N10: a path that is Git again with auto or worktree policy brings the
        // worktree rows back, so members are created with worktrees.
        else if (!m_worktrees->isChecked() && m_folderPolicy
                 && m_folderPolicy->currentData().toString() != QStringLiteral("shared"))
            m_worktrees->setChecked(true);
        // B6: without a Git folder only a shared folder makes sense; the worktree
        // entry is greyed out and the note says why. QA #347: a folder that is not
        // a Git repository shows Shared, fixed, with the reason - also for the
        // folder the dialog opened with, not only after the field lost focus.
        if (m_folderPolicy) {
            if (auto *model = qobject_cast<QStandardItemModel *>(m_folderPolicy->model()))
                if (auto *item = model->item(m_folderPolicy->findData(QStringLiteral("worktree"))))
                    item->setEnabled(git);
            if (!git && m_folderCommitted && m_folderPolicy->currentData().toString() != "shared") {
                // No signals: currentIndexChanged would call validate() again in the
                // middle of this one (the crash seen in newTeamStartsTheLeadWithTheTemplate).
                const QSignalBlocker block(m_folderPolicy);
                m_folderPolicy->setCurrentIndex(m_folderPolicy->findData(QStringLiteral("shared")));
                m_worktrees->setChecked(false);
                m_forcedShared = true;
            }
            const bool sharedOnly = !folder.isEmpty() && !git && m_folderCommitted;
            m_folderPolicy->setEnabled(!sharedOnly);
            m_folderPolicy->setToolTip(sharedOnly ? tr("Folder is not a Git repository: every member works in this folder, so the policy is shared.")
                                                  : tr("Auto lets the lead choose the folder each member works in"));
        }
        m_gitNote->setVisible(!folder.isEmpty() && !git);
        m_gitNote->setText(tr("Folder is not a Git repository — members will share one folder."));
    }
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
        // A style sheet set on the row (even an unchanged or empty one)
        // re-styles every combo in it, popup included, and the Windows 11
        // style closes an open list when it unpolishes its popup. validate
        // runs on every role edit and account reply, also while a list is
        // open, so the row only flips a property and repolishes itself.
        if (r->box->property("error").toBool() != !error.isEmpty()) {
            r->box->setProperty("error", !error.isEmpty());
            r->box->style()->unpolish(r->box); r->box->style()->polish(r->box); r->box->update();
        }
        bad += !error.isEmpty();
    }
    QStringList problems;
    if (m_name->text().trimmed().isEmpty()) problems << tr("Name the team");
    if (folder.isEmpty()) problems << tr("Choose an existing folder");
    else if (probed.checking) problems << tr("Checking the folder…");
    else if (probed.unresponsive) problems << tr("The folder does not answer: its disk may be busy");
    else if (!probed.dir) problems << tr("Choose an existing folder");
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
    const auto *t = m_teams->catalog().templ(m_templateId);
    if (t && m_planning->currentData() == "template") {
        for (int i = 0; i < t->tasks.size(); ++i) {
            const auto &k = t->tasks[i];
            QJsonArray deps; for (int d : k.deps) deps.append(QJsonObject{{"task", d + 1}, {"kind", "hard"}});
            tasks.append(QJsonObject{{"id", i + 1}, {"title", k.title}, {"assignee", k.role == "lead" ? QStringLiteral("lead") : byRole.value(k.role)}, {"deps", deps}});
        }
    }
    const bool worktrees = m_worktrees->isChecked();
    const QString folderPolicy = m_folderPolicy ? m_folderPolicy->currentData().toString() : (worktrees ? QStringLiteral("worktree") : QStringLiteral("shared"));
    QJsonObject settings{{"folder_policy", folderPolicy}, {"approval", m_review->isChecked() ? "review" : "auto"},
                         {"max_members", m_maxMembers->value()}};
    if (t && !t->phaseApproval.isEmpty()) settings.insert("phase_approval", t->phaseApproval);
    QJsonObject input{{"name", m_name->text().trimmed()}, {"goal", m_goal->toPlainText().trimmed()}, {"folder", m_folder->text().trimmed()},
                      {"project", m_project}, {"template", t ? t->id : QString()}, {"lead", lead}, {"members", members}, {"tasks", tasks},
                      {"start", true}, {"settings", settings}};
    m_create->setEnabled(false);
    m_summary->setText(tr("Creating the team…"));
    m_teams->call(QStringLiteral("create"), input, [this, guard = QPointer<TeamNewDialog>(this)](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard) return;
        if (!ok) { m_summary->setText(QStringLiteral("⚠ ") + error); m_create->setEnabled(true); return; }
        m_created = result.value("team").toString();
        accept();
    });
}
