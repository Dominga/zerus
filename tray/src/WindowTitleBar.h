#pragma once
#include <QApplication>
#include <QHBoxLayout>
#include <QLabel>
#include <QPainter>
#include <QPushButton>
#include <QWidget>
#include <functional>

// The title bar a skin may draw in place of the system one
// ("window":{"main":"frameless"}). It carries the Zerus mark, the Go to search
// and the window buttons. On Windows the window keeps its native frame
// behavior through FramelessWindow below: edges resize, the bar drags and
// snaps, a double click maximizes, and DWM keeps the shadow. Other systems
// keep their own title bar.
class WindowTitleBar : public QWidget {
public:
    enum Glyph { Minimize, Maximize, Restore, Close };
    explicit WindowTitleBar(QWidget *parent = nullptr) : QWidget(parent)
    {
        setObjectName("windowTitleBar");
        setFixedHeight(40);
        auto *row = new QHBoxLayout(this);
        row->setContentsMargins(8, 0, 6, 1);
        row->setSpacing(10);
        // The window may lend the bar its own controls: one in place of the
        // mark, a group after the name and a group before the window buttons.
        m_markSlot = new QHBoxLayout; m_markSlot->setContentsMargins(0, 0, 0, 0); m_markSlot->setSpacing(0);
        m_mark = new QLabel; m_mark->setObjectName("windowTitleMark"); m_mark->setFixedSize(18, 18);
        m_markSlot->addWidget(m_mark); row->addLayout(m_markSlot);
        m_name = new QLabel(QStringLiteral("Zerus")); m_name->setObjectName("windowTitleName");
        row->addWidget(m_name);
        m_leading = new QHBoxLayout; m_leading->setContentsMargins(4, 0, 0, 0); m_leading->setSpacing(2); row->addLayout(m_leading);
        row->addStretch(1);
        m_search = new QPushButton; m_search->setObjectName("windowTitleSearch");
        m_search->setAccessibleName(tr("Search sessions, IDs and messages"));
        m_search->setCursor(Qt::PointingHandCursor); m_search->setFixedHeight(28);
        m_search->setMinimumWidth(180); m_search->setMaximumWidth(460);
        m_search->setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
        auto *searchRow = new QHBoxLayout(m_search); searchRow->setContentsMargins(10, 0, 6, 0); searchRow->setSpacing(8);
        m_searchIcon = new QLabel; m_searchIcon->setFixedSize(14, 14); m_searchIcon->setAttribute(Qt::WA_TransparentForMouseEvents);
        m_searchText = new QLabel(tr("Search sessions, IDs and messages…")); m_searchText->setObjectName("windowTitleSearchText");
        m_searchText->setAttribute(Qt::WA_TransparentForMouseEvents);
        m_shortcut = new QLabel; m_shortcut->setObjectName("windowTitleShortcut"); m_shortcut->setAttribute(Qt::WA_TransparentForMouseEvents);
        searchRow->addWidget(m_searchIcon); searchRow->addWidget(m_searchText, 1); searchRow->addWidget(m_shortcut);
        row->addWidget(m_search, 3);
        row->addStretch(1);
        m_trailing = new QHBoxLayout; m_trailing->setContentsMargins(0, 0, 6, 0); m_trailing->setSpacing(2); row->addLayout(m_trailing);
        connect(m_search, &QPushButton::clicked, this, [this] { if (search) search(); });
        const auto button = [this, row](Glyph glyph, const QString &caption, const char *name) {
            auto *b = new GlyphButton(glyph, this); b->setObjectName(QLatin1String(name));
            b->setToolTip(caption); b->setAccessibleName(caption); row->addWidget(b); m_buttons.append(b); return b;
        };
        auto *minimize = button(Minimize, tr("Minimize"), "windowMinimize");
        m_maximize = button(Maximize, tr("Maximize"), "windowMaximize");
        auto *close = button(Close, tr("Close window"), "windowClose");
        connect(minimize, &QPushButton::clicked, this, [this] { window()->showMinimized(); });
        connect(m_maximize, &QPushButton::clicked, this, [this] { window()->isMaximized() ? window()->showNormal() : window()->showMaximized(); });
        connect(close, &QPushButton::clicked, this, [this] { window()->close(); });
    }
    std::function<void()> search;

    void setShortcut(const QString &text) { m_shortcut->setText(text); m_shortcut->setVisible(!text.isEmpty()); }
    void setMark(const QPixmap &pixmap) { m_mark->setPixmap(pixmap); }
    void setMarkVisible(bool visible) { m_mark->setVisible(visible); }
    QHBoxLayout *markSlot() const { return m_markSlot; }
    QHBoxLayout *leading() const { return m_leading; }
    QHBoxLayout *trailing() const { return m_trailing; }
    // Colors come from the workspace theme, so the bar follows dark and light.
    void setColors(const QColor &background, const QColor &border, const QColor &field, const QColor &fieldBorder,
                   const QColor &text, const QColor &muted, const QColor &chip)
    {
        m_background = background; m_border = border;
        m_name->setStyleSheet(QStringLiteral("color:%1;font-size:13px;font-weight:600;").arg(text.name()));
        m_search->setStyleSheet(QStringLiteral("QPushButton#windowTitleSearch{background:%1;border:1px solid %2;border-radius:8px;padding:0}"
                                               "QPushButton#windowTitleSearch:hover{border-color:%3}")
                                    .arg(field.name(), fieldBorder.name(), muted.name()));
        m_searchText->setStyleSheet(QStringLiteral("color:%1;font-size:12px;background:transparent;").arg(muted.name()));
        m_shortcut->setStyleSheet(QStringLiteral("color:%1;background:%2;border-radius:4px;padding:1px 6px;font-size:10px;font-family:Consolas,monospace;")
                                      .arg(muted.name(), chip.name()));
        QPixmap lens(28, 28); lens.setDevicePixelRatio(2); lens.fill(Qt::transparent);
        {
            QPainter p(&lens); p.setRenderHint(QPainter::Antialiasing); p.setPen(QPen(muted, 1.6, Qt::SolidLine, Qt::RoundCap));
            p.drawEllipse(QRectF(1.5, 1.5, 8.5, 8.5)); p.drawLine(QPointF(9, 9), QPointF(12.5, 12.5));
        }
        m_searchIcon->setPixmap(lens);
        for (auto *b : m_buttons) { b->color = muted; b->hover = chip; b->update(); }
        update();
    }
    void setMaximized(bool maximized)
    {
        m_maximize->glyph = maximized ? Restore : Maximize;
        m_maximize->setToolTip(maximized ? tr("Restore") : tr("Maximize")); m_maximize->setAccessibleName(m_maximize->toolTip());
        m_maximize->update();
    }
    // Empty bar, the mark and the name drag the window; buttons and the search do not.
    bool dragsAt(const QPoint &local) const
    {
        if (!rect().contains(local)) return false;
        const auto *child = childAt(local);
        return !child || child == m_mark || child == m_name;
    }

protected:
    void paintEvent(QPaintEvent *) override
    {
        QPainter p(this);
        p.fillRect(rect(), m_background);
        p.fillRect(QRect(0, height() - 1, width(), 1), m_border);
    }

private:
    class GlyphButton : public QPushButton {
    public:
        GlyphButton(Glyph g, QWidget *parent) : QPushButton(parent), glyph(g) { setFixedSize(38, 30); setFocusPolicy(Qt::NoFocus); setFlat(true); }
        Glyph glyph;
        QColor color = QColor("#9aa3ad"), hover = QColor("#1f242c");
    protected:
        void paintEvent(QPaintEvent *) override
        {
            QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
            const bool over = underMouse() || isDown();
            if (over) { p.setPen(Qt::NoPen); p.setBrush(glyph == Close ? QColor("#c42b1c") : hover); p.drawRoundedRect(rect().adjusted(2, 2, -2, -2), 6, 6); }
            p.setPen(QPen(over && glyph == Close ? QColor(Qt::white) : color, 1.2)); p.setBrush(Qt::NoBrush);
            const QPointF c = QRectF(rect()).center();
            switch (glyph) {
            case Minimize: p.drawLine(c + QPointF(-5, 0), c + QPointF(5, 0)); break;
            case Maximize: p.drawRect(QRectF(c.x() - 4.5, c.y() - 4.5, 9, 9)); break;
            case Restore:
                p.drawRect(QRectF(c.x() - 4.5, c.y() - 2.5, 7, 7));
                p.drawPolyline(QPolygonF{c + QPointF(-2.5, -2.5), c + QPointF(-2.5, -4.5), c + QPointF(4.5, -4.5), c + QPointF(4.5, 2.5), c + QPointF(2.5, 2.5)});
                break;
            case Close: p.drawLine(c + QPointF(-4.5, -4.5), c + QPointF(4.5, 4.5)); p.drawLine(c + QPointF(4.5, -4.5), c + QPointF(-4.5, 4.5)); break;
            }
        }
    };
    QLabel *m_mark, *m_name, *m_searchIcon, *m_searchText, *m_shortcut;
    QPushButton *m_search;
    QHBoxLayout *m_markSlot, *m_leading, *m_trailing;
    GlyphButton *m_maximize;
    QList<GlyphButton *> m_buttons;
    QColor m_background = QColor("#12161b"), m_border = QColor("#1f242c");
};

// Windows: a frameless top-level window with the native frame behavior kept
// (WindowTitleBar.cpp). Elsewhere both do nothing.
namespace FramelessWindow {
void apply(QWidget *window);
// Handles the frame's messages for nativeEvent(); false leaves a message to Qt.
bool nativeEvent(QWidget *window, const WindowTitleBar *bar, void *message, qintptr *result);
}
