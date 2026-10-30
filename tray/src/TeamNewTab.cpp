#include "TeamNewTab.h"
#include "PathProbe.h"
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamTemplateList.h"
#include "TeamWidgets.h"
#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QGridLayout>
#include <QJsonArray>
#include <QLineEdit>
#include <QLocale>
#include <QMenu>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPointer>
#include <QProgressBar>
#include <QRadioButton>
#include <QRegularExpression>
#include <QScrollArea>
#include <QSpinBox>
#include <QSplitter>
#include <QStackedWidget>
#include <QTabWidget>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
// The members read as a table from this settings column width, as cards below.
constexpr int tableWidth = 720;
// The draft's fields, in the order an update names them (spec 3).
const QStringList draftFields{"name", "goal", "addendum", "settings", "members", "tasks"};

QComboBox *combo(const char *name)
{
    auto *c = new QComboBox; c->setObjectName(name); c->setMinimumWidth(60);
    c->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); c->setMinimumContentsLength(4);
    return c;
}

QLabel *fieldLabel(const QString &text)
{
    auto *l = new QLabel(text); l->setObjectName("teamNewLabel"); l->setFixedWidth(88);
    return l;
}

QString shortName(const QString &goal)
{
    QString line = goal.section('\n', 0, 0).trimmed();
    line = line.section(':', 0, 0).trimmed();
    const auto words = line.split(' ', Qt::SkipEmptyParts);
    line = words.mid(0, 5).join(' ');
    return line.left(60);
}
}

int TeamNewTab::s_pollInterval = 2000;

void TeamNewTab::setDraftPollInterval(int milliseconds) { s_pollInterval = milliseconds; }

TeamNewTab::TeamNewTab(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const Coordinator &coordinator,
                       const QString &project, QWidget *parent)
    : QWidget(parent), m_teams(teams), m_client(client), m_computers(computers), m_coordinator(coordinator), m_project(project)
{
    setObjectName("teamNewDialog");   // the form, as New team's dialog was
    setAttribute(Qt::WA_StyledBackground);
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    m_settingsPane = buildSettings();
    m_leadPane = buildLead();
    // Two columns side by side; a narrow tab shows them as two tabs.
    m_columns = new QSplitter(Qt::Horizontal); m_columns->setObjectName("teamNewColumns"); m_columns->setChildrenCollapsible(false);
    m_columns->addWidget(m_settingsPane); m_columns->addWidget(m_leadPane);
    m_columns->setStretchFactor(0, 3); m_columns->setStretchFactor(1, 2);
    // Each column keeps a usable width; below both the tab shows them as tabs.
    m_settingsPane->setMinimumWidth(settingsMinimum); m_leadPane->setMinimumWidth(leadMinimum);
    m_narrow = new QTabWidget; m_narrow->setObjectName("teamNewNarrow"); m_narrow->setDocumentMode(true); m_narrow->hide();
    outer->addWidget(m_columns, 1); outer->addWidget(m_narrow, 1);
    // The footer, outside both columns.
    auto *footer = new QFrame; footer->setObjectName("teamNewFooter");
    auto *fh = new QHBoxLayout(footer); fh->setContentsMargins(20, 10, 20, 10); fh->setSpacing(10);
    m_summary = new QLabel; m_summary->setObjectName("teamNewSummary"); m_summary->setWordWrap(true); m_summary->setTextFormat(Qt::RichText);
    m_preflight = new QLabel; m_preflight->setObjectName("teamNewPreflight"); m_preflight->setTextFormat(Qt::RichText);
    m_reason = new QLabel; m_reason->setObjectName("teamNewReason");
    m_progress = new QProgressBar; m_progress->setRange(0, 0); m_progress->setFixedSize(120, 4); m_progress->setTextVisible(false); m_progress->hide();
    m_discard = button(tr("Discard draft"), "teamNewDiscard");
    m_create = button(tr("Create team"), "teamNewCreate", true);
    fh->addWidget(m_summary, 1); fh->addWidget(m_preflight); fh->addWidget(m_progress); fh->addWidget(m_reason);
    fh->addWidget(m_discard); fh->addWidget(m_create);
    outer->addWidget(footer);
    connect(m_create, &QPushButton::clicked, this, [this] { createTeam(); });
    connect(m_discard, &QPushButton::clicked, this, [this] { discardDraft(); });

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
            if (host.isEmpty()) {
                const QSignalBlocker block(m_leadAccount);
                const QString keep = m_leadAccount->currentData().toString();
                m_leadAccount->clear(); m_leadAccount->addItem(tr("Default account"), QString());
                for (const auto &a : list) if (a.provider == m_leadAgent->currentData().toString())
                    m_leadAccount->addItem(a.label.isEmpty() ? a.id : a.label + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
                m_leadAccount->setCurrentIndex(qMax(0, m_leadAccount->findData(keep)));
            }
            renderMembers();
        });
        m_client->requestAccounts({});
        for (const auto &c : computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
    }
    connect(m_teams, &TeamController::catalogChanged, this, [this] { m_templateList->setCatalog(m_teams->catalog()); renderTemplateCard(); });
    // The owner's edits go out together shortly after the last one; an open
    // draft is read again while the tab shows.
    m_flushTimer.setSingleShot(true); m_flushTimer.setInterval(600);
    connect(&m_flushTimer, &QTimer::timeout, this, [this] { flush(); });
    m_pollTimer.setInterval(s_pollInterval);
    connect(&m_pollTimer, &QTimer::timeout, this, [this] { if (isVisible()) readDraft(); });
    setDark(true);
    renderLead(); renderPlan(); renderUnseen();
    // F6: the list tells the form only now that every part of it exists.
    m_templateList->selected = [this](const QString &id) { pickTemplate(id); };
}

QWidget *TeamNewTab::buildSettings()
{
    const Colors c = colorsFor(true);
    m_scroll = new QScrollArea; m_scroll->setObjectName("teamNewScroll"); m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *content = new QWidget; content->setObjectName("teamNewContent");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(20, 14, 16, 14); v->setSpacing(12);
    auto *head = row(8);
    head->addWidget(label(tr("SETTINGS"), c.dim, 10, true), 1);
    auto *ask = button(tr("Templates and roles: Team designer →"), "teamNewAskDesigner"); ask->setProperty("ghost", true);
    ask->setToolTip(tr("The Team designer keeps the templates and roles; the lead plans this team with you"));
    connect(ask, &QPushButton::clicked, this, [this] { if (askDesigner) askDesigner(); });
    head->addWidget(ask);
    v->addLayout(head);
    // F6: the compact template list; the card only for the selected template.
    v->addWidget(label(tr("Template"), c.muted, 12, false, "teamNewLabel"));
    m_templateList = new TeamTemplateList; m_templateList->setObjectName("teamNewTemplates");
    m_templateList->setDetailVisible(false);
    m_templateList->setCatalog(m_teams->catalog());
    v->addWidget(m_templateList);
    m_templateCard = new QWidget; m_templateCard->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum); m_templateCardLayout = new QVBoxLayout(m_templateCard); m_templateCardLayout->setContentsMargins(0, 0, 0, 0);
    v->addWidget(m_templateCard);
    // Name and folder.
    auto *grid = new QGridLayout; grid->setHorizontalSpacing(8); grid->setVerticalSpacing(8);
    m_name = new QLineEdit; m_name->setObjectName("teamNewName"); m_name->setPlaceholderText(tr("Checkout redesign"));
    grid->addWidget(fieldLabel(tr("Name")), 0, 0); grid->addWidget(m_name, 0, 1, 1, 2); grid->addWidget(unseenMark("name"), 0, 3);
    m_folder = new QLineEdit(QDir::toNativeSeparators(m_coordinator.folder)); m_folder->setObjectName("teamNewFolder");
    auto *browse = button(tr("Browse…"), "teamNewBrowse");
    connect(browse, &QPushButton::clicked, this, [this] {
        const auto dir = QFileDialog::getExistingDirectory(this, tr("Team folder"), m_folder->text());
        if (!dir.isEmpty()) m_folder->setText(QDir::toNativeSeparators(dir));
    });
    grid->addWidget(fieldLabel(tr("Folder")), 1, 0); grid->addWidget(m_folder, 1, 1); grid->addWidget(browse, 1, 2);
    m_worktrees = new QCheckBox(tr("Worktree per member"), this); m_worktrees->setObjectName("teamNewWorktrees"); m_worktrees->setChecked(true);
    m_worktrees->hide();   // B6: the folder policy combo is the control
    // B6: auto is the default for a new team; Shared forces one folder.
    m_folderPolicy = new QComboBox; m_folderPolicy->setObjectName("teamNewFolderPolicy");
    m_folderPolicy->addItem(tr("Auto — the lead picks per member"), QStringLiteral("auto"));
    m_folderPolicy->addItem(tr("Shared folder"), QStringLiteral("shared"));
    m_folderPolicy->addItem(tr("Worktree per member"), QStringLiteral("worktree"));
    m_folderPolicy->setToolTip(tr("Auto lets the lead choose the folder each member works in"));
    m_branch = new QLabel; m_branch->setObjectName("teamNewBranch");
    auto *policyRow = row(8); policyRow->addWidget(m_folderPolicy); policyRow->addWidget(m_branch, 1);
    grid->addWidget(fieldLabel(tr("Folders")), 2, 0); grid->addLayout(policyRow, 2, 1, 1, 2);
    grid->setColumnStretch(1, 1);
    v->addLayout(grid);
    m_gitNote = new QLabel(tr("Folder is not a Git repository — members will share one folder. Worktrees are off."));
    m_gitNote->setObjectName("teamNewGitNote"); m_gitNote->setWordWrap(true);
    v->addWidget(m_gitNote);
    // N10: typing a path keeps auto; a finished non-Git path forces shared.
    // #744: the folder's state comes from PathProbe's worker; its answer runs
    // validate() again.
    connect(&PathProbe::instance(), &PathProbe::answered, this, [this](const QString &path) {
        const QString folder = m_folder ? m_folder->text().trimmed() : QString();
        if (!folder.isEmpty() && (path == folder || path == PathProbe::gitOf(folder))) validate();
    });
    connect(m_folder, &QLineEdit::editingFinished, this, [this] { m_folderCommitted = true; validate(); });
    connect(m_folder, &QLineEdit::textChanged, this, [this] {
        m_folderCommitted = false;
        // N10: resuming the path cancels the rule that forced shared.
        if (m_forcedShared) {
            m_forcedShared = false;
            const QSignalBlocker block(m_folderPolicy);
            m_folderPolicy->setCurrentIndex(m_folderPolicy->findData(QStringLiteral("auto")));
        }
        validate();
    });
    connect(m_folderPolicy, &QComboBox::currentIndexChanged, this, [this] {
        m_worktrees->setChecked(m_folderPolicy->currentData().toString() != QStringLiteral("shared"));
        touch("settings");
    });
    connect(m_worktrees, &QCheckBox::toggled, this, [this](bool on) { for (auto &m : m_members) m.worktree = on; renderMembers(); });
    // Members.
    auto *membersHead = row(8);
    m_membersCount = new QLabel; m_membersCount->setTextFormat(Qt::RichText);
    membersHead->addWidget(m_membersCount); membersHead->addWidget(unseenMark("members")); membersHead->addStretch();
    m_addMember = button(tr("+ Member"), "teamNewAddMember");
    auto *menu = new QMenu(m_addMember);
    connect(menu, &QMenu::aboutToShow, this, [this, menu] {
        menu->clear();
        for (const auto &r : m_teams->catalog().roles) {
            if (r.id == "lead") continue;
            auto *action = menu->addAction(QStringLiteral("%1 — %2%3").arg(r.name, agentNames.value(r.agent, r.agent), r.model.isEmpty() ? QString() : QStringLiteral(" · ") + r.model));
            connect(action, &QAction::triggered, this, [this, id = r.id] { addMember(m_teams->catalog().role(id)); });
        }
        menu->addSeparator();
        connect(menu->addAction(tr("Member without a role")), &QAction::triggered, this, [this] { addMember(nullptr); });
    });
    m_addMember->setMenu(menu);
    membersHead->addWidget(m_addMember);
    v->addLayout(membersHead);
    m_membersBox = new QWidget; m_membersLayout = new QVBoxLayout(m_membersBox); m_membersLayout->setContentsMargins(0, 0, 0, 0); m_membersLayout->setSpacing(6);
    v->addWidget(m_membersBox);
    // Options: planning, approval of the plan and of later phases, the limit.
    auto *options = new QFrame; options->setObjectName("teamNewOptions");
    auto *og = new QGridLayout(options); og->setContentsMargins(0, 12, 0, 0); og->setHorizontalSpacing(10); og->setVerticalSpacing(8);
    m_planning = segmented({{"lead", tr("Lead plans")}, {"template", tr("Use template tasks")}}, "lead", "teamNewPlanning",
                           [this](const QString &value) { m_planningValue = value; touch("tasks"); });
    og->addWidget(fieldLabel(tr("Planning")), 0, 0); og->addWidget(m_planning, 0, 1, Qt::AlignLeft);
    og->addWidget(unseenMark("settings"), 0, 3, Qt::AlignRight);
    auto *approval = row(8);
    m_review = new QCheckBox(tr("Review plan before running")); m_review->setObjectName("teamNewReview"); m_review->setChecked(true);
    m_phaseApproval = new QComboBox; m_phaseApproval->setObjectName("teamNewPhaseApproval");
    m_phaseApproval->addItem(tr("Later phases: as the plan"), QString());
    m_phaseApproval->addItem(tr("Later phases: review"), QStringLiteral("review"));
    m_phaseApproval->addItem(tr("Later phases: auto"), QStringLiteral("auto"));
    approval->addWidget(m_review); approval->addWidget(m_phaseApproval); approval->addStretch();
    og->addWidget(fieldLabel(tr("Approval")), 1, 0); og->addLayout(approval, 1, 1, 1, 3);
    m_max = new QSpinBox; m_max->setObjectName("teamNewMaxMembers"); m_max->setRange(1, 32); m_max->setValue(6); m_max->setFixedWidth(80);
    og->addWidget(fieldLabel(tr("Max members")), 2, 0); og->addWidget(m_max, 2, 1, Qt::AlignLeft);
    og->setColumnStretch(2, 1);
    v->addWidget(options);
    connect(m_review, &QCheckBox::toggled, this, [this] { touch("settings"); });
    connect(m_phaseApproval, &QComboBox::currentIndexChanged, this, [this] { touch("settings"); });
    connect(m_max, &QSpinBox::valueChanged, this, [this] { touch("settings"); });
    // The protocol addendum, folded: how the lead runs the team; only the lead reads it.
    auto *protocolHead = row(6);
    m_protocolToggle = new QToolButton; m_protocolToggle->setObjectName("teamNewProtocolToggle"); m_protocolToggle->setCheckable(true);
    m_protocolToggle->setToolButtonStyle(Qt::ToolButtonTextOnly); m_protocolToggle->setAutoRaise(true); m_protocolToggle->setCursor(Qt::PointingHandCursor);
    m_protocolToggle->setText(tr("▸ Protocol addendum"));
    protocolHead->addWidget(m_protocolToggle); protocolHead->addWidget(unseenMark("addendum")); protocolHead->addStretch();
    v->addLayout(protocolHead);
    m_protocol = new QPlainTextEdit; m_protocol->setObjectName("teamNewProtocol"); m_protocol->setMinimumHeight(72); m_protocol->setMaximumHeight(140);
    m_protocol->setPlaceholderText(tr("How the lead splits the work, checks reports and starts the next wave… A template fills this in."));
    m_protocol->hide();
    v->addWidget(m_protocol);
    connect(m_protocolToggle, &QToolButton::toggled, this, [this](bool open) {
        m_protocol->setVisible(open);
        m_protocolToggle->setText(open ? tr("▾ Protocol addendum") : tr("▸ Protocol addendum"));
    });
    // Yours once edited: another template no longer replaces it.
    connect(m_protocol, &QPlainTextEdit::textChanged, this, [this] { if (!m_applying) m_protocolEdited = true; touch("addendum"); });
    connect(m_name, &QLineEdit::textEdited, this, [this] { touch("name"); });
    v->addStretch(1);
    m_scroll->setWidget(content);
    return m_scroll;
}

QWidget *TeamNewTab::buildLead()
{
    const Colors c = colorsFor(true);
    auto *pane = new QFrame; pane->setObjectName("teamNewLead");
    auto *v = new QVBoxLayout(pane); v->setContentsMargins(16, 14, 20, 14); v->setSpacing(10);
    auto *head = row(8);
    head->addWidget(label(tr("LEAD"), c.dim, 10, true));
    m_leadTitle = elided(QString(), c.text2, 12); m_leadTitle->setObjectName("teamNewLeadTitle");
    head->addWidget(m_leadTitle, 1);
    m_leadTabs = segmented({{"chat", tr("Chat")}, {"goal", tr("Goal")}, {"plan", tr("Plan")}}, "chat", "teamNewLeadTabs",
                           [this](const QString &page) { showLeadPage(page); });
    head->addWidget(m_leadTabs);
    v->addLayout(head);
    m_leadStack = new QStackedWidget; m_leadStack->setObjectName("teamNewLeadPages");
    v->addWidget(m_leadStack, 1);
    // Chat: before Start planning, who leads; then the lead's conversation.
    auto *chatPage = new QWidget; chatPage->setObjectName("teamNewChatPage");
    auto *cv = new QVBoxLayout(chatPage); cv->setContentsMargins(0, 0, 0, 0); cv->setSpacing(8);
    m_leadSetup = new QFrame; m_leadSetup->setObjectName("teamNewLeadSetup");
    auto *sv = new QVBoxLayout(m_leadSetup); sv->setContentsMargins(14, 12, 14, 12); sv->setSpacing(8);
    auto *intro = label(tr("Who leads the team? The chat here is with the lead: it drafts the team with you."), c.text2, 12);
    intro->setWordWrap(true);
    sv->addWidget(intro);
    m_leadThis = new QRadioButton(tr("This session")); m_leadThis->setObjectName("teamNewLeadThis");
    m_leadThis->setEnabled(!m_coordinator.name.isEmpty());
    QStringList details{m_coordinator.name, agentNames.value(m_coordinator.agent, m_coordinator.agent)};
    for (const auto &part : {m_coordinator.model, m_coordinator.account}) if (!part.isEmpty()) details << part;
    m_leadThis->setToolTip(details.join(QStringLiteral(" · ")));
    m_leadNew = new QRadioButton(tr("A new session in the team folder")); m_leadNew->setObjectName("teamNewLeadNew");
    (m_coordinator.name.isEmpty() ? m_leadNew : m_leadThis)->setChecked(true);
    sv->addWidget(m_leadThis);
    if (!m_coordinator.name.isEmpty()) {
        auto *who = label(details.join(QStringLiteral(" · ")), c.dim, 11, false, "teamNewLeadThisDetails");
        who->setWordWrap(true); who->setContentsMargins(22, 0, 0, 0);
        sv->addWidget(who);
    }
    m_busy = new QLabel(tr("This session is working. The draft's brief waits until it is free — it is queued once, never sent twice."));
    m_busy->setObjectName("teamNewBusy"); m_busy->setWordWrap(true); m_busy->setVisible(m_coordinator.busy);
    sv->addWidget(m_busy);
    sv->addWidget(m_leadNew);
    auto *leadRow = new QGridLayout; leadRow->setContentsMargins(22, 0, 0, 0); leadRow->setHorizontalSpacing(6); leadRow->setVerticalSpacing(6);
    m_leadAgent = combo("teamNewLeadAgent"); m_leadAgent->setMinimumWidth(136);
    for (const auto &a : agents) m_leadAgent->addItem(workspaceIcon(a, c.fg), agentNames.value(a), a);
    m_leadModel = combo("teamNewLeadModel"); m_leadModel->setEditable(true); m_leadModel->lineEdit()->setPlaceholderText(tr("default model"));
    m_leadEffort = combo("teamNewLeadEffort"); m_leadEffort->addItem(QStringLiteral("—"), QString());
    for (const auto *e : {"low", "medium", "high", "xhigh"}) m_leadEffort->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
    m_leadAccount = combo("teamNewLeadAccount"); m_leadAccount->setMinimumWidth(110); m_leadAccount->addItem(tr("Default account"), QString());
    if (const auto *role = m_teams->catalog().role("lead")) {
        m_leadAgent->setCurrentIndex(qMax(0, m_leadAgent->findData(role->agent.isEmpty() ? QStringLiteral("claude") : role->agent)));
        m_leadModel->setCurrentText(role->model);
        m_leadEffort->setCurrentIndex(qMax(0, m_leadEffort->findData(role->effort)));
    }
    // Two rows, so the column stays narrow.
    leadRow->addWidget(m_leadAgent, 0, 0); leadRow->addWidget(m_leadModel, 0, 1);
    leadRow->addWidget(m_leadEffort, 1, 0); leadRow->addWidget(m_leadAccount, 1, 1);
    leadRow->setColumnStretch(1, 1);
    sv->addLayout(leadRow);
    m_start = button(tr("Start planning"), "teamNewStartPlanning", true);
    auto *startRow = row(8); startRow->addStretch(); startRow->addWidget(m_start); startRow->addStretch();
    sv->addLayout(startRow);
    auto *note = label(tr("Nothing is created until you press it. Start planning makes a draft and gives the lead its brief; "
                          "worktrees and members come only with Create team."), c.muted, 11, false, "teamNewStartNote");
    note->setWordWrap(true); note->setAlignment(Qt::AlignHCenter);
    sv->addWidget(note);
    m_startError = label(QString(), c.danger, 11, false, "teamNewStartError"); m_startError->setWordWrap(true); m_startError->hide();
    sv->addWidget(m_startError);
    cv->addWidget(m_leadSetup);
    m_leadStarting = label(tr("The lead's session is starting; its chat shows here once it runs."), c.muted, 12, false, "teamNewLeadStarting");
    m_leadStarting->setWordWrap(true);
    cv->addWidget(m_leadStarting);
    m_chat = new TeamConversation(m_client); m_chat->setObjectName("teamNewLeadChat");
    m_chat->setPlaceholder(tr("Message the lead…"));
    cv->addWidget(m_chat, 1);
    cv->addStretch();
    m_unseenLegend = label(tr("● changed by the lead, not yet seen — click a mark when you have seen it"), c.dim, 11, false, "teamNewUnseenLegend");
    m_unseenLegend->setWordWrap(true);
    cv->addWidget(m_unseenLegend);
    m_leadStack->addWidget(chatPage);
    for (auto *radio : {m_leadThis, m_leadNew}) connect(radio, &QRadioButton::toggled, this, [this] { renderLead(); validate(); });
    connect(m_leadAgent, &QComboBox::currentIndexChanged, this, [this] {
        const QSignalBlocker block(m_leadAccount);
        m_leadAccount->clear(); m_leadAccount->addItem(tr("Default account"), QString());
        for (const auto &a : m_accounts.value(QString())) if (a.provider == m_leadAgent->currentData().toString())
            m_leadAccount->addItem(a.label.isEmpty() ? a.id : a.label + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
    });
    connect(m_start, &QPushButton::clicked, this, [this] { startPlanning(); });
    // Goal: the owner and the lead both write it; every member reads it.
    auto *goalPage = new QWidget; goalPage->setObjectName("teamNewGoalPage");
    auto *gv = new QVBoxLayout(goalPage); gv->setContentsMargins(0, 0, 0, 0); gv->setSpacing(6);
    auto *goalHead = row(6);
    goalHead->addWidget(label(tr("Goal"), c.fg, 13, true)); goalHead->addWidget(unseenMark("goal"));
    goalHead->addWidget(label(tr("what the team delivers — every member reads it with each task"), c.dim, 11), 1);
    gv->addLayout(goalHead);
    m_goal = new QPlainTextEdit; m_goal->setObjectName("teamNewGoal");
    m_goal->setPlaceholderText(tr("Describe the result, constraints and what “done” means… The lead may write it with you."));
    gv->addWidget(m_goal, 1);
    m_leadStack->addWidget(goalPage);
    connect(m_goal, &QPlainTextEdit::textChanged, this, [this] {
        // Before the draft the name follows the goal until you type one.
        if (!m_applying && m_draft.isEmpty() && !m_name->isModified()) { const QSignalBlocker block(m_name); m_name->setText(shortName(m_goal->toPlainText())); }
        touch("goal");
    });
    // Plan: the tasks the lead drafts; you approve them after Create team.
    auto *planPage = new QWidget; planPage->setObjectName("teamNewPlanPage");
    auto *pv = new QVBoxLayout(planPage); pv->setContentsMargins(0, 0, 0, 0); pv->setSpacing(6);
    auto *planHead = row(6);
    planHead->addWidget(label(tr("Plan"), c.fg, 13, true)); planHead->addWidget(unseenMark("tasks"));
    planHead->addWidget(label(tr("the lead drafts it; you approve it after Create team"), c.dim, 11), 1);
    pv->addLayout(planHead);
    m_planEmpty = label(tr("No plan yet. Ask the lead in Chat, or use the template's tasks under Planning."), c.muted, 12, false, "teamNewPlanEmpty");
    m_planEmpty->setWordWrap(true);
    pv->addWidget(m_planEmpty);
    auto *planScroll = new QScrollArea; planScroll->setObjectName("teamNewPlanScroll"); planScroll->setWidgetResizable(true); planScroll->setFrameShape(QFrame::NoFrame);
    m_planBox = new QWidget; m_planBox->setObjectName("teamNewPlan");
    m_planLayout = new QVBoxLayout(m_planBox); m_planLayout->setContentsMargins(0, 0, 0, 0); m_planLayout->setSpacing(4);
    planScroll->setWidget(m_planBox);
    pv->addWidget(planScroll, 1);
    m_leadStack->addWidget(planPage);
    return pane;
}

QToolButton *TeamNewTab::unseenMark(const QString &field)
{
    auto *mark = new QToolButton; mark->setObjectName("teamNewUnseen"); mark->setProperty("field", field);
    mark->setText(QStringLiteral("●")); mark->setAutoRaise(true); mark->setCursor(Qt::PointingHandCursor);
    mark->setToolTip(tr("Changed by the lead, not yet seen — click when you have seen it"));
    mark->hide();
    connect(mark, &QToolButton::clicked, this, [this, field] { markSeen(field); });
    m_marks[field].append(mark);
    return mark;
}

void TeamNewTab::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const QString cardSel = alpha(c.accent, dark ? .10 : .08).name(QColor::HexArgb), cardSelBorder = alpha(c.accent, .45).name(QColor::HexArgb);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        // The tab paints its own background, as the team workspace does: the
        // window's palette may be light under a dark theme.
        "QWidget#teamNewDialog{background:%3;}"
        "QWidget#teamNewContent,QWidget#teamNewPlan,QWidget#teamNewChatPage,QWidget#teamNewGoalPage,QWidget#teamNewPlanPage{background:transparent;}"
        "QScrollArea#teamNewScroll,QScrollArea#teamNewPlanScroll{background:transparent;border:none;}"
        "QScrollArea#teamNewScroll > QWidget#qt_scrollarea_viewport,QScrollArea#teamNewPlanScroll > QWidget#qt_scrollarea_viewport{background:transparent;}"
        "QStackedWidget#teamNewLeadPages{background:transparent;}"
        "QFrame#teamNewLead{background:transparent;border:none;border-left:1px solid %4;}"
        "QRadioButton{color:%13;}"
        "QSplitter#teamNewColumns::handle{background:%4;width:1px;}"
        "QFrame#teamNewLeadSetup{background:%1;border:1px solid %2;border-radius:12px;}"
        "QFrame#teamNewTemplate{background:%1;border:1px solid %2;border-radius:10px;}"
        "QFrame#teamNewMemberRow{background:transparent;border:none;border-bottom:1px solid %4;}"
        "QFrame#teamNewMemberRow[error=\"true\"]{background:%6;}"
        "QFrame#teamNewMemberCard{background:%3;border:1px solid %4;border-radius:10px;}"
        "QFrame#teamNewMemberCard[error=\"true\"]{border-color:%7;}"
        "QFrame#teamNewMembersTable{border:1px solid %4;border-radius:10px;}"
        "QFrame#teamNewPlanTask{border-bottom:1px solid %4;}"
        "QLabel#teamNewEmptyMembers{color:%8;border:1px dashed %4;border-radius:10px;padding:16px;}"
        "QFrame#teamNewOptions{border-top:1px solid %4;}"
        "QFrame#teamNewFooter{background:%3;border-top:1px solid %4;}"
        "QLabel#teamNewLabel{color:%8;font-size:12px;}"
        "QLabel#teamNewBusy{color:%9;font-size:12px;}"
        "QLabel#teamNewGitNote{color:%9;background:%10;border:1px solid %11;border-radius:8px;padding:6px 10px;}"
        "QLabel#teamNewBranch{color:%12;font-size:11px;}"
        "QToolButton#teamNewUnseen{color:%5;border:0;font-size:10px;padding:0 2px;}"
        "QToolButton#teamNewProtocolToggle{color:%8;border:0;padding:0;font-size:12px;}"
        "QLineEdit#teamNewFolder{font-family:Consolas,'Cascadia Mono',monospace;}"
        "QPlainTextEdit#teamNewGoal, QPlainTextEdit#teamNewProtocol{font-size:13px;}")
        .arg(cardSel, cardSelBorder, c.surface.name(), c.border.name(), c.accent.name(), alpha(c.danger, .08).name(QColor::HexArgb),
             alpha(c.danger, .7).name(QColor::HexArgb), c.muted.name(), c.warn.name(), alpha(c.warn, .10).name(QColor::HexArgb),
             alpha(c.warn, .45).name(QColor::HexArgb), c.dim.name(), c.fg.name()));
    m_templateList->setDark(dark);
    m_chat->setTheme(dark);
    renderTemplateCard(); renderMembers(); renderPlan();
}

void TeamNewTab::setCoordinatorBusy(bool busy) { m_coordinator.busy = busy; m_busy->setVisible(busy); }

void TeamNewTab::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    arrange();
}

void TeamNewTab::arrange()
{
    const bool wide = width() >= settingsMinimum + leadMinimum + 40;
    if (wide != m_wide) {
        m_wide = wide;
        if (wide) {
            while (m_narrow->count()) m_narrow->removeTab(0);
            m_columns->addWidget(m_settingsPane); m_columns->addWidget(m_leadPane);
            m_settingsPane->setMinimumWidth(settingsMinimum); m_leadPane->setMinimumWidth(leadMinimum);
            m_settingsPane->show(); m_leadPane->show();
            m_narrow->hide(); m_columns->show();
        } else {
            // A tab takes the whole width, however narrow.
            m_settingsPane->setMinimumWidth(0); m_leadPane->setMinimumWidth(0);
            m_narrow->addTab(m_settingsPane, tr("Settings"));
            m_narrow->addTab(m_leadPane, tr("Lead"));
            m_columns->hide(); m_narrow->show();
        }
    }
    const int settings = qMax(settingsMinimum, width() * 3 / 5);
    if (wide) m_columns->setSizes({settings, width() - settings});
    const bool table = (wide ? settings : width()) >= tableWidth;
    if (table != m_tableMembers) { m_tableMembers = table; renderMembers(); }
}

void TeamNewTab::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    if (!m_draft.isEmpty()) { readDraft(); m_pollTimer.start(s_pollInterval); }
}

void TeamNewTab::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_pollTimer.stop();
    // Leaving keeps the draft: what the owner typed goes out now.
    if (!m_dirty.isEmpty()) flush();
}

bool TeamNewTab::eventFilter(QObject *watched, QEvent *event)
{
    if (watched == m_membersPopup && event->type() == QEvent::Hide) {
        watched->removeEventFilter(this);
        m_membersPopup = nullptr;
        // The list hides while its combo still handles the pick: the held
        // back rebuild runs after that.
        QTimer::singleShot(0, this, [this] { renderMembers(); });
    }
    return QWidget::eventFilter(watched, event);
}

void TeamNewTab::showLeadPage(const QString &page)
{
    m_leadPage = page;
    m_leadStack->setCurrentIndex(page == "goal" ? 1 : page == "plan" ? 2 : 0);
    for (auto *b : m_leadTabs->findChildren<QPushButton *>()) b->setChecked(b->property("segment") == page);
}

void TeamNewTab::selectTemplate(const QString &id)
{
    m_templateList->setCurrent(id);
    pickTemplate(id);
}

void TeamNewTab::pickTemplate(const QString &id)
{
    m_template = id.isEmpty() ? QStringLiteral("-") : id;
    m_members.clear();
    const auto &catalog = m_teams->catalog();
    const auto *t = catalog.templ(m_template);
    if (t) {
        for (const auto &seat : t->roster) for (int i = 0; i < seat.count; ++i) addMember(catalog.role(seat.role));
        m_planningValue = t->planning.isEmpty() ? QStringLiteral("lead") : t->planning;
        for (auto *b : m_planning->findChildren<QPushButton *>()) b->setChecked(b->property("segment") == m_planningValue);
        const QSignalBlocker blockReview(m_review), blockPhase(m_phaseApproval), blockMax(m_max);
        m_review->setChecked(t->approval != "auto");
        m_phaseApproval->setCurrentIndex(qMax(0, m_phaseApproval->findData(t->phaseApproval)));
        if (t->maxMembers > 0) m_max->setValue(t->maxMembers);
    }
    if (!m_protocolEdited) {
        const QSignalBlocker block(m_protocol);
        m_protocol->setPlainText(t ? t->leadProtocol : QString());
        touch("addendum");
    }
    touch("members"); touch("settings"); touch("tasks");
    renderTemplateCard(); renderMembers();
}

void TeamNewTab::addMember(const Teams::Role *role)
{
    Member m;
    if (role) {
        m.role = role->name; m.agent = role->agent.isEmpty() ? QStringLiteral("claude") : role->agent; m.account = role->account;
        m.model = role->model; m.effort = role->effort; m.permissions = role->permissions; m.computer = role->computer; m.prompt = role->prompt;
        m.worktree = m_worktrees->isChecked() && role->folder != "read_only" && role->folder != "shared";
    } else { m.agent = "claude"; m.worktree = m_worktrees->isChecked(); }
    m_members.append(m);
    touch("members");
    renderMembers();
}

void TeamNewTab::renderTemplateCard()
{
    if (!m_templateCardLayout) return;
    while (auto *item = m_templateCardLayout->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    const auto *t = catalog.templ(m_template);
    if (!t) return;
    auto *card = new QFrame; card->setObjectName("teamNewTemplate"); card->setProperty("template", t->id);
    auto *v = new QVBoxLayout(card); v->setContentsMargins(12, 9, 12, 9); v->setSpacing(4);
    v->addWidget(label(t->name, c.fg, 12, true));
    if (!t->description.isEmpty()) { auto *use = label(t->description, c.dim, 11); use->setWordWrap(true); v->addWidget(use); }
    auto *dots = row(4);
    int members = 0;
    for (const auto &seat : t->roster) {
        const auto *role = catalog.role(seat.role);
        const QString roleName = role ? role->name : seat.role;
        const QColor color = role && !role->color.isEmpty() ? QColor(role->color) : roleColor(roleName, m_dark);
        dots->addWidget(new Pill(seat.count > 1 ? QStringLiteral("%1 ×%2").arg(roleName).arg(seat.count) : roleName, color));
        members += seat.count;
    }
    dots->addStretch();
    v->addLayout(dots);
    v->addWidget(label(tr("%1 members · %2 tasks").arg(members).arg(t->tasks.size()), c.dim, 11));
    m_templateCardLayout->addWidget(card);
}

QStringList TeamNewTab::rowErrors(const Member &m) const
{
    QStringList errors;
    if (m.role.trimmed().isEmpty()) errors << tr("Pick a role");
    for (const auto &computer : m_computers) if (computer.alias == m.computer && !computer.online) errors << tr("Computer %1 is offline").arg(computer.label);
    for (const auto &a : m_accounts.value(m.computer)) if (a.id == m.account && !a.signedIn) errors << tr("Account “%1” is not signed in").arg(a.label.isEmpty() ? a.id : a.label);
    return errors;
}

void TeamNewTab::renderMembers()
{
    if (!m_membersLayout) return;
    // Rebuilding deletes the combo whose list is open and so closes the list
    // under the pointer. A draft read, an account reply, a folder policy or a
    // role edit can ask for the rows meanwhile: they are rebuilt once it closes.
    if (QWidget *popup = QApplication::activePopupWidget(); popup && popup->parentWidget() && m_membersBox->isAncestorOf(popup->parentWidget())) {
        if (m_membersPopup != popup) {
            if (m_membersPopup) m_membersPopup->removeEventFilter(this);
            m_membersPopup = popup;
            popup->installEventFilter(this);
        }
        validate();
        return;
    }
    const Colors c = colorsFor(m_dark);
    while (auto *item = m_membersLayout->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    const bool git = PathProbe::instance().folder(m_folder->text().trimmed()).git;   // #744
    if (m_members.isEmpty()) {
        auto *empty = new QLabel(m_draft.isEmpty() ? tr("No members yet. Pick a template, add members by role, or let the lead draft them.")
                                                   : tr("No members yet. The lead drafts them with you, or add them by role."));
        empty->setObjectName("teamNewEmptyMembers"); empty->setAlignment(Qt::AlignCenter); empty->setWordWrap(true);
        m_membersLayout->addWidget(empty);
        validate();
        return;
    }
    QList<std::pair<QString, QString>> roleItems;
    for (const auto &r : m_teams->catalog().roles) if (r.id != "lead") roleItems.append({r.name, r.name});
    // The widgets of one member, bound to its row in m_members.
    const auto widgets = [this, c, git, roleItems](int i) {
        struct W { QComboBox *role, *agent, *model, *effort, *permissions, *computer, *account; QCheckBox *worktree; QToolButton *duplicate, *remove; QLabel *tile; };
        const Member &m = m_members[i];
        const QStringList errors = rowErrors(m);
        W w{};
        w.role = combo("teamNewRole"); w.role->setEditable(true);
        if (m.role.isEmpty()) w.role->lineEdit()->setPlaceholderText(tr("— role —"));
        for (const auto &[value, text] : roleItems) w.role->addItem(text, value);
        w.role->setCurrentText(m.role);
        w.agent = combo("teamNewAgent");
        for (const auto &a : agents) w.agent->addItem(workspaceIcon(a, c.fg), agentNames.value(a), a);
        w.agent->setCurrentIndex(qMax(0, w.agent->findData(m.agent)));
        w.model = combo("teamNewModel"); w.model->setEditable(true); w.model->setCurrentText(m.model); w.model->lineEdit()->setPlaceholderText(tr("default"));
        w.effort = combo("teamNewEffort"); w.effort->addItem(QStringLiteral("—"), QString());
        for (const auto *e : {"low", "medium", "high", "xhigh"}) w.effort->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
        w.effort->setCurrentIndex(qMax(0, w.effort->findData(m.effort)));
        w.permissions = combo("teamNewPermissions");
        w.permissions->addItem(tr("Account"), QString()); w.permissions->addItem(tr("Auto"), QStringLiteral("auto")); w.permissions->addItem(tr("Bypass"), QStringLiteral("bypass"));
        w.permissions->setCurrentIndex(qMax(0, w.permissions->findData(m.permissions)));
        w.computer = combo("teamNewComputer");
        for (const auto &computer : m_computers) w.computer->addItem(computer.label, computer.alias);
        w.computer->setCurrentIndex(qMax(0, w.computer->findData(m.computer)));
        w.account = combo("teamNewAccount"); w.account->addItem(tr("Default account"), QString());
        for (const auto &a : m_accounts.value(m.computer)) if (a.provider == m.agent)
            w.account->addItem(a.label.isEmpty() ? a.id : a.label + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
        w.account->setCurrentIndex(qMax(0, w.account->findData(m.account)));
        w.worktree = new QCheckBox; w.worktree->setObjectName("teamNewWorktree"); w.worktree->setChecked(git && m.worktree); w.worktree->setEnabled(git);
        w.worktree->setToolTip(tr("Own worktree"));
        w.duplicate = new QToolButton; w.duplicate->setIcon(workspaceIcon("copy", c.muted)); w.duplicate->setToolTip(tr("Duplicate")); w.duplicate->setAutoRaise(true);
        w.remove = new QToolButton; w.remove->setIcon(workspaceIcon("trash", c.muted)); w.remove->setToolTip(tr("Remove")); w.remove->setAutoRaise(true); w.remove->setObjectName("teamNewRemove");
        for (auto *b : {w.duplicate, w.remove}) { b->setIconSize(QSize(14, 14)); b->setCursor(Qt::PointingHandCursor); }
        w.tile = new QLabel; w.tile->setPixmap(agentLogo(m.agent, c.fg).pixmap(16, 16));
        const QString danger = QStringLiteral("border-color:%1;").arg(c.danger.name());
        for (const auto &error : errors) {
            if (error.startsWith(tr("Pick a role").left(4))) w.role->setStyleSheet(danger);
            if (error.contains(tr("offline"))) w.computer->setStyleSheet(danger);
            if (error.contains(tr("signed in"))) w.account->setStyleSheet(danger);
        }
        const auto update = [this, i](const std::function<void(Member &)> &apply, bool rerender) {
            if (i >= m_members.size()) return;
            apply(m_members[i]);
            touch("members");
            if (rerender) QTimer::singleShot(0, this, [this] { renderMembers(); });   // a click inside it may be running
        };
        connect(w.role, &QComboBox::currentTextChanged, this, [this, update](const QString &text) {
            update([this, text](Member &m) {
                m.role = text.trimmed();
                if (const auto *r = m_teams->catalog().roleNamed(text)) m.prompt = r->prompt;
            }, false);
        });
        connect(w.role, &QComboBox::activated, this, [update] { update([](Member &) {}, true); });
        connect(w.agent, &QComboBox::activated, this, [update, a = w.agent] { update([a](Member &m) { m.agent = a->currentData().toString(); m.account.clear(); }, true); });
        connect(w.model, &QComboBox::currentTextChanged, this, [update](const QString &text) { update([text](Member &m) { m.model = text.trimmed(); }, false); });
        connect(w.effort, &QComboBox::activated, this, [update, e = w.effort] { update([e](Member &m) { m.effort = e->currentData().toString(); }, false); });
        connect(w.permissions, &QComboBox::activated, this, [update, p = w.permissions] { update([p](Member &m) { m.permissions = p->currentData().toString(); }, false); });
        connect(w.computer, &QComboBox::activated, this, [update, k = w.computer] { update([k](Member &m) { m.computer = k->currentData().toString(); m.account.clear(); }, true); });
        connect(w.account, &QComboBox::activated, this, [update, a = w.account] { update([a](Member &m) { m.account = a->currentData().toString(); }, true); });
        connect(w.worktree, &QCheckBox::toggled, this, [update](bool on) { update([on](Member &m) { m.worktree = on; }, false); });
        connect(w.duplicate, &QToolButton::clicked, this, [this, i] {
            if (i >= m_members.size()) return;
            Member copy = m_members[i]; copy.id.clear();
            m_members.insert(i + 1, copy); touch("members");
            QTimer::singleShot(0, this, [this] { renderMembers(); });
        });
        connect(w.remove, &QToolButton::clicked, this, [this, i] {
            if (i >= m_members.size()) return;
            m_members.removeAt(i); touch("members");
            QTimer::singleShot(0, this, [this] { renderMembers(); });
        });
        return std::make_pair(w, errors);
    };
    if (m_tableMembers) {
        auto *table = new QFrame; table->setObjectName("teamNewMembersTable");
        auto *tv = new QVBoxLayout(table); tv->setContentsMargins(0, 0, 0, 0); tv->setSpacing(0);
        const QList<int> widths{120, 150, 0, 76, 96, 96, 24, 54};
        const auto grid = [&widths](QWidget *host) {
            auto *g = new QGridLayout(host); g->setContentsMargins(8, 5, 8, 5); g->setHorizontalSpacing(6);
            for (int col = 0; col < widths.size(); ++col) { if (widths[col]) g->setColumnMinimumWidth(col, widths[col]); g->setColumnStretch(col, widths[col] ? 0 : 1); }
            return g;
        };
        auto *head = new QWidget; auto *hg = grid(head);
        const QStringList titles{tr("Role"), tr("Agent"), tr("Model"), tr("Effort"), tr("Permissions"), tr("Computer"), tr("WT"), QString()};
        for (int col = 0; col < titles.size(); ++col) hg->addWidget(label(titles[col], c.dim, 11), 0, col);
        tv->addWidget(head);
        for (int i = 0; i < m_members.size(); ++i) {
            auto [w, errors] = widgets(i);
            auto *line = new QFrame; line->setObjectName("teamNewMemberRow"); line->setProperty("error", !errors.isEmpty());
            auto *g = grid(line);
            auto *agentBox = new QWidget; auto *ah = new QHBoxLayout(agentBox); ah->setContentsMargins(0, 0, 0, 0); ah->setSpacing(4);
            ah->addWidget(w.agent, 1); w.tile->hide();   // the combo shows the agent's icon
            auto *actions = new QWidget; auto *ach = new QHBoxLayout(actions); ach->setContentsMargins(0, 0, 0, 0); ach->setSpacing(0);
            ach->addWidget(w.duplicate); ach->addWidget(w.remove);
            g->addWidget(w.role, 0, 0); g->addWidget(agentBox, 0, 1); g->addWidget(w.model, 0, 2); g->addWidget(w.effort, 0, 3);
            g->addWidget(w.permissions, 0, 4); g->addWidget(w.computer, 0, 5); g->addWidget(w.worktree, 0, 6, Qt::AlignCenter); g->addWidget(actions, 0, 7);
            w.account->hide();   // the table has no account column; the role's account applies
            if (!errors.isEmpty()) {
                auto *error = label(errors.join(QStringLiteral(" · ")), c.danger, 11); error->setObjectName("teamNewRowError");
                g->addWidget(error, 1, 0, 1, widths.size());
            }
            tv->addWidget(line);
        }
        m_membersLayout->addWidget(table);
    } else {
        for (int i = 0; i < m_members.size(); ++i) {
            auto [w, errors] = widgets(i);
            const Member &m = m_members[i];
            auto *card = new QFrame; card->setObjectName("teamNewMemberCard"); card->setProperty("error", !errors.isEmpty());
            auto *cv = new QVBoxLayout(card); cv->setContentsMargins(10, 8, 10, 8); cv->setSpacing(6);
            auto *head = row(6);
            head->addWidget(w.tile);
            head->addWidget(m.role.isEmpty() ? static_cast<QWidget *>(label(tr("No role"), c.danger, 12)) : new Pill(m.role, roleColor(m.role, m_dark)));
            head->addWidget(elided(agentNames.value(m.agent, m.agent) + (m.model.isEmpty() ? QString() : QStringLiteral(" · ") + m.model), c.dim, 12), 1);
            head->addWidget(w.duplicate); head->addWidget(w.remove);
            cv->addLayout(head);
            auto *g = new QGridLayout; g->setHorizontalSpacing(8); g->setVerticalSpacing(6);
            g->setColumnMinimumWidth(0, 84); g->setColumnMinimumWidth(2, 84); g->setColumnStretch(1, 1); g->setColumnStretch(3, 1);
            const QList<std::pair<QString, QWidget *>> cells{{tr("Role"), w.role}, {tr("Agent"), w.agent}, {tr("Model"), w.model}, {tr("Effort"), w.effort},
                                                            {tr("Permissions"), w.permissions}, {tr("Computer"), w.computer}, {tr("Account"), w.account}, {tr("Worktree"), w.worktree}};
            for (int k = 0; k < cells.size(); ++k) {
                g->addWidget(label(cells[k].first, c.muted, 11), k / 2, (k % 2) * 2);
                g->addWidget(cells[k].second, k / 2, (k % 2) * 2 + 1);
            }
            w.worktree->setText(tr("Own worktree"));
            cv->addLayout(g);
            if (!errors.isEmpty()) {
                auto *error = label(errors.join(QStringLiteral(" · ")), c.danger, 11); error->setObjectName("teamNewRowError"); error->setWordWrap(true);
                cv->addWidget(error);
            }
            m_membersLayout->addWidget(card);
        }
    }
    validate();
}

void TeamNewTab::renderPlan()
{
    if (!m_planLayout) return;
    const Colors c = colorsFor(m_dark);
    while (auto *item = m_planLayout->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    m_planEmpty->setVisible(m_tasks.isEmpty());
    for (const auto &task : m_tasks) {
        auto *line = new QFrame; line->setObjectName("teamNewPlanTask"); line->setProperty("task", task.id);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(4, 5, 4, 5); h->setSpacing(8);
        h->addWidget(label(QStringLiteral("#%1").arg(task.id), c.dim, 11, true));
        auto *title = label(task.title, c.fg, 12); title->setWordWrap(true);
        h->addWidget(title, 1);
        QStringList deps; for (const auto &dep : task.deps) deps << QStringLiteral("#%1").arg(dep.task);
        QStringList meta;
        if (!task.assignee.isEmpty()) meta << task.assignee;
        if (!deps.isEmpty()) meta << tr("after %1").arg(deps.join(QStringLiteral(", ")));
        if (task.phase > 1) meta << tr("phase %1").arg(task.phase);
        h->addWidget(label(meta.join(QStringLiteral(" · ")), c.muted, 11));
        m_planLayout->addWidget(line);
    }
    m_planLayout->addStretch();
}

void TeamNewTab::renderLead()
{
    if (!m_leadSetup) return;
    const bool drafting = !m_draft.isEmpty();
    m_leadSetup->setVisible(!drafting);
    m_leadStarting->setVisible(drafting && m_leadSessionName.isEmpty());
    if (drafting && m_leadSessionName != m_chat->session()) m_chat->setSession({}, m_leadSessionName);
    // U53: the draft's lead is its team's member; swap-member changes its account.
    m_chat->setAccountLocked(tr("The lead of this draft: its account changes with Swap member once the team exists, or choose it above."));
    if (!drafting && !m_chat->session().isEmpty()) m_chat->setSession({}, {});
    m_chat->setVisible(drafting && !m_leadSessionName.isEmpty());
    m_leadTitle->setText(!drafting ? QString() : m_leadSessionName.isEmpty() ? tr("new session") : m_leadSessionName);
    for (auto *w : {static_cast<QWidget *>(m_leadAgent), static_cast<QWidget *>(m_leadModel), static_cast<QWidget *>(m_leadEffort), static_cast<QWidget *>(m_leadAccount)})
        w->setEnabled(m_leadNew->isChecked() && !m_starting);
    m_start->setText(m_starting ? tr("Starting…") : tr("Start planning"));
    // The folder is the draft's from Start planning on.
    m_folder->setReadOnly(drafting);
    if (auto *browse = findChild<QPushButton *>("teamNewBrowse")) browse->setEnabled(!drafting);
}

void TeamNewTab::renderUnseen()
{
    for (auto it = m_marks.cbegin(); it != m_marks.cend(); ++it)
        for (auto *mark : it.value()) mark->setVisible(m_unseen.contains(it.key()));
    for (auto *b : m_leadTabs->findChildren<QPushButton *>()) {
        const QString page = b->property("segment").toString();
        const bool marked = (page == "goal" && m_unseen.contains("goal")) || (page == "plan" && m_unseen.contains("tasks"));
        const QString text = page == "goal" ? tr("Goal") : page == "plan" ? tr("Plan") : tr("Chat");
        b->setText(marked ? text + QStringLiteral(" ●") : text);
    }
    m_unseenLegend->setVisible(!m_unseen.isEmpty());
}

void TeamNewTab::validate()
{
    // B6: a combo signal may reach validate before the last widgets exist.
    if (!m_folderPolicy || !m_name || !m_create || !m_start || !m_goal) return;
    const Colors c = colorsFor(m_dark);
    const QString folder = m_folder->text().trimmed();
    // #744: never a stat on this thread. Until the worker answers, what depends
    // on Git stays as it is and Start planning waits.
    const auto probed = PathProbe::instance().folder(folder);
    const bool git = probed.git;
    if (!probed.checking && !probed.unresponsive) {
        m_worktrees->setEnabled(git);
        if (!git && m_worktrees->isChecked()) { const QSignalBlocker block(m_worktrees); m_worktrees->setChecked(false); }
        // N10: a Git path with auto or worktree policy brings the worktree rows back.
        else if (git && !m_worktrees->isChecked() && m_folderPolicy->currentData().toString() != QStringLiteral("shared"))
            m_worktrees->setChecked(true);
        // B6/N10: typing a path keeps auto; a finished non-Git path forces shared.
        // QA #347: the draft's folder is fixed, so from Start planning on the policy
        // is what the owner and the lead chose - and a non-Git folder has one folder
        // for every member: it shows Shared, disabled, with the reason beside the
        // warning, instead of an Auto this folder can never honour.
        const bool inDraft = !m_draft.isEmpty();
        const bool sharedOnly = !folder.isEmpty() && !git;
        if (sharedOnly && (inDraft || m_folderCommitted)) {
            if (m_folderPolicy->currentData().toString() != "shared") {
                // No signals: currentIndexChanged would call validate() again inside this one.
                const QSignalBlocker block(m_folderPolicy);
                m_folderPolicy->setCurrentIndex(m_folderPolicy->findData(QStringLiteral("shared")));
                m_worktrees->setChecked(false);
                m_forcedShared = true;
                if (!inDraft) touch("settings");
            }
            m_folderPolicy->setEnabled(false);
            m_folderPolicy->setToolTip(tr("Folder is not a Git repository: every member works in this folder, so the policy is shared."));
        } else {
            m_folderPolicy->setEnabled(true);
            m_folderPolicy->setToolTip(tr("Auto lets the lead choose the folder each member works in"));
        }
        m_gitNote->setVisible(!folder.isEmpty() && !git);
    }
    const QString name = m_name->text().trimmed();
    QString slug = name.toLower(); slug.replace(QRegularExpression(QStringLiteral("[^a-z0-9]+")), QStringLiteral("-")); slug.remove(QRegularExpression(QStringLiteral("^-+|-+$")));
    m_branch->setText(git ? tr("· branch team/%1/<role>").arg(slug.isEmpty() ? QStringLiteral("…") : slug) : QString());
    int bad = 0; for (const auto &m : m_members) bad += !rowErrors(m).isEmpty();
    m_membersCount->setText(QStringLiteral("<b>%1</b> &nbsp;<span style='color:%2;'>%3 / %4</span>").arg(tr("Members"), c.dim.name()).arg(m_members.size() + 1).arg(m_max->value()));
    m_membersCount->setStyleSheet(QStringLiteral("color:%1;font-size:13px;").arg(c.fg.name()));
    m_membersCount->setToolTip(tr("The lead counts as one member"));
    m_addMember->setEnabled(m_members.size() + 1 < m_max->value() && !m_creating);
    const bool drafting = !m_draft.isEmpty();
    const bool folderOk = !folder.isEmpty() && probed.dir;
    // Start planning: a folder and a lead.
    QString startReason;
    if (!folder.isEmpty() && probed.checking) startReason = tr("Checking the folder…");
    else if (!folder.isEmpty() && probed.unresponsive) startReason = tr("The folder does not answer: its disk may be busy");
    else if (!folderOk) startReason = tr("Choose an existing folder");
    else if (m_leadThis->isChecked() && m_coordinator.name.isEmpty()) startReason = tr("Choose who leads");
    m_start->setEnabled(!drafting && !m_starting && startReason.isEmpty());
    m_start->setToolTip(startReason);
    // Create team: the draft, saved, with no error here or in the preflight.
    QString reason;
    if (!drafting) reason = tr("Start planning first");
    else if (m_goal->toPlainText().trimmed().isEmpty()) reason = tr("Write the goal, or let the lead write it");
    else if (name.isEmpty()) reason = tr("Name the team");
    else if (!folderOk) reason = tr("Choose an existing folder");
    else if (bad) reason = bad == 1 ? tr("Fix 1 member row") : tr("Fix %1 member rows").arg(bad);
    else if (m_members.size() + 1 > m_max->value()) reason = tr("More members than the limit");
    else if (!m_preflightErrors.isEmpty()) reason = m_preflightErrors.size() == 1 ? tr("Fix 1 preflight error") : tr("Fix %1 preflight errors").arg(m_preflightErrors.size());
    // Owner, 2026-10-29: saving never blocks Create team (it saves first); a
    // blocker shows red in its own field.
    const QString redBorder = QStringLiteral("border:1px solid %1;").arg(c.danger.name());
    // Only on a change: setting a style sheet repolishes the widget (#11).
    const auto mark = [](QWidget *field, const QString &style) { if (field->styleSheet() != style) field->setStyleSheet(style); };
    mark(m_goal, drafting && m_goal->toPlainText().trimmed().isEmpty() ? redBorder : QString());
    mark(m_name, drafting && name.isEmpty() ? redBorder : QString());
    mark(m_folder, drafting && !folderOk ? redBorder : QString());
    m_reason->setText(reason);
    m_reason->setStyleSheet(QStringLiteral("color:%1;").arg((drafting ? c.danger : c.muted).name()));
    m_reason->setVisible(!reason.isEmpty() && !m_creating);
    if (!m_creating) {
        QString summary;
        if (!m_saveError.isEmpty()) summary = QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.danger.name(), tr("Not saved: %1").arg(m_saveError.toHtmlEscaped()));
        else if (!drafting) summary = QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(), tr("Nothing is created until you press <b>Start planning</b>."));
        else {
            const QString saved = m_savedAt.isValid() ? tr("Draft saved %1").arg(QLocale().toString(m_savedAt.time(), QLocale::ShortFormat)) : tr("Draft");
            summary = QStringLiteral("<span style='color:%1;'>%2 · rev %3</span>").arg(c.muted.name(), saved).arg(m_revision);
        }
        m_summary->setText(summary);
    }
    // The preflight of the draft: ok, or its errors and warnings.
    m_preflight->setVisible(drafting);
    QStringList parts;
    if (m_preflightErrors.isEmpty()) parts << QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(), tr("Preflight: ok"));
    else parts << QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.danger.name(),
        m_preflightErrors.size() == 1 ? tr("Preflight: 1 error") : tr("Preflight: %1 errors").arg(m_preflightErrors.size()));
    if (!m_preflightWarnings.isEmpty()) parts << QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.warn.name(),
        m_preflightWarnings.size() == 1 ? tr("1 warning") : tr("%1 warnings").arg(m_preflightWarnings.size()));
    m_preflight->setText(parts.join(QStringLiteral(" · ")));
    m_preflight->setToolTip((m_preflightErrors + m_preflightWarnings).join('\n'));
    m_discard->setVisible(drafting);
    m_discard->setEnabled(!m_creating);
    m_create->setEnabled(reason.isEmpty() && !m_creating);
    m_create->setToolTip(reason);
}

void TeamNewTab::touch(const QString &field)
{
    if (m_applying) return;
    if (!m_draft.isEmpty()) { m_dirty.insert(field); m_flushTimer.start(); }
    validate();
}

QJsonObject TeamNewTab::memberJson(const Member &m, const QString &id) const
{
    return {{"id", id}, {"role", m.role.trimmed()}, {"agent", m.agent}, {"account", m.account}, {"model", m.model.trimmed()},
            {"effort", m.effort}, {"permissions", m.permissions}, {"computer", m.computer},
            {"folder", m.worktree && m_worktrees->isEnabled() ? "worktree" : "shared"}, {"prompt", m.prompt}};
}

QJsonArray TeamNewTab::membersJson() const
{
    // Ids the draft gave stay; new rows get the next free one for their role.
    Teams::Team ids; ids.lead = "lead";
    Teams::Member leadMember; leadMember.id = "lead"; ids.members.append(leadMember);
    for (const auto &m : m_members) if (!m.id.isEmpty()) { Teams::Member placed; placed.id = m.id; ids.members.append(placed); }
    QJsonArray members;
    for (const auto &m : m_members) {
        QString id = m.id;
        if (id.isEmpty()) { id = nextMemberId(ids, m.role); Teams::Member placed; placed.id = id; ids.members.append(placed); }
        members.append(memberJson(m, id));
    }
    return members;
}

QJsonArray TeamNewTab::templateTasks() const
{
    QJsonArray tasks;
    const auto *t = m_teams->catalog().templ(m_template);
    if (!t || m_planningValue != "template") return tasks;
    QMap<QString, QString> byRole;   // catalog role id -> first member id
    for (const auto &value : membersJson()) {
        const auto member = value.toObject();
        if (const auto *role = m_teams->catalog().roleNamed(member["role"].toString()); role && !byRole.contains(role->id)) byRole.insert(role->id, member["id"].toString());
    }
    for (int i = 0; i < t->tasks.size(); ++i) {
        const auto &k = t->tasks[i];
        QJsonArray deps; for (int d : k.deps) deps.append(QJsonObject{{"task", d + 1}, {"kind", "hard"}});
        tasks.append(QJsonObject{{"id", i + 1}, {"title", k.title}, {"assignee", k.role == "lead" ? QStringLiteral("lead") : byRole.value(k.role)}, {"deps", deps}});
    }
    return tasks;
}

QJsonObject TeamNewTab::settingsJson() const
{
    // The draft's settings as read, with the fields this form owns.
    QJsonObject settings = m_settings;
    settings.insert("folder_policy", m_folderPolicy->currentData().toString());
    settings.insert("approval", m_review->isChecked() ? "review" : "auto");
    const QString phase = m_phaseApproval->currentData().toString();
    if (phase.isEmpty()) settings.remove("phase_approval"); else settings.insert("phase_approval", phase);
    settings.insert("max_members", m_max->value());
    return settings;
}

QJsonValue TeamNewTab::fieldValue(const QString &field) const
{
    if (field == "name") return m_name->text().trimmed();
    if (field == "goal") return m_goal->toPlainText().trimmed();
    if (field == "addendum") return m_protocol->toPlainText().trimmed();
    if (field == "settings") return settingsJson();
    if (field == "members") return membersJson();
    if (field == "tasks") return templateTasks();
    return {};
}

void TeamNewTab::startPlanning()
{
    validate();
    if (!m_start->isEnabled()) return;
    QJsonObject lead{{"role", "Lead"}};
    QString session;
    if (m_leadThis->isChecked()) {
        session = m_coordinator.name;
        lead.insert("agent", m_coordinator.agent.isEmpty() ? m_coordinator.name.section('/', 0, 0) : m_coordinator.agent);
        lead.insert("session", QJsonObject{{"host", ""}, {"name", session}});
    } else {
        lead.insert("agent", m_leadAgent->currentData().toString());
        for (const auto &[key, value] : {std::pair{"model", m_leadModel->currentText().trimmed()}, std::pair{"effort", m_leadEffort->currentData().toString()},
                                         std::pair{"account", m_leadAccount->currentData().toString()}})
            if (!value.isEmpty()) lead.insert(QLatin1String(key), value);
    }
    if (const auto *role = m_teams->catalog().role("lead")) lead.insert("prompt", role->prompt);
    QJsonObject input{{"action", "start"}, {"folder", m_folder->text().trimmed()}, {"lead", lead}};
    if (!m_name->text().trimmed().isEmpty()) input.insert("name", m_name->text().trimmed());
    if (m_teams->catalog().templ(m_template)) input.insert("template", m_template);
    m_starting = true; m_startError->hide();
    renderLead(); validate();
    m_teams->call(QStringLiteral("draft"), input, [this, guard = QPointer<TeamNewTab>(this), session](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard) return;
        m_starting = false;
        if (!ok) { m_startError->setText(error); m_startError->show(); renderLead(); validate(); return; }
        m_draft = result.value("team").toString();
        m_revision = result.value("revision").toInteger(1);
        m_leadSessionName = session;
        m_savedAt = QDateTime::currentDateTime();
        // What the form holds so far is the draft's first content; the folder,
        // name and lead went with start.
        m_dirty.insert("settings");
        if (!m_goal->toPlainText().trimmed().isEmpty()) m_dirty.insert("goal");
        if (!m_protocol->toPlainText().trimmed().isEmpty()) m_dirty.insert("addendum");
        if (!m_members.isEmpty()) m_dirty.insert("members");
        if (!templateTasks().isEmpty()) m_dirty.insert("tasks");
        renderLead(); renderMembers();
        flush();
        m_pollTimer.start(s_pollInterval);
    });
}

void TeamNewTab::flush()
{
    if (m_draft.isEmpty() || m_dirty.isEmpty() || m_sending) return;
    m_flushTimer.stop();
    QJsonObject set;
    for (const auto &field : draftFields) if (m_dirty.contains(field)) set.insert(field, fieldValue(field));
    m_sending = true;
    validate();
    const QJsonObject input{{"action", "update"}, {"team", m_draft}, {"revision", m_revision}, {"set", set}};
    m_teams->call(QStringLiteral("draft"), input, [this, guard = QPointer<TeamNewTab>(this), set, draft = m_draft](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard || draft != m_draft) return;
        m_sending = false;
        if (!ok) {
            // Someone wrote first: the owner's edit still wins, against the
            // revision the refusal names. Once; then the next read decides.
            static const QRegularExpression stale(QStringLiteral("Draft revision is (\\d+) now"));
            const auto match = stale.match(error);
            if (match.hasMatch() && !m_retried) { m_retried = true; m_revision = match.captured(1).toLongLong(); flush(); return; }
            m_retried = false; m_saveError = error;
            if (m_createAfterSave) { m_createAfterSave = false; m_create->setText(tr("Create team")); }
            validate();
            // The edits stay to be sent: try again later rather than leave
            // Create team waiting on a save nothing retries.
            QTimer::singleShot(5000, this, [this, draft] { if (draft == m_draft) flush(); });
            return;
        }
        m_retried = false; m_saveError.clear();
        m_revision = result.value("draft_revision").toInteger(m_revision + 1);
        // A field changed again while this went out stays to be sent.
        for (auto it = set.constBegin(); it != set.constEnd(); ++it) if (fieldValue(it.key()) == it.value()) m_dirty.remove(it.key());
        if (result.value("unseen").isArray()) { m_unseen.clear(); for (const auto &f : result.value("unseen").toArray()) m_unseen << f.toString(); }
        m_savedAt = QDateTime::currentDateTime();
        renderUnseen(); validate();
        if (!m_dirty.isEmpty()) m_flushTimer.start();
        else if (m_createAfterSave) createTeam();
    });
}

void TeamNewTab::openDraft(const QString &team)
{
    if (team.isEmpty() || team == m_draft) return;
    m_draft = team; m_revision = 0; m_dirty.clear(); m_saveError.clear();
    renderLead(); validate();
    readDraft();
    if (isVisible()) m_pollTimer.start(s_pollInterval);
}

void TeamNewTab::readDraft()
{
    if (m_draft.isEmpty()) return;
    m_teams->call(QStringLiteral("status"), {{"team", m_draft}}, [this, guard = QPointer<TeamNewTab>(this), draft = m_draft](bool ok, const QJsonObject &result, const QString &) {
        if (!guard || draft != m_draft || !ok || m_sending) return;
        // The settings as stored; the form changes only its own fields.
        if (!m_dirty.contains("settings")) m_settings = result.value("settings").toObject();
        applyDraft(Teams::parse(result));
    }, false);
}

void TeamNewTab::applyDraft(const Teams::Team &team)
{
    if (team.id != m_draft) return;
    if (!team.draft()) {
        // Created or discarded elsewhere: the tab starts over.
        const QString id = m_draft;
        resetDraft();
        if (team.status == "stopped") { if (discarded) discarded(id); }
        else if (created) created(id);
        return;
    }
    // Revisions only grow: a read that left before the last update is old.
    if (team.draftRevision < m_revision) return;
    if (const auto *lead = team.member(team.lead)) m_leadSessionName = lead->session;
    if (team.draftRevision != m_revision) {
        // The lead's edits, except the fields the owner is changing now.
        m_applying = true;
        if (!m_dirty.contains("name") && m_name->text() != team.name) m_name->setText(team.name);
        if (!m_dirty.contains("goal") && m_goal->toPlainText() != team.goal) m_goal->setPlainText(team.goal);
        if (!m_dirty.contains("addendum") && m_protocol->toPlainText() != team.leadProtocol) {
            m_protocol->setPlainText(team.leadProtocol);
            if (!team.leadProtocol.isEmpty()) m_protocolEdited = true;
        }
        if (!m_dirty.contains("settings")) {
            const QSignalBlocker blockPolicy(m_folderPolicy), blockReview(m_review), blockPhase(m_phaseApproval), blockMax(m_max);
            if (!team.folderPolicy.isEmpty()) m_folderPolicy->setCurrentIndex(qMax(0, m_folderPolicy->findData(team.folderPolicy)));
            m_worktrees->setChecked(m_folderPolicy->currentData().toString() != QStringLiteral("shared"));
            m_review->setChecked(team.approval != "auto");
            m_phaseApproval->setCurrentIndex(qMax(0, m_phaseApproval->findData(team.phaseApproval)));
            if (team.maxMembers > 0) m_max->setValue(team.maxMembers);
        }
        if (!m_dirty.contains("members")) {
            m_members.clear();
            for (const auto &member : team.members) {
                if (member.id == team.lead) continue;
                Member m{member.id, member.role, member.agent, member.account, member.model, member.effort, member.permissions, member.computer, member.prompt,
                         member.folder != "shared" && member.folder != "read_only"};
                m_members.append(m);
            }
            renderMembers();
        }
        if (!m_dirty.contains("tasks")) { m_tasks = team.tasks; renderPlan(); }
        m_revision = team.draftRevision;
        m_applying = false;
    }
    m_unseen = team.draftUnseen;
    m_preflightErrors = team.errors; m_preflightWarnings = team.warnings;
    if (!m_savedAt.isValid()) m_savedAt = QDateTime::fromSecsSinceEpoch(qint64(team.updated));
    renderLead(); renderUnseen(); validate();
}

void TeamNewTab::markSeen(const QString &field)
{
    if (m_draft.isEmpty()) return;
    m_teams->call(QStringLiteral("draft"), {{"action", "seen"}, {"team", m_draft}, {"fields", QJsonArray{field}}},
        [this, guard = QPointer<TeamNewTab>(this), field](bool ok, const QJsonObject &result, const QString &error) {
            if (!guard) return;
            if (!ok) { m_saveError = error; validate(); return; }
            if (result.value("unseen").isArray()) { m_unseen.clear(); for (const auto &f : result.value("unseen").toArray()) m_unseen << f.toString(); }
            else m_unseen.removeAll(field);
            renderUnseen();
        });
}

void TeamNewTab::createTeam()
{
    validate();
    if (!m_create->isEnabled()) return;
    // Unsaved edits go out first; the create follows the save that clears them.
    if (m_sending || !m_dirty.isEmpty()) {
        m_createAfterSave = true;
        m_create->setText(tr("Saving…"));
        flush();
        return;
    }
    m_createAfterSave = false;
    m_creating = true;
    m_create->setText(tr("Creating…")); m_progress->show();
    m_summary->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(colorsFor(m_dark).muted.name(),
        tr("Running the preflight · creating the team…")));
    validate();
    m_teams->call(QStringLiteral("draft"), {{"action", "create"}, {"team", m_draft}},
        [this, guard = QPointer<TeamNewTab>(this)](bool ok, const QJsonObject &result, const QString &error) {
            if (!guard) return;
            m_creating = false; m_progress->hide(); m_create->setText(tr("Create team"));
            if (!ok) { m_saveError = error; validate(); return; }
            const QString id = result.value("team").toString(m_draft);
            resetDraft();
            if (created) created(id);
        });
}

void TeamNewTab::discardDraft()
{
    if (m_draft.isEmpty()) return;
    bool stopLead = false;
    bool sure = false;
    if (confirmDiscard) sure = confirmDiscard(&stopLead);
    else {
        QMessageBox box(QMessageBox::Warning, tr("Discard draft"),
                        tr("Discard the draft “%1”? Nothing of it is created.").arg(m_name->text().trimmed().isEmpty() ? tr("New team") : m_name->text().trimmed()),
                        QMessageBox::NoButton, this);
        auto *stop = new QCheckBox(tr("Also stop the lead's session"));
        box.setCheckBox(stop);
        auto *discard = box.addButton(tr("Discard draft"), QMessageBox::DestructiveRole);
        box.addButton(QMessageBox::Cancel);
        box.exec();
        sure = box.clickedButton() == discard;
        stopLead = stop->isChecked();
    }
    if (!sure) return;
    m_teams->call(QStringLiteral("draft"), {{"action", "discard"}, {"team", m_draft}, {"stop_lead", stopLead}},
        [this, guard = QPointer<TeamNewTab>(this), draft = m_draft](bool ok, const QJsonObject &, const QString &error) {
            if (!guard || draft != m_draft) return;
            if (!ok) { m_saveError = error; validate(); return; }
            resetDraft();
            if (discarded) discarded(draft);
        });
}

void TeamNewTab::resetDraft()
{
    m_pollTimer.stop(); m_flushTimer.stop();
    m_draft.clear(); m_leadSessionName.clear(); m_revision = 0;
    m_dirty.clear(); m_unseen.clear(); m_preflightErrors.clear(); m_preflightWarnings.clear();
    m_tasks.clear(); m_settings = {}; m_saveError.clear(); m_savedAt = {};
    renderLead(); renderPlan(); renderUnseen(); renderMembers();
}
