#include "TaskRow.h"
#include "MonospaceFont.h"
#include "SessionStatusBadge.h"
#include "TaskDrag.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QApplication>
#include <QDateTime>
#include <QGraphicsOpacityEffect>
#include <QHBoxLayout>
#include <QLabel>
#include <QMouseEvent>
#include <QMenu>
#include <QContextMenuEvent>
#include <QPainter>
#include <QTextLayout>
#include <QTimer>
#include <QToolTip>
#include <QToolButton>
#include <QVBoxLayout>

TaskColors TaskColors::of(bool dark) {
    if (dark) return {QColor("#e8edf4"), QColor("#9eabba"), QColor("#242d36"), QColor("#34404a"), QColor("#8bdfc0"),
                      QColor("#2b3540"), QColor("#edbd77"), QColor("#98bdeb"), QColor("#c5a8f5"), true};
    return {QColor("#1a2733"), QColor("#657487"), QColor("#f3f6f8"), QColor("#dbe3e9"), QColor("#167357"),
            QColor("#edf2f5"), QColor("#9b6216"), QColor("#366ba9"), QColor("#6c43b8"), false};
}

// The stage circle; In progress spins a half ring inside it while the row shows.
class TaskGlyph : public QLabel {
public:
    TaskGlyph() { connect(&m_timer, &QTimer::timeout, this, [this] { m_angle = (m_angle + 12) % 360; update(); }); m_timer.setInterval(33); }
    void setSpinning(bool spinning, const QColor &color) {
        m_color = color; setProperty("spinning", spinning);
        if (spinning && isVisible()) m_timer.start(); else if (!spinning) m_timer.stop();
        update();
    }
protected:
    void showEvent(QShowEvent *event) override { QLabel::showEvent(event); if (property("spinning").toBool()) m_timer.start(); }
    void hideEvent(QHideEvent *event) override { QLabel::hideEvent(event); m_timer.stop(); }
    void paintEvent(QPaintEvent *event) override {
        QLabel::paintEvent(event);
        if (!property("spinning").toBool()) return;
        QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
        p.setPen(QPen(m_color, 2)); const QPointF center = QRectF(rect()).center();
        p.drawArc(QRectF(center.x() - 5, center.y() - 5, 10, 10), (-45 - m_angle) * 16, 180 * 16);
    }
private:
    QTimer m_timer; QColor m_color; int m_angle = 0;
};

// The session's status as the session list shows it.
class TaskStatusPill : public QWidget {
public:
    void setStatus(const QString &caption, int kind, bool dark) {
        setProperty("caption", caption); setProperty("kind", kind); m_dark = dark;
        setFixedSize(SessionStatusBadge::width(caption, SessionStatusBadge::Kind(kind), font()), 18); update();
    }
protected:
    void paintEvent(QPaintEvent *) override {
        QPainter p(this);
        SessionStatusBadge::paint(&p, rect(), property("caption").toString(), SessionStatusBadge::Kind(property("kind").toInt()), m_dark);
    }
private:
    bool m_dark = true;
};

QColor TaskColors::stage(const QString &stage) const {
    if (stage == QLatin1String("review")) return warn;
    if (stage == QLatin1String("active")) return accent;
    if (stage == QLatin1String("todo")) return blue;
    if (stage == QLatin1String("done")) return violet;
    return muted;
}

namespace {
QString age(qint64 ms) {
    const qint64 minutes = qMax<qint64>(0, ms / 60000);
    if (minutes < 60) return QStringLiteral("%1 min").arg(minutes);
    if (minutes < 24 * 60) return QStringLiteral("%1 h").arg(minutes / 60);
    return QStringLiteral("%1 d").arg(minutes / (24 * 60));
}
// The text as at most `lines` lines of `width`, the last one elided.
QString clamp(const QString &text, const QFont &font, int width, int lines) {
    if (width <= 0) return text;
    QTextLayout layout(text, font); layout.beginLayout();
    QStringList out;
    for (QTextLine line = layout.createLine(); line.isValid(); line = layout.createLine()) {
        line.setLineWidth(width);
        if (int(out.size()) == lines - 1) {
            const QString rest = text.mid(line.textStart()).simplified();
            out << QFontMetrics(font).elidedText(rest, Qt::ElideRight, width); break;
        }
        out << text.mid(line.textStart(), line.textLength()).trimmed();
    }
    layout.endLayout();
    return out.join(QLatin1Char('\n'));
}
}

QString TaskRow::actionKey(TaskAction action) {
    switch (action) {
    case TaskAction::Edit: return QStringLiteral("edit");
    case TaskAction::Rename: return QStringLiteral("rename");
    case TaskAction::ToBacklog: return QStringLiteral("toBacklog");
    case TaskAction::Top: return QStringLiteral("top");
    case TaskAction::SendNow: return QStringLiteral("sendNow");
    case TaskAction::Pause: return QStringLiteral("pause");
    case TaskAction::ToTodo: return QStringLiteral("toTodo");
    case TaskAction::Cancel: return QStringLiteral("cancel");
    case TaskAction::BackToTodo: return QStringLiteral("backToTodo");
    case TaskAction::Accept: return QStringLiteral("accept");
    }
    return {};
}

TaskRow::TaskRow(QWidget *parent) : QWidget(parent), m_colors(TaskColors::of(true)) {
    setAttribute(Qt::WA_Hover); setCursor(Qt::PointingHandCursor);
    setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Fixed);   // rows never take the list's spare height
    // Tab reaches a row, Enter or Space opens it, and its actions follow the focus.
    setFocusPolicy(Qt::TabFocus);
    auto *layout = new QHBoxLayout(this); layout->setContentsMargins(12, 6, 12, 10); layout->setSpacing(11);
    m_glyph = new TaskGlyph; m_glyph->setObjectName("taskGlyph"); m_glyph->setFixedSize(22, 22); m_glyph->setAlignment(Qt::AlignCenter);
    auto *glyphColumn = new QVBoxLayout; glyphColumn->setContentsMargins(0, 4, 0, 0); glyphColumn->addWidget(m_glyph); glyphColumn->addStretch();
    layout->addLayout(glyphColumn);
    auto *main = m_mainLayout = new QVBoxLayout; main->setContentsMargins(0, 0, 0, 0); main->setSpacing(0); layout->addLayout(main, 1);
    // The header's height is fixed, so showing the icons never moves anything.
    auto *header = new QWidget; header->setFixedHeight(34);
    auto *headerLayout = m_headerLayout = new QHBoxLayout(header); headerLayout->setContentsMargins(0, 0, 0, 0); headerLayout->setSpacing(8);
    // The rich-text labels take no mouse input: presses, drags and clicks on them are the row's.
    m_meta = new QLabel; m_meta->setObjectName("taskMeta"); m_meta->setTextFormat(Qt::RichText); m_meta->setTextInteractionFlags(Qt::NoTextInteraction);
    // The meta keeps its width, then In progress's status, then the rest, which gives way first.
    m_meta->setSizePolicy(QSizePolicy::Maximum, QSizePolicy::Preferred); m_meta->setMinimumWidth(1);
    m_status = new TaskStatusPill; m_status->setObjectName("taskStatus"); m_status->hide();
    m_tail = new QLabel; m_tail->setObjectName("taskTail"); m_tail->setTextFormat(Qt::RichText); m_tail->setTextInteractionFlags(Qt::NoTextInteraction);
    m_tail->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    m_actions = new QWidget; m_actions->setObjectName("taskActions");
    auto *actionsLayout = new QHBoxLayout(m_actions); actionsLayout->setContentsMargins(0, 0, 0, 0); actionsLayout->setSpacing(2);
    m_more = new QToolButton(m_actions); m_more->setObjectName("taskMoreActions"); m_more->setFixedSize(34, 32);
    m_more->setIconSize(QSize(20, 20)); m_more->setAutoRaise(true); m_more->setPopupMode(QToolButton::InstantPopup);
    m_more->setCursor(Qt::PointingHandCursor); m_more->setToolTip(tr("Task actions")); m_more->setAccessibleName(tr("Task actions")); m_more->hide();
    m_menu = new QMenu(m_more); m_menu->setObjectName("taskContextMenu"); m_menu->setToolTipsVisible(true); m_more->setMenu(m_menu);
    connect(m_menu, &QMenu::aboutToShow, this, &TaskRow::rebuildMenu);
    actionsLayout->addWidget(m_more);
    headerLayout->addWidget(m_meta); headerLayout->addWidget(m_status); headerLayout->addWidget(m_tail, 1); headerLayout->addWidget(m_actions);
    main->addWidget(header);
    m_text = new QLabel; m_text->setObjectName("taskText"); m_text->setTextFormat(Qt::PlainText);
    m_text->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Preferred);
    main->addWidget(m_text);
    m_summary = new QLabel; m_summary->setObjectName("taskSummary"); m_summary->setTextFormat(Qt::RichText); m_summary->setWordWrap(true);
    m_summary->setTextInteractionFlags(Qt::NoTextInteraction);
    main->addSpacing(2); main->addWidget(m_summary); m_summary->hide();
}

void TaskRow::setCompact(bool compact) {
    if (property("compact").toBool() == compact) return;
    setProperty("compact", compact);
    if (compact) {
        m_headerLayout->removeWidget(m_actions); m_actions->setFixedHeight(34);
        static_cast<QHBoxLayout *>(layout())->addWidget(m_actions, 0, Qt::AlignTop);
        m_mainLayout->removeWidget(m_text); m_mainLayout->insertWidget(0, m_text);
        m_headerLayout->parentWidget()->setFixedHeight(22);
    } else {
        layout()->removeWidget(m_actions); m_headerLayout->addWidget(m_actions);
        m_mainLayout->removeWidget(m_text); m_mainLayout->insertWidget(1, m_text);
        m_headerLayout->parentWidget()->setFixedHeight(34);
    }
    m_tail->setVisible(!compact); m_summary->setVisible(!compact && !m_summary->text().isEmpty());
    m_headerLayout->setAlignment(compact ? Qt::Alignment(Qt::AlignLeft) : Qt::Alignment{});
    updateGeometry();
}

void TaskRow::setTask(const TaskItem &task, const TaskRowState &state, const TaskColors &colors, qint64 now) {
    const bool rebuild = task.stage != m_task.stage || m_buttons.isEmpty();
    const bool menuChanged = rebuild || state.blocked != m_state.blocked || colors.dark != m_colors.dark;
    m_task = task; m_state = state; m_colors = colors;
    setCursor(TaskDrag::draggable(task) ? Qt::OpenHandCursor : Qt::PointingHandCursor);
    setProperty("mine", state.mine);
    const QColor stage = colors.stage(task.stage);
    const QString glyph = task.stage == QLatin1String("todo") ? QString::number(task.queue)
        : task.stage == QLatin1String("review") || task.stage == QLatin1String("done") ? QStringLiteral("✓")
        : QString();
    m_glyph->setText(glyph); m_glyph->setSpinning(task.stage == QLatin1String("active"), stage);
    setAccessibleName(task.id + QStringLiteral(": ") + task.displayTitle());
    setWorkspaceStyle(m_glyph, QStringLiteral("border:2px solid %1;border-radius:11px;color:%1;font-size:11px;font-weight:700;").arg(stage.name()));
    // Meta: ID, the session (or the target), the session's status for In
    // progress, the time, and who filed it.
    const auto who = [&](const QString &text) {
        return state.mine ? QStringLiteral("<span style='color:%1;font-weight:600'>%2</span>").arg(colors.accent.name(), text.toHtmlEscaped())
                          : text.toHtmlEscaped();
    };
    QStringList meta{QStringLiteral("<span style=\"font-family:'%1';font-size:10.5px\">%2</span>").arg(monospaceFamily(), task.id.toHtmlEscaped())};
    if (task.targetKind == QLatin1String("new_session") && (task.stage == "todo" || task.stage == "backlog")) meta << tr("→ new session when started");
    else if (task.targetKind == QLatin1String("subagent") && (task.stage == "todo" || task.stage == "backlog")) meta << tr("→ new subagent");
    else if (task.stage == QLatin1String("todo") || task.stage == QLatin1String("backlog")) {
        meta << (task.targetSession.isEmpty()
            ? QStringLiteral("→ any session") + (state.anyNow.isEmpty() ? QString() : QStringLiteral(" · now ") + state.anyNow.toHtmlEscaped())
            : who(QStringLiteral("→ ") + TaskBoard::sessionLabel(task.targetHost, task.targetSession)));
    } else if (!task.worker.isEmpty()) {
        meta << tr("Subagent: %1").arg(task.worker.value("name").toString()).toHtmlEscaped();
    } else if (task.targetKind == QLatin1String("subagent") && !task.sessionName.isEmpty()) {
        meta << tr("coordinator %1 · child unconfirmed").arg(TaskBoard::sessionLabel(task.sessionHost, task.sessionName)).toHtmlEscaped();
    } else if (!task.sessionName.isEmpty()) {
        meta << who(TaskBoard::sessionLabel(task.sessionHost, task.sessionName));
    }
    const bool active = task.stage == QLatin1String("active");
    m_status->setVisible(active && !state.sessionStatus.isEmpty());
    if (m_status->isVisibleTo(this)) m_status->setStatus(state.sessionStatus, state.statusKind, colors.dark);
    QStringList tail;
    if (active) tail << age(now - task.stageAt);
    if (task.stage == QLatin1String("done") && now - task.stageAt > TaskBoard::RecentDoneMs)
        tail << QStringLiteral("accepted ") + QDateTime::fromMSecsSinceEpoch(task.stageAt).toString(QStringLiteral("d MMM"));
    if (!task.author.isEmpty())
        tail << QStringLiteral("<span style='color:%1'>filed by %2</span>").arg(colors.violet.name(), TaskBoard::sessionLabel(task.authorHost, task.author).toHtmlEscaped());
    m_meta->setText(meta.join(QStringLiteral("&nbsp;&nbsp;")));
    m_tail->setText(tail.join(QStringLiteral("&nbsp;&nbsp;")));
    for (auto *label : {m_meta, m_tail}) setWorkspaceStyle(label, QStringLiteral("color:%1;font-size:11px;").arg(colors.muted.name()));
    const bool quiet = task.stage == QLatin1String("backlog") || task.stage == QLatin1String("done");
    const bool compact = property("compact").toBool();
    setWorkspaceStyle(m_text, QStringLiteral("color:%1;%2").arg((quiet ? colors.muted : colors.fg).name(), compact ? "font-weight:600;" : ""));
    // In review: the agent's latest report, or the cancellation.
    QString summary;
    if (!compact && task.stage == QLatin1String("review")) {
        if (task.cancelledAt > 0)
            summary = QStringLiteral("<b>Cancelled at %1%2</b> %3").arg(QDateTime::fromMSecsSinceEpoch(task.cancelledAt).toString(QStringLiteral("HH:mm")),
                task.cancelReason.isEmpty() ? QString() : QStringLiteral(":"), task.cancelReason.toHtmlEscaped());
        else if (!task.summary.isEmpty())
            summary = QStringLiteral("<b>%1 reported:</b> %2").arg(TaskBoard::sessionLabel(task.sessionHost, task.sessionName).toHtmlEscaped(), task.summary.toHtmlEscaped());
    }
    m_summary->setText(compact ? QString() : summary); m_summary->setVisible(!compact && !summary.isEmpty());
    setWorkspaceStyle(m_summary, QStringLiteral("color:%1;border-left:2px solid %2;padding:1px 0 1px 10px;").arg(colors.muted.name(), stage.name()));
    if (rebuild) {
        qDeleteAll(m_buttons); m_buttons.clear();
        auto actions = TaskBoard::actions(task.stage);
        if (!actions.contains(TaskBoard::primaryAction(task.stage))) actions << TaskBoard::primaryAction(task.stage);
        for (const auto action : actions) {
            auto *button = new QToolButton(m_actions); button->setObjectName(QStringLiteral("taskAction:") + actionKey(action));
            button->setFixedSize(34, 32); button->setIconSize(QSize(20, 20)); button->setAutoRaise(true); button->setCursor(Qt::ArrowCursor);
            button->setAttribute(Qt::WA_LayoutUsesWidgetRect);
            connect(button, &QToolButton::clicked, this, [this, action, button] {
                const QString why = button->property("blockedReason").toString();
                if (why.isEmpty()) return emit actionRequested(action);
                // A blocked action says why beside it, as a blocked message send does.
                if (property("compact").toBool()) emit opened();
                QToolTip::showText(button->mapToGlobal(QPoint(0, button->height())), why.toHtmlEscaped(), button, {}, 5000);
            });
            button->installEventFilter(this);
            m_actions->layout()->addWidget(button); m_buttons.insert(int(action), button);
        }
    }
    const int actionCount = m_buttons.size();
    m_actions->setFixedWidth(compact ? 70 : actionCount * 34 + qMax(0, actionCount - 1) * 2);
    if (m_more->property("iconColor") != colors.muted) { m_more->setProperty("iconColor", colors.muted); m_more->setIcon(workspaceIcon("more", colors.muted)); }
    setWorkspaceStyle(m_more, QStringLiteral("QToolButton{border:0;border-radius:6px;background:transparent;}"
        "QToolButton::menu-indicator{image:none;width:0;height:0;}QToolButton:hover{background:%1;}").arg(colors.hover.name()));
    if (menuChanged && !m_menu->isVisible()) rebuildMenu();
    for (auto it = m_buttons.begin(); it != m_buttons.end(); ++it) {
        const auto action = TaskAction(it.key()); const QString why = state.blocked.value(it.key());
        // A blocked action stays clickable, dimmed, so a click can say why it does nothing.
        QColor icon = action == TaskBoard::primaryAction(task.stage) ? colors.accent : colors.muted;
        if (!why.isEmpty()) icon.setAlphaF(0.45);
        const auto iconKey = TaskBoard::actionIcon(action) + icon.name(QColor::HexArgb);
        if (it.value()->property("taskIconKey").toString() != iconKey) {
            it.value()->setProperty("taskIconKey", iconKey); it.value()->setIcon(workspaceIcon(TaskBoard::actionIcon(action), icon));
        }
        it.value()->setProperty("blockedReason", why); it.value()->setToolTip(why.isEmpty() ? TaskBoard::actionLabel(action) : why);
        it.value()->setAccessibleName(TaskBoard::actionLabel(action));
        it.value()->setAccessibleDescription(why);
        setWorkspaceStyle(it.value(), QStringLiteral("QToolButton{border:0;border-radius:6px;background:transparent;}QToolButton:hover{background:%1;}").arg(colors.hover.name()));
    }
    updateActions();
    layoutText(); update();
}

void TaskRow::rebuildMenu() {
    m_menu->clear();
    for (const auto action : TaskBoard::actions(m_task.stage)) {
        const QString why = m_state.blocked.value(int(action));
        auto *entry = m_menu->addAction(workspaceIcon(TaskBoard::actionIcon(action), m_colors.muted), TaskBoard::actionLabel(action));
        entry->setObjectName(QStringLiteral("taskMenuAction:") + actionKey(action));
        entry->setEnabled(why.isEmpty()); entry->setToolTip(why.isEmpty() ? TaskBoard::actionLabel(action) : why);
        connect(entry, &QAction::triggered, this, [this, action] { emit actionRequested(action); });
    }
}

void TaskRow::updateActions() {
    const auto *focus = QApplication::focusWidget();
    showActions(m_hovered || (focus && (focus == this || isAncestorOf(focus))));
}

void TaskRow::focusInEvent(QFocusEvent *event) { QWidget::focusInEvent(event); updateActions(); update(); }

// The focus may move to one of the row's own actions: decide once it has landed.
void TaskRow::focusOutEvent(QFocusEvent *event) { QWidget::focusOutEvent(event); QTimer::singleShot(0, this, &TaskRow::updateActions); update(); }

bool TaskRow::eventFilter(QObject *watched, QEvent *event) {
    if (event->type() == QEvent::FocusOut) QTimer::singleShot(0, this, &TaskRow::updateActions);
    return QWidget::eventFilter(watched, event);
}

void TaskRow::keyPressEvent(QKeyEvent *event) {
    if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter || event->key() == Qt::Key_Space) { emit opened(); event->accept(); return; }
    QWidget::keyPressEvent(event);
}

void TaskRow::contextMenuEvent(QContextMenuEvent *event) {
    m_pressed = false;
    m_menu->popup(event->reason() == QContextMenuEvent::Keyboard ? mapToGlobal(rect().center()) : event->globalPos()); event->accept();
}

void TaskRow::showActions(bool shown) {
    // The container keeps its width, so the meta never moves.
    const bool compact = property("compact").toBool();
    for (auto it = m_buttons.begin(); it != m_buttons.end(); ++it) {
        const bool primary = TaskAction(it.key()) == TaskBoard::primaryAction(m_task.stage);
        it.value()->setVisible(compact ? primary : shown);
        it.value()->setEnabled(true);
    }
    m_more->setVisible(compact);
}

void TaskRow::layoutText() {
    const QString text = property("compact").toBool() ? m_task.displayTitle().simplified() : m_task.text;
    m_text->setText(clamp(text, m_text->font(), m_text->width() > 0 ? m_text->width() : width() - 60, 2));
}

void TaskRow::resizeEvent(QResizeEvent *event) { QWidget::resizeEvent(event); layoutText(); }

void TaskRow::enterEvent(QEnterEvent *event) { QWidget::enterEvent(event); m_hovered = true; updateActions(); update(); }

void TaskRow::leaveEvent(QEvent *event) { QWidget::leaveEvent(event); m_hovered = false; updateActions(); update(); }

// A click opens the task when it starts and ends on the row, outside its actions.
void TaskRow::mousePressEvent(QMouseEvent *event) {
    QWidget::mousePressEvent(event);
    m_pressed = event->button() == Qt::LeftButton && !m_actions->geometry().translated(m_actions->parentWidget()->mapTo(this, QPoint()))
                                                         .contains(event->position().toPoint());
    m_pressAt = event->position().toPoint();
}

// A writable row pressed outside its actions is dragged once the pointer
// moves past the drag distance; the release after a drag opens nothing.
void TaskRow::mouseMoveEvent(QMouseEvent *event) {
    QWidget::mouseMoveEvent(event);
    if (!m_pressed || !(event->buttons() & Qt::LeftButton) || !TaskDrag::draggable(m_task)) return;
    if ((event->position().toPoint() - m_pressAt).manhattanLength() < QApplication::startDragDistance()) return;
    // The row takes the move, so Qt hands it to no parent once the drag is over.
    event->accept(); m_pressed = false; emit dragStarted(m_pressAt);
}

void TaskRow::setDragging(bool dragging) {
    setProperty("dragging", dragging);
    if (!dragging) { setGraphicsEffect(nullptr); return; }
    auto *fade = new QGraphicsOpacityEffect(this); fade->setOpacity(0.4); setGraphicsEffect(fade);
}

void TaskRow::mouseReleaseEvent(QMouseEvent *event) {
    QWidget::mouseReleaseEvent(event);
    const bool click = std::exchange(m_pressed, false) && event->button() == Qt::LeftButton && rect().contains(event->position().toPoint());
    if (click) emit opened();
}

void TaskRow::paintEvent(QPaintEvent *) {
    QPainter p(this);
    const bool lit = m_hovered || hasFocus() || property("expanded").toBool();
    if (property("compact").toBool()) {
        const bool expanded = property("expanded").toBool();
        QColor fill = expanded ? m_colors.accent : m_colors.fg; fill.setAlphaF(expanded ? .07 : lit ? .035 : 0);
        p.setRenderHint(QPainter::Antialiasing); p.setBrush(fill);
        p.setPen(QPen(expanded ? m_colors.accent : m_colors.border, expanded ? 1.2 : .5));
        p.drawRoundedRect(QRectF(rect()).adjusted(.5, .5, -.5, -.5), 7, 7); return;
    }
    QColor tint = m_state.mine ? m_colors.accent : m_colors.fg;
    tint.setAlphaF(m_state.mine ? (lit ? 0.11 : 0.07) : (lit ? 0.04 : 0));
    p.fillRect(rect(), tint);
    if (m_state.mine) { p.setPen(Qt::NoPen); p.setBrush(m_colors.accent); p.drawRoundedRect(QRectF(0, 6, 3, height() - 12), 1.5, 1.5); }
    if (property("separated").toBool()) { p.setPen(QPen(m_colors.border, 1)); p.drawLine(41, 0, width(), 0); }
}
