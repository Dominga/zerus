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

inline QString path()
{
    const auto directory = QDir::homePath() + QStringLiteral("/.local/state/hgs");
    QDir().mkpath(directory);
    return directory + QStringLiteral("/zerus.log");
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
    write("info", QStringLiteral("Zerus %1 started").arg(QCoreApplication::applicationVersion()));
}

inline void open()
{
    if (!QFileInfo::exists(path()))
        write("info", QStringLiteral("Log opened"));
    QDesktopServices::openUrl(QUrl::fromLocalFile(path()));
}

} // namespace ForkLog
