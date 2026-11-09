#pragma once
#include "TeamModel.h"
#include <QDateTime>
#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QPointer>
#include <QWidget>
#include <functional>

class TeamController;
class QComboBox;
class QLabel;
class QPushButton;
class QScrollArea;
class QStackedWidget;
class QVBoxLayout;

// WhiteWh fork, U38 (#665): the Profile view of the team workspace, beside
// Graph, Board and Layers (design/agent-teams/profile). It reads `hgs team
// profile` (a phase or the plan: summary, ranked losses, critical paths,
// members, roles, models and the task rows) and `hgs team spans` (each
// task's timeline, in pages) in the background.
namespace TeamProfile {

// One span of a task's timeline (spans.rs): work, rework, a stage, a wait.
struct Span {
    int task = 0, attempt = 0;
    QString kind, member, stageKind;
    QString record;   // #719: a test run's record ("tests": member is its suite, stageKind its status)
    double from = 0, to = 0;
    bool running = false;
    double seconds() const { return to - from; }
};

// A member's lane: its own spans, the waits of the tasks it was given, and
// the moments it had no work while a task was open.
struct Lane {
    QString member, role, model;
    double utilization = 0;
    qint64 busy = 0, idleSeconds = 0;
    QList<Span> spans, waits;
    QList<std::pair<double, double>> idle;
};

// What the view draws, built once per read (never per paint).
struct Model {
    QJsonObject profile;
    double from = 0, to = 0;                  // the scope's window
    QList<Lane> lanes;
    QList<Span> real, planned;                // the critical paths as bars
    QList<Span> tests;                        // U62 (#719): the test runs of the window, oldest first
    qint64 plannedSeconds = 0, realSeconds = 0;
    QHash<int, QString> titles;               // task id -> title
    QHash<int, QString> assignees;
    // Owner 2026-10-31 (#746): each task's estimate (-1: none) against its
    // working time (work and rework) in the scope.
    struct Estimate { int task = 0; double estimate = -1, worked = 0; bool running = false; };
    QList<Estimate> estimates;
    bool truncated = false;                   // spans past the page bound
    int tasks = 0;
    static Model build(const Teams::Team &team, const QJsonObject &profile, const QJsonArray &timelines, double now,
                       const QJsonArray &tests = {});
};

// Task #79 (owner, 2026-11-06: "All colours must be clearly readable"): one
// fixed palette, the same in every skin. Every kind has its own colour in
// each theme (CIE76 distance of at least 24 between any two, at least 3:1
// against every shipped lane background) and every wait its own hatch, so a
// wait is told by its pattern too, not only by hue. The Tauri profile uses the
// same table (client/src/ui/teams/profile-palette.ts).
struct KindStyle {
    QString key, kind, stage;       // stage: the stage kind of a "stage" span
    QColor dark, light;
    Qt::BrushStyle pattern;         // Qt::SolidPattern for a span, a hatch for a wait
};
// Every kind the timeline draws, in legend order.
const QList<KindStyle> &kindStyles();
// Task 93: no kind more saturated than this (CIE LCh chroma), dark and light.
constexpr double ChromaCapDark = 45, ChromaCapLight = 55;
// The palette key of a span: question, permission, stop and no_report are one
// "blocked" kind, delivery and start_delay one "delivery" kind, and a stage
// other than review or test one "stage" kind.
QString kindKey(const QString &kind, const QString &stageKind);
QColor kindColor(const QString &kind, const QString &stageKind, bool dark);
Qt::BrushStyle kindPattern(const QString &kind, const QString &stageKind);
// The brush a kind's bar and swatch fill with: Qt's own pattern, or (task 118,
// Qt::TexturePattern) the sparse dot of a wait for a person, in `tone`.
QBrush kindBrush(const QColor &tone, Qt::BrushStyle pattern);
QString kindName(const QString &kind, const QString &stageKind);
// Task 118: what a wait for a person's answer waited on ("acceptance", "question",
// ...) and the loss's reasons with their seconds: "phase acceptance 6m, a question 5m".
QString personReason(const QString &reason);
QString personReasons(const QJsonObject &reasons);
// The name the legend gives a palette key.
QString keyName(const QString &key);
// WCAG contrast ratio of two colours.
double contrast(const QColor &a, const QColor &b);
// The text colour for a label on a bar of `fill`: near black or white, whichever reads better.
QColor labelOn(const QColor &fill);
// Where a lane draws, from the lane's top: the work bar (with its title), the
// waits strip under it, and build and merge as a thin strip along the bottom,
// never over the work bar.
struct LaneBands { static constexpr int Row = 52, WorkTop = 8, WorkHeight = 22, WaitTop = 33, WaitHeight = 7, BuildTop = 43, BuildHeight = 5; };
// "5h 30m", "26m", "45s".
QString duration(qint64 seconds);
// "1.09M", "846k", "512".
QString tokens(qint64 count);

// One scope's reads, kept for a while: at most `MaxEntries` scopes, `MaxSpanPages`
// pages of spans each (TASK_PAGE tasks a page); a scope is read again when
// the team changed and the entry is older than `Stale`.
class Cache : public QObject {
    Q_OBJECT
public:
    explicit Cache(TeamController *teams, QObject *parent = nullptr);
    // U62 (#719): at most TestPage test runs of the window, one read.
    static constexpr int MaxEntries = 6, MaxSpanPages = 20, SpanPage = 50, LossPage = 20, TestPage = 100;
    static constexpr qint64 Stale = 60000;
    struct Entry {
        QJsonObject profile, compare;
        QJsonArray timelines, tests;
        QString error;
        bool loading = false, ready = false, truncated = false;
        qint64 at = 0, revision = -1;
    };
    // The scope "phase:N", "plan" or "compare:A:B"; reads it when missing or stale.
    const Entry *entry(const QString &team, const QString &scope, qint64 revision);
    void reload(const QString &team, const QString &scope);
    // The next page of task rows (tokens per task).
    void moreTasks(const QString &team, const QString &scope);
    int size() const { return int(m_entries.size()); }
    int reads() const { return m_reads; }
signals:
    void changed(const QString &team, const QString &scope);
private:
    void load(const QString &team, const QString &scope, qint64 revision);
    void spans(const QString &key, const QString &team, int phase, int after, int pages);
    void tests(const QString &key, const QString &team);
    void touch(const QString &key);
    TeamController *m_teams;
    QHash<QString, Entry> m_entries;
    QStringList m_order;                       // least recently used first
    QHash<QString, quint64> m_generation;
    int m_reads = 0;
};

}   // namespace TeamProfile

class TeamProfileTimeline;
class TeamProfileBars;

class TeamProfileView : public QWidget {
public:
    TeamProfileView(TeamController *teams, TeamProfile::Cache *cache, QWidget *parent = nullptr);
    void setDark(bool dark);
    // The team as the workspace shows it; reads in the background when needed.
    void setTeam(const Teams::Team &team);
    // The scope: a phase, or 0 for the plan.
    void setPhase(int phase);
    int phase() const { return m_phase; }
    void setTab(const QString &tab);       // time, tokens, members, compare
    QString tab() const { return m_tab; }
    void setLossOrder(const QString &order);   // time or tokens
    // Draws a read directly (tests and the large-team bound).
    void showModel(const TeamProfile::Model &model);
    const TeamProfile::Model &model() const { return m_model; }
    // Opens a task's card (the workspace's drawer).
    std::function<void(int task)> openTask;
    // Where Export JSON writes (tests); empty: a save dialog.
    std::function<QString()> exportPath;
    // The task the view marks in the timeline after a click on a loss.
    int selectedTask() const;
private:
    void read();
    void render();
    void renderLosses();
    const Teams::Phase *archivedScope() const;
    QWidget *archivedPage();
    QWidget *summaryTiles();
    QWidget *timePage();
    QWidget *estimatesCard();
public:
    static constexpr int EstimateRows = 30;
private:
    QWidget *tokensPage();
    QWidget *membersPage();
    QWidget *comparePage();
    void pick(int task);
    void exportJson();
    QString scope() const;
    TeamController *m_teams;
    TeamProfile::Cache *m_cache;
    Teams::Team m_team;
    TeamProfile::Model m_model;
    int m_phase = 0, m_compareA = 0, m_compareB = 0;
    QString m_tab = QStringLiteral("time"), m_order = QStringLiteral("time"), m_shownKey;
    bool m_dark = true;
    QComboBox *m_scope;
    QWidget *m_tabs, *m_body = nullptr, *m_side;
    QVBoxLayout *m_bodyHost, *m_lossList;
    QLabel *m_state;
    QPointer<TeamProfileTimeline> m_timeline;
    QPushButton *m_lossesToggle = nullptr;
    bool m_lossesOpen = true;
};
