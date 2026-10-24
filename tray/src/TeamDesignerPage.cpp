#include "TeamDesignerPage.h"
#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamWidgets.h"
#include <QComboBox>
#include <QDir>
#include <QJsonObject>
#include <QLineEdit>
#include <QHBoxLayout>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QScrollBar>
#include <QSplitter>
#include <QStackedWidget>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};

QLabel *chip(const QString &text, const char *name)
{
    auto *l = new QLabel(text); l->setObjectName(name);
    l->setProperty("designerChip", true);
    return l;
}
}

TeamDesignerPage::TeamDesignerPage(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamDesignerPage");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    m_split = new QSplitter(Qt::Horizontal); m_split->setObjectName("teamDesignerSplit"); m_split->setChildrenCollapsible(false);
    // The chat column.
    auto *chat = new QWidget; chat->setObjectName("teamDesignerChat"); chat->setMinimumWidth(380);
    auto *cv = new QVBoxLayout(chat); cv->setContentsMargins(0, 0, 0, 0); cv->setSpacing(0);
    auto *header = new QWidget; header->setObjectName("teamDesignerChatHeader");
    auto *hv = new QVBoxLayout(header); hv->setContentsMargins(14, 10, 14, 10); hv->setSpacing(8);
    auto *top = row(8);
    auto *tile = new QLabel; tile->setObjectName("teamDesignerTile"); tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
    top->addWidget(tile);
    m_title = new QLabel(tr("Team designer")); m_title->setObjectName("teamDesignerPageTitle");
    top->addWidget(m_title, 1);
    m_status = new QLabel; m_status->setObjectName("teamDesignerAgentStatus");
    top->addWidget(m_status);
    m_open = button(tr("Open in Activity"), "teamDesignerOpen"); m_open->setProperty("ghost", true);
    top->addWidget(m_open);
    hv->addLayout(top);
    auto *chips = row(6);
    m_reads = chip(QString(), "teamDesignerReads"); m_proposes = chip(tr("Proposes only"), "teamDesignerProposes"); m_version = chip(QString(), "teamDesignerVersion");
    m_reads->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    chips->addWidget(m_reads, 1); chips->addWidget(m_proposes); chips->addWidget(m_version);
    hv->addLayout(chips);
    cv->addWidget(header);
    m_chatStack = new QStackedWidget;
    m_chatStack->addWidget(emptyState());
    m_chatSplit = new QSplitter(Qt::Vertical); m_chatSplit->setChildrenCollapsible(false);
    m_proposals = new QScrollArea; m_proposals->setObjectName("teamDesignerProposals"); m_proposals->setWidgetResizable(true);
    m_proposals->setFrameShape(QFrame::NoFrame); m_proposals->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_chat = new TeamConversation(teams->client());
    m_chat->setPlaceholder(tr("Ask for changes…"));
    m_chat->setQuickAsks({tr("Explain current roles"), tr("Make a cheaper team for docs")});
    m_chatSplit->addWidget(m_chat); m_chatSplit->addWidget(m_proposals);
    m_chatSplit->setStretchFactor(0, 3); m_chatSplit->setStretchFactor(1, 2);
    m_chatStack->addWidget(m_chatSplit);
    cv->addWidget(m_chatStack, 1);
    m_split->addWidget(chat);
    // The catalog.
    auto *catalog = new QWidget; catalog->setObjectName("teamDesignerCatalog");
    auto *kv = new QVBoxLayout(catalog); kv->setContentsMargins(14, 10, 14, 10);
    m_editor = new TeamCatalogEditor(teams);
    m_editor->setShowDraft(true);
    m_editor->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm ? confirm(name, title, text, action) : true; };
    m_editor->askRedo = [this](qint64 version) {
        // The designer redoes its draft in its own conversation.
        m_chat->send(tr("The catalog changed to v%1 after your draft. Discard your proposals and propose again on v%1.").arg(version));
    };
    kv->addWidget(m_editor);
    m_split->addWidget(catalog);
    m_split->setStretchFactor(0, 0); m_split->setStretchFactor(1, 1);
    m_split->setSizes({540, 1000});
    outer->addWidget(m_split, 1);
    QObject::connect(m_open, &QPushButton::clicked, this, [this] {
        const QString session = designerSession ? designerSession() : QString();
        if (!session.isEmpty() && openDesigner) openDesigner(session);
    });
    QObject::connect(m_teams, &TeamController::catalogChanged, this, [this] { refresh(); });
    setDark(true);
}

QWidget *TeamDesignerPage::emptyState()
{
    auto *w = new QWidget; w->setObjectName("teamDesignerEmpty");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(24, 24, 24, 24); v->setSpacing(10);
    v->addStretch();
    auto *icon = new QLabel; icon->setObjectName("teamDesignerEmptyIcon"); icon->setFixedSize(36, 36);
    v->addWidget(icon);
    auto *title = new QLabel(tr("Design roles and teams with an agent")); title->setObjectName("teamDesignerEmptyTitle"); title->setWordWrap(true);
    v->addWidget(title);
    auto *text = new QLabel(tr("The designer reads the selected session's folder and the team history, and proposes changes to roles, "
                               "templates and routing. <b>It never saves anything</b> — you keep or reject each proposal and apply them."));
    text->setObjectName("teamDesignerEmptyText"); text->setWordWrap(true); text->setTextFormat(Qt::RichText);
    v->addWidget(text);
    m_goal = new QPlainTextEdit; m_goal->setObjectName("teamDesignerGoal"); m_goal->setFixedHeight(84);
    m_goal->setPlaceholderText(tr("What do you need? e.g. a security reviewer for payment code and a small team for bugfixes"));
    v->addWidget(m_goal);
    auto *examplesBox = new QWidget; auto *examples = new FlowLayout(6); examplesBox->setLayout(examples);
    for (const auto &example : {tr("Security reviewer for payments"), tr("Team for small bugfixes"), tr("Cheaper team for docs"), tr("Explain current roles")}) {
        auto *b = button(example, "teamDesignerExample"); b->setProperty("chip", true);
        QObject::connect(b, &QPushButton::clicked, this, [this, example] { m_goal->setPlainText(example); });
        examples->addWidget(b);
    }
    v->addWidget(examplesBox);
    // Which agent designs, on which model; it runs on this computer.
    auto *agentRow = row(6);
    m_agent = new QComboBox; m_agent->setObjectName("teamDesignerAgent");
    for (const auto &a : agents) m_agent->addItem(workspaceIcon(a, colorsFor(true).fg), agentNames.value(a), a);
    m_model = new QComboBox; m_model->setObjectName("teamDesignerModel"); m_model->setEditable(true);
    m_model->lineEdit()->setPlaceholderText(tr("default model"));
    m_effort = new QComboBox; m_effort->setObjectName("teamDesignerEffort");
    m_effort->addItem(QStringLiteral("—"), QString());
    for (const auto *e : {"low", "medium", "high", "xhigh"}) m_effort->addItem(QString::fromLatin1(e), QString::fromLatin1(e));
    const auto models = [this] {
        // Suggestions only: the field takes any model the agent's command line knows.
        const QString agent = m_agent->currentData().toString();
        const QString keep = m_model->currentText();
        m_model->clear(); m_model->addItems(Teams::knownModels(agent));
        m_model->setCurrentText(Teams::knownModels(agent).contains(keep) ? keep : QString());
        // DeepSeek runs its own model; Kimi has no effort.
        m_model->setEnabled(agent != "dsh"); m_effort->setEnabled(agent != "dsh" && agent != "kimi");
    };
    QObject::connect(m_agent, &QComboBox::currentIndexChanged, this, models);
    models();
    m_model->setCurrentText(QStringLiteral("opus")); m_effort->setCurrentIndex(m_effort->findData(QStringLiteral("high")));
    agentRow->addWidget(m_agent, 4); agentRow->addWidget(m_model, 5); agentRow->addWidget(m_effort, 2);
    v->addLayout(agentRow);
    auto *bottom = row(8);
    auto *where = label(tr("Runs on this computer"), colorsFor(true).dim, 11); where->setObjectName("teamDesignerWhere");
    bottom->addWidget(where);
    bottom->addStretch();
    m_start = button(tr("Start designer"), "teamDesignerStart", true);
    QObject::connect(m_start, &QPushButton::clicked, this, [this] {
        const QJsonObject launch{{"agent", m_agent->currentData().toString()}, {"model", m_model->isEnabled() ? m_model->currentText().trimmed() : QString()},
                                 {"effort", m_effort->isEnabled() ? m_effort->currentData().toString() : QString()}};
        if (startDesigner) startDesigner(m_folder, m_goal->toPlainText().trimmed(), launch);
        m_start->setEnabled(false); m_start->setText(tr("Starting…"));
    });
    bottom->addWidget(m_start);
    v->addLayout(bottom);
    v->addStretch();
    return w;
}

void TeamDesignerPage::setContext(const QString &folder, const QString &project)
{
    m_folder = folder; m_project = project;
    refresh();
}

void TeamDesignerPage::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamDesignerChat{border-right:1px solid %1;}"
        "QWidget#teamDesignerChatHeader{border-bottom:1px solid %1;}"
        "QLabel#teamDesignerPageTitle{color:%2;font-size:16px;font-weight:600;}"
        "QLabel[designerChip=\"true\"]{color:%3;border:1px solid %1;border-radius:9px;padding:1px 8px;font-size:11px;}"
        "QLabel#teamDesignerTile{background:%4;border-radius:7px;}"
        "QLabel#teamDesignerEmptyTitle{color:%2;font-size:15px;font-weight:600;}"
        "QLabel#teamDesignerEmptyText{color:%5;font-size:12px;}"
        "QFrame#teamDesignerProposal{background:%6;border:1px solid %1;border-radius:10px;}")
        .arg(c.border.name(), c.fg.name(), c.text2.name(), alpha(c.accent, .18).name(QColor::HexArgb), c.muted.name(), c.surface.name()));
    findChild<QLabel *>("teamDesignerTile")->setPixmap(workspaceIcon("team", c.accent).pixmap(16, 16));
    findChild<QLabel *>("teamDesignerEmptyIcon")->setPixmap(workspaceIcon("team", c.accent).pixmap(32, 32));
    m_editor->setDark(dark);
    m_chat->setTheme(dark);
    refresh();
}

void TeamDesignerPage::refresh()
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    const QString session = designerSession ? designerSession() : QString();
    const Live live = session.isEmpty() || !designerLive ? Live{} : designerLive();
    int undecided = 0; for (const auto &p : catalog.proposals) undecided += p.state == "proposed";
    const bool stale = !catalog.proposals.isEmpty() && catalog.draftBase != catalog.version;
    QString status; QColor tone = c.muted;
    if (session.isEmpty()) status = tr("Not started");
    else if (live.attention) { status = tr("Needs your answer"); tone = c.warn; }
    else if (live.working) { status = tr("Working"); tone = c.accent; }
    else if (stale) { status = tr("Draft is out of date"); tone = c.warn; }
    else if (undecided) { status = tr("Waiting for your decision"); tone = c.folder; }
    else status = tr("Idle");
    m_status->setText(status);
    m_status->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:9px;padding:1px 8px;font-size:11px;")
                                .arg(tone.name(), alpha(tone, .5).name(QColor::HexArgb)));
    m_reads->setText(tr("Reads %1 (selected session)").arg(m_folder.isEmpty() ? tr("no folder") : QDir::toNativeSeparators(m_folder)));
    m_reads->setToolTip(m_folder);
    m_version->setText(catalog.builtin ? tr("Built-in catalog") : tr("Catalog v%1").arg(catalog.version));
    m_open->setVisible(!session.isEmpty());
    m_chatStack->setCurrentIndex(session.isEmpty() ? 0 : 1);
    if (!session.isEmpty()) m_chat->setSession({}, session);
    else if (m_start) { m_start->setEnabled(true); m_start->setText(tr("Start designer")); }
    m_editor->setDesignerWorking(live.working);
    renderProposals();
}

void TeamDesignerPage::renderProposals()
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    const int scroll = m_proposals->verticalScrollBar()->value();
    auto *content = new QWidget; content->setObjectName("teamDesignerProposalList");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(10, 8, 10, 8); v->setSpacing(6);
    const auto decide = [this](const QString &id, const QString &decision) {
        m_teams->call(QStringLiteral("catalog decide"), {{"proposal", id}, {"decision", decision}}, [](bool, const QJsonObject &, const QString &) {});
    };
    // Apply stands with the proposals it writes.
    int kept = 0, undecided = 0;
    for (const auto &p : catalog.proposals) { kept += p.state == "kept"; undecided += p.state == "proposed"; }
    const bool stale = catalog.draftBase != catalog.version;
    auto *bar = row(6);
    QStringList counts;
    if (kept) counts << tr("%n kept", nullptr, kept);
    if (undecided) counts << tr("%n undecided", nullptr, undecided);
    bar->addWidget(label(counts.isEmpty() ? tr("No change kept") : counts.join(QStringLiteral(" · ")), c.muted, 11), 1);
    if (!m_closed.isEmpty()) {
        auto *reopen = button(tr("Show closed"), "teamDesignerShowClosed"); reopen->setProperty("ghost", true);
        QObject::connect(reopen, &QPushButton::clicked, this, [this] { m_closed.clear(); renderProposals(); });
        bar->addWidget(reopen);
    }
    auto *apply = button(kept == 1 ? tr("Apply 1 change") : tr("Apply %1 changes").arg(kept), "teamDesignerApplyKept", true);
    apply->setEnabled(kept > 0 && !stale);
    apply->setToolTip(stale ? tr("The catalog changed after this draft; applying would overwrite those edits.")
                            : tr("Write the kept changes as the next catalog version"));
    QObject::connect(apply, &QPushButton::clicked, this, [this] { m_editor->applyKept(); });
    bar->addWidget(apply);
    v->addLayout(bar);
    for (const auto &p : catalog.proposals) {
        if (p.state != "proposed" && m_closed.contains(p.id)) continue;
        const bool rejected = p.state == "rejected";
        auto *card = new QFrame; card->setObjectName("teamDesignerProposal");
        auto *cv = new QVBoxLayout(card); cv->setContentsMargins(8, 6, 8, 6); cv->setSpacing(3);
        auto *head = row(6);
        const QString markText = p.action == "add" ? QStringLiteral("NEW") : p.action == "remove" ? QStringLiteral("REMOVED") : QStringLiteral("CHANGED");
        const QColor markColor = p.action == "add" ? c.accent : p.action == "remove" ? c.danger : c.folder;
        auto *m = label(markText, markColor, 9, true);
        m->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:3px;padding:0 4px;").arg(markColor.name(), alpha(markColor, .6).name(QColor::HexArgb)));
        head->addWidget(m);
        auto *title = elided(p.title, rejected ? c.dim : c.fg, 12); QFont tf = title->font(); tf.setWeight(QFont::DemiBold); tf.setStrikeOut(rejected); title->setFont(tf);
        head->addWidget(title, 1);
        if (p.state == "proposed") {
            auto *reject = button(tr("× Reject"), "teamProposalReject"); reject->setProperty("ghost", true);
            auto *keep = button(tr("✓ Keep"), "teamProposalKeep");
            QObject::connect(reject, &QPushButton::clicked, this, [decide, id = p.id] { decide(id, "reject"); });
            QObject::connect(keep, &QPushButton::clicked, this, [decide, id = p.id] { decide(id, "keep"); });
            head->addWidget(reject); head->addWidget(keep);
        } else {
            head->addWidget(label(p.state == "kept" ? tr("✓ Kept") : tr("Rejected"), p.state == "kept" ? c.accent : c.danger, 11));
            auto *undo = button(tr("Undo"), "teamProposalUndo"); undo->setProperty("ghost", true);
            QObject::connect(undo, &QPushButton::clicked, this, [decide, id = p.id] { decide(id, "undo"); });
            head->addWidget(undo);
            // A decided card can be closed; its decision stays.
            auto *close = button(QStringLiteral("×"), "teamProposalClose"); close->setProperty("ghost", true);
            close->setToolTip(tr("Close this card; the decision stays")); close->setAccessibleName(tr("Close"));
            QObject::connect(close, &QPushButton::clicked, this, [this, id = p.id] { m_closed.insert(id); renderProposals(); });
            head->addWidget(close);
        }
        cv->addLayout(head);
        auto *foot = row(6);
        auto *reason = label(p.reason, c.muted, 11); reason->setWordWrap(true);
        foot->addWidget(reason, 1);
        auto *show = button(tr("Show in catalog"), "teamProposalShow"); show->setProperty("ghost", true);
        QObject::connect(show, &QPushButton::clicked, this, [this, id = p.id] { m_editor->showProposal(id); });
        foot->addWidget(show, 0, Qt::AlignTop);
        cv->addLayout(foot);
        if (rejected) { auto *effect = card; effect->setStyleSheet(QStringLiteral("QFrame#teamDesignerProposal{background:transparent;}")); }
        v->addWidget(card);
    }
    v->addStretch();
    retire(m_proposals->takeWidget());
    m_proposals->setWidget(content);
    m_proposals->verticalScrollBar()->setValue(scroll);
    m_proposals->setVisible(!catalog.proposals.isEmpty());
}

void TeamDesignerPage::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_teams->setWatchCatalogPage(true);
    refresh();
}

void TeamDesignerPage::hideEvent(QHideEvent *event)
{
    QWidget::hideEvent(event);
    m_teams->setWatchCatalogPage(false);
}
