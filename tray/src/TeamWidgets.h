#pragma once
// WhiteWh fork: agent teams. Painted pieces shared by the Team tab, the task
// card and the team workspace: theme colors, role and status badges, the
// segmented progress and the task graph or layered list.
#include "NewSessionDense.h"
#include "TeamModel.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"
#include <QCoreApplication>
#include <QDateTime>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QVBoxLayout>
#include <QKeyEvent>
#include <algorithm>
#include <cmath>
#include <functional>
#include <tuple>

namespace TeamUi {

QIcon agentLogo(const QString &agent, const QColor &color);   // below, with agentIcon
using namespace Teams;

struct Colors {
    QColor fg, text2, text3, muted, dim, accent, accentMuted, warn, danger, folder, surface, border, card, tile;
    bool dark = true;
};

inline Colors colorsFor(bool dark)
{
    const auto skin = WorkspaceSkin::current();
    const auto t = NewSessionDense::tokens(skin, dark);
    const auto &p = skin.palette(dark);
    Colors c;
    c.dark = dark; c.fg = t.text; c.text2 = t.text2; c.text3 = t.text3; c.muted = t.muted; c.dim = t.dim;
    c.accent = t.accent; c.warn = t.warn; c.danger = t.danger; c.folder = t.folder;
    c.accentMuted = QColor(dark ? "#5c9e86" : "#4f8f78");
    c.surface = QColor(p.surface.isEmpty() ? (dark ? "#1b1f26" : "#ffffff") : p.surface);
    c.border = t.border; c.card = t.field; c.tile = QColor(dark ? "#0f1318" : "#eef2f5");
    return c;
}

inline QColor toneColor(const Colors &c, Tone tone)
{
    switch (tone) {
    case Tone::Active: return c.accent;
    case Tone::Assigned: return c.text3;
    case Tone::Done: return c.accentMuted;
    case Tone::Muted: return c.muted;
    case Tone::Dim: return c.dim;
    case Tone::Warn: return c.warn;
    case Tone::Danger: return c.danger;
    case Tone::Planning: return c.folder;
    case Tone::Neutral: break;
    }
    return c.text2;
}

// The handoff's role palette; other roles get a stable color from it.
inline QColor roleColor(const QString &role, bool dark)
{
    static const QList<std::tuple<const char *, const char *, const char *>> palette{
        {"lead", "#b79cff", "#6f4fd8"}, {"architect", "#8e9bff", "#3d4bc4"}, {"backend", "#6fa8ff", "#2f66c9"},
        {"frontend", "#f08bb4", "#b83a72"}, {"tester", "#56c8e0", "#137f95"}, {"reviewer", "#d9b38c", "#8a5a2b"},
        {"security", "#e48de0", "#a2349c"}, {"integrator", "#a3b1c6", "#4d5d75"}, {"researcher", "#b9d36b", "#5f7a12"}};
    const auto key = role.toLower();
    for (const auto &[name, d, l] : palette) if (key.contains(QLatin1String(name))) return QColor(dark ? d : l);
    const auto &[name, d, l] = palette[1 + qHash(key) % (palette.size() - 1)];
    Q_UNUSED(name);
    return QColor(dark ? d : l);
}

inline QColor alpha(QColor color, double a) { color.setAlphaF(a); return color; }

// A 12px status mark: icon and text always come together.
inline void glyph(QPainter &p, const QRectF &r, const QColor &color, const QString &kind)
{
    p.save();
    p.setRenderHint(QPainter::Antialiasing);
    const QRectF c = r.adjusted(1, 1, -1, -1);
    QPen pen(color, 1.3); pen.setCapStyle(Qt::RoundCap); p.setPen(pen); p.setBrush(Qt::NoBrush);
    const QPointF m = c.center(); const double s = c.width() / 2;
    if (kind == "accepted" || kind == "done") {
        p.setBrush(color); p.drawEllipse(c);
        p.setPen(QPen(QColor(Qt::white), 1.4, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin)); p.setBrush(Qt::NoBrush);
        QPainterPath tick; tick.moveTo(m + QPointF(-s * .45, 0)); tick.lineTo(m + QPointF(-s * .1, s * .35)); tick.lineTo(m + QPointF(s * .45, -s * .35)); p.drawPath(tick);
    } else if (kind == "draft" || kind == "not_created" || kind == "not_started") {
        pen.setStyle(Qt::DotLine); p.setPen(pen); p.drawEllipse(c);
    } else if (kind == "cancelled") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .6, s * .6), m + QPointF(s * .6, -s * .6));
    } else if (kind == "working" || kind == "in_progress" || kind == "running") {
        p.drawEllipse(c); p.setBrush(color); p.setPen(Qt::NoPen);
        QPolygonF play{m + QPointF(-s * .3, -s * .45), m + QPointF(s * .5, 0), m + QPointF(-s * .3, s * .45)}; p.drawPolygon(play);
    } else if (kind == "assigned") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .45, 0));
        p.drawLine(m + QPointF(s * .1, -s * .35), m + QPointF(s * .45, 0)); p.drawLine(m + QPointF(s * .1, s * .35), m + QPointF(s * .45, 0));
    } else if (kind == "submitted") {
        p.drawEllipse(c); p.drawLine(m + QPointF(0, s * .45), m + QPointF(0, -s * .45));
        p.drawLine(m + QPointF(-s * .35, -s * .1), m + QPointF(0, -s * .45)); p.drawLine(m + QPointF(s * .35, -s * .1), m + QPointF(0, -s * .45));
    } else if (kind == "paused") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .25, -s * .35), m + QPointF(-s * .25, s * .35)); p.drawLine(m + QPointF(s * .25, -s * .35), m + QPointF(s * .25, s * .35));
    } else if (kind == "idle" || kind == "pending" || kind == "planning") {
        p.drawEllipse(c);
        if (kind == "idle") p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .45, 0));
    } else if (kind == "answer" || kind == "stuck") {
        p.drawEllipse(c);
        QFont f; f.setPixelSize(int(c.height() * .8)); f.setBold(true); p.setFont(f);
        p.drawText(c, Qt::AlignCenter, kind == "answer" ? QStringLiteral("?") : QStringLiteral("!"));
    } else if (kind == "sent") {
        // An outgoing arrow: the hand-off left for the member.
        p.drawLine(m + QPointF(-s * .45, s * .55), m + QPointF(s * .45, s * .55));
        p.drawLine(m + QPointF(-s * .45, 0), m + QPointF(s * .3, 0));
        p.drawLine(m + QPointF(s * .05, -s * .35), m + QPointF(s * .4, 0));
        p.drawLine(m + QPointF(s * .05, s * .35), m + QPointF(s * .4, 0));
    } else if (kind == "delivered" || kind == "late") {
        // An inbox: the task sits in the agent's terminal. "late" adds the dot
        // of a start that never came.
        QPainterPath tray;
        tray.moveTo(c.left() + s * .05, c.top() + s * .45);
        tray.lineTo(c.left() + s * .05, c.bottom() - s * .15);
        tray.lineTo(c.right() - s * .05, c.bottom() - s * .15);
        tray.lineTo(c.right() - s * .05, c.top() + s * .45);
        p.drawPath(tray);
        p.drawLine(QPointF(c.left() + s * .05, c.top() + s * .35), QPointF(c.right() - s * .05, c.top() + s * .35));
        if (kind == "late") {
            p.setPen(Qt::NoPen); p.setBrush(color);
            p.drawEllipse(QPointF(c.right() - s * .1, c.top() + s * .1), s * .3, s * .3);
        }
    } else if (kind == "asking") {
        // A speech bubble with the question it carries (A3).
        const QRectF bubble(c.left(), c.top() + s * .1, c.width(), c.height() - s * .4);
        p.drawRoundedRect(bubble, 2.5, 2.5);
        p.drawLine(QPointF(c.left() + s * .55, bubble.bottom()), QPointF(c.left() + s * .3, c.bottom() - s * .05));
        QFont f; f.setPixelSize(qMax(6, int(c.height() * .7))); f.setBold(true); p.setFont(f);
        p.drawText(bubble, Qt::AlignCenter, QStringLiteral("?"));
    } else if (kind == "held") {
        p.drawEllipse(c);
        p.drawLine(m + QPointF(-s * .22, -s * .35), m + QPointF(-s * .22, s * .35));
        p.drawLine(m + QPointF(s * .22, -s * .35), m + QPointF(s * .22, s * .35));
    } else if (kind == "reported" || kind == "no_report") {
        // A document: a check for a report, a cross for a turn that ended
        // without one.
        QPainterPath doc;
        doc.moveTo(c.left() + s * .25, c.top());
        doc.lineTo(c.right() - s * .55, c.top());
        doc.lineTo(c.right() - s * .15, c.top() + s * .4);
        doc.lineTo(c.right() - s * .15, c.bottom());
        doc.lineTo(c.left() + s * .25, c.bottom());
        doc.closeSubpath();
        p.drawPath(doc);
        p.drawLine(QPointF(c.right() - s * .55, c.top()), QPointF(c.right() - s * .55, c.top() + s * .4));
        p.drawLine(QPointF(c.right() - s * .55, c.top() + s * .4), QPointF(c.right() - s * .15, c.top() + s * .4));
        if (kind == "reported") {
            p.drawLine(m + QPointF(-s * .35, s * .1), m + QPointF(-s * .1, s * .35));
            p.drawLine(m + QPointF(-s * .1, s * .35), m + QPointF(s * .3, -s * .25));
        } else {
            p.drawLine(m + QPointF(-s * .3, -s * .2), m + QPointF(s * .2, s * .3));
            p.drawLine(m + QPointF(-s * .3, s * .3), m + QPointF(s * .2, -s * .2));
        }
    } else if (kind == "stopped") {
        p.setPen(Qt::NoPen); p.setBrush(color);
        p.drawRoundedRect(QRectF(c.left() + s * .15, c.top() + s * .15, c.width() - s * .3, c.height() - s * .3), 1.5, 1.5);
    } else if (kind == "rejected") {
        // A curved arrow back to the member.
        QPainterPath back;
        back.moveTo(c.right() - s * .1, c.bottom() - s * .2);
        back.cubicTo(c.right() - s * .15, c.top() + s * .15, c.left() + s * .1, c.top() + s * .1, c.left() + s * .2, m.y());
        p.drawPath(back);
        p.drawLine(QPointF(c.left() + s * .2, m.y()), QPointF(c.left() + s * .5, m.y() - s * .35));
        p.drawLine(QPointF(c.left() + s * .2, m.y()), QPointF(c.left() + s * .5, m.y() + s * .35));
    } else if (kind == "offline" || kind == "unconfirmed" || kind == "failed") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .4, -s * .4), m + QPointF(s * .4, s * .4)); p.drawLine(m + QPointF(-s * .4, s * .4), m + QPointF(s * .4, -s * .4));
    } else {
        p.drawEllipse(c);
    }
    p.restore();
}

// A glyph as a pixmap or icon, for buttons and rows that need one.
inline QPixmap glyphPixmap(const QColor &color, const QString &kind, int size = 14, double ratio = 2.0)
{
    QPixmap pix(int(size * ratio), int(size * ratio));
    pix.setDevicePixelRatio(ratio);
    pix.fill(Qt::transparent);
    QPainter p(&pix);
    glyph(p, QRectF(0, 0, size, size), color, kind);
    return pix;
}

inline QIcon glyphIcon(const QColor &color, const QString &kind, int size = 14)
{
    return QIcon(glyphPixmap(color, kind, size));
}

// "just now", "3 min ago", "2 h ago": the time a phase tooltip names.
inline QString agoText(double then, double now)
{
    if (then <= 0 || now <= then + 1) return QCoreApplication::translate("TeamPanel", "just now");
    const int seconds = int(now - then);
    if (seconds < 60) return QCoreApplication::translate("TeamPanel", "%1 s ago").arg(seconds);
    if (seconds < 3600) return QCoreApplication::translate("TeamPanel", "%1 min ago").arg(seconds / 60);
    if (seconds < 86400) return QCoreApplication::translate("TeamPanel", "%1 h ago").arg(seconds / 3600);
    return QCoreApplication::translate("TeamPanel", "%1 d ago").arg(seconds / 86400);
}

// An attempt phase as the desktop draws it (A1, spec 11): the glyph kind, its
// tone, and a tooltip naming the state, the time and the next step. `phase` is
// empty when the task has no attempt.
struct PhaseMark {
    QString phase, kind, tooltip;
    Tone tone = Tone::Neutral;
};

inline PhaseMark phaseMark(const Team &team, const Task &task, double now = 0)
{
    PhaseMark mark;
    mark.phase = lastPhase(task);
    mark.kind = mark.phase;
    if (mark.phase.isEmpty()) return mark;
    const auto tr = [](const char *text) { return QCoreApplication::translate("TeamPanel", text); };
    if (now <= 0) now = double(QDateTime::currentSecsSinceEpoch());
    const auto *attempt = task.last();
    const QString role = task.assignee.isEmpty() ? tr("You") : team.roleOf(task.assignee);
    const double delivered = attempt ? (attempt->deliveredAt > 0 ? attempt->deliveredAt : attempt->sentAt) : 0;
    if (mark.phase == "sent") {
        mark.tone = Tone::Assigned; mark.tooltip = tr("Sending to %1").arg(role);
    } else if (mark.phase == "delivered") {
        mark.tone = Tone::Assigned; mark.tooltip = tr("Delivered %1, not started yet").arg(agoText(delivered, now));
    } else if (mark.phase == "late") {
        mark.tone = Tone::Warn; mark.tooltip = tr("Delivered %1 and not started: check its terminal").arg(agoText(delivered, now));
    } else if (mark.phase == "working") {
        mark.tone = Tone::Active;
        mark.tooltip = attempt && attempt->startedAt > 0 ? tr("%1 started %2").arg(role, agoText(attempt->startedAt, now))
                                                         : tr("%1 is working on it").arg(role);
    } else if (mark.phase == "asking") {
        mark.tone = Tone::Warn;
        const auto *question = attempt ? attempt->openQuestion() : nullptr;
        mark.tooltip = question ? tr("%1 asks: \"%2\" (Answer)").arg(role, question->text)
                                : tr("%1 asks a question (Answer)").arg(role);
    } else if (mark.phase == "held") {
        mark.tone = Tone::Muted;
        mark.tooltip = task.hold.by.isEmpty() ? tr("Held: waiting for the owner")
                      : task.hold.reason.isEmpty() ? tr("Held by %1: waiting for the owner").arg(task.hold.by)
                                                   : tr("Held by %1: %2").arg(task.hold.by, task.hold.reason);
    } else if (mark.phase == "reported") {
        mark.tone = Tone::Done;
        mark.tooltip = !attempt || attempt->reportAt <= 0 ? tr("Report from %1").arg(role)
                                                          : tr("Report from %1, %2").arg(role, agoText(attempt->reportAt, now));
    } else if (mark.phase == "no_report") {
        mark.tone = Tone::Warn; mark.tooltip = tr("Turn ended without a report");
    } else if (mark.phase == "stopped") {
        mark.tone = Tone::Danger; mark.tooltip = tr("%1's session stopped").arg(role);
    } else if (mark.phase == "rejected") {
        mark.tone = Tone::Danger; mark.tooltip = tr("Rejected: returned to %1").arg(role);
    } else if (mark.phase == "unconfirmed") {
        mark.tone = Tone::Danger; mark.tooltip = tr("Delivery unconfirmed");
    }
    return mark;
}

// One row of a member's queue (A5): the task's phase icon, its number and
// title, and the phase's tooltip. The current task's row comes first.
inline QPushButton *memberQueueRow(const Team &team, const Task &task, const Colors &c, bool current)
{
    const auto mark = phaseMark(team, task);
    auto *row = new QPushButton;
    row->setObjectName("teamMemberQueueRow");
    row->setProperty("task", task.id);
    row->setProperty("current", current);
    row->setProperty("phase", mark.phase);
    row->setIcon(glyphIcon(toneColor(c, mark.tone == Tone::Neutral ? Tone::Muted : mark.tone), mark.kind, 13));
    row->setIconSize(QSize(13, 13));
    row->setText(QStringLiteral("#%1 %2").arg(task.id).arg(task.title));
    row->setFlat(true);
    row->setCursor(Qt::PointingHandCursor);
    row->setFixedHeight(18);
    row->setToolTip(mark.tooltip);
    if (!mark.tooltip.isEmpty()) row->setAccessibleName(row->text() + QStringLiteral(" - ") + mark.tooltip);
    row->setStyleSheet(QStringLiteral("QPushButton{text-align:left;font-size:11px;color:%1;border:none;padding:0 2px;}"
                                      "QPushButton:hover{color:%2;}").arg(current ? c.text2.name() : c.muted.name(), c.fg.name()));
    return row;
}


inline QString kindOfTask(const Task &task, const Status &status)
{
    // A finished task keeps its own mark (accepted, cancelled or rejected): hgs
    // sends no attempt phase for one on purpose (N4).
    if (task.finished()) return task.status;
    // A live task shows the attempt's phase; one hgs did not send is derived
    // (Teams::lastPhase).
    const QString phase = lastPhase(task);
    if (!phase.isEmpty()) return phase;
    if (status.tone == Tone::Danger) return QStringLiteral("unconfirmed");
    if (status.tone == Tone::Warn) return QStringLiteral("no_report");
    return task.status;
}

// The summary node of a collapsed phase: a finished phase says so, and both
// kinds stay clickable.
inline QString phaseSummaryText(int done, int total)
{
    if (total > 0 && done == total) return QCoreApplication::translate("TeamPanel", "%1/%2 done · click to open").arg(done).arg(total);
    return QCoreApplication::translate("TeamPanel", "%1/%2 · click to open").arg(done).arg(total);
}

// A pill: role badges and the team badge. Text, border and fill share a color.
class Pill : public QWidget {
public:
    Pill(const QString &text, const QColor &color, const QString &kind = {}, bool filled = true, QWidget *parent = nullptr)
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_filled(filled)
    {
        QFont f = font(); f.setPixelSize(11); f.setWeight(QFont::Medium); setFont(f);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        setToolTip(text);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_text) + 14 + (m_kind.isEmpty() ? 0 : 14), 18}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const QRectF r = QRectF(rect()).adjusted(.5, .5, -.5, -.5);
        p.setPen(QPen(alpha(m_color, m_filled ? .45 : 1), 1)); p.setBrush(m_filled ? alpha(m_color, .12) : Qt::transparent);
        p.drawRoundedRect(r, 9, 9);
        int x = 7;
        if (!m_kind.isEmpty()) { glyph(p, QRectF(6, 4, 10, 10), m_color, m_kind); x = 20; }
        p.setPen(m_color); p.drawText(QRect(x, 0, width() - x - 6, height()), Qt::AlignVCenter | Qt::AlignLeft,
                                      fontMetrics().elidedText(m_text, Qt::ElideRight, width() - x - 6));
    }
private:
    QString m_text; QColor m_color; QString m_kind; bool m_filled;
};

// A status: mark and text, elided; a fixed width keeps neighbours in place.
// `tooltip` names the state, the time and the next step when the text alone
// does not (the attempt phases of the delivery spec).
class StatusMark : public QWidget {
public:
    StatusMark(const QString &text, const QColor &color, const QString &kind, int fixedWidth = 0, Qt::Alignment align = Qt::AlignLeft,
               QWidget *parent = nullptr, const QString &tooltip = {})
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_align(align)
    {
        QFont f = font(); f.setPixelSize(12); setFont(f);
        if (fixedWidth) setFixedWidth(fixedWidth); else setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        setFixedHeight(18);
        const QString tip = tooltip.isEmpty() ? text : text.isEmpty() ? tooltip : tooltip + QLatin1Char('\n') + text;
        setToolTip(tip);
    }
    QSize sizeHint() const override { return {fontMetrics().horizontalAdvance(m_text) + 18, 18}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        const QString text = fontMetrics().elidedText(m_text, Qt::ElideRight, width() - 18);
        const int w = fontMetrics().horizontalAdvance(text) + 17;
        const int x = m_align & Qt::AlignRight ? width() - w : 0;
        glyph(p, QRectF(x, 3, 12, 12), m_color, m_kind);
        p.setPen(m_color); p.drawText(QRect(x + 17, 0, width() - x - 17, height()), Qt::AlignVCenter | Qt::AlignLeft, text);
    }
private:
    QString m_text; QColor m_color; QString m_kind; Qt::Alignment m_align;
};

class Progress : public QWidget {
public:
    Progress(const QList<QColor> &segments, int height, const QColor &empty, QWidget *parent = nullptr)
        : QWidget(parent), m_segments(segments), m_empty(empty) { setFixedHeight(height); }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing); p.setPen(Qt::NoPen);
        if (m_segments.isEmpty()) { p.setBrush(m_empty); p.drawRoundedRect(rect(), 2, 2); return; }
        const double gap = 2, w = (width() - gap * (m_segments.size() - 1)) / m_segments.size();
        for (int i = 0; i < m_segments.size(); ++i) {
            p.setBrush(m_segments[i]); p.drawRoundedRect(QRectF(i * (w + gap), 0, qMax(1.0, w), height()), 1.5, 1.5);
        }
    }
private:
    QList<QColor> m_segments; QColor m_empty;
};

// G6: the two compact bars that replace one bar over every task: the phase in
// work (or the next draft waiting for approval) with its done/total, and the
// phases with finished/planned, where a discarded draft never counts. The
// caption carries the live phase number; both bars share one tooltip.
class PhaseProgressBars : public QWidget {
public:
    PhaseProgressBars(const Colors &colors, const Teams::PhaseProgress &progress, bool compact = false, QWidget *parent = nullptr)
        : QWidget(parent), m_c(colors), m_p(progress), m_compact(compact)
    {
        setObjectName("teamPhaseProgress");
        setProperty("phase", progress.phase);
        setProperty("phaseDone", progress.done);
        setProperty("phaseTotal", progress.total);
        setProperty("phaseDraft", progress.draft);
        setProperty("phasesDone", progress.phasesDone);
        setProperty("phasesTotal", progress.phasesTotal);
        setToolTip(progress.tooltip());
        setFixedHeight(compact ? 16 : 18);
        setSizePolicy(QSizePolicy::Fixed, QSizePolicy::Fixed);
        setFixedWidth((compact ? 62 : 92) * 2 + (compact ? 8 : 10));
    }
    QSize sizeHint() const override { return {width(), height()}; }
protected:
    void paintEvent(QPaintEvent *) override
    {
        if (!m_p.valid()) return;
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        const int blockWidth = m_compact ? 62 : 92, gap = m_compact ? 8 : 10;
        const auto block = [&](int x, const QString &caption, const QString &count, int done, int total) {
            QFont f = font(); f.setPixelSize(9); f.setWeight(QFont::DemiBold); p.setFont(f);
            p.setPen(m_c.dim);
            p.drawText(QRect(x, 0, blockWidth, 10), Qt::AlignLeft | Qt::AlignVCenter, caption);
            QFont mono = font(); mono.setPixelSize(9); p.setFont(mono); p.setPen(m_c.text2);
            p.drawText(QRect(x, 0, blockWidth, 10), Qt::AlignRight | Qt::AlignVCenter, count);
            const QRectF track(x, 12, blockWidth, 4);
            p.setPen(Qt::NoPen); p.setBrush(m_c.border); p.drawRoundedRect(track, 2, 2);
            const double fraction = total > 0 ? qBound(0.0, done / double(total), 1.0) : 0.0;
            if (fraction > 0) { p.setBrush(m_c.accent); p.drawRoundedRect(QRectF(x, 12, blockWidth * fraction, 4), 2, 2); }
        };
        const QString phaseCaption = m_p.draft ? tr("Phase %1 · draft").arg(m_p.phase) : tr("Phase %1").arg(m_p.phase);
        block(0, phaseCaption, m_p.phaseText(), m_p.done, m_p.total);
        block(blockWidth + gap, tr("Phases"), m_p.phasesText(), m_p.phasesDone, m_p.phasesTotal);
    }
private:
    Colors m_c; Teams::PhaseProgress m_p; bool m_compact;
};

// Tasks as a graph or as a list by layer. Painted, so hovering a task lights
// its chain without moving anything. Graph runs top to bottom (the inspector);
// Wide runs left to right with layer columns (the team workspace). While
// planning, a node's handle drags onto another node to add a dependency, and a
// selected edge is removed with Delete or its ×.
// G9: the status chip of a phase header, from status.phase_summary.
inline QString phaseStatusText(const QString &status)
{
    if (status == "done") return QCoreApplication::translate("TeamPanel", "Done");
    if (status == "in_work") return QCoreApplication::translate("TeamPanel", "In work");
    if (status == "approved") return QCoreApplication::translate("TeamPanel", "Approved (not started)");
    if (status == "draft") return QCoreApplication::translate("TeamPanel", "Draft");
    if (status == "paused") return QCoreApplication::translate("TeamPanel", "Paused");
    return status;
}

inline QColor phaseStatusColor(const Colors &c, const QString &status)
{
    if (status == "done") return c.accent;
    if (status == "in_work") return c.folder;
    if (status == "approved") return c.muted;
    if (status == "draft") return c.warn;
    if (status == "paused") return c.dim;
    return c.muted;
}

class TaskView : public QWidget {
public:
    enum Mode { List, Graph, Wide };
    struct Node { int id = 0, layer = 0, phase = 1; QString title, role, kind, statusText, deps, agent, tooltip; QColor tone, roleTone; bool problem = false, draft = false, error = false, match = true; int attempt = 0; };
    // A phase group as the plan, board and layers show it: the heading is
    // "Phase 2 · QA fixes · 3/5". A finished phase starts collapsed; its tasks
    // are in `tasks` but no node is built, and one summary node stands in.
    // G9: the header also carries the phase's status, progress and health from
    // status.phase_summary/phases_health, and a draft's Approve/Discard.
    struct PhaseBand {
        int n = 0; QString heading, title; bool barrier = false, draft = false, collapsed = false; int done = 0, total = 0; QList<int> tasks;
        QString status;       // done|in_work|approved|draft|paused (empty: older hgs)
        QString healthLine;   // "path 8 · par 2.4 · 1 warning"
        QString healthTip;
        QString blocked;      // F12: why Approve is disabled, empty when ready
        QString blockedTip;
        int changes = 0;      // the lead's pending edits on a draft
    };
    TaskView(const Colors &colors, Mode mode, QWidget *parent = nullptr) : QWidget(parent), m_c(colors), m_mode(mode)
    {
        setMouseTracking(true);
        setFocusPolicy(Qt::ClickFocus);
        QFont f = font(); f.setPixelSize(12); setFont(f);
    }
    // Compatibility with the two-mode form.
    TaskView(const Colors &colors, bool graph, QWidget *parent = nullptr) : TaskView(colors, graph ? Graph : List, parent) {}
    std::function<void(int task, const QPoint &global)> clicked;
    // Planning: `task` gains a dependency on `dependency`, or loses it.
    std::function<void(int dependency, int task)> linked, unlinked;
    bool editable = false;
    QList<Node> nodes;
    // Phase grouping: empty while the plan is phase 1 only. The owner's
    // toggles call phaseToggled; the caller re-renders with the new collapse.
    QList<PhaseBand> bands;
    std::function<void(int phase)> phaseToggled;
    // G9: the draft decisions from the band header, and the F10 reorder.
    std::function<void(int phase)> phaseApprove, phaseDiscard, phaseMoveUp, phaseMoveDown;
    // G9: a decision goes out once; the header control stays off until a new
    // render builds a new view.
    QSet<int> decided;
    // The painted place of a band's header controls, for hit tests and tests.
    QRectF approveRect(int phase) const { return m_approve.value(phase); }
    QRectF discardRect(int phase) const { return m_discard.value(phase); }
    QRectF moveUpRect(int phase) const { return m_moveUp.value(phase); }
    QRectF moveDownRect(int phase) const { return m_moveDown.value(phase); }
    QString bandTooltip(int phase) const { return m_bandTips.value(phase); }
    const PhaseBand *bandOf(int phase) const
    {
        for (const auto &band : bands) if (band.n == phase) return &band;
        return nullptr;
    }
    // The painted place of a phase header or of a collapsed phase's summary
    // node, in the view's own (unzoomed) coordinates.
    QRectF bandRect(int phase) const { return m_bandRects.value(phase); }
    QRectF summaryRect(int phase) const { return m_summaries.value(phase); }
    // G3: where a task landed; a dead-end sits below the tasks others need.
    QRectF nodeRect(int id) const { return m_rects.value(id); }
    QMap<int, QList<int>> edges;   // task -> its dependencies
    QSet<QPair<int, int>> soft;    // (dependency, task) edges that need only In progress
    QSet<int> cycle, emphasis;     // emphasis: Show what blocks
    std::function<QSet<int>(int)> chain;
    double zoom = 1.0;
    // Wide: the visible canvas; the graph centers in it and its layer bands
    // fill its height (design D3, no empty bottom).
    QSize area;
    // G4: one entry per layer inside a phase band, with the small "L2 · 3"
    // label rect and the count; the paint draws its stripe and label from this.
    struct BandLayer { int phase = 0, layer = 0, count = 0; QRectF rect; };
    QList<BandLayer> bandLayers() const { return m_bandLayers; }
    // Wide: the graph's own size at zoom 1.
    QSizeF natural() const
    {
        if (banded() && m_mode == Wide) {
            double wide = BandMin;
            int tall = 8;
            for (const auto &band : bands) {
                QMap<int, int> lines;
                for (int id : band.tasks) if (const Node *node = nodeOf(id)) ++lines[node->layer];
                int rows = 0;
                for (int count : lines) rows = qMax(rows, count);
                wide = qMax(wide, double(bandWidth(band)));
                tall += BandHead + (band.collapsed ? 0 : (BandTop - BandHead) + qMax(1, rows) * (WideH + 12) + 4) + BandGap;
            }
            return {wide + 8, double(tall + 2)};
        }
        QMap<int, int> column;
        for (const auto &n : nodes) ++column[n.layer];
        int layers = 0, tall = 0;
        for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); tall = qMax(tall, it.value()); }
        return {12.0 + layers * WideStep, 30.0 + tall * (WideH + 12) + 8};
    }
    void layout()
    {
        m_rects.clear(); m_bandRects.clear(); m_summaries.clear(); m_bandBackgrounds.clear(); m_listLayers.clear(); m_bandLayers.clear();
        m_approve.clear(); m_discard.clear(); m_moveUp.clear(); m_moveDown.clear();
        // G3: inside a layer (and inside a phase band) a task other tasks depend
        // on comes first, in the existing order; a dead-end task — nothing
        // depends on it — sinks to the bottom of its column.
        QSet<int> needed;
        for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) needed.insert(dep);
        if (!needed.isEmpty()) {
            std::stable_partition(nodes.begin(), nodes.end(), [&needed](const Node &n) { return needed.contains(n.id); });
            for (auto &band : bands)
                std::stable_partition(band.tasks.begin(), band.tasks.end(), [&needed](int id) { return needed.contains(id); });
        }
        QMap<int, int> column;
        if (m_mode == Graph) {
            if (banded()) {
                int y = 4, wide = Gutter + NodeW + 4;
                for (const auto &band : bands) {
                    QMap<int, int> used;
                    int layers = 0, bandWide = Gutter + NodeW + 4;
                    for (int id : band.tasks) {
                        const Node *node = nodeOf(id);
                        if (!node) continue;
                        layers = qMax(layers, node->layer + 1);
                        const int x = Gutter + used[node->layer]++ * (NodeW + 6);
                        m_rects.insert(id, QRectF(x, y + BandTop + node->layer * (NodeH + 26), NodeW, NodeH));
                        bandWide = qMax(bandWide, x + NodeW + 4);
                    }
                    m_bandRects.insert(band.n, QRectF(0, y, bandWide, BandHead));
                    m_bandBackgrounds.append(QRectF(0, y, bandWide, 0));
                    if (band.collapsed) { m_summaries.insert(band.n, QRectF(Gutter, y + BandTop, NodeW, NodeH)); m_rects.insert(-band.n, m_summaries.value(band.n)); }
                    wide = qMax(wide, bandWide);
                    y += BandTop + qMax(1, layers) * (NodeH + 26) + BandGap;
                }
                setFixedSize(int((wide + 4) * zoom), int((y + 2) * zoom));
            } else {
                for (const auto &n : nodes) {
                    const int x = Gutter + column[n.layer]++ * (NodeW + 6), y = 8 + n.layer * (NodeH + 26);
                    m_rects.insert(n.id, QRectF(x, y, NodeW, NodeH));
                }
                int layers = 0, wide = 0;
                for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); wide = qMax(wide, it.value()); }
                setFixedSize(int((Gutter + wide * (NodeW + 6) + 4) * zoom), int((8 + layers * (NodeH + 26)) * zoom));
            }
        } else if (m_mode == Wide) {
            const QSizeF size = natural();
            const double viewW = area.isValid() ? area.width() / zoom : 0, viewH = area.isValid() ? area.height() / zoom : 0;
            m_ox = qMax(0.0, (viewW - size.width()) / 2);
            m_oy = banded() ? 0.0 : qMax(0.0, (viewH - size.height()) / 2);
            if (banded()) {
                // G9: phases stack top to bottom; a band's layers run left to
                // right inside it. A collapsed band is one header row.
                const double width = qMax(size.width() - 8, viewW > 0 ? viewW - 16 : 0.0);
                double y = 4;
                for (const auto &band : bands) {
                    QMap<int, int> counts;
                    for (int id : band.tasks) if (const Node *node = nodeOf(id)) ++counts[node->layer];
                    int rows = 0;
                    for (int count : counts) rows = qMax(rows, count);
                    const double body = band.collapsed ? 0 : (BandTop - BandHead) + qMax(1, rows) * (WideH + 12) + 4;
                    m_bandRects.insert(band.n, QRectF(8, y, width, BandHead - 4));
                    m_bandBackgrounds.append(QRectF(0, y - 4, width + 16, BandHead + body + 6));
                    if (band.collapsed) {
                        // G9: a collapsed band is its header row alone; edges
                        // that end in it attach to the header.
                        m_rects.insert(-band.n, m_bandRects.value(band.n));
                    } else {
                        QMap<int, int> used;
                        for (int id : band.tasks) {
                            const Node *node = nodeOf(id);
                            if (!node) continue;
                            m_rects.insert(id, QRectF(14 + node->layer * WideStep, y + BandTop + used[node->layer]++ * (WideH + 12), WideW, WideH));
                        }
                        // G4: each layer inside the band keeps its stripe and
                        // small label above the band's tasks.
                        for (auto it = counts.cbegin(); it != counts.cend(); ++it) {
                            const double left = 14 + it.key() * WideStep;
                            const double w = qMin(double(WideStep), width - left + 8);
                            if (w <= 4) continue;
                            BandLayer layer; layer.phase = band.n; layer.layer = it.key(); layer.count = it.value();
                            layer.rect = QRectF(left, y + BandHead, w, BandTop - BandHead);
                            m_bandLayers.append(layer);
                        }
                    }
                    y += BandHead + body + BandGap;
                }
                m_layers = 0;
                const double finalW = qMax(size.width(), viewW);
                const double finalH = qMax(qMax(double(y), size.height()), viewH);
                setFixedSize(int(finalW * zoom), int(finalH * zoom));
            } else {
                for (const auto &n : nodes) {
                    const double x = m_ox + 12 + n.layer * WideStep, y = m_oy + 30 + column[n.layer]++ * (WideH + 12);
                    m_rects.insert(n.id, QRectF(x, y, WideW, WideH));
                }
                int layers = 0;
                for (auto it = column.cbegin(); it != column.cend(); ++it) layers = qMax(layers, it.key() + 1);
                m_layers = layers;
                setFixedSize(int(qMax(size.width(), viewW) * zoom), int(qMax(size.height(), viewH) * zoom));
            }
        } else if (banded()) {
            int y = 0;
            for (const auto &band : bands) {
                if (y) y += 8;
                m_bandRects.insert(band.n, QRectF(0, y, 10000, BandHead));
                y += BandHead + 2;
                if (band.collapsed) continue;
                QMap<int, int> counts;
                for (int id : band.tasks) if (const Node *node = nodeOf(id); node && node->match) ++counts[node->layer];
                int layer = -1;
                for (int id : band.tasks) {
                    const Node *node = nodeOf(id);
                    if (!node || !node->match) continue;
                    if (node->layer != layer) {
                        layer = node->layer;
                        m_listLayers.append({band.n, layer, counts.value(layer), QRectF(0, y, 10000, 20)});
                        y += 22;
                    }
                    m_rects.insert(id, QRectF(0, y, 10000, 38));
                    y += 40;
                }
            }
            setMinimumHeight(y + 4); setMaximumHeight(y + 4);
        } else {
            int y = 0, layer = -1;
            for (const auto &n : nodes) {
                if (!n.match) continue;
                if (n.layer != layer) { layer = n.layer; y += 24; }
                m_rects.insert(n.id, QRectF(0, y, 10000, 38)); y += 40;
            }
            setMinimumHeight(y + 4); setMaximumHeight(y + 4);
        }
        update();
    }
    QPair<int, int> selectedEdge() const { return m_edge; }
protected:
    static constexpr int NodeW = 118, NodeH = 46, Gutter = 20, WideW = 146, WideH = 76, WideStep = 170;
    static constexpr int BandHead = 40, BandTop = 58, BandGap = 10, BandMin = 240;
    bool banded() const { return !bands.isEmpty(); }
    const Node *nodeOf(int id) const
    {
        for (const auto &node : nodes) if (node.id == id) return &node;
        return nullptr;
    }
    // Wide: a band is as wide as its own layers, and at least one header.
    int bandWidth(const PhaseBand &band) const
    {
        int layers = 0;
        for (int id : band.tasks) if (const Node *node = nodeOf(id)) layers = qMax(layers, node->layer + 1);
        return qMax(BandMin, qMax(1, layers) * WideStep);
    }
    // A collapsed phase's tasks hang their edges on the phase summary node.
    int anchor(int id) const
    {
        if (m_rects.contains(id)) return id;
        for (const auto &band : bands)
            if (band.collapsed && band.tasks.contains(id) && m_bandRects.contains(band.n)) return -band.n;
        return 0;
    }
    // The phase header or summary node under a point, 0 when there is none.
    int phaseAt(const QPointF &position) const
    {
        const QPointF point = graph() ? position / zoom : position;
        for (auto it = m_bandRects.cbegin(); it != m_bandRects.cend(); ++it) if (it.value().contains(point)) return it.key();
        for (auto it = m_summaries.cbegin(); it != m_summaries.cend(); ++it) if (it.value().contains(point)) return it.key();
        return 0;
    }
    // A phase header: chevron, "Phase 2 · QA fixes", the phase status chip,
    // done/total, the health line and, for a draft, Approve and Discard (and
    // the F10 move arrows). G9: without phase_summary (older hgs) it falls back
    // to today's heading with the barrier and draft tags.
    void paintBand(QPainter &p, const PhaseBand &band, const QRectF &r)
    {
        p.setOpacity(1);
        QStringList tips;
        if (!band.healthTip.isEmpty()) tips << band.healthTip;
        if (!band.blockedTip.isEmpty()) tips << band.blockedTip;
        if (band.blocked.isEmpty() && band.healthLine.isEmpty()) m_bandTips.remove(band.n);
        else m_bandTips.insert(band.n, tips.join(QLatin1Char('\n')));
        const double line1 = r.top() + 3, lineH = 16;
        const QPointF middle(r.left() + 7, line1 + lineH / 2);
        QPainterPath chevron;
        if (band.collapsed) { chevron.moveTo(middle + QPointF(-2, -4)); chevron.lineTo(middle + QPointF(3, 0)); chevron.lineTo(middle + QPointF(-2, 4)); }
        else { chevron.moveTo(middle + QPointF(-4, -2)); chevron.lineTo(middle + QPointF(0, 3)); chevron.lineTo(middle + QPointF(4, -2)); }
        p.setPen(Qt::NoPen); p.setBrush(m_c.muted); p.drawPath(chevron);
        QFont titleFont = font(); titleFont.setPixelSize(11); titleFont.setWeight(QFont::DemiBold);
        QFont metaFont = font(); metaFont.setPixelSize(9); metaFont.setWeight(QFont::DemiBold);
        const QFontMetrics tf(titleFont), mf(metaFont);
        double right = r.right() - 6;
        const auto pill = [&](const QString &text, const QColor &tone) {
            const double width = QFontMetrics(metaFont).horizontalAdvance(text) + 12;
            const QRectF box(right - width, line1, width, lineH);
            p.setPen(QPen(alpha(tone, .5), 1)); p.setBrush(alpha(tone, .12)); p.drawRoundedRect(box, 7, 7);
            p.setFont(metaFont); p.setPen(tone); p.drawText(box, Qt::AlignCenter, text);
            right -= width + 5;
        };
        const auto control = [&](const QString &text, const QColor &tone, bool enabled, QRectF *store) {
            const double width = mf.horizontalAdvance(text) + 14;
            const QRectF box(right - width, line1, width, lineH);
            p.setPen(QPen(enabled ? alpha(tone, .65) : m_c.border, 1));
            p.setBrush(enabled ? alpha(tone, .12) : QColor(Qt::transparent));
            p.drawRoundedRect(box, 4, 4);
            p.setFont(metaFont); p.setPen(enabled ? tone : m_c.dim);
            p.drawText(box, Qt::AlignCenter, text);
            if (store) *store = box;
            right -= width + 5;
        };
        if (band.draft) {
            control(QCoreApplication::translate("TeamPanel", "Approve"), m_c.accent, band.blocked.isEmpty() && !decided.contains(band.n), &m_approve[band.n]);
            control(QCoreApplication::translate("TeamPanel", "Discard"), m_c.danger, !decided.contains(band.n), &m_discard[band.n]);
            const QRectF up(right - 14, line1, 14, lineH), down(right - 30, line1, 14, lineH);
            m_moveUp.insert(band.n, up); m_moveDown.insert(band.n, down);
            p.setFont(metaFont); p.setPen(m_c.text3);
            p.drawText(up, Qt::AlignCenter, QStringLiteral("↑"));
            p.drawText(down, Qt::AlignCenter, QStringLiteral("↓"));
            right -= 36;
        } else {
            m_approve.remove(band.n); m_discard.remove(band.n); m_moveUp.remove(band.n); m_moveDown.remove(band.n);
        }
        if (!band.status.isEmpty()) {
            pill(phaseStatusText(band.status), phaseStatusColor(m_c, band.status));
        } else if (band.draft) {
            pill(QCoreApplication::translate("TeamPanel", "draft"), m_c.warn);
        }
        if (!band.status.isEmpty() || band.draft) {
            const QString progress = QStringLiteral("%1/%2").arg(band.done).arg(band.total);
            const double width = mf.horizontalAdvance(progress) + 2;
            p.setFont(metaFont); p.setPen(m_c.muted);
            p.drawText(QRectF(right - width, line1, width, lineH), Qt::AlignRight | Qt::AlignVCenter, progress);
            right -= width + 8;
        }
        const double titleLeft = r.left() + 17;
        const double room = qMax(0.0, right - titleLeft);
        p.setFont(titleFont); p.setPen(m_c.text2);
        p.drawText(QRectF(titleLeft, line1, room, lineH), Qt::AlignLeft | Qt::AlignVCenter, tf.elidedText(band.heading, Qt::ElideRight, int(room)));
        // The second line: the phase health and, for a blocked draft, the F12
        // reason. Details are in the header's tooltip.
        QStringList meta;
        if (!band.healthLine.isEmpty()) meta << band.healthLine;
        if (!band.blocked.isEmpty()) meta << band.blocked;
        if (band.changes > 0) meta << QCoreApplication::translate("TeamPanel", "%1 change(s) from the lead").arg(band.changes);
        if (!meta.isEmpty()) {
            QFont small = font(); small.setPixelSize(9);
            const bool blocked = !band.blocked.isEmpty();
            p.setFont(small); p.setPen(blocked ? m_c.warn : m_c.text3);
            p.drawText(QRectF(titleLeft, r.top() + 21, qMax(0.0, r.width() - 24), 13), Qt::AlignLeft | Qt::AlignVCenter,
                       QFontMetrics(small).elidedText(meta.join(QStringLiteral(" · ")), Qt::ElideRight, int(qMax(0.0, r.width() - 24))));
        }
    }
    // A collapsed phase stands in for its tasks in the graph.
    void paintSummary(QPainter &p, int phase, const QRectF &r)
    {
        const PhaseBand *band = nullptr;
        for (const auto &candidate : bands) if (candidate.n == phase) band = &candidate;
        p.setOpacity(1);
        p.setPen(QPen(m_c.border, 1, Qt::DashLine)); p.setBrush(m_c.card);
        p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 8, 8);
        p.setPen(Qt::NoPen); p.setBrush(m_c.dim); p.drawRoundedRect(QRectF(r.left() + 1, r.top() + 6, 3, r.height() - 12), 1.5, 1.5);
        QFont title = font(); title.setPixelSize(11); title.setWeight(QFont::DemiBold); p.setFont(title); p.setPen(m_c.fg);
        const QString text = !band || band->title.isEmpty() ? QCoreApplication::translate("TeamPanel", "Phase %1").arg(phase)
                                                            : QCoreApplication::translate("TeamPanel", "Phase %1 · %2").arg(phase).arg(band->title);
        p.drawText(r.adjusted(9, 6, -6, -18), Qt::AlignLeft | Qt::AlignTop, QFontMetrics(title).elidedText(text, Qt::ElideRight, int(r.width() - 16)));
        QFont meta = font(); meta.setPixelSize(10); p.setFont(meta); p.setPen(m_c.muted);
        p.drawText(r.adjusted(9, 0, -6, -7), Qt::AlignLeft | Qt::AlignBottom,
                   phaseSummaryText(band ? band->done : 0, band ? band->total : 0));
    }
    // The layer list: a "LAYER n · count" rule and one task row.
    void paintLayerHeader(QPainter &p, int layer, int count, const QRectF &r)
    {
        QFont caps = font(); caps.setPixelSize(10); caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .6);
        p.setOpacity(1); p.setFont(caps); p.setPen(m_c.dim);
        const QString text = QCoreApplication::translate("TeamPanel", "LAYER %1 · %2").arg(layer + 1).arg(count);
        p.drawText(QRectF(2, r.top(), width(), r.height()), Qt::AlignLeft | Qt::AlignVCenter, text);
        const int lw = QFontMetrics(caps).horizontalAdvance(text) + 10;
        p.setPen(m_c.border); p.drawLine(QPointF(lw, r.center().y()), QPointF(width() - 2, r.center().y()));
    }
    void paintListRow(QPainter &p, const Node &n, bool dim)
    {
        const QRectF r = m_rects[n.id];
        p.setOpacity(dim ? .35 : 1);
        const QRectF box(1, r.top(), width() - 2, r.height());
        if (n.problem || n.error) {
            p.setPen(QPen(n.tone == m_c.warn ? m_c.warn : m_c.danger, 1)); p.setBrush(alpha(n.tone == m_c.warn ? m_c.warn : m_c.danger, .07));
            p.drawRoundedRect(box.adjusted(.5, .5, -.5, -.5), 7, 7);
        } else if (m_hover == n.id) { p.setPen(Qt::NoPen); p.setBrush(alpha(m_c.fg, .05)); p.drawRoundedRect(box, 7, 7); }
        glyph(p, QRectF(8, r.top() + 5, 12, 12), n.tone, n.kind);
        QFont mono = NewSessionDense::mono(11); p.setFont(mono); p.setPen(m_c.dim);
        const QString id = QStringLiteral("#%1").arg(n.id);
        const int idw = QFontMetrics(mono).horizontalAdvance(id) + 6;
        p.drawText(QRectF(26, r.top() + 2, idw, 18), Qt::AlignLeft | Qt::AlignVCenter, id);
        QFont small = font(); small.setPixelSize(10); small.setWeight(QFont::Medium);
        const int roleW = QFontMetrics(small).horizontalAdvance(n.role) + 14;
        const QRectF pill(width() - roleW - 6, r.top() + 3, roleW, 16);
        p.setPen(QPen(alpha(n.roleTone, .45), 1)); p.setBrush(alpha(n.roleTone, .12)); p.drawRoundedRect(pill, 8, 8);
        p.setFont(small); p.setPen(n.roleTone); p.drawText(pill, Qt::AlignCenter, n.role);
        QFont title = font(); title.setPixelSize(12); p.setFont(title); p.setPen(m_c.fg);
        const double titleW = pill.left() - 32 - idw;
        p.drawText(QRectF(26 + idw, r.top() + 2, titleW, 18), Qt::AlignLeft | Qt::AlignVCenter, QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(titleW)));
        QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.tone);
        p.drawText(QRectF(26, r.top() + 19, width() - 120, 16), Qt::AlignLeft | Qt::AlignVCenter, n.statusText);
        if (!n.deps.isEmpty()) {
            p.setFont(NewSessionDense::mono(10)); p.setPen(m_c.dim);
            p.drawText(QRectF(width() - 112, r.top() + 19, 104, 16), Qt::AlignRight | Qt::AlignVCenter, n.deps);
        }
    }
    bool graph() const { return m_mode != List; }
    QPointF out(const QRectF &r) const { return m_mode == Wide ? QPointF(r.right(), r.center().y()) : QPointF(r.center().x(), r.bottom()); }
    QPointF in(const QRectF &r) const { return m_mode == Wide ? QPointF(r.left(), r.center().y()) : QPointF(r.center().x(), r.top()); }
    QRectF handle(const QRectF &r) const
    {
        return m_mode == Wide ? QRectF(r.right() - 4, r.center().y() - 6, 8, 12) : QRectF(r.center().x() - 6, r.bottom() - 4, 12, 8);
    }
    QPainterPath edgePath(int dep, int task) const
    {
        const QRectF a = m_rects[dep], b = m_rects[task];
        QPainterPath path;
        const bool forward = m_mode == Wide ? b.left() > a.right() : b.top() > a.bottom();
        if (forward) {
            const QPointF s = out(a), e = in(b);
            const QPointF bend = m_mode == Wide ? QPointF(18, 0) : QPointF(0, 14);
            path.moveTo(s); path.cubicTo(s + bend, e - bend, e);
        } else {   // a back edge of a cycle: a loop beside the nodes
            const QPointF s(a.right(), a.center().y()), e(b.right(), b.center().y());
            path.moveTo(s); path.cubicTo(s + QPointF(28, 0), e + QPointF(28, 0), e);
        }
        return path;
    }
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        if (graph()) p.fillRect(rect(), m_c.surface);
        const QSet<int> lit = m_hover && chain && !m_dragFrom ? chain(m_hover) : QSet<int>();
        const auto dimmed = [&](const Node &n) {
            if (!lit.isEmpty()) return !lit.contains(n.id);
            if (!emphasis.isEmpty()) return !emphasis.contains(n.id);
            return graph() && !n.match;
        };
        if (graph()) {
            p.scale(zoom, zoom);
            if (banded() && m_mode == Wide) {
                for (int i = 0; i < m_bandBackgrounds.size(); ++i)
                    if (i % 2) p.fillRect(m_bandBackgrounds[i], alpha(m_c.fg, .025));
                // G4: the alternating layer stripes and the small layer label
                // live inside every band, under its phase header.
                QFont caps = font(); caps.setPixelSize(9); caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .5);
                const double bottom = height() / zoom;
                for (const auto &layer : m_bandLayers) {
                    if (layer.layer % 2)
                        p.fillRect(QRectF(layer.rect.left(), layer.rect.top(), layer.rect.width(), qMax(0.0, bottom - layer.rect.top())), alpha(m_c.fg, .03));
                    p.setFont(caps); p.setPen(m_c.dim);
                    p.drawText(layer.rect.adjusted(2, 0, -2, 0), Qt::AlignLeft | Qt::AlignVCenter,
                               QStringLiteral("L%1 · %2").arg(layer.layer + 1).arg(layer.count));
                }
            }
            QFont small = font(); small.setPixelSize(10); p.setFont(small); p.setPen(m_c.dim);
            if (banded()) {
                // The phase headers name the bands; each band draws its own layer
                // labels and stripes above.
            } else if (m_mode == Wide) {
                QMap<int, int> counts; for (const auto &n : nodes) ++counts[n.layer];
                for (int l = 0; l < m_layers; ++l) {
                    if (l % 2) p.fillRect(QRectF(m_ox + 4 + l * WideStep, 0, WideStep, height() / zoom), alpha(m_c.fg, .025));
                    QFont caps = small; caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .6); p.setFont(caps);
                    p.drawText(QRectF(m_ox + 12 + l * WideStep, 6, WideStep, 18), Qt::AlignLeft | Qt::AlignVCenter,
                               QCoreApplication::translate("TeamPanel", "LAYER %1 · %2").arg(l + 1).arg(counts.value(l)));
                }
            } else {
                QSet<int> labelled;
                for (const auto &n : nodes) if (!labelled.contains(n.layer)) {
                    labelled.insert(n.layer); p.drawText(QRectF(0, m_rects[n.id].top(), Gutter, NodeH), Qt::AlignCenter, QStringLiteral("L%1").arg(n.layer + 1));
                }
            }
            for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
                const int from = anchor(dep), to = anchor(it.key());
                if (!from || !to || from == to) continue;
                const bool hot = lit.contains(dep) && lit.contains(it.key());
                const bool bad = cycle.contains(dep) && cycle.contains(it.key());
                const bool selected = m_edge == qMakePair(dep, it.key());
                QColor color = selected || bad ? m_c.danger : hot ? m_c.accent : m_c.border;
                if (!hot && !selected && (!lit.isEmpty() || !emphasis.isEmpty()) && !(emphasis.contains(dep) && emphasis.contains(it.key()))) color = alpha(color, .35);
                p.setPen(QPen(color, hot || selected ? 2 : 1.4, selected || soft.contains({dep, it.key()}) ? Qt::DashLine : Qt::SolidLine)); p.setBrush(Qt::NoBrush);
                const auto path = edgePath(from, to);
                p.drawPath(path);
                const QPointF e = path.pointAtPercent(1), before = path.pointAtPercent(.96);
                const QLineF tail(e, before);
                QLineF wing1 = tail; wing1.setLength(5); wing1.setAngle(tail.angle() + 35);
                QLineF wing2 = tail; wing2.setLength(5); wing2.setAngle(tail.angle() - 35);
                p.drawLine(wing1); p.drawLine(wing2);
                if (selected) {   // the × that removes it
                    const QPointF mid = path.pointAtPercent(.5);
                    p.setPen(Qt::NoPen); p.setBrush(m_c.danger); p.drawEllipse(mid, 7, 7);
                    p.setPen(QPen(Qt::white, 1.5)); p.drawLine(mid + QPointF(-3, -3), mid + QPointF(3, 3)); p.drawLine(mid + QPointF(-3, 3), mid + QPointF(3, -3));
                }
            }
            if (m_dragFrom && m_rects.contains(m_dragFrom)) {
                p.setPen(QPen(m_c.accent, 1.6, Qt::DashLine)); p.drawLine(out(m_rects[m_dragFrom]), m_dragAt / zoom);
            }
            for (const auto &n : nodes) {
                const QRectF r = m_rects[n.id];
                p.setOpacity(dimmed(n) ? .35 : 1);
                const bool target = m_dragFrom && m_hover == n.id && n.id != m_dragFrom;
                p.setPen(target ? QPen(m_c.accent, 2)
                         : n.error || n.problem ? QPen(n.error || n.tone == m_c.danger ? m_c.danger : m_c.warn, 1)
                                                : QPen(m_c.border, 1, n.draft ? Qt::DashLine : Qt::SolidLine));
                p.setBrush(n.error ? alpha(m_c.danger, .1) : m_c.card);
                p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 8, 8);
                p.setPen(Qt::NoPen); p.setBrush(n.tone); p.drawRoundedRect(QRectF(r.left() + 1, r.top() + 6, 3, r.height() - 12), 1.5, 1.5);
                QFont mono = NewSessionDense::mono(10); p.setFont(mono); p.setPen(m_c.dim);
                const QString id = QStringLiteral("#%1").arg(n.id);
                p.drawText(QRectF(r.left() + 9, r.top() + 5, 30, 16), Qt::AlignLeft | Qt::AlignVCenter, id);
                const int idw = QFontMetrics(mono).horizontalAdvance(id) + 4;
                QFont title = font(); title.setPixelSize(12); if (m_mode == Wide) title.setWeight(QFont::DemiBold); p.setFont(title); p.setPen(m_c.fg);
                if (m_mode == Wide) {
                    // Two lines of title, then role and status.
                    const QRectF text(r.left() + 9 + idw, r.top() + 5, r.width() - 14 - idw, 32);
                    const QFontMetrics fm(title);
                    QString shown = n.title;
                    if (fm.boundingRect(text.toRect(), Qt::TextWordWrap, shown).height() > 32) {
                        while (shown.size() > 4 && fm.boundingRect(text.toRect(), Qt::TextWordWrap, shown + QStringLiteral("…")).height() > 32) shown.chop(1);
                        shown += QStringLiteral("…");
                    }
                    p.drawText(text, Qt::TextWordWrap | Qt::AlignLeft | Qt::AlignTop, shown);
                    QFont small = font(); small.setPixelSize(10); small.setWeight(QFont::Medium);
                    const int roleW = QFontMetrics(small).horizontalAdvance(n.role) + 14;
                    const QRectF pill(r.left() + 9, r.top() + 38, roleW, 15);
                    p.setPen(QPen(alpha(n.roleTone, .45), 1)); p.setBrush(alpha(n.roleTone, .12)); p.drawRoundedRect(pill, 7, 7);
                    p.setFont(small); p.setPen(n.roleTone); p.drawText(pill, Qt::AlignCenter, n.role);
                    if (!n.agent.isEmpty()) agentLogo(n.agent, m_c.text3).paint(&p, QRect(int(pill.right() + 5), int(pill.top()), 14, 14));
                    glyph(p, QRectF(r.left() + 9, r.top() + 58, 11, 11), n.tone, n.kind);
                    QFont meta = font(); meta.setPixelSize(11); p.setFont(meta); p.setPen(n.tone);
                    p.drawText(QRectF(r.left() + 24, r.top() + 55, r.width() - 30, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(meta).elidedText(n.statusText, Qt::ElideRight, int(r.width() - 30)));
                } else {
                    p.drawText(QRectF(r.left() + 9 + idw, r.top() + 5, r.width() - 14 - idw, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(r.width() - 14 - idw)));
                    p.setPen(Qt::NoPen); p.setBrush(n.roleTone); p.drawEllipse(QRectF(r.left() + 10, r.top() + 29, 7, 7));
                    QFont role = font(); role.setPixelSize(11); p.setFont(role); p.setPen(m_c.text3);
                    const QString who = n.role + (n.attempt > 1 ? QStringLiteral(" ·%1").arg(n.attempt) : QString());
                    p.drawText(QRectF(r.left() + 21, r.top() + 24, r.width() - 42, 16), Qt::AlignLeft | Qt::AlignVCenter,
                               QFontMetrics(role).elidedText(who, Qt::ElideRight, int(r.width() - 42)));
                    glyph(p, QRectF(r.right() - 17, r.top() + 26, 12, 12), n.tone, n.kind);
                }
                if (n.error) { p.setPen(m_c.danger); p.drawText(QRectF(r.right() - 14, r.top() + 2, 12, 12), Qt::AlignCenter, QStringLiteral("!")); }
                if (editable && (m_hover == n.id || m_dragFrom == n.id)) {
                    p.setOpacity(1); p.setPen(Qt::NoPen); p.setBrush(m_c.accent); p.drawRoundedRect(handle(r), 3, 3);
                }
            }
            // A collapsed phase is one summary node; its header stays above.
            // G9: in the Wide graph a collapsed band is its header row only.
            if (m_mode != Wide)
                for (auto it = m_summaries.cbegin(); it != m_summaries.cend(); ++it) paintSummary(p, it.key(), it.value());
            for (const auto &band : bands) paintBand(p, band, m_bandRects.value(band.n));
            return;
        }
        if (banded()) {
            for (const auto &band : bands) {
                paintBand(p, band, m_bandRects.value(band.n));
                if (band.collapsed) continue;
                for (const auto &layer : m_listLayers) {
                    if (layer.phase != band.n) continue;
                    paintLayerHeader(p, layer.n, layer.count, layer.rect);
                }
                for (int id : band.tasks) {
                    const Node *node = nodeOf(id);
                    if (!node || !node->match) continue;
                    paintListRow(p, *node, dimmed(*node));
                }
            }
            return;
        }
        int layer = -1, count = 0;
        QMap<int, int> perLayer;
        for (const auto &n : nodes) if (n.match) ++perLayer[n.layer];
        for (const auto &n : nodes) {
            if (!n.match) continue;
            if (n.layer != layer) { layer = n.layer; count = perLayer[layer]; paintLayerHeader(p, layer, count, QRectF(2, m_rects[n.id].top() - 24, width() - 4, 20)); }
            paintListRow(p, n, dimmed(n));
        }
    }
    int at(const QPointF &position) const
    {
        const QPointF point = graph() ? position / zoom : position;
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it) {
            if (it.key() < 0) continue;   // a collapsed phase's summary is no task
            QRectF r = it.value(); if (!graph()) r.setWidth(width());
            if (r.adjusted(0, 0, 0, editable ? 4 : 0).contains(point)) return it.key();
        }
        return 0;
    }
    // The edge under the point, within 5px, or its × when selected.
    QPair<int, int> edgeAt(const QPointF &position) const
    {
        const QPointF point = position / zoom;
        for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
            if (!m_rects.contains(dep) || !m_rects.contains(it.key())) continue;
            const auto path = edgePath(dep, it.key());
            for (int i = 0; i <= 40; ++i) if (QLineF(path.pointAtPercent(i / 40.), point).length() <= 5) return {dep, it.key()};
        }
        return {0, 0};
    }
    void mousePressEvent(QMouseEvent *e) override
    {
        if (e->button() == Qt::LeftButton && phaseAt(e->position())) return;   // toggled on release
        if (!editable || !graph() || e->button() != Qt::LeftButton) return QWidget::mousePressEvent(e);
        const QPointF point = e->position() / zoom;
        if (m_edge.first && QLineF(edgePath(m_edge.first, m_edge.second).pointAtPercent(.5), point).length() <= 8) {
            const auto edge = m_edge; m_edge = {0, 0}; update();
            if (unlinked) unlinked(edge.first, edge.second);
            return;
        }
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it)
            if (it.key() > 0 && handle(it.value()).adjusted(-4, -4, 4, 4).contains(point)) { m_dragFrom = it.key(); m_dragAt = e->position(); m_edge = {0, 0}; update(); return; }
        if (!at(e->position())) { m_edge = edgeAt(e->position()); update(); }
    }
    void mouseMoveEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom) { m_dragAt = e->position(); m_hover = id; update(); return; }
        if (phaseAt(e->position())) {
            if (m_hover) { m_hover = 0; update(); }
            setCursor(Qt::PointingHandCursor);
            setToolTip(bandTooltip(phaseAt(e->position())));
            return;
        }
        if (id != m_hover) {
            m_hover = id;
            // The hovered node's phase tooltip names the state, the time and
            // the next step (spec 11).
            QString tip;
            for (const auto &node : nodes) if (node.id == id) { tip = node.tooltip; break; }
            setToolTip(tip);
            setCursor(id ? Qt::PointingHandCursor : Qt::ArrowCursor);
            update();
        }
    }
    void leaveEvent(QEvent *) override { if (!m_dragFrom) { m_hover = 0; setToolTip({}); update(); } }
    void mouseReleaseEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom) {
            const int from = m_dragFrom; m_dragFrom = 0; update();
            if (id && id != from && linked) linked(from, id);
            return;
        }
        if (e->button() == Qt::LeftButton) {
            const QPointF point = graph() ? e->position() / zoom : e->position();
            for (auto it = m_approve.cbegin(); it != m_approve.cend(); ++it)
                if (it.value().contains(point)) {
                    const auto *band = bandOf(it.key());
                    if (band && band->blocked.isEmpty() && !decided.contains(it.key()) && phaseApprove) {
                        decided.insert(it.key()); update();
                        phaseApprove(it.key());
                    }
                    return;
                }
            for (auto it = m_discard.cbegin(); it != m_discard.cend(); ++it)
                if (it.value().contains(point)) {
                    if (!decided.contains(it.key()) && phaseDiscard) { decided.insert(it.key()); update(); phaseDiscard(it.key()); }
                    return;
                }
            for (auto it = m_moveUp.cbegin(); it != m_moveUp.cend(); ++it)
                if (it.value().contains(point)) { if (phaseMoveUp) phaseMoveUp(it.key()); return; }
            for (auto it = m_moveDown.cbegin(); it != m_moveDown.cend(); ++it)
                if (it.value().contains(point)) { if (phaseMoveDown) phaseMoveDown(it.key()); return; }
            const int phase = phaseAt(e->position());
            if (phase) { if (phaseToggled) phaseToggled(phase); return; }
        }
        if (id && e->button() == Qt::LeftButton && clicked) clicked(id, e->globalPosition().toPoint());
    }
    void keyPressEvent(QKeyEvent *e) override
    {
        if (editable && m_edge.first && (e->key() == Qt::Key_Delete || e->key() == Qt::Key_Backspace)) {
            const auto edge = m_edge; m_edge = {0, 0}; update();
            if (unlinked) unlinked(edge.first, edge.second);
            return;
        }
        QWidget::keyPressEvent(e);
    }
private:
    Colors m_c;
    Mode m_mode;
    int m_hover = 0, m_dragFrom = 0, m_layers = 0;
    double m_ox = 0, m_oy = 0;
    QPointF m_dragAt;
    QPair<int, int> m_edge{0, 0};
    QMap<int, QRectF> m_rects;
    // Phase grouping: a header per band, one summary rect per collapsed band
    // (keyed by phase), the alternating wide bands, and the layer rules inside
    // the list's phase groups.
    QMap<int, QRectF> m_bandRects, m_summaries;
    // G9: the bands' header controls and tooltips, rebuilt by layout().
    QMap<int, QRectF> m_approve, m_discard, m_moveUp, m_moveDown;
    QMap<int, QString> m_bandTips;
    QList<QRectF> m_bandBackgrounds;
    QList<BandLayer> m_bandLayers;
    struct ListLayer { int phase = 0, n = 0, count = 0; QRectF rect; };
    QList<ListLayer> m_listLayers;
};

// G9: the tooltip details behind a phase's health line (C7 numbers + warnings).
inline QString phaseHealthTip(const Team &team, int phase)
{
    const auto *health = team.phaseHealthOf(phase);
    if (!health) return QString();
    QStringList details;
    details << QCoreApplication::translate("TeamPanel", "%1 tasks · critical path %2 · parallelism %3")
                   .arg(health->tasks).arg(health->criticalPath)
                   .arg(health->parallelism, 0, 'f', 1);
    for (const auto &warning : health->warnings) details << warning;
    return details.join(QLatin1Char('\n'));
}

// The phase bands a task view draws, with the owner's collapse choices.
// G9: status.phase_summary gives the status chip and the health line;
// phases_health the tooltip details; phases[].waiting the disabled reason.
inline QList<TaskView::PhaseBand> phaseBands(const Team &team, const std::function<bool(const PhaseGroup &)> &open)
{
    QList<TaskView::PhaseBand> bands;
    const auto tr = [](const char *text, const QString &a = {}, const QString &b = {}) {
        return QCoreApplication::translate("TeamPanel", text).arg(a, b);
    };
    for (const auto &group : phaseGroups(team)) {
        TaskView::PhaseBand band;
        band.n = group.n; band.title = group.title; band.heading = phaseHeading(group);
        band.barrier = group.barrier; band.draft = group.draft; band.done = group.done; band.total = group.tasks.size();
        band.tasks = group.tasks; band.collapsed = !open(group);
        if (const auto *summary = team.phaseSummaryOf(group.n)) {
            band.status = summary->status;
            band.healthLine = summary->healthLine;
            // With a summary the header owns the numbers, so the heading is
            // just "Phase N · title" (the fallback keeps today's heading).
            band.heading = summary->title.isEmpty() ? tr("Phase %1", QString::number(group.n))
                                                    : tr("Phase %1 · %2", QString::number(group.n), summary->title);
            band.done = summary->done;
            band.total = summary->total;
            band.healthTip = phaseHealthTip(team, group.n);
        }
        // F12: a draft whose predecessor is not ready says why Approve is off.
        if (const auto *record = team.phase(group.n); record && !record->waiting.text.isEmpty()) {
            band.blocked = record->waiting.text;
            QStringList open;
            for (int id : record->waiting.open) open << QStringLiteral("#%1").arg(id);
            QString tip = tr("Phase %1 is not ready: %2", QString::number(record->waiting.phase),
                             open.isEmpty() ? tr("no open tasks listed") : open.join(QStringLiteral(", ")));
            const QString also = approveBlocked(team, group.n);
            if (!also.isEmpty()) tip += QLatin1Char('\n') + also;
            band.blockedTip = tip;
        } else if (band.draft) {
            const QString blocked = approveBlocked(team, group.n);
            if (!blocked.isEmpty()) { band.blocked = approveBlockedShort(team, group.n); band.blockedTip = blocked; }
        }
        if (const auto *record = team.phase(group.n)) band.changes = record->changes.size();
        bands.append(band);
    }
    return bands;
}

// The tasks a grouped view builds nodes for, phase by phase and each phase's
// own layers in order; a collapsed phase builds none.
inline QList<int> phaseOrder(const Team &team, const QList<TaskView::PhaseBand> &bands)
{
    QList<int> order;
    for (const auto &band : bands) {
        if (band.collapsed) continue;
        const auto layers = phaseLayers(team, band.tasks);
        QMap<int, QList<int>> byLayer;
        for (int id : band.tasks) byLayer[layers.value(id)].append(id);
        for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    }
    return order;
}

// Replaced content: a click inside it may still be running, so it goes once
// control returns; meanwhile it is hidden and keeps no names to be found by.
inline void retire(QObject *object)
{
    if (!object) return;
    if (auto *widget = qobject_cast<QWidget *>(object)) widget->hide();
    object->setObjectName({});
    for (auto *child : object->findChildren<QObject *>()) child->setObjectName({});
    object->deleteLater();
}

inline QLabel *label(const QString &text, const QColor &color, int pixels = 12, bool bold = false, const char *name = nullptr)
{
    auto *l = new QLabel(text);
    QFont f = l->font(); f.setPixelSize(pixels); if (bold) f.setWeight(QFont::DemiBold); l->setFont(f);
    QPalette pal = l->palette(); pal.setColor(QPalette::WindowText, color); l->setPalette(pal);
    l->setTextFormat(Qt::PlainText);
    if (name) l->setObjectName(name);
    return l;
}

// QA: a label that keeps its whole text (for tests and tooltips) and paints it
// elided with an ellipsis when the row is narrower than the text.
class ElidingLabel : public QLabel {
public:
    explicit ElidingLabel(const QString &text, QWidget *parent = nullptr) : QLabel(text, parent)
    {
        setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
        setMinimumWidth(20);
    }
protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.setPen(palette().color(QPalette::WindowText));
        p.drawText(rect(), Qt::AlignVCenter | Qt::AlignLeft,
                   fontMetrics().elidedText(text(), Qt::ElideRight, width()));
    }
};

inline QLabel *elided(const QString &text, const QColor &color, int pixels = 12)
{
    auto *l = label(text, color, pixels);
    l->setToolTip(text);
    l->setMinimumWidth(20);
    l->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    return l;
}

// A member's queue (A5) under its current task: the open task first, then the
// tasks returned to it, at most five rows and "+N more". Nothing when the only
// entry is the current task, which the card already names.
inline QWidget *memberQueueWidget(const Team &team, const Member &member, const Colors &c, const std::function<void(int)> &open)
{
    if (member.queue.size() < 2) return nullptr;
    auto *box = new QWidget;
    box->setObjectName("teamMemberQueue");
    box->setProperty("member", member.id);
    auto *v = new QVBoxLayout(box);
    v->setContentsMargins(0, 0, 0, 0); v->setSpacing(1);
    const int shown = qMin(5, int(member.queue.size()));
    for (int i = 0; i < shown; ++i) {
        const auto *task = team.task(member.queue[i]);
        if (!task) continue;
        auto *row = memberQueueRow(team, *task, c, i == 0);
        QObject::connect(row, &QPushButton::clicked, row, [open, id = task->id] { if (open) open(id); });
        v->addWidget(row);
    }
    if (member.queue.size() > shown) {
        auto *more = label(QCoreApplication::translate("TeamPanel", "+%1 more").arg(member.queue.size() - shown), c.dim, 11, false, "teamMemberQueueMore");
        v->addWidget(more);
    }
    return box;
}

inline QPushButton *button(const QString &text, const char *name, bool primary = false)
{
    auto *b = new QPushButton(text);
    b->setObjectName(name); b->setAutoDefault(false); b->setCursor(Qt::PointingHandCursor);
    b->setProperty("primary", primary);
    return b;
}

inline QFrame *card(const Colors &c, const QColor &border = {})
{
    auto *f = new QFrame;
    f->setObjectName("teamCard");
    f->setStyleSheet(QStringLiteral("QFrame#teamCard{background:%1;border:1px solid %2;border-radius:9px;}")
                         .arg(c.surface.name(QColor::HexArgb), (border.isValid() ? border : c.border).name(QColor::HexArgb)));
    return f;
}

inline QHBoxLayout *row(int spacing = 6)
{
    auto *l = new QHBoxLayout; l->setContentsMargins(0, 0, 0, 0); l->setSpacing(spacing); return l;
}

inline QString agentIcon(const QString &agent)
{
    return QStringList{"claude", "codex", "kimi", "opencode", "dsh"}.contains(agent) ? agent : QStringLiteral("terminal");
}

// An agent's own logo (LobeHub lobe-icons, MIT, in tray/resources/agent-icons):
// in its colors where it has them, its monochrome parts in `color`; the line
// icon for an agent without one.
inline QIcon agentLogo(const QString &agent, const QColor &color)
{
    const QIcon logo = agentLogoIcon(agent, color);
    return logo.isNull() ? workspaceIcon(agentIcon(agent), color) : logo;
}

inline QString clock(double at) { return QDateTime::fromSecsSinceEpoch(qint64(at)).toString("HH:mm"); }

// 12m, 3h 12m, 3d 4h, 2w 3d (design HANDOFF §14, fmtDur).
inline QString duration(double seconds)
{
    const qint64 s = qMax<qint64>(0, qint64(seconds));
    const qint64 minutes = s / 60, hours = s / 3600, days = s / 86400;
    if (hours < 1) return QCoreApplication::translate("TeamPanel", "%1m").arg(qMax<qint64>(1, minutes));
    if (days < 1) return QCoreApplication::translate("TeamPanel", "%1h %2m").arg(hours).arg(minutes % 60);
    if (days < 7) return QCoreApplication::translate("TeamPanel", "%1d %2h").arg(days).arg(hours % 24);
    return QCoreApplication::translate("TeamPanel", "%1w %2d").arg(days / 7).arg(days % 7);
}

// The WhiteWh look of team surfaces: ghost buttons, accent primary ones,
// segmented controls (a widget with property seg) and fields. Set on the
// surface's top widget; children inherit it.
inline QString teamSheet(const Colors &c)
{
    const QString accentText = c.dark ? QStringLiteral("#0f1a16") : QStringLiteral("#ffffff");
    const QString accentHover = c.dark ? QStringLiteral("#a8e6cc") : QStringLiteral("#115d46");
    const QString segmentOn = c.dark ? QStringLiteral("#26443a") : QStringLiteral("#e0f0e9");
    const auto a = [](const QColor &color, double value) { return alpha(color, value).name(QColor::HexArgb); };
    return QStringLiteral(
        "QPushButton{background:transparent;color:%1;border:1px solid %2;border-radius:7px;padding:4px 10px;}"
        "QPushButton:hover{background:%3;}"
        "QPushButton:disabled{color:%4;border-color:%5;}"
        "QPushButton[primary=\"true\"]{background:%6;color:%7;border:1px solid %6;font-weight:600;}"
        "QPushButton[primary=\"true\"]:hover{background:%8;border-color:%8;}"
        "QPushButton[primary=\"true\"]:disabled{background:%9;color:%10;border-color:transparent;}"
        "QPushButton[danger=\"true\"]{color:%11;border-color:transparent;}"
        "QPushButton[ghost=\"true\"]{border-color:transparent;}"
        "QPushButton[chip=\"true\"]{border-radius:11px;padding:2px 9px;color:%12;}"
        "QWidget[seg=\"true\"]{background:%13;border:1px solid %2;border-radius:8px;}"
        "QWidget[seg=\"true\"] QPushButton{border:none;border-radius:6px;padding:4px 12px;color:%12;background:transparent;}"
        "QWidget[seg=\"true\"] QPushButton:checked{background:%14;color:%6;font-weight:600;}"
        "QLineEdit,QComboBox,QPlainTextEdit,QSpinBox{background:%15;color:%1;border:1px solid %2;border-radius:6px;padding:3px 6px;}"
        "QLineEdit:focus,QComboBox:focus,QPlainTextEdit:focus,QSpinBox:focus{border-color:%6;}"
        "QLineEdit:disabled,QComboBox:disabled,QPlainTextEdit:disabled{color:%4;}")
        .arg(c.fg.name(), c.border.name(), a(c.fg, .06), c.dim.name(), a(c.border, .6), c.accent.name(), accentText, accentHover,
             a(c.accent, .35), a(QColor(accentText), .7), c.danger.name(), c.text2.name(), c.tile.name(), segmentOn, c.card.name());
}

// Items that keep their width and wrap onto the next line.
class FlowLayout : public QLayout {
public:
    explicit FlowLayout(int spacing) { setSpacing(spacing); setContentsMargins(0, 0, 0, 0); }
    ~FlowLayout() override { while (auto *item = takeAt(0)) delete item; }
    void addItem(QLayoutItem *item) override { m_items.append(item); }
    int count() const override { return m_items.size(); }
    QLayoutItem *itemAt(int index) const override { return m_items.value(index); }
    QLayoutItem *takeAt(int index) override { return index >= 0 && index < m_items.size() ? m_items.takeAt(index) : nullptr; }
    Qt::Orientations expandingDirections() const override { return {}; }
    bool hasHeightForWidth() const override { return true; }
    int heightForWidth(int width) const override { return arrange(QRect(0, 0, width, 0), false); }
    void setGeometry(const QRect &rect) override { QLayout::setGeometry(rect); arrange(rect, true); }
    QSize sizeHint() const override { return minimumSize(); }
    QSize minimumSize() const override
    {
        QSize size; for (auto *item : m_items) size = size.expandedTo(item->minimumSize());
        return size;
    }
private:
    int arrange(const QRect &rect, bool apply) const
    {
        int x = rect.x(), y = rect.y(), line = 0;
        for (auto *item : m_items) {
            const QSize hint = item->sizeHint();
            if (x + hint.width() > rect.right() + 1 && line > 0) { x = rect.x(); y += line + spacing(); line = 0; }
            if (apply) item->setGeometry(QRect(QPoint(x, y), hint));
            x += hint.width() + spacing(); line = qMax(line, hint.height());
        }
        return y + line - rect.y();
    }
    QList<QLayoutItem *> m_items;
};

// A segmented control: one checked button among `items` (value, text).
inline QWidget *segmented(const QList<std::pair<QString, QString>> &items, const QString &current, const char *name,
                          const std::function<void(const QString &)> &picked)
{
    auto *box = new QWidget; box->setObjectName(name); box->setProperty("seg", true);
    box->setAttribute(Qt::WA_StyledBackground);
    auto *h = new QHBoxLayout(box); h->setContentsMargins(2, 2, 2, 2); h->setSpacing(0);
    for (const auto &[value, text] : items) {
        auto *b = new QPushButton(text); b->setCheckable(true); b->setChecked(value == current); b->setAutoDefault(false);
        b->setAutoExclusive(true); b->setCursor(Qt::PointingHandCursor); b->setProperty("segment", value);
        h->addWidget(b);
        QObject::connect(b, &QPushButton::clicked, box, [picked, value = value] { if (picked) picked(value); });
    }
    box->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed);
    return box;
}

// The same control with icons: `items` are (value, icon, tooltip); it keeps
// its width however narrow the window gets.
inline QWidget *iconSegmented(const QList<std::tuple<QString, QString, QString>> &items, const QString &current, const char *name,
                              const QColor &color, const std::function<void(const QString &)> &picked)
{
    QList<std::pair<QString, QString>> plain;
    for (const auto &item : items) plain.append({std::get<0>(item), QString()});
    auto *box = segmented(plain, current, name, picked);
    const auto buttons = box->findChildren<QPushButton *>();
    for (int i = 0; i < buttons.size() && i < items.size(); ++i) {
        const auto &[value, icon, tip] = items[i];
        buttons[i]->setIcon(workspaceIcon(icon, color)); buttons[i]->setIconSize(QSize(16, 16));
        buttons[i]->setToolTip(tip); buttons[i]->setAccessibleName(tip);
        buttons[i]->setStyleSheet(QStringLiteral("padding:4px 8px;"));
    }
    return box;
}

}
