#include "TeamAccountMove.h"

#include <QCoreApplication>
#include <QJsonArray>
#include <QRegularExpression>

namespace Teams::AccountMove {

namespace {
QString tr(const char *text) { return QCoreApplication::translate("Teams", text); }

bool local(const Member &member) { return member.host.isEmpty() && !member.session.isEmpty(); }

QStringList ids(const QJsonValue &value)
{
    QStringList found;
    for (const auto &item : value.toArray()) found << (item.isString() ? item.toString() : item.toObject().value("member").toString());
    found.removeAll(QString());
    return found;
}
}

QString accountOf(const QString &agent, const QString &account)
{
    if (account.isEmpty() || account == QLatin1String("native") || account.startsWith(QLatin1String("native-")))
        return QStringLiteral("native-") + agent;
    return account;
}

QString labelOf(const QList<QJsonObject> &profiles, const QString &agent, const QString &account)
{
    const QString id = accountOf(agent, account);
    for (const auto &profile : profiles)
        if (profile.value("id").toString() == id && !profile.value("label").toString().isEmpty()) return profile.value("label").toString();
    return id.startsWith(QLatin1String("native-")) ? tr("Default account") : id;
}

QString memberAccount(const Member &member)
{
    return accountOf(member.agent, member.sessionAccountKnown && !member.session.isEmpty() ? member.sessionAccount : member.account);
}

bool movable(const QString &agent)
{
    return agent == QLatin1String("claude") || agent == QLatin1String("codex");
}

QStringList agents(const Team &team)
{
    QStringList found;
    for (const auto &member : team.members)
        if (movable(member.agent) && local(member) && !found.contains(member.agent)) found << member.agent;
    return found;
}

QStringList fromAccounts(const Team &team, const QString &agent)
{
    QStringList found;
    for (const auto &member : team.members)
        if (member.agent == agent && local(member) && !found.contains(memberAccount(member))) found << memberAccount(member);
    return found;
}

QStringList affected(const Team &team, const QString &agent, const QString &from)
{
    QStringList found;
    const QString account = accountOf(agent, from);
    for (const auto &member : team.members)
        if (member.agent == agent && local(member) && memberAccount(member) == account) found << member.id;
    return found;
}

Block blockOf(const Member &member)
{
    Block block;
    const QString &error = member.error;
    if (error.isEmpty()) return block;
    // #619's launch check: "<folder> is on <branch>, not <member branch>; ...".
    static const QRegularExpression branch(QStringLiteral("^(.+?) is on (\\S+), not ([^\\s;,]+)[;,]"));
    if (const auto match = branch.match(error); match.hasMatch()) {
        block.kind = Block::FolderBranch;
        block.folder = match.captured(1); block.current = match.captured(2); block.own = match.captured(3);
        block.text = tr("Its folder is on %1, not %2").arg(block.current, block.own);
        return block;
    }
    if (error.contains(QLatin1String("waiting for the old session to finish its turn"))) {
        block.kind = Block::OldSession;
        block.text = tr("Waiting for its old session to finish its turn");
        return block;
    }
    if (error.contains(QLatin1String("already has an account"))) {
        block.kind = Block::OldSession;
        block.text = tr("Its old session still holds the name");
        return block;
    }
    if (error.contains(QLatin1String("switch is uncertain"))) {
        block.kind = Block::Uncertain;
        block.text = tr("The account switch did not confirm");
    }
    return block;
}

Step stepOf(const Team &team, const Move &move, const QString &id)
{
    Step step; step.member = id;
    const Member *member = team.member(id);
    const auto set = [&](const char *state, const QString &text, const char *kind, Tone tone) {
        step.state = QString::fromLatin1(state); step.text = text; step.kind = QString::fromLatin1(kind); step.tone = tone;
        return step;
    };
    const QString to = accountOf(move.agent, move.to);
    if (!move.finished) {
        // U53's switch keeps the session: its account changes under it.
        if (member && member->sessionAccountKnown && !member->session.isEmpty() && accountOf(member->agent, member->sessionAccount) == to)
            return set("resumed", tr("Resumed on %1").arg(move.toLabel), "done", Tone::Done);
        return set("moving", tr("Moving to %1").arg(move.toLabel), "working", Tone::Active);
    }
    if (!move.error.isEmpty()) return set("failed", tr("Not moved: %1").arg(move.error), "failed", Tone::Danger);
    const QStringList fresh = ids(move.receipt.value("fresh"));
    QStringList swapped = ids(move.receipt.value("members"));
    if (fresh.contains(id)) {
        if (member && !member->session.isEmpty() && (move.cleared.contains(id) || member->runId != move.runs.value(id)))
            return set("launched", tr("Launched on %1 in a new session").arg(move.toLabel), "done", Tone::Done);
        if (member && !member->error.isEmpty()) {
            const auto block = blockOf(*member);
            return set("failed", block.kind == Block::None ? tr("Not launched: %1").arg(member->error) : block.text, "failed", Tone::Danger);
        }
        return set("fresh", tr("Old session paused and saved; a new session starts on %1").arg(move.toLabel), "working", Tone::Active);
    }
    if (swapped.contains(id)) return set("resumed", tr("Resumed on %1 in the same conversation").arg(move.toLabel), "done", Tone::Done);
    // Neither: the switch did not move it and the member's error says why.
    if (member) {
        const auto block = blockOf(*member);
        if (block.kind == Block::OldSession) return set("waiting", block.text, "paused", Tone::Warn);
        if (block.kind == Block::Uncertain) return set("uncertain", tr("Uncertain: %1").arg(member->error), "stuck", Tone::Warn);
        if (!member->error.isEmpty()) return set("failed", tr("Not moved: %1").arg(member->error), "failed", Tone::Danger);
    }
    return set("failed", tr("Not moved"), "failed", Tone::Danger);
}

QList<Step> progress(const Team &team, const Move &move)
{
    QList<Step> steps;
    for (const auto &id : move.members) steps << stepOf(team, move, id);
    return steps;
}

void observe(Move &move, const Team &team)
{
    if (!move.finished || move.team != team.id) return;
    const QStringList fresh = ids(move.receipt.value("fresh"));
    for (const auto &id : fresh)
        if (const Member *member = team.member(id); member && member->session.isEmpty()) move.cleared.insert(id);
}

}
