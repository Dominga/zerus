#include <QtTest>

#include <QTimer>

#include "SlowEventLog.h"

// WhiteWh fork (owner, 2026-10-30): the opt-in slow-event log names what kept
// the GUI thread busy, by its own time, in a bounded number of lines.
class TestSlowEventLog : public QObject {
    Q_OBJECT
private slots:
    void init() { m_lines.clear(); }
    void aSlowEventIsLoggedWithItsReceiverAndAFastOneIsNot();
    void aHandlerIsTimedWithoutTheEventsDeliveredInsideIt();
    void theLinesAreBoundedInRateAndTheRestCounted();
    void aTimerAndAQueuedCallAreNamedByTheirOwner();
    void statisticsRankHandlersByTheirTotalTime();
    void aReceiverDeletedByItsOwnEventIsSafe();
    void aScopeNamesThePartOfAnEventItCovers();
    void theEnvironmentTurnsItOn();

private:
    SlowEventLog::Meter meter(SlowEventLog::Config config = {})
    {
        return SlowEventLog::Meter(config, [this](const QString &line) { m_lines << line; });
    }
    static bool busy(int ms)
    {
        QThread::msleep(ms);
        return true;
    }
    QStringList m_lines;
};

void TestSlowEventLog::aSlowEventIsLoggedWithItsReceiverAndAFastOneIsNot()
{
    auto log = meter();
    QObject window;
    window.setObjectName(QStringLiteral("sessionsWindow"));
    QEvent paint(QEvent::Paint), resize(QEvent::Resize);
    QVERIFY(log.measure(&window, &resize, [] { return busy(1); }));
    QVERIFY(m_lines.isEmpty());
    QVERIFY(log.measure(&window, &paint, [] { return busy(70); }));
    QCOMPARE(m_lines.size(), 1);
    QVERIFY2(m_lines[0].startsWith(QStringLiteral("Slow GUI event: ")), qPrintable(m_lines[0]));
    QVERIFY2(m_lines[0].contains(QStringLiteral("ms in QObject \"sessionsWindow\" Paint")), qPrintable(m_lines[0]));
    QVERIFY(log.totalMs() >= 70);
}

void TestSlowEventLog::aHandlerIsTimedWithoutTheEventsDeliveredInsideIt()
{
    // A click that opens a modal dialog: the dialog's own events are theirs.
    auto log = meter();
    QObject outer, inner;
    outer.setObjectName(QStringLiteral("button"));
    inner.setObjectName(QStringLiteral("dialog"));
    QEvent click(QEvent::MouseButtonRelease), paint(QEvent::Paint);
    log.measure(&outer, &click, [&] {
        busy(5);
        log.measure(&inner, &paint, [] { return busy(80); });
        return busy(5);
    });
    QCOMPARE(m_lines.size(), 1);
    QVERIFY2(m_lines[0].contains(QStringLiteral("\"dialog\" Paint")), qPrintable(m_lines[0]));
    // Both are counted once in the GUI thread's time.
    QVERIFY2(log.totalMs() >= 90 && log.totalMs() < 400, qPrintable(QString::number(log.totalMs())));
}

void TestSlowEventLog::theLinesAreBoundedInRateAndTheRestCounted()
{
    // The window outlasts eight sleeps even where a 12 ms sleep takes several
    // times that (macOS runners).
    auto log = meter({10, 3, 1500});
    QObject receiver;
    QEvent event(QEvent::UpdateRequest);
    for (int i = 0; i < 8; ++i) log.measure(&receiver, &event, [] { return busy(12); });
    QCOMPARE(m_lines.size(), 3);
    QCOMPARE(log.suppressed(), 5);
    QTest::qWait(1600);
    log.measure(&receiver, &event, [] { return busy(12); });
    QCOMPARE(m_lines.size(), 4);
    QVERIFY2(m_lines.last().contains(QStringLiteral("(5 more slow event(s) were not logged)")), qPrintable(m_lines.last()));
    QCOMPARE(log.suppressed(), 0);
}

void TestSlowEventLog::aTimerAndAQueuedCallAreNamedByTheirOwner()
{
    QObject workspace;
    workspace.setObjectName(QStringLiteral("teamWorkspace"));
    QTimer timer(&workspace);
    QTimerEvent tick(1);
    QCOMPARE(SlowEventLog::handlerName(&timer, &tick), QStringLiteral("QTimer Timer [in QObject \"teamWorkspace\"]"));
    QObject child(&workspace);
    child.setObjectName(QStringLiteral("render"));
    QEvent call(QEvent::MetaCall);
    QCOMPARE(SlowEventLog::handlerName(&child, &call), QStringLiteral("QObject \"render\" MetaCall [in QObject \"teamWorkspace\"]"));
    // A named receiver of an ordinary event stands alone.
    QEvent paint(QEvent::Paint);
    QCOMPARE(SlowEventLog::handlerName(&child, &paint), QStringLiteral("QObject \"render\" Paint"));
}

void TestSlowEventLog::statisticsRankHandlersByTheirTotalTime()
{
    auto log = meter({1000});
    log.setStatistics(true);
    QObject list, graph;
    list.setObjectName(QStringLiteral("sessionList"));
    graph.setObjectName(QStringLiteral("teamPhaseGraph"));
    QEvent paint(QEvent::Paint);
    // Far apart, so a sleep that overshoots (macOS runners) keeps the order.
    for (int i = 0; i < 3; ++i) log.measure(&list, &paint, [] { return busy(2); });
    log.measure(&graph, &paint, [] { return busy(150); });
    const auto top = log.top(10);
    QCOMPARE(top.size(), 2);
    QCOMPARE(top[0].name, QStringLiteral("QObject \"teamPhaseGraph\" Paint"));
    QCOMPARE((top[1].name), QStringLiteral("QObject \"sessionList\" Paint"));
    QCOMPARE(top[1].count, 3);
    QVERIFY(top[0].maxMs >= 40 && top[1].totalMs >= 6);
    // Statistics do not log what is under the threshold.
    QVERIFY(m_lines.isEmpty());
    log.reset();
    QVERIFY(log.top(10).isEmpty());
    QCOMPARE(log.totalMs(), 0.0);
}

void TestSlowEventLog::aReceiverDeletedByItsOwnEventIsSafe()
{
    auto log = meter({10});
    auto *doomed = new QObject;
    QEvent event(QEvent::DeferredDelete);
    log.measure(doomed, &event, [doomed] { delete doomed; return busy(15); });
    QCOMPARE(m_lines.size(), 1);
    // It was named before its event deleted it.
    QVERIFY2(m_lines[0].contains(QStringLiteral("QObject DeferredDelete")), qPrintable(m_lines[0]));
}

void TestSlowEventLog::aScopeNamesThePartOfAnEventItCovers()
{
    // A process reply delivered through an anonymous object ranks by its owner.
    auto log = meter();
    SlowEventLog::Meter::setActive(&log);
    QObject front, window;
    window.setObjectName(QStringLiteral("sessionsWindow"));
    QEvent call(QEvent::MetaCall);
    log.measure(&front, &call, [&] {
        const SlowEventLog::Scope scope(&window, "result");
        return busy(70);
    });
    SlowEventLog::Meter::setActive(nullptr);
    QCOMPARE(m_lines.size(), 1);
    QVERIFY2(m_lines[0].contains(QStringLiteral("ms in result for QObject \"sessionsWindow\"")), qPrintable(m_lines[0]));
    // Without an active meter a scope costs nothing and names nothing.
    { const SlowEventLog::Scope idle(&window, "result"); }
    QCOMPARE(m_lines.size(), 1);
}

void TestSlowEventLog::theEnvironmentTurnsItOn()
{
    qunsetenv("ZERUS_SLOW_EVENTS");
    QCOMPARE(SlowEventLog::thresholdFromEnvironment(), 0);
    qputenv("ZERUS_SLOW_EVENTS", "0");
    QCOMPARE(SlowEventLog::thresholdFromEnvironment(), 0);
    qputenv("ZERUS_SLOW_EVENTS", "1");
    QCOMPARE(SlowEventLog::thresholdFromEnvironment(), 50);
    qputenv("ZERUS_SLOW_EVENTS", "120");
    QCOMPARE(SlowEventLog::thresholdFromEnvironment(), 120);
    qputenv("ZERUS_SLOW_EVENTS", "yes");
    QCOMPARE(SlowEventLog::thresholdFromEnvironment(), 0);
    qunsetenv("ZERUS_SLOW_EVENTS");
}

QTEST_GUILESS_MAIN(TestSlowEventLog)
#include "test_sloweventlog.moc"
