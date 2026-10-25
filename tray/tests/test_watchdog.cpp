#include <QtTest>

#include <QThread>

#include <atomic>

#include "MainThreadWatchdog.h"

// Сторож главного потока (см. MainThreadWatchdog.h). Тест сам и есть главный поток:
// «зависание» -- это QThread::msleep без цикла событий, «здоровье» -- QTest::qWait,
// который цикл крутит. Колбэк приходит ИЗ СТОРОЖЕВОГО потока, отсюда atomic.
// Порог 300 мс против секунды простоя -- запас на загруженный бокс, а не точность.

class TestMainThreadWatchdog : public QObject {
    Q_OBJECT
private slots:
    void firesOnceWhenTheMainThreadStopsPumpingEvents();
    void staysQuietWhileEventsFlow();
    void stopsWatchingWhenDestroyed();
    void holdsWhileTheSystemGoesToSleep();
    void holdsAfterAWakeUntilTheMainThreadRuns();
    void firesWhenTheMainThreadStaysSilentPastTheWakeGrace();
};

void TestMainThreadWatchdog::firesOnceWhenTheMainThreadStopsPumpingEvents()
{
    std::atomic<int> fired{0};
    MainThreadWatchdog dog(300, [&fired]() { ++fired; });
    QThread::msleep(1000);
    // Ровно один раз, а не на каждой проверке: в бою колбэк -- abort(), второго шанса
    // ему не нужно, а в тесте повторные срабатывания маскировали бы гонки.
    QCOMPARE(fired.load(), 1);
}

void TestMainThreadWatchdog::staysQuietWhileEventsFlow()
{
    std::atomic<int> fired{0};
    MainThreadWatchdog dog(300, [&fired]() { ++fired; });
    QTest::qWait(1000);
    QCOMPARE(fired.load(), 0);
}

void TestMainThreadWatchdog::stopsWatchingWhenDestroyed()
{
    std::atomic<int> fired{0};
    {
        MainThreadWatchdog dog(300, [&fired]() { ++fired; });
    }
    QThread::msleep(800);
    QCOMPARE(fired.load(), 0);
}

// The transition into sleep: the main thread stops while the process still runs.
// Beats in between do not end the hold; only a power-on does.
void TestMainThreadWatchdog::holdsWhileTheSystemGoesToSleep()
{
    std::atomic<int> fired{0};
    MainThreadWatchdog dog(300, [&fired]() { ++fired; });
    dog.systemWillSleep();
    QTest::qWait(200);
    QThread::msleep(1000);
    QCOMPARE(fired.load(), 0);
}

// A dark wake: silent until the main thread first runs, then watched as usual.
void TestMainThreadWatchdog::holdsAfterAWakeUntilTheMainThreadRuns()
{
    std::atomic<int> fired{0};
    MainThreadWatchdog dog(300, [&fired]() { ++fired; });
    dog.systemWillSleep();
    dog.systemPoweredOn();
    QThread::msleep(1000);
    QCOMPARE(fired.load(), 0);
    QTest::qWait(200);
    QThread::msleep(1000);
    QCOMPARE(fired.load(), 1);
}

// A main thread that never runs again after a wake is still a hang.
void TestMainThreadWatchdog::firesWhenTheMainThreadStaysSilentPastTheWakeGrace()
{
    std::atomic<int> fired{0};
    MainThreadWatchdog dog(300, [&fired]() { ++fired; }, 600);
    dog.systemPoweredOn();
    QThread::msleep(1500);
    QCOMPARE(fired.load(), 1);
}

QTEST_GUILESS_MAIN(TestMainThreadWatchdog)
#include "test_watchdog.moc"
