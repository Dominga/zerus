#include "TeamNewTab.h"
#include "HgsClient.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDir>
#include <QFileDialog>
#include <QGridLayout>
#include <QJsonArray>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QPointer>
#include <QProgressBar>
#include <QRegularExpression>
#include <QScrollArea>
#include <QSpinBox>
#include <QTimer>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
constexpr int wideWidth = 760, maxContent = 980;

QComboBox *combo(const char *name)
{
    auto *c = new QComboBox; c->setObjectName(name); c->setMinimumWidth(60);
    c->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon); c->setMinimumContentsLength(4);
    return c;
}

QLabel *section(const QString &title, const QString &sub, const Colors &c)
{
    auto *l = new QLabel(sub.isEmpty() ? QStringLiteral("<b>%1</b>").arg(title.toHtmlEscaped())
                                       : QStringLiteral("<b>%1</b> &nbsp;<span style='color:%2;font-weight:400;'>%3</span>").arg(title.toHtmlEscaped(), c.dim.name(), sub.toHtmlEscaped()));
    l->setTextFormat(Qt::RichText);
    l->setStyleSheet(QStringLiteral("color:%1;font-size:13px;").arg(c.fg.name()));
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

// A clickable card frame.
class Card : public QFrame {
public:
    std::function<void()> clicked;
protected:
    void mouseReleaseEvent(QMouseEvent *event) override { if (event->button() == Qt::LeftButton && clicked) clicked(); QFrame::mouseReleaseEvent(event); }
};
}

TeamNewTab::TeamNewTab(TeamController *teams, HgsClient *client, const QList<Computer> &computers, const Coordinator &coordinator,
                       const QString &project, QWidget *parent)
    : QWidget(parent), m_teams(teams), m_client(client), m_computers(computers), m_coordinator(coordinator), m_project(project)
{
    setObjectName("teamNewDialog");   // the form, as New team's dialog was
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    m_scroll = new QScrollArea; m_scroll->setObjectName("teamNewScroll"); m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    auto *page = new QWidget; page->setObjectName("teamNewPage");
    auto *centre = new QHBoxLayout(page); centre->setContentsMargins(0, 0, 0, 0);
    auto *content = new QWidget; content->setObjectName("teamNewContent"); content->setMaximumWidth(maxContent);
    auto *v = new QVBoxLayout(content); v->setContentsMargins(20, 14, 20, 14); v->setSpacing(14);
    centre->addStretch(); centre->addWidget(content, 100); centre->addStretch();
    // 1. Coordinator.
    m_coordinatorCard = new QFrame; m_coordinatorCard->setObjectName("teamNewCoordinator");
    v->addWidget(m_coordinatorCard);
    m_busy = new QLabel(tr("This session is working. The planning brief waits until it is free — it is queued once, never sent twice."));
    m_busy->setObjectName("teamNewBusy"); m_busy->setWordWrap(true); m_busy->setVisible(coordinator.busy);
    v->addWidget(m_busy);
    // 2. Template.
    auto *templateHead = row(8);
    templateHead->addWidget(section(tr("Template"), {}, colorsFor(true)), 1);
    auto *ask = button(tr("Not sure? Ask Team designer →"), "teamNewAskDesigner"); ask->setProperty("ghost", true);
    connect(ask, &QPushButton::clicked, this, [this] { if (askDesigner) askDesigner(); });
    templateHead->addWidget(ask);
    v->addLayout(templateHead);
    m_templatesBox = new QWidget; m_templates = new QGridLayout(m_templatesBox); m_templates->setContentsMargins(0, 0, 0, 0); m_templates->setSpacing(8);
    v->addWidget(m_templatesBox);
    // 3. Goal and name.
    v->addWidget(section(tr("Goal"), tr("what the team should deliver — the coordinator plans from this"), colorsFor(true)));
    m_goal = new QPlainTextEdit; m_goal->setObjectName("teamNewGoal"); m_goal->setMinimumHeight(96); m_goal->setMaximumHeight(140);
    m_goal->setPlaceholderText(tr("Describe the result, constraints and what “done” means…"));
    v->addWidget(m_goal);
    v->addWidget(section(tr("Team protocol"), tr("how the coordinator runs the team — only the coordinator reads this"), colorsFor(true)));
    m_protocol = new QPlainTextEdit; m_protocol->setObjectName("teamNewProtocol"); m_protocol->setMinimumHeight(72); m_protocol->setMaximumHeight(140);
    m_protocol->setPlaceholderText(tr("How to split the work, check reports and start the next wave… A template fills this in."));
    v->addWidget(m_protocol);
    // The Team designer helps with both texts in its own conversation; its
    // answer fills the fields only when you say so.
    v->addWidget(section(tr("Ask the designer"), tr("the Team designer drafts the goal and the protocol with you"), colorsFor(true)));
    m_helperIdle = new QWidget; m_helperIdle->setObjectName("teamNewHelperIdle");
    {
        auto *h = new QHBoxLayout(m_helperIdle); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
        h->addWidget(label(tr("The Team designer is not running."), colorsFor(true).muted, 12), 1);
        auto *start = button(tr("Start designer"), "teamNewHelperStart");
        connect(start, &QPushButton::clicked, this, [this] { if (startDesigner) startDesigner(); m_helperNote->setText(tr("Starting the Team designer…")); });
        h->addWidget(start);
    }
    v->addWidget(m_helperIdle);
    m_helper = new TeamConversation(client); m_helper->setObjectName("teamNewHelper"); m_helper->setMinimumHeight(280);
    m_helper->setPlaceholder(tr("Tell the designer what the team should do…"));
    v->addWidget(m_helper);
    {
        auto *h = row(8);
        m_helperNote = label(QString(), colorsFor(true).muted, 11); m_helperNote->setWordWrap(true);
        m_helperDraft = button(tr("Draft goal and protocol"), "teamNewHelperDraft");
        m_helperDraft->setToolTip(tr("Send the template, members and both fields to the designer and ask for a GOAL and a PROTOCOL"));
        connect(m_helperDraft, &QPushButton::clicked, this, [this] { m_helper->send(helperRequest()); });
        m_helperUse = button(tr("Use its answer"), "teamNewHelperUse", true);
        m_helperUse->setToolTip(tr("Fill Goal and Team protocol from the designer's last reply"));
        connect(m_helperUse, &QPushButton::clicked, this, [this] {
            QString goal, protocol;
            if (!parseHelperReply(m_helper->details().value("last_message").toString(), &goal, &protocol)) {
                m_helperNote->setText(tr("The last reply has no GOAL: or PROTOCOL: section; ask with Draft goal and protocol."));
                return;
            }
            if (!goal.isEmpty()) m_goal->setPlainText(goal);
            if (!protocol.isEmpty()) m_protocol->setPlainText(protocol);
            m_helperNote->setText(tr("Filled from the designer's reply; edit them as you like."));
        });
        h->addWidget(m_helperNote, 1); h->addWidget(m_helperDraft); h->addWidget(m_helperUse);
        v->addLayout(h);
    }
    auto *helperTimer = new QTimer(this); helperTimer->setInterval(2000);
    connect(helperTimer, &QTimer::timeout, this, [this] { if (isVisible()) updateHelper(); });
    helperTimer->start();
    auto *nameRow = row(10);
    auto *nameLabel = new QLabel(tr("Team name")); nameLabel->setFixedWidth(96); nameLabel->setObjectName("teamNewLabel");
    m_name = new QLineEdit; m_name->setObjectName("teamNewName");
    nameRow->addWidget(nameLabel); nameRow->addWidget(m_name, 1);
    v->addLayout(nameRow);
    // 4. Folder.
    v->addWidget(section(tr("Folder"), {}, colorsFor(true)));
    auto *folderRow = row(6);
    m_folder = new QLineEdit(QDir::toNativeSeparators(coordinator.folder)); m_folder->setObjectName("teamNewFolder");
    auto *browse = button(tr("Browse…"), "teamNewBrowse");
    connect(browse, &QPushButton::clicked, this, [this] {
        const auto dir = QFileDialog::getExistingDirectory(this, tr("Team folder"), m_folder->text());
        if (!dir.isEmpty()) m_folder->setText(QDir::toNativeSeparators(dir));
    });
    folderRow->addWidget(m_folder, 1); folderRow->addWidget(browse);
    v->addLayout(folderRow);
    auto *wtRow = row(8);
    m_worktrees = new QCheckBox(tr("Worktree per member")); m_worktrees->setObjectName("teamNewWorktrees"); m_worktrees->setChecked(true);
    m_branch = new QLabel; m_branch->setObjectName("teamNewBranch");
    wtRow->addWidget(m_worktrees); wtRow->addWidget(m_branch, 1);
    v->addLayout(wtRow);
    m_gitNote = new QLabel(tr("Folder is not a Git repository — members will share one folder. Worktrees are off."));
    m_gitNote->setObjectName("teamNewGitNote"); static_cast<QLabel *>(m_gitNote)->setWordWrap(true);
    v->addWidget(m_gitNote);
    // 5. Members.
    auto *membersHead = row(8);
    m_membersCount = new QLabel; m_membersCount->setTextFormat(Qt::RichText);
    membersHead->addWidget(m_membersCount, 1);
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
    // 6. Options.
    auto *options = new QFrame; options->setObjectName("teamNewOptions");
    auto *og = new QGridLayout(options); og->setContentsMargins(0, 12, 0, 0); og->setHorizontalSpacing(10); og->setVerticalSpacing(8);
    const auto optionLabel = [](const QString &text) { auto *l = new QLabel(text); l->setObjectName("teamNewLabel"); l->setFixedWidth(96); return l; };
    m_planning = segmented({{"lead", tr("Lead plans")}, {"template", tr("Use template tasks")}}, "lead", "teamNewPlanning",
                           [this](const QString &value) { m_planningValue = value; validate(); });
    og->addWidget(optionLabel(tr("Planning")), 0, 0); og->addWidget(m_planning, 0, 1, Qt::AlignLeft);
    m_review = new QCheckBox(tr("Review plan before running")); m_review->setObjectName("teamNewReview"); m_review->setChecked(true);
    og->addWidget(optionLabel(tr("Approval")), 1, 0); og->addWidget(m_review, 1, 1);
    m_max = new QSpinBox; m_max->setObjectName("teamNewMaxMembers"); m_max->setRange(1, 32); m_max->setValue(6); m_max->setFixedWidth(80);
    og->addWidget(optionLabel(tr("Max members")), 2, 0); og->addWidget(m_max, 2, 1, Qt::AlignLeft);
    og->setColumnStretch(2, 1);
    v->addWidget(options);
    v->addStretch();
    m_scroll->setWidget(page);
    outer->addWidget(m_scroll, 1);
    // 7. Footer, outside the scroll area.
    auto *footer = new QFrame; footer->setObjectName("teamNewFooter");
    auto *fh = new QHBoxLayout(footer); fh->setContentsMargins(20, 10, 20, 10); fh->setSpacing(10);
    m_summary = new QLabel; m_summary->setObjectName("teamNewSummary"); m_summary->setWordWrap(true); m_summary->setTextFormat(Qt::RichText);
    m_reason = new QLabel; m_reason->setObjectName("teamNewReason");
    m_progress = new QProgressBar; m_progress->setRange(0, 0); m_progress->setFixedSize(120, 4); m_progress->setTextVisible(false); m_progress->hide();
    m_create = button(tr("Create team"), "teamNewCreate", true);
    fh->addWidget(m_summary, 1); fh->addWidget(m_progress); fh->addWidget(m_reason); fh->addWidget(m_create);
    outer->addWidget(footer);

    connect(m_create, &QPushButton::clicked, this, [this] { create(); });
    // Yours once edited: another template no longer replaces it.
    connect(m_protocol, &QPlainTextEdit::textChanged, this, [this] { m_protocolEdited = true; });
    connect(m_goal, &QPlainTextEdit::textChanged, this, [this] {
        if (!m_name->isModified()) { const QSignalBlocker block(m_name); m_name->setText(shortName(m_goal->toPlainText())); }
        validate();
    });
    connect(m_name, &QLineEdit::textEdited, this, [this] { validate(); });
    connect(m_folder, &QLineEdit::textChanged, this, [this] { validate(); });
    connect(m_worktrees, &QCheckBox::toggled, this, [this](bool on) { for (auto &m : m_members) m.worktree = on; renderMembers(); });
    connect(m_max, &QSpinBox::valueChanged, this, [this] { validate(); });
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
            renderMembers();
        });
        m_client->requestAccounts({});
        for (const auto &c : computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
    }
    connect(m_teams, &TeamController::catalogChanged, this, [this] { renderTemplates(); });
    setDark(true);
    // Nothing picked yet: the cards and an empty members box.
    m_template = QStringLiteral("-");
    renderTemplates(); renderMembers();
}

void TeamNewTab::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const QString cardSel = alpha(c.accent, dark ? .10 : .08).name(QColor::HexArgb), cardSelBorder = alpha(c.accent, .45).name(QColor::HexArgb);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamNewDialog,QWidget#teamNewPage{background:transparent;}"
        "QFrame#teamNewCoordinator{background:%1;border:1px solid %2;border-radius:12px;}"
        "QFrame#teamNewTemplate{background:%3;border:1px solid %4;border-radius:10px;}"
        "QFrame#teamNewTemplate[picked=\"true\"]{background:%1;border:2px solid %5;}"
        "QFrame#teamNewMemberRow{background:transparent;border:none;border-bottom:1px solid %4;}"
        "QFrame#teamNewMemberRow[error=\"true\"]{background:%6;}"
        "QFrame#teamNewMemberCard{background:%3;border:1px solid %4;border-radius:10px;}"
        "QFrame#teamNewMemberCard[error=\"true\"]{border-color:%7;}"
        "QFrame#teamNewMembersTable{border:1px solid %4;border-radius:10px;}"
        "QLabel#teamNewEmptyMembers{color:%8;border:1px dashed %4;border-radius:10px;padding:16px;}"
        "QFrame#teamNewOptions{border-top:1px solid %4;}"
        "QFrame#teamNewFooter{background:%3;border-top:1px solid %4;}"
        "QLabel#teamNewLabel{color:%8;font-size:12px;}"
        "QLabel#teamNewBusy{color:%9;font-size:12px;}"
        "QLabel#teamNewGitNote{color:%9;background:%10;border:1px solid %11;border-radius:8px;padding:6px 10px;}"
        "QLabel#teamNewBranch{color:%12;font-size:11px;}"
        "QLineEdit#teamNewFolder{font-family:Consolas,'Cascadia Mono',monospace;}"
        "QPlainTextEdit#teamNewGoal, QPlainTextEdit#teamNewProtocol{font-size:13px;}")
        .arg(cardSel, cardSelBorder, c.surface.name(), c.border.name(), c.accent.name(), alpha(c.danger, .08).name(QColor::HexArgb),
             alpha(c.danger, .7).name(QColor::HexArgb), c.muted.name(), c.warn.name(), alpha(c.warn, .10).name(QColor::HexArgb),
             alpha(c.warn, .45).name(QColor::HexArgb), c.dim.name()));
    renderCoordinator(); renderTemplates(); renderMembers();
}

void TeamNewTab::setCoordinatorBusy(bool busy) { m_coordinator.busy = busy; m_busy->setVisible(busy); }

void TeamNewTab::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    const bool wide = width() >= wideWidth;
    const int columns = qMax(1, qMin(width(), maxContent) / 190);
    if (wide != m_wide) { m_wide = wide; renderMembers(); }
    if (columns != m_templateColumns) renderTemplates();
}

void TeamNewTab::renderCoordinator()
{
    const Colors c = colorsFor(m_dark);
    delete m_coordinatorCard->layout();
    for (auto *child : m_coordinatorCard->findChildren<QWidget *>(QString(), Qt::FindDirectChildrenOnly)) retire(child);
    auto *h = new QHBoxLayout(m_coordinatorCard); h->setContentsMargins(12, 10, 12, 10); h->setSpacing(10);
    auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(m_coordinator.agent), c.fg).pixmap(22, 22)); tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
    h->addWidget(tile, 0, Qt::AlignTop);
    auto *col = new QVBoxLayout; col->setSpacing(2);
    auto *top = row(8);
    top->addWidget(label(tr("COORDINATOR"), c.dim, 10, true));
    top->addWidget(label(m_coordinator.name, c.fg, 13, true));
    if (!m_coordinator.status.isEmpty()) {
        auto *status = label(m_coordinator.status, c.text2, 11);
        status->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:9px;padding:0 7px;").arg(c.text2.name(), c.border.name()));
        top->addWidget(status);
    }
    top->addStretch();
    col->addLayout(top);
    QStringList details{agentNames.value(m_coordinator.agent, m_coordinator.agent)};
    for (const auto &part : {m_coordinator.model, m_coordinator.account, QDir::toNativeSeparators(m_coordinator.folder), m_coordinator.computer})
        if (!part.isEmpty()) details << part;
    col->addWidget(elided(details.join(QStringLiteral(" · ")), c.dim, 12));
    h->addLayout(col, 1);
    auto *lock = label(tr("This session"), c.text2, 11);
    lock->setToolTip(tr("The coordinator is this session. It is not started again — it receives the planning brief."));
    lock->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:9px;padding:1px 8px;").arg(c.text2.name(), c.border.name()));
    h->addWidget(lock, 0, Qt::AlignVCenter);
}

void TeamNewTab::renderTemplates()
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    while (auto *item = m_templates->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    m_templateColumns = qMax(1, qMin(width() > 0 ? width() : maxContent, maxContent) / 190);
    QList<std::tuple<QString, QString, QString, QList<Seat>, int>> cards;
    for (const auto &t : catalog.templates) cards.append({t.id, t.name, t.description, t.roster, int(t.tasks.size())});
    cards.append({QString(), tr("No template"), tr("Start empty — the coordinator plans everything"), {}, 0});
    int at = 0;
    for (const auto &[id, name, description, roster, tasks] : cards) {
        auto *card = new Card; card->setObjectName("teamNewTemplate"); card->setProperty("picked", id == m_template); card->setProperty("template", id);
        card->setCursor(Qt::PointingHandCursor); card->setMinimumHeight(92);
        card->clicked = [this, id = id] { pickTemplate(id); };
        auto *v = new QVBoxLayout(card); v->setContentsMargins(10, 8, 10, 8); v->setSpacing(3);
        v->addWidget(label(name, c.fg, 12, true));
        auto *use = label(description, c.dim, 11); use->setWordWrap(true); v->addWidget(use);
        v->addStretch();
        auto *dots = row(3);
        int members = 0;
        for (const auto &seat : roster) {
            const auto *role = catalog.role(seat.role);
            const QString roleName = role ? role->name : seat.role;
            const QColor color = role && !role->color.isEmpty() ? QColor(role->color) : roleColor(roleName, m_dark);
            auto *dot = label(QStringLiteral("●"), color, 11); dot->setToolTip(roleName);
            dots->addWidget(dot);
            if (seat.count > 1) dots->addWidget(label(QStringLiteral("×%1").arg(seat.count), c.dim, 10));
            members += seat.count;
        }
        dots->addStretch();
        v->addLayout(dots);
        v->addWidget(label(id.isEmpty() ? tr("no members yet") : tr("%1 members · %2 tasks").arg(members).arg(tasks), c.dim, 11));
        m_templates->addWidget(card, at / m_templateColumns, at % m_templateColumns);
        ++at;
    }
    for (int i = 0; i < m_templateColumns; ++i) m_templates->setColumnStretch(i, 1);
}

void TeamNewTab::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    updateHelper();
}

void TeamNewTab::updateHelper()
{
    if (!m_helper) return;
    const QString session = designerSession ? designerSession() : QString();
    if (session != m_helper->session()) m_helper->setSession({}, session);
    m_helperIdle->setVisible(session.isEmpty());
    m_helper->setVisible(!session.isEmpty());
    m_helperDraft->setEnabled(!session.isEmpty()); m_helperUse->setEnabled(!session.isEmpty());
    if (!session.isEmpty() && m_helperNote->text() == tr("Starting the Team designer…")) m_helperNote->clear();
}

QString TeamNewTab::helperRequest() const
{
    QStringList roles; for (const auto &m : m_members) roles << m.role + QStringLiteral(" (") + m.agent + QLatin1Char(')');
    const auto *t = m_teams->catalog().templ(m_template);
    const auto field = [](const QString &text) { return text.trimmed().isEmpty() ? QStringLiteral("(empty)") : text.trimmed(); };
    return QStringLiteral(
        "[New team] Help me write this team's goal and team protocol; change no file and propose nothing to the catalog.\n"
        "Template: %1\nCoordinator: %2 (%3)\nMembers: %4\nFolder: %5\n\nCurrent goal:\n%6\n\nCurrent team protocol:\n%7\n\n"
        "Ask me what you need first if anything is unclear. When you answer, reply with exactly these two sections and nothing before them:\n"
        "GOAL:\n<the result, constraints and what done means; every member reads it with each task>\n"
        "PROTOCOL:\n<how the coordinator runs the team: waves, checks of reports, when to start the next wave; only the coordinator reads it>")
        .arg(t ? t->name : QStringLiteral("none"), m_coordinator.name, m_coordinator.agent,
             roles.isEmpty() ? QStringLiteral("none yet") : roles.join(QStringLiteral(", ")), m_folder->text().trimmed(),
             field(m_goal->toPlainText()), field(m_protocol->toPlainText()));
}

bool TeamNewTab::parseHelperReply(const QString &reply, QString *goal, QString *protocol)
{
    // "GOAL:" and "PROTOCOL:" on their own lines, bold or as headings too.
    static const QRegularExpression heading(QStringLiteral("(?im)^[#*_ \\t]*(GOAL|PROTOCOL)[*_ \\t]*:[*_ \\t]*"));
    QList<std::pair<QString, int>> marks; QList<int> starts;
    for (auto it = heading.globalMatch(reply); it.hasNext();) {
        const auto m = it.next();
        marks.append({m.captured(1).toUpper(), int(m.capturedEnd())}); starts.append(int(m.capturedStart()));
    }
    goal->clear(); protocol->clear();
    for (int i = 0; i < marks.size(); ++i) {
        const int end = i + 1 < marks.size() ? starts[i + 1] : int(reply.size());
        const QString text = reply.mid(marks[i].second, end - marks[i].second).trimmed();
        (marks[i].first == QLatin1String("GOAL") ? *goal : *protocol) = text;
    }
    return !goal->isEmpty() || !protocol->isEmpty();
}

void TeamNewTab::pickTemplate(const QString &id)
{
    m_template = id;
    m_members.clear();
    const auto &catalog = m_teams->catalog();
    if (const auto *t = catalog.templ(id)) {
        for (const auto &seat : t->roster) for (int i = 0; i < seat.count; ++i) addMember(catalog.role(seat.role));
        m_planningValue = t->planning.isEmpty() ? QStringLiteral("lead") : t->planning;
        for (auto *b : m_planning->findChildren<QPushButton *>()) b->setChecked(b->property("segment") == m_planningValue);
        m_review->setChecked(t->approval != "auto");
        if (t->maxMembers > 0) m_max->setValue(t->maxMembers);
    }
    if (!m_protocolEdited) {
        const QSignalBlocker block(m_protocol);
        const auto *t = catalog.templ(id);
        m_protocol->setPlainText(t ? t->leadProtocol : QString());
    }
    renderTemplates(); renderMembers();
}

void TeamNewTab::addMember(const Role *role)
{
    Member m;
    if (role) {
        m.role = role->name; m.agent = role->agent.isEmpty() ? QStringLiteral("claude") : role->agent; m.account = role->account;
        m.model = role->model; m.effort = role->effort; m.permissions = role->permissions; m.computer = role->computer; m.prompt = role->prompt;
        m.worktree = m_worktrees->isChecked() && role->folder != "read_only" && role->folder != "shared";
    } else { m.agent = "claude"; m.worktree = m_worktrees->isChecked(); }
    m_members.append(m);
    renderMembers();
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
    const Colors c = colorsFor(m_dark);
    while (auto *item = m_membersLayout->takeAt(0)) { if (auto *w = item->widget()) retire(w); delete item; }
    const bool git = QDir(m_folder->text().trimmed()).exists(QStringLiteral(".git"));
    if (m_members.isEmpty()) {
        auto *empty = new QLabel(tr("No members yet. Pick a template above, or add members by role.")); empty->setObjectName("teamNewEmptyMembers");
        empty->setAlignment(Qt::AlignCenter);
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
        w.tile = new QLabel; w.tile->setPixmap(workspaceIcon(agentIcon(m.agent), c.fg).pixmap(16, 16));
        const QString danger = QStringLiteral("border-color:%1;").arg(c.danger.name());
        for (const auto &error : errors) {
            if (error.startsWith(tr("Pick a role").left(4))) w.role->setStyleSheet(danger);
            if (error.contains(tr("offline"))) w.computer->setStyleSheet(danger);
            if (error.contains(tr("signed in"))) w.account->setStyleSheet(danger);
        }
        const auto update = [this, i](const std::function<void(Member &)> &apply, bool rerender) {
            if (i >= m_members.size()) return;
            apply(m_members[i]);
            if (rerender) QTimer::singleShot(0, this, [this] { renderMembers(); });   // a click inside it may be running
            else validate();
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
        connect(w.duplicate, &QToolButton::clicked, this, [this, i] { if (i < m_members.size()) { m_members.insert(i + 1, m_members[i]); QTimer::singleShot(0, this, [this] { renderMembers(); }); } });
        connect(w.remove, &QToolButton::clicked, this, [this, i] { if (i < m_members.size()) { m_members.removeAt(i); QTimer::singleShot(0, this, [this] { renderMembers(); }); } });
        return std::make_pair(w, errors);
    };
    if (m_wide) {
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

void TeamNewTab::validate()
{
    const Colors c = colorsFor(m_dark);
    const QString folder = m_folder->text().trimmed();
    const bool git = !folder.isEmpty() && QDir(folder).exists(QStringLiteral(".git"));
    m_worktrees->setEnabled(git);
    if (!git && m_worktrees->isChecked()) { const QSignalBlocker block(m_worktrees); m_worktrees->setChecked(false); }
    m_gitNote->setVisible(!folder.isEmpty() && !git);
    const QString name = m_name->text().trimmed();
    QString slug = name.toLower(); slug.replace(QRegularExpression(QStringLiteral("[^a-z0-9]+")), QStringLiteral("-")); slug.remove(QRegularExpression(QStringLiteral("^-+|-+$")));
    m_branch->setText(git ? tr("· branch team/%1/<role>").arg(slug.isEmpty() ? QStringLiteral("…") : slug) : QString());
    int bad = 0; for (const auto &m : m_members) bad += !rowErrors(m).isEmpty();
    m_membersCount->setText(QStringLiteral("<b>%1</b> &nbsp;<span style='color:%2;'>%3 / %4</span>").arg(tr("Members"), c.dim.name()).arg(m_members.size()).arg(m_max->value()));
    m_membersCount->setStyleSheet(QStringLiteral("color:%1;font-size:13px;").arg(c.fg.name()));
    m_addMember->setEnabled(m_members.size() + 1 < m_max->value() && !m_creating);
    QString reason;
    if (m_goal->toPlainText().trimmed().isEmpty()) reason = tr("Write a goal");
    else if (name.isEmpty()) reason = tr("Name the team");
    else if (folder.isEmpty() || !QDir(folder).exists()) reason = tr("Choose an existing folder");
    else if (bad) reason = bad == 1 ? tr("Fix 1 member row") : tr("Fix %1 member rows").arg(bad);
    else if (m_members.size() + 1 > m_max->value()) reason = tr("More members than the limit");
    m_reason->setText(reason);
    m_reason->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name()));
    m_reason->setVisible(!reason.isEmpty() && !m_creating);
    if (!m_creating)
        m_summary->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.muted.name(),
            tr("Create sends <b>this session</b> the planning brief; you approve its plan before members start.")));
    m_create->setEnabled(reason.isEmpty() && !m_creating);
    m_create->setToolTip(reason);
}

QJsonObject TeamNewTab::memberJson(const Member &m, const QString &id) const
{
    return {{"id", id}, {"role", m.role.trimmed()}, {"agent", m.agent}, {"account", m.account}, {"model", m.model.trimmed()},
            {"effort", m.effort}, {"permissions", m.permissions}, {"computer", m.computer},
            {"folder", m.worktree && m_worktrees->isEnabled() ? "worktree" : "shared"}, {"prompt", m.prompt}};
}

void TeamNewTab::create()
{
    validate();
    if (!m_create->isEnabled()) return;
    QJsonObject lead{{"id", "lead"}, {"role", "Lead"}, {"agent", m_coordinator.agent.isEmpty() ? m_coordinator.name.section('/', 0, 0) : m_coordinator.agent},
                     {"session", QJsonObject{{"host", ""}, {"name", m_coordinator.name}}}};
    if (const auto *role = m_teams->catalog().role("lead")) lead.insert("prompt", role->prompt);
    QJsonArray members;
    Team draft; draft.lead = "lead";
    Teams::Member leadMember; leadMember.id = "lead"; draft.members.append(leadMember);
    QMap<QString, QString> byRole;   // catalog role id -> first member id
    for (const auto &m : m_members) {
        const QString id = nextMemberId(draft, m.role);
        Teams::Member placed; placed.id = id; draft.members.append(placed);
        members.append(memberJson(m, id));
        if (const auto *role = m_teams->catalog().roleNamed(m.role); role && !byRole.contains(role->id)) byRole.insert(role->id, id);
    }
    QJsonArray tasks;
    const auto *t = m_teams->catalog().templ(m_template);
    if (t && m_planningValue == "template") {
        for (int i = 0; i < t->tasks.size(); ++i) {
            const auto &k = t->tasks[i];
            QJsonArray deps; for (int d : k.deps) deps.append(QJsonObject{{"task", d + 1}, {"kind", "hard"}});
            tasks.append(QJsonObject{{"id", i + 1}, {"title", k.title}, {"assignee", k.role == "lead" ? QStringLiteral("lead") : byRole.value(k.role)}, {"deps", deps}});
        }
    }
    const bool worktrees = m_worktrees->isChecked() && m_worktrees->isEnabled();
    const QJsonObject input{{"name", m_name->text().trimmed()}, {"goal", m_goal->toPlainText().trimmed()}, {"folder", m_folder->text().trimmed()},
                            {"project", m_project}, {"template", t ? t->id : QString()}, {"lead_protocol", m_protocol->toPlainText().trimmed()},
                            {"lead", lead}, {"members", members}, {"tasks", tasks},
                            {"start", true},
                            {"settings", QJsonObject{{"folder_policy", worktrees ? "worktree" : "shared"}, {"approval", m_review->isChecked() ? "review" : "auto"},
                                                     {"max_members", m_max->value()}}}};
    m_creating = true;
    m_create->setText(tr("Creating…")); m_progress->show();
    m_summary->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(colorsFor(m_dark).muted.name(),
        tr("Saving the team · sending the planning brief to %1…").arg(m_coordinator.name.toHtmlEscaped())));
    validate();
    m_teams->call(QStringLiteral("create"), input, [this, guard = QPointer<TeamNewTab>(this)](bool ok, const QJsonObject &result, const QString &error) {
        if (!guard) return;
        m_creating = false; m_progress->hide(); m_create->setText(tr("Create team"));
        if (!ok) { validate(); m_reason->setText(error); m_reason->show(); return; }
        if (created) created(result.value("team").toString());
    });
}
