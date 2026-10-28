// Sessions rows, prepared off the GUI thread (spec "Model preparation off the
// GUI thread"): rowInputs() copies what the rows read on the GUI thread,
// prepareRows() runs on the shared preparation thread, and applyRows() writes
// the newest result, touching only rows whose digest changed.
#include "SessionsWindow.h"

#include "AccountFilter.h"
#include "ArchiveBrowser.h"
#include "CacheKeepalive.h"
#include "GitStatusBadge.h"
#include "JsonParse.h"
#include "MachineAppearance.h"
#include "MachineFilter.h"
#include "MachinePlatform.h"
#include "MessageComposer.h"
#include "ProcessRunner.h"
#include "ProjectFilter.h"
#include "SessionCreationController.h"
#include "SessionList.h"
#include "SessionPanelDock.h"
#include "SessionPresentation.h"
#include "SessionLeads.h"
#include "SessionSearch.h"
#include "SessionStates.h"
#include "SessionTooltip.h"
#include "TeamController.h"
#include "TerminalView.h"
#include "WorkspaceIcons.h"

#include <QAction>
#include <QCryptographicHash>
#include <QDataStream>
#include <QDateTime>
#include <QDir>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QScreen>
#include <QScrollBar>
#include <QSettings>
#include <QStackedWidget>
#include <QTabWidget>
#include <QToolTip>

#include <algorithm>
#include <functional>
#include <tuple>

namespace {

using SessionPresentation::childCount;
using SessionPresentation::currentAction;
using SessionPresentation::currentActivity;
using SessionPresentation::scheduled;
using SessionPresentation::sessionLabel;

// The roles a row's item can hold. Applying a row clears those it does not set,
// so a reused item reads exactly as a new one would.
constexpr int DigestRole = SessionRoles::Digest;
QList<int> rowRoles()
{
    QList<int> roles{Qt::ToolTipRole, Qt::AccessibleTextRole, Qt::AccessibleDescriptionRole};
    for (int role = Qt::UserRole; role < DigestRole; ++role) roles.append(role);
    return roles;
}

QByteArray rowDigest(const SessionsWindow::PreparedRow &row)
{
    QByteArray bytes;
    QDataStream stream(&bytes, QIODevice::WriteOnly);
    stream << row.key << int(row.kind) << row.flags.toInt() << row.hidden << row.roles;
    return QCryptographicHash::hash(bytes, QCryptographicHash::Sha1);
}

bool terminating(const SessionsWindow::Entry &entry, const SessionsWindow::RowInputs &inputs)
{
    const auto it = inputs.terminating.constFind(entry.key);
    return it != inputs.terminating.cend() && it->session.runId == entry.session.runId && it->session.created == entry.session.created;
}

QColor machineColor(const SessionsWindow::RowInputs &inputs, const QString &machine)
{
    const auto it = inputs.machineColors.constFind(machine);
    return it != inputs.machineColors.cend() ? *it : MachineAppearance::paletteColor(machine);
}

}

void SessionsWindow::setRowPreparationForTests(RowPreparation mode) { s_rowPreparation = mode; }

SessionsWindow::RowInputs SessionsWindow::rowInputs() const
{
    // Copies only: containers are implicitly shared, so this costs little.
    RowInputs inputs;
    inputs.entries = m_entries;
    inputs.fleet = m_fleet;
    inputs.organization = m_organization;
    inputs.rowPreferences = m_sessionRowPreferences;
    inputs.selection = rowSelection();
    inputs.readSelectionKey = m_readSelectionKey;
    inputs.creationSessionToSelect = m_creationSessionToSelect;
    inputs.revealCreation = m_revealCreation;
    inputs.drafts = m_composer->draftKeys();
    inputs.childDrafts = m_subagentComposer->draftKeys();
    inputs.expanded = m_expandedSessions;
    inputs.filter = m_filter;
    inputs.hostFilters = m_hostFilters; inputs.projectFilters = m_projectFilters; inputs.accountFilters = m_accountFilters;
    inputs.folderHost = m_folderFilterHost; inputs.folderPath = m_folderFilterPath; inputs.folderCheckout = m_folderFilterCheckout;
    std::tie(inputs.archiveFrom, inputs.archiveUntil) = m_archiveBrowser->includedRange();
    inputs.pendingLaunches = m_pendingLaunches;
    for (auto it = m_pendingLaunches.begin(); it != m_pendingLaunches.end(); ++it)
        if (m_creations->owns(it.key())) inputs.ownedCreations.insert(it.key());
    QSettings settings;
    inputs.includeCompleted = settings.value("workspace/showCompletedSubagents", false).toBool();
    inputs.hideEmptyProjects = settings.value("workspace/hideEmptyProjects", true).toBool();
    inputs.terminating = m_terminating;
    inputs.gitStatus = m_client.gitStatusSnapshots();
    // Every row's machine is this computer, a peer or a launch's host; their
    // colors come from the GUI thread's cache and settings.
    const auto addColor = [&inputs](const QString &machine) {
        if (!machine.isEmpty() && !inputs.machineColors.contains(machine)) inputs.machineColors.insert(machine, MachineAppearance::color(machine));
    };
    addColor(m_fleet.local().host);
    for (const auto &peer : m_fleet.peerNames()) addColor(peer);
    for (const auto &value : m_pendingLaunches) addColor(value.toObject().value("host").toString());
    inputs.accountProfiles = m_sessionAccountProfiles;
    inputs.accountDetailsProfiles = m_sessionAccountDetailsProfiles;
    inputs.tooltipFont = QToolTip::font();
    const int availableWidth = m_sessions->screen() ? m_sessions->screen()->availableGeometry().width() : 460;
    inputs.accessibleWidth = qBound(160, availableWidth - 40, 420);
    inputs.nowSecs = QDateTime::currentSecsSinceEpoch();
    // WhiteWh fork: a team's state, whether a member shows a problem (a folded
    // team keeps only those) and each lead's strip need the live sessions.
    if (m_teams) {
        inputs.teams = m_teams->teams();
        for (const auto &team : std::as_const(inputs.teams)) {
            if (team.status == "archived") continue;
            for (const auto &member : team.members)
                if (Teams::memberStatus(team, member, teamLive(member)).problem) inputs.teamProblems.insert(team.id + '\n' + member.id);
            inputs.teamStrips.insert(team.id, teamStrip(team));
        }
    }
    inputs.collapsedTeams = m_collapsedTeams;
    inputs.sessionLeads = SessionLeads::all();
    for (const auto &entry : std::as_const(m_entries))
        if (const int hours = CacheKeepalive::hours(entry.host, entry.session)) inputs.keepWarmHours.insert(entry.key, hours);
    return inputs;
}

QVariantMap SessionsWindow::teamStrip(const Teams::Team &team) const
{
    const auto teams = m_teams ? m_teams->teams() : QList<Teams::Team>{};
    const auto teamById = [&](const QString &id) -> const Teams::Team * {
        for (const auto &candidate : teams) if (candidate.id == id) return &candidate;
        return nullptr;
    };
    // G1: a parent's row rolls up its whole subtree (sessions and open tasks),
    // not only its own members.
    int subtreeSessions = 0, subtreeOpen = 0;
    QSet<QString> seenSessions, seenTeams;
    std::function<void(const Teams::Team &)> walk = [&](const Teams::Team &current) {
        if (seenTeams.contains(current.id) || seenTeams.size() > 64) return;
        seenTeams.insert(current.id);
        for (const auto &member : current.members)
            if (!member.session.isEmpty() && !seenSessions.contains(member.session)) { seenSessions.insert(member.session); ++subtreeSessions; }
        for (const auto &task : current.tasks) if (!task.finished()) ++subtreeOpen;
        for (const auto &child : current.children) if (const auto *found = teamById(child.team)) walk(*found);
    };
    walk(team);
    const auto live = [this](const Teams::Member &member) { return teamLive(member); };
    const QString status = Teams::displayStatus(team, live);
    // G6: two compact bars - the phase in work (or the next draft) and the
    // phases - replace one bar over every task of every phase.
    const auto bars = Teams::phaseProgress(team);
    const QString rollup = team.children.isEmpty() ? QString()
        : tr("%1 sessions \u00b7 %2 open").arg(subtreeSessions).arg(subtreeOpen);
    return {{"id", team.id}, {"name", team.name}, {"status", status}, {"label", Teams::statusLabel(status)},
            {"tone", int(Teams::statusTone(status))},
            {"phaseText", bars.phaseText()}, {"phasesText", bars.phasesText()},
            {"phaseDone", bars.done}, {"phaseTotal", bars.total},
            {"phasesDone", bars.phasesDone}, {"phasesTotal", bars.phasesTotal},
            {"phaseDraft", bars.draft}, {"phaseNumber", bars.phase},
            {"progressTooltip", bars.tooltip()},
            {"rollup", rollup}, {"openTasks", subtreeOpen}, {"subtreeSessions", subtreeSessions},
            {"members", QString::number(team.members.size())}, {"answers", Teams::answers(team, live)}};
}

void SessionsWindow::requestRows()
{
    if (s_rowPreparation == RowPreparation::Blocking) {
        m_rowRequestQueued = false;
        auto inputs = rowInputs(); inputs.generation = ++m_rowGeneration;
        PreparedRows prepared;
        ProcessRunner::prepareAndWait([&inputs, &prepared] { prepared = prepareRows(inputs); });
        m_rowsPending = false;
        applyRows(prepared);
        return;
    }
    // Several rebuilds in one pass of the event loop prepare their rows once.
    if (m_rowRequestQueued) return;
    m_rowRequestQueued = true;
    QMetaObject::invokeMethod(this, [this] { if (m_rowRequestQueued) { m_rowRequestQueued = false; postRows(); } }, Qt::QueuedConnection);
}

void SessionsWindow::postRows()
{
    auto inputs = rowInputs(); inputs.generation = ++m_rowGeneration;
    m_rowsPending = true;
    ProcessRunner::post(this, [inputs = std::move(inputs)] { return prepareRows(inputs); },
        [this](const PreparedRows &prepared) {
            // A newer request supersedes this one; its own result follows.
            if (prepared.generation != m_rowGeneration) { ++m_droppedRowPreparations; return; }
            m_rowsPending = false;
            applyRows(prepared);
        });
}

QMap<int, QVariant> SessionsWindow::sessionCardRoles(const Entry &e, const RowInputs &inputs)
{
    QMap<int, QVariant> roles;
    const auto &s = e.session;
    const bool ending = terminating(e, inputs);
    const QString rowStatus = ending ? tr("Terminating…") : status(s, e.online);
    QString desc = ending ? tr("Waiting for the session to stop") : currentAction(s, e.online).simplified();
    // U22/N17: an owner warning about DeepSeek's session-log upload owns line
    // three; an activity excerpt never hides it, because the block's promise is
    // what the owner needs to see.
    const bool uploadWarning = !s.uploadNote.isEmpty();
    if (uploadWarning) desc = s.uploadNote;
    roles.insert(SessionRoles::Model, s.model.isEmpty() ? (s.cmd == "sh" ? QString() : tr("Model unknown")) : s.model);
    roles.insert(SessionRoles::Effort, s.effort.isEmpty() ? QStringLiteral("—") : s.effort);
    roles.insert(SessionRoles::Title, sessionLabel(s)); roles.insert(SessionRoles::Agent, s.cmd);
    // F4: the Team designer keeps its fixed name, whatever hgs called the session.
    const bool designer = Teams::isDesignerSession(e.host, s.name);
    if (designer) roles.insert(SessionRoles::Title, Teams::designerTitle());
    // WhiteWh fork: a team member's card names its role (its id where the
    // role repeats), not the team's long session name.
    for (const auto &team : inputs.teams) {
        const auto *member = team.status == "archived" ? nullptr : team.memberOfSession(e.host, s.name);
        if (!member || member->lead) continue;
        const QString role = team.roleOf(member->id);
        const bool repeated = std::count_if(team.members.cbegin(), team.members.cend(), [&](const auto &m) { return team.roleOf(m.id) == role; }) > 1;
        roles.insert(SessionRoles::Title, repeated || role.isEmpty() ? member->id : role);
        break;
    }
    QString meta = SessionPresentation::checkoutLine(s);
    roles.insert(SessionRoles::MetaIcon, SessionPresentation::checkoutIcon(s));
    if (s.state == "archived" && s.archivedAt > 0) {
        if (!meta.isEmpty()) meta += QStringLiteral(" / ");
        meta += QDateTime::fromSecsSinceEpoch(qint64(s.archivedAt)).toLocalTime().toString("d MMM HH:mm");
    }
    roles.insert(SessionRoles::Meta, meta); roles.insert(SessionRoles::Status, rowStatus); roles.insert(SessionRoles::Detail, desc);
    if (uploadWarning) roles.insert(SessionRoles::UploadWarning, true);
    const auto gitPath = SessionPresentation::gitStatusPath(s);
    const auto git = SessionPresentation::gitStatus(s, inputs.gitStatus.value(e.host + '\n' + gitPath), e.online);
    roles.insert(SessionRoles::GitHost, e.host); roles.insert(SessionRoles::GitPath, gitPath);
    roles.insert(SessionRoles::GitStatus, git);
    roles.insert(Qt::AccessibleDescriptionRole, GitStatusBadge::tooltip(git));
    roles.insert(SessionRoles::Attention, !ending && (s.reviewLater || (e.online && !s.attentionAcknowledged
        && (s.needsAction() || SessionRows::childNeedsAction(e, inputs.selection)))));
    roles.insert(SessionRoles::ReviewLater, s.reviewLater);
    roles.insert(SessionRoles::Muted, s.muted);
    roles.insert(SessionRoles::Failure, s.phase=="error");
    roles.insert(SessionRoles::Draft, inputs.drafts.contains(e.key));
    roles.insert(SessionRoles::Unread, !ending && s.visibleUnreadReply(e.online));
    roles.insert(SessionRoles::Working, !ending && SessionPresentation::working(s, e.online));
    roles.insert(SessionRoles::WorkingSince, SessionPresentation::backgroundIdle(s) ? s.backgroundStarted : s.phase == "compacting" ? s.compactionStarted : s.turnStarted);
    roles.insert(SessionRoles::Scheduled, !ending && scheduled(s, e.online));
    roles.insert(SessionRoles::CoordinationWait, !ending && currentActivity(s, e.online) && s.hasCoordinationWait() ? s.coordinationWait : QJsonObject());
    roles.insert(SessionRoles::Waiting, !ending && e.online && SessionPresentation::waitingIdle(s));
    roles.insert(SessionRoles::OptionalQuestions, ending ? 0 : SessionPresentation::optionalQuestionCount(s, e.online));
    // G7: the services the session runs, apart from Background work.
    QVariantList services;
    for (const auto &service : s.services)
        services.append(QVariantMap{{"name", service.name}, {"command", service.command}, {"since", service.since}});
    roles.insert(SessionRoles::Services, services);
    // U25 (spec 2.2): only confirmed current commands of a live session.
    const bool jobs = !ending && currentActivity(s, e.online) && s.backgroundProcesses > 0;
    roles.insert(SessionRoles::BackgroundJobs, jobs ? s.backgroundProcesses : 0);
    roles.insert(SessionRoles::BackgroundCommand, jobs ? s.backgroundCommand : QString());
    roles.insert(SessionRoles::TeamMuted, s.muted && s.teamMuted);
    roles.insert(SessionRoles::MachineName, e.machine);
    roles.insert(SessionRoles::MachineColor, machineColor(inputs, e.machine));
    roles.insert(SessionRoles::Host, e.machine + (e.online ? QString() : tr(" (offline)")));
    roles.insert(SessionRoles::Children, childCount(s, false, e.online, inputs.includeCompleted));
    if (const auto *group = inputs.organization.group(inputs.organization.groupFor(e.identity))) {
        roles.insert(SessionRoles::ProjectColor, QColor(group->color));
        roles.insert(SessionRoles::ProjectVivid, group->vivid);
    }
    // F4: and its own colour, so nobody takes it for a work session.
    if (designer) { roles.insert(SessionRoles::ProjectColor, QColor(Teams::designerColor())); roles.insert(SessionRoles::ProjectVivid, true); }
    // U30 (#508): the session's state; Keep cache warm counts its hours.
    roles.insert(SessionRoles::SessionState, SessionStates::facts(s, e.online, inputs.keepWarmHours.value(e.key) * 3600.0));
    return roles;
}

QMap<int, QVariant> SessionsWindow::childCardRoles(const Entry &e, const QString &id, const QJsonObject &child, const RowInputs &inputs)
{
    const auto &s = e.session;
    return {{SessionRoles::Title, SessionRows::childName(child, id)}, {SessionRoles::Agent, child.value("provider").toString(s.cmd)},
        {SessionRoles::Status, currentActivity(s, e.online) ? SessionPresentation::childStatus(child) : tr("Recorded")},
        {SessionRoles::Detail, child.value("detail").toString(child.value("task").toString()).simplified()},
        {SessionRoles::Working, !terminating(e, inputs) && currentActivity(s, e.online) && SessionPresentation::childState(child) == "working"},
        {SessionRoles::Attention, currentActivity(s, e.online) && SessionRows::childAction(child)},
        {SessionRoles::Unread, false}, {SessionRoles::Muted, s.muted}, {SessionRoles::ChildId, id},
        {SessionRoles::MachineName, e.machine}, {SessionRoles::MachineColor, machineColor(inputs, e.machine)}, {SessionRoles::Host, e.machine}};
}

SessionsWindow::PreparedRows SessionsWindow::prepareRows(const RowInputs &in)
{
    JsonParse::reportGuiThreadPreparation("Sessions rows prepared on the GUI thread");
    PreparedRows out;
    out.generation = in.generation;
    const auto &selection = in.selection;
    const auto &organization = in.organization;
    const auto messageDraft = [&in](const QString &key) { return in.drafts.contains(key); };
    const auto childDraft = [&in](const QString &key) { return in.childDrafts.contains(key); };
    const auto needsAttention = [&selection](const Entry &entry) { return SessionRows::needsAttention(entry, selection); };
    // WhiteWh fork: a Windows peer's folders follow its own path style.
    const auto style = [&in](const QString &host) { return MachinePlatform::of(in.fleet.metricsOf(host), host.isEmpty()); };
    const auto inFolder = [&in, &style](const Entry &entry) {
        const auto &s = entry.session;
        return entry.host == in.folderHost && MachinePlatform::same(in.folderCheckout ? s.gitRoot : s.canonicalCwd, in.folderPath, style(entry.host));
    };
    // Sessions with an unsent draft, their own or a subagent's, count as Active.
    for (const auto &entry : in.entries)
        if (SessionRows::hasDraft(entry, selection, messageDraft, childDraft)) out.drafts.insert(entry.key);
    const auto &drafts = out.drafts;

    SessionAccounts::Catalog accounts;
    accounts.reset(in.accountProfiles, in.accountDetailsProfiles);
    for (const auto &entry : in.entries) accounts.observe(entry.host, entry.machine, entry.session);
    out.accounts = accounts.catalogAccounts();

    int attention = 0, saved = 0, archived = 0, total = 0, active = 0;
    for (const auto &entry : in.entries) {
        if (!in.projectFilters.isEmpty() && !in.projectFilters.contains(organization.groupFor(entry.identity))) continue;
        out.projectKeys.insert(entry.key);
        if (!in.hostFilters.isEmpty() && !in.hostFilters.contains(entry.host.isEmpty() ? "@local" : entry.host)) continue;
        const auto &s = entry.session;
        if (!SessionAccounts::accepts(in.accountFilters, entry.host, s)) continue;
        if (!in.folderPath.isEmpty() && !inFolder(entry)) continue;
        if (s.state == "archived") { ++archived; continue; }
        ++total;
        const bool needs = needsAttention(entry);
        if (needs && !s.muted) ++attention;
        if (needs || SessionPresentation::active(s, entry.online, drafts.contains(entry.key))) ++active;
        if (s.state != "running") ++saved;
    }
    out.counts = {{"all", total}, {"active", active}, {"attention", attention}, {"paused", saved}, {"archived", archived}};
    out.unreadReplies = in.fleet.unreadReplies().size();
    for (auto it = in.pendingLaunches.begin(); it != in.pendingLaunches.end(); ++it) {
        const auto value = it.value().toObject();
        const auto group = organization.group(value["group"].toString()) ? value["group"].toString() : organization.defaultProject();
        if (value["creation_schema"] == 1 && (in.projectFilters.isEmpty() || in.projectFilters.contains(group))) out.creationScope[it.key()] = value;
    }

    // Text searches have a separate result list; these rows follow the filters.
    QList<const Entry *> visible;
    for (const auto &e : in.entries) {
        const auto &s = e.session;
        if (e.key == in.creationSessionToSelect && s.state != "archived") { visible.append(&e); continue; }
        if ((s.state == "archived") != (in.filter == "archived")) continue;
        if (s.state == "archived" && !(s.archivedAt >= in.archiveFrom && s.archivedAt < in.archiveUntil)) continue;
        // The open conversation stays while its reply was just read.
        const bool keptOpen = e.key == selection.key && e.key == in.readSelectionKey;
        if (in.filter == "attention" && !needsAttention(e) && !keptOpen) continue;
        if (in.filter == "active" && !needsAttention(e) && !SessionPresentation::active(s, e.online, drafts.contains(e.key)) && !keptOpen) continue;
        if (in.filter == "paused" && s.state == "running") continue;
        if (!in.folderPath.isEmpty() && !inFolder(e)) continue;
        const QString machine = e.host.isEmpty() ? QStringLiteral("@local") : e.host;
        const auto project = organization.groupFor(e.identity);
        const bool accountMatches = SessionAccounts::accepts(in.accountFilters, e.host, s);
        if ((in.hostFilters.isEmpty() || in.hostFilters.contains(machine)) && accountMatches) ++out.projectCounts[project];
        if (!out.projectKeys.contains(e.key)) continue;
        if (accountMatches) ++out.machineCounts[machine];
        if (!in.hostFilters.isEmpty() && !in.hostFilters.contains(machine)) continue;
        ++out.accountCounts[SessionAccounts::binding(e.host, s)];
        if (!accountMatches) continue;
        visible.append(&e);
    }

    struct Row { QString group; int entry = -1; bool hidden = false; QString child, launch; };
    QList<Row> rows;
    const auto &groups = organization.groups();
    const bool hasGroups = groups.size() > 1;
    const bool filtering = !in.accountFilters.isEmpty() || !in.projectFilters.isEmpty() || !in.folderPath.isEmpty() || !in.hostFilters.isEmpty() || in.filter != "all";
    QStringList visibleIdentities;
    visibleIdentities.reserve(visible.size());
    for (const auto *entry : visible) visibleIdentities.append(entry->identity);
    auto projectMembers = organization.orderedMembers(visibleIdentities);
    // WhiteWh fork: agent teams. A team's members stand under its lead, in
    // whichever project they were filed. G1: parents come before their
    // sub-teams, so a sub-team lead is indented under the lead it works for.
    QSet<int> subordinateRows;   // visible entries listed under their lead
    QSet<int> teamMemberRows, teamHiddenRows, lastMemberRows;
    QHash<int, int> teamDepth;
    QHash<int, QVariantList> teamLines;
    QHash<int, QString> leadTeams;   // lead rows and their teams
    QList<Teams::Team> orderedTeams = in.teams;
    const auto teamById = [&](const QString &id) -> const Teams::Team * {
        for (const auto &team : orderedTeams) if (team.id == id) return &team;
        return nullptr;
    };
    const auto depthOf = [&](const Teams::Team &team) {
        int depth = 0; const Teams::Team *at = &team;
        QSet<QString> seen;
        while (at && at->parent.known && depth < 32 && !seen.contains(at->id)) {
            seen.insert(at->id);
            at = teamById(at->parent.team);
            if (!at) break;
            ++depth;
        }
        return depth;
    };
    std::stable_sort(orderedTeams.begin(), orderedTeams.end(), [&](const Teams::Team &a, const Teams::Team &b) {
        return depthOf(a) < depthOf(b);
    });
    const auto collapsedDeep = [&](const Teams::Team &team) {
        const Teams::Team *at = &team; int guard = 0;
        while (at && guard++ < 32) {
            if (in.collapsedTeams.contains(at->id)) return true;
            at = at->parent.known ? teamById(at->parent.team) : nullptr;
        }
        return false;
    };
    if (!orderedTeams.isEmpty()) {
        QHash<int, QString> groupOf;
        for (auto it = projectMembers.cbegin(); it != projectMembers.cend(); ++it) for (int i : it.value()) groupOf.insert(i, it.key());
        for (const auto &team : orderedTeams) {
            if (team.status == "archived") continue;
            int lead = -1; QList<int> follow;
            for (int i = 0; i < visible.size(); ++i) {
                const auto *member = team.memberOfSession(visible[i]->host, visible[i]->session.name);
                if (!member) continue;
                if (member->lead) lead = i; else follow.append(i);
            }
            if (lead < 0 || !groupOf.contains(lead)) continue;
            const QString target = groupOf.value(lead);
            for (int i : follow) {
                const QString from = groupOf.value(i);
                if (from.isEmpty() || from == target) continue;
                projectMembers[from].removeAll(i); projectMembers[target].append(i); groupOf[i] = target;
            }
        }
    }
    const auto &leads = in.sessionLeads;
    QString adoptedGroup;
    for (const auto *entry : visible) if (entry->key == in.creationSessionToSelect) { adoptedGroup = organization.groupFor(entry->identity); break; }
    const auto revealed = in.pendingLaunches.value(in.revealCreation).toObject();
    const auto revealedGroup = organization.group(revealed["group"].toString()) ? revealed["group"].toString() : organization.defaultProject();
    for (const auto &group : groups) {
        if (!in.projectFilters.isEmpty() && !in.projectFilters.contains(group.id) && adoptedGroup != group.id && (!revealed["show_pending"].toBool() || group.id != revealedGroup)) continue;
        auto members = projectMembers.value(group.id);
        QStringList launches;
        for (auto it = in.pendingLaunches.begin(); it != in.pendingLaunches.end(); ++it) {
            const auto launch = it.value().toObject();
            const bool creation = launch["creation_schema"] == 1, reveal = it.key() == in.revealCreation;
            if (!reveal && !(in.filter == "all" || (creation && in.filter == "active"))) continue;
            if (!reveal && !in.folderPath.isEmpty() && (!creation || launch["host"].toString() != in.folderHost
                || !MachinePlatform::same(launch["target"].toString(), in.folderPath, style(in.folderHost)))) continue;
            SessionInfo pendingAccount; pendingAccount.cmd = launch["cmd"].toString(); pendingAccount.accountId = launch["account"].toString();
            if (!reveal && !SessionAccounts::accepts(in.accountFilters, launch["host"].toString(), pendingAccount)) continue;
            // Older versions kept mapping receipts for a week. Those abandoned
            // intents must not suddenly become visible launches after an upgrade.
            if (!launch.value("show_pending").toBool()) continue;
            const QString requested = launch.value("group").toString();
            const QString destination = organization.group(requested) ? requested : organization.defaultProject();
            const QString host = launch.value("host").toString();
            if (destination == group.id && (reveal || in.hostFilters.isEmpty() || in.hostFilters.contains(host.isEmpty() ? "@local" : host))) launches.append(it.key());
        }
        if (!leads.isEmpty()) {
            // WhiteWh fork: a subordinate follows its lead when both are listed
            // here and the lead reports to no one. One pass: a project can list
            // thousands of sessions.
            QHash<QString, int> at;
            for (int i : members) if (!at.contains(visible[i]->identity)) at.insert(visible[i]->identity, i);
            QHash<int, QList<int>> followers;
            QSet<int> following;
            for (int i : members) {
                const auto lead = leads.value(visible[i]->identity).toString();
                const int j = lead.isEmpty() ? -1 : at.value(lead, -1);
                if (j < 0 || j == i || !leads.value(lead).toString().isEmpty()) continue;
                followers[j].append(i); following.insert(i);
            }
            if (!following.isEmpty()) {
                QList<int> ordered;
                for (int i : members) {
                    if (following.contains(i)) continue;
                    ordered.append(i);
                    for (int j : followers.value(i)) { ordered.append(j); subordinateRows.insert(j); }
                }
                members = ordered;
            }
        }
        // WhiteWh fork: agent teams. A team's members listed here follow its
        // lead, indented; the lead's card carries the team. A folded team
        // keeps only members with a problem.
        for (const auto &team : orderedTeams) {
            if (team.status == "archived") continue;
            int lead = -1; QList<int> follow;
            for (int i : members) {
                const auto *member = team.memberOfSession(visible[i]->host, visible[i]->session.name);
                if (!member) continue;
                if (member->lead) lead = i; else follow.append(i);
            }
            if (lead < 0 || leadTeams.contains(lead)) continue;
            // G1: a sub-team's lead sits indented under its parent's lead.
            if (team.parent.known) subordinateRows.insert(lead);
            const bool collapsed = collapsedDeep(team);
            // Spec 2.5: the members follow the team's tree - each sub-lead
            // followed by its reports - over the rows still shown.
            QHash<QString, int> rowOf;
            for (int i : follow) rowOf.insert(team.memberOfSession(visible[i]->host, visible[i]->session.name)->id, i);
            for (int i : follow) {
                const auto *member = team.memberOfSession(visible[i]->host, visible[i]->session.name);
                if (collapsed && !in.teamProblems.contains(team.id + '\n' + member->id)) teamHiddenRows.insert(i);
            }
            const auto tree = team.memberTree([&](const QString &id) { return rowOf.contains(id) && !teamHiddenRows.contains(rowOf.value(id)); });
            QList<int> treeOrder;
            for (const auto &node : tree) {
                const int i = rowOf.value(node.member->id, -1);
                if (i < 0 || node.depth < 1) continue;
                treeOrder.append(i);
                teamDepth.insert(i, node.depth);
                QVariantList lines; for (bool on : node.through) lines.append(on);
                teamLines.insert(i, lines);
                if (node.last) lastMemberRows.insert(i);
            }
            for (int i : follow) if (!treeOrder.contains(i)) treeOrder.append(i);
            follow = treeOrder;
            const QSet<int> following(follow.cbegin(), follow.cend());
            QList<int> ordered;
            for (int i : members) {
                if (following.contains(i)) continue;
                ordered.append(i);
                if (i == lead) ordered += follow;
            }
            members = ordered;
            leadTeams.insert(lead, team.id);
            for (int i : follow) teamMemberRows.insert(i);
        }
        if (members.isEmpty() && launches.isEmpty() && (filtering || in.hideEmptyProjects || !group.accessible)) continue;
        if (hasGroups) rows.append({group.id, -1, false});
        for (int i : members) {
            const bool hidden = hasGroups && group.collapsed;
            rows.append({group.id, i, hidden || teamHiddenRows.contains(i), {}});
            if (in.expanded.contains(visible[i]->key)) {
                const auto roster = SessionRows::roster(*visible[i], selection);
                // Stable ID order avoids moving a row under the pointer when a child finishes.
                for (auto it = roster.begin(); it != roster.end(); ++it)
                    if (SessionRows::showChild(*visible[i], it.key(), it.value().toObject(), visible[i]->session.subagentCountsComplete,
                                               in.includeCompleted, selection, childDraft))
                        rows.append({group.id, i, hidden, it.key()});
            }
        }
        for (const auto &id : launches) rows.append({group.id, -1, hasGroups && group.collapsed, {}, id});
    }

    const auto rowKey = [&](const Row &row) {
        return !row.launch.isEmpty() ? "#launch\n" + row.launch : row.entry < 0 ? "#group\n" + row.group
            : visible[row.entry]->key + (row.child.isEmpty() ? QString() : "\n#child\n" + row.child);
    };
    QSet<QString> keys;
    out.rows.reserve(rows.size());
    int starting = 0, creating = 0;
    for (int i = 0; i < rows.size(); ++i) {
        const auto &row = rows[i];
        PreparedRow prepared;
        auto &r = prepared.roles;
        prepared.key = rowKey(row);
        out.uniqueKeys = out.uniqueKeys && !keys.contains(prepared.key);
        keys.insert(prepared.key);
        prepared.child = row.child; prepared.launch = row.launch;
        prepared.parentKey = row.entry >= 0 ? visible[row.entry]->key : QString();
        prepared.hidden = row.hidden;
        // Session cards already expose their status; details belong in Activity
        // and Details, not a hover popup covering neighbouring rows.
        r[Qt::ToolTipRole] = QString();
        r[SessionRoles::Key] = prepared.key; r[SessionRoles::Group] = row.group;
        r[SessionRoles::ChildId] = row.child;
        r[SessionRoles::ParentKey] = prepared.parentKey;
        r[SessionRoles::LaunchId] = row.launch;
        r[SessionRoles::WorkingSince] = 0.0;
        r[SessionRoles::Scheduled] = false;
        r[SessionRoles::OptionalQuestions] = 0;
        r[SessionRoles::RowCollapsed] = false;
        r[SessionRoles::GitStatus] = QJsonObject();
        r[SessionRoles::MetaIcon] = QString();
        r[SessionRoles::GitHost] = QString(); r[SessionRoles::GitPath] = QString();
        r[Qt::AccessibleDescriptionRole] = QString();
        r[SessionRoles::Header] = row.entry < 0 && row.launch.isEmpty();
        // A live session takes tasks of its project dragged onto it.
        const bool live = row.entry >= 0 && row.child.isEmpty() && row.launch.isEmpty() && visible[row.entry]->session.state != "archived";
        r[SessionRoles::TaskTarget] = live ? QStringList{visible[row.entry]->host, visible[row.entry]->session.name} : QStringList();
        // WhiteWh fork: a session's place under its lead and in its team's tree.
        const bool sessionRow = row.entry >= 0 && row.child.isEmpty() && row.launch.isEmpty();
        r[SessionRoles::Subordinate] = sessionRow && subordinateRows.contains(row.entry);
        r[SessionRoles::TeamMember] = sessionRow && teamMemberRows.contains(row.entry) ? (lastMemberRows.contains(row.entry) ? 2 : 1) : 0;
        r[SessionRoles::TeamDepth] = row.entry >= 0 && row.child.isEmpty() ? teamDepth.value(row.entry, 1) : 0;
        r[SessionRoles::TeamLines] = row.entry >= 0 && row.child.isEmpty() ? teamLines.value(row.entry) : QVariantList();
        if (!row.launch.isEmpty()) {
            prepared.kind = PreparedRow::Launch;
            const auto launch = in.pendingLaunches.value(row.launch).toObject();
            const QString name = launch.value("tag").toString(), host = launch.value("host").toString();
            const QString machine = host.isEmpty() ? in.fleet.local().host : host;
            const bool waiting = in.nowSecs - launch.value("created").toInteger() > 120;
            const bool creation = launch["creation_schema"] == 1;
            const QString state = creation ? SessionCreationController::caption(launch) : waiting ? tr("Waiting for session") : tr("Starting…");
            const QString detail = creation ? launch["error"].toString(tr("Preparing the session in the background"))
                : waiting ? tr("Check the terminal for startup progress") : tr("Waiting for the session to appear");
            prepared.flags = creation ? Qt::ItemIsEnabled | Qt::ItemIsSelectable : Qt::ItemIsEnabled;
            r[SessionRoles::Title] = name; r[SessionRoles::Status] = state; r[SessionRoles::Detail] = detail;
            r[SessionRoles::Meta] = launch.value("target").toString(); r[SessionRoles::Agent] = launch.value("cmd").toString();
            r[SessionRoles::Host] = machine; r[SessionRoles::MachineName] = machine; r[SessionRoles::MachineColor] = machineColor(in, machine);
            r[Qt::AccessibleTextRole] = QString("%1, %2, %3").arg(name, machine, state);
            if (in.ownedCreations.contains(row.launch)) ++creating; else ++starting;
        } else if (row.entry < 0) {
            prepared.kind = PreparedRow::Header;
            const auto *group = organization.group(row.group);
            int total = 0, alerts = 0, busy = 0, unread = 0;
            for (const auto *e : visible) if (organization.groupFor(e->identity) == row.group) ++total;
            for (const auto &candidate : rows) if (candidate.group == row.group && !candidate.launch.isEmpty()) ++total;
            // Attention remains visible even when a search or state filter hides a member.
            for (const auto &e : in.entries) if (organization.groupFor(e.identity) == row.group) {
                if (!e.session.muted && (e.session.reviewLater || (e.online && !e.session.attentionAcknowledged
                    && (e.session.needsAction() || SessionRows::childNeedsAction(e, selection))))) ++alerts;
                if (!e.session.muted && e.session.visibleUnreadReply(e.online)) ++unread;
                if (SessionPresentation::working(e.session, e.online)) ++busy;
            }
            prepared.flags = Qt::ItemIsEnabled | Qt::ItemIsDropEnabled | Qt::ItemIsDragEnabled;
            r[SessionRoles::Title] = group->name;
            r[SessionRoles::ProjectColor] = QColor(group->color);
            r[SessionRoles::ProjectVivid] = group->vivid;
            r[SessionRoles::Collapsed] = group->collapsed;
            r[SessionRoles::Total] = total; r[SessionRoles::Attention] = alerts; r[SessionRoles::Working] = busy; r[SessionRoles::Unread] = unread;
            r[SessionRoles::ProjectFolder] = group->folders.isEmpty() ? QString() : QDir::toNativeSeparators(group->folders.first().path);
            const QString description = tr("%1: %2 shown, %3 need a response, %4 working, %5 unread replies. Click to collapse or expand; drag to reorder.")
                .arg(group->name).arg(total).arg(alerts).arg(busy).arg(unread);
            r[Qt::ToolTipRole] = sessionTooltip({description}, 330, in.tooltipFont).html; r[Qt::AccessibleTextRole] = description;
        } else if (!row.child.isEmpty()) {
            prepared.kind = PreparedRow::Child;
            const auto &e = *visible[row.entry];
            const auto child = SessionRows::roster(e, selection).value(row.child).toObject();
            prepared.flags = Qt::ItemIsEnabled | Qt::ItemIsSelectable;
            const auto roles = childCardRoles(e, row.child, child, in);
            for (auto it = roles.cbegin(); it != roles.cend(); ++it) r[it.key()] = it.value();
            r[Qt::AccessibleTextRole] = tr("Subagent %1, %2").arg(SessionRows::childName(child, row.child), SessionPresentation::childStatus(child));
        } else {
            prepared.kind = PreparedRow::Session;
            const auto &e = *visible[row.entry]; const auto &s = e.session;
            const auto roster = SessionRows::roster(e, selection);
            bool hasChildren = false;
            for (auto it = roster.begin(); it != roster.end(); ++it)
                if (SessionRows::showChild(e, it.key(), it.value().toObject(), s.subagentCountsComplete, in.includeCompleted, selection, childDraft)) {
                    hasChildren = true; break;
                }
            // WhiteWh fork: a team's lead folds its members as a session folds its subagents.
            const QString leadTeam = leadTeams.value(row.entry);
            const auto strip = leadTeam.isEmpty() ? QVariantMap() : in.teamStrips.value(leadTeam);
            if (!strip.isEmpty()) r[SessionRoles::TeamLead] = strip;   // a row keeps only valid roles
            r[SessionRoles::HasChildren] = hasChildren || !leadTeam.isEmpty();
            r[SessionRoles::Expanded] = leadTeam.isEmpty() ? in.expanded.contains(e.key) : !in.collapsedTeams.contains(leadTeam);
            if (out.firstSession < 0) out.firstSession = i;
            if (!row.hidden && out.firstVisible < 0) out.firstVisible = i;
            prepared.flags = Qt::ItemIsEnabled | Qt::ItemIsSelectable | Qt::ItemIsDragEnabled | Qt::ItemIsDropEnabled;
            r[SessionRoles::Identity] = e.identity;
            const bool collapsed = in.rowPreferences.collapsed(e.host, s);
            r[SessionRoles::RowCollapsed] = collapsed;
            const auto roles = sessionCardRoles(e, in);
            for (auto role = roles.cbegin(); role != roles.cend(); ++role) r[role.key()] = role.value();
            if (collapsed) r[Qt::AccessibleDescriptionRole] = tr("Collapsed session row. Press Right to expand.");
            const auto rowStatus = roles.value(SessionRoles::Status).toString(), desc = roles.value(SessionRoles::Detail).toString();
            const auto meta = roles.value(SessionRoles::Meta).toString(), gitPath = roles.value(SessionRoles::GitPath).toString();
            if (e.online && !gitPath.isEmpty()) out.gitReads.append({e.host, gitPath});
            QStringList accessibleFields{s.name, e.machine + QStringLiteral(" / ") + s.cmd + QStringLiteral(" / ") + rowStatus, meta};
            const double workingSince = r.value(SessionRoles::WorkingSince).toDouble();
            if (r.value(SessionRoles::Working).toBool() && workingSince > 0)
                accessibleFields << tr("%1 since %2").arg(SessionPresentation::backgroundIdle(s) ? tr("Background") : s.phase == "compacting" ? tr("Compacting") : tr("Working"),
                    QDateTime::fromSecsSinceEpoch(qint64(workingSince)).toLocalTime().toString("d MMM HH:mm:ss"));
            if (!s.cwd.isEmpty()) accessibleFields << s.cwd;
            accessibleFields << (s.model + (s.effort.isEmpty() ? QString() : "  " + s.effort));
            if (s.visibleUnreadReply(e.online)) accessibleFields << tr("New reply (not viewed yet)");
            const int optionalQuestions = roles.value(SessionRoles::OptionalQuestions).toInt();
            if (optionalQuestions > 0) accessibleFields << SessionPresentation::optionalQuestionsLabel(optionalQuestions);
            if (s.reviewLater) accessibleFields << tr("Marked needs attention: review later");
            // N36 of #499: the team default lives in this desktop only; hgs
            // keeps the session unmuted for Android and the other desktops.
            if (s.muted && s.teamMuted) accessibleFields << tr("Muted on this computer: team members are muted by default");
            else if (s.muted) accessibleFields << (s.sharedNotifications && e.online ? tr("Muted on all devices") : tr("Muted on all devices (last confirmed)"));
            accessibleFields << desc;
            if (!childCount(s, false, e.online, in.includeCompleted).isEmpty())
                accessibleFields << tr("Subagents: %1").arg(childCount(s, true, e.online, in.includeCompleted));
            if (!currentActivity(s, e.online) && !s.currentTool.isEmpty()) accessibleFields << tr("Last recorded: %1").arg(s.currentTool + " / " + s.toolDetail);
            if (s.gitMetadataState == "unavailable") accessibleFields << tr("Git information unavailable");
            r[Qt::AccessibleTextRole] = sessionTooltip(accessibleFields, in.accessibleWidth, in.tooltipFont).plain;
        }
        prepared.digest = rowDigest(prepared);
        out.rows.append(std::move(prepared));
    }

    int visibleWorking = 0, visibleAttention = 0;
    for (const auto *e : visible) {
        if (!e->session.muted && needsAttention(*e)) ++visibleAttention;
        if (SessionPresentation::working(e->session, e->online)) ++visibleWorking;
    }
    QString count = visible.isEmpty() ? tr("No matching sessions")
        : tr("%1 shown, %2 working, %3 attention").arg(visible.size()).arg(visibleWorking).arg(visibleAttention);
    if (starting) count = visible.isEmpty() ? tr("%1 starting").arg(starting) : count + tr(", %1 starting").arg(starting);
    if (creating) {
        const auto creations = creating == 1 ? tr("1 creation") : tr("%1 creations").arg(creating);
        count = visible.isEmpty() && !starting ? creations : count + ", " + creations;
    }
    out.countText = count;
    out.countShort = QString::number(visible.size() + starting + creating);
    return out;
}

void SessionsWindow::applyListRows(const PreparedRows &prepared)
{
    static const QList<int> universe = rowRoles();
    const auto write = [](QListWidgetItem *item, const PreparedRow &row, bool fresh) {
        for (auto it = row.roles.cbegin(); it != row.roles.cend(); ++it) item->setData(it.key(), it.value());
        if (!fresh) for (int role : universe)
            if (!row.roles.contains(role) && item->data(role).isValid()) item->setData(role, QVariant());
        if (item->flags() != row.flags) item->setFlags(row.flags);
        item->setData(DigestRole, row.digest);
    };
    const auto &rows = prepared.rows;
    if (!prepared.uniqueKeys) {
        // Rows cannot be matched by key: rebuild every item.
        m_sessions->clear();
        for (const auto &row : rows) {
            auto *item = new QListWidgetItem(m_sessions);
            write(item, row, true); item->setHidden(row.hidden);
        }
        return;
    }
    QHash<QString, int> wanted;
    wanted.reserve(rows.size());
    for (int i = 0; i < rows.size(); ++i) wanted.insert(rows[i].key, i);
    QHash<QString, QListWidgetItem *> items;
    for (int i = m_sessions->count() - 1; i >= 0; --i) {
        auto *item = m_sessions->item(i);
        const auto key = item->data(SessionRoles::Key).toString();
        if (!wanted.contains(key) || items.contains(key)) delete m_sessions->takeItem(i);
        else items.insert(key, item);
    }
    // Rows keep their items: only a changed digest writes roles, and a row
    // that moved is moved with its selection and persistent indexes.
    for (int i = 0; i < rows.size(); ++i) {
        const auto &row = rows[i];
        auto *item = items.value(row.key);
        if (!item) {
            item = new QListWidgetItem;
            write(item, row, true);
            m_sessions->insertItem(i, item);
            item->setHidden(row.hidden);
            continue;
        }
        const int at = m_sessions->row(item);
        if (at != i && !m_sessions->model()->moveRow(QModelIndex(), at, QModelIndex(), i)) {
            const bool hidden = item->isHidden();
            m_sessions->insertItem(i, m_sessions->takeItem(at)); item->setHidden(hidden);
        }
        if (item->data(DigestRole).toByteArray() != row.digest) write(item, row, false);
        if (item->isHidden() != row.hidden) item->setHidden(row.hidden);
    }
}

void SessionsWindow::applyRows(const PreparedRows &prepared)
{
    // A drag in progress keeps its rows; its end rebuilds them.
    if (m_sessions->dragging()) { ++m_droppedRowPreparations; return; }
    m_rebuilding = true;
    QSet<QString> selectedKeys;
    for (const auto *item : m_sessions->selectedItems()) selectedKeys.insert(item->data(SessionRoles::Key).toString());
    if (!m_sessions->currentItem() && selectedKeys.isEmpty() && !m_selectedKey.isEmpty()) selectedKeys.insert(m_selectedKey);
    const QString requestedSelection = m_rowSelection ? *m_rowSelection
        : m_selectedCreation.isEmpty() ? m_selectedKey : "#launch\n" + m_selectedCreation;
    m_rowSelection.reset();
    const bool creationPlaced = std::exchange(m_rowCreationPlaced, false);
    const auto &counts = prepared.counts;

    m_heading->setText(m_filter == "archived" ? tr("Archive") : tr("Sessions"));
    const int unreadCount = prepared.unreadReplies;
    m_markAllReadAction->setEnabled(unreadCount > 0);
    m_markAllReadAction->setText(unreadCount ? tr("Mark all as read (%1)").arg(unreadCount) : tr("Mark all as read"));
    m_markAllReadAction->setIcon(workspaceIcon("read-all", QColor(m_muted)));
    m_markAllReadButton->setEnabled(unreadCount > 0); m_markAllReadButton->setAccessibleName(m_markAllReadAction->text());
    m_markAllReadButton->setToolTip(m_markAllReadAction->text() + '\n' + m_markAllReadAction->toolTip());
    const int attention = counts.value("attention");
    QString countCaption;
    for (auto *button : m_filters) {
        const auto id = button->property("filter").toString(); button->setChecked(m_pages->currentIndex() == 0 && id == m_filter);
        if (id == m_filter) countCaption = button->property("caption").toString();
        const bool pagedArchive=id=="archived"&&m_archiveBrowser->paged();
        const bool partial=pagedArchive&&(!m_archiveBrowser->totalsKnown()||m_archiveBrowser->hasMore());
        button->setText(pagedArchive&&!m_archiveBrowser->totalsKnown()&&!counts.value(id)?QStringLiteral("…"):QString::number(counts.value(id))+(partial?QStringLiteral("+"):QString())); button->setProperty("count", counts.value(id));
        button->setToolTip(button->property("caption").toString() + (pagedArchive?tr(" / %1 loaded%2").arg(counts.value(id)).arg(partial?tr("; more can be loaded in Archive"):QString()):QString(" / %1").arg(counts.value(id))));
        button->setIcon(workspaceIcon(id, QColor(id == "attention" && attention > 0 ? (m_dark ? "#f0c77b" : "#885400") : id == m_filter ? m_accent : m_muted)));
    }
    m_sessionsNav->setChecked(m_pages->currentIndex() == 0);
    m_machinesNav->setChecked(m_pages->currentIndex() == 2);
    m_searchResults->setCreations(prepared.creationScope);
    m_searchResults->setAccountScope(m_accountFilters);
    m_searchResults->setProjectScope(prepared.projectKeys, !m_projectFilters.isEmpty());
    m_searchResults->setFolderScope(m_folderFilterHost,m_folderFilterPath,m_folderFilterCheckout);
    m_searchResults->setHostsScope(m_fleet, m_filter, m_hostFilters, prepared.drafts);
    m_search->setPlaceholderText(m_filter=="archived"?tr("Search archive names and IDs…"):tr("Search sessions, IDs and messages…"));
    if (!m_folderFilterPath.isEmpty()) countCaption += tr(" / selected folder");
    { const QSignalBlocker block(m_machineFilter); m_machineFilter->setSelection(m_hostFilters); m_machineFilter->setFleet(m_fleet);
        m_machineFilter->setSessionCounts(prepared.machineCounts, countCaption + (m_projectFilters.isEmpty() ? QString() : tr(" / selected projects")) + (m_accountFilters.isEmpty() ? QString() : tr(" / selected accounts"))); }
    { const QSignalBlocker block(m_projectFilter); m_projectFilter->setSelection(m_projectFilters); m_projectFilter->setProjects(m_organization.groups());
        m_projectFilter->setSessionCounts(prepared.projectCounts, countCaption + (m_hostFilters.isEmpty() ? QString() : tr(" / selected machines")) + (m_accountFilters.isEmpty() ? QString() : tr(" / selected accounts"))); }
    { const QSignalBlocker block(m_accountFilter); m_accountFilter->setSelection(m_accountFilters); m_accountFilter->setAccounts(prepared.accounts);
        m_accountFilter->setSessionCounts(prepared.accountCounts, countCaption + (m_hostFilters.isEmpty() ? QString() : tr(" / selected machines")) + (m_projectFilters.isEmpty() ? QString() : tr(" / selected projects"))); }

    const int scroll = m_sessions->verticalScrollBar()->value();
    applyListRows(prepared);
    int selectedRow = -1;
    for (int i = 0; i < prepared.rows.size(); ++i) {
        const auto &row = prepared.rows[i];
        if (row.kind == PreparedRow::Launch ? row.launch == m_selectedCreation
            : row.kind == PreparedRow::Child ? row.parentKey == requestedSelection && row.child == m_subagentId
            : row.kind == PreparedRow::Session && row.key == requestedSelection) selectedRow = i;
    }
    // Live streams take their folders first; the others are polled.
    updateGitWatch();
    for (const auto &[host, path] : prepared.gitReads) m_client.requestGitStatus(host, path);
    // List scopes do not choose a replacement for a conversation that still exists.
    const bool preserveConversation = selected();
    // Choose a first row only on initial load. A removed conversation leaves an
    // empty selection until an explicit choice, even after later fleet updates.
    if (!m_selectedKey.isEmpty()) m_initialSelectionPending = false;
    if (selectedRow < 0 && !preserveConversation && m_initialSelectionPending)
        selectedRow = prepared.firstVisible < 0 ? prepared.firstSession : prepared.firstVisible;
    if (selectedRow >= 0) m_initialSelectionPending = false;
    if (m_sessions->bulkSelecting()
        || (selectedRow >= 0 && m_sessions->item(selectedRow)->data(SessionRoles::Key).toString() == m_selectedKey)) {
        m_sessions->setCurrentRow(selectedRow, QItemSelectionModel::NoUpdate);
        QItemSelection selection;
        for (int i = 0; i < m_sessions->count(); ++i) {
            auto *item = m_sessions->item(i);
            if (!item->isHidden() && selectedKeys.contains(item->data(SessionRoles::Key).toString())) {
                const auto index = m_sessions->model()->index(i, 0); selection.select(index, index);
            }
        }
        m_sessions->selectionModel()->select(selection, QItemSelectionModel::ClearAndSelect);
    } else {
        m_sessions->clearBulkSelection(); m_sessions->setCurrentRow(selectedRow);
    }
    m_sessions->verticalScrollBar()->setValue(scroll);
    m_count->setText(prepared.countText);
    m_countFull = m_count->text(); m_countShort = prepared.countShort;
    // The strip keeps this row hidden (applySessionStrip()).
    m_folderFilterClear->setVisible(!m_folderFilterPath.isEmpty() && !(m_sessionDock && m_sessionDock->chromeNarrow()));
    const auto folderStyle=MachinePlatform::of(m_fleet.metricsOf(m_folderFilterHost), m_folderFilterHost.isEmpty());
    m_folderFilterClear->setText(tr("Folder: %1   ×").arg(MachinePlatform::leaf(m_folderFilterPath,folderStyle)));m_folderFilterClear->setToolTip(MachinePlatform::native(m_folderFilterPath,folderStyle)+tr("\nClear folder filter"));
    m_count->setToolTip(m_countFull);
    m_count->setVisible(!m_searchResults->active());
    applySessionStrip();
    m_rebuilding = false;
    updateSelectionActions();
    m_sessions->syncActivityAnimation();
    // A vanished target also clears while search results own the list.
    if ((selectedRow < 0 && !preserveConversation) || (selectedRow >= 0 && (!m_searchResults->active() || creationPlaced))) selectSession(); else renderDetails();
    // A fork opens its terminal once selected; a revealed row is shown now.
    if (!m_forkTerminalKey.isEmpty()) {
        const auto key = std::exchange(m_forkTerminalKey, {});
        QTimer::singleShot(0, this, [this, key] { if (m_selectedKey == key) { m_detailTabs->setCurrentWidget(m_terminal); m_terminal->connectSession(); } });
    }
    revealPendingRow();
    emit rowsApplied(prepared.generation);
}

void SessionsWindow::revealRow(const QString &key, bool select)
{
    // Rows still being prepared, as after a filter change, reveal it again.
    if (m_rowsPending || m_rowRequestQueued) m_reveal = PendingReveal{key, select};
    for (int row = 0; row < m_sessions->count(); ++row)
        if (m_sessions->item(row)->data(SessionRoles::Key).toString() == key) {
            if (select) m_sessions->setCurrentRow(row);
            m_sessions->scrollToItem(m_sessions->item(row)); return;
        }
}

void SessionsWindow::revealPendingRow()
{
    if (!m_reveal || m_rowsPending || m_rowRequestQueued) return;
    const auto reveal = *std::exchange(m_reveal, std::nullopt);
    revealRow(reveal.key, reveal.select);
}
