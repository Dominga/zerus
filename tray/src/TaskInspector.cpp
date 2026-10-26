#include "TaskInspector.h"
#include "ActivityView.h"
#include "TaskDialogs.h"
#include "TaskRow.h"
#include "TaskStageFilter.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QDateTime>
#include <QDropEvent>
#include <QFrame>
#include <QScrollBar>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QLineEdit>
#include <QMessageBox>
#include <QToolButton>
#include <QComboBox>
#include <QPushButton>
#include <QResizeEvent>
#include <QScrollArea>
#include <QSet>
#include <QTextBrowser>
#include <QTabBar>
#include <QSettings>
#include <QSignalBlocker>
#include <QSplitter>
#include <QStackedWidget>
#include <QVBoxLayout>
#include <algorithm>

namespace {
constexpr int VisibleTasks = 100;
constexpr int VisibleHistory = 80;
// Native Cocoa tab sizing can elide these labels even in a wide pane. Use the
// actual label font to reserve a complete caption on every platform.
class DetailTabs : public QTabBar {
protected:
    QSize tabSizeHint(int index) const override {
        return {fontMetrics().horizontalAdvance(tabText(index)) + 24, 34};
    }
};
QString authorLabel(const QJsonObject &actor) {
    if (actor.value("kind") != "agent") return QObject::tr("You");
    const auto name = TaskBoard::sessionLabel(actor.value("host").toString(), actor.value("session").toString());
    return name.isEmpty() ? QObject::tr("Agent") : name;
}
QJsonObject journalEntry(const QString &id, const QJsonObject &actor, const QString &label, const QString &text, double at, bool event = false) {
    return {{"id", id}, {"author", authorLabel(actor)}, {"label", label}, {"text", text}, {"at", at},
        {"role", event ? QStringLiteral("event") : actor.value("kind") == "agent" ? QStringLiteral("agent") : QStringLiteral("person")}};
}
QString sessionId(const QString &host, const QString &name, const QList<TaskSession> &sessions) {
    for (const auto &session : sessions) if (session.host == host && session.name == name) return session.id;
    return {};
}
}

TaskInspector::TaskInspector(HgsClient *client, TasksView *board, QWidget *parent)
    : QWidget(parent), m_client(client), m_board(board) {
    setObjectName("taskInspector");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(2, 10, 2, 0); layout->setSpacing(8);
    auto *header = new QHBoxLayout;
    m_scope = new QLabel; m_scope->setObjectName("taskInspectorScope"); m_scope->setTextFormat(Qt::PlainText); m_scope->setWordWrap(true);
    header->addWidget(m_scope, 1); m_board->moveControlsTo(header); layout->addLayout(header);
    m_filters = new QWidget; auto *filters = new QHBoxLayout(m_filters);
    filters->setContentsMargins(0, 0, 0, 0); filters->setSpacing(4);
    for (const auto &stage : TaskBoard::stages()) {
        auto *tile = new TaskTile(stage, m_filters); m_tiles.insert(stage, tile); filters->addWidget(tile, 1);
        tile->setAcceptDrops(true); tile->installEventFilter(this);
        connect(tile, &QAbstractButton::clicked, this, [this, stage] {
            if (m_stage != stage && !canLeaveDetails()) return;
            if (m_stage != stage) { closeInlineDetails(); m_stage = stage; m_selected.clear(); m_detail = {}; ++m_generation; m_showOlder = false; }
            refresh();
        });
    }
    layout->addWidget(m_filters);
    m_scroll = new QScrollArea; m_scroll->setObjectName("taskInspectorList"); m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame); m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_listPage = new QWidget; m_listPage->setObjectName("taskInspectorCards");
    m_listPage->setAcceptDrops(true); m_listPage->installEventFilter(this);
    m_dropLine = new QFrame(m_listPage); m_dropLine->setObjectName("taskInspectorDropLine"); m_dropLine->hide();
    m_autoScroll.setInterval(30);
    connect(&m_autoScroll, &QTimer::timeout, this, [this] {
        m_scroll->verticalScrollBar()->setValue(m_scroll->verticalScrollBar()->value() + m_scrollStep);
        followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt));
    });
    connect(m_board, &TasksView::taskDragFinished, this, [this] {
        endListDrag(); for (auto *tile : std::as_const(m_tiles)) tile->setDropTarget(false);
    });
    m_cards = new QVBoxLayout(m_listPage); m_cards->setContentsMargins(0, 0, 0, 0); m_cards->setSpacing(4); m_cards->addStretch();
    m_scroll->setWidget(m_listPage); m_scroll->setMinimumHeight(150);
    auto *boardPane = new QWidget; auto *boardLayout = new QVBoxLayout(boardPane);
    boardLayout->setContentsMargins(0, 0, 0, 0); boardLayout->setSpacing(0); boardLayout->addWidget(m_scroll, 1);
    m_older = new QPushButton; m_older->setObjectName("taskInspectorOlder"); m_older->hide(); boardLayout->addWidget(m_older);
    connect(m_older, &QPushButton::clicked, this, [this] { m_showOlder = !m_showOlder; refresh(); });
    m_splitter = new QSplitter(Qt::Vertical); m_splitter->setObjectName("taskInspectorSplit");
    m_splitter->setHandleWidth(7); m_splitter->setChildrenCollapsible(false); m_splitter->addWidget(boardPane);
    layout->addWidget(m_splitter, 1);
    m_details = new QWidget; m_details->setObjectName("taskDetailsPane"); m_details->setMinimumHeight(220);
    auto *details = new QVBoxLayout(m_details); details->setContentsMargins(0, 4, 0, 0); details->setSpacing(4);
    auto *context = new QHBoxLayout;
    m_state = new QLabel; m_state->setObjectName("taskInspectorState"); m_state->setTextFormat(Qt::PlainText);
    m_state->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed); m_state->setFixedHeight(24); context->addWidget(m_state, 1);
    m_open = new QPushButton; m_open->setObjectName("taskOpenActivity"); m_open->setAutoDefault(false);
    m_edit = new QPushButton; m_edit->setObjectName("taskInspectorEdit"); m_edit->setAutoDefault(false);
    m_open->setFixedSize(30, 28); m_edit->setFixedSize(30, 28);
    for (auto *button : {m_open, m_edit}) { button->setIconSize(QSize(20, 20)); context->addWidget(button); }
    m_open->setToolTip(tr("Open executor Activity")); m_open->setAccessibleName(m_open->toolTip());
    m_edit->setToolTip(tr("Task details")); m_edit->setAccessibleName(m_edit->toolTip());
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (const auto *found = executor()) emit executorRequested(found->host, found->name, found->id);
    });
    connect(m_edit, &QPushButton::clicked, this, [this] {
        openInlineDetails();
    });
    details->addLayout(context);
    m_detailTabs = new DetailTabs; m_detailTabs->setObjectName("taskDetailTabs"); m_detailTabs->setExpanding(false);
    m_detailTabs->setDocumentMode(true); m_detailTabs->setDrawBase(false); m_detailTabs->setElideMode(Qt::ElideNone);
    m_detailTabs->setUsesScrollButtons(false); QFont tabFont = font(); tabFont.setPixelSize(12); m_detailTabs->setFont(tabFont);
    m_detailTabs->addTab(tr("Description")); m_detailTabs->addTab(tr("Result")); m_detailTabs->addTab(tr("History"));
    details->addWidget(m_detailTabs);
    m_assignment = new QPushButton; m_assignment->setObjectName("taskInspectorAssignment"); m_assignment->setFlat(true);
    m_assignment->setSizePolicy(QSizePolicy::Ignored, QSizePolicy::Fixed);
    m_assignment->setToolTip(tr("Change assignment in task details")); details->addWidget(m_assignment);
    connect(m_assignment, &QPushButton::clicked, this, [this] {
        openInlineDetails(); if (m_form) if (auto *choice = m_form->findChild<QComboBox *>("taskAssignedTo")) choice->setFocus();
    });
    m_moreDetail = new QPushButton; m_moreDetail->setObjectName("taskDetailMore"); m_moreDetail->setFlat(true); m_moreDetail->hide();
    auto *historyNavigation = new QHBoxLayout; historyNavigation->addWidget(m_moreDetail, 0, Qt::AlignLeft); historyNavigation->addStretch();
    m_latestHistory = new QPushButton(tr("Latest")); m_latestHistory->setObjectName("taskHistoryLatest"); m_latestHistory->setFlat(true); m_latestHistory->hide();
    historyNavigation->addWidget(m_latestHistory); details->addLayout(historyNavigation);
    connect(m_latestHistory, &QPushButton::clicked, this, [this] { m_historyBefore = {}; m_historyVisible = 8; ++m_generation; fetch(); });
    connect(m_moreDetail, &QPushButton::clicked, this, [this] {
        if (m_detailTabs->currentIndex() == 2 && m_historyVisible < m_detail.value("log").toArray().size()) {
            m_historyVisible += 8; renderDetail();
        } else if (m_detailTabs->currentIndex() == 2 && m_detail.value("history_end").toInt() > m_detail.value("log").toArray().size()) {
            m_historyBefore = m_detail.value("log").toArray().first().toObject(); m_historyVisible = 8; ++m_generation; fetch();
        } else openInlineDetails();
    });
    m_content = new QStackedWidget; m_content->setObjectName("taskDetailContent");
    m_emptyDetail = new QLabel; m_emptyDetail->setObjectName("taskDetailEmpty"); m_emptyDetail->setAlignment(Qt::AlignCenter);
    m_emptyDetail->setWordWrap(true); m_content->addWidget(m_emptyDetail);
    m_activity = new ActivityView; m_activity->setObjectName("taskDetailActivity");
    m_activity->browser()->setObjectName("taskInspectorDetail"); m_activity->browser()->setAccessibleName(tr("Selected task details"));
    // Task file references are resolved by the existing task attachment dialog,
    // never relative to whichever session happens to be open.
    connect(m_activity, &ActivityView::fileReferenceActivated, this, [this] {
        openInlineDetails();
    });
    m_content->addWidget(m_activity); details->addWidget(m_content, 1);
    auto *noteRow = new QHBoxLayout; noteRow->setSpacing(4);
    m_note = new QLineEdit; m_note->setObjectName("taskInspectorNote"); m_note->setPlaceholderText(tr("Task note · not sent to executor"));
    m_saveNote = new QPushButton; m_saveNote->setObjectName("taskInspectorSaveNote"); m_saveNote->setFixedSize(30, 28);
    m_saveNote->setIconSize(QSize(20, 20)); m_saveNote->setToolTip(tr("Save note in task history")); m_saveNote->setAccessibleName(m_saveNote->toolTip());
    noteRow->addWidget(m_note, 1); noteRow->addWidget(m_saveNote); details->addLayout(noteRow);
    m_noteError = new QLabel; m_noteError->setObjectName("taskInspectorNoteError"); m_noteError->setTextFormat(Qt::PlainText); m_noteError->setWordWrap(true); m_noteError->hide(); details->addWidget(m_noteError);
    connect(m_note, &QLineEdit::textChanged, this, [this](const QString &text) {
        if (!m_noteKey.isEmpty()) { QSettings settings; if (text.isEmpty()) settings.remove(m_noteKey); else settings.setValue(m_noteKey, text); }
    });
    const auto saveNote = [this] {
        if (m_noteBusy || m_note->text().trimmed().isEmpty() || m_detail.isEmpty()) return;
        m_noteBusy = true; m_note->setEnabled(false); m_saveNote->setEnabled(false); m_noteError->hide();
        const auto uuid = m_selected;
        m_board->saveTaskNote(uuid, m_note->text().trimmed(), [this, uuid] {
            m_noteBusy = false;
            if (m_selected == uuid) m_note->clear();
            else QSettings().remove(QStringLiteral("tasks/noteDraft/%1/%2").arg(m_project, uuid));
            m_note->setEnabled(true); m_saveNote->setEnabled(true); fetch();
        }, [this](const QString &error) {
            m_noteBusy = false; m_note->setEnabled(true); m_saveNote->setEnabled(true); m_noteError->setText(error); m_noteError->show();
        });
    };
    connect(m_saveNote, &QPushButton::clicked, this, saveNote); connect(m_note, &QLineEdit::returnPressed, this, saveNote);
    m_commands = new QWidget; m_commands->setObjectName("taskInspectorActions");
    m_commandLayout = new QHBoxLayout(m_commands); m_commandLayout->setContentsMargins(0, 2, 0, 2); m_commandLayout->setSpacing(4);
    m_commands->setFixedHeight(36); m_commandLayout->addStretch();
    for (const auto action : {TaskAction::ToBacklog, TaskAction::Top, TaskAction::ToTodo, TaskAction::Cancel,
                             TaskAction::BackToTodo, TaskAction::Accept, TaskAction::SendNow, TaskAction::Pause}) {
        auto *button = new QToolButton; button->setObjectName(QStringLiteral("taskInspectorAction:") + TaskRow::actionKey(action));
        button->setIconSize(QSize(20, 20)); button->setFixedSize(34, 32); button->setAutoRaise(true);
        button->setAccessibleName(TaskBoard::actionLabel(action)); button->hide();
        connect(button, &QToolButton::clicked, this, [this, action] {
            if (!m_detail.isEmpty() && m_detail.value("uuid").toString() == m_selected) m_board->actOnTask(TaskBoard::item(labels(m_detail)), action);
        });
        m_commandLayout->addWidget(button); m_commandButtons.insert(int(action), button);
    }
    details->addWidget(m_commands); m_splitter->addWidget(m_details);
    auto saved = QSettings().value("tasks/inspectorSplit").toList();
    m_splitter->setSizes(saved.size() == 2 && saved[0].toInt() > 0 && saved[1].toInt() > 0
        ? QList<int>{saved[0].toInt(), saved[1].toInt()} : QList<int>{400, 320});
    connect(m_splitter, &QSplitter::splitterMoved, this, [this] {
        const auto sizes = m_splitter->sizes(); QSettings().setValue("tasks/inspectorSplit", QVariantList{sizes[0], sizes[1]});
    });
    connect(m_detailTabs, &QTabBar::currentChanged, this, [this] { renderDetail(); });
    connect(m_board, &TasksView::taskEditRequested, this, [this](const QString &uuid) {
        selectTask(uuid); if (m_selected != uuid) return;
        m_inlineRequested = uuid; openInlineDetails();
    });
    connect(m_board, &TasksView::boardChanged, this, &TaskInspector::refresh);
    connect(m_client, &HgsClient::taskReady, this, [this](quint64 id, const QJsonObject &reply) {
        if (id != m_request) return;
        const auto generation = property("requestGeneration").toULongLong(); m_request = 0;
        if (generation != m_generation) { fetch(); return; }
        auto task = reply.value("task").toObject();
        if (task.value("uuid").toString() != m_selected) {
            m_detail = {}; renderDetail(); m_state->setText(tr("The task reply did not match the selected task.")); return;
        }
        // Legacy CLI replies include the full journal. Retain only this bounded
        // window; the execution model will introduce indexed history reads.
        const auto log = task.value("log").toArray(); QJsonArray recent; int end = log.size();
        bool started = false;
        for (const auto &value : log) {
            const auto kind = value.toObject().value("kind").toString();
            if (kind == QLatin1String("sent")) started = false;
            else if (kind == QLatin1String("started")) started = true;
        }
        if (!m_historyBefore.isEmpty()) {
            end = -1;
            for (int i = 0; i < log.size(); ++i) if (log.at(i).toObject() == m_historyBefore) { end = i; break; }
            if (end < 0) { m_state->setText(tr("History changed; return to Latest to refresh.")); return; }
        }
        for (int i = qMax(0, end - VisibleHistory); i < end; ++i) recent.append(log.at(i));
        task["log_total"] = log.size(); task["history_end"] = end; task["start_confirmed"] = started; task["log"] = recent;
        if (task != m_detail) { m_detail = task; renderDetail(); }
        if (m_inlineRequested == m_selected) openInlineDetails();
    });
    connect(m_client, &HgsClient::taskFailed, this, [this](quint64 id, const QString &error) {
        if (id != m_request) return;
        const auto generation = property("requestGeneration").toULongLong(); m_request = 0;
        if (generation != m_generation) { fetch(); return; }
        m_detail = {}; renderDetail(); m_state->setText(tr("Task could not be read: %1").arg(error));
    });
    setTheme(true);
}

void TaskInspector::setContext(const QString &project, const QString &host, const QString &name, const QList<TaskSession> &sessions) {
    if (project == m_project && host == m_host && name == m_session && sessions == m_sessions) return;
    const QString id = sessionId(host, name, sessions);
    const bool changed = project != m_project || host != m_host
        || (id.isEmpty() || m_sessionId.isEmpty() ? name != m_session : id != m_sessionId);
    if (changed && !canLeaveDetails()) return;
    if (changed) closeInlineDetails();
    m_project = project; m_host = host; m_session = name; m_sessionId = id; m_sessions = sessions;
    if (changed) { ++m_generation; m_selected.clear(); m_detail = {}; m_stage.clear(); m_showOlder = false; }
    refresh();
}

void TaskInspector::showEvent(QShowEvent *event) {
    QWidget::showEvent(event); m_board->setInspectorVisible(true); refresh();
}

void TaskInspector::hideEvent(QHideEvent *event) {
    endListDrag(); QWidget::hideEvent(event); m_board->setInspectorVisible(false);
}

void TaskInspector::selectTask(const QString &uuid) {
    if (m_selected != uuid && !canLeaveDetails()) return;
    if (m_selected != uuid) closeInlineDetails();
    const auto board = m_board->displayBoard();
    for (const auto &task : board.tasks) if (task.uuid == uuid) {
        m_stage = task.stage;
        if (!TaskBoard::keptFor(task, m_host, m_session, m_sessionId)) m_board->setOnlyThisSession(false);
        if (task.stage == QLatin1String("done") && QDateTime::currentMSecsSinceEpoch() - task.stageAt > TaskBoard::RecentDoneMs) m_showOlder = true;
        break;
    }
    if (m_selected != uuid) {
        m_selected = uuid; m_inlineRequested.clear(); ++m_generation; m_historyVisible = 8; m_historyBefore = {};
        const QSignalBlocker blocker(m_detailTabs); m_detailTabs->setCurrentIndex(0);
    }
    refresh(); fetch();
}

void TaskInspector::refresh() {
    if (!m_cards) return;
    const auto board = m_board->displayBoard(); const auto colors = TaskColors::of(m_dark);
    if (m_form) m_form->updateSessionLabels(m_sessions);
    const TaskBoard::View view{m_host, m_session, m_board->onlyThisSession(), QDateTime::currentMSecsSinceEpoch(), m_sessionId};
    if (m_stage.isEmpty() && !board.projectId.isEmpty()) {
        m_stage = m_board->selectedStage();
        if (!board.count(m_stage, view)) for (const auto &stage : TaskBoard::stages())
            if (board.count(stage, view)) { m_stage = stage; break; }
    }
    // Follow a selected task's recorded stage change while keeping its details open.
    for (const auto &task : board.tasks) if (task.uuid == m_selected) m_stage = task.stage;
    for (const auto &stage : TaskBoard::stages()) {
        auto *tile = m_tiles.value(stage); tile->setColors(colors); tile->setCount(board.count(stage, view)); tile->setChecked(stage == m_stage);
    }
    auto rows = board.rows(m_stage, view);
    const auto older = m_stage == QLatin1String("done") ? board.rows(m_stage, view, true) : QList<TaskItem>();
    if (m_showOlder) rows += older;
    if (m_form && !m_selected.isEmpty() && std::none_of(rows.cbegin(), rows.cend(), [this](const TaskItem &t) { return t.uuid == m_selected; }))
        for (const auto &task : board.tasks) if (task.uuid == m_selected) { rows.prepend(task); break; }
    const int total = rows.size();
    QList<TaskItem> shown = rows.mid(0, VisibleTasks);
    // An explicit selection beyond the window replaces its last card, never
    // growing the retained widget set past the bound.
    if (!m_selected.isEmpty() && std::none_of(shown.begin(), shown.end(), [this](const TaskItem &t) { return t.uuid == m_selected; }))
        for (const auto &task : rows) if (task.uuid == m_selected) { if (shown.size() == VisibleTasks) shown.removeLast(); shown.append(task); break; }
    QSet<QString> keep; int index = 0; bool selectedShown = false;
    for (const auto &task : shown) {
        auto *row = m_rows.value(task.uuid);
        if (!row) {
            row = new TaskRow(m_listPage); row->setCompact(true); m_rows.insert(task.uuid, row);
            row->setObjectName(QStringLiteral("taskInspectorRow:") + task.id);
            connect(row, &TaskRow::opened, this, [this, row] {
                selectTask(row->task().uuid);
            });
            connect(row, &TaskRow::actionRequested, this, [this, row](TaskAction action) { m_board->actOnTask(row->task(), action); });
            connect(row, &TaskRow::dragStarted, this, [this, row](const QPoint &hotSpot) { m_board->dragTask(row, hotSpot); });
        }
        row->setTask(task, m_board->rowState(task), colors, view.now);
        row->setProperty("expanded", task.uuid == m_selected);
        m_cards->removeWidget(row); m_cards->insertWidget(index++, row); row->show(); keep.insert(task.uuid);
        if (task.uuid == m_selected) selectedShown = true;
    }
    for (auto it = m_rows.begin(); it != m_rows.end();) {
        if (keep.contains(it.key())) { ++it; continue; }
        auto *row = it.value(); m_cards->removeWidget(row); m_board->retireTaskRow(row); it = m_rows.erase(it);
    }
    if (!selectedShown && !m_selected.isEmpty()) { m_selected.clear(); m_detail = {}; ++m_generation; }
    m_scope->setText(m_project.isEmpty() ? tr("Open a project session to see tasks")
        : total > VisibleTasks ? tr("%1 of %2 tasks").arg(VisibleTasks).arg(total)
        : total == 1 ? tr("1 task") : tr("%1 tasks").arg(total));
    m_older->setVisible(!older.isEmpty()); m_older->setText(m_showOlder ? tr("Hide older") : tr("Show older (%1)").arg(older.size()));
    updateFilters();
    renderDetail(); if (isVisible()) fetch();
    if (m_overList) QMetaObject::invokeMethod(this, [this] {
        if (m_overList) followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt));
    }, Qt::QueuedConnection);
}

void TaskInspector::resizeEvent(QResizeEvent *event) {
    QWidget::resizeEvent(event); updateFilters();
}

void TaskInspector::updateFilters() {
    if (!m_filters) return;
    int required = (m_tiles.size() - 1) * 4;
    for (auto *tile : std::as_const(m_tiles)) required += tile->expandedWidth();
    for (auto *tile : std::as_const(m_tiles)) tile->setCompact(m_filters->width() < required);
}

QList<TaskRow *> TaskInspector::shownRows() const {
    QList<TaskRow *> rows;
    for (int i = 0; i < m_cards->count(); ++i)
        if (auto *row = qobject_cast<TaskRow *>(m_cards->itemAt(i)->widget()); row && !row->isHidden()) rows << row;
    return rows;
}

int TaskInspector::dropGap(const QString &uuid, const QPoint &point) const {
    const auto rows = shownRows(); int gap = rows.size(), from = -1;
    for (int i = 0; i < rows.size(); ++i) {
        if (rows[i]->task().uuid == uuid) from = i;
        if (gap == rows.size() && point.y() < rows[i]->y() + rows[i]->height() / 2) gap = i;
    }
    return from >= 0 && (gap == from || gap == from + 1) ? -1 : gap;
}

void TaskInspector::followDrag(const TaskDrag &drag, const QPoint &point) {
    const auto rows = shownRows();
    if (!m_board->canReorderTask(drag, m_stage) || rows.isEmpty()) { endListDrag(); return; }
    m_drag = drag; m_dragAt = m_listPage->mapTo(m_scroll->viewport(), point); m_overList = true;
    const int gap = dropGap(drag.uuid, point);
    if (gap < 0) m_dropLine->hide();
    else {
        const auto *edge = gap < rows.size() ? rows[gap] : rows.last();
        const int y = edge->y() + (gap < rows.size() ? 0 : edge->height());
        m_dropLine->setGeometry(8, y - 1, m_listPage->width() - 16, 2); m_dropLine->show(); m_dropLine->raise();
    }
    const int height = m_scroll->viewport()->height();
    m_scrollStep = m_dragAt.y() < 28 ? -8 : m_dragAt.y() > height - 28 ? 8 : 0;
    if (!m_scrollStep) m_autoScroll.stop(); else if (!m_autoScroll.isActive()) m_autoScroll.start();
}

void TaskInspector::endListDrag() { m_autoScroll.stop(); m_overList = false; m_scrollStep = 0; m_dropLine->hide(); }

bool TaskInspector::eventFilter(QObject *watched, QEvent *event) {
    const auto type = event->type();
    if (type != QEvent::DragEnter && type != QEvent::DragMove && type != QEvent::DragLeave && type != QEvent::Drop)
        return QWidget::eventFilter(watched, event);
    if (watched == m_listPage) {
        if (type == QEvent::DragLeave) { endListDrag(); return true; }
        auto *drop = static_cast<QDropEvent *>(event); const auto drag = TaskDrag::from(drop->mimeData()); const auto rows = shownRows();
        if (!drag || rows.isEmpty() || !m_board->canReorderTask(*drag, m_stage)) { endListDrag(); drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) {
            const int gap = dropGap(drag->uuid, drop->position().toPoint()); endListDrag();
            if (gap >= 0) m_board->reorderTask(*drag, gap < rows.size() ? rows[gap]->task().uuid : rows.last()->task().uuid, gap < rows.size());
        } else followDrag(*drag, drop->position().toPoint());
        return true;
    }
    for (auto it = m_tiles.cbegin(); it != m_tiles.cend(); ++it) {
        if (it.value() != watched) continue;
        auto *tile = it.value();
        if (type == QEvent::DragLeave) { tile->setDropTarget(false); return true; }
        auto *drop = static_cast<QDropEvent *>(event); const auto drag = TaskDrag::from(drop->mimeData());
        const bool takes = drag && m_board->canStageTask(*drag, it.key());
        tile->setDropTarget(takes && type != QEvent::Drop);
        if (!takes) { drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) m_board->stageTask(*drag, it.key());
        return true;
    }
    return QWidget::eventFilter(watched, event);
}

void TaskInspector::fetch() {
    if (m_request || m_selected.isEmpty() || m_project.isEmpty() || !isVisible()) return;
    setProperty("requestGeneration", QVariant::fromValue(m_generation));
    m_request = m_client->requestTask({"get", m_selected, "--project", m_project});
}

QJsonObject TaskInspector::labels(QJsonObject task) const {
    const auto resolve = [this](QJsonObject key) {
        const QString id = key.value("session_id").toString();
        if (id.isEmpty() || key.value("host").isNull()) return key;
        for (const auto &candidate : m_sessions) if (candidate.host == key.value("host").toString() && candidate.id == id) {
            key["session"] = candidate.name; break;
        }
        return key;
    };
    for (const auto *key : {"target", "session", "author"}) if (task.value(key).isObject()) task[key] = resolve(task.value(key).toObject());
    auto log = task.value("log").toArray();
    for (auto it = log.begin(); it != log.end(); ++it) {
        auto entry = it->toObject();
        for (const auto *key : {"actor", "session"}) if (entry.value(key).isObject()) entry[key] = resolve(entry.value(key).toObject());
        *it = entry;
    }
    task["log"] = log; return task;
}

const TaskSession *TaskInspector::executor() const {
    const auto task = TaskBoard::item(labels(m_detail));
    const bool sent = !task.sessionName.isEmpty();
    const QString host = sent ? task.sessionHost : task.targetHost;
    const QString id = sent ? task.sessionId : task.targetId;
    const QString name = sent ? task.sessionName : task.targetSession;
    for (const auto &candidate : m_sessions) if (candidate.host == host
        && (id.isEmpty() ? candidate.name == name : candidate.id == id)) return &candidate;
    return nullptr;
}

void TaskInspector::renderDetail() {
    if (!m_activity) return;
    // Keep the previous document and fixed controls during an asynchronous
    // selection read. Publish the new task once, when its exact reply arrives.
    if (!m_selected.isEmpty() && m_detail.value("uuid").toString() != m_selected) {
        m_open->setEnabled(false); m_edit->setEnabled(false); m_assignment->setEnabled(false);
        m_note->setEnabled(false); m_saveNote->setEnabled(false); m_detailTabs->setEnabled(false);
        for (auto *button : std::as_const(m_commandButtons)) button->setEnabled(false);
        m_commandsKey.clear(); m_state->setText(tr("Loading selected task…")); return;
    }
    const auto data = labels(m_detail); auto task = TaskBoard::item(data);
    for (const auto &listed : m_board->board().tasks) if (listed.uuid == task.uuid) { task.queue = listed.queue; break; }
    m_open->setEnabled(!m_detail.isEmpty() && executor()); m_edit->setEnabled(!m_detail.isEmpty());
    m_detailTabs->setEnabled(!m_detail.isEmpty()); m_moreDetail->hide(); m_moreDetail->setEnabled(true);
    m_latestHistory->setVisible(!m_detail.isEmpty() && m_detailTabs->currentIndex() == 2 && !m_historyBefore.isEmpty());
    renderCommands(task);
    m_assignment->setVisible(!m_detail.isEmpty() && m_detailTabs->currentIndex() == 0 && !m_form);
    const auto assigned = task.targetSession.isEmpty() ? tr("Any session") : TaskBoard::sessionLabel(task.targetHost, task.targetSession);
    m_assignment->setText(tr("Assigned to: %1").arg(assigned));
    m_assignment->setEnabled(task.writable && (task.stage == QLatin1String("todo") || task.stage == QLatin1String("backlog")));
    m_note->setEnabled(!m_detail.isEmpty() && task.writable && !m_noteBusy); m_saveNote->setEnabled(m_note->isEnabled());
    const auto noteKey = m_selected.isEmpty() ? QString() : QStringLiteral("tasks/noteDraft/%1/%2").arg(m_project, m_selected);
    if (m_noteKey != noteKey) { m_noteKey.clear(); m_note->setText(QSettings().value(noteKey).toString()); m_noteKey = noteKey; m_noteError->hide(); }
    if (m_form && m_detailTabs->currentIndex() == 0) { m_content->setCurrentWidget(m_formScroll); return; }
    const auto empty = [this](const QString &text) {
        m_activity->setTaskJournal({}, {}); m_emptyDetail->setText(text); m_content->setCurrentWidget(m_emptyDetail);
    };
    if (m_detail.isEmpty()) {
        m_state->setText(tr("Task details")); m_state->setToolTip({});
        empty(m_selected.isEmpty() ? tr("Select a task above to see its details.") : tr("Loading task…")); return;
    }
    const auto log = data.value("log").toArray();
    const bool started = data.value("start_confirmed").toBool();
    QString context;
    if (!task.sessionName.isEmpty()) {
        const QString worker = TaskBoard::sessionLabel(task.sessionHost, task.sessionName);
        context = task.stage == QLatin1String("active") && !started ? tr("Dispatched to %1 · start unconfirmed").arg(worker)
            : task.stage == QLatin1String("active") ? tr("Executor: %1").arg(worker) : tr("Last executor: %1").arg(worker);
    } else context = tr("Assigned: %1").arg(task.targetSession.isEmpty() ? tr("Any session") : TaskBoard::sessionLabel(task.targetHost, task.targetSession));
    if (task.stage == QLatin1String("todo")) context += tr(" · #%1 in queue").arg(task.queue);
    m_state->setText(context); m_state->setToolTip(QStringLiteral("%1 · %2\n%3").arg(task.id, TaskBoard::stageLabel(task.stage), context));
    QJsonArray entries;
    switch (m_detailTabs->currentIndex()) {
    case 0: {
        entries.append(journalEntry("description", data.value("author").toObject(), tr("Description"), task.text, data.value("created_at").toDouble()));
        const int files = data.value("attachments").toArray().size();
        m_moreDetail->setVisible(files > 0); m_moreDetail->setText(tr("Open attachments (%1)").arg(files));
        break;
    }
    case 1: {
        if (task.summary.isEmpty() && !task.cancelledAt) { empty(tr("No result reported yet.")); return; }
        QJsonObject record;
        const QString kind = task.cancelledAt ? QStringLiteral("cancelled") : QStringLiteral("report");
        for (const auto &value : log) {
            const auto entry = value.toObject();
            if (entry.value("kind") == kind) record = entry;
        }
        const auto actor = record.contains("actor") ? record.value("actor").toObject()
            : QJsonObject{{"kind", "agent"}, {"session", task.sessionName}, {"host", task.sessionHost}};
        entries.append(journalEntry("result", actor, task.cancelledAt ? tr("Cancelled") : tr("Report"),
            task.cancelledAt ? task.cancelReason : task.summary, record.value("at").toDouble()));
        break;
    }
    case 2: {
        const int first = qMax(0, int(log.size()) - m_historyVisible);
        if (first > 0) { m_moreDetail->show(); m_moreDetail->setText(tr("Earlier events (%1)").arg(first)); }
        else if (data.value("history_end").toInt() > log.size()) { m_moreDetail->show(); m_moreDetail->setText(tr("Earlier events…")); }
        for (int i = first; i < log.size(); ++i) {
            const auto entry = log.at(i).toObject(); const auto kind = entry.value("kind").toString();
            const bool message = kind == QLatin1String("comment") || kind == QLatin1String("report");
            entries.append(journalEntry(QString::number(data.value("history_end").toInt() - log.size() + i), entry.value("actor").toObject(),
                kind == QLatin1String("report") ? tr("Report") : tr("Comment"),
                message ? entry.value("text").toString() : taskHistoryAction(entry), entry.value("at").toDouble(), !message));
        }
        if (log.isEmpty()) { empty(tr("No history yet.")); return; }
        break;
    }
    }
    m_content->setCurrentWidget(m_activity);
    m_activity->setTaskJournal(QStringLiteral("task\n%1\n%2\n%3\n%4").arg(m_project, task.uuid).arg(m_detailTabs->currentIndex()).arg(m_historyVisible), entries,
        m_detailTabs->currentIndex() == 2 && m_historyVisible == 8);
}

bool TaskInspector::canLeaveDetails() {
    if (m_noteBusy || (m_form && m_form->isBusy())) return false;
    if (!m_form || !m_form->hasEdits()) return true;
    return QMessageBox::question(this, tr("Unsaved task changes"), tr("Discard the unsaved description, attachment and assignment changes?"),
        QMessageBox::Discard | QMessageBox::Cancel, QMessageBox::Cancel) == QMessageBox::Discard;
}

void TaskInspector::closeInlineDetails() {
    if (!m_form) return;
    m_content->removeWidget(m_formScroll); m_formScroll->hide(); m_formScroll->deleteLater(); m_form = nullptr; m_formScroll = nullptr; m_commandsKey.clear();
}

void TaskInspector::openInlineDetails() {
    if (m_detail.isEmpty() || m_detail.value("uuid").toString() != m_selected) return;
    m_inlineRequested.clear();
    m_detailTabs->setCurrentIndex(0);
    if (!m_form) {
        const auto task = TaskBoard::item(labels(m_detail));
        m_form = new TaskForm(labels(m_detail), m_sessions,
            [this, task](const TaskTarget &target) { return m_board->taskSendBlocker(task, target); }, m_dark, m_content, true);
        m_formScroll = new QScrollArea; m_formScroll->setObjectName("taskInlineEditor"); m_formScroll->setWidgetResizable(true);
        m_formScroll->setFrameShape(QFrame::NoFrame); m_formScroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
        m_formScroll->setWidget(m_form); m_content->addWidget(m_formScroll);
        connect(m_form, &TaskForm::dismissRequested, this, [this] { if (canLeaveDetails()) { closeInlineDetails(); renderDetail(); } });
        QPointer<TaskForm> guard(m_form);
        connect(m_form, &TaskForm::saveRequested, this, [this, task, guard](const TaskEdits &edits) {
            m_board->saveTaskEdits(task, edits, [this, guard] {
                if (guard && guard == m_form) { closeInlineDetails(); m_detail = {}; ++m_generation; renderDetail(); fetch(); }
            }, [guard](const QString &error) { if (guard) guard->setError(error); });
        });
    }
    m_content->setCurrentWidget(m_formScroll); m_assignment->hide(); m_commandsKey.clear(); renderCommands(TaskBoard::item(labels(m_detail)));
}

void TaskInspector::renderCommands(const TaskItem &task) {
    const auto state = m_board->rowState(task);
    QString key = task.uuid + QLatin1Char('/') + task.stage + (m_form ? QStringLiteral("/editing") : QString());
    for (auto i = state.blocked.cbegin(); i != state.blocked.cend(); ++i) key += QString::number(i.key()) + i.value();
    if (m_commandsKey == key) return;
    m_commandsKey = key;
    auto actions = m_detail.isEmpty() ? QList<TaskAction>() : TaskBoard::actions(task.stage); actions.removeAll(TaskAction::Edit);
    const auto primary = TaskBoard::primaryAction(task.stage);
    if (!m_detail.isEmpty() && !actions.contains(primary)) actions << primary;
    for (auto it = m_commandButtons.cbegin(); it != m_commandButtons.cend(); ++it) {
        const auto action = TaskAction(it.key()); auto *button = it.value();
        button->setVisible(actions.contains(action));
        button->setIcon(workspaceIcon(TaskBoard::actionIcon(action), action == primary ? TaskColors::of(m_dark).accent : TaskColors::of(m_dark).muted));
        const auto why = m_form ? tr("Save or discard description changes first") : state.blocked.value(int(action));
        button->setToolTip(why.isEmpty() ? TaskBoard::actionLabel(action) : why); button->setEnabled(why.isEmpty());
        button->setProperty("primaryAction", action == primary);
    }
}

void TaskInspector::setContentScale(double scale) { m_activity->setContentScale(scale); }

void TaskInspector::setTheme(bool dark) {
    m_dark = dark; if (!m_cards) return;
    const auto colors = TaskColors::of(dark);
    setWorkspaceStyle(this, QStringLiteral("#taskInspector{background:%2;color:%1;}"
        "#taskInspector QLabel{color:%1;}QScrollArea,#taskInspectorCards{background:%2;border:0;}"
        "#taskDetailsPane{border-top:1px solid %3;}"
        "#taskInspectorState,#taskDetailEmpty{color:%4;font-size:11px;}"
        "#taskDetailTabs::tab{font-size:12px;color:%4;padding:6px 10px;border:0;border-bottom:2px solid transparent;}"
        "#taskDetailTabs::tab:selected{color:%1;border-bottom-color:%5;}"
        "#taskInspectorSplit::handle{background:%3;}"
        "#taskDetailMore,#taskHistoryLatest{color:%5;border:0;padding:4px;}" )
        .arg(colors.fg.name(), colors.surface.name(), colors.border.name(), colors.muted.name(), colors.accent.name()) + workspaceScrollbars(dark));
    m_activity->setTheme(dark);
    m_commandsKey.clear();
    setWorkspaceStyle(m_dropLine, QStringLiteral("background:%1;border-radius:1px;").arg(colors.accent.name()));
    m_saveNote->setIcon(workspaceIcon("task-check", colors.muted));
    m_noteError->setStyleSheet(QStringLiteral("color:%1;").arg(QStringLiteral("#e06c75")));
    m_open->setIcon(workspaceIcon("external", colors.muted)); m_edit->setIcon(workspaceIcon("task-list", colors.muted));
    for (auto *button : {m_open, m_edit}) setWorkspaceStyle(button,
        QStringLiteral("QPushButton{border:0;border-radius:5px;background:transparent;}QPushButton:hover{background:%1;}").arg(colors.hover.name()));
    refresh();
}
