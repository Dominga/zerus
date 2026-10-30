#pragma once
#include "ComposerToolbar.h"
#include <QColor>
#include <QJsonArray>
#include <QJsonObject>
#include <functional>

class QLabel;
class QVBoxLayout;

// WhiteWh fork: U53, the account of a running session (spec 2026-10-28 team
// customization, section 10). The composer shows the session's account by its
// label beside the model; its menu lists the same agent's signed-in accounts
// on that computer, each with its usage. Only the owner's choice changes an
// account: a pick asks for confirmation (the agent restarts into the same
// conversation), then `hgs switch-account` runs and the chip says Switching…
// until the receipt (switched, rolled_back, uncertain) or the refusal returns.
namespace AccountSwitch {

struct Option {
    QString id, label, usage, tooltip;
    QColor usageColor;
    bool current = false;
};
// The accounts a session may move to: `profiles` (an account listing, its
// `usage` or `account_status` snapshot) on the session's computer, of its own
// agent and signed in, the current one first and marked.
QList<Option> options(const QJsonArray &profiles, const QString &host, const QString &provider, const QString &current, bool dark);
// The session's account id as the switch reads it: none is the native one.
QString accountOf(const QString &provider, const QString &accountId);
// Whether the agent can change accounts at all (U53: Claude and Codex).
bool switchable(const QString &provider);

// What a switch ended with: the line its owner shows, its tone, and the
// account the session runs on afterwards.
struct Outcome {
    enum Kind { None, Switched, RolledBack, Uncertain, Refused } kind = None;
    QString text, account, label;
};
// `receipt` is the CLI's JSON receipt; `error` a refusal (no receipt).
Outcome outcome(const QJsonObject &receipt, const QString &error, const QString &target);
}

// The chip ("sessionAccountChip"). Its popover (accountPopover()) lists the options
// ("sessionAccountOption"), then asks to confirm ("sessionAccountConfirm",
// Cancel "sessionAccountCancel", Move "sessionAccountMove").
class AccountSwitchChip : public ToolbarChip {
public:
    explicit AccountSwitchChip(QWidget *parent = nullptr);
    // `key` names the session; another key forgets the last outcome. An
    // empty label hides the chip (an hgs without U53 names no account).
    void setSession(const QString &key, const QString &provider, const QString &accountId, const QString &label);
    QString sessionKey() const { return m_key; }
    void setOptions(const QList<AccountSwitch::Option> &options);
    // The session cannot change now (it works, is stopped or offline): the
    // chip is disabled and says why.
    void setBlocked(const QString &reason);
    // The account changes elsewhere (a team member): the menu lists the
    // accounts with this note and none is chosen here.
    void setLocked(const QString &reason);
    void setTheme(bool dark);
    // The owner's layout may leave no room for it (a narrow composer, the
    // Tasks tab): hidden then, whatever the session.
    void setAllowed(bool allowed);
    // A switch of the shown session is running (one at a time for the chip).
    bool switching() const { return !m_switching.isEmpty(); }
    // The receipt or refusal of this chip's request, for `key`.
    AccountSwitch::Outcome finish(const QString &key, const QJsonObject &receipt, const QString &error);
    // The shown session's last outcome, none for another session.
    AccountSwitch::Outcome lastOutcome() const { return m_outcomeKey == m_key ? m_outcome : AccountSwitch::Outcome{}; }
    // The owner runs the switch: hgs switch-account <session> --account id.
    std::function<void(const QString &key, const QString &account)> switchRequested;
    ChipPopover *accountPopover() const { return m_popover; }
private:
    void showList();
    void showConfirm(const AccountSwitch::Option &option);
    void clearPage();
    void render();
    ChipPopover *m_popover = nullptr;
    QWidget *m_page = nullptr;
    QVBoxLayout *m_pageLayout = nullptr;
    QString m_key, m_provider, m_account, m_label, m_blocked, m_locked;
    QString m_switchKey, m_switching, m_switchingLabel, m_outcomeKey;
    QList<AccountSwitch::Option> m_options;
    AccountSwitch::Outcome m_outcome;
    bool m_dark = true, m_allowed = true;
};
