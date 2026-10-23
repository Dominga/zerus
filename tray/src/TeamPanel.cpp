#include "TeamPanel.h"
#include "NewSessionDense.h"
#include "TeamController.h"
#include "WorkspaceIcons.h"
#include "WorkspaceSkin.h"
#include <QClipboard>
#include <QComboBox>
#include <QCursor>
#include <QGuiApplication>
#include <QInputDialog>
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QTimer>
#include <QToolButton>
#include <QUuid>
#include <QVBoxLayout>
#include <cmath>
#include <tuple>

using namespace Teams;

namespace {

struct Colors {
    QColor fg, text2, text3, muted, dim, accent, accentMuted, warn, danger, folder, surface, border, card, tile;
    bool dark = true;
};

Colors colorsFor(bool dark)
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

QColor toneColor(const Colors &c, Tone tone)
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
QColor roleColor(const QString &role, bool dark)
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

QColor alpha(QColor color, double a) { color.setAlphaF(a); return color; }

// A 12px status mark: icon and text always come together.
void glyph(QPainter &p, const QRectF &r, const QColor &color, const QString &kind)
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

QString kindOfTask(const Task &task, const Status &status)
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

// Tasks as a graph (layers top to bottom) or as a list by layer. Painted, so
// hovering a task lights its chain without moving anything.
class TaskView : public QWidget {
public:
    struct Node { int id, layer; QString title, role, kind, statusText, deps; QColor tone, roleTone; bool problem, draft, error, match; int attempt; };
    TaskView(const Colors &colors, bool graph, QWidget *parent = nullptr) : QWidget(parent), m_c(colors), m_graph(graph)
    {
        setMouseTracking(true);
        QFont f = font(); f.setPixelSize(12); setFont(f);
    }
    std::function<void(int task, const QPoint &global)> clicked;
    QList<Node> nodes;
    QMap<int, QList<int>> edges;   // task -> its dependencies
    QSet<QPair<int, int>> soft;    // (dependency, task) edges that need only In progress
    QSet<int> cycle, emphasis;     // emphasis: Show what blocks
    std::function<QSet<int>(int)> chain;
    double zoom = 1.0;
    void layout()
    {
        m_rects.clear();
        QMap<int, int> column;
        if (m_graph) {
            for (const auto &n : nodes) {
                const int x = Gutter + column[n.layer]++ * (NodeW + 6), y = 8 + n.layer * (NodeH + 26);
                m_rects.insert(n.id, QRectF(x, y, NodeW, NodeH));
            }
            int layers = 0, wide = 0;
            for (auto it = column.cbegin(); it != column.cend(); ++it) { layers = qMax(layers, it.key() + 1); wide = qMax(wide, it.value()); }
            setFixedSize(int((Gutter + wide * (NodeW + 6)) * zoom), int((8 + layers * (NodeH + 26)) * zoom));
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
protected:
    static constexpr int NodeW = 118, NodeH = 46, Gutter = 20;
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        if (m_graph) p.fillRect(rect(), m_c.surface);
        const QSet<int> lit = m_hover ? chain(m_hover) : QSet<int>();
        const auto dimmed = [&](const Node &n) {
            if (!lit.isEmpty()) return !lit.contains(n.id);
            if (!emphasis.isEmpty()) return !emphasis.contains(n.id);
            return m_graph && !n.match;
        };
        if (m_graph) {
            p.scale(zoom, zoom);
            QFont small = font(); small.setPixelSize(10); p.setFont(small); p.setPen(m_c.dim);
            QSet<int> labelled;
            for (const auto &n : nodes) if (!labelled.contains(n.layer)) {
                labelled.insert(n.layer); p.drawText(QRectF(0, m_rects[n.id].top(), Gutter, NodeH), Qt::AlignCenter, QStringLiteral("L%1").arg(n.layer + 1));
            }
            for (auto it = edges.cbegin(); it != edges.cend(); ++it) for (int dep : it.value()) {
                if (!m_rects.contains(dep) || !m_rects.contains(it.key())) continue;
                const QRectF a = m_rects[dep], b = m_rects[it.key()];
                const bool hot = lit.contains(dep) && lit.contains(it.key());
                const bool bad = cycle.contains(dep) && cycle.contains(it.key());
                QColor color = bad ? m_c.danger : hot ? m_c.accent : m_c.border;
                if (!hot && (!lit.isEmpty() || !emphasis.isEmpty()) && !(emphasis.contains(dep) && emphasis.contains(it.key()))) color = alpha(color, .35);
                p.setPen(QPen(color, hot ? 2 : 1.4, soft.contains({dep, it.key()}) ? Qt::DashLine : Qt::SolidLine)); p.setBrush(Qt::NoBrush);
                QPainterPath path;
                if (b.top() > a.bottom()) {
                    const QPointF s(a.center().x(), a.bottom()), e(b.center().x(), b.top());
                    path.moveTo(s); path.cubicTo(s + QPointF(0, 14), e - QPointF(0, 14), e);
                    p.drawPath(path);
                    p.drawLine(e, e + QPointF(-3.5, -5)); p.drawLine(e, e + QPointF(3.5, -5));
                } else {   // a back edge of a cycle: a loop on the right
                    const QPointF s(a.right(), a.center().y()), e(b.right(), b.center().y());
                    path.moveTo(s); path.cubicTo(s + QPointF(28, 0), e + QPointF(28, 0), e); p.drawPath(path);
                }
            }
            for (const auto &n : nodes) {
                const QRectF r = m_rects[n.id];
                p.setOpacity(dimmed(n) ? .35 : 1);
                p.setPen(n.error || n.problem ? QPen(n.error || n.tone == m_c.danger ? m_c.danger : m_c.warn, 1)
                                              : QPen(m_c.border, 1, n.draft ? Qt::DashLine : Qt::SolidLine));
                p.setBrush(n.error ? alpha(m_c.danger, .1) : m_c.card);
                p.drawRoundedRect(r.adjusted(.5, .5, -.5, -.5), 8, 8);
                p.setPen(Qt::NoPen); p.setBrush(n.tone); p.drawRoundedRect(QRectF(r.left() + 1, r.top() + 6, 3, r.height() - 12), 1.5, 1.5);
                QFont mono = NewSessionDense::mono(10); p.setFont(mono); p.setPen(m_c.dim);
                const QString id = QStringLiteral("#%1").arg(n.id);
                p.drawText(QRectF(r.left() + 9, r.top() + 5, 30, 16), Qt::AlignLeft | Qt::AlignVCenter, id);
                const int idw = QFontMetrics(mono).horizontalAdvance(id) + 4;
                QFont title = font(); title.setPixelSize(12); p.setFont(title); p.setPen(m_c.fg);
                p.drawText(QRectF(r.left() + 9 + idw, r.top() + 5, r.width() - 14 - idw, 16), Qt::AlignLeft | Qt::AlignVCenter,
                           QFontMetrics(title).elidedText(n.title, Qt::ElideRight, int(r.width() - 14 - idw)));
                p.setPen(Qt::NoPen); p.setBrush(n.roleTone); p.drawEllipse(QRectF(r.left() + 10, r.top() + 29, 7, 7));
                QFont role = font(); role.setPixelSize(11); p.setFont(role); p.setPen(m_c.text3);
                const QString who = n.role + (n.attempt > 1 ? QStringLiteral(" ·%1").arg(n.attempt) : QString());
                p.drawText(QRectF(r.left() + 21, r.top() + 24, r.width() - 42, 16), Qt::AlignLeft | Qt::AlignVCenter,
                           QFontMetrics(role).elidedText(who, Qt::ElideRight, int(r.width() - 42)));
                glyph(p, QRectF(r.right() - 17, r.top() + 26, 12, 12), n.tone, n.kind);
                if (n.error) { p.setPen(m_c.danger); p.drawText(QRectF(r.right() - 14, r.top() + 2, 12, 12), Qt::AlignCenter, QStringLiteral("!")); }
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
                const QString label = TeamPanel::tr("LAYER %1 · %2").arg(layer + 1).arg(count);
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
    }
    int at(const QPointF &position) const
    {
        const QPointF point = m_graph ? position / zoom : position;
        for (auto it = m_rects.cbegin(); it != m_rects.cend(); ++it) {
            QRectF r = it.value(); if (!m_graph) r.setWidth(width());
            if (r.contains(point)) return it.key();
        }
        return 0;
    }
    void mouseMoveEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (id != m_hover) { m_hover = id; setCursor(id ? Qt::PointingHandCursor : Qt::ArrowCursor); update(); }
    }
    void leaveEvent(QEvent *) override { m_hover = 0; update(); }
    void mouseReleaseEvent(QMouseEvent *e) override
    {
        const int id = at(e->position());
        if (id && e->button() == Qt::LeftButton && clicked) clicked(id, e->globalPosition().toPoint());
    }
private:
    Colors m_c;
    bool m_graph;
    int m_hover = 0;
    QMap<int, QRectF> m_rects;
};

QLabel *label(const QString &text, const QColor &color, int pixels = 12, bool bold = false, const char *name = nullptr)
{
    auto *l = new QLabel(text);
    QFont f = l->font(); f.setPixelSize(pixels); if (bold) f.setWeight(QFont::DemiBold); l->setFont(f);
    QPalette pal = l->palette(); pal.setColor(QPalette::WindowText, color); l->setPalette(pal);
    l->setTextFormat(Qt::PlainText);
    if (name) l->setObjectName(name);
    return l;
}

QLabel *elided(const QString &text, const QColor &color, int pixels = 12)
{
    auto *l = label(text, color, pixels);
    l->setToolTip(text);
    l->setMinimumWidth(20);
    l->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    return l;
}

QPushButton *button(const QString &text, const char *name, bool primary = false)
{
    auto *b = new QPushButton(text);
    b->setObjectName(name); b->setAutoDefault(false); b->setCursor(Qt::PointingHandCursor);
    b->setProperty("primary", primary);
    return b;
}

QFrame *card(const Colors &c, const QColor &border = {})
{
    auto *f = new QFrame;
    f->setObjectName("teamCard");
    f->setStyleSheet(QStringLiteral("QFrame#teamCard{background:%1;border:1px solid %2;border-radius:9px;}")
                         .arg(c.surface.name(QColor::HexArgb), (border.isValid() ? border : c.border).name(QColor::HexArgb)));
    return f;
}

QHBoxLayout *row(int spacing = 6)
{
    auto *l = new QHBoxLayout; l->setContentsMargins(0, 0, 0, 0); l->setSpacing(spacing); return l;
}

QString agentIcon(const QString &agent)
{
    return QStringList{"claude", "codex", "kimi", "opencode", "dsh"}.contains(agent) ? agent : QStringLiteral("terminal");
}

QString clock(double at) { return QDateTime::fromSecsSinceEpoch(qint64(at)).toString("HH:mm"); }

QString duration(double seconds)
{
    const qint64 s = qMax<qint64>(0, qint64(seconds));
    return s >= 3600 ? TeamPanel::tr("%1h %2m").arg(s / 3600).arg((s % 3600) / 60) : TeamPanel::tr("%1m").arg(qMax<qint64>(1, s / 60));
}

}  // namespace

TeamPanel::TeamPanel(TeamController *controller, QWidget *parent) : QWidget(parent), m_controller(controller)
{
    setObjectName("teamPanel");
    auto *layout = new QVBoxLayout(this);
    layout->setContentsMargins(0, 0, 0, 0);
    m_scroll = new QScrollArea;
    m_scroll->setObjectName("teamPanelScroll");
    m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame);
    m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    layout->addWidget(m_scroll);
    QObject::connect(m_controller, &TeamController::teamsChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_controller, &TeamController::watchedChanged, this, [this] { scheduleRender(); });
    QObject::connect(m_controller, &TeamController::feedChanged, this, [this] { scheduleRender(); });
    render();
}

void TeamPanel::setTheme(bool dark) { m_dark = dark; scheduleRender(); }

void TeamPanel::showTeam(const QString &team)
{
    if (m_team == team) return;
    m_team = team; m_showBlockers = false; m_feedMember.clear(); m_notice.clear();
    m_controller->watch(team);
    render();
}

void TeamPanel::scheduleRender()
{
    if (m_pending) return;
    m_pending = true;
    QTimer::singleShot(0, this, [this] { m_pending = false; render(); });
}

void TeamPanel::resizeEvent(QResizeEvent *event)
{
    QWidget::resizeEvent(event);
    // The graph needs 400px; below that the list by layer replaces it.
    static const int threshold = 400;
    if (m_content && (m_content->property("narrow").toBool() != (width() < threshold))) scheduleRender();
}

bool TeamPanel::confirm(const QString &name, const QString &title, const QString &text, const QString &action)
{
    if (findChild<QMessageBox *>(name)) return false;
    QMessageBox dialog(QMessageBox::Warning, title, text, QMessageBox::NoButton, this);
    dialog.setObjectName(name); dialog.setTextFormat(Qt::PlainText);
    auto *ok = dialog.addButton(action, QMessageBox::DestructiveRole);
    auto *cancel = dialog.addButton(QMessageBox::Cancel);
    dialog.setDefaultButton(cancel); dialog.setEscapeButton(cancel);
    dialog.exec();
    return dialog.clickedButton() == ok;
}

void TeamPanel::act(const QString &operation, const QJsonObject &input)
{
    QJsonObject body = input;
    body.insert("team", m_team);
    m_notice.clear();
    m_controller->act(operation, body, [this, guard = QPointer<TeamPanel>(this)](bool ok, const QString &message) {
        if (!guard) return;
        m_notice = ok ? QString() : message;
        scheduleRender();
    });
}

void TeamPanel::remind(const Team &team, const Member &member, int taskId)
{
    const auto *task = team.task(taskId);
    const auto *attempt = task ? task->last() : nullptr;
    if (!task || !attempt || !sendMessage) return;
    const QJsonObject report{{"request_id", QUuid::createUuid().toString(QUuid::WithoutBraces)}, {"team", team.id},
                             {"task", task->id}, {"attempt", attempt->id}, {"text", "..."}};
    const QString text = tr("[Zerus team · from: You · task: #%1 · attempt: %2]\nYour turn ended without a report for task #%1 %3. "
                            "Report once with JSON on stdin:\n  hgs %4team report --json\n  %5")
                             .arg(task->id).arg(attempt->n).arg(task->title, member.host.isEmpty() ? QString() : QStringLiteral("@HOST "),
                                  QString::fromUtf8(QJsonDocument(report).toJson(QJsonDocument::Compact)));
    sendMessage(member.host, member.session, text);
}

void TeamPanel::showEvent(QShowEvent *event)
{
    QWidget::showEvent(event);
    if (m_dirty) render();
}

void TeamPanel::render()
{
    // A hidden tab renders once it is shown; polls keep coming meanwhile.
    if (!isVisible() && m_content) { m_dirty = true; return; }
    m_dirty = false;
    const Colors c = colorsFor(m_dark);
    const int scroll = m_scroll->verticalScrollBar()->value();
    const bool narrow = width() < 400;
    auto *content = new QWidget;
    content->setObjectName("teamPanelContent");
    content->setProperty("narrow", narrow);
    auto *layout = new QVBoxLayout(content);
    layout->setContentsMargins(narrow ? 8 : 12, narrow ? 8 : 12, narrow ? 8 : 12, 12);
    layout->setSpacing(12);
    const Team *team = m_controller->watched();
    if (!team && !m_team.isEmpty()) team = m_controller->team(m_team);
    if (!team) {
        auto *empty = label(m_team.isEmpty() ? tr("No team for this session") : tr("Loading team…"), c.text2, 13, true, "teamEmptyTitle");
        layout->addWidget(empty);
        if (m_team.isEmpty()) {
            auto *hint = label(tr("Teams appear here when the selected session leads or works in one, or when you select a team "
                                  "in the session list. A lead session creates one with hgs team create (hgs guide team)."), c.muted, 12);
            hint->setWordWrap(true); hint->setObjectName("teamEmptyHint");
            layout->addWidget(hint);
        }
        if (answersChanged) answersChanged(0);
    } else {
        const auto live = m_live ? m_live : LiveLookup([](const Member &) { return Live{}; });
        const QString status = displayStatus(*team, live);
        const auto problemList = problems(*team, live);
        layout->addWidget(header(*team, status, narrow));
        if (!m_notice.isEmpty()) {
            auto *notice = label(m_notice, c.danger, 12, false, "teamNotice");
            notice->setWordWrap(true);
            layout->addWidget(notice);
        }
        if (team->status == "planning") layout->addWidget(planning(*team));
        if (status == "stuck") layout->addWidget(stuck(*team, problemList));
        const int waiting = Teams::answers(*team, live);
        if (waiting) layout->addWidget(answers(*team));
        layout->addWidget(members(*team, narrow));
        layout->addWidget(tasks(*team, narrow));
        if (team->full) layout->addWidget(feed(*team));
        if (answersChanged) answersChanged(waiting);
    }
    layout->addStretch();
    m_scroll->setWidget(content);
    m_content = content;
    m_scroll->verticalScrollBar()->setValue(scroll);
}

QWidget *TeamPanel::header(const Team &team, const QString &status, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c);
    box->setObjectName("teamCard");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(7);
    auto *top = row(8);
    auto *icon = new QLabel; icon->setPixmap(workspaceIcon("team", c.fg).pixmap(16, 16)); top->addWidget(icon);
    auto *name = elided(team.name, c.fg, 14); QFont f = name->font(); f.setWeight(QFont::DemiBold); name->setFont(f);
    name->setObjectName("teamName");
    top->addWidget(name, 1);
    auto *badge = new Pill(statusLabel(status), toneColor(c, statusTone(status)), status, false);
    badge->setObjectName("teamStatusBadge");
    top->addWidget(badge);
    auto *more = new QToolButton; more->setObjectName("teamMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    menu->addAction(tr("Copy team ID"), this, [id = team.id] { QGuiApplication::clipboard()->setText(id); });
    if (QStringList{"running", "paused", "planning"}.contains(team.status))
        menu->addAction(tr("Stop…"), this, [this, name = team.name] {
            if (confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                 "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                act("stop", {});
        });
    more->setMenu(menu);
    top->addWidget(more);
    v->addLayout(top);
    QList<QColor> segments;
    for (auto tone : progress(team)) segments.append(toneColor(c, tone));
    v->addWidget(new Progress(segments, 6, c.border));
    QMap<Tone, int> counts;
    for (const auto &task : team.tasks) ++counts[taskStatus(task).tone];
    QStringList legend;
    legend << tr("%1/%2 accepted").arg(accepted(team)).arg(team.tasks.size());
    if (counts.value(Tone::Active) + counts.value(Tone::Assigned)) legend << tr("Active %1").arg(counts.value(Tone::Active) + counts.value(Tone::Assigned));
    if (counts.value(Tone::Danger) + counts.value(Tone::Warn)) legend << tr("Problems %1").arg(counts.value(Tone::Danger) + counts.value(Tone::Warn));
    int pending = 0; for (const auto &task : team.tasks) pending += task.status == "pending";
    if (pending) legend << tr("Pending %1").arg(pending);
    auto *legendLabel = label(legend.join(QStringLiteral("  ·  ")), c.text3, 11, false, "teamLegend");
    legendLabel->setWordWrap(true);
    v->addWidget(legendLabel);
    QStringList meta;
    if (team.created > 0) meta << tr("%1 elapsed").arg(duration(QDateTime::currentSecsSinceEpoch() - team.created));
    if (!team.project.isEmpty()) meta << team.project;
    else if (!team.folder.isEmpty()) meta << team.folder;
    if (!meta.isEmpty()) v->addWidget(elided(meta.join(QStringLiteral(" · ")), c.muted, 11));
    if (const auto next = nextLine(team); !next.isEmpty()) {
        auto *nextLabel = elided(next, c.text2, 12); nextLabel->setObjectName("teamNext");
        v->addWidget(nextLabel);
    }
    auto *actions = row(6);
    if (team.status == "running") {
        auto *pause = button(tr("Pause"), "teamPause");
        pause->setToolTip(tr("Running turns finish; nothing new is handed out"));
        QObject::connect(pause, &QPushButton::clicked, this, [this] { act("pause", {}); });
        actions->addWidget(pause);
    } else if (team.status == "paused") {
        auto *resume = button(tr("Resume"), "teamResume", true);
        QObject::connect(resume, &QPushButton::clicked, this, [this] { act("resume", {}); });
        actions->addWidget(resume);
    }
    if (team.status == "running" || team.status == "paused") {
        auto *stop = button(tr("Stop…"), "teamStop");
        QObject::connect(stop, &QPushButton::clicked, this, [this, name = team.name] {
            if (confirm("teamStopConfirm", tr("Stop team?"), tr("Stop %1? Every open task is cancelled and no task is handed out again. "
                                                                 "Member sessions keep running; their current turns finish.").arg(name), tr("Stop team")))
                act("stop", {});
        });
        actions->addWidget(stop);
    }
    if (team.status == "done" || team.status == "stopped") {
        auto *archive = button(tr("Archive"), "teamArchive");
        QObject::connect(archive, &QPushButton::clicked, this, [this, name = team.name] {
            if (confirm("teamArchiveConfirm", tr("Archive team?"), tr("Archive %1? Its history and feed are kept; it leaves the session list.").arg(name), tr("Archive")))
                act("archive", {});
        });
        actions->addWidget(archive);
    }
    actions->addStretch();
    if (actions->count() > 1) v->addLayout(actions); else delete actions;
    if (team.status == "paused") {
        auto *paused = label(tr("Paused — running turns finish, nothing new is handed out"), c.muted, 11);
        paused->setWordWrap(true);
        v->addWidget(paused);
    }
    Q_UNUSED(narrow);
    return box;
}

QWidget *TeamPanel::planning(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c, alpha(team.errors.isEmpty() ? c.accent : c.danger, .6));
    box->setObjectName("teamPlanning");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(6);
    v->addWidget(label(tr("Plan from Lead — review before running"), c.fg, 12, true));
    if (!team.errors.isEmpty()) {
        v->addWidget(label(tr("%n plan error(s)", nullptr, int(team.errors.size())), c.danger, 12, true, "teamPlanErrors"));
        for (const auto &error : team.errors) { auto *l = label(QStringLiteral("• ") + error, c.danger, 12); l->setWordWrap(true); v->addWidget(l); }
    }
    for (const auto &warning : team.warnings) { auto *l = label(QStringLiteral("• ") + warning, c.warn, 12); l->setWordWrap(true); v->addWidget(l); }
    auto *actions = row(6);
    auto *approve = button(tr("Approve && Run"), "teamApprove", true);
    approve->setEnabled(team.errors.isEmpty() && !team.tasks.isEmpty());
    approve->setToolTip(team.errors.isEmpty() ? tr("Start the members and hand out the first tasks")
                                              : tr("Fix %n plan error(s) to run", nullptr, int(team.errors.size())));
    QObject::connect(approve, &QPushButton::clicked, this, [this] { act("approve", {}); });
    auto *discard = button(tr("Discard plan…"), "teamDiscard");
    QObject::connect(discard, &QPushButton::clicked, this, [this, name = team.name] {
        if (confirm("teamDiscardConfirm", tr("Discard plan?"), tr("Discard the plan of %1? The team stops before any member starts.").arg(name), tr("Discard plan")))
            act("discard", {});
    });
    actions->addWidget(approve); actions->addWidget(discard); actions->addStretch();
    v->addLayout(actions);
    v->addWidget(label(tr("The lead edits the plan; editing it here comes in a later update."), c.muted, 11));
    return box;
}

QWidget *TeamPanel::stuck(const Team &team, const QList<Problem> &problems)
{
    const Colors c = colorsFor(m_dark);
    auto *box = card(c, c.warn);
    box->setObjectName("teamStuck");
    box->setStyleSheet(box->styleSheet() + QStringLiteral("QFrame#teamStuck{background:%1;}").arg(alpha(c.warn, .1).name(QColor::HexArgb)));
    auto *v = new QVBoxLayout(box); v->setContentsMargins(10, 10, 10, 10); v->setSpacing(5);
    auto *title = row(6);
    auto *mark = new StatusMark(tr("Team stuck — nothing can start"), c.fg, "stuck");
    title->addWidget(mark, 1); v->addLayout(title);
    int open = 0; for (const auto &task : team.tasks) open += !task.finished() && task.status != "draft";
    v->addWidget(label(problems.isEmpty() ? tr("%n open task(s) wait; none can start.", nullptr, open)
                                          : tr("%1 open tasks wait on %2 problems:").arg(open).arg(problems.size()), c.text2, 12));
    for (const auto &problem : problems) {
        auto *link = button(QStringLiteral("• ") + problem.text, "teamStuckProblem");
        link->setFlat(true); link->setStyleSheet(QStringLiteral("text-align:left;padding:0;color:%1;").arg(c.accent.name()));
        QObject::connect(link, &QPushButton::clicked, this, [this, task = problem.task, member = problem.member, t = team] {
            if (task) taskMenu(t, task, QCursor::pos());
            else if (const auto *m = t.member(member); m && openSession) openSession(m->host, m->session);
        });
        v->addWidget(link);
    }
    auto *blockers = button(m_showBlockers ? tr("Hide blockers") : tr("Show what blocks"), "teamShowBlockers");
    QObject::connect(blockers, &QPushButton::clicked, this, [this] { m_showBlockers = !m_showBlockers; scheduleRender(); });
    auto *r = row(); r->addWidget(blockers); r->addStretch(); v->addLayout(r);
    return box;
}

QWidget *TeamPanel::answers(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamAnswers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    int n = 0;
    auto *items = new QVBoxLayout; items->setSpacing(6);
    for (const auto &member : team.members) {
        const auto state = m_live ? m_live(member) : Live{};
        if (member.session.isEmpty() || !state.attention) continue;
        ++n;
        auto *line = card(c);
        auto *h = new QHBoxLayout(line); h->setContentsMargins(8, 5, 6, 5); h->setSpacing(6);
        h->addWidget(new StatusMark(QString(), c.warn, "answer", 16));
        if (width() >= 400) h->addWidget(new Pill(team.roleOf(member.id), roleColor(team.roleOf(member.id), m_dark)));
        h->addWidget(elided(state.attentionText.isEmpty() ? tr("Question in %1").arg(member.session) : state.attentionText, c.fg, 12), 1);
        auto *go = button(tr("Go to question"), "teamGoToQuestion");
        QObject::connect(go, &QPushButton::clicked, this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
        h->addWidget(go);
        items->addWidget(line);
    }
    auto *title = row(6);
    title->addWidget(label(tr("Needs your answer"), c.fg, 12, true));
    title->addWidget(new Pill(QString::number(n), c.warn));
    title->addStretch();
    v->addLayout(title); v->addLayout(items);
    return box;
}

QWidget *TeamPanel::members(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamMembers");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Members"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.members.size()), c.muted, 12));
    title->addStretch();
    v->addLayout(title);
    for (const auto &member : team.members) v->addWidget(memberRow(team, member, narrow));
    return box;
}

QWidget *TeamPanel::memberRow(const Team &team, const Member &member, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    const auto state = m_live ? m_live(member) : Live{};
    const auto status = memberStatus(team, member, state);
    const QColor tone = toneColor(c, status.tone);
    auto *box = card(c, status.problem ? alpha(tone, .8) : QColor());
    box->setObjectName("teamCard");
    box->setProperty("teamMember", member.id);
    box->setAccessibleName(tr("%1, %2").arg(team.roleOf(member.id), status.text));
    if (member.session.isEmpty() && team.status == "planning")
        box->setStyleSheet(QStringLiteral("QFrame#teamCard{background:transparent;border:1px dashed %1;border-radius:9px;}").arg(c.border.name()));
    auto *outer = new QVBoxLayout(box); outer->setContentsMargins(8, 7, 6, 7); outer->setSpacing(4);
    auto *h = row(8);
    auto *tile = new QLabel; tile->setFixedSize(28, 28); tile->setAlignment(Qt::AlignCenter);
    tile->setStyleSheet(QStringLiteral("background:%1;border-radius:7px;").arg(c.tile.name()));
    tile->setPixmap(workspaceIcon(agentIcon(member.agent), c.fg).pixmap(16, 16));
    tile->setToolTip(member.session.isEmpty() ? member.agent : member.agent + QStringLiteral(" · ") + member.session);
    h->addWidget(tile, 0, Qt::AlignTop);
    auto *body = new QVBoxLayout; body->setSpacing(3);
    auto *line1 = row(6);
    const QString role = team.roleOf(member.id);
    line1->addWidget(new Pill(member.lead && member.role != role ? role + QStringLiteral(" · ") + member.role : role, roleColor(role, m_dark), member.lead ? "lead" : QString()));
    if (state.attention) line1->addWidget(new Pill(QStringLiteral("?1"), c.warn));
    line1->addWidget(elided(member.model.isEmpty() ? member.agent : member.model, c.text3, 12), 1);
    if (!narrow) line1->addWidget(new StatusMark(status.text, tone, status.kind, 112, Qt::AlignRight));
    body->addLayout(line1);
    if (narrow) body->addWidget(new StatusMark(status.text, tone, status.kind));
    auto *line2 = row(8);
    const Task *current = team.task(status.task);
    QString doing = current ? QStringLiteral("#%1 %2").arg(current->id).arg(current->title) : tr("No task");
    if (current && current->last() && current->last()->n > 1) doing += tr(" · att %1").arg(current->last()->n);
    auto *doingLabel = elided(doing, current ? c.text2 : c.muted, 11);
    line2->addWidget(doingLabel, 1);
    line2->addWidget(label(member.computer.isEmpty() ? tr("this computer") : member.computer, c.muted, 11));
    if (!narrow) {
        // Task chips: this member's tasks, colored by status.
        int shown = 0;
        for (const auto &task : team.tasks) {
            if (task.assignee != member.id || shown >= 4) continue;
            ++shown;
            auto *chip = new QPushButton(QStringLiteral("#%1").arg(task.id));
            chip->setObjectName("teamTaskChip"); chip->setFlat(true); chip->setCursor(Qt::PointingHandCursor);
            const QColor chipColor = toneColor(c, taskStatus(task).tone);
            chip->setStyleSheet(QStringLiteral("QPushButton{font:10px;color:%1;border:1px solid %2;border-radius:4px;padding:0 4px;min-height:16px;}")
                                    .arg(chipColor.name(), alpha(chipColor, task.id == status.task ? 1 : .4).name(QColor::HexArgb)));
            chip->setToolTip(QStringLiteral("#%1 %2 · %3").arg(task.id).arg(task.title, taskStatus(task).text));
            QObject::connect(chip, &QPushButton::clicked, this, [this, t = team, id = task.id] { taskMenu(t, id, QCursor::pos()); });
            line2->addWidget(chip);
        }
    }
    body->addLayout(line2);
    h->addLayout(body, 1);
    auto *more = new QToolButton; more->setObjectName("teamMemberMore"); more->setText(QStringLiteral("⋯")); more->setAutoRaise(true);
    more->setPopupMode(QToolButton::InstantPopup); more->setAccessibleName(tr("Member actions"));
    more->setStyleSheet(QStringLiteral("QToolButton::menu-indicator{image:none;width:0;}"));
    auto *menu = new QMenu(more);
    auto *open = menu->addAction(tr("Open session"), this, [this, host = member.host, session = member.session] { if (openSession) openSession(host, session); });
    open->setEnabled(!member.session.isEmpty());
    if (!member.lead) {
        if (member.state == "paused")
            menu->addAction(tr("Resume member"), this, [this, id = member.id] { act("resume-member", {{"member", id}}); });
        else if (team.status != "planning")
            menu->addAction(tr("Pause member"), this, [this, id = member.id] { act("pause-member", {{"member", id}}); });
        auto *replace = menu->addAction(tr("Replace agent / model…"));
        replace->setEnabled(false); replace->setToolTip(tr("Comes with the plan editor"));
        menu->addAction(tr("Remove from team…"), this, [this, id = member.id, role] {
            if (confirm("teamRemoveMemberConfirm", tr("Remove member?"), tr("Remove %1 from the team? Its session keeps running. "
                                                                            "Reassign or cancel its open tasks first.").arg(role), tr("Remove")))
                act("remove-member", {{"member", id}});
        });
    }
    more->setMenu(menu);
    h->addWidget(more, 0, Qt::AlignTop);
    outer->addLayout(h);
    if (status.problem) outer->addWidget(problemActions(team, member, status));
    return box;
}

QWidget *TeamPanel::problemActions(const Team &team, const Member &member, const Status &status)
{
    auto *box = new QWidget; box->setObjectName("teamProblemActions");
    auto *h = new QHBoxLayout(box); h->setContentsMargins(36, 0, 0, 0); h->setSpacing(6);
    const Task *task = team.task(status.task);
    const auto *attempt = task ? task->last() : nullptr;
    const auto add = [&](const QString &text, const char *name, bool primary, const std::function<void()> &run) {
        auto *b = button(text, name, primary); QObject::connect(b, &QPushButton::clicked, this, run); h->addWidget(b); return b;
    };
    const QString host = member.host, session = member.session;
    if (status.kind == "unconfirmed" && task && attempt) {
        add(tr("Check session"), "teamCheckSession", true, [this, host, session] { if (openSession) openSession(host, session); });
        auto *arrived = add(tr("It arrived"), "teamCheckDelivered", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "delivered"}}); });
        arrived->setToolTip(tr("The session shows the task: mark it delivered"));
        add(tr("Send again"), "teamSendAgain", false, [this, id = task->id, n = attempt->n, role = team.roleOf(member.id)] {
            if (confirm("teamSendAgainConfirm", tr("Send the task again?"),
                        tr("Attempt %1 may have reached %2. Send the task again as a new attempt? The old attempt is revoked; "
                           "a late report for it is refused.").arg(n).arg(role), tr("Send again")))
                act("retry", {{"task", id}});
        });
        auto *cancel = add(tr("Cancel attempt"), "teamCancelAttempt", false, [this, id = task->id] { act("check", {{"task", id}, {"outcome", "not_delivered"}}); });
        cancel->setToolTip(tr("Nothing arrived: the task returns to Pending"));
    } else if (status.kind == "no_report" && task && attempt) {
        add(tr("Remind"), "teamRemind", true, [this, t = team, member, id = task->id] { remind(t, member, id); });
        add(tr("Use last reply as report"), "teamUseLastReply", false, [this, id = task->id, attemptId = attempt->id] {
            act("use-last-reply", {{"task", id}, {"attempt", attemptId}});
        });
        add(tr("Reassign…"), "teamReassign", false, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
    } else if (status.kind == "offline") {
        if (task) add(tr("Reassign…"), "teamReassign", true, [this, t = team, id = task->id] { reassignMenu(t, id, QCursor::pos()); });
        auto *wait = add(tr("Wait"), "teamWait", false, [] {});
        wait->setToolTip(tr("Open attempts are not revoked; the member continues when its computer answers"));
    } else if (status.kind == "answer") {
        add(tr("Go to question"), "teamGoToQuestion", true, [this, host, session] { if (openSession) openSession(host, session); });
    } else if (status.kind == "failed") {
        auto *again = add(tr("Start again"), "teamRelaunch", true, [this, id = member.id] { act("relaunch", {{"member", id}}); });
        again->setToolTip(member.error);
        for (const auto &t : team.tasks)
            if (t.assignee == member.id && !t.finished()) {
                add(tr("Reassign…"), "teamReassign", false, [this, tm = team, id = t.id] { reassignMenu(tm, id, QCursor::pos()); });
                break;
            }
    }
    h->addStretch();
    return box;
}

void TeamPanel::reassignMenu(const Team &team, int taskId, const QPoint &global)
{
    const auto *task = team.task(taskId);
    if (!task) return;
    QMenu menu(this);
    menu.setObjectName("teamReassignMenu");
    for (const auto &member : team.members) {
        if (member.id == task->assignee) continue;
        const auto status = memberStatus(team, member, m_live ? m_live(member) : Live{});
        auto *action = menu.addAction(QStringLiteral("%1 · %2 — %3").arg(team.roleOf(member.id), member.model.isEmpty() ? member.agent : member.model, status.text));
        QObject::connect(action, &QAction::triggered, this, [this, taskId, id = member.id] { act("reassign", {{"task", taskId}, {"member", id}}); });
    }
    if (menu.isEmpty()) menu.addAction(tr("No other member"))->setEnabled(false);
    menu.exec(global);
}

void TeamPanel::taskMenu(const Team &team, int taskId, const QPoint &global)
{
    const auto *task = team.task(taskId);
    if (!task) return;
    QMenu menu(this);
    menu.setObjectName("teamTaskMenu");
    auto *title = menu.addAction(QStringLiteral("#%1 %2 · %3").arg(task->id).arg(task->title, taskStatus(*task).text));
    title->setEnabled(false);
    menu.addSeparator();
    const auto *assignee = team.member(task->assignee);
    if (assignee && !assignee->session.isEmpty())
        menu.addAction(tr("Open %1's session").arg(team.roleOf(assignee->id)), this, [this, host = assignee->host, session = assignee->session] {
            if (openSession) openSession(host, session);
        });
    const bool open = !task->finished() && task->status != "draft";
    if (task->status == "submitted") {
        menu.addAction(tr("Accept"), this, [this, taskId] { act("accept", {{"task", taskId}}); });
        menu.addAction(tr("Reject with comment…"), this, [this, taskId] {
            bool ok = false;
            const auto reason = QInputDialog::getMultiLineText(this, tr("Reject with comment"), tr("Why is the report not accepted? The task goes back for another attempt."), {}, &ok);
            if (ok && !reason.trimmed().isEmpty()) act("reject", {{"task", taskId}, {"reason", reason.trimmed()}});
        });
    }
    if (open) {
        menu.addAction(tr("Reassign…"), this, [this, t = team, taskId] { reassignMenu(t, taskId, QCursor::pos()); });
        menu.addAction(tr("Retry"), this, [this, taskId, n = task->last() ? task->last()->n : 0] {
            if (n == 0 || confirm("teamRetryConfirm", tr("Retry task?"), tr("Revoke attempt %1 and hand the task out again?").arg(n), tr("Retry")))
                act("retry", {{"task", taskId}});
        });
        menu.addAction(tr("Take over"), this, [this, taskId] {
            if (confirm("teamTakeOverConfirm", tr("Take over task?"), tr("You do this task yourself; its current attempt is revoked and no member gets it."), tr("Take over")))
                act("take-over", {{"task", taskId}});
        });
        menu.addAction(tr("Cancel task…"), this, [this, taskId] {
            if (confirm("teamCancelTaskConfirm", tr("Cancel task?"), tr("Cancel this task? Tasks that depend on it hard never become ready."), tr("Cancel task")))
                act("cancel", {{"task", taskId}});
        });
    } else if (task->finished()) {
        auto *none = menu.addAction(tr("Decided — no actions"));
        none->setEnabled(false);
    }
    menu.exec(global);
}

QWidget *TeamPanel::tasks(const Team &team, bool narrow)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamTasks");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(6);
    auto *title = row(6);
    title->addWidget(label(tr("Tasks"), c.fg, 12, true));
    title->addWidget(label(QString::number(team.tasks.size()), c.muted, 12));
    title->addStretch();
    for (const auto &[key, text] : QList<std::pair<QString, QString>>{{"all", tr("All")}, {"active", tr("Active")}, {"problems", tr("Problems")}}) {
        auto *b = button(text, "teamTaskFilter");
        b->setCheckable(true); b->setChecked(m_filter == key); b->setProperty("filter", key);
        QObject::connect(b, &QPushButton::clicked, this, [this, key = key] { m_filter = key; scheduleRender(); });
        title->addWidget(b);
    }
    v->addLayout(title);
    // Problem tasks: their own state, or a problem of the member working on them.
    QSet<int> problemTasks;
    QMap<int, Tone> problemTone;
    for (const auto &p : problems(team, m_live ? m_live : LiveLookup([](const Member &) { return Live{}; })))
        if (p.task) { problemTasks.insert(p.task); if (problemTone.value(p.task, Tone::Warn) != Tone::Danger) problemTone[p.task] = p.tone; }
    for (const auto &task : team.tasks) if (const auto status = taskStatus(task); status.problem) { problemTasks.insert(task.id); problemTone[task.id] = status.tone; }
    auto *view = new TaskView(c, !narrow);
    view->setObjectName(narrow ? "teamTaskList" : "teamTaskGraph");
    view->zoom = m_zoom;
    view->chain = [team](int id) { return Teams::chain(team, id); };
    view->cycle = team.cycle;
    QList<int> order;
    const auto byLayer = layered(team);
    for (auto it = byLayer.cbegin(); it != byLayer.cend(); ++it) order += it.value();
    for (int id : order) {
        const auto *task = team.task(id);
        const auto status = taskStatus(*task);
        const QString role = task->assignee.isEmpty() ? tr("You") : team.roleOf(task->assignee);
        TaskView::Node node;
        node.id = id; node.layer = team.layers.value(id); node.title = task->title; node.role = role;
        node.problem = problemTasks.contains(id);
        node.tone = toneColor(c, node.problem ? problemTone.value(id, Tone::Danger) : status.tone);
        node.kind = kindOfTask(*task, status);
        node.statusText = status.text; node.roleTone = roleColor(role, m_dark);
        node.draft = task->status == "draft"; node.error = team.cycle.contains(id) || (!task->assignee.isEmpty() && !team.member(task->assignee));
        node.attempt = task->last() ? task->last()->n : 0;
        QStringList deps;
        for (const auto &dep : task->deps) {
            deps << QStringLiteral("#%1").arg(dep.task); view->edges[id].append(dep.task);
            if (dep.soft && !team.allHard) view->soft.insert({dep.task, id});
        }
        node.deps = deps.isEmpty() ? QString() : QStringLiteral("← ") + deps.join(' ');
        const bool active = QStringList{"assigned", "in_progress", "submitted"}.contains(task->status);
        node.match = m_filter == "all" || (m_filter == "active" && active) || (m_filter == "problems" && node.problem);
        view->nodes.append(node);
    }
    if (m_showBlockers) for (int id : problemTasks) view->emphasis += Teams::chain(team, id);
    view->clicked = [this, team](int id, const QPoint &global) { taskMenu(team, id, global); };
    view->layout();
    if (narrow) v->addWidget(view);
    else {
        auto *tools = row(4);
        const auto zoomTo = [this](double zoom) { m_zoom = qBound(0.5, std::round(zoom * 10) / 10, 1.5); scheduleRender(); };
        auto *out = button(QStringLiteral("−"), "teamZoomOut"); QObject::connect(out, &QPushButton::clicked, this, [=] { zoomTo(m_zoom - .1); });
        auto *level = label(QStringLiteral("%1%").arg(int(std::round(m_zoom * 100))), c.text3, 11, false, "teamZoomLevel");
        auto *in = button(QStringLiteral("+"), "teamZoomIn"); QObject::connect(in, &QPushButton::clicked, this, [=] { zoomTo(m_zoom + .1); });
        auto *fit = button(tr("Fit"), "teamZoomFit");
        tools->addWidget(out); tools->addWidget(level); tools->addWidget(in); tools->addWidget(fit); tools->addStretch();
        tools->addWidget(label(tr("Hover = chain · click = actions"), c.muted, 11));
        v->addLayout(tools);
        auto *area = new QScrollArea; area->setObjectName("teamGraphArea");
        area->setWidget(view); area->setWidgetResizable(false); area->setFrameShape(QFrame::NoFrame);
        area->setMaximumHeight(430);
        area->setMinimumHeight(qMin(430, view->height() + 4));
        area->setStyleSheet(QStringLiteral("QScrollArea#teamGraphArea{background:%1;border:1px solid %2;border-radius:9px;}")
                                .arg(c.surface.name(), c.border.name()));
        area->viewport()->setAutoFillBackground(true);
        QPalette surface = area->viewport()->palette(); surface.setColor(QPalette::Window, c.surface); area->viewport()->setPalette(surface);
        QObject::connect(fit, &QPushButton::clicked, this, [=] {
            const double natural = view->width() / m_zoom;
            zoomTo(natural > 0 ? qMin(1.5, (area->viewport()->width() - 4) / natural) : 1.0);
        });
        v->addWidget(area);
        auto *legend = label(tr("L = layer: tasks that can run at the same time"), c.muted, 10);
        v->addWidget(legend);
    }
    return box;
}

QWidget *TeamPanel::feed(const Team &team)
{
    const Colors c = colorsFor(m_dark);
    auto *box = new QWidget; box->setObjectName("teamFeed");
    auto *v = new QVBoxLayout(box); v->setContentsMargins(0, 0, 0, 0); v->setSpacing(2);
    auto *title = row(6);
    title->addWidget(label(tr("Feed"), c.fg, 12, true));
    title->addStretch();
    auto *filter = new QComboBox; filter->setObjectName("teamFeedFilter");
    filter->addItem(tr("All members"), QString());
    for (const auto &member : team.members) filter->addItem(team.roleOf(member.id), team.roleOf(member.id));
    filter->setCurrentIndex(qMax(0, filter->findData(m_feedMember)));
    QObject::connect(filter, &QComboBox::currentIndexChanged, this, [this, filter] { m_feedMember = filter->currentData().toString(); scheduleRender(); });
    title->addWidget(filter);
    v->addLayout(title);
    int shown = 0;
    for (const auto &event : m_controller->feed()) {
        const auto line = feedLine(team, event);
        if (!m_feedMember.isEmpty() && line.by != m_feedMember) continue;
        ++shown;
        auto *r = new QWidget; r->setObjectName("teamFeedRow");
        auto *h = new QHBoxLayout(r); h->setContentsMargins(0, 3, 0, 3); h->setSpacing(6);
        auto *time = label(clock(line.at), c.dim, 11); time->setFont(NewSessionDense::mono(11)); time->setFixedWidth(38);
        time->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(line.at)).toString(Qt::ISODate));
        h->addWidget(time, 0, Qt::AlignTop);
        if (!line.by.isEmpty()) h->addWidget(new Pill(line.by, line.by == tr("You") ? c.text3 : roleColor(line.by, m_dark)), 0, Qt::AlignTop);
        auto *text = label(line.text, line.tone == Tone::Neutral ? c.text2 : toneColor(c, line.tone), 12);
        text->setWordWrap(true);
        h->addWidget(text, 1);
        v->addWidget(r);
    }
    if (!shown) v->addWidget(label(tr("Nothing yet"), c.muted, 12));
    if (m_controller->feedHasOlder()) {
        auto *older = button(tr("Load older"), "teamFeedOlder");
        QObject::connect(older, &QPushButton::clicked, this, [this] { m_controller->loadOlder(); });
        auto *r = row(); r->addWidget(older); r->addStretch(); v->addLayout(r);
    } else if (shown) v->addWidget(label(tr("Beginning of team history"), c.muted, 11));
    return box;
}
