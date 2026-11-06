// WhiteWh fork, U38: Graph and Board overlay the pipeline profile as a heat
// tint (design/agent-teams/profile, the heat views): the Heat switch in the
// toolbar, kept per team, the legend under it and the profile's losses, read
// from `hgs team profile` in bounded pages.
#include "TeamWorkspace.h"
#include "TeamController.h"
#include "TeamHeat.h"
#include "TeamWidgets.h"
#include "WorkspaceIcons.h"
#include <QHBoxLayout>
#include <QLabel>
#include <QMenu>
#include <QPointer>
#include <QPushButton>

using namespace Teams;
using namespace TeamUi;

namespace {
// A whole plan's profile reads every task's journal window and, for Tokens,
// up to 200 session records.
constexpr int HeatTimeoutMs = 2 * 60 * 1000;
// Waits grow while tasks run: a shown tint is read again after a minute.
qint64 heatAgeMs = 60 * 1000;
}

void TeamWorkspace::setHeatAge(int milliseconds) { heatAgeMs = milliseconds; }

bool TeamWorkspace::heatView(const QString &view)
{
    return view == QLatin1String("graph") || view == QLatin1String("board") || view == QLatin1String("layers");
}

QWidget *TeamWorkspace::heatCardLine(const Team &team, int taskId)
{
    // Task 82: the drawer's task card shows the same two measures.
    const Heat *shown = heat(team);
    const Task *task = team.task(taskId);
    if (!shown || !task) return nullptr;
    const Colors c = colorsFor(m_dark);
    auto *late = heatBoardLabel(*shown, *task, c);
    auto *badge = heatOverrunBadge(*shown, *task, c);
    if (!late && !badge) return nullptr;
    auto *line = new QWidget; line->setObjectName("teamCardHeat");
    auto *h = new QHBoxLayout(line); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(6);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon(QStringLiteral("flame"), c.text2).pixmap(12, 12)); h->addWidget(icon);
    if (late) h->addWidget(late);
    if (badge) h->addWidget(badge);
    h->addStretch(1);
    line->setToolTip(heatTip(*shown, *task));
    return line;
}

QString TeamWorkspace::heatShown(const Team &team) const
{
    // A plan has no timeline yet. Task 82: Graph, Board and Layers take the tint.
    if (team.status == QLatin1String("planning") || !heatView(m_view)) return {};
    const QString mode = heatMode(team.id);
    return mode == QLatin1String("off") ? QString() : mode;
}

const Heat *TeamWorkspace::heat(const Team &team)
{
    const QString metric = heatShown(team);
    if (metric.isEmpty()) return nullptr;
    auto &read = m_heat[metric];
    if (read.team != team.id) { read = HeatRead{}; read.team = team.id; }
    if (!read.reading && (!read.at.isValid() || read.at.hasExpired(heatAgeMs))) readHeat(team.id, metric, QJsonValue(), std::make_shared<Heat>());
    return read.error.isEmpty() ? read.heat.get() : nullptr;
}

void TeamWorkspace::readHeat(const QString &team, const QString &metric, const QJsonValue &cursor, const std::shared_ptr<Heat> &into, const QJsonValue &tasks)
{
    auto &read = m_heat[metric];
    read.reading = true;
    const quint64 generation = ++read.generation;
    into->metric = metric;
    // Time reads no session; Tokens pages the losses by tokens.
    QJsonObject input{{"team", team}, {"tokens", metric == QLatin1String("tokens")}, {"limit", 100}};
    if (!cursor.isNull() && !cursor.isUndefined()) input.insert(metric == QLatin1String("tokens") ? "after_tokens" : "after_time", cursor);
    // Task 82: the task rows (lateness and overrun) page on their own cursor.
    if (!tasks.isNull() && !tasks.isUndefined()) input.insert("after_task", tasks);
    QPointer<TeamWorkspace> self(this);
    m_teams->call(QStringLiteral("profile"), input, [self, team, metric, into, generation](bool ok, const QJsonObject &result, const QString &error) {
        if (!self) return;
        auto &read = self->m_heat[metric];
        // Another team or a newer read since: this answer is stale.
        if (read.team != team || read.generation != generation) return;
        if (ok) {
            const HeatCursors next = addHeatPage(*into, result);
            if (!next.done() && into->pages < HeatPages) { self->readHeat(team, metric, next.losses, into, next.tasks); return; }
        }
        read.reading = false; read.at.start();
        read.error = ok ? QString() : (error.isEmpty() ? QObject::tr("no answer") : error.trimmed());
        if (ok) read.heat = into;
        // #709 L6: the read a minute later mostly draws what is shown; only
        // a changed tint, legend or error rebuilds the view.
        const QByteArray digest = ok ? heatDigest(*into, colorsFor(self->m_dark)) : "error\n" + read.error.toUtf8();
        if (digest == read.digest) return;
        read.digest = digest;
        self->scheduleRender();
    }, false, HeatTimeoutMs);
}

QWidget *TeamWorkspace::heatSwitch(const Team &team)
{
    if (team.status == QLatin1String("planning") || !heatView(m_view)) return nullptr;
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamWorkspaceHeatBox"); box->setProperty("toolbarLevels", "01");
    auto *h = new QHBoxLayout(box); h->setContentsMargins(0, 0, 0, 0); h->setSpacing(0);
    // The mock-up's "Heat  Off | Lateness | Tokens" while the toolbar has
    // room (ToolbarRow's first step), one flame button with the same three
    // choices at its second; the icon-only step leaves it out (the legend of
    // a shown tint stays).
    auto *wide = new QWidget; wide->setObjectName("teamWorkspaceHeatWide"); wide->setProperty("toolbarLevels", "0");
    auto *wh = new QHBoxLayout(wide); wh->setContentsMargins(0, 0, 0, 0); wh->setSpacing(6);
    wh->addWidget(label(tr("Heat"), c.text3, 11));
    const QString mode = heatMode(team.id);
    const auto pick = [this, id = team.id](const QString &value) { setHeatMode(id, value); scheduleRender(); };
    const QList<std::pair<QString, QString>> choices{{"off", tr("Off")}, {"time", tr("Lateness")}, {"tokens", tr("Tokens")}};
    auto *seg = segmented(choices, mode, "teamWorkspaceHeat", pick);
    auto *menuButton = button(QString(), "teamWorkspaceHeatMenu"); menuButton->setProperty("toolbarLevels", "1"); menuButton->hide();
    menuButton->setIcon(workspaceIcon(QStringLiteral("flame"), mode == QLatin1String("off") ? c.text2 : c.accent)); menuButton->setIconSize(QSize(13, 13));
    // As narrow as the other icon buttons: no menu arrow.
    menuButton->setStyleSheet(QStringLiteral("QPushButton#teamWorkspaceHeatMenu::menu-indicator{image:none;width:0px;}"));
    menuButton->setFixedWidth(menuButton->iconSize().width() + 16);
    const QString current = mode == QLatin1String("time") ? tr("Lateness") : mode == QLatin1String("tokens") ? tr("Tokens") : tr("Off");
    menuButton->setToolTip(tr("Heat: %1").arg(current)); menuButton->setAccessibleName(menuButton->toolTip());
    auto *menu = new QMenu(menuButton); menu->setObjectName("teamWorkspaceHeatChoices");
    for (const auto &[value, text] : choices) {
        auto *action = menu->addAction(text); action->setCheckable(true); action->setChecked(value == mode); action->setData(value);
        QObject::connect(action, &QAction::triggered, this, [pick, value = value] { pick(value); });
    }
    menuButton->setMenu(menu);
    // #755: Off, Lateness and Tokens as icons alone (a crossed-out flame, a
    // flame, two coins), each named with its purpose in its tooltip.
    for (auto *b : seg->findChildren<QPushButton *>()) {
        const QString value = b->property("segment").toString();
        b->setIcon(workspaceIcon(value == QLatin1String("time") ? QStringLiteral("flame") : value == QLatin1String("tokens") ? QStringLiteral("coins")
                                                                                                                      : QStringLiteral("heat-off"),
                                 b->isChecked() ? c.accent : c.text2));
        b->setIconSize(QSize(16, 16));
        const QString purpose = value == QLatin1String("off") ? tr("No tint")
                              : value == QLatin1String("time") ? tr("Tint each task by how late it is against the plan; its overrun shows beside it")
                                                               : tr("Tint each task by the tokens it lost to rework and long tests");
        b->setAccessibleName(tr("Heat: %1").arg(b->text())); b->setAccessibleDescription(purpose);
        b->setToolTip(b->text() + QLatin1Char('\n') + purpose);
        b->setText(QString()); b->setFixedWidth(b->iconSize().width() + 16);
    }
    wh->addWidget(seg);
    h->addWidget(wide); h->addWidget(menuButton);
    return box;
}

QWidget *TeamWorkspace::heatLegendRow(const Team &team)
{
    const QString metric = heatShown(team);
    if (metric.isEmpty()) return nullptr;
    const Heat *shown = heat(team);
    const auto &read = m_heat[metric];
    return heatLegend(colorsFor(m_dark), metric, shown, read.reading, read.error);
}
