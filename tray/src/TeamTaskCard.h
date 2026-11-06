#pragma once
#include <QHash>
#include <QSet>
#include <QJsonArray>
#include <QJsonObject>
#include <QWidget>
#include <functional>
#include "TeamModel.h"

class QVBoxLayout;
class QScrollArea;
class QKeyEvent;

// WhiteWh fork: agent teams. A task's card (design/agent-teams S3): its status,
// the problem with its actions, description, criteria, what it depends on and
// blocks, the assignee, attempts newest first, the report and inline Reject /
// Mark done forms; actions by status in the footer, the rest under More with
// the reason when they do not apply. The Team tab shows it over its content;
// the team workspace as a drawer.
class TeamTaskCard : public QWidget {
public:
    explicit TeamTaskCard(QWidget *parent = nullptr);
    void setTask(const Teams::Team &team, int task, const Teams::LiveLookup &live, bool dark);
    int task() const { return m_task; }
    std::function<void()> back;
    std::function<void(const QString &operation, const QJsonObject &input)> act;
    // Task 55: an action whose answer the card shows (a member's question):
    // done(ok, message) once hgs answered. Without it, `act` is used.
    using Done = std::function<void(bool ok, const QString &message)>;
    std::function<void(const QString &operation, const QJsonObject &input, const Done &done)> actThen;
    std::function<void(int task)> openTask;
    // G1: open the sub-team a delegated task names.
    std::function<void(const QString &team)> openTeam;
    std::function<void(const QString &host, const QString &session)> openSession;
    std::function<void(int task, const QPoint &global)> reassign;
    // Task 82: the heat's lateness and overrun line under the meta row, or nullptr.
    std::function<QWidget *(const Teams::Team &team, int task)> heatLine;
    // The actions a member's problem offers (the Team tab's own buttons).
    std::function<QWidget *(const Teams::Member &member, const Teams::Status &status)> problemActions;
    std::function<bool(const QString &name, const QString &title, const QString &text, const QString &action)> confirm;
    // U62 (#719): the task's test runs, read in the background when the card
    // opens and at most every RunsStaleMs after; a run's link opens its record.
    using RunsRead = std::function<void(const QJsonArray &runs, const QString &error)>;
    std::function<void(int task, const RunsRead &done)> readTestRuns;
    std::function<void(const QString &record)> openTestRecord;
    static constexpr qint64 RunsStaleMs = 15000;
protected:
    // Esc closes the card, as its back button does.
    void keyPressEvent(QKeyEvent *event) override;
private:
    void readRuns();
    struct TestRuns { int task = 0; qint64 at = 0; bool loading = false; QJsonArray runs; QString error; quint64 generation = 0; };
    TestRuns m_runs;
    void rebuild();
    Teams::Team m_team;
    // #744: an evidence file's check and bytes, read by PathProbe's worker,
    // kept for the task shown.
    struct EvidenceRead { bool done = false; bool allowed = false; QByteArray bytes; };
    QHash<QString, EvidenceRead> m_evidence;
    EvidenceRead evidenceRead(const QString &path, const QStringList &roots, qint64 limit);
    Teams::LiveLookup m_live;
    int m_task = 0;
    bool m_dark = true;
    QString m_form;   // "reject" or "done" while an inline form is open
    // Task 55: the owner's answer to each open question survives the card's
    // rebuild on every poll (it was lost, so Send found nothing to send); an
    // answer on its way sends nothing again; a refusal stays on its question.
    QHash<QString, QString> m_answers;
    QSet<QString> m_answering;
    QHash<QString, QString> m_answerErrors;
    bool m_focusAnswer = false;
public:
    // Task 55: the next build puts the cursor in the first answer field
    // (Answer… in the team views opens the card for this).
    void requestAnswerFocus() { m_focusAnswer = true; }
    // The answer typed for a question (tests).
    QString answerDraft(const QString &question) const { return m_answers.value(question); }
private:
    QScrollArea *m_scroll;
    QWidget *m_footer;
    QVBoxLayout *m_layout;
};
