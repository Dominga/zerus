#pragma once
#include "TasksView.h"
#include <QJsonObject>
#include <QWidget>

class QCheckBox;
class QLabel;
class QPushButton;
class QTextBrowser;
class QScrollArea;
class QVBoxLayout;
class TaskTile;

// Session tasks beside Activity. Shares the board's listing, but fetches only
// the selected task's history. It never infers task execution from native work.
class TaskInspector : public QWidget {
    Q_OBJECT
public:
    TaskInspector(HgsClient *client, TasksView *board, QWidget *parent = nullptr);
    void setContext(const QString &project, const QString &host, const QString &session, const QList<TaskSession> &sessions);
    void setTheme(bool dark);
    void selectTask(const QString &uuid);
    QString selectedTask() const { return m_selected; }
    QString selectedStage() const { return m_stage; }
    int displayedCount() const { return m_rows.size(); }
signals:
    void boardRequested();
    void executorRequested(const QString &host, const QString &name, const QString &id);
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
private:
    void refresh();
    void fetch();
    void renderDetail();
    void updateFilters();
    void updateDetailHeight();
    QJsonObject labels(QJsonObject task) const;
    const TaskSession *executor() const;
    HgsClient *m_client;
    TasksView *m_board;
    QString m_project, m_host, m_session, m_sessionId, m_selected, m_stage;
    QList<TaskSession> m_sessions;
    QJsonObject m_detail;
    quint64 m_request = 0, m_generation = 0;
    QLabel *m_scope, *m_state;
    QCheckBox *m_only;
    QWidget *m_filters = nullptr, *m_listPage = nullptr, *m_expansion = nullptr;
    QScrollArea *m_scroll = nullptr;
    QVBoxLayout *m_cards = nullptr;
    QHash<QString, TaskTile *> m_tiles;
    QHash<QString, TaskRow *> m_rows;
    QPushButton *m_older;
    QTextBrowser *m_description;
    QPushButton *m_open, *m_edit;
    bool m_dark = true;
    bool m_showOlder = false;
};
