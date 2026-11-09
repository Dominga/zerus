#pragma once
#include "TeamModel.h"
#include <QDialog>
class TeamController;
class TeamTaskCard;
class QListWidget;
class QTableWidget;
class QLabel;
class QPushButton;
class QStackedWidget;

// Two replaceable metadata pages and one selected full task. No history prefetch.
class TeamArchive : public QDialog {
public:
    TeamArchive(TeamController *controller, const Teams::Team &team, QWidget *parent = nullptr, int phase = 0);
    static constexpr int PageSize = 50;
private:
    void phases(int cursor);
    void tasks(int phase, int cursor);
    void openTask(int task);
    TeamController *m_controller;
    Teams::Team m_team;
    QListWidget *m_phases;
    QTableWidget *m_tasks;
    QLabel *m_state;
    QPushButton *m_nextPhases, *m_nextTasks, *m_retry;
    QStackedWidget *m_stack;
    TeamTaskCard *m_card;
    int m_phase = 0, m_phaseCursor = 0, m_taskCursor = 0;
    quint64 m_generation = 0;
    bool m_loading = false;
    std::function<void()> m_again;
};
