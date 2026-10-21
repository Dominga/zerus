#pragma once
#include <QDateTime>
#include <QPainter>
#include <QTimer>
#include <QWidget>

// WhiteWh fork: the line above the message field while the selected session
// works, as Claude Code shows it: an animated mark, what the agent is doing now
// (its plan's current step, else its tool, else its activity) and how long the
// turn has run.
class WorkingLine : public QWidget {
public:
    explicit WorkingLine(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("workingLine");
        setFixedHeight(24);
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
        m_timer.setInterval(120);
        QObject::connect(&m_timer, &QTimer::timeout, this, [this] { ++m_frame; update(); });
        hide();
    }
    // started: the turn's start in seconds since the epoch, or 0 when unknown.
    void setWorking(bool working, const QString &text, double started)
    {
        m_text = text; m_started = started;
        setToolTip(working ? text : QString());
        setAccessibleName(working ? tr("Working: %1").arg(text) : QString());
        setVisible(working);
        if (working && !m_timer.isActive()) m_timer.start();
        if (!working) m_timer.stop();
        update();
    }
    void setColors(const QColor &accent, const QColor &text, const QColor &muted)
    {
        if (accent.isValid()) m_accent = accent;
        if (text.isValid()) m_textColor = text;
        if (muted.isValid()) m_muted = muted;
        update();
    }
    QString text() const { return m_text; }
    // "9m 24s", "1h 05m" or "12s".
    static QString elapsed(qint64 seconds)
    {
        if (seconds < 60) return QStringLiteral("%1s").arg(seconds);
        if (seconds < 3600) return QStringLiteral("%1m %2s").arg(seconds / 60).arg(seconds % 60, 2, 10, QChar('0'));
        return QStringLiteral("%1h %2m").arg(seconds / 3600).arg(seconds / 60 % 60, 2, 10, QChar('0'));
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        static const QStringList frames{QStringLiteral("·"), QStringLiteral("✢"), QStringLiteral("✳"), QStringLiteral("✶"),
                                        QStringLiteral("✻"), QStringLiteral("✽"), QStringLiteral("✻"), QStringLiteral("✶"),
                                        QStringLiteral("✳"), QStringLiteral("✢")};
        QPainter p(this);
        p.setRenderHint(QPainter::TextAntialiasing);
        auto mark = font(); mark.setPixelSize(14);
        p.setFont(mark); p.setPen(m_accent.isValid() ? m_accent : palette().color(QPalette::Highlight));
        const QRect markRect(2, 0, 18, height());
        p.drawText(markRect, Qt::AlignCenter, frames.at(m_frame % frames.size()));
        auto label = font(); label.setPixelSize(12);
        const QFontMetrics metrics(label);
        QString time;
        if (m_started > 0)
            time = elapsed(qMax<qint64>(0, QDateTime::currentSecsSinceEpoch() - qint64(m_started)));
        const int timeWidth = time.isEmpty() ? 0 : metrics.horizontalAdvance(time) + 10;
        const int x = markRect.right() + 8;
        const int room = qMax(0, width() - x - timeWidth - 4);
        const auto shown = metrics.elidedText(m_text, Qt::ElideRight, room);
        p.setFont(label);
        p.setPen(m_textColor.isValid() ? m_textColor : palette().color(QPalette::WindowText));
        p.drawText(QRect(x, 0, room, height()), Qt::AlignLeft | Qt::AlignVCenter, shown);
        if (!time.isEmpty()) {
            p.setPen(m_muted.isValid() ? m_muted : palette().color(QPalette::PlaceholderText));
            p.drawText(QRect(x + metrics.horizontalAdvance(shown) + 10, 0, timeWidth, height()), Qt::AlignLeft | Qt::AlignVCenter, time);
        }
    }

private:
    QTimer m_timer;
    int m_frame = 0;
    QString m_text;
    double m_started = 0;
    // Until the window's theme names them, the palette's.
    QColor m_accent, m_textColor, m_muted;
};
