#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "HgsClient.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QComboBox>
#include <QFile>
#include <QFileDialog>
#include <QGridLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QSpinBox>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QMap<QString, QString> agentNames{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
const QStringList swatches{"#b79cff", "#8e9bff", "#6fa8ff", "#f08bb4", "#56c8e0", "#d9b38c", "#e48de0", "#a3b1c6", "#b9d36b"};
constexpr int listWidth = 230, labelWidth = 120, rowHeight = 33;

QComboBox *choice(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name)
{
    auto *box = new QComboBox; box->setObjectName(name);
    for (const auto &[value, text] : items) box->addItem(text, value);
    if (box->findData(current) < 0 && !current.isEmpty()) box->addItem(current, current);
    box->setCurrentIndex(qMax(0, box->findData(current)));
    return box;
}

QString slug(const QString &name)
{
    QString id;
    for (const auto ch : name.toLower()) id += ch.isLetterOrNumber() && ch.unicode() < 128 ? ch : QChar('-');
    while (id.contains("--")) id.replace("--", "-");
    id = id.left(40);
    while (id.startsWith('-')) id.remove(0, 1);
    while (id.endsWith('-')) id.chop(1);
    return id.isEmpty() ? QStringLiteral("item") : id;
}

// English plurals without a translation file: "1 change", "3 changes".
QString counted(int n, const QString &one, const QString &many) { return (n == 1 ? one : many).arg(n); }

// A catalog value as one line for the diff.
QString shown(const QString &key, const QJsonValue &value)
{
    if (value.isUndefined() && key == "dangerous") return QStringLiteral("no");
    if (key == "roster") {
        QStringList parts;
        for (const auto &v : value.toArray()) {
            const auto o = v.toObject();
            parts << o.value("role").toString() + (o.value("optional").toBool() ? QStringLiteral(" 0–1") : o.value("count").toInt(1) > 1 ? QStringLiteral(" × %1").arg(o.value("count").toInt()) : QString());
        }
        return parts.join(QStringLiteral(", "));
    }
    if (key == "tasks") {
        QStringList parts;
        for (const auto &v : value.toArray()) parts << v.toObject().value("title").toString();
        return parts.join(QStringLiteral(" → "));
    }
    if (value.isArray()) { QStringList parts; for (const auto &v : value.toArray()) parts << v.toVariant().toString(); return parts.join(QStringLiteral(", ")); }
    if (value.isBool()) return value.toBool() ? QStringLiteral("yes") : QStringLiteral("no");
    if (value.isDouble()) return QString::number(value.toDouble());
    return value.toString();
}

QList<std::pair<QString, QString>> fieldsOf(const QString &kind)
{
    const auto t = [](const char *text) { return QCoreApplication::translate("TeamCatalogEditor", text); };
    if (kind == "role")
        return {{"name", t("Name")}, {"tags", t("Specialization")}, {"agent", t("Default agent")}, {"account", t("Account")}, {"model", t("Model")},
                {"effort", t("Effort")}, {"computer", t("Computer")}, {"permissions", t("Permissions")}, {"folder", t("Folder policy")},
                {"can_message", t("Can message")}, {"dangerous", t("Dangerous actions")}, {"prompt", t("Role prompt")}};
    if (kind == "template")
        return {{"name", t("Name")}, {"description", t("Description")}, {"lead_protocol", t("Lead protocol")}, {"roster", t("Roster")},
                {"tasks", t("Task skeleton")}, {"planning", t("Planning")}, {"approval", t("Approval")}, {"max_members", t("Max members")}};
    return {{"when", t("When the goal has")}, {"template", t("Use template")}, {"approval", t("Approval")}};
}

const Teams::Proposal *proposalFor(const Teams::Catalog &catalog, const QString &kind, const QString &id)
{
    const Teams::Proposal *found = nullptr;
    for (const auto &p : catalog.proposals) if (p.kind == kind && p.target == id) found = &p;
    return found;
}

QJsonObject baseJson(const Teams::Catalog &catalog, const QString &kind, const QString &id)
{
    if (kind == "role") { if (const auto *r = catalog.role(id)) return r->json(); }
    else if (kind == "template") { if (const auto *t = catalog.templ(id)) return t->json(); }
    else for (const auto &r : catalog.routing) if (r.id == id) return r.json();
    return {};
}

// A field label with a warn dot when the field has an unsaved edit.
QLabel *fieldLabel(const QString &text, bool edited, const Colors &c)
{
    auto *l = new QLabel(edited ? QStringLiteral("%1 <span style='color:%2;'>●</span>").arg(text.toHtmlEscaped(), c.warn.name()) : text.toHtmlEscaped());
    l->setTextFormat(Qt::RichText); l->setFixedWidth(labelWidth); l->setAlignment(Qt::AlignRight | Qt::AlignTop);
    l->setStyleSheet(QStringLiteral("color:%1;font-size:12px;padding-top:5px;").arg(c.muted.name()));
    return l;
}

QLabel *mark(const QString &action, const Colors &c)
{
    const QString text = action == "add" ? QStringLiteral("NEW") : action == "remove" ? QStringLiteral("REMOVED") : QStringLiteral("CHANGED");
    const QColor color = action == "add" ? c.accent : action == "remove" ? c.danger : c.folder;
    auto *l = new QLabel(text); l->setObjectName("teamCatalogMark");
    l->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:3px;padding:0 4px;font-size:9px;font-weight:700;")
                         .arg(color.name(), alpha(color, .6).name(QColor::HexArgb)));
    return l;
}
}

TeamCatalogEditor::TeamCatalogEditor(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamCatalogEditor");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(10);
    // Header: title, meta and the actions of the mode.
    auto *header = new QHBoxLayout; header->setSpacing(8);
    m_title = new QLabel; m_title->setObjectName("teamCatalogTitle");
    m_meta = new QLabel; m_meta->setObjectName("teamCatalogStatus"); m_meta->setTextFormat(Qt::RichText);
    m_meta->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    header->addWidget(m_title); header->addWidget(m_meta, 1);
    m_openDesigner = button(tr("Open Team designer"), "teamCatalogAskDesigner"); m_openDesigner->hide();
    m_import = button(tr("Import…"), "teamCatalogImport"); m_import->setProperty("ghost", true);
    m_export = button(tr("Export…"), "teamCatalogExport"); m_export->setProperty("ghost", true);
    m_revert = button(tr("Revert"), "teamCatalogRevert");
    m_save = button(tr("Save catalog"), "teamCatalogSave", true);
    m_discard = button(tr("Discard draft"), "teamDesignerDiscard"); m_discard->setProperty("danger", true);
    m_apply = button(tr("Apply"), "teamDesignerApply", true);
    for (auto *b : {m_openDesigner, m_import, m_export, m_revert, m_save, m_discard, m_apply}) header->addWidget(b);
    outer->addLayout(header);
    // Banner: working designer, out-of-date draft, applied.
    m_banner = new QFrame; m_banner->setObjectName("teamCatalogBanner"); m_banner->hide();
    auto *bh = new QHBoxLayout(m_banner); bh->setContentsMargins(10, 6, 10, 6); bh->setSpacing(8);
    m_bannerText = new QLabel; m_bannerText->setWordWrap(true); m_bannerText->setObjectName("teamCatalogBannerText");
    m_bannerAction = button(QString(), "teamCatalogRedo"); m_bannerAction->hide();
    m_bannerDiscard = button(tr("Discard draft"), "teamCatalogBannerDiscard"); m_bannerDiscard->hide();
    bh->addWidget(m_bannerText, 1); bh->addWidget(m_bannerAction); bh->addWidget(m_bannerDiscard);
    outer->addWidget(m_banner);
    // Segments with the diff legend on the right.
    auto *tabsRow = new QHBoxLayout;
    m_tabs = segmented({{"roles", tr("Roles")}, {"templates", tr("Templates")}, {"routing", tr("Routing")}}, "roles", "teamCatalogTabs",
                       [this](const QString &v) { setTab(v == "roles" ? Tab::Roles : v == "templates" ? Tab::Templates : Tab::Routing); });
    m_legend = new QLabel; m_legend->setObjectName("teamCatalogLegend"); m_legend->setTextFormat(Qt::RichText);
    tabsRow->addWidget(m_tabs); tabsRow->addStretch(); tabsRow->addWidget(m_legend);
    outer->addLayout(tabsRow);
    // One scroll area for the whole body: the list column and the editor card.
    m_body = new QScrollArea; m_body->setObjectName("teamCatalogEditorArea"); m_body->setWidgetResizable(true); m_body->setFrameShape(QFrame::NoFrame);
    auto *content = new QWidget; content->setObjectName("teamCatalogBody");
    auto *ch = new QHBoxLayout(content); ch->setContentsMargins(0, 0, 4, 0); ch->setSpacing(12);
    m_left = new QWidget; m_left->setFixedWidth(listWidth);
    auto *lv = new QVBoxLayout(m_left); lv->setContentsMargins(0, 0, 0, 0); lv->setSpacing(4);
    const auto makeList = [this](const char *name) {
        auto *list = new QListWidget; list->setObjectName(name);
        list->setVerticalScrollBarPolicy(Qt::ScrollBarAlwaysOff); list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        list->setFrameShape(QFrame::NoFrame); list->setSpacing(1);
        connect(list, &QListWidget::currentRowChanged, this, [this] { rebuildEditor(); });
        return list;
    };
    m_roles = makeList("teamCatalogRoles"); m_templates = makeList("teamCatalogTemplates");
    m_add = button(tr("+ Role"), "teamCatalogAdd"); m_add->setProperty("ghost", true);
    lv->addWidget(m_roles); lv->addWidget(m_templates); lv->addWidget(m_add, 0, Qt::AlignLeft); lv->addStretch();
    connect(m_add, &QPushButton::clicked, this, [this] {
        if (m_tab == Tab::Roles) {
            Role r; r.name = tr("New role"); r.id = slug(r.name);
            for (int n = 2; m_catalog.role(r.id); ++n) r.id = slug(r.name) + QStringLiteral("-%1").arg(n);
            r.agent = "claude"; r.folder = "worktree"; r.canMessage = "lead"; r.color = swatches[m_catalog.roles.size() % swatches.size()];
            m_catalog.roles.append(r); changed(); rebuildLists(); m_roles->setCurrentRow(m_catalog.roles.size() - 1);
        } else {
            Template t; t.name = tr("New template"); t.id = slug(t.name);
            for (int n = 2; m_catalog.templ(t.id); ++n) t.id = slug(t.name) + QStringLiteral("-%1").arg(n);
            t.planning = "lead"; t.approval = "review";
            m_catalog.templates.append(t); changed(); rebuildLists(); m_templates->setCurrentRow(m_catalog.templates.size() - 1);
        }
    });
    auto *right = new QWidget; m_editorHost = new QVBoxLayout(right); m_editorHost->setContentsMargins(0, 0, 0, 0); m_editorHost->addStretch();
    ch->addWidget(m_left, 0, Qt::AlignTop); ch->addWidget(right, 1);
    m_body->setWidget(content);
    outer->addWidget(m_body, 1);

    connect(m_openDesigner, &QPushButton::clicked, this, [this] { if (askDesigner) askDesigner(); });
    connect(m_revert, &QPushButton::clicked, this, [this] {
        if (confirm && !confirm(QString(), tr("Revert catalog"), counted(unsavedChanges(), tr("Drop %1 unsaved change?"), tr("Drop %1 unsaved changes?")), tr("Revert"))) return;
        load();
    });
    connect(m_save, &QPushButton::clicked, this, [this] {
        m_save->setEnabled(false); m_meta->setText(tr("Saving…"));
        QJsonObject input{{"catalog", m_catalog.json()}, {"version", m_catalog.version}};
        m_teams->call(QStringLiteral("catalog set"), input, [this](bool ok, const QJsonObject &result, const QString &error) {
            if (!ok) { m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); m_save->setEnabled(true); return; }
            m_dirty = false;
            m_applied = tr("Saved as v%1").arg(result.value("version").toInteger());
            updateHeader();
        });
    });
    connect(m_discard, &QPushButton::clicked, this, [this] {
        if (confirm && !confirm(QString(), tr("Discard draft"), tr("Drop every proposal in the designer's draft? The catalog stays as it is."), tr("Discard draft"))) return;
        m_teams->call(QStringLiteral("catalog discard"), {}, [this](bool ok, const QJsonObject &, const QString &error) { if (!ok) m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); });
    });
    connect(m_bannerDiscard, &QPushButton::clicked, m_discard, &QPushButton::click);
    connect(m_apply, &QPushButton::clicked, this, [this] {
        int kept = 0; for (const auto &p : m_teams->catalog().proposals) kept += p.state == "kept";
        if (confirm && !confirm(QString(), tr("Apply changes"), counted(kept, tr("Write %1 kept change as the next catalog version?"), tr("Write %1 kept changes as the next catalog version?")) + QLatin1Char(' ') + tr("Undecided ones stay in the draft; running teams keep their roster."), tr("Apply"))) return;
        m_teams->call(QStringLiteral("catalog apply"), {}, [this](bool ok, const QJsonObject &result, const QString &error) {
            if (!ok) { m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); return; }
            m_applied = tr("Applied as v%1").arg(result.value("version").toInteger());
            updateHeader();
        });
    });
    connect(m_bannerAction, &QPushButton::clicked, this, [this] { if (askRedo) askRedo(m_teams->catalog().version); });
    connect(m_export, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getSaveFileName(this, tr("Export team catalog"), QStringLiteral("zerus-team-catalog.json"), tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly) || file.write(QJsonDocument(m_catalog.json()).toJson()) < 0)
            m_meta->setText(tr("⚠ Could not write %1").arg(path).toHtmlEscaped());
        else m_meta->setText(tr("Exported to %1").arg(path).toHtmlEscaped());
    });
    connect(m_import, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getOpenFileName(this, tr("Import team catalog"), {}, tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        QFile file(path);
        const auto doc = file.open(QIODevice::ReadOnly) ? QJsonDocument::fromJson(file.readAll()) : QJsonDocument();
        if (!doc.isObject()) { m_meta->setText(tr("⚠ %1 is not a team catalog").arg(path).toHtmlEscaped()); return; }
        const qint64 version = m_catalog.version;
        m_catalog = parseCatalog({{"catalog", doc.object()}});
        m_catalog.version = version;
        changed(); rebuildLists(); rebuildEditor();
    });
    connect(m_teams, &TeamController::catalogChanged, this, [this] { if (!m_dirty) load(); else updateHeader(); });
    setDark(true);
    if (m_teams->catalogReady()) load(); else m_teams->refreshCatalog();
}

void TeamCatalogEditor::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QLabel#teamCatalogTitle{color:%1;font-size:14px;font-weight:600;}"
        "QListWidget#teamCatalogRoles,QListWidget#teamCatalogTemplates{background:transparent;border:none;outline:none;}"
        "QListWidget#teamCatalogRoles::item,QListWidget#teamCatalogTemplates::item{border:1px solid transparent;border-radius:7px;}"
        "QListWidget#teamCatalogRoles::item:selected,QListWidget#teamCatalogTemplates::item:selected{background:%2;border-color:%3;}"
        "QListWidget#teamCatalogRoles::item:hover,QListWidget#teamCatalogTemplates::item:hover{background:%4;}"
        "QFrame#teamCatalogCard{background:%5;border:1px solid %6;border-radius:10px;}")
        .arg(c.fg.name(), alpha(c.accent, dark ? .10 : .08).name(QColor::HexArgb), alpha(c.accent, .45).name(QColor::HexArgb),
             alpha(c.fg, .04).name(QColor::HexArgb), c.surface.name(), c.border.name()));
    rebuildLists(); rebuildEditor(); updateHeader();
}

void TeamCatalogEditor::setDesignerButton(bool shown) { m_openDesigner->setVisible(shown); }

bool TeamCatalogEditor::draftMode() const { return m_showDraft && !m_teams->catalog().proposals.isEmpty() && !m_dirty; }

void TeamCatalogEditor::load()
{
    m_catalog = m_teams->catalog();
    m_catalog.proposals.clear();
    m_dirty = false;
    rebuildLists(); rebuildEditor(); updateHeader();
}

void TeamCatalogEditor::changed()
{
    m_dirty = true;
    m_applied.clear();
    rebuildLists();   // the edited item's dot
    updateHeader();
}

int TeamCatalogEditor::unsavedChanges() const
{
    if (!m_dirty) return 0;
    const auto &base = m_teams->catalog();
    int n = 0;
    const auto count = [&n](const QMap<QString, QJsonObject> &a, const QMap<QString, QJsonObject> &b) {
        for (auto it = a.cbegin(); it != a.cend(); ++it) n += b.value(it.key()) != it.value();
        for (auto it = b.cbegin(); it != b.cend(); ++it) n += !a.contains(it.key());
    };
    QMap<QString, QJsonObject> mine, theirs;
    for (const auto &r : m_catalog.roles) mine.insert("r" + r.id, r.json());
    for (const auto &r : base.roles) theirs.insert("r" + r.id, r.json());
    for (const auto &t : m_catalog.templates) mine.insert("t" + t.id, t.json());
    for (const auto &t : base.templates) theirs.insert("t" + t.id, t.json());
    for (const auto &r : m_catalog.routing) mine.insert("u" + r.id, r.json());
    for (const auto &r : base.routing) theirs.insert("u" + r.id, r.json());
    count(mine, theirs);
    return qMax(1, n);
}

void TeamCatalogEditor::updateHeader()
{
    const Colors c = colorsFor(m_dark);
    const auto &base = m_teams->catalog();
    const bool draft = draftMode();
    int kept = 0, undecided = 0, total = base.proposals.size();
    QMap<QString, int> perKind;
    for (const auto &p : base.proposals) { kept += p.state == "kept"; undecided += p.state == "proposed"; ++perKind[p.kind]; }
    const bool stale = draft && base.draftBase != base.version;
    m_title->setText(draft ? tr("Draft catalog") : tr("Team catalog"));
    const QString version = base.builtin ? tr("built-in") : QStringLiteral("v%1").arg(base.version);
    if (draft) m_meta->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.dim.name(),
        tr("based on v%1 · %2 of %3 kept · %4 undecided").arg(base.draftBase).arg(kept).arg(total).arg(undecided)));
    else if (m_dirty) m_meta->setText(QStringLiteral("<span style='color:%1;'>%2 · </span><span style='color:%3;'>%4</span>")
        .arg(c.dim.name(), version, c.warn.name(), counted(unsavedChanges(), tr("%1 unsaved change"), tr("%1 unsaved changes"))));
    else m_meta->setText(QStringLiteral("<span style='color:%1;'>%2</span>").arg(c.dim.name(), version));
    for (auto *b : {m_import, m_export, m_revert, m_save}) b->setVisible(!draft);
    m_revert->setEnabled(m_dirty); m_save->setEnabled(m_dirty);
    m_discard->setVisible(draft); m_apply->setVisible(draft);
    m_apply->setText(counted(kept, tr("Apply %1 change"), tr("Apply %1 changes")));
    m_apply->setEnabled(draft && kept > 0 && !stale);
    m_apply->setToolTip(stale ? tr("The catalog changed after this draft; applying would overwrite those edits.")
                              : undecided ? tr("Undecided changes stay in the draft") : QString());
    // Counters per tab while a draft shows.
    const QStringList kinds{"role", "template", "rule"};
    const QStringList names{tr("Roles"), tr("Templates"), tr("Routing")};
    const auto buttons = m_tabs->findChildren<QPushButton *>();
    for (int i = 0; i < buttons.size() && i < 3; ++i)
        buttons[i]->setText(draft && perKind.value(kinds[i]) ? QStringLiteral("%1  %2").arg(names[i]).arg(perKind.value(kinds[i])) : names[i]);
    m_legend->setVisible(draft);
    m_legend->setText(QStringLiteral("<span style='color:%1;'>NEW</span> &nbsp;<span style='color:%2;'>CHANGED</span> &nbsp;<span style='color:%3;'>REMOVED</span> &nbsp;"
                                     "<span style='color:%4;text-decoration:line-through;'>old</span> <span style='color:%5;'>new</span>")
                          .arg(c.accent.name(), c.folder.name(), c.danger.name(), c.danger.name(), c.accent.name()));
    // One banner at a time: out of date, working designer, then a result.
    QString text; QColor tone; bool redo = false;
    if (stale) {
        text = tr("Draft is out of date — Apply is off. The catalog changed to v%1 after this draft was made on v%2; applying would overwrite those edits.")
                   .arg(base.version).arg(base.draftBase);
        tone = c.warn; redo = true;
    } else if (!draft && m_designerWorking) {
        text = tr("The designer is working — proposals will appear here as a draft. Saving manual edits now makes that draft out of date.");
        tone = c.folder;
    } else if (!m_applied.isEmpty()) { text = m_applied; tone = c.accent; }
    m_banner->setVisible(!text.isEmpty());
    m_bannerText->setText(text);
    m_banner->setStyleSheet(QStringLiteral("QFrame#teamCatalogBanner{background:%1;border:1px solid %2;border-radius:8px;} QLabel{color:%3;}")
                                .arg(alpha(tone, .10).name(QColor::HexArgb), alpha(tone, .45).name(QColor::HexArgb), c.fg.name()));
    m_bannerAction->setVisible(redo && askRedo);
    m_bannerAction->setText(tr("Ask designer to redo on v%1").arg(base.version));
    m_bannerDiscard->setVisible(redo);
}

void TeamCatalogEditor::setTab(Tab tab)
{
    m_tab = tab;
    for (auto *b : m_tabs->findChildren<QPushButton *>()) {
        const auto v = b->property("segment").toString();
        b->setChecked((v == "roles" && tab == Tab::Roles) || (v == "templates" && tab == Tab::Templates) || (v == "routing" && tab == Tab::Routing));
    }
    rebuildLists(); rebuildEditor();
}

QWidget *TeamCatalogEditor::listRow(const QString &kind, const QString &id, const QString &name, const QColor &color, const QString &agent,
                                    const Proposal *proposal, bool dirty)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setProperty("catalogItem", id);
    auto *h = new QHBoxLayout(w); h->setContentsMargins(8, 4, 8, 4); h->setSpacing(6);
    if (kind == "role") h->addWidget(new Pill(name, color));
    else { auto *l = elided(name, c.fg, 12); h->addWidget(l, 1); }
    if (kind == "role") h->addStretch();
    if (proposal && proposal->state == "rejected") h->addWidget(label(tr("rejected"), c.dim, 10));
    else if (proposal) h->addWidget(mark(proposal->action, c));
    if (dirty) h->addWidget(label(QStringLiteral("●"), c.warn, 9));
    if (!agent.isEmpty()) {
        auto *tile = new QLabel; tile->setPixmap(workspaceIcon(agentIcon(agent), c.fg).pixmap(14, 14)); tile->setToolTip(agentNames.value(agent, agent));
        h->addWidget(tile);
    }
    if (proposal && proposal->state == "rejected") w->setStyleSheet(QStringLiteral("color:%1;").arg(c.dim.name()));
    Q_UNUSED(id);
    return w;
}

void TeamCatalogEditor::rebuildLists()
{
    const Colors c = colorsFor(m_dark);
    const bool draft = draftMode();
    const auto &base = m_teams->catalog();
    const auto fill = [&](QListWidget *list, const QString &kind) {
        const QString current = list->currentItem() ? list->currentItem()->data(Qt::UserRole).toString() : QString();
        const QSignalBlocker block(list);
        list->clear();
        QStringList ids;
        if (draft) {
            if (kind == "role") for (const auto &r : base.roles) ids << r.id;
            else for (const auto &t : base.templates) ids << t.id;
            for (const auto &p : base.proposals) if (p.kind == kind && p.action == "add" && !ids.contains(p.target)) ids << p.target;
        } else if (kind == "role") for (const auto &r : m_catalog.roles) ids << r.id;
        else for (const auto &t : m_catalog.templates) ids << t.id;
        for (const auto &id : ids) {
            const auto *p = draft ? proposalFor(base, kind, id) : nullptr;
            QJsonObject value = draft ? baseJson(base, kind, id) : baseJson(m_catalog, kind, id);
            if (p && p->action != "remove" && p->state != "rejected") value = p->value;
            else if (p && value.isEmpty()) value = p->value;
            const bool dirty = !draft && baseJson(base, kind, id) != baseJson(m_catalog, kind, id);
            const QString name = value.value("name").toString(id);
            const QColor color = value.value("color").toString().isEmpty() ? roleColor(name, m_dark) : QColor(value.value("color").toString());
            auto *item = new QListWidgetItem; item->setData(Qt::UserRole, id); item->setSizeHint(QSize(listWidth - 4, rowHeight - 2));
            list->addItem(item);
            list->setItemWidget(item, listRow(kind, id, name, color, value.value("agent").toString(), p, dirty));
        }
        int row = 0;
        for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(Qt::UserRole).toString() == current) row = i;
        list->setCurrentRow(list->count() ? row : -1);
        list->setFixedHeight(qMax(rowHeight, list->count() * rowHeight + 4));
    };
    fill(m_roles, "role"); fill(m_templates, "template");
    m_roles->setVisible(m_tab == Tab::Roles); m_templates->setVisible(m_tab == Tab::Templates);
    m_left->setVisible(m_tab != Tab::Routing);
    m_add->setText(m_tab == Tab::Roles ? tr("+ Role") : tr("+ Template"));
    m_add->setVisible(!draft);
    Q_UNUSED(c);
}

void TeamCatalogEditor::rebuildEditor()
{
    QWidget *editor = nullptr;
    const bool draft = draftMode();
    auto *list = m_tab == Tab::Roles ? m_roles : m_tab == Tab::Templates ? m_templates : nullptr;
    const QString id = list && list->currentItem() ? list->currentItem()->data(Qt::UserRole).toString() : QString();
    if (draft) editor = m_tab == Tab::Routing ? draftEditor("rule", {}) : id.isEmpty() ? nullptr : draftEditor(m_tab == Tab::Roles ? "role" : "template", id);
    else if (m_tab == Tab::Roles) { for (int i = 0; i < m_catalog.roles.size(); ++i) if (m_catalog.roles[i].id == id) editor = roleEditor(i); }
    else if (m_tab == Tab::Templates) { for (int i = 0; i < m_catalog.templates.size(); ++i) if (m_catalog.templates[i].id == id) editor = templateEditor(i); }
    else editor = routingEditor();
    if (!editor) editor = label(tr("Nothing selected"), colorsFor(m_dark).muted, 12);
    if (m_editorCard) { m_editorHost->removeWidget(m_editorCard); retire(m_editorCard); }   // a click inside it may be running
    m_editorCard = editor;
    m_editorHost->insertWidget(0, editor);
}

QWidget *TeamCatalogEditor::roleEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(14, 12, 14, 14); outer->setSpacing(10);
    auto *v = new QWidget; v->setObjectName("teamRoleEditor");
    Role &r = m_catalog.roles[index];
    const Role *orig = m_teams->catalog().role(r.id);
    const auto edited = [&](const auto &mine, const auto &theirs) { return !orig || mine != theirs; };
    const auto set = [this](const std::function<void()> &apply) { return [this, apply] { apply(); changed(); }; };
    // Header: badge, purpose, Duplicate, Delete.
    auto *head = row(8);
    head->addWidget(new Pill(r.name, QColor(r.color.isEmpty() ? roleColor(r.name, m_dark).name() : r.color)));
    head->addWidget(elided(r.prompt.section('\n', 0, 0), c.muted, 12), 1);
    auto *duplicate = button(tr("Duplicate"), "teamRoleDuplicate"); duplicate->setProperty("ghost", true);
    auto *remove = button(tr("Delete"), "teamRoleDelete"); remove->setProperty("danger", true);
    head->addWidget(duplicate); head->addWidget(remove);
    outer->addLayout(head);
    auto *grid = new QGridLayout(v); grid->setContentsMargins(0, 0, 0, 0); grid->setHorizontalSpacing(10); grid->setVerticalSpacing(8);
    grid->setColumnMinimumWidth(0, labelWidth); grid->setColumnStretch(1, 1);
    int at = 0;
    const auto add = [&](const QString &text, bool changedField, QWidget *field) { grid->addWidget(fieldLabel(text, changedField && orig, c), at, 0); grid->addWidget(field, at++, 1); };
    const auto box = [](QLayout *layout) { auto *b = new QWidget; layout->setContentsMargins(0, 0, 0, 0); b->setLayout(layout); return b; };
    auto *name = new QLineEdit(r.name); name->setObjectName("teamRoleName");
    connect(name, &QLineEdit::editingFinished, this, set([this, index, name] { m_catalog.roles[index].name = name->text().trimmed(); rebuildLists(); }));
    add(tr("Name"), edited(r.name, orig ? orig->name : QString()), name);
    auto *colors = row(4);
    for (const auto &swatch : swatches) {
        auto *b = new QToolButton; b->setFixedSize(18, 18); b->setAutoRaise(false); b->setCursor(Qt::PointingHandCursor);
        b->setStyleSheet(QStringLiteral("QToolButton{background:%1;border:%2;border-radius:4px;}").arg(swatch, swatch == r.color ? QStringLiteral("2px solid ") + c.fg.name() : QStringLiteral("none")));
        connect(b, &QToolButton::clicked, this, [this, index, swatch] { m_catalog.roles[index].color = swatch; changed(); rebuildLists(); rebuildEditor(); });
        colors->addWidget(b);
    }
    colors->addStretch();
    add(tr("Icon / color"), edited(r.color, orig ? orig->color : QString()), box(colors));
    // Specialization: tag chips with ×, and a field that adds one.
    auto *tags = row(4);
    for (int i = 0; i < r.tags.size(); ++i) {
        auto *chip = button(r.tags[i] + QStringLiteral("  ×"), "teamRoleTag"); chip->setProperty("chip", true);
        connect(chip, &QPushButton::clicked, this, [this, index, i] { m_catalog.roles[index].tags.removeAt(i); changed(); rebuildEditor(); });
        tags->addWidget(chip);
    }
    auto *skill = new QLineEdit; skill->setObjectName("teamRoleTags"); skill->setPlaceholderText(tr("+ skill")); skill->setFixedWidth(110);
    connect(skill, &QLineEdit::returnPressed, this, [this, index, skill] {
        const auto text = skill->text().trimmed();
        if (text.isEmpty() || m_catalog.roles[index].tags.contains(text)) return;
        m_catalog.roles[index].tags << text; changed(); rebuildEditor();
    });
    tags->addWidget(skill); tags->addStretch();
    add(tr("Specialization"), edited(r.tags, orig ? orig->tags : QStringList()), box(tags));
    auto *agentRow = row(6);
    QList<std::pair<QString, QString>> agentItems; for (const auto &a : agents) agentItems.append({a, agentNames.value(a)});
    auto *agent = choice(agentItems, r.agent, "teamRoleAgent");
    auto *account = new QComboBox; account->setObjectName("teamRoleAccount"); fillAccounts(account, r);
    m_accountChoice = account; m_accountRole = index;
    // Suggestions only: any model the agent's command line knows.
    auto *model = new QComboBox; model->setObjectName("teamRoleModel"); model->setEditable(true); model->setInsertPolicy(QComboBox::NoInsert);
    model->addItems(Teams::knownModels(r.agent)); model->setCurrentText(r.model); model->lineEdit()->setPlaceholderText(tr("default model"));
    model->setEnabled(r.agent != "dsh");
    auto *effort = choice({{"", "—"}, {"low", "low"}, {"medium", "medium"}, {"high", "high"}, {"xhigh", "xhigh"}}, r.effort, "teamRoleEffort");
    agentRow->addWidget(agent, 11); agentRow->addWidget(account, 10); agentRow->addWidget(model, 11); agentRow->addWidget(effort, 7);
    // Another agent has other accounts and models.
    connect(agent, &QComboBox::activated, this, [this, index, agent] {
        auto &role = m_catalog.roles[index]; const auto value = agent->currentData().toString(); if (value == role.agent) return;
        role.agent = value; role.account.clear(); if (!Teams::knownModels(value).contains(role.model)) role.model.clear();
        changed(); rebuildLists(); rebuildEditor();
    });
    connect(account, &QComboBox::activated, this, set([this, index, account] { m_catalog.roles[index].account = account->currentData().toString(); }));
    const auto modelChanged = set([this, index, model] { m_catalog.roles[index].model = model->currentText().trimmed(); });
    connect(model->lineEdit(), &QLineEdit::editingFinished, this, modelChanged);
    connect(model, &QComboBox::activated, this, modelChanged);
    connect(effort, &QComboBox::activated, this, set([this, index, effort] { m_catalog.roles[index].effort = effort->currentData().toString(); }));
    add(tr("Default agent"), orig && (r.agent != orig->agent || r.account != orig->account || r.model != orig->model || r.effort != orig->effort), box(agentRow));
    QList<std::pair<QString, QString>> computerItems{{QString(), tr("Any (this computer)")}};
    for (const auto &c : m_computers) if (!c.alias.isEmpty()) computerItems.append({c.alias, c.online ? c.label : tr("%1 (offline)").arg(c.label)});
    auto *computer = choice(computerItems, r.computer, "teamRoleComputer");
    // Accounts belong to a computer.
    connect(computer, &QComboBox::activated, this, [this, index, computer] {
        auto &role = m_catalog.roles[index]; const auto value = computer->currentData().toString(); if (value == role.computer) return;
        role.computer = value; role.account.clear(); changed(); rebuildEditor();
    });
    add(tr("Computer"), edited(r.computer, orig ? orig->computer : QString()), computer);
    auto *permissions = choice({{"", tr("Account's mode")}, {"auto", tr("Auto (workspace)")}, {"bypass", tr("Bypass")}}, r.permissions, "teamRolePermissions");
    connect(permissions, &QComboBox::activated, this, set([this, index, permissions] { m_catalog.roles[index].permissions = permissions->currentData().toString(); }));
    add(tr("Permissions"), edited(r.permissions, orig ? orig->permissions : QString()), permissions);
    auto *folder = row(0);
    folder->addWidget(segmented({{"worktree", tr("Own worktree")}, {"shared", tr("Shared folder")}, {"read_only", tr("Read only")}}, r.folder.isEmpty() ? QStringLiteral("worktree") : r.folder,
                                "teamRoleFolder", [this, index](const QString &value) { m_catalog.roles[index].folder = value; changed(); }));
    folder->addStretch();
    add(tr("Folder policy"), edited(r.folder, orig ? orig->folder : QString()), box(folder));
    auto *message = row(0);
    message->addWidget(segmented({{"lead", tr("Lead only")}, {"team", tr("Whole team")}}, r.canMessage.isEmpty() ? QStringLiteral("lead") : r.canMessage,
                                 "teamRoleMessage", [this, index](const QString &value) { m_catalog.roles[index].canMessage = value; changed(); }));
    message->addStretch();
    add(tr("Can message"), edited(r.canMessage, orig ? orig->canMessage : QString()), box(message));
    auto *dangerous = new QCheckBox(tr("May push / deploy / delete (only via tasks you approved)")); dangerous->setChecked(r.dangerous);
    connect(dangerous, &QCheckBox::toggled, this, [this, index](bool on) { m_catalog.roles[index].dangerous = on; changed(); });
    add(tr("Dangerous actions"), edited(r.dangerous, orig ? orig->dangerous : false), dangerous);
    auto *prompt = new QPlainTextEdit(r.prompt); prompt->setObjectName("teamRolePrompt"); prompt->setFixedHeight(84);
    add(tr("Role prompt"), edited(r.prompt, orig ? orig->prompt : QString()), prompt);
    // What a member reads, in order: project rules, then the role, then the task.
    auto *preview = new QLabel; preview->setObjectName("teamRolePreview"); preview->setWordWrap(true); preview->setTextFormat(Qt::RichText);
    preview->setStyleSheet(QStringLiteral("QLabel{background:%1;border:1px solid %2;border-radius:8px;padding:4px 8px;}").arg(c.card.name(), c.border.name()));
    const auto showPreview = [preview, c](const QString &role, const QString &text) {
        const auto block = [&](const QString &head, const QString &body) {
            return QStringLiteral("<p style='margin:4px 0;'><span style='color:%1;font-size:10px;'>%2</span><br><span style='color:%3;'>%4</span></p>")
                .arg(c.dim.name(), head.toHtmlEscaped(), c.fg.name(), body.toHtmlEscaped());
        };
        preview->setText(block(tr("1 · PROJECT RULES"), tr("The project's iron rules, when it has any")) +
                         block(tr("2 · ROLE · %1").arg(role.toUpper()), text.isEmpty() ? tr("(no role prompt)") : text.left(240)) +
                         block(tr("3 · TASK (PER ATTEMPT)"), tr("#N title — description, acceptance criteria, report command")));
    };
    showPreview(r.name, r.prompt);
    connect(prompt, &QPlainTextEdit::textChanged, this, [this, index, prompt, showPreview] {
        m_catalog.roles[index].prompt = prompt->toPlainText(); showPreview(m_catalog.roles[index].name, m_catalog.roles[index].prompt); changed();
    });
    add(tr("Prompt order"), false, preview);
    auto *used = row(4);
    for (const auto &t : m_catalog.templates) for (const auto &seat : t.roster) if (seat.role == r.id) {
        auto *chip = label(QStringLiteral("%1 × %2").arg(t.name).arg(seat.count), c.text2, 11);
        chip->setStyleSheet(QStringLiteral("color:%1;border:1px solid %2;border-radius:9px;padding:1px 7px;").arg(c.text2.name(), c.border.name()));
        used->addWidget(chip);
    }
    if (!used->count()) used->addWidget(label(tr("No template uses this role"), c.dim, 11));
    used->addStretch();
    add(tr("Used in"), false, box(used));
    outer->addWidget(v);
    connect(duplicate, &QPushButton::clicked, this, [this, index] {
        Role copy = m_catalog.roles[index]; copy.name += tr(" copy"); copy.id = slug(copy.name);
        for (int n = 2; m_catalog.role(copy.id); ++n) copy.id = slug(copy.name) + QStringLiteral("-%1").arg(n);
        m_catalog.roles.insert(index + 1, copy); changed(); rebuildLists(); m_roles->setCurrentRow(index + 1);
    });
    connect(remove, &QPushButton::clicked, this, [this, index] {
        for (const auto &t : m_catalog.templates) for (const auto &seat : t.roster) if (seat.role == m_catalog.roles[index].id) {
            QMessageBox::information(this, tr("Role in use"), tr("Template %1 uses this role; remove it there first.").arg(t.name));
            return;
        }
        if (confirm && !confirm(QString(), tr("Delete role"), tr("Delete the role %1 from the catalog? Running teams keep their members.").arg(m_catalog.roles[index].name), tr("Delete"))) return;
        m_catalog.roles.removeAt(index); changed(); rebuildLists(); rebuildEditor();
    });
    return w;
}

QWidget *TeamCatalogEditor::templateEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *outer = new QVBoxLayout(w); outer->setContentsMargins(14, 12, 14, 14); outer->setSpacing(10);
    auto *v = new QWidget; v->setObjectName("teamTemplateEditor");
    Template &t = m_catalog.templates[index];
    auto *head = row(8);
    head->addWidget(label(t.name, c.fg, 13, true), 1);
    auto *remove = button(tr("Delete"), "teamTemplateDelete"); remove->setProperty("danger", true);
    head->addWidget(remove);
    outer->addLayout(head);
    auto *grid = new QGridLayout(v); grid->setContentsMargins(0, 0, 0, 0); grid->setHorizontalSpacing(10); grid->setVerticalSpacing(8);
    grid->setColumnMinimumWidth(0, labelWidth); grid->setColumnStretch(1, 1);
    int at = 0;
    const auto add = [&](const QString &text, QWidget *field) { grid->addWidget(fieldLabel(text, false, c), at, 0); grid->addWidget(field, at++, 1); };
    const auto box = [](QLayout *layout) { auto *b = new QWidget; layout->setContentsMargins(0, 0, 0, 0); b->setLayout(layout); return b; };
    auto *name = new QLineEdit(t.name); name->setObjectName("teamTemplateName");
    connect(name, &QLineEdit::editingFinished, this, [this, index, name] { m_catalog.templates[index].name = name->text().trimmed(); changed(); rebuildLists(); });
    add(tr("Name"), name);
    auto *description = new QLineEdit(t.description); description->setPlaceholderText(tr("When to use this template"));
    connect(description, &QLineEdit::editingFinished, this, [this, index, description] { m_catalog.templates[index].description = description->text().trimmed(); changed(); });
    add(tr("Description"), description);
    auto *protocol = new QPlainTextEdit(t.leadProtocol); protocol->setFixedHeight(60); protocol->setPlaceholderText(tr("Extra instructions for the coordinator of this kind of team"));
    connect(protocol, &QPlainTextEdit::textChanged, this, [this, index, protocol] { m_catalog.templates[index].leadProtocol = protocol->toPlainText(); changed(); });
    add(tr("Lead protocol"), protocol);
    // Roster: a stepper per catalog role; 0 leaves it out, "only if needed" makes it 0–1.
    auto *roster = new QGridLayout; roster->setHorizontalSpacing(8); roster->setVerticalSpacing(4);
    int r = 0;
    for (const auto &role : m_catalog.roles) {
        if (role.id == "lead") continue;
        int seat = -1;
        for (int i = 0; i < t.roster.size(); ++i) if (t.roster[i].role == role.id) seat = i;
        roster->addWidget(new Pill(role.name, QColor(role.color.isEmpty() ? roleColor(role.name, m_dark).name() : role.color)), r, 0, Qt::AlignLeft);
        auto *count = new QSpinBox; count->setObjectName("teamTemplateSeatCount"); count->setRange(0, 8); count->setValue(seat >= 0 ? t.roster[seat].count : 0);
        auto *optional = new QCheckBox(tr("only if needed")); optional->setChecked(seat >= 0 && t.roster[seat].optional); optional->setEnabled(seat >= 0);
        roster->addWidget(count, r, 1); roster->addWidget(optional, r, 2);
        connect(count, &QSpinBox::valueChanged, this, [this, index, id = role.id, optional](int n) {
            auto &seats = m_catalog.templates[index].roster;
            int at = -1; for (int i = 0; i < seats.size(); ++i) if (seats[i].role == id) at = i;
            if (n == 0 && at >= 0) seats.removeAt(at);
            else if (n > 0 && at < 0) seats.append({id, n, false});
            else if (at >= 0) seats[at].count = n;
            optional->setEnabled(n > 0);
            changed();
        });
        connect(optional, &QCheckBox::toggled, this, [this, index, id = role.id](bool on) {
            for (auto &seat : m_catalog.templates[index].roster) if (seat.role == id) seat.optional = on;
            changed();
        });
        ++r;
    }
    roster->setColumnStretch(3, 1);
    add(tr("Roster"), box(roster));
    // Task skeleton: title, role, earlier tasks it depends on.
    auto *skeleton = new QWidget; auto *sv = new QVBoxLayout(skeleton); sv->setContentsMargins(0, 0, 0, 0); sv->setSpacing(4);
    QList<std::pair<QString, QString>> taskRoles{{"lead", tr("Lead")}};
    for (const auto &role : m_catalog.roles) if (role.id != "lead") taskRoles.append({role.id, role.name});
    for (int i = 0; i < t.tasks.size(); ++i) {
        auto *line = row(6);
        line->addWidget(label(QStringLiteral("#%1").arg(i + 1), c.dim, 11));
        auto *title = new QLineEdit(t.tasks[i].title);
        auto *role = choice(taskRoles, t.tasks[i].role, "teamTemplateTaskRole");
        QStringList depText; for (int d : t.tasks[i].deps) depText << QString::number(d + 1);
        auto *deps = new QLineEdit(depText.join(QStringLiteral(", "))); deps->setPlaceholderText(tr("after #")); deps->setFixedWidth(70);
        auto *del = new QToolButton; del->setIcon(workspaceIcon("trash", c.muted)); del->setAutoRaise(true); del->setToolTip(tr("Delete task"));
        connect(title, &QLineEdit::editingFinished, this, [this, index, i, title] { m_catalog.templates[index].tasks[i].title = title->text().trimmed(); changed(); });
        connect(role, &QComboBox::activated, this, [this, index, i, role] { m_catalog.templates[index].tasks[i].role = role->currentData().toString(); changed(); });
        connect(deps, &QLineEdit::editingFinished, this, [this, index, i, deps] {
            QList<int> list;
            for (const auto &part : deps->text().split(',')) { const int n = part.trimmed().remove('#').toInt(); if (n >= 1 && n <= i) list << n - 1; }
            m_catalog.templates[index].tasks[i].deps = list; changed();
        });
        connect(del, &QToolButton::clicked, this, [this, index, i] {
            auto &tasks = m_catalog.templates[index].tasks;
            tasks.removeAt(i);
            for (auto &k : tasks) { QList<int> kept; for (int d : k.deps) if (d != i) kept << (d > i ? d - 1 : d); k.deps = kept; }
            changed(); rebuildEditor();
        });
        line->addWidget(title, 1); line->addWidget(role); line->addWidget(deps); line->addWidget(del);
        sv->addLayout(line);
    }
    auto *addTask = button(tr("+ Task"), "teamTemplateAddTask"); addTask->setProperty("ghost", true);
    connect(addTask, &QPushButton::clicked, this, [this, index] {
        auto &tasks = m_catalog.templates[index].tasks;
        tasks.append({tr("New task"), QStringLiteral("lead"), tasks.isEmpty() ? QList<int>{} : QList<int>{int(tasks.size()) - 1}});
        changed(); rebuildEditor();
    });
    sv->addWidget(addTask, 0, Qt::AlignLeft);
    add(tr("Task skeleton"), skeleton);
    auto *planning = row(0);
    planning->addWidget(segmented({{"lead", tr("Lead plans")}, {"template", tr("Use template tasks")}}, t.planning.isEmpty() ? QStringLiteral("lead") : t.planning,
                                  "teamTemplatePlanning", [this, index](const QString &value) { m_catalog.templates[index].planning = value; changed(); }));
    planning->addStretch();
    add(tr("Planning"), box(planning));
    auto *approval = row(0);
    approval->addWidget(segmented({{"review", tr("Review plan first")}, {"auto", tr("Run when the plan is valid")}}, t.approval.isEmpty() ? QStringLiteral("review") : t.approval,
                                  "teamTemplateApproval", [this, index](const QString &value) { m_catalog.templates[index].approval = value; changed(); }));
    approval->addStretch();
    add(tr("Approval"), box(approval));
    auto *max = new QSpinBox; max->setRange(1, 32); max->setValue(t.maxMembers > 0 ? t.maxMembers : 6); max->setFixedWidth(80);
    connect(max, &QSpinBox::valueChanged, this, [this, index](int n) { m_catalog.templates[index].maxMembers = n; changed(); });
    add(tr("Max members"), max);
    outer->addWidget(v);
    connect(remove, &QPushButton::clicked, this, [this, index] {
        if (confirm && !confirm(QString(), tr("Delete template"), tr("Delete the template %1 and its routing rules?").arg(m_catalog.templates[index].name), tr("Delete"))) return;
        const QString id = m_catalog.templates[index].id;
        m_catalog.templates.removeAt(index);
        m_catalog.routing.erase(std::remove_if(m_catalog.routing.begin(), m_catalog.routing.end(), [&](const Rule &rule) { return rule.templateId == id; }), m_catalog.routing.end());
        changed(); rebuildLists(); rebuildEditor();
    });
    return w;
}

QWidget *TeamCatalogEditor::routingEditor()
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(14, 12, 14, 14); v->setSpacing(8);
    auto *inner = new QWidget; inner->setObjectName("teamRoutingEditor");
    auto *iv = new QVBoxLayout(inner); iv->setContentsMargins(0, 0, 0, 0); iv->setSpacing(6);
    auto *intro = label(tr("Rules run in order; the first whose words all appear in the goal picks the template. Otherwise the first template."), c.muted, 12);
    intro->setWordWrap(true); iv->addWidget(intro);
    QList<std::pair<QString, QString>> templates;
    for (const auto &t : m_catalog.templates) templates.append({t.id, t.name});
    for (int i = 0; i < m_catalog.routing.size(); ++i) {
        auto *r = row(6);
        r->addWidget(label(QStringLiteral("%1").arg(i + 1), c.dim, 11));
        r->addWidget(label(tr("When the goal has"), c.muted, 12));
        auto *when = new QLineEdit(m_catalog.routing[i].when); when->setObjectName("teamRuleWhen");
        auto *templ = choice(templates, m_catalog.routing[i].templateId, "teamRuleTemplate");
        auto *approval = choice({{"review", tr("review")}, {"auto", tr("auto")}}, m_catalog.routing[i].approval.isEmpty() ? QStringLiteral("review") : m_catalog.routing[i].approval, "teamRuleApproval");
        auto *up = new QToolButton; up->setText(QStringLiteral("↑")); up->setAutoRaise(true); up->setEnabled(i > 0); up->setToolTip(tr("Move up"));
        auto *del = new QToolButton; del->setIcon(workspaceIcon("trash", c.muted)); del->setAutoRaise(true); del->setToolTip(tr("Delete rule"));
        connect(when, &QLineEdit::editingFinished, this, [this, i, when] { m_catalog.routing[i].when = when->text().trimmed(); changed(); });
        connect(templ, &QComboBox::activated, this, [this, i, templ] { m_catalog.routing[i].templateId = templ->currentData().toString(); changed(); });
        connect(approval, &QComboBox::activated, this, [this, i, approval] { m_catalog.routing[i].approval = approval->currentData().toString(); changed(); });
        connect(up, &QToolButton::clicked, this, [this, i] { m_catalog.routing.swapItemsAt(i, i - 1); changed(); rebuildEditor(); });
        connect(del, &QToolButton::clicked, this, [this, i] { m_catalog.routing.removeAt(i); changed(); rebuildEditor(); });
        r->addWidget(when, 1); r->addWidget(label(QStringLiteral("→"), c.muted, 12)); r->addWidget(templ); r->addWidget(approval); r->addWidget(up); r->addWidget(del);
        iv->addLayout(r);
    }
    auto *add = button(tr("+ Rule"), "teamRoutingAdd"); add->setProperty("ghost", true); add->setEnabled(!templates.isEmpty());
    connect(add, &QPushButton::clicked, this, [this] {
        Rule r; r.id = QStringLiteral("rule-%1").arg(m_catalog.routing.size() + 1);
        while (std::any_of(m_catalog.routing.cbegin(), m_catalog.routing.cend(), [&](const Rule &x) { return x.id == r.id; })) r.id += 'x';
        r.templateId = m_catalog.templates.first().id; r.approval = "review";
        m_catalog.routing.append(r); changed(); rebuildEditor();
    });
    iv->addWidget(add, 0, Qt::AlignLeft);
    // Try it: which template a goal would get.
    auto *tryBox = new QFrame; tryBox->setStyleSheet(QStringLiteral("QFrame{background:%1;border:1px solid %2;border-radius:8px;}").arg(c.card.name(), c.border.name()));
    auto *tv = new QVBoxLayout(tryBox); tv->setContentsMargins(10, 8, 10, 8); tv->setSpacing(4);
    tv->addWidget(label(tr("Try it"), c.fg, 12, true));
    auto *goal = new QLineEdit; goal->setObjectName("teamRoutingTry"); goal->setPlaceholderText(tr("Type a goal, e.g. fix the checkout bug"));
    auto *result = label(QString(), c.text2, 12); result->setObjectName("teamRoutingResult"); result->setWordWrap(true);
    connect(goal, &QLineEdit::textChanged, this, [this, result](const QString &text) {
        const auto lower = text.toLower();
        for (const auto &rule : m_catalog.routing) {
            const auto words = rule.when.toLower().split(' ', Qt::SkipEmptyParts);
            if (!words.isEmpty() && std::all_of(words.cbegin(), words.cend(), [&](const QString &word) { return lower.contains(word); }))
                if (const auto *t = m_catalog.templ(rule.templateId)) { result->setText(tr("→ %1 (rule “%2”, %3)").arg(t->name, rule.when, rule.approval)); return; }
        }
        result->setText(m_catalog.templates.isEmpty() ? tr("No templates") : tr("→ %1 (no rule matched)").arg(m_catalog.templates.first().name));
    });
    tv->addWidget(goal); tv->addWidget(result);
    iv->addWidget(tryBox);
    v->addWidget(inner);
    return w;
}

QWidget *TeamCatalogEditor::draftEditor(const QString &kind, const QString &id)
{
    const Colors c = colorsFor(m_dark);
    const auto &base = m_teams->catalog();
    auto *w = new QFrame; w->setObjectName("teamCatalogCard");
    auto *v = new QVBoxLayout(w); v->setContentsMargins(14, 12, 14, 14); v->setSpacing(10);
    const auto decide = [this](const QString &proposal, const QString &decision) {
        m_teams->call(QStringLiteral("catalog decide"), {{"proposal", proposal}, {"decision", decision}},
                      [this](bool ok, const QJsonObject &, const QString &error) { if (!ok) m_meta->setText(QStringLiteral("⚠ ") + error.toHtmlEscaped()); });
    };
    // One item, or every rule for Routing.
    QStringList ids;
    if (kind == "rule") {
        for (const auto &r : base.routing) ids << r.id;
        for (const auto &p : base.proposals) if (p.kind == "rule" && p.action == "add" && !ids.contains(p.target)) ids << p.target;
    } else ids << id;
    for (const auto &item : ids) {
        const auto *p = proposalFor(base, kind, item);
        const QJsonObject old = baseJson(base, kind, item);
        const QJsonObject next = p && p->action != "remove" ? p->value : old;
        const bool rejected = p && p->state == "rejected";
        auto *head = row(8);
        const QString name = next.value("name").toString(next.value("when").toString(item));
        if (kind == "role") head->addWidget(new Pill(name, next.value("color").toString().isEmpty() ? roleColor(name, m_dark) : QColor(next.value("color").toString())));
        else head->addWidget(label(name, c.fg, 13, true));
        if (p) head->addWidget(rejected ? label(tr("rejected"), c.dim, 10) : mark(p->action, c));
        head->addStretch();
        if (p) {
            if (p->state == "proposed") {
                auto *reject = button(tr("Reject"), "teamProposalReject"); reject->setProperty("ghost", true);
                auto *keep = button(tr("Keep"), "teamProposalKeep");
                connect(reject, &QPushButton::clicked, this, [decide, pid = p->id] { decide(pid, "reject"); });
                connect(keep, &QPushButton::clicked, this, [decide, pid = p->id] { decide(pid, "keep"); });
                head->addWidget(reject); head->addWidget(keep);
            } else {
                head->addWidget(label(p->state == "kept" ? tr("Kept") : tr("Rejected"), p->state == "kept" ? c.accent : c.danger, 11));
                auto *undo = button(tr("Undo"), "teamProposalUndo"); undo->setProperty("ghost", true);
                connect(undo, &QPushButton::clicked, this, [decide, pid = p->id] { decide(pid, "undo"); });
                head->addWidget(undo);
            }
        }
        v->addLayout(head);
        if (p && !p->reason.isEmpty()) { auto *reason = label(p->reason, c.muted, 12); reason->setWordWrap(true); v->addWidget(reason); }
        auto *grid = new QGridLayout; grid->setHorizontalSpacing(10); grid->setVerticalSpacing(6); grid->setColumnStretch(1, 1);
        int at = 0;
        for (const auto &[key, text] : fieldsOf(kind)) {
            const QString before = shown(key, old.value(key)), after = shown(key, next.value(key));
            if (before.isEmpty() && after.isEmpty()) continue;
            grid->addWidget(fieldLabel(text, false, c), at, 0);
            auto *values = new QVBoxLayout; values->setSpacing(2);
            const bool removed = p && p->action == "remove";
            const bool differs = p && before != after;
            if ((differs || removed) && !before.isEmpty()) {
                auto *o = label(before, rejected ? c.dim : c.text2, 12); QFont of = o->font(); of.setStrikeOut(!rejected); o->setFont(of); o->setWordWrap(true);
                if (!rejected) o->setStyleSheet(QStringLiteral("background:%1;padding:1px 4px;border-radius:3px;").arg(alpha(c.danger, .14).name(QColor::HexArgb)));
                values->addWidget(o);
            }
            if (!removed && (differs || !p || after == before)) {
                auto *n = label(after, rejected ? c.dim : c.fg, 12); n->setWordWrap(true);
                if (differs && !rejected) n->setStyleSheet(QStringLiteral("background:%1;padding:1px 4px;border-radius:3px;").arg(alpha(c.accent, .14).name(QColor::HexArgb)));
                if (rejected) { QFont nf = n->font(); nf.setStrikeOut(true); n->setFont(nf); }
                values->addWidget(n);
            }
            auto *holder = new QWidget; values->setContentsMargins(0, 0, 0, 0); holder->setLayout(values);
            grid->addWidget(holder, at++, 1);
        }
        v->addLayout(grid);
    }
    if (ids.isEmpty()) v->addWidget(label(tr("No routing rules"), c.muted, 12));
    return w;
}

void TeamCatalogEditor::showProposal(const QString &proposal)
{
    for (const auto &p : m_teams->catalog().proposals) if (p.id == proposal) {
        setTab(p.kind == "role" ? Tab::Roles : p.kind == "template" ? Tab::Templates : Tab::Routing);
        auto *list = p.kind == "role" ? m_roles : m_templates;
        if (p.kind != "rule") for (int i = 0; i < list->count(); ++i) if (list->item(i)->data(Qt::UserRole).toString() == p.target) list->setCurrentRow(i);
        return;
    }
}

void TeamCatalogEditor::applyKept()
{
    if (m_apply->isEnabled()) m_apply->click();
}

void TeamCatalogEditor::setSources(HgsClient *client, const QList<Computer> &computers)
{
    m_computers = computers;
    if (client && client != m_client) {
        m_client = client;
        connect(m_client, &HgsClient::accountsReady, this, [this](quint64, const QString &host, const QJsonObject &result) {
            if (host != QString() && std::none_of(m_computers.cbegin(), m_computers.cend(), [&](const Computer &c) { return c.alias == host; })) return;
            QList<Account> list;
            for (const auto &value : result.value("profiles").toArray()) {
                const auto p = value.toObject();
                list.append({p.value("id").toString(), p.value("provider").toString(), p.value("label").toString(),
                             p.value("account_status").toObject().value("status").toString() != "signed_out"});
            }
            m_accounts.insert(host, list);
            if (m_accountChoice && m_accountRole >= 0 && m_accountRole < m_catalog.roles.size()) fillAccounts(m_accountChoice, m_catalog.roles[m_accountRole]);
        });
    }
    if (!m_client) return;
    m_client->requestAccounts({});
    for (const auto &c : m_computers) if (!c.alias.isEmpty() && c.online) m_client->requestAccounts(c.alias);
}

void TeamCatalogEditor::fillAccounts(QComboBox *box, const Teams::Role &role) const
{
    const QSignalBlocker blocker(box);
    box->clear(); box->addItem(tr("Default account"), QString());
    const auto agent = role.agent.isEmpty() ? QStringLiteral("claude") : role.agent;
    for (const auto &a : m_accounts.value(role.computer)) if (a.provider == agent)
        box->addItem((a.label.isEmpty() ? a.id : a.label) + (a.signedIn ? QString() : tr(" (signed out)")), a.id);
    // An account this computer did not report (yet) stays as written.
    if (!role.account.isEmpty() && box->findData(role.account) < 0) box->addItem(role.account, role.account);
    box->setCurrentIndex(qMax(0, box->findData(role.account)));
}
