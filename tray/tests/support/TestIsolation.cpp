// Compiled into every Windows test program (tests/CMakeLists.txt). The native
// QSettings format there is the registry, so tests that use default settings
// would read and write the developer's HKEY_CURRENT_USER. Like hgs-tray on
// Windows they use INI files, here in a directory of their own. Tests that set a
// path themselves still do so later, in initTestCase(). The program also names
// the fixture worker pool its fake executables share (support/fixture-worker.py).
// Its temporary files live beside the test programs, not in %TEMP%: every fake
// hgs is a copy of the small unsigned test helper, and Windows Defender's
// machine-learning check flagged hundreds of such copies in %TEMP% as
// Trojan:Win32/Bearfoos.A!ml (owner, 2026-10-29). ZERUS_TEST_SYSTEM_TEMP=1 keeps
// %TEMP%.
#include <QCoreApplication>
#include <QFileInfo>
#include <QDir>
#include <QSettings>
#include <QStandardPaths>
#include <QUuid>

#include <windows.h>

namespace {
// QSettings creates the directory on its first write, and it is removed when the
// program ends normally. A test child that is killed on purpose before it writes
// (test_messagecomposer's crash child) leaves nothing behind.
struct SettingsDirectory {
    QString path;
    ~SettingsDirectory() { if (!path.isEmpty()) QDir(path).removeRecursively(); }
};

void moveTemporaryFiles()
{
    if (qgetenv("ZERUS_TEST_SYSTEM_TEMP") == "1") return;
    // Only the user's own %TEMP% moves: a TEMP someone chose (a sandbox's, a
    // script's) stays, since a lower-integrity sandbox cannot write a folder a
    // normal process created in the build tree.
    const QString system = QDir::cleanPath(QString::fromLocal8Bit(qgetenv("LOCALAPPDATA")) + QStringLiteral("/Temp"));
    const QString current = QDir::cleanPath(QString::fromLocal8Bit(qgetenv("TEMP")));
    if (!current.isEmpty() && current.compare(system, Qt::CaseInsensitive) != 0) return;
    const QString dir = QFileInfo(QStringLiteral(ZERUS_TEST_HELPER)).absolutePath() + QStringLiteral("/tmp");
    if (!QDir().mkpath(dir)) return;
    const QByteArray native = QDir::toNativeSeparators(dir).toLocal8Bit();
    qputenv("TEMP", native);
    qputenv("TMP", native);
}

// U62 (#720): a crashing test program, and every helper or child tray it
// starts (they inherit the mode), shows no error box, so Windows Error
// Reporting writes no dump into the person's %LOCALAPPDATA%\CrashDumps and
// never rotates the owner's own dumps out. QtTest still prints the crash.
void quietCrashes()
{
    SetErrorMode(SetErrorMode(0) | SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX);
    qputenv("ZERUS_TEST_ERROR_MODE", QByteArray::number(uint(GetErrorMode())));
}

void isolateSettings()
{
    quietCrashes();
    moveTemporaryFiles();
    static SettingsDirectory directory;
    directory.path = QDir::tempPath() + '/' + QCoreApplication::applicationName() + '-'
        + QString::number(QCoreApplication::applicationPid()) + '-' + QUuid::createUuid().toString(QUuid::Id128).left(8);
    QSettings::setDefaultFormat(QSettings::IniFormat);
    QSettings::setPath(QSettings::IniFormat, QSettings::UserScope, directory.path);
    QSettings::setPath(QSettings::IniFormat, QSettings::SystemScope, directory.path);
    QStandardPaths::setTestModeEnabled(true);
    // Zerus's log (ForkLog.h) goes to this folder too: the fixtures' notices and
    // errors once filled the person's zerus.log while a test ran (owner,
    // 2026-10-30). A test that sets its own home still does.
    if (qEnvironmentVariableIsEmpty("HGS_TRAY_HOME"))
        qputenv("HGS_TRAY_HOME", QDir::toNativeSeparators(directory.path).toLocal8Bit());
    // ZERUS_FIXTURE_POOL=0 starts every fixture in a Python process of its own.
    if (qgetenv("ZERUS_FIXTURE_POOL") != "0")
        qputenv("ZERUS_FIXTURE_POOL", QByteArray::number(QCoreApplication::applicationPid()));
}
} // namespace

Q_COREAPP_STARTUP_FUNCTION(isolateSettings)
