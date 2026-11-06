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

// Task 82 (owner, 2026-11-06: "heat is not telling"): a task's lateness
// against the plan (`hgs team profile` tasks[].lateness, task 81). Own
// lateness tints; lateness inherited from a predecessor running late is shown
// muted, "from #N"; a task before its planned start, or waiting on
// dependencies that run on schedule, is cold.
struct HeatLateness {
    bool known = false;          // the plan has every moment it needs
    QString unknown;             // why not, when it does not
    double plannedStart = -1, plannedEnd = -1, actualStart = -1, actualEnd = -1;   // seconds since the epoch; -1: none
    qint64 startLate = 0, inherited = 0, own = 0, endLate = -1;
    int from = 0;                // the predecessor the inherited part comes from
};

// Task 82: the task's own work against its estimate (tasks[].overrun), a
// measure of its own beside the lateness: on time but slow, or late but fast.
struct HeatOverrun {
    bool started = false;        // an attempt exists
    bool known = false;          // the task has an estimate
    qint64 estimate = 0, work = 0, overrun = 0;
    double ratio = 0;
    bool running = false;
};

struct Heat {
    QString metric;                       // "time" or "tokens"
    QHash<int, QList<HeatLoss>> losses;   // by task, the largest first
    QHash<int, HeatLateness> lateness;    // task 82: by task; absent without a baseline
    QHash<int, HeatOverrun> overrun;      // task 82: by task; absent before the profile names it
    bool complete = true;                 // every loss page was read
    bool tasksComplete = true;            // every task page was read
    bool tokensPartial = false;           // some spans' tokens are unknown
    int pages = 0;
    // A task's heat by the metric: own lateness (time) or tokens lost; 0 without any.
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

// The cursors of the next pages: the losses' and the tasks' (null: complete).
struct HeatCursors { QJsonValue losses, tasks; bool done() const { return (losses.isNull() || losses.isUndefined()) && (tasks.isNull() || tasks.isUndefined()); } };
// Adds one profile page to `heat` (only the lists not yet complete).
HeatCursors addHeatPage(Heat &heat, const QJsonObject &profile);
// Task 82: the overrun badge's text: "+40m over 1h", "within 1h", "no
// estimate"; `compact` (a graph node): "x1.7". Empty before the task started.
QString overrunText(const HeatOverrun &overrun, bool compact);
// Its own scale: the muted text at or under the estimate, warming to violet at twice it.
QColor overrunColor(const Colors &c, const HeatOverrun &overrun);
// The badge beside the lateness ("teamHeatOverrun"), nullptr when it shows none.
QLabel *heatOverrunBadge(const Heat &heat, const Teams::Task &task, const Colors &c);
// "HH:mm" today, "d MMM HH:mm" before.
QString heatMoment(double at);

// "12k", "1.2M": a token count in short.
QString tokensText(qint64 tokens);
// What the loss was: "Rework: attempt 2 returned", "Waited for the lead".
QString heatLossName(const HeatLoss &loss);
// The second line's text: "lost 1h 47m", "lost 412k tokens", "no loss", or
// empty for a task the profile does not cover (a draft, a cancelled task).
QString heatLabel(const Heat &heat, const Teams::Task &task);
// The tooltip's lines: the loss and the task's top loss, then the next two.
QString heatTip(const Heat &heat, const Teams::Task &task);
// #709 L6: what the tint draws in `c`, to tell a read that changes nothing
// drawn from one that does.
QByteArray heatDigest(const Heat &heat, const Colors &c);
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
