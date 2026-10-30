#include "AccountSwitch.h"
#include "AccountUsage.h"
#include "WorkspaceStyle.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QTimer>
#include <QVBoxLayout>
#include <algorithm>

namespace AccountSwitch {

namespace {
// The usage states that mean no sign-in to run with (New session's list).
const QStringList SignedOut{"signed_out", "expired", "credentials_locked", "credentials_unavailable", "desktop_session_unavailable"};
QString reasonText(QString error)
{
    error = error.trimmed();
    if (error.startsWith(QLatin1String("hgs: "))) error = error.mid(5);
    return error;
}
}

QString accountOf(const QString &provider, const QString &accountId)
{
    return accountId.isEmpty() ? QStringLiteral("native-") + provider : accountId;
}

bool switchable(const QString &provider)
{
    return provider == QLatin1String("claude") || provider == QLatin1String("codex");
}

QList<Option> options(const QJsonArray &profiles, const QString &host, const QString &provider, const QString &current, bool dark)
{
    QList<Option> rows;
    for (const auto &value : profiles) {
        const auto profile = value.toObject();
        const auto id = profile.value("id").toString();
        if (id.isEmpty() || profile.value("host").toString() != host || profile.value("provider").toString() != provider || profile.value("removed").toBool()) continue;
        // The listing's snapshot stands in while a probe runs (New session's rule).
        auto usage = profile.value("usage").toObject();
        const auto cached = profile.value("account_status").toObject();
        if (!cached.isEmpty() && (usage.isEmpty() || usage.value("status") == QLatin1String("loading") || usage.value("status") == QLatin1String("offline")
                                  || cached.value("checked_at").toDouble() > usage.value("checked_at").toDouble())) {
            const auto refreshing = usage.value("refreshing"); usage = cached;
            if (!refreshing.isUndefined()) usage["refreshing"] = refreshing;
        }
        const bool isCurrent = id == current;
        // A signed-out account has nothing to run with; the current one stays listed.
        if (!isCurrent && SignedOut.contains(usage.value("status").toString())) continue;
        Option option;
        option.id = id; option.current = isCurrent;
        option.label = profile.value("label").toString();
        if (option.label.isEmpty()) option.label = usage.value("identity").toObject().value("email").toString(id);
        option.usage = AccountUsage::launchSummary(usage);
        const auto tone = AccountUsage::severest(usage);
        const bool historical = usage.value("offline").toBool() || usage.value("refresh_error").toBool();
        if (!historical && tone && *tone != AccountUsage::Tone::Normal) option.usageColor = AccountUsage::toneColor(*tone, dark);
        option.tooltip = AccountUsage::tooltip(usage);
        rows.append(option);
    }
    std::stable_sort(rows.begin(), rows.end(), [](const Option &a, const Option &b) { return a.current && !b.current; });
    return rows;
}

Outcome outcome(const QJsonObject &receipt, const QString &error, const QString &target)
{
    Outcome result;
    const auto from = receipt.value("from").toObject(), to = receipt.value("to").toObject();
    const auto fromLabel = from.value("label").toString(from.value("account_id").toString());
    const auto toLabel = to.value("label").toString(to.value("account_id").toString(target));
    const auto status = receipt.value("status").toString();
    if (status == QLatin1String("switched")) {
        result.kind = Outcome::Switched; result.account = to.value("account_id").toString(); result.label = toLabel;
        result.text = QObject::tr("Moved to %1; the agent resumed the same conversation.").arg(toLabel);
    } else if (status == QLatin1String("uncertain")) {
        result.kind = Outcome::Uncertain; result.account = to.value("account_id").toString(); result.label = toLabel;
        result.text = QObject::tr("Moved to %1, but the resumed conversation did not confirm. Check the session; Zerus does not retry it.").arg(toLabel);
    } else if (status == QLatin1String("rolled_back")) {
        result.kind = Outcome::RolledBack; result.account = from.value("account_id").toString(); result.label = fromLabel;
        const auto reason = receipt.value("reason").toString();
        result.text = receipt.value("relaunched").toBool(true)
            ? QObject::tr("Stayed on %1: %2 could not start the agent (%3). It resumed on %1.").arg(fromLabel, target, reason)
            : QObject::tr("The move to %1 failed (%2) and the agent did not restart on %3: %4. Resume it from Activity.")
                  .arg(target, reason, fromLabel, receipt.value("relaunch_error").toString());
    } else {
        result.kind = Outcome::Refused;
        result.text = QObject::tr("Not moved: %1").arg(error.isEmpty() ? QObject::tr("the switch gave no receipt") : reasonText(error));
    }
    return result;
}
}

using namespace AccountSwitch;

AccountSwitchChip::AccountSwitchChip(QWidget *parent) : ToolbarChip(parent)
{
    setObjectName("sessionAccountChip"); setIconName(QStringLiteral("accounts")); setTone(ChipTone::Quiet);
    // Its name stays "chipPopover": the popover's own style draws its panel by it.
    auto *popover = m_popover = new ChipPopover(this);
    m_page = new QWidget; m_page->setObjectName("sessionAccountPage");
    m_pageLayout = new QVBoxLayout(m_page); m_pageLayout->setContentsMargins(12, 10, 12, 10); m_pageLayout->setSpacing(6);
    popover->setContent(m_page);
    connect(this, &QPushButton::clicked, this, [this, popover] {
        if (!switching() && m_blocked.isEmpty()) { showList(); popover->showFor(this); }
    });
    render();
}

void AccountSwitchChip::setSession(const QString &key, const QString &provider, const QString &accountId, const QString &label)
{
    // The last outcome and a switch in flight show only on their own session.
    if (key != m_key) m_popover->hide();
    m_key = key; m_provider = provider; m_account = accountOf(provider, accountId); m_label = label;
    render();
}

void AccountSwitchChip::setOptions(const QList<Option> &options)
{
    m_options = options;
    // An open confirmation stays as the owner read it.
    if (m_popover->isVisible() && !m_page->property("confirming").toBool()) showList();
}

void AccountSwitchChip::setBlocked(const QString &reason)
{
    if (reason == m_blocked) return;
    m_blocked = reason;
    if (!reason.isEmpty()) m_popover->hide();
    render();
}

void AccountSwitchChip::setLocked(const QString &reason)
{
    if (reason == m_locked) return;
    m_locked = reason; render();
}

void AccountSwitchChip::setAllowed(bool allowed)
{
    if (allowed == m_allowed) return;
    m_allowed = allowed;
    if (!allowed) m_popover->hide();
    render();
}

void AccountSwitchChip::setTheme(bool dark)
{
    m_dark = dark; ToolbarChip::setTheme(dark);
    m_popover->setTheme(dark);
    setWorkspaceStyle(m_page, QString("QWidget#sessionAccountPage QLabel { color:%1; background:transparent; border:0; }"
        " QLabel#sessionAccountTitle, QLabel#sessionAccountConfirmTitle { font-weight:600; }"
        " QLabel#sessionAccountNote, QLabel#sessionAccountOutcome { color:%2; }"
        " QPushButton#sessionAccountOption { text-align:left; background:transparent; border:1px solid transparent; border-radius:7px; padding:0; }"
        " QPushButton#sessionAccountOption:hover:enabled { background:%3; border-color:%4; }"
        " QPushButton#sessionAccountOption[current=\"true\"] { background:%3; }"
        " QPushButton#sessionAccountCancel, QPushButton#sessionAccountMove { padding:4px 12px; border-radius:7px; border:1px solid %4; background:%3; color:%1; }"
        " QPushButton#sessionAccountMove { background:%5; border-color:%5; color:%6; font-weight:600; }")
        .arg(dark ? "#e8edf4" : "#1a2733", dark ? "#a1adbb" : "#647386", dark ? "#222a33" : "#eef2f5",
             dark ? "#3d4855" : "#dce2e8", dark ? "#8bdfc0" : "#167357", dark ? "#10201b" : "#ffffff"));
}

Outcome AccountSwitchChip::finish(const QString &key, const QJsonObject &receipt, const QString &error)
{
    if (key != m_switchKey) return {};
    const auto target = m_switchingLabel;
    m_switchKey.clear(); m_switching.clear(); m_switchingLabel.clear();
    m_outcome = AccountSwitch::outcome(receipt, error, target); m_outcomeKey = key;
    if (key == m_key && !m_outcome.account.isEmpty()) { m_account = m_outcome.account; m_label = m_outcome.label; }
    render();
    return m_outcome;
}

void AccountSwitchChip::clearPage()
{
    while (auto *item = m_pageLayout->takeAt(0)) {
        // A row may be the sender: deleted later, its names go now so no
        // lookup finds the retired page.
        if (auto *widget = item->widget()) {
            for (auto *child : widget->findChildren<QWidget *>()) child->setObjectName(QString());
            widget->setObjectName(QString()); widget->hide(); widget->deleteLater();
        }
        delete item;
    }
}

void AccountSwitchChip::showList()
{
    clearPage(); m_page->setProperty("confirming", false);
    auto *title = new QLabel(tr("Account for this session")); title->setObjectName("sessionAccountTitle");
    m_pageLayout->addWidget(title);
    if (const auto outcome = lastOutcome(); outcome.kind != Outcome::None) {
        auto *last = new QLabel(outcome.text); last->setObjectName("sessionAccountOutcome"); last->setWordWrap(true);
        m_pageLayout->addWidget(last);
    }
    const QString note = !m_locked.isEmpty() ? m_locked
        : !switchable(m_provider) ? tr("Only Claude and Codex sessions change their account.")
        : tr("Signed-in %1 accounts on this computer. A move restarts the agent into the same conversation.").arg(m_provider == "codex" ? tr("Codex") : tr("Claude"));
    auto *hint = new QLabel(note); hint->setObjectName("sessionAccountNote"); hint->setWordWrap(true);
    m_pageLayout->addWidget(hint);
    const bool choosable = m_locked.isEmpty() && switchable(m_provider);
    for (const auto &option : std::as_const(m_options)) {
        auto *row = new QPushButton; row->setObjectName("sessionAccountOption"); row->setProperty("account", option.id);
        row->setProperty("current", option.current); row->setCursor(Qt::PointingHandCursor); row->setMinimumHeight(42);
        row->setEnabled(choosable && !option.current);
        row->setToolTip(option.tooltip); row->setAccessibleName(option.label + (option.current ? tr(" (current)") : QString()));
        row->setAccessibleDescription(option.usage);
        auto *h = new QHBoxLayout(row); h->setContentsMargins(8, 4, 8, 4); h->setSpacing(8);
        auto *text = new QVBoxLayout; text->setSpacing(1);
        auto *name = new QLabel(option.label); name->setObjectName("sessionAccountOptionName");
        QFont bold = name->font(); bold.setWeight(QFont::DemiBold); name->setFont(bold);
        auto *usage = new QLabel(option.usage); usage->setObjectName("sessionAccountOptionUsage");
        if (option.usageColor.isValid()) usage->setStyleSheet(QStringLiteral("color:%1;").arg(option.usageColor.name()));
        else usage->setStyleSheet(QStringLiteral("color:%1;").arg(m_dark ? "#a1adbb" : "#647386"));
        text->addWidget(name); text->addWidget(usage);
        h->addLayout(text, 1);
        if (option.current) {
            auto *mark = new QLabel(tr("Current")); mark->setObjectName("sessionAccountOptionCurrent");
            mark->setStyleSheet(QStringLiteral("color:%1;").arg(m_dark ? "#8bdfc0" : "#167357"));
            h->addWidget(mark, 0, Qt::AlignVCenter);
        }
        for (auto *label : {name, usage}) label->setAttribute(Qt::WA_TransparentForMouseEvents);
        connect(row, &QPushButton::clicked, this, [this, option] { showConfirm(option); });
        m_pageLayout->addWidget(row);
    }
    if (m_options.size() <= 1 && choosable) {
        auto *none = new QLabel(tr("No other signed-in account of this agent. Add or sign in to one in Accounts."));
        none->setObjectName("sessionAccountNone"); none->setWordWrap(true);
        m_pageLayout->addWidget(none);
    }
    setTheme(m_dark);
    if (m_popover->isVisible()) m_popover->reposition();
}

void AccountSwitchChip::showConfirm(const Option &option)
{
    clearPage(); m_page->setProperty("confirming", true);
    auto *frame = new QWidget; frame->setObjectName("sessionAccountConfirm"); frame->setProperty("account", option.id);
    auto *v = new QVBoxLayout(frame); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(8);
    auto *title = new QLabel(tr("Move this session to %1?").arg(option.label)); title->setObjectName("sessionAccountConfirmTitle"); title->setWordWrap(true);
    auto *text = new QLabel(tr("The agent stops once it is idle, its conversation is copied to %1 and it restarts there in the same "
                               "conversation, folder and name. If it cannot start on %1, it goes back to %2. Nothing is deleted from %2.")
                                .arg(option.label, m_label));
    text->setObjectName("sessionAccountConfirmText"); text->setWordWrap(true);
    auto *buttons = new QHBoxLayout; buttons->addStretch(1);
    auto *cancel = new QPushButton(tr("Cancel")); cancel->setObjectName("sessionAccountCancel");
    auto *move = new QPushButton(tr("Move to %1").arg(option.label)); move->setObjectName("sessionAccountMove"); move->setDefault(true);
    buttons->addWidget(cancel); buttons->addWidget(move);
    v->addWidget(title); v->addWidget(text); v->addLayout(buttons);
    m_pageLayout->addWidget(frame);
    connect(cancel, &QPushButton::clicked, this, [this] { showList(); });
    connect(move, &QPushButton::clicked, this, [this, option] {
        m_popover->hide();
        // Only this confirmed choice reaches hgs, once.
        if (!m_switching.isEmpty() || !m_blocked.isEmpty() || !m_locked.isEmpty()) return;
        m_switchKey = m_key; m_switching = option.id; m_switchingLabel = option.label; m_outcome = {}; m_outcomeKey.clear();
        render();
        if (switchRequested) switchRequested(m_key, option.id);
    });
    setTheme(m_dark);
    if (m_popover->isVisible()) m_popover->reposition();
    move->setFocus(Qt::OtherFocusReason);
}

void AccountSwitchChip::render()
{
    const bool known = !m_key.isEmpty() && !m_label.isEmpty();
    setActive(known && m_allowed);
    if (!m_switching.isEmpty() && m_switchKey == m_key) {
        setLabels(tr("Switching to %1…").arg(m_switchingLabel), tr("Switching…"));
        setDetail(tr("Moving this session from %1 to %2; the agent restarts into the same conversation.").arg(m_label, m_switchingLabel));
        setTone(ChipTone::Neutral); setEnabled(false);
        return;
    }
    setLabels(m_label);
    QStringList detail{tr("Account: %1").arg(m_label)};
    const bool elsewhere = switching();   // another session's switch runs
    if (elsewhere) detail << tr("Another session is changing its account; wait for it.");
    else if (!m_blocked.isEmpty()) detail << m_blocked;
    else if (!m_locked.isEmpty()) detail << m_locked;
    else if (switchable(m_provider)) detail << tr("Choose another signed-in account for this session");
    const auto outcome = lastOutcome();
    if (outcome.kind != Outcome::None) detail << outcome.text;
    setDetail(detail.join('\n'));
    setAccessibleName(tr("Account: %1").arg(m_label));
    // A chip that cannot be used now reads as plainly disabled.
    setTone(!m_blocked.isEmpty() || elsewhere ? ChipTone::Quiet : outcome.kind == Outcome::Refused ? ChipTone::Danger
            : outcome.kind == Outcome::RolledBack || outcome.kind == Outcome::Uncertain ? ChipTone::Warning : ChipTone::Quiet);
    setEnabled(known && m_blocked.isEmpty() && !elsewhere);
}
