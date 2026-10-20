#pragma once
#include "SessionCardDelegate.h"
#include "SessionList.h"
#include <QApplication>
#include <QCursor>
#include <QEasingCurve>
#include <QElapsedTimer>
#include <QPainter>
#include <QScrollBar>
#include <QSplitter>
#include <QTimer>
#include <QVariantAnimation>
#include <functional>

// The session column can collapse into a strip that keeps every row. The panel
// floats over a placeholder in the splitter: growing it over the conversation
// (hover, search) never resizes the terminal, and docking or collapsing
// resizes the conversation once instead of on every animation frame.
class SessionPanelDock : public QObject {
public:
    enum Mode { Docked, Collapsed, Peek };
    // Below these expansions the panel shows strip controls and only the active filter.
    static constexpr qreal NarrowChrome = .45, NarrowFilters = .75;
    std::function<void()> layoutChanged, modeChanged;
    std::function<bool()> keepPeek;

    SessionPanelDock(QSplitter *splitter, QWidget *slot, QWidget *panel, SessionList *list, QObject *owner)
        : QObject(owner), m_splitter(splitter), m_slot(slot), m_panel(panel), m_list(list), m_host(panel->parentWidget()),
          m_dockMinimum(qMax(panel->minimumWidth(), slot->minimumWidth())), m_stripWidth(SessionStrip::width(list->property("compact").toBool()))
    {
        m_shadow = new Shadow(m_host); m_shadow->hide();
        m_panel->setMinimumWidth(0); m_list->setMinimumWidth(0);
        m_slot->setMinimumWidth(m_dockMinimum);
        // The strip draws its own thin scroll handle, only under the pointer.
        m_bar = new QScrollBar(Qt::Vertical, m_panel); m_bar->setObjectName("sessionStripScroll"); m_bar->hide();
        auto *source = m_list->verticalScrollBar();
        connect(source, &QScrollBar::rangeChanged, m_bar, [this, source](int low, int high) {
            m_bar->setRange(low, high); m_bar->setPageStep(source->pageStep()); updateBar();
        });
        connect(source, &QScrollBar::valueChanged, m_bar, [this](int value) { const QSignalBlocker block(m_bar); m_bar->setValue(value); });
        connect(m_bar, &QScrollBar::valueChanged, source, &QScrollBar::setValue);
        m_animation.setDuration(220); m_animation.setEasingCurve(QEasingCurve::OutCubic);
        connect(&m_animation, &QVariantAnimation::valueChanged, this, [this](const QVariant &value) { setExpansion(value.toReal()); });
        connect(&m_animation, &QVariantAnimation::finished, this, &SessionPanelDock::settle);
        m_enter.setSingleShot(true); m_enter.setInterval(150);
        connect(&m_enter, &QTimer::timeout, this, [this] {
            if (m_mode != Collapsed || !m_hoverExpands || !pointerInside()) return;
            setMode(Peek); m_peekByHover = true; m_outside.invalidate(); m_leave.start();
        });
        // A hover-opened panel follows the pointer itself: leave events are not
        // delivered when the pointer jumps out of the window or onto a popup.
        m_leave.setInterval(100);
        connect(&m_leave, &QTimer::timeout, this, [this] {
            if (m_mode != Peek || !m_peekByHover) { m_leave.stop(); return; }
            // Menus, drags and an active search keep the panel open.
            if (pointerInside() || QApplication::activePopupWidget() || m_list->dragging() || (keepPeek && keepPeek())) { m_outside.invalidate(); return; }
            if (!m_outside.isValid()) m_outside.start();
            else if (m_outside.elapsed() >= 350) setMode(Collapsed);
        });
        m_slot->installEventFilter(this); m_host->installEventFilter(this); m_panel->installEventFilter(this);
        m_panel->raise();
    }

    Mode mode() const { return m_mode; }
    bool chromeNarrow() const { return m_chromeNarrow; }
    bool filtersNarrow() const { return m_filtersNarrow; }
    int dockedWidth() const { return m_mode == Docked && !animating() ? m_slot->width() : m_dockedWidth; }
    void setHoverExpands(bool enabled) { m_hoverExpands = enabled; if (!enabled) m_enter.stop(); }
    void setEdgeColor(const QColor &color) { m_shadow->edge = color; m_shadow->update(); }
    // Follows the row height, so strip tiles stay square.
    void setStripWidth(int width) {
        if (width == m_stripWidth) return;
        m_stripWidth = width;
        if (m_mode != Docked) fixSlot(true);
        place();
    }
    // Starts in the saved state without animating into it.
    void restore(bool collapsed, int dockedWidth) {
        if (collapsed) setMode(Collapsed, false); else place();
        // The panel has not been laid out yet; keep the saved width and the usual inset.
        if (dockedWidth > 0) m_dockedWidth = dockedWidth;
        m_rowInset = 17;
    }
    // Docked <-> collapsed; a panel shown over the conversation gets pinned.
    void toggle() { setMode(m_mode == Docked ? Collapsed : Docked); }
    void peek() { if (m_mode == Collapsed) setMode(Peek); }
    // Escape returns a panel shown over the conversation to the strip.
    bool dismissPeek() { if (m_mode != Peek) return false; setMode(Collapsed); return true; }

    void setMode(Mode mode, bool animate = true) {
        if (mode == m_mode && !animating()) return;
        if (m_mode == Docked && !animating()) { m_dockedWidth = m_slot->width(); m_rowInset = m_panel->width() - m_list->viewport()->width(); }
        const Mode from = m_mode;
        m_mode = mode; m_peekByHover = false; m_enter.stop(); m_leave.stop();
        // While the panel is over the conversation, a click elsewhere returns it to the strip.
        if (mode == Peek) qApp->installEventFilter(this); else qApp->removeEventFilter(this);
        m_decorated = mode == Peek || (from == Peek && mode == Collapsed);
        // The conversation takes the space at once and the panel animates above it.
        if (mode != Docked) fixSlot(true);
        const qreal target = mode == Collapsed ? 0 : 1;
        m_animation.stop();
        if (!animate || qFuzzyCompare(m_expansion, target)) { setExpansion(target); settle(); }
        else { m_animation.setStartValue(m_expansion); m_animation.setEndValue(target); m_animation.start(); }
        if (modeChanged) modeChanged();
    }

protected:
    bool eventFilter(QObject *watched, QEvent *event) override {
        const auto type = event->type();
        if (type == QEvent::MouseButtonPress && m_mode == Peek) {
            // The click still reaches its target; menus opened from the panel are other windows.
            const auto *widget = qobject_cast<QWidget *>(watched);
            if (widget && widget->window() == m_panel->window() && widget != m_panel && !m_panel->isAncestorOf(widget)) setMode(Collapsed);
        }
        if ((watched == m_slot && (type == QEvent::Resize || type == QEvent::Move)) || (watched == m_host && type == QEvent::Resize)) place();
        if (watched == m_panel && type == QEvent::Enter) {
            if (m_mode == Collapsed && m_hoverExpands) m_enter.start();
            updateBar();
        } else if (watched == m_panel && type == QEvent::Leave) {
            m_enter.stop(); updateBar();
        }
        return false;
    }

private:
    class Shadow : public QWidget {
    public:
        explicit Shadow(QWidget *parent) : QWidget(parent) { setAttribute(Qt::WA_TransparentForMouseEvents); }
        qreal alpha = 0;
        QColor edge;
    protected:
        void paintEvent(QPaintEvent *) override {
            QPainter p(this); QLinearGradient shade(0, 0, width(), 0);
            shade.setColorAt(0, QColor(0, 0, 0, qRound(110 * alpha))); shade.setColorAt(1, Qt::transparent);
            p.fillRect(rect(), shade);
            QColor line(edge); line.setAlphaF(line.alphaF() * alpha); p.fillRect(QRect(0, 0, 1, height()), line);
        }
    };

    bool animating() const { return m_animation.state() == QAbstractAnimation::Running; }
    bool pointerInside() const { return m_panel->isVisible() && m_panel->rect().contains(m_panel->mapFromGlobal(QCursor::pos())); }
    int expandedWidth() const {
        const int limit = qMax(m_dockMinimum, m_host->width() - 160);
        return qBound(m_dockMinimum, m_dockedWidth, limit);
    }
    void fixSlot(bool strip) {
        // The splitter keeps its own sizes; it only moves the conversation when told.
        const auto sizes = m_splitter->sizes();
        const int total = sizes.size() == 2 ? sizes[0] + sizes[1] : 0;
        const int width = strip ? m_stripWidth : qMin(expandedWidth(), total);
        if (strip) m_slot->setFixedWidth(width);
        else { m_slot->setMinimumWidth(m_dockMinimum); m_slot->setMaximumWidth(QWIDGETSIZE_MAX); }
        if (total > 0) m_splitter->setSizes({width, total - width});
    }
    void setExpansion(qreal expansion) {
        const bool wasCards = m_expansion >= 1;
        m_expansion = expansion;
        m_list->setProperty("expandedRowWidth", expandedWidth() - m_rowInset);
        m_list->setProperty("expansion", expansion);
        // Row hints are as narrow as the strip only while it is not full width.
        if (wasCards != (expansion >= 1)) m_list->doItemsLayout();
        const bool chrome = expansion < NarrowChrome, filters = expansion < NarrowFilters;
        if (chrome != m_chromeNarrow || filters != m_filtersNarrow) {
            m_chromeNarrow = chrome; m_filtersNarrow = filters;
            m_list->setVerticalScrollBarPolicy(chrome ? Qt::ScrollBarAlwaysOff : Qt::ScrollBarAsNeeded);
            if (layoutChanged) layoutChanged();
        }
        place(); m_list->viewport()->update();
    }
    void settle() {
        if (m_mode == Docked) fixSlot(false);
        if (m_mode != Peek) m_decorated = false;
        place();
    }
    void place() {
        const QPoint at = m_slot->mapTo(m_host, QPoint());
        const int width = m_mode == Docked && !animating() ? m_slot->width()
            : qRound(m_stripWidth + (expandedWidth() - m_stripWidth) * m_expansion);
        m_panel->setGeometry(at.x(), at.y(), width, m_slot->height());
        // A skin may collapse the list to nothing (strip width 0).
        m_panel->setVisible(width > 0);
        const qreal alpha = m_decorated ? m_expansion : 0;
        // Only a shadow at the panel edge; the conversation is not dimmed.
        m_shadow->alpha = alpha; m_shadow->setGeometry(at.x() + width, at.y(), 18, m_slot->height());
        m_shadow->setVisible(alpha > 0);
        if (alpha > 0) { m_shadow->raise(); m_shadow->update(); }
        m_panel->raise();
        updateBar();
    }
    void updateBar() {
        const QPoint top = m_list->mapTo(m_panel, QPoint());
        m_bar->setGeometry(m_panel->width() - 7, top.y(), 7, m_list->height());
        m_bar->setVisible(m_mode == Collapsed && m_chromeNarrow && !animating() && m_panel->underMouse() && m_bar->maximum() > 0);
        if (m_bar->isVisible()) m_bar->raise();
    }

    QSplitter *m_splitter;
    QWidget *m_slot, *m_panel;
    SessionList *m_list;
    QWidget *m_host;
    int m_dockMinimum, m_stripWidth, m_dockedWidth = 310, m_rowInset = 17;
    Shadow *m_shadow;
    QScrollBar *m_bar;
    QVariantAnimation m_animation;
    QTimer m_enter, m_leave;
    QElapsedTimer m_outside;
    Mode m_mode = Docked;
    qreal m_expansion = 1;
    bool m_chromeNarrow = false, m_filtersNarrow = false, m_decorated = false, m_hoverExpands = false, m_peekByHover = false;
};
