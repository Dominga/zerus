#pragma once
#include "TeamModel.h"
#include "TeamWidgets.h"
#include <QHash>
#include <QJsonObject>
#include <QJsonValue>

// WhiteWh fork: agent teams, U38 (design/agent-teams/profile, the heat views).
// Graph and Board overlay the pipeline profile as a heat tint: the more a task
// lost, by time (waits and rework) or by tokens (rework and long tests), the
// warmer its node or card, with the loss on its second line and its top loss
// in the tooltip. The numbers are `hgs team profile`'s ranked losses of the
// plan, read in bounded pages. Off by default; the choice is kept per team.
namespace TeamUi {

// One ranked loss of a task, as `hgs team profile` gives it.
struct HeatLoss {
    QString kind;              // rework, lead_wait, permission_wait, blocked_chain, member_wait, long_test, merge, conflict
    qint64 seconds = 0;
    qint64 tokens = -1;        // -1: unknown or none
    QJsonObject detail;
};

struct Heat {
    QString metric;                       // "time" or "tokens"
    QHash<int, QList<HeatLoss>> losses;   // by task, the largest first
    bool complete = true;                 // every page was read
    bool tokensPartial = false;           // some spans' tokens are unknown
    int pages = 0;
    // A task's loss by the metric: seconds or tokens; 0 without any.
    qint64 lost(int task) const;
    // The largest loss of any task (the tokens scale's end).
    qint64 most() const;
    // 0 to 1: time against 2 h, tokens against the largest task's.
    double share(int task) const;
};

// Off, "time" or "tokens" for a team; off by default, kept in QSettings.
QString heatMode(const QString &team);
void setHeatMode(const QString &team, const QString &mode);

// The scale's end for time: two hours or more is the warmest.
constexpr qint64 HeatTimeScale = 2 * 3600;
// At most this many pages of 100 losses are read for one tint.
constexpr int HeatPages = 5;

// Adds one profile page to `heat`; the cursor of the next page, or null when
// the list is complete.
QJsonValue addHeatPage(Heat &heat, const QJsonObject &profile);

// "12k", "1.2M": a token count in short.
QString tokensText(qint64 tokens);
// What the loss was: "Rework: attempt 2 returned", "Waited for the lead".
QString heatLossName(const HeatLoss &loss);
// The second line's text: "lost 1h 47m", "lost 412k tokens", "no loss", or
// empty for a task the profile does not cover (a draft, a cancelled task).
QString heatLabel(const Heat &heat, const Teams::Task &task);
// The tooltip's lines: the loss and the task's top loss, then the next two.
QString heatTip(const Heat &heat, const Teams::Task &task);
// Whether a task takes the tint at all: drafts and cancelled tasks do not.
bool heatCovers(const Teams::Task &task);
// A node's or card's fill for `share`, and its loss text's color.
QColor heatFill(const Colors &c, double share);
QColor heatTextColor(const Colors &c, double share);

// A graph node takes the tint, the loss in the working time's place and the
// tooltip's lines; a task the profile does not cover keeps its node.
void applyHeat(TaskView::Node &node, const Heat &heat, const Teams::Task &task, const Colors &c);
// A Board card's loss ("teamBoardHeat"), nullptr when it shows none; its
// card's fill, the surface without a loss.
QLabel *heatBoardLabel(const Heat &heat, const Teams::Task &task, const Colors &c);
QColor heatCardFill(const Heat *heat, const Teams::Task &task, const Colors &c);

// The legend under the toolbar ("teamHeatLegend"): the metric, 0, the scale,
// its end, and the note that only the tint changes; or the reading and error
// states.
QWidget *heatLegend(const Colors &c, const QString &metric, const Heat *heat, bool reading, const QString &error);

}
