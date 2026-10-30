#pragma once
#include <QHash>
#include <QJsonObject>
#include <QList>
#include <QSet>
#include <QString>
#include <QStringList>
#include "TeamModel.h"

// WhiteWh fork, U54: moving a team's members to another account. Every member
// of one agent whose session really runs on an account (team status's
// session_account) moves with one `swap-member {from_account, to_account,
// provider}`; these helpers are shared by the member cards, the Move dialog
// and the tests.
namespace Teams::AccountMove {

// "", "native" and "native-<agent>" all name the agent's default account;
// this returns "native-<agent>" for them and the id otherwise.
QString accountOf(const QString &agent, const QString &account);
// The account's label from `account ls` profiles, or a readable fallback.
QString labelOf(const QList<QJsonObject> &profiles, const QString &agent, const QString &account);
// The account a member's session runs on; before it has one, or with an hgs
// that sends no session_account, the configured one.
QString memberAccount(const Member &member);
// Only Claude and Codex change account in place (U53).
bool movable(const QString &agent);
// The agents a move can take: movable ones with a local member session.
QStringList agents(const Team &team);
// The accounts those members run on, in roster order, for From.
QStringList fromAccounts(const Team &team, const QString &agent);
// The members a move from `from` takes: of this agent, a local session, the
// lead included; the same rule as the server's selection.
QStringList affected(const Team &team, const QString &agent, const QString &from);

// What keeps a member from its move or its launch, read from its error.
struct Block {
    enum Kind { None, OldSession, FolderBranch, Uncertain };
    Kind kind = None;
    QString text;                    // for the card, without the details
    QString folder, current, own;    // FolderBranch: the folder and its two branches
};
Block blockOf(const Member &member);

// One move the owner started from this desktop.
struct Move {
    QString team, agent, from, fromLabel, to, toLabel;
    QStringList members;
    QHash<QString, QString> runs;     // each member's run before the move
    QSet<QString> cleared;            // fresh members seen without a session
    bool running = false, finished = false;
    QJsonObject receipt;
    QString error;
};
// One member's progress: moving, resumed, fresh, launched, waiting,
// uncertain or failed, with its text and glyph.
struct Step {
    QString member, state, text, kind;
    Tone tone = Tone::Neutral;
};
QList<Step> progress(const Team &team, const Move &move);
Step stepOf(const Team &team, const Move &move, const QString &member);
// After each team read: a fresh member without a session is waiting for its
// new one, so a session it gets later is the launched one.
void observe(Move &move, const Team &team);

}
