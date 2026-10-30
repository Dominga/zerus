#include "TeamWaitingBanner.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

using namespace TeamUi;

namespace {
QColor kindColor(const Colors &c, const QString &kind)
{
    if (kind == "report_escalated" || kind == "permission_escalated") return c.warn;
    if (kind == "phase_draft" || kind == "plan_draft") return c.folder;
    if (kind == "owner_word_asked") return c.violet;
    if (kind == "team_finished") return c.accent;
    return c.text2;
}
}

TeamWaitingFeed::TeamWaitingFeed(TeamController *teams, QObject *parent) : QObject(parent), m_teams(teams)
{
    // Teams change at the pace of agents; a minute is soon enough for a banner.
    m_poll.setInterval(30000);
    QObject::connect(&m_poll, &QTimer::timeout, this, [this] { refresh(); });
}

void TeamWaitingFeed::setActive(bool active)
{
    if (active == m_poll.isActive()) return;
    if (!active) { m_poll.stop(); return; }
    m_poll.start();
    refresh();
}

void TeamWaitingFeed::refresh()
{
    if (!m_teams || m_inFlight) return;
    m_inFlight = true;
    const quint64 read = ++m_read;
    m_teams->call(QStringLiteral("waiting"), QJsonObject{{"limit", MaxItems}}, [this, read](bool ok, const QJsonObject &result, const QString &) {
        m_inFlight = false;
        // A failed read keeps what is shown; the next one tries again.
        if (!ok || read != m_read) return;
        m_items.clear();
        QList<TeamWaitingNotices::Item> seen;
        for (const auto &value : result["items"].toArray()) {
            const auto item = TeamWaitingPage::parse(value.toObject());
            m_items << item;
            seen.append({item.id, item.team, item.teamName, item.kind, item.title});
        }
        m_total = result["total"].toInt(int(m_items.size()));
        emit changed();
        auto notices = TeamWaitingNotices::stored();
        const auto fresh = notices.observe(seen, m_teams->teams(), !result["next"].isObject());
        notices.store();
        for (const auto &notice : fresh) emit notify(notice.token, notice.title, notice.body);
    }, false);
}

TeamWaitingBanner::TeamWaitingBanner(TeamWaitingFeed *feed, QWidget *parent) : QWidget(parent), m_feed(feed)
{
    setObjectName("teamWaitingBannerBox");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(10, 6, 10, 4); outer->setSpacing(0);
    m_frame = new QFrame; m_frame->setObjectName("teamWaitingBanner"); m_frame->setAttribute(Qt::WA_StyledBackground);
    outer->addWidget(m_frame);
    m_layout = new QVBoxLayout(m_frame); m_layout->setContentsMargins(10, 6, 8, 6); m_layout->setSpacing(4);
    if (feed) QObject::connect(feed, &TeamWaitingFeed::changed, this, [this] { render(); });
    setDark(true);
}

void TeamWaitingBanner::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    m_frame->setStyleSheet(QStringLiteral("QFrame#teamWaitingBanner{background:%1;border:1px solid %2;border-radius:8px;}"
                                 "QLabel#teamWaitingBannerTeam{color:%3;background:%4;border-radius:6px;padding:1px 7px;font-size:11px;}")
                      .arg(alpha(c.warn, .08).name(QColor::HexArgb), alpha(c.warn, .45).name(QColor::HexArgb), c.text2.name(), c.tile.name()));
    render();
}

void TeamWaitingBanner::setExpanded(bool expanded)
{
    if (m_expanded == expanded) return;
    m_expanded = expanded;
    render();
}

QString TeamWaitingBanner::actionText(const QString &action)
{
    // U55: Approve... and Accept... open the team at the item, where the
    // owner decides with the usual confirmations.
    if (action == "approve_phase" || action == "approve_plan") return QObject::tr("Approve…");
    if (action == "decide_report") return QObject::tr("Accept…");
    if (action == "decide_proposal") return QObject::tr("Answer");
    return QObject::tr("Open team");
}

QWidget *TeamWaitingBanner::rowFor(const TeamWaitingPage::Item &item, bool first)
{
    const Colors c = colorsFor(m_dark);
    const QColor tone = kindColor(c, item.kind);
    auto *line = new QWidget; line->setObjectName(first ? "teamWaitingBannerLine" : "teamWaitingBannerRow");
    line->setProperty("itemId", item.id); line->setProperty("kind", item.kind);
    auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
    auto *icon = new QLabel; icon->setObjectName("teamWaitingBannerIcon"); icon->setFixedSize(16, 16);
    icon->setPixmap(workspaceIcon(TeamWaitingPage::kindIcon(item.kind), tone).pixmap(16, 16));
    icon->setToolTip(TeamWaitingPage::kindLabel(item.kind));
    h->addWidget(icon);
    auto *title = elided(item.title, c.fg, 12); title->setObjectName("teamWaitingBannerTitle");
    h->addWidget(title, 1);
    auto *team = new QLabel(item.teamName); team->setObjectName("teamWaitingBannerTeam"); team->setTextFormat(Qt::PlainText);
    team->setToolTip(tr("Team %1").arg(item.teamName));
    h->addWidget(team);
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto *age = label(agoText(item.since, now), c.muted, 11, false, "teamWaitingBannerAge");
    age->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(item.since)).toString(QStringLiteral("yyyy-MM-dd HH:mm")));
    h->addWidget(age);
    auto *action = button(actionText(item.action), "teamWaitingBannerAction");
    action->setProperty("action", item.action);
    action->setAccessibleName(tr("%1 in team %2").arg(actionText(item.action), item.teamName));
    QObject::connect(action, &QPushButton::clicked, this, [this, item] { if (openItem) openItem(item); });
    h->addWidget(action);
    return line;
}

void TeamWaitingBanner::render()
{
    while (auto *child = m_layout->takeAt(0)) {
        if (auto *widget = child->widget()) retire(widget);
        delete child;
    }
    const auto items = m_feed ? m_feed->items() : QList<TeamWaitingPage::Item>{};
    const int total = m_feed ? qMax(m_feed->total(), int(items.size())) : 0;
    setProperty("count", total);
    // Hidden while nothing waits (setHidden keeps a hidden parent's children
    // from showing before the parent does).
    setHidden(items.isEmpty());
    if (items.isEmpty()) return;
    if (items.size() == 1) { m_layout->addWidget(rowFor(items.first(), true)); return; }
    // Several fold into one line: the count, the newest and a toggle.
    const Colors c = colorsFor(m_dark);
    auto *head = new QWidget; head->setObjectName("teamWaitingBannerHead");
    auto *h = new QHBoxLayout(head); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(8);
    auto *icon = new QLabel; icon->setFixedSize(16, 16); icon->setPixmap(workspaceIcon("inbox", c.warn).pixmap(16, 16));
    h->addWidget(icon);
    auto *count = label(tr("%1 wait for you").arg(total), c.fg, 12, true, "teamWaitingBannerCount");
    h->addWidget(count);
    const auto &newest = items.first();
    auto *title = elided(newest.title, c.text2, 12); title->setObjectName("teamWaitingBannerNewest");
    h->addWidget(title, 1);
    h->addWidget(label(agoText(newest.since, double(QDateTime::currentSecsSinceEpoch())), c.muted, 11, false, "teamWaitingBannerAge"));
    auto *toggle = button(m_expanded ? tr("Hide") : tr("Show all"), "teamWaitingBannerToggle");
    toggle->setProperty("ghost", true);
    toggle->setIcon(workspaceIcon(m_expanded ? "chevron-down" : "chevron-right", c.text2));
    QObject::connect(toggle, &QPushButton::clicked, this, [this] { setExpanded(!m_expanded); });
    h->addWidget(toggle);
    m_layout->addWidget(head);
    if (!m_expanded) return;
    for (int i = 0; i < items.size() && i < MaxRows; ++i) m_layout->addWidget(rowFor(items[i], false));
    if (total > MaxRows) {
        auto *all = button(tr("Open the waiting list (%1)").arg(total), "teamWaitingBannerAll");
        all->setProperty("ghost", true);
        QObject::connect(all, &QPushButton::clicked, this, [this] { if (openAll) openAll(); });
        auto *line = new QWidget; line->setObjectName("teamWaitingBannerMore");
        auto *lh = new QHBoxLayout(line); lh->setContentsMargins(24, 0, 0, 0); lh->addWidget(all); lh->addStretch();
        m_layout->addWidget(line);
    }
}
