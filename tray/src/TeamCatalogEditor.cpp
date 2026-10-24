#include "TeamCatalogEditor.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QButtonGroup>
#include <QCheckBox>
#include <QComboBox>
#include <QFile>
#include <QFileDialog>
#include <QFormLayout>
#include <QJsonDocument>
#include <QLineEdit>
#include <QListWidget>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QSpinBox>
#include <QTabWidget>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
const QStringList swatches{"#b79cff", "#8e9bff", "#6fa8ff", "#f08bb4", "#56c8e0", "#d9b38c", "#e48de0", "#a3b1c6", "#b9d36b"};

QComboBox *choice(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name)
{
    auto *box = new QComboBox; box->setObjectName(name);
    for (const auto &[value, text] : items) box->addItem(text, value);
    if (box->findData(current) < 0 && !current.isEmpty()) box->addItem(current, current);
    box->setCurrentIndex(qMax(0, box->findData(current)));
    return box;
}

// A segmented control of checkable buttons.
QWidget *segments(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name, const std::function<void(const QString &)> &picked)
{
    auto *box = new QWidget; box->setObjectName(name);
    auto *h = new QHBoxLayout(box); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(0);
    auto *group = new QButtonGroup(box);
    for (const auto &[value, text] : items) {
        auto *b = new QPushButton(text); b->setCheckable(true); b->setChecked(value == current); b->setAutoDefault(false);
        b->setProperty("segment", value);
        group->addButton(b); h->addWidget(b);
        QObject::connect(b, &QPushButton::clicked, box, [picked, value = value] { picked(value); });
    }
    h->addStretch();
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
}

TeamCatalogEditor::TeamCatalogEditor(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamCatalogEditor");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(10);
    auto *heading = new QLabel(tr("Teams")); heading->setObjectName("heading"); outer->addWidget(heading);
    auto *intro = new QLabel(tr("Roles, templates and routing rules that New team and the Team designer use. Running teams keep the roster they started with."));
    intro->setWordWrap(true); outer->addWidget(intro);
    auto *bar = new QHBoxLayout;
    m_status = new QLabel; m_status->setObjectName("teamCatalogStatus");
    bar->addWidget(m_status, 1);
    auto *ask = new QPushButton(tr("Ask Team designer")); ask->setObjectName("teamCatalogAskDesigner"); ask->setAutoDefault(false);
    connect(ask, &QPushButton::clicked, this, [this] { if (askDesigner) askDesigner(); });
    auto *import = new QPushButton(tr("Import…")); import->setObjectName("teamCatalogImport"); import->setAutoDefault(false);
    auto *exportButton = new QPushButton(tr("Export…")); exportButton->setObjectName("teamCatalogExport"); exportButton->setAutoDefault(false);
    m_revert = new QPushButton(tr("Revert")); m_revert->setObjectName("teamCatalogRevert"); m_revert->setAutoDefault(false);
    m_save = new QPushButton(tr("Save catalog")); m_save->setObjectName("teamCatalogSave"); m_save->setAutoDefault(false);
    for (auto *b : {ask, import, exportButton, m_revert, m_save}) bar->addWidget(b);
    outer->addLayout(bar);
    m_tabs = new QTabWidget; m_tabs->setObjectName("teamCatalogTabs");
    const auto listPage = [this](QListWidget *&list, const QString &add, const char *name, const std::function<void()> &adder) {
        auto *page = new QWidget; auto *h = new QHBoxLayout(page); h->setContentsMargins(0, 8, 0, 0);
        auto *left = new QVBoxLayout;
        list = new QListWidget; list->setObjectName(name); list->setFixedWidth(220);
        auto *addButton = new QPushButton(add); addButton->setAutoDefault(false);
        connect(addButton, &QPushButton::clicked, this, adder);
        left->addWidget(addButton); left->addWidget(list, 1);
        h->addLayout(left);
        connect(list, &QListWidget::currentRowChanged, this, [this] { rebuildEditor(); });
        return page;
    };
    m_tabs->addTab(listPage(m_roles, tr("+ Role"), "teamCatalogRoles", [this] {
        Role r; r.name = tr("New role"); r.id = slug(r.name);
        for (int n = 2; m_catalog.role(r.id); ++n) r.id = slug(r.name) + QStringLiteral("-%1").arg(n);
        r.agent = "claude"; r.folder = "worktree"; r.canMessage = "lead"; r.color = swatches[m_catalog.roles.size() % swatches.size()];
        m_catalog.roles.append(r); changed(); rebuildLists(); m_roles->setCurrentRow(m_catalog.roles.size() - 1);
    }), tr("Roles"));
    m_tabs->addTab(listPage(m_templates, tr("+ Template"), "teamCatalogTemplates", [this] {
        Template t; t.name = tr("New template"); t.id = slug(t.name);
        for (int n = 2; m_catalog.templ(t.id); ++n) t.id = slug(t.name) + QStringLiteral("-%1").arg(n);
        t.planning = "lead"; t.approval = "review";
        m_catalog.templates.append(t); changed(); rebuildLists(); m_templates->setCurrentRow(m_catalog.templates.size() - 1);
    }), tr("Templates"));
    m_tabs->addTab(new QWidget, tr("Routing"));
    connect(m_tabs, &QTabWidget::currentChanged, this, [this] { rebuildEditor(); });
    outer->addWidget(m_tabs, 1);
    m_editor = new QScrollArea; m_editor->setObjectName("teamCatalogEditorArea"); m_editor->setWidgetResizable(true); m_editor->setFrameShape(QFrame::NoFrame);
    outer->addWidget(m_editor, 3);
    connect(m_revert, &QPushButton::clicked, this, [this] { load(); });
    connect(m_save, &QPushButton::clicked, this, [this] {
        m_save->setEnabled(false); m_status->setText(tr("Saving…"));
        QJsonObject input{{"catalog", m_catalog.json()}, {"version", m_catalog.version}};
        m_teams->call(QStringLiteral("catalog set"), input, [this](bool ok, const QJsonObject &result, const QString &error) {
            if (!ok) { m_status->setText(QStringLiteral("⚠ ") + error); m_save->setEnabled(true); return; }
            m_dirty = false;
            m_status->setText(tr("Saved as version %1").arg(result.value("version").toInteger()));
        });
    });
    connect(exportButton, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getSaveFileName(this, tr("Export team catalog"), QStringLiteral("zerus-team-catalog.json"), tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        QFile file(path);
        if (!file.open(QIODevice::WriteOnly) || file.write(QJsonDocument(m_catalog.json()).toJson()) < 0)
            m_status->setText(tr("⚠ Could not write %1").arg(path));
        else m_status->setText(tr("Exported to %1").arg(path));
    });
    connect(import, &QPushButton::clicked, this, [this] {
        const auto path = QFileDialog::getOpenFileName(this, tr("Import team catalog"), {}, tr("JSON (*.json)"));
        if (path.isEmpty()) return;
        QFile file(path);
        const auto doc = file.open(QIODevice::ReadOnly) ? QJsonDocument::fromJson(file.readAll()) : QJsonDocument();
        if (!doc.isObject()) { m_status->setText(tr("⚠ %1 is not a team catalog").arg(path)); return; }
        const qint64 version = m_catalog.version;
        m_catalog = parseCatalog({{"catalog", doc.object()}});
        m_catalog.version = version;
        changed(); rebuildLists(); rebuildEditor();
        m_status->setText(tr("Imported; check and Save to apply"));
    });
    connect(m_teams, &TeamController::catalogChanged, this, [this] { if (!m_dirty) load(); });
    if (m_teams->catalogReady()) load(); else m_teams->refreshCatalog();
    m_save->setEnabled(false); m_revert->setEnabled(false);
}

void TeamCatalogEditor::load()
{
    m_catalog = m_teams->catalog();
    m_catalog.proposals.clear();
    m_dirty = false;
    m_save->setEnabled(false); m_revert->setEnabled(false);
    m_status->setText(m_catalog.builtin ? tr("Built-in catalog") : tr("Version %1").arg(m_catalog.version));
    rebuildLists();
    rebuildEditor();
}

void TeamCatalogEditor::changed()
{
    m_dirty = true;
    m_save->setEnabled(true); m_revert->setEnabled(true);
    m_status->setText(tr("Unsaved changes"));
}

void TeamCatalogEditor::rebuildLists()
{
    const int role = m_roles->currentRow(), templ = m_templates->currentRow();
    {
        const QSignalBlocker a(m_roles), b(m_templates);
        m_roles->clear(); m_templates->clear();
        for (const auto &r : m_catalog.roles) {
            auto *item = new QListWidgetItem(QStringLiteral("● %1").arg(r.name));
            item->setForeground(QColor(r.color.isEmpty() ? roleColor(r.name, m_dark) : QColor(r.color)));
            item->setToolTip(r.agent);
            m_roles->addItem(item);
        }
        for (const auto &t : m_catalog.templates) m_templates->addItem(t.name);
        m_roles->setCurrentRow(qBound(0, role, m_roles->count() - 1));
        m_templates->setCurrentRow(qBound(0, templ, m_templates->count() - 1));
    }
}

void TeamCatalogEditor::rebuildEditor()
{
    QWidget *editor = nullptr;
    const int tab = m_tabs->currentIndex();
    if (tab == 0 && m_roles->currentRow() >= 0 && m_roles->currentRow() < m_catalog.roles.size()) editor = roleEditor(m_roles->currentRow());
    else if (tab == 1 && m_templates->currentRow() >= 0 && m_templates->currentRow() < m_catalog.templates.size()) editor = templateEditor(m_templates->currentRow());
    else if (tab == 2) editor = routingEditor();
    if (!editor) editor = new QLabel(tr("Nothing selected"));
    retire(m_editor->takeWidget());   // a click inside it may be running
    m_editor->setWidget(editor);
}

QWidget *TeamCatalogEditor::roleEditor(int index)
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamRoleEditor");
    auto *form = new QFormLayout(w); form->setLabelAlignment(Qt::AlignRight);
    Role &r = m_catalog.roles[index];
    const auto set = [this](const std::function<void()> &apply) { return [this, apply] { apply(); changed(); }; };
    auto *name = new QLineEdit(r.name); name->setObjectName("teamRoleName");
    connect(name, &QLineEdit::editingFinished, this, set([this, index, name] { m_catalog.roles[index].name = name->text().trimmed(); rebuildLists(); }));
    form->addRow(tr("Name"), name);
    auto *colors = new QWidget; auto *ch = new QHBoxLayout(colors); ch->setContentsMargins(0, 0, 0, 0); ch->setSpacing(4);
    ch->addWidget(new Pill(r.name, QColor(r.color.isEmpty() ? roleColor(r.name, m_dark).name() : r.color)));
    for (const auto &swatch : swatches) {
        auto *b = new QToolButton; b->setFixedSize(18, 18); b->setAutoRaise(false);
        b->setStyleSheet(QStringLiteral("QToolButton{background:%1;border:%2;border-radius:4px;}").arg(swatch, swatch == r.color ? QStringLiteral("2px solid ") + c.fg.name() : QStringLiteral("none")));
        connect(b, &QToolButton::clicked, this, [this, index, swatch] { m_catalog.roles[index].color = swatch; changed(); rebuildLists(); rebuildEditor(); });
        ch->addWidget(b);
    }
    ch->addStretch();
    form->addRow(tr("Icon / color"), colors);
    auto *prompt = new QPlainTextEdit(r.prompt); prompt->setObjectName("teamRolePrompt"); prompt->setFixedHeight(90);
    form->addRow(tr("Role prompt"), prompt);
    // What a member reads, in order: project rules, then the role, then the task.
    auto *preview = new QLabel; preview->setObjectName("teamRolePreview"); preview->setWordWrap(true); preview->setTextFormat(Qt::RichText);
    const auto showPreview = [preview, c](const QString &role, const QString &text) {
        const auto block = [&](const QString &head, const QString &body) {
            return QStringLiteral("<p style='margin:4px 0;'><span style='color:%1;font-size:10px;'>%2</span><br>%3</p>").arg(c.dim.name(), head.toHtmlEscaped(), body.toHtmlEscaped());
        };
        preview->setText(block(tr("1 · PROJECT RULES"), tr("The project's iron rules, when it has any")) +
                         block(tr("2 · ROLE · %1").arg(role.toUpper()), text.isEmpty() ? tr("(no role prompt)") : text.left(240)) +
                         block(tr("3 · TASK (FILLED PER ATTEMPT)"), tr("#N title — description, acceptance criteria, report command")));
    };
    showPreview(r.name, r.prompt);
    connect(prompt, &QPlainTextEdit::textChanged, this, [this, index, prompt, showPreview] {
        m_catalog.roles[index].prompt = prompt->toPlainText(); showPreview(m_catalog.roles[index].name, m_catalog.roles[index].prompt); changed();
    });
    form->addRow(tr("Preview"), preview);
    auto *agentRow = new QWidget; auto *ah = new QHBoxLayout(agentRow); ah->setContentsMargins(0, 0, 0, 0); ah->setSpacing(4);
    QList<std::pair<QString, QString>> agentItems; for (const auto &a : agents) agentItems.append({a, a});
    auto *agent = choice(agentItems, r.agent, "teamRoleAgent");
    auto *account = new QLineEdit(r.account); account->setPlaceholderText(tr("default account")); account->setObjectName("teamRoleAccount");
    auto *model = new QLineEdit(r.model); model->setPlaceholderText(tr("default model")); model->setObjectName("teamRoleModel");
    auto *effort = choice({{"", "—"}, {"low", "low"}, {"medium", "medium"}, {"high", "high"}, {"xhigh", "xhigh"}}, r.effort, "teamRoleEffort");
    ah->addWidget(agent); ah->addWidget(account, 1); ah->addWidget(model, 1); ah->addWidget(effort);
    connect(agent, &QComboBox::activated, this, set([this, index, agent] { m_catalog.roles[index].agent = agent->currentData().toString(); }));
    connect(account, &QLineEdit::editingFinished, this, set([this, index, account] { m_catalog.roles[index].account = account->text().trimmed(); }));
    connect(model, &QLineEdit::editingFinished, this, set([this, index, model] { m_catalog.roles[index].model = model->text().trimmed(); }));
    connect(effort, &QComboBox::activated, this, set([this, index, effort] { m_catalog.roles[index].effort = effort->currentData().toString(); }));
    form->addRow(tr("Default agent"), agentRow);
    auto *permissions = choice({{"", tr("Account's mode")}, {"auto", tr("Auto")}, {"bypass", tr("Bypass")}}, r.permissions, "teamRolePermissions");
    connect(permissions, &QComboBox::activated, this, set([this, index, permissions] { m_catalog.roles[index].permissions = permissions->currentData().toString(); }));
    auto *permissionBox = new QWidget; auto *pv = new QVBoxLayout(permissionBox); pv->setContentsMargins(0, 0, 0, 0);
    pv->addWidget(permissions);
    auto *hint = label(tr("Allow-list hgs team … for this role so reports don't ask for approval (Claude allowlist / Codex rules)."), c.muted, 11);
    hint->setWordWrap(true); pv->addWidget(hint);
    form->addRow(tr("Permissions"), permissionBox);
    auto *computer = new QLineEdit(r.computer); computer->setPlaceholderText(tr("this computer")); computer->setObjectName("teamRoleComputer");
    connect(computer, &QLineEdit::editingFinished, this, set([this, index, computer] { m_catalog.roles[index].computer = computer->text().trimmed(); }));
    form->addRow(tr("Computer"), computer);
    form->addRow(tr("Folder policy"), segments({{"worktree", tr("Own worktree")}, {"shared", tr("Shared folder")}, {"read_only", tr("Read only")}}, r.folder.isEmpty() ? QStringLiteral("worktree") : r.folder,
                                              "teamRoleFolder", [this, index](const QString &v) { m_catalog.roles[index].folder = v; changed(); }));
    form->addRow(tr("Can message"), segments({{"lead", tr("Lead only")}, {"team", tr("Whole team")}}, r.canMessage.isEmpty() ? QStringLiteral("lead") : r.canMessage,
                                            "teamRoleMessage", [this, index](const QString &v) { m_catalog.roles[index].canMessage = v; changed(); }));
    auto *tags = new QLineEdit(r.tags.join(QStringLiteral(", "))); tags->setPlaceholderText(tr("unit, Playwright")); tags->setObjectName("teamRoleTags");
    connect(tags, &QLineEdit::editingFinished, this, set([this, index, tags] {
        QStringList list; for (const auto &t : tags->text().split(',')) if (!t.trimmed().isEmpty()) list << t.trimmed();
        m_catalog.roles[index].tags = list;
    }));
    form->addRow(tr("Specialization"), tags);
    auto *dangerous = new QCheckBox(tr("May push / deploy / delete (only via tasks you approved)")); dangerous->setChecked(r.dangerous);
    connect(dangerous, &QCheckBox::toggled, this, [this, index](bool on) { m_catalog.roles[index].dangerous = on; changed(); });
    form->addRow(tr("Dangerous actions"), dangerous);
    auto *remove = new QPushButton(tr("Delete role")); remove->setAutoDefault(false); remove->setObjectName("teamRoleDelete");
    connect(remove, &QPushButton::clicked, this, [this, index] {
        for (const auto &t : m_catalog.templates) for (const auto &seat : t.roster) if (seat.role == m_catalog.roles[index].id) {
            QMessageBox::information(this, tr("Role in use"), tr("Template %1 uses this role; remove it there first.").arg(t.name));
            return;
        }
        m_catalog.roles.removeAt(index); changed(); rebuildLists(); rebuildEditor();
    });
    form->addRow(QString(), remove);
    return w;
}

QWidget *TeamCatalogEditor::templateEditor(int index)
{
    auto *w = new QWidget; w->setObjectName("teamTemplateEditor");
    auto *form = new QFormLayout(w); form->setLabelAlignment(Qt::AlignRight);
    Template &t = m_catalog.templates[index];
    auto *name = new QLineEdit(t.name); name->setObjectName("teamTemplateName");
    connect(name, &QLineEdit::editingFinished, this, [this, index, name] { m_catalog.templates[index].name = name->text().trimmed(); changed(); rebuildLists(); });
    form->addRow(tr("Name"), name);
    auto *description = new QLineEdit(t.description);
    connect(description, &QLineEdit::editingFinished, this, [this, index, description] { m_catalog.templates[index].description = description->text().trimmed(); changed(); });
    form->addRow(tr("Description"), description);
    auto *protocol = new QPlainTextEdit(t.leadProtocol); protocol->setFixedHeight(60); protocol->setPlaceholderText(tr("Extra instructions for the lead of this kind of team"));
    connect(protocol, &QPlainTextEdit::textChanged, this, [this, index, protocol] { m_catalog.templates[index].leadProtocol = protocol->toPlainText(); changed(); });
    form->addRow(tr("Lead protocol"), protocol);
    QList<std::pair<QString, QString>> roleItems;
    for (const auto &r : m_catalog.roles) roleItems.append({r.id, r.name});
    // Roster: role × count.
    auto *roster = new QWidget; auto *rv = new QVBoxLayout(roster); rv->setContentsMargins(0, 0, 0, 0); rv->setSpacing(3);
    for (int i = 0; i < t.roster.size(); ++i) {
        auto *r = new QHBoxLayout;
        auto *role = choice(roleItems, t.roster[i].role, "teamTemplateSeatRole");
        auto *count = new QSpinBox; count->setRange(1, 8); count->setValue(t.roster[i].count);
        auto *optional = new QCheckBox(tr("only if needed")); optional->setChecked(t.roster[i].optional);
        auto *del = new QToolButton; del->setText(QStringLiteral("✕")); del->setAutoRaise(true);
        connect(role, &QComboBox::activated, this, [this, index, i, role] { m_catalog.templates[index].roster[i].role = role->currentData().toString(); changed(); });
        connect(count, &QSpinBox::valueChanged, this, [this, index, i](int n) { m_catalog.templates[index].roster[i].count = n; changed(); });
        connect(optional, &QCheckBox::toggled, this, [this, index, i](bool on) { m_catalog.templates[index].roster[i].optional = on; changed(); });
        connect(del, &QToolButton::clicked, this, [this, index, i] { m_catalog.templates[index].roster.removeAt(i); changed(); rebuildEditor(); });
        r->addWidget(role, 1); r->addWidget(count); r->addWidget(optional); r->addWidget(del);
        rv->addLayout(r);
    }
    auto *addSeat = new QPushButton(tr("+ Seat")); addSeat->setAutoDefault(false);
    connect(addSeat, &QPushButton::clicked, this, [this, index] {
        m_catalog.templates[index].roster.append({m_catalog.roles.isEmpty() ? QString() : m_catalog.roles.last().id, 1, false}); changed(); rebuildEditor();
    });
    rv->addWidget(addSeat, 0, Qt::AlignLeft);
    form->addRow(tr("Roster"), roster);
    // Task skeleton: title, role, earlier tasks it depends on.
    auto *skeleton = new QWidget; auto *sv = new QVBoxLayout(skeleton); sv->setContentsMargins(0, 0, 0, 0); sv->setSpacing(3);
    QList<std::pair<QString, QString>> taskRoles{{"lead", tr("Lead")}};
    taskRoles += roleItems;
    for (int i = 0; i < t.tasks.size(); ++i) {
        auto *r = new QHBoxLayout;
        r->addWidget(new QLabel(QStringLiteral("#%1").arg(i + 1)));
        auto *title = new QLineEdit(t.tasks[i].title);
        auto *role = choice(taskRoles, t.tasks[i].role, "teamTemplateTaskRole");
        QStringList depText; for (int d : t.tasks[i].deps) depText << QString::number(d + 1);
        auto *deps = new QLineEdit(depText.join(QStringLiteral(", "))); deps->setPlaceholderText(tr("after #")); deps->setFixedWidth(70);
        auto *del = new QToolButton; del->setText(QStringLiteral("✕")); del->setAutoRaise(true);
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
        r->addWidget(title, 1); r->addWidget(role); r->addWidget(deps); r->addWidget(del);
        sv->addLayout(r);
    }
    auto *addTask = new QPushButton(tr("+ Task")); addTask->setAutoDefault(false);
    connect(addTask, &QPushButton::clicked, this, [this, index] {
        auto &tasks = m_catalog.templates[index].tasks;
        tasks.append({tr("New task"), QStringLiteral("lead"), tasks.isEmpty() ? QList<int>{} : QList<int>{int(tasks.size()) - 1}});
        changed(); rebuildEditor();
    });
    sv->addWidget(addTask, 0, Qt::AlignLeft);
    form->addRow(tr("Task skeleton"), skeleton);
    auto *planning = choice({{"lead", tr("Lead plans")}, {"template", tr("Use template tasks")}}, t.planning.isEmpty() ? QStringLiteral("lead") : t.planning, "teamTemplatePlanning");
    connect(planning, &QComboBox::activated, this, [this, index, planning] { m_catalog.templates[index].planning = planning->currentData().toString(); changed(); });
    form->addRow(tr("Planning"), planning);
    auto *approval = choice({{"review", tr("Review plan before running")}, {"auto", tr("Run when the plan has no errors")}}, t.approval.isEmpty() ? QStringLiteral("review") : t.approval, "teamTemplateApproval");
    connect(approval, &QComboBox::activated, this, [this, index, approval] { m_catalog.templates[index].approval = approval->currentData().toString(); changed(); });
    form->addRow(tr("Approval"), approval);
    auto *max = new QSpinBox; max->setRange(1, 32); max->setValue(t.maxMembers > 0 ? t.maxMembers : 6);
    connect(max, &QSpinBox::valueChanged, this, [this, index](int n) { m_catalog.templates[index].maxMembers = n; changed(); });
    form->addRow(tr("Max members"), max);
    auto *remove = new QPushButton(tr("Delete template")); remove->setAutoDefault(false);
    connect(remove, &QPushButton::clicked, this, [this, index] {
        const QString id = m_catalog.templates[index].id;
        m_catalog.templates.removeAt(index);
        m_catalog.routing.erase(std::remove_if(m_catalog.routing.begin(), m_catalog.routing.end(), [&](const Rule &r) { return r.templateId == id; }), m_catalog.routing.end());
        changed(); rebuildLists(); rebuildEditor();
    });
    form->addRow(QString(), remove);
    return w;
}

QWidget *TeamCatalogEditor::routingEditor()
{
    const Colors c = colorsFor(m_dark);
    auto *w = new QWidget; w->setObjectName("teamRoutingEditor");
    auto *v = new QVBoxLayout(w);
    v->addWidget(label(tr("Rules run in order; the first whose words all appear in the goal picks the template. Otherwise the first template."), c.muted, 12));
    QList<std::pair<QString, QString>> templates;
    for (const auto &t : m_catalog.templates) templates.append({t.id, t.name});
    for (int i = 0; i < m_catalog.routing.size(); ++i) {
        auto *r = new QHBoxLayout;
        r->addWidget(new QLabel(tr("When the goal has")));
        auto *when = new QLineEdit(m_catalog.routing[i].when); when->setObjectName("teamRuleWhen");
        auto *templ = choice(templates, m_catalog.routing[i].templateId, "teamRuleTemplate");
        auto *approval = choice({{"review", tr("review")}, {"auto", tr("auto")}}, m_catalog.routing[i].approval.isEmpty() ? QStringLiteral("review") : m_catalog.routing[i].approval, "teamRuleApproval");
        auto *up = new QToolButton; up->setText(QStringLiteral("↑")); up->setAutoRaise(true); up->setEnabled(i > 0);
        auto *del = new QToolButton; del->setText(QStringLiteral("✕")); del->setAutoRaise(true);
        connect(when, &QLineEdit::editingFinished, this, [this, i, when] { m_catalog.routing[i].when = when->text().trimmed(); changed(); });
        connect(templ, &QComboBox::activated, this, [this, i, templ] { m_catalog.routing[i].templateId = templ->currentData().toString(); changed(); });
        connect(approval, &QComboBox::activated, this, [this, i, approval] { m_catalog.routing[i].approval = approval->currentData().toString(); changed(); });
        connect(up, &QToolButton::clicked, this, [this, i] { m_catalog.routing.swapItemsAt(i, i - 1); changed(); rebuildEditor(); });
        connect(del, &QToolButton::clicked, this, [this, i] { m_catalog.routing.removeAt(i); changed(); rebuildEditor(); });
        r->addWidget(when, 1); r->addWidget(new QLabel(QStringLiteral("→"))); r->addWidget(templ); r->addWidget(approval); r->addWidget(up); r->addWidget(del);
        v->addLayout(r);
    }
    auto *add = new QPushButton(tr("+ Rule")); add->setAutoDefault(false); add->setEnabled(!templates.isEmpty());
    connect(add, &QPushButton::clicked, this, [this] {
        Rule r; r.id = QStringLiteral("rule-%1").arg(m_catalog.routing.size() + 1);
        while (std::any_of(m_catalog.routing.cbegin(), m_catalog.routing.cend(), [&](const Rule &x) { return x.id == r.id; })) r.id += 'x';
        r.templateId = m_catalog.templates.first().id; r.approval = "review";
        m_catalog.routing.append(r); changed(); rebuildEditor();
    });
    v->addWidget(add, 0, Qt::AlignLeft);
    // Try it: which template a goal would get.
    auto *tryRow = new QHBoxLayout;
    auto *goal = new QLineEdit; goal->setObjectName("teamRoutingTry"); goal->setPlaceholderText(tr("Try it: type a goal"));
    auto *result = new QLabel; result->setObjectName("teamRoutingResult");
    connect(goal, &QLineEdit::textChanged, this, [this, result](const QString &text) {
        const auto lower = text.toLower();
        for (const auto &rule : m_catalog.routing) {
            const auto words = rule.when.toLower().split(' ', Qt::SkipEmptyParts);
            if (!words.isEmpty() && std::all_of(words.cbegin(), words.cend(), [&](const QString &w) { return lower.contains(w); }))
                if (const auto *t = m_catalog.templ(rule.templateId)) { result->setText(tr("→ %1 (rule “%2”, %3)").arg(t->name, rule.when, rule.approval)); return; }
        }
        result->setText(m_catalog.templates.isEmpty() ? tr("No templates") : tr("→ %1 (no rule matched)").arg(m_catalog.templates.first().name));
    });
    tryRow->addWidget(goal, 1); tryRow->addWidget(result);
    v->addLayout(tryRow);
    v->addStretch();
    return w;
}
