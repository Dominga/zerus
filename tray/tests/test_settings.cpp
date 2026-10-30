#include <QtTest>

#include <QElapsedTimer>
#include <QFile>
#include <QLockFile>
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
        SettingsJson::reset();
        QSettings().clear();
        QSettings().sync();
    }
    void bigValuesLiveInFilesOfTheirOwnWrittenOffTheGuiThread();
    void anOldInIValueMovesIntoItsFileOnce();
    void aGuiThreadWriteNeverWaitsForAHeldLock();
    void aLockLeftByThisProcessQueuesTheWritesAndIsKept();
    void aLockOfADeadProcessIsRemovedAndLogged();
    void theWatchdogTraceNamesTheActivityAndTheStack();
    void testsKeepTheLogInTheirOwnFolder();

private:
    static QStringList &logged()
    {
        static QStringList lines;
        return lines;
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
    // Nothing of it in the INI, and no temporary file left by QSaveFile.
    QVERIFY(!QSettings().contains(key));
    const QDir folder = QFileInfo(SettingsJson::filePath(key)).dir();
    QVERIFY2(folder.entryList({QStringLiteral("*.organization.json.*")}, QDir::Files | QDir::Hidden).isEmpty(), qPrintable(folder.path()));
    // It is read once: a change on disk by someone else does not replace it.
    QFile other(SettingsJson::filePath(key));
    QVERIFY(other.open(QIODevice::WriteOnly)); other.write("{\"projects\":[\"other\"]}"); other.close();
    QCOMPARE(SettingsJson::object(key), document.object());
    SettingsJson::remove(key);
    SettingsJson::flush();
    QVERIFY(!QFileInfo::exists(SettingsJson::filePath(key)));
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
    QTRY_VERIFY(!QSettings().contains(key));
    QCOMPARE(SettingsJson::object(key).keys(), QStringList{"arch\n/w"});
    // Once: a later read takes the file and the INI stays without it.
    SettingsJson::forget(key);
    std::thread([&key] { SettingsJson::preload({key}); }).join();
    QVERIFY(!QSettings().contains(key));
    QCOMPARE(SettingsJson::bytes(key), legacy);
}

void TestSettings::aGuiThreadWriteNeverWaitsForAHeldLock()
{
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("zerus"));
    QCOMPARE(QSettings().value("workspace/skin").toString(), QStringLiteral("zerus"));
    QLockFile held(Settings::lockPath());
    QVERIFY(held.tryLock(0));
    QElapsedTimer timer; timer.start();
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("whitewh"));
    Settings::set(QStringLiteral("workspace/theme"), QStringLiteral("dark"));
    Settings::remove(QStringLiteral("workspace/skin"));
    Settings::set(QStringLiteral("workspace/skin"), QStringLiteral("ocean"));
    // A reservation is refused, not waited for.
    QVERIFY(!Settings::setNow(QStringLiteral("tasks/messageRequest/p/t"), QStringLiteral("id")));
    QVERIFY(!SettingsJson::storeNow(QStringLiteral("tasks/newCard/p/request"), QJsonDocument(QJsonObject{{"title", "x"}})));
    QVERIFY2(timer.elapsed() < 1000, qPrintable(QString::number(timer.elapsed())));
    QVERIFY(Settings::queued());
    QCOMPARE(QSettings().value("workspace/skin").toString(), QStringLiteral("zerus"));
    // Freed, the queue is written in its order.
    held.unlock();
    Settings::flush();
    QVERIFY(!Settings::queued());
    QSettings fresh;
    fresh.sync();
    QCOMPARE(fresh.value("workspace/skin").toString(), QStringLiteral("ocean"));
    QCOMPARE(fresh.value("workspace/theme").toString(), QStringLiteral("dark"));
    QVERIFY(!fresh.contains("tasks/messageRequest/p/t"));
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
    // Qt's own sync would now wait for the lock; this write does not.
    QElapsedTimer timer; timer.start();
    Settings::set(QStringLiteral("workspace/theme"), QStringLiteral("light"));
    QVERIFY2(timer.elapsed() < 1000, qPrintable(QString::number(timer.elapsed())));
    QVERIFY(Settings::queued());
    // The janitor names it and keeps it: this process may be inside a sync.
    const QString line = Settings::checkLock(0);
    QVERIFY2(line.contains(QStringLiteral("pid %1").arg(QCoreApplication::applicationPid())) && line.contains("this process"), qPrintable(line));
    QVERIFY(QFileInfo::exists(path));
    QCOMPARE(logged().size(), 1);
    // The same episode is logged once.
    QVERIFY(Settings::checkLock(0).isEmpty());
    // Once the reader lets go and the leftover goes, the write lands.
    CloseHandle(reader);
    QVERIFY(QFile::remove(path));
    Settings::flush();
    QSettings fresh;
    fresh.sync();
    QCOMPARE(fresh.value("workspace/theme").toString(), QStringLiteral("light"));
#else
    QSKIP("a lock file that cannot be deleted is a Windows sharing rule");
#endif
}

void TestSettings::aLockOfADeadProcessIsRemovedAndLogged()
{
    // A lock as QLockFile writes it, naming a pid no process has (Windows
    // pids are multiples of 4).
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
    QVERIFY(GetErrorMode() & SEM_NOGPFAULTERRORBOX);
#else
    QSKIP("TestIsolation is Windows-only");
#endif
}

QTEST_GUILESS_MAIN(TestSettings)
#include "test_settings.moc"
