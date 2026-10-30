#include "TeamWaitingPage.h"
#include "TeamWaitingAnswer.h"
#include "TeamController.h"
#include "TeamWidgets.h"
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QMouseEvent>
#include <QScrollArea>
#include <QScrollBar>
#include <QSet>
#include <QVBoxLayout>
#include <algorithm>

using namespace TeamUi;

namespace {
// A row opens its team on a click anywhere in it, as its Open button does.
class WaitingRow : public QFrame {
public:
    std::function<void()> clicked;
protected:
    void mouseReleaseEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton && rect().contains(event->position().toPoint()) && clicked) clicked();
        QFrame::mouseReleaseEvent(event);
    }
};

QColor kindColor(const Colors &c, const QString &kind)
{
    if (kind == "report_escalated" || kind == "permission_escalated") return c.warn;
    if (kind == "phase_draft" || kind == "plan_draft") return c.folder;
    if (kind == "owner_word_asked") return c.violet;
    if (kind == "team_finished") return c.accent;
    return c.text2;
}

QList<TeamWaitingPage::Item> parseItems(const QJsonArray &items)
{
    QList<TeamWaitingPage::Item> out;
    for (const auto &value : items) out << TeamWaitingPage::parse(value.toObject());
    return out;
}
}

TeamWaitingPage::Item TeamWaitingPage::parse(const QJsonObject &item)
{
    Item out;
    out.id = item["id"].toString(); out.team = item["team"].toString(); out.teamName = item["team_name"].toString();
    out.kind = item["kind"].toString(); out.title = item["title"].toString(); out.action = item["action"].toString();
    out.member = item["member"].toString(); out.proposal = item["proposal"].toString(); out.since = item["since"].toDouble();
    out.memberId = item["member_id"].toString();
    out.phase = item["phase"].toInt(); out.task = item["task"].toInt();
    if (out.teamName.isEmpty()) out.teamName = out.team;
    return out;
}

QString TeamWaitingPage::kindLabel(const QString &kind)
{
    if (kind == "phase_draft") return tr("Phase draft");
    if (kind == "plan_draft") return tr("Plan");
    if (kind == "report_escalated") return tr("Report");
    if (kind == "permission_escalated") return tr("Permission");
    if (kind == "owner_word_asked") return tr("Your word");
    if (kind == "task_waits_owner") return tr("Held task");
    if (kind == "team_finished") return tr("All decided");
    return kind;
}

QString TeamWaitingPage::kindIcon(const QString &kind)
{
    if (kind == "phase_draft") return QStringLiteral("pencil");
    if (kind == "plan_draft") return QStringLiteral("eye");
    if (kind == "report_escalated") return QStringLiteral("message");
    if (kind == "permission_escalated") return QStringLiteral("shield");
    if (kind == "owner_word_asked") return QStringLiteral("key");
    if (kind == "task_waits_owner") return QStringLiteral("clock");
    if (kind == "team_finished") return QStringLiteral("check-circle");
    return QStringLiteral("warning");
}

TeamWaitingPage::TeamWaitingPage(TeamController *teams, QWidget *parent) : QWidget(parent), m_teams(teams)
{
    setObjectName("teamWaitingPage");
    auto *outer = new QVBoxLayout(this); outer->setContentsMargins(0, 0, 0, 0); outer->setSpacing(0);
    auto *header = new QWidget; header->setObjectName("teamWaitingHeader");
    auto *h = new QHBoxLayout(header); h->setContentsMargins(20, 14, 20, 14); h->setSpacing(10);
    m_tile = new QLabel; m_tile->setObjectName("teamWaitingTile"); m_tile->setFixedSize(28, 28); m_tile->setAlignment(Qt::AlignCenter);
    m_title = new QLabel(tr("Waiting for you")); m_title->setObjectName("teamWaitingTitle");
    m_count = new QLabel; m_count->setObjectName("teamWaitingCount");
    m_refresh = button(tr("Refresh"), "teamWaitingRefresh"); m_refresh->setProperty("ghost", true);
    QObject::connect(m_refresh, &QPushButton::clicked, this, [this] { refresh(); });
    h->addWidget(m_tile); h->addWidget(m_title); h->addWidget(m_count, 1); h->addWidget(m_refresh);
    outer->addWidget(header);
    m_scroll = new QScrollArea; m_scroll->setObjectName("teamWaitingScroll"); m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame); m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_list = new QWidget; m_list->setObjectName("teamWaitingList");
    m_rows = new QVBoxLayout(m_list); m_rows->setContentsMargins(20, 14, 20, 20); m_rows->setSpacing(8);
    m_scroll->setWidget(m_list);
    outer->addWidget(m_scroll, 1);
    // The page reads only while it shows; teams change at the pace of agents.
    m_poll.setInterval(15000);
    QObject::connect(&m_poll, &QTimer::timeout, this, [this] { refresh(); });
    setDark(true);
}

void TeamWaitingPage::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    m_poll.start();
    refresh();
}

void TeamWaitingPage::hideEvent(QHideEvent *event)
{
    m_poll.stop();
    QWidget::hideEvent(event);
}

void TeamWaitingPage::setDark(bool dark)
{
    m_dark = dark;
    const Colors c = colorsFor(dark);
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    setStyleSheet(teamSheet(c) + QStringLiteral(
        "QWidget#teamWaitingPage,QScrollArea#teamWaitingScroll,QWidget#teamWaitingList{background:%1;}"
        "QWidget#teamWaitingHeader{background:%1;border-bottom:1px solid %2;}"
        "QLabel#teamWaitingTitle{color:%3;font-size:17px;font-weight:600;}"
        "QLabel#teamWaitingCount{color:%4;font-size:12px;}"
        "QLabel#teamWaitingTile{background:%5;border-radius:7px;}"
        "QFrame#teamWaitingRow{background:%6;border:1px solid %7;border-radius:10px;}"
        "QFrame#teamWaitingRow:hover{border-color:%8;}"
        "QLabel#teamWaitingTeam{color:%9;background:%10;border-radius:6px;padding:1px 7px;font-size:11px;}"
        "QLabel#teamWaitingEmptyTitle{color:%3;font-size:15px;font-weight:600;}"
        "QLabel#teamWaitingEmptyText{color:%4;font-size:12px;}")
        .arg(c.bg.name(), c.border.name(), c.fg.name(), c.muted.name(), a(c.accent, .18), c.cardFill.name(), c.cardBorder.name(),
             a(c.accent, .6), c.text2.name(), c.tile.name()));
    m_tile->setPixmap(workspaceIcon("inbox", c.accent).pixmap(16, 16));
    m_refresh->setIcon(workspaceIcon("refresh", c.text2));
    for (const auto &open : std::as_const(m_answers)) if (open) open->setDark(dark);
    render();
}

void TeamWaitingPage::refresh()
{
    if (!m_teams) return;
    const quint64 read = ++m_headRead;
    // A poll reads every item on screen and a page more (at most 200, the
    // server's bound), so an arrival never pushes a shown item out of view.
    const int limit = qMin(int(m_head.size() + m_older.size()) + PageSize, 200);
    m_teams->call(QStringLiteral("waiting"), QJsonObject{{"limit", limit}}, [this, read](bool ok, const QJsonObject &result, const QString &error) {
        if (read != m_headRead) return;
        if (!ok) { m_error = error; render(); return; }
        // An unchanged answer rebuilds nothing, but the ages move once a minute.
        if (result == m_lastHead && m_error.isEmpty() && m_rendered.isValid() && m_rendered.elapsed() < 60000) return;
        m_lastHead = result; m_error.clear();
        const QList<Item> head = parseItems(result["items"].toArray());
        const bool complete = !result["next"].isObject();
        QSet<QString> ids; for (const auto &item : head) ids.insert(item.id);
        // Newest first, ties by id, as hgs orders them.
        const auto sortsBefore = [](const Item &a, const Item &b) { return a.since > b.since || (a.since == b.since && a.id < b.id); };
        const auto covered = [&](const Item &item) { return complete || (!head.isEmpty() && !sortsBefore(head.last(), item)); };
        if (!m_older.isEmpty()) {
            // A shown item past the new page's end may have been pushed out by
            // arrivals: the older pages start again rather than skip it.
            const bool gap = std::any_of(m_head.cbegin(), m_head.cend(), [&](const Item &item) { return !ids.contains(item.id) && !covered(item); });
            if (gap) { m_older.clear(); ++m_olderRead; }
            // The page holds an older item now, or it holds its place and the
            // item is decided: shown once, or not at all.
            else m_older.erase(std::remove_if(m_older.begin(), m_older.end(), [&](const Item &item) { return ids.contains(item.id) || covered(item); }), m_older.end());
        }
        m_head = head;
        m_total = result["total"].toInt(m_head.size());
        if (m_older.isEmpty()) m_headNext = result["next"];
        if (countChanged) countChanged(m_total);
        render();
    }, false);
}

void TeamWaitingPage::loadOlder()
{
    const QJsonValue cursor = m_older.isEmpty() ? m_headNext : m_olderNext;
    if (!m_teams || m_loadingOlder || !cursor.isObject()) return;
    m_loadingOlder = true; render();
    const quint64 read = m_olderRead;
    m_teams->call(QStringLiteral("waiting"), QJsonObject{{"limit", PageSize}, {"before", cursor}}, [this, read](bool ok, const QJsonObject &result, const QString &error) {
        m_loadingOlder = false;
        // The older pages started again after this read was sent.
        if (read != m_olderRead) { render(); return; }
        if (!ok) { m_error = error; render(); return; }
        QSet<QString> ids; for (const auto &item : std::as_const(m_items)) ids.insert(item.id);
        for (const auto &item : parseItems(result["items"].toArray())) if (!ids.contains(item.id)) m_older << item;
        m_olderNext = result["next"];
        render();
    }, false);
}

TeamWaitingAnswer *TeamWaitingPage::answerOf(const QString &id) const
{
    return m_answers.value(id).data();
}

TeamWaitingAnswer *TeamWaitingPage::answer(const Item &item)
{
    if (auto *found = m_answers.value(item.id).data()) return found;
    auto *made = new TeamWaitingAnswer(m_teams, item, m_dark);
    made->answered = [this](const Item &done) {
        const Item copy = done;
        drop(copy.id);
        if (answered) answered(copy);
    };
    made->closed = [this, id = item.id] { closeAnswer(id); };
    m_answers.insert(item.id, made);
    return made;
}

void TeamWaitingPage::closeAnswer(const QString &id)
{
    if (auto found = m_answers.take(id)) found->deleteLater();
    render();
}

void TeamWaitingPage::drop(const QString &id)
{
    const auto gone = [&id](const Item &item) { return item.id == id; };
    const auto before = m_head.size() + m_older.size();
    m_head.erase(std::remove_if(m_head.begin(), m_head.end(), gone), m_head.end());
    m_older.erase(std::remove_if(m_older.begin(), m_older.end(), gone), m_older.end());
    if (auto found = m_answers.take(id)) found->deleteLater();
    if (m_head.size() + m_older.size() == before) return;
    m_total = qMax(0, m_total - 1);
    // The next read draws the list as the store has it.
    m_lastHead = {};
    if (countChanged) countChanged(m_total);
    render();
}

QWidget *TeamWaitingPage::rowFor(const Item &item)
{
    const Colors c = colorsFor(m_dark);
    const QColor tone = kindColor(c, item.kind);
    const bool proposal = item.action == "decide_proposal";
    const QString kindName = proposal ? tr("Proposal") : kindLabel(item.kind);
    // #724: a decision the owner takes in one click is answered in the row.
    const bool inPlace = TeamWaitingAnswer::answersInPlace(item.action);
    auto *frame = new WaitingRow; frame->setObjectName("teamWaitingRow"); frame->setAttribute(Qt::WA_StyledBackground);
    frame->setProperty("kind", item.kind); frame->setProperty("team", item.team); frame->setProperty("itemId", item.id);
    frame->setCursor(Qt::PointingHandCursor);
    auto *rowBox = new QVBoxLayout(frame); rowBox->setContentsMargins(12, 9, 10, 9); rowBox->setSpacing(0);
    auto *h = new QHBoxLayout; h->setContentsMargins(0, 0, 0, 0); h->setSpacing(10);
    rowBox->addLayout(h);
    auto *icon = new QLabel; icon->setObjectName("teamWaitingKindIcon"); icon->setFixedSize(18, 18);
    icon->setPixmap(workspaceIcon(kindIcon(item.kind), tone).pixmap(18, 18)); icon->setToolTip(kindName);
    h->addWidget(icon, 0, Qt::AlignTop);
    auto *text = new QVBoxLayout; text->setContentsMargins(0, 0, 0, 0); text->setSpacing(4);
    auto *title = elided(item.title, c.fg, 13); title->setObjectName("teamWaitingItemTitle");
    text->addWidget(title);
    auto *meta = row(8);
    auto *kind = label(kindName, tone, 11, true, "teamWaitingKind");
    auto *team = new QLabel(item.teamName); team->setObjectName("teamWaitingTeam"); team->setTextFormat(Qt::PlainText);
    team->setToolTip(tr("Team %1").arg(item.teamName));
    meta->addWidget(kind); meta->addWidget(team); meta->addStretch();
    text->addLayout(meta);
    h->addLayout(text, 1);
    const double now = double(QDateTime::currentSecsSinceEpoch());
    auto *since = label(agoText(item.since, now), c.muted, 11, false, "teamWaitingSince");
    since->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(item.since)).toString(QStringLiteral("yyyy-MM-dd HH:mm")));
    h->addWidget(since);
    TeamWaitingAnswer *shown = m_answers.value(item.id).data();
    if (TeamWaitingAnswer::approves(item.action)) {
        auto *approve = button(tr("Approve…"), "teamWaitingApprove", true);
        approve->setAccessibleName(tr("Approve in team %1").arg(item.teamName));
        approve->setToolTip(item.kind == "plan_draft" ? tr("Approve the plan and run it; the team's question comes first")
                                                      : tr("Approve the phase; the team's question comes first"));
        approve->setEnabled(!shown || !shown->busy());
        QObject::connect(approve, &QPushButton::clicked, this, [this, item] {
            auto *made = answer(item);
            render();
            made->approve();
        });
        h->addWidget(approve);
    } else if (TeamWaitingAnswer::opensCard(item.action)) {
        auto *toggle = button(shown ? tr("Close") : tr("Answer"), "teamWaitingAnswerToggle", !shown);
        toggle->setAccessibleName(tr("Answer in team %1").arg(item.teamName));
        toggle->setEnabled(!shown || !shown->busy());
        QObject::connect(toggle, &QPushButton::clicked, this, [this, item] {
            if (m_answers.value(item.id)) closeAnswer(item.id);
            else { answer(item); render(); }
        });
        h->addWidget(toggle);
    }
    auto *open = button(tr("Open team"), "teamWaitingOpen");
    if (inPlace) open->setProperty("ghost", true);
    open->setAccessibleName(tr("Open team %1").arg(item.teamName));
    h->addWidget(open);
    const auto go = [this, item] { if (openTeam) openTeam(item); };
    QObject::connect(open, &QPushButton::clicked, frame, go);
    // A click in an open answer stays there.
    frame->clicked = [this, id = item.id, go] { if (!m_answers.value(id)) go(); };
    if (shown) {
        rowBox->addWidget(shown);
        shown->setVisible(shown->hasContent());
    }
    return frame;
}

void TeamWaitingPage::render()
{
    const Colors c = colorsFor(m_dark);
    const int scrolled = m_scroll->verticalScrollBar()->value();
    // #724: an open answer keeps its typed words and its question across the
    // rebuild; one whose item is gone goes too, unless it is still asking.
    for (const auto &open : std::as_const(m_answers)) if (open) { open->hide(); open->setParent(nullptr); }
    while (auto *child = m_rows->takeAt(0)) {
        if (auto *widget = child->widget()) retire(widget);
        delete child;
    }
    m_items = m_head + m_older;
    QSet<QString> listed;
    for (const auto &item : std::as_const(m_items)) listed.insert(item.id);
    for (auto it = m_answers.begin(); it != m_answers.end();) {
        if (!*it) { it = m_answers.erase(it); continue; }
        if (listed.contains(it.key()) || (*it)->busy()) { ++it; continue; }
        (*it)->deleteLater();
        it = m_answers.erase(it);
    }
    if (m_total < 0) m_count->setText(m_error.isEmpty() ? tr("Reading the teams") : QString());
    else {
        QSet<QString> teams; for (const auto &item : std::as_const(m_items)) teams.insert(item.team);
        const int shown = int(teams.size());
        m_count->setText(m_total == 0 ? tr("Nothing waits for you")
                         : (m_total == 1 ? tr("1 item") : tr("%1 items").arg(m_total)) + QStringLiteral("   ")
                               + (shown == 1 ? tr("from 1 team shown") : tr("from %1 teams shown").arg(shown)));
    }
    if (!m_error.isEmpty()) m_rows->addWidget(refusal(tr("The waiting list was not read: %1").arg(m_error), c, "teamWaitingError"));
    if (m_total == 0 && m_items.isEmpty()) {
        auto *empty = new QWidget; empty->setObjectName("teamWaitingEmpty");
        auto *v = new QVBoxLayout(empty); v->setContentsMargins(0, 60, 0, 0); v->setSpacing(8);
        auto *icon = new QLabel; icon->setPixmap(workspaceIcon("check-circle", c.accent).pixmap(36, 36)); icon->setAlignment(Qt::AlignCenter);
        auto *title = new QLabel(tr("Nothing waits for you")); title->setObjectName("teamWaitingEmptyTitle"); title->setAlignment(Qt::AlignCenter);
        auto *text = new QLabel(tr("Draft phases and plans, reports and permissions escalated to you, the lead's proposals, "
                                   "tasks you hold and teams with every task decided show here, from every team."));
        text->setObjectName("teamWaitingEmptyText"); text->setAlignment(Qt::AlignCenter); text->setWordWrap(true);
        v->addWidget(icon); v->addWidget(title); v->addWidget(text);
        m_rows->addWidget(empty);
    }
    for (const auto &item : std::as_const(m_items)) m_rows->addWidget(rowFor(item));
    const QJsonValue next = m_older.isEmpty() ? m_headNext : m_olderNext;
    if (next.isObject()) {
        auto *older = button(m_loadingOlder ? tr("Loading") : tr("Show older"), "teamWaitingOlder");
        older->setEnabled(!m_loadingOlder);
        QObject::connect(older, &QPushButton::clicked, this, [this] { loadOlder(); });
        auto *line = new QWidget; line->setObjectName("teamWaitingOlderRow");
        auto *centered = new QHBoxLayout(line); centered->setContentsMargins(0, 4, 0, 0);
        centered->addStretch(); centered->addWidget(older); centered->addStretch();
        m_rows->addWidget(line);
    }
    m_rows->addStretch(1);
    m_scroll->verticalScrollBar()->setValue(scrolled);
    m_rendered.start();
}
