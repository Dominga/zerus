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
    int task = 0;
    double at = 0;
    qint64 durationMs = 0;
    bool reused = false, overBudget = false;
    int flakyRuns = 0, flakyFailed = 0;     // #718's recent history; 0 runs: no mark
    double budgetMinutes = 0;
    QStringList affected;
    static Run parse(const QJsonObject &record);
    bool flaky() const { return flakyRuns > 0 && flakyFailed > 0 && flakyFailed < flakyRuns; }
};

// A request still in the queue: waiting, or running since `started`.
struct Pending {
    QString id, suite, askedBy, state, resources;
    int task = 0;
    bool forced = false;
    double at = 0, started = 0, seconds = 0, budgetMinutes = 0;
    static Pending parse(const QJsonObject &item);
    bool running() const { return state == QLatin1String("running"); }
};

// "45s", "3m 05s", "1h 20m".
QString runTime(qint64 ms);
// The status as the view names it: Passed, Failed, Timed out.
QString statusName(const QString &status);

}   // namespace TeamTests

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
