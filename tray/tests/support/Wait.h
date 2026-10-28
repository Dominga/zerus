#pragma once
// #525: waits that follow the work, not the clock. A fixed QTRY timeout fails
// on a loaded machine while the work is still running and passes alone; these
// waits poll the condition for as long as the work it depends on is in flight,
// and fail only once everything is idle and the condition still does not hold.
//
// Work in flight: the hgs children every client starts (ProcessRunner), and the
// probes a suite registers (an Activity render that is still pending, say). A
// window polls on its own, so "nothing in flight" may never hold; a wait instead
// outlasts the work that was in flight when it began and the work that started,
// two generations (a reply that runs the next command), plus any child still
// running after `longMs`. Once that has drained the condition gets `settleMs`
// more to land from the event queue. A hard ceiling, below Qt's 300 s function
// watchdog, only guards work that never ends.
//
// A modal dialog or popup that stays open `modalMs` after the work drained is
// closed and the test fails: a test's own timer answers the dialogs it expects
// within milliseconds, so one still open then would hang the suite (ModalGuard,
// started in a suite's init(), covers a dialog opened by a click in the test).
#include <QApplication>
#include <QDialog>
#include <QElapsedTimer>
#include <QObject>
#include <QString>
#include <QTest>
#include <QTimer>
#include <QWidget>
#include <functional>
#include <vector>

#include "ProcessRunner.h"

namespace TestWait {

// A probe says whether work the suite knows about is still in flight.
inline std::vector<std::function<bool()>> &probes()
{
    static std::vector<std::function<bool()>> list;
    return list;
}
inline void addProbe(std::function<bool()> probe) { probes().push_back(std::move(probe)); }

constexpr int settleMs = 2000;    // drained time a condition still gets
constexpr int modalMs = 5000;     // drained time a dialog may stay open unanswered
constexpr int longMs = 1000;      // a child running this long is work, not a poll
constexpr int ceilingMs = 120000; // only for work that never ends

inline bool probing()
{
    for (const auto &probe : probes()) if (probe()) return true;
    return false;
}

// Work a retry loop should outlast: a pending probe or a long-running child.
inline bool busy() { return probing() || ProcessRunner::oldestInFlightMs() >= longMs; }

inline QString &lastFailure()
{
    static QString text;
    return text;
}

// Milliseconds since the work in flight at restart() and the work it started
// finished, or -1 while it is still running.
class Drain {
public:
    Drain() { restart(); }
    void restart() { m_mark.start(); m_generations = 0; }
    qint64 poll()
    {
        const qint64 oldest = ProcessRunner::oldestInFlightMs();
        const bool probe = probing();
        if (m_generations < 2) {
            // Every child still running was asked for after the mark.
            if (probe || (oldest >= 0 && oldest >= m_mark.elapsed())) return -1;
            m_mark.start();
            if (++m_generations < 2) return -1;
            m_since.start();
        }
        if (probe || oldest >= longMs) m_since.start();
        return m_since.elapsed();
    }

private:
    QElapsedTimer m_mark, m_since;
    int m_generations = 0;
};

// Closes a dialog or popup left open after the work drained, and says so.
class ModalGuard {
public:
    ModalGuard()
    {
        m_timer.setInterval(50);
        QObject::connect(&m_timer, &QTimer::timeout, [this] { check(); });
        m_timer.start();
    }
    // The dialog it closed, empty when none.
    QString closed() const { return m_closed; }

private:
    void check()
    {
        QWidget *open = QApplication::activeModalWidget();
        if (!open) open = QApplication::activePopupWidget();
        if (!open || open != m_open) { m_open = open; m_drain.restart(); return; }
        if (m_drain.poll() < modalMs) return;
        m_closed = open->objectName().isEmpty() ? QString::fromLatin1(open->metaObject()->className()) : open->objectName();
        m_open = nullptr;
        if (auto *dialog = qobject_cast<QDialog *>(open)) dialog->reject();
        else open->close();
    }

    QTimer m_timer;
    QWidget *m_open = nullptr;
    Drain m_drain;
    QString m_closed;
};

// Polls `done` until it holds. False, with lastFailure() set, when it still
// does not hold `settleMs` after the work drained, or when a dialog was left
// open and had to be closed.
inline bool until(const std::function<bool()> &done, const char *what)
{
    lastFailure().clear();
    ModalGuard guard;
    Drain drain;
    QElapsedTimer total; total.start();
    while (true) {
        if (done()) return true;
        if (!guard.closed().isEmpty()) {
            lastFailure() = QStringLiteral("%1: the dialog %2 stayed open after the work drained and was closed").arg(QString::fromLatin1(what), guard.closed());
            return false;
        }
        if (drain.poll() >= settleMs) {
            lastFailure() = QStringLiteral("%1: still false %2 ms after the work drained").arg(QString::fromLatin1(what)).arg(settleMs);
            return false;
        }
        if (total.elapsed() >= ceilingMs) {
            lastFailure() = QStringLiteral("%1: work still in flight after %2 ms").arg(QString::fromLatin1(what)).arg(ceilingMs);
            return false;
        }
        QTest::qWait(10);
    }
}

// Lets the work in flight finish, for a check that something did NOT happen, or
// that a view is left as it was: true `quietMs` after it drained.
inline bool settle(int quietMs = 500)
{
    Drain drain;
    QElapsedTimer total; total.start();
    while (total.elapsed() < ceilingMs) {
        if (drain.poll() >= quietMs) return true;
        QTest::qWait(10);
    }
    lastFailure() = QStringLiteral("work still in flight after %1 ms").arg(ceilingMs);
    return false;
}

} // namespace TestWait

// QTRY_VERIFY / QTRY_COMPARE without a clock: the bound is the work.
#define WAIT_VERIFY(expr) \
    do { if (!TestWait::until([&]() -> bool { return bool(expr); }, #expr)) QFAIL(qPrintable(TestWait::lastFailure())); } while (0)
#define WAIT_VERIFY2(expr, message) \
    do { if (!TestWait::until([&]() -> bool { return bool(expr); }, #expr)) QFAIL(qPrintable(TestWait::lastFailure() + QStringLiteral(" - ") + QString(message))); } while (0)
#define WAIT_COMPARE(actual, expected) \
    do { TestWait::until([&]() -> bool { return (actual) == (expected); }, #actual); QCOMPARE(actual, expected); } while (0)
#define WAIT_SETTLED() \
    do { if (!TestWait::settle()) QFAIL(qPrintable(TestWait::lastFailure())); } while (0)
