#pragma once
#include "TasksView.h"
#include <QJsonObject>
#include <QWidget>

class QLabel;
class QPushButton;
class ActivityView;
class QSplitter;
class QStackedWidget;
class QScrollArea;
class QVBoxLayout;
class TaskTile;
class QTabBar;
class QFrame;
class TaskForm;
class QLineEdit;
class QHBoxLayout;
class QToolButton;

// Session tasks beside Activity. Shares the board's listing, but fetches only
// the selected task's history. It never infers task execution from native work.
class TaskInspector : public QWidget {
    Q_OBJECT
public:
    TaskInspector(HgsClient *client, TasksView *board, QWidget *parent = nullptr);
    void setContext(const QString &project, const QString &host, const QString &session, const QList<TaskSession> &sessions);
    void setTheme(bool dark);
    void setContentScale(double scale);
    void selectTask(const QString &uuid);
    QString selectedTask() const { return m_selected; }
    QString selectedStage() const { return m_stage; }
    int displayedCount() const { return m_rows.size(); }
signals:
    void executorRequested(const QString &host, const QString &name, const QString &id);
protected:
    void showEvent(QShowEvent *event) override;
    void hideEvent(QHideEvent *event) override;
    void resizeEvent(QResizeEvent *event) override;
    bool eventFilter(QObject *watched, QEvent *event) override;
private:
    void refresh();
    void fetch();
    void renderDetail();
    void updateFilters();
    void openInlineDetails();
    void closeInlineDetails();
    bool canLeaveDetails();
    void renderCommands(const TaskItem &task);
    QList<TaskRow *> shownRows() const;
    int dropGap(const QString &uuid, const QPoint &point) const;
    void followDrag(const TaskDrag &drag, const QPoint &point);
    void endListDrag();
    QJsonObject labels(QJsonObject task) const;
    const TaskSession *executor() const;
    HgsClient *m_client;
    TasksView *m_board;
    QString m_project, m_host, m_session, m_sessionId, m_selected, m_stage;
    QList<TaskSession> m_sessions;
    QJsonObject m_detail;
    quint64 m_request = 0, m_generation = 0;
    QLabel *m_scope, *m_state;
    QWidget *m_filters = nullptr, *m_listPage = nullptr, *m_details = nullptr;
    QScrollArea *m_scroll = nullptr;
    QVBoxLayout *m_cards = nullptr;
    QHash<QString, TaskTile *> m_tiles;
    QHash<QString, TaskRow *> m_rows;
    QPushButton *m_older;
    ActivityView *m_activity;
    QSplitter *m_splitter;
    QStackedWidget *m_content;
    QLabel *m_emptyDetail;
    QPushButton *m_moreDetail, *m_latestHistory;
    QJsonObject m_historyBefore;
    QPushButton *m_open, *m_edit;
    QPushButton *m_assignment, *m_saveNote;
    QLineEdit *m_note;
    QLabel *m_noteError;
    QWidget *m_commands;
    QHBoxLayout *m_commandLayout;
    QHash<int, QToolButton *> m_commandButtons;
    QPointer<TaskForm> m_form;
    QScrollArea *m_formScroll = nullptr;
    QString m_commandsKey, m_noteKey, m_inlineRequested;
    bool m_noteBusy = false;
    QTabBar *m_detailTabs;
    bool m_dark = true;
    bool m_showOlder = false;
    int m_historyVisible = 8;
    QFrame *m_dropLine;
    QTimer m_autoScroll;
    TaskDrag m_drag;
    QPoint m_dragAt;
    int m_scrollStep = 0;
    bool m_overList = false;
};
