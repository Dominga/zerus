// WhiteWh fork, U54: the team workspace's account move (the members menu's
// Move to another account), the account each member card shows and the review
// action for a member folder left on another branch.
#include "TeamWorkspace.h"
#include "AccountSwitch.h"
#include "HgsClient.h"
#include "ProcessRunner.h"
#include "TeamController.h"
#include "TeamPanel.h"
#include "TeamWidgets.h"
#include <QComboBox>
#include <QDialog>
#include <QDialogButtonBox>
#include <QGridLayout>
#include <QJsonArray>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>

using namespace Teams;
using namespace TeamUi;

namespace {
// Each member's U53 switch waits for an idle agent, copies its conversation
// and waits for the resumed one: one call for all of them gets 15 minutes.
constexpr int MoveTimeoutMs = 15 * 60 * 1000;

QString agentName(const QString &agent)
{
    if (agent == QLatin1String("claude")) return QStringLiteral("Claude Code");
    if (agent == QLatin1String("codex")) return QStringLiteral("Codex");
    return agent;
}

// Rebuilds a combo only when its items change, so a poll keeps the choice.
void fill(QComboBox *box, const QList<QPair<QString, QString>> &items)
{
    QList<QPair<QString, QString>> shown;
    for (int i = 0; i < box->count(); ++i) shown << qMakePair(box->itemData(i).toString(), box->itemText(i));
    if (shown == items) return;
    const QString current = box->currentData().toString();
    const QSignalBlocker block(box);
    box->clear();
    for (const auto &[id, text] : items) box->addItem(text, id);
    box->setCurrentIndex(qMax(0, box->findData(current)));
}

// A member as the move names it: the lead says so when its role does not.
QString memberName(const Team &team, const QString &id)
{
    const QString role = team.roleOf(id);
    const auto *member = team.member(id);
    if (!member || !member->lead || role.contains(QLatin1String("lead"), Qt::CaseInsensitive)) return role;
    return QObject::tr("%1 (lead)").arg(role);
}

QString gitError(const ProcessRunner::Result &result)
{
    if (result.outcome == ProcessRunner::Result::FailedToStart) return QObject::tr("git could not start: %1").arg(result.errorString);
    if (result.outcome == ProcessRunner::Result::TimedOut) return QObject::tr("git did not answer in time");
    QString text = QString::fromUtf8(result.standardError).trimmed();
    if (text.isEmpty()) text = QString::fromUtf8(result.standardOutput).trimmed();
    return QObject::tr("git refused: %1").arg(text.isEmpty() ? QObject::tr("exit code %1").arg(result.exitCode) : text);
}
}

void TeamWorkspace::requestAccounts()
{
    if (m_accountsRequest || m_accountsRead) return;
    m_accountsRequest = m_teams->client()->requestAccounts({});
}

QString TeamWorkspace::accountLabel(const Member &member) const
{
    // Labels are read for this computer; another computer's account shows its id.
    return AccountMove::labelOf(member.host.isEmpty() ? m_accountProfiles : QList<QJsonObject>{}, member.agent, AccountMove::memberAccount(member));
}

AccountMove::Step TeamWorkspace::moveStep(const Team &team, const QString &member) const
{
    if (m_move.team != team.id || !m_move.members.contains(member)) return {};
    return AccountMove::stepOf(team, m_move, member);
}

QWidget *TeamWorkspace::memberBlock(const Team &team, const Member &m)
{
    // The default account reads "Default account" without the listing; another
    // one needs its label (the Move dialog reads them anyway).
    if (!AccountMove::memberAccount(m).startsWith(QLatin1String("native-"))) requestAccounts();
    const Colors c = colorsFor(m_dark);
    const auto step = moveStep(team, m.id);
    const auto block = AccountMove::blockOf(m);
    const QString refusal = m_checkoutErrors.value(m.id);
    if (step.state.isEmpty() && block.kind == AccountMove::Block::None && refusal.isEmpty()) return nullptr;
    auto *box = new QWidget; box->setObjectName("teamWorkspaceMemberBlock"); box->setProperty("member", m.id);
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(3);
    if (!step.state.isEmpty()) {
        auto *mark = new StatusMark(step.text, toneColor(c, step.tone), step.kind, 0, Qt::AlignLeft, nullptr, step.text);
        mark->setObjectName("teamWorkspaceMemberMove"); mark->setProperty("step", step.state);
        v->addWidget(mark);
    }
    // The state line names the wait; the folder's branches and the action are here.
    if (block.kind == AccountMove::Block::FolderBranch && step.state.isEmpty()) {
        auto *reason = label(block.text, c.danger, 11, false, "teamWorkspaceMemberBlockReason");
        reason->setWordWrap(true);
        reason->setToolTip(m.error);
        reason->setProperty("blockKind", QStringLiteral("folder_branch"));
        v->addWidget(reason);
    }
    auto *line = row(6);
    QPushButton *action = nullptr;
    const QString teamId = team.id, id = m.id;
    if (block.kind == AccountMove::Block::FolderBranch && m.host.isEmpty() && m.computer.isEmpty()) {
        // #619's review action, never for a folder on another computer.
        action = button(tr("Check out %1…").arg(block.own), "teamWorkspaceMemberCheckout");
        action->setEnabled(!m_checkingOut.contains(m.id));
        action->setToolTip(tr("Switches %1 to %2 with git switch, without --force or a reset, then relaunches it").arg(block.folder, block.own));
        QObject::connect(action, &QPushButton::clicked, this, [this, teamId, id] {
            QTimer::singleShot(0, this, [this, teamId, id] {
                const auto *current = currentTeam(teamId);
                if (const auto *found = current ? current->member(id) : nullptr) checkoutMemberBranch(*current, *found, AccountMove::blockOf(*found));
            });
        });
    } else if (block.kind == AccountMove::Block::OldSession && !m.session.isEmpty()) {
        action = button(tr("Move again…"), "teamWorkspaceMemberMoveAgain");
        action->setEnabled(!m_move.running);
        action->setToolTip(tr("Once its old session is idle, move it again; a busy session is never interrupted"));
        QObject::connect(action, &QPushButton::clicked, this, [this, teamId, id] {
            QTimer::singleShot(0, this, [this, teamId, id] {
                const auto *current = currentTeam(teamId);
                if (const auto *found = current ? current->member(id) : nullptr) accountMoveDialog(*current, *found);
            });
        });
    } else if (block.kind == AccountMove::Block::OldSession) {
        action = button(tr("Relaunch"), "teamWorkspaceMemberRelaunch");
        action->setToolTip(tr("Once its old session is paused, start its new session"));
        QObject::connect(action, &QPushButton::clicked, this, [this, id] { m_panel->act("relaunch", {{"member", id}}); });
    } else if (block.kind == AccountMove::Block::Uncertain && !m.session.isEmpty()) {
        action = button(tr("Open session"), "teamWorkspaceMemberOpenSession");
        action->setToolTip(tr("Check which account and conversation it runs on; Zerus does not retry the switch"));
        QObject::connect(action, &QPushButton::clicked, this, [this, host = m.host, session = m.session] { if (openSession) openSession(host, session); });
    }
    if (action) { action->setProperty("ghost", true); line->addWidget(action); }
    if (line->count()) {
        line->addStretch();
        v->addLayout(line);
    } else {
        delete line;
    }
    if (!refusal.isEmpty()) {
        auto *text = label(refusal, c.danger, 11, false, "teamWorkspaceMemberCheckoutError");
        text->setWordWrap(true);
        v->addWidget(text);
    }
    return box;
}

void TeamWorkspace::checkoutMemberBranch(const Team &team, const Member &member, const AccountMove::Block &block)
{
    if (block.kind != AccountMove::Block::FolderBranch || !member.host.isEmpty() || !member.computer.isEmpty() || m_checkingOut.contains(member.id)) return;
    const QString role = team.roleOf(member.id);
    const QString text = tr("Switch %1 from %2 to %3?\n\nZerus runs git switch %3 in that folder, without --force and without a reset; "
                            "a folder with uncommitted changes is left as it is. %2 is kept. Then %4 is relaunched.")
                             .arg(block.folder, block.current, block.own, role);
    if (!m_panel->confirm("teamCheckoutConfirm", tr("Check out the member branch?"), text, tr("Check out"))) return;
    const QString id = member.id, folder = block.folder, own = block.own;
    m_checkingOut.insert(id); m_checkoutErrors.remove(id);
    scheduleRender();
    const auto fail = [this, id](const QString &error) {
        m_checkingOut.remove(id); m_checkoutErrors.insert(id, error); scheduleRender();
    };
    ProcessRunner::Request status;
    status.program = QStringLiteral("git");
    status.arguments = {QStringLiteral("-C"), folder, QStringLiteral("status"), QStringLiteral("--porcelain")};
    status.timeoutMs = 20000;
    ProcessRunner::run(status, this, [this, id, folder, own, fail](const ProcessRunner::Result &result) {
        if (result.outcome != ProcessRunner::Result::Finished || result.exitCode != 0) return fail(gitError(result));
        if (!result.standardOutput.trimmed().isEmpty())
            return fail(tr("%1 has uncommitted changes; commit them, then check out %2.").arg(folder, own));
        ProcessRunner::Request change;
        change.program = QStringLiteral("git");
        change.arguments = {QStringLiteral("-C"), folder, QStringLiteral("switch"), own};
        change.timeoutMs = 20000;
        ProcessRunner::run(change, this, [this, id, fail](const ProcessRunner::Result &switched) {
            if (switched.outcome != ProcessRunner::Result::Finished || switched.exitCode != 0) return fail(gitError(switched));
            m_checkingOut.remove(id);
            m_panel->act("relaunch", {{"member", id}}, [this, id](bool ok, const QString &message) {
                if (!ok) m_checkoutErrors.insert(id, tr("Checked out; the relaunch was refused: %1").arg(message));
                scheduleRender();
            });
            scheduleRender();
        });
    });
}

void TeamWorkspace::accountMoveDialog(const Team &team, const Member &member)
{
    if (m_moveDialog) { m_moveDialog->raise(); m_moveDialog->activateWindow(); return; }
    if (m_move.running) return;
    // A finished move's results give way to the next one.
    m_move = {};
    m_accountsRead = false; requestAccounts();
    const Colors c = colorsFor(m_dark);
    auto *dialog = new QDialog(this);
    m_moveDialog = dialog;
    dialog->setObjectName("teamMoveAccountDialog");
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    dialog->setProperty("team", team.id);
    dialog->setWindowTitle(tr("Move to another account"));
    dialog->resize(540, 380);
    auto *v = new QVBoxLayout(dialog);
    auto *intro = label(tr("Every member of the agent whose session runs on the first account moves to the second, the lead included. "
                           "Each one stops once it is idle and resumes on the new account in its own conversation, folder and name; "
                           "a busy one is never interrupted."),
                        c.text2, 12);
    intro->setWordWrap(true);
    v->addWidget(intro);
    auto *grid = new QGridLayout; grid->setHorizontalSpacing(6); grid->setVerticalSpacing(6);
    auto *agent = new QComboBox; agent->setObjectName("teamMoveAccountAgent");
    for (const auto &a : AccountMove::agents(team)) agent->addItem(agentLogo(a, c.fg), agentName(a), a);
    agent->setCurrentIndex(qMax(0, agent->findData(member.agent)));
    auto *from = new QComboBox; from->setObjectName("teamMoveAccountFrom");
    auto *to = new QComboBox; to->setObjectName("teamMoveAccountTo");
    grid->addWidget(label(tr("Agent"), c.muted, 11), 0, 0); grid->addWidget(agent, 0, 1, 1, 3);
    grid->addWidget(label(tr("From"), c.muted, 11), 1, 0); grid->addWidget(from, 1, 1);
    grid->addWidget(label(tr("To"), c.muted, 11), 1, 2); grid->addWidget(to, 1, 3);
    grid->setColumnStretch(1, 1); grid->setColumnStretch(3, 1);
    v->addLayout(grid);
    auto *head = label(QString(), c.fg, 12, true, "teamMoveAccountHead"); head->setWordWrap(true);
    v->addWidget(head);
    auto *list = new QWidget; list->setObjectName("teamMoveAccountMembers");
    auto *rows = new QVBoxLayout(list); rows->setContentsMargins(0, 0, 0, 0); rows->setSpacing(4);
    v->addWidget(list);
    auto *note = label(QString(), c.muted, 11, false, "teamMoveAccountNote"); note->setWordWrap(true);
    v->addWidget(note);
    auto *error = label(QString(), c.danger, 12, false, "teamMoveAccountError"); error->setWordWrap(true); error->hide();
    v->addWidget(error);
    v->addStretch();
    auto *buttons = new QDialogButtonBox;
    auto *apply = buttons->addButton(tr("Move…"), QDialogButtonBox::AcceptRole); apply->setObjectName("teamMoveAccountApply");
    auto *close = buttons->addButton(QDialogButtonBox::Cancel); close->setObjectName("teamMoveAccountClose");
    v->addWidget(buttons);
    // The first From is the account of the member the menu was opened on.
    from->setProperty("initial", AccountMove::memberAccount(member));
    QObject::connect(agent, &QComboBox::currentIndexChanged, dialog, [this] { renderAccountMove(); });
    QObject::connect(from, &QComboBox::currentIndexChanged, dialog, [this] { renderAccountMove(); });
    QObject::connect(close, &QPushButton::clicked, dialog, &QDialog::close);
    QObject::connect(dialog, &QDialog::finished, dialog, [this] {
        // Closed after the move finished: the cards stop showing its steps.
        if (m_move.finished) { m_move = {}; scheduleRender(); }
    });
    QObject::connect(apply, &QPushButton::clicked, dialog, [this, dialog, agent, from, to] {
        const auto *current = currentTeam(dialog->property("team").toString());
        if (!current || to->currentData().toString().isEmpty()) return;
        const QString provider = agent->currentData().toString(), source = from->currentData().toString(), target = to->currentData().toString();
        const QStringList ids = AccountMove::affected(*current, provider, source);
        if (ids.isEmpty()) return;
        QStringList roles;
        for (const auto &id : ids) roles << memberName(*current, id);
        const QString fromLabel = AccountMove::labelOf(m_accountProfiles, provider, source);
        const QString toLabel = AccountMove::labelOf(m_accountProfiles, provider, target);
        const QString text = tr("Move %1 from %2 to %3?\n\nEach one stops once it is idle, its conversation is copied to %3 and it resumes there "
                                "in the same conversation, folder and name. A busy member is not interrupted: it waits, and its card says so. "
                                "One that cannot resume is paused and saved, and gets a new session on %3.")
                                 .arg(roles.join(QStringLiteral(", ")), fromLabel, toLabel);
        if (!m_panel->confirm("teamMoveAccountConfirm", tr("Move members to another account?"), text, tr("Move"))) return;
        startAccountMove(*current, provider, source, target);
    });
    renderAccountMove();
    dialog->show();
}

void TeamWorkspace::startAccountMove(const Team &team, const QString &agent, const QString &from, const QString &to)
{
    if (m_move.running) return;
    m_move = {};
    m_move.team = team.id; m_move.agent = agent; m_move.from = from; m_move.to = to;
    m_move.fromLabel = AccountMove::labelOf(m_accountProfiles, agent, from);
    m_move.toLabel = AccountMove::labelOf(m_accountProfiles, agent, to);
    m_move.members = AccountMove::affected(team, agent, from);
    for (const auto &id : m_move.members) if (const auto *member = team.member(id)) m_move.runs.insert(id, member->runId);
    m_move.running = true;
    const QString teamId = team.id;
    m_teams->call(QStringLiteral("swap-member"), {{"team", team.id}, {"from_account", from}, {"to_account", to}, {"provider", agent}},
        [this, teamId](bool ok, const QJsonObject &result, const QString &error) {
            if (m_move.team != teamId || !m_move.running) return;
            m_move.running = false; m_move.finished = true;
            if (ok) m_move.receipt = result; else m_move.error = error.trimmed().startsWith(QLatin1String("hgs: ")) ? error.trimmed().mid(5) : error.trimmed();
            m_accountsRead = false; requestAccounts();
            // Without its dialog the results stay on the cards for two minutes.
            if (!m_moveDialog)
                QTimer::singleShot(120000, this, [this, teamId] {
                    if (m_move.team == teamId && m_move.finished && !m_moveDialog) { m_move = {}; scheduleRender(); }
                });
            renderAccountMove();
            scheduleRender();
        }, true, MoveTimeoutMs);
    renderAccountMove();
    scheduleRender();
}

void TeamWorkspace::renderAccountMove()
{
    if (!m_moveDialog) return;
    QDialog *dialog = m_moveDialog;
    const auto *team = currentTeam(dialog->property("team").toString());
    if (!team) return;
    const Colors c = colorsFor(m_dark);
    auto *agent = dialog->findChild<QComboBox *>("teamMoveAccountAgent");
    auto *from = dialog->findChild<QComboBox *>("teamMoveAccountFrom");
    auto *to = dialog->findChild<QComboBox *>("teamMoveAccountTo");
    auto *head = dialog->findChild<QLabel *>("teamMoveAccountHead");
    auto *note = dialog->findChild<QLabel *>("teamMoveAccountNote");
    auto *error = dialog->findChild<QLabel *>("teamMoveAccountError");
    auto *list = dialog->findChild<QWidget *>("teamMoveAccountMembers");
    auto *apply = dialog->findChild<QPushButton *>("teamMoveAccountApply");
    auto *close = dialog->findChild<QPushButton *>("teamMoveAccountClose");
    const bool started = m_move.team == team->id && (m_move.running || m_move.finished);
    const QString provider = started ? m_move.agent : agent->currentData().toString();
    if (!started) {
        QList<QPair<QString, QString>> sources;
        for (const auto &account : AccountMove::fromAccounts(*team, provider)) sources << qMakePair(account, AccountMove::labelOf(m_accountProfiles, provider, account));
        fill(from, sources);
        if (const QString initial = from->property("initial").toString(); !initial.isEmpty() && from->findData(initial) >= 0) {
            const QSignalBlocker block(from);
            from->setCurrentIndex(from->findData(initial));
            from->setProperty("initial", QString());
        }
        QJsonArray profiles; for (const auto &profile : m_accountProfiles) profiles.append(profile);
        QList<QPair<QString, QString>> targets;
        for (const auto &option : AccountSwitch::options(profiles, QString(), provider, from->currentData().toString(), m_dark)) {
            if (option.current) continue;
            targets << qMakePair(option.id, option.usage.isEmpty() ? option.label : tr("%1 (%2)").arg(option.label, option.usage));
        }
        fill(to, targets);
    }
    for (auto *box : std::initializer_list<QComboBox *>{agent, from, to}) box->setEnabled(!started);
    const QStringList ids = started ? m_move.members : AccountMove::affected(*team, provider, from->currentData().toString());
    // The member rows: role, session and, once the move runs, its step.
    for (auto *old : list->findChildren<QWidget *>("teamMoveAccountMember", Qt::FindDirectChildrenOnly)) { old->setObjectName(QString()); old->hide(); old->deleteLater(); }
    for (const auto &id : ids) {
        const auto *member = team->member(id);
        auto *line = new QWidget(list); line->setObjectName("teamMoveAccountMember"); line->setProperty("member", id);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
        const QString role = team->roleOf(id);
        h->addWidget(new Pill(memberName(*team, id), roleColor(role, m_dark)));
        h->addWidget(elided(member ? member->session : QString(), c.text3, 11), 1);
        if (started) {
            const auto step = AccountMove::stepOf(*team, m_move, id);
            auto *mark = new StatusMark(step.text, toneColor(c, step.tone), step.kind, 260, Qt::AlignLeft, nullptr, step.text);
            mark->setObjectName("teamMoveAccountStep"); mark->setProperty("step", step.state);
            h->addWidget(mark);
        }
        list->layout()->addWidget(line);
    }
    const QString toLabel = started ? m_move.toLabel : AccountMove::labelOf(m_accountProfiles, provider, to->currentData().toString());
    if (m_move.running && started) head->setText(tr("Moving %1 to %2; each one waits until its agent is idle.").arg(ids.size() == 1 ? tr("1 member") : tr("%1 members").arg(ids.size()), toLabel));
    else if (started) head->setText(m_move.error.isEmpty() ? tr("The move to %1 is done; each member's result is below.").arg(toLabel) : tr("Nothing moved to %1.").arg(toLabel));
    else if (ids.isEmpty()) head->setText(tr("No member runs on this account"));
    else head->setText(ids.size() == 1 ? tr("Moves 1 member:") : tr("Moves %1 members:").arg(ids.size()));
    if (!started && to->count() == 0)
        note->setText(m_accountsRead ? tr("No other signed-in %1 account on this computer; sign one in under Accounts.").arg(agentName(provider))
                                     : tr("Reading the accounts…"));
    else if (!started) note->setText(tr("Members on other computers and members without a session stay where they are."));
    else note->setText(m_move.running ? tr("You may close this window; the cards show each member's progress.") : QString());
    error->setText(m_move.error); error->setVisible(started && !m_move.error.isEmpty());
    apply->setVisible(!started);
    apply->setEnabled(!ids.isEmpty() && to->count() > 0 && !to->currentData().toString().isEmpty());
    close->setText(started ? tr("Close") : tr("Cancel"));
}
