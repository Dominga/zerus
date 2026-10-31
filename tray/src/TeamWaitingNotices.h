#pragma once
#include <QCoreApplication>
#include <QSet>
#include <QSettings>
#include <QStringList>
#include <algorithm>

#include "Settings.h"
#include "TeamAttentionNotices.h"
#include "TeamModel.h"

// WhiteWh fork, U55: one desktop notification the first time an item of
// `hgs team waiting` (U50) waits for the owner, never again for it. The ids
// seen are kept in QSettings (teams/waitingNotified), at most Limit of them;
// an id that no longer waits is dropped once the whole list was read.
//
// The N7 attention notices (TeamAttentionNotices) already announce an
// escalated report, a permission, a phase draft and a replan from each team's
// attention list. The owner gets one notification per thing, whichever comes
// first: a waiting item an attention item covers is not announced here, and
// an attention item whose waiting item was already seen here is not announced
// there (suppressed()).
class TeamWaitingNotices {
    Q_DECLARE_TR_FUNCTIONS(TeamWaitingNotices)
public:
    struct Item { QString id, team, teamName, kind, title; };
    struct Notice { QString token, team, title, body; };
    static constexpr int Limit = 500;
    // At most this many notifications for one read; the rest are one more.
    static constexpr int PerRead = 3;
    static constexpr const char *kSetting = "teams/waitingNotified";

    explicit TeamWaitingNotices(const QStringList &known = {}) : m_known(known)
    {
        while (m_known.size() > Limit) m_known.removeFirst();
    }
    static TeamWaitingNotices stored() { return TeamWaitingNotices(Settings::value(QLatin1String(kSetting)).toStringList()); }
    void store() const { Settings::set(QLatin1String(kSetting), m_known); }
    QStringList known() const { return m_known; }

    // The waiting id (owner_waiting.rs) of an attention item, empty for a kind
    // the waiting list does not carry; a replan names only its prefix.
    static QString waitingId(const Teams::Team &team, const Teams::Team::Attention &item)
    {
        if (item.kind == QLatin1String("phase_draft")) return team.id + QStringLiteral("/phase_draft/%1").arg(item.phase);
        if (item.kind == QLatin1String("report")) return team.id + QStringLiteral("/report_escalated/%1").arg(item.task);
        if (item.kind == QLatin1String("permission")) return team.id + QStringLiteral("/permission_escalated/") + item.member;
        if (item.kind == QLatin1String("replan")) return team.id + QStringLiteral("/owner_word_asked/replan-");
        return {};
    }
    static bool matches(const QString &waiting, const QString &id)
    {
        return !waiting.isEmpty() && (id == waiting || (waiting.endsWith(QLatin1Char('-')) && id.startsWith(waiting)));
    }
    // An attention item of the polled team list announces this item already.
    static bool covered(const QList<Teams::Team> &teams, const Item &item)
    {
        for (const auto &team : teams) {
            if (team.id != item.team) continue;
            for (const auto &attention : team.attention) if (matches(waitingId(team, attention), item.id)) return true;
        }
        return false;
    }
    bool known(const QString &id) const { return m_known.contains(id); }
    QStringList knownIds() const { return m_known; }

    // One read of the waiting list: a notice for each item seen for the first
    // time that no attention item covers. `complete`: the read holds every
    // waiting item, so the ids it lacks no longer wait and are dropped.
    QList<Notice> observe(const QList<Item> &items, const QList<Teams::Team> &teams, bool complete)
    {
        QList<Notice> fresh;
        QSet<QString> present;
        for (const auto &item : items) {
            present.insert(item.id);
            if (m_known.contains(item.id)) {
                // Still waiting: it moves to the newest end and is kept.
                m_known.removeAll(item.id); m_known.append(item.id);
                continue;
            }
            m_known.append(item.id);
            if (covered(teams, item)) continue;
            fresh.append({token(item), item.team, tr("Waiting for you in %1").arg(item.teamName.isEmpty() ? item.team : item.teamName), item.title});
        }
        if (complete) m_known.erase(std::remove_if(m_known.begin(), m_known.end(), [&present](const QString &id) { return !present.contains(id); }), m_known.end());
        while (m_known.size() > Limit) m_known.removeFirst();
        if (fresh.size() > PerRead) {
            const int more = int(fresh.size()) - PerRead + 1;
            const Notice last = fresh[PerRead - 1];
            fresh = fresh.mid(0, PerRead - 1);
            fresh.append({last.token, last.team, tr("Waiting for you"), tr("%1 more items wait for you").arg(more)});
        }
        return fresh;
    }

    // The attention tokens not to announce: their waiting item was seen here.
    QSet<QString> suppressed(const QList<Teams::Team> &teams) const
    {
        QSet<QString> tokens;
        for (const auto &team : teams)
            for (const auto &attention : team.attention) {
                const QString waiting = waitingId(team, attention);
                if (waiting.isEmpty()) continue;
                if (std::any_of(m_known.cbegin(), m_known.cend(), [&waiting](const QString &id) { return matches(waiting, id); }))
                    tokens.insert(TeamAttentionNotices::token(team, attention));
            }
        return tokens;
    }
    // The attention notices of one poll without those announced here already.
    static void dropAnnounced(TeamAttentionNotices::Changes &changes, const QList<Teams::Team> &teams)
    {
        if (changes.raised.isEmpty()) return;
        const auto skip = stored().suppressed(teams);
        changes.raised.erase(std::remove_if(changes.raised.begin(), changes.raised.end(),
                                            [&skip](const TeamAttentionNotices::Notice &notice) { return skip.contains(notice.token); }),
                             changes.raised.end());
    }
    // The tray's token: the attention prefix, so a click opens the team.
    static QString token(const Item &item)
    {
        return QString::fromLatin1(TeamAttentionNotices::kPrefix) + item.team + QStringLiteral(":waiting:") + item.id;
    }

private:
    QStringList m_known;   // oldest first
};
