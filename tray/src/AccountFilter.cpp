#include "AccountFilter.h"
#include "ScopeFilterWidgets.h"
#include "WorkspaceIcons.h"
#include <QLabel>
#include <QMenu>
#include <QPushButton>
#include <QScrollArea>
#include <QSignalBlocker>
#include <QVBoxLayout>
#include <QWidgetAction>

AccountFilter::AccountFilter(QWidget *parent) : QWidget(parent) {
    setObjectName("accountFilterChips"); setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Minimum);
    m_flow = new ScopeFilterWidgets::ChipFlow(this);
    m_button = new QPushButton; m_button->setObjectName("accountFilter"); m_button->setFixedHeight(30); m_button->setMinimumWidth(28);
    m_button->setAttribute(Qt::WA_LayoutUsesWidgetRect); m_button->setIconSize(QSize(16,16));
    m_button->setAccessibleName(tr("Filter sessions by accounts"));
    m_menu = new QMenu(m_button); m_menu->setObjectName("accountFilterMenu"); m_button->setMenu(m_menu);
    connect(m_menu, &QMenu::aboutToShow, this, [this] { emit opened(); rebuildMenu(); });
    rebuildChips();
}
QString AccountFilter::caption(const SessionAccounts::Account &account) const {
    QString text = account.provider.isEmpty() ? account.name : account.name + " · " + IdentityBadges::providerName(account.provider);
    const auto rows = choices();
    if (std::any_of(rows.cbegin(), rows.cend(), [&account](const auto &other) {
        return other.id != account.id && other.name == account.name && other.provider == account.provider;
    })) {
        QStringList profiles;
        for (const auto &binding : account.bindings) {
            const auto fields = QJsonDocument::fromJson(binding.toUtf8()).array();
            if (fields.size() != 4) continue;
            const auto host = fields[0].toString(), profile = fields[3].toString();
            profiles.append((host.isEmpty() ? tr("This computer") : host) + " / " + (profile.isEmpty() ? tr("Not recorded") : profile));
        }
        profiles.sort(); text += " · " + profiles.join(", ");
    }
    return text;
}
QList<SessionAccounts::Account> AccountFilter::choices() const {
    auto rows = m_accounts; QSet<QString> known;
    QHash<QString,int> positions;
    for (int i = 0; i < rows.size(); ++i) { known.unite(rows[i].bindings); positions.insert(rows[i].id, i); }
    for (const auto &binding : m_selection) if (!known.contains(binding)) {
        auto account = m_selectedAccounts.value(binding);
        if (account.id.isEmpty()) account = {binding, tr("Unavailable account"), {}, {}, {binding}};
        if (positions.contains(account.id)) rows[positions.value(account.id)].bindings.insert(binding);
        else { account.bindings = {binding}; positions.insert(account.id, rows.size()); rows.append(account); }
    }
    return rows;
}
void AccountFilter::setSelection(const QSet<QString> &bindings) {
    if (m_selection == bindings) return;
    for (const auto &account : choices()) for (const auto &binding : account.bindings)
        if (bindings.contains(binding)) m_selectedAccounts.insert(binding, account);
    for (auto it = m_selectedAccounts.begin(); it != m_selectedAccounts.end();) {
        if (!bindings.contains(it.key())) it = m_selectedAccounts.erase(it); else ++it;
    }
    m_selection = bindings; rebuildChips(); updateMenu(); emit selectionChanged(bindings);
}
void AccountFilter::toggle(const SessionAccounts::Account &account, bool checked) {
    auto next = m_selection;
    if (checked) next.unite(account.bindings); else next.subtract(account.bindings);
    setSelection(next);
}
void AccountFilter::setAccounts(const QList<SessionAccounts::Account> &accounts) {
    m_accounts = accounts;
    for (const auto &account : accounts) for (const auto &binding : account.bindings)
        if (m_selection.contains(binding)) m_selectedAccounts.insert(binding, account);
    for (auto it = m_selectedAccounts.begin(); it != m_selectedAccounts.end();) {
        if (!m_selection.contains(it.key())) it = m_selectedAccounts.erase(it); else ++it;
    }
    rebuildChips(); updateMenu();
}
void AccountFilter::setSessionCounts(const QMap<QString,int> &counts, const QString &caption) { m_counts = counts; m_caption = caption; updateMenu(); }
void AccountFilter::setTheme(bool dark) { m_dark = dark; m_signature.clear(); rebuildChips(); }
void AccountFilter::setConcealed(bool concealed) { m_concealed = concealed; setVisible(!concealed && !m_selection.isEmpty()); }
void AccountFilter::rebuildChips() {
    const auto rows = choices(); QString signature = QString::number(m_dark); int count = 0;
    for (const auto &account : rows) if (account.bindings.intersects(m_selection)) { ++count; signature += '\n' + account.id + caption(account); }
    QStringList bindings(m_selection.cbegin(), m_selection.cend()); bindings.sort(); signature += bindings.join('\n');
    m_button->setIcon(workspaceIcon("accounts", QColor(m_selection.isEmpty() ? (m_dark ? "#a1adbb" : "#647386") : (m_dark ? "#8bdfc0" : "#167357"))));
    m_button->setText(count ? QString::number(count) : QString());
    m_button->setToolTip(count ? tr("%1 accounts selected; click to change").arg(count) : tr("All accounts: choose one or more"));
    setVisible(!m_concealed && count);
    if (signature == m_signature) return;
    m_signature = signature;
    while (auto *item = m_flow->takeAt(0)) { item->widget()->hide(); item->widget()->deleteLater(); delete item; }
    for (const auto &account : rows) if (account.bindings.intersects(m_selection)) {
        auto *chip = new QPushButton; chip->setObjectName("accountFilterChip"); chip->setProperty("account", account.id);
        chip->setAttribute(Qt::WA_LayoutUsesWidgetRect); chip->setAutoDefault(false); chip->setFixedHeight(27);
        chip->setText(chip->fontMetrics().elidedText(caption(account), Qt::ElideMiddle, 190).replace('&', "&&") + "  ×");
        chip->setIcon(workspaceIcon(account.provider, IdentityBadges::providerColor(account.provider, m_dark))); chip->setIconSize(QSize(14,14));
        chip->setToolTip(tr("Remove %1 from account filter").arg(caption(account))); chip->setAccessibleName(chip->toolTip());
        chip->setStyleSheet(QString("QPushButton { background:%1; color:%2; border:1px solid %3; border-radius:7px; padding:0 7px; min-height:25px; font-size:11px; }")
            .arg(m_dark ? "#233d35" : "#e5f3ed", m_dark ? "#8bdfc0" : "#167357", m_dark ? "#3a6253" : "#a9cdbc"));
        connect(chip, &QPushButton::clicked, this, [this,account] { toggle(account, false); }); m_flow->addWidget(chip);
    }
    m_flow->invalidate(); updateGeometry();
}
void AccountFilter::rebuildMenu() {
    m_menu->clear(); auto *panel = new QWidget; auto *layout = new QVBoxLayout(panel); layout->setContentsMargins(10,8,10,8); layout->setSpacing(4);
    auto *title = new QLabel(tr("Choose accounts")); layout->addWidget(title);
    auto *scope = new QLabel; scope->setObjectName("accountFilterCountScope"); scope->setStyleSheet("font-size:10px;"); layout->addWidget(scope);
    auto *all = new QPushButton; all->setObjectName("allAccountFilters"); all->setCheckable(true); layout->addWidget(all);
    connect(all, &QPushButton::clicked, this, [this] { setSelection({}); });
    auto *choicesWidget = new QWidget; auto *rows = new QVBoxLayout(choicesWidget); rows->setContentsMargins(0,0,0,0); rows->setSpacing(4);
    for (const auto &account : choices()) {
        auto *check = new ScopeFilterWidgets::Choice(caption(account), m_dark); check->setObjectName("accountFilterChoice"); check->setProperty("account", account.id);
        check->setProperty("bindings", QStringList(account.bindings.cbegin(), account.bindings.cend()));
        check->setIcon(workspaceIcon(account.provider, IdentityBadges::providerColor(account.provider, m_dark))); rows->addWidget(check);
        connect(check, &QCheckBox::clicked, this, [this,account](bool checked) { toggle(account, checked); });
    }
    auto *scroll = new QScrollArea; scroll->setObjectName("accountFilterChoices"); scroll->setWidgetResizable(true); scroll->setFrameShape(QFrame::NoFrame);
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff); scroll->setWidget(choicesWidget);
    scroll->setMinimumWidth(qMin(520, choicesWidget->sizeHint().width() + 24)); scroll->setFixedHeight(qMin(324, choicesWidget->sizeHint().height() + 4)); layout->addWidget(scroll);
    auto *hint = new QLabel(tr("Changes apply immediately")); hint->setStyleSheet("font-size:10px;"); layout->addWidget(hint);
    auto *action = new QWidgetAction(m_menu); action->setDefaultWidget(panel); m_menu->addAction(action); updateMenu();
}
void AccountFilter::updateMenu() {
    int total = 0; for (auto count : m_counts) total += count;
    if (auto *scope = m_menu->findChild<QLabel *>("accountFilterCountScope")) scope->setText(tr("%1 before account selection and search").arg(m_caption));
    if (auto *all = m_menu->findChild<QPushButton *>("allAccountFilters")) { all->setText(tr("All accounts (%1)").arg(total)); all->setChecked(m_selection.isEmpty()); }
    const auto rows = choices();
    for (auto *check : m_menu->findChildren<QCheckBox *>("accountFilterChoice")) {
        // Keep the open menu's bound choices stable until it is opened again.
        const auto members = check->property("bindings").toStringList(); QSet<QString> bindings(members.cbegin(), members.cend());
        const auto found = std::find_if(rows.cbegin(), rows.cend(), [&bindings](const auto &account) { return account.bindings.intersects(bindings); });
        if (found == rows.cend()) continue;
        int count = 0; for (const auto &binding : bindings) count += m_counts.value(binding);
        const bool any = bindings.intersects(m_selection), all = (m_selection & bindings) == bindings;
        const QSignalBlocker blocker(check); check->setTristate(any && !all);
        check->setCheckState(all ? Qt::Checked : any ? Qt::PartiallyChecked : Qt::Unchecked);
        check->setText(tr("%1 (%2)").arg(check->fontMetrics().elidedText(caption(*found), Qt::ElideMiddle, 360).replace('&', "&&")).arg(count));
        check->setToolTip(caption(*found) + '\n' + found->machines.join(", ")); check->setProperty("sessionCount", count);
        check->setAccessibleName(tr("%1, %2 sessions").arg(caption(*found)).arg(count));
    }
}
