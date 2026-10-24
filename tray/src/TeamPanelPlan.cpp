// The Team tab while a team plans:
// editable members and tasks, dependencies dragged on the graph, plan errors,
// and the pinned footer with Approve & Run, Send back to lead and Discard plan.
// Every edit sends the whole plan once (`hgs team plan`) with the revision it
// was made against, so a lead's concurrent change is never overwritten.
#include "TeamController.h"
#include "TeamPanel.h"
#include "TeamWidgets.h"
#include <QCheckBox>
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QFormLayout>
#include <QInputDialog>
#include <QLineEdit>
#include <QMenu>
#include <QPlainTextEdit>
#include <QScrollArea>
#include <QToolButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
const QStringList agents{"claude", "codex", "kimi", "dsh", "opencode"};
QString agentLabel(const QString &agent)
{
    static const QMap<QString, QString> names{{"claude", "Claude Code"}, {"codex", "Codex"}, {"kimi", "Kimi"}, {"dsh", "DeepSeek"}, {"opencode", "OpenCode"}};
    return names.value(agent, agent);
}
}

void TeamPanel::savePlan(const Team &edited)
{
    QJsonObject input = planInput(edited);
    input.insert("revision", edited.revision);
    act("plan", input);
}

QWidget *TeamPanel::planMembers(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.members.size()), c.muted, 12));
    title->addStretch();
    auto *add = button(tr("+ Member"), "teamAddMember");
    QObject::connect(add, &QPushButton::clicked, this, [this, team] { addMember(team, QCursor::pos()); });
    title->addWidget(add);
    v->addLayout(title);
    for (const auto &member : team.members) {
        auto *rowBox = card(c);
        rowBox->setObjectName("teamCard");
        rowBox->setProperty("teamMember", member.id);
        if (member.session.isEmpty())
            rowBox->setStyleSheet(QStringLiteral("QFrame#teamCard{background:transparent;border:1px dashed %1;border-radius:9px;}").arg(c.border.name()));
        auto *outer = new QVBoxLayout(rowBox); outer->setContentsMargins(8, 7, 6, 7); outer->setSpacing(5);
        auto *h = row(8);
        auto *tile = new QLabel; tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
        tile->setStyleSheet(QStringLiteral("background:%1;border-radius:7px;").arg(c.tile.name()));
        tile->setPixmap(workspaceIcon(agentIcon(member.agent), c.fg).pixmap(16, 16));
        h->addWidget(tile, 0, Qt::AlignTop);
        auto *body = new QVBoxLayout; body->setSpacing(4);
        auto *line1 = row(6);
        const QString role = team.roleOf(member.id);
        line1->addWidget(new Pill(member.lead && member.role != role ? role + QStringLiteral(" · ") + member.role : role, roleColor(role, m_dark)));
        line1->addWidget(new StatusMark(member.session.isEmpty() ? tr("Created on approval") : tr("Planning"),
                                        member.session.isEmpty() ? c.dim : c.folder, member.session.isEmpty() ? "not_created" : "planning"));
        line1->addStretch();
        body->addLayout(line1);
        auto *line2 = row(6);
        auto *agent = new QComboBox; agent->setObjectName("teamMemberAgent");
        for (const auto &a : agents) agent->addItem(agentLabel(a), a);
        if (agent->findData(member.agent) < 0) agent->addItem(member.agent, member.agent);
        agent->setCurrentIndex(agent->findData(member.agent));
        auto *model = new QComboBox; model->setObjectName("teamMemberModel"); model->setEditable(true);
        model->setMinimumWidth(70); model->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
        agent->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed); agent->setMinimumWidth(84);
        model->addItem(member.model);
        for (const auto &known : {QStringLiteral("Opus"), QStringLiteral("Sonnet"), QStringLiteral("gpt-5.5-codex"), QStringLiteral("deepseek-v4")})
            if (known != member.model) model->addItem(known);
        model->setCurrentText(member.model);
        model->lineEdit()->setPlaceholderText(tr("default model"));
        // The lead already runs: its agent and model stay.
        agent->setEnabled(!member.lead && member.session.isEmpty()); model->setEnabled(!member.lead && member.session.isEmpty());
        const auto changed = [this, team, id = member.id, agent, model] {
            Team edited = team;
            for (auto &m : edited.members) if (m.id == id) { m.agent = agent->currentData().toString(); m.model = model->currentText().trimmed(); }
            savePlan(edited);
        };
        QObject::connect(agent, &QComboBox::activated, this, changed);
        QObject::connect(model->lineEdit(), &QLineEdit::editingFinished, this, [changed, model, before = member.model] {
            if (model->currentText().trimmed() != before) changed();
        });
        line2->addWidget(agent, 2); line2->addWidget(model, 3);
        body->addLayout(line2);
        int tasks = 0; for (const auto &t : team.tasks) tasks += t.assignee == member.id;
        body->addWidget(elided((member.computer.isEmpty() ? tr("this computer") : member.computer) + QStringLiteral(" · ") + tr("%n task(s)", nullptr, tasks), c.muted, 11));
        h->addLayout(body, 1);
        if (!member.lead) {
            auto *more = new QToolButton; more->setObjectName("teamMemberMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
            more->setPopupMode(QToolButton::InstantPopup); more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
            auto *menu = new QMenu(more);
            menu->addAction(tr("Role prompt…"), this, [this, team, id = member.id, prompt = member.prompt] {
                bool ok = false;
                const auto text = QInputDialog::getMultiLineText(this, tr("Role prompt"), tr("Instructions this member gets with every task:"), prompt, &ok);
                if (!ok) return;
                Team edited = team;
                for (auto &m : edited.members) if (m.id == id) m.prompt = text.trimmed();
                savePlan(edited);
            });
            auto *worktree = menu->addAction(tr("Own worktree"));
            worktree->setCheckable(true); worktree->setChecked(member.folder == "worktree");
            QObject::connect(worktree, &QAction::toggled, this, [this, team, id = member.id](bool on) {
                Team edited = team;
                for (auto &m : edited.members) if (m.id == id) m.folder = on ? "worktree" : "shared";
                savePlan(edited);
            });
            menu->addSeparator();
            menu->addAction(tr("Remove from plan"), this, [this, team, id = member.id] {
                Team edited = team;
                edited.members.erase(std::remove_if(edited.members.begin(), edited.members.end(), [&](const Member &m) { return m.id == id; }), edited.members.end());
                for (auto &t : edited.tasks) if (t.assignee == id) t.assignee.clear();
                savePlan(edited);
            });
            more->setMenu(menu);
            h->addWidget(more, 0, Qt::AlignTop);
        }
        outer->addLayout(h);
        // A row error with its fix, from the plan check.
        for (const auto &error : team.errors) if (error.contains(team.roleOf(member.id)) || error.contains(member.id + QStringLiteral(":"))) {
            auto *e = label(error, c.danger, 11); e->setWordWrap(true); outer->addWidget(e);
        }
        v->addWidget(rowBox);
    }
    return box;
}

void TeamPanel::addMember(const Team &team, const QPoint &global)
{
    QMenu menu(this);
    menu.setObjectName("teamAddMemberMenu");
    const auto &catalog = m_controller->catalog();
    const auto addWith = [this, team](Member member) {
        Team edited = team;
        member.id = nextMemberId(team, member.role);
        edited.members.append(member);
        savePlan(edited);
    };
    for (const auto &role : catalog.roles) {
        if (role.id == "lead") continue;
        menu.addAction(QStringLiteral("%1 · %2").arg(role.name, agentLabel(role.agent.isEmpty() ? "claude" : role.agent)), this, [role, addWith] {
            Member m;
            m.role = role.name; m.agent = role.agent.isEmpty() ? QStringLiteral("claude") : role.agent; m.account = role.account;
            m.model = role.model; m.effort = role.effort; m.permissions = role.permissions; m.computer = role.computer;
            m.folder = role.folder == "worktree" ? QStringLiteral("worktree") : QStringLiteral("shared");
            m.prompt = role.prompt;
            addWith(m);
        });
    }
    if (!catalog.roles.isEmpty()) menu.addSeparator();
    menu.addAction(tr("Custom role…"), this, [this, addWith] {
        bool ok = false;
        const auto name = QInputDialog::getText(this, tr("New member"), tr("Role name:"), QLineEdit::Normal, {}, &ok).trimmed();
        if (!ok || name.isEmpty()) return;
        Member m; m.role = name; m.agent = QStringLiteral("claude"); m.folder = QStringLiteral("shared");
        addWith(m);
    });
    if (!m_controller->catalogReady()) m_controller->refreshCatalog();
    menu.exec(global);
}

void TeamPanel::editTask(const Team &team, int taskId)
{
    const Task *existing = team.task(taskId);
    Task task = existing ? *existing : Task{};
    if (!existing) { task.id = nextTaskId(team); task.status = "draft"; }
    QDialog dialog(this);
    dialog.setObjectName("teamTaskEditor");
    dialog.setWindowTitle(existing ? tr("Task #%1").arg(task.id) : tr("New task"));
    dialog.resize(520, 560);
    auto *form = new QFormLayout(&dialog);
    auto *title = new QLineEdit(task.title); title->setObjectName("teamTaskEditTitle");
    auto *description = new QPlainTextEdit(task.description); description->setObjectName("teamTaskEditDescription"); description->setFixedHeight(90);
    auto *criteria = new QPlainTextEdit(task.criteria.join('\n')); criteria->setObjectName("teamTaskEditCriteria"); criteria->setFixedHeight(70);
    criteria->setPlaceholderText(tr("One criterion per line"));
    auto *assignee = new QComboBox; assignee->setObjectName("teamTaskEditAssignee");
    assignee->addItem(tr("No assignee"), QString());
    for (const auto &m : team.members) assignee->addItem(QStringLiteral("%1 · %2").arg(team.roleOf(m.id), agentLabel(m.agent)), m.id);
    assignee->setCurrentIndex(qMax(0, assignee->findData(task.assignee)));
    form->addRow(tr("Title"), title);
    form->addRow(tr("Description"), description);
    form->addRow(tr("Acceptance criteria"), criteria);
    form->addRow(tr("Assignee"), assignee);
    // Dependencies: each other task, hard or soft.
    auto *depsBox = new QWidget; auto *deps = new QVBoxLayout(depsBox); deps->setContentsMargins(0, 0, 0, 0); deps->setSpacing(2);
    QList<std::tuple<int, QCheckBox *, QCheckBox *>> depRows;
    for (const auto &other : team.tasks) {
        if (other.id == task.id) continue;
        auto *r = row(8);
        auto *on = new QCheckBox(QStringLiteral("#%1 %2").arg(other.id).arg(other.title));
        auto *soft = new QCheckBox(tr("soft"));
        soft->setToolTip(tr("Soft: may start once #%1 is In progress; hard waits until it is Accepted").arg(other.id));
        bool depends = false, isSoft = false;
        for (const auto &d : task.deps) if (d.task == other.id) { depends = true; isSoft = d.soft; }
        on->setChecked(depends); soft->setChecked(isSoft); soft->setEnabled(depends);
        QObject::connect(on, &QCheckBox::toggled, soft, &QCheckBox::setEnabled);
        r->addWidget(on, 1); r->addWidget(soft);
        deps->addLayout(r);
        depRows.append({other.id, on, soft});
    }
    if (depRows.isEmpty()) deps->addWidget(new QLabel(tr("No other tasks yet")));
    auto *scroll = new QScrollArea; scroll->setWidget(depsBox); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame); scroll->setMaximumHeight(160);
    form->addRow(tr("Depends on"), scroll);
    auto *buttons = new QDialogButtonBox(QDialogButtonBox::Save | QDialogButtonBox::Cancel);
    QPushButton *remove = nullptr;
    if (existing) { remove = buttons->addButton(tr("Delete task"), QDialogButtonBox::DestructiveRole); remove->setObjectName("teamTaskEditDelete"); }
    bool removed = false;
    QObject::connect(buttons, &QDialogButtonBox::accepted, &dialog, [&] { if (!title->text().trimmed().isEmpty()) dialog.accept(); else title->setFocus(); });
    QObject::connect(buttons, &QDialogButtonBox::rejected, &dialog, &QDialog::reject);
    if (remove) QObject::connect(remove, &QPushButton::clicked, &dialog, [&] { removed = true; dialog.accept(); });
    form->addRow(buttons);
    if (dialog.exec() != QDialog::Accepted) return;
    Team edited = team;
    if (removed) {
        edited.tasks.erase(std::remove_if(edited.tasks.begin(), edited.tasks.end(), [&](const Task &t) { return t.id == task.id; }), edited.tasks.end());
        for (auto &t : edited.tasks) t.deps.erase(std::remove_if(t.deps.begin(), t.deps.end(), [&](const Dep &d) { return d.task == task.id; }), t.deps.end());
        savePlan(edited);
        return;
    }
    task.title = title->text().trimmed();
    task.description = description->toPlainText().trimmed();
    task.criteria.clear();
    for (const auto &line : criteria->toPlainText().split('\n')) if (!line.trimmed().isEmpty()) task.criteria << line.trimmed();
    task.assignee = assignee->currentData().toString();
    task.deps.clear();
    for (const auto &[id, on, soft] : depRows) if (on->isChecked()) task.deps.append({id, soft->isChecked()});
    bool replaced = false;
    for (auto &t : edited.tasks) if (t.id == task.id) { t = task; replaced = true; }
    if (!replaced) edited.tasks.append(task);
    savePlan(edited);
}

void TeamPanel::link(const Team &team, int dependency, int taskId, bool add)
{
    Team edited = team;
    for (auto &t : edited.tasks) {
        if (t.id != taskId) continue;
        t.deps.erase(std::remove_if(t.deps.begin(), t.deps.end(), [&](const Dep &d) { return d.task == dependency; }), t.deps.end());
        if (add) t.deps.append({dependency, false});
    }
    savePlan(edited);
}

QWidget *TeamPanel::planFooter(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamPlanFooter");
    box->setStyleSheet(QStringLiteral("QWidget#teamPlanFooter{border-top:1px solid %1;}").arg(c.border.name()));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 8, 10, 10); v->setSpacing(6);
    if (!team.errors.isEmpty())
        v->addWidget(new StatusMark(tr("Fix %n plan error(s) to run", nullptr, int(team.errors.size())), c.warn, "stuck"));
    auto *buttons = row(6);
    auto *approve = button(tr("Approve && Run"), "teamApprove", true);
    approve->setEnabled(team.errors.isEmpty() && !team.tasks.isEmpty());
    approve->setToolTip(team.tasks.isEmpty() ? tr("The plan has no tasks yet")
                        : team.errors.isEmpty() ? tr("Start the members and hand out the first tasks")
                                                : tr("Fix %n plan error(s) to run", nullptr, int(team.errors.size())));
    QObject::connect(approve, &QPushButton::clicked, this, [this] { act("approve", {}); });
    auto *sendBack = button(tr("Send back to lead"), "teamSendBack");
    sendBack->setCheckable(true); sendBack->setChecked(m_sendBack);
    QObject::connect(sendBack, &QPushButton::toggled, this, [this](bool on) { m_sendBack = on; scheduleRender(); });
    auto *discard = button(tr("Discard plan…"), "teamDiscard");
    discard->setStyleSheet(QStringLiteral("color:%1;").arg(c.danger.name()));
    QObject::connect(discard, &QPushButton::clicked, this, [this, name = team.name] {
        if (confirm("teamDiscardConfirm", tr("Discard plan?"), tr("Discard the plan of %1? The team stops before any member starts.").arg(name), tr("Discard plan")))
            act("discard", {});
    });
    buttons->addWidget(approve); buttons->addWidget(sendBack); buttons->addStretch(); buttons->addWidget(discard);
    v->addLayout(buttons);
    if (m_sendBack) {
        auto *comment = new QPlainTextEdit; comment->setObjectName("teamSendBackText"); comment->setFixedHeight(60);
        comment->setPlaceholderText(tr("What should the lead change?"));
        v->addWidget(comment);
        auto *send = button(tr("Send to Lead"), "teamSendBackSubmit", true);
        QObject::connect(send, &QPushButton::clicked, this, [this, comment, team] {
            const auto text = comment->toPlainText().trimmed();
            const auto *lead = team.member(team.lead);
            if (text.isEmpty() || !lead || lead->session.isEmpty() || !sendMessage) return;
            sendMessage(lead->host, lead->session,
                        tr("[Zerus team · from: You · planning]\nPlan review for %1: %2\nChange the plan and send it again with hgs team plan --json (hgs guide team).")
                            .arg(team.name, text));
            m_sendBack = false; scheduleRender();
        });
        auto *r = row(); r->addWidget(send); r->addStretch(); v->addLayout(r);
    }
    return box;
}
