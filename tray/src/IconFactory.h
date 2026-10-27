#pragma once

#include <QColor>
#include <QHash>
#include <QIcon>
#include <QString>

// Two stacked counters fit Plasma's square tray cells without shrinking a wide
// image: total above, attention below. Each row has its own contrasting surface,
// so colored icons remain readable on either macOS menu-bar appearance too.
class IconFactory {
public:
    QIcon forCounts(int total, int attention, bool stale);
    // No trustworthy snapshot: unknown counters.
    QIcon forError();

    // Applies to the neutral row and Swarm mark; amber is reserved for attention.
    void setForegroundOverride(const QString &colorName);

    static QColor foreground(const QString &override);
    // Windows: the taskbar's theme (1 light, 0 dark) instead of the registry,
    // so a test does not depend on the machine's theme; -1 reads it again.
    static void setTaskbarLightForTests(int light);

    // Badge text: "0".."99", then "99+". The tooltip retains the exact count.
    static QString countText(int count);

    // Unbadged brand mark for window decorations and the Linux launcher.
    static QPixmap brandPixmap(int size, const QColor &color = Qt::white);

private:
    static QPixmap renderTile(int size, const QString &total, const QString &attention,
                              bool highlighted, bool dot, const QColor &fg);
    QIcon cached(const QString &total, const QString &attention, bool highlighted, bool dot);

    QString m_foregroundOverride;
    // Stable cache keys prevent needless native menu-bar redraws. The dual-counter
    // state space is larger, so retain at most 128 rendered appearances.
    QHash<QString, QIcon> m_cache;
};
