#include "ComposerToolbar.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"

#include <QApplication>
#include <QFontMetricsF>
#include <QHBoxLayout>
#include <QKeyEvent>
#include <QPainter>
#include <QScreen>
#include <QStyleOption>
#include <QStylePainter>
#include <QTimer>
#include <QtMath>
#include <QVBoxLayout>
#include <algorithm>

namespace {
constexpr int ChipHeight = 24, ChipPadding = 9, IconSize = 14, IconGap = 5, Spacing = 6, NeverShortened = 100;

struct ToneColors { const char *text, *background, *border, *hover; };
ToneColors toneColors(ChipTone tone, bool dark)
{
    switch (tone) {
    case ChipTone::Quiet: return dark ? ToneColors{"#a1adbb", "transparent", "transparent", "#27313b"} : ToneColors{"#647386", "transparent", "transparent", "#e8edf3"};
    case ChipTone::Neutral: return dark ? ToneColors{"#e8edf4", "#222a33", "#3d4855", "#2a3440"} : ToneColors{"#1a2733", "#f1f4f7", "#dce2e8", "#e8edf3"};
    case ChipTone::Success: return dark ? ToneColors{"#72cdb2", "transparent", "transparent", "#27313b"} : ToneColors{"#237a62", "transparent", "transparent", "#e8edf3"};
    case ChipTone::Warning: return dark ? ToneColors{"#efbd78", "#2a2419", "#5a4a2e", "#33291b"} : ToneColors{"#91621a", "#fbf3e4", "#e6cf9f", "#f6e8cc"};
    case ChipTone::Danger: return dark ? ToneColors{"#ff9ca8", "#2a1e24", "#5a3540", "#33222a"} : ToneColors{"#b52d48", "#fdeef0", "#efc2ca", "#f9dde2"};
    case ChipTone::Accent: return dark ? ToneColors{"#8bdfc0", "#233a35", "#456e61", "#2a4640"} : ToneColors{"#167357", "#e7f3ed", "#a5c8b8", "#dcefe6"};
    }
    return {};
}
QString accent(bool dark) { return dark ? QStringLiteral("#8bdfc0") : QStringLiteral("#167357"); }
}

ChipPopover::ChipPopover(QWidget *parent) : QFrame(parent, Qt::Popup | Qt::FramelessWindowHint)
{
    // Translucent top-level widgets skip Qt's background fill: paintEvent draws it.
    setObjectName("chipPopover"); setAttribute(Qt::WA_TranslucentBackground); setFocusPolicy(Qt::StrongFocus);
}

void ChipPopover::setContent(QWidget *content)
{
    if (!layout()) { auto *box = new QVBoxLayout(this); box->setContentsMargins(0, 0, 0, 0); }
    m_content = content; layout()->addWidget(content); content->show();
}

void ChipPopover::setTheme(bool dark)
{
    setWorkspaceStyle(this, QString("QFrame#chipPopover { background:%1; color:%2; border:1px solid %3; border-radius:9px; }"
        " QFrame#chipPopover QLabel { color:%2; background:transparent; border:0; }")
        .arg(dark ? "#171d24" : "#f8fafb", dark ? "#c9d2dd" : "#1a2733", dark ? "#3d4855" : "#dce2e8"));
}

void ChipPopover::showFor(QWidget *anchor)
{
    m_anchor = anchor; m_returnFocus = QApplication::focusWidget();
    const QRect available = anchor->screen()->availableGeometry();
    // As wide as the content needs (recovery has five actions in a row), within the screen.
    const QSize content = m_content ? m_content->sizeHint().expandedTo(m_content->minimumSizeHint()).expandedTo(m_content->minimumSize()) : QSize();
    setFixedWidth(qMin(qMax(260, content.width()), qMin(560, available.width() - 24)));
    // Measure after show(): only a shown popup has its content's final style and fonts.
    show(); reposition(); setFocus(Qt::PopupFocusReason);
}

void ChipPopover::reposition()
{
    if (!m_anchor) return;
    // Not adjustSize(): it measures wrapped text at the size-hint width, but the fixed width
    // minus the 1 px frame is narrower, so the last wrapped line would be cut off.
    QLayout *box = layout();
    resize(width(), box && box->hasHeightForWidth() ? qMax(box->totalHeightForWidth(width()), minimumSizeHint().height()) : sizeHint().height());
    const QRect available = m_anchor->screen()->availableGeometry();
    const QPoint origin = m_anchor->mapToGlobal(QPoint(0, 0));
    const QWidget *window = m_anchor->window();
    const bool leftHalf = origin.x() + m_anchor->width() / 2 < window->mapToGlobal(window->rect().center()).x();
    int x = leftHalf ? origin.x() : origin.x() + m_anchor->width() - width();
    int y = origin.y() - height() - 6;
    x = qBound(available.left() + 8, x, available.right() - width() - 8);
    y = qBound(available.top() + 8, y, available.bottom() - height() - 8);
    move(x, y);
}

bool ChipPopover::event(QEvent *event)
{
    // A top-level window grows downward; keep the bottom edge above the chip instead.
    const bool handled = QFrame::event(event);
    if (event->type() == QEvent::LayoutRequest && isVisible()) reposition();
    return handled;
}

void ChipPopover::paintEvent(QPaintEvent *)
{
    QStyleOption option; option.initFrom(this);
    QPainter painter(this); style()->drawPrimitive(QStyle::PE_Widget, &option, &painter, this);
}

void ChipPopover::hideEvent(QHideEvent *event)
{
    QFrame::hideEvent(event);
    if (m_returnFocus && m_returnFocus->isVisible() && m_returnFocus->isEnabled()) m_returnFocus->setFocus(Qt::PopupFocusReason);
    m_returnFocus = nullptr;
}

ToolbarChip::ToolbarChip(QWidget *parent) : QPushButton(parent), m_flashTimer(new QTimer(this))
{
    setFixedHeight(ChipHeight); setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Fixed);
    setFocusPolicy(Qt::TabFocus); setCursor(Qt::PointingHandCursor); setAutoDefault(false);
    setIconSize(QSize(IconSize, IconSize));
    m_flashTimer->setSingleShot(true); m_flashTimer->setInterval(1000);
    connect(m_flashTimer, &QTimer::timeout, this, [this] { m_flashing = false; setProperty("flashing", false); render(); });
    connect(this, &QPushButton::clicked, this, [this] { if (m_popover && m_popover->content()) openPopover(); });
    render(); updateVisibility();
}

QString ToolbarChip::visibleText() const
{
    const int available = qMax(0, width() - 2 * ChipPadding - 2 - (m_icon.isEmpty() ? 0 : IconSize + IconGap));
    return fontMetrics().elidedText(labelText(m_compact), Qt::ElideRight, available);
}

QString ToolbarChip::labelText(bool compact) const
{
    if (!compact) return m_full;
    return m_short.isEmpty() && m_icon.isEmpty() ? m_full : m_short;
}

void ToolbarChip::setLabels(const QString &full, const QString &shortText)
{
    if (full == m_full && shortText == m_short) return;
    m_full = full; m_short = shortText;
    setText(labelText(m_compact)); setAccessibleName(full);
    if (m_detail.isEmpty()) setToolTip(full);
    updateGeometry(); update(); emit fitChanged();
}

void ToolbarChip::setDetail(const QString &detail)
{
    m_detail = detail; setToolTip(detail.isEmpty() ? m_full : detail); setAccessibleDescription(detail);
}

void ToolbarChip::setTone(ChipTone tone) { if (tone == m_tone) return; m_tone = tone; render(); }

void ToolbarChip::setIconName(const QString &name)
{
    if (name == m_icon) return;
    m_icon = name; render(); updateGeometry(); emit fitChanged();
}

void ToolbarChip::setTheme(bool dark) { m_dark = dark; render(); if (m_popover) m_popover->setTheme(dark); }

void ToolbarChip::setActive(bool active)
{
    if (active == m_active) return;
    m_active = active; updateVisibility(); emit fitChanged();
}

void ToolbarChip::setFitHidden(bool hidden) { if (hidden == m_fitHidden) return; m_fitHidden = hidden; updateVisibility(); }

void ToolbarChip::setCompact(bool compact)
{
    if (compact == m_compact) return;
    m_compact = compact; setText(labelText(compact)); updateGeometry(); update();
}

QSize ToolbarChip::labelSizeHint(bool compact) const
{
    ensurePolished();   // the 11 px stylesheet font
    const QString text = labelText(compact);
    // Round the real text width up: elidedText() compares the unrounded one.
    int width = 2 * ChipPadding + 2 + qCeil(QFontMetricsF(font()).horizontalAdvance(text));
    if (!m_icon.isEmpty()) width += IconSize + (text.isEmpty() ? 0 : IconGap);
    return {width, ChipHeight};
}

QSize ToolbarChip::minimumSizeHint() const { return {2 * ChipPadding + 2 + (m_icon.isEmpty() ? 12 : IconSize), ChipHeight}; }

void ToolbarChip::setPopoverContent(QWidget *content)
{
    if (!m_popover) { m_popover = new ChipPopover(this); m_popover->setTheme(m_dark); }
    m_popover->setContent(content);
}

void ToolbarChip::openPopover() { if (m_popover && m_popover->content() && isVisible()) m_popover->showFor(this); }

void ToolbarChip::closePopover() { if (m_popover && m_popover->isVisible()) m_popover->hide(); }

void ToolbarChip::flash() { m_flashing = true; setProperty("flashing", true); render(); m_flashTimer->start(); }

void ToolbarChip::keyPressEvent(QKeyEvent *event)
{
    if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) { click(); event->accept(); return; }
    QPushButton::keyPressEvent(event);
}

void ToolbarChip::paintEvent(QPaintEvent *)
{
    QStylePainter painter(this); QStyleOptionButton option; initStyleOption(&option);
    option.text = visibleText();
    painter.drawControl(QStyle::CE_PushButton, option);
}

void ToolbarChip::render()
{
    // Cache and recovery chips re-render every second: only restyle real changes.
    const auto colors = toneColors(m_tone, m_dark);
    const QString border = m_flashing ? accent(m_dark) : QString(colors.border);
    // Select the chip type alone: a QPushButton rule would also restyle the buttons in its popover.
    setWorkspaceStyle(this, QString("ToolbarChip { font-size:11px; min-height:0; padding:0 %6px; border-radius:%7px; text-align:left;"
        " color:%1; background:%2; border:1px solid %3; } ToolbarChip:hover { background:%4; }"
        " ToolbarChip:focus[keyboardFocus=\"true\"] { border-color:%5; }")
        .arg(colors.text, colors.background, border, colors.hover, accent(m_dark)).arg(ChipPadding).arg(m_tone == ChipTone::Accent ? 11 : 6));
    const QString iconKey = m_icon + '|' + colors.text;
    if (iconKey != m_renderedIcon) {
        m_renderedIcon = iconKey; setIcon(m_icon.isEmpty() ? QIcon() : workspaceIcon(m_icon, QColor(colors.text)));
    }
}

void ToolbarChip::updateVisibility()
{
    const bool visible = m_active && !m_fitHidden;
    if (visible) { setVisible(true); return; }
    QWidget *focus = QApplication::focusWidget();
    const bool heldFocus = focus && (focus == this || (m_popover && (focus == m_popover || m_popover->isAncestorOf(focus))));
    closePopover(); setVisible(false);
    if (heldFocus) emit focusReleased();
}

namespace {
// The row's zones: the left one from the left edge, the right one to the right
// edge and the centre one centred between them (grid 1fr auto 1fr). A side
// wider than its half pushes the centre over rather than overlapping it.
class ZoneRow : public QLayout {
public:
    ZoneRow(QWidget *parent, QHBoxLayout *left, QHBoxLayout *centre, QHBoxLayout *right) : QLayout(parent), m_zones{left, centre, right}
    {
        setContentsMargins(0, 0, 0, 0);
        for (auto *zone : {left, centre, right}) addChildLayout(zone);
    }
    ~ZoneRow() override { while (auto *item = takeAt(0)) delete item; }
    void addItem(QLayoutItem *) override {}
    int count() const override { return int(m_zones.size()); }
    QLayoutItem *itemAt(int index) const override { return index >= 0 && index < count() ? m_zones[index] : nullptr; }
    // Removing a zone ends the row's layout of it (QLayout::removeItem, a zone's deletion).
    QLayoutItem *takeAt(int index) override { return index >= 0 && index < count() ? m_zones.takeAt(index) : nullptr; }
    Qt::Orientations expandingDirections() const override { return Qt::Horizontal; }
    QSize sizeHint() const override
    {
        int w = 0, h = 0;
        for (auto *zone : m_zones) { const auto hint = zone->sizeHint(); w += hint.width(); h = qMax(h, hint.height()); }
        return {w + 2 * spacing(), h};
    }
    QSize minimumSize() const override { return {0, sizeHint().height()}; }
    void setGeometry(const QRect &rect) override
    {
        QLayout::setGeometry(rect);
        if (m_zones.size() != 3) return;
        const auto used = [](QLayoutItem *zone) { return zone->isEmpty() ? 0 : zone->sizeHint().width(); };
        const int gap = spacing(), left = used(m_zones[0]), centre = used(m_zones[1]), right = used(m_zones[2]);
        if (!centre) {
            m_zones[0]->setGeometry(QRect(rect.x(), rect.y(), qMin(left, rect.width()), rect.height()));
            const int width = qMax(qBound(0, rect.width() - left - (left ? gap : 0), rect.width()), qMin(right, rect.width()));
            m_zones[1]->setGeometry(QRect(rect.x() + left, rect.y(), 0, rect.height()));
            m_zones[2]->setGeometry(QRect(rect.right() + 1 - width, rect.y(), width, rect.height()));
            return;
        }
        int x = rect.x() + (rect.width() - centre) / 2;
        x = qMax(x, rect.x() + left + (left ? gap : 0));
        x = qMin(x, rect.right() + 1 - centre - right - (right ? gap : 0));
        x = qMax(x, rect.x());
        m_zones[0]->setGeometry(QRect(rect.x(), rect.y(), qMax(0, x - gap - rect.x()), rect.height()));
        m_zones[1]->setGeometry(QRect(x, rect.y(), centre, rect.height()));
        const int start = x + centre + gap;
        m_zones[2]->setGeometry(QRect(start, rect.y(), qMax(0, rect.right() + 1 - start), rect.height()));
    }
private:
    QList<QLayoutItem *> m_zones;
};
}

ComposerToolbar::ComposerToolbar(QWidget *parent) : QWidget(parent)
{
    setObjectName("composerToolbar"); setFixedHeight(RowHeight);
    // Never let the chips widen the composer: the row fits itself instead.
    setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    m_leading = new QHBoxLayout; m_leading->setSpacing(Spacing); m_leading->setContentsMargins(0, 0, 0, 0);
    m_centre = new QHBoxLayout; m_centre->setSpacing(8); m_centre->setContentsMargins(0, 0, 0, 0);
    m_trailing = new QHBoxLayout; m_trailing->setSpacing(Spacing); m_trailing->setContentsMargins(0, 0, 0, 0);
    // The left zone keeps its items left, the right one right.
    m_leading->addStretch(1); m_trailing->addStretch(1);
    auto *row = new ZoneRow(this, m_leading, m_centre, m_trailing); row->setSpacing(Spacing * 2);
    row->setSizeConstraint(QLayout::SetNoConstraint);
}

ComposerToolbar::Zone ComposerToolbar::zone(Slot slot, bool three)
{
    switch (slot) {
    case Slot::Attach: case Slot::FileReference: case Slot::Attachments: case Slot::Voice: case Slot::VoiceCancel: case Slot::VoiceResult:
    case Slot::IronRules: case Slot::Latest: case Slot::MarkRead: case Slot::Compaction: case Slot::CompactionCancel:
        return Zone::Left;
    case Slot::VoiceMode: return three ? Zone::Centre : Zone::Left;
    case Slot::UsageLimit: case Slot::Recovery: case Slot::Cache: case Slot::Context: case Slot::SessionReference:
        return three ? Zone::Centre : Zone::Right;
    case Slot::Status: case Slot::StatusRefresh: case Slot::Retry: case Slot::Model:
        return Zone::Right;
    }
    return Zone::Left;
}

// The order inside a zone: two zones keep the classic order (the voice mode
// after Voice), three follow the prototype.
int ComposerToolbar::rank(Slot slot, bool three)
{
    static const QList<Slot> classic{Slot::Attach, Slot::FileReference, Slot::Attachments, Slot::Voice, Slot::VoiceMode, Slot::VoiceCancel,
        Slot::VoiceResult, Slot::IronRules, Slot::Latest, Slot::MarkRead, Slot::Compaction, Slot::CompactionCancel,
        Slot::UsageLimit, Slot::Recovery, Slot::Cache, Slot::Context, Slot::SessionReference, Slot::Status, Slot::StatusRefresh, Slot::Retry, Slot::Model};
    static const QList<Slot> zones{Slot::Attach, Slot::FileReference, Slot::Attachments, Slot::Voice, Slot::VoiceCancel, Slot::VoiceResult,
        Slot::IronRules, Slot::Latest, Slot::MarkRead, Slot::Compaction, Slot::CompactionCancel,
        Slot::VoiceMode, Slot::UsageLimit, Slot::Recovery, Slot::Cache, Slot::Context, Slot::SessionReference,
        Slot::Status, Slot::StatusRefresh, Slot::Retry, Slot::Model};
    return int((three ? zones : classic).indexOf(slot));
}

int ComposerToolbar::priority(Slot slot)
{
    switch (slot) {
    case Slot::MarkRead: return 10;
    case Slot::Attachments: return 20;
    case Slot::Voice: return 25;
    case Slot::Cache: return 30;
    case Slot::Recovery: return 40;
    case Slot::UsageLimit: return 50;
    default: return NeverShortened;
    }
}

QHBoxLayout *ComposerToolbar::zoneLayout(Zone zone) const
{
    return zone == Zone::Left ? m_leading : zone == Zone::Centre ? m_centre : m_trailing;
}

void ComposerToolbar::place(const Item &item)
{
    const Zone where = zone(item.slot, m_three);
    int index = 0;
    for (const auto &existing : m_items)
        if (existing.widget && existing.widget != item.widget && zone(existing.slot, m_three) == where
            && rank(existing.slot, m_three) < rank(item.slot, m_three) && zoneLayout(where)->indexOf(existing.widget) >= 0) ++index;
    // The right zone starts with its stretch.
    if (where == Zone::Right) ++index;
    zoneLayout(where)->insertWidget(index, item.widget, item.slot == Slot::Status ? 1 : 0, Qt::AlignVCenter);
}

void ComposerToolbar::remove(QWidget *item)
{
    for (int i = 0; i < m_items.size(); ++i) {
        if (m_items[i].widget != item) continue;
        for (auto *zone : {m_leading, m_centre, m_trailing}) zone->removeWidget(item);
        if (qobject_cast<ToolbarChip *>(item)) disconnect(item, nullptr, this, nullptr); else item->removeEventFilter(this);
        m_items.removeAt(i);
        break;
    }
    m_fitted.clear(); scheduleFit();
}

void ComposerToolbar::setThreeZones(bool three)
{
    if (three == m_three) return;
    m_three = three;
    for (const auto &item : m_items) if (item.widget) for (auto *zone : {m_leading, m_centre, m_trailing}) zone->removeWidget(item.widget);
    auto order = m_items;
    std::stable_sort(order.begin(), order.end(), [three](const Item &a, const Item &b) { return rank(a.slot, three) < rank(b.slot, three); });
    for (const auto &item : order) if (item.widget) place(item);
    m_fitted.clear(); fit();
}

void ComposerToolbar::add(Slot slot, QWidget *item)
{
    m_items.append({slot, item});
    place(m_items.last());
    if (auto *chip = qobject_cast<ToolbarChip *>(item)) {
        chip->setTheme(m_dark); connect(chip, &ToolbarChip::fitChanged, this, &ComposerToolbar::scheduleFit);
        connect(chip, &ToolbarChip::focusReleased, this, [this] {
            if (m_focusFallback && m_focusFallback->isVisible() && m_focusFallback->isEnabled()) m_focusFallback->setFocus(Qt::OtherFocusReason);
        });
    } else item->installEventFilter(this);
    scheduleFit();
}

void ComposerToolbar::setTheme(bool dark)
{
    m_dark = dark;
    for (const auto &item : m_items) if (auto *chip = qobject_cast<ToolbarChip *>(item.widget)) chip->setTheme(dark);
}

void ComposerToolbar::closePopovers()
{
    for (const auto &item : m_items) if (auto *chip = qobject_cast<ToolbarChip *>(item.widget)) chip->closePopover();
}

void ComposerToolbar::scheduleFit()
{
    if (m_scheduled || m_fitting) return;
    m_scheduled = true;
    QTimer::singleShot(0, this, [this] { m_scheduled = false; fit(); });
}

bool ComposerToolbar::event(QEvent *event)
{
    // A plain item (context counter) changed its size hint.
    if (event->type() == QEvent::LayoutRequest) scheduleFit();
    return QWidget::event(event);
}

bool ComposerToolbar::eventFilter(QObject *watched, QEvent *event)
{
    if (event->type() == QEvent::Show || event->type() == QEvent::Hide) scheduleFit();
    return QWidget::eventFilter(watched, event);
}

void ComposerToolbar::resizeEvent(QResizeEvent *event) { QWidget::resizeEvent(event); fit(); }

// Everything fit() depends on. Its own compaction changes none of it, so the
// layout requests that compaction causes do not start another pass.
QList<int> ComposerToolbar::fitInputs() const
{
    QList<int> inputs{contentsRect().width()};
    for (const auto &item : m_items) {
        if (!item.widget) continue;
        if (auto *chip = qobject_cast<ToolbarChip *>(item.widget))
            inputs << chip->isActive() << chip->labelSizeHint(false).width() << chip->labelSizeHint(true).width();
        else inputs << item.widget->isVisibleTo(this) << item.widget->sizeHint().width();
    }
    return inputs;
}

int ComposerToolbar::zoneWidth(Zone zone) const
{
    int width = 0, count = 0;
    for (const auto &item : m_items) {
        // The status elides into whatever its zone has left.
        if (!item.widget || !item.widget->isVisibleTo(this) || ComposerToolbar::zone(item.slot, m_three) != zone || item.slot == Slot::Status) continue;
        width += qMin(item.widget->sizeHint().width(), item.widget->maximumWidth()); ++count;
    }
    return width + qMax(0, count - 1) * zoneLayout(zone)->spacing();
}

int ComposerToolbar::requiredWidth() const
{
    const int left = zoneWidth(Zone::Left), centre = zoneWidth(Zone::Centre), right = zoneWidth(Zone::Right);
    // The stretch between the zones is empty, so the layout spaces only real items.
    if (!centre) return left + right + (left && right ? Spacing * 2 : 0);
    // The centre stays centred: both sides get the wider side's width.
    return centre + 2 * qMax(left, right) + Spacing * 4;
}

void ComposerToolbar::fit()
{
    const auto inputs = fitInputs();
    if (m_fitting || inputs == m_fitted) return;
    m_fitting = true;
    auto order = m_items;
    std::stable_sort(order.begin(), order.end(), [](const Item &a, const Item &b) { return priority(a.slot) < priority(b.slot); });
    QList<ToolbarChip *> chips;   // lowest priority first
    for (const auto &item : order)
        if (auto *chip = qobject_cast<ToolbarChip *>(item.widget); chip && priority(item.slot) < NeverShortened) {
            chip->setCompact(false); chip->setFitHidden(false); chip->setMaximumWidth(QWIDGETSIZE_MAX); chips << chip;
        }
    const int available = contentsRect().width();
    for (auto *chip : chips) {
        if (requiredWidth() <= available) break;
        if (chip->isActive() && chip->labelSizeHint(true).width() < chip->labelSizeHint(false).width()) chip->setCompact(true);
    }
    for (const auto &item : order) {
        if (requiredWidth() <= available) break;
        if (item.slot == Slot::MarkRead) if (auto *chip = qobject_cast<ToolbarChip *>(item.widget)) chip->setFitHidden(true);
    }
    // Last resort: elide the widest chips so the row never wraps.
    int excess = requiredWidth() - available;
    if (excess > 0) {
        QList<ToolbarChip *> widest;
        for (auto *chip : chips) if (chip->isVisibleTo(this)) widest << chip;
        std::sort(widest.begin(), widest.end(), [](ToolbarChip *a, ToolbarChip *b) { return a->sizeHint().width() > b->sizeHint().width(); });
        for (auto *chip : widest) {
            if (excess <= 0) break;
            const int hint = chip->sizeHint().width(), target = qMax(chip->minimumSizeHint().width(), hint - excess);
            chip->setMaximumWidth(target); excess -= hint - target;
        }
    }
    m_fitted = inputs; m_fitting = false;
}
