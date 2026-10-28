#pragma once
// WhiteWh fork: Zerus's own log, a file the person can open from the tray
// menu or the status bar. Warnings, notices and connection changes go to
// ~/.local/state/hgs/zerus.log beside the relay's log (docs/fork-updates.md).
#include <QCoreApplication>
#include <QDateTime>
#include <QDesktopServices>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QMutex>
#include <QUrl>

namespace ForkLog {

// The usual folder, or the temporary folder when that one cannot be written
// (a profile Zerus may not change, a sandbox): the log still exists to open.
// HGS_TRAY_HOME points a launch at another home, as a test does: the log, the
// single-instance socket and the tray's own state then stay out of the real
// profile.
inline QString home()
{
    return qEnvironmentVariable("HGS_TRAY_HOME", QDir::homePath());
}

inline QString path()
{
    static const QString file = [] {
        const auto directory = home() + QStringLiteral("/.local/state/hgs");
        const auto usual = directory + QStringLiteral("/zerus.log");
        QDir().mkpath(directory);
        QFile probe(usual);
        if (probe.open(QIODevice::Append)) return usual;
        return QDir::temp().filePath(QStringLiteral("zerus.log"));
    }();
    return file;
}

inline QMutex &mutex()
{
    static QMutex lock;
    return lock;
}

// Bounded: past 4 MiB the file becomes zerus.log.1, replacing the older copy.
inline void write(const char *level, const QString &text)
{
    QMutexLocker locked(&mutex());
    const auto file = path();
    if (QFileInfo(file).size() > 4 * 1024 * 1024) {
        QFile::remove(file + QStringLiteral(".1"));
        QFile::rename(file, file + QStringLiteral(".1"));
    }
    QFile out(file);
    if (!out.open(QIODevice::Append | QIODevice::Text))
        return;
    const auto line = QDateTime::currentDateTime().toString(QStringLiteral("yyyy-MM-dd HH:mm:ss.zzz "))
        + QLatin1String(level) + QLatin1Char(' ') + QString(text).replace(QLatin1Char('\n'), QStringLiteral("\n    "));
    out.write((line + QLatin1Char('\n')).toUtf8());
}

inline QtMessageHandler &previous()
{
    static QtMessageHandler handler = nullptr;
    return handler;
}

// Qt's warnings join the log; debug output stays on the console only. A
// warning Qt repeats on every repaint is written once, then counted.
inline void handler(QtMsgType type, const QMessageLogContext &context, const QString &message)
{
    static const char *const levels[] = {"debug", "warning", "critical", "fatal", "info"};
    static QString last;
    static int repeated = 0;
    static QMutex order;
    if (type != QtDebugMsg && type >= 0 && type <= QtInfoMsg) {
        QMutexLocker locked(&order);
        if (message == last && type != QtFatalMsg) {
            ++repeated;
        } else {
            if (repeated)
                write("info", QStringLiteral("previous message repeated %1 times").arg(repeated));
            last = message;
            repeated = 0;
            write(levels[type], message);
        }
    }
    if (previous())
        previous()(type, context, message);
}

inline void install()
{
    previous() = qInstallMessageHandler(handler);
}

// U3: every launch writes its own line, but "started" belongs only to the
// process that takes the single-instance lock and becomes the tray. A second
// launch says "already running" and exits, so counting "started" lines no
// longer counts launches, and a crash timeline can name the owning process.
inline void launch(qint64 pid)
{
    write("info", QStringLiteral("Zerus launched (pid %1)").arg(pid));
}

inline void alreadyRunning(qint64 pid, bool forwarded)
{
    write("info", forwarded
        ? QStringLiteral("Already running (pid %1): forwarded the activation to the tray that owns this computer").arg(pid)
        : QStringLiteral("Already running (pid %1): no tray answered, so this launch exits").arg(pid));
}

inline void ownsInstance(qint64 pid, const QString &lockPath)
{
    write("info", QStringLiteral("Single instance: pid %1 holds %2").arg(pid).arg(QDir::toNativeSeparators(lockPath)));
}

inline void started()
{
    write("info", QStringLiteral("Zerus %1 started (pid %2)")
        .arg(QCoreApplication::applicationVersion()).arg(QCoreApplication::applicationPid()));
}

// False when no program could show it; the caller says where the file is.
inline bool open()
{
    if (!QFileInfo::exists(path()))
        write("info", QStringLiteral("Log opened"));
    return QFileInfo::exists(path()) && QDesktopServices::openUrl(QUrl::fromLocalFile(path()));
}

} // namespace ForkLog
