#include <QtTest>

#include <QElapsedTimer>
#include <QFile>
#include <QLockFile>
#include <QRegularExpression>
#include <QSet>
#include <QSettings>
#include <QTemporaryDir>

#include <thread>

#include "ForkLog.h"
#include "HangTrace.h"
#include "JsonParse.h"
#include "Settings.h"
#include "SettingsJson.h"

#ifdef Q_OS_WIN
#include <windows.h>
#endif

// WhiteWh fork (owner, 2026-10-30): the tray never waits on its settings file.
// The big JSON values live in files of their own, a GUI-thread write never
// waits for the INI's lock, a held lock is named in the log, and the
// watchdog's trace says what the main thread was doing.
class TestSettings : public QObject {
    Q_OBJECT
private slots:
    void initTestCase()
    {
        QVERIFY(m_settings.isValid());
        QCoreApplication::setOrganizationName(QStringLiteral("hgs-tests"));
        QCoreApplication::setApplicationName(QStringLiteral("settings"));
        QSettings::setDefaultFormat(QSettings::IniFormat);
        QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
        Settings::setLog([](const char *, const QString &text) { logged() << text; });
    }
    void init()
    {
        logged().clear();
        Settings::flush();
        SettingsJson::reset();
        QVERIFY(releaseLock());
        QSettings().clear();
        QSettings().sync();
    }
    void bigValuesLiveInFilesOfTheirOwnWrittenOffTheGuiThread();
    void anOldInIValueMovesIntoItsFileOnce();
    void aGuiThreadWriteNeverWaitsForAHeldLock();
    void aLockLeftByThisProcessQueuesTheWritesAndIsKept();
    void aLockOfADeadProcessIsRemovedAndLogged();
    void theWatchdogTraceNamesTheActivityAndTheStack();
    void aQueuedChangeReadsBackAtOnce();
    void theWriterHoldsTheLockForTheWholeWriteAndLeavesNone();
    void theRollbackBuildKeepsItsProjects();
    void aTestTrayUsesAnIniFileEverywhere();
    void noDirectQSettingsWriteIsLeftInTheDesktop();
    void testsKeepTheLogInTheirOwnFolder();

private:
    static QStringList &logged()
    {
        static QStringList lines;
        return lines;
    }
    // A lock another computer's program holds: the writer waits for it (a
    // lock naming this process would be a leftover it removes).
    static void holdLockElsewhere()
    {
        QDir().mkpath(QFileInfo(Settings::lockPath()).absolutePath());
        QFile lock(Settings::lockPath());
        QVERIFY(lock.open(QIODevice::WriteOnly));
        lock.write("4\nhgs-tray\nelsewhere.example\n");
    }
    // The writer may be reading the lock that instant, which fails a delete on
    // Windows: the release tries for a while.
    static bool releaseLock()
    {
        for (int attempt = 0; attempt < 100; ++attempt) {
            if (QFile::remove(Settings::lockPath()) || !QFileInfo::exists(Settings::lockPath())) return true;
            QThread::msleep(20);
        }
        return false;
    }
    static QByteArray onDisk(const QString &key)
    {
        QFile file(SettingsJson::filePath(key));
        return file.open(QIODevice::ReadOnly) ? file.readAll() : QByteArray();
    }
    QTemporaryDir m_settings;
};

void TestSettings::bigValuesLiveInFilesOfTheirOwnWrittenOffTheGuiThread()
{
    for (const auto *key : {"workspace/worktreeSnapshots", "workspace/organization", "swarm/observedOrganization", "swarm/observedVersions"})
        QVERIFY2(SettingsJson::inFile(QString::fromLatin1(key)), key);
    QVERIFY(!SettingsJson::inFile(QStringLiteral("workspace/skin")));
    const QString key = QStringLiteral("workspace/organization");
    QCOMPARE(SettingsJson::filePath(key), QSettings().fileName() + QStringLiteral(".organization.json"));
    const QJsonDocument document(QJsonObject{{"projects", QJsonArray{"web"}}});
    SettingsJson::store(key, document);
    // The GUI thread reads its own copy at once, without a parse.
    QCOMPARE(SettingsJson::object(key), document.object());
    SettingsJson::flush();
    QCOMPARE(QJsonDocument::fromJson(onDisk(key)), document);
    // #729 (#709 N7): its INI copy stays for the rollback build, stamped; and
    // no temporary file is left by QSaveFile.
    Settings::flush();
    QCOMPARE(QJsonDocument::fromJson(QSettings().value(key).toByteArray()), document);
    QVERIFY(QSettings().value(QStringLiteral("settingsJson/organization")).toString().contains(QLatin1Char(':')));
    const QDir folder = QFileInfo(SettingsJson::filePath(key)).dir();
    QVERIFY2(folder.entryList({QStringLiteral("*.organization.json.*")}, QDir::Files | QDir::Hidden).isEmpty(), qPrintable(folder.path()));
    // It is read once: a change on disk by someone else does not replace it.
    QFile other(SettingsJson::filePath(key));
    QVERIFY(other.open(QIODevice::WriteOnly)); other.write("{\"projects\":[\"other\"]}"); other.close();
    QCOMPARE(SettingsJson::object(key), document.object());
    SettingsJson::remove(key);
    SettingsJson::flush();
    Settings::flush();
    QVERIFY(!QFileInfo::exists(SettingsJson::filePath(key)));
    QVERIFY(!QSettings().contains(key) && !QSettings().contains(QStringLiteral("settingsJson/organization")));
    QVERIFY(SettingsJson::object(key).isEmpty());
}

void TestSettings::anOldInIValueMovesIntoItsFileOnce()
{
    const QString key = QStringLiteral("workspace/worktreeSnapshots");
    const QByteArray legacy = "{\"arch\\n/w\":{\"sampled_at\":1}}";
    QSettings().setValue(key, legacy);
    QSettings().sync();
    // The first read (the tray's preload, off the GUI thread) moves it.
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    QCOMPARE(onDisk(key), legacy);
    QCOMPARE(SettingsJson::object(key).keys(), QStringList{"arch\n/w"});
    // #729: the INI keeps its copy (the rollback build reads it), now stamped.
    Settings::flush();
    QCOMPARE(QSettings().value(key).toByteArray(), legacy);
    QVERIFY(QSettings().contains(QStringLiteral("settingsJson/worktree-snapshots")));
    // A later read takes the file; both copies agree.
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    QCOMPARE(SettingsJson::bytes(key), legacy);
    QCOMPARE(QSettings().value(key).toByteArray(), legacy);
}

void TestSettings::aGuiThreadWriteNeverWaitsForAHeldLock()
{
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("zerus"));
    Settings::flush();
    QCOMPARE(QSettings().value("workspace/skin").toString(), QStringLiteral("zerus"));
    holdLockElsewhere();
    QElapsedTimer timer; timer.start();
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("whitewh"));
    Settings::set(QStringLiteral("workspace/theme"), QStringLiteral("dark"));
    Settings::remove(QStringLiteral("workspace/skin"));
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("ocean"));
    QVERIFY2(timer.elapsed() < 200, qPrintable(QString::number(timer.elapsed())));
    // #729 (#709 N5, L8): the queue reads back at once; the file waits.
    QCOMPARE(Settings::value("workspace/skin").toString(), QStringLiteral("ocean"));
    QCOMPARE(Settings::value("workspace/theme").toString(), QStringLiteral("dark"));
    QVERIFY(Settings::queued());
    QCOMPARE(QSettings().value("workspace/skin").toString(), QStringLiteral("zerus"));
    // A reservation waits a bounded second on the GUI thread, then is withdrawn.
    timer.restart();
    QVERIFY(!Settings::setNow(QStringLiteral("tasks/messageRequest/p/t"), QStringLiteral("id")));
    QVERIFY2(timer.elapsed() < 1800, qPrintable(QString::number(timer.elapsed())));
    QVERIFY(!Settings::contains(QStringLiteral("tasks/messageRequest/p/t")));
    QVERIFY(!SettingsJson::storeNow(QStringLiteral("tasks/newCard/p/request"), QJsonDocument(QJsonObject{{"title", "x"}})));
    // Freed, the queue is written in its order, and nothing withdrawn lands.
    QVERIFY(releaseLock());
    Settings::flush();
    QVERIFY(!Settings::queued());
    QSettings fresh;
    QCOMPARE(fresh.value("workspace/skin").toString(), QStringLiteral("ocean"));
    QCOMPARE(fresh.value("workspace/theme").toString(), QStringLiteral("dark"));
    QVERIFY(!fresh.contains("tasks/messageRequest/p/t"));
    QVERIFY(!fresh.contains("tasks/newCard/p/request"));
    QVERIFY(Settings::setNow(QStringLiteral("tasks/messageRequest/p/t"), QStringLiteral("id")));
    QCOMPARE(QSettings().value("tasks/messageRequest/p/t").toString(), QStringLiteral("id"));
}

// The owner's hang (2026-10-29/30): a scanner held hgs-tray.ini.lock open, so
// the delete at the end of a sync failed and the lock still named this live
// process; the next sync on the GUI thread waited for it until it was 30 s old.
void TestSettings::aLockLeftByThisProcessQueuesTheWritesAndIsKept()
{
#ifdef Q_OS_WIN
    const QString path = Settings::lockPath();
    // The settings folder exists only after a first write.
    QDir().mkpath(QFileInfo(path).absolutePath());
    QLockFile lock(path);
    QVERIFY(lock.tryLock(0));
    // Another program reads the lock file and does not let it be deleted.
    HANDLE reader = CreateFileW(reinterpret_cast<const wchar_t *>(path.utf16()), GENERIC_READ,
                                FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_EXISTING, 0, nullptr);
    QVERIFY(reader != INVALID_HANDLE_VALUE);
    lock.unlock();
    QVERIFY2(QFileInfo::exists(path), "the delete must fail while the file is open");
    qint64 pid = 0; QString host, app;
    QVERIFY(QLockFile(path).getLockInfo(&pid, &host, &app));
    QCOMPARE(pid, QCoreApplication::applicationPid());
    // Qt's own sync would now wait for the lock; this write does not, and it
    // reads back at once.
    QElapsedTimer timer; timer.start();
    Settings::set(QStringLiteral("workspace/theme"), QStringLiteral("light"));
    QVERIFY2(timer.elapsed() < 1000, qPrintable(QString::number(timer.elapsed())));
    QCOMPARE(Settings::value("workspace/theme").toString(), QStringLiteral("light"));
    // The janitor names it and keeps it: this process may be inside a sync.
    const QString line = Settings::checkLock(0);
    QVERIFY2(line.contains(QStringLiteral("pid %1").arg(QCoreApplication::applicationPid())) && line.contains("this process"), qPrintable(line));
    QVERIFY(QFileInfo::exists(path));
    QCOMPARE(logged().size(), 1);
    // The same episode is logged once.
    QVERIFY(Settings::checkLock(0).isEmpty());
    // Once the reader lets go, the writer removes its own leftover (it is the
    // only one in this process that takes the lock) and the write lands.
    CloseHandle(reader);
    Settings::flush();
    QVERIFY(!QFileInfo::exists(path));
    QCOMPARE(QSettings().value("workspace/theme").toString(), QStringLiteral("light"));
#else
    QSKIP("a lock file that cannot be deleted is a Windows sharing rule");
#endif
}

void TestSettings::aLockOfADeadProcessIsRemovedAndLogged()
{
    // A lock as QLockFile writes it, naming a pid no process has (Windows
    // pids are multiples of 4).
    QDir().mkpath(QFileInfo(Settings::lockPath()).absolutePath());
    QFile lock(Settings::lockPath());
    QVERIFY(lock.open(QIODevice::WriteOnly));
    lock.write("999999\nhgs-tray\n" + QSysInfo::machineHostName().toUtf8() + "\n");
    lock.close();
    const QString line = Settings::checkLock(0);
    QVERIFY2(line.contains(QStringLiteral("the process is gone, so the lock was removed")), qPrintable(line));
    QVERIFY(!QFileInfo::exists(Settings::lockPath()));
}

void TestSettings::theWatchdogTraceNamesTheActivityAndTheStack()
{
    HangTrace::rememberMainThread();
    QString stack;
    std::thread reader([&stack] {
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        stack = HangTrace::mainThreadStack();
    });
    // The main thread is busy here while the other thread reads it.
    QElapsedTimer busy; busy.start();
    while (busy.elapsed() < 400) QThread::msleep(10);
    reader.join();
#if defined(Q_OS_WIN) && defined(_M_X64)
    QVERIFY2(stack.count(QLatin1Char('\n')) >= 2, qPrintable(stack));
    QVERIFY2(stack.contains(QStringLiteral("test_settings.exe+0x")) || stack.contains(QStringLiteral("Qt6Core")), qPrintable(stack));
#endif
    const QString report = HangTrace::report(31500, 30000, "writing the settings file");
    QVERIFY2(report.contains(QStringLiteral("did not answer for 31.5 s (the bound is 30 s)")), qPrintable(report));
    QVERIFY2(report.contains(QStringLiteral("Main thread: writing the settings file")), qPrintable(report));
    QVERIFY(report.contains(QStringLiteral("Stack:")));
    // A GUI-thread write says what it does while it does it.
    QVERIFY(!Settings::activity());
}

void TestSettings::testsKeepTheLogInTheirOwnFolder()
{
#ifdef Q_OS_WIN
    const QString home = qEnvironmentVariable("HGS_TRAY_HOME");
    QVERIFY2(!home.isEmpty(), "TestIsolation points the log at the test folder");
    QVERIFY2(QDir::cleanPath(ForkLog::path()).startsWith(QDir::cleanPath(QDir::fromNativeSeparators(home)), Qt::CaseInsensitive), qPrintable(ForkLog::path()));
    QVERIFY(!QDir::cleanPath(ForkLog::path()).startsWith(QDir::cleanPath(QDir::homePath() + QStringLiteral("/.local")), Qt::CaseInsensitive));
    ForkLog::write("info", QStringLiteral("test line"));
    QFile log(ForkLog::path());
    QVERIFY(log.open(QIODevice::ReadOnly));
    QVERIFY(log.readAll().contains("test line"));
    // U62 (#720): the settings, the temporary folder and the crash mode are the
    // test's own too: no INI in the person's %APPDATA%, no temporary file in
    // the person's %TEMP%, no error box (and so no dump in CrashDumps).
    const auto below = [](const QString &path, const QString &root) {
        return !root.isEmpty() && QDir::cleanPath(path).startsWith(QDir::cleanPath(root) + QLatin1Char('/'), Qt::CaseInsensitive);
    };
    QSettings defaults;
    QVERIFY2(!below(defaults.fileName(), QDir::homePath() + QStringLiteral("/AppData/Roaming")), qPrintable(defaults.fileName()));
    QVERIFY2(defaults.format() == QSettings::IniFormat, "never the registry");
    if (qgetenv("ZERUS_TEST_SYSTEM_TEMP") != "1") {
        const QString personTemp = QDir::homePath() + QStringLiteral("/AppData/Local/Temp");
        QVERIFY2(!below(QDir::tempPath(), personTemp) && QDir::cleanPath(QDir::tempPath()).compare(QDir::cleanPath(personTemp), Qt::CaseInsensitive) != 0,
                 qPrintable(QDir::tempPath()));
    }
    // The mode as the program started with it: Qt's own file code sets a
    // temporary mode and restores the old one, and two threads doing so at once
    // can drop the flag later (the runner sets it for the whole run too).
    QVERIFY(qEnvironmentVariableIntValue("ZERUS_TEST_ERROR_MODE") & SEM_NOGPFAULTERRORBOX);
#else
    QSKIP("TestIsolation is Windows-only");
#endif
}

// #729 (#709 L8): every read through Settings sees the queue, a removed group
// included, until the writer has written it.
void TestSettings::aQueuedChangeReadsBackAtOnce()
{
    Settings::set(QStringLiteral("machines/a/x"), 1);
    Settings::set(QStringLiteral("machines/a/y"), 2);
    Settings::flush();
    holdLockElsewhere();
    Settings::remove(QStringLiteral("machines/a"));
    QVERIFY(!Settings::contains(QStringLiteral("machines/a/x")));
    QCOMPARE(Settings::value(QStringLiteral("machines/a/y"), 7).toInt(), 7);
    Settings::set(QStringLiteral("machines/a/y"), 3);
    QCOMPARE(Settings::value(QStringLiteral("machines/a/y")).toInt(), 3);
    QVERIFY(!Settings::contains(QStringLiteral("machines/a/x")));
    QCOMPARE(QSettings().value(QStringLiteral("machines/a/x")).toInt(), 1);
    QVERIFY(releaseLock());
    Settings::flush();
    QVERIFY(!QSettings().contains(QStringLiteral("machines/a/x")));
    QCOMPARE(QSettings().value(QStringLiteral("machines/a/y")).toInt(), 3);
}

// #729 (#709 N5): the writer takes the lock once for the whole write, never
// probes it first, and leaves neither the lock nor its copy behind.
void TestSettings::theWriterHoldsTheLockForTheWholeWriteAndLeavesNone()
{
    const QString ini = QSettings().fileName();
    for (int i = 0; i < 20; ++i) Settings::set(QStringLiteral("loop/%1").arg(i), i);
    Settings::flush();
    QVERIFY(!QFileInfo::exists(Settings::lockPath()));
    QVERIFY(!QFileInfo::exists(ini + QStringLiteral(".writing")));
    QCOMPARE(QSettings().value(QStringLiteral("loop/19")).toInt(), 19);
    // While another program holds the lock nothing is written, not even a copy.
    holdLockElsewhere();
    const QDateTime before = QFileInfo(ini).lastModified();
    Settings::set(QStringLiteral("loop/0"), 100);
    QTest::qWait(400);
    QCOMPARE(QFileInfo(ini).lastModified(), before);
    QVERIFY(!QFileInfo::exists(ini + QStringLiteral(".writing")));
    QVERIFY(Settings::queued());
    QVERIFY(releaseLock());
    Settings::flush();
    QCOMPARE(QSettings().value(QStringLiteral("loop/0")).toInt(), 100);
    QVERIFY(!QFileInfo::exists(Settings::lockPath()));
}

// #729 (#709 N7): the updater's rollback build reads the moved values from the
// INI. Its copy stays there; a change the rollback build made wins over the
// file on the next read, and a file an earlier build moved out puts its copy
// back.
void TestSettings::theRollbackBuildKeepsItsProjects()
{
    const QString key = QStringLiteral("workspace/organization");
    const QJsonDocument mine(QJsonObject{{"projects", QJsonArray{"web"}}});
    SettingsJson::store(key, mine);
    SettingsJson::flush();
    Settings::flush();
    QCOMPARE(QJsonDocument::fromJson(QSettings().value(key).toByteArray()), mine);
    // The rollback build writes the INI only, and knows no stamp.
    const QByteArray rollback = "{\"projects\":[\"web\",\"api\"]}";
    QSettings().setValue(key, rollback);
    QSettings().sync();
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    QCOMPARE(SettingsJson::bytes(key), rollback);
    QCOMPARE(onDisk(key), rollback);
    // Both copies agree now: the next read keeps them.
    Settings::flush();
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    QCOMPARE(SettingsJson::bytes(key), rollback);
    // A file the earlier build (whitewh-116) moved out of the INI: the copy
    // comes back.
    QSettings().remove(key);
    QSettings().remove(QStringLiteral("settingsJson/organization"));
    QSettings().sync();
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    Settings::flush();
    QCOMPARE(QSettings().value(key).toByteArray(), rollback);
}

// #729 (#709 L5): HGS_TRAY_HOME gives a test's tray an INI file below its home
// on every platform, never the person's native preferences.
void TestSettings::aTestTrayUsesAnIniFileEverywhere()
{
    QTemporaryDir home;
    QVERIFY(home.isValid());
    Settings::useHome(home.path());
    {
        const QSettings settings;
        QCOMPARE(settings.format(), QSettings::IniFormat);
        QVERIFY2(QDir::cleanPath(settings.fileName()).startsWith(QDir::cleanPath(home.path()) + QLatin1Char('/')), qPrintable(settings.fileName()));
    }
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, m_settings.path());
}

// #729 (#709 N6): the desktop changes its settings only through Settings; a
// direct QSettings write (setValue, remove, sync, clear) anywhere else in
// tray/src fails here.
void TestSettings::noDirectQSettingsWriteIsLeftInTheDesktop()
{
    const QDir source(QStringLiteral(ZERUS_TRAY_SOURCE));
    QVERIFY2(source.exists(), ZERUS_TRAY_SOURCE);
    const QRegularExpression declared(QStringLiteral(R"(\bQSettings\s*&?\s*(\w+)\s*[;({])"));
    const QRegularExpression temporary(QStringLiteral(R"(\bQSettings\s*\(\s*\)\s*\.\s*(setValue|remove|sync|clear)\s*\()"));
    QStringList found;
    for (const auto &name : source.entryList({QStringLiteral("*.cpp"), QStringLiteral("*.h"), QStringLiteral("*.mm")}, QDir::Files)) {
        if (name == QLatin1String("Settings.cpp")) continue;
        QFile file(source.filePath(name));
        QVERIFY(file.open(QIODevice::ReadOnly));
        const QString text = QString::fromUtf8(file.readAll());
        QSet<QString> names;
        for (auto it = declared.globalMatch(text); it.hasNext();) names.insert(it.next().captured(1));
        const QStringList lines = text.split(QLatin1Char('\n'));
        for (int i = 0; i < lines.size(); ++i) {
            const QString &line = lines.at(i);
            if (line.trimmed().startsWith(QLatin1String("//"))) continue;
            bool direct = temporary.match(line).hasMatch();
            for (const auto &variable : std::as_const(names)) {
                const QRegularExpression write(QStringLiteral(R"(\b%1\s*(\.|->)\s*(setValue|remove|sync|clear)\s*\()").arg(QRegularExpression::escape(variable)));
                direct = direct || write.match(line).hasMatch();
            }
            if (direct) found << QStringLiteral("%1:%2: %3").arg(name).arg(i + 1).arg(line.trimmed());
        }
    }
    QVERIFY2(found.isEmpty(), qPrintable(found.join(QLatin1Char('\n'))));
}

QTEST_GUILESS_MAIN(TestSettings)
#include "test_settings.moc"
