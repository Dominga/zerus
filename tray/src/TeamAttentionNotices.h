#pragma once
#include <QCoreApplication>
#include <QSet>
#include <QStringList>

#include "TeamModel.h"

// WhiteWh fork, N7 (#364, U18): the owner's attention items of every team
// (C4: a report waiting, a permission the lead left unanswered, a phase
// draft...) as one line each - the member, what waits and its age - and as
// desktop notifications, raised once per item and withdrawn when it is gone.
// An item is known by its team, kind, member, task, phase and start time; the
// known tokens are kept so a restart does not raise them again.
class TeamAttentionNotices {
    Q_DECLARE_TR_FUNCTIONS(TeamAttentionNotices)
public:
    struct Notice { QString token, team, title, body; };
    struct Changes { QList<Notice> raised; QStringList cleared; };

    static constexpr const char *kPrefix = "zerus-team-attention:";

    explicit TeamAttentionNotices(const QStringList &known = {}) : m_known(known.begin(), known.end()) {}

    static QString token(const Teams::Team &team, const Teams::Team::Attention &item)
    {
        return QString::fromLatin1(kPrefix) + team.id + QLatin1Char(':') + item.kind + QLatin1Char(':') + item.member
               + QLatin1Char(':') + QString::number(item.task) + QLatin1Char(':') + QString::number(item.phase)
               + QLatin1Char(':') + QString::number(qint64(item.since));
    }
    static QString teamOf(const QString &token)
    {
        if (!token.startsWith(QLatin1String(kPrefix))) return {};
        return token.mid(int(qstrlen(kPrefix))).section(QLatin1Char(':'), 0, 0);
    }
    // What waits, with the member: the server's own sentence when it sends
    // one ("Backend waits for your answer: Run cargo test?").
    static QString what(const Teams::Team::Attention &item)
    {
        if (!item.summary.isEmpty()) return item.summary;
        if (item.kind == QLatin1String("report"))
            return tr("Report on #%1 from %2 waits for your decision").arg(item.task).arg(item.member);
        if (item.kind == QLatin1String("permission"))
            return tr("%1 waits for your answer: a permission request").arg(item.member.isEmpty() ? tr("A member") : item.member);
        QStringList parts{QString(item.kind).replace(QLatin1Char('_'), QLatin1Char(' '))};
        if (!item.member.isEmpty()) parts << item.member;
        if (item.task > 0) parts << QStringLiteral("#%1").arg(item.task);
        return parts.join(QStringLiteral(" · "));
    }
    static QString age(const Teams::Team::Attention &item)
    {
        return item.minutes == 1 ? tr("1 min") : tr("%1 min").arg(item.minutes);
    }

    Changes observe(const QList<Teams::Team> &teams)
    {
        Changes changes;
        QSet<QString> present;
        for (const auto &team : teams)
            for (const auto &item : team.attention) {
                const auto key = token(team, item);
                present.insert(key);
                if (m_known.contains(key)) continue;
                changes.raised.append({key, team.id, tr("Team %1 needs you").arg(team.name),
                                       what(item) + QStringLiteral(" · ") + age(item)});
            }
        for (const auto &key : std::as_const(m_known))
            if (!present.contains(key)) changes.cleared.append(key);
        m_known = present;
        return changes;
    }
    QStringList known() const { return QStringList(m_known.begin(), m_known.end()); }

private:
    QSet<QString> m_known;
};
