#pragma once
#include <QAbstractButton>
#include <QFrame>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPointer>
#include <functional>

#include "TeamWidgets.h"

// WhiteWh fork, U25 (spec 5.3): the pieces of a phase "scroll" in the Team tab,
// the same in Graph, Board and Layers. Colours are the skin's tokens (Colors).
namespace TeamUi {

// The owner's phase state names the scroll's look (prototype .pscroll.ph-*):
// planned and draft dashed in the folder colour with a hatched body, rejected
// warn, queued dashed, running accent at 45 %, done plain.
inline QColor phaseStateColor(const Colors &c, const QString &state)
{
    if (state == "draft" || state == "planned") return c.folder;
    if (state == "rejected") return c.warn;
    if (state == "awaiting_acceptance") return c.folder;   // U63: waits for you, like a draft
    if (state == "running") return c.accent;
    if (state == "done") return c.accentMuted;
    return c.muted;
}

inline QString phaseStateBadge(const QString &state)
{
    if (state == "planned") return QCoreApplication::translate("TeamPanel", "In the plan");
    if (state == "draft") return QCoreApplication::translate("TeamPanel", "Draft — waiting for you");
    if (state == "rejected") return QCoreApplication::translate("TeamPanel", "Sent back");
    if (state == "queued") return QCoreApplication::translate("TeamPanel", "Accepted · queued");
    if (state == "running") return QCoreApplication::translate("TeamPanel", "Running");
    if (state == "awaiting_acceptance") return QCoreApplication::translate("TeamPanel", "Awaiting your acceptance");
    if (state == "done") return QCoreApplication::translate("TeamPanel", "Done");
    if (state == "paused") return QCoreApplication::translate("TeamPanel", "Paused");
    if (state == "cancelled") return QCoreApplication::translate("TeamPanel", "Cancelled");
    return state;
}

// The scroll's frame: its border and fill by state, the line under the header
// and, for a draft or a planned phase, the hatched body.
class PhaseFrame : public QFrame {
public:
    PhaseFrame(const Colors &colors, const QString &state, QWidget *parent = nullptr) : QFrame(parent), m_c(colors), m_state(state)
    {
        setObjectName("teamPhaseScroll"); setProperty("state", state);
        setAttribute(Qt::WA_StyledBackground, false);
    }
    // The header; the body below it gets the separator and the hatch.
    QPointer<QWidget> header;
    bool expanded = true;
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QRectF r = QRectF(rect()).adjusted(.5, .5, -.5, -.5);
        QPainterPath shape; shape.addRoundedRect(r, 10, 10);
        const bool dashed = m_state == "draft" || m_state == "planned" || m_state == "queued" || m_state == "idle";
        const bool folder = m_state == "draft";
        QColor fill = m_c.cardFill;
        if (folder) fill = blend(m_c.cardFill, m_c.folder, .05);
        p.fillPath(shape, fill);
        const int split = header ? header->geometry().bottom() + 1 : 0;
        if (expanded && split > 0 && (m_state == "draft" || m_state == "planned")) {
            p.save(); p.setClipPath(shape); p.setClipRect(QRect(0, split, width(), height() - split), Qt::IntersectClip);
            p.setPen(QPen(alpha(m_c.folder, .06), 1));
            for (int x = -height(); x < width() + height(); x += 9) p.drawLine(QPointF(x, height()), QPointF(x + height(), 0));
            p.restore();
        }
        QColor edge = m_c.cardBorder;
        if (m_state == "draft") edge = m_c.folder;
        else if (m_state == "planned") edge = WorkspaceSkin::color(WorkspaceSkin::active(), m_c.dark, QStringLiteral("cardSelBorder"));
        else if (m_state == "rejected") edge = m_c.warn;
        else if (m_state == "awaiting_acceptance") edge = m_c.folder;
        else if (m_state == "running") edge = blend(m_c.cardBorder, m_c.accent, .45);
        QPen pen(edge, 1); if (dashed) { pen.setStyle(Qt::CustomDashLine); pen.setDashPattern({4, 3}); }
        p.setPen(pen); p.setBrush(Qt::NoBrush); p.drawPath(shape);
        if (expanded && split > 0 && split < height() - 2) {
            QPen line(m_state == "draft" || m_state == "planned" ? alpha(m_c.folder, .6) : m_c.cardBorder, 1);
            if (m_state == "draft" || m_state == "planned") { line.setStyle(Qt::CustomDashLine); line.setDashPattern({4, 3}); }
            p.setPen(line); p.drawLine(QPointF(1, split + .5), QPointF(width() - 1, split + .5));
        }
    }
private:
    static QColor blend(const QColor &base, const QColor &over, double amount)
    {
        return QColor::fromRgbF(base.redF() + (over.redF() - base.redF()) * amount, base.greenF() + (over.greenF() - base.greenF()) * amount,
                                base.blueF() + (over.blueF() - base.blueF()) * amount);
    }
    Colors m_c; QString m_state;
};

// The scroll's header row: a click on it outside its buttons folds or opens it.
class PhaseHeader : public QFrame {
public:
    std::function<void()> clicked;
protected:
    void mouseReleaseEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton && rect().contains(event->position().toPoint()) && clicked) clicked();
        QFrame::mouseReleaseEvent(event);
    }
};

// The number in its circle (prototype .pn).
class PhaseNumber : public QWidget {
public:
    PhaseNumber(int n, const QColor &tone, const QColor &fill, QWidget *parent = nullptr) : QWidget(parent), m_n(n), m_tone(tone), m_fill(fill)
    {
        setFixedSize(18, 18);
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        p.setPen(Qt::NoPen); p.setBrush(m_fill); p.drawEllipse(rect());
        QFont f = font(); f.setPixelSize(10); f.setWeight(QFont::Bold); p.setFont(f); p.setPen(m_tone);
        p.drawText(rect(), Qt::AlignCenter, QString::number(m_n));
    }
private:
    int m_n; QColor m_tone, m_fill;
};

// The status badge (prototype .phb): the state's colour, dashed for a draft.
class PhaseBadge : public QWidget {
public:
    PhaseBadge(const QString &text, const QColor &color, const QString &kind, bool dashed, QWidget *parent = nullptr)
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_dashed(dashed)
    {
        QFont f = font(); f.setPixelSize(11); f.setWeight(QFont::DemiBold); setFont(f);
        setFixedSize(sizeHint());
        setToolTip(text); setAccessibleName(text);
    }
    QString text() const { return m_text; }
    QSize sizeHint() const override { return {QFontMetrics(font()).horizontalAdvance(m_text) + 16 + (m_kind.isEmpty() ? 0 : 14), 19}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        QPen pen(m_color, 1); if (m_dashed) { pen.setStyle(Qt::CustomDashLine); pen.setDashPattern({3, 2}); }
        p.setPen(pen); p.setBrush(Qt::NoBrush); p.drawRoundedRect(QRectF(rect()).adjusted(.5, .5, -.5, -.5), 9.5, 9.5);
        int x = 8;
        if (!m_kind.isEmpty()) { glyph(p, QRectF(7, 4.5, 10, 10), m_color, m_kind); x = 21; }
        p.setPen(m_color); p.drawText(QRect(x, 0, width() - x - 6, height()), Qt::AlignVCenter | Qt::AlignLeft, m_text);
    }
private:
    QString m_text; QColor m_color; QString m_kind; bool m_dashed;
};

// accepted/tasks: a 110 px bar and the count in mono (prototype .pprog).
class PhaseBar : public QWidget {
public:
    PhaseBar(const Colors &colors, int done, int total, const QColor &fill, QWidget *parent = nullptr)
        : QWidget(parent), m_c(colors), m_done(done), m_total(total), m_fill(fill)
    {
        setFixedSize(110 + 6 + countWidth(), 16);
        setProperty("done", done); setProperty("total", total);
        setToolTip(QCoreApplication::translate("TeamPanel", "%1 of %2 tasks accepted").arg(done).arg(total));
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen);
        p.setBrush(m_c.border); p.drawRoundedRect(QRectF(0, 6, 110, 4), 2, 2);
        const double fraction = m_total > 0 ? qBound(0.0, m_done / double(m_total), 1.0) : 0.0;
        if (fraction > 0) { p.setBrush(m_fill); p.drawRoundedRect(QRectF(0, 6, 110 * fraction, 4), 2, 2); }
        p.setFont(mono()); p.setPen(m_c.text2);
        p.drawText(QRect(116, 0, width() - 116, height()), Qt::AlignVCenter | Qt::AlignLeft, QStringLiteral("%1/%2").arg(m_done).arg(m_total));
    }
private:
    static QFont mono() { QFont f(QStringLiteral("Cascadia Mono")); f.setStyleHint(QFont::Monospace); f.setPixelSize(11); return f; }
    int countWidth() const { return QFontMetrics(mono()).horizontalAdvance(QStringLiteral("%1/%2").arg(m_done).arg(m_total)) + 2; }
    Colors m_c; int m_done, m_total; QColor m_fill;
};

// A small on/off switch (prototype .switch): 30x17, its knob the accent when on.
class ToggleSwitch : public QAbstractButton {
public:
    ToggleSwitch(const Colors &colors, QWidget *parent = nullptr) : QAbstractButton(parent), m_c(colors)
    {
        setCheckable(true); setFixedSize(30, 17); setCursor(Qt::PointingHandCursor);
    }
    QSize sizeHint() const override { return {30, 17}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen);
        p.setOpacity(isEnabled() ? 1.0 : .45);
        p.setBrush(isChecked() ? m_c.segOn : m_c.border); p.drawRoundedRect(QRectF(rect()), 8.5, 8.5);
        p.setBrush(isChecked() ? m_c.accent : m_c.muted); p.drawEllipse(QRectF(isChecked() ? 15 : 2, 2, 13, 13));
    }
private:
    Colors m_c;
};

// A label that asks for its whole text but gives way down to `least` pixels,
// elided (a phase title beside its actions).
class ShrinkLabel : public ElidingLabel {
public:
    ShrinkLabel(const QString &text, int least, QWidget *parent = nullptr) : ElidingLabel(text, parent), m_least(least)
    {
        setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Preferred);
    }
    QSize minimumSizeHint() const override { return {qMin(m_least, QLabel::sizeHint().width()), QLabel::minimumSizeHint().height()}; }
private:
    int m_least;
};

// A row that runs `clicked` (a board card, a layers row, a side list row).
class ClickFrame : public QFrame {
public:
    std::function<void()> clicked;
protected:
    void mouseReleaseEvent(QMouseEvent *event) override
    {
        if (event->button() == Qt::LeftButton && rect().contains(event->position().toPoint()) && clicked) clicked();
        QFrame::mouseReleaseEvent(event);
    }
};

}
