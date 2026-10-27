#pragma once
#include "CachedText.h"
#include <QPainter>
#include <QPolygonF>
#include <cmath>

namespace SessionStatusBadge {
enum Kind { Neutral, Working, Attention, Error, Paused, Unread, Draft, Scheduled, Background, Waiting, MutedAttention, MutedUnread,
            // U30 (#508): the session states of SESSION-STATES.md.
            Deps, Build, Cooling, Cold, Stopped };
// U30 §6: the state tokens, dark and light.
inline QColor stateTone(Kind kind, bool dark) {
    switch (kind) {
    case Deps: return QColor(dark ? "#6fa8ff" : "#2f66c9");
    case Cooling: return QColor(dark ? "#56c8e0" : "#137f95");
    case Cold: return QColor(dark ? "#8fa6c4" : "#55708f");
    case Build: return QColor(dark ? "#4cc98a" : "#2a9d63");
    case Stopped: return QColor(dark ? "#8b939e" : "#6b7785");
    default: break;
    }
    return {};
}
inline bool isState(Kind kind) { return kind == Deps || kind == Build || kind == Cooling || kind == Cold || kind == Stopped; }
// The state glyphs: hourglass, build, thermo, snow, stop (10 px in a badge, 12 px on line 3).
inline void glyph(QPainter *p, const QPointF &c, qreal size, const QString &name, const QColor &color) {
    p->save(); p->setRenderHint(QPainter::Antialiasing);
    const qreal h = size / 2;
    QPen pen(color, size >= 12 ? 1.3 : 1.15, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin); p->setPen(pen); p->setBrush(Qt::NoBrush);
    if (name == QLatin1String("hourglass")) {
        p->drawLine(c + QPointF(-h * .7, -h), c + QPointF(h * .7, -h)); p->drawLine(c + QPointF(-h * .7, h), c + QPointF(h * .7, h));
        QPolygonF glass; glass << c + QPointF(-h * .55, -h) << c + QPointF(h * .55, h) << c + QPointF(-h * .55, h) << c + QPointF(h * .55, -h);
        p->drawPolyline(glass);
    } else if (name == QLatin1String("thermo")) {
        p->drawLine(c + QPointF(0, -h), c + QPointF(0, h * .35));
        p->setBrush(color); p->drawEllipse(c + QPointF(0, h * .55), h * .38, h * .38);
    } else if (name == QLatin1String("snow")) {
        for (int i = 0; i < 3; ++i) {
            const qreal a = i * M_PI / 3;
            p->drawLine(c + QPointF(std::cos(a) * h, std::sin(a) * h), c - QPointF(std::cos(a) * h, std::sin(a) * h));
        }
    } else if (name == QLatin1String("stop")) {
        p->setBrush(color); p->drawRoundedRect(QRectF(c.x() - h * .65, c.y() - h * .65, h * 1.3, h * 1.3), 1, 1);
    } else if (name == QLatin1String("build")) {
        // A terminal prompt: the build runs in its own shell.
        p->drawLine(c + QPointF(-h * .8, -h * .55), c + QPointF(-h * .15, 0)); p->drawLine(c + QPointF(-h * .15, 0), c + QPointF(-h * .8, h * .55));
        p->drawLine(c + QPointF(h * .1, h * .6), c + QPointF(h * .85, h * .6));
    }
    p->restore();
}
inline QColor mutedTone(bool dark) { return QColor(dark ? "#c4ccd5" : "#637080"); }
inline QFont font(QFont base) { base.setPixelSize(11); base.setWeight(QFont::Medium); return base; }
// The state's color for a frame around the session; invalid for Neutral.
inline QColor edge(Kind kind, bool dark) {
    switch (kind) {
    case Working: return QColor(dark ? "#4cc98a" : "#2a9d63");
    case Background: return QColor(dark ? "#86a9d3" : "#466b99");
    case Attention: case Unread: return QColor(dark ? "#ffda76" : "#e0a82e");
    case MutedAttention: case MutedUnread: return mutedTone(dark);
    case Error: return QColor(dark ? "#ff8fa0" : "#c7405a");
    case Paused: return QColor(dark ? "#a988d6" : "#9474c0");
    case Draft: return QColor(dark ? "#6b9bd8" : "#5b8cc9");
    case Scheduled: return QColor(dark ? "#5ab8cf" : "#2b8aa1");
    case Waiting: return QColor(dark ? "#d4ae65" : "#a37a24");
    case Deps: case Build: case Cooling: case Cold: case Stopped: return stateTone(kind, dark);
    case Neutral: break;
    }
    return {};
}
inline int width(const QString &text, Kind kind, const QFont &base) {
    // Leave room for fractional glyph advances before elidedText rounds them.
    // Cooling's 16 px heat bar sits after its caption.
    return CachedText::width(font(base), text) + 16 + (kind == Neutral ? 0 : 16) + (kind == Cooling ? 22 : 0);
}
// The maximum halo is a 5px ellipse. Include its antialiasing fringe.
inline QPointF indicatorCenter(const QRect &rect) { return {qreal(rect.x() + 11), rect.center().y() + .5}; }
inline QRect workingIndicatorRect(const QRect &rect) {
    const auto c = indicatorCenter(rect);
    return QRectF(c.x() - 6, c.y() - 6, 12, 12).toAlignedRect().intersected(rect);
}
// WhiteWh fork: the cards view outlines calm states and keeps the ones that
// ask for the user filled. Set by the window with the skin.
inline bool &outlined() { static bool value = false; return value; }
// labelOpacity fades the caption while the collapsed session strip grows into cards.
inline void paint(QPainter *p, const QRect &rect, const QString &text, Kind kind, bool dark, qreal pulse = 0, qreal labelOpacity = 1, bool indicatorOnly = false, qreal heat = -1) {
    p->save(); p->setRenderHint(QPainter::Antialiasing); p->setClipRect(rect, Qt::IntersectClip);
    if (isState(kind)) {
        // U30 §6: a frame in the state's tone (about half), a light fill for
        // deps and cold, the glyph and the caption in the tone; Cooling's heat
        // bar shows the warmth left.
        const QColor tone = stateTone(kind, dark);
        QColor line = tone; line.setAlphaF(.5);
        QColor fill = tone; fill.setAlphaF(kind == Deps || kind == Cold ? .11 : 0);
        p->setPen(QPen(line, 1)); p->setBrush(fill);
        p->drawRoundedRect(QRectF(rect).adjusted(.5, .5, -.5, -.5), rect.height() / 2., rect.height() / 2.);
        const QPointF center = indicatorCenter(rect);
        glyph(p, center, 10, kind == Deps ? QStringLiteral("hourglass") : kind == Cooling ? QStringLiteral("thermo") : kind == Cold ? QStringLiteral("snow")
                             : kind == Stopped ? QStringLiteral("stop") : QStringLiteral("build"), tone);
        if (indicatorOnly) { p->restore(); return; }
        p->setPen(tone); p->setFont(font(p->font())); p->setOpacity(p->opacity() * labelOpacity);
        const int bar = kind == Cooling ? 22 : 0;
        const QRect label = rect.adjusted(23, 0, -7 - bar, 0);
        CachedText::draw(p, label, Qt::AlignVCenter, labelOpacity < 1 ? text : CachedText::elided(p->font(), text, Qt::ElideRight, qMax(0, label.width())));
        if (kind == Cooling && heat >= 0) {
            const QRectF track(rect.right() - 7 - 16, rect.center().y() - 1, 16, 3);
            QColor back = tone; back.setAlphaF(.25);
            p->setPen(Qt::NoPen); p->setBrush(back); p->drawRoundedRect(track, 1.5, 1.5);
            p->setBrush(tone); p->drawRoundedRect(QRectF(track.x(), track.y(), track.width() * qBound<qreal>(.04, heat, 1), track.height()), 1.5, 1.5);
        }
        p->restore();
        return;
    }
    const QColor background = kind == Working ? QColor(dark ? "#194d36" : "#ccefdc")
        : kind == Waiting ? QColor(dark ? "#443a25" : "#f5e8c7")
        : kind == Background ? QColor(dark ? "#273544" : "#e9f0f8")
        : (kind == Attention || kind == Unread) ? QColor(dark ? "#ffda76" : "#f4ce65")
        : kind == Error ? QColor(dark ? "#ffabb6" : "#b73750")
        : kind == Paused ? QColor(dark ? "#443654" : "#e8ddf5")
        : kind == Draft ? QColor(dark ? "#263446" : "#e3edf9")
        : kind == Scheduled ? QColor(dark ? "#173e48" : "#d3edf4") : QColor(dark ? "#2b343e" : "#e7edf2");
    const QColor foreground = kind == Working ? QColor(dark ? "#97f0ba" : "#145b37")
        : kind == Waiting ? QColor(dark ? "#edd096" : "#795913")
        : kind == Background ? QColor(dark ? "#aac0df" : "#466b99")
        : (kind == Attention || kind == Unread) ? QColor("#392900")
        : kind == Error ? QColor(dark ? "#46202a" : "#ffffff")
        : kind == Paused ? QColor(dark ? "#dec3ff" : "#634187")
        : kind == Draft ? QColor(dark ? "#9cc0ec" : "#2f5f93")
        : kind == Scheduled ? QColor(dark ? "#9fe0f0" : "#165a6b") : QColor(dark ? "#b6c2d0" : "#566575");
    const bool calls = kind == Attention || kind == Unread || kind == Error;
    if (outlined() && !calls) {
        QColor tone = edge(kind, dark).isValid() ? edge(kind, dark) : QColor(dark ? "#8b939e" : "#8a96a3");
        QColor fill = tone; fill.setAlphaF(dark ? .12 : .10);
        QColor line = tone; line.setAlphaF(.5);
        p->setPen(QPen(line, 1)); p->setBrush(fill); p->drawRoundedRect(QRectF(rect).adjusted(.5, .5, -.5, -.5), rect.height() / 2., rect.height() / 2.);
    } else { p->setPen(Qt::NoPen); p->setBrush(background); p->drawRoundedRect(rect, 5, 5); }
    const QPointF center = indicatorCenter(rect);
    p->setFont(font(p->font())); p->setPen(foreground);
    if (kind == Working) {
        p->setPen(Qt::NoPen); QColor halo(foreground); halo.setAlphaF(.08 + .25 * pulse);
        p->setBrush(halo); p->drawEllipse(center, 4 + pulse, 4 + pulse);
        p->setBrush(foreground); p->drawEllipse(center, 2.5, 2.5);
    } else if (kind == Background) {
        p->setPen(Qt::NoPen);p->setBrush(foreground);p->drawEllipse(center,2.5,2.5);
    } else if (kind == Attention || kind == Error || kind == MutedAttention) {
        auto bold = p->font(); bold.setBold(true); p->setFont(bold);
        CachedText::draw(p, QRectF(center.x() - 5, rect.y(), 10, rect.height()), Qt::AlignCenter, QStringLiteral("!"));
    } else if (kind == Paused) {
        p->setPen(QPen(foreground, 1.5, Qt::SolidLine, Qt::RoundCap));
        p->drawLine(center + QPointF(-2, -3), center + QPointF(-2, 3));
        p->drawLine(center + QPointF(2, -3), center + QPointF(2, 3));
    } else if (kind == Draft) {
        // A pencil: an unsent message waits in the composer.
        p->setPen(QPen(foreground, 1.4, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        p->drawLine(center + QPointF(-3, 3), center + QPointF(2.5, -2.5));
        p->drawLine(center + QPointF(1.5, -3.5), center + QPointF(3.5, -1.5));
        p->drawLine(center + QPointF(-3, 3), center + QPointF(-3.5, 3.5));
    } else if (kind == Scheduled || kind == Waiting) {
        // A clock: the agent's own schedule starts its next turn.
        p->setPen(QPen(foreground, 1.3, Qt::SolidLine, Qt::RoundCap)); p->setBrush(Qt::NoBrush);
        p->drawEllipse(center, 4.5, 4.5);
        p->drawLine(center, center + QPointF(0, -2.8));
        p->drawLine(center, center + QPointF(2.2, 0));
    } else if (kind == Unread || kind == MutedUnread) {
        p->setPen(QPen(foreground, 1)); p->setBrush(Qt::NoBrush);
        p->drawRoundedRect(QRectF(center.x() - 4, center.y() - 3, 8, 6), 1, 1);
        p->drawLine(center + QPointF(-4, -3), center); p->drawLine(center, center + QPointF(4, -3));
    }
    if (indicatorOnly) { p->restore(); return; }
    p->setPen(foreground); p->setFont(font(p->font())); p->setOpacity(p->opacity() * labelOpacity);
    const QRect label = rect.adjusted(kind == Neutral ? 7 : 23, 0, -7, 0);
    // A growing badge reveals its caption under the clip instead of re-eliding it.
    CachedText::draw(p, label, Qt::AlignVCenter, labelOpacity < 1 ? text : CachedText::elided(p->font(), text, Qt::ElideRight, qMax(0, label.width())));
    p->restore();
}
}
