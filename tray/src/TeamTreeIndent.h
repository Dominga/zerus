#pragma once
#include <QPainter>
#include <QPainterPath>
#include <QVBoxLayout>
#include <QWidget>

// CSS color-mix(in srgb, a share, b): `share` of `a`, the rest of `b`.
inline QColor colorMix(const QColor &a, const QColor &b, qreal share)
{
    return QColor::fromRgbF(a.redF() * share + b.redF() * (1 - share), a.greenF() * share + b.greenF() * (1 - share),
                            a.blueF() * share + b.blueF() * (1 - share));
}

// WhiteWh fork, U25 part 10 (spec 5.6, 5.7): a member card inside the team's
// tree. Level 0 (the lead and its direct reports) is not indented; each level
// below a sub-lead moves the card right by `step` and draws its elbow: a line
// from above the card down to `elbowY` below the card's top, turning right
// with a rounded corner into the card. A line continues to the next sibling
// only while one follows (`last` false); `through[k]` keeps the line of level
// k+1 running past this row for a later sibling of an ancestor, so a line ends
// at the elbow of the last child of its level - no tails. The metrics are the
// prototype's `.mcard.d2::before` (margin 20, left -12, top -7, height 27,
// width 10, radius 6).
class TeamTreeIndent : public QWidget {
public:
    struct Metrics {
        int step = 20;        // indent per level
        int lineOffset = 12;  // the line runs this far left of the card
        int elbowY = 20;      // the elbow, below the card's top
        int elbowWidth = 10;  // from the line to the card
        int radius = 6;       // the elbow's corner
        int gap = 6;          // space above every card but the first
    };

    TeamTreeIndent(QWidget *card, int level, bool last, const QList<bool> &through, const QColor &line, bool first,
                   const Metrics &metrics = {}, QWidget *parent = nullptr)
        : QWidget(parent), m_level(level), m_last(last), m_through(through), m_line(line), m_metrics(metrics)
    {
        setObjectName("teamTreeIndent");
        setProperty("level", level);
        m_top = first ? 0 : metrics.gap;
        auto *v = new QVBoxLayout(this);
        v->setContentsMargins(level * metrics.step, m_top, 0, 0);
        v->setSpacing(0);
        v->addWidget(card);
    }

    int level() const { return m_level; }

protected:
    void paintEvent(QPaintEvent *) override
    {
        if (m_level <= 0) return;
        QPainter p(this);
        p.setRenderHint(QPainter::Antialiasing);
        QPen pen(m_line, 1);
        p.setPen(pen);
        p.setBrush(Qt::NoBrush);
        const qreal half = 0.5;
        // The lines of the levels above, where a later sibling still follows.
        for (int k = 1; k < m_level; ++k) {
            if (k - 1 < m_through.size() && m_through.at(k - 1)) {
                const qreal x = k * m_metrics.step - m_metrics.lineOffset + half;
                p.drawLine(QPointF(x, 0), QPointF(x, height()));
            }
        }
        // Its own elbow, and the line on to the next sibling when one follows.
        const qreal x = m_level * m_metrics.step - m_metrics.lineOffset + half;
        const qreal y = m_top + m_metrics.elbowY + half;
        const qreal r = m_metrics.radius;
        QPainterPath elbow(QPointF(x, 0));
        elbow.lineTo(x, y - r);
        elbow.quadTo(QPointF(x, y), QPointF(x + r, y));
        elbow.lineTo(x + m_metrics.elbowWidth, y);
        p.drawPath(elbow);
        if (!m_last) p.drawLine(QPointF(x, y - r), QPointF(x, height()));
    }

private:
    int m_level = 0;
    bool m_last = true;
    QList<bool> m_through;
    QColor m_line;
    Metrics m_metrics;
    int m_top = 0;
};
