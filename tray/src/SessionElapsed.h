#pragma once

#include <QDateTime>
#include <QObject>
#include <QString>
#include <cmath>
#include <limits>

namespace SessionElapsed {
inline QString text(double started, qint64 now = QDateTime::currentSecsSinceEpoch())
{
    if (!std::isfinite(started) || started <= 0) return {};
    const auto seconds = started >= now ? qint64(0) : now - qint64(started);
    if (seconds < 60) return QString("%1s").arg(seconds);
    if (seconds < 3600) return QString("%1m %2s").arg(seconds / 60).arg(seconds % 60, 2, 10, QLatin1Char('0'));
    if (seconds < 86400) return QString("%1h %2m").arg(seconds / 3600).arg(seconds / 60 % 60, 2, 10, QLatin1Char('0'));
    return QString("%1d %2h").arg(seconds / 86400).arg(seconds / 3600 % 24);
}
// When text() next reads differently, in msecs since the epoch; never: max.
inline qint64 changesAt(double started, qint64 nowMs)
{
    if (!std::isfinite(started) || started <= 0 || started > 4e12) return std::numeric_limits<qint64>::max();
    const qint64 now = nowMs / 1000, begin = qint64(started);
    if (started >= now) return (begin + 1) * 1000;
    const qint64 seconds = now - begin, step = seconds < 3600 ? 1 : seconds < 86400 ? 60 : 3600;
    return (begin + (seconds / step + 1) * step) * 1000;
}
inline QString status(const QString &label, double started, qint64 now = QDateTime::currentSecsSinceEpoch())
{
    const auto elapsed = text(started, now);
    return label + (elapsed.isEmpty() ? QString() : " (" + elapsed + ")");
}
inline QString working(double started, qint64 now = QDateTime::currentSecsSinceEpoch()) { return status(QObject::tr("Working"), started, now); }
}
