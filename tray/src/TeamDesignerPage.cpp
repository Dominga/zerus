#include "TeamDesignerPage.h"
#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "TeamConversation.h"
#include "TeamWidgets.h"
#include <QComboBox>
#include <QDir>
#include <QHBoxLayout>
#include <QJsonObject>
#include <QMenu>
#include <QScrollArea>
#include <QScrollBar>
#include <QSignalBlocker>
#include <QSplitter>
#include <QTransform>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};

// A header chip: an icon and its text.
QWidget *chip(const QString &icon, const char *name, QLabel **text)
{
    auto *w = new QWidget; w->setObjectName(name); w->setProperty("designerChip", true); w->setAttribute(Qt::WA_StyledBackground);
    auto *h = new QHBoxLayout(w); h->setContentsMargins(8, 2, 9, 2); h->setSpacing(6);
    auto *glyph = new QLabel; glyph->setObjectName("designerChipIcon"); glyph->setProperty("icon", icon); glyph->setFixedSize(12, 12);
    *text = new QLabel; (*text)->setObjectName(name);
    if (icon.isEmpty()) glyph->hide();
    h->addWidget(glyph); h->addWidget(*text, 1);
    w->setObjectName(QByteArray(name) + "Chip");
    return w;
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
    m_open = new QPushButton; m_open->setObjectName("teamDesignerOpen"); m_open->setProperty("ghost", true); m_open->setFixedSize(30, 28);
    m_open->setToolTip(tr("Open in Activity")); m_open->setAccessibleName(tr("Open in Activity")); m_open->setCursor(Qt::PointingHandCursor);
    top->addWidget(m_open);
    hv->addLayout(top);
    auto *projectRow = row(8);
    auto *projectLabel = new QLabel(tr("Project")); projectLabel->setObjectName("teamDesignerProjectLabel");
    m_projectChoice = new QComboBox; m_projectChoice->setObjectName("teamDesignerProject");
    m_projectChoice->setAccessibleName(tr("Project for the Team designer"));
    m_projectChoice->setMinimumContentsLength(18);
    m_projectChoice->setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    projectRow->addWidget(projectLabel); projectRow->addWidget(m_projectChoice, 1);
    hv->addLayout(projectRow);
    QObject::connect(m_projectChoice, &QComboBox::activated, this, [this](int index) {
        if (index < 0) return;
        m_project = m_projectChoice->itemData(index, Qt::UserRole + 2).toString();
        m_folder = m_projectChoice->itemData(index, Qt::UserRole + 1).toString();
        refresh();
    });
    auto *chips = row(6);
    auto *reads = chip(QStringLiteral("folder"), "teamDesignerReads", &m_reads);
    auto *proposes = chip(QStringLiteral("shield"), "teamDesignerProposes", &m_proposes); m_proposes->setText(tr("Proposes only"));
    auto *version = chip(QString(), "teamDesignerVersion", &m_version);
    reads->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred); m_reads->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    chips->addWidget(reads, 1); chips->addWidget(proposes); chips->addWidget(version); chips->addStretch();
    hv->addLayout(chips);
    cv->addWidget(header);
    // Above the composer: the proposals, then the DRAFT bar.
    auto *above = new QWidget; above->setObjectName("teamDesignerAbove");
    auto *av = new QVBoxLayout(above); av->setContentsMargins(16, 0, 16, 0); av->setSpacing(8);
    m_proposals = new QScrollArea; m_proposals->setObjectName("teamDesignerProposals"); m_proposals->setWidgetResizable(true);
    m_proposals->setFrameShape(QFrame::NoFrame); m_proposals->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_proposals->viewport()->setAutoFillBackground(false);
    av->addWidget(m_proposals);
    m_draftBar = new QFrame; m_draftBar->setObjectName("teamDesignerDraftBar");
    auto *dh = new QHBoxLayout(m_draftBar); dh->setContentsMargins(10, 6, 8, 6); dh->setSpacing(8);
    auto *draftMark = new QLabel(QStringLiteral("DRAFT")); draftMark->setObjectName("teamDesignerDraftMark");
    m_draftText = new QLabel; m_draftText->setObjectName("teamDesignerDraftText"); m_draftText->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    m_draftDiscard = button(tr("Discard draft"), "teamDesignerDraftDiscard"); m_draftDiscard->setProperty("danger", true);
    m_draftApply = button(QString(), "teamDesignerApplyKept", true);
    dh->addWidget(draftMark); dh->addWidget(m_draftText, 1); dh->addWidget(m_draftDiscard); dh->addWidget(m_draftApply);
    av->addWidget(m_draftBar);
    // U15: a refused Apply or Discard, under the bar until it succeeds.
    m_draftError = new QLabel; m_draftError->setObjectName("teamDesignerDraftError"); m_draftError->setWordWrap(true);
    m_draftError->setTextFormat(Qt::PlainText); m_draftError->hide();
    av->addWidget(m_draftError);
    m_chat = new TeamConversation(teams->client());
    // U16: the designer's questions and permission requests are answered here.
    m_chat->setQuestionsShown(true);
    m_chat->setAgentName(tr("the designer"));
    m_chat->openTerminal = [this](const QString &host, const QString &session) { if (openTerminal) openTerminal(host, session); };
    m_chat->useDesignerLayout(intro(), above);
    m_chat->openSession = [this] {
        const QString session = designerSession ? designerSession() : QString();
        if (!session.isEmpty() && openDesigner) openDesigner(session);
    };
    m_chat->startRequested = [this](const QString &text) {
        if (m_folder.isEmpty()) {
            startFailed(tr("Choose a project with a folder on this computer first."));
            return;
        }
        const QJsonObject launch{{"agent", m_agent}, {"model", m_agent == "dsh" ? QString() : m_model},
                                 {"effort", m_agent == "dsh" || m_agent == "kimi" ? QString() : m_effort},
                                 {"project", m_project}};
        if (startDesigner) startDesigner(m_folder, text, launch);
    };
    cv->addWidget(m_chat, 1);
    m_split->addWidget(chat);
    // The catalog.
    auto *catalog = new QWidget; catalog->setObjectName("teamDesignerCatalog");
    auto *kv = new QVBoxLayout(catalog); kv->setContentsMargins(16, 12, 16, 10);
    m_editor = new TeamCatalogEditor(teams);
    m_editor->setShowDraft(true);
    m_editor->confirm = [this](const QString &name, const QString &title, const QString &text, const QString &action) { return confirm ? confirm(name, title, text, action) : true; };
    m_editor->askRedo = [this](qint64 version) {
        // The designer redoes its draft in its own conversation; its next
        // proposals replace the out-of-date draft (N38, #548).
        if (!designerSession || designerSession().isEmpty()) return;
        m_chat->send(tr("The catalog changed to v%1 after your draft. Propose your changes again on v%1; your new proposals replace the old draft.").arg(version));
    };
    // N38 (#548): only a running designer is asked; with none, the click would
    // start a new designer session, and the owner discards the draft instead.
    m_editor->canRedo = [this] { return designerSession && !designerSession().isEmpty(); };
    kv->addWidget(m_editor);
    m_split->addWidget(catalog);
    m_split->setStretchFactor(0, 0); m_split->setStretchFactor(1, 1);
    m_split->setSizes({520, 1000});
    outer->addWidget(m_split, 1);
    QObject::connect(m_open, &QPushButton::clicked, this, [this] {
        const QString session = designerSession ? designerSession() : QString();
        if (!session.isEmpty() && openDesigner) openDesigner(session);
    });
    QObject::connect(m_chat->modelButton(), &QPushButton::clicked, this, [this] { showModelMenu(); });
    QObject::connect(m_draftApply, &QPushButton::clicked, this, [this] { m_editor->applyKept(); });
    QObject::connect(m_draftDiscard, &QPushButton::clicked, this, [this] { m_editor->discardDraft(); });
    QObject::connect(m_teams, &TeamController::catalogChanged, this, [this] { refresh(); });
    setDark(true);
}

QWidget *TeamDesignerPage::intro()
{
    auto *w = new QWidget; w->setObjectName("teamDesignerEmpty");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(40, 12, 40, 8); v->setSpacing(8);
    auto *icon = new QLabel; icon->setObjectName("teamDesignerEmptyIcon"); icon->setFixedSize(40, 40); icon->setAlignment(Qt::AlignCenter);
    v->addWidget(icon, 0, Qt::AlignHCenter);
    auto *title = new QLabel(tr("Design roles and teams with an agent")); title->setObjectName("teamDesignerEmptyTitle");
    title->setWordWrap(true); title->setAlignment(Qt::AlignHCenter);
    v->addWidget(title);
    auto *text = new QLabel(tr("Choose a project and describe the work. The designer reads the project and the existing catalog, "
                               "asks about your tasks, reuses suitable roles and teams, and proposes only what is missing. "
                               "It never saves anything — you keep or reject each proposal and apply them."));
    text->setObjectName("teamDesignerEmptyText"); text->setWordWrap(true); text->setAlignment(Qt::AlignHCenter);
    v->addWidget(text);
    return w;
}

void TeamDesignerPage::setProjects(const QList<Project> &projects, const QString &defaultProject)
{
    const QSignalBlocker block(m_projectChoice);
    m_projectChoice->clear();
    m_projectChoice->addItem(projects.isEmpty() ? tr("No local project folders") : tr("Choose a project…"));
    int current = -1, fallback = -1;
    for (const auto &project : projects) {
        if (project.folder.isEmpty()) continue;
        QString title = project.name;
        if (!project.folderName.isEmpty()) title += tr(" — %1").arg(project.folderName);
        const int index = m_projectChoice->count();
        m_projectChoice->addItem(title, project.id);
        m_projectChoice->setItemData(index, project.folder, Qt::UserRole + 1);
        m_projectChoice->setItemData(index, project.name, Qt::UserRole + 2);
        m_projectChoice->setItemData(index, QDir::toNativeSeparators(project.folder), Qt::ToolTipRole);
        if (project.id == defaultProject && fallback < 0) fallback = index;
        if (project.folder == m_folder && (m_project.isEmpty() || project.id == m_project || project.name == m_project)) current = index;
    }
    if (current < 0) current = fallback >= 0 ? fallback : m_projectChoice->count() > 1 ? 1 : 0;
    m_projectChoice->setCurrentIndex(current);
    m_project = m_projectChoice->itemData(current, Qt::UserRole + 2).toString();
    m_folder = m_projectChoice->itemData(current, Qt::UserRole + 1).toString();
    refresh();
}

void TeamDesignerPage::setContext(const QString &folder, const QString &project)
{
    m_folder = folder; m_project = project;
    const QSignalBlocker block(m_projectChoice);
    int index = -1;
    for (int i = 1; i < m_projectChoice->count(); ++i) {
        if (m_projectChoice->itemData(i, Qt::UserRole + 1).toString() == folder
            && (project.isEmpty() || m_projectChoice->itemData(i).toString() == project
                || m_projectChoice->itemData(i, Qt::UserRole + 2).toString() == project)) {
            index = i;
            break;
        }
    }
    if (index < 0 && !folder.isEmpty()) {
        index = m_projectChoice->count();
        m_projectChoice->addItem(project.isEmpty() ? QDir::toNativeSeparators(folder) : project, project);
        m_projectChoice->setItemData(index, folder, Qt::UserRole + 1);
        m_projectChoice->setItemData(index, project, Qt::UserRole + 2);
    }
    m_projectChoice->setCurrentIndex(qMax(0, index));
    if (index >= 0) m_project = m_projectChoice->itemData(index, Qt::UserRole + 2).toString();
    refresh();
}

void TeamDesignerPage::startFailed(const QString &error)
{
    m_chat->startFinished(error.isEmpty() ? tr("The Team designer did not start.") : tr("The Team designer did not start: %1").arg(error));
}

void TeamDesignerPage::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamDesignerChat{border-right:1px solid %1;}"
        "QWidget#teamDesignerChatHeader{border-bottom:1px solid %1;}"
        "QLabel#teamDesignerPageTitle{color:%2;font-size:17px;font-weight:600;}"
        "QWidget[designerChip=\"true\"]{background:%7;border:1px solid %1;border-radius:7px;}"
        "QWidget[designerChip=\"true\"] QLabel{color:%3;font-size:12px;background:transparent;border:none;}"
        "QLabel#teamDesignerTile{background:%4;border-radius:7px;}"
        "QLabel#teamDesignerEmptyTitle{color:%2;font-size:17px;font-weight:600;}"
        "QLabel#teamDesignerEmptyText{color:%5;font-size:13px;}"
        "QFrame#teamDesignerProposal{background:%6;border:1px solid %1;border-radius:10px;}"
        "QScrollArea#teamDesignerProposals,QWidget#teamDesignerProposalList{background:transparent;}"
        "QFrame#teamDesignerDraftBar{background:transparent;border:1px dashed %8;border-radius:9px;}"
        "QLabel#teamDesignerDraftMark{color:%9;background:%10;border:1px solid %8;border-radius:4px;padding:0 5px;font-size:10px;font-weight:700;}"
        "QLabel#teamDesignerDraftText{color:%3;font-size:12px;}")
        .arg(c.border.name(), c.fg.name(), c.text2.name(), a(c.accent, .18), c.muted.name(), c.cardFill.name(), c.tile.name(),
             a(c.folder, .55), c.folder.name(), a(c.folder, .10)));
    m_projectChoice->setStyleSheet(QStringLiteral(
        "QComboBox#teamDesignerProject{background:%1;color:%2;border:1px solid %3;border-radius:7px;padding:5px 9px;}"
        "QComboBox#teamDesignerProject:disabled{color:%4;}"
        "QComboBox#teamDesignerProject::drop-down{border:none;width:22px;}"
        "QComboBox#teamDesignerProject QAbstractItemView{background:%1;color:%2;selection-background-color:%5;}")
        .arg(c.surface.name(), c.fg.name(), c.border.name(), c.muted.name(), c.tile.name()));
    findChild<QLabel *>("teamDesignerTile")->setPixmap(workspaceIcon("team", c.accent).pixmap(16, 16));
    findChild<QLabel *>("teamDesignerEmptyIcon")->setPixmap(workspaceIcon("team", c.accent).pixmap(36, 36));
    for (auto *glyph : findChildren<QLabel *>("designerChipIcon"))
        if (!glyph->property("icon").toString().isEmpty()) glyph->setPixmap(workspaceIcon(glyph->property("icon").toString(), c.text2).pixmap(12, 12));
    // Open in Activity: an arrow up out of the column.
    m_open->setIcon(workspaceIcon("external", c.text2));
    m_editor->setDark(dark);
    m_chat->setTheme(dark);
    refresh();
}

void TeamDesignerPage::refresh()
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    const QString session = designerSession ? designerSession() : QString();
    const Teams::Live live = session.isEmpty() || !designerLive ? Teams::Live{} : designerLive();
    // N38 (#548): the redo offer follows whether a designer runs now.
    m_editor->refreshBanner();
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
    m_status->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:11px;padding:2px 12px;font-size:12px;")
                                .arg(tone.name(), alpha(tone, .6).name(QColor::HexArgb)));
    m_reads->setText(tr("Reads %1 (project)").arg(m_folder.isEmpty() ? tr("no folder") : QDir::toNativeSeparators(m_folder)));
    m_reads->setToolTip(m_folder);
    m_projectChoice->setEnabled(session.isEmpty() && m_projectChoice->count() > 1);
    m_projectChoice->setToolTip(session.isEmpty() ? tr("Choose the project the designer reads")
                                                  : tr("The running designer keeps its original project"));
    m_version->setText(catalog.builtin ? tr("Built-in catalog") : tr("Catalog v%1").arg(catalog.version));
    m_open->setVisible(!session.isEmpty());
    m_chat->setSession({}, session);
    m_chat->setPlaceholder(session.isEmpty() ? m_folder.isEmpty() ? tr("Choose a project above…") : tr("Describe the project tasks…")
                                             : tr("Ask for changes…"));
    m_editor->setDesignerWorking(live.working);
    updateModelButton();
    renderProposals();
    renderDraftBar();
}

void TeamDesignerPage::updateModelButton()
{
    auto *b = m_chat->modelButton();
    const QString session = designerSession ? designerSession() : QString();
    if (session.isEmpty()) {
        QStringList parts{m_agent == "dsh" ? agentNames.value(m_agent) : m_model.isEmpty() ? agentNames.value(m_agent) : m_model};
        if (!m_effort.isEmpty() && m_agent != "dsh" && m_agent != "kimi") parts << m_effort;
        b->setText(parts.join(' '));
        b->setIcon(workspaceIcon("chevron-down", colorsFor(m_dark).text2)); b->setLayoutDirection(Qt::RightToLeft);
        b->setToolTip(tr("The designer's agent, model and effort"));
        b->setEnabled(true);
    } else {
        // A running designer keeps its model; its session changes it.
        const auto details = m_chat->details();
        QStringList parts{details.value("model").toString(details.value("agent").toString(tr("Designer")))};
        if (!details.value("effort").toString().isEmpty()) parts << details.value("effort").toString();
        b->setText(parts.join(' ')); b->setIcon(QIcon());
        b->setToolTip(tr("Change the designer's model in its own session"));
        b->setEnabled(false);
    }
}

// Models by agent, then the effort; a choice applies at once.
void TeamDesignerPage::showModelMenu()
{
    auto *menu = new QMenu(this); menu->setObjectName("teamDesignerModelMenu"); menu->setAttribute(Qt::WA_DeleteOnClose);
    for (const auto &agent : agents) {
        menu->addSection(agentNames.value(agent));
        QStringList models = Teams::knownModels(agent);
        // DeepSeek runs its own model.
        if (models.isEmpty() || agent == "dsh") models = QStringList{QString()};
        for (const auto &model : models) {
            auto *action = menu->addAction(model.isEmpty() ? tr("Default model") : model);
            action->setCheckable(true); action->setChecked(agent == m_agent && model == m_model);
            QObject::connect(action, &QAction::triggered, this, [this, agent, model] { m_agent = agent; m_model = model; updateModelButton(); });
        }
    }
    menu->addSeparator();
    auto *efforts = menu->addMenu(tr("Effort") + QStringLiteral("  ·  ") + (m_effort.isEmpty() ? QStringLiteral("—") : m_effort));
    efforts->setEnabled(m_agent != "dsh" && m_agent != "kimi");
    for (const auto *effort : {"low", "medium", "high", "xhigh"}) {
        auto *action = efforts->addAction(QString::fromLatin1(effort));
        action->setCheckable(true); action->setChecked(m_effort == QLatin1String(effort));
        QObject::connect(action, &QAction::triggered, this, [this, effort] { m_effort = QString::fromLatin1(effort); updateModelButton(); });
    }
    menu->addSeparator();
    auto *note = menu->addAction(tr("Runs on this computer")); note->setEnabled(false);
    auto *b = m_chat->modelButton();
    const QSize size = menu->sizeHint();
    menu->popup(b->mapToGlobal(QPoint(b->width() - size.width(), -size.height() - 6)));
}

// The DRAFT bar above the composer repeats the catalog's draft header.
void TeamDesignerPage::renderDraftBar()
{
    const auto &catalog = m_teams->catalog();
    int kept = 0, undecided = 0;
    for (const auto &p : catalog.proposals) { kept += p.state == "kept"; undecided += p.state == "proposed"; }
    const bool stale = catalog.draftBase != catalog.version;
    m_draftBar->setVisible(!catalog.proposals.isEmpty());
    const QString error = m_teams->draftError();
    m_draftError->setText(error.isEmpty() ? QString() : QStringLiteral("⚠ ") + error);
    m_draftError->setStyleSheet(QStringLiteral("QLabel{color:%1;font-size:11px;}").arg(colorsFor(m_dark).danger.name()));
    m_draftError->setVisible(!error.isEmpty());
    m_draftText->setText(tr("based on v%1 · %2 of %3 kept · %4 undecided").arg(catalog.draftBase).arg(kept).arg(catalog.proposals.size()).arg(undecided));
    m_draftText->setToolTip(m_draftText->text());
    m_draftApply->setText(QStringLiteral("✓ ") + (kept == 1 ? tr("Apply 1 change") : tr("Apply %1 changes").arg(kept)));
    m_draftApply->setEnabled(kept > 0 && !stale);
    m_draftApply->setToolTip(stale ? tr("The catalog changed after this draft; applying would overwrite those edits.")
                                   : tr("Write the kept changes as the next catalog version"));
}

void TeamDesignerPage::renderProposals()
{
    const Colors c = colorsFor(m_dark);
    const auto &catalog = m_teams->catalog();
    const int scroll = m_proposals->verticalScrollBar()->value();
    auto *content = new QWidget; content->setObjectName("teamDesignerProposalList");
    auto *v = new QVBoxLayout(content); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    // U15: decisions go through the controller, with or without a designer session.
    const auto decide = [this](const QString &id, const QString &decision) { m_teams->decideProposal(id, decision); };
    if (!m_closed.isEmpty()) {
        auto *bar = row(6); bar->addStretch();
        auto *reopen = button(tr("Show closed"), "teamDesignerShowClosed"); reopen->setProperty("ghost", true);
        QObject::connect(reopen, &QPushButton::clicked, this, [this] { m_closed.clear(); renderProposals(); });
        bar->addWidget(reopen);
        v->addLayout(bar);
    }
    int shown = 0;
    for (const auto &p : catalog.proposals) {
        if (p.state != "proposed" && m_closed.contains(p.id)) continue;
        ++shown;
        const bool rejected = p.state == "rejected";
        auto *card = new QFrame; card->setObjectName("teamDesignerProposal");
        auto *cv = new QVBoxLayout(card); cv->setContentsMargins(14, 10, 12, 10); cv->setSpacing(5);
        auto *head = row(8);
        const QString markText = p.action == "add" ? QStringLiteral("NEW") : p.action == "remove" ? QStringLiteral("REMOVED") : QStringLiteral("CHANGED");
        const QColor markColor = p.action == "add" ? c.accent : p.action == "remove" ? c.danger : c.folder;
        auto *m = label(markText, markColor, 10, true);
        m->setStyleSheet(QStringLiteral("color:%1;background:%2;border:1px solid %3;border-radius:4px;padding:0 5px;")
                             .arg(markColor.name(), alpha(markColor, .10).name(QColor::HexArgb), alpha(markColor, .55).name(QColor::HexArgb)));
        head->addWidget(m);
        auto *title = elided(p.title, rejected ? c.dim : c.fg, 13); QFont tf = title->font(); tf.setWeight(QFont::DemiBold); tf.setStrikeOut(rejected); title->setFont(tf);
        head->addWidget(title, 1);
        if (p.state == "proposed") {
            auto *reject = button(tr("× Reject"), "teamProposalReject"); reject->setProperty("ghost", true);
            auto *keep = button(tr("✓ Keep"), "teamProposalKeep");
            QObject::connect(reject, &QPushButton::clicked, this, [decide, id = p.id] { decide(id, "reject"); });
            QObject::connect(keep, &QPushButton::clicked, this, [decide, id = p.id] { decide(id, "keep"); });
            head->addWidget(reject); head->addWidget(keep);
        } else {
            head->addWidget(label(p.state == "kept" ? tr("✓ Kept") : tr("Rejected"), p.state == "kept" ? c.accent : c.danger, 12));
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
        if (!p.reason.isEmpty()) { auto *reason = label(p.reason, c.text2, 12); reason->setWordWrap(true); cv->addWidget(reason); }
        auto *show = button(tr("Show in catalog"), "teamProposalShow"); show->setProperty("ghost", true);
        show->setStyleSheet(QStringLiteral("QPushButton{padding:2px 0;color:%1;}").arg(c.fg.name()));
        QObject::connect(show, &QPushButton::clicked, this, [this, id = p.id] { m_editor->showProposal(id); });
        cv->addWidget(show, 0, Qt::AlignLeft);
        // A refused decision stays with its proposal until a decision lands.
        if (const auto error = m_teams->proposalError(p.id); !error.isEmpty()) cv->addWidget(refusal(error, c, "teamProposalError"));
        if (rejected) card->setStyleSheet(QStringLiteral("QFrame#teamDesignerProposal{background:transparent;}"));
        v->addWidget(card);
    }
    retire(m_proposals->takeWidget());
    m_proposals->setWidget(content);
    m_proposals->verticalScrollBar()->setValue(scroll);
    m_proposals->setVisible(shown > 0 || !m_closed.isEmpty());
    // The cards take their height up to about half the column; then they scroll.
    content->adjustSize();
    m_proposals->setFixedHeight(qMin(content->sizeHint().height() + 2, qMax(160, height() / 2 - 80)));
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
