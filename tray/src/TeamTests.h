#pragma once
#include "TeamModel.h"
#include <QHash>
#include <QJsonArray>
#include <QJsonObject>
#include <QSet>
#include <QTimer>
#include <QWidget>
#include <functional>

class TeamController;
class QPushButton;
class QVBoxLayout;

// WhiteWh fork, U62 (#719): the team's Tests view, beside Graph, Board, Layers
// and Profile. It reads `hgs team test` (action queue, then results in pages)
// in the background while it is shown: what runs now, what waits, each suite
// with its latest runs and Force run, and the history with its status,
// duration, who asked and the reused, flaky and over budget marks a record
// carries (#718). A record's excerpt shows only when its row is opened.
namespace TeamTests {

// One result of the database (test_queue.rs Record) as the view shows it.
struct Run {
    QString id, suite, status, askedBy, commit, excerpt, reusedFrom;
    // #732: the test runner (a Machine's alias) it ran on; empty is here.
    QString runner;
    int task = 0;
    double at = 0;
    qint64 durationMs = 0, recordedMs = 0;
    bool reused = false, overBudget = false, due = false;
    // #718's flags: passed only on its retry, slower than recorded, crashed.
    bool retried = false, slower = false, crash = false;
    int flakyRuns = 0, flakyFailed = 0;     // a recent history when sent; 0 runs: none
    double budgetMinutes = 0;
    QStringList affected;
    static Run parse(const QJsonObject &record);
    bool flaky() const { return retried || status == QLatin1String("flaky") || (flakyRuns > 0 && flakyFailed > 0 && flakyFailed < flakyRuns); }
    QString flakyTip() const;
};

// A request still in the queue: waiting, or running since `started`.
struct Pending {
    QString id, suite, askedBy, state, resources;
    // #732: the platform classes it needs and why it does not run yet (no
    // runner of its class, or that runner unreachable); empty: it just waits.
    QStringList platforms;
    QString reason;
    int task = 0;
    bool forced = false;
    double at = 0, started = 0, seconds = 0, budgetMinutes = 0;
    static Pending parse(const QJsonObject &item);
    bool running() const { return state == QLatin1String("running"); }
    bool waitsForRunner() const { return !running() && !reason.isEmpty(); }
};

// #732: "box", or "this computer" for a run here.
QString runnerName(const QString &runner);

// "45s", "3m 05s", "1h 20m".
QString runTime(qint64 ms);
// The status as the view names it: Passed, Failed, Timed out.
QString statusName(const QString &status);

}   // namespace TeamTests

// #756: the Builds tab, `hgs team builds`: for each folder two or more
// members share, per build folder the running build against its usual time,
// the queue (with requests that wait for the same tree), the cache's hit rate
// and the recent builds, a failure naming the member whose file broke it; and
// who changes what in that folder.
namespace TeamBuilds {

struct Running {
    QString member, templateName;
    double started = 0, seconds = 0, usualMs = 0;
    bool snapshot = true;
};
struct Waiting {
    QString member, templateName, coalescedWith;
    double since = 0, seconds = 0;
    // Shared mode: the members whose edits are not ready yet.
    QStringList waitingFor;
};
// A member and the files: a blame, a change list or an overlap.
struct Files {
    QString who;
    QStringList files;
};
struct Done {
    QString member, templateName, by, error;
    double at = 0;
    qint64 ms = 0;
    int exit = -1;
    bool hit = false, timedOut = false;
    QList<Files> blamed;
    bool failed() const { return !hit && exit != 0; }
};
struct BuildFolder {
    QString name;
    QList<Running> running;
    QList<Waiting> queue;
    double usualMs = 0, hitRate = -1;    // -1: nothing finished yet
    QList<Done> timeline;
};
struct Folder {
    QString folder;
    QStringList members;
    QList<BuildFolder> builds;
    QList<Files> changes;               // member -> its changed files
    QList<Files> overlaps;              // file -> the members changing it
};
struct State {
    bool enabled = false;
    QList<Folder> folders;
    static State parse(const QJsonObject &result);
};

}   // namespace TeamBuilds

class TeamTestsView : public QWidget {
public:
    TeamTestsView(TeamController *teams, QWidget *parent = nullptr);
    static constexpr int Page = 20, MaxPages = 10, PollMs = 5000;
    void setDark(bool dark);
    // The team as the workspace shows it; reads when shown and every PollMs.
    void setTeam(const Teams::Team &team);
    void reload();
    // Opens a record's row (a task drawer's link).
    void showRecord(const QString &id);
    QString openRecord() const { return m_open; }
    const QList<TeamTests::Run> &runs() const { return m_runs; }
    const QList<TeamTests::Pending> &pending() const { return m_pending; }
    std::function<void(int task)> openTask;
    // #756: "tests" or "builds".
    QString tab() const { return m_tab; }
    void setTab(const QString &tab);
    const TeamBuilds::State &builds() const { return m_builds; }
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
private:
    void readQueue();
    void readResults(bool more);
    void force(const QString &suite);
    void render();
    QWidget *runningRow(const TeamTests::Pending &item);
    QWidget *queueRow(const TeamTests::Pending &item);
    QWidget *suiteRow(const QString &suite);
    QWidget *historyRow(const TeamTests::Run &run);
    QWidget *taskLink(int task);
    // #756: the Builds tab.
    void readBuilds();
    void renderBuilds(QVBoxLayout *v);
    QWidget *changesStrip(const TeamBuilds::Folder &folder);
    QWidget *buildRunningRow(const TeamBuilds::Running &item);
    QWidget *buildQueueRow(const TeamBuilds::Waiting &item, int ahead);
    QWidget *buildDoneRow(const TeamBuilds::Done &item);
    QString who(const QString &member) const;
    QString m_tab = QStringLiteral("tests");
    TeamBuilds::State m_builds;
    QByteArray m_buildsShape;
    bool m_buildsLoading = false, m_buildsRead = false;
    QString m_buildsError;
    TeamController *m_teams;
    Teams::Team m_team;
    QString m_teamId, m_open, m_error;
    bool m_dark = true, m_queueLoading = false, m_resultsLoading = false, m_hasMore = false, m_read = false;
    quint64 m_generation = 0;
    int m_pages = 0;
    QList<TeamTests::Pending> m_pending;
    QList<TeamTests::Run> m_runs;
    QSet<QString> m_forcing;               // suites whose Force run is on its way
    QHash<QString, QString> m_forceError;  // suite -> what hgs refused
    QTimer m_poll;
    QVBoxLayout *m_body;
    QWidget *m_content = nullptr;
};
