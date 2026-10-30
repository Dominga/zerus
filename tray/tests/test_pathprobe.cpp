// WhiteWh fork, #744: PathProbe asks the file system about a path on a worker,
// bounded in time, and keeps the answer; the GUI thread never waits for it,
// even when a stat blocks (the owner's tray waited 39 s in isDir, 2026-10-31).
#include <QtTest>
#include <QElapsedTimer>
#include <QTemporaryDir>
#include <QThread>
#include <atomic>
#include <memory>
#include "PathProbe.h"

namespace {
// The event loop's longest pause while `wait` runs: a timer ticks every 5 ms.
template <typename Wait>
qint64 longestPause(Wait wait)
{
    QElapsedTimer beat; beat.start();
    qint64 worst = 0;
    QTimer heart; heart.setInterval(5);
    QObject::connect(&heart, &QTimer::timeout, &heart, [&] { worst = qMax(worst, beat.restart()); });
    heart.start();
    wait();
    return qMax(worst, beat.elapsed());
}
}

class TestPathProbe : public QObject {
    Q_OBJECT
private slots:
    void init() { PathProbe::instance().clear(); PathProbe::setStat({}); PathProbe::setTimeout(PathProbe::DefaultTimeoutMs); }
    void cleanup() { PathProbe::setStat({}); PathProbe::setTimeout(PathProbe::DefaultTimeoutMs); PathProbe::instance().clear(); }
    void aSlowStatIsAnsweredLaterAndKept();
    void aStatThatHangsDoesNotAnswerWithinItsBound();
    void anActionGetsAFreshAnswer();
    void aFolderAndItsGitAreReadTogether();
    void readsAndWorkAreBounded();
};

// The first ask says checking at once; the answer comes from the worker and is
// kept: asking again within its life runs no second stat.
void TestPathProbe::aSlowStatIsAnsweredLaterAndKept()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    auto stats = std::make_shared<std::atomic<int>>(0);
    PathProbe::setStat([stats](const QString &path) { ++*stats; QThread::msleep(400); return PathProbe::statOf(path); });
    QSignalSpy answered(&PathProbe::instance(), &PathProbe::answered);
    QElapsedTimer asked; asked.start();
    const auto first = PathProbe::instance().info(dir.path());
    QVERIFY2(asked.elapsed() < 50, qPrintable(QString::number(asked.elapsed())));
    QCOMPARE(first.state, PathProbe::Info::Checking);
    // Asked again while the worker reads it: still one stat.
    QCOMPARE(PathProbe::instance().info(dir.path()).state, PathProbe::Info::Checking);
    const qint64 pause = longestPause([&] { QTRY_COMPARE(answered.size(), 1); });
    QVERIFY2(pause < 100, qPrintable(QString::number(pause)));
    QCOMPARE(answered.first().first().toString(), dir.path());
    const auto known = PathProbe::instance().info(dir.path());
    QCOMPARE(known.state, PathProbe::Info::Answered);
    QVERIFY(known.exists && known.dir && !known.file);
    QCOMPARE(stats->load(), 1);
    // A missing path is an answer too.
    const QString missing = dir.filePath("gone");
    QCOMPARE(PathProbe::instance().info(missing).state, PathProbe::Info::Checking);
    QTRY_COMPARE(PathProbe::instance().info(missing).state, PathProbe::Info::Answered);
    QVERIFY(!PathProbe::instance().info(missing).exists);
}

// A stat that hangs (a disk deleting large folders) counts as not answering
// once its bound passes; the event loop never waits for it, and its late answer
// replaces the "not answering" one.
void TestPathProbe::aStatThatHangsDoesNotAnswerWithinItsBound()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    PathProbe::setTimeout(200);
    PathProbe::setStat([](const QString &path) { QThread::msleep(1200); return PathProbe::statOf(path); });
    bool called = false; PathProbe::Info got;
    QElapsedTimer asked; asked.start();
    PathProbe::instance().check(dir.path(), this, [&](const PathProbe::Info &info) { got = info; called = true; });
    QVERIFY(!called);
    const qint64 pause = longestPause([&] { QTRY_VERIFY(called); });
    QVERIFY2(pause < 100, qPrintable(QString::number(pause)));
    QCOMPARE(got.state, PathProbe::Info::Unresponsive);
    QVERIFY2(asked.elapsed() < 1000, qPrintable(QString::number(asked.elapsed())));
    QCOMPARE(PathProbe::instance().info(dir.path()).state, PathProbe::Info::Unresponsive);
    QTRY_COMPARE_WITH_TIMEOUT(PathProbe::instance().info(dir.path()).state, PathProbe::Info::Answered, 5000);
    QVERIFY(PathProbe::instance().info(dir.path()).dir);
}

// An action (open the folder) waits for a fresh answer, not the kept one.
void TestPathProbe::anActionGetsAFreshAnswer()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    auto stats = std::make_shared<std::atomic<int>>(0);
    PathProbe::setStat([stats](const QString &path) { ++*stats; return PathProbe::statOf(path); });
    PathProbe::instance().info(dir.path());
    QTRY_COMPARE(PathProbe::instance().info(dir.path()).state, PathProbe::Info::Answered);
    QCOMPARE(stats->load(), 1);
    QTest::qWait(1100);
    bool dirFound = false; bool called = false;
    PathProbe::instance().check(dir.path(), this, [&](const PathProbe::Info &info) { dirFound = info.dir; called = true; });
    QTRY_VERIFY(called);
    QVERIFY(dirFound);
    QCOMPARE(stats->load(), 2);
    // A gone context gets nothing.
    auto *context = new QObject;
    bool late = false;
    QTest::qWait(1100);
    PathProbe::instance().check(dir.path(), context, [&](const PathProbe::Info &) { late = true; });
    delete context;
    QTest::qWait(200);
    QVERIFY(!late);
}

// A team's folder and its .git, both from the worker.
void TestPathProbe::aFolderAndItsGitAreReadTogether()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    QVERIFY(QDir(dir.path()).mkdir(".git"));
    QVERIFY(PathProbe::instance().folder(dir.path()).checking);
    QTRY_VERIFY(!PathProbe::instance().folder(dir.path()).checking);
    const auto folder = PathProbe::instance().folder(dir.path());
    QVERIFY(folder.dir && folder.git && !folder.unresponsive);
    QTemporaryDir plain; QVERIFY(plain.isValid());
    PathProbe::instance().folder(plain.path());
    QTRY_VERIFY(!PathProbe::instance().folder(plain.path()).checking);
    QVERIFY(PathProbe::instance().folder(plain.path()).dir && !PathProbe::instance().folder(plain.path()).git);
    QCOMPARE(PathProbe::gitOf(dir.path()), dir.path() + "/.git");
}

// A file's bytes and other work run on the worker; too large, missing or late
// is not ok.
void TestPathProbe::readsAndWorkAreBounded()
{
    QTemporaryDir dir; QVERIFY(dir.isValid());
    QFile file(dir.filePath("evidence.html")); QVERIFY(file.open(QIODevice::WriteOnly)); file.write("<p>ok</p>"); file.close();
    bool done = false, ok = false; QByteArray bytes;
    PathProbe::instance().read(file.fileName(), 1024, this, [&](bool good, const QByteArray &read) { ok = good; bytes = read; done = true; });
    QTRY_VERIFY(done);
    QVERIFY(ok); QCOMPARE(bytes, QByteArray("<p>ok</p>"));
    done = false;
    PathProbe::instance().read(file.fileName(), 4, this, [&](bool good, const QByteArray &) { ok = good; done = true; });
    QTRY_VERIFY(done); QVERIFY(!ok);
    done = false;
    PathProbe::instance().read(dir.filePath("missing"), 1024, this, [&](bool good, const QByteArray &) { ok = good; done = true; });
    QTRY_VERIFY(done); QVERIFY(!ok);
    PathProbe::setTimeout(150);
    done = false; ok = true;
    const qint64 pause = longestPause([&] {
        PathProbe::instance().run(this, [] { QThread::msleep(800); return QVariant(1); }, [&](bool good, const QVariant &) { ok = good; done = true; });
        QTRY_VERIFY(done);
    });
    QVERIFY(!ok);
    QVERIFY2(pause < 100, qPrintable(QString::number(pause)));
}

QTEST_MAIN(TestPathProbe)
#include "test_pathprobe.moc"
