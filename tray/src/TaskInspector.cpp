#include "TaskInspector.h"
#include "ActivityView.h"
#include "TaskDialogs.h"
#include "TaskRow.h"
#include "TaskStageFilter.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QPushButton>
#include <QResizeEvent>
#include <QScrollArea>
#include <QSet>
#include <QTextBrowser>
#include <QTabBar>
#include <QSettings>
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
        connect(tile, &QAbstractButton::clicked, this, [this, stage] {
            if (m_stage != stage) { m_stage = stage; m_selected.clear(); m_detail = {}; ++m_generation; m_showOlder = false; }
            refresh();
        });
    }
    layout->addWidget(m_filters);
    m_scroll = new QScrollArea; m_scroll->setObjectName("taskInspectorList"); m_scroll->setWidgetResizable(true);
    m_scroll->setFrameShape(QFrame::NoFrame); m_scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    m_listPage = new QWidget; m_listPage->setObjectName("taskInspectorCards");
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
    for (auto *button : {m_open, m_edit}) { button->setFixedSize(30, 28); button->setIconSize(QSize(20, 20)); context->addWidget(button); }
    m_open->setToolTip(tr("Open executor Activity")); m_open->setAccessibleName(m_open->toolTip());
    m_edit->setToolTip(tr("Edit task / actions")); m_edit->setAccessibleName(m_edit->toolTip());
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (const auto *found = executor()) emit executorRequested(found->host, found->name, found->id);
    });
    connect(m_edit, &QPushButton::clicked, this, [this] {
        if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    details->addLayout(context);
    m_detailTabs = new DetailTabs; m_detailTabs->setObjectName("taskDetailTabs"); m_detailTabs->setExpanding(false);
    m_detailTabs->setDocumentMode(true); m_detailTabs->setDrawBase(false); m_detailTabs->setElideMode(Qt::ElideNone);
    m_detailTabs->setUsesScrollButtons(false); QFont tabFont = font(); tabFont.setPixelSize(12); m_detailTabs->setFont(tabFont);
    m_detailTabs->addTab(tr("Description")); m_detailTabs->addTab(tr("Result")); m_detailTabs->addTab(tr("History"));
    details->addWidget(m_detailTabs);
    m_moreDetail = new QPushButton; m_moreDetail->setObjectName("taskDetailMore"); m_moreDetail->setFlat(true); m_moreDetail->hide();
    details->addWidget(m_moreDetail, 0, Qt::AlignLeft);
    connect(m_moreDetail, &QPushButton::clicked, this, [this] {
        if (m_detailTabs->currentIndex() == 2 && m_historyVisible < m_detail.value("log").toArray().size()) {
            m_historyVisible += 8; renderDetail();
        } else if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    m_content = new QStackedWidget; m_content->setObjectName("taskDetailContent");
    m_emptyDetail = new QLabel; m_emptyDetail->setObjectName("taskDetailEmpty"); m_emptyDetail->setAlignment(Qt::AlignCenter);
    m_emptyDetail->setWordWrap(true); m_content->addWidget(m_emptyDetail);
    m_activity = new ActivityView; m_activity->setObjectName("taskDetailActivity");
    m_activity->browser()->setObjectName("taskInspectorDetail"); m_activity->browser()->setAccessibleName(tr("Selected task details"));
    // Task file references are resolved by the existing task attachment dialog,
    // never relative to whichever session happens to be open.
    connect(m_activity, &ActivityView::fileReferenceActivated, this, [this] {
        if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    m_content->addWidget(m_activity); details->addWidget(m_content, 1); m_splitter->addWidget(m_details);
    auto saved = QSettings().value("tasks/inspectorSplit").toList();
    m_splitter->setSizes(saved.size() == 2 && saved[0].toInt() > 0 && saved[1].toInt() > 0
        ? QList<int>{saved[0].toInt(), saved[1].toInt()} : QList<int>{400, 320});
    connect(m_splitter, &QSplitter::splitterMoved, this, [this] {
        const auto sizes = m_splitter->sizes(); QSettings().setValue("tasks/inspectorSplit", QVariantList{sizes[0], sizes[1]});
    });
    connect(m_detailTabs, &QTabBar::currentChanged, this, [this] { renderDetail(); });
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
        const auto log = task.value("log").toArray(); QJsonArray recent;
        for (int i = qMax(0, int(log.size()) - VisibleHistory); i < log.size(); ++i) recent.append(log.at(i));
        task["log_total"] = log.size(); task["log"] = recent;
        if (task != m_detail) { m_detail = task; renderDetail(); }
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
    m_project = project; m_host = host; m_session = name; m_sessionId = id; m_sessions = sessions;
    if (changed) { ++m_generation; m_selected.clear(); m_detail = {}; m_stage.clear(); m_showOlder = false; }
    refresh();
}

void TaskInspector::showEvent(QShowEvent *event) {
    QWidget::showEvent(event); m_board->setInspectorVisible(true); refresh();
}

void TaskInspector::hideEvent(QHideEvent *event) {
    QWidget::hideEvent(event); m_board->setInspectorVisible(false);
}

void TaskInspector::selectTask(const QString &uuid) {
    const auto board = m_board->displayBoard();
    for (const auto &task : board.tasks) if (task.uuid == uuid) {
        m_stage = task.stage;
        if (!TaskBoard::keptFor(task, m_host, m_session, m_sessionId)) m_board->setOnlyThisSession(false);
        if (task.stage == QLatin1String("done") && QDateTime::currentMSecsSinceEpoch() - task.stageAt > TaskBoard::RecentDoneMs) m_showOlder = true;
        break;
    }
    if (m_selected != uuid) { m_selected = uuid; ++m_generation; m_detail = {}; m_historyVisible = 8; m_detailTabs->setCurrentIndex(0); }
    refresh(); fetch();
}

void TaskInspector::refresh() {
    if (!m_cards) return;
    const auto board = m_board->displayBoard(); const auto colors = TaskColors::of(m_dark);
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
        }
        row->setTask(task, m_board->rowState(task), colors, view.now);
        row->setProperty("expanded", task.uuid == m_selected);
        m_cards->removeWidget(row); m_cards->insertWidget(index++, row); row->show(); keep.insert(task.uuid);
        if (task.uuid == m_selected) selectedShown = true;
    }
    for (auto it = m_rows.begin(); it != m_rows.end();) {
        if (keep.contains(it.key())) { ++it; continue; }
        auto *row = it.value(); m_cards->removeWidget(row); row->hide(); row->setObjectName({}); row->deleteLater(); it = m_rows.erase(it);
    }
    if (!selectedShown && !m_selected.isEmpty()) { m_selected.clear(); m_detail = {}; ++m_generation; }
    m_scope->setText(m_project.isEmpty() ? tr("Open a project session to see tasks")
        : total > VisibleTasks ? tr("%1 of %2 tasks").arg(VisibleTasks).arg(total)
        : total == 1 ? tr("1 task") : tr("%1 tasks").arg(total));
    m_older->setVisible(!older.isEmpty()); m_older->setText(m_showOlder ? tr("Hide older") : tr("Show older (%1)").arg(older.size()));
    updateFilters();
    renderDetail(); if (isVisible()) fetch();
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
    const auto data = labels(m_detail); auto task = TaskBoard::item(data);
    for (const auto &listed : m_board->board().tasks) if (listed.uuid == task.uuid) { task.queue = listed.queue; break; }
    m_open->setEnabled(!m_detail.isEmpty() && executor()); m_edit->setEnabled(!m_detail.isEmpty());
    m_detailTabs->setEnabled(!m_detail.isEmpty()); m_moreDetail->hide();
    const auto empty = [this](const QString &text) {
        m_activity->setTaskJournal({}, {}); m_emptyDetail->setText(text); m_content->setCurrentWidget(m_emptyDetail);
    };
    if (m_detail.isEmpty()) {
        m_state->setText(tr("Task details")); m_state->setToolTip({});
        empty(m_selected.isEmpty() ? tr("Select a task above to see its details.") : tr("Loading task…")); return;
    }
    const auto log = data.value("log").toArray();
    bool started = false;
    for (const auto &entry : log) {
        const auto kind = entry.toObject().value("kind").toString();
        if (kind == QLatin1String("sent")) started = false;
        else if (kind == QLatin1String("started")) started = true;
    }
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
        else if (data.value("log_total").toInt() > VisibleHistory) { m_moreDetail->show(); m_moreDetail->setText(tr("Full history…")); }
        for (int i = first; i < log.size(); ++i) {
            const auto entry = log.at(i).toObject(); const auto kind = entry.value("kind").toString();
            const bool message = kind == QLatin1String("comment") || kind == QLatin1String("report");
            entries.append(journalEntry(QString::number(data.value("log_total").toInt() - log.size() + i), entry.value("actor").toObject(),
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
        "#taskDetailMore{color:%5;border:0;padding:4px;}" )
        .arg(colors.fg.name(), colors.surface.name(), colors.border.name(), colors.muted.name(), colors.accent.name()) + workspaceScrollbars(dark));
    m_activity->setTheme(dark);
    m_open->setIcon(workspaceIcon("external", colors.muted)); m_edit->setIcon(workspaceIcon("more", colors.muted));
    for (auto *button : {m_open, m_edit}) setWorkspaceStyle(button,
        QStringLiteral("QPushButton{border:0;border-radius:5px;background:transparent;}QPushButton:hover{background:%1;}").arg(colors.hover.name()));
    refresh();
}
