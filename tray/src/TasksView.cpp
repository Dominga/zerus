#include "Settings.h"
#include "TasksView.h"
#include <QUuid>
#include "TaskStageFilter.h"
#include "TaskContent.h"
#include "TaskMessageOutbox.h"
#include "AttachmentViewer.h"
#include "AttachmentViewer.h"
#include "HgsClient.h"
#include "TaskDialogs.h"
#include "MessageComposer.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QAbstractButton>
#include <QDateTime>
#include <QDrag>
#include <QDropEvent>
#include <QFrame>
#include <QHBoxLayout>
#include <QJsonDocument>
#include <QLabel>
#include <QPainter>
#include <QPushButton>
#include <QScrollArea>
#include <QScrollBar>
#include <QSet>
#include <QSettings>
#include <QToolButton>
#include <QVBoxLayout>

namespace {
QColor alpha(QColor color, double a) { color.setAlphaF(a); return color; }
}

TaskTabBadge::TaskTabBadge(QWidget *parent) : QWidget(parent) { setObjectName("taskTabBadge"); setCounts(0, 0); }

void TaskTabBadge::setCounts(int review, int todo) {
    m_review = review; m_todo = todo; setProperty("review", review); setProperty("todo", todo);
    setAccessibleName(tr("%1 to review, %2 to do").arg(review).arg(todo));
    // A tab button keeps the size it has; the owner relayouts the tab for it.
    resize(sizeHint()); setVisible(review > 0 || todo > 0); update();
}

void TaskTabBadge::setTheme(bool dark) { m_dark = dark; update(); }

QSize TaskTabBadge::sizeHint() const {
    QFont small = font(); small.setPixelSize(10); small.setBold(true); const QFontMetrics metrics(small);
    int width = 0;
    for (const int count : {m_review, m_todo}) if (count) width += qMax(16, metrics.horizontalAdvance(QString::number(count)) + 10) + 4;
    return {qMax(0, width), 16};
}

void TaskTabBadge::paintEvent(QPaintEvent *) {
    QPainter p(this); p.setRenderHint(QPainter::Antialiasing);
    QFont small = font(); small.setPixelSize(10); small.setBold(true); p.setFont(small); const QFontMetrics metrics(small);
    int x = 0;
    const auto pill = [&](int count, const QColor &fill, const QColor &text) {
        if (!count) return;
        const QString number = QString::number(count); const int width = qMax(16, metrics.horizontalAdvance(number) + 10);
        p.setPen(Qt::NoPen); p.setBrush(fill); p.drawRoundedRect(QRectF(x, (height() - 16) / 2.0, width, 16), 8, 8);
        p.setPen(text); p.drawText(QRect(x, (height() - 16) / 2, width, 16), Qt::AlignCenter, number); x += width + 4;
    };
    pill(m_review, QColor(m_dark ? "#ffda76" : "#f4ce65"), QColor("#392900"));
    pill(m_todo, QColor(m_dark ? "#303942" : "#e5ebef"), QColor(m_dark ? "#a2adbc" : "#627082"));
}

TasksView::TasksView(HgsClient *client, QWidget *parent) : QWidget(parent), m_client(client) {
    setObjectName("tasksView");
    auto *layout = new QVBoxLayout(this); layout->setContentsMargins(0, 12, 0, 0); layout->setSpacing(0);
    auto *tiles = new QHBoxLayout; tiles->setSpacing(8);
    for (const auto &stage : TaskBoard::stages()) {
        auto *tile = new TaskTile(stage, this); m_tiles.insert(stage, tile); tiles->addWidget(tile, 1);
        tile->setAcceptDrops(true); tile->installEventFilter(this);
        connect(tile, &QAbstractButton::clicked, this, [this, stage] { selectStage(stage); });
    }
    const auto square = [this](const QString &name) {
        auto *button = new QToolButton(this); button->setObjectName(name); button->setFixedSize(34, 34); button->setIconSize(QSize(20, 20));
        button->setCursor(Qt::PointingHandCursor); return button;
    };
    m_only = square("taskOnlySession"); m_only->setCheckable(true);
    m_only->setChecked(Settings::value("tasks/onlySession").toBool());
    connect(m_only, &QToolButton::toggled, this, [this](bool only) {
        Settings::set("tasks/onlySession", only);
        const auto colors = TaskColors::of(m_dark); m_only->setIcon(workspaceIcon(QStringLiteral("task-funnel"), only ? colors.accent : colors.muted));
        render();
    });
    m_settings = square("taskSettings");
    connect(m_settings, &QToolButton::clicked, this, &TasksView::openSettings);
    tiles->addWidget(m_only); tiles->addWidget(m_settings);
    layout->addLayout(tiles); layout->addSpacing(14);
    m_errorLabel = new QLabel; m_errorLabel->setObjectName("tasksError"); m_errorLabel->setWordWrap(true); m_errorLabel->setTextFormat(Qt::PlainText);
    m_errorLabel->hide(); layout->addWidget(m_errorLabel);
    m_damaged = new QLabel; m_damaged->setObjectName("tasksDamaged"); m_damaged->setWordWrap(true); m_damaged->setTextFormat(Qt::PlainText);
    m_damaged->hide(); layout->addWidget(m_damaged);
    m_scroll = new QScrollArea; m_scroll->setObjectName("taskList"); m_scroll->setWidgetResizable(true); m_scroll->setFrameShape(QFrame::NoFrame);
    // The list's surface holds only its rows; the spare height below stays the page's.
    m_listPage = new QWidget; m_listPage->setObjectName("taskListPage");
    auto *pageLayout = new QVBoxLayout(m_listPage); pageLayout->setContentsMargins(0, 0, 0, 0); pageLayout->setSpacing(0);
    m_listHost = new QWidget; m_listHost->setObjectName("taskListHost"); pageLayout->addWidget(m_listHost); pageLayout->addStretch();
    // The page takes To do rows dropped on it; the line shows where one would go.
    m_listPage->setAcceptDrops(true); m_listPage->installEventFilter(this);
    m_dropLine = new QFrame(m_listPage); m_dropLine->setObjectName("taskDropLine"); m_dropLine->hide();
    m_autoScroll.setInterval(30);
    connect(&m_autoScroll, &QTimer::timeout, this, [this] {
        auto *bar = m_scroll->verticalScrollBar(); bar->setValue(bar->value() + m_scrollStep);
        // The rows moved under the pointer.
        followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt));
    });
    m_list = new QVBoxLayout(m_listHost); m_list->setContentsMargins(0, 0, 0, 0); m_list->setSpacing(0);
    m_empty = new QLabel; m_empty->setObjectName("taskEmpty"); m_empty->setAlignment(Qt::AlignCenter); m_empty->setMargin(18);
    m_list->addWidget(m_empty);
    m_scroll->setWidget(m_listPage); layout->addWidget(m_scroll, 1);
    m_older = new QPushButton; m_older->setObjectName("showOlderTasks"); m_older->setFlat(true); m_older->setCursor(Qt::PointingHandCursor); m_older->hide();
    connect(m_older, &QPushButton::clicked, this, [this] { m_showOlder = !m_showOlder; render(); });
    layout->addWidget(m_older, 0, Qt::AlignLeft);
    m_composer = new MessageComposer(this); m_composer->setObjectName("taskComposer"); m_composer->setTaskMode(true);
    connect(m_composer, &MessageComposer::sendRequested, this, &TasksView::addTask);
    layout->addWidget(m_composer);
    connect(m_client, &HgsClient::taskReady, this, [this](quint64 id, const QJsonObject &result) {
        const auto pending = m_pending.take(id); if (pending.first) pending.first(result);
    });
    connect(m_client, &HgsClient::taskFailed, this, [this](quint64 id, const QString &error) {
        if (!m_pending.contains(id)) return;
        const auto pending = m_pending.take(id); if (pending.second) pending.second(error);
    });
    m_timer.setSingleShot(true); connect(&m_timer, &QTimer::timeout, this, &TasksView::refresh);
    setTheme(true);
}

void TasksView::setContext(const QString &projectId, const QString &projectName, const QString &host, const QString &session,
                           const QString &sessionKey, const QList<TaskSession> &sessions) {
    const bool project = projectId != m_projectId;
    // The window passes the context on every update; only a change is drawn again.
    if (!project && projectName == m_projectName && host == m_host && session == m_session && sessionKey == m_sessionKey && sessions == m_sessions) return;
    m_projectName = projectName; m_host = host; m_session = session; m_sessions = sessions;
    for (auto *dialog : findChildren<TaskDialog *>()) dialog->updateSessionLabels(sessions);
    if (project) {
        m_projectId = projectId; ++m_generation; m_board = {}; m_loaded = false; m_failing = false; m_error.clear(); m_showOlder = false;
        m_stage = QStringLiteral("todo");
        for (auto *row : std::as_const(m_rows)) retire(row);
        m_rows.clear();
        emit countsChanged(0, 0);   // until the new project's listing arrives
    }
    m_sessionKey = sessionKey; applyComposerKey();
    m_composer->setAvailability(!m_projectId.isEmpty() && !m_session.isEmpty(), m_session.isEmpty() ? tr("Open a session to add tasks for it.")
        : tr("This session is in no project; tasks belong to a project."));
    render();
    if (project) refresh();
}

void TasksView::setPolling(bool polling) {
    m_polling = polling;
    if (!polling) m_timer.stop(); else schedule();
}

void TasksView::setRefreshIntervals(int visibleMs, int hiddenMs) { m_visibleMs = visibleMs; m_hiddenMs = hiddenMs; schedule(); }

void TasksView::setInspectorVisible(bool visible) {
    if (visible == m_inspectorVisible) return;
    m_inspectorVisible = visible;
    if (visible) refresh(); else schedule();
}

void TasksView::moveControlsTo(QBoxLayout *toolbar) {
    toolbar->addWidget(m_only); toolbar->addWidget(m_settings);
}

bool TasksView::onlyThisSession() const { return m_only->isChecked(); }
void TasksView::setOnlyThisSession(bool only) { m_only->setChecked(only); }

void TasksView::showEvent(QShowEvent *event) { QWidget::showEvent(event); applyComposerKey(); if (m_loaded) refresh(); }

void TasksView::hideEvent(QHideEvent *event) { QWidget::hideEvent(event); applyComposerKey(); schedule(); }

// The input holds the open session's task draft only while the tab shows: a hidden
// tab keeps no enabled field in the window.
void TasksView::applyComposerKey() {
    const QString key = isVisible() && !m_sessionKey.isEmpty() ? QStringLiteral("task-draft\n") + m_sessionKey : QString();
    if (key != m_composer->sessionKey()) m_composer->setSessionKey(key);
}

// A project hgs cannot list is retried at the slow pace until a listing succeeds.
void TasksView::schedule() {
    if (m_polling && !m_projectId.isEmpty() && !m_listing) m_timer.start((isVisible() || m_inspectorVisible) && !m_failing ? m_visibleMs : m_hiddenMs);
}

QStringList TasksView::withProject(QStringList arguments) const { return arguments << QStringLiteral("--project") << m_projectId; }

void TasksView::request(const QStringList &arguments, const QString &input, std::function<void(const QJsonObject &)> done,
                        std::function<void(const QString &)> failed) {
    if (!failed) failed = [this](const QString &error) { emit notice(error, true); refresh(); };
    m_pending.insert(m_client->requestTask(arguments, input), {std::move(done), std::move(failed)});
}

void TasksView::refresh() {
    m_timer.stop();
    if (m_projectId.isEmpty()) return;
    // A mutation may finish while an older list snapshot is in flight. One
    // follow-up read must observe it even when periodic polling is disabled.
    if (m_listing) { m_refreshPending = true; return; }
    m_refreshPending = false;
    const auto generation = m_generation; m_listing = generation + 1;
    request({QStringLiteral("list"), QStringLiteral("--project"), m_projectId}, {},
        [this, generation](const QJsonObject &listing) {
            m_listing = 0;
            if (m_refreshPending || generation != m_generation) { refresh(); return; }
            received(listing, generation);
        },
        [this, generation](const QString &error) {
            m_listing = 0;
            if (m_refreshPending || generation != m_generation) return refresh();
            // What is not readable now is not shown as current: no rows, no counts.
            m_error = error; m_failing = true; m_board = {}; render(); emit countsChanged(0, 0); schedule();
        });
}

void TasksView::received(const QJsonObject &listing, quint64 generation) {
    if (generation != m_generation) return refresh();   // the project changed meanwhile
    m_error.clear(); m_failing = false; m_board = TaskBoard::fromListing(listing);
    if (!m_loaded) { m_loaded = true; m_stage = m_board.count("review", view()) ? QStringLiteral("review") : QStringLiteral("todo"); }
    render();
    int review = 0, todo = 0;
    for (const auto &task : std::as_const(m_board.tasks)) { review += task.stage == QLatin1String("review"); todo += task.stage == QLatin1String("todo"); }
    emit countsChanged(review, todo);
    schedule();
}

TaskBoard::View TasksView::view() const {
    const auto *current = session(m_host, m_session);
    return {m_host, m_session, m_only->isChecked(), QDateTime::currentMSecsSinceEpoch(), current ? current->id : QString()};
}

TaskBoard TasksView::projectedBoard() const {
    auto board = m_board;
    for (auto &task : board.tasks) {
        const auto resolve = [this](const QString &host, const QString &id, QString &name, bool &known) {
            if (id.isEmpty()) return;
            const TaskSession *found = nullptr;
            for (const auto &candidate : m_sessions) {
                if (candidate.host != host || candidate.id != id) continue;
                if (found) { known = false; return; }
                found = &candidate;
            }
            if (found) { name = found->name; known = true; }
            else known = false;
        };
        resolve(task.targetHost, task.targetId, task.targetSession, task.targetKnown);
        resolve(task.sessionHost, task.sessionId, task.sessionName, task.sessionKnown);
        bool authorKnown = true; resolve(task.authorHost, task.authorId, task.author, authorKnown);
    }
    return board;
}

const TaskSession *TasksView::session(const QString &host, const QString &name) const {
    for (const auto &session : m_sessions) if (session.host == host && session.name == name) return &session;
    return nullptr;
}

// Why Send now cannot paste into the session the task would go to, as hgs would say it.
QString TasksView::sendBlocker(const TaskItem &task) const {
    if (task.uuid == m_descriptionDraft && !task.uuid.isEmpty()) return tr("Save the description before starting this task");
    if (task.targetKind == QLatin1String("new_session")) {
        if (task.targetId.isEmpty() || task.targetMachine.isEmpty()) return tr("Choose a parent with a confirmed computer and session identity");
        return m_executorLaunchBlockers.value(task.uuid);
    }
    const bool any = task.targetSession.isEmpty();
    const QString host = any ? m_host : task.targetHost, name = any ? m_session : task.targetSession;
    if (name.isEmpty()) return tr("Open the session to send this task to");
    if (!any && !task.targetKnown) return task.targetId.isEmpty()
        ? tr("%1 is on a computer this one has not verified").arg(TaskBoard::sessionLabel(host, name))
        : tr("%1 no longer has a confirmed session identity — choose another session").arg(TaskBoard::sessionLabel(host, name));
    // A session on another computer takes the task there; hgs refuses when it cannot be reached.
    const auto *found = session(host, name);
    if (!found) return tr("%1 no longer exists — choose another session").arg(TaskBoard::sessionLabel(host, name));
    return found->blocker;
}

void TasksView::selectStage(const QString &stage) {
    if (stage != m_stage) m_showOlder = false;
    m_stage = stage; render();
}

void TasksView::render() {
    const auto v = view(); const auto colors = TaskColors::of(m_dark);
    const auto board = projectedBoard();
    for (const auto &stage : TaskBoard::stages()) {
        auto *tile = m_tiles.value(stage); tile->setCount(board.count(stage, v)); tile->setChecked(stage == m_stage);
    }
    m_errorLabel->setText(tr("Tasks could not be read: %1").arg(m_error)); m_errorLabel->setVisible(!m_error.isEmpty());
    m_damaged->setText(m_board.damaged == 1 ? tr("1 task file of %1 could not be read.").arg(m_projectName)
                                            : tr("%1 task files of %2 could not be read.").arg(m_board.damaged).arg(m_projectName));
    m_damaged->setVisible(m_board.damaged > 0 && m_error.isEmpty());
    const bool done = m_stage == QLatin1String("done");
    QList<TaskItem> rows = m_error.isEmpty() ? board.rows(m_stage, v) : QList<TaskItem>();
    const auto older = done && m_error.isEmpty() ? board.rows(m_stage, v, true) : QList<TaskItem>();
    if (m_showOlder) rows += older;
    QSet<QString> shown; int index = 0;
    for (const auto &task : std::as_const(rows)) {
        auto *row = m_rows.value(task.uuid);
        if (!row) {
            row = new TaskRow(m_listHost); m_rows.insert(task.uuid, row);
            connect(row, &TaskRow::actionRequested, this, [this, row](TaskAction action) { act(row->task(), action); });
            connect(row, &TaskRow::opened, this, [this, row] {
                if (m_openInInspector) emit taskSelected(row->task().uuid); else openTask(row->task());
            });
            connect(row, &TaskRow::dragStarted, this, [this, row](const QPoint &hotSpot) { startDrag(row, hotSpot); });
        }
        row->setObjectName(QStringLiteral("taskRow:") + task.id);
        const auto state = rowState(task);
        if (row->property("separated").toBool() != (index > 0)) { row->setProperty("separated", index > 0); row->update(); }
        row->setTask(task, state, colors, v.now);
        // A row already in its place is not moved, which would lay the list out again.
        if (m_list->indexOf(row) != index) { m_list->removeWidget(row); m_list->insertWidget(index, row); }
        ++index; row->show(); shown.insert(task.uuid);
    }
    for (auto it = m_rows.begin(); it != m_rows.end();) {
        if (shown.contains(it.key())) { ++it; continue; }
        retire(it.value()); it = m_rows.erase(it);
    }
    const int hidden = m_board.rows(m_stage, {m_host, m_session, false, v.now}).size() - m_board.rows(m_stage, v).size();
    const QString empty = v.only && hidden > 0 ? tr("No tasks of %1 here").arg(TaskBoard::sessionLabel(m_host, m_session))
        : m_stage == QLatin1String("review") ? tr("Nothing to review") : m_stage == QLatin1String("active") ? tr("No session is running a task")
        : m_stage == QLatin1String("todo") ? tr("Empty — write a prompt in the input below") : m_stage == QLatin1String("backlog") ? tr("Empty")
        : tr("Nothing yet");
    m_empty->setText(m_projectId.isEmpty() && !m_session.isEmpty() ? tr("Tasks belong to a project. Move this session into one to keep tasks for it.") : empty);
    m_empty->setVisible(rows.isEmpty() && m_error.isEmpty() && (!m_projectId.isEmpty() || !m_session.isEmpty()));
    m_older->setText(m_showOlder ? tr("Hide older") : tr("Show older (%1)").arg(older.size())); m_older->setVisible(!older.isEmpty());
    m_only->setToolTip(m_only->isChecked() ? tr("Show the tasks of every session of %1").arg(m_projectName)
                                           : tr("Show only the tasks of %1").arg(TaskBoard::sessionLabel(m_host, m_session)));
    m_only->setAccessibleName(m_only->toolTip());
    m_settings->setToolTip(tr("Task settings for %1").arg(m_projectName)); m_settings->setAccessibleName(m_settings->toolTip());
    updateComposer();
    emit boardChanged();
    // A listing during a drag moves the rows under a resting pointer: the line
    // follows them once they are laid out.
    if (m_overList)
        QMetaObject::invokeMethod(this, [this] { if (m_overList) followDrag(m_drag, m_listPage->mapFrom(m_scroll->viewport(), m_dragAt)); },
                                  Qt::QueuedConnection);
}

void TasksView::setExecutorLaunches(const QJsonObject &launches) {
    QHash<QString, QString> blockers;
    for (const auto &value : launches) {
        const auto record = value.toObject(); const auto task = record.value("task").toString();
        if (!task.isEmpty()) blockers[task] = record.value("phase") == "waiting" ? tr("The new executor is starting")
            : tr("The previous executor launch needs review before another launch.");
    }
    if (blockers == m_executorLaunchBlockers) return;
    m_executorLaunchBlockers = blockers; render();
}

void TasksView::setDescriptionDraft(const QString &uuid, bool unsaved) {
    const auto blocked = unsaved ? uuid : QString();
    if (m_descriptionDraft == blocked) return;
    m_descriptionDraft = blocked; render(); emit boardChanged();
}

TaskRowState TasksView::rowState(const TaskItem &task) const {
    const auto v = view();
    TaskRowState state; state.mine = TaskBoard::belongsTo(task, m_host, m_session, v.id); state.anyNow = TaskBoard::sessionLabel(m_host, m_session);
    if (const auto *ran = session(task.sessionHost, task.sessionName); task.worker.isEmpty() && ran && task.sessionKnown && task.stage == QLatin1String("active")) { state.sessionStatus = ran->status; state.statusKind = ran->kind; }
    if (TaskBoard::actions(task.stage).contains(TaskAction::SendNow))
        if (const auto why = sendBlocker(task); !why.isEmpty()) state.blocked.insert(int(TaskAction::SendNow), why);
    state.blocked.insert(int(TaskAction::Pause), tr("Pausing an individual task is not available yet"));
    if (!task.writable)
        for (const auto action : TaskBoard::actions(task.stage)) if (action != TaskAction::Edit) state.blocked.insert(int(action), tr("Update Zerus to edit this task"));
    return state;
}

void TasksView::updateComposer() {
    const QString target = m_stage == QLatin1String("backlog") ? tr("Backlog") : tr("To do");
    const QString session = TaskBoard::sessionLabel(m_host, m_session);
    m_composer->setSendLabel(target);
    m_composer->setPlaceholder(m_session.isEmpty() ? tr("Task prompt…") : tr("Task prompt for %1 / %2…").arg(m_projectName, session));
    if (m_session.isEmpty() || m_projectId.isEmpty()) return m_composer->setSessionStatus({});
    // The session and the stage stand out in the foreground colour.
    const auto strong = [fg = TaskColors::of(m_dark).fg.name()](const QString &text) {
        return QStringLiteral("<b style='color:%1;font-weight:600'>%2</b>").arg(fg, text.toHtmlEscaped());
    };
    const QString hint = tr("Task for %1 · Enter adds to %2 · Shift+Enter for a new line");
    m_composer->setSessionStatus(hint.arg(session, target), false, false, hint.toHtmlEscaped().arg(strong(session), strong(target)));
}

QList<TasksView::Step> TasksView::steps(const TaskItem &task, TaskAction action) const {
    switch (action) {
    case TaskAction::ToBacklog: return {{withProject({"stage", task.uuid, "backlog"}), {}}};
    case TaskAction::Top: return {{withProject({"move", task.uuid, "--top"}), {}}};
    case TaskAction::ToTodo: case TaskAction::BackToTodo: return {{withProject({"stage", task.uuid, "todo"}), {}}};
    case TaskAction::Accept: return {{withProject({"stage", task.uuid, "done"}), {}}};
    case TaskAction::SendNow: {
        // An Any session task goes to the open session, on whichever computer runs it.
        QStringList arguments{"send", task.uuid};
        if (task.targetSession.isEmpty()) arguments << "--session" << m_session;
        if (task.targetSession.isEmpty() && !m_host.isEmpty()) arguments << "--host" << m_host;
        return {{withProject(arguments), {}}};
    }
    case TaskAction::Rename: case TaskAction::Edit: case TaskAction::Cancel: case TaskAction::Pause: break;
    }
    return {};
}

QList<TasksView::Step> TasksView::editSteps(const TaskItem &task, const TaskEdits &edits) const {
    QList<Step> out;
    if (edits.attachments) out << Step{withProject({"edit", task.uuid, "--content-json"}), TaskContent::payload(edits.text.value_or(task.text), *edits.attachments)};
    else if (edits.text) out << Step{withProject({"edit", task.uuid}), *edits.text};
    if (edits.target)
        out << Step{withProject(targetArguments(task.uuid, edits.target->host, edits.target->any ? QString() : edits.target->session, edits.target->id, edits.target->mode)), {}};
    return out;
}

QStringList TasksView::targetArguments(const QString &uuid, const QString &host, const QString &session, const QString &id, const QString &mode) {
    const bool planned = mode == QLatin1String("new_session") || mode == QLatin1String("subagent");
    QStringList arguments{"target", uuid, planned ? (mode == QLatin1String("new_session") ? QStringLiteral("new-session") : mode) : session.isEmpty() ? QStringLiteral("any") : session};
    if (planned) arguments << "--parent" << session;
    if (!session.isEmpty() && !host.isEmpty()) arguments << "--host" << host;
    if (!session.isEmpty() && !id.isEmpty()) arguments << "--session-id" << id;
    return arguments;
}

void TasksView::retarget(const TaskDrag &task, const QString &host, const QString &session) {
    if (task.stage != QLatin1String("todo") && task.stage != QLatin1String("backlog")) return;
    const auto *target = this->session(host, session);
    run({{targetArguments(task.uuid, host, session, target ? target->id : QString()) << QStringLiteral("--project") << task.projectId, {}}});
}

void TasksView::run(QList<Step> steps, std::function<void()> finished, std::function<void(const QString &)> failed) {
    if (steps.isEmpty()) { refresh(); if (finished) finished(); return; }
    auto step = steps.takeFirst();
    request(step.arguments, step.input, [this, steps, then = step.then, finished, failed](const QJsonObject &reply) {
        if (then) then(reply);
        run(steps, finished, failed);
    }, failed ? std::function<void(const QString &)>([this, failed](const QString &error) { failed(error); refresh(); })
              : std::function<void(const QString &)>());
}

void TasksView::act(const TaskItem &task, TaskAction action) {
    if (action == TaskAction::Rename) {
        if (!task.writable) return;
        emit taskRenameRequested(task.uuid); return;
    }
    if (action == TaskAction::Pause) return; // Task-scoped execution accounting must precede pause.
    if (action == TaskAction::Edit) {
        if (m_openInInspector) { emit taskSelected(task.uuid); emit taskEditRequested(task.uuid); return; }
        return openTask(task);
    }
    if (!task.writable || !TaskBoard::actions(task.stage).contains(action)) return;
    if (action == TaskAction::Cancel) return confirmCancel(task);
    if (action == TaskAction::SendNow && task.targetKind == QLatin1String("new_session")) { emit newExecutorRequested(task, m_projectId); return; }
    if (action == TaskAction::SendNow && !sendBlocker(task).isEmpty()) return;
    run(steps(task, action));
}

void TasksView::addTask(const QString &key, const QString &text, const QList<MessageAttachment> &attachments) {
    if (key != m_composer->sessionKey() || m_projectId.isEmpty() || m_session.isEmpty()) return;
    const QString stage = m_stage == QLatin1String("backlog") ? QStringLiteral("backlog") : QStringLiteral("todo");
    QStringList arguments{"add", "--project", m_projectId, "--target", m_session};
    if (const auto *target = session(m_host, m_session); target && !target->id.isEmpty()) arguments << "--session-id" << target->id;
    if (!m_host.isEmpty()) arguments << "--host" << m_host;
    if (stage == QLatin1String("backlog")) arguments << "--backlog";
    if (!attachments.isEmpty()) arguments << "--content-json";
    m_composer->prepareSendingAsync(key, false, [this, key, arguments, text, attachments, stage](bool ready) {
        if (!ready) return;
        AttachmentFiles::runAsync<QString>(this, [text, attachments] {
            return attachments.isEmpty() ? text : TaskContent::payload(text, TaskContent::encode(attachments));
        }, [this, key, arguments, stage](const QString &input) {
            request(arguments, input,
                [this, key, stage](const QJsonObject &) { m_composer->deliveryFinished(key, true); selectStage(stage); refresh(); },
                [this, key](const QString &error) { m_composer->deliveryFinished(key, false, error); emit notice(error, true); });
        });
    });
}

void TasksView::startDrag(TaskRow *row, const QPoint &hotSpot) {
    if (m_projectId.isEmpty() || m_dragSource || !TaskDrag::draggable(row->task())) return;
    // A listing may take the row out of the list while the drag runs; the drag
    // carries the task, and the row is kept until the drag ends.
    m_dragSource = row;
    auto *drag = new QDrag(this);
    QPointer<QDrag> dragGuard(drag);
    drag->setMimeData(TaskDrag::of(m_projectId, row->task()).mimeData());
    drag->setPixmap(row->grab()); drag->setHotSpot(hotSpot);
    row->setDragging(true);
    execDrag(drag);
    if (dragGuard) dragGuard->deleteLater();
    const QPointer<TaskRow> source = std::exchange(m_dragSource, nullptr);
    if (source) {
        source->setDragging(false);
        if (source->property("taskRetired").toBool()) source->deleteLater();
    }
    // The drag is over wherever it ended, whether or not a leave reached the target.
    endListDrag();
    for (auto *tile : std::as_const(m_tiles)) tile->setDropTarget(false);
    emit taskDragFinished();
}

void TasksView::retire(TaskRow *row) {
    row->setProperty("taskRetired", true); row->hide(); row->setObjectName({});
    if (row != m_dragSource) row->deleteLater();
}

Qt::DropAction TasksView::execDrag(QDrag *drag) { return drag->exec(Qt::MoveAction); }

bool TasksView::takesOnTile(const TaskDrag &drag, const QString &stage) const {
    const auto list = [](const QString &s) { return s == QLatin1String("todo") || s == QLatin1String("backlog"); };
    return canReorderTask(drag, drag.stage) && list(stage) && list(drag.stage) && drag.stage != stage;
}

bool TasksView::canReorderTask(const TaskDrag &drag, const QString &stage) const {
    if (m_projectId.isEmpty() || drag.projectId != m_projectId || drag.stage != stage) return false;
    for (const auto &task : m_board.tasks)
        if (task.uuid == drag.uuid) return TaskDrag::draggable(task) && task.stage == stage;
    return false;
}

bool TasksView::canStageTask(const TaskDrag &drag, const QString &stage) const { return takesOnTile(drag, stage); }

void TasksView::reorderTask(const TaskDrag &drag, const QString &anchor, bool before) {
    if (!canReorderTask(drag, drag.stage) || anchor == drag.uuid) return;
    for (const auto &task : m_board.tasks) if (task.uuid == anchor && task.stage == drag.stage && task.writable) {
        run({{withProject({"move", drag.uuid, before ? "--before" : "--after", anchor}), {}}}); return;
    }
}

void TasksView::stageTask(const TaskDrag &drag, const QString &stage) {
    if (canStageTask(drag, stage)) run({{withProject({"stage", drag.uuid, stage}), {}}});
}

bool TasksView::takesInList(const TaskDrag &drag) const {
    return canReorderTask(drag, m_stage) && !shownRows().isEmpty();
}

QList<TaskRow *> TasksView::shownRows() const {
    QList<TaskRow *> rows;
    for (int i = 0; i < m_list->count(); ++i)
        if (auto *row = qobject_cast<TaskRow *>(m_list->itemAt(i)->widget()); row && !row->isHidden()) rows << row;
    return rows;
}

int TasksView::dropGap(const QString &uuid, const QPoint &point) const {
    const auto rows = shownRows(); int gap = int(rows.size()), from = -1;
    for (int i = 0; i < rows.size(); ++i) {
        if (rows[i]->task().uuid == uuid) from = i;
        if (gap == rows.size() && point.y() < rows[i]->mapTo(m_listPage, QPoint()).y() + rows[i]->height() / 2) gap = i;
    }
    return from >= 0 && (gap == from || gap == from + 1) ? -1 : gap;
}

void TasksView::followDrag(const TaskDrag &drag, const QPoint &point) {
    m_drag = drag; m_dragAt = m_listPage->mapTo(m_scroll->viewport(), point); m_overList = true;
    const auto rows = shownRows(); const int gap = rows.isEmpty() ? -1 : dropGap(drag.uuid, point);
    if (gap < 0) m_dropLine->hide();
    else {
        const auto *edge = gap < rows.size() ? rows[gap] : rows.last();
        const int y = edge->mapTo(m_listPage, QPoint(0, gap < rows.size() ? 0 : edge->height())).y();
        m_dropLine->setGeometry(m_listHost->x() + 8, y - 1, m_listHost->width() - 16, 2); m_dropLine->show(); m_dropLine->raise();
    }
    // Near the top or bottom of the visible list it scrolls that way.
    const int margin = 28, height = m_scroll->viewport()->height();
    m_scrollStep = m_dragAt.y() < margin ? -8 : m_dragAt.y() > height - margin ? 8 : 0;
    if (!m_scrollStep) m_autoScroll.stop(); else if (!m_autoScroll.isActive()) m_autoScroll.start();
}

void TasksView::reorder(const TaskDrag &drag, const QPoint &point) {
    const auto rows = shownRows(); const int gap = rows.isEmpty() ? -1 : dropGap(drag.uuid, point);
    if (gap < 0) return;
    reorderTask(drag, gap < rows.size() ? rows[gap]->task().uuid : rows.last()->task().uuid, gap < rows.size());
}

void TasksView::endListDrag() { m_autoScroll.stop(); m_scrollStep = 0; m_overList = false; m_dropLine->hide(); }

bool TasksView::eventFilter(QObject *watched, QEvent *event) {
    const auto type = event->type();
    if (type != QEvent::DragEnter && type != QEvent::DragMove && type != QEvent::DragLeave && type != QEvent::Drop)
        return QWidget::eventFilter(watched, event);
    if (watched == m_listPage) {
        if (type == QEvent::DragLeave) { endListDrag(); return true; }
        auto *drop = static_cast<QDropEvent *>(event);
        const auto drag = TaskDrag::from(drop->mimeData());
        if (!drag || !takesInList(*drag)) { endListDrag(); drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) { endListDrag(); reorder(*drag, drop->position().toPoint()); }
        else followDrag(*drag, drop->position().toPoint());
        return true;
    }
    for (auto it = m_tiles.cbegin(); it != m_tiles.cend(); ++it) {
        if (it.value() != watched) continue;
        auto *tile = it.value();
        if (type == QEvent::DragLeave) { tile->setDropTarget(false); return true; }
        auto *drop = static_cast<QDropEvent *>(event);
        const auto drag = TaskDrag::from(drop->mimeData());
        const bool takes = drag && takesOnTile(*drag, it.key());
        tile->setDropTarget(takes && type != QEvent::Drop);
        if (!takes) { drop->ignore(); return true; }
        drop->setDropAction(Qt::MoveAction); drop->accept();
        if (type == QEvent::Drop) stageTask(*drag, it.key());
        return true;
    }
    return QWidget::eventFilter(watched, event);
}

void TasksView::openTask(const TaskItem &task) {
    request(withProject({"get", task.uuid}), {}, [this](const QJsonObject &reply) {
        for (auto *open : findChildren<TaskDialog *>()) open->reject();
        auto shown = reply.value("task").toObject();
        const auto label = [this](QJsonObject key) {
            const QString id = key.value("session_id").toString();
            if (id.isEmpty() || key.value("host").isNull()) return key;
            for (const auto &candidate : m_sessions) if (candidate.host == key.value("host").toString() && candidate.id == id) {
                key["session"] = candidate.name; break;
            }
            return key;
        };
        for (const auto &key : {"target", "session", "author"}) if (shown.value(key).isObject()) shown[key] = label(shown.value(key).toObject());
        auto log = shown.value("log").toArray();
        for (auto it = log.begin(); it != log.end(); ++it) {
            auto entry = it->toObject();
            for (const auto &key : {"session", "actor"}) if (entry.value(key).isObject()) entry[key] = label(entry.value(key).toObject());
            *it = entry;
        }
        shown["log"] = log;
        const auto task = TaskBoard::item(shown);
        // Edits start with the freshly fetched description, including when the
        // listing changed before the dialog opened.
        const auto blocker = [this, task](const TaskTarget &target) {
            if (target == TaskTarget{task.targetSession.isEmpty(), task.targetHost, task.targetSession, task.targetId}) return sendBlocker(task);
            auto chosen = task; chosen.targetSession = target.any ? QString() : target.session; chosen.targetHost = target.host; chosen.targetId = target.id; chosen.targetKnown = true;
            return sendBlocker(chosen);
        };
        auto *dialog = new TaskDialog(shown, m_sessions, blocker, m_dark, this);
        dialog->setAttribute(Qt::WA_DeleteOnClose);
        // The dialog closes once hgs took everything; a refusal keeps it, and the edits, open.
        QPointer<TaskDialog> guard(dialog);
        const auto close = [guard] { if (guard) guard->accept(); };
        const auto refused = [this, guard](const QString &error) { if (guard) guard->setError(error); else emit notice(error, true); };
        connect(dialog, &TaskDialog::saveRequested, this, [this, task, close, refused](const TaskEdits &edits) { run(editSteps(task, edits), close, refused); });
        connect(dialog, &TaskDialog::actionRequested, this, [this, task, close, refused](const TaskEdits &edits, TaskAction action) {
            // The action meets the task as the edits leave it.
            auto edited = task;
            if (edits.target) { edited.targetSession = edits.target->any ? QString() : edits.target->session; edited.targetHost = edits.target->host; }
            if (action == TaskAction::Cancel) return run(editSteps(task, edits), [this, close, edited] { close(); confirmCancel(edited); }, refused);
            run(editSteps(task, edits) + steps(edited, action), close, refused);
        });
        connect(dialog, &TaskDialog::commentRequested, this, [this, guard, refused, uuid = task.uuid](const QString &text) {
            request(withProject({"comment", uuid}), text, [this, guard](const QJsonObject &reply) {
                refresh();
                if (!guard) return;
                guard->clearComment(); guard->setHistory(reply.value("task").toObject().value("log").toArray());
            }, refused);
        });
        dialog->open();
    });
}

void TasksView::createNamedTask(const QJsonObject &intent, std::function<void(const QJsonObject &)> saved,
                                std::function<void(const QString &)> refused) {
    const auto project = intent.value("project").toString();
    if (project.isEmpty() || project != m_projectId) { refused(tr("The selected project changed. The new task draft was kept.")); return; }
    QStringList args{"add", "--project", project, "--title", intent.value("title").toString(), "--request-id", intent.value("request_id").toString(),
                     "--target", intent.value("session").toString("any"), "--content-json"};
    if (intent.value("stage") == QLatin1String("backlog")) args << "--backlog";
    if (!intent.value("session_id").toString().isEmpty()) args << "--session-id" << intent.value("session_id").toString();
    if (!intent.value("host").toString().isEmpty()) args << "--host" << intent.value("host").toString();
    request(args, TaskContent::payload({}, {}), [this, intent, project, saved, refused](const QJsonObject &reply) {
        const auto task = reply.value("task").toObject();
        if (task.value("uuid") != intent.value("request_id")) { refused(tr("Task creation needs review: the reply did not match the reserved card.")); return; }
        if (project == m_projectId) {
            const auto uuid = task.value("uuid").toString();
            auto found = std::find_if(m_board.tasks.begin(), m_board.tasks.end(), [uuid](const TaskItem &t) { return t.uuid == uuid; });
            if (found == m_board.tasks.end()) m_board.tasks.prepend(TaskBoard::item(task));
            render(); emit boardChanged();
        }
        saved(task); refresh();
    }, refused);
}

void TasksView::saveTaskEdits(const TaskItem &task, const TaskEdits &edits,
                              std::function<void()> saved, std::function<void(const QString &)> refused) {
    run(editSteps(task, edits), saved, refused);
}

void TasksView::renameTask(const QString &uuid, const QString &title, std::function<void()> saved, std::function<void(const QString &)> refused) {
    run({{withProject({"rename", uuid}), title}}, saved, refused);
}

void TasksView::saveTaskAssignment(const TaskItem &task, const TaskTarget &target, std::function<void()> saved, std::function<void(const QString &)> refused) {
    run({{withProject(targetArguments(task.uuid, target.host, target.any ? QString() : target.session, target.id, target.mode)), {}}}, saved, refused);
}

void TasksView::saveTaskNote(const QString &uuid, const QString &text, const QList<MessageAttachment> &files,
                             std::function<void()> saved, std::function<void(const QString &)> refused) {
    if (files.isEmpty()) run({{withProject({"comment", uuid}), text}}, saved, refused);
    else {
        const auto project = m_projectId;
        AttachmentFiles::runAsync<QString>(this, [text, files] { return TaskContent::payload(text, TaskContent::encode(files)); },
            [this, project, uuid, saved, refused](const QString &payload) {
                if (project != m_projectId) { refused(tr("The selected project changed; the comment draft was kept.")); return; }
                run({{{"comment", uuid, "--content-json", "--project", project}, payload}}, saved, refused);
            });
    }
}

void TasksView::saveTaskMessage(const QString &uuid, const QString &mode, const QString &dispatch, const QString &text, const QList<MessageAttachment> &files,
                               std::function<void()> saved, std::function<void(const QString &)> refused) {
    const auto project = m_projectId;
    AttachmentFiles::runAsync<TaskMessageOutbox::Request>(this, [project, uuid, mode, dispatch, text, files] {
        return TaskMessageOutbox::reserve(project, uuid, {{"content", QJsonObject{{"text", text}, {"attachments", TaskContent::encode(files)}}}, {"mode", mode}, {"dispatch", dispatch}});
    }, [this, project, uuid, mode, dispatch, text, files, saved, refused](const TaskMessageOutbox::Request &request) {
        if (!request.error.isEmpty()) { refused(request.error); return; }
        if (project != m_projectId) { refused(tr("The selected project changed; the original message was kept for recovery.")); return; }
        if (request.differentIntent) {
            // Retain the previous comment in the discussion using a read-only
            // receipt check, then handle the new explicit submission separately.
            submitTaskMessage(project, uuid, request.record, true, [this, project, uuid, mode, dispatch, text, files, saved, refused] {
                if (project != m_projectId) { refused(tr("The selected project changed. Your new comment draft was kept.")); return; }
                saveTaskMessage(uuid, mode, dispatch, text, files, saved, refused);
            }, refused); return;
        }
        submitTaskMessage(project, uuid, request.record, request.recovering, saved, refused);
    });
}

void TasksView::submitTaskMessage(const QString &project, const QString &uuid, const QJsonObject &record, bool recovery,
                                std::function<void()> saved, std::function<void(const QString &)> refused) {
    const auto intent = record.value("intent").toObject(); const auto id = record.value("id").toString();
    QStringList args{"message", uuid, "--request-id", id, "--mode", intent.value("mode").toString(),
        "--expect-dispatch", intent.value("dispatch").toString(), "--content-json", "--project", project};
    if (recovery) args << "--recover-only";
    request(args, QString::fromUtf8(QJsonDocument(intent.value("content").toObject()).toJson(QJsonDocument::Compact)),
        [this, project, uuid, id, saved, refused](const QJsonObject &reply) {
            if (reply.value("task").toObject().value("uuid").toString() != uuid || reply.value("message").toObject().value("request_id").toString() != id) {
                refused(tr("The message reply did not match its original identity. Recovery data was kept.")); return;
            }
            refresh();
            AttachmentFiles::runAsync<QString>(this, [project, uuid, id] { return TaskMessageOutbox::release(project, uuid, id); }, [saved, refused](const QString &error) {
                if (error.isEmpty()) saved(); else refused(error);
            });
        }, refused);
}

void TasksView::recoverTaskMessage(const QString &uuid, std::function<void(const QJsonObject &)> checked, std::function<void(const QString &)> refused) {
    const auto project = m_projectId;
    AttachmentFiles::runAsync<TaskMessageOutbox::Request>(this, [project, uuid] { return TaskMessageOutbox::read(TaskMessageOutbox::path(project, uuid)); },
        [this, project, uuid, checked, refused](const TaskMessageOutbox::Request &request) {
            if (!request.error.isEmpty()) { refused(request.error); return; }
            if (project != m_projectId) { refused(tr("Return to this project before checking delivery.")); return; }
            const auto content = request.record.value("intent").toObject().value("content").toObject();
            submitTaskMessage(project, uuid, request.record, true, [checked, content] { checked(content); }, refused);
        });
}

void TasksView::releaseTaskMessage(const QString &uuid, std::function<void()> released, std::function<void(const QString &)> refused) {
    const auto project = m_projectId; const auto id = Settings::value(TaskMessageOutbox::key(project, uuid)).toString();
    AttachmentFiles::runAsync<QString>(this, [project, uuid, id] { return TaskMessageOutbox::release(project, uuid, id); }, [released, refused](const QString &error) {
        if (error.isEmpty()) released(); else refused(error);
    });
}

QString TasksView::taskSendBlocker(const TaskItem &task, const TaskTarget &target) const {
    auto chosen = task;
    chosen.targetSession = target.any ? QString() : target.session;
    chosen.targetHost = target.host; chosen.targetId = target.id; chosen.targetKnown = true;
    return sendBlocker(chosen);
}

void TasksView::confirmCancel(const TaskItem &task) {
    const auto *ran = session(task.sessionHost, task.sessionName);
    auto *dialog = new TaskCancelDialog(task, ran && ran->running, ran && ran->working, this);
    dialog->setAttribute(Qt::WA_DeleteOnClose);
    connect(dialog, &QDialog::accepted, this, [this, dialog, task] {
        QStringList arguments{"cancel", task.uuid};
        if (!dialog->reason().isEmpty()) arguments << "--reason" << dialog->reason();
        run({{withProject(arguments), {}, [this, task](const QJsonObject &reply) {
            // The task is cancelled even when hgs refused the cancel message.
            const auto message = reply.value("cancel_message").toObject();
            if (!message.value("sent").toBool(true))
                emit notice(tr("%1 is cancelled; its session did not get the cancel message: %2").arg(task.id, message.value("error").toString()), true);
        }}});
    });
    dialog->open();
}

void TasksView::openSettings() {
    if (m_projectId.isEmpty()) return;
    request({"settings", "--project", m_projectId}, {}, [this](const QJsonObject &reply) {
        auto *dialog = new TaskSettingsDialog(m_projectName, reply.value("settings").toObject(), this);
        dialog->setAttribute(Qt::WA_DeleteOnClose);
        QPointer<TaskSettingsDialog> guard(dialog);
        connect(dialog, &TaskSettingsDialog::saveRequested, this, [this, guard](const QJsonObject &changes) {
            if (changes.isEmpty()) { if (guard) guard->accept(); return; }
            request({"settings", "--project", m_projectId, "--set"}, QString::fromUtf8(QJsonDocument(changes).toJson(QJsonDocument::Compact)),
                [guard](const QJsonObject &) { if (guard) guard->accept(); },
                [this, guard](const QString &error) { if (guard) guard->setError(error); else emit notice(error, true); });
        });
        dialog->open();
    });
}

void TasksView::setTheme(bool dark) {
    m_dark = dark; const auto colors = TaskColors::of(dark);
    for (auto *tile : std::as_const(m_tiles)) tile->setColors(colors);
    for (auto *button : {m_only, m_settings}) {
        const bool on = button->isChecked();
        button->setIcon(workspaceIcon(button == m_only ? QStringLiteral("task-funnel") : QStringLiteral("settings"), on ? colors.accent : colors.muted));
        setWorkspaceStyle(button, QStringLiteral("QToolButton{border:1px solid %1;border-radius:9px;background:transparent;}"
            "QToolButton:hover{border-color:%2;}QToolButton:checked{border-color:%3;background:%4;}")
            .arg(colors.border.name(), colors.muted.name(), alpha(colors.accent, 0.5).name(QColor::HexArgb), alpha(colors.accent, 0.12).name(QColor::HexArgb)));
    }
    setWorkspaceStyle(m_scroll, QStringLiteral("QScrollArea,#taskListPage{background:transparent;}") + workspaceScrollbars(dark));
    setWorkspaceStyle(m_listHost, QStringLiteral("#taskListHost{background:%1;border-radius:10px;}").arg(colors.surface.name()));
    setWorkspaceStyle(m_dropLine, QStringLiteral("background:%1;border-radius:1px;").arg(colors.stage(QStringLiteral("todo")).name()));
    setWorkspaceStyle(m_empty, QStringLiteral("color:%1;").arg(colors.muted.name()));
    setWorkspaceStyle(m_errorLabel, QStringLiteral("color:%1;padding:0 0 10px 0;").arg(dark ? "#f4ab9b" : "#a13224"));
    setWorkspaceStyle(m_damaged, QStringLiteral("color:%1;padding:0 0 10px 0;").arg(colors.warn.name()));
    setWorkspaceStyle(m_older, QStringLiteral("QPushButton{border:0;color:%1;padding:6px 2px;font-size:12px;}").arg(colors.accent.name()));
    m_composer->setTheme(dark);
    render();
}
