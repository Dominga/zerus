#pragma once
#include <QCoreApplication>
#include <QJsonArray>
#include <QJsonObject>
#include <QList>
#include <QSet>
#include <QString>
#include <QStringList>

// WhiteWh fork, #569: a private store that hgs refuses is never silent. Each
// listing carries "private_stores" (path, reason, fix) while a store of that
// computer stays refused; a store is raised once per (computer, path, reason)
// and raised again only after it was resolved and comes back. The keys
// already raised are kept across restarts by the caller (known/restore).
class PrivateStoreNotices {
public:
    struct Notice { QString key, title, body; };

    explicit PrivateStoreNotices(const QStringList &known = {}) : m_known(known.cbegin(), known.cend()) {}

    // The stores refused now on `host` ("" for this computer, `machine` its
    // name). Only a listing that answered (`ok`) can resolve one: an offline
    // computer keeps what it last reported.
    QList<Notice> observe(const QString &host, const QString &machine, bool ok, const QJsonArray &stores)
    {
        QList<Notice> raised;
        if (!ok) return raised;
        const QString prefix = host + QLatin1Char('\n');
        QSet<QString> current;
        for (const auto &value : stores) {
            const auto store = value.toObject();
            const QString path = store.value(QStringLiteral("path")).toString();
            const QString reason = store.value(QStringLiteral("reason")).toString();
            if (path.isEmpty()) continue;
            const QString key = prefix + path + QLatin1Char('\n') + reason;
            current.insert(key);
            if (m_known.contains(key)) continue;
            m_known.insert(key);
            const QString where = machine.isEmpty() ? host : machine;
            raised.append({key, QCoreApplication::translate("PrivateStoreNotices", "Zerus: a private store is refused"),
                           QCoreApplication::translate("PrivateStoreNotices", "%1: %2\n%3\nFix: %4")
                               .arg(where.isEmpty() ? QCoreApplication::translate("PrivateStoreNotices", "this computer") : where,
                                    path, reason, store.value(QStringLiteral("fix")).toString())});
        }
        // Resolved on this computer: raised again if it comes back.
        for (auto it = m_known.begin(); it != m_known.end();) {
            if (it->startsWith(prefix) && !current.contains(*it)) it = m_known.erase(it);
            else ++it;
        }
        return raised;
    }

    // A computer no longer configured: its stores are forgotten.
    void forget(const QString &host)
    {
        const QString prefix = host + QLatin1Char('\n');
        for (auto it = m_known.begin(); it != m_known.end();) {
            if (it->startsWith(prefix)) it = m_known.erase(it);
            else ++it;
        }
    }

    // What to keep across restarts, bounded.
    QStringList known() const
    {
        QStringList keys(m_known.cbegin(), m_known.cend());
        keys.sort();
        return keys.mid(0, 128);
    }

private:
    QSet<QString> m_known;
};
