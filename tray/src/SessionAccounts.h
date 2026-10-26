#pragma once
#include "AccountCatalog.h"
#include "HgsClient.h"
#include "IdentityBadge.h"
#include <QHash>
#include <QJsonDocument>
#include <algorithm>

// A selection contains recorded machine-local bindings, never captions or agent
// types. Verified account identities only group those bindings for presentation.
namespace SessionAccounts {
inline QString binding(const QString &host, const QString &provider, const QString &id, const QString &home = {}) {
    return QString::fromUtf8(QJsonDocument(QJsonArray{host, provider, id.isEmpty() ? QStringLiteral("home") : QStringLiteral("id"), id.isEmpty() ? home : id}).toJson(QJsonDocument::Compact));
}
inline QString binding(const QString &host, const SessionInfo &session) {
    return binding(host, session.cmd, session.accountId, session.accountHome);
}
inline bool accepts(const QSet<QString> &selection, const QString &host, const SessionInfo &session) {
    return selection.isEmpty() || selection.contains(binding(host, session));
}
struct Account {
    QString id, name, provider;
    QStringList machines;
    QSet<QString> bindings;
};
class Catalog {
public:
    void reset(const QJsonArray &profiles, const QJsonArray &bindingEvidence = {}) {
        m_accounts.clear(); m_groups.clear(); m_catalogIds.clear();
        QHash<QString, QSet<QString>> homes;
        for (const auto &value : AccountCatalog::group(profiles)) {
            const auto group = value.toObject(); const auto id = group["account_key"].toString();
            m_catalogIds.insert(id);
            Account account{id, group["label"].toString(), group["provider"].toString(), {}, {}};
            const auto identity = group["usage"].toObject()["identity"].toObject();
            const auto email = identity["email"].toString(), organization = identity["organization"].toString();
            if (account.name.isEmpty()) account.name = QObject::tr("Unnamed account");
            if (!email.isEmpty() && !account.name.contains(email, Qt::CaseInsensitive)) account.name += " (" + email + ")";
            if (!organization.isEmpty()) account.name += " (" + organization + ")";
            for (const auto &memberValue : group["members"].toArray()) {
                const auto member = memberValue.toObject();
                const auto key = binding(member["host"].toString(), account.provider, member["id"].toString(), member["home"].toString());
                account.bindings.insert(key); m_groups.insert(key, id);
                const auto machine = member["machine"].toString(); if (!account.machines.contains(machine)) account.machines.append(machine);
                if (!member["home"].toString().isEmpty()) homes[binding(member["host"].toString(), account.provider, {}, member["home"].toString())].insert(id);
            }
            m_accounts.insert(id, account);
        }
        // Removed or unresolved profiles are evidence against a home alias,
        // never extra chooser accounts. Keep ambiguity checks across history.
        for (const auto &value : AccountCatalog::group(bindingEvidence)) {
            const auto group = value.toObject();
            for (const auto &memberValue : group["members"].toArray()) {
                const auto member = memberValue.toObject();
                if (!member["home"].toString().isEmpty()) homes[binding(member["host"].toString(), group["provider"].toString(), {}, member["home"].toString())].insert(group["account_key"].toString());
            }
        }
        // Older sessions may record only their agent home. Ambiguous homes do
        // not borrow a catalog account or the machine's current default.
        for (auto it = homes.cbegin(); it != homes.cend(); ++it) if (it.value().size() == 1) m_groups.insert(it.key(), *it.value().cbegin());
    }
    void observe(const QString &host, const QString &machine, const SessionInfo &session) {
        const auto key = binding(host, session); auto id = m_groups.value(key);
        if (id.isEmpty()) {
            id = "binding:" + key;
            const auto name = !session.accountId.isEmpty() ? session.accountId
                : !session.accountHome.isEmpty() ? session.accountHome
                : session.cmd == "sh" || session.cmd == "shell" || session.cmd == "bash" || session.cmd == "zsh" || session.cmd == "pwsh" ? QObject::tr("No account (shell)") : QObject::tr("Account not recorded");
            m_accounts.insert(id, {id, name, session.cmd, {}, {}}); m_groups.insert(key, id);
        }
        auto &account = m_accounts[id]; account.bindings.insert(key);
        if (!account.machines.contains(machine)) account.machines.append(machine);
    }
    QList<Account> accounts() const {
        auto rows = m_accounts.values();
        std::sort(rows.begin(), rows.end(), [](const Account &a, const Account &b) {
            const auto first = IdentityBadges::providerName(a.provider) + '\n' + a.name;
            const auto second = IdentityBadges::providerName(b.provider) + '\n' + b.name;
            const int order = QString::localeAwareCompare(first, second); return order ? order < 0 : a.id < b.id;
        });
        return rows;
    }
    QList<Account> catalogAccounts() const {
        auto rows = accounts();
        rows.removeIf([this](const Account &account) { return !m_catalogIds.contains(account.id); });
        return rows;
    }
    QString description(const QString &host, const SessionInfo &session) const {
        const auto key = binding(host, session); const auto account = m_accounts.value(m_groups.value(key));
        return account.name.isEmpty() ? QObject::tr("Account not recorded") : account.name + " (" + IdentityBadges::providerName(account.provider) + ")";
    }
private:
    QHash<QString, Account> m_accounts;
    QHash<QString, QString> m_groups;
    QSet<QString> m_catalogIds;
};
}
