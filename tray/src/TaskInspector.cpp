#include "TaskInspector.h"
#include "TaskDialogs.h"
#include "TaskRow.h"
#include "WorkspaceIcons.h"
#include "WorkspaceStyle.h"
#include <QCheckBox>
#include <QDateTime>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QLabel>
#include <QListWidget>
#include <QPushButton>
#include <QScrollBar>
#include <QSignalBlocker>
#include <QTextBrowser>
#include <QVBoxLayout>

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
    connect(m_only, &QCheckBox::toggled, this, &TaskInspector::refresh); layout->addWidget(m_only);
    m_list = new QListWidget; m_list->setObjectName("taskInspectorList"); m_list->setWordWrap(true);
    m_list->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff); m_list->setMinimumHeight(96); m_list->setMaximumHeight(220);
    m_list->setIconSize(QSize(20, 20)); m_list->setSpacing(2); layout->addWidget(m_list);
    connect(m_list, &QListWidget::currentRowChanged, this, [this](int row) {
        const auto *item = m_list->item(row); if (!item) return;
        const QString uuid = item->data(Qt::UserRole).toString();
        if (uuid == m_selected) return;
        m_selected = uuid; ++m_generation; m_detail = {}; renderDetail(); fetch();
    });
    m_state = new QLabel; m_state->setObjectName("taskInspectorState"); m_state->setWordWrap(true); m_state->setTextFormat(Qt::PlainText);
    layout->addWidget(m_state);
    m_description = new QTextBrowser; m_description->setObjectName("taskInspectorDetail");
    m_description->setOpenLinks(false); m_description->setOpenExternalLinks(false);
    m_description->setFrameShape(QFrame::NoFrame); layout->addWidget(m_description, 1);
    auto *actions = new QHBoxLayout;
    m_open = new QPushButton(tr("Open Activity")); m_open->setObjectName("taskOpenActivity"); m_open->setAutoDefault(false);
    m_edit = new QPushButton(tr("Edit / actions")); m_edit->setObjectName("taskInspectorEdit"); m_edit->setAutoDefault(false);
    connect(m_open, &QPushButton::clicked, this, [this] {
        if (const auto *found = executor()) emit executorRequested(found->host, found->name, found->id);
    });
    connect(m_edit, &QPushButton::clicked, this, [this] {
        if (!m_detail.isEmpty()) m_board->editTask(TaskBoard::item(labels(m_detail)));
    });
    actions->addWidget(m_open); actions->addWidget(m_edit); actions->addStretch(); layout->addLayout(actions);
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
    if (changed) { ++m_generation; m_selected.clear(); m_detail = {}; }
    refresh();
}

void TaskInspector::showEvent(QShowEvent *event) {
    QWidget::showEvent(event); m_board->setInspectorVisible(true); refresh();
}

void TaskInspector::hideEvent(QHideEvent *event) {
    QWidget::hideEvent(event); m_board->setInspectorVisible(false);
}

void TaskInspector::selectTask(const QString &uuid) {
    m_only->setChecked(false);
    if (m_selected != uuid) { m_selected = uuid; ++m_generation; m_detail = {}; }
    refresh(); fetch();
}

void TaskInspector::refresh() {
    if (!m_list) return;
    const QSignalBlocker blocker(m_list);
    const auto board = m_board->displayBoard(); const auto colors = TaskColors::of(m_dark);
    const int scroll = m_list->verticalScrollBar()->value();
    m_list->clear(); int total = 0, selected = -1;
    // Execution and review come before the ordered, manually dispatched To do.
    for (const auto &stage : {"active", "review", "todo", "backlog"}) {
        const auto rows = board.rows(stage, {m_host, m_session, m_only->isChecked(), QDateTime::currentMSecsSinceEpoch(), m_sessionId});
        for (const auto &task : rows) {
            ++total; if (m_list->count() >= VisibleTasks) continue;
            QString prefix = TaskBoard::stageLabel(task.stage);
            if (task.queue > 0 && task.stage == QLatin1String("todo")) prefix += tr(" #%1").arg(task.queue);
            auto *item = new QListWidgetItem(workspaceIcon(TaskBoard::stageIcon(task.stage), colors.stage(task.stage)),
                QStringLiteral("%1 · %2\n%3").arg(task.id, prefix, task.text.left(240).simplified()), m_list);
            item->setData(Qt::UserRole, task.uuid); item->setToolTip(task.text.left(2000));
            if (task.uuid == m_selected) selected = m_list->count() - 1;
        }
    }
    // A selected Done task remains available when opened from the project board.
    if (selected < 0 && !m_selected.isEmpty()) for (const auto &task : board.tasks) {
        if (task.uuid != m_selected || (m_only->isChecked() && !TaskBoard::keptFor(task, m_host, m_session, m_sessionId))) continue;
        auto *item = new QListWidgetItem(QStringLiteral("%1 · %2\n%3").arg(task.id, TaskBoard::stageLabel(task.stage), task.text.left(240).simplified()), m_list);
        item->setData(Qt::UserRole, task.uuid); selected = m_list->count() - 1; break;
    }
    if (selected < 0 && m_list->count()) selected = 0;
    const QString uuid = selected < 0 ? QString() : m_list->item(selected)->data(Qt::UserRole).toString();
    if (uuid != m_selected) { m_selected = uuid; ++m_generation; m_detail = {}; }
    m_list->setCurrentRow(selected); m_list->verticalScrollBar()->setValue(scroll);
    const int rowHeight = m_list->count() ? m_list->sizeHintForRow(0) : 48;
    m_list->setFixedHeight(qBound(96, rowHeight * m_list->count() + 8, 220));
    m_scope->setText(m_project.isEmpty() ? tr("Open a project session to see tasks")
        : total > VisibleTasks ? tr("%1 of %2 tasks").arg(VisibleTasks).arg(total)
        : total == 1 ? tr("1 task") : tr("%1 tasks").arg(total));
    renderDetail(); if (isVisible()) fetch();
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
    m_state->setText(QStringLiteral("%1 · %2").arg(task.id, TaskBoard::stageLabel(task.stage)));
    const auto colors = TaskColors::of(m_dark);
    const auto field = [&](const QString &title, const QString &value) {
        return QStringLiteral("<p><span style='color:%1'>%2</span><br><b>%3</b></p>")
            .arg(colors.muted.name(), escaped(title), escaped(value));
    };
    QString html = field(tr("Assigned to"), task.targetSession.isEmpty() ? tr("Any session") : TaskBoard::sessionLabel(task.targetHost, task.targetSession));
    bool started = false;
    for (const auto &entry : data.value("log").toArray()) {
        const auto kind = entry.toObject().value("kind").toString();
        if (kind == QLatin1String("sent")) started = false;
        else if (kind == QLatin1String("started")) started = true;
    }
    if (!task.sessionName.isEmpty()) {
        const QString title = task.stage == QLatin1String("active")
            ? (started ? tr("Executor") : tr("Dispatched to · start unconfirmed")) : tr("Last executor");
        html += field(title, TaskBoard::sessionLabel(task.sessionHost, task.sessionName));
        if (const auto *found = executor()) html += field(tr("Session status"), found->status);
    }
    if (task.stage == QLatin1String("todo")) html += field(tr("Project queue position"), tr("#%1 · starts with Send now").arg(task.queue));
    if (!task.summary.isEmpty()) html += field(tr("Agent report"), task.summary);
    if (task.cancelledAt) html += field(tr("Cancelled"), task.cancelReason);
    html += QStringLiteral("<h3>%1</h3>").arg(escaped(tr("Description")));
    // Task markdown is untrusted. Do not load remote images from this preview.
    html += QStringLiteral("<div>%1</div>").arg(escaped(task.text).replace('\n', QStringLiteral("<br>")));
    const int files = data.value("attachments").toArray().size();
    if (files) html += field(tr("Attachments"), tr("%1 · open Edit / actions to view").arg(files == 1 ? tr("1 file") : tr("%1 files").arg(files)));
    html += QStringLiteral("<h3>%1</h3>").arg(escaped(tr("History")));
    const auto log = data.value("log").toArray();
    if (data.value("log_total").toInt() > VisibleHistory) html += QStringLiteral("<p>%1</p>").arg(escaped(tr("Latest %1 events; full history in Edit / actions.").arg(VisibleHistory)));
    for (int i = qMax(0, int(log.size()) - VisibleHistory); i < log.size(); ++i) html += taskHistoryLine(log.at(i).toObject());
    // Avoid replacing a document on unchanged polls: preserve selection and scroll.
    if (m_description->property("renderedHtml").toString() == html) return;
    const int scroll = m_description->verticalScrollBar()->value();
    m_description->setProperty("renderedHtml", html); m_description->setHtml(html);
    m_description->verticalScrollBar()->setValue(scroll);
}

void TaskInspector::setTheme(bool dark) {
    m_dark = dark; if (!m_list) return;
    const auto colors = TaskColors::of(dark);
    setWorkspaceStyle(this, QStringLiteral("QListWidget,QTextBrowser{background:%1;color:%2;border:0;}"
        "QListWidget::item{padding:8px 6px;border-radius:7px;}QListWidget::item:selected{background:%3;color:%2;}")
        .arg(colors.surface.name(), colors.fg.name(), colors.border.name()) + workspaceScrollbars(dark));
    refresh();
}
