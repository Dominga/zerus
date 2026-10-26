#include "TaskInspector.h"
#include "TaskDialogs.h"
#include "TaskRow.h"
#include "TaskStageFilter.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QCheckBox>
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QPushButton>
#include <QResizeEvent>
#include <QScrollArea>
#include <QScrollBar>
#include <QSet>
#include <QTextDocument>
#include <QTextBrowser>
#include <QTabBar>
#include <QVBoxLayout>
#include <QtMath>
#include <algorithm>

namespace {
constexpr int VisibleTasks = 100;
constexpr int VisibleHistory = 80;
QString escaped(const QString &text) { return text.toHtmlEscaped(); }
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
    auto *overview = new QPushButton(tr("Project board")); overview->setObjectName("taskProjectBoard"); overview->setAutoDefault(false);
    overview->setToolTip(tr("Plan, reorder and add tasks for the project"));
    connect(overview, &QPushButton::clicked, this, &TaskInspector::boardRequested);
    header->addWidget(m_scope, 1); header->addWidget(overview); layout->addLayout(header);
    m_only = new QCheckBox(tr("This session")); m_only->setObjectName("taskInspectorOnlySession"); m_only->setChecked(true);
    connect(m_only, &QCheckBox::toggled, this, &TaskInspector::refresh); header->insertWidget(1, m_only);
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
    m_scroll->setWidget(m_listPage); layout->addWidget(m_scroll, 1);
    m_expansion = new QWidget(m_listPage); m_expansion->setObjectName("taskInlineDetails");
    auto *details = new QVBoxLayout(m_expansion); details->setContentsMargins(10, 4, 10, 12); details->setSpacing(8);
    m_state = new QLabel; m_state->setObjectName("taskInspectorState"); m_state->setWordWrap(true); m_state->setTextFormat(Qt::PlainText);
    details->addWidget(m_state);
    m_detailTabs = new QTabBar; m_detailTabs->setObjectName("taskDetailTabs"); m_detailTabs->setExpanding(false); m_detailTabs->setDrawBase(false);
    m_detailTabs->addTab(tr("Description")); m_detailTabs->addTab(tr("Result")); m_detailTabs->addTab(tr("History"));
    details->addWidget(m_detailTabs);
    m_description = new QTextBrowser; m_description->setObjectName("taskInspectorDetail");
    m_description->setOpenLinks(false); m_description->setOpenExternalLinks(false);
    m_description->setFrameShape(QFrame::NoFrame); details->addWidget(m_description);
    connect(m_detailTabs, &QTabBar::currentChanged, this, [this] { renderDetail(); });
    connect(m_description, &QTextBrowser::anchorClicked, this, [this](const QUrl &url) {
        if (url == QUrl("hgs-task:older")) { m_historyVisible += 8; renderDetail(); }
        else if (url == QUrl("hgs-task:attachments") || url == QUrl("hgs-task:history"))
            if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    auto *actions = new QHBoxLayout;
    m_open = new QPushButton(tr("Open Activity")); m_open->setObjectName("taskOpenActivity"); m_open->setAutoDefault(false);
    m_edit = new QPushButton(tr("Edit / actions")); m_edit->setObjectName("taskInspectorEdit"); m_edit->setAutoDefault(false);
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (const auto *found = executor()) emit executorRequested(found->host, found->name, found->id);
    });
    connect(m_edit, &QPushButton::clicked, this, [this] {
        if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    actions->addWidget(m_open); actions->addWidget(m_edit); actions->addStretch(); details->addLayout(actions);
    m_expansion->hide();
    m_older = new QPushButton; m_older->setObjectName("taskInspectorOlder"); m_older->hide(); layout->addWidget(m_older);
    connect(m_older, &QPushButton::clicked, this, [this] { m_showOlder = !m_showOlder; refresh(); });
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
        if (!TaskBoard::keptFor(task, m_host, m_session, m_sessionId)) m_only->setChecked(false);
        if (task.stage == QLatin1String("done") && QDateTime::currentMSecsSinceEpoch() - task.stageAt > TaskBoard::RecentDoneMs) m_showOlder = true;
        break;
    }
    if (m_selected != uuid) { m_selected = uuid; ++m_generation; m_detail = {}; m_historyVisible = 8; m_detailTabs->setCurrentIndex(0); }
    refresh(); fetch();
}

void TaskInspector::refresh() {
    if (!m_cards) return;
    const auto board = m_board->displayBoard(); const auto colors = TaskColors::of(m_dark);
    const TaskBoard::View view{m_host, m_session, m_only->isChecked(), QDateTime::currentMSecsSinceEpoch(), m_sessionId};
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
    m_cards->removeWidget(m_expansion); QSet<QString> keep; int index = 0, expanded = -1;
    for (const auto &task : shown) {
        auto *row = m_rows.value(task.uuid);
        if (!row) {
            row = new TaskRow(m_listPage); row->setCompact(true); m_rows.insert(task.uuid, row);
            row->setObjectName(QStringLiteral("taskInspectorRow:") + task.id);
            connect(row, &TaskRow::opened, this, [this, row] {
                if (m_selected == row->task().uuid) { m_selected.clear(); m_detail = {}; ++m_generation; refresh(); }
                else selectTask(row->task().uuid);
            });
            connect(row, &TaskRow::actionRequested, this, [this, row](TaskAction action) { m_board->actOnTask(row->task(), action); });
        }
        row->setTask(task, m_board->rowState(task), colors, view.now);
        row->setProperty("expanded", task.uuid == m_selected);
        m_cards->removeWidget(row); m_cards->insertWidget(index++, row); row->show(); keep.insert(task.uuid);
        if (task.uuid == m_selected) expanded = index;
    }
    for (auto it = m_rows.begin(); it != m_rows.end();) {
        if (keep.contains(it.key())) { ++it; continue; }
        auto *row = it.value(); m_cards->removeWidget(row); row->hide(); row->setObjectName({}); row->deleteLater(); it = m_rows.erase(it);
    }
    if (expanded < 0 && !m_selected.isEmpty()) { m_selected.clear(); m_detail = {}; ++m_generation; }
    if (expanded >= 0) m_cards->insertWidget(expanded, m_expansion);
    m_expansion->setVisible(expanded >= 0);
    m_scope->setText(m_project.isEmpty() ? tr("Open a project session to see tasks")
        : total > VisibleTasks ? tr("%1 of %2 tasks").arg(VisibleTasks).arg(total)
        : total == 1 ? tr("1 task") : tr("%1 tasks").arg(total));
    m_older->setVisible(!older.isEmpty()); m_older->setText(m_showOlder ? tr("Hide older") : tr("Show older (%1)").arg(older.size()));
    updateFilters();
    renderDetail(); if (isVisible()) fetch();
}

void TaskInspector::resizeEvent(QResizeEvent *event) {
    QWidget::resizeEvent(event); updateFilters(); updateDetailHeight();
}

void TaskInspector::updateFilters() {
    if (!m_filters) return;
    int required = (m_tiles.size() - 1) * 4;
    for (auto *tile : std::as_const(m_tiles)) required += tile->expandedWidth();
    for (auto *tile : std::as_const(m_tiles)) tile->setCompact(m_filters->width() < required);
}

void TaskInspector::updateDetailHeight() {
    if (!m_description) return;
    m_description->document()->setTextWidth(qMax(100, m_description->viewport()->width()));
    const int maximum = qBound(160, m_scroll->viewport()->height() / 2, 360);
    m_description->setFixedHeight(qBound(80, qCeil(m_description->document()->size().height()) + 16, maximum));
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
    if (!m_description) return;
    const auto data = labels(m_detail); auto task = TaskBoard::item(data);
    for (const auto &listed : m_board->board().tasks) if (listed.uuid == task.uuid) { task.queue = listed.queue; break; }
    m_open->setEnabled(!m_detail.isEmpty() && executor()); m_edit->setEnabled(!m_detail.isEmpty());
    if (m_detail.isEmpty()) {
        m_state->setText(m_selected.isEmpty() ? tr("No tasks here. Add one on the project board.") : tr("Loading task…"));
        m_description->setProperty("renderedHtml", QVariant()); m_description->clear(); return;
    }
    bool started = false;
    for (const auto &entry : data.value("log").toArray()) {
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
    m_state->setText(context); m_state->setToolTip(QStringLiteral("%1 · %2").arg(task.id, TaskBoard::stageLabel(task.stage)));
    QString html;
    const auto body = [](const QString &text) { return escaped(text).replace('\n', QStringLiteral("<br>")); };
    switch (m_detailTabs->currentIndex()) {
    case 0: {
        // Task text is untrusted; attachments are opened explicitly.
        html = body(task.text);
        const int files = data.value("attachments").toArray().size();
        if (files) html += QStringLiteral("<p><a href='hgs-task:attachments'>%1</a></p>")
            .arg(escaped(files == 1 ? tr("1 attachment") : tr("%1 attachments").arg(files)));
        break;
    }
    case 1:
        html = task.cancelledAt ? QStringLiteral("<b>%1</b><p>%2</p>").arg(escaped(tr("Cancelled")), body(task.cancelReason))
            : task.summary.isEmpty() ? escaped(tr("No result reported yet.")) : body(task.summary);
        break;
    case 2: {
        const auto log = data.value("log").toArray(); const int first = qMax(0, int(log.size()) - m_historyVisible);
        if (first > 0) html += QStringLiteral("<p><a href='hgs-task:older'>%1</a></p>").arg(escaped(tr("Earlier events (%1)").arg(first)));
        else if (data.value("log_total").toInt() > VisibleHistory)
            html += QStringLiteral("<p><a href='hgs-task:history'>%1</a></p>").arg(escaped(tr("Full history…")));
        for (int i = first; i < log.size(); ++i) html += taskHistoryLine(log.at(i).toObject());
        if (log.isEmpty()) html += escaped(tr("No history yet."));
        break;
    }
    }
    // Avoid replacing a document on unchanged polls: preserve selection and scroll.
    if (m_description->property("renderedHtml").toString() == html) return;
    const int scroll = m_description->verticalScrollBar()->value();
    m_description->setProperty("renderedHtml", html); m_description->setHtml(html);
    updateDetailHeight(); m_description->verticalScrollBar()->setValue(scroll);
}

void TaskInspector::setTheme(bool dark) {
    m_dark = dark; if (!m_cards) return;
    const auto colors = TaskColors::of(dark);
    setWorkspaceStyle(this, QStringLiteral("#taskInspector{background:%2;color:%1;}"
        "#taskInspector QLabel,#taskInspector QCheckBox{color:%1;}"
        "QScrollArea,#taskInspectorCards{background:%2;border:0;}QTextBrowser{background:transparent;color:%1;border:0;}"
        "#taskInlineDetails{background:%2;border:1px solid %3;border-radius:8px;}"
        "#taskInspectorState{color:%4;font-size:11px;}"
        "#taskDetailTabs::tab{color:%4;padding:5px 8px;border:0;border-bottom:2px solid transparent;}"
        "#taskDetailTabs::tab:selected{color:%1;border-bottom-color:%5;}")
        .arg(colors.fg.name(), colors.surface.name(), colors.border.name(), colors.muted.name(), colors.accent.name()) + workspaceScrollbars(dark));
    refresh();
}
