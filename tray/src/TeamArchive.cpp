#include "TeamArchive.h"
#include "TeamController.h"
#include "TeamTaskCard.h"
#include "TeamProfile.h"
#include <QDateTime>
#include <QHeaderView>
#include <QLabel>
#include <QListWidget>
#include <QPointer>
#include <QPushButton>
#include <QStackedWidget>
#include <QTableWidget>
#include <QVBoxLayout>

TeamArchive::TeamArchive(TeamController *controller, const Teams::Team &team, QWidget *parent, int phase)
    : QDialog(parent), m_controller(controller), m_team(team)
{
    setObjectName("teamArchive");
    setWindowTitle(tr("Archived phases: %1").arg(team.name));
    setAttribute(Qt::WA_DeleteOnClose);
    setWindowModality(Qt::WindowModal);
    resize(900, 650);
    auto *outer = new QVBoxLayout(this);
    m_stack = new QStackedWidget;
    outer->addWidget(m_stack);
    auto *page = new QWidget;
    auto *layout = new QVBoxLayout(page);
    m_state = new QLabel; m_state->setObjectName("teamArchiveState");
    m_state->setWordWrap(true); layout->addWidget(m_state);
    m_retry = new QPushButton(tr("Retry")); m_retry->hide(); layout->addWidget(m_retry);
    connect(m_retry, &QPushButton::clicked, this, [this] { if (m_again) m_again(); });
    m_phases = new QListWidget; m_phases->setObjectName("teamArchivePhases"); layout->addWidget(m_phases);
    auto *first = new QPushButton(tr("First phases")); layout->addWidget(first);
    connect(first, &QPushButton::clicked, this, [this] { phases(0); });
    m_nextPhases = new QPushButton(tr("Next phases")); m_nextPhases->setObjectName("teamArchiveNextPhases");
    layout->addWidget(m_nextPhases);
    connect(m_nextPhases, &QPushButton::clicked, this, [this] { phases(m_phaseCursor); });
    m_tasks = new QTableWidget(0, 5); m_tasks->setObjectName("teamArchiveTasks");
    m_tasks->setHorizontalHeaderLabels({tr("Task"), tr("Title"), tr("Outcome"), tr("Worked"), tr("Closed")});
    m_tasks->horizontalHeader()->setSectionResizeMode(1, QHeaderView::Stretch);
    m_tasks->setEditTriggers(QAbstractItemView::NoEditTriggers);
    layout->addWidget(m_tasks);
    m_nextTasks = new QPushButton(tr("Next tasks")); m_nextTasks->setObjectName("teamArchiveNextTasks");
    m_nextTasks->setEnabled(false); layout->addWidget(m_nextTasks);
    connect(m_nextTasks, &QPushButton::clicked, this, [this] { tasks(m_phase, m_taskCursor); });
    connect(m_phases, &QListWidget::currentRowChanged, this, [this](int row) {
        if (row >= 0) tasks(m_phases->item(row)->data(Qt::UserRole).toInt(), 0);
    });
    connect(m_tasks, &QTableWidget::cellDoubleClicked, this, [this](int row, int) {
        openTask(m_tasks->item(row, 0)->data(Qt::UserRole).toInt());
    });
    m_tasks->setToolTip(tr("Double-click a task to read its report"));
    m_stack->addWidget(page);
    m_card = new TeamTaskCard; m_card->setController(controller);
    m_card->back = [this] { m_stack->setCurrentIndex(0); m_card->setTask({}, 0, {}, true); };
    m_card->openTask = [this](int task) { openTask(task); };
    m_stack->addWidget(m_card);
    phases(qMax(0, phase - 1));
}

void TeamArchive::phases(int cursor)
{
    if (m_loading) return;
    m_loading = true;
    ++m_generation;
    m_phases->setEnabled(false);
    m_nextPhases->setEnabled(false); m_retry->hide();
    m_state->setText(tr("Reading archived phases..."));
    m_again = [this, cursor] { m_nextPhases->setEnabled(true); phases(cursor); };
    QPointer<TeamArchive> self(this);
    m_controller->call("status", {{"team", m_team.id}, {"archived_phases", true}, {"cursor", cursor}, {"limit", PageSize}},
        [self](bool ok, const QJsonObject &result, const QString &error) {
            if (!self) return;
            self->m_loading = false; self->m_phases->setEnabled(true);
            if (!ok) { self->m_state->setText(tr("Archive not read: %1").arg(error)); self->m_retry->show(); return; }
            self->m_state->setText(tr("One page at a time. Select a phase; double-click a task for its report."));
            self->m_phases->clear(); self->m_tasks->setRowCount(0);
            for (const auto &value : result.value("items").toArray()) {
                if (self->m_phases->count() == PageSize) break;
                const auto p = value.toObject(); const auto a = p.value("archived").toObject();
                const int n = p.value("n").toInt();
                auto *item = new QListWidgetItem(tr("Phase %1 / %2 / %3 accepted, %4 rejected, %5 cancelled")
                    .arg(n).arg(p.value("title").toString()).arg(a.value("accepted").toInt())
                    .arg(a.value("rejected").toInt()).arg(a.value("cancelled").toInt()), self->m_phases);
                item->setData(Qt::UserRole, n);
                item->setToolTip(QDateTime::fromSecsSinceEpoch(qint64(a.value("closed_at").toDouble())).toString(Qt::ISODate));
            }
            self->m_phaseCursor = result.value("next_cursor").toInt();
            self->m_nextPhases->setEnabled(self->m_phaseCursor > 0);
            if (self->m_phases->count()) self->m_phases->setCurrentRow(0);
        }, false);
}

void TeamArchive::tasks(int phase, int cursor)
{
    if (m_loading) return;
    m_loading = true;
    m_phases->setEnabled(false);
    m_phase = phase;
    const quint64 generation = ++m_generation;
    m_nextTasks->setEnabled(false); m_retry->hide();
    m_state->setText(tr("Reading archived tasks..."));
    m_again = [this, phase, cursor] { tasks(phase, cursor); };
    QPointer<TeamArchive> self(this);
    m_controller->call("status", {{"team", m_team.id}, {"archived_phase", phase}, {"cursor", cursor}, {"limit", PageSize}},
        [self, generation](bool ok, const QJsonObject &result, const QString &error) {
            if (!self || generation != self->m_generation) return;
            self->m_loading = false; self->m_phases->setEnabled(true);
            if (!ok) { self->m_state->setText(tr("Archive not read: %1").arg(error)); self->m_retry->show(); return; }
            self->m_tasks->setRowCount(0);
            for (const auto &value : result.value("items").toArray()) {
                const int row = self->m_tasks->rowCount(); if (row == PageSize) break;
                const auto task = value.toObject(); const auto a = task.value("archived").toObject();
                const int id = task.value("id").toInt();
                self->m_tasks->insertRow(row);
                const QStringList cells{QString::number(id), task.value("title").toString(), task.value("status").toString(),
                    TeamProfile::duration(qint64(a.value("timing").toObject().value("worked_s").toDouble())),
                    QDateTime::fromSecsSinceEpoch(qint64(a.value("accepted_at").toDouble())).toString(Qt::ISODate)};
                for (int col = 0; col < cells.size(); ++col) self->m_tasks->setItem(row, col, new QTableWidgetItem(cells[col]));
                self->m_tasks->item(row, 0)->setData(Qt::UserRole, id);
            }
            self->m_taskCursor = result.value("next_cursor").toInt();
            self->m_nextTasks->setEnabled(self->m_taskCursor > 0);
            self->m_state->setText(tr("Phase %1 / %2 tasks on this page").arg(self->m_phase).arg(self->m_tasks->rowCount()));
        }, false);
}

void TeamArchive::openTask(int task)
{
    m_card->setTask(m_team, task, {}, true);
    m_stack->setCurrentWidget(m_card);
}
