#pragma once
// Painted pieces shared by the Team tab, the task
// card and the team workspace: theme colors, role and status badges, the
// segmented progress and the task graph or layered list.
#include "TeamModel.h"
#include "WorkspaceIcons.h"
#include <QCoreApplication>
#include <QDateTime>
#include <QFontDatabase>
#include <QFrame>
#include <QHBoxLayout>
#include <QLabel>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QKeyEvent>
#include <cmath>
#include <functional>
#include <tuple>

namespace TeamUi {
using namespace Teams;

struct Colors {
    QColor fg, text2, text3, muted, dim, accent, accentMuted, warn, danger, folder, surface, border, card, tile;
    bool dark = true;
};

inline Colors colorsFor(bool dark)
{
    const auto pick = [dark](const char *d, const char *l) { return QColor(dark ? d : l); };
    Colors c;
    c.dark = dark; c.fg = pick("#e6e8eb", "#1a2733"); c.text2 = pick("#c4cad1", "#334250"); c.text3 = pick("#aab1ba", "#4d5c6b");
    c.muted = pick("#9aa3ad", "#627082"); c.dim = pick("#6f7883", "#8d97a2");
    c.accent = pick("#7dd3b0", "#167357"); c.warn = pick("#e8a75a", "#a66000"); c.danger = pick("#ef7d6d", "#b3261e"); c.folder = pick("#7fb6e0", "#2f6f9f");
    c.accentMuted = pick("#5c9e86", "#4f8f78");
    c.surface = pick("#1b1f26", "#ffffff");
    c.border = pick("#2c323b", "#dce2e8"); c.card = pick("#151920", "#f5f7f9"); c.tile = pick("#0f1318", "#eef2f5");
    return c;
}

// Ids, counts and times line up in a monospaced font.
inline QFont mono(int pixels)
{
    auto font = QFontDatabase::systemFont(QFontDatabase::FixedFont);
    font.setFamilies({QStringLiteral("JetBrains Mono"), QStringLiteral("Cascadia Mono"), QStringLiteral("Consolas"),
                      QStringLiteral("SF Mono"), QStringLiteral("Menlo"), QStringLiteral("DejaVu Sans Mono"), font.family()});
    font.setStyleHint(QFont::Monospace);
    font.setPixelSize(pixels);
    return font;
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
    } else if (kind == "answer" || kind == "no_report" || kind == "stuck") {
        p.drawEllipse(c);
        QFont f; f.setPixelSize(int(c.height() * .8)); f.setBold(true); p.setFont(f);
        p.drawText(c, Qt::AlignCenter, kind == "answer" ? QStringLiteral("?") : QStringLiteral("!"));
    } else if (kind == "offline" || kind == "unconfirmed" || kind == "rejected" || kind == "failed" || kind == "stopped") {
        p.drawEllipse(c); p.drawLine(m + QPointF(-s * .4, -s * .4), m + QPointF(s * .4, s * .4)); p.drawLine(m + QPointF(-s * .4, s * .4), m + QPointF(s * .4, -s * .4));
    } else {
        p.drawEllipse(c);
    }
    p.restore();
}

inline QString kindOfTask(const Task &task, const Status &status)
{
    if (status.tone == Tone::Danger) return QStringLiteral("unconfirmed");
    if (status.tone == Tone::Warn) return QStringLiteral("no_report");
    return task.status;
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
class StatusMark : public QWidget {
public:
    StatusMark(const QString &text, const QColor &color, const QString &kind, int fixedWidth = 0, Qt::Alignment align = Qt::AlignLeft, QWidget *parent = nullptr)
        : QWidget(parent), m_text(text), m_color(color), m_kind(kind), m_align(align)
    {
        QFont f = font(); f.setPixelSize(12); setFont(f);
        if (fixedWidth) setFixedWidth(fixedWidth); else setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        setFixedHeight(18); setToolTip(text);
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

// Tasks as a graph or as a list by layer. Painted, so hovering a task lights
// its chain without moving anything. Graph runs top to bottom (the inspector);
// Wide runs left to right with layer columns (the team workspace). While
// planning, a node's handle drags onto another node to add a dependency, and a
// selected edge is removed with Delete or its ×.
class TaskView : public QWidget {
public:
    enum Mode { List, Graph, Wide };
    struct Node { int id = 0, layer = 0; QString title, role, kind, statusText, deps, agent; QColor tone, roleTone; bool problem = false, draft = false, error = false, match = true; int attempt = 0; };
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
    QMap<int, QList<int>> edges;   // task -> its dependencies
    QSet<QPair<int, int>> soft;    // (dependency, task) edges that need only In progress
    QSet<int> cycle, emphasis;     // emphasis: Show what blocks
    std::function<QSet<int>(int)> chain;
    double zoom = 1.0;
    // Wide: the visible canvas; the graph centers in it and its layer bands
    // fill its height.
    QSize area;
    // Wide: the graph's own size at zoom 1.
    QSizeF natural() const
    {
        QMap<int, int> column;
        for (const auto &n : nodes) ++column[n.layer];
        int layers = 0, tall = 0;
        for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); tall = qMax(tall, it.value()); }
        return {12.0 + layers * WideStep, 30.0 + tall * (WideH + 12) + 8};
    }
    void layout()
    {
        m_rects.clear();
        QMap<int, int> column;
        if (m_mode == Graph) {
            for (const auto &n : nodes) {
                const int x = Gutter + column[n.layer]++ * (NodeW + 6), y = 8 + n.layer * (NodeH + 26);
                m_rects.insert(n.id, QRectF(x, y, NodeW, NodeH));
            }
            int layers = 0, wide = 0;
            for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); wide = qMax(wide, it.value()); }
            setFixedSize(int((Gutter + wide * (NodeW + 6) + 4) * zoom), int((8 + layers * (NodeH + 26)) * zoom));
        } else if (m_mode == Wide) {
            const QSizeF size = natural();
            const double viewW = area.isValid() ? area.width() / zoom : 0, viewH = area.isValid() ? area.height() / zoom : 0;
            m_ox = qMax(0.0, (viewW - size.width()) / 2);
            const double oy = qMax(0.0, (viewH - size.height()) / 2);
            for (const auto &n : nodes) {
                const double x = m_ox + 12 + n.layer * WideStep, y = oy + 30 + column[n.layer]++ * (WideH + 12);
                m_rects.insert(n.id, QRectF(x, y, WideW, WideH));
            }
            int layers = 0;
            for (auto it = column.cbegin(); it != column.cend(); ++it) layers = qMax(layers, it.key() + 1);
            m_layers = layers;
            setFixedSize(int(qMax(size.width(), viewW) * zoom), int(qMax(size.height(), viewH) * zoom));
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
            QFont small = font(); small.setPixelSize(10); p.setFont(small); p.setPen(m_c.dim);
            if (m_mode == Wide) {
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
                if (!m_rects.contains(dep) || !m_rects.contains(it.key())) continue;
                const bool hot = lit.contains(dep) && lit.contains(it.key());
                const bool bad = cycle.contains(dep) && cycle.contains(it.key());
                const bool selected = m_edge == qMakePair(dep, it.key());
                QColor color = selected || bad ? m_c.danger : hot ? m_c.accent : m_c.border;
                if (!hot && !selected && (!lit.isEmpty() || !emphasis.isEmpty()) && !(emphasis.contains(dep) && emphasis.contains(it.key()))) color = alpha(color, .35);
                p.setPen(QPen(color, hot || selected ? 2 : 1.4, selected || soft.contains({dep, it.key()}) ? Qt::DashLine : Qt::SolidLine)); p.setBrush(Qt::NoBrush);
                const auto path = edgePath(dep, it.key());
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
                QFont mono = TeamUi::mono(10); p.setFont(mono); p.setPen(m_c.dim);
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
                    if (!n.agent.isEmpty()) workspaceIcon(n.agent, m_c.text3).paint(&p, QRect(int(pill.right() + 5), int(pill.top()), 14, 14));
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
            return;
        }
        int layer = -1, count = 0;
        QMap<int, int> perLayer;
        for (const auto &n : nodes) if (n.match) ++perLayer[n.layer];
        for (const auto &n : nodes) {
            if (!n.match) continue;
            const QRectF r = m_rects[n.id];
            if (n.layer != layer) {
                layer = n.layer; count = perLayer[layer];
                QFont caps = font(); caps.setPixelSize(10); caps.setWeight(QFont::DemiBold); caps.setLetterSpacing(QFont::AbsoluteSpacing, .6);
                p.setOpacity(1); p.setFont(caps); p.setPen(m_c.dim);
                const QString label = QCoreApplication::translate("TeamPanel", "LAYER %1 · %2").arg(layer + 1).arg(count);
                p.drawText(QRectF(2, r.top() - 22, width(), 18), Qt::AlignLeft | Qt::AlignVCenter, label);
                const int lw = QFontMetrics(caps).horizontalAdvance(label) + 10;
                p.setPen(m_c.border); p.drawLine(QPointF(lw, r.top() - 13), QPointF(width() - 2, r.top() - 13));
            }
            p.setOpacity(dimmed(n) ? .35 : 1);
            const QRectF box(1, r.top(), width() - 2, r.height());
            if (n.problem || n.error) {
                p.setPen(QPen(n.tone == m_c.warn ? m_c.warn : m_c.danger, 1)); p.setBrush(alpha(n.tone == m_c.warn ? m_c.warn : m_c.danger, .07));
                p.drawRoundedRect(box.adjusted(.5, .5, -.5, -.5), 7, 7);
            } else if (m_hover == n.id) { p.setPen(Qt::NoPen); p.setBrush(alpha(m_c.fg, .05)); p.drawRoundedRect(box, 7, 7); }
            glyph(p, QRectF(8, r.top() + 5, 12, 12), n.tone, n.kind);
            QFont mono = TeamUi::mono(11); p.setFont(mono); p.setPen(m_c.dim);
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
                p.setFont(TeamUi::mono(10)); p.setPen(m_c.dim);
                p.drawText(QRectF(width() - 112, r.top() + 19, 104, 16), Qt::AlignRight | Qt::AlignVCenter, n.deps);
            }
        }
    }
    int at(const QPointF &position) const
    {
        const QPointF point = graph() ? position / zoom : position;
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it) {
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
        if (!editable || !graph() || e->button() != Qt::LeftButton) return QWidget::mousePressEvent(e);
        const QPointF point = e->position() / zoom;
        if (m_edge.first && QLineF(edgePath(m_edge.first, m_edge.second).pointAtPercent(.5), point).length() <= 8) {
            const auto edge = m_edge; m_edge = {0, 0}; update();
            if (unlinked) unlinked(edge.first, edge.second);
            return;
        }
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it)
            if (handle(it.value()).adjusted(-4, -4, 4, 4).contains(point)) { m_dragFrom = it.key(); m_dragAt = e->position(); m_edge = {0, 0}; update(); return; }
        if (!at(e->position())) { m_edge = edgeAt(e->position()); update(); }
    }
    void mouseMoveEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom) { m_dragAt = e->position(); m_hover = id; update(); return; }
        if (id != m_hover) { m_hover = id; setCursor(id ? Qt::PointingHandCursor : Qt::ArrowCursor); update(); }
    }
    void leaveEvent(QEvent *) override { if (!m_dragFrom) { m_hover = 0; update(); } }
    void mouseReleaseEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (m_dragFrom) {
            const int from = m_dragFrom; m_dragFrom = 0; update();
            if (id && id != from && linked) linked(from, id);
            return;
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
    double m_ox = 0;
    QPointF m_dragAt;
    QPair<int, int> m_edge{0, 0};
    QMap<int, QRectF> m_rects;
};

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

inline QLabel *elided(const QString &text, const QColor &color, int pixels = 12)
{
    auto *l = label(text, color, pixels);
    l->setToolTip(text);
    l->setMinimumWidth(20);
    l->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    return l;
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

inline QString clock(double at) { return QDateTime::fromSecsSinceEpoch(qint64(at)).toString("HH:mm"); }

// 12m, 3h 12m, 3d 4h, 2w 3d.
inline QString duration(double seconds)
{
    const qint64 s = qMax<qint64>(0, qint64(seconds));
    const qint64 minutes = s / 60, hours = s / 3600, days = s / 86400;
    if (hours < 1) return QCoreApplication::translate("TeamPanel", "%1m").arg(qMax<qint64>(1, minutes));
    if (days < 1) return QCoreApplication::translate("TeamPanel", "%1h %2m").arg(hours).arg(minutes % 60);
    if (days < 7) return QCoreApplication::translate("TeamPanel", "%1d %2h").arg(days).arg(hours % 24);
    return QCoreApplication::translate("TeamPanel", "%1w %2d").arg(days / 7).arg(days % 7);
}

// The look of team surfaces: ghost buttons, accent primary ones,
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
